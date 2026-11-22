//! Object › Stroke Styles: named custom dash, dot and stripe styles.

use designcraft_doc::{StrokeStyleDef, StrokeType};
use serde_json::{Value, json};

use super::{CommandSpec, bad, cmd, has_doc, ok, str_param};

pub fn specs() -> Vec<CommandSpec> {
    vec![
        cmd!(query "strokeStyle.list", "Stroke Styles", [], None, "{} → [{name, kind}]", has_doc, |s, _| {
            Ok(serde_json::to_value(&s.doc()?.doc.stroke_styles).unwrap_or_default())
        }),
        cmd!(
            "strokeStyle.new",
            "New Stroke Style…",
            ["Object", "Stroke Styles"],
            None,
            "{name, type: stripes|dash|dot, bands?: [[start, width] (fractions of the weight)], pattern?: [dash, gap, …] (pt)} — apply with object.stroke {type: {kind: \"style\", name}}",
            has_doc,
            |s, p| {
                const ID: &str = "strokeStyle.new";
                let name = str_param(p, "name").map(str::trim).filter(|n| !n.is_empty()).ok_or_else(|| bad(ID, "`name` required"))?.to_string();
                let nums = |v: &Value| v.as_array().map(|a| a.iter().filter_map(Value::as_f64).collect::<Vec<_>>()).unwrap_or_default();
                let kind = match str_param(p, "type").unwrap_or("stripes") {
                    "stripes" => {
                        let bands: Vec<(f64, f64)> = p
                            .get("bands")
                            .and_then(Value::as_array)
                            .map(|a| {
                                a.iter()
                                    .filter_map(|b| {
                                        let v = nums(b);
                                        (v.len() == 2).then(|| (v[0].clamp(0.0, 1.0), v[1].clamp(0.0, 1.0)))
                                    })
                                    .collect()
                            })
                            .unwrap_or_default();
                        if bands.is_empty() || bands.iter().any(|(a, w)| a + w > 1.0 + 1e-9 || *w <= 0.0) {
                            return Err(bad(ID, "stripes need `bands`: [[start, width], …] inside 0–1"));
                        }
                        StrokeType::Stripes { bands }
                    }
                    "dash" => {
                        let pattern = p.get("pattern").map(nums).unwrap_or_else(|| vec![12.0, 4.0]);
                        if !StrokeType::valid_dash_pattern(&pattern) {
                            return Err(bad(ID, "a dash `pattern` needs 1 to 64 lengths, each 0 or 0.1 to 10000 pt, not all 0"));
                        }
                        StrokeType::Dashed { pattern }
                    }
                    "dot" => StrokeType::Dotted,
                    t => return Err(bad(ID, format!("unknown type `{t}` (stripes, dash, dot)"))),
                };
                s.edit(|d, _| {
                    d.stroke_styles.retain(|x| x.name != name);
                    d.stroke_styles.push(StrokeStyleDef { name: name.clone(), kind });
                    Ok(json!({"name": name}))
                })
            }
        ),
        cmd!("strokeStyle.delete", "Delete Stroke Style", [], None, "{name} — strokes using it become solid", has_doc, |s, p| {
            let name = str_param(p, "name").unwrap_or("").to_string();
            s.edit(|d, _| {
                let n = d.stroke_styles.len();
                d.stroke_styles.retain(|x| x.name != name);
                if d.stroke_styles.len() == n {
                    return Err(bad("strokeStyle.delete", format!("no stroke style `{name}`")));
                }
                ok()
            })
        }),
    ]
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use crate::Session;

    #[test]
    fn custom_stripes_render_with_gaps_and_round_trip() {
        let mut s = Session::new();
        s.execute("file.new", &json!({})).unwrap();
        s.execute("strokeStyle.new", &json!({"name": "Rails", "type": "stripes", "bands": [[0, 0.3], [0.7, 0.3]]})).unwrap();
        let id =
            s.execute("frame.create", &json!({"rect": [100, 100, 300, 300], "shape": "rectangle", "content": "unassigned"})).unwrap()["id"].clone();
        s.execute("object.stroke", &json!({"ids": [id], "weight": 20, "swatch": "[Black]", "type": {"kind": "style", "name": "Rails"}})).unwrap();
        let d = s.doc().unwrap().doc.clone();
        let mut r = designcraft_render::Renderer::new();
        r.threads = 0;
        let img = r.render_page(&d, &s.cache, 0, 1.0, false, &Default::default()).unwrap();
        // The left edge at x = 100: rails at 90–96 and 104–110, a gap in between.
        let dark = |x: u32| img.pixel(x, 200)[0] < 100;
        assert!(dark(92) && dark(108), "rails");
        assert!(!dark(100), "the gap");
        let pdf = s.execute("file.exportPdf", &json!({})).unwrap();
        assert!(pdf["warnings"].as_array().is_none_or(|w| w.iter().all(|x| !x.as_str().unwrap_or("").contains("striped"))));
        let back = designcraft_idml::import_idml(&designcraft_idml::export_idml(&d)).unwrap();
        assert_eq!(back.stroke_styles.len(), 1, "IDML StripedStrokeStyle");
        let it = back.spreads[0].items.iter().find(|i| i.stroke.weight == 20.0).unwrap();
        assert_eq!(it.stroke.kind, designcraft_doc::StrokeType::Style { name: "Rails".into() });
        s.execute("strokeStyle.delete", &json!({"name": "Rails"})).unwrap();
        assert!(s.execute("strokeStyle.new", &json!({"name": "Bad", "bands": [[0.5, 0.7]]})).is_err());
    }
}
