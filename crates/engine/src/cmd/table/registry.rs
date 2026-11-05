use super::cell::{get_cell_stroke, set_cell};
use super::content::{drop_cells, place_graphic, sort_rows};
use super::options::{get, options};
use super::selection::{move_rc, select, step_cell, toggle_selection};
use super::sizing::{distribute_cols, distribute_rows, resize_border, set_col_width, set_row_height};
use super::structure::{
    can_convert_rows, convert_from_text, convert_rows, convert_to_text, delete_cols, delete_rows, delete_table, insert, insert_col, insert_row,
    merge, split, unmerge,
};
use super::styles::{cell_style_apply, cell_style_create, cell_style_edit, table_style_apply, table_style_create, table_style_edit};
use super::target::{edit_table, in_cell, in_table, in_table_or_ids, target};
use super::{CommandSpec, RowKind, cmd, has_doc, has_text, json, ok};

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
            "{name, fromSelection?: bool (the target cell's look), fill?, tint?, insets?: n|[t,l,b,r], vj?: top|center|bottom|justify, firstBaseline?: ascent|capHeight|leading|xHeight|fixed|emboxHeight, firstBaselineMin?: pt, stroke?: {weight?, color?, tint?, type?, gapColor?, gapTint?} (all four edges; values as table.setCell's stroke), paragraphStyle?, basedOn?: cell style name, story?, table?, rows?: [a, b] | row?, cols?: [a, b] | col? (the target cells; default: the selection)} → {name}",
            has_doc,
            |s, p| cell_style_create(s, p)
        ),
        cmd!(
            "style.cell.apply",
            "Apply Cell Style",
            [],
            None,
            "{name, story?, table?, rows?: [a,b] | row?, cols?: [a,b] | col?} — the target cells (the whole table when the cursor is in it)",
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
            "{name, basedOn?: table style name, header?, body?, footer?, leftColumn?, rightColumn?: cell style names, border?: {weight?, color?, tint?, type?, gapColor?, gapTint?}, altRows?: {first, firstColor, next, nextColor}, spaceBefore?, spaceAfter?} → {name}",
            has_doc,
            table_style_create
        ),
        cmd!(
            "style.table.apply",
            "Apply Table Style",
            [],
            None,
            "{name, story?, table?, rows?: [a,b] | row?, cols?: [a,b] | col?}",
            in_table,
            table_style_apply
        ),
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
            // InDesign's Cmd+Alt+Shift+T opens the Insert Table dialog (`app.insertTableDialog`).
            None,
            "{rows?: body rows (4, at most 500), cols? | columns? (4, at most 200), headerRows? (0), footerRows? (0, each at most 100), tableStyle?: name, width?} — at the text insertion point, styled in the same step",
            has_text,
            insert
        ),
        cmd!(
            "table.convertFromText",
            "Convert Text to Table",
            ["Table"],
            None,
            "{columnSeparator?: tab|comma|paragraph|text (tab), rowSeparator?: paragraph|tab|comma|text (paragraph), columns?: number of columns when both separators are the same (1, at most 200), tableStyle?: name, width?: table width (default: the column)} — the selected paragraphs become a table, as InDesign's Convert Text to Table; the words tab, comma and paragraph name those separators, any other text is the separator itself",
            has_text,
            convert_from_text
        ),
        cmd!(
            "table.convertToText",
            "Convert Table to Text",
            ["Table"],
            None,
            "{columnSeparator?: tab|comma|paragraph|text (tab), rowSeparator?: paragraph|tab|comma|text (paragraph), story?, table?, rows?: [a, b] | row?, cols?: [a, b] | col? (the target cells; default: the selection)} — the table becomes its cells' text, as InDesign's Convert Table to Text",
            in_table,
            convert_to_text
        ),
        cmd!(
            "table.insertRow",
            "Insert Row",
            [],
            None,
            "{where?: above|below, count? (1), story?, table?, rows?: [a, b] | row?, cols?: [a, b] | col? (the target cells; default: the selection)}",
            in_table,
            |s, p| insert_row(s, p, None)
        ),
        cmd!(
            "table.insertRowAbove",
            "Insert Row Above",
            ["Table", "Insert"],
            None,
            "{count?, story?, table?, rows?: [a, b] | row?, cols?: [a, b] | col? (the target cells; default: the selection)}",
            in_table,
            |s, p| insert_row(s, p, Some(true))
        ),
        cmd!(
            "table.insertRowBelow",
            "Insert Row Below",
            ["Table", "Insert"],
            None,
            "{count?, story?, table?, rows?: [a, b] | row?, cols?: [a, b] | col? (the target cells; default: the selection)}",
            in_table,
            |s, p| insert_row(s, p, Some(false))
        ),
        cmd!(
            "table.insertColumn",
            "Insert Column",
            [],
            None,
            "{where?: left|right, count? (1), width?, story?, table?, rows?: [a, b] | row?, cols?: [a, b] | col? (the target cells; default: the selection)}",
            in_table,
            |s, p| insert_col(s, p, None)
        ),
        cmd!(
            "table.insertColumnLeft",
            "Insert Column Left",
            ["Table", "Insert"],
            None,
            "{count?, width?, story?, table?, rows?: [a, b] | row?, cols?: [a, b] | col? (the target cells; default: the selection)}",
            in_table,
            |s, p| insert_col(s, p, Some(true))
        ),
        cmd!(
            "table.insertColumnRight",
            "Insert Column Right",
            ["Table", "Insert"],
            None,
            "{count?, width?, story?, table?, rows?: [a, b] | row?, cols?: [a, b] | col? (the target cells; default: the selection)}",
            in_table,
            |s, p| insert_col(s, p, Some(false))
        ),
        cmd!(
            "table.deleteRow",
            "Delete Row",
            ["Table", "Delete"],
            None,
            "{story?, table?, rows?: [a, b] | row?, cols?: [a, b] | col? (the target cells; default: the selection)} — the rows of the target cells",
            in_table,
            delete_rows
        ),
        cmd!(
            "table.deleteColumn",
            "Delete Column",
            ["Table", "Delete"],
            None,
            "{story?, table?, rows?: [a, b] | row?, cols?: [a, b] | col? (the target cells; default: the selection)} — the columns of the target cells",
            in_table,
            delete_cols
        ),
        cmd!(
            "table.delete",
            "Delete Table",
            ["Table", "Delete"],
            None,
            "{story?, table?, rows?: [a, b] | row?, cols?: [a, b] | col? (the target cells; default: the selection)}",
            in_table,
            delete_table
        ),
        cmd!(
            "table.merge",
            "Merge Cells",
            ["Table"],
            None,
            "{story?, table?, rows?: [a, b] | row?, cols?: [a, b] | col? (the target cells; default: the selection)} — merge the target cell range",
            in_table,
            merge
        ),
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
            ["Table"],
            None,
            "{path? | base64?, name?: with base64, fit?: proportional|fill (default proportional), story?, table?, rows?: [a, b] | row?, cols?: [a, b] | col? (the target cells; default: the selection)} — an image in the target cell (its text is kept but hidden)",
            in_table,
            place_graphic
        ),
        cmd!(
            "table.textCell",
            "Convert Cell to Text Cell",
            ["Table"],
            None,
            "{story?, table?, rows?: [a, b] | row?, cols?: [a, b] | col? (the target cells; default: the selection)} — drop the target cells' graphics",
            in_table,
            |s, p| {
                let g = target(s, p, "table.textCell")?;
                edit_table(s, &g, "table.textCell", |t| {
                    for (r, c) in t.selected_owners(g.range) {
                        if let Some(cell) = t.cell_mut(r, c) {
                            cell.graphic = None;
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
            "{from?, to, story?, table?, rows?: [a, b] | row?, cols?: [a, b] | col? (the target cells; default: the selection)} (0-based; from defaults to the target row) — like dragging a row",
            in_table,
            |s, p| move_rc(s, p, true)
        ),
        cmd!(
            "table.moveColumn",
            "Move Column",
            [],
            None,
            "{from?, to, story?, table?, rows?: [a, b] | row?, cols?: [a, b] | col? (the target cells; default: the selection)} (0-based; from defaults to the target column)",
            in_table,
            |s, p| move_rc(s, p, false)
        ),
        cmd!(
            "table.sortRows",
            "Sort",
            ["Table"],
            None,
            "{column?: 0-based (default: the target cell's), descending?: bool, story?, table?, rows?: [a, b] | row?, cols?: [a, b] | col? (the target cells; default: the selection)} — body rows by that column's text (numbers numerically)",
            in_table,
            sort_rows
        ),
        cmd!(
            "table.unmerge",
            "Unmerge Cells",
            ["Table"],
            None,
            "{story?, table?, rows?: [a, b] | row?, cols?: [a, b] | col? (the target cells; default: the selection)}",
            in_table,
            unmerge
        ),
        cmd!(
            "table.splitHorizontally",
            "Split Cell Horizontally",
            ["Table"],
            None,
            "{story?, table?, rows?: [a, b] | row?, cols?: [a, b] | col? (the target cells; default: the selection)} — each target cell becomes two, one above the other",
            in_table,
            |s, p| split(s, p, true)
        ),
        cmd!(
            "table.splitVertically",
            "Split Cell Vertically",
            ["Table"],
            None,
            "{story?, table?, rows?: [a, b] | row?, cols?: [a, b] | col? (the target cells; default: the selection)} — each target cell becomes two, side by side",
            in_table,
            |s, p| split(s, p, false)
        ),
        cmd!(
            "table.setCell",
            "Cell Options",
            [],
            None,
            "{fill?: swatch, tint?: 0..1, insets?: n | [t,l,b,r], vj? | verticalJustification?: top|center|bottom|justify, firstBaseline?: ascent|capHeight|leading|xHeight|fixed|emboxHeight, firstBaselineMin?: pt, rotation?: 0|90|180|270 (Cell Options › Text › Rotation; kept and written back to IDML, composed horizontally for now), text?, stroke?: {weight?, color?: swatch, tint?: 0..1, type?: solid|dashed|dotted|thickThin|thinThick|thinThin|thickThick|thinThickThin|thickThinThick|wavy|hashed | {kind:…}, gapColor?: swatch, gapTint?: 0..1, edges?: name | [names] (all|outer|inner|top|left|bottom|right|innerHorizontal|innerVertical; default all)}, story?, table?, rows?: [a, b] | row?, cols?: [a, b] | col? (the target cells; default: the selection)} — top/left/bottom/right are the target range's outer sides, innerHorizontal/innerVertical the lines between its rows/columns",
            in_table,
            set_cell
        ),
        cmd!(
            query "table.getCellStroke",
            "Get Cell Stroke",
            [],
            None,
            "{story?, table?, rows?, cols?, edges?} → {weight, color, tint, type, gapColor, gapTint, fill, fillTint, edges: count} of the chosen edges of the target cells; a value that differs between them is null",
            in_table_or_ids,
            get_cell_stroke
        ),
        cmd!(
            "table.setRowHeight",
            "Row Height",
            [],
            None,
            "{height, mode?: atLeast|exactly, story?, table?, rows?: [a, b] | row?, cols?: [a, b] | col? (the target cells; default: the selection)}",
            in_table,
            set_row_height
        ),
        cmd!(
            "table.setColumnWidth",
            "Column Width",
            [],
            None,
            "{width, story?, table?, rows?: [a, b] | row?, cols?: [a, b] | col? (the target cells; default: the selection)}",
            in_table,
            set_col_width
        ),
        cmd!(
            "table.resizeBorder",
            "Resize Row or Column",
            [],
            None,
            "{story?, table?, frame?, axis: column|row, index, delta? | size?, keepTableSize?: bool} — moves the border after column/row `index` by `delta` (or makes that column/row `size` wide/high); later columns/rows move along. The last index is the table's end/bottom edge. keepTableSize: on an inner border the next column/row gives or takes the change; on the edge all columns/rows scale proportionally. A dragged row becomes At Least. Refuses a locked `frame`",
            in_table_or_ids,
            resize_border
        ),
        cmd!(
            "table.distributeColumns",
            "Distribute Columns Evenly",
            ["Table"],
            None,
            "{story?, table?, rows?: [a, b] | row?, cols?: [a, b] | col? (the target cells; default: the selection)}",
            in_table,
            distribute_cols
        ),
        cmd!(
            "table.distributeRows",
            "Distribute Rows Evenly",
            ["Table"],
            None,
            "{story?, table?, rows?: [a,b] | row?, cols?: [a,b] | col?} — equal fixed heights preserving the selected rows' composed total height; may overset cell text; unmerge row-spanning cells first",
            in_table,
            distribute_rows
        ),
        cmd!(
            "table.options",
            "Table Options",
            [],
            None,
            "{direction?: leftToRight|rightToLeft, border?: {weight?, color?, tint?, type?, gapColor?, gapTint?}, spaceBefore?, spaceAfter?, headerRows?, footerRows?, repeatHeader?, repeatFooter?, altRows?: {first, firstColor, firstTint, next, nextColor, nextTint, skipFirst, skipLast} | null, altCols?: … | null, story?, table?, rows?: [a, b] | row?, cols?: [a, b] | col? (the target cells; default: the selection)}",
            in_table,
            options
        ),
        cmd!(
            "table.convertRowsToHeader",
            "To Header",
            ["Table", "Convert Rows"],
            None,
            "{story?, table?, rows?: [a, b] | row?, cols?: [a, b] | col? (the target cells; default: the selection)} — the target rows, at the top of the table or right after its header rows",
            |s| can_convert_rows(s, RowKind::Header),
            |s, p| convert_rows(s, p, RowKind::Header)
        ),
        cmd!(
            "table.convertRowsToBody",
            "To Body",
            ["Table", "Convert Rows"],
            None,
            "{story?, table?, rows?: [a, b] | row?, cols?: [a, b] | col? (the target cells; default: the selection)} — the target header rows next to the body, footer rows next to it, or both",
            |s| can_convert_rows(s, RowKind::Body),
            |s, p| convert_rows(s, p, RowKind::Body)
        ),
        cmd!(
            "table.convertRowsToFooter",
            "To Footer",
            ["Table", "Convert Rows"],
            None,
            "{story?, table?, rows?: [a, b] | row?, cols?: [a, b] | col? (the target cells; default: the selection)} — the target rows, at the bottom of the table or right before its footer rows",
            |s| can_convert_rows(s, RowKind::Footer),
            |s, p| convert_rows(s, p, RowKind::Footer)
        ),
        cmd!(noundo "table.select", "Select Cells", [], None, "{story?, table?, rows?: [a,b] | row?, cols?: [a,b] | col?, what?: cell|row|column|table}", in_table_or_ids, select),
        cmd!(noundo "table.selectTable", "Select Table", ["Table", "Select"], None, "{story?, table?, rows?: [a, b] | row?, cols?: [a, b] | col? (the target cells; default: the selection)}", in_table, |s, p| {
            select(s, &crate::cmd::with_param(p, "what", json!("table")))
        }),
        cmd!(noundo "table.selectRow", "Select Row", ["Table", "Select"], None, "{story?, table?, rows?: [a, b] | row?, cols?: [a, b] | col? (the target cells; default: the selection)}", in_table, |s, p| {
            select(s, &crate::cmd::with_param(p, "what", json!("row")))
        }),
        cmd!(noundo "table.selectColumn", "Select Column", ["Table", "Select"], None, "{story?, table?, rows?: [a, b] | row?, cols?: [a, b] | col? (the target cells; default: the selection)}", in_table, |s, p| {
            select(s, &crate::cmd::with_param(p, "what", json!("column")))
        }),
        cmd!(noundo "table.nextCell", "Next Cell", [], None, "{}", in_cell, |s, _| step_cell(s, true)),
        cmd!(noundo "table.prevCell", "Previous Cell", [], None, "{}", in_cell, |s, _| step_cell(s, false)),
        cmd!(
            noundo "table.toggleSelection",
            "Toggle Text and Cell Selection",
            [],
            None,
            "{} — Esc in a table, as InDesign: the insertion point or text in a cell selects that cell; selected cells select the text of the first one",
            in_cell,
            |s, _| toggle_selection(s)
        ),
        cmd!(query "table.get", "Get Table", [], None, "{story?, table?, rows?: [a,b] | row?, cols?: [a,b] | col?} → rows, columns, cells (text, spans, fill), options", in_table_or_ids, get),
    ]
}
