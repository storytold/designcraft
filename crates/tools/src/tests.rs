use std::sync::Arc;

use designcraft_compose::Cache;
use designcraft_doc::build::NewDocument;
use designcraft_doc::{Document, Guide, Item, ItemId, Orientation, ParaFormat, Selection, Shape, SpreadRef, Stroke};
use designcraft_geom::shapes;
use designcraft_geom::{Point, Rect, Unit};

use super::*;

fn ctx<'a>(d: &'a Document, s: &'a Selection, c: &'a Cache, l: &'a CanvasLayout) -> ToolContext<'a> {
    ToolContext {
        doc: d,
        selection: s,
        cache: c,
        layout: l,
        zoom: 1.0,
        layer: d.default_layer(),
        snap: SnapView::OFF,
        unit: Unit::Points,
        stroke_in_front: false,
    }
}

#[test]
fn color_theme_tool_requests_a_palette_without_editing_the_image() {
    let mut d = Document::new(&NewDocument::default());
    let id = add_rect(&mut d, Rect::new(100.0, 100.0, 200.0, 200.0));
    d.item_mut(id).unwrap().content = designcraft_doc::Content::Graphic(designcraft_doc::Graphic {
        layer_options: None,
        asset: designcraft_doc::AssetId(1),
        size: (100.0, 100.0),
        xf: designcraft_geom::Affine::IDENTITY,
        clip: None,
        auto_fit: Default::default(),
        fit_align: 4,
        crop: [0.0; 4],
        wrap: Default::default(),
        fill: Default::default(),
    });
    let (s, c, l) = (Selection::default(), Cache::new(), CanvasLayout::new(&d, false));
    let off = l.xf(SpreadRef::Doc(0)).translation();
    let cx = ctx(&d, &s, &c, &l);
    let mut tool = create("colorTheme");
    assert_eq!(tool.id(), "colorTheme");
    assert_eq!(
        tool.pointer(&cx, &PointerEvent::new(PointerKind::Down, 150.0 + off.x, 150.0 + off.y)),
        vec![Action::Dialog("colorTheme".into(), serde_json::json!({"id":id.0}))]
    );
    assert!(tool.pointer(&cx, &PointerEvent::new(PointerKind::Up, 150.0 + off.x, 150.0 + off.y)).is_empty());
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
fn resize_side_shift_keeps_the_other_axis_centred() {
    let r = Rect::new(0.0, 0.0, 100.0, 50.0);
    let shift = Mods { shift: true, ..Default::default() };
    assert_eq!(select::resize_rect(r, 3, Point::new(150.0, 25.0), shift), Rect::new(0.0, -12.5, 150.0, 62.5));
    assert_eq!(select::resize_rect(r, 5, Point::new(50.0, 100.0), shift), Rect::new(-50.0, 0.0, 150.0, 100.0));
}

#[test]
fn resize_corner_alt_shift_scales_around_the_centre() {
    let r = Rect::new(0.0, 0.0, 100.0, 50.0);
    let mods = Mods { shift: true, alt: true, ..Default::default() };
    // From the centre the pointer is at 0.2 by 0.6; the larger factor drives and the corner stays on y = 40.
    assert_eq!(select::resize_rect(r, 4, Point::new(60.0, 40.0), mods), Rect::new(20.0, 10.0, 80.0, 40.0));
    assert_eq!(select::resize_rect(r, 0, Point::new(-50.0, 10.0), mods), Rect::new(-50.0, -25.0, 150.0, 75.0));
}

#[test]
fn resize_corner_alt_mirrors_the_opposite_side() {
    let r = Rect::new(0.0, 0.0, 100.0, 50.0);
    let alt = Mods { alt: true, ..Default::default() };
    assert_eq!(select::resize_rect(r, 0, Point::new(10.0, 5.0), alt), Rect::new(10.0, 5.0, 90.0, 45.0));
    assert_eq!(select::resize_rect(r, 4, Point::new(120.0, 60.0), alt), Rect::new(-20.0, -10.0, 120.0, 60.0));
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

/// On a page whose parent B is based on A, A's items are hit too, B's win where they overlap, and
/// nothing is hit when the page hides parent items.
#[test]
fn parent_hit_reaches_base_parent_items_topmost_first() {
    let mut d = Document::new(&NewDocument { pages: 1, facing_pages: false, ..Default::default() });
    let lid = d.default_layer();
    let a = d.parents[0].id;
    let b = d.add_parent("B", "Opening", 1, 12.0, designcraft_doc::Margins::uniform(36.0));
    Arc::make_mut(&mut d.parents[1]).parent.as_mut().unwrap().based_on = Some(a);
    d.apply_parent(&[0], Some(b)).unwrap();
    let (a_item, _) = d.add_text_frame(SpreadRef::Parent(0), Rect::new(100.0, 100.0, 200.0, 200.0), lid, "a", ParaFormat::default()).unwrap();
    let (b_item, _) = d.add_text_frame(SpreadRef::Parent(1), Rect::new(150.0, 150.0, 250.0, 250.0), lid, "b", ParaFormat::default()).unwrap();
    let hit = |d: &Document, x: f64, y: f64| {
        let (s, c, l) = (Selection::default(), Cache::new(), CanvasLayout::new(d, false));
        let off = l.xf(SpreadRef::Doc(0)).translation();
        ctx(d, &s, &c, &l).hit_parent_on_page(Point::new(x + off.x, y + off.y))
    };
    assert_eq!(hit(&d, 120.0, 120.0), Some((0, a_item)));
    assert_eq!(hit(&d, 175.0, 175.0), Some((0, b_item)), "B's item is drawn over A's");
    Arc::make_mut(&mut d.spreads[0]).pages[0].show_parent_items = false;
    assert_eq!(hit(&d, 120.0, 120.0), None);
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
    // While drawing, the stroke so far is shown, in the colour of the layer it goes on.
    let layer = d.layer(cx.layer).unwrap().color;
    match &t.overlays(&cx)[..] {
        [crate::Overlay::Path { path, color, dashed: false }] => {
            assert_eq!(*color, layer);
            assert!(path.elements().len() > 40, "the stroke so far: {} points", path.elements().len());
            assert_eq!(path.elements()[0], designcraft_geom::PathEl::MoveTo(designcraft_geom::Point::new(100.0 + off.x, 100.0 + off.y)));
        }
        other => panic!("{other:?}"),
    }
    let a = t.pointer(&cx, &PointerEvent::new(PointerKind::Up, 150.0 + off.x, 150.0 + off.y));
    assert!(t.overlays(&cx).is_empty(), "the preview ends with the stroke");
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
fn page_tool_selects_a_page_and_opens_page_size_on_double_click() {
    let d = Document::new(&NewDocument::default());
    let (s, c, l) = (Selection::default(), Cache::new(), CanvasLayout::new(&d, false));
    let cx = ctx(&d, &s, &c, &l);
    let off = l.xf(SpreadRef::Doc(0)).translation();
    let (x, y) = (200.0 + off.x + d.spreads[0].pages[0].x, 200.0 + off.y);
    let mut t = create("page");
    let a = t.pointer(&cx, &PointerEvent::new(PointerKind::Down, x, y));
    assert_eq!(a, vec![Action::Exec("layout.selectPage".into(), serde_json::json!({"page": 1, "ids": []}))]);
    let a = t.pointer(&cx, &PointerEvent::new(PointerKind::DoubleClick, x, y));
    assert_eq!(a, vec![Action::Dialog("cmd:layout.pageSize".into(), serde_json::json!({"pages": [1], "width": 612.0, "height": 792.0}))]);
    // The selected page shows its handles and its liquid page rule.
    let s = Selection { page: Some(0), ..Default::default() };
    let cx = ctx(&d, &s, &c, &l);
    let o = t.overlays(&cx);
    assert!(matches!(o.as_slice(), [Overlay::PageHandles(_), Overlay::Tag { text, .. }] if text == "Off"), "{o:?}");
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

/// A text frame with overset text (selected), an empty frame and a threaded pair x → y, on page 1.
struct ThreadDoc {
    d: Document,
    l: CanvasLayout,
    s: Selection,
    c: Cache,
    /// The selected text frame.
    a: ItemId,
    /// An empty (unassigned) frame.
    e: ItemId,
    x: ItemId,
    y: ItemId,
}

fn thread_doc() -> ThreadDoc {
    let mut d = Document::new(&NewDocument { pages: 2, ..Default::default() });
    let lid = d.default_layer();
    let (a, _) = d
        .add_text_frame(SpreadRef::Doc(0), Rect::new(100.0, 100.0, 250.0, 160.0), lid, &"Words flow on. ".repeat(40), ParaFormat::default())
        .unwrap();
    let e = ItemId(d.alloc());
    d.insert_item(SpreadRef::Doc(0), Item::new(e, lid, Shape::Rectangle, shapes::rectangle(Rect::new(100.0, 400.0, 250.0, 500.0))), None).unwrap();
    let (x, _) = d.add_text_frame(SpreadRef::Doc(0), Rect::new(300.0, 100.0, 450.0, 160.0), lid, "x", ParaFormat::default()).unwrap();
    let (y, _) = d.add_text_frame(SpreadRef::Doc(0), Rect::new(300.0, 400.0, 450.0, 500.0), lid, "", ParaFormat::default()).unwrap();
    d.thread(x, y).unwrap();
    let l = CanvasLayout::new(&d, false);
    ThreadDoc { s: Selection::items(vec![a]), c: Cache::new(), l, d, a, e, x, y }
}

impl ThreadDoc {
    fn cx(&self) -> ToolContext<'_> {
        ctx(&self.d, &self.s, &self.c, &self.l)
    }
    /// Canvas point of a spread-0 point.
    fn at(&self, x: f64, y: f64) -> Point {
        self.l.to_canvas(SpreadRef::Doc(0), Point::new(x, y))
    }
    fn port(&self, id: ItemId, out: bool) -> Point {
        thread::port_center(&self.d, &self.l, id, out, 1.0).unwrap()
    }
}

fn click(t: &mut Box<dyn Tool>, cx: &ToolContext, p: Point, mods: Mods) -> Vec<Action> {
    let mut v = t.pointer(cx, &PointerEvent::new(PointerKind::Down, p.x, p.y).with_mods(mods));
    v.extend(t.pointer(cx, &PointerEvent::new(PointerKind::Up, p.x, p.y).with_mods(mods)));
    v
}

fn thread_exec(p: serde_json::Value) -> Vec<Action> {
    vec![Action::Exec("text.thread".into(), p)]
}

#[test]
fn out_port_loads_text_and_a_click_threads_an_empty_frame() {
    let td = thread_doc();
    let cx = td.cx();
    let mut t = create("selection");
    let port = td.port(td.a, true);
    // The out port sits on the right edge, 12 px above the bottom-right corner.
    assert!((port - td.at(250.0, 148.0)).hypot() < 1e-9, "{port:?}");
    // A press just off the port's centre still loads (any zoom: tolerance is in pixels).
    assert!(click(&mut t, &cx, port + designcraft_geom::Vec2::new(3.0, -3.0), Mods::default()).is_empty());
    assert_eq!(t.cursor(&cx, td.at(500.0, 700.0), Mods::default()), Cursor::LoadedText);
    let over_e = td.at(175.0, 450.0);
    assert_eq!(t.cursor(&cx, over_e, Mods::default()), Cursor::ThreadLink);
    // A frame something already threads into can't join; neither can the loaded frame itself.
    assert_eq!(t.cursor(&cx, td.at(375.0, 450.0), Mods::default()), Cursor::NotAllowed);
    assert_eq!(t.cursor(&cx, td.at(175.0, 130.0), Mods::default()), Cursor::LoadedText);
    assert_eq!(click(&mut t, &cx, over_e, Mods::default()), thread_exec(serde_json::json!({"from": td.a.0, "to": td.e.0})));
    // Unloaded again: the cursor and clicks are back to normal.
    assert_eq!(t.cursor(&cx, over_e, Mods::default()), Cursor::Arrow);
}

#[test]
fn ports_hit_at_any_zoom() {
    let td = thread_doc();
    let mut cx = td.cx();
    cx.zoom = 4.0;
    let port = thread::port_center(&td.d, &td.l, td.a, false, 4.0).unwrap();
    assert!((port - td.at(100.0, 100.0 + 14.5 / 4.0)).hypot() < 1e-9);
    assert_eq!(thread::port_at(&cx, port + designcraft_geom::Vec2::new(1.5, 1.5)), Some((td.a, false)));
    assert_eq!(thread::port_at(&cx, port + designcraft_geom::Vec2::new(4.0, 0.0)), None, "8 screen px away at 4×");
}

#[test]
fn loaded_click_on_empty_page_makes_a_column_frame() {
    let td = thread_doc();
    let cx = td.cx();
    let mut t = create("selection");
    click(&mut t, &cx, td.port(td.a, true), Mods::default());
    let col = td.d.spreads[0].pages[0].column_rects()[0];
    let a = click(&mut t, &cx, td.at(200.0, 600.0), Mods::default());
    assert_eq!(a, thread_exec(serde_json::json!({"from": td.a.0, "rect": [col.x0, 600.0, col.x1, col.y1], "spread": {"kind": "doc", "index": 0}})));
}

#[test]
fn loaded_drag_makes_a_frame_of_that_size_and_shift_autoflows() {
    let td = thread_doc();
    let cx = td.cx();
    let mut t = create("selection");
    click(&mut t, &cx, td.port(td.a, true), Mods::default());
    let (p0, p1) = (td.at(320.0, 560.0), td.at(470.0, 700.0));
    let shift = Mods { shift: true, ..Default::default() };
    assert!(t.pointer(&cx, &PointerEvent::new(PointerKind::Down, p0.x, p0.y).with_mods(shift)).is_empty());
    assert!(t.pointer(&cx, &PointerEvent::new(PointerKind::Drag, p1.x, p1.y).with_mods(shift)).is_empty());
    assert_eq!(t.overlays(&cx), vec![Overlay::Marquee(Rect::from_points(p0, p1))]);
    let a = t.pointer(&cx, &PointerEvent::new(PointerKind::Up, p1.x, p1.y).with_mods(shift));
    assert_eq!(
        a,
        thread_exec(
            serde_json::json!({"from": td.a.0, "rect": [320.0, 560.0, 470.0, 700.0], "spread": {"kind": "doc", "index": 0}, "autoflow": true})
        )
    );
    assert_eq!(t.cursor(&cx, td.at(500.0, 700.0), Mods::default()), Cursor::Arrow, "autoflow unloads");
}

#[test]
fn loaded_drag_snaps_like_frame_drawing() {
    let td = thread_doc();
    let cx = ToolContext { snap: SnapView::FACTORY, ..td.cx() };
    let mut t = create("selection");
    click(&mut t, &cx, td.port(td.a, true), Mods::default());
    let col = td.d.spreads[0].pages[0].column_rects()[0];
    // Press and release a point or two inside the column's edges: both sides snap to them.
    let (p0, p1) = (td.at(col.x0 + 1.5, 560.0), td.at(col.x1 - 2.0, 650.0));
    t.pointer(&cx, &PointerEvent::new(PointerKind::Down, p0.x, p0.y));
    t.pointer(&cx, &PointerEvent::new(PointerKind::Drag, p1.x, p1.y));
    let Some(Overlay::Marquee(m)) = t.overlays(&cx).first().cloned() else { panic!("no marquee") };
    assert!((m.x0 - td.at(col.x0, 0.0).x).abs() < 1e-9 && (m.x1 - td.at(col.x1, 0.0).x).abs() < 1e-9, "{m:?}");
    let a = t.pointer(&cx, &PointerEvent::new(PointerKind::Up, p1.x, p1.y));
    let [Action::Exec(id, p)] = a.as_slice() else { panic!("{a:?}") };
    assert_eq!(id, "text.thread");
    let r: Vec<f64> = p["rect"].as_array().unwrap().iter().map(|v| v.as_f64().unwrap()).collect();
    assert_eq!((r[0], r[2]), (col.x0, col.x1), "{p}");
}

#[test]
fn alt_click_stays_loaded() {
    let mut td = thread_doc();
    let original = td.d.clone();
    let mut t = create("selection");
    let alt = Mods { alt: true, ..Default::default() };
    let col = td.d.spreads[0].pages[0].column_rects()[0];
    let empty = td.at(200.0, 600.0);
    let new_frame_from =
        |f: ItemId| thread_exec(serde_json::json!({"from": f.0, "rect": [col.x0, 600.0, col.x1, col.y1], "spread": {"kind": "doc", "index": 0}}));
    {
        let cx = td.cx();
        click(&mut t, &cx, td.port(td.a, true), Mods::default());
        assert_eq!(click(&mut t, &cx, td.at(175.0, 450.0), alt), thread_exec(serde_json::json!({"from": td.a.0, "to": td.e.0})));
        assert_eq!(t.cursor(&cx, td.at(500.0, 700.0), Mods::default()), Cursor::LoadedText);
        // The thread failed (the document is unchanged): still loaded from a.
        assert_eq!(click(&mut t, &cx, empty, alt), new_frame_from(td.a));
    }
    // That click's command ran: the next click threads after the frame it added.
    let (n, _) =
        td.d.add_text_frame(SpreadRef::Doc(0), Rect::new(col.x0, 600.0, col.x1, col.y1), td.d.default_layer(), "", ParaFormat::default()).unwrap();
    td.d.thread(td.a, n).unwrap();
    {
        let cx = td.cx();
        assert_eq!(click(&mut t, &cx, td.at(175.0, 450.0), alt), thread_exec(serde_json::json!({"from": n.0, "to": td.e.0})));
    }
    // Undone: back on a, not on a's old neighbour or the frame the undo removed.
    td.d = original;
    {
        let cx = td.cx();
        assert_eq!(click(&mut t, &cx, empty, Mods::default()), new_frame_from(td.a));
    }
    // From x, which threads into y: when threading e in between fails, x stays loaded, not y.
    let td = ThreadDoc { s: Selection::items(vec![td.x]), ..td };
    let cx = td.cx();
    click(&mut t, &cx, td.port(td.x, true), Mods::default());
    assert_eq!(click(&mut t, &cx, td.at(175.0, 450.0), alt), thread_exec(serde_json::json!({"from": td.x.0, "to": td.e.0})));
    assert_eq!(click(&mut t, &cx, empty, Mods::default()), new_frame_from(td.x));
}

#[test]
fn escape_unloads_and_keeps_the_selection() {
    let td = thread_doc();
    let cx = td.cx();
    let mut t = create("selection");
    click(&mut t, &cx, td.port(td.a, true), Mods::default());
    assert_eq!(t.key(&cx, ToolKey::Escape, Mods::default()), vec![Action::Cancel]);
    assert_eq!(t.cursor(&cx, td.at(500.0, 700.0), Mods::default()), Cursor::Arrow);
    // An ordinary click on empty space deselects again.
    let p = td.at(500.0, 700.0);
    assert_eq!(
        t.pointer(&cx, &PointerEvent::new(PointerKind::Down, p.x, p.y)),
        vec![Action::Exec("selection.set".into(), serde_json::json!({"ids": []}))]
    );
}

#[test]
fn loaded_click_on_the_linked_frame_unthreads() {
    // x is selected; its out port links to y.
    let td = thread_doc();
    let td = ThreadDoc { s: Selection::items(vec![td.x]), ..td };
    let cx = td.cx();
    let mut t = create("selection");
    click(&mut t, &cx, td.port(td.x, true), Mods::default());
    let over_y = td.at(375.0, 450.0);
    assert_eq!(t.cursor(&cx, over_y, Mods::default()), Cursor::Unthread);
    assert_eq!(
        click(&mut t, &cx, over_y, Mods::default()),
        vec![Action::Exec("text.unthread".into(), serde_json::json!({"frame": td.x.0, "side": "after"}))]
    );
    // From y's in port, x is the frame before.
    let td = ThreadDoc { s: Selection::items(vec![td.y]), ..td };
    let cx = td.cx();
    click(&mut t, &cx, td.port(td.y, false), Mods::default());
    assert_eq!(
        click(&mut t, &cx, td.at(375.0, 130.0), Mods::default()),
        vec![Action::Exec("text.unthread".into(), serde_json::json!({"frame": td.y.0, "side": "before"}))]
    );
}

#[test]
fn in_port_threads_in_front() {
    let td = thread_doc();
    let cx = td.cx();
    let mut t = create("selection");
    click(&mut t, &cx, td.port(td.a, false), Mods::default());
    assert_eq!(click(&mut t, &cx, td.at(175.0, 450.0), Mods::default()), thread_exec(serde_json::json!({"from": td.e.0, "to": td.a.0})));
}

#[test]
fn double_click_on_a_port_breaks_its_link() {
    let td = thread_doc();
    let td = ThreadDoc { s: Selection::items(vec![td.x]), ..td };
    let cx = td.cx();
    // A double click sent as such (control channel), and the canvas's second press.
    let second = |p: Point| [PointerEvent::new(PointerKind::DoubleClick, p.x, p.y), PointerEvent::new(PointerKind::Down, p.x, p.y).with_clicks(2)];
    let p = td.port(td.x, true);
    for ev in second(p) {
        let mut t = create("selection");
        click(&mut t, &cx, p, Mods::default());
        let a = t.pointer(&cx, &ev);
        assert_eq!(a, vec![Action::Exec("text.unthread".into(), serde_json::json!({"frame": td.x.0, "side": "after"}))], "{ev:?}");
        assert!(t.pointer(&cx, &PointerEvent::new(PointerKind::Up, p.x, p.y)).is_empty());
        assert_eq!(t.cursor(&cx, td.at(500.0, 700.0), Mods::default()), Cursor::Arrow);
        // The in port of the first frame has no link: nothing to break.
        let q = td.port(td.x, false);
        for ev in second(q) {
            assert!(t.pointer(&cx, &ev).is_empty(), "{ev:?}");
        }
    }
}

#[test]
fn clicks_in_a_row_edit_and_select_text() {
    let mut d = Document::new(&NewDocument::default());
    let lid = d.default_layer();
    let (fid, _) = d.add_text_frame(SpreadRef::Doc(0), Rect::new(100.0, 100.0, 300.0, 200.0), lid, "Two words", ParaFormat::default()).unwrap();
    let (s, c, l) = (Selection::default(), Cache::new(), CanvasLayout::new(&d, false));
    let cx = ctx(&d, &s, &c, &l);
    let off = l.xf(SpreadRef::Doc(0)).translation();
    let (x, y) = (150.0 + off.x, 110.0 + off.y);
    let press = |n: u8| PointerEvent::new(PointerKind::Down, x, y).with_clicks(n);
    // Selection tool: the second press of a double click edits the text there.
    let mut t = create("selection");
    let a = t.pointer(&cx, &press(1));
    assert!(matches!(&a[0], Action::Exec(id, _) if id == "selection.set"), "{a:?}");
    t.pointer(&cx, &PointerEvent::new(PointerKind::Up, x, y));
    let a = t.pointer(&cx, &press(2));
    assert_eq!(a.first(), Some(&Action::SwitchTool("type".into())));
    assert!(matches!(&a[1], Action::Exec(id, p) if id == "text.placeCaret" && p["frame"] == fid.0), "{a:?}");
    // Type tool: word, line, paragraph, then the story.
    let mut t = create("type");
    let unit = |a: Vec<Action>| match a.as_slice() {
        [Action::Exec(id, p)] if id == "text.selectAt" => p["unit"].as_str().map(str::to_string),
        [Action::Exec(id, _)] if id == "text.placeCaret" => None,
        other => panic!("{other:?}"),
    };
    assert_eq!(unit(t.pointer(&cx, &press(1))), None);
    for (n, want) in [(2, "word"), (3, "line"), (4, "paragraph"), (5, "story"), (9, "story")] {
        assert_eq!(unit(t.pointer(&cx, &press(n))).as_deref(), Some(want), "{n} clicks");
    }
    // A double click sent as such (control channel) selects a word too.
    assert_eq!(unit(t.pointer(&cx, &PointerEvent::new(PointerKind::DoubleClick, x, y))).as_deref(), Some("word"));
    // Moving while pressed extends the selection by the unit the clicks chose (#981).
    let a = t.pointer(&cx, &PointerEvent::new(PointerKind::Drag, x + 40.0, y));
    assert!(matches!(&a[..], [Action::Exec(id, p)] if id == "text.extendTo" && p["unit"] == "word" && p["frame"] == fid.0), "{a:?}");
    t.pointer(&cx, &PointerEvent::new(PointerKind::Up, x + 40.0, y));
    t.pointer(&cx, &press(3));
    let a = t.pointer(&cx, &PointerEvent::new(PointerKind::Drag, x + 40.0, y));
    assert!(matches!(&a[..], [Action::Exec(id, p)] if id == "text.extendTo" && p["unit"] == "line"), "{a:?}");
    // One click, then a drag: a character at a time, as before.
    t.pointer(&cx, &PointerEvent::new(PointerKind::Up, x + 40.0, y));
    t.pointer(&cx, &press(1));
    let a = t.pointer(&cx, &PointerEvent::new(PointerKind::Drag, x + 40.0, y));
    assert!(matches!(&a[..], [Action::Exec(id, p)] if id == "text.extendTo" && p.get("unit").is_none()), "{a:?}");
}

/// A 100×100 frame at (100, 100) holding a 200×100 graphic that overhangs it.
fn graphic_frame(doc: &mut Document) -> ItemId {
    let id = ItemId(doc.alloc());
    let mut item = Item::new(id, doc.default_layer(), Shape::Rectangle, shapes::rectangle(Rect::new(100.0, 100.0, 200.0, 200.0)));
    item.content = designcraft_doc::Content::Graphic(designcraft_doc::Graphic {
        layer_options: None,
        asset: designcraft_doc::AssetId(1),
        size: (200.0, 100.0),
        xf: designcraft_geom::Affine::translate((50.0, 100.0)),
        clip: None,
        auto_fit: Default::default(),
        fit_align: 4,
        crop: [0.0; 4],
        wrap: Default::default(),
        fill: Default::default(),
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

/// With text selected, the Eyedropper on a text frame picks up its text formatting instead of
/// copying the frame's fill and stroke onto the selection (InDesign's Eyedropper).
#[test]
fn eyedropper_with_text_selected_picks_up_text_formatting() {
    let mut d = Document::new(&NewDocument::default());
    let lid = d.default_layer();
    let (head, _) = d.add_text_frame(SpreadRef::Doc(0), Rect::new(100.0, 100.0, 300.0, 140.0), lid, "Header", ParaFormat::default()).unwrap();
    let (_, body) = d.add_text_frame(SpreadRef::Doc(0), Rect::new(100.0, 200.0, 300.0, 300.0), lid, "Body", ParaFormat::default()).unwrap();
    let s = Selection::text(designcraft_doc::TextSel { story: body, anchor: 0, focus: 4, frame: None, cell: None });
    let (c, l) = (Cache::new(), CanvasLayout::new(&d, false));
    let off = l.xf(SpreadRef::Doc(0)).translation();
    let cx = ctx(&d, &s, &c, &l);
    let mut t = create("eyedropper");
    let a = t.pointer(&cx, &PointerEvent::new(PointerKind::Down, 150.0 + off.x, 120.0 + off.y));
    assert_eq!(a, vec![Action::Exec("type.pickUpFormat".into(), serde_json::json!({"frame": head.0, "point": [150.0, 120.0]}))]);
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
fn selection_tool_moves_selected_content_and_the_frame_otherwise() {
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
    // With the content selected (the grabber, or a double-click), the pointer is a hand over it and
    // a drag anywhere on it moves it, as in InDesign (Dansky's Exclude Overlap tutorial, 7:13).
    let edge = spread_pt(&l, 115.0, 185.0);
    assert_eq!(t.cursor(&ctx(&d, &content, &c, &l), edge, Mods::default()), Cursor::Hand);
    assert!(t.pointer(&ctx(&d, &content, &c, &l), &PointerEvent::new(PointerKind::Down, edge.x, edge.y)).is_empty());
    let a = t.pointer(&ctx(&d, &content, &c, &l), &PointerEvent::new(PointerKind::Drag, edge.x + 30.0, edge.y));
    assert_eq!(a[0], Action::Begin("Move Content".into()));
    let p = preview_params(&a);
    assert_eq!((p["dx"].as_f64(), &p["content"]), (Some(30.0), &serde_json::json!(true)));
    t.pointer(&ctx(&d, &content, &c, &l), &PointerEvent::new(PointerKind::Up, edge.x + 30.0, edge.y));
    // Shift keeps it on one axis; the content stays selected.
    let shift = Mods { shift: true, ..Default::default() };
    t.pointer(&ctx(&d, &content, &c, &l), &PointerEvent::new(PointerKind::Down, edge.x, edge.y).with_mods(shift));
    let a = t.pointer(&ctx(&d, &content, &c, &l), &PointerEvent::new(PointerKind::Drag, edge.x + 30.0, edge.y + 4.0).with_mods(shift));
    let p = preview_params(&a);
    assert_eq!((p["dx"].as_f64(), p["dy"].as_f64(), &p["content"]), (Some(30.0), Some(0.0), &serde_json::json!(true)));
    assert_eq!(
        t.pointer(&ctx(&d, &content, &c, &l), &PointerEvent::new(PointerKind::Up, edge.x + 30.0, edge.y).with_mods(shift)),
        vec![Action::Commit]
    );
    // Off the content (the graphic overhangs the frame's sides only), a press selects as before.
    assert_eq!(t.cursor(&ctx(&d, &content, &c, &l), spread_pt(&l, 150.0, 250.0), Mods::default()), Cursor::Arrow);
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
    let frame = Selection::items(vec![f]);
    let content = Selection { content: true, ..Selection::items(vec![f]) };
    let set = |content: bool| vec![Action::Exec("selection.set".into(), serde_json::json!({"ids": [f.0], "content": content}))];
    // The canvas's presses in a row: the first selects the frame, the second its content.
    let a = t.pointer(&ctx(&d, &Selection::default(), &c, &l), &PointerEvent::new(PointerKind::Down, at.x, at.y));
    assert_eq!(a, vec![Action::Exec("selection.set".into(), serde_json::json!({"ids": [f.0], "content": false}))]);
    t.pointer(&ctx(&d, &frame, &c, &l), &PointerEvent::new(PointerKind::Up, at.x, at.y));
    assert_eq!(t.pointer(&ctx(&d, &frame, &c, &l), &PointerEvent::new(PointerKind::Down, at.x, at.y).with_clicks(2)), set(true));
    assert!(t.pointer(&ctx(&d, &content, &c, &l), &PointerEvent::new(PointerKind::Up, at.x, at.y)).is_empty());
    // With the content selected, the first press keeps it and the second selects the frame.
    assert!(t.pointer(&ctx(&d, &content, &c, &l), &PointerEvent::new(PointerKind::Down, at.x, at.y)).is_empty());
    t.pointer(&ctx(&d, &content, &c, &l), &PointerEvent::new(PointerKind::Up, at.x, at.y));
    assert_eq!(t.pointer(&ctx(&d, &content, &c, &l), &PointerEvent::new(PointerKind::Down, at.x, at.y).with_clicks(2)), set(false));
    // A double click sent as such (control channel) toggles too.
    let dbl = PointerEvent::new(PointerKind::DoubleClick, at.x, at.y);
    assert_eq!(t.pointer(&ctx(&d, &frame, &c, &l), &dbl), set(true));
    assert_eq!(t.pointer(&ctx(&d, &content, &c, &l), &dbl), set(false));
}

/// A 200 × 150 pt rectangle frame, selected.
fn live_corner_doc() -> (Document, ItemId) {
    let mut d = Document::new(&NewDocument::default());
    let id = add_rect(&mut d, Rect::new(100.0, 100.0, 300.0, 250.0));
    (d, id)
}

fn corner_preview(a: &[Action]) -> Vec<(String, f64)> {
    let p = match a {
        [Action::Preview(cmd, p)] | [Action::Exec(cmd, p)] if cmd == "object.cornerOptions" => p,
        other => panic!("{other:?}"),
    };
    p["corners"].as_array().unwrap().iter().map(|c| (c["shape"].as_str().unwrap().to_string(), c["size"].as_f64().unwrap())).collect()
}

fn diamonds(o: &[Overlay]) -> Vec<Point> {
    o.iter().filter_map(|o| if let Overlay::LiveCornerDiamond(p) = o { Some(*p) } else { None }).collect()
}

#[test]
fn live_corners_click_widget_then_drag_rounds_all_corners() {
    let (d, id) = live_corner_doc();
    let s = Selection { items: vec![id], ..Default::default() };
    let (c, l) = (Cache::new(), CanvasLayout::new(&d, false));
    let cx = ctx(&d, &s, &c, &l);
    let at = |x, y| spread_pt(&l, x, y);
    let mut t = create("selection");
    // The widget sits on the right edge, a little below the top-right corner.
    let w = at(300.0, 111.5);
    assert!(t.overlays(&cx).iter().any(|o| matches!(o, Overlay::LiveCornerWidget(p) if near(*p, w))));
    assert!(diamonds(&t.overlays(&cx)).is_empty());
    assert_eq!(t.pointer(&cx, &PointerEvent::new(PointerKind::Down, w.x, w.y)), vec![Action::Handled]);
    assert!(t.pointer(&cx, &PointerEvent::new(PointerKind::Up, w.x, w.y)).is_empty());
    // Without corner sizes the diamonds sit 12 px along the top and bottom edges.
    let want = [at(112.0, 100.0), at(288.0, 100.0), at(288.0, 250.0), at(112.0, 250.0)];
    let got = diamonds(&t.overlays(&cx));
    assert!(got.len() == 4 && got.iter().zip(want).all(|(g, w)| near(*g, w)), "{got:?}");
    assert!(!t.overlays(&cx).iter().any(|o| matches!(o, Overlay::LiveCornerWidget(_))));
    // Drag the top-left diamond 20 pt toward the centre.
    let a = t.pointer(&cx, &PointerEvent::new(PointerKind::Down, want[0].x, want[0].y));
    assert_eq!(a, vec![Action::Begin("Corner Options".into())]);
    let a = t.pointer(&cx, &PointerEvent::new(PointerKind::Drag, want[0].x + 20.0, want[0].y + 3.0));
    assert_eq!(corner_preview(&a), vec![("rounded".to_string(), 20.0); 4]);
    // The size stops at half the shorter side.
    let a = t.pointer(&cx, &PointerEvent::new(PointerKind::Drag, want[0].x + 500.0, want[0].y));
    assert_eq!(corner_preview(&a), vec![("rounded".to_string(), 75.0); 4]);
    assert_eq!(t.pointer(&cx, &PointerEvent::new(PointerKind::Up, want[0].x + 500.0, want[0].y)), vec![Action::Commit]);
}

#[test]
fn live_corners_shift_drag_and_alt_click_change_one_corner() {
    let (mut d, id) = live_corner_doc();
    d.item_mut(id).unwrap().corners = designcraft_geom::corners::CornerOptions::uniform(designcraft_geom::corners::CornerShape::Rounded, 30.0);
    let s = Selection { items: vec![id], ..Default::default() };
    let (c, l) = (Cache::new(), CanvasLayout::new(&d, false));
    let cx = ctx(&d, &s, &c, &l);
    let at = |x, y| spread_pt(&l, x, y);
    let mut t = create("selection");
    let w = at(300.0, 111.5);
    t.pointer(&cx, &PointerEvent::new(PointerKind::Down, w.x, w.y));
    t.pointer(&cx, &PointerEvent::new(PointerKind::Up, w.x, w.y));
    // With 30 pt corners the diamonds sit 30 pt along the edges.
    let tr = at(270.0, 100.0);
    assert!(diamonds(&t.overlays(&cx)).iter().any(|p| near(*p, tr)));
    let shift = Mods { shift: true, ..Default::default() };
    t.pointer(&cx, &PointerEvent::new(PointerKind::Down, tr.x, tr.y).with_mods(shift));
    let a = t.pointer(&cx, &PointerEvent::new(PointerKind::Drag, tr.x - 10.0, tr.y).with_mods(shift));
    let r = |n: f64| ("rounded".to_string(), n);
    assert_eq!(corner_preview(&a), vec![r(30.0), r(40.0), r(30.0), r(30.0)]);
    t.pointer(&cx, &PointerEvent::new(PointerKind::Up, tr.x - 10.0, tr.y));
    // Alt-click steps the bottom-right corner's shape; Shift+Alt steps every corner.
    let br = at(270.0, 250.0);
    let alt = Mods { alt: true, ..Default::default() };
    let a = t.pointer(&cx, &PointerEvent::new(PointerKind::Down, br.x, br.y).with_mods(alt));
    assert_eq!(corner_preview(&a), vec![r(30.0), r(30.0), ("inverseRounded".to_string(), 30.0), r(30.0)]);
    assert!(t.pointer(&cx, &PointerEvent::new(PointerKind::Drag, br.x - 30.0, br.y).with_mods(alt)).is_empty());
    assert!(t.pointer(&cx, &PointerEvent::new(PointerKind::Up, br.x - 30.0, br.y)).is_empty());
    let a = t.pointer(&cx, &PointerEvent::new(PointerKind::Down, br.x, br.y).with_mods(Mods { alt: true, shift: true, ..Default::default() }));
    assert_eq!(corner_preview(&a), vec![("inverseRounded".to_string(), 30.0); 4]);
}

#[test]
fn live_corners_end_on_escape_empty_click_and_other_selection() {
    let (mut d, id) = live_corner_doc();
    let other = add_rect(&mut d, Rect::new(400.0, 100.0, 500.0, 200.0));
    let s = Selection { items: vec![id], ..Default::default() };
    let (c, l) = (Cache::new(), CanvasLayout::new(&d, false));
    let cx = ctx(&d, &s, &c, &l);
    let w = spread_pt(&l, 300.0, 111.5);
    let mut t = create("selection");
    let enter = |t: &mut Box<dyn Tool>| {
        t.pointer(&cx, &PointerEvent::new(PointerKind::Down, w.x, w.y));
        t.pointer(&cx, &PointerEvent::new(PointerKind::Up, w.x, w.y));
        assert_eq!(diamonds(&t.overlays(&cx)).len(), 4);
    };
    // Escape leaves corner editing and keeps the selection; the next Escape deselects.
    enter(&mut t);
    assert_eq!(t.key(&cx, ToolKey::Escape, Mods::default()), vec![Action::Handled]);
    assert!(diamonds(&t.overlays(&cx)).is_empty());
    assert_eq!(t.key(&cx, ToolKey::Escape, Mods::default()), vec![Action::Exec("selection.set".into(), serde_json::json!({"ids": []}))]);
    // A click on empty canvas.
    enter(&mut t);
    let e = spread_pt(&l, 350.0, 400.0);
    t.pointer(&cx, &PointerEvent::new(PointerKind::Down, e.x, e.y));
    t.pointer(&cx, &PointerEvent::new(PointerKind::Up, e.x, e.y));
    assert!(diamonds(&t.overlays(&cx)).is_empty());
    // Another selection.
    enter(&mut t);
    let s2 = Selection { items: vec![other], ..Default::default() };
    let cx2 = ctx(&d, &s2, &c, &l);
    assert!(diamonds(&t.overlays(&cx2)).is_empty());
    t.pointer(&cx2, &PointerEvent::new(PointerKind::Move, 0.0, 0.0));
    assert!(diamonds(&t.overlays(&cx)).is_empty());
    // A frame too small on screen has no widget.
    let mut far = ctx(&d, &s, &c, &l);
    far.zoom = 0.1;
    assert!(!t.overlays(&far).iter().any(|o| matches!(o, Overlay::LiveCornerWidget(_))));
}

#[test]
fn live_corner_diamonds_follow_a_rotated_frame() {
    let mut d = Document::new(&NewDocument::default());
    let id = add_rotated(&mut d, Rect::new(100.0, 100.0, 300.0, 250.0), 30.0);
    let s = Selection { items: vec![id], ..Default::default() };
    let (c, l) = (Cache::new(), CanvasLayout::new(&d, false));
    let cx = ctx(&d, &s, &c, &l);
    let m = l.xf(SpreadRef::Doc(0)) * d.item(id).unwrap().xf;
    let mut t = create("selection");
    // The widget: 11.5 pt from the top-right corner along the turned right edge.
    let w = m * Point::new(300.0, 111.5);
    t.pointer(&cx, &PointerEvent::new(PointerKind::Down, w.x, w.y));
    t.pointer(&cx, &PointerEvent::new(PointerKind::Up, w.x, w.y));
    let tl = m * Point::new(112.0, 100.0);
    assert!(diamonds(&t.overlays(&cx)).iter().any(|p| near(*p, tl)), "{:?}", t.overlays(&cx));
    // Dragging along the turned top edge sets the size.
    t.pointer(&cx, &PointerEvent::new(PointerKind::Down, tl.x, tl.y));
    let to = m * Point::new(137.0, 100.0);
    let a = t.pointer(&cx, &PointerEvent::new(PointerKind::Drag, to.x, to.y));
    let got = corner_preview(&a);
    assert!(got.iter().all(|(shape, size)| shape == "rounded" && (size - 25.0).abs() < 1e-6), "{got:?}");
}

#[test]
fn pen_cursor_shows_add_delete_and_close() {
    let mut d = Document::new(&NewDocument::default());
    let lid = d.default_layer();
    let (fid, _) = d.add_text_frame(SpreadRef::Doc(0), Rect::new(100.0, 100.0, 200.0, 200.0), lid, "", ParaFormat::default()).unwrap();
    let (c, l) = (Cache::new(), CanvasLayout::new(&d, false));
    let selected = Selection::items(vec![fid]);
    let none = Selection::default();
    let off = l.xf(SpreadRef::Doc(0)).translation();
    let at = |x: f64, y: f64| Point::new(x + off.x, y + off.y);
    let mut t = create("pen");
    let m = Mods::default();

    let cx = ctx(&d, &selected, &c, &l);
    assert_eq!(t.cursor(&cx, at(200.0, 100.0), m), Cursor::PenDelete, "on a selected path's anchor");
    assert_eq!(t.cursor(&cx, at(150.0, 101.0), m), Cursor::PenAdd, "on a selected path's outline");
    assert_eq!(t.cursor(&cx, at(150.0, 150.0), m), Cursor::Pen, "inside, away from the outline");
    let click = |t: &mut Box<dyn Tool>, cx: &ToolContext, p: Point| {
        let down = t.pointer(cx, &PointerEvent::new(PointerKind::Down, p.x, p.y));
        let up = t.pointer(cx, &PointerEvent::new(PointerKind::Up, p.x, p.y));
        (down, up)
    };
    let (down, up) = click(&mut t, &cx, at(200.0, 100.0));
    assert_eq!(down, vec![Action::Exec("path.deleteAnchor".into(), serde_json::json!({"id": fid.0, "subpath": 0, "anchor": 1}))]);
    assert!(up.is_empty() && !t.busy(), "deleting starts no path");
    let (down, _) = click(&mut t, &cx, at(150.0, 101.0));
    assert!(matches!(&down[..], [Action::Exec(cmd, p)] if cmd == "path.addAnchor" && p["id"] == fid.0), "{down:?}");

    // Unselected paths are drawn over; the first anchor of the path being drawn closes it.
    let cx = ctx(&d, &none, &c, &l);
    assert_eq!(t.cursor(&cx, at(200.0, 100.0), m), Cursor::Pen, "unselected path");
    click(&mut t, &cx, at(300.0, 300.0));
    assert_eq!(t.cursor(&cx, at(300.0, 300.0), m), Cursor::Pen, "one anchor cannot close yet");
    click(&mut t, &cx, at(400.0, 300.0));
    assert_eq!(t.cursor(&cx, at(301.0, 301.0), m), Cursor::PenClose, "on the first anchor");
    assert_eq!(t.cursor(&cx, at(350.0, 350.0), m), Cursor::Pen, "elsewhere while drawing");
}

#[test]
fn anchor_tools_show_pen_state_cursors() {
    let d = Document::new(&NewDocument::default());
    let (s, c, l) = (Selection::default(), Cache::new(), CanvasLayout::new(&d, false));
    let cx = ctx(&d, &s, &c, &l);
    let p = Point::new(10.0, 10.0);
    let cursor = |id: &str| create(id).cursor(&cx, p, Mods::default());
    assert_eq!(cursor("addAnchor"), Cursor::PenAdd);
    assert_eq!(cursor("deleteAnchor"), Cursor::PenDelete);
    assert_eq!(cursor("convertDirection"), Cursor::PenConvert);
}

/// A text frame holding a 3×3 table of 80 pt columns, optionally rotated about its centre.
fn table_frame(degrees: f64) -> (Document, ItemId) {
    let mut d = Document::new(&NewDocument::default());
    let layer = d.default_layer();
    let (id, sid) = d.add_text_frame(SpreadRef::Doc(0), Rect::new(100.0, 100.0, 400.0, 400.0), layer, "", ParaFormat::default()).unwrap();
    let tid = d.alloc();
    d.story_mut(sid).unwrap().insert_table(0, designcraft_doc::Table::new(tid, 3, 3, 0, 0, 240.0));
    d.item_mut(id).unwrap().xf = designcraft_geom::Affine::rotate_about(degrees.to_radians(), Point::new(250.0, 250.0));
    (d, id)
}

#[test]
fn type_tool_drags_a_table_column_border() {
    for degrees in [0.0, 90.0] {
        let (d, id) = table_frame(degrees);
        let (s, c, l) = (Selection::default(), Cache::new(), CanvasLayout::new(&d, false));
        let cx = ctx(&d, &s, &c, &l);
        let it = d.item(id).unwrap();
        let sid = it.text_frame().unwrap().story;
        let cs = c.get(&d, sid, None);
        let cell = cs.frames[0].tables[0].cell(1, 0).unwrap();
        let xf = cx.item_canvas_xf(id).unwrap() * d.text_xf(it);
        let on_border = xf * Point::new(cell.rect.x1 + 1.0, (cell.rect.y0 + cell.rect.y1) / 2.0);
        let dragged = xf * Point::new(cell.rect.x1 + 21.0, (cell.rect.y0 + cell.rect.y1) / 2.0);
        let mut t = create("type");
        let expected = if degrees == 0.0 { Cursor::ResizeH } else { Cursor::ResizeV };
        assert_eq!(t.cursor(&cx, on_border, Mods::default()), expected);
        // A click with a 1 px wobble on the border changes nothing.
        assert!(t.pointer(&cx, &PointerEvent::new(PointerKind::Down, on_border.x, on_border.y)).is_empty());
        assert!(t.pointer(&cx, &PointerEvent::new(PointerKind::Drag, on_border.x + 1.0, on_border.y)).is_empty());
        assert!(t.pointer(&cx, &PointerEvent::new(PointerKind::Up, on_border.x + 1.0, on_border.y)).is_empty());
        // A press on the border starts a resize, not a text selection.
        assert!(t.pointer(&cx, &PointerEvent::new(PointerKind::Down, on_border.x, on_border.y)).is_empty());
        let a = t.pointer(&cx, &PointerEvent::new(PointerKind::Drag, dragged.x, dragged.y));
        assert_eq!(a[0], Action::Begin("Resize Column".into()));
        let Action::Preview(cmd, p) = &a[1] else { panic!("{a:?}") };
        assert_eq!(
            (cmd.as_str(), &p["axis"], &p["index"], &p["frame"]),
            ("table.resizeBorder", &serde_json::json!("column"), &serde_json::json!(0), &serde_json::json!(id.0))
        );
        assert!((p["delta"].as_f64().unwrap() - 20.0).abs() < 1e-6, "{p}");
        assert_eq!(t.pointer(&cx, &PointerEvent::new(PointerKind::Up, dragged.x, dragged.y)), vec![Action::Commit]);
        // Esc cancels a drag in progress.
        t.pointer(&cx, &PointerEvent::new(PointerKind::Down, on_border.x, on_border.y));
        t.pointer(&cx, &PointerEvent::new(PointerKind::Drag, dragged.x, dragged.y));
        assert_eq!(t.key(&cx, ToolKey::Escape, Mods::default()), vec![Action::Cancel]);
        assert!(t.pointer(&cx, &PointerEvent::new(PointerKind::Up, dragged.x, dragged.y)).is_empty());
        // Inside the cell the Type tool places the caret as usual.
        let inside = xf * Point::new(cell.rect.x0 + 20.0, (cell.rect.y0 + cell.rect.y1) / 2.0);
        let a = t.pointer(&cx, &PointerEvent::new(PointerKind::Down, inside.x, inside.y));
        assert!(matches!(&a[..], [Action::Exec(cmd, _)] if cmd == "text.placeCaret"), "{a:?}");
    }
}

#[test]
fn eyedropper_samples_a_placed_image_and_matches_other_objects() {
    let mut d = Document::new(&NewDocument::default());
    let lid = d.default_layer();
    let photo = ItemId(d.alloc());
    let mut it = Item::new(photo, lid, Shape::Rectangle, shapes::rectangle(Rect::new(100.0, 100.0, 200.0, 200.0)));
    it.content = designcraft_doc::Content::Graphic(designcraft_doc::Graphic {
        layer_options: None,
        asset: designcraft_doc::AssetId(1),
        size: (100.0, 100.0),
        xf: designcraft_geom::Affine::translate((0.0, 0.0)),
        clip: None,
        auto_fit: Default::default(),
        fit_align: 4,
        crop: [0.0; 4],
        wrap: Default::default(),
        fill: Default::default(),
    });
    d.assets.insert(
        designcraft_doc::AssetId(1),
        std::sync::Arc::new(designcraft_doc::Asset {
            id: designcraft_doc::AssetId(1),
            name: "photo.png".into(),
            mime: "image/png".into(),
            link: None,
            data: std::sync::Arc::new(vec![]),
            pixels: Some((100, 100)),
            page: 0,
            pdf_crop: Default::default(),
            pdf_box: None,
        }),
    );
    d.insert_item(SpreadRef::Doc(0), it, None).unwrap();
    // An unfilled text frame over part of the photo lets it show through.
    d.add_text_frame(SpreadRef::Doc(0), Rect::new(150.0, 150.0, 250.0, 250.0), lid, "x", ParaFormat::default()).unwrap();
    let swatch = add_rect(&mut d, Rect::new(300.0, 100.0, 350.0, 150.0));
    d.item_mut(swatch).unwrap().fill.swatch = "[Black]".into();
    let target = add_rect(&mut d, Rect::new(400.0, 100.0, 450.0, 150.0));
    // The photo's layer is locked: sampling it still works.
    d.layers.iter_mut().for_each(|l| l.locked = true);
    let s = Selection { items: vec![target], ..Default::default() };
    let (c, l) = (Cache::new(), CanvasLayout::new(&d, false));
    let off = l.xf(SpreadRef::Doc(0)).translation();
    let cx = ctx(&d, &s, &c, &l);
    let mut t = create("eyedropper");
    let a = t.pointer(&cx, &PointerEvent::new(PointerKind::Down, 120.0 + off.x, 130.0 + off.y));
    let sample = |x: f64, y: f64| Action::Exec("color.sample".into(), serde_json::json!({"at": [x, y], "spread": {"kind": "doc", "index": 0}}));
    assert_eq!(a, vec![sample(120.0, 130.0)]);
    let a = t.pointer(&cx, &PointerEvent::new(PointerKind::Down, 180.0 + off.x, 180.0 + off.y));
    assert_eq!(a, vec![sample(180.0, 180.0)], "through the text frame");
    // With the toolbox's Stroke box in front, the colour goes to the stroke (a line has no fill).
    let mut cx_stroke = ctx(&d, &s, &c, &l);
    cx_stroke.stroke_in_front = true;
    let a = t.pointer(&cx_stroke, &PointerEvent::new(PointerKind::Down, 120.0 + off.x, 130.0 + off.y));
    assert_eq!(
        a,
        vec![Action::Exec(
            "color.sample".into(),
            serde_json::json!({"at": [120.0, 130.0], "spread": {"kind": "doc", "index": 0}, "target": "stroke"})
        )]
    );
    // A filled object is matched as before (here on a locked layer: not reachable, so nothing).
    assert!(t.pointer(&cx, &PointerEvent::new(PointerKind::Down, 320.0 + off.x, 120.0 + off.y)).is_empty());
    let mut d2 = d.clone();
    d2.layers.iter_mut().for_each(|l| l.locked = false);
    let cx = ctx(&d2, &s, &c, &l);
    let a = t.pointer(&cx, &PointerEvent::new(PointerKind::Down, 320.0 + off.x, 120.0 + off.y));
    assert_eq!(a, vec![Action::Exec("object.matchAttributes".into(), serde_json::json!({"from": swatch.0}))]);
    // An image the pointer can reach names its frame, whose attributes apply where no pixel shows.
    let a = t.pointer(&cx, &PointerEvent::new(PointerKind::Down, 120.0 + off.x, 130.0 + off.y));
    let sampled = serde_json::json!({"at": [120.0, 130.0], "spread": {"kind": "doc", "index": 0}, "matchFrom": photo.0});
    assert_eq!(a, vec![Action::Exec("color.sample".into(), sampled)]);
    // Vector graphics and missing assets must retain the ordinary attribute-matching path.
    for mime in ["application/pdf", "application/postscript", "image/svg+xml", "application/octet-stream"] {
        std::sync::Arc::make_mut(d2.assets.get_mut(&designcraft_doc::AssetId(1)).unwrap()).mime = mime.into();
        let cx = ctx(&d2, &s, &c, &l);
        let a = t.pointer(&cx, &PointerEvent::new(PointerKind::Down, 120.0 + off.x, 130.0 + off.y));
        assert_eq!(a, vec![Action::Exec("object.matchAttributes".into(), serde_json::json!({"from": photo.0}))], "{mime}");
    }
    d2.assets.clear();
    let cx = ctx(&d2, &s, &c, &l);
    let a = t.pointer(&cx, &PointerEvent::new(PointerKind::Down, 120.0 + off.x, 130.0 + off.y));
    assert_eq!(a, vec![Action::Exec("object.matchAttributes".into(), serde_json::json!({"from": photo.0}))]);
}

#[test]
fn double_clicking_a_handle_fits_the_frame_to_its_content_along_that_side() {
    let mut d = Document::new(&NewDocument::default());
    let id = add_rect(&mut d, Rect::new(100.0, 100.0, 300.0, 200.0));
    let s = Selection { items: vec![id], ..Default::default() };
    let (c, l) = (Cache::new(), CanvasLayout::new(&d, false));
    let off = l.xf(SpreadRef::Doc(0)).translation();
    let cx = ctx(&d, &s, &c, &l);
    let mut t = create("selection");
    for ((x, y), axis) in [((300.0, 200.0), "both"), ((200.0, 200.0), "height"), ((300.0, 150.0), "width")] {
        let a = t.pointer(&cx, &PointerEvent::new(PointerKind::DoubleClick, x + off.x, y + off.y));
        assert_eq!(a, vec![Action::Exec("object.fit".into(), serde_json::json!({"mode": "fitFrameToContent", "axis": axis}))], "{axis}");
    }
}

#[test]
fn fitting_handle_axes_follow_spread_and_frame_rotation() {
    for spread_turn in 0..4 {
        for frame_turn in 0..4 {
            let mut d = Document::new(&NewDocument::default());
            let id = add_rect(&mut d, Rect::new(100.0, 100.0, 300.0, 200.0));
            d.item_mut(id).unwrap().xf = designcraft_geom::Affine::rotate(f64::from(frame_turn) * std::f64::consts::FRAC_PI_2);
            Arc::make_mut(&mut d.spreads[0]).pages[0].view_rotation = spread_turn;
            let s = Selection { items: vec![id], ..Default::default() };
            let (c, l) = (Cache::new(), CanvasLayout::new(&d, false));
            let cx = ctx(&d, &s, &c, &l);
            let b = cx.selection_bounds().unwrap();
            let mut t = create("selection");
            let swapped = (spread_turn + frame_turn) % 2 == 1;
            for (index, axis) in [(1, if swapped { "width" } else { "height" }), (3, if swapped { "height" } else { "width" }), (4, "both")] {
                let p = select::handles(b)[index];
                let a = t.pointer(&cx, &PointerEvent::new(PointerKind::DoubleClick, p.x, p.y));
                assert_eq!(
                    a,
                    vec![Action::Exec("object.fit".into(), serde_json::json!({"mode": "fitFrameToContent", "axis": axis}))],
                    "spread={spread_turn}, frame={frame_turn}, handle={index}"
                );
            }
        }
    }
}

#[test]
fn pen_continues_an_open_path_from_either_end_point() {
    let mut d = Document::new(&NewDocument::default());
    let lid = d.default_layer();
    let id = ItemId(d.alloc());
    let anchors = vec![
        designcraft_geom::Anchor::corner(Point::new(100.0, 100.0)),
        designcraft_geom::Anchor {
            p: Point::new(200.0, 100.0),
            h_in: Point::new(170.0, 80.0),
            h_out: Point::new(230.0, 120.0),
            kind: designcraft_geom::AnchorKind::Smooth,
        },
    ];
    let mut it = Item::new(id, lid, Shape::Path, designcraft_geom::PathData::single(designcraft_geom::SubPath::new(anchors, false)));
    it.stroke = Stroke::default();
    d.insert_item(SpreadRef::Doc(0), it, None).unwrap();
    let (s, c, l) = (Selection::default(), Cache::new(), CanvasLayout::new(&d, false));
    let cx = ctx(&d, &s, &c, &l);
    let off = l.xf(SpreadRef::Doc(0)).translation();
    let at = |x: f64, y: f64| (x + off.x, y + off.y);
    let ev = |k: PointerKind, (x, y): (f64, f64)| PointerEvent::new(k, x, y);
    let mut t = create("pen");
    // The cursor says the end point continues the path; elsewhere it's the plain pen.
    assert_eq!(t.cursor(&cx, Point::new(at(200.0, 100.0).0, at(200.0, 100.0).1), Mods::default()), Cursor::PenContinue);
    assert_eq!(t.cursor(&cx, Point::new(at(150.0, 300.0).0, at(150.0, 300.0).1), Mods::default()), Cursor::Pen);
    // Clicking the last anchor adds nothing yet; the next click appends to that path.
    assert!(t.pointer(&cx, &ev(PointerKind::Down, at(200.0, 100.0))).is_empty());
    assert!(t.pointer(&cx, &ev(PointerKind::Up, at(200.0, 100.0))).is_empty(), "a click on the end point changes nothing");
    t.pointer(&cx, &ev(PointerKind::Down, at(300.0, 200.0)));
    let a = t.pointer(&cx, &ev(PointerKind::Up, at(300.0, 200.0)));
    let [Action::Exec(cmd, p)] = a.as_slice() else { panic!("{a:?}") };
    assert_eq!((cmd.as_str(), p["id"].as_u64(), p["subpath"].as_u64()), ("path.appendAnchor", Some(id.0), Some(0)));
    assert_eq!((p["anchor"]["p"][0].as_f64(), p["anchor"]["p"][1].as_f64()), (Some(300.0), Some(200.0)));
    // Clicking the first anchor of the other end closes it.
    let a = t.pointer(&cx, &ev(PointerKind::Down, at(100.0, 100.0)));
    assert_eq!(a, vec![Action::Exec("path.close".into(), serde_json::json!({"id": id.0, "subpath": 0}))]);
    assert!(t.pointer(&cx, &ev(PointerKind::Up, at(100.0, 100.0))).is_empty());
    // From the first anchor the path keeps its direction (arrowheads stay on their ends, and a
    // press then Esc leaves nothing to undo): a drag there sets the first anchor's direction
    // lines, and new anchors go before it.
    let mut t = create("pen");
    assert!(t.pointer(&cx, &ev(PointerKind::Down, at(100.0, 100.0))).is_empty());
    t.pointer(&cx, &ev(PointerKind::Drag, at(80.0, 140.0)));
    let a = t.pointer(&cx, &ev(PointerKind::Up, at(80.0, 140.0)));
    assert_eq!(
        a,
        vec![Action::Exec(
            "path.setAnchor".into(),
            serde_json::json!({"id": id.0, "subpath": 0, "anchor": "first", "in": [80.0, 140.0], "out": [120.0, 60.0]})
        )]
    );
    t.pointer(&cx, &ev(PointerKind::Down, at(20.0, 200.0)));
    t.pointer(&cx, &ev(PointerKind::Drag, at(40.0, 220.0)));
    let a = t.pointer(&cx, &ev(PointerKind::Up, at(40.0, 220.0)));
    let [Action::Exec(cmd, p)] = a.as_slice() else { panic!("{a:?}") };
    assert_eq!((cmd.as_str(), p["at"].as_str(), p["subpath"].as_u64()), ("path.appendAnchor", Some("start"), Some(0)));
    assert_eq!(p["anchor"], serde_json::json!({"p": [20.0, 200.0], "in": [40.0, 220.0], "out": [0.0, 180.0]}));
    let mut t = create("pen");
    assert!(t.pointer(&cx, &ev(PointerKind::Down, at(100.0, 100.0))).is_empty());
    assert!(t.pointer(&cx, &ev(PointerKind::Up, at(100.0, 100.0))).is_empty());
    assert!(t.key(&cx, ToolKey::Escape, Mods::default()).iter().all(|a| !matches!(a, Action::Exec(c, _) if c.starts_with("path."))));
    // Closed paths and locked paths aren't continued.
    let mut d2 = d.clone();
    d2.item_mut(id).unwrap().locked = true;
    let cx = ctx(&d2, &s, &c, &l);
    let t = create("pen");
    assert_eq!(t.cursor(&cx, Point::new(at(200.0, 100.0).0, at(200.0, 100.0).1), Mods::default()), Cursor::Pen);
}

#[test]
fn pen_over_a_selected_open_paths_end_point_continues_it() {
    let mut d = Document::new(&NewDocument::default());
    let lid = d.default_layer();
    let id = ItemId(d.alloc());
    let anchors = vec![
        designcraft_geom::Anchor::corner(Point::new(100.0, 100.0)),
        designcraft_geom::Anchor::corner(Point::new(200.0, 100.0)),
        designcraft_geom::Anchor::corner(Point::new(200.0, 200.0)),
    ];
    let mut it = Item::new(id, lid, Shape::Path, designcraft_geom::PathData::single(designcraft_geom::SubPath::new(anchors, false)));
    it.stroke = Stroke::default();
    d.insert_item(SpreadRef::Doc(0), it, None).unwrap();
    let (s, c, l) = (Selection::items(vec![id]), Cache::new(), CanvasLayout::new(&d, false));
    let cx = ctx(&d, &s, &c, &l);
    let off = l.xf(SpreadRef::Doc(0)).translation();
    let at = |x: f64, y: f64| Point::new(x + off.x, y + off.y);
    let mut t = create("pen");
    // The end point wins over deleting the selected path's anchor; a middle anchor still deletes.
    assert_eq!(t.cursor(&cx, at(200.0, 200.0), Mods::default()), Cursor::PenContinue);
    assert_eq!(t.cursor(&cx, at(200.0, 100.0), Mods::default()), Cursor::PenDelete);
    let p = at(200.0, 200.0);
    assert!(t.pointer(&cx, &PointerEvent::new(PointerKind::Down, p.x, p.y)).is_empty(), "no anchor is deleted");
    assert!(t.busy(), "the path is being continued");
}

#[test]
fn the_selection_tool_drags_a_path_type_bracket_along_its_path() {
    let mut d = Document::new(&NewDocument::default());
    let lid = d.default_layer();
    let id = ItemId(d.alloc());
    let line = designcraft_geom::PathData::single(designcraft_geom::SubPath::new(
        vec![designcraft_geom::Anchor::corner(Point::new(100.0, 300.0)), designcraft_geom::Anchor::corner(Point::new(500.0, 300.0))],
        false,
    ));
    let mut it = Item::new(id, lid, Shape::GraphicLine, line);
    let sid = designcraft_doc::StoryId(d.alloc());
    let mut tf = designcraft_doc::TextFrame { story: sid, options: Default::default() };
    tf.options.path = Some(designcraft_doc::PathType { start: 40.0, end: Some(300.0), ..Default::default() });
    it.content = designcraft_doc::Content::Text(tf);
    let mut st = designcraft_doc::Story::with_text(sid, "On the line", Default::default());
    st.frames = vec![id];
    d.stories.insert(sid, Arc::new(st));
    d.insert_item(SpreadRef::Doc(0), it, None).unwrap();
    let (c, l) = (Cache::new(), CanvasLayout::new(&d, false));
    let off = l.xf(SpreadRef::Doc(0)).translation();
    let at = |x: f64, y: f64| Point::new(x + off.x, y + off.y);
    let ev = |k: PointerKind, p: Point| PointerEvent::new(k, p.x, p.y);
    // Not selected: no brackets.
    let none = Selection::default();
    assert!(create("selection").overlays(&ctx(&d, &none, &c, &l)).is_empty());
    let s = Selection { items: vec![id], ..Default::default() };
    let cx = ctx(&d, &s, &c, &l);
    let mut t = create("selection");
    // Both brackets drawn across the path at 140 and 400 (a line and a tick each).
    let o = t.overlays(&cx);
    let xs: Vec<f64> = o.iter().filter_map(|o| if let Overlay::Line { a, b, .. } = o { (a.x == b.x).then_some(a.x - off.x) } else { None }).collect();
    assert_eq!(xs, [140.0, 400.0], "{o:?}");
    assert_eq!(t.cursor(&cx, at(140.0, 300.0), Mods::default()), Cursor::PathBracket);
    assert_ne!(t.cursor(&cx, at(250.0, 300.0), Mods::default()), Cursor::PathBracket);
    // Drag the start bracket: its distance along the path follows the pointer, off the path too.
    assert_eq!(t.pointer(&cx, &ev(PointerKind::Down, at(140.0, 301.0))), vec![Action::Begin("Move Bracket".into())]);
    let a = t.pointer(&cx, &ev(PointerKind::Drag, at(220.0, 330.0)));
    let [Action::Preview(cmd, p)] = a.as_slice() else { panic!("{a:?}") };
    assert_eq!((cmd.as_str(), p["id"].as_u64()), ("type.pathOptions", Some(id.0)));
    assert!((p["start"].as_f64().unwrap() - 120.0).abs() < 1e-6, "{p}");
    assert_eq!(t.pointer(&cx, &ev(PointerKind::Up, at(220.0, 330.0))), vec![Action::Commit]);
    // The end bracket.
    t.pointer(&cx, &ev(PointerKind::Down, at(400.0, 300.0)));
    let a = t.pointer(&cx, &ev(PointerKind::Drag, at(450.0, 290.0)));
    let [Action::Preview(_, p)] = a.as_slice() else { panic!("{a:?}") };
    assert!((p["end"].as_f64().unwrap() - 350.0).abs() < 1e-6, "{p}");
}

#[test]
fn direct_selection_drags_the_end_anchors_of_type_on_a_path() {
    let mut d = Document::new(&NewDocument::default());
    let lid = d.default_layer();
    let id = ItemId(d.alloc());
    let line = designcraft_geom::PathData::single(designcraft_geom::SubPath::new(
        vec![designcraft_geom::Anchor::corner(Point::new(100.0, 300.0)), designcraft_geom::Anchor::corner(Point::new(500.0, 300.0))],
        false,
    ));
    let mut it = Item::new(id, lid, Shape::GraphicLine, line);
    let sid = designcraft_doc::StoryId(d.alloc());
    let mut tf = designcraft_doc::TextFrame { story: sid, options: Default::default() };
    // The default brackets: on the first and last anchors.
    tf.options.path = Some(designcraft_doc::PathType::default());
    it.content = designcraft_doc::Content::Text(tf);
    let mut st = designcraft_doc::Story::with_text(sid, "On the line", Default::default());
    st.frames = vec![id];
    d.stories.insert(sid, Arc::new(st));
    d.insert_item(SpreadRef::Doc(0), it, None).unwrap();
    let (c, l) = (Cache::new(), CanvasLayout::new(&d, false));
    let off = l.xf(SpreadRef::Doc(0)).translation();
    let s = Selection { items: vec![id], content: true, ..Default::default() };
    let cx = ctx(&d, &s, &c, &l);
    for x in [100.0, 500.0] {
        let p = Point::new(x + off.x, 300.0 + off.y);
        let mut direct = create("directSelection");
        assert_ne!(direct.cursor(&cx, p, Mods::default()), Cursor::PathBracket, "anchor at {x}");
        assert_eq!(direct.pointer(&cx, &PointerEvent::new(PointerKind::Down, p.x, p.y)), vec![Action::Begin("Move Anchor".into())]);
    }
}

#[test]
fn a_selected_graphics_own_handles_resize_it_and_leave_the_frame() {
    let mut d = Document::new(&NewDocument::default());
    let f = graphic_frame(&mut d);
    let (c, l) = (Cache::new(), CanvasLayout::new(&d, false));
    let content = Selection { content: true, ..Selection::items(vec![f]) };
    let set_params = |a: &[Action]| match a.last() {
        Some(Action::Preview(cmd, p)) if cmd == "transform.set" => p.clone(),
        other => panic!("{other:?}"),
    };
    for tool in ["selection", "directSelection"] {
        let mut t = create(tool);
        let cx = ctx(&d, &content, &c, &l);
        // The graphic's bottom-right corner, (250, 200), outside the frame's (200, 200).
        let corner = spread_pt(&l, 250.0, 200.0);
        assert_ne!(t.cursor(&cx, corner, Mods::default()), Cursor::Arrow, "{tool}");
        assert_eq!(t.pointer(&cx, &PointerEvent::new(PointerKind::Down, corner.x, corner.y)), vec![Action::Begin("Resize Content".into())], "{tool}");
        let to = spread_pt(&l, 300.0, 250.0);
        let a = t.pointer(&cx, &PointerEvent::new(PointerKind::Drag, to.x, to.y));
        let p = set_params(&a);
        assert_eq!(
            (&p["ids"], &p["content"], &p["bounds"]),
            (&serde_json::json!([f.0]), &serde_json::json!(true), &serde_json::json!([50.0, 100.0, 300.0, 250.0]))
        );
        assert!(t.overlays(&cx).contains(&Overlay::ContentGhost(f)));
        // Shift keeps the graphic's proportions (2:1).
        let a = t.pointer(&cx, &PointerEvent::new(PointerKind::Drag, to.x, to.y).with_mods(Mods { shift: true, ..Default::default() }));
        let b = set_params(&a)["bounds"].as_array().unwrap().iter().map(|v| v.as_f64().unwrap()).collect::<Vec<_>>();
        assert!(((b[2] - b[0]) / (b[3] - b[1]) - 2.0).abs() < 1e-6, "{b:?}");
        assert_eq!(t.pointer(&cx, &PointerEvent::new(PointerKind::Up, to.x, to.y)), vec![Action::Commit]);
    }
    // The frame's own handles don't resize the frame while its graphic is selected.
    let mut t = create("selection");
    let frame_corner = spread_pt(&l, 200.0, 200.0);
    let a = t.pointer(&ctx(&d, &content, &c, &l), &PointerEvent::new(PointerKind::Down, frame_corner.x, frame_corner.y));
    assert!(!a.contains(&Action::Begin("Resize".into())), "{a:?}");
}

#[test]
fn eyedropper_ignores_inactive_states_of_a_multi_state_object() {
    use designcraft_doc::{Asset, AssetId, Content, Graphic};
    let mut d = Document::new(&NewDocument::default());
    let lid = d.default_layer();
    d.assets.insert(
        AssetId(1),
        std::sync::Arc::new(Asset {
            id: AssetId(1),
            name: "photo.png".into(),
            mime: "image/png".into(),
            link: None,
            data: std::sync::Arc::new(vec![]),
            pixels: Some((100, 100)),
            ..Default::default()
        }),
    );
    let area = Rect::new(100.0, 100.0, 200.0, 200.0);
    let raster = {
        let mut it = Item::new(ItemId(d.alloc()), lid, Shape::Rectangle, shapes::rectangle(area));
        it.content = Content::Graphic(Graphic {
            layer_options: None,
            asset: AssetId(1),
            size: (100.0, 100.0),
            xf: designcraft_geom::Affine::translate((0.0, 0.0)),
            clip: None,
            auto_fit: Default::default(),
            fit_align: 4,
            crop: [0.0; 4],
            wrap: Default::default(),
            fill: Default::default(),
        });
        it
    };
    let mut vector = Item::new(ItemId(d.alloc()), lid, Shape::Rectangle, shapes::rectangle(area));
    vector.fill.swatch = "[Black]".into();
    let target = add_rect(&mut d, Rect::new(400.0, 100.0, 450.0, 150.0));
    let s = Selection { items: vec![target], ..Default::default() };
    let samples_image = |a: &[Action]| a.iter().any(|a| matches!(a, Action::Exec(c, _) if c == "color.sample"));
    // Children [vector, raster] with the raster on top: it samples only while it is the active state.
    for (active, samples) in [(1, true), (0, false)] {
        let mut group = Item::new(ItemId(d.alloc()), lid, Shape::Rectangle, shapes::rectangle(area));
        group.content = Content::Group { items: vec![std::sync::Arc::new(vector.clone()), std::sync::Arc::new(raster.clone())] };
        group.states = vec!["a".into(), "b".into()];
        group.active_state = active;
        let mut d2 = d.clone();
        d2.insert_item(SpreadRef::Doc(0), group, None).unwrap();
        let (c, l) = (Cache::new(), CanvasLayout::new(&d2, false));
        let off = l.xf(SpreadRef::Doc(0)).translation();
        let cx = ctx(&d2, &s, &c, &l);
        let a = create("eyedropper").pointer(&cx, &PointerEvent::new(PointerKind::Down, 120.0 + off.x, 130.0 + off.y));
        assert_eq!(samples_image(&a), samples, "active state {active}: {a:?}");
    }
}

#[test]
fn loaded_text_cursor_threads_into_a_frame_inside_a_group() {
    // `hit` stops at the group, so the loaded cursor showed NotAllowed over a text frame inside
    // one and a click did nothing. InDesign threads into it (gaps 27).
    let mut td = thread_doc();
    let lid = td.d.default_layer();
    let rect = Rect::new(600.0, 100.0, 700.0, 200.0);
    // The frame, then the same frame moved inside a group. Both transforms are the identity, so
    // the child's coordinates stay the spread's.
    let (inner, _) = td.d.add_text_frame(SpreadRef::Doc(0), rect, lid, "", ParaFormat::default()).unwrap();
    let frame_item = td.d.item(inner).cloned().unwrap();
    td.d.remove_item(inner).unwrap();
    let gid = ItemId(td.d.alloc());
    let mut group = Item::new(gid, lid, Shape::Group, shapes::rectangle(rect));
    group.content = designcraft_doc::item::Content::Group { items: vec![std::sync::Arc::new(frame_item)] };
    td.d.insert_item(SpreadRef::Doc(0), group, None).unwrap();
    td.l = CanvasLayout::new(&td.d, false);

    let mut t = create("selection");
    let cx = td.cx();
    click(&mut t, &cx, td.port(td.a, true), Mods::default());
    let p = td.at(650.0, 150.0);

    // Selection still reaches the group, which is what a plain click there should pick.
    assert_eq!(cx.hit(p).map(|(_, i)| i), Some(gid));
    // The loaded text cursor reaches the frame inside it.
    assert_eq!(cx.hit_text_frame(p).map(|(_, i)| i), Some(inner), "the frame in the group");
    assert_eq!(t.cursor(&cx, p, Mods::default()), Cursor::ThreadLink);
    // And the click threads to that frame, not to the group.
    assert_eq!(click(&mut t, &cx, p, Mods::default()), thread_exec(serde_json::json!({"from": td.a.0, "to": inner.0})));

    // A group with nothing threadable in it still says no.
    let mut td2 = thread_doc();
    let r2 = Rect::new(600.0, 300.0, 700.0, 400.0);
    let plain = ItemId(td2.d.alloc());
    let child = Item::new(plain, lid, Shape::Rectangle, shapes::rectangle(r2));
    let g2 = ItemId(td2.d.alloc());
    let mut group2 = Item::new(g2, lid, Shape::Group, shapes::rectangle(r2));
    group2.content = designcraft_doc::item::Content::Group { items: vec![std::sync::Arc::new(child)] };
    td2.d.insert_item(SpreadRef::Doc(0), group2, None).unwrap();
    td2.l = CanvasLayout::new(&td2.d, false);
    let cx2 = td2.cx();
    let mut t2 = create("selection");
    click(&mut t2, &cx2, td2.port(td2.a, true), Mods::default());
    let q = td2.at(650.0, 350.0);
    assert_eq!(cx2.hit_text_frame(q).map(|(_, i)| i), None, "no text frame in that group");
    assert_eq!(t2.cursor(&cx2, q, Mods::default()), Cursor::NotAllowed);
}
