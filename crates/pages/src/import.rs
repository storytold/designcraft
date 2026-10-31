//! Building the document: the page setup from the document root (`TP.DocumentArchive`), then the
//! objects placed on each page (its page group), then the body text.

use std::collections::{BTreeSet, HashMap};
use std::sync::Arc;

use designcraft_color::{Color, Swatch, swatch};
use designcraft_doc::build::NewDocument;
use designcraft_doc::item::{Content, Fill, Graphic, Shape, Stroke, TextFrame, TextFrameOptions, VerticalJustification};
use designcraft_doc::page::Margins;
use designcraft_doc::story::{BASIC_PARAGRAPH, ParaFormat};
use designcraft_doc::{Asset, AssetId, Document, Item, ItemId, LayerId, SpreadRef, StoryId};
use designcraft_geom::{Affine, BezPath, PathData, Point, Rect, Unit, shapes, units};

use crate::package::Package;
use crate::proto::Msg;
use crate::style::{self, CharProps, ParaProps, Rgba, ShapeProps};
use crate::{FontLookup, Import, PagesError, Result};

const DOCUMENT: u32 = 10000;
const PAGE_GROUP: u32 = 10010;
const Z_ORDER: u32 = 10015;
const STORAGE: u32 = 2001;
const SHAPE: u32 = 2011;
const IMAGE: u32 = 3005;
const MASK: u32 = 3006;
const GROUP: u32 = 3008;
const TABLE: u32 = 6000;

/// Deepest group nesting followed.
const MAX_DEPTH: usize = 32;
/// Most drawables visited in one import: shared children of groups are visited once per
/// placement, so depth alone doesn't bound the work.
const MAX_VISITS: usize = 200_000;
/// Most pages a document may ask for.
const MAX_PAGES: u64 = 10_000;

/// Where an object sits: its unrotated box and rotation (degrees counter-clockwise), and whether
/// it is flipped horizontally.
#[derive(Clone, Copy, Debug, PartialEq)]
struct Geometry {
    x: f64,
    y: f64,
    w: f64,
    h: f64,
    angle: f64,
    flip: bool,
}

impl Geometry {
    /// A `TSD.GeometryArchive`: position 1, size 2, flags 3 (4: flipped), angle 4.
    fn read(m: Msg) -> Geometry {
        let (x, y) = m.pair(1).unwrap_or((0.0, 0.0));
        let (w, h) = m.pair(2).unwrap_or((0.0, 0.0));
        Geometry { x, y, w: w.max(0.0), h: h.max(0.0), angle: m.f32(4).unwrap_or(0.0), flip: m.uint(3).is_some_and(|f| f & 4 != 0) }
    }

    /// Inner space (0,0)-(w,h) → the parent's space: rotated and flipped about the centre.
    fn xf(&self) -> Affine {
        let c = (self.w / 2.0, self.h / 2.0);
        let mut xf = Affine::translate((self.x, self.y)) * Affine::translate(c) * Affine::rotate(-self.angle.to_radians());
        if self.flip {
            xf *= Affine::scale_non_uniform(-1.0, 1.0);
        }
        xf * Affine::translate((-c.0, -c.1))
    }

    fn rect(&self) -> Rect {
        Rect::new(0.0, 0.0, self.w, self.h)
    }
}

pub(crate) struct Importer<'p> {
    pkg: &'p Package,
    fonts: FontLookup<'p>,
    pub(crate) doc: Document,
    layer: LayerId,
    pub(crate) warnings: BTreeSet<String>,
    para_styles: HashMap<u64, (CharProps, ParaProps)>,
    char_styles: HashMap<u64, CharProps>,
    assets: HashMap<u64, Option<AssetId>>,
    /// Drawables still allowed to be visited ([`MAX_VISITS`]).
    visits_left: usize,
}

pub(crate) fn import(pkg: &Package, fonts: FontLookup) -> Result<Import> {
    let root = pkg.typed(1, DOCUMENT).ok_or_else(|| PagesError::NotPages("no Pages document (a Keynote or Numbers file?)".into()))?;
    let groups: Vec<Msg> = root.reference(3).and_then(|g| pkg.typed(g, PAGE_GROUP)).map(|g| g.msgs(1).collect()).unwrap_or_default();
    let pages = groups.iter().filter_map(|p| p.uint(1)).max().map_or(1, |m| m.saturating_add(1)).min(MAX_PAGES);
    let mut im = Importer {
        pkg,
        fonts,
        doc: Document::new(&new_document(root, usize::try_from(pages).unwrap_or(1))),
        layer: LayerId(0),
        warnings: BTreeSet::new(),
        para_styles: HashMap::new(),
        char_styles: HashMap::new(),
        assets: HashMap::new(),
        visits_left: MAX_VISITS,
    };
    im.layer = im.doc.default_layer();
    // The document's paint order (`TP.DrawablesZOrderArchive`, root field 20), back to front.
    let z: HashMap<u64, usize> = root
        .reference(20)
        .and_then(|z| pkg.typed(z, Z_ORDER))
        .map(|z| z.msgs(1).filter_map(|r| r.uint(1)).enumerate().map(|(i, id)| (id, i)).collect())
        .unwrap_or_default();
    for g in groups {
        let Some(page) = g.uint(1).and_then(|p| usize::try_from(p).ok()) else { continue };
        let Some((si, pi)) = im.doc.page_loc(page) else { continue };
        let x = im.doc.spreads.get(si).and_then(|s| s.pages.get(pi)).map_or(0.0, |p| p.x);
        // Drawables the order doesn't list stay above those it does, in page-group order.
        let mut ids: Vec<u64> = g.msgs(4).filter_map(|o| o.reference(1)).collect();
        ids.sort_by_key(|id| z.get(id).copied().unwrap_or(usize::MAX));
        for id in ids {
            if let Some(mut item) = im.drawable(id, 0) {
                item.xf = Affine::translate((x, 0.0)) * item.xf;
                im.doc.insert_item(SpreadRef::Doc(si), item, None).map_err(|e| PagesError::Invalid(e.to_string()))?;
            }
        }
    }
    if let Some(body) = root.reference(4).and_then(|b| pkg.typed(b, STORAGE)) {
        im.body_text(body)?;
    }
    Ok(Import { document: im.doc, warnings: im.warnings.into_iter().collect() })
}

/// The page setup: size and margins (`TP.DocumentArchive` 30–35: width, height, left, right,
/// top, bottom margins).
fn new_document(root: Msg, pages: usize) -> NewDocument {
    let f = |n: u32, d: f64| root.f32(n).filter(|v| *v >= 0.0).unwrap_or(d);
    let (width, height) = (f(30, 612.0).max(1.0), f(31, 792.0).max(1.0));
    let mm = |v: f64| v * units::PT_PER_MM;
    let metric = [(210.0, 297.0), (148.0, 210.0), (297.0, 420.0), (176.0, 250.0)]
        .iter()
        .any(|&(a, b)| [(mm(a), mm(b)), (mm(b), mm(a))].iter().any(|&(w, h)| (w - width).abs() < 1.0 && (h - height).abs() < 1.0));
    NewDocument {
        title: String::new(),
        width,
        height,
        pages,
        facing_pages: false,
        margins: Margins { top: f(34, 72.0), bottom: f(35, 72.0), inside: f(32, 72.0), outside: f(33, 72.0) },
        units: if metric { Unit::Millimeters } else { Unit::Inches },
        ..Default::default()
    }
}

/// The path of a shape's path source (`TSD.PathSourceArchive`), in a box of `w` × `h`, and the
/// kind of shape it is. Bezier paths (5) are scaled from their natural size (2); regular polygons
/// (4, type 1) are drawn in the box; other kinds are approximated by the box.
fn path_source(m: Option<Msg>, w: f64, h: f64, warnings: &mut BTreeSet<String>) -> (PathData, Shape) {
    let rect = || (shapes::rectangle(Rect::new(0.0, 0.0, w, h)), Shape::Rectangle);
    let Some(m) = m else { return rect() };
    if let Some(b) = m.msg(5) {
        let (nw, nh) = b.pair(2).unwrap_or((w, h));
        let (sx, sy) = (if nw > 0.0 { w / nw } else { 1.0 }, if nh > 0.0 { h / nh } else { 1.0 });
        let Some(bp) = b.msg(3).map(|p| bezier(p, sx, sy)).filter(|bp| !bp.elements().is_empty()) else { return rect() };
        return (PathData::from_bezpath(&bp), Shape::Path);
    }
    if let Some(s) = m.msg(4) {
        if s.uint(1) == Some(1) {
            let sides = s.f32(2).map_or(5, |v| v.round().clamp(3.0, 1000.0) as u32);
            return (polygon(w, h, sides), Shape::Polygon);
        }
        // Rounded corners are not read yet: a plain rectangle.
        return rect();
    }
    warnings.insert("some shapes (arrows, stars, callouts, freehand drawings) became rectangles".into());
    rect()
}

/// A `TSP.Path`: elements in 1, each a type (1 move, 2 line, 4 curve, 5 close) and points in 2.
fn bezier(p: Msg, sx: f64, sy: f64) -> BezPath {
    let mut bp = BezPath::new();
    let mut open = false;
    for e in p.msgs(1) {
        let pts: Vec<Point> = e.msgs(2).map(|q| Point::new(q.f32(1).unwrap_or(0.0) * sx, q.f32(2).unwrap_or(0.0) * sy)).collect();
        match (e.uint(1), pts.as_slice()) {
            (Some(1), [p, ..]) => {
                bp.move_to(*p);
                open = true;
            }
            (Some(2), [p, ..]) if open => bp.line_to(*p),
            (Some(4), [a, b, c, ..]) if open => bp.curve_to(*a, *b, *c),
            (Some(5), _) if open => {
                bp.close_path();
                open = false;
            }
            _ => {}
        }
    }
    bp
}

/// A regular polygon filling a `w` × `h` box, first vertex at the top.
fn polygon(w: f64, h: f64, sides: u32) -> PathData {
    let mut bp = BezPath::new();
    for i in 0..sides {
        let a = std::f64::consts::TAU * f64::from(i) / f64::from(sides);
        let p = Point::new(w / 2.0 * (1.0 + a.sin()), h / 2.0 * (1.0 - a.cos()));
        if i == 0 { bp.move_to(p) } else { bp.line_to(p) }
    }
    bp.close_path();
    PathData::from_bezpath(&bp)
}

/// The words of a PostScript name's family part: `HelveticaNeue` → `Helvetica Neue`.
fn split_words(s: &str) -> String {
    let mut out = String::with_capacity(s.len() + 4);
    let mut prev: Option<char> = None;
    for c in s.chars() {
        if c.is_uppercase() && prev.is_some_and(char::is_lowercase) {
            out.push(' ');
        }
        out.push(c);
        prev = Some(c);
    }
    out
}

impl<'p> Importer<'p> {
    pub(crate) fn paragraph_style(&mut self, id: u64) -> (CharProps, ParaProps) {
        let pkg = self.pkg;
        self.para_styles.entry(id).or_insert_with(|| style::paragraph_style(pkg, id)).clone()
    }

    pub(crate) fn character_style(&mut self, id: u64) -> CharProps {
        let pkg = self.pkg;
        self.char_styles.entry(id).or_insert_with(|| style::character_style(pkg, id)).clone()
    }

    /// Family and style of the font with PostScript name `ps`, made bold or italic when the text
    /// asks for it and the font is the regular face.
    pub(crate) fn font(&mut self, ps: &str, bold: bool, italic: bool) -> (String, String) {
        let (family, style) = (self.fonts)(ps).unwrap_or_else(|| match ps.split_once('-') {
            Some((f, s)) => (split_words(f), split_words(s)),
            None => (split_words(ps), "Regular".into()),
        });
        let style = match (style.as_str(), bold, italic) {
            ("Regular" | "Roman", true, true) => "Bold Italic".into(),
            ("Regular" | "Roman", true, false) => "Bold".into(),
            ("Regular" | "Roman", false, true) => "Italic".into(),
            ("Italic", true, _) => "Bold Italic".into(),
            _ => style,
        };
        (family, style)
    }

    /// The swatch of an sRGB colour, added to the document the first time it is used.
    pub(crate) fn swatch(&mut self, [r, g, b, a]: Rgba) -> String {
        if a <= 0.0 {
            return swatch::NONE.into();
        }
        let c = Color::rgb(r as f32, g as f32, b as f32);
        let name = swatch::value_name(c);
        if !self.doc.swatches.iter().any(|s| s.name == name) {
            let mut s = Swatch::color(&name, c);
            s.named = false;
            self.doc.swatches.push(s);
        }
        name
    }

    fn new_item(&mut self, shape: Shape, path: PathData, xf: Affine) -> Item {
        let mut item = Item::new(ItemId(self.doc.alloc()), self.layer, shape, path);
        item.xf = xf;
        item.fill = Fill::none();
        item.stroke = Stroke::none();
        item
    }

    fn apply_style(&mut self, item: &mut Item, s: &ShapeProps) {
        if let Some(Some(c)) = s.fill {
            item.fill = Fill::swatch(&self.swatch(c));
        }
        if let Some(Some((c, w))) = s.stroke {
            item.stroke = Stroke { swatch: self.swatch(c), weight: w, ..Stroke::none() };
        }
        if let Some(o) = s.opacity {
            item.opacity = o as f32;
        }
        if s.gradient_or_image_fill {
            self.warnings.insert("gradient and image fills show a solid colour".into());
        }
        if s.shadow {
            self.warnings.insert("shadows are not imported yet".into());
        }
    }

    /// The page item for drawable `id`, or None (with a warning) for what isn't read.
    fn drawable(&mut self, id: u64, depth: usize) -> Option<Item> {
        if depth > MAX_DEPTH {
            return None;
        }
        let Some(left) = self.visits_left.checked_sub(1) else {
            self.warnings.insert(format!("only the first {MAX_VISITS} objects were imported"));
            return None;
        };
        self.visits_left = left;
        let ty = self.pkg.type_of(id)?;
        let m = self.pkg.object(id)?;
        match ty {
            SHAPE => self.shape(m),
            IMAGE => self.image(m),
            GROUP => self.group(m, depth),
            TABLE => {
                self.warnings.insert("tables are not imported yet".into());
                None
            }
            other => {
                self.warnings.insert(format!("objects of type {other} are not imported yet"));
                None
            }
        }
    }

    /// A shape (`TSWP.ShapeInfoArchive`: shape in 1, text storage in 2). Placement in
    /// shape.1.1, graphic style in shape.2, path source in shape.3.
    fn shape(&mut self, m: Msg) -> Option<Item> {
        let shape = m.msg(1)?;
        let geo = Geometry::read(shape.msg(1)?.msg(1)?);
        let (path, kind) = path_source(shape.msg(3), geo.w, geo.h, &mut self.warnings);
        let mut item = self.new_item(kind, path, geo.xf());
        let props = shape.reference(2).map(|s| style::graphic_style(self.pkg, s)).unwrap_or_default();
        self.apply_style(&mut item, &props);
        let storage = m.reference(2).and_then(|s| self.pkg.typed(s, STORAGE));
        if let Some(storage) = storage {
            let sid = StoryId(self.doc.alloc());
            let mut story = self.story(sid, storage);
            if !story.text.trim().is_empty() {
                story.frames.push(item.id);
                story.direction = self.doc.new_story_direction();
                self.doc.stories.insert(sid, Arc::new(story));
                let [l, t, r, b] = props.padding.unwrap_or([4.0; 4]);
                let options = TextFrameOptions {
                    inset: [t, l, b, r],
                    vertical_justification: match props.valign {
                        Some(1) => VerticalJustification::Center,
                        Some(2) => VerticalJustification::Bottom,
                        _ => VerticalJustification::Top,
                    },
                    ..Default::default()
                };
                item.content = Content::Text(TextFrame { story: sid, options });
            }
        }
        Some(item)
    }

    /// An image (`TSD.ImageArchive`): placement in 1.1, mask in 5, data in 15 (adjusted), 13, 11
    /// (original) or 12 (thumbnail).
    fn image(&mut self, m: Msg) -> Option<Item> {
        let geo = Geometry::read(m.msg(1)?.msg(1)?);
        let data_id = [15, 13, 11, 12].into_iter().find_map(|f| m.reference(f).filter(|d| self.pkg.data(*d).is_some()));
        let Some(asset) = data_id.and_then(|d| self.asset(d)) else {
            self.warnings.insert("some images are missing from the package or in a format DesignCraft can't place".into());
            return None;
        };
        let Some(natural) = self.doc.assets.get(&asset).and_then(|a| designcraft_images::natural_size(&a.data)).filter(|(w, h)| *w > 0.0 && *h > 0.0)
        else {
            self.warnings.insert("some images (such as PDFs) have no size DesignCraft can read and were skipped".into());
            return None;
        };
        // The mask (a drawable over the image, in its space) is the frame; without one the frame
        // is the image's box.
        let (path, kind) = match m.reference(5).and_then(|k| self.pkg.typed(k, MASK)) {
            Some(mask) => {
                let mg = mask.msg(1).and_then(|d| d.msg(1)).map(Geometry::read).unwrap_or(geo);
                let (p, kind) = path_source(mask.msg(2), mg.w, mg.h, &mut self.warnings);
                // A mask's path keeps the coordinates it was drawn at (a resized image's mask
                // still has its first size), so its bounds are stretched over the mask's box.
                let mut bp = p.to_bezpath();
                let b = designcraft_geom::Shape::bounding_box(&bp);
                if b.width() > 1e-6 && b.height() > 1e-6 {
                    bp.apply_affine(Affine::scale_non_uniform(mg.w / b.width(), mg.h / b.height()) * Affine::translate((-b.x0, -b.y0)));
                }
                bp.apply_affine(mg.xf());
                (PathData::from_bezpath(&bp), kind)
            }
            None => (shapes::rectangle(geo.rect()), Shape::Rectangle),
        };
        let mut item = self.new_item(kind, path, geo.xf());
        if let Some(o) = m.reference(3).and_then(|s| style::media_opacity(self.pkg, s)) {
            item.opacity = o as f32;
        }
        item.content = Content::Graphic(Graphic {
            layer_options: None,
            asset,
            size: natural,
            xf: Affine::scale_non_uniform(geo.w / natural.0, geo.h / natural.1),
            clip: None,
            auto_fit: Default::default(),
            fit_align: 4,
            crop: [0.0; 4],
            wrap: Default::default(),
            fill: Fill::none(),
        });
        Some(item)
    }

    /// The asset of data file `id`, placed once however often it is used.
    fn asset(&mut self, id: u64) -> Option<AssetId> {
        if let Some(a) = self.assets.get(&id) {
            return *a;
        }
        let (bytes, name) = self.pkg.data(id)?;
        let mime = designcraft_images::mime(bytes);
        let placeable = mime.starts_with("image/") || mime == "application/pdf";
        let asset = placeable.then(|| {
            let aid = AssetId(self.doc.alloc());
            let asset = Asset {
                id: aid,
                name: name.to_string(),
                mime: mime.to_string(),
                data: Arc::new(bytes.to_vec()),
                pixels: designcraft_images::pixel_size(bytes),
                ..Default::default()
            };
            self.doc.assets.insert(aid, Arc::new(asset));
            aid
        });
        self.assets.insert(id, asset);
        asset
    }

    /// A group (`TSD.GroupArchive`: placement in 1.1, children in 2, in the group's space).
    fn group(&mut self, m: Msg, depth: usize) -> Option<Item> {
        let geo = m.msg(1).and_then(|d| d.msg(1)).map(Geometry::read)?;
        let children: Vec<Arc<Item>> = m.msgs(2).filter_map(|r| r.uint(1)).filter_map(|c| self.drawable(c, depth + 1)).map(Arc::new).collect();
        if children.is_empty() {
            return None;
        }
        let mut item = self.new_item(Shape::Group, PathData::default(), geo.xf());
        item.content = Content::Group { items: children };
        Some(item)
    }

    /// Body text (word-processing documents): one frame in the first page's margins.
    fn body_text(&mut self, body: Msg) -> Result<()> {
        let sid = StoryId(self.doc.alloc());
        let mut story = self.story(sid, body);
        if story.text.trim().is_empty() {
            return Ok(());
        }
        let Some((si, pi)) = self.doc.page_loc(0) else { return Ok(()) };
        let Some(page) = self.doc.spreads.get(si).and_then(|s| s.pages.get(pi)) else { return Ok(()) };
        let r = page.margin_rect();
        let (fid, empty) = self
            .doc
            .add_text_frame(SpreadRef::Doc(si), r, self.layer, "", ParaFormat { style: BASIC_PARAGRAPH.into(), ..Default::default() })
            .map_err(|e| PagesError::Invalid(e.to_string()))?;
        self.doc.stories.remove(&empty);
        story.frames.push(fid);
        if let Some(item) = self.doc.item_mut(fid) {
            item.content = Content::Text(TextFrame { story: sid, options: TextFrameOptions::default() });
        }
        self.doc.stories.insert(sid, Arc::new(story));
        self.warnings.insert("the body text is in one frame on the first page; thread more frames to show all of it".into());
        Ok(())
    }
}
