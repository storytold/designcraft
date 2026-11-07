//! Typing a new position for a selected placed image moves the frame and its content together.
use designcraft_engine::Session;
use designcraft_engine::cmd::base64_encode;
use designcraft_geom::Rect;
use designcraft_render::Rendered;
use serde_json::json;

/// The frame's top-left and the placed graphic's top-left on the spread.
fn corners(s: &Session, f: u64) -> ((f64, f64), (f64, f64)) {
    let d = &s.doc().unwrap().doc;
    let id = designcraft_doc::ItemId(f);
    let loc = d.find(id).unwrap();
    let it = d.item(id).unwrap();
    let g = it.graphic().unwrap();
    let b = (d.parent_xf(&loc) * it.xf * g.xf).transform_rect_bbox(Rect::new(0.0, 0.0, g.size.0, g.size.1));
    let fb = it.bounds();
    ((fb.x0, fb.y0), (b.x0, b.y0))
}

#[test]
fn moving_a_frame_through_the_position_fields_moves_its_image() {
    let mut s = Session::new();
    s.execute("file.new", &json!({})).unwrap();
    let f = s.execute("frame.create", &json!({"rect": [100, 100, 300, 300], "content": "graphic"})).unwrap()["id"].as_u64().unwrap();
    let png = Rendered { width: 200, height: 100, pixels: vec![200u8; 200 * 100 * 4] }.to_png();
    s.execute("file.place", &json!({"base64": base64_encode(&png), "name": "a.png", "frame": f})).unwrap();
    s.execute("selection.set", &json!({"ids": [f]})).unwrap();
    let (frame0, graphic0) = corners(&s, f);

    s.execute("transform.set", &json!({"x": 150, "y": 170, "ref": 0})).unwrap();

    let (frame1, graphic1) = corners(&s, f);
    assert!((frame1.0 - 150.0).abs() < 1e-6 && (frame1.1 - 170.0).abs() < 1e-6, "the frame moved to the typed position: {frame1:?}");
    let (fdx, fdy) = (frame1.0 - frame0.0, frame1.1 - frame0.1);
    assert!(fdx.abs() > 1.0 && fdy.abs() > 1.0, "the frame moved: {frame0:?} -> {frame1:?}");
    assert!(
        (graphic1.0 - graphic0.0 - fdx).abs() < 1e-6 && (graphic1.1 - graphic0.1 - fdy).abs() < 1e-6,
        "the image stayed behind: {graphic0:?} -> {graphic1:?} while the frame went {frame0:?} -> {frame1:?}"
    );

    // Undo puts both back.
    s.execute("edit.undo", &json!({})).unwrap();
    let (frame2, graphic2) = corners(&s, f);
    assert!((frame2.0 - frame0.0).abs() < 1e-6 && (graphic2.0 - graphic0.0).abs() < 1e-6, "{frame2:?} {graphic2:?}");
}
