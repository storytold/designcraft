use designcraft_doc::arrow::apply;
use designcraft_doc::geom::BezPath;
use designcraft_doc::geom::kurbo::Shape;
use designcraft_doc::{Arrowhead, Stroke};

#[test]
fn start_head_follows_tangent_when_start_handle_is_missing() {
    let mut bp = BezPath::new();
    bp.move_to((0.0, 0.0));
    bp.curve_to((0.0, 0.0), (100.0, 0.0), (100.0, 100.0));
    let st = Stroke { weight: 1.0, start: Arrowhead::Bar, ..Stroke::default() };
    let (_, heads) = apply(&bp, &st, false).unwrap();
    // The curve leaves heading along +x, so the bar at the start stands upright.
    let b = heads[0].path.bounding_box();
    assert!(b.height() > b.width() * 3.0, "{b:?}");
}

#[test]
fn end_head_follows_tangent_when_end_handle_is_missing() {
    let mut bp = BezPath::new();
    bp.move_to((0.0, 0.0));
    bp.curve_to((100.0, 0.0), (100.0, 100.0), (100.0, 100.0));
    let st = Stroke { weight: 1.0, end: Arrowhead::Bar, ..Stroke::default() };
    let (_, heads) = apply(&bp, &st, false).unwrap();
    // The curve arrives heading along +y, so the bar at the end lies across, horizontally.
    let b = heads[0].path.bounding_box();
    assert!(b.width() > b.height() * 3.0, "{b:?}");
}
