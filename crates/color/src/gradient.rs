//! Gradients: definitions (swatch-able) and their placement on an object.

use kurbo::{Affine, Point, Rect, Vec2};
use serde::{Deserialize, Serialize};

use crate::Color;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum GradientKind {
    #[default]
    Linear,
    Radial,
    /// Freeform gradients (points/lines) — rendered via a mesh approximation.
    Freeform,
}

impl GradientKind {
    pub fn label(self) -> &'static str {
        match self {
            GradientKind::Linear => "Linear",
            GradientKind::Radial => "Radial",
            GradientKind::Freeform => "Freeform",
        }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct GradientStop {
    /// 0..=1 along the gradient.
    pub offset: f32,
    /// The display color. For a stop with a `swatch`, the swatch's color when it was last
    /// resolved ([`GradientStop::swatch`]).
    pub color: Color,
    #[serde(default = "one")]
    pub opacity: f32,
    /// Midpoint to the next stop, 0.13..=0.87 (InDesign's diamond), default 0.5.
    #[serde(default = "half")]
    pub midpoint: f32,
    /// The color swatch the stop takes its ink from: a spot or process color, a tint of one,
    /// or `[Paper]`. Print export writes the stop in that swatch's inks; `None` is a plain color.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub swatch: Option<String>,
}

impl GradientStop {
    /// A plain-color stop, opaque, with the midpoint halfway to the next stop.
    pub fn new(offset: f32, color: Color) -> Self {
        Self { offset, color, opacity: 1.0, midpoint: 0.5, swatch: None }
    }
}

fn one() -> f32 {
    1.0
}
fn half() -> f32 {
    0.5
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Gradient {
    pub kind: GradientKind,
    pub stops: Vec<GradientStop>,
}

impl Default for Gradient {
    /// InDesign's default "White, Black" gradient.
    fn default() -> Self {
        Self { kind: GradientKind::Linear, stops: vec![GradientStop::new(0.0, Color::WHITE), GradientStop::new(1.0, Color::BLACK)] }
    }
}

impl Gradient {
    /// Colour and opacity at `t` (honours midpoints).
    pub fn sample(&self, t: f32) -> (Color, f32) {
        self.sample_in(None, t)
    }

    /// For each window `(stops[k], stops[k + 1])`, the largest end offset of windows `0..=k`: the
    /// first window whose end reaches `t` is the first `k` where this does, so a binary search
    /// finds it (stops in any order).
    fn window_ends(&self) -> Vec<f32> {
        let mut end = f32::NAN;
        self.stops
            .iter()
            .skip(1)
            .map(|s| {
                end = end.max(s.offset);
                end
            })
            .collect()
    }

    /// [`Self::sample`], with [`Self::window_ends`] given when it was worked out once (else a scan).
    fn sample_in(&self, ends: Option<&[f32]>, t: f32) -> (Color, f32) {
        let stops = &self.stops;
        let Some(first) = stops.first() else {
            return (Color::BLACK, 1.0);
        };
        if t <= first.offset {
            return (first.color, first.opacity);
        }
        // The first window whose end offset is at or past `t` (none for a NaN `t`).
        let k = match ends {
            None => stops.windows(2).position(|w| t <= w[1].offset).unwrap_or(stops.len()),
            Some(ends) => ends.partition_point(|e| !(*e >= t)),
        };
        if let (Some(a), Some(b)) = (stops.get(k), stops.get(k + 1)) {
            let span = (b.offset - a.offset).max(1e-6);
            let u = (t - a.offset) / span;
            // Map through the midpoint: u=mid → 0.5.
            let m = a.midpoint.clamp(0.01, 0.99);
            let v = if u < m { 0.5 * u / m } else { 0.5 + 0.5 * (u - m) / (1.0 - m) };
            return (a.color.lerp(&b.color, v), a.opacity + (b.opacity - a.opacity) * v);
        }
        match stops.last() {
            Some(l) => (l.color, l.opacity),
            None => (Color::BLACK, 1.0),
        }
    }
    /// Stops expanded so that midpoints are represented as explicit stops (for renderers without midpoints).
    pub fn expanded_stops(&self) -> Vec<(f32, Color, f32)> {
        let ends = self.window_ends();
        let mut out = Vec::new();
        for (i, s) in self.stops.iter().enumerate() {
            out.push((s.offset, s.color, s.opacity));
            if let Some(n) = self.stops.get(i + 1)
                && (s.midpoint - 0.5).abs() > 1e-3
            {
                let t = s.offset + (n.offset - s.offset) * s.midpoint;
                let (c, o) = self.sample_in(Some(&ends), t);
                out.push((t, c, o));
            }
        }
        out
    }
    /// Stops for a renderer that blends between them in RGB: the expanded stops, plus samples
    /// through each span that isn't RGB at both ends, so process colours keep blending as inks.
    pub fn display_stops(&self) -> Vec<(f32, Color, f32)> {
        const STEPS: usize = 16;
        let stops = self.expanded_stops();
        let ends = self.window_ends();
        let mut out: Vec<(f32, Color, f32)> = Vec::with_capacity(stops.len() * STEPS);
        for (i, s) in stops.iter().enumerate() {
            out.push(*s);
            let Some(n) = stops.get(i + 1) else { continue };
            if matches!((s.1, n.1), (Color::Rgb { .. }, Color::Rgb { .. })) || n.0 - s.0 <= 1e-4 {
                continue;
            }
            for j in 1..STEPS {
                let t = s.0 + (n.0 - s.0) * j as f32 / STEPS as f32;
                let (c, a) = self.sample_in(Some(&ends), t);
                out.push((t, c, a));
            }
        }
        out
    }
    pub fn reverse(&mut self) {
        // A midpoint belongs to the segment after its stop, so it moves with that segment and mirrors.
        let mids: Vec<_> = self.stops.iter().rev().skip(1).map(|s| 1.0 - s.midpoint).collect();
        self.stops.reverse();
        for (s, m) in self.stops.iter_mut().zip(mids.into_iter().chain([0.5])) {
            s.offset = 1.0 - s.offset;
            s.midpoint = m;
        }
    }
    pub fn sort(&mut self) {
        self.stops.sort_by(|a, b| a.offset.total_cmp(&b.offset));
    }
}

/// Where the gradient sits on an object, in document coordinates.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct GradientGeom {
    pub start: Point,
    pub end: Point,
    /// Radial aspect ratio (height / width), 1 = circle.
    #[serde(default = "one64")]
    pub aspect: f64,
}

fn one64() -> f64 {
    1.0
}

impl GradientGeom {
    /// Default placement for a bounding box: linear spans left→right through the centre at `angle_deg`,
    /// radial is centred with radius = half the larger dimension.
    pub fn fit(kind: GradientKind, b: Rect, angle_deg: f64) -> Self {
        let c = b.center();
        match kind {
            GradientKind::Radial => {
                let r = b.width().max(b.height()) / 2.0;
                Self { start: c, end: c + Vec2::new(r, 0.0), aspect: 1.0 }
            }
            _ => {
                let a = angle_deg.to_radians();
                let d = Vec2::new(a.cos(), -a.sin());
                // Project the box corners onto the direction to cover the whole box.
                let half = (b.width() * d.x.abs() + b.height() * d.y.abs()) / 2.0;
                Self { start: c - d * half, end: c + d * half, aspect: 1.0 }
            }
        }
    }
    pub fn transform(&mut self, a: Affine) {
        self.start = a * self.start;
        self.end = a * self.end;
    }
    pub fn angle_deg(&self) -> f64 {
        let v = self.end - self.start;
        (-v.y).atan2(v.x).to_degrees()
    }
    pub fn length(&self) -> f64 {
        (self.end - self.start).hypot()
    }
}

/// A gradient applied to a fill or stroke.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct GradientPaint {
    pub gradient: Gradient,
    /// None = fit to the object's bounds at `angle` each render (fresh gradients behave like this).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub geom: Option<GradientGeom>,
    #[serde(default)]
    pub angle: f64,
    /// Name of the gradient swatch, if linked.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub swatch: Option<String>,
}

impl GradientPaint {
    pub fn new(gradient: Gradient) -> Self {
        Self { gradient, geom: None, angle: 0.0, swatch: None }
    }
    pub fn resolve(&self, bounds: Rect) -> GradientGeom {
        self.geom.unwrap_or_else(|| GradientGeom::fit(self.gradient.kind, bounds, self.angle))
    }
}

#[cfg(test)]
mod tests {
    /// The sampler as it was: the first window whose end reaches `t`, by a scan.
    fn sample_by_scan(g: &super::Gradient, t: f32) -> (super::Color, f32) {
        let stops = &g.stops;
        if stops.is_empty() {
            return (super::Color::BLACK, 1.0);
        }
        if t <= stops[0].offset {
            return (stops[0].color, stops[0].opacity);
        }
        for w in stops.windows(2) {
            let (a, b) = (&w[0], &w[1]);
            if t <= b.offset {
                let span = (b.offset - a.offset).max(1e-6);
                let u = (t - a.offset) / span;
                let m = a.midpoint.clamp(0.01, 0.99);
                let v = if u < m { 0.5 * u / m } else { 0.5 + 0.5 * (u - m) / (1.0 - m) };
                return (a.color.lerp(&b.color, v), a.opacity + (b.opacity - a.opacity) * v);
            }
        }
        stops.last().map_or((super::Color::BLACK, 1.0), |l| (l.color, l.opacity))
    }

    /// The search finds the window the scan found, for stops in order, out of order, repeated
    /// and NaN; and many stops expand in linear time.
    #[test]
    fn sampling_by_search_matches_the_scan() {
        use super::{Color, Gradient, GradientKind, GradientStop};
        let offsets = [0.0, 0.3, 0.2, 0.2, f32::NAN, 0.9, 0.5, 1.0, 0.95];
        let stops: Vec<GradientStop> = offsets
            .iter()
            .enumerate()
            .map(|(i, &o)| GradientStop { midpoint: 0.3, ..GradientStop::new(o, Color::rgb8((i * 25) as u8, 0, 255 - (i * 25) as u8)) })
            .collect();
        for n in 0..=stops.len() {
            let g = Gradient { kind: GradientKind::Linear, stops: stops[..n].to_vec() };
            for t in [-1.0, 0.0, 0.1, 0.2, 0.25, 0.3, 0.6, 0.9, 0.93, 1.0, 2.0, f32::NAN, f32::INFINITY] {
                // Compared as printed: a NaN offset gives NaN components on both sides.
                let a = format!("{:?}", g.sample_in(Some(&g.window_ends()), t));
                let b = format!("{:?}", sample_by_scan(&g, t));
                assert_eq!(format!("{:?}", g.sample(t)), b, "{n} stops at {t} (scan)");
                assert_eq!(a, b, "{n} stops at {t}");
            }
        }
        let many = Gradient {
            kind: GradientKind::Linear,
            stops: (0..100_000).map(|i| GradientStop { midpoint: 0.3, ..GradientStop::new(i as f32 / 100_000.0, Color::BLACK) }).collect(),
        };
        let t = std::time::Instant::now();
        assert_eq!(many.expanded_stops().len(), 199_999);
        assert!(t.elapsed() < std::time::Duration::from_secs(2), "{:?}", t.elapsed());
    }

    use super::*;

    #[test]
    fn sample_endpoints_and_mid() {
        let g = Gradient::default();
        assert_eq!(g.sample(0.0).0.to_hex(), "#ffffff");
        assert_eq!(g.sample(1.0).0.to_hex(), "#000000");
        let mid = g.sample(0.5).0.to_rgb()[0];
        assert!((mid - 0.5).abs() < 1e-5);
    }

    #[test]
    fn process_stops_blend_as_inks() {
        // LYH Studio episode 6: magenta to cyan is C 50 M 50 halfway (a light lilac), not the RGB mix.
        let mut g = Gradient::default();
        g.stops[0].color = Color::cmyk(0.0, 1.0, 0.0, 0.0);
        g.stops[1].color = Color::cmyk(1.0, 0.0, 0.0, 0.0);
        assert_eq!(g.sample(0.5).0, Color::cmyk(0.5, 0.5, 0.0, 0.0));
        let d = g.display_stops();
        assert_eq!(d.len(), 17, "sampled through the span");
        assert!(d.iter().any(|(o, c, _)| (*o - 0.5).abs() < 1e-6 && *c == Color::cmyk(0.5, 0.5, 0.0, 0.0)));
        // A midpoint stays an ink too; RGB stops aren't sampled.
        g.stops[0].midpoint = 0.25;
        assert!(matches!(g.expanded_stops()[1].1, Color::Cmyk { .. }));
        assert_eq!(Gradient::default().display_stops().len(), 2);
        // Grey and CMYK meet as inks; RGB with anything else blends in RGB.
        assert_eq!(Color::gray(1.0).lerp(&Color::cmyk(1.0, 0.0, 0.0, 0.0), 0.5), Color::cmyk(0.5, 0.0, 0.0, 0.5));
        assert!(matches!(Color::rgb(1.0, 0.0, 0.0).lerp(&Color::cmyk(1.0, 0.0, 0.0, 0.0), 0.5), Color::Rgb { .. }));
    }

    #[test]
    fn saved_stops_without_a_swatch_are_plain_colors() {
        let old = r#"{"offset": 0.0, "color": {"model": "cmyk", "c": 0.0, "m": 0.3, "y": 0.9, "k": 0.1}, "opacity": 1.0, "midpoint": 0.5}"#;
        let stop: GradientStop = serde_json::from_str(old).unwrap();
        assert_eq!(stop, GradientStop::new(0.0, Color::cmyk(0.0, 0.3, 0.9, 0.1)));
        assert!(!serde_json::to_string(&stop).unwrap().contains("swatch"), "plain stops save as before");
        let on = GradientStop { swatch: Some("Gold".into()), ..stop };
        assert_eq!(serde_json::from_str::<GradientStop>(&serde_json::to_string(&on).unwrap()).unwrap(), on);
    }

    #[test]
    fn midpoint_shifts() {
        let mut g = Gradient::default();
        g.stops[0].midpoint = 0.25;
        // At t = 0.25 we should be half way.
        assert!((g.sample(0.25).0.to_rgb()[0] - 0.5).abs() < 1e-5);
        assert_eq!(g.expanded_stops().len(), 3);
    }

    #[test]
    fn fit_linear_horizontal() {
        let g = GradientGeom::fit(GradientKind::Linear, Rect::new(0.0, 0.0, 100.0, 50.0), 0.0);
        assert_eq!(g.start, Point::new(0.0, 25.0));
        assert_eq!(g.end, Point::new(100.0, 25.0));
        assert!((g.angle_deg()).abs() < 1e-9);
        let v = GradientGeom::fit(GradientKind::Linear, Rect::new(0.0, 0.0, 100.0, 50.0), 90.0);
        assert!((v.angle_deg() - 90.0).abs() < 1e-9);
        assert!((v.length() - 50.0).abs() < 1e-9);
    }

    #[test]
    fn reverse_stops() {
        let mut g = Gradient::default();
        g.stops[0].midpoint = 0.25;
        g.reverse();
        assert_eq!(g.stops[0].color.to_hex(), "#000000");
        assert_eq!(g.stops[0].offset, 0.0);
        // The midpoint moves to its reversed segment and mirrors.
        assert!((g.sample(0.75).0.to_rgb()[0] - 0.5).abs() < 1e-5);
        assert_eq!(g.stops.iter().map(|s| s.midpoint).collect::<Vec<_>>(), [0.75, 0.5]);

        // With several stops, the reversed gradient is the original mirrored.
        let mut g = Gradient::default();
        g.stops[0].midpoint = 0.3;
        g.stops.insert(1, GradientStop { offset: 0.4, color: Color::rgb(1.0, 0.0, 0.0), opacity: 0.5, midpoint: 0.8, swatch: None });
        let original = g.clone();
        g.reverse();
        for t in [0.1, 0.3, 0.5, 0.7, 0.9] {
            let ((c, o), (want_c, want_o)) = (g.sample(t), original.sample(1.0 - t));
            assert!(c.to_rgb().iter().zip(want_c.to_rgb()).all(|(a, b)| (a - b).abs() < 1e-5), "colour at {t}");
            assert!((o - want_o).abs() < 1e-5, "opacity at {t}");
        }
    }
}
