//! Hyperlinks and bookmarks (Window → Interactive), exported to PDF as link annotations and the outline.

use designcraft_doc::{Bookmark, Hyperlink, HyperlinkDest, HyperlinkSource};
use serde_json::{Value, json};

use super::{CommandSpec, bad, cmd, has_doc, str_param};
use crate::{Result, Session};

/// Register commands for editing hyperlinks and manual bookmarks.
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
                d.bookmarks.push(Bookmark { name, page, children: vec![], generated_by_toc: false });
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

/// `url`, `email`, `page` (1-based) or `anchor` (a text anchor's id, or its name).
///
/// `None` is "no destination given", which `hyperlink.edit` needs for an edit that only renames.
/// An `anchor` that is given and names none is an error, not `None`: an unknown name made
/// `hyperlink.create` fall through to `page` and an unknown id was stored as it stood, so the
/// link went to an anchor nobody has and the export quietly left it out -- and on
/// `hyperlink.edit` the `None` read as "no destination given", so the command reported success
/// and kept the old destination (noted in review).
fn dest_param(id: &str, d: &designcraft_doc::Document, p: &Value) -> Result<Option<HyperlinkDest>> {
    if let Some(u) = str_param(p, "url") {
        return Ok(Some(HyperlinkDest::Url(u.to_string())));
    }
    if let Some(e) = str_param(p, "email") {
        return Ok(Some(HyperlinkDest::Email(e.to_string())));
    }
    if let Some(a) = p.get("anchor").filter(|a| !a.is_null()) {
        let anchors = || d.stories.values().flat_map(|st| st.anchors.iter());
        // An id names one anchor or none.
        if let Some(n) = a.as_u64() {
            return match anchors().any(|x| x.id == n) {
                true => Ok(Some(HyperlinkDest::Anchor(n))),
                false => Err(bad(id, format!("no text anchor {a} (anchor.list names them)"))),
            };
        }
        // A name need not. `anchor.create` does not require one to be unique -- and an IDML
        // import can bring a document that already has two of the same -- so taking the first
        // match stored whichever anchor happened to come first, and a link on a name shared by
        // anchors on two pages went to the wrong page with nothing to say so (noted in review).
        // The id still reaches either of them.
        let Some(name) = a.as_str() else { return Err(bad(id, format!("no text anchor {a} (anchor.list names them)"))) };
        let mut named = anchors().filter(|x| x.name == name);
        return match (named.next(), named.next()) {
            (Some(x), None) => Ok(Some(HyperlinkDest::Anchor(x.id))),
            (Some(_), Some(_)) => {
                let ids: Vec<String> = anchors().filter(|x| x.name == name).map(|x| x.id.to_string()).collect();
                Err(bad(id, format!("more than one text anchor is named {name:?} ({}); give the id instead", ids.join(", "))))
            }
            (None, _) => Err(bad(id, format!("no text anchor {a} (anchor.list names them)"))),
        };
    }
    Ok(p.get("page").and_then(Value::as_u64).map(|pg| HyperlinkDest::Page((pg as usize).saturating_sub(1))))
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

    /// `anchor.create` does not require a name to be unique -- and an IDML import can bring a
    /// document that already has two of the same -- so the lookup taking the first match meant a
    /// link made on a shared name went to whichever anchor came first, a different page, with
    /// nothing to say so (noted in review). The id still reaches either of them.
    #[test]
    fn a_name_two_anchors_share_cannot_pick_one_of_them() {
        let mut s = Session::new();
        s.execute("file.new", &json!({"width": 300, "height": 300, "pages": 3, "facingPages": false, "margins": 0})).unwrap();

        // Two anchors with one name, in text on different pages.
        let mut made = Vec::new();
        for page in [1usize, 2] {
            let sp = s.doc().unwrap().doc.page_loc(page).unwrap().0;
            let f = s
                .execute("frame.create", &json!({"spread": sp, "rect": [20, 20, 280, 100], "content": "text", "text": "Photo 1 full size"}))
                .unwrap();
            s.execute("text.select", &json!({"story": f["story"].clone(), "anchor": 0, "focus": 7})).unwrap();
            made.push(s.execute("anchor.create", &json!({"name": "Photo 1"})).unwrap());
        }
        assert_ne!(made[0]["id"], made[1]["id"], "two anchors, one name");
        let thumb = s.execute("frame.create", &json!({"rect": [20, 20, 120, 80], "content": "unassigned"})).unwrap();

        // Creating on the shared name is refused, and says what to do instead.
        let e = s.execute("hyperlink.create", &json!({"anchor": "Photo 1", "ids": [thumb["id"].clone()]})).expect_err("ambiguous").to_string();
        assert!(e.contains("more than one text anchor"), "{e}");
        assert!(e.contains("give the id instead"), "it says what to do instead: {e}");
        assert!(s.execute("hyperlink.list", &json!({})).unwrap().as_array().is_some_and(|l| l.is_empty()), "and no link was made");

        // By id it works, and the id picks the one that was asked for.
        s.execute("hyperlink.create", &json!({"anchor": made[1]["id"].clone(), "ids": [thumb["id"].clone()]})).unwrap();
        let live = s.execute("hyperlink.list", &json!({})).unwrap()[0]["id"].clone();
        let dest = |s: &mut Session| s.execute("hyperlink.list", &json!({})).unwrap()[0]["dest"].clone();
        assert_eq!(dest(&mut s), json!({"kind": "anchor", "value": made[1]["id"].clone()}));

        // An edit on the shared name is refused and leaves the destination as it was.
        let e = s.execute("hyperlink.edit", &json!({"id": live.clone(), "anchor": "Photo 1"})).expect_err("ambiguous").to_string();
        assert!(e.contains("more than one text anchor"), "{e}");
        assert_eq!(dest(&mut s), json!({"kind": "anchor", "value": made[1]["id"].clone()}), "the refused edit changed nothing");

        // And the other anchor is still reachable by its id.
        s.execute("hyperlink.edit", &json!({"id": live, "anchor": made[0]["id"].clone()})).unwrap();
        assert_eq!(dest(&mut s), json!({"kind": "anchor", "value": made[0]["id"].clone()}));
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

        // An anchor's id works as well as its name, and an unknown name is refused.
        s.execute("hyperlink.delete", &json!({"id": links[0]["id"].clone()})).unwrap();
        s.execute("hyperlink.create", &json!({"anchor": anchor["id"].clone(), "ids": [thumb["id"].clone()], "name": "Thumbnail"})).unwrap();
        assert_eq!(s.execute("hyperlink.list", &json!({})).unwrap()[0]["name"], "Thumbnail");
        assert_eq!(link_pages(&mut s), vec![3]);
        let e = s.execute("hyperlink.create", &json!({"anchor": "No Such Anchor", "ids": [thumb["id"].clone()]})).expect_err("refused").to_string();
        assert!(e.contains("no text anchor"), "and it says why, rather than that no destination was given: {e}");

        // An unknown anchor *id* is refused as well. `as_u64` answered for any number, so a link
        // was stored to an anchor nobody has and the export quietly left it out (noted in review).
        let live = s.execute("hyperlink.list", &json!({})).unwrap()[0]["id"].clone();
        let missing = anchor["id"].as_u64().expect("the anchor's id") + 9_999;
        let e = s.execute("hyperlink.create", &json!({"anchor": missing, "ids": [thumb["id"].clone()]})).expect_err("refused").to_string();
        assert!(e.contains("no text anchor"), "an unknown id is refused: {e}");

        // And `hyperlink.edit` reports it rather than reading it as "no destination given", which
        // made the command succeed and keep the old destination (noted in review).
        for unknown in [json!(missing), json!("No Such Anchor")] {
            let e = s.execute("hyperlink.edit", &json!({"id": live.clone(), "anchor": unknown.clone()})).expect_err("refused").to_string();
            assert!(e.contains("no text anchor"), "{unknown}: {e}");
        }
        let after = s.execute("hyperlink.list", &json!({})).unwrap();
        assert_eq!(after[0]["dest"], json!({"kind": "anchor", "value": anchor["id"].clone()}), "the destination is unchanged: {after}");
        assert_eq!(after[0]["name"], "Thumbnail", "and so is the name");
        assert_eq!(link_pages(&mut s), vec![3], "and the export still lands on the anchor's page");

        // A rename that names no destination still works: that is what `None` is for, and it must
        // not have become an error on the way.
        s.execute("hyperlink.edit", &json!({"id": live.clone(), "name": "Thumb"})).unwrap();
        let renamed = s.execute("hyperlink.list", &json!({})).unwrap();
        assert_eq!(renamed[0]["name"], "Thumb");
        assert_eq!(renamed[0]["dest"], json!({"kind": "anchor", "value": anchor["id"].clone()}), "the destination survives a rename");

        // A link to a deleted anchor is not exported, rather than pointing at the wrong page.
        s.execute("text.select", &json!({"story": dest["story"].clone(), "anchor": 0, "focus": 8})).unwrap();
        s.execute("text.delete", &json!({})).unwrap();
        assert!(s.execute("anchor.list", &json!({})).unwrap().as_array().unwrap().is_empty());
        assert_eq!(link_pages(&mut s), Vec::<usize>::new(), "no link rather than a wrong one");
    }
}
