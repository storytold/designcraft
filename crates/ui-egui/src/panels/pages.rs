//! Pages panel: parents at the top, document spreads with live thumbnails below.

use std::collections::HashMap;
use std::sync::Mutex;

use designcraft_doc::{SpreadId, SpreadRef};
use egui::Color32;
use serde_json::json;

use crate::DesignApp;

/// What a thumbnail shows: a document page (absolute index) or a whole parent spread.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
enum ThumbOf {
    Page(usize),
    Parent(SpreadId),
}

/// (document uid, page or parent, thumbnail height).
type ThumbKey = (u64, ThumbOf, u32);

/// The newest thumbnail of each page and parent, with the document revision it shows.
static THUMBS: Mutex<Option<HashMap<ThumbKey, Thumb>>> = Mutex::new(None);

/// A rendered thumbnail: the document revision, and, when it drew images still decoding in the
/// background, the image generation and time (s) it was rendered at.
type Thumb = (u64, Option<(u64, f64)>, egui::TextureHandle);

/// How soon (s) a thumbnail drawn with images still decoding is rendered again once more have
/// finished: a page of photos decoding one after another re-renders a few times, not once each.
const THUMB_IMAGES_SECS: f64 = 0.25;

/// Widest parent thumbnail, in pixels (a spread of many pages stays a small texture).
const MAX_THUMB_W: f64 = 1024.0;

/// Seconds the document stays unchanged before thumbnails of an older revision are drawn again.
const THUMB_QUIET_SECS: f64 = 0.4;

/// Thumbnails of an older revision drawn again in one frame.
const THUMB_REFRESHES_PER_FRAME: usize = 1;

/// When the active document's revision last changed, and how many thumbnails were drawn again in
/// the current frame.
#[derive(Clone, Copy, Debug, Default)]
struct ThumbClock {
    doc: (u64, u64),
    changed_at: f64,
    frame: u64,
    refreshed: usize,
}

#[cfg(test)]
thread_local! {
    /// Thumbnails rendered on this thread.
    static THUMB_RENDERS: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
}

/// The thumbnail of `of`, `h` points high. A thumbnail of an older revision stays on screen until
/// the document has been unchanged for [`THUMB_QUIET_SECS`]; after that, at most
/// [`THUMB_REFRESHES_PER_FRAME`] of them are drawn again per frame.
fn thumb(app: &mut DesignApp, ctx: &egui::Context, of: ThumbOf, h: f32) -> Option<egui::TextureHandle> {
    let st = app.session.active()?;
    let key = (st.uid, of, h.round() as u32);
    let clock_id = egui::Id::unique("pages_thumb_clock");
    let (now, frame) = (ctx.input(|i| i.time), ctx.cumulative_frame_nr());
    let mut clock = ctx.data(|d| d.get_temp::<ThumbClock>(clock_id)).unwrap_or_default();
    let changed = clock.doc != (st.uid, st.revision);
    if changed {
        clock = ThumbClock { doc: (st.uid, st.revision), changed_at: now, frame, refreshed: 0 };
    }
    if clock.frame != frame {
        clock.frame = frame;
        clock.refreshed = 0;
    }
    if now < clock.changed_at {
        clock.changed_at = now;
    }
    let mut g = THUMBS.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
    let map = g.get_or_insert_with(HashMap::new);
    if changed {
        // Drop the thumbnails of pages and parents the document no longer has.
        let pages = st.doc.page_count();
        map.retain(|k, _| {
            k.0 != st.uid
                || match k.1 {
                    ThumbOf::Page(abs) => abs < pages,
                    ThumbOf::Parent(id) => st.doc.parent_index(id).is_some(),
                }
        });
    }
    if let Some((rev, pending, t)) = map.get(&key) {
        if *rev == st.revision {
            // Images it drew as decoding have finished since: draw it again, in batches.
            let stale = pending.and_then(|(images, at)| (images != designcraft_render::images::generation()).then_some(at + THUMB_IMAGES_SECS - now));
            match stale {
                Some(wait) if wait <= 0.0 && clock.refreshed < THUMB_REFRESHES_PER_FRAME => {}
                _ => {
                    if let Some(after) = stale.and_then(|w| std::time::Duration::try_from_secs_f64(w.max(0.0)).ok()) {
                        ctx.request_repaint_after(after);
                    }
                    ctx.data_mut(|d| d.insert_temp(clock_id, clock));
                    return Some(t.clone());
                }
            }
        }
        let wait = THUMB_QUIET_SECS - (now - clock.changed_at);
        if wait > 0.0 || clock.refreshed >= THUMB_REFRESHES_PER_FRAME {
            ctx.request_repaint_after(std::time::Duration::from_secs_f64(wait.max(0.0)));
            ctx.data_mut(|d| d.insert_temp(clock_id, clock));
            return Some(t.clone());
        }
        clock.refreshed += 1;
    }
    ctx.data_mut(|d| d.insert_temp(clock_id, clock));
    #[cfg(test)]
    THUMB_RENDERS.with(|n| n.set(n.get() + 1));
    let px = h as f64 * ctx.pixels_per_point() as f64;
    let mut r = designcraft_render::Renderer::new();
    r.threads = 0;
    // Thumbnails don't wait for decodes: images still decoding are drawn when they finish.
    let opts = designcraft_render::RenderOptions { printing_only: true, defer_images: true, ..Default::default() };
    let images = designcraft_render::images::generation();
    let (img, name) = match of {
        ThumbOf::Page(abs) => {
            let page = st.doc.page(abs)?;
            let img = r.render_page(&st.doc, &app.session.cache, abs, px / page.height.max(1e-6), false, &opts)?;
            (img, format!("thumb{abs}"))
        }
        ThumbOf::Parent(id) => {
            let i = st.doc.parent_index(id)?;
            let b = st.doc.parents.get(i)?.bounds();
            if !(b.width() > 0.0 && b.height() > 0.0) {
                return None;
            }
            let scale = (px / b.height()).min(MAX_THUMB_W / b.width());
            let (w, h) = ((b.width() * scale).round().max(1.0) as u32, (b.height() * scale).round().max(1.0) as u32);
            let view = designcraft_geom::Affine::scale(scale) * designcraft_geom::Affine::translate(-b.origin().to_vec2());
            let o = designcraft_render::RenderOptions { background: Some([255, 255, 255, 255]), ..opts };
            let placed = [designcraft_render::Placed { spread: SpreadRef::Parent(i), xf: designcraft_geom::Affine::IDENTITY }];
            (r.render(&st.doc, &app.session.cache, &placed, (w, h), view, &o), format!("thumbp{}", id.0))
        }
    };
    let ci = egui::ColorImage::from_rgba_premultiplied([img.width as usize, img.height as usize], &img.pixels);
    let tex = crate::widgets::load_texture(ctx, format!("{name}_{}", h.round()), ci, egui::TextureOptions::LINEAR);
    map.insert(key, (st.revision, r.images_pending().then_some((images, now)), tex.clone()));
    Some(tex)
}

/// Thumbnail sizes of the Pages panel (Panel Options › Size), InDesign's six.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum IconSize {
    ExtraSmall,
    #[default]
    Small,
    Medium,
    Large,
    ExtraLarge,
    Jumbo,
}

impl IconSize {
    pub const ALL: [IconSize; 6] = [IconSize::ExtraSmall, IconSize::Small, IconSize::Medium, IconSize::Large, IconSize::ExtraLarge, IconSize::Jumbo];

    pub fn label(self) -> &'static str {
        match self {
            IconSize::ExtraSmall => "Extra Small",
            IconSize::Small => "Small",
            IconSize::Medium => "Medium",
            IconSize::Large => "Large",
            IconSize::ExtraLarge => "Extra Large",
            IconSize::Jumbo => "Jumbo",
        }
    }

    pub fn key(self) -> &'static str {
        match self {
            IconSize::ExtraSmall => "extraSmall",
            IconSize::Small => "small",
            IconSize::Medium => "medium",
            IconSize::Large => "large",
            IconSize::ExtraLarge => "extraLarge",
            IconSize::Jumbo => "jumbo",
        }
    }

    pub fn from_key(k: &str) -> Option<IconSize> {
        IconSize::ALL.into_iter().find(|s| s.key() == k)
    }

    /// Height of a page thumbnail.
    fn page_h(self) -> f32 {
        [14.0, 24.0, THUMB_H, 55.0, 80.0, 120.0][self as usize]
    }

    /// Height of a parent row.
    fn row_h(self) -> f32 {
        [18.0, ROW_H, 40.0, 56.0, 76.0, 110.0][self as usize]
    }
}

/// How the Pages panel's parts share a change of the panel's height (Panel Options › Resize).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum PanelResize {
    Proportional,
    PagesFixed,
    #[default]
    ParentsFixed,
}

impl PanelResize {
    pub const ALL: [PanelResize; 3] = [PanelResize::Proportional, PanelResize::PagesFixed, PanelResize::ParentsFixed];

    pub fn label(self) -> &'static str {
        match self {
            PanelResize::Proportional => "Proportional",
            PanelResize::PagesFixed => "Pages Fixed",
            PanelResize::ParentsFixed => "Parents Fixed",
        }
    }

    pub fn key(self) -> &'static str {
        match self {
            PanelResize::Proportional => "proportional",
            PanelResize::PagesFixed => "pagesFixed",
            PanelResize::ParentsFixed => "parentsFixed",
        }
    }

    pub fn from_key(k: &str) -> Option<PanelResize> {
        PanelResize::ALL.into_iter().find(|s| s.key() == k)
    }
}

/// The Pages panel's Panel Options, as InDesign's (kept with the UI settings).
#[derive(Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct PanelOptions {
    pub page_size: IconSize,
    pub page_thumbnails: bool,
    pub parent_size: IconSize,
    pub parent_thumbnails: bool,
    pub parents_vertical: bool,
    pub pages_on_top: bool,
    pub resize: PanelResize,
}

impl Default for PanelOptions {
    fn default() -> Self {
        PanelOptions {
            page_size: IconSize::Medium,
            page_thumbnails: true,
            parent_size: IconSize::Small,
            parent_thumbnails: true,
            parents_vertical: true,
            pages_on_top: false,
            resize: PanelResize::ParentsFixed,
        }
    }
}

/// What is being dragged in the Pages panel.
#[derive(Clone, Debug, PartialEq)]
enum PageDrag {
    /// A document page (absolute index).
    Page(usize),
    /// A parent (`None` = [None]).
    Parent(Option<SpreadId>),
}

/// The footer: page count and buttons, always at the panel's bottom.
const FOOTER_H: f32 = 28.0;
/// The draggable rule between the parents and the pages.
const DIVIDER_H: f32 = 9.0;
/// Parent rows shown at first; the divider shows more.
const PARENTS_DEFAULT_ROWS: f32 = 4.5;
/// Selected-page tint (multiplied over the thumbnail).
const SELECTED_TINT: Color32 = Color32::from_rgb(0x88, 0xc7, 0xfb);
/// Page-number badge of the selected page.
const BADGE: Color32 = Color32::from_rgb(0x2f, 0x73, 0xe0);
/// Thumbnail height (InDesign: about 28×37 pt for Letter).
const THUMB_H: f32 = 37.0;
/// Parent rows.
const ROW_H: f32 = 29.0;
/// The New Parent row under the parents.
const NEW_PARENT_H: f32 = 20.0;
/// An alternate layout's column: its header's height and the narrowest column.
const LAYOUT_HEADER_H: f32 = 34.0;
const LAYOUT_COL_MIN: f32 = 96.0;

/// A spread's label under its thumbnails, as InDesign's: one name for a single page, "2–3" for
/// pages numbered on in one section (an en dash), and the names apart ("2, 1", "8, A") when a section starts
/// inside the spread.
/// The names read left to right as the thumbnails are drawn, so a spread bound right to left
/// lists its second page first.
pub(crate) fn spread_label(app: &DesignApp, doc: &designcraft_doc::Document, first: usize, n: usize) -> String {
    let mut order: Vec<usize> = (first..first + n).collect();
    // In the order the thumbnails are drawn: by each page's x (a right-to-left pair shows its second page on the left).
    if let Some(sp) = doc.page_loc(first).and_then(|(si, _)| doc.spreads.get(si)) {
        let x = |abs: usize| sp.pages.get(abs.saturating_sub(first)).map_or(0.0, |p| p.x);
        order.sort_by(|&a, &b| x(a).total_cmp(&x(b)));
    }
    let names: Vec<String> = order.into_iter().map(|abs| app.session.page_label(abs)).collect();
    let starts = |abs: usize| abs == 0 || doc.sections.iter().any(|s| s.start == abs);
    match (names.first(), names.last()) {
        (Some(a), Some(b)) if names.len() > 1 => {
            if !app.session.prefs.absolute_page_numbers && (first + 1..first + n).any(starts) {
                names.join(", ")
            } else {
                format!("{a}\u{2013}{b}")
            }
        }
        (Some(a), _) => a.clone(),
        _ => String::new(),
    }
}

/// A row of the Pages panel's parents list: [None] (`parent` = `None`) or a parent.
struct ParentRow {
    label: String,
    prefix: Option<String>,
    /// Index in `doc.parents` and id.
    parent: Option<(usize, SpreadId)>,
    /// Page outlines (left, width) and the icon's width, for an icon 1 high.
    pages: Vec<(f32, f32)>,
    unit_w: f32,
}

impl ParentRow {
    /// A parent's row, from the spread's own pages; blank pages of `aspect` when it has no size.
    fn new(i: usize, sp: &designcraft_doc::Spread, aspect: f32) -> ParentRow {
        let b = sp.bounds();
        let (pages, unit_w) = if b.width() > 0.0 && b.height() > 0.0 && b.width().is_finite() && b.height().is_finite() {
            let k = 1.0 / b.height();
            (sp.pages.iter().map(|p| (((p.x - b.x0) * k) as f32, (p.width * k) as f32)).collect(), (b.width() * k) as f32)
        } else {
            let n = sp.pages.len().max(1);
            ((0..n).map(|k| (aspect * k as f32, aspect)).collect(), aspect * n as f32)
        };
        ParentRow {
            label: sp.parent.as_ref().map(|i| i.label()).unwrap_or_default(),
            prefix: sp.parent.as_ref().map(|i| i.prefix.clone()),
            parent: Some((i, sp.id)),
            pages,
            unit_w,
        }
    }

    fn none(aspect: f32) -> ParentRow {
        ParentRow { label: "[None]".into(), prefix: None, parent: None, pages: vec![(0.0, aspect)], unit_w: aspect }
    }
}

/// What the Pages panel shows this frame and how its parts share the room.
struct Plan {
    uid: u64,
    editing_parents: bool,
    /// The [None] row, then each parent.
    rows: Vec<ParentRow>,
    opts: PanelOptions,
    /// A parent's row and the height of its icon.
    row_h: f32,
    icon_h: f32,
    parent_thumbnails: bool,
    /// How far the divider can move, and the heights the parents and the pages get.
    parents_min: f32,
    parents_max: f32,
    parents_h: f32,
    space: f32,
    pages_h: f32,
    /// The current page and its spread.
    cur: usize,
    cur_spread: Option<usize>,
    /// The parent (index in `doc.parents`) the canvas shows while parents are edited.
    cur_parent: Option<usize>,
    /// Outside Edit Parents, the parent clicked in the list (the panel menu acts on it).
    selected_parent: Option<SpreadId>,
    tallest: f64,
    /// A page thumbnail's height.
    th: f32,
    thumbnails: bool,
    layouts: Vec<designcraft_doc::LayoutRange>,
    columns: bool,
}

/// The width of a parent's tile when the parents lie side by side (Show Vertically off): its icon and the prefix.
fn tile_width(icon_h: f32, unit_w: f32) -> f32 {
    (icon_h * unit_w + 16.0).max(44.0)
}

impl Plan {
    fn new(app: &DesignApp, ui: &egui::Ui, doc: &designcraft_doc::Document, uid: u64, editing_parents: bool) -> Plan {
        let aspect = (doc.settings.page_width / doc.settings.page_height.max(1.0)) as f32;
        // While parents are edited, the row of the parent the canvas shows is selected.
        let cur_parent = if editing_parents {
            let layout = designcraft_tools::CanvasLayout::new(doc, true);
            crate::canvas::current_slot(app, &layout).and_then(|i| layout.slots.get(i)).and_then(|s| match s.spread {
                SpreadRef::Parent(i) => Some(i),
                SpreadRef::Doc(_) => None,
            })
        } else {
            None
        };
        let selected_parent = selected_parent(app).filter(|id| !editing_parents && doc.parent_index(*id).is_some());
        let mut rows = vec![ParentRow::none(aspect)];
        rows.extend(doc.parents.iter().enumerate().map(|(i, p)| ParentRow::new(i, p, aspect)));
        // As in InDesign: parents and pages each scroll on their own (parents on top unless Panel
        // Options says pages), a divider between them resizes them, the footer stays at the bottom.
        let opts = app.ui.pages_panel.clone();
        let total = {
            let h = ui.available_height();
            if h.is_finite() && h < 4000.0 { h } else { 480.0 }
        }
        .max(160.0);
        let row_h = opts.parent_size.row_h();
        let icon_h = row_h - 5.0;
        // Extra Small has no room for a thumbnail; Show Thumbnails off draws blank pages.
        let parent_thumbnails = opts.parent_thumbnails && opts.parent_size != IconSize::ExtraSmall;
        let natural = if opts.parents_vertical {
            rows.len() as f32 * row_h
        } else {
            let width = (ui.available_width() - 4.0).max(1.0);
            let (mut lines, mut x) = (1.0_f32, 0.0_f32);
            for row in &rows {
                let w = tile_width(icon_h, row.unit_w) + 6.0;
                if x > 0.0 && x + w > width {
                    lines += 1.0;
                    x = 0.0;
                }
                x += w;
            }
            lines * (icon_h + 22.0)
        } + NEW_PARENT_H;
        let space = (total - FOOTER_H - DIVIDER_H).max(2.0 * ROW_H);
        let parents_max = (space - 2.0 * ROW_H).max(ROW_H);
        // A row taller than the room left (large icons in a short panel) gets that room.
        let parents_min = row_h.min(natural).min(parents_max);
        // The divider's place as last dragged, and how a change of the panel's height is shared.
        let parents_h = match app.ui.pages_parents_height {
            None => natural.min(PARENTS_DEFAULT_ROWS * ROW_H),
            Some([h, at]) => match opts.resize {
                PanelResize::ParentsFixed => h,
                PanelResize::PagesFixed => h + (space - at),
                PanelResize::Proportional => h * space / at.max(1.0),
            },
        }
        .clamp(parents_min, parents_max)
        .min(natural);
        let pages_h = (space - parents_h).max(ROW_H);
        let cur = crate::canvas::current_page(app).unwrap_or(0);
        let cur_spread = doc.page_loc(cur).map(|l| l.0);
        let tallest = doc.spreads.iter().flat_map(|sp| sp.pages.iter().map(|p| p.height)).fold(doc.settings.page_height, f64::max).max(1.0);
        let th = opts.page_size.page_h();
        // Extra Small has no room for a thumbnail; Show Thumbnails off draws blank pages.
        let thumbnails = opts.page_thumbnails && opts.page_size != IconSize::ExtraSmall;
        let layouts = doc.layouts();
        let columns = layouts.len() > 1;
        Plan {
            uid,
            editing_parents,
            rows,
            opts,
            row_h,
            icon_h,
            parent_thumbnails,
            parents_min,
            parents_max,
            parents_h,
            space,
            pages_h,
            cur,
            cur_spread,
            cur_parent,
            selected_parent,
            tallest,
            th,
            thumbnails,
            layouts,
            columns,
        }
    }

    /// One scale for the whole document (pages of their own size show in proportion): the document's page size at the
    /// panel's size, unless a taller page would pass 1.8 times it.
    fn scale(&self, doc: &designcraft_doc::Document) -> f32 {
        (self.th / doc.settings.page_height.max(1.0) as f32).min(1.8 * self.th / self.tallest as f32)
    }
}

/// A page or a parent dropped on a page: what was dragged, the page it landed on and whether the drop copies (Alt).
type PageDrop = (PageDrag, usize, bool);

/// Applies a drop on a page: pages reorder (Alt copies a page that is a spread of its own to where it is dropped, in one
/// undo step; a page from a larger spread moves as usual), parents apply.
fn apply_drop(app: &mut DesignApp, doc: &designcraft_doc::Document, drop: Option<PageDrop>) {
    match drop {
        Some((PageDrag::Page(from), to, alt)) => {
            let copy = doc.page_loc(from).and_then(|(si, _)| (alt && doc.spreads.get(si).is_some_and(|s| s.pages.len() == 1)).then_some(si));
            if let Some(si) = copy {
                let _ = app.run("layout.pages.duplicateSpread", json!({"spread": si, "to": to}));
            } else if from != to {
                let _ = app.run("layout.pages.move", json!({"from": from, "to": to}));
            }
        }
        Some((PageDrag::Parent(parent), to, _)) => {
            let _ = app.run("layout.pages.applyParent", json!({"pages": [to], "parent": parent.map(|id| id.0)}));
        }
        None => {}
    }
}

pub fn show(app: &mut DesignApp, ui: &mut egui::Ui) {
    let Some(st) = app.session.active() else { return };
    let doc = st.doc.clone();
    let plan = Plan::new(app, ui, &doc, st.uid, st.editing_parents);
    framework::show(app, ui, &doc, &plan);
}

/// The panel menu's page commands, acting on the current page and its spread.
pub fn panel_menu(app: &mut DesignApp, ui: &mut egui::Ui) {
    let Some(st) = app.session.active() else { return };
    let doc = st.doc.clone();
    let cur = crate::canvas::current_page(app).unwrap_or(0);
    let Some((si, _)) = doc.page_loc(cur) else { return };
    let layouts = doc.layouts();
    // The dock's menu button opens a framework menu: its rows are `MenuItem`s.
    framework::panel_menu(app, ui, &doc, &layouts, cur, si);
}

/// Panel Options…, with the panel's current options.
pub fn open_panel_options(app: &mut DesignApp) {
    let o = &app.ui.pages_panel;
    app.ui.dialog = Some(crate::dialogs::Dialog::new(
        "pagesPanelOptions",
        json!({
            "pageSize": o.page_size.key(), "pageThumbnails": o.page_thumbnails,
            "parentSize": o.parent_size.key(), "parentThumbnails": o.parent_thumbnails, "parentsVertical": o.parents_vertical,
            "layout": if o.pages_on_top { "pagesOnTop" } else { "parentsOnTop" }, "resize": o.resize.key(),
        }),
    ));
}

/// The spreads holding layout `l`'s pages (every layout starts a spread of its own).
fn spread_range(doc: &designcraft_doc::Document, l: &designcraft_doc::LayoutRange) -> std::ops::Range<usize> {
    let first = doc.page_loc(l.start).map_or(doc.spreads.len(), |(si, _)| si);
    let end = l.end.checked_sub(1).and_then(|last| doc.page_loc(last)).map_or(first, |(si, _)| si.saturating_add(1));
    first..end.max(first)
}

/// The size of layout `l`'s first page, in points.
fn layout_size(doc: &designcraft_doc::Document, l: &designcraft_doc::LayoutRange) -> (f64, f64) {
    doc.page(l.start).map_or((doc.settings.page_width, doc.settings.page_height), |p| (p.width, p.height))
}

/// The layout (index into `layouts`) holding absolute page `abs`.
pub(crate) fn layout_index(layouts: &[designcraft_doc::LayoutRange], abs: usize) -> usize {
    layouts.iter().rposition(|l| l.start <= abs).unwrap_or(0)
}

/// At most `n` characters of `s`, with an ellipsis when cut.
fn truncate(s: &str, n: usize) -> String {
    if s.chars().count() <= n { s.to_string() } else { format!("{}…", s.chars().take(n.saturating_sub(1)).collect::<String>()) }
}

/// Create Alternate Layout…, from layout `k`, at its pages' other orientation.
pub(crate) fn open_create_alternate(app: &mut DesignApp, doc: &designcraft_doc::Document, layouts: &[designcraft_doc::LayoutRange], k: usize) {
    let Some(l) = layouts.get(k) else { return };
    let (w, h) = layout_size(doc, l);
    let (nw, nh) = (h, w);
    let name = format!("{} {}", l.name, if nw > nh { "H" } else { "V" });
    app.ui.dialog = Some(crate::dialogs::Dialog::new("cmd:layout.createAlternate", json!({"name": name, "width": nw, "height": nh, "from": l.name})));
}

/// Alternate Layout Options… for layout `k`: its name and page size.
pub(crate) fn open_layout_options(app: &mut DesignApp, doc: &designcraft_doc::Document, layouts: &[designcraft_doc::LayoutRange], k: usize) {
    let Some(l) = layouts.get(k) else { return };
    let (w, h) = layout_size(doc, l);
    app.ui.dialog = Some(crate::dialogs::Dialog::new(
        "cmd:layout.alternate.options",
        json!({"layout": l.name, "name": l.name, "width": w, "height": h, "sizeWas": [w, h]}),
    ));
}

/// Parent Options… for parent `id`: prefix, name and Based on Parent.
pub(crate) fn open_parent_options(app: &mut DesignApp, doc: &designcraft_doc::Document, id: SpreadId) {
    let Some(info) = doc.parent_index(id).and_then(|i| doc.parents.get(i)).and_then(|p| p.parent.as_ref()) else { return };
    let bases: Vec<serde_json::Value> =
        doc.parents.iter().filter(|p| p.id != id).filter_map(|p| p.parent.as_ref().map(|i| json!([p.id.0.to_string(), i.label()]))).collect();
    app.ui.dialog = Some(crate::dialogs::Dialog::new(
        "parentOptions",
        json!({
            "parent": id.0,
            "prefix": info.prefix,
            "name": info.name,
            "basedOn": info.based_on.map(|b| b.0.to_string()).unwrap_or_default(),
            "bases": bases,
        }),
    ));
}

/// The parent selected in the parents list, when it belongs to the active document.
pub(crate) fn selected_parent(app: &DesignApp) -> Option<SpreadId> {
    let uid = app.session.active()?.uid;
    app.ui.pages_selected_parent.filter(|(u, _)| *u == uid).map(|(_, id)| id)
}

/// Select parent `id` (None: none) of the active document in the parents list.
pub(crate) fn select_parent(app: &mut DesignApp, id: Option<SpreadId>) {
    app.ui.pages_selected_parent = id.zip(app.session.active().map(|st| st.uid)).map(|(id, uid)| (uid, id));
}

/// The parent the panel menu's parent commands act on: the one selected in the list, or the one
/// the canvas shows while parents are edited.
pub(crate) fn target_parent(app: &DesignApp, doc: &designcraft_doc::Document) -> Option<SpreadId> {
    if let Some(id) = selected_parent(app).filter(|id| doc.parent_index(*id).is_some()) {
        return Some(id);
    }
    if !app.session.active().is_some_and(|st| st.editing_parents) {
        return None;
    }
    let layout = designcraft_tools::CanvasLayout::new(doc, true);
    let i = crate::canvas::current_slot(app, &layout).and_then(|i| layout.slots.get(i)).and_then(|s| match s.spread {
        SpreadRef::Parent(i) => Some(i),
        SpreadRef::Doc(_) => None,
    })?;
    doc.parents.get(i).map(|p| p.id)
}

/// One parent command: Duplicate Parent Spread, Parent Options… or Delete Parent.
#[derive(Clone, Copy, PartialEq)]
enum ParentAction {
    Duplicate,
    Options,
    Delete,
}

impl ParentAction {
    const ALL: [ParentAction; 3] = [ParentAction::Duplicate, ParentAction::Options, ParentAction::Delete];

    fn label(self) -> &'static str {
        match self {
            ParentAction::Duplicate => "Duplicate Parent Spread",
            ParentAction::Options => "Parent Options…",
            ParentAction::Delete => "Delete Parent",
        }
    }

    fn run(self, app: &mut DesignApp, doc: &designcraft_doc::Document, id: SpreadId) {
        match self {
            ParentAction::Duplicate => {
                if let Ok(r) = app.run("layout.parents.duplicate", json!({"parent": id.0})) {
                    select_parent(app, r.get("id").and_then(serde_json::Value::as_u64).map(SpreadId));
                }
            }
            ParentAction::Options => open_parent_options(app, doc, id),
            ParentAction::Delete => {
                if app.run("layout.parents.delete", json!({"id": id.0})).is_ok() {
                    app.views.clear();
                    if selected_parent(app) == Some(id) {
                        select_parent(app, None);
                    }
                }
            }
        }
    }
}

/// What an entry of a Pages panel menu does.
#[derive(Clone)]
enum Act {
    InsertPages(usize),
    MovePages(usize),
    DuplicateSpread {
        si: usize,
        at_end: bool,
    },
    DeletePages(Vec<usize>),
    NewParent,
    Parent(ParentAction, SpreadId),
    ApplyParent {
        page: usize,
        parent: Option<SpreadId>,
    },
    /// A command on a page's parent items: Override All, Remove All Local Overrides, Detach All.
    PageCommand {
        id: &'static str,
        page: usize,
    },
    CreateAlternate(usize),
    DuplicateAlternate(usize),
    AlternateOptions(usize),
    ExportLayout(usize),
    DeleteAlternate(usize),
    /// Numbering & Section Options… for the page named: the page the menu was opened on.
    Section(usize),
    Shuffle {
        si: usize,
        allow: bool,
    },
    ToPreviousSpread {
        page: usize,
        si: usize,
    },
    PanelOptions,
}

impl Act {
    /// Does it. `layouts` are the document's layouts, for the layout commands.
    fn run(self, app: &mut DesignApp, doc: &designcraft_doc::Document, layouts: &[designcraft_doc::LayoutRange]) {
        match self {
            Act::InsertPages(page) => {
                crate::dialogs::open_insert_pages(app, page);
            }
            Act::MovePages(page) => {
                app.ui.dialog = Some(crate::dialogs::Dialog::new("cmd:layout.pages.move", json!({"from": page, "to": page})));
            }
            Act::DuplicateSpread { si, at_end } => {
                let params = if at_end { json!({"spread": si, "atEnd": true}) } else { json!({"spread": si}) };
                let _ = app.run("layout.pages.duplicateSpread", params);
            }
            Act::DeletePages(pages) => {
                let _ = crate::dialogs::delete_pages(app, pages);
            }
            Act::NewParent => {
                crate::menus::activate(app, "layout.parents.new", &serde_json::Value::Null);
            }
            Act::Parent(action, id) => action.run(app, doc, id),
            Act::ApplyParent { page, parent } => {
                let _ = app.run("layout.pages.applyParent", json!({"pages": [page], "parent": parent.map(|id| id.0)}));
            }
            Act::PageCommand { id, page } => {
                let _ = app.run(id, json!({"page": page}));
            }
            Act::CreateAlternate(k) => open_create_alternate(app, doc, layouts, k),
            Act::DuplicateAlternate(k) => {
                let _ = app.run("layout.alternate.duplicate", json!({"layout": k}));
            }
            Act::AlternateOptions(k) => open_layout_options(app, doc, layouts, k),
            Act::ExportLayout(k) => {
                let name = layouts.get(k).map(|l| l.name.clone());
                crate::export_batch_dialog::open(app, name.map(|n| vec![n]));
            }
            Act::DeleteAlternate(k) => {
                if app.run("layout.alternate.delete", json!({"layout": k})).is_ok() {
                    app.views.clear();
                }
            }
            Act::Section(page) => {
                // The dialog opens on the page in view, so the clicked page is brought into view first, leaving Edit Parents
                // as a page click does, as the section marker does.
                if app.session.active().is_some_and(|st| st.editing_parents) {
                    let _ = app.run("layout.parents.edit", json!({"on": false}));
                    if let Some(v) = app.view_mut() {
                        v.fitted = false;
                    }
                }
                crate::canvas::go_to_page(app, page);
                crate::menus::activate(app, "layout.section", &serde_json::Value::Null);
            }
            Act::Shuffle { si, allow } => {
                let _ = app.run("layout.spreadShuffle", json!({"spread": si, "allow": allow}));
            }
            Act::ToPreviousSpread { page, si } => {
                let _ = app.run("layout.pages.toSpread", json!({"page": page + 1, "spread": si.saturating_sub(1)}));
            }
            Act::PanelOptions => open_panel_options(app),
        }
    }
}

/// An entry of a Pages panel menu.
enum MenuEntry {
    Item {
        label: String,
        enabled: bool,
        check: Option<bool>,
        hint: Option<String>,
        act: Act,
    },
    Sep,
    /// A submenu, which is a row that opens `items`.
    Sub {
        label: String,
        enabled: bool,
        items: Vec<MenuEntry>,
    },
}

impl MenuEntry {
    fn item(label: impl Into<String>, act: Act) -> MenuEntry {
        MenuEntry::Item { label: label.into(), enabled: true, check: None, hint: None, act }
    }

    fn enabled(mut self, on: bool) -> MenuEntry {
        if let MenuEntry::Item { enabled, .. } = &mut self {
            *enabled = on;
        }
        self
    }
}

/// The entries of the menu on the panel's empty part, as InDesign's: the commands that need no page.
fn empty_entries(lang: &str, cur: usize) -> Vec<MenuEntry> {
    let tr = |label: &str| crate::i18n::tr(lang, label).to_owned();
    vec![
        MenuEntry::item(tr("Insert Pages…"), Act::InsertPages(cur)),
        MenuEntry::item(tr("Move Pages…"), Act::MovePages(cur)),
        MenuEntry::Sep,
        MenuEntry::item(tr("Panel Options…"), Act::PanelOptions),
    ]
}

/// The entries of the commands on layout `k` of `count`: in its column header's menu and in the panel menu.
fn layout_entries(lang: &str, count: usize, k: usize) -> Vec<MenuEntry> {
    let tr = |label: &str| crate::i18n::tr(lang, label).to_owned();
    vec![
        MenuEntry::item(tr("Create Alternate Layout…"), Act::CreateAlternate(k)),
        MenuEntry::item(tr("Duplicate Alternate Layout"), Act::DuplicateAlternate(k)),
        MenuEntry::item(tr("Alternate Layout Options…"), Act::AlternateOptions(k)),
        MenuEntry::item(tr("Export Layout…"), Act::ExportLayout(k)),
        MenuEntry::item(tr("Delete Alternate Layout"), Act::DeleteAlternate(k)).enabled(count > 1),
    ]
}

/// The parent commands. `target` set: each acts on that parent (named in the entry when `named`); unset: each is a
/// submenu of the parents.
fn parent_entries(lang: &str, doc: &designcraft_doc::Document, target: Option<SpreadId>, named: bool) -> Vec<MenuEntry> {
    let tr = |label: &str| crate::i18n::tr(lang, label).to_owned();
    let label_of = |id: SpreadId| doc.parent_index(id).and_then(|i| doc.parents.get(i)).and_then(|p| p.parent.as_ref()).map(|i| i.label());
    let mut entries = vec![MenuEntry::item(tr("New Parent…"), Act::NewParent)];
    for action in ParentAction::ALL {
        let label = tr(action.label());
        entries.push(match target {
            Some(id) => {
                let shown = match label_of(id) {
                    Some(name) if named => format!("{} “{name}”", label.trim_end_matches('…')) + if label.ends_with('…') { "…" } else { "" },
                    _ => label,
                };
                MenuEntry::item(shown, Act::Parent(action, id))
            }
            None => MenuEntry::Sub {
                label: label.trim_end_matches('…').to_owned(),
                enabled: !doc.parents.is_empty(),
                items: doc
                    .parents
                    .iter()
                    .filter_map(|p| p.parent.as_ref().map(|info| MenuEntry::item(info.label(), Act::Parent(action, p.id))))
                    .collect(),
            },
        });
    }
    entries
}

/// The entries of the menu of page `abs` in spread `si`, in InDesign's order: pages, parents, layouts and numbering,
/// shuffling.
fn page_entries(app: &DesignApp, doc: &designcraft_doc::Document, layouts: &[designcraft_doc::LayoutRange], abs: usize, si: usize) -> Vec<MenuEntry> {
    let lang = app.ui.language.as_str();
    let tr = |label: &str| crate::i18n::tr(lang, label).to_owned();
    let pages_in_spread = doc.spreads.get(si).map_or(1, |sp| sp.pages.len());
    let mut entries = vec![
        MenuEntry::item(tr("Insert Pages…"), Act::InsertPages(abs)),
        MenuEntry::item(tr("Move Pages…"), Act::MovePages(abs)),
        MenuEntry::item(tr("Duplicate Spread"), Act::DuplicateSpread { si, at_end: false }),
        MenuEntry::item(tr("Duplicate Spread at End of Document"), Act::DuplicateSpread { si, at_end: true }),
        MenuEntry::item(tr("Delete Page"), Act::DeletePages(vec![abs])),
    ];
    if pages_in_spread > 1 {
        let first = doc.first_page_of_spread(si);
        entries.push(MenuEntry::item(tr("Delete Spread"), Act::DeletePages((first..first.saturating_add(pages_in_spread)).collect())));
    }
    entries.push(MenuEntry::Sep);
    entries.extend(parent_entries(lang, doc, target_parent(app, doc), true));
    let mut apply = vec![MenuEntry::item(tr("[None]"), Act::ApplyParent { page: abs, parent: None })];
    for pp in &doc.parents {
        let label = pp.parent.as_ref().map(|i| i.label()).unwrap_or_default();
        apply.push(MenuEntry::item(label, Act::ApplyParent { page: abs, parent: Some(pp.id) }));
    }
    entries.push(MenuEntry::Sub { label: tr("Apply Parent to Pages"), enabled: true, items: apply });
    entries.push(MenuEntry::item(tr("Override All Parent Page Items"), Act::PageCommand { id: "layout.overrideParentItems", page: abs }));
    entries.push(MenuEntry::Sub {
        label: tr("Parent Pages"),
        enabled: true,
        items: [("layout.removeOverrides", "Remove All Local Overrides"), ("layout.detachAll", "Detach All Objects from Parent")]
            .into_iter()
            .map(|(id, label)| MenuEntry::item(tr(label), Act::PageCommand { id, page: abs }))
            .collect(),
    });
    entries.push(MenuEntry::Sep);
    entries.extend(layout_entries(lang, layouts.len(), layout_index(layouts, abs)));
    entries.push(MenuEntry::item(tr("Numbering & Section Options…"), Act::Section(abs)));
    entries.push(MenuEntry::Sep);
    let shuffle = doc.spreads.get(si).is_none_or(|sp| sp.allow_shuffle);
    entries.push(MenuEntry::Item {
        label: tr("Allow Selected Spread to Shuffle"),
        enabled: true,
        check: Some(shuffle),
        hint: None,
        act: Act::Shuffle { si, allow: !shuffle },
    });
    // Not in InDesign's menu (it drags pages into spreads): kept for spreads of up to 10 pages.
    if si > 0 {
        entries.push(MenuEntry::Item {
            label: tr("Add to Previous Spread"),
            enabled: true,
            check: None,
            hint: Some(tr("Spreads of up to 10 pages")),
            act: Act::ToPreviousSpread { page: abs, si },
        });
    }
    entries.push(MenuEntry::Sep);
    entries.push(MenuEntry::item(tr("Panel Options…"), Act::PanelOptions));
    entries
}

mod framework;

#[cfg(test)]
#[cfg(not(target_arch = "wasm32"))]
mod successor_tests;
#[cfg(test)]
mod tests;

#[cfg(test)]
mod spread_label_tests {
    use serde_json::json;

    use super::*;

    /// Create a facing-page document for section navigation tests.
    fn app() -> DesignApp {
        let mut app = DesignApp::new(designcraft_engine::Session::new(), crate::Services::default());
        app.run("file.new", json!({"facingPages": true, "pages": 9})).unwrap();
        app
    }

    /// Read the label displayed for each document spread.
    fn labels(app: &DesignApp) -> Vec<String> {
        let doc = app.session.doc().unwrap().doc.clone();
        (0..doc.spreads.len()).map(|si| spread_label(app, &doc, doc.first_page_of_spread(si), doc.spreads[si].pages.len())).collect()
    }

    /// Section boundaries split a spread label instead of implying continuous numbering.
    #[test]
    fn spreads_are_labelled_as_indesign_labels_them() {
        // Chris Nielsen's "Numbering Page Sections": sections starting at 1 on pages 3 and 7,
        // the second in letters, label their spreads "2, 1" and "4, A".
        let mut app = app();
        assert_eq!(labels(&app), ["1", "2–3", "4–5", "6–7", "8–9"]);
        app.run("layout.section", json!({"page": 3, "startNumber": 1})).unwrap();
        app.run("layout.section", json!({"page": 7, "startNumber": 1, "style": "upperLetters"})).unwrap();
        assert_eq!(labels(&app), ["1", "2, 1", "2–3", "4, A", "B–C"]);
        // Absolute numbering: positions, always a range.
        app.session.prefs.absolute_page_numbers = true;
        assert_eq!(labels(&app), ["1", "2–3", "4–5", "6–7", "8–9"]);
    }

    /// Bound right to left, a spread's label follows the document's reading order: the left
    /// page first ("3–2", not "2–3"), and a section inside a spread lists its pages left to
    /// right. Left-to-right labels are unchanged. A spread of more pages, drawn in document
    /// order, lists its names in that order, apart while a section still starts inside it.
    #[test]
    fn rtl_binding_labels_spreads_in_visual_left_to_right_order() {
        let mut app = app();
        assert_eq!(labels(&app), ["1", "2–3", "4–5", "6–7", "8–9"]);
        app.run("layout.documentSetup", json!({"binding": "rightToLeft"})).unwrap();
        assert_eq!(labels(&app), ["1", "3–2", "5–4", "7–6", "9–8"]);
        app.run("layout.section", json!({"page": 3, "startNumber": 1})).unwrap();
        let got = labels(&app);
        assert_eq!(got[1], "1, 2");
        assert_eq!(got[2], "3–2");
        app.run("layout.pages.toSpread", json!({"page": 4, "spread": 1})).unwrap();
        // Three pages, drawn in document order: 2, 3 and 4 are numbered 2, 1 and 2, and the
        // section that starts on page 3 stays inside the spread, so the names stay apart.
        assert_eq!(labels(&app)[1], "2, 1, 2");
    }
}

#[cfg(test)]
mod pages_footer_workflow_tests {
    use super::*;
    use craft_ui_widgets::runtime::inspect::{Element, Recorded, record};
    use egui::vec2;
    use egui::{Event, Key, Modifiers, PointerButton, UiBuilder};
    use egui_kittest::{
        Harness,
        kittest::{NodeT, Queryable},
    };
    use std::{collections::HashSet, sync::Arc};

    const ACTIONS: [&str; 3] = ["Delete selected pages", "Create new page", "Edit page size"];
    struct Rig {
        app: DesignApp,
        controls: Option<Arc<Recorded>>,
        enabled: bool,
        sizing: bool,
        full_panel: bool,
    }
    type Mark = (Arc<designcraft_doc::Document>, String, (usize, usize));
    fn step(h: &mut Harness<'_, Rig>) {
        let mut raw = std::mem::take(h.input_mut());
        let ctx = h.ctx.clone();
        h.state_mut().app.raw_input_hook(&ctx, &mut raw);
        *h.input_mut() = raw;
        h.step();
    }
    fn idle(h: &mut Harness<'_, Rig>, frames: usize) {
        for _ in 0..frames {
            step(h);
        }
    }
    fn event(h: &mut Harness<'_, Rig>, event: Event) {
        h.input_mut().events.push(event);
        step(h);
    }
    fn key(h: &mut Harness<'_, Rig>, key: Key) {
        for pressed in [true, false] {
            event(h, Event::Key { key, physical_key: None, pressed, repeat: false, modifiers: Modifiers::NONE });
        }
    }
    fn pointer(h: &mut Harness<'_, Rig>, pos: egui::Pos2, pressed: bool) {
        event(h, Event::PointerMoved(pos));
        event(h, Event::PointerButton { pos, button: PointerButton::Primary, pressed, modifiers: Modifiers::NONE });
    }
    /// Genuine already-installed native face, using the protected current47 Interactive fixture contract.
    /// This is a strict test precondition; it does not change the production/default font input.
    fn install_arabic_fixture(ctx: &egui::Context, language: &str) {
        #[cfg(target_os = "macos")]
        let family = "Geeza Pro";
        #[cfg(target_os = "windows")]
        let family = "Segoe UI";
        #[cfg(not(any(target_os = "macos", target_os = "windows")))]
        let family = "DejaVu Sans";
        // The regular face's style name: DejaVu Sans calls it Book.
        #[cfg(any(target_os = "macos", target_os = "windows"))]
        let regular = "Regular";
        #[cfg(not(any(target_os = "macos", target_os = "windows")))]
        let regular = "Book";
        let db = designcraft_fonts::FontDb::global();
        assert!(db.has_family(family), "the native Arabic fixture requires the already-installed {family}; do not skip Arabic or substitute tofu");
        let face = db.face(family, regular);
        assert!(face.family.eq_ignore_ascii_case(family), "the named fixture must resolve to {family}, not {}", face.family);
        assert!(
            !face.is_placeholder() && face.covers('\u{0627}') && face.covers('\u{0625}') && face.covers('\u{0637}') && face.covers('\u{0639}'),
            "the real existing face must contain U+0627/U+0625 and both actual failed U+0637/U+0639 glyphs"
        );
        #[cfg(target_os = "macos")]
        assert_eq!(face.index(), 0, "the observed existing Geeza Pro Regular fixture is TTC face0");
        assert!(face.style.eq_ignore_ascii_case(regular), "the genuine fixture requires the requested regular face");
        for key in ACTIONS.into_iter().chain(crate::panels::properties::PAGE_PRESETS.iter().map(|preset| preset.0)).chain(["Custom Page Size…"]) {
            for c in crate::i18n::tr(language, key).chars().filter(|c| designcraft_fonts::is_rtl(*c)) {
                assert!(face.covers(c), "{family} lacks the real glyph U+{:04X} from {key}", u32::from(c));
            }
        }
        let mut definitions = ctx.fonts(|fonts| fonts.definitions().clone());
        let fallback = crate::theme::SystemFont::new("Arab", face.family.clone(), face.data().to_vec(), face.index());
        crate::theme::add_system_fallbacks(&mut definitions, &[fallback], language);
        ctx.set_fonts(definitions);
        eprintln!(
            "Pages footer Arabic fixture: real existing {} {}, face {}, {} font bytes, full original Pages footer Arabic-string glyph coverage checked",
            face.family,
            face.style,
            face.index(),
            face.data().len()
        );
    }

    fn harness(mut app: DesignApp, full_panel: bool) -> Harness<'static, Rig> {
        let ctx = egui::Context::default();
        app.install_framework(&ctx);
        app.raw_input_hook(&ctx, &mut egui::RawInput::default());
        let mut ready = false;
        let mut locale_fonts_ready = false;
        let mut h = Harness::builder().with_context(ctx).with_size(vec2(680.0, 640.0)).build_ui_state(
            move |ui, state: &mut Rig| {
                if !ready {
                    crate::theme::install_fonts(ui.ctx(), &state.app.ui.language);
                    ready = true;
                    return;
                }
                if !locale_fonts_ready {
                    locale_fonts_ready = true;
                    if state.app.ui.language == "ar" {
                        install_arabic_fixture(ui.ctx(), &state.app.ui.language);
                        return;
                    }
                }
                let builder = if state.sizing { UiBuilder::new().sizing_pass() } else { UiBuilder::new() };
                ui.scope_builder(builder, |ui| {
                    ui.add_enabled_ui(state.enabled, |ui| {
                        state.controls = record(ui, egui::Id::unique("pages-footer-workflow"), |ui| {
                            if state.full_panel {
                                show(&mut state.app, ui);
                                return;
                            }
                            let Some(st) = state.app.session.active() else { return };
                            let doc = st.doc.clone();
                            let cur = crate::canvas::current_page(&state.app).unwrap_or(0);
                            framework::footer(&mut state.app, ui, &doc, cur);
                        })
                        .1;
                    })
                });
            },
            Rig { app, controls: None, enabled: true, sizing: false, full_panel },
        );
        h.input_mut().max_texture_side = Some(8192);
        idle(&mut h, 4);
        h
    }
    fn fixture() -> DesignApp {
        let mut session = designcraft_engine::Session::new();
        session.execute("file.new", &json!({"pages":3,"facingPages":true})).unwrap();
        session.execute("layout.pageSize", &json!({"pages":[1],"width":605,"height":805})).unwrap();
        session.execute("layout.pageSize", &json!({"pages":[1],"width":615,"height":815})).unwrap();
        session.execute("edit.undo", &json!({})).unwrap();
        assert!(!session.active().unwrap().history.undo.is_empty());
        assert_eq!(session.active().unwrap().history.redo.len(), 1);
        let mut app = DesignApp::new_for_panel_tests(session, crate::Services::default());
        app.canvas_rect = Some(egui::Rect::from_min_size(egui::Pos2::ZERO, vec2(1000.0, 800.0)));
        crate::canvas::go_to_page(&mut app, 0);
        assert_eq!(crate::canvas::current_page(&app), Some(0));
        app
    }
    fn mark(h: &Harness<'_, Rig>) -> Mark {
        let st = h.state().app.session.active().unwrap();
        (
            st.doc.clone(),
            format!("{:?}", (st.uid, st.identity_epoch, &st.selection.items, &st.selection.text, &st.selection.cells)),
            (st.history.undo.len(), st.history.redo.len()),
        )
    }
    fn unchanged(h: &Harness<'_, Rig>, before: &Mark) {
        let now = mark(h);
        assert!(Arc::ptr_eq(&now.0, &before.0), "rendering/refused input does not COW the document");
        assert_eq!((now.1, now.2), (before.1.clone(), before.2));
    }
    fn name(h: &Harness<'_, Rig>, caption: &str) -> String {
        crate::i18n::tr(&h.state().app.ui.language, caption).to_owned()
    }
    fn element(h: &Harness<'_, Rig>, caption: &str) -> Element {
        let label = name(h, caption);
        let found: Vec<_> = h
            .state()
            .controls
            .as_ref()
            .unwrap()
            .elements
            .iter()
            .filter(|e| {
                e.component == "IconButton" && e.part.is_none() && e.facts.as_ref().is_some_and(|f| f.name.as_deref() == Some(label.as_str()))
            })
            .cloned()
            .collect();
        assert_eq!(found.len(), 1, "one actual named footer IconButton: {label}");
        let field = found.into_iter().next().unwrap();
        assert_eq!(field.facts.as_ref().unwrap().role, format!("{:?}", egui::accesskit::Role::Button));
        let rect = h.get_by(|n| n.role() == egui::accesskit::Role::Button && n.label().as_deref() == Some(label.as_str())).rect();
        assert!(rect.width() > 0.0 && rect.height() > 0.0 && field.rect.contains(rect.center()), "real AX hit target");
        assert!(field.rect.height() <= FOOTER_H, "footer controls fit the unchanged footer allocation");
        field
    }
    fn pos(h: &Harness<'_, Rig>, caption: &str) -> egui::Pos2 {
        let label = name(h, caption);
        h.get_by(|n| n.role() == egui::accesskit::Role::Button && n.label().as_deref() == Some(label.as_str())).rect().center()
    }
    fn click(h: &mut Harness<'_, Rig>, caption: &str) {
        element(h, caption);
        let p = pos(h, caption);
        pointer(h, p, true);
        pointer(h, p, false);
        idle(h, 2);
    }
    fn native_click(h: &mut Harness<'_, Rig>, caption: &str) {
        let label = name(h, caption);
        let p = h.get_by_label(&label).rect().center();
        pointer(h, p, true);
        pointer(h, p, false);
        idle(h, 2);
    }
    fn tab_to(h: &mut Harness<'_, Rig>, caption: &str) {
        let field = element(h, caption);
        let label = name(h, caption);
        let mut seen = HashSet::new();
        loop {
            let focused = {
                let current = element(h, caption);
                assert_eq!(current.id, field.id, "the actual target IconButton remains current");
                assert!(!current.facts.as_ref().unwrap().disabled, "the current IconButton target must be enabled");
                let control = h.get_by(|n| n.role() == egui::accesskit::Role::Button && n.label().as_deref() == Some(label.as_str()));
                let access = control.accesskit_node();
                assert!(!access.is_disabled(), "the actual named Button AX member must be enabled");
                access.is_focused()
            };
            if focused {
                return;
            }
            // Native focus can belong to an arena root; cycle identities must be actual AX members.
            let focused_members: Vec<_> = {
                let root = h.queryable_node();
                std::iter::once(root.clone())
                    .chain(root.children_recursive())
                    .filter_map(|node| {
                        let access = node.accesskit_node();
                        access.is_focused().then(|| format!("{:?}", access.id()))
                    })
                    .collect()
            };
            assert!(seen.insert(focused_members.clone()), "natural Tab reaches {label}; repeated AX focus {focused_members:?}");
            key(h, Key::Tab);
        }
    }
    fn journal_one(h: &Harness<'_, Rig>, start: usize, cmd: &str, params: serde_json::Value) {
        assert_eq!(&h.state().app.session.journal[start..], &[(cmd.to_owned(), params)]);
    }
    fn count(h: &Harness<'_, Rig>) -> usize {
        h.state().app.session.active().unwrap().doc.page_count()
    }
    fn sizes(h: &Harness<'_, Rig>) -> Vec<(f64, f64)> {
        h.state().app.session.active().unwrap().doc.spreads.iter().flat_map(|sp| sp.pages.iter().map(|p| (p.width, p.height))).collect()
    }
    fn preset() -> (f64, f64) {
        let p = crate::panels::properties::PAGE_PRESETS.iter().find(|p| p.0 == "A4").unwrap();
        (p.1, p.2)
    }

    #[test]
    fn installed_pages_footer_idle_keeps_model_identity_nonempty_undo_redo_and_three_ids() {
        let mut h = harness(fixture(), false);
        let before = mark(&h);
        let start = h.state().app.session.journal.len();
        let ids: Vec<_> = ACTIONS.iter().map(|c| element(&h, c).id).collect();
        assert_eq!(ids.iter().copied().collect::<HashSet<_>>().len(), 3);
        idle(&mut h, 8);
        unchanged(&h, &before);
        assert_eq!(h.state().app.session.journal.len(), start);
        assert_eq!(ids, ACTIONS.iter().map(|c| element(&h, c).id).collect::<Vec<_>>());
    }
    #[test]
    fn installed_pages_footer_create_pointer_has_exact_payload_one_undo_and_real_redo() {
        let mut h = harness(fixture(), false);
        let before = mark(&h);
        let start = h.state().app.session.journal.len();
        let cur = crate::canvas::current_page(&h.state().app).unwrap();
        click(&mut h, "Create new page");
        journal_one(&h, start, "layout.pages.insert", json!({"after":cur,"count":1}));
        assert_eq!(count(&h), 4);
        assert_eq!(mark(&h).2, (before.2.0 + 1, 0));
        let after = mark(&h).0;
        h.state_mut().app.run("edit.undo", json!({})).unwrap();
        idle(&mut h, 2);
        assert_eq!(mark(&h).0.as_ref(), before.0.as_ref());
        assert_eq!(mark(&h).2, (before.2.0, 1));
        h.state_mut().app.run("edit.redo", json!({})).unwrap();
        idle(&mut h, 2);
        assert_eq!(mark(&h).0.as_ref(), after.as_ref());
        assert_eq!(mark(&h).2, (before.2.0 + 1, 0));
    }
    #[test]
    fn installed_pages_footer_natural_tab_space_and_enter_activate_once() {
        let mut h = harness(fixture(), false);
        for caption in ACTIONS {
            let before = mark(&h);
            tab_to(&mut h, caption);
            unchanged(&h, &before);
        }
        for k in [Key::Space, Key::Enter] {
            tab_to(&mut h, "Create new page");
            let cur = crate::canvas::current_page(&h.state().app).unwrap();
            let old = count(&h);
            let start = h.state().app.session.journal.len();
            key(&mut h, k);
            idle(&mut h, 3);
            journal_one(&h, start, "layout.pages.insert", json!({"after":cur,"count":1}));
            assert_eq!(count(&h), old + 1);
        }
    }
    #[test]
    fn installed_pages_footer_delete_empty_current_page_has_real_undo_redo() {
        let mut h = harness(fixture(), false);
        crate::canvas::go_to_page(&mut h.state_mut().app, 1);
        idle(&mut h, 2);
        assert_eq!(crate::canvas::current_page(&h.state().app), Some(1));
        let before = mark(&h);
        let start = h.state().app.session.journal.len();
        click(&mut h, "Delete selected pages");
        journal_one(&h, start, "layout.pages.delete", json!({"pages":[1]}));
        assert_eq!(count(&h), 2);
        assert!(h.state().app.ui.dialog.is_none());
        assert_eq!(mark(&h).2, (before.2.0 + 1, 0));
        let after = mark(&h).0;
        h.state_mut().app.run("edit.undo", json!({})).unwrap();
        idle(&mut h, 2);
        assert_eq!(mark(&h).0.as_ref(), before.0.as_ref());
        h.state_mut().app.run("edit.redo", json!({})).unwrap();
        idle(&mut h, 2);
        assert_eq!(mark(&h).0.as_ref(), after.as_ref());
    }
    #[test]
    fn installed_pages_footer_last_page_delete_keeps_real_engine_refusal_and_history() {
        let mut app = fixture();
        app.run("layout.pages.delete", json!({"pages":[1,2]})).unwrap();
        crate::canvas::go_to_page(&mut app, 0);
        let mut h = harness(app, false);
        let before = mark(&h);
        let start = h.state().app.session.journal.len();
        click(&mut h, "Delete selected pages");
        assert_eq!(count(&h), 1);
        assert_eq!(mark(&h).0.as_ref(), before.0.as_ref());
        assert_eq!((mark(&h).1, mark(&h).2), (before.1.clone(), before.2));
        assert_eq!(h.state().app.session.journal.len(), start);
        let error = h.state_mut().app.run("layout.pages.delete", json!({"pages":[0]})).unwrap_err();
        assert!(error.contains("at least one page"), "{error}");
        assert_eq!(mark(&h).0.as_ref(), before.0.as_ref());
        assert_eq!(mark(&h).2, before.2);
    }

    #[test]
    fn installed_pages_footer_populated_delete_opens_original_warning_without_mutation() {
        let mut app = fixture();
        let item = app.run("frame.create", json!({"rect":[10,10,40,40]})).unwrap()["id"].as_u64().unwrap();
        assert!(app.session.active().unwrap().doc.page_has_items(0));
        let mut h = harness(app, false);
        let before = mark(&h);
        let start = h.state().app.session.journal.len();
        click(&mut h, "Delete selected pages");
        let dialog = h.state().app.ui.dialog.as_ref().unwrap();
        assert_eq!(dialog.id, "deletePages");
        assert_eq!(dialog.fields.get("pages"), Some(&json!([0])));
        unchanged(&h, &before);
        assert_eq!(h.state().app.session.journal.len(), start);
        crate::dialogs::cancel(&mut h.state_mut().app);
        idle(&mut h, 2);
        unchanged(&h, &before);
        assert!(h.state().app.ui.dialog.is_none());
        assert!(h.state().app.session.active().unwrap().doc.item(designcraft_doc::ItemId(item)).is_some());
    }
    #[test]
    fn installed_pages_footer_size_popup_uses_current_one_based_page_and_keeps_landscape() {
        let mut app = fixture();
        app.run("layout.pageSize", json!({"pages":[2],"width":900,"height":600})).unwrap();
        crate::canvas::go_to_page(&mut app, 1);
        let mut h = harness(app, false);
        let before = mark(&h);
        let old_sizes = sizes(&h);
        let start = h.state().app.session.journal.len();
        click(&mut h, "Edit page size");
        unchanged(&h, &before);
        assert_eq!(h.state().app.session.journal.len(), start);
        native_click(&mut h, "A4");
        let (w, hh) = preset();
        journal_one(&h, start, "layout.pageSize", json!({"pages":[2],"width":hh,"height":w}));
        assert_eq!(sizes(&h), vec![old_sizes[0], (hh, w), old_sizes[2]]);
        assert_eq!(mark(&h).2, (before.2.0 + 1, 0));
        h.state_mut().app.run("edit.undo", json!({})).unwrap();
        idle(&mut h, 2);
        assert_eq!(mark(&h).0.as_ref(), before.0.as_ref());
        h.state_mut().app.run("edit.redo", json!({})).unwrap();
        idle(&mut h, 2);
        assert_eq!(sizes(&h)[1], (hh, w));
    }
    #[test]
    fn installed_pages_footer_size_menu_escape_and_custom_boundary_keep_history() {
        let mut h = harness(fixture(), false);
        let before = mark(&h);
        let start = h.state().app.session.journal.len();
        tab_to(&mut h, "Edit page size");
        key(&mut h, Key::Enter);
        idle(&mut h, 2);
        assert_eq!(h.get_all_by_label("A4").count(), 1);
        key(&mut h, Key::Escape);
        idle(&mut h, 2);
        assert_eq!(h.query_all_by_label("A4").count(), 0);
        unchanged(&h, &before);
        click(&mut h, "Edit page size");
        native_click(&mut h, "Custom Page Size…");
        let dialog = h.state().app.ui.dialog.as_ref().unwrap();
        assert_eq!(dialog.id, "cmd:layout.pageSize");
        assert_eq!(dialog.fields.get("pages"), Some(&json!([1])));
        assert_eq!(dialog.fields.get("width"), Some(&json!(605.0)));
        assert_eq!(dialog.fields.get("height"), Some(&json!(805.0)));
        unchanged(&h, &before);
        assert_eq!(h.state().app.session.journal.len(), start);
    }
    #[test]
    fn installed_pages_footer_real_thumbnail_drag_duplicates_the_whole_spread_not_an_insert() {
        let mut h = harness(fixture(), true);
        let before = mark(&h);
        let label = format!("Page {}", h.state().app.session.page_label(1));
        let from = h.get_by_label(&label).rect().center();
        let to = pos(&h, "Create new page");
        let start = h.state().app.session.journal.len();
        pointer(&mut h, from, true);
        event(&mut h, Event::PointerMoved(from + vec2(10.0, 0.0)));
        idle(&mut h, 2);
        assert!(
            matches!(craft_ui_widgets::runtime::drag::active::<PageDrag>(&h.ctx).as_deref(), Some(PageDrag::Page(1))),
            "actual app thumbnail drag source"
        );
        for n in 1..=5 {
            event(&mut h, Event::PointerMoved(from + (to - from) * (n as f32 / 5.0)));
        }
        pointer(&mut h, to, false);
        idle(&mut h, 3);
        journal_one(&h, start, "layout.pages.duplicateSpread", json!({"spread":1,"atEnd":true}));
        assert_eq!(count(&h), 5);
        assert_eq!(mark(&h).2, (before.2.0 + 1, 0));
        h.state_mut().app.run("edit.undo", json!({})).unwrap();
        idle(&mut h, 2);
        assert_eq!(mark(&h).0.as_ref(), before.0.as_ref());
    }
    #[test]
    fn installed_pages_footer_held_press_retires_on_current_page_or_document_revision_change() {
        let mut h = harness(fixture(), false);
        for navigation in [true, false] {
            let old = element(&h, "Create new page").id;
            let p = pos(&h, "Create new page");
            pointer(&mut h, p, true);
            if navigation {
                crate::canvas::go_to_page(&mut h.state_mut().app, 1);
            } else {
                let next = {
                    let st = h.state().app.session.active().unwrap();
                    let (si, pi) = st.doc.page_loc(1).unwrap();
                    !st.doc.spreads[si].pages[pi].show_parent_overlay
                };
                h.state_mut().app.run("layout.pageOptions", json!({"pages":[2],"showParentOverlay":next})).unwrap();
            }
            idle(&mut h, 2);
            assert_ne!(element(&h, "Create new page").id, old);
            let before = mark(&h);
            let start = h.state().app.session.journal.len();
            pointer(&mut h, p, false);
            idle(&mut h, 2);
            unchanged(&h, &before);
            assert_eq!(h.state().app.session.journal.len(), start);
        }
        let cur = crate::canvas::current_page(&h.state().app).unwrap();
        let start = h.state().app.session.journal.len();
        click(&mut h, "Create new page");
        journal_one(&h, start, "layout.pages.insert", json!({"after":cur,"count":1}));
    }
    #[test]
    fn installed_pages_footer_held_press_cannot_cross_documents_with_the_same_local_page_index() {
        let mut h = harness(fixture(), false);
        let original = mark(&h);
        let old = element(&h, "Create new page").id;
        let p = pos(&h, "Create new page");
        pointer(&mut h, p, true);
        h.state_mut().app.run("file.new", json!({"pages":3,"facingPages":true})).unwrap();
        let dir = tempfile::tempdir().unwrap();
        h.state_mut().app.run("file.saveAs", json!({"path":dir.path().join("Other.designcraft")})).unwrap();
        crate::canvas::go_to_page(&mut h.state_mut().app, 0);
        idle(&mut h, 2);
        assert_eq!(crate::canvas::current_page(&h.state().app), Some(0));
        assert_ne!(element(&h, "Create new page").id, old);
        let before = mark(&h);
        let start = h.state().app.session.journal.len();
        pointer(&mut h, p, false);
        idle(&mut h, 2);
        unchanged(&h, &before);
        assert_eq!(h.state().app.session.journal.len(), start);
        h.state_mut().app.run("file.close", json!({})).unwrap();
        idle(&mut h, 2);
        unchanged(&h, &original);
    }
    #[test]
    fn installed_pages_footer_disabled_sizing_and_no_document_cannot_commit() {
        let mut h = harness(fixture(), false);
        let before = mark(&h);
        let start = h.state().app.session.journal.len();
        let p = pos(&h, "Create new page");
        pointer(&mut h, p, true);
        h.state_mut().enabled = false;
        idle(&mut h, 2);
        for caption in ACTIONS {
            assert!(element(&h, caption).facts.as_ref().unwrap().disabled);
            let label = name(&h, caption);
            assert!(h.get_by_label(&label).accesskit_node().is_disabled());
        }
        pointer(&mut h, p, false);
        key(&mut h, Key::Space);
        key(&mut h, Key::Enter);
        unchanged(&h, &before);
        h.state_mut().enabled = true;
        h.state_mut().sizing = true;
        idle(&mut h, 2);
        pointer(&mut h, p, true);
        pointer(&mut h, p, false);
        key(&mut h, Key::Space);
        unchanged(&h, &before);
        assert_eq!(h.state().app.session.journal.len(), start);
        h.state_mut().sizing = false;
        idle(&mut h, 2);
        click(&mut h, "Create new page");
        assert_eq!(count(&h), 4);
        let app = DesignApp::new_for_panel_tests(designcraft_engine::Session::new(), crate::Services::default());
        let mut empty = harness(app, false);
        let start = empty.state().app.session.journal.len();
        assert_eq!(empty.query_all_by_role(egui::accesskit::Role::Button).count(), 0);
        key(&mut empty, Key::Tab);
        key(&mut empty, Key::Space);
        key(&mut empty, Key::Enter);
        idle(&mut empty, 3);
        assert!(empty.state().app.session.active().is_none());
        assert_eq!(empty.state().app.session.journal.len(), start);
        assert!(empty.state_mut().app.run("layout.pages.insert", json!({"after":0,"count":1})).is_err());
        assert!(empty.state().app.session.active().is_none());
    }
    #[test]
    fn installed_pages_footer_german_arabic_names_input_and_real_save_reopen() {
        for language in ["de", "ar"] {
            let mut app = fixture();
            app.ui.language = language.to_owned();
            let mut h = harness(app, false);
            let before = mark(&h);
            let mut ids = HashSet::new();
            for caption in ACTIONS {
                assert!(ids.insert(element(&h, caption).id));
            }
            unchanged(&h, &before);
            tab_to(&mut h, "Create new page");
            key(&mut h, Key::Space);
            idle(&mut h, 2);
            assert_eq!(count(&h), 4);
            click(&mut h, "Edit page size");
            native_click(&mut h, "A4");
            let dir = tempfile::tempdir().unwrap();
            let path = dir.path().join("صفحات & Seiten.designcraft");
            h.state_mut().app.run("file.saveAs", json!({"path":path})).unwrap();
            let expected = sizes(&h);
            let saved = mark(&h).0;
            h.state_mut().app.run("file.open", json!({"path":path})).unwrap();
            idle(&mut h, 3);
            assert_eq!(sizes(&h), expected);
            assert_eq!(count(&h), 4);
            assert_eq!(h.state().app.session.active().unwrap().doc.spreads, saved.spreads);
            assert!(path.is_file());
            for caption in ACTIONS {
                element(&h, caption);
            }
        }
    }
}
