//! The Typography panel: the OpenType features the fonts of the selected text offer, grouped as
//! Affinity's are. Every feature is a checkbox that shows how much of the selection it is applied to
//! (checked: all of it; a dash: some of it; empty: none). A feature only some of the selection's fonts
//! offer is italic. Changes apply to the selection through `type.feature`, and Standard Ligatures
//! through `type.char`'s `ligatures` attribute, which shaping reads.

use std::collections::BTreeMap;

use serde_json::{Value, json};

use crate::DesignApp;

/// A feature's state over the selection: applied to none of it, some of it, or all of it.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
enum Covered {
    None,
    Some,
    All,
}

impl Covered {
    fn parse(s: &str) -> Self {
        match s {
            "all" => Covered::All,
            "some" => Covered::Some,
            _ => Covered::None,
        }
    }
}

/// A switch: its OpenType tag and menu label.
type Switch = (&'static str, &'static str);

/// A group of mutually exclusive choices: each one's label and the tags it turns on.
type Choice = (&'static str, &'static [&'static str]);

const LIGATURES: &[Switch] =
    &[("liga", "Standard Ligatures"), ("dlig", "Discretionary Ligatures"), ("hlig", "Historical Ligatures"), ("clig", "Contextual Ligatures")];

const ALTERNATES: &[Switch] = &[
    ("calt", "Contextual Alternates"),
    ("salt", "Stylistic Alternates"),
    ("swsh", "Swash"),
    ("titl", "Titling Alternates"),
    ("hist", "Historical Forms"),
    ("ornm", "Ornaments"),
    ("zero", "Slashed Zero"),
];

/// One axis of the figures (style or width), and the choice that partners a choice on the other
/// axis when no choice on this one applies to all of the selection.
struct FigureAxis {
    choices: &'static [Choice],
    fallback: usize,
}

const FIGURE_STYLE: FigureAxis = FigureAxis { choices: &[("Lining", &["lnum"]), ("Oldstyle", &["onum"])], fallback: 0 };

const FIGURE_WIDTH: FigureAxis = FigureAxis { choices: &[("Tabular", &["tnum"]), ("Proportional", &["pnum"])], fallback: 1 };

const FIGURE_POSITION: &[Choice] = &[
    ("Numerator", &["numr"]),
    ("Denominator", &["dnom"]),
    ("Superscript", &["sups"]),
    ("Subscript", &["subs"]),
    ("Scientific Inferiors", &["sinf"]),
];

const FRACTION_AND_ORDINAL: &[Switch] = &[("frac", "Fractions"), ("ordn", "Ordinal")];

const CAPITALS: &[Choice] = &[("Caps to Small Caps", &["c2sc"]), ("Small Caps", &["smcp"]), ("All Small Caps", &["c2sc", "smcp"])];

/// The sections of on/off switches above Other Features.
const SWITCH_SECTIONS: &[&[Switch]] = &[LIGATURES, ALTERNATES, FRACTION_AND_ORDINAL];

/// The character format's on/off features (`otf::TOGGLES`) no other section shows, so each one the
/// character style dialog and IDML carry has a control here too.
fn other_toggles() -> Vec<Switch> {
    designcraft_doc::otf::TOGGLES
        .iter()
        .filter(|(tag, ..)| !SWITCH_SECTIONS.iter().any(|section| section.iter().any(|(t, _)| t == tag)))
        .map(|(tag, label, _)| (*tag, *label))
        .collect()
}

/// What this frame's clicks set: OpenType feature tags, and the `ligatures` attribute.
#[derive(Default)]
struct Changes {
    features: BTreeMap<String, bool>,
    ligatures: Option<bool>,
}

impl Changes {
    /// Standard Ligatures is the `ligatures` attribute: shaping turns `liga` off through it, and
    /// IDML carries it as `Ligatures`, so `liga` stays out of the feature list. The tag is also
    /// cleared from the selection's features, since an earlier `-liga` would keep shaping it off.
    fn set(&mut self, tag: &str, on: bool) {
        if tag == "liga" {
            self.ligatures = Some(on);
            self.features.insert("liga".to_string(), true);
        } else {
            self.features.insert(tag.to_string(), on);
        }
    }

    fn apply(self, app: &mut DesignApp) {
        if !self.features.is_empty() {
            let _ = app.run("type.feature", json!({"features": self.features}));
        }
        if let Some(on) = self.ligatures {
            let _ = app.run("type.char", json!({"attrs": {"ligatures": on}}));
        }
    }
}

/// A feature the selection's fonts offer, and its state over the selection.
#[derive(Clone, Copy)]
struct Feature {
    applied: Covered,
    available: Covered,
}

/// What the selection's fonts offer: each feature's state, and each stylistic set's name and state.
#[derive(Default)]
struct Offered {
    features: BTreeMap<String, Feature>,
    sets: Vec<Set>,
}

struct Set {
    number: u32,
    tag: String,
    name: Option<String>,
    feature: Feature,
}

impl Offered {
    fn get(&self, tag: &str) -> Option<Feature> {
        self.features.get(tag).copied()
    }

    /// A choice's state: all of its tags applied is all, none is none, otherwise some. Its
    /// availability is the least of its tags'. None when no tag is offered.
    fn choice(&self, tags: &[&str]) -> Option<Feature> {
        let found: Vec<Feature> = tags.iter().filter_map(|t| self.get(t)).collect();
        if found.is_empty() {
            return None;
        }
        let applied = if found.iter().all(|f| f.applied == Covered::All) {
            Covered::All
        } else if found.iter().all(|f| f.applied == Covered::None) {
            Covered::None
        } else {
            Covered::Some
        };
        let available = found.iter().map(|f| f.available).min().unwrap_or(Covered::None);
        Some(Feature { applied, available })
    }
}

/// The app owns feature/selection policy; the installed SDK owns the control's input and focus.
struct Controls {
    framework: bool,
    identity: Option<(u64, u64, String)>,
}

impl Controls {
    fn new(app: &DesignApp) -> Self {
        #[cfg(not(target_arch = "wasm32"))]
        let framework = app.framework_installed;
        #[cfg(target_arch = "wasm32")]
        let framework = false;
        Self {
            framework,
            // Text ranges, frames and table-cell selections are distinct edit targets, including
            // two ranges in the same story. A held action must not follow a changed target.
            identity: app.session.active().map(|st| (st.uid, st.identity_epoch, format!("{:?}", st.selection))),
        }
    }
}

pub fn show(app: &mut DesignApp, ui: &mut egui::Ui) {
    #[cfg(not(target_arch = "wasm32"))]
    if app.framework_installed {
        use craft_ui_widgets::runtime::env::{Direction, with_env};

        let direction = if crate::i18n::is_rtl(&app.ui.language) { Direction::Rtl } else { Direction::Ltr };
        with_env(ui, |scope| scope.direction = Some(direction), |ui| show_contents(app, ui));
        return;
    }
    show_contents(app, ui);
}

fn show_contents(app: &mut DesignApp, ui: &mut egui::Ui) {
    if super::text_attrs(app).is_none() {
        crate::rtl::label(ui, crate::i18n::tr(&app.ui.language, "Select text or a text frame."));
        return;
    }
    let offered = offered(app);
    if offered.features.is_empty() && offered.sets.is_empty() {
        crate::rtl::label(ui, crate::i18n::tr(&app.ui.language, "The selected fonts have no OpenType features."));
        return;
    }
    let controls = Controls::new(app);
    let lang = app.ui.language.clone();
    let mut changes = Changes::default();
    crate::rtl::label(ui, egui::RichText::new(crate::i18n::tr(&lang, "Typography")).strong());
    egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
        section(ui, &lang, "Ligatures", |ui| switches(&controls, ui, &lang, &offered, LIGATURES, &mut changes));
        section(ui, &lang, "Alternates", |ui| switches(&controls, ui, &lang, &offered, ALTERNATES, &mut changes));
        section(ui, &lang, "Stylistic Sets", |ui| stylistic_sets(&controls, ui, &lang, &offered, &mut changes));
        section(ui, &lang, "Figure Style", |ui| figures(&controls, ui, &lang, &offered, &FIGURE_STYLE, &FIGURE_WIDTH, &mut changes));
        section(ui, &lang, "Figure Width", |ui| figures(&controls, ui, &lang, &offered, &FIGURE_WIDTH, &FIGURE_STYLE, &mut changes));
        section(ui, &lang, "Figure Position", |ui| {
            choices(&controls, ui, &lang, &offered, FIGURE_POSITION, &mut changes);
            switches(&controls, ui, &lang, &offered, FRACTION_AND_ORDINAL, &mut changes);
        });
        section(ui, &lang, "Capitals", |ui| choices(&controls, ui, &lang, &offered, CAPITALS, &mut changes));
        section(ui, &lang, "Other Features", |ui| switches(&controls, ui, &lang, &offered, &other_toggles(), &mut changes));
    });
    changes.apply(app);
}

/// A collapsible section, open at first. Its body runs only when it has something to show.
fn section(ui: &mut egui::Ui, lang: &str, title: &str, body: impl FnOnce(&mut egui::Ui)) {
    egui::CollapsingHeader::new(crate::i18n::tr(lang, title)).id_salt(title).default_open(true).show(ui, body);
}

/// A checkbox for a feature's state: checked when all of the selection has it, a dash when some of
/// it does, italic when only some of the selection's fonts offer it. Returns the state to set when
/// clicked.
fn tri_checkbox(controls: &Controls, ui: &mut egui::Ui, key: &str, text: &str, feature: Feature) -> Option<bool> {
    #[cfg(not(target_arch = "wasm32"))]
    if controls.framework {
        use craft_behavior::Mixed;
        use craft_ui_widgets::choice::Checkbox;
        use craft_ui_widgets::contract::{Bind, FieldLayout, LabelKind, Props as _, Show as _, ShowContainer as _, SideLabel};
        use craft_ui_widgets::display::Text;
        use craft_ui_widgets::layout_components::Row;

        let value = match feature.applied {
            Covered::Some => Mixed::Mixed,
            Covered::All => Mixed::Same(true),
            Covered::None => Mixed::Same(false),
        };
        if feature.available == Covered::Some {
            // Public label content preserves italics without changing the complete accessible
            // name or creating a second click/focus handler for the label.
            return Row::new()
                .id_salt(("typography-partial", &controls.identity, key))
                .show(ui, |ui, cx| {
                    cx.labelled_with(
                        ui,
                        key,
                        text.to_owned().into(),
                        SideLabel::new().kind(LabelKind::Hidden).layout(FieldLayout::After),
                        Checkbox::new(Bind::given(value)),
                        Text::new(text.to_owned()).italic(),
                    )
                    .events
                    .committed()
                    .copied()
                })
                .inner
                .flatten();
        }
        return Checkbox::new(Bind::given(value))
            .id_salt(("typography-feature", &controls.identity, key))
            .label(text.to_owned())
            .show(ui)
            .events
            .committed()
            .copied();
    }
    #[cfg(target_arch = "wasm32")]
    let _ = (controls.framework, &controls.identity, key);
    let mut on = feature.applied == Covered::All;
    let mut label = egui::RichText::new(text);
    if feature.available == Covered::Some {
        label = label.italics();
    }
    let response = ui.add(egui::Checkbox::new(&mut on, crate::rtl::widget(ui, label)).indeterminate(feature.applied == Covered::Some));
    response.clicked().then_some(feature.applied != Covered::All)
}

/// On/off switches the fonts offer.
fn switches(controls: &Controls, ui: &mut egui::Ui, lang: &str, offered: &Offered, items: &[Switch], changes: &mut Changes) {
    for (tag, label) in items {
        let Some(feature) = offered.get(tag) else { continue };
        if let Some(on) = tri_checkbox(controls, ui, tag, crate::i18n::tr(lang, label), feature) {
            changes.set(tag, on);
        }
    }
}

/// Mutually exclusive choices, as checkboxes: choosing one turns the others off. Choosing a checked
/// one turns it off.
fn choices(controls: &Controls, ui: &mut egui::Ui, lang: &str, offered: &Offered, items: &[Choice], changes: &mut Changes) {
    for (i, (label, tags)) in items.iter().enumerate() {
        let Some(feature) = offered.choice(tags) else { continue };
        let Some(on) = tri_checkbox(controls, ui, label, crate::i18n::tr(lang, label), feature) else { continue };
        for t in *tags {
            changes.set(t, on);
        }
        if on {
            for (j, (_, others)) in items.iter().enumerate() {
                if j == i {
                    continue;
                }
                for t in others.iter().filter(|t| !tags.contains(t)) {
                    changes.set(t, false);
                }
            }
        }
    }
}

/// A figure style or width, as checkboxes like `choices`.
fn figures(controls: &Controls, ui: &mut egui::Ui, lang: &str, offered: &Offered, axis: &FigureAxis, other: &FigureAxis, changes: &mut Changes) {
    for (i, (label, tags)) in axis.choices.iter().enumerate() {
        let Some(feature) = offered.choice(tags) else { continue };
        if let Some(on) = tri_checkbox(controls, ui, label, crate::i18n::tr(lang, label), feature) {
            figure_change(offered, axis, other, i, on, changes);
        }
    }
}

/// The changes turning `choice` on `axis` on or off makes. IDML's figure styles pair a style with a
/// width, and a style or width alone is the default figures there, so turning one on also sets the
/// other axis: the choice all of the selection has, else that axis's fallback. Turning one off
/// clears both axes (the font's default figures).
fn figure_change(offered: &Offered, axis: &FigureAxis, other: &FigureAxis, choice: usize, on: bool, changes: &mut Changes) {
    for (_, tags) in axis.choices.iter().chain(other.choices) {
        for t in *tags {
            changes.set(t, false);
        }
    }
    let Some((_, tags)) = axis.choices.get(choice).filter(|_| on) else { return };
    let partner = other
        .choices
        .iter()
        .find(|(_, t)| offered.choice(t).is_some_and(|f| f.applied == Covered::All))
        .or_else(|| other.choices.get(other.fallback))
        .map_or(&[][..], |(_, t)| *t);
    for t in tags.iter().chain(partner) {
        changes.set(t, true);
    }
}

/// Stylistic sets by the name their fonts give them, or the generic `Stylistic Set NN` when the
/// selection's fonts name them differently.
fn stylistic_sets(controls: &Controls, ui: &mut egui::Ui, lang: &str, offered: &Offered, changes: &mut Changes) {
    for set in &offered.sets {
        let label = match &set.name {
            Some(name) => format!("{name} ({})", set.tag),
            None => format!("{} {:02}", crate::i18n::tr(lang, "Stylistic Set"), set.number),
        };
        if let Some(on) = tri_checkbox(controls, ui, &set.tag, &label, set.feature) {
            changes.set(&set.tag, on);
        }
    }
}

/// The features the selection's fonts offer, with their state over the selection.
fn offered(app: &mut DesignApp) -> Offered {
    let v = app.session.execute("type.openTypeFeatures", &json!({})).unwrap_or(Value::Null);
    let mut out = Offered::default();
    for f in v["features"].as_array().into_iter().flatten() {
        let (Some(tag), Some(applied), Some(available)) = (f["tag"].as_str(), f["applied"].as_str(), f["available"].as_str()) else { continue };
        out.features.insert(tag.to_string(), Feature { applied: Covered::parse(applied), available: Covered::parse(available) });
    }
    for s in v["stylisticSets"].as_array().into_iter().flatten() {
        let (Some(number), Some(tag), Some(applied), Some(available)) =
            (s["set"].as_u64(), s["tag"].as_str(), s["applied"].as_str(), s["available"].as_str())
        else {
            continue;
        };
        let Ok(number) = u32::try_from(number) else { continue };
        let feature = Feature { applied: Covered::parse(applied), available: Covered::parse(available) };
        out.features.insert(tag.to_string(), feature);
        out.sets.push(Set { number, tag: tag.to_string(), name: s["name"].as_str().map(str::to_string), feature });
    }
    out
}

#[cfg(test)]
mod tests {
    use std::io::Read;

    use egui::vec2;
    use egui_kittest::{Harness, kittest::Queryable};

    use super::*;

    /// A Typography panel over "office" (all selected), its fonts installed before the first frame.
    fn harness() -> Harness<'static, DesignApp> {
        let mut app = DesignApp::new(designcraft_engine::Session::new(), crate::Services::default());
        app.run("file.new", json!({})).unwrap();
        app.run("frame.create", json!({"rect": [72, 72, 300, 200], "content": "text", "text": "office 1024", "caret": true})).unwrap();
        app.run("edit.selectAll", json!({})).unwrap();
        let mut h = Harness::builder().with_size(vec2(320.0, 1400.0)).build_ui_state(
            |ui, app: &mut DesignApp| {
                let id = egui::Id::unique("test_fonts");
                if ui.data(|d| d.get_temp::<bool>(id)).is_none() {
                    crate::theme::install_fonts(ui.ctx(), "");
                    ui.data_mut(|d| d.insert_temp(id, true));
                    return;
                }
                show(app, ui);
            },
            app,
        );
        h.run_steps(3);
        h
    }

    fn click(h: &mut Harness<'static, DesignApp>, label: &str) {
        h.get_by_label(label).click();
        h.run_steps(2);
    }

    fn chars(h: &mut Harness<'static, DesignApp>) -> Value {
        super::super::text_attrs(h.state_mut()).unwrap()["chars"].clone()
    }

    /// Every `OTFFigureStyle` the stories' IDML export writes is `expected`.
    fn assert_idml_figures(h: &Harness<'static, DesignApp>, expected: &str, why: &str) {
        let idml = designcraft_idml::export_idml(&h.state().session.active().unwrap().doc);
        let mut z = zip::ZipArchive::new(std::io::Cursor::new(idml)).unwrap();
        let mut stories = String::new();
        for i in 0..z.len() {
            let mut f = z.by_index(i).unwrap();
            if f.name().starts_with("Stories/") {
                f.read_to_string(&mut stories).unwrap();
            }
        }
        let found: Vec<&str> = stories.split("OTFFigureStyle=\"").skip(1).filter_map(|s| s.split('"').next()).collect();
        assert!(!found.is_empty() && found.iter().all(|f| *f == expected), "{why}: {found:?}");
    }

    #[test]
    fn every_character_format_toggle_has_a_control() {
        let mut tags: Vec<&str> = SWITCH_SECTIONS.iter().flat_map(|s| s.iter().map(|(t, _)| *t)).collect();
        tags.extend(other_toggles().iter().map(|(t, _)| *t));
        for (tag, ..) in designcraft_doc::otf::TOGGLES {
            assert!(tags.contains(tag), "{tag} has no Typography panel control");
        }
        assert_eq!(tags.len(), tags.iter().collect::<std::collections::BTreeSet<_>>().len(), "a feature shown twice: {tags:?}");
        // Source Serif 4 offers `mark`, one of the toggles only Other Features shows.
        let mut h = harness();
        click(&mut h, "Mark Positioning");
        assert_eq!(chars(&mut h)["otfFeatures"], json!(["-mark"]));
    }

    #[test]
    fn a_figure_style_or_width_chosen_alone_exports_as_a_figure_style() {
        let mut h = harness();
        click(&mut h, "Lining");
        assert_idml_figures(&h, "ProportionalLining", "Lining takes the fallback width");
        click(&mut h, "Tabular");
        assert_idml_figures(&h, "TabularLining", "Tabular keeps the applied style");
        click(&mut h, "Oldstyle");
        assert_idml_figures(&h, "TabularOldstyle", "Oldstyle keeps the applied width");
        click(&mut h, "Oldstyle");
        assert_idml_figures(&h, "Default", "turning a choice off clears both axes");
        assert!(!["tnum", "pnum", "lnum", "onum"].iter().any(|t| chars(&mut h)["otfFeatures"].as_array().unwrap().contains(&json!(t))));
    }

    #[test]
    fn standard_ligatures_switch_the_ligatures_attribute() {
        let mut h = harness();
        click(&mut h, "Standard Ligatures");
        let c = chars(&mut h);
        assert_eq!(c["ligatures"], false, "{c}");
        assert!(!c["otfFeatures"].as_array().is_some_and(|l| l.iter().any(|t| t.as_str().is_some_and(|t| t.ends_with("liga")))), "{c}");
        click(&mut h, "Standard Ligatures");
        assert_eq!(chars(&mut h)["ligatures"], true, "the switch turns ligatures back on");
    }

    #[test]
    fn standard_ligatures_switch_on_clears_a_feature_list_liga_off() {
        let mut h = harness();
        h.state_mut().run("type.feature", json!({"features": {"liga": false}})).unwrap();
        assert_eq!(chars(&mut h)["otfFeatures"], json!(["-liga"]));
        h.run_steps(2);
        click(&mut h, "Standard Ligatures");
        let c = chars(&mut h);
        assert_eq!(c["ligatures"], true, "{c}");
        assert!(!c["otfFeatures"].as_array().is_some_and(|l| l.iter().any(|t| t.as_str().is_some_and(|t| t.ends_with("liga")))), "{c}");
    }
}

#[cfg(all(test, not(target_arch = "wasm32")))]
mod framework_controls_tests {
    use super::*;
    use craft_ui_widgets::runtime::inspect::{Element, Recorded};
    use egui::{Event, Key, Modifiers, PointerButton, vec2};
    use egui_kittest::Harness;
    use egui_kittest::kittest::{NodeT, Queryable};
    use std::sync::Arc;

    struct State {
        app: DesignApp,
        recorded: Option<Arc<Recorded>>,
    }

    fn text_app() -> (DesignApp, Value) {
        let mut app = DesignApp::new(designcraft_engine::Session::new(), crate::Services::default());
        app.run("file.new", json!({})).unwrap();
        let frame = app.run("frame.create", json!({"rect": [72, 72, 300, 200], "content": "text", "text": "office 1024", "caret": true})).unwrap();
        app.run("edit.selectAll", json!({})).unwrap();
        // These exact existing bundled faces need neither an OS install nor network/font fixtures.
        app.run("type.char", json!({"attrs": {"fontFamily": "Source Serif 4", "fontStyle": "Regular"}})).unwrap();
        (app, frame["story"].clone())
    }

    /// A real already-installed Arabic face is a native fixture precondition. Retain the app's
    /// existing definitions and baseline alignment; never replace missing Arabic with tofu.
    fn install_arabic_fixture(ctx: &egui::Context, app: &mut DesignApp) {
        #[cfg(target_os = "macos")]
        let family = "Geeza Pro";
        #[cfg(target_os = "windows")]
        let family = "Segoe UI";
        #[cfg(not(any(target_os = "macos", target_os = "windows")))]
        let family = "DejaVu Sans";
        let db = designcraft_fonts::FontDb::global();
        assert!(db.has_family(family), "the native Arabic fixture requires already-installed {family}; do not skip Arabic or substitute tofu");
        let face = db.face(family, "Regular");
        assert!(face.family.eq_ignore_ascii_case(family), "the named fixture must resolve to {family}, not {}", face.family);
        assert!(!face.is_placeholder() && face.covers('\u{062A}'), "the Arabic face must include the reported missing real U+062A glyph");
        let language = app.ui.language.clone();
        let mut labels: Vec<String> = [
            "Select text or a text frame.",
            "The selected fonts have no OpenType features.",
            "Typography",
            "Ligatures",
            "Alternates",
            "Stylistic Sets",
            "Figure Style",
            "Figure Width",
            "Figure Position",
            "Capitals",
            "Other Features",
            "Stylistic Set",
        ]
        .iter()
        .map(|key| crate::i18n::tr(&language, key).to_owned())
        .collect();
        for section in SWITCH_SECTIONS {
            labels.extend(section.iter().map(|(_, key)| crate::i18n::tr(&language, key).to_owned()));
        }
        labels.extend(other_toggles().iter().map(|(_, key)| crate::i18n::tr(&language, key).to_owned()));
        for choices in [FIGURE_STYLE.choices, FIGURE_WIDTH.choices, FIGURE_POSITION, CAPITALS] {
            labels.extend(choices.iter().map(|(key, _)| crate::i18n::tr(&language, key).to_owned()));
        }
        // Include actual font-supplied set names, not another panel's fixed label list.
        for set in offered(app).sets {
            labels.push(match set.name {
                Some(name) => format!("{name} ({})", set.tag),
                None => format!("{} {:02}", crate::i18n::tr(&language, "Stylistic Set"), set.number),
            });
        }
        let mut checked = std::collections::BTreeSet::new();
        for label in &labels {
            for c in label.chars().filter(|c| designcraft_fonts::is_rtl(*c) || ('\u{0600}'..='\u{06ff}').contains(c)) {
                assert!(face.covers(c), "{family} lacks real Typography glyph U+{:04X} from {label}", u32::from(c));
                checked.insert(c);
            }
        }
        assert!(!checked.is_empty(), "the fixture must cover actual Arabic Typography labels");
        let mut definitions = ctx.fonts(|fonts| fonts.definitions().clone());
        let fallback = crate::theme::SystemFont::new("Arab", face.family.clone(), face.data().to_vec(), face.index());
        crate::theme::add_system_fallbacks(&mut definitions, &[fallback], &language);
        ctx.set_fonts(definitions);
        eprintln!(
            "Typography Arabic fixture: real existing {} {}, face {}, {} font bytes, {} unique Arabic Typography glyphs explicitly covered",
            face.family,
            face.style,
            face.index(),
            face.data().len(),
            checked.len()
        );
    }

    fn harness(mut app: DesignApp, installed: bool) -> Harness<'static, State> {
        let ctx = egui::Context::default();
        crate::theme::install_fonts(&ctx, "");
        if installed {
            app.framework_opacity = true;
            app.install_framework(&ctx);
            assert!(app.framework_installed);
        }
        app.raw_input_hook(&ctx, &mut egui::RawInput::default());
        let mut locale_fonts_ready = false;
        let mut h = Harness::builder().with_context(ctx).with_size(vec2(360.0, 1400.0)).build_ui_state(
            move |ui, state: &mut State| {
                if !locale_fonts_ready {
                    locale_fonts_ready = true;
                    if state.app.ui.language == "ar" {
                        install_arabic_fixture(ui.ctx(), &mut state.app);
                        return;
                    }
                }
                let region = ui.scope_id();
                let (_, recorded) = craft_ui_widgets::runtime::inspect::record(ui, region, |ui| show(&mut state.app, ui));
                if recorded.is_some() {
                    state.recorded = recorded;
                }
            },
            State { app, recorded: None },
        );
        h.input_mut().max_texture_side = Some(8192);
        steps(&mut h, 3);
        h
    }

    fn steps(h: &mut Harness<'_, State>, count: usize) {
        for _ in 0..count {
            let mut raw = std::mem::take(h.input_mut());
            let ctx = h.ctx.clone();
            h.state_mut().app.raw_input_hook(&ctx, &mut raw);
            *h.input_mut() = raw;
            h.step();
        }
    }

    fn event(h: &mut Harness<'_, State>, event: Event) {
        h.input_mut().events.push(event);
        steps(h, 1);
    }

    fn key(h: &mut Harness<'_, State>, key: Key) {
        for pressed in [true, false] {
            event(h, Event::Key { key, physical_key: None, pressed, repeat: false, modifiers: Modifiers::NONE });
        }
        steps(h, 2);
    }

    fn pointer(h: &mut Harness<'_, State>, pos: egui::Pos2, pressed: bool) {
        event(h, Event::PointerMoved(pos));
        event(h, Event::PointerButton { pos, button: PointerButton::Primary, pressed, modifiers: Modifiers::NONE });
    }

    fn label(h: &Harness<'_, State>, english: &str) -> String {
        crate::i18n::tr(&h.state().app.ui.language, english).to_owned()
    }

    fn named_element(h: &Harness<'_, State>, component: &str, name: &str) -> Element {
        let found: Vec<_> = h
            .state()
            .recorded
            .as_ref()
            .unwrap()
            .elements
            .iter()
            .filter(|e| e.component == component && e.facts.as_ref().is_some_and(|f| f.name.as_deref() == Some(name)))
            .cloned()
            .collect();
        assert_eq!(found.len(), 1, "one real {component} named {name}: {found:?}");
        found[0].clone()
    }

    fn click_name(h: &mut Harness<'_, State>, name: &str) {
        let target = named_element(h, "Checkbox", name);
        let pos = target.rect.center();
        pointer(h, pos, true);
        pointer(h, pos, false);
        steps(h, 2);
    }

    fn click(h: &mut Harness<'_, State>, english: &str) {
        let name = label(h, english);
        click_name(h, &name);
    }

    fn tab_to(h: &mut Harness<'_, State>, english: &str) {
        let name = label(h, english);
        let native = h.state().recorded.as_ref().unwrap().elements.iter().filter(|e| e.stop.is_some()).count();
        let headers = h.query_all_by(|n| n.role() == egui::accesskit::Role::Button).count();
        for _ in 0..=native + headers {
            if h.query_by(|n| n.role() == egui::accesskit::Role::CheckBox && n.label().as_deref() == Some(name.as_str()))
                .is_some_and(|n| n.is_focused())
            {
                return;
            }
            key(h, Key::Tab);
        }
        panic!("natural Tab did not reach {name} in the current control cycle");
    }

    fn toggled(h: &Harness<'_, State>, english: &str) -> Option<egui::accesskit::Toggled> {
        let name = label(h, english);
        h.get_by(|n| n.role() == egui::accesskit::Role::CheckBox && n.label().as_deref() == Some(name.as_str())).accesskit_node().toggled()
    }

    fn chars(h: &mut Harness<'_, State>) -> Value {
        super::super::text_attrs(&mut h.state_mut().app).unwrap()["chars"].clone()
    }

    fn offers(h: &mut Harness<'_, State>) -> Value {
        h.state_mut().app.session.execute("type.openTypeFeatures", &json!({})).unwrap()
    }

    fn coverage(h: &mut Harness<'_, State>, tag: &str) -> Value {
        let value = offers(h);
        value["features"].as_array().unwrap().iter().find(|f| f["tag"] == tag).unwrap().clone()
    }

    fn calls(h: &Harness<'_, State>, from: usize) -> Vec<(String, Value)> {
        h.state().app.session.journal[from..].iter().filter(|(id, _)| matches!(id.as_str(), "type.feature" | "type.char")).cloned().collect()
    }

    fn undo_len(h: &Harness<'_, State>) -> usize {
        h.state().app.session.active().unwrap().history.undo.len()
    }

    fn undo(h: &mut Harness<'_, State>) {
        h.state_mut().app.run("edit.undo", json!({})).unwrap();
        steps(h, 2);
    }

    fn select(h: &mut Harness<'_, State>, story: &Value, anchor: usize, focus: usize) {
        h.state_mut().app.run("text.select", json!({"story": story, "anchor": anchor, "focus": focus})).unwrap();
        steps(h, 2);
    }

    fn feature_call(features: Value) -> (String, Value) {
        ("type.feature".to_owned(), json!({"features": features}))
    }

    #[test]
    fn installed_switches_toggle_by_pointer_and_natural_space_with_independent_undo_and_no_replay() {
        let (app, _) = text_app();
        let mut h = harness(app, true);
        let before = chars(&mut h);
        let start = h.state().app.session.journal.len();
        let history = undo_len(&h);
        assert_eq!(toggled(&h, "Mark Positioning"), Some(egui::accesskit::Toggled::True));
        click(&mut h, "Mark Positioning");
        assert_eq!(calls(&h, start), [feature_call(json!({"mark": false}))]);
        assert_eq!(coverage(&mut h, "mark")["applied"], "none");
        assert_eq!(undo_len(&h), history + 1);
        steps(&mut h, 3);
        assert_eq!(calls(&h, start).len(), 1);
        let off = chars(&mut h);
        tab_to(&mut h, "Mark Positioning");
        let start = h.state().app.session.journal.len();
        key(&mut h, Key::Space);
        assert_eq!(calls(&h, start), [feature_call(json!({"mark": true}))]);
        assert_eq!(coverage(&mut h, "mark")["applied"], "all");
        assert_eq!(undo_len(&h), history + 2);
        steps(&mut h, 3);
        assert_eq!(calls(&h, start).len(), 1);
        undo(&mut h);
        assert_eq!(chars(&mut h), off);
        undo(&mut h);
        assert_eq!(chars(&mut h), before);
    }

    #[test]
    fn installed_standard_ligatures_preserve_two_commands_and_clear_stale_liga_with_separate_undo() {
        for stale in [false, true] {
            let (mut app, _) = text_app();
            if stale {
                app.run("type.feature", json!({"features": {"liga": false}})).unwrap();
            }
            let mut h = harness(app, true);
            let before = chars(&mut h);
            let history = undo_len(&h);
            let start = h.state().app.session.journal.len();
            click(&mut h, "Standard Ligatures");
            let on = stale;
            assert_eq!(calls(&h, start), [feature_call(json!({"liga": true})), ("type.char".to_owned(), json!({"attrs": {"ligatures": on}}))]);
            assert_eq!(chars(&mut h)["ligatures"], on);
            assert!(!chars(&mut h)["otfFeatures"].as_array().unwrap().iter().any(|v| v.as_str().is_some_and(|s| s.ends_with("liga"))));
            assert_eq!(undo_len(&h), history + 2, "the original command grouping is not collapsed");
            steps(&mut h, 3);
            assert_eq!(calls(&h, start).len(), 2);
            undo(&mut h);
            assert_eq!(chars(&mut h)["ligatures"], before["ligatures"]);
            undo(&mut h);
            assert_eq!(chars(&mut h), before, "Undo restores the original -liga when one existed");
        }
    }

    #[test]
    fn installed_figure_axes_pair_keep_the_partner_and_deselect_to_default() {
        let (app, _) = text_app();
        let mut h = harness(app, true);
        let actions = [
            ("Lining", json!({"lnum": true, "onum": false, "tnum": false, "pnum": true})),
            ("Tabular", json!({"lnum": true, "onum": false, "tnum": true, "pnum": false})),
            ("Oldstyle", json!({"lnum": false, "onum": true, "tnum": true, "pnum": false})),
            ("Oldstyle", json!({"lnum": false, "onum": false, "tnum": false, "pnum": false})),
        ];
        let mut checkpoints = Vec::new();
        for (name, expected) in actions {
            checkpoints.push(chars(&mut h));
            let start = h.state().app.session.journal.len();
            let history = undo_len(&h);
            click(&mut h, name);
            assert_eq!(calls(&h, start), [feature_call(expected.clone())]);
            for (tag, on) in expected.as_object().unwrap() {
                assert_eq!(coverage(&mut h, tag)["applied"], if on.as_bool().unwrap() { "all" } else { "none" });
            }
            assert_eq!(undo_len(&h), history + 1);
            steps(&mut h, 3);
            assert_eq!(calls(&h, start).len(), 1);
        }
        for before in checkpoints.into_iter().rev() {
            undo(&mut h);
            assert_eq!(chars(&mut h), before);
        }
    }

    #[test]
    fn installed_capital_and_position_choices_are_exclusive_but_can_be_deselected() {
        let (app, _) = text_app();
        let mut h = harness(app, true);
        for (name, expected) in [
            ("Small Caps", json!({"c2sc": false, "smcp": true})),
            ("Caps to Small Caps", json!({"c2sc": true, "smcp": false})),
            ("All Small Caps", json!({"c2sc": true, "smcp": true})),
            ("All Small Caps", json!({"c2sc": false, "smcp": false})),
            ("Superscript", json!({"numr": false, "dnom": false, "sups": true, "subs": false, "sinf": false})),
            ("Subscript", json!({"numr": false, "dnom": false, "sups": false, "subs": true, "sinf": false})),
            ("Subscript", json!({"subs": false})),
        ] {
            let before = chars(&mut h);
            let start = h.state().app.session.journal.len();
            let history = undo_len(&h);
            click(&mut h, name);
            assert_eq!(calls(&h, start), [feature_call(expected.clone())]);
            for (tag, on) in expected.as_object().unwrap() {
                assert_eq!(coverage(&mut h, tag)["applied"], if on.as_bool().unwrap() { "all" } else { "none" });
            }
            assert_eq!(undo_len(&h), history + 1);
            // Check each transition's own Undo, then reapply through the actual control for the next one.
            undo(&mut h);
            assert_eq!(chars(&mut h), before);
            click(&mut h, name);
        }
    }

    #[test]
    fn installed_named_stylistic_set_preserves_the_font_name_and_exact_feature_target() {
        let (app, _) = text_app();
        let mut h = harness(app, true);
        let available = offers(&mut h);
        let set = available["stylisticSets"].as_array().unwrap().iter().find(|s| s["tag"] == "ss01").unwrap();
        let name = format!("{} ({})", set["name"].as_str().expect("the exact bundled Source Serif 4 names ss01"), set["tag"].as_str().unwrap());
        assert_eq!(set["applied"], "none");
        let before = chars(&mut h);
        let start = h.state().app.session.journal.len();
        let history = undo_len(&h);
        click_name(&mut h, &name);
        assert_eq!(calls(&h, start), [feature_call(json!({"ss01": true}))]);
        assert!(chars(&mut h)["otfFeatures"].as_array().unwrap().contains(&json!("ss01")));
        assert_eq!(undo_len(&h), history + 1);
        undo(&mut h);
        assert_eq!(chars(&mut h), before);
    }

    #[test]
    fn installed_mixed_coverage_and_partial_font_availability_keep_real_labels_and_control_input() {
        let (mut app, story) = text_app();
        app.run("text.select", json!({"story": story, "anchor": 0, "focus": 6})).unwrap();
        app.run("type.char", json!({"attrs": {"fontFamily": "Source Sans 3", "fontStyle": "Regular"}})).unwrap();
        app.run("type.feature", json!({"features": {"mark": false}})).unwrap();
        app.run("text.select", json!({"story": story, "anchor": 0, "focus": 11})).unwrap();
        let mut h = harness(app, true);
        assert_eq!(coverage(&mut h, "mark")["applied"], "some");
        assert_eq!(toggled(&h, "Mark Positioning"), Some(egui::accesskit::Toggled::Mixed));
        let before = h.state().app.session.active().unwrap().doc.clone();
        let start = h.state().app.session.journal.len();
        click(&mut h, "Mark Positioning");
        assert_eq!(calls(&h, start), [feature_call(json!({"mark": true}))]);
        assert_eq!(coverage(&mut h, "mark")["applied"], "all");
        undo(&mut h);
        assert!(Arc::ptr_eq(&h.state().app.session.active().unwrap().doc, &before));
        assert_eq!(coverage(&mut h, "dlig")["available"], "some");
        let name = label(&h, "Discretionary Ligatures");
        let content = named_element(&h, "Text", &name);
        assert!(content.stop.is_none(), "italic content adds no keyboard stop");
        let start = h.state().app.session.journal.len();
        // Click the real styled label, not just the mark, to exercise public label activation.
        pointer(&mut h, content.rect.center(), true);
        pointer(&mut h, content.rect.center(), false);
        steps(&mut h, 2);
        assert_eq!(calls(&h, start), [feature_call(json!({"dlig": true}))]);
        assert_eq!(coverage(&mut h, "dlig")["applied"], "all");
        undo(&mut h);
        assert!(Arc::ptr_eq(&h.state().app.session.active().unwrap().doc, &before));
    }

    #[test]
    fn installed_held_input_cannot_retarget_another_range_or_document() {
        for target in ["range", "document", "absent"] {
            let (app, story) = text_app();
            let mut h = harness(app, true);
            let original = h.state().app.session.active().unwrap().doc.clone();
            let old_target = named_element(&h, "Checkbox", "Mark Positioning");
            pointer(&mut h, old_target.rect.center(), true);
            match target {
                "range" => select(&mut h, &story, 0, 6),
                "document" => {
                    h.state_mut().app.run("file.new", json!({})).unwrap();
                    h.state_mut()
                        .app
                        .run("frame.create", json!({"rect": [72, 72, 300, 200], "content": "text", "text": "office 1024", "caret": true}))
                        .unwrap();
                    h.state_mut().app.run("edit.selectAll", json!({})).unwrap();
                    steps(&mut h, 2);
                }
                _ => {
                    h.state_mut().app.run("selection.set", json!({"ids": []})).unwrap();
                    steps(&mut h, 2);
                }
            }
            let next = h.state().app.session.active().unwrap().doc.clone();
            let history = undo_len(&h);
            let start = h.state().app.session.journal.len();
            pointer(&mut h, old_target.rect.center(), false);
            steps(&mut h, 3);
            assert!(calls(&h, start).is_empty(), "held press must not follow changed {target}");
            assert_eq!(undo_len(&h), history);
            assert!(Arc::ptr_eq(&h.state().app.session.active().unwrap().doc, &next));
            assert!(Arc::ptr_eq(&h.state().app.session.documents()[0].doc, &original));
            if target != "absent" {
                let before = chars(&mut h);
                click(&mut h, "Mark Positioning");
                assert_eq!(calls(&h, start), [feature_call(json!({"mark": false}))]);
                undo(&mut h);
                assert_eq!(chars(&mut h), before);
                assert!(Arc::ptr_eq(&h.state().app.session.documents()[0].doc, &original));
            }
        }
    }

    #[test]
    fn installed_real_caret_refusal_and_absent_selection_preserve_documents_and_history() {
        let (app, story) = text_app();
        let mut h = harness(app, true);
        select(&mut h, &story, 0, 0);
        let before = h.state().app.session.active().unwrap().doc.clone();
        let history = undo_len(&h);
        let start = h.state().app.session.journal.len();
        click(&mut h, "Mark Positioning");
        assert!(calls(&h, start).is_empty(), "failed type.feature is not a successful journal entry");
        assert!(h.state().app.ui.status.contains("select text"));
        assert_eq!(undo_len(&h), history);
        assert!(Arc::ptr_eq(&h.state().app.session.active().unwrap().doc, &before));
        h.state_mut().app.run("selection.set", json!({"ids": []})).unwrap();
        steps(&mut h, 2);
        assert!(!h.state().recorded.as_ref().unwrap().elements.iter().any(|e| e.component == "Checkbox"));
        assert!(calls(&h, start).is_empty());
        assert_eq!(undo_len(&h), history);
        assert!(Arc::ptr_eq(&h.state().app.session.active().unwrap().doc, &before));
    }

    #[test]
    fn installed_localized_complete_labels_keep_natural_keyboard_and_logical_rtl_names() {
        for language in ["de", "ar"] {
            let (mut app, _) = text_app();
            app.ui.language = language.to_owned();
            let mut h = harness(app, true);
            let name = label(&h, "Superscript");
            assert_ne!(name, "Superscript", "the fixture exercises a translation");
            let node = named_element(&h, "Checkbox", &name);
            assert!(node.stop.is_some());
            assert_eq!(node.facts.as_ref().unwrap().name.as_deref(), Some(name.as_str()));
            tab_to(&mut h, "Superscript");
            let start = h.state().app.session.journal.len();
            key(&mut h, Key::Space);
            assert_eq!(calls(&h, start), [feature_call(json!({"numr": false, "dnom": false, "sups": true, "subs": false, "sinf": false}))]);
            assert_eq!(coverage(&mut h, "sups")["applied"], "all");
        }
    }

    #[test]
    fn uninstalled_parent_retains_original_checkbox_and_open_type_dispatch() {
        let (app, _) = text_app();
        let mut h = harness(app, false);
        assert!(!h.state().app.framework_installed);
        let before = chars(&mut h);
        let start = h.state().app.session.journal.len();
        let history = undo_len(&h);
        h.get_by_label("Mark Positioning").click();
        steps(&mut h, 2);
        assert_eq!(calls(&h, start), [feature_call(json!({"mark": false}))]);
        assert_eq!(chars(&mut h)["otfFeatures"], json!(["-mark"]));
        assert_eq!(undo_len(&h), history + 1);
        steps(&mut h, 3);
        assert_eq!(calls(&h, start).len(), 1);
        undo(&mut h);
        assert_eq!(chars(&mut h), before);
    }
}
