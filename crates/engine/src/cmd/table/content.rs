use super::selection::cell_at;
use super::target::{clamp_selection, edit_table, target};
use super::{CellRange, Result, Session, TableSel, Value, bad, json};

pub(super) fn drop_cells(s: &mut Session, p: &Value) -> Result<Value> {
    const ID: &str = "table.dropCells";
    let frame = crate::cmd::id_param(p, "frame").ok_or_else(|| bad(ID, "`frame` required"))?;
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
        && s.doc()?.doc.story(sid).and_then(|st| st.tables.get(&tid)).is_some_and(|t| t.selected_owners(ts.range).binary_search(&(fr, fc)).is_ok())
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

pub(super) fn place_graphic(s: &mut Session, p: &Value) -> Result<Value> {
    const ID: &str = "table.placeGraphic";
    let g = target(s, p, ID)?;
    let (bytes, name, link) = crate::cmd::file::read_source(p)?;
    let (pw, ph) = designcraft_render::image_size(&bytes).ok_or_else(|| bad(ID, "unsupported or corrupt image"))?;
    let (nw, nh) = designcraft_render::natural_size(&bytes, 0).ok_or_else(|| bad(ID, "invalid image dimensions"))?;
    let fill = p.get("fit").and_then(Value::as_str) == Some("fill");
    // Compute fitting before the edit: a small EPS can overflow in a huge cell.
    let t = s.doc()?.doc.story(g.story).and_then(|st| st.tables.get(&g.table)).ok_or_else(|| bad(ID, "no table"))?;
    let (r, c) = t.owner(g.range.r0, g.range.c0);
    let cell = t.cell(r, c).ok_or_else(|| bad(ID, "no cell"))?;
    let (rs, cs) = (cell.row_span.max(1) as usize, cell.col_span.max(1) as usize);
    let w: f64 = t.columns.get(c..c.saturating_add(cs).min(t.ncols())).ok_or_else(|| bad(ID, "invalid cell span"))?.iter().map(|x| x.width).sum();
    let h: f64 = t.rows.get(r..r.saturating_add(rs).min(t.nrows())).ok_or_else(|| bad(ID, "invalid cell span"))?.iter().map(|x| x.height).sum();
    let ins = cell.insets;
    let (bw, bh) = ((w - ins[1] - ins[3]).max(1.0), (h - ins[0] - ins[2]).max(1.0));
    let k = if fill { (bw / nw).max(bh / nh) } else { (bw / nw).min(bh / nh) };
    let xf = designcraft_geom::Affine::translate(((bw - nw * k) / 2.0, (bh - nh * k) / 2.0)) * designcraft_geom::Affine::scale(k);
    crate::cmd::file::validate_placement((nw, nh), designcraft_geom::Rect::new(0.0, 0.0, bw, bh), xf, ID)?;
    s.edit(|d, sel| {
        let fresh = designcraft_doc::AssetId(d.alloc());
        let mime = designcraft_render::image_mime(&bytes).to_string();
        let aid = d.add_asset(designcraft_doc::Asset {
            id: fresh,
            name,
            mime,
            link,
            data: std::sync::Arc::new(bytes),
            pixels: Some((pw, ph)),
            ..Default::default()
        });
        let st = d.story_mut(g.story).ok_or_else(|| bad(ID, "no story"))?;
        let t = st.table_mut(g.table).ok_or_else(|| bad(ID, "no table"))?;
        let cell = t.cell_mut(r, c).ok_or_else(|| bad(ID, format!("no cell {r},{c}")))?;
        cell.graphic = Some(designcraft_doc::Graphic {
            layer_options: None,
            asset: aid,
            size: (nw, nh),
            xf,
            clip: None,
            auto_fit: if fill { designcraft_doc::Fitting::FillProportionally } else { designcraft_doc::Fitting::FitProportionally },
            fit_align: 4,
            crop: [0.0; 4],
            wrap: Default::default(),
            fill: Default::default(),
        });
        clamp_selection(d, sel);
        Ok(json!({"asset": aid.0, "row": r, "col": c}))
    })
}

pub(super) fn sort_rows(s: &mut Session, p: &Value) -> Result<Value> {
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

    fn distribution_session() -> Session {
        let mut s = Session::new();
        s.execute("file.new", &json!({})).unwrap();
        let r = s.execute("frame.create", &json!({"rect": [72, 72, 500, 700], "content": "text", "text": ""})).unwrap();
        s.execute("text.select", &json!({"story": r["story"], "anchor": 0, "focus": 0})).unwrap();
        s.execute("table.insert", &json!({"rows": 3, "cols": 2, "headerRows": 1})).unwrap();
        for (row, height) in [(0, 25), (1, 40), (2, 80), (3, 30)] {
            s.execute("table.setRowHeight", &json!({"row": row, "height": height, "mode": "exactly"})).unwrap();
            s.execute("table.setCell", &json!({"row": row, "col": 0, "text": format!("row {row}")})).unwrap();
        }
        s
    }

    fn table(s: &Session) -> designcraft_doc::Table {
        (**s.doc().unwrap().doc.stories.values().flat_map(|st| st.tables.values()).next().unwrap()).clone()
    }

    #[test]
    fn distribute_selected_rows_preserves_height_content_and_undo() {
        let mut s = distribution_session();
        s.execute("table.select", &json!({"rows": [1, 2], "what": "row"})).unwrap();
        let before = table(&s);
        let selection = s.doc().unwrap().selection.cells;
        let result = s.execute("table.distributeRows", &json!({})).unwrap();
        assert_eq!(result, json!({"rows": 2, "height": 60.0}));
        let after = table(&s);
        assert_eq!(after.rows.iter().map(|r| r.height).collect::<Vec<_>>(), [25.0, 60.0, 60.0, 30.0]);
        assert_eq!(after.cells, before.cells);
        assert_eq!(after.rows[0].kind, before.rows[0].kind);
        assert_eq!(s.doc().unwrap().selection.cells, selection);
        let st = s.doc().unwrap();
        let sid = st.doc.stories.values().find(|st| !st.tables.is_empty()).unwrap().id;
        let cs = s.cache.get(&st.doc, sid, None);
        let heights: Vec<_> =
            cs.frames.iter().flat_map(|f| &f.tables).flat_map(|f| &f.cells).filter(|c| c.col == 0).map(|c| c.rect.height()).collect();
        assert_eq!(heights, [25.0, 60.0, 60.0, 30.0]);
        s.execute("edit.undo", &json!({})).unwrap();
        assert_eq!(table(&s), before);
        s.execute("edit.redo", &json!({})).unwrap();
        assert_eq!(table(&s), after);
    }

    #[test]
    fn distribution_uses_content_grown_heights_and_explicit_table() {
        let mut s = distribution_session();
        s.execute("table.setRowHeight", &json!({"rows": [1, 2], "height": 3, "mode": "atLeast"})).unwrap();
        s.execute("table.setCell", &json!({"row": 2, "col": 0, "text": "one\ntwo\nthree"})).unwrap();
        let st = s.doc().unwrap();
        let story = st.doc.stories.values().find(|st| !st.tables.is_empty()).unwrap();
        let (sid, tid) = (story.id, *story.tables.keys().next().unwrap());
        let cs = s.cache.get(&st.doc, sid, None);
        let total: f64 = cs
            .frames
            .iter()
            .flat_map(|f| &f.tables)
            .flat_map(|f| &f.cells)
            .filter(|c| c.col == 0 && (1..=2).contains(&c.row))
            .map(|c| c.rect.height())
            .sum();
        let result = s.execute("table.distributeRows", &json!({"story": sid.0, "table": tid, "rows": [1, 2]})).unwrap();
        assert!((result["height"].as_f64().unwrap() * 2.0 - total).abs() < 1e-6);
        assert!(table(&s).rows[1].height > 3.0);
        assert_eq!(table(&s).rows[1].mode, designcraft_doc::RowHeightMode::Exactly);
    }

    #[test]
    fn distribution_keeps_column_spans_and_caret() {
        let mut s = distribution_session();
        s.execute("table.merge", &json!({"rows": [1, 1], "cols": [0, 1]})).unwrap();
        let before = table(&s);
        let caret = s.doc().unwrap().selection.text;
        let result = s.execute("table.distributeRows", &json!({"rows": [2, 1]})).unwrap();
        assert_eq!(result["height"], 60.0);
        assert_eq!(table(&s).cell(1, 0).unwrap().col_span, 2);
        assert_eq!(table(&s).cells, before.cells);
        assert_eq!(s.doc().unwrap().selection.text, caret);
    }

    #[test]
    fn distribution_rejects_merges_and_missing_rows_without_changes() {
        let mut s = distribution_session();
        s.execute("table.merge", &json!({"rows": [1, 2], "cols": [0, 0]})).unwrap();
        let before = table(&s);
        assert!(s.execute("table.distributeRows", &json!({"rows": [1, 2]})).is_err());
        assert_eq!(table(&s), before);
        s.execute("edit.undo", &json!({})).unwrap();
        s.execute("table.setRowHeight", &json!({"rows": [0, 3], "height": 10000, "mode": "exactly"})).unwrap();
        let before = table(&s);
        assert!(s.execute("table.distributeRows", &json!({"rows": [0, 3]})).is_err());
        assert_eq!(table(&s), before);
        assert!(s.execute("table.distributeRows", &json!({"rows": [0, 99]})).is_err());
        for params in
            [json!({"rows": "all"}), json!({"rows": [0]}), json!({"rows": [-1, 2]}), json!({"row": 1.5}), json!({"table": "bad"}), json!([])]
        {
            assert!(s.execute("table.distributeRows", &params).is_err(), "{params}");
            assert_eq!(table(&s), before);
        }
    }

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
        let g = s.execute("table.placeGraphic", &json!({"base64": crate::cmd::file::base64_encode(&png), "name": "r.png", "fit": "fill"})).unwrap();
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

    #[test]
    fn move_row_cannot_move_a_header_row_out_of_place() {
        let mut s = Session::new();
        s.execute("file.new", &json!({})).unwrap();
        let r = s.execute("frame.create", &json!({"rect": [72, 72, 500, 400], "content": "text", "text": ""})).unwrap();
        s.execute("text.select", &json!({"story": r["story"], "anchor": 0, "focus": 0})).unwrap();
        s.execute("table.insert", &json!({"rows": 3, "cols": 2, "headerRows": 1})).unwrap();
        assert!(s.execute("table.moveRow", &json!({"from": 0, "to": 2})).is_err());
        s.doc().unwrap().doc.check().unwrap();
        // Dragging the selected header row onto the last body row is refused too.
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
        assert!(s.execute("table.dropCells", &json!({"frame": fid.0, "from": [from.x, from.y], "to": [to.x, to.y]})).is_err());
        s.doc().unwrap().doc.check().unwrap();
    }

    #[test]
    fn table_options_limits_header_and_footer_rows() {
        let mut s = Session::new();
        s.execute("file.new", &json!({})).unwrap();
        let r = s.execute("frame.create", &json!({"rect": [72, 72, 500, 700], "content": "text", "text": ""})).unwrap();
        s.execute("text.select", &json!({"story": r["story"], "anchor": 0, "focus": 0})).unwrap();
        s.execute("table.insert", &json!({"rows": 2, "cols": 2})).unwrap();
        // The limit `table.insert` has: the table used to grow by as many rows as were asked for.
        s.execute("table.options", &json!({"headerRows": 5000, "footerRows": 5000})).unwrap();
        let t = table(&s);
        assert_eq!((t.header_rows(), t.footer_rows(), t.nrows()), (100, 100, 201));
    }
}
