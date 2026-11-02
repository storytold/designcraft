use super::{
    CellAddr, CellRange, Document, Result, Selection, Session, StoryId, Table, TableSel, TextSel, Value, bad, compose, floor_char_boundary, has_doc,
    str_param,
};

// ---------- enablement and targeting ----------

pub(super) fn in_table(s: &Session) -> std::result::Result<(), String> {
    has_doc(s)?;
    let st = s.active().ok_or("no document open")?;
    if st.selection.cells.is_some() || st.selection.text.is_some_and(|t| t.cell.is_some()) {
        return Ok(());
    }
    if let Some(t) = st.selection.text
        && st.doc.story(t.story).is_some_and(|x| x.table_at(t.focus).is_some())
    {
        return Ok(());
    }
    Err("place the insertion point in a table".into())
}

pub(super) fn in_table_or_ids(s: &Session) -> std::result::Result<(), String> {
    has_doc(s)
}

pub(super) fn in_cell(s: &Session) -> std::result::Result<(), String> {
    has_doc(s)?;
    // Footnote text is addressed as a cell too, but isn't in a table.
    let in_table = |c: CellAddr| c.footnote_id().is_none();
    if s.active().is_some_and(|d| d.selection.text.is_some_and(|t| t.cell.is_some_and(in_table))) {
        Ok(())
    } else {
        Err("not in a table cell".into())
    }
}

/// The table a command acts on and its target cell range.
pub(super) struct Tgt {
    pub(super) story: StoryId,
    pub(super) table: u64,
    pub(super) range: CellRange,
}

fn find_table(doc: &Document, id: u64) -> Option<StoryId> {
    doc.stories.iter().find(|(_, s)| s.tables.contains_key(&id)).map(|(k, _)| *k)
}

fn pair(p: &Value, many: &str, one: &str) -> Option<(usize, usize)> {
    if let Some(a) = p.get(many).and_then(Value::as_array) {
        let a0 = a.first()?.as_u64()? as usize;
        let a1 = a.get(1).and_then(Value::as_u64).map_or(a0, |v| v as usize);
        return Some((a0.min(a1), a0.max(a1)));
    }
    p.get(one).and_then(Value::as_u64).map(|v| (v as usize, v as usize))
}

pub(super) fn target(s: &Session, p: &Value, cmd: &str) -> Result<Tgt> {
    let st = s.doc()?;
    let doc = &st.doc;
    let explicit_table = p.get("table").and_then(Value::as_u64);
    let explicit_story = p.get("story").and_then(Value::as_u64).map(StoryId);
    let (story, table, range) = if let Some(tid) = explicit_table {
        let sid = explicit_story.or_else(|| find_table(doc, tid)).ok_or_else(|| bad(cmd, format!("no table {tid}")))?;
        (sid, tid, None)
    } else if let Some(c) = st.selection.cells {
        (c.story, c.table, Some(c.range))
    } else if let Some(t) = st.selection.text {
        match t.cell {
            Some(c) => (t.story, c.table, Some(CellRange::cell(c.row, c.col))),
            None => {
                let tid = doc.story(t.story).and_then(|x| x.table_at(t.focus)).ok_or_else(|| bad(cmd, "the insertion point is not in a table"))?;
                (t.story, tid, None)
            }
        }
    } else {
        return Err(bad(cmd, "place the insertion point in a table"));
    };
    let t = doc.story(story).and_then(|x| x.tables.get(&table)).ok_or_else(|| bad(cmd, format!("no table {table}")))?;
    let (nr, nc) = (t.nrows(), t.ncols());
    let mut range = range.unwrap_or(CellRange { r0: 0, c0: 0, r1: nr - 1, c1: nc - 1 });
    if let Some((a, b)) = pair(p, "rows", "row") {
        range.r0 = a;
        range.r1 = b;
    }
    if let Some((a, b)) = pair(p, "cols", "col") {
        range.c0 = a;
        range.c1 = b;
    }
    if range.r1 >= nr || range.c1 >= nc {
        return Err(bad(cmd, format!("cell range out of bounds ({nr}×{nc} table)")));
    }
    Ok(Tgt { story, table, range })
}

/// Edit the target table; afterwards selections pointing into it are clamped to its new shape.
pub(super) fn edit_table<T>(s: &mut Session, g: &Tgt, cmd: &str, f: impl FnOnce(&mut Table) -> Result<T>) -> Result<T> {
    let cmd = cmd.to_string();
    s.edit(|d, sel| {
        let st = d.story_mut(g.story).ok_or_else(|| bad(&cmd, "no story"))?;
        let t = st.table_mut(g.table).ok_or_else(|| bad(&cmd, "no table"))?;
        let r = f(t)?;
        clamp_selection(d, sel);
        Ok(r)
    })
}

/// Keep text/cell selections valid after the table changed shape (or disappeared).
pub(super) fn clamp_selection(d: &Document, sel: &mut Selection) {
    if let Some(ts) = sel.cells {
        match d.story(ts.story).and_then(|x| x.tables.get(&ts.table)) {
            Some(t) => {
                let (mr, mc) = (t.nrows() - 1, t.ncols() - 1);
                let r = ts.range;
                sel.cells = Some(TableSel { range: CellRange::new(r.r0.min(mr), r.c0.min(mc), r.r1.min(mr), r.c1.min(mc)), ..ts });
            }
            None => sel.cells = None,
        }
    }
    if let Some(t) = sel.text
        && let Some(c) = t.cell
    {
        let story = d.story(t.story);
        match story.and_then(|x| x.tables.get(&c.table)) {
            Some(tb) => {
                let (r, col) = tb.owner(c.row.min(tb.nrows() - 1), c.col.min(tb.ncols() - 1));
                let text = tb.cell(r, col).map_or("", |x| x.text.text.as_str());
                sel.text = Some(TextSel {
                    anchor: floor_char_boundary(text, t.anchor),
                    focus: floor_char_boundary(text, t.focus),
                    cell: Some(CellAddr { table: c.table, row: r, col }),
                    ..t
                });
            }
            None => {
                let pos = story.map_or(0, |x| floor_char_boundary(&x.text, t.anchor));
                sel.text = Some(TextSel { anchor: pos, focus: pos, cell: None, ..t });
            }
        }
    }
}

pub(super) fn count(p: &Value) -> usize {
    p.get("count").and_then(Value::as_u64).unwrap_or(1).clamp(1, 1000) as usize
}

pub(super) fn color_param(p: &Value, key: &str) -> Option<String> {
    str_param(p, key).map(str::to_string)
}

// ---------- create / convert ----------

/// Width of the text column at the caret (the new table spans it).
pub(super) fn column_width(s: &Session, t: &TextSel) -> f64 {
    let Some(st) = s.active() else { return 300.0 };
    let specs = compose::frame_specs(&st.doc, t.story);
    let spec = t.frame.and_then(|f| specs.iter().find(|x| x.id == f)).or(specs.first());
    spec.and_then(|f| f.columns().first().map(|c| c.width())).unwrap_or(300.0)
}

/// The direction of the paragraph at `offset` in `story`, as composition resolves it (its style
/// chain, then local overrides). A table created there takes it.
pub(super) fn para_direction(d: &Document, story: StoryId, offset: usize) -> designcraft_doc::TextDirection {
    d.story(story)
        .and_then(|st| st.paras.get(st.para_at(offset)))
        .map_or(designcraft_doc::TextDirection::LeftToRight, |pf| d.styles.resolve_para(pf).0.direction)
}
