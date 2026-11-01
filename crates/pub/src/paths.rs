// Copyright (c) 2026 srt8hellcat2-ui. SPDX-License-Identifier: MIT OR Apache-2.0
//! Freeform geometry confirmed in the samples documented in odraw-geometry.md §5.
use std::collections::BTreeSet;

use designcraft_geom::{BezPath, PathData, Point, Rect};

use crate::{escher::EShape, read};

pub(crate) fn freeform(sh: &EShape, frame: Rect, warnings: &mut Vec<String>) -> Option<PathData> {
    match decode(sh, frame, warnings) {
        Ok(path) => Some(path),
        Err(reason) => {
            warnings.push(format!("Freeform path could not be decoded: {reason}."));
            None
        }
    }
}

fn elements(raw: &[u8], size: u16) -> Result<&[u8], &'static str> {
    let count = usize::from(read::u16_at(raw, 0).map_err(|_| "truncated array header")?);
    let capacity = usize::from(read::u16_at(raw, 2).map_err(|_| "truncated array header")?);
    let stored_size = read::u16_at(raw, 4).map_err(|_| "truncated array header")?;
    if stored_size != size {
        return Err("array element size is not confirmed by the samples");
    }
    if count > capacity {
        return Err("array count exceeds capacity");
    }
    let bytes = count.checked_mul(usize::from(size)).ok_or("array size overflow")?;
    let end = 6usize.checked_add(bytes).ok_or("array size overflow")?;
    // Counts are u16: at most 65,535 points or records, bounded before allocation.
    raw.get(6..end).filter(|_| end == raw.len()).ok_or("array length does not match its count")
}

fn decode(sh: &EShape, frame: Rect, warnings: &mut Vec<String>) -> Result<PathData, &'static str> {
    let raw = sh.complex.get(&0x0145).ok_or("vertices are missing")?;
    let vertices = elements(raw, 8)?;
    let raw = sh.complex.get(&0x0146).ok_or("segment records are missing")?;
    let segments = elements(raw, 2)?;
    let coordinate = |key, default| f64::from(sh.prop(key).unwrap_or(default) as i32);
    let (left, top) = (coordinate(0x0140, 0), coordinate(0x0141, 0));
    let (width, height) = (coordinate(0x0142, 21_600) - left, coordinate(0x0143, 21_600) - top);
    if width <= 0.0 || height <= 0.0 || !frame.is_finite() {
        return Err("invalid geometry bounds");
    }
    let (sx, sy) = (frame.width() / width, frame.height() / height);
    if !sx.is_finite() || !sy.is_finite() {
        return Err("invalid geometry scale");
    }
    let mut points = Vec::with_capacity(vertices.len() / 8);
    for vertex in vertices.chunks_exact(8) {
        let x = read::u32_at(vertex, 0).map_err(|_| "truncated vertex")?;
        let y = read::u32_at(vertex, 4).map_err(|_| "truncated vertex")?;
        if [x, y].iter().any(|v| (0x8000_0000..=0x8000_007f).contains(v)) {
            return Err("guide references are not confirmed by the samples");
        }
        let point = Point::new(frame.x0 + (f64::from(x as i32) - left) * sx, frame.y0 + (f64::from(y as i32) - top) * sy);
        if !point.is_finite() {
            return Err("invalid vertex coordinate");
        }
        points.push(point);
    }
    let mut points = points.into_iter();
    let mut path = BezPath::new();
    let mut current = false;
    let mut drawable = false;
    let mut ended = false;
    let mut stopped = false;
    let mut escape_codes = BTreeSet::new();
    for segment in segments.chunks_exact(2) {
        let word = read::u16_at(segment, 0).map_err(|_| "truncated segment")?;
        let (kind, count) = (word >> 13, word & 0x1fff);
        match kind {
            0 | 1 => {
                if !current {
                    return Err("drawing segment has no starting point");
                }
                for _ in 0..count {
                    let a = points.next().ok_or("segment exceeds vertex count")?;
                    if kind == 0 {
                        path.line_to(a);
                    } else {
                        let b = points.next().ok_or("curve exceeds vertex count")?;
                        let c = points.next().ok_or("curve exceeds vertex count")?;
                        path.curve_to(a, b, c);
                    }
                    drawable = true;
                }
            }
            2 => {
                if count != 0 {
                    return Err("move count is not zero");
                }
                path.move_to(points.next().ok_or("move exceeds vertex count")?);
                current = true;
            }
            3 => {
                if count != 1 || !current {
                    return Err("invalid close segment");
                }
                path.close_path();
            }
            4 => {
                ended = true;
                break;
            }
            5 => {
                let code = (word >> 8) & 0x1f;
                if escape_codes.insert(code) {
                    warnings.push(format!("Freeform escape 0x{code:02X} is not implemented; its path effect was skipped."));
                }
                // Only zero-point escapes leave the vertex cursor unambiguous.
                if word & 0xff != 0 {
                    warnings.push("Freeform path stopped at an unsupported escape; only the drawable prefix was retained.".into());
                    stopped = true;
                    break;
                }
            }
            _ => {
                warnings.push(format!("Freeform segment type {kind} is not implemented; only the drawable prefix was retained."));
                stopped = true;
                break;
            }
        }
    }
    if !drawable {
        return Err("no drawable supported segments");
    }
    if !stopped && (!ended || points.next().is_some()) {
        return Err("path end or vertex count does not match the segments");
    }
    Ok(PathData::from_bezpath(&path))
}

#[cfg(test)]
mod tests {
    use super::*;
    use designcraft_geom::PathEl;

    fn shape(vertices: &[(i32, i32)], words: &[u16]) -> EShape {
        let mut sh = EShape::default();
        let mut v = Vec::new();
        v.extend((vertices.len() as u16).to_le_bytes());
        v.extend((vertices.len() as u16).to_le_bytes());
        v.extend(8u16.to_le_bytes());
        for (x, y) in vertices {
            v.extend(x.to_le_bytes());
            v.extend(y.to_le_bytes());
        }
        let mut s = Vec::new();
        s.extend((words.len() as u16).to_le_bytes());
        s.extend((words.len() as u16).to_le_bytes());
        s.extend(2u16.to_le_bytes());
        for word in words {
            s.extend(word.to_le_bytes());
        }
        sh.complex.insert(0x145, v);
        sh.complex.insert(0x146, s);
        sh.props.extend([(0x140, 10), (0x141, 20), (0x142, 110), (0x143, 220)]);
        sh
    }

    #[test]
    fn line_cubic_close_end_and_frame_scaling() {
        let sh = shape(&[(10, 20), (110, 20), (110, 220), (10, 220), (10, 20)], &[0x4000, 1, 0x2001, 0x6001, 0x8000]);
        let mut warnings = Vec::new();
        let path = freeform(&sh, Rect::new(30.0, 40.0, 230.0, 140.0), &mut warnings).unwrap();
        assert!(warnings.is_empty());
        assert_eq!(
            path.to_bezpath().elements(),
            &[
                PathEl::MoveTo((30.0, 40.0).into()),
                PathEl::LineTo((230.0, 40.0).into()),
                PathEl::CurveTo((230.0, 140.0).into(), (30.0, 140.0).into(), (30.0, 40.0).into()),
                PathEl::ClosePath,
            ]
        );
    }

    #[test]
    fn escapes_warn_once_and_keep_the_supported_path() {
        let sh = shape(&[(10, 20), (110, 220)], &[0x4000, 0xac00, 1, 0xac00, 0x8000]);
        let mut warnings = Vec::new();
        assert_eq!(freeform(&sh, Rect::ZERO, &mut warnings).unwrap().to_bezpath().elements().len(), 2);
        assert_eq!(warnings.len(), 1);
        assert!(warnings[0].contains("0x0C"));
        let sh = shape(&[(10, 20), (110, 220)], &[0x4000, 1, 0xa301]);
        let mut warnings = Vec::new();
        assert_eq!(freeform(&sh, Rect::ZERO, &mut warnings).unwrap().to_bezpath().elements().len(), 2);
        assert!(warnings.iter().any(|w| w.contains("prefix")));
    }

    #[test]
    fn hostile_arrays_counts_and_unknown_coordinates_are_rejected() {
        let base = shape(&[(10, 20), (110, 220)], &[0x4000, 1, 0x8000]);
        let check = |sh: EShape| {
            let mut warnings = Vec::new();
            assert!(freeform(&sh, Rect::ZERO, &mut warnings).is_none());
            assert!(!warnings.is_empty());
        };
        for size in [0, 4, 0xfff0] {
            let mut sh = base.clone();
            sh.complex.get_mut(&0x145).unwrap()[4..6].copy_from_slice(&u16::to_le_bytes(size));
            check(sh);
        }
        let mut sh = base.clone();
        sh.complex.get_mut(&0x145).unwrap()[2..4].copy_from_slice(&0u16.to_le_bytes());
        check(sh);
        let mut sh = base.clone();
        sh.complex.get_mut(&0x145).unwrap().pop();
        check(sh);
        check(shape(&[(i32::MIN, 20)], &[0x4000, 1, 0x8000]));
        check(shape(&[(10, 20)], &[0x4000, 0x2001, 0x8000]));
        check(shape(&[(10, 20)], &[1, 0x8000]));
        check(shape(&[(10, 20), (110, 220)], &[0x4001, 1, 0x8000]));
        check(shape(&[(10, 20), (110, 220)], &[0x4000, 1, 0x6000, 0x8000]));
        let mut sh = base;
        sh.props.insert(0x142, 10);
        check(sh);
    }
}
