//! The Pages panel on the craft framework: the parents as a list or as tiles, the spreads with their page thumbnails, the
//! divider between them, the footer and the menus.
//!
//! The framework has no component for a page thumbnail laid out in spreads (pages of their own size centred on the spine,
//! a label under each spread, a column for each alternate layout), so [`PageCell`] is the app's own leaf: a button
//! that paints what the panel gives it and takes the drags and drops of pages and parents. Everything it reports goes
//! through the same engine commands as the native panel's.

use std::ops::Range;

use craft_behavior::{
    CursorIcon, CursorPriority, DisplayOrder, ItemSource, Metrics, PointerButton, PressPhase, Selection, SelectionChange, Sense, SizeHint,
};
use craft_ui_panels::sections::DropAction;
use craft_ui_widgets::buttons::{Button, IconButton};
use craft_ui_widgets::contract::overlay::Anchor;
use craft_ui_widgets::contract::{
    Access, Bind, CollectionEvent, Common, Component, Cx, DisabledLook, DropMark, DropOperation, DropTarget, LeadSource, ListDrag, MeasureCx,
    NoValue, Outcome, PaintUi, Pointer as _, PopupKind, Props as _, Role, SelectMode, Show as _, ShowCollection as _, ShowContainer as _, Sizing,
    Trigger,
};
use craft_ui_widgets::display::{Divider, Shade, Text};
use craft_ui_widgets::list::{List, ListRow};
use craft_ui_widgets::menu::{Menu, MenuItem};
use craft_ui_widgets::primitives::IconSpec;
use craft_ui_widgets::runtime::drag::{self, DragPreview};
use craft_ui_widgets::runtime::env::{Direction, Label, with_env};
use craft_ui_widgets::style::{ComponentStyle, Flags, FocusLook, StyleCx, TypeRole, Variant};
use designcraft_doc::SpreadId;
use egui::{Color32, Pos2, Rect, Stroke, StrokeKind, Ui, UiBuilder, Vec2, pos2, vec2};
use serde_json::json;

use super::{
    Act, BADGE, DIVIDER_H, FOOTER_H, LAYOUT_HEADER_H, MenuEntry, NEW_PARENT_H, PageDrag, PageDrop, ParentRow, Plan, SELECTED_TINT, ThumbOf,
    apply_drop, empty_entries, layout_entries, layout_index, layout_size, open_create_alternate, open_layout_options, page_entries, parent_entries,
    select_parent, spread_label, spread_range, thumb, tile_width, truncate,
};
use crate::DesignApp;
use crate::icons;
use crate::theme::Tokens;

/// What a [`PageCell`] is for.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Kind {
    /// A button: a page, a spread's label, a section marker, a layout's header, a parent's tile.
    Button,
    /// The divider's handle: a drag reports how far it moved.
    Handle,
    /// Shows only: a parent's icon in its row, the empty part of the panel a menu hangs from.
    Plain,
}

/// How a cell shows in this pass.
#[derive(Clone, Copy, Debug, Default)]
pub(super) struct Look {
    /// The pointer is over it.
    pub hovered: bool,
    /// A press is held on it.
    pub held: bool,
    /// A drag it takes is over it.
    pub over: bool,
}

/// A click on a cell: the second click of a double click, or the keyboard or a screen reader activating it.
#[derive(Clone, Copy, Debug)]
pub(super) struct Click {
    pub double: bool,
    pub by_key: bool,
}

/// What a cell paints: the painter clipped to its rect, the rect and how the cell shows.
type Paint<'a> = Box<dyn FnOnce(&egui::Painter, Rect, Look) + 'a>;

/// The look of a cell: it has no surface of its own; the panel paints what the cell shows.
#[derive(Clone, Debug, PartialEq)]
pub(super) struct PageCellStyle {
    focus: FocusLook,
}

impl ComponentStyle for PageCellStyle {
    fn focus(&self) -> Option<&FocusLook> {
        Some(&self.focus)
    }

    fn metrics(&self) -> Metrics {
        Metrics::new()
    }
}

/// What a cell reports.
#[derive(Debug)]
pub(super) struct PageCellOutput {
    pub response: egui::Response,
    pub click: Option<Click>,
    /// The page or parent dropped on the cell, and whether the drop copies (Alt).
    pub dropped: Option<(PageDrag, DropOperation)>,
    /// How far a handle was dragged in this pass.
    pub moved: Option<f32>,
}

impl Outcome for PageCellOutput {
    type Value = NoValue;

    fn response(&self) -> &egui::Response {
        &self.response
    }
}

/// Where a drop on a cell lands: the cell.
struct OnCell {
    rect: Rect,
    accepts: fn(&PageDrag) -> bool,
}

impl DropTarget<PageDrag> for OnCell {
    type Drop = (PageDrag, DropOperation);

    fn accept(&self, payload: &PageDrag, _: Pos2, op: DropOperation) -> Option<Self::Drop> {
        (self.accepts)(payload).then(|| (payload.clone(), op))
    }

    fn mark(&self, _: &Self::Drop) -> DropMark {
        DropMark::Into(self.rect)
    }
}

/// A leaf of the Pages panel: it paints what the panel gives it, senses a click (a double click, the keyboard) and,
/// when the panel says so, drags a page or a parent and takes drops of them.
#[derive(craft_ui_widgets::contract::Component, craft_ui_widgets::contract::Props)]
#[component(content_painter)]
pub(super) struct PageCell<'a> {
    #[common]
    common: Common,
    kind: Kind,
    selected: bool,
    size: Vec2,
    drag: Option<(PageDrag, Label)>,
    takes: Option<fn(&PageDrag) -> bool>,
    paint: Option<Paint<'a>>,
    look: Look,
    click: Option<Click>,
    dropped: Option<(PageDrag, DropOperation)>,
    moved: Option<f32>,
}

impl<'a> PageCell<'a> {
    fn new(label: impl Into<Label>, kind: Kind) -> Self {
        PageCell {
            common: Common::new(),
            kind,
            selected: false,
            size: Vec2::ZERO,
            drag: None,
            takes: None,
            paint: None,
            look: Look::default(),
            click: None,
            dropped: None,
            moved: None,
        }
        .label(label)
    }

    /// A button named `label`.
    pub(super) fn button(label: impl Into<Label>) -> Self {
        Self::new(label, Kind::Button)
    }

    /// The divider's handle, named `label`.
    pub(super) fn handle(label: impl Into<Label>) -> Self {
        Self::new(label, Kind::Handle)
    }

    /// A cell that only shows.
    pub(super) fn plain() -> Self {
        Self::new(Label::Empty, Kind::Plain)
    }

    /// The size a cell measures when it is laid out in a flow.
    pub(super) fn sized(mut self, size: Vec2) -> Self {
        self.size = size;
        self
    }

    /// Whether the cell is the page or parent the panel has chosen.
    pub(super) fn chosen(mut self, on: bool) -> Self {
        self.selected = on;
        self
    }

    /// A drag that starts on the cell carries `payload`, shown as the badge `name`.
    pub(super) fn drags(mut self, payload: PageDrag, name: impl Into<Label>) -> Self {
        self.drag = Some((payload, name.into()));
        self
    }

    /// The cell takes the drops `accepts` takes.
    pub(super) fn takes(mut self, accepts: fn(&PageDrag) -> bool) -> Self {
        self.takes = Some(accepts);
        self
    }

    /// What the cell paints.
    pub(super) fn painted(mut self, paint: impl FnOnce(&egui::Painter, Rect, Look) + 'a) -> Self {
        self.paint = Some(Box::new(paint));
        self
    }
}

impl Component for PageCell<'_> {
    const ROLE: Role = Role::BUTTON;
    const ALT_ROLES: &'static [Role] = &[Role::SPLITTER, Role::None];
    const OVERLAYS: bool = false;
    const PARTS: bool = false;
    // The thumbnails are data: they dim with the cell.
    const DISABLED: DisabledLook = DisabledLook::Faded;
    type Style = PageCellStyle;
    type Output = PageCellOutput;

    fn style(cx: &StyleCx<'_>, _: PageCellStyleKey) -> PageCellStyle {
        PageCellStyle { focus: cx.focus_look() }
    }

    fn measure(&mut self, _: &MeasureCx<'_, Self>) -> Vec2 {
        self.size
    }

    fn sense(&self) -> Sense {
        match self.kind {
            Kind::Button if self.drag.is_some() => Sense::CLICK | Sense::DRAG | Sense::FOCUS,
            Kind::Button => Sense::CLICK | Sense::FOCUS,
            Kind::Handle => Sense::DRAG | Sense::HOVER,
            Kind::Plain => Sense::HOVER,
        }
    }

    fn flags(&self) -> Flags {
        if self.selected { Flags::SELECTED } else { Flags::empty() }
    }

    /// A press that moves starts the cell's drag, or, for the handle, reports how far it moved; a click, the second
    /// click of a double click and the keyboard are reported as a click; a drag the cell takes is over it or drops on it.
    fn update(&mut self, cx: &mut Cx<'_, Self>) {
        let phase = cx.press().phase;
        self.look.hovered = cx.hit();
        self.look.held = phase.button_down();
        match phase {
            PressPhase::DragStart { start, pos } if start.button == PointerButton::Primary => {
                if self.kind == Kind::Handle {
                    self.moved = Some(pos.y - start.origin.y);
                } else if let Some((payload, name)) = self.drag.clone() {
                    cx.start_drag(payload, DragPreview::Badge(name));
                }
            }
            PressPhase::Drag { delta, .. } if self.kind == Kind::Handle => self.moved = Some(delta.y),
            _ => {}
        }
        if self.kind == Kind::Handle && (self.look.hovered || self.look.held) {
            cx.cursor(CursorIcon::ResizeVertical, CursorPriority::Hover);
        }
        if let Some(activation) = cx.activation() {
            let double = !activation.by_key && matches!(phase, PressPhase::Release { start, .. } if start.clicks == 2);
            self.click = Some(Click { double, by_key: activation.by_key });
        }
        if let Some(accepts) = self.takes {
            let rect = cx.rect;
            self.look.over = cx.dragging::<PageDrag>().is_some_and(|(payload, at)| rect.contains(at) && accepts(&payload));
            self.dropped = cx.drop_target::<PageDrag, _>(&OnCell { rect, accepts });
        }
    }

    fn access(&self, access: &mut Access, _: &Cx<'_, Self>) {
        match self.kind {
            Kind::Button => {}
            Kind::Handle => {
                access.role(Role::SPLITTER);
            }
            Kind::Plain => {
                access.role(Role::None);
            }
        }
    }

    fn paint(&mut self, ui: &mut PaintUi<'_>, cx: &Cx<'_, Self>) {
        let Some(paint) = self.paint.take() else { return };
        let content = ui.content_painter(cx);
        paint(&content.painter, cx.rect, self.look);
    }

    fn finish(self, cx: Cx<'_, Self>) -> PageCellOutput {
        PageCellOutput { response: cx.response.clone(), click: self.click, dropped: self.dropped, moved: self.moved }
    }
}

/// The glyph of one of the app's own icons.
#[derive(craft_ui::contract::MemoProps)]
struct PanelIcon(&'static str);

impl craft_ui_widgets::primitives::Glyph for PanelIcon {
    fn paint(&self, painter: &egui::Painter, _tokens: &craft_ui_widgets::style::UiTokens, rect: egui::Rect, color: egui::Color32, _mirrored: bool) {
        icons::paint(painter, rect, self.0, color);
    }
}

/// The glyph of the New Parent row.
static NEW_PARENT_ICON: PanelIcon = PanelIcon("plus");

fn full_uv() -> Rect {
    Rect::from_min_max(pos2(0.0, 0.0), pos2(1.0, 1.0))
}

/// A cell is happy with any page or parent dropped on it.
fn takes_any(_: &PageDrag) -> bool {
    true
}

/// A parent's icon in `rect`, `h` high: its rendered spread, or blank pages when it has none, and the outline of each of
/// its pages.
fn paint_parent_icon(painter: &egui::Painter, rect: Rect, tex: Option<&egui::TextureHandle>, pages: &[(f32, f32)], h: f32, border: Color32) {
    match tex {
        Some(tex) => {
            painter.image(tex.id(), rect, full_uv(), Color32::WHITE);
        }
        None => {
            painter.rect_filled(rect, 0.0, Color32::WHITE);
        }
    }
    for (x, w) in pages {
        let page = Rect::from_min_size(pos2(rect.min.x + x * h, rect.min.y), vec2(w * h, h));
        painter.rect_stroke(page, 0.0, Stroke::new(0.5, border), StrokeKind::Inside);
    }
}

/// Leaves Edit Parents, so the canvas moves on document pages again, and has the canvas refit to the page it goes to.
fn leave_parent_edit(app: &mut DesignApp) {
    let _ = app.run("layout.parents.edit", json!({"on": false}));
    if let Some(view) = app.view_mut() {
        view.fitted = false;
    }
}

/// Shows `entries` as the rows of a menu's body; the entry run is put in `picked`.
fn menu_rows(ui: &mut Ui, entries: &[MenuEntry], picked: &mut Option<Act>) {
    for (at, entry) in entries.iter().enumerate() {
        match entry {
            MenuEntry::Sep => {
                Divider::horizontal().show(ui);
            }
            MenuEntry::Item { label, enabled, check, hint, act } => {
                let mut item = MenuItem::new(label.clone()).id_salt(at).enabled(*enabled);
                if let Some(on) = check {
                    item = item.check(*on);
                }
                if let Some(hint) = hint {
                    item = item.tooltip(hint.clone());
                }
                if item.show(ui).ran {
                    *picked = Some(act.clone());
                }
            }
            MenuEntry::Sub { label, enabled, items } => {
                Menu::submenu_row_enabled(ui, label.clone(), *enabled, |ui, _| menu_rows(ui, items, picked));
            }
        }
    }
}

/// A context menu named `label` over `opener`, with the entries `entries` makes while it is open: the entry run.
fn context_menu(ui: &mut Ui, label: String, opener: &egui::Response, mut entries: impl FnMut() -> Vec<MenuEntry>) -> Option<Act> {
    let mut picked = None;
    let _ = Menu::context(label, opener).id(opener.id.with("menu")).show(ui, |ui, _| menu_rows(ui, &entries(), &mut picked));
    picked
}

// ---------------------------------------------------------------------------------------------------------------------
// The panel

pub(super) fn show(app: &mut DesignApp, ui: &mut Ui, doc: &designcraft_doc::Document, p: &Plan) {
    let direction = if crate::i18n::is_rtl(&app.ui.language) { Direction::Rtl } else { Direction::Ltr };
    let mut drop: Option<PageDrop> = None;
    with_env(
        ui,
        |scope| scope.direction = Some(direction),
        |ui| {
            ui.spacing_mut().item_spacing.y = 0.0;
            if p.opts.pages_on_top {
                pages(app, ui, doc, p, &mut drop);
                divider(app, ui, p);
                parents(app, ui, doc, p);
            } else {
                parents(app, ui, doc, p);
                divider(app, ui, p);
                pages(app, ui, doc, p, &mut drop);
            }
            ui.spacing_mut().item_spacing.y = 4.0;
            footer_contents(app, ui, doc, p.cur);
        },
    );
    apply_drop(app, doc, drop);
}

/// The divider: drag it to show more or fewer parents.
fn divider(app: &mut DesignApp, ui: &mut Ui, p: &Plan) {
    let (_, rect) = ui.allocate_space(vec2(ui.available_width(), DIVIDER_H));
    let t = Tokens::get(ui.ctx());
    let (idle, lit) = (t.section_divider, t.text_dim);
    let out = PageCell::handle("Parents and pages divider")
        .id_salt("pages_divider")
        .painted(move |painter, rect, look| {
            let line = Rect::from_center_size(rect.center(), vec2(rect.width() + 20.0, 1.5));
            painter.rect_filled(line, 0.0, if look.hovered || look.held { lit } else { idle });
        })
        .show_at(ui, rect);
    if let Some(dy) = out.moved {
        let dy = dy * if p.opts.pages_on_top { -1.0 } else { 1.0 };
        app.ui.pages_parents_height = Some([(p.parents_h + dy).clamp(p.parents_min, p.parents_max), p.space]);
    }
}

// ---------------------------------------------------------------------------------------------------------------------
// Parents

fn parents(app: &mut DesignApp, ui: &mut Ui, doc: &designcraft_doc::Document, p: &Plan) {
    if p.opts.parents_vertical {
        parents_list(app, ui, doc, p);
    } else {
        parent_tiles(app, ui, doc, p);
    }
}

/// A row of the parents list: [None], a parent, or New Parent under them.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
enum ParentKey {
    None,
    Parent(SpreadId),
    New,
}

/// The rows of the parents list.
struct ParentRows {
    keys: Vec<ParentKey>,
    row_h: f32,
}

impl DisplayOrder<ParentKey> for ParentRows {
    fn len(&self) -> usize {
        self.keys.len()
    }

    fn key(&self, at: usize) -> ParentKey {
        self.keys[at]
    }

    fn index_of(&self, key: &ParentKey) -> Option<usize> {
        self.keys.iter().position(|held| held == key)
    }
}

impl ItemSource<ParentKey> for ParentRows {
    fn size_hint(&self, at: usize) -> SizeHint {
        SizeHint::Height(if self.keys[at] == ParentKey::New { NEW_PARENT_H } else { self.row_h })
    }
}

/// The parents as a list: [None] and each parent with its icon at the end, and the New Parent row. A click picks a
/// parent, a double click edits it, a drag carries it to a page, and its menu is the parent commands.
fn parents_list(app: &mut DesignApp, ui: &mut Ui, doc: &designcraft_doc::Document, p: &Plan) {
    let ctx = ui.ctx().clone();
    let lang = app.ui.language.clone();
    let tr = |s: &str| crate::i18n::tr(&lang, s).to_owned();
    let border = Tokens::get(&ctx).border;
    let mut keys = vec![ParentKey::None];
    keys.extend(p.rows.iter().filter_map(|row| row.parent).map(|(_, id)| ParentKey::Parent(id)));
    keys.push(ParentKey::New);
    let source = ParentRows { keys, row_h: p.row_h };
    let active = p.rows.iter().find_map(|row| {
        let (i, id) = row.parent?;
        (p.cur_parent == Some(i) || p.selected_parent == Some(id)).then_some(ParentKey::Parent(id))
    });
    let mut selected = active.map_or_else(Selection::default, Selection::single);
    let rect = Rect::from_min_size(ui.cursor().min, vec2(ui.available_width(), p.parents_h));
    let new_label = tr("New Parent");
    let out = ui
        .scope_builder(UiBuilder::new().max_rect(rect), |ui| {
            ui.set_min_size(rect.size());
            List::<ParentKey>::new(tr("Parent Pages"))
                .id_salt("pages_parents")
                .select(SelectMode::Single)
                .arrows_select(true)
                .sizing(Sizing::Fill)
                .selection(Bind::bound(&mut selected))
                .drag_payload(|drag: &ListDrag<ParentKey>| match drag.grabbed {
                    ParentKey::None => Some(PageDrag::Parent(None)),
                    ParentKey::Parent(id) => Some(PageDrag::Parent(Some(id))),
                    ParentKey::New => None,
                })
                .show_items(ui, &source, &mut |key, _| {
                    if *key == ParentKey::New {
                        return ListRow::new(new_label.clone()).icon(IconSpec::glyph(&NEW_PARENT_ICON));
                    }
                    let row = match key {
                        ParentKey::Parent(id) => p.rows.iter().find(|row| row.parent.is_some_and(|(_, held)| held == *id)),
                        _ => p.rows.first(),
                    };
                    let Some(row) = row else { return ListRow::new(Label::Empty) };
                    let tex = row.parent.filter(|_| p.parent_thumbnails).and_then(|(_, id)| thumb(app, &ctx, ThumbOf::Parent(id), p.icon_h));
                    let (pages, h) = (row.pages.clone(), p.icon_h);
                    let icon = PageCell::plain()
                        .id_salt("pages_parent_icon")
                        .sized(vec2(row.unit_w * h, h))
                        .painted(move |painter, rect, _| paint_parent_icon(painter, rect, tex.as_ref(), &pages, h, border));
                    ListRow::new(row.label.clone()).trailing_field(icon, |_| {})
                })
        })
        .inner;
    let mut picked = None;
    let mut edit = None;
    let mut menu = None;
    // An arrow key that moves the selection over New Parent does not make a parent; a click or Enter on it does.
    let by_keys = out.events.iter().any(|event| matches!(event, CollectionEvent::LeadMoved { by: LeadSource::Keys, .. }));
    for event in out.events {
        match event {
            CollectionEvent::Selection(SelectionChange::Set(keys)) => {
                if let Some(key) = keys.last().copied().filter(|key| !(by_keys && *key == ParentKey::New)) {
                    picked = Some(key);
                }
            }
            CollectionEvent::LeadMoved { key, by: LeadSource::Pointer } => picked = Some(key),
            CollectionEvent::Activate(key) => {
                picked = Some(key);
                edit = Some(key);
            }
            CollectionEvent::ContextMenu(request) => menu = Some(request),
            _ => {}
        }
    }
    match picked {
        Some(ParentKey::None) => select_parent(app, None),
        Some(ParentKey::Parent(id)) => select_parent(app, Some(id)),
        // New Parent in one click: the next free prefix (B, C…), named Parent.
        Some(ParentKey::New) => {
            if let Ok(made) = app.run("layout.parents.new", json!({})) {
                select_parent(app, made.get("id").and_then(serde_json::Value::as_u64).map(SpreadId));
            }
        }
        None => {}
    }
    if let Some(ParentKey::Parent(id)) = edit {
        let _ = app.run("layout.parents.edit", json!({"on": true, "parent": id.0}));
    }
    // The menu of the parent a context request named hangs where the request was made until it closes.
    let menu_id = out.response.id.with("parent-menu");
    if let Some(request) = menu
        && let ParentKey::Parent(id) = request.key
    {
        ctx.data_mut(|d| d.insert_temp(menu_id, (id, request.at)));
    }
    if let Some((id, at)) = ctx.data(|d| d.get_temp::<(SpreadId, Pos2)>(menu_id)) {
        let mut open = true;
        let mut chosen = None;
        let _ = Menu::new(tr("Parent Pages"))
            .id(menu_id.with("menu"))
            .opened_by(&out.response, Trigger::Manual)
            .anchor(Anchor::Point(at))
            .open(Bind::bound(&mut open))
            .show(ui, |ui, _| menu_rows(ui, &parent_entries(&lang, doc, Some(id), false), &mut chosen));
        if !open || chosen.is_some() {
            ctx.data_mut(|d| d.remove_temp::<(SpreadId, Pos2)>(menu_id));
        }
        if let Some(act) = chosen {
            act.run(app, doc, &p.layouts);
        }
    }
}

/// The parents side by side (Show Vertically off): a tile of the icon and the prefix each, wrapping, and the New Parent
/// button under them.
fn parent_tiles(app: &mut DesignApp, ui: &mut Ui, doc: &designcraft_doc::Document, p: &Plan) {
    let lang = app.ui.language.clone();
    egui::ScrollArea::vertical().id_salt("pages_parents").max_height(p.parents_h).min_scrolled_height(p.parents_h).auto_shrink([false, true]).show(
        ui,
        |ui| {
            let width = ui.available_width();
            let tile_h = p.icon_h + 22.0;
            let (mut x, mut y) = (0.0_f32, 0.0_f32);
            let mut places = Vec::with_capacity(p.rows.len());
            for row in &p.rows {
                let w = tile_width(p.icon_h, row.unit_w);
                if x > 0.0 && x + w > width {
                    x = 0.0;
                    y += tile_h;
                }
                places.push(vec2(x, y));
                x += w + 6.0;
            }
            let (_, area) = ui.allocate_space(vec2(width, y + tile_h));
            for (row, at) in p.rows.iter().zip(places) {
                let tile = Rect::from_min_size(area.min + at, vec2(tile_width(p.icon_h, row.unit_w), tile_h));
                parent_tile(app, ui, doc, p, row, tile, &lang);
            }
            let (_, new) = ui.allocate_space(vec2(ui.available_width(), NEW_PARENT_H));
            let label = crate::i18n::tr(&lang, "New Parent").to_owned();
            if Button::new(label).id_salt("pages_new_parent").variant(Variant::GHOST).show_at(ui, new).clicked()
                && let Ok(made) = app.run("layout.parents.new", json!({}))
            {
                select_parent(app, made.get("id").and_then(serde_json::Value::as_u64).map(SpreadId));
            }
        },
    );
}

/// One parent's tile.
fn parent_tile(app: &mut DesignApp, ui: &mut Ui, doc: &designcraft_doc::Document, p: &Plan, row: &ParentRow, tile: Rect, lang: &str) {
    let ctx = ui.ctx().clone();
    let t = Tokens::get(&ctx);
    let id = row.parent.map(|(_, id)| id);
    let active = row.parent.is_some_and(|(i, id)| p.cur_parent == Some(i) || p.selected_parent == Some(id));
    let tex =
        (p.parent_thumbnails && ui.is_rect_visible(tile)).then_some(id).flatten().and_then(|id| thumb(app, &ctx, ThumbOf::Parent(id), p.icon_h));
    let (pages, h, unit_w) = (row.pages.clone(), p.icon_h, row.unit_w);
    let short = row.prefix.clone().unwrap_or_else(|| row.label.clone());
    let (selected_fill, hover_fill, ink, border) = (t.row_selected, t.hover, t.text_strong, t.border);
    let out = PageCell::button(row.label.clone())
        .id_salt(("pages_parent", id.map(|id| id.0)))
        .chosen(active)
        .drags(PageDrag::Parent(id), row.label.clone())
        .tooltip(row.label.clone())
        .painted(move |painter, rect, look| {
            if active {
                painter.rect_filled(rect, 0.0, selected_fill);
            } else if look.hovered {
                painter.rect_filled(rect, 0.0, hover_fill);
            }
            let icon = Rect::from_min_size(pos2(rect.center().x - h * unit_w / 2.0, rect.min.y + 3.0), vec2(h * unit_w, h));
            paint_parent_icon(painter, icon, tex.as_ref(), &pages, h, border);
            painter.text(pos2(rect.center().x, rect.max.y - 9.0), egui::Align2::CENTER_CENTER, short, egui::FontId::proportional(11.0), ink);
        })
        .show_at(ui, tile);
    if let Some(click) = out.click {
        select_parent(app, id);
        if click.double
            && let Some(id) = id
        {
            let _ = app.run("layout.parents.edit", json!({"on": true, "parent": id.0}));
        }
    }
    if let Some(id) = id
        && let Some(act) =
            context_menu(ui, crate::i18n::tr(lang, "Parent Pages").to_owned(), &out.response, || parent_entries(lang, doc, Some(id), false))
    {
        act.run(app, doc, &p.layouts);
    }
}

// ---------------------------------------------------------------------------------------------------------------------
// Pages

/// The document's spreads, each with its page thumbnails, as InDesign's, in a scroll area of their own. Alternate layouts
/// stand side by side, each a column with its header.
fn pages(app: &mut DesignApp, ui: &mut Ui, doc: &designcraft_doc::Document, p: &Plan, drop: &mut Option<PageDrop>) {
    let area = if p.columns { egui::ScrollArea::both() } else { egui::ScrollArea::vertical() };
    area.id_salt("pages_spreads").max_height(p.pages_h).min_scrolled_height(p.pages_h).auto_shrink([false, false]).show(ui, |ui| {
        ui.add_space(4.0);
        if p.columns {
            layout_columns(app, ui, doc, p, drop);
        } else {
            spreads(app, ui, doc, p, 0..doc.spreads.len(), drop);
        }
        // Right-click on the empty part of the panel: the short menu.
        let rest = ui.available_rect_before_wrap();
        let (_, empty) = ui.allocate_space(vec2(ui.available_width(), rest.height().max(8.0)));
        let out = PageCell::plain().id_salt("pages_empty").show_at(ui, empty);
        let lang = app.ui.language.clone();
        let label = crate::i18n::tr(&lang, "Pages").to_owned();
        if let Some(act) = context_menu(ui, label, &out.response, || empty_entries(&lang, p.cur)) {
            act.run(app, doc, &p.layouts);
        }
    });
}

/// The alternate layouts side by side, as InDesign's: a column each, its header over it, and a column to add one more.
fn layout_columns(app: &mut DesignApp, ui: &mut Ui, doc: &designcraft_doc::Document, p: &Plan, drop: &mut Option<PageDrop>) {
    let scale = p.scale(doc);
    let current = layout_index(&p.layouts, p.cur);
    ui.horizontal_top(|ui| {
        ui.spacing_mut().item_spacing = vec2(6.0, 0.0);
        for (k, l) in p.layouts.iter().enumerate() {
            let range = spread_range(doc, l);
            let widest = doc.spreads.get(range.clone()).unwrap_or_default().iter().map(|sp| sp.bounds().width() as f32).fold(0.0, f32::max);
            let col_w = (widest * scale + 24.0).max(super::LAYOUT_COL_MIN);
            ui.vertical(|ui| {
                ui.set_width(col_w);
                layout_header(app, ui, doc, p, k, k == current);
                spreads(app, ui, doc, p, range, drop);
            });
        }
        let (_, rect) = ui.allocate_space(vec2(28.0, LAYOUT_HEADER_H));
        let label = crate::i18n::tr(&app.ui.language, "Create Alternate Layout…").to_owned();
        let glyph = PanelIcon("plus");
        let add = IconButton::new(IconSpec::glyph(&glyph))
            .label(label.clone())
            .id_salt("pages_create_layout")
            .variant(Variant::GHOST)
            .tooltip(label)
            .show_at(ui, rect);
        if add.response.clicked() {
            open_create_alternate(app, doc, &p.layouts, current);
        }
    });
}

/// An alternate layout column's header: its name and page size, and a menu (the arrow, or a right-click) to create,
/// duplicate, change or delete layouts. Double-click: Alternate Layout Options.
fn layout_header(app: &mut DesignApp, ui: &mut Ui, doc: &designcraft_doc::Document, p: &Plan, k: usize, current: bool) {
    let Some(l) = p.layouts.get(k) else { return };
    let lang = app.ui.language.clone();
    let t = Tokens::get(ui.ctx());
    let (_, rect) = ui.allocate_space(vec2(ui.available_width(), LAYOUT_HEADER_H));
    let (w, h) = layout_size(doc, l);
    let (name, size) = (l.name.clone(), format!("{} × {}", w.round(), h.round()));
    let (fill, strong, dim) = (if current { t.row_selected } else { t.panel_darker }, t.text_strong, t.text_dim);
    let header = PageCell::button(l.name.clone())
        .id_salt(("pages_layout", k))
        .chosen(current)
        .painted(move |painter, rect, _| {
            painter.rect_filled(rect.shrink2(vec2(0.0, 2.0)), 2.0, fill);
            let galley = painter.layout(name.clone(), crate::theme::semibold(11.0), strong, (rect.width() - 26.0).max(8.0));
            let galley =
                if galley.rows.len() > 1 { painter.layout_no_wrap(truncate(&name, 14), crate::theme::semibold(11.0), strong) } else { galley };
            painter.galley(rect.min + vec2(5.0, 5.0), galley, strong);
            painter.text(rect.min + vec2(5.0, 20.0), egui::Align2::LEFT_TOP, size, egui::FontId::proportional(10.0), dim);
        })
        .show_at(ui, rect);
    let label = crate::i18n::tr(&lang, "Layout Options").to_owned();
    let glyph = PanelIcon("chevron-down");
    let arrow_at = Rect::from_min_size(pos2(rect.max.x - 20.0, rect.min.y + 4.0), vec2(18.0, LAYOUT_HEADER_H - 8.0));
    let arrow = IconButton::new(IconSpec::glyph(&glyph))
        .label(format!("{label}: {}", l.name))
        .id(header.response.id.with("menu-button"))
        .variant(Variant::GHOST)
        .popup(PopupKind::Menu)
        .show_at(ui, arrow_at)
        .response;
    let mut chosen = None;
    let _ = Menu::new(label)
        .id(header.response.id.with("layout-menu"))
        .opened_by(&arrow, Trigger::Click)
        .opened_by(&header.response, Trigger::Context)
        .show(ui, |ui, _| menu_rows(ui, &layout_entries(&lang, p.layouts.len(), k), &mut chosen));
    if let Some(act) = chosen {
        act.run(app, doc, &p.layouts);
    }
    if let Some(click) = header.click {
        if click.double {
            open_layout_options(app, doc, &p.layouts, k);
        } else {
            // As a page click: leave Edit Parents first, so the view moves on document pages.
            if p.editing_parents {
                leave_parent_edit(app);
            }
            crate::canvas::go_to_page(app, l.start);
        }
    }
}

/// The document's spreads `range`, each a row centred on its spine.
fn spreads(app: &mut DesignApp, ui: &mut Ui, doc: &designcraft_doc::Document, p: &Plan, range: Range<usize>, drop: &mut Option<PageDrop>) {
    let followed_id = egui::Id::unique("pages_panel_followed");
    let center = ui.available_width() / 2.0;
    // One scale for the whole document: each row is as tall as its spread's tallest page.
    let scale = p.scale(doc);
    for (si, sp) in doc.spreads.iter().enumerate().take(range.end).skip(range.start) {
        let first = doc.first_page_of_spread(si);
        let row_h = sp.pages.iter().map(|pg| pg.height as f32 * scale).fold(0.0, f32::max).max(p.th * 0.25);
        let spine = sp.spine_x() as f32;
        let (_, row) = ui.allocate_space(vec2(ui.available_width(), 8.0 + row_h + 22.0));
        let top = row.min.y + 8.0;
        if p.cur_spread == Some(si) && !p.editing_parents {
            let key = (p.uid, p.cur);
            if ui.data(|d| d.get_temp::<(u64, usize)>(followed_id)) != Some(key) {
                ui.scroll_to_rect(row, None);
                ui.data_mut(|d| d.insert_temp(followed_id, key));
            }
        }
        // Only the rows in view are shown: their thumbnails, markers and menus.
        if !ui.is_rect_visible(row) {
            continue;
        }
        for (pi, pg) in sp.pages.iter().enumerate() {
            let abs = first + pi;
            let x0 = row.min.x + center + (pg.x as f32 - spine) * scale;
            let at = Rect::from_min_size(pos2(x0, top), vec2(pg.width as f32 * scale, pg.height as f32 * scale));
            // The parent's prefix, in the outer top corner (black on white).
            let prefix = pg
                .parent
                .and_then(|pid| doc.parents.iter().find(|x| x.id == pid))
                .map(|pp| (pp.parent.as_ref().map(|i| i.prefix.clone()).unwrap_or_default(), pg.side == designcraft_doc::PageSide::Left));
            page(app, ui, doc, p, Thumbnail { abs, si, at, prefix }, drop);
            if abs == 0 || doc.sections.iter().any(|s| s.start == abs) {
                section_marker(app, ui, p, abs, pos2(at.center().x, row.min.y + 4.0));
            }
        }
        spread_badge(app, ui, doc, p, (si, first), pos2(row.min.x + center, top + row_h + 11.0));
    }
}

/// A page's thumbnail as the panel places it: the page and its spread, where it lies, and its parent's prefix with
/// whether the badge is at the left.
struct Thumbnail {
    abs: usize,
    si: usize,
    at: Rect,
    prefix: Option<(String, bool)>,
}

/// One page's thumbnail: a click goes to it, a drag moves or copies it, a page or a parent dropped on it reorders or
/// applies, and its menu is the page's.
fn page(app: &mut DesignApp, ui: &mut Ui, doc: &designcraft_doc::Document, p: &Plan, shown: Thumbnail, drop: &mut Option<PageDrop>) {
    let Thumbnail { abs, si, at, prefix } = shown;
    let ctx = ui.ctx().clone();
    let selected = abs == p.cur && !p.editing_parents;
    let name = app.session.page_label(abs);
    let tex = (p.thumbnails && ui.is_rect_visible(at)).then(|| thumb(app, &ctx, ThumbOf::Page(abs), p.th)).flatten();
    let out = PageCell::button(format!("Page {name}"))
        .id_salt(("pages_page", abs))
        .chosen(selected)
        .drags(PageDrag::Page(abs), name)
        .takes(takes_any)
        .painted(move |painter, rect, look| {
            let tint = if selected { SELECTED_TINT } else { Color32::WHITE };
            match &tex {
                Some(tex) => {
                    painter.image(tex.id(), rect, full_uv(), tint);
                }
                None => {
                    painter.rect_filled(rect, 0.0, tint);
                }
            }
            painter.rect_stroke(rect, 0.0, Stroke::new(0.5, Color32::from_gray(0x4a)), StrokeKind::Inside);
            if let Some((prefix, left)) = prefix {
                let galley = painter.layout_no_wrap(prefix, crate::theme::semibold(9.5), Color32::BLACK);
                let w = galley.size().x + 2.0;
                let badge = if left {
                    Rect::from_min_size(rect.left_top(), vec2(w, 10.0))
                } else {
                    Rect::from_min_size(rect.right_top() - vec2(w, 0.0), vec2(w, 10.0))
                };
                painter.rect_filled(badge, 0.0, Color32::WHITE);
                painter.galley(badge.min + vec2(1.0, -0.5), galley, Color32::BLACK);
            }
            if look.over {
                painter.rect_stroke(rect, 0.0, Stroke::new(2.0, BADGE), StrokeKind::Inside);
            }
        })
        .show_at(ui, at);
    if out.click.is_some() {
        select_parent(app, None);
        if p.editing_parents {
            leave_parent_edit(app);
        }
        crate::canvas::go_to_page(app, abs);
    }
    if let Some((payload, op)) = out.dropped {
        *drop = Some((payload, abs, op == DropOperation::Copy));
    }
    let label = crate::i18n::tr(&app.ui.language, "Pages").to_owned();
    if let Some(act) = context_menu(ui, label, &out.response, || page_entries(app, doc, &p.layouts, abs, si)) {
        act.run(app, doc, &p.layouts);
    }
}

/// The marker over the first page of a section: the keyboard, a screen reader or a double click opens the page's
/// numbering options (a single pointer click only selects, as on a page).
fn section_marker(app: &mut DesignApp, ui: &mut Ui, p: &Plan, abs: usize, center: Pos2) {
    let label =
        format!("{}: {}", crate::i18n::tr(&app.ui.language, "Numbering & Section Options…").trim_end_matches('…'), app.session.page_label(abs));
    let ink = Tokens::get(ui.ctx()).icon;
    let out = PageCell::button(label)
        .id_salt(("pages_section", abs))
        .painted(move |painter, rect, _| {
            let c = rect.center();
            painter.add(egui::Shape::convex_polygon(vec![c + vec2(-4.5, -3.0), c + vec2(4.5, -3.0), c + vec2(0.0, 3.0)], ink, Stroke::NONE));
        })
        .show_at(ui, Rect::from_center_size(center, vec2(12.0, 8.0)));
    if out.click.is_some_and(|click| click.double || click.by_key) {
        if p.editing_parents {
            leave_parent_edit(app);
        }
        crate::canvas::go_to_page(app, abs);
        crate::menus::activate(app, "layout.section", &serde_json::Value::Null);
    }
}

/// A spread's label under its thumbnails, with the badge on the spread of the current page; a click goes to the spread.
fn spread_badge(app: &mut DesignApp, ui: &mut Ui, doc: &designcraft_doc::Document, p: &Plan, (si, first): (usize, usize), center: Pos2) {
    let label = spread_label(app, doc, first, doc.spreads[si].pages.len());
    // White on the badge; the theme's text colour elsewhere, so it reads on a light panel.
    let badge = p.cur_spread == Some(si) && !p.editing_parents;
    let ink = if badge { Color32::WHITE } else { Tokens::get(ui.ctx()).text_strong };
    let galley = ui.painter().layout_no_wrap(label.clone(), crate::theme::semibold(11.0), ink);
    let at = Rect::from_center_size(center, vec2(galley.size().x + 7.0, 12.5));
    let out = PageCell::button(label)
        .id_salt(("pages_spread_label", si))
        .painted(move |painter, rect, _| {
            if badge {
                painter.rect_filled(rect, 0.0, BADGE);
            }
            painter.galley(rect.center() - galley.size() / 2.0, galley, ink);
        })
        .show_at(ui, at);
    if out.click.is_some() {
        if p.editing_parents {
            leave_parent_edit(app);
        }
        crate::canvas::go_to_page(app, first);
    }
}

// ---------------------------------------------------------------------------------------------------------------------
// Footer

/// What a pick in the Edit page size menu does.
enum SizePick {
    /// A preset's size for the current page, in its orientation.
    Preset(f64, f64),
    /// Custom Page Size…, from the page's size.
    Custom(f64, f64),
}

/// An action of the footer.
fn footer_button(ui: &mut Ui, icon: &'static str, selected: bool, label: &str, id: egui::Id, popup: bool) -> egui::Response {
    let glyph = PanelIcon(icon);
    let mut button = IconButton::new(IconSpec::glyph(&glyph))
        .label(label.to_owned())
        .id(id)
        .size(craft_theme::ControlSize::Sm)
        .variant(Variant::GHOST)
        .selected(selected)
        .tooltip(label.to_owned());
    if popup {
        button = button.popup(PopupKind::Menu);
    }
    button.show(ui).response
}

/// The rows of the Edit page size menu: InDesign's presets for page `cur`, in its orientation, then Custom Page Size….
fn size_rows(ui: &mut Ui, lang: &str, doc: &designcraft_doc::Document, cur: usize) -> Option<SizePick> {
    let (si, pi) = doc.page_loc(cur)?;
    let pg = &doc.spreads[si].pages[pi];
    let landscape = pg.width > pg.height;
    let current = crate::panels::properties::preset_name(pg.width, pg.height);
    let mut picked = None;
    for (name, w, h) in crate::panels::properties::PAGE_PRESETS {
        let (w, h) = if landscape { (h, w) } else { (w, h) };
        if MenuItem::new(crate::i18n::tr(lang, name).to_owned()).id_salt(*name).radio_mark(current == *name).show(ui).ran {
            picked = Some(SizePick::Preset(*w, *h));
        }
    }
    Divider::horizontal().show(ui);
    if MenuItem::new(crate::i18n::tr(lang, "Custom Page Size…").to_owned()).id_salt("custom").show(ui).ran {
        picked = Some(SizePick::Custom(pg.width, pg.height));
    }
    picked
}

/// The footer: the page count and the buttons, always at the panel's bottom.
pub(super) fn footer(app: &mut DesignApp, ui: &mut Ui, doc: &designcraft_doc::Document, cur: usize) {
    let direction = if crate::i18n::is_rtl(&app.ui.language) { Direction::Rtl } else { Direction::Ltr };
    with_env(ui, |scope| scope.direction = Some(direction), |ui| footer_contents(app, ui, doc, cur));
}

fn footer_contents(app: &mut DesignApp, ui: &mut Ui, doc: &designcraft_doc::Document, cur: usize) {
    // A page has an absolute index: retire held input when its document, revision or current-page target changes.
    let target = app.session.active().map(|st| (st.uid, st.identity_epoch, st.revision, st.editing_parents));
    let footer_id = ui.scope_id().with(("pages-footer", target, cur));
    let lang = app.ui.language.clone();
    let tr = |s: &str| crate::i18n::tr(&lang, s).to_owned();
    let dragging = drag::active::<PageDrag>(ui.ctx()).is_some();
    ui.horizontal(|ui| {
        ui.set_min_height(FOOTER_H - 4.0);
        let counts = tr("{pages} Pages in {spreads} Spreads")
            .replace("{pages}", &doc.page_count().to_string())
            .replace("{spreads}", &doc.spreads.len().to_string());
        Text::new(counts).role(TypeRole::Caption).shade(Shade::Muted).show(ui);
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            if footer_button(ui, "trash", false, &tr("Delete selected pages"), footer_id.with("trash"), false).clicked() {
                let _ = crate::dialogs::delete_pages(app, vec![cur]);
            }
            // Create new page; a page dropped on it is duplicated at the end of the document.
            let create = tr("Create new page");
            let shown = DropAction::new(|payload: &PageDrag| matches!(payload, PageDrag::Page(_)))
                .id(footer_id.with("plus-drop"))
                .show(ui, |ui, _| footer_button(ui, "plus", dragging, &create, footer_id.with("plus"), false));
            let plus = shown.inner.unwrap_or_else(|| shown.output.response.clone());
            match shown.output.dropped {
                Some(PageDrag::Page(from)) => {
                    if let Some((si, _)) = doc.page_loc(from) {
                        let _ = app.run("layout.pages.duplicateSpread", json!({"spread": si, "atEnd": true}));
                    }
                }
                Some(PageDrag::Parent(_)) => {}
                None => {
                    if plus.clicked() {
                        let _ = app.run("layout.pages.insert", json!({"after": cur, "count": 1}));
                    }
                }
            }
            // Edit page size: InDesign's presets for the current page, or Custom Page Size….
            let size = footer_button(ui, "panel-pages", false, &tr("Edit page size"), footer_id.with("panel-pages"), true);
            let picked = Menu::new(tr("Edit page size"))
                .id(footer_id.with("size-menu"))
                .opened_by(&size, Trigger::Click)
                .show(ui, |ui, _| size_rows(ui, &lang, doc, cur))
                .inner
                .flatten();
            match picked {
                Some(SizePick::Preset(w, h)) => {
                    let _ = app.run("layout.pageSize", json!({"pages": [cur + 1], "width": w, "height": h}));
                }
                Some(SizePick::Custom(width, height)) => {
                    app.ui.dialog =
                        Some(crate::dialogs::Dialog::new("cmd:layout.pageSize", json!({"pages": [cur + 1], "width": width, "height": height})));
                }
                None => {}
            }
        });
    });
}
