use super::{Affine, Deserialize, PathData, Rect, Serialize, StoryId, is_default};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum VerticalJustification {
    #[default]
    Top,
    Center,
    Bottom,
    Justify,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum FirstBaseline {
    #[default]
    Ascent,
    CapHeight,
    Leading,
    XHeight,
    Fixed,
    /// The top of the font's em box: its `BASE` em box; else, for a CJK font, its OS/2 typo
    /// ascender when the typo ascender and descender span one em; else half an em above half the
    /// cap height.
    EmboxHeight,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum AutoSize {
    #[default]
    Off,
    HeightOnly,
    WidthOnly,
    HeightAndWidth,
    HeightAndWidthProportionally,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum ColumnsKind {
    #[default]
    FixedNumber,
    FixedWidth,
    FlexibleWidth,
}

/// Text Frame Options.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct TextFrameOptions {
    pub columns: u32,
    pub columns_kind: ColumnsKind,
    pub gutter: f64,
    /// Column width for fixed/flexible width columns.
    pub column_width: f64,
    pub balance_columns: bool,
    /// Inset spacing: top, left, bottom, right.
    pub inset: [f64; 4],
    pub vertical_justification: VerticalJustification,
    pub vj_paragraph_spacing_limit: f64,
    pub first_baseline: FirstBaseline,
    pub first_baseline_min: f64,
    pub ignore_wrap: bool,
    pub auto_size: AutoSize,
    /// Reference point (0..9, row-major from the top left) that stays fixed when auto-sizing.
    pub auto_size_ref: u8,
    /// Auto-size never makes the frame shorter than `auto_size_min_height` while this is on.
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub auto_size_use_min_height: bool,
    #[serde(skip_serializing_if = "is_default")]
    pub auto_size_min_height: f64,
    /// Auto-size never makes the frame narrower than `auto_size_min_width` while this is on.
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub auto_size_use_min_width: bool,
    #[serde(skip_serializing_if = "is_default")]
    pub auto_size_min_width: f64,
    /// Width-growing auto-size sets each paragraph on one line (the frame widens to the longest).
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub auto_size_no_line_breaks: bool,
    pub column_rule: bool,
    pub column_rule_weight: f64,
    pub column_rule_color: String,
    /// 0..1.
    pub column_rule_tint: f32,
    /// Horizontal offset of each rule from the middle of its gutter.
    pub column_rule_offset: f64,
    /// The rule starts this far below the column top and ends this far above its bottom.
    pub column_rule_top_inset: f64,
    pub column_rule_bottom_inset: f64,
    /// Use a custom baseline grid for this frame.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub baseline_grid: Option<FrameBaselineGrid>,
    /// Type on a Path: the text runs along the item's path instead of filling it.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub path: Option<PathType>,
}

/// A text frame's custom baseline grid. Saved as `[start, increment]` when it starts from the
/// top of the page, otherwise as `[start, increment, relativeTo]`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct FrameBaselineGrid {
    /// The first grid line, down from the line `relative_to` names.
    pub start: f64,
    /// The distance between grid lines; 0 or less turns the grid off.
    pub increment: f64,
    pub relative_to: FrameGridRelative,
}

impl FrameBaselineGrid {
    /// A grid measured from the top of the page.
    pub fn new(start: f64, increment: f64) -> Self {
        FrameBaselineGrid { start, increment, relative_to: FrameGridRelative::TopOfPage }
    }
}

/// What a frame's custom baseline grid's start is measured from.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum FrameGridRelative {
    #[default]
    #[serde(rename = "page")]
    TopOfPage,
    #[serde(rename = "margin")]
    TopMargin,
    #[serde(rename = "frame")]
    TopOfFrame,
    #[serde(rename = "inset")]
    TopInset,
}

impl FrameGridRelative {
    pub const ALL: [FrameGridRelative; 4] =
        [FrameGridRelative::TopOfPage, FrameGridRelative::TopMargin, FrameGridRelative::TopOfFrame, FrameGridRelative::TopInset];

    /// The name commands and native files use: page, margin, frame or inset.
    pub fn name(self) -> &'static str {
        match self {
            FrameGridRelative::TopOfPage => "page",
            FrameGridRelative::TopMargin => "margin",
            FrameGridRelative::TopOfFrame => "frame",
            FrameGridRelative::TopInset => "inset",
        }
    }

    pub fn from_name(name: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|r| r.name() == name)
    }
}

impl Serialize for FrameBaselineGrid {
    fn serialize<S: serde::Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        // Files saved before Relative To existed hold two elements; a page-relative grid keeps that form.
        match self.relative_to {
            FrameGridRelative::TopOfPage => (self.start, self.increment).serialize(s),
            r => (self.start, self.increment, r).serialize(s),
        }
    }
}

impl<'de> Deserialize<'de> for FrameBaselineGrid {
    fn deserialize<D: serde::Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        struct Grid;
        impl<'de> serde::de::Visitor<'de> for Grid {
            type Value = FrameBaselineGrid;
            fn expecting(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
                f.write_str("[start, increment] or [start, increment, relativeTo]")
            }
            fn visit_seq<A: serde::de::SeqAccess<'de>>(self, mut a: A) -> Result<Self::Value, A::Error> {
                use serde::de::Error;
                let start = a.next_element()?.ok_or_else(|| A::Error::invalid_length(0, &self))?;
                let increment = a.next_element()?.ok_or_else(|| A::Error::invalid_length(1, &self))?;
                let relative_to = a.next_element()?.unwrap_or_default();
                if a.next_element::<serde::de::IgnoredAny>()?.is_some() {
                    return Err(A::Error::invalid_length(4, &self));
                }
                Ok(FrameBaselineGrid { start, increment, relative_to })
            }
        }
        d.deserialize_seq(Grid)
    }
}

/// Type on a Path Options.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct PathType {
    /// Where the text starts, as a distance along the path (the start bracket).
    pub start: f64,
    /// Where the text ends, as a distance along the path (the end bracket); `None` is the path's
    /// end. Text past it is overset.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub end: Option<f64>,
    /// Run the other way (the text sits on the other side).
    pub flip: bool,
    pub align: PathAlign,
}

/// Which part of the type sits on the path.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum PathAlign {
    #[default]
    Baseline,
    Ascender,
    Descender,
    Center,
}

impl Default for TextFrameOptions {
    fn default() -> Self {
        TextFrameOptions {
            columns: 1,
            columns_kind: ColumnsKind::FixedNumber,
            gutter: 12.0,
            column_width: 0.0,
            balance_columns: false,
            inset: [0.0; 4],
            vertical_justification: VerticalJustification::Top,
            vj_paragraph_spacing_limit: 0.0,
            first_baseline: FirstBaseline::Ascent,
            first_baseline_min: 0.0,
            ignore_wrap: false,
            auto_size: AutoSize::Off,
            auto_size_ref: 4,
            auto_size_use_min_height: false,
            auto_size_min_height: 0.0,
            auto_size_use_min_width: false,
            auto_size_min_width: 0.0,
            auto_size_no_line_breaks: false,
            column_rule: false,
            column_rule_weight: 1.0,
            column_rule_color: designcraft_color::swatch::BLACK.into(),
            column_rule_tint: 1.0,
            column_rule_offset: 0.0,
            column_rule_top_inset: 0.0,
            column_rule_bottom_inset: 0.0,
            baseline_grid: None,
            path: None,
        }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum WrapMode {
    #[default]
    None,
    BoundingBox,
    Contour,
    JumpObject,
    JumpToNextColumn,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum WrapSide {
    #[default]
    BothSides,
    RightSide,
    LeftSide,
    TowardsSpine,
    AwayFromSpine,
    LargestArea,
}

/// Text Wrap panel › Contour Options › Type: the outline a contour wrap follows on a graphic frame.
/// Other frames always wrap around their own path.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum ContourType {
    /// The placed graphic's rectangle (within the frame).
    BoundingBox,
    /// Edges detected in the image. Composition follows the frame path.
    DetectEdges,
    /// The image's alpha channel. Composition follows the frame path.
    AlphaChannel,
    /// A path saved in the image. Composition follows the frame path.
    PhotoshopPath,
    /// The frame's path.
    GraphicFrame,
    /// The graphic's clipping path, or the frame's path when it has none.
    #[default]
    SameAsClipping,
}

/// Text Wrap panel settings.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct TextWrap {
    pub mode: WrapMode,
    /// Top, left, bottom, right offsets.
    pub offsets: [f64; 4],
    pub invert: bool,
    pub side: WrapSide,
    pub contour: ContourType,
    /// A custom wrap boundary in the item's inner space, offsets already included. Bounding-box
    /// and contour wraps follow it instead of the frame; other modes use its bounds. Resizing the
    /// item transforms it with the path; other path edits keep it as it is.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub path: Option<PathData>,
}

impl TextWrap {
    pub fn is_default(&self) -> bool {
        *self == TextWrap::default()
    }
}

/// Vertical text: composed in a box `area.height()` wide and `area.width()` tall at the origin,
/// placed so lines go down and follow each other leftwards (a clockwise quarter turn).
pub fn vertical_text_xf(area: Rect) -> Affine {
    Affine::new([0.0, 1.0, -1.0, 0.0, area.x1, area.y0])
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TextFrame {
    pub story: StoryId,
    #[serde(default)]
    pub options: TextFrameOptions,
}
