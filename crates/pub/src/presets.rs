// Copyright (c) 2026 srt8hellcat2-ui. SPDX-License-Identifier: MIT OR Apache-2.0
//! The confirmed subset of ECMA preset geometry; see docs/pub-format/preset-geometry.md.
//! Definitions are local constants, never instructions supplied by a document.
use std::collections::BTreeMap;

use designcraft_geom::{BezPath, PathData, Point, Rect};

enum Piece {
    Move(&'static str, &'static str),
    Cubic([(&'static str, &'static str); 3]),
    Close,
}

struct Preset {
    guides: &'static [(&'static str, &'static str)],
    pieces: &'static [Piece],
}

// Arithmetic and path order checked against the official heart definition.
// Control points outside the frame are intentional and must not be clamped.
const HEART: Preset = Preset {
    guides: &[
        ("dx1", "*/ w 49 48"),
        ("dx2", "*/ w 10 48"),
        ("x1", "+- hc 0 dx1"),
        ("x2", "+- hc 0 dx2"),
        ("x3", "+- hc dx2 0"),
        ("x4", "+- hc dx1 0"),
        ("y1", "+- t 0 hd3"),
    ],
    pieces: &[
        Piece::Move("hc", "hd4"),
        Piece::Cubic([("x3", "y1"), ("x4", "hd4"), ("hc", "b")]),
        Piece::Cubic([("x1", "hd4"), ("x2", "y1"), ("hc", "hd4")]),
        Piece::Close,
    ],
};

fn finite(value: f64) -> Result<f64, &'static str> {
    if value.is_finite() { Ok(value) } else { Err("non-finite preset coordinate") }
}

fn operand(name: &str, guides: &BTreeMap<&str, f64>) -> Result<f64, &'static str> {
    finite(match guides.get(name) {
        Some(value) => *value,
        None => name.parse().map_err(|_| "unknown preset guide")?,
    })
}

fn formula(text: &str, guides: &BTreeMap<&str, f64>) -> Result<f64, &'static str> {
    let mut words = text.split_whitespace();
    let op = words.next().ok_or("missing preset operator")?;
    let mut arg = || operand(words.next().ok_or("missing preset operand")?, guides);
    let (a, b, c) = (arg()?, arg()?, arg()?);
    if words.next().is_some() {
        return Err("extra preset operand");
    }
    finite(match op {
        "*/" if c != 0.0 => a * b / c,
        "*/" => return Err("preset division by zero"),
        "+-" => a + b - c,
        _ => return Err("unsupported preset operator"),
    })
}

fn draw(preset: &Preset, frame: Rect) -> Result<PathData, &'static str> {
    let (w, h) = (finite(frame.width())?, finite(frame.height())?);
    if !frame.is_finite() || w < 0.0 || h < 0.0 {
        return Err("invalid preset frame");
    }
    let mut guides = BTreeMap::from([("w", w), ("h", h), ("hc", w / 2.0), ("hd3", h / 3.0), ("hd4", h / 4.0), ("t", 0.0), ("b", h)]);
    for &(name, expression) in preset.guides {
        if guides.contains_key(name) {
            return Err("duplicate preset guide");
        }
        let value = formula(expression, &guides)?;
        guides.insert(name, value);
    }
    let point =
        |x, y| -> Result<Point, &'static str> { Ok(Point::new(finite(frame.x0 + operand(x, &guides)?)?, finite(frame.y0 + operand(y, &guides)?)?)) };
    let mut path = BezPath::new();
    let mut current = false;
    for piece in preset.pieces {
        match *piece {
            Piece::Move(x, y) => {
                path.move_to(point(x, y)?);
                current = true;
            }
            Piece::Cubic([a, b, end]) if current => path.curve_to(point(a.0, a.1)?, point(b.0, b.1)?, point(end.0, end.1)?),
            Piece::Close if current => path.close_path(),
            _ => return Err("preset path has no starting point"),
        }
    }
    Ok(PathData::from_bezpath(&path))
}

pub(crate) fn heart(frame: Rect) -> Result<PathData, &'static str> {
    draw(&HEART, frame)
}

#[cfg(test)]
mod tests {
    use super::*;
    use designcraft_geom::PathEl;

    #[test]
    fn arithmetic_uses_named_guides_and_signed_values() {
        let guides = BTreeMap::from([("w", 96.0), ("hc", 48.0)]);
        assert_eq!(formula("*/ w 49 48", &guides).unwrap(), 98.0);
        assert_eq!(formula("*/ w 10 48", &guides).unwrap(), 20.0);
        assert_eq!(formula("+- hc 0 98", &guides).unwrap(), -50.0);
        assert_eq!(formula("+- hc 20 0", &guides).unwrap(), 68.0);
        for bad in ["", "*/ 1 2", "*/ 1 2 0", "+- 1 2 3 4", "+- missing 0 0", "sin 1 2 3", "+- NaN 0 0", "*/ 1e308 2 1"] {
            assert!(formula(bad, &guides).is_err(), "{bad}");
        }
    }

    #[test]
    fn heart_has_the_hand_computed_cubics_in_a_translated_non_square_frame() {
        let path = heart(Rect::new(10.0, 20.0, 106.0, 140.0)).unwrap().to_bezpath();
        // w=96, h=120: dx1=98, dx2=20, y1=-40 before translation.
        assert_eq!(
            path.elements(),
            &[
                PathEl::MoveTo(Point::new(58.0, 50.0)),
                PathEl::CurveTo(Point::new(78.0, -20.0), Point::new(156.0, 50.0), Point::new(58.0, 140.0)),
                PathEl::CurveTo(Point::new(-40.0, 50.0), Point::new(38.0, -20.0), Point::new(58.0, 50.0)),
                PathEl::ClosePath,
            ]
        );
        let doubled = heart(Rect::new(20.0, 40.0, 212.0, 280.0)).unwrap().to_bezpath();
        assert_eq!(doubled, designcraft_geom::Affine::scale(2.0) * path);
    }

    #[test]
    fn invalid_frames_and_definitions_return_errors() {
        for frame in [Rect::new(1.0, 0.0, 0.0, 1.0), Rect::new(0.0, 2.0, 1.0, 1.0), Rect::new(0.0, 0.0, f64::INFINITY, 1.0)] {
            assert!(heart(frame).is_err());
        }
        assert!(heart(Rect::ZERO).is_ok());
        let frame = Rect::new(0.0, 0.0, 96.0, 120.0);
        assert!(draw(&Preset { guides: &[("a", "+- later 0 0")], pieces: &[] }, frame).is_err());
        assert!(draw(&Preset { guides: &[("w", "+- 1 0 0")], pieces: &[] }, frame).is_err());
        assert!(draw(&Preset { guides: &[], pieces: &[Piece::Close] }, frame).is_err());
    }
}
