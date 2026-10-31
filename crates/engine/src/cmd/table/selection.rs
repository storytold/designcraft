use super::target::{Tgt, edit_table, target};
use super::{CellAddr, CellRange, Result, Selection, Session, StoryId, TableSel, TextSel, Value, bad, json, ok, str_param};

// ---------- selection ----------

pub(super) fn select(s: &mut Session, p: &Value) -> Result<Value> {
    let g = target(s, p, "table.select")?;
    let (nr, nc) = {
        let st = s.doc()?;
        let t = st.doc.story(g.story).and_then(|x| x.tables.get(&g.table)).ok_or_else(|| bad("table.select", "no table"))?;
        (t.nrows(), t.ncols())
    };
    let mut r = g.range;
    match str_param(p, "what") {
        Some("table") => r = CellRange { r0: 0, c0: 0, r1: nr - 1, c1: nc - 1 },
        Some("row") => {
            r.c0 = 0;
            r.c1 = nc - 1;
        }
        Some("column") => {
            r.r0 = 0;
            r.r1 = nr - 1;
        }
        _ => {}
    }
    let st = s.doc_mut()?;
    // The caret goes in the cell that covers the range's first position, a merged one included.
    let (row, col) = st.doc.story(g.story).and_then(|x| x.tables.get(&g.table)).map_or((r.r0, r.c0), |t| t.owner(r.r0, r.c0));
    // The caret's frame is kept only within the same story.
    let frame = st.selection.text.filter(|t| t.story == g.story).and_then(|t| t.frame);
    st.selection = Selection {
        cells: Some(TableSel { story: g.story, table: g.table, range: r }),
        text: Some(TextSel { story: g.story, anchor: 0, focus: 0, frame, cell: Some(CellAddr { table: g.table, row, col }) }),
        ..Default::default()
    };
    st.bump_revision();
    Ok(json!({"story": g.story.0, "table": g.table, "range": r}))
}

/// Table › Go to Row…: select row `row` (from 1) of the header, body or footer rows.
pub(super) fn go_to_row(s: &mut Session, p: &Value) -> Result<Value> {
    const ID: &str = "table.goToRow";
    // Only the table comes from `p`: `row` here counts within a section, not from the table's top.
    let mut at = serde_json::Map::new();
    for key in ["story", "table"] {
        if let Some(v) = p.get(key) {
            at.insert(key.into(), v.clone());
        }
    }
    let g = target(s, &Value::Object(at), ID)?;
    let n = p.get("row").and_then(Value::as_u64).filter(|&n| n >= 1).ok_or_else(|| bad(ID, "row must be a whole number from 1"))?;
    let (index, owners, nc) = {
        let st = s.doc()?;
        let t = st.doc.story(g.story).and_then(|x| x.tables.get(&g.table)).ok_or_else(|| bad(ID, "no table"))?;
        let (h, f, nr) = (t.header_rows(), t.footer_rows(), t.nrows());
        let (section, first, count) = match str_param(p, "section").unwrap_or("body") {
            "header" => ("header", 0, h),
            "body" => ("body", h, nr.saturating_sub(h + f)),
            "footer" => ("footer", nr.saturating_sub(f), f),
            other => return Err(bad(ID, format!("unknown section `{other}` (header, body or footer)"))),
        };
        if count == 0 {
            return Err(bad(ID, format!("the table has no {section} rows")));
        }
        let n = usize::try_from(n).unwrap_or(usize::MAX);
        if n > count {
            return Err(bad(ID, format!("row {n} is past the last of the table's {count} {section} rows")));
        }
        (first + n - 1, t.owners(), t.ncols())
    };
    let r = select(s, &json!({"story": g.story.0, "table": g.table, "rows": [index, index], "what": "row"}))?;
    // The page of the frame that shows the row: the cells that start in it or reach it from above.
    let start = index.saturating_mul(nc);
    let cells: Vec<(usize, usize)> = owners.get(start..start.saturating_add(nc)).map(<[_]>::to_vec).unwrap_or_default();
    let st = s.doc()?;
    let cs = s.cache.get(&st.doc, g.story, None);
    let placed = cs.frames.iter().find_map(|ft| {
        ft.tables
            .iter()
            .filter(|t| t.table == g.table)
            .flat_map(|t| t.cells.iter())
            .find(|c| !c.repeated && cells.contains(&(c.row, c.col)))
            .map(|c| (ft.frame, c.rect.center()))
    });
    let frame = placed.map(|(f, _)| f);
    // The page under the row's first cell: a frame can span two pages of a spread.
    let page = placed.and_then(|(f, at)| page_under(&st.doc, f, at)).map(|p| p + 1);
    // The caret is shown in the frame that shows the row.
    if let Some(f) = frame
        && let Some(t) = s.doc_mut()?.selection.text.as_mut()
    {
        t.frame = Some(f);
    }
    Ok(json!({"story": r["story"], "table": r["table"], "range": r["range"], "page": page}))
}

/// The page (from 0) under `at`, a point in the inner space of text frame `frame`.
fn page_under(doc: &designcraft_doc::Document, frame: designcraft_doc::ItemId, at: designcraft_geom::Point) -> Option<usize> {
    let loc = doc.find(frame)?;
    let designcraft_doc::SpreadRef::Doc(si) = loc.spread else { return None };
    let it = doc.item_at(&loc)?;
    let p = doc.parent_xf(&loc) * doc.text_xf(it) * at;
    Some(doc.first_page_of_spread(si) + doc.spread(loc.spread)?.page_at_x(p.x).unwrap_or(0))
}

/// Move the caret to the next/previous cell (Tab / Shift-Tab), selecting its text. Tab in the last
/// cell adds a row.
pub(crate) fn step_cell(s: &mut Session, forward: bool) -> Result<Value> {
    let t = s.doc()?.selection.text.ok_or_else(|| bad("table.nextCell", "no caret"))?;
    let c = t.cell.ok_or_else(|| bad("table.nextCell", "not in a cell"))?;
    let (owners, nr, nc) = {
        let st = s.doc()?;
        let tb = st.doc.story(t.story).and_then(|x| x.tables.get(&c.table)).ok_or_else(|| bad("table.nextCell", "no table"))?;
        (tb.owners(), tb.nrows(), tb.ncols())
    };
    let cur = c.row * nc + c.col;
    let next =
        if forward { (cur + 1..nr * nc).find(|&i| owners[i] == (i / nc, i % nc)) } else { (0..cur).rev().find(|&i| owners[i] == (i / nc, i % nc)) };
    let (row, col) = match next {
        Some(i) => (i / nc, i % nc),
        None if forward => {
            let g = Tgt { story: t.story, table: c.table, range: CellRange::cell(nr - 1, 0) };
            edit_table(s, &g, "table.nextCell", |tb| {
                tb.insert_rows(nr, 1);
                ok()
            })?;
            (nr, 0)
        }
        None => return ok(),
    };
    let st = s.doc_mut()?;
    let len = st.doc.text_story(t.story, Some(CellAddr { table: c.table, row, col })).map_or(0, |x| x.len());
    st.selection = Selection::text(TextSel { anchor: 0, focus: len, cell: Some(CellAddr { table: c.table, row, col }), ..t });
    st.bump_revision();
    Ok(json!({"row": row, "col": col}))
}

/// With one cell selected (a merged one counts as one), select its whole text instead, so what is
/// typed next replaces it, as InDesign does. Other selections are left as they are.
pub(crate) fn one_selected_cell_to_text(s: &mut Session) -> Result<()> {
    let st = s.doc_mut()?;
    let (Some(cs), Some(t)) = (st.selection.cells, st.selection.text) else { return Ok(()) };
    let Some(tb) = st.doc.story(cs.story).and_then(|x| x.tables.get(&cs.table)) else { return Ok(()) };
    let (row, col) = tb.owner(cs.range.r0, cs.range.c0);
    if tb.owner(cs.range.r1, cs.range.c1) != (row, col) {
        return Ok(());
    }
    let cell = CellAddr { table: cs.table, row, col };
    let len = st.doc.text_story(cs.story, Some(cell)).map_or(0, |x| x.len());
    st.selection = Selection::text(TextSel { story: cs.story, anchor: 0, focus: len, cell: Some(cell), ..t });
    Ok(())
}

/// Esc in a table: text in a cell becomes that cell's selection, and selected cells the text of
/// the first one (InDesign toggles between the two).
pub(super) fn toggle_selection(s: &mut Session) -> Result<Value> {
    const ID: &str = "table.toggleSelection";
    let st = s.doc_mut()?;
    let t = st.selection.text.ok_or_else(|| bad(ID, "no caret"))?;
    let c = t.cell.filter(|c| c.footnote_id().is_none()).ok_or_else(|| bad(ID, "not in a cell"))?;
    let cells = match st.selection.cells {
        Some(cs) => {
            // A merged cell's text belongs to the cell at its top left.
            let (row, col) = st
                .doc
                .story(cs.story)
                .and_then(|x| x.tables.get(&cs.table))
                .map_or((cs.range.r0, cs.range.c0), |tb| tb.owner(cs.range.r0, cs.range.c0));
            let cell = CellAddr { table: cs.table, row, col };
            let len = st.doc.text_story(cs.story, Some(cell)).map_or(0, |x| x.len());
            st.selection = Selection::text(TextSel { story: cs.story, anchor: 0, focus: len, cell: Some(cell), ..t });
            false
        }
        None => {
            st.selection = Selection {
                cells: Some(TableSel { story: t.story, table: c.table, range: CellRange::cell(c.row, c.col) }),
                text: Some(TextSel { anchor: 0, focus: 0, ..t }),
                ..Default::default()
            };
            true
        }
    };
    st.bump_revision();
    Ok(json!({"cells": cells}))
}

/// The table cell under spread point `pt` in text frame `frame`: (story, table, row, col).
pub(super) fn cell_at(s: &Session, frame: designcraft_doc::ItemId, pt: designcraft_geom::Point) -> Option<(StoryId, u64, usize, usize)> {
    let st = s.active()?;
    let loc = st.doc.find(frame)?;
    let it = st.doc.item_at(&loc)?;
    let sid = it.text_frame()?.story;
    let inner = (st.doc.parent_xf(&loc) * st.doc.text_xf(it)).inverse() * pt;
    let cs = s.cache.get(&st.doc, sid, None);
    let fi = cs.frames.iter().position(|f| f.frame == frame)?;
    let (table, row, col, _) = designcraft_compose::hit_cell(&cs, fi, inner)?;
    Some((sid, table, row, col))
}

pub(super) fn move_rc(s: &mut Session, p: &Value, rows: bool) -> Result<Value> {
    const ID: &str = "table.move";
    let g = target(s, p, ID)?;
    let from = p.get("from").and_then(Value::as_u64).map_or(if rows { g.range.r0 } else { g.range.c0 }, |v| v as usize);
    let to = p.get("to").and_then(Value::as_u64).ok_or_else(|| bad(ID, "`to` required"))? as usize;
    edit_table(s, &g, ID, |t| {
        if rows { t.move_row(from, to) } else { t.move_col(from, to) }.map_err(|e| bad(ID, e))?;
        Ok(json!({"from": from, "to": to}))
    })
}

#[cfg(test)]
mod go_to_row_tests {
    use designcraft_doc::CellRange;
    use serde_json::json;

    use crate::Session;

    /// A table of 1 header, 3 body and 1 footer rows, 2 columns wide, with the caret in its first cell.
    fn session() -> Session {
        let mut s = Session::new();
        s.execute("file.new", &json!({})).unwrap();
        let r = s.execute("frame.create", &json!({"rect": [72, 72, 500, 700], "content": "text", "text": ""})).unwrap();
        s.execute("text.select", &json!({"story": r["story"], "anchor": 0, "focus": 0})).unwrap();
        s.execute("table.insert", &json!({"rows": 3, "cols": 2, "headerRows": 1, "footerRows": 1})).unwrap();
        s
    }

    fn selected(s: &Session) -> CellRange {
        s.doc().unwrap().selection.cells.unwrap().range
    }

    #[test]
    fn go_to_row_selects_a_row_counted_within_its_section() {
        let mut s = session();
        let r = s.execute("table.goToRow", &json!({"row": 2})).unwrap();
        assert_eq!(selected(&s), CellRange { r0: 2, c0: 0, r1: 2, c1: 1 }, "the second body row, under the header row");
        assert_eq!(r["page"], 1);
        s.execute("table.goToRow", &json!({"row": 1, "section": "header"})).unwrap();
        assert_eq!(selected(&s), CellRange { r0: 0, c0: 0, r1: 0, c1: 1 });
        s.execute("table.goToRow", &json!({"row": 1, "section": "footer"})).unwrap();
        assert_eq!(selected(&s), CellRange { r0: 4, c0: 0, r1: 4, c1: 1 });
        s.execute("table.goToRow", &json!({"row": 3, "section": "body"})).unwrap();
        assert_eq!(selected(&s), CellRange { r0: 3, c0: 0, r1: 3, c1: 1 });
        // The caret goes to the row's first cell.
        assert_eq!(s.doc().unwrap().selection.text.and_then(|t| t.cell).map(|c| (c.row, c.col)), Some((3, 0)));
    }

    #[test]
    fn go_to_row_refuses_rows_outside_the_section() {
        let mut s = session();
        s.execute("table.goToRow", &json!({"row": 1})).unwrap();
        let before = selected(&s);
        for p in [
            json!({"row": 4}),
            json!({"row": 0}),
            json!({"row": -1}),
            json!({"row": "2"}),
            json!({}),
            json!({"row": 2, "section": "footer"}),
            json!({"row": 1, "section": "side"}),
        ] {
            assert!(s.execute("table.goToRow", &p).is_err(), "{p}");
            assert_eq!(selected(&s), before, "{p} leaves the selection");
        }
        s.execute("table.options", &json!({"headerRows": 0, "footerRows": 0})).unwrap();
        assert!(s.execute("table.goToRow", &json!({"row": 1, "section": "header"})).is_err());
        assert!(s.execute("table.goToRow", &json!({"row": 1, "section": "footer"})).is_err());
    }

    #[test]
    fn go_to_row_selects_a_row_of_a_named_table_outside_the_selection() {
        let mut s = session();
        let t = s.execute("table.get", &json!({})).unwrap();
        let other = s.execute("frame.create", &json!({"rect": [72, 720, 500, 760], "content": "text", "text": "Notes"})).unwrap();
        // A click in the other frame: the caret is shown in that frame.
        s.execute("text.placeCaret", &json!({"frame": other["id"], "point": [80, 730]})).unwrap();
        assert_eq!(s.doc().unwrap().selection.text.and_then(|t| t.frame).map(|f| f.0), other["id"].as_u64());
        assert!(s.execute("table.goToRow", &json!({"row": 2})).is_err(), "no table named and none selected");
        s.execute("table.goToRow", &json!({"story": t["story"], "table": t["table"], "row": 2})).unwrap();
        assert_eq!(selected(&s), CellRange { r0: 2, c0: 0, r1: 2, c1: 1 });
        let frame = s.doc().unwrap().selection.text.and_then(|t| t.frame).map(|f| f.0);
        assert_ne!(frame, other["id"].as_u64(), "the caret is not left in the other story's frame");
        // table.select naming the table from another story does not keep that story's frame either.
        s.execute("text.placeCaret", &json!({"frame": other["id"], "point": [80, 730]})).unwrap();
        s.execute("table.select", &json!({"story": t["story"], "table": t["table"], "what": "table"})).unwrap();
        let frame = s.doc().unwrap().selection.text.and_then(|t| t.frame).map(|f| f.0);
        assert_ne!(frame, other["id"].as_u64());
    }

    #[test]
    fn go_to_row_names_the_page_that_shows_the_row() {
        let mut s = Session::new();
        s.execute("file.new", &json!({"pages": 2, "facingPages": false})).unwrap();
        let a = s.execute("frame.create", &json!({"rect": [72, 72, 400, 200], "content": "text", "text": ""})).unwrap();
        let b = s.execute("frame.create", &json!({"spread": 1, "rect": [72, 72, 400, 600], "content": "unassigned"})).unwrap();
        s.execute("text.thread", &json!({"from": a["id"], "to": b["id"]})).unwrap();
        s.execute("text.select", &json!({"story": a["story"], "anchor": 0, "focus": 0})).unwrap();
        s.execute("table.insert", &json!({"rows": 20, "cols": 2, "headerRows": 1})).unwrap();
        for row in 0..21 {
            s.execute("table.setRowHeight", &json!({"row": row, "height": 20, "mode": "exactly"})).unwrap();
        }
        assert_eq!(s.execute("table.goToRow", &json!({"row": 1})).unwrap()["page"], 1);
        assert_eq!(s.execute("table.goToRow", &json!({"row": 20})).unwrap()["page"], 2);
        let frame = s.doc().unwrap().selection.text.and_then(|t| t.frame).map(|f| f.0);
        assert_eq!(frame, b["id"].as_u64(), "the caret is in the frame that shows the row");
        // The header row repeats on page 2; the row itself is the one on page 1.
        assert_eq!(s.execute("table.goToRow", &json!({"row": 1, "section": "header"})).unwrap()["page"], 1);
    }

    #[test]
    fn go_to_row_puts_the_caret_in_a_merged_cell_that_covers_the_row() {
        let mut s = session();
        // Body rows 1 and 2 (table rows 1 and 2) share one merged cell in column 0.
        s.execute("table.merge", &json!({"rows": [1, 2], "col": 0})).unwrap();
        s.execute("table.goToRow", &json!({"row": 2})).unwrap();
        assert_eq!(selected(&s), CellRange { r0: 2, c0: 0, r1: 2, c1: 1 });
        assert_eq!(s.doc().unwrap().selection.text.and_then(|t| t.cell).map(|c| (c.row, c.col)), Some((1, 0)));
    }

    #[test]
    fn table_get_without_cells_reads_the_rows_alone() {
        // The Go to Row dialog reads the row counts and the selection, not every cell's text.
        let mut s = session();
        s.execute("table.goToRow", &json!({"row": 2})).unwrap();
        let t = s.execute("table.get", &json!({"cells": false})).unwrap();
        assert!(t.get("cells").is_none(), "{t}");
        assert_eq!((t["headerRows"].clone(), t["footerRows"].clone(), t["range"]["r0"].clone()), (json!(1), json!(1), json!(2)));
        assert_eq!(t["rows"].as_array().map(Vec::len), Some(5));
        assert_eq!(s.execute("table.get", &json!({})).unwrap()["cells"].as_array().map(Vec::len), Some(10));
    }

    #[test]
    fn go_to_row_names_the_page_under_the_row_in_a_frame_across_the_spine() {
        let mut s = Session::new();
        s.execute("file.new", &json!({"pages": 3, "facingPages": true})).unwrap();
        // Pages 2 and 3 meet at x = 612; the frame's center is on page 3, the narrow table on page 2.
        let f = s.execute("frame.create", &json!({"spread": 1, "rect": [400, 72, 900, 400], "content": "text", "text": ""})).unwrap();
        s.execute("text.select", &json!({"story": f["story"], "anchor": 0, "focus": 0})).unwrap();
        s.execute("table.insert", &json!({"rows": 2, "cols": 2})).unwrap();
        s.execute("table.setColumnWidth", &json!({"cols": [0, 1], "width": 50})).unwrap();
        assert_eq!(s.execute("table.goToRow", &json!({"row": 2})).unwrap()["page"], 2);
    }
}
