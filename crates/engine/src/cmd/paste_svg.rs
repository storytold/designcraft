//! Paste vector art copied in another app (Illustrator, VectorCraft, Figma, Inkscape) as editable
//! paths, as InDesign pastes Illustrator art. Those apps put the copied objects on the system
//! clipboard as SVG markup; each path becomes a DesignCraft path with its fill and stroke, groups
//! stay groups, and text comes in as outlines.

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

/// Cap conversion nesting too; the shared image parser bounds XML before usvg runs.
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
    too_deep: bool,
}

fn paint(p: &usvg::Paint, opacity: f32, notes: &mut Notes) -> Option<Paint> {
    let rgb = match p {
        usvg::Paint::Color(c) => [c.red, c.green, c.blue],
        // Gradients and patterns come in as their first colour (InDesign keeps Illustrator
        // gradients; DesignCraft's swatch gradients can't be made from SVG stops yet).
        usvg::Paint::LinearGradient(g) => {
            notes.gradients += 1;
            g.stops().first().map(|s| [s.color().red, s.color().green, s.color().blue])?
        }
        usvg::Paint::RadialGradient(g) => {
            notes.gradients += 1;
            g.stops().first().map(|s| [s.color().red, s.color().green, s.color().blue])?
        }
        usvg::Paint::Pattern(_) => {
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

/// `t` (SVG px space) then px → pt.
fn to_affine(t: usvg::Transform) -> Affine {
    let pt = designcraft_images::PT_PER_PX;
    Affine::scale(pt) * Affine::new([f64::from(t.sx), f64::from(t.ky), f64::from(t.kx), f64::from(t.sy), f64::from(t.tx), f64::from(t.ty)])
}

fn bezpath(data: &usvg::tiny_skia_path::Path, xf: Affine) -> BezPath {
    use usvg::tiny_skia_path::PathSegment as S;
    let p = |q: usvg::tiny_skia_path::Point| xf * Point::new(f64::from(q.x), f64::from(q.y));
    let mut bp = BezPath::new();
    for seg in data.segments() {
        match seg {
            S::MoveTo(a) => bp.move_to(p(a)),
            S::LineTo(a) => bp.line_to(p(a)),
            S::QuadTo(a, b) => bp.quad_to(p(a), p(b)),
            S::CubicTo(a, b, c) => bp.curve_to(p(a), p(b), p(c)),
            S::Close => bp.close_path(),
        }
    }
    bp
}

fn convert(g: &usvg::Group, depth: usize, notes: &mut Notes, out: &mut Vec<Art>) {
    for node in g.children() {
        match node {
            usvg::Node::Path(path) => {
                if !path.is_visible() || path.data().is_empty() {
                    continue;
                }
                let xf = to_affine(path.abs_transform());
                let fill = path.fill().and_then(|f| paint(f.paint(), f.opacity().get(), notes));
                let stroke = path.stroke().and_then(|s| {
                    let c = paint(s.paint(), s.opacity().get(), notes)?;
                    // A stroke scales with the art: its width in points.
                    let scale = xf.determinant().abs().sqrt();
                    let width = round(f64::from(s.width().get()) * scale);
                    let dashes = s.dasharray().map(|d| d.iter().map(|v| round(f64::from(*v) * scale)).collect::<Vec<_>>());
                    let style = StrokeStyle {
                        width,
                        cap: match s.linecap() {
                            usvg::LineCap::Butt => designcraft_doc::Cap::Butt,
                            usvg::LineCap::Round => designcraft_doc::Cap::Round,
                            usvg::LineCap::Square => designcraft_doc::Cap::Projecting,
                        },
                        join: match s.linejoin() {
                            usvg::LineJoin::Round => designcraft_doc::Join::Round,
                            usvg::LineJoin::Bevel => designcraft_doc::Join::Bevel,
                            _ => designcraft_doc::Join::Miter,
                        },
                        miter: f64::from(s.miterlimit().get()),
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
                out.push(Art::Path { path: bezpath(path.data(), xf), fill, stroke, opacity: 1.0 });
            }
            usvg::Node::Group(child) => {
                if depth >= MAX_DEPTH {
                    notes.too_deep = true;
                    continue;
                }
                let mut items = Vec::new();
                convert(child, depth + 1, notes, &mut items);
                // usvg wraps nodes in groups for transforms and opacity; only a group of several
                // objects stays a group. Its opacity goes to what it holds.
                let opacity = child.opacity().get();
                if items.len() > 1 {
                    out.push(Art::Group(items, opacity));
                } else {
                    for mut a in items {
                        a.fade(opacity);
                        out.push(a);
                    }
                }
            }
            // Text arrives as outlines, as Illustrator's Create Outlines would give it.
            usvg::Node::Text(text) => {
                let mut items = Vec::new();
                convert(text.flattened(), depth + 1, notes, &mut items);
                if items.len() > 1 {
                    out.push(Art::Group(items, 1.0));
                } else {
                    out.extend(items);
                }
            }
            usvg::Node::Image(_) => notes.images += 1,
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

/// The swatch holding exactly `rgb`: an existing process colour, else a new unnamed one (hidden
/// from the Swatches panel until Add to Swatches), as InDesign does for pasted colours.
fn swatch_for(d: &mut designcraft_doc::Document, rgb: [u8; 3]) -> String {
    use designcraft_color::swatch::{Swatch, SwatchValue};
    let color = designcraft_color::Color::rgb8(rgb[0], rgb[1], rgb[2]);
    if let Some(w) = d.swatches.iter().find(|w| matches!(&w.value, SwatchValue::Color { color: c, .. } if *c == color)) {
        return w.name.clone();
    }
    let base = format!("R={} G={} B={}", rgb[0], rgb[1], rgb[2]);
    let name = designcraft_doc::Styles::unique_name(|n| d.swatch(n).is_some(), &base);
    d.swatches.push(Swatch {
        name: name.clone(),
        value: SwatchValue::Color { color, color_type: designcraft_color::ColorType::Process },
        locked: false,
        named: false,
        hidden: true,
    });
    name
}

fn build(d: &mut designcraft_doc::Document, art: Vec<Art>, layer: designcraft_doc::LayerId, shift: Affine) -> Vec<Item> {
    let mut out = Vec::new();
    for a in art {
        match a {
            Art::Path { path, fill, stroke, opacity } => {
                let data = PathData::from_bezpath(&(shift * path));
                if data.subpaths.is_empty() {
                    continue;
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
                    Some(f) => Fill::swatch(&swatch_for(d, f.rgb)),
                    None => Fill::none(),
                };
                it.stroke = match stroke {
                    Some((c, s)) => Stroke {
                        swatch: swatch_for(d, c.rgb),
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
                let children = build(d, items, layer, shift);
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
    let mut notes = Notes::default();
    let mut art = Vec::new();
    convert(tree.root(), 0, &mut notes, &mut art);
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
        let items = build(d, art, layer, shift);
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
