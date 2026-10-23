//! `.sla` XML → [`Document`]: colours, styles, layers, pages, master pages, items and stories.

use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::io::Read;
use std::sync::Arc;

use designcraft_color::{Color, ColorType, Swatch, SwatchValue, swatch as sw};
use designcraft_doc::{
    Asset, AssetId, BASIC_PARAGRAPH, CharAttrs, CharFormat, CharacterStyle, Columns, Content, DocSettings, Document, Fill, FirstBaseline, Graphic,
    Guide, Item, ItemId, Layer, LayerId, Margins, NumberStyle, Orientation, Page, PageId, PageSide, ParaFormat, ParagraphStyle, ParentInfo, Section,
    Shape, Spread, SpreadId, Story, StoryId, Stroke, Styles, TextFrame, TextFrameOptions, VerticalJustification,
};
use designcraft_geom::{Affine, BezPath, PathData, Rect, Unit, shapes};

use crate::text::{self, ShadowAttrs, StoryBuilder, StoryShadow, StyleNames, TextShadow};
use crate::xml::{self, El};
use crate::{Result, SlaError};

/// A document read from a `.sla` file, with what couldn't be carried over.
pub struct Imported {
    pub document: Document,
    pub warnings: Vec<String>,
}

/// Import a Scribus document. `read_link` reads a placed image's file by the path the document
/// names (relative to the `.sla` file, or absolute); `None` leaves the image missing.
pub fn import_sla(bytes: &[u8], read_link: &dyn Fn(&str) -> Option<Vec<u8>>) -> Result<Document> {
    import_sla_report(bytes, read_link, &|_| false).map(|i| i.document)
}

/// [`import_sla`] with its warnings. `has_family` says whether a font family is available: Scribus
/// names a font by family and style in one string ("Times New Roman Bold"), and the families it
/// knows decide where the family ends.
pub fn import_sla_report(bytes: &[u8], read_link: &dyn Fn(&str) -> Option<Vec<u8>>, has_family: &dyn Fn(&str) -> bool) -> Result<Imported> {
    if !crate::is_sla(bytes) {
        return Err(SlaError::NotSla("the file doesn't start with a Scribus document element".into()));
    }
    if bytes.len() > crate::MAX_BYTES {
        return Err(SlaError::Invalid(format!("the file is larger than {} MB", crate::MAX_BYTES >> 20)));
    }
    let root = xml::parse(bytes).map_err(SlaError::Invalid)?;
    let doc = root.find(&["DOCUMENT", "Document"]).ok_or_else(|| SlaError::Invalid("no DOCUMENT element".into()))?;
    let mut im = Importer::new(read_link, has_family);
    im.run(doc)?;
    im.finish(doc)
}

/// Pages, items and colours past these are dropped (with a warning): a hostile file can't make
/// the import allocate without bound.
const MAX_PAGES: usize = 10_000;
const MAX_ITEMS: usize = 500_000;
const MAX_SWATCHES: usize = 10_000;
const MAX_GROUP_DEPTH: usize = 64;
/// Largest embedded image, once inflated, and largest total of them all: compressed data can
/// inflate a thousandfold.
const MAX_EMBEDDED_IMAGE: u64 = 256 * 1024 * 1024;
const MAX_TOTAL_EMBEDDED: u64 = 1024 * 1024 * 1024;
/// Most bytes all linked images may hold together: what a `.designcraft` file can store.
const MAX_TOTAL_LINKED: u64 = 4 << 30;
/// Page rects compared, in total, to find the page under items without a usable `OwnPage`:
/// past it such items go on the first page, so a hostile file can't make the lookup quadratic.
const MAX_PAGE_LOOKUPS: usize = 10_000_000;

const PAGE_OBJECTS: &[&str] = &["PAGEOBJECT", "PageObject"];
const MASTER_OBJECTS: &[&str] = &["MASTEROBJECT", "MasterObject"];
const ITEM_ELEMENTS: &[&str] = &["PAGEOBJECT", "PageObject", "MASTEROBJECT", "MasterObject", "FRAMEOBJECT", "FrameObject"];

/// Scribus colour names → swatch names, the swatches the document defines, and Scribus font
/// names → (family, style).
pub struct Palette<'a> {
    map: HashMap<String, String>,
    swatches: Vec<Swatch>,
    unknown: BTreeSet<String>,
    has_family: &'a dyn Fn(&str) -> bool,
    fonts: HashMap<String, (String, String)>,
}

impl<'a> Palette<'a> {
    fn new(has_family: &'a dyn Fn(&str) -> bool) -> Self {
        let swatches: Vec<Swatch> = designcraft_color::default_swatches().into_iter().filter(|s| s.name.starts_with('[')).collect();
        Palette { map: HashMap::new(), swatches, unknown: BTreeSet::new(), has_family, fonts: HashMap::new() }
    }

    /// The family and style of the Scribus font `name` ("Times New Roman Bold").
    pub fn font(&mut self, name: &str) -> (String, String) {
        if let Some(hit) = self.fonts.get(name) {
            return hit.clone();
        }
        let split = crate::text::split_font(name, self.has_family);
        if self.fonts.len() < 10_000 {
            self.fonts.insert(name.to_string(), split.clone());
        }
        split
    }

    /// The swatch for the Scribus colour `name`. A colour the document doesn't define is made
    /// from the `#rrggbb` its name carries (`FromSVG#336699`), else drawn in black.
    pub fn swatch(&mut self, name: &str) -> String {
        let name = name.trim();
        if name.is_empty() || name == "None" {
            return sw::NONE.into();
        }
        if let Some(s) = self.map.get(name) {
            return s.clone();
        }
        if let Some(c) = name.rsplit_once('#').and_then(|(_, hex)| hex_rgb(hex)) {
            return self.add(name, c, false);
        }
        if self.unknown.len() < 100 {
            self.unknown.insert(name.to_string());
        }
        sw::BLACK.into()
    }

    fn add(&mut self, name: &str, color: Color, spot: bool) -> String {
        if self.swatches.len() >= MAX_SWATCHES {
            return sw::BLACK.into();
        }
        // Black and white are DesignCraft's own swatches.
        let special = match color {
            Color::Cmyk { c, m, y, k } if name == "Black" && c == 0.0 && m == 0.0 && y == 0.0 && k == 1.0 => Some(sw::BLACK),
            _ if name == "White" && is_white(color) => Some(sw::PAPER),
            _ => None,
        };
        let swatch = match special {
            Some(s) => s.to_string(),
            None => {
                let mut unique = name.to_string();
                let mut n = 2;
                while self.swatches.iter().any(|s| s.name == unique) {
                    unique = format!("{name} {n}");
                    n += 1;
                }
                let color_type = if spot { ColorType::Spot } else { ColorType::Process };
                self.swatches.push(Swatch {
                    name: unique.clone(),
                    value: SwatchValue::Color { color, color_type },
                    locked: false,
                    named: true,
                    hidden: false,
                });
                unique
            }
        };
        self.map.insert(name.to_string(), swatch.clone());
        swatch
    }
}

fn is_white(c: Color) -> bool {
    match c {
        Color::Cmyk { c, m, y, k } => c == 0.0 && m == 0.0 && y == 0.0 && k == 0.0,
        Color::Rgb { r, g, b } => r >= 1.0 && g >= 1.0 && b >= 1.0,
        Color::Gray { k } => k == 0.0,
    }
}

/// `rrggbb` (with or without `#`) → RGB.
fn hex_rgb(hex: &str) -> Option<Color> {
    hex_bytes(hex).map(|[r, g, b]| Color::rgb8(r, g, b))
}

fn hex_bytes(hex: &str) -> Option<[u8; 3]> {
    let hex = hex.trim().trim_start_matches('#');
    if hex.len() != 6 || !hex.chars().all(|c| c.is_ascii_hexdigit()) {
        return None;
    }
    let byte = |i: usize| hex.get(i..i + 2).and_then(|h| u8::from_str_radix(h, 16).ok());
    Some([byte(0)?, byte(2)?, byte(4)?])
}

/// `ccmmyykk` hex (old files) → CMYK.
fn hex_cmyk(hex: &str) -> Option<Color> {
    let hex = hex.trim().trim_start_matches('#');
    if hex.len() != 8 || !hex.chars().all(|c| c.is_ascii_hexdigit()) {
        return None;
    }
    let part = |i: usize| hex.get(i..i + 2).and_then(|h| u8::from_str_radix(h, 16).ok()).map(|v| f32::from(v) / 255.0);
    Some(Color::cmyk(part(0)?, part(2)?, part(4)?, part(6)?))
}

/// CIE L*a*b* (D50) → sRGB.
fn lab_to_rgb(l: f64, a: f64, b: f64) -> Color {
    let fy = (l + 16.0) / 116.0;
    let fx = fy + a / 500.0;
    let fz = fy - b / 200.0;
    let f = |t: f64| if t.powi(3) > 0.008856 { t.powi(3) } else { (t - 16.0 / 116.0) / 7.787 };
    // D50 white, then Bradford-adapted XYZ → linear sRGB.
    let (x, y, z) = (0.9642 * f(fx), f(fy), 0.8249 * f(fz));
    let rl = 3.1339 * x - 1.6169 * y - 0.4906 * z;
    let gl = -0.9788 * x + 1.9161 * y + 0.0335 * z;
    let bl = 0.0719 * x - 0.2290 * y + 1.4052 * z;
    let gamma = |v: f64| {
        let v = v.clamp(0.0, 1.0);
        (if v <= 0.003_130_8 { 12.92 * v } else { 1.055 * v.powf(1.0 / 2.4) - 0.055 }) as f32
    };
    Color::rgb(gamma(rl), gamma(gl), gamma(bl))
}

/// One document or master page as Scribus places it on its canvas.
struct PageInfo {
    canvas: (f64, f64),
    width: f64,
    height: f64,
    margins: Margins,
    left: bool,
    name: String,
    master: String,
    guides_v: Vec<f64>,
    guides_h: Vec<f64>,
}

/// A text frame's place in its thread.
struct FrameLink {
    next: Option<i64>,
    back: Option<i64>,
}

struct Importer<'a> {
    read_link: &'a dyn Fn(&str) -> Option<Vec<u8>>,
    next_id: u64,
    warnings: Vec<String>,
    palette: Palette<'a>,
    styles: Styles,
    names: StyleNames,
    settings: DocSettings,
    layers: Vec<Layer>,
    /// Scribus layer number → (layer, level: higher is in front).
    layer_of: HashMap<i64, (LayerId, i64)>,
    spreads: Vec<Spread>,
    parents: Vec<Spread>,
    /// For each document page (in page order): its spread and the canvas origin of that spread.
    page_spread: Vec<(usize, (f64, f64))>,
    page_rects: Vec<Rect>,
    master_of: HashMap<String, (usize, (f64, f64))>,
    sections: Vec<Section>,
    stories: BTreeMap<StoryId, Story>,
    /// Scribus `ItemID` of a text frame → its story.
    story_of: HashMap<i64, StoryId>,
    /// Stories whose text is all shadowed, and the shadow: their frames cast it.
    story_shadow: HashMap<StoryId, TextShadow>,
    /// Stories only partly shadowed (their shadows are left out).
    partly_shadowed: usize,
    /// Scribus `ItemID` → the DesignCraft frame made for it.
    frame_ids: HashMap<i64, ItemId>,
    /// Each story's frames in thread order, by Scribus `ItemID`.
    threads: BTreeMap<StoryId, Vec<i64>>,
    /// Frames without a usable `ItemID` and the story each was given.
    loose_frames: Vec<(StoryId, ItemId)>,
    assets: BTreeMap<AssetId, Arc<Asset>>,
    linked: HashMap<String, (AssetId, (f64, f64))>,
    missing_links: BTreeSet<String>,
    unsupported_links: BTreeSet<String>,
    skipped: BTreeMap<&'static str, usize>,
    items: usize,
    /// Bytes of embedded images inflated so far (see [`MAX_TOTAL_EMBEDDED`]).
    embedded_bytes: u64,
    /// Bytes of linked images read so far (see [`MAX_TOTAL_LINKED`]).
    linked_bytes: u64,
    /// Page rects compared so far (see [`MAX_PAGE_LOOKUPS`]).
    page_lookups: usize,
    /// Items put on the first page because the lookup budget was used up.
    unplaced: usize,
}

impl<'a> Importer<'a> {
    fn new(read_link: &'a dyn Fn(&str) -> Option<Vec<u8>>, has_family: &'a dyn Fn(&str) -> bool) -> Self {
        Importer {
            read_link,
            next_id: 1,
            warnings: Vec::new(),
            palette: Palette::new(has_family),
            styles: Styles::default(),
            names: StyleNames::default(),
            settings: DocSettings::default(),
            layers: Vec::new(),
            layer_of: HashMap::new(),
            spreads: Vec::new(),
            parents: Vec::new(),
            page_spread: Vec::new(),
            page_rects: Vec::new(),
            master_of: HashMap::new(),
            sections: Vec::new(),
            stories: BTreeMap::new(),
            story_of: HashMap::new(),
            story_shadow: HashMap::new(),
            partly_shadowed: 0,
            frame_ids: HashMap::new(),
            threads: BTreeMap::new(),
            loose_frames: Vec::new(),
            assets: BTreeMap::new(),
            linked: HashMap::new(),
            missing_links: BTreeSet::new(),
            unsupported_links: BTreeSet::new(),
            skipped: BTreeMap::new(),
            items: 0,
            embedded_bytes: 0,
            linked_bytes: 0,
            page_lookups: 0,
            unplaced: 0,
        }
    }

    fn alloc(&mut self) -> u64 {
        let id = self.next_id;
        self.next_id = self.next_id.saturating_add(1);
        id
    }

    fn run(&mut self, doc: &El) -> Result<()> {
        self.settings_from(doc);
        self.colors(doc);
        self.styles_from(doc);
        self.layers_from(doc);
        self.masters(doc);
        self.pages(doc)?;
        self.sections_from(doc);
        self.stories_from(doc);
        for e in doc.named(MASTER_OBJECTS) {
            let Some(&(si, origin)) = e.get("OnMasterPage").and_then(|n| self.master_of.get(n)) else {
                *self.skipped.entry("master items on a master page that doesn't exist").or_default() += 1;
                continue;
            };
            let items = self.item(e, origin, 0);
            if let Some(s) = self.parents.get_mut(si) {
                s.items.extend(items.into_iter().map(Arc::new));
            }
        }
        for e in doc.named(PAGE_OBJECTS) {
            let Some((si, origin)) = self.spread_for(e) else { continue };
            let items = self.item(e, origin, 0);
            if let Some(s) = self.spreads.get_mut(si) {
                s.items.extend(items.into_iter().map(Arc::new));
            }
        }
        // `FRAMEOBJECT`s are the frames of inline objects. The text that would anchor them is
        // not imported (and the files seen keep orphans of deleted ones), so they are left out.
        self.order_by_layer();
        self.link_threads();
        Ok(())
    }

    fn settings_from(&mut self, doc: &El) {
        let s = &mut self.settings;
        s.page_width = doc.num(&["PageWidth", "PAGEWIDTH"]).filter(|w| *w > 0.0).unwrap_or(595.28).clamp(1.0, 15_552.0);
        s.page_height = doc.num(&["PageHeight", "PAGEHEIGHT"]).filter(|h| *h > 0.0).unwrap_or(841.89).clamp(1.0, 15_552.0);
        s.facing_pages = doc.num(&["PagePositioning", "BOOK"]).is_some_and(|p| p != 0.0);
        let bleed = |k: &[&str]| doc.num(k).unwrap_or(0.0).clamp(0.0, 1000.0);
        // DesignCraft's order is top, bottom, inside, outside; Scribus keeps inside as Left and
        // outside as Right (its facing-pages labels).
        s.bleed = [bleed(&["BleedTop"]), bleed(&["BleedBottom"]), bleed(&["BleedLeft"]), bleed(&["BleedRight"])];
        let unit = match doc.num(&["UnitIndex", "UNITS"]).map(|u| u as i64) {
            Some(1) => Unit::Millimeters,
            Some(2) => Unit::Inches,
            Some(3) => Unit::Picas,
            Some(4) => Unit::Centimeters,
            Some(5) => Unit::Ciceros,
            _ => Unit::Points,
        };
        s.horizontal_units = unit;
        s.vertical_units = unit;
    }

    fn colors(&mut self, doc: &El) {
        for c in doc.named(&["COLOR", "Color"]) {
            let Some(name) = c.any(&["Name", "NAME"]).map(str::trim).filter(|n| !n.is_empty() && *n != "None") else { continue };
            // The first definition of a name wins (as later references can only mean one).
            if self.palette.map.contains_key(name) {
                continue;
            }
            if c.flag(&["Register", "REGISTER"]) == Some(true) {
                self.palette.map.insert(name.to_string(), sw::REGISTRATION.into());
                continue;
            }
            let pct = |k: &str| c.num(&[k]).map(|v| (v / 100.0).clamp(0.0, 1.0) as f32);
            let byte = |k: &str| c.num(&[k]).map(|v| (v / 255.0).clamp(0.0, 1.0) as f32);
            let color = match c.any(&["Space", "SPACE"]) {
                Some("CMYK") => Some(Color::cmyk(pct("C").unwrap_or(0.0), pct("M").unwrap_or(0.0), pct("Y").unwrap_or(0.0), pct("K").unwrap_or(0.0))),
                Some("RGB") => Some(Color::rgb(byte("R").unwrap_or(0.0), byte("G").unwrap_or(0.0), byte("B").unwrap_or(0.0))),
                Some("Lab") => Some(lab_to_rgb(c.num(&["L"]).unwrap_or(0.0), c.num(&["A"]).unwrap_or(0.0), c.num(&["B"]).unwrap_or(0.0))),
                _ => c.get("CMYK").and_then(hex_cmyk).or_else(|| c.get("RGB").and_then(hex_rgb)),
            };
            match color {
                Some(color) => {
                    let spot = c.flag(&["Spot", "SPOT"]) == Some(true);
                    self.palette.add(name, color, spot);
                }
                None => self.warnings.push(format!("colour “{name}” has no value Scribus documents use; drawn in black")),
            }
        }
    }

    fn styles_from(&mut self, doc: &El) {
        // Names first: styles can name parents defined after them.
        let para_els: Vec<&El> = doc.named(&["STYLE", "ParagraphStyle"]).take(10_000).collect();
        let char_els: Vec<&El> = doc.named(&["CHARSTYLE", "CharacterStyle"]).take(10_000).collect();
        let is_default = |e: &El| e.flag(&["DefaultStyle"]) == Some(true);
        let mut default_char: Option<&El> = None;
        for e in &char_els {
            let Some(name) = e.any(&["CName", "Name", "CNAME"]) else { continue };
            if is_default(e) {
                default_char = Some(e);
                self.names.chars.insert(name.to_string(), designcraft_doc::NO_CHAR_STYLE.into());
            } else {
                self.names.chars.insert(name.to_string(), name.to_string());
            }
        }
        for e in &para_els {
            let Some(name) = e.any(&["Name", "NAME"]) else { continue };
            let target = if is_default(e) { BASIC_PARAGRAPH } else { name };
            self.names.para.insert(name.to_string(), target.to_string());
        }
        // Character styles, flattened onto their parents in DesignCraft's own `based_on`.
        let by_name: HashMap<&str, &El> = char_els.iter().filter_map(|e| Some((e.any(&["CName", "Name", "CNAME"])?, *e))).collect();
        // Text shadows aren't a character attribute in DesignCraft: each style's, with its
        // parents', is kept for the runs that use it (see `story_shadow`).
        self.names.base_shadow = default_char.map(ShadowAttrs::read).unwrap_or_default();
        for (name, e) in &by_name {
            let mut chain = vec![*e];
            let mut seen = BTreeSet::from([*name]);
            let mut parent = e.any(&["CParent", "CPARENT"]);
            while let Some(p) = parent.filter(|p| seen.insert(p) && seen.len() < 32) {
                let Some(pe) = by_name.get(p).filter(|pe| !is_default(pe)) else { break };
                chain.push(*pe);
                parent = pe.any(&["CParent", "CPARENT"]);
            }
            let mut attrs = ShadowAttrs::default();
            for ce in chain.iter().rev() {
                attrs.merge(&ShadowAttrs::read(ce));
            }
            self.names.char_shadows.insert(name.to_string(), attrs);
        }
        for e in &char_els {
            if is_default(e) {
                continue;
            }
            let Some(name) = e.any(&["CName", "Name", "CNAME"]) else { continue };
            let based_on = e.any(&["CParent", "CPARENT"]).map(|p| self.names.chars(Some(p))).filter(|p| p != designcraft_doc::NO_CHAR_STYLE);
            let chars = text::char_attrs(e, &mut self.palette);
            if self.styles.character.iter().all(|s| s.name != name) {
                self.styles.character.push(CharacterStyle { name: name.to_string(), based_on, chars, shortcut: String::new() });
            }
        }
        // The default character style is the base of all text: it goes under [Basic Paragraph].
        let mut basic_chars = default_char.map(|e| text::char_attrs(e, &mut self.palette)).unwrap_or_default();
        for e in &para_els {
            let Some(name) = e.any(&["Name", "NAME"]) else { continue };
            let (para, leading) = text::para_attrs(e);
            // A paragraph style's character attributes can come from a character style it names.
            let mut chars = CharAttrs::default();
            let mut seen = BTreeSet::new();
            let mut parent = e.any(&["CParent", "CPARENT"]);
            let mut chain = Vec::new();
            while let Some(p) = parent.filter(|p| seen.insert(p.to_string()) && seen.len() < 32) {
                let Some(ce) = by_name.get(p).filter(|ce| !is_default(ce)) else { break };
                chain.push(*ce);
                parent = ce.any(&["CParent", "CPARENT"]);
            }
            for ce in chain.iter().rev() {
                chars.merge(&text::char_attrs(ce, &mut self.palette));
            }
            chars.merge(&text::char_attrs(e, &mut self.palette));
            if leading.is_some() {
                chars.leading = leading;
            }
            if is_default(e) {
                basic_chars.merge(&chars);
                if let Some(basic) = self.styles.paragraph.iter_mut().find(|s| s.name == BASIC_PARAGRAPH) {
                    basic.para.merge(&para);
                }
                continue;
            }
            let based_on = Some(self.names.para(e.any(&["Parent", "PARENT"]))).filter(|p| p != name);
            if self.styles.paragraph.iter().all(|s| s.name != name) {
                self.styles.paragraph.push(ParagraphStyle {
                    name: name.to_string(),
                    based_on,
                    next_style: None,
                    para,
                    chars,
                    shortcut: String::new(),
                });
            }
        }
        if let Some(basic) = self.styles.paragraph.iter_mut().find(|s| s.name == BASIC_PARAGRAPH) {
            basic.chars.merge(&basic_chars);
            // Scribus breaks words only at the soft hyphens its Hyphenate Text puts in the text.
            basic.para.hyphenate = Some(false);
        }
    }

    fn layers_from(&mut self, doc: &El) {
        let mut layers: Vec<(i64, i64, Layer)> = Vec::new();
        for e in doc.named(&["LAYERS", "Layers"]).take(1000) {
            let number = e.num(&["Number", "NUMMER"]).unwrap_or(layers.len() as f64) as i64;
            if layers.iter().any(|(n, _, _)| *n == number) {
                continue;
            }
            let level = e.num(&["Level", "LEVEL"]).unwrap_or(number as f64) as i64;
            let color = e.any(&["LayerColor", "LAYERC"]).and_then(hex_bytes).unwrap_or([43, 155, 255]);
            let id = LayerId(self.alloc());
            let layer = Layer {
                id,
                name: e.any(&["Name", "NAME"]).filter(|n| !n.is_empty()).map_or_else(|| format!("Layer {}", number + 1), str::to_string),
                color,
                visible: e.flag(&["IsViewable", "SICHTBAR"]).unwrap_or(true),
                locked: !e.flag(&["IsEditable", "EDIT"]).unwrap_or(true),
                printable: e.flag(&["IsPrintable", "DRUCKEN"]).unwrap_or(true),
                show_guides: true,
                suppress_wrap_when_hidden: false,
            };
            layers.push((number, level, layer));
        }
        if layers.is_empty() {
            let id = LayerId(self.alloc());
            layers.push((
                0,
                0,
                Layer {
                    id,
                    name: "Layer 1".into(),
                    color: [43, 155, 255],
                    visible: true,
                    locked: false,
                    printable: true,
                    show_guides: true,
                    suppress_wrap_when_hidden: false,
                },
            ));
        }
        // DesignCraft lists the frontmost layer first.
        layers.sort_by_key(|(_, level, _)| std::cmp::Reverse(*level));
        for (number, level, layer) in &layers {
            self.layer_of.insert(*number, (layer.id, *level));
        }
        self.layers = layers.into_iter().map(|(_, _, l)| l).collect();
    }

    fn page_info(&self, e: &El) -> PageInfo {
        let width = e.num(&["PageWidth", "PAGEWIDTH"]).filter(|w| *w > 0.0).unwrap_or(self.settings.page_width).clamp(1.0, 15_552.0);
        let height = e.num(&["PageHeight", "PAGEHEIGHT"]).filter(|h| *h > 0.0).unwrap_or(self.settings.page_height).clamp(1.0, 15_552.0);
        let m = |k: &[&str]| e.num(k).unwrap_or(0.0).clamp(0.0, 10_000.0);
        let (l, r) = (m(&["BorderLeft", "BORDERLEFT"]), m(&["BorderRight", "BORDERRIGHT"]));
        let left = self.settings.facing_pages && e.flag(&["LeftPage", "LEFT"]) == Some(true);
        let guides = |k: &str| -> Vec<f64> {
            e.get(k).unwrap_or("").split_whitespace().filter_map(|v| v.parse::<f64>().ok()).filter(|v| v.is_finite()).take(1000).collect()
        };
        PageInfo {
            canvas: (e.num(&["PageXPosition", "PAGEXPOS"]).unwrap_or(0.0), e.num(&["PageYPosition", "PAGEYPOS"]).unwrap_or(0.0)),
            width,
            height,
            // The page's own left and right; the inside of a left page is its right.
            margins: Margins {
                top: m(&["BorderTop", "BORDERTOP"]),
                bottom: m(&["BorderBottom", "BORDERBOTTOM"]),
                inside: if left { r } else { l },
                outside: if left { l } else { r },
            },
            left,
            name: e.any(&["PageName", "NAM"]).unwrap_or("").to_string(),
            master: e.any(&["MasterPageName", "MNAM"]).unwrap_or("").to_string(),
            guides_v: guides("VerticalGuides"),
            guides_h: guides("HorizontalGuides"),
        }
    }

    fn make_page(&mut self, info: &PageInfo, x: f64, parent: Option<SpreadId>) -> Page {
        let side = match (self.settings.facing_pages, info.left) {
            (false, _) => PageSide::Single,
            (true, true) => PageSide::Left,
            (true, false) => PageSide::Right,
        };
        let guide = |orientation, position| Guide { orientation, position, spread: false, locked: false, layer: None, liquid: false };
        let mut guides: Vec<Guide> = info.guides_v.iter().map(|g| guide(Orientation::Vertical, x + g)).collect();
        guides.extend(info.guides_h.iter().map(|g| guide(Orientation::Horizontal, *g)));
        Page {
            id: PageId(self.alloc()),
            width: info.width,
            height: info.height,
            x,
            margins: info.margins,
            columns: Columns { count: 1, gutter: 12.0, positions: None },
            local_margins: false,
            parent,
            side,
            overridden: vec![],
            guides,
            show_parent_items: true,
            liquid: Default::default(),
            transition: None,
            view_rotation: 0,
            show_parent_overlay: false,
        }
    }

    fn masters(&mut self, doc: &El) {
        for (i, e) in doc.named(&["MASTERPAGE", "MasterPage"]).take(1000).enumerate() {
            let info = self.page_info(e);
            let name = if info.name.is_empty() { format!("Master {}", i + 1) } else { info.name.clone() };
            if self.master_of.contains_key(&name) {
                continue;
            }
            let page = self.make_page(&info, 0.0, None);
            let prefix = if i < 26 { char::from(b'A' + i as u8).to_string() } else { format!("M{}", i + 1) };
            let id = SpreadId(self.alloc());
            self.parents.push(Spread {
                id,
                pages: vec![page],
                items: vec![],
                parent: Some(ParentInfo { prefix, name: name.clone(), based_on: None }),
                allow_shuffle: true,
            });
            self.master_of.insert(name, (self.parents.len() - 1, info.canvas));
        }
    }

    fn pages(&mut self, doc: &El) -> Result<()> {
        let mut pages: Vec<(f64, PageInfo)> =
            doc.named(&["PAGE", "Page"]).enumerate().map(|(i, e)| (e.num(&["PageNumber", "NUM"]).unwrap_or(i as f64), self.page_info(e))).collect();
        if pages.len() > MAX_PAGES {
            self.warnings.push(format!("only the first {MAX_PAGES} of {} pages were imported", pages.len()));
            pages.truncate(MAX_PAGES);
        }
        pages.sort_by(|a, b| a.0.total_cmp(&b.0));
        if pages.is_empty() {
            return Err(SlaError::Invalid("the document has no pages".into()));
        }
        // Pages side by side on one canvas row form a spread.
        let mut prev: Option<(f64, f64, f64)> = None;
        for (_, info) in &pages {
            let joins = prev.is_some_and(|(px, py, pw)| (info.canvas.1 - py).abs() < 1.0 && (info.canvas.0 - (px + pw)).abs() < 1.0)
                && self.spreads.last().is_some_and(|s| s.pages.len() < 10);
            if !joins {
                let id = SpreadId(self.alloc());
                self.spreads.push(Spread { id, pages: vec![], items: vec![], parent: None, allow_shuffle: true });
            }
            let si = self.spreads.len() - 1;
            let origin = match self.spreads.get(si).and_then(|s| s.pages.first()) {
                Some(_) => self.page_spread.last().map_or(info.canvas, |(_, o)| *o),
                None => info.canvas,
            };
            let parent = self.master_of.get(&info.master).and_then(|(i, _)| self.parents.get(*i)).map(|s| s.id);
            let page = self.make_page(info, info.canvas.0 - origin.0, parent);
            if let Some(s) = self.spreads.get_mut(si) {
                s.pages.push(page);
            }
            self.page_spread.push((si, origin));
            self.page_rects.push(Rect::new(info.canvas.0, info.canvas.1, info.canvas.0 + info.width, info.canvas.1 + info.height));
            prev = Some((info.canvas.0, info.canvas.1, info.width));
        }
        Ok(())
    }

    fn sections_from(&mut self, doc: &El) {
        let pages = self.page_spread.len();
        let mut sections: Vec<Section> = Vec::new();
        if let Some(list) = doc.find(&["Sections"]) {
            for e in list.named(&["Section"]).take(1000) {
                if e.flag(&["Active"]) == Some(false) {
                    continue;
                }
                let start = e.num(&["From"]).unwrap_or(0.0).max(0.0) as usize;
                if start >= pages || sections.iter().any(|s| s.start == start) {
                    continue;
                }
                let style = match e.get("Type").unwrap_or("") {
                    "Type_i_ii_iii" => NumberStyle::LowerRoman,
                    "Type_I_II_III" => NumberStyle::UpperRoman,
                    "Type_a_b_c" => NumberStyle::LowerLetters,
                    "Type_A_B_C" => NumberStyle::UpperLetters,
                    _ => NumberStyle::Arabic,
                };
                let start_number = e.num(&["Start"]).map(|n| n.clamp(1.0, 1e9) as u32);
                sections.push(Section { start, start_number, style, prefix: String::new(), marker: String::new(), include_prefix: false });
            }
        }
        sections.sort_by_key(|s| s.start);
        if sections.first().is_none_or(|s| s.start != 0) {
            let first = doc.num(&["FirstPageNumber", "FIRSTNUM"]).map_or(1, |n| n.clamp(1.0, 1e9) as u32);
            sections.insert(
                0,
                Section {
                    start: 0,
                    start_number: Some(first),
                    style: NumberStyle::Arabic,
                    prefix: String::new(),
                    marker: String::new(),
                    include_prefix: false,
                },
            );
        }
        self.sections = sections;
    }

    /// Thread the text frames (by `ItemID`) and build one story per thread from its first frame.
    fn stories_from(&mut self, doc: &El) {
        let mut frames: Vec<(i64, FrameLink, &El)> = Vec::new();
        let mut seen = BTreeSet::new();
        let mut stack: Vec<(&El, usize)> = doc.named(ITEM_ELEMENTS).map(|e| (e, 0)).collect();
        while let Some((e, depth)) = stack.pop() {
            match item_type(e) {
                4 | 8 => {
                    if let Some(id) = e.num(&["ItemID"]).map(|v| v as i64).filter(|id| seen.insert(*id)) {
                        let link = |k: &[&str]| e.num(k).map(|v| v as i64).filter(|v| *v >= 0);
                        frames.push((id, FrameLink { next: link(&["NextItem", "NEXTITEM"]), back: link(&["BackItem", "BACKITEM"]) }, e));
                    }
                }
                12 if depth < MAX_GROUP_DEPTH => stack.extend(e.named(ITEM_ELEMENTS).map(|c| (c, depth + 1))),
                _ => {}
            }
        }
        let by_id: HashMap<i64, usize> = frames.iter().enumerate().map(|(i, f)| (f.0, i)).collect();
        let mut placed = BTreeSet::new();
        // Heads first (no frame before them), then anything a broken chain left out.
        let heads: Vec<usize> = (0..frames.len())
            .filter(|i| frames.get(*i).is_some_and(|f| f.1.back.is_none_or(|b| !by_id.contains_key(&b))))
            .chain(0..frames.len())
            .collect();
        for head in heads {
            if placed.contains(&head) {
                continue;
            }
            let mut chain = Vec::new();
            let mut at = Some(head);
            while let Some(i) = at.filter(|i| !placed.contains(i)) {
                placed.insert(i);
                chain.push(i);
                at = frames.get(i).and_then(|f| f.1.next).and_then(|n| by_id.get(&n).copied());
            }
            let sid = StoryId(self.alloc());
            let mut b = StoryBuilder::new(&self.names);
            // The first frame holds the thread's text; read the others only if it is empty.
            for i in &chain {
                if !b.is_empty() {
                    break;
                }
                if let Some((_, _, e)) = frames.get(*i) {
                    let source = e.find(&["StoryText"]).unwrap_or(e);
                    b.read(source, &mut self.palette);
                }
            }
            if b.truncated {
                self.warnings.push("a story longer than 64 MB was cut short".into());
            }
            let shadow = b.shadow(|f, p| run_size(&self.styles, f, p));
            let story = b.finish(sid);
            self.note_shadow(sid, shadow);
            let ids: Vec<i64> = chain.iter().filter_map(|i| frames.get(*i).map(|f| f.0)).collect();
            for id in &ids {
                self.story_of.insert(*id, sid);
            }
            self.stories.insert(sid, story);
            self.threads.insert(sid, ids);
        }
    }

    fn note_shadow(&mut self, sid: StoryId, shadow: StoryShadow) {
        match shadow {
            StoryShadow::None => {}
            StoryShadow::All(sh) => {
                self.story_shadow.insert(sid, sh);
            }
            StoryShadow::Partial => self.partly_shadowed = self.partly_shadowed.saturating_add(1),
        }
    }

    /// The spread a page item goes on, and that spread's canvas origin: the page that owns it,
    /// else the page it lies on (or nearest; the first page once [`MAX_PAGE_LOOKUPS`] is used up).
    fn spread_for(&mut self, e: &El) -> Option<(usize, (f64, f64))> {
        if let Some(own) = e.num(&["OwnPage"]).filter(|p| *p >= 0.0).map(|p| p as usize)
            && let Some(at) = self.page_spread.get(own)
        {
            return Some(*at);
        }
        let x = e.num(&["XPosition", "XPOS"]).unwrap_or(0.0) + e.num(&["Width", "WIDTH"]).unwrap_or(0.0) / 2.0;
        let y = e.num(&["YPosition", "YPOS"]).unwrap_or(0.0) + e.num(&["Height", "HEIGHT"]).unwrap_or(0.0) / 2.0;
        let distance = |r: &Rect| {
            let dx = (r.x0 - x).max(x - r.x1).max(0.0);
            let dy = (r.y0 - y).max(y - r.y1).max(0.0);
            dx.hypot(dy)
        };
        self.page_lookups = self.page_lookups.saturating_add(self.page_rects.len());
        if self.page_lookups > MAX_PAGE_LOOKUPS {
            self.unplaced = self.unplaced.saturating_add(1);
            return self.page_spread.first().copied();
        }
        let nearest = self.page_rects.iter().enumerate().min_by(|a, b| distance(a.1).total_cmp(&distance(b.1)))?.0;
        self.page_spread.get(nearest).copied()
    }

    fn layer(&self, e: &El) -> LayerId {
        let number = e.num(&["Layer", "LAYER"]).map(|n| n as i64).unwrap_or(0);
        self.layer_of.get(&number).or_else(|| self.layer_of.values().min_by_key(|(_, level)| *level)).map(|(id, _)| *id).unwrap_or(LayerId(0))
    }

    /// The items for `e`, placed relative to `origin` (its spread's canvas origin, or its
    /// group's): one, or two for a shadowed text frame with a fill or stroke.
    fn item(&mut self, e: &El, origin: (f64, f64), depth: usize) -> Vec<Item> {
        if self.items >= MAX_ITEMS {
            *self.skipped.entry("items past the import limit").or_default() += 1;
            return Vec::new();
        }
        self.items += 1;
        let kind = item_type(e);
        let reason = match kind {
            9 => Some("LaTeX and render frames"),
            10 => Some("3D (OSG) frames"),
            11 => Some("symbols"),
            16 => Some("tables"),
            17 => Some("note frames"),
            _ => None,
        };
        if let Some(reason) = reason {
            *self.skipped.entry(reason).or_default() += 1;
            return Vec::new();
        }
        let x = e.num(&["XPosition", "XPOS"]).unwrap_or(0.0);
        let y = e.num(&["YPosition", "YPOS"]).unwrap_or(0.0);
        let w = e.num(&["Width", "WIDTH"]).unwrap_or(0.0).clamp(0.0, 1e6);
        let h = e.num(&["Height", "HEIGHT"]).unwrap_or(0.0).clamp(0.0, 1e6);
        let rotation = e.num(&["Rotation", "ROT"]).unwrap_or(0.0);
        let flip_h = e.flag(&["FlippedH", "FLIPPEDH", "FlipHorizontal"]) == Some(true);
        let flip_v = e.flag(&["FlippedV", "FLIPPEDV", "FlipVertical"]) == Some(true);
        let mut flip = Affine::IDENTITY;
        if flip_h {
            flip = Affine::new([-1.0, 0.0, 0.0, 1.0, w, 0.0]) * flip;
        }
        if flip_v {
            flip = Affine::new([1.0, 0.0, 0.0, -1.0, 0.0, h]) * flip;
        }
        let xf = Affine::translate((x - origin.0, y - origin.1)) * Affine::rotate(rotation.to_radians());
        let rect = Rect::new(0.0, 0.0, w, h);
        let mut bez = e.any(&["Path", "path"]).and_then(|p| BezPath::from_svg(p).ok()).filter(|b| !b.elements().is_empty());
        if let Some(b) = bez.as_mut() {
            b.apply_affine(flip);
        }
        let frame_type = e.num(&["FrameType", "FRTYPE"]).unwrap_or(0.0) as i64;
        let (shape, path) = match kind {
            5 => (Shape::GraphicLine, bez.map(|b| PathData::from_bezpath(&b)).unwrap_or_else(|| shapes::line((0.0, 0.0).into(), (w, 0.0).into()))),
            12 => (Shape::Group, shapes::rectangle(rect)),
            _ => {
                let shape = match (kind, frame_type) {
                    // Polylines, regular polygons, arcs and spirals; a polygon is the rectangle
                    // or ellipse its frame type says until its path is edited.
                    (7 | 13 | 14 | 15, _) => Shape::Path,
                    (_, 1) => Shape::Oval,
                    (_, 0) => Shape::Rectangle,
                    _ => Shape::Path,
                };
                let fallback = || if frame_type == 1 { shapes::ellipse(rect) } else { shapes::rectangle(rect) };
                (shape, bez.map(|b| PathData::from_bezpath(&b)).unwrap_or_else(fallback))
            }
        };
        let id = ItemId(self.alloc());
        let mut item = Item::new(id, self.layer(e), shape, path);
        item.xf = xf;
        if let Some(name) = e.any(&["AutoName", "ANNAME"]).filter(|n| !n.is_empty()) {
            item.name = name.chars().take(256).collect();
        }
        item.locked = e.flag(&["Locked", "LOCK"]) == Some(true);
        item.nonprinting = e.flag(&["Printable", "PRINTABLE"]) == Some(false);
        let transparency = e.num(&["FillTransparency", "TransValue"]).unwrap_or(0.0).clamp(0.0, 1.0);
        item.opacity = (1.0 - transparency) as f32;
        if kind != 12 {
            item.fill = self.fill(e);
            item.stroke = self.stroke(e);
        }
        if kind == 5 {
            item.fill = Fill::none();
        }
        match kind {
            2 => item.content = self.graphic(e, w, h, flip),
            4 | 8 => {
                if kind == 8 {
                    *self.skipped.entry("text on a path (imported as text frames)").or_default() += 1;
                    item.path = shapes::rectangle(rect);
                    item.shape = Shape::Rectangle;
                }
                let scribus_id = e.num(&["ItemID"]).map(|v| v as i64);
                let story = match scribus_id.and_then(|i| self.story_of.get(&i).copied()) {
                    Some(sid) if !scribus_id.is_some_and(|i| self.frame_ids.contains_key(&i)) => {
                        if let Some(i) = scribus_id {
                            self.frame_ids.insert(i, id);
                        }
                        sid
                    }
                    _ => {
                        // No usable ItemID: a story of its own.
                        let sid = StoryId(self.alloc());
                        let mut b = StoryBuilder::new(&self.names);
                        b.read(e.find(&["StoryText"]).unwrap_or(e), &mut self.palette);
                        let shadow = b.shadow(|f, p| run_size(&self.styles, f, p));
                        self.stories.insert(sid, b.finish(sid));
                        self.note_shadow(sid, shadow);
                        self.loose_frames.push((sid, id));
                        sid
                    }
                };
                item.content = Content::Text(TextFrame { story, options: text_options(e) });
                if let Some(sh) = self.story_shadow.get(&story) {
                    return self.shadowed_frame(item, sh.clone(), rotation);
                }
            }
            12 => {
                let group_xf = Affine::rotate(-rotation.to_radians());
                let mut items = Vec::new();
                if depth < MAX_GROUP_DEPTH {
                    for c in e.named(ITEM_ELEMENTS) {
                        for mut child in self.item(c, (x, y), depth + 1) {
                            child.xf = group_xf * child.xf;
                            items.push(Arc::new(child));
                        }
                    }
                } else {
                    *self.skipped.entry("groups nested too deeply").or_default() += 1;
                }
                if items.is_empty() {
                    return Vec::new();
                }
                item.fill = Fill::none();
                item.stroke = Stroke::none();
                item.content = Content::Group { items };
            }
            _ => {}
        }
        vec![item]
    }

    /// A text frame whose text casts Scribus' text shadow. DesignCraft has no character shadow;
    /// a frame's drop shadow is the shadow of its text when the frame has no fill or stroke, so
    /// those move to a rectangle of their own behind it. Scribus offsets the shadow in the
    /// frame's text space, a drop shadow on the page.
    fn shadowed_frame(&mut self, mut item: Item, sh: TextShadow, rotation: f64) -> Vec<Item> {
        let mut items = Vec::new();
        if !item.fill.is_none() || !item.stroke.is_none() {
            let mut back = Item::new(ItemId(self.alloc()), item.layer, item.shape, item.path.clone());
            back.xf = item.xf;
            back.name = item.name.clone();
            back.locked = item.locked;
            back.nonprinting = item.nonprinting;
            back.opacity = item.opacity;
            back.fill = std::mem::replace(&mut item.fill, Fill::none());
            back.stroke = std::mem::replace(&mut item.stroke, Stroke::none());
            items.push(back);
        }
        let off = Affine::rotate(rotation.to_radians()) * designcraft_geom::Point::new(sh.dx, sh.dy);
        let ds = &mut item.effects.drop_shadow;
        ds.on = true;
        ds.color = sh.color;
        ds.opacity = sh.tint;
        // The drop shadow's angle is where the light comes from: opposite the offset.
        ds.angle = off.y.atan2(-off.x).to_degrees();
        ds.distance = off.to_vec2().hypot();
        ds.size = 0.0;
        ds.spread = 0.0;
        items.push(item);
        items
    }

    fn fill(&mut self, e: &El) -> Fill {
        let name = e.any(&["FillColor", "PCOLOR"]).unwrap_or("None");
        let swatch = self.palette.swatch(name);
        if swatch == sw::NONE {
            return Fill::none();
        }
        let mut f = Fill::swatch(&swatch);
        f.tint = text::shade(e.num(&["FillShade", "SHADE"]).unwrap_or(100.0));
        f
    }

    fn stroke(&mut self, e: &El) -> Stroke {
        let name = e.any(&["LineColor", "PCOLOR2"]).unwrap_or("None");
        let weight = e.num(&["LineWidth", "PWIDTH"]).unwrap_or(1.0).clamp(0.0, 1000.0);
        let swatch = self.palette.swatch(name);
        if swatch == sw::NONE || weight <= 0.0 {
            return Stroke::none();
        }
        Stroke { swatch, tint: text::shade(e.num(&["LineShade", "SHADE2"]).unwrap_or(100.0)), weight, ..Stroke::default() }
    }

    /// An image frame's content: the image, linked or embedded, scaled and offset in the frame.
    fn graphic(&mut self, e: &El, w: f64, h: f64, flip: Affine) -> Content {
        let file = e.any(&["ImageFileName", "PFILE"]).map(str::trim).filter(|f| !f.is_empty()).map(str::to_string);
        let embedded = e.get("ImageData").filter(|d| !d.trim().is_empty());
        if file.is_none() && embedded.is_none() {
            return Content::Unassigned;
        }
        let sx = e.num(&["ImageScaleX", "LOCALSCX"]).filter(|s| *s != 0.0).unwrap_or(1.0).clamp(-1e4, 1e4);
        let sy = e.num(&["ImageScaleY", "LOCALSCY"]).filter(|s| *s != 0.0).unwrap_or(1.0).clamp(-1e4, 1e4);
        let ox = e.num(&["ImageOffsetX", "LOCALX"]).unwrap_or(0.0);
        let oy = e.num(&["ImageOffsetY", "LOCALY"]).unwrap_or(0.0);
        let rot = e.num(&["ImageRotation", "LOCALROT"]).unwrap_or(0.0);
        // One image pixel to the point, scaled, rotated and offset (in image units).
        let gxf = flip * Affine::translate((ox * sx, oy * sy)) * Affine::rotate(rot.to_radians()) * Affine::scale_non_uniform(sx, sy);
        let fallback = (w / sx.abs(), h / sy.abs());
        let (asset, size) = match (embedded, &file) {
            (Some(data), _) => {
                let ext = e.get("inlineImageExt").unwrap_or("");
                let budget = MAX_TOTAL_EMBEDDED.saturating_sub(self.embedded_bytes);
                match inflate_embedded(data, budget) {
                    Some(bytes) => {
                        self.embedded_bytes = self.embedded_bytes.saturating_add(bytes.len() as u64);
                        self.add_asset(bytes, file.as_deref().or(Some(ext)).unwrap_or("image"), None, fallback)
                    }
                    None if budget < MAX_EMBEDDED_IMAGE => {
                        *self.skipped.entry("embedded images past the 1 GB total").or_default() += 1;
                        return Content::Unassigned;
                    }
                    None => {
                        self.warnings.push("an embedded image couldn't be read".into());
                        return Content::Unassigned;
                    }
                }
            }
            (None, Some(path)) => {
                if let Some(hit) = self.linked.get(path) {
                    *hit
                } else {
                    // Once the total is used up, the files aren't read at all.
                    let data = if self.linked_bytes >= MAX_TOTAL_LINKED { Vec::new() } else { (self.read_link)(path).unwrap_or_default() };
                    let data = if self.linked_bytes >= MAX_TOTAL_LINKED {
                        *self.skipped.entry("linked images past the 4 GB total (left unloaded)").or_default() += 1;
                        data
                    } else if !data.is_empty() && designcraft_images::mime(&data) == "application/octet-stream" && !designcraft_images::is_eps(&data)
                    {
                        self.unsupported_links.insert(path.clone());
                        Vec::new()
                    } else {
                        if data.is_empty() {
                            self.missing_links.insert(path.clone());
                        }
                        data
                    };
                    // Past the total the image stays linked, without its data, to be relinked.
                    let data = if data.len() as u64 > MAX_TOTAL_LINKED.saturating_sub(self.linked_bytes) {
                        *self.skipped.entry("linked images past the 4 GB total (left unloaded)").or_default() += 1;
                        Vec::new()
                    } else {
                        self.linked_bytes = self.linked_bytes.saturating_add(data.len() as u64);
                        data
                    };
                    let hit = self.add_asset(data, path, Some(path.clone()), fallback);
                    self.linked.insert(path.clone(), hit);
                    hit
                }
            }
            (None, None) => return Content::Unassigned,
        };
        Content::Graphic(Graphic {
            asset,
            size,
            xf: gxf,
            clip: None,
            auto_fit: Default::default(),
            fit_align: 4,
            crop: [0.0; 4],
            wrap: Default::default(),
            fill: Default::default(),
        })
    }

    fn add_asset(&mut self, data: Vec<u8>, path: &str, link: Option<String>, fallback: (f64, f64)) -> (AssetId, (f64, f64)) {
        let size = designcraft_images::natural_size(&data).filter(|(w, h)| *w > 0.0 && *h > 0.0).unwrap_or(fallback);
        let mime = if data.is_empty() { mime_from_name(path) } else { designcraft_images::mime(&data).to_string() };
        let pixels = designcraft_images::pixel_size(&data);
        let name = path.rsplit(['/', '\\']).next().filter(|n| !n.is_empty()).unwrap_or("image").to_string();
        let fresh = AssetId(self.alloc());
        let id = designcraft_doc::insert_asset(
            &mut self.assets,
            Asset { id: fresh, name, mime, link, data: Arc::new(data), pixels, ..Default::default() },
        );
        (id, size)
    }

    /// Items on lower layers first, keeping the file's order within a layer.
    fn order_by_layer(&mut self) {
        let level: HashMap<LayerId, i64> = self.layer_of.values().map(|(id, l)| (*id, *l)).collect();
        for s in self.spreads.iter_mut().chain(self.parents.iter_mut()) {
            s.items.sort_by_key(|it| level.get(&it.layer).copied().unwrap_or(0));
        }
    }

    fn link_threads(&mut self) {
        for (sid, ids) in &self.threads {
            let frames: Vec<ItemId> = ids.iter().filter_map(|i| self.frame_ids.get(i).copied()).collect();
            if let Some(story) = self.stories.get_mut(sid) {
                story.frames = frames;
            }
        }
        for (sid, id) in &self.loose_frames {
            if let Some(story) = self.stories.get_mut(sid) {
                story.frames = vec![*id];
            }
        }
        // A thread whose frames were all dropped (tables, limits) leaves no story behind.
        self.stories.retain(|_, s| !s.frames.is_empty());
    }

    fn finish(mut self, doc: &El) -> Result<Imported> {
        let mut warnings = std::mem::take(&mut self.warnings);
        for (what, n) in &self.skipped {
            warnings.push(format!("{n} {what} not imported"));
        }
        if self.partly_shadowed > 0 {
            warnings.push(format!("text shadows on part of a story are not imported ({} stories)", self.partly_shadowed));
        }
        if self.unplaced > 0 {
            warnings.push(format!("{} items without a page of their own were put on the first page", self.unplaced));
        }
        if !self.missing_links.is_empty() {
            let list: Vec<&str> = self.missing_links.iter().map(String::as_str).take(20).collect();
            warnings.push(format!("{} linked images not found: {}", self.missing_links.len(), list.join(", ")));
        }
        if !self.unsupported_links.is_empty() {
            let list: Vec<&str> = self.unsupported_links.iter().map(String::as_str).take(20).collect();
            warnings.push(format!("{} linked images are in a format DesignCraft can't place: {}", self.unsupported_links.len(), list.join(", ")));
        }
        if !self.palette.unknown.is_empty() {
            let list: Vec<&str> = self.palette.unknown.iter().map(String::as_str).take(20).collect();
            warnings.push(format!("colours the document doesn't define, drawn in black: {}", list.join(", ")));
        }
        let title = doc.any(&["DocumentInfoTitle", "TITLE"]).map(str::trim).filter(|t| !t.is_empty()).unwrap_or("Untitled").to_string();
        let mut d = Document {
            title,
            settings: self.settings,
            spreads: self.spreads.into_iter().map(Arc::new).collect(),
            parents: self.parents.into_iter().map(Arc::new).collect(),
            layers: self.layers,
            stories: self.stories.into_iter().map(|(k, v)| (k, Arc::new(v))).collect(),
            styles: Arc::new(self.styles),
            swatches: self.palette.swatches,
            color_groups: Default::default(),
            conditions: Default::default(),
            inks: Default::default(),
            stroke_styles: Default::default(),
            articles: vec![],
            xml: Default::default(),
            endnote_options: Default::default(),
            endnote_story: None,
            sections: self.sections,
            assets: self.assets,
            hyperlinks: vec![],
            data_merge: Default::default(),
            bookmarks: vec![],
            user_words: vec![],
            hyphenation_exceptions: vec![],
            toc: None,
            text_variables: designcraft_doc::vars::defaults(),
            footnote_options: Default::default(),
            xref_formats: designcraft_doc::xref::default_formats(),
            index: None,
            created: designcraft_doc::vars::now(),
            modified: 0,
            next_id: self.next_id,
            font_scope: 0,
        };
        warnings.extend(d.fix_parent_bases());
        d.check().map_err(|e| SlaError::Invalid(e.to_string()))?;
        Ok(Imported { document: d, warnings })
    }
}

/// The font size a run gets: its own, its character style's, its paragraph style's, the
/// document default's, as DesignCraft resolves them.
fn run_size(styles: &Styles, f: &CharFormat, p: &ParaFormat) -> f64 {
    styles.resolve_char(&styles.resolve_para(p).1, f).size
}

/// `ItemType` (1.7) or `PTYPE`: 2 image, 4 text, 5 line, 6 polygon, 7 polyline, 8 text on a path,
/// 12 group, 16 table, …
fn item_type(e: &El) -> i64 {
    e.num(&["ItemType", "PTYPE"]).map_or(6, |t| t as i64)
}

fn text_options(e: &El) -> TextFrameOptions {
    let inset = |k: &[&str]| e.num(k).unwrap_or(0.0).clamp(0.0, 10_000.0);
    TextFrameOptions {
        columns: e.num(&["Columns", "COLUMNS"]).unwrap_or(1.0).clamp(1.0, 40.0) as u32,
        gutter: e.num(&["ColumnGap", "COLGAP"]).unwrap_or(0.0).clamp(0.0, 10_000.0),
        inset: [
            inset(&["TextToFrameDistanceTop", "TEXTRA"]),
            inset(&["TextToFrameDistanceLeft", "EXTRA"]),
            inset(&["TextToFrameDistanceBottom", "BEXTRA"]),
            inset(&["TextToFrameDistanceRight", "REXTRA"]),
        ],
        vertical_justification: match e.num(&["VerticalAlignment", "VAlign"]).map(|v| v as i64) {
            Some(1) => VerticalJustification::Center,
            Some(2) => VerticalJustification::Bottom,
            _ => VerticalJustification::Top,
        },
        // 0 maximum ascent, 1 font ascent, 2 line spacing, 3 baseline grid.
        first_baseline: match e.num(&["FirstLineOffset", "FLOP"]).map(|v| v as i64) {
            Some(2 | 3) => FirstBaseline::Leading,
            _ => FirstBaseline::Ascent,
        },
        ..Default::default()
    }
}

fn mime_from_name(path: &str) -> String {
    let ext = path.rsplit('.').next().unwrap_or("").to_ascii_lowercase();
    match ext.as_str() {
        "jpg" | "jpeg" => "image/jpeg",
        "tif" | "tiff" => "image/tiff",
        "gif" => "image/gif",
        "webp" => "image/webp",
        "svg" => "image/svg+xml",
        "pdf" => "application/pdf",
        "eps" | "ps" => "application/postscript",
        "psd" => "image/vnd.adobe.photoshop",
        _ => "image/png",
    }
    .into()
}

/// An embedded image (`ImageData`): base64 of Qt's compressed form, a 4-byte big-endian length
/// followed by a zlib stream.
/// `None` when it can't be read or inflates past `budget` (or [`MAX_EMBEDDED_IMAGE`]).
fn inflate_embedded(b64: &str, budget: u64) -> Option<Vec<u8>> {
    let cap = budget.min(MAX_EMBEDDED_IMAGE);
    let raw = base64_decode(b64)?;
    let stream = raw.get(4..)?;
    let mut out = Vec::new();
    flate2::read::ZlibDecoder::new(stream).take(cap.saturating_add(1)).read_to_end(&mut out).ok()?;
    (!out.is_empty() && out.len() as u64 <= cap).then_some(out)
}

/// Standard base64 (whitespace ignored); `None` on any other character.
fn base64_decode(s: &str) -> Option<Vec<u8>> {
    let mut out = Vec::with_capacity(s.len() / 4 * 3);
    let mut acc = 0u32;
    let mut bits = 0u32;
    for c in s.bytes() {
        let v = match c {
            b'A'..=b'Z' => c - b'A',
            b'a'..=b'z' => c - b'a' + 26,
            b'0'..=b'9' => c - b'0' + 52,
            b'+' => 62,
            b'/' => 63,
            b'=' => break,
            c if c.is_ascii_whitespace() => continue,
            _ => return None,
        };
        acc = (acc << 6) | u32::from(v);
        bits += 6;
        if bits >= 8 {
            bits -= 8;
            out.push((acc >> bits) as u8);
            acc &= (1 << bits) - 1;
        }
    }
    Some(out)
}

#[cfg(test)]
mod unit_tests {
    use super::*;

    #[test]
    fn base64_round_trips_known_values() {
        assert_eq!(base64_decode("aGVsbG8=").as_deref(), Some(&b"hello"[..]));
        assert_eq!(base64_decode("aGVs\nbG8").as_deref(), Some(&b"hello"[..]));
        assert_eq!(base64_decode("a$b"), None);
    }

    #[test]
    fn embedded_images_stop_at_their_budget() {
        use std::io::Write;
        let mut z = flate2::write::ZlibEncoder::new(Vec::new(), flate2::Compression::best());
        z.write_all(&[0u8; 4096]).unwrap();
        let mut q = 4096u32.to_be_bytes().to_vec();
        q.extend(z.finish().unwrap());
        let b64 = crate::tests::base64(&q);
        assert_eq!(inflate_embedded(&b64, 4096).map(|b| b.len()), Some(4096));
        assert_eq!(inflate_embedded(&b64, 4095), None, "past the remaining budget");
        assert_eq!(inflate_embedded(&b64, 0), None);
    }

    #[test]
    fn linked_images_stop_at_the_total() {
        let mut sla = String::from(r#"<SCRIBUSUTF8NEW Version="1.7"><Document><Page PageWidth="10" PageHeight="10"/>"#);
        for i in 0..3 {
            sla.push_str(&format!(r#"<PageObject ItemType="2" Width="5" Height="5" ImageFileName="{i}.svg"/>"#));
        }
        sla.push_str("</Document></SCRIBUSUTF8NEW>");
        // Three images of 63 bytes with `used` bytes of the total taken → (images loaded, files read, warnings).
        let run = |used: u64| {
            let reads = std::cell::Cell::new(0);
            let read = |_: &str| {
                reads.set(reads.get() + 1);
                Some(br#"<svg xmlns="http://www.w3.org/2000/svg" width="1" height="1"/>"#.to_vec())
            };
            let mut im = Importer::new(&read, &|_| false);
            im.linked_bytes = used;
            let root = crate::xml::parse(sla.as_bytes()).unwrap();
            let doc = root.find(&["Document"]).unwrap();
            im.run(doc).unwrap();
            let out = im.finish(doc).unwrap();
            assert_eq!(out.document.assets.len(), 3, "every image stays linked");
            (out.document.assets.values().filter(|a| !a.data.is_empty()).count(), reads.get(), out.warnings)
        };
        // Room for one.
        let (loaded, _, warnings) = run(MAX_TOTAL_LINKED - 70);
        assert_eq!(loaded, 1);
        assert!(warnings.iter().any(|w| w.contains("2 linked images past the 4 GB total")), "{warnings:?}");
        // None left: no file is even read.
        let (loaded, reads, warnings) = run(MAX_TOTAL_LINKED);
        assert_eq!((loaded, reads), (0, 0));
        assert!(warnings.iter().any(|w| w.contains("3 linked images past the 4 GB total")), "{warnings:?}");
    }

    #[test]
    fn oversized_files_are_refused() {
        let mut big = br#"<SCRIBUSUTF8NEW Version="1.7">"#.to_vec();
        big.resize(crate::MAX_BYTES + 1, b' ');
        let err = import_sla(&big, &|_| None).map(|_| ()).unwrap_err();
        assert!(err.to_string().contains("larger than"), "{err}");
    }

    #[test]
    fn repeated_colour_names_keep_their_first_definition() {
        let mut colours = String::new();
        for i in 0..5000 {
            colours.push_str(&format!(r#"<Color Name="X" Space="RGB" R="{}" G="0" B="0"/>"#, i % 256));
        }
        let sla = format!(r#"<SCRIBUSUTF8NEW Version="1.7"><Document>{colours}<Page PageWidth="10" PageHeight="10"/></Document></SCRIBUSUTF8NEW>"#);
        let started = std::time::Instant::now();
        let d = import_sla(sla.as_bytes(), &|_| None).unwrap();
        assert!(started.elapsed().as_secs() < 5, "repeats must not cost quadratic time");
        let xs: Vec<_> = d.swatches.iter().filter(|s| s.name.starts_with('X')).collect();
        assert_eq!(xs.len(), 1, "no orphaned copies");
        assert!(matches!(xs[0].value, SwatchValue::Color { color: Color::Rgb { r, .. }, .. } if r == 0.0));
    }

    #[test]
    fn colour_values() {
        assert_eq!(hex_rgb("#ff0080"), Some(Color::rgb8(255, 0, 128)));
        assert_eq!(hex_rgb("#ff00"), None);
        assert!(matches!(hex_cmyk("#ff000000"), Some(Color::Cmyk { c, .. }) if c == 1.0));
        // L* 100 is white, L* 0 black.
        assert!(matches!(lab_to_rgb(100.0, 0.0, 0.0), Color::Rgb { r, g, b } if r > 0.99 && g > 0.99 && b > 0.99));
        assert_eq!(lab_to_rgb(0.0, 0.0, 0.0), Color::rgb(0.0, 0.0, 0.0));
    }

    #[test]
    fn colours_named_after_their_value_are_made_on_demand() {
        let mut p = Palette::new(&|_| false);
        let name = p.swatch("FromSVG#336699");
        assert_eq!(name, "FromSVG#336699");
        assert!(p.swatches.iter().any(|s| s.name == name));
        assert_eq!(p.swatch("Nonexistent"), sw::BLACK);
        assert_eq!(p.swatch("None"), sw::NONE);
    }
}
