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
            "{url? | email? | page? (1-based) | anchor? (a text anchor's id or name; the link follows it as the text reflows), name?, ids?} — from the selected text, or the selected frames",
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
        cmd!("hyperlink.edit", "Hyperlink Options…", [], None, "{id, name?, url? | email? | page? (1-based) | anchor?}", has_doc, |s, p| {
            let id = p.get("id").and_then(Value::as_u64).ok_or_else(|| bad("hyperlink.edit", "missing id"))?;
            let dest = dest_param("hyperlink.edit", &s.doc()?.doc, p)?;
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

/// `url`, `email`, `page` (1-based) or `anchor` (a text anchor's id, or its name). `None` when
/// none of them is given; an error when one is given and cannot be used, which `page 0` and a
/// page past the end both were — they became page 1 and a link the export then left out.
fn dest_param(id: &str, d: &designcraft_doc::Document, p: &Value) -> Result<Option<HyperlinkDest>> {
    if let Some(u) = str_param(p, "url") {
        return Ok(Some(HyperlinkDest::Url(u.to_string())));
    }
    if let Some(e) = str_param(p, "email") {
        return Ok(Some(HyperlinkDest::Email(e.to_string())));
    }
    if let Some(a) = p.get("anchor").filter(|a| !a.is_null()) {
        let by_name = |name: &str| d.stories.values().flat_map(|st| st.anchors.iter()).find(|x| x.name == name).map(|x| x.id);
        let found =
            a.as_u64().filter(|id| d.stories.values().any(|st| st.anchors.iter().any(|x| x.id == *id))).or_else(|| a.as_str().and_then(by_name));
        return match found {
            Some(anchor) => Ok(Some(HyperlinkDest::Anchor(anchor))),
            None => Err(bad(id, format!("no text anchor {a} (anchor.list names them)"))),
        };
    }
    match p.get("page").filter(|v| !v.is_null()) {
        None => Ok(None),
        Some(pg) => {
            let pages = d.page_count();
            match pg.as_u64().map(|n| n as usize) {
                Some(n) if n >= 1 && n <= pages => Ok(Some(HyperlinkDest::Page(n - 1))),
                _ => Err(bad(id, format!("page {pg}: the document has {pages} page(s), numbered from 1"))),
            }
        }
    }
}

/// The anchor's own name, for a link to one that was not given a name.
fn dest_label(d: &designcraft_doc::Document, dest: &HyperlinkDest) -> String {
    if let HyperlinkDest::Anchor(a) = dest
        && let Some(x) = d.stories.values().flat_map(|st| st.anchors.iter()).find(|x| x.id == *a)
    {
        return x.name.clone();
    }
    dest.label()
}

fn create(s: &mut Session, p: &Value) -> Result<Value> {
    let st = s.doc()?;
    let dest = dest_param("hyperlink.create", &st.doc, p)?.ok_or_else(|| bad("hyperlink.create", "give `url`, `email`, `page` or `anchor`"))?;
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
            let name = name.clone().unwrap_or_else(|| dest_label(d, &dest));
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

#[cfg(test)]
mod anchor_dest_tests {
    use serde_json::json;

    use crate::Session;

    /// The page object numbers of a PDF, in order, and the page each link annotation points at
    /// (as an index into that list).
    fn link_pages(s: &mut Session) -> Vec<usize> {
        let r = s.execute("file.exportPdf", &json!({})).unwrap();
        let bytes = super::super::file::base64_decode(r["base64"].as_str().unwrap());
        let pdf = String::from_utf8_lossy(&bytes).into_owned();
        let kids: Vec<&str> = pdf
            .split("/Kids[")
            .nth(1)
            .and_then(|k| k.split(']').next())
            .map(|k| k.split(" 0 R").map(str::trim).filter(|s| !s.is_empty()).collect())
            .unwrap_or_default();
        let object =
            |n: &str| -> String { pdf.split(&format!("\n{n} 0 obj\n")).nth(1).and_then(|o| o.split("\nendobj").next()).unwrap_or("").to_string() };
        let mut out = Vec::new();
        for part in pdf.split("/Subtype/Link").skip(1) {
            let Some(dest) = part.split("/Dest ").nth(1).and_then(|d| d.split(" 0 R").next()) else { continue };
            // `[<page> 0 R/XYZ …]`
            let page = object(dest.trim());
            let Some(page) = page.trim_start_matches('[').split(" 0 R").next() else { continue };
            if let Some(i) = kids.iter().position(|k| *k == page.trim()) {
                out.push(i);
            }
        }
        out
    }

    /// A frame can link to a text anchor, as a cross-reference refers to one, and the link lands
    /// on whichever page the anchor's text is on when the PDF is written — IDML files link photo
    /// thumbnails to their full-size pictures that way (#1446).
    #[test]
    fn a_frame_can_link_to_a_text_anchor_and_follows_it() {
        let mut s = Session::new();
        s.execute("file.new", &json!({"width": 300, "height": 300, "pages": 3, "facingPages": false, "margins": 0})).unwrap();
        // The destination: an anchor in text on page 3.
        let sp = s.doc().unwrap().doc.page_loc(2).unwrap().0;
        let dest =
            s.execute("frame.create", &json!({"spread": sp, "rect": [20, 20, 280, 100], "content": "text", "text": "Photo 1 full size"})).unwrap();
        s.execute("text.select", &json!({"story": dest["story"].clone(), "anchor": 0, "focus": 7})).unwrap();
        let anchor = s.execute("anchor.create", &json!({"name": "Photo 1"})).unwrap();
        // The source: a thumbnail frame on page 1, linked by the anchor's name.
        let thumb = s.execute("frame.create", &json!({"rect": [20, 20, 120, 80], "content": "unassigned"})).unwrap();
        s.execute("hyperlink.create", &json!({"anchor": "Photo 1", "ids": [thumb["id"].clone()]})).unwrap();
        let links = s.execute("hyperlink.list", &json!({})).unwrap();
        assert_eq!(links[0]["dest"], json!({"kind": "anchor", "value": anchor["id"].clone()}), "{links}");
        // Named after the anchor, since no name was given.
        assert_eq!(links[0]["name"], "Photo 1");
        assert_eq!(link_pages(&mut s), vec![2], "the link lands on the anchor's page (the third)");

        // A page inserted in front moves the anchor's text to page 4, and the link follows it
        // without being edited.
        s.execute("layout.pages.insert", &json!({"count": 1, "after": 0})).unwrap();
        assert_eq!(s.execute("anchor.list", &json!({})).unwrap()[0]["page"], "4");
        assert_eq!(link_pages(&mut s), vec![3], "the link follows the text");

        // A page number the document does not have is refused, rather than becoming page 1 or a
        // link the export quietly leaves out.
        let dest_err = |s: &mut Session, dest: serde_json::Value| {
            let mut params = dest;
            params["ids"] = json!([thumb["id"].clone()]);
            s.execute("hyperlink.create", &params).expect_err("refused").to_string()
        };
        for (params, wanted) in [
            (json!({"page": 0}), "numbered from 1"),
            (json!({"page": 99}), "the document has 4 page(s)"),
            (json!({"anchor": 12345}), "no text anchor"),
            (json!({"anchor": "No Such Anchor"}), "no text anchor"),
        ] {
            let e = dest_err(&mut s, params.clone());
            assert!(e.contains(wanted), "{params}: {e}");
        }
        // The pages it does have still work, 1-based.
        let made = s.execute("hyperlink.create", &json!({"page": 4, "ids": [thumb["id"].clone()]})).unwrap()["ids"][0].clone();
        let links = s.execute("hyperlink.list", &json!({})).unwrap();
        let link = links.as_array().unwrap().iter().find(|h| h["id"] == made).expect("the new link");
        assert_eq!(link["dest"], json!({"kind": "page", "value": 3}), "page 4 is index 3");
        s.execute("hyperlink.delete", &json!({"id": made})).unwrap();

        // An anchor's id works as well as its name, and an unknown name is refused.
        s.execute("hyperlink.delete", &json!({"id": links[0]["id"].clone()})).unwrap();
        s.execute("hyperlink.create", &json!({"anchor": anchor["id"].clone(), "ids": [thumb["id"].clone()], "name": "Thumbnail"})).unwrap();
        assert_eq!(s.execute("hyperlink.list", &json!({})).unwrap()[0]["name"], "Thumbnail");
        assert_eq!(link_pages(&mut s), vec![3]);
        assert!(s.execute("hyperlink.create", &json!({"anchor": "No Such Anchor", "ids": [thumb["id"].clone()]})).is_err());

        // A link to a deleted anchor is not exported, rather than pointing at the wrong page.
        s.execute("text.select", &json!({"story": dest["story"].clone(), "anchor": 0, "focus": 8})).unwrap();
        s.execute("text.delete", &json!({})).unwrap();
        assert!(s.execute("anchor.list", &json!({})).unwrap().as_array().unwrap().is_empty());
        assert_eq!(link_pages(&mut s), Vec::<usize>::new(), "no link rather than a wrong one");
    }
}
