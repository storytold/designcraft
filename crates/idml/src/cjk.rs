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
