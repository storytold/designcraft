use std::sync::Arc;

use designcraft_compose::Cache;
use designcraft_doc::build::NewDocument;
use designcraft_doc::{Document, Guide, Item, Margins, PageSide, Selection, Shape, Stroke};
use designcraft_geom::shapes;
use designcraft_geom::{Affine, Unit};

use crate::layout::CanvasLayout;

use super::*;

/// Snapping offers the parent page the canvas shows: a single-sided page uses a two-page parent's first page.
#[test]
fn shown_parent_page_matches_page_side() {
    let two = Document::new(&NewDocument { facing_pages: true, ..Default::default() });
    assert_eq!(two.parents[0].pages.len(), 2);
    assert_eq!(two.parents[0].parent_page_for_side(PageSide::Single), Some(0));
    assert_eq!(two.parents[0].parent_page_for_side(PageSide::Left), Some(0));
    assert_eq!(two.parents[0].parent_page_for_side(PageSide::Right), Some(1));
    let mut one = Document::new(&NewDocument { facing_pages: false, ..Default::default() });
    assert_eq!(one.parents[0].pages.len(), 1);
    assert_eq!(one.parents[0].parent_page_for_side(PageSide::Single), Some(0));
    Arc::make_mut(&mut one.parents[0]).pages.clear();
    assert_eq!(one.parents[0].parent_page_for_side(PageSide::Single), None);
}

// The factory flags are const. Pin them anyway.
#[test]
#[allow(clippy::assertions_on_constants)]
fn factory_zone_is_four_pixels() {
    assert!(SnapView::FACTORY.snap_to_guides);
    assert!(!SnapView::FACTORY.snap_to_document_grid);
    assert!(SnapView::FACTORY.smart_guides);
    assert_eq!(SnapView::FACTORY.zone_px, 4.0);
    assert!(!SnapView::OFF.any());
    assert!(SnapView::FACTORY.any());
}

fn doc_with_items(moving: Rect, other: Rect, guide_x: f64) -> (Document, ItemId, ItemId) {
    let mut doc = Document::new(&NewDocument::default());
    let layer = doc.default_layer();
    let make = |doc: &mut Document, rect: Rect| {
        let id = ItemId(doc.alloc());
        let item = Item::new(id, layer, Shape::Rectangle, shapes::rectangle(rect));
        doc.insert_item(SpreadRef::Doc(0), item, None).unwrap();
        id
    };
    let moving_id = make(&mut doc, moving);
    let other_id = make(&mut doc, other);
    let page = &mut Arc::make_mut(&mut doc.spreads[0]).pages[0];
    page.guides.push(Guide { orientation: Orientation::Vertical, position: guide_x, spread: false, locked: false, layer: None, liquid: false });
    (doc, moving_id, other_id)
}

fn ctx_on<'a>(doc: &'a Document, sel: &'a Selection, cache: &'a Cache, layout: &'a CanvasLayout) -> ToolContext<'a> {
    ToolContext {
        doc,
        selection: sel,
        cache,
        layout,
        zoom: 1.0,
        layer: doc.default_layer(),
        snap: SnapView::FACTORY,
        unit: Unit::Points,
        stroke_in_front: false,
        loaded: None,
    }
}

fn y_req(rect: Rect) -> SnapRequest<'static> {
    SnapRequest {
        spread: SpreadRef::Doc(0),
        gesture: Gesture::Move,
        rect,
        x_edges: [false, false, false],
        y_edges: [true, true, true],
        exclude: &[],
        exclude_guides: &[],
        copying: false,
        lengths: [None, None],
        angle: None,
        radius: 0.0,
        pointer: Point::new(0.0, 0.0),
    }
}

fn move_req<'a>(rect: Rect, exclude: &'a [ItemId]) -> SnapRequest<'a> {
    SnapRequest {
        spread: SpreadRef::Doc(0),
        gesture: Gesture::Move,
        rect,
        x_edges: [true, true, true],
        y_edges: [false, false, false],
        exclude,
        exclude_guides: &[],
        copying: false,
        lengths: [None, None],
        angle: None,
        radius: 0.0,
        pointer: Point::new(0.0, 0.0),
    }
}

/// The bleed guide attracts like a ruler guide: a top edge 2 pt below the top bleed line
/// snaps up to it; with no bleed there is nothing there to snap to.
#[test]
fn bleed_guide_is_a_snap_target() {
    let far = Rect::new(300.0, 300.0, 320.0, 320.0);
    for (bleed, expected) in [([9.0, 9.0, 9.0, 9.0], -2.0), ([0.0; 4], 0.0)] {
        let (mut doc, moving, _other) = doc_with_items(Rect::new(80.0, 100.0, 100.0, 140.0), far, 500.0);
        doc.settings.bleed = bleed;
        let top = doc.spreads[0].bounds().y0 - 9.0;
        let rect = Rect::new(80.0, top + 2.0, 100.0, top + 40.0);
        let cache = Cache::new();
        let layout = CanvasLayout::new(&doc, false);
        let sel = Selection::default();
        let mut cx = ctx_on(&doc, &sel, &cache, &layout);
        cx.snap.zone_px = 4.0;
        let hit = snap(
            &cx,
            SnapRequest {
                spread: SpreadRef::Doc(0),
                gesture: Gesture::Move,
                rect,
                x_edges: [false, false, false],
                y_edges: [true, true, true],
                exclude: &[moving],
                exclude_guides: &[],
                copying: false,
                lengths: [None, None],
                angle: None,
                radius: 0.0,
                pointer: Point::new(0.0, 0.0),
            },
        );
        assert!((hit.delta.y - expected).abs() < 1e-6, "bleed {bleed:?}: {:?}", hit.delta);
    }
}

#[test]
fn guide_beats_a_closer_object_edge() {
    // Right edge at 100.5. Object edge at 100 (0.5 away). Guide at 104 (3.5 away). Zone is 4.
    let (doc, moving, _other) = doc_with_items(Rect::new(80.0, 100.0, 100.5, 140.0), Rect::new(90.0, 100.0, 100.0, 160.0), 104.0);
    let cache = Cache::new();
    let layout = CanvasLayout::new(&doc, false);
    let sel = Selection::default();
    let mut cx = ctx_on(&doc, &sel, &cache, &layout);
    cx.snap.zone_px = 4.0;
    let hit = snap(
        &cx,
        SnapRequest {
            spread: SpreadRef::Doc(0),
            gesture: Gesture::Move,
            rect: Rect::new(80.0, 100.0, 100.5, 140.0),
            x_edges: [true, true, true],
            y_edges: [false, false, false],
            exclude: &[moving],
            exclude_guides: &[],
            copying: false,
            lengths: [None, None],
            angle: None,
            radius: 0.0,
            pointer: Point::new(0.0, 0.0),
        },
    );
    assert!((hit.delta.x - 3.5).abs() < 1e-6);
    assert!(hit.guides.is_empty());
}

/// A dragged guide is not its own target: excluded, the guide at 104 doesn't pull a line at 102.
/// Nor does the baseline grid pull it.
#[test]
fn excluded_guide_is_not_a_target() {
    let far = Rect::new(400.0, 400.0, 420.0, 420.0);
    let (doc, _, _) = doc_with_items(far, far, 104.0);
    let cache = Cache::new();
    let layout = CanvasLayout::new(&doc, false);
    let sel = Selection::default();
    let cx = ctx_on(&doc, &sel, &cache, &layout);
    let line = Rect::new(102.0, 100.0, 102.0, 300.0);
    let dragged = [designcraft_doc::GuideSel { spread: SpreadRef::Doc(0), page: 0, index: 0 }];
    let hit = snap(&cx, move_req(line, &[]));
    assert!((hit.delta.x - 2.0).abs() < 1e-6, "{:?}", hit.delta);
    let hit = snap(&cx, SnapRequest { exclude_guides: &dragged, ..move_req(line, &[]) });
    assert_eq!(hit.delta.x, 0.0);
    // A dragged guide passes the baseline grid's lines (36 + 12n), which a moved object meets.
    let flat = Rect::new(100.0, 86.0, 300.0, 86.0);
    assert!((snap(&cx, y_req(flat)).delta.y - -2.0).abs() < 1e-6);
    assert_eq!(snap(&cx, SnapRequest { gesture: Gesture::Guide, ..y_req(flat) }).delta.y, 0.0);
}

#[test]
fn parent_centered_stroke_attracts_at_the_outer_edge() {
    let mut doc = Document::new(&NewDocument::default());
    let pid = doc.add_parent("A", "A-Parent", 1, 12.0, Margins::uniform(36.0));
    doc.apply_parent(&[0], Some(pid)).unwrap();
    let layer = doc.default_layer();
    let id = ItemId(doc.alloc());
    let (ppi, ppg) = doc.parent_page_for(0).unwrap();
    let parent_x = doc.parents[ppi].pages[ppg].x;
    let mut item = Item::new(id, layer, Shape::Rectangle, shapes::rectangle(Rect::new(parent_x + 72.0, 140.0, parent_x + 252.0, 300.0)));
    item.stroke = Stroke::default(); // 1 pt, center
    doc.insert_item(SpreadRef::Parent(ppi), item, None).unwrap();
    let cache = Cache::new();
    let layout = CanvasLayout::new(&doc, false);
    let sel = Selection::default();
    let mut cx = ctx_on(&doc, &sel, &cache, &layout);
    cx.snap.snap_to_guides = false;
    // Moving left edge at 253. Visible parent right edge is 252.5.
    let hit = snap(
        &cx,
        SnapRequest {
            spread: SpreadRef::Doc(0),
            gesture: Gesture::Move,
            rect: Rect::new(253.0, 140.0, 353.0, 240.0),
            x_edges: [true, false, false],
            y_edges: [false, false, false],
            exclude: &[],
            exclude_guides: &[],
            copying: false,
            lengths: [None, None],
            angle: None,
            radius: 0.0,
            pointer: Point::new(0.0, 0.0),
        },
    );
    assert!((hit.delta.x - -0.5).abs() < 1e-6, "delta {}", hit.delta.x);
}

#[test]
fn object_edge_snaps_when_guide_snap_is_off() {
    let (doc, moving, _other) = doc_with_items(Rect::new(80.0, 100.0, 100.5, 140.0), Rect::new(90.0, 100.0, 100.0, 160.0), 104.0);
    let cache = Cache::new();
    let layout = CanvasLayout::new(&doc, false);
    let sel = Selection::default();
    let mut cx = ctx_on(&doc, &sel, &cache, &layout);
    cx.snap.snap_to_guides = false;
    let hit = snap(
        &cx,
        SnapRequest {
            spread: SpreadRef::Doc(0),
            gesture: Gesture::Move,
            rect: Rect::new(80.0, 100.0, 100.5, 140.0),
            x_edges: [true, true, true],
            y_edges: [false, false, false],
            exclude: &[moving],
            exclude_guides: &[],
            copying: false,
            lengths: [None, None],
            angle: None,
            radius: 0.0,
            pointer: Point::new(0.0, 0.0),
        },
    );
    assert!((hit.delta.x - -0.5).abs() < 1e-6);
    assert!(matches!(hit.guides.first(), Some(Overlay::Guide { .. })));
}

#[test]
fn hidden_layer_guide_is_not_a_target() {
    let (mut doc, moving, _) = doc_with_items(Rect::new(80.0, 100.0, 102.0, 140.0), Rect::new(300.0, 100.0, 340.0, 140.0), 104.0);
    let layer = doc.default_layer();
    doc.layers.iter_mut().find(|l| l.id == layer).unwrap().visible = false;
    Arc::make_mut(&mut doc.spreads[0]).pages[0].guides[0].layer = Some(layer);
    let cache = Cache::new();
    let layout = CanvasLayout::new(&doc, false);
    let sel = Selection::default();
    let cx = ctx_on(&doc, &sel, &cache, &layout);
    let hit = snap(
        &cx,
        SnapRequest {
            spread: SpreadRef::Doc(0),
            gesture: Gesture::Move,
            rect: Rect::new(80.0, 100.0, 102.0, 140.0),
            x_edges: [false, false, true],
            y_edges: [false, false, false],
            exclude: &[moving],
            exclude_guides: &[],
            copying: false,
            lengths: [None, None],
            angle: None,
            radius: 0.0,
            pointer: Point::new(0.0, 0.0),
        },
    );
    assert_eq!(hit.delta.x, 0.0);
}

#[test]
fn alt_copy_snaps_to_the_original() {
    let mut doc = Document::new(&NewDocument::default());
    let layer = doc.default_layer();
    let id = ItemId(doc.alloc());
    let item = Item::new(id, layer, Shape::Rectangle, shapes::rectangle(Rect::new(100.0, 40.0, 160.0, 80.0)));
    doc.insert_item(SpreadRef::Doc(0), item, None).unwrap();
    let cache = Cache::new();
    let layout = CanvasLayout::new(&doc, false);
    let sel = Selection::default();
    let mut cx = ctx_on(&doc, &sel, &cache, &layout);
    cx.snap.snap_to_guides = false;
    let hit = snap(
        &cx,
        SnapRequest {
            spread: SpreadRef::Doc(0),
            gesture: Gesture::Move,
            rect: Rect::new(162.0, 40.0, 222.0, 80.0),
            x_edges: [true, false, false],
            y_edges: [false, false, false],
            exclude: &[id],
            exclude_guides: &[],
            copying: true,
            lengths: [None, None],
            angle: None,
            radius: 0.0,
            pointer: Point::new(0.0, 0.0),
        },
    );
    assert!((hit.delta.x - -2.0).abs() < 1e-6);
}

#[test]
fn alt_copy_does_not_stick_to_the_preview() {
    let mut doc = Document::new(&NewDocument::default());
    let layer = doc.default_layer();
    let make = |doc: &mut Document, rect: Rect| {
        let id = ItemId(doc.alloc());
        let item = Item::new(id, layer, Shape::Rectangle, shapes::rectangle(rect));
        doc.insert_item(SpreadRef::Doc(0), item, None).unwrap();
        id
    };
    let original = make(&mut doc, Rect::new(100.0, 40.0, 160.0, 80.0));
    let ghost = make(&mut doc, Rect::new(161.0, 40.0, 221.0, 80.0));
    let cache = Cache::new();
    let layout = CanvasLayout::new(&doc, false);
    let sel = Selection::items(vec![ghost]);
    let mut cx = ctx_on(&doc, &sel, &cache, &layout);
    cx.snap.snap_to_guides = false;
    let hit = snap(
        &cx,
        SnapRequest {
            spread: SpreadRef::Doc(0),
            gesture: Gesture::Move,
            rect: Rect::new(161.0, 40.0, 221.0, 80.0),
            x_edges: [true, false, false],
            y_edges: [false, false, false],
            exclude: &[original],
            exclude_guides: &[],
            copying: true,
            lengths: [None, None],
            angle: None,
            radius: 0.0,
            pointer: Point::new(0.0, 0.0),
        },
    );
    assert!((hit.delta.x - -1.0).abs() < 1e-6, "delta.x = {}", hit.delta.x);
}

#[test]
fn parent_page_guides_snap_on_the_document_page() {
    // Facing document: page 1 is a right page at x = 0; its parent page is A's right page at
    // x = 612 in the parent spread.
    let mut doc = Document::new(&NewDocument::default());
    let layer = doc.default_layer();
    let id = ItemId(doc.alloc());
    let item = Item::new(id, layer, Shape::Rectangle, shapes::rectangle(Rect::new(80.0, 100.0, 100.5, 140.0)));
    doc.insert_item(SpreadRef::Doc(0), item, None).unwrap();
    Arc::make_mut(&mut doc.parents[0]).pages[1].guides.push(Guide {
        orientation: Orientation::Vertical,
        position: 612.0 + 104.0,
        spread: false,
        locked: false,
        layer: None,
        liquid: false,
    });
    let cache = Cache::new();
    let layout = CanvasLayout::new(&doc, false);
    let sel = Selection::default();
    let mut cx = ctx_on(&doc, &sel, &cache, &layout);
    cx.snap.zone_px = 4.0;
    let hit = snap(&cx, move_req(Rect::new(80.0, 100.0, 100.5, 140.0), &[id]));
    assert!((hit.delta.x - 3.5).abs() < 1e-6, "snaps to the parent guide at x = 104: {:?}", hit.delta);
}

#[test]
fn grid_beats_a_closer_guide() {
    let mut doc = Document::new(&NewDocument::default());
    doc.settings.grid.horizontal = 72.0;
    doc.settings.grid.vertical = 72.0;
    doc.settings.grid.subdivisions = 8; // 9 pt
    let layer = doc.default_layer();
    let id = ItemId(doc.alloc());
    let item = Item::new(id, layer, Shape::Rectangle, shapes::rectangle(Rect::new(10.0, 10.0, 40.0, 40.0)));
    doc.insert_item(SpreadRef::Doc(0), item, None).unwrap();
    // Guide 1 pt away at x = 11. Subdivision line at 9 is 1 pt the other way. Grid must win.
    Arc::make_mut(&mut doc.spreads[0]).pages[0].guides.push(Guide {
        orientation: Orientation::Vertical,
        position: 11.0,
        spread: true,
        locked: false,
        layer: None,
        liquid: false,
    });
    let cache = Cache::new();
    let layout = CanvasLayout::new(&doc, false);
    let sel = Selection::default();
    let mut cx = ctx_on(&doc, &sel, &cache, &layout);
    cx.snap.snap_to_document_grid = true;
    let hit = snap(&cx, move_req(Rect::new(10.0, 10.0, 40.0, 40.0), &[id]));
    assert!((hit.delta.x - -1.0).abs() < 1e-6);
    assert!(hit.guides.is_empty());
}

#[test]
fn hidden_baseline_still_snaps_and_zero_increment_does_not() {
    let mut doc = Document::new(&NewDocument::default());
    doc.settings.baseline_grid.start = 36.0;
    doc.settings.baseline_grid.increment = 12.0;
    let cache = Cache::new();
    let layout = CanvasLayout::new(&doc, false);
    let sel = Selection::default();
    let cx = ctx_on(&doc, &sel, &cache, &layout);
    // Top at 49 wants the line at 48 (start 36, increment 12). The margin at 36 is 13 pt away.
    let hit = snap(&cx, y_req(Rect::new(100.0, 49.0, 140.0, 80.0)));
    assert!((hit.delta.y - -1.0).abs() < 1e-6);
    let mut dead = doc.clone();
    dead.settings.baseline_grid.increment = 0.0;
    let layout = CanvasLayout::new(&dead, false);
    let cx = ctx_on(&dead, &sel, &cache, &layout);
    let hit = snap(&cx, y_req(Rect::new(100.0, 49.0, 140.0, 80.0)));
    assert_eq!(hit.delta.y, 0.0);
}

#[test]
fn baseline_relative_to_the_top_margin_snaps_from_the_margin() {
    let mut doc = Document::new(&NewDocument::default());
    std::sync::Arc::make_mut(&mut doc.spreads[0]).pages[0].margins.top = 37.0;
    doc.settings.baseline_grid.start = 0.0;
    doc.settings.baseline_grid.increment = 10.0;
    doc.settings.baseline_grid.relative_to = designcraft_doc::GridRelative::TopMargin;
    let cache = Cache::new();
    let layout = CanvasLayout::new(&doc, false);
    let sel = Selection::default();
    let cx = ctx_on(&doc, &sel, &cache, &layout);
    // Lines at 37 + 10n: a top at 58 goes up to 57, not down to the page-relative 60.
    let hit = snap(&cx, y_req(Rect::new(100.0, 58.0, 140.0, 80.0)));
    assert!((hit.delta.y - -1.0).abs() < 1e-6, "{}", hit.delta.y);
}

#[test]
fn nearer_of_margin_and_baseline_wins() {
    // Bottom margin at 792 - 54 = 738, between baseline lines 732 and 744.
    let doc = Document::new(&NewDocument {
        margins: designcraft_doc::Margins { top: 54.0, bottom: 54.0, inside: 54.0, outside: 42.0 },
        ..Default::default()
    });
    let cache = Cache::new();
    let layout = CanvasLayout::new(&doc, false);
    let sel = Selection::default();
    // At 50% the 4 px zone is 8 pt: both the margin and a baseline line are in reach.
    let cx = ToolContext { zoom: 0.5, ..ctx_on(&doc, &sel, &cache, &layout) };
    let bottom = |y1: f64| SnapRequest { y_edges: [false, false, true], gesture: Gesture::Resize, ..y_req(Rect::new(100.0, 600.0, 140.0, y1)) };
    assert!((snap(&cx, bottom(739.5)).delta.y - -1.5).abs() < 1e-6, "the margin is nearer");
    assert!((snap(&cx, bottom(742.5)).delta.y - 1.5).abs() < 1e-6, "the baseline line is nearer");
    assert!((snap(&cx, bottom(741.0)).delta.y - -3.0).abs() < 1e-6, "a tie goes to the margin");
}

#[test]
fn rotated_item_matches_its_own_side() {
    let mut doc = Document::new(&NewDocument::default());
    let layer = doc.default_layer();
    let id = ItemId(doc.alloc());
    let mut item = Item::new(id, layer, Shape::Rectangle, shapes::rectangle(Rect::new(0.0, 0.0, 200.0, 40.0)));
    item.xf = Affine::rotate((-30.0_f64).to_radians());
    doc.insert_item(SpreadRef::Doc(0), item, None).unwrap();
    let cache = Cache::new();
    let layout = CanvasLayout::new(&doc, false);
    let sel = Selection::default();
    let mut cx = ctx_on(&doc, &sel, &cache, &layout);
    cx.snap.snap_to_guides = false;
    cx.snap.snap_to_document_grid = false;
    // Width 193.2 is the axis-aligned box. 200 is the side. Zone 4 reaches 200 from 197, not from 193.
    let hit = snap(
        &cx,
        SnapRequest {
            spread: SpreadRef::Doc(0),
            gesture: Gesture::Resize,
            rect: Rect::new(80.0, 200.0, 277.0, 280.0),
            x_edges: [false, false, true],
            y_edges: [false, false, false],
            exclude: &[],
            exclude_guides: &[],
            copying: false,
            lengths: [Some(197.0), None],
            angle: None,
            radius: 0.0,
            pointer: Point::new(277.0, 240.0),
        },
    );
    assert!((hit.delta.x - 3.0).abs() < 1e-6);
    assert!(matches!(hit.guides.last(), Some(Overlay::Measure { text, .. }) if text.contains("200")));
}

#[test]
fn grouped_child_matches_spread_side() {
    // Group scale 2 on X and 1 on Y. Child rotated -90 degrees (decompose reports +90).
    // Spread-space sides are local width times 1 (100) and local height times 2 (80).
    // 82 is 2 pt from 80 and far from 200, which is what per-node scales would offer.
    let mut doc = Document::new(&NewDocument::default());
    let layer = doc.default_layer();
    let child_id = ItemId(doc.alloc());
    let mut child = Item::new(child_id, layer, Shape::Rectangle, shapes::rectangle(Rect::new(0.0, 0.0, 100.0, 40.0)));
    child.xf = Affine::rotate((-90.0_f64).to_radians());
    let group_id = ItemId(doc.alloc());
    let mut group = Item::new(group_id, layer, Shape::Group, designcraft_geom::PathData::default());
    group.xf = Affine::scale_non_uniform(2.0, 1.0);
    group.content = designcraft_doc::Content::Group { items: vec![std::sync::Arc::new(child)] };
    doc.insert_item(SpreadRef::Doc(0), group, None).unwrap();
    let cache = Cache::new();
    let layout = CanvasLayout::new(&doc, false);
    let sel = Selection::default();
    let mut cx = ctx_on(&doc, &sel, &cache, &layout);
    cx.snap.snap_to_guides = false;
    cx.snap.snap_to_document_grid = false;
    cx.snap.align_edges = false;
    cx.snap.align_centers = false;
    let hit = snap(
        &cx,
        SnapRequest {
            spread: SpreadRef::Doc(0),
            gesture: Gesture::Resize,
            rect: Rect::new(200.0, 200.0, 282.0, 240.0),
            x_edges: [false, false, true],
            y_edges: [false, false, false],
            exclude: &[],
            exclude_guides: &[],
            copying: false,
            lengths: [Some(82.0), None],
            angle: None,
            radius: 0.0,
            pointer: Point::new(282.0, 220.0),
        },
    );
    assert!((hit.delta.x - -2.0).abs() < 1e-6, "delta.x = {}, want -2 toward local height times 2", hit.delta.x);
}

#[test]
fn move_matches_a_stationary_gap() {
    let mut doc = Document::new(&NewDocument::default());
    let layer = doc.default_layer();
    // Stationary gap of 20 between [0,40] and [60,90]. Moving box at [112,150], 22 away from 90.
    for r in [Rect::new(0.0, 0.0, 40.0, 30.0), Rect::new(60.0, 0.0, 90.0, 30.0), Rect::new(112.0, 0.0, 150.0, 30.0)] {
        let id = ItemId(doc.alloc());
        let item = Item::new(id, layer, Shape::Rectangle, shapes::rectangle(r));
        doc.insert_item(SpreadRef::Doc(0), item, None).unwrap();
    }
    let moving = doc.spreads[0].items[2].id;
    let cache = Cache::new();
    let layout = CanvasLayout::new(&doc, false);
    let sel = Selection::default();
    let mut cx = ctx_on(&doc, &sel, &cache, &layout);
    cx.snap.snap_to_guides = false;
    let hit = snap(
        &cx,
        SnapRequest {
            spread: SpreadRef::Doc(0),
            gesture: Gesture::Move,
            rect: Rect::new(112.0, 0.0, 150.0, 30.0),
            x_edges: [true, true, true],
            y_edges: [false, false, false],
            exclude: &[moving],
            exclude_guides: &[],
            copying: false,
            lengths: [None, None],
            angle: None,
            radius: 0.0,
            pointer: Point::new(0.0, 0.0),
        },
    );
    assert!((hit.delta.x - -2.0).abs() < 1e-6);
    assert!(hit.guides.iter().any(|g| matches!(g, Overlay::Gap { .. })));
}

fn doc_with_rects(rects: &[Rect]) -> Document {
    let mut doc = Document::new(&NewDocument::default());
    let layer = doc.default_layer();
    for &r in rects {
        let id = ItemId(doc.alloc());
        let item = Item::new(id, layer, Shape::Rectangle, shapes::rectangle(r));
        doc.insert_item(SpreadRef::Doc(0), item, None).unwrap();
    }
    doc
}

fn gap_overlays(guides: &[Overlay]) -> usize {
    guides.iter().filter(|g| matches!(g, Overlay::Gap { .. })).count()
}

// #188: a box dragged between two others snaps to the middle and marks both gaps.
#[test]
fn move_centers_between_two_boxes_side_by_side() {
    // Neighbours end at 40 and start at 160. The 30 wide mover at [83,113] is 2 left of centre.
    let doc = doc_with_rects(&[Rect::new(0.0, 0.0, 40.0, 30.0), Rect::new(160.0, 0.0, 200.0, 30.0)]);
    let cache = Cache::new();
    let layout = CanvasLayout::new(&doc, false);
    let sel = Selection::default();
    let mut cx = ctx_on(&doc, &sel, &cache, &layout);
    cx.snap.snap_to_guides = false;
    let hit = snap(&cx, move_req(Rect::new(83.0, 5.0, 113.0, 25.0), &[]));
    assert!((hit.delta.x - 2.0).abs() < 1e-6, "{}", hit.delta.x);
    assert_eq!(gap_overlays(&hit.guides), 2);
}

#[test]
fn move_centers_between_two_boxes_stacked() {
    let doc = doc_with_rects(&[Rect::new(0.0, 0.0, 30.0, 40.0), Rect::new(0.0, 160.0, 30.0, 200.0)]);
    let cache = Cache::new();
    let layout = CanvasLayout::new(&doc, false);
    let sel = Selection::default();
    let mut cx = ctx_on(&doc, &sel, &cache, &layout);
    cx.snap.snap_to_guides = false;
    let hit = snap(&cx, y_req(Rect::new(5.0, 83.0, 25.0, 113.0)));
    assert!((hit.delta.y - 2.0).abs() < 1e-6, "{}", hit.delta.y);
    assert_eq!(gap_overlays(&hit.guides), 2);
}

fn with_baseline_grid(mut doc: Document) -> Document {
    doc.settings.baseline_grid.start = 36.0;
    doc.settings.baseline_grid.increment = 12.0;
    doc
}

// #188: with Snap to Guides on, the hidden baseline grid took vertical moves first.
#[test]
fn vertical_centering_beats_a_closer_baseline() {
    // Middle at 200. Mover centre 198 (+2). Its top and bottom are 3 from lines 180 and 216.
    let doc = with_baseline_grid(doc_with_rects(&[Rect::new(100.0, 100.0, 130.0, 140.0), Rect::new(100.0, 260.0, 130.0, 300.0)]));
    let cache = Cache::new();
    let layout = CanvasLayout::new(&doc, false);
    let sel = Selection::default();
    let cx = ctx_on(&doc, &sel, &cache, &layout);
    let hit = snap(&cx, y_req(Rect::new(105.0, 183.0, 125.0, 213.0)));
    assert!((hit.delta.y - 2.0).abs() < 1e-6, "{}", hit.delta.y);
    assert_eq!(gap_overlays(&hit.guides), 2);
}

#[test]
fn vertical_alignment_beats_a_closer_baseline() {
    // Top 107 is 1 from the line at 108 and 2 from the other box's top at 105.
    let doc = with_baseline_grid(doc_with_rects(&[Rect::new(300.0, 105.0, 340.0, 135.0)]));
    let cache = Cache::new();
    let layout = CanvasLayout::new(&doc, false);
    let sel = Selection::default();
    let cx = ctx_on(&doc, &sel, &cache, &layout);
    let hit = snap(&cx, y_req(Rect::new(100.0, 107.0, 140.0, 137.0)));
    assert!((hit.delta.y - -2.0).abs() < 1e-6, "{}", hit.delta.y);
    assert!(hit.guides.iter().any(|g| matches!(g, Overlay::Guide { .. })));
}

#[test]
fn move_off_center_outside_the_zone_does_not_center() {
    // 10 off centre, zone 4.
    let doc = doc_with_rects(&[Rect::new(0.0, 0.0, 40.0, 30.0), Rect::new(160.0, 0.0, 200.0, 30.0)]);
    let cache = Cache::new();
    let layout = CanvasLayout::new(&doc, false);
    let sel = Selection::default();
    let mut cx = ctx_on(&doc, &sel, &cache, &layout);
    cx.snap.snap_to_guides = false;
    let hit = snap(&cx, move_req(Rect::new(95.0, 5.0, 125.0, 25.0), &[]));
    assert_eq!(hit.delta.x, 0.0);
    assert_eq!(gap_overlays(&hit.guides), 0);
}

#[test]
fn centering_needs_smart_spacing() {
    let doc = doc_with_rects(&[Rect::new(0.0, 0.0, 40.0, 30.0), Rect::new(160.0, 0.0, 200.0, 30.0)]);
    let cache = Cache::new();
    let layout = CanvasLayout::new(&doc, false);
    let sel = Selection::default();
    let mut cx = ctx_on(&doc, &sel, &cache, &layout);
    cx.snap.snap_to_guides = false;
    cx.snap.smart_spacing = false;
    let hit = snap(&cx, move_req(Rect::new(83.0, 5.0, 113.0, 25.0), &[]));
    assert_eq!(hit.delta.x, 0.0);
}

#[test]
fn create_does_not_match_spacing() {
    let mut doc = Document::new(&NewDocument::default());
    let layer = doc.default_layer();
    for r in [Rect::new(0.0, 0.0, 40.0, 30.0), Rect::new(60.0, 0.0, 90.0, 30.0)] {
        let id = ItemId(doc.alloc());
        let item = Item::new(id, layer, Shape::Rectangle, shapes::rectangle(r));
        doc.insert_item(SpreadRef::Doc(0), item, None).unwrap();
    }
    let cache = Cache::new();
    let layout = CanvasLayout::new(&doc, false);
    let sel = Selection::default();
    let mut cx = ctx_on(&doc, &sel, &cache, &layout);
    cx.snap.snap_to_guides = false;
    cx.snap.smart_dimensions = false;
    let hit = snap(
        &cx,
        SnapRequest {
            spread: SpreadRef::Doc(0),
            gesture: Gesture::Create,
            rect: Rect::new(112.0, 0.0, 150.0, 30.0),
            x_edges: [true, true, true],
            y_edges: [false, false, false],
            exclude: &[],
            exclude_guides: &[],
            copying: false,
            lengths: [Some(38.0), None],
            angle: None,
            radius: 0.0,
            pointer: Point::new(0.0, 0.0),
        },
    );
    assert!((hit.delta.x).abs() < 1e-6);
}

#[test]
fn rotation_matches_another_items_angle() {
    let mut doc = Document::new(&NewDocument::default());
    let layer = doc.default_layer();
    let id = ItemId(doc.alloc());
    let mut item = Item::new(id, layer, Shape::Rectangle, shapes::rectangle(Rect::new(0.0, 0.0, 80.0, 40.0)));
    item.xf = Affine::rotate((-30.0_f64).to_radians());
    doc.insert_item(SpreadRef::Doc(0), item, None).unwrap();
    let cache = Cache::new();
    let layout = CanvasLayout::new(&doc, false);
    let sel = Selection::default();
    let mut cx = ctx_on(&doc, &sel, &cache, &layout);
    cx.snap.snap_to_guides = false;
    let hit = snap(
        &cx,
        SnapRequest {
            spread: SpreadRef::Doc(0),
            gesture: Gesture::Rotate,
            rect: Rect::new(100.0, 100.0, 160.0, 140.0),
            x_edges: [false, false, false],
            y_edges: [false, false, false],
            exclude: &[],
            exclude_guides: &[],
            copying: false,
            lengths: [None, None],
            angle: Some(28.0),
            radius: 80.0,
            pointer: Point::new(180.0, 120.0),
        },
    );
    let angle = hit.angle.expect("rotation hit");
    assert!((angle - 30.0).abs() < 1e-6);
    assert!(matches!(hit.guides.first(), Some(Overlay::Measure { .. })));
}

#[test]
fn parent_item_angle_matches() {
    // Document::new already has A-Parent. Apply a second parent and insert on the
    // page parent_page_for shows, not Parent(0). A facing right page sits near x 612.
    let mut doc = Document::new(&NewDocument::default());
    let pid = doc.add_parent("B", "B-Parent", 1, 12.0, Margins::uniform(36.0));
    doc.apply_parent(&[0], Some(pid)).unwrap();
    std::sync::Arc::make_mut(&mut doc.spreads[0]).pages[0].show_parent_items = true;
    let (ppi, ppg) = doc.parent_page_for(0).unwrap();
    let parent_x = doc.parents[ppi].pages[ppg].x;
    let rect = Rect::new(parent_x + 72.0, 140.0, parent_x + 152.0, 180.0);
    let center = rect.center();
    let layer = doc.default_layer();
    let id = ItemId(doc.alloc());
    let mut item = Item::new(id, layer, Shape::Rectangle, shapes::rectangle(rect));
    item.xf = Affine::translate((center.x, center.y)) * Affine::rotate((-30.0_f64).to_radians()) * Affine::translate((-center.x, -center.y));
    doc.insert_item(SpreadRef::Parent(ppi), item, None).unwrap();
    assert!(doc.spreads[0].items.iter().all(|it| (designcraft_geom::decompose::decompose(it.xf).rotation - 30.0).abs() > 1.0));
    let cache = Cache::new();
    let layout = CanvasLayout::new(&doc, false);
    let sel = Selection::default();
    let mut cx = ctx_on(&doc, &sel, &cache, &layout);
    cx.snap.snap_to_guides = false;
    let hit = snap(
        &cx,
        SnapRequest {
            spread: SpreadRef::Doc(0),
            gesture: Gesture::Rotate,
            rect: Rect::new(100.0, 100.0, 160.0, 140.0),
            x_edges: [false, false, false],
            y_edges: [false, false, false],
            exclude: &[],
            exclude_guides: &[],
            copying: false,
            lengths: [None, None],
            angle: Some(28.0),
            radius: 80.0,
            pointer: Point::new(180.0, 120.0),
        },
    );
    let angle = hit.angle.expect("parent rotation hit");
    assert!((angle - 30.0).abs() < 1e-6);
}

#[test]
fn rotated_group_uses_the_per_child_union() {
    // Two children do not fill their local union. Rotating that union as one rectangle
    // swings the empty corners out. The snap target is each child's visible box, transformed, then united.
    let mut doc = Document::new(&NewDocument::default());
    let layer = doc.default_layer();
    let mut child_a = Item::new(ItemId(doc.alloc()), layer, Shape::Rectangle, shapes::rectangle(Rect::new(0.0, 0.0, 10.0, 10.0)));
    child_a.stroke = Stroke { weight: 2.0, ..Stroke::default() };
    let child_b = Item::new(ItemId(doc.alloc()), layer, Shape::Rectangle, shapes::rectangle(Rect::new(40.0, 30.0, 50.0, 40.0)));
    let mut group = Item::new(ItemId(doc.alloc()), layer, Shape::Group, designcraft_geom::PathData::default());
    let xf = Affine::translate((300.0, 400.0)) * Affine::rotate((-45.0_f64).to_radians());
    group.xf = xf;
    group.content = designcraft_doc::Content::Group { items: vec![Arc::new(child_a), Arc::new(child_b)] };
    doc.insert_item(SpreadRef::Doc(0), group, None).unwrap();
    let a_vis = Rect::new(-1.0, -1.0, 11.0, 11.0);
    let b_vis = Rect::new(40.0, 30.0, 50.0, 40.0);
    let per_child = xf.transform_rect_bbox(a_vis).union(xf.transform_rect_bbox(b_vis));
    let rotated_union = xf.transform_rect_bbox(a_vis.union(b_vis));
    // At 45 degrees the empty corners move y, not x. The two boxes share x0.
    assert!((per_child.y0 - rotated_union.y0).abs() > 1.0, "fixture does not separate the two boxes");
    let gap = 2.0;
    // A short rect just above the per-child top. Its other edges stay outside the zone of the rotated union.
    let moving = Rect::new(per_child.x0, per_child.y0 - gap - 4.0, per_child.x0 + 20.0, per_child.y0 - gap);
    let cache = Cache::new();
    let layout = CanvasLayout::new(&doc, false);
    let sel = Selection::default();
    let mut cx = ctx_on(&doc, &sel, &cache, &layout);
    cx.snap.snap_to_guides = false;
    cx.snap.snap_to_document_grid = false;
    let hit = snap(&cx, y_req(moving));
    assert!((hit.delta.y - gap).abs() < 1e-4, "delta.y {}, want {gap} onto {}", hit.delta.y, per_child.y0);
    let wrong = rotated_union.y0 - moving.y1;
    assert!((hit.delta.y - wrong).abs() > 1.0, "delta matched the rotated local union");
}

#[test]
fn rotated_group_child_does_not_offer_local_zero() {
    // The group is at 30 degrees. Its child stays at local 0, so the absolute angle is 30.
    // 2 degrees is inside the zone of 0 and far from 30. Local 0 must not win.
    let mut doc = Document::new(&NewDocument::default());
    let layer = doc.default_layer();
    let child = Item::new(ItemId(doc.alloc()), layer, Shape::Rectangle, shapes::rectangle(Rect::new(0.0, 0.0, 40.0, 20.0)));
    let mut group = Item::new(ItemId(doc.alloc()), layer, Shape::Group, designcraft_geom::PathData::default());
    group.xf = Affine::rotate((-30.0_f64).to_radians());
    group.content = designcraft_doc::Content::Group { items: vec![Arc::new(child)] };
    doc.insert_item(SpreadRef::Doc(0), group, None).unwrap();
    let cache = Cache::new();
    let layout = CanvasLayout::new(&doc, false);
    let sel = Selection::default();
    let mut cx = ctx_on(&doc, &sel, &cache, &layout);
    cx.snap.snap_to_guides = false;
    let ask = |angle: f64| {
        snap(
            &cx,
            SnapRequest {
                spread: SpreadRef::Doc(0),
                gesture: Gesture::Rotate,
                rect: Rect::new(100.0, 100.0, 160.0, 140.0),
                x_edges: [false, false, false],
                y_edges: [false, false, false],
                exclude: &[],
                exclude_guides: &[],
                copying: false,
                lengths: [None, None],
                angle: Some(angle),
                radius: 80.0,
                pointer: Point::new(180.0, 120.0),
            },
        )
    };
    let near_zero = ask(2.0);
    assert!(near_zero.angle.is_none(), "local 0 was a target: {:?}", near_zero.angle);
    let near_group = ask(28.0);
    let angle = near_group.angle.expect("absolute 30");
    assert!((angle - 30.0).abs() < 1e-6, "angle {angle}");
}
