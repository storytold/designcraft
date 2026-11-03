use super::{BlendMode, Deserialize, Serialize, is_default};

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
    /// How the shadow blends with what's behind it (InDesign: Multiply). A document saved before
    /// effects had modes reads Normal, as they were drawn then.
    #[serde(default = "normal_blend")]
    pub mode: BlendMode,
}

/// The blend of an effect saved before effects had one.
fn normal_blend() -> BlendMode {
    BlendMode::Normal
}

impl Default for DropShadow {
    fn default() -> Self {
        DropShadow {
            on: false,
            mode: BlendMode::Multiply,
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
    /// How the shadow blends with the object (InDesign: Multiply; Normal when saved before).
    #[serde(default = "normal_blend")]
    pub mode: BlendMode,
}

impl Default for InnerShadow {
    fn default() -> Self {
        InnerShadow {
            on: false,
            mode: BlendMode::Multiply,
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
    /// How the glow blends with what's behind it (InDesign: Screen; Normal when saved before).
    #[serde(default = "normal_blend")]
    pub mode: BlendMode,
}

impl Default for OuterGlow {
    fn default() -> Self {
        // InDesign starts a glow screened in white.
        OuterGlow { on: false, color: designcraft_color::swatch::PAPER.into(), opacity: 0.75, size: 7.0, spread: 0.0, mode: BlendMode::Screen }
    }
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
