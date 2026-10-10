//! Layers panel: expandable layer rows listing the current spread's objects (`<rectangle>`,
//! `<first words…>` for text frames, `<table>`), groups expandable in turn, with eye / lock columns
//! per layer and per object and a selection square in the layer colour.

use designcraft_doc::{Content, Document, Item, ItemId};
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
        if !story.tables.is_empty() {
            return "<table>".to_string();
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

/// The selection square at the right of a row: filled = selected; hollow on hover.
fn selection_square(
    ui: &mut Ui,
    language: &str,
    row: Rect,
    color: Color32,
    selected: bool,
    small: bool,
    key: impl std::hash::Hash + std::fmt::Debug,
) -> bool {
    let s = if small { 7.0 } else { 8.0 };
    let r = Rect::from_center_size(pos2(row.max.x - 12.0, row.center().y), vec2(s, s));
    let resp = ui.interact(r.expand(4.0), ui.id().with(("selsq", key)), Sense::click());
    if selected {
        ui.painter().rect_filled(r, 0.0, color);
    } else if resp.hovered() {
        ui.painter().rect_stroke(r, 0.0, Stroke::new(1.0, color), StrokeKind::Inside);
    }
    resp.on_hover_ui(|ui| {
        crate::rtl::label(ui, crate::i18n::tr(language, "Select"));
    })
    .clicked()
}

/// Indent per group level.
const INDENT: f32 = 12.0;

/// Whether `it` or anything inside it is selected.
fn holds_selection(it: &Item, selected: &[ItemId]) -> bool {
    selected.contains(&it.id) || it.children().iter().any(|c| holds_selection(c, selected))
}

/// One object's row and, for an open group, its children's rows (topmost first) below it.
#[allow(clippy::too_many_arguments)]
fn item_rows(app: &mut DesignApp, ui: &mut Ui, doc: &Document, it: &Item, depth: usize, lc: Color32, layer_visible: bool, selected: &[ItemId]) {
    let t = Tokens::get(ui.ctx());
    let (row, resp) = ui.allocate_exact_size(vec2(ui.available_width(), ROW_H), Sense::click());
    let is_sel = selected.contains(&it.id);
    if resp.hovered() {
        ui.painter().rect_filled(row, 0.0, t.hover);
    }
    let (eye, lock) = eye_lock(ui, &app.ui.language, row, !it.hidden, it.locked, !layer_visible, ("item", it.id.0));
    let indent = depth as f32 * INDENT;
    // Groups (and frames holding pasted-in objects) open like layers.
    let kids = it.children();
    let open_id = egui::Id::new(("group_open", it.id.0));
    let mut tri_clicked = false;
    let open = !kids.is_empty() && {
        // Closed by default, unless something inside is selected.
        let open: bool = ui.data(|d| d.get_temp(open_id)).unwrap_or_else(|| kids.iter().any(|c| holds_selection(c, selected)));
        let tri = Rect::from_min_size(row.min + vec2(2.0 * COL_W + 14.0 + indent, 0.0), vec2(14.0, ROW_H));
        let c = tri.center();
        let pts = if open {
            vec![c + vec2(-3.5, -1.5), c + vec2(3.5, -1.5), c + vec2(0.0, 2.5)]
        } else {
            vec![c + vec2(-1.5, -3.5), c + vec2(2.5, 0.0), c + vec2(-1.5, 3.5)]
        };
        ui.painter().add(egui::Shape::convex_polygon(pts, t.icon, Stroke::NONE));
        tri_clicked = ui.interact(tri, ui.id().with(("gtri", it.id.0)), Sense::click()).clicked();
        open
    };
    let name = item_label(doc, it);
    let clip = Rect::from_min_max(row.min, pos2(row.max.x - 26.0, row.max.y));
    ui.painter().with_clip_rect(clip).text(
        row.min + vec2(2.0 * COL_W + 30.0 + indent, ROW_H / 2.0),
        egui::Align2::LEFT_CENTER,
        &name,
        egui::FontId::proportional(11.0),
        if it.hidden { t.text_dim } else { t.text },
    );
    let sq = selection_square(ui, &app.ui.language, row, lc, is_sel, true, ("item", it.id.0));
    if eye {
        let _ = app.run("object.setFlags", json!({"ids": [it.id.0], "hidden": !it.hidden}));
    } else if lock {
        let _ = app.run("object.setFlags", json!({"ids": [it.id.0], "locked": !it.locked}));
    } else if tri_clicked {
        ui.data_mut(|d| d.insert_temp(open_id, !open));
    } else if sq || resp.clicked() {
        let add = ui.input(|i| i.modifiers.shift || i.modifiers.command);
        let _ = app.run("selection.set", json!({"ids": [it.id.0], "add": add}));
    }
    if open && depth < 64 {
        for c in kids.iter().rev() {
            item_rows(app, ui, doc, c, depth + 1, lc, layer_visible, selected);
        }
    }
}

pub fn show(app: &mut DesignApp, ui: &mut Ui) {
    let t = Tokens::get(ui.ctx());
    let Some(st) = app.session.active() else { return };
    let doc = st.doc.clone();
    let active = st.active_layer;
    let selected: Vec<ItemId> = st.selection.items.clone();
    let sel_layers: Vec<_> = selected.iter().filter_map(|i| doc.item(*i).map(|it| it.layer)).collect();
    // Objects on the current spread, topmost first.
    let cur = crate::canvas::current_page(app).unwrap_or(0);
    let spread_items: Vec<std::sync::Arc<Item>> =
        doc.page_loc(cur).and_then(|(si, _)| doc.spreads.get(si)).map(|sp| sp.items.iter().rev().cloned().collect()).unwrap_or_default();
    ui.spacing_mut().item_spacing.y = 0.0;
    for l in doc.layers.iter().rev() {
        let open_id = egui::Id::new(("layer_open", l.id.0));
        let open: bool = ui.data(|d| d.get_temp(open_id)).unwrap_or(l.id == active);
        let lc = c32(l.color);
        let (row, resp) = ui.allocate_exact_size(vec2(ui.available_width(), ROW_H), Sense::click());
        if l.id == active {
            ui.painter().rect_filled(row, 0.0, t.row_selected);
        } else if resp.hovered() {
            ui.painter().rect_filled(row, 0.0, t.hover);
        }
        let (eye, lock) = eye_lock(ui, &app.ui.language, row, l.visible, l.locked, false, ("layer", l.id.0));
        // Disclosure triangle.
        let tri = Rect::from_min_size(row.min + vec2(2.0 * COL_W + 2.0, 0.0), vec2(14.0, ROW_H));
        let c = tri.center();
        let pts = if open {
            vec![c + vec2(-4.0, -2.0), c + vec2(4.0, -2.0), c + vec2(0.0, 3.0)]
        } else {
            vec![c + vec2(-2.0, -4.0), c + vec2(3.0, 0.0), c + vec2(-2.0, 4.0)]
        };
        ui.painter().add(egui::Shape::convex_polygon(pts, t.icon, Stroke::NONE));
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
        let sq = selection_square(ui, &app.ui.language, row, lc, sel_layers.contains(&l.id), false, ("layer", l.id.0));
        if eye {
            let _ = app.run("layer.set", json!({"id": l.id.0, "visible": !l.visible}));
        } else if lock {
            let _ = app.run("layer.set", json!({"id": l.id.0, "locked": !l.locked}));
        } else if tri_r.clicked() {
            ui.data_mut(|d| d.insert_temp(open_id, !open));
        } else if sq {
            let ids: Vec<u64> = spread_items.iter().filter(|i| i.layer == l.id).map(|i| i.id.0).collect();
            let _ = app.run("selection.set", json!({"ids": ids}));
        } else if resp.clicked() {
            let _ = app.run("layer.activate", json!({"id": l.id.0}));
        }
        resp.context_menu(|ui| {
            if ui.button(crate::rtl::widget(ui, crate::i18n::tr(&app.ui.language, "Delete Layer"))).clicked() {
                let _ = app.run("layer.delete", json!({"id": l.id.0}));
                ui.close();
            }
            if ui.button(crate::rtl::widget(ui, crate::i18n::tr(&app.ui.language, "Move Selection Here"))).clicked() {
                let _ = app.run("object.setLayer", json!({"layer": l.id.0}));
                ui.close();
            }
            ui.separator();
            let active = app.session.active().map(|d| d.active_layer);
            if let Some(a) = active.filter(|a| *a != l.id)
                && ui.button(crate::rtl::widget(ui, crate::i18n::tr(&app.ui.language, "Merge into Active Layer"))).clicked()
            {
                let _ = app.run("layer.merge", json!({"ids": [a.0, l.id.0], "into": a.0}));
                ui.close();
            }
            for (label, params) in [
                ("Hide Others", json!({"id": l.id.0, "hide": true})),
                ("Lock Others", json!({"id": l.id.0, "lock": true})),
                ("Show All Layers", json!({"id": l.id.0, "show": true})),
                ("Unlock All Layers", json!({"id": l.id.0, "unlock": true})),
            ] {
                if ui.button(crate::rtl::widget(ui, crate::i18n::tr(&app.ui.language, label))).clicked() {
                    let _ = app.run("layer.others", params);
                    ui.close();
                }
            }
            if ui.button(crate::rtl::widget(ui, crate::i18n::tr(&app.ui.language, "Delete Unused Layers"))).clicked() {
                let _ = app.run("layer.deleteUnused", json!({}));
                ui.close();
            }
        });
        if !open {
            continue;
        }
        for it in spread_items.iter().filter(|i| i.layer == l.id) {
            item_rows(app, ui, &doc, it, 0, lc, l.visible, &selected);
        }
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

    #[test]
    fn a_frame_holding_just_a_table_is_labelled_table() {
        let mut s = designcraft_engine::Session::new();
        s.execute("file.new", &json!({})).unwrap();
        let r = s.execute("frame.create", &json!({"rect": [36, 36, 300, 200], "content": "text"})).unwrap();
        s.execute("table.insert", &json!({"rows": 2, "cols": 2})).unwrap();
        let doc = &s.doc().unwrap().doc;
        assert_eq!(item_label(doc, doc.item(ItemId(r["id"].as_u64().unwrap())).unwrap()), "<table>");
    }
}
