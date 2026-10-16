//! The Typography panel: the OpenType features the fonts of the selected text offer, grouped as
//! Affinity's are. Every feature is a checkbox that shows how much of the selection it is applied to
//! (checked: all of it; a dash: some of it; empty: none). A feature only some of the selection's fonts
//! offer is italic. Changes apply to the selection through `type.feature`.

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

const FIGURE_STYLE: &[Choice] = &[("Lining", &["lnum"]), ("Oldstyle", &["onum"])];

const FIGURE_WIDTH: &[Choice] = &[("Tabular", &["tnum"]), ("Proportional", &["pnum"])];

const FIGURE_POSITION: &[Choice] = &[
    ("Numerator", &["numr"]),
    ("Denominator", &["dnom"]),
    ("Superscript", &["sups"]),
    ("Subscript", &["subs"]),
    ("Scientific Inferiors", &["sinf"]),
];

const FRACTION_AND_ORDINAL: &[Switch] = &[("frac", "Fractions"), ("ordn", "Ordinal")];

const CAPITALS: &[Choice] = &[("Caps to Small Caps", &["c2sc"]), ("Small Caps", &["smcp"]), ("All Small Caps", &["c2sc", "smcp"])];

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
    // Changes this frame: each feature tag to the state it is set to.
    let mut changes: BTreeMap<String, bool> = BTreeMap::new();
    crate::rtl::label(ui, egui::RichText::new(crate::i18n::tr(&lang, "Typography")).strong());
    egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
        section(ui, &lang, "Ligatures", |ui| switches(ui, &lang, &offered, LIGATURES, &mut changes));
        section(ui, &lang, "Alternates", |ui| switches(ui, &lang, &offered, ALTERNATES, &mut changes));
        section(ui, &lang, "Stylistic Sets", |ui| stylistic_sets(ui, &lang, &offered, &mut changes));
        section(ui, &lang, "Figure Style", |ui| choices(ui, &lang, &offered, FIGURE_STYLE, &mut changes));
        section(ui, &lang, "Figure Width", |ui| choices(ui, &lang, &offered, FIGURE_WIDTH, &mut changes));
        section(ui, &lang, "Figure Position", |ui| {
            choices(ui, &lang, &offered, FIGURE_POSITION, &mut changes);
            switches(ui, &lang, &offered, FRACTION_AND_ORDINAL, &mut changes);
        });
        section(ui, &lang, "Capitals", |ui| choices(ui, &lang, &offered, CAPITALS, &mut changes));
    });
    if !changes.is_empty() {
        let _ = app.run("type.feature", json!({"features": changes}));
    }
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
fn switches(ui: &mut egui::Ui, lang: &str, offered: &Offered, items: &[Switch], changes: &mut BTreeMap<String, bool>) {
    for (tag, label) in items {
        let Some(feature) = offered.get(tag) else { continue };
        if let Some(on) = tri_checkbox(ui, crate::i18n::tr(lang, label), feature) {
            changes.insert(tag.to_string(), on);
        }
    }
}

/// Mutually exclusive choices, as checkboxes: choosing one turns the others off. Choosing a checked
/// one turns it off.
fn choices(ui: &mut egui::Ui, lang: &str, offered: &Offered, items: &[Choice], changes: &mut BTreeMap<String, bool>) {
    for (i, (label, tags)) in items.iter().enumerate() {
        let Some(feature) = offered.choice(tags) else { continue };
        let Some(on) = tri_checkbox(ui, crate::i18n::tr(lang, label), feature) else { continue };
        for t in *tags {
            changes.insert(t.to_string(), on);
        }
        if on {
            for (j, (_, others)) in items.iter().enumerate() {
                if j == i {
                    continue;
                }
                for t in others.iter().filter(|t| !tags.contains(t)) {
                    changes.insert(t.to_string(), false);
                }
            }
        }
    }
}

/// Stylistic sets by the name their fonts give them, or the generic `Stylistic Set NN` when the
/// selection's fonts name them differently.
fn stylistic_sets(ui: &mut egui::Ui, lang: &str, offered: &Offered, changes: &mut BTreeMap<String, bool>) {
    for set in &offered.sets {
        let label = match &set.name {
            Some(name) => format!("{name} ({})", set.tag),
            None => format!("{} {:02}", crate::i18n::tr(lang, "Stylistic Set"), set.number),
        };
        if let Some(on) = tri_checkbox(ui, &label, set.feature) {
            changes.insert(set.tag.clone(), on);
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
