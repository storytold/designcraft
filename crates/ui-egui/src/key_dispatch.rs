//! Key dispatch: egui's key presses matched against the keymap, one call per frame before the UI reads its keys.
//!
//! craft-keymap matches one press (`Matcher`, over a `Layout`); this module is the egui half the keymap left to the app:
//! it reads a frame's events, teaches the layout what the presses typed, takes the presses that ran a command (with the
//! text they typed) out of the input, and leaves the rest for the widgets.
//!
//! On the desktop, egui-winit sends egui's Copy, Cut and Paste events in place of the key press for any chord of Cmd with
//! C, X or V (Ctrl off macOS; on Windows also Shift+Delete, Ctrl+Insert and Shift+Insert). The web build sends the key
//! press and then the event. The plain clipboard chords (Cmd+C, Cmd+X, Cmd+V, Cmd+Insert, Shift+Delete, Shift+Insert)
//! stay with the app's canvas and text tools, so bindings on them run only through the menu; an event with more modifiers
//! held counts as its chord, read from the frame's modifiers (Cmd+Shift+V), and runs that chord's binding.
//!
//! While [`Active::typing`], the text field keeps, without lookup:
//! - the plain clipboard chords' events and presses (other clipboard chords are looked up);
//! - a press with a Text event of its own in the frame, since it typed that text. This covers AltGr: the browser on
//!   Windows reports AltGr+Q (`@` on a German layout) as Ctrl+Alt+Q, and the web build's text agent sends the `@` after
//!   it. egui-winit reports AltGr with neither Ctrl nor Alt. A Ctrl+Alt press without text is a command chord.
//!
//! Other presses are looked up; the keymap's typing rule and `text_editing_chords` leave plain keys and the
//! text-editing chords (Cmd+Z, word moves) to the field.

use craft_keymap::{Active, Hit, Key, KeyInput, Keymap, Layout, Matcher, Mods, Platform, Press};
use egui::{Event, InputState, Key as EguiKey};

/// Runs a keymap against egui's input.
///
/// It reads key presses (never repeats). A press that runs a command is removed from the events, together with the text
/// it typed when it has no command modifier, so no widget sees it too. Other events stay. A chord pressed again within
/// the double-press time in the same panel runs its double binding when it has one.
#[derive(Clone, Debug)]
pub(crate) struct Dispatcher {
    matcher: Matcher,
    /// Whether a press's Text event comes before it when no text field has focus, as eframe's web build sends them.
    text_first: bool,
    layout: Layout,
    /// Whether right Alt is held. egui-winit reports AltGr with neither Ctrl nor Alt, so its presses would otherwise
    /// teach the layout as plain ones.
    right_alt: bool,
}

/// eframe's web build sends a press's Text event first when no text field has focus.
const TEXT_FIRST: bool = cfg!(target_arch = "wasm32");

impl Default for Dispatcher {
    fn default() -> Dispatcher {
        Dispatcher { matcher: Matcher::new(), text_first: TEXT_FIRST, layout: Layout::new(), right_alt: false }
    }
}

impl Dispatcher {
    pub(crate) fn new() -> Dispatcher {
        Dispatcher::default()
    }

    /// The commands this frame's key presses run with `active` focus, in press order. `accept` decides each hit before
    /// its event is removed: a declined hit is left out of the result and its event, with any text it typed, stays in
    /// egui's input in its place. A declined press still counts toward a double press.
    pub(crate) fn dispatch_filtered(
        &mut self,
        input: &mut InputState,
        keymap: &Keymap,
        active: &Active,
        mut accept: impl FnMut(&Hit) -> bool,
    ) -> Vec<Hit> {
        let platform = keymap.platform();
        let now = input.time;
        let frame_mods = Mods::from_egui(&input.modifiers, platform);
        let mut hits = Vec::new();
        let mut kept = Vec::with_capacity(input.events.len());
        let events = std::mem::take(&mut input.events);
        let text_of = text_of_press(&events, self.text_first && !active.typing);
        self.report_latin(input.keyboard_layout_is_latin());
        self.learn(&events, &text_of, platform);
        let mut drop = vec![false; events.len()];
        // The web build sends both a clipboard key press and the clipboard event it causes. The press decides: its event
        // goes with it when it runs a command and stays otherwise.
        let causes = clipboard_event_follows(&events, &self.layout, platform, input);
        let mut caused_taken: Vec<Option<bool>> = vec![None; events.len()];
        for (i, event) in events.into_iter().enumerate() {
            match &event {
                Event::Key { pressed: true, repeat: false, .. } => {
                    let input = key_input(&event, platform, input);
                    let held = input.mods;
                    if let Some(chord) = self.layout.resolve(&input) {
                        let plain_clip = is_plain_clipboard(chord.key, chord.mods);
                        let keep = if active.typing { text_of[i].is_some() || plain_clip } else { plain_clip && causes[i].is_some() };
                        if keep {
                            if let Some(j) = causes[i] {
                                caused_taken[j] = Some(false);
                            }
                            kept.push((i, event));
                            continue;
                        }
                        let hit = self.press(&input, false, now, keymap, active).filter(&mut accept);
                        if let Some(j) = causes[i] {
                            caused_taken[j] = Some(hit.is_some());
                        }
                        if let Some(hit) = hit {
                            hits.push(hit);
                            if let Some(j) = text_of[i].filter(|_| !(held.cmd || held.ctrl)) {
                                drop[j] = true;
                            }
                            continue;
                        }
                    }
                }
                // A repeat that does not run leaves its clipboard event in place with it.
                Event::Key { pressed: true, repeat: true, .. } => {
                    if let Some(j) = causes[i] {
                        caused_taken[j] = Some(false);
                    }
                }
                // Desktop clipboard events, and web ones whose press stayed or ran nothing.
                Event::Copy | Event::Cut | Event::Paste(_) => match caused_taken[i] {
                    Some(true) => continue,
                    Some(false) => {}
                    None => {
                        let (key, held, _) = clipboard_chord(&event, frame_mods);
                        // A focused text field copies and pastes on the plain chords itself.
                        let hit = if is_plain_clipboard(key, held) {
                            None
                        } else {
                            self.press(&KeyInput::new(held).with_typed(key), false, now, keymap, active)
                        };
                        if let Some(hit) = hit
                            && accept(&hit)
                        {
                            hits.push(hit);
                            continue;
                        }
                    }
                },
                _ => {}
            }
            kept.push((i, event));
        }
        input.events = kept.into_iter().filter(|(i, _)| !drop[*i]).map(|(_, e)| e).collect();
        hits
    }

    /// Takes the platform's answer on whether the layout is Latin. `None` (not reported) leaves the layout as it is, so
    /// an earlier report or typing decides.
    fn report_latin(&mut self, reported: Option<bool>) {
        if reported.is_some() {
            self.layout.set_latin(reported);
        }
    }

    /// Teaches the layout what this frame's presses typed.
    fn learn(&mut self, events: &[Event], text_of: &[Option<usize>], platform: Platform) {
        for (event, text) in events.iter().zip(text_of) {
            match event {
                Event::Key { physical_key: Some(EguiKey::AltRight), pressed, .. } => self.right_alt = *pressed,
                Event::WindowFocused(false) => self.right_alt = false,
                _ => {}
            }
            if self.right_alt {
                continue;
            }
            let Some(Event::Text(text)) = text.and_then(|j| events.get(j)) else { continue };
            if let Some(KeyInput { mods, physical: Some(physical), .. }) = KeyInput::from_egui(event, platform) {
                self.layout.observe(physical, mods, text);
            }
        }
    }

    fn press(&mut self, input: &KeyInput, repeat: bool, now: f64, keymap: &Keymap, active: &Active) -> Option<Hit> {
        self.matcher.press(&self.layout, keymap, &Press { input, repeat, now, active, outer: &[] })
    }
}

/// A clipboard event: Copy, Cut or Paste.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Clip {
    Copy,
    Cut,
    Paste,
}

/// The suite's egui fork has an inherent `InputState::keyboard_layout_is_latin`; crates.io egui has neither the method nor
/// the field. A method call picks an inherent method over a trait method of the same name, so the dispatcher reaches
/// egui's method on the fork and this `None` everywhere else.
#[allow(dead_code, reason = "unused when egui has the inherent method")]
trait KeyboardLayoutIsLatin {
    fn keyboard_layout_is_latin(&self) -> Option<bool> {
        None
    }
}

impl KeyboardLayoutIsLatin for InputState {}

/// The suite's egui fork has an inherent `InputState::latin_layout_char`, the character a key pressed this frame types on
/// the Latin layout shortcuts follow; crates.io egui reports none. Reached as [`KeyboardLayoutIsLatin`] is.
#[allow(dead_code, reason = "unused when egui has the inherent method")]
trait LatinLayoutChar {
    fn latin_layout_char(&self, _physical_key: EguiKey) -> Option<char> {
        None
    }
}

impl LatinLayoutChar for InputState {}

/// A press as [`KeyInput::from_egui`] reads it, with the Latin layout's character `input` reports for its key.
fn key_input(event: &Event, platform: Platform, input: &InputState) -> KeyInput {
    let mut key = KeyInput::from_egui(event, platform).unwrap_or_default();
    if let Event::Key { physical_key: Some(physical), .. } = event {
        key.latin = input.latin_layout_char(*physical);
    }
    key
}

/// For each clipboard key press whose clipboard event comes later in the frame, as the web build sends them: the index of
/// that event.
fn clipboard_event_follows(events: &[Event], layout: &Layout, platform: Platform, input: &InputState) -> Vec<Option<usize>> {
    let mut out = vec![None; events.len()];
    let mut ahead: [Option<usize>; 3] = [None; 3];
    for (i, event) in events.iter().enumerate().rev() {
        if let Some(seen) = event_clip(event).and_then(|c| ahead.get_mut(c as usize)) {
            *seen = Some(i);
        } else if let Event::Key { pressed: true, .. } = event {
            let chord = layout.resolve(&key_input(event, platform, input));
            let clip = chord.and_then(|c| clipboard_kind(c.key, c.mods));
            out[i] = clip.and_then(|c| ahead.get_mut(c as usize)).and_then(Option::take);
        }
    }
    out
}

/// For each key press, the index of the Text event it typed. egui-winit, and eframe's web text agent while a text field
/// has focus, send the Text after the press, before the next one. With `text_first`, eframe's web build with no text field
/// focused, the Text comes right before its press, and only for a press without Ctrl or Cmd.
fn text_of_press(events: &[Event], text_first: bool) -> Vec<Option<usize>> {
    let is_text = |e: Option<&Event>| matches!(e, Some(Event::Text(t)) if !t.is_empty());
    let mut out = vec![None; events.len()];
    if text_first {
        for (i, event) in events.iter().enumerate().skip(1) {
            if let Event::Key { pressed: true, modifiers, .. } = event
                && !(modifiers.ctrl || modifiers.command || modifiers.mac_cmd)
                && is_text(events.get(i - 1))
            {
                out[i] = Some(i - 1);
            }
        }
        return out;
    }
    let mut text_ahead = None;
    for (i, event) in events.iter().enumerate().rev() {
        match event {
            Event::Key { pressed: true, .. } => out[i] = text_ahead.take(),
            _ if is_text(Some(event)) => text_ahead = Some(i),
            _ => {}
        }
    }
    out
}

fn event_clip(event: &Event) -> Option<Clip> {
    match event {
        Event::Copy => Some(Clip::Copy),
        Event::Cut => Some(Clip::Cut),
        Event::Paste(_) => Some(Clip::Paste),
        _ => None,
    }
}

/// The clipboard event a backend sends for a press, if any.
fn clipboard_kind(key: Key, held: Mods) -> Option<Clip> {
    match key {
        Key::C if held.cmd => Some(Clip::Copy),
        Key::X if held.cmd => Some(Clip::Cut),
        Key::V if held.cmd => Some(Clip::Paste),
        Key::Delete if held.shift => Some(Clip::Cut),
        Key::Insert if held.shift => Some(Clip::Paste),
        Key::Insert if held.cmd || held.ctrl => Some(Clip::Copy),
        _ => None,
    }
}

/// Whether a chord is one of the plain clipboard chords, which egui's text fields act on: Cmd+C, Cmd+X, Cmd+V, Cmd+Insert,
/// Shift+Delete and Shift+Insert.
fn is_plain_clipboard(key: Key, held: Mods) -> bool {
    match key {
        Key::C | Key::X | Key::V => held == Mods::CMD,
        Key::Insert => held == Mods::CMD || held == Mods::SHIFT,
        Key::Delete => held == Mods::SHIFT,
        _ => false,
    }
}

/// The press a clipboard event stands for, with the modifiers held, and the event's kind. Off macOS egui-winit also sends
/// Cut for Shift+Delete and Paste for Shift+Insert.
fn clipboard_chord(event: &Event, held: Mods) -> (Key, Mods, Clip) {
    let with_cmd = Mods { cmd: true, ..held };
    let shift_only = held.shift && !held.cmd;
    match event {
        Event::Cut if shift_only => (Key::Delete, held, Clip::Cut),
        Event::Cut => (Key::X, with_cmd, Clip::Cut),
        Event::Paste(_) if shift_only => (Key::Insert, held, Clip::Paste),
        Event::Paste(_) => (Key::V, with_cmd, Clip::Paste),
        _ => (Key::C, with_cmd, Clip::Copy),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use craft_keymap::{Binding, CommandDef, Scope, Scopes};
    use egui::Modifiers;

    /// A press that ran a command takes the text it typed with it, in the web build where that text comes first.
    #[test]
    fn web_text_before_its_press_goes_with_that_press() {
        let km = Keymap::new(
            vec![CommandDef::new("tool.marquee", "Marquee", Scope::app()).with_default(Binding::parse("M").unwrap())],
            Scopes::new(),
            Platform::Windows,
        );
        let press = |key| Event::Key { key, physical_key: Some(key), pressed: true, repeat: false, modifiers: Modifiers::NONE };
        let mut input = InputState::default();
        input.events = vec![Event::Text("m".into()), press(EguiKey::M), Event::Text("q".into()), press(EguiKey::Q)];
        let mut d = Dispatcher { text_first: true, ..Dispatcher::new() };
        let hits = d.dispatch_filtered(&mut input, &km, &Active::NONE, |_| true);
        assert_eq!(hits.len(), 1);
        assert_eq!(input.events, [Event::Text("q".into()), press(EguiKey::Q)]);
    }

    /// The platform's report of a Latin layout wins over what typing showed, and a frame without a report keeps it.
    #[test]
    fn the_platforms_latin_report_overrides_typing() {
        /// Stands in for the fork's `InputState`, whose inherent method wins over the fallback.
        struct ForkInput(Option<bool>);
        impl ForkInput {
            fn keyboard_layout_is_latin(&self) -> Option<bool> {
                self.0
            }
        }
        impl KeyboardLayoutIsLatin for ForkInput {}

        let km = Keymap::new(vec![], Scopes::new(), Platform::Windows);
        let typing = |text: &str| {
            let mut input = InputState::default();
            let key = Event::Key { key: EguiKey::C, physical_key: Some(EguiKey::C), pressed: true, repeat: false, modifiers: Modifiers::NONE };
            input.events = vec![key, Event::Text(text.into())];
            input
        };

        // crates.io egui reports nothing, so typing decides.
        let mut d = Dispatcher::new();
        assert_eq!(InputState::default().keyboard_layout_is_latin(), None);
        d.dispatch_filtered(&mut typing("с"), &km, &Active::NONE, |_| true);
        assert!(!d.layout.is_latin());

        // A report wins over typing, and frames without one keep it.
        d.report_latin(ForkInput(Some(true)).keyboard_layout_is_latin());
        assert!(d.layout.is_latin());
        d.report_latin(ForkInput(None).keyboard_layout_is_latin());
        d.dispatch_filtered(&mut typing("с"), &km, &Active::NONE, |_| true);
        assert!(d.layout.is_latin());
        d.report_latin(ForkInput(Some(false)).keyboard_layout_is_latin());
        d.dispatch_filtered(&mut typing("c"), &km, &Active::NONE, |_| true);
        assert!(!d.layout.is_latin());
    }
}
