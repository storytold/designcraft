use super::cell::StrokeEdit;
use super::target::{color_param, edit_table, target};
use super::{AltFills, Result, Session, Value, bad, f64_or, json};

fn parse_alt(v: &Value) -> Option<AltFills> {
    if v.is_null() {
        return None;
    }
    let d = AltFills::default();
    Some(AltFills {
        first: v.get("first").and_then(Value::as_u64).map_or(d.first, |x| u32::try_from(x).unwrap_or(u32::MAX)),
        first_color: color_param(v, "firstColor").unwrap_or(d.first_color),
        first_tint: v.get("firstTint").and_then(Value::as_f64).map_or(d.first_tint, |x| x as f32),
        next: v.get("next").and_then(Value::as_u64).map_or(d.next, |x| u32::try_from(x).unwrap_or(u32::MAX)),
        next_color: color_param(v, "nextColor").unwrap_or(d.next_color),
        next_tint: v.get("nextTint").and_then(Value::as_f64).map_or(d.next_tint, |x| x as f32),
        skip_first: v.get("skipFirst").and_then(Value::as_u64).map_or(0, |x| u32::try_from(x).unwrap_or(u32::MAX)),
        skip_last: v.get("skipLast").and_then(Value::as_u64).map_or(0, |x| u32::try_from(x).unwrap_or(u32::MAX)),
    })
}

pub(super) fn options(s: &mut Session, p: &Value) -> Result<Value> {
    let g = target(s, p, "table.options")?;
    let border = match p.get("border").filter(|v| !v.is_null()) {
        Some(b) => Some(StrokeEdit::parse(&s.doc()?.doc, b, "table.options")?),
        None => None,
    };
    let p = p.clone();
    edit_table(s, &g, "table.options", |t| {
        let o = &mut t.options;
        if let Some(v) = p.get("direction") {
            o.direction = serde_json::from_value(v.clone()).map_err(|e| bad("table.options", e.to_string()))?;
        }
        if let Some(b) = &border {
            o.border = b.apply(o.border.clone());
            o.borders = Default::default();
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
        // At most 100 of each, as in `table.insert`: the rows the table lacks are added below.
        let h = p.get("headerRows").and_then(Value::as_u64).map(|v| v.min(100) as usize);
        let f = p.get("footerRows").and_then(Value::as_u64).map(|v| v.min(100) as usize);
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

pub(super) fn get(s: &mut Session, p: &Value) -> Result<Value> {
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
                "insets": cell.insets, "vj": cell.vj, "firstBaseline": cell.first_baseline, "firstBaselineMin": cell.first_baseline_min, "rotation": cell.rotation, "style": cell.style}))
        })
        .collect();
    Ok(json!({
        "story": g.story.0, "table": g.table, "anchor": st.doc.story(g.story).and_then(|x| x.table_anchor(g.table)),
        "rows": t.rows, "columns": t.columns, "headerRows": t.header_rows(), "footerRows": t.footer_rows(),
        "cells": cells, "options": t.options, "range": g.range, "style": t.style,
    }))
}
