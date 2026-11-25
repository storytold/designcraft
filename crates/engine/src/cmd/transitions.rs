//! Window › Interactive › Page Transitions: a transition per spread for interactive PDF.

use std::sync::Arc;

use designcraft_doc::{PageTransition, TransitionKind};
use serde_json::{Value, json};

use super::{CommandSpec, bad, cmd, has_doc};
use crate::Result;

fn spreads_of(d: &designcraft_doc::Document, p: &Value) -> Result<Vec<usize>> {
    if p.get("all").and_then(Value::as_bool) == Some(true) {
        return Ok((0..d.spreads.len()).collect());
    }
    if let Some(si) = p.get("spread").and_then(Value::as_u64) {
        return Ok(vec![si as usize]);
    }
    let pages: Vec<usize> =
        p.get("pages").and_then(Value::as_array).map(|a| a.iter().filter_map(Value::as_u64).map(|n| n as usize).collect()).unwrap_or_default();
    if pages.is_empty() {
        return Err(bad("page.transition", "give `pages` (1-based), `spread` or `all`"));
    }
    let mut v: Vec<usize> = pages.iter().filter_map(|n| n.checked_sub(1).and_then(|i| d.page_loc(i)).map(|(si, _)| si)).collect();
    v.sort_unstable();
    v.dedup();
    Ok(v)
}

pub fn specs() -> Vec<CommandSpec> {
    vec![
        cmd!(
            "page.transition",
            "Page Transition",
            [],
            None,
            "{pages?: [1-based] | spread? | all?: bool, kind: blinds|box|comb|cover|dissolve|fade|push|split|uncover|wipe|zoomIn|zoomOut|none, duration?: seconds (1), horizontal?: bool} — for interactive PDF",
            has_doc,
            |s, p| {
                let kind = p.get("kind").and_then(Value::as_str).unwrap_or("none");
                let t = if kind == "none" {
                    None
                } else {
                    let k: TransitionKind =
                        serde_json::from_value(json!(kind)).map_err(|_| bad("page.transition", format!("unknown kind `{kind}`")))?;
                    Some(PageTransition {
                        kind: k,
                        duration: p.get("duration").and_then(Value::as_f64).unwrap_or(1.0).clamp(0.0, 60.0),
                        horizontal: p.get("horizontal").and_then(Value::as_bool).unwrap_or(false),
                    })
                };
                let list = spreads_of(&s.doc()?.doc, p)?;
                s.edit(|d, _| {
                    for si in &list {
                        let sp = d.spreads.get_mut(*si).ok_or_else(|| bad("page.transition", format!("no spread {si}")))?;
                        if let Some(pg) = Arc::make_mut(sp).pages.first_mut() {
                            pg.transition = t;
                        }
                    }
                    Ok(json!({"spreads": list.len()}))
                })
            }
        ),
        cmd!(query "page.transitions", "Page Transitions", [], None, "{} → [{spread, kind, duration, horizontal}]", has_doc, |s, _| {
            let d = &s.doc()?.doc;
            Ok(Value::Array(
                d.spreads
                    .iter()
                    .enumerate()
                    .filter_map(|(si, sp)| {
                        let t = sp.pages.first()?.transition?;
                        Some(json!({"spread": si, "kind": t.kind, "duration": t.duration, "horizontal": t.horizontal}))
                    })
                    .collect(),
            ))
        }),
    ]
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use crate::Session;

    #[test]
    fn transitions_go_into_the_pdf() {
        let mut s = Session::new();
        s.execute("file.new", &json!({"pages": 3})).unwrap();
        s.execute("page.transition", &json!({"pages": [2], "kind": "wipe", "duration": 2})).unwrap();
        assert_eq!(s.execute("page.transitions", &json!({})).unwrap()[0]["kind"], "wipe");
        let r = s.execute("file.exportPdf", &json!({})).unwrap();
        let bytes = super::super::file::base64_decode(r["base64"].as_str().unwrap());
        let text = String::from_utf8_lossy(&bytes);
        // Pages 2 and 3 share a spread, and the transition is the spread's.
        assert_eq!(text.matches("/Trans<<").count(), 2);
        assert!(text.contains("/S/Wipe") && text.contains("/D 2.00"));
        // The updated file still reads, with all its pages.
        assert_eq!(designcraft_render::pdf_page_count(&bytes), Some(3));
        // Exported as spreads: one PDF page.
        let r = s.execute("file.exportPdf", &json!({"spreads": true})).unwrap();
        let bytes = super::super::file::base64_decode(r["base64"].as_str().unwrap());
        assert_eq!(String::from_utf8_lossy(&bytes).matches("/Trans<<").count(), 1);
        // With transparency too, the page keeps both its group and its transition.
        let id = s.execute("frame.create", &json!({"rect": [100, 100, 200, 200]})).unwrap()["id"].clone();
        s.execute("object.opacity", &json!({"ids": [id], "opacity": 0.5})).unwrap();
        let r = s.execute("file.exportPdf", &json!({"pages": [1]})).unwrap();
        let bytes = super::super::file::base64_decode(r["base64"].as_str().unwrap());
        assert_eq!(designcraft_render::pdf_page_count(&bytes), Some(1));
        s.execute("page.transition", &json!({"pages": [1], "kind": "fade"})).unwrap();
        let r = s.execute("file.exportPdf", &json!({"pages": [1]})).unwrap();
        let text = String::from_utf8_lossy(&super::super::file::base64_decode(r["base64"].as_str().unwrap())).to_string();
        let last = &text[text.rfind("/Type/Page/").unwrap()..];
        assert!(last.contains("/S/Fade") && last[..last.find("endobj").unwrap()].contains("/S/Transparency"), "{last}");
        s.execute("page.transition", &json!({"all": true, "kind": "none"})).unwrap();
        assert!(s.execute("page.transitions", &json!({})).unwrap().as_array().unwrap().is_empty());
    }

    #[test]
    fn unsorted_pages_count_each_transition_spread_once() {
        let mut s = Session::new();
        s.execute("file.new", &json!({"pages": 3})).unwrap();
        // Pages 2 and 3 share a spread, with page 1's spread between them in the request.
        let result = s.execute("page.transition", &json!({"pages": [2, 1, 3], "kind": "fade"})).unwrap();
        assert_eq!(result["spreads"], 2);
        let transitions = s.execute("page.transitions", &json!({})).unwrap();
        assert_eq!(transitions.as_array().unwrap().len(), 2);
        assert_eq!(transitions[0]["spread"], 0);
        assert_eq!(transitions[1]["spread"], 1);
        assert!(transitions.as_array().unwrap().iter().all(|t| t["kind"] == "fade"));
        s.execute("edit.undo", &json!({})).unwrap();
        assert!(s.execute("page.transitions", &json!({})).unwrap().as_array().unwrap().is_empty());
    }
}
