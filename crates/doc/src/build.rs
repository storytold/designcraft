//! Creating documents (File → New → Document), presets, and page/spread structure edits
//! (insert/delete/move pages, repagination into spreads, parents).

use std::collections::BTreeMap;
use std::{collections::HashSet, sync::Arc};

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
    Preset { name: "A0", intent: Intent::Print, width: 841.0 * MM, height: 1189.0 * MM, units: Unit::Millimeters },
    Preset { name: "A1", intent: Intent::Print, width: 594.0 * MM, height: 841.0 * MM, units: Unit::Millimeters },
    Preset { name: "A2", intent: Intent::Print, width: 420.0 * MM, height: 594.0 * MM, units: Unit::Millimeters },
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
            file_info: Default::default(),
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
            local_margins: false,
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
            if self.settings.right_to_left_binding {
                pages.reverse();
            }
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

    /// Delete a parent spread and unapply it wherever it is used. Local overrides survive as
    /// ordinary items; the deleted parent's own items leave their text threads normally.
    pub fn delete_parent(&mut self, id: SpreadId) -> Result<()> {
        let index = self.parent_index(id).ok_or_else(|| DocError::Invalid(format!("no parent {id}")))?;
        let parent = self.parents.get(index).ok_or_else(|| DocError::Invalid(format!("no parent {id}")))?;
        let roots: Vec<_> = parent.items.iter().map(|item| item.id).collect();
        let mut deleted = HashSet::new();
        for item in &parent.items {
            item.walk(&mut |item| {
                deleted.insert(item.id);
            });
        }
        for item in roots {
            self.remove_item(item)?;
        }
        self.parents.remove(index);

        let mut detached = Vec::new();
        for spread in self.spreads.iter_mut().chain(self.parents.iter_mut()) {
            let spread = Arc::make_mut(spread);
            if let Some(parent) = &mut spread.parent
                && parent.based_on == Some(id)
            {
                parent.based_on = None;
            }
            for page in &mut spread.pages {
                if page.parent == Some(id) {
                    page.parent = None;
                }
                page.overridden.retain(|item| !deleted.contains(item));
            }
            for item in &spread.items {
                item.walk(&mut |item| {
                    if item.overrides.is_some_and(|source| deleted.contains(&source)) {
                        detached.push(item.id);
                    }
                });
            }
        }
        for item in detached {
            if let Some(item) = self.item_mut(item) {
                item.overrides = None;
            }
        }
        Ok(())
    }

    /// List the pages of each facing parent spread in binding order: left then right, or right
    /// then left when bound right to left. Positions stay, so the parent items keep their pages.
    pub fn order_parent_pages(&mut self) {
        let first = if self.settings.right_to_left_binding { PageSide::Right } else { PageSide::Left };
        for sp in &mut self.parents {
            let pair = matches!(sp.pages.as_slice(), [a, b] if a.side != b.side && a.side != PageSide::Single && b.side != PageSide::Single);
            if pair && sp.pages.first().is_some_and(|p| p.side != first) {
                Arc::make_mut(sp).pages.reverse();
            }
        }
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
        // Whether each shuffled page was a left page, to mirror its dragged columns if it moves.
        let mut was_left: BTreeMap<PageId, bool> = BTreeMap::new();
        let mut abs = 0;
        for sp in self.spreads.drain(..) {
            if !sp.allow_shuffle && sp.pages.len() > 1 {
                abs += sp.pages.len();
                kept.push((pages.len(), sp));
                continue;
            }
            for p in &sp.pages {
                parity.push(odd.get(abs).copied().unwrap_or(abs % 2 == 0));
                was_left.insert(p.id, p.side == PageSide::Left);
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
                // A right page closes its spread, so two right pages in a row (a section restart
                // at an even position) never share one.
                let after_ending = cur.last().is_some_and(|(q, _)| q.side != starts);
                if (p.side == starts || after_ending) && !cur.is_empty() {
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
        // A page that crosses the spine keeps its inside and outside margins, so its margin box
        // mirrors; its dragged column guides mirror with it, as in InDesign.
        let crossed = |p: &Page| was_left.get(&p.id).is_some_and(|&l| l != (p.side == PageSide::Left)) && p.columns.positions.is_some();
        for sp in &mut self.spreads {
            if !sp.pages.iter().any(crossed) {
                continue;
            }
            for p in &mut Arc::make_mut(sp).pages {
                if crossed(&*p) {
                    p.columns = p.columns.mirrored(p.margin_rect().width());
                }
            }
        }
        // Pages can change sides, and so parent pages.
        self.sync_parent_margins();
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
                    // Dragged column guides too: the same width and margins keep them valid.
                    // They are taken as for this single page; repaginating mirrors them if the
                    // page lands on a left page.
                    p.columns.positions = p.columns_from(t).positions;
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

    /// Whether absolute page `abs` holds page items: the ones Delete Pages removes with it
    /// (an item belongs to the page its centre is over).
    pub fn page_has_items(&self, abs: usize) -> bool {
        let Some((si, pi)) = self.page_loc(abs) else { return false };
        let Some(sp) = self.spreads.get(si) else { return false };
        sp.items.iter().any(|it| sp.page_at_x(it.bounds().center().x) == Some(pi))
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
        self.sync_parent_margins();
        Ok(())
    }

    /// Parent page (spread index, page index) to show behind document page `abs`.
    pub fn parent_page_for(&self, abs: usize) -> Option<(usize, usize)> {
        self.parent_page_of(self.page(abs)?)
    }

    /// Parent page (spread index, page index) applied to `page`: the page on its side of its
    /// parent spread, or in its place for single-sided pages ([`Spread::parent_page_at`]).
    pub fn parent_page_of(&self, page: &Page) -> Option<(usize, usize)> {
        let pi = self.parent_index(page.parent?)?;
        // A parent spread can come without pages from a file.
        let parent = self.parents.get(pi)?;
        // Only a single-sided page's position matters: look for it only then.
        let index = if page.side == PageSide::Single && parent.pages.len() >= 2 {
            self.spreads.iter().find_map(|s| s.pages.iter().position(|p| p.id == page.id)).unwrap_or(0)
        } else {
            0
        };
        Some((pi, parent.parent_page_at(page.side, index)?))
    }

    /// The parent page applied to `page`.
    pub fn parent_page(&self, page: &Page) -> Option<&Page> {
        let (pi, qi) = self.parent_page_of(page)?;
        self.parents.get(pi)?.pages.get(qi)
    }

    /// Ruler guides `page` shows from its parent page, moved from the parent page's place in its
    /// spread to `page`'s place in its own. They stay stored on the parent page.
    pub fn parent_guides<'a>(&'a self, page: &Page) -> impl Iterator<Item = Guide> + 'a {
        let src = self.parent_page(page);
        let dx = src.map_or(0.0, |q| page.x - q.x);
        src.into_iter().flat_map(|q| q.guides.iter()).map(move |g| {
            let mut g = g.clone();
            if g.orientation == Orientation::Vertical {
                g.position += dx;
            }
            g
        })
    }

    /// Give every document page without local margins its parent page's margins and columns.
    pub fn sync_parent_margins(&mut self) {
        for si in 0..self.spreads.len() {
            for pi in 0..self.spreads[si].pages.len() {
                let Some(page) = self.spreads[si].pages.get(pi) else { continue };
                if page.local_margins {
                    continue;
                }
                let Some(src) = self.parent_page(page).filter(|q| !q.same_margins(page)).map(|q| (q.margins, page.columns_from(q))) else {
                    continue;
                };
                if let Some(page) = Arc::make_mut(&mut self.spreads[si]).pages.get_mut(pi) {
                    (page.margins, page.columns) = src;
                }
            }
        }
    }

    /// Ruler guides document page `abs` inherits from its applied parent page (see
    /// [`Document::parent_guides`]).
    pub fn inherited_guides(&self, abs: usize) -> Vec<Guide> {
        self.page(abs).map(|p| self.parent_guides(p).collect()).unwrap_or_default()
    }

    /// Mark document pages whose margins or columns differ from their parent page's as local, so
    /// that a later sync keeps them (documents saved before pages tracked local margins).
    pub fn mark_differing_margins_local(&mut self) {
        for si in 0..self.spreads.len() {
            for pi in 0..self.spreads[si].pages.len() {
                let Some(page) = self.spreads[si].pages.get(pi) else { continue };
                if page.local_margins || self.parent_page(page).is_none_or(|q| q.same_margins(page)) {
                    continue;
                }
                if let Some(page) = Arc::make_mut(&mut self.spreads[si]).pages.get_mut(pi) {
                    page.local_margins = true;
                }
            }
        }
    }

    /// Let document pages whose margins and columns match their parent page's follow it again,
    /// after an edit that set every page and parent page alike (Document Setup's margins).
    pub fn unmark_matching_local_margins(&mut self) {
        for si in 0..self.spreads.len() {
            for pi in 0..self.spreads[si].pages.len() {
                let Some(page) = self.spreads[si].pages.get(pi) else { continue };
                if !page.local_margins || !self.parent_page(page).is_some_and(|q| q.same_margins(page)) {
                    continue;
                }
                if let Some(page) = Arc::make_mut(&mut self.spreads[si]).pages.get_mut(pi) {
                    page.local_margins = false;
                }
            }
        }
    }

    /// Parent `pi` and the parents it is based on, base first: `[A, B]` for B based on A.
    /// A dangling id, a repeated parent or [`MAX_PARENT_DEPTH`] ends the walk.
    pub fn parent_chain(&self, pi: usize) -> Vec<usize> {
        let mut out = Vec::new();
        let mut cur = Some(pi);
        while let Some(i) = cur {
            let Some(ps) = self.parents.get(i) else { break };
            if out.contains(&i) || out.len() >= MAX_PARENT_DEPTH {
                break;
            }
            out.push(i);
            cur = ps.parent.as_ref().and_then(|info| info.based_on).and_then(|b| self.parent_index(b));
        }
        out.reverse();
        out
    }

    /// Parent pages drawn behind page `page` of spread `sr`, back to front, as (parent index,
    /// parent page index). A document page shows its parent and that parent's bases; a parent page
    /// shows the parents its spread is based on.
    pub fn shown_parent_pages(&self, sr: SpreadRef, page: usize) -> Vec<(usize, usize)> {
        let Some(pg) = self.spread(sr).and_then(|sp| sp.pages.get(page)) else { return Vec::new() };
        let chain = match sr {
            SpreadRef::Doc(_) => pg.parent.and_then(|id| self.parent_index(id)).map(|pi| self.parent_chain(pi)).unwrap_or_default(),
            SpreadRef::Parent(pi) => {
                let mut c = self.parent_chain(pi);
                // The spread's own items are not part of what shows behind it.
                if c.last() == Some(&pi) {
                    c.pop();
                }
                c
            }
        };
        chain.into_iter().filter_map(|i| Some((i, self.parents.get(i)?.parent_page_at(pg.side, page)?))).collect()
    }

    /// Whether basing parent `pi` on `base` would make a `based_on` cycle (or names no parent).
    pub fn parent_base_cycles(&self, pi: usize, base: SpreadId) -> bool {
        let Some(mut cur) = self.parent_index(base) else { return true };
        // Not `parent_chain`: its depth cap would miss a long cycle. Every parent is visited at most once.
        let mut seen = std::collections::HashSet::new();
        loop {
            if cur == pi {
                return true;
            }
            if !seen.insert(cur) || seen.len() > self.parents.len() {
                return false;
            }
            let next = self.parents.get(cur).and_then(|ps| ps.parent.as_ref()).and_then(|info| info.based_on).and_then(|b| self.parent_index(b));
            let Some(n) = next else { return false };
            cur = n;
        }
    }

    /// Clear `based_on` links that name a missing parent or close a cycle (hand-edited or damaged
    /// files). Returns one warning per link cleared.
    pub fn fix_parent_bases(&mut self) -> Vec<String> {
        let mut warnings = Vec::new();
        for pi in 0..self.parents.len() {
            let Some(ps) = self.parents.get(pi) else { continue };
            let Some(base) = ps.parent.as_ref().and_then(|info| info.based_on) else { continue };
            let label = ps.parent.as_ref().map(ParentInfo::label).unwrap_or_default();
            let why = if self.parent_index(base).is_none() {
                "names a missing parent"
            } else if self.parent_base_cycles(pi, base) {
                "makes a cycle"
            } else {
                continue;
            };
            if let Some(info) = self.parents.get_mut(pi).map(Arc::make_mut).and_then(|ps| ps.parent.as_mut()) {
                info.based_on = None;
            }
            warnings.push(format!("parent {label}: Based on Parent {why}; it was set to [None]"));
        }
        warnings
    }
}

/// Deepest `based_on` chain followed.
pub const MAX_PARENT_DEPTH: usize = 64;

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

    /// Files saved before pages tracked local margins: pages that differ from their parent keep
    /// their margins; the rest follow the parent.
    #[test]
    fn differing_margins_from_older_files_stay_local() {
        let mut d = Document::new(&NewDocument { pages: 2, ..Default::default() });
        Arc::make_mut(&mut d.spreads[1]).pages[0].margins = Margins::uniform(90.0);
        d.mark_differing_margins_local();
        Arc::make_mut(&mut d.parents[0]).pages.iter_mut().for_each(|p| p.margins = Margins::uniform(12.0));
        d.sync_parent_margins();
        assert_eq!(d.page(0).map(|p| (p.margins.top, p.local_margins)), Some((12.0, false)));
        assert_eq!(d.page(1).map(|p| (p.margins.top, p.local_margins)), Some((90.0, true)));
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

    /// Pages inserted after a page with dragged column guides keep them, as they keep its column
    /// count and gutter; pages that follow a parent take the parent's.
    #[test]
    fn inserted_pages_keep_dragged_columns() {
        let dragged = Columns { count: 2, gutter: 12.0, positions: Some(vec![0.0, 100.0, 130.0, 540.0]) };
        let widths = |p: &Page| p.column_rects().iter().map(|r| r.width()).collect::<Vec<_>>();
        let mut d = Document::new(&NewDocument { pages: 1, facing_pages: false, ..Default::default() });
        let pg = &mut Arc::make_mut(&mut d.spreads[0]).pages[0];
        (pg.columns, pg.local_margins) = (dragged.clone(), true);
        d.insert_pages(Some(0), 2, None).unwrap();
        for i in 1..3 {
            let p = d.page(i).unwrap();
            assert_eq!(p.columns, dragged, "page {i}");
            assert_eq!(widths(p), vec![100.0, 410.0], "page {i}");
        }

        let mut d = Document::new(&NewDocument { pages: 1, facing_pages: false, ..Default::default() });
        Arc::make_mut(&mut d.parents[0]).pages[0].columns = dragged.clone();
        d.sync_parent_margins();
        d.insert_pages(Some(0), 1, d.parents.first().map(|p| p.id)).unwrap();
        assert_eq!(d.page(1).map(widths), Some(vec![100.0, 410.0]));
    }

    /// A right page whose gutter was dragged toward the spine keeps it there when inserting a
    /// page before it makes it a left page: its dragged columns mirror across the margin box.
    #[test]
    fn dragged_columns_mirror_when_a_page_changes_side() {
        let mut d = Document::new(&NewDocument { pages: 1, facing_pages: true, ..Default::default() });
        let pg = &mut Arc::make_mut(&mut d.spreads[0]).pages[0];
        assert_eq!(pg.side, PageSide::Right);
        let w = pg.margin_rect().width();
        (pg.columns, pg.local_margins) = (Columns { count: 2, gutter: 12.0, positions: Some(vec![0.0, 100.0, 112.0, w]) }, true);
        let id = pg.id;
        // How far the gutter's spine-side edge is from the margin on the spine side.
        let from_spine = |p: &Page| {
            let (m, c) = (p.margin_rect(), p.column_rects());
            match p.side {
                PageSide::Left => m.x1 - c[1].x0,
                _ => c[0].x1 - m.x0,
            }
        };
        assert!((from_spine(d.page(0).unwrap()) - 100.0).abs() < 1e-9);

        d.insert_pages(None, 1, None).unwrap();
        let moved = d.page(1).unwrap();
        assert_eq!((moved.id, moved.side), (id, PageSide::Left));
        assert!((from_spine(moved) - 100.0).abs() < 1e-9, "{:?}", moved.column_rects());
        let widths: Vec<f64> = moved.column_rects().iter().map(|r| r.width()).collect();
        assert!((widths[0] - (w - 112.0)).abs() < 1e-9 && (widths[1] - 100.0).abs() < 1e-9, "{widths:?}");
        // The inserted page is a right page like the one it copies: no mirroring.
        assert_eq!(d.page(0).unwrap().columns.positions, Some(vec![0.0, 100.0, 112.0, w]));

        // Moving it back to the right undoes the mirror.
        d.delete_pages(&[0]).unwrap();
        let back = d.page(0).unwrap();
        assert_eq!(back.side, PageSide::Right);
        assert!((from_spine(back) - 100.0).abs() < 1e-9, "{:?}", back.column_rects());
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

    #[test]
    fn presets_and_parents() {
        let nd = NewDocument::from_preset("A4").unwrap();
        assert!((nd.width - 595.2756).abs() < 1e-3);
        let a0 = NewDocument::from_preset("A0").unwrap();
        assert_eq!((a0.width.round(), a0.height.round()), (2384.0, 3370.0));
        let mut d = Document::new(&nd);
        assert_eq!(d.next_parent_prefix(), "B");
        let b = d.add_parent("B", "Parent", 2, 12.0, Margins::uniform(36.0));
        d.apply_parent(&[0], Some(b)).unwrap();
        assert_eq!(d.parent_page_for(0), Some((1, 1)));
        d.apply_parent(&[0], None).unwrap();
        assert_eq!(d.parent_page_for(0), None);
    }

    fn base(d: &mut Document, pi: usize, on: Option<SpreadId>) {
        Arc::make_mut(&mut d.parents[pi]).parent.as_mut().unwrap().based_on = on;
    }

    /// C based on B based on A: pages using C show A, then B, then C; C itself shows A and B.
    #[test]
    fn parent_chain_is_base_first() {
        let mut d = Document::new(&NewDocument { pages: 2, facing_pages: false, ..Default::default() });
        let a = d.parents[0].id;
        let b = d.add_parent("B", "Parent", 1, 12.0, Margins::uniform(36.0));
        let c = d.add_parent("C", "Parent", 1, 12.0, Margins::uniform(36.0));
        base(&mut d, 1, Some(a));
        base(&mut d, 2, Some(b));
        d.apply_parent(&[1], Some(c)).unwrap();
        assert_eq!(d.parent_chain(2), vec![0, 1, 2]);
        assert_eq!(d.shown_parent_pages(SpreadRef::Doc(1), 0), vec![(0, 0), (1, 0), (2, 0)]);
        assert_eq!(d.shown_parent_pages(SpreadRef::Doc(0), 0), vec![(0, 0)]);
        assert_eq!(d.shown_parent_pages(SpreadRef::Parent(2), 0), vec![(0, 0), (1, 0)]);
        assert!(d.parent_base_cycles(0, c), "A on C closes C → B → A");
        assert!(d.parent_base_cycles(0, a));
        assert!(!d.parent_base_cycles(2, a));
        assert!(d.fix_parent_bases().is_empty());
    }

    /// A cycle or a dangling base from a file is cleared with a warning; walks stay bounded before that.
    #[test]
    fn fix_parent_bases_breaks_cycles_and_dangling_ids() {
        let mut d = Document::new(&NewDocument { pages: 1, facing_pages: false, ..Default::default() });
        let a = d.parents[0].id;
        let b = d.add_parent("B", "Parent", 1, 12.0, Margins::uniform(36.0));
        d.add_parent("C", "Parent", 1, 12.0, Margins::uniform(36.0));
        base(&mut d, 0, Some(b));
        base(&mut d, 1, Some(a));
        base(&mut d, 2, Some(SpreadId(99_999)));
        assert_eq!(d.parent_chain(0), vec![1, 0]);
        assert_eq!(d.parent_chain(2), vec![2]);
        let w = d.fix_parent_bases();
        assert_eq!(w.len(), 2, "{w:?}");
        assert!(w[0].contains("A-Parent") && w[0].contains("cycle"), "{w:?}");
        assert!(w[1].contains("C-Parent") && w[1].contains("missing"), "{w:?}");
        assert_eq!(d.parents[0].parent.as_ref().unwrap().based_on, None);
        assert_eq!(d.parents[1].parent.as_ref().unwrap().based_on, Some(a));
        assert_eq!(d.parents[2].parent.as_ref().unwrap().based_on, None);
    }

    /// A cycle longer than the drawing depth cap is still found and broken.
    #[test]
    fn fix_parent_bases_breaks_a_long_cycle() {
        let mut d = Document::new(&NewDocument { pages: 1, facing_pages: false, ..Default::default() });
        for i in 1..65 {
            d.add_parent(&format!("P{i}"), "Parent", 1, 12.0, Margins::uniform(36.0));
        }
        assert_eq!(d.parents.len(), 65);
        let ids: Vec<SpreadId> = d.parents.iter().map(|p| p.id).collect();
        for i in 0..65 {
            base(&mut d, i, Some(ids[(i + 1) % 65]));
        }
        assert_eq!(d.parent_chain(0).len(), MAX_PARENT_DEPTH);
        let w = d.fix_parent_bases();
        assert_eq!(w.len(), 1, "{w:?}");
        assert_eq!(d.parents[0].parent.as_ref().unwrap().based_on, None);
        assert!((0..65).all(|i| d.parent_base_cycles(i, ids[(i + 1) % 65]) == (i == 0)), "only the cleared link would close it again");
    }
}
