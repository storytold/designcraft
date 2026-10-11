//! Document swatches with InDesign semantics: named swatches that page items reference by name,
//! the four special swatches (`[None]`, `[Paper]`, `[Black]`, `[Registration]`), process vs spot
//! colours, tints of a base swatch and gradient swatches. The default set's composition is ours;
//! the CMYK naming convention (`C=100 M=0 Y=0 K=0`) is a plain description of the ink values.

use serde::{Deserialize, Serialize};

use crate::{Color, Gradient};

pub const NONE: &str = "[None]";
pub const PAPER: &str = "[Paper]";
pub const BLACK: &str = "[Black]";
pub const REGISTRATION: &str = "[Registration]";

/// Process colours print as CMYK separations; spot colours on their own plate.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ColorType {
    #[default]
    Process,
    Spot,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum SwatchValue {
    None,
    Paper {
        color: Color,
    },
    Registration,
    Color {
        color: Color,
        color_type: ColorType,
    },
    /// A tint (0..=1) of another colour swatch.
    Tint {
        base: String,
        tint: f32,
    },
    Gradient {
        gradient: Gradient,
    },
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Swatch {
    pub name: String,
    pub value: SwatchValue,
    /// Special swatches can't be deleted or renamed.
    #[serde(default)]
    pub locked: bool,
    /// User-named (false = the name is derived from the colour values and tracks edits).
    #[serde(default)]
    pub named: bool,
    /// An unnamed colour (mixed in the Color panel or picker): applied to objects but not
    /// listed in the Swatches panel until Add to Swatches.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub hidden: bool,
}

impl Swatch {
    pub fn color(name: &str, c: Color) -> Self {
        Swatch {
            name: name.into(),
            value: SwatchValue::Color { color: c, color_type: ColorType::Process },
            locked: false,
            named: true,
            hidden: false,
        }
    }
    pub fn cmyk(c: f32, m: f32, y: f32, k: f32) -> Self {
        let col = Color::cmyk(c, m, y, k);
        Swatch {
            name: cmyk_name(c, m, y, k),
            value: SwatchValue::Color { color: col, color_type: ColorType::Process },
            locked: false,
            named: false,
            hidden: false,
        }
    }
    pub fn is_special(&self) -> bool {
        matches!(self.value, SwatchValue::None | SwatchValue::Paper { .. } | SwatchValue::Registration) || self.name == BLACK
    }
}

/// `C=100 M=0 Y=0 K=0`.
pub fn cmyk_name(c: f32, m: f32, y: f32, k: f32) -> String {
    let p = |v: f32| (v * 100.0).round() as i32;
    format!("C={} M={} Y={} K={}", p(c), p(m), p(y), p(k))
}

/// The name Name with Color Value gives a colour: `C=100 M=0 Y=0 K=0`, or `R=16 G=128 B=128`.
pub fn value_name(c: Color) -> String {
    match c {
        Color::Cmyk { c, m, y, k } => cmyk_name(c, m, y, k),
        other => {
            let [r, g, b] = other.to_rgb();
            format!("R={} G={} B={}", (r * 255.0).round(), (g * 255.0).round(), (b * 255.0).round())
        }
    }
}

/// Resolve a swatch name + tint into a display colour. `None` for `[None]` or unknown names.
/// Gradients resolve to their first stop (callers that render gradients use [`resolve_gradient`]).
pub fn resolve(swatches: &[Swatch], name: &str, tint: f32) -> Option<Color> {
    resolve_depth(swatches, name, tint, 0)
}

fn resolve_depth(swatches: &[Swatch], name: &str, tint: f32, depth: u8) -> Option<Color> {
    if depth > 8 {
        return None;
    }
    let s = swatches.iter().find(|s| s.name == name)?;
    let base = match &s.value {
        SwatchValue::None => return None,
        SwatchValue::Paper { color } => return Some(*color),
        SwatchValue::Registration => Color::cmyk(1.0, 1.0, 1.0, 1.0),
        SwatchValue::Color { color, .. } => *color,
        SwatchValue::Tint { base, tint: t } => return resolve_depth(swatches, base, t * tint, depth + 1),
        SwatchValue::Gradient { gradient } => gradient.stops.first().map(|s| s.color)?,
    };
    Some(apply_tint(base, tint))
}

pub fn resolve_gradient<'a>(swatches: &'a [Swatch], name: &str) -> Option<&'a Gradient> {
    match &swatches.iter().find(|s| s.name == name)?.value {
        SwatchValue::Gradient { gradient } => Some(gradient),
        _ => None,
    }
}

/// Tint = ink percentage: CMYK/Gray components scale; RGB mixes towards white.
pub fn apply_tint(c: Color, tint: f32) -> Color {
    let t = tint.clamp(0.0, 1.0);
    if (t - 1.0).abs() < f32::EPSILON {
        return c;
    }
    match c {
        Color::Cmyk { c, m, y, k } => Color::cmyk(c * t, m * t, y * t, k * t),
        Color::Gray { k } => Color::gray(k * t),
        Color::Rgb { r, g, b } => Color::rgb(1.0 - (1.0 - r) * t, 1.0 - (1.0 - g) * t, 1.0 - (1.0 - b) * t),
    }
}

/// A new document's swatches, in panel order.
pub fn default_swatches() -> Vec<Swatch> {
    let special = |name: &str, value: SwatchValue| Swatch { name: name.into(), value, locked: true, named: true, hidden: false };
    let mut v = vec![
        special(NONE, SwatchValue::None),
        special(REGISTRATION, SwatchValue::Registration),
        special(PAPER, SwatchValue::Paper { color: Color::WHITE }),
        special(BLACK, SwatchValue::Color { color: Color::cmyk(0.0, 0.0, 0.0, 1.0), color_type: ColorType::Process }),
    ];
    for (c, m, y, k) in [
        (1.0, 0.0, 0.0, 0.0),
        (0.0, 1.0, 0.0, 0.0),
        (0.0, 0.0, 1.0, 0.0),
        (0.0, 1.0, 1.0, 0.0),
        (1.0, 0.0, 1.0, 0.0),
        (1.0, 1.0, 0.0, 0.0),
        (0.15, 1.0, 1.0, 0.0),
        (0.75, 0.05, 1.0, 0.0),
        (1.0, 0.9, 0.1, 0.0),
    ] {
        v.push(Swatch::cmyk(c, m, y, k));
    }
    v
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_start_with_specials() {
        let s = default_swatches();
        assert_eq!(s[0].name, NONE);
        assert_eq!(s[3].name, BLACK);
        assert!(s[..4].iter().all(|s| s.locked));
        assert!(s.iter().any(|s| s.name == "C=100 M=0 Y=0 K=0"));
    }

    #[test]
    fn resolves_tints_and_specials() {
        let mut s = default_swatches();
        s.push(Swatch {
            name: "Half cyan".into(),
            value: SwatchValue::Tint { base: "C=100 M=0 Y=0 K=0".into(), tint: 0.5 },
            locked: false,
            named: true,
            hidden: false,
        });
        assert_eq!(resolve(&s, NONE, 1.0), None);
        assert_eq!(resolve(&s, PAPER, 1.0), Some(Color::WHITE));
        assert_eq!(resolve(&s, BLACK, 0.4), Some(Color::cmyk(0.0, 0.0, 0.0, 0.4)));
        assert_eq!(resolve(&s, "Half cyan", 0.5), Some(Color::cmyk(0.25, 0.0, 0.0, 0.0)));
        assert_eq!(resolve(&s, "missing", 1.0), None);
    }

    #[test]
    fn tint_cycles_terminate() {
        let s = vec![
            Swatch { name: "a".into(), value: SwatchValue::Tint { base: "b".into(), tint: 0.5 }, locked: false, named: true, hidden: false },
            Swatch { name: "b".into(), value: SwatchValue::Tint { base: "a".into(), tint: 0.5 }, locked: false, named: true, hidden: false },
        ];
        assert_eq!(resolve(&s, "a", 1.0), None);
    }

    #[test]
    fn rgb_tint_mixes_to_white() {
        let c = apply_tint(Color::rgb(1.0, 0.0, 0.0), 0.5);
        assert_eq!(c, Color::rgb(1.0, 0.5, 0.5));
    }
}
