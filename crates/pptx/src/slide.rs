//! A page as a slide: items in stacking order as shapes, pictures, groups and text boxes.

use std::collections::HashMap;
use std::fmt::Write as _;

use designcraft_color::swatch::resolve_gradient;
use designcraft_color::{BlendMode, GradientKind};
use designcraft_compose::Cache;
use designcraft_doc::{Arrowhead, AssetId, Cap, Content, Document, Graphic, Item, Join, Shape, Stroke, StrokeAlign, StrokeType};
use designcraft_geom::kurbo::{PathEl, Shape as _};
use designcraft_geom::{Affine, BezPath, Point, Rect, Vec2, corners};

use crate::fonts::Fonts;
use crate::package::{Media, REL_IMAGE, Rel, Slide};
use crate::xml::{angle, emu, emu_len, esc, hex, pct};
use crate::{PptxError, PptxOptions, Result};

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

/// A DrawingML transform box (slide space, points).
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

    pub fn xml(&self) -> String {
        let mut a = String::new();
        let r = angle(self.rot);
        if r != 0 {
            let _ = write!(a, " rot=\"{r}\"");
        }
        if self.flip_h {
            a.push_str(" flipH=\"1\"");
        }
        if self.flip_v {
            a.push_str(" flipV=\"1\"");
        }
        format!(
            "<a:xfrm{a}><a:off x=\"{}\" y=\"{}\"/><a:ext cx=\"{}\" cy=\"{}\"/></a:xfrm>",
            emu(self.rect.x0),
            emu(self.rect.y0),
            emu_len(self.rect.width()),
            emu_len(self.rect.height())
        )
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
    pub xml: String,
}

fn custom_geometry(bp: &BezPath, w: f64, h: f64, filled: bool) -> String {
    let (we, he) = (emu_len(w).max(1), emu_len(h).max(1));
    let mut p = format!(
        "<a:custGeom><a:avLst/><a:gdLst/><a:ahLst/><a:cxnLst/><a:rect l=\"0\" t=\"0\" r=\"r\" b=\"b\"/><a:pathLst><a:path w=\"{we}\" h=\"{he}\"{}>",
        if filled { "" } else { " fill=\"none\"" }
    );
    let pt = |q: Point| format!("<a:pt x=\"{}\" y=\"{}\"/>", emu(q.x), emu(q.y));
    let mut last = Point::ZERO;
    for el in bp.elements() {
        match *el {
            PathEl::MoveTo(a) => {
                let _ = write!(p, "<a:moveTo>{}</a:moveTo>", pt(a));
                last = a;
            }
            PathEl::LineTo(a) => {
                let _ = write!(p, "<a:lnTo>{}</a:lnTo>", pt(a));
                last = a;
            }
            PathEl::QuadTo(a, b) => {
                // As a cubic: PowerPoint's quadratic support is patchy.
                let c1 = last + (a - last) * (2.0 / 3.0);
                let c2 = b + (a - b) * (2.0 / 3.0);
                let _ = write!(p, "<a:cubicBezTo>{}{}{}</a:cubicBezTo>", pt(c1), pt(c2), pt(b));
                last = b;
            }
            PathEl::CurveTo(a, b, c) => {
                let _ = write!(p, "<a:cubicBezTo>{}{}{}</a:cubicBezTo>", pt(a), pt(b), pt(c));
                last = c;
            }
            PathEl::ClosePath => p.push_str("<a:close/>"),
        }
    }
    p.push_str("</a:path></a:pathLst></a:custGeom>");
    p
}

fn preset(name: &str) -> String {
    format!("<a:prstGeom prst=\"{name}\"><a:avLst/></a:prstGeom>")
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
            return Geometry { xfrm, xml: preset("rect") };
        }
        if it.shape == Shape::Oval && it.corners.is_none() && closed {
            return Geometry { xfrm, xml: preset("ellipse") };
        }
        // The path in the box's own space: scaled (not flipped: the box flips), from its corner.
        let (kx, ky) = (dm.sx.abs(), dm.sy.abs());
        let mut local = bp.clone();
        local.apply_affine(Affine::scale_non_uniform(kx, ky) * Affine::translate(-inner.origin().to_vec2()));
        return Geometry { xfrm, xml: custom_geometry(&local, xfrm.rect.width(), xfrm.rect.height(), closed) };
    }
    // Sheared: the transformed path in slide space.
    let mut pb = bp;
    pb.apply_affine(m);
    let b = pb.bounding_box();
    pb.apply_affine(Affine::translate(-b.origin().to_vec2()));
    Geometry { xfrm: XfrmBox::plain(b), xml: custom_geometry(&pb, b.width(), b.height(), closed) }
}

pub struct Exporter<'a> {
    pub doc: &'a Document,
    pub cache: &'a Cache,
    pub opts: &'a PptxOptions,
    pub warnings: Vec<String>,
    pub media: Vec<Media>,
    /// Asset → (picture media name, SVG media name).
    media_of: HashMap<AssetId, Option<(String, Option<String>)>>,
    pub fonts: Fonts,
    // Per slide.
    pub out: String,
    next_id: u32,
    rels: Vec<Rel>,
    rel_of: HashMap<String, String>,
    /// Spread space → slide space.
    pub to_slide: Affine,
    /// The page's bleed area in spread space (items outside it are left out).
    clip: Rect,
    pub page_name: Option<String>,
}

impl<'a> Exporter<'a> {
    pub fn new(doc: &'a Document, cache: &'a Cache, opts: &'a PptxOptions) -> Self {
        Exporter {
            doc,
            cache,
            opts,
            warnings: Vec::new(),
            media: Vec::new(),
            media_of: HashMap::new(),
            fonts: Fonts::default(),
            out: String::new(),
            next_id: 2,
            rels: Vec::new(),
            rel_of: HashMap::new(),
            to_slide: Affine::IDENTITY,
            clip: Rect::ZERO,
            page_name: None,
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
        self.out.clear();
        self.next_id = 2;
        self.rels.clear();
        self.rel_of.clear();
        self.to_slide = Affine::translate(-bounds.origin().to_vec2());
        let b = doc.settings.bleed;
        self.clip = Rect::new(bounds.x0 - b[2].max(b[3]), bounds.y0 - b[0], bounds.x1 + b[2].max(b[3]), bounds.y1 + b[1]);
        let page_name = doc.page_name(abs);
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
                    if it.layer != layer.id || page.overridden.contains(&it.id) {
                        continue;
                    }
                    if parent.pages.len() > 1 && parent.page_at_x(it.bounds().center().x) != Some(ppage) {
                        continue;
                    }
                    // Parent items compose per page (page numbers, running heads).
                    self.page_name = Some(page_name.clone());
                    self.item(it, Affine::translate((dx, 0.0)), 1.0);
                    self.page_name = None;
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
            shapes: std::mem::take(&mut self.out),
            rels: std::mem::take(&mut self.rels),
            name: page_name,
        })
    }

    fn rel(&mut self, target: String) -> String {
        if let Some(r) = self.rel_of.get(&target) {
            return r.clone();
        }
        let id = format!("rId{}", self.rels.len() + 2);
        self.rels.push(Rel { id: id.clone(), kind: REL_IMAGE, target: target.clone() });
        self.rel_of.insert(target, id.clone());
        id
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
            self.shape(it, xf, opacity, None);
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
            _ => self.shape(it, xf, opacity, None),
        }
    }

    fn group(&mut self, it: &Item, xf: Affine, opacity: f32) {
        let mut body = std::mem::take(&mut self.out);
        let id = self.id();
        for c in it.shown_children() {
            self.item(c, xf, opacity);
        }
        let inner = std::mem::take(&mut self.out);
        if inner.is_empty() {
            self.out = body;
            return;
        }
        // Children are in slide space: the group's child space is the slide's.
        let inner_bounds = it.shown_children().map(|c| c.bounds()).reduce(|a, b| a.union(b)).unwrap_or(Rect::ZERO);
        let b = (self.to_slide * xf).transform_rect_bbox(inner_bounds);
        let (x, y, w, h) = (emu(b.x0), emu(b.y0), emu_len(b.width()), emu_len(b.height()));
        let _ = write!(
            body,
            "<p:grpSp><p:nvGrpSpPr><p:cNvPr id=\"{id}\" name=\"{}\"{}/><p:cNvGrpSpPr/><p:nvPr/></p:nvGrpSpPr><p:grpSpPr><a:xfrm><a:off x=\"{x}\" y=\"{y}\"/><a:ext cx=\"{w}\" cy=\"{h}\"/><a:chOff x=\"{x}\" y=\"{y}\"/><a:chExt cx=\"{w}\" cy=\"{h}\"/></a:xfrm></p:grpSpPr>{inner}</p:grpSp>",
            esc(&name_of(it, id)),
            descr(it)
        );
        self.out = body;
    }

    /// A shape with the item's geometry, fill, stroke and effects; `text` is a `<p:txBody>`.
    pub fn shape(&mut self, it: &Item, xf: Affine, opacity: f32, text: Option<&str>) {
        let m = self.to_slide * xf;
        let g = geometry(it, m);
        let fill = self.fill(&it.fill, it.path.is_closed(), opacity);
        let line = self.line(&it.stroke, opacity);
        let fx = self.effects(it, opacity);
        if text.is_none() && fill == "<a:noFill/>" && line.contains("<a:noFill/>") && fx.is_empty() {
            return;
        }
        let id = self.id();
        let txbox = if text.is_some() && it.fill.is_none() && it.stroke.is_none() { " txBox=\"1\"" } else { "" };
        let _ = write!(
            self.out,
            "<p:sp><p:nvSpPr><p:cNvPr id=\"{id}\" name=\"{}\"{}/><p:cNvSpPr{txbox}/><p:nvPr/></p:nvSpPr><p:spPr>{}{}{fill}{line}{fx}</p:spPr>{}</p:sp>",
            esc(&name_of(it, id)),
            descr(it),
            g.xfrm.xml(),
            g.xml,
            text.unwrap_or("")
        );
    }

    /// An sRGB colour element for a swatch and tint, `None` for [None] and unknown swatches.
    pub fn color(&self, swatch: &str, tint: f32, alpha: f32) -> Option<String> {
        if let Some(g) = resolve_gradient(&self.doc.swatches, swatch) {
            // A gradient used where only a colour fits (text): its first stop.
            let (_, c, a) = g.expanded_stops().into_iter().next()?;
            return Some(srgb(c.to_rgb(), a * alpha));
        }
        let c = self.doc.resolve_color(swatch, tint)?;
        Some(srgb(c.to_rgb(), alpha))
    }

    pub fn fill(&mut self, f: &designcraft_doc::Fill, closed: bool, opacity: f32) -> String {
        if f.is_none() || !closed {
            return "<a:noFill/>".into();
        }
        if let Some(g) = resolve_gradient(&self.doc.swatches, &f.swatch) {
            let mut gs = String::new();
            for (o, c, a) in g.expanded_stops() {
                let _ = write!(gs, "<a:gs pos=\"{}\">{}</a:gs>", pct(o.clamp(0.0, 1.0) as f64), srgb(c.to_rgb(), a * opacity));
            }
            if gs.is_empty() {
                return "<a:noFill/>".into();
            }
            if f.gradient_vector.is_some() {
                self.warn("gradients set with the Gradient tool use their angle only; PowerPoint stretches them over the whole shape");
            }
            let shade = match g.kind {
                GradientKind::Radial => {
                    "<a:path path=\"circle\"><a:fillToRect l=\"50000\" t=\"50000\" r=\"50000\" b=\"50000\"/></a:path>".to_string()
                }
                _ => {
                    let deg = match f.gradient_vector {
                        Some([x0, y0, x1, y1]) => (y1 - y0).atan2(x1 - x0).to_degrees(),
                        None => -f.gradient_angle.unwrap_or(0.0),
                    };
                    format!("<a:lin ang=\"{}\" scaled=\"0\"/>", angle(deg))
                }
            };
            return format!("<a:gradFill rotWithShape=\"1\"><a:gsLst>{gs}</a:gsLst>{shade}</a:gradFill>");
        }
        match self.color(&f.swatch, f.tint, opacity) {
            Some(c) => format!("<a:solidFill>{c}</a:solidFill>"),
            None => "<a:noFill/>".into(),
        }
    }

    pub fn line(&mut self, s: &Stroke, opacity: f32) -> String {
        if s.is_none() || s.weight <= 0.0 {
            return "<a:ln><a:noFill/></a:ln>".into();
        }
        let Some(color) = self.color(&s.swatch, s.tint, opacity) else { return "<a:ln><a:noFill/></a:ln>".into() };
        let cap = match s.cap {
            Cap::Butt => "flat",
            Cap::Round => "rnd",
            Cap::Projecting => "sq",
        };
        let cmpd = match s.kind {
            StrokeType::ThickThin => "thickThin",
            StrokeType::ThinThick => "thinThick",
            StrokeType::ThinThin | StrokeType::ThickThick => "dbl",
            StrokeType::ThinThickThin | StrokeType::ThickThinThick => "tri",
            _ => "sng",
        };
        if matches!(s.kind, StrokeType::Wavy | StrokeType::Hashed | StrokeType::Stripes { .. } | StrokeType::Style { .. }) {
            self.warn("wavy, hashed, striped and custom stroke styles are exported as solid lines");
        }
        let algn = match s.align {
            StrokeAlign::Inside => " algn=\"in\"",
            StrokeAlign::Outside => {
                self.warn("strokes aligned outside are centred on the path (PowerPoint has centre and inside only)");
                ""
            }
            StrokeAlign::Center => "",
        };
        let w = s.weight;
        let dash = match &s.kind {
            StrokeType::Dashed { pattern } if pattern.iter().any(|v| *v > 0.0) => {
                let mut v: Vec<f64> = pattern.iter().map(|x| x.max(0.0)).collect();
                if v.len() % 2 == 1 {
                    v.extend(v.clone());
                }
                let mut d = String::from("<a:custDash>");
                for pair in v.chunks(2) {
                    if let [a, b] = pair {
                        let _ = write!(d, "<a:ds d=\"{}\" sp=\"{}\"/>", pct(a / w), pct(b / w));
                    }
                }
                d.push_str("</a:custDash>");
                d
            }
            StrokeType::Dotted => "<a:prstDash val=\"sysDot\"/>".into(),
            _ => "<a:prstDash val=\"solid\"/>".into(),
        };
        let join = match s.join {
            Join::Miter => format!("<a:miter lim=\"{}\"/>", pct(s.miter_limit.max(1.0))),
            Join::Round => "<a:round/>".into(),
            Join::Bevel => "<a:bevel/>".into(),
        };
        let ends = |tag: &str, a: Arrowhead, me: &mut Self| -> String {
            let kind = match a {
                Arrowhead::None => return String::new(),
                Arrowhead::Simple | Arrowhead::SimpleWide => "arrow",
                Arrowhead::Triangle | Arrowhead::TriangleWide | Arrowhead::Curved => "triangle",
                Arrowhead::Barbed => "stealth",
                Arrowhead::Circle | Arrowhead::CircleSolid => "oval",
                Arrowhead::Square | Arrowhead::SquareSolid => "diamond",
                Arrowhead::Bar => {
                    me.warn("bar arrowheads have no PowerPoint equivalent and are left out");
                    return String::new();
                }
            };
            // PowerPoint's large heads come closest to DesignCraft's at the same weight.
            format!("<a:{tag} type=\"{kind}\" w=\"lg\" len=\"lg\"/>")
        };
        let head = ends("headEnd", s.start, self);
        let tail = ends("tailEnd", s.end, self);
        format!("<a:ln w=\"{}\" cap=\"{cap}\" cmpd=\"{cmpd}\"{algn}><a:solidFill>{color}</a:solidFill>{dash}{join}{head}{tail}</a:ln>", emu_len(w))
    }

    /// The item's effects that DrawingML has (in schema order).
    pub fn effects(&mut self, it: &Item, opacity: f32) -> String {
        let fx = &it.effects;
        let mut s = String::new();
        if fx.outer_glow.on
            && let Some(c) = self.color(&fx.outer_glow.color, 1.0, fx.outer_glow.opacity * opacity)
        {
            let _ = write!(s, "<a:glow rad=\"{}\">{c}</a:glow>", emu_len(fx.outer_glow.size));
        }
        if fx.inner_shadow.on {
            let sh = &fx.inner_shadow;
            if let Some(c) = self.color(&sh.color, 1.0, sh.opacity * opacity) {
                let a = self.doc.light_angle(sh.angle, sh.global_light);
                let _ = write!(
                    s,
                    "<a:innerShdw blurRad=\"{}\" dist=\"{}\" dir=\"{}\">{c}</a:innerShdw>",
                    emu_len(sh.size),
                    emu_len(sh.distance.abs()),
                    shadow_dir(a)
                );
            }
        }
        if fx.drop_shadow.on {
            let sh = &fx.drop_shadow;
            if let Some(c) = self.color(&sh.color, 1.0, sh.opacity * opacity) {
                let a = self.doc.light_angle(sh.angle, sh.global_light);
                let _ = write!(
                    s,
                    "<a:outerShdw blurRad=\"{}\" dist=\"{}\" dir=\"{}\" algn=\"ctr\" rotWithShape=\"0\">{c}</a:outerShdw>",
                    emu_len(sh.size),
                    emu_len(sh.distance.abs()),
                    shadow_dir(a)
                );
            }
        }
        if fx.feather > 0.0 {
            let _ = write!(s, "<a:softEdge rad=\"{}\"/>", emu_len(fx.feather));
        }
        if s.is_empty() { s } else { format!("<a:effectLst>{s}</a:effectLst>") }
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
            self.shape(it, xf, opacity, None);
            return;
        };
        let m = self.to_slide * xf;
        let [a, b, c, d, _, _] = g.xf.as_coeffs();
        let frame = it.inner_bounds();
        let img = g.xf.transform_rect_bbox(Rect::new(0.0, 0.0, g.size.0, g.size.1));
        let axis = b.abs() < 1e-9 && c.abs() < 1e-9 && a > 0.0 && d > 0.0;
        let (geo, crop) = if axis && img.width() > 0.0 && img.height() > 0.0 {
            // The frame's geometry with the image cropped (or padded) to the frame's box.
            let crop = format!(
                "<a:srcRect l=\"{}\" t=\"{}\" r=\"{}\" b=\"{}\"/>",
                pct((frame.x0 - img.x0) / img.width()),
                pct((frame.y0 - img.y0) / img.height()),
                pct((img.x1 - frame.x1) / img.width()),
                pct((img.y1 - frame.y1) / img.height())
            );
            (geometry(it, m), crop)
        } else {
            // Rotated, skewed or flipped inside its frame: the image's own box, uncropped, and
            // the frame drawn on its own.
            self.warn("images rotated, skewed or flipped inside their frames are not cropped to the frame");
            self.shape(it, xf, opacity, None);
            let gi = Rect::new(0.0, 0.0, g.size.0, g.size.1);
            let gm = m * g.xf;
            let xfrm = box_of(gi, gm).map(|(x, _)| x).unwrap_or_else(|| XfrmBox::plain(gm.transform_rect_bbox(gi)));
            (Geometry { xfrm, xml: preset("rect") }, String::new())
        };
        let rid = self.rel(format!("../media/{name}"));
        let svg_ext = match svg {
            Some(s) => {
                let sid = self.rel(format!("../media/{s}"));
                format!(
                    "<a:extLst><a:ext uri=\"{{96DAC541-7B7A-43D3-8B79-37D633B846F1}}\"><asvg:svgBlip xmlns:asvg=\"http://schemas.microsoft.com/office/drawing/2016/SVG/main\" r:embed=\"{sid}\"/></a:ext></a:extLst>"
                )
            }
            None => String::new(),
        };
        let alpha = if opacity < 0.999 { format!("<a:alphaModFix amt=\"{}\"/>", pct(opacity as f64)) } else { String::new() };
        let fill = self.fill(&it.fill, it.path.is_closed(), opacity);
        let line = if axis { self.line(&it.stroke, opacity) } else { "<a:ln><a:noFill/></a:ln>".into() };
        let fx = self.effects(it, opacity);
        let id = self.id();
        let _ = write!(
            self.out,
            "<p:pic><p:nvPicPr><p:cNvPr id=\"{id}\" name=\"{}\"{}/><p:cNvPicPr><a:picLocks noChangeAspect=\"1\"/></p:cNvPicPr><p:nvPr/></p:nvPicPr>\
<p:blipFill><a:blip r:embed=\"{rid}\">{alpha}{svg_ext}</a:blip>{crop}<a:stretch><a:fillRect/></a:stretch></p:blipFill>\
<p:spPr>{}{}{fill}{line}{fx}</p:spPr></p:pic>",
            esc(&name_of(it, id)),
            descr(it),
            geo.xfrm.xml(),
            geo.xml
        );
    }

    /// Paragraph shading and rules, table cell fills: a plain filled rectangle in slide space.
    pub fn rect_shape(&mut self, xfrm: XfrmBox, fill: &str, name: &str) {
        let id = self.id();
        let _ = write!(
            self.out,
            "<p:sp><p:nvSpPr><p:cNvPr id=\"{id}\" name=\"{}\"/><p:cNvSpPr/><p:nvPr/></p:nvSpPr><p:spPr>{}{}{fill}<a:ln><a:noFill/></a:ln></p:spPr></p:sp>",
            esc(name),
            xfrm.xml(),
            preset("rect")
        );
    }
}

/// DrawingML shadow direction (clockwise from +x, y down) for a light angle (degrees,
/// counter-clockwise; the shadow falls away from the light).
fn shadow_dir(light: f64) -> i64 {
    let a = light.to_radians();
    let off = Vec2::new(-a.cos(), a.sin());
    angle(off.y.atan2(off.x).to_degrees())
}

pub fn srgb(rgb: [f32; 3], alpha: f32) -> String {
    let a = alpha.clamp(0.0, 1.0);
    if a >= 0.999 {
        format!("<a:srgbClr val=\"{}\"/>", hex(rgb))
    } else {
        format!("<a:srgbClr val=\"{}\"><a:alpha val=\"{}\"/></a:srgbClr>", hex(rgb), pct(a as f64))
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

fn descr(it: &Item) -> String {
    if it.alt_text.is_empty() { String::new() } else { format!(" descr=\"{}\"", esc(&it.alt_text)) }
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
    let img = image::RgbaImage::from_raw(w, h, rgba)?;
    let mut out = std::io::Cursor::new(Vec::new());
    img.write_to(&mut out, image::ImageFormat::Png).ok()?;
    Some(out.into_inner())
}
