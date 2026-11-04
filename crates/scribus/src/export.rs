//! Document → Scribus XML.

use designcraft_color::{Color, ColorType, SwatchValue, swatch};
use designcraft_doc::{
    CharProps, Content, Document, Item, ItemId, Layer, LayerId, Leading, PageSide, ParaFormat, ParaProps, Shape, Spread, SpreadId, Story, StoryId,
    StrokeType, TextFrameOptions, VerticalJustification, story as st,
};
use designcraft_geom::{Affine, PathData, Point, Rect, Unit};
use std::collections::{HashMap, HashSet};

use crate::VERSION;
use crate::xml::{El, document};

/// Vertical space between spreads, in points. Scribus lays spreads out from this document
/// attribute rather than from the pages' `PageYPosition`, so it must match the coordinates the
/// items are written at.
const SPREAD_GAP: f64 = 40.0;

/// Export options.
#[derive(Clone, Debug, Default)]
pub struct ExportOptions {
    /// Folder (relative to the `.sla` file) where placed images are written. `None` puts them next
    /// to the document.
    pub image_folder: Option<String>,
}

/// A file the caller must write next to the `.sla` file for its image links to resolve.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ExportedImage {
    /// Path as written in `ImageFileName` (relative, `/` separated).
    pub path: String,
    pub data: Vec<u8>,
}

/// The `.sla` bytes and the image files they link to.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Exported {
    pub data: Vec<u8>,
    pub images: Vec<ExportedImage>,
}

/// Export a document as a Scribus 1.7 `.sla` file.
pub fn export_scribus(doc: &Document) -> Exported {
    export_scribus_with(doc, &ExportOptions::default())
}

/// Export with options.
pub fn export_scribus_with(doc: &Document, opts: &ExportOptions) -> Exported {
    Ex::new(doc, opts).build()
}

/// Where an object being exported sits: its absolute transform, page, master page and group.
struct ObjCtx<'a> {
    abs: Affine,
    own_page: i64,
    master: Option<&'a str>,
    group: Option<Point>,
    depth: usize,
}

struct Ex<'a> {
    d: &'a Document,
    opts: &'a ExportOptions,
    images: Vec<ExportedImage>,
    image_paths: HashMap<designcraft_doc::AssetId, String>,
    used_paths: HashSet<String>,
    ids: HashMap<ItemId, i64>,
    next_id: i64,
    layer_number: HashMap<LayerId, i64>,
    /// Spread → its page offset in Scribus's document coordinate space.
    spread_y: HashMap<SpreadId, f64>,
    /// Parent spread → (page index → master page name).
    master_names: HashMap<SpreadId, Vec<String>>,
    /// Item → (previous, next) in its text thread.
    threads: HashMap<ItemId, (Option<ItemId>, Option<ItemId>)>,
    /// The frame holding each story's text.
    heads: HashSet<ItemId>,
    /// Item → story, for text frames.
    item_story: HashMap<ItemId, StoryId>,
    /// Page (spread, index) → Scribus page number (0-based).
    page_number: HashMap<(usize, usize), i64>,
    warnings: Vec<&'static str>,
}

impl<'a> Ex<'a> {
    fn new(d: &'a Document, opts: &'a ExportOptions) -> Self {
        let mut layer_number = HashMap::new();
        // DesignCraft lists the frontmost layer first; Scribus numbers layers bottom to top.
        let n = d.layers.len();
        for (i, layer) in d.layers.iter().enumerate() {
            layer_number.insert(layer.id, (n - 1 - i) as i64);
        }
        let mut spread_y = HashMap::new();
        let mut y = 20.0;
        for s in &d.spreads {
            spread_y.insert(s.id, y);
            let h = s.pages.iter().map(|p| p.height).fold(0.0_f64, f64::max);
            y += h + SPREAD_GAP;
        }
        let mut page_number = HashMap::new();
        let mut number = 0i64;
        for (si, s) in d.spreads.iter().enumerate() {
            for pi in 0..s.pages.len() {
                page_number.insert((si, pi), number);
                number += 1;
            }
        }
        let mut master_names = HashMap::new();
        for parent in &d.parents {
            let base = parent.parent.as_ref().map(|p| p.name.clone()).unwrap_or_else(|| "Master".into());
            let names = if parent.pages.len() <= 1 { vec![base] } else { (0..parent.pages.len()).map(|i| format!("{base} {}", i + 1)).collect() };
            master_names.insert(parent.id, names);
        }
        let mut threads = HashMap::new();
        let mut heads = HashSet::new();
        let mut item_story = HashMap::new();
        for story in d.stories.values() {
            for (i, f) in story.frames.iter().enumerate() {
                item_story.insert(*f, story.id);
                let prev = i.checked_sub(1).and_then(|p| story.frames.get(p).copied());
                let next = story.frames.get(i + 1).copied();
                threads.insert(*f, (prev, next));
            }
            if let Some(h) = story.frames.first() {
                heads.insert(*h);
            }
        }
        Ex {
            d,
            opts,
            images: Vec::new(),
            image_paths: HashMap::new(),
            used_paths: HashSet::new(),
            ids: HashMap::new(),
            next_id: 1_000_000,
            layer_number,
            spread_y,
            master_names,
            threads,
            heads,
            item_story,
            page_number,
            warnings: Vec::new(),
        }
    }

    fn build(mut self) -> Exported {
        self.assign_ids();
        let mut doc_el = El::new("Document");
        self.write_document_attributes(&mut doc_el);
        self.write_colors(&mut doc_el);
        doc_el.push(El::new("Hyphenator"));
        self.write_styles(&mut doc_el);
        self.write_layers(&mut doc_el);
        self.write_sections(&mut doc_el);
        self.write_masters(&mut doc_el);
        self.write_pages(&mut doc_el);
        self.write_master_objects(&mut doc_el);
        self.write_page_items(&mut doc_el);
        let root = El::new("SCRIBUSUTF8NEW").attr("Version", VERSION).child(doc_el);
        let data = document(&root);
        for w in &self.warnings {
            log::warn!("scribus export: {w}");
        }
        Exported { data, images: self.images }
    }

    fn warn(&mut self, key: &'static str) {
        if !self.warnings.contains(&key) {
            self.warnings.push(key);
        }
    }

    fn assign_ids(&mut self) {
        fn walk(ex: &mut Ex, item: &Item) {
            if !ex.ids.contains_key(&item.id) {
                ex.ids.insert(item.id, ex.next_id);
                ex.next_id += 1;
            }
            if let Content::Group { items } = &item.content {
                for c in items {
                    walk(ex, c);
                }
            }
        }
        for s in &self.d.spreads {
            for item in &s.items {
                walk(self, item);
            }
        }
        for s in &self.d.parents {
            for item in &s.items {
                walk(self, item);
            }
        }
    }

    fn write_document_attributes(&mut self, e: &mut El) {
        let settings = &self.d.settings;
        let first = self.d.spreads.first().and_then(|s| s.pages.first());
        let (width, height) = first.map_or((settings.page_width, settings.page_height), |p| (p.width, p.height));
        let side = first.map(|p| p.side).unwrap_or(PageSide::Right);
        let margins = first.map(|p| p.margins).unwrap_or(designcraft_doc::Margins::uniform(0.0));
        let (left, right) = margin_sides(margins, side);
        let pages: usize = self.d.spreads.iter().map(|s| s.pages.len()).sum();
        let first_number = self.d.sections.first().and_then(|s| s.start_number).unwrap_or(1);
        e.set("PageCount", pages);
        e.set("PageWidth", num(width));
        e.set("PageHeight", num(height));
        e.set("BorderLeft", num(left));
        e.set("BorderRight", num(right));
        e.set("BorderTop", num(margins.top));
        e.set("BorderBottom", num(margins.bottom));
        e.set("MarginPreset", 0);
        let [bt, bb, bi, bo] = settings.bleed;
        let (bl, br) = if side == PageSide::Left { (bo, bi) } else { (bi, bo) };
        e.set("BleedTop", num(bt));
        e.set("BleedBottom", num(bb));
        e.set("BleedLeft", num(bl));
        e.set("BleedRight", num(br));
        e.set("PageOrientation", if width > height { 1 } else { 0 });
        e.set("PageSize", "Custom");
        e.set("FirstPageNumber", first_number);
        e.set("PagePositioning", if settings.facing_pages { 1 } else { 0 });
        // Scribus positions spreads from these, not from each page's `PageYPosition`.
        e.set("GapVertical", num(SPREAD_GAP));
        e.set("GapHorizontal", 0);
        e.set("UnitIndex", unit_index(settings.horizontal_units));
        e.set("BaselineGridSpacing", num(settings.baseline_grid.increment));
        e.set("BaselineGridOffset", num(settings.baseline_grid.start));
        // Scribus keeps the document's default text font on the Document element too.
        let (_, default_chars) = self.d.styles.resolve_para_style(&self.d.styles.default_paragraph);
        e.set("FontFace", font_name(&default_chars.font_family, &default_chars.font_style));
        e.set("FontSize", num(default_chars.size.max(0.1)));
        e.set("ActiveLayer", self.layer_number.get(&self.d.layers.first().map(|l| l.id).unwrap_or(LayerId(0))).copied().unwrap_or(0));
        let info = &self.d.file_info;
        e.set("DocumentInfoTitle", &info.document_title);
        e.set("DocumentInfoAuthor", &info.author);
        e.set("DocumentInfoComments", &info.description);
        e.set("DocumentInfoKeywords", info.keywords.join(", "));
        e.set("DocumentInfoRights", &info.copyright_notice);
        e.set("DocumentInfoSubject", "");
        e.set("DocumentInfoPublisher", "");
        e.set("AutomaticHyphenation", 1);
        e.set("ShowGuides", 1);
        e.set("ShowMargins", 1);
        e.set("ShowImages", 1);
        e.set("ShowFrames", 1);
    }

    fn write_colors(&mut self, e: &mut El) {
        for sw in &self.d.swatches {
            let Some((space, attrs)) = color_attrs(&sw.value) else { continue };
            let mut el = El::new("Color").attr("Name", scribus_swatch(&sw.name)).attr("Space", space);
            for (k, v) in attrs {
                el.set(k, v);
            }
            if matches!(sw.value, SwatchValue::Color { color_type: ColorType::Spot, .. }) {
                el.set("Spot", "1");
            }
            e.push(el);
        }
    }

    fn write_styles(&mut self, e: &mut El) {
        let styles = &self.d.styles;
        let mut any = false;
        for sw in &styles.character {
            let mut el = El::new("CharacterStyle").attr("Name", &sw.name);
            if sw.name == styles.default_character {
                el.set("DefaultStyle", "1");
            }
            let mut cp = CharProps::default();
            cp.apply(&styles.char_style_attrs(&sw.name));
            put_char(&mut el, &cp);
            e.push(el);
            any = true;
        }
        if !any {
            let mut el = El::new("CharacterStyle").attr("Name", &styles.default_character).attr("DefaultStyle", "1");
            put_char(&mut el, &CharProps::default());
            e.push(el);
        }
        let mut any = false;
        for ps in &styles.paragraph {
            let mut el = El::new("ParagraphStyle").attr("Name", &ps.name);
            if ps.name == styles.default_paragraph {
                el.set("DefaultStyle", "1");
            }
            let (para, chars) = styles.resolve_para_style(&ps.name);
            put_para(&mut el, &para, &chars);
            put_char(&mut el, &chars);
            e.push(el);
            any = true;
        }
        if !any {
            let mut el = El::new("ParagraphStyle").attr("Name", &styles.default_paragraph).attr("DefaultStyle", "1");
            put_para(&mut el, &ParaProps::default(), &CharProps::default());
            put_char(&mut el, &CharProps::default());
            e.push(el);
        }
    }

    fn write_layers(&mut self, e: &mut El) {
        let mut layers: Vec<&Layer> = self.d.layers.iter().collect();
        layers.reverse(); // Scribus numbers bottom to top
        for (i, layer) in layers.iter().enumerate() {
            let el = El::new("Layers")
                .attr("Number", i)
                // Scribus uses `Level` as the layer's stacking position and `Number` as its id;
                // duplicate levels make it lose the layers (and everything drawn on them).
                .attr("Level", i)
                .attr("Name", &layer.name)
                .attr("IsViewable", bool_num(layer.visible))
                .attr("IsPrintable", bool_num(layer.printable))
                .attr("IsEditable", bool_num(!layer.locked))
                .attr("IsSelectable", bool_num(!layer.locked))
                .attr("FlowControl", 1)
                .attr("Transparency", 1)
                .attr("BlendMode", 0)
                .attr("LayerColor", hex(layer.color));
            e.push(el);
        }
    }

    fn write_sections(&mut self, e: &mut El) {
        let pages: usize = self.d.spreads.iter().map(|s| s.pages.len()).sum();
        let mut sections = self.d.sections.clone();
        sections.sort_by_key(|s| s.start);
        if sections.is_empty() {
            sections.push(designcraft_doc::Section {
                start: 0,
                start_number: Some(1),
                style: Default::default(),
                prefix: String::new(),
                marker: String::new(),
                include_prefix: false,
            });
        }
        let mut wrap = El::new("Sections");
        for (i, s) in sections.iter().enumerate() {
            let to = sections.get(i + 1).map(|n| n.start.saturating_sub(1)).unwrap_or(pages.saturating_sub(1));
            wrap.push(
                El::new("Section")
                    .attr("Number", i)
                    .attr("Name", if s.marker.is_empty() { format!("Section {}", i + 1) } else { s.marker.clone() })
                    .attr("From", s.start.min(pages.saturating_sub(1)))
                    .attr("To", to)
                    .attr("Type", "Type_1_2_3")
                    .attr("Start", s.start_number.unwrap_or(1))
                    .attr("Reversed", 0)
                    .attr("Active", 1)
                    .attr("FillChar", 0)
                    .attr("FieldWidth", 0),
            );
        }
        e.push(wrap);
    }

    fn write_masters(&mut self, e: &mut El) {
        let parents = self.d.parents.clone();
        // A master page's `PageNumber` is its index among all master pages, and the objects on it
        // name that number in `OwnPage`; every master sharing 0 attaches its objects to every page.
        let mut number = 0i64;
        for parent in &parents {
            let names = self.master_names.get(&parent.id).cloned().unwrap_or_else(|| vec!["Master".into()]);
            for (pi, page) in parent.pages.iter().enumerate() {
                let name = names.get(pi).cloned().unwrap_or_else(|| names[0].clone());
                let (left, right) = margin_sides(page.margins, page.side);
                e.push(
                    El::new("MasterPage")
                        .attr("PageXPosition", num(100.0))
                        .attr("PageYPosition", num(20.0))
                        .attr("PageWidth", num(page.width))
                        .attr("PageHeight", num(page.height))
                        .attr("BorderLeft", num(left))
                        .attr("BorderRight", num(right))
                        .attr("BorderTop", num(page.margins.top))
                        .attr("BorderBottom", num(page.margins.bottom))
                        .attr("PageNumber", number)
                        .attr("PageName", &name)
                        .attr("MasterPageName", "")
                        .attr("Size", "Custom")
                        .attr("Orientation", if page.width > page.height { 1 } else { 0 })
                        .attr("LeftPage", bool_num(page.side == PageSide::Left))
                        .attr("Preset", 0)
                        .attr("VerticalGuides", guide_list(&page.guides, true, page.x))
                        .attr("HorizontalGuides", guide_list(&page.guides, false, page.x)),
                );
                number += 1;
            }
        }
    }

    fn write_master_objects(&mut self, e: &mut El) {
        let parents = self.d.parents.clone();
        let mut number = 0i64;
        for parent in &parents {
            let names = self.master_names.get(&parent.id).cloned().unwrap_or_else(|| vec!["Master".into()]);
            for (pi, _page) in parent.pages.iter().enumerate() {
                let name = names.get(pi).cloned().unwrap_or_else(|| names[0].clone());
                for item in &parent.items {
                    if page_for(item, parent) != pi {
                        continue;
                    }
                    let abs = Affine::translate((100.0, 20.0)) * item.xf;
                    let mut el = El::new("MasterObject");
                    self.write_object_attrs(&mut el, item, ObjCtx { abs, own_page: number, master: Some(&name), group: None, depth: 0 });
                    e.push(el);
                }
                number += 1;
            }
        }
    }

    fn write_pages(&mut self, e: &mut El) {
        let spreads = self.d.spreads.clone();
        let mut number = 0i64;
        for spread in &spreads {
            let y = self.spread_y.get(&spread.id).copied().unwrap_or(0.0);
            for page in &spread.pages {
                let (left, right) = margin_sides(page.margins, page.side);
                let master = page.parent.and_then(|id| self.master_name(id, page.side));
                e.push(
                    El::new("Page")
                        .attr("PageXPosition", num(page_origin_x(page.side, page.width, self.d.settings.facing_pages)))
                        .attr("PageYPosition", num(y))
                        .attr("PageWidth", num(page.width))
                        .attr("PageHeight", num(page.height))
                        .attr("BorderLeft", num(left))
                        .attr("BorderRight", num(right))
                        .attr("BorderTop", num(page.margins.top))
                        .attr("BorderBottom", num(page.margins.bottom))
                        .attr("PageNumber", number)
                        .attr("PageName", "")
                        .attr("MasterPageName", master.unwrap_or_default())
                        .attr("Size", "Custom")
                        .attr("Orientation", if page.width > page.height { 1 } else { 0 })
                        .attr("LeftPage", bool_num(page.side == PageSide::Left))
                        .attr("Preset", 0)
                        .attr("VerticalGuides", guide_list(&page.guides, true, page.x))
                        .attr("HorizontalGuides", guide_list(&page.guides, false, page.x)),
                );
                number += 1;
            }
        }
    }

    fn write_page_items(&mut self, e: &mut El) {
        let spreads = self.d.spreads.clone();
        for (si, spread) in spreads.iter().enumerate() {
            let y = self.spread_y.get(&spread.id).copied().unwrap_or(0.0);
            for item in &spread.items {
                let pi = page_for(item, spread);
                let number = self.page_number.get(&(si, pi)).copied().unwrap_or(0);
                let page = &spread.pages[pi];
                let page_x = page_origin_x(page.side, page.width, self.d.settings.facing_pages);
                let origin = Affine::translate((page_x - page.x, y));
                let abs = origin * item.xf;
                let mut el = El::new("PageObject");
                self.write_object_attrs(&mut el, item, ObjCtx { abs, own_page: number, master: None, group: None, depth: 0 });
                e.push(el);
            }
        }
    }

    fn master_name(&self, id: SpreadId, side: PageSide) -> Option<String> {
        let names = self.master_names.get(&id)?;
        if names.len() == 1 {
            return names.first().cloned();
        }
        let parent = self.d.parents.iter().find(|p| p.id == id)?;
        let pi = parent.pages.iter().position(|p| p.side == side).unwrap_or(0);
        names.get(pi).cloned()
    }

    fn write_object_attrs(&mut self, el: &mut El, item: &Item, ctx: ObjCtx<'_>) {
        let ObjCtx { abs, own_page, master, group, depth } = ctx;
        if depth > crate::xml::MAX_DEPTH {
            self.warn("groups nested too deeply are exported without their inner objects");
            return;
        }
        let c = abs.as_coeffs();
        let rotation = c[1].atan2(c[0]).to_degrees();
        let sx = c[0].hypot(c[1]);
        let sy = c[2].hypot(c[3]);
        let shear = (c[0] * c[2] + c[1] * c[3]).abs();
        if shear > 1e-6 && sx > 1e-9 && sy > 1e-9 {
            self.warn("sheared objects are exported without the shear");
        }
        let path = if matches!(item.shape, Shape::GraphicLine) {
            let bounds = item.path.bounds().unwrap_or(Rect::new(0.0, 0.0, 1.0, 0.0));
            let len = bounds.width().hypot(bounds.height()).max(0.1);
            designcraft_geom::shapes::line(Point::ZERO, Point::new(len, 0.0))
        } else if item.is_group() && item.path.is_empty() {
            let b = item.children().iter().filter_map(|c| c.path.transformed(c.xf).bounds()).reduce(|a, b| a.union(b)).unwrap_or(Rect::ZERO);
            designcraft_geom::shapes::rectangle(Rect::new(b.x0, b.y0, b.x1, b.y1))
        } else {
            item.path.transformed(Affine::scale_non_uniform(sx.max(1e-9), sy.max(1e-9)))
        };
        let bounds = path.bounds().unwrap_or_else(|| Rect::new(0.0, 0.0, 0.0, 0.0));
        let (width, height) = if matches!(item.shape, Shape::GraphicLine) {
            (bounds.width().max(0.1), 1.0)
        } else {
            (bounds.width().max(0.0), bounds.height().max(0.0))
        };
        let item_type = item_type(item);
        let frame_type = frame_type(item);
        el.set("XPosition", num(c[4]));
        el.set("YPosition", num(c[5]));
        el.set("OwnPage", own_page);
        el.set("ItemID", self.ids.get(&item.id).copied().unwrap_or(0));
        el.set("ItemType", item_type);
        el.set("Width", num(width));
        el.set("Height", num(height));
        el.set("FrameType", frame_type);
        el.set("ClipEdited", 0);
        el.set("AutoName", if item.name.is_empty() { "Object" } else { &item.name });
        el.set("Rotation", num(rotation));
        el.set("Layer", self.layer_number.get(&item.layer).copied().unwrap_or(0));
        el.set("gWidth", 0);
        el.set("gHeight", 0);
        if let Some(local) = group {
            el.set("gXpos", num(local.x));
            el.set("gYpos", num(local.y));
        } else {
            el.set("gXpos", num(c[4]));
            el.set("gYpos", num(c[5]));
        }
        if let Some(name) = master {
            el.set("OnMasterPage", name);
        }
        let path_s = path_string(&path, width, height, item.shape);
        el.set("Path", &path_s);
        el.set("ContourLinePath", &path_s);
        self.put_fill(el, item);
        self.put_stroke(el, item);
        if item.opacity < 1.0 {
            el.set("FillTransparency", num(item.opacity as f64 * 100.0));
        }
        if item.blend != designcraft_color::BlendMode::Normal {
            el.set("FillBlendMode", blend_number(item.blend));
        }
        let (prev, next) = self.threads.get(&item.id).copied().unwrap_or((None, None));
        el.set("BackItem", prev.and_then(|p| self.ids.get(&p)).copied().unwrap_or(-1));
        el.set("NextItem", next.and_then(|n| self.ids.get(&n)).copied().unwrap_or(-1));
        match &item.content {
            Content::Text(tf) => {
                self.put_text_frame(el, &tf.options);
                let story = self.item_story.get(&item.id).and_then(|id| self.d.stories.get(id)).cloned();
                let head = self.heads.contains(&item.id);
                if let Some(s) = story
                    && head
                {
                    el.push(self.story_element(&s));
                }
            }
            Content::Graphic(g) => {
                if let Some(asset) = self.d.assets.get(&g.asset).cloned()
                    && let Some(name) = self.image_for(&asset)
                {
                    el.set("ImageFileName", name);
                    self.put_image_geometry(el, g, item);
                }
            }
            Content::Group { items } => {
                for child in items {
                    let child_abs = abs * child.xf;
                    let local = Point::new(child.xf.as_coeffs()[4], child.xf.as_coeffs()[5]);
                    let mut child_el = El::new(if master.is_some() { "MasterObject" } else { "PageObject" });
                    self.write_object_attrs(&mut child_el, child, ObjCtx { abs: child_abs, own_page, master, group: Some(local), depth: depth + 1 });
                    el.push(child_el);
                }
            }
            Content::Unassigned => {}
        }
    }

    fn put_text_frame(&mut self, el: &mut El, o: &TextFrameOptions) {
        el.set("Columns", o.columns.max(1));
        el.set("ColumnGap", num(o.gutter));
        el.set("AutomaticTextFrame", bool_num(o.auto_size == designcraft_doc::AutoSize::HeightOnly));
        el.set("TextToFrameDistanceLeft", num(o.inset[1]));
        el.set("TextToFrameDistanceTop", num(o.inset[0]));
        el.set("TextToFrameDistanceBottom", num(o.inset[2]));
        el.set("TextToFrameDistanceRight", num(o.inset[3]));
        el.set(
            "VerticalAlignment",
            match o.vertical_justification {
                VerticalJustification::Top => 0,
                VerticalJustification::Center => 1,
                VerticalJustification::Bottom => 2,
                VerticalJustification::Justify => 3,
            },
        );
        el.set("FirstLineOffset", 1);
        el.set("PathTextType", if o.path.is_some() { 1 } else { 0 });
        el.set("TextPathFlipped", bool_num(o.path.as_ref().is_some_and(|p| p.flip)));
        el.set("PathTextShowPath", 0);
    }

    fn image_for(&mut self, asset: &designcraft_doc::Asset) -> Option<String> {
        let data = asset.data.to_vec();
        if data.is_empty() {
            // A link-only placement keeps its original path.
            return asset.link.clone();
        }
        if let Some(path) = self.image_paths.get(&asset.id) {
            return Some(path.clone());
        }
        let base = sanitize_name(if asset.name.is_empty() { "image" } else { &asset.name });
        let p = std::path::Path::new(&base);
        let (stem, ext) = match (p.file_stem().and_then(|s| s.to_str()), p.extension().and_then(|e| e.to_str())) {
            (Some(stem), Some(ext)) => (stem.to_string(), ext.to_string()),
            _ => (base, extension(&asset.mime).to_string()),
        };
        let folder = match &self.opts.image_folder {
            Some(folder) => {
                let clean: Vec<String> =
                    folder.split(['/', '\\']).filter(|part| !part.is_empty() && *part != "." && *part != "..").map(sanitize_name).collect();
                if clean.is_empty() { String::new() } else { format!("{}/", clean.join("/")) }
            }
            None => String::new(),
        };
        let mut name = format!("{stem}.{ext}");
        let mut n = 2;
        while self.used_paths.contains(&format!("{folder}{name}")) {
            name = format!("{stem}-{n}.{ext}");
            n += 1;
        }
        let path = format!("{folder}{name}");
        self.used_paths.insert(path.clone());
        self.image_paths.insert(asset.id, path.clone());
        self.images.push(ExportedImage { path: path.clone(), data });
        Some(path)
    }

    fn put_image_geometry(&mut self, el: &mut El, g: &designcraft_doc::Graphic, item: &Item) {
        // Scribus applies the camera's EXIF orientation when it loads the picture. A graphic whose
        // transform was built in raw pixel space (as the importer does) already carries that
        // uprighting, so divide it out before writing the scale, offset and rotation.
        let mut xf = g.xf;
        if let Some(asset) = self.d.assets.get(&g.asset)
            && let Some((iw, ih)) = asset.pixels
            && (g.size.0 - f64::from(iw)).abs() < 1e-6
            && (g.size.1 - f64::from(ih)).abs() < 1e-6
        {
            let o = designcraft_images::exif_orientation(&asset.data);
            if o != 1 {
                xf *= exif_inverse(o, g.size.0, g.size.1);
            }
        }
        let c = xf.as_coeffs();
        let (pw, ph) = g.size;
        let scale_x = c[0].hypot(c[1]);
        let scale_y = c[2].hypot(c[3]);
        let (ix, iy) = self.d.assets.get(&g.asset).and_then(|a| a.pixels).map(|(w, h)| (w as f64, h as f64)).unwrap_or((pw.max(1.0), ph.max(1.0)));
        let px_to_frame_x = if ix > 0.0 { pw / ix } else { 1.0 };
        let px_to_frame_y = if iy > 0.0 { ph / iy } else { 1.0 };
        let sx = scale_x * px_to_frame_x;
        let sy = scale_y * px_to_frame_y;
        let ox = if sx.abs() > 1e-12 { c[4] / sx } else { 0.0 };
        let oy = if sy.abs() > 1e-12 { c[5] / sy } else { 0.0 };
        let rot = c[1].atan2(c[0]).to_degrees();
        let _ = item;
        el.set("ImageScaleX", num(sx.max(1e-9)));
        el.set("ImageScaleY", num(sy.max(1e-9)));
        el.set("ImageOffsetX", num(ox));
        el.set("ImageOffsetY", num(oy));
        el.set("ImageRotation", num(rot));
        el.set("ImageVisible", 1);
        el.set("ImageScaleType", 1);
        el.set("ImageRatio", 1);
        el.set("ImagePageNumber", 0);
    }

    fn put_fill(&mut self, el: &mut El, item: &Item) {
        let (base, tint) = resolve_tint(self.d, &item.fill.swatch);
        if swatch_is_none(&base) {
            return;
        }
        el.set("FillColor", scribus_swatch(&base));
        el.set("FillShade", num(tint as f64 * item.fill.tint as f64 * 100.0));
    }

    fn put_stroke(&mut self, el: &mut El, item: &Item) {
        let (base, tint) = resolve_tint(self.d, &item.stroke.swatch);
        if swatch_is_none(&base) || item.stroke.weight <= 0.0 {
            return;
        }
        el.set("LineColor", scribus_swatch(&base));
        el.set("LineShade", num(tint as f64 * item.stroke.tint as f64 * 100.0));
        el.set("LineWidth", num(item.stroke.weight));
        el.set(
            "LinePenStyle",
            match item.stroke.kind {
                StrokeType::Solid | StrokeType::Style { .. } => 1,
                StrokeType::Dotted => 3,
                _ => 2,
            },
        );
    }

    fn story_element(&mut self, story: &Story) -> El {
        let mut e = El::new("StoryText");
        let styles = &self.d.styles;
        let first_style = para_style(story, 0, styles);
        let (_, cp) = styles.resolve_para(&ParaFormat { style: first_style.clone(), ..Default::default() });
        let mut ds = El::new("DefaultStyle").attr("Parent", &first_style);
        put_char(&mut ds, &cp);
        e.push(ds);
        let text = &story.text;
        let mut pos = 0usize;
        let mut para = 0usize;
        for run in &story.chars {
            let end = pos.saturating_add(run.len).min(text.len());
            if end <= pos {
                continue;
            }
            let slice = match text.get(pos..end) {
                Some(s) => s,
                None => {
                    // A run that cuts a character in half: extend to the next boundary.
                    let mut e2 = end;
                    while e2 < text.len() && !text.is_char_boundary(e2) {
                        e2 += 1;
                    }
                    text.get(pos..e2).unwrap_or("")
                }
            };
            let style = para_style(story, para, styles);
            let mut pf = story.paras.get(para).cloned().unwrap_or_default();
            pf.style = style.clone();
            let (_, base) = styles.resolve_para(&pf);
            let chars = styles.resolve_char(&base, &run.format);
            let mut buf = String::new();
            for ch in slice.chars() {
                match ch {
                    '\n' => {
                        push_content(&mut e, &buf, run, &chars);
                        buf.clear();
                        e.push(El::new("para").attr("Parent", &style));
                        para = para.saturating_add(1);
                    }
                    '\t' => {
                        push_content(&mut e, &buf, run, &chars);
                        buf.clear();
                        e.push(El::new("tab"));
                    }
                    st::PAGE_NUMBER => {
                        push_content(&mut e, &buf, run, &chars);
                        buf.clear();
                        e.push(El::new("var").attr("name", "pgno"));
                    }
                    c if unsupported_mark(c) => {
                        self.warn("special text markers (footnotes, tables, anchors, breaks) are not exported");
                    }
                    c if (c as u32) < 0x20 || c == '\u{2028}' || c == '\u{2029}' => {
                        push_content(&mut e, &buf, run, &chars);
                        buf.clear();
                        let mut u = El::new("Content").attr("Unicode", c as u32);
                        if !run.format.style.is_empty() && run.format.style != designcraft_doc::NO_CHAR_STYLE {
                            u.set("CParent", &run.format.style);
                        }
                        put_char(&mut u, &chars);
                        e.push(u);
                    }
                    c => buf.push(c),
                }
            }
            push_content(&mut e, &buf, run, &chars);
            pos = end;
        }
        let last = para_style(story, para.min(story.paras.len().saturating_sub(1)), styles);
        e.push(El::new("trail").attr("Parent", last));
        e
    }
}

/// Text markers the `.sla` format has no equivalent for. Dropping one keeps the story text and
/// its runs valid; the exported document simply has the surrounding text.
fn unsupported_mark(c: char) -> bool {
    matches!(
        c,
        st::SECTION_MARKER
            | st::COLUMN_BREAK
            | st::FRAME_BREAK
            | st::PAGE_BREAK
            | st::ODD_PAGE_BREAK
            | st::EVEN_PAGE_BREAK
            | st::INDENT_HERE
            | st::RIGHT_INDENT_TAB
            | st::NEXT_PAGE_NUMBER
            | st::PREV_PAGE_NUMBER
            | st::TABLE_ANCHOR
            | designcraft_doc::notes::FOOTNOTE_REF
            | designcraft_doc::endnotes::ENDNOTE_REF
            | designcraft_doc::xref::ANCHOR_MARK
            | designcraft_doc::xref::XREF_MARK
            | designcraft_doc::anchored::OBJECT_MARK
            | designcraft_doc::index::INDEX_MARK
    )
}

fn push_content(e: &mut El, text: &str, run: &designcraft_doc::CharRun, chars: &CharProps) {
    if text.is_empty() {
        return;
    }
    let mut el = El::new("Content").attr("Chars", text);
    if !run.format.style.is_empty() && run.format.style != designcraft_doc::NO_CHAR_STYLE {
        el.set("CParent", &run.format.style);
    }
    put_char(&mut el, chars);
    e.push(el);
}

fn para_style(story: &Story, index: usize, styles: &designcraft_doc::Styles) -> String {
    let name = story.paras.get(index).map(|p| p.style.clone()).unwrap_or_else(|| styles.default_paragraph.clone());
    let name = if name.is_empty() { styles.default_paragraph.clone() } else { name };
    if styles.para(&name).is_some() {
        name
    } else if styles.para(&styles.default_paragraph).is_some() {
        styles.default_paragraph.clone()
    } else {
        designcraft_doc::BASIC_PARAGRAPH.to_string()
    }
}

// ---------- attributes ----------

fn put_char(el: &mut El, c: &CharProps) {
    el.set("Font", font_name(&c.font_family, &c.font_style));
    el.set("FontSize", num(c.size.max(0.1)));
    el.set("FontColor", scribus_swatch(&c.fill));
    el.set("FillShade", num(c.fill_tint as f64 * 100.0));
    if !swatch_is_none(&c.stroke) {
        el.set("StrokeColor", scribus_swatch(&c.stroke));
        el.set("StrokeShade", num(c.stroke_tint as f64 * 100.0));
        el.set("TextOutlineWidth", num(c.stroke_weight.max(0.0)));
    } else {
        el.set("StrokeColor", "None");
    }
    el.set("ScaleHorizontal", num(c.h_scale * 100.0));
    el.set("ScaleVertical", num(c.v_scale * 100.0));
    let baseline = if c.size.abs() > 1e-9 { c.baseline_shift / c.size * 100.0 } else { 0.0 };
    el.set("BaselineOffset", num(baseline));
    el.set("Kerning", num(c.tracking));
    el.set("Language", "en");
}

fn put_para(el: &mut El, p: &ParaProps, chars: &CharProps) {
    el.set("Alignment", align_number(p.align));
    el.set("LeftMargin", num(p.left_indent));
    el.set("RightMargin", num(p.right_indent));
    el.set("FirstIndent", num(p.first_line_indent));
    el.set("GapBefore", num(p.space_before));
    el.set("GapAfter", num(p.space_after));
    el.set("HasDropCap", if p.drop_cap_lines > 0 { 1 } else { 0 });
    el.set("DropCapLines", p.drop_cap_lines.max(2));
    el.set("DropCapChars", p.drop_cap_chars);
    let leading = match chars.leading {
        Leading::Points(v) => v,
        Leading::Auto => chars.size * p.auto_leading,
    };
    el.set("LineSpacingMode", 0);
    el.set("LineSpacing", num(leading.max(0.1)));
}

fn align_number(a: designcraft_doc::Align) -> i64 {
    use designcraft_doc::Align;
    match a {
        Align::Left | Align::TowardsSpine => 0,
        Align::Center => 1,
        Align::Right | Align::AwayFromSpine => 2,
        Align::FullyJustified => 3,
        Align::LeftJustified => 5,
        Align::CenterJustified => 6,
        Align::RightJustified => 7,
    }
}

/// The inverse of the importer's EXIF uprighting: map upright pixel coordinates back to the raw
/// ones Scribus stores, so `xf * inverse` is the transform Scribus should apply itself.
fn exif_inverse(o: u8, w: f64, h: f64) -> Affine {
    match o {
        2 => Affine::new([-1.0, 0.0, 0.0, 1.0, w, 0.0]),
        3 => Affine::new([-1.0, 0.0, 0.0, -1.0, w, h]),
        4 => Affine::new([1.0, 0.0, 0.0, -1.0, 0.0, h]),
        5 => Affine::new([0.0, 1.0, 1.0, 0.0, 0.0, 0.0]),
        6 => Affine::new([0.0, -1.0, 1.0, 0.0, 0.0, h]),
        7 => Affine::new([0.0, -1.0, -1.0, 0.0, h, w]),
        8 => Affine::new([0.0, 1.0, -1.0, 0.0, w, 0.0]),
        _ => Affine::IDENTITY,
    }
}

fn font_name(family: &str, style: &str) -> String {
    // Scribus names a face by family and style, including the default styles: "DejaVu Sans
    // Book" and "Geist Regular", not "DejaVu Sans" or "Geist".
    let (family, style) = (family.trim(), style.trim());
    if style.is_empty() || family.eq_ignore_ascii_case(style) || family.to_lowercase().ends_with(&format!(" {}", style.to_lowercase())) {
        family.to_string()
    } else {
        format!("{family} {style}")
    }
}

fn scribus_swatch(name: &str) -> String {
    match name {
        swatch::NONE => "None".into(),
        swatch::REGISTRATION => "Registration".into(),
        swatch::PAPER => "White".into(),
        swatch::BLACK => "Black".into(),
        other => other.into(),
    }
}

fn swatch_is_none(name: &str) -> bool {
    name.is_empty() || name == swatch::NONE || name == "None"
}

fn resolve_tint(d: &Document, name: &str) -> (String, f32) {
    let mut n = name.to_string();
    let mut tint = 1.0f32;
    for _ in 0..8 {
        let Some(s) = d.swatches.iter().find(|s| s.name == n) else { break };
        match &s.value {
            SwatchValue::Tint { base, tint: t } => {
                tint *= *t;
                n = base.clone();
            }
            _ => break,
        }
    }
    (n, tint)
}

fn color_attrs(value: &SwatchValue) -> Option<(&'static str, Vec<(&'static str, String)>)> {
    let color = match value {
        SwatchValue::Color { color, .. } | SwatchValue::Paper { color } => color,
        _ => return None,
    };
    Some(match *color {
        Color::Rgb { r, g, b } => ("RGB", vec![("R", num(r as f64 * 255.0)), ("G", num(g as f64 * 255.0)), ("B", num(b as f64 * 255.0))]),
        Color::Cmyk { c, m, y, k } => {
            ("CMYK", vec![("C", num(c as f64 * 100.0)), ("M", num(m as f64 * 100.0)), ("Y", num(y as f64 * 100.0)), ("K", num(k as f64 * 100.0))])
        }
        Color::Gray { k } => ("Gray", vec![("G", num(k as f64 * 100.0))]),
    })
}

fn item_type(item: &Item) -> i64 {
    match &item.content {
        Content::Text(_) => 4,
        Content::Graphic(_) => 2,
        Content::Group { .. } => 12,
        Content::Unassigned if matches!(item.shape, Shape::GraphicLine) => 5,
        _ => 6,
    }
}

fn blend_number(b: designcraft_color::BlendMode) -> i64 {
    use designcraft_color::BlendMode;
    match b {
        BlendMode::Normal => 0,
        BlendMode::Darken => 1,
        BlendMode::Multiply => 2,
        BlendMode::Screen | BlendMode::Lighten => 3,
        BlendMode::Overlay => 4,
        BlendMode::HardLight => 5,
        BlendMode::SoftLight => 6,
        BlendMode::Difference => 7,
        BlendMode::Exclusion => 8,
        BlendMode::ColorDodge => 9,
        BlendMode::ColorBurn => 10,
        BlendMode::Hue => 11,
        BlendMode::Saturation => 12,
        BlendMode::Color => 13,
        BlendMode::Luminosity => 14,
    }
}

fn frame_type(item: &Item) -> i64 {
    match item.shape {
        Shape::Oval => 1,
        Shape::Polygon | Shape::Path => 3,
        _ => 0,
    }
}

fn path_string(path: &PathData, width: f64, height: f64, shape: Shape) -> String {
    match shape {
        Shape::Rectangle | Shape::Group => rectangle_path(width.max(0.1), height.max(0.1)),
        Shape::Oval => ellipse_path(width.max(0.1), height.max(0.1)),
        _ => {
            let mut out = String::new();
            for (i, sp) in path.subpaths.iter().enumerate() {
                if i > 0 {
                    out.push(' ');
                }
                let Some(first) = sp.anchors.first() else { continue };
                out.push_str(&format!("M{} {}", num(first.p.x), num(first.p.y)));
                for (j, a) in sp.anchors.iter().enumerate().skip(1) {
                    let prev = &sp.anchors[j - 1];
                    if !prev.has_out() && !a.has_in() {
                        out.push_str(&format!(" L{} {}", num(a.p.x), num(a.p.y)));
                    } else {
                        out.push_str(&format!(
                            " C{} {} {} {} {} {}",
                            num(prev.h_out.x),
                            num(prev.h_out.y),
                            num(a.h_in.x),
                            num(a.h_in.y),
                            num(a.p.x),
                            num(a.p.y)
                        ));
                    }
                }
                if sp.closed {
                    if sp.anchors.len() > 1 {
                        let last = sp.anchors.last().unwrap_or(first);
                        if !last.has_out() && !first.has_in() {
                            out.push_str(&format!(" L{} {}", num(first.p.x), num(first.p.y)));
                        } else {
                            out.push_str(&format!(
                                " C{} {} {} {} {} {}",
                                num(last.h_out.x),
                                num(last.h_out.y),
                                num(first.h_in.x),
                                num(first.h_in.y),
                                num(first.p.x),
                                num(first.p.y)
                            ));
                        }
                    }
                    out.push_str(" Z");
                }
            }
            if out.is_empty() { rectangle_path(width.max(0.1), height.max(0.1)) } else { out }
        }
    }
}

fn rectangle_path(w: f64, h: f64) -> String {
    format!("M0 0 L{} 0 L{} {} L0 {} L0 0 Z", num(w), num(w), num(h), num(h))
}

fn ellipse_path(w: f64, h: f64) -> String {
    let (rx, ry) = (w / 2.0, h / 2.0);
    let (kx, ky) = (rx * 0.552_284_8, ry * 0.552_284_8);
    format!(
        "M{} 0 C{} 0 {} {} {} {} C{} {} {} {} {} {} C{} {} {} {} {} {} C{} {} {} {} {} 0 Z",
        num(w),
        num(w),
        num(w),
        num(ky),
        num(w - rx),
        num(ry),
        num(w),
        num(h - ky),
        num(w),
        num(h),
        num(w - rx),
        num(h),
        num(rx + kx),
        num(h),
        num(0.0),
        num(h - ky),
        num(0.0),
        num(ry),
        num(0.0),
        num(ry),
        num(rx - kx),
        num(0.0),
        num(w)
    )
}

// ---------- helpers ----------

/// Scribus lays a facing document's left page at x = 100 and its right page one width further;
/// a non-facing document keeps one on every page.
fn page_origin_x(side: PageSide, width: f64, facing: bool) -> f64 {
    if facing && side != PageSide::Left { 100.0 + width } else { 100.0 }
}

fn margin_sides(m: designcraft_doc::Margins, _side: PageSide) -> (f64, f64) {
    // Scribus writes the inside (gutter) margin as `BorderLeft` and the outside one as
    // `BorderRight`, for every page: it mirrors them for left pages itself.
    (m.inside, m.outside)
}

fn guide_list(guides: &[designcraft_doc::Guide], vertical: bool, page_x: f64) -> String {
    let mut s = String::new();
    for g in guides {
        let matches = matches!(g.orientation, designcraft_doc::Orientation::Vertical) == vertical;
        if !matches {
            continue;
        }
        let v = if vertical { g.position - page_x } else { g.position };
        s.push_str(&num(v));
        s.push(' ');
    }
    s
}

fn page_for(item: &Item, spread: &Spread) -> usize {
    if spread.pages.len() <= 1 {
        return 0;
    }
    let b = item.bounds();
    let center = b.center().x;
    spread.pages.iter().position(|p| center >= p.x && center <= p.x + p.width).unwrap_or(0)
}

fn unit_index(u: Unit) -> i64 {
    match u {
        Unit::Points => 0,
        Unit::Millimeters => 1,
        Unit::Inches => 2,
        Unit::Picas => 3,
        Unit::Centimeters => 4,
        Unit::Ciceros => 5,
        Unit::Agates => 6,
        Unit::Pixels => 7,
        _ => 1,
    }
}

fn bool_num(v: bool) -> i64 {
    if v { 1 } else { 0 }
}

fn hex(c: [u8; 3]) -> String {
    format!("#{:02x}{:02x}{:02x}", c[0], c[1], c[2])
}

fn extension(mime: &str) -> &'static str {
    match mime {
        "image/jpeg" => "jpg",
        "image/gif" => "gif",
        "image/tiff" => "tif",
        "image/webp" => "webp",
        "image/svg+xml" => "svg",
        "application/pdf" => "pdf",
        _ => "png",
    }
}

fn sanitize_name(name: &str) -> String {
    let base = std::path::Path::new(name).file_name().and_then(|n| n.to_str()).unwrap_or("image");
    let mut out: String = base.chars().map(|c| if c.is_ascii_alphanumeric() || matches!(c, '.' | '-' | '_') { c } else { '_' }).collect();
    while out.starts_with('.') {
        out.remove(0);
    }
    if out.is_empty() { "image".into() } else { out }
}

/// Format a coordinate without exponent notation and without a trailing `.0`.
fn num(v: f64) -> String {
    if !v.is_finite() {
        return "0".into();
    }
    let r = (v * 1_000_000.0).round() / 1_000_000.0;
    if r == r.trunc() && r.abs() < 1e15 {
        format!("{}", r as i64)
    } else {
        let s = format!("{r:.6}");
        s.trim_end_matches('0').trim_end_matches('.').to_string()
    }
}
