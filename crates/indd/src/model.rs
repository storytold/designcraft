//! Document model built from INDD objects.

use std::collections::{BTreeMap, HashMap};
use std::sync::Arc;

use crate::attrs::{self, Attrs};
use crate::bytes::{f64_at, transform, u16_at, u32_at, u32_list, u32s};
use crate::container::Container;
use crate::objects::{self, Obj};
use crate::text::read_string;
use crate::{Budget, InddError, Result};

// Classes
pub(crate) const C_SPREAD_LAYER_PAGE: u32 = 0x50F;
pub(crate) const C_LAYER: u32 = 0x302;
pub(crate) const C_SPLINE: u32 = 0x6201;
pub(crate) const C_GROUP: u32 = 0x401;
pub(crate) const C_MCOL: u32 = 0x263;
pub(crate) const C_FRAME: u32 = 0x227;
pub(crate) const C_STORY: u32 = 0x201;
pub(crate) const C_STYLE: u32 = 0x205;
pub(crate) const C_FONT: u32 = 0x3E03;
pub(crate) const C_SWATCH: u32 = 0x1F05;
pub(crate) const C_BLOB: u32 = 0x129;
const GRAPHIC_CLASSES: [u32; 2] = [0x2501, 0x6601];
const STRAND_TEXT: u32 = 0x234;
const STRAND_CHARS: u32 = 0x235;
const STRAND_PARAS: u32 = 0x236;
const STRAND_FRAMES: u32 = 0x228;

// Implementations
const I_HIER: u32 = 0x15B;
const I_XFORM: u32 = 0x151;
const I_PATH: u32 = 0x162B;
const I_GATTR: u32 = 0x6E03;
const I_VISIBLE: u32 = 0x2C32;

// Graphic attributes
const GA_STROKE_COLOR: u32 = 0x6E64;
const GA_STROKE_WEIGHT: u32 = 0x6E65;
const GA_STROKE_TINT: u32 = 0x6E66;
const GA_FILL_COLOR: u32 = 0x6E68;
const GA_FILL_TINT: u32 = 0x6E69;

// Text attributes
pub(crate) const TA_COLOR: u32 = 0x1B01;
pub(crate) const TA_STYLE: u32 = 0x1B02;
pub(crate) const TA_SIZE: u32 = 0x1B03;
pub(crate) const TA_AUTOLEAD: u32 = 0x1B1A;
pub(crate) const TA_LEADING: u32 = 0x1B1B;
pub(crate) const TA_FONT: u32 = 0x1B2B;
pub(crate) const TA_JUSTIFY: u32 = 0x1B7E;

const MAX_DEPTH: usize = 32;
/// Upper bound for page items in one document. Items may be named as children more than once,
/// so without it a few objects could expand into billions of items.
pub(crate) const MAX_ITEMS: usize = 100_000;
/// Upper bound for spread layers and spread-layer entries visited (spreads may share layers and layers may
/// repeat, so the visits are not bounded by the file size).
const MAX_VISITS: usize = 1_000_000;
/// The model may hold this many times the file size (objects, embedded files, geometry).
const MODEL_FACTOR: usize = 8;
const MODEL_FLOOR: usize = 1 << 20;

#[derive(Debug, Clone)]
pub struct Swatch {
    pub name: String,
    pub space: &'static str,
    pub values: Vec<f64>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct PathPoint {
    pub left: (f64, f64),
    pub anchor: (f64, f64),
    pub right: (f64, f64),
}

#[derive(Debug, Clone)]
pub struct Subpath {
    pub points: Vec<PathPoint>,
    pub open: bool,
}

#[derive(Debug, Clone)]
pub struct Graphic {
    pub uid: u32,
    pub transform: [f64; 6],
    pub bounds: Option<[f64; 4]>,
    /// Embedded file; shared by every item that places the same file.
    pub data: Option<Arc<[u8]>>,
    pub uri: Option<String>,
    pub proxy: Option<Arc<[u8]>>,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Kind {
    Shape,
    Text,
    Group,
    Graphic,
}

#[derive(Debug, Clone)]
pub struct Item {
    pub uid: u32,
    pub kind: Kind,
    pub transform: [f64; 6],
    pub paths: Vec<Subpath>,
    pub fill: Option<u32>,
    pub fill_tint: f64,
    pub stroke: Option<u32>,
    pub stroke_weight: f64,
    pub stroke_tint: f64,
    pub story: Option<u32>,
    pub children: Vec<Item>,
    pub graphic: Option<Graphic>,
    pub layer: Option<u32>,
    pub hidden: bool,
}

#[derive(Debug, Clone)]
pub struct Page {
    pub uid: u32,
    pub transform: [f64; 6],
    pub bounds: [f64; 4],
}

#[derive(Debug, Clone)]
pub struct Spread {
    pub uid: u32,
    pub pages: Vec<Page>,
    pub items: Vec<Item>,
}

#[derive(Debug, Clone)]
pub struct Run {
    pub length: usize,
    pub style: u32,
    pub attrs: Attrs,
}

#[derive(Debug, Clone)]
pub struct Story {
    pub uid: u32,
    pub text: String,
    pub paras: Vec<Run>,
    pub chars: Vec<Run>,
}

#[derive(Debug, Clone)]
pub struct Style {
    pub uid: u32,
    pub name: String,
    pub based_on: Option<u32>,
    pub attrs: Attrs,
}

#[derive(Debug, Clone)]
pub struct Document {
    pub major: u32,
    pub minor: u32,
    pub swatches: BTreeMap<u32, Swatch>,
    pub fonts: BTreeMap<u32, String>,
    pub styles: BTreeMap<u32, Style>,
    pub stories: BTreeMap<u32, Story>,
    pub spreads: Vec<Spread>,
    /// Bottom to top.
    pub layers: Vec<(u32, String)>,
    pub page_size: (f64, f64),
    pub name: String,
}

pub struct Builder<'a> {
    c: &'a Container<'a>,
    objs: BTreeMap<u32, Obj>,
    default_stroke: f64,
    budget: Budget,
}

/// State of the page-item walk: cycle detection, the item count and embedded files read so far.
struct Walk<'f> {
    f2s: &'f HashMap<u32, u32>,
    /// UIDs from the spread layer down to the current item.
    path: Vec<u32>,
    items: usize,
    visits: usize,
    blobs: HashMap<u32, Option<Arc<[u8]>>>,
    budget: Budget,
}

impl Walk<'_> {
    /// Counts one spread layer or spread layer entry.
    fn visit(&mut self) -> Result<()> {
        self.visits = self.visits.saturating_add(1);
        if self.visits > MAX_VISITS {
            return Err(InddError::TooLarge("too many spread layer entries"));
        }
        Ok(())
    }

    /// An embedded file, read once per UID and charged to the budget once.
    fn blob(&mut self, c: &Container<'_>, uid: u32) -> Result<Option<Arc<[u8]>>> {
        if let Some(b) = self.blobs.get(&uid) {
            return Ok(b.clone());
        }
        let data = match c.object_capped(uid, self.budget.left()) {
            Ok(d) => {
                self.budget.take(d.len(), "embedded files")?;
                Some(Arc::from(d))
            }
            Err(e @ InddError::TooLarge(_)) => return Err(e),
            Err(_) => None,
        };
        self.blobs.insert(uid, data.clone());
        Ok(data)
    }
}

impl<'a> Builder<'a> {
    /// Reads every object of the container, within a memory budget derived from the file size.
    pub fn new(c: &'a Container<'a>) -> Result<Self> {
        let mut budget = Budget::for_input(c.file_len(), MODEL_FACTOR, MODEL_FLOOR);
        let objs = objects::load(c, &mut budget)?;
        Ok(Builder { c, objs, default_stroke: 1.0, budget })
    }

    fn cls(&self, uid: u32) -> Option<u32> {
        self.objs.get(&uid).map(|o| o.cls)
    }

    fn block(&self, uid: u32, implementation: u32) -> Option<&[u8]> {
        self.objs.get(&uid)?.block(implementation)
    }

    // --- resources -----------------------------------------------------------------------------
    fn swatches(&self) -> BTreeMap<u32, Swatch> {
        let mut out = BTreeMap::new();
        for o in self.objs.values().filter(|o| o.cls == C_SWATCH) {
            let name = o.block(0x1F10).filter(|nm| nm.len() > 7).and_then(|nm| read_string(nm, 3)).map(|(s, _)| s).unwrap_or_default();
            let (mut space, mut values) = ("CMYK", Vec::new());
            if let Some(cv) = o.block(0x1F01)
                && let (Some(sp), Some(n)) = (u32_at(cv, 0), u16_at(cv, 4))
            {
                let n = n as usize;
                if 6 + 8 * n <= cv.len() {
                    values = (0..n).filter_map(|i| f64_at(cv, 6 + 8 * i)).collect();
                }
                space = match sp {
                    5 => "RGB",
                    7 => "LAB",
                    _ => "CMYK",
                };
            }
            out.insert(o.uid, Swatch { name, space, values });
        }
        out
    }

    fn fonts(&self) -> BTreeMap<u32, String> {
        self.objs.values().filter(|o| o.cls == C_FONT).filter_map(|o| Some((o.uid, read_string(o.block(0x3E05)?, 5)?.0))).collect()
    }

    fn styles(&self) -> BTreeMap<u32, Style> {
        let mut out = BTreeMap::new();
        for o in self.objs.values().filter(|o| o.cls == C_STYLE) {
            let nb = o.block(0x230);
            let mut name = String::new();
            if let Some(nb) = nb
                && let Some(i) = nb.get(24..).and_then(|t| t.iter().position(|&b| b == 0x40)).map(|p| p + 24)
                && let Some(len) = u16_at(nb, i - 1)
            {
                name = crate::text::cp1252(crate::bytes::clamp(nb, i + 1, (len & 0x3FFF) as usize));
            }
            let based = nb.and_then(|nb| u32_at(nb, 4)).filter(|&b| b != 0);
            let attrs = o.block(0x23F).map(attrs::text_attrs).unwrap_or_default();
            out.insert(o.uid, Style { uid: o.uid, name, based_on: based, attrs });
        }
        out
    }

    fn layer_names(&self) -> BTreeMap<u32, String> {
        let mut out = BTreeMap::new();
        for o in self.objs.values().filter(|o| o.cls == C_LAYER) {
            let Some(b) = o.block(0x304) else { continue };
            if let Some(i) = b.iter().position(|&c| c == 0x40)
                && i >= 2
                && let Some(&len) = b.get(i - 1)
            {
                out.insert(o.uid, crate::text::cp1252(crate::bytes::clamp(b, i + 1, len as usize)));
            }
        }
        out
    }

    fn default_graphic_attrs(&self) -> Attrs {
        let mut best = Attrs::new();
        for o in self.objs.values() {
            if o.cls == C_SPLINE || o.cls == C_GROUP {
                continue;
            }
            if let Some(b) = o.block(I_GATTR).filter(|b| b.len() > 4) {
                let a = attrs::graphic_attrs(b);
                if a.len() > best.len() {
                    best = a;
                }
            }
        }
        best
    }

    // --- stories -------------------------------------------------------------------------------
    fn strands(&self, story: &Obj) -> Vec<u32> {
        let b = story.block(0x223).unwrap_or(&[]);
        let mut found = Vec::new();
        let mut i = 0;
        while i + 4 <= b.len() {
            if let Some(v) = u32_at(b, i)
                && matches!(self.cls(v), Some(STRAND_TEXT | STRAND_CHARS | STRAND_PARAS | STRAND_FRAMES))
                && !found.contains(&v)
            {
                found.push(v);
            }
            i += 2;
        }
        found
    }

    fn strand_data(&self, strand: u32) -> Option<&[u8]> {
        let ln = self.block(strand, 0x261).filter(|b| b.len() >= 10)?;
        self.block(u32_at(ln, 6)?, 0x262)
    }

    fn story(&self, o: &Obj) -> Story {
        let (mut text, mut paras, mut chars) = (String::new(), Vec::new(), Vec::new());
        for s in self.strands(o) {
            let Some(data) = self.strand_data(s) else { continue };
            match self.cls(s) {
                Some(STRAND_TEXT) => text = parse_text(data),
                Some(STRAND_PARAS) => paras = parse_runs(data),
                Some(STRAND_CHARS) => chars = parse_runs(data),
                _ => {}
            }
        }
        Story { uid: o.uid, text, paras, chars }
    }

    /// Frame column (0x227) → story.
    fn frame_story(&self) -> HashMap<u32, u32> {
        let mut strand_story = HashMap::new();
        for o in self.objs.values().filter(|o| o.cls == C_STORY && o.block(0x223).is_some()) {
            for s in self.strands(o) {
                strand_story.insert(s, o.uid);
            }
        }
        let mut out = HashMap::new();
        for o in self.objs.values().filter(|o| o.cls == C_FRAME) {
            if let Some(st) = o.block(0x220).and_then(|b| u32_at(b, 0)).and_then(|s| strand_story.get(&s)) {
                out.insert(o.uid, *st);
            }
        }
        out
    }

    // --- page items ----------------------------------------------------------------------------
    fn kids(&self, o: &Obj) -> Vec<u32> {
        let Some(h) = o.block(I_HIER).filter(|h| h.len() >= 12) else { return Vec::new() };
        u32_at(h, 8).and_then(|n| u32s(h, 12, n as usize)).unwrap_or_default()
    }

    fn graphic(&self, o: &Obj, w: &mut Walk<'_>) -> Result<Graphic> {
        let xf = transform(o.block(I_XFORM));
        let bounds = o.block(0x1633).filter(|b| b.len() >= 32).and_then(|b| Some([f64_at(b, 0)?, f64_at(b, 8)?, f64_at(b, 16)?, f64_at(b, 24)?]));
        let mut data = None;
        let mut uri = None;
        let link = o.block(0x8CBC).filter(|b| b.len() >= 12).and_then(|b| u32_at(b, 8));
        let res = link.and_then(|l| self.block(l, 0x8C9B)).filter(|b| b.len() >= 12).and_then(|b| u32_at(b, 8));
        if let Some(rb) = res.and_then(|r| self.block(r, 0x8C92)) {
            uri = read_uri(rb);
            if let Some(v) = (0..rb.len()).filter_map(|i| u32_at(rb, i)).find(|&v| self.cls(v) == Some(C_BLOB)) {
                data = w.blob(self.c, v)?;
            }
        }
        let proxy_uid = o
            .block(0x170D)
            .and_then(|b| u32_at(b, 0))
            .and_then(|p| self.block(p, 0x119))
            .and_then(|b| u32_at(b, 0))
            .filter(|&v| self.cls(v) == Some(C_BLOB));
        let proxy = match proxy_uid {
            Some(v) => w.blob(self.c, v)?,
            None => None,
        };
        Ok(Graphic { uid: o.uid, transform: xf, bounds, data, uri, proxy })
    }

    /// The page item `uid`, or None when it is not a page item, too deep, or one of its own
    /// ancestors (a cycle). Fails once the document holds more than [`MAX_ITEMS`] items.
    fn item(&self, uid: u32, w: &mut Walk<'_>, depth: usize) -> Result<Option<Item>> {
        if depth > MAX_DEPTH || w.path.contains(&uid) {
            return Ok(None);
        }
        let Some(o) = self.objs.get(&uid) else { return Ok(None) };
        if o.cls != C_GROUP && o.cls != C_SPLINE {
            return Ok(None);
        }
        w.items = w.items.saturating_add(1);
        if w.items > MAX_ITEMS {
            return Err(InddError::TooLarge("too many page items"));
        }
        w.budget.take(std::mem::size_of::<Item>(), "page items")?;
        w.path.push(uid);
        let it = self.item_body(o, w, depth);
        w.path.pop();
        it.map(Some)
    }

    fn item_body(&self, o: &Obj, w: &mut Walk<'_>, depth: usize) -> Result<Item> {
        let uid = o.uid;
        let hidden = o.block(I_VISIBLE).and_then(|b| b.first()).is_some_and(|&v| v == 0);
        let mut it = Item {
            uid,
            kind: Kind::Shape,
            transform: [1.0, 0.0, 0.0, 1.0, 0.0, 0.0],
            paths: Vec::new(),
            fill: None,
            fill_tint: -1.0,
            stroke: None,
            stroke_weight: 0.0,
            stroke_tint: -1.0,
            story: None,
            children: Vec::new(),
            graphic: None,
            layer: None,
            hidden,
        };
        if o.cls == C_GROUP {
            it.kind = Kind::Group;
            it.transform = transform(o.block(0x40D).or_else(|| o.block(I_XFORM)));
            for k in self.kids(o) {
                if let Some(ch) = self.item(k, w, depth + 1)? {
                    it.children.push(ch);
                }
            }
            return Ok(it);
        }
        it.transform = transform(o.block(I_XFORM));
        it.paths = o.block(I_PATH).map(parse_path).unwrap_or_default();
        let points: usize = it.paths.iter().map(|p| p.points.len()).sum();
        w.budget.take(points.saturating_mul(std::mem::size_of::<PathPoint>()), "path geometry")?;
        let ga = o.block(I_GATTR).map(attrs::graphic_attrs).unwrap_or_default();
        it.fill = attrs::uid(ga.get(&GA_FILL_COLOR));
        it.stroke = attrs::uid(ga.get(&GA_STROKE_COLOR));
        it.fill_tint = attrs::double(ga.get(&GA_FILL_TINT)).unwrap_or(-1.0);
        it.stroke_tint = attrs::double(ga.get(&GA_STROKE_TINT)).unwrap_or(-1.0);
        it.stroke_weight = attrs::double(ga.get(&GA_STROKE_WEIGHT)).unwrap_or(self.default_stroke);
        for k in self.kids(o) {
            let Some(ko) = self.objs.get(&k) else { continue };
            if ko.cls == C_MCOL {
                it.kind = Kind::Text;
                if let Some(st) = self.kids(ko).iter().find_map(|col| w.f2s.get(col)) {
                    it.story = Some(*st);
                }
            } else if GRAPHIC_CLASSES.contains(&ko.cls) {
                it.kind = Kind::Graphic;
                it.graphic = Some(self.graphic(ko, w)?);
            } else if let Some(ch) = self.item(k, w, depth + 1)? {
                it.children.push(ch);
            }
        }
        Ok(it)
    }

    pub fn build(mut self) -> Result<Document> {
        let swatches = self.swatches();
        let defaults = self.default_graphic_attrs();
        self.default_stroke = attrs::double(defaults.get(&GA_STROKE_WEIGHT)).unwrap_or(1.0);
        let f2s = self.frame_story();
        let stories: BTreeMap<u32, Story> = self.objs.values().filter(|o| o.cls == C_STORY).map(|o| (o.uid, self.story(o))).collect();
        let names = self.layer_names();
        let doc = self.objs.get(&1).ok_or_else(|| InddError::Damaged("no document object".into()))?;
        let order = doc.block(0x301).map(|b| u32_list(b, 0)).unwrap_or_default();
        let mut placed = std::collections::HashSet::new();
        let mut layers: Vec<(u32, String)> = Vec::new();
        for u in order.iter().chain(names.keys()) {
            if let Some(n) = names.get(u)
                && placed.insert(*u)
            {
                layers.push((*u, n.clone()));
            }
        }
        let spread_ids = doc.block(0x501).map(|b| u32_list(b, 0)).unwrap_or_default();
        let mut walk = Walk { f2s: &f2s, path: Vec::new(), items: 0, visits: 0, blobs: HashMap::new(), budget: self.budget };
        let mut spreads = Vec::new();
        let mut page_size = (612.0, 792.0);
        let mut seen_spreads = std::collections::HashSet::new();
        for sid in spread_ids {
            // A spread listed twice is read once.
            if !seen_spreads.insert(sid) {
                continue;
            }
            let Some(sp) = self.objs.get(&sid) else { continue };
            let (mut pages, mut items) = (Vec::new(), Vec::new());
            let layer_ids = sp.block(0x503).and_then(|sl| u32s(sl, 12, u32_at(sl, 8)? as usize)).unwrap_or_default();
            for lid in layer_ids {
                walk.visit()?;
                let Some(lo) = self.objs.get(&lid) else { continue };
                let Some(kb) = lo.block(0x303) else { continue };
                let (doc_layer, guides) = lo.block(0x302).map(|r| (u32_at(r, 0).unwrap_or(0), u16_at(r, 4).unwrap_or(0))).unwrap_or((0, 0));
                if guides != 0 {
                    continue;
                }
                let kids = u32_at(kb, 8).and_then(|n| u32s(kb, 12, n as usize)).unwrap_or_default();
                for k in kids {
                    walk.visit()?;
                    let Some(ko) = self.objs.get(&k) else { continue };
                    if ko.cls == C_SPREAD_LAYER_PAGE {
                        let Some(pb) = ko.block(0x5DD).and_then(|b| Some([f64_at(b, 0)?, f64_at(b, 8)?, f64_at(b, 16)?, f64_at(b, 24)?])) else {
                            continue;
                        };
                        pages.push(Page { uid: k, transform: transform(ko.block(0x5CC)), bounds: pb });
                        page_size = (pb[2] - pb[0], pb[3] - pb[1]);
                    } else if let Some(mut it) = self.item(k, &mut walk, 0)? {
                        it.layer = Some(doc_layer);
                        items.push(it);
                    }
                }
            }
            spreads.push(Spread { uid: sid, pages, items });
        }
        Ok(Document {
            major: self.c.major,
            minor: self.c.minor,
            swatches,
            fonts: self.fonts(),
            styles: self.styles(),
            stories,
            spreads,
            layers,
            page_size,
            name: "document.indd".into(),
        })
    }
}

// --- decoders ----------------------------------------------------------------------------------
pub fn parse_path(b: &[u8]) -> Vec<Subpath> {
    let mut out = Vec::new();
    let Some(nsub) = u32_at(b, 0) else { return out };
    let mut off = 4usize;
    for _ in 0..nsub {
        let Some(n) = u32_at(b, off) else { break };
        off += 4;
        let mut pts = Vec::new();
        for _ in 0..n {
            let Some(kind) = u32_at(b, off) else { return out };
            off += 4;
            let d = |i: usize| f64_at(b, off + 8 * i);
            if kind == 2 {
                let (Some(x), Some(y)) = (d(0), d(1)) else { return out };
                off += 16;
                pts.push(PathPoint { left: (x, y), anchor: (x, y), right: (x, y) });
            } else {
                let (Some(lx), Some(ly), Some(ax), Some(ay), Some(rx), Some(ry)) = (d(0), d(1), d(2), d(3), d(4), d(5)) else { return out };
                off += 48;
                pts.push(PathPoint { left: (lx, ly), anchor: (ax, ay), right: (rx, ry) });
            }
        }
        let open = u16_at(b, off).unwrap_or(0) != 0;
        off += 2;
        out.push(Subpath { points: pts, open });
    }
    out
}

/// Text strand data: u32 kind, u32 owner, u16 chunks; chunk = u32 size, u32 nchars, u16 tag, chars.
pub fn parse_text(d: &[u8]) -> String {
    let mut off = 8usize;
    let Some(n) = u16_at(d, off) else { return String::new() };
    off += 2;
    let mut out = String::new();
    for _ in 0..n {
        let (Some(o4), Some(o8), Some(o10)) = (off.checked_add(4), off.checked_add(8), off.checked_add(10)) else { break };
        let (Some(size), Some(nchars), Some(tag)) = (u32_at(d, off), u32_at(d, o4), u16_at(d, o8)) else { break };
        if tag & 0x4000 != 0 {
            out.push_str(&crate::text::cp1252(crate::bytes::clamp(d, o10, (tag & 0x3FFF) as usize)));
        } else {
            out.push_str(&crate::text::utf16le(crate::bytes::clamp(d, o10, (nchars as usize).saturating_mul(2))));
        }
        off = off.saturating_add(4).saturating_add(size as usize);
    }
    out
}

/// Run strand data: u32 kind, u32 owner, u16 runs; run = u32 size, u32 length, u32 style, attrs.
pub fn parse_runs(d: &[u8]) -> Vec<Run> {
    let mut off = 8usize;
    let Some(n) = u16_at(d, off) else { return Vec::new() };
    off += 2;
    let mut runs = Vec::new();
    for _ in 0..n {
        let (Some(o4), Some(o8), Some(o12)) = (off.checked_add(4), off.checked_add(8), off.checked_add(12)) else { break };
        let (Some(size), Some(length), Some(style)) = (u32_at(d, off), u32_at(d, o4), u32_at(d, o8)) else { break };
        let body_end = o4.saturating_add(size as usize);
        let body = d.get(o12..body_end.min(d.len())).unwrap_or(&[]);
        runs.push(Run { length: length as usize, style, attrs: attrs::text_attrs(body) });
        off = body_end;
    }
    runs
}

fn read_uri(b: &[u8]) -> Option<String> {
    let i = b.windows(5).position(|w| w == b"file:")?;
    let rest = b.get(i..)?;
    let end = rest.iter().position(|&c| !(0x20..0x7F).contains(&c)).unwrap_or(rest.len());
    Some(String::from_utf8_lossy(rest.get(..end)?).into_owned())
}
