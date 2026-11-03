// Copyright (c) 2026 srt8hellcat2-ui. SPDX-License-Identifier: MIT OR Apache-2.0
//! Default ECMA textArchUp/textCircle paths; no unverified adjustment conversion.
use designcraft_geom::{BezPath, PathData, Rect, kurbo::Arc};
use std::f64::consts::PI;

/// Geometry for the two fixture-confirmed numbers; only arch is enabled by the
/// importer. The XML's default
/// guides reduce to a left-middle start, half radii and these sweeps. Circle is
/// deliberately just short of a full turn (21599998 / 60000 degrees), not closed.
pub(crate) fn default_path(kind: u16, rect: Rect) -> Option<PathData> {
    if !matches!(kind, 144 | 146)
        || ![rect.x0, rect.y0, rect.x1, rect.y1].iter().all(|v| v.is_finite())
        || rect.width() <= 0.0
        || rect.height() <= 0.0
        || rect.width() > 1_000_000.0
        || rect.height() > 1_000_000.0
    {
        return None;
    }
    let sweep = if kind == 144 {
        PI
    } else {
        // DrawingML angles are radial directions. On a noncircular ellipse the
        // final two-unit gap needs conversion to the ellipse parameter angle.
        let gap = (2.0 / 60_000.0_f64).to_radians();
        2.0 * PI - (rect.width() * gap.sin()).atan2(rect.height() * gap.cos())
    };
    let mut path = BezPath::new();
    path.move_to((rect.x0, rect.center().y));
    Arc::new(rect.center(), (rect.width() / 2.0, rect.height() / 2.0), PI, sweep, 0.0).to_cubic_beziers(0.001, |a, b, end| path.curve_to(a, b, end));
    Some(PathData::from_bezpath(&path))
}

/// Fit based on the same resolved faces and shaping used for drawing. The probe
/// is a clone, so unsupported/overset input leaves the imported story untouched.
#[derive(Clone, Copy, Debug)]
pub(crate) struct Fit {
    pub size: f64,
    pub baseline_shift: f64,
}

fn from_measurements(length: f64, advance: f64, descent: f64) -> Option<Fit> {
    if ![length, advance, descent].iter().all(|v| v.is_finite()) || length <= 0.0 || advance <= 0.0 {
        return None;
    }
    // One representable step leaves room for floating-point endpoint rounding;
    // it is not a visible margin or a reference-image fitting constant.
    let size = (length / advance).next_down();
    let baseline_shift = -descent.max(0.0) * size;
    ((0.1..=1296.0).contains(&size) && baseline_shift.is_finite()).then_some(Fit { size, baseline_shift })
}

pub(crate) fn measure(doc: &designcraft_doc::Document, id: designcraft_doc::ItemId, path: &PathData) -> Option<Fit> {
    use designcraft_compose::{ComposeOptions, compose_story, path_glyphs};
    use designcraft_doc::{PathType, Shape};
    use designcraft_geom::Shape as _;
    let mut probe = doc.clone();
    let sid = probe.item(id)?.text_frame()?.story;
    let story = probe.story_mut(sid)?;
    if story.text.is_empty() || story.text.len() > 16_384 {
        return None;
    }
    let end = story.text.len();
    story.format_chars(0..end, |cf| {
        cf.over.size = Some(1.0);
        cf.over.h_scale = Some(1.0);
        cf.over.baseline_shift = Some(0.0);
    });
    let mut straight = BezPath::new();
    straight.move_to((0.0, 0.0));
    straight.line_to((1_000_000.0, 0.0));
    let item = probe.item_mut(id)?;
    item.shape = Shape::Path;
    item.path = PathData::from_bezpath(&straight);
    let tf = item.text_frame_mut()?;
    tf.options.path = Some(PathType::default());
    tf.options.inset = [0.0; 4];
    tf.options.vertical_justification = designcraft_doc::VerticalJustification::Top;
    let cs = compose_story(&probe, sid, &ComposeOptions::default());
    let ft = cs.frame(id)?;
    if cs.is_overset() || ft.lines.len() != 1 {
        return None;
    }
    let line = ft.lines.first()?;
    let advance = line.glyphs.iter().map(|g| g.adv).sum::<f64>();
    // Straight baseline at y=0: the lowest ink point is the actual shaped text's
    // descender. Place that measured amount inside the curve, without an em guess.
    let descent = path_glyphs(&cs, ft, &straight, &PathType::default()).iter().map(|(_, p)| p.bounding_box().y1).fold(0.0_f64, f64::max);
    let length = designcraft_geom::warp::PathWarp::new(&path.to_bezpath(), false).length();
    from_measurements(length, advance, descent)
}

#[cfg(test)]
mod tests {
    use super::*;
    use designcraft_geom::{Point, warp::PathWarp};

    fn near(a: f64, b: f64) -> Result<(), String> {
        if (a - b).abs() < 0.002 { Ok(()) } else { Err(format!("{a} != {b}")) }
    }

    #[test]
    fn default_arch_and_glyph_tangent_have_hand_computed_coordinates() -> Result<(), String> {
        // Circle of radius 50: left (10,70), top (60,20), right (110,70).
        let path = default_path(144, Rect::new(10.0, 20.0, 110.0, 120.0)).ok_or("no arch")?;
        let warp = PathWarp::new(&path.to_bezpath(), false);
        near(warp.length(), PI * 50.0)?;
        let (start, a) = warp.at(0.0).ok_or("no start")?;
        near(start.x, 10.0)?;
        near(start.y, 70.0)?;
        near(a, -PI / 2.0)?;
        let mid = warp.length() / 2.0;
        let (top, a) = warp.at(mid).ok_or("no midpoint")?;
        near(top.x, 60.0)?;
        near(top.y, 20.0)?;
        near(a, 0.0)?;
        // A glyph centre at x=mid, baseline 12 lands at the top, upright.
        let xf = warp.glyph(mid, 12.0).ok_or("no glyph transform")?;
        let p = xf * Point::new(mid, 12.0);
        near(p.x, 60.0)?;
        near(p.y, 20.0)?;
        let above = xf * Point::new(mid, 2.0);
        near(above.x, 60.0)?;
        near(above.y, 10.0)?;
        let end = warp.at(warp.length()).ok_or("no end")?.0;
        near(end.x, 110.0)?;
        near(end.y, 70.0)?;
        Ok(())
    }

    #[test]
    fn default_circle_is_open_and_almost_one_turn() -> Result<(), String> {
        let path = default_path(146, Rect::new(0.0, 0.0, 100.0, 100.0)).ok_or("no circle")?;
        if path.subpaths.iter().any(|p| p.closed) {
            return Err("circle closed".into());
        }
        let warp = PathWarp::new(&path.to_bezpath(), false);
        near(warp.length(), PI * 100.0)?;
        let (p, a) = warp.at(warp.length() / 2.0).ok_or("no opposite point")?;
        near(p.x, 100.0)?;
        near(p.y, 50.0)?;
        near(a, PI / 2.0)?;
        let xf = warp.glyph(warp.length() / 2.0, 12.0).ok_or("no glyph")?;
        let p = xf * Point::new(warp.length() / 2.0, 2.0);
        near(p.x, 110.0)?;
        near(p.y, 50.0)
    }

    #[test]
    fn ellipse_circle_endpoint_uses_radial_angle_not_parameter_angle() -> Result<(), String> {
        let path = default_path(146, Rect::new(0.0, 0.0, 200.0, 100.0)).ok_or("no ellipse")?;
        let warp = PathWarp::new(&path.to_bezpath(), false);
        let end = warp.at(warp.length()).ok_or("no ellipse endpoint")?.0;
        // A two-unit gap = pi/5400000 radians. At the left radius of 100,
        // dy is 100*pi/5400000 = 0.000058177641733; dx differs only at order gap².
        if end.x.abs() > 1e-8 || (end.y - 50.000_058_177_641_73).abs() > 1e-8 {
            return Err(format!("wrong radial-angle endpoint: {end:?}"));
        }
        Ok(())
    }

    #[test]
    fn fit_uses_measured_advance_and_ink_descent() -> Result<(), String> {
        // Four shaped advances sum to 4 pt at the measurement size of 1 pt.
        // A 200 pt path needs size 50; measured ink descent 0.2 becomes 10 pt.
        let fit = from_measurements(200.0, 4.0, 0.2).ok_or("no measured fit")?;
        near(fit.size, 50.0)?;
        near(fit.baseline_shift, -10.0)?;
        if fit.size * 4.0 > 200.0 {
            return Err("advance exceeds path".into());
        }
        if from_measurements(200.0, 4.0, 1e308).is_some() {
            return Err("non-finite baseline accepted".into());
        }
        for advance in [0.0, -1.0, f64::NAN] {
            if from_measurements(200.0, advance, 0.2).is_some() {
                return Err("invalid measurement accepted".into());
            }
        }
        Ok(())
    }

    #[test]
    fn same_letter_count_uses_different_resolved_advances() -> Result<(), String> {
        use designcraft_doc::{Document, ParaFormat, SpreadRef};
        let path = default_path(144, Rect::new(0.0, 0.0, 100.0, 100.0)).ok_or("no arch")?;
        let mut sizes = Vec::new();
        for text in ["iiii", "WWWW"] {
            let mut doc = Document::new(&Default::default());
            let (id, _) = doc
                .add_text_frame(SpreadRef::Doc(0), Rect::new(0.0, 0.0, 100.0, 100.0), doc.default_layer(), text, ParaFormat::default())
                .map_err(|e| e.to_string())?;
            sizes.push(measure(&doc, id, &path).ok_or("no shaped measurement")?.size);
        }
        let mut it = sizes.into_iter();
        let narrow = it.next().ok_or("no narrow fit")?;
        let wide = it.next().ok_or("no wide fit")?;
        if narrow <= wide {
            return Err("equal character counts ignored different glyph advances".into());
        }
        Ok(())
    }

    #[test]
    fn unsupported_and_invalid_paths_are_declined() -> Result<(), String> {
        for (kind, r) in [
            (145, Rect::new(0.0, 0.0, 100.0, 100.0)),
            (156, Rect::ZERO),
            (144, Rect::ZERO),
            (146, Rect::new(f64::NAN, 0.0, 100.0, 100.0)),
            (144, Rect::new(0.0, 0.0, 1e20, 100.0)),
        ] {
            if default_path(kind, r).is_some() {
                return Err("invalid path accepted".into());
            }
        }
        Ok(())
    }
}
