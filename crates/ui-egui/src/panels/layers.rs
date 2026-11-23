//! Layers panel: expandable layer rows listing the current spread's objects topmost first
//! (`<rectangle>`, `<first words…>` for text frames; groups expand to their members), with eye /
//! lock columns per layer and per object and a selection square in the layer colour. Drag a
//! layer row to reorder layers, an object row to restack it or move it to another layer (group
//! members restack within their group), and a filled selection square onto another layer to move
//! the selection there. A selected row drags the selected objects that share its parent (for a
//! top-level row, those on its layer). Locked and hidden layers take drops only with Cmd/Ctrl
//! held.
//!
//! With the craft UI framework installed the panel is one framework `List` over the same rows (see `framework`): the
//! rows, drops, rename in place and menu run the engine commands below; the native rows stay for the browser build.

use designcraft_doc::{Content, Document, Item, ItemId, LayerId};
use egui::{Color32, Rect, Sense, Stroke, StrokeKind, Ui, pos2, vec2};
use serde_json::json;

use crate::theme::Tokens;
use crate::{DesignApp, icons};

const ROW_H: f32 = 24.0;
/// Width of the eye and lock columns.
const COL_W: f32 = 22.0;

/// Display name of an item: its own name, the first words of a text frame's story, the file a
/// graphic frame shows (as InDesign lists placed images), or the kind.
pub fn item_label(doc: &Document, it: &Item) -> String {
    if !it.name.is_empty() {
        return it.name.clone();
    }
    if let Content::Text(tf) = &it.content
        && let Some(story) = doc.story(tf.story)
    {
        let words: String =
            story.text.chars().map(|c| if c.is_control() || (c as u32) >= 0xE000 && (c as u32) <= 0xF8FF { ' ' } else { c }).collect();
        let words = words.split_whitespace().collect::<Vec<_>>().join(" ");
        if !words.is_empty() {
            // Whole words up to about 24 characters.
            let mut short = String::new();
            for w in words.split(' ') {
                if !short.is_empty() && short.chars().count() + 1 + w.chars().count() > 24 {
                    break;
                }
                if !short.is_empty() {
                    short.push(' ');
                }
                short.push_str(w);
            }
            let short: String = short.chars().take(24).collect();
            let ell = if short.chars().count() < words.chars().count() { "…" } else { "" };
            return format!("<{short}{ell}>");
        }
    }
    if let Some(name) = it.graphic().and_then(|g| doc.assets.get(&g.asset)).map(|a| a.name.trim()).filter(|n| !n.is_empty()) {
        return format!("<{name}>");
    }
    it.default_label().to_string()
}

fn c32(c: [u8; 3]) -> Color32 {
    Color32::from_rgb(c[0], c[1], c[2])
}

/// Eye and lock cells (with column rules). Returns (eye clicked, lock clicked).
fn eye_lock(
    ui: &mut Ui,
    language: &str,
    row: Rect,
    visible: bool,
    locked: bool,
    dim: bool,
    key: impl std::hash::Hash + std::fmt::Debug + Copy,
) -> (bool, bool) {
    let t = Tokens::get(ui.ctx());
    let eye = Rect::from_min_size(row.min, vec2(COL_W, ROW_H));
    let lock = Rect::from_min_size(row.min + vec2(COL_W, 0.0), vec2(COL_W, ROW_H));
    for r in [eye, lock] {
        ui.painter().line_segment([r.right_top(), r.right_bottom()], Stroke::new(1.0, t.border));
    }
    let col = if dim { t.text_disabled } else { t.icon };
    if visible {
        icons::paint(ui.painter(), Rect::from_center_size(eye.center(), vec2(15.0, 15.0)), "eye", col);
    }
    if locked {
        icons::paint(ui.painter(), Rect::from_center_size(lock.center(), vec2(13.0, 13.0)), "lock", col);
    }
    let e = ui.interact(eye, ui.scope_id().with(("eye", key)), Sense::click()).on_hover_ui(|ui| {
        crate::rtl::label(ui, crate::i18n::tr(language, if visible { "Hide" } else { "Show" }));
    });
    let l = ui.interact(lock, ui.scope_id().with(("lock", key)), Sense::click()).on_hover_ui(|ui| {
        crate::rtl::label(ui, crate::i18n::tr(language, if locked { "Unlock" } else { "Lock" }));
    });
    (e.clicked(), l.clicked())
}

/// The selection square at the right of a row: filled = selected; hollow on hover. Click to
/// select; when filled, drag it to another layer.
fn selection_square(
    ui: &mut Ui,
    language: &str,
    row: Rect,
    color: Color32,
    selected: bool,
    small: bool,
    key: impl std::hash::Hash + std::fmt::Debug,
) -> egui::Response {
    let s = if small { 7.0 } else { 8.0 };
    let r = Rect::from_center_size(pos2(row.max.x - 12.0, row.center().y), vec2(s, s));
    let sense = if selected && !small { Sense::click_and_drag() } else { Sense::click() };
    let resp = ui.interact(r.expand(4.0), ui.scope_id().with(("selsq", key)), sense);
    if selected {
        ui.painter().rect_filled(r, 0.0, color);
    } else if resp.hovered() {
        ui.painter().rect_stroke(r, 0.0, Stroke::new(1.0, color), StrokeKind::Inside);
    }
    let tip = if selected && !small { "Select; drag to move the selection to another layer" } else { "Select" };
    resp.on_hover_ui(|ui| {
        crate::rtl::label(ui, crate::i18n::tr(language, tip));
    })
}

/// What a drag in the Layers panel carries.
#[derive(Clone, Debug, PartialEq)]
enum LayersDrag {
    Layer(LayerId),
    /// Objects that share one parent: the spread (`group: None`) or a group.
    Objects {
        ids: Vec<ItemId>,
        group: Option<ItemId>,
    },
    /// The selected objects, from a layer's selection square.
    Selection,
}

/// Where a drag would land.
#[derive(Clone, Copy, Debug, PartialEq)]
enum DropAt {
    /// The layer slot at this index of `doc.layers` (0 = top).
    LayerSlot(usize),
    /// Onto a layer: to the top of its stack.
    OntoLayer(LayerId),
    /// In front of (`above`) or behind an object.
    Object { id: ItemId, above: bool },
}

enum RowKind {
    Layer,
    Object { id: ItemId, group: Option<ItemId> },
}

/// A laid-out row, for drop targeting.
struct Row {
    rect: Rect,
    layer: LayerId,
    /// Index of `layer` in `doc.layers`.
    layer_index: usize,
    /// The layer is locked or hidden.
    closed: bool,
    kind: RowKind,
    /// Bottom of the row's block: the last row of an open layer or group, else the row itself.
    end: f32,
}

enum Indicator {
    Line(f32),
    Box(Rect),
}

/// The drop for `drag` with the pointer at `p`, and how to show it. `None` where it can't land.
/// Objects land on a locked or hidden layer only with `force` (Cmd/Ctrl held).
fn drop_target(drag: &LayersDrag, rows: &[Row], p: egui::Pos2, force: bool) -> Option<(DropAt, Indicator)> {
    let r = rows.iter().find(|r| r.rect.contains(p))?;
    let header = rows.iter().find(|h| h.layer == r.layer && matches!(h.kind, RowKind::Layer))?;
    if r.closed && !force && !matches!(drag, LayersDrag::Layer(_)) {
        return None;
    }
    match drag {
        LayersDrag::Layer(src) => {
            let before = p.y < (header.rect.min.y + header.end) / 2.0;
            let slot = if before { r.layer_index } else { r.layer_index + 1 };
            let from = rows.iter().find(|h| h.layer == *src && matches!(h.kind, RowKind::Layer))?.layer_index;
            if slot == from || slot == from + 1 {
                return None;
            }
            Some((DropAt::LayerSlot(slot), Indicator::Line(if before { header.rect.min.y } else { header.end })))
        }
        LayersDrag::Selection => Some((DropAt::OntoLayer(r.layer), Indicator::Box(header.rect))),
        LayersDrag::Objects { ids, group } => match r.kind {
            RowKind::Layer if group.is_none() => Some((DropAt::OntoLayer(r.layer), Indicator::Box(r.rect))),
            RowKind::Object { id, group: g } if g == *group && !ids.contains(&id) => {
                let above = p.y < r.rect.center().y;
                Some((DropAt::Object { id, above }, Indicator::Line(if above { r.rect.min.y } else { r.end })))
            }
            _ => None,
        },
    }
}

/// The command a drop runs.
fn drop_command(drag: &LayersDrag, at: DropAt, doc: &Document, selected: &[ItemId]) -> Option<(&'static str, serde_json::Value)> {
    let ids = |ids: &[ItemId]| ids.iter().map(|i| i.0).collect::<Vec<_>>();
    match (drag, at) {
        (LayersDrag::Layer(id), DropAt::LayerSlot(slot)) => {
            let from = doc.layers.iter().position(|l| l.id == *id)?;
            let to = if slot > from { slot - 1 } else { slot };
            Some(("layer.move", json!({"id": id.0, "to": to})))
        }
        (LayersDrag::Selection, DropAt::OntoLayer(l)) => {
            // Group members move with their group.
            let mut tops: Vec<ItemId> = Vec::new();
            for t in selected.iter().filter_map(|i| doc.top_level_of(*i)) {
                if !tops.contains(&t) {
                    tops.push(t);
                }
            }
            let moves = tops.iter().any(|i| doc.item(*i).is_some_and(|it| it.layer != l));
            moves.then(|| ("object.setLayer", json!({"layer": l.0, "ids": ids(&tops)})))
        }
        (LayersDrag::Objects { ids: o, .. }, DropAt::OntoLayer(l)) => Some(("object.reorder", json!({"ids": ids(o), "layer": l.0, "index": 0}))),
        (LayersDrag::Objects { ids: o, .. }, DropAt::Object { id, above }) => {
            Some(("object.reorder", json!({"ids": ids(o), if above { "above" } else { "below" }: id.0})))
        }
        _ => None,
    }
}

/// Nesting limit for listing group members.
const MAX_DEPTH: usize = 32;

/// Per-frame inputs shared by the object rows.
struct Listing<'a> {
    doc: &'a Document,
    language: &'a str,
    selected: &'a [ItemId],
    /// The top-level objects of the selection (see `show`).
    shown: &'a [ItemId],
    /// A new selection: its first row scrolls into view.
    reveal: bool,
    scrolled: &'a std::cell::Cell<bool>,
    layer: LayerId,
    layer_index: usize,
    layer_color: Color32,
    layer_visible: bool,
    layer_closed: bool,
}

/// Rows for `items` (topmost first) under `group` at `depth`; the commands their clicks run go
/// to `cmds`.
fn object_rows(
    ui: &mut Ui,
    ls: &Listing,
    items: &[std::sync::Arc<Item>],
    group: Option<ItemId>,
    depth: usize,
    rows: &mut Vec<Row>,
    cmds: &mut Vec<(&'static str, serde_json::Value)>,
) {
    let t = Tokens::get(ui.ctx());
    for it in items.iter().rev() {
        let (row, resp) = ui.allocate_exact_size(vec2(ui.available_width(), ROW_H), Sense::click_and_drag());
        let is_sel = ls.selected.contains(&it.id);
        if is_sel || ls.shown.contains(&it.id) {
            ui.painter().rect_filled(row, 0.0, t.row_selected);
            if ls.reveal && !ls.scrolled.get() {
                resp.scroll_to_me(Some(egui::Align::Center));
                ls.scrolled.set(true);
            }
        } else if resp.hovered() {
            ui.painter().rect_filled(row, 0.0, t.hover);
        }
        let (eye, lock) = eye_lock(ui, ls.language, row, !it.hidden, it.locked, !ls.layer_visible, ("item", it.id.0));
        let indent = 14.0 * depth as f32;
        let expandable = !it.children().is_empty() && it.states.is_empty() && depth < MAX_DEPTH;
        let open_id = egui::Id::unique(("group_open", it.id.0));
        let open = expandable && ui.data(|d| d.get_temp(open_id)).unwrap_or(false);
        let mut tri_clicked = false;
        if expandable {
            let tri = Rect::from_min_size(row.min + vec2(2.0 * COL_W + 14.0 + indent, 0.0), vec2(14.0, ROW_H));
            disclosure(ui, tri, open, t.icon);
            tri_clicked = ui.interact(tri, ui.scope_id().with(("gtri", it.id.0)), Sense::click()).clicked();
        }
        let name = item_label(ls.doc, it);
        resp.widget_info(|| egui::WidgetInfo::selected(egui::Role::Button, true, is_sel, &name));
        let field = Rect::from_min_max(row.min + vec2(2.0 * COL_W + 28.0 + indent, 3.0), pos2(row.max.x - 28.0, row.max.y - 3.0));
        let renaming = match rename_field(ui, field, it.id.0) {
            // An empty name goes back to the automatic label.
            Some(Some(new)) => {
                if new != it.name {
                    cmds.push(("object.rename", json!({"id": it.id.0, "name": new})));
                }
                false
            }
            Some(None) => false,
            None => ui.data(|d| d.get_temp::<RenameState>(egui::Id::unique(RENAME))).is_some_and(|(k, ..)| k == it.id.0),
        };
        if !renaming {
            let clip = Rect::from_min_max(row.min, pos2(row.max.x - 26.0, row.max.y));
            ui.painter().with_clip_rect(clip).text(
                row.min + vec2(2.0 * COL_W + 30.0 + indent, ROW_H / 2.0),
                egui::Align2::LEFT_CENTER,
                &name,
                egui::FontId::proportional(11.0),
                if it.hidden { t.text_dim } else { t.text },
            );
        }
        let sq = selection_square(ui, ls.language, row, ls.layer_color, is_sel, true, ("item", it.id.0)).clicked();
        // Dragging a selected row drags the selected objects that share its parent (top-level
        // rows: those on this layer).
        let dragged: Vec<ItemId> = if is_sel {
            let siblings: &[std::sync::Arc<Item>] = items;
            siblings.iter().map(|i| i.id).filter(|i| ls.selected.contains(i)).collect()
        } else {
            vec![it.id]
        };
        resp.dnd_set_drag_payload(LayersDrag::Objects { ids: dragged, group });
        if resp.dragged() {
            super::pages::drag_ghost(ui, &name, &t);
        }
        if eye {
            cmds.push(("object.setFlags", json!({"ids": [it.id.0], "hidden": !it.hidden})));
        } else if lock {
            cmds.push(("object.setFlags", json!({"ids": [it.id.0], "locked": !it.locked})));
        } else if tri_clicked {
            ui.data_mut(|d| d.insert_temp(open_id, !open));
        } else if sq || resp.clicked() {
            // As InDesign's: a slow double-click (a second click on the selected object's row,
            // after the double-click delay) renames it; a quick double-click only selects.
            let (now, add) = ui.input(|i| (i.time, i.modifiers.shift || i.modifiers.command));
            let delay = ui.ctx().options(|o| o.input_options.max_double_click_delay);
            let last_id = egui::Id::unique(("layers_last_click", it.id.0));
            let last: Option<f64> = ui.data(|d| d.get_temp(last_id));
            ui.data_mut(|d| d.insert_temp(last_id, now));
            let slow = !sq && !add && !resp.double_clicked() && last.is_some_and(|t| now - t > delay && now - t < SLOW_RENAME_WINDOW);
            if slow && (ls.selected == [it.id] || ls.shown == [it.id]) {
                // Its own name, or the label it shows without the brackets.
                let current = if it.name.is_empty() { name.trim_start_matches('<').trim_end_matches('>').to_string() } else { it.name.clone() };
                start_rename(ui, it.id.0, &current);
            } else {
                cmds.push(("selection.set", json!({"ids": [it.id.0], "add": add})));
            }
        }
        let at = rows.len();
        rows.push(Row {
            rect: row,
            layer: ls.layer,
            layer_index: ls.layer_index,
            closed: ls.layer_closed,
            kind: RowKind::Object { id: it.id, group },
            end: row.max.y,
        });
        if open {
            object_rows(ui, ls, it.children(), Some(it.id), depth + 1, rows, cmds);
            let end = rows.last().map_or(row.max.y, |r| r.end.max(r.rect.max.y));
            if let Some(r) = rows.get_mut(at) {
                r.end = end;
            }
        }
    }
}

/// A disclosure triangle in `r`: pointing down when open.
fn disclosure(ui: &Ui, r: Rect, open: bool, color: Color32) {
    let c = r.center();
    let pts = if open {
        vec![c + vec2(-4.0, -2.0), c + vec2(4.0, -2.0), c + vec2(0.0, 3.0)]
    } else {
        vec![c + vec2(-2.0, -4.0), c + vec2(3.0, 0.0), c + vec2(-2.0, 4.0)]
    };
    ui.painter().add(egui::Shape::convex_polygon(pts, color, Stroke::NONE));
}

/// The panel: framework components when the framework is installed, the native rows in the browser build.
pub fn show(app: &mut DesignApp, ui: &mut Ui) {
    #[cfg(not(target_arch = "wasm32"))]
    if app.framework_installed {
        framework::show(app, ui);
        return;
    }
    show_native(app, ui);
}

fn show_native(app: &mut DesignApp, ui: &mut Ui) {
    let t = Tokens::get(ui.ctx());
    let Some(st) = app.session.active() else { return };
    let doc = st.doc.clone();
    let active = st.active_layer;
    let selected: Vec<ItemId> = st.selection.items.clone();
    let sel_layers: Vec<_> = selected.iter().filter_map(|i| doc.item(*i).map(|it| it.layer)).collect();
    let language = app.ui.language.clone();
    // The rows a selection reveals: an object selected inside a closed group shows as its group.
    let shown: Vec<ItemId> = selected.iter().map(|i| doc.top_level_of(*i).unwrap_or(*i)).collect();
    // A new selection (made on the canvas or anywhere else) is revealed: its layer opens and its row
    // scrolls into view, once, so the panel can still be scrolled and closed freely afterwards.
    let last_id = egui::Id::unique("layers_revealed_selection");
    let now: Vec<u64> = shown.iter().map(|i| i.0).collect();
    let reveal = !now.is_empty() && ui.data(|d| d.get_temp::<Vec<u64>>(last_id)).as_ref() != Some(&now);
    ui.data_mut(|d| d.insert_temp(last_id, now));
    let scrolled = std::cell::Cell::new(false);
    // Objects on the current spread, back to front.
    let cur = crate::canvas::current_page(app).unwrap_or(0);
    let spread_items: Vec<std::sync::Arc<Item>> =
        doc.page_loc(cur).and_then(|(si, _)| doc.spreads.get(si)).map(|sp| sp.items.clone()).unwrap_or_default();
    let mut rows: Vec<Row> = Vec::new();
    let mut cmds: Vec<(&'static str, serde_json::Value)> = Vec::new();
    ui.spacing_mut().item_spacing.y = 0.0;
    // `doc.layers` runs top (frontmost) to bottom, as the panel lists them.
    for (li, l) in doc.layers.iter().enumerate() {
        let open_id = egui::Id::unique(("layer_open", l.id.0));
        let mut open: bool = ui.data(|d| d.get_temp(open_id)).unwrap_or(l.id == active);
        if reveal && sel_layers.contains(&l.id) && !open {
            open = true;
            ui.data_mut(|d| d.insert_temp(open_id, true));
        }
        let lc = c32(l.color);
        let (row, resp) = ui.allocate_exact_size(vec2(ui.available_width(), ROW_H), Sense::click_and_drag());
        resp.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Button, true, &l.name));
        if l.id == active {
            ui.painter().rect_filled(row, 0.0, t.row_selected);
        } else if resp.hovered() {
            ui.painter().rect_filled(row, 0.0, t.hover);
        }
        let (eye, lock) = eye_lock(ui, &language, row, l.visible, l.locked, false, ("layer", l.id.0));
        let tri = Rect::from_min_size(row.min + vec2(2.0 * COL_W + 2.0, 0.0), vec2(14.0, ROW_H));
        disclosure(ui, tri, open, t.icon);
        let tri_r = ui.interact(tri, ui.scope_id().with(("tri", l.id.0)), Sense::click());
        // Layer colour chip and name.
        let chip = Rect::from_min_size(row.min + vec2(2.0 * COL_W + 18.0, 7.0), vec2(4.0, 10.0));
        ui.painter().rect_filled(chip, 0.0, lc);
        ui.painter().text(
            row.min + vec2(2.0 * COL_W + 28.0, ROW_H / 2.0),
            egui::Align2::LEFT_CENTER,
            &l.name,
            egui::FontId::proportional(11.5),
            t.text_strong,
        );
        if l.id == active {
            icons::paint(ui.painter(), Rect::from_center_size(pos2(row.max.x - 32.0, row.center().y), vec2(13.0, 13.0)), "tool-pen", t.icon);
        }
        let sq = selection_square(ui, &language, row, lc, sel_layers.contains(&l.id), false, ("layer", l.id.0));
        sq.dnd_set_drag_payload(LayersDrag::Selection);
        if sq.dragged() {
            super::pages::drag_ghost(ui, crate::i18n::tr(&language, "Move Selection Here"), &t);
        }
        resp.dnd_set_drag_payload(LayersDrag::Layer(l.id));
        if resp.dragged() {
            super::pages::drag_ghost(ui, &l.name, &t);
        }
        if eye {
            cmds.push(("layer.set", json!({"id": l.id.0, "visible": !l.visible})));
        } else if lock {
            cmds.push(("layer.set", json!({"id": l.id.0, "locked": !l.locked})));
        } else if tri_r.clicked() {
            ui.data_mut(|d| d.insert_temp(open_id, !open));
        } else if sq.clicked() {
            let ids: Vec<u64> = spread_items.iter().filter(|i| i.layer == l.id).map(|i| i.id.0).collect();
            cmds.push(("selection.set", json!({"ids": ids})));
        } else if resp.double_clicked() {
            // The first click of the pair activated the layer; the second opens its options.
            cmds.push(("app.layerOptionsDialog", json!({"id": l.id.0})));
        } else if resp.clicked() {
            cmds.push(("layer.activate", json!({"id": l.id.0})));
        }
        resp.context_menu(|ui| {
            let options = crate::i18n::tr(&language, "Layer Options for \"{name}\"…").replace("{name}", &l.name);
            if ui.button(crate::rtl::widget(ui, options)).clicked() {
                cmds.push(("app.layerOptionsDialog", json!({"id": l.id.0})));
                ui.close();
            }
            ui.separator();
            let duplicate = crate::i18n::tr(&language, "Duplicate Layer \"{name}\"").replace("{name}", &l.name);
            if ui.button(crate::rtl::widget(ui, duplicate)).clicked() {
                cmds.push(("layer.duplicate", json!({"id": l.id.0})));
                ui.close();
            }
            if ui.button(crate::rtl::widget(ui, crate::i18n::tr(&language, "Delete Layer"))).clicked() {
                cmds.push(("layer.delete", json!({"id": l.id.0})));
                ui.close();
            }
            if ui.button(crate::rtl::widget(ui, crate::i18n::tr(&language, "Move Selection Here"))).clicked() {
                cmds.push(("object.setLayer", json!({"layer": l.id.0})));
                ui.close();
            }
            ui.separator();
            if active != l.id && ui.button(crate::rtl::widget(ui, crate::i18n::tr(&language, "Merge into Active Layer"))).clicked() {
                cmds.push(("layer.merge", json!({"ids": [active.0, l.id.0], "into": active.0})));
                ui.close();
            }
            for (label, params) in [
                ("Hide Others", json!({"id": l.id.0, "hide": true})),
                ("Lock Others", json!({"id": l.id.0, "lock": true})),
                ("Show All Layers", json!({"id": l.id.0, "show": true})),
                ("Unlock All Layers", json!({"id": l.id.0, "unlock": true})),
            ] {
                if ui.button(crate::rtl::widget(ui, crate::i18n::tr(&language, label))).clicked() {
                    cmds.push(("layer.others", params));
                    ui.close();
                }
            }
            if ui.button(crate::rtl::widget(ui, crate::i18n::tr(&language, "Delete Unused Layers"))).clicked() {
                cmds.push(("layer.deleteUnused", json!({})));
                ui.close();
            }
        });
        let at = rows.len();
        rows.push(Row { rect: row, layer: l.id, layer_index: li, closed: l.locked || !l.visible, kind: RowKind::Layer, end: row.max.y });
        if !open {
            continue;
        }
        // The layer's objects keep their spread order; the panel lists them topmost first.
        let on_layer: Vec<std::sync::Arc<Item>> = spread_items.iter().filter(|i| i.layer == l.id).cloned().collect();
        let ls = Listing {
            doc: &doc,
            language: &language,
            selected: &selected,
            shown: &shown,
            reveal,
            scrolled: &scrolled,
            layer: l.id,
            layer_index: li,
            layer_color: lc,
            layer_visible: l.visible,
            layer_closed: l.locked || !l.visible,
        };
        object_rows(ui, &ls, &on_layer, None, 0, &mut rows, &mut cmds);
        let end = rows.last().map_or(row.max.y, |r| r.end.max(r.rect.max.y));
        if let Some(r) = rows.get_mut(at) {
            r.end = end;
        }
    }
    // Drag and drop: an indicator line between rows (a box around a layer row) while dragging;
    // one command on release.
    if let Some(drag) = egui::DragAndDrop::payload::<LayersDrag>(ui.ctx())
        && let Some(p) = ui.ctx().pointer_latest_pos()
        && let Some((at, indicator)) = drop_target(&drag, &rows, p, ui.input(|i| i.modifiers.command))
    {
        let x = ui.max_rect().x_range();
        match indicator {
            Indicator::Line(y) => {
                ui.painter().line_segment([pos2(x.min, y), pos2(x.max, y)], Stroke::new(2.0, t.accent));
            }
            Indicator::Box(r) => {
                ui.painter().rect_stroke(r, 0.0, Stroke::new(2.0, t.accent), StrokeKind::Inside);
            }
        }
        if ui.input(|i| i.pointer.any_released()) {
            egui::DragAndDrop::clear_payload(ui.ctx());
            if let Some(c) = drop_command(&drag, at, &doc, &selected) {
                cmds.push(c);
            }
        }
    }
    for (id, params) in cmds {
        let _ = app.run(id, params);
    }
    ui.add_space(6.0);
    ui.spacing_mut().item_spacing.y = 4.0;
    ui.horizontal(|ui| {
        ui.label(
            egui::RichText::new(format!(
                "{}: {}, {}",
                crate::i18n::tr(&app.ui.language, "Page"),
                app.session.page_label(cur),
                crate::i18n::count_label(&app.ui.language, if doc.layers.len() == 1 { "layer" } else { "layers" }, doc.layers.len())
            ))
            .size(11.0)
            .color(t.text_dim),
        );
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            if icons::button(ui, "trash", 20.0, false, crate::i18n::tr(&app.ui.language, "Delete Layer")).clicked() {
                let _ = app.run("layer.delete", json!({"id": active.0}));
            }
            // Create New Layer; a layer dropped on it is duplicated.
            let dragging = matches!(egui::DragAndDrop::payload::<LayersDrag>(ui.ctx()).as_deref(), Some(LayersDrag::Layer(_)));
            let plus = icons::button(ui, "plus", 20.0, dragging, crate::i18n::tr(&app.ui.language, "Create New Layer"));
            if let Some(payload) = plus.dnd_release_payload::<LayersDrag>() {
                if let LayersDrag::Layer(id) = *payload {
                    let _ = app.run("layer.duplicate", json!({"id": id.0}));
                }
            } else if plus.clicked() {
                let _ = app.run("layer.new", json!({}));
            }
        });
    });
}

/// Seconds within which a second click on a selected row renames it (after the double-click delay).
const SLOW_RENAME_WINDOW: f64 = 1.5;

/// The object whose row is being renamed.
type RenameKey = u64;
/// The row, the field's text, and the text it started with.
type RenameState = (RenameKey, String, String);
const RENAME: &str = "layers_rename";

/// The name field of the row being renamed (a layer's double-click, an object's slow one), in `r`: `Some(Some(name))` when Enter
/// or a click elsewhere confirms it with a changed text, `Some(None)` when Esc cancels or the text
/// is the one it started with, `None` while typing or when this row isn't being renamed.
fn rename_field(ui: &mut Ui, r: Rect, key: RenameKey) -> Option<Option<String>> {
    let id = egui::Id::unique(RENAME);
    let (k, mut text, start): RenameState = ui.data(|d| d.get_temp(id))?;
    if k != key {
        return None;
    }
    let edit = ui.put(r, egui::TextEdit::singleline(&mut text).font(egui::FontId::proportional(11.0)).margin(vec2(2.0, 1.0)));
    if !edit.has_focus() && !edit.lost_focus() {
        edit.request_focus();
    }
    let done = if ui.input(|i| i.key_pressed(egui::Key::Escape)) {
        Some(None)
    } else if edit.lost_focus() {
        // An unnamed object's field starts with its automatic label; confirming that text
        // unchanged must not turn the label into a name.
        let new = text.trim();
        if new == start.trim() { Some(None) } else { Some(Some(new.to_string())) }
    } else {
        None
    };
    ui.data_mut(|d| {
        if done.is_some() {
            d.remove::<RenameState>(id);
        } else {
            d.insert_temp(id, (key, text, start));
        }
    });
    done
}

/// Start renaming row `key`, its field showing `name`.
fn start_rename(ui: &Ui, key: RenameKey, name: &str) {
    ui.data_mut(|d| d.insert_temp(egui::Id::unique(RENAME), (key, name.to_string(), name.to_string())));
}

/// The panel on the craft UI framework: one tree `List` of layer rows and the object rows under them, the footer bar and
/// the layer menu, all framework components. It runs the engine commands the native rows above run.
#[cfg(not(target_arch = "wasm32"))]
mod framework {
    use std::borrow::Cow;
    use std::cell::RefCell;
    use std::collections::HashMap;
    use std::sync::Arc;

    use craft_behavior::{
        ColumnKey, ColumnWidth, DisplayOrder, EditEvent, IconId, ItemSource, Metrics, PressPhase, Selection, SelectionChange, Sense,
    };
    use craft_ui_panels::sections::{FooterAction, FooterBar};
    use craft_ui_widgets::contract::{
        Bind, CollectionEvent, ColumnSpec, Common, Component, Cx, DragPreview, DropAt as PlaceAt, DropKind, DropPlace, ListDrag, MeasureCx, PaintUi,
        Pointer as _, Props as _, ReorderRule, Role, Show as _, ShowCollection as _, ShowContainer as _, Sizing, ToggleColumn,
    };
    use craft_ui_widgets::display::{Divider, Shade, Text};
    use craft_ui_widgets::list::{List, ListRow, RowEvent, RowInk, ToggleCell};
    use craft_ui_widgets::menu::{Menu, MenuItem};
    use craft_ui_widgets::primitives::{ContentPainterCx, IconSpec, PaintShape, StrokeWidth};
    use craft_ui_widgets::runtime::env::Label;
    use craft_ui_widgets::style::{ComponentStyle, DataColor, FocusLook, StyleCx, TypeRole};
    use egui::{Color32, CornerRadius, Rect, Response, Ui, Vec2, vec2};
    use serde_json::{Value, json};

    use super::{Document, DropAt, Item, ItemId, LayerId, LayersDrag, MAX_DEPTH, c32, drop_command, item_label};
    use crate::DesignApp;

    const EYE: ColumnKey = ColumnKey::new("eye");
    const LOCK: ColumnKey = ColumnKey::new("lock");
    const NAME: ColumnKey = ColumnKey::new("name");

    type Command = (&'static str, Value);
    type Event = CollectionEvent<LayerKey, RowEvent>;

    /// A row of the tree: a layer, or an object on one (a group's members sit under the group).
    #[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
    pub(super) enum LayerKey {
        Layer(LayerId),
        Object(ItemId),
    }

    /// What a drag of a layer row carries for the footer's Create New Layer, which duplicates the layer.
    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    struct DragLayer(LayerId);

    /// What a drag of a filled selection square carries: the selection, for the layer rows to take.
    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    struct DragSelection;

    /// What the panel keeps between passes.
    #[derive(Clone, Default)]
    pub(super) struct Memory {
        /// The layers and groups the user opened or closed. A layer nobody touched is open while it is the active one.
        open: HashMap<LayerKey, bool>,
        /// The rows the last pass revealed, so a selection scrolls into view once.
        revealed: Vec<u64>,
        /// The row keyboard navigation moves from.
        lead: Option<LayerKey>,
        /// The layer whose menu the last request was for.
        menu: Option<LayerId>,
    }

    impl Memory {
        fn is_open(&self, key: &LayerKey, active: LayerId) -> bool {
            self.open.get(key).copied().unwrap_or(*key == LayerKey::Layer(active))
        }
    }

    /// One displayed row.
    struct Line {
        key: LayerKey,
        layer: LayerId,
        /// The group the object sits in; `None` for a layer and for an object at the top level of its layer.
        group: Option<ItemId>,
        depth: usize,
        has_children: bool,
        open: bool,
        label: String,
        /// The eye cell: the layer is shown, the object is not hidden.
        on: bool,
        locked: bool,
        /// The row is on a hidden layer, or is hidden itself.
        dim: bool,
        color: Color32,
    }

    /// The rows of the tree in display order: layers top to bottom, and under each open layer its objects topmost first.
    struct Lines {
        rows: Vec<Line>,
        index: HashMap<LayerKey, usize>,
    }

    /// What the walk down one layer's objects needs.
    struct Walk<'a> {
        doc: &'a Document,
        memory: &'a Memory,
        active: LayerId,
        layer: LayerId,
        dim: bool,
        color: Color32,
    }

    impl Lines {
        fn build(doc: &Document, spread: &[Arc<Item>], memory: &Memory, active: LayerId) -> Lines {
            let mut rows = Vec::new();
            for l in &doc.layers {
                let key = LayerKey::Layer(l.id);
                let open = memory.is_open(&key, active);
                let color = c32(l.color);
                rows.push(Line {
                    key,
                    layer: l.id,
                    group: None,
                    depth: 0,
                    has_children: true,
                    open,
                    label: l.name.clone(),
                    on: l.visible,
                    locked: l.locked,
                    dim: false,
                    color,
                });
                if open {
                    let items: Vec<Arc<Item>> = spread.iter().filter(|i| i.layer == l.id).cloned().collect();
                    let walk = Walk { doc, memory, active, layer: l.id, dim: !l.visible, color };
                    walk.push_objects(&mut rows, &items, None, 1);
                }
            }
            let index = rows.iter().enumerate().map(|(at, row)| (row.key, at)).collect();
            Lines { rows, index }
        }

        fn line(&self, key: &LayerKey) -> Option<&Line> {
            self.index.get(key).and_then(|at| self.rows.get(*at))
        }
    }

    impl Walk<'_> {
        /// The rows of `items` (back to front) topmost first, each group's members under it while it is open.
        fn push_objects(&self, rows: &mut Vec<Line>, items: &[Arc<Item>], group: Option<ItemId>, depth: usize) {
            for it in items.iter().rev() {
                let key = LayerKey::Object(it.id);
                let expandable = !it.children().is_empty() && it.states.is_empty() && depth <= MAX_DEPTH;
                let open = expandable && self.memory.is_open(&key, self.active);
                rows.push(Line {
                    key,
                    layer: self.layer,
                    group,
                    depth,
                    has_children: expandable,
                    open,
                    label: item_label(self.doc, it),
                    on: !it.hidden,
                    locked: it.locked,
                    dim: self.dim || it.hidden,
                    color: self.color,
                });
                if open {
                    self.push_objects(rows, it.children(), Some(it.id), depth + 1);
                }
            }
        }
    }

    impl DisplayOrder<LayerKey> for Lines {
        fn len(&self) -> usize {
            self.rows.len()
        }

        fn key(&self, at: usize) -> LayerKey {
            self.rows[at].key
        }

        fn index_of(&self, key: &LayerKey) -> Option<usize> {
            self.index.get(key).copied()
        }

        fn depth(&self, at: usize) -> usize {
            self.rows[at].depth
        }

        fn has_children(&self, at: usize) -> bool {
            self.rows[at].has_children
        }

        fn open(&self, at: usize) -> bool {
            self.rows[at].open
        }

        fn text(&self, at: usize) -> Option<Cow<'_, str>> {
            Some(Cow::Borrowed(self.rows[at].label.as_str()))
        }
    }

    impl ItemSource<LayerKey> for Lines {
        fn toggled(&self, at: usize, column: &ColumnKey) -> bool {
            let row = &self.rows[at];
            if *column == EYE { row.on } else { row.locked }
        }
    }

    /// The panel's words in the app's language, and what its rows hand back.
    struct Words {
        hide: String,
        show: String,
        lock: String,
        unlock: String,
        select: String,
        select_drag: String,
        badge: String,
        active: String,
    }

    impl Words {
        fn of(language: &str) -> Words {
            let tr = |text: &str| crate::i18n::tr(language, text).to_owned();
            Words {
                hide: tr("Hide"),
                show: tr("Show"),
                lock: tr("Lock"),
                unlock: tr("Unlock"),
                select: tr("Select"),
                select_drag: tr("Select; drag to move the selection to another layer"),
                badge: tr("Move Selection Here"),
                active: tr("Active Layer"),
            }
        }
    }

    /// What the app holds about the document the panel shows.
    struct Panel {
        doc: Arc<Document>,
        identity: (u64, u64),
        active: LayerId,
        selected: Vec<ItemId>,
        language: String,
        page: usize,
        spread: Vec<Arc<Item>>,
    }

    impl Panel {
        fn of(app: &DesignApp) -> Option<Panel> {
            let st = app.session.active()?;
            let doc = st.doc.clone();
            let page = crate::canvas::current_page(app).unwrap_or(0);
            let spread = doc.page_loc(page).and_then(|(si, _)| doc.spreads.get(si)).map(|sp| sp.items.clone()).unwrap_or_default();
            Some(Panel {
                identity: (st.uid, st.identity_epoch),
                active: st.active_layer,
                selected: st.selection.items.clone(),
                language: app.ui.language.clone(),
                page,
                spread,
                doc,
            })
        }

        /// The top-level objects of the selection: an object selected inside a closed group shows as its group.
        fn shown(&self) -> Vec<ItemId> {
            self.selected.iter().map(|i| self.doc.top_level_of(*i).unwrap_or(*i)).collect()
        }

        fn layer_closed(&self, layer: LayerId) -> bool {
            self.doc.layer(layer).is_some_and(|l| l.locked || !l.visible)
        }
    }

    /// The first row a new selection reveals, and the layers it opens: once per selection, so the panel can still be
    /// scrolled and closed freely afterwards.
    fn reveal_selection(panel: &Panel, memory: &mut Memory) -> bool {
        let now: Vec<u64> = panel.shown().iter().map(|i| i.0).collect();
        let reveal = !now.is_empty() && memory.revealed != now;
        memory.revealed = now;
        if reveal {
            for layer in panel.selected.iter().filter_map(|i| panel.doc.item(*i).map(|it| it.layer)) {
                let key = LayerKey::Layer(layer);
                if !memory.is_open(&key, panel.active) {
                    memory.open.insert(key, true);
                }
            }
        }
        reveal
    }

    /// The list's selection: the selected objects (and their top-level groups) that have rows, else the active layer.
    fn list_selection(panel: &Panel, lines: &Lines, lead: Option<LayerKey>) -> Selection<LayerKey> {
        let mut items: Vec<LayerKey> = Vec::new();
        for id in panel.selected.iter().chain(&panel.shown()) {
            let key = LayerKey::Object(*id);
            if lines.index.contains_key(&key) && !items.contains(&key) {
                items.push(key);
            }
        }
        if items.is_empty() {
            items.push(LayerKey::Layer(panel.active));
        }
        let lead = lead.filter(|key| items.contains(key)).or_else(|| items.first().copied());
        Selection::new(items, lead, lead)
    }

    // ---------------------------------------------------------------------------------------------------------------
    // The selection square

    /// The look of the selection square.
    #[derive(Clone, Debug, PartialEq)]
    struct SelectionSquareStyle {
        focus: FocusLook,
        /// The cell the square sits in, which a press on it takes.
        cell: f32,
        /// The square itself.
        mark: f32,
    }

    impl ComponentStyle for SelectionSquareStyle {
        fn focus(&self) -> Option<&FocusLook> {
            Some(&self.focus)
        }

        fn metrics(&self) -> Metrics {
            Metrics::new()
        }
    }

    /// The square at the end of a row, in the layer's colour: filled while the layer's objects (or the object) are
    /// selected, outlined under the pointer otherwise. A click selects; a filled one on a layer row also drags the
    /// selection to another layer.
    #[derive(craft_ui_widgets::contract::Component, craft_ui_widgets::contract::Props)]
    struct SelectionSquare {
        #[common]
        common: Common,
        color: Color32,
        on: bool,
        /// The badge a drag from the square shows; the square drags only when it has one.
        drag: Option<Label>,
    }

    impl SelectionSquare {
        fn new(name: String, color: Color32, on: bool, drag: Option<Label>) -> Self {
            SelectionSquare { common: Common::new(), color, on, drag }.label(name)
        }
    }

    impl Component for SelectionSquare {
        const ROLE: Role = Role::BUTTON;
        const PARTS: bool = false;
        const SHOWS_LABEL: bool = false;
        const OVERLAYS: bool = false;
        type Style = SelectionSquareStyle;
        type Output = Response;

        fn style(cx: &StyleCx<'_>, _: SelectionSquareStyleKey) -> SelectionSquareStyle {
            SelectionSquareStyle { focus: cx.focus_look(), cell: cx.tokens.size.icon.sm, mark: cx.tokens.size.icon.glyph_sm }
        }

        fn measure(&mut self, cx: &MeasureCx<'_, Self>) -> Vec2 {
            Vec2::splat(cx.style.cell)
        }

        fn sense(&self) -> Sense {
            if self.drag.is_some() { Sense::CLICK | Sense::DRAG | Sense::FOCUS } else { Sense::CLICK | Sense::FOCUS }
        }

        fn update(&mut self, cx: &mut Cx<'_, Self>) {
            cx.join_roving();
            if let Some(badge) = &self.drag
                && matches!(cx.press().phase, PressPhase::DragStart { .. })
            {
                cx.start_drag(DragSelection, DragPreview::Badge(badge.clone()));
            }
        }

        fn paint(&mut self, ui: &mut PaintUi<'_>, cx: &Cx<'_, Self>) {
            let mark = Rect::from_center_size(cx.rect.center(), Vec2::splat(cx.style.mark));
            let shape = PaintShape::Rect(mark, CornerRadius::ZERO);
            let draw = ui.draw();
            if self.on {
                draw.data_fill(&shape, DataColor::new(self.color));
            } else if cx.hit() {
                draw.stroke(&shape, StrokeWidth::Thin, DataColor::new(self.color));
            }
        }

        fn finish(self, cx: Cx<'_, Self>) -> Response {
            cx.response.clone()
        }
    }

    // ---------------------------------------------------------------------------------------------------------------
    // The rows

    /// What a row shows besides its line.
    #[derive(Clone, Copy)]
    struct Facts {
        /// A layer row of the active layer.
        active: bool,
        /// The square is filled: the layer holds selected objects, the object is selected.
        square_on: bool,
        /// The active layer while objects hold the selection.
        highlight: bool,
    }

    /// The row of `line`; the list adds its cells and state.
    fn row_of<'r>(line: &Line, facts: Facts, words: &'r Words, squares: &'r RefCell<Vec<LayerKey>>) -> ListRow<'r> {
        let is_layer = matches!(line.key, LayerKey::Layer(_));
        let eye_tip = if line.on { &words.hide } else { &words.show };
        let lock_tip = if line.locked { &words.unlock } else { &words.lock };
        let mut row = ListRow::new(line.label.clone())
            .layer(true)
            .strong(is_layer)
            .depth(line.depth)
            .highlight(facts.highlight)
            .toggle(ToggleCell::eye(EYE, line.on).empty_off().tooltip(eye_tip.clone()))
            .toggle(ToggleCell::lock(LOCK, line.locked).empty_off().tooltip(lock_tip.clone()));
        if line.has_children {
            row = row.group(line.open);
        }
        if line.dim {
            row = row.ink(RowInk::Dimmed);
        }
        if is_layer {
            let color = DataColor::new(line.color);
            row = row.lead_painter(move |cx: &ContentPainterCx<'_>, slot: Rect| {
                let chip = Rect::from_center_size(slot.center(), vec2(cx.stroke_width(StrokeWidth::Heavy), slot.width()));
                cx.with_draw(|draw| draw.data_fill(&PaintShape::Rect(chip, CornerRadius::ZERO), color));
            });
        }
        let drags = is_layer && facts.square_on;
        let hint = if drags { &words.select_drag } else { &words.select };
        let square =
            SelectionSquare::new(hint.clone(), line.color, facts.square_on, drags.then(|| Label::from(words.badge.clone()))).tooltip(hint.clone());
        let key = line.key;
        row = row.trailing_field(square, move |response: Response| {
            if craft_ui_widgets::contract::clicked(&response) {
                squares.borrow_mut().push(key);
            }
        });
        if facts.active {
            row = row.trailing_icon(IconSpec::svg(IconId::new("pen_tool")), words.active.clone());
        }
        row
    }

    // ---------------------------------------------------------------------------------------------------------------
    // Drops

    /// A reorder as the list reports it: the rows it moves and the position among the rows that remain.
    struct Reorder<'a> {
        grabbed: LayerKey,
        keys: &'a [LayerKey],
        to: usize,
    }

    /// The objects a drag of the row `grabbed` carries: the selected objects that share its parent when it is selected
    /// (back to front), else the row alone.
    fn dragged_objects(lines: &Lines, selected: &[ItemId], grabbed: &Line, id: ItemId) -> Vec<ItemId> {
        if !selected.contains(&id) {
            return vec![id];
        }
        lines
            .rows
            .iter()
            .rev()
            .filter(|r| r.layer == grabbed.layer && r.group == grabbed.group)
            .filter_map(|r| match r.key {
                LayerKey::Object(other) if selected.contains(&other) => Some(other),
                _ => None,
            })
            .collect()
    }

    /// Where a layer dropped between the rows that remain goes: a gap between two layers' blocks.
    fn layer_landing(doc: &Document, src: LayerId, remaining: &[&Line], to: usize) -> Option<(LayersDrag, DropAt)> {
        if remaining.get(to).is_some_and(|next| !matches!(next.key, LayerKey::Layer(_))) {
            return None;
        }
        let slot = remaining[..to].iter().filter(|r| matches!(r.key, LayerKey::Layer(_))).count();
        let from = doc.layers.iter().position(|l| l.id == src)?;
        if slot == from {
            return None;
        }
        // `drop_command` counts a slot among all the layers, the dragged one included.
        let slot = if slot > from { slot + 1 } else { slot };
        Some((LayersDrag::Layer(src), DropAt::LayerSlot(slot)))
    }

    /// Where objects dropped between the rows that remain go: above the object under the gap when it sits in their own
    /// parent, else below the last row of that parent above the gap. Groups take no outsiders and their members stay in
    /// their group, as in the native rows.
    fn object_landing(remaining: &[&Line], to: usize, group: Option<ItemId>) -> Option<(LayerId, DropAt)> {
        let next = remaining.get(to).copied();
        let at_object = |line: &Line| matches!(line.key, LayerKey::Object(_));
        if let Some(n) = next.filter(|n| at_object(n) && n.group == group)
            && let LayerKey::Object(id) = n.key
        {
            return Some((n.layer, DropAt::Object { id, above: true }));
        }
        if group.is_none() && next.is_some_and(at_object) {
            return None;
        }
        let above = &remaining[..to];
        let before = above.iter().rposition(|r| r.group == group)?;
        let b = above[before];
        // What lies between it and the gap must hang off it: the open members of a group.
        if above[before + 1..].iter().any(|r| r.depth <= b.depth) || next.is_some_and(|n| n.depth > b.depth) {
            return None;
        }
        match b.key {
            LayerKey::Object(id) => Some((b.layer, DropAt::Object { id, above: false })),
            LayerKey::Layer(layer) => Some((layer, DropAt::OntoLayer(layer))),
        }
    }

    /// The drop a reorder makes, `None` where it cannot land: objects land on a locked or hidden layer only with `force`
    /// (Cmd/Ctrl held).
    fn landing(panel: &Panel, lines: &Lines, mv: &Reorder<'_>, force: bool) -> Option<(LayersDrag, DropAt)> {
        let remaining: Vec<&Line> = lines.rows.iter().filter(|r| !mv.keys.contains(&r.key)).collect();
        let to = mv.to.min(remaining.len());
        let id = match mv.grabbed {
            LayerKey::Layer(src) => return layer_landing(&panel.doc, src, &remaining, to),
            LayerKey::Object(id) => id,
        };
        let grabbed = lines.line(&mv.grabbed)?;
        let (layer, at) = object_landing(&remaining, to, grabbed.group)?;
        if panel.layer_closed(layer) && !force {
            return None;
        }
        let ids = dragged_objects(lines, &panel.selected, grabbed, id);
        Some((LayersDrag::Objects { ids, group: grabbed.group }, at))
    }

    /// The layer a drop of the selection onto the row `at` moves it to; locked and hidden layers take it only with `force`.
    fn selection_target(panel: &Panel, lines: &Lines, at: &PlaceAt<LayerKey>, force: bool) -> Option<LayerId> {
        let layer = lines.line(at.key()?)?.layer;
        (force || !panel.layer_closed(layer)).then_some(layer)
    }

    // ---------------------------------------------------------------------------------------------------------------
    // What the list reports

    /// The command a change of the list's selection runs: objects select, a layer row activates its layer.
    fn selection_command(change: SelectionChange<LayerKey>) -> Option<Command> {
        match change {
            SelectionChange::Set(keys) => {
                let ids: Vec<u64> = keys
                    .iter()
                    .filter_map(|key| match key {
                        LayerKey::Object(id) => Some(id.0),
                        LayerKey::Layer(_) => None,
                    })
                    .collect();
                if !ids.is_empty() {
                    return Some(("selection.set", json!({"ids": ids})));
                }
                keys.iter().rev().find_map(|key| match key {
                    LayerKey::Layer(id) => Some(("layer.activate", json!({"id": id.0}))),
                    LayerKey::Object(_) => None,
                })
            }
            SelectionChange::Toggle(LayerKey::Object(id)) => Some(("selection.toggle", json!({"id": id.0}))),
            SelectionChange::Toggle(LayerKey::Layer(id)) => Some(("layer.activate", json!({"id": id.0}))),
            _ => None,
        }
    }

    /// The command a flipped eye or lock cell runs; none while the cell already holds the state it landed on.
    fn toggle_command(lines: &Lines, key: LayerKey, eye: bool, on: bool) -> Option<Command> {
        let line = lines.line(&key)?;
        if (if eye { line.on } else { line.locked }) == on {
            return None;
        }
        Some(match (key, eye) {
            (LayerKey::Layer(id), true) => ("layer.set", json!({"id": id.0, "visible": on})),
            (LayerKey::Layer(id), false) => ("layer.set", json!({"id": id.0, "locked": on})),
            (LayerKey::Object(id), true) => ("object.setFlags", json!({"ids": [id.0], "hidden": !on})),
            (LayerKey::Object(id), false) => ("object.setFlags", json!({"ids": [id.0], "locked": on})),
        })
    }

    /// The text a row's rename field opens with: the object's own name, or the label it shows without its brackets.
    fn rename_draft(panel: &Panel, key: &LayerKey) -> Option<String> {
        let LayerKey::Object(id) = key else { return None };
        let it = panel.doc.item(*id)?;
        Some(if it.name.is_empty() { item_label(&panel.doc, it).trim_start_matches('<').trim_end_matches('>').to_owned() } else { it.name.clone() })
    }

    /// The command a committed rename runs. An unedited draft keeps an unnamed object unnamed; an empty name goes back
    /// to the automatic label.
    fn rename_command(panel: &Panel, key: &LayerKey, name: &str) -> Option<Command> {
        let LayerKey::Object(id) = key else { return None };
        let name = name.trim();
        let it = panel.doc.item(*id)?;
        let unedited = rename_draft(panel, key).is_some_and(|draft| draft.trim() == name);
        (!unedited && name != it.name).then(|| ("object.rename", json!({"id": id.0, "name": name})))
    }

    /// The command a drop on the row `at` runs: the selection moves to the row's layer.
    fn dropped_command(panel: &Panel, lines: &Lines, at: &PlaceAt<LayerKey>, force: bool) -> Option<Command> {
        let layer = selection_target(panel, lines, at, force)?;
        drop_command(&LayersDrag::Selection, DropAt::OntoLayer(layer), &panel.doc, &panel.selected)
    }

    /// The command a click on the selection square of `key` runs, with `add` for Shift or Cmd/Ctrl held.
    fn square_command(panel: &Panel, key: LayerKey, add: bool) -> Command {
        match key {
            LayerKey::Layer(layer) => {
                let ids: Vec<u64> = panel.spread.iter().filter(|i| i.layer == layer).map(|i| i.id.0).collect();
                ("selection.set", json!({"ids": ids}))
            }
            LayerKey::Object(id) => ("selection.set", json!({"ids": [id.0], "add": add})),
        }
    }

    /// What one pass of the list asks for, in the order the list reported it.
    struct Reported {
        commands: Vec<Command>,
        /// A right-click asked for a menu: the layer it is for, `None` where the panel has no menu.
        menu: Option<Option<LayerId>>,
    }

    /// The value a gesture ended with: a one-step change or a commit.
    fn settled<T>(edit: EditEvent<T>) -> Option<T> {
        match edit {
            EditEvent::Set { after, .. } | EditEvent::Commit { after, .. } => Some(after),
            _ => None,
        }
    }

    /// Remembers the row a pick made the lead and returns the command it runs.
    fn pick(memory: &mut Memory, change: SelectionChange<LayerKey>) -> Option<Command> {
        match &change {
            SelectionChange::Set(keys) => memory.lead = keys.last().copied(),
            SelectionChange::Toggle(key) => memory.lead = Some(*key),
            _ => {}
        }
        selection_command(change)
    }

    /// The commands the list's events run, and the memory they change. `force` is Cmd/Ctrl held.
    fn apply(panel: &Panel, lines: &Lines, memory: &mut Memory, events: Vec<Event>, force: bool) -> Reported {
        let mut reported = Reported { commands: Vec::new(), menu: None };
        let mut toggles: Vec<(LayerKey, bool, bool)> = Vec::new();
        for event in events {
            match event {
                CollectionEvent::Selection(change) => reported.commands.extend(pick(memory, change)),
                CollectionEvent::LeadMoved { key, .. } => memory.lead = Some(key),
                CollectionEvent::Activate(LayerKey::Layer(id)) => reported.commands.push(("app.layerOptionsDialog", json!({"id": id.0}))),
                CollectionEvent::Open(change) => open_rows(lines, memory, &change),
                CollectionEvent::Toggle(edit) => toggles.extend(edit.landed().map(|cell| (cell.key, cell.column == EYE, cell.on))),
                CollectionEvent::Rename(edit) => {
                    reported.commands.extend(settled(edit).and_then(|renamed| rename_command(panel, &renamed.key, &renamed.name)));
                }
                CollectionEvent::ContextMenu(request) => reported.menu = Some(menu_layer(request.key)),
                CollectionEvent::EmptyContextMenu { .. } => reported.menu = Some(None),
                CollectionEvent::Reorder(edit) => reported.commands.extend(settled(edit).and_then(|mv| {
                    let mv = Reorder { grabbed: mv.key, keys: &mv.keys, to: mv.to };
                    let (drag, at) = landing(panel, lines, &mv, force)?;
                    drop_command(&drag, at, &panel.doc, &panel.selected)
                })),
                CollectionEvent::Dropped { at, payload, .. } if payload.get::<DragSelection>().is_some() => {
                    reported.commands.extend(dropped_command(panel, lines, &at, force));
                }
                _ => {}
            }
        }
        reported.commands.extend(toggle_commands(lines, toggles));
        reported
    }

    /// The layer a menu request on `key` is for.
    fn menu_layer(key: LayerKey) -> Option<LayerId> {
        match key {
            LayerKey::Layer(id) => Some(id),
            LayerKey::Object(_) => None,
        }
    }

    /// The commands the flipped cells run. A sweep reports a cell more than once, and the last report stands.
    fn toggle_commands(lines: &Lines, toggles: Vec<(LayerKey, bool, bool)>) -> Vec<Command> {
        let mut last: Vec<(LayerKey, bool, bool)> = Vec::new();
        for (key, eye, on) in toggles {
            match last.iter_mut().find(|held| held.0 == key && held.1 == eye) {
                Some(held) => held.2 = on,
                None => last.push((key, eye, on)),
            }
        }
        last.into_iter().filter_map(|(key, eye, on)| toggle_command(lines, key, eye, on)).collect()
    }

    /// Opens or closes the group of `change`, every group with Alt held.
    fn open_rows(lines: &Lines, memory: &mut Memory, change: &craft_ui_widgets::contract::OpenChange<LayerKey>) {
        if change.all {
            for line in lines.rows.iter().filter(|line| line.has_children) {
                memory.open.insert(line.key, change.open);
            }
        } else {
            memory.open.insert(change.key, change.open);
        }
    }

    // ---------------------------------------------------------------------------------------------------------------
    // The list

    /// What showing the list returns.
    struct Listed {
        events: Vec<Event>,
        response: Response,
        /// The rect of the row a reveal asked for, to bring into view in the page's own scroll.
        revealed: Option<Rect>,
        squares: Vec<LayerKey>,
    }

    fn columns() -> Vec<ColumnSpec> {
        vec![
            ColumnSpec::toggle(EYE, Label::Empty, ToggleColumn::default()).icon(IconId::new("eye")),
            ColumnSpec::toggle(LOCK, Label::Empty, ToggleColumn::default()).icon(IconId::new("lock")),
            ColumnSpec::new(NAME, Label::Empty, ColumnWidth::fill(1.0)),
        ]
    }

    /// The first row of the display order that holds the selection.
    fn reveal_key(panel: &Panel, lines: &Lines) -> Option<LayerKey> {
        let shown = panel.shown();
        lines.rows.iter().map(|r| r.key).find(|key| matches!(key, LayerKey::Object(id) if panel.selected.contains(id) || shown.contains(id)))
    }

    fn list(ui: &mut Ui, panel: &Panel, lines: &Lines, selection: &mut Selection<LayerKey>, reveal: Option<LayerKey>) -> Listed {
        let words = Words::of(&panel.language);
        let squares: RefCell<Vec<LayerKey>> = RefCell::new(Vec::new());
        let ctx = ui.ctx().clone();
        let shown = panel.shown();
        let objects_selected = !panel.selected.is_empty();
        let sel_layers: Vec<LayerId> = panel.selected.iter().filter_map(|i| panel.doc.item(*i).map(|it| it.layer)).collect();
        let mut widget = List::<LayerKey>::new(crate::i18n::tr(&panel.language, "Layers").to_owned())
            .id_salt(("layers", panel.identity))
            .tree(true)
            .natural_height(true)
            .sizing(Sizing::FillWidth)
            .header(false)
            .columns(columns())
            .rename(true)
            .slow_click_rename(true)
            .reorder(ReorderRule::new().into_groups())
            .selection(Bind::bound(selection))
            .can_rename(|key: &LayerKey| matches!(key, LayerKey::Object(_)))
            .rename_draft(|key: &LayerKey| rename_draft(panel, key))
            .accepts(|drag: &ListDrag<LayerKey>, _over: Option<&LayerKey>, insert: usize| {
                let before = lines.rows[..insert.min(lines.rows.len())].iter().filter(|r| drag.keys.contains(&r.key)).count();
                let mv = Reorder { grabbed: drag.grabbed, keys: &drag.keys, to: insert.saturating_sub(before) };
                landing(panel, lines, &mv, craft_ui_widgets::runtime::pointer::mods(&ctx).cmd).is_some()
            })
            .drag_payload(|drag: &ListDrag<LayerKey>| match drag.grabbed {
                LayerKey::Layer(id) => Some(DragLayer(id)),
                LayerKey::Object(_) => None,
            })
            .takes_drop(DropKind::new(DropPlace::Onto, |_: &DragSelection, at: &PlaceAt<LayerKey>| {
                selection_target(panel, lines, at, craft_ui_widgets::runtime::pointer::mods(&ctx).cmd).is_some()
            }));
        if let Some(key) = reveal {
            widget = widget.reveal(key);
        }
        let out = widget.show_items(ui, lines, &mut |key, icx| {
            let line = &lines.rows[icx.at];
            let is_active = *key == LayerKey::Layer(panel.active);
            let square_on = match line.key {
                LayerKey::Layer(layer) => sel_layers.contains(&layer),
                LayerKey::Object(id) => panel.selected.contains(&id) || shown.contains(&id),
            };
            let facts = Facts { active: is_active, square_on, highlight: is_active && objects_selected };
            row_of(line, facts, &words, &squares).cells(icx.cells()).state(icx.row.clone())
        });
        let revealed = reveal.and_then(|key| out.shown.iter().find(|row| row.key == key).map(|row| row.rect));
        Listed { events: out.events, response: out.response, revealed, squares: squares.into_inner() }
    }

    // ---------------------------------------------------------------------------------------------------------------
    // The layer menu

    /// A row of the layer menu.
    enum MenuEntry {
        Rule,
        Item(String, Command),
    }

    /// The menu of `layer`, as the native menu lists it.
    fn menu_entries(panel: &Panel, layer: &designcraft_doc::Layer) -> Vec<MenuEntry> {
        let tr = |text: &str| crate::i18n::tr(&panel.language, text).to_owned();
        let id = layer.id.0;
        let named = |text: &str| tr(text).replace("{name}", &layer.name);
        let mut entries = vec![
            MenuEntry::Item(named("Layer Options for \"{name}\"…"), ("app.layerOptionsDialog", json!({"id": id}))),
            MenuEntry::Rule,
            MenuEntry::Item(named("Duplicate Layer \"{name}\""), ("layer.duplicate", json!({"id": id}))),
            MenuEntry::Item(tr("Delete Layer"), ("layer.delete", json!({"id": id}))),
            MenuEntry::Item(tr("Move Selection Here"), ("object.setLayer", json!({"layer": id}))),
            MenuEntry::Rule,
        ];
        if panel.active != layer.id {
            let merge = json!({"ids": [panel.active.0, id], "into": panel.active.0});
            entries.push(MenuEntry::Item(tr("Merge into Active Layer"), ("layer.merge", merge)));
        }
        for (label, key) in [("Hide Others", "hide"), ("Lock Others", "lock"), ("Show All Layers", "show"), ("Unlock All Layers", "unlock")] {
            entries.push(MenuEntry::Item(tr(label), ("layer.others", json!({"id": id, key: true}))));
        }
        entries.push(MenuEntry::Item(tr("Delete Unused Layers"), ("layer.deleteUnused", json!({}))));
        entries
    }

    /// Shows the menu of `layer` from the list's response and returns the command a row of it ran.
    fn layer_menu(ui: &mut Ui, panel: &Panel, layer: LayerId, opener: &Response) -> Option<Command> {
        let layer = panel.doc.layer(layer)?;
        let entries = menu_entries(panel, layer);
        let shown = Menu::context(crate::i18n::tr(&panel.language, "Layer").to_owned(), opener).show(ui, |ui, _| {
            let mut ran = None;
            for (at, entry) in entries.iter().enumerate() {
                match entry {
                    MenuEntry::Rule => {
                        Divider::horizontal().show(ui);
                    }
                    MenuEntry::Item(label, _) => {
                        if MenuItem::new(label.clone()).show(ui).ran {
                            ran = Some(at);
                        }
                    }
                }
            }
            ran
        });
        match shown.inner.flatten().and_then(|at| entries.into_iter().nth(at)) {
            Some(MenuEntry::Item(_, command)) => Some(command),
            _ => None,
        }
    }

    // ---------------------------------------------------------------------------------------------------------------
    // The footer

    /// The footer bar: the page and layer count at its start, Delete Layer and Create New Layer at its end. A layer row
    /// dropped on Create New Layer is duplicated.
    fn footer(ui: &mut Ui, panel: &Panel, page_label: &str) -> Vec<Command> {
        let tr = |text: &str| crate::i18n::tr(&panel.language, text).to_owned();
        let layers = crate::i18n::count_label(&panel.language, if panel.doc.layers.len() == 1 { "layer" } else { "layers" }, panel.doc.layers.len());
        let status = format!("{}: {page_label}, {layers}", tr("Page"));
        let actions = [
            FooterAction::new(IconSpec::svg(IconId::new("trash")), tr("Delete Layer")),
            FooterAction::new(IconSpec::svg(IconId::new("plus")), tr("Create New Layer")).accepts_drops::<DragLayer>(|_| true),
        ];
        let (bar, _) = FooterBar::actions(actions).id_salt("layers_footer").show_footer_leading(ui, |ui, _| {
            Text::new(status.clone()).role(TypeRole::Caption).shade(Shade::Faint).show(ui);
        });
        let mut commands = Vec::new();
        match bar.picked {
            Some(0) => commands.push(("layer.delete", json!({"id": panel.active.0}))),
            Some(1) => commands.push(("layer.new", json!({}))),
            _ => {}
        }
        if let Some(dropped) = &bar.dropped
            && dropped.index == 1
            && let Some(DragLayer(id)) = dropped.payload::<DragLayer>()
        {
            commands.push(("layer.duplicate", json!({"id": id.0})));
        }
        commands
    }

    // ---------------------------------------------------------------------------------------------------------------

    pub(super) fn show(app: &mut DesignApp, ui: &mut Ui) {
        let Some(panel) = Panel::of(app) else { return };
        let memory_id = egui::Id::unique(("layers_panel", panel.identity));
        let mut memory: Memory = ui.data(|d| d.get_temp(memory_id)).unwrap_or_default();
        let reveal = reveal_selection(&panel, &mut memory);
        let lines = Lines::build(&panel.doc, &panel.spread, &memory, panel.active);
        let mut selection = list_selection(&panel, &lines, memory.lead);
        let reveal = reveal.then(|| reveal_key(&panel, &lines)).flatten();
        let listed = list(ui, &panel, &lines, &mut selection, reveal);
        if let Some(rect) = listed.revealed {
            ui.scroll_to_rect(rect, Some(egui::Align::Center));
        }
        let mods = craft_ui_widgets::runtime::pointer::mods(ui.ctx());
        let mut commands: Vec<Command> = listed.squares.iter().map(|key| square_command(&panel, *key, mods.shift || mods.cmd)).collect();
        let reported = apply(&panel, &lines, &mut memory, listed.events, mods.cmd);
        commands.extend(reported.commands);
        if let Some(menu) = reported.menu {
            memory.menu = menu;
        }
        if let Some(layer) = memory.menu {
            commands.extend(layer_menu(ui, &panel, layer, &listed.response));
        }
        let page_label = app.session.page_label(panel.page);
        commands.extend(footer(ui, &panel, &page_label));
        ui.data_mut(|d| d.insert_temp(memory_id, memory));
        for (id, params) in commands {
            let _ = app.run(id, params);
        }
    }

    #[cfg(test)]
    mod tests {
        use super::*;

        /// A document with objects `a`, `b` and a group `g` of `m1`, `m2` on Layer 1, and `x` on Art (the active layer above).
        struct Scene {
            app: DesignApp,
            a: ItemId,
            b: ItemId,
            g: ItemId,
            x: ItemId,
            art: LayerId,
        }

        fn scene() -> Scene {
            let mut app = DesignApp::new_for_panel_tests(designcraft_engine::Session::new(), crate::Services::default());
            app.run("file.new", json!({})).unwrap();
            let frame = |app: &mut DesignApp| ItemId(app.run("frame.create", json!({"rect": [0, 0, 10, 10]})).unwrap()["id"].as_u64().unwrap());
            let (a, b, m1, m2) = (frame(&mut app), frame(&mut app), frame(&mut app), frame(&mut app));
            let g = ItemId(app.run("object.group", json!({"ids": [m1.0, m2.0]})).unwrap()["id"].as_u64().unwrap());
            let art = LayerId(app.run("layer.new", json!({"name": "Art"})).unwrap()["id"].as_u64().unwrap());
            let x = frame(&mut app);
            app.run("selection.set", json!({"ids": []})).unwrap();
            Scene { app, a, b, g, x, art }
        }

        /// The panel's rows with every layer and the group open.
        fn rows(scene: &Scene) -> (Panel, Lines) {
            let panel = Panel::of(&scene.app).unwrap();
            let mut memory = Memory::default();
            for l in &panel.doc.layers {
                memory.open.insert(LayerKey::Layer(l.id), true);
            }
            memory.open.insert(LayerKey::Object(scene.g), true);
            let lines = Lines::build(&panel.doc, &panel.spread, &memory, panel.active);
            (panel, lines)
        }

        fn at(lines: &Lines, key: LayerKey) -> usize {
            lines.index[&key]
        }

        /// The drop of `keys` (grabbed at `grabbed`) released in the gap before row `insert`.
        fn gap(panel: &Panel, lines: &Lines, grabbed: LayerKey, keys: &[LayerKey], insert: usize, force: bool) -> Option<(LayersDrag, DropAt)> {
            let before = lines.rows[..insert].iter().filter(|r| keys.contains(&r.key)).count();
            landing(panel, lines, &Reorder { grabbed, keys, to: insert - before }, force)
        }

        #[test]
        fn objects_land_above_the_object_under_the_gap_and_take_their_selected_siblings() {
            let mut scene = scene();
            let (a, b, x) = (LayerKey::Object(scene.a), LayerKey::Object(scene.b), LayerKey::Object(scene.x));
            let (panel, lines) = rows(&scene);
            let got = gap(&panel, &lines, a, &[a], at(&lines, b), false);
            assert_eq!(got, Some((LayersDrag::Objects { ids: vec![scene.a], group: None }, DropAt::Object { id: scene.b, above: true })));
            // A selected object drags the selected objects of its layer, back to front, to the other layer.
            scene.app.run("selection.set", json!({"ids": [scene.a.0, scene.b.0]})).unwrap();
            let (panel, lines) = rows(&scene);
            let got = gap(&panel, &lines, a, &[a, b], at(&lines, x), false);
            assert_eq!(got, Some((LayersDrag::Objects { ids: vec![scene.a, scene.b], group: None }, DropAt::Object { id: scene.x, above: true })));
        }

        #[test]
        fn an_object_cannot_land_inside_a_group_and_a_member_cannot_leave_it() {
            let scene = scene();
            let a = LayerKey::Object(scene.a);
            let (panel, lines) = rows(&scene);
            let members: Vec<ItemId> = lines
                .rows
                .iter()
                .filter(|r| r.group == Some(scene.g))
                .filter_map(|r| match r.key {
                    LayerKey::Object(id) => Some(id),
                    LayerKey::Layer(_) => None,
                })
                .collect();
            assert_eq!(members.len(), 2, "the open group lists its members");
            let (first, second) = (LayerKey::Object(members[0]), LayerKey::Object(members[1]));
            assert_eq!(gap(&panel, &lines, a, &[a], at(&lines, first), false), None, "outsiders stay out of the group");
            // A member lands among its group's members only.
            let got = gap(&panel, &lines, second, &[second], at(&lines, first), false);
            assert_eq!(
                got,
                Some((LayersDrag::Objects { ids: vec![members[1]], group: Some(scene.g) }, DropAt::Object { id: members[0], above: true }))
            );
            assert_eq!(gap(&panel, &lines, second, &[second], 0, false), None, "a member does not leave its group");
        }

        #[test]
        fn a_locked_layer_takes_objects_only_with_cmd() {
            let mut scene = scene();
            scene.app.run("layer.set", json!({"id": scene.art.0, "locked": true})).unwrap();
            let (a, x) = (LayerKey::Object(scene.a), LayerKey::Object(scene.x));
            let (panel, lines) = rows(&scene);
            assert_eq!(gap(&panel, &lines, a, &[a], at(&lines, x), false), None);
            assert!(gap(&panel, &lines, a, &[a], at(&lines, x), true).is_some());
        }

        #[test]
        fn a_layer_lands_between_two_layers_blocks_and_not_where_it_already_is() {
            let scene = scene();
            let (panel, lines) = rows(&scene);
            let (art, l1) = (LayerKey::Layer(scene.art), LayerKey::Layer(panel.doc.layers[1].id));
            let end = gap(&panel, &lines, art, &[art], lines.rows.len(), false).expect("below the last layer");
            assert_eq!(drop_command(&end.0, end.1, &panel.doc, &panel.selected), Some(("layer.move", json!({"id": scene.art.0, "to": 1}))));
            assert_eq!(gap(&panel, &lines, art, &[art], at(&lines, l1), false), None, "it is already above Layer 1");
            assert_eq!(gap(&panel, &lines, art, &[art], at(&lines, LayerKey::Object(scene.x)), false), None, "not among another layer's objects");
        }

        #[test]
        fn a_selection_dropped_on_a_row_goes_to_that_rows_layer() {
            let mut scene = scene();
            scene.app.run("selection.set", json!({"ids": [scene.a.0]})).unwrap();
            let (panel, lines) = rows(&scene);
            let on_object = PlaceAt::Onto(LayerKey::Object(scene.x));
            assert_eq!(selection_target(&panel, &lines, &on_object, false), Some(scene.art));
            assert_eq!(
                dropped_command(&panel, &lines, &on_object, false),
                Some(("object.setLayer", json!({"layer": scene.art.0, "ids": [scene.a.0]})))
            );
            scene.app.run("layer.set", json!({"id": scene.art.0, "locked": true})).unwrap();
            let (panel, lines) = rows(&scene);
            assert_eq!(selection_target(&panel, &lines, &on_object, false), None);
            assert_eq!(selection_target(&panel, &lines, &on_object, true), Some(scene.art));
        }

        #[test]
        fn a_rename_that_keeps_the_automatic_label_or_the_name_runs_nothing() {
            let mut scene = scene();
            let (panel, _) = rows(&scene);
            let key = LayerKey::Object(scene.a);
            assert_eq!(rename_draft(&panel, &key).as_deref(), Some("rectangle"), "the label without its brackets");
            assert_eq!(rename_command(&panel, &key, "rectangle"), None, "unedited");
            assert_eq!(rename_command(&panel, &key, "  Hero "), Some(("object.rename", json!({"id": scene.a.0, "name": "Hero"}))));
            scene.app.run("object.rename", json!({"id": scene.a.0, "name": "Hero"})).unwrap();
            let (panel, _) = rows(&scene);
            assert_eq!(rename_command(&panel, &key, "Hero"), None, "unchanged");
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn text_frames_are_labelled_with_their_first_words() {
        let mut s = designcraft_engine::Session::new();
        s.execute("file.new", &json!({})).unwrap();
        let r = s.execute("frame.create", &json!({"rect": [36, 36, 300, 200], "content": "text"})).unwrap();
        s.execute("text.insert", &json!({"text": "Every page begins as an empty field of possibility"})).unwrap();
        let rect = s.execute("frame.create", &json!({"rect": [36, 236, 300, 400]})).unwrap();
        let doc = &s.doc().unwrap().doc;
        let tf = doc.item(ItemId(r["id"].as_u64().unwrap())).unwrap();
        assert_eq!(item_label(doc, tf), "<Every page begins as an…>");
        let rr = doc.item(ItemId(rect["id"].as_u64().unwrap())).unwrap();
        assert_eq!(item_label(doc, rr), "<rectangle>");
    }

    /// The panel alone, 304 pt wide; rows start at (8, 8).
    fn panel(app: DesignApp) -> egui_kittest::Harness<'static, DesignApp> {
        let mut h = egui_kittest::Harness::builder().with_size(vec2(320.0, 400.0)).build_ui_state(|ui, app: &mut DesignApp| show(app, ui), app);
        h.run_steps(2);
        h
    }

    /// The point `y` of the way down row `i`, in the name column (or at the selection square).
    fn row(i: usize, y: f32, square: bool) -> egui::Pos2 {
        pos2(if square { 300.0 } else { 150.0 }, 8.0 + ROW_H * (i as f32 + y))
    }

    fn drag(h: &mut egui_kittest::Harness<'static, DesignApp>, from: egui::Pos2, to: egui::Pos2) {
        h.hover_at(from);
        h.run_steps(1);
        h.drag_at(from);
        h.run_steps(1);
        for k in 1..=6 {
            h.hover_at(from.lerp(to, k as f32 / 6.0));
            h.run_steps(1);
        }
        h.drop_at(to);
        h.run_steps(3);
    }

    fn app() -> DesignApp {
        let mut app = DesignApp::new(designcraft_engine::Session::new(), crate::Services::default());
        app.run("file.new", json!({})).unwrap();
        app
    }

    fn layers(app: &DesignApp) -> Vec<String> {
        app.session.active().unwrap().doc.layers.iter().map(|l| l.name.clone()).collect()
    }

    #[test]
    fn dragging_a_layer_row_below_another_reorders_the_layers() {
        let mut app = app();
        app.run("layer.new", json!({"name": "Art"})).unwrap();
        assert_eq!(layers(&app), ["Art", "Layer 1"]);
        let mut h = panel(app);
        // Art (open, no objects) is row 0, Layer 1 row 1: drop on Layer 1's lower half.
        drag(&mut h, row(0, 0.5, false), row(1, 0.8, false));
        assert_eq!(layers(h.state()), ["Layer 1", "Art"]);
        h.state_mut().run("edit.undo", json!({})).unwrap();
        assert_eq!(layers(h.state()), ["Art", "Layer 1"], "one undo step");
    }

    #[test]
    fn a_layer_dropped_on_create_new_layer_is_duplicated() {
        use egui_kittest::kittest::Queryable;
        let mut app = app();
        app.run("layer.new", json!({"name": "Art"})).unwrap();
        app.run("frame.create", json!({"rect": [0, 0, 10, 10]})).unwrap();
        app.run("selection.set", json!({"ids": []})).unwrap();
        let mut h = panel(app);
        let plus = h.get_by_label("Create New Layer").rect().center();
        // Rows: Art, its rectangle, Layer 1. Drop Layer 1 on the button.
        drag(&mut h, row(2, 0.5, false), plus);
        assert_eq!(layers(h.state()), ["Art", "Layer 1 copy", "Layer 1"]);
        drag(&mut h, row(0, 0.5, false), plus);
        assert_eq!(layers(h.state()), ["Art copy", "Art", "Layer 1 copy", "Layer 1"]);
        let d = &h.state().session.active().unwrap().doc;
        assert_eq!(d.spreads[0].items.iter().filter(|i| i.layer == d.layers[0].id).count(), 1, "the copy holds the rectangle");
    }

    #[test]
    fn dragging_objects_and_the_selection_square_moves_them_between_layers() {
        let mut app = app();
        let l1 = app.session.active().unwrap().active_layer;
        let a = app.run("frame.create", json!({"rect": [0, 0, 10, 10]})).unwrap()["id"].as_u64().unwrap();
        let art = LayerId(app.run("layer.new", json!({"name": "Art"})).unwrap()["id"].as_u64().unwrap());
        let x = app.run("frame.create", json!({"rect": [0, 0, 10, 10]})).unwrap()["id"].as_u64().unwrap();
        app.run("selection.set", json!({"ids": []})).unwrap();
        let mut h = panel(app);
        let layer_of =
            |h: &egui_kittest::Harness<'static, DesignApp>, id: u64| h.state().session.active().unwrap().doc.item(ItemId(id)).unwrap().layer;
        // Rows: Art, x, Layer 1 (closed). Open Layer 1: Art, x, Layer 1, a.
        click(&mut h, pos2(8.0 + 2.0 * COL_W + 9.0, row(2, 0.5, false).y));
        // x dropped on the lower half of a goes behind it on Layer 1.
        drag(&mut h, row(1, 0.5, false), row(3, 0.8, false));
        assert_eq!(layer_of(&h, x), l1);
        let order: Vec<u64> = h.state().session.active().unwrap().doc.spreads[0].items.iter().map(|i| i.id.0).collect();
        assert_eq!(order, [x, a], "x is behind a");
        // Rows: Art, Layer 1, a, x. Select x and drag Layer 1's selection square onto Art.
        h.state_mut().run("selection.set", json!({"ids": [x]})).unwrap();
        h.run_steps(2);
        drag(&mut h, row(1, 0.5, true), row(0, 0.5, false));
        assert_eq!(layer_of(&h, x), art);
        assert_eq!(layer_of(&h, a), l1);
    }

    #[test]
    fn locked_layers_take_drops_with_cmd_and_group_members_move_with_their_group() {
        let mut app = app();
        let l1 = app.session.active().unwrap().active_layer;
        let a = app.run("frame.create", json!({"rect": [0, 0, 10, 10]})).unwrap()["id"].as_u64().unwrap();
        let b = app.run("frame.create", json!({"rect": [0, 0, 10, 10]})).unwrap()["id"].as_u64().unwrap();
        let g = app.run("object.group", json!({"ids": [a, b]})).unwrap()["id"].as_u64().unwrap();
        let art = LayerId(app.run("layer.new", json!({"name": "Art"})).unwrap()["id"].as_u64().unwrap());
        app.run("layer.set", json!({"id": art.0, "locked": true})).unwrap();
        // Select a group member directly.
        app.run("selection.set", json!({"ids": [a]})).unwrap();
        let mut h = panel(app);
        let layer_of =
            |h: &egui_kittest::Harness<'static, DesignApp>, id: u64| h.state().session.active().unwrap().doc.item(ItemId(id)).unwrap().layer;
        // Rows: Art (open, empty), Layer 1. Without Cmd the locked layer refuses the drop.
        drag(&mut h, row(1, 0.5, true), row(0, 0.5, false));
        assert_eq!(layer_of(&h, g), l1);
        h.event(egui::Event::ModifiersChanged(egui::Modifiers::COMMAND));
        drag(&mut h, row(1, 0.5, true), row(0, 0.5, false));
        h.event(egui::Event::ModifiersChanged(egui::Modifiers::NONE));
        h.run_steps(1);
        assert!([g, a, b].iter().all(|i| layer_of(&h, *i) == art), "the whole group moved");
    }

    fn click(h: &mut egui_kittest::Harness<'static, DesignApp>, p: egui::Pos2) {
        h.hover_at(p);
        h.drag_at(p);
        h.drop_at(p);
        h.run_steps(3);
    }

    #[test]
    fn selecting_an_object_opens_its_layer_in_the_panel() {
        use egui_kittest::Harness;
        let mut app = DesignApp::new(designcraft_engine::Session::new(), crate::Services::default());
        app.run("file.new", json!({})).unwrap();
        let id = app.run("frame.create", json!({"rect": [72, 72, 200, 200]})).unwrap()["id"].as_u64().unwrap();
        let first = app.session.active().unwrap().doc.item(ItemId(id)).unwrap().layer;
        // A new layer becomes the active one: the first layer's row starts closed.
        app.run("layer.new", json!({})).unwrap();
        app.run("selection.set", json!({"ids": []})).unwrap();
        let mut h = Harness::builder().with_size(egui::vec2(260.0, 400.0)).build_ui_state(|ui, app: &mut DesignApp| show(app, ui), app);
        h.run_steps(2);
        let open = |h: &Harness<'_, DesignApp>| h.ctx.data(|d| d.get_temp::<bool>(egui::Id::unique(("layer_open", first.0)))).unwrap_or(false);
        assert!(!open(&h), "closed while nothing on it is selected");
        h.state_mut().run("selection.set", json!({"ids": [id]})).unwrap();
        h.run_steps(2);
        assert!(open(&h), "selecting the object opens its layer");
        // Closed again by hand, it stays closed while the selection stays the same.
        h.ctx.data_mut(|d| d.insert_temp(egui::Id::unique(("layer_open", first.0)), false));
        h.run_steps(2);
        assert!(!open(&h));
    }

    /// As InDesign's: a slow double-click on an object's row renames it; a quick one doesn't.
    #[test]
    fn a_slow_double_click_renames_an_object_and_a_quick_one_does_not() {
        use egui_kittest::{Harness, kittest::Queryable};
        let mut app = DesignApp::new(designcraft_engine::Session::new(), crate::Services::default());
        app.run("file.new", json!({})).unwrap();
        let id = app.run("frame.create", json!({"rect": [72, 72, 200, 200], "content": "unassigned"})).unwrap()["id"].as_u64().unwrap();
        app.run("selection.set", json!({"ids": []})).unwrap();
        let mut h =
            Harness::builder().with_size(egui::vec2(260.0, 400.0)).with_step_dt(0.05).build_ui_state(|ui, app: &mut DesignApp| show(app, ui), app);
        h.ctx.data_mut(|d| d.insert_temp(egui::Id::unique(("layer_open", h.state().session.active().unwrap().active_layer.0)), true));
        h.run_steps(2);
        let row = h.get_by_label("<rectangle>").rect().center();
        let click = |h: &mut Harness<'_, DesignApp>| {
            h.input_mut().events.push(egui::Event::PointerMoved(row));
            for pressed in [true, false] {
                h.input_mut().events.push(egui::Event::PointerButton {
                    pos: row,
                    button: egui::PointerButton::Primary,
                    pressed,
                    modifiers: egui::Modifiers::NONE,
                });
                h.step();
            }
        };
        let renaming = |h: &Harness<'_, DesignApp>| h.query_by_role(egui::accesskit::Role::TextInput).is_some();
        // A quick double-click selects it and doesn't rename.
        click(&mut h);
        click(&mut h);
        h.run_steps(2);
        assert_eq!(h.state().session.active().unwrap().selection.items, vec![ItemId(id)]);
        assert!(!renaming(&h), "a double-click doesn't rename");
        // A second click a moment later renames it.
        h.run_steps(12);
        click(&mut h);
        h.run_steps(2);
        assert!(renaming(&h), "a slow double-click renames");
    }

    #[test]
    fn rows_rename_in_place() {
        use egui_kittest::{Harness, kittest::Queryable};
        let mut app = DesignApp::new(designcraft_engine::Session::new(), crate::Services::default());
        app.run("file.new", json!({})).unwrap();
        let id = app.run("frame.create", json!({"rect": [72, 72, 200, 200]})).unwrap()["id"].as_u64().unwrap();
        let mut h = Harness::builder().with_size(egui::vec2(260.0, 400.0)).build_ui_state(|ui, app: &mut DesignApp| show(app, ui), app);
        h.run_steps(2);
        let rename = |h: &mut Harness<'_, DesignApp>, key: RenameKey, typed: &str| {
            // As a slow double-click starts it (with an emptied field here, so the typing is the name).
            h.ctx.data_mut(|d| d.insert_temp(egui::Id::unique(RENAME), (key, String::new(), String::new())));
            h.run_steps(2);
            h.get_by_role(egui::accesskit::Role::TextInput).type_text(typed);
            h.run_steps(1);
            h.key_press(egui::Key::Enter);
            h.run_steps(2);
        };
        rename(&mut h, id, "Hero bottle");
        assert_eq!(h.state().session.active().unwrap().doc.item(ItemId(id)).unwrap().name, "Hero bottle");
        // Esc leaves the name as it was.
        h.ctx.data_mut(|d| d.insert_temp(egui::Id::unique(RENAME), (id, String::new(), String::new())));
        h.run_steps(2);
        h.get_by_role(egui::accesskit::Role::TextInput).type_text("Oops");
        h.key_press(egui::Key::Escape);
        h.run_steps(2);
        assert_eq!(h.state().session.active().unwrap().doc.item(ItemId(id)).unwrap().name, "Hero bottle");
    }

    #[test]
    fn confirming_an_unedited_automatic_label_keeps_the_object_unnamed() {
        use egui_kittest::{Harness, kittest::Queryable};
        let mut app = DesignApp::new(designcraft_engine::Session::new(), crate::Services::default());
        app.run("file.new", json!({})).unwrap();
        let id = app.run("frame.create", json!({"rect": [72, 72, 200, 200]})).unwrap()["id"].as_u64().unwrap();
        let mut h = Harness::builder().with_size(egui::vec2(260.0, 400.0)).build_ui_state(|ui, app: &mut DesignApp| show(app, ui), app);
        h.run_steps(2);
        let name = |h: &Harness<'_, DesignApp>| h.state().session.active().unwrap().doc.item(ItemId(id)).unwrap().name.clone();
        // As a slow double-click on the unnamed row starts it: the field shows the automatic label.
        let start = |h: &mut Harness<'_, DesignApp>| {
            h.ctx.data_mut(|d| d.insert_temp(egui::Id::unique(RENAME), (id, "Rectangle".to_string(), "Rectangle".to_string())));
            h.run_steps(2);
        };
        start(&mut h);
        h.key_press(egui::Key::Enter);
        h.run_steps(2);
        assert_eq!(name(&h), "", "Enter without editing keeps following the automatic label");
        start(&mut h);
        h.get_by_role(egui::accesskit::Role::TextInput).type_text("X");
        h.run_steps(1);
        h.key_press(egui::Key::Enter);
        h.run_steps(2);
        assert!(name(&h).contains('X'), "an edited text becomes the name");
    }
}

/// The panel on the installed framework, driven with real pointer and key events: the successor, under the framework
/// rows, of the native tests above.
#[cfg(all(test, not(target_arch = "wasm32")))]
mod framework_tests {
    use super::*;
    use egui::{Event, Key, Modifiers, PointerButton};
    use egui_kittest::{Harness, kittest::Queryable};

    type Window = Harness<'static, DesignApp>;

    fn app() -> DesignApp {
        let mut app = DesignApp::new_for_panel_tests(designcraft_engine::Session::new(), crate::Services::default());
        app.ui.language = "en".to_owned();
        app.run("file.new", json!({})).unwrap();
        app
    }

    /// The panel alone on a context that has the framework's runtime, as the desktop shell runs it.
    fn open(mut app: DesignApp) -> Window {
        let ctx = egui::Context::default();
        app.framework_transform = true;
        app.install_framework(&ctx);
        assert!(app.framework_installed, "the framework rows need the installed runtime");
        let mut ready = false;
        let mut h = Harness::builder().with_context(ctx).with_size(vec2(320.0, 600.0)).with_step_dt(0.05).build_ui_state(
            move |ui, app: &mut DesignApp| {
                if !ready {
                    crate::theme::install_fonts(ui.ctx(), &app.ui.language);
                    crate::theme::apply(ui.ctx(), &crate::theme::Tokens::for_brightness(Default::default()));
                    ready = true;
                    return;
                }
                show(app, ui);
            },
            app,
        );
        h.input_mut().max_texture_side = Some(8192);
        steps(&mut h, 4);
        h
    }

    fn steps(h: &mut Window, count: usize) {
        for _ in 0..count {
            let mut raw = std::mem::take(h.input_mut());
            let ctx = h.ctx.clone();
            h.state_mut().raw_input_hook(&ctx, &mut raw);
            *h.input_mut() = raw;
            h.step();
        }
    }

    fn event(h: &mut Window, event: Event) {
        let mut raw = std::mem::take(h.input_mut());
        raw.events.push(event);
        let ctx = h.ctx.clone();
        h.state_mut().raw_input_hook(&ctx, &mut raw);
        *h.input_mut() = raw;
        h.step();
    }

    fn button(pos: egui::Pos2, button: PointerButton, pressed: bool, modifiers: Modifiers) -> Event {
        Event::PointerButton { pos, button, pressed, modifiers }
    }

    fn click_with(h: &mut Window, at: egui::Pos2, which: PointerButton) {
        event(h, Event::PointerMoved(at));
        event(h, button(at, which, true, Modifiers::NONE));
        event(h, button(at, which, false, Modifiers::NONE));
        steps(h, 3);
    }

    fn click_at(h: &mut Window, at: egui::Pos2) {
        click_with(h, at, PointerButton::Primary);
    }

    /// Two clicks in a row, a frame apart.
    fn double_click(h: &mut Window, at: egui::Pos2) {
        event(h, Event::PointerMoved(at));
        for _ in 0..2 {
            event(h, button(at, PointerButton::Primary, true, Modifiers::NONE));
            event(h, button(at, PointerButton::Primary, false, Modifiers::NONE));
        }
        steps(h, 3);
    }

    /// Presses at `from`, drags to `to` in steps and releases, with `mods` held.
    fn drag_with(h: &mut Window, from: egui::Pos2, to: egui::Pos2, mods: Modifiers) {
        if mods != Modifiers::NONE {
            event(h, Event::ModifiersChanged(mods));
        }
        event(h, Event::PointerMoved(from));
        steps(h, 2);
        event(h, button(from, PointerButton::Primary, true, mods));
        steps(h, 1);
        for k in 1..=8 {
            event(h, Event::PointerMoved(from.lerp(to, k as f32 / 8.0)));
        }
        steps(h, 2);
        event(h, button(to, PointerButton::Primary, false, mods));
        if mods != Modifiers::NONE {
            event(h, Event::ModifiersChanged(Modifiers::NONE));
        }
        steps(h, 3);
    }

    fn drag(h: &mut Window, from: egui::Pos2, to: egui::Pos2) {
        drag_with(h, from, to, Modifiers::NONE);
    }

    fn center(h: &Window, label: &str) -> egui::Pos2 {
        h.get_by_label(label).rect().center()
    }

    fn shows(h: &Window, label: &str) -> bool {
        h.query_all_by_label(label).next().is_some()
    }

    fn journal(h: &Window, command: &str) -> usize {
        h.state().session.journal.iter().filter(|(id, _)| id == command).count()
    }

    fn doc(h: &Window) -> &Document {
        &h.state().session.active().unwrap().doc
    }

    fn layer_names(h: &Window) -> Vec<String> {
        doc(h).layers.iter().map(|l| l.name.clone()).collect()
    }

    fn layer_of(h: &Window, id: ItemId) -> LayerId {
        doc(h).item(id).unwrap().layer
    }

    fn frame(app: &mut DesignApp) -> ItemId {
        ItemId(app.run("frame.create", json!({"rect": [0, 0, 10, 10]})).unwrap()["id"].as_u64().unwrap())
    }

    fn name(app: &mut DesignApp, id: ItemId, name: &str) {
        app.run("object.rename", json!({"id": id.0, "name": name})).unwrap();
    }

    /// Layers Art (active, empty) and Layer 1, which holds the object `A`.
    fn two_layers() -> (DesignApp, LayerId, ItemId) {
        let mut app = app();
        let a = frame(&mut app);
        name(&mut app, a, "A");
        let art = LayerId(app.run("layer.new", json!({"name": "Art"})).unwrap()["id"].as_u64().unwrap());
        app.run("selection.set", json!({"ids": []})).unwrap();
        (app, art, a)
    }

    #[test]
    fn a_click_activates_a_layer_and_selects_an_object() {
        let (app, art, a) = two_layers();
        let l1 = app.session.active().unwrap().doc.layers[1].id;
        assert_eq!(app.session.active().unwrap().active_layer, art);
        let mut h = open(app);
        assert!(shows(&h, "Art") && shows(&h, "Layer 1"));
        assert!(!shows(&h, "A"), "Layer 1 starts closed: only the active layer shows its objects");
        click_at(&mut h, center(&h, "Layer 1"));
        assert_eq!(h.state().session.active().unwrap().active_layer, l1, "a click on a layer row activates it");
        assert!(shows(&h, "A"), "and the active layer shows its objects");
        click_at(&mut h, center(&h, "A"));
        assert_eq!(h.state().session.active().unwrap().selection.items, vec![a], "a click on an object row selects it");
    }

    #[test]
    fn an_eye_or_lock_cell_flips_one_flag_with_one_command() {
        let (mut app, art, a) = two_layers();
        app.run("layer.activate", json!({"id": app.session.active().unwrap().doc.layers[1].id.0})).unwrap();
        let mut h = open(app);
        let before = journal(&h, "layer.set");
        click_at(&mut h, center(&h, "Visibility Art"));
        assert!(!doc(&h).layer(art).unwrap().visible);
        click_at(&mut h, center(&h, "Lock Art"));
        assert!(doc(&h).layer(art).unwrap().locked);
        assert_eq!(journal(&h, "layer.set") - before, 2, "one command for each flip");
        click_at(&mut h, center(&h, "Visibility A"));
        assert!(doc(&h).item(a).unwrap().hidden);
        click_at(&mut h, center(&h, "Lock A"));
        assert!(doc(&h).item(a).unwrap().locked);
    }

    #[test]
    fn dragging_a_layer_row_below_the_last_one_reorders_the_layers_in_one_undo_step() {
        let (app, _, _) = two_layers();
        let mut h = open(app);
        assert_eq!(layer_names(&h), ["Art", "Layer 1"]);
        let (from, last) = (center(&h, "Art"), h.get_by_label("Layer 1").rect());
        drag(&mut h, from, egui::pos2(last.center().x, last.bottom() - 1.0));
        assert_eq!(layer_names(&h), ["Layer 1", "Art"]);
        h.state_mut().run("edit.undo", json!({})).unwrap();
        assert_eq!(layer_names(&h), ["Art", "Layer 1"], "one undo step");
    }

    #[test]
    fn a_layer_dropped_on_create_new_layer_is_duplicated() {
        let (app, _, _) = two_layers();
        let mut h = open(app);
        let (from, to) = (center(&h, "Layer 1"), center(&h, "Create New Layer"));
        drag(&mut h, from, to);
        assert_eq!(layer_names(&h), ["Art", "Layer 1 copy", "Layer 1"]);
    }

    #[test]
    fn the_footer_buttons_create_and_delete_layers() {
        let (app, _, _) = two_layers();
        let mut h = open(app);
        click_at(&mut h, center(&h, "Create New Layer"));
        assert_eq!(doc(&h).layers.len(), 3);
        click_at(&mut h, center(&h, "Delete Layer"));
        assert_eq!(doc(&h).layers.len(), 2, "the active layer goes");
    }

    /// Layers Art (active) and Layer 1 with `A`, and `X` on Art; both layers are open for the drags.
    fn objects_on_two_layers() -> (Window, ItemId, ItemId, LayerId, LayerId) {
        let (mut app, art, a) = two_layers();
        let l1 = app.session.active().unwrap().doc.layers[1].id;
        let x = frame(&mut app);
        name(&mut app, x, "X");
        let mut h = open(app);
        click_at(&mut h, center(&h, "Expand Layer 1"));
        (h, a, x, art, l1)
    }

    #[test]
    fn dragging_an_object_row_below_another_moves_it_behind_it_on_that_layer() {
        let (mut h, a, x, _, l1) = objects_on_two_layers();
        let a_row = h.get_by_label("A").rect();
        drag(&mut h, center(&h, "X"), egui::pos2(a_row.center().x, a_row.bottom() - 1.0));
        assert_eq!(layer_of(&h, x), l1);
        let order: Vec<ItemId> = doc(&h).spreads[0].items.iter().map(|i| i.id).collect();
        assert_eq!(order, [x, a], "X is behind A");
        h.state_mut().run("edit.undo", json!({})).unwrap();
        assert_ne!(layer_of(&h, x), l1, "one undo step");
    }

    #[test]
    fn a_filled_selection_square_drags_the_selection_to_another_layer() {
        let (mut h, a, x, art, l1) = objects_on_two_layers();
        h.state_mut().run("selection.set", json!({"ids": [a.0]})).unwrap();
        steps(&mut h, 3);
        let square = h.get_by_label("Select; drag to move the selection to another layer").rect().center();
        drag(&mut h, square, center(&h, "Art"));
        assert_eq!(layer_of(&h, a), art);
        assert_eq!(layer_of(&h, x), art);
        assert_ne!(layer_of(&h, a), l1);
    }

    #[test]
    fn a_locked_layer_takes_the_selection_only_with_cmd() {
        let (mut h, a, _, art, l1) = objects_on_two_layers();
        h.state_mut().run("layer.set", json!({"id": art.0, "locked": true})).unwrap();
        h.state_mut().run("selection.set", json!({"ids": [a.0]})).unwrap();
        steps(&mut h, 3);
        let square = h.get_by_label("Select; drag to move the selection to another layer").rect().center();
        drag(&mut h, square, center(&h, "Art"));
        assert_eq!(layer_of(&h, a), l1, "a locked layer refuses the drop");
        let square = h.get_by_label("Select; drag to move the selection to another layer").rect().center();
        drag_with(&mut h, square, center(&h, "Art"), Modifiers::COMMAND);
        assert_eq!(layer_of(&h, a), art, "Cmd forces it");
    }

    #[test]
    fn selecting_an_object_opens_its_layer_in_the_panel() {
        let mut app = app();
        let id = frame(&mut app);
        // A new layer becomes the active one: the first layer's row starts closed.
        app.run("layer.new", json!({})).unwrap();
        app.run("selection.set", json!({"ids": []})).unwrap();
        let mut h = open(app);
        assert!(!shows(&h, "Collapse Layer 1"), "closed while nothing on it is selected");
        h.state_mut().run("selection.set", json!({"ids": [id.0]})).unwrap();
        steps(&mut h, 3);
        assert!(shows(&h, "Collapse Layer 1"), "selecting the object opens its layer");
        // Closed again by hand, it stays closed while the selection stays the same.
        click_at(&mut h, center(&h, "Collapse Layer 1"));
        assert!(shows(&h, "Expand Layer 1"));
        steps(&mut h, 3);
        assert!(shows(&h, "Expand Layer 1"));
    }

    /// The text field of the rename in place, once it is open.
    fn renaming(h: &Window) -> bool {
        h.query_by_role(egui::accesskit::Role::TextInput).is_some()
    }

    fn rename_target() -> (Window, ItemId) {
        let mut app = app();
        let id = frame(&mut app);
        app.run("selection.set", json!({"ids": []})).unwrap();
        let mut h = open(app);
        // The layer is the active one and open; the object is the only row under it.
        assert!(shows(&h, "<rectangle>"));
        (h, id)
    }

    /// Opens the rename of the object row at `row` as a person does: a click on an unselected row selects it, and a
    /// click on the selected one, with no second click after it, renames it.
    fn start_rename(h: &mut Window, row: egui::Pos2) {
        click_at(h, row);
        steps(h, 12);
        if !renaming(h) {
            click_at(h, row);
            steps(h, 12);
        }
    }

    fn press_key(h: &mut Window, key: Key) {
        event(h, Event::Key { key, physical_key: None, pressed: true, repeat: false, modifiers: Modifiers::NONE });
        event(h, Event::Key { key, physical_key: None, pressed: false, repeat: false, modifiers: Modifiers::NONE });
        steps(h, 3);
    }

    /// As InDesign's: a slow double-click on an object's row renames it; a quick one does not.
    #[test]
    fn a_slow_double_click_renames_an_object_and_a_quick_one_does_not() {
        let (mut h, id) = rename_target();
        let row = center(&h, "<rectangle>");
        double_click(&mut h, row);
        assert_eq!(h.state().session.active().unwrap().selection.items, vec![id]);
        assert!(!renaming(&h), "a double-click doesn't rename");
        steps(&mut h, 12);
        click_at(&mut h, row);
        steps(&mut h, 12);
        assert!(renaming(&h), "a slow double-click renames");
    }

    #[test]
    fn a_row_renames_in_place_and_escape_or_an_unedited_label_leave_the_name() {
        let (mut h, id) = rename_target();
        let row = center(&h, "<rectangle>");
        let object_name = |h: &Window| doc(h).item(id).unwrap().name.clone();
        // Enter on the automatic label as it was opened keeps the object following it.
        start_rename(&mut h, row);
        assert!(renaming(&h));
        press_key(&mut h, Key::Enter);
        assert!(!renaming(&h) && object_name(&h).is_empty(), "an unedited label is not a name");
        // Typed over, Enter names it.
        start_rename(&mut h, row);
        h.get_by_role(egui::accesskit::Role::TextInput).type_text("Hero bottle");
        steps(&mut h, 2);
        press_key(&mut h, Key::Enter);
        assert_eq!(object_name(&h), "Hero bottle");
        // Escape leaves the name as it was.
        let row = center(&h, "Hero bottle");
        start_rename(&mut h, row);
        h.get_by_role(egui::accesskit::Role::TextInput).type_text("Oops");
        press_key(&mut h, Key::Escape);
        assert_eq!(object_name(&h), "Hero bottle");
    }

    #[test]
    fn double_clicking_a_layer_opens_its_options() {
        let (app, _, _) = two_layers();
        let mut h = open(app);
        let at = center(&h, "Art");
        double_click(&mut h, at);
        assert_eq!(h.state().ui.dialog.as_ref().map(|d| d.id.as_str()), Some("layerSettings"));
    }

    #[test]
    fn a_layer_rows_menu_runs_the_layer_commands_and_an_object_row_has_none() {
        let (app, _, _) = two_layers();
        let mut h = open(app);
        click_with(&mut h, center(&h, "Art"), PointerButton::Secondary);
        assert!(shows(&h, "Duplicate Layer \"Art\""));
        assert!(shows(&h, "Delete Unused Layers"));
        assert!(!shows(&h, "Merge into Active Layer"), "Art is the active layer");
        click_at(&mut h, center(&h, "Duplicate Layer \"Art\""));
        assert_eq!(layer_names(&h), ["Art copy", "Art", "Layer 1"]);
        // A right-click on an object row asks for no layer menu.
        click_at(&mut h, center(&h, "Layer 1"));
        click_with(&mut h, center(&h, "A"), PointerButton::Secondary);
        assert!(!shows(&h, "Delete Unused Layers"));
    }
}
