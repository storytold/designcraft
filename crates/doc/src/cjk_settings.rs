//! Value types of the Japanese typesetting settings beyond the basic CJK attributes in
//! [`crate::cjk`]: kenten (圏点, emphasis marks) and their colour, and the overprint choice they
//! share with other adornments. Shatai, auto tate-chu-yoko and the grid settings are plain numbers
//! and switches on [`crate::CharAttrs`] / [`crate::ParaAttrs`].
use serde::{Deserialize, Serialize};

/// The kenten mark set beside each character (Kenten Settings › Kenten Type).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum KentenKind {
    /// Sesame dot (ゴマ), the usual Japanese mark.
    #[default]
    SesameDot,
    /// White sesame dot (白ゴマ).
    WhiteSesameDot,
    /// Fisheye (蛇の目): a black circle in a ring.
    Fisheye,
    BlackCircle,
    SmallBlackCircle,
    /// Bullseye (二重丸): two rings.
    Bullseye,
    BlackTriangle,
    WhiteTriangle,
    WhiteCircle,
    SmallWhiteCircle,
    /// [`crate::CharProps::kenten_character`] in [`crate::CharProps::kenten_font`].
    Custom,
}

impl KentenKind {
    /// Every kind, in menu order.
    pub const ALL: [KentenKind; 11] = [
        KentenKind::SesameDot,
        KentenKind::WhiteSesameDot,
        KentenKind::Fisheye,
        KentenKind::BlackCircle,
        KentenKind::SmallBlackCircle,
        KentenKind::Bullseye,
        KentenKind::BlackTriangle,
        KentenKind::WhiteTriangle,
        KentenKind::WhiteCircle,
        KentenKind::SmallWhiteCircle,
        KentenKind::Custom,
    ];

    /// The character drawn for a preset kind (`None` for [`KentenKind::Custom`]). The marks are
    /// the emphasis characters of JIS X 4051 / W3C JLReq; the small circles are the bullet forms.
    pub fn mark(self) -> Option<&'static str> {
        Some(match self {
            KentenKind::SesameDot => "\u{FE45}",
            KentenKind::WhiteSesameDot => "\u{FE46}",
            KentenKind::Fisheye => "\u{25C9}",
            KentenKind::BlackCircle => "\u{25CF}",
            KentenKind::SmallBlackCircle => "\u{2022}",
            KentenKind::Bullseye => "\u{25CE}",
            KentenKind::BlackTriangle => "\u{25B2}",
            KentenKind::WhiteTriangle => "\u{25B3}",
            KentenKind::WhiteCircle => "\u{25CB}",
            KentenKind::SmallWhiteCircle => "\u{25E6}",
            KentenKind::Custom => return None,
        })
    }
}

/// The longest custom kenten mark kept, in characters.
pub const KENTEN_MARK_MAX_CHARS: usize = 8;

/// The text of the mark for `kind` and the stored `character`. A custom mark is `character`
/// (cut to [`KENTEN_MARK_MAX_CHARS`]); an empty one falls back to the sesame dot. Documents made
/// before [`KentenKind`] existed store a preset as its character with the default kind, so a
/// non-empty `character` with [`KentenKind::SesameDot`] is drawn as that character.
pub fn kenten_mark(kind: KentenKind, character: &str) -> String {
    let custom = || -> String { character.chars().filter(|c| !c.is_control()).take(KENTEN_MARK_MAX_CHARS).collect() };
    let text = match kind {
        KentenKind::Custom => custom(),
        KentenKind::SesameDot if !character.is_empty() => custom(),
        k => k.mark().unwrap_or_default().to_string(),
    };
    if text.is_empty() { "\u{FE45}".to_string() } else { text }
}

/// Which side of the line kenten sit on: above in horizontal text and to the right in vertical
/// text, or below / to the left.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum KentenPosition {
    #[default]
    AboveRight,
    BelowLeft,
}

/// Where a mark sits along its character: centred on it (中付き), or at its start (肩付き).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum KentenAlignment {
    #[default]
    Center,
    Start,
}

/// Overprint of an adornment's fill or stroke: as the text's (`Auto`), or set on or off.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum AdornmentOverprint {
    #[default]
    Auto,
    On,
    Off,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn kenten_marks_follow_the_kind_and_cap_custom_text() {
        assert_eq!(kenten_mark(KentenKind::SesameDot, ""), "\u{FE45}");
        assert_eq!(kenten_mark(KentenKind::WhiteCircle, "x"), "\u{25CB}");
        assert_eq!(kenten_mark(KentenKind::SesameDot, "○"), "○", "documents that stored the mark as text");
        assert_eq!(kenten_mark(KentenKind::Custom, ""), "\u{FE45}");
        let long = "★".repeat(10_000);
        assert_eq!(kenten_mark(KentenKind::Custom, &long).chars().count(), KENTEN_MARK_MAX_CHARS);
        assert_eq!(kenten_mark(KentenKind::Custom, "\n\u{7}"), "\u{FE45}", "control characters are dropped");
        assert!(KentenKind::ALL.iter().all(|k| (*k == KentenKind::Custom) == k.mark().is_none()));
    }
}
