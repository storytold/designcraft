//! Pages panel: parents at the top, document spreads with live thumbnails below.

use std::collections::HashMap;
use std::sync::Mutex;

use designcraft_doc::{SpreadId, SpreadRef};
use egui::{Color32, Sense, Stroke, StrokeKind, vec2};
use serde_json::json;

use crate::theme::Tokens;
use crate::{DesignApp, icons};

/// What a thumbnail shows: a document page (absolute index) or a whole parent spread.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
enum ThumbOf {
    Page(usize),
    Parent(SpreadId),
}

/// (document uid, revision, page or parent, thumbnail height).
type ThumbKey = (u64, u64, ThumbOf, u32);

static THUMBS: Mutex<Option<HashMap<ThumbKey, egui::TextureHandle>>> = Mutex::new(None);

/// Widest parent thumbnail, in pixels (a spread of many pages stays a small texture).
const MAX_THUMB_W: f64 = 1024.0;

fn thumb(app: &mut DesignApp, ctx: &egui::Context, of: ThumbOf, h: f32) -> Option<egui::TextureHandle> {
    let st = app.session.active()?;
    let key = (st.uid, st.revision, of, h.round() as u32);
    let mut g = THUMBS.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
    let map = g.get_or_insert_with(HashMap::new);
    if let Some(t) = map.get(&key) {
        return Some(t.clone());
    }
    let px = h as f64 * ctx.pixels_per_point() as f64;
    let mut r = designcraft_render::Renderer::new();
    r.threads = 0;
    let opts = designcraft_render::RenderOptions { printing_only: true, ..Default::default() };
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
    // Drop thumbnails of older revisions.
    let (uid, rev) = (st.uid, st.revision);
    map.retain(|k, _| !(k.0 == uid && k.1 != rev));
    map.insert(key, tex.clone());
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

/// A spread's label under its thumbnails, as InDesign's: one name for a single page, "2–3" for
/// pages numbered on in one section (an en dash), and the names apart ("2, 1", "8, A") when a section starts
/// inside the spread.
pub(crate) fn spread_label(app: &DesignApp, doc: &designcraft_doc::Document, first: usize, n: usize) -> String {
    let names: Vec<String> = (first..first + n).map(|abs| app.session.page_label(abs)).collect();
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

/// Paint a parent's icon `h` high, right-aligned at `right`: its rendered spread (when
/// `thumbnails`), or blank pages for [None] and when the spread can't be rendered.
fn parent_icon(app: &mut DesignApp, ui: &egui::Ui, right: egui::Pos2, row: &ParentRow, h: f32, thumbnails: bool, t: &Tokens) {
    let area = egui::Rect::from_min_size(egui::pos2(right.x - row.unit_w * h, right.y - h / 2.0), vec2(row.unit_w * h, h));
    let tex = if thumbnails { row.parent.and_then(|(_, id)| thumb(app, ui.ctx(), ThumbOf::Parent(id), h)) } else { None };
    let p = ui.painter();
    match tex {
        Some(tex) => {
            p.image(tex.id(), area, egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0)), Color32::WHITE);
        }
        None => {
            p.rect_filled(area, 0.0, Color32::WHITE);
        }
    }
    for (x, w) in &row.pages {
        let r = egui::Rect::from_min_size(egui::pos2(area.min.x + x * h, area.min.y), vec2(w * h, h));
        p.rect_stroke(r, 0.0, Stroke::new(0.5, t.border), StrokeKind::Inside);
    }
}

pub fn show(app: &mut DesignApp, ui: &mut egui::Ui) {
    let t = Tokens::get(ui.ctx());
    let Some(st) = app.session.active() else { return };
    let uid = st.uid;
    let doc = st.doc.clone();
    let editing_parents = st.editing_parents;
    let aspect = (doc.settings.page_width / doc.settings.page_height.max(1.0)) as f32;
    let mut drop: Option<(PageDrag, usize)> = None;
    ui.spacing_mut().item_spacing.y = 0.0;
    // ---- Parents: [None] and each parent, rows with the parent's thumbnail at the right.
    // While parents are edited, the row of the parent the canvas shows is selected.
    let cur_parent = if editing_parents {
        let layout = designcraft_tools::CanvasLayout::new(&doc, true);
        crate::canvas::current_slot(app, &layout).and_then(|i| layout.slots.get(i)).and_then(|s| match s.spread {
            SpreadRef::Parent(i) => Some(i),
            SpreadRef::Doc(_) => None,
        })
    } else {
        None
    };
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
    // Parents side by side (Show Vertically off): tiles of the icon and the prefix, wrapping.
    let tile_w = |unit_w: f32| (icon_h * unit_w + 16.0).max(44.0);
    let natural = if opts.parents_vertical {
        rows.len() as f32 * row_h
    } else {
        let width = (ui.available_width() - 4.0).max(1.0);
        let (mut lines, mut x) = (1.0_f32, 0.0_f32);
        for row in &rows {
            let w = tile_w(row.unit_w) + 6.0;
            if x > 0.0 && x + w > width {
                lines += 1.0;
                x = 0.0;
            }
            x += w;
        }
        lines * (icon_h + 22.0)
    };
    let space = (total - FOOTER_H - DIVIDER_H).max(2.0 * ROW_H);
    let parents_max = (space - 2.0 * ROW_H).max(ROW_H);
    // A row taller than the room left (large icons in a short panel) gets that room.
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
    let parents_part = |app: &mut DesignApp, ui: &mut egui::Ui| {
        egui::ScrollArea::vertical().id_salt("pages_parents").max_height(parents_h).min_scrolled_height(parents_h).auto_shrink([false, true]).show(
            ui,
            |ui| {
                ui.spacing_mut().item_spacing = vec2(6.0, 0.0);
                let each = |app: &mut DesignApp, ui: &mut egui::Ui, row: &ParentRow| {
                    let label = row.label.as_str();
                    let (r, resp) = if opts.parents_vertical {
                        ui.allocate_exact_size(vec2(ui.available_width(), row_h), Sense::click_and_drag())
                    } else {
                        ui.allocate_exact_size(vec2(tile_w(row.unit_w), icon_h + 22.0), Sense::click_and_drag())
                    };
                    let active = row.parent.is_some_and(|(i, _)| cur_parent == Some(i));
                    if active {
                        ui.painter().rect_filled(r, 0.0, t.row_selected);
                    } else if resp.hovered() {
                        ui.painter().rect_filled(r, 0.0, t.hover);
                    }
                    if opts.parents_vertical {
                        ui.painter().text(
                            r.left_center() + vec2(3.0, 0.0),
                            egui::Align2::LEFT_CENTER,
                            label,
                            egui::FontId::proportional(11.5),
                            t.text_strong,
                        );
                        parent_icon(app, ui, egui::pos2(r.max.x - 10.0, r.center().y), row, icon_h, parent_thumbnails, &t);
                    } else {
                        let w = icon_h * row.unit_w;
                        parent_icon(app, ui, egui::pos2(r.center().x + w / 2.0, r.min.y + 3.0 + icon_h / 2.0), row, icon_h, parent_thumbnails, &t);
                        let short = row.prefix.clone().unwrap_or_else(|| label.to_string());
                        ui.painter().text(
                            egui::pos2(r.center().x, r.max.y - 9.0),
                            egui::Align2::CENTER_CENTER,
                            short,
                            egui::FontId::proportional(11.0),
                            t.text_strong,
                        );
                    }
                    resp.widget_info(|| egui::WidgetInfo::selected(egui::Role::Button, true, active, label));
                    let resp = if opts.parents_vertical { resp } else { resp.on_hover_text(label) };
                    resp.dnd_set_drag_payload(PageDrag::Parent(row.parent.map(|(_, id)| id)));
                    if resp.double_clicked()
                        && let Some((_, id)) = row.parent
                    {
                        let _ = app.run("layout.parents.edit", json!({"on": true, "parent": id.0}));
                    }
                    if resp.dragged() {
                        drag_ghost(ui, label, &t);
                    }
                    if let Some((_, id)) = row.parent {
                        resp.context_menu(|ui| {
                            let label = crate::i18n::tr(&app.ui.language, "Delete Parent");
                            if ui.button(crate::rtl::widget(ui, label)).clicked() {
                                if app.run("layout.parents.delete", json!({"id": id.0})).is_ok() {
                                    app.views.clear();
                                }
                                ui.close();
                            }
                        });
                    }
                };
                if opts.parents_vertical {
                    for row in &rows {
                        each(app, ui, row);
                    }
                } else {
                    ui.horizontal_wrapped(|ui| {
                        ui.spacing_mut().item_spacing = vec2(6.0, 0.0);
                        for row in &rows {
                            each(app, ui, row);
                        }
                    });
                }
            },
        );
    };
    // ---- Document spreads.
    let cur = crate::canvas::current_page(app).unwrap_or(0);
    let cur_spread = doc.page_loc(cur).map(|l| l.0);
    let followed_id = egui::Id::unique("pages_panel_followed");
    let dragging = egui::DragAndDrop::has_any_payload(ui.ctx());
    let tallest = doc.spreads.iter().flat_map(|sp| sp.pages.iter().map(|p| p.height)).fold(doc.settings.page_height, f64::max).max(1.0);
    let th = opts.page_size.page_h();
    // Extra Small has no room for a thumbnail; Show Thumbnails off draws blank pages.
    let thumbnails = opts.page_thumbnails && opts.page_size != IconSize::ExtraSmall;
    let mut pages_part = |app: &mut DesignApp, ui: &mut egui::Ui| {
        let center = ui.available_width() / 2.0;
        egui::ScrollArea::vertical().id_salt("pages_spreads").max_height(pages_h).min_scrolled_height(pages_h).auto_shrink([false, false]).show(
            ui,
            |ui| {
                ui.add_space(4.0);
                for (si, sp) in doc.spreads.iter().enumerate() {
                    let first = doc.first_page_of_spread(si);
                    // One scale for the whole document (pages of their own size show in proportion): the
                    // document's page size at the panel's size, unless a taller page would pass 1.8 times it.
                    // Each row is as tall as its spread's tallest page.
                    let scale = (th / doc.settings.page_height.max(1.0) as f32).min(1.8 * th / tallest as f32);
                    let row_h = sp.pages.iter().map(|p| p.height as f32 * scale).fold(0.0, f32::max).max(th * 0.25);
                    let spine = sp.spine_x() as f32;
                    let (row, _) = ui.allocate_exact_size(vec2(ui.available_width(), 8.0 + row_h + 22.0), Sense::hover());
                    let top = row.min.y + 8.0;
                    if cur_spread == Some(si) && !editing_parents {
                        let key = (uid, cur);
                        if ui.data(|d| d.get_temp::<(u64, usize)>(followed_id)) != Some(key) {
                            ui.scroll_to_rect(row, None);
                            ui.data_mut(|d| d.insert_temp(followed_id, key));
                        }
                    }
                    for (pi, p) in sp.pages.iter().enumerate() {
                        let abs = first + pi;
                        let x0 = row.min.x + center + (p.x as f32 - spine) * scale;
                        let pr = egui::Rect::from_min_size(egui::pos2(x0, top), vec2(p.width as f32 * scale, p.height as f32 * scale));
                        let resp = ui.interact(pr, ui.scope_id().with(("page", abs)), Sense::click_and_drag());
                        let selected = abs == cur && !editing_parents;
                        // Screen readers hear "Page 3".
                        resp.widget_info(|| {
                            egui::WidgetInfo::selected(egui::Role::Button, true, selected, format!("Page {}", app.session.page_label(abs)))
                        });
                        let tint = if selected { SELECTED_TINT } else { Color32::WHITE };
                        match thumbnails.then(|| thumb(app, ui.ctx(), ThumbOf::Page(abs), th)).flatten() {
                            Some(tex) => {
                                ui.painter().image(tex.id(), pr, egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0)), tint);
                            }
                            None => {
                                ui.painter().rect_filled(pr, 0.0, tint);
                            }
                        }
                        ui.painter().rect_stroke(pr, 0.0, Stroke::new(0.5, Color32::from_gray(0x4a)), StrokeKind::Outside);
                        if abs == 0 || doc.sections.iter().any(|s| s.start == abs) {
                            let c = egui::pos2(pr.center().x, row.min.y + 4.0);
                            ui.painter().add(egui::Shape::convex_polygon(
                                vec![c + vec2(-4.5, -3.0), c + vec2(4.5, -3.0), c + vec2(0.0, 3.0)],
                                t.icon,
                                Stroke::NONE,
                            ));
                            let marker =
                                ui.interact(egui::Rect::from_center_size(c, vec2(12.0, 8.0)), ui.scope_id().with(("section", abs)), Sense::click());
                            let label = format!(
                                "{}: {}",
                                crate::i18n::tr(&app.ui.language, "Numbering & Section Options…").trim_end_matches('…'),
                                app.session.page_label(abs)
                            );
                            marker.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Button, true, &label));
                            // A double click, or the keyboard or a screen reader activating it (a
                            // single pointer click only selects, as on a page).
                            let activated = marker.double_clicked() || marker.clicked() && !marker.clicked_by(egui::PointerButton::Primary);
                            if activated {
                                if editing_parents {
                                    let _ = app.run("layout.parents.edit", json!({"on": false}));
                                    if let Some(view) = app.view_mut() {
                                        view.fitted = false;
                                    }
                                }
                                crate::canvas::go_to_page(app, abs);
                                crate::menus::activate(app, "layout.section", &serde_json::Value::Null);
                            }
                        }
                        // Parent prefix in the outer top corner (black on white).
                        if let Some(pid) = p.parent
                            && let Some(pp) = doc.parents.iter().find(|x| x.id == pid)
                        {
                            let prefix = pp.parent.as_ref().map(|i| i.prefix.clone()).unwrap_or_default();
                            let g = ui.painter().layout_no_wrap(prefix, crate::theme::semibold(9.5), Color32::BLACK);
                            let w = g.size().x + 2.0;
                            let br = if p.side == designcraft_doc::PageSide::Left {
                                egui::Rect::from_min_size(pr.left_top(), vec2(w, 10.0))
                            } else {
                                egui::Rect::from_min_size(pr.right_top() - vec2(w, 0.0), vec2(w, 10.0))
                            };
                            ui.painter().rect_filled(br, 0.0, Color32::WHITE);
                            ui.painter().galley(br.min + vec2(1.0, -0.5), g, Color32::BLACK);
                        }
                        // Drag and drop: pages reorder, parents apply.
                        resp.dnd_set_drag_payload(PageDrag::Page(abs));
                        if resp.dragged() {
                            drag_ghost(ui, &app.session.page_label(abs), &t);
                        }
                        if dragging && resp.contains_pointer() {
                            ui.painter().rect_stroke(pr.expand(1.5), 0.0, Stroke::new(2.0, BADGE), StrokeKind::Outside);
                        }
                        if let Some(payload) = resp.dnd_release_payload::<PageDrag>() {
                            drop = Some(((*payload).clone(), abs));
                        }
                        if resp.clicked() || resp.double_clicked() {
                            if editing_parents {
                                let _ = app.run("layout.parents.edit", json!({"on": false}));
                                // The canvas refits to the page's spread once it is centred.
                                if let Some(v) = app.view_mut() {
                                    v.fitted = false;
                                }
                            }
                            crate::canvas::go_to_page(app, abs);
                        }
                        resp.context_menu(|ui| page_menu(app, ui, &doc, abs, si));
                    }
                    let label = spread_label(app, &doc, first, sp.pages.len());
                    // White on the badge; the theme's text colour elsewhere, so it reads on a light panel.
                    let badge = cur_spread == Some(si) && !editing_parents;
                    let ink = if badge { Color32::WHITE } else { t.text_strong };
                    let g = ui.painter().layout_no_wrap(label.clone(), crate::theme::semibold(11.0), ink);
                    let c = egui::pos2(row.min.x + center, top + row_h + 11.0);
                    let br = egui::Rect::from_center_size(c, vec2(g.size().x + 7.0, 12.5));
                    if badge {
                        ui.painter().rect_filled(br, 0.0, BADGE);
                    }
                    ui.painter().galley(c - g.size() / 2.0, g, ink);
                    let lr = ui.interact(br, ui.scope_id().with(("spread_label", si)), Sense::click());
                    // A click goes to the spread, so assistive technology hears a button.
                    lr.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Button, true, &label));
                    if lr.clicked() {
                        if editing_parents {
                            let _ = app.run("layout.parents.edit", json!({"on": false}));
                            if let Some(view) = app.view_mut() {
                                view.fitted = false;
                            }
                        }
                        crate::canvas::go_to_page(app, first);
                    }
                }
                // Right-click on the empty part of the panel: the short menu.
                let rest = ui.available_rect_before_wrap();
                let (_, empty) = ui.allocate_exact_size(vec2(ui.available_width(), rest.height().max(8.0)), Sense::click());
                empty.context_menu(|ui| empty_menu(app, ui));
            },
        );
    };
    // The divider: drag it to show more or fewer parents.
    let divider = |app: &mut DesignApp, ui: &mut egui::Ui| {
        let (dr, dresp) = ui.allocate_exact_size(vec2(ui.available_width(), DIVIDER_H), Sense::drag());
        let line = egui::Rect::from_center_size(dr.center(), vec2(dr.width() + 20.0, 1.5));
        ui.painter().rect_filled(line, 0.0, if dresp.hovered() || dresp.dragged() { t.text_dim } else { t.section_divider });
        let dresp = dresp.on_hover_cursor(egui::CursorIcon::ResizeVertical);
        if dresp.dragged() {
            let dy = dresp.drag_delta().y * if opts.pages_on_top { -1.0 } else { 1.0 };
            app.ui.pages_parents_height = Some([(parents_h + dy).clamp(parents_min, parents_max), space]);
        }
        dresp.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Splitter, true, "Parents and pages divider"));
    };
    if opts.pages_on_top {
        pages_part(app, ui);
        divider(app, ui);
        parents_part(app, ui);
    } else {
        parents_part(app, ui);
        divider(app, ui);
        pages_part(app, ui);
    }
    ui.spacing_mut().item_spacing.y = 4.0;
    ui.horizontal(|ui| {
        ui.set_min_height(FOOTER_H - 4.0);
        crate::rtl::label(
            ui,
            egui::RichText::new(
                crate::i18n::tr(&app.ui.language, "{pages} Pages in {spreads} Spreads")
                    .replace("{pages}", &doc.page_count().to_string())
                    .replace("{spreads}", &doc.spreads.len().to_string()),
            )
            .size(11.0)
            .color(t.text_dim),
        );
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            if icons::button(ui, "trash", 20.0, false, crate::i18n::tr(&app.ui.language, "Delete selected pages")).clicked() {
                let _ = crate::dialogs::delete_pages(app, vec![cur]);
            }
            // Create new page; a page dropped on it is duplicated at the end of the document.
            let plus = icons::button(ui, "plus", 20.0, dragging, crate::i18n::tr(&app.ui.language, "Create new page"));
            if let Some(payload) = plus.dnd_release_payload::<PageDrag>() {
                if let PageDrag::Page(from) = *payload
                    && let Some((si, _)) = doc.page_loc(from)
                {
                    let _ = app.run("layout.pages.duplicateSpread", json!({"spread": si, "atEnd": true}));
                }
            } else if plus.clicked() {
                let _ = app.run("layout.pages.insert", json!({"after": cur, "count": 1}));
            }
            // Edit page size: InDesign's presets for the current page, or Custom Page Size….
            let size = icons::button(ui, "panel-pages", 20.0, false, crate::i18n::tr(&app.ui.language, "Edit page size"));
            egui::Popup::menu(&size).show(|ui| {
                let Some((si, pi)) = doc.page_loc(cur) else { return };
                let pg = &doc.spreads[si].pages[pi];
                let landscape = pg.width > pg.height;
                let current = crate::panels::properties::preset_name(pg.width, pg.height);
                for (name, w, h) in crate::panels::properties::PAGE_PRESETS {
                    let (w, h) = if landscape { (h, w) } else { (w, h) };
                    if ui.selectable_label(current == *name, crate::rtl::widget(ui, crate::i18n::tr(&app.ui.language, name))).clicked() {
                        let _ = app.run("layout.pageSize", json!({"pages": [cur + 1], "width": w, "height": h}));
                        ui.close();
                    }
                }
                ui.separator();
                if ui.button(crate::rtl::widget(ui, crate::i18n::tr(&app.ui.language, "Custom Page Size…"))).clicked() {
                    app.ui.dialog =
                        Some(crate::dialogs::Dialog::new("cmd:layout.pageSize", json!({"pages": [cur + 1], "width": pg.width, "height": pg.height})));
                    ui.close();
                }
            });
        });
    });
    // Alt-drag copies a page that is a spread of its own to where it's dropped, in one undo
    // step; a page from a larger spread moves as usual.
    let alt = ui.input(|i| i.modifiers.alt);
    let copy = |from: usize| {
        let (si, _) = doc.page_loc(from)?;
        (alt && doc.spreads.get(si).is_some_and(|s| s.pages.len() == 1)).then_some(si)
    };
    match drop {
        Some((PageDrag::Page(from), to)) => {
            if let Some(si) = copy(from) {
                let _ = app.run("layout.pages.duplicateSpread", json!({"spread": si, "to": to}));
            } else if from != to {
                let _ = app.run("layout.pages.move", json!({"from": from, "to": to}));
            }
        }
        Some((PageDrag::Parent(parent), to)) => {
            let _ = app.run("layout.pages.applyParent", json!({"pages": [to], "parent": parent.map(|id| id.0)}));
        }
        _ => {}
    }
}

/// The panel menu's page commands, acting on the current page and its spread.
pub fn panel_menu(app: &mut DesignApp, ui: &mut egui::Ui) {
    let Some(st) = app.session.active() else { return };
    let doc = st.doc.clone();
    let cur = crate::canvas::current_page(app).unwrap_or(0);
    let Some((si, _)) = doc.page_loc(cur) else { return };
    page_menu(app, ui, &doc, cur, si);
}

/// Right-click on the panel's empty part, as InDesign's: the commands that need no page.
fn empty_menu(app: &mut DesignApp, ui: &mut egui::Ui) {
    let lang = app.ui.language.clone();
    let cur = crate::canvas::current_page(app).unwrap_or(0);
    let item = |ui: &mut egui::Ui, label: &str| ui.button(crate::rtl::widget(ui, crate::i18n::tr(&lang, label))).clicked();
    if item(ui, "Insert Pages…") {
        crate::dialogs::open_insert_pages(app, cur);
        ui.close();
    }
    if item(ui, "Move Pages…") {
        app.ui.dialog = Some(crate::dialogs::Dialog::new("cmd:layout.pages.move", json!({"from": cur, "to": cur})));
        ui.close();
    }
    ui.separator();
    if item(ui, "Panel Options…") {
        open_panel_options(app);
        ui.close();
    }
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

fn page_menu(app: &mut DesignApp, ui: &mut egui::Ui, doc: &designcraft_doc::Document, abs: usize, si: usize) {
    let lang = app.ui.language.clone();
    let tr = |s: &str| crate::i18n::tr(&lang, s).to_owned();
    let pages_in_spread = doc.spreads.get(si).map_or(1, |sp| sp.pages.len());
    let item = |ui: &mut egui::Ui, label: &str| ui.button(crate::rtl::widget(ui, tr(label))).clicked();
    // InDesign's order: pages, parents, layouts and numbering, shuffling.
    if item(ui, "Insert Pages…") {
        crate::dialogs::open_insert_pages(app, abs);
        ui.close();
    }
    if item(ui, "Move Pages…") {
        app.ui.dialog = Some(crate::dialogs::Dialog::new("cmd:layout.pages.move", json!({"from": abs, "to": abs})));
        ui.close();
    }
    if item(ui, "Duplicate Spread") {
        let _ = app.run("layout.pages.duplicateSpread", json!({"spread": si}));
        ui.close();
    }
    if item(ui, "Duplicate Spread at End of Document") {
        let _ = app.run("layout.pages.duplicateSpread", json!({"spread": si, "atEnd": true}));
        ui.close();
    }
    if item(ui, "Delete Page") {
        let _ = crate::dialogs::delete_pages(app, vec![abs]);
        ui.close();
    }
    if pages_in_spread > 1 && item(ui, "Delete Spread") {
        let first = doc.first_page_of_spread(si);
        let _ = crate::dialogs::delete_pages(app, (first..first.saturating_add(pages_in_spread)).collect());
        ui.close();
    }
    ui.separator();
    if item(ui, "New Parent…") {
        crate::menus::activate(app, "layout.parents.new", &serde_json::Value::Null);
        ui.close();
    }
    crate::menus::menu_button(ui, crate::rtl::widget(ui, tr("Apply Parent to Pages")), |ui| {
        if ui.button(crate::rtl::widget(ui, tr("[None]"))).clicked() {
            let _ = app.run("layout.pages.applyParent", json!({"pages": [abs], "parent": null}));
            ui.close();
        }
        for pp in &doc.parents {
            let l = pp.parent.as_ref().map(|i| i.label()).unwrap_or_default();
            if ui.button(&l).clicked() {
                let _ = app.run("layout.pages.applyParent", json!({"pages": [abs], "parent": pp.id.0}));
                ui.close();
            }
        }
    });
    if item(ui, "Override All Parent Page Items") {
        let _ = app.run("layout.overrideParentItems", json!({"page": abs}));
        ui.close();
    }
    crate::menus::menu_button(ui, crate::rtl::widget(ui, tr("Parent Pages")), |ui| {
        for (id, label) in [("layout.removeOverrides", "Remove All Local Overrides"), ("layout.detachAll", "Detach All Objects from Parent")] {
            if ui.button(crate::rtl::widget(ui, tr(label))).clicked() {
                let _ = app.run(id, json!({"page": abs}));
                ui.close();
            }
        }
    });
    ui.separator();
    if item(ui, "Create Alternate Layout…") {
        crate::menus::activate(app, "layout.createAlternate", &serde_json::Value::Null);
        ui.close();
    }
    if item(ui, "Numbering & Section Options…") {
        crate::menus::activate(app, "layout.section", &serde_json::Value::Null);
        ui.close();
    }
    ui.separator();
    let mut shuffle = doc.spreads.get(si).is_none_or(|sp| sp.allow_shuffle);
    if ui.checkbox(&mut shuffle, crate::rtl::widget(ui, tr("Allow Selected Spread to Shuffle"))).clicked() {
        let _ = app.run("layout.spreadShuffle", json!({"spread": si, "allow": shuffle}));
        ui.close();
    }
    // Not in InDesign's menu (it drags pages into spreads): kept for spreads of up to 10 pages.
    if si > 0
        && ui
            .button(crate::rtl::widget(ui, tr("Add to Previous Spread")))
            .on_hover_ui(|ui| {
                crate::rtl::label(ui, tr("Spreads of up to 10 pages"));
            })
            .clicked()
    {
        let _ = app.run("layout.pages.toSpread", json!({"page": abs + 1, "spread": si - 1}));
        ui.close();
    }
    ui.separator();
    if item(ui, "Panel Options…") {
        open_panel_options(app);
        ui.close();
    }
}

/// A small label following the pointer while dragging.
pub(crate) fn drag_ghost(ui: &egui::Ui, label: &str, t: &Tokens) {
    let Some(p) = ui.ctx().pointer_interact_pos() else { return };
    let painter = ui.ctx().layer_painter(egui::LayerId::new(egui::Order::Tooltip, egui::Id::unique("page_drag")));
    let g = painter.layout_no_wrap(label.to_string(), egui::FontId::proportional(11.0), t.text_strong);
    let r = egui::Rect::from_min_size(p + vec2(10.0, 8.0), g.size() + vec2(10.0, 6.0));
    painter.rect(r, 2.0, t.panel_darker, Stroke::new(1.0, t.field_border), StrokeKind::Inside);
    painter.galley(r.min + vec2(5.0, 3.0), g, t.text_strong);
}

#[cfg(test)]
mod tests;

#[cfg(test)]
mod spread_label_tests {
    use egui_kittest::{Harness, kittest::Queryable};
    use serde_json::json;

    use super::*;

    /// Section-marker activation targets its page and leaves parent editing without editing the document.
    #[test]
    fn double_clicking_a_section_marker_opens_that_pages_numbering_options() {
        let mut app = app();
        app.run("layout.section", json!({"page": 1, "startNumber": 1, "style": "lowerRoman"})).unwrap();
        app.run("layout.section", json!({"page": 3, "startNumber": 1, "style": "arabic"})).unwrap();
        app.canvas_rect = Some(egui::Rect::from_min_size(egui::Pos2::ZERO, vec2(1000.0, 800.0)));
        app.run("layout.parents.edit", json!({"on": true})).unwrap();
        let mut h = Harness::builder().with_size(vec2(280.0, 900.0)).with_step_dt(1.0 / 60.0).build_ui_state(
            |ui, app: &mut DesignApp| {
                let key = egui::Id::unique("section_marker_test_fonts");
                if ui.data(|d| d.get_temp::<bool>(key)).is_none() {
                    crate::theme::install_fonts(ui.ctx(), "");
                    ui.data_mut(|d| d.insert_temp(key, true));
                    return;
                }
                show(app, ui);
            },
            app,
        );
        h.run_steps(3);
        let pos = h.get_by_label("Numbering & Section Options: 1").rect().center();
        let before = h.state().session.doc().unwrap().doc.clone();
        for _ in 0..2 {
            for pressed in [true, false] {
                h.event(egui::Event::PointerMoved(pos));
                h.event(egui::Event::PointerButton { pos, button: egui::PointerButton::Primary, pressed, modifiers: egui::Modifiers::NONE });
                h.run_steps(1);
            }
        }
        let dialog = h.state().ui.dialog.as_ref().unwrap();
        assert_eq!(dialog.id, "numberingSection");
        assert_eq!(dialog.fields.get("page"), Some(&json!(3)));
        assert!(!h.state().session.doc().unwrap().editing_parents);
        assert!(std::sync::Arc::ptr_eq(&h.state().session.doc().unwrap().doc, &before), "opening options does not edit the section");
    }

    #[test]
    fn a_section_marker_opens_its_options_from_the_keyboard_but_not_on_one_click() {
        let mut app = app();
        app.run("layout.section", json!({"page": 3, "startNumber": 5})).unwrap();
        app.canvas_rect = Some(egui::Rect::from_min_size(egui::Pos2::ZERO, vec2(1000.0, 800.0)));
        let mut h = Harness::builder().with_size(vec2(280.0, 900.0)).with_step_dt(1.0 / 60.0).build_ui_state(
            |ui, app: &mut DesignApp| {
                let key = egui::Id::unique("section_marker_key_test_fonts");
                if ui.data(|d| d.get_temp::<bool>(key)).is_none() {
                    crate::theme::install_fonts(ui.ctx(), "");
                    ui.data_mut(|d| d.insert_temp(key, true));
                    return;
                }
                show(app, ui);
            },
            app,
        );
        h.run_steps(3);
        h.get_by_label("Numbering & Section Options: 5").click();
        h.run_steps(2);
        assert!(h.state().ui.dialog.is_none(), "one pointer click doesn't open the options");
        h.run_steps(30);
        h.get_by_label("Numbering & Section Options: 5").click_accesskit();
        h.run_steps(2);
        assert_eq!(h.state().ui.dialog.as_ref().map(|d| d.id.as_str()), Some("numberingSection"));
    }

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

    /// Labels navigate to document spreads, including when parent editing was active.
    #[test]
    fn the_panel_shows_spread_labels_and_a_click_on_one_goes_to_its_spread() {
        let mut app = app();
        app.run("layout.section", json!({"page": 3, "startNumber": 1})).unwrap();
        // The canvas the panel's clicks move (not drawn here).
        app.canvas_rect = Some(egui::Rect::from_min_size(egui::pos2(0.0, 0.0), vec2(1000.0, 800.0)));
        let mut h = Harness::builder().with_size(vec2(260.0, 900.0)).build_ui_state(
            |ui, app: &mut DesignApp| {
                let key = egui::Id::unique("test_fonts");
                if ui.data(|d| d.get_temp::<bool>(key)).is_none() {
                    crate::theme::install_fonts(ui.ctx(), "");
                    ui.data_mut(|d| d.insert_temp(key, true));
                    return;
                }
                egui::ScrollArea::vertical().show(ui, |ui| show(app, ui));
            },
            app,
        );
        h.run_steps(3);
        // A spread label is a button: a click goes to the spread.
        let _ = h.get_by(|n| n.role() == egui::accesskit::Role::Button && n.label().as_deref() == Some("2, 1"));
        assert!(h.query_by_label("2 1").is_none());
        // Each page is named for screen readers: two pages are numbered 1.
        assert_eq!(h.query_all_by_label("Page 1").count(), 2);
        h.state_mut().run("layout.parents.edit", json!({"on": true})).unwrap();
        if let Some(view) = h.state_mut().view_mut() {
            view.fitted = true;
        }
        h.run_steps(3);
        h.get_by_label("2–3").click();
        h.run_steps(3);
        assert!(!h.state().session.doc().unwrap().editing_parents);
        assert!(!h.state().view().unwrap().fitted);
        assert_eq!(h.state().session.doc().unwrap().doc.page_loc(crate::canvas::current_page(h.state()).unwrap()).unwrap().0, 2);
    }
}
