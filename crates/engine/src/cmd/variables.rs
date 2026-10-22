//! Type → Text Variables: define, list, delete and insert variables.

use designcraft_doc::vars::{TextVariable, VarKind, var_char};
use serde_json::{Value, json};

use super::{CommandSpec, bad, cmd, has_doc, str_param};
use crate::{Result, Session};

pub fn specs() -> Vec<CommandSpec> {
    vec![
        cmd!(query "variables.list", "Text Variables", [], None, "{} → [{index, name, type, …, value (page 1)}]", has_doc, |s, _| {
            let d = &s.doc()?.doc;
            Ok(Value::Array(
                d.text_variables
                    .iter()
                    .enumerate()
                    .map(|(i, v)| {
                        let mut o = serde_json::to_value(v).unwrap_or_default();
                        o["index"] = json!(i);
                        o["value"] = json!(d.variable_value(i, Some(0)));
                        o
                    })
                    .collect(),
            ))
        }),
        cmd!(
            "variables.define",
            "Define Text Variable…",
            ["Type", "Text Variables"],
            None,
            "{name, type: custom|lastPageNumber|chapterNumber|fileName|creationDate|modificationDate|outputDate|runningHeader, text?, format?, style?, use?: firstOnPage|lastOnPage, character?, section?, extension?, before?, after?} — creates or replaces by name",
            has_doc,
            define
        ),
        cmd!(
            "variables.delete",
            "Delete Text Variable",
            [],
            None,
            "{name} — instances become plain text of their current value",
            has_doc,
            |s, p| {
                let name = str_param(p, "name").ok_or_else(|| bad("variables.delete", "missing name"))?.to_string();
                s.edit(|d, _| {
                    let i = d.variable(&name).ok_or_else(|| bad("variables.delete", format!("no variable `{name}`")))?;
                    // Replace instances with their value (first page context), then shift higher indices down.
                    let value = d.variable_value(i, Some(0)).unwrap_or_default();
                    let n = d.text_variables.len();
                    let ids: Vec<_> = d.stories.keys().copied().collect();
                    for sid in ids {
                        if !d.story(sid).is_some_and(|st| needs_unvar(st, i, 0)) {
                            continue;
                        }
                        let st = d.story_mut(sid).ok_or(designcraft_doc::DocError::NoStory(sid))?;
                        unvar(st, i, n, &value, 0);
                    }
                    d.text_variables.remove(i);
                    Ok(Value::Null)
                })
            }
        ),
        cmd!(
            "variables.insert",
            "Insert Variable",
            ["Type", "Text Variables"],
            None,
            "{name} — at the text insertion point",
            super::has_text,
            |s, p| {
                let name = str_param(p, "name").ok_or_else(|| bad("variables.insert", "missing name"))?;
                let i = s.doc()?.doc.variable(name).ok_or_else(|| bad("variables.insert", format!("no variable `{name}`")))?;
                let c = var_char(i).ok_or_else(|| bad("variables.insert", "too many variables"))?;
                s.execute("text.insert", &json!({"text": c.to_string()}))
            }
        ),
    ]
}

/// Nesting limit for tables in cells and notes (hostile documents).
const MAX_DEPTH: usize = 32;

/// Whether `st` or a table cell or foot/endnote nested in it holds an instance of variable `i`
/// or a later variable.
fn needs_unvar(st: &designcraft_doc::Story, i: usize, depth: usize) -> bool {
    if depth > MAX_DEPTH {
        return false;
    }
    st.text.chars().any(|c| designcraft_doc::vars::var_index(c).is_some_and(|k| k >= i))
        || st.tables.values().any(|t| t.cells.iter().any(|c| needs_unvar(&c.text, i, depth + 1)))
        || st.notes.iter().chain(st.endnotes.iter()).any(|n| needs_unvar(&n.text, i, depth + 1))
}

/// Replaces variable `i`'s instances in `st` with `value` and moves the instances of variables
/// `i + 1..n` down by one, including table cells and foot/endnotes nested in the story. Only
/// touches (and unshares) the parts that hold such instances; a story whose nested text changed
/// gets a new revision.
fn unvar(st: &mut designcraft_doc::Story, i: usize, n: usize, value: &str, depth: usize) {
    if depth > MAX_DEPTH {
        return;
    }
    if st.text.chars().any(|c| designcraft_doc::vars::var_index(c).is_some_and(|k| k >= i)) {
        let target = var_char(i).unwrap_or(' ').to_string();
        while let Some(pos) = st.text.find(&target) {
            st.replace(pos..pos + target.len(), value);
        }
        for k in i + 1..n {
            let (from, to) = (var_char(k).unwrap_or(' ').to_string(), var_char(k - 1).unwrap_or(' ').to_string());
            while let Some(pos) = st.text.find(&from) {
                st.replace(pos..pos + from.len(), &to);
            }
        }
    }
    let mut nested = false;
    for t in st.tables.values_mut() {
        if !t.cells.iter().any(|c| needs_unvar(&c.text, i, depth + 1)) {
            continue;
        }
        for c in &mut std::sync::Arc::make_mut(t).cells {
            if needs_unvar(&c.text, i, depth + 1) {
                unvar(&mut c.text, i, n, value, depth + 1);
            }
        }
        nested = true;
    }
    for note in st.notes.iter_mut().chain(st.endnotes.iter_mut()) {
        if needs_unvar(&note.text, i, depth + 1) {
            unvar(&mut std::sync::Arc::make_mut(note).text, i, n, value, depth + 1);
            nested = true;
        }
    }
    if nested {
        st.rev = st.rev.wrapping_add(1);
    }
}

fn define(s: &mut Session, p: &Value) -> Result<Value> {
    let name = str_param(p, "name").ok_or_else(|| bad("variables.define", "missing name"))?.to_string();
    let fmt = || str_param(p, "format").unwrap_or("MM/dd/yy").to_string();
    let kind = match str_param(p, "type").unwrap_or("custom") {
        "custom" => VarKind::Custom { text: str_param(p, "text").unwrap_or("").into() },
        "lastPageNumber" => VarKind::LastPageNumber { section: p.get("section").and_then(Value::as_bool).unwrap_or(false) },
        "chapterNumber" => VarKind::ChapterNumber,
        "fileName" => VarKind::FileName { extension: p.get("extension").and_then(Value::as_bool).unwrap_or(false) },
        "creationDate" => VarKind::CreationDate { format: fmt() },
        "modificationDate" => VarKind::ModificationDate { format: fmt() },
        "outputDate" => VarKind::OutputDate { format: fmt() },
        "runningHeader" => VarKind::RunningHeader {
            style: str_param(p, "style").ok_or_else(|| bad("variables.define", "runningHeader needs `style`"))?.into(),
            use_: if str_param(p, "use") == Some("lastOnPage") { designcraft_doc::vars::Use::LastOnPage } else { Default::default() },
            character: p.get("character").and_then(Value::as_bool).unwrap_or(false),
        },
        t => return Err(bad("variables.define", format!("unknown type `{t}`"))),
    };
    let mut v = TextVariable::new(&name, kind);
    v.before = str_param(p, "before").unwrap_or("").into();
    v.after = str_param(p, "after").unwrap_or("").into();
    s.edit(|d, _| {
        let i = match d.variable(&name) {
            Some(i) => {
                d.text_variables[i] = v.clone();
                i
            }
            None => {
                if d.text_variables.len() >= designcraft_doc::vars::VAR_MAX {
                    return Err(bad("variables.define", "too many variables"));
                }
                d.text_variables.push(v.clone());
                d.text_variables.len() - 1
            }
        };
        Ok(json!({"index": i}))
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A parent-page page-number marker follows Numbering & Section Options: changing a section's
    /// start number renames its pages without changing their index or the number of sections, and
    /// the marker composed for a page shows the new name.
    #[test]
    fn parent_page_number_follows_section_renumbering() {
        let mut s = Session::new();
        s.execute("file.new", &json!({"pages": 3, "facingPages": false})).unwrap();
        let marker = designcraft_doc::story::PAGE_NUMBER.to_string();
        let r = s
            .execute("frame.create", &json!({"spread": {"kind": "parent", "index": 0}, "rect": [72, 30, 500, 60], "content": "text", "text": marker}))
            .unwrap();
        let sid = s.doc().unwrap().doc.item(designcraft_doc::ItemId(r["id"].as_u64().unwrap())).unwrap().text_frame().unwrap().story;
        assert_eq!(s.doc().unwrap().doc.story(sid).unwrap().text, marker);
        let shown = |s: &Session, page: usize| -> usize {
            let d = s.doc().unwrap().doc.clone();
            let cs = s.cache.get(&d, sid, Some(page));
            cs.frames.iter().flat_map(|f| f.lines.iter()).map(|l| l.glyphs.iter().filter(|g| g.visible && g.adv > 0.0).count()).sum()
        };
        assert_eq!(shown(&s, 2), 1, "page 3");
        s.execute("layout.section", &json!({"page": 1, "startNumber": 100})).unwrap();
        assert_eq!(s.doc().unwrap().doc.page_name(2), "102");
        assert_eq!(shown(&s, 2), 3, "page 102 after renumbering");
    }

    /// A section marker character shows the marker text of the section of the page it is composed
    /// for: on each document page a parent-page frame is shown on, and in a frame on a document
    /// page. A section without marker text shows nothing; the parent page itself shows "Section".
    #[test]
    fn section_marker_shows_the_pages_section_marker() {
        let mut s = Session::new();
        s.execute("file.new", &json!({"pages": 4, "facingPages": false})).unwrap();
        s.execute("layout.section", &json!({"page": 3, "startNumber": 1, "marker": "Intro"})).unwrap();
        let marker = designcraft_doc::story::SECTION_MARKER.to_string();
        let r = s
            .execute("frame.create", &json!({"spread": {"kind": "parent", "index": 0}, "rect": [72, 30, 500, 60], "content": "text", "text": marker}))
            .unwrap();
        let parent = s.doc().unwrap().doc.item(designcraft_doc::ItemId(r["id"].as_u64().unwrap())).unwrap().text_frame().unwrap().story;
        let sp = s.doc().unwrap().doc.page_loc(3).unwrap().0;
        let r = s.execute("frame.create", &json!({"spread": sp, "rect": [72, 200, 400, 260], "content": "text", "text": marker})).unwrap();
        let own = s.doc().unwrap().doc.item(designcraft_doc::ItemId(r["id"].as_u64().unwrap())).unwrap().text_frame().unwrap().story;
        let shown = |s: &Session, sid, page: Option<usize>| -> usize {
            let d = s.doc().unwrap().doc.clone();
            let cs = s.cache.get(&d, sid, page);
            cs.frames.iter().flat_map(|f| f.lines.iter()).map(|l| l.glyphs.iter().filter(|g| g.visible && g.adv > 0.0).count()).sum()
        };
        assert_eq!(shown(&s, parent, Some(0)), 0, "the first section has no marker");
        assert_eq!(shown(&s, parent, Some(2)), "Intro".len());
        assert_eq!(shown(&s, parent, Some(3)), "Intro".len());
        assert_eq!(shown(&s, own, None), "Intro".len(), "a frame on a document page");
        assert_eq!(shown(&s, parent, None), "Section".len(), "the parent page itself");
        // Editing the marker recomposes.
        s.execute("layout.section", &json!({"page": 3, "marker": "Appendix"})).unwrap();
        assert_eq!(shown(&s, parent, Some(2)), "Appendix".len());
        assert_eq!(shown(&s, own, None), "Appendix".len());
    }

    /// Two sections that both number from 1 repeat page names ("1, 2, 1, 2"). A parent-page running
    /// header is composed for the page it is shown on, not the first page with that name.
    #[test]
    fn running_header_on_parent_page_with_repeated_page_numbers() {
        let mut s = Session::new();
        s.execute("file.new", &json!({"pages": 4, "facingPages": false})).unwrap();
        s.execute("layout.section", &json!({"page": 3, "startNumber": 1})).unwrap();
        let names: Vec<String> = (0..4).map(|i| s.doc().unwrap().doc.page_name(i)).collect();
        assert_eq!(names, ["1", "2", "1", "2"]);
        s.execute("style.paragraph.create", &json!({"name": "Heading"})).ok();
        // The only heading is on the third page (the second section's page 1).
        let sp = s.doc().unwrap().doc.page_loc(2).unwrap().0;
        let r = s.execute("frame.create", &json!({"spread": sp, "rect": [72, 200, 400, 260], "content": "text", "text": "Epsilon"})).unwrap();
        let sid = s.doc().unwrap().doc.item(designcraft_doc::ItemId(r["id"].as_u64().unwrap())).unwrap().text_frame().unwrap().story;
        s.execute("text.select", &json!({"story": sid.0, "anchor": 0, "focus": 1})).unwrap();
        s.execute("style.paragraph.apply", &json!({"name": "Heading"})).unwrap();
        s.execute("variables.define", &json!({"name": "Chapter Head", "type": "runningHeader", "style": "Heading"})).unwrap();
        let r = s
            .execute("frame.create", &json!({"spread": {"kind": "parent", "index": 0}, "rect": [72, 30, 500, 60], "content": "text", "text": ""}))
            .unwrap();
        let sid = s.doc().unwrap().doc.item(designcraft_doc::ItemId(r["id"].as_u64().unwrap())).unwrap().text_frame().unwrap().story;
        s.execute("text.select", &json!({"story": sid.0, "anchor": 0, "focus": 0})).unwrap();
        s.execute("variables.insert", &json!({"name": "Chapter Head"})).unwrap();
        let d = s.doc().unwrap().doc.clone();
        let shown = |page: usize| -> usize {
            let cs = s.cache.get(&d, sid, Some(page));
            cs.frames.iter().flat_map(|f| f.lines.iter()).map(|l| l.glyphs.iter().filter(|g| g.visible && g.adv > 0.0).count()).sum()
        };
        assert_eq!(shown(0), 0, "no heading on or before the first page");
        assert_eq!(shown(2), "Epsilon".len(), "the third page (named 1) shows its own heading");
        assert_eq!(shown(3), "Epsilon".len(), "the fourth page (named 2) carries it");
    }

    /// Deleting a variable flattens its instances in table cells too and moves the instances of
    /// later variables down, so cells keep showing the same text.
    #[test]
    fn delete_variable_reaches_table_cells() {
        let mut s = Session::new();
        s.execute("file.new", &json!({})).unwrap();
        s.execute("variables.define", &json!({"name": "A", "type": "custom", "text": "Alpha"})).unwrap();
        let b = s.execute("variables.define", &json!({"name": "B", "type": "custom", "text": "Beta"})).unwrap()["index"].as_u64().unwrap() as usize;
        let a = s.doc().unwrap().doc.variable("A").unwrap();
        let r = s.execute("frame.create", &json!({"rect": [72, 72, 400, 300], "content": "text", "text": ""})).unwrap();
        let sid = s.doc().unwrap().doc.item(designcraft_doc::ItemId(r["id"].as_u64().unwrap())).unwrap().text_frame().unwrap().story;
        s.execute("text.select", &json!({"story": sid.0, "anchor": 0, "focus": 0})).unwrap();
        s.execute("table.insert", &json!({"rows": 1, "cols": 2})).unwrap();
        let text = format!("{}|{}", var_char(a).unwrap(), var_char(b).unwrap());
        s.execute("table.setCell", &json!({"row": 0, "col": 0, "text": text})).unwrap();
        s.execute("variables.delete", &json!({"name": "A"})).unwrap();
        let d = &s.doc().unwrap().doc;
        let t = d.story(sid).unwrap().tables.values().next().unwrap();
        let nb = d.variable("B").unwrap();
        assert_eq!(t.cells[0].text.text, format!("Alpha|{}", var_char(nb).unwrap()));
    }

    /// Deleting a variable flattens its instances in footnotes and endnotes too and moves the
    /// instances of later variables down.
    #[test]
    fn delete_variable_reaches_notes() {
        let mut s = Session::new();
        s.execute("file.new", &json!({})).unwrap();
        s.execute("variables.define", &json!({"name": "A", "type": "custom", "text": "Alpha"})).unwrap();
        let b = s.execute("variables.define", &json!({"name": "B", "type": "custom", "text": "Beta"})).unwrap()["index"].as_u64().unwrap() as usize;
        let a = s.doc().unwrap().doc.variable("A").unwrap();
        let r = s.execute("frame.create", &json!({"rect": [72, 72, 400, 300], "content": "text", "text": "x"})).unwrap();
        let sid = s.doc().unwrap().doc.item(designcraft_doc::ItemId(r["id"].as_u64().unwrap())).unwrap().text_frame().unwrap().story;
        let text = format!("{}|{}", var_char(a).unwrap(), var_char(b).unwrap());
        s.execute("text.select", &json!({"story": sid.0, "anchor": 1, "focus": 1})).unwrap();
        s.execute("footnote.insert", &json!({"text": text})).unwrap();
        s.execute("text.select", &json!({"story": sid.0, "anchor": 0, "focus": 0})).unwrap();
        s.execute("endnote.insert", &json!({"text": text})).unwrap();
        s.execute("variables.delete", &json!({"name": "A"})).unwrap();
        let d = &s.doc().unwrap().doc;
        let st = d.story(sid).unwrap();
        let want = format!("Alpha|{}", var_char(d.variable("B").unwrap()).unwrap());
        assert_eq!(st.notes[0].text.text, want);
        assert_eq!(st.endnotes[0].text.text, want);
    }

    #[test]
    fn running_header_on_parent_page_follows_headings() {
        let mut s = Session::new();
        s.execute("file.new", &json!({"pages": 3, "facingPages": false})).unwrap();
        s.execute("style.paragraph.create", &json!({"name": "Heading"})).ok();
        // Headings on pages 1 and 3 (page 2 carries page 1's).
        for (page, text) in [(0, "Alpha"), (2, "Epsilon")] {
            let sp = s.doc().unwrap().doc.page_loc(page).unwrap().0;
            let r = s.execute("frame.create", &json!({"spread": sp, "rect": [72, 200, 400, 260], "content": "text", "text": text})).unwrap();
            let sid = s.doc().unwrap().doc.item(designcraft_doc::ItemId(r["id"].as_u64().unwrap())).unwrap().text_frame().unwrap().story;
            s.execute("text.select", &json!({"story": sid.0, "anchor": 0, "focus": 1})).unwrap();
            s.execute("style.paragraph.apply", &json!({"name": "Heading"})).unwrap();
        }
        s.execute("variables.define", &json!({"name": "Chapter Head", "type": "runningHeader", "style": "Heading"})).unwrap();
        s.execute("variables.define", &json!({"name": "Total", "type": "custom", "text": "of "})).unwrap();
        // A header frame on the parent with "<running header> · <page> / <last page>".
        let r = s
            .execute("frame.create", &json!({"spread": {"kind": "parent", "index": 0}, "rect": [72, 30, 500, 60], "content": "text", "text": ""}))
            .unwrap();
        let fid = designcraft_doc::ItemId(r["id"].as_u64().unwrap());
        let sid = s.doc().unwrap().doc.item(fid).unwrap().text_frame().unwrap().story;
        s.execute("text.select", &json!({"story": sid.0, "anchor": 0, "focus": 0})).unwrap();
        s.execute("variables.insert", &json!({"name": "Chapter Head"})).unwrap();
        s.execute("text.insert", &json!({"text": " · "})).unwrap();
        s.execute("variables.insert", &json!({"name": "Last Page Number"})).unwrap();
        let d = s.doc().unwrap().doc.clone();
        assert!(matches!(d.find(fid).unwrap().spread, designcraft_doc::SpreadRef::Parent(0)));
        // Visible glyphs of the header composed for each page: "Alpha · 3" vs "Epsilon · 3".
        let shown = |page: usize| -> usize {
            let cs = s.cache.get(&d, sid, Some(page));
            cs.frames.iter().flat_map(|f| f.lines.iter()).map(|l| l.glyphs.iter().filter(|g| g.visible && g.adv > 0.0).count()).sum()
        };
        let (p1, p2, p3) = (shown(0), shown(1), shown(2));
        assert_eq!(p1, p2, "page 2 carries page 1's header");
        assert_eq!(p3, p1 + 2, "Epsilon is two letters longer than Alpha");
        let idx = s.cache.running_index(&d);
        assert_eq!(idx.value("Heading", false, Default::default(), 0), "Alpha");
        assert_eq!(idx.value("Heading", false, Default::default(), 1), "Alpha");
        assert_eq!(idx.value("Heading", false, Default::default(), 2), "Epsilon");
        let list = s.execute("variables.list", &json!({})).unwrap();
        assert!(list.as_array().unwrap().iter().any(|v| v["name"] == "Last Page Number" && v["value"] == "3"));
        // Deleting a variable flattens its instances to text.
        s.execute("variables.delete", &json!({"name": "Last Page Number"})).unwrap();
        assert!(s.doc().unwrap().doc.story(sid).unwrap().text.ends_with(" · 3"));
    }
}
