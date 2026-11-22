use super::target::{edit_table, target};
use super::{Result, RowHeightMode, Session, Table, Value, bad, compose, json, ok, str_param};

pub(super) fn set_row_height(s: &mut Session, p: &Value) -> Result<Value> {
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

pub(super) fn set_col_width(s: &mut Session, p: &Value) -> Result<Value> {
    let g = target(s, p, "table.setColumnWidth")?;
    let w = p.get("width").and_then(Value::as_f64).ok_or_else(|| bad("table.setColumnWidth", "missing width"))?.clamp(3.0, 10000.0);
    edit_table(s, &g, "table.setColumnWidth", |t| {
        for c in &mut t.columns[g.range.c0..=g.range.c1] {
            c.width = w;
        }
        ok()
    })
}

/// Smallest column width and row height a border drag leaves.
const MIN_TRACK: f64 = 3.0;

const MAX_TRACK: f64 = 10000.0;

/// Composed height of `row` (laid out from cells that span only that row), else its model height.
fn composed_row_height(cs: &compose::ComposedStory, t: &Table, row: usize) -> Option<f64> {
    let laid_out = cs
        .frames
        .iter()
        .flat_map(|f| &f.tables)
        .filter(|f| f.table == t.id)
        .flat_map(|f| &f.cells)
        .filter(|c| c.row == row && t.cell(c.row, c.col).is_some_and(|x| x.row_span <= 1))
        .map(|c| c.rect.height())
        .filter(|h| h.is_finite() && *h > 0.0)
        .reduce(f64::max);
    laid_out.or_else(|| t.rows.get(row).map(|r| r.height))
}

/// Move the border after `index` among `sizes` by `delta`. Returns the new sizes.
fn resize_track(sizes: &[f64], index: usize, delta: f64, keep: bool) -> Option<Vec<f64>> {
    let mut out = sizes.to_vec();
    let cur = *sizes.get(index)?;
    let last = index + 1 == sizes.len();
    if keep && last {
        // Scale every track so the table edge moves by `delta`.
        let total: f64 = sizes.iter().sum();
        let smallest = sizes.iter().copied().fold(f64::INFINITY, f64::min);
        let largest = sizes.iter().copied().fold(0.0, f64::max);
        if !(total > 0.0 && smallest > 0.0 && largest > 0.0) {
            return None;
        }
        let lo = (MIN_TRACK / smallest).min(1.0);
        let hi = (MAX_TRACK / largest).max(1.0);
        let f = ((total + delta) / total).clamp(lo, hi);
        out.iter_mut().for_each(|v| *v *= f);
    } else if keep {
        let next = *sizes.get(index + 1)?;
        let pair = cur + next;
        let a = (cur + delta).clamp(MIN_TRACK, (pair - MIN_TRACK).max(MIN_TRACK));
        let [this, after] = out.get_mut(index..index + 2)? else { return None };
        *this = a;
        *after = (pair - a).max(MIN_TRACK);
    } else {
        *out.get_mut(index)? = (cur + delta).clamp(MIN_TRACK, MAX_TRACK);
    }
    out.iter().all(|v| v.is_finite()).then_some(out)
}

/// The frame a border drag names must hold the table's story and be editable.
fn check_border_frame(st: &crate::DocState, p: &Value, story: designcraft_doc::StoryId) -> Result<()> {
    const ID: &str = "table.resizeBorder";
    if let Some(fid) = crate::cmd::id_param(p, "frame") {
        let it = st.doc.item(fid).ok_or_else(|| bad(ID, format!("no frame {}", fid.0)))?;
        if it.text_frame().map(|f| f.story) != Some(story) {
            return Err(bad(ID, "the table is not in that frame's story"));
        }
        if it.locked || st.doc.layer(it.layer).is_some_and(|l| l.locked) {
            return Err(bad(ID, "the frame or its layer is locked"));
        }
    }
    Ok(())
}

/// The change in track size: `delta`, or `size` measured from the current size `cur`.
fn border_delta(p: &Value, cur: f64) -> Result<f64> {
    const ID: &str = "table.resizeBorder";
    let delta = match (p.get("delta").and_then(Value::as_f64), p.get("size").and_then(Value::as_f64)) {
        (Some(d), _) => d,
        (None, Some(sz)) => sz - cur,
        (None, None) => return Err(bad(ID, "`delta` or `size` required")),
    };
    if !delta.is_finite() {
        return Err(bad(ID, "`delta`/`size` must be a finite number"));
    }
    Ok(delta)
}

pub(super) fn resize_border(s: &mut Session, p: &Value) -> Result<Value> {
    const ID: &str = "table.resizeBorder";
    let column = match str_param(p, "axis") {
        Some("column") => true,
        Some("row") => false,
        _ => return Err(bad(ID, "axis must be `column` or `row`")),
    };
    let index = p.get("index").and_then(Value::as_u64).and_then(|v| usize::try_from(v).ok()).ok_or_else(|| bad(ID, "`index` required"))?;
    let keep = p.get("keepTableSize").and_then(Value::as_bool).unwrap_or(false);
    let g = target(s, p, ID)?;
    let st = s.doc()?;
    check_border_frame(st, p, g.story)?;
    let t = st.doc.story(g.story).and_then(|x| x.tables.get(&g.table)).ok_or_else(|| bad(ID, "no table"))?;
    let sizes: Vec<f64> = if column {
        t.columns.iter().map(|c| c.width).collect()
    } else {
        let cs = s.cache.get(&st.doc, g.story, None);
        (0..t.nrows()).map(|r| composed_row_height(&cs, t, r).unwrap_or(MIN_TRACK)).collect()
    };
    let cur = *sizes.get(index).ok_or_else(|| bad(ID, format!("index {index} out of range")))?;
    let delta = border_delta(p, cur)?;
    let new = resize_track(&sizes, index, delta, keep).ok_or_else(|| bad(ID, "invalid table geometry"))?;
    let size = new.get(index).copied().unwrap_or(cur);
    let scaled = keep && index + 1 == sizes.len();
    edit_table(s, &g, ID, |t| {
        if column {
            for (c, w) in t.columns.iter_mut().zip(&new) {
                c.width = *w;
            }
        } else {
            for (i, (r, h)) in t.rows.iter_mut().zip(&new).enumerate() {
                // Only the rows the drag changed take the new height.
                if scaled || i == index || (keep && i == index + 1) {
                    r.height = *h;
                }
                if i == index && !scaled {
                    r.mode = RowHeightMode::AtLeast;
                }
            }
        }
        Ok(json!({"axis": if column { "column" } else { "row" }, "index": index, "size": size}))
    })
}

pub(super) fn distribute_rows(s: &mut Session, p: &Value) -> Result<Value> {
    const ID: &str = "table.distributeRows";
    if !p.is_object() {
        return Err(bad(ID, "parameters must be an object"));
    }
    if let Some(rows) = p.get("rows") {
        let valid = rows.as_array().is_some_and(|a| a.len() == 2 && a.iter().all(|v| v.as_u64().is_some_and(|n| usize::try_from(n).is_ok())));
        if !valid {
            return Err(bad(ID, "rows must be two non-negative row indices"));
        }
    }
    for key in ["row", "story", "table"] {
        if let Some(v) = p.get(key)
            && v.as_u64().is_none_or(|n| usize::try_from(n).is_err())
        {
            return Err(bad(ID, format!("{key} must be a non-negative integer")));
        }
    }
    let g = target(s, p, ID)?;
    let st = s.doc()?;
    let t = st.doc.story(g.story).and_then(|x| x.tables.get(&g.table)).ok_or_else(|| bad(ID, "no table"))?;
    if t.cells.iter().any(|c| c.row_span > 1) {
        return Err(bad(ID, "unmerge cells that span rows first"));
    }
    let cs = s.cache.get(&st.doc, g.story, None);
    let mut total = 0.0;
    for row in g.range.r0..=g.range.r1 {
        let height = cs
            .frames
            .iter()
            .flat_map(|f| &f.tables)
            .filter(|f| f.table == g.table)
            .flat_map(|f| &f.cells)
            .filter(|c| c.row == row)
            .map(|c| c.rect.height())
            .reduce(f64::max)
            .ok_or_else(|| bad(ID, "all selected rows must be laid out; enlarge or thread the text frame first"))?;
        if !height.is_finite() || height <= 0.0 {
            return Err(bad(ID, "invalid composed row height"));
        }
        total += height;
    }
    let count = g.range.r1 - g.range.r0 + 1;
    let height = total / count as f64;
    if !height.is_finite() {
        return Err(bad(ID, "invalid total row height"));
    }
    edit_table(s, &g, ID, |t| {
        let rows = t.rows.get_mut(g.range.r0..=g.range.r1).ok_or_else(|| bad(ID, "invalid row range"))?;
        for row in rows {
            row.height = height;
            row.mode = RowHeightMode::Exactly;
        }
        Ok(json!({"rows": count, "height": height}))
    })
}

pub(super) fn distribute_cols(s: &mut Session, p: &Value) -> Result<Value> {
    let g = target(s, p, "table.distributeColumns")?;
    edit_table(s, &g, "table.distributeColumns", |t| {
        let cols = &mut t.columns[g.range.c0..=g.range.c1];
        let avg = cols.iter().map(|c| c.width).sum::<f64>() / cols.len() as f64;
        cols.iter_mut().for_each(|c| c.width = avg);
        ok()
    })
}

#[cfg(test)]
mod resize_border_tests {
    use serde_json::json;

    use crate::Session;

    /// A 3×3 table of 80 pt columns; returns the session, frame id, story and table ids.
    fn session() -> (Session, u64, u64, u64) {
        let mut s = Session::new();
        s.execute("file.new", &json!({})).unwrap();
        let r = s.execute("frame.create", &json!({"rect": [72, 72, 472, 600], "content": "text", "text": ""})).unwrap();
        s.execute("text.select", &json!({"story": r["story"], "anchor": 0, "focus": 0})).unwrap();
        let t = s.execute("table.insert", &json!({"rows": 3, "cols": 3, "width": 240})).unwrap();
        (s, r["id"].as_u64().unwrap(), t["story"].as_u64().unwrap(), t["table"].as_u64().unwrap())
    }

    fn table(s: &Session) -> designcraft_doc::Table {
        (**s.doc().unwrap().doc.stories.values().flat_map(|st| st.tables.values()).next().unwrap()).clone()
    }

    fn widths(s: &Session) -> Vec<f64> {
        table(s).columns.iter().map(|c| c.width).collect()
    }

    /// Composed cell rects of column 0, top to bottom.
    fn rows(s: &Session) -> Vec<designcraft_geom::Rect> {
        let st = s.doc().unwrap();
        let sid = st.doc.stories.values().find(|x| !x.tables.is_empty()).unwrap().id;
        let cs = s.cache.get(&st.doc, sid, None);
        cs.frames.iter().flat_map(|f| &f.tables).flat_map(|f| &f.cells).filter(|c| c.col == 0).map(|c| c.rect).collect()
    }

    fn resize(s: &mut Session, ids: (u64, u64, u64), mut p: serde_json::Value) {
        p["frame"] = json!(ids.0);
        p["story"] = json!(ids.1);
        p["table"] = json!(ids.2);
        s.execute("table.resizeBorder", &p).unwrap();
    }

    #[test]
    fn column_border_drags_widen_the_table_or_keep_its_width() {
        let (mut s, f, st, t) = session();
        resize(&mut s, (f, st, t), json!({"axis": "column", "index": 0, "delta": 20}));
        assert_eq!(widths(&s), [100.0, 80.0, 80.0]);
        resize(&mut s, (f, st, t), json!({"axis": "column", "index": 1, "delta": 30, "keepTableSize": true}));
        assert_eq!(widths(&s), [100.0, 110.0, 50.0]);
        // Clamped at the minimum; the neighbour takes only what is left.
        resize(&mut s, (f, st, t), json!({"axis": "column", "index": 1, "delta": 500, "keepTableSize": true}));
        assert_eq!(widths(&s), [100.0, 157.0, 3.0]);
        resize(&mut s, (f, st, t), json!({"axis": "column", "index": 0, "delta": -1000}));
        assert_eq!(widths(&s), [3.0, 157.0, 3.0]);
        // Shift on the end edge scales every column.
        resize(&mut s, (f, st, t), json!({"axis": "column", "index": 2, "size": 166, "keepTableSize": true}));
        let w = widths(&s);
        assert!((w.iter().sum::<f64>() - 326.0).abs() < 1e-9, "{w:?}");
        assert!((w[1] / w[0] - 157.0 / 3.0).abs() < 1e-9, "{w:?}");
        assert!(s.execute("table.resizeBorder", &json!({"table": t, "axis": "column", "index": 3, "delta": 1})).is_err());
        assert!(s.execute("table.resizeBorder", &json!({"table": t, "axis": "column", "index": 0})).is_err());
    }

    #[test]
    fn row_drag_sets_at_least_and_moves_later_rows() {
        let (mut s, f, st, t) = session();
        s.execute("table.setRowHeight", &json!({"table": t, "row": 0, "height": 30, "mode": "exactly"})).unwrap();
        let before = rows(&s);
        resize(&mut s, (f, st, t), json!({"axis": "row", "index": 0, "delta": 12}));
        let row = &table(&s).rows[0];
        assert_eq!((row.height, row.mode), (42.0, designcraft_doc::RowHeightMode::AtLeast));
        let after = rows(&s);
        assert!((after[0].height() - 42.0).abs() < 1e-9);
        assert!((after[1].y0 - before[1].y0 - 12.0).abs() < 1e-9 && (after[1].height() - before[1].height()).abs() < 1e-9);
        // Below the minimum: clamped, never negative.
        resize(&mut s, (f, st, t), json!({"axis": "row", "index": 0, "delta": -1000}));
        assert_eq!(table(&s).rows[0].height, 3.0);
    }

    #[test]
    fn a_drag_is_one_undo_step_and_locked_frames_refuse() {
        let (mut s, f, st, t) = session();
        let before = table(&s);
        s.begin_interaction("Resize Column").unwrap();
        for d in [5.0, 15.0, 25.0] {
            let p = json!({"frame": f, "story": st, "table": t, "axis": "column", "index": 0, "delta": d});
            s.preview("table.resizeBorder", p).unwrap();
        }
        s.commit_interaction().unwrap();
        assert_eq!(widths(&s), [105.0, 80.0, 80.0]);
        s.execute("edit.undo", &json!({})).unwrap();
        assert_eq!(table(&s), before);
        s.execute("object.setFlags", &json!({"ids": [f], "locked": true})).unwrap();
        assert!(s.execute("table.resizeBorder", &json!({"frame": f, "table": t, "axis": "column", "index": 0, "delta": 5})).is_err());
    }
}

#[cfg(test)]
mod insert_size_tests {
    use serde_json::json;

    use crate::Session;

    /// A table size that is given but is not one a table can have used to be corrected in
    /// silence: `{"rows": 0}` made a one-row table, `-1` and `1.5` made the default four, and
    /// 10000 was capped at 500 — so a caller's mistake became a table nobody asked for.
    #[test]
    fn a_table_size_insert_cannot_mean_is_refused() {
        let mut s = Session::new();
        s.execute("file.new", &json!({"pages": 1})).unwrap();
        let frame = s.execute("frame.create", &json!({"rect": [20, 20, 400, 300], "content": "text", "text": ""})).unwrap();
        let at = |s: &mut Session| {
            s.execute("text.select", &json!({"story": frame["story"].clone(), "anchor": 0, "focus": 0})).unwrap();
        };
        for (params, names) in [
            (json!({"rows": 0}), "rows 0: 1 to 500"),
            (json!({"rows": -1}), "rows -1: 1 to 500"),
            (json!({"rows": 1.5}), "rows 1.5: 1 to 500"),
            (json!({"rows": 10000}), "rows 10000: 1 to 500"),
            (json!({"cols": 0}), "cols 0: 1 to 200"),
            (json!({"columns": 201}), "cols 201: 1 to 200"),
            (json!({"headerRows": 101}), "headerRows 101: 0 to 100"),
            (json!({"footerRows": -2}), "footerRows -2: 0 to 100"),
        ] {
            at(&mut s);
            let e = s.execute("table.insert", &params).expect_err("refused").to_string();
            assert!(e.contains(names), "{params}: {e}");
        }
        // Nothing was inserted by any of those.
        at(&mut s);
        assert!(s.execute("table.get", &json!({})).is_err(), "no table yet");
        // The sizes a table can have still work, and absent means the default four by four.
        at(&mut s);
        s.execute("table.insert", &json!({})).unwrap();
        let t = s.execute("table.get", &json!({})).unwrap();
        assert_eq!(t["cells"].as_array().unwrap().len(), 16, "four by four");
        at(&mut s);
        s.execute("table.insert", &json!({"rows": 1, "cols": 2, "headerRows": 1})).unwrap();
        let t = s.execute("table.get", &json!({})).unwrap();
        assert_eq!(t["cells"].as_array().unwrap().len(), 4, "one body row and one header row, two columns");
    }
}
