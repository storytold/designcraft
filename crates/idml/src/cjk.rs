//! IDML enum mappings for CJK character formatting.
use designcraft_doc::cjk::{CharacterAlignment as A, LeadingModel as L};
pub(crate) fn leading_in(v: &str) -> Option<L> {
    Some(match v {
        "LeadingModelRoman" => L::Roman,
        "LeadingModelAkiBelow" => L::AkiBelow,
        "LeadingModelAkiAbove" => L::AkiAbove,
        "LeadingModelCenter" => L::Center,
        "LeadingModelCenterDown" => L::CenterDown,
        _ => return None,
    })
}
pub(crate) fn leading_out(v: L) -> &'static str {
    match v {
        L::Roman => "LeadingModelRoman",
        L::AkiBelow => "LeadingModelAkiBelow",
        L::AkiAbove => "LeadingModelAkiAbove",
        L::Center => "LeadingModelCenter",
        L::CenterDown => "LeadingModelCenterDown",
    }
}
pub(crate) fn alignment_in(v: &str) -> Option<A> {
    Some(match v {
        "AlignBaseline" => A::Baseline,
        "AlignEmTop" => A::EmTop,
        "AlignEmCenter" => A::EmCenter,
        "AlignEmBottom" => A::EmBottom,
        "AlignIcfTop" => A::IcfTop,
        "AlignIcfBottom" => A::IcfBottom,
        _ => return None,
    })
}
pub(crate) fn alignment_out(v: A) -> &'static str {
    match v {
        A::Baseline => "AlignBaseline",
        A::EmTop => "AlignEmTop",
        A::EmCenter => "AlignEmCenter",
        A::EmBottom => "AlignEmBottom",
        A::IcfTop => "AlignIcfTop",
        A::IcfBottom => "AlignIcfBottom",
    }
}
pub(crate) fn warichu_align_in(v: &str) -> Option<designcraft_doc::cjk::WarichuAlignment> {
    use designcraft_doc::cjk::WarichuAlignment as A;
    Some(match v {
        "Auto" => A::Auto,
        "Left" | "LeftAlign" => A::Left,
        "Center" | "CenterAlign" => A::Center,
        "Right" | "RightAlign" => A::Right,
        "FullJustify" | "Justify" | "LeftJustify" | "CenterJustify" | "RightJustify" => A::Justify,
        _ => return None,
    })
}

pub(crate) fn warichu_align_out(v: designcraft_doc::cjk::WarichuAlignment) -> &'static str {
    use designcraft_doc::cjk::WarichuAlignment as A;
    match v {
        A::Auto => "Auto",
        A::Left => "Left",
        A::Center => "Center",
        A::Right => "Right",
        A::Justify => "FullJustify",
    }
}

pub(crate) fn kenten_character(v: &str) -> Option<&'static str> {
    Some(match v {
        "None" => "",
        "KentenSesameDot" => "﹅",
        "KentenWhiteSesameDot" => "﹆",
        "KentenBlackCircle" => "●",
        "KentenWhiteCircle" => "○",
        "KentenBlackTriangle" => "▲",
        "KentenWhiteTriangle" => "△",
        "KentenBlackBullseye" => "◉",
        "KentenWhiteBullseye" => "◎",
        "KentenFisheye" => "◉",
        _ => return None,
    })
}

// Kenten, shatai, auto tate-chu-yoko and grid settings.

use designcraft_doc::cjk_settings::{AdornmentOverprint, KentenAlignment, KentenKind, KentenPosition};

/// `KentenKind`. `None`, `KentenSesameDot` and `KentenSmallBlackCircle` are confirmed names; the
/// others follow their pattern. `KentenBlackBullseye` / `KentenWhiteBullseye` are read as well.
pub(crate) fn kenten_kind_in(v: &str) -> Option<KentenKind> {
    Some(match v.trim() {
        "KentenSesameDot" => KentenKind::SesameDot,
        "KentenWhiteSesameDot" => KentenKind::WhiteSesameDot,
        "KentenFisheye" | "KentenBlackBullseye" => KentenKind::Fisheye,
        "KentenBlackCircle" => KentenKind::BlackCircle,
        "KentenSmallBlackCircle" => KentenKind::SmallBlackCircle,
        "KentenBullseye" | "KentenWhiteBullseye" => KentenKind::Bullseye,
        "KentenBlackTriangle" => KentenKind::BlackTriangle,
        "KentenWhiteTriangle" => KentenKind::WhiteTriangle,
        "KentenWhiteCircle" => KentenKind::WhiteCircle,
        "KentenSmallWhiteCircle" => KentenKind::SmallWhiteCircle,
        "Custom" => KentenKind::Custom,
        _ => return None,
    })
}

pub(crate) fn kenten_kind_out(k: KentenKind) -> &'static str {
    match k {
        KentenKind::SesameDot => "KentenSesameDot",
        KentenKind::WhiteSesameDot => "KentenWhiteSesameDot",
        KentenKind::Fisheye => "KentenFisheye",
        KentenKind::BlackCircle => "KentenBlackCircle",
        KentenKind::SmallBlackCircle => "KentenSmallBlackCircle",
        KentenKind::Bullseye => "KentenBullseye",
        KentenKind::BlackTriangle => "KentenBlackTriangle",
        KentenKind::WhiteTriangle => "KentenWhiteTriangle",
        KentenKind::WhiteCircle => "KentenWhiteCircle",
        KentenKind::SmallWhiteCircle => "KentenSmallWhiteCircle",
        KentenKind::Custom => "Custom",
    }
}

pub(crate) fn kenten_position_in(v: &str) -> Option<KentenPosition> {
    match v.trim() {
        "AboveRight" => Some(KentenPosition::AboveRight),
        "BelowLeft" => Some(KentenPosition::BelowLeft),
        _ => None,
    }
}

pub(crate) fn kenten_position_out(v: KentenPosition) -> &'static str {
    match v {
        KentenPosition::AboveRight => "AboveRight",
        KentenPosition::BelowLeft => "BelowLeft",
    }
}

/// `KentenAlignment`: `AlignKentenCenter` is confirmed, `AlignKentenLeft` (肩付き) is a guess.
pub(crate) fn kenten_alignment_in(v: &str) -> Option<KentenAlignment> {
    match v.trim() {
        "AlignKentenCenter" => Some(KentenAlignment::Center),
        "AlignKentenLeft" | "AlignKentenStart" => Some(KentenAlignment::Start),
        _ => None,
    }
}

pub(crate) fn kenten_alignment_out(v: KentenAlignment) -> &'static str {
    match v {
        KentenAlignment::Center => "AlignKentenCenter",
        KentenAlignment::Start => "AlignKentenLeft",
    }
}

/// Adornment overprint: `Auto` is confirmed, `OverprintOn` / `OverprintOff` are guesses.
pub(crate) fn overprint_in(v: &str) -> Option<AdornmentOverprint> {
    match v.trim() {
        "Auto" => Some(AdornmentOverprint::Auto),
        "OverprintOn" | "true" => Some(AdornmentOverprint::On),
        "OverprintOff" | "false" => Some(AdornmentOverprint::Off),
        _ => None,
    }
}

pub(crate) fn overprint_out(v: AdornmentOverprint) -> &'static str {
    match v {
        AdornmentOverprint::Auto => "Auto",
        AdornmentOverprint::On => "OverprintOn",
        AdornmentOverprint::Off => "OverprintOff",
    }
}

/// `GridAlignment` other than `None`: the grid reference. Both `ICF` spellings are read; unknown
/// values align the roman baseline.
pub(crate) fn grid_reference_in(v: &str) -> A {
    match v.trim() {
        "AlignEmTop" => A::EmTop,
        "AlignEmCenter" => A::EmCenter,
        "AlignEmBottom" => A::EmBottom,
        "AlignICFTop" | "AlignIcfTop" => A::IcfTop,
        "AlignICFBottom" | "AlignIcfBottom" => A::IcfBottom,
        _ => A::Baseline,
    }
}

pub(crate) fn grid_reference_out(v: A) -> &'static str {
    match v {
        A::Baseline => "AlignBaseline",
        A::EmTop => "AlignEmTop",
        A::EmCenter => "AlignEmCenter",
        A::EmBottom => "AlignEmBottom",
        A::IcfTop => "AlignICFTop",
        A::IcfBottom => "AlignICFBottom",
    }
}

/// IDML writes `ShataiDegreeAngle` in hundredths of a degree (`4500` = 45°).
pub(crate) const SHATAI_ANGLE_UNITS: f64 = 100.0;
