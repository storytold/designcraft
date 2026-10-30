use super::{Deserialize, Point, Rect, Serialize};

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
    /// Three lines of the same width.
    Triple,
    /// Dashes three weights long, gaps two weights (InDesign's Dashed (3 and 2)).
    #[serde(rename = "dashed3x2")]
    Dashed3x2,
    /// Dashes and gaps four weights long (Dashed (4 and 4)).
    #[serde(rename = "dashed4x4")]
    Dashed4x4,
    /// Hash ticks leaning back and forward.
    LeftSlantHash,
    RightSlantHash,
    /// A chain of hollow diamonds, tip to tip.
    WhiteDiamond,
    /// Dots closer together than Dotted's.
    JapaneseDots,
    /// One of InDesign's hidden stroke styles: what a document stroke style of that name resolves
    /// to ([`HiddenStroke::of`]); never stored.
    Hidden {
        style: HiddenStroke,
    },
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
            StrokeType::Triple => "Triple",
            StrokeType::Dashed3x2 => "Dashed (3 and 2)",
            StrokeType::Dashed4x4 => "Dashed (4 and 4)",
            StrokeType::LeftSlantHash => "Left Slant Hash",
            StrokeType::RightSlantHash => "Right Slant Hash",
            StrokeType::WhiteDiamond => "White Diamond",
            StrokeType::JapaneseDots => "Japanese Dots",
            StrokeType::Hidden { .. } => "Custom",
            StrokeType::Stripes { .. } => "Stripes",
            StrokeType::Style { .. } => "Custom",
        }
    }

    /// The built-in types (serialized `kind`s) in the order of InDesign's Type menu, plain Dashed
    /// last, after its separator.
    pub const BUILT_IN: [&'static str; 18] = [
        "solid",
        "thickThick",
        "thickThin",
        "thickThinThick",
        "thinThick",
        "thinThickThin",
        "thinThin",
        "triple",
        "dashed3x2",
        "dashed4x4",
        "leftSlantHash",
        "rightSlantHash",
        "hashed",
        "dotted",
        "wavy",
        "whiteDiamond",
        "japaneseDots",
        "dashed",
    ];

    /// The dash pattern of the dashed and dotted types for a stroke of `weight` (points), and
    /// whether its dashes are round dots (very short dashes with round caps: a zero-length dash
    /// isn't drawn by every stroker). Dotted spaces its dots three weights apart and Japanese Dots
    /// two, as InDesign draws them.
    pub fn dashes(&self, weight: f64) -> Option<(Vec<f64>, bool)> {
        let dot = |spacing: f64| (vec![weight * 1e-3, weight * (spacing - 1e-3)], true);
        Some(match self {
            StrokeType::Dashed { pattern } => (pattern.clone(), false),
            StrokeType::Dashed3x2 => (vec![3.0 * weight, 2.0 * weight], false),
            StrokeType::Dashed4x4 => (vec![4.0 * weight, 4.0 * weight], false),
            StrokeType::Dotted => dot(3.0),
            StrokeType::JapaneseDots => dot(2.0),
            _ => return None,
        })
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

    /// Whether this type draws round dots along a path at most `length` points long: a dotted
    /// type whose pattern expands there (otherwise it draws solid, see [`Self::expandable_dashes`]).
    pub fn draws_dots(&self, weight: f64, length: f64) -> bool {
        self.dashes(weight).is_some_and(|(pattern, dots)| dots && Self::expandable_dashes(&pattern, length))
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
            StrokeType::Triple => vec![(0.0, 0.2), (0.4, 0.2), (0.8, 0.2)],
            StrokeType::Stripes { bands } => bands.clone(),
            _ => return None,
        })
    }

    /// The parts of a hidden stroke style and their colours (`None`: the stroke's colour); `None`
    /// for every other type.
    pub fn painted(&self, bp: &designcraft_geom::BezPath, weight: f64, tol: f64) -> Option<Vec<designcraft_geom::stroke_style::Part>> {
        use designcraft_geom::stroke_style as ss;
        let StrokeType::Hidden { style } = self else { return None };
        Some(match style {
            HiddenStroke::Rainbow => ss::colour_bands(bp, weight, &ss::RAINBOW, tol),
            HiddenStroke::Rasta => ss::colour_bands(bp, weight, &ss::RASTA, tol),
            HiddenStroke::Feet => ss::feet(bp, weight),
            HiddenStroke::Woof => ss::woof(bp, weight),
            HiddenStroke::Happy => ss::happy(bp, weight, tol),
            HiddenStroke::Lights => ss::lights(bp, weight, tol),
            HiddenStroke::Bounce => ss::bounce(bp, weight),
        })
    }

    /// The filled area for types drawn as fills (stripes, wavy, hash, and a hidden style's parts
    /// together); `None` for solid, dashed and dotted strokes (and unresolved named styles).
    pub fn outline(&self, bp: &designcraft_geom::BezPath, weight: f64, tol: f64) -> Option<designcraft_geom::BezPath> {
        use designcraft_geom::stroke_style as ss;
        if let Some(parts) = self.painted(bp, weight, tol) {
            let mut all = designcraft_geom::BezPath::new();
            for el in parts.iter().flat_map(|(p, _)| p.elements()) {
                all.push(*el);
            }
            return Some(all);
        }
        match self {
            StrokeType::Wavy => Some(ss::wavy(bp, weight, tol)),
            StrokeType::Hashed => Some(ss::hashed(bp, weight, tol)),
            StrokeType::LeftSlantHash => Some(ss::slant_hash(bp, weight, -1.0, tol)),
            StrokeType::RightSlantHash => Some(ss::slant_hash(bp, weight, 1.0, tol)),
            StrokeType::WhiteDiamond => Some(ss::diamonds(bp, weight, tol)),
            t => t.bands().map(|b| ss::stripes(bp, weight, &b, tol)),
        }
    }
}

/// InDesign's hidden stroke styles: a document stroke style named exactly one of these (a Dash
/// style, or a Stripe style for Rainbow and Rasta) draws a set pattern instead of its own.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum HiddenStroke {
    Feet,
    Happy,
    Lights,
    Woof,
    Bounce,
    Rainbow,
    Rasta,
}

impl HiddenStroke {
    /// The hidden style a stroke style named `name` of `kind` is, if any. The name is matched as
    /// InDesign matches it, capitals included.
    pub fn of(name: &str, kind: &StrokeType) -> Option<HiddenStroke> {
        Some(match (name, kind) {
            ("Feet", StrokeType::Dashed { .. }) => HiddenStroke::Feet,
            ("Happy", StrokeType::Dashed { .. }) => HiddenStroke::Happy,
            ("Lights", StrokeType::Dashed { .. }) => HiddenStroke::Lights,
            ("Woof", StrokeType::Dashed { .. }) => HiddenStroke::Woof,
            ("Bounce", StrokeType::Dashed { .. }) => HiddenStroke::Bounce,
            ("Rainbow", StrokeType::Stripes { .. }) => HiddenStroke::Rainbow,
            ("Rasta", StrokeType::Stripes { .. }) => HiddenStroke::Rasta,
            _ => return None,
        })
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
