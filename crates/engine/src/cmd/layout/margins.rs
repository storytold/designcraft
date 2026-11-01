use super::parents::parent_spread_index;
use super::{Margins, Result, Session, Value, bad, has_doc, json};

/// Each page's margin box, by spread and page id.
pub(super) fn margin_boxes(d: &designcraft_doc::Document) -> Vec<(designcraft_doc::SpreadRef, designcraft_doc::PageId, designcraft_geom::Rect)> {
    d.spread_refs()
        .filter_map(|r| d.spread(r).map(|sp| (r, sp)))
        .flat_map(|(r, sp)| sp.pages.iter().map(move |pg| (r, pg.id, pg.margin_rect())))
        .collect()
}

/// Layout Adjustment: objects on each page move and resize from its old margin box to its new
/// one (content keeps its size; frames that auto-fit refit).
pub(super) fn adjust_layout(
    d: &mut designcraft_doc::Document,
    before: &[(designcraft_doc::SpreadRef, designcraft_doc::PageId, designcraft_geom::Rect)],
) {
    use designcraft_geom::Affine;
    let after = margin_boxes(d);
    for (r, pid, old) in before {
        let Some((_, _, new)) = after.iter().find(|(r2, p2, _)| r2 == r && p2 == pid) else { continue };
        if old == new || old.width() < 1e-6 || old.height() < 1e-6 {
            continue;
        }
        let m = Affine::translate((new.x0, new.y0))
            * Affine::scale_non_uniform(new.width() / old.width(), new.height() / old.height())
            * Affine::translate((-old.x0, -old.y0));
        let Some(sp) = d.spread(*r) else { continue };
        // The page's objects (by centre, measured against the old boxes).
        let ids: Vec<designcraft_doc::ItemId> = sp
            .items
            .iter()
            .filter(|it| {
                let c = it.bounds().center();
                // Pages share a spread side by side: the margin box's page is the one under the centre's x.
                before
                    .iter()
                    .filter(|(r2, _, _)| r2 == r)
                    .min_by(|a, b| dist(a.2, c.x).total_cmp(&dist(b.2, c.x)))
                    .is_some_and(|(_, p2, _)| p2 == pid)
            })
            .map(|it| it.id)
            .collect();
        for id in ids {
            let Some(it) = d.item_mut(id) else { continue };
            if matches!(it.content, designcraft_doc::Content::Group { .. }) {
                it.xf = m * it.xf;
            } else {
                let inner = it.xf.inverse() * m * it.xf;
                it.path.transform(inner);
                if let Some(w) = &mut it.wrap.path {
                    w.transform(inner);
                }
                let ib = it.inner_bounds();
                if let designcraft_doc::Content::Graphic(g) = &mut it.content {
                    // Content keeps its size but moves with the frame; auto-fit refits.
                    let o = inner * designcraft_geom::Point::ZERO;
                    g.xf = Affine::translate(o.to_vec2()) * g.xf;
                    if let Some(xf) = g.fitted(ib, g.auto_fit) {
                        g.xf = xf;
                    }
                }
            }
        }
    }
}

/// Horizontal distance from `x` to a margin box (0 inside).
fn dist(r: designcraft_geom::Rect, x: f64) -> f64 {
    if x < r.x0 {
        r.x0 - x
    } else if x > r.x1 {
        x - r.x1
    } else {
        0.0
    }
}

/// Apply the `margins`, `columns` and `gutter` params of `layout.marginsAndColumns` to one page.
pub(super) fn set_margins_columns(pg: &mut designcraft_doc::Page, p: &Value) {
    let width_before = pg.margin_rect().width();
    match p.get("margins") {
        Some(Value::Number(n)) => pg.margins = Margins::uniform(n.as_f64().unwrap_or(36.0)),
        Some(v @ Value::Object(_)) => {
            if let Ok(m) = serde_json::from_value(v.clone()) {
                pg.margins = m;
            }
        }
        _ => {}
    }
    scale_columns(pg, width_before);
    // A new count or gutter re-divides the columns evenly: dragged column positions are what
    // the new value replaces. The dialog always sends both, so an unchanged value keeps them.
    if let Some(c) = p.get("columns").and_then(Value::as_u64) {
        let c = u32::try_from(c).unwrap_or(u32::MAX).clamp(1, 216);
        if c != pg.columns.count {
            pg.columns.count = c;
            pg.columns.positions = None;
        }
    }
    if let Some(g) = p.get("gutter").and_then(Value::as_f64).filter(|g| g.is_finite()) {
        let g = g.max(0.0);
        if (g - pg.columns.gutter).abs() > GUTTER_TOLERANCE {
            pg.columns.gutter = g;
            pg.columns.positions = None;
        }
    }
}

/// Dragged column positions are measured across the margin box. When the box was
/// `width_before` wide and now has another width, they scale with it, as InDesign's do; a set
/// that doesn't scale into a valid one goes back to the even split rather than stay hidden.
fn scale_columns(pg: &mut designcraft_doc::Page, width_before: f64) {
    let width = pg.margin_rect().width();
    if (width - width_before).abs() <= GUTTER_TOLERANCE {
        return;
    }
    let Some(positions) = pg.columns.positions.as_mut() else { return };
    let factor = width / width_before;
    if factor.is_finite() && factor > 0.0 {
        positions.iter_mut().for_each(|p| *p *= factor);
        // The ends sit exactly on the margins whatever the rounding.
        if let Some(first) = positions.first_mut() {
            *first = 0.0;
        }
        if let Some(last) = positions.last_mut() {
            *last = width;
        }
    }
    if pg.columns.valid_positions(width).is_none() {
        pg.columns.positions = None;
    }
}

/// Gives `pg` the page width `width`, scaling its dragged columns with the margin box.
pub(super) fn set_page_width(pg: &mut designcraft_doc::Page, width: f64) {
    let before = pg.margin_rect().width();
    pg.width = width;
    scale_columns(pg, before);
}

/// How far a gutter may differ from the page's and still count as unchanged, in points. The
/// dialog shows the gutter in the ruler unit, rounded (inches to four decimals: up to 0.0036 pt
/// off), so an OK that leaves the field alone sends back a slightly different number.
const GUTTER_TOLERANCE: f64 = 0.01;

/// Column guides can be dragged: a document is open and View › Grids & Guides › Lock Column
/// Guides is off.
pub(super) fn column_guides_unlocked(s: &Session) -> std::result::Result<(), String> {
    has_doc(s)?;
    match s.active() {
        Some(st) if st.doc.settings.column_guides_locked => Err("column guides are locked (View › Grids & Guides › Lock Column Guides)".into()),
        _ => Ok(()),
    }
}

/// A column guide dragged with the Selection tool: gutter `gutter` of a page moves to `position`.
pub(super) fn move_column_guide(s: &mut Session, p: &Value) -> Result<Value> {
    const CMD: &str = "layout.moveColumnGuide";
    let index = |key: &str| -> Result<usize> {
        let v = p.get(key).ok_or_else(|| bad(CMD, format!("missing `{key}`")))?;
        v.as_u64().and_then(|n| usize::try_from(n).ok()).ok_or_else(|| bad(CMD, format!("`{key}` must be a non-negative integer")))
    };
    let (page, gutter) = (index("page")?, index("gutter")?);
    let position =
        p.get("position").and_then(Value::as_f64).filter(|v| v.is_finite()).ok_or_else(|| bad(CMD, "`position` must be a finite number"))?;
    let r = match p.get("parent") {
        None | Some(Value::Null) => crate::cmd::spread_param(p, CMD)?,
        Some(v) => designcraft_doc::SpreadRef::Parent(parent_spread_index(&s.doc()?.doc, v).ok_or_else(|| bad(CMD, "no such parent"))?),
    };
    s.edit(|d, _| {
        if d.settings.column_guides_locked {
            return Err(bad(CMD, "column guides are locked"));
        }
        let pg = d.spread_mut(r).ok_or_else(|| bad(CMD, "no such spread"))?.pages.get_mut(page).ok_or_else(|| bad(CMD, "no such page"))?;
        let m = pg.margin_rect();
        let count = pg.columns.count;
        if gutter.checked_add(1).is_none_or(|g| g >= count as usize) {
            return Err(bad(CMD, format!("the page has {} gutters", count.saturating_sub(1))));
        }
        let positions =
            pg.columns.with_gutter_moved(m.width(), gutter, position - m.x0).ok_or_else(|| bad(CMD, "the columns cannot take that position"))?;
        let left = positions.get(gutter * 2 + 1).map_or(position, |v| m.x0 + v);
        pg.columns.positions = Some(positions.clone());
        match r {
            designcraft_doc::SpreadRef::Doc(_) => pg.local_margins = true,
            designcraft_doc::SpreadRef::Parent(_) => d.sync_parent_margins(),
        }
        Ok(json!({"positions": positions, "left": left}))
    })
}

#[cfg(test)]
mod adjust_tests {
    use designcraft_doc::Margins;
    use serde_json::json;

    use crate::Session;

    #[test]
    fn adjust_layout_follows_the_new_page_size_and_margins() {
        let mut s = Session::new();
        s.execute("file.new", &json!({"pages": 1, "facingPages": false})).unwrap();
        // Letter, 36 pt margins: a frame filling the left half of the margin box.
        let id = s.execute("frame.create", &json!({"rect": [36, 36, 306, 756]})).unwrap()["id"].as_u64().unwrap();
        s.execute("layout.documentSetup", &json!({"width": 1224, "adjustLayout": true})).unwrap();
        let b = s.doc().unwrap().doc.item(designcraft_doc::ItemId(id)).unwrap().bounds();
        assert!((b.x0 - 36.0).abs() < 1e-6 && (b.x1 - 612.0).abs() < 1e-6, "half of the wider margin box: {b:?}");
        s.execute("layout.marginsAndColumns", &json!({"margins": 72, "adjustLayout": true})).unwrap();
        let b = s.doc().unwrap().doc.item(designcraft_doc::ItemId(id)).unwrap().bounds();
        assert!((b.x0 - 72.0).abs() < 1e-6 && (b.y0 - 72.0).abs() < 1e-6 && (b.y1 - 720.0).abs() < 1e-6, "{b:?}");
        // Without the option objects stay put.
        s.execute("layout.marginsAndColumns", &json!({"margins": 36})).unwrap();
        let b2 = s.doc().unwrap().doc.item(designcraft_doc::ItemId(id)).unwrap().bounds();
        assert_eq!(b, b2);
    }

    #[test]
    fn parent_margins_change_the_parent_and_follow_unoverridden_pages() {
        let mut s = Session::new();
        s.execute("file.new", &json!({"pages": 3, "facingPages": false})).unwrap();
        // Page 3 has its own margins (an override); pages 1 and 2 still follow the parent.
        s.execute("layout.marginsAndColumns", &json!({"pages": [2], "margins": 100})).unwrap();
        let parent_before = s.doc().unwrap().doc.parents[0].pages[0].margins;
        assert_eq!(parent_before, Margins::uniform(36.0));
        s.execute("layout.marginsAndColumns", &json!({"parent": 0, "margins": 60, "columns": 2})).unwrap();
        let d = &s.doc().unwrap().doc;
        // The parent page itself changed (this is what was drawn on screen).
        assert_eq!(d.parents[0].pages[0].margins, Margins::uniform(60.0));
        assert_eq!(d.parents[0].pages[0].columns.count, 2);
        // Pages without their own margins follow; the page given its own keeps them.
        assert_eq!(d.page(0).unwrap().margins, Margins::uniform(60.0));
        assert_eq!(d.page(1).unwrap().margins, Margins::uniform(60.0));
        assert_eq!(d.page(2).unwrap().margins, Margins::uniform(100.0));
        // An unknown parent is an error, not a silent no-op.
        assert!(s.execute("layout.marginsAndColumns", &json!({"parent": 9, "margins": 10})).is_err());
    }

    /// Margins and Columns re-divides the columns: a new count or gutter replaces column guides
    /// that were dragged (an imported Custom columns page), rather than being ignored by them.
    #[test]
    fn a_new_column_count_replaces_dragged_column_positions() {
        let mut s = Session::new();
        s.execute("file.new", &json!({"pages": 1, "facingPages": false})).unwrap();
        // There is no command for dragging a column guide yet, so set what an import would leave.
        let drag = |s: &mut Session| {
            s.edit(|d, _| {
                let pg = &mut std::sync::Arc::make_mut(&mut d.spreads[0]).pages[0];
                pg.columns = designcraft_doc::Columns { count: 2, gutter: 12.0, positions: Some(vec![0.0, 100.0, 130.0, 540.0]) };
                Ok(())
            })
            .unwrap();
        };

        drag(&mut s);
        let dragged = s.doc().unwrap().doc.page(0).unwrap().column_rects();
        assert_eq!(dragged.len(), 2);
        assert!((dragged[0].x1 - dragged[0].x0 - 100.0).abs() < 1e-6, "the dragged gutter is in use: {dragged:?}");

        // A new count: evenly divided again, and the stale positions are gone.
        s.execute("layout.marginsAndColumns", &json!({"columns": 3})).unwrap();
        let pg = s.doc().unwrap().doc.page(0).unwrap().clone();
        assert_eq!((pg.columns.count, pg.columns.positions.clone()), (3, None));
        assert_eq!(pg.column_rects().len(), 3);

        // So does a new gutter on its own.
        drag(&mut s);
        s.execute("layout.marginsAndColumns", &json!({"gutter": 24})).unwrap();
        let pg = s.doc().unwrap().doc.page(0).unwrap().clone();
        assert_eq!(pg.columns.positions, None);
        assert!((pg.column_rects()[1].x0 - pg.column_rects()[0].x1 - 24.0).abs() < 1e-6);

        // Margins that keep the margin box's width leave them alone; a new width scales them.
        drag(&mut s);
        s.execute("layout.marginsAndColumns", &json!({"margins": {"top": 72, "bottom": 72, "inside": 36, "outside": 36}})).unwrap();
        assert_eq!(s.doc().unwrap().doc.page(0).unwrap().columns.positions, Some(vec![0.0, 100.0, 130.0, 540.0]));
        s.execute("layout.marginsAndColumns", &json!({"margins": 72})).unwrap();
        assert_scaled(s.doc().unwrap().doc.page(0).unwrap().columns.positions.clone(), 468.0);
    }

    /// The params the Margins and Columns dialog sends on OK: every margin, the count and the
    /// gutter, whether or not the user changed them.
    fn dialog_params(margin: f64, columns: u32, gutter: f64) -> serde_json::Value {
        json!({"margins": {"top": margin, "bottom": margin, "inside": margin, "outside": margin},
            "columns": columns, "gutter": gutter, "adjustLayout": false})
    }

    const DRAGGED: [f64; 4] = [0.0, 100.0, 130.0, 540.0];

    fn dragged() -> designcraft_doc::Columns {
        designcraft_doc::Columns { count: 2, gutter: 12.0, positions: Some(DRAGGED.to_vec()) }
    }

    /// `positions` are [`DRAGGED`] scaled from its 540 pt margin box to one `width` wide.
    #[track_caller]
    fn assert_scaled(positions: Option<Vec<f64>>, width: f64) {
        let want: Vec<f64> = DRAGGED.iter().map(|p| p * width / 540.0).collect();
        let got = positions.unwrap_or_default();
        assert!(got.len() == want.len() && got.iter().zip(&want).all(|(g, w)| (g - w).abs() < 1e-6), "{got:?} ≠ {want:?}");
    }

    /// An OK in the dialog keeps dragged columns unless the count or gutter it sends differs.
    #[test]
    fn the_dialog_keeps_dragged_columns_unless_the_count_or_gutter_changes() {
        let mut s = Session::new();
        s.execute("file.new", &json!({"pages": 1, "facingPages": false})).unwrap();
        let drag = |s: &mut Session| {
            s.edit(|d, _| {
                std::sync::Arc::make_mut(&mut d.spreads[0]).pages[0].columns = dragged();
                Ok(())
            })
            .unwrap();
        };
        let positions = |s: &Session| s.doc().unwrap().doc.page(0).unwrap().columns.positions.clone();

        // The same count and gutter (the gutter as an inch field rounds it) keep them in use.
        drag(&mut s);
        s.execute("layout.marginsAndColumns", &dialog_params(36.0, 2, 12.0)).unwrap();
        assert_eq!(positions(&s), Some(DRAGGED.to_vec()));
        s.execute("layout.marginsAndColumns", &dialog_params(36.0, 2, 0.1667 * 72.0)).unwrap();
        assert_eq!(positions(&s), Some(DRAGGED.to_vec()));
        let rects = s.doc().unwrap().doc.page(0).unwrap().column_rects();
        assert!((rects[0].x1 - rects[0].x0 - 100.0).abs() < 1e-6, "the dragged columns are in use: {rects:?}");

        // Top and bottom margins leave the margin box's width alone, and the columns with it.
        let mut taller = dialog_params(36.0, 2, 12.0);
        taller["margins"]["top"] = json!(72.0);
        taller["margins"]["bottom"] = json!(72.0);
        s.execute("layout.marginsAndColumns", &taller).unwrap();
        assert_eq!(positions(&s), Some(DRAGGED.to_vec()));

        // A new width scales them with the margin box, and the old width scales them back.
        s.execute("layout.marginsAndColumns", &dialog_params(72.0, 2, 12.0)).unwrap();
        assert_scaled(positions(&s), 468.0);
        let rects = s.doc().unwrap().doc.page(0).unwrap().column_rects();
        assert!((rects[0].x1 - rects[0].x0 - 100.0 * 468.0 / 540.0).abs() < 1e-6, "the scaled columns are in use: {rects:?}");
        s.execute("layout.marginsAndColumns", &dialog_params(36.0, 2, 12.0)).unwrap();
        assert_scaled(positions(&s), 540.0);

        // A new count resets them.
        drag(&mut s);
        s.execute("layout.marginsAndColumns", &dialog_params(36.0, 3, 12.0)).unwrap();
        let pg = s.doc().unwrap().doc.page(0).unwrap().clone();
        assert_eq!((pg.columns.count, pg.columns.positions), (3, None));

        // So does a new gutter.
        drag(&mut s);
        s.execute("layout.marginsAndColumns", &dialog_params(36.0, 2, 18.0)).unwrap();
        let pg = s.doc().unwrap().doc.page(0).unwrap().clone();
        assert_eq!((pg.columns.gutter, pg.columns.positions), (18.0, None));
    }

    /// A page of another width (Page Size, Document Setup) scales its dragged columns.
    #[test]
    fn a_new_page_width_scales_dragged_columns() {
        let mut s = Session::new();
        s.execute("file.new", &json!({"pages": 1, "facingPages": false})).unwrap();
        let drag = |s: &mut Session| {
            s.edit(|d, _| {
                // A dragged page's columns are its own (`local_margins`), as the drag will make them.
                let pg = &mut std::sync::Arc::make_mut(&mut d.spreads[0]).pages[0];
                pg.columns = dragged();
                pg.local_margins = true;
                Ok(())
            })
            .unwrap();
        };
        let positions = |s: &Session| s.doc().unwrap().doc.page(0).unwrap().columns.positions.clone();

        drag(&mut s);
        s.execute("layout.pageSize", &json!({"pages": [1], "height": 900})).unwrap();
        assert_eq!(positions(&s), Some(DRAGGED.to_vec()), "a new height keeps them");
        s.execute("layout.pageSize", &json!({"pages": [1], "width": 700})).unwrap();
        assert_scaled(positions(&s), 628.0);
        s.execute("layout.pageSize", &json!({"pages": [1], "width": 612})).unwrap();
        assert_scaled(positions(&s), 540.0);

        drag(&mut s);
        s.execute("layout.documentSetup", &json!({"width": 700})).unwrap();
        assert_scaled(positions(&s), 628.0);
    }

    /// Parent pages follow the same rule, and the document pages that follow them see it.
    #[test]
    fn the_dialog_keeps_a_parents_dragged_columns_unless_the_count_or_gutter_changes() {
        let mut s = Session::new();
        s.execute("file.new", &json!({"pages": 1, "facingPages": false})).unwrap();
        let drag = |s: &mut Session| {
            s.edit(|d, _| {
                std::sync::Arc::make_mut(&mut d.parents[0]).pages[0].columns = dragged();
                d.sync_parent_margins();
                Ok(())
            })
            .unwrap();
        };
        let positions = |s: &Session| {
            let d = &s.doc().unwrap().doc;
            (d.parents[0].pages[0].columns.positions.clone(), d.page(0).unwrap().columns.positions.clone())
        };
        let on_parent = |m: f64, c: u32, g: f64| {
            let mut p = dialog_params(m, c, g);
            p["parent"] = json!(0);
            p
        };

        drag(&mut s);
        assert_eq!(positions(&s), (Some(DRAGGED.to_vec()), Some(DRAGGED.to_vec())));
        s.execute("layout.marginsAndColumns", &on_parent(36.0, 2, 12.0)).unwrap();
        assert_eq!(positions(&s), (Some(DRAGGED.to_vec()), Some(DRAGGED.to_vec())));
        s.execute("layout.marginsAndColumns", &on_parent(48.0, 2, 12.0)).unwrap();
        let (parent, follower) = positions(&s);
        assert_scaled(parent, 516.0);
        assert_scaled(follower, 516.0);

        drag(&mut s);
        s.execute("layout.marginsAndColumns", &on_parent(36.0, 3, 12.0)).unwrap();
        assert_eq!(positions(&s), (None, None));

        drag(&mut s);
        s.execute("layout.marginsAndColumns", &on_parent(36.0, 2, 24.0)).unwrap();
        assert_eq!(positions(&s), (None, None));
    }
}
