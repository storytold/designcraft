use super::invoke::activate;
use super::{DesignApp, ScreenMode, Value, json};

/// The command a paste event stands for besides plain Paste. egui-winit turns every ⌘+V chord into
/// `Event::Paste` with the clipboard's text and drops the key press, and the keymap reads the chord
/// from the frame's final modifiers, which miss a chord released within the frame; the modifiers
/// held when it went down tell them apart. `None` when the chord is another one, or the native
/// menu's accelerator takes it.
#[cfg(not(target_arch = "wasm32"))]
pub fn paste_chord_command(app: &DesignApp, modifiers: egui::Modifiers) -> Option<&'static str> {
    let id = "edit.pasteInPlace";
    if app.native_menu && app.keymap.overrides().get(id).is_none() {
        return None;
    }
    let platform = app.keymap.platform();
    let held = craft_keymap::Mods::from_egui(&modifiers, platform);
    let pressed = craft_keymap::Chord::new(held, craft_keymap::Key::V);
    // A shortcut the user set is the menu bar's key equivalent once the bar carries it.
    let on_bar = app
        .native_shortcuts
        .get(id)
        .and_then(|sc| craft_keymap::Binding::parse(sc).ok())
        .is_some_and(|b| !b.trigger.double && b.trigger.chord.effective(platform) == pressed);
    if app.native_menu && on_bar {
        return None;
    }
    let bound =
        app.keymap.bindings(&craft_keymap::CommandRef::new(id)).iter().any(|b| !b.trigger.double && b.trigger.chord.effective(platform) == pressed);
    bound.then_some(id)
}

/// Global keyboard shortcuts: menu commands, panels and tools, through the keymap.
pub fn shortcuts(app: &mut DesignApp, ctx: &egui::Context) {
    app.sync_keymap();
    // Only a focused text field takes the keys: the canvas (or a button) having focus after a click
    // must not swallow tool shortcuts until Esc clears it.
    if ctx.text_edit_focused() || keys_taken(app, ctx) {
        return;
    }
    let typing = app.session.wants_text();
    let keys = ctx.input_mut(|i| take_keys(app, i, typing));
    run_keys(app, keys, typing);
}

/// Keys that a floating panel in its own OS window left over (craft-ui's
/// `DockOutput::forwarded_events`, with the modifiers held in that window): they run the main
/// window's shortcuts, clipboard and tool keys in the frame they arrive, so the keyboard works
/// while a floating window has focus. craft-ui forwards nothing while one of that window's
/// widgets has keyboard focus.
pub fn forwarded_shortcuts(app: &mut DesignApp, ctx: &egui::Context, events: Vec<egui::Event>, modifiers: egui::Modifiers) {
    if events.is_empty() || keys_taken(app, ctx) {
        return;
    }
    app.sync_keymap();
    let typing = app.session.wants_text();
    // The keymap dispatches from an input state: the main window's, with the floating window's
    // keys and modifiers in place of its own.
    let mut input = ctx.input(|i| i.clone());
    input.events = events;
    input.modifiers = modifiers;
    let keys = take_keys(app, &mut input, typing);
    run_keys(app, keys, typing);
    crate::canvas::forwarded_keys(app, input.events, modifiers);
}

/// Whether something other than the shortcuts has the keyboard: a dialog or Quick Apply, an input
/// method composing at the text caret, an open canvas context menu (Escape closes it, as a
/// native menu would) or the menu search.
fn keys_taken(app: &DesignApp, ctx: &egui::Context) -> bool {
    app.ui.dialog.is_some()
        || app.ui.palette.is_some()
        || app.session.tool_composing()
        || crate::canvas::context_menu_open(ctx)
        || crate::chrome::search_had_focus(ctx)
}

/// The key presses [`take_keys`] took from an input, run by [`run_keys`] once the input is
/// released: commands may read the context.
struct Keys {
    /// Presentation keys in order: `None` leaves Presentation, `Some(step)` moves through the spreads.
    presentation: Vec<Option<isize>>,
    hits: Vec<craft_keymap::egui::Hit>,
    /// An Esc press that nothing took.
    escape: bool,
}

/// Takes the presentation keys and the command, panel and tool shortcuts out of `input`'s key
/// presses.
fn take_keys(app: &mut DesignApp, input: &mut egui::InputState, typing: bool) -> Keys {
    let presentation = if app.ui.screen_mode == ScreenMode::Presentation { presentation_keys(input) } else { Vec::new() };
    let contexts: &[&str] = if typing { &[crate::keys::TEXT_CONTEXT] } else { &[] };
    let active = craft_keymap::Active { panel: None, contexts, typing };
    let native = &app.native_shortcuts;
    // Presses the dispatcher leaves alone: Clear (the active tool deletes) and the keys the
    // native menu's accelerators already handled. An item holds one key equivalent, its
    // command's first shortcut in effect, and fires only for it exactly: the Plus key standing
    // in for = never reaches it, and a command's other bindings run here.
    // Copy, Cut and Paste stay too (the dispatcher's default): the canvas and text tools
    // handle them, and Paste carries the text.
    let accept = |hit: &craft_keymap::egui::Hit| {
        let c = &hit.command;
        let on_bar =
            || c.params.is_null() && native.get(&c.id).and_then(|sc| craft_keymap::Binding::parse(sc).ok()).is_some_and(|b| b.trigger == hit.trigger);
        let leave = hit.exact && hit.cycle.is_empty() && (c.id == "edit.clear" || on_bar());
        !leave
    };
    let hits = app.key_dispatcher.dispatch_filtered(input, &app.keymap, &active, accept);
    let escape = input.events.iter().any(
        |e| matches!(e, egui::Event::Key { key: egui::Key::Escape, pressed: true, repeat: false, modifiers, .. } if !modifiers.command && !modifiers.alt),
    );
    Keys { presentation, hits, escape }
}

/// Runs what [`take_keys`] took, then Esc's return to the Selection tool.
fn run_keys(app: &mut DesignApp, keys: Keys, typing: bool) {
    for step in keys.presentation {
        match step {
            None => {
                let _ = app.run("view.togglePresentation", json!({}));
            }
            Some(step) => crate::canvas::step_spread(app, step),
        }
    }
    for hit in keys.hits {
        if !hit.cycle.is_empty() {
            // Tools sharing a key: the press after the group's current tool selects the next one.
            let current = app.session.tool_id();
            let tools: Vec<&str> = hit.cycle.iter().filter_map(|c| c.params.get("tool").and_then(Value::as_str)).collect();
            let next = tools.iter().position(|t| *t == current).and_then(|i| tools.get((i + 1) % tools.len())).or(tools.first());
            if let Some(tool) = next.map(|t| t.to_string()) {
                log::debug!("tool shortcut -> {tool}");
                app.select_tool(&tool);
            }
        } else if hit.command.id == crate::keys::TOOL_COMMAND
            && let Some(tool) = hit.command.params.get("tool").and_then(Value::as_str).map(str::to_string)
        {
            log::debug!("tool shortcut -> {tool}");
            app.select_tool(&tool);
        } else {
            // Like choosing the menu item: "…" commands open their dialog.
            activate(app, &hit.command.id, &hit.command.params);
        }
    }
    // Esc with no shortcut of its own goes back to the Selection tool.
    if keys.escape && !typing && app.session.tool_id() != "selection" && !app.session.tool_busy() && app.ui.flyout.is_none() {
        app.select_tool("selection");
    }
}

/// Takes the Presentation keys out of `input`. Presentation hides every control, so Esc always
/// leaves it, and only that: the canvas must not also see the press and switch tools or deselect.
/// The arrow keys, Page Up/Down, Home and End go through the spreads.
fn presentation_keys(input: &mut egui::InputState) -> Vec<Option<isize>> {
    let mut steps = Vec::new();
    for e in input.events.clone() {
        let egui::Event::Key { key, pressed: true, modifiers, repeat: false, .. } = e else { continue };
        if key == egui::Key::Escape {
            input.consume_key(modifiers, key);
            steps.push(None);
            continue;
        }
        if !modifiers.command && !modifiers.alt {
            let step = match key {
                egui::Key::ArrowRight | egui::Key::ArrowDown | egui::Key::PageDown => Some(1),
                egui::Key::ArrowLeft | egui::Key::ArrowUp | egui::Key::PageUp => Some(-1),
                egui::Key::Home => Some(isize::MIN),
                egui::Key::End => Some(isize::MAX),
                _ => None,
            };
            if let Some(step) = step {
                input.consume_key(modifiers, key);
                steps.push(Some(step));
            }
        }
    }
    steps
}
