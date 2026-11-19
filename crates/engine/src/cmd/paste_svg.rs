//! Paste vector art copied in another app (Illustrator, VectorCraft, Figma, Inkscape) as editable
//! paths, as InDesign pastes Illustrator art. Those apps put the copied objects on the system
//! clipboard as SVG markup; each path becomes a DesignCraft path with its fill and stroke, groups
//! stay groups, gradients stay gradients, and text comes in as outlines.

use std::collections::{HashMap, HashSet};
use std::sync::Arc;

use craft_svg::tree::document::{Document as SvgDocument, LineCap, LineJoin, Node as SvgNode, Paint as SvgPaint, Segment};
use designcraft_doc::{Content, Fill, Item, ItemId, Selection, Shape, Stroke, StrokeType};
use designcraft_geom::{Affine, BezPath, PathData, Point, Rect};
use designcraft_images::usvg;
use serde_json::{Value, json};

use super::{CommandSpec, Result, Session, bad, cmd, has_doc, spread_param, str_param};

pub fn specs() -> Vec<CommandSpec> {
    vec![cmd!(
        "edit.pasteSvg",
        "Paste Vector Art",
        [],
        None,
        "{svg: markup (what Illustrator, VectorCraft or Figma put on the clipboard), spread?, at?: [x, y] (spread coords of the art's centre; default the first page's centre)} → {ids, warnings} — paths with their fills and strokes, groups as groups, text as outlines",
        has_doc,
        paste_svg
    )]
}

/// Is `text` SVG markup (rather than text to paste as text)?
pub fn is_svg_markup(text: &str) -> bool {
    designcraft_images::is_svg(text.as_bytes())
}

/// Cap conversion nesting too; the shared SVG scene builder enforces this limit.
const MAX_DEPTH: usize = 64;

/// A converted SVG node, in points.
enum Art {
    Path { path: BezPath, fill: Option<Paint>, stroke: Option<(Paint, StrokeStyle)>, opacity: f32 },
    Group(Vec<Art>, f32),
}

impl Art {
    /// Fold a wrapping group's opacity into this object.
    fn fade(&mut self, by: f32) {
        match self {
            Art::Path { opacity, .. } | Art::Group(_, opacity) => *opacity *= by,
        }
    }
}

struct Paint {
    rgb: [u8; 3],
    opacity: f32,
    /// A gradient paint's definition and placement (`rgb` is then its first stop).
    gradient: Option<Box<PastedGradient>>,
}

/// An SVG gradient definition converted once and shared by every paint that uses it: the parsed
/// tree holds its own copy of the stops for each use, so one gradient of many stops referenced by
/// many paths must not be converted (or kept) once per path.
struct GradientDef {
    gradient: designcraft_color::Gradient,
    /// The definition as a swatch key (equal gradients, equal keys), worked out once.
    key: String,
}

/// An SVG gradient as the shared scene can't hold it (it keeps the first colour only): the
/// shared definition, and where it sits in the art's CSS-pixel space.
#[derive(Clone)]
struct PastedGradient {
    def: Arc<GradientDef>,
    at: GradientAt,
}

#[derive(Clone, Copy)]
enum GradientAt {
    Linear(Point, Point),
    /// Centre and horizontal and vertical radii.
    Radial(Point, f64, f64),
}

/// The gradient definitions converted so far, by a hash of their kind and stops (the parsed tree
/// gives each use its own copy, so a gradient's address is no identity across uses).
#[derive(Default)]
struct GradientDefs {
    by_hash: HashMap<u64, Vec<Arc<GradientDef>>>,
}

impl GradientDefs {
    /// The definition for `base`, converted and keyed on first sight. A hash collision is checked
    /// by comparing the stops, so two different gradients never share a definition.
    fn get(&mut self, kind: designcraft_color::GradientKind, base: &usvg::BaseGradient) -> Option<Arc<GradientDef>> {
        use designcraft_color::{Color, Gradient, GradientKind, GradientStop};
        use std::hash::{Hash, Hasher};
        let stops = base.stops();
        if stops.is_empty() {
            return None;
        }
        let mut h = std::collections::hash_map::DefaultHasher::new();
        matches!(kind, GradientKind::Radial).hash(&mut h);
        stops.len().hash(&mut h);
        for s in stops {
            s.offset().get().to_bits().hash(&mut h);
            let c = s.color();
            (c.red, c.green, c.blue).hash(&mut h);
            s.opacity().get().to_bits().hash(&mut h);
        }
        let bucket = self.by_hash.entry(h.finish()).or_default();
        if let Some(def) = bucket.iter().find(|d| Self::same(&d.gradient, kind, stops)) {
            return Some(Arc::clone(def));
        }
        let gradient = Gradient {
            kind,
            stops: stops
                .iter()
                .map(|s| {
                    let c = s.color();
                    GradientStop { opacity: s.opacity().get(), ..GradientStop::new(s.offset().get(), Color::rgb8(c.red, c.green, c.blue)) }
                })
                .collect(),
        };
        let def = Arc::new(GradientDef { key: format!("{gradient:?}"), gradient });
        bucket.push(Arc::clone(&def));
        Some(def)
    }

    fn same(gradient: &designcraft_color::Gradient, kind: designcraft_color::GradientKind, stops: &[usvg::Stop]) -> bool {
        use designcraft_color::Color;
        gradient.kind == kind
            && gradient.stops.len() == stops.len()
            && gradient.stops.iter().zip(stops).all(|(g, s)| {
                let c = s.color();
                g.offset == s.offset().get() && g.opacity == s.opacity().get() && g.color == Color::rgb8(c.red, c.green, c.blue)
            })
    }
}

/// The fill and stroke gradients of each path the shared scene keeps, in its order.
type PathGradients = [Option<PastedGradient>; 2];

fn mapped(m: usvg::Transform, x: f32, y: f32) -> Point {
    let mut p = usvg::tiny_skia_path::Point::from_xy(x, y);
    m.map_point(&mut p);
    Point::new(f64::from(p.x), f64::from(p.y))
}

/// The far end of a linear gradient's axis after `m`. SVG's constant-colour lines are
/// perpendicular to the gradient vector in the gradient's own space, and an affine transform
/// needn't keep them perpendicular to the transformed vector; so this is the foot of the
/// perpendicular from the transformed start onto the transformed end's constant-colour line, not
/// the transformed end. Under a non-uniform scale the two differ, and the colours would sit in
/// the wrong places.
fn linear_end(m: usvg::Transform, g: &usvg::LinearGradient) -> Point {
    let a = mapped(m, g.x1(), g.y1());
    let (x1, y1, x2, y2) = (f64::from(g.x1()), f64::from(g.y1()), f64::from(g.x2()), f64::from(g.y2()));
    let (dx, dy) = (x2 - x1, y2 - y1);
    let (sx, kx, ky, sy) = (f64::from(m.sx), f64::from(m.kx), f64::from(m.ky), f64::from(m.sy));
    let det = sx * sy - kx * ky;
    // `det` times the transformed lines' normal: the inverse transpose of the transform's linear
    // part, applied to the gradient vector.
    let (nx, ny) = (sy * dx - ky * dy, sx * dy - kx * dx);
    let n2 = nx * nx + ny * ny;
    if det.abs() <= f64::EPSILON || n2 <= f64::EPSILON {
        // A singular transform: the transformed end is all there is.
        return mapped(m, g.x2(), g.y2());
    }
    let s = (dx * dx + dy * dy) * det / n2;
    Point::new(a.x + nx * s, a.y + ny * s)
}

fn pasted_gradient(paint: &usvg::Paint, abs: usvg::Transform, defs: &mut GradientDefs) -> Option<PastedGradient> {
    use designcraft_color::GradientKind;
    let (kind, base, at): (_, &usvg::BaseGradient, _) = match paint {
        usvg::Paint::LinearGradient(g) => {
            let m = abs.pre_concat(g.transform());
            (GradientKind::Linear, g, GradientAt::Linear(mapped(m, g.x1(), g.y1()), linear_end(m, g)))
        }
        usvg::Paint::RadialGradient(g) => {
            // The circle's radius along each axis after the transforms (an ellipse, as an
            // objectBoundingBox gradient on a box that isn't square).
            let m = abs.pre_concat(g.transform());
            let r = f64::from(g.r().get());
            let rx = r * f64::from(m.sx).hypot(f64::from(m.ky));
            let ry = r * f64::from(m.kx).hypot(f64::from(m.sy));
            (GradientKind::Radial, g, GradientAt::Radial(mapped(m, g.cx(), g.cy()), rx, ry))
        }
        usvg::Paint::Color(_) | usvg::Paint::Pattern(_) => return None,
    };
    Some(PastedGradient { def: defs.get(kind, base)?, at })
}

/// The gradients of the paths [`SvgDocument::from_tree`] keeps, in the order it keeps them: it
/// skips the same nodes (hidden or empty paths, images, groups past `max_depth`).
fn path_gradients(group: &usvg::Group, depth: usize, max_depth: usize, defs: &mut GradientDefs, out: &mut Vec<PathGradients>) {
    for node in group.children() {
        match node {
            usvg::Node::Path(path) => {
                if !path.is_visible() || path.data().is_empty() {
                    continue;
                }
                let abs = path.abs_transform();
                let fill = path.fill().and_then(|f| pasted_gradient(f.paint(), abs, defs));
                let stroke = path.stroke().and_then(|s| pasted_gradient(s.paint(), abs, defs));
                out.push([fill, stroke]);
            }
            usvg::Node::Group(child) if depth < max_depth => path_gradients(child, depth + 1, max_depth, defs, out),
            usvg::Node::Text(text) if depth < max_depth => path_gradients(text.flattened(), depth + 1, max_depth, defs, out),
            _ => {}
        }
    }
}

struct StrokeStyle {
    width: f64,
    cap: designcraft_doc::Cap,
    join: designcraft_doc::Join,
    miter: f64,
    dashes: Option<Vec<f64>>,
}

#[derive(Default)]
struct Notes {
    gradients: usize,
    images: usize,
    /// Paths whose fill-opacity and stroke-opacity differ (one object opacity holds only one).
    mixed_opacity: usize,
    too_deep: bool,
}

/// A scene paint; `gradient` is the same paint's gradient read from the SVG tree.
fn paint(p: &SvgPaint, gradient: Option<PastedGradient>, notes: &mut Notes) -> Option<Paint> {
    let (rgb, opacity, gradient) = match p {
        SvgPaint::Color { rgb, opacity } => (*rgb, *opacity, None),
        SvgPaint::Gradient { first_rgb, opacity } => {
            let rgb = *first_rgb.as_ref()?;
            // The tree's gradient, when it's this one (it starts with the scene's first colour).
            let gradient =
                gradient.filter(|g| g.def.gradient.stops.first().is_some_and(|s| s.color == designcraft_color::Color::rgb8(rgb[0], rgb[1], rgb[2])));
            if gradient.is_none() {
                // Retained as its first colour.
                notes.gradients += 1;
            }
            (rgb, *opacity, gradient.map(Box::new))
        }
        SvgPaint::Pattern => {
            notes.gradients += 1;
            return None;
        }
    };
    Some(Paint { rgb, opacity, gradient })
}

/// To 1/10000 pt: SVG numbers are 32-bit floats (a 1 pt stroke reads as 0.99999994).
fn round(v: f64) -> f64 {
    (v * 10_000.0).round() / 10_000.0
}

fn bezpath(segments: &[Segment]) -> BezPath {
    let p = |q: [f64; 2]| Point::new(q[0] * designcraft_images::PT_PER_PX, q[1] * designcraft_images::PT_PER_PX);
    let mut bp = BezPath::new();
    for seg in segments {
        match seg {
            Segment::MoveTo(a) => bp.move_to(p(*a)),
            Segment::LineTo(a) => bp.line_to(p(*a)),
            Segment::QuadTo(a, b) => bp.quad_to(p(*a), p(*b)),
            Segment::CubicTo(a, b, c) => bp.curve_to(p(*a), p(*b), p(*c)),
            Segment::Close => bp.close_path(),
        }
    }
    bp
}

/// `gradients` holds each scene path's gradients from [`path_gradients`], taken in turn.
fn convert(nodes: &[SvgNode], gradients: &mut std::vec::IntoIter<PathGradients>, notes: &mut Notes, out: &mut Vec<Art>) {
    for node in nodes {
        match node {
            SvgNode::Path(path) => {
                let [fill_gradient, stroke_gradient] = gradients.next().unwrap_or_default();
                let fill = path.fill.as_ref().and_then(|p| paint(p, fill_gradient, notes));
                let stroke = path.stroke.as_ref().and_then(|s| {
                    let c = paint(&s.paint, stroke_gradient, notes)?;
                    let width = round(s.width * designcraft_images::PT_PER_PX);
                    let dashes = s.dash_array.as_ref().map(|d| d.iter().map(|v| round(v * designcraft_images::PT_PER_PX)).collect::<Vec<_>>());
                    let style = StrokeStyle {
                        width,
                        cap: match s.cap {
                            LineCap::Butt => designcraft_doc::Cap::Butt,
                            LineCap::Round => designcraft_doc::Cap::Round,
                            LineCap::Square => designcraft_doc::Cap::Projecting,
                        },
                        join: match s.join {
                            LineJoin::Round => designcraft_doc::Join::Round,
                            LineJoin::Bevel => designcraft_doc::Join::Bevel,
                            _ => designcraft_doc::Join::Miter,
                        },
                        miter: s.miter_limit,
                        dashes,
                    };
                    Some((c, style))
                });
                if fill.is_none() && stroke.is_none() {
                    continue;
                }
                if let (Some(f), Some((c, _))) = (&fill, &stroke)
                    && (f.opacity - c.opacity).abs() > f32::EPSILON
                {
                    notes.mixed_opacity += 1;
                }
                out.push(Art::Path { path: bezpath(&path.segments), fill, stroke, opacity: 1.0 });
            }
            SvgNode::Group { children, opacity } => {
                let mut items = Vec::new();
                convert(children, gradients, notes, &mut items);
                // usvg wraps nodes in groups for transforms and opacity; only a group of several
                // objects stays a group. Its opacity goes to what it holds.
                if items.len() > 1 {
                    out.push(Art::Group(items, *opacity));
                } else {
                    for mut a in items {
                        a.fade(*opacity);
                        out.push(a);
                    }
                }
            }
        }
    }
}

fn bounds(art: &[Art]) -> Option<Rect> {
    art.iter()
        .filter_map(|a| match a {
            Art::Path { path, .. } => Some(designcraft_geom::Shape::bounding_box(path)),
            Art::Group(items, _) => bounds(items),
        })
        .reduce(|a, b| a.union(b))
}

/// The document's RGB colour swatches by colour and its swatch names, looked up once per paste
/// (a pasted SVG can hold tens of thousands of colours).
struct PasteSwatches {
    by_rgb: HashMap<[u32; 3], String>,
    by_gradient: HashMap<String, String>,
    names: HashSet<String>,
}

impl PasteSwatches {
    /// A gradient as a key: equal gradients, equal keys.
    fn gradient_key(g: &designcraft_color::Gradient) -> String {
        format!("{g:?}")
    }

    /// An RGB colour as a key: equal colours, equal keys (`+ 0.0` makes -0 and 0 one key).
    fn key(r: f32, g: f32, b: f32) -> [u32; 3] {
        [(r + 0.0).to_bits(), (g + 0.0).to_bits(), (b + 0.0).to_bits()]
    }

    fn new(d: &designcraft_doc::Document) -> Self {
        use designcraft_color::swatch::SwatchValue;
        let (mut by_rgb, mut by_gradient) = (HashMap::new(), HashMap::new());
        for w in &d.swatches {
            // The first swatch of a colour or gradient is the one, as a search from the start finds it.
            match &w.value {
                SwatchValue::Color { color: designcraft_color::Color::Rgb { r, g, b }, .. } => {
                    by_rgb.entry(Self::key(*r, *g, *b)).or_insert_with(|| w.name.clone());
                }
                SwatchValue::Gradient { gradient } => {
                    by_gradient.entry(Self::gradient_key(gradient)).or_insert_with(|| w.name.clone());
                }
                _ => {}
            }
        }
        Self { by_rgb, by_gradient, names: d.swatches.iter().map(|w| w.name.clone()).collect() }
    }
}

/// The swatch holding exactly `rgb`: an existing process colour, else a new unnamed one (hidden
/// from the Swatches panel until Add to Swatches), as InDesign does for pasted colours.
fn swatch_for(d: &mut designcraft_doc::Document, known: &mut PasteSwatches, rgb: [u8; 3]) -> String {
    use designcraft_color::swatch::{Swatch, SwatchValue};
    let color = designcraft_color::Color::rgb8(rgb[0], rgb[1], rgb[2]);
    let designcraft_color::Color::Rgb { r, g, b } = color else { return String::new() };
    let key = PasteSwatches::key(r, g, b);
    if let Some(name) = known.by_rgb.get(&key) {
        return name.clone();
    }
    let base = format!("R={} G={} B={}", rgb[0], rgb[1], rgb[2]);
    let name = designcraft_doc::Styles::unique_name(|n| known.names.contains(n), &base);
    known.names.insert(name.clone());
    known.by_rgb.insert(key, name.clone());
    d.swatches.push(Swatch {
        name: name.clone(),
        value: SwatchValue::Color { color, color_type: designcraft_color::ColorType::Process },
        locked: false,
        named: false,
        hidden: true,
    });
    name
}

/// The swatch holding exactly `def`: an existing gradient swatch, else a new unnamed one (hidden
/// until Add to Swatches), as Swatches ▸ apply does for an unnamed gradient.
fn gradient_swatch_for(d: &mut designcraft_doc::Document, known: &mut PasteSwatches, def: &GradientDef) -> String {
    use designcraft_color::swatch::{Swatch, SwatchValue};
    if let Some(name) = known.by_gradient.get(&def.key) {
        return name.clone();
    }
    let name = designcraft_doc::Styles::unique_name(|n| known.names.contains(n), "Gradient");
    known.names.insert(name.clone());
    known.by_gradient.insert(def.key.clone(), name.clone());
    d.swatches.push(Swatch {
        name: name.clone(),
        value: SwatchValue::Gradient { gradient: def.gradient.clone() },
        locked: false,
        named: false,
        hidden: true,
    });
    name
}

/// A pasted fill's or stroke's swatch and gradient vector (in the item's space, which is the
/// spread's: pasted items aren't transformed), on an object whose bounds are `b`.
fn paint_swatch(d: &mut designcraft_doc::Document, known: &mut PasteSwatches, p: Paint, shift: Affine, b: Rect) -> (String, Option<[f64; 4]>) {
    let Some(PastedGradient { def, at }) = p.gradient.map(|g| *g) else {
        return (swatch_for(d, known, p.rgb), None);
    };
    let pt = |q: Point| shift * Point::new(q.x * designcraft_images::PT_PER_PX, q.y * designcraft_images::PT_PER_PX);
    let vector = match at {
        // A gradient with no length paints no ramp; it fits the object instead.
        GradientAt::Linear(a, z) => Some((pt(a), pt(z))).filter(|(a, z)| (*z - *a).hypot() > 1e-9),
        GradientAt::Radial(c, rx, ry) => {
            // DesignCraft's radial gradient stretches with the object's box (`Fill::radial_ellipse`):
            // the radius that gives the SVG ellipse's area, which is the SVG ellipse itself when
            // it has the box's proportions (an objectBoundingBox gradient).
            let k = if b.width() > 1e-9 && b.height() > 1e-9 { b.height() / b.width() } else { 1.0 };
            let r = (rx * ry / k).sqrt() * designcraft_images::PT_PER_PX;
            let c = pt(c);
            Some((c, c + designcraft_geom::Vec2::new(r, 0.0))).filter(|(a, z)| (*z - *a).hypot() > 1e-9)
        }
    }
    .filter(|(a, z)| [a.x, a.y, z.x, z.y].iter().all(|v| v.is_finite()))
    .map(|(a, z)| [a.x, a.y, z.x, z.y]);
    (gradient_swatch_for(d, known, &def), vector)
}

fn build(d: &mut designcraft_doc::Document, known: &mut PasteSwatches, art: Vec<Art>, layer: designcraft_doc::LayerId, shift: Affine) -> Vec<Item> {
    let mut out = Vec::new();
    for a in art {
        match a {
            Art::Path { path, fill, stroke, opacity } => {
                let path = shift * path;
                let data = PathData::from_bezpath(&path);
                if data.subpaths.is_empty() {
                    continue;
                }
                let anchors: usize = data.subpaths.iter().map(|s| s.anchors.len()).sum();
                // A straight two-point segment is a graphic line (Line tool); anything else a path.
                let straight = |a: &designcraft_geom::Anchor| a.h_in == a.p && a.h_out == a.p;
                let line = data.subpaths.len() == 1 && anchors == 2 && data.subpaths.iter().all(|s| !s.closed && s.anchors.iter().all(straight));
                // The object's opacity: its own times the paint's. Items have no separate fill and
                // stroke opacity, so a path with both takes the fill's (`Notes::mixed_opacity`).
                let paint = fill.as_ref().map(|f| f.opacity).or(stroke.as_ref().map(|(c, _)| c.opacity)).unwrap_or(1.0);
                let b = designcraft_geom::Shape::bounding_box(&path);
                let mut it = Item::new(ItemId(d.alloc()), layer, if line { Shape::GraphicLine } else { Shape::Path }, data);
                it.fill = match fill {
                    Some(f) => {
                        let (swatch, gradient_vector) = paint_swatch(d, known, f, shift, b);
                        Fill { gradient_vector, ..Fill::swatch(&swatch) }
                    }
                    None => Fill::none(),
                };
                it.stroke = match stroke {
                    Some((c, s)) => {
                        let (swatch, gradient_vector) = paint_swatch(d, known, c, shift, b);
                        Stroke {
                            swatch,
                            gradient_vector,
                            weight: s.width,
                            cap: s.cap,
                            join: s.join,
                            miter_limit: s.miter,
                            kind: s.dashes.map_or(StrokeType::Solid, |pattern| StrokeType::Dashed { pattern }),
                            ..Stroke::default()
                        }
                    }
                    None => Stroke::none(),
                };
                it.opacity = (opacity * paint).clamp(0.0, 1.0);
                out.push(it);
            }
            Art::Group(items, opacity) => {
                let children = build(d, known, items, layer, shift);
                if children.is_empty() {
                    continue;
                }
                let mut g = Item::new(ItemId(d.alloc()), layer, Shape::Group, PathData::default());
                g.stroke = Stroke::none();
                g.opacity = opacity.clamp(0.0, 1.0);
                g.content = Content::Group { items: children.into_iter().map(std::sync::Arc::new).collect() };
                out.push(g);
            }
        }
    }
    out
}

fn paste_svg(s: &mut Session, p: &Value) -> Result<Value> {
    const ID: &str = "edit.pasteSvg";
    let svg = str_param(p, "svg").ok_or_else(|| bad(ID, "missing svg"))?;
    if !is_svg_markup(svg) {
        return Err(bad(ID, "the clipboard doesn't hold SVG markup"));
    }
    let tree = designcraft_images::svg_tree(svg.as_bytes()).ok_or_else(|| bad(ID, "the SVG on the clipboard can't be read"))?;
    let scene = SvgDocument::from_tree(&tree, MAX_DEPTH);
    let mut notes = Notes { images: scene.images, too_deep: scene.too_deep, ..Default::default() };
    let mut gradients = Vec::new();
    let mut defs = GradientDefs::default();
    path_gradients(tree.root(), 0, MAX_DEPTH, &mut defs, &mut gradients);
    let mut art = Vec::new();
    convert(&scene.nodes, &mut gradients.into_iter(), &mut notes, &mut art);
    let b = bounds(&art).ok_or_else(|| bad(ID, "the SVG on the clipboard has no paths to paste"))?;
    let sr = spread_param(p, ID)?;
    let layer = s.doc()?.active_layer;
    let at = match super::point_param(p, "at") {
        Some(at) => at,
        None => {
            let d = &s.doc()?.doc;
            let sp = d.spread(sr).ok_or_else(|| bad(ID, "no such spread"))?;
            sp.pages.first().map_or(Point::ZERO, |pg| pg.bounds().center())
        }
    };
    if !(at.x.is_finite() && at.y.is_finite()) {
        return Err(bad(ID, "at must be finite"));
    }
    let shift = Affine::translate(at - b.center());
    let mut warnings = Vec::new();
    if notes.gradients > 0 {
        warnings.push(format!("{} gradient or pattern paint(s) pasted as their first colour", notes.gradients));
    }
    if notes.too_deep {
        warnings.push(format!("objects nested more than {MAX_DEPTH} groups deep left out"));
    }
    if notes.mixed_opacity > 0 {
        warnings.push(format!("{} object(s) with different fill and stroke opacity pasted with the fill's opacity for both", notes.mixed_opacity));
    }
    if notes.images > 0 {
        warnings.push(format!("{} embedded image(s) left out (place images with File › Place)", notes.images));
    }
    s.edit(|d, sel| {
        let mut known = PasteSwatches::new(d);
        let items = build(d, &mut known, art, layer, shift);
        let mut ids = Vec::new();
        for it in items {
            ids.push(it.id);
            d.insert_item(sr, it, None)?;
        }
        *sel = Selection::items(ids.clone());
        Ok(json!({"ids": ids.iter().map(|i| i.0).collect::<Vec<_>>(), "warnings": warnings}))
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Session;
    use designcraft_doc::SpreadRef;

    const SPIRAL: &str = r##"<?xml version="1.0" encoding="UTF-8"?>
<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 400 400" width="400pt" height="400pt">
<path fill="none" stroke="#000000" stroke-width="1" d="M200,200 C200,210 210,220 220,220 C240,220 250,200 250,180 L300,100"/>
</svg>"##;

    fn session() -> Session {
        let mut s = Session::new();
        s.execute("file.new", &json!({})).unwrap();
        s
    }

    #[test]
    fn an_illustrator_path_pastes_as_an_editable_path_in_points() {
        let mut s = session();
        let undo = s.doc().unwrap().history.undo.len();
        let r = s.execute("edit.pasteSvg", &json!({"svg": SPIRAL, "at": [300, 400]})).unwrap();
        let ids = r["ids"].as_array().unwrap();
        assert_eq!(ids.len(), 1, "{r}");
        let st = s.doc().unwrap();
        let it = st.doc.item(ItemId(ids[0].as_u64().unwrap())).unwrap();
        assert_eq!(it.shape, Shape::Path);
        // Five anchors: three curve ends and a line, editable with the Direct Selection tool.
        assert_eq!(it.path.subpaths.len(), 1);
        assert_eq!(it.path.subpaths[0].anchors.len(), 4);
        assert!(it.fill.is_none());
        assert_eq!(it.stroke.weight, 1.0, "a 1-unit stroke in a pt-sized SVG is 1 pt");
        let c = st.doc.swatch(&it.stroke.swatch).unwrap();
        assert!(matches!(&c.value, designcraft_color::swatch::SwatchValue::Color { color, .. } if color.to_rgb() == [0.0, 0.0, 0.0]));
        // Centred on `at`, at its own size (100 × 120 user units of a 400 pt viewBox).
        let b = it.bounds();
        assert!((b.center().x - 300.0).abs() < 0.01 && (b.center().y - 400.0).abs() < 0.01, "{b:?}");
        assert!((b.width() - 100.0).abs() < 0.01 && (b.height() - 120.0).abs() < 0.01, "{b:?}");
        assert_eq!(st.selection.items, vec![it.id]);
        assert_eq!(st.history.undo.len(), undo + 1, "one undo step");
    }

    #[test]
    fn groups_fills_strokes_and_px_units_come_through() {
        let mut s = session();
        let svg = r##"<svg xmlns="http://www.w3.org/2000/svg" width="200" height="100">
            <g id="logo"><rect x="0" y="0" width="100" height="100" fill="#e92819" opacity="0.5"/>
            <circle cx="150" cy="50" r="50" fill="none" stroke="#0000ff" stroke-width="8" stroke-dasharray="4 2" stroke-linecap="round"/></g>
            <line x1="0" y1="0" x2="200" y2="0" stroke="#000"/></svg>"##;
        let r = s.execute("edit.pasteSvg", &json!({"svg": svg})).unwrap();
        let st = s.doc().unwrap();
        let ids: Vec<ItemId> = r["ids"].as_array().unwrap().iter().map(|v| ItemId(v.as_u64().unwrap())).collect();
        assert_eq!(ids.len(), 2, "the group and the line: {r}");
        let g = st.doc.item(ids[0]).unwrap();
        let Content::Group { items } = &g.content else { panic!("group") };
        assert_eq!(items.len(), 2);
        let (rect, circle) = (&items[0], &items[1]);
        assert_eq!(st.doc.swatch(&rect.fill.swatch).unwrap().name, "R=233 G=40 B=25");
        assert!((rect.opacity - 0.5).abs() < 1e-3);
        // CSS pixels: 8 px = 6 pt, the 200 px art is 150 pt wide.
        assert!((circle.stroke.weight - 6.0).abs() < 1e-6);
        assert_eq!(circle.stroke.cap, designcraft_doc::Cap::Round);
        assert!(matches!(&circle.stroke.kind, StrokeType::Dashed { pattern } if (pattern[0] - 3.0).abs() < 1e-6));
        assert_eq!(st.doc.item(ids[1]).unwrap().shape, Shape::GraphicLine);
        let all = g.bounds().union(st.doc.item(ids[1]).unwrap().bounds());
        assert!((all.width() - 150.0).abs() < 0.5, "{all:?}");
        // Centred on the first page by default.
        let page = st.doc.spread(SpreadRef::Doc(0)).unwrap().pages[0].bounds();
        assert!((all.center().x - page.center().x).abs() < 0.5, "{all:?} {page:?}");
    }

    #[test]
    fn differing_fill_and_stroke_opacity_takes_the_fills_and_warns() {
        let mut s = session();
        let svg = |stroke_opacity: &str| {
            format!(
                r##"<svg xmlns="http://www.w3.org/2000/svg" width="100pt" height="100pt" viewBox="0 0 100 100"><rect width="100" height="100" fill="#ff0000" fill-opacity="0.4" stroke="#000" stroke-opacity="{stroke_opacity}"/></svg>"##
            )
        };
        let r = s.execute("edit.pasteSvg", &json!({"svg": svg("0.9")})).unwrap();
        let id = ItemId(r["ids"][0].as_u64().unwrap());
        assert!((s.doc().unwrap().doc.item(id).unwrap().opacity - 0.4).abs() < 1e-3);
        assert!(r["warnings"].as_array().unwrap().iter().any(|w| w.as_str().unwrap().contains("fill's opacity")), "{r}");
        let r = s.execute("edit.pasteSvg", &json!({"svg": svg("0.4")})).unwrap();
        let id = ItemId(r["ids"][0].as_u64().unwrap());
        assert!((s.doc().unwrap().doc.item(id).unwrap().opacity - 0.4).abs() < 1e-3);
        assert_eq!(r["warnings"], json!([]), "equal opacities fit the object's");
    }

    /// Issue #2714: a logo's gradient copied in VectorCraft pasted as its first colour.
    #[test]
    fn gradients_paste_as_gradient_swatches_where_they_were() {
        use designcraft_color::{Color, GradientKind, swatch::SwatchValue};
        let mut s = session();
        let svg = r##"<svg xmlns="http://www.w3.org/2000/svg" width="200" height="100">
            <defs><linearGradient id="a"><stop offset="0" stop-color="#ff0000"/><stop offset="0.25" stop-color="#00ff00" stop-opacity="0.5"/><stop offset="1" stop-color="#0000ff"/></linearGradient>
            <radialGradient id="b" gradientUnits="userSpaceOnUse" cx="150" cy="50" r="40"><stop offset="0" stop-color="#fff"/><stop offset="1" stop-color="#000"/></radialGradient></defs>
            <rect x="0" y="0" width="100" height="100" fill="url(#a)" stroke="url(#a)" stroke-width="4"/>
            <circle cx="150" cy="50" r="40" fill="url(#b)" transform="translate(100 0) scale(0.5) translate(-100 0)"/></svg>"##;
        let r = s.execute("edit.pasteSvg", &json!({"svg": svg, "at": [300, 400]})).unwrap();
        assert_eq!(r["warnings"], json!([]), "nothing lost: {r}");
        let st = s.doc().unwrap();
        let ids: Vec<ItemId> = r["ids"].as_array().unwrap().iter().map(|v| ItemId(v.as_u64().unwrap())).collect();
        assert_eq!(ids.len(), 2, "{r}");
        let (rect, circle) = (st.doc.item(ids[0]).unwrap(), st.doc.item(ids[1]).unwrap());
        let gradient = |name: &str| match &st.doc.swatch(name).unwrap().value {
            SwatchValue::Gradient { gradient } => gradient.clone(),
            v => panic!("{name} is {v:?}, not a gradient"),
        };
        // The linear gradient, all three stops, on both fill and stroke: one swatch.
        let g = gradient(&rect.fill.swatch);
        assert_eq!(g.kind, GradientKind::Linear);
        assert_eq!(
            g.stops.iter().map(|s| (s.offset, s.color, s.opacity)).collect::<Vec<_>>(),
            [(0.0, Color::rgb8(255, 0, 0), 1.0), (0.25, Color::rgb8(0, 255, 0), 0.5), (1.0, Color::rgb8(0, 0, 255), 1.0)]
        );
        assert_eq!(rect.stroke.swatch, rect.fill.swatch);
        // Across the square from its left edge to its right (objectBoundingBox), at its top.
        let b = rect.bounds();
        let v = rect.fill.gradient_vector.unwrap();
        let near = |a: f64, b: f64| (a - b).abs() < 0.01;
        assert!(near(v[0], b.x0) && near(v[1], b.y0) && near(v[2], b.x1) && near(v[3], b.y0), "{v:?} on {b:?}");
        assert_eq!(rect.stroke.gradient_vector, rect.fill.gradient_vector);
        // The radial gradient, centred on the circle, with the circle's radius after its
        // transform: 40 px × 0.5 = 15 pt.
        let g = gradient(&circle.fill.swatch);
        assert_eq!(g.kind, GradientKind::Radial);
        assert_eq!(g.stops.len(), 2);
        let b = circle.bounds();
        let v = circle.fill.gradient_vector.unwrap();
        assert!(near(v[0], b.center().x) && near(v[1], b.center().y) && near(v[2] - v[0], 15.0) && near(v[3], v[1]), "{v:?} on {b:?}");
        assert!(near(b.width(), 30.0), "{b:?}");
    }

    /// A non-square objectBoundingBox gradient's colours follow SVG's constant-colour lines, which
    /// a non-uniform scale does not keep perpendicular to the transformed endpoints.
    #[test]
    fn a_diagonal_gradient_on_a_non_square_rectangle_keeps_svg_colour_placement() {
        let mut s = session();
        let svg = r##"<svg xmlns="http://www.w3.org/2000/svg" width="200" height="100">
            <defs><linearGradient id="a" x1="0" y1="0" x2="1" y2="1"><stop offset="0" stop-color="#ff0000"/><stop offset="1" stop-color="#0000ff"/></linearGradient></defs>
            <rect width="200" height="100" fill="url(#a)"/></svg>"##;
        let r = s.execute("edit.pasteSvg", &json!({"svg": svg, "at": [300, 400]})).unwrap();
        assert_eq!(r["warnings"], json!([]), "{r}");
        let st = s.doc().unwrap();
        let it = st.doc.item(ItemId(r["ids"][0].as_u64().unwrap())).unwrap();
        let b = it.bounds();
        let v = it.fill.gradient_vector.unwrap();
        let (dx, dy) = (v[2] - v[0], v[3] - v[1]);
        // The gradient's parameter (0..1) at a point given in box fractions.
        let t = |fx: f64, fy: f64| {
            let (x, y) = (b.x0 + fx * b.width() - v[0], b.y0 + fy * b.height() - v[1]);
            (x * dx + y * dy) / (dx * dx + dy * dy)
        };
        // SVG (objectBoundingBox, (0,0) → (1,1)): t = (x + y) / 2 in box fractions.
        for (fx, fy) in [(0.0, 0.0), (0.5, 0.0), (0.0, 0.5), (1.0, 1.0), (1.0, 0.0), (0.0, 1.0)] {
            let want = (fx + fy) / 2.0;
            assert!((t(fx, fy) - want).abs() < 1e-3, "({fx}, {fy}): {} != {want} ({v:?} on {b:?})", t(fx, fy));
        }
    }

    /// A hostile SVG can reference one large gradient from many paths; the stops must be kept
    /// once, not once per path.
    #[test]
    fn a_gradient_shared_by_many_paths_is_converted_once() {
        let svg = r##"<svg xmlns="http://www.w3.org/2000/svg" width="200" height="100">
            <defs><linearGradient id="a"><stop offset="0" stop-color="#ff0000"/><stop offset="0.5" stop-color="#00ff00"/><stop offset="1" stop-color="#0000ff"/></linearGradient></defs>
            <rect x="0" y="0" width="50" height="50" fill="url(#a)"/>
            <rect x="50" y="0" width="50" height="50" fill="url(#a)"/>
            <rect x="100" y="0" width="50" height="50" fill="url(#a)"/></svg>"##;
        let tree = designcraft_images::svg_tree(svg.as_bytes()).unwrap();
        let mut defs = GradientDefs::default();
        let mut out = Vec::new();
        path_gradients(tree.root(), 0, MAX_DEPTH, &mut defs, &mut out);
        assert_eq!(out.len(), 3);
        let first = out[0][0].as_ref().unwrap().def.clone();
        for entry in &out[1..] {
            let def = &entry[0].as_ref().unwrap().def;
            assert!(std::sync::Arc::ptr_eq(&first, def), "one definition for every path");
        }
        assert_eq!(defs.by_hash.values().map(Vec::len).sum::<usize>(), 1);
    }

    #[test]
    fn text_and_garbage_are_refused_without_an_undo_step() {
        let mut s = session();
        let undo = s.doc().unwrap().history.undo.len();
        for bad in ["just words", "<svg", "<svg xmlns=\"http://www.w3.org/2000/svg\"></svg>"] {
            assert!(s.execute("edit.pasteSvg", &json!({"svg": bad})).is_err(), "{bad}");
        }
        assert_eq!(s.doc().unwrap().history.undo.len(), undo);
        assert!(is_svg_markup(SPIRAL) && !is_svg_markup("R=233 G=40 B=25"));
    }

    #[test]
    fn deeply_nested_groups_do_not_exhaust_the_stack() {
        let mut s = session();
        let before = serde_json::to_value(&s.doc().unwrap().doc).unwrap();
        let undo = s.doc().unwrap().history.undo.len();
        let depth = 2000;
        let svg = format!(
            r#"<svg xmlns="http://www.w3.org/2000/svg" width="10" height="10">{}<rect width="10" height="10"/>{}</svg>"#,
            "<g opacity=\"0.99\">".repeat(depth),
            "</g>".repeat(depth)
        );
        assert!(s.execute("edit.pasteSvg", &json!({"svg": svg})).is_err());
        assert_eq!(serde_json::to_value(&s.doc().unwrap().doc).unwrap(), before);
        assert_eq!(s.doc().unwrap().history.undo.len(), undo);
    }
}
