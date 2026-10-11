//! Glyphs panel: every character of a font in a grid; click to insert it at the text cursor.

use egui::{Sense, vec2};
use serde_json::json;

use crate::DesignApp;
use crate::theme::Tokens;

const CELL: f32 = 30.0;
/// Characters shown at most (the rest via the search field).
const MAX: usize = 1200;

#[derive(Clone, Default)]
struct State {
    family: String,
    style: String,
    query: String,
    recent: Vec<char>,
}

pub fn show(app: &mut DesignApp, ui: &mut egui::Ui) {
    let t = Tokens::get(ui.ctx());
    let id = egui::Id::new("glyphs_panel");
    let mut st: State = ui.data(|d| d.get_temp(id)).unwrap_or_default();
    // Default to the font at the text cursor.
    if st.family.is_empty() {
        let a = super::text_attrs(app);
        st.family = a
            .as_ref()
            .and_then(|a| a["chars"]["fontFamily"].as_str().map(str::to_string))
            .unwrap_or_else(|| designcraft_fonts::DEFAULT_FAMILY.into());
        st.style = a.as_ref().and_then(|a| a["chars"]["fontStyle"].as_str().map(str::to_string)).unwrap_or_else(|| "Regular".into());
    }
    let (db, scope) = (super::fonts(app), super::font_scope(app));
    ui.horizontal(|ui| {
        if let Some(f) = super::font_combo(app, ui, "glyph_family", &st.family, 140.0) {
            st.style = db.styles(&f).into_iter().next().unwrap_or_else(|| "Regular".into());
            st.family = f;
        }
        egui::ComboBox::from_id_salt("glyph_style").selected_text(&st.style).width(80.0).show_ui(ui, |ui| {
            for s in db.styles(&st.family) {
                if ui.selectable_label(s == st.style, &s).clicked() {
                    st.style = s;
                }
            }
        });
    });
    ui.add(
        egui::TextEdit::singleline(&mut st.query)
            .hint_text(crate::rtl::widget(ui, crate::i18n::tr(&app.ui.language, "Search: character or U+code")))
            .desired_width(f32::INFINITY),
    );
    let face = db.face(&st.family, &st.style);
    let q = st.query.trim().to_string();
    let code = q.strip_prefix("U+").or_else(|| q.strip_prefix("u+")).and_then(|h| u32::from_str_radix(h, 16).ok());
    let mut chars: Vec<char> = face.chars().into_iter().map(|(c, _)| c).filter(|c| !c.is_control()).collect();
    chars.sort();
    chars.dedup();
    if !q.is_empty() {
        chars.retain(|c| Some(*c as u32) == code || q.contains(*c) || format!("{:04X}", *c as u32).contains(&q.to_ascii_uppercase()));
    }
    chars.truncate(MAX);
    let mut insert: Option<char> = None;
    if !st.recent.is_empty() {
        ui.horizontal_wrapped(|ui| {
            crate::rtl::label(ui, egui::RichText::new(crate::i18n::tr(&app.ui.language, "Recently Used")).size(10.5).color(t.text_dim));
            for c in st.recent.clone() {
                if ui.small_button(c.to_string()).on_hover_text(format!("U+{:04X}", c as u32)).clicked() {
                    insert = Some(c);
                }
            }
        });
    }
    // The scroll bar floats over the grid's right edge: leave it room.
    let scroll = &ui.spacing().scroll;
    let bar = scroll.bar_inner_margin + scroll.bar_width + scroll.bar_outer_margin;
    let cols = (((ui.available_width() - bar) / CELL).floor() as u32).max(1);
    let ppp = ui.ctx().pixels_per_point();
    let cell_px = (CELL * ppp).round() as u32;
    let ink = t.text_strong;
    let key = egui::Id::new(("glyph_grid", &st.family, &st.style, scope, &q, cols, cell_px, ink.to_array()));
    let tex: egui::TextureHandle = match ui.data(|d| d.get_temp::<egui::TextureHandle>(key)) {
        Some(t) => t,
        None => {
            let img = designcraft_render::glyphs::glyph_grid(&db, &st.family, &st.style, &chars, cols, cell_px, ink.to_array());
            let ci = egui::ColorImage::from_rgba_premultiplied([img.width as usize, img.height as usize], &img.pixels);
            let h = ui.ctx().load_texture("glyph_grid", ci, egui::TextureOptions::LINEAR);
            ui.data_mut(|d| d.insert_temp(key, h.clone()));
            h
        }
    };
    let rows = (chars.len() as u32).div_ceil(cols);
    egui::ScrollArea::vertical().max_height(320.0).show(ui, |ui| {
        let size = vec2(cols as f32 * CELL, rows as f32 * CELL);
        let (r, resp) = ui.allocate_exact_size(size, Sense::click());
        ui.painter().rect_filled(r, 0.0, t.input);
        ui.painter().image(tex.id(), r, egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0)), egui::Color32::WHITE);
        for c in 1..cols {
            let x = r.min.x + c as f32 * CELL;
            ui.painter().line_segment([egui::pos2(x, r.min.y), egui::pos2(x, r.max.y)], egui::Stroke::new(1.0, t.border));
        }
        for row in 1..rows {
            let y = r.min.y + row as f32 * CELL;
            ui.painter().line_segment([egui::pos2(r.min.x, y), egui::pos2(r.max.x, y)], egui::Stroke::new(1.0, t.border));
        }
        let cell_at = |p: egui::Pos2| {
            let (cx, cy) = (((p.x - r.min.x) / CELL) as u32, ((p.y - r.min.y) / CELL) as u32);
            chars.get((cy * cols + cx.min(cols - 1)) as usize).copied()
        };
        if let Some(p) = resp.hover_pos()
            && let Some(c) = cell_at(p)
        {
            let (cx, cy) = (((p.x - r.min.x) / CELL).floor(), ((p.y - r.min.y) / CELL).floor());
            let cr = egui::Rect::from_min_size(r.min + vec2(cx * CELL, cy * CELL), vec2(CELL, CELL));
            ui.painter().rect_stroke(cr, 0.0, egui::Stroke::new(1.5, t.accent), egui::StrokeKind::Inside);
            resp.clone().on_hover_text(format!("U+{:04X}", c as u32));
        }
        if resp.clicked()
            && let Some(c) = resp.interact_pointer_pos().and_then(cell_at)
        {
            insert = Some(c);
        }
    });
    if let Some(c) = insert {
        match app.run("text.insert", json!({"text": c.to_string(), "raw": true})) {
            Ok(_) => {
                st.recent.retain(|x| *x != c);
                st.recent.insert(0, c);
                st.recent.truncate(12);
            }
            Err(e) => app.status(format!("Glyphs: {e}")),
        }
    }
    crate::rtl::label(
        ui,
        egui::RichText::new(
            crate::i18n::tr(&app.ui.language, "{count} glyphs — click to insert at the text cursor").replace("{count}", &chars.len().to_string()),
        )
        .size(10.5)
        .color(t.text_dim),
    );
    ui.data_mut(|d| d.insert_temp(id, st));
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_grid_leaves_room_for_the_scroll_bar() {
        let mut app = DesignApp::new(designcraft_engine::Session::new(), crate::Services::default());
        app.run("file.new", json!({})).unwrap();
        let ctx = egui::Context::default();
        crate::theme::install_fonts(&ctx, "");
        crate::theme::apply(&ctx, &Tokens::for_brightness(crate::theme::Brightness::MediumDark));
        let scroll = ctx.global_style().spacing.scroll;
        let bar = scroll.bar_inner_margin + scroll.bar_width + scroll.bar_outer_margin;
        for width in [236.0, 256.0, 300.0, 333.0] {
            let mut panel = egui::Rect::NOTHING;
            let mut grid = None;
            for _ in 0..2 {
                // The app raises the texture limit for the grid (see `test_window`).
                let raw = egui::RawInput {
                    screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, vec2(800.0, 700.0))),
                    max_texture_side: Some(8192),
                    ..Default::default()
                };
                let mut out = ctx.run_ui(raw, |ui| {
                    ui.allocate_ui(vec2(width, 650.0), |ui| {
                        ui.set_width(width);
                        panel = ui.max_rect();
                        show(&mut app, ui);
                    });
                });
                out.textures_delta.clear();
                // The grid is the one textured mesh that isn't text.
                grid = out.shapes.iter().find_map(|s| match &s.shape {
                    egui::Shape::Mesh(m) if m.texture_id != egui::TextureId::default() => Some(m.calc_bounds()),
                    _ => None,
                });
            }
            let grid = grid.expect("the glyph grid is drawn");
            assert!(grid.right() <= panel.right() - bar + 0.5, "{width}: the grid {grid:?} runs under the scroll bar of {panel:?}");
            assert!(grid.right() > panel.right() - bar - CELL, "{width}: no column is left out ({grid:?})");
        }
    }
}
