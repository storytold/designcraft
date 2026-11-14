//! A page as a slide: items in stacking order as shapes, pictures, groups and text boxes.

use std::collections::HashMap;

use designcraft_color::swatch::resolve_gradient;
use designcraft_color::{BlendMode, GradientKind};
use designcraft_compose::Cache;
use designcraft_doc::{Arrowhead, AssetId, Cap, Content, Document, Graphic, Item, Join, Shape, Stroke, StrokeAlign, StrokeType};
use designcraft_geom::kurbo::{PathEl, Shape as _};
use designcraft_geom::{Affine, BezPath, Rect, Vec2, corners};

use crate::fonts::Fonts;
use crate::model::{Media, Slide};
use crate::{PptxError, PptxOptions, Result};

/// Preserve fractional DesignCraft opacity in DrawingML instead of rounding it through 8-bit
/// alpha and back to PresentationML's 100,000-step alpha value.
pub(crate) fn color_with_opacity(color: designcraft_color::Color, opacity: f32) -> craft_pptx::ColorRef {
    let [r, g, b, _] = color.to_rgba8(1.0);
    let color = craft_pptx::ColorRef::rgb(craft_pptx::Rgba::rgb(r, g, b));
    let alpha = if opacity.is_finite() { (f64::from(opacity.clamp(0.0, 1.0)) * 100_000.0).round() as i32 } else { 0 };
    if alpha == 100_000 { color } else { color.with(craft_pptx::ColorTransform::Alpha(alpha)) }
}

/// Objects PowerPoint can't show as editable objects, which a caller should rasterise before
/// exporting: placed PDF and EPS graphics, type on a path, and effects without a DrawingML
/// counterpart.
pub fn needs_raster(doc: &Document, it: &Item) -> bool {
    let fx = &it.effects;
    let effects = fx.inner_glow.on || fx.bevel.on || fx.satin.on || fx.directional_feather.on || fx.gradient_feather.on;
    let placed = |g: &Graphic| {
        doc.assets.get(&g.asset).is_some_and(|a| {
            let m = a.mime.as_str();
            m == "application/pdf" || m.contains("postscript") || m.contains("illustrator")
        })
    };
    let on_path = it.text_frame().is_some_and(|t| t.options.path.is_some());
    effects || on_path || it.graphic().is_some_and(placed) || it.children().iter().any(|c| needs_raster(doc, c))
}

/// A transform box in slide coordinates, expressed in points.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct XfrmBox {
    pub rect: Rect,
    pub rot: f64,
    pub flip_h: bool,
    pub flip_v: bool,
}

impl XfrmBox {
    pub fn plain(rect: Rect) -> Self {
        XfrmBox { rect, rot: 0.0, flip_h: false, flip_v: false }
    }
}

/// An affine split into rotation, axis scales and shear: `m = translate · rotate(rot) ·
/// [sx, shear·sy; 0, sy]`.
#[derive(Clone, Copy, Debug)]
pub struct Decomposed {
    pub rot: f64,
    pub sx: f64,
    pub sy: f64,
    pub shear: f64,
}

pub fn decompose(m: Affine) -> Decomposed {
    let [a, b, c, d, _, _] = m.as_coeffs();
    let sx = a.hypot(b);
    let rot = b.atan2(a);
    let (cos, sin) = (rot.cos(), rot.sin());
    let c2 = cos * c + sin * d;
    let d2 = -sin * c + cos * d;
    let shear = if d2.abs() > 1e-12 { c2 / d2 } else { 0.0 };
    Decomposed { rot: rot.to_degrees(), sx, sy: d2, shear }
}

/// The box `inner` occupies under `m` when `m` has no shear: rotated about its centre, flipped
/// vertically for a negative determinant. `None` for sheared transforms.
pub fn box_of(inner: Rect, m: Affine) -> Option<(XfrmBox, Decomposed)> {
    let dm = decompose(m);
    if dm.shear.abs() > 1e-6 || !dm.sx.is_finite() || !dm.sy.is_finite() {
        return None;
    }
    let c = m * inner.center();
    let (w, h) = (inner.width() * dm.sx.abs(), inner.height() * dm.sy.abs());
    let rect = Rect::new(c.x - w / 2.0, c.y - h / 2.0, c.x + w / 2.0, c.y + h / 2.0);
    Some((XfrmBox { rect, rot: dm.rot, flip_h: false, flip_v: dm.sy < 0.0 }, dm))
}

/// Shape geometry: a preset, or a custom path in the box's own (unrotated) space.
pub struct Geometry {
    pub xfrm: XfrmBox,
    pub model: craft_pptx::Geom,
}

/// Is `bp` exactly the rectangle `r` (any start corner and direction)?
fn is_rect(bp: &BezPath, r: Rect) -> bool {
    let mut pts = Vec::new();
    for el in bp.elements() {
        match *el {
            PathEl::MoveTo(p) | PathEl::LineTo(p) => pts.push(p),
            PathEl::ClosePath => {}
            _ => return false,
        }
    }
    if pts.len() == 5 && pts.first().zip(pts.last()).is_some_and(|(a, b)| (*a - *b).hypot() < 1e-6) {
        pts.pop();
    }
    pts.len() == 4
        && pts.iter().all(|p| ((p.x - r.x0).abs() < 1e-6 || (p.x - r.x1).abs() < 1e-6) && ((p.y - r.y0).abs() < 1e-6 || (p.y - r.y1).abs() < 1e-6))
}

/// The geometry of `it` (its path, corner options applied) under `m` (inner → slide).
pub fn geometry(it: &Item, m: Affine) -> Geometry {
    let bp = if it.corners.is_none() { it.path.to_bezpath() } else { corners::apply(&it.path, &it.corners) };
    let closed = it.path.is_closed();
    let inner = bp.bounding_box();
    if let Some((xfrm, dm)) = box_of(inner, m) {
        if it.corners.is_none() && is_rect(&bp, inner) {
            return Geometry { xfrm, model: craft_pptx::Geom::preset("rect") };
        }
        if it.shape == Shape::Oval && it.corners.is_none() && closed {
            return Geometry { xfrm, model: craft_pptx::Geom::preset("ellipse") };
        }
        // The path in the box's own space: scaled (not flipped: the box flips), from its corner.
        let (kx, ky) = (dm.sx.abs(), dm.sy.abs());
        let mut local = bp.clone();
        local.apply_affine(Affine::scale_non_uniform(kx, ky) * Affine::translate(-inner.origin().to_vec2()));
        return Geometry {
            xfrm,
            model: craft_pptx::Geom::Custom { paths: vec![craft_pptx::CustomPath::from_bez(&local, xfrm.rect.width(), xfrm.rect.height(), closed)] },
        };
    }
    // Sheared: the transformed path in slide space.
    let mut pb = bp;
    pb.apply_affine(m);
    let b = pb.bounding_box();
    pb.apply_affine(Affine::translate(-b.origin().to_vec2()));
    Geometry {
        xfrm: XfrmBox::plain(b),
        model: craft_pptx::Geom::Custom { paths: vec![craft_pptx::CustomPath::from_bez(&pb, b.width(), b.height(), closed)] },
    }
}

pub struct Exporter<'a> {
    pub doc: &'a Document,
    pub cache: &'a Cache,
    pub opts: &'a PptxOptions,
    pub warnings: Vec<String>,
    pub media: Vec<Media>,
    /// Model shapes in stacking order.
    pub native_shapes: Vec<craft_pptx::Shape>,
    pub native_media: Vec<(u32, String, Option<String>)>,
    /// Asset → (picture media name, SVG media name).
    media_of: HashMap<AssetId, Option<(String, Option<String>)>>,
    pub fonts: Fonts,
    next_id: u32,
    /// Spread space → slide space.
    pub to_slide: Affine,
    /// The page's bleed area in spread space (items outside it are left out).
    clip: Rect,
    /// The page parent items are composed for (page numbers, running heads).
    pub page: Option<usize>,
}

impl<'a> Exporter<'a> {
    pub fn new(doc: &'a Document, cache: &'a Cache, opts: &'a PptxOptions) -> Self {
        Exporter {
            doc,
            cache,
            opts,
            warnings: Vec::new(),
            media: Vec::new(),
            native_shapes: Vec::new(),
            native_media: Vec::new(),
            media_of: HashMap::new(),
            fonts: Fonts::default(),
            next_id: 2,
            to_slide: Affine::IDENTITY,
            clip: Rect::ZERO,
            page: None,
        }
    }

    pub fn warn(&mut self, w: impl Into<String>) {
        let w = w.into();
        if !self.warnings.contains(&w) {
            self.warnings.push(w);
        }
    }

    pub fn id(&mut self) -> u32 {
        let i = self.next_id;
        self.next_id = self.next_id.saturating_add(1);
        i
    }

    pub fn slide(&mut self, abs: usize) -> Result<Slide> {
        let doc = self.doc;
        let (si, pi) = doc.page_loc(abs).ok_or(PptxError::BadPage(abs + 1))?;
        let sp = doc.spreads.get(si).ok_or(PptxError::BadPage(abs + 1))?;
        let page = sp.pages.get(pi).ok_or(PptxError::BadPage(abs + 1))?;
        let bounds = page.bounds();
        self.native_shapes.clear();
        self.native_media.clear();
        self.next_id = 2;
        self.to_slide = Affine::translate(-bounds.origin().to_vec2());
        let b = doc.settings.bleed;
        self.clip = Rect::new(bounds.x0 - b[2].max(b[3]), bounds.y0 - b[0], bounds.x1 + b[2].max(b[3]), bounds.y1 + b[1]);
        let page_name = doc.page_name(abs);
        // What the canvas hides: this page's overrides and those of the parent pages behind it.
        let overridden = doc.page_loc(abs).map(|(si, pi)| doc.overridden_parent_items(designcraft_doc::SpreadRef::Doc(si), pi)).unwrap_or_default();
        for layer in doc.layers.iter().rev() {
            if !layer.visible || !layer.printable {
                continue;
            }
            if page.show_parent_items
                && let Some((ppi, ppage)) = doc.parent_page_for(abs)
                && let Some(parent) = doc.parents.get(ppi)
                && let Some(pp) = parent.pages.get(ppage)
            {
                let dx = page.x - pp.x;
                for it in &parent.items {
                    if it.layer != layer.id || overridden.contains(&it.id) {
                        continue;
                    }
                    if parent.pages.len() > 1 && parent.page_at_x(it.bounds().center().x) != Some(ppage) {
                        continue;
                    }
                    // Parent items compose per page (page numbers, running heads).
                    self.page = Some(abs);
                    self.item(it, Affine::translate((dx, 0.0)), 1.0);
                    self.page = None;
                }
            }
            for it in &sp.items {
                if it.layer == layer.id {
                    self.item(it, Affine::IDENTITY, 1.0);
                }
            }
        }
        Ok(Slide {
            size: (bounds.width(), bounds.height()),
            name: page_name,
            shapes: std::mem::take(&mut self.native_shapes),
            media: std::mem::take(&mut self.native_media),
        })
    }

    /// One item under `parent` (inner-of-parent → spread), with an inherited opacity.
    pub fn item(&mut self, it: &Item, parent: Affine, opacity: f32) {
        if it.hidden || it.nonprinting {
            return;
        }
        let reach = it.stroke.extent()
            + 2.0
            + if it.effects.drop_shadow.on { it.effects.drop_shadow.distance.abs() + it.effects.drop_shadow.size.abs() } else { 0.0 };
        let vb = parent.transform_rect_bbox(it.bounds()).inflate(reach, reach);
        if vb.x1 < self.clip.x0 || vb.x0 > self.clip.x1 || vb.y1 < self.clip.y0 || vb.y0 > self.clip.y1 {
            return;
        }
        if it.blend != BlendMode::Normal {
            self.warn("blend modes have no PowerPoint equivalent; those objects are drawn normally");
        }
        if needs_raster(self.doc, it) && !it.is_group() {
            self.warn("some effects (inner glow, bevel and emboss, satin, gradient and directional feather), type on a path and placed PDF/EPS graphics can't be exported as PowerPoint objects; rasterise them (Object Export Options) or they are approximated");
        }
        let xf = parent * it.xf;
        let opacity = opacity * it.opacity.clamp(0.0, 1.0);
        if it.has_nested_items() {
            // Paste Into: the frame, then its contents (PowerPoint can't clip a group to a shape).
            self.shape(it, xf, opacity);
            let fb = xf.transform_rect_bbox(it.inner_bounds());
            for c in it.children() {
                let cb = xf.transform_rect_bbox(c.bounds());
                if cb.x0 < fb.x0 - 0.5 || cb.y0 < fb.y0 - 0.5 || cb.x1 > fb.x1 + 0.5 || cb.y1 > fb.y1 + 0.5 {
                    self.warn("contents pasted into a frame are not clipped to it");
                }
                self.item(c, xf, opacity);
            }
            return;
        }
        if !it.children().is_empty() {
            self.group(it, xf, opacity);
            return;
        }
        match &it.content {
            Content::Graphic(g) => self.picture(it, g, xf, opacity),
            Content::Text(t) => {
                let t = t.clone();
                crate::text::frame(self, it, &t, xf, opacity);
            }
            _ => self.shape(it, xf, opacity),
        }
    }

    fn group(&mut self, it: &Item, xf: Affine, opacity: f32) {
        let native_start = self.native_shapes.len();
        let id = self.id();
        for c in it.shown_children() {
            self.item(c, xf, opacity);
        }
        let children: Vec<_> = self.native_shapes.drain(native_start..).collect();
        if children.is_empty() {
            return;
        }
        // Children are in slide space: the group's child space is the slide's.
        let inner_bounds = it.shown_children().map(|c| c.bounds()).reduce(|a, b| a.union(b)).unwrap_or(Rect::ZERO);
        let b = (self.to_slide * xf).transform_rect_bbox(inner_bounds);
        self.native_shapes.push(craft_pptx::Shape {
            id: craft_pptx::ShapeId(id),
            name: name_of(it, id),
            xfrm: Some(craft_pptx::Xfrm::new(b.x0, b.y0, b.width(), b.height())),
            kind: craft_pptx::ShapeKind::Group { children, child: craft_pptx::Xfrm::new(b.x0, b.y0, b.width(), b.height()) },
            descr: it.alt_text.clone(),
            ..Default::default()
        });
    }

    /// Add the item's geometry, fill, stroke and effects to the shared presentation model.
    pub fn shape(&mut self, it: &Item, xf: Affine, opacity: f32) {
        self.shape_inner(it, xf, opacity, None);
    }

    /// Add a text-bearing shape to the shared presentation model.
    pub fn shape_with_text_model(&mut self, it: &Item, xf: Affine, opacity: f32, model: craft_pptx::TextBody) {
        self.shape_inner(it, xf, opacity, Some(model));
    }

    fn shape_inner(&mut self, it: &Item, xf: Affine, opacity: f32, model_text: Option<craft_pptx::TextBody>) {
        let m = self.to_slide * xf;
        let g = geometry(it, m);
        if model_text.is_none() && it.fill.is_none() && it.stroke.is_none() && it.effects == designcraft_doc::Effects::default() {
            return;
        }
        let id = self.id();
        let Some(mut shape) = self.native_shape(it, &g, id, opacity) else { return };
        shape.text_box = it.fill.is_none() && it.stroke.is_none();
        shape.text = model_text;
        self.native_shapes.push(shape);
    }

    /// A shape mapped into the shared presentation model.
    fn native_shape(&mut self, it: &Item, geometry: &Geometry, id: u32, opacity: f32) -> Option<craft_pptx::Shape> {
        let fill = self.model_fill(&it.fill, it.path.is_closed(), opacity);
        let line = self.model_line(&it.stroke, opacity);
        let effects = self.model_effects(it, opacity);
        let xfrm = craft_pptx::Xfrm {
            x: geometry.xfrm.rect.x0,
            y: geometry.xfrm.rect.y0,
            w: geometry.xfrm.rect.width(),
            h: geometry.xfrm.rect.height(),
            rot: geometry.xfrm.rot,
            flip_h: geometry.xfrm.flip_h,
            flip_v: geometry.xfrm.flip_v,
        };
        Some(craft_pptx::Shape {
            id: craft_pptx::ShapeId(id),
            name: name_of(it, id),
            xfrm: Some(xfrm),
            geom: geometry.model.clone(),
            fill: Some(fill),
            line: Some(line),
            effects: (effects != craft_pptx::Effects::default()).then_some(effects),
            descr: it.alt_text.clone(),
            ..Default::default()
        })
    }

    /// An sRGB color in the shared PresentationML model, with DesignCraft tint and opacity applied.
    pub(crate) fn model_color(&self, swatch: &str, tint: f32, alpha: f32) -> Option<craft_pptx::ColorRef> {
        if let Some(gradient) = resolve_gradient(&self.doc.swatches, swatch) {
            let (_, color, gradient_alpha) = gradient.expanded_stops().into_iter().next()?;
            return Some(color_with_opacity(color, gradient_alpha * alpha));
        }
        Some(color_with_opacity(self.doc.resolve_color(swatch, tint)?, alpha))
    }

    fn model_fill(&mut self, fill: &designcraft_doc::Fill, closed: bool, opacity: f32) -> craft_pptx::Fill {
        if fill.is_none() || !closed {
            return craft_pptx::Fill::None;
        }
        if let Some(gradient) = resolve_gradient(&self.doc.swatches, &fill.swatch) {
            if fill.gradient_vector.is_some() {
                self.warn("gradients set with the Gradient tool use their angle only; PowerPoint stretches them over the whole shape");
            }
            let stops = gradient
                .expanded_stops()
                .into_iter()
                .map(|(offset, color, alpha)| craft_pptx::style::GradientStop {
                    pos: f64::from(offset),
                    color: color_with_opacity(color, alpha * opacity),
                })
                .collect();
            let shape = match gradient.kind {
                GradientKind::Radial => craft_pptx::style::GradientShape::Path { path: "circle".into(), focus: [0.5; 4] },
                GradientKind::Linear | GradientKind::Freeform => {
                    let angle = match fill.gradient_vector {
                        Some([x0, y0, x1, y1]) => (y1 - y0).atan2(x1 - x0).to_degrees(),
                        None => -fill.gradient_angle.unwrap_or(0.0),
                    };
                    craft_pptx::style::GradientShape::Linear { angle, scaled: false }
                }
            };
            return craft_pptx::Fill::Gradient(craft_pptx::style::Gradient {
                stops,
                shape: Some(shape),
                rotate_with_shape: Some(true),
                flip: None,
                tile: [0.0; 4],
            });
        }
        self.model_color(&fill.swatch, fill.tint, opacity).map_or(craft_pptx::Fill::None, craft_pptx::Fill::solid)
    }

    fn model_line(&mut self, stroke: &Stroke, opacity: f32) -> craft_pptx::Line {
        if stroke.is_none() || stroke.weight <= 0.0 {
            return craft_pptx::Line::none();
        }
        if matches!(stroke.kind, StrokeType::Wavy | StrokeType::Hashed | StrokeType::Stripes { .. } | StrokeType::Style { .. }) {
            self.warn("wavy, hashed, striped and custom stroke styles are exported as solid lines");
        }
        if stroke.align == StrokeAlign::Outside {
            self.warn("strokes aligned outside are centred on the path (PowerPoint has centre and inside only)");
        }
        if stroke.start == Arrowhead::Bar || stroke.end == Arrowhead::Bar {
            self.warn("bar arrowheads have no PowerPoint equivalent and are left out");
        }
        let mut line = craft_pptx::Line {
            fill: Some(self.model_fill(&stroke.as_fill(), true, opacity)),
            width: Some(stroke.weight),
            cap: Some(match stroke.cap {
                Cap::Butt => craft_pptx::style::LineCap::Flat,
                Cap::Round => craft_pptx::style::LineCap::Round,
                Cap::Projecting => craft_pptx::style::LineCap::Square,
            }),
            join: Some(match stroke.join {
                Join::Miter => craft_pptx::style::LineJoin::Miter,
                Join::Round => craft_pptx::style::LineJoin::Round,
                Join::Bevel => craft_pptx::style::LineJoin::Bevel,
            }),
            miter_limit: (stroke.join == Join::Miter).then_some((stroke.miter_limit.max(1.0) * 100_000.0).round() as u32),
            align: Some(match stroke.align {
                StrokeAlign::Inside => craft_pptx::style::LineAlign::Inside,
                StrokeAlign::Center | StrokeAlign::Outside => craft_pptx::style::LineAlign::Center,
            }),
            compound: Some(match stroke.kind {
                StrokeType::ThickThin => craft_pptx::style::Compound::ThickThin,
                StrokeType::ThinThick => craft_pptx::style::Compound::ThinThick,
                StrokeType::ThinThin | StrokeType::ThickThick => craft_pptx::style::Compound::Double,
                StrokeType::ThinThickThin | StrokeType::ThickThinThick => craft_pptx::style::Compound::Triple,
                _ => craft_pptx::style::Compound::Single,
            }),
            ..Default::default()
        };
        match &stroke.kind {
            StrokeType::Dashed { pattern } if pattern.iter().any(|length| *length > 0.0) => {
                let mut pattern = pattern.iter().map(|length| length.max(0.0)).collect::<Vec<_>>();
                if pattern.len() % 2 == 1 {
                    pattern.extend(pattern.clone());
                }
                let pairs = pattern
                    .chunks(2)
                    .filter_map(|pair| pair.first().zip(pair.get(1)))
                    .map(|(dash, gap)| (*dash / stroke.weight, *gap / stroke.weight))
                    .collect();
                line.dash = Some(craft_pptx::style::Dash::Custom { pattern: pairs });
            }
            StrokeType::Dotted => line.dash = Some(craft_pptx::style::Dash::Preset { name: "sysDot".into() }),
            _ => {}
        }
        let end = |arrow: Arrowhead| {
            match arrow {
                Arrowhead::None | Arrowhead::Bar => None,
                Arrowhead::Simple | Arrowhead::SimpleWide => Some("arrow"),
                Arrowhead::Triangle | Arrowhead::TriangleWide | Arrowhead::Curved => Some("triangle"),
                Arrowhead::Barbed => Some("stealth"),
                Arrowhead::Circle | Arrowhead::CircleSolid => Some("oval"),
                Arrowhead::Square | Arrowhead::SquareSolid => Some("diamond"),
            }
            .map(|kind| craft_pptx::style::LineEnd { kind: kind.into(), w: "lg".into(), len: "lg".into() })
        };
        line.head = end(stroke.start);
        line.tail = end(stroke.end);
        line
    }

    fn model_effects(&self, it: &Item, opacity: f32) -> craft_pptx::Effects {
        let fx = &it.effects;
        let shadow = |color: &str, alpha: f32, angle: f64, global: bool, distance: f64, blur: f64, inner: bool| {
            self.model_color(color, 1.0, alpha * opacity).map(|color| {
                let light = self.doc.light_angle(angle, global).to_radians();
                let offset = Vec2::new(-light.cos(), light.sin());
                craft_pptx::style::Shadow {
                    color,
                    blur,
                    dist: distance.abs(),
                    dir: offset.y.atan2(offset.x).to_degrees(),
                    inner,
                    sx: 1.0,
                    sy: 1.0,
                    kx: 0.0,
                    ky: 0.0,
                    align: String::new(),
                    rotate_with_shape: true,
                }
            })
        };
        let outer_shadow = fx
            .drop_shadow
            .on
            .then(|| {
                shadow(
                    &fx.drop_shadow.color,
                    fx.drop_shadow.opacity,
                    fx.drop_shadow.angle,
                    fx.drop_shadow.global_light,
                    fx.drop_shadow.distance,
                    fx.drop_shadow.size,
                    false,
                )
            })
            .flatten();
        let inner_shadow = fx
            .inner_shadow
            .on
            .then(|| {
                shadow(
                    &fx.inner_shadow.color,
                    fx.inner_shadow.opacity,
                    fx.inner_shadow.angle,
                    fx.inner_shadow.global_light,
                    fx.inner_shadow.distance,
                    fx.inner_shadow.size,
                    true,
                )
            })
            .flatten();
        let glow = if fx.outer_glow.on {
            self.model_color(&fx.outer_glow.color, 1.0, fx.outer_glow.opacity * opacity)
                .map(|color| craft_pptx::style::Glow { color, radius: fx.outer_glow.size })
        } else {
            None
        };
        craft_pptx::Effects { outer_shadow, inner_shadow, glow, soft_edge: (fx.feather > 0.0).then_some(fx.feather), ..Default::default() }
    }

    /// The media for a placed graphic: PNG, JPEG and GIF as they are, SVG with a PNG fallback,
    /// other rasters converted to PNG.
    fn media_for(&mut self, asset: AssetId) -> Option<(String, Option<String>)> {
        if let Some(m) = self.media_of.get(&asset) {
            return m.clone();
        }
        let r = self.convert(asset);
        self.media_of.insert(asset, r.clone());
        r
    }

    fn convert(&mut self, asset: AssetId) -> Option<(String, Option<String>)> {
        let a = self.doc.assets.get(&asset)?.clone();
        let data: &[u8] = &a.data;
        if data.is_empty() {
            self.warn(format!("`{}` is missing (linked file not found); left out", a.name));
            return None;
        }
        let n = self.media.len() + 1;
        let mime = designcraft_images::mime(data);
        let ext = match mime {
            "image/png" => Some("png"),
            "image/jpeg" => Some("jpeg"),
            "image/gif" => Some("gif"),
            _ => None,
        };
        if let Some(ext) = ext {
            let name = format!("image{n}.{ext}");
            self.media.push(Media { name: name.clone(), bytes: data.to_vec() });
            return Some((name, None));
        }
        if mime == "image/svg+xml" {
            let png = designcraft_images::render_svg(data, 2048).and_then(|(px, w, h)| encode_png(unpremultiply(px), w, h));
            let Some(png) = png else {
                self.warn(format!("`{}` could not be read; left out", a.name));
                return None;
            };
            let name = format!("image{n}.png");
            let svg = format!("image{}.svg", n + 1);
            self.media.push(Media { name: name.clone(), bytes: png });
            self.media.push(Media { name: svg.clone(), bytes: data.to_vec() });
            return Some((name, Some(svg)));
        }
        if mime == "application/pdf" || mime == "application/octet-stream" {
            self.warn(format!(
                "`{}` (PDF, EPS or an unknown format) can't be placed in PowerPoint; rasterise it (Object Export Options) to include it",
                a.name
            ));
            return None;
        }
        let Some(img) = designcraft_images::decode_rgba(data) else {
            self.warn(format!("`{}` could not be read; left out", a.name));
            return None;
        };
        let (w, h) = img.dimensions();
        let png = encode_png(img.into_raw(), w, h)?;
        let name = format!("image{n}.png");
        self.media.push(Media { name: name.clone(), bytes: png });
        Some((name, None))
    }

    fn picture(&mut self, it: &Item, g: &Graphic, xf: Affine, opacity: f32) {
        let g = if it.media.is_some() { it.drawn_graphic(&self.doc.assets).unwrap_or_else(|| g.clone()) } else { g.clone() };
        let Some((name, svg)) = self.media_for(g.asset) else {
            self.shape(it, xf, opacity);
            return;
        };
        let m = self.to_slide * xf;
        let [a, b, c, d, _, _] = g.xf.as_coeffs();
        let frame = it.inner_bounds();
        let img = g.xf.transform_rect_bbox(Rect::new(0.0, 0.0, g.size.0, g.size.1));
        let axis = b.abs() < 1e-9 && c.abs() < 1e-9 && a > 0.0 && d > 0.0;
        let (geo, crop_values) = if axis && img.width() > 0.0 && img.height() > 0.0 {
            // The frame's geometry with the image cropped (or padded) to the frame's box.
            let crop_values = [
                (frame.x0 - img.x0) / img.width(),
                (frame.y0 - img.y0) / img.height(),
                (img.x1 - frame.x1) / img.width(),
                (img.y1 - frame.y1) / img.height(),
            ];
            (geometry(it, m), crop_values)
        } else {
            // Rotated, skewed or flipped inside its frame: the image's own box, uncropped, and
            // the frame drawn on its own.
            self.warn("images rotated, skewed or flipped inside their frames are not cropped to the frame");
            self.shape(it, xf, opacity);
            let gi = Rect::new(0.0, 0.0, g.size.0, g.size.1);
            let gm = m * g.xf;
            let xfrm = box_of(gi, gm).map(|(x, _)| x).unwrap_or_else(|| XfrmBox::plain(gm.transform_rect_bbox(gi)));
            (Geometry { xfrm, model: craft_pptx::Geom::preset("rect") }, [0.0; 4])
        };
        let id = self.id();
        let mut picture_fill = craft_pptx::style::PictureFill { media: craft_pptx::MediaId(0), crop: crop_values, ..Default::default() };
        if opacity < 0.999 {
            picture_fill.alpha = Some(f64::from(opacity));
        }
        let model_line = if axis { self.model_line(&it.stroke, opacity) } else { craft_pptx::Line::none() };
        let model_fill = if axis { self.model_fill(&it.fill, it.path.is_closed(), opacity) } else { craft_pptx::Fill::None };
        let effects = self.model_effects(it, opacity);
        self.native_shapes.push(craft_pptx::Shape {
            id: craft_pptx::ShapeId(id),
            name: name_of(it, id),
            xfrm: Some(craft_pptx::Xfrm {
                x: geo.xfrm.rect.x0,
                y: geo.xfrm.rect.y0,
                w: geo.xfrm.rect.width(),
                h: geo.xfrm.rect.height(),
                rot: geo.xfrm.rot,
                flip_h: geo.xfrm.flip_h,
                flip_v: geo.xfrm.flip_v,
            }),
            kind: craft_pptx::ShapeKind::Picture { fill: picture_fill },
            geom: geo.model.clone(),
            fill: Some(model_fill),
            line: Some(model_line),
            effects: (effects != craft_pptx::Effects::default()).then_some(effects),
            descr: it.alt_text.clone(),
            ..Default::default()
        });
        self.native_media.push((id, name.clone(), svg));
    }

    /// Paragraph shading and rules, table cell fills: a plain filled rectangle in slide space.
    pub fn rect_shape(&mut self, xfrm: XfrmBox, fill: craft_pptx::Fill, name: &str) {
        let id = self.id();
        self.native_shapes.push(craft_pptx::Shape {
            id: craft_pptx::ShapeId(id),
            name: name.into(),
            xfrm: Some(craft_pptx::Xfrm {
                x: xfrm.rect.x0,
                y: xfrm.rect.y0,
                w: xfrm.rect.width(),
                h: xfrm.rect.height(),
                rot: xfrm.rot,
                flip_h: xfrm.flip_h,
                flip_v: xfrm.flip_v,
            }),
            geom: craft_pptx::Geom::preset("rect"),
            fill: Some(fill),
            line: Some(craft_pptx::Line::none()),
            ..Default::default()
        });
    }
}

fn name_of(it: &Item, id: u32) -> String {
    if !it.name.is_empty() {
        return it.name.clone();
    }
    let kind = match (&it.content, it.shape) {
        (Content::Group { .. }, _) => "Group",
        (Content::Text(_), _) => "Text Frame",
        (Content::Graphic(_), _) => "Picture",
        (_, Shape::Oval) => "Ellipse",
        (_, Shape::Polygon) => "Polygon",
        (_, Shape::GraphicLine) => "Line",
        (_, Shape::Path) => "Path",
        _ => "Rectangle",
    };
    format!("{kind} {id}")
}

fn unpremultiply(mut px: Vec<u8>) -> Vec<u8> {
    for p in px.as_chunks_mut::<4>().0 {
        let a = p[3] as u32;
        if a > 0 && a < 255 {
            for c in &mut p[..3] {
                *c = ((*c as u32 * 255 + a / 2) / a).min(255) as u8;
            }
        }
    }
    px
}

fn encode_png(rgba: Vec<u8>, w: u32, h: u32) -> Option<Vec<u8>> {
    let image = craft_image_codecs::Image::from_u8(w, h, craft_image_codecs::ChannelLayout::Rgba, rgba).ok()?;
    craft_image_codecs::encode(&image, craft_image_codecs::Format::Png, &Default::default()).ok()
}
