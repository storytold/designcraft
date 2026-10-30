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

/// Three pages with "Alpha" in the Heading style on page 1 and "Epsilon" on page 3, a
/// `Chapter Head` running header on Heading, and a text frame on pages 1 and 3 that each hold
/// "<head>|<head>" (two instances). Returns the two frames' stories.
fn running_heads() -> (Session, designcraft_doc::StoryId, designcraft_doc::StoryId) {
    let mut s = Session::new();
    s.execute("file.new", &json!({"pages": 3, "facingPages": false})).unwrap();
    s.execute("style.paragraph.create", &json!({"name": "Heading"})).ok();
    let frame = |s: &mut Session, page: usize, y: f64, text: &str| {
        let sp = s.doc().unwrap().doc.page_loc(page).unwrap().0;
        let r = s.execute("frame.create", &json!({"spread": sp, "rect": [72, y, 400, y + 60.0], "content": "text", "text": text})).unwrap();
        s.doc().unwrap().doc.item(designcraft_doc::ItemId(r["id"].as_u64().unwrap())).unwrap().text_frame().unwrap().story
    };
    for (page, text) in [(0, "Alpha"), (2, "Epsilon")] {
        let sid = frame(&mut s, page, 200.0, text);
        s.execute("text.select", &json!({"story": sid.0, "anchor": 0, "focus": 1})).unwrap();
        s.execute("style.paragraph.apply", &json!({"name": "Heading"})).unwrap();
    }
    s.execute("variables.define", &json!({"name": "Chapter Head", "type": "runningHeader", "style": "Heading"})).unwrap();
    let c = var_char(s.doc().unwrap().doc.variable("Chapter Head").unwrap()).unwrap();
    let both = format!("{c}|{c}");
    let (p1, p3) = (frame(&mut s, 0, 400.0, &both), frame(&mut s, 2, 400.0, &both));
    (s, p1, p3)
}

/// Convert Variable to Text turns the selected instance into the value it shows on its own
/// page (Epsilon on page 3, not page 1's Alpha); the other instance and the variable stay.
#[test]
fn convert_variable_to_text_uses_the_instances_page() {
    let (mut s, p1, p3) = running_heads();
    let c = var_char(s.doc().unwrap().doc.variable("Chapter Head").unwrap()).unwrap();
    let second = c.len_utf8() + 1;
    s.execute("text.select", &json!({"story": p3.0, "anchor": second, "focus": second + c.len_utf8()})).unwrap();
    s.execute("variables.convertToText", &json!({})).unwrap();
    let d = &s.doc().unwrap().doc;
    assert_eq!(d.story(p3).unwrap().text, format!("{c}|Epsilon"), "only the selected instance");
    assert_eq!(d.story(p1).unwrap().text, format!("{c}|{c}"));
    assert!(d.variable("Chapter Head").is_some(), "the variable stays defined");
    let t = s.doc().unwrap().selection.text.unwrap();
    assert_eq!((t.anchor, t.focus), (second, second + "Epsilon".len()), "the converted text stays selected");
    s.execute("edit.undo", &json!({})).unwrap();
    assert_eq!(s.doc().unwrap().doc.story(p3).unwrap().text, format!("{c}|{c}"), "one undo step");
}

/// The menu item is enabled only for a selection that holds an instance: not a caret, not
/// text without one.
#[test]
fn convert_variable_to_text_needs_a_selected_instance() {
    let (mut s, p1, _) = running_heads();
    let c = var_char(s.doc().unwrap().doc.variable("Chapter Head").unwrap()).unwrap();
    let bar = c.len_utf8();
    let enabled = |s: &Session| (crate::find_command("variables.convertToText").unwrap().enabled)(s).is_ok();
    for (anchor, focus) in [(0, 0), (bar, bar + 1)] {
        s.execute("text.select", &json!({"story": p1.0, "anchor": anchor, "focus": focus})).unwrap();
        assert!(!enabled(&s), "{anchor}..{focus}");
    }
    s.execute("text.select", &json!({"story": p1.0, "anchor": 0, "focus": bar})).unwrap();
    assert!(enabled(&s));
}

/// A story threaded from a frame on page 1, which holds only "Lead", to a frame on page 3,
/// where the caret is left at the end of "Body". A cell's or a note's own byte 0 read in this
/// story is on page 1, its anchor on page 3.
fn threaded(s: &mut Session) -> designcraft_doc::StoryId {
    let sp1 = s.doc().unwrap().doc.page_loc(0).unwrap().0;
    let sp3 = s.doc().unwrap().doc.page_loc(2).unwrap().0;
    let r = s.execute("frame.create", &json!({"spread": sp1, "rect": [72, 500, 400, 520], "content": "text", "text": "Lead\nBody"})).unwrap();
    let from = r["id"].as_u64().unwrap();
    s.execute("text.thread", &json!({"from": from, "rect": [72, 500, 400, 700], "spread": sp3})).unwrap();
    let d = s.doc().unwrap().doc.clone();
    let sid = d.item(designcraft_doc::ItemId(from)).unwrap().text_frame().unwrap().story;
    let cs = s.cache.get(&d, sid, None);
    let page = |pos| designcraft_compose::vars::page_of(&d, &cs, pos);
    assert_eq!((page(0), page("Lead\nB".len())), (Some(0), Some(2)), "Lead on page 1, Body on page 3");
    s.execute("text.select", &json!({"story": sid.0, "anchor": "Lead\nBody".len(), "focus": "Lead\nBody".len()})).unwrap();
    sid
}

/// An instance selected in a table cell becomes the value on the page of the table's anchor.
#[test]
fn convert_variable_to_text_in_a_table_cell() {
    let (mut s, ..) = running_heads();
    let c = var_char(s.doc().unwrap().doc.variable("Chapter Head").unwrap()).unwrap();
    let sid = threaded(&mut s);
    s.execute("table.insert", &json!({"rows": 1, "cols": 1})).unwrap();
    s.execute("table.setCell", &json!({"row": 0, "col": 0, "text": c.to_string()})).unwrap();
    let table = *s.doc().unwrap().doc.story(sid).unwrap().tables.keys().next().unwrap();
    let cell = designcraft_doc::CellAddr { table, row: 0, col: 0 };
    s.doc_mut().unwrap().selection =
        designcraft_doc::Selection::text(designcraft_doc::TextSel { story: sid, anchor: 0, focus: c.len_utf8(), frame: None, cell: Some(cell) });
    s.execute("variables.convertToText", &json!({})).unwrap();
    let d = &s.doc().unwrap().doc;
    assert_eq!(d.text_story(sid, Some(cell)).unwrap().text, "Epsilon");
}

/// An instance selected in a footnote becomes the value on the page of the note's reference.
#[test]
fn convert_variable_to_text_in_a_footnote() {
    let (mut s, ..) = running_heads();
    let c = var_char(s.doc().unwrap().doc.variable("Chapter Head").unwrap()).unwrap();
    let sid = threaded(&mut s);
    s.execute("footnote.insert", &json!({"text": c.to_string()})).unwrap();
    let id = s.doc().unwrap().doc.story(sid).unwrap().notes[0].id;
    let note = designcraft_doc::CellAddr::footnote(id);
    let at = s.doc().unwrap().doc.text_story(sid, Some(note)).unwrap().text.find(c).unwrap();
    s.doc_mut().unwrap().selection = designcraft_doc::Selection::text(designcraft_doc::TextSel {
        story: sid,
        anchor: at,
        focus: at + c.len_utf8(),
        frame: None,
        cell: Some(note),
    });
    s.execute("variables.convertToText", &json!({})).unwrap();
    let d = &s.doc().unwrap().doc;
    let text = &d.text_story(sid, Some(note)).unwrap().text;
    assert!(!text.contains(c) && text.ends_with("Epsilon"), "{text:?}");
}

/// Text Variables › Convert to Text turns every instance of the variable into the value on
/// its page, in table cells, footnotes and endnotes on the page of their anchor, and keeps the
/// variable.
#[test]
fn convert_all_instances_to_text() {
    let (mut s, p1, p3) = running_heads();
    let c = var_char(s.doc().unwrap().doc.variable("Chapter Head").unwrap()).unwrap();
    let sid = threaded(&mut s);
    let at_end = |s: &mut Session| {
        let n = s.doc().unwrap().doc.story(sid).unwrap().len();
        s.execute("text.select", &json!({"story": sid.0, "anchor": n, "focus": n})).unwrap();
    };
    s.execute("footnote.insert", &json!({"text": c.to_string()})).unwrap();
    at_end(&mut s);
    s.execute("endnote.insert", &json!({"text": c.to_string()})).unwrap();
    at_end(&mut s);
    s.execute("table.insert", &json!({"rows": 1, "cols": 1})).unwrap();
    s.execute("table.setCell", &json!({"row": 0, "col": 0, "text": c.to_string()})).unwrap();
    s.execute("variables.convertAllToText", &json!({"name": "Chapter Head"})).unwrap();
    let d = &s.doc().unwrap().doc;
    assert_eq!(d.story(p1).unwrap().text, "Alpha|Alpha");
    assert_eq!(d.story(p3).unwrap().text, "Epsilon|Epsilon");
    let st = d.story(sid).unwrap();
    assert_eq!(st.tables.values().next().unwrap().cells[0].text.text, "Epsilon", "table cell");
    assert!(st.notes[0].text.text.ends_with("Epsilon"), "footnote: {:?}", st.notes[0].text.text);
    assert!(st.endnotes[0].text.text.ends_with("Epsilon"), "endnote: {:?}", st.endnotes[0].text.text);
    assert!(d.variable("Chapter Head").is_some(), "the variable stays defined");
}

/// A caret in a table cell or a footnote after an instance stays after the value Convert to
/// Text writes there, so typing lands after it.
#[test]
fn convert_all_keeps_the_caret_in_a_cell_or_a_footnote() {
    for footnote in [false, true] {
        let (mut s, ..) = running_heads();
        let c = var_char(s.doc().unwrap().doc.variable("Chapter Head").unwrap()).unwrap();
        let sid = threaded(&mut s);
        let text = format!("{c}|");
        let at = if footnote {
            s.execute("footnote.insert", &json!({"text": text})).unwrap();
            designcraft_doc::CellAddr::footnote(s.doc().unwrap().doc.story(sid).unwrap().notes[0].id)
        } else {
            s.execute("table.insert", &json!({"rows": 1, "cols": 1})).unwrap();
            s.execute("table.setCell", &json!({"row": 0, "col": 0, "text": text})).unwrap();
            let table = *s.doc().unwrap().doc.story(sid).unwrap().tables.keys().next().unwrap();
            designcraft_doc::CellAddr { table, row: 0, col: 0 }
        };
        let n = s.doc().unwrap().doc.text_story(sid, Some(at)).unwrap().text.len();
        s.doc_mut().unwrap().selection =
            designcraft_doc::Selection::text(designcraft_doc::TextSel { story: sid, anchor: n, focus: n, frame: None, cell: Some(at) });
        s.execute("variables.convertAllToText", &json!({"name": "Chapter Head"})).unwrap();
        s.execute("text.insert", &json!({"text": "x"})).unwrap();
        let text = &s.doc().unwrap().doc.text_story(sid, Some(at)).unwrap().text;
        assert!(text.ends_with("Epsilon|x"), "footnote {footnote}: {text:?}");
    }
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

/// A running header reads a page in text order: in a two-column frame the right column's
/// headings come after the left column's, even when they sit higher on the page. (The first
/// and last were picked by height, so a dictionary page's header named its left column's last
/// entry, which sits lowest, instead of the right column's.)
#[test]
fn running_header_reads_columns_in_text_order() {
    use designcraft_doc::story::COLUMN_BREAK;
    let mut s = Session::new();
    s.execute("file.new", &json!({"pages": 1, "facingPages": false})).unwrap();
    s.execute("style.paragraph.create", &json!({"name": "Heading"})).ok();
    // Left column: a line, Alpha, a line, Beta (low). Right column: Gamma (at the top), a line.
    let text = format!("x\nAlpha\nx\nBeta{COLUMN_BREAK}\nGamma\ny");
    let r = s.execute("frame.create", &json!({"rect": [72, 72, 472, 400], "content": "text", "text": text})).unwrap();
    s.execute("object.textFrameOptions", &json!({"ids": [r["id"]], "columns": 2, "gutter": 20})).unwrap();
    let sid = s.doc().unwrap().doc.item(designcraft_doc::ItemId(r["id"].as_u64().unwrap())).unwrap().text_frame().unwrap().story;
    for word in ["Alpha", "Beta", "Gamma"] {
        let at = text.find(word).unwrap();
        s.execute("text.select", &json!({"story": sid.0, "anchor": at, "focus": at + word.len()})).unwrap();
        s.execute("style.paragraph.apply", &json!({"name": "Heading"})).unwrap();
    }
    s.execute("variables.define", &json!({"name": "Entry", "type": "runningHeader", "style": "Heading"})).unwrap();
    let d = s.doc().unwrap().doc.clone();
    let cs = s.cache.get(&d, sid, None);
    let y = |word: &str| crate::compose::caret(&cs, text.find(word).unwrap()).unwrap().2;
    assert!(y("Gamma") < y("Beta") && y("Gamma") < y("Alpha"), "Gamma heads the right column, above the left column's headings");
    let idx = s.cache.running_index(&d);
    assert_eq!(idx.value("Heading", false, designcraft_doc::vars::Use::FirstOnPage, 0), "Alpha");
    assert_eq!(idx.value("Heading", false, designcraft_doc::vars::Use::LastOnPage, 0), "Gamma");
}
/// Stories on one page are ordered by where each one's text starts there: a story whose first
/// heading sits low in its first column comes after another story's heading above it, even
/// though its later column starts higher still.
#[test]
fn running_header_orders_stories_by_their_first_heading() {
    use designcraft_doc::story::COLUMN_BREAK;
    let mut s = Session::new();
    s.execute("file.new", &json!({"pages": 1, "facingPages": false})).unwrap();
    s.execute("style.paragraph.create", &json!({"name": "Heading"})).ok();
    // Story A, two columns: filler, then Alpha low in column 1; Gamma at the top of column 2.
    let a_text = format!("{}Alpha{COLUMN_BREAK}\nGamma\ny", "x\n".repeat(8));
    let a = s.execute("frame.create", &json!({"rect": [72, 72, 300, 400], "content": "text", "text": a_text})).unwrap();
    s.execute("object.textFrameOptions", &json!({"ids": [a["id"]], "columns": 2, "gutter": 12})).unwrap();
    // Story B beside it: Beta between Gamma's height and Alpha's.
    let b_text = "Beta".to_string();
    let b = s.execute("frame.create", &json!({"rect": [320, 120, 560, 160], "content": "text", "text": b_text})).unwrap();
    let story =
        |s: &Session, id: &Value| s.doc().unwrap().doc.item(designcraft_doc::ItemId(id.as_u64().unwrap())).unwrap().text_frame().unwrap().story;
    let (sa, sb) = (story(&s, &a["id"]), story(&s, &b["id"]));
    for (sid, text, word) in [(sa, &a_text, "Alpha"), (sa, &a_text, "Gamma"), (sb, &b_text, "Beta")] {
        let at = text.find(word).unwrap();
        s.execute("text.select", &json!({"story": sid.0, "anchor": at, "focus": at + word.len()})).unwrap();
        s.execute("style.paragraph.apply", &json!({"name": "Heading"})).unwrap();
    }
    s.execute("variables.define", &json!({"name": "Entry", "type": "runningHeader", "style": "Heading"})).unwrap();
    let d = s.doc().unwrap().doc.clone();
    let y = |sid, text: &str, word: &str| {
        let cs = s.cache.get(&d, sid, None);
        let (fi, _, b, ..) = crate::compose::caret(&cs, text.find(word).unwrap()).unwrap();
        d.item(cs.frames[fi].frame).unwrap().bounds().y0 + b
    };
    let (ya, yg, yb) = (y(sa, &a_text, "Alpha"), y(sa, &a_text, "Gamma"), y(sb, &b_text, "Beta"));
    assert!(yg < yb && yb < ya, "Gamma {yg} above Beta {yb} above Alpha {ya}");
    let idx = s.cache.running_index(&d);
    assert_eq!(idx.value("Heading", false, designcraft_doc::vars::Use::FirstOnPage, 0), "Beta");
    assert_eq!(idx.value("Heading", false, designcraft_doc::vars::Use::LastOnPage, 0), "Gamma");
}
