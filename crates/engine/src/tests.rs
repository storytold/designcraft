use serde_json::json;

use super::*;

fn session() -> Session {
    let mut s = Session::new();
    s.execute("file.new", &json!({"pages": 2})).unwrap();
    s
}

#[test]
fn create_frame_type_and_undo() {
    let mut s = session();
    let r = s.execute("frame.create", &json!({"rect": [36, 36, 300, 200], "content": "text"})).unwrap();
    let sid = r["story"].as_u64().unwrap();
    s.execute("text.insert", &json!({"text": "Hello, world"})).unwrap();
    let st = s.execute("story.get", &json!({"story": sid})).unwrap();
    assert_eq!(st["text"], "Hello, world");
    s.execute("text.delete", &json!({})).unwrap();
    assert_eq!(s.execute("story.get", &json!({"story": sid})).unwrap()["text"], "Hello, worl");
    s.execute("edit.undo", &json!({})).unwrap();
    assert_eq!(s.execute("story.get", &json!({"story": sid})).unwrap()["text"], "Hello, world");
    s.execute("edit.undo", &json!({})).unwrap();
    s.execute("edit.undo", &json!({})).unwrap();
    assert!(s.doc().unwrap().doc.stories.is_empty());
}

#[test]
fn smart_quotes_and_formatting() {
    let mut s = session();
    s.execute("frame.create", &json!({"rect": [36, 36, 300, 200], "content": "text"})).unwrap();
    s.execute("text.insert", &json!({"text": "\"hi\" it's"})).unwrap();
    let t = s.execute("document.inspect", &json!({})).unwrap();
    let sid = t["stories"][0]["id"].as_u64().unwrap();
    assert_eq!(s.execute("story.get", &json!({"story": sid})).unwrap()["text"], "\u{201C}hi\u{201D} it\u{2019}s");
    s.execute("text.select", &json!({"story": sid, "anchor": 0, "focus": 4})).unwrap();
    s.execute("type.char", &json!({"attrs": {"size": 24, "fontStyle": "Bold"}})).unwrap();
    let a = s.execute("type.selectionAttrs", &json!({})).unwrap();
    assert_eq!(a["chars"]["size"], 24.0);
    s.execute("type.para", &json!({"attrs": {"align": "center"}})).unwrap();
    assert_eq!(s.execute("type.selectionAttrs", &json!({})).unwrap()["para"]["align"], "center");
}

#[test]
fn tool_gesture_creates_one_undo_step() {
    let mut s = session();
    s.set_tool("rectangle");
    let v = ViewInfo::at_zoom(1.0);
    use designcraft_tools::{PointerEvent, PointerKind};
    s.pointer(&PointerEvent::new(PointerKind::Down, 10.0, 10.0), v).unwrap();
    for i in 1..10 {
        s.pointer(&PointerEvent::new(PointerKind::Drag, 10.0 + i as f64 * 10.0, 10.0 + i as f64 * 5.0), v).unwrap();
    }
    s.pointer(&PointerEvent::new(PointerKind::Up, 100.0, 55.0), v).unwrap();
    let st = s.doc().unwrap();
    assert_eq!(st.history.undo.len(), 1);
    assert_eq!(st.doc.spreads[0].items.len(), 1);
    assert_eq!(st.doc.spreads[0].items[0].bounds(), designcraft_geom::Rect::new(10.0, 10.0, 100.0, 55.0));
    // Move with the selection tool.
    s.set_tool("selection");
    s.pointer(&PointerEvent::new(PointerKind::Down, 50.0, 30.0), v).unwrap();
    s.pointer(&PointerEvent::new(PointerKind::Drag, 70.0, 30.0), v).unwrap();
    s.pointer(&PointerEvent::new(PointerKind::Drag, 90.0, 40.0), v).unwrap();
    s.pointer(&PointerEvent::new(PointerKind::Up, 90.0, 40.0), v).unwrap();
    let st = s.doc().unwrap();
    assert_eq!(st.history.undo.len(), 2);
    assert_eq!(st.doc.spreads[0].items[0].bounds().x0, 50.0);
    // A move that ends near the margin snaps to it.
    s.pointer(&PointerEvent::new(PointerKind::Down, 60.0, 30.0), v).unwrap();
    s.pointer(&PointerEvent::new(PointerKind::Drag, 48.0, 30.0), v).unwrap();
    s.pointer(&PointerEvent::new(PointerKind::Up, 48.0, 30.0), v).unwrap();
    // The rectangle tool's 1 pt centered stroke sits half a point outside the path.
    // The visible edge lands on the 36 pt margin.
    let it = &s.doc().unwrap().doc.spreads[0].items[0];
    assert!((it.visible_bounds().x0 - 36.0).abs() < 1e-6, "visible {:?} path {:?}", it.visible_bounds(), it.bounds());
}

#[test]
fn shift_move_does_not_take_the_other_axis() {
    use designcraft_tools::{PointerEvent, PointerKind};
    let mut s = session();
    s.execute("frame.create", &json!({"rect": [80.0, 100.0, 140.0, 140.0], "content": "unassigned"})).unwrap();
    s.execute("guide.add", &json!({"orientation": "horizontal", "position": 102.0, "page": 0})).unwrap();
    s.set_tool("selection");
    let v = ViewInfo::at_zoom(1.0);
    let shift = designcraft_tools::Mods { shift: true, ..Default::default() };
    s.pointer(&PointerEvent::new(PointerKind::Down, 100.0, 120.0).with_mods(shift), v).unwrap();
    s.pointer(&PointerEvent::new(PointerKind::Drag, 160.0, 121.0).with_mods(shift), v).unwrap();
    s.pointer(&PointerEvent::new(PointerKind::Up, 160.0, 121.0).with_mods(shift), v).unwrap();
    let b = s.doc().unwrap().doc.spreads[0].items[0].bounds();
    assert!((b.y0 - 100.0).abs() < 1e-6, "locked axis jumped to {b:?}");
    assert!(b.x0 > 80.0);
}

#[test]
fn cmd_move_does_not_snap() {
    use designcraft_tools::{PointerEvent, PointerKind};
    let mut s = session();
    s.execute("frame.create", &json!({"rect": [80.0, 100.0, 140.0, 140.0], "content": "unassigned"})).unwrap();
    s.execute("guide.add", &json!({"orientation": "horizontal", "position": 102.0, "page": 0})).unwrap();
    s.set_tool("selection");
    let v = ViewInfo::at_zoom(1.0);
    let cmd = designcraft_tools::Mods { cmd: true, shift: false, ..Default::default() };
    s.pointer(&PointerEvent::new(PointerKind::Down, 110.0, 120.0).with_mods(cmd), v).unwrap();
    s.pointer(&PointerEvent::new(PointerKind::Drag, 160.0, 121.0).with_mods(cmd), v).unwrap();
    s.pointer(&PointerEvent::new(PointerKind::Up, 160.0, 121.0).with_mods(cmd), v).unwrap();
    let b = s.doc().unwrap().doc.spreads[0].items[0].bounds();
    assert!((b.y0 - 101.0).abs() < 1e-6, "cmd move snapped to {b:?}");
    assert!(b.x0 > 80.0);
}

#[test]
fn cmd_resize_still_snaps_to_a_guide() {
    use designcraft_tools::{PointerEvent, PointerKind};
    let mut s = session();
    s.execute("frame.create", &json!({"rect": [100.0, 100.0, 200.0, 180.0], "content": "unassigned"})).unwrap();
    s.execute("guide.add", &json!({"orientation": "vertical", "position": 250.0, "page": 0})).unwrap();
    s.set_tool("selection");
    let v = ViewInfo::at_zoom(1.0);
    let b0 = s.doc().unwrap().doc.spreads[0].items[0].bounds();
    let right = s.layout().to_canvas(designcraft_doc::SpreadRef::Doc(0), designcraft_geom::Point::new(b0.x1, b0.center().y));
    let cmd = designcraft_tools::Mods { cmd: true, ..Default::default() };
    s.pointer(&PointerEvent::new(PointerKind::Down, right.x, right.y).with_mods(cmd), v).unwrap();
    s.pointer(&PointerEvent::new(PointerKind::Drag, right.x + 47.0, right.y).with_mods(cmd), v).unwrap();
    s.pointer(&PointerEvent::new(PointerKind::Up, right.x + 47.0, right.y).with_mods(cmd), v).unwrap();
    let it = &s.doc().unwrap().doc.spreads[0].items[0];
    let b = it.bounds();
    assert!((b.x1 - 250.0).abs() < 1e-6, "cmd resize did not snap the moving edge: {b:?}");
    assert!((b.x0 - b0.x0).abs() < 1e-6 && (b.y0 - b0.y0).abs() < 1e-6 && (b.y1 - b0.y1).abs() < 1e-6, "{b:?}");
    let expect = (b.x1 - b.x0).abs() / b0.width() * (b.y1 - b.y0).abs() / b0.height();
    assert!((it.stroke.weight - expect.sqrt()).abs() < 1e-6, "content scale weight {} want {}", it.stroke.weight, expect.sqrt());
}

#[test]
fn shift_move_on_a_turned_spread_stays_on_the_line() {
    use designcraft_tools::{PointerEvent, PointerKind};
    let mut s = session();
    s.execute("frame.create", &json!({"rect": [80.0, 100.0, 140.0, 140.0], "content": "unassigned"})).unwrap();
    s.execute("layout.rotateSpreadView", &json!({"angle": 90})).unwrap();
    // Vertical guide sits on the spread axis that a screen-horizontal Shift lock must not offer.
    s.execute("guide.add", &json!({"orientation": "vertical", "position": 82.0, "page": 0})).unwrap();
    s.set_tool("selection");
    let v = ViewInfo::at_zoom(1.0);
    let b0 = s.doc().unwrap().doc.spreads[0].items[0].bounds();
    let c = s.layout().to_canvas(designcraft_doc::SpreadRef::Doc(0), b0.center());
    let shift = designcraft_tools::Mods { shift: true, ..Default::default() };
    s.pointer(&PointerEvent::new(PointerKind::Down, c.x, c.y).with_mods(shift), v).unwrap();
    s.pointer(&PointerEvent::new(PointerKind::Drag, c.x + 60.0, c.y + 1.0).with_mods(shift), v).unwrap();
    s.pointer(&PointerEvent::new(PointerKind::Up, c.x + 60.0, c.y + 1.0).with_mods(shift), v).unwrap();
    let b = s.doc().unwrap().doc.spreads[0].items[0].bounds();
    assert!((b.x0 - b0.x0).abs() < 1e-6, "locked screen axis jumped to {b:?}");
    assert!((b.y0 - b0.y0).abs() > 1.0, "free axis did not move: {b:?}");
}

#[test]
fn pages_and_layers() {
    let mut s = session();
    s.execute("layout.pages.insert", &json!({"count": 3})).unwrap();
    assert_eq!(s.doc().unwrap().doc.page_count(), 5);
    s.execute("layout.pages.delete", &json!({"pages": [0]})).unwrap();
    assert_eq!(s.doc().unwrap().doc.page_count(), 4);
    s.execute("layer.new", &json!({"name": "Text"})).unwrap();
    assert_eq!(s.doc().unwrap().doc.layers[0].name, "Text");
}

#[test]
fn sample_document_is_valid_and_composes() {
    let mut s = Session::new();
    s.execute("file.newSample", &json!({})).unwrap();
    let st = s.doc().unwrap();
    st.doc.check().unwrap();
    assert_eq!(st.doc.page_count(), 4);
    let i = s.execute("document.inspect", &json!({})).unwrap();
    assert!(i["stories"].as_array().unwrap().len() >= 8);
    // Save/load round trip.
    let bytes = cmd::file_bytes(&s.doc().unwrap().doc);
    let back = cmd::file_from(&bytes).unwrap();
    assert_eq!(back.page_count(), 4);
    assert_eq!(back.assets.len(), s.doc().unwrap().doc.assets.len());
    assert!(back.assets.values().all(|a| !a.data.is_empty()));
}

#[test]
fn every_command_has_metadata() {
    let mut ids = std::collections::HashSet::new();
    for c in command_specs() {
        assert!(ids.insert(c.id), "duplicate command {}", c.id);
        assert!(!c.label.is_empty() && !c.params.is_empty(), "{}", c.id);
    }
    assert!(command_specs().len() > 60);
}

#[test]
fn align_and_distribute() {
    let mut s = session();
    let mut ids = vec![];
    for (x, w) in [(10.0, 20.0), (100.0, 40.0), (300.0, 10.0)] {
        let r = s.execute("frame.create", &json!({"rect": [x, 50.0 + x / 10.0, x + w, 100.0], "content": "unassigned"})).unwrap();
        ids.push(r["id"].as_u64().unwrap());
    }
    s.execute("object.align", &json!({"ids": ids, "edge": "top"})).unwrap();
    let st = s.doc().unwrap();
    let tops: Vec<f64> = ids.iter().map(|i| st.doc.item(designcraft_doc::ItemId(*i)).unwrap().bounds().y0).collect();
    assert!(tops.iter().all(|t| (*t - 51.0).abs() < 1e-9), "{tops:?}");
    s.execute("object.distribute", &json!({"ids": ids, "by": "spacing"})).unwrap();
    let st = s.doc().unwrap();
    let b: Vec<designcraft_geom::Rect> = ids.iter().map(|i| st.doc.item(designcraft_doc::ItemId(*i)).unwrap().bounds()).collect();
    assert!(((b[1].x0 - b[0].x1) - (b[2].x0 - b[1].x1)).abs() < 1e-9);
    s.execute("object.align", &json!({"ids": [ids[0]], "edge": "left", "to": "margins"})).unwrap();
    assert_eq!(s.doc().unwrap().doc.item(designcraft_doc::ItemId(ids[0])).unwrap().bounds().x0, 36.0);
}

#[test]
fn pen_draws_and_direct_selection_edits() {
    use designcraft_tools::{PointerEvent, PointerKind};
    let mut s = session();
    let v = ViewInfo::at_zoom(1.0);
    s.set_tool("pen");
    let click = |s: &mut Session, x: f64, y: f64| {
        s.pointer(&PointerEvent::new(PointerKind::Down, x, y), v).unwrap();
        s.pointer(&PointerEvent::new(PointerKind::Up, x, y), v).unwrap();
    };
    click(&mut s, 100.0, 100.0);
    click(&mut s, 200.0, 100.0);
    // A smooth point (drag).
    s.pointer(&PointerEvent::new(PointerKind::Down, 200.0, 200.0), v).unwrap();
    s.pointer(&PointerEvent::new(PointerKind::Drag, 240.0, 200.0), v).unwrap();
    s.pointer(&PointerEvent::new(PointerKind::Up, 240.0, 200.0), v).unwrap();
    click(&mut s, 100.0, 100.0); // close
    let st = s.doc().unwrap();
    let it = &st.doc.spreads[0].items[0];
    assert_eq!(it.path.subpaths[0].anchors.len(), 3);
    assert!(it.path.subpaths[0].closed);
    assert!(it.path.subpaths[0].anchors[2].has_out());
    let id = it.id;
    // Direct Selection: drag the first anchor.
    s.set_tool("directSelection");
    s.execute("selection.set", &json!({"ids": [id.0]})).unwrap();
    s.pointer(&PointerEvent::new(PointerKind::Down, 100.0, 100.0), v).unwrap();
    s.pointer(&PointerEvent::new(PointerKind::Drag, 90.0, 80.0), v).unwrap();
    s.pointer(&PointerEvent::new(PointerKind::Up, 90.0, 80.0), v).unwrap();
    let a = s.doc().unwrap().doc.item(id).unwrap().path.subpaths[0].anchors[0].p;
    // The pen placed this anchor on the baseline at y 96. Dragging toward (90, 80)
    // proposes y 76, and the baseline at 72 is inside the zone, so the anchor lands at (90, 72).
    assert_eq!(a, designcraft_geom::Point::new(90.0, 72.0));
}

#[test]
fn rotate_scale_shear_and_eyedropper() {
    let mut s = session();
    let a = s.execute("frame.create", &json!({"rect": [100, 100, 200, 150], "content": "unassigned"})).unwrap()["id"].as_u64().unwrap();
    s.execute("object.fill", &json!({"swatch": "C=100 M=0 Y=0 K=0"})).unwrap();
    let b = s.execute("frame.create", &json!({"rect": [300, 100, 340, 140], "content": "unassigned"})).unwrap()["id"].as_u64().unwrap();
    s.execute("transform.rotate", &json!({"ids": [a], "angle": 90})).unwrap();
    let r = s.doc().unwrap().doc.item(designcraft_doc::ItemId(a)).unwrap().bounds();
    assert!((r.width() - 50.0).abs() < 1e-6 && (r.height() - 100.0).abs() < 1e-6, "{r:?}");
    s.execute("transform.scale", &json!({"ids": [b], "sx": 2.0})).unwrap();
    let r = s.doc().unwrap().doc.item(designcraft_doc::ItemId(b)).unwrap().bounds();
    assert!((r.width() - 80.0).abs() < 1e-6, "{r:?}");
    s.execute("selection.set", &json!({"ids": [b]})).unwrap();
    s.execute("object.matchAttributes", &json!({"from": a})).unwrap();
    assert_eq!(s.doc().unwrap().doc.item(designcraft_doc::ItemId(b)).unwrap().fill.swatch, "C=100 M=0 Y=0 K=0");
    s.execute("transform.shear", &json!({"ids": [b], "angle": 20})).unwrap();
}

#[test]
fn place_gun_click_drag_and_into_frame() {
    use designcraft_tools::{PointerEvent, PointerKind};
    let mut s = session();
    let png = designcraft_render::Rendered { width: 40, height: 20, pixels: vec![200; 40 * 20 * 4] }.to_png();
    let b64 = cmd::base64_encode(&png);
    s.execute("place.load", &json!({"base64": b64, "name": "a.png"})).unwrap();
    assert_eq!(s.tool_id(), "placeGun");
    let v = ViewInfo::at_zoom(1.0);
    s.pointer(&PointerEvent::new(PointerKind::Down, 100.0, 100.0), v).unwrap();
    s.pointer(&PointerEvent::new(PointerKind::Drag, 180.0, 300.0), v).unwrap();
    s.pointer(&PointerEvent::new(PointerKind::Up, 180.0, 300.0), v).unwrap();
    let st = s.doc().unwrap();
    let it = st.doc.spreads[0].items.last().unwrap();
    // Dragged 80×200 → fit proportionally: 80×40.
    assert_eq!(it.bounds(), designcraft_geom::Rect::new(100.0, 100.0, 180.0, 140.0));
    assert_eq!(s.tool_id(), "selection");
    // Into an empty frame.
    let f = s.execute("frame.create", &json!({"rect": [300, 300, 400, 400], "content": "graphic"})).unwrap()["id"].as_u64().unwrap();
    s.execute("place.load", &json!({"base64": b64})).unwrap();
    s.execute("place.drop", &json!({"frame": f})).unwrap();
    assert!(matches!(s.doc().unwrap().doc.item(designcraft_doc::ItemId(f)).unwrap().content, designcraft_doc::Content::Graphic(_)));
}

#[test]
fn preflight_reports_overset_and_missing_fonts() {
    let mut s = session();
    s.execute("frame.create", &json!({"rect": [36, 36, 100, 50], "content": "text", "text": "This text will not fit in such a tiny frame at all."}))
        .unwrap();
    s.execute("text.select", &json!({"story": s.doc().unwrap().doc.stories.keys().next().unwrap().0, "anchor": 0, "focus": 4})).unwrap();
    s.execute("type.char", &json!({"attrs": {"fontFamily": "Nonexistent Sans"}})).unwrap();
    let r = s.execute("preflight.run", &json!({})).unwrap();
    let kinds: Vec<&str> = r["issues"].as_array().unwrap().iter().map(|i| i["kind"].as_str().unwrap()).collect();
    assert!(kinds.contains(&"overset") && kinds.contains(&"missingFont"), "{kinds:?}");
    assert_eq!(r["errors"], 2);
}

#[test]
fn hyperlinks_and_bookmarks_reach_the_pdf() {
    let mut s = Session::new();
    s.execute("file.newSample", &json!({})).unwrap();
    s.execute("selection.set", &json!({"ids": [39]})).unwrap();
    s.execute("hyperlink.create", &json!({"url": "https://example.com/designcraft"})).unwrap();
    let d = s.execute("document.inspect", &json!({})).unwrap();
    let sid = d["stories"][0]["id"].as_u64().unwrap();
    s.execute("text.select", &json!({"story": sid, "anchor": 0, "focus": 3})).unwrap();
    s.execute("hyperlink.create", &json!({"page": 3})).unwrap();
    s.execute("bookmark.add", &json!({"name": "Cover", "page": 1})).unwrap();
    s.execute("bookmark.add", &json!({"name": "Feature", "page": 2})).unwrap();
    assert_eq!(s.execute("hyperlink.list", &json!({})).unwrap().as_array().unwrap().len(), 2);
    let r = s.execute("file.exportPdf", &json!({})).unwrap();
    let bytes = cmd::base64_decode(r["base64"].as_str().unwrap());
    let pdf = hayro_syntax::Pdf::new(bytes).expect("valid pdf");
    assert_eq!(pdf.pages().len(), 4);
    // Round trip through the native format keeps them.
    let back = cmd::file_from(&cmd::file_bytes(&s.doc().unwrap().doc)).unwrap();
    assert_eq!(back.hyperlinks.len(), 2);
    assert_eq!(back.bookmarks.len(), 2);
}

#[test]
fn serialized_document_opens_again() {
    // The web app saves (downloads) what `file.serialize` returns: it must be the file's exact bytes.
    let mut s = session();
    s.execute("frame.create", &json!({"rect": [36, 36, 300, 200], "content": "text", "text": "Saved on the web"})).unwrap();
    let r = s.execute("file.serialize", &json!({})).unwrap();
    let bytes = cmd::base64_decode(r["base64"].as_str().expect("base64"));
    assert_eq!(bytes, cmd::to_bytes(&s.doc().unwrap().doc));
    s.execute("file.openBytes", &json!({"name": "copy", "base64": r["base64"]})).unwrap();
    let d = s.execute("document.inspect", &json!({})).unwrap();
    assert_eq!(d["stories"][0]["preview"], "Saved on the web");
}

#[test]
fn active_layer_survives_undoing_new_layer() {
    let mut s = session();
    let layer_of = |s: &mut Session, id: u64| {
        let d = s.execute("document.inspect", &json!({})).unwrap();
        let items = d["spreads"].as_array().unwrap().iter().flat_map(|sp| sp["items"].as_array().unwrap().clone()).collect::<Vec<_>>();
        let it = items.into_iter().find(|i| i["id"] == id).unwrap();
        let layers: Vec<Value> = d["layers"].as_array().unwrap().iter().map(|l| l["id"].clone()).collect();
        (it["layer"].clone(), layers)
    };
    // New Layer, then Undo: the new layer is gone, so it can't stay the active one.
    s.execute("layer.new", &json!({})).unwrap();
    s.execute("edit.undo", &json!({})).unwrap();
    let f = s.execute("frame.create", &json!({"rect": [36, 36, 300, 200], "content": "text", "text": "Visible"})).unwrap();
    let (layer, layers) = layer_of(&mut s, f["id"].as_u64().unwrap());
    assert!(layers.contains(&layer), "frame on layer {layer} not in {layers:?}");
    // Delete Unused Layers removing the (empty) active layer.
    s.execute("layer.new", &json!({})).unwrap();
    s.execute("layer.deleteUnused", &json!({})).unwrap();
    let f = s.execute("frame.create", &json!({"rect": [36, 236, 300, 400], "content": "text", "text": "Also visible"})).unwrap();
    let (layer, layers) = layer_of(&mut s, f["id"].as_u64().unwrap());
    assert!(layers.contains(&layer), "frame on layer {layer} not in {layers:?}");
}

#[test]
fn step_and_repeat_grid() {
    let mut s = session();
    s.execute("frame.create", &json!({"rect": [36, 36, 66, 66], "content": "unassigned"})).unwrap();
    let r = s.execute("edit.stepAndRepeat", &json!({"rows": 3, "columns": 4, "dx": 40, "dy": 40})).unwrap();
    assert_eq!(r["created"], 11);
    assert_eq!(s.doc().unwrap().doc.spreads[0].items.len(), 12);
    let last = s.doc().unwrap().doc.spreads[0].items.last().unwrap().bounds();
    assert_eq!(last.x0, 36.0 + 3.0 * 40.0);
    assert_eq!(last.y0, 36.0 + 2.0 * 40.0);
}

/// rows × columns once overflowed: a panic in debug builds, and in release builds a product that
/// wrapped to 0 passed the 1000-copy limit and started a grid of 2⁶⁴ copies.
#[test]
fn step_and_repeat_with_a_huge_grid_makes_no_grid() {
    let mut s = session();
    s.execute("frame.create", &json!({"rect": [36, 36, 66, 66], "content": "unassigned"})).unwrap();
    let items = |s: &Session| s.doc().unwrap().doc.spreads[0].items.len();
    let before = items(&s);
    // Counts past the limit are an error, and nothing is copied.
    assert!(s.execute("edit.stepAndRepeat", &json!({"rows": 4_294_967_296_u64, "columns": 4_294_967_296_u64})).is_err());
    assert_eq!(items(&s), before);
}

/// The column count is the caller's and sizes a list every time the page's columns are laid out.
#[test]
fn new_document_caps_the_column_count() {
    let mut s = Session::new();
    s.execute("file.new", &json!({"columns": 4_000_000_000_u64})).unwrap();
    assert_eq!(s.doc().unwrap().doc.spreads[0].pages[0].columns.count, 216);
    // Past u32 the count wrapped (2³² + 3 became 3).
    s.execute("file.new", &json!({"columns": 4_294_967_299_u64})).unwrap();
    assert_eq!(s.doc().unwrap().doc.spreads[0].pages[0].columns.count, 216);
}

#[test]
fn snippets_roundtrip_between_documents() {
    let mut s = Session::new();
    s.execute("file.newSample", &json!({})).unwrap();
    s.execute("selection.set", &json!({"ids": [39]})).unwrap();
    let snip = s.execute("snippet.export", &json!({})).unwrap();
    s.execute("file.new", &json!({})).unwrap();
    let r = s.execute("snippet.place", &json!({"base64": snip["base64"], "x": 36.0, "y": 36.0})).unwrap();
    let id = designcraft_doc::ItemId(r["ids"][0].as_u64().unwrap());
    let st = s.doc().unwrap();
    let it = st.doc.item(id).unwrap();
    assert_eq!(it.bounds().x0, 36.0);
    let sid = it.text_frame().unwrap().story;
    assert!(st.doc.story(sid).unwrap().text.starts_with("Every page begins"));
    assert!(st.doc.styles.para("Body").is_some());
    st.doc.check().unwrap();
}

#[test]
fn object_styles_create_and_apply() {
    let mut s = session();
    let a = s.execute("frame.create", &json!({"rect": [36, 36, 136, 136], "content": "unassigned"})).unwrap()["id"].as_u64().unwrap();
    s.execute("object.fill", &json!({"swatch": "C=0 M=100 Y=0 K=0"})).unwrap();
    s.execute("object.stroke", &json!({"weight": 4.0})).unwrap();
    s.execute("style.object.create", &json!({"name": "Magenta Box"})).unwrap();
    let b = s.execute("frame.create", &json!({"rect": [200, 36, 300, 136], "content": "unassigned"})).unwrap()["id"].as_u64().unwrap();
    s.execute("style.object.apply", &json!({"name": "Magenta Box"})).unwrap();
    let d = &s.doc().unwrap().doc;
    let (ia, ib) = (d.item(designcraft_doc::ItemId(a)).unwrap(), d.item(designcraft_doc::ItemId(b)).unwrap());
    assert_eq!(ia.fill, ib.fill);
    assert_eq!(ib.stroke.weight, 4.0);
    assert_eq!(ib.object_style, "Magenta Box");
}

/// Unicode text per page, through hayro's interpreter (ToUnicode / ActualText).
fn pdf_text(bytes: &[u8]) -> Vec<String> {
    use hayro_interpret::font::Glyph;
    use hayro_interpret::{
        BlendMode, ClipPath, Context, Device, GlyphDrawMode, Image, InterpreterCache, InterpreterSettings, Paint, PathDrawMode, SoftMask,
        interpret_page,
    };
    struct Ex(String);
    impl Device<'_> for Ex {
        fn set_soft_mask(&mut self, _: Option<SoftMask<'_>>) {}
        fn set_blend_mode(&mut self, _: BlendMode) {}
        fn draw_path(&mut self, _: &kurbo::BezPath, _: kurbo::Affine, _: &Paint<'_>, _: &PathDrawMode) {}
        fn push_clip_path(&mut self, _: &ClipPath) {}
        fn push_transparency_group(&mut self, _: f32, _: Option<SoftMask<'_>>, _: BlendMode) {}
        fn draw_glyph(&mut self, g: &Glyph<'_>, _: kurbo::Affine, _: kurbo::Affine, _: &Paint<'_>, _: &GlyphDrawMode) {
            match g.as_unicode() {
                Some(hayro_cmap::BfString::Char(c)) => self.0.push(c),
                Some(hayro_cmap::BfString::String(s)) => self.0.push_str(&s),
                None => self.0.push('\u{FFFD}'),
            }
        }
        fn draw_image(&mut self, _: Image<'_, '_>, _: kurbo::Affine) {}
        fn pop_clip_path(&mut self) {}
        fn pop_transparency_group(&mut self) {}
    }
    let pdf = hayro_syntax::Pdf::new(bytes.to_vec()).expect("valid PDF");
    let cache = InterpreterCache::new();
    pdf.pages()
        .iter()
        .map(|page| {
            let mut ctx =
                Context::new(kurbo::Affine::IDENTITY, kurbo::Rect::new(0.0, 0.0, 1.0, 1.0), &cache, pdf.xref(), InterpreterSettings::default());
            let mut ex = Ex(String::new());
            interpret_page(page, &mut ctx, &mut ex);
            ex.0
        })
        .collect()
}

#[test]
fn export_pdf_of_the_sample() {
    let mut s = Session::new();
    s.execute("file.newSample", &json!({})).unwrap();
    let r = s.execute("file.exportPdf", &json!({"bleed": true, "marks": true})).unwrap();
    assert_eq!(r["pages"], 4);
    let bytes = cmd::base64_decode(r["base64"].as_str().unwrap());
    assert!(bytes.starts_with(b"%PDF-"));
    let pdf = hayro_syntax::Pdf::new(bytes.clone()).expect("parse");
    assert_eq!(pdf.pages().len(), 4);
    let raw = String::from_utf8_lossy(&bytes);
    assert!(raw.contains("/FontFile2") || raw.contains("/FontFile3"), "fonts embedded");
    let text = pdf_text(&bytes);
    let all: String = text.join("\n");
    assert!(all.contains("Notes on the Grid"), "{all}");
    // Page range + spreads + file output.
    let dir = std::env::temp_dir().join(format!("dc-pdf-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("out.pdf");
    let r = s.execute("file.exportPdf", &json!({"path": path.to_string_lossy(), "pages": "2-3", "spreads": true})).unwrap();
    assert_eq!(r["pages"], 1, "pages 2–3 are one spread");
    assert!(std::fs::read(&path).unwrap().starts_with(b"%PDF-"));
    std::fs::remove_dir_all(&dir).ok();
    assert!(s.execute("file.exportPdf", &json!({"pages": "9"})).is_err());
    assert!(s.execute("file.exportPdf", &json!({"standard": "bogus"})).is_err());
}

#[test]
fn object_set_flags_and_wrap_invert() {
    let mut s = session();
    let r = s.execute("frame.create", &json!({"rect": [36, 36, 300, 200]})).unwrap();
    let id = r["id"].as_u64().unwrap();
    let item = |s: &Session| s.doc().unwrap().doc.item(designcraft_doc::ItemId(id)).cloned().unwrap();
    s.execute("object.setFlags", &json!({"ids": [id], "hidden": true})).unwrap();
    assert!(item(&s).hidden);
    s.execute("object.setFlags", &json!({"ids": [id], "hidden": false, "locked": true})).unwrap();
    assert!(!item(&s).hidden && item(&s).locked);
    s.execute("object.setFlags", &json!({"ids": [id], "locked": false})).unwrap();
    assert!(!item(&s).locked);
    s.execute("selection.set", &json!({"ids": [id]})).unwrap();
    s.execute("object.textWrap", &json!({"mode": "boundingBox", "invert": true})).unwrap();
    assert!(item(&s).wrap.invert);
}

#[test]
fn gridify_while_drawing_frames() {
    use designcraft_tools::{Mods, PointerEvent, PointerKind, ToolKey};
    let mut s = session();
    let n0 = s.doc().unwrap().doc.spreads[0].items.len();
    s.set_tool("rectangleFrame");
    let v = ViewInfo::at_zoom(1.0);
    s.pointer(&PointerEvent::new(PointerKind::Down, 100.0, 100.0), v).unwrap();
    s.pointer(&PointerEvent::new(PointerKind::Drag, 312.0, 212.0), v).unwrap();
    for k in [ToolKey::Right, ToolKey::Right, ToolKey::Up, ToolKey::Left] {
        assert!(s.tool_key(k, Mods::default(), v).unwrap(), "arrow keys gridify while dragging");
    }
    s.pointer(&PointerEvent::new(PointerKind::Up, 312.0, 212.0), v).unwrap();
    let d = &s.doc().unwrap().doc;
    let new: Vec<_> = d.spreads[0].items[n0..].iter().map(|i| i.bounds()).collect();
    assert_eq!(new.len(), 4, "2 columns × 2 rows");
    assert!((new[0].width() - 100.0).abs() < 1e-6 && (new[1].x0 - 212.0).abs() < 1e-6, "12 pt gutters: {new:?}");
    s.execute("edit.undo", &json!({})).unwrap();
    assert_eq!(s.doc().unwrap().doc.spreads[0].items.len(), n0, "one undo step");
}

#[test]
fn gap_tool_drag_moves_the_gap() {
    use designcraft_tools::{PointerEvent, PointerKind};
    let mut s = session();
    let a = s.execute("frame.create", &json!({"rect": [100, 100, 200, 300]})).unwrap()["id"].as_u64().unwrap();
    s.set_tool("gap");
    let v = ViewInfo::at_zoom(1.0);
    s.pointer(&PointerEvent::new(PointerKind::Down, 250.0, 200.0), v).unwrap();
    s.pointer(&PointerEvent::new(PointerKind::Drag, 270.0, 205.0), v).unwrap();
    s.pointer(&PointerEvent::new(PointerKind::Up, 270.0, 205.0), v).unwrap();
    // The gap between the frame and the page's right edge: the frame's right side moved 20.
    let b = s.doc().unwrap().doc.item(designcraft_doc::ItemId(a)).unwrap().bounds();
    assert_eq!(b.x1, 220.0, "{b:?}");
    assert_eq!(s.doc().unwrap().history.undo.len(), 2, "create + one gap move");
}

/// Parameters reach commands from the control channel, MCP and scripts as any JSON value.
/// Commands that add a key to their parameters (`q["fixedLayout"] = …`) panicked on an array.
#[test]
fn commands_take_non_object_params_without_panicking() {
    let mut s = Session::new();
    s.execute("file.new", &json!({})).unwrap();
    for p in [json!([1, 2]), json!("text"), json!(3), Value::Null] {
        let r = s.execute("file.exportFixedEpub", &p);
        assert!(r.is_ok(), "{p}: {r:?}");
    }
}

#[test]
fn direct_selection_drag_and_arrow_keys_move_placed_content() {
    use designcraft_tools::{Mods, PointerEvent, PointerKind, SnapView, ToolKey};
    let mut s = session();
    let png = designcraft_render::Rendered { width: 40, height: 20, pixels: vec![200; 40 * 20 * 4] }.to_png();
    let f = s.execute("frame.create", &json!({"rect": [100, 100, 300, 300], "content": "graphic"})).unwrap()["id"].as_u64().unwrap();
    s.execute("place.load", &json!({"base64": cmd::base64_encode(&png)})).unwrap();
    s.execute("place.drop", &json!({"frame": f})).unwrap();
    let xfs = |s: &Session| {
        let it = s.doc().unwrap().doc.item(designcraft_doc::ItemId(f)).unwrap().clone();
        (it.xf, it.graphic().unwrap().xf.translation())
    };
    let (frame0, g0) = xfs(&s);
    let v = ViewInfo { snap: SnapView::OFF, ..ViewInfo::at_zoom(1.0) };
    let at = s.layout().to_canvas(designcraft_doc::SpreadRef::Doc(0), designcraft_geom::Point::new(150.0, 150.0));
    s.set_tool("directSelection");
    s.pointer(&PointerEvent::new(PointerKind::Down, at.x, at.y), v).unwrap();
    s.pointer(&PointerEvent::new(PointerKind::Drag, at.x + 20.0, at.y + 10.0), v).unwrap();
    s.pointer(&PointerEvent::new(PointerKind::Drag, at.x + 30.0, at.y + 10.0), v).unwrap();
    s.pointer(&PointerEvent::new(PointerKind::Up, at.x + 30.0, at.y + 10.0), v).unwrap();
    assert!(s.doc().unwrap().selection.content);
    let (frame1, g1) = xfs(&s);
    assert_eq!(frame1, frame0, "the frame stays");
    assert_eq!(g1 - g0, designcraft_geom::Vec2::new(30.0, 10.0));
    s.execute("edit.undo", &json!({})).unwrap();
    assert_eq!(xfs(&s).1, g0, "one undo step");
    // Arrow keys nudge the selected content; Shift nudges ten times as far.
    s.set_tool("selection");
    s.tool_key(ToolKey::Right, Mods::default(), v).unwrap();
    s.tool_key(ToolKey::Down, Mods { shift: true, ..Default::default() }, v).unwrap();
    let (frame2, g2) = xfs(&s);
    assert_eq!(frame2, frame0);
    assert_eq!(g2 - g0, designcraft_geom::Vec2::new(1.0, 10.0));
}
