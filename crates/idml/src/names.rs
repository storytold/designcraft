//! Name and enumeration mappings between the DesignCraft model and IDML.

use designcraft_color::BlendMode;
use designcraft_doc::{
    Align, Arrowhead, AutoSize, Cap, Capitalization, ClippingKind, ContourType, FirstBaseline, Join, Leading, NumberStyle, Position, StartParagraph,
    StrokeAlign, StrokeType, TabAlign, VerticalJustification, WrapMode, WrapSide,
};
use designcraft_geom::Unit;
use designcraft_geom::corners::CornerShape;

/// `%` → `%25`, `:` → `%3a` (IDML Self-id escaping).
const ANCHORS: [&str; 9] = [
    "TopLeftAnchor",
    "TopCenterAnchor",
    "TopRightAnchor",
    "LeftCenterAnchor",
    "CenterAnchor",
    "RightCenterAnchor",
    "BottomLeftAnchor",
    "BottomCenterAnchor",
    "BottomRightAnchor",
];

pub fn anchor_rel_out(r: designcraft_doc::anchored::AnchorRelative, vertical: bool) -> &'static str {
    use designcraft_doc::anchored::AnchorRelative as R;
    match r {
        R::Anchor if vertical => "LineBaseline",
        R::Anchor => "AnchorLocation",
        R::TextFrame => "TextFrame",
        R::ColumnEdge => "ColumnEdge",
        R::PageMargin => "PageMargins",
        R::PageEdge => "PageEdge",
    }
}

pub fn anchor_rel_in(s: &str) -> designcraft_doc::anchored::AnchorRelative {
    use designcraft_doc::anchored::AnchorRelative as R;
    match s {
        "TextFrame" => R::TextFrame,
        "ColumnEdge" => R::ColumnEdge,
        "PageMargins" => R::PageMargin,
        "PageEdge" => R::PageEdge,
        _ => R::Anchor,
    }
}

/// `PDFAttribute` `PDFCrop` for a placed PDF's crop choice.
pub fn pdf_crop_out(c: designcraft_doc::PdfCrop) -> &'static str {
    use designcraft_doc::PdfCrop as C;
    match c {
        C::Crop => "CropPDF",
        C::Art => "CropArt",
        C::Trim => "CropTrim",
        C::Bleed => "CropBleed",
        C::Media => "CropMedia",
        C::ContentVisible => "CropContentVisibleLayers",
        C::ContentAll => "CropContentAllLayers",
    }
}

/// A placed PDF's crop choice from `PDFCrop` (the crop box when unknown).
pub fn pdf_crop_in(s: &str) -> designcraft_doc::PdfCrop {
    use designcraft_doc::PdfCrop as C;
    C::ALL.into_iter().find(|c| pdf_crop_out(*c) == s).unwrap_or(C::Crop)
}

pub fn anchor_out(i: u8) -> &'static str {
    ANCHORS[(i as usize).min(8)]
}

pub fn anchor_in(s: &str) -> u8 {
    ANCHORS.iter().position(|a| *a == s).unwrap_or(4) as u8
}

pub fn fitting_out(f: designcraft_doc::Fitting) -> &'static str {
    use designcraft_doc::Fitting as F;
    match f {
        F::None => "None",
        F::FillProportionally => "FillProportionally",
        F::FitProportionally => "Proportionally",
        F::FitContentToFrame => "ContentToFrame",
        F::CenterContent => "CenterContent",
    }
}

pub fn fitting_in(s: &str) -> designcraft_doc::Fitting {
    use designcraft_doc::Fitting as F;
    match s {
        "FillProportionally" => F::FillProportionally,
        "Proportionally" => F::FitProportionally,
        "ContentToFrame" => F::FitContentToFrame,
        "CenterContent" => F::CenterContent,
        _ => F::None,
    }
}

pub fn liquid_rule_out(rule: designcraft_doc::LiquidRule) -> &'static str {
    use designcraft_doc::LiquidRule as R;
    match rule {
        R::Off => "Off",
        R::Scale => "Scale",
        R::ReCenter => "Recenter",
        R::GuideBased => "GuideBased",
        R::ObjectBased => "ObjectBased",
        R::ControlledByParent => "UseMaster",
    }
}

pub fn liquid_rule_in(s: &str) -> designcraft_doc::LiquidRule {
    use designcraft_doc::LiquidRule as R;
    match s {
        "Off" => R::Off,
        "Scale" => R::Scale,
        "Recenter" => R::ReCenter,
        "GuideBased" => R::GuideBased,
        "ObjectBased" => R::ObjectBased,
        "UseMaster" => R::ControlledByParent,
        _ => R::Off,
    }
}

pub fn escape_id(s: &str) -> String {
    s.replace('%', "%25").replace(':', "%3a")
}
pub fn unescape_id(s: &str) -> String {
    s.replace("%3a", ":").replace("%3A", ":").replace("%25", "%")
}

/// An `AppliedFont` value → font family. InDesign can write the family with its `$ID/` prefix
/// (`$ID/Arial`), and `$ID/` alone for no font; some writers append the style after a tab.
pub fn font_family_in(v: &str) -> Option<String> {
    // Spaces are part of a family's name ("Minion Pro " is another font than "Minion Pro").
    let v = v.trim_matches(['\n', '\r']);
    let family = v.split('\t').next().unwrap_or(v);
    let family = family.trim_start().strip_prefix("$ID/").unwrap_or(family);
    (!family.trim().is_empty()).then(|| family.to_string())
}

/// Built-in style names: (ours, IDML `Name`).
pub const PARA_BUILTINS: &[(&str, &str)] = &[("[No Paragraph Style]", "$ID/[No paragraph style]"), ("[Basic Paragraph]", "$ID/NormalParagraphStyle")];
pub const CHAR_BUILTINS: &[(&str, &str)] = &[("[None]", "$ID/[No character style]")];
pub const CELL_BUILTINS: &[(&str, &str)] = &[("[None]", "$ID/[None]")];
pub const TABLE_BUILTINS: &[(&str, &str)] = &[("[Basic Table]", "$ID/[Basic Table]"), ("[No table style]", "$ID/[No table style]")];
pub const OBJECT_BUILTINS: &[(&str, &str)] =
    &[("[None]", "$ID/[None]"), ("[Basic Graphics Frame]", "$ID/[Normal Graphics Frame]"), ("[Basic Text Frame]", "$ID/[Normal Text Frame]")];

/// Our style name → IDML `Name` (groups: `A/B` → `A:B`; a literal `:` or `\` is escaped as `\:`
/// or `\\`, so it is not read back as a group separator).
pub fn style_name_out(builtins: &[(&str, &str)], name: &str) -> String {
    builtins
        .iter()
        .find(|(o, _)| *o == name)
        .map(|(_, i)| i.to_string())
        .unwrap_or_else(|| name.replace('\\', "\\\\").replace(':', "\\:").replace('/', ":"))
}
/// IDML `Name` → our style name.
pub fn style_name_in(builtins: &[(&str, &str)], name: &str) -> String {
    if let Some((o, _)) = builtins.iter().find(|(_, i)| *i == name) {
        return o.to_string();
    }
    let name = name.strip_prefix("$ID/").unwrap_or(name);
    let mut out = String::with_capacity(name.len());
    let mut chars = name.chars();
    while let Some(c) = chars.next() {
        match c {
            '\\' => match chars.clone().next() {
                Some(next @ ('\\' | ':')) => {
                    out.push(next);
                    chars.next();
                }
                _ => out.push('\\'),
            },
            ':' => out.push('/'),
            _ => out.push(c),
        }
    }
    out
}
/// `ParagraphStyle/…` Self id for one of our names.
pub fn style_self(kind: &str, builtins: &[(&str, &str)], name: &str) -> String {
    format!("{kind}/{}", escape_id(&style_name_out(builtins, name)))
}

pub fn align_out(a: Align) -> &'static str {
    match a {
        Align::Left => "LeftAlign",
        Align::Center => "CenterAlign",
        Align::Right => "RightAlign",
        Align::LeftJustified => "LeftJustified",
        Align::CenterJustified => "CenterJustified",
        Align::RightJustified => "RightJustified",
        Align::FullyJustified => "FullyJustified",
        Align::TowardsSpine => "ToBindingSide",
        Align::AwayFromSpine => "AwayFromBindingSide",
    }
}
pub fn align_in(s: &str) -> Option<Align> {
    Align::ALL.into_iter().find(|a| align_out(*a) == s)
}

pub fn caps_out(c: Capitalization) -> &'static str {
    match c {
        Capitalization::Normal => "Normal",
        Capitalization::AllCaps => "AllCaps",
        Capitalization::SmallCaps => "SmallCaps",
        Capitalization::OpenTypeAllSmallCaps => "CapToSmallCap",
    }
}
pub fn caps_in(s: &str) -> Option<Capitalization> {
    [Capitalization::Normal, Capitalization::AllCaps, Capitalization::SmallCaps, Capitalization::OpenTypeAllSmallCaps]
        .into_iter()
        .find(|c| caps_out(*c) == s)
}

pub fn position_out(p: Position) -> &'static str {
    match p {
        Position::Normal => "Normal",
        Position::Superscript => "Superscript",
        Position::Subscript => "Subscript",
        Position::OtSuperscript => "OTSuperscript",
        Position::OtSubscript => "OTSubscript",
        Position::OtNumerator => "OTNumerator",
        Position::OtDenominator => "OTDenominator",
    }
}
pub fn position_in(s: &str) -> Option<Position> {
    [
        Position::Normal,
        Position::Superscript,
        Position::Subscript,
        Position::OtSuperscript,
        Position::OtSubscript,
        Position::OtNumerator,
        Position::OtDenominator,
    ]
    .into_iter()
    .find(|p| position_out(*p) == s)
}

pub fn start_para_out(s: StartParagraph) -> &'static str {
    match s {
        StartParagraph::Anywhere => "Anywhere",
        StartParagraph::NextColumn => "NextColumn",
        StartParagraph::NextFrame => "NextFrame",
        StartParagraph::NextPage => "NextPage",
        StartParagraph::NextOddPage => "NextOddPage",
        StartParagraph::NextEvenPage => "NextEvenPage",
    }
}
pub fn start_para_in(s: &str) -> Option<StartParagraph> {
    [
        StartParagraph::Anywhere,
        StartParagraph::NextColumn,
        StartParagraph::NextFrame,
        StartParagraph::NextPage,
        StartParagraph::NextOddPage,
        StartParagraph::NextEvenPage,
    ]
    .into_iter()
    .find(|p| start_para_out(*p) == s)
}

pub fn tab_align_out(a: TabAlign) -> &'static str {
    match a {
        TabAlign::Left => "LeftAlign",
        TabAlign::Center => "CenterAlign",
        TabAlign::Right => "RightAlign",
        TabAlign::Char => "CharacterAlign",
    }
}
pub fn tab_align_in(s: &str) -> TabAlign {
    match s {
        "CenterAlign" => TabAlign::Center,
        "RightAlign" => TabAlign::Right,
        "CharacterAlign" => TabAlign::Char,
        _ => TabAlign::Left,
    }
}

pub fn leading_out(l: Leading) -> (String, &'static str) {
    match l {
        Leading::Auto => ("Auto".into(), "enumeration"),
        Leading::Points(v) => (num(v), "unit"),
    }
}
pub fn leading_in(s: &str) -> Option<Leading> {
    if s == "Auto" { Some(Leading::Auto) } else { s.trim().parse::<f64>().ok().filter(|v| v.is_finite()).map(Leading::Points) }
}

pub fn stroke_align_out(a: StrokeAlign) -> &'static str {
    match a {
        StrokeAlign::Center => "CenterAlignment",
        StrokeAlign::Inside => "InsideAlignment",
        StrokeAlign::Outside => "OutsideAlignment",
    }
}
pub fn stroke_align_in(s: &str) -> StrokeAlign {
    match s {
        "InsideAlignment" => StrokeAlign::Inside,
        "OutsideAlignment" => StrokeAlign::Outside,
        _ => StrokeAlign::Center,
    }
}
pub fn cap_out(c: Cap) -> &'static str {
    match c {
        Cap::Butt => "ButtEndCap",
        Cap::Round => "RoundEndCap",
        Cap::Projecting => "ProjectingEndCap",
    }
}
pub fn cap_in(s: &str) -> Cap {
    match s {
        "RoundEndCap" => Cap::Round,
        "ProjectingEndCap" => Cap::Projecting,
        _ => Cap::Butt,
    }
}
pub fn join_out(j: Join) -> &'static str {
    match j {
        Join::Miter => "MiterEndJoin",
        Join::Round => "RoundEndJoin",
        Join::Bevel => "BevelEndJoin",
    }
}
pub fn join_in(s: &str) -> Join {
    match s {
        "RoundEndJoin" => Join::Round,
        "BevelEndJoin" => Join::Bevel,
        _ => Join::Miter,
    }
}

/// Stroke style Self ids for our stroke types (built-in InDesign stroke styles).
pub fn stroke_type_out(t: &StrokeType) -> String {
    let s = match t {
        // Named custom styles are written as StripedStrokeStyle / DashedStrokeStyle elements.
        StrokeType::Style { name } => return format!("CustomStrokeStyle/{name}"),
        StrokeType::Stripes { .. } => "StrokeStyle/$ID/Solid",
        StrokeType::Solid => "StrokeStyle/$ID/Solid",
        StrokeType::Dashed { .. } => "StrokeStyle/$ID/Dashed",
        StrokeType::Dotted => "StrokeStyle/$ID/Canned Dotted",
        StrokeType::ThickThin => "StrokeStyle/$ID/ThickThin",
        StrokeType::ThinThick => "StrokeStyle/$ID/ThinThick",
        StrokeType::ThinThin => "StrokeStyle/$ID/ThinThin",
        StrokeType::ThickThick => "StrokeStyle/$ID/ThickThick",
        StrokeType::ThinThickThin => "StrokeStyle/$ID/ThinThickThin",
        StrokeType::ThickThinThick => "StrokeStyle/$ID/ThickThinThick",
        StrokeType::Wavy => "StrokeStyle/$ID/Wavy",
        StrokeType::Hashed => "StrokeStyle/$ID/Straight Hash",
    };
    s.to_string()
}
pub fn stroke_type_in(s: &str) -> StrokeType {
    for prefix in ["StripedStrokeStyle/", "DashedStrokeStyle/", "DottedStrokeStyle/", "CustomStrokeStyle/"] {
        if let Some(name) = s.strip_prefix(prefix) {
            return StrokeType::Style { name: name.to_string() };
        }
    }
    let n = s.rsplit('/').next().unwrap_or(s).replace([' ', '-'], "").to_ascii_lowercase();
    match n.as_str() {
        "dashed" | "dashed(3and2)" | "dashed(4and4)" | "canneddashed3x2" | "canneddashed4x4" => StrokeType::Dashed { pattern: vec![] },
        "dotted" | "japanesedots" | "canneddotted" => StrokeType::Dotted,
        "thickthin" => StrokeType::ThickThin,
        "thinthick" => StrokeType::ThinThick,
        "thinthin" => StrokeType::ThinThin,
        "thickthick" => StrokeType::ThickThick,
        "thinthickthin" => StrokeType::ThinThickThin,
        "thickthinthick" => StrokeType::ThickThinThick,
        "wavy" => StrokeType::Wavy,
        "straighthash" | "leftslanthash" | "rightslanthash" => StrokeType::Hashed,
        _ => StrokeType::Solid,
    }
}

pub fn arrow_out(a: Arrowhead) -> &'static str {
    match a {
        Arrowhead::None => "None",
        Arrowhead::SimpleWide => "SimpleWideArrowHead",
        Arrowhead::Simple => "SimpleArrowHead",
        Arrowhead::Triangle => "TriangleArrowHead",
        Arrowhead::TriangleWide => "TriangleWideArrowHead",
        Arrowhead::Barbed => "BarbedArrowHead",
        Arrowhead::Curved => "CurvedArrowHead",
        Arrowhead::Circle => "CircleArrowHead",
        Arrowhead::CircleSolid => "CircleSolidArrowHead",
        Arrowhead::Square => "SquareArrowHead",
        Arrowhead::SquareSolid => "SquareSolidArrowHead",
        Arrowhead::Bar => "BarArrowHead",
    }
}
pub fn arrow_in(s: &str) -> Arrowhead {
    [
        Arrowhead::SimpleWide,
        Arrowhead::Simple,
        Arrowhead::Triangle,
        Arrowhead::TriangleWide,
        Arrowhead::Barbed,
        Arrowhead::Curved,
        Arrowhead::Circle,
        Arrowhead::CircleSolid,
        Arrowhead::Square,
        Arrowhead::SquareSolid,
        Arrowhead::Bar,
    ]
    .into_iter()
    .find(|a| arrow_out(*a) == s)
    .unwrap_or(Arrowhead::None)
}

pub fn box_width_out(w: designcraft_doc::BoxWidth) -> &'static str {
    match w {
        designcraft_doc::BoxWidth::Column => "ColumnWidth",
        designcraft_doc::BoxWidth::Text => "TextWidth",
    }
}
pub fn box_width_in(s: &str) -> designcraft_doc::BoxWidth {
    if s == "TextWidth" { designcraft_doc::BoxWidth::Text } else { designcraft_doc::BoxWidth::Column }
}

pub fn box_top_out(t: designcraft_doc::BoxTop) -> &'static str {
    match t {
        designcraft_doc::BoxTop::Ascent => "AscentTopOrigin",
        designcraft_doc::BoxTop::Baseline => "BaselineTopOrigin",
        designcraft_doc::BoxTop::EmBox => "EmBoxTopOrigin",
        designcraft_doc::BoxTop::Leading => "LeadingTopOrigin",
    }
}
pub fn box_top_in(s: &str) -> designcraft_doc::BoxTop {
    match s {
        "BaselineTopOrigin" => designcraft_doc::BoxTop::Baseline,
        "EmBoxTopOrigin" => designcraft_doc::BoxTop::EmBox,
        "LeadingTopOrigin" => designcraft_doc::BoxTop::Leading,
        _ => designcraft_doc::BoxTop::Ascent,
    }
}

pub fn box_bottom_out(b: designcraft_doc::BoxBottom) -> &'static str {
    match b {
        designcraft_doc::BoxBottom::Descent => "DescentBottomOrigin",
        designcraft_doc::BoxBottom::Baseline => "BaselineBottomOrigin",
        designcraft_doc::BoxBottom::EmBox => "EmBoxBottomOrigin",
    }
}
pub fn box_bottom_in(s: &str) -> designcraft_doc::BoxBottom {
    match s {
        "BaselineBottomOrigin" => designcraft_doc::BoxBottom::Baseline,
        "EmBoxBottomOrigin" => designcraft_doc::BoxBottom::EmBox,
        _ => designcraft_doc::BoxBottom::Descent,
    }
}

pub fn list_align_out(a: designcraft_doc::ListAlign) -> &'static str {
    match a {
        designcraft_doc::ListAlign::Left => "LeftAlign",
        designcraft_doc::ListAlign::Center => "CenterAlign",
        designcraft_doc::ListAlign::Right => "RightAlign",
    }
}
pub fn list_align_in(s: &str) -> designcraft_doc::ListAlign {
    match s {
        "CenterAlign" => designcraft_doc::ListAlign::Center,
        "RightAlign" => designcraft_doc::ListAlign::Right,
        _ => designcraft_doc::ListAlign::Left,
    }
}

pub fn corner_out(c: CornerShape) -> &'static str {
    match c {
        CornerShape::None => "None",
        CornerShape::Rounded => "RoundedCorner",
        CornerShape::InverseRounded => "InverseRoundedCorner",
        CornerShape::Inset => "InsetCorner",
        CornerShape::Bevel => "BevelCorner",
        CornerShape::Fancy => "FancyCorner",
    }
}
pub fn corner_in(s: &str) -> CornerShape {
    CornerShape::ALL.into_iter().find(|c| corner_out(*c) == s).unwrap_or(CornerShape::None)
}

pub fn blend_out(b: BlendMode) -> &'static str {
    match b {
        BlendMode::Normal => "Normal",
        BlendMode::Darken => "Darken",
        BlendMode::Multiply => "Multiply",
        BlendMode::ColorBurn => "ColorBurn",
        BlendMode::Lighten => "Lighten",
        BlendMode::Screen => "Screen",
        BlendMode::ColorDodge => "ColorDodge",
        BlendMode::Overlay => "Overlay",
        BlendMode::SoftLight => "SoftLight",
        BlendMode::HardLight => "HardLight",
        BlendMode::Difference => "Difference",
        BlendMode::Exclusion => "Exclusion",
        BlendMode::Hue => "Hue",
        BlendMode::Saturation => "Saturation",
        BlendMode::Color => "Color",
        BlendMode::Luminosity => "Luminosity",
    }
}
pub fn blend_in(s: &str) -> BlendMode {
    BlendMode::ALL.into_iter().find(|b| blend_out(*b) == s).unwrap_or_default()
}

pub fn wrap_mode_out(m: WrapMode) -> &'static str {
    match m {
        WrapMode::None => "None",
        WrapMode::BoundingBox => "BoundingBoxTextWrap",
        WrapMode::Contour => "Contour",
        WrapMode::JumpObject => "JumpObjectTextWrap",
        WrapMode::JumpToNextColumn => "NextColumnTextWrap",
    }
}
pub fn clipping_kind_out(k: ClippingKind) -> &'static str {
    match k {
        ClippingKind::DetectEdges => "DetectEdges",
        ClippingKind::AlphaChannel => "AlphaChannel",
        ClippingKind::PhotoshopPath => "PhotoshopPath",
        ClippingKind::UserModifiedPath => "UserModifiedPath",
    }
}
/// `None` for `ClippingType="None"` and unknown types.
pub fn clipping_kind_in(s: &str) -> Option<ClippingKind> {
    [ClippingKind::DetectEdges, ClippingKind::AlphaChannel, ClippingKind::PhotoshopPath, ClippingKind::UserModifiedPath]
        .into_iter()
        .find(|k| clipping_kind_out(*k) == s)
}
pub fn wrap_mode_in(s: &str) -> WrapMode {
    [WrapMode::BoundingBox, WrapMode::Contour, WrapMode::JumpObject, WrapMode::JumpToNextColumn]
        .into_iter()
        .find(|m| wrap_mode_out(*m) == s)
        .unwrap_or(WrapMode::None)
}
pub fn contour_type_out(c: ContourType) -> &'static str {
    match c {
        ContourType::BoundingBox => "BoundingBox",
        ContourType::DetectEdges => "DetectEdges",
        ContourType::AlphaChannel => "AlphaChannel",
        ContourType::PhotoshopPath => "PhotoshopPath",
        ContourType::GraphicFrame => "GraphicFrame",
        ContourType::SameAsClipping => "SameAsClipping",
    }
}
pub fn contour_type_in(s: &str) -> ContourType {
    [ContourType::BoundingBox, ContourType::DetectEdges, ContourType::AlphaChannel, ContourType::PhotoshopPath, ContourType::GraphicFrame]
        .into_iter()
        .find(|c| contour_type_out(*c) == s)
        .unwrap_or(ContourType::SameAsClipping)
}
pub fn wrap_side_out(s: WrapSide) -> &'static str {
    match s {
        WrapSide::BothSides => "BothSides",
        WrapSide::RightSide => "RightSide",
        WrapSide::LeftSide => "LeftSide",
        WrapSide::TowardsSpine => "SideTowardsSpine",
        WrapSide::AwayFromSpine => "SideAwayFromSpine",
        WrapSide::LargestArea => "LargestArea",
    }
}
pub fn wrap_side_in(s: &str) -> WrapSide {
    [WrapSide::RightSide, WrapSide::LeftSide, WrapSide::TowardsSpine, WrapSide::AwayFromSpine, WrapSide::LargestArea]
        .into_iter()
        .find(|m| wrap_side_out(*m) == s)
        .unwrap_or(WrapSide::BothSides)
}

pub fn vj_out(v: VerticalJustification) -> &'static str {
    match v {
        VerticalJustification::Top => "TopAlign",
        VerticalJustification::Center => "CenterAlign",
        VerticalJustification::Bottom => "BottomAlign",
        VerticalJustification::Justify => "JustifyAlign",
    }
}
pub fn vj_in(s: &str) -> VerticalJustification {
    match s {
        "CenterAlign" => VerticalJustification::Center,
        "BottomAlign" => VerticalJustification::Bottom,
        "JustifyAlign" => VerticalJustification::Justify,
        _ => VerticalJustification::Top,
    }
}
pub fn first_baseline_out(f: FirstBaseline) -> &'static str {
    match f {
        FirstBaseline::Ascent => "AscentOffset",
        FirstBaseline::CapHeight => "CapHeight",
        FirstBaseline::Leading => "LeadingOffset",
        FirstBaseline::XHeight => "XHeight",
        FirstBaseline::Fixed => "FixedHeight",
        FirstBaseline::EmboxHeight => "EmboxHeight",
    }
}
pub fn first_baseline_in(s: &str) -> FirstBaseline {
    [FirstBaseline::CapHeight, FirstBaseline::Leading, FirstBaseline::XHeight, FirstBaseline::Fixed, FirstBaseline::EmboxHeight]
        .into_iter()
        .find(|m| first_baseline_out(*m) == s)
        .unwrap_or(FirstBaseline::Ascent)
}
pub fn auto_size_out(a: AutoSize) -> &'static str {
    match a {
        AutoSize::Off => "Off",
        AutoSize::HeightOnly => "HeightOnly",
        AutoSize::WidthOnly => "WidthOnly",
        AutoSize::HeightAndWidth => "HeightAndWidth",
        AutoSize::HeightAndWidthProportionally => "HeightAndWidthProportionally",
    }
}
pub fn auto_size_in(s: &str) -> AutoSize {
    [AutoSize::HeightOnly, AutoSize::WidthOnly, AutoSize::HeightAndWidth, AutoSize::HeightAndWidthProportionally]
        .into_iter()
        .find(|m| auto_size_out(*m) == s)
        .unwrap_or(AutoSize::Off)
}

/// Auto-size reference points in our 0..9 order (row-major, top-left first).
pub const REF_POINTS: [&str; 9] = [
    "TopLeftPoint",
    "TopCenterPoint",
    "TopRightPoint",
    "LeftCenterPoint",
    "CenterPoint",
    "RightCenterPoint",
    "BottomLeftPoint",
    "BottomCenterPoint",
    "BottomRightPoint",
];

/// IDML `PageNumberStyle` of a section, also written for `FootnoteNumberingStyle`.
pub fn number_style_out(n: NumberStyle) -> &'static str {
    match n {
        NumberStyle::Arabic => "Arabic",
        NumberStyle::ArabicLeadingZero => "SingleLeadingZeros",
        NumberStyle::ArabicThreeDigits => "DoubleLeadingZeros",
        NumberStyle::ArabicFourDigits => "TripleLeadingZeros",
        NumberStyle::UpperRoman => "UpperRoman",
        NumberStyle::LowerRoman => "LowerRoman",
        NumberStyle::UpperLetters => "UpperLetters",
        NumberStyle::LowerLetters => "LowerLetters",
        NumberStyle::Symbols => "Symbols",
        NumberStyle::Kanji => "Kanji",
    }
}
pub fn number_style_in(s: &str) -> NumberStyle {
    match s {
        "SingleLeadingZeros" => NumberStyle::ArabicLeadingZero,
        "DoubleLeadingZeros" => NumberStyle::ArabicThreeDigits,
        "TripleLeadingZeros" => NumberStyle::ArabicFourDigits,
        "UpperRoman" => NumberStyle::UpperRoman,
        "LowerRoman" => NumberStyle::LowerRoman,
        "UpperLetters" => NumberStyle::UpperLetters,
        "LowerLetters" => NumberStyle::LowerLetters,
        "Symbols" => NumberStyle::Symbols,
        "Kanji" => NumberStyle::Kanji,
        _ => NumberStyle::Arabic,
    }
}

pub fn unit_out(u: Unit) -> &'static str {
    match u {
        Unit::Points => "Points",
        Unit::Picas => "Picas",
        Unit::Inches => "Inches",
        Unit::InchesDecimal => "InchesDecimal",
        Unit::Millimeters => "Millimeters",
        Unit::Centimeters => "Centimeters",
        Unit::Ciceros => "Ciceros",
        Unit::Agates => "Agates",
        Unit::Pixels => "Pixels",
        Unit::Q => "U",
        Unit::Ha => "Ha",
    }
}
pub fn unit_in(s: &str) -> Unit {
    [
        Unit::Points,
        Unit::Picas,
        Unit::Inches,
        Unit::InchesDecimal,
        Unit::Millimeters,
        Unit::Centimeters,
        Unit::Ciceros,
        Unit::Agates,
        Unit::Pixels,
        Unit::Q,
        Unit::Ha,
    ]
    .into_iter()
    .find(|u| unit_out(*u) == s)
    .unwrap_or(Unit::Points)
}

/// IDML layer colour enumeration names for our layer colour table.
pub fn layer_color_out(c: [u8; 3]) -> Option<String> {
    designcraft_doc::LAYER_COLORS.iter().find(|(_, v)| *v == c).map(|(n, _)| n.replace(' ', ""))
}
pub fn layer_color_in(s: &str) -> Option<[u8; 3]> {
    designcraft_doc::LAYER_COLORS.iter().find(|(n, _)| n.replace(' ', "") == s).map(|(_, v)| *v)
}

/// Format a number compactly (no trailing zeros, no `-0`).
pub fn num(v: f64) -> String {
    if !v.is_finite() {
        return "0".into();
    }
    let r = (v * 1e9).round() / 1e9;
    if r == r.trunc() && r.abs() < 1e15 {
        let i = r as i64;
        return i.to_string();
    }
    let s = format!("{r}");
    if s == "-0" { "0".into() } else { s }
}

pub fn pt(x: f64, y: f64) -> String {
    format!("{} {}", num(x), num(y))
}

// ---------- list numbering ----------

/// A paragraph's `NumberingFormat` ("A, B, C, D...", "001, 002, 003...", …), told apart by its
/// first item. None for formats with no counterpart (other CJK forms, Arabic, Hebrew, …).
pub fn numbering_format_in(s: &str) -> Option<NumberStyle> {
    Some(match s.split(',').next().unwrap_or("").trim() {
        "1" => NumberStyle::Arabic,
        "01" => NumberStyle::ArabicLeadingZero,
        "001" => NumberStyle::ArabicThreeDigits,
        "0001" => NumberStyle::ArabicFourDigits,
        "I" => NumberStyle::UpperRoman,
        "i" => NumberStyle::LowerRoman,
        "A" => NumberStyle::UpperLetters,
        "a" => NumberStyle::LowerLetters,
        "一" => NumberStyle::Kanji,
        _ => return None,
    })
}
pub fn numbering_format_out(n: NumberStyle) -> &'static str {
    match n {
        NumberStyle::Arabic | NumberStyle::Symbols => "1, 2, 3, 4...",
        NumberStyle::ArabicLeadingZero => "01, 02, 03...",
        NumberStyle::ArabicThreeDigits => "001, 002, 003...",
        NumberStyle::ArabicFourDigits => "0001, 0002, 0003...",
        NumberStyle::UpperRoman => "I, II, III, IV...",
        NumberStyle::LowerRoman => "i, ii, iii, iv...",
        NumberStyle::UpperLetters => "A, B, C, D...",
        NumberStyle::LowerLetters => "a, b, c, d...",
        NumberStyle::Kanji => "一, 二, 三, 四...",
    }
}
/// Label text in IDML metacharacters (a tab is `^t`).
pub fn list_text_out(s: &str) -> String {
    s.replace('\t', "^t")
}

// ---------- footnote options ----------

/// IDML `FootnoteNumberingStyle`: the page number styles and `Asterisks`.
pub fn note_style_in(s: &str) -> NumberStyle {
    match s {
        "Asterisks" => NumberStyle::Symbols,
        s => number_style_in(s),
    }
}
pub const NOTE_RESTART: [(designcraft_doc::notes::NoteRestart, &str); 4] = [
    (designcraft_doc::notes::NoteRestart::Never, "DontRestart"),
    (designcraft_doc::notes::NoteRestart::Page, "PageRestart"),
    (designcraft_doc::notes::NoteRestart::Spread, "SpreadRestart"),
    (designcraft_doc::notes::NoteRestart::Section, "SectionRestart"),
];
pub const NOTE_AFFIX: [(designcraft_doc::notes::AffixIn, &str); 4] = [
    (designcraft_doc::notes::AffixIn::None, "NoPrefixSuffix"),
    (designcraft_doc::notes::AffixIn::Reference, "PrefixSuffixReference"),
    (designcraft_doc::notes::AffixIn::Text, "PrefixSuffixMarker"),
    (designcraft_doc::notes::AffixIn::Both, "PrefixSuffixBoth"),
];
pub fn note_marker_out(p: Position) -> &'static str {
    match p {
        Position::Superscript | Position::OtSuperscript => "SuperscriptMarker",
        Position::Subscript | Position::OtSubscript => "SubscriptMarker",
        _ => "NormalMarker",
    }
}
pub fn note_marker_in(s: &str) -> Position {
    match s {
        "SuperscriptMarker" => Position::Superscript,
        "SubscriptMarker" => Position::Subscript,
        _ => Position::Normal,
    }
}

/// Nested-style delimiter → (property type, value).
pub fn nested_until_out(u: &designcraft_doc::NestedUntil) -> (&'static str, String) {
    use designcraft_doc::NestedUntil as N;
    let e = |v: &str| ("enumeration", v.to_string());
    match u {
        N::Sentences => e("Sentence"),
        N::Words => e("AnyWord"),
        N::Characters => e("AnyCharacter"),
        N::Letters => e("Letters"),
        N::Digits => e("Digits"),
        N::Tab => e("Tabs"),
        N::ForcedLineBreak => e("ForcedLineBreak"),
        N::EmSpace => e("EmSpaces"),
        N::EnSpace => e("EnSpaces"),
        N::Dropcap => e("Dropcap"),
        N::Chars(c) => ("string", c.clone()),
    }
}

pub fn nested_until_in(ty: &str, v: &str) -> designcraft_doc::NestedUntil {
    use designcraft_doc::NestedUntil as N;
    if ty == "string" {
        return N::Chars(v.to_string());
    }
    match v.trim() {
        "Sentence" => N::Sentences,
        "AnyCharacter" => N::Characters,
        "Letters" => N::Letters,
        "Digits" => N::Digits,
        "Tabs" => N::Tab,
        "ForcedLineBreak" => N::ForcedLineBreak,
        "EmSpaces" => N::EmSpace,
        "EnSpaces" => N::EnSpace,
        "Dropcap" => N::Dropcap,
        _ => N::Words,
    }
}

pub fn path_align_out(a: designcraft_doc::PathAlign) -> &'static str {
    match a {
        designcraft_doc::PathAlign::Baseline => "BaselineAlignment",
        designcraft_doc::PathAlign::Ascender => "AscenderAlignment",
        designcraft_doc::PathAlign::Descender => "DescenderAlignment",
        designcraft_doc::PathAlign::Center => "CenterAlignment",
    }
}

pub fn path_align_in(v: &str) -> designcraft_doc::PathAlign {
    match v {
        "AscenderAlignment" => designcraft_doc::PathAlign::Ascender,
        "DescenderAlignment" => designcraft_doc::PathAlign::Descender,
        "CenterAlignment" => designcraft_doc::PathAlign::Center,
        _ => designcraft_doc::PathAlign::Baseline,
    }
}

pub fn digits_out(d: designcraft_doc::Digits) -> &'static str {
    use designcraft_doc::Digits;
    match d {
        Digits::Default => "DefaultDigits",
        Digits::Arabic => "ArabicDigits",
        Digits::Hindi => "HindiDigits",
        Digits::Farsi => "FarsiDigits",
        Digits::Native => "NativeDigits",
    }
}
pub fn digits_in(s: &str) -> Option<designcraft_doc::Digits> {
    use designcraft_doc::Digits;
    [Digits::Default, Digits::Arabic, Digits::Hindi, Digits::Farsi, Digits::Native].into_iter().find(|d| digits_out(*d) == s)
}

/// A TOC separator in IDML's special-character notation.
pub fn toc_separator_out(s: &str) -> String {
    designcraft_doc::story::toc_separator_notation(s)
}
/// IDML may prefix a separator with its localized-string marker.
pub fn toc_separator_in(s: &str) -> String {
    designcraft_doc::story::toc_separator_text(s.strip_prefix("$ID/").unwrap_or(s))
}

#[cfg(test)]
mod toc_separator_tests {
    use super::*;

    #[test]
    fn localized_right_indent_tab_is_not_an_ordinary_tab() {
        assert_eq!(toc_separator_in("$ID/^y"), "\u{E006}");
        assert_eq!(toc_separator_in("$ID/ ^y "), " \u{E006} ");
        assert_eq!(toc_separator_in("^t"), "\t");
        assert_eq!(toc_separator_out("\u{E006}"), "^y");
        assert_eq!(toc_separator_in("^^y ^q ^"), "^y ^q ^");
    }
}
