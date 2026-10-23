//! Gradients written in their inks. A gradient whose stops name a spot swatch shades in that
//! spot: one spot with paper or tints of itself is a Separation shading, a spot with process
//! inks or a second spot a DeviceN shading (process colorants in CMYK order, then the spots).
//! In print, a process gradient with a `[Paper]` stop shades in DeviceCMYK. An overprinting
//! gradient of fewer than four process inks is a DeviceN shading of just those inks: nonzero
//! overprint mode doesn't apply to shadings (ISO 32000-2 §8.6.7), so a DeviceCMYK shading's
//! zero components would knock out the plates beneath.

use craft_pdf::write::backend as krilla;
use designcraft_color::swatch::{ColorType, SwatchValue, apply_tint};
use designcraft_color::{Color, Gradient};
use krilla::color::device_n::{self, Colorant, DeviceNSpace, Process};
use krilla::color::separation::{self, SeparationColorant, SeparationSpace};
use krilla::paint::Stop;

use crate::export::{Exporter, norm, q, regular};

/// A stop as inks.
enum Ink {
    /// CMYK tints (0..=1); `[Paper]` is all zero.
    Process([f32; 4]),
    Spot {
        name: String,
        alternate: Color,
        tint: f32,
    },
}

/// Inks of a plain color: CMYK as it is, gray as black. RGB has none.
fn process_inks(c: Color) -> Option<[f32; 4]> {
    let tint = |v: f32| if v.is_finite() { v.clamp(0.0, 1.0) } else { 0.0 };
    match c {
        Color::Cmyk { c, m, y, k } => Some([tint(c), tint(m), tint(y), tint(k)]),
        Color::Gray { k } => Some([0.0, 0.0, 0.0, tint(k)]),
        Color::Rgb { .. } => None,
    }
}

/// The shading's color space.
enum Space {
    Separation(SeparationSpace),
    DeviceN(DeviceNSpace),
}

impl Exporter<'_> {
    /// The inks a stop prints with, following tints and the Ink Manager as solid fills do.
    /// `None` when they can't go into an ink shading (RGB, `[Registration]`).
    fn stop_ink(&self, stop: &designcraft_color::GradientStop) -> Option<Ink> {
        let Some(mut n) = stop.swatch.as_deref() else { return process_inks(stop.color).map(Ink::Process) };
        let mut t = 1.0f32;
        for _ in 0..8 {
            // A swatch that's gone leaves the stop's own color.
            let Some(sw) = self.doc.swatch(n) else { return process_inks(stop.color).map(Ink::Process) };
            match &sw.value {
                SwatchValue::Paper { .. } => return Some(Ink::Process([0.0; 4])),
                SwatchValue::Tint { base, tint } => {
                    t *= tint;
                    n = base;
                }
                SwatchValue::Color { color_type: ColorType::Spot, .. } => {
                    let (ink, process) = self.doc.inks.resolve(&sw.name);
                    let target =
                        self.doc.swatch(ink).filter(|w| matches!(w.value, SwatchValue::Color { color_type: ColorType::Spot, .. })).unwrap_or(sw);
                    let SwatchValue::Color { color, .. } = &target.value else { return None };
                    if process {
                        return process_inks(apply_tint(*color, t)).map(Ink::Process);
                    }
                    let tint = if t.is_finite() { t.clamp(0.0, 1.0) } else { 1.0 };
                    return Some(Ink::Spot { name: target.name.clone(), alternate: *color, tint });
                }
                SwatchValue::Color { color, .. } => return process_inks(apply_tint(*color, t)).map(Ink::Process),
                SwatchValue::Registration | SwatchValue::None | SwatchValue::Gradient { .. } => return None,
            }
        }
        None
    }

    /// The gradient's stops in its inks, or `None` to write it in device colors (no spot, or
    /// inks that can't share one shading space, which is warned about). `overprint`: the paint
    /// overprints.
    pub(crate) fn ink_gradient_stops(&mut self, g: &Gradient, swatch: &str, overprint: bool) -> Option<Vec<Stop>> {
        let overprint = overprint && !self.rgb_only;
        // Plain-color stops (all older documents) keep their device colors.
        if !overprint && g.stops.iter().all(|s| s.swatch.is_none()) {
            return None;
        }
        let inks: Vec<Option<Ink>> = g.stops.iter().map(|s| self.stop_ink(s)).collect();
        let has_spot = inks.iter().any(|i| matches!(i, Some(Ink::Spot { .. })));
        let on_paper = g.stops.iter().any(|s| s.swatch.as_deref() == Some(designcraft_color::swatch::PAPER));
        if !has_spot && !overprint && !(on_paper && self.for_print() && !self.rgb_only) {
            return None;
        }
        let Some(inks) = inks.into_iter().collect::<Option<Vec<Ink>>>() else {
            return self.unshaded(swatch, "a stop has no inks (RGB or [Registration])", has_spot);
        };
        // Spots in the order the stops name them; process inks a stop sets.
        let mut spots: Vec<(String, Color)> = Vec::new();
        let mut process = [false; 4];
        for i in &inks {
            match i {
                Ink::Process(v) => {
                    for (used, v) in process.iter_mut().zip(v) {
                        *used |= *v > 1e-6;
                    }
                }
                Ink::Spot { name, alternate, .. } => {
                    if !spots.iter().any(|(n, _)| n == name) {
                        spots.push((name.clone(), *alternate));
                    }
                }
            }
        }
        let process: Vec<usize> = (0..4).filter(|j| process.get(*j).copied().unwrap_or(false)).collect();
        let space = match (spots.as_slice(), process.is_empty()) {
            // All four inks are painted either way.
            ([], false) if overprint && process.len() < 4 => {
                let colorants: Vec<Colorant> = process.iter().filter_map(|j| Process::ALL.get(*j).copied().map(Colorant::Process)).collect();
                match DeviceNSpace::new(colorants) {
                    Some(s) => Some(Space::DeviceN(s)),
                    None => return self.unshaded(swatch, "its inks don't fit one DeviceN space", false),
                }
            }
            ([], _) => None,
            ([(name, alternate)], true) => {
                Some(Space::Separation(SeparationSpace::new(SeparationColorant::Custom(name.clone()), regular(alternate, self.rgb_only))))
            }
            _ => {
                if self.rgb_only {
                    return self.unshaded(swatch, "DeviceN isn't allowed in PDF/A", true);
                }
                let mut colorants: Vec<Colorant> = process.iter().filter_map(|j| Process::ALL.get(*j).copied().map(Colorant::Process)).collect();
                for (name, alternate) in &spots {
                    let Color::Cmyk { c, m, y, k } = *alternate else {
                        return self.unshaded(swatch, &format!("'{name}' has no CMYK values"), true);
                    };
                    colorants.push(Colorant::Spot { name: name.clone(), alternate: krilla::color::cmyk::Color::new(q(c), q(m), q(y), q(k)) });
                }
                match DeviceNSpace::new(colorants) {
                    Some(s) => Some(Space::DeviceN(s)),
                    None => return self.unshaded(swatch, "its inks don't fit one DeviceN space", true),
                }
            }
        };
        // Tints per colorant at each authored stop.
        let tints: Vec<Vec<f32>> = inks
            .iter()
            .map(|i| {
                let mut v = vec![0.0f32; process.len() + spots.len()];
                match i {
                    Ink::Process(p) => {
                        for (slot, j) in v.iter_mut().zip(&process) {
                            *slot = p.get(*j).copied().unwrap_or(0.0);
                        }
                    }
                    Ink::Spot { name, tint, .. } => {
                        if let Some(slot) = spots.iter().position(|(n, _)| n == name).and_then(|k| v.get_mut(process.len() + k)) {
                            *slot = *tint;
                        }
                    }
                }
                v
            })
            .collect();
        // A midpoint becomes a stop halfway between its neighbours' tints, as the display does.
        let mut expanded: Vec<(f32, Vec<f32>, f32)> = Vec::with_capacity(tints.len() * 2);
        for (k, (s, v)) in g.stops.iter().zip(&tints).enumerate() {
            expanded.push((s.offset, v.clone(), s.opacity));
            if let (Some(n), Some(nv)) = (g.stops.get(k + 1), tints.get(k + 1))
                && (s.midpoint - 0.5).abs() > 1e-3
            {
                let mid: Vec<f32> = v.iter().zip(nv).map(|(a, b)| (a + b) / 2.0).collect();
                expanded.push((s.offset + (n.offset - s.offset) * s.midpoint, mid, (s.opacity + n.opacity) / 2.0));
            }
        }
        let mut stops = Vec::with_capacity(expanded.len());
        let mut last = 0.0f32;
        for (o, v, a) in expanded {
            let o = if o.is_finite() { o.clamp(last, 1.0) } else { last };
            last = o;
            let color: krilla::color::Color = match &space {
                Some(Space::Separation(sp)) => separation::Color::new(q(v.first().copied().unwrap_or(0.0)), sp.clone()).into(),
                Some(Space::DeviceN(sp)) => device_n::Color::new(v.iter().map(|t| q(*t)).collect(), sp.clone())?.into(),
                // Process inks only (a gradient to [Paper] in print, or an overprinting one of
                // all four inks).
                None => {
                    let mut cmyk = [0.0f32; 4];
                    for (t, j) in v.iter().zip(&process) {
                        if let Some(slot) = cmyk.get_mut(*j) {
                            *slot = *t;
                        }
                    }
                    let [c, m, y, k] = cmyk;
                    krilla::color::cmyk::Color::new(q(c), q(m), q(y), q(k)).into()
                }
            };
            stops.push(Stop { offset: norm(o), color, opacity: norm(a) });
        }
        (!stops.is_empty()).then_some(stops)
    }

    /// Falls back to device colors, warning when that loses a spot ink.
    fn unshaded(&mut self, swatch: &str, why: &str, has_spot: bool) -> Option<Vec<Stop>> {
        if has_spot {
            self.warn(format!("gradient '{swatch}': spot inks printed as process color ({why})"));
        }
        None
    }
}
