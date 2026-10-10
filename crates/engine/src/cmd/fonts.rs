//! Type › Find/Replace Font: the fonts a document uses (missing ones flagged) and replacing one
//! font with another everywhere — local formatting and style definitions.

use std::collections::BTreeMap;
use std::sync::Arc;

use designcraft_doc::{CharAttrs, Document, Story};
use designcraft_fonts::FontSource;
use serde_json::{Value, json};

use super::{CommandSpec, bad, cmd, has_doc, str_param};
use crate::{Result, Session};

pub fn specs() -> Vec<CommandSpec> {
    vec![
        cmd!(query "font.list", "Fonts in Document", [], None, "{} → [{family, style, characters, missing, styleMissing, source: bundled|installed|document|added (null when missing)}] (missing first)", has_doc, |s, _| {
            Ok(Value::Array(list(&s.doc()?.doc)))
        }),
        cmd!(
            "font.replace",
            "Find/Replace Font…",
            ["Type"],
            None,
            "{family, style?, toFamily, toStyle?, redefineStyles?: true} — replaces the font in text and (by default) in paragraph and character styles",
            has_doc,
            replace
        ),
    ]
}

/// Every story's text, including table cells and footnotes.
fn for_each_story(d: &Document, f: &mut dyn FnMut(&Story)) {
    for st in d.stories.values() {
        f(st);
        for t in st.tables.values() {
            for c in &t.cells {
                f(&c.text);
            }
        }
        for n in &st.notes {
            f(&n.text);
        }
    }
}

fn list(d: &Document) -> Vec<Value> {
    let db = designcraft_fonts::FontDb::global().scoped(d.font_scope);
    let mut used: BTreeMap<(String, String), usize> = BTreeMap::new();
    for_each_story(d, &mut |st| {
        let ranges = st.para_ranges();
        for (pi, r) in ranges.iter().enumerate() {
            let (_, base) = d.styles.resolve_para(&st.paras[pi]);
            for (rr, fmt) in st.runs() {
                let n = st.text[rr.start.max(r.start)..rr.end.min(r.end).max(rr.start.max(r.start))].chars().count();
                if n == 0 && !(r.is_empty() && rr.contains(&r.start)) {
                    continue;
                }
                let p = d.styles.resolve_char(&base, fmt);
                if let Some(f) = d.styles.composite_fonts.iter().find(|f| f.name == p.font_family.trim_start_matches("CompositeFont/")) {
                    let text = st.text.get(rr.start.max(r.start)..rr.end.min(r.end).max(rr.start.max(r.start))).unwrap_or("");
                    for c in text.chars() {
                        if let Some(e) = f.entry(c) {
                            *used.entry((e.family.clone(), e.style.clone())).or_default() += 1;
                        }
                    }
                } else {
                    *used.entry((p.font_family, p.font_style)).or_default() += n;
                }
            }
        }
    });
    let mut out: Vec<(bool, Value)> = used
        .into_iter()
        .map(|((family, style), n)| {
            let missing = !db.has_family(&family);
            let style_missing = !missing && !db.styles(&family).iter().any(|s| s.eq_ignore_ascii_case(&style));
            let source = (!missing).then(|| match db.face(&family, &style).source {
                FontSource::Bundled => "bundled",
                FontSource::Installed(_) => "installed",
                FontSource::Document(_) => "document",
                FontSource::Memory => "added",
            });
            (
                missing || style_missing,
                json!({"family": family, "style": style, "characters": n, "missing": missing, "styleMissing": style_missing, "source": source}),
            )
        })
        .collect();
    out.sort_by_key(|(m, _)| !*m);
    out.into_iter().map(|(_, v)| v).collect()
}

/// Replace the font in a sparse attribute set (only where the family is set); true when it changed.
fn swap(a: &mut CharAttrs, family: &str, style: Option<&str>, to_family: &str, to_style: Option<&str>) -> bool {
    let fam_ok = a.font_family.as_deref().is_some_and(|f| f.eq_ignore_ascii_case(family));
    let style_ok = style.is_none_or(|s| a.font_style.as_deref().is_some_and(|x| x.eq_ignore_ascii_case(s)));
    if !fam_ok || !style_ok {
        return false;
    }
    a.font_family = Some(to_family.to_string());
    if let Some(ts) = to_style {
        a.font_style = Some(ts.to_string());
    }
    true
}

fn replace(s: &mut Session, p: &Value) -> Result<Value> {
    let family = str_param(p, "family").ok_or_else(|| bad("font.replace", "missing family"))?.to_string();
    let style = str_param(p, "style").map(str::to_string);
    let to_family = str_param(p, "toFamily").ok_or_else(|| bad("font.replace", "missing toFamily"))?.to_string();
    let to_style = str_param(p, "toStyle").map(str::to_string);
    let styles_too = p.get("redefineStyles").and_then(Value::as_bool).unwrap_or(true);
    s.edit(|d, _| {
        let mut changed = 0usize;
        if styles_too {
            let st = d.styles_mut();
            for ps in &mut st.paragraph {
                changed += swap(&mut ps.chars, &family, style.as_deref(), &to_family, to_style.as_deref()) as usize;
            }
            for cs in &mut st.character {
                changed += swap(&mut cs.chars, &family, style.as_deref(), &to_family, to_style.as_deref()) as usize;
            }
            for f in &mut st.composite_fonts {
                for e in &mut f.entries {
                    if e.family.eq_ignore_ascii_case(&family) && style.as_ref().is_none_or(|s| e.style.eq_ignore_ascii_case(s)) {
                        e.family.clone_from(&to_family);
                        if let Some(s) = &to_style {
                            e.style.clone_from(s);
                        }
                        changed += 1;
                    }
                }
            }
        }
        // Local formatting in every story (cells and footnotes too).
        let fix = |st: &mut Story, changed: &mut usize| {
            let mut any = false;
            for r in &mut st.chars {
                if swap(&mut r.format.over, &family, style.as_deref(), &to_family, to_style.as_deref()) {
                    any = true;
                    *changed += 1;
                }
            }
            if any {
                st.rev += 1;
            }
        };
        let ids: Vec<_> = d.stories.keys().copied().collect();
        for sid in ids {
            let Some(st) = d.story_mut(sid) else { continue };
            fix(st, &mut changed);
            for t in st.tables.values_mut() {
                for c in &mut Arc::make_mut(t).cells {
                    fix(&mut c.text, &mut changed);
                }
            }
            for n in &mut st.notes {
                fix(&mut Arc::make_mut(n).text, &mut changed);
            }
        }
        Ok(json!({"changed": changed}))
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn find_and_replace_font() {
        let mut s = Session::new();
        s.execute("file.new", &json!({})).unwrap();
        let r = s.execute("frame.create", &json!({"rect": [72, 72, 400, 300], "content": "text", "text": "Some text here."})).unwrap();
        let sid = s.doc().unwrap().doc.item(designcraft_doc::ItemId(r["id"].as_u64().unwrap())).unwrap().text_frame().unwrap().story;
        s.execute("text.select", &json!({"story": sid.0, "anchor": 0, "focus": 4})).unwrap();
        s.execute("type.char", &json!({"fontFamily": "Nonexistent Sans", "fontStyle": "Bold"})).unwrap();
        let l = s.execute("font.list", &json!({})).unwrap();
        assert_eq!(l[0]["family"], "Nonexistent Sans", "missing fonts first: {l}");
        assert_eq!(l[0]["missing"], true);
        assert_eq!(l[0]["characters"], 4);
        // Replace it with an available family.
        let avail = l.as_array().unwrap().iter().find(|f| f["missing"] == false).unwrap()["family"].as_str().unwrap().to_string();
        let r = s.execute("font.replace", &json!({"family": "Nonexistent Sans", "toFamily": avail, "toStyle": "Regular"})).unwrap();
        assert_eq!(r["changed"], 1);
        let l = s.execute("font.list", &json!({})).unwrap();
        assert!(l.as_array().unwrap().iter().all(|f| f["missing"] == false), "{l}");
        // Style definitions are redefined too.
        s.execute("style.paragraph.create", &json!({"name": "Odd", "chars": {"fontFamily": "Nonexistent Sans"}})).unwrap();
        let r = s.execute("font.replace", &json!({"family": "Nonexistent Sans", "toFamily": avail})).unwrap();
        assert_eq!(r["changed"], 1);
        assert_eq!(s.doc().unwrap().doc.styles.para("Odd").unwrap().chars.font_family.as_deref(), Some(avail.as_str()));
    }

    /// InDesign names a family installed in several formats `Family (OTF)`; the plain family
    /// stands for it, and a family that isn't available under either name is still missing.
    #[test]
    fn a_family_with_a_format_suffix_uses_the_plain_family() {
        let mut s = Session::new();
        s.execute("file.new", &json!({})).unwrap();
        let r = s.execute("frame.create", &json!({"rect": [72, 72, 400, 300], "content": "text", "text": "Some text here."})).unwrap();
        let sid = s.doc().unwrap().doc.item(designcraft_doc::ItemId(r["id"].as_u64().unwrap())).unwrap().text_frame().unwrap().story;
        s.execute("text.select", &json!({"story": sid.0, "anchor": 0, "focus": 15})).unwrap();
        s.execute("type.char", &json!({"fontFamily": "Source Serif 4 (OTF)", "fontStyle": "Bold"})).unwrap();
        let l = s.execute("font.list", &json!({})).unwrap();
        let entry = l.as_array().unwrap().iter().find(|f| f["family"] == "Source Serif 4 (OTF)").unwrap();
        assert_eq!((&entry["missing"], &entry["styleMissing"], &entry["source"]), (&json!(false), &json!(false), &json!("bundled")), "{l}");
        let st = s.doc().unwrap();
        let cs = s.cache.get(&st.doc, sid, None);
        let faces: Vec<_> =
            cs.frames.iter().flat_map(|f| &f.lines).flat_map(|l| &l.glyphs).filter(|g| g.visible && g.len > 0).map(|g| g.face.get()).collect();
        assert!(!faces.is_empty() && faces.iter().all(|f| f.family == "Source Serif 4" && f.style == "Bold"));
        let kinds = |s: &mut Session| {
            s.execute("preflight.run", &json!({})).unwrap()["issues"]
                .as_array()
                .unwrap()
                .iter()
                .map(|i| i["kind"].as_str().unwrap().to_string())
                .collect::<Vec<_>>()
        };
        assert!(!kinds(&mut s).iter().any(|k| k == "missingFont"));

        s.execute("text.select", &json!({"story": sid.0, "anchor": 0, "focus": 4})).unwrap();
        s.execute("type.char", &json!({"fontFamily": "Nope (OTF)"})).unwrap();
        let l = s.execute("font.list", &json!({})).unwrap();
        assert_eq!((&l[0]["family"], &l[0]["missing"]), (&json!("Nope (OTF)"), &json!(true)), "{l}");
        assert!(kinds(&mut s).iter().any(|k| k == "missingFont"));
    }
}
