//! Colour and gradient overlays. DesignCraft has no such effect, so an overlay is baked into what
//! its node paints: fill, stroke and text colours, and pixels. The overlay lies over the node's
//! own paint in its blend mode at its opacity, inside the node's alpha (which it keeps).
//!
//! Blend modes follow the W3C Compositing and Blending formulas; the modes PDF lacks (Vivid
//! Light, Linear Light, Pin Light, Hard Mix, Add, Subtract, Average, Negation, Reflect, Glow,
//! Darker/Lighter Color) use their usual definitions.

use std::borrow::Cow;

use crate::model::{Affine, Blend, Effect, Image, Kind, Node, Pixels, Point};
use crate::paint::{Color, Gradient, GradientKind, Paint, Stop, Stroke};

/// Deepest subtree an overlay is baked into (the reader bounds trees well below this).
const MAX_DEPTH: usize = 256;

/// Most pixels an overlay is baked into per image.
const MAX_PIXELS: usize = 64 << 20;

#[derive(Clone, Copy)]
enum Source<'a> {
    Solid(Color),
    Gradient(&'a Gradient),
}

#[derive(Clone, Copy)]
struct Overlay<'a> {
    source: Source<'a>,
    opacity: f64,
    blend: Blend,
}

/// `node` with its colour and gradient overlays baked in (borrowed unchanged when it has none).
/// `warn` hears what could only be approximated.
pub(crate) fn apply<'a>(node: &'a Node, warn: &mut dyn FnMut(&str)) -> Cow<'a, Node> {
    let overlays: Vec<Overlay> = node
        .effects
        .iter()
        .filter_map(|e| match e {
            Effect::ColorOverlay { color, opacity, blend } => Some(Overlay { source: Source::Solid(*color), opacity: *opacity, blend: *blend }),
            Effect::GradientOverlay { gradient, opacity, blend } => {
                Some(Overlay { source: Source::Gradient(gradient), opacity: *opacity, blend: *blend })
            }
            _ => None,
        })
        .filter(|o| o.opacity > 0.0)
        .collect();
    if overlays.is_empty() {
        return Cow::Borrowed(node);
    }
    let mut out = node.clone();
    out.effects.retain(|e| !matches!(e, Effect::ColorOverlay { .. } | Effect::GradientOverlay { .. }));
    for o in &overlays {
        if matches!(o.blend, Blend::Erase | Blend::PassThrough) {
            warn("overlay effects in a blend mode DesignCraft doesn't have (imported as Normal)");
        }
        bake_node(&mut out, o, 0, warn);
    }
    Cow::Owned(out)
}

fn bake_node(node: &mut Node, o: &Overlay, depth: usize, warn: &mut dyn FnMut(&str)) {
    if depth > MAX_DEPTH {
        return;
    }
    match &mut node.kind {
        Kind::Shape { fills, strokes, .. } => {
            for f in fills.iter_mut() {
                *f = paint(f, o, warn);
            }
            strokes.iter_mut().for_each(|s| stroke(s, o, warn));
        }
        Kind::Artboard { background, strokes, .. } => {
            for f in background.iter_mut() {
                *f = paint(f, o, warn);
            }
            strokes.iter_mut().for_each(|s| stroke(s, o, warn));
        }
        Kind::Text(t) => {
            for r in &mut t.runs {
                r.fill = paint(&r.fill, o, warn);
                if let Some(s) = &mut r.stroke {
                    stroke(s, o, warn);
                }
            }
        }
        Kind::Table(t) => {
            for c in &mut t.cells {
                for r in &mut c.runs {
                    r.fill = paint(&r.fill, o, warn);
                }
            }
            for s in t.vertical.iter_mut().chain(t.horizontal.iter_mut()).flatten() {
                stroke(s, o, warn);
            }
        }
        Kind::Image(img) => {
            if !pixels(img, o) {
                warn("overlay effects on images too large to recolour (left out)");
            }
        }
        Kind::Layer | Kind::Group | Kind::Unsupported | Kind::MasterInstance { .. } => {}
    }
    for e in &mut node.effects {
        match e {
            Effect::Outline { color, .. } => *color = over(*color, o.source.at_solid(), o),
            Effect::DropShadow { .. } | Effect::InnerShadow { .. } | Effect::OuterGlow { .. } => {}
            Effect::ColorOverlay { .. } | Effect::GradientOverlay { .. } => {}
        }
    }
    for c in &mut node.children {
        bake_node(c, o, depth + 1, warn);
    }
}

fn stroke(s: &mut Stroke, o: &Overlay, warn: &mut dyn FnMut(&str)) {
    s.paint = paint(&s.paint, o, warn);
}

/// A paint with the overlay over it.
fn paint(p: &Paint, o: &Overlay, warn: &mut dyn FnMut(&str)) -> Paint {
    match (p, o.source) {
        (Paint::None, _) => Paint::None,
        (Paint::Solid(c), Source::Solid(s)) => Paint::Solid(over(*c, s, o)),
        (Paint::Gradient(g), Source::Solid(s)) => {
            Paint::Gradient(Gradient { stops: g.stops.iter().map(|st| Stop { color: over(st.color, s, o), ..*st }).collect(), ..g.clone() })
        }
        // Over one colour, the result is the overlay's gradient with each stop laid over it.
        (Paint::Solid(c), Source::Gradient(g)) => {
            Paint::Gradient(Gradient { stops: g.stops.iter().map(|st| Stop { color: over(*c, st.color, o), ..*st }).collect(), ..g.clone() })
        }
        (Paint::Gradient(base), Source::Gradient(g)) => {
            if !(o.blend == Blend::Normal && o.opacity >= 1.0) {
                warn("gradient overlays on gradients (approximated)");
            }
            // Exact when the overlay covers it; otherwise over the base gradient's average colour.
            let avg = average(base);
            Paint::Gradient(Gradient { stops: g.stops.iter().map(|st| Stop { color: over(avg, st.color, o), ..*st }).collect(), ..g.clone() })
        }
    }
}

impl Source<'_> {
    /// One colour for the source (a gradient's average) where only one fits.
    fn at_solid(&self) -> Color {
        match self {
            Source::Solid(c) => *c,
            Source::Gradient(g) => average(g),
        }
    }
}

fn average(g: &Gradient) -> Color {
    let n = g.stops.len().max(1) as f64;
    let (mut r, mut gr, mut b, mut a) = (0.0, 0.0, 0.0, 0.0);
    for s in &g.stops {
        let [sr, sg, sb] = rgb(s.color);
        r += sr;
        gr += sg;
        b += sb;
        a += s.color.alpha();
    }
    Color::Rgb { r: r / n, g: gr / n, b: b / n, a: a / n }
}

/// `source` laid over `base` in the overlay's mode and opacity; `base`'s alpha is kept.
fn over(base: Color, source: Color, o: &Overlay) -> Color {
    let k = (o.opacity * source.alpha()).clamp(0.0, 1.0);
    let alpha = base.alpha();
    // Normal mixes within the colour model when both share it, so CMYK stays CMYK.
    if matches!(o.blend, Blend::Normal | Blend::PassThrough | Blend::Erase) {
        let mix = |a: f64, b: f64| a + (b - a) * k;
        match (base, source) {
            (Color::Cmyk { c, m, y, k: kk, .. }, Color::Cmyk { c: c2, m: m2, y: y2, k: k2, .. }) => {
                return Color::Cmyk { c: mix(c, c2), m: mix(m, m2), y: mix(y, y2), k: mix(kk, k2), a: alpha };
            }
            (Color::Gray { v, .. }, Color::Gray { v: v2, .. }) => return Color::Gray { v: mix(v, v2), a: alpha },
            (_, s) if k >= 1.0 => return with_alpha(s, alpha),
            _ => {}
        }
    }
    let b = rgb(base);
    let s = rgb(source);
    let blended = blend(o.blend, b, s);
    let [r, g, bl] = [0, 1, 2].map(|i| b[i] + (blended[i] - b[i]) * k);
    Color::Rgb { r, g, b: bl, a: alpha }
}

fn with_alpha(c: Color, a: f64) -> Color {
    match c {
        Color::Rgb { r, g, b, .. } => Color::Rgb { r, g, b, a },
        Color::Cmyk { c, m, y, k, .. } => Color::Cmyk { c, m, y, k, a },
        Color::Gray { v, .. } => Color::Gray { v, a },
        Color::Lab { l, a: la, b, .. } => Color::Lab { l, a: la, b, alpha: a },
    }
}

/// sRGB components (0..1) of a colour; CMYK goes through DesignCraft's working CMYK conversion.
fn rgb(c: Color) -> [f64; 3] {
    let f = |v: f64| if v.is_finite() { v.clamp(0.0, 1.0) } else { 0.0 };
    match c {
        Color::Rgb { r, g, b, .. } => [f(r), f(g), f(b)],
        Color::Gray { v, .. } => [f(v); 3],
        Color::Cmyk { c, m, y, k, .. } => {
            let u = |v: f64| f(v) as f32;
            designcraft_color::Color::cmyk(u(c), u(m), u(y), u(k)).to_rgb().map(|v| f(f64::from(v)))
        }
        Color::Lab { l, a, b, .. } => {
            let u = |v: f64| if v.is_finite() { v as f32 } else { 0.0 };
            designcraft_color::cms::lab::lab_to_srgb(designcraft_color::cms::Lab { l: u(l), a: u(a), b: u(b) }).map(|v| f(f64::from(v)))
        }
    }
}

/// Bake the overlay into an image's pixels. False if they couldn't be read or are too many.
fn pixels(img: &mut Image, o: &Overlay) -> bool {
    let Some(n) = (img.width as usize).checked_mul(img.height as usize).filter(|n| *n <= MAX_PIXELS) else { return false };
    let Some(len) = n.checked_mul(4) else { return false };
    let mut px = match &img.pixels {
        Pixels::Rgba8(p) => match p.get(..len) {
            Some(p) => p.to_vec(),
            None => return false,
        },
        Pixels::Encoded(bytes) => match image::load_from_memory(bytes) {
            Ok(d) if d.width() == img.width && d.height() == img.height => d.to_rgba8().into_raw(),
            _ => return false,
        },
    };
    let width = img.width.max(1) as usize;
    // A solid source is converted once; a gradient is sampled at each pixel's centre.
    let solid = match o.source {
        Source::Solid(c) => Some((rgb(c), c.alpha())),
        Source::Gradient(_) => None,
    };
    let to_unit = match o.source {
        Source::Gradient(g) => match invert(g.transform) {
            Some(m) => Some((img.transform.then(m), g)),
            None => return false,
        },
        Source::Solid(_) => None,
    };
    for (i, p) in px.as_chunks_mut::<4>().0.iter_mut().enumerate() {
        if p[3] == 0 {
            continue;
        }
        let (s, sa) = match (solid, &to_unit) {
            (Some(s), _) => s,
            (None, Some((m, g))) => {
                let q = m.apply(Point { x: (i % width) as f64 + 0.5, y: (i / width) as f64 + 0.5 });
                let c = sample(g, q);
                (rgb(c), c.alpha())
            }
            (None, None) => continue,
        };
        let k = (o.opacity * sa).clamp(0.0, 1.0);
        let b = [p[0], p[1], p[2]].map(|v| f64::from(v) / 255.0);
        let m = blend(o.blend, b, s);
        for c in 0..3 {
            p[c] = ((b[c] + (m[c] - b[c]) * k).clamp(0.0, 1.0) * 255.0).round() as u8;
        }
    }
    img.pixels = Pixels::Rgba8(px);
    true
}

/// The gradient's colour at unit-space point `q`.
fn sample(g: &Gradient, q: Point) -> Color {
    let t = match g.kind {
        GradientKind::Linear => q.x,
        GradientKind::Radial => q.x.hypot(q.y),
        GradientKind::Conical => q.y.atan2(q.x).rem_euclid(std::f64::consts::TAU) / std::f64::consts::TAU,
    };
    let t = if t.is_finite() { t.clamp(0.0, 1.0) } else { 0.0 };
    let (Some(first), Some(last)) = (g.stops.first(), g.stops.last()) else { return Color::Rgb { r: 0.0, g: 0.0, b: 0.0, a: 0.0 } };
    if t <= first.offset {
        return first.color;
    }
    for w in g.stops.windows(2) {
        let [a, b] = [&w[0], &w[1]];
        if t <= b.offset {
            let span = b.offset - a.offset;
            let u = if span > 1e-9 { (t - a.offset) / span } else { 1.0 };
            // Affinity's midpoint bias (see `Stop::half_point`).
            let k = 1.0 + 8.0 * (a.bias - 0.5).abs();
            let u = if a.bias >= 0.5 { u.powf(k) } else { 1.0 - (1.0 - u).powf(k) };
            let (x, y) = (rgb(a.color), rgb(b.color));
            let [r, g, bl] = [0, 1, 2].map(|i| x[i] + (y[i] - x[i]) * u);
            return Color::Rgb { r, g, b: bl, a: a.color.alpha() + (b.color.alpha() - a.color.alpha()) * u };
        }
    }
    last.color
}

fn invert(m: Affine) -> Option<Affine> {
    let [a, b, c, d, e, f] = m.0;
    let det = a * d - b * c;
    if !(det.is_finite() && det.abs() > 1e-12) {
        return None;
    }
    let (ia, ib, ic, id) = (d / det, -b / det, -c / det, a / det);
    Some(Affine([ia, ib, ic, id, -(ia * e + ic * f), -(ib * e + id * f)]))
}

/// The blend of backdrop `b` and source `s` (sRGB, 0..1).
fn blend(mode: Blend, b: [f64; 3], s: [f64; 3]) -> [f64; 3] {
    use Blend::*;
    let sep = |f: fn(f64, f64) -> f64| [0, 1, 2].map(|i| f(b[i], s[i]).clamp(0.0, 1.0));
    match mode {
        Normal | PassThrough | Erase => s,
        Darken => sep(f64::min),
        Lighten => sep(f64::max),
        Multiply => sep(|b, s| b * s),
        Screen => sep(screen),
        ColorBurn => sep(burn),
        ColorDodge => sep(dodge),
        Blend::Overlay => sep(|b, s| hard_light(s, b)),
        HardLight => sep(hard_light),
        SoftLight => sep(soft_light),
        Difference => sep(|b, s| (b - s).abs()),
        Exclusion => sep(|b, s| b + s - 2.0 * b * s),
        Add => sep(|b, s| b + s),
        Subtract => sep(|b, s| b - s),
        VividLight => sep(|b, s| if s <= 0.5 { burn(b, 2.0 * s) } else { dodge(b, 2.0 * s - 1.0) }),
        LinearLight => sep(|b, s| b + 2.0 * s - 1.0),
        PinLight => sep(|b, s| if s <= 0.5 { b.min(2.0 * s) } else { b.max(2.0 * s - 1.0) }),
        HardMix => sep(|b, s| if b + s >= 1.0 { 1.0 } else { 0.0 }),
        Average => sep(|b, s| (b + s) / 2.0),
        Negation => sep(|b, s| 1.0 - (1.0 - b - s).abs()),
        Reflect => sep(reflect),
        Glow => sep(|b, s| reflect(s, b)),
        DarkerColor => {
            if lum(s) < lum(b) {
                s
            } else {
                b
            }
        }
        LighterColor => {
            if lum(s) > lum(b) {
                s
            } else {
                b
            }
        }
        Hue => set_lum(set_sat(s, sat(b)), lum(b)),
        Saturation => set_lum(set_sat(b, sat(s)), lum(b)),
        Blend::Color => set_lum(s, lum(b)),
        Luminosity => set_lum(b, lum(s)),
    }
}

fn screen(b: f64, s: f64) -> f64 {
    b + s - b * s
}

fn burn(b: f64, s: f64) -> f64 {
    if b >= 1.0 {
        1.0
    } else if s <= 0.0 {
        0.0
    } else {
        1.0 - ((1.0 - b) / s).min(1.0)
    }
}

fn dodge(b: f64, s: f64) -> f64 {
    if b <= 0.0 {
        0.0
    } else if s >= 1.0 {
        1.0
    } else {
        (b / (1.0 - s)).min(1.0)
    }
}

fn hard_light(b: f64, s: f64) -> f64 {
    if s <= 0.5 { b * 2.0 * s } else { screen(b, 2.0 * s - 1.0) }
}

fn soft_light(b: f64, s: f64) -> f64 {
    if s <= 0.5 {
        b - (1.0 - 2.0 * s) * b * (1.0 - b)
    } else {
        let d = if b <= 0.25 { ((16.0 * b - 12.0) * b + 4.0) * b } else { b.sqrt() };
        b + (2.0 * s - 1.0) * (d - b)
    }
}

fn reflect(b: f64, s: f64) -> f64 {
    if s >= 1.0 { 1.0 } else { (b * b / (1.0 - s)).min(1.0) }
}

fn lum(c: [f64; 3]) -> f64 {
    0.3 * c[0] + 0.59 * c[1] + 0.11 * c[2]
}

fn clip_color(c: [f64; 3]) -> [f64; 3] {
    let l = lum(c);
    let n = c[0].min(c[1]).min(c[2]);
    let x = c[0].max(c[1]).max(c[2]);
    c.map(|v| {
        let mut v = v;
        if n < 0.0 && l - n > 1e-12 {
            v = l + (v - l) * l / (l - n);
        }
        if x > 1.0 && x - l > 1e-12 {
            v = l + (v - l) * (1.0 - l) / (x - l);
        }
        v.clamp(0.0, 1.0)
    })
}

fn set_lum(c: [f64; 3], l: f64) -> [f64; 3] {
    let d = l - lum(c);
    clip_color(c.map(|v| v + d))
}

fn sat(c: [f64; 3]) -> f64 {
    c[0].max(c[1]).max(c[2]) - c[0].min(c[1]).min(c[2])
}

fn set_sat(c: [f64; 3], s: f64) -> [f64; 3] {
    let max = c[0].max(c[1]).max(c[2]);
    let min = c[0].min(c[1]).min(c[2]);
    if max - min <= 1e-12 {
        return [0.0; 3];
    }
    c.map(|v| (v - min) * s / (max - min))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn solid(c: Color, opacity: f64, blend: Blend) -> Node {
        Node {
            class: crate::stream::Tag::of(b"ShpN"),
            name: String::new(),
            visible: true,
            locked: false,
            opacity: 1.0,
            blend: Blend::Normal,
            kind: Kind::Shape {
                path: Default::default(),
                fills: vec![Paint::Solid(Color::Rgb { r: 1.0, g: 0.5, b: 0.0, a: 1.0 })],
                strokes: vec![],
                even_odd: false,
            },
            mask: None,
            pixel_mask: None,
            effects: vec![Effect::ColorOverlay { color: c, opacity, blend }],
            children: vec![],
        }
    }

    fn fill(n: &Node) -> Color {
        match &n.kind {
            Kind::Shape { fills, .. } => match fills.first() {
                Some(Paint::Solid(c)) => *c,
                other => panic!("{other:?}"),
            },
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn a_full_colour_overlay_replaces_the_fill_and_keeps_its_alpha() {
        let white = Color::Rgb { r: 1.0, g: 1.0, b: 1.0, a: 1.0 };
        let n = apply(&solid(white, 1.0, Blend::Normal), &mut |_| {}).into_owned();
        assert_eq!(fill(&n), white);
        assert!(n.effects.is_empty(), "the overlay is baked, not kept");
        // Half way, mixed in RGB.
        let n = apply(&solid(white, 0.5, Blend::Normal), &mut |_| {}).into_owned();
        assert_eq!(fill(&n), Color::Rgb { r: 1.0, g: 0.75, b: 0.5, a: 1.0 });
    }

    #[test]
    fn blend_modes_follow_their_formulas() {
        let b = [0.25, 0.5, 1.0];
        assert_eq!(blend(Blend::Multiply, b, [0.5; 3]), [0.125, 0.25, 0.5]);
        assert_eq!(blend(Blend::Screen, b, [0.5; 3]), [0.625, 0.75, 1.0]);
        assert_eq!(blend(Blend::VividLight, [0.5; 3], [0.75; 3]), [1.0; 3]);
        assert_eq!(blend(Blend::HardMix, [0.5; 3], [0.6; 3]), [1.0; 3]);
        // Color keeps the backdrop's luminosity with the source's hue and saturation.
        let c = blend(Blend::Color, [0.5; 3], [1.0, 0.0, 0.0]);
        assert!((lum(c) - 0.5).abs() < 1e-9 && c[0] > c[1] && (c[1] - c[2]).abs() < 1e-9, "{c:?}");
    }

    #[test]
    fn overlays_recolour_pixels_inside_their_alpha() {
        let mut n = solid(Color::Rgb { r: 0.0, g: 0.0, b: 1.0, a: 1.0 }, 1.0, Blend::Normal);
        n.kind = Kind::Image(Image { width: 2, height: 1, pixels: Pixels::Rgba8(vec![255, 0, 0, 255, 255, 0, 0, 0]), transform: Affine::IDENTITY });
        let out = apply(&n, &mut |_| {}).into_owned();
        let Kind::Image(img) = &out.kind else { panic!() };
        assert_eq!(img.pixels, Pixels::Rgba8(vec![0, 0, 255, 255, 255, 0, 0, 0]));
    }

    #[test]
    fn a_gradient_overlay_over_one_colour_becomes_that_gradient() {
        let g = Gradient {
            kind: GradientKind::Linear,
            stops: vec![
                Stop { offset: 0.0, color: Color::Rgb { r: 0.0, g: 0.0, b: 0.0, a: 1.0 }, bias: 0.5 },
                Stop { offset: 1.0, color: Color::Rgb { r: 1.0, g: 1.0, b: 1.0, a: 1.0 }, bias: 0.5 },
            ],
            transform: Affine([10.0, 0.0, 0.0, 10.0, 0.0, 0.0]),
        };
        let mut n = solid(Color::Rgb { r: 0.0, g: 0.0, b: 0.0, a: 1.0 }, 1.0, Blend::Normal);
        n.effects = vec![Effect::GradientOverlay { gradient: g.clone(), opacity: 1.0, blend: Blend::Normal }];
        let out = apply(&n, &mut |_| {}).into_owned();
        let Kind::Shape { fills, .. } = &out.kind else { panic!() };
        assert_eq!(fills.first(), Some(&Paint::Gradient(g)));
    }
}
