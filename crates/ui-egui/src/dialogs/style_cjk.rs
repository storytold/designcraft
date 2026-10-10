//! The CJK sections of Paragraph Style Options and Character Style Options, listed while Show CJK
//! Features is on ([`crate::cjk_features`]): Tate-chu-yoko, Kenten, Japanese Composition, Grid
//! and Warichu settings. They edit `c.<attr>` and `p.<attr>` fields like the other sections, and
//! OK applies them with the dialog's style edit command.

use designcraft_doc::cjk::Kinsoku;
use serde_json::{Value, json};

use super::{CharFields, Dialog, char_check, char_choice, char_number};

struct Section {
    id: &'static str,
    label: &'static str,
    /// Absent from Character Style Options.
    para_only: bool,
    /// Listed after Export Tagging.
    after_export: bool,
}

const SECTIONS: &[Section] = &[
    Section { id: "cjk.tcy", label: "Tate-chu-yoko Settings", para_only: false, after_export: false },
    Section { id: "cjk.kenten", label: "Kenten Settings", para_only: false, after_export: false },
    Section { id: "cjk.composition", label: "Japanese Composition Settings", para_only: true, after_export: false },
    Section { id: "cjk.grid", label: "Grid Settings", para_only: true, after_export: false },
    Section { id: "cjk.warichu", label: "Warichu Settings", para_only: false, after_export: true },
];

/// The section list of a style options dialog: `base` with the CJK sections when `cjk` is on.
/// They go before Export Tagging, except Warichu Settings, which follows it; without Export
/// Tagging they all go last.
pub(super) fn sections<'a>(cjk: bool, para: bool, base: &[(&'a str, &'a str)]) -> Vec<(&'a str, &'a str)> {
    if !cjk {
        return base.to_vec();
    }
    let shown = move |after: bool| SECTIONS.iter().filter(move |s| (para || !s.para_only) && s.after_export == after).map(|s| (s.id, s.label));
    let mut out = Vec::with_capacity(base.len() + SECTIONS.len());
    let mut placed = false;
    for s in base {
        if s.0 == "export" && !placed {
            out.extend(shown(false));
            out.push(*s);
            out.extend(shown(true));
            placed = true;
        } else {
            out.push(*s);
        }
    }
    if !placed {
        out.extend(shown(false));
        out.extend(shown(true));
    }
    out
}

pub(super) fn is_section(id: &str) -> bool {
    SECTIONS.iter().any(|s| s.id == id)
}

/// Draws CJK section `id`. `para` holds the paragraph style's resolved paragraph attributes; it
/// is `None` in Character Style Options, which have no paragraph-only sections.
pub(super) fn section(
    lang: &str,
    ui: &mut egui::Ui,
    d: &mut Dialog,
    id: &str,
    cf: &CharFields,
    para: Option<&Value>,
    doc: &designcraft_doc::Document,
) {
    match (id, para) {
        ("cjk.tcy", _) => tate_chu_yoko(lang, ui, d, cf),
        ("cjk.kenten", _) => kenten(lang, ui, d, cf),
        ("cjk.warichu", _) => warichu(lang, ui, d, cf),
        ("cjk.composition", Some(pv)) => japanese_composition(lang, ui, d, cf, pv, doc),
        ("cjk.grid", Some(pv)) => grid_settings(lang, ui, d, pv),
        _ => {}
    }
}

/// Tate-chu-yoko Settings: the switch and the run's offsets.
fn tate_chu_yoko(lang: &str, ui: &mut egui::Ui, d: &mut Dialog, cf: &CharFields) {
    char_check(ui, d, cf, "tateChuYoko", crate::i18n::tr(lang, "Tate-Chu-Yoko"));
    ui.add_space(4.0);
    egui::Grid::new("cjktcy").num_columns(2).spacing([8.0, 6.0]).show(ui, |ui| {
        crate::rtl::label(ui, crate::i18n::tr(lang, "Up/Down Position:"));
        char_number(ui, d, cf, "tateChuYokoYOffset", " pt", 1.0, (-1000.0, 1000.0));
        ui.end_row();
        crate::rtl::label(ui, crate::i18n::tr(lang, "Left/Right Position:"));
        char_number(ui, d, cf, "tateChuYokoXOffset", " pt", 1.0, (-1000.0, 1000.0));
        ui.end_row();
    });
}

/// The preset kenten marks: (label, character). The sesame dot is the empty character, which the
/// composer draws as U+FE45 and IDML export writes as `KentenSesameDot`.
pub(super) const KENTEN: &[(&str, &str)] = &[
    ("Sesame Dot", ""),
    ("White Sesame Dot", "\u{FE46}"),
    ("Bullseye", "\u{25C9}"),
    ("Black Circle", "\u{25CF}"),
    ("Small Black Circle", "\u{2022}"),
    ("Double Circle", "\u{25CE}"),
    ("Black Triangle", "\u{25B2}"),
    ("White Triangle", "\u{25B3}"),
    ("White Circle", "\u{25CB}"),
    ("Small White Circle", "\u{25E6}"),
];

/// Dialog-only: Custom was picked, so the Character field is editable even while it holds a
/// preset mark.
const KENTEN_CUSTOM: &str = "cjk.kentenCustom";

fn kenten_preset(character: &str) -> Option<&'static (&'static str, &'static str)> {
    let character = if character == "\u{FE45}" { "" } else { character };
    KENTEN.iter().find(|k| k.1 == character)
}

/// Kenten Type picks: `None` switches kenten off, `Some(mark)` sets that mark.
pub(super) fn pick_kenten(d: &mut Dialog, cf: &CharFields, mark: Option<&str>) {
    d.fields.remove(KENTEN_CUSTOM);
    match mark {
        Some(m) => {
            cf.set(d, "kenten", json!(true));
            cf.set(d, "kentenCharacter", json!(m));
        }
        None => {
            cf.set(d, "kenten", json!(false));
            // IDML export writes any stored mark as a custom kenten, even with kenten off.
            if cf.get(d, "kentenCharacter").as_str().is_some_and(|c| !c.is_empty()) {
                cf.set(d, "kentenCharacter", json!(""));
            }
        }
    }
}

/// Kenten Settings: the mark (a preset or a custom character).
fn kenten(lang: &str, ui: &mut egui::Ui, d: &mut Dialog, cf: &CharFields) {
    let on = cf.get(d, "kenten").as_bool();
    let character = cf.get(d, "kentenCharacter").as_str().unwrap_or("").to_string();
    let preset = kenten_preset(&character);
    let custom = on == Some(true) && (d.b(KENTEN_CUSTOM) || preset.is_none());
    let shown = match on {
        None => String::new(),
        Some(false) => crate::i18n::tr(lang, "None").to_string(),
        Some(true) if custom => crate::i18n::tr(lang, "Custom").to_string(),
        Some(true) => preset.map_or(String::new(), |p| crate::i18n::tr(lang, p.0).to_string()),
    };
    egui::Grid::new("cjkkenten").num_columns(2).spacing([8.0, 6.0]).show(ui, |ui| {
        crate::rtl::label(ui, crate::i18n::tr(lang, "Kenten Type:"));
        egui::ComboBox::from_id_salt("cjkkententype").selected_text(crate::rtl::widget(ui, shown)).width(200.0).show_ui(ui, |ui| {
            if cf.sparse && ui.selectable_label(on.is_none(), " ").clicked() {
                d.fields.remove(KENTEN_CUSTOM);
                cf.set(d, "kenten", Value::Null);
                cf.set(d, "kentenCharacter", Value::Null);
            }
            if ui.selectable_label(on == Some(false), crate::rtl::widget(ui, crate::i18n::tr(lang, "None"))).clicked() {
                pick_kenten(d, cf, None);
            }
            for (label, mark) in KENTEN {
                let selected = on == Some(true) && !custom && preset.is_some_and(|p| p.1 == *mark);
                if ui.selectable_label(selected, crate::rtl::widget(ui, crate::i18n::tr(lang, label))).clicked() {
                    pick_kenten(d, cf, Some(*mark));
                }
            }
            if ui.selectable_label(custom, crate::rtl::widget(ui, crate::i18n::tr(lang, "Custom"))).clicked() {
                d.fields.insert(KENTEN_CUSTOM.into(), json!(true));
                cf.set(d, "kenten", json!(true));
            }
        });
        ui.end_row();
        crate::rtl::label(ui, crate::i18n::tr(lang, "Character:"));
        let mut text = if custom { character.clone() } else { String::new() };
        if ui.add_enabled(custom, egui::TextEdit::singleline(&mut text).desired_width(40.0)).changed() {
            // One mark: the character typed last replaces the one before it.
            let mark: String = text.chars().last().map(String::from).unwrap_or_default();
            cf.set(d, "kentenCharacter", json!(mark));
        }
        ui.end_row();
    });
}

/// A whole-number attribute (stored as an integer: the model's fields are `u32`).
fn char_count(ui: &mut egui::Ui, d: &mut Dialog, cf: &CharFields, key: &str, range: (f64, f64)) {
    let v = cf.get(d, key).as_f64();
    let field = crate::widgets::NumField::number(key, v, "", 0).width(80.0).range(range.0, range.1);
    let edit = if cf.sparse { field.show_or_clear(ui) } else { field.show(ui).map(Some) };
    if let Some(n) = edit {
        cf.set(d, key, n.map_or(Value::Null, |n| json!(n.round().max(range.0).min(range.1) as u64)));
    }
}

/// Warichu Settings: the switch, lines, size, spacing, alignment and line break minimums.
fn warichu(lang: &str, ui: &mut egui::Ui, d: &mut Dialog, cf: &CharFields) {
    char_check(ui, d, cf, "warichu", crate::i18n::tr(lang, "Warichu"));
    ui.add_space(4.0);
    egui::Grid::new("cjkwarichu").num_columns(2).spacing([8.0, 6.0]).show(ui, |ui| {
        crate::rtl::label(ui, crate::i18n::tr(lang, "Lines:"));
        char_count(ui, d, cf, "warichuLines", (2.0, 16.0));
        ui.end_row();
        crate::rtl::label(ui, crate::i18n::tr(lang, "Warichu Size:"));
        char_number(ui, d, cf, "warichuSize", "%", 1.0, (1.0, 1000.0));
        ui.end_row();
        crate::rtl::label(ui, crate::i18n::tr(lang, "Line Spacing:"));
        char_number(ui, d, cf, "warichuLineSpacing", " pt", 1.0, (-1000.0, 1000.0));
        ui.end_row();
        crate::rtl::label(ui, crate::i18n::tr(lang, "Alignment:"));
        char_choice(
            lang,
            ui,
            d,
            cf,
            "warichuAlignment",
            &[
                (json!("auto"), "Auto"),
                (json!("left"), "Left"),
                (json!("center"), "Center"),
                (json!("right"), "Right"),
                (json!("justify"), "Justify"),
            ],
        );
        ui.end_row();
    });
    ui.add_space(6.0);
    crate::rtl::label(ui, egui::RichText::new(crate::i18n::tr(lang, "Line Break Options")).font(crate::theme::semibold(12.0)));
    egui::Grid::new("cjkwarichubreak").num_columns(2).spacing([8.0, 6.0]).show(ui, |ui| {
        crate::rtl::label(ui, crate::i18n::tr(lang, "Characters Before Break:"));
        char_count(ui, d, cf, "warichuCharsBeforeBreak", (0.0, 100.0));
        ui.end_row();
        crate::rtl::label(ui, crate::i18n::tr(lang, "Characters After Break:"));
        char_count(ui, d, cf, "warichuCharsAfterBreak", (0.0, 100.0));
        ui.end_row();
    });
}

/// A paragraph attribute as shown: the dialog's `p.<key>` edit, else the resolved value.
fn para_value(d: &Dialog, pv: &Value, key: &str) -> Value {
    d.fields.get(&format!("p.{key}")).cloned().unwrap_or_else(|| pv.get(key).cloned().unwrap_or(Value::Null))
}

/// A paragraph attribute from `opts` (value, label); `current` is the value shown.
fn para_choice(lang: &str, ui: &mut egui::Ui, d: &mut Dialog, key: &str, current: &str, opts: &[(&str, &str)]) {
    let shown = opts.iter().find(|o| o.0 == current).map_or("", |o| crate::i18n::tr(lang, o.1));
    egui::ComboBox::from_id_salt(("cjkp", key)).selected_text(crate::rtl::widget(ui, shown)).width(220.0).show_ui(ui, |ui| {
        for (v, label) in opts {
            if ui.selectable_label(*v == current, crate::rtl::widget(ui, crate::i18n::tr(lang, label))).clicked() {
                d.fields.insert(format!("p.{key}"), json!(v));
            }
        }
    });
}

fn para_check(ui: &mut egui::Ui, d: &mut Dialog, pv: &Value, key: &str, label: &str) {
    let mut on = para_value(d, pv, key).as_bool().unwrap_or(false);
    if ui.checkbox(&mut on, crate::rtl::widget(ui, label)).changed() {
        d.fields.insert(format!("p.{key}"), json!(on));
    }
}

/// The built-in kinsoku sets ([`Kinsoku::named`]) and their labels.
const BUILT_IN_KINSOKU: &[(&str, &str)] = &[
    ("HardKinsoku", "Hard Kinsoku"),
    ("SoftKinsoku", "Soft Kinsoku"),
    ("SimplifiedChineseKinsoku", "Simplified Chinese Kinsoku"),
    ("TraditionalChineseKinsoku", "Traditional Chinese Kinsoku"),
    ("KoreanKinsoku", "Korean Kinsoku"),
];

/// The Kinsoku Set menu: no kinsoku, the built-in sets, then every other named set the document's
/// paragraph styles and paragraphs use (the model keeps each set by value, not in a list).
pub(super) fn kinsoku_sets(doc: &designcraft_doc::Document) -> Vec<Kinsoku> {
    let mut sets = vec![Kinsoku::default()];
    sets.extend(BUILT_IN_KINSOKU.iter().filter_map(|(name, _)| Kinsoku::named(name)));
    let used = doc.styles.paragraph.iter().map(|s| &s.para.kinsoku).chain(doc.stories.values().flat_map(|s| s.paras.iter().map(|p| &p.para.kinsoku)));
    for k in used.flatten().flatten() {
        if !k.name.is_empty() && !sets.iter().any(|s| s.name == k.name) {
            sets.push(k.clone());
        }
    }
    sets
}

fn kinsoku_label(lang: &str, name: &str) -> String {
    if name.is_empty() {
        return crate::i18n::tr(lang, "None").to_string();
    }
    BUILT_IN_KINSOKU.iter().find(|k| k.0 == name).map_or_else(|| name.to_string(), |k| crate::i18n::tr(lang, k.1).to_string())
}

/// IDML's names for "no mojikumi set".
fn mojikumi_value(v: &str) -> &str {
    if matches!(v, "Nothing" | "None") { "" } else { v }
}

/// The Mojikumi Set menu as (value, label): none (empty label), the document's sets, and the
/// `current` value when it names a set the document doesn't define (kept as imported).
pub(super) fn mojikumi_sets(doc: &designcraft_doc::Document, current: &str) -> Vec<(String, String)> {
    let mut out = vec![(String::new(), String::new())];
    for t in &doc.styles.mojikumi_tables {
        let v = format!("MojikumiTable/{}", t.name);
        if !out.iter().any(|o| o.0 == v) {
            out.push((v, t.name.clone()));
        }
    }
    let current = mojikumi_value(current);
    if !current.is_empty() && !out.iter().any(|o| o.0 == current) {
        out.push((current.to_string(), current.trim_start_matches("MojikumiTable/").trim_start_matches("$ID/").to_string()));
    }
    out
}

/// Japanese Composition Settings: kinsoku, hanging, bunri-kinshi, mojikumi, the leading model,
/// rensuuji and line-end ideographic spaces.
fn japanese_composition(lang: &str, ui: &mut egui::Ui, d: &mut Dialog, cf: &CharFields, pv: &Value, doc: &designcraft_doc::Document) {
    egui::Grid::new("cjkcomp").num_columns(2).spacing([8.0, 6.0]).show(ui, |ui| {
        crate::rtl::label(ui, crate::i18n::tr(lang, "Kinsoku Set:"));
        let sets = kinsoku_sets(doc);
        // `null`: no set given, so the composer's default rules apply.
        let name = para_value(d, pv, "kinsoku").get("name").and_then(Value::as_str).map(str::to_string);
        let shown = name.as_deref().map_or_else(|| crate::i18n::tr(lang, "Default Rules").to_string(), |n| kinsoku_label(lang, n));
        egui::ComboBox::from_id_salt("cjkkinsoku").selected_text(crate::rtl::widget(ui, shown)).width(220.0).show_ui(ui, |ui| {
            // `style.paragraph.edit` takes `null` as the default rules, also over a Based On set.
            if ui.selectable_label(name.is_none(), crate::rtl::widget(ui, crate::i18n::tr(lang, "Default Rules"))).clicked() {
                d.fields.insert("p.kinsoku".into(), Value::Null);
            }
            for k in &sets {
                if ui.selectable_label(name.as_deref() == Some(k.name.as_str()), crate::rtl::widget(ui, kinsoku_label(lang, &k.name))).clicked() {
                    if let Ok(v) = serde_json::to_value(k) {
                        d.fields.insert("p.kinsoku".into(), v);
                    }
                }
            }
        });
        ui.end_row();
        crate::rtl::label(ui, crate::i18n::tr(lang, "Kinsoku Type:"));
        let kind = para_value(d, pv, "kinsokuType");
        let kind = match kind.as_str().unwrap_or("") {
            "KinsokuPushInFirst" => "",
            k => k,
        }
        .to_string();
        para_choice(
            lang,
            ui,
            d,
            "kinsokuType",
            &kind,
            &[
                ("", "Push In First"),
                ("KinsokuPushOutFirst", "Push Out First"),
                ("KinsokuPushOutOnly", "Push Out Only"),
                ("KinsokuPrioritizeAdjustmentAmount", "Prioritize Adjustment Amount"),
            ],
        );
        ui.end_row();
        crate::rtl::label(ui, crate::i18n::tr(lang, "Hanging Type:"));
        let hang = para_value(d, pv, "kinsokuHang").as_str().unwrap_or("none").to_string();
        para_choice(lang, ui, d, "kinsokuHang", &hang, &[("none", "None"), ("regular", "Standard"), ("force", "Force")]);
        ui.end_row();
    });
    para_check(ui, d, pv, "bunriKinshi", crate::i18n::tr(lang, "Bunri-Kinshi"));
    ui.add_space(4.0);
    egui::Grid::new("cjkcomp2").num_columns(2).spacing([8.0, 6.0]).show(ui, |ui| {
        crate::rtl::label(ui, crate::i18n::tr(lang, "Mojikumi Set:"));
        let raw = para_value(d, pv, "mojikumi");
        let current = mojikumi_value(raw.as_str().unwrap_or("")).to_string();
        let sets = mojikumi_sets(doc, &current);
        let label = |l: &str| if l.is_empty() { crate::i18n::tr(lang, "None").to_string() } else { l.to_string() };
        let shown = sets.iter().find(|s| s.0 == current).map_or_else(String::new, |s| label(s.1.as_str()));
        egui::ComboBox::from_id_salt("cjkmojikumi").selected_text(crate::rtl::widget(ui, shown)).width(220.0).show_ui(ui, |ui| {
            for (v, l) in &sets {
                if ui.selectable_label(*v == current, crate::rtl::widget(ui, label(l.as_str()))).clicked() {
                    d.fields.insert("p.mojikumi".into(), json!(v));
                }
            }
        });
        ui.end_row();
        crate::rtl::label(ui, crate::i18n::tr(lang, "Leading Model:"));
        // Labelled by where the composer measures the leading from (`cjk_line_reference` in
        // compose): `akiAbove` from the em box top, `akiBelow` from its bottom.
        let mut opts = vec![
            (json!("akiAbove"), "Em Box Top/Right"),
            (json!("center"), "Em Box Center"),
            (json!("roman"), "Roman Baseline"),
            (json!("akiBelow"), "Em Box Bottom/Left"),
        ];
        // Kept from IDML; not offered otherwise.
        if cf.get(d, "leadingModel") == json!("centerDown") {
            opts.push((json!("centerDown"), "Em Box Center (Down)"));
        }
        char_choice(lang, ui, d, cf, "leadingModel", &opts);
        ui.end_row();
    });
    para_check(ui, d, pv, "rensuuji", crate::i18n::tr(lang, "Rensuuji"));
    para_check(ui, d, pv, "treatIdeographicSpaceAsSpace", crate::i18n::tr(lang, "Absorb Ideographic Space at Line End"));
}

/// Grid Settings: whether lines align to the baseline grid, and only the first line.
fn grid_settings(lang: &str, ui: &mut egui::Ui, d: &mut Dialog, pv: &Value) {
    use designcraft_doc::GridAlign;
    let cur: GridAlign = serde_json::from_value(para_value(d, pv, "gridAlign")).unwrap_or_default();
    let set = |d: &mut Dialog, g: GridAlign| {
        if let Ok(v) = serde_json::to_value(g) {
            d.fields.insert("p.gridAlign".into(), v);
        }
    };
    egui::Grid::new("cjkgrid").num_columns(2).spacing([8.0, 6.0]).show(ui, |ui| {
        crate::rtl::label(ui, crate::i18n::tr(lang, "Grid Alignment:"));
        let on = cur != GridAlign::None;
        let shown = crate::i18n::tr(lang, if on { "Roman Baseline" } else { "None" });
        egui::ComboBox::from_id_salt("cjkgridalign").selected_text(crate::rtl::widget(ui, shown)).width(220.0).show_ui(ui, |ui| {
            if ui.selectable_label(!on, crate::rtl::widget(ui, crate::i18n::tr(lang, "None"))).clicked() {
                set(d, GridAlign::None);
            }
            if ui.selectable_label(on, crate::rtl::widget(ui, crate::i18n::tr(lang, "Roman Baseline"))).clicked() && !on {
                set(d, GridAlign::AllLines);
            }
        });
        ui.end_row();
    });
    let mut first = cur == GridAlign::FirstLineOnly;
    let check = egui::Checkbox::new(&mut first, crate::rtl::widget(ui, crate::i18n::tr(lang, "Align Only First Line to Grid")));
    if ui.add_enabled(cur != GridAlign::None, check).changed() {
        set(d, if first { GridAlign::FirstLineOnly } else { GridAlign::AllLines });
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::DesignApp;

    /// A document with paragraph style Body on its text and character style Emphasis.
    fn app_with_text() -> DesignApp {
        let mut app = DesignApp::new(designcraft_engine::Session::new(), crate::Services::default());
        app.run("file.new", json!({})).unwrap();
        app.run("style.paragraph.create", json!({"name": "Body"})).unwrap();
        app.run("style.character.create", json!({"name": "Emphasis", "chars": {"fontStyle": "Italic"}})).unwrap();
        let r = app.run("frame.create", json!({"rect": [72, 72, 300, 200], "content": "text", "text": "漢字かな"})).unwrap();
        app.run("text.select", json!({"story": r["story"], "anchor": 0, "focus": 2})).unwrap();
        app.run("style.paragraph.apply", json!({"name": "Body"})).unwrap();
        app
    }

    /// The text painted by the open dialog once it has settled.
    fn painted(app: &mut DesignApp, ctx: &egui::Context) -> Vec<String> {
        fn collect(shape: &egui::Shape, out: &mut Vec<String>) {
            match shape {
                egui::Shape::Text(t) => out.push(t.galley.job.text.trim().to_string()),
                egui::Shape::Vec(shapes) => shapes.iter().for_each(|s| collect(s, out)),
                _ => {}
            }
        }
        let screen = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(1400.0, 900.0));
        let mut texts = Vec::new();
        for _ in 0..4 {
            let time = ctx.input(|i| i.time) + 1.0 / 60.0;
            let mut output = ctx.run_ui(egui::RawInput { screen_rect: Some(screen), time: Some(time), ..Default::default() }, |ui| {
                crate::dialogs::show(app, ui.ctx());
            });
            output.textures_delta.clear();
            texts.clear();
            for s in &output.shapes {
                collect(&s.shape, &mut texts);
            }
        }
        texts
    }

    /// The list model, not painted labels: the section list scrolls, so lower rows may be clipped.
    #[test]
    fn cjk_sections_follow_show_cjk_features() {
        let mut app = app_with_text();
        let base = [("general", "General"), ("export", "Export Tagging")];
        let ids = |app: &DesignApp, para: bool| sections(crate::cjk_features(app), para, &base).into_iter().map(|s| s.0).collect::<Vec<_>>();
        app.run("prefs.set", json!({"cjkFeatures": true})).unwrap();
        assert_eq!(ids(&app, true), ["general", "cjk.tcy", "cjk.kenten", "cjk.composition", "cjk.grid", "export", "cjk.warichu"]);
        assert_eq!(ids(&app, false), ["general", "cjk.tcy", "cjk.kenten", "export", "cjk.warichu"]);
        assert_eq!(
            sections(true, false, &[("general", "General")]).into_iter().map(|s| s.0).collect::<Vec<_>>(),
            ["general", "cjk.tcy", "cjk.kenten", "cjk.warichu"]
        );
        app.run("prefs.set", json!({"cjkFeatures": false})).unwrap();
        assert_eq!(ids(&app, true), ["general", "export"]);
        assert_eq!(ids(&app, false), ["general", "export"]);
    }

    #[test]
    fn cjk_sections_draw() {
        let mut app = app_with_text();
        app.run("prefs.set", json!({"cjkFeatures": true})).unwrap();
        let ctx = egui::Context::default();
        for lang in ["", "ja", "ar"] {
            app.run("app.language", json!({"lang": lang})).unwrap();
            for s in SECTIONS {
                for fields in [
                    json!({"id": "paragraphStyleOptions", "name": "Body"}),
                    json!({"id": "paragraphStyleOptions", "new": true}),
                    json!({"id": "characterStyleOptions", "name": "Emphasis"}),
                    json!({"id": "characterStyleOptions", "name": "Emphasis", "c.kenten": true, "c.kentenCharacter": "※", "c.warichuLines": null}),
                ] {
                    let id = fields["id"].as_str().unwrap().to_string();
                    let mut f = fields.clone();
                    f["section"] = json!(s.id);
                    app.ui.dialog = Some(Dialog::new(&id, f));
                    painted(&mut app, &ctx);
                    assert!(app.ui.dialog.is_some(), "{lang} {id} {} stays open", s.id);
                }
            }
        }
    }

    #[test]
    fn kenten_and_japanese_composition_reach_the_style_and_its_paragraph() {
        let mut app = app_with_text();
        app.run("prefs.set", json!({"cjkFeatures": true})).unwrap();
        if let Some(st) = app.session.active_mut() {
            let table = designcraft_doc::cjk::MojikumiTable { name: "Body Text".into(), ..Default::default() };
            std::sync::Arc::make_mut(&mut st.doc).styles_mut().mojikumi_tables.push(table);
        }
        let doc = app.session.active().unwrap().doc.clone();
        let hard = kinsoku_sets(&doc).into_iter().find(|k| k.name == "HardKinsoku").unwrap();
        let moji = mojikumi_sets(&doc, "").into_iter().find(|s| s.1 == "Body Text").unwrap().0;
        assert_eq!(moji, "MojikumiTable/Body Text");

        let ctx = egui::Context::default();
        app.ui.dialog = Some(Dialog::new("paragraphStyleOptions", json!({"name": "Body", "section": "cjk.kenten"})));
        painted(&mut app, &ctx);
        {
            let d = app.ui.dialog.as_mut().unwrap();
            pick_kenten(d, &CharFields { base: &Value::Null, sparse: false }, Some("\u{25CF}"));
            d.fields.insert("section".into(), json!("cjk.composition"));
            d.fields.insert("p.kinsoku".into(), serde_json::to_value(&hard).unwrap());
            d.fields.insert("p.mojikumi".into(), json!(moji));
            d.fields.insert("p.kinsokuHang".into(), json!("regular"));
        }
        painted(&mut app, &ctx);
        crate::dialogs::confirm(&mut app).unwrap();

        let doc = app.session.active().unwrap().doc.clone();
        let st = doc.styles.para("Body").unwrap();
        assert_eq!(st.para.kinsoku, Some(Some(hard)));
        assert_eq!(st.para.mojikumi.as_deref(), Some("MojikumiTable/Body Text"));
        assert_eq!(st.para.kinsoku_hang, Some(designcraft_doc::cjk::KinsokuHang::Regular));
        assert_eq!((st.chars.kenten, st.chars.kenten_character.as_deref()), (Some(true), Some("\u{25CF}")));
        let a = app.run("type.selectionAttrs", json!({})).unwrap();
        assert_eq!(a["para"]["kinsoku"]["name"], "HardKinsoku", "the paragraph follows its style");
        assert_eq!(a["para"]["mojikumi"], "MojikumiTable/Body Text");
        assert_eq!(a["chars"]["kenten"], true);
        assert_eq!(a["chars"]["kentenCharacter"], "\u{25CF}");

        // Default Rules (`null`) replaces the set.
        app.ui.dialog = Some(Dialog::new("paragraphStyleOptions", json!({"name": "Body", "section": "cjk.composition", "p.kinsoku": null})));
        painted(&mut app, &ctx);
        crate::dialogs::confirm(&mut app).unwrap();
        assert_eq!(app.session.active().unwrap().doc.styles.para("Body").unwrap().para.kinsoku, Some(None));
        assert!(app.run("type.selectionAttrs", json!({})).unwrap()["para"]["kinsoku"].is_null());

        // A character style sets only the kenten attributes.
        app.ui.dialog = Some(Dialog::new("characterStyleOptions", json!({"name": "Emphasis", "section": "cjk.kenten"})));
        painted(&mut app, &ctx);
        {
            let d = app.ui.dialog.as_mut().unwrap();
            let base = json!({});
            pick_kenten(d, &CharFields { base: &base, sparse: true }, Some(""));
        }
        crate::dialogs::confirm(&mut app).unwrap();
        let st = app.session.active().unwrap().doc.styles.char_style("Emphasis").cloned().unwrap();
        assert_eq!((st.chars.kenten, st.chars.kenten_character.as_deref()), (Some(true), Some("")));
    }
}
