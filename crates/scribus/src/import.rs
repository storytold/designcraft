//! Scribus XML → Document.

use std::collections::{HashMap, HashSet};
use std::sync::Arc;

use designcraft_color::{BlendMode, Color, ColorType, Swatch, SwatchValue, swatch};
use designcraft_doc::build::NewDocument;
use designcraft_doc::{
    Asset, AssetId, CharAttrs, CharFormat, CharRun, CharacterStyle, Columns, Content, DocSettings, Document, Fill, FirstBaseline, Graphic, GridAlign,
    Guide, Item, ItemId, Layer, LayerId, Leading, Margins, Orientation, Page, PageId, PageSide, ParaAttrs, ParaFormat, ParagraphStyle, ParentInfo,
    Section, Shape, Spread, SpreadId, Story, StoryId, Stroke, StrokeType, Styles, TextFrame, TextFrameOptions, VerticalJustification, story as st,
};
use designcraft_geom::{Affine, PathData, Point, Rect, Unit, shapes};

use crate::xml::{El, MAX_DEPTH, Node, parse};
use crate::{Result, ScribusError};

const BASIC: &str = designcraft_doc::BASIC_PARAGRAPH;
const DEFAULT_PARA: &str = "Default Paragraph Style";
const DEFAULT_CHAR: &str = "Default Character Style";

/// An imported Scribus document and what the import could not bring in.
#[derive(Debug)]
pub struct Imported {
    pub document: Document,
    /// One line per problem that did not stop the import.
    pub warnings: Vec<String>,
}

/// Import a Scribus `.sla` document without reading its linked images: the caller brings a
/// reader for those ([`import_scribus_with`]). This crate never touches the file system.
pub fn import_scribus(bytes: &[u8]) -> Result<Document> {
    import_scribus_with(bytes, &|_| None)
}

/// Import with a custom reader for linked image files (`path` → bytes, as written in the file).
pub fn import_scribus_with(bytes: &[u8], read_link: &dyn Fn(&str) -> Option<Vec<u8>>) -> Result<Document> {
    import_scribus_report(bytes, read_link).map(|r| r.document)
}

/// Import, reporting the problems that did not stop it.
pub fn import_scribus_report(bytes: &[u8], read_link: &dyn Fn(&str) -> Option<Vec<u8>>) -> Result<Imported> {
    let root = parse(bytes).map_err(ScribusError::Invalid)?;
    if !matches!(root.local(), "SCRIBUSUTF8NEW" | "SCRIBUSUTF8" | "SCRIBUSUTF8OLD") {
        return Err(ScribusError::NotScribus(format!("root is <{}>", root.name)));
    }
    let document = root.find("Document").ok_or_else(|| ScribusError::Invalid("missing <Document>".into()))?;
    let mut im = Importer::new(read_link);
    im.run(document)?;
    let (document, warnings) = im.finish()?;
    Ok(Imported { document, warnings })
}

struct Importer<'r> {
    read_link: &'r dyn Fn(&str) -> Option<Vec<u8>>,
    doc: Document,
    warnings: Vec<String>,
    styles: Styles,
    default_para: String,
    default_char: String,
    swatches: Vec<Swatch>,
    layers: Vec<Layer>,
    layer_ids: Vec<LayerId>,
    spreads: Vec<Spread>,
    parents: Vec<Spread>,
    /// PageNumber attribute → (spread index, page index).
    page_at: HashMap<i64, (usize, usize)>,
    /// PageNumber attribute → the page's absolute origin in Scribus coordinates.
    page_origin: HashMap<i64, (f64, f64)>,
    /// Master page name → (parent index, x, y).
    masters: HashMap<String, (usize, f64, f64)>,
    item_ids: HashMap<String, ItemId>,
    /// Text frames whose story is still to be built.
    text_items: Vec<PendingText>,
    assigned: HashSet<ItemId>,
    warnings_shown: HashSet<&'static str>,
}

#[derive(Clone)]
struct PendingText {
    item: ItemId,
    element: El,
}

impl<'r> Importer<'r> {
    fn new(read_link: &'r dyn Fn(&str) -> Option<Vec<u8>>) -> Self {
        let settings = DocSettings::default();
        let doc = Document::new(&NewDocument {
            pages: 1,
            width: settings.page_width,
            height: settings.page_height,
            margins: Margins::uniform(36.0),
            ..Default::default()
        });
        Importer {
            read_link,
            doc,
            warnings: Vec::new(),
            styles: Styles::default(),
            default_para: BASIC.into(),
            default_char: designcraft_doc::NO_CHAR_STYLE.into(),
            swatches: designcraft_color::default_swatches(),
            layers: Vec::new(),
            layer_ids: Vec::new(),
            spreads: Vec::new(),
            parents: Vec::new(),
            page_at: HashMap::new(),
            page_origin: HashMap::new(),
            masters: HashMap::new(),
            item_ids: HashMap::new(),
            text_items: Vec::new(),
            assigned: HashSet::new(),
            warnings_shown: HashSet::new(),
        }
    }

    fn warn(&mut self, s: impl Into<String>) {
        self.warnings.push(s.into());
    }

    fn warn_once(&mut self, key: &'static str, s: impl Into<String>) {
        if self.warnings_shown.insert(key) {
            self.warn(s);
        }
    }

    fn run(&mut self, document: &El) -> Result<()> {
        let settings = self.settings(document);
        self.doc.settings = settings;
        self.doc.title = title_of(document);
        self.file_info(document);
        self.read_colors(document);
        self.read_styles(document);
        self.read_layers(document);
        self.read_masters(document);
        self.read_pages(document);
        self.read_sections(document);
        self.read_items(document);
        self.build_stories();
        Ok(())
    }

    fn settings(&mut self, e: &El) -> DocSettings {
        let mut s = DocSettings::default();
        s.page_width = e.num("PageWidth").unwrap_or(612.0).max(1.0);
        s.page_height = e.num("PageHeight").unwrap_or(792.0).max(1.0);
        s.facing_pages = e.get("PagePositioning").is_none_or(|v| v != "0");
        s.horizontal_units = unit(e.get("UnitIndex"));
        s.vertical_units = s.horizontal_units;
        s.bleed = [
            e.num("BleedTop").unwrap_or(0.0),
            e.num("BleedBottom").unwrap_or(0.0),
            e.num("BleedLeft").unwrap_or(0.0),
            e.num("BleedRight").unwrap_or(0.0),
        ];
        if let Some(v) = e.num("BaselineGridSpacing")
            && v > 0.0
        {
            s.baseline_grid.increment = v;
        }
        s.baseline_grid.start = e.num("BaselineGridOffset").unwrap_or(0.0);
        s
    }

    fn file_info(&mut self, e: &El) {
        let f = &mut self.doc.file_info;
        f.document_title = e.get("DocumentInfoTitle").unwrap_or("").trim().to_string();
        f.author = e.get("DocumentInfoAuthor").unwrap_or("").trim().to_string();
        f.description = e.get("DocumentInfoComments").unwrap_or("").trim().to_string();
        f.copyright_notice = e.get("DocumentInfoRights").unwrap_or("").trim().to_string();
        f.keywords = designcraft_doc::file_info::parse_keywords(e.get("DocumentInfoKeywords").unwrap_or(""));
    }

    fn read_colors(&mut self, document: &El) {
        for e in document.find_all("Color") {
            let Some(name) = e.get("Name").filter(|n| !n.is_empty()) else { continue };
            if matches!(name, "None" | "Black" | "White" | "Registration") {
                continue;
            }
            let value = match e.get("Space").unwrap_or("") {
                "RGB" => {
                    let f = |k: &str, d: f32| e.num(k).map_or(d, |v| (v / 255.0) as f32);
                    Color::rgb(f("R", 0.0), f("G", 0.0), f("B", 0.0))
                }
                "CMYK" => {
                    let f = |k: &str| e.num(k).map_or(0.0, |v| (v / 100.0) as f32);
                    Color::cmyk(f("C"), f("M"), f("Y"), f("K"))
                }
                "Gray" => Color::gray(e.num("G").map_or(0.0, |v| (v / 100.0) as f32)),
                other => {
                    self.warn(format!("colour `{name}` uses an unsupported space `{other}`; it was imported as black"));
                    Color::BLACK
                }
            };
            let spot = e.get("Register") == Some("1") || e.get("Spot") == Some("1");
            self.swatches.push(Swatch {
                name: name.into(),
                value: SwatchValue::Color { color: value, color_type: if spot { ColorType::Spot } else { ColorType::Process } },
                locked: false,
                named: true,
                hidden: false,
            });
        }
        if document.find("Gradient").is_some() {
            self.warn_once("gradients", "gradients are not imported: filled and stroked objects use their plain colour instead");
        }
    }

    fn read_layers(&mut self, document: &El) {
        let mut elements: Vec<&El> = document.find_all("Layers").collect();
        elements.sort_by_key(|e| e.get("Number").and_then(|v| v.parse::<i64>().ok()).unwrap_or(i64::MAX));
        for (i, e) in elements.iter().enumerate() {
            let id = LayerId(self.doc.alloc());
            let name = e.get("Name").filter(|n| !n.is_empty()).map_or_else(|| format!("Layer {}", i + 1), str::to_string);
            let color = e.get("LayerColor").and_then(hex_rgb).unwrap_or(designcraft_doc::LAYER_COLORS[i % designcraft_doc::LAYER_COLORS.len()].1);
            self.layers.push(Layer {
                id,
                name,
                color,
                visible: bool_attr(e, "IsViewable", true),
                locked: !bool_attr(e, "IsEditable", true),
                printable: bool_attr(e, "IsPrintable", true),
                show_guides: true,
                suppress_wrap_when_hidden: false,
            });
            self.layer_ids.push(id);
        }
        if self.layers.is_empty() {
            let id = LayerId(self.doc.alloc());
            self.layers.push(Layer {
                id,
                name: "Layer 1".into(),
                color: designcraft_doc::LAYER_COLORS[0].1,
                visible: true,
                locked: false,
                printable: true,
                show_guides: true,
                suppress_wrap_when_hidden: false,
            });
            self.layer_ids.push(id);
        }
    }

    fn read_styles(&mut self, document: &El) {
        let mut chars: HashMap<String, CharacterStyle> = HashMap::new();
        for e in document.find_all("CharacterStyle") {
            let Some(name) = e.get("Name").filter(|n| !n.is_empty()) else { continue };
            let style = CharacterStyle {
                name: name.into(),
                based_on: e.get("Parent").filter(|p| !p.is_empty()).map(str::to_string),
                chars: self.char_attrs(e),
                shortcut: e.get("Shortcut").unwrap_or("").into(),
            };
            chars.insert(name.into(), style);
        }
        // Paragraph styles copy their character style's attributes: DesignCraft's paragraph styles
        // do not have a separate character-style parent.
        let mut para_elements: Vec<(&str, &El)> = Vec::new();
        for e in document.find_all("ParagraphStyle") {
            if let Some(name) = e.get("Name").filter(|n| !n.is_empty()) {
                para_elements.push((name, e));
            }
        }
        let mut paragraphs: Vec<ParagraphStyle> = Vec::new();
        for (name, e) in para_elements {
            let mut attrs = e.get("CParent").and_then(|p| chars.get(p)).map(|c| c.chars.clone()).unwrap_or_default();
            attrs.merge(&self.char_attrs(e));
            // Scribus keeps line spacing on the paragraph style; DesignCraft's leading is a
            // character attribute, so it travels with the style's character part.
            let mut auto_leading = None;
            let mut grid_align = None;
            if let Some(v) = e.num("LineSpacing").filter(|v| *v > 0.0) {
                match int_attr(e, "LineSpacingMode", 0) {
                    0 => attrs.leading = Some(Leading::Points(v)),
                    1 => {
                        attrs.leading = Some(Leading::Auto);
                        auto_leading = Some(v / 100.0);
                    }
                    2 => grid_align = Some(GridAlign::AllLines),
                    _ => {}
                }
            }
            let para = ParaAttrs {
                align: match int_attr(e, "Alignment", 0) {
                    1 => designcraft_doc::Align::Center,
                    2 => designcraft_doc::Align::Right,
                    3 | 4 => designcraft_doc::Align::FullyJustified,
                    5 => designcraft_doc::Align::LeftJustified,
                    6 => designcraft_doc::Align::CenterJustified,
                    7 => designcraft_doc::Align::RightJustified,
                    _ => designcraft_doc::Align::Left,
                }
                .into(),
                left_indent: e.num("LeftMargin").filter(|v| *v >= 0.0),
                right_indent: e.num("RightMargin").filter(|v| *v >= 0.0),
                first_line_indent: e.num("FirstIndent"),
                space_before: e.num("GapBefore"),
                space_after: e.num("GapAfter"),
                drop_cap_lines: if bool_attr(e, "HasDropCap", false) { Some(int_attr(e, "DropCapLines", 0).clamp(0, 60) as u32) } else { None },
                auto_leading,
                grid_align,
                ..Default::default()
            };
            paragraphs.push(ParagraphStyle {
                name: name.into(),
                based_on: e.get("Parent").filter(|p| !p.is_empty()).map(str::to_string),
                next_style: e.get("NextStyle").filter(|p| !p.is_empty()).map(str::to_string),
                para,
                chars: attrs,
                shortcut: e.get("Shortcut").unwrap_or("").into(),
            });
        }
        if let Some(s) = chars.get(DEFAULT_CHAR) {
            self.default_char = s.name.clone();
        }
        if paragraphs.iter().any(|p| p.name == DEFAULT_PARA) {
            self.default_para = DEFAULT_PARA.into();
        }
        for s in chars.into_values() {
            if !self.styles.character.iter().any(|c| c.name == s.name) {
                self.styles.character.push(s);
            }
        }
        for p in paragraphs {
            if !self.styles.paragraph.iter().any(|c| c.name == p.name) {
                self.styles.paragraph.push(p);
            }
        }
        self.styles.default_paragraph = self.default_para.clone();
        self.styles.default_character = self.default_char.clone();
    }

    /// Character formatting on a style, text run or `<Content>` element.
    fn char_attrs(&self, e: &El) -> CharAttrs {
        let mut a = CharAttrs::default();
        if let Some(font) = e.get("Font").filter(|v| !v.is_empty()) {
            let (family, style) = split_font(font);
            a.font_family = Some(family);
            a.font_style = Some(style);
        }
        if let Some(v) = e.num("FontSize").filter(|v| *v > 0.0 && *v <= 10_000.0) {
            a.size = Some(v);
        }
        if let Some(c) = e.get("FontColor").filter(|c| !c.is_empty()) {
            a.fill = Some(swatch_name(c));
        }
        if let Some(v) = e.num("FillShade") {
            a.fill_tint = Some((v / 100.0).clamp(0.0, 1.0) as f32);
        }
        if let Some(c) = e.get("StrokeColor").filter(|c| !c.is_empty() && *c != "None") {
            a.stroke = Some(swatch_name(c));
            if let Some(v) = e.num("StrokeShade") {
                a.stroke_tint = Some((v / 100.0).clamp(0.0, 1.0) as f32);
            }
            if let Some(v) = e.num("TextOutlineWidth").filter(|v| *v > 0.0) {
                a.stroke_weight = Some(v);
            }
        }
        if let Some(v) = e.num("ScaleHorizontal").filter(|v| *v > 0.0) {
            a.h_scale = Some(v / 100.0);
        }
        if let Some(v) = e.num("ScaleVertical").filter(|v| *v > 0.0) {
            a.v_scale = Some(v / 100.0);
        }
        if let Some(v) = e.num("Kerning") {
            a.tracking = Some(v);
        }
        let size = a.size.unwrap_or(12.0);
        if let Some(v) = e.num("BaselineOffset") {
            a.baseline_shift = Some(v / 100.0 * size);
        }
        a
    }

    fn read_masters(&mut self, document: &El) {
        let masters: Vec<&El> = document.find_all("MasterPage").collect();
        for e in masters {
            let Some(name) = e.get("PageName").filter(|n| !n.is_empty()) else { continue };
            let width = e.num("PageWidth").unwrap_or(self.doc.settings.page_width).max(1.0);
            let height = e.num("PageHeight").unwrap_or(self.doc.settings.page_height).max(1.0);
            let side = if e.get("LeftPage") == Some("1") { PageSide::Left } else { PageSide::Right };
            let margins = margins_of(e, side);
            let mut page = page(e, width, height, 0.0, margins, side, true);
            page.parent = None;
            let index = self.parents.len();
            let prefix = parent_prefix(index);
            let spread_id = SpreadId(self.doc.alloc());
            let spread = Spread {
                id: spread_id,
                pages: vec![page],
                items: Vec::new(),
                parent: Some(ParentInfo { prefix, name: name.to_string(), based_on: None }),
                allow_shuffle: true,
            };
            self.parents.push(spread);
            self.masters.insert(name.into(), (index, e.num("PageXPosition").unwrap_or(0.0), e.num("PageYPosition").unwrap_or(0.0)));
        }
    }

    fn read_pages(&mut self, document: &El) {
        let mut pages: Vec<&El> = document.find_all("Page").collect();
        pages.sort_by_key(|e| int_attr(e, "PageNumber", 0));
        if pages.is_empty() {
            self.warn("the document has no pages");
            return;
        }
        let first_number = document.num("FirstPageNumber").unwrap_or(1.0) as i64;
        // Scribus places the pages of a spread at one Y; group them in reading order, then put each
        // group at its leftmost X.
        let mut groups: Vec<(f64, Vec<&El>)> = Vec::new();
        for e in pages {
            let y = e.num("PageYPosition").unwrap_or(0.0);
            match groups.iter_mut().find(|(gy, _)| (*gy - y).abs() < 0.5) {
                Some((_, g)) => g.push(e),
                None => groups.push((y, vec![e])),
            }
        }
        for (group_y, group) in groups {
            let min_x = group.iter().filter_map(|e| e.num("PageXPosition")).reduce(f64::min).unwrap_or(0.0);
            let mut cells = Vec::new();
            for e in group {
                let number = int_attr(e, "PageNumber", 0);
                let width = e.num("PageWidth").unwrap_or(self.doc.settings.page_width).max(1.0);
                let height = e.num("PageHeight").unwrap_or(self.doc.settings.page_height).max(1.0);
                let side = if self.doc.settings.facing_pages {
                    if (first_number + number) % 2 == 1 { PageSide::Right } else { PageSide::Left }
                } else {
                    PageSide::Single
                };
                let margins = margins_of(e, side);
                let absolute_x = e.num("PageXPosition").unwrap_or(min_x);
                let mut p = page(e, width, height, absolute_x - min_x, margins, side, true);
                p.parent = e.get("MasterPageName").filter(|n| !n.is_empty()).and_then(|n| match self.masters.get(n) {
                    Some((i, _, _)) => self.parents.get(*i).map(|s| s.id),
                    None => {
                        self.warn(format!("page {} uses the master page `{n}`, which is not in the file", number + first_number));
                        None
                    }
                });
                cells.push((number, p, absolute_x));
            }
            let spread_index = self.spreads.len();
            let spread_id = SpreadId(self.doc.alloc());
            let mut spread = Spread { id: spread_id, pages: Vec::new(), items: Vec::new(), parent: None, allow_shuffle: true };
            for (page_index, (number, p, _absolute_x)) in cells.into_iter().enumerate() {
                self.page_at.insert(number, (spread_index, page_index));
                // Items are stored in document coordinates; keep the spread's own origin (the
                // leftmost page and the group's y) so they can be brought into spread space.
                self.page_origin.insert(number, (min_x, group_y));
                spread.pages.push(p);
            }
            self.spreads.push(spread);
        }
    }

    fn read_sections(&mut self, document: &El) {
        let mut sections = Vec::new();
        if let Some(all) = document.find("Sections") {
            let mut elements: Vec<&El> = all.find_all("Section").collect();
            elements.sort_by_key(|e| int_attr(e, "From", 0));
            for e in elements {
                if !bool_attr(e, "Active", true) {
                    continue;
                }
                let start = int_attr(e, "From", 0).max(0) as usize;
                let start_number = e.num("Start").filter(|v| *v >= 1.0).map(|v| v as u32);
                sections.push(Section {
                    start,
                    start_number,
                    style: designcraft_doc::NumberStyle::Arabic,
                    prefix: String::new(),
                    marker: e.get("Name").unwrap_or("").to_string(),
                    include_prefix: false,
                });
            }
        }
        if sections.is_empty() {
            sections.push(Section {
                start: 0,
                start_number: Some(document.num("FirstPageNumber").unwrap_or(1.0).max(1.0) as u32),
                style: Default::default(),
                prefix: String::new(),
                marker: String::new(),
                include_prefix: false,
            });
        }
        self.doc.sections = sections;
    }

    fn read_items(&mut self, document: &El) {
        for e in document.find_all("PageObject") {
            let Some(number) = e.get("OwnPage").and_then(|v| v.parse::<i64>().ok()) else { continue };
            let Some((si, pi)) = self.page_at.get(&number).copied() else {
                self.warn(format!("an object says it is on page {number}, which is not in the document"));
                continue;
            };
            let Some((x, y)) = self.page_origin.get(&number).copied() else { continue };
            if let Some(item) = self.build_item(e, x, y, 0) {
                let _ = pi;
                self.spreads[si].items.push(Arc::new(item));
            }
        }
        for e in document.find_all("MasterObject") {
            let name = e.get("OnMasterPage").or_else(|| e.get("MasterPageName"));
            let Some(name) = name.filter(|n| !n.is_empty()) else { continue };
            let Some((mi, mx, my)) = self.masters.get(name).copied() else {
                self.warn(format!("a master object says it is on `{name}`, which is not in the file"));
                continue;
            };
            if let Some(item) = self.build_item(e, mx, my, 1) {
                self.parents[mi].items.push(Arc::new(item));
            }
        }
    }

    fn build_item(&mut self, e: &El, origin_x: f64, origin_y: f64, depth: usize) -> Option<Item> {
        if depth > MAX_DEPTH {
            self.warn("an object is nested too deeply; its contents were not imported");
            return None;
        }
        let x = e.num("XPosition").unwrap_or(0.0);
        let y = e.num("YPosition").unwrap_or(0.0);
        let rotation = e.num("Rotation").unwrap_or(0.0);
        let width = e.num("Width").unwrap_or(0.0).max(0.0);
        let height = e.num("Height").unwrap_or(0.0).max(0.0);
        let item_type = int_attr(e, "ItemType", 6);
        let frame_type = int_attr(e, "FrameType", 0);
        let id = ItemId(self.doc.alloc());
        if let Some(scribus) = e.get("ItemID") {
            self.item_ids.insert(scribus.to_string(), id);
        }
        let layer = self.layer_of(e);
        let shape = match item_type {
            4 => Shape::Rectangle,
            5 => Shape::GraphicLine,
            12 => Shape::Group,
            _ => match frame_type {
                1 => Shape::Oval,
                2 | 3 => Shape::Polygon,
                _ => Shape::Rectangle,
            },
        };
        let path = if item_type == 5 {
            shapes::line(Point::new(0.0, 0.0), Point::new(width.max(0.1), 0.0))
        } else {
            e.get("Path")
                .filter(|p| !p.trim().is_empty())
                .and_then(parse_path)
                .unwrap_or_else(|| shapes::rectangle(Rect::new(0.0, 0.0, width, height)))
        };
        let mut item = Item::new(id, layer, shape, path);
        item.name = e.get("AutoName").filter(|n| !n.is_empty()).map_or_else(|| format!("Object {id}"), str::to_string);
        item.xf = Affine::translate((x - origin_x, y - origin_y)) * Affine::rotate(rotation.to_radians());
        // Children are positioned in document coordinates; their local transform comes from the
        // group's own absolute transform.
        let group_xf = Affine::translate((x, y)) * Affine::rotate(rotation.to_radians());
        item.fill = self.fill_of(e);
        item.stroke = self.stroke_of(e);
        item.blend = blend_mode(int_attr(e, "FillBlendMode", 0));
        if let Some(v) = e.num("FillTransparency") {
            item.opacity = (v / 100.0).clamp(0.0, 1.0) as f32;
        }
        match item_type {
            4 => {
                let story = StoryId(0);
                item.content = Content::Text(TextFrame { story, options: text_options(e) });
                self.text_items.push(PendingText { item: id, element: e.clone() });
            }
            2 => {
                item.content = Content::Graphic(self.graphic_of(e, width, height));
            }
            5 => {}
            12 => {
                let mut children = Vec::new();
                for child in e.elements().filter(|c| c.local() == "PageObject" || c.local() == "MasterObject") {
                    if let Some(mut c) = self.build_item(child, 0.0, 0.0, depth + 1) {
                        // Scribus stores a group child's position in document coordinates; the
                        // document model keeps children in the group's own space.
                        c.xf = group_xf.inverse() * c.xf;
                        children.push(Arc::new(c));
                    }
                }
                item.content = Content::Group { items: children };
            }
            6 => {}
            other => {
                self.warn(format!("`{}` is an unsupported Scribus object (type {other}); it was not imported", item.name));
                return None;
            }
        }
        Some(item)
    }

    fn layer_of(&self, e: &El) -> LayerId {
        let n = int_attr(e, "Layer", 0).max(0) as usize;
        self.layer_ids.get(n).copied().or_else(|| self.layer_ids.first().copied()).unwrap_or(LayerId(1))
    }

    fn fill_of(&self, e: &El) -> Fill {
        let name = e.get("FillColor").unwrap_or("None");
        Fill { swatch: swatch_name(name), tint: (e.num("FillShade").unwrap_or(100.0) / 100.0).clamp(0.0, 1.0) as f32, ..Fill::none() }
    }

    fn stroke_of(&self, e: &El) -> Stroke {
        let Some(color) = e.get("LineColor").filter(|c| !c.is_empty() && *c != "None") else { return Stroke::none() };
        let weight = e.num("LineWidth").unwrap_or(1.0).max(0.0);
        if weight <= 0.0 {
            return Stroke::none();
        }
        let pen = int_attr(e, "LinePenStyle", 1);
        let kind = match pen {
            0 | 1 => StrokeType::Solid,
            2 => StrokeType::Dashed { pattern: vec![4.0 * weight.max(0.2), 3.0 * weight.max(0.2)] },
            3 => StrokeType::Dotted,
            4 => StrokeType::Dashed { pattern: vec![4.0 * weight.max(0.2), 3.0 * weight.max(0.2), 1.0, 3.0 * weight.max(0.2)] },
            _ => StrokeType::Solid,
        };
        Stroke {
            swatch: swatch_name(color),
            tint: (e.num("LineShade").unwrap_or(100.0) / 100.0).clamp(0.0, 1.0) as f32,
            weight,
            kind,
            ..Stroke::default()
        }
    }

    fn graphic_of(&mut self, e: &El, width: f64, height: f64) -> Graphic {
        let link = e.get("ImageFileName").filter(|n| !n.trim().is_empty()).map(str::to_string);
        let data = link.as_deref().and_then(|l| (self.read_link)(l));
        let pixels = data.as_deref().and_then(designcraft_images::pixel_size);
        let mime = data
            .as_deref()
            .map(designcraft_images::mime)
            .filter(|m| *m != "application/octet-stream")
            .unwrap_or("application/octet-stream")
            .to_string();
        let name = link.as_deref().and_then(|l| std::path::Path::new(l).file_name()).and_then(|n| n.to_str()).unwrap_or("image").to_string();
        let asset_id = AssetId(self.doc.alloc());
        let asset =
            Asset { id: asset_id, name, mime, link: link.clone(), data: Arc::new(data.clone().unwrap_or_default()), pixels, ..Default::default() };
        let aid = self.doc.add_asset(asset);
        if link.is_some() && data.is_none() {
            self.warn(format!("the linked image `{}` could not be read; the frame is empty", link.unwrap_or_default()));
        }
        let size = pixels.map(|(w, h)| (w as f64, h as f64)).unwrap_or((width.max(1.0), height.max(1.0)));
        let sx = e.num("ImageScaleX").filter(|v| v.is_finite()).unwrap_or(1.0);
        let sy = e.num("ImageScaleY").filter(|v| v.is_finite()).unwrap_or(1.0);
        let ox = e.num("ImageOffsetX").unwrap_or(0.0);
        let oy = e.num("ImageOffsetY").unwrap_or(0.0);
        let mut xf = Affine::translate((-ox, -oy)) * Affine::scale_non_uniform(sx, sy);
        if let Some(rot) = e.num("ImageRotation").filter(|v| v.abs() > 1e-9) {
            let c = Point::new(width / 2.0, height / 2.0);
            xf = Affine::translate(c.to_vec2()) * Affine::rotate(rot.to_radians()) * Affine::translate(-c.to_vec2()) * xf;
            self.warn_once("image-rotation", "rotated images are approximated (the image is rotated about the frame centre)");
        }
        Graphic {
            layer_options: None,
            asset: aid,
            size,
            xf,
            clip: None,
            auto_fit: Default::default(),
            fit_align: 4,
            crop: [0.0; 4],
            wrap: Default::default(),
            fill: Fill::none(),
        }
    }

    fn item_mut(&mut self, id: ItemId) -> Option<&mut Item> {
        for spread in self.spreads.iter_mut().chain(self.parents.iter_mut()) {
            for arc in &mut spread.items {
                if let Some(item) = find_item_mut(arc, id) {
                    return Some(item);
                }
            }
        }
        None
    }

    fn build_stories(&mut self) {
        let pending = std::mem::take(&mut self.text_items);
        let next: HashMap<ItemId, Option<String>> = pending.iter().map(|p| (p.item, p.element.get("NextItem").map(str::to_string))).collect();
        for p in &pending {
            if self.assigned.contains(&p.item) {
                continue;
            }
            let Some(story_el) = p.element.find("StoryText") else { continue };
            let default = p.element.get("ParagraphStyle").filter(|s| !s.is_empty()).unwrap_or(&self.default_para).to_string();
            let (text, paras, chars) = self.parse_story(story_el, &default);
            let sid = StoryId(self.doc.alloc());
            if let Some(tf) = self.item_mut(p.item).and_then(Item::text_frame_mut) {
                tf.story = sid;
            }
            let mut frames = vec![p.item];
            self.assigned.insert(p.item);
            let mut seen = HashSet::new();
            seen.insert(p.item);
            let mut cur = p.item;
            while let Some(Some(next_id)) = next.get(&cur) {
                let Some(&fid) = self.item_ids.get(next_id) else { break };
                if !seen.insert(fid) {
                    self.warn_once("thread-cycle", "a text thread loops back on itself; the chain was cut");
                    break;
                }
                if let Some(tf) = self.item_mut(fid).and_then(Item::text_frame_mut) {
                    tf.story = sid;
                }
                frames.push(fid);
                self.assigned.insert(fid);
                cur = fid;
            }
            let story = Story { id: sid, text, paras, chars, frames, ..Story::new(sid) };
            self.doc.stories.insert(sid, Arc::new(story));
        }
        // Frames with no story text and no head: one empty story each.
        for p in pending {
            if self.assigned.contains(&p.item) {
                continue;
            }
            let sid = StoryId(self.doc.alloc());
            let story = Story::with_text(sid, "", ParaFormat { style: self.default_para.clone(), ..Default::default() });
            if let Some(tf) = self.item_mut(p.item).and_then(Item::text_frame_mut) {
                tf.story = sid;
            }
            let mut story = story;
            story.frames = vec![p.item];
            self.doc.stories.insert(sid, Arc::new(story));
            self.assigned.insert(p.item);
        }
    }

    #[allow(unused_assignments)]
    fn parse_story(&mut self, e: &El, default: &str) -> (String, Vec<ParaFormat>, Vec<CharRun>) {
        let mut text = String::new();
        let mut paras: Vec<ParaFormat> = Vec::new();
        let mut chars: Vec<CharRun> = Vec::new();
        let mut run_len = 0usize;
        let mut run_fmt = CharFormat { style: designcraft_doc::NO_CHAR_STYLE.into(), over: CharAttrs::default() };
        let mut style = e.find("DefaultStyle").and_then(|d| d.get("Parent")).filter(|p| !p.is_empty()).unwrap_or(default).to_string();
        let base = e.find("DefaultStyle").map(|d| self.char_attrs(d)).unwrap_or_default();

        macro_rules! flush {
            () => {
                if run_len > 0 {
                    chars.push(CharRun { len: run_len, format: run_fmt.clone() });
                    run_len = 0;
                }
            };
        }
        fn append(text: &mut String, run_len: &mut usize, s: &str) {
            text.push_str(s);
            *run_len += s.len();
        }
        for node in &e.children {
            let Node::El(child) = node else { continue };
            match child.local() {
                "Content" => {
                    let mut fmt = CharFormat { style: designcraft_doc::NO_CHAR_STYLE.into(), over: base.clone() };
                    if let Some(cp) = child.get("CParent").filter(|p| !p.is_empty()) {
                        fmt.style = cp.to_string();
                    }
                    fmt.over.merge(&self.char_attrs(child));
                    if fmt != run_fmt {
                        flush!();
                        run_fmt = fmt.clone();
                    }
                    if let Some(s) = child.get("Chars") {
                        append(&mut text, &mut run_len, s);
                    }
                    if let Some(u) = child.get("Unicode").and_then(parse_unicode) {
                        text.push(u);
                        run_len += u.len_utf8();
                    }
                }
                "tab" => {
                    if (CharFormat { style: designcraft_doc::NO_CHAR_STYLE.into(), over: base.clone() }) != run_fmt {
                        flush!();
                        run_fmt = CharFormat { style: designcraft_doc::NO_CHAR_STYLE.into(), over: base.clone() };
                    }
                    text.push('\t');
                    run_len += 1;
                }
                "var" => match child.get("name") {
                    Some("pgno") => {
                        if (CharFormat { style: designcraft_doc::NO_CHAR_STYLE.into(), over: base.clone() }) != run_fmt {
                            flush!();
                            run_fmt = CharFormat { style: designcraft_doc::NO_CHAR_STYLE.into(), over: base.clone() };
                        }
                        text.push(st::PAGE_NUMBER);
                        run_len += st::PAGE_NUMBER.len_utf8();
                    }
                    Some("pgco") => {
                        self.warn_once("page-count-var", "the `pgco` text variable (page count) is not available; it was imported as empty text");
                    }
                    other => {
                        self.warn(format!("the text variable `{}` is not available; it was imported as empty text", other.unwrap_or("")));
                    }
                },
                "para" | "trail" => {
                    if let Some(parent) = child.get("Parent").filter(|p| !p.is_empty()) {
                        style = parent.to_string();
                    }
                    if child.local() == "para" {
                        text.push('\n');
                        run_len += 1;
                    }
                    flush!();
                    paras.push(ParaFormat { style: style.clone(), ..Default::default() });
                }
                "DefaultStyle" => {}
                other => {
                    self.warn_once("story-element", format!("unsupported story element `{other}` ignored"));
                }
            }
        }
        if paras.is_empty() || !text.is_empty() || chars.is_empty() {
            flush!();
            paras.push(ParaFormat { style, ..Default::default() });
        }
        // Defensive: a malformed story with more breaks than paragraphs still gets one format per
        // paragraph, so the document invariants hold.
        while paras.len() < text.matches('\n').count() + 1 {
            paras.push(ParaFormat { style: self.default_para.clone(), ..Default::default() });
        }
        paras.truncate(text.matches('\n').count() + 1);
        if chars.is_empty() {
            chars.push(CharRun { len: 0, format: CharFormat::default() });
        }
        // Runs must cover exactly the text.
        let mut total: usize = chars.iter().map(|r| r.len).sum();
        if total > text.len() {
            let mut left = text.len();
            for r in &mut chars {
                r.len = r.len.min(left);
                left -= r.len;
            }
            chars.retain(|r| r.len > 0);
            if chars.is_empty() {
                chars.push(CharRun { len: 0, format: CharFormat::default() });
            }
            total = text.len();
        }
        if total < text.len()
            && let Some(last) = chars.last_mut()
        {
            last.len += text.len() - total;
        }
        (text, paras, chars)
    }

    fn finish(mut self) -> Result<(Document, Vec<String>)> {
        if self.spreads.is_empty() {
            return Err(ScribusError::Invalid("the document has no spreads".into()));
        }
        self.doc.spreads = std::mem::take(&mut self.spreads).into_iter().map(Arc::new).collect();
        self.doc.parents = std::mem::take(&mut self.parents).into_iter().map(Arc::new).collect();
        self.doc.layers = std::mem::take(&mut self.layers);
        self.doc.styles = Arc::new(std::mem::take(&mut self.styles));
        self.doc.swatches = std::mem::take(&mut self.swatches);
        self.doc.settings.primary_story = None;
        self.doc.check().map_err(|e| ScribusError::Invalid(e.to_string()))?;
        let warnings = std::mem::take(&mut self.warnings);
        Ok((self.doc, warnings))
    }
}

/// Find an item by id, descending into groups. `Arc::make_mut` clones the shared group only when
/// the wanted item is inside it.
fn find_item_mut(arc: &mut Arc<Item>, id: ItemId) -> Option<&mut Item> {
    if arc.id == id {
        return Arc::get_mut(arc);
    }
    let item = Arc::make_mut(arc);
    let Content::Group { items } = &mut item.content else { return None };
    for child in items {
        if let Some(found) = find_item_mut(child, id) {
            return Some(found);
        }
    }
    None
}

fn title_of(e: &El) -> String {
    e.get("DocumentInfoTitle").map(str::trim).filter(|t| !t.is_empty()).unwrap_or("Untitled").to_string()
}

fn page(e: &El, width: f64, height: f64, x: f64, margins: Margins, side: PageSide, local_margins: bool) -> Page {
    let mut guides = Vec::new();
    for (attr, orientation) in [("VerticalGuides", Orientation::Vertical), ("HorizontalGuides", Orientation::Horizontal)] {
        for v in e.get(attr).unwrap_or("").split_whitespace().filter_map(|v| v.parse::<f64>().ok()).filter(|v| v.is_finite()) {
            let position = if orientation == Orientation::Vertical { x + v } else { v };
            guides.push(Guide { orientation, position, spread: false, locked: false, layer: None, liquid: false });
        }
    }
    Page {
        id: PageId(0),
        width,
        height,
        x,
        margins,
        columns: Columns::default(),
        local_margins,
        parent: None,
        side,
        overridden: Vec::new(),
        guides,
        show_parent_items: true,
        liquid: Default::default(),
        show_parent_overlay: false,
        transition: None,
        view_rotation: 0,
    }
}

fn margins_of(e: &El, side: PageSide) -> Margins {
    let left = e.num("BorderLeft").unwrap_or(0.0).max(0.0);
    let right = e.num("BorderRight").unwrap_or(0.0).max(0.0);
    let top = e.num("BorderTop").unwrap_or(0.0).max(0.0);
    let bottom = e.num("BorderBottom").unwrap_or(0.0).max(0.0);
    match side {
        PageSide::Left => Margins { top, bottom, inside: right, outside: left },
        _ => Margins { top, bottom, inside: left, outside: right },
    }
}

fn text_options(e: &El) -> TextFrameOptions {
    let mut o = TextFrameOptions {
        columns: int_attr(e, "Columns", 1).clamp(1, designcraft_doc::page::MAX_COLUMNS as i64) as u32,
        gutter: e.num("ColumnGap").unwrap_or(0.0).max(0.0),
        inset: [
            e.num("TextToFrameDistanceTop").unwrap_or(0.0).max(0.0),
            e.num("TextToFrameDistanceLeft").unwrap_or(0.0).max(0.0),
            e.num("TextToFrameDistanceBottom").unwrap_or(0.0).max(0.0),
            e.num("TextToFrameDistanceRight").unwrap_or(0.0).max(0.0),
        ],
        vertical_justification: match int_attr(e, "VerticalAlignment", 0) {
            1 => VerticalJustification::Center,
            2 => VerticalJustification::Bottom,
            3 => VerticalJustification::Justify,
            _ => VerticalJustification::Top,
        },
        first_baseline: FirstBaseline::Ascent,
        ..Default::default()
    };
    if bool_attr(e, "AutomaticTextFrame", false) {
        o.auto_size = designcraft_doc::AutoSize::HeightOnly;
    }
    if int_attr(e, "TextPathType", 0) > 0 {
        o.path = Some(designcraft_doc::PathType { flip: bool_attr(e, "TextPathFlipped", false), start: 0.0, end: None, ..Default::default() });
    }
    o
}

fn unit(v: Option<&str>) -> Unit {
    match v.and_then(|v| v.parse::<u32>().ok()) {
        Some(0) => Unit::Points,
        Some(2) => Unit::Inches,
        Some(3) => Unit::Picas,
        Some(4) => Unit::Centimeters,
        Some(5) => Unit::Ciceros,
        Some(6) => Unit::Agates,
        Some(7) => Unit::Pixels,
        _ => Unit::Millimeters,
    }
}

fn swatch_name(name: &str) -> String {
    match name {
        "None" | "" => swatch::NONE.into(),
        "Black" => swatch::BLACK.into(),
        "White" => swatch::PAPER.into(),
        "Registration" => swatch::REGISTRATION.into(),
        other => other.into(),
    }
}

fn split_font(full: &str) -> (String, String) {
    const STYLES: &[&str] = &[
        "Bold Italic",
        "Bold Oblique",
        "Bold Condensed",
        "Semi Bold",
        "SemiBold",
        "Extra Bold",
        "Ultra Bold",
        "Extra Light",
        "Ultra Light",
        "Book Italic",
        "Book",
        "Bold",
        "Italic",
        "Oblique",
        "Regular",
        "Medium",
        "Light",
        "Thin",
        "Black",
        "Heavy",
        "Condensed",
        "Narrow",
        "Wide",
        "Small Caps",
    ];
    for style in STYLES {
        if let Some(family) = full.strip_suffix(style).and_then(|f| f.strip_suffix(' ')).filter(|f| !f.is_empty()) {
            return (family.to_string(), (*style).to_string());
        }
    }
    (full.to_string(), "Regular".into())
}

fn parse_unicode(s: &str) -> Option<char> {
    let v = s.trim();
    let n = if let Some(hex) = v.strip_prefix("0x").or_else(|| v.strip_prefix("0X")) {
        u32::from_str_radix(hex, 16).ok()?
    } else {
        v.parse::<u32>().ok()?
    };
    char::from_u32(n)
}

fn hex_rgb(s: &str) -> Option<[u8; 3]> {
    let h = s.strip_prefix('#')?;
    if h.len() != 6 {
        return None;
    }
    let v = u32::from_str_radix(h, 16).ok()?;
    Some([(v >> 16) as u8, (v >> 8) as u8, v as u8])
}

fn bool_attr(e: &El, key: &str, default: bool) -> bool {
    match e.get(key) {
        Some(v) => matches!(v.trim(), "1" | "true" | "True"),
        None => default,
    }
}

fn int_attr(e: &El, key: &str, default: i64) -> i64 {
    e.get(key).and_then(|v| v.trim().parse::<i64>().ok()).unwrap_or(default)
}

fn blend_mode(v: i64) -> BlendMode {
    match v {
        1 => BlendMode::Darken,
        2 => BlendMode::Multiply,
        3 => BlendMode::Screen,
        4 => BlendMode::Overlay,
        5 => BlendMode::HardLight,
        6 => BlendMode::SoftLight,
        7 => BlendMode::Difference,
        8 => BlendMode::Exclusion,
        9 => BlendMode::ColorDodge,
        10 => BlendMode::ColorBurn,
        11 => BlendMode::Hue,
        12 => BlendMode::Saturation,
        13 => BlendMode::Color,
        14 => BlendMode::Luminosity,
        _ => BlendMode::Normal,
    }
}

fn parent_prefix(index: usize) -> String {
    let mut n = index;
    let mut s = String::new();
    loop {
        s.insert(0, (b'A' + (n % 26) as u8) as char);
        if n < 26 {
            break;
        }
        n = n / 26 - 1;
    }
    s
}

/// Parse the SVG-ish path Scribus stores on a frame: `M`, `L`, `C`, `Q`, `H`, `V`, `Z`, absolute
/// or relative. Other commands and malformed numbers end the parse; the valid prefix is kept.
fn parse_path(s: &str) -> Option<PathData> {
    const MAX_TOKENS: usize = 1_000_000;
    enum T {
        Cmd(char),
        Num(f64),
    }
    let mut toks = Vec::new();
    let mut it = s.chars().peekable();
    while let Some(&c) = it.peek() {
        if c.is_ascii_alphabetic() {
            toks.push(T::Cmd(c));
            it.next();
        } else if c.is_whitespace() || c == ',' {
            it.next();
        } else {
            let mut num = String::new();
            if matches!(c, '+' | '-') {
                num.push(c);
                it.next();
            }
            while let Some(&d) = it.peek() {
                if d.is_ascii_digit() || d == '.' {
                    num.push(d);
                    it.next();
                } else {
                    break;
                }
            }
            if let Some(&d) = it.peek()
                && (d == 'e' || d == 'E')
            {
                num.push(d);
                it.next();
                if let Some(&sign) = it.peek()
                    && (sign == '+' || sign == '-')
                {
                    num.push(sign);
                    it.next();
                }
                while let Some(&d) = it.peek()
                    && d.is_ascii_digit()
                {
                    num.push(d);
                    it.next();
                }
            }
            let v = num.parse::<f64>().ok().filter(|v| v.is_finite())?;
            toks.push(T::Num(v));
        }
        if toks.len() > MAX_TOKENS {
            return None;
        }
    }
    let mut bp = designcraft_geom::BezPath::new();
    let mut i = 0;
    let mut cur = Point::ZERO;
    let mut start = Point::ZERO;
    let mut last_cmd = 'M';
    let mut open = false;
    while i < toks.len() {
        let cmd = match toks.get(i) {
            Some(T::Cmd(c)) => {
                i += 1;
                *c
            }
            Some(T::Num(_)) => last_cmd,
            None => break,
        };
        let rel = cmd.is_ascii_lowercase();
        let c = cmd.to_ascii_uppercase();
        let num = |i: &mut usize| -> Option<f64> {
            let v = match toks.get(*i) {
                Some(T::Num(v)) => *v,
                _ => return None,
            };
            *i += 1;
            Some(v)
        };
        match c {
            'M' => {
                let x = num(&mut i)?;
                let y = num(&mut i)?;
                let p = if rel { cur + designcraft_geom::Vec2::new(x, y) } else { Point::new(x, y) };
                bp.move_to(p);
                cur = p;
                start = p;
                open = true;
                last_cmd = 'L';
            }
            'L' => {
                let x = num(&mut i)?;
                let y = num(&mut i)?;
                let p = if rel { cur + designcraft_geom::Vec2::new(x, y) } else { Point::new(x, y) };
                bp.line_to(p);
                cur = p;
                last_cmd = 'L';
            }
            'H' => {
                let x = num(&mut i)?;
                let p = Point::new(if rel { cur.x + x } else { x }, cur.y);
                bp.line_to(p);
                cur = p;
                last_cmd = 'H';
            }
            'V' => {
                let y = num(&mut i)?;
                let p = Point::new(cur.x, if rel { cur.y + y } else { y });
                bp.line_to(p);
                cur = p;
                last_cmd = 'V';
            }
            'C' => {
                let mut v = [0.0; 6];
                for x in &mut v {
                    *x = num(&mut i)?;
                }
                let (p1, p2, p3) = if rel {
                    (
                        cur + designcraft_geom::Vec2::new(v[0], v[1]),
                        cur + designcraft_geom::Vec2::new(v[2], v[3]),
                        cur + designcraft_geom::Vec2::new(v[4], v[5]),
                    )
                } else {
                    (Point::new(v[0], v[1]), Point::new(v[2], v[3]), Point::new(v[4], v[5]))
                };
                bp.curve_to(p1, p2, p3);
                cur = p3;
                last_cmd = 'C';
            }
            'Q' => {
                let mut v = [0.0; 4];
                for x in &mut v {
                    *x = num(&mut i)?;
                }
                let (p1, p2) = if rel {
                    (cur + designcraft_geom::Vec2::new(v[0], v[1]), cur + designcraft_geom::Vec2::new(v[2], v[3]))
                } else {
                    (Point::new(v[0], v[1]), Point::new(v[2], v[3]))
                };
                bp.quad_to(p1, p2);
                cur = p2;
                last_cmd = 'Q';
            }
            'Z' => {
                bp.close_path();
                cur = start;
                open = false;
            }
            _ => break,
        }
        let _ = open;
    }
    if bp.elements().is_empty() { None } else { Some(PathData::from_bezpath(&bp)) }
}
