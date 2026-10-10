//! Hyperlinks and bookmarks (Window → Interactive), exported to PDF as link annotations and the outline.

use designcraft_doc::{Bookmark, Hyperlink, HyperlinkDest, HyperlinkSource};
use serde_json::{Value, json};

use super::{CommandSpec, bad, cmd, has_doc, str_param};
use crate::{Result, Session};

pub fn specs() -> Vec<CommandSpec> {
    vec![
        cmd!(
            "hyperlink.create",
            "New Hyperlink…",
            ["Type", "Hyperlinks & Cross-References"],
            None,
            "{url? | email? | page? (1-based), name?, ids?} — from the selected text, or the selected frames",
            has_doc,
            create
        ),
        cmd!("hyperlink.delete", "Delete Hyperlink", [], None, "{id}", has_doc, |s, p| {
            let id = p.get("id").and_then(Value::as_u64).ok_or_else(|| bad("hyperlink.delete", "missing id"))?;
            s.edit(|d, _| {
                d.hyperlinks.retain(|h| h.id != id);
                Ok(Value::Null)
            })
        }),
        cmd!(query "hyperlink.list", "Hyperlinks", [], None, "{}", has_doc, |s, _| Ok(serde_json::to_value(&s.doc()?.doc.hyperlinks).unwrap_or_default())),
        cmd!("hyperlink.edit", "Hyperlink Options…", [], None, "{id, name?, url? | email? | page? (1-based)}", has_doc, |s, p| {
            let id = p.get("id").and_then(Value::as_u64).ok_or_else(|| bad("hyperlink.edit", "missing id"))?;
            let dest = dest_param(p);
            let name = str_param(p, "name").map(str::to_string);
            s.edit(|d, _| {
                let h = d.hyperlinks.iter_mut().find(|h| h.id == id).ok_or_else(|| bad("hyperlink.edit", format!("no hyperlink {id}")))?;
                if let Some(n) = name {
                    h.name = n;
                }
                if let Some(dst) = dest {
                    h.dest = dst;
                }
                Ok(Value::Null)
            })
        }),
        cmd!(noundo "hyperlink.goToSource", "Go To Source", [], None, "{id} — selects the hyperlink's text or frame", has_doc, |s, p| {
            let id = p.get("id").and_then(Value::as_u64).ok_or_else(|| bad("hyperlink.goToSource", "missing id"))?;
            let st = s.doc_mut()?;
            let h = st.doc.hyperlinks.iter().find(|h| h.id == id).cloned().ok_or_else(|| bad("hyperlink.goToSource", format!("no hyperlink {id}")))?;
            st.selection = match h.source {
                HyperlinkSource::Item { id } => designcraft_doc::Selection::items(vec![id]),
                HyperlinkSource::Text { story, start, end } => {
                    let frame = st.doc.story(story).and_then(|x| x.frames.first().copied());
                    designcraft_doc::Selection::text(designcraft_doc::TextSel { story, anchor: start, focus: end, frame, cell: None })
                }
            };
            st.bump_revision();
            Ok(Value::Null)
        }),
        cmd!("bookmark.rename", "Rename Bookmark", [], None, "{index, name}", has_doc, |s, p| {
            let i = p.get("index").and_then(Value::as_u64).ok_or_else(|| bad("bookmark.rename", "missing index"))? as usize;
            let name =
                str_param(p, "name").map(str::trim).filter(|n| !n.is_empty()).ok_or_else(|| bad("bookmark.rename", "missing name"))?.to_string();
            s.edit(|d, _| {
                let b = d.bookmarks.get_mut(i).ok_or_else(|| bad("bookmark.rename", format!("no bookmark {i}")))?;
                b.name = name;
                Ok(Value::Null)
            })
        }),
        cmd!("bookmark.add", "New Bookmark", [], None, "{name?, page? (1-based; default: current selection's page or 1)}", has_doc, |s, p| {
            let page = p.get("page").and_then(Value::as_u64).map(|v| (v as usize).saturating_sub(1)).unwrap_or(0);
            let n = s.doc()?.doc.page_count();
            if page >= n {
                return Err(bad("bookmark.add", format!("no page {}", page + 1)));
            }
            let name = str_param(p, "name").map(str::to_string);
            s.edit(|d, _| {
                let name = name.clone().unwrap_or_else(|| format!("Bookmark {}", d.bookmarks.len() + 1));
                d.bookmarks.push(Bookmark { name, page, children: vec![] });
                Ok(json!({"index": d.bookmarks.len() - 1}))
            })
        }),
        cmd!("bookmark.delete", "Delete Bookmark", [], None, "{index}", has_doc, |s, p| {
            let i = p.get("index").and_then(Value::as_u64).ok_or_else(|| bad("bookmark.delete", "missing index"))? as usize;
            s.edit(|d, _| {
                if i < d.bookmarks.len() {
                    d.bookmarks.remove(i);
                }
                Ok(Value::Null)
            })
        }),
        cmd!(query "bookmark.list", "Bookmarks", [], None, "{}", has_doc, |s, _| Ok(serde_json::to_value(&s.doc()?.doc.bookmarks).unwrap_or_default())),
    ]
}

fn dest_param(p: &Value) -> Option<HyperlinkDest> {
    if let Some(u) = str_param(p, "url") {
        Some(HyperlinkDest::Url(u.to_string()))
    } else if let Some(e) = str_param(p, "email") {
        Some(HyperlinkDest::Email(e.to_string()))
    } else {
        p.get("page").and_then(Value::as_u64).map(|pg| HyperlinkDest::Page((pg as usize).saturating_sub(1)))
    }
}

fn create(s: &mut Session, p: &Value) -> Result<Value> {
    let dest = dest_param(p).ok_or_else(|| bad("hyperlink.create", "give `url`, `email` or `page`"))?;
    let st = s.doc()?;
    let sources: Vec<HyperlinkSource> = if let Some(ids) = super::ids_param(p, "ids") {
        ids.into_iter().map(|id| HyperlinkSource::Item { id }).collect()
    } else if let Some(t) = st.selection.text.filter(|t| !t.is_caret()) {
        vec![HyperlinkSource::Text { story: t.story, start: t.range().start, end: t.range().end }]
    } else {
        st.selection.items.iter().map(|id| HyperlinkSource::Item { id: *id }).collect()
    };
    if sources.is_empty() {
        return Err(bad("hyperlink.create", "select text or frames first"));
    }
    let name = str_param(p, "name").map(str::to_string);
    s.edit(|d, _| {
        let mut ids = vec![];
        for src in sources {
            let id = d.alloc();
            let name = name.clone().unwrap_or_else(|| match &dest {
                HyperlinkDest::Url(u) => u.clone(),
                HyperlinkDest::Email(e) => e.clone(),
                HyperlinkDest::Page(p) => format!("Page {}", p + 1),
            });
            d.hyperlinks.push(Hyperlink { id, name, source: src, dest: dest.clone() });
            ids.push(id);
        }
        Ok(json!({"ids": ids}))
    })
}

#[cfg(test)]
mod panel_tests {
    use serde_json::json;

    use crate::Session;

    #[test]
    fn hyperlinks_edit_and_go_to_source_bookmarks_rename() {
        let mut s = Session::new();
        s.execute("file.new", &json!({"pages": 2})).unwrap();
        let r = s.execute("frame.create", &json!({"rect": [72, 72, 400, 300], "content": "text", "text": "Visit us"})).unwrap();
        s.execute("text.select", &json!({"story": r["story"], "anchor": 6, "focus": 8})).unwrap();
        let h = s.execute("hyperlink.create", &json!({"url": "https://a.example"})).unwrap()["ids"][0].clone();
        s.execute("hyperlink.edit", &json!({"id": h, "url": "https://b.example", "name": "Site"})).unwrap();
        let l = s.execute("hyperlink.list", &json!({})).unwrap();
        assert_eq!(l[0]["name"], "Site");
        assert!(l[0].to_string().contains("b.example"));
        s.execute("edit.deselectAll", &json!({})).unwrap();
        s.execute("hyperlink.goToSource", &json!({"id": h})).unwrap();
        let t = s.doc().unwrap().selection.text.unwrap();
        assert_eq!((t.anchor, t.focus), (6, 8));
        s.execute("bookmark.add", &json!({"page": 2})).unwrap();
        s.execute("bookmark.rename", &json!({"index": 0, "name": "Back"})).unwrap();
        assert_eq!(s.execute("bookmark.list", &json!({})).unwrap()[0]["name"], "Back");
    }
}
