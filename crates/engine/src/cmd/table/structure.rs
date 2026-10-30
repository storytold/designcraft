use super::target::{column_width, count, edit_table, in_cell, para_direction, target};
use super::{CellAddr, Result, RowKind, Selection, Session, Table, TextSel, Value, bad, json, ok, str_param};

pub(super) fn insert(s: &mut Session, p: &Value) -> Result<Value> {
    let t = s.doc()?.selection.text.ok_or_else(|| bad("table.insert", "no insertion point"))?;
    if t.cell.is_some() {
        return Err(bad("table.insert", "tables can't be nested in cells"));
    }
    // A size that is given but is not one a table can have used to be corrected in silence:
    // `{"rows": 0}` made a table with one row, `-1` and `1.5` made the default four, and 10000
    // was capped at 500. Named `given` rather than `count`, which this module already imports
    // for the repeat count of a row or column insert.
    let given = |key: &str, alt: Option<&str>, default: u64, range: std::ops::RangeInclusive<u64>| -> Result<usize> {
        let v = p.get(key).or_else(|| alt.and_then(|a| p.get(a))).filter(|v| !v.is_null());
        match v {
            None => Ok(default as usize),
            Some(v) => match v.as_u64().filter(|n| range.contains(n)) {
                Some(n) => Ok(n as usize),
                None => Err(bad("table.insert", format!("{key} {v}: {} to {}", range.start(), range.end()))),
            },
        }
    };
    let rows = given("rows", None, 4, 1..=500)?;
    let cols = given("cols", Some("columns"), 4, 1..=200)?;
    let header = given("headerRows", None, 0, 0..=100)?;
    let footer = given("footerRows", None, 0, 0..=100)?;
    let width = p.get("width").and_then(Value::as_f64).unwrap_or_else(|| column_width(s, &t));
    s.edit(|d, sel| {
        let id = d.alloc();
        let mut table = Table::new(id, rows, cols, header, footer, width);
        table.options.direction = para_direction(d, t.story, t.range().start);
        let st = d.story_mut(t.story).ok_or_else(|| bad("table.insert", "no story"))?;
        // Cells start with the paragraph format at the caret.
        let pf = st.paras[st.para_at(t.range().start)].clone();
        let cf = st.char_format_at(t.range().start).clone();
        for c in &mut table.cells {
            c.text.paras[0] = designcraft_doc::ParaFormat { table: None, ..pf.clone() };
            c.text.chars[0].format = cf.clone();
        }
        let r = t.range();
        st.delete(r.clone());
        st.insert_table(r.start, table);
        *sel = Selection::text(TextSel { story: t.story, anchor: 0, focus: 0, frame: t.frame, cell: Some(CellAddr { table: id, row: 0, col: 0 }) });
        Ok(json!({"story": t.story.0, "table": id}))
    })
}

pub(super) fn convert_from_text(s: &mut Session, p: &Value) -> Result<Value> {
    let t = s.doc()?.selection.text.ok_or_else(|| bad("table.convertFromText", "no text selected"))?;
    if t.cell.is_some() {
        return Err(bad("table.convertFromText", "select text outside tables"));
    }
    let sep = if str_param(p, "columnSeparator") == Some("comma") { ',' } else { '\t' };
    let width = p.get("width").and_then(Value::as_f64).unwrap_or_else(|| column_width(s, &t));
    s.edit(|d, sel| {
        let id = d.alloc();
        let direction = para_direction(d, t.story, t.range().start);
        let st = d.story_mut(t.story).ok_or_else(|| bad("table.convertFromText", "no story"))?;
        let ranges = st.para_ranges();
        let (pa, pb) = (st.para_at(t.range().start), st.para_at(t.range().end));
        if (pa..=pb).any(|i| st.paras[i].table.is_some()) {
            return Err(bad("table.convertFromText", "the selection contains a table"));
        }
        let (a, b) = (ranges[pa].start, ranges[pb].end);
        let data: Vec<Vec<String>> = st.text[a..b].split('\n').map(|line| line.split(sep).map(|c| c.trim().to_string()).collect()).collect();
        let pf = st.paras[pa].clone();
        let cf = st.format_after(a).clone();
        let mut table = Table::from_strings(id, &data, width, &pf, &cf);
        table.options.direction = direction;
        st.delete(a..b);
        let anchor = st.insert_table(a, table);
        *sel = Selection::text(TextSel::caret(t.story, anchor));
        Ok(json!({"table": id, "rows": data.len()}))
    })
}

pub(super) fn convert_to_text(s: &mut Session, p: &Value) -> Result<Value> {
    let g = target(s, p, "table.convertToText")?;
    s.edit(|d, sel| {
        let st = d.story_mut(g.story).ok_or_else(|| bad("table.convertToText", "no story"))?;
        let text = st.tables.get(&g.table).map(|t| t.plain_text()).unwrap_or_default();
        let a = st.table_anchor(g.table).ok_or_else(|| bad("table.convertToText", "no anchor"))?;
        st.replace(a..a + designcraft_doc::TABLE_ANCHOR.len_utf8(), &text);
        *sel = Selection::text(TextSel::caret(g.story, a + text.len()));
        ok()
    })
}

pub(super) fn delete_table(s: &mut Session, p: &Value) -> Result<Value> {
    let g = target(s, p, "table.delete")?;
    s.edit(|d, sel| {
        let st = d.story_mut(g.story).ok_or_else(|| bad("table.delete", "no story"))?;
        let a = st.table_anchor(g.table).ok_or_else(|| bad("table.delete", "no anchor"))?;
        // Remove the anchor paragraph (with one separator) so no empty paragraph is left behind.
        let end = a + designcraft_doc::TABLE_ANCHOR.len_utf8();
        let r = if &st.text[end..] == "\n" && a > 0 && st.text[..a].ends_with('\n') {
            // Inserted at the end of the story: remove the empty paragraph after it too.
            a - 1..end + 1
        } else if st.text[end..].starts_with('\n') {
            a..end + 1
        } else if a > 0 && st.text[..a].ends_with('\n') {
            a - 1..end
        } else {
            a..end
        };
        st.delete(r.clone());
        *sel = Selection::text(TextSel::caret(g.story, r.start.min(st.len())));
        ok()
    })
}

// ---------- rows and columns ----------

pub(super) fn insert_row(s: &mut Session, p: &Value, above: Option<bool>) -> Result<Value> {
    let g = target(s, p, "table.insertRow")?;
    let above = above.unwrap_or(str_param(p, "where") == Some("above"));
    let n = count(p);
    let at = if above { g.range.r0 } else { g.range.r1 + 1 };
    let r = edit_table(s, &g, "table.insertRow", |t| {
        t.insert_rows(at, n);
        Ok(json!({"rows": t.nrows()}))
    })?;
    shift_selection(s, g.table, at, n, true);
    Ok(r)
}

/// After inserting rows/columns at `at`, selections at or after it follow their cells.
fn shift_selection(s: &mut Session, table: u64, at: usize, n: usize, rows: bool) {
    let Ok(st) = s.doc_mut() else { return };
    if let Some(t) = st.selection.text.as_mut()
        && let Some(c) = t.cell.as_mut()
        && c.table == table
    {
        let v = if rows { &mut c.row } else { &mut c.col };
        if *v >= at {
            *v += n;
        }
    }
    if let Some(ts) = st.selection.cells.as_mut()
        && ts.table == table
    {
        let (a, b) = if rows { (&mut ts.range.r0, &mut ts.range.r1) } else { (&mut ts.range.c0, &mut ts.range.c1) };
        if *a >= at {
            *a += n;
        }
        if *b >= at {
            *b += n;
        }
    }
}

pub(super) fn insert_col(s: &mut Session, p: &Value, left: Option<bool>) -> Result<Value> {
    let g = target(s, p, "table.insertColumn")?;
    let left = left.unwrap_or(str_param(p, "where") == Some("left"));
    let rtl = s
        .doc()?
        .doc
        .story(g.story)
        .and_then(|st| st.tables.get(&g.table))
        .is_some_and(|t| t.options.direction == designcraft_doc::TextDirection::RightToLeft);
    let n = count(p);
    let at = if left != rtl { g.range.c0 } else { g.range.c1 + 1 };
    let width = p.get("width").and_then(Value::as_f64);
    let r = edit_table(s, &g, "table.insertColumn", |t| {
        let w = width.unwrap_or_else(|| t.columns[g.range.c0.min(t.ncols() - 1)].width);
        t.insert_cols(at, n, w);
        Ok(json!({"columns": t.ncols()}))
    })?;
    shift_selection(s, g.table, at, n, false);
    Ok(r)
}

pub(super) fn delete_rows(s: &mut Session, p: &Value) -> Result<Value> {
    let g = target(s, p, "table.deleteRow")?;
    let all = s.doc()?.doc.story(g.story).and_then(|x| x.tables.get(&g.table)).is_some_and(|t| g.range.r0 == 0 && g.range.r1 + 1 >= t.nrows());
    if all {
        return delete_table(s, p);
    }
    edit_table(s, &g, "table.deleteRow", |t| {
        t.delete_rows(g.range.r0, g.range.r1 - g.range.r0 + 1);
        Ok(json!({"rows": t.nrows()}))
    })
}

pub(super) fn delete_cols(s: &mut Session, p: &Value) -> Result<Value> {
    let g = target(s, p, "table.deleteColumn")?;
    let all = s.doc()?.doc.story(g.story).and_then(|x| x.tables.get(&g.table)).is_some_and(|t| g.range.c0 == 0 && g.range.c1 + 1 >= t.ncols());
    if all {
        return delete_table(s, p);
    }
    edit_table(s, &g, "table.deleteColumn", |t| {
        t.delete_cols(g.range.c0, g.range.c1 - g.range.c0 + 1);
        Ok(json!({"columns": t.ncols()}))
    })
}

pub(super) fn merge(s: &mut Session, p: &Value) -> Result<Value> {
    let g = target(s, p, "table.merge")?;
    let r = edit_table(s, &g, "table.merge", |t| {
        t.merge(g.range).map_err(|e| bad("table.merge", e))?;
        Ok(t.expand_range(g.range))
    })?;
    // The caret goes to the merged cell.
    let st = s.doc_mut()?;
    st.selection = Selection::text(TextSel {
        story: g.story,
        anchor: 0,
        focus: 0,
        frame: st.selection.text.and_then(|t| t.frame),
        cell: Some(CellAddr { table: g.table, row: r.r0, col: r.c0 }),
    });
    ok()
}

pub(super) fn split(s: &mut Session, p: &Value, horizontal: bool) -> Result<Value> {
    let g = target(s, p, "table.split")?;
    edit_table(s, &g, "table.split", |t| {
        t.split_cells(g.range, horizontal).map_err(|e| bad("table.split", e))?;
        Ok(json!({"rows": t.nrows(), "cols": t.ncols()}))
    })
}

pub(super) fn unmerge(s: &mut Session, p: &Value) -> Result<Value> {
    let g = target(s, p, "table.unmerge")?;
    edit_table(s, &g, "table.unmerge", |t| {
        for r in g.range.r0..=g.range.r1 {
            for c in g.range.c0..=g.range.c1 {
                t.unmerge(r, c);
            }
        }
        ok()
    })
}

/// Convert Rows is greyed out unless the selected cells (or the caret's cell) can change kind.
pub(super) fn can_convert_rows(s: &Session, kind: RowKind) -> std::result::Result<(), String> {
    let st = s.active().ok_or("no document open")?;
    if st.selection.cells.is_none() {
        in_cell(s)?;
    }
    let g = target(s, &Value::Null, "table.convertRows").map_err(|e| e.to_string())?;
    let t = st.doc.story(g.story).and_then(|x| x.tables.get(&g.table)).ok_or("no table")?;
    t.convert_rows_counts(g.range.r0, g.range.r1, kind).map(|_| ())
}

pub(super) fn convert_rows(s: &mut Session, p: &Value, kind: RowKind) -> Result<Value> {
    const ID: &str = "table.convertRows";
    let g = target(s, p, ID)?;
    edit_table(s, &g, ID, |t| {
        let (h, f) = t.convert_rows_counts(g.range.r0, g.range.r1, kind).map_err(|e| bad(ID, e))?;
        t.set_header_footer(h, f);
        Ok(json!({"headerRows": h, "footerRows": f}))
    })
}
