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
    let frame = st.selection.text.and_then(|t| t.frame);
    st.selection = Selection {
        cells: Some(TableSel { story: g.story, table: g.table, range: r }),
        text: Some(TextSel { story: g.story, anchor: 0, focus: 0, frame, cell: Some(CellAddr { table: g.table, row: r.r0, col: r.c0 }) }),
        ..Default::default()
    };
    st.bump_revision();
    Ok(json!({"story": g.story.0, "table": g.table, "range": r}))
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
