use super::parents::parent_spread_index;
use super::{Margins, Result, Session, Value, bad, has_doc, json};

/// Each page's margin box, by spread and page id.
pub(super) fn margin_boxes(d: &designcraft_doc::Document) -> Vec<(designcraft_doc::SpreadRef, designcraft_doc::PageId, designcraft_geom::Rect)> {
    d.spread_refs()
        .filter_map(|r| d.spread(r).map(|sp| (r, sp)))
        .flat_map(|(r, sp)| sp.pages.iter().map(move |pg| (r, pg.id, pg.margin_rect())))
        .collect()
}

/// Options controlling Layout Adjustment (`layout.adjust`).
#[derive(Clone, Copy, Debug, Default)]
pub(super) struct AdjustOptions {
    pub adjust_font_size: bool,
    pub font_limits: Option<(f64, f64)>,
    pub adjust_locked_content: bool,
}

/// Layout Adjustment: objects on each page move and resize from its old margin box to its new
/// one (content keeps its size; frames that auto-fit refit).
pub(super) fn adjust_layout(
    d: &mut designcraft_doc::Document,
    before: &[(designcraft_doc::SpreadRef, designcraft_doc::PageId, designcraft_geom::Rect)],
) {
    adjust_layout_with(d, before, AdjustOptions::default());
}

/// Captures each eligible top-level item's page assignment from `before` margin boxes before any
/// page geometry or spread layout changes.
pub(super) fn assign_items(
    d: &designcraft_doc::Document,
    before: &[(designcraft_doc::SpreadRef, designcraft_doc::PageId, designcraft_geom::Rect)],
    adjust_locked_content: bool,
) -> Vec<(designcraft_doc::PageId, designcraft_doc::ItemId)> {
    let locked_layers: Vec<designcraft_doc::LayerId> =
        if adjust_locked_content { Vec::new() } else { d.layers.iter().filter(|l| l.locked).map(|l| l.id).collect() };
    let mut seen_spreads = Vec::new();
    let mut assigned = Vec::new();
    for (r, _, _) in before {
        if seen_spreads.contains(r) {
            continue;
        }
        seen_spreads.push(*r);
        let Some(sp) = d.spread(*r) else { continue };
        for it in &sp.items {
            if !(adjust_locked_content || (!it.locked && !locked_layers.contains(&it.layer))) {
                continue;
            }
            let c = it.bounds().center();
            // Pages share a spread side by side: the margin box's page is the one under the centre's x.
            if let Some((_, pid, _)) = before.iter().filter(|(r2, _, _)| r2 == r).min_by(|a, b| dist(a.2, c.x).total_cmp(&dist(b.2, c.x))) {
                assigned.push((*pid, it.id));
            }
        }
    }
    assigned
}

/// Layout Adjustment with explicit options, deriving page assignments from `before`.
pub(super) fn adjust_layout_with(
    d: &mut designcraft_doc::Document,
    before: &[(designcraft_doc::SpreadRef, designcraft_doc::PageId, designcraft_geom::Rect)],
    opts: AdjustOptions,
) {
    let assigned = assign_items(d, before, opts.adjust_locked_content);
    adjust_layout_assigned(d, before, &assigned, opts);
}

/// Layout Adjustment using pre-captured page assignments so spread relayout or repagination
/// cannot reassign items to another page.
pub(super) fn adjust_layout_assigned(
    d: &mut designcraft_doc::Document,
    before: &[(designcraft_doc::SpreadRef, designcraft_doc::PageId, designcraft_geom::Rect)],
    assigned: &[(designcraft_doc::PageId, designcraft_doc::ItemId)],
    opts: AdjustOptions,
) {
    use designcraft_geom::Affine;
    let after = margin_boxes(d);
    let locked_layers: Vec<designcraft_doc::LayerId> =
        if opts.adjust_locked_content { Vec::new() } else { d.layers.iter().filter(|l| l.locked).map(|l| l.id).collect() };
    let mut story_scales: std::collections::BTreeMap<designcraft_doc::StoryId, f64> = std::collections::BTreeMap::new();
    for (_, pid, old) in before {
        let Some((_, _, new)) = after.iter().find(|(_, p2, _)| p2 == pid) else { continue };
        if old == new || old.width() < 1e-6 || old.height() < 1e-6 || new.width() < 1e-6 || new.height() < 1e-6 {
            continue;
        }
        let sx = new.width() / old.width();
        let sy = new.height() / old.height();
        let font_scale = (sx * sy).sqrt();
        let m = Affine::translate((new.x0, new.y0)) * Affine::scale_non_uniform(sx, sy) * Affine::translate((-old.x0, -old.y0));
        let ids: Vec<designcraft_doc::ItemId> = assigned.iter().filter(|(p2, _)| p2 == pid).map(|(_, id)| *id).collect();
        let story_filter = if opts.adjust_locked_content { StoryFilter::All } else { StoryFilter::UnlockedOnly };
        for id in ids {
            let Some(it) = d.item_mut(id) else { continue };
            adjust_item(it, m, opts.adjust_locked_content, &locked_layers, 0);
            if opts.adjust_font_size {
                let mut sids = Vec::new();
                collect_stories(it, story_filter, false, &locked_layers, 0, &mut sids);
                for sid in sids {
                    story_scales.entry(sid).or_insert(font_scale);
                }
            }
        }
    }
    if opts.adjust_font_size && !opts.adjust_locked_content && !story_scales.is_empty() {
        // A story threaded into any locked frame (or locked layer/ancestor) keeps its text size.
        let mut locked_stories = Vec::new();
        for r in d.spread_refs() {
            let Some(sp) = d.spread(r) else { continue };
            for it in &sp.items {
                collect_stories(it, StoryFilter::LockedOnly, false, &locked_layers, 0, &mut locked_stories);
            }
        }
        for sid in locked_stories {
            story_scales.remove(&sid);
        }
    }
    if opts.adjust_font_size && !story_scales.is_empty() {
        let styles = d.styles.clone();
        for (sid, scale) in story_scales {
            if let Some(story) = d.story_mut(sid) {
                scale_story_fonts(story, &styles, scale, opts.font_limits, 0);
            }
        }
    }
}

/// Transforms an item and any eligible nested group children in their own path spaces without
/// scaling group transforms, so text frames inside groups scale fonts only once.
fn adjust_item(
    it: &mut designcraft_doc::Item,
    m: designcraft_geom::Affine,
    locked_ok: bool,
    locked_layers: &[designcraft_doc::LayerId],
    depth: usize,
) {
    if depth >= 32 {
        return;
    }
    let inner = it.xf.inverse() * m * it.xf;
    it.path.transform(inner);
    if let Some(w) = &mut it.wrap.path {
        w.transform(inner);
    }
    let ib = it.inner_bounds();
    match &mut it.content {
        designcraft_doc::Content::Group { items } => {
            for child in items {
                if !locked_ok && (child.locked || locked_layers.contains(&child.layer)) {
                    continue;
                }
                adjust_item(std::sync::Arc::make_mut(child), inner, locked_ok, locked_layers, depth + 1);
            }
        }
        designcraft_doc::Content::Graphic(g) => {
            // Content keeps its size and moves its own origin with the frame; auto-fit refits.
            let origin = g.xf * designcraft_geom::Point::ZERO;
            g.xf = designcraft_geom::Affine::translate((inner * origin) - origin) * g.xf;
            if let Some(xf) = g.fitted(ib, g.auto_fit) {
                g.xf = xf;
            }
        }
        designcraft_doc::Content::Unassigned | designcraft_doc::Content::Text(_) => {}
    }
}

#[derive(Clone, Copy)]
enum StoryFilter {
    All,
    UnlockedOnly,
    LockedOnly,
}

/// Collects story IDs from text frames and nested group children matching `filter`, up to a bounded depth.
fn collect_stories(
    it: &designcraft_doc::Item,
    filter: StoryFilter,
    parent_locked: bool,
    locked_layers: &[designcraft_doc::LayerId],
    depth: usize,
    out: &mut Vec<designcraft_doc::StoryId>,
) {
    if depth >= 32 {
        return;
    }
    let is_locked = parent_locked || it.locked || locked_layers.contains(&it.layer);
    if matches!(filter, StoryFilter::UnlockedOnly) && is_locked {
        return;
    }
    match &it.content {
        designcraft_doc::Content::Text(tf) => {
            let take = match filter {
                StoryFilter::All => true,
                StoryFilter::UnlockedOnly => !is_locked,
                StoryFilter::LockedOnly => is_locked,
            };
            if take {
                out.push(tf.story);
            }
        }
        designcraft_doc::Content::Group { items } => {
            for child in items {
                collect_stories(child, filter, is_locked, locked_layers, depth + 1, out);
            }
        }
        designcraft_doc::Content::Unassigned | designcraft_doc::Content::Graphic(_) => {}
    }
}

/// Scales resolved character font sizes (and explicit point leading) across a story,
/// including anchored tables, footnotes, and endnotes.
fn scale_story_fonts(story: &mut designcraft_doc::Story, styles: &designcraft_doc::Styles, scale: f64, limits: Option<(f64, f64)>, depth: usize) {
    if depth >= 32 || !scale.is_finite() || scale <= 0.0 || (scale - 1.0).abs() < 1e-6 {
        return;
    }
    let scale_char = |para_chars: &designcraft_doc::CharProps, fmt: &mut designcraft_doc::CharFormat| {
        let cp = styles.resolve_char(para_chars, fmt);
        let cur = cp.size;
        if !cur.is_finite() || cur <= 0.0 {
            return;
        }
        let mut next = cur * scale;
        if let Some((min, max)) = limits {
            next = next.clamp(min, max);
        }
        next = next.clamp(designcraft_doc::MIN_TYPE_SIZE, designcraft_doc::MAX_TYPE_SIZE);
        fmt.over.size = Some(next);
        if let designcraft_doc::Leading::Points(lead) = cp.leading {
            let ratio = next / cur;
            if ratio.is_finite() && ratio > 0.0 {
                fmt.over.leading = Some(designcraft_doc::Leading::Points((lead * ratio).clamp(0.0, designcraft_doc::MAX_MEASURE)));
            }
        }
    };
    if story.text.is_empty() {
        let (_, para_chars) = styles.resolve_para(story.paras.first().unwrap_or(&designcraft_doc::ParaFormat::default()));
        if let Some(run) = story.chars.first_mut() {
            scale_char(&para_chars, &mut run.format);
            story.rev = story.rev.wrapping_add(1);
        }
    } else {
        let ranges = story.para_ranges();
        for (pi, r) in ranges.into_iter().enumerate() {
            let (_, para_chars) = styles.resolve_para(story.paras.get(pi).unwrap_or(&designcraft_doc::ParaFormat::default()));
            let end = (r.end + 1).min(story.text.len());
            story.format_chars(r.start..end, |fmt| scale_char(&para_chars, fmt));
        }
    }
    for t in story.tables.values_mut() {
        let table = std::sync::Arc::make_mut(t);
        for cell in &mut table.cells {
            scale_story_fonts(&mut cell.text, styles, scale, limits, depth + 1);
        }
    }
    for n in story.notes.iter_mut().chain(story.endnotes.iter_mut()) {
        let note = std::sync::Arc::make_mut(n);
        scale_story_fonts(&mut note.text, styles, scale, limits, depth + 1);
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
pub(super) fn scale_columns(pg: &mut designcraft_doc::Page, width_before: f64) {
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

    #[test]
    fn adjust_layout_command_scales_margins_fonts_and_respects_limits_and_locks() {
        let mut s = Session::new();
        s.execute("file.new", &json!({"pages": 1, "facingPages": false, "width": 612, "height": 792, "margins": 36})).unwrap();
        let tf = s.execute("frame.create", &json!({"rect": [36, 36, 576, 756], "content": "text", "text": "Heading\nBody"})).unwrap();
        let tf_id = designcraft_doc::ItemId(tf["id"].as_u64().unwrap());
        let sid = designcraft_doc::StoryId(tf["story"].as_u64().unwrap());
        s.execute("text.select", &json!({"story": sid.0, "anchor": 0, "focus": 7})).unwrap();
        s.execute("type.char", &json!({"attrs": {"size": 24.0, "leading": {"kind": "points", "value": 30.0}}})).unwrap();
        s.execute("text.select", &json!({"story": sid.0, "anchor": 8, "focus": 12})).unwrap();
        s.execute("type.char", &json!({"attrs": {"size": 12.0}})).unwrap();

        let locked = s.execute("frame.create", &json!({"rect": [100, 100, 200, 200]})).unwrap();
        let locked_id = designcraft_doc::ItemId(locked["id"].as_u64().unwrap());
        s.execute("object.lock", &json!({"ids": [locked_id.0]})).unwrap();

        s.execute(
            "layout.adjust",
            &json!({
                "width": 1224,
                "height": 1584,
                "autoAdjustMargins": true,
                "adjustFontSize": true,
                "fontSizeLimits": true,
                "minFontSize": 10,
                "maxFontSize": 36,
                "adjustLockedContent": false
            }),
        )
        .unwrap();

        let d = &s.doc().unwrap().doc;
        let pg = d.page(0).unwrap();
        assert_eq!(pg.margins, Margins::uniform(72.0));
        let tb = d.item(tf_id).unwrap().bounds();
        assert!(
            (tb.x0 - 72.0).abs() < 1e-6 && (tb.y0 - 72.0).abs() < 1e-6 && (tb.x1 - 1152.0).abs() < 1e-6 && (tb.y1 - 1512.0).abs() < 1e-6,
            "{tb:?}"
        );
        let st = d.story(sid).unwrap();
        let head = st.char_format_of(0);
        let body = st.char_format_of(8);
        assert_eq!(head.over.size, Some(36.0), "24 * 2 clamped to maxFontSize 36");
        assert_eq!(head.over.leading, Some(designcraft_doc::Leading::Points(45.0)), "30 * (36 / 24)");
        assert_eq!(body.over.size, Some(24.0), "12 * 2 within 10..=36");
        let lb = d.item(locked_id).unwrap().bounds();
        assert!((lb.x0 - 100.0).abs() < 1e-6 && (lb.x1 - 200.0).abs() < 1e-6, "locked frame stays put: {lb:?}");

        s.execute("layout.adjust", &json!({"width": 2448, "height": 3168, "autoAdjustMargins": true, "adjustLockedContent": true})).unwrap();
        let lb2 = s.doc().unwrap().doc.item(locked_id).unwrap().bounds();
        assert!((lb2.x0 - 200.0).abs() < 1e-6 && (lb2.y0 - 200.0).abs() < 1e-6 && (lb2.x1 - 400.0).abs() < 1e-6, "{lb2:?}");

        assert!(s.execute("layout.adjust", &json!({"minFontSize": 24, "maxFontSize": 12})).is_err());
        assert!(s.execute("layout.adjust", &json!({"width": 0})).is_err());
        // Collapsing margins must be rejected without mutating the document.
        assert!(s.execute("layout.adjust", &json!({"width": 306, "margins": 200})).is_err());
        assert_eq!(s.doc().unwrap().doc.settings.page_width, 2448.0);
    }

    #[test]
    fn adjust_layout_avoids_double_scaling_groups_and_facing_spread_items() {
        let mut s = Session::new();
        s.execute("file.new", &json!({"pages": 3, "facingPages": true, "width": 612, "height": 792, "margins": 36})).unwrap();
        // Grouped text frame on page 1 (spread 0): scales font once and keeps transforms unscaled.
        let tf = s.execute("frame.create", &json!({"spread": 0, "rect": [36, 36, 300, 300], "content": "text", "text": "Group"})).unwrap();
        let tf_id = tf["id"].as_u64().unwrap();
        let sid = designcraft_doc::StoryId(tf["story"].as_u64().unwrap());
        s.execute("text.select", &json!({"story": sid.0, "anchor": 0, "focus": 5})).unwrap();
        s.execute("type.char", &json!({"attrs": {"size": 20.0}})).unwrap();
        let bx = s.execute("frame.create", &json!({"spread": 0, "rect": [300, 36, 576, 300]})).unwrap()["id"].as_u64().unwrap();
        let gid = designcraft_doc::ItemId(s.execute("object.group", &json!({"ids": [tf_id, bx]})).unwrap()["id"].as_u64().unwrap());

        // Left-page frame on facing spread 1 whose doubled centre (882) falls in the old right page (648..1188).
        let left_id =
            designcraft_doc::ItemId(s.execute("frame.create", &json!({"spread": 1, "rect": [306, 36, 576, 756]})).unwrap()["id"].as_u64().unwrap());

        s.execute("layout.adjust", &json!({"width": 1224, "height": 1584, "autoAdjustMargins": true, "adjustFontSize": true})).unwrap();

        let d = &s.doc().unwrap().doc;
        let grp = d.item(gid).unwrap();
        assert_eq!(grp.xf, designcraft_geom::Affine::IDENTITY, "group transform stays unscaled");
        assert_eq!(grp.children()[0].xf, designcraft_geom::Affine::IDENTITY, "child text transform stays unscaled");
        let gb = grp.bounds();
        assert!(
            (gb.x0 - 72.0).abs() < 1e-6 && (gb.y0 - 72.0).abs() < 1e-6 && (gb.x1 - 1152.0).abs() < 1e-6 && (gb.y1 - 600.0).abs() < 1e-6,
            "{gb:?}"
        );
        assert_eq!(d.story(sid).unwrap().char_format_of(0).over.size, Some(40.0), "grouped story scales once (20 * 2)");

        let lb = d.item(left_id).unwrap().bounds();
        assert!(
            (lb.x0 - 612.0).abs() < 1e-6 && (lb.y0 - 72.0).abs() < 1e-6 && (lb.x1 - 1152.0).abs() < 1e-6 && (lb.y1 - 1512.0).abs() < 1e-6,
            "left-page frame is transformed only once across a facing spread: {lb:?}"
        );
    }

    #[test]
    fn adjust_layout_skips_locked_group_children_and_shifts_graphic_by_own_origin() {
        let mut s = Session::new();
        s.execute("file.new", &json!({"pages": 1, "facingPages": false, "width": 612, "height": 792, "margins": 36})).unwrap();

        let u_tf = s.execute("frame.create", &json!({"rect": [36, 36, 300, 300], "content": "text", "text": "Free"})).unwrap();
        let u_id = designcraft_doc::ItemId(u_tf["id"].as_u64().unwrap());
        let u_sid = designcraft_doc::StoryId(u_tf["story"].as_u64().unwrap());
        s.execute("text.select", &json!({"story": u_sid.0, "anchor": 0, "focus": 4})).unwrap();
        s.execute("type.char", &json!({"attrs": {"size": 20.0}})).unwrap();

        let l_tf = s.execute("frame.create", &json!({"rect": [36, 350, 300, 500], "content": "text", "text": "Lock"})).unwrap();
        let l_id = designcraft_doc::ItemId(l_tf["id"].as_u64().unwrap());
        let l_sid = designcraft_doc::StoryId(l_tf["story"].as_u64().unwrap());
        s.execute("text.select", &json!({"story": l_sid.0, "anchor": 0, "focus": 4})).unwrap();
        s.execute("type.char", &json!({"attrs": {"size": 20.0}})).unwrap();

        s.execute("object.group", &json!({"ids": [u_id.0, l_id.0]})).unwrap();
        s.edit(|d, _| {
            d.item_mut(l_id).unwrap().locked = true;
            Ok(())
        })
        .unwrap();

        let gf_id = designcraft_doc::ItemId(
            s.execute("frame.create", &json!({"rect": [100, 100, 200, 200], "content": "graphic"})).unwrap()["id"].as_u64().unwrap(),
        );
        s.edit(|d, _| {
            let graphic = designcraft_doc::Graphic {
                layer_options: None,
                asset: designcraft_doc::AssetId(1),
                size: (40.0, 30.0),
                xf: designcraft_geom::Affine::translate((100.0, 100.0)),
                clip: None,
                auto_fit: designcraft_doc::Fitting::None,
                fit_align: 4,
                crop: [0.0; 4],
                wrap: Default::default(),
                fill: designcraft_doc::Fill::none(),
            };
            d.item_mut(gf_id).unwrap().content = designcraft_doc::Content::Graphic(graphic);
            Ok(())
        })
        .unwrap();

        s.execute("layout.adjust", &json!({"width": 1224, "height": 1584, "margins": 36, "adjustFontSize": true, "adjustLockedContent": false}))
            .unwrap();

        let d = &s.doc().unwrap().doc;
        let sx: f64 = 1152.0 / 540.0;
        let sy: f64 = 1512.0 / 720.0;
        let font_scale = (sx * sy).sqrt();
        let ub = d.item(u_id).unwrap().bounds();
        assert!((ub.x0 - 36.0).abs() < 1e-6 && (ub.x1 - (36.0 + sx * 264.0)).abs() < 1e-6, "unlocked child moves: {ub:?}");
        assert!((d.story(u_sid).unwrap().char_format_of(0).over.size.unwrap() - 20.0 * font_scale).abs() < 1e-6);

        let lb = d.item(l_id).unwrap().bounds();
        assert!(
            (lb.x0 - 36.0).abs() < 1e-6 && (lb.y0 - 350.0).abs() < 1e-6 && (lb.x1 - 300.0).abs() < 1e-6 && (lb.y1 - 500.0).abs() < 1e-6,
            "locked child stays put: {lb:?}"
        );
        assert_eq!(d.story(l_sid).unwrap().char_format_of(0).over.size, Some(20.0), "locked child story keeps font size");

        let gf = d.item(gf_id).unwrap();
        let fb = gf.bounds();
        let designcraft_doc::Content::Graphic(g) = &gf.content else { panic!("expected graphic") };
        let origin = g.xf * designcraft_geom::Point::ZERO;
        assert!((origin.x - fb.x0).abs() < 1e-6 && (origin.y - fb.y0).abs() < 1e-6, "graphic origin follows frame ({fb:?}), got {origin:?}");
        assert_eq!(&g.xf.as_coeffs()[..4], &[1.0, 0.0, 0.0, 1.0], "graphic stays unscaled");
    }

    #[test]
    fn adjust_layout_preserves_custom_page_margins_when_size_is_unchanged() {
        let mut s = Session::new();
        s.execute("file.new", &json!({"pages": 1, "facingPages": false, "width": 612, "height": 792, "margins": 36})).unwrap();
        s.execute("layout.pageSize", &json!({"pages": [1], "width": 1224})).unwrap();
        s.execute("layout.marginsAndColumns", &json!({"pages": [0], "margins": {"top": 36, "bottom": 36, "inside": 400, "outside": 100}})).unwrap();

        s.execute("layout.adjust", &json!({"width": 612, "height": 792, "autoAdjustMargins": true})).unwrap();
        assert_eq!(s.doc().unwrap().doc.page(0).unwrap().margins.inside, 400.0);
    }

    #[test]
    fn adjust_layout_retains_right_page_assignment_when_facing_spread_resizes() {
        let mut s = Session::new();
        s.execute("file.new", &json!({"pages": 3, "facingPages": true, "width": 612, "height": 792, "margins": 36})).unwrap();
        let right_id =
            designcraft_doc::ItemId(s.execute("frame.create", &json!({"spread": 1, "rect": [648, 100, 748, 200]})).unwrap()["id"].as_u64().unwrap());

        s.execute("layout.adjust", &json!({"width": 306, "height": 792, "margins": 36})).unwrap();
        let b1 = s.doc().unwrap().doc.item(right_id).unwrap().bounds();
        let sx_down = 234.0 / 540.0;
        assert!(
            (b1.x0 - 342.0).abs() < 1e-6 && (b1.x1 - (342.0 + 100.0 * sx_down)).abs() < 1e-6,
            "right-page frame retains right-page transform when shrunk: {b1:?}"
        );

        s.execute("layout.adjust", &json!({"width": 1224, "height": 792, "margins": 36})).unwrap();
        let b2 = s.doc().unwrap().doc.item(right_id).unwrap().bounds();
        let sx_up = 1152.0 / 540.0;
        assert!(
            (b2.x0 - 1260.0).abs() < 1e-6 && (b2.x1 - (1260.0 + 100.0 * sx_up)).abs() < 1e-6,
            "right-page frame retains right-page transform when widened: {b2:?}"
        );
    }

    #[test]
    fn adjust_layout_skips_font_scaling_for_story_shared_with_locked_frame() {
        let mut s = Session::new();
        s.execute("file.new", &json!({"pages": 1, "facingPages": false, "width": 612, "height": 792, "margins": 36})).unwrap();

        let a = s.execute("frame.create", &json!({"rect": [36, 36, 300, 200], "content": "text", "text": "Shared"})).unwrap();
        let b = s.execute("frame.create", &json!({"rect": [36, 300, 300, 500], "content": "text", "text": ""})).unwrap();
        let (a_id, b_id) = (designcraft_doc::ItemId(a["id"].as_u64().unwrap()), designcraft_doc::ItemId(b["id"].as_u64().unwrap()));
        let sid = designcraft_doc::StoryId(a["story"].as_u64().unwrap());
        s.execute("text.select", &json!({"story": sid.0, "anchor": 0, "focus": 6})).unwrap();
        s.execute("type.char", &json!({"attrs": {"size": 20.0}})).unwrap();
        s.execute("text.thread", &json!({"from": a_id.0, "to": b_id.0})).unwrap();

        let dummy = s.execute("frame.create", &json!({"rect": [400, 300, 500, 400]})).unwrap();
        s.execute("object.group", &json!({"ids": [b_id.0, dummy["id"].as_u64().unwrap()]})).unwrap();
        s.edit(|d, _| {
            d.item_mut(b_id).unwrap().locked = true;
            Ok(())
        })
        .unwrap();

        s.execute("layout.adjust", &json!({"width": 1224, "height": 1584, "margins": 36, "adjustFontSize": true, "adjustLockedContent": false}))
            .unwrap();

        let d = &s.doc().unwrap().doc;
        assert!(d.item(a_id).unwrap().bounds().x1 > 300.0, "unlocked frame still resizes");
        assert_eq!(d.item(b_id).unwrap().bounds().x1, 300.0, "locked child frame stays put");
        assert_eq!(d.story(sid).unwrap().char_format_of(0).over.size, Some(20.0), "shared story keeps its font size");
    }

    #[test]
    fn adjust_layout_preserves_valid_asymmetric_margins_at_scale_one() {
        let mut s = Session::new();
        s.execute("file.new", &json!({"pages": 1, "facingPages": false, "width": 612, "height": 792, "margins": 36})).unwrap();
        s.execute("layout.marginsAndColumns", &json!({"pages": [0], "margins": {"top": 36, "bottom": 36, "inside": 400, "outside": 100}})).unwrap();

        s.execute("layout.adjust", &json!({"width": 612, "height": 792, "autoAdjustMargins": true})).unwrap();

        let m = s.doc().unwrap().doc.page(0).unwrap().margins;
        assert_eq!((m.top, m.bottom, m.inside, m.outside), (36.0, 36.0, 400.0, 100.0));
    }

    #[test]
    fn adjust_layout_scales_each_page_margins_by_its_own_ratio() {
        let mut s = Session::new();
        s.execute("file.new", &json!({"pages": 2, "facingPages": false, "width": 612, "height": 792, "margins": 36})).unwrap();
        s.execute("layout.pageSize", &json!({"pages": [1], "width": 1224})).unwrap();
        s.execute("layout.marginsAndColumns", &json!({"pages": [0], "margins": 72})).unwrap();

        s.execute("layout.adjust", &json!({"width": 1224, "height": 792, "autoAdjustMargins": true})).unwrap();

        let d = &s.doc().unwrap().doc;
        let m0 = d.page(0).unwrap().margins;
        let m1 = d.page(1).unwrap().margins;
        assert_eq!((m0.top, m0.bottom, m0.inside, m0.outside), (72.0, 72.0, 72.0, 72.0));
        assert_eq!((m1.top, m1.bottom, m1.inside, m1.outside), (36.0, 36.0, 72.0, 72.0));
    }
}
