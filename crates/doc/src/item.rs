//! Page items: frames (text / graphic / unassigned), paths, lines and groups.

use std::sync::Arc;

use designcraft_color::BlendMode;
use designcraft_geom::corners::CornerOptions;
use designcraft_geom::{Affine, PathData, Point, Rect};
use serde::{Deserialize, Serialize};

use crate::ids::{AssetId, ItemId, LayerId, StoryId};

/// Fill: a swatch reference and tint (InDesign fills always go through swatches or unnamed colours).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Fill {
    pub swatch: String,
    pub tint: f32,
    /// Gradient angle (degrees) and length override for gradient swatches (`None` = fit to bounds).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub gradient_angle: Option<f64>,
    /// Gradient Swatch tool vector in the item's own space: start and end of a linear gradient,
    /// centre and radius point of a radial one (`None` = fit to bounds at `gradient_angle`).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub gradient_vector: Option<[f64; 4]>,
    pub overprint: bool,
}

impl Default for Fill {
    fn default() -> Self {
        Fill::none()
    }
}

impl Fill {
    /// A radial gradient's ellipse on an object whose bounds (in its own space) are `b`: centre and
    /// horizontal and vertical radii. As in InDesign, a radial gradient stretches with the object's
    /// box: by default it's centred and reaches the box's sides (an ellipse in a non-square box);
    /// with a Gradient Swatch tool vector it's centred at the vector's start, and the vector's
    /// length, measured with the box made square, sets its size.
    pub fn radial_ellipse(&self, b: designcraft_geom::Rect) -> (designcraft_geom::Point, f64, f64) {
        let k = if b.width() > 1e-9 && b.height() > 1e-9 { b.height() / b.width() } else { 1.0 };
        match self.gradient_vector {
            Some([x0, y0, x1, y1]) => {
                let r = designcraft_geom::Vec2::new(x1 - x0, (y1 - y0) / k).hypot();
                (designcraft_geom::Point::new(x0, y0), r, r * k)
            }
            // A box with no width or height (a straight line's) has no ellipse: a circle across
            // its length.
            None if b.width() <= 1e-9 || b.height() <= 1e-9 => {
                let r = b.width().max(b.height()).max(1e-3) / 2.0;
                (b.center(), r, r)
            }
            None => (b.center(), b.width() / 2.0, b.height() / 2.0),
        }
    }

    /// A linear gradient's axis on an object whose bounds (in its own space) are `b`: the
    /// Gradient Swatch tool's vector, or the box's extent along the gradient's angle through its
    /// centre. A box with no extent along that angle (a straight line, across it) takes its longer
    /// side.
    pub fn linear_axis(&self, b: Rect) -> (Point, Point) {
        if let Some([x0, y0, x1, y1]) = self.gradient_vector {
            return (Point::new(x0, y0), Point::new(x1, y1));
        }
        let a = self.gradient_angle.unwrap_or(0.0).to_radians();
        let half = (b.width() * a.cos().abs() + b.height() * a.sin().abs()) / 2.0;
        let half = if half > 1e-9 { half } else { b.width().max(b.height()).max(1e-3) / 2.0 };
        let d = designcraft_geom::Vec2::new(a.cos(), -a.sin()) * half;
        (b.center() - d, b.center() + d)
    }

    pub fn none() -> Self {
        Fill { swatch: designcraft_color::swatch::NONE.into(), tint: 1.0, gradient_angle: None, gradient_vector: None, overprint: false }
    }
    pub fn swatch(name: &str) -> Self {
        Fill { swatch: name.into(), ..Fill::none() }
    }
    pub fn is_none(&self) -> bool {
        self.swatch == designcraft_color::swatch::NONE
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum StrokeAlign {
    #[default]
    Center,
    Inside,
    Outside,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Cap {
    #[default]
    Butt,
    Round,
    Projecting,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Join {
    #[default]
    Miter,
    Round,
    Bevel,
}

/// The shortest non-zero dash or gap a command accepts, in points.
pub const MIN_DASH: f64 = 0.1;
/// The longest dash or gap a command accepts, in points.
pub const MAX_DASH: f64 = 10_000.0;
/// The most lengths a dash pattern holds.
pub const MAX_DASH_ENTRIES: usize = 64;
/// The most pattern repeats a renderer expands along one path; finer patterns draw solid.
pub const MAX_DASH_PERIODS: f64 = 20_000.0;

/// An upper bound of the length of `bp`: the length of its control polygon.
pub fn path_length_bound(bp: &designcraft_geom::BezPath) -> f64 {
    use designcraft_geom::PathSeg;
    bp.segments()
        .map(|s| match s {
            PathSeg::Line(l) => l.p0.distance(l.p1),
            PathSeg::Quad(q) => q.p0.distance(q.p1) + q.p1.distance(q.p2),
            PathSeg::Cubic(c) => c.p0.distance(c.p1) + c.p1.distance(c.p2) + c.p2.distance(c.p3),
        })
        .sum()
}

/// Stroke types (Stroke panel → Type). Dash/gap arrays are in points.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", tag = "kind")]
pub enum StrokeType {
    #[default]
    Solid,
    Dashed {
        pattern: Vec<f64>,
    },
    Dotted,
    ThickThin,
    ThinThick,
    ThinThin,
    ThickThick,
    ThinThickThin,
    ThickThinThick,
    Wavy,
    Hashed,
    /// A custom striped stroke: bands (start, width) as fractions of the weight from the left edge.
    Stripes {
        bands: Vec<(f64, f64)>,
    },
    /// A named stroke style of the document (Stroke Styles), by name.
    Style {
        name: String,
    },
}

impl StrokeType {
    pub fn label(&self) -> &'static str {
        match self {
            StrokeType::Solid => "Solid",
            StrokeType::Dashed { .. } => "Dashed",
            StrokeType::Dotted => "Dotted",
            StrokeType::ThickThin => "Thick - Thin",
            StrokeType::ThinThick => "Thin - Thick",
            StrokeType::ThinThin => "Thin - Thin",
            StrokeType::ThickThick => "Thick - Thick",
            StrokeType::ThinThickThin => "Thin - Thick - Thin",
            StrokeType::ThickThinThick => "Thick - Thin - Thick",
            StrokeType::Wavy => "Wavy",
            StrokeType::Hashed => "Straight Hash",
            StrokeType::Stripes { .. } => "Stripes",
            StrokeType::Style { .. } => "Custom",
        }
    }

    /// Whether `pattern` is a dash pattern commands accept: 1 to [`MAX_DASH_ENTRIES`] lengths,
    /// each 0 or from [`MIN_DASH`] to [`MAX_DASH`] points, not all 0.
    pub fn valid_dash_pattern(pattern: &[f64]) -> bool {
        !pattern.is_empty()
            && pattern.len() <= MAX_DASH_ENTRIES
            && pattern.iter().all(|x| *x == 0.0 || (MIN_DASH..=MAX_DASH).contains(x))
            && pattern.iter().any(|x| *x > 0.0)
    }

    /// Whether a renderer may expand `pattern` along a path at most `length` points long: at
    /// most [`MAX_DASH_ENTRIES`] finite, non-negative lengths that repeat at most
    /// [`MAX_DASH_PERIODS`] times. Otherwise the stroke draws solid, so a degenerate pattern
    /// from a file can't stall rendering.
    pub fn expandable_dashes(pattern: &[f64], length: f64) -> bool {
        let period: f64 = pattern.iter().sum();
        pattern.len() <= MAX_DASH_ENTRIES
            && pattern.iter().all(|x| x.is_finite() && *x >= 0.0)
            && period > 0.0
            && length.is_finite()
            && length / period <= MAX_DASH_PERIODS
    }

    /// The bands of a striped type (start, width as fractions of the weight).
    pub fn bands(&self) -> Option<Vec<(f64, f64)>> {
        Some(match self {
            StrokeType::ThickThin => vec![(0.0, 0.5), (0.75, 0.25)],
            StrokeType::ThinThick => vec![(0.0, 0.25), (0.5, 0.5)],
            StrokeType::ThinThin => vec![(0.0, 1.0 / 3.0), (2.0 / 3.0, 1.0 / 3.0)],
            StrokeType::ThickThick => vec![(0.0, 0.4), (0.6, 0.4)],
            StrokeType::ThinThickThin => vec![(0.0, 0.2), (0.35, 0.3), (0.8, 0.2)],
            StrokeType::ThickThinThick => vec![(0.0, 0.35), (0.45, 0.1), (0.65, 0.35)],
            StrokeType::Stripes { bands } => bands.clone(),
            _ => return None,
        })
    }

    /// The filled area for types drawn as fills (stripes, wavy, hash); `None` for solid, dashed
    /// and dotted strokes (and unresolved named styles).
    pub fn outline(&self, bp: &designcraft_geom::BezPath, weight: f64, tol: f64) -> Option<designcraft_geom::BezPath> {
        use designcraft_geom::stroke_style as ss;
        match self {
            StrokeType::Wavy => Some(ss::wavy(bp, weight, tol)),
            StrokeType::Hashed => Some(ss::hashed(bp, weight, tol)),
            t => t.bands().map(|b| ss::stripes(bp, weight, &b, tol)),
        }
    }
}

/// A named custom stroke style (Stroke Styles dialog): dashes, dots or stripes.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StrokeStyleDef {
    pub name: String,
    pub kind: StrokeType,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Arrowhead {
    #[default]
    None,
    SimpleWide,
    Simple,
    Triangle,
    TriangleWide,
    Barbed,
    Curved,
    Circle,
    CircleSolid,
    Square,
    SquareSolid,
    Bar,
}

impl Arrowhead {
    pub const ALL: [Arrowhead; 12] = [
        Arrowhead::None,
        Arrowhead::Simple,
        Arrowhead::SimpleWide,
        Arrowhead::Triangle,
        Arrowhead::TriangleWide,
        Arrowhead::Barbed,
        Arrowhead::Curved,
        Arrowhead::Circle,
        Arrowhead::CircleSolid,
        Arrowhead::Square,
        Arrowhead::SquareSolid,
        Arrowhead::Bar,
    ];

    pub fn label(self) -> &'static str {
        match self {
            Arrowhead::None => "None",
            Arrowhead::Simple => "Simple",
            Arrowhead::SimpleWide => "Simple - Wide",
            Arrowhead::Triangle => "Triangle",
            Arrowhead::TriangleWide => "Triangle - Wide",
            Arrowhead::Barbed => "Barbed",
            Arrowhead::Curved => "Curved",
            Arrowhead::Circle => "Circle",
            Arrowhead::CircleSolid => "Circle - Solid",
            Arrowhead::Square => "Square",
            Arrowhead::SquareSolid => "Square - Solid",
            Arrowhead::Bar => "Bar",
        }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Stroke {
    pub swatch: String,
    pub tint: f32,
    pub weight: f64,
    #[serde(rename = "type")]
    pub kind: StrokeType,
    pub align: StrokeAlign,
    pub cap: Cap,
    pub join: Join,
    pub miter_limit: f64,
    pub start: Arrowhead,
    pub end: Arrowhead,
    /// Gap colour for dashed/striped strokes.
    pub gap_swatch: String,
    pub gap_tint: f32,
    pub overprint: bool,
    /// Overprint Gap (Attributes panel).
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub gap_overprint: bool,
    /// A gradient swatch's angle on the stroke, as [`Fill::gradient_angle`].
    #[serde(skip_serializing_if = "Option::is_none")]
    pub gradient_angle: Option<f64>,
    /// The Gradient Swatch tool's vector on the stroke, as [`Fill::gradient_vector`].
    #[serde(skip_serializing_if = "Option::is_none")]
    pub gradient_vector: Option<[f64; 4]>,
}

impl Default for Stroke {
    /// InDesign's default for drawn shapes: 1 pt black.
    fn default() -> Self {
        Stroke {
            swatch: designcraft_color::swatch::BLACK.into(),
            tint: 1.0,
            weight: 1.0,
            kind: StrokeType::Solid,
            align: StrokeAlign::Center,
            cap: Cap::Butt,
            join: Join::Miter,
            miter_limit: 4.0,
            start: Arrowhead::None,
            end: Arrowhead::None,
            gap_swatch: designcraft_color::swatch::NONE.into(),
            gap_tint: 1.0,
            overprint: false,
            gap_overprint: false,
            gradient_angle: None,
            gradient_vector: None,
        }
    }
}

impl Stroke {
    /// The stroke's paint (swatch, tint and gradient geometry) as a fill, to paint it the same way:
    /// a gradient swatch on a stroke runs across the object's box as on its fill.
    pub fn as_fill(&self) -> Fill {
        Fill {
            swatch: self.swatch.clone(),
            tint: self.tint,
            gradient_angle: self.gradient_angle,
            gradient_vector: self.gradient_vector,
            overprint: self.overprint,
        }
    }

    /// How far the painted stroke can reach beyond the path (arrowheads reach further).
    pub fn extent(&self) -> f64 {
        if self.start == Arrowhead::None && self.end == Arrowhead::None { self.weight } else { self.weight.max(0.5) * 5.5 }
    }

    pub fn none() -> Self {
        Stroke { swatch: designcraft_color::swatch::NONE.into(), ..Stroke::default() }
    }
    pub fn is_none(&self) -> bool {
        self.swatch == designcraft_color::swatch::NONE || self.weight <= 0.0
    }
}

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
    /// Preserve the ascent-based first-baseline calculation of native format 1 documents.
    /// New documents use typographic ascenders and measured cap/x heights.
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub legacy_first_baseline: bool,
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
    pub baseline_grid: Option<(f64, f64)>,
    /// Type on a Path: the text runs along the item's path instead of filling it.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub path: Option<PathType>,
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
            legacy_first_baseline: false,
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

/// Frame fitting options (Object → Fitting → Frame Fitting Options).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Fitting {
    #[default]
    None,
    FillProportionally,
    FitProportionally,
    FitContentToFrame,
    CenterContent,
}

/// A placed graphic inside a frame.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Graphic {
    pub asset: AssetId,
    /// Natural size of the graphic in points (pixels at its resolution, or PDF page size).
    pub size: (f64, f64),
    /// Graphic space (0,0)-(w,h) → frame inner space.
    pub xf: Affine,
    /// Clipping path: only the part of the graphic inside it shows.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub clip: Option<Arc<ClippingPath>>,
    #[serde(default)]
    pub auto_fit: Fitting,
    /// Frame Fitting Options › Align From: the reference point (0–8, row-major; 4 = centre).
    #[serde(default = "center_ref")]
    pub fit_align: u8,
    /// Frame Fitting Options › Crop Amount: top, left, bottom, right (negative adds space).
    #[serde(default)]
    pub crop: [f64; 4],
    /// Text wrap set on the graphic itself (Direct Selection), around the part its frame shows.
    #[serde(default, skip_serializing_if = "TextWrap::is_default")]
    pub wrap: TextWrap,
    /// The swatch applied to the image itself: InDesign colours a greyscale or bitmap image with
    /// it, colour images ignore it. `None` (the default, like InDesign's Black) leaves it as is.
    #[serde(default, skip_serializing_if = "Fill::is_none")]
    pub fill: Fill,
}

fn center_ref() -> u8 {
    4
}

/// Most anchors a clipping path read from a file keeps (detected edges can be dense).
pub const MAX_CLIP_ANCHORS: usize = 100_000;

/// Clipping Path › Type: where the clipping path came from.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum ClippingKind {
    /// Traced from the image's edges against a light background.
    DetectEdges,
    /// Traced from one of the image's alpha channels.
    AlphaChannel,
    /// A path saved in the image file.
    PhotoshopPath,
    /// One of the above, edited by hand.
    UserModifiedPath,
}

/// A placed graphic's clipping path. The tracing options are kept as the file had them; the
/// outline itself is `path`.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ClippingPath {
    pub kind: ClippingKind,
    /// The outline in graphic space ((0,0)-(w,h) is the graphic's natural rectangle).
    pub path: PathData,
    /// Show the graphic outside the path instead of inside it.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub invert: bool,
    /// Holes inside the traced shape are part of the path.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub include_inside_edges: bool,
    /// Detect Edges: trace only the part of the image the frame shows.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub restrict_to_frame: bool,
    /// Trace the full-resolution image rather than the screen proxy.
    #[serde(default = "yes")]
    pub use_high_resolution: bool,
    /// Detect Edges / Alpha Channel: the darkness or opacity level that counts as inside (0–255).
    #[serde(default = "clip_threshold")]
    pub threshold: f64,
    /// How closely the traced path follows the pixels, in pixels.
    #[serde(default = "clip_tolerance")]
    pub tolerance: f64,
    /// Inset Frame: how far the path is pulled in (points; negative pushes it out).
    #[serde(default)]
    pub inset: f64,
    /// The alpha channel or image path the clip came from.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub source_name: String,
    /// Index of the alpha channel (-1 = none).
    #[serde(default = "no_index")]
    pub source_index: i32,
}

fn yes() -> bool {
    true
}
fn clip_threshold() -> f64 {
    25.0
}
fn clip_tolerance() -> f64 {
    2.0
}
fn no_index() -> i32 {
    -1
}

impl ClippingPath {
    /// A clipping path of `kind` with the default options.
    pub fn new(kind: ClippingKind, path: PathData) -> Self {
        Self {
            kind,
            path,
            invert: false,
            include_inside_edges: false,
            restrict_to_frame: false,
            use_high_resolution: true,
            threshold: clip_threshold(),
            tolerance: clip_tolerance(),
            inset: 0.0,
            source_name: String::new(),
            source_index: no_index(),
        }
    }
}

/// Liquid Layout object rules: which edges stay at their distance from the page edge, and
/// whether the object may stretch.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct ObjectLiquid {
    pub resize_width: bool,
    pub resize_height: bool,
    pub pin_top: bool,
    pub pin_bottom: bool,
    pub pin_left: bool,
    pub pin_right: bool,
}

/// Object Export Options: tagged PDF and EPUB/HTML handling of one object.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct ExportOptions {
    /// Tagged PDF: the object is decoration (an artifact), not content.
    pub artifact: bool,
    /// EPUB/HTML: export the object as an image (whatever it is).
    pub rasterize: bool,
    /// EPUB/HTML: "left", "center" or "right" ("" = as the flow puts it).
    pub align: String,
    /// EPUB/HTML: start a new page (screen) before the object.
    pub page_break_before: bool,
}

impl ExportOptions {
    pub fn is_default(&self) -> bool {
        *self == ExportOptions::default()
    }
}

/// A PDF form field.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FormField {
    pub kind: FieldKind,
    /// Field name (unique in the exported PDF).
    pub name: String,
    /// Default value: the text, the chosen option, or "On"/"" for a check box.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub value: String,
    /// Choices of a combo box or list box.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub options: Vec<String>,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub multiline: bool,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub required: bool,
    /// Font size of the field's text (0 = auto).
    #[serde(default)]
    pub font_size: f64,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum FieldKind {
    TextField,
    CheckBox,
    ComboBox,
    ListBox,
    Signature,
}

/// The source and template of a live caption.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LiveCaption {
    pub source: ItemId,
    pub template: String,
}

/// Media panel options for a placed video or sound.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct MediaOptions {
    pub play_on_page_load: bool,
    #[serde(rename = "loop")]
    pub looping: bool,
    /// Show the player controls.
    pub controls: bool,
    /// Poster image (shown on the page and in print); `None` = a placeholder frame.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub poster: Option<AssetId>,
}

impl Item {
    /// The graphic to draw for this frame: a media frame with a poster draws the poster
    /// stretched over the media rectangle.
    pub fn drawn_graphic(&self, assets: &std::collections::BTreeMap<AssetId, Arc<crate::Asset>>) -> Option<Graphic> {
        let Content::Graphic(g) = &self.content else { return None };
        if let Some(poster) = self.media.as_ref().and_then(|m| m.poster)
            && let Some((pw, ph)) = assets.get(&poster).and_then(|a| a.pixels)
        {
            let (pw, ph) = (pw.max(1) as f64, ph.max(1) as f64);
            return Some(Graphic { asset: poster, size: (pw, ph), xf: g.xf * Affine::scale_non_uniform(g.size.0 / pw, g.size.1 / ph), ..g.clone() });
        }
        Some(g.clone())
    }
}

/// Vertical text: composed in a box `area.height()` wide and `area.width()` tall at the origin,
/// placed so lines go down and follow each other leftwards (a clockwise quarter turn).
pub fn vertical_text_xf(area: Rect) -> Affine {
    Affine::new([0.0, 1.0, -1.0, 0.0, area.x1, area.y0])
}

/// "video" or "audio" for a media MIME type.
pub fn media_kind(mime: &str) -> Option<&'static str> {
    if mime.starts_with("video/") {
        Some("video")
    } else if mime.starts_with("audio/") {
        Some("audio")
    } else {
        None
    }
}

/// MIME type of a media file by its extension.
pub fn media_mime(name: &str) -> Option<&'static str> {
    let ext = name.rsplit('.').next()?.to_ascii_lowercase();
    Some(match ext.as_str() {
        "mp4" | "m4v" => "video/mp4",
        "mov" => "video/quicktime",
        "webm" => "video/webm",
        "ogv" => "video/ogg",
        "mp3" => "audio/mpeg",
        "m4a" => "audio/mp4",
        "wav" => "audio/wav",
        "ogg" | "oga" => "audio/ogg",
        "aac" => "audio/aac",
        _ => return None,
    })
}

/// A button's On Release action.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", tag = "kind")]
pub enum ButtonAction {
    GoToPage { page: usize },
    GoToFirstPage,
    GoToLastPage,
    GoToNextPage,
    GoToPreviousPage,
    GoToUrl { url: String },
}

fn is_zero_usize(v: &usize) -> bool {
    *v == 0
}

impl Graphic {
    /// The part of the graphic its frame shows, in the frame's inner space (bounding boxes);
    /// None when nothing shows.
    pub fn shown_bounds(&self, frame: Rect) -> Option<Rect> {
        let r = self.xf.transform_rect_bbox(Rect::new(0.0, 0.0, self.size.0, self.size.1)).intersect(frame);
        (r.width() > 0.0 && r.height() > 0.0).then_some(r)
    }

    /// The region the clipping path lets through, in frame inner space, and whether it is filled
    /// even-odd (an inverted clip: the graphic's rectangle minus the path). `None` when the graphic
    /// has no clipping path or the path is empty.
    pub fn clip_region(&self) -> Option<(designcraft_geom::BezPath, bool)> {
        use designcraft_geom::Shape;
        let c = self.clip.as_ref().filter(|c| !c.path.is_empty())?;
        let mut path = c.path.to_bezpath();
        if !c.invert {
            path.apply_affine(self.xf);
            return Some((path, false));
        }
        let outer = Rect::new(0.0, 0.0, self.size.0, self.size.1).union(path.bounding_box()).inflate(1.0, 1.0);
        let mut region = outer.to_path(0.1);
        region.extend(path.iter());
        region.apply_affine(self.xf);
        Some((region, true))
    }

    /// The graphic transform that fits it into `frame` (the frame's inner rect) with `mode`,
    /// honouring the crop amounts and the reference point.
    pub fn fitted(&self, frame: Rect, mode: Fitting) -> Option<Affine> {
        let (nw, nh) = self.size;
        if nw <= 0.0 || nh <= 0.0 {
            return None;
        }
        let [t, l, b, r] = self.crop;
        let area = Rect::new(frame.x0 - l, frame.y0 - t, frame.x1 + r, frame.y1 + b);
        if area.width() <= 0.0 || area.height() <= 0.0 {
            return None;
        }
        let (fx, fy) = ([0.0, 0.5, 1.0][(self.fit_align % 3) as usize], [0.0, 0.5, 1.0][(self.fit_align / 3).min(2) as usize]);
        let place = |w: f64, h: f64| (area.x0 + (area.width() - w) * fx, area.y0 + (area.height() - h) * fy);
        Some(match mode {
            Fitting::None => return None,
            Fitting::FillProportionally | Fitting::FitProportionally => {
                let k = if mode == Fitting::FillProportionally {
                    (area.width() / nw).max(area.height() / nh)
                } else {
                    (area.width() / nw).min(area.height() / nh)
                };
                Affine::translate(place(nw * k, nh * k)) * Affine::scale(k)
            }
            Fitting::FitContentToFrame => Affine::translate((area.x0, area.y0)) * Affine::scale_non_uniform(area.width() / nw, area.height() / nh),
            Fitting::CenterContent => {
                let cur = self.xf.transform_rect_bbox(Rect::new(0.0, 0.0, nw, nh));
                let (x, y) = place(cur.width(), cur.height());
                Affine::translate((x - cur.x0, y - cur.y0)) * self.xf
            }
        })
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TextFrame {
    pub story: StoryId,
    #[serde(default)]
    pub options: TextFrameOptions,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", tag = "type")]
pub enum Content {
    #[default]
    Unassigned,
    Text(TextFrame),
    Graphic(Graphic),
    Group {
        items: Vec<Arc<Item>>,
    },
}

/// What kind of spline the item was drawn as (labels in the Layers panel, tool behaviours).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Shape {
    #[default]
    Rectangle,
    Oval,
    Polygon,
    GraphicLine,
    Path,
    Group,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct DropShadow {
    pub on: bool,
    pub color: String,
    pub opacity: f32,
    pub angle: f64,
    /// Use Global Light (the document angle instead of `angle`).
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub global_light: bool,
    pub distance: f64,
    pub size: f64,
    pub spread: f64,
}

impl Default for DropShadow {
    fn default() -> Self {
        DropShadow {
            on: false,
            color: designcraft_color::swatch::BLACK.into(),
            opacity: 0.75,
            angle: 120.0,
            distance: 7.0,
            size: 5.0,
            spread: 0.0,
            global_light: false,
        }
    }
}

/// Inner shadow: a blurred, offset shadow inside the object's edge.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct InnerShadow {
    pub on: bool,
    pub color: String,
    pub opacity: f32,
    pub angle: f64,
    /// Use Global Light (the document angle instead of `angle`).
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub global_light: bool,
    pub distance: f64,
    pub size: f64,
    /// Percentage (0–100) of `size` that hardens the edge instead of blurring it.
    pub choke: f64,
}

impl Default for InnerShadow {
    fn default() -> Self {
        InnerShadow {
            on: false,
            color: designcraft_color::swatch::BLACK.into(),
            opacity: 0.75,
            angle: 120.0,
            distance: 7.0,
            size: 7.0,
            choke: 0.0,
            global_light: false,
        }
    }
}

/// Outer glow: a blurred silhouette around the object, without offset.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct OuterGlow {
    pub on: bool,
    pub color: String,
    pub opacity: f32,
    pub size: f64,
    /// Percentage (0–100) of `size` that grows the silhouette instead of blurring it.
    pub spread: f64,
}

impl Default for OuterGlow {
    fn default() -> Self {
        OuterGlow { on: false, color: designcraft_color::swatch::BLACK.into(), opacity: 0.75, size: 7.0, spread: 0.0 }
    }
}

fn is_default<T: Default + PartialEq>(v: &T) -> bool {
    *v == T::default()
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Effects {
    /// Drop shadow. `size` is the blur size; `spread` is a percentage (0–100) of it that grows
    /// the silhouette instead of blurring it.
    pub drop_shadow: DropShadow,
    /// Basic feather width (0 = off).
    pub feather: f64,
    /// Basic feather's choke and corners.
    #[serde(skip_serializing_if = "is_default")]
    pub feather_options: FeatherOptions,
    #[serde(skip_serializing_if = "is_default")]
    pub inner_shadow: InnerShadow,
    #[serde(skip_serializing_if = "is_default")]
    pub outer_glow: OuterGlow,
    #[serde(skip_serializing_if = "is_default")]
    pub gradient_feather: GradientFeather,
    #[serde(skip_serializing_if = "is_default")]
    pub inner_glow: InnerGlow,
    #[serde(skip_serializing_if = "is_default")]
    pub bevel: Bevel,
    #[serde(skip_serializing_if = "is_default")]
    pub satin: Satin,
    #[serde(skip_serializing_if = "is_default")]
    pub directional_feather: DirectionalFeather,
}

/// Basic Feather › Corners: how the fade follows the shape's corners.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum FeatherCorners {
    /// Mitred: the fade keeps the corners sharp.
    Sharp,
    /// The fade rounds the corners by its width.
    Rounded,
    /// From opaque to transparent, softly (InDesign's default).
    #[default]
    Diffused,
}

/// Basic Feather options besides its width.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct FeatherOptions {
    /// Percentage (0–100) of the width that stays opaque: the fade gets shorter.
    pub choke: f64,
    pub corners: FeatherCorners,
}

/// Inner/Outer Glow › Technique.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum GlowTechnique {
    /// A soft, blurred glow.
    #[default]
    Softer,
    /// A glow that keeps closer to the shape's edges.
    Precise,
}

/// Directional Feather: each side fades over its own width (item space, before rotation).
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct DirectionalFeather {
    pub on: bool,
    /// Top, left, bottom, right widths in points.
    pub widths: [f64; 4],
}

/// Inner Glow: a glow inside the edge (or from the centre).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct InnerGlow {
    pub on: bool,
    pub color: String,
    pub opacity: f32,
    pub size: f64,
    /// Percentage (0–100) of `size` that hardens the glow.
    pub choke: f64,
    /// Glow from the centre instead of the edges.
    pub center: bool,
    /// How the glow blends with the object (InDesign: Screen).
    pub mode: BlendMode,
    pub technique: GlowTechnique,
}

impl Default for InnerGlow {
    fn default() -> Self {
        InnerGlow {
            on: false,
            color: designcraft_color::swatch::PAPER.into(),
            opacity: 0.75,
            size: 7.0,
            choke: 0.0,
            center: false,
            mode: BlendMode::Screen,
            technique: GlowTechnique::Softer,
        }
    }
}

/// Bevel and Emboss (inner bevel): a highlight on the lit edges, a shadow on the others.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Bevel {
    pub on: bool,
    pub size: f64,
    /// Depth in percent (scales the highlight and shadow offset).
    pub depth: f64,
    pub angle: f64,
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub global_light: bool,
    pub highlight: String,
    pub highlight_opacity: f32,
    pub shadow: String,
    pub shadow_opacity: f32,
}

impl Default for Bevel {
    fn default() -> Self {
        Bevel {
            on: false,
            size: 7.0,
            depth: 100.0,
            angle: 120.0,
            global_light: true,
            highlight: designcraft_color::swatch::PAPER.into(),
            highlight_opacity: 0.75,
            shadow: designcraft_color::swatch::BLACK.into(),
            shadow_opacity: 0.75,
        }
    }
}

/// Satin: soft interior shading from two offset copies of the shape.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Satin {
    pub on: bool,
    pub color: String,
    pub opacity: f32,
    pub angle: f64,
    pub distance: f64,
    pub size: f64,
    pub invert: bool,
}

impl Default for Satin {
    fn default() -> Self {
        Satin { on: false, color: designcraft_color::swatch::BLACK.into(), opacity: 0.5, angle: 120.0, distance: 7.0, size: 7.0, invert: false }
    }
}

/// Gradient Feather: the object fades from `start` to `end` opacity along a gradient.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct GradientFeather {
    pub on: bool,
    pub radial: bool,
    /// Degrees (linear), when no vector was dragged.
    pub angle: f64,
    pub start: f32,
    pub end: f32,
    /// Gradient Feather tool vector in the item's space: start and end (linear), centre and
    /// radius point (radial).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub vector: Option<[f64; 4]>,
}

impl Default for GradientFeather {
    fn default() -> Self {
        GradientFeather { on: false, radial: false, angle: 0.0, start: 1.0, end: 0.0, vector: None }
    }
}

impl GradientFeather {
    /// The gradient's two points in item space for an object with inner bounds `b`.
    pub fn points(&self, b: designcraft_geom::Rect) -> (designcraft_geom::Point, designcraft_geom::Point) {
        use designcraft_geom::{Point, Vec2};
        if let Some([x0, y0, x1, y1]) = self.vector {
            return (Point::new(x0, y0), Point::new(x1, y1));
        }
        let c = b.center();
        if self.radial {
            return (c, c + Vec2::new(b.width().max(b.height()) / 2.0, 0.0));
        }
        let a = self.angle.to_radians();
        let d = Vec2::new(a.cos(), -a.sin());
        let half = (b.width() * d.x.abs() + b.height() * d.y.abs()) / 2.0;
        (c - d * half, c + d * half)
    }
}

impl Effects {
    /// These effects with opacities in 0–1, percentages in 0–100 and sizes, widths and distances
    /// not below 0, or an error naming the first value that isn't a finite number.
    pub fn sanitized(mut self) -> Result<Effects, String> {
        fn finite(name: &str, v: f64) -> Result<f64, String> {
            if v.is_finite() { Ok(v) } else { Err(format!("{name} must be a finite number")) }
        }
        let len = |name: &str, v: f64| finite(name, v).map(|v| v.max(0.0));
        let pct = |name: &str, v: f64| finite(name, v).map(|v| v.clamp(0.0, 100.0));
        let opacity = |name: &str, v: f32| finite(name, f64::from(v)).map(|v| v.clamp(0.0, 1.0) as f32);
        let ds = &mut self.drop_shadow;
        ds.opacity = opacity("dropShadow.opacity", ds.opacity)?;
        ds.angle = finite("dropShadow.angle", ds.angle)?;
        ds.distance = len("dropShadow.distance", ds.distance)?;
        ds.size = len("dropShadow.size", ds.size)?;
        ds.spread = pct("dropShadow.spread", ds.spread)?;
        self.feather = len("feather", self.feather)?;
        let e = &mut self.inner_shadow;
        e.opacity = opacity("innerShadow.opacity", e.opacity)?;
        e.angle = finite("innerShadow.angle", e.angle)?;
        e.distance = len("innerShadow.distance", e.distance)?;
        e.size = len("innerShadow.size", e.size)?;
        e.choke = pct("innerShadow.choke", e.choke)?;
        let e = &mut self.outer_glow;
        e.opacity = opacity("outerGlow.opacity", e.opacity)?;
        e.size = len("outerGlow.size", e.size)?;
        e.spread = pct("outerGlow.spread", e.spread)?;
        let e = &mut self.gradient_feather;
        e.angle = finite("gradientFeather.angle", e.angle)?;
        e.start = opacity("gradientFeather.start", e.start)?;
        e.end = opacity("gradientFeather.end", e.end)?;
        if let Some(v) = &mut e.vector {
            for c in v.iter_mut() {
                *c = finite("gradientFeather.vector", *c)?;
            }
        }
        let e = &mut self.inner_glow;
        e.opacity = opacity("innerGlow.opacity", e.opacity)?;
        e.size = len("innerGlow.size", e.size)?;
        e.choke = pct("innerGlow.choke", e.choke)?;
        let e = &mut self.bevel;
        e.size = len("bevel.size", e.size)?;
        e.depth = len("bevel.depth", e.depth)?;
        e.angle = finite("bevel.angle", e.angle)?;
        e.highlight_opacity = opacity("bevel.highlightOpacity", e.highlight_opacity)?;
        e.shadow_opacity = opacity("bevel.shadowOpacity", e.shadow_opacity)?;
        let e = &mut self.satin;
        e.opacity = opacity("satin.opacity", e.opacity)?;
        e.angle = finite("satin.angle", e.angle)?;
        e.distance = len("satin.distance", e.distance)?;
        e.size = len("satin.size", e.size)?;
        for w in self.directional_feather.widths.iter_mut() {
            *w = len("directionalFeather.widths", *w)?;
        }
        Ok(self)
    }

    /// Any effect drawn with soft (raster) filters?
    pub fn any(&self) -> bool {
        self.drop_shadow.on
            || self.feather > 0.0
            || self.inner_shadow.on
            || self.outer_glow.on
            || self.gradient_feather.on
            || self.inner_glow.on
            || self.bevel.on
            || self.satin.on
            || self.directional_feather.on
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Item {
    pub id: ItemId,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub name: String,
    pub layer: LayerId,
    #[serde(default)]
    pub shape: Shape,
    /// Geometry in the item's inner coordinate space.
    pub path: PathData,
    /// Inner space → spread space.
    #[serde(default = "identity")]
    pub xf: Affine,
    #[serde(default)]
    pub fill: Fill,
    #[serde(default = "Stroke::none")]
    pub stroke: Stroke,
    #[serde(default)]
    pub corners: CornerOptions,
    #[serde(default = "one")]
    pub opacity: f32,
    #[serde(default)]
    pub blend: BlendMode,
    #[serde(default)]
    pub effects: Effects,
    #[serde(default)]
    pub wrap: TextWrap,
    #[serde(default)]
    pub locked: bool,
    #[serde(default)]
    pub hidden: bool,
    #[serde(default)]
    pub nonprinting: bool,
    #[serde(default)]
    pub object_style: String,
    #[serde(default)]
    pub content: Content,
    /// For items on a document page that override a parent-page item.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub overrides: Option<ItemId>,
    /// Script Label (for scripts and agents).
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub label: String,
    /// Alternative text (Object Export Options) for tagged PDF and EPUB.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub alt_text: String,
    /// Liquid Layout (object-based pages): pins and resize permissions.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub liquid: Option<ObjectLiquid>,
    /// Object Export Options (beyond alt text).
    #[serde(default, skip_serializing_if = "ExportOptions::is_default")]
    pub export_options: ExportOptions,
    /// Buttons and Forms: an interactive PDF form field drawn in this frame.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub form_field: Option<FormField>,
    /// Live caption: this text frame shows `template` filled from the source object's metadata,
    /// kept current as the source changes.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub live_caption: Option<LiveCaption>,
    /// Object › Object Layer Options: layers of a placed PDF that are hidden.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub pdf_hidden_layers: Vec<String>,
    /// Window › Interactive › Media: options of a placed video or sound.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub media: Option<MediaOptions>,
    /// Buttons and Forms: what clicking this object does in an interactive PDF.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub button: Option<ButtonAction>,
    /// Object States: a group whose children are states (named here); only `active_state` shows.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub states: Vec<String>,
    #[serde(default, skip_serializing_if = "is_zero_usize")]
    pub active_state: usize,
    /// XML tag (Tags panel); its content is the element's content in XML export/import.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub xml_tag: String,
    /// Groups: Isolate Blending (blend modes only within the group) and Knockout Group (the
    /// group's objects don't show through each other).
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub isolate: bool,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub knockout: bool,
    /// Flex Layout: this frame lays out the objects nested in it (see [`crate::flex`]).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub flex: Option<crate::flex::FlexLayout>,
}

fn identity() -> Affine {
    Affine::IDENTITY
}
fn one() -> f32 {
    1.0
}

impl Item {
    pub fn new(id: ItemId, layer: LayerId, shape: Shape, path: PathData) -> Self {
        Item {
            id,
            name: String::new(),
            layer,
            shape,
            path,
            xf: Affine::IDENTITY,
            fill: Fill::none(),
            stroke: Stroke::none(),
            corners: CornerOptions::default(),
            opacity: 1.0,
            blend: BlendMode::default(),
            effects: Effects::default(),
            wrap: TextWrap::default(),
            locked: false,
            hidden: false,
            nonprinting: false,
            object_style: crate::styles::NO_OBJECT_STYLE.into(),
            content: Content::Unassigned,
            overrides: None,
            label: String::new(),
            alt_text: String::new(),
            xml_tag: String::new(),
            button: None,
            pdf_hidden_layers: Vec::new(),
            live_caption: None,
            form_field: None,
            export_options: ExportOptions::default(),
            media: None,
            liquid: None,
            states: Vec::new(),
            active_state: 0,
            isolate: false,
            knockout: false,
            flex: None,
        }
    }

    pub fn is_text_frame(&self) -> bool {
        matches!(self.content, Content::Text(_))
    }
    pub fn text_frame(&self) -> Option<&TextFrame> {
        match &self.content {
            Content::Text(t) => Some(t),
            _ => None,
        }
    }
    pub fn text_frame_mut(&mut self) -> Option<&mut TextFrame> {
        match &mut self.content {
            Content::Text(t) => Some(t),
            _ => None,
        }
    }
    /// Does this item, or the graphic it holds, wrap text?
    pub fn has_wrap(&self) -> bool {
        self.wrap.mode != WrapMode::None || self.graphic().is_some_and(|g| g.wrap.mode != WrapMode::None)
    }
    pub fn graphic(&self) -> Option<&Graphic> {
        match &self.content {
            Content::Graphic(g) => Some(g),
            _ => None,
        }
    }
    pub fn children(&self) -> &[Arc<Item>] {
        match &self.content {
            Content::Group { items } => items,
            _ => &[],
        }
    }
    /// The children that show: all of them, or just the active state of a multi-state object.
    pub fn shown_children(&self) -> impl Iterator<Item = &Arc<Item>> {
        let only = (!self.states.is_empty()).then_some(self.active_state);
        self.children().iter().enumerate().filter(move |(i, _)| only.is_none_or(|a| a == *i)).map(|(_, c)| c)
    }

    pub fn children_mut(&mut self) -> Option<&mut Vec<Arc<Item>>> {
        match &mut self.content {
            Content::Group { items } => Some(items),
            _ => None,
        }
    }

    /// Inner-space bounds of the path (the frame's "geometric bounds" before transform).
    pub fn inner_bounds(&self) -> Rect {
        self.path.bounds().unwrap_or(Rect::ZERO)
    }

    /// The box a gradient feather spans, in item space: the path's bounds, or a group's visible
    /// objects' (a group has no path of its own). Only the active states contribute, including in
    /// nested groups. Walk iteratively so deeply nested documents don't exhaust the call stack.
    pub fn feather_bounds(&self) -> Rect {
        if !self.is_group() {
            return self.inner_bounds();
        }
        let mut stack: Vec<_> = self.shown_children().map(|c| (c.as_ref(), c.xf)).collect();
        let mut bounds: Option<Rect> = None;
        while let Some((it, xf)) = stack.pop() {
            if it.hidden {
                continue;
            }
            if it.is_group() {
                stack.extend(it.shown_children().map(|c| (c.as_ref(), xf * c.xf)));
            } else {
                let b = xf.transform_rect_bbox(it.inner_bounds());
                bounds = Some(bounds.map_or(b, |have| have.union(b)));
            }
        }
        bounds.unwrap_or(Rect::ZERO)
    }

    /// Spread-space geometric bounds (axis-aligned box of the transformed path; groups: union).
    /// A group (its bounds are its children's). A frame holding pasted-into items is not.
    pub fn is_group(&self) -> bool {
        self.shape == Shape::Group && matches!(self.content, Content::Group { .. })
    }

    /// Does this object (or anything in it) involve transparency (opacity, a blend mode, effects,
    /// knockout)?
    pub fn involves_transparency(&self) -> bool {
        self.opacity < 1.0
            || self.blend != Default::default()
            || self.effects.any()
            || self.knockout
            || self.children().iter().any(|c| c.involves_transparency())
    }

    /// A frame with items pasted into it (Edit › Paste Into): drawn clipped to its path.
    pub fn has_nested_items(&self) -> bool {
        self.shape != Shape::Group && matches!(&self.content, Content::Group { items } if !items.is_empty())
    }

    pub fn bounds(&self) -> Rect {
        self.bounds_under(Affine::IDENTITY).unwrap_or(Rect::ZERO)
    }

    /// Bounds of the outline under `outer` (then this item's own transform). A group's are its
    /// children's outlines transformed by the full transform, which a rotated group's box of its
    /// children's boxes would overstate.
    fn bounds_under(&self, outer: Affine) -> Option<Rect> {
        let xf = outer * self.xf;
        if self.shape == Shape::Group
            && let Content::Group { items } = &self.content
        {
            return items.iter().filter_map(|c| c.bounds_under(xf)).reduce(|a, b| a.union(b));
        }
        self.path.transformed(xf).bounds()
    }

    /// Visible bounds including stroke weight (approximate for inside/outside alignment).
    pub fn visible_bounds(&self) -> Rect {
        let b = self.bounds();
        if self.stroke.is_none() {
            return b;
        }
        let w = match self.stroke.align {
            StrokeAlign::Center => self.stroke.weight / 2.0,
            StrokeAlign::Inside => 0.0,
            StrokeAlign::Outside => self.stroke.weight,
        };
        // Arrowheads on open paths.
        let w = if self.path.is_closed() { w } else { w.max(self.stroke.extent() - self.stroke.weight) };
        b.inflate(w, w)
    }

    /// The text frame's text area (inner space): inside the part of a visible stroke that lies
    /// within the frame, then the inset, on each edge.
    pub fn text_area(&self) -> Rect {
        let r = self.inner_bounds();
        let s = self.stroke_inside();
        let inset = self.text_frame().map(|t| t.options.inset).unwrap_or([0.0; 4]).map(|v| v + s);
        Rect::new(r.x0 + inset[1], r.y0 + inset[0], (r.x1 - inset[3]).max(r.x0 + inset[1]), (r.y1 - inset[2]).max(r.y0 + inset[0]))
    }

    /// How far a visible stroke reaches inside the path: half its weight centred, all of it inside,
    /// none outside.
    fn stroke_inside(&self) -> f64 {
        let w = self.stroke.weight;
        if self.stroke.is_none() || !w.is_finite() {
            return 0.0;
        }
        match self.stroke.align {
            StrokeAlign::Center => w / 2.0,
            StrokeAlign::Inside => w,
            StrokeAlign::Outside => 0.0,
        }
    }

    /// Spread-space centre.
    pub fn center(&self) -> Point {
        self.bounds().center()
    }

    /// The Layers-panel label: `<rectangle>`, the first words of a text frame, the placed file name…
    pub fn default_label(&self) -> &'static str {
        match (&self.content, self.shape) {
            // A frame with items pasted into it is named by its shape, as in InDesign.
            (Content::Group { .. }, Shape::Group) => "<group>",
            (Content::Text(_), _) => "<text frame>",
            (Content::Graphic(_), _) => "<image>",
            (_, Shape::Rectangle) => "<rectangle>",
            (_, Shape::Oval) => "<ellipse>",
            (_, Shape::Polygon) => "<polygon>",
            (_, Shape::GraphicLine) => "<line>",
            (_, Shape::Path) | (_, Shape::Group) => "<path>",
        }
    }

    /// Whether the item's own geometry (path, transform, a placed graphic's size and transform)
    /// is finite. JSON has no infinity or NaN, so a document holding one can't be saved.
    pub fn geometry_is_finite(&self) -> bool {
        let pt = |p: Point| p.x.is_finite() && p.y.is_finite();
        let affine = |a: &Affine| a.as_coeffs().iter().all(|c| c.is_finite());
        let path = self.path.subpaths.iter().flat_map(|s| &s.anchors).all(|a| pt(a.p) && pt(a.h_in) && pt(a.h_out));
        let graphic = match &self.content {
            Content::Graphic(g) => g.size.0.is_finite() && g.size.1.is_finite() && affine(&g.xf),
            _ => true,
        };
        path && affine(&self.xf) && graphic
    }

    /// Visit this item and descendants (pre-order).
    pub fn walk<'a>(&'a self, f: &mut impl FnMut(&'a Item)) {
        f(self);
        for c in self.children() {
            c.walk(f);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use designcraft_geom::shapes;

    #[test]
    fn graphics_without_a_clipping_path_load_and_save_as_before() {
        let g = Graphic {
            asset: AssetId(3),
            size: (40.0, 30.0),
            xf: Affine::translate((5.0, 5.0)),
            clip: None,
            auto_fit: Fitting::None,
            fit_align: 4,
            crop: [0.0; 4],
            wrap: Default::default(),
            fill: Default::default(),
        };
        let v = serde_json::to_value(&g).unwrap();
        assert!(v.get("clip").is_none(), "{v}");
        assert_eq!(serde_json::from_value::<Graphic>(v.clone()).unwrap(), g);
        // A clip written with only its type and path gets the default options.
        let mut v = v;
        v["clip"] = serde_json::json!({"kind": "alphaChannel", "path": {"subpaths": []}});
        let c = serde_json::from_value::<Graphic>(v).unwrap().clip.unwrap();
        assert_eq!(*c, ClippingPath::new(ClippingKind::AlphaChannel, PathData::default()));
    }

    #[test]
    fn bounds_and_text_area() {
        let mut it = Item::new(ItemId(1), LayerId(1), Shape::Rectangle, shapes::rectangle(Rect::new(0.0, 0.0, 100.0, 50.0)));
        it.xf = Affine::translate((10.0, 20.0));
        assert_eq!(it.bounds(), Rect::new(10.0, 20.0, 110.0, 70.0));
        it.content = Content::Text(TextFrame { story: StoryId(2), options: TextFrameOptions { inset: [5.0, 6.0, 7.0, 8.0], ..Default::default() } });
        assert_eq!(it.text_area(), Rect::new(6.0, 5.0, 92.0, 43.0));
        it.stroke = Stroke { weight: 4.0, ..Stroke::default() };
        assert_eq!(it.visible_bounds(), Rect::new(8.0, 18.0, 112.0, 72.0));
    }

    #[test]
    fn text_area_starts_inside_the_stroke() {
        let mut it = Item::new(ItemId(1), LayerId(1), Shape::Rectangle, shapes::rectangle(Rect::new(0.0, 0.0, 100.0, 50.0)));
        it.content = Content::Text(TextFrame { story: StoryId(2), options: TextFrameOptions { inset: [5.0, 6.0, 7.0, 8.0], ..Default::default() } });
        let unstroked = Rect::new(6.0, 5.0, 92.0, 43.0);
        let stroked = |align: StrokeAlign, swatch: &str| {
            let mut it = it.clone();
            it.stroke = Stroke { weight: 4.0, align, swatch: swatch.into(), ..Stroke::default() };
            it.text_area()
        };
        let black = designcraft_color::swatch::BLACK;
        // The part of the stroke inside the frame, on every edge, then the inset.
        assert_eq!(stroked(StrokeAlign::Center, black), Rect::new(8.0, 7.0, 90.0, 41.0));
        assert_eq!(stroked(StrokeAlign::Inside, black), Rect::new(10.0, 9.0, 88.0, 39.0));
        assert_eq!(stroked(StrokeAlign::Outside, black), unstroked);
        // A stroke without a colour doesn't count.
        assert_eq!(stroked(StrokeAlign::Inside, designcraft_color::swatch::NONE), unstroked);
        // Nor does a weight that isn't a number.
        it.stroke = Stroke { weight: f64::NAN, ..Stroke::default() };
        assert_eq!(it.text_area(), unstroked);
        // A stroke wider than the frame leaves no room for text.
        it.stroke = Stroke { weight: 200.0, align: StrokeAlign::Inside, ..Stroke::default() };
        let a = it.text_area();
        assert!(a.width() == 0.0 && a.height() == 0.0, "{a:?}");
    }

    #[test]
    fn group_bounds_union_children() {
        let a = Item::new(ItemId(1), LayerId(1), Shape::Rectangle, shapes::rectangle(Rect::new(0.0, 0.0, 10.0, 10.0)));
        let b = Item::new(ItemId(2), LayerId(1), Shape::Rectangle, shapes::rectangle(Rect::new(20.0, 20.0, 30.0, 40.0)));
        let mut g = Item::new(ItemId(3), LayerId(1), Shape::Group, PathData::default());
        g.content = Content::Group { items: vec![Arc::new(a), Arc::new(b)] };
        assert_eq!(g.bounds(), Rect::new(0.0, 0.0, 30.0, 40.0));
        let mut n = 0;
        g.walk(&mut |_| n += 1);
        assert_eq!(n, 3);
    }

    #[test]
    fn rotated_group_bounds_follow_the_children_outlines() {
        let rects = [Rect::new(0.0, 0.0, 120.0, 20.0), Rect::new(10.0, 40.0, 200.0, 70.0)];
        let child_xf = Affine::rotate(-20f64.to_radians());
        let items = rects
            .iter()
            .enumerate()
            .map(|(i, r)| {
                let mut c = Item::new(ItemId(i as u64 + 1), LayerId(1), Shape::Rectangle, shapes::rectangle(*r));
                c.xf = child_xf;
                Arc::new(c)
            })
            .collect();
        let mut g = Item::new(ItemId(9), LayerId(1), Shape::Group, PathData::default());
        g.content = Content::Group { items };
        g.xf = Affine::translate((50.0, 60.0)) * Affine::rotate(30f64.to_radians());
        let full = g.xf * child_xf;
        let corners = rects.iter().flat_map(|r| [(r.x0, r.y0), (r.x1, r.y0), (r.x1, r.y1), (r.x0, r.y1)]).map(|(x, y)| full * Point::new(x, y));
        let want = corners.fold(None::<Rect>, |acc, p| Some(acc.map_or(Rect::from_points(p, p), |r| r.union_pt(p)))).unwrap();
        let got = g.bounds();
        for (a, b) in [(got.x0, want.x0), (got.y0, want.y0), (got.x1, want.x1), (got.y1, want.y1)] {
            assert!((a - b).abs() < 1e-9, "{got:?} != {want:?}");
        }
        // The box of the children's rotated boxes is larger.
        let boxes = rects.iter().map(|r| g.xf.transform_rect_bbox(child_xf.transform_rect_bbox(*r))).reduce(|a, b| a.union(b)).unwrap();
        assert!(boxes.height() > got.height() + 1.0);
    }

    #[test]
    fn feather_bounds_follow_visible_states_in_nested_groups() {
        let a = Item::new(ItemId(1), LayerId(1), Shape::Rectangle, shapes::rectangle(Rect::new(10.0, 20.0, 30.0, 40.0)));
        let mut b = a.clone();
        b.id = ItemId(2);
        b.xf = Affine::translate((500.0, 500.0));
        let mut state = Item::new(ItemId(3), LayerId(1), Shape::Group, PathData::default());
        state.content = Content::Group { items: vec![Arc::new(a), Arc::new(b)] };
        state.states = vec!["Visible".into(), "Inactive".into()];
        assert_eq!(state.feather_bounds(), Rect::new(10.0, 20.0, 30.0, 40.0));
        state.active_state = 1;
        assert_eq!(state.feather_bounds(), Rect::new(510.0, 520.0, 530.0, 540.0));
        state.active_state = 0;
        state.xf = Affine::translate((5.0, 10.0)) * Affine::scale(2.0);
        let mut hidden = state.clone();
        hidden.id = ItemId(4);
        hidden.hidden = true;
        hidden.xf = Affine::translate((-1000.0, -1000.0));
        let mut outer = Item::new(ItemId(5), LayerId(1), Shape::Group, PathData::default());
        outer.content = Content::Group { items: vec![Arc::new(state), Arc::new(hidden)] };
        assert_eq!(outer.feather_bounds(), Rect::new(25.0, 50.0, 65.0, 90.0));
    }

    #[test]
    fn serde_roundtrip() {
        let mut it = Item::new(ItemId(1), LayerId(1), Shape::Oval, shapes::ellipse(Rect::new(0.0, 0.0, 10.0, 10.0)));
        it.fill = Fill::swatch("[Black]");
        it.content = Content::Text(TextFrame { story: StoryId(9), options: TextFrameOptions::default() });
        let s = serde_json::to_string(&it).unwrap();
        let back: Item = serde_json::from_str(&s).unwrap();
        assert_eq!(it, back);
    }
}
