//! Preferences: the application's (`prefs.set`, kept by the session) and the ones InDesign stores
//! with the document (`document.preferences`: units, keyboard increment, grids, guide colours).

use serde_json::{Map, Value};

use super::{CommandSpec, bad, cmd, has_doc};
use crate::Result;

/// Document settings that Preferences edits (Document Setup edits the rest).
const DOC_KEYS: &[&str] = &[
    "horizontalUnits",
    "verticalUnits",
    "keyboardIncrement",
    "baselineGrid",
    "grid",
    "pasteboard",
    "marginColor",
    "columnColor",
    "bleedColor",
    "slugColor",
    "advancedType",
    "overprintBlack",
    "glyphFallback",
    "rulerOrigin",
];

pub fn specs() -> Vec<CommandSpec> {
    vec![
        cmd!(
            "edit.transparencyBlendSpace",
            "Transparency Blend Space",
            ["Edit", "Transparency Blend Space"],
            None,
            "{space: cmyk|rgb} — the colour space transparency is composited in for output",
            has_doc,
            |s, p| {
                let space: designcraft_doc::BlendSpace = p
                    .get("space")
                    .and_then(|v| serde_json::from_value(v.clone()).ok())
                    .ok_or_else(|| bad("edit.transparencyBlendSpace", "`space`: cmyk or rgb"))?;
                s.edit(|d, _| {
                    d.settings.blend_space = space;
                    Ok(serde_json::json!({"space": space}))
                })
            }
        ),
        cmd!(
            noundo "prefs.set",
            "Preferences",
            [],
            None,
            "{showHiddenCharacters?, typographersQuotes?, polygonSides?, starInset?, scaleStrokes?, dimensionsIncludeStroke?, transformationsAreTotals?, absolutePageNumbers?, highlightHj?, highlightKeeps?, highlightCustomTracking?, highlightSubstitutedFonts?, richBlackOutput?, favoriteFonts?: [family], showFontNamesInEnglish?} → all application preferences",
            super::always,
            |s, p| {
                let cur = serde_json::to_value(&s.prefs).map_err(|e| bad("prefs.set", e.to_string()))?;
                let new = merge("prefs.set", cur, p, None)?;
                s.prefs = serde_json::from_value(new.clone()).map_err(|e| bad("prefs.set", e.to_string()))?;
                Ok(new)
            }
        ),
        cmd!(
            "document.preferences",
            "Document Preferences",
            [],
            None,
            "{horizontalUnits?, verticalUnits?: points|picas|inches|millimeters|…, keyboardIncrement? (pt), baselineGrid?: {start, increment, relativeTo, color, viewThreshold}, grid?: {horizontal, vertical, subdivisions, color, inBack}, pasteboard?: [h, v], marginColor?, columnColor?, bleedColor?, slugColor?: [r, g, b], advancedType?: {superscriptSize, superscriptPosition, subscriptSize, subscriptPosition} (%), overprintBlack?, glyphFallback? (draw characters the font lacks from fallback fonts; off: as the font's missing-glyph box), rulerOrigin?: spread|page|spine (what rulers and X/Y measure from, with the zero point)} → those settings",
            has_doc,
            |s, p| {
                let cur = serde_json::to_value(&s.doc()?.doc.settings).map_err(|e| bad("document.preferences", e.to_string()))?;
                let new = merge("document.preferences", cur, p, Some(DOC_KEYS))?;
                let settings = serde_json::from_value(new.clone()).map_err(|e| bad("document.preferences", e.to_string()))?;
                if p.as_object().is_some_and(|o| !o.is_empty()) {
                    s.edit(|d, _| {
                        d.settings = settings;
                        Ok(Value::Null)
                    })?;
                }
                let out: Map<String, Value> = new
                    .as_object()
                    .into_iter()
                    .flatten()
                    .filter(|(k, _)| DOC_KEYS.contains(&k.as_str()))
                    .map(|(k, v)| (k.clone(), v.clone()))
                    .collect();
                Ok(Value::Object(out))
            }
        ),
    ]
}

/// `cur` with the keys of `p` replaced (objects merged one level deep); unknown keys are errors.
fn merge(cmd: &str, mut cur: Value, p: &Value, allowed: Option<&[&str]>) -> Result<Value> {
    let (Some(c), Some(new)) = (cur.as_object_mut(), p.as_object()) else { return Ok(cur) };
    for (k, v) in new {
        if !c.contains_key(k) || allowed.is_some_and(|a| !a.contains(&k.as_str())) {
            return Err(bad(cmd, format!("unknown preference {k}")));
        }
        match (c.get_mut(k), v) {
            (Some(Value::Object(old)), Value::Object(v)) => {
                for (k2, v2) in v {
                    old.insert(k2.clone(), v2.clone());
                }
            }
            _ => {
                c.insert(k.clone(), v.clone());
            }
        }
    }
    Ok(cur)
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use crate::Session;

    #[test]
    fn application_and_document_preferences() {
        let mut s = Session::new();
        let r = s.execute("prefs.set", &json!({"scaleStrokes": false})).unwrap();
        assert_eq!(r["scaleStrokes"], false);
        assert!(!s.prefs.scale_strokes);
        assert_eq!(r["showFontNamesInEnglish"], false, "native CJK font names by default");
        s.execute("prefs.set", &json!({"showFontNamesInEnglish": true})).unwrap();
        assert!(s.prefs.show_font_names_in_english);
        assert!(s.execute("prefs.set", &json!({"nope": 1})).is_err());
        s.execute("file.new", &json!({})).unwrap();
        let r = s
            .execute("document.preferences", &json!({"horizontalUnits": "millimeters", "keyboardIncrement": 2.5, "grid": {"subdivisions": 4}}))
            .unwrap();
        assert_eq!(r["horizontalUnits"], "millimeters");
        let st = &s.doc().unwrap().doc.settings;
        assert_eq!(st.horizontal_units, designcraft_geom::Unit::Millimeters);
        assert_eq!(st.keyboard_increment, 2.5);
        assert_eq!(st.grid.subdivisions, 4);
        assert_eq!(st.grid.horizontal, 72.0, "the rest of the grid is kept");
        assert!(s.execute("document.preferences", &json!({"pageWidth": 100})).is_err(), "Document Setup's, not Preferences'");
        s.execute("edit.undo", &json!({})).unwrap();
        assert_eq!(s.doc().unwrap().doc.settings.horizontal_units, designcraft_geom::Unit::Picas);
    }
}

#[cfg(test)]
mod advanced_type_tests {
    use serde_json::json;

    use crate::Session;

    #[test]
    fn advanced_type_sizes_superscripts() {
        let mut s = Session::new();
        s.execute("file.new", &json!({})).unwrap();
        let r = s.execute("frame.create", &json!({"rect": [72, 72, 400, 300], "content": "text", "text": "x2"})).unwrap();
        let sid = designcraft_doc::StoryId(r["story"].as_u64().unwrap());
        s.execute("text.select", &json!({"story": r["story"], "anchor": 1, "focus": 2})).unwrap();
        s.execute("type.char", &json!({"position": "superscript"})).unwrap();
        let glyph = |s: &Session| {
            let cs = s.cache.get(&s.doc().unwrap().doc, sid, None);
            let g = cs.frames[0].lines[0].glyphs.iter().find(|g| g.byte == 1).unwrap().clone();
            let base = cs.frames[0].lines[0].glyphs.iter().find(|g| g.byte == 0).unwrap().clone();
            (g.sy / base.sy, base.y - g.y)
        };
        let (k, rise) = glyph(&s);
        assert!((k - 0.583).abs() < 1e-3, "{k}");
        s.execute("document.preferences", &json!({"advancedType": {"superscriptSize": 80, "superscriptPosition": 50}})).unwrap();
        let (k2, rise2) = glyph(&s);
        assert!((k2 - 0.8).abs() < 1e-3, "{k2}");
        assert!(rise2 > rise, "{rise} → {rise2}");
        s.execute("edit.undo", &json!({})).unwrap();
        assert!((glyph(&s).0 - 0.583).abs() < 1e-3);
    }
}

#[cfg(test)]
mod advanced_type_idml_tests {
    use serde_json::json;

    use crate::Session;

    #[test]
    fn advanced_type_round_trips_through_idml() {
        let mut s = Session::new();
        s.execute("file.new", &json!({})).unwrap();
        s.execute("document.preferences", &json!({"advancedType": {"subscriptPosition": 12.5}})).unwrap();
        let back = designcraft_idml::import_idml(&designcraft_idml::export_idml(&s.doc().unwrap().doc)).unwrap();
        assert_eq!(back.settings.advanced_type.subscript_position, 12.5);
        assert_eq!(back.settings.advanced_type.superscript_size, 58.3);
    }
}

#[cfg(test)]
mod highlight_tests {
    use serde_json::json;

    use crate::Session;

    #[test]
    fn hj_violations_and_custom_tracking_are_highlighted_on_screen() {
        let mut s = Session::new();
        s.execute("file.new", &json!({})).unwrap();
        // A narrow justified column of long words: loose lines.
        let r = s
            .execute("frame.create", &json!({"rect": [72, 72, 172, 400], "content": "text", "text": "Incomprehensibilities notwithstanding characteristically uncharacteristically a b"}))
            .unwrap();
        s.execute("text.select", &json!({"story": r["story"], "anchor": 0, "focus": 3})).unwrap();
        s.execute("type.para", &json!({"align": "fullyJustified"})).unwrap();
        s.execute("type.char", &json!({"tracking": 50})).unwrap();
        let sid = designcraft_doc::StoryId(r["story"].as_u64().unwrap());
        let d = s.doc().unwrap().doc.clone();
        let cs = s.cache.get(&d, sid, None);
        assert!(cs.frames[0].lines.iter().any(|l| l.hj > 0), "some line breaks the word-spacing limits");
        assert!(cs.styles.iter().any(|st| st.custom_tracking));
        let render = |hj: bool, tr: bool| {
            let mut r = designcraft_render::Renderer::new();
            r.threads = 0;
            let o = designcraft_render::RenderOptions { highlight_hj: hj, highlight_custom_tracking: tr, ..Default::default() };
            r.render_page(&d, &s.cache, 0, 1.0, false, &o).unwrap()
        };
        let plain = render(false, false);
        let lit = render(true, true);
        let count = |img: &designcraft_render::Rendered, pred: &dyn Fn([u8; 4]) -> bool| {
            (72..172).flat_map(|x| (72..400).map(move |y| (x, y))).filter(|&(x, y)| pred(img.pixel(x, y))).count()
        };
        let yellow = |p: [u8; 4]| p[0] > 240 && p[1] > 200 && p[2] < 190;
        let green = |p: [u8; 4]| p[1] > 220 && p[0] < 200 && p[2] < 200;
        assert_eq!(count(&plain, &yellow), 0);
        assert!(count(&lit, &yellow) > 50, "H&J shading");
        assert!(count(&lit, &green) > 10, "custom tracking shading");
    }
}

#[cfg(test)]
mod keep_highlight_tests {
    use serde_json::json;

    use crate::Session;

    #[test]
    fn unkeepable_paragraph_is_flagged() {
        let mut s = Session::new();
        s.execute("file.new", &json!({})).unwrap();
        let text = "word ".repeat(120);
        let r = s.execute("frame.create", &json!({"rect": [72, 72, 400, 160], "content": "text", "text": text, "caret": false})).unwrap();
        s.execute("object.textFrameOptions", &json!({"ids": [r["id"]], "columns": 2})).unwrap();
        let sid = designcraft_doc::StoryId(r["story"].as_u64().unwrap());
        let flagged = |s: &Session| s.cache.get(&s.doc().unwrap().doc, sid, None).frames[0].lines.iter().any(|l| l.keep_violation);
        assert!(!flagged(&s), "no keep options, no violation");
        s.execute("text.select", &json!({"story": r["story"], "anchor": 0, "focus": 0})).unwrap();
        s.execute("type.para", &json!({"keepLinesTogether": true, "keepAllLines": true})).unwrap();
        assert!(flagged(&s), "too long to keep together in one column");
    }
}

#[cfg(test)]
mod blend_space_tests {
    use serde_json::json;

    use crate::Session;

    #[test]
    fn blend_space_is_set_and_kept_in_idml() {
        let mut s = Session::new();
        s.execute("file.new", &json!({})).unwrap();
        assert_eq!(s.doc().unwrap().doc.settings.blend_space, designcraft_doc::BlendSpace::Cmyk);
        s.execute("edit.transparencyBlendSpace", &json!({"space": "rgb"})).unwrap();
        let back = designcraft_idml::import_idml(&designcraft_idml::export_idml(&s.doc().unwrap().doc)).unwrap();
        assert_eq!(back.settings.blend_space, designcraft_doc::BlendSpace::Rgb);
        assert!(s.execute("edit.transparencyBlendSpace", &json!({"space": "lab"})).is_err());
    }
}

#[cfg(test)]
mod glyph_fallback_tests {
    use serde_json::json;

    use crate::Session;

    #[test]
    fn glyph_fallback_is_an_undoable_document_preference() {
        let mut s = Session::new();
        s.execute("file.new", &json!({})).unwrap();
        assert_eq!(s.execute("document.preferences", &json!({})).unwrap()["glyphFallback"], false, "new documents: off");
        let r = s.execute("frame.create", &json!({"rect": [72, 72, 400, 300], "content": "text", "text": "語"})).unwrap();
        let sid = designcraft_doc::StoryId(r["story"].as_u64().unwrap());
        let gid = |s: &Session| s.cache.get(&s.doc().unwrap().doc, sid, None).frames[0].lines[0].glyphs[0].gid;
        assert_eq!(gid(&s), 0);
        designcraft_fonts::FontDb::global().add_font(designcraft_fonts::testing::font_with("DC Test Prefs Helper", &['語']).unwrap());
        assert_eq!(s.execute("document.preferences", &json!({"glyphFallback": true})).unwrap()["glyphFallback"], true);
        assert!(s.doc().unwrap().doc.settings.glyph_fallback);
        assert_ne!(gid(&s), 0, "drawn from a fallback font");
        s.execute("edit.undo", &json!({})).unwrap();
        assert!(!s.doc().unwrap().doc.settings.glyph_fallback);
        assert_eq!(gid(&s), 0);
    }

    #[test]
    fn idml_imports_draw_missing_glyphs_as_boxes() {
        let mut s = Session::new();
        s.execute("file.new", &json!({})).unwrap();
        s.execute("document.preferences", &json!({"glyphFallback": true})).unwrap();
        let back = designcraft_idml::import_idml(&designcraft_idml::export_idml(&s.doc().unwrap().doc)).unwrap();
        assert!(!back.settings.glyph_fallback, "as InDesign draws them");
    }
}
