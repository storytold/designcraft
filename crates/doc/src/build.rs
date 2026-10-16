//! Creating documents (File → New → Document), presets, and page/spread structure edits
//! (insert/delete/move pages, repagination into spreads, parents).

use std::collections::BTreeMap;
use std::sync::Arc;

use designcraft_geom::{Affine, Unit, units};
use serde::{Deserialize, Serialize};

use crate::ids::{ItemId, LayerId, PageId, SpreadId};
use crate::item::Item;
use crate::page::{Columns, Guide, Margins, Orientation, Page, PageSide, ParentInfo, Section, Spread};
use crate::styles::Styles;
use crate::{DataMerge, DocError, DocSettings, Document, Intent, LAYER_COLORS, Layer, ParaFormat, Result, SpreadRef};

/// A New Document preset.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Preset {
    pub name: &'static str,
    pub intent: Intent,
    pub width: f64,
    pub height: f64,
    pub units: Unit,
}

const MM: f64 = units::PT_PER_MM;

pub const PRESETS: &[Preset] = &[
    Preset { name: "Letter", intent: Intent::Print, width: 612.0, height: 792.0, units: Unit::Picas },
    Preset { name: "Legal", intent: Intent::Print, width: 612.0, height: 1008.0, units: Unit::Picas },
    Preset { name: "Tabloid", intent: Intent::Print, width: 792.0, height: 1224.0, units: Unit::Picas },
    Preset { name: "Letter - Half", intent: Intent::Print, width: 396.0, height: 612.0, units: Unit::Picas },
    Preset { name: "A3", intent: Intent::Print, width: 297.0 * MM, height: 420.0 * MM, units: Unit::Millimeters },
    Preset { name: "A4", intent: Intent::Print, width: 210.0 * MM, height: 297.0 * MM, units: Unit::Millimeters },
    Preset { name: "A5", intent: Intent::Print, width: 148.0 * MM, height: 210.0 * MM, units: Unit::Millimeters },
    Preset { name: "A6", intent: Intent::Print, width: 105.0 * MM, height: 148.0 * MM, units: Unit::Millimeters },
    Preset { name: "B5", intent: Intent::Print, width: 176.0 * MM, height: 250.0 * MM, units: Unit::Millimeters },
    Preset { name: "Business Card", intent: Intent::Print, width: 252.0, height: 144.0, units: Unit::Picas },
    Preset { name: "Postcard", intent: Intent::Print, width: 432.0, height: 288.0, units: Unit::Picas },
    Preset { name: "Web 1920 × 1080", intent: Intent::Web, width: 1920.0, height: 1080.0, units: Unit::Pixels },
    Preset { name: "Web 1366 × 768", intent: Intent::Web, width: 1366.0, height: 768.0, units: Unit::Pixels },
    Preset { name: "Web 1024 × 768", intent: Intent::Web, width: 1024.0, height: 768.0, units: Unit::Pixels },
    Preset { name: "Mobile 1080 × 1920", intent: Intent::Mobile, width: 1080.0, height: 1920.0, units: Unit::Pixels },
    Preset { name: "Mobile 390 × 844", intent: Intent::Mobile, width: 390.0, height: 844.0, units: Unit::Pixels },
    Preset { name: "Tablet 1024 × 1366", intent: Intent::Mobile, width: 1024.0, height: 1366.0, units: Unit::Pixels },
];

/// File → New → Document parameters.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct NewDocument {
    pub title: String,
    pub intent: Intent,
    pub width: f64,
    pub height: f64,
    pub pages: usize,
    pub start_page: u32,
    pub facing_pages: bool,
    pub primary_text_frame: bool,
    pub columns: u32,
    pub gutter: f64,
    pub margins: Margins,
    pub bleed: [f64; 4],
    pub slug: [f64; 4],
    pub units: Unit,
}

impl Default for NewDocument {
    fn default() -> Self {
        NewDocument {
            title: "Untitled-1".into(),
            intent: Intent::Print,
            width: 612.0,
            height: 792.0,
            pages: 1,
            start_page: 1,
            facing_pages: true,
            primary_text_frame: false,
            columns: 1,
            gutter: 12.0,
            margins: Margins::uniform(36.0),
            bleed: [0.0; 4],
            slug: [0.0; 4],
            units: Unit::Picas,
        }
    }
}

impl NewDocument {
    pub fn from_preset(name: &str) -> Option<Self> {
        let p = PRESETS.iter().find(|p| p.name.eq_ignore_ascii_case(name))?;
        let web = p.intent != Intent::Print;
        Some(NewDocument {
            intent: p.intent,
            width: p.width,
            height: p.height,
            units: p.units,
            facing_pages: !web,
            margins: if p.units == Unit::Millimeters { Margins::uniform(12.7 * MM) } else { Margins::uniform(36.0) },
            ..Default::default()
        })
    }
}

impl Document {
    /// Build a new document.
    pub fn new(nd: &NewDocument) -> Document {
        let settings = DocSettings {
            intent: nd.intent,
            page_width: nd.width,
            page_height: nd.height,
            facing_pages: nd.facing_pages,
            primary_text_frame: nd.primary_text_frame,
            bleed: nd.bleed,
            slug: nd.slug,
            horizontal_units: nd.units,
            vertical_units: nd.units,
            blend_space: if nd.intent == Intent::Print { crate::BlendSpace::Cmyk } else { crate::BlendSpace::Rgb },
            glyph_fallback: false,
            ..Default::default()
        };
        let mut d = Document {
            title: nd.title.clone(),
            settings,
            spreads: vec![],
            parents: vec![],
            layers: vec![],
            stories: BTreeMap::new(),
            styles: Arc::new(Styles::default()),
            swatches: designcraft_color::default_swatches(),
            color_groups: vec![],
            conditions: vec![],
            inks: Default::default(),
            stroke_styles: vec![],
            articles: vec![],
            xml: Default::default(),
            endnote_options: Default::default(),
            endnote_story: None,
            sections: vec![Section {
                start: 0,
                start_number: Some(nd.start_page.max(1)),
                style: Default::default(),
                prefix: String::new(),
                marker: String::new(),
                include_prefix: false,
            }],
            assets: BTreeMap::new(),
            hyperlinks: vec![],
            data_merge: DataMerge::default(),
            bookmarks: vec![],
            user_words: vec![],
            hyphenation_exceptions: vec![],
            toc: None,
            text_variables: crate::vars::defaults(),
            footnote_options: Default::default(),
            xref_formats: crate::xref::default_formats(),
            index: None,
            created: crate::vars::now(),
            modified: 0,
            next_id: 0,
            font_scope: 0,
        };
        let lid = LayerId(d.alloc());
        d.layers.push(Layer {
            id: lid,
            name: "Layer 1".into(),
            color: LAYER_COLORS[0].1,
            visible: true,
            locked: false,
            printable: true,
            show_guides: true,
            suppress_wrap_when_hidden: false,
        });
        // A-Parent.
        let parent_id = d.add_parent("A", "Parent", nd.columns, nd.gutter, nd.margins);
        // Document pages.
        let pages: Vec<Page> = (0..nd.pages.max(1)).map(|_| d.make_page(Some(parent_id), nd.columns, nd.gutter, nd.margins)).collect();
        d.spreads = vec![Arc::new(Spread { id: SpreadId(d.alloc()), pages, items: vec![], parent: None, allow_shuffle: true })];
        d.repaginate();
        if nd.primary_text_frame {
            d.add_primary_frames(nd.columns, nd.gutter);
        }
        d
    }

    /// Primary Text Frame: a margin-sized frame on every page, threaded into one story.
    fn add_primary_frames(&mut self, columns: u32, gutter: f64) {
        let layer = self.default_layer();
        let mut prev: Option<ItemId> = None;
        let mut story = None;
        for page in 0..self.page_count() {
            let Some((si, pi)) = self.page_loc(page) else { continue };
            let r = self.spreads[si].pages[pi].margin_rect();
            let Ok((fid, sid)) = self.add_text_frame(SpreadRef::Doc(si), r, layer, "", ParaFormat::default()) else { continue };
            if let Some(it) = self.item_mut(fid)
                && let Some(tf) = it.text_frame_mut()
            {
                tf.options.columns = columns.max(1);
                tf.options.gutter = gutter;
            }
            match prev {
                Some(p) => {
                    let _ = self.thread(p, fid);
                }
                None => story = Some(sid),
            }
            prev = Some(fid);
        }
        self.settings.primary_story = story;
    }

    fn make_page(&mut self, parent: Option<SpreadId>, columns: u32, gutter: f64, margins: Margins) -> Page {
        Page {
            id: PageId(self.alloc()),
            width: self.settings.page_width,
            height: self.settings.page_height,
            x: 0.0,
            margins,
            columns: Columns { count: columns.max(1), gutter, positions: None },
            parent,
            side: PageSide::Single,
            overridden: vec![],
            guides: vec![],
            show_parent_items: true,
            liquid: Default::default(),
            transition: None,
            view_rotation: 0,
            show_parent_overlay: false,
        }
    }

    /// Add a parent spread (`prefix`-`name`) and return its id.
    pub fn add_parent(&mut self, prefix: &str, name: &str, columns: u32, gutter: f64, margins: Margins) -> SpreadId {
        let n = if self.settings.facing_pages { 2 } else { 1 };
        let mut pages: Vec<Page> = (0..n).map(|_| self.make_page(None, columns, gutter, margins)).collect();
        if n == 2 {
            pages[0].side = PageSide::Left;
            pages[1].side = PageSide::Right;
        }
        let id = SpreadId(self.alloc());
        let mut sp = Spread {
            id,
            pages,
            items: vec![],
            parent: Some(ParentInfo { prefix: prefix.into(), name: name.into(), based_on: None }),
            allow_shuffle: true,
        };
        sp.relayout();
        self.parents.push(Arc::new(sp));
        id
    }

    pub fn parent_index(&self, id: SpreadId) -> Option<usize> {
        self.parents.iter().position(|p| p.id == id)
    }

    /// The next free parent prefix (A, B, C…).
    pub fn next_parent_prefix(&self) -> String {
        for c in 'A'..='Z' {
            let s = c.to_string();
            if !self.parents.iter().any(|p| p.parent.as_ref().is_some_and(|i| i.prefix == s)) {
                return s;
            }
        }
        format!("P{}", self.parents.len() + 1)
    }

    /// Redistribute document pages into spreads (facing: page 1 alone on the right, then pairs).
    /// Items move with the page their centre is on. Spreads with `allow_shuffle == false` are kept.
    /// Facing pages take their side from their page number, as InDesign shuffles them: an odd
    /// number is a right page (a left page bound right to left), so a section that starts at 1 on
    /// a left page moves it to the right and the pages after it follow.
    pub fn repaginate(&mut self) {
        let odd: Vec<bool> = (0..self.page_count()).map(|i| self.page_number(i) % 2 == 1).collect();
        // Collect pages with their items (items in page-relative coordinates) and their numbers' parity.
        let mut pages: Vec<(Page, Vec<Arc<Item>>)> = Vec::new();
        let mut parity: Vec<bool> = Vec::new();
        let mut kept: Vec<(usize, Arc<Spread>)> = Vec::new();
        let mut abs = 0;
        for sp in self.spreads.drain(..) {
            if !sp.allow_shuffle && sp.pages.len() > 1 {
                abs += sp.pages.len();
                kept.push((pages.len(), sp));
                continue;
            }
            for _ in &sp.pages {
                parity.push(odd.get(abs).copied().unwrap_or(abs % 2 == 0));
                abs += 1;
            }
            let mut sp = Arc::unwrap_or_clone(sp);
            let mut per: Vec<Vec<Arc<Item>>> = vec![Vec::new(); sp.pages.len()];
            for it in std::mem::take(&mut sp.items) {
                let cx = it.bounds().center().x;
                let pi = sp.page_at_x(cx).unwrap_or(0);
                let dx = sp.pages[pi].x;
                let mut it = Arc::unwrap_or_clone(it);
                it.xf = Affine::translate((-dx, 0.0)) * it.xf;
                per[pi].push(Arc::new(it));
            }
            for (p, items) in sp.pages.into_iter().zip(per) {
                pages.push((p, items));
            }
        }
        let facing = self.settings.facing_pages;
        let rtl = facing && self.settings.right_to_left_binding;
        let mut spreads: Vec<Spread> = Vec::new();
        let mut cur: Vec<(Page, Vec<Arc<Item>>)> = Vec::new();
        let total = pages.len();
        let flush = |cur: &mut Vec<(Page, Vec<Arc<Item>>)>, spreads: &mut Vec<Spread>, id: SpreadId| {
            if cur.is_empty() {
                return;
            }
            let mut sp = Spread { id, pages: vec![], items: vec![], parent: None, allow_shuffle: true };
            let n = cur.len();
            let mut all_items = Vec::new();
            for (i, (mut p, items)) in cur.drain(..).enumerate() {
                p.side = if !facing {
                    PageSide::Single
                } else if n == 1 {
                    // Lone page: right if odd page number position, decided by caller via side preset.
                    p.side
                } else if (i == 0) != rtl {
                    PageSide::Left
                } else {
                    PageSide::Right
                };
                all_items.push(items);
                sp.pages.push(p);
            }
            sp.relayout();
            for (p, items) in sp.pages.iter().zip(all_items) {
                for it in items {
                    let mut it = Arc::unwrap_or_clone(it);
                    it.xf = Affine::translate((p.x, 0.0)) * it.xf;
                    sp.items.push(Arc::new(it));
                }
            }
            spreads.push(sp);
        };
        for (i, (mut p, items)) in pages.into_iter().enumerate() {
            if facing {
                // A page is a right page when its number is odd (1, 3, 5 …), counting each
                // section from its start number (Document Setup › Start Page # included).
                // Bound right to left, odd pages are left pages and spreads start on the right.
                let odd = parity.get(i).copied().unwrap_or(i % 2 == 0);
                p.side = if odd != rtl { PageSide::Right } else { PageSide::Left };
                let starts = if rtl { PageSide::Right } else { PageSide::Left };
                if p.side == starts && !cur.is_empty() {
                    let id = SpreadId(self.alloc());
                    flush(&mut cur, &mut spreads, id);
                }
                cur.push((p, items));
                if cur.len() == 2 || i + 1 == total {
                    let id = SpreadId(self.alloc());
                    flush(&mut cur, &mut spreads, id);
                }
            } else {
                cur.push((p, items));
                let id = SpreadId(self.alloc());
                flush(&mut cur, &mut spreads, id);
            }
        }
        let mut out: Vec<Arc<Spread>> = spreads.into_iter().map(Arc::new).collect();
        // Re-insert kept (non-shuffling) spreads at roughly their old page positions.
        for (page_pos, sp) in kept.into_iter().rev() {
            let mut n = 0;
            let mut at = out.len();
            for (i, s) in out.iter().enumerate() {
                if n >= page_pos {
                    at = i;
                    break;
                }
                n += s.pages.len();
            }
            out.insert(at, sp);
        }
        self.spreads = out;
    }

    /// Insert `count` pages after absolute page `after` (None = at the start), using `parent`.
    pub fn insert_pages(&mut self, after: Option<usize>, count: usize, parent: Option<SpreadId>) -> Result<Vec<PageId>> {
        if count == 0 {
            return Ok(vec![]);
        }
        let n = self.page_count();
        let at = after.map_or(0, |a| a.saturating_add(1).min(n));
        let template = self.page(at.saturating_sub(1).min(n.saturating_sub(1))).cloned();
        let (cols, gutter, margins) =
            template.as_ref().map(|t| (t.columns.count, t.columns.gutter, t.margins)).unwrap_or((1, 12.0, Margins::uniform(36.0)));
        let mut ids = Vec::new();
        let new_pages: Vec<Page> = (0..count)
            .map(|_| {
                let mut p = self.make_page(parent, cols, gutter, margins);
                if let Some(t) = &template {
                    p.width = t.width;
                    p.height = t.height;
                }
                ids.push(p.id);
                p
            })
            .collect();
        // Splice into the spread holding page `at` (or append a new spread), then repaginate. At
        // the start of a spread that doesn't shuffle, the pages go in before it instead.
        let loc = match self.page_loc(at) {
            Some((si, 0)) if !self.spreads[si].allow_shuffle => {
                let id = SpreadId(self.alloc());
                let mut sp = Spread { id, pages: new_pages, items: vec![], parent: None, allow_shuffle: true };
                sp.relayout();
                self.spreads.insert(si, Arc::new(sp));
                self.repaginate();
                return Ok(ids);
            }
            l => l,
        };
        match loc {
            Some((si, pi)) => {
                let sp = Arc::make_mut(&mut self.spreads[si]);
                let x = sp.pages[pi].x;
                let w: f64 = new_pages.iter().map(|p| p.width).sum();
                // Shift items right of the insertion point.
                for it in &mut sp.items {
                    if it.bounds().center().x >= x {
                        let it = Arc::make_mut(it);
                        it.xf = Affine::translate((w, 0.0)) * it.xf;
                    }
                }
                for (k, p) in new_pages.into_iter().enumerate() {
                    sp.pages.insert(pi + k, p);
                }
                sp.relayout();
            }
            None => {
                let id = SpreadId(self.alloc());
                let mut sp = Spread { id, pages: new_pages, items: vec![], parent: None, allow_shuffle: true };
                sp.relayout();
                self.spreads.push(Arc::new(sp));
            }
        }
        self.repaginate();
        Ok(ids)
    }

    /// Delete absolute pages (items whose centre is on a deleted page are deleted too).
    pub fn delete_pages(&mut self, pages: &[usize]) -> Result<()> {
        if pages.len() >= self.page_count() {
            return Err(DocError::Invalid("a document needs at least one page".into()));
        }
        let mut doomed: Vec<(usize, usize)> = pages.iter().filter_map(|p| self.page_loc(*p)).collect();
        doomed.sort();
        doomed.dedup();
        let mut remove_items: Vec<ItemId> = Vec::new();
        for &(si, pi) in &doomed {
            let sp = &self.spreads[si];
            for it in &sp.items {
                if sp.page_at_x(it.bounds().center().x) == Some(pi) {
                    remove_items.push(it.id);
                }
            }
        }
        for id in remove_items {
            let _ = self.remove_item(id);
        }
        for &(si, pi) in doomed.iter().rev() {
            let sp = Arc::make_mut(&mut self.spreads[si]);
            let w = sp.pages[pi].width;
            let x = sp.pages[pi].x;
            sp.pages.remove(pi);
            for it in &mut sp.items {
                if it.bounds().center().x >= x + w {
                    let it = Arc::make_mut(it);
                    it.xf = Affine::translate((-w, 0.0)) * it.xf;
                }
            }
            sp.relayout();
        }
        self.spreads.retain(|s| !s.pages.is_empty());
        self.repaginate();
        Ok(())
    }

    /// Move absolute page `from` so it ends up at index `to` (items travel with it).
    pub fn move_page(&mut self, from: usize, to: usize) -> Result<()> {
        let n = self.page_count();
        if from >= n || to >= n {
            return Err(DocError::NoPage(from.max(to)));
        }
        if from == to {
            return Ok(());
        }
        // Flatten to single-page spreads, reorder, repaginate.
        let facing = self.settings.facing_pages;
        self.settings.facing_pages = false;
        self.repaginate();
        let sp = self.spreads.remove(from);
        self.spreads.insert(to, sp);
        self.settings.facing_pages = facing;
        self.repaginate();
        Ok(())
    }

    /// Move absolute page `abs` (with the objects on it) to the end of spread `to`, which stops
    /// shuffling (Layout › Pages: spreads of up to 10 pages).
    pub fn page_to_spread(&mut self, abs: usize, to: usize) -> Result<()> {
        let (si, pi) = self.page_loc(abs).ok_or(DocError::NoPage(abs))?;
        if to >= self.spreads.len() {
            return Err(DocError::Invalid(format!("no spread {to}")));
        }
        if si == to {
            return Ok(());
        }
        if self.spreads[to].pages.len() >= 10 {
            return Err(DocError::Invalid("a spread holds at most 10 pages".into()));
        }
        // Take the page and its objects out of their spread.
        let src = Arc::make_mut(&mut self.spreads[si]);
        let page = src.pages.remove(pi);
        let pb = page.bounds();
        let (moving, staying): (Vec<_>, Vec<_>) = std::mem::take(&mut src.items).into_iter().partition(|it| {
            let c = it.bounds().center();
            c.x >= pb.x0 && c.x < pb.x1
        });
        src.items = staying;
        src.relayout();
        // Append to the target; objects keep their place on the page.
        let dst = Arc::make_mut(&mut self.spreads[to]);
        let x = dst.pages.last().map_or(0.0, |p| p.x + p.width);
        let dx = x - page.x;
        dst.pages.push(page);
        dst.relayout();
        dst.allow_shuffle = false;
        for it in moving {
            let mut it = Arc::unwrap_or_clone(it);
            it.xf = Affine::translate((dx, 0.0)) * it.xf;
            dst.items.push(Arc::new(it));
        }
        if self.spreads[si].pages.is_empty() {
            self.spreads.remove(si);
        }
        Ok(())
    }

    /// Apply a parent (None = [None]) to absolute pages.
    pub fn apply_parent(&mut self, pages: &[usize], parent: Option<SpreadId>) -> Result<()> {
        if let Some(p) = parent
            && self.parent_index(p).is_none()
        {
            return Err(DocError::Invalid(format!("no parent {p}")));
        }
        for &abs in pages {
            let (si, pi) = self.page_loc(abs).ok_or(DocError::NoPage(abs))?;
            Arc::make_mut(&mut self.spreads[si]).pages[pi].parent = parent;
        }
        Ok(())
    }

    /// Parent page (spread index, page index) to show behind document page `abs`.
    pub fn parent_page_for(&self, abs: usize) -> Option<(usize, usize)> {
        let page = self.page(abs)?;
        let pi = self.parent_index(page.parent?)?;
        Some((pi, shown_parent_page(&self.parents[pi], page.side)?))
    }

    /// Ruler guides document page `abs` inherits from its applied parent page. They are stored on
    /// the parent page only: positions are moved from the parent page's place in its spread to
    /// this page's place in its own, so they draw and snap where the page is. Editing them
    /// happens on the parent page.
    pub fn inherited_guides(&self, abs: usize) -> Vec<Guide> {
        let (Some(page), Some((pi, qi))) = (self.page(abs), self.parent_page_for(abs)) else { return Vec::new() };
        let Some(parent) = self.parents.get(pi).and_then(|ps| ps.pages.get(qi)) else { return Vec::new() };
        let dx = page.x - parent.x;
        parent
            .guides
            .iter()
            .map(|g| {
                let mut g = g.clone();
                if g.orientation == Orientation::Vertical {
                    g.position += dx;
                }
                g
            })
            .collect()
    }

    /// Every parent page behind document page `abs` (spread index, page index): the applied parent
    /// and each parent it is based on, deepest first. At each level the document page's side picks
    /// the page, as in [`Self::parent_page_for`]. The chain ends at a parent page that hides its own
    /// parent's items, or at a cycle.
    pub fn parent_chain_for(&self, abs: usize) -> Vec<(usize, usize)> {
        let mut chain: Vec<(usize, usize)> = Vec::new();
        let Some(page) = self.page(abs) else { return chain };
        let mut next = page.parent;
        while let Some(id) = next {
            let Some(pi) = self.parent_index(id) else { break };
            if chain.iter().any(|&(seen, _)| seen == pi) {
                break;
            }
            let ps = &self.parents[pi];
            let Some(idx) = shown_parent_page(ps, page.side) else { break };
            chain.push((pi, idx));
            if !ps.pages[idx].show_parent_items {
                break;
            }
            next = ps.parent.as_ref().and_then(|info| info.based_on);
        }
        chain.reverse();
        chain
    }

    /// The parent items showing on document page `abs`: deepest parent first (bottom of the
    /// stack), z-order within a level. Items overridden by the document page, or by a parent page
    /// higher in the chain, are left out. An item belongs to every parent page it overlaps; one that
    /// crosses the spine shows on each document page applying such a page, cut at the spine.
    pub fn shown_parent_items(&self, abs: usize) -> Vec<ShownParentItem<'_>> {
        let Some(page) = self.page(abs) else { return Vec::new() };
        if !page.show_parent_items {
            return Vec::new();
        }
        let chain = self.parent_chain_for(abs);
        let mut items = Vec::new();
        for (level, &(pi, idx)) in chain.iter().enumerate() {
            let parent = &self.parents[pi];
            let dx = page.x - parent.pages[idx].x;
            let above = chain.get(level + 1..).unwrap_or_default();
            for it in &parent.items {
                let clip = if parent.pages.len() > 1 {
                    let b = it.bounds();
                    let touched: Vec<usize> =
                        parent.pages.iter().enumerate().filter(|(_, p)| b.x0 < p.x + p.width && b.x1 > p.x).map(|(j, _)| j).collect();
                    match touched.as_slice() {
                        // On the pasteboard: the nearest page's.
                        [] if parent.page_at_x(b.center().x) == Some(idx) => None,
                        [only] if *only == idx => None,
                        many if many.contains(&idx) => {
                            // Cut at the spine side(s), open towards the outside edge(s).
                            let x0 = if idx == 0 { page.x - 1.0e4 } else { page.x };
                            let x1 = if idx + 1 == parent.pages.len() { page.x + page.width + 1.0e4 } else { page.x + page.width };
                            Some((x0, x1))
                        }
                        _ => continue,
                    }
                } else {
                    None
                };
                let overridden =
                    page.overridden.contains(&it.id) || above.iter().any(|&(api, aidx)| self.parents[api].pages[aidx].overridden.contains(&it.id));
                if !overridden {
                    items.push(ShownParentItem { item: it, dx, clip_x: clip });
                }
            }
        }
        items
    }
}

/// A parent item showing on a document page (see [`Document::shown_parent_items`]).
#[derive(Clone, Copy, Debug)]
pub struct ShownParentItem<'a> {
    pub item: &'a Arc<Item>,
    /// x offset from the parent spread to the document spread.
    pub dx: f64,
    /// For an item crossing the spine: the document-spread x range it is cut to.
    pub clip_x: Option<(f64, f64)>,
}

/// The page of parent spread `ps` behind a document page on side `side`: the left page of a facing
/// parent, otherwise the last. A single-sided page takes the parent's first page, like a left-hand
/// page. A parent spread can come without pages from a file.
fn shown_parent_page(ps: &Spread, side: PageSide) -> Option<usize> {
    let first = ps.pages.len() >= 2 && matches!(side, PageSide::Left | PageSide::Single);
    if first { Some(0) } else { ps.pages.len().checked_sub(1) }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn new_facing_document_layout() {
        let d = Document::new(&NewDocument { pages: 4, ..Default::default() });
        assert_eq!(d.page_count(), 4);
        // [1] [2 3] [4]
        assert_eq!(d.spreads.iter().map(|s| s.pages.len()).collect::<Vec<_>>(), vec![1, 2, 1]);
        assert_eq!(d.spreads[0].pages[0].side, PageSide::Right);
        assert_eq!(d.spreads[1].pages[0].side, PageSide::Left);
        assert_eq!(d.spreads[1].pages[1].x, 612.0);
        assert_eq!(d.parents.len(), 1);
        assert_eq!(d.parents[0].pages.len(), 2);
        d.check().unwrap();
    }

    /// A document whose parent spread has no pages passes `check()`; showing the parent behind a
    /// page used to compute index `0 - 1`.
    #[test]
    fn parent_without_pages_is_not_shown() {
        let mut d = Document::new(&NewDocument { pages: 2, ..Default::default() });
        assert!(d.parent_page_for(0).is_some());
        Arc::make_mut(&mut d.parents[0]).pages.clear();
        d.check().unwrap();
        assert_eq!(d.parent_page_for(0), None);
        assert_eq!(d.parent_page_for(1), None);
    }

    /// A single-sided page applying a two-page parent shows the parent's first page.
    #[test]
    fn single_sided_page_shows_first_parent_page() {
        let mut d = Document::new(&NewDocument { pages: 2, facing_pages: false, ..Default::default() });
        let extra = d.parents[0].pages[0].clone();
        Arc::make_mut(&mut d.parents[0]).pages.push(extra);
        assert_eq!(d.parents[0].pages.len(), 2);
        assert_eq!(d.parent_page_for(0), Some((0, 0)));
        assert_eq!(d.parent_page_for(1), Some((0, 0)));
        Arc::make_mut(&mut d.parents[0]).pages.clear();
        assert_eq!(d.parent_page_for(0), None);
    }

    #[test]
    fn non_facing_one_page_per_spread() {
        let d = Document::new(&NewDocument { pages: 3, facing_pages: false, ..Default::default() });
        assert_eq!(d.spreads.len(), 3);
        assert!(d.spreads.iter().all(|s| s.pages.len() == 1 && s.pages[0].side == PageSide::Single));
    }

    #[test]
    fn insert_and_delete_pages_reshuffle() {
        let mut d = Document::new(&NewDocument { pages: 1, ..Default::default() });
        d.insert_pages(Some(0), 3, d.parents.first().map(|p| p.id)).unwrap();
        assert_eq!(d.page_count(), 4);
        assert_eq!(d.spreads.len(), 3);
        d.delete_pages(&[1, 2]).unwrap();
        assert_eq!(d.page_count(), 2);
        assert_eq!(d.spreads.iter().map(|s| s.pages.len()).collect::<Vec<_>>(), vec![1, 1]);
        assert!(d.delete_pages(&[0, 1]).is_err());
    }

    #[test]
    fn items_travel_with_pages() {
        let mut d = Document::new(&NewDocument { pages: 3, ..Default::default() });
        // Put a frame on page 3 (spread 1, page index 1).
        let lid = d.default_layer();
        let r = designcraft_geom::Rect::new(612.0 + 100.0, 100.0, 612.0 + 200.0, 200.0);
        let (id, _) = d.add_text_frame(crate::SpreadRef::Doc(1), r, lid, "hello", Default::default()).unwrap();
        d.move_page(2, 0).unwrap();
        // The frame's page is now page 1 (spread 0, single right page at x=0).
        let loc = d.find(id).unwrap();
        assert_eq!(loc.spread, crate::SpreadRef::Doc(0));
        let b = d.item(id).unwrap().bounds();
        assert!((b.x0 - 100.0).abs() < 1e-9, "{b:?}");
        d.check().unwrap();
    }

    /// Parents A ← B ← C (C based on B, B on A): a page applying C shows the items of all three,
    /// deepest at the bottom. An override on the page or higher in the chain hides an item; a cycle
    /// in `based_on` doesn't loop forever.
    #[test]
    fn nested_parents_show_their_whole_chain() {
        let mut d = Document::new(&NewDocument { pages: 3, ..Default::default() });
        let lid = d.default_layer();
        let a = d.parents[0].id;
        let b = d.add_parent("B", "Parent", 1, 0.0, Margins::uniform(36.0));
        let c = d.add_parent("C", "Parent", 1, 0.0, Margins::uniform(36.0));
        Arc::make_mut(&mut d.parents[1]).parent.as_mut().unwrap().based_on = Some(a);
        Arc::make_mut(&mut d.parents[2]).parent.as_mut().unwrap().based_on = Some(b);
        // One box on each parent's right page (x 612–1224).
        let mut ids = Vec::new();
        for pi in 0..3 {
            let id = ItemId(d.alloc());
            let r = designcraft_geom::Rect::new(700.0, 100.0 + pi as f64 * 50.0, 800.0, 140.0 + pi as f64 * 50.0);
            let it = Item::new(id, lid, crate::Shape::Rectangle, designcraft_geom::shapes::rectangle(r));
            d.insert_item(crate::SpreadRef::Parent(pi), it, None).unwrap();
            ids.push(id);
        }
        // Page 3 is a right page (spread [2 3]).
        d.apply_parent(&[2], Some(c)).unwrap();
        assert_eq!(d.parent_chain_for(2), vec![(0, 1), (1, 1), (2, 1)]);
        let shown: Vec<ItemId> = d.shown_parent_items(2).iter().map(|s| s.item.id).collect();
        assert_eq!(shown, ids);
        // B overrides A's box; the page overrides C's.
        Arc::make_mut(&mut d.parents[1]).pages[1].overridden.push(ids[0]);
        let (si, pi) = d.page_loc(2).unwrap();
        Arc::make_mut(&mut d.spreads[si]).pages[pi].overridden.push(ids[2]);
        let shown: Vec<ItemId> = d.shown_parent_items(2).iter().map(|s| s.item.id).collect();
        assert_eq!(shown, vec![ids[1]]);
        // A parent hiding its own parent's items ends the chain.
        Arc::make_mut(&mut d.parents[2]).pages[1].show_parent_items = false;
        assert_eq!(d.parent_chain_for(2), vec![(2, 1)]);
        // Cycle: A based on C.
        Arc::make_mut(&mut d.parents[2]).pages[1].show_parent_items = true;
        Arc::make_mut(&mut d.parents[0]).parent.as_mut().unwrap().based_on = Some(c);
        assert_eq!(d.parent_chain_for(2), vec![(0, 1), (1, 1), (2, 1)]);
    }

    /// A parent item crossing the spine shows on each document page applying that parent page,
    /// cut at the spine; a page applying another parent doesn't show it. An item wholly on one
    /// parent page stays on that page, uncut.
    #[test]
    fn parent_item_across_the_spine_shows_on_both_sides() {
        let mut d = Document::new(&NewDocument { pages: 3, ..Default::default() });
        let lid = d.default_layer();
        // Parent pages: left x 0–612, right x 612–1224. One item across the spine (centre right of
        // it), one on the right page only.
        let mut ids = Vec::new();
        for r in [designcraft_geom::Rect::new(100.0, 100.0, 1200.0, 700.0), designcraft_geom::Rect::new(700.0, 100.0, 800.0, 200.0)] {
            let id = ItemId(d.alloc());
            d.insert_item(crate::SpreadRef::Parent(0), Item::new(id, lid, crate::Shape::Rectangle, designcraft_geom::shapes::rectangle(r)), None)
                .unwrap();
            ids.push(id);
        }
        // Spread [2 3]: page index 1 is left, 2 is right; both apply A.
        let left = d.shown_parent_items(1);
        assert_eq!(left.iter().map(|s| s.item.id).collect::<Vec<_>>(), vec![ids[0]]);
        let (x0, x1) = left[0].clip_x.unwrap();
        assert!(x0 < 0.0 && (x1 - 612.0).abs() < 1e-9, "{x0} {x1}");
        let right = d.shown_parent_items(2);
        assert_eq!(right.iter().map(|s| s.item.id).collect::<Vec<_>>(), ids);
        assert!((right[0].clip_x.unwrap().0 - 612.0).abs() < 1e-9 && right[1].clip_x.is_none());
        // The right page applies another parent: the left page still shows its half.
        let b = d.add_parent("B", "Parent", 1, 0.0, Margins::uniform(36.0));
        d.apply_parent(&[2], Some(b)).unwrap();
        assert!(d.shown_parent_items(2).is_empty());
        assert_eq!(d.shown_parent_items(1).len(), 1);
    }

    #[test]
    fn presets_and_parents() {
        let nd = NewDocument::from_preset("A4").unwrap();
        assert!((nd.width - 595.2756).abs() < 1e-3);
        let mut d = Document::new(&nd);
        assert_eq!(d.next_parent_prefix(), "B");
        let b = d.add_parent("B", "Parent", 2, 12.0, Margins::uniform(36.0));
        d.apply_parent(&[0], Some(b)).unwrap();
        assert_eq!(d.parent_page_for(0), Some((1, 1)));
        d.apply_parent(&[0], None).unwrap();
        assert_eq!(d.parent_page_for(0), None);
    }
}
