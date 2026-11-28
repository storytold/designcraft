//! Ruby settings: the readings set beside parent text and how they are placed, sized and
//! coloured (the four Ruby panes of the Character panel menu and of style options).
//!
//! The values live as `ruby*` fields of [`crate::CharAttrs`] so styles inherit them one by one.
//! [`RubySpec`] gathers the resolved values of a run, clamped to safe ranges, for layout.
use serde::{Deserialize, Serialize};

use crate::CharProps;
use crate::cjk_settings::AdornmentOverprint;

/// Mono ruby gives each parent character its own reading; group ruby sets one reading over the
/// whole run.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum RubyType {
    #[default]
    Group,
    /// Per-character (mono) ruby: the readings of the parent characters, in order, separated by
    /// U+3000 IDEOGRAPHIC SPACE (as IDML stores them).
    PerCharacter,
}

/// How a ruby shorter than its parent is spread over it.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum RubyAlignment {
    /// Flush with the parent's start (top in vertical text).
    Left,
    Center,
    /// Flush with the parent's end (bottom in vertical text).
    Right,
    /// First and last ruby characters flush with the parent's ends, the rest spread evenly.
    FullJustify,
    /// JIS 1-2-1 rule: the space at each end is half the space between ruby characters.
    #[default]
    Jis,
    /// The space at each end equals the space between ruby characters.
    EqualAki,
    /// Half a ruby character inset at each end, the rest spread between ruby characters.
    OneAki,
}

/// Above the parent in horizontal text (right of it in vertical text), or below (left).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum RubyPosition {
    #[default]
    AboveRight,
    BelowLeft,
}

/// How far a ruby longer than its parent may extend over the neighbouring characters.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum RubyOverhang {
    None,
    /// One ruby character.
    #[default]
    OneRuby,
    HalfRuby,
    /// One parent character.
    OneChar,
    HalfChar,
    NoLimit,
}

/// How the parent characters are spaced out when the ruby is longer than they are.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum RubyParentSpacing {
    NoAdjustment,
    /// Space before the first and after the last parent character.
    BothSides,
    /// 1-2-1: the space at each end is half the space between parent characters.
    #[default]
    Aki121,
    /// The space at each end equals the space between parent characters.
    EqualAki,
    /// Space only between parent characters.
    FullJustify,
}

/// Longest ruby reading laid out, in characters.
pub const MAX_RUBY_CHARS: usize = 256;

/// A run's resolved ruby settings, every number finite and inside the range the controls allow.
#[derive(Clone, Debug, PartialEq)]
pub struct RubySpec {
    pub kind: RubyType,
    pub alignment: RubyAlignment,
    pub position: RubyPosition,
    /// Points along the line, and away from the parent text across it.
    pub x_offset: f64,
    pub y_offset: f64,
    /// Empty: the parent's font. An empty style with a family set is that family's Regular.
    pub font_family: String,
    pub font_style: String,
    /// Points; `None` is half the parent size.
    pub size: Option<f64>,
    /// 1.0 = 100 %.
    pub x_scale: f64,
    pub y_scale: f64,
    /// Use the font's ruby glyphs (OpenType `ruby`).
    pub open_type_pro: bool,
    /// Tate-chu-yoko inside vertical ruby: runs of up to this many digits (0 = off), Latin
    /// letters too when `auto_tcy_include_roman`, squeezed to one ruby em when
    /// `auto_tcy_auto_scale`.
    pub auto_tcy_digits: u32,
    pub auto_tcy_include_roman: bool,
    pub auto_tcy_auto_scale: bool,
    pub overhang: RubyOverhang,
    pub parent_spacing: RubyParentSpacing,
    /// At a line's start or end, a longer ruby is set flush with the line edge.
    pub auto_align: bool,
    /// A longer ruby is first narrowed, down to `scaling_min` of its width.
    pub auto_scaling: bool,
    pub scaling_min: f64,
    /// Empty: the text's colour (and its tint when the tint is `None`).
    pub fill: String,
    pub fill_tint: Option<f32>,
    pub stroke: String,
    pub stroke_tint: Option<f32>,
    /// `None`: the text's stroke weight.
    pub stroke_weight: Option<f64>,
    pub overprint_fill: AdornmentOverprint,
    pub overprint_stroke: AdornmentOverprint,
}

fn finite_or(v: f64, default: f64) -> f64 {
    if v.is_finite() { v } else { default }
}

fn tint(v: Option<f32>) -> Option<f32> {
    v.filter(|t| t.is_finite()).map(|t| t.clamp(0.0, 1.0))
}

impl RubySpec {
    /// The ruby settings of resolved character properties.
    pub fn of(p: &CharProps) -> RubySpec {
        RubySpec {
            kind: p.ruby_type,
            alignment: p.ruby_alignment,
            position: p.ruby_position,
            x_offset: finite_or(p.ruby_x_offset, 0.0).clamp(-1000.0, 1000.0),
            y_offset: finite_or(p.ruby_y_offset, 0.0).clamp(-1000.0, 1000.0),
            font_family: p.ruby_font.clone(),
            font_style: p.ruby_font_style.clone(),
            size: p.ruby_font_size.filter(|s| s.is_finite() && *s > 0.0).map(|s| s.clamp(0.1, 1296.0)),
            x_scale: finite_or(p.ruby_x_scale, 1.0).clamp(0.01, 10.0),
            y_scale: finite_or(p.ruby_y_scale, 1.0).clamp(0.01, 10.0),
            open_type_pro: p.ruby_open_type_pro,
            auto_tcy_digits: p.ruby_auto_tcy_digits.min(9),
            auto_tcy_include_roman: p.ruby_auto_tcy_include_roman,
            auto_tcy_auto_scale: p.ruby_auto_tcy_auto_scale,
            overhang: p.ruby_overhang_amount,
            parent_spacing: p.ruby_parent_spacing,
            auto_align: p.ruby_auto_align,
            auto_scaling: p.ruby_auto_scaling,
            scaling_min: finite_or(p.ruby_scaling_min, 0.66).clamp(0.1, 1.0),
            fill: p.ruby_fill.clone(),
            fill_tint: tint(p.ruby_fill_tint),
            stroke: p.ruby_stroke.clone(),
            stroke_tint: tint(p.ruby_stroke_tint),
            stroke_weight: p.ruby_stroke_weight.filter(|w| w.is_finite()).map(|w| w.clamp(0.0, 800.0)),
            overprint_fill: p.ruby_overprint_fill,
            overprint_stroke: p.ruby_overprint_stroke,
        }
    }

    /// The ruby size for parent text of `parent_size` points.
    pub fn size_for(&self, parent_size: f64) -> f64 {
        self.size.unwrap_or(parent_size * 0.5)
    }
}

/// The readings of a mono ruby, one per parent character, or `None` when their number doesn't
/// match `parents` (the ruby is then set as a group).
pub fn mono_readings(text: &str, parents: usize) -> Option<Vec<&str>> {
    let parts: Vec<&str> = text.split('\u{3000}').collect();
    (parts.len() == parents && parents > 0).then_some(parts)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn spec_keeps_hostile_numbers_in_range() {
        let p = CharProps {
            ruby_font_size: Some(f64::NAN),
            ruby_x_scale: f64::INFINITY,
            ruby_y_scale: -3.0,
            ruby_x_offset: 1e300,
            ruby_scaling_min: 0.0,
            ruby_auto_tcy_digits: u32::MAX,
            ruby_fill_tint: Some(7.0),
            ruby_stroke_weight: Some(f64::NEG_INFINITY),
            ..CharProps::default()
        };
        let s = RubySpec::of(&p);
        assert_eq!(s.size, None);
        assert_eq!(s.size_for(12.0), 6.0);
        assert_eq!((s.x_scale, s.y_scale, s.x_offset, s.scaling_min), (1.0, 0.01, 1000.0, 0.1));
        assert_eq!((s.auto_tcy_digits, s.fill_tint, s.stroke_weight), (9, Some(1.0), None));
    }

    #[test]
    fn mono_readings_need_one_per_character() {
        assert_eq!(mono_readings("かん\u{3000}じ", 2), Some(vec!["かん", "じ"]));
        assert_eq!(mono_readings("かんじ", 2), None);
    }
}
