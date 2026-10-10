//! Table menu: create, convert, insert/delete rows and columns, merge, cell/row/column options,
//! table options and cell selection.
//!
//! Commands target the table at the text selection (a caret in a cell or next to a table anchor),
//! the selected cells, or an explicit `{story, table}`; `rows: [a, b]` / `cols: [a, b]` (or `row` /
//! `col`) narrow the cell range (default: the selected cells, else the caret's cell, else the
//! whole table).

use designcraft_compose as compose;
use designcraft_doc::{
    AltFills, CellAddr, CellRange, CellStroke, Document, RowHeightMode, Selection, StoryId, StrokeType, Table, TableSel, TextSel,
    VerticalJustification,
};
use serde_json::{Value, json};

use super::{CommandSpec, bad, cmd, f64_or, has_doc, has_text, ok, str_param};
use crate::{Result, Session};

pub fn specs() -> Vec<CommandSpec> {
    let mut v = style_specs();
    v.extend(table_specs());
    v
}

fn style_specs() -> Vec<CommandSpec> {
    vec![
        cmd!(
            "style.cell.create",
            "New Cell Style…",
            [],
            None,
            "{name, fromSelection?: bool (the target cell's look), fill?, tint?, insets?: n|[t,l,b,r], vj?: top|center|bottom|justify, stroke?: {weight, color, tint?}, paragraphStyle?} → {name}",
            has_doc,
            |s, p| cell_style_create(s, p)
        ),
        cmd!(
            "style.cell.apply",
            "Apply Cell Style",
            [],
            None,
            "{name, story?, table?} — the target cells (the whole table when the cursor is in it)",
            in_table,
            cell_style_apply
        ),
        cmd!(
            "style.cell.edit",
            "Cell Style Options…",
            [],
            None,
            "{name, …same as style.cell.create} — cells using the style follow",
            has_doc,
            cell_style_edit
        ),
        cmd!(
            "style.table.create",
            "New Table Style…",
            [],
            None,
            "{name, header?, body?, footer?, leftColumn?, rightColumn?: cell style names, border?: {weight, color}, altRows?: {first, firstColor, next, nextColor}, spaceBefore?, spaceAfter?} → {name}",
            has_doc,
            table_style_create
        ),
        cmd!("style.table.apply", "Apply Table Style", [], None, "{name, story?, table?}", in_table, table_style_apply),
        cmd!(
            "style.table.edit",
            "Table Style Options…",
            [],
            None,
            "{name, …same as style.table.create} — tables using the style follow",
            has_doc,
            table_style_edit
        ),
    ]
}

fn table_specs() -> Vec<CommandSpec> {
    vec![
        cmd!(
            "table.insert",
            "Create Table",
            [],
            Some("Cmd+Alt+Shift+T"),
            "{rows?: body rows (4), cols? (4), headerRows? (0), footerRows? (0), width?} — at the text insertion point",
            has_text,
            insert
        ),
        cmd!(
            "table.convertFromText",
            "Convert Text to Table",
            ["Table"],
            None,
            "{columnSeparator?: tab|comma} — selected paragraphs become rows",
            has_text,
            convert_from_text
        ),
        cmd!(
            "table.convertToText",
            "Convert Table to Text",
            ["Table"],
            None,
            "{} — cells separated by tabs, rows by paragraphs",
            in_table,
            convert_to_text
        ),
        cmd!("table.insertRow", "Insert Row", [], None, "{where?: above|below, count? (1)}", in_table, |s, p| insert_row(s, p, None)),
        cmd!("table.insertRowAbove", "Insert Row Above", ["Table", "Insert"], None, "{count?}", in_table, |s, p| insert_row(s, p, Some(true))),
        cmd!("table.insertRowBelow", "Insert Row Below", ["Table", "Insert"], None, "{count?}", in_table, |s, p| insert_row(s, p, Some(false))),
        cmd!("table.insertColumn", "Insert Column", [], None, "{where?: left|right, count? (1), width?}", in_table, |s, p| insert_col(s, p, None)),
        cmd!("table.insertColumnLeft", "Insert Column Left", ["Table", "Insert"], None, "{count?}", in_table, |s, p| insert_col(s, p, Some(true))),
        cmd!("table.insertColumnRight", "Insert Column Right", ["Table", "Insert"], None, "{count?}", in_table, |s, p| insert_col(s, p, Some(false))),
        cmd!("table.deleteRow", "Delete Row", ["Table", "Delete"], None, "{} — the rows of the target cells", in_table, delete_rows),
        cmd!("table.deleteColumn", "Delete Column", ["Table", "Delete"], None, "{} — the columns of the target cells", in_table, delete_cols),
        cmd!("table.delete", "Delete Table", ["Table", "Delete"], None, "{}", in_table, delete_table),
        cmd!("table.merge", "Merge Cells", ["Table"], None, "{} — merge the target cell range", in_table, merge),
        cmd!(
            "table.dropCells",
            "Drag Rows or Columns",
            [],
            None,
            "{frame, from: [x, y], to: [x, y]} — selected whole rows (or columns) dragged from `from` move to the row (column) at `to`; otherwise a text drag",
            has_doc,
            drop_cells
        ),
        cmd!(
            "table.placeGraphic",
            "Convert Cell to Graphic Cell",
            ["Table", "Convert Cell Type"],
            None,
            "{path | base64+name, fit?: proportional|fill (default proportional)} — an image in the target cell (its text is kept but hidden)",
            in_table,
            place_graphic
        ),
        cmd!(
            "table.textCell",
            "Convert Cell to Text Cell",
            ["Table", "Convert Cell Type"],
            None,
            "{} — drop the target cells' graphics",
            in_table,
            |s, p| {
                let g = target(s, p, "table.textCell")?;
                edit_table(s, &g, "table.textCell", |t| {
                    for r in g.range.r0..=g.range.r1 {
                        for c in g.range.c0..=g.range.c1 {
                            if let Some(cell) = t.cell_mut(r, c) {
                                cell.graphic = None;
                            }
                        }
                    }
                    ok()
                })
            }
        ),
        cmd!(
            "table.moveRow",
            "Move Row",
            [],
            None,
            "{from?, to} (0-based; from defaults to the target row) — like dragging a row",
            in_table,
            |s, p| move_rc(s, p, true)
        ),
        cmd!("table.moveColumn", "Move Column", [], None, "{from?, to} (0-based; from defaults to the target column)", in_table, |s, p| move_rc(
            s, p, false
        )),
        cmd!(
            "table.sortRows",
            "Sort",
            ["Table"],
            None,
            "{column?: 0-based (default: the target cell's), descending?: bool} — body rows by that column's text (numbers numerically)",
            in_table,
            sort_rows
        ),
        cmd!("table.unmerge", "Unmerge Cells", ["Table"], None, "{}", in_table, unmerge),
        cmd!(
            "table.splitHorizontally",
            "Split Cell Horizontally",
            ["Table"],
            None,
            "{} — the target cell becomes two, one above the other",
            in_table,
            |s, p| split(s, p, true)
        ),
        cmd!(
            "table.splitVertically",
            "Split Cell Vertically",
            ["Table"],
            None,
            "{} — the target cell becomes two, side by side",
            in_table,
            |s, p| split(s, p, false)
        ),
        cmd!(
            "table.setCell",
            "Cell Options",
            [],
            None,
            "{fill?: swatch, tint?, insets?: n | [t,l,b,r], vj?: top|center|bottom|justify, text?, stroke?: {weight?, color?, tint?, type?: solid|dashed|dotted, edges?: all|outer|inner|top|left|bottom|right}}",
            in_table,
            set_cell
        ),
        cmd!("table.setRowHeight", "Row Height", [], None, "{height, mode?: atLeast|exactly}", in_table, set_row_height),
        cmd!("table.setColumnWidth", "Column Width", [], None, "{width}", in_table, set_col_width),
        cmd!("table.distributeColumns", "Distribute Columns Evenly", ["Table"], None, "{}", in_table, distribute_cols),
        cmd!(
            "table.options",
            "Table Options",
            [],
            None,
            "{direction?: leftToRight|rightToLeft, border?: {weight?, color?, tint?, type?}, spaceBefore?, spaceAfter?, headerRows?, footerRows?, repeatHeader?, repeatFooter?, altRows?: {first, firstColor, firstTint, next, nextColor, nextTint, skipFirst, skipLast} | null, altCols?: … | null}",
            in_table,
            options
        ),
        cmd!(noundo "table.select", "Select Cells", [], None, "{story?, table?, rows?: [a,b], cols?: [a,b], what?: cell|row|column|table}", in_table_or_ids, select),
        cmd!(noundo "table.selectTable", "Select Table", ["Table", "Select"], None, "{}", in_table, |s, p| {
            select(s, &super::with_param(p, "what", json!("table")))
        }),
        cmd!(noundo "table.selectRow", "Select Row", ["Table", "Select"], None, "{}", in_table, |s, p| {
            select(s, &super::with_param(p, "what", json!("row")))
        }),
        cmd!(noundo "table.selectColumn", "Select Column", ["Table", "Select"], None, "{}", in_table, |s, p| {
            select(s, &super::with_param(p, "what", json!("column")))
        }),
        cmd!(noundo "table.nextCell", "Next Cell", [], None, "{}", in_cell, |s, _| step_cell(s, true)),
        cmd!(noundo "table.prevCell", "Previous Cell", [], None, "{}", in_cell, |s, _| step_cell(s, false)),
        cmd!(query "table.get", "Get Table", [], None, "{story?, table?} → rows, columns, cells (text, spans, fill), options", in_table_or_ids, get),
    ]
}

// ---------- enablement and targeting ----------

fn in_table(s: &Session) -> std::result::Result<(), String> {
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

fn in_table_or_ids(s: &Session) -> std::result::Result<(), String> {
    has_doc(s)
}

fn in_cell(s: &Session) -> std::result::Result<(), String> {
    has_doc(s)?;
    if s.active().is_some_and(|d| d.selection.text.is_some_and(|t| t.cell.is_some())) { Ok(()) } else { Err("not in a table cell".into()) }
}

/// The table a command acts on and its target cell range.
struct Tgt {
    story: StoryId,
    table: u64,
    range: CellRange,
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

fn target(s: &Session, p: &Value, cmd: &str) -> Result<Tgt> {
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
fn edit_table<T>(s: &mut Session, g: &Tgt, cmd: &str, f: impl FnOnce(&mut Table) -> Result<T>) -> Result<T> {
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
fn clamp_selection(d: &Document, sel: &mut Selection) {
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
                let len = tb.cell(r, col).map_or(0, |x| x.text.len());
                sel.text =
                    Some(TextSel { anchor: t.anchor.min(len), focus: t.focus.min(len), cell: Some(CellAddr { table: c.table, row: r, col }), ..t });
            }
            None => {
                let len = story.map_or(0, |x| x.len());
                sel.text = Some(TextSel { anchor: t.anchor.min(len), focus: t.anchor.min(len), cell: None, ..t });
            }
        }
    }
}

fn count(p: &Value) -> usize {
    p.get("count").and_then(Value::as_u64).unwrap_or(1).clamp(1, 1000) as usize
}

fn color_param(p: &Value, key: &str) -> Option<String> {
    str_param(p, key).map(str::to_string)
}

// ---------- create / convert ----------

/// Width of the text column at the caret (the new table spans it).
fn column_width(s: &Session, t: &TextSel) -> f64 {
    let Some(st) = s.active() else { return 300.0 };
    let specs = compose::frame_specs(&st.doc, t.story);
    let spec = t.frame.and_then(|f| specs.iter().find(|x| x.id == f)).or(specs.first());
    spec.and_then(|f| f.columns().first().map(|c| c.width())).unwrap_or(300.0)
}

/// The direction of the paragraph at `offset` in `story`, as composition resolves it (its style
/// chain, then local overrides). A table created there takes it.
fn para_direction(d: &Document, story: StoryId, offset: usize) -> designcraft_doc::TextDirection {
    d.story(story)
        .and_then(|st| st.paras.get(st.para_at(offset)))
        .map_or(designcraft_doc::TextDirection::LeftToRight, |pf| d.styles.resolve_para(pf).0.direction)
}

fn insert(s: &mut Session, p: &Value) -> Result<Value> {
    let t = s.doc()?.selection.text.ok_or_else(|| bad("table.insert", "no insertion point"))?;
    if t.cell.is_some() {
        return Err(bad("table.insert", "tables can't be nested in cells"));
    }
    let rows = p.get("rows").and_then(Value::as_u64).unwrap_or(4).clamp(1, 500) as usize;
    let cols = p.get("cols").or_else(|| p.get("columns")).and_then(Value::as_u64).unwrap_or(4).clamp(1, 200) as usize;
    let header = p.get("headerRows").and_then(Value::as_u64).unwrap_or(0).min(100) as usize;
    let footer = p.get("footerRows").and_then(Value::as_u64).unwrap_or(0).min(100) as usize;
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

fn convert_from_text(s: &mut Session, p: &Value) -> Result<Value> {
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

fn convert_to_text(s: &mut Session, p: &Value) -> Result<Value> {
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

fn delete_table(s: &mut Session, p: &Value) -> Result<Value> {
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

fn insert_row(s: &mut Session, p: &Value, above: Option<bool>) -> Result<Value> {
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

fn insert_col(s: &mut Session, p: &Value, left: Option<bool>) -> Result<Value> {
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

fn delete_rows(s: &mut Session, p: &Value) -> Result<Value> {
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

fn delete_cols(s: &mut Session, p: &Value) -> Result<Value> {
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

fn merge(s: &mut Session, p: &Value) -> Result<Value> {
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

fn split(s: &mut Session, p: &Value, horizontal: bool) -> Result<Value> {
    let g = target(s, p, "table.split")?;
    edit_table(s, &g, "table.split", |t| {
        t.split_cell(g.range.r0, g.range.c0, horizontal);
        Ok(json!({"rows": t.nrows(), "cols": t.ncols()}))
    })
}

fn unmerge(s: &mut Session, p: &Value) -> Result<Value> {
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

fn parse_stroke(v: &Value, mut base: CellStroke) -> CellStroke {
    if let Some(w) = v.get("weight").and_then(Value::as_f64) {
        base.weight = w.max(0.0);
    }
    if let Some(c) = color_param(v, "color") {
        base.color = c;
    }
    if let Some(t) = v.get("tint").and_then(Value::as_f64) {
        base.tint = t.clamp(0.0, 1.0) as f32;
    }
    match str_param(v, "type") {
        Some("dashed") => base.kind = StrokeType::Dashed { pattern: vec![base.weight.max(1.0) * 3.0, base.weight.max(1.0) * 2.0] },
        Some("dotted") => base.kind = StrokeType::Dotted,
        Some("solid") => base.kind = StrokeType::Solid,
        _ => {}
    }
    base
}

fn set_cell(s: &mut Session, p: &Value) -> Result<Value> {
    let g = target(s, p, "table.setCell")?;
    let fill = color_param(p, "fill");
    let tint = p.get("tint").and_then(Value::as_f64).map(|t| t.clamp(0.0, 1.0) as f32);
    let insets: Option<[f64; 4]> = match p.get("insets") {
        Some(Value::Number(n)) => n.as_f64().map(|v| [v.max(0.0); 4]),
        Some(Value::Array(a)) if a.len() == 4 => {
            let v: Vec<f64> = a.iter().map(|x| x.as_f64().unwrap_or(0.0).max(0.0)).collect();
            Some([v[0], v[1], v[2], v[3]])
        }
        _ => None,
    };
    let vj = match str_param(p, "vj").or_else(|| str_param(p, "verticalJustification")) {
        Some("top") => Some(VerticalJustification::Top),
        Some("center") => Some(VerticalJustification::Center),
        Some("bottom") => Some(VerticalJustification::Bottom),
        Some("justify") => Some(VerticalJustification::Justify),
        Some(o) => return Err(bad("table.setCell", format!("unknown vj `{o}`"))),
        None => None,
    };
    let text = str_param(p, "text").map(str::to_string);
    let stroke = p.get("stroke").cloned();
    edit_table(s, &g, "table.setCell", |t| {
        let owners = t.owners();
        let nc = t.ncols();
        let rg = g.range;
        for r in rg.r0..=rg.r1 {
            for c in rg.c0..=rg.c1 {
                if owners[r * nc + c] != (r, c) {
                    continue;
                }
                let Some(cell) = t.cell_mut(r, c) else { continue };
                if let Some(f) = &fill {
                    cell.fill = f.clone();
                }
                if let Some(ti) = tint {
                    cell.fill_tint = ti;
                }
                if let Some(i) = insets {
                    cell.insets = i;
                }
                if let Some(v) = vj {
                    cell.vj = v;
                }
                if let Some(tx) = &text {
                    let len = cell.text.len();
                    cell.text.replace(0..len, tx);
                }
                if let Some(sv) = &stroke {
                    let edges = str_param(sv, "edges").unwrap_or("all");
                    for (i, on) in [
                        ("top", r == rg.r0),
                        ("left", c == rg.c0),
                        ("bottom", r + cell.row_span as usize - 1 == rg.r1),
                        ("right", c + cell.col_span as usize - 1 == rg.c1),
                    ]
                    .iter()
                    .enumerate()
                    .map(|(i, (name, outer))| (i, edges == "all" || edges == *name || (edges == "outer" && *outer) || (edges == "inner" && !*outer)))
                    {
                        if on {
                            cell.strokes[i] = parse_stroke(sv, cell.strokes[i].clone());
                        }
                    }
                }
            }
        }
        // Neighbouring cells share edges: mirror the edges along the range boundary.
        if let Some(sv) = &stroke {
            let edges = str_param(sv, "edges").unwrap_or("all");
            if matches!(edges, "all" | "outer" | "bottom") && rg.r1 + 1 < t.nrows() {
                for c in rg.c0..=rg.c1 {
                    let s0 = t.cell(rg.r1, c).map(|x| x.strokes[2].clone());
                    if let (Some(s0), Some(n)) = (s0, t.cell_mut(rg.r1 + 1, c)) {
                        n.strokes[0] = s0;
                    }
                }
            }
            if matches!(edges, "all" | "outer" | "right") && rg.c1 + 1 < t.ncols() {
                for r in rg.r0..=rg.r1 {
                    let s0 = t.cell(r, rg.c1).map(|x| x.strokes[3].clone());
                    if let (Some(s0), Some(n)) = (s0, t.cell_mut(r, rg.c1 + 1)) {
                        n.strokes[1] = s0;
                    }
                }
            }
        }
        ok()
    })
}

fn set_row_height(s: &mut Session, p: &Value) -> Result<Value> {
    let g = target(s, p, "table.setRowHeight")?;
    let h = p.get("height").and_then(Value::as_f64).map(|h| h.clamp(0.0, 10000.0));
    let mode = match str_param(p, "mode") {
        Some("exactly") => Some(RowHeightMode::Exactly),
        Some("atLeast") => Some(RowHeightMode::AtLeast),
        Some(o) => return Err(bad("table.setRowHeight", format!("unknown mode `{o}`"))),
        None => None,
    };
    if h.is_none() && mode.is_none() {
        return Err(bad("table.setRowHeight", "missing height"));
    }
    edit_table(s, &g, "table.setRowHeight", |t| {
        for row in &mut t.rows[g.range.r0..=g.range.r1] {
            if let Some(h) = h {
                row.height = h;
            }
            if let Some(m) = mode {
                row.mode = m;
            }
        }
        ok()
    })
}

fn set_col_width(s: &mut Session, p: &Value) -> Result<Value> {
    let g = target(s, p, "table.setColumnWidth")?;
    let w = p.get("width").and_then(Value::as_f64).ok_or_else(|| bad("table.setColumnWidth", "missing width"))?.clamp(3.0, 10000.0);
    edit_table(s, &g, "table.setColumnWidth", |t| {
        for c in &mut t.columns[g.range.c0..=g.range.c1] {
            c.width = w;
        }
        ok()
    })
}

fn distribute_cols(s: &mut Session, p: &Value) -> Result<Value> {
    let g = target(s, p, "table.distributeColumns")?;
    edit_table(s, &g, "table.distributeColumns", |t| {
        let cols = &mut t.columns[g.range.c0..=g.range.c1];
        let avg = cols.iter().map(|c| c.width).sum::<f64>() / cols.len() as f64;
        cols.iter_mut().for_each(|c| c.width = avg);
        ok()
    })
}

fn parse_alt(v: &Value) -> Option<AltFills> {
    if v.is_null() {
        return None;
    }
    let d = AltFills::default();
    Some(AltFills {
        first: v.get("first").and_then(Value::as_u64).map_or(d.first, |x| x as u32),
        first_color: color_param(v, "firstColor").unwrap_or(d.first_color),
        first_tint: v.get("firstTint").and_then(Value::as_f64).map_or(d.first_tint, |x| x as f32),
        next: v.get("next").and_then(Value::as_u64).map_or(d.next, |x| x as u32),
        next_color: color_param(v, "nextColor").unwrap_or(d.next_color),
        next_tint: v.get("nextTint").and_then(Value::as_f64).map_or(d.next_tint, |x| x as f32),
        skip_first: v.get("skipFirst").and_then(Value::as_u64).map_or(0, |x| x as u32),
        skip_last: v.get("skipLast").and_then(Value::as_u64).map_or(0, |x| x as u32),
    })
}

fn options(s: &mut Session, p: &Value) -> Result<Value> {
    let g = target(s, p, "table.options")?;
    let p = p.clone();
    edit_table(s, &g, "table.options", |t| {
        let o = &mut t.options;
        if let Some(v) = p.get("direction") {
            o.direction = serde_json::from_value(v.clone()).map_err(|e| bad("table.options", e.to_string()))?;
        }
        if let Some(b) = p.get("border") {
            o.border = parse_stroke(b, o.border.clone());
        }
        o.space_before = f64_or(&p, "spaceBefore", o.space_before);
        o.space_after = f64_or(&p, "spaceAfter", o.space_after);
        if let Some(b) = p.get("repeatHeader").and_then(Value::as_bool) {
            o.repeat_header = b;
        }
        if let Some(b) = p.get("repeatFooter").and_then(Value::as_bool) {
            o.repeat_footer = b;
        }
        if let Some(v) = p.get("altRows") {
            o.alt_rows = parse_alt(v);
        }
        if let Some(v) = p.get("altCols") {
            o.alt_cols = parse_alt(v);
        }
        let h = p.get("headerRows").and_then(Value::as_u64).map(|v| v as usize);
        let f = p.get("footerRows").and_then(Value::as_u64).map(|v| v as usize);
        if h.is_some() || f.is_some() {
            let (h, f) = (h.unwrap_or(t.header_rows()), f.unwrap_or(t.footer_rows()));
            // Add rows when there aren't enough for the requested header/footer plus one body row.
            let need = h + f + 1;
            if t.nrows() < need {
                let at = t.nrows();
                t.insert_rows(at, need - at);
            }
            t.set_header_footer(h, f);
        }
        Ok(json!({"options": t.options}))
    })
}

// ---------- selection ----------

fn select(s: &mut Session, p: &Value) -> Result<Value> {
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
    st.revision += 1;
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
    st.revision += 1;
    Ok(json!({"row": row, "col": col}))
}

fn get(s: &mut Session, p: &Value) -> Result<Value> {
    let g = target(s, p, "table.get")?;
    let st = s.doc()?;
    let t = st.doc.story(g.story).and_then(|x| x.tables.get(&g.table)).ok_or_else(|| bad("table.get", "no table"))?;
    let owners = t.owners();
    let cells: Vec<Value> = (0..t.nrows())
        .flat_map(|r| (0..t.ncols()).map(move |c| (r, c)))
        .filter(|&(r, c)| owners[r * t.ncols() + c] == (r, c))
        .filter_map(|(r, c)| {
            let cell = t.cell(r, c)?;
            Some(json!({"row": r, "col": c, "rowSpan": cell.row_span, "colSpan": cell.col_span, "text": cell.text.text, "fill": cell.fill,
                "insets": cell.insets, "vj": cell.vj, "style": cell.style}))
        })
        .collect();
    Ok(json!({
        "story": g.story.0, "table": g.table, "anchor": st.doc.story(g.story).and_then(|x| x.table_anchor(g.table)),
        "rows": t.rows, "columns": t.columns, "headerRows": t.header_rows(), "footerRows": t.footer_rows(),
        "cells": cells, "options": t.options, "range": g.range, "style": t.style,
    }))
}

// ---------- cell and table styles ----------

fn stroke_param(v: Option<&Value>) -> Option<designcraft_doc::CellStroke> {
    let o = v?.as_object()?;
    let mut st = designcraft_doc::CellStroke::default();
    if let Some(w) = o.get("weight").and_then(Value::as_f64) {
        st.weight = w.max(0.0);
    }
    if let Some(c) = o.get("color").and_then(Value::as_str) {
        st.color = c.to_string();
    }
    if let Some(t) = o.get("tint").and_then(Value::as_f64) {
        st.tint = t.clamp(0.0, 1.0) as f32;
    }
    Some(st)
}

/// Fill in the cell style fields given in `p`.
fn cell_style_fields(cs: &mut designcraft_doc::CellStyle, p: &Value, cmd: &str) -> Result<()> {
    if let Some(f) = color_param(p, "fill") {
        cs.fill = Some(f);
    }
    if let Some(t) = p.get("tint").and_then(Value::as_f64) {
        cs.fill_tint = Some(t.clamp(0.0, 1.0) as f32);
    }
    match p.get("insets") {
        Some(Value::Number(n)) => cs.insets = n.as_f64().map(|v| [v.max(0.0); 4]),
        Some(Value::Array(a)) if a.len() == 4 => {
            let v: Vec<f64> = a.iter().map(|x| x.as_f64().unwrap_or(0.0).max(0.0)).collect();
            cs.insets = Some([v[0], v[1], v[2], v[3]]);
        }
        _ => {}
    }
    if let Some(v) = p.get("vj") {
        cs.vj = Some(serde_json::from_value(v.clone()).map_err(|e| bad(cmd, format!("vj: {e}")))?);
    }
    if let Some(st) = stroke_param(p.get("stroke")) {
        cs.stroke = Some(st);
    }
    if let Some(ps) = str_param(p, "paragraphStyle") {
        cs.paragraph_style = Some(ps.to_string());
    }
    Ok(())
}

fn cell_style_create(s: &mut Session, p: &Value) -> Result<Value> {
    let base = str_param(p, "name").unwrap_or("Cell Style 1").to_string();
    let mut cs = designcraft_doc::CellStyle::default();
    if p.get("fromSelection").and_then(Value::as_bool).unwrap_or(false) {
        let g = target(s, p, "style.cell.create")?;
        let d = &s.doc()?.doc;
        let cell = d
            .story(g.story)
            .and_then(|st| st.tables.get(&g.table))
            .and_then(|t| t.cell(g.range.r0, g.range.c0))
            .ok_or_else(|| bad("style.cell.create", "no cell"))?;
        cs.fill = Some(cell.fill.clone());
        cs.fill_tint = Some(cell.fill_tint);
        cs.insets = Some(cell.insets);
        cs.vj = Some(cell.vj);
        cs.stroke = Some(cell.strokes[0].clone());
        cs.paragraph_style = cell.text.paras.first().map(|p| p.style.clone());
    }
    cell_style_fields(&mut cs, p, "style.cell.create")?;
    s.edit(|d, _| {
        let name = designcraft_doc::Styles::unique_name(|n| d.styles.cell.iter().any(|c| c.name == n), &base);
        cs.name = name.clone();
        d.styles_mut().cell.push(cs);
        Ok(json!({"name": name}))
    })
}

fn cell_style_apply(s: &mut Session, p: &Value) -> Result<Value> {
    let name = str_param(p, "name").ok_or_else(|| bad("style.cell.apply", "missing name"))?.to_string();
    let cs = s
        .doc()?
        .doc
        .styles
        .cell
        .iter()
        .find(|c| c.name == name)
        .cloned()
        .ok_or_else(|| bad("style.cell.apply", format!("no cell style `{name}`")))?;
    let g = target(s, p, "style.cell.apply")?;
    edit_table(s, &g, "style.cell.apply", |t| {
        let owners = t.owners();
        let nc = t.ncols();
        let mut n = 0;
        for r in g.range.r0..=g.range.r1 {
            for c in g.range.c0..=g.range.c1 {
                if owners[r * nc + c] == (r, c)
                    && let Some(cell) = t.cell_mut(r, c)
                {
                    cs.apply_to(cell);
                    n += 1;
                }
            }
        }
        Ok(json!({"cells": n}))
    })
}

/// Re-apply cell styles named in `names` to every cell using them (after a style edit).
fn reapply_cell_styles(d: &mut Document, names: &[String]) {
    let styles = d.styles.cell.clone();
    for sid in d.stories.keys().copied().collect::<Vec<_>>() {
        let Some(st) = d.story_mut(sid) else { continue };
        for t in st.tables.values_mut() {
            if !t.cells.iter().any(|c| names.contains(&c.style)) {
                continue;
            }
            for cell in &mut std::sync::Arc::make_mut(t).cells {
                if names.contains(&cell.style)
                    && let Some(cs) = styles.iter().find(|x| x.name == cell.style)
                {
                    cs.apply_to(cell);
                }
            }
        }
        st.rev += 1;
    }
}

fn cell_style_edit(s: &mut Session, p: &Value) -> Result<Value> {
    let name = str_param(p, "name").ok_or_else(|| bad("style.cell.edit", "missing name"))?.to_string();
    let p = p.clone();
    s.edit(|d, _| {
        let cs = d.styles_mut().cell.iter_mut().find(|c| c.name == name).ok_or_else(|| bad("style.cell.edit", format!("no cell style `{name}`")))?;
        cell_style_fields(cs, &p, "style.cell.edit")?;
        reapply_cell_styles(d, std::slice::from_ref(&name));
        ok()
    })
}

fn table_style_fields(ts: &mut designcraft_doc::TableStyle, p: &Value) {
    for (k, f) in [
        ("header", &mut ts.header),
        ("body", &mut ts.body),
        ("footer", &mut ts.footer),
        ("leftColumn", &mut ts.left_column),
        ("rightColumn", &mut ts.right_column),
    ] {
        if let Some(v) = p.get(k) {
            *f = v.as_str().map(str::to_string);
        }
    }
    if let Some(b) = stroke_param(p.get("border")) {
        ts.border = Some(b);
    }
    if let Some(a) = p.get("altRows").and_then(Value::as_object) {
        let n = |k: &str, d: u32| a.get(k).and_then(Value::as_u64).map_or(d, |v| v as u32);
        let c = |k: &str| a.get(k).and_then(Value::as_str).unwrap_or(designcraft_color::swatch::NONE).to_string();
        let t = |k: &str| a.get(k).and_then(Value::as_f64).map_or(1.0, |v| v.clamp(0.0, 1.0) as f32);
        ts.alt_rows = Some(designcraft_doc::AltFills {
            first: n("first", 1),
            first_color: c("firstColor"),
            first_tint: t("firstTint"),
            next: n("next", 1),
            next_color: c("nextColor"),
            next_tint: t("nextTint"),
            skip_first: n("skipFirst", 0),
            skip_last: n("skipLast", 0),
        });
    }
    if let Some(v) = p.get("spaceBefore").and_then(Value::as_f64) {
        ts.space_before = Some(v);
    }
    if let Some(v) = p.get("spaceAfter").and_then(Value::as_f64) {
        ts.space_after = Some(v);
    }
}

fn table_style_create(s: &mut Session, p: &Value) -> Result<Value> {
    let base = str_param(p, "name").unwrap_or("Table Style 1").to_string();
    let mut ts = designcraft_doc::TableStyle::default();
    table_style_fields(&mut ts, p);
    s.edit(|d, _| {
        let name = designcraft_doc::Styles::unique_name(|n| d.styles.table.iter().any(|c| c.name == n), &base);
        ts.name = name.clone();
        d.styles_mut().table.push(ts);
        Ok(json!({"name": name}))
    })
}

fn table_style_apply(s: &mut Session, p: &Value) -> Result<Value> {
    let name = str_param(p, "name").ok_or_else(|| bad("style.table.apply", "missing name"))?.to_string();
    let doc = &s.doc()?.doc;
    let ts = doc.styles.table.iter().find(|c| c.name == name).cloned().ok_or_else(|| bad("style.table.apply", format!("no table style `{name}`")))?;
    let cells = doc.styles.cell.clone();
    let g = target(s, p, "style.table.apply")?;
    edit_table(s, &g, "style.table.apply", |t| {
        ts.apply_to(t, &cells);
        ok()
    })
}

fn table_style_edit(s: &mut Session, p: &Value) -> Result<Value> {
    let name = str_param(p, "name").ok_or_else(|| bad("style.table.edit", "missing name"))?.to_string();
    let p = p.clone();
    s.edit(|d, _| {
        let ts =
            d.styles_mut().table.iter_mut().find(|c| c.name == name).ok_or_else(|| bad("style.table.edit", format!("no table style `{name}`")))?;
        table_style_fields(ts, &p);
        let (ts, cells) = (ts.clone(), d.styles.cell.clone());
        for sid in d.stories.keys().copied().collect::<Vec<_>>() {
            let Some(st) = d.story_mut(sid) else { continue };
            for t in st.tables.values_mut() {
                if t.style == name {
                    ts.apply_to(std::sync::Arc::make_mut(t), &cells);
                }
            }
            st.rev += 1;
        }
        ok()
    })
}

#[cfg(test)]
mod style_tests {
    use serde_json::json;

    use crate::Session;

    #[test]
    fn cell_and_table_styles_apply_and_follow_edits() {
        let mut s = Session::new();
        s.execute("file.new", &json!({})).unwrap();
        let r = s.execute("frame.create", &json!({"rect": [72, 72, 500, 400], "content": "text", "text": ""})).unwrap();
        let sid = r["story"].as_u64().unwrap();
        s.execute("text.select", &json!({"story": sid, "anchor": 0, "focus": 0})).unwrap();
        s.execute("table.insert", &json!({"rows": 3, "cols": 2, "headerRows": 1})).unwrap();
        s.execute("style.paragraph.create", &json!({"name": "Cell Head"})).unwrap();
        s.execute("style.cell.create", &json!({"name": "Head", "fill": "Black", "paragraphStyle": "Cell Head", "insets": 6})).unwrap();
        s.execute("style.cell.create", &json!({"name": "Body", "fill": "[Paper]"})).unwrap();
        s.execute("style.table.create", &json!({"name": "Data", "header": "Head", "body": "Body", "border": {"weight": 2, "color": "Black"}}))
            .unwrap();
        s.execute("style.table.apply", &json!({"name": "Data"})).unwrap();
        let t = |s: &Session| -> designcraft_doc::Table {
            let d = &s.doc().unwrap().doc;
            (**d.story(designcraft_doc::StoryId(sid)).unwrap().tables.values().next().unwrap()).clone()
        };
        let tb = t(&s);
        assert_eq!(tb.style, "Data");
        assert_eq!(tb.options.border.weight, 2.0);
        let head = tb.cell(0, 0).unwrap();
        assert_eq!((head.fill.as_str(), head.style.as_str(), head.insets), ("Black", "Head", [6.0; 4]));
        assert_eq!(head.text.paras[0].style, "Cell Head");
        assert_eq!(tb.cell(1, 1).unwrap().style, "Body");
        // Editing the cell style updates its cells.
        s.execute("style.cell.edit", &json!({"name": "Head", "fill": "C=100 M=0 Y=0 K=0"})).unwrap();
        assert_eq!(t(&s).cell(0, 1).unwrap().fill, "C=100 M=0 Y=0 K=0");
        // Editing the table style re-applies it.
        s.execute("style.table.edit", &json!({"name": "Data", "border": {"weight": 4}})).unwrap();
        assert_eq!(t(&s).options.border.weight, 4.0);
        assert!(s.execute("style.cell.apply", &json!({"name": "Nope"})).is_err());
    }
}

/// The table cell under spread point `pt` in text frame `frame`: (story, table, row, col).
fn cell_at(s: &Session, frame: designcraft_doc::ItemId, pt: designcraft_geom::Point) -> Option<(StoryId, u64, usize, usize)> {
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

fn drop_cells(s: &mut Session, p: &Value) -> Result<Value> {
    const ID: &str = "table.dropCells";
    let frame = super::id_param(p, "frame").ok_or_else(|| bad(ID, "`frame` required"))?;
    let pt = |k: &str| -> Result<designcraft_geom::Point> {
        let a = p.get(k).and_then(Value::as_array).ok_or_else(|| bad(ID, format!("`{k}` required")))?;
        Ok(designcraft_geom::Point::new(a.first().and_then(Value::as_f64).unwrap_or(0.0), a.get(1).and_then(Value::as_f64).unwrap_or(0.0)))
    };
    let (from, to) = (pt("from")?, pt("to")?);
    let sel = s.doc()?.selection.cells;
    let hit_from = cell_at(s, frame, from);
    let hit_to = cell_at(s, frame, to);
    if let (Some(ts), Some((sid, tid, fr, fc)), Some((_, tid2, tr, tc))) = (sel, hit_from, hit_to)
        && ts.story == sid
        && ts.table == tid
        && tid2 == tid
        && ts.range.contains(fr, fc)
    {
        let (nr, nc) = {
            let t = s.doc()?.doc.story(sid).and_then(|st| st.tables.get(&tid)).ok_or_else(|| bad(ID, "no table"))?;
            (t.nrows(), t.ncols())
        };
        let r = ts.range;
        let rows = r.c0 == 0 && r.c1 + 1 == nc;
        let cols = r.r0 == 0 && r.r1 + 1 == nr;
        if rows || cols {
            let (lo, hi, target) = if rows { (r.r0, r.r1, tr) } else { (r.c0, r.c1, tc) };
            if target >= lo && target <= hi {
                return Ok(json!({"moved": 0}));
            }
            let n = hi - lo + 1;
            let r2 = s.edit(|d, selm| {
                let t = d.story_mut(sid).and_then(|st| st.table_mut(tid)).ok_or_else(|| bad(ID, "no table"))?;
                let mv = |t: &mut designcraft_doc::Table, a: usize, b: usize| if rows { t.move_row(a, b) } else { t.move_col(a, b) };
                let new_lo = if target > hi {
                    for _ in 0..n {
                        mv(t, lo, target).map_err(|e| bad(ID, e))?;
                    }
                    target + 1 - n
                } else {
                    for k in 0..n {
                        mv(t, lo + k, target + k).map_err(|e| bad(ID, e))?;
                    }
                    target
                };
                let range =
                    if rows { CellRange::new(new_lo, r.c0, new_lo + n - 1, r.c1) } else { CellRange::new(r.r0, new_lo, r.r1, new_lo + n - 1) };
                selm.cells = Some(TableSel { range, ..ts });
                Ok(json!({"moved": n, "to": new_lo}))
            })?;
            return Ok(r2);
        }
    }
    // Not a row/column drag: select text from the press to the release.
    s.execute("text.placeCaret", &json!({"frame": frame.0, "point": [from.x, from.y]}))?;
    s.execute("text.extendTo", &json!({"frame": frame.0, "point": [to.x, to.y]}))
}

fn place_graphic(s: &mut Session, p: &Value) -> Result<Value> {
    const ID: &str = "table.placeGraphic";
    let g = target(s, p, ID)?;
    let (bytes, name, link) = super::file::read_source(p)?;
    let (pw, ph) = designcraft_render::image_size(&bytes).ok_or_else(|| bad(ID, "unsupported or corrupt image"))?;
    let fill = p.get("fit").and_then(Value::as_str) == Some("fill");
    s.edit(|d, sel| {
        let aid = designcraft_doc::AssetId(d.alloc());
        let mime = designcraft_render::image_mime(&bytes).to_string();
        d.assets.insert(
            aid,
            std::sync::Arc::new(designcraft_doc::Asset {
                page: 0,
                id: aid,
                name,
                mime,
                link,
                data: std::sync::Arc::new(bytes),
                pixels: Some((pw, ph)),
            }),
        );
        let st = d.story_mut(g.story).ok_or_else(|| bad(ID, "no story"))?;
        let t = st.table_mut(g.table).ok_or_else(|| bad(ID, "no table"))?;
        let (r, c) = t.owner(g.range.r0, g.range.c0);
        let cell = t.cell(r, c).ok_or_else(|| bad(ID, "no cell"))?;
        let (rs, cs) = (cell.row_span.max(1) as usize, cell.col_span.max(1) as usize);
        let w: f64 = t.columns[c..(c + cs).min(t.ncols())].iter().map(|x| x.width).sum();
        let h: f64 = t.rows[r..(r + rs).min(t.nrows())].iter().map(|x| x.height).sum();
        let ins = cell.insets;
        let (bw, bh) = ((w - ins[1] - ins[3]).max(1.0), (h - ins[0] - ins[2]).max(1.0));
        let (nw, nh) = (pw as f64, ph as f64);
        let k = if fill { (bw / nw).max(bh / nh) } else { (bw / nw).min(bh / nh) };
        let xf = designcraft_geom::Affine::translate(((bw - nw * k) / 2.0, (bh - nh * k) / 2.0)) * designcraft_geom::Affine::scale(k);
        let cell = t.cell_mut(r, c).ok_or_else(|| bad(ID, format!("no cell {r},{c}")))?;
        cell.graphic = Some(designcraft_doc::Graphic {
            asset: aid,
            size: (nw, nh),
            xf,
            auto_fit: if fill { designcraft_doc::Fitting::FillProportionally } else { designcraft_doc::Fitting::FitProportionally },
            fit_align: 4,
            crop: [0.0; 4],
        });
        clamp_selection(d, sel);
        Ok(json!({"asset": aid.0, "row": r, "col": c}))
    })
}

fn move_rc(s: &mut Session, p: &Value, rows: bool) -> Result<Value> {
    const ID: &str = "table.move";
    let g = target(s, p, ID)?;
    let from = p.get("from").and_then(Value::as_u64).map_or(if rows { g.range.r0 } else { g.range.c0 }, |v| v as usize);
    let to = p.get("to").and_then(Value::as_u64).ok_or_else(|| bad(ID, "`to` required"))? as usize;
    edit_table(s, &g, ID, |t| {
        if rows { t.move_row(from, to) } else { t.move_col(from, to) }.map_err(|e| bad(ID, e))?;
        Ok(json!({"from": from, "to": to}))
    })
}

fn sort_rows(s: &mut Session, p: &Value) -> Result<Value> {
    let g = target(s, p, "table.sortRows")?;
    let col = p.get("column").and_then(Value::as_u64).map_or(g.range.c0, |c| c as usize);
    let desc = p.get("descending").and_then(Value::as_bool).unwrap_or(false);
    edit_table(s, &g, "table.sortRows", |t| {
        let (nr, nc) = (t.nrows(), t.ncols());
        if col >= nc {
            return Err(bad("table.sortRows", format!("no column {col}")));
        }
        if t.cells.iter().any(|c| c.row_span > 1) {
            return Err(bad("table.sortRows", "unmerge cells that span rows first"));
        }
        let (h, f) = (t.header_rows(), t.footer_rows());
        let body: Vec<usize> = (h..nr - f).collect();
        let key = |r: usize| t.cell(r, col).map(|c| c.text.text.trim().to_string()).unwrap_or_default();
        let mut order = body.clone();
        order.sort_by(|a, b| {
            let (ka, kb) = (key(*a), key(*b));
            let o = match (ka.replace(',', "").parse::<f64>(), kb.replace(',', "").parse::<f64>()) {
                (Ok(x), Ok(y)) => x.total_cmp(&y),
                _ => ka.to_lowercase().cmp(&kb.to_lowercase()),
            };
            if desc { o.reverse() } else { o }
        });
        let rows = t.rows.clone();
        let cells = t.cells.clone();
        for (k, src) in order.iter().enumerate() {
            let dst = body[k];
            t.rows[dst] = rows[*src].clone();
            for c in 0..nc {
                t.cells[dst * nc + c] = cells[src * nc + c].clone();
            }
        }
        Ok(json!({"sorted": body.len()}))
    })
}

#[cfg(test)]
mod sort_tests {
    use serde_json::json;

    use crate::Session;

    #[test]
    fn sort_body_rows_by_a_column() {
        let mut s = Session::new();
        s.execute("file.new", &json!({})).unwrap();
        let r = s.execute("frame.create", &json!({"rect": [72, 72, 500, 400], "content": "text", "text": ""})).unwrap();
        s.execute("text.select", &json!({"story": r["story"], "anchor": 0, "focus": 0})).unwrap();
        s.execute("table.insert", &json!({"rows": 3, "cols": 2, "headerRows": 1})).unwrap();
        let cells = [("Name", "Qty"), ("pear", "10"), ("Apple", "9"), ("fig", "100")];
        for (r, (a, b)) in cells.iter().enumerate() {
            s.execute("table.setCell", &json!({"rows": [r, r], "cols": [0, 0], "text": a})).unwrap();
            s.execute("table.setCell", &json!({"rows": [r, r], "cols": [1, 1], "text": b})).unwrap();
        }
        let col = |s: &Session, c: usize| -> Vec<String> {
            let d = &s.doc().unwrap().doc;
            let t = d.stories.values().flat_map(|st| st.tables.values()).next().unwrap();
            (0..t.nrows()).map(|r| t.cell(r, c).unwrap().text.text.clone()).collect()
        };
        s.execute("table.sortRows", &json!({"column": 0})).unwrap();
        assert_eq!(col(&s, 0), ["Name", "Apple", "fig", "pear"], "header stays; case-insensitive");
        s.execute("table.sortRows", &json!({"column": 1, "descending": true})).unwrap();
        assert_eq!(col(&s, 1), ["Qty", "100", "10", "9"], "numbers by value");
        s.execute("table.splitVertically", &json!({"rows": [1, 1], "cols": [0, 0]})).unwrap();
        let r = s.execute("table.splitHorizontally", &json!({"rows": [0, 0], "cols": [1, 1]})).unwrap();
        assert_eq!((r["rows"].as_u64(), r["cols"].as_u64()), (Some(5), Some(3)));
    }

    #[test]
    fn graphic_cells_draw_their_image() {
        let mut s = Session::new();
        s.execute("file.new", &json!({})).unwrap();
        let r = s.execute("frame.create", &json!({"rect": [72, 72, 500, 400], "content": "text", "text": ""})).unwrap();
        s.execute("text.select", &json!({"story": r["story"], "anchor": 0, "focus": 0})).unwrap();
        s.execute("table.insert", &json!({"rows": 2, "cols": 2})).unwrap();
        s.execute("table.rowHeight", &json!({"height": 100})).ok();
        // A red image into the first cell.
        let px: Vec<u8> = (0..40 * 40).flat_map(|_| [255, 0, 0, 255]).collect();
        let png = designcraft_render::Rendered { width: 40, height: 40, pixels: px }.to_png();
        s.execute("table.setCell", &json!({"rows": [0, 0], "cols": [0, 0], "text": ""})).ok();
        let g = s.execute("table.placeGraphic", &json!({"base64": super::super::file::base64_encode(&png), "name": "r.png", "fit": "fill"})).unwrap();
        assert_eq!((g["row"].as_u64(), g["col"].as_u64()), (Some(0), Some(0)));
        let d = s.doc().unwrap().doc.clone();
        let st = d.stories.values().find(|st| !st.tables.is_empty()).unwrap();
        let t = st.tables.values().next().unwrap();
        let cs = s.cache.get(&d, st.id, None);
        let cell = cs.frames[0].tables[0].cells.iter().find(|c| c.row == 0 && c.col == 0).unwrap().clone();
        let fx = d.item(cs.frames[0].frame).unwrap().xf;
        let centre = fx * cell.clip.center();
        let mut rr = designcraft_render::Renderer::new();
        rr.threads = 0;
        let img = rr.render_page(&d, &s.cache, 0, 1.0, false, &Default::default()).unwrap();
        let p = img.pixel(centre.x as u32, centre.y as u32);
        assert!(p[0] > 200 && p[1] < 60, "red in the graphic cell: {p:?}");
        assert!(t.cell(0, 0).unwrap().graphic.is_some());
        s.execute("table.textCell", &json!({})).unwrap();
        let d = s.doc().unwrap().doc.clone();
        assert!(d.stories.values().flat_map(|st| st.tables.values()).all(|t| t.cell(0, 0).unwrap().graphic.is_none()));
    }

    #[test]
    fn move_rows_and_columns() {
        let mut s = Session::new();
        s.execute("file.new", &json!({})).unwrap();
        let r = s.execute("frame.create", &json!({"rect": [72, 72, 500, 400], "content": "text", "text": ""})).unwrap();
        s.execute("text.select", &json!({"story": r["story"], "anchor": 0, "focus": 0})).unwrap();
        s.execute("table.insert", &json!({"rows": 3, "cols": 2})).unwrap();
        for (r, c, t) in [(0, 0, "a"), (1, 0, "b"), (2, 0, "c"), (0, 1, "x")] {
            s.execute("table.setCell", &json!({"rows": [r, r], "cols": [c, c], "text": t})).unwrap();
        }
        let cell = |s: &Session, r: usize, c: usize| {
            let d = &s.doc().unwrap().doc;
            let t = d.stories.values().flat_map(|st| st.tables.values()).next().unwrap();
            t.cell(r, c).unwrap().text.text.clone()
        };
        // Dragging a selected row onto the last row moves it there.
        let d = s.doc().unwrap().doc.clone();
        let (sid, tid) = d.stories.values().find_map(|st| st.tables.keys().next().map(|k| (st.id, *k))).unwrap();
        let fid = d.stories[&sid].frames[0];
        let cs = s.cache.get(&d, sid, None);
        let cells = &cs.frames[0].tables[0].cells;
        let centre = |r: usize| {
            let c = cells.iter().find(|c| c.row == r && c.col == 0).unwrap().rect.center();
            d.text_xf(d.item(fid).unwrap()) * c
        };
        let (from, to) = (centre(0), centre(2));
        s.execute("table.select", &json!({"story": sid.0, "table": tid, "rows": [0, 0], "what": "row"})).unwrap();
        let r2 = s.execute("table.dropCells", &json!({"frame": fid.0, "from": [from.x, from.y], "to": [to.x, to.y]})).unwrap();
        assert_eq!(r2["moved"], 1);
        assert_eq!([cell(&s, 0, 0), cell(&s, 1, 0), cell(&s, 2, 0)], ["b", "c", "a"]);
        s.execute("edit.undo", &json!({})).unwrap();
        assert_eq!([cell(&s, 0, 0), cell(&s, 1, 0), cell(&s, 2, 0)], ["a", "b", "c"]);
        s.execute("table.moveRow", &json!({"from": 0, "to": 2})).unwrap();
        assert_eq!([cell(&s, 0, 0), cell(&s, 1, 0), cell(&s, 2, 0)], ["b", "c", "a"]);
        s.execute("table.moveColumn", &json!({"from": 1, "to": 0})).unwrap();
        assert_eq!((cell(&s, 2, 0), cell(&s, 2, 1)), ("x".to_string(), "a".to_string()));
    }
}

#[cfg(test)]
mod arabic_tests {
    use super::*;
    #[test]
    fn arabic_table_direction_and_physical_left_insertion_support_undo() {
        let mut s = Session::new();
        s.execute("file.new", &json!({})).unwrap();
        let r = s.execute("frame.create", &json!({"rect": [0, 0, 400, 300], "content": "text", "text": ""})).unwrap();
        s.execute("text.select", &json!({"story": r["story"], "anchor": 0, "focus": 0})).unwrap();
        s.execute("table.insert", &json!({"rows": 1, "cols": 2})).unwrap();
        s.execute("table.setCell", &json!({"rows": [0, 0], "cols": [0, 0], "text": "Right"})).unwrap();
        s.execute("table.options", &json!({"direction": "rightToLeft"})).unwrap();
        s.execute("table.select", &json!({"rows": [0, 0], "cols": [0, 0]})).unwrap();
        s.execute("table.insertColumnLeft", &json!({})).unwrap();
        let t = s.doc().unwrap().doc.stories.values().flat_map(|st| st.tables.values()).next().unwrap();
        assert_eq!(t.ncols(), 3);
        assert_eq!(t.cell(0, 0).unwrap().text.text, "Right");
        assert!(t.cell(0, 1).unwrap().text.text.is_empty());
        s.execute("edit.undo", &json!({})).unwrap();
        let t = s.doc().unwrap().doc.stories.values().flat_map(|st| st.tables.values()).next().unwrap();
        assert_eq!(t.ncols(), 2);
        assert_eq!(t.options.direction, designcraft_doc::TextDirection::RightToLeft);
    }

    /// A text frame whose paragraphs have a paragraph style with `direction`, with the story's
    /// text selected. Returns the session and the story id.
    fn styled_frame(direction: &str, text: &str) -> (Session, u64) {
        let mut s = Session::new();
        s.execute("file.new", &json!({"preset": "A4", "pages": 1})).unwrap();
        s.execute("style.paragraph.create", &json!({"name": "Dir", "para": {"direction": direction}})).unwrap();
        let r = s.execute("frame.create", &json!({"rect": [40, 330, 555, 420], "content": "text", "text": text, "caret": true})).unwrap();
        s.execute("edit.selectAll", &json!({})).unwrap();
        s.execute("style.paragraph.apply", &json!({"name": "Dir"})).unwrap();
        s.execute("edit.selectAll", &json!({})).unwrap();
        (s, r["story"].as_u64().unwrap())
    }

    fn only_table(s: &Session) -> std::sync::Arc<designcraft_doc::Table> {
        s.doc().unwrap().doc.stories.values().flat_map(|st| st.tables.values()).next().unwrap().clone()
    }

    /// Text converted to a table in a right-to-left paragraph laid its columns out left to right
    /// (#100): the first column belongs on the right.
    #[test]
    fn tables_take_the_direction_of_their_paragraph() {
        use designcraft_doc::TextDirection::{LeftToRight, RightToLeft};
        let (mut s, sid) = styled_frame("rightToLeft", "١\t٢\t٣\nأ\tب\tج");
        s.execute("table.convertFromText", &json!({"columnSeparator": "tab"})).unwrap();
        assert_eq!(only_table(&s).options.direction, RightToLeft);
        let st = s.doc().unwrap();
        let cs = s.cache.get(&st.doc, designcraft_doc::StoryId(sid), None);
        let placed = &cs.frames[0].tables[0];
        let (first, last) = (placed.cell(0, 0).unwrap().rect, placed.cell(0, 2).unwrap().rect);
        assert!(first.x0 > last.x0, "cell ١ is the rightmost: {first:?} {last:?}");

        let (mut s, _) = styled_frame("leftToRight", "1\t2\t3\na\tb\tc");
        s.execute("table.convertFromText", &json!({"columnSeparator": "tab"})).unwrap();
        assert_eq!(only_table(&s).options.direction, LeftToRight);

        for (direction, expected) in [("rightToLeft", RightToLeft), ("leftToRight", LeftToRight)] {
            let (mut s, _) = styled_frame(direction, "x");
            s.execute("table.insert", &json!({"rows": 1, "cols": 2})).unwrap();
            assert_eq!(only_table(&s).options.direction, expected, "table.insert in a {direction} paragraph");
        }
    }
}
