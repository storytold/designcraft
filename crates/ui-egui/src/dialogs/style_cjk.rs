//! The CJK sections of Paragraph Style Options and Character Style Options, listed while Show CJK
//! Features is on ([`crate::cjk_features`]): Auto Tate-chu-yoko, Tate-chu-yoko, the four ruby
//! sections, Kenten, Kenten Color, Shatai, Japanese Composition, Grid and Warichu settings. They
//! edit `c.<attr>` and `p.<attr>` fields like the other sections, and OK applies them with the
//! dialog's style edit command. The Ruby dialog (Type › Ruby…) shows the four ruby sections over
//! the selected text and applies them with `type.ruby`.

use designcraft_doc::cjk::Kinsoku;
use designcraft_doc::cjk_settings::KentenKind;
use serde_json::{Value, json};

use super::{CharFields, Dialog};
use crate::DesignApp;

struct Section {
    id: &'static str,
    label: &'static str,
    /// Absent from Character Style Options.
    para_only: bool,
    /// Listed after Export Tagging.
    after_export: bool,
}

const fn section(id: &'static str, label: &'static str, para_only: bool, after_export: bool) -> Section {
    Section { id, label, para_only, after_export }
}

/// In the order of the reference dialog.
const SECTIONS: &[Section] = &[
    section("cjk.autoTcy", "Auto Tate-chu-yoko Settings", true, false),
    section("cjk.tcy", "Tate-chu-yoko Settings", false, false),
    section("cjk.rubyPlacement", "Ruby Placement and Spacing", false, false),
    section("cjk.rubyFont", "Ruby Font and Size", false, false),
    section("cjk.rubyLonger", "Adjustment When Ruby Is Longer Than Parent", false, false),
    section("cjk.rubyColor", "Ruby Color", false, false),
    section("cjk.kenten", "Kenten Settings", false, false),
    section("cjk.kentenColor", "Kenten Color", false, false),
    section("cjk.shatai", "Shatai", false, false),
    section("cjk.composition", "Japanese Composition Settings", true, false),
    section("cjk.grid", "Grid Settings", true, false),
    section("cjk.warichu", "Warichu Settings", false, true),
];

/// The Ruby dialog's sections, the same panes as in the style dialogs.
pub(super) const RUBY_SECTIONS: &[(&str, &str)] = &[
    ("cjk.rubyPlacement", "Ruby Placement and Spacing"),
    ("cjk.rubyFont", "Ruby Font and Size"),
    ("cjk.rubyLonger", "Adjustment When Ruby Is Longer Than Parent"),
    ("cjk.rubyColor", "Ruby Color"),
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

/// The `type.ruby` parameters for the Ruby dialog's edits: the reading and every `c.ruby*`
/// field, in the command's units (percentages for scales and tints).
pub(super) fn ruby_params(d: &Dialog) -> Value {
    let mut p = serde_json::Map::new();
    p.insert("text".into(), json!(d.s("text")));
    // (attribute, parameter, factor from the model's unit to the parameter's)
    const MAP: &[(&str, &str, f64)] = &[
        ("rubyType", "type", 0.0),
        ("rubyAlignment", "alignment", 0.0),
        ("rubyPosition", "position", 0.0),
        ("rubyXOffset", "xOffset", 1.0),
        ("rubyYOffset", "yOffset", 1.0),
        ("rubyFont", "font", 0.0),
        ("rubyFontStyle", "fontStyle", 0.0),
        ("rubyFontSize", "size", 1.0),
        ("rubyXScale", "xScale", 100.0),
        ("rubyYScale", "yScale", 100.0),
        ("rubyOpenTypePro", "openTypePro", 0.0),
        ("rubyAutoTcyDigits", "autoTcyDigits", 1.0),
        ("rubyAutoTcyIncludeRoman", "autoTcyIncludeRoman", 0.0),
        ("rubyAutoTcyAutoScale", "autoTcyAutoScale", 0.0),
        ("rubyOverhangAmount", "overhang", 0.0),
        ("rubyParentSpacing", "parentSpacing", 0.0),
        ("rubyAutoAlign", "autoAlign", 0.0),
        ("rubyAutoScaling", "autoScaling", 0.0),
        ("rubyScalingMin", "scalingPercent", 100.0),
        ("rubyFill", "fill", 0.0),
        ("rubyStroke", "stroke", 0.0),
        ("rubyFillTint", "fillTint", 100.0),
        ("rubyStrokeTint", "strokeTint", 100.0),
        ("rubyStrokeWeight", "strokeWeight", 1.0),
        ("rubyOverprintFill", "overprintFill", 0.0),
        ("rubyOverprintStroke", "overprintStroke", 0.0),
    ];
    for (attr, param, factor) in MAP {
        let Some(v) = d.fields.get(&format!("c.{attr}")) else { continue };
        let v = match v.as_f64() {
            Some(x) if *factor != 0.0 => json!(x * factor),
            _ => v.clone(),
        };
        p.insert((*param).into(), v);
    }
    Value::Object(p)
}

/// Kenten Type picks: `None` switches kenten off (keeping the other settings), `Some(kind)`
/// switches them on with that kind. A preset stores its mark as the character too, as
/// `type.kenten` does.
pub(super) fn pick_kenten(d: &mut Dialog, cf: &CharFields, kind: Option<KentenKind>) {
    let Some(kind) = kind else {
        cf.set(d, "kenten", json!(false));
        return;
    };
    cf.set(d, "kenten", json!(true));
    cf.set(d, "kentenKind", json!(kind));
    if let Some(mark) = kind.mark() {
        cf.set(d, "kentenCharacter", json!(mark));
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

    /// The list model, not painted labels: the section list scrolls, so lower rows may be clipped.
    #[test]
    fn cjk_sections_follow_show_cjk_features() {
        let mut app = app_with_text();
        let base = [("general", "General"), ("export", "Export Tagging")];
        let ids = |app: &DesignApp, para: bool| sections(crate::cjk_features(app), para, &base).into_iter().map(|s| s.0).collect::<Vec<_>>();
        app.run("prefs.set", json!({"cjkFeatures": true})).unwrap();
        let character =
            ["cjk.tcy", "cjk.rubyPlacement", "cjk.rubyFont", "cjk.rubyLonger", "cjk.rubyColor", "cjk.kenten", "cjk.kentenColor", "cjk.shatai"];
        let mut para = vec!["general", "cjk.autoTcy"];
        para.extend(character);
        para.extend(["cjk.composition", "cjk.grid", "export", "cjk.warichu"]);
        assert_eq!(ids(&app, true), para);
        let mut chars = vec!["general"];
        chars.extend(character);
        chars.extend(["export", "cjk.warichu"]);
        assert_eq!(ids(&app, false), chars);
        let mut last = vec!["general"];
        last.extend(character);
        last.push("cjk.warichu");
        assert_eq!(sections(true, false, &[("general", "General")]).into_iter().map(|s| s.0).collect::<Vec<_>>(), last);
        app.run("prefs.set", json!({"cjkFeatures": false})).unwrap();
        assert_eq!(ids(&app, true), ["general", "export"]);
        assert_eq!(ids(&app, false), ["general", "export"]);
    }

}
