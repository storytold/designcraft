//! Live preflight (the Preflight panel and the status-bar indicator): overset text, missing
//! fonts, missing or low-resolution graphics, RGB content in print documents, empty text frames.

use designcraft_doc::{Content, Document, Intent, Item, SpreadRef};
use serde::Serialize;
use serde_json::{Value, json};

use super::{CommandSpec, cmd, has_doc};
use crate::{Result, Session};

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Issue {
    /// `error` or `warning`.
    pub severity: &'static str,
    pub kind: &'static str,
    pub message: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub item: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub page: Option<usize>,
}

pub fn specs() -> Vec<CommandSpec> {
    vec![cmd!(query "preflight.run", "Preflight Document", ["Window", "Output", "Preflight"], None,
        "{minPpi?: 150} → issues [{severity, kind, message, item?, page?}]", has_doc, run)]
}

fn page_of(d: &Document, sr: SpreadRef, it: &Item) -> Option<usize> {
    let SpreadRef::Doc(si) = sr else { return None };
    let sp = d.spreads.get(si)?;
    Some(d.first_page_of_spread(si) + sp.page_at_x(it.bounds().center().x).unwrap_or(0))
}

pub fn check(s: &Session, min_ppi: f64) -> Vec<Issue> {
    let Some(st) = s.active() else { return vec![] };
    let d = &st.doc;
    let mut out = Vec::new();
    // Stories: overset and fonts.
    let db = designcraft_fonts::FontDb::global();
    let mut missing_fonts: Vec<String> = Vec::new();
    let mut unsupported_typography = std::collections::BTreeSet::new();
    for story in d.stories.values() {
        let cs = s.cache.get(d, story.id, None);
        if cs.is_overset() {
            let last = story.frames.last().copied();
            let page = last.and_then(|f| d.find(f).zip(d.item(f))).and_then(|(loc, it)| page_of(d, loc.spread, it));
            let n = story.text[cs.overset_at.unwrap_or(0).min(story.len())..].chars().count();
            out.push(Issue { severity: "error", kind: "overset", message: format!("Overset text: {n} characters"), item: last.map(|i| i.0), page });
        }
        let ranges = story.para_ranges();
        for (pi, pf) in story.paras.iter().enumerate() {
            let (para, base) = d.styles.resolve_para(pf);
            let Some(range) = ranges.get(pi) else { continue };
            let has_rtl = story.text.get(range.clone()).is_some_and(|text| text.chars().any(designcraft_fonts::is_rtl));
            if has_rtl {
                if !matches!(para.arabic_justification.as_str(), "" | "DefaultJustification") {
                    unsupported_typography.insert(format!(
                        "Arabic justification policy `{}` is preserved; vendor-specific Naskh/Arabic algorithms are not implemented",
                        para.arabic_justification
                    ));
                }
                if para.paragraph_kashida_width.is_some() && para.arabic_justification != "DefaultJustification" {
                    unsupported_typography
                        .insert("Paragraph Kashida width preset is preserved; automatic elongation uses the engine's bounded allocation".into());
                }
            }
            if !matches!(para.mojikumi.as_str(), "" | "Nothing" | "None") {
                unsupported_typography.insert(format!("Mojikumi `{}` is preserved, but its spacing table is not applied", para.mojikumi));
            }
            if !para.kinsoku_type.is_empty() {
                unsupported_typography
                    .insert(format!("Kinsoku priority `{}` is preserved, but push-in/push-out priority is not applied", para.kinsoku_type));
            }
            let mut fams = vec![base.font_family.clone()];
            for (run, f) in story.runs().filter(|(run, _)| run.start < range.end && run.end > range.start) {
                let props = d.styles.resolve_char(&base, f);
                if has_rtl
                    && story.text.get(run.start.max(range.start)..run.end.min(range.end)).is_some_and(|t| t.chars().any(designcraft_fonts::is_rtl))
                {
                    use designcraft_doc::arabic::DiacriticPosition as P;
                    if !matches!(props.diacritic_position, P::Default | P::OpenType) {
                        unsupported_typography.insert(format!(
                            "Diacritic preset `{:?}` is preserved; font OpenType anchors and explicit offsets are used",
                            props.diacritic_position
                        ));
                    }
                    if !matches!(props.positional_form.as_str(), "" | "None" | "Calculate" | "Initial" | "Medial" | "Final" | "Isolated") {
                        unsupported_typography.insert(format!("Unknown positional form `{}` is preserved but not applied", props.positional_form));
                    }
                }
                fams.push(props.font_family);
            }
            for fam in fams {
                let composite = d.styles.composite_fonts.iter().find(|f| f.name == fam.trim_start_matches("CompositeFont/"));
                let families: Vec<&str> = composite.map_or_else(|| vec![fam.as_str()], |f| f.entries.iter().map(|e| e.family.as_str()).collect());
                for family in families {
                    if !db.has_family(family) && !missing_fonts.iter().any(|f| f == family) {
                        missing_fonts.push(family.to_string());
                    }
                }
            }
        }
    }
    for f in missing_fonts {
        out.push(Issue { severity: "error", kind: "missingFont", message: format!("Missing font: {f}"), item: None, page: None });
    }
    for message in unsupported_typography {
        out.push(Issue { severity: "warning", kind: "unsupportedTypography", message, item: None, page: None });
    }
    // Items.
    for (si, sp) in d.spreads.iter().enumerate() {
        for top in &sp.items {
            top.walk(&mut |it| {
                let page = page_of(d, SpreadRef::Doc(si), it);
                match &it.content {
                    Content::Graphic(g) => match d.assets.get(&g.asset) {
                        None => {
                            out.push(Issue { severity: "error", kind: "missingLink", message: "Missing graphic".into(), item: Some(it.id.0), page })
                        }
                        Some(a) if a.data.is_empty() => out.push(Issue {
                            severity: "error",
                            kind: "missingLink",
                            message: format!("Missing link: {}", a.link.as_deref().unwrap_or(&a.name)),
                            item: Some(it.id.0),
                            page,
                        }),
                        Some(a) => {
                            // Effective ppi: pixels per inch at the placed size. Placed PDF and
                            // SVG pages export as vectors; their `pixels` is only the preview's.
                            let vector = designcraft_images::is_pdf(&a.data) || designcraft_images::is_svg(&a.data);
                            if let Some((pw, _)) = a.pixels.filter(|_| !vector) {
                                let placed_w = (g.xf * it.xf).as_coeffs()[0].hypot((g.xf * it.xf).as_coeffs()[1]) * g.size.0;
                                let ppi = pw as f64 / (placed_w / 72.0).max(1e-6);
                                if d.settings.intent == Intent::Print && ppi < min_ppi {
                                    out.push(Issue {
                                        severity: "warning",
                                        kind: "lowResolution",
                                        message: format!("{}: effective {ppi:.0} ppi (< {min_ppi:.0})", a.name),
                                        item: Some(it.id.0),
                                        page,
                                    });
                                }
                            }
                        }
                    },
                    Content::Text(tf) if d.story(tf.story).is_some_and(|s| s.is_empty()) => {
                        out.push(Issue { severity: "warning", kind: "emptyText", message: "Empty text frame".into(), item: Some(it.id.0), page });
                    }
                    _ => {}
                }
            });
        }
    }
    // RGB swatches in use in a print document.
    if d.settings.intent == Intent::Print {
        for sw in &d.swatches {
            if let designcraft_color::SwatchValue::Color { color: designcraft_color::Color::Rgb { .. }, .. } = sw.value {
                out.push(Issue {
                    severity: "warning",
                    kind: "rgbColor",
                    message: format!("RGB swatch in a print document: {}", sw.name),
                    item: None,
                    page: None,
                });
            }
        }
    }
    out
}

fn run(s: &mut Session, p: &Value) -> Result<Value> {
    let min = p.get("minPpi").and_then(Value::as_f64).unwrap_or(150.0);
    let issues = check(s, min);
    let errors = issues.iter().filter(|i| i.severity == "error").count();
    Ok(json!({"errors": errors, "warnings": issues.len() - errors, "issues": issues}))
}

#[cfg(test)]
mod placed_vector_tests {
    use serde_json::json;

    use crate::Session;
    use crate::cmd::base64_encode;

    /// A placed PDF logo was reported as
    /// "logo.pdf: effective 72 ppi (< 150)", though it exports as vectors.
    #[test]
    fn vector_graphics_have_no_resolution() {
        let mut logo = Session::new();
        logo.execute("file.new", &json!({"width": 100, "height": 50})).unwrap();
        let pdf = logo.execute("file.exportPdf", &json!({})).unwrap();
        let mut s = Session::new();
        s.execute("file.new", &json!({})).unwrap();
        s.execute("file.place", &json!({"base64": pdf["base64"], "name": "logo.pdf", "x": 72, "y": 72, "width": 300})).unwrap();
        // Deselect the logo, or the photo would replace it in its frame.
        s.execute("selection.set", &json!({"ids": []})).unwrap();
        let png = designcraft_render::Rendered { width: 40, height: 20, pixels: vec![200; 40 * 20 * 4] }.to_png();
        s.execute("file.place", &json!({"base64": base64_encode(&png), "name": "photo.png", "x": 72, "y": 300, "width": 300})).unwrap();
        let r = s.execute("preflight.run", &json!({})).unwrap();
        let low: Vec<&str> =
            r["issues"].as_array().unwrap().iter().filter(|i| i["kind"] == "lowResolution").map(|i| i["message"].as_str().unwrap()).collect();
        assert_eq!(low, ["photo.png: effective 10 ppi (< 150)"]);
    }
}

#[cfg(test)]
mod arabic_tests {
    use super::*;

    #[test]
    fn arabic_unsupported_presets_are_reported_without_flagging_supported_offsets() {
        let mut s = Session::new();
        s.execute("file.new", &json!({})).unwrap();
        let text = "بُسْمِ";
        let r = s.execute("frame.create", &json!({"rect": [0, 0, 300, 200], "content": "text", "text": text})).unwrap();
        s.execute("text.select", &json!({"story": r["story"], "anchor": 0, "focus": text.len()})).unwrap();
        s.execute("type.char", &json!({"attrs": {"diacriticPosition": "openType", "diacriticXOffset": 150}})).unwrap();
        assert!(!check(&s, 150.0).iter().any(|i| i.kind == "unsupportedTypography"));
        s.execute("type.para", &json!({"attrs": {"arabicJustification": "NaskhJustification", "paragraphKashidaWidth": 2}})).unwrap();
        s.execute("type.char", &json!({"attrs": {"diacriticPosition": "tight"}})).unwrap();
        let issues = check(&s, 150.0);
        let typography: Vec<_> = issues.iter().filter(|i| i.kind == "unsupportedTypography").collect();
        assert_eq!(typography.len(), 3, "{typography:?}");
        assert!(typography.iter().all(|i| i.severity == "warning"));
    }
}
