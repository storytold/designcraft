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

pub fn show(app: &mut DesignApp, ui: &mut egui::Ui) {
    if super::text_attrs(app).is_none() {
        crate::rtl::label(ui, crate::i18n::tr(&app.ui.language, "Select text or a text frame."));
        return;
    }
    let offered = offered(app);
    if offered.features.is_empty() && offered.sets.is_empty() {
        crate::rtl::label(ui, crate::i18n::tr(&app.ui.language, "The selected fonts have no OpenType features."));
        return;
    }
    let lang = app.ui.language.clone();
    let mut changes = Changes::default();
    crate::rtl::label(ui, egui::RichText::new(crate::i18n::tr(&lang, "Typography")).strong());
    egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
        section(ui, &lang, "Ligatures", |ui| switches(ui, &lang, &offered, LIGATURES, &mut changes));
        section(ui, &lang, "Alternates", |ui| switches(ui, &lang, &offered, ALTERNATES, &mut changes));
        section(ui, &lang, "Stylistic Sets", |ui| stylistic_sets(ui, &lang, &offered, &mut changes));
        section(ui, &lang, "Figure Style", |ui| figures(ui, &lang, &offered, &FIGURE_STYLE, &FIGURE_WIDTH, &mut changes));
        section(ui, &lang, "Figure Width", |ui| figures(ui, &lang, &offered, &FIGURE_WIDTH, &FIGURE_STYLE, &mut changes));
        section(ui, &lang, "Figure Position", |ui| {
            choices(ui, &lang, &offered, FIGURE_POSITION, &mut changes);
            switches(ui, &lang, &offered, FRACTION_AND_ORDINAL, &mut changes);
        });
        section(ui, &lang, "Capitals", |ui| choices(ui, &lang, &offered, CAPITALS, &mut changes));
        section(ui, &lang, "Other Features", |ui| switches(ui, &lang, &offered, &other_toggles(), &mut changes));
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
fn tri_checkbox(ui: &mut egui::Ui, text: &str, feature: Feature) -> Option<bool> {
    let mut on = feature.applied == Covered::All;
    let mut label = egui::RichText::new(text);
    if feature.available == Covered::Some {
        label = label.italics();
    }
    let response = ui.add(egui::Checkbox::new(&mut on, crate::rtl::widget(ui, label)).indeterminate(feature.applied == Covered::Some));
    response.clicked().then_some(feature.applied != Covered::All)
}

/// On/off switches the fonts offer.
fn switches(ui: &mut egui::Ui, lang: &str, offered: &Offered, items: &[Switch], changes: &mut Changes) {
    for (tag, label) in items {
        let Some(feature) = offered.get(tag) else { continue };
        if let Some(on) = tri_checkbox(ui, crate::i18n::tr(lang, label), feature) {
            changes.set(tag, on);
        }
    }
}

/// Mutually exclusive choices, as checkboxes: choosing one turns the others off. Choosing a checked
/// one turns it off.
fn choices(ui: &mut egui::Ui, lang: &str, offered: &Offered, items: &[Choice], changes: &mut Changes) {
    for (i, (label, tags)) in items.iter().enumerate() {
        let Some(feature) = offered.choice(tags) else { continue };
        let Some(on) = tri_checkbox(ui, crate::i18n::tr(lang, label), feature) else { continue };
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
fn figures(ui: &mut egui::Ui, lang: &str, offered: &Offered, axis: &FigureAxis, other: &FigureAxis, changes: &mut Changes) {
    for (i, (label, tags)) in axis.choices.iter().enumerate() {
        let Some(feature) = offered.choice(tags) else { continue };
        if let Some(on) = tri_checkbox(ui, crate::i18n::tr(lang, label), feature) {
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
fn stylistic_sets(ui: &mut egui::Ui, lang: &str, offered: &Offered, changes: &mut Changes) {
    for set in &offered.sets {
        let label = match &set.name {
            Some(name) => format!("{name} ({})", set.tag),
            None => format!("{} {:02}", crate::i18n::tr(lang, "Stylistic Set"), set.number),
        };
        if let Some(on) = tri_checkbox(ui, &label, set.feature) {
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
