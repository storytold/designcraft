#[cfg(not(target_arch = "wasm32"))]
mod sysfonts;

#[cfg(not(target_arch = "wasm32"))]
mod fallback;

#[cfg(not(target_arch = "wasm32"))]
mod docfonts;

mod vertical;

/// A glyph that keeps drawing past the element cap stops growing the path, and says so.
#[test]
fn the_outline_pen_stops_at_the_element_cap() {
    use super::outline::MAX_OUTLINE_ELEMENTS;
    use super::{BezPath, FlipPen, OutlinePen};
    let mut pen = FlipPen(BezPath::new());
    pen.move_to(0.0, 0.0);
    for i in 0..MAX_OUTLINE_ELEMENTS + 10 {
        pen.line_to(i as f32, 0.0);
    }
    pen.close();
    assert_eq!(pen.0.elements().len(), MAX_OUTLINE_ELEMENTS);
    assert!(!pen.has_room());
}
