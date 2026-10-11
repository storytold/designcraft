use std::sync::Arc;

use designcraft_compose::Cache;
use designcraft_doc::build::NewDocument;
use designcraft_doc::{Document, Guide, Item, ItemId, Orientation, ParaFormat, Selection, Shape, SpreadRef, Stroke};
use designcraft_geom::shapes;
use designcraft_geom::{Point, Rect, Unit};

use super::*;

fn ctx<'a>(d: &'a Document, s: &'a Selection, c: &'a Cache, l: &'a CanvasLayout) -> ToolContext<'a> {
    ToolContext { doc: d, selection: s, cache: c, layout: l, zoom: 1.0, layer: d.default_layer(), snap: SnapView::OFF, unit: Unit::Points }
}

#[test]
fn frame_tool_drag_emits_begin_preview_commit() {
    let d = Document::new(&NewDocument::default());
    let (s, c, l) = (Selection::default(), Cache::new(), CanvasLayout::new(&d, false));
    let cx = ctx(&d, &s, &c, &l);
    let mut t = create("rectangleFrame");
    assert!(t.pointer(&cx, &PointerEvent::new(PointerKind::Down, 10.0, 10.0)).is_empty());
    let a = t.pointer(&cx, &PointerEvent::new(PointerKind::Drag, 110.0, 60.0));
    assert!(matches!(&a[0], Action::Begin(_)));
    match &a[1] {
        Action::Preview(cmd, p) => {
            assert_eq!(cmd, "frame.create");
            assert_eq!(p["rect"], serde_json::json!([10.0, 10.0, 110.0, 60.0]));
            assert_eq!(p["content"], "graphic");
        }
        other => panic!("{other:?}"),
    }
    assert_eq!(t.pointer(&cx, &PointerEvent::new(PointerKind::Up, 110.0, 60.0)), vec![Action::Commit]);
}

#[test]
fn selection_click_and_marquee() {
    let mut d = Document::new(&NewDocument::default());
    let lid = d.default_layer();
    let (fid, _) = d.add_text_frame(SpreadRef::Doc(0), Rect::new(100.0, 100.0, 200.0, 200.0), lid, "x", ParaFormat::default()).unwrap();
    let (s, c, l) = (Selection::default(), Cache::new(), CanvasLayout::new(&d, false));
    let cx = ctx(&d, &s, &c, &l);
    let mut t = create("selection");
    let a = t.pointer(&cx, &PointerEvent::new(PointerKind::Down, 150.0, 150.0));
    assert_eq!(a, vec![Action::Exec("selection.set".into(), serde_json::json!({"ids": [fid.0], "content": false}))]);
    t.pointer(&cx, &PointerEvent::new(PointerKind::Up, 150.0, 150.0));
    // Marquee from empty space.
    t.pointer(&cx, &PointerEvent::new(PointerKind::Down, 20.0, 20.0));
    t.pointer(&cx, &PointerEvent::new(PointerKind::Drag, 300.0, 300.0));
    let a = t.pointer(&cx, &PointerEvent::new(PointerKind::Up, 300.0, 300.0));
    assert_eq!(a, vec![Action::Exec("selection.set".into(), serde_json::json!({"ids": [fid.0], "add": false}))]);
}

#[test]
fn resize_rect_modifiers() {
    let r = Rect::new(0.0, 0.0, 100.0, 50.0);
    assert_eq!(select::resize_rect(r, 4, designcraft_geom::Point::new(200.0, 80.0), Mods::default()), Rect::new(0.0, 0.0, 200.0, 80.0));
    let p = select::resize_rect(r, 4, designcraft_geom::Point::new(200.0, 60.0), Mods { shift: true, ..Default::default() });
    assert_eq!(p, Rect::new(0.0, 0.0, 200.0, 100.0));
    let c = select::resize_rect(r, 3, designcraft_geom::Point::new(150.0, 25.0), Mods { alt: true, ..Default::default() });
    assert_eq!(c, Rect::new(-50.0, 0.0, 150.0, 50.0));
}

#[test]
fn cmd_shift_click_overrides_parent_item() {
    let mut d = Document::new(&NewDocument::default());
    let lid = d.default_layer();
    // On the parent's right page, which shows behind document page 1 (a right page).
    let px = d.parents[0].pages.last().unwrap().x;
    let (pid, _) =
        d.add_text_frame(SpreadRef::Parent(0), Rect::new(px + 100.0, 100.0, px + 200.0, 200.0), lid, "folio", ParaFormat::default()).unwrap();
    let (s, c, l) = (Selection::default(), Cache::new(), CanvasLayout::new(&d, false));
    let cx = ctx(&d, &s, &c, &l);
    let off = l.xf(SpreadRef::Doc(0)).translation();
    let x = d.spreads[0].pages[0].x + 150.0 + off.x;
    let mut t = create("selection");
    // A plain click doesn't reach parent items.
    let a = t.pointer(&cx, &PointerEvent::new(PointerKind::Down, x, 150.0 + off.y));
    assert_eq!(a, vec![Action::Exec("selection.set".into(), serde_json::json!({"ids": []}))]);
    t.pointer(&cx, &PointerEvent::new(PointerKind::Up, x, 150.0 + off.y));
    let mut ev = PointerEvent::new(PointerKind::Down, x, 150.0 + off.y);
    ev.mods = Mods { cmd: true, shift: true, ..Default::default() };
    let a = t.pointer(&cx, &ev);
    assert_eq!(a, vec![Action::Exec("layout.overrideParentItems".into(), serde_json::json!({"page": 0, "ids": [pid.0]}))]);
}

#[test]
fn gradient_tool_drag_sets_the_vector() {
    let mut d = Document::new(&NewDocument::default());
    let lid = d.default_layer();
    let (fid, _) = d.add_text_frame(SpreadRef::Doc(0), Rect::new(100.0, 100.0, 300.0, 200.0), lid, "", ParaFormat::default()).unwrap();
    let (s, c, l) = (Selection::default(), Cache::new(), CanvasLayout::new(&d, false));
    let cx = ctx(&d, &s, &c, &l);
    let off = l.xf(SpreadRef::Doc(0)).translation();
    let mut t = create("gradientSwatch");
    assert_eq!(t.pointer(&cx, &PointerEvent::new(PointerKind::Down, 120.0 + off.x, 150.0 + off.y)), vec![Action::Begin("Gradient".into())]);
    // Shift snaps a slightly tilted drag to horizontal.
    let a = t.pointer(&cx, &PointerEvent::new(PointerKind::Drag, 220.0 + off.x, 153.0 + off.y).with_mods(Mods { shift: true, ..Default::default() }));
    match &a[0] {
        Action::Preview(cmd, p) => {
            assert_eq!(cmd, "object.gradient");
            assert_eq!(p["ids"], serde_json::json!([fid.0]));
            assert_eq!(p["from"], serde_json::json!([120.0, 150.0]));
            let to = p["to"].as_array().unwrap();
            assert!((to[1].as_f64().unwrap() - 150.0).abs() < 1e-9, "{to:?}");
        }
        other => panic!("{other:?}"),
    }
    assert_eq!(t.pointer(&cx, &PointerEvent::new(PointerKind::Up, 220.0 + off.x, 153.0 + off.y)), vec![Action::Commit]);
}

#[test]
fn anchor_tools_emit_path_commands() {
    let mut d = Document::new(&NewDocument::default());
    let lid = d.default_layer();
    let (fid, _) = d.add_text_frame(SpreadRef::Doc(0), Rect::new(100.0, 100.0, 200.0, 200.0), lid, "", ParaFormat::default()).unwrap();
    let (s, c, l) = (Selection::items(vec![fid]), Cache::new(), CanvasLayout::new(&d, false));
    let cx = ctx(&d, &s, &c, &l);
    let off = l.xf(SpreadRef::Doc(0)).translation();
    let at = |x: f64, y: f64| (x + off.x, y + off.y);
    let ev = |k, (x, y): (f64, f64)| PointerEvent::new(k, x, y);
    let mut add = create("addAnchor");
    match &add.pointer(&cx, &ev(PointerKind::Down, at(150.0, 101.0)))[..] {
        [Action::Exec(cmd, p)] => {
            assert_eq!(cmd, "path.addAnchor");
            assert_eq!(p["at"], serde_json::json!([150.0, 101.0]));
        }
        other => panic!("{other:?}"),
    }
    assert!(add.pointer(&cx, &ev(PointerKind::Down, at(150.0, 150.0))).is_empty(), "inside, away from the edge");
    let mut del = create("deleteAnchor");
    assert_eq!(
        del.pointer(&cx, &ev(PointerKind::Down, at(200.0, 100.0))),
        vec![Action::Exec("path.deleteAnchor".into(), serde_json::json!({"id": fid.0, "subpath": 0, "anchor": 1}))]
    );
    // Convert: click toggles, drag pulls out handles.
    let mut conv = create("convertDirection");
    conv.pointer(&cx, &ev(PointerKind::Down, at(100.0, 100.0)));
    assert_eq!(
        conv.pointer(&cx, &ev(PointerKind::Up, at(100.0, 100.0))),
        vec![Action::Exec("path.convertAnchor".into(), serde_json::json!({"id": fid.0, "subpath": 0, "anchor": 0}))]
    );
    conv.pointer(&cx, &ev(PointerKind::Down, at(100.0, 100.0)));
    let a = conv.pointer(&cx, &ev(PointerKind::Drag, at(120.0, 90.0)));
    assert_eq!(a[0], Action::Begin("Convert Direction Point".into()));
    assert!(matches!(&a[1], Action::Preview(c, p) if c == "path.convertAnchor" && p["to"] == serde_json::json!([120.0, 90.0])));
    assert_eq!(conv.pointer(&cx, &ev(PointerKind::Up, at(120.0, 90.0))), vec![Action::Commit]);
}

#[test]
fn scissors_tool_cuts_where_clicked() {
    let mut d = Document::new(&NewDocument::default());
    let lid = d.default_layer();
    let (fid, _) = d.add_text_frame(SpreadRef::Doc(0), Rect::new(100.0, 100.0, 200.0, 200.0), lid, "", ParaFormat::default()).unwrap();
    let (s, c, l) = (Selection::items(vec![fid]), Cache::new(), CanvasLayout::new(&d, false));
    let cx = ctx(&d, &s, &c, &l);
    let off = l.xf(SpreadRef::Doc(0)).translation();
    let mut t = create("scissors");
    let a = t.pointer(&cx, &PointerEvent::new(PointerKind::Down, 150.0 + off.x, 100.0 + off.y));
    assert_eq!(a, vec![Action::Exec("path.split".into(), serde_json::json!({"id": fid.0, "at": [150.0, 100.0]}))]);
}

#[test]
fn pencil_draws_a_smooth_path() {
    let d = Document::new(&NewDocument::default());
    let (s, c, l) = (Selection::default(), Cache::new(), CanvasLayout::new(&d, false));
    let cx = ctx(&d, &s, &c, &l);
    let off = l.xf(SpreadRef::Doc(0)).translation();
    let mut t = create("pencil");
    t.pointer(&cx, &PointerEvent::new(PointerKind::Down, 100.0 + off.x, 100.0 + off.y));
    for i in 1..=90 {
        let a = (i as f64 * 2.0).to_radians();
        t.pointer(&cx, &PointerEvent::new(PointerKind::Drag, 100.0 + 50.0 * a.sin() + off.x, 100.0 + 50.0 * (1.0 - a.cos()) + off.y));
    }
    assert!(t.busy());
    let a = t.pointer(&cx, &PointerEvent::new(PointerKind::Up, 150.0 + off.x, 150.0 + off.y));
    match &a[..] {
        [Action::Exec(cmd, p)] => {
            assert_eq!(cmd, "path.create");
            let n = p["anchors"].as_array().unwrap().len();
            assert!((3..30).contains(&n), "{n} anchors");
            assert_eq!(p["anchors"][0]["p"], serde_json::json!([100.0, 100.0]));
        }
        other => panic!("{other:?}"),
    }
}

#[test]
fn zoom_tool_click_and_scrub() {
    let d = Document::new(&NewDocument::default());
    let (s, c, l) = (Selection::default(), Cache::new(), CanvasLayout::new(&d, false));
    let cx = ctx(&d, &s, &c, &l);
    let mut t = create("zoom");
    t.pointer(&cx, &PointerEvent::new(PointerKind::Down, 100.0, 100.0));
    assert_eq!(
        t.pointer(&cx, &PointerEvent::new(PointerKind::Up, 100.0, 100.0)),
        vec![Action::View(serde_json::json!({"zoomAt": [100.0, 100.0], "factor": 2.0}))]
    );
    // Dragging 150 px right zooms in by e, anchored at the press point; no click zoom after.
    t.pointer(&cx, &PointerEvent::new(PointerKind::Down, 100.0, 100.0));
    match &t.pointer(&cx, &PointerEvent::new(PointerKind::Drag, 250.0, 100.0))[..] {
        [Action::View(v)] => {
            assert_eq!(v["zoomAt"], serde_json::json!([100.0, 100.0]));
            assert!((v["factor"].as_f64().unwrap() - std::f64::consts::E).abs() < 1e-9);
        }
        other => panic!("{other:?}"),
    }
    assert!(t.pointer(&cx, &PointerEvent::new(PointerKind::Up, 250.0, 100.0)).is_empty());
}

#[test]
fn hand_tool_alt_press_power_zooms() {
    let d = Document::new(&NewDocument::default());
    let (s, c, l) = (Selection::default(), Cache::new(), CanvasLayout::new(&d, false));
    let cx = ctx(&d, &s, &c, &l);
    let mut t = create("hand");
    let alt = Mods { alt: true, ..Default::default() };
    let phase = |a: Vec<Action>| match &a[..] {
        [Action::View(v)] => v["powerZoom"].as_str().unwrap_or("").to_string(),
        other => panic!("{other:?}"),
    };
    assert_eq!(phase(t.pointer(&cx, &PointerEvent::new(PointerKind::Down, 10.0, 10.0).with_mods(alt))), "start");
    assert_eq!(phase(t.pointer(&cx, &PointerEvent::new(PointerKind::Drag, 50.0, 60.0))), "move");
    assert_eq!(phase(t.pointer(&cx, &PointerEvent::new(PointerKind::Up, 50.0, 60.0))), "end");
    // A plain press pans as before.
    t.pointer(&cx, &PointerEvent::new(PointerKind::Down, 10.0, 10.0));
    assert!(t.pointer(&cx, &PointerEvent::new(PointerKind::Up, 10.0, 10.0)).is_empty());
}

#[test]
fn page_tool_opens_page_size() {
    let d = Document::new(&NewDocument::default());
    let (s, c, l) = (Selection::default(), Cache::new(), CanvasLayout::new(&d, false));
    let cx = ctx(&d, &s, &c, &l);
    let off = l.xf(SpreadRef::Doc(0)).translation();
    let mut t = create("page");
    let a = t.pointer(&cx, &PointerEvent::new(PointerKind::Up, 200.0 + off.x + d.spreads[0].pages[0].x, 200.0 + off.y));
    assert_eq!(a, vec![Action::Dialog("cmd:layout.pageSize".into(), serde_json::json!({"pages": [1], "width": 612.0, "height": 792.0}))]);
}

#[test]
fn type_on_path_tool_targets_paths() {
    let mut d = Document::new(&NewDocument::default());
    let lid = d.default_layer();
    let id = designcraft_doc::ItemId(d.alloc());
    let mut it = designcraft_doc::Item::new(
        id,
        lid,
        designcraft_doc::Shape::GraphicLine,
        designcraft_geom::shapes::line(designcraft_geom::Point::new(100.0, 100.0), designcraft_geom::Point::new(300.0, 100.0)),
    );
    it.stroke = designcraft_doc::Stroke::default();
    d.insert_item(SpreadRef::Doc(0), it, None).unwrap();
    let (s, c, l) = (Selection::default(), Cache::new(), CanvasLayout::new(&d, false));
    let cx = ctx(&d, &s, &c, &l);
    let off = l.xf(SpreadRef::Doc(0)).translation();
    let mut t = create("typeOnPath");
    let a = t.pointer(&cx, &PointerEvent::new(PointerKind::Up, 200.0 + off.x, 100.0 + off.y));
    assert_eq!(a, vec![Action::Exec("type.onPath".into(), serde_json::json!({"id": id.0})), Action::SwitchTool("type".into())]);
}

#[test]
fn vertical_type_tool_draws_vertical_frames() {
    let d = Document::new(&NewDocument::default());
    let (s, c, l) = (Selection::default(), Cache::new(), CanvasLayout::new(&d, false));
    let cx = ctx(&d, &s, &c, &l);
    let mut t = create("verticalType");
    assert_eq!(t.id(), "verticalType");
    t.pointer(&cx, &PointerEvent::new(PointerKind::Down, 100.0, 100.0));
    let a = t.pointer(&cx, &PointerEvent::new(PointerKind::Drag, 200.0, 300.0));
    let p = a.iter().find_map(|x| match x {
        Action::Preview(id, p) if id == "frame.create" => Some(p.clone()),
        _ => None,
    });
    assert_eq!(p.unwrap()["vertical"], true);
}

#[test]
fn turned_spread_maps_points_both_ways() {
    let mut d = Document::new(&NewDocument::default());
    std::sync::Arc::make_mut(&mut d.spreads[0]).pages[0].view_rotation = 1;
    let l = CanvasLayout::new(&d, false);
    let slot = l.slots[0];
    let b = d.spreads[0].bounds();
    assert!((slot.bounds.width() - b.height()).abs() < 1e-6);
    let p = designcraft_geom::Point::new(b.x0 + 10.0, b.y0 + 20.0);
    let c = slot.to_canvas(p);
    let back = slot.to_spread(c);
    assert!((back - p).hypot() < 1e-9);
    // A quarter turn clockwise: the spread's top-left lands at the canvas top-right.
    assert!((c.x - (slot.bounds.x1 - 20.0)).abs() < 1e-6 && (c.y - (slot.bounds.y0 + 10.0)).abs() < 1e-6, "{c:?} {:?}", slot.bounds);
    let (sr, sp) = l.spread_at(c).unwrap();
    assert_eq!(sr, SpreadRef::Doc(0));
    assert!((sp - p).hypot() < 1e-9);
    // A canvas drag rightwards is a drag down the spread.
    let dv = slot.delta_to_spread(designcraft_geom::Vec2::new(10.0, 0.0));
    assert!(dv.x.abs() < 1e-9 && (dv.y + 10.0).abs() < 1e-9 || (dv.y - 10.0).abs() < 1e-9, "{dv:?}");
}

fn add_rect(doc: &mut Document, rect: Rect) -> ItemId {
    let layer = doc.default_layer();
    let id = ItemId(doc.alloc());
    let item = Item::new(id, layer, Shape::Rectangle, shapes::rectangle(rect));
    doc.insert_item(SpreadRef::Doc(0), item, None).unwrap();
    id
}

/// Dimensions on, ruler guides and the document grid off. Alignment is optional.
fn dims_ctx<'a>(d: &'a Document, s: &'a Selection, c: &'a Cache, l: &'a CanvasLayout, align: bool) -> ToolContext<'a> {
    let mut cx = ctx(d, s, c, l);
    cx.snap = SnapView::FACTORY;
    cx.snap.snap_to_guides = false;
    cx.snap.snap_to_document_grid = false;
    cx.snap.align_edges = align;
    cx.snap.align_centers = align;
    cx
}

fn spread_pt(l: &CanvasLayout, x: f64, y: f64) -> Point {
    l.to_canvas(SpreadRef::Doc(0), Point::new(x, y))
}

fn near(a: Point, b: Point) -> bool {
    (a.x - b.x).abs() < 1e-4 && (a.y - b.y).abs() < 1e-4
}

fn guide_matches(overlays: &[Overlay], a: Point, b: Point) -> bool {
    overlays.iter().any(|o| match o {
        Overlay::Guide { a: ga, b: gb } => (near(*ga, a) && near(*gb, b)) || (near(*ga, b) && near(*gb, a)),
        _ => false,
    })
}

fn guide_list(overlays: &[Overlay]) -> String {
    overlays
        .iter()
        .filter_map(|o| match o {
            Overlay::Guide { a, b } => Some(format!("({:.4},{:.4})-({:.4},{:.4})", a.x, a.y, b.x, b.y)),
            _ => None,
        })
        .collect::<Vec<_>>()
        .join("; ")
}

#[test]
fn shift_alt_resize_keeps_aspect_and_center() {
    // 100 by 50, center (150, 125), aspect 2. Corner handle, Shift and Alt.
    // The centered width is 160. Another item's side is 163, inside the zone.
    let mut doc = Document::new(&NewDocument::default());
    let moving = add_rect(&mut doc, Rect::new(100.0, 100.0, 200.0, 150.0));
    add_rect(&mut doc, Rect::new(400.0, 400.0, 563.0, 430.0));
    let sel = Selection::items(vec![moving]);
    let cache = Cache::new();
    let layout = CanvasLayout::new(&doc, false);
    let cx = dims_ctx(&doc, &sel, &cache, &layout, false);
    let mut t = create("selection");
    let mods = Mods { shift: true, alt: true, ..Mods::default() };
    let down = spread_pt(&layout, 200.0, 150.0);
    let drag = spread_pt(&layout, 230.0, 155.0);
    t.pointer(&cx, &PointerEvent::new(PointerKind::Down, down.x, down.y).with_mods(mods));
    let a = t.pointer(&cx, &PointerEvent::new(PointerKind::Drag, drag.x, drag.y).with_mods(mods));
    let params = match &a[..] {
        [Action::Preview(cmd, p)] => {
            assert_eq!(cmd, "transform.resize");
            p
        }
        other => panic!("{other:?}"),
    };
    let to = params["to"].as_array().unwrap();
    let (x0, y0, x1, y1) = (to[0].as_f64().unwrap(), to[1].as_f64().unwrap(), to[2].as_f64().unwrap(), to[3].as_f64().unwrap());
    let (w, h) = (x1 - x0, y1 - y0);
    let (cx0, cy0) = ((x0 + x1) / 2.0, (y0 + y1) / 2.0);
    assert!((cx0 - 150.0).abs() < 1e-4 && (cy0 - 125.0).abs() < 1e-4, "center walked to ({cx0}, {cy0}) rect {to:?}");
    assert!((w / h - 2.0).abs() < 1e-4, "aspect {} rect {to:?}", w / h);
    assert!((w - 163.0).abs() < 1e-4, "driving length {w}, want the matched 163, not twice the correction");
}

#[test]
fn line_length_keeps_the_crossed_guide() {
    // Diagonal from (100, 100) to (180, 140). Length is about 89.44, near a 92 side.
    // Endpoint y is 2 pt from the other item's top, so Y is a position snap and X is the length.
    let mut doc = Document::new(&NewDocument::default());
    add_rect(&mut doc, Rect::new(400.0, 142.0, 492.0, 180.0));
    let sel = Selection::default();
    let cache = Cache::new();
    let layout = CanvasLayout::new(&doc, false);
    let cx = dims_ctx(&doc, &sel, &cache, &layout, true);
    let mut t = create("line");
    let a = spread_pt(&layout, 100.0, 100.0);
    let b = spread_pt(&layout, 180.0, 140.0);
    t.pointer(&cx, &PointerEvent::new(PointerKind::Down, a.x, a.y));
    let actions = t.pointer(&cx, &PointerEvent::new(PointerKind::Drag, b.x, b.y));
    let params = actions
        .iter()
        .find_map(|act| match act {
            Action::Preview(cmd, p) if cmd == "line.create" => Some(p),
            _ => None,
        })
        .unwrap();
    let end = params["b"].as_array().unwrap();
    let (bx, by) = (end[0].as_f64().unwrap(), end[1].as_f64().unwrap());
    let len = 80.0_f64.hypot(40.0);
    let target = 92.0_f64;
    let dy = 142.0 - 100.0;
    // Y stays on the guide. X is solved so the segment length is 92, not by adding the difference to x.
    let remain = target * target - dy * dy;
    let expect_x = 100.0 + remain.sqrt();
    assert!((by - 142.0).abs() < 1e-4, "endpoint y {by} left the guide at 142 (x {bx})");
    assert!((bx - expect_x).abs() < 1e-4, "endpoint x {bx}, want {expect_x}");
    let got = (bx - 100.0).hypot(by - 100.0);
    assert!((got - target).abs() < 1e-4, "length {got}, raw was {len}");
    assert!((bx - (180.0 + (target - len))).abs() > 0.1, "x still took the raw length difference");
}

#[test]
fn stroked_move_aligns_visible_edges() {
    // 2 pt centered strokes. The other path ends at 70, so its visible edge is 71.
    // A geometric move would land the path on 71. The visible edge must land there instead.
    let mut doc = Document::new(&NewDocument::default());
    let layer = doc.default_layer();
    let stroke = Stroke { weight: 2.0, ..Stroke::default() };
    let make = |doc: &mut Document, rect: Rect| {
        let id = ItemId(doc.alloc());
        let mut item = Item::new(id, layer, Shape::Rectangle, shapes::rectangle(rect));
        item.stroke = stroke.clone();
        doc.insert_item(SpreadRef::Doc(0), item, None).unwrap();
        id
    };
    let _other = make(&mut doc, Rect::new(40.0, 100.0, 70.0, 140.0));
    let moving = make(&mut doc, Rect::new(100.0, 100.0, 160.0, 140.0));
    let sel = Selection::items(vec![moving]);
    let cache = Cache::new();
    let layout = CanvasLayout::new(&doc, false);
    let cx = dims_ctx(&doc, &sel, &cache, &layout, true);
    let mut t = create("selection");
    let down = spread_pt(&layout, 130.0, 120.0);
    let drag = spread_pt(&layout, 103.0, 120.0);
    t.pointer(&cx, &PointerEvent::new(PointerKind::Down, down.x, down.y));
    let actions = t.pointer(&cx, &PointerEvent::new(PointerKind::Drag, drag.x, drag.y));
    let dx = match actions.last() {
        Some(Action::Preview(cmd, p)) => {
            assert_eq!(cmd, "transform.move");
            p["dx"].as_f64().unwrap()
        }
        other => panic!("{other:?}"),
    };
    assert!((dx - -28.0).abs() < 1e-4, "dx {dx}, want -28 so the visible edge meets 71");
}

#[test]
fn vertical_line_length_matches_along_the_segment() {
    // Vertical, length 80, near a 84 side. The match must move y, not add 4 to x.
    let mut doc = Document::new(&NewDocument::default());
    add_rect(&mut doc, Rect::new(400.0, 400.0, 450.0, 484.0));
    let sel = Selection::default();
    let cache = Cache::new();
    let layout = CanvasLayout::new(&doc, false);
    let cx = dims_ctx(&doc, &sel, &cache, &layout, false);
    let mut t = create("line");
    let a = spread_pt(&layout, 100.0, 100.0);
    let b = spread_pt(&layout, 100.0, 180.0);
    t.pointer(&cx, &PointerEvent::new(PointerKind::Down, a.x, a.y));
    let actions = t.pointer(&cx, &PointerEvent::new(PointerKind::Drag, b.x, b.y));
    let params = actions
        .iter()
        .find_map(|act| match act {
            Action::Preview(cmd, p) if cmd == "line.create" => Some(p),
            _ => None,
        })
        .unwrap();
    let end = params["b"].as_array().unwrap();
    let (bx, by) = (end[0].as_f64().unwrap(), end[1].as_f64().unwrap());
    assert!((bx - 100.0).abs() < 1e-4, "x moved to {bx}");
    assert!((by - 184.0).abs() < 1e-4, "y {by}, want 184");
    let xf = layout.xf(SpreadRef::Doc(0));
    let seg = (xf * Point::new(100.0, 100.0), xf * Point::new(100.0, 184.0));
    let overlays = t.overlays(&cx);
    assert!(guide_matches(&overlays, seg.0, seg.1), "guide is not the segment [{}]", guide_list(&overlays));
}

#[test]
fn shift_frame_stays_square_on_the_driving_guide() {
    // Pointer dx is larger, so x drives. The square's right edge is 3 pt from the guide at 203.
    // Y must follow that snapped size, not stay at the unconstrained square.
    let mut doc = Document::new(&NewDocument::default());
    doc.settings.baseline_grid.increment = 0.0;
    Arc::make_mut(&mut doc.spreads[0]).pages[0].guides.push(Guide {
        orientation: Orientation::Vertical,
        position: 203.0,
        spread: true,
        locked: false,
        layer: None,
        liquid: false,
    });
    let sel = Selection::default();
    let cache = Cache::new();
    let layout = CanvasLayout::new(&doc, false);
    let mut cx = ctx(&doc, &sel, &cache, &layout);
    cx.snap = SnapView::FACTORY;
    cx.snap.snap_to_document_grid = false;
    cx.snap.smart_dimensions = false;
    let mut t = create("rectangle");
    let mods = Mods { shift: true, ..Mods::default() };
    let a = spread_pt(&layout, 120.0, 120.0);
    let b = spread_pt(&layout, 200.0, 150.0);
    t.pointer(&cx, &PointerEvent::new(PointerKind::Down, a.x, a.y).with_mods(mods));
    let actions = t.pointer(&cx, &PointerEvent::new(PointerKind::Drag, b.x, b.y).with_mods(mods));
    let rect = actions
        .iter()
        .find_map(|act| match act {
            Action::Preview(cmd, p) if cmd == "frame.create" => p["rect"].as_array(),
            _ => None,
        })
        .unwrap();
    let (x0, y0, x1, y1) = (rect[0].as_f64().unwrap(), rect[1].as_f64().unwrap(), rect[2].as_f64().unwrap(), rect[3].as_f64().unwrap());
    assert!((x1 - 203.0).abs() < 1e-4, "driving edge {x1} left the guide");
    assert!((y1 - y0 - (x1 - x0)).abs() < 1e-4, "not square: {x0} {y0} {x1} {y1}");
    assert!((y1 - 203.0).abs() < 1e-4, "other edge {y1} did not follow the snapped size");
}

#[test]
fn drag_onto_the_next_spread_snaps_to_that_guide() {
    // The origin spread has a guide 4 pt from the unsnapped top. The destination guide is 2 pt away.
    // Snapping on the origin would move by 4. Snapping under the pointer moves by 2.
    let mut doc = Document::new(&NewDocument { pages: 2, facing_pages: false, ..NewDocument::default() });
    doc.settings.baseline_grid.increment = 0.0;
    let id = add_rect(&mut doc, Rect::new(100.0, 100.0, 160.0, 140.0));
    let guide = |position: f64| Guide { orientation: Orientation::Horizontal, position, spread: true, locked: false, layer: None, liquid: false };
    Arc::make_mut(&mut doc.spreads[1]).pages[0].guides.push(guide(52.0));
    let sel = Selection::items(vec![id]);
    let cache = Cache::new();
    let layout = CanvasLayout::new(&doc, false);
    assert_eq!(layout.slots.len(), 2);
    let item = Rect::new(100.0, 100.0, 160.0, 140.0);
    let raw_top = 50.0;
    let corner0 = layout.to_canvas(SpreadRef::Doc(0), Point::new(item.x0, item.y0));
    let corner_raw = layout.to_canvas(SpreadRef::Doc(1), Point::new(item.x0, raw_top));
    let travel = designcraft_geom::Vec2::new(corner_raw.x - corner0.x, corner_raw.y - corner0.y);
    let origin_top = item.y0 + layout.delta_to_spread(SpreadRef::Doc(0), travel).y;
    Arc::make_mut(&mut doc.spreads[0]).pages[0].guides.push(guide(origin_top - 4.0));
    let mut cx = ctx(&doc, &sel, &cache, &layout);
    cx.snap = SnapView::FACTORY;
    cx.snap.snap_to_document_grid = false;
    let down = layout.to_canvas(SpreadRef::Doc(0), item.center());
    let drag_at = Point::new(down.x + (corner_raw.x - corner0.x), down.y + (corner_raw.y - corner0.y));
    let mut t = create("selection");
    t.pointer(&cx, &PointerEvent::new(PointerKind::Down, down.x, down.y));
    let actions = t.pointer(&cx, &PointerEvent::new(PointerKind::Drag, drag_at.x, drag_at.y));
    let params = match actions.last() {
        Some(Action::Preview(cmd, p)) => {
            assert_eq!(cmd, "transform.move");
            p
        }
        other => panic!("{other:?}"),
    };
    let dy = params["dy"].as_f64().unwrap();
    assert_eq!(params["toSpread"], spread_json(SpreadRef::Doc(1)));
    assert!((dy - (52.0 - item.y0)).abs() < 1e-3, "dy {dy}, origin snap would not land on 52");
}

#[test]
fn dimension_guide_lies_on_the_committed_side() {
    let mut problems = Vec::new();

    // Centered scale. Width 120 grows to the matched 123, so each side moves 1.5, not 3.
    let mut doc = Document::new(&NewDocument::default());
    let moving = add_rect(&mut doc, Rect::new(100.0, 100.0, 200.0, 160.0));
    add_rect(&mut doc, Rect::new(400.0, 400.0, 523.0, 440.0));
    let sel = Selection::items(vec![moving]);
    let cache = Cache::new();
    let layout = CanvasLayout::new(&doc, false);
    let cx = dims_ctx(&doc, &sel, &cache, &layout, false);
    let mut t = create("scale");
    let center = spread_pt(&layout, 150.0, 130.0);
    t.pointer(&cx, &PointerEvent::new(PointerKind::Down, center.x + 50.0, center.y + 30.0));
    let drag = t.pointer(&cx, &PointerEvent::new(PointerKind::Drag, center.x + 60.0, center.y + 40.0));
    let (sx, sy) = match &drag[..] {
        [Action::Preview(cmd, p)] => {
            assert_eq!(cmd, "transform.scale");
            (p["sx"].as_f64().unwrap(), p["sy"].as_f64().unwrap())
        }
        other => panic!("{other:?}"),
    };
    let committed = Rect::new(150.0 - 50.0 * sx.abs(), 130.0 - 30.0 * sy.abs(), 150.0 + 50.0 * sx.abs(), 130.0 + 30.0 * sy.abs());
    let xf = layout.xf(SpreadRef::Doc(0));
    let bottom = (xf * Point::new(committed.x0, committed.y1), xf * Point::new(committed.x1, committed.y1));
    let overlays = t.overlays(&cx);
    let scale_guides = overlays.iter().filter(|o| matches!(o, Overlay::Guide { .. })).count();
    if scale_guides != 1 || !guide_matches(&overlays, bottom.0, bottom.1) {
        problems.push(format!(
            "scale guide is not on the committed bottom ({:.4},{:.4})-({:.4},{:.4}), sx {sx} sy {sy}, {scale_guides} guides [{}]",
            bottom.0.x,
            bottom.0.y,
            bottom.1.x,
            bottom.1.y,
            guide_list(&overlays)
        ));
    }

    // Shift resize. Width snaps 130 to 133, then the height follows the aspect and the bottom moves.
    let mut doc = Document::new(&NewDocument::default());
    let moving = add_rect(&mut doc, Rect::new(100.0, 100.0, 200.0, 150.0));
    add_rect(&mut doc, Rect::new(400.0, 400.0, 533.0, 430.0));
    let sel = Selection::items(vec![moving]);
    let cache = Cache::new();
    let layout = CanvasLayout::new(&doc, false);
    let cx = dims_ctx(&doc, &sel, &cache, &layout, false);
    let mut t = create("selection");
    let mods = Mods { shift: true, ..Mods::default() };
    let down = spread_pt(&layout, 200.0, 150.0);
    let drag_at = spread_pt(&layout, 230.0, 155.0);
    t.pointer(&cx, &PointerEvent::new(PointerKind::Down, down.x, down.y));
    let a = t.pointer(&cx, &PointerEvent::new(PointerKind::Drag, drag_at.x, drag_at.y).with_mods(mods));
    let to = match &a[..] {
        [Action::Preview(cmd, p)] => {
            assert_eq!(cmd, "transform.resize");
            p["to"].as_array().unwrap()
        }
        other => panic!("{other:?}"),
    };
    let rect = Rect::new(to[0].as_f64().unwrap(), to[1].as_f64().unwrap(), to[2].as_f64().unwrap(), to[3].as_f64().unwrap());
    let xf = layout.xf(SpreadRef::Doc(0));
    let y = rect.y0.max(rect.y1);
    let (x0, x1) = (rect.x0.min(rect.x1), rect.x0.max(rect.x1));
    let side = (xf * Point::new(x0, y), xf * Point::new(x1, y));
    let overlays = t.overlays(&cx);
    let shift_guides = overlays.iter().filter(|o| matches!(o, Overlay::Guide { .. })).count();
    if shift_guides != 1 || !guide_matches(&overlays, side.0, side.1) {
        problems.push(format!(
            "shift guide is not on the committed side ({:.4},{:.4})-({:.4},{:.4}), rect {to:?}, {shift_guides} guides [{}]",
            side.0.x,
            side.0.y,
            side.1.x,
            side.1.y,
            guide_list(&overlays)
        ));
    }
    assert!(problems.is_empty(), "{}", problems.join(" | "));
}

#[test]
fn rotation_drag_lands_on_the_other_angle() {
    // Reference starts at 10. A raw command delta of 18 would show 28.
    // The other item is at 30, so the composed preview delta is 20, not 30.
    let mut doc = Document::new(&NewDocument::default());
    let moving = add_rotated(&mut doc, Rect::new(100.0, 100.0, 180.0, 140.0), 10.0);
    add_rotated(&mut doc, Rect::new(300.0, 80.0, 380.0, 140.0), 30.0);
    let sel = Selection::items(vec![moving]);
    let cache = Cache::new();
    let layout = CanvasLayout::new(&doc, false);
    let cx = dims_ctx(&doc, &sel, &cache, &layout, false);
    let center = cx.selection_bounds().unwrap().center();
    let radius = 80.0;
    let down = Point::new(center.x + radius, center.y);
    let aim = (-18.0_f64).to_radians();
    let drag_at = Point::new(center.x + radius * aim.cos(), center.y + radius * aim.sin());
    let raw_command = -((drag_at - center).atan2() - (down - center).atan2()).to_degrees();
    assert!((raw_command - 18.0).abs() < 1e-6, "unsnapped command {raw_command}");
    let mut t = create("rotate");
    t.pointer(&cx, &PointerEvent::new(PointerKind::Down, down.x, down.y));
    let actions = t.pointer(&cx, &PointerEvent::new(PointerKind::Drag, drag_at.x, drag_at.y));
    let angle = match &actions[..] {
        [Action::Preview(cmd, p)] => {
            assert_eq!(cmd, "transform.rotate");
            p["angle"].as_f64().unwrap()
        }
        other => panic!("{other:?}"),
    };
    assert!((angle - 20.0).abs() < 1e-4, "preview angle {angle}, want 20");
}

fn add_rotated(doc: &mut Document, rect: Rect, degrees: f64) -> ItemId {
    let layer = doc.default_layer();
    let id = ItemId(doc.alloc());
    let mut item = Item::new(id, layer, Shape::Rectangle, shapes::rectangle(rect));
    item.xf = designcraft_geom::Affine::rotate((-degrees).to_radians());
    doc.insert_item(SpreadRef::Doc(0), item, None).unwrap();
    id
}

#[test]
fn pen_click_snaps_to_a_guide() {
    let mut d = Document::new(&NewDocument::default());
    Arc::make_mut(&mut d.spreads[0]).pages[0].guides.push(Guide {
        orientation: Orientation::Vertical,
        position: 100.0,
        spread: true,
        locked: false,
        layer: None,
        liquid: false,
    });
    let (s, c, l) = (Selection::default(), Cache::new(), CanvasLayout::new(&d, false));
    let mut cx = ctx(&d, &s, &c, &l);
    cx.snap = SnapView::FACTORY;
    let mut t = create("pen");
    let click = |t: &mut Box<dyn Tool>, cx: &ToolContext, x: f64, y: f64| {
        t.pointer(cx, &PointerEvent::new(PointerKind::Down, x, y));
        t.pointer(cx, &PointerEvent::new(PointerKind::Up, x, y))
    };
    assert!(click(&mut t, &cx, 102.0, 80.0).is_empty());
    let up = click(&mut t, &cx, 180.0, 80.0);
    let Action::Exec(cmd, p) = &up[0] else {
        panic!("expected path.create, got {up:?}");
    };
    assert_eq!(cmd, "path.create");
    let x = p["anchors"][0]["p"][0].as_f64().unwrap();
    assert!((x - 100.0).abs() < 1e-6, "anchor x {x}");
}

/// A 100×100 frame at (100, 100) holding a 200×100 graphic that overhangs it.
fn graphic_frame(doc: &mut Document) -> ItemId {
    let id = ItemId(doc.alloc());
    let mut item = Item::new(id, doc.default_layer(), Shape::Rectangle, shapes::rectangle(Rect::new(100.0, 100.0, 200.0, 200.0)));
    item.content = designcraft_doc::Content::Graphic(designcraft_doc::Graphic {
        asset: designcraft_doc::AssetId(1),
        size: (200.0, 100.0),
        xf: designcraft_geom::Affine::translate((50.0, 100.0)),
        auto_fit: Default::default(),
        fit_align: 4,
        crop: [0.0; 4],
    });
    doc.insert_item(SpreadRef::Doc(0), item, None).unwrap();
    id
}

fn preview_params(a: &[Action]) -> serde_json::Value {
    match a.last() {
        Some(Action::Preview(cmd, p)) if cmd == "transform.move" => p.clone(),
        other => panic!("{other:?}"),
    }
}

#[test]
fn direct_selection_drags_content_inside_its_frame() {
    let mut d = Document::new(&NewDocument::default());
    let f = graphic_frame(&mut d);
    let (c, l) = (Cache::new(), CanvasLayout::new(&d, false));
    let none = Selection::default();
    let mut t = create("directSelection");
    let at = spread_pt(&l, 120.0, 130.0);
    let a = t.pointer(&ctx(&d, &none, &c, &l), &PointerEvent::new(PointerKind::Down, at.x, at.y));
    assert_eq!(a, vec![Action::Exec("selection.set".into(), serde_json::json!({"ids": [f.0], "content": true}))]);
    let sel = Selection { content: true, ..Selection::items(vec![f]) };
    let cx = ctx(&d, &sel, &c, &l);
    let a = t.pointer(&cx, &PointerEvent::new(PointerKind::Drag, at.x + 15.0, at.y - 4.0));
    assert_eq!(a.first(), Some(&Action::Begin("Move Content".into())));
    let p = preview_params(&a);
    assert_eq!(
        (p["dx"].as_f64(), p["dy"].as_f64(), &p["content"], &p["ids"]),
        (Some(15.0), Some(-4.0), &serde_json::json!(true), &serde_json::json!([f.0]))
    );
    assert_eq!(t.cursor(&cx, at, Mods::default()), Cursor::HandGrab);
    assert!(t.overlays(&cx).contains(&Overlay::ContentGhost(f)));
    // Shift keeps the drag on one axis.
    let a = t.pointer(&cx, &PointerEvent::new(PointerKind::Drag, at.x + 15.0, at.y - 4.0).with_mods(Mods { shift: true, ..Default::default() }));
    let p = preview_params(&a);
    assert_eq!((p["dx"].as_f64(), p["dy"].as_f64()), (Some(15.0), Some(0.0)));
    assert_eq!(t.pointer(&cx, &PointerEvent::new(PointerKind::Up, at.x + 15.0, at.y)), vec![Action::Commit]);
}

#[test]
fn selection_tool_grabber_moves_content_elsewhere_moves_the_frame() {
    let mut d = Document::new(&NewDocument::default());
    let f = graphic_frame(&mut d);
    let (c, l) = (Cache::new(), CanvasLayout::new(&d, false));
    let frame = Selection::items(vec![f]);
    let content = Selection { content: true, ..Selection::items(vec![f]) };
    let centre = spread_pt(&l, 150.0, 150.0);
    let mut t = create("selection");
    // Hovering the grabber shows the hand; it is there on hover before any click, too.
    assert_eq!(t.cursor(&ctx(&d, &Selection::default(), &c, &l), centre, Mods::default()), Cursor::Hand);
    let g = select::grabbers(&ctx(&d, &Selection::default(), &c, &l), Some(centre));
    assert!(matches!(g.as_slice(), [(id, p)] if *id == f && near(*p, centre)), "{g:?}");
    // Press on the grabber: the content is selected; the drag moves it.
    let a = t.pointer(&ctx(&d, &frame, &c, &l), &PointerEvent::new(PointerKind::Down, centre.x, centre.y));
    assert_eq!(a, vec![Action::Exec("selection.set".into(), serde_json::json!({"ids": [f.0], "content": true}))]);
    let a = t.pointer(&ctx(&d, &content, &c, &l), &PointerEvent::new(PointerKind::Drag, centre.x - 20.0, centre.y + 10.0));
    let p = preview_params(&a);
    assert_eq!((p["dx"].as_f64(), p["dy"].as_f64(), &p["content"]), (Some(-20.0), Some(10.0), &serde_json::json!(true)));
    assert_eq!(t.pointer(&ctx(&d, &content, &c, &l), &PointerEvent::new(PointerKind::Up, centre.x - 20.0, centre.y + 10.0)), vec![Action::Commit]);
    // A press elsewhere in the frame keeps the content selection; dragging selects and moves the frame.
    let edge = spread_pt(&l, 115.0, 185.0);
    assert!(t.pointer(&ctx(&d, &content, &c, &l), &PointerEvent::new(PointerKind::Down, edge.x, edge.y)).is_empty());
    let a = t.pointer(&ctx(&d, &content, &c, &l), &PointerEvent::new(PointerKind::Drag, edge.x + 30.0, edge.y));
    assert_eq!(a[0], Action::Exec("selection.set".into(), serde_json::json!({"ids": [f.0]})));
    assert_eq!(a[1], Action::Begin("Move".into()));
    let p = preview_params(&a);
    assert_eq!((p["dx"].as_f64(), p.get("content"), p.get("ids")), (Some(30.0), None, None));
    t.pointer(&ctx(&d, &frame, &c, &l), &PointerEvent::new(PointerKind::Up, edge.x + 30.0, edge.y));
    // With the frame selected, a drag away from the grabber moves the frame.
    t.pointer(&ctx(&d, &frame, &c, &l), &PointerEvent::new(PointerKind::Down, edge.x, edge.y));
    let a = t.pointer(&ctx(&d, &frame, &c, &l), &PointerEvent::new(PointerKind::Drag, edge.x, edge.y + 30.0));
    assert_eq!(a[0], Action::Begin("Move".into()));
    assert_eq!(preview_params(&a).get("content"), None);
}

#[test]
fn double_click_toggles_frame_and_content() {
    let mut d = Document::new(&NewDocument::default());
    let f = graphic_frame(&mut d);
    let (c, l) = (Cache::new(), CanvasLayout::new(&d, false));
    let at = spread_pt(&l, 120.0, 180.0);
    let mut t = create("selection");
    let dbl = PointerEvent::new(PointerKind::DoubleClick, at.x, at.y);
    let frame = Selection::items(vec![f]);
    let a = t.pointer(&ctx(&d, &frame, &c, &l), &dbl);
    assert_eq!(a, vec![Action::Exec("selection.set".into(), serde_json::json!({"ids": [f.0], "content": true}))]);
    let content = Selection { content: true, ..Selection::items(vec![f]) };
    let a = t.pointer(&ctx(&d, &content, &c, &l), &dbl);
    assert_eq!(a, vec![Action::Exec("selection.set".into(), serde_json::json!({"ids": [f.0], "content": false}))]);
}
