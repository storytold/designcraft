//! Affinity document → DesignCraft document (DesignCraft's own mapping; the reader in the other
//! modules is shared with VectorCraft and PhotoCraft).
//!
//! Publisher pages become DesignCraft pages, one spread per Affinity spread. Documents without
//! pages (Designer, Photo) get a page per artboard, or one page for the canvas. Top-level Affinity
//! layers become document layers by name; nested layers and groups become groups. Shapes become
//! path frames, text becomes text frames with one story each, and images become graphic frames.
//! Colours stay in their own space (CMYK stays CMYK) as unnamed colours. Everything the reader
//! or this mapping can't represent is listed in [`Imported::warnings`].

use std::collections::{BTreeMap, HashMap};
use std::sync::Arc;

use designcraft_color::{BlendMode, Color, ColorType, Gradient, GradientKind, GradientStop, Swatch, SwatchValue, swatch};
use designcraft_doc::build::NewDocument;
use designcraft_doc::{
    Align, Cap, Capitalization, CharAttrs, CharFormat, Content, Document, Fill, FirstBaseline, Graphic, Intent, Item, Join, LAYER_COLORS, Layer,
    LayerId, Leading, Margins, NO_CHAR_STYLE, Page, PageId, PageSide, Shape, Spread, SpreadId, Story, StoryId, Stroke, StrokeAlign, StrokeType,
    TextFrame, TextFrameOptions,
};
use designcraft_doc::{AssetId, Cell, CellStroke, ItemId, Row, RowHeightMode, RowKind, Table, TableOptions, VerticalJustification};
use designcraft_geom::{Affine, BezPath, PathData, Point, Rect, Unit};

use crate::model::{self, Kind, Node, Pixels};
use crate::paint::{self, Paint};
use crate::{Error, Limits};

/// An imported document and what could not be imported faithfully, one line per kind.
#[derive(Debug)]
pub struct Imported {
    pub document: Document,
    pub warnings: Vec<String>,
}

/// Read an Affinity document (`.afpub`, `.af`, `.afdesign`, `.afphoto`) as a DesignCraft document.
pub fn import(bytes: &[u8]) -> Result<Imported, Error> {
    let src = model::read(bytes, Limits::default())?;
    Builder::new(&src).build(&src)
}

/// Largest page side DesignCraft accepts (216 in).
const MAX_PAGE: f64 = 15552.0;
/// Nested groups followed; the reader already caps nesting at 128 levels.
const MAX_DEPTH: usize = 160;

/// A DesignCraft spread to build: its origin and pages in Affinity document pixels, and the
/// top-level nodes placed on it.
struct PageSet<'a> {
    origin: model::Point,
    pages: Vec<model::Rect>,
    nodes: Vec<&'a Node>,
}

/// A text or table node's transform split into DesignCraft's terms: `to_frame` scales node space
/// to points, `place` rotates and moves that frame space onto the spread.
struct Placement {
    to_frame: Affine,
    place: Affine,
    rotated: bool,
    /// How much font sizes grow (the vertical scale), and the horizontal scale on top of that.
    size_scale: f64,
    h_scale: f64,
}

struct Builder {
    doc: Document,
    /// Document pixels → points.
    scale: f64,
    /// DesignCraft parents made from Affinity master pages, by (master spread, its page).
    parents: HashMap<(usize, usize), Option<SpreadId>>,
    /// Top-level Affinity layers by name.
    layers: HashMap<String, LayerId>,
    /// Swatch names by colour, for reuse.
    colors: Vec<(Color, String)>,
    warnings: BTreeMap<String, usize>,
    images: usize,
    /// Fonts' line heights in em, by family and style.
    line_heights: HashMap<(String, String), f64>,
}

impl Builder {
    fn new(src: &model::Document) -> Self {
        let scale = 72.0 / src.dpi;
        let first = src.spreads.iter().flat_map(|s| s.pages.first().copied().or(Some(s.bounds))).next();
        let (w, h) =
            first.map(|r| ((r.x1 - r.x0) * scale, (r.y1 - r.y0) * scale)).filter(|(w, h)| valid_size(*w) && valid_size(*h)).unwrap_or((595.0, 842.0));
        let nd = NewDocument {
            title: "Untitled".into(),
            intent: Intent::Print,
            width: w,
            height: h,
            pages: 1,
            facing_pages: src.spreads.iter().any(|s| s.pages.len() == 2),
            columns: 1,
            margins: Margins::uniform(0.0),
            units: Unit::Millimeters,
            ..NewDocument::default()
        };
        let mut doc = Document::new(&nd);
        // Parents come from the Affinity masters the pages use (see `parent_for`).
        doc.parents.clear();
        Self {
            doc,
            scale,
            parents: HashMap::new(),
            layers: HashMap::new(),
            colors: Vec::new(),
            warnings: BTreeMap::new(),
            images: 0,
            line_heights: HashMap::new(),
        }
    }

    fn warn(&mut self, what: &str) {
        *self.warnings.entry(what.to_string()).or_default() += 1;
    }

    fn build(mut self, src: &model::Document) -> Result<Imported, Error> {
        let default_layer = self.doc.default_layer();
        let mut spreads = Vec::new();
        let sets = self.page_sets(src);
        let facing = self.doc.settings.facing_pages;
        for (si, set) in sets.iter().enumerate() {
            let to_spread = Affine::scale(self.scale) * Affine::translate((-set.origin.x, -set.origin.y));
            let mut pages = Vec::new();
            let count = set.pages.len();
            let instances = master_instances(set.nodes.iter().copied());
            for (pi, r) in set.pages.iter().enumerate() {
                // The master page Affinity shows on this page, as a DesignCraft parent.
                let parent = instances
                    .iter()
                    .find(|(_, page, _, n)| pi >= *page && pi < page.saturating_add(*n))
                    .and_then(|(master, page, master_page, _)| self.parent_for(src, *master, master_page + (pi - page), default_layer));
                let (w, h) = ((r.x1 - r.x0) * self.scale, (r.y1 - r.y0) * self.scale);
                if !valid_size(w) || !valid_size(h) {
                    self.warn("pages larger than 216 in or without a size (left out)");
                    continue;
                }
                if (r.y0 - set.origin.y).abs() * self.scale > 0.5 {
                    self.warn("pages of a spread not side by side (aligned at the top)");
                }
                let side = match (facing, count, pi) {
                    (false, ..) => PageSide::Single,
                    (true, 2, 0) => PageSide::Left,
                    (true, 2, _) => PageSide::Right,
                    (true, 1, _) if si == 0 => PageSide::Right,
                    (true, 1, _) => PageSide::Left,
                    _ => PageSide::Single,
                };
                pages.push(Page {
                    id: PageId(self.doc.alloc()),
                    width: w,
                    height: h,
                    x: (r.x0 - set.origin.x) * self.scale,
                    margins: Margins::uniform(0.0),
                    columns: Default::default(),
                    parent,
                    side,
                    overridden: vec![],
                    guides: vec![],
                    show_parent_items: true,
                    liquid: Default::default(),
                    transition: None,
                    view_rotation: 0,
                });
            }
            if pages.is_empty() {
                continue;
            }
            let mut items = Vec::new();
            for node in &set.nodes {
                self.top_level(node, to_spread, default_layer, &mut items);
            }
            spreads.push(Arc::new(Spread {
                id: SpreadId(self.doc.alloc()),
                pages,
                items: items.into_iter().map(Arc::new).collect(),
                parent: None,
                allow_shuffle: true,
            }));
        }
        if spreads.is_empty() {
            if self.warnings.keys().any(|w| w.starts_with("pages larger than")) {
                return Err(Error::Unsupported("its pages are larger than 216 in, DesignCraft's largest page"));
            }
            return Err(Error::Malformed("document has no pages"));
        }
        self.doc.spreads = spreads;
        if let Some(p) = self.doc.spreads.first().and_then(|s| s.pages.first()) {
            self.doc.settings.page_width = p.width;
            self.doc.settings.page_height = p.height;
        }
        // The new document's own layer goes when every object came in on an Affinity layer.
        let used = |d: &Document, id: LayerId| d.spreads.iter().any(|s| s.items.iter().any(|i| i.layer == id));
        if self.doc.layers.len() > 1 && !used(&self.doc, default_layer) {
            self.doc.layers.retain(|l| l.id != default_layer);
        }
        self.doc.check().map_err(|_| Error::Malformed("the document's structure could not be mapped"))?;
        let mut warnings: Vec<String> = src.warnings.clone();
        warnings.extend(self.warnings.into_iter().map(|(w, n)| if n > 1 { format!("{w} ({n}×)") } else { w }));
        Ok(Imported { document: self.doc, warnings })
    }

    /// The spreads to build: Publisher spreads as they are; otherwise a page per artboard, or the
    /// canvas as one page.
    fn page_sets<'a>(&mut self, src: &'a model::Document) -> Vec<PageSet<'a>> {
        let mut sets = Vec::new();
        for spread in &src.spreads {
            if !spread.pages.is_empty() {
                let origin = spread
                    .pages
                    .iter()
                    .fold(model::Point { x: f64::INFINITY, y: f64::INFINITY }, |o, r| model::Point { x: o.x.min(r.x0), y: o.y.min(r.y0) });
                sets.push(PageSet { origin, pages: spread.pages.clone(), nodes: spread.nodes.iter().collect() });
                continue;
            }
            let first = sets.len();
            let mut loose = Vec::new();
            for node in &spread.nodes {
                match &node.kind {
                    Kind::Artboard { rect, .. } => {
                        sets.push(PageSet { origin: model::Point { x: rect.x0, y: rect.y0 }, pages: vec![*rect], nodes: vec![node] })
                    }
                    _ => loose.push(node),
                }
            }
            if sets.len() == first {
                let b = spread.bounds;
                sets.push(PageSet { origin: model::Point { x: b.x0, y: b.y0 }, pages: vec![b], nodes: loose });
            } else if !loose.is_empty() {
                self.warn("objects outside artboards (placed with the first artboard)");
                if let Some(set) = sets.get_mut(first) {
                    // Behind the artboard, as they were on the canvas.
                    let boards = std::mem::take(&mut set.nodes);
                    set.nodes = loose.into_iter().chain(boards).collect();
                }
            }
        }
        sets
    }

    /// The DesignCraft parent showing page `page` of Affinity master spread `master`, made the
    /// first time a page uses it: one page, with the master's objects on that page.
    fn parent_for(&mut self, src: &model::Document, master: usize, page: usize, default_layer: LayerId) -> Option<SpreadId> {
        if let Some(id) = self.parents.get(&(master, page)) {
            return *id;
        }
        self.parents.insert((master, page), None);
        let page_index = page;
        let m = src.masters.get(master)?;
        let r = m.pages.get(page).or_else(|| m.pages.last()).copied().unwrap_or(m.bounds);
        let (w, h) = ((r.x1 - r.x0) * self.scale, (r.y1 - r.y0) * self.scale);
        if !valid_size(w) || !valid_size(h) {
            return None;
        }
        if !master_instances(&m.nodes).is_empty() {
            self.warn("master pages based on other master pages (only their own objects are used)");
        }
        let to_spread = Affine::scale(self.scale) * Affine::translate((-r.x0, -r.y0));
        let mut items = Vec::new();
        for node in &m.nodes {
            self.top_level(node, to_spread, default_layer, &mut items);
        }
        // A master spread's other page draws its own objects.
        let area = Rect::new(0.0, 0.0, w, h);
        items.retain(|it| {
            let b = it.bounds();
            b.x1 > area.x0 && b.x0 < area.x1 && b.y1 > area.y0 && b.y0 < area.y1
        });
        let index = self.doc.parents.len();
        let prefix = parent_prefix(index);
        let id = SpreadId(self.doc.alloc());
        let page = Page {
            id: PageId(self.doc.alloc()),
            width: w,
            height: h,
            x: 0.0,
            margins: Margins::uniform(0.0),
            columns: Default::default(),
            parent: None,
            side: PageSide::Single,
            overridden: vec![],
            guides: vec![],
            show_parent_items: true,
            liquid: Default::default(),
            transition: None,
            view_rotation: 0,
        };
        let name = if m.pages.len() > 1 { format!("Master {} page {}", master + 1, page_index + 1) } else { format!("Master {}", master + 1) };
        self.doc.parents.push(Arc::new(Spread {
            id,
            pages: vec![page],
            items: items.into_iter().map(Arc::new).collect(),
            parent: Some(designcraft_doc::ParentInfo { prefix, name, based_on: None }),
            allow_shuffle: true,
        }));
        self.parents.insert((master, page_index), Some(id));
        Some(id)
    }

    /// A node directly on a spread: Affinity layers become document layers, their content items.
    fn top_level(&mut self, node: &Node, xf: Affine, default_layer: LayerId, out: &mut Vec<Item>) {
        let plain = node.opacity >= 1.0
            && matches!(node.blend, model::Blend::Normal | model::Blend::PassThrough)
            && node.mask.is_none()
            && node.pixel_mask.is_none();
        if matches!(node.kind, Kind::Layer) && plain {
            let layer = self.layer(&node.name);
            for child in &node.children {
                if let Some(mut it) = self.item(child, xf, layer, 1) {
                    it.hidden |= !node.visible;
                    it.locked |= node.locked;
                    out.push(it);
                }
            }
            return;
        }
        if let Some(it) = self.item(node, xf, default_layer, 0) {
            out.push(it);
        }
    }

    fn layer(&mut self, name: &str) -> LayerId {
        let name = if name.trim().is_empty() { "Layer" } else { name.trim() };
        if let Some(id) = self.layers.get(name) {
            return *id;
        }
        let id = LayerId(self.doc.alloc());
        let color = LAYER_COLORS.get(self.layers.len() % LAYER_COLORS.len().max(1)).map_or([43, 155, 255], |c| c.1);
        // Affinity lists layers bottom to top; DesignCraft's first layer is the frontmost.
        self.doc.layers.insert(
            0,
            Layer {
                id,
                name: name.to_string(),
                color,
                visible: true,
                locked: false,
                printable: true,
                show_guides: true,
                suppress_wrap_when_hidden: false,
            },
        );
        self.layers.insert(name.to_string(), id);
        id
    }

    fn new_item(&mut self, layer: LayerId, shape: Shape, path: PathData) -> Item {
        Item::new(ItemId(self.doc.alloc()), layer, shape, path)
    }

    /// One node and its children. `xf` maps Affinity document pixels to spread points.
    fn item(&mut self, node: &Node, xf: Affine, layer: LayerId, depth: usize) -> Option<Item> {
        if depth > MAX_DEPTH {
            self.warn("objects nested too deeply (left out)");
            return None;
        }
        let mut it = match &node.kind {
            Kind::Layer | Kind::Group | Kind::Unsupported => {
                let kids = self.children(node, xf, layer, depth)?;
                let mut g = self.new_item(layer, Shape::Group, PathData::default());
                g.content = Content::Group { items: kids };
                g
            }
            Kind::Shape { path, fills, strokes, even_odd } => {
                let mut it = self.shape(path, *even_odd, fills, strokes, xf, layer, !node.children.is_empty())?;
                if !node.children.is_empty() {
                    // Children of a shape are clipped by it: DesignCraft's frame with pasted-into items.
                    if let Some(kids) = self.children(node, xf, layer, depth) {
                        it.content = Content::Group { items: kids };
                    }
                }
                it
            }
            Kind::Artboard { path, even_odd, background, strokes, .. } => {
                let mut it = self.shape(path, *even_odd, background, strokes, xf, layer, true)?;
                it.shape = Shape::Rectangle;
                if let Some(kids) = self.children(node, xf, layer, depth) {
                    it.content = Content::Group { items: kids };
                }
                it
            }
            Kind::Text(t) => self.text(t, xf, layer)?,
            Kind::Table(t) => self.table(t, xf, layer)?,
            Kind::Image(img) => self.image_node(node, img, xf, layer, depth, &[])?,
            // Shown through the page's parent (see `parent_for`).
            Kind::MasterInstance { .. } => return None,
        };
        if matches!(node.kind, Kind::Text(_) | Kind::Table(_)) && !node.children.is_empty() {
            self.warn("objects inside text (left out)");
        }
        self.node_props(node, &mut it);
        // An image's pixel mask is part of its pixels (see `image_node`).
        if node.pixel_mask.is_some() && !matches!(node.kind, Kind::Image(_)) {
            self.warn("pixel masks on objects other than images (imported without them)");
        }
        match &node.mask {
            Some(mask) => {
                let path = self.path_data(mask, xf, false);
                if path.is_empty() {
                    return Some(it);
                }
                let mut clip = self.new_item(layer, Shape::Path, path);
                clip.content = Content::Group { items: vec![Arc::new(it)] };
                Some(clip)
            }
            None => Some(it),
        }
    }

    /// Name, visibility, lock, opacity and blend mode of `node` onto `it`.
    fn node_props(&mut self, node: &Node, it: &mut Item) {
        self.effects(&node.effects, it);
        it.name = node.name.clone();
        it.hidden |= !node.visible;
        it.locked |= node.locked;
        it.opacity = (f64::from(it.opacity) * node.opacity.clamp(0.0, 1.0)) as f32;
        it.blend = self.blend(node.blend);
    }

    /// An image node with its pixel mask applied, and the images inside it, which Affinity clips
    /// to its pixels: each is masked by everything that masks its parent and by the parent's
    /// own transparency. `inherited` are the masks of the enclosing images.
    fn image_node(&mut self, node: &Node, img: &model::Image, xf: Affine, layer: LayerId, depth: usize, inherited: &[AlphaMask]) -> Option<Item> {
        if depth > MAX_DEPTH {
            self.warn("objects nested too deeply (left out)");
            return None;
        }
        let mut masks = inherited.to_vec();
        if let Some(m) = &node.pixel_mask {
            masks.push(AlphaMask { image: m, alpha: false });
        }
        let it = self.image(img, xf, layer, &node.name, &masks)?;
        if node.children.iter().any(|c| !matches!(c.kind, Kind::Image(_))) {
            self.warn("objects other than images inside pixel layers (left out)");
        }
        if !node.children.iter().any(|c| matches!(c.kind, Kind::Image(_))) {
            return Some(it);
        }
        let mut clip = masks;
        clip.push(AlphaMask { image: img, alpha: true });
        let mut items = vec![Arc::new(it)];
        for child in &node.children {
            let Kind::Image(ci) = &child.kind else { continue };
            if let Some(mut c) = self.image_node(child, ci, xf, layer, depth + 1, &clip) {
                self.node_props(child, &mut c);
                items.push(Arc::new(c));
            }
        }
        let mut g = self.new_item(layer, Shape::Group, PathData::default());
        g.content = Content::Group { items };
        Some(g)
    }

    /// Shadows and glows. Affinity gives the direction a shadow goes; DesignCraft (like InDesign)
    /// the direction its light comes from.
    fn effects(&mut self, effects: &[model::Effect], it: &mut Item) {
        let k = self.scale;
        let pt = move |v: f64| (v * k).clamp(0.0, 1000.0);
        let degrees = |a: f64| (180.0 - a.to_degrees()).rem_euclid(360.0);
        for e in effects {
            match e {
                model::Effect::DropShadow { color, opacity, radius, distance, angle } => {
                    let (color, alpha) = self.color(color);
                    it.effects.drop_shadow = designcraft_doc::DropShadow {
                        on: true,
                        color: self.swatch(color),
                        opacity: (opacity * alpha) as f32,
                        angle: degrees(*angle),
                        global_light: false,
                        distance: pt(*distance),
                        size: pt(*radius),
                        spread: 0.0,
                    };
                }
                model::Effect::InnerShadow { color, opacity, radius, distance, angle } => {
                    let (color, alpha) = self.color(color);
                    it.effects.inner_shadow = designcraft_doc::InnerShadow {
                        on: true,
                        color: self.swatch(color),
                        opacity: (opacity * alpha) as f32,
                        angle: degrees(*angle),
                        global_light: false,
                        distance: pt(*distance),
                        size: pt(*radius),
                        choke: 0.0,
                    };
                }
                model::Effect::Outline { color, opacity, width, align } => {
                    let (color, alpha) = self.color(color);
                    let swatch = self.swatch(color);
                    let weight = pt(*width);
                    if weight <= 0.0 {
                        continue;
                    }
                    if (opacity * alpha) < 0.999 {
                        self.warn("semi-transparent outline effects (imported opaque)");
                    }
                    let shape = !matches!(it.content, Content::Group { .. } | Content::Graphic(_));
                    match &it.content {
                        // Text: an outline on the characters.
                        Content::Text(tf) => {
                            let id = tf.story;
                            if let Some(story) = self.doc.stories.get_mut(&id) {
                                let story = Arc::make_mut(story);
                                let n = story.text.len();
                                story.format_chars(0..n, |f| {
                                    f.over.stroke = Some(swatch.clone());
                                    f.over.stroke_weight = Some(weight);
                                });
                            }
                        }
                        // A shape without a stroke of its own: the outline is its stroke.
                        _ if it.stroke.is_none() && shape => {
                            it.stroke = Stroke {
                                swatch,
                                weight,
                                align: match align {
                                    1 => StrokeAlign::Inside,
                                    2 => StrokeAlign::Center,
                                    _ => StrokeAlign::Outside,
                                },
                                join: Join::Round,
                                ..Stroke::default()
                            };
                        }
                        _ => self.warn("outline effects on stroked objects, groups or images (left out)"),
                    }
                }
                model::Effect::OuterGlow { color, opacity, radius } => {
                    let (color, alpha) = self.color(color);
                    it.effects.outer_glow = designcraft_doc::OuterGlow {
                        on: true,
                        color: self.swatch(color),
                        opacity: (opacity * alpha) as f32,
                        size: pt(*radius),
                        spread: 0.0,
                    };
                }
            }
        }
    }

    fn children(&mut self, node: &Node, xf: Affine, layer: LayerId, depth: usize) -> Option<Vec<Arc<Item>>> {
        let kids: Vec<Arc<Item>> = node.children.iter().filter_map(|c| self.item(c, xf, layer, depth + 1)).map(Arc::new).collect();
        (!kids.is_empty()).then_some(kids)
    }

    fn blend(&mut self, b: model::Blend) -> BlendMode {
        use model::Blend as B;
        match b {
            B::Normal | B::PassThrough => BlendMode::Normal,
            B::Darken => BlendMode::Darken,
            B::Multiply => BlendMode::Multiply,
            B::ColorBurn => BlendMode::ColorBurn,
            B::Lighten => BlendMode::Lighten,
            B::Screen => BlendMode::Screen,
            B::ColorDodge => BlendMode::ColorDodge,
            B::Overlay => BlendMode::Overlay,
            B::SoftLight => BlendMode::SoftLight,
            B::HardLight => BlendMode::HardLight,
            B::Difference => BlendMode::Difference,
            B::Exclusion => BlendMode::Exclusion,
            B::Hue => BlendMode::Hue,
            B::Saturation => BlendMode::Saturation,
            B::Color => BlendMode::Color,
            B::Luminosity => BlendMode::Luminosity,
            _ => {
                self.warn("blend modes DesignCraft doesn't have (imported as Normal)");
                BlendMode::Normal
            }
        }
    }

    // ---------- shapes and paint ----------

    fn path_data(&mut self, path: &model::Path, xf: Affine, even_odd: bool) -> PathData {
        let mut subpaths: Vec<BezPath> = Vec::new();
        for sp in &path.subpaths {
            let p = |q: model::Point| xf * Point::new(q.x, q.y);
            let mut bp = BezPath::new();
            let mut last = p(sp.start);
            bp.move_to(last);
            for [c1, c2, e] in &sp.segments {
                let (c1, c2, e) = (p(*c1), p(*c2), p(*e));
                // Straight segments stay lines (corner points without handles in DesignCraft).
                if on_chord(last, c1, e) && on_chord(last, c2, e) {
                    bp.line_to(e);
                } else {
                    bp.curve_to(c1, c2, e);
                }
                last = e;
            }
            if sp.closed {
                bp.close_path();
            }
            if bp.elements().iter().any(|e| e.end_point().is_some_and(|q| !q.x.is_finite() || !q.y.is_finite())) {
                self.warn("outlines with invalid coordinates (left out)");
                continue;
            }
            subpaths.push(bp);
        }
        if even_odd && subpaths.len() > 1 {
            nonzero_like_even_odd(&mut subpaths);
        }
        let mut all = BezPath::new();
        for bp in subpaths {
            all.extend(bp);
        }
        PathData::from_bezpath(&all)
    }

    /// A shape item. `clip` when it clips children (its outline must be closed).
    fn shape(
        &mut self,
        path: &model::Path,
        even_odd: bool,
        fills: &[Paint],
        strokes: &[paint::Stroke],
        xf: Affine,
        layer: LayerId,
        clip: bool,
    ) -> Option<Item> {
        let data = self.path_data(path, xf, even_odd);
        if data.is_empty() {
            return None;
        }
        let shape = shape_kind(&data);
        let mut it = self.new_item(layer, shape, data);
        let fill = fills.iter().find(|p| visible(p));
        let stroke = strokes.iter().find(|s| visible(&s.paint) && s.width > 0.0);
        let mut alpha = 1.0;
        if let Some(f) = fill {
            let (fill, a) = self.fill(f, xf);
            it.fill = fill;
            alpha = a;
        }
        if let Some(s) = stroke {
            let (stroke, a) = self.stroke(s, xf);
            it.stroke = stroke;
            if fill.is_none() {
                alpha = a;
            } else if (a - alpha).abs() > 1e-3 {
                self.warn("different fill and stroke opacity (the fill's is used)");
            }
            if s.behind && fill.is_some() {
                self.warn("strokes behind the fill (drawn over it)");
            }
        }
        if alpha < 1.0 {
            it.opacity = alpha.clamp(0.0, 1.0) as f32;
        }
        // Affinity fills open curves as if closed; DesignCraft fills closed paths only.
        if !it.fill.is_none() && !it.path.is_closed() {
            let mut closed = it.path.clone();
            closed.subpaths.iter_mut().for_each(|sp| sp.closed = true);
            if it.stroke.is_none() || clip {
                it.shape = shape_kind(&closed);
                it.path = closed;
            } else {
                // The fill closed, the stroke still open: a group of the two.
                let mut filled = self.new_item(layer, shape_kind(&closed), closed);
                filled.fill = std::mem::replace(&mut it.fill, Fill::none());
                filled.opacity = it.opacity;
                let mut g = self.new_item(layer, Shape::Group, PathData::default());
                g.content = Content::Group { items: vec![Arc::new(filled), Arc::new(it)] };
                return Some(g);
            }
        }
        Some(it)
    }

    /// A fill and its opacity (colour alpha becomes the object's opacity).
    fn fill(&mut self, p: &Paint, xf: Affine) -> (Fill, f64) {
        match p {
            Paint::None => (Fill::none(), 1.0),
            Paint::Solid(c) => {
                let (color, alpha) = self.color(c);
                (Fill::swatch(&self.swatch(color)), alpha)
            }
            Paint::Gradient(g) => {
                let kind = match g.kind {
                    paint::GradientKind::Linear => GradientKind::Linear,
                    paint::GradientKind::Radial => GradientKind::Radial,
                    paint::GradientKind::Conical => {
                        self.warn("conical gradients (imported as radial)");
                        GradientKind::Radial
                    }
                };
                let mut stops: Vec<GradientStop> = g
                    .stops
                    .iter()
                    .map(|s| {
                        let (color, alpha) = self.color(&s.color);
                        GradientStop {
                            offset: s.offset.clamp(0.0, 1.0) as f32,
                            color,
                            opacity: alpha.clamp(0.0, 1.0) as f32,
                            midpoint: s.half_point().clamp(0.13, 0.87) as f32,
                        }
                    })
                    .collect();
                if stops.is_empty() {
                    return (Fill::none(), 1.0);
                }
                stops.sort_by(|a, b| a.offset.total_cmp(&b.offset));
                let gradient = Gradient { kind, stops };
                let name = self.gradient_swatch(gradient);
                let m = xf * affine(g.transform);
                let (a, b) = (m * Point::ZERO, m * Point::new(1.0, 0.0));
                if g.kind != paint::GradientKind::Linear
                    && (m * Point::new(0.0, 1.0) - a).hypot() > 0.0
                    && ((m * Point::new(0.0, 1.0) - a).hypot() - (b - a).hypot()).abs() > 1e-3 * (b - a).hypot().max(1.0)
                {
                    self.warn("elliptical gradients (imported as circular)");
                }
                let mut fill = Fill::swatch(&name);
                if [a.x, a.y, b.x, b.y].iter().all(|v| v.is_finite()) && (b - a).hypot() > 1e-9 {
                    fill.gradient_vector = Some([a.x, a.y, b.x, b.y]);
                }
                (fill, 1.0)
            }
        }
    }

    fn stroke(&mut self, s: &paint::Stroke, xf: Affine) -> (Stroke, f64) {
        let scale = xf.determinant().abs().sqrt();
        let mut alpha = 1.0;
        let swatch = match &s.paint {
            Paint::Solid(c) => {
                let (color, a) = self.color(c);
                alpha = a;
                self.swatch(color)
            }
            Paint::Gradient(g) => {
                self.warn("gradient strokes (imported in their first colour)");
                match g.stops.first() {
                    Some(stop) => {
                        let (color, a) = self.color(&stop.color);
                        alpha = a;
                        self.swatch(color)
                    }
                    None => swatch::NONE.to_string(),
                }
            }
            Paint::None => swatch::NONE.to_string(),
        };
        let weight = s.width * scale;
        let kind = match &s.dash {
            Some((dashes, _)) if dashes.iter().any(|d| *d > 0.0) => {
                let pattern: Vec<f64> = dashes.iter().take(16).map(|d| (d * scale).max(0.0)).filter(|d| d.is_finite()).collect();
                if pattern.is_empty() { StrokeType::Solid } else { StrokeType::Dashed { pattern } }
            }
            _ => StrokeType::Solid,
        };
        let stroke = Stroke {
            swatch,
            weight: if weight.is_finite() { weight.clamp(0.0, 1000.0) } else { 1.0 },
            kind,
            align: match s.align {
                paint::Align::Center => StrokeAlign::Center,
                paint::Align::Inside => StrokeAlign::Inside,
                paint::Align::Outside => StrokeAlign::Outside,
            },
            cap: match s.cap {
                paint::Cap::Butt => Cap::Butt,
                paint::Cap::Round => Cap::Round,
                paint::Cap::Square => Cap::Projecting,
            },
            join: match s.join {
                paint::Join::Miter => Join::Miter,
                paint::Join::Round => Join::Round,
                paint::Join::Bevel => Join::Bevel,
            },
            miter_limit: if s.miter_limit.is_finite() { s.miter_limit.clamp(1.0, 500.0) } else { 4.0 },
            ..Stroke::default()
        };
        (stroke, alpha)
    }

    /// A colour in DesignCraft's terms and its alpha.
    fn color(&mut self, c: &paint::Color) -> (Color, f64) {
        let u = |v: f64| if v.is_finite() { v.clamp(0.0, 1.0) as f32 } else { 0.0 };
        let color = match *c {
            paint::Color::Rgb { r, g, b, .. } => Color::rgb(u(r), u(g), u(b)),
            paint::Color::Cmyk { c, m, y, k, .. } => Color::cmyk(u(c), u(m), u(y), u(k)),
            // Affinity's grey is a lightness; DesignCraft's is an amount of black ink.
            paint::Color::Gray { v, .. } => Color::gray(1.0 - u(v)),
            paint::Color::Lab { l, a, b, .. } => {
                self.warn("Lab colours (imported as RGB)");
                let f = |v: f64| if v.is_finite() { v as f32 } else { 0.0 };
                let [r, g, b] = designcraft_color::cms::lab::lab_to_srgb(designcraft_color::cms::Lab { l: f(l), a: f(a), b: f(b) });
                Color::rgb(r.clamp(0.0, 1.0), g.clamp(0.0, 1.0), b.clamp(0.0, 1.0))
            }
        };
        let a = c.alpha();
        (color, if a.is_finite() { a.clamp(0.0, 1.0) } else { 1.0 })
    }

    /// The swatch holding `color`: an existing one, else a new unnamed colour.
    fn swatch(&mut self, color: Color) -> String {
        if let Some((_, name)) = self.colors.iter().find(|(c, _)| *c == color) {
            return name.clone();
        }
        let existing = self
            .doc
            .swatches
            .iter()
            .find(|w| matches!(&w.value, SwatchValue::Color { color: c, color_type: ColorType::Process } if *c == color))
            .map(|w| w.name.clone());
        let name = match existing {
            Some(n) => n,
            None => {
                let name = self.unique_swatch_name(&color_name(color));
                self.doc.swatches.push(Swatch {
                    name: name.clone(),
                    value: SwatchValue::Color { color, color_type: ColorType::Process },
                    locked: false,
                    named: false,
                    hidden: true,
                });
                name
            }
        };
        self.colors.push((color, name.clone()));
        name
    }

    fn gradient_swatch(&mut self, gradient: Gradient) -> String {
        if let Some(w) = self.doc.swatches.iter().find(|w| matches!(&w.value, SwatchValue::Gradient { gradient: g } if *g == gradient)) {
            return w.name.clone();
        }
        let name = self.unique_swatch_name("Gradient");
        self.doc.swatches.push(Swatch { name: name.clone(), value: SwatchValue::Gradient { gradient }, locked: false, named: false, hidden: true });
        name
    }

    fn unique_swatch_name(&self, base: &str) -> String {
        if self.doc.swatch(base).is_none() {
            return base.to_string();
        }
        (2..).map(|i| format!("{base} {i}")).find(|n| self.doc.swatch(n).is_none()).unwrap_or_else(|| base.to_string())
    }

    // ---------- text ----------

    /// How a text or table node sits on the spread: `None` (warned) for an unusable transform.
    /// `m` places the node, `type_m` scales its type (see [`model::Text::text_transform`]).
    fn placement(&mut self, m: Affine, type_m: Affine) -> Option<Placement> {
        let [a, b, c, d, e, f] = m.as_coeffs();
        let sx = a.hypot(b);
        let det = a * d - b * c;
        if !(sx > 1e-9 && det.is_finite() && det.abs() > 1e-12) {
            self.warn("text with an invalid transform (left out)");
            return None;
        }
        let sy = det / sx;
        let shear = (a * c + b * d) / (sx * sx);
        if shear.abs() > 1e-3 || sy < 0.0 {
            self.warn("skewed or flipped text (imported without the skew or flip)");
        }
        let angle = b.atan2(a);
        let rotated = angle.abs() > 1e-9;
        // The type's vertical scale gives the font size, its horizontal scale relative to that the
        // characters' width.
        let [ta, tb, tc, td, ..] = type_m.as_coeffs();
        let (tsx, tdet) = (ta.hypot(tb), ta * td - tb * tc);
        let type_scale = if tsx > 1e-9 && tdet.is_finite() && tdet.abs() > 1e-12 {
            let tsy = (tdet / tsx).abs();
            (tsy, tsx / tsy)
        } else {
            self.warn("text with an invalid text scale (imported unscaled)");
            (xf_scale(m), 1.0)
        };
        Some(Placement {
            to_frame: Affine::scale_non_uniform(sx, sy.abs()),
            place: if rotated { Affine::translate((e, f)) * Affine::rotate(angle) } else { Affine::translate((e, f)) },
            rotated,
            size_scale: type_scale.0,
            h_scale: type_scale.1,
        })
    }

    /// A text frame for `story` over `rect` (frame space).
    fn frame(&mut self, rect: Rect, p: &Placement, layer: LayerId, mut story: Story, options: TextFrameOptions) -> Option<Item> {
        if ![rect.x0, rect.y0, rect.x1, rect.y1].iter().all(|v| v.is_finite()) || rect.width() <= 0.0 || rect.height() <= 0.0 {
            self.warn("text frames without a size (left out)");
            return None;
        }
        let corners = [Point::new(rect.x0, rect.y0), Point::new(rect.x1, rect.y0), Point::new(rect.x1, rect.y1), Point::new(rect.x0, rect.y1)];
        let (path, item_xf) = if p.rotated { (rect_path(&corners), p.place) } else { (rect_path(&corners.map(|q| p.place * q)), Affine::IDENTITY) };
        let mut it = self.new_item(layer, Shape::Rectangle, path);
        it.xf = item_xf;
        story.frames = vec![it.id];
        it.content = Content::Text(TextFrame { story: story.id, options });
        self.doc.stories.insert(story.id, Arc::new(story));
        Some(it)
    }

    fn text(&mut self, t: &model::Text, xf: Affine, layer: LayerId) -> Option<Item> {
        let p = self.placement(xf * affine(t.transform), xf * affine(t.text_transform))?;
        let id = StoryId(self.doc.alloc());
        let (story, max_size) = self.story(id, &t.runs, (p.size_scale, p.h_scale), t.frame.is_none());
        let alpha = self.text_alpha(&t.runs);
        let mut options = TextFrameOptions::default();
        let rect = match t.frame {
            Some(r) => Rect::from_points(p.to_frame * Point::new(r.x0, r.y0), p.to_frame * Point::new(r.x1, r.y1)),
            None => {
                // Artistic text: a frame wide enough for its longest line, with the first baseline
                // exactly at Affinity's anchor.
                let lines = story.text.split(['\n', '\u{2028}']).collect::<Vec<_>>();
                let longest = lines.iter().map(|l| l.chars().count()).max().unwrap_or(0) as f64;
                let w = (longest * max_size * 0.75 + max_size).max(1.0);
                let line = story_leading(&story).unwrap_or(max_size * 1.2).max(max_size * 1.2);
                let h = (lines.len() as f64 + 1.0) * line;
                let anchor = p.to_frame * Point::new(t.anchor.x, t.anchor.y);
                let x0 = match t.align {
                    model::Align::Center => anchor.x - w / 2.0,
                    model::Align::Right => anchor.x - w,
                    _ => anchor.x,
                };
                options.first_baseline = FirstBaseline::Fixed;
                options.first_baseline_min = max_size;
                Rect::new(x0, anchor.y - max_size, x0 + w, anchor.y - max_size + h)
            }
        };
        let mut it = self.frame(rect, &p, layer, story, options)?;
        it.opacity = alpha as f32;
        Some(it)
    }

    /// The opacity a text frame takes from its characters' colour: DesignCraft has no per-character
    /// opacity, so text whose characters share one becomes a frame with that opacity.
    fn text_alpha(&mut self, runs: &[model::TextRun]) -> f64 {
        let alphas: Vec<f64> =
            runs.iter().filter_map(|r| if let Paint::Solid(c) = &r.fill { Some(c.alpha()) } else { None }).filter(|a| a.is_finite()).collect();
        let (lo, hi) = alphas.iter().fold((1.0f64, 0.0f64), |(lo, hi), a| (lo.min(*a), hi.max(*a)));
        if alphas.is_empty() || lo >= 1.0 {
            return 1.0;
        }
        if hi - lo > 0.01 {
            self.warn("text with characters of different transparency (imported opaque)");
            return 1.0;
        }
        lo.clamp(0.0, 1.0)
    }

    /// A table: a text frame over the table's bounds whose story holds just the table.
    fn table(&mut self, t: &model::Table, xf: Affine, layer: LayerId) -> Option<Item> {
        let p = self.placement(xf * affine(t.transform), xf * affine(t.text_transform))?;
        let (ncols, nrows) = (t.ncols(), t.nrows());
        let (&x0, &x1, &y0, &y1) = (t.columns.first()?, t.columns.last()?, t.rows.first()?, t.rows.last()?);
        let rect = Rect::from_points(p.to_frame * Point::new(x0, y0), p.to_frame * Point::new(x1, y1));
        let [sx, _, _, sy, _, _] = p.to_frame.as_coeffs();
        let id = self.doc.alloc();
        let mut table = Table::new(id, nrows, ncols, 0, 0, rect.width());
        for (col, w) in table.columns.iter_mut().zip(t.columns.windows(2)) {
            col.width = ((w[1] - w[0]) * sx).max(1.0);
        }
        for (row, h) in table.rows.iter_mut().zip(t.rows.windows(2)) {
            *row = Row { height: ((h[1] - h[0]) * sy).max(1.0), mode: RowHeightMode::AtLeast, kind: RowKind::Body };
        }
        let edge = |b: &mut Self, s: Option<&Option<paint::Stroke>>| -> CellStroke {
            match s {
                Some(Some(s)) => {
                    let (stroke, _) = b.stroke(s, xf);
                    CellStroke { weight: stroke.weight, color: stroke.swatch, tint: 1.0, kind: stroke.kind }
                }
                _ => CellStroke::none(),
            }
        };
        for (i, src) in t.cells.iter().enumerate() {
            if src.merged_left {
                continue;
            }
            let (r, c) = (i / ncols.max(1), i % ncols.max(1));
            // Cells to the right merged into this one widen it.
            let span = 1 + t.cells.iter().skip(i + 1).take(ncols - c - 1).take_while(|n| n.merged_left).count();
            let last = c + span - 1;
            let (mut text, _) = self.story(StoryId(0), &src.runs, (p.size_scale, p.h_scale), false);
            if src.runs.iter().any(|r| matches!(&r.fill, Paint::Solid(c) if c.alpha() < 1.0)) {
                self.warn("semi-transparent text in tables (imported opaque)");
            }
            let top: Vec<CellStroke> = (c..=last).map(|k| edge(self, t.horizontal.get(r * ncols + k))).collect();
            let bottom: Vec<CellStroke> = (c..=last).map(|k| edge(self, t.horizontal.get((r + 1) * ncols + k))).collect();
            let strokes = [
                widest(top),
                edge(self, t.vertical.get(r * (ncols + 1) + c)),
                widest(bottom),
                edge(self, t.vertical.get(r * (ncols + 1) + last + 1)),
            ];
            let [l, tp, rt, bt] = src.inset;
            let row_height = table.rows.get(r).map_or(0.0, |row| row.height);
            let (top, bottom) = fit_cell_text(&mut text, row_height, tp * sy, bt * sy);
            let Some(cell) = table.cell_mut(r, c) else { continue };
            *cell = Cell {
                text,
                col_span: span as u32,
                insets: [top, l * sx, bottom, rt * sx],
                vj: match src.valign {
                    model::VAlign::Top => VerticalJustification::Top,
                    model::VAlign::Center => VerticalJustification::Center,
                    model::VAlign::Bottom => VerticalJustification::Bottom,
                },
                strokes,
                border_overrides: [true; 4],
                ..Cell::default()
            };
        }
        table.options = TableOptions {
            border: CellStroke::none(),
            space_before: 0.0,
            space_after: 0.0,
            repeat_header: false,
            repeat_footer: false,
            ..TableOptions::default()
        };
        let mut story = Story::new(StoryId(self.doc.alloc()));
        story.insert_table(0, table);
        // The anchor's paragraph is the story's last.
        if story.text.ends_with('\n') {
            let n = story.text.len();
            story.delete(n - 1..n);
        }
        // Room below the table, so rows that grow with their text (other fonts) are never cut off.
        let rect = Rect::new(rect.x0, rect.y0, rect.x1, rect.y1 + rect.height() * 0.5);
        self.frame(rect, &p, layer, story, TextFrameOptions::default())
    }

    /// A story from runs → (the story, its largest font size in points). Each paragraph takes the
    /// alignment of the run it starts with.
    fn story(&mut self, id: StoryId, runs: &[model::TextRun], scale: (f64, f64), artistic: bool) -> (Story, f64) {
        let mut story = Story::new(id);
        let mut max_size: f64 = 0.0;
        let mut first = true;
        let mut formats: Vec<(usize, model::Paragraph)> = Vec::new();
        for run in runs {
            let text: String = run
                .text
                .chars()
                .filter_map(|c| match c {
                    '\u{2029}' | '\r' | '\n' => Some('\n'),
                    '\u{2028}' | '\t' => Some(c),
                    c if c.is_control() => None,
                    // DesignCraft's special characters (private use, U+E000 on) can't come from Affinity
                    // text; icon fonts further up the private use area stay.
                    '\u{E000}'..='\u{E0FF}' => None,
                    c => Some(c),
                })
                .collect();
            if text.is_empty() {
                continue;
            }
            let attrs = self.char_attrs(run, scale);
            max_size = max_size.max(attrs.size.unwrap_or(12.0));
            let format = CharFormat { style: NO_CHAR_STYLE.into(), over: attrs };
            let at = story.text.len();
            if first {
                // The empty story's run holds the typing format: replace it with the first run's.
                story.chars = vec![designcraft_doc::CharRun { len: 0, format: format.clone() }];
                first = false;
            }
            formats.push((at, run.paragraph));
            story.insert_with(at, &text, format);
        }
        // Affinity ends a story with a paragraph mark; DesignCraft's last paragraph has none.
        if story.text.ends_with('\n') {
            let n = story.text.len();
            story.delete(n - 1..n);
        }
        let ranges = story.para_ranges();
        let fonts: Vec<(String, String)> = ranges
            .iter()
            .map(|r| {
                let f = &story.char_format_at(r.start).over;
                (f.font_family.clone().unwrap_or_default(), f.font_style.clone().unwrap_or_default())
            })
            .collect();
        let line_heights: HashMap<(String, String), f64> = fonts.iter().map(|f| (f.clone(), self.line_height(&f.0, &f.1))).collect();
        let (v, h) = (scale.0, scale.0 * scale.1);
        let mut previous_after = 0.0;
        let mut para_leading: Vec<(std::ops::Range<usize>, f64)> = Vec::new();
        for (pi, range) in ranges.iter().enumerate() {
            let f = formats.iter().rev().find(|(at, _)| *at <= range.start).map(|(_, f)| *f).unwrap_or_default();
            let Some(p) = story.paras.get_mut(pi) else { continue };
            let pt = |x: f64, k: f64| if (x * k).is_finite() { x * k } else { 0.0 };
            let align = align_of(f.align);
            if align != Align::Left {
                p.para.align = Some(align);
            }
            // Affinity places the first line and the others from the left edge; DesignCraft's
            // first-line indent is relative to its left indent.
            let left = pt(f.left, h);
            let first = pt(f.first - f.left, h);
            let right = pt(f.right, h);
            for (value, field) in [(left, &mut p.para.left_indent), (first, &mut p.para.first_line_indent), (right, &mut p.para.right_indent)] {
                if value.abs() > 1e-9 {
                    *field = Some(value.clamp(-5000.0, 5000.0));
                }
            }
            // Between two paragraphs Affinity keeps the larger of the space after the first and
            // before the second; DesignCraft adds them.
            let (before, after) = (pt(f.space_before, v).clamp(0.0, 5000.0), pt(f.space_after, v).clamp(0.0, 5000.0));
            let before = if pi == 0 { before } else { (before - previous_after).max(0.0) };
            if before > 1e-9 {
                p.para.space_before = Some(before);
            }
            if after > 1e-9 {
                p.para.space_after = Some(after);
            }
            previous_after = after;
            if let Some(l) = f.leading.map(|l| pt(l, v)).filter(|l| *l > 0.0) {
                para_leading.push((range.clone(), l.min(5000.0)));
            }
            // Automatic leading, measured on thumbnails: artistic text spaces its lines by the font
            // size (Bebas Neue Pro and Impact lines exactly 1 em apart), frame text by the font's
            // own line height (Arial 1.14–1.17 em); DesignCraft uses 120 % of the size.
            let ratio =
                if artistic { f.auto_leading } else { fonts.get(pi).and_then(|f| line_heights.get(f)).copied().unwrap_or(1.2) * f.auto_leading };
            if ratio.is_finite() && (0.5..=3.0).contains(&ratio) && (ratio - 1.2).abs() > 0.005 {
                p.para.auto_leading = Some(ratio);
            }
            // Affinity's hyphenation settings aren't read yet; none of the documents checked
            // hyphenates, while DesignCraft's paragraphs do by default.
            p.para.hyphenate = Some(false);
        }
        // A paragraph's fixed leading, where its characters don't set their own.
        for (range, l) in para_leading {
            story.format_chars(range, |f| {
                if f.over.leading.is_none() {
                    f.over.leading = Some(Leading::Points(l));
                }
            });
        }
        (story, if max_size > 0.0 { max_size } else { 12.0 })
    }

    /// A font's own line height (ascent plus descent) in em, as DesignCraft will lay it out (an
    /// installed face, or the fallback DesignCraft uses for a missing one).
    fn line_height(&mut self, family: &str, style: &str) -> f64 {
        let key = (family.to_string(), style.to_string());
        if let Some(h) = self.line_heights.get(&key) {
            return *h;
        }
        let defaults = designcraft_doc::CharProps::default();
        let family = if family.is_empty() { defaults.font_family.as_str() } else { family };
        let style = if style.is_empty() { defaults.font_style.as_str() } else { style };
        let face = designcraft_fonts::FontDb::global().face(family, style);
        let h = if face.upem > 0.0 { (face.ascent + face.descent) / face.upem } else { 1.2 };
        let h = if h.is_finite() && h > 0.0 { h } else { 1.2 };
        self.line_heights.insert(key, h);
        h
    }

    /// Character attributes; `scale` is the vertical scale to points and the horizontal scale
    /// relative to it.
    fn char_attrs(&mut self, run: &model::TextRun, (size_scale, h_scale): (f64, f64)) -> CharAttrs {
        let finite = |v: f64, d: f64| if v.is_finite() { v } else { d };
        let size = finite(run.size * size_scale, 12.0).clamp(0.1, 1296.0);
        let mut a = CharAttrs { size: Some(size), ..CharAttrs::default() };
        if !run.family.trim().is_empty() {
            a.font_family = Some(run.family.trim().to_string());
            a.font_style = Some(style_name(run.weight, run.italic));
        }
        // Text squeezed or stretched by its frame's transform keeps that as horizontal scaling, on
        // top of its own.
        let h_scale = h_scale * run.h_scale;
        if h_scale.is_finite() && (h_scale - 1.0).abs() > 1e-3 {
            a.h_scale = Some(h_scale.clamp(0.01, 10.0));
        }
        if run.tracking != 0.0 {
            a.tracking = Some(finite(run.tracking * 1000.0, 0.0).clamp(-1000.0, 10000.0));
        }
        if let Some(l) = run.leading {
            let l = finite(l * size_scale, 0.0);
            if l > 0.0 {
                a.leading = Some(Leading::Points(l.min(5000.0)));
            }
        }
        if run.all_caps {
            a.capitalization = Some(Capitalization::AllCaps);
        }
        if !run.features.is_empty() {
            a.otf_features = Some(run.features.clone());
        }
        match &run.fill {
            Paint::Solid(c) => {
                // Transparency is the frame's (see `text_alpha`).
                let (color, _) = self.color(c);
                a.fill = Some(self.swatch(color));
            }
            Paint::Gradient(g) => {
                self.warn("gradient text (imported in its first colour)");
                if let Some(stop) = g.stops.first() {
                    let (color, _) = self.color(&stop.color);
                    a.fill = Some(self.swatch(color));
                }
            }
            Paint::None => a.fill = Some(swatch::NONE.to_string()),
        }
        if let Some(st) = &run.stroke {
            let color = match &st.paint {
                Paint::Solid(c) => Some(self.color(c).0),
                Paint::Gradient(g) => g.stops.first().map(|stop| self.color(&stop.color).0),
                Paint::None => None,
            };
            let weight = st.width * size_scale;
            if let Some(color) = color
                && weight.is_finite()
                && weight > 0.0
            {
                a.stroke = Some(self.swatch(color));
                a.stroke_weight = Some(weight.min(1000.0));
            }
        }
        a
    }

    // ---------- images ----------

    fn image(&mut self, img: &model::Image, xf: Affine, layer: LayerId, name: &str, masks: &[AlphaMask]) -> Option<Item> {
        let (w, h) = (f64::from(img.width), f64::from(img.height));
        if img.width == 0 || img.height == 0 {
            return None;
        }
        let masked = if masks.is_empty() {
            None
        } else {
            let baked = bake(img, masks).and_then(|px| png(img.width, img.height, &px));
            if baked.is_none() {
                self.warn("pixel masks that could not be applied (imported without them)");
            }
            baked
        };
        let (data, mime) = match (masked, &img.pixels) {
            (Some(png), _) => (png, "image/png".to_string()),
            (None, Pixels::Encoded(bytes)) => (bytes.clone(), designcraft_images::mime(bytes).to_string()),
            (None, Pixels::Rgba8(rgba)) => match png(img.width, img.height, rgba) {
                Some(png) => (png, "image/png".to_string()),
                None => {
                    self.warn("pixel layers that could not be stored (left out)");
                    return None;
                }
            },
        };
        let m = xf * affine(img.transform);
        let corners = [Point::ZERO, Point::new(w, 0.0), Point::new(w, h), Point::new(0.0, h)].map(|p| m * p);
        if corners.iter().any(|p| !p.x.is_finite() || !p.y.is_finite()) {
            self.warn("images with an invalid transform (left out)");
            return None;
        }
        self.images += 1;
        let aid = AssetId(self.doc.alloc());
        let asset_name = if name.trim().is_empty() { format!("Image {}", self.images) } else { name.trim().to_string() };
        self.doc.assets.insert(
            aid,
            Arc::new(designcraft_doc::Asset {
                id: aid,
                name: asset_name,
                mime,
                link: None,
                data: Arc::new(data),
                pixels: Some((img.width, img.height)),
                page: 0,
            }),
        );
        let mut it = self.new_item(layer, Shape::Rectangle, rect_path(&corners));
        it.content = Content::Graphic(Graphic { asset: aid, size: (w, h), xf: m, auto_fit: Default::default(), fit_align: 4, crop: [0.0; 4] });
        Some(it)
    }
}

/// Does the paint show? A fully transparent colour is the same as none.
fn visible(p: &Paint) -> bool {
    match p {
        Paint::None => false,
        Paint::Solid(c) => c.alpha() > 0.0,
        Paint::Gradient(g) => g.stops.iter().any(|s| s.color.alpha() > 0.0),
    }
}

/// Affinity lets a cell's text run over its top and bottom insets, and even past the row; a
/// DesignCraft row grows to fit its text instead. Shrink the insets, and if that isn't enough the
/// line spacing, so the rows stay as close to Affinity's as the fonts allow → the insets.
fn fit_cell_text(story: &mut Story, row: f64, top: f64, bottom: f64) -> (f64, f64) {
    let lines = story.text.split(['\n', '\u{2028}']).count().max(1) as f64;
    let size = story.chars.iter().map(|run| run.format.over.size.unwrap_or(12.0)).fold(0.0f64, f64::max);
    let line = story
        .chars
        .iter()
        .map(|run| match run.format.over.leading {
            Some(Leading::Points(l)) => l,
            _ => run.format.over.size.unwrap_or(12.0) * 1.2,
        })
        .fold(0.0f64, f64::max);
    // A first line takes about its font's ascent and descent; later lines their leading.
    let first = size * 1.2;
    let need = first + (lines - 1.0) * line;
    if !(row > 0.0 && need.is_finite()) || need + top + bottom <= row {
        return (top, bottom);
    }
    if need <= row {
        let spare = (row - need) / 2.0;
        return (top.min(spare), bottom.min(spare));
    }
    if lines > 1.0 {
        let fitted = Leading::Points(((row - first) / (lines - 1.0)).max(size * 0.8));
        for run in &mut story.chars {
            run.format.over.leading = Some(fitted);
        }
        story.rev += 1;
    }
    (0.0, 0.0)
}

/// One stroke for a merged cell's edge made of several: the heaviest.
fn widest(strokes: Vec<CellStroke>) -> CellStroke {
    strokes.into_iter().reduce(|a, b| if b.weight > a.weight { b } else { a }).unwrap_or_else(CellStroke::none)
}

/// How much a transform scales (the average of its axes).
fn xf_scale(m: Affine) -> f64 {
    m.determinant().abs().sqrt()
}

/// The master page instances among `nodes` (also inside layers and groups): (master, first page,
/// first master page, page count).
fn master_instances<'a>(nodes: impl IntoIterator<Item = &'a Node>) -> Vec<(usize, usize, usize, usize)> {
    fn visit(n: &Node, depth: usize, out: &mut Vec<(usize, usize, usize, usize)>) {
        if depth > MAX_DEPTH || !n.visible {
            return;
        }
        match n.kind {
            Kind::MasterInstance { master, page, master_page, count } => out.push((master, page, master_page, count)),
            Kind::Layer | Kind::Group => n.children.iter().for_each(|c| visit(c, depth + 1, out)),
            _ => {}
        }
    }
    let mut out = Vec::new();
    nodes.into_iter().for_each(|n| visit(n, 0, &mut out));
    out
}

/// Parent prefixes: A, B … Z, then A1, B1 ….
fn parent_prefix(index: usize) -> String {
    let letter = char::from(b'A' + (index % 26) as u8);
    match index / 26 {
        0 => letter.to_string(),
        n => format!("{letter}{n}"),
    }
}

fn align_of(a: model::Align) -> Align {
    match a {
        model::Align::Left => Align::Left,
        model::Align::Center => Align::Center,
        model::Align::Right => Align::Right,
        model::Align::Justify => Align::LeftJustified,
    }
}

fn valid_size(v: f64) -> bool {
    v.is_finite() && v > 0.0 && v <= MAX_PAGE
}

fn affine(m: model::Affine) -> Affine {
    Affine::new(m.0)
}

fn rect_path(corners: &[Point; 4]) -> PathData {
    PathData::single(designcraft_geom::SubPath::polyline(corners, true))
}

/// Does control point `c` lie on the segment from `a` to `b`?
fn on_chord(a: Point, c: Point, b: Point) -> bool {
    let (ab, ac) = (b - a, c - a);
    let len = ab.hypot();
    if len < 1e-9 {
        return ac.hypot() < 1e-6;
    }
    let along = ac.dot(ab) / len;
    (ab.cross(ac) / len).abs() < 1e-6 * len.max(1.0) && along >= -1e-9 && along <= len + 1e-9
}

/// Rectangle for a closed four-corner axis-aligned outline, line for one straight segment.
fn shape_kind(p: &PathData) -> Shape {
    let [sp] = p.subpaths.as_slice() else { return Shape::Path };
    let straight = (0..sp.segment_count()).all(|i| sp.segment_is_line(i));
    if !straight {
        return Shape::Path;
    }
    if !sp.closed && sp.anchors.len() == 2 {
        return Shape::GraphicLine;
    }
    if sp.closed && sp.anchors.len() == 4 {
        let pts: Vec<Point> = sp.anchors.iter().map(|a| a.p).collect();
        let axis = |a: Point, b: Point| (a.x - b.x).abs() < 1e-6 || (a.y - b.y).abs() < 1e-6;
        if pts.iter().zip(pts.iter().cycle().skip(1)).all(|(a, b)| axis(*a, *b)) {
            return Shape::Rectangle;
        }
    }
    Shape::Path
}

/// DesignCraft fills by the nonzero rule. For an even-odd outline whose subpaths don't cross,
/// alternating the direction of nested subpaths gives the same holes.
fn nonzero_like_even_odd(subpaths: &mut [BezPath]) {
    use designcraft_geom::Shape as _;
    let n = subpaths.len().min(4096);
    let starts: Vec<Option<Point>> = subpaths.iter().map(|bp| bp.elements().first().and_then(|e| e.end_point())).collect();
    let mut depth = vec![0usize; n];
    for i in 0..n {
        let Some(Some(p)) = starts.get(i) else { continue };
        for (j, other) in subpaths.iter().enumerate().take(n) {
            if j != i
                && other.winding(*p) != 0
                && let Some(d) = depth.get_mut(i)
            {
                *d += 1;
            }
        }
    }
    let Some(outer) = subpaths.first().map(|bp| bp.area().signum()) else { return };
    for (bp, d) in subpaths.iter_mut().zip(depth) {
        let want = if d % 2 == 0 { outer } else { -outer };
        if bp.area().signum() != want {
            *bp = bp.reverse_subpaths();
        }
    }
}

/// `Bold Italic`, `Light`, `Regular`…: the style names DesignCraft's font matching understands.
fn style_name(weight: i64, italic: bool) -> String {
    let w = match weight {
        ..=149 => "Thin",
        150..=249 => "ExtraLight",
        250..=349 => "Light",
        350..=449 => "Regular",
        450..=549 => "Medium",
        550..=649 => "SemiBold",
        650..=749 => "Bold",
        750..=849 => "ExtraBold",
        _ => "Black",
    };
    match (w, italic) {
        ("Regular", true) => "Italic".into(),
        (w, true) => format!("{w} Italic"),
        (w, false) => w.into(),
    }
}

fn color_name(c: Color) -> String {
    let p = |v: f32| (v * 100.0).round() as i32;
    let b = |v: f32| (v * 255.0).round() as i32;
    match c {
        Color::Cmyk { c, m, y, k } => swatch::cmyk_name(c, m, y, k),
        Color::Rgb { r, g, b: bl } => format!("R={} G={} B={}", b(r), b(g), b(bl)),
        Color::Gray { k } => format!("K={}", p(k)),
    }
}

fn story_leading(story: &Story) -> Option<f64> {
    story.chars.iter().filter_map(|r| if let Some(Leading::Points(l)) = r.format.over.leading { Some(l) } else { None }).reduce(f64::max)
}

/// What hides parts of an image: a grey pixel mask (white shows), or another image's
/// transparency (`alpha`). Outside the mask the image is hidden.
#[derive(Clone, Copy)]
struct AlphaMask<'a> {
    image: &'a model::Image,
    alpha: bool,
}

/// Largest image (pixels) whose masks are worked into its pixels.
const MAX_BAKE_PIXELS: usize = 100_000_000;

/// An image's straight-alpha RGBA pixels (placed files are decoded).
fn rgba(img: &model::Image) -> Option<std::borrow::Cow<'_, [u8]>> {
    let pixels = (img.width as usize).checked_mul(img.height as usize).filter(|n| *n <= MAX_BAKE_PIXELS)?;
    let len = pixels.checked_mul(4)?;
    match &img.pixels {
        Pixels::Rgba8(p) => p.get(..len).map(std::borrow::Cow::Borrowed),
        Pixels::Encoded(bytes) => {
            let decoded = image::load_from_memory(bytes).ok()?.to_rgba8();
            (decoded.width() == img.width && decoded.height() == img.height).then(|| std::borrow::Cow::Owned(decoded.into_raw()))
        }
    }
}

/// `img`'s pixels with `masks` multiplied into their alpha, sampling each mask (nearest pixel)
/// where the image's pixel centres fall in document space.
fn bake(img: &model::Image, masks: &[AlphaMask]) -> Option<Vec<u8>> {
    let mut px = rgba(img)?.into_owned();
    let width = img.width as usize;
    for mask in masks {
        let src = rgba(mask.image)?;
        // Image pixels → document → mask pixels.
        let to_mask = img.transform.then(invert(mask.image.transform)?);
        let (mw, mh) = (mask.image.width as usize, mask.image.height as usize);
        for (i, p) in px.as_chunks_mut::<4>().0.iter_mut().enumerate() {
            let (x, y) = ((i % width.max(1)) as f64 + 0.5, (i / width.max(1)) as f64 + 0.5);
            let q = to_mask.apply(model::Point { x, y });
            let inside = q.x >= 0.0 && q.y >= 0.0 && q.x < mw as f64 && q.y < mh as f64;
            let at = if inside { (q.y as usize).checked_mul(mw).and_then(|r| r.checked_add(q.x as usize)) } else { None };
            let level = match at.and_then(|k| src.get(k * 4..k * 4 + 4)) {
                Some([v, _, _, a]) if !mask.alpha => u32::from(*v) * u32::from(*a) / 255,
                Some([_, _, _, a]) => u32::from(*a),
                _ => 0,
            };
            p[3] = (u32::from(p[3]) * level / 255) as u8;
        }
    }
    Some(px)
}

fn invert(m: model::Affine) -> Option<model::Affine> {
    let [a, b, c, d, e, f] = m.0;
    let det = a * d - b * c;
    if !(det.is_finite() && det.abs() > 1e-12) {
        return None;
    }
    let (ia, ib, ic, id) = (d / det, -b / det, -c / det, a / det);
    Some(model::Affine([ia, ib, ic, id, -(ia * e + ic * f), -(ib * e + id * f)]))
}

fn png(width: u32, height: u32, rgba: &[u8]) -> Option<Vec<u8>> {
    let expected = (width as usize).checked_mul(height as usize)?.checked_mul(4)?;
    let pixels = rgba.get(..expected)?.to_vec();
    let img = image::RgbaImage::from_raw(width, height, pixels)?;
    let mut out = std::io::Cursor::new(Vec::new());
    img.write_to(&mut out, image::ImageFormat::Png).ok()?;
    Some(out.into_inner())
}

#[cfg(test)]
mod tests {
    use super::*;
    use designcraft_geom::Shape as _;

    fn square(x0: f64, size: f64, clockwise: bool) -> BezPath {
        let x1 = x0 + size;
        let mut pts = [Point::new(x0, x0), Point::new(x1, x0), Point::new(x1, x1), Point::new(x0, x1)];
        if !clockwise {
            pts.reverse();
        }
        let mut bp = BezPath::new();
        bp.move_to(pts[0]);
        for p in &pts[1..] {
            bp.line_to(*p);
        }
        bp.close_path();
        bp
    }

    #[test]
    fn even_odd_holes_survive_nonzero_filling() {
        // Three nested squares drawn the same way round: even-odd shows a ring and a centre.
        let mut subpaths = vec![square(0.0, 100.0, true), square(20.0, 60.0, true), square(40.0, 20.0, true)];
        nonzero_like_even_odd(&mut subpaths);
        let mut all = BezPath::new();
        for bp in subpaths {
            all.extend(bp);
        }
        assert_ne!(all.winding(Point::new(10.0, 50.0)), 0, "ring");
        assert_eq!(all.winding(Point::new(30.0, 50.0)), 0, "hole");
        assert_ne!(all.winding(Point::new(50.0, 50.0)), 0, "centre");
    }

    #[test]
    fn style_names_follow_weight_and_slant() {
        assert_eq!(style_name(400, false), "Regular");
        assert_eq!(style_name(400, true), "Italic");
        assert_eq!(style_name(700, true), "Bold Italic");
        assert_eq!(style_name(300, false), "Light");
        assert_eq!(style_name(i64::MIN, false), "Thin");
        assert_eq!(style_name(i64::MAX, false), "Black");
    }

    #[test]
    fn straight_cubic_segments_become_lines() {
        let (a, b) = (Point::new(0.0, 0.0), Point::new(30.0, 0.0));
        assert!(on_chord(a, Point::new(10.0, 0.0), b));
        assert!(!on_chord(a, Point::new(10.0, 1.0), b));
        assert!(!on_chord(a, Point::new(-5.0, 0.0), b), "a control point beyond the end makes a curve");
        assert!(on_chord(a, a, a));
    }

    fn node(kind: Kind, children: Vec<Node>, pixel_mask: Option<model::Image>) -> Node {
        Node {
            class: crate::stream::Tag::of(b"Rstr"),
            name: String::new(),
            visible: true,
            locked: false,
            opacity: 1.0,
            blend: model::Blend::Normal,
            kind,
            mask: None,
            pixel_mask,
            effects: vec![],
            children,
        }
    }

    fn rgba(width: u32, height: u32, px: &[[u8; 4]]) -> model::Image {
        model::Image { width, height, pixels: Pixels::Rgba8(px.concat()), transform: model::Affine::IDENTITY }
    }

    fn alpha_of(d: &Document, item: &Item) -> Vec<u8> {
        let Content::Graphic(g) = &item.content else { panic!("not an image") };
        let png = &d.assets[&g.asset].data;
        image::load_from_memory(png).unwrap().to_rgba8().pixels().map(|p| p.0[3]).collect()
    }

    #[test]
    fn pixel_masks_and_parent_pixels_clip_images() {
        // A 2 × 2 parent whose right column is transparent, masked to hide its bottom row; a child
        // image covering it twice as large (each child pixel is half a parent pixel).
        const O: [u8; 4] = [10, 20, 30, 255];
        const T: [u8; 4] = [10, 20, 30, 0];
        let parent = rgba(2, 2, &[O, T, O, T]);
        let mask = rgba(2, 2, &[[255, 255, 255, 255], [255, 255, 255, 255], [0, 0, 0, 255], [0, 0, 0, 255]]);
        let mut child = rgba(4, 4, &[O; 16]);
        child.transform = model::Affine([0.5, 0.0, 0.0, 0.5, 0.0, 0.0]);
        let tree = node(Kind::Image(parent), vec![node(Kind::Image(child), vec![], None)], Some(mask));
        let src = model::Document {
            dpi: 72.0,
            spreads: vec![model::Spread {
                bounds: model::Rect { x0: 0.0, y0: 0.0, x1: 100.0, y1: 100.0 },
                pages: vec![model::Rect { x0: 0.0, y0: 0.0, x1: 100.0, y1: 100.0 }],
                transparent: false,
                nodes: vec![tree],
            }],
            masters: vec![],
            saved_by: None,
            warnings: vec![],
        };
        let imported = Builder::new(&src).build(&src).unwrap();
        let d = &imported.document;
        let group = d.spreads[0].items.first().unwrap();
        let Content::Group { items } = &group.content else { panic!("parent and child form a group") };
        assert_eq!(alpha_of(d, &items[0]), [255, 0, 0, 0], "the mask hides the parent's bottom row");
        let child = alpha_of(d, &items[1]);
        assert_eq!(child[..4], [255, 255, 0, 0], "top row: shown over the parent's opaque pixel only");
        assert!(child[8..].iter().all(|a| *a == 0), "bottom rows: masked away with the parent");
        assert!(imported.warnings.iter().all(|w| !w.contains("pixel mask")), "{:?}", imported.warnings);
    }

    fn one_page(nodes: Vec<Node>) -> model::Document {
        model::Document {
            dpi: 72.0,
            spreads: vec![model::Spread {
                bounds: model::Rect { x0: 0.0, y0: 0.0, x1: 100.0, y1: 100.0 },
                pages: vec![model::Rect { x0: 0.0, y0: 0.0, x1: 100.0, y1: 100.0 }],
                transparent: false,
                nodes,
            }],
            masters: vec![],
            saved_by: None,
            warnings: vec![],
        }
    }

    #[test]
    fn open_curves_are_filled_like_affinity_fills_them() {
        let p = |x: f64, y: f64| model::Point { x, y };
        let line = |a: model::Point, b: model::Point| [a, b, b];
        // An open "V" with a fill: Affinity fills it as if closed.
        let open = model::Path {
            subpaths: vec![model::SubPath {
                start: p(0.0, 0.0),
                segments: vec![line(p(0.0, 0.0), p(50.0, 50.0)), line(p(50.0, 50.0), p(100.0, 0.0))],
                closed: false,
            }],
        };
        let blue = Paint::Solid(paint::Color::Rgb { r: 0.0, g: 0.0, b: 1.0, a: 1.0 });
        let black = paint::Stroke {
            paint: Paint::Solid(paint::Color::Rgb { r: 0.0, g: 0.0, b: 0.0, a: 1.0 }),
            width: 2.0,
            cap: paint::Cap::Butt,
            join: paint::Join::Miter,
            miter_limit: 4.0,
            align: paint::Align::Center,
            dash: None,
            behind: false,
        };
        let shape =
            |strokes: Vec<paint::Stroke>| node(Kind::Shape { path: open.clone(), fills: vec![blue.clone()], strokes, even_odd: false }, vec![], None);
        let src = one_page(vec![shape(vec![]), shape(vec![black])]);
        let d = Builder::new(&src).build(&src).unwrap().document;
        let items = &d.spreads[0].items;
        assert!(items[0].path.is_closed() && !items[0].fill.is_none(), "fill only: the outline is closed");
        let Content::Group { items: parts } = &items[1].content else { panic!("fill and stroke: a group") };
        assert!(parts[0].path.is_closed() && !parts[0].fill.is_none() && parts[0].stroke.is_none(), "the closed fill");
        assert!(!parts[1].path.is_closed() && parts[1].fill.is_none() && !parts[1].stroke.is_none(), "the open stroke");
    }

    #[test]
    fn affine_inverse_round_trips() {
        let m = model::Affine([2.0, 0.5, -1.0, 3.0, 10.0, -4.0]);
        let p = model::Point { x: 3.0, y: 7.0 };
        let q = invert(m).unwrap().apply(m.apply(p));
        assert!((q.x - p.x).abs() < 1e-9 && (q.y - p.y).abs() < 1e-9);
        assert!(invert(model::Affine([0.0; 6])).is_none());
        assert!(invert(model::Affine([f64::NAN, 0.0, 0.0, 1.0, 0.0, 0.0])).is_none());
    }

    #[test]
    fn hostile_pixel_buffers_are_refused_not_indexed() {
        assert!(png(4, 4, &[0; 10]).is_none());
        assert!(png(u32::MAX, u32::MAX, &[]).is_none());
        assert!(png(1, 1, &[1, 2, 3, 4]).is_some());
    }
}
