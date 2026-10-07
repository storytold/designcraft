use designcraft_doc::arabic::{CharacterDirection as D, DiacriticPosition as P};

pub fn direction_in(s: &str) -> Option<D> {
    match s.trim() {
        "DefaultDirection" => Some(D::Default),
        "LeftToRightDirection" => Some(D::LeftToRight),
        "RightToLeftDirection" => Some(D::RightToLeft),
        _ => None,
    }
}
pub fn direction_out(d: D) -> &'static str {
    match d {
        D::Default => "DefaultDirection",
        D::LeftToRight => "LeftToRightDirection",
        D::RightToLeft => "RightToLeftDirection",
    }
}
pub fn diacritic_in(s: &str) -> Option<P> {
    match s.trim() {
        "DefaultPosition" => Some(P::Default),
        "OpentypePosition" => Some(P::OpenType),
        "OpentypePositionFromBaseline" => Some(P::OpenTypeFromBaseline),
        "LoosePosition" => Some(P::Loose),
        "MediumPosition" => Some(P::Medium),
        "TightPosition" => Some(P::Tight),
        _ => None,
    }
}
pub fn diacritic_out(p: P) -> &'static str {
    match p {
        P::Default => "DefaultPosition",
        P::OpenType => "OpentypePosition",
        P::OpenTypeFromBaseline => "OpentypePositionFromBaseline",
        P::Loose => "LoosePosition",
        P::Medium => "MediumPosition",
        P::Tight => "TightPosition",
    }
}
pub fn kashidas_in(s: &str) -> Option<bool> {
    match s.trim() {
        "DefaultKashidas" => Some(true),
        "KashidasOff" => Some(false),
        _ => None,
    }
}
