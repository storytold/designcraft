//! Paste vector art copied in another app (Illustrator, VectorCraft, Figma, Inkscape) as editable
//! paths, as InDesign pastes Illustrator art. Those apps put the copied objects on the system
//! clipboard as SVG markup; each path becomes a DesignCraft path with its fill and stroke, groups
//! stay groups, and text comes in as outlines.

use std::collections::{HashMap, HashSet};

use craft_svg::tree::document::{Document as SvgDocument, FillRule, LineCap, LineJoin, Node as SvgNode, Paint as SvgPaint, Segment};
use designcraft_doc::{Content, Fill, Item, ItemId, Selection, Shape, Stroke, StrokeType};
use designcraft_geom::{Affine, BezPath, PathData, Point, Rect};
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

/// A converted SVG node, in points. `even_odd`: the path is filled with `fill-rule="evenodd"`.
enum Art {
    Path { path: BezPath, fill: Option<Paint>, even_odd: bool, stroke: Option<(Paint, StrokeStyle)>, opacity: f32 },
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

#[derive(Clone, Copy)]
struct Paint {
    rgb: [u8; 3],
    opacity: f32,
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
    /// Even-odd paths left as they were: their contours cross or touch, or they are too large to check.
    even_odd_kept: usize,
    /// Dashed even-odd paths with a contour reversed (its dashes run the other way).
    dashes_reversed: usize,
    too_deep: bool,
}

fn paint(p: &SvgPaint, notes: &mut Notes) -> Option<Paint> {
    let (rgb, opacity) = match p {
        SvgPaint::Color { rgb, opacity } => (*rgb, *opacity),
        // Gradients and patterns are retained as their first color where available.
        SvgPaint::Gradient { stops, opacity, .. } => {
            notes.gradients += 1;
            (stops.first()?.rgb, *opacity)
        }
        SvgPaint::Pattern { .. } => {
            notes.gradients += 1;
            return None;
        }
    };
    Some(Paint { rgb, opacity })
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

fn convert(nodes: &[SvgNode], notes: &mut Notes, out: &mut Vec<Art>) {
    for node in nodes {
        match node {
            SvgNode::Path(path) => {
                let fill = path.fill.as_ref().and_then(|p| paint(p, notes));
                let stroke = path.stroke.as_ref().and_then(|s| {
                    let c = paint(&s.paint, notes)?;
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
                let even_odd = fill.is_some() && path.fill_rule == FillRule::EvenOdd;
                out.push(Art::Path { path: bezpath(&path.segments), fill, even_odd, stroke, opacity: 1.0 });
            }
            SvgNode::Group { children, opacity, .. } => {
                let mut items = Vec::new();
                convert(children, notes, &mut items);
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
            // Embedded images are left out; the scene's image count gives the warning.
            SvgNode::Image(_) => {}
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
    names: HashSet<String>,
}

impl PasteSwatches {
    /// An RGB colour as a key: equal colours, equal keys (`+ 0.0` makes -0 and 0 one key).
    fn key(r: f32, g: f32, b: f32) -> [u32; 3] {
        [(r + 0.0).to_bits(), (g + 0.0).to_bits(), (b + 0.0).to_bits()]
    }

    fn new(d: &designcraft_doc::Document) -> Self {
        use designcraft_color::swatch::SwatchValue;
        let mut by_rgb = HashMap::new();
        for w in &d.swatches {
            if let SwatchValue::Color { color: designcraft_color::Color::Rgb { r, g, b }, .. } = &w.value {
                // The first swatch of a colour is the one, as a search from the start finds it.
                by_rgb.entry(Self::key(*r, *g, *b)).or_insert_with(|| w.name.clone());
            }
        }
        Self { by_rgb, names: d.swatches.iter().map(|w| w.name.clone()).collect() }
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

/// Flattening tolerance (pt) and limits for checking an even-odd path's contours for crossings.
const CROSS_TOLERANCE: f64 = 0.05;

/// What one paste may spend checking and turning even-odd paths, across all of them: flattened
/// edges, edge comparisons, contour boxes checked and containment work (path elements walked).
struct FillBudget {
    edges: usize,
    tests: usize,
    boxes: usize,
    containment: usize,
}

impl FillBudget {
    fn new() -> Self {
        Self { edges: 200_000, tests: 4_000_000, boxes: 16_000_000, containment: super::paths::MAX_HOLE_TESTS }
    }
}

/// Items fill with the non-zero rule. Turn an even-odd path's contours so that each one nested an
/// odd number of times runs against the contour around it, and the item fills the same area. Each
/// outer contour and the contours inside it take whichever of the two working directions reverses
/// fewer of them, so contours that already fill alike keep their direction. Contours that enclose
/// no area (an open two-point line) fill nothing either way: they are not counted or reversed.
/// Returns how many contours were reversed, or `None`, leaving the path as it was, when contours
/// cross or touch (reversing contours is not checked to fill the same area there) or the paste's
/// budget runs out.
fn even_odd_to_non_zero(data: &mut PathData, budget: &mut FillBudget) -> Option<usize> {
    if designcraft_geom::hit::edges_cross(data, CROSS_TOLERANCE, &mut budget.edges, &mut budget.tests) != Some(false) {
        return None;
    }
    let nest = super::paths::nesting(data, "edit.pasteSvg", &mut budget.containment, &mut budget.boxes).ok()?;
    // Contours to reverse for the directions where every even depth runs clockwise, counted with
    // all the contours per outer contour.
    let turn: Vec<bool> = nest.iter().map(|k| k.fills && k.cw == (k.depth % 2 == 1)).collect();
    let mut per_outer: HashMap<usize, (usize, usize)> = HashMap::new();
    for (k, t) in nest.iter().zip(&turn).filter(|(k, _)| k.fills) {
        let e = per_outer.entry(k.outer).or_default();
        e.0 += usize::from(*t);
        e.1 += 1;
    }
    let mut reversed = 0;
    for ((sp, k), t) in data.subpaths.iter_mut().zip(&nest).zip(turn) {
        let (turned, total) = per_outer.get(&k.outer).copied().unwrap_or_default();
        if k.fills && t != (turned * 2 > total) {
            sp.reverse();
            reversed += 1;
        }
    }
    Some(reversed)
}

#[allow(clippy::too_many_arguments)]
fn build(
    d: &mut designcraft_doc::Document,
    known: &mut PasteSwatches,
    notes: &mut Notes,
    budget: &mut FillBudget,
    art: Vec<Art>,
    layer: designcraft_doc::LayerId,
    shift: Affine,
) -> Vec<Item> {
    let mut out = Vec::new();
    for a in art {
        match a {
            Art::Path { path, fill, even_odd, stroke, opacity } => {
                let mut data = PathData::from_bezpath(&(shift * path));
                if data.subpaths.is_empty() {
                    continue;
                }
                if even_odd {
                    match even_odd_to_non_zero(&mut data, budget) {
                        None => notes.even_odd_kept += 1,
                        Some(reversed) if reversed > 0 && stroke.as_ref().is_some_and(|(_, s)| s.dashes.is_some()) => notes.dashes_reversed += 1,
                        Some(_) => {}
                    }
                }
                let anchors: usize = data.subpaths.iter().map(|s| s.anchors.len()).sum();
                // A straight two-point segment is a graphic line (Line tool); anything else a path.
                let straight = |a: &designcraft_geom::Anchor| a.h_in == a.p && a.h_out == a.p;
                let line = data.subpaths.len() == 1 && anchors == 2 && data.subpaths.iter().all(|s| !s.closed && s.anchors.iter().all(straight));
                // The object's opacity: its own times the paint's. Items have no separate fill and
                // stroke opacity, so a path with both takes the fill's (`Notes::mixed_opacity`).
                let paint = fill.map(|f| f.opacity).or(stroke.as_ref().map(|(c, _)| c.opacity)).unwrap_or(1.0);
                let mut it = Item::new(ItemId(d.alloc()), layer, if line { Shape::GraphicLine } else { Shape::Path }, data);
                it.fill = match fill {
                    Some(f) => Fill::swatch(&swatch_for(d, known, f.rgb)),
                    None => Fill::none(),
                };
                it.stroke = match stroke {
                    Some((c, s)) => Stroke {
                        swatch: swatch_for(d, known, c.rgb),
                        weight: s.width,
                        cap: s.cap,
                        join: s.join,
                        miter_limit: s.miter,
                        kind: s.dashes.map_or(StrokeType::Solid, |pattern| StrokeType::Dashed { pattern }),
                        ..Stroke::default()
                    },
                    None => Stroke::none(),
                };
                it.opacity = (opacity * paint).clamp(0.0, 1.0);
                out.push(it);
            }
            Art::Group(items, opacity) => {
                let children = build(d, known, notes, budget, items, layer, shift);
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
    let mut art = Vec::new();
    convert(&scene.nodes, &mut notes, &mut art);
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
        let items = build(d, &mut known, &mut notes, &mut FillBudget::new(), art, layer, shift);
        if notes.even_odd_kept > 0 {
            warnings.push(format!(
                "{} even-odd path(s) pasted as drawn with the non-zero rule: their contours cross or touch, or could not be checked",
                notes.even_odd_kept
            ));
        }
        if notes.dashes_reversed > 0 {
            warnings.push(format!(
                "{} dashed even-odd path(s) had contours reversed to keep their holes: their dashes run the other way",
                notes.dashes_reversed
            ));
        }
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

    /// Paste `svg` centered on (100, 100) and report whether the pasted path fills the center and a
    /// point inside its outer band, as the item draws (non-zero rule).
    fn fills_center_and_band(s: &mut Session, svg: &str) -> (bool, bool) {
        use designcraft_geom::{FillRule as Rule, hit::fill_contains};
        let r = s.execute("edit.pasteSvg", &json!({"svg": svg, "at": [100, 100]})).unwrap();
        let it = s.doc().unwrap().doc.item(ItemId(r["ids"][0].as_u64().unwrap())).unwrap().clone();
        let bp = it.xf * it.path.to_bezpath();
        (fill_contains(&bp, Rule::NonZero, Point::new(100.0, 100.0)), fill_contains(&bp, Rule::NonZero, Point::new(60.0, 60.0)))
    }

    #[test]
    fn even_odd_holes_stay_holes() {
        let mut s = session();
        // 100 units = 100 pt: the square spans 50..150 and the inner one 80..120. Both subpaths run
        // the same way, so only the even-odd rule leaves the center empty.
        let svg = |rule: &str, inner: &str| {
            format!(
                r##"<svg xmlns="http://www.w3.org/2000/svg" width="100pt" height="100pt" viewBox="0 0 100 100"><path fill="#000" fill-rule="{rule}" d="M0,0H100V100H0Z {inner}"/></svg>"##
            )
        };
        assert_eq!(fills_center_and_band(&mut s, &svg("evenodd", "M30,30H70V70H30Z")), (false, true), "even-odd: a hole");
        assert_eq!(fills_center_and_band(&mut s, &svg("evenodd", "M30,30V70H70V30Z")), (false, true), "even-odd, reversed inner: a hole");
        assert_eq!(fills_center_and_band(&mut s, &svg("nonzero", "M30,30H70V70H30Z")), (true, true), "non-zero, same way: solid");
        assert_eq!(fills_center_and_band(&mut s, &svg("nonzero", "M30,30V70H70V30Z")), (false, true), "non-zero, reversed inner: a hole");
        // An island inside the hole is filled again, as even-odd draws it.
        let island = svg("evenodd", "M20,20H80V80H20Z M40,40H60V60H40Z");
        assert_eq!(fills_center_and_band(&mut s, &island), (true, true), "even-odd island");
        // The hole between the island and the inner square stays empty. (Drawn the same way, the
        // three squares would fill it with the non-zero rule.)
        let r = s.execute("edit.pasteSvg", &json!({"svg": island, "at": [100, 100]})).unwrap();
        let it = s.doc().unwrap().doc.item(ItemId(r["ids"][0].as_u64().unwrap())).unwrap().clone();
        let bp = it.xf * it.path.to_bezpath();
        assert!(
            !designcraft_geom::hit::fill_contains(&bp, designcraft_geom::FillRule::NonZero, Point::new(75.0, 75.0)),
            "the hole around the island"
        );
    }

    /// Paste `d` as one even-odd path centered on (100, 100); the item and the paste's warnings.
    fn paste_even_odd(s: &mut Session, d: &str, extra: &str) -> (Item, Value) {
        let svg = format!(
            r##"<svg xmlns="http://www.w3.org/2000/svg" width="100pt" height="100pt" viewBox="0 0 100 100"><path fill="#000" fill-rule="evenodd" {extra} d="{d}"/></svg>"##
        );
        let r = s.execute("edit.pasteSvg", &json!({"svg": svg, "at": [100, 100]})).unwrap();
        let it = s.doc().unwrap().doc.item(ItemId(r["ids"][0].as_u64().unwrap())).unwrap().clone();
        (it, r["warnings"].clone())
    }

    /// Does the square start at its top left corner going down, as drawn?
    fn starts_down(sp: &designcraft_geom::SubPath) -> bool {
        let v = sp.anchors[1].p - sp.anchors[0].p;
        v.x.abs() < 1e-6 && v.y > 0.0 && sp.anchors[0].p.x < sp.anchors[2].p.x
    }

    fn warns_even_odd(w: &Value) -> bool {
        w.as_array().unwrap().iter().any(|w| w.as_str().unwrap().contains("even-odd"))
    }

    #[test]
    fn even_odd_paths_that_fill_alike_keep_their_contours_direction() {
        let mut s = session();
        // One square drawn against the usual direction, dashed: nothing to turn, so its dashes
        // start where they did.
        let (one, w) = paste_even_odd(&mut s, "M0,0V100H100V0Z", r##"stroke="#000" stroke-dasharray="10 5""##);
        assert_eq!(w, json!([]));
        assert!(starts_down(&one.path.subpaths[0]));
        // A ring whose hole already runs the other way is left as drawn.
        let (ring, _) = paste_even_odd(&mut s, "M0,0V100H100V0Z M30,30H70V70H30Z", "");
        assert!(starts_down(&ring.path.subpaths[0]));
        assert!((ring.path.subpaths[0].area() > 0.0) != (ring.path.subpaths[1].area() > 0.0));
        // Two separate squares drawn opposite ways fill alike under both rules: both kept.
        let (two, w) = paste_even_odd(&mut s, "M0,0V40H40V0Z M60,0H100V40H60Z", r##"stroke="#000" stroke-dasharray="10 5""##);
        assert_eq!(w, json!([]));
        assert!(starts_down(&two.path.subpaths[0]));
        let v = two.path.subpaths[1].anchors[1].p - two.path.subpaths[1].anchors[0].p;
        assert!(v.y.abs() < 1e-6 && v.x > 0.0, "the second square still starts going right");
        // A line inside a square encloses no area: it does not count, so the square keeps its dashes.
        let (lined, w) = paste_even_odd(&mut s, "M0,0V100H100V0Z M30,50H70", r##"stroke="#000" stroke-dasharray="10 5""##);
        assert_eq!(w, json!([]));
        assert!(starts_down(&lined.path.subpaths[0]));
        // A dashed ring drawn one way needs a contour reversed: its dashes change, and the paste says so.
        let (_, w) = paste_even_odd(&mut s, "M0,0V100H100V0Z M30,30V70H70V30Z", r##"stroke="#000" stroke-dasharray="10 5""##);
        assert!(w.as_array().unwrap().iter().any(|w| w.as_str().unwrap().contains("dashes run the other way")), "{w}");
    }

    #[test]
    fn even_odd_paths_with_crossing_contours_are_pasted_as_drawn_with_a_warning() {
        use designcraft_geom::{FillRule as Rule, hit::fill_contains};
        let mut s = session();
        // A plus sign of two bars running opposite ways: the middle is empty under both rules, and
        // turning the bars to run the same way would fill it.
        let (plus, w) = paste_even_odd(&mut s, "M0,40H100V60H0Z M40,0V100H60V0Z", "");
        assert!(warns_even_odd(&w), "{w}");
        assert!(!fill_contains(&(plus.xf * plus.path.to_bezpath()), Rule::NonZero, Point::new(100.0, 100.0)), "the middle stays empty");
        // Too many edge comparisons to check: pasted as drawn, with the warning, not refused.
        let d: String = (0..3000).map(|i| format!("M{a},{a}H{e}V{e}H{a}Z", a = f64::from(i) / 100.0, e = 100.0 - f64::from(i) / 100.0)).collect();
        let (many, w) = paste_even_odd(&mut s, &d, "");
        assert_eq!(many.path.subpaths.len(), 3000);
        assert!(warns_even_odd(&w), "{w}");
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
