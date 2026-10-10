//! Layers panel: expandable layer rows listing the current spread's objects topmost first
//! (`<rectangle>`, `<first words…>` for text frames; groups expand to their members), with eye /
//! lock columns per layer and per object and a selection square in the layer colour. Drag a
//! layer row to reorder layers, an object row to restack it or move it to another layer (group
//! members restack within their group), and a filled selection square onto another layer to move
//! the selection there. A selected row drags the selected objects that share its parent (for a
//! top-level row, those on its layer). Locked and hidden layers take drops only with Cmd/Ctrl
//! held.

use designcraft_doc::{Content, Document, Item, ItemId, LayerId};
use egui::{Color32, Rect, Sense, Stroke, StrokeKind, Ui, pos2, vec2};
use serde_json::json;

use crate::theme::Tokens;
use crate::{DesignApp, icons};

const ROW_H: f32 = 24.0;
/// Width of the eye and lock columns.
const COL_W: f32 = 22.0;

/// Display name of an item: its own name, the first words of a text frame's story, or the kind.
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
    let e = ui.interact(eye, ui.id().with(("eye", key)), Sense::click()).on_hover_ui(|ui| {
        crate::rtl::label(ui, crate::i18n::tr(language, if visible { "Hide" } else { "Show" }));
    });
    let l = ui.interact(lock, ui.id().with(("lock", key)), Sense::click()).on_hover_ui(|ui| {
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
    let resp = ui.interact(r.expand(4.0), ui.id().with(("selsq", key)), sense);
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
        if resp.hovered() {
            ui.painter().rect_filled(row, 0.0, t.hover);
        }
        let (eye, lock) = eye_lock(ui, ls.language, row, !it.hidden, it.locked, !ls.layer_visible, ("item", it.id.0));
        let indent = 14.0 * depth as f32;
        let expandable = !it.children().is_empty() && it.states.is_empty() && depth < MAX_DEPTH;
        let open_id = egui::Id::new(("group_open", it.id.0));
        let open = expandable && ui.data(|d| d.get_temp(open_id)).unwrap_or(false);
        let mut tri_clicked = false;
        if expandable {
            let tri = Rect::from_min_size(row.min + vec2(2.0 * COL_W + 14.0 + indent, 0.0), vec2(14.0, ROW_H));
            disclosure(ui, tri, open, t.icon);
            tri_clicked = ui.interact(tri, ui.id().with(("gtri", it.id.0)), Sense::click()).clicked();
        }
        let name = item_label(ls.doc, it);
        let clip = Rect::from_min_max(row.min, pos2(row.max.x - 26.0, row.max.y));
        ui.painter().with_clip_rect(clip).text(
            row.min + vec2(2.0 * COL_W + 30.0 + indent, ROW_H / 2.0),
            egui::Align2::LEFT_CENTER,
            &name,
            egui::FontId::proportional(11.0),
            if it.hidden { t.text_dim } else { t.text },
        );
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
            let add = ui.input(|i| i.modifiers.shift || i.modifiers.command);
            cmds.push(("selection.set", json!({"ids": [it.id.0], "add": add})));
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

pub fn show(app: &mut DesignApp, ui: &mut Ui) {
    let t = Tokens::get(ui.ctx());
    let Some(st) = app.session.active() else { return };
    let doc = st.doc.clone();
    let active = st.active_layer;
    let selected: Vec<ItemId> = st.selection.items.clone();
    let sel_layers: Vec<_> = selected.iter().filter_map(|i| doc.item(*i).map(|it| it.layer)).collect();
    let language = app.ui.language.clone();
    // Objects on the current spread, back to front.
    let cur = crate::canvas::current_page(app).unwrap_or(0);
    let spread_items: Vec<std::sync::Arc<Item>> =
        doc.page_loc(cur).and_then(|(si, _)| doc.spreads.get(si)).map(|sp| sp.items.clone()).unwrap_or_default();
    let mut rows: Vec<Row> = Vec::new();
    let mut cmds: Vec<(&'static str, serde_json::Value)> = Vec::new();
    ui.spacing_mut().item_spacing.y = 0.0;
    // `doc.layers` runs top (frontmost) to bottom, as the panel lists them.
    for (li, l) in doc.layers.iter().enumerate() {
        let open_id = egui::Id::new(("layer_open", l.id.0));
        let open: bool = ui.data(|d| d.get_temp(open_id)).unwrap_or(l.id == active);
        let lc = c32(l.color);
        let (row, resp) = ui.allocate_exact_size(vec2(ui.available_width(), ROW_H), Sense::click_and_drag());
        if l.id == active {
            ui.painter().rect_filled(row, 0.0, t.row_selected);
        } else if resp.hovered() {
            ui.painter().rect_filled(row, 0.0, t.hover);
        }
        let (eye, lock) = eye_lock(ui, &language, row, l.visible, l.locked, false, ("layer", l.id.0));
        let tri = Rect::from_min_size(row.min + vec2(2.0 * COL_W + 2.0, 0.0), vec2(14.0, ROW_H));
        disclosure(ui, tri, open, t.icon);
        let tri_r = ui.interact(tri, ui.id().with(("tri", l.id.0)), Sense::click());
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
        } else if resp.clicked() {
            cmds.push(("layer.activate", json!({"id": l.id.0})));
        }
        resp.context_menu(|ui| {
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
                "Page: {}, {} Layer{}",
                app.session.page_label(cur),
                doc.layers.len(),
                if doc.layers.len() == 1 { "" } else { "s" }
            ))
            .size(11.0)
            .color(t.text_dim),
        );
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            if icons::button(ui, "trash", 20.0, false, crate::i18n::tr(&app.ui.language, "Delete Layer")).clicked() {
                let _ = app.run("layer.delete", json!({"id": active.0}));
            }
            if icons::button(ui, "plus", 20.0, false, crate::i18n::tr(&app.ui.language, "Create New Layer")).clicked() {
                let _ = app.run("layer.new", json!({}));
            }
        });
    });
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
}
