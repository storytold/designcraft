//! Swatches and Color panels.

use designcraft_color::{ColorType, SwatchValue};
use egui::{Sense, vec2};
use serde_json::json;

use crate::theme::Tokens;
use crate::{DesignApp, icons};

pub fn show(app: &mut DesignApp, ui: &mut egui::Ui) {
    let t = Tokens::get(ui.ctx());
    let Some(st) = app.session.active() else { return };
    let doc = st.doc.clone();
    // Another document (switched to, or moved to this one's place by a close) drops a rename in
    // place: a swatch of the same name there isn't the one being renamed.
    let uid = st.uid;
    if ui.data(|d| d.get_temp::<u64>(egui::Id::unique(RENAME_DOC))) != Some(uid) {
        ui.data_mut(|d| {
            d.remove_temp::<(String, String)>(egui::Id::unique(RENAMING));
            d.remove_temp::<(String, f64)>(egui::Id::unique(RENAME_CLICK));
            d.insert_temp(egui::Id::unique(RENAME_DOC), uid);
        });
    }
    // With nothing selected, the proxy, Tint field and highlighted row are those of new shapes.
    let target = super::color_target_or_defaults(app);
    header(app, ui, &doc, target.as_ref());
    // The row of the colour in front of the proxy is highlighted.
    let fill_cur = target.map(|(fill, _, stroke, _)| if app.ui.stroke_front { stroke } else { fill });
    // Unnamed colours (mixed in the Color panel) aren't listed until Add to Swatches; swatches in
    // colour groups are listed under their folder.
    let grouped: Vec<&str> = doc.color_groups.iter().flat_map(|g| g.swatches.iter().map(String::as_str)).collect();
    // A gradient dragged from the Gradient panel's thumbnail is dropped on the list, colour
    // groups included.
    let (_, dropped) = ui.dnd_drop_zone::<GradientDrag, _>(egui::Frame::NONE, |ui| {
        for sw in doc.swatches.iter().filter(|w| !w.hidden && !grouped.contains(&w.name.as_str())) {
            swatch_row(app, ui, &doc, sw, 0.0, fill_cur.as_deref());
        }
        for g in &doc.color_groups {
            let open_id = egui::Id::unique(("color_group_open", &g.name));
            let mut open: bool = ui.data(|d| d.get_temp(open_id)).unwrap_or(true);
            let (row, resp) = ui.allocate_exact_size(vec2(ui.available_width(), 22.0), Sense::click());
            resp.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Button, true, &g.name));
            if resp.hovered() {
                ui.painter().rect_filled(row, 0.0, t.hover);
            }
            icons::paint(
                ui.painter(),
                egui::Rect::from_min_size(row.min + vec2(2.0, 3.0), vec2(16.0, 16.0)),
                if open { "chevron-down" } else { "chevron-right" },
                t.text_dim,
            );
            // A folder.
            let f = egui::Rect::from_min_size(row.min + vec2(20.0, 5.0), vec2(16.0, 12.0));
            ui.painter().rect_stroke(f, 1.5, egui::Stroke::new(1.2, t.text_dim), egui::StrokeKind::Inside);
            ui.painter().line_segment([f.left_top() + vec2(1.0, 3.0), f.right_top() + vec2(-1.0, 3.0)], egui::Stroke::new(1.0, t.text_dim));
            ui.painter().text(row.min + vec2(42.0, 11.0), egui::Align2::LEFT_CENTER, &g.name, egui::FontId::proportional(12.5), t.text);
            // A double click's first click folded the group: it stays as it was.
            if resp.clicked() {
                open = !open;
                ui.data_mut(|d| d.insert_temp(open_id, open));
            }
            if resp.double_clicked() {
                let _ = app.run("app.colorGroupOptionsDialog", json!({"name": g.name}));
            }
            resp.context_menu(|ui| {
                if ui.button(crate::rtl::widget(ui, crate::i18n::tr(&app.ui.language, "Color Group Options…"))).clicked() {
                    let _ = app.run("app.colorGroupOptionsDialog", json!({"name": g.name}));
                    ui.close();
                }
                if ui.button(crate::rtl::widget(ui, crate::i18n::tr(&app.ui.language, "Ungroup Color Group"))).clicked() {
                    let _ = app.run("swatch.ungroupColorGroup", json!({"name": g.name}));
                    ui.close();
                }
            });
            if open {
                for n in &g.swatches {
                    if let Some(sw) = doc.swatches.iter().find(|w| w.name == *n) {
                        swatch_row(app, ui, &doc, sw, 16.0, fill_cur.as_deref());
                    }
                }
            }
        }
    });
    // An unnamed gradient becomes "New Gradient Swatch"; one that is a swatch already stays one.
    if let Some(g) = dropped
        && doc.swatch(&g.0).is_some_and(|w| w.hidden)
    {
        let _ = app.run("swatch.new", json!({"swatch": g.0}));
    }
    ui.add_space(6.0);
    // InDesign's bottom row: New Color Group, New Swatch, Delete Swatch (the one last clicked).
    let picked: Option<String> = ui.data(|d| d.get_temp(egui::Id::unique(PICKED))).filter(|n: &String| doc.swatch(n).is_some_and(|w| !w.locked));
    ui.horizontal(|ui| {
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            let del = ui
                .add_enabled_ui(picked.is_some(), |ui| icons::button(ui, "trash", 20.0, false, crate::i18n::tr(&app.ui.language, "Delete Swatch")))
                .inner;
            if del.clicked()
                && let Some(name) = &picked
                && app.run("swatch.delete", json!({"name": name})).is_ok()
            {
                ui.data_mut(|d| d.remove::<String>(egui::Id::unique(PICKED)));
            }
            // The selection's fill or stroke (whichever is in front) becomes a swatch; with nothing
            // selected, a new colour.
            if icons::button(ui, "plus", 20.0, false, crate::i18n::tr(&app.ui.language, "New Swatch")).clicked() {
                let target = if app.ui.stroke_front { "stroke" } else { "fill" };
                let _ = app.run("swatch.new", json!({"target": target}));
            }
            if folder_button(ui, crate::i18n::tr(&app.ui.language, "New Color Group")).clicked() {
                let _ = app.run("swatch.newColorGroup", json!({}));
            }
            crate::menus::menu_button(ui, "☰", |ui| {
                let current = fill_cur.as_deref().filter(|n| doc.swatch(n).is_some_and(|w| !w.hidden));
                if ui
                    .add_enabled(current.is_some(), egui::Button::new(crate::rtl::widget(ui, crate::i18n::tr(&app.ui.language, "Swatch Options…"))))
                    .clicked()
                {
                    let _ = app.run("app.swatchOptionsDialog", json!({"name": current}));
                    ui.close();
                }
                if ui.button(crate::rtl::widget(ui, crate::i18n::tr(&app.ui.language, "Color Theme…"))).clicked() {
                    crate::menus::activate(app, "swatch.addColorTheme", &serde_json::Value::Null);
                    ui.close();
                }
                crate::menus::menu_button(ui, crate::rtl::widget(ui, crate::i18n::tr(&app.ui.language, "Ink Manager")), |ui| {
                    let Ok(l) = app.session.execute("ink.list", &json!({})) else { return };
                    let mut all = l["allToProcess"].as_bool().unwrap_or(false);
                    if ui.checkbox(&mut all, crate::rtl::widget(ui, crate::i18n::tr(&app.ui.language, "All Spots to Process"))).changed() {
                        let _ = app.run("ink.options", json!({"allToProcess": all}));
                        ui.close();
                    }
                    ui.separator();
                    let inks = l["inks"].as_array().cloned().unwrap_or_default();
                    if inks.is_empty() {
                        crate::rtl::label(ui, crate::i18n::tr(&app.ui.language, "No spot inks"));
                    }
                    for ink in inks {
                        let name = ink["name"].as_str().unwrap_or("").to_string();
                        let mut process = ink["process"].as_bool().unwrap_or(false);
                        ui.add_enabled_ui(!all, |ui| {
                            if ui.checkbox(&mut process, format!("{name} → process")).changed() {
                                let _ = app.run("ink.options", json!({"ink": name, "toProcess": process}));
                                ui.close();
                            }
                        });
                    }
                });
                if ui.button(crate::rtl::widget(ui, crate::i18n::tr(&app.ui.language, "New Color Group"))).clicked() {
                    let _ = app.run("swatch.newColorGroup", json!({}));
                    ui.close();
                }
                if ui.button(crate::rtl::widget(ui, crate::i18n::tr(&app.ui.language, "Load Swatches…"))).clicked() {
                    let _ = app.run("app.loadSwatches", json!({}));
                    ui.close();
                }
                if ui.button(crate::rtl::widget(ui, crate::i18n::tr(&app.ui.language, "Save Swatches for Exchange…"))).clicked() {
                    let _ = app.run("app.saveSwatches", json!({}));
                    ui.close();
                }
            });
        });
    });
}

/// Temp-data key of the swatch last clicked (Delete Swatch removes it).
const PICKED: &str = "swatches_picked";
/// Temp-data key of the swatch renamed in place and its name being typed.
const RENAMING: &str = "swatches_renaming";
/// Temp-data key of a click on the picked swatch's name: (swatch, time). Unless it turns into a
/// double-click (Swatch Options), it starts renaming.
const RENAME_CLICK: &str = "swatches_rename_click";
/// Temp-data key of the document (its uid) the panel last showed: a rename belongs to it.
const RENAME_DOC: &str = "swatches_rename_doc";

/// The Swatches panel's top row, as InDesign's: a Fill/Stroke proxy (click: the other in front),
/// Formatting Affects Container / Text, and the tint of the colour in front. `target` is
/// [`super::color_target`]: fill and stroke swatches and tints of the text or the first object.
fn header(app: &mut DesignApp, ui: &mut egui::Ui, doc: &designcraft_doc::Document, target: Option<&(String, f32, String, f32)>) {
    let t = Tokens::get(ui.ctx());
    let (fill_sw, stroke_sw) = target.map_or(("[None]", "[Black]"), |(f, _, s, _)| (f.as_str(), s.as_str()));
    let text = super::colors_affect_text(app) || app.ui.formatting_affects_text;
    let stroke_front = app.ui.stroke_front;
    ui.add_space(3.0);
    ui.horizontal(|ui| {
        let (r, resp) = ui.allocate_exact_size(vec2(27.0, 27.0), Sense::click());
        let tip = crate::i18n::tr(&app.ui.language, if stroke_front { "Stroke" } else { "Fill" });
        resp.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Button, true, tip));
        let fill_r = egui::Rect::from_min_size(r.min + vec2(1.0, 2.0), vec2(16.0, 16.0));
        let stroke_r = egui::Rect::from_min_size(r.min + vec2(10.0, 10.0), vec2(16.0, 16.0));
        let paint = |stroke: bool| {
            let p = ui.painter();
            if stroke {
                // The stroke: a frame of its colour.
                let (c, g) = crate::widgets::swatch_colors(doc, stroke_sw, 1.0);
                crate::widgets::paint_chip(p, stroke_r, c, g);
                p.rect_filled(stroke_r.shrink(4.0), 0.0, t.panel);
                p.rect_stroke(stroke_r.shrink(4.0), 0.0, egui::Stroke::new(1.0, t.text_dim), egui::StrokeKind::Outside);
                p.rect_stroke(stroke_r, 0.0, egui::Stroke::new(1.0, t.text_dim), egui::StrokeKind::Outside);
            } else {
                let (c, g) = crate::widgets::swatch_colors(doc, fill_sw, 1.0);
                crate::widgets::paint_chip(p, fill_r, c, g);
                p.rect_stroke(fill_r, 0.0, egui::Stroke::new(1.0, t.text_dim), egui::StrokeKind::Outside);
            }
        };
        // The one in front is drawn last.
        paint(!stroke_front);
        paint(stroke_front);
        if resp
            .on_hover_ui(|ui| {
                crate::rtl::label(ui, tip);
            })
            .clicked()
        {
            let _ = app.run("color.toggleFillStroke", json!({}));
        }
        ui.add_space(4.0);
        for (k, (icon, tip)) in
            [("format-container", "Formatting affects container (J)"), ("format-text", "Formatting affects text (J)")].into_iter().enumerate()
        {
            if icons::button(ui, icon, 20.0, text == (k == 1), crate::i18n::tr(&app.ui.language, tip)).clicked() {
                let _ = app.run("app.formattingAffectsText", json!({"on": k == 1}));
            }
        }
        ui.add_space(6.0);
        crate::widgets::caption(ui, crate::i18n::tr(&app.ui.language, "Tint:"));
        // With nothing selected InDesign's field reads 100.
        let tint = target.map_or(Some(100.0), |(_, ft, _, st)| Some(f64::from(if stroke_front { *st } else { *ft }) * 100.0));
        if let Some(v) = crate::widgets::number(ui, "swatches_tint", tint, "", 44.0, 0) {
            let _ = set_tint(app, v);
        }
        crate::widgets::caption(ui, "%");
    });
}

/// The Tint field: `percent` of the colour in front of the proxy, for the text or each selected
/// object, which keep their own swatches.
fn set_tint(app: &mut DesignApp, percent: f64) -> Result<serde_json::Value, String> {
    let v = percent.clamp(0.0, 100.0) / 100.0;
    let stroke = app.ui.stroke_front;
    if super::colors_affect_text(app) {
        app.run("type.char", json!({"attrs": {if stroke { "strokeTint" } else { "fillTint" }: v}}))
    } else {
        app.run(if stroke { "object.stroke" } else { "object.fill" }, json!({"tint": v}))
    }
}

/// A folder button (New Color Group), drawn like the colour groups' folders.
fn folder_button(ui: &mut egui::Ui, tip: &str) -> egui::Response {
    let t = Tokens::get(ui.ctx());
    let (r, resp) = ui.allocate_exact_size(vec2(20.0, 20.0), Sense::click());
    resp.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Button, true, tip));
    if resp.hovered() {
        ui.painter().rect_filled(r, 3.0, t.hover);
    }
    let f = egui::Rect::from_center_size(r.center(), vec2(14.0, 10.0));
    ui.painter().rect_stroke(f, 1.5, egui::Stroke::new(1.2, t.icon), egui::StrokeKind::Inside);
    ui.painter().line_segment([f.left_top() + vec2(1.0, 3.0), f.right_top() + vec2(-1.0, 3.0)], egui::Stroke::new(1.0, t.icon));
    resp.on_hover_ui(|ui| {
        crate::rtl::label(ui, tip);
    })
}

/// The name field of a swatch being renamed in place, over `rect`; false when `sw` isn't. Enter or
/// a click elsewhere renames it (`swatch.options`), Escape leaves it as it was.
fn rename_field(app: &mut DesignApp, ui: &mut egui::Ui, sw: &designcraft_color::swatch::Swatch, rect: egui::Rect) -> bool {
    let key = egui::Id::unique(RENAMING);
    let Some((name, mut text)) = ui.data(|d| d.get_temp::<(String, String)>(key)).filter(|(n, _)| *n == sw.name) else { return false };
    let id = egui::Id::unique(("swatch_rename", &sw.name));
    let edit = egui::TextEdit::singleline(&mut text).id(id).font(egui::FontId::proportional(12.5)).margin(vec2(2.0, 1.0));
    let resp = ui.put(rect, edit);
    if !resp.has_focus() && !resp.lost_focus() {
        // The field opens with the name selected.
        resp.request_focus();
        if let Some(mut state) = egui::TextEdit::load_state(ui.ctx(), id) {
            state
                .cursor
                .set_char_range(Some(egui::text::CCursorRange::two(egui::text::CCursor::new(0), egui::text::CCursor::new(text.chars().count()))));
            state.store(ui.ctx(), id);
        }
    }
    if resp.lost_focus() {
        ui.data_mut(|d| d.remove_temp::<(String, String)>(key));
        let to = text.trim();
        if !ui.input(|i| i.key_pressed(egui::Key::Escape)) && !to.is_empty() && to != name {
            match app.run("swatch.options", json!({"name": name, "to": to})) {
                // Still picked under its new name, unless the click that ended the rename picked
                // another swatch.
                Ok(_) => ui.data_mut(|d| {
                    if d.get_temp::<String>(egui::Id::unique(PICKED)).is_none_or(|p| p == name) {
                        d.insert_temp(egui::Id::unique(PICKED), to.to_string());
                    }
                }),
                Err(e) => {
                    app.status_error("Swatch Options", e);
                }
            }
        }
    } else {
        ui.data_mut(|d| d.insert_temp(key, (name, text)));
    }
    true
}

/// The swatch's chip following the pointer while the row is dragged.
fn paint_drag_chip(ui: &egui::Ui, doc: &designcraft_doc::Document, name: &str, p: egui::Pos2) {
    let t = Tokens::get(ui.ctx());
    let (c, g) = crate::widgets::swatch_colors(doc, name, 1.0);
    let painter = ui.ctx().layer_painter(egui::LayerId::new(egui::Order::Tooltip, egui::Id::unique("swatch_drag")));
    let chip = egui::Rect::from_min_size(p + vec2(8.0, 8.0), vec2(16.0, 16.0));
    crate::widgets::paint_chip(&painter, chip, c, g);
    painter.rect_stroke(chip, 0.0, egui::Stroke::new(1.0, t.field_border), egui::StrokeKind::Outside);
}

/// What a Swatches panel row marks on its right, as InDesign's columns.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct RowMarks {
    /// [None], [Registration] and [Black] can't be edited ([Paper] can).
    not_editable: bool,
    kind: Option<ColorType>,
    mode: Option<RowMode>,
}

/// The colour-mode column: the ink model, or the special swatches' own glyphs.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum RowMode {
    Cmyk,
    Rgb,
    Gray,
    None,
    Registration,
}

fn row_marks(sw: &designcraft_color::swatch::Swatch) -> RowMarks {
    use designcraft_color::Color;
    // Paper's colour can be edited, whatever the swatch is called (as the options dialog allows).
    let not_editable = sw.locked && !matches!(sw.value, SwatchValue::Paper { .. });
    match &sw.value {
        SwatchValue::Color { color, color_type } => {
            let mode = match color {
                Color::Cmyk { .. } => RowMode::Cmyk,
                Color::Rgb { .. } => RowMode::Rgb,
                Color::Gray { .. } => RowMode::Gray,
            };
            RowMarks { not_editable, kind: Some(*color_type), mode: Some(mode) }
        }
        SwatchValue::None => RowMarks { not_editable, kind: None, mode: Some(RowMode::None) },
        SwatchValue::Registration => RowMarks { not_editable, kind: None, mode: Some(RowMode::Registration) },
        _ => RowMarks { not_editable, kind: None, mode: None },
    }
}

/// A row's accessible name: the swatch's name, then what its marks show (colour type, colour
/// mode, that it can't be edited). [None] and [Registration] say their mode by name.
fn row_label(lang: &str, name: &str, marks: RowMarks) -> String {
    let tr = |s: &str| crate::i18n::tr(lang, s).to_string();
    let mut parts = vec![name.to_string()];
    match marks.kind {
        Some(ColorType::Process) => parts.push(tr("Process")),
        Some(ColorType::Spot) => parts.push(tr("Spot")),
        None => {}
    }
    match marks.mode {
        Some(RowMode::Cmyk) => parts.push(tr("CMYK")),
        Some(RowMode::Rgb) => parts.push(tr("RGB")),
        Some(RowMode::Gray) => parts.push(tr("Gray")),
        Some(RowMode::None | RowMode::Registration) | None => {}
    }
    if marks.not_editable {
        parts.push(tr("Cannot be edited"));
    }
    parts.join(", ")
}

/// Whether accessible name `label` is swatch `name`'s row (its name, then its marks).
#[cfg(test)]
pub(crate) fn names_swatch(label: &str, name: &str) -> bool {
    label.strip_prefix(name).is_some_and(|rest| rest.is_empty() || rest.starts_with(", "))
}

/// A pencil with a stroke through it: the swatch can't be edited. Drawn in code.
fn paint_not_editable(p: &egui::Painter, r: egui::Rect, ink: egui::Color32) {
    // The pencil: its tip at the lower left, its body up to the right.
    let tip = r.left_bottom() + vec2(1.5, -1.5);
    let d = vec2(1.0, -1.0).normalized();
    let n = vec2(d.y, -d.x) * 1.9;
    let (b0, b1) = (tip + d * 3.4, tip + d * 12.5);
    p.add(egui::Shape::convex_polygon(vec![tip, b0 + n, b0 - n], ink, egui::Stroke::NONE));
    p.add(egui::Shape::convex_polygon(vec![b0 + n, b1 + n, b1 - n, b0 - n], ink, egui::Stroke::NONE));
    // The stroke through it, with a gap of the panel's colour around it so it reads on the body.
    let (s0, s1) = (r.left_top() + vec2(1.5, 1.5), r.right_bottom() - vec2(1.5, 1.5));
    p.line_segment([s0, s1], egui::Stroke::new(3.2, egui::Color32::from_black_alpha(140)));
    p.line_segment([s0, s1], egui::Stroke::new(1.2, ink));
}

/// The colour type: a process colour is a halftone square, a spot colour a disc in a square.
fn paint_color_type(p: &egui::Painter, r: egui::Rect, kind: ColorType, ink: egui::Color32) {
    let sq = egui::Rect::from_center_size(r.center(), vec2(10.0, 10.0));
    p.rect_filled(sq, 0.0, ink);
    if kind == ColorType::Spot {
        p.circle_filled(sq.center(), 3.2, egui::Color32::from_black_alpha(110));
    } else {
        // A fine halftone: a grey square with a light dot screen.
        let dot = egui::Color32::from_black_alpha(70);
        for i in 0..4u8 {
            for j in 0..4u8 {
                let o = if j % 2 == 0 { 1.5 } else { 2.6 };
                p.circle_filled(sq.min + vec2(o + 2.2 * f32::from(i), 1.6 + 2.3 * f32::from(j)), 0.6, dot);
            }
        }
    }
}

/// The colour mode: CMYK's four inks meeting in the middle, RGB's three primaries, a grey ramp,
/// [None]'s red slash and [Registration]'s target.
fn paint_mode(p: &egui::Painter, r: egui::Rect, mode: RowMode, ink: egui::Color32) {
    let sq = egui::Rect::from_center_size(r.center(), vec2(11.0, 11.0));
    let c = sq.center();
    let rgb = egui::Color32::from_rgb;
    match mode {
        RowMode::Cmyk => {
            let corners = [sq.left_top(), sq.right_top(), sq.right_bottom(), sq.left_bottom()];
            // Top, right, bottom, left: magenta, yellow, black, cyan.
            let inks = [rgb(236, 0, 140), rgb(255, 230, 0), rgb(30, 30, 30), rgb(0, 174, 239)];
            for (k, colour) in inks.into_iter().enumerate() {
                let (a, b) = (corners[k], corners[(k + 1) % 4]);
                p.add(egui::Shape::convex_polygon(vec![a, b, c], colour, egui::Stroke::NONE));
            }
            p.rect_stroke(sq, 0.0, egui::Stroke::new(1.0, ink), egui::StrokeKind::Outside);
        }
        RowMode::Rgb => {
            let w = sq.width() / 3.0;
            for (k, colour) in [rgb(230, 40, 40), rgb(40, 190, 60), rgb(40, 90, 230)].into_iter().enumerate() {
                let x = sq.min.x + w * k as f32;
                p.rect_filled(egui::Rect::from_min_max(egui::pos2(x, sq.min.y), egui::pos2(x + w, sq.max.y)), 0.0, colour);
            }
            p.rect_stroke(sq, 0.0, egui::Stroke::new(1.0, ink), egui::StrokeKind::Outside);
        }
        RowMode::Gray => {
            let w = sq.width() / 4.0;
            for k in 0..4u8 {
                let x = sq.min.x + w * f32::from(k);
                p.rect_filled(
                    egui::Rect::from_min_max(egui::pos2(x, sq.min.y), egui::pos2(x + w, sq.max.y)),
                    0.0,
                    egui::Color32::from_gray(255 - 70 * k),
                );
            }
            p.rect_stroke(sq, 0.0, egui::Stroke::new(1.0, ink), egui::StrokeKind::Outside);
        }
        RowMode::None => {
            p.rect_filled(sq, 0.0, egui::Color32::WHITE);
            p.line_segment([sq.left_bottom(), sq.right_top()], egui::Stroke::new(1.5, rgb(220, 30, 30)));
            p.rect_stroke(sq, 0.0, egui::Stroke::new(1.0, ink), egui::StrokeKind::Outside);
        }
        RowMode::Registration => {
            p.rect_stroke(sq, 0.0, egui::Stroke::new(1.0, ink), egui::StrokeKind::Inside);
            p.circle_stroke(c, 3.0, egui::Stroke::new(1.0, ink));
            p.line_segment([c - vec2(4.5, 0.0), c + vec2(4.5, 0.0)], egui::Stroke::new(1.0, ink));
            p.line_segment([c - vec2(0.0, 4.5), c + vec2(0.0, 4.5)], egui::Stroke::new(1.0, ink));
        }
    }
}

/// One Swatches panel row: chip, name, type; click applies it to the fill or stroke in front of
/// the proxy (of the frames or their text, see [`super::colors_affect_text`]), the context menu
/// to the stroke, colour groups and delete.
fn swatch_row(
    app: &mut DesignApp,
    ui: &mut egui::Ui,
    doc: &designcraft_doc::Document,
    sw: &designcraft_color::swatch::Swatch,
    indent: f32,
    fill_cur: Option<&str>,
) {
    let t = Tokens::get(ui.ctx());
    // Rows can be dragged onto the Gradient panel's ramp.
    let (row, resp) = ui.allocate_exact_size(vec2(ui.available_width(), 22.0), Sense::click_and_drag());
    // What the row's marks show, for screen readers too: "Cyan, Process, CMYK".
    let marks = row_marks(sw);
    let spoken = row_label(&app.ui.language, &sw.name, marks);
    resp.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Button, true, &spoken));
    resp.dnd_set_drag_payload(SwatchDrag(sw.name.clone()));
    if resp.dragged()
        && let Some(p) = ui.ctx().pointer_interact_pos()
    {
        paint_drag_chip(ui, doc, &sw.name, p);
    }
    if fill_cur == Some(sw.name.as_str()) {
        ui.painter().rect_filled(row, 0.0, t.row_selected);
    } else if resp.hovered() {
        ui.painter().rect_filled(row, 0.0, t.hover);
    }
    let (c, g) = crate::widgets::swatch_colors(doc, &sw.name, 1.0);
    let chip = egui::Rect::from_min_size(row.min + vec2(4.0 + indent, 3.0), vec2(16.0, 16.0));
    crate::widgets::paint_chip(ui.painter(), chip, c, g);
    let name_rect = egui::Rect::from_min_max(row.min + vec2(26.0 + indent, 1.0), egui::pos2(row.max.x - 72.0, row.max.y - 1.0));
    if !rename_field(app, ui, sw, name_rect) {
        ui.painter().text(row.min + vec2(28.0 + indent, 11.0), egui::Align2::LEFT_CENTER, &sw.name, egui::FontId::proportional(12.5), t.text);
    }
    // InDesign's columns on the right: not editable, colour type, colour mode.
    let col = |k: f32| egui::Rect::from_min_size(egui::pos2(row.max.x - 19.0 - 18.0 * k, row.min.y + 4.5), vec2(13.0, 13.0));
    if marks.not_editable {
        paint_not_editable(ui.painter(), col(2.0), t.text_dim);
    }
    if let Some(kind) = marks.kind {
        paint_color_type(ui.painter(), col(1.0), kind, t.text_dim);
    }
    if let Some(mode) = marks.mode {
        paint_mode(ui.painter(), col(0.0), mode, t.text_dim);
    }
    let label = match &sw.value {
        SwatchValue::Gradient { .. } => crate::i18n::tr(&app.ui.language, "Gradient").to_string(),
        SwatchValue::Tint { tint, .. } => format!("{:.0}%", tint * 100.0),
        _ => String::new(),
    };
    if !label.is_empty() {
        ui.painter().text(row.right_center() - vec2(6.0, 0.0), egui::Align2::RIGHT_CENTER, label, egui::FontId::proportional(10.5), t.text_dim);
    }
    if resp.clicked() && !resp.double_clicked() {
        // A click on the name of the swatch picked already renames it in place, as InDesign's
        // (a slow double-click), unless it turns into a double-click.
        let picked = ui.data(|d| d.get_temp::<String>(egui::Id::unique(PICKED))).is_some_and(|p| p == sw.name);
        // A click on another swatch cancels a rename still waiting.
        ui.data_mut(|d| d.remove_temp::<(String, f64)>(egui::Id::unique(RENAME_CLICK)));
        if picked && !sw.locked && resp.interact_pointer_pos().is_some_and(|p| name_rect.contains(p)) {
            let now = ui.input(|i| i.time);
            ui.data_mut(|d| d.insert_temp(egui::Id::unique(RENAME_CLICK), (sw.name.clone(), now)));
        }
        ui.data_mut(|d| d.insert_temp(egui::Id::unique(PICKED), sw.name.clone()));
        let _ = super::apply_swatch(app, app.ui.stroke_front, &sw.name, None);
    }
    // Double-click: Swatch Options (the first click has applied the swatch).
    if resp.double_clicked() {
        ui.data_mut(|d| d.remove_temp::<(String, f64)>(egui::Id::unique(RENAME_CLICK)));
        let _ = app.run("app.swatchOptionsDialog", json!({"name": sw.name}));
    }
    if let Some((name, at)) = ui.data(|d| d.get_temp::<(String, f64)>(egui::Id::unique(RENAME_CLICK))).filter(|(n, _)| *n == sw.name) {
        let wait = ui.ctx().options(|o| o.input_options.max_double_click_delay);
        if ui.data(|d| d.get_temp::<String>(egui::Id::unique(PICKED))).is_none_or(|p| p != name) {
            // Another swatch picked since: no rename.
            ui.data_mut(|d| d.remove_temp::<(String, f64)>(egui::Id::unique(RENAME_CLICK)));
        } else if ui.input(|i| i.time) - at > wait {
            ui.data_mut(|d| {
                d.remove_temp::<(String, f64)>(egui::Id::unique(RENAME_CLICK));
                d.insert_temp(egui::Id::unique(RENAMING), (name.clone(), name));
            });
        } else {
            ui.ctx().request_repaint_after(std::time::Duration::from_secs_f64(wait));
        }
    }
    resp.context_menu(|ui| {
        if ui.button(crate::rtl::widget(ui, crate::i18n::tr(&app.ui.language, "Swatch Options…"))).clicked() {
            let _ = app.run("app.swatchOptionsDialog", json!({"name": sw.name}));
            ui.close();
        }
        if ui.button(crate::rtl::widget(ui, crate::i18n::tr(&app.ui.language, "Apply to Stroke"))).clicked() {
            let _ = super::apply_swatch(app, true, &sw.name, None);
            ui.close();
        }
        if !sw.locked {
            crate::menus::menu_button(ui, crate::rtl::widget(ui, crate::i18n::tr(&app.ui.language, "Move to Color Group")), |ui| {
                if ui.button(crate::rtl::widget(ui, crate::i18n::tr(&app.ui.language, "(Top Level)"))).clicked() {
                    let _ = app.run("swatch.moveToGroup", json!({"swatches": [sw.name], "group": null}));
                    ui.close();
                }
                for g in &doc.color_groups {
                    if ui.button(&g.name).clicked() {
                        let _ = app.run("swatch.moveToGroup", json!({"swatches": [sw.name], "group": g.name}));
                        ui.close();
                    }
                }
            });
            if ui.button(crate::rtl::widget(ui, crate::i18n::tr(&app.ui.language, "New Color Group with Swatch"))).clicked() {
                let _ = app.run("swatch.newColorGroup", json!({"swatches": [sw.name]}));
                ui.close();
            }
            ui.separator();
            if ui.button(crate::rtl::widget(ui, crate::i18n::tr(&app.ui.language, "Delete Swatch"))).clicked() {
                let _ = app.run("swatch.delete", json!({"name": sw.name}));
                ui.close();
            }
        }
    });
}

/// Colour models of the Color panel (its panel menu: Lab, CMYK, RGB).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ColorMode {
    Cmyk,
    Rgb,
    Lab,
}

/// Color panel: fill/stroke proxy, sliders in the chosen model (or a tint slider for a named
/// swatch), the spectrum ramp, Add to Swatches. Edits apply unnamed colours (`object.color`).
pub fn color_panel(app: &mut DesignApp, ui: &mut egui::Ui) {
    let t = Tokens::get(ui.ctx());
    let target = super::color_target_or_defaults(app);
    let Some((fill, fill_tint, stroke_sw, stroke_tint)) = target else {
        ui.label(crate::rtl::widget(
            ui,
            egui::RichText::new(crate::i18n::tr(&app.ui.language, "Select an object or text to color its fill or stroke.")).color(t.text_dim),
        ));
        return;
    };
    let text = super::colors_affect_text(app);
    let Some(doc) = app.session.active().map(|d| d.doc.clone()) else { return };
    let mode_id = egui::Id::unique("color_panel_mode");
    let mut stroke = app.ui.stroke_front;
    let default_mode = if doc.settings.intent == designcraft_doc::Intent::Print { ColorMode::Cmyk } else { ColorMode::Rgb };
    let mut mode: ColorMode = ui.data(|d| d.get_temp(mode_id)).unwrap_or(default_mode);
    let target = if stroke { "stroke" } else { "fill" };
    let name = if stroke { stroke_sw.clone() } else { fill.clone() };
    // Proxy: fill (front) and stroke chips; click to choose, double-click for the Color Picker.
    ui.horizontal(|ui| {
        for (k, label) in [(false, "Fill"), (true, "Stroke")] {
            let n = if k { &stroke_sw } else { &fill };
            let (c, g) = crate::widgets::swatch_colors(&doc, n, 1.0);
            let (r, resp) = ui.allocate_exact_size(vec2(22.0, 22.0), Sense::click());
            crate::widgets::paint_chip(ui.painter(), r.shrink(2.0), c, g);
            if k == stroke {
                ui.painter().rect_stroke(r, 2.0, egui::Stroke::new(1.5, t.accent), egui::StrokeKind::Inside);
            }
            let resp = resp.on_hover_text(
                crate::i18n::tr(&app.ui.language, "{label}: {name} (double-click: Color Picker)")
                    .replace("{label}", crate::i18n::tr(&app.ui.language, label))
                    .replace("{name}", n),
            );
            if resp.clicked() {
                stroke = k;
            }
            if resp.double_clicked() {
                let mut fields = picker_fields(doc.resolve_color(n, 1.0), k);
                fields["text"] = json!(text);
                app.ui.dialog = Some(crate::dialogs::Dialog::new("colorPicker", fields));
            }
        }
        ui.label(egui::RichText::new(&name).size(11.0).color(t.text_dim));
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            let combo = craft_ui::ComboBox::from_id_salt("color_mode")
                .width(64.0)
                .selected_text(match mode {
                    ColorMode::Cmyk => "CMYK",
                    ColorMode::Rgb => "RGB",
                    ColorMode::Lab => "Lab",
                })
                .show_ui(ui, |ui| {
                    ui.selectable_value(&mut mode, ColorMode::Lab, "Lab");
                    ui.selectable_value(&mut mode, ColorMode::Cmyk, "CMYK");
                    ui.selectable_value(&mut mode, ColorMode::Rgb, "RGB");
                });
            let _ = crate::widgets::name_combo(combo, crate::i18n::tr(&app.ui.language, "Color Mode"));
        });
    });
    app.ui.stroke_front = stroke;
    ui.data_mut(|d| d.insert_temp(mode_id, mode));
    // A gradient: the stop selected in the Gradient panel, as InDesign's Color panel shows it.
    if !text && let Some(g) = designcraft_color::swatch::resolve_gradient(&doc.swatches, &name).cloned() {
        gradient_stop(app, ui, &g, target, mode);
        return;
    }
    let sw = doc.swatch(&name).cloned();
    let Some(c) = doc.resolve_color(&name, 1.0) else {
        ui.label(crate::rtl::widget(
            ui,
            egui::RichText::new(crate::i18n::tr(&app.ui.language, "[None] — choose a colour on the ramp.")).color(t.text_dim),
        ));
        spectrum(app, ui, target);
        return;
    };
    // A named swatch shows its tint; mixing its values makes an unnamed colour.
    if let Some(sw) = sw.as_ref().filter(|w| !w.hidden && !w.locked) {
        let cur = if stroke { stroke_tint } else { fill_tint };
        let mut tint = cur * 100.0;
        let mut commit = None;
        ui.horizontal(|ui| {
            ui.label("T");
            // The slider applies on release. The field keeps typed text while it has focus (the
            // tint is read back from the document every frame) and applies it, clamped, on Enter
            // or when it loses focus.
            ui.spacing_mut().slider_width = (ui.available_width() - 60.0).max(60.0);
            // The painted "T" caption is not a text node next to the slider, so name it here.
            let slider = ui
                .add(egui::Slider::new(&mut tint, 0.0..=100.0).integer().show_value(false))
                .accessible_name(crate::i18n::tr(&app.ui.language, "Tint"));
            if slider.drag_stopped() {
                commit = Some(tint);
            }
            if let Some(v) =
                crate::widgets::NumField::number("color_tint", Some(f64::from(cur) * 100.0), "%", 0).width(44.0).range(0.0, 100.0).show(ui)
            {
                commit = Some(v as f32);
            }
        });
        if let Some(v) = commit {
            let _ = super::apply_swatch(app, stroke, &sw.name, Some(v / 100.0));
        }
    }
    let mut send: Option<serde_json::Value> = None;
    match mode {
        ColorMode::Cmyk => {
            let mut v = c.to_cmyk();
            let mut done = false;
            for (k, label) in ["C", "M", "Y", "K"].iter().enumerate() {
                done |= channel(ui, label, &mut v[k], 100.0, "%", |x| cmyk_ramp(k, x));
            }
            if done {
                send = Some(json!({"model": "cmyk", "c": v[0], "m": v[1], "y": v[2], "k": v[3]}));
            }
        }
        ColorMode::Rgb => {
            let mut v = c.to_rgb();
            let mut done = false;
            for (k, label) in ["R", "G", "B"].iter().enumerate() {
                done |= channel(ui, label, &mut v[k], 255.0, "", |x| {
                    let mut p = [0.0; 3];
                    p[k] = x;
                    egui::Color32::from_rgb((p[0] * 255.0) as u8, (p[1] * 255.0) as u8, (p[2] * 255.0) as u8)
                });
            }
            if done {
                send = Some(json!({"model": "rgb", "r": v[0], "g": v[1], "b": v[2]}));
            }
        }
        ColorMode::Lab => {
            let lab = c.to_lab();
            ui.label(egui::RichText::new(format!("L {:.0}   a {:.0}   b {:.0}", lab.l, lab.a, lab.b)).color(t.text_dim));
        }
    }
    if let Some(color) = send {
        let _ = app.run("object.color", json!({"color": color, "target": target, "text": text}));
    }
    spectrum(app, ui, target);
    ui.horizontal(|ui| {
        let unnamed = sw.as_ref().is_some_and(|w| w.hidden);
        if ui.add_enabled(unnamed, egui::Button::new(crate::rtl::widget(ui, crate::i18n::tr(&app.ui.language, "Add to Swatches")))).clicked() {
            let _ = app.run("swatch.addToSwatches", json!({"swatch": name, "target": target}));
        }
    });
}

/// The Color Picker's fields for `color` (`None`: black): its values in its own model, so OK
/// keeps a CMYK colour CMYK.
pub(crate) fn picker_fields(color: Option<designcraft_color::Color>, stroke: bool) -> serde_json::Value {
    let color = color.unwrap_or(designcraft_color::Color::BLACK);
    let mut f = json!({"target": if stroke { "stroke" } else { "fill" }, "hex": color.to_hex()});
    match color {
        designcraft_color::Color::Cmyk { c, m, y, k } => {
            f["model"] = json!("cmyk");
            f["cmyk"] = json!([c, m, y, k].map(|v| f64::from(v) * 100.0));
        }
        designcraft_color::Color::Rgb { r, g, b } => {
            f["model"] = json!("rgb");
            f["rgb"] = json!([r, g, b].map(|v| f64::from(v) * 255.0));
        }
        designcraft_color::Color::Gray { .. } => f["model"] = json!("rgb"),
    }
    f
}

/// One slider with a gradient track of the channel and a value field. True when it was changed
/// (on release, or when a typed value is committed by Enter or leaving the field).
fn channel(ui: &mut egui::Ui, label: &str, v: &mut f32, scale: f32, suffix: &str, ramp: impl Fn(f32) -> egui::Color32) -> bool {
    let mut changed = false;
    ui.horizontal(|ui| {
        ui.label(egui::RichText::new(label).monospace());
        let w = (ui.available_width() - 60.0).max(60.0);
        let (track, resp) = ui.allocate_exact_size(vec2(w, 14.0), Sense::click_and_drag());
        // Gradient track.
        let n = 24;
        for i in 0..n {
            let x0 = track.min.x + track.width() * i as f32 / n as f32;
            let x1 = track.min.x + track.width() * (i + 1) as f32 / n as f32;
            let r = egui::Rect::from_min_max(egui::pos2(x0, track.min.y + 2.0), egui::pos2(x1 + 0.5, track.max.y - 2.0));
            ui.painter().rect_filled(r, 0.0, ramp((i as f32 + 0.5) / n as f32));
        }
        let x = track.min.x + track.width() * v.clamp(0.0, 1.0);
        ui.painter().add(egui::Shape::convex_polygon(
            vec![egui::pos2(x, track.max.y - 3.0), egui::pos2(x - 4.0, track.max.y + 3.0), egui::pos2(x + 4.0, track.max.y + 3.0)],
            ui.visuals().text_color(),
            egui::Stroke::NONE,
        ));
        if let Some(p) = resp.interact_pointer_pos() {
            *v = ((p.x - track.min.x) / track.width()).clamp(0.0, 1.0);
        }
        changed |= resp.drag_stopped() || resp.clicked();
        // The value is applied when typing ends or a drag stops; until then the field's number
        // lives here, because `v` is read back from the document every frame.
        let pending_id = ui.scope_id().with(("channel", label));
        let mut num = ui.data(|d| d.get_temp::<f32>(pending_id)).unwrap_or((*v * scale).round());
        // The channel name is a painted label beside a bare spinner: name the spinner too.
        let field =
            ui.add(egui::DragValue::new(&mut num).range(0.0..=scale).suffix(suffix).update_while_editing(false)).accessible_name(label.to_owned());
        if field.dragged() {
            ui.data_mut(|d| d.insert_temp(pending_id, num));
        } else {
            ui.data_mut(|d| d.remove::<f32>(pending_id));
            if field.changed() || field.drag_stopped() {
                *v = num / scale;
                changed = true;
            }
        }
    });
    changed
}

fn cmyk_ramp(k: usize, x: f32) -> egui::Color32 {
    let mut v = [0.0f32; 4];
    v[k] = x;
    let [r, g, b] = designcraft_color::Color::cmyk(v[0], v[1], v[2], v[3]).to_rgb();
    egui::Color32::from_rgb((r * 255.0) as u8, (g * 255.0) as u8, (b * 255.0) as u8)
}

/// The Color panel on a gradient fill (or stroke): the stop selected in the Gradient panel (the
/// stop before a selected midpoint), in the panel's colour mode, as InDesign's. Its fields and the
/// spectrum colour that stop.
fn gradient_stop(app: &mut DesignApp, ui: &mut egui::Ui, g: &designcraft_color::Gradient, target: &str, mode: ColorMode) {
    use designcraft_color::Color;
    let t = Tokens::get(ui.ctx());
    let sel: RampSel = ui.data(|d| d.get_temp(egui::Id::unique(GRAD_SEL))).unwrap_or(RampSel::Stop(0));
    let i = match sel {
        RampSel::Stop(i) | RampSel::Mid(i) => i,
    }
    .min(g.stops.len().saturating_sub(1));
    let Some(stop) = g.stops.get(i) else { return };
    let c = stop.color;
    let pct = |v: f32| (v * 100.0).round();
    let values = match c {
        Color::Cmyk { c, m, y, k } => format!("C={} M={} Y={} K={}", pct(c), pct(m), pct(y), pct(k)),
        Color::Gray { k } => format!("K={}", pct(k)),
        Color::Rgb { .. } => {
            let [r, g, b, _] = c.to_rgba8(1.0);
            format!("R={r} G={g} B={b}")
        }
    };
    ui.label(egui::RichText::new(values).size(11.0).color(t.text_dim));
    let mut new = None;
    match mode {
        ColorMode::Cmyk => {
            let mut v = c.to_cmyk();
            let mut done = false;
            for (k, label) in ["C", "M", "Y", "K"].iter().enumerate() {
                done |= channel(ui, label, &mut v[k], 100.0, "%", |x| cmyk_ramp(k, x));
            }
            if done {
                new = Some(Color::cmyk(v[0], v[1], v[2], v[3]));
            }
        }
        ColorMode::Rgb => {
            let mut v = c.to_rgb();
            let mut done = false;
            for (k, label) in ["R", "G", "B"].iter().enumerate() {
                done |= channel(ui, label, &mut v[k], 255.0, "", |x| {
                    let mut p = [0.0; 3];
                    p[k] = x;
                    egui::Color32::from_rgb((p[0] * 255.0) as u8, (p[1] * 255.0) as u8, (p[2] * 255.0) as u8)
                });
            }
            if done {
                new = Some(Color::rgb(v[0], v[1], v[2]));
            }
        }
        ColorMode::Lab => {
            let lab = c.to_lab();
            ui.label(egui::RichText::new(format!("L {:.0}   a {:.0}   b {:.0}", lab.l, lab.a, lab.b)).color(t.text_dim));
        }
    }
    if let Some([r, gr, b]) = spectrum_pick(ui, &app.ui.language) {
        new = Some(Color::rgb(r, gr, b));
    }
    if let Some(c) = new {
        let mut stops = stops_json(g);
        set_stop_color(&mut stops, i, color_json(&c));
        let _ = app.run("object.gradient", json!({"stops": stops, "target": target}));
    }
}

/// The spectrum ramp: hue across, light to dark down; click to apply.
fn spectrum(app: &mut DesignApp, ui: &mut egui::Ui, target: &str) {
    if let Some([r, g, b]) = spectrum_pick(ui, &app.ui.language) {
        let text = super::colors_affect_text(app);
        let _ = app.run("object.color", json!({"color": {"model": "rgb", "r": r, "g": g, "b": b}, "target": target, "text": text}));
    }
}

/// The spectrum ramp; a click on it gives that colour (RGB, 0–1, in 1/255 steps).
fn spectrum_pick(ui: &mut egui::Ui, language: &str) -> Option<[f32; 3]> {
    let (r, resp) = ui.allocate_exact_size(vec2(ui.available_width(), 36.0), Sense::click());
    resp.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Button, true, crate::i18n::tr(language, "Color spectrum")));
    let at = |u: f32, v: f32| -> [f32; 3] {
        let c = egui::ecolor::Hsva::new(u, (v * 2.0).min(1.0), (2.0 - v * 2.0).min(1.0), 1.0).to_rgb();
        [c[0], c[1], c[2]]
    };
    let (nx, ny) = (48, 8);
    for i in 0..nx {
        for j in 0..ny {
            let c = at((i as f32 + 0.5) / nx as f32, (j as f32 + 0.5) / ny as f32);
            let cell = egui::Rect::from_min_max(
                egui::pos2(r.min.x + r.width() * i as f32 / nx as f32, r.min.y + r.height() * j as f32 / ny as f32),
                egui::pos2(r.min.x + r.width() * (i + 1) as f32 / nx as f32 + 0.5, r.min.y + r.height() * (j + 1) as f32 / ny as f32 + 0.5),
            );
            ui.painter().rect_filled(cell, 0.0, egui::Color32::from_rgb((c[0] * 255.0) as u8, (c[1] * 255.0) as u8, (c[2] * 255.0) as u8));
        }
    }
    let p = resp.interact_pointer_pos().filter(|_| resp.clicked())?;
    let c = at(((p.x - r.min.x) / r.width()).clamp(0.0, 1.0), ((p.y - r.min.y) / r.height()).clamp(0.0, 1.0));
    Some(c.map(|v| (v * 255.0).round() / 255.0))
}

/// A swatch dragged out of the Swatches panel (the Gradient panel's ramp takes it as a stop).
#[derive(Clone, Debug)]
pub struct SwatchDrag(pub String);

/// The selection's gradient dragged from the Gradient panel's thumbnail (its swatch name): the
/// Swatches panel takes it as a new gradient swatch.
#[derive(Clone, Debug)]
pub struct GradientDrag(pub String);

/// A colour in `object.gradient` form, keeping its model (CMYK stays CMYK).
fn color_json(c: &designcraft_color::Color) -> serde_json::Value {
    use designcraft_color::Color;
    match *c {
        // The model form takes fractions; bare CMYK would be read as 0..100.
        Color::Cmyk { c, m, y, k } => json!({"model": "cmyk", "c": c, "m": m, "y": y, "k": k}),
        Color::Gray { k } => json!({"model": "gray", "k": k}),
        Color::Rgb { .. } => json!(c.to_hex()),
    }
}

#[cfg(test)]
mod stop_color_tests {
    use super::*;

    #[test]
    fn a_new_stop_colour_drops_the_stops_swatch() {
        let mut stops = vec![json!({"location": 0, "color": "#000000", "swatch": "Gold"})];
        set_stop_color(&mut stops, 0, json!("#ff0000"));
        assert_eq!(stops[0]["color"], "#ff0000");
        assert!(stops[0].get("swatch").is_none());
    }
}

#[cfg(test)]
mod color_panel_defaults_tests {
    use super::*;
    use egui_kittest::kittest::Queryable;

    #[test]
    fn color_panel_with_no_selection_edits_future_shapes_without_recoloring_existing_ones() {
        let mut app = DesignApp::new(designcraft_engine::Session::new(), crate::Services::default());
        app.run("file.new", json!({})).unwrap();
        let old =
            designcraft_doc::ItemId(app.run("frame.create", json!({"rect":[72,72,200,200],"content":"unassigned"})).unwrap()["id"].as_u64().unwrap());
        let original = app.session.active().unwrap().doc.item(old).unwrap().stroke.clone();
        app.run("selection.set", json!({"ids":[]})).unwrap();
        app.ui.stroke_front = true;
        app.run("window.panel", json!({"panel":"color"})).unwrap();
        let mut h = crate::test_window::open(app, vec2(1440.0, 900.0));
        assert!(h.query_by_label("Select an object or text to color its fill or stroke.").is_none());
        let ramp = h.get_by_label("Color spectrum").rect();
        crate::test_window::click_at(&mut h, egui::pos2(ramp.left() + ramp.width() / 3.0, ramp.center().y));
        let defaults = h.state().app.session.active().unwrap().doc.settings.new_object.clone();
        assert_ne!(defaults.stroke, original.swatch);
        assert_eq!(h.state().app.session.active().unwrap().doc.item(old).unwrap().stroke, original);
        h.state_mut().app.run("selection.set", json!({"ids":[old.0]})).unwrap();
        assert_eq!(crate::panels::color_target(&mut h.state_mut().app).unwrap().2, original.swatch);
        h.state_mut().app.run("selection.set", json!({"ids":[]})).unwrap();
        let new = designcraft_doc::ItemId(
            h.state_mut().app.run("frame.create", json!({"rect":[240,72,368,200],"content":"unassigned"})).unwrap()["id"].as_u64().unwrap(),
        );
        assert_eq!(h.state().app.session.active().unwrap().doc.item(new).unwrap().stroke.swatch, defaults.stroke);
        // Two undo steps: the new frame, then the spectrum click's default stroke.
        h.state_mut().app.run("edit.undo", json!({})).unwrap();
        h.state_mut().app.run("edit.undo", json!({})).unwrap();
        assert_eq!(h.state().app.session.active().unwrap().doc.settings.new_object.stroke, original.swatch);
        assert_eq!(h.state().app.session.active().unwrap().doc.item(old).unwrap().stroke, original);
        // With the Fill box in front, the ramp sets the default fill instead.
        let original_fill = h.state().app.session.active().unwrap().doc.item(old).unwrap().fill.clone();
        h.state_mut().app.ui.stroke_front = false;
        h.run_steps(2);
        let ramp = h.get_by_label("Color spectrum").rect();
        crate::test_window::click_at(&mut h, egui::pos2(ramp.left() + ramp.width() * 2.0 / 3.0, ramp.center().y));
        let defaults = h.state().app.session.active().unwrap().doc.settings.new_object.clone();
        assert_ne!(defaults.fill, original_fill.swatch);
        assert_eq!(defaults.stroke, original.swatch, "the stroke default is left alone");
        assert_eq!(h.state().app.session.active().unwrap().doc.item(old).unwrap().fill, original_fill);
    }

    /// The Swatches panel shows the same defaults: its Tint field reads the default fill's tint,
    /// and a click on a swatch row sets the default fill, not an existing frame's.
    #[test]
    fn swatches_panel_with_no_selection_shows_and_edits_future_shape_colors() {
        use egui::accesskit::Role;
        let mut app = DesignApp::new(designcraft_engine::Session::new(), crate::Services::default());
        app.run("file.new", json!({})).unwrap();
        let old =
            designcraft_doc::ItemId(app.run("frame.create", json!({"rect":[72,72,200,200],"content":"unassigned"})).unwrap()["id"].as_u64().unwrap());
        let original = app.session.active().unwrap().doc.item(old).unwrap().fill.clone();
        app.run("selection.set", json!({"ids":[]})).unwrap();
        app.run("object.fill", json!({"swatch": "[Black]", "tint": 0.4})).unwrap();
        app.ui.stroke_front = false;
        let mut h = egui_kittest::Harness::builder().with_size(vec2(320.0, 480.0)).build_ui_state(
            |ui, app: &mut DesignApp| {
                // The panel's fonts are installed on the first frame and used from the next.
                let id = egui::Id::unique("test_fonts");
                if ui.data(|d| d.get_temp::<bool>(id)).is_none() {
                    crate::theme::install_fonts(ui.ctx(), "");
                    ui.data_mut(|d| d.insert_temp(id, true));
                    return;
                }
                show(app, ui);
            },
            app,
        );
        h.run_steps(3);
        let fields = h.query_all_by(|n| matches!(n.role(), Role::SpinButton | Role::TextInput)).filter_map(|n| n.value()).collect::<Vec<_>>();
        assert!(fields.iter().any(|v| v.starts_with("40")), "the Tint field shows the default fill's 40 %: {fields:?}");
        let row = h.get_by_label("[Paper]").rect().center();
        h.hover_at(row);
        h.drag_at(row);
        h.drop_at(row);
        h.run_steps(3);
        let defaults = h.state().session.active().unwrap().doc.settings.new_object.clone();
        assert_eq!((defaults.fill.as_str(), defaults.fill_tint), ("[Paper]", 1.0));
        assert_eq!(h.state().session.active().unwrap().doc.item(old).unwrap().fill, original);
    }
}

/// Stops in `object.gradient` form: a stop on a swatch also names it, so it keeps its inks.
fn stops_json(g: &designcraft_color::Gradient) -> Vec<serde_json::Value> {
    g.stops
        .iter()
        .map(|s| {
            let mut v = json!({"location": s.offset as f64 * 100.0, "color": color_json(&s.color), "opacity": s.opacity as f64 * 100.0, "midpoint": s.midpoint as f64 * 100.0});
            if let (Some(o), Some(w)) = (v.as_object_mut(), &s.swatch) {
                o.insert("swatch".to_string(), json!(w));
            }
            v
        })
        .collect()
}

fn color32(c: &designcraft_color::Color, alpha: f32) -> egui::Color32 {
    let [r, g, b] = c.to_rgb();
    egui::Color32::from_rgba_unmultiplied((r * 255.0) as u8, (g * 255.0) as u8, (b * 255.0) as u8, (alpha * 255.0) as u8)
}

/// The panel's thumbnail: the gradient in a square, linear across or radial from the centre.
fn thumbnail(p: &egui::Painter, r: egui::Rect, g: &designcraft_color::Gradient, border: egui::Color32) {
    let n = 32;
    if g.kind == designcraft_color::GradientKind::Radial {
        // Rings from the outside in.
        for i in (0..n).rev() {
            let t = (i as f32 + 0.5) / n as f32;
            let (c, a) = g.sample(t);
            let rad = r.width() / 2.0 * std::f32::consts::SQRT_2 * (i + 1) as f32 / n as f32;
            p.with_clip_rect(r).circle_filled(r.center(), rad, color32(&c, a));
        }
    } else {
        for i in 0..n {
            let (a, b) = (i as f32 / n as f32, (i + 1) as f32 / n as f32);
            let (c, al) = g.sample((a + b) / 2.0);
            let x0 = r.left() + r.width() * a;
            p.rect_filled(egui::Rect::from_x_y_ranges(x0..=r.left() + r.width() * b + 0.5, r.y_range()), 0.0, color32(&c, al));
        }
    }
    p.rect_stroke(r, 0.0, egui::Stroke::new(1.0, border), egui::StrokeKind::Outside);
}

/// Temp-data key of what's selected on the Gradient panel's ramp ([`RampSel`]): the Color panel
/// edits that stop.
pub(super) const GRAD_SEL: &str = "grad_sel";

/// The gradient across `ramp`, sampled in bands.
fn paint_ramp(painter: &egui::Painter, ramp: egui::Rect, g: &designcraft_color::Gradient) {
    let n = 96;
    for i in 0..n {
        let (a, b) = (i as f32 / n as f32, (i + 1) as f32 / n as f32);
        let (c, alpha) = g.sample((a + b) / 2.0);
        let x0 = ramp.left() + ramp.width() * a;
        let x1 = ramp.left() + ramp.width() * b + 0.5;
        painter.rect_filled(egui::Rect::from_x_y_ranges(x0..=x1, ramp.y_range()), 0.0, color32(&c, alpha));
    }
}

/// The stops after dragging marker `h` to `o` (`off`: dropped off the ramp, which removes a stop),
/// and the selection to move to if the drop changed it.
fn ramp_release(g: &designcraft_color::Gradient, h: RampSel, o: f32, off: bool) -> (Vec<serde_json::Value>, Option<RampSel>) {
    let mut stops = stops_json(g);
    let mut sel = None;
    match h {
        RampSel::Stop(i) if off => {
            if i < stops.len() {
                stops.remove(i);
            }
            sel = Some(RampSel::Stop(0));
        }
        RampSel::Stop(i) => set_stop(&mut stops, i, "location", json!(o as f64 * 100.0)),
        RampSel::Mid(i) => set_stop(&mut stops, i, "midpoint", json!(o as f64 * 100.0)),
    }
    (stops, sel)
}

/// Dragging the gradient's thumbnail: its swatch is the payload, and a thumbnail follows the pointer.
fn drag_gradient_thumbnail(ui: &egui::Ui, resp: &egui::Response, g: &designcraft_color::Gradient, swatch: &str, t: &Tokens) {
    resp.dnd_set_drag_payload(GradientDrag(swatch.to_string()));
    if resp.dragged()
        && let Some(p) = ui.ctx().pointer_interact_pos()
    {
        let painter = ui.ctx().layer_painter(egui::LayerId::new(egui::Order::Tooltip, egui::Id::unique("gradient_drag")));
        thumbnail(&painter, egui::Rect::from_min_size(p + vec2(8.0, 8.0), vec2(18.0, 18.0)), g, t.field_border);
    }
}

/// The Gradient panel's Type menu; the kind chosen, as the command's parameters.
fn gradient_type_combo(ui: &mut egui::Ui, lang: &str, g: &designcraft_color::Gradient, is_gradient: bool) -> Option<serde_json::Value> {
    let mut send = None;
    let shown = if is_gradient { crate::i18n::tr(lang, g.kind.label()) } else { "" };
    let combo = craft_ui::ComboBox::from_id_salt("grad_type").selected_text(crate::rtl::widget(ui, shown)).width(104.0).show_ui(ui, |ui| {
        for (k, v) in [("Linear", "linear"), ("Radial", "radial")] {
            if ui.selectable_label(is_gradient && g.kind.label() == k, crate::rtl::widget(ui, crate::i18n::tr(lang, k))).clicked() {
                send = Some(json!({"kind": v}));
            }
        }
    });
    let _ = crate::widgets::name_combo(combo, crate::i18n::tr(lang, "Type"));
    send
}

/// What's selected on the ramp: a stop, or the midpoint after a stop.
#[derive(Clone, Copy, PartialEq, Debug)]
pub(super) enum RampSel {
    Stop(usize),
    Mid(usize),
}

/// A marker being dragged: what, where (0–1), off the panel (stops only).
type RampDrag = (RampSel, f32, bool);

/// The drag kept from the last frame, if it still names a marker of a gradient with `n` stops
/// (the selection or its gradient may have changed since).
fn live_drag(d: Option<RampDrag>, n: usize) -> Option<RampDrag> {
    d.filter(|(h, _, _)| match *h {
        RampSel::Stop(i) => i < n,
        RampSel::Mid(i) => i.saturating_add(1) < n,
    })
}

/// Sets `key` of stop `i` in `object.gradient` stops; nothing if there's no such stop.
/// A new colour replaces the stop's swatch (`object.gradient` gives `swatch` precedence).
fn set_stop_color(stops: &mut [serde_json::Value], i: usize, color: serde_json::Value) {
    if let Some(o) = stops.get_mut(i).and_then(serde_json::Value::as_object_mut) {
        o.remove("swatch");
    }
    set_stop(stops, i, "color", color);
}

fn set_stop(stops: &mut [serde_json::Value], i: usize, key: &str, v: serde_json::Value) {
    if let Some(o) = stops.get_mut(i).and_then(serde_json::Value::as_object_mut) {
        o.insert(key.to_string(), v);
    }
}

/// Gradient panel, laid out as InDesign's: thumbnail, Type, Location and Angle, Reverse, and the
/// ramp with midpoint diamonds above it and stops below it. Drag a stop or a diamond to move it,
/// click below the ramp to add a stop, drag a stop off to remove it, and drop a swatch from the
/// Swatches panel on a stop to colour it or elsewhere on the ramp to add a stop (or right-click a
/// stop for its colour). Until the fill is
/// a gradient the ramp shows the default White → Black, and using it gives the fill a gradient.
pub fn gradient_panel(app: &mut DesignApp, ui: &mut egui::Ui) {
    let t = Tokens::get(ui.ctx());
    let lang = app.ui.language.clone();
    let Some(st) = app.session.active() else { return };
    let item = st.selection.items.first().and_then(|i| st.doc.item(*i));
    let has_item = item.is_some();
    // The fill's gradient, or the stroke's when the toolbox's Stroke box is in front.
    let stroke_front = app.ui.stroke_front;
    let fill = item.map(|it| if stroke_front { it.stroke.as_fill() } else { it.fill.clone() }).unwrap_or_default();
    let cur = designcraft_color::swatch::resolve_gradient(&st.doc.swatches, &fill.swatch).cloned();
    let is_gradient = cur.is_some();
    let g = cur.unwrap_or_default();
    let radial = g.kind == designcraft_color::GradientKind::Radial;
    let sel_id = egui::Id::unique(GRAD_SEL);
    let drag_id = egui::Id::unique("grad_drag");
    let mut sel: RampSel = ui.data(|d| d.get_temp(sel_id)).unwrap_or(RampSel::Stop(0));
    let last = g.stops.len().saturating_sub(1);
    sel = match sel {
        RampSel::Stop(i) => RampSel::Stop(i.min(last)),
        RampSel::Mid(i) if i < last => RampSel::Mid(i),
        RampSel::Mid(_) => RampSel::Stop(0),
    };
    let mut send: Option<serde_json::Value> = None;
    // The ramp spans the panel, however wide the rows above are.
    let full = ui.available_width().max(60.0);
    ui.add_enabled_ui(has_item, |ui| {
        ui.horizontal(|ui| {
            let (r, resp) = ui.allocate_exact_size(vec2(34.0, 34.0), Sense::click_and_drag());
            thumbnail(ui.painter(), r.shrink(1.0), &g, t.field_border);
            // Drag the gradient onto the Swatches panel to keep it as a swatch.
            if is_gradient {
                drag_gradient_thumbnail(ui, &resp, &g, &fill.swatch, &t);
            }
            // Clicking the thumbnail applies the gradient shown (InDesign's way to start one).
            if resp.clicked() && !is_gradient {
                send = Some(json!({}));
            }
            ui.add_space(4.0);
            ui.vertical(|ui| {
                ui.spacing_mut().item_spacing.x = 4.0;
                ui.horizontal(|ui| {
                    crate::widgets::caption_w(ui, crate::i18n::tr(&lang, "Type:"), 52.0);
                    send = gradient_type_combo(ui, &lang, &g, is_gradient).or(send.take());
                });
                ui.horizontal(|ui| {
                    crate::widgets::caption_w(ui, crate::i18n::tr(&lang, "Location:"), 52.0);
                    let loc = match sel {
                        RampSel::Stop(i) => g.stops.get(i).map(|s| s.offset as f64 * 100.0),
                        RampSel::Mid(i) => g.stops.get(i).map(|s| s.midpoint as f64 * 100.0),
                    };
                    if let Some(v) = crate::widgets::number(ui, "grad_loc", loc.filter(|_| is_gradient), "", 50.0, 3) {
                        let mut stops = stops_json(&g);
                        match sel {
                            RampSel::Stop(i) => set_stop(&mut stops, i, "location", json!(v.clamp(0.0, 100.0))),
                            RampSel::Mid(i) => set_stop(&mut stops, i, "midpoint", json!(v.clamp(13.0, 87.0))),
                        }
                        send = Some(json!({"stops": stops}));
                    }
                    crate::widgets::caption(ui, "%");
                    ui.add_space(4.0);
                    crate::widgets::caption(ui, crate::i18n::tr(&lang, "Angle:"));
                    let angle =
                        fill.gradient_vector.map(|[x0, y0, x1, y1]| (-(y1 - y0)).atan2(x1 - x0).to_degrees()).or(fill.gradient_angle).unwrap_or(0.0);
                    ui.add_enabled_ui(is_gradient && !radial, |ui| {
                        if let Some(v) = crate::widgets::number(ui, "grad_angle", is_gradient.then_some(angle), "°", 44.0, 1) {
                            send = Some(json!({"angle": v}));
                        }
                    });
                });
                ui.horizontal(|ui| {
                    crate::widgets::caption_w(ui, crate::i18n::tr(&lang, "Reverse"), 52.0);
                    if icons::button(ui, "swap", 20.0, false, crate::i18n::tr(&lang, "Reverse the gradient")).clicked() && is_gradient {
                        send = Some(json!({"reverse": true}));
                    }
                });
            });
        });
        ui.add_space(6.0);
        // The ramp: midpoint diamonds above, stops below.
        let w = full;
        let (area, resp) = ui.allocate_exact_size(vec2(w, 40.0), Sense::click_and_drag());
        let ramp = egui::Rect::from_min_size(area.min + vec2(6.0, 10.0), vec2(w - 12.0, 14.0));
        let painter = ui.painter();
        paint_ramp(painter, ramp, &g);
        painter.rect_stroke(ramp, 0.0, egui::Stroke::new(1.0, t.field_border), egui::StrokeKind::Outside);
        let x_of = |o: f32| ramp.left() + ramp.width() * o;
        let o_of = |x: f32| ((x - ramp.left()) / ramp.width()).clamp(0.0, 1.0);
        let mid_x = |i: usize| match (g.stops.get(i), g.stops.get(i + 1)) {
            (Some(a), Some(b)) => Some(x_of(a.offset + (b.offset - a.offset) * a.midpoint)),
            _ => None,
        };
        // The marker under the pointer: a stop below the ramp, a diamond above it.
        let hit = |p: egui::Pos2| -> Option<RampSel> {
            if p.y < ramp.top() {
                (0..last)
                    .filter_map(|i| mid_x(i).map(|x| (i, (x - p.x).abs())))
                    .filter(|(_, d)| *d < 6.0)
                    .min_by(|a, b| a.1.total_cmp(&b.1))
                    .map(|(i, _)| RampSel::Mid(i))
            } else {
                g.stops
                    .iter()
                    .enumerate()
                    .map(|(i, s)| (i, (x_of(s.offset) - p.x).abs()))
                    .filter(|(_, d)| *d < 7.0)
                    .min_by(|a, b| a.1.total_cmp(&b.1))
                    .map(|(i, _)| RampSel::Stop(i))
            }
        };
        let mut dragging = live_drag(ui.data(|d| d.get_temp(drag_id)), g.stops.len());
        if resp.drag_started()
            && let Some(p) = resp.interact_pointer_pos()
            && let Some(h) = hit(p)
            && let Some(o) = match h {
                RampSel::Stop(i) => g.stops.get(i).map(|s| s.offset),
                RampSel::Mid(i) => g.stops.get(i).map(|s| s.midpoint),
            }
        {
            sel = h;
            dragging = Some((h, o, false));
        }
        if resp.dragged()
            && let (Some((h, _, _)), Some(p)) = (dragging, resp.interact_pointer_pos())
        {
            dragging = Some(match h {
                RampSel::Stop(_) => (h, o_of(p.x), p.y > area.bottom() + 16.0 && g.stops.len() > 2),
                RampSel::Mid(i) => {
                    let m = match (g.stops.get(i), g.stops.get(i + 1)) {
                        (Some(a), Some(b)) if b.offset - a.offset > 1e-6 => ((o_of(p.x) - a.offset) / (b.offset - a.offset)).clamp(0.13, 0.87),
                        _ => 0.5,
                    };
                    (h, m, false)
                }
            });
        }
        if resp.drag_stopped()
            && let Some((h, o, off)) = dragging.take()
        {
            let (stops, kept) = ramp_release(&g, h, o, off);
            if let Some(kept) = kept {
                sel = kept;
            }
            send = Some(json!({"stops": stops}));
        }
        if resp.clicked()
            && let Some(p) = resp.interact_pointer_pos()
        {
            match hit(p) {
                Some(h) => sel = h,
                // Below the ramp, away from the stops: a new stop with the colour there.
                None if p.y > ramp.bottom() => {
                    let o = o_of(p.x);
                    let (c, a) = g.sample(o);
                    let mut stops = stops_json(&g);
                    stops.push(json!({"location": o as f64 * 100.0, "color": color_json(&c), "opacity": a as f64 * 100.0, "midpoint": 50.0}));
                    send = Some(json!({"stops": stops}));
                    sel = RampSel::Stop(g.stops.iter().filter(|s| s.offset <= o).count());
                }
                None => {}
            }
        }
        // Right-click a stop: its colour (InDesign takes it from the Swatches or Color panel).
        if resp.secondary_clicked()
            && let Some(RampSel::Stop(i)) = resp.interact_pointer_pos().and_then(hit)
        {
            sel = RampSel::Stop(i);
        }
        if let (true, RampSel::Stop(i)) = (is_gradient, sel)
            && let Some(s) = g.stops.get(i)
        {
            resp.context_menu(|ui| {
                ui.horizontal(|ui| {
                    crate::widgets::caption(ui, crate::i18n::tr(&lang, "Stop Color:"));
                    let [r, gg, bb] = s.color.to_rgb();
                    let mut rgb = [(r * 255.0) as u8, (gg * 255.0) as u8, (bb * 255.0) as u8];
                    if egui::color_picker::color_edit_button_srgb(ui, &mut rgb).changed() {
                        let mut stops = stops_json(&g);
                        set_stop_color(&mut stops, i, json!(format!("#{:02x}{:02x}{:02x}", rgb[0], rgb[1], rgb[2])));
                        send = Some(json!({"stops": stops}));
                    }
                });
            });
        }
        // A swatch dropped from the Swatches panel: on a stop it colours that stop, elsewhere on
        // the ramp it's a new stop there.
        let drop_at = |p: egui::Pos2| match hit(egui::pos2(p.x, ramp.bottom() + 4.0)) {
            Some(RampSel::Stop(i)) => match g.stops.get(i) {
                Some(s) => (Some(i), s.offset),
                None => (None, o_of(p.x)),
            },
            _ => (None, o_of(p.x)),
        };
        if resp.dnd_hover_payload::<SwatchDrag>().is_some()
            && let Some(p) = ui.ctx().pointer_hover_pos()
        {
            let x = x_of(drop_at(p).1);
            painter.line_segment([egui::pos2(x, ramp.top() - 3.0), egui::pos2(x, ramp.bottom() + 14.0)], egui::Stroke::new(2.0, t.accent));
        }
        if let Some(sw) = resp.dnd_release_payload::<SwatchDrag>()
            && let Some(p) = ui.ctx().pointer_interact_pos()
        {
            let mut stops = stops_json(&g);
            match drop_at(p) {
                (Some(i), _) => {
                    if let Some(o) = stops.get_mut(i).and_then(serde_json::Value::as_object_mut) {
                        o.insert("swatch".to_string(), json!(sw.0));
                        o.remove("color");
                    }
                    sel = RampSel::Stop(i);
                }
                (None, o) => {
                    stops.push(json!({"location": o as f64 * 100.0, "swatch": sw.0, "midpoint": 50.0}));
                    sel = RampSel::Stop(g.stops.iter().filter(|s| s.offset <= o).count());
                }
            }
            send = Some(json!({"stops": stops}));
        }
        // Diamonds.
        for i in 0..last {
            let x = match dragging {
                Some((RampSel::Mid(j), m, _)) if j == i => match (g.stops.get(i), g.stops.get(i + 1)) {
                    (Some(a), Some(b)) => x_of(a.offset + (b.offset - a.offset) * m),
                    _ => ramp.left(),
                },
                _ => mid_x(i).unwrap_or(ramp.left()),
            };
            let (cy, r) = (ramp.top() - 6.0, 4.0);
            let pts = vec![egui::pos2(x, cy - r), egui::pos2(x + r, cy), egui::pos2(x, cy + r), egui::pos2(x - r, cy)];
            let on = sel == RampSel::Mid(i);
            painter.add(egui::Shape::convex_polygon(pts, if on { t.text } else { egui::Color32::TRANSPARENT }, egui::Stroke::new(1.0, t.text_dim)));
        }
        // Stops: a colour square under a pointer.
        for (i, s) in g.stops.iter().enumerate() {
            let (o, off) = match dragging {
                Some((RampSel::Stop(j), o, off)) if j == i => (o, off),
                _ => (s.offset, false),
            };
            if off {
                continue;
            }
            let x = x_of(o);
            let top = ramp.bottom() + 2.0;
            let on = sel == RampSel::Stop(i);
            let edge = if on { t.text_strong } else { t.text_dim };
            painter.add(egui::Shape::convex_polygon(
                vec![egui::pos2(x, top), egui::pos2(x + 5.0, top + 5.0), egui::pos2(x - 5.0, top + 5.0)],
                if on { t.text_strong } else { t.panel },
                egui::Stroke::new(1.0, edge),
            ));
            let sq = egui::Rect::from_min_size(egui::pos2(x - 5.0, top + 5.0), vec2(10.0, 9.0));
            painter.rect_filled(sq, 0.0, color32(&s.color, 1.0));
            painter.rect_stroke(sq, 0.0, egui::Stroke::new(if on { 1.5 } else { 1.0 }, edge), egui::StrokeKind::Inside);
        }
        ui.data_mut(|d| {
            d.insert_temp(sel_id, sel);
            match dragging {
                Some(v) => {
                    d.insert_temp(drag_id, v);
                }
                None => d.remove::<RampDrag>(drag_id),
            }
        });
    });
    if let Some(mut p) = send {
        if let Some(o) = p.as_object_mut() {
            o.insert("target".to_string(), json!(if stroke_front { "stroke" } else { "fill" }));
        }
        let _ = app.run("object.gradient", p);
    }
}

#[cfg(test)]
mod gradient_panel_tests {
    use designcraft_color::Color;
    use egui_kittest::{Harness, kittest::Queryable};
    use serde_json::json;

    use super::*;

    fn app_with_frame() -> DesignApp {
        let mut app = DesignApp::new(designcraft_engine::Session::new(), crate::Services::default());
        app.run("file.new", json!({})).unwrap();
        app.run("frame.create", json!({"rect": [100, 100, 300, 300], "content": "unassigned"})).unwrap();
        app
    }

    fn gradient(app: &DesignApp) -> Option<designcraft_color::Gradient> {
        let d = &app.session.doc().unwrap().doc;
        let id = app.session.doc().unwrap().selection.items[0];
        designcraft_color::swatch::resolve_gradient(&d.swatches, &d.item(id).unwrap().fill.swatch).cloned()
    }

    #[test]
    fn panel_edits_keep_cmyk_and_grey_stops() {
        let mut app = app_with_frame();
        app.run(
            "object.gradient",
            json!({"stops": [{"location": 0, "swatch": "C=0 M=100 Y=0 K=0"}, {"location": 50, "color": {"gray": 60}}, {"location": 100, "color": {"model": "gray", "k": 0.01}}]}),
        )
        .unwrap();
        let g = gradient(&app).unwrap();
        // What the panel sends when a stop moves: the colours come back exactly.
        let mut stops = stops_json(&g);
        stops[1]["location"] = json!(80.0);
        app.run("object.gradient", json!({"stops": stops})).unwrap();
        let g = gradient(&app).unwrap();
        assert_eq!(g.stops[0].color, Color::cmyk(0.0, 1.0, 0.0, 0.0));
        assert_eq!(g.stops[1].color, Color::gray(0.6));
        assert!((g.stops[1].offset - 0.8).abs() < 1e-6);
        assert_eq!(g.stops[2].color, Color::gray(0.01), "a 1 % grey stays 1 %");
        assert_eq!(g.stops[0].swatch.as_deref(), Some("C=0 M=100 Y=0 K=0"), "a stop keeps its swatch");
        assert_eq!(g.stops[1].swatch, None);
    }

    #[test]
    fn a_drag_kept_from_a_longer_gradient_is_dropped() {
        let d = |h| Some((h, 0.5, false));
        assert_eq!(live_drag(d(RampSel::Stop(1)), 2), d(RampSel::Stop(1)));
        assert_eq!(live_drag(d(RampSel::Stop(2)), 2), None);
        assert_eq!(live_drag(d(RampSel::Mid(0)), 2), d(RampSel::Mid(0)));
        assert_eq!(live_drag(d(RampSel::Mid(1)), 2), None);
        assert_eq!(live_drag(d(RampSel::Mid(usize::MAX)), 2), None);
        let mut stops = vec![json!({"location": 0})];
        set_stop(&mut stops, 3, "location", json!(50));
        assert_eq!(stops, vec![json!({"location": 0})]);
    }

    #[test]
    fn a_swatch_dropped_on_the_ramp_is_a_new_stop() {
        let app = app_with_frame();
        let mut h = Harness::builder().with_size(vec2(280.0, 200.0)).build_ui_state(
            |ui, app: &mut DesignApp| {
                let id = egui::Id::unique("test_fonts");
                if ui.data(|d| d.get_temp::<bool>(id)).is_none() {
                    crate::theme::install_fonts(ui.ctx(), "");
                    ui.data_mut(|d| d.insert_temp(id, true));
                    return;
                }
                gradient_panel(app, ui);
            },
            app,
        );
        h.run_steps(3);
        assert!(gradient(h.state()).is_none(), "no gradient yet");
        // The ramp sits under the Reverse row: drop cyan at its middle.
        let row = h.get_by_label("Reverse the gradient").rect();
        let at = egui::pos2(h.ctx.content_rect().center().x, row.bottom() + 6.0 + 17.0);
        egui::DragAndDrop::set_payload(&h.ctx, SwatchDrag("C=100 M=0 Y=0 K=0".into()));
        h.event(egui::Event::PointerMoved(at));
        h.run_steps(2);
        h.event(egui::Event::PointerButton { pos: at, button: egui::PointerButton::Primary, pressed: false, modifiers: Default::default() });
        h.run_steps(3);
        let g = gradient(h.state()).expect("the drop gives the fill a gradient");
        assert_eq!(g.stops.len(), 3, "White, cyan, Black");
        assert_eq!(g.stops[1].color, Color::cmyk(1.0, 0.0, 0.0, 0.0));
        assert!((g.stops[1].offset - 0.5).abs() < 0.05, "{}", g.stops[1].offset);
    }

    /// The Gradient panel's thumbnail drags the selection's gradient onto the Swatches panel,
    /// which keeps it as a gradient swatch.
    #[test]
    fn a_gradient_dragged_onto_the_swatches_becomes_a_swatch() {
        let mut app = app_with_frame();
        app.run("object.gradient", json!({})).unwrap();
        app.run("window.panel.float", json!({"panel": "gradient", "x": 300, "y": 120})).unwrap();
        app.run("window.panel.float", json!({"panel": "swatches", "x": 620, "y": 120})).unwrap();
        let named =
            |app: &DesignApp| app.session.doc().unwrap().doc.swatches.iter().filter(|w| !w.hidden).map(|w| w.name.clone()).collect::<Vec<_>>();
        let before = named(&app);
        let mut h = crate::test_window::open(app, vec2(1400.0, 900.0));
        h.run_steps(6);
        drag_gradient_to(&mut h, "[Black]");
        let after = named(&h.state().app);
        assert_eq!(after.len(), before.len() + 1);
        assert_eq!(after.last().map(String::as_str), Some("New Gradient Swatch"));
        let d = &h.state().app.session.doc().unwrap().doc;
        assert!(matches!(d.swatch("New Gradient Swatch").map(|w| &w.value), Some(SwatchValue::Gradient { .. })));
        let id = h.state().app.session.doc().unwrap().selection.items[0];
        assert_eq!(d.item(id).unwrap().fill.swatch, "New Gradient Swatch", "the object keeps it, now by name");
    }

    /// Drag the Gradient panel's thumbnail onto the Swatches panel row labelled `label`.
    fn drag_gradient_to(h: &mut egui_kittest::Harness<'static, crate::test_window::Window>, label: &str) {
        use egui::{Event, Modifiers, PointerButton};
        // The thumbnail sits left of the Type row.
        let ty = h.get_by_label("Type:").rect();
        let from = egui::pos2(ty.min.x - 21.0, ty.min.y + 12.0);
        let to = h.get_by(|n| n.label().is_some_and(|l| crate::panels::swatches::names_swatch(&l, label))).rect().center();
        h.event(Event::PointerMoved(from));
        h.step();
        h.event(Event::PointerButton { pos: from, button: PointerButton::Primary, pressed: true, modifiers: Modifiers::NONE });
        h.step();
        for i in 1..=10 {
            h.event(Event::PointerMoved(from + (to - from) * (i as f32 / 10.0)));
            h.step();
        }
        h.event(Event::PointerButton { pos: to, button: PointerButton::Primary, pressed: false, modifiers: Modifiers::NONE });
        h.run_steps(4);
    }

    /// A colour group's row is part of the list too: a gradient dropped on it becomes a swatch.
    #[test]
    fn a_gradient_dropped_on_a_colour_group_becomes_a_swatch() {
        let mut app = app_with_frame();
        app.run("object.gradient", json!({})).unwrap();
        let group = app.run("swatch.newColorGroup", json!({})).unwrap()["name"].as_str().unwrap().to_string();
        app.run("window.panel.float", json!({"panel": "gradient", "x": 300, "y": 120})).unwrap();
        // Tall enough to show the group's row below the swatches.
        app.run("window.panel.float", json!({"panel": "swatches", "x": 620, "y": 120, "height": 700})).unwrap();
        let mut h = crate::test_window::open(app, vec2(1400.0, 900.0));
        h.run_steps(6);
        drag_gradient_to(&mut h, &group);
        let d = &h.state().app.session.doc().unwrap().doc;
        assert!(matches!(d.swatch("New Gradient Swatch").map(|w| &w.value), Some(SwatchValue::Gradient { .. })), "dropped on the group's row");
    }
}

#[cfg(test)]
mod color_panel_tests {
    use egui::vec2;
    use egui_kittest::Harness;
    use egui_kittest::kittest::Queryable;
    use serde_json::json;

    use crate::DesignApp;

    fn fill(app: &DesignApp, id: u64) -> designcraft_color::Color {
        let d = &app.session.active().unwrap().doc;
        d.resolve_color(&d.item(designcraft_doc::ItemId(id)).unwrap().fill.swatch, 1.0).unwrap()
    }

    /// A number typed into the M field stays while typing and applies as a percent on Enter:
    /// M 1 is 1 %, not 100 %.
    #[test]
    fn a_typed_channel_value_is_kept_and_applied_as_a_percent() {
        let mut app = DesignApp::new(designcraft_engine::Session::new(), crate::Services::default());
        app.run("file.new", json!({})).unwrap();
        let id = app.run("frame.create", json!({"rect": [36, 36, 200, 200]})).unwrap()["id"].as_u64().unwrap();
        app.run("selection.set", json!({"ids": [id]})).unwrap();
        app.run("object.color", json!({"color": {"c": 0, "m": 50, "y": 100, "k": 0}})).unwrap();
        let mut h = Harness::builder().with_size(vec2(320.0, 300.0)).build_ui_state(|ui, app: &mut DesignApp| super::color_panel(app, ui), app);
        h.run_steps(3);
        let fields: Vec<egui::Rect> = h.query_all_by(|n| n.role() == egui::accesskit::Role::SpinButton).map(|n| n.rect()).collect();
        assert_eq!(fields.len(), 4, "C, M, Y and K fields");
        let m = fields[1].center();
        h.hover_at(m);
        h.drag_at(m);
        h.drop_at(m);
        h.run_steps(2);
        h.key_press_modifiers(egui::Modifiers::COMMAND, egui::Key::A);
        h.event(egui::Event::Text("1".into()));
        h.run_steps(4);
        let typed = h.query_by(|n| n.role() == egui::accesskit::Role::SpinButton && n.is_focused()).and_then(|n| n.value());
        assert_eq!(typed.as_deref(), Some("1"), "the typed text stays while the field has focus");
        let designcraft_color::Color::Cmyk { m, .. } = fill(h.state(), id) else { panic!("not CMYK") };
        assert!((m - 0.5).abs() < 1e-6, "nothing is applied while typing: {m}");
        h.key_press(egui::Key::Enter);
        h.run_steps(2);
        let designcraft_color::Color::Cmyk { c, m, y, k } = fill(h.state(), id) else { panic!("not CMYK") };
        assert!(c == 0.0 && (m - 0.01).abs() < 1e-6 && (y - 1.0).abs() < 1e-6 && k == 0.0, "{c} {m} {y} {k}");
    }
}

#[cfg(test)]
mod swatches_header_tests {
    use egui_kittest::{Harness, kittest::Queryable};
    use serde_json::json;

    use super::*;

    fn harness(app: DesignApp) -> Harness<'static, DesignApp> {
        Harness::builder().with_size(vec2(300.0, 520.0)).build_ui_state(
            |ui, app: &mut DesignApp| {
                let id = egui::Id::unique("test_fonts");
                if ui.data(|d| d.get_temp::<bool>(id)).is_none() {
                    crate::theme::install_fonts(ui.ctx(), "");
                    ui.data_mut(|d| d.insert_temp(id, true));
                    return;
                }
                show(app, ui);
            },
            app,
        )
    }

    fn items(app: &DesignApp) -> Vec<designcraft_doc::Item> {
        let st = app.session.doc().unwrap();
        st.selection.items.iter().map(|id| st.doc.item(*id).unwrap().clone()).collect()
    }

    #[test]
    fn swatches_go_to_the_fill_or_stroke_in_front_and_the_bottom_row_deletes_the_one_clicked() {
        let mut app = DesignApp::new(designcraft_engine::Session::new(), crate::Services::default());
        app.run("file.new", json!({})).unwrap();
        app.run("frame.create", json!({"rect": [36, 36, 200, 200], "content": "unassigned"})).unwrap();
        app.run("swatch.create", json!({"name": "Brand", "color": "#108080"})).unwrap();
        let mut h = harness(app);
        h.run_steps(3);
        // Fill in front: a swatch is the fill.
        h.get_by(|n| n.label().is_some_and(|l| crate::panels::swatches::names_swatch(&l, "C=0 M=100 Y=0 K=0"))).click();
        h.run_steps(2);
        assert_eq!(items(h.state())[0].fill.swatch, "C=0 M=100 Y=0 K=0");
        // The proxy brings the stroke to the front; the next swatch is the stroke.
        h.get_by_label("Fill").click();
        h.run_steps(2);
        assert!(h.state().ui.stroke_front);
        h.get_by(|n| n.label().is_some_and(|l| crate::panels::swatches::names_swatch(&l, "C=100 M=0 Y=0 K=0"))).click();
        h.run_steps(2);
        let it = &items(h.state())[0];
        assert_eq!((it.fill.swatch.as_str(), it.stroke.swatch.as_str()), ("C=0 M=100 Y=0 K=0", "C=100 M=0 Y=0 K=0"));
        // Formatting affects text, from the panel.
        h.get_by_label("Formatting affects text (J)").click();
        h.run_steps(2);
        assert!(h.state().ui.formatting_affects_text);
        h.get_by_label("Formatting affects container (J)").click();
        h.run_steps(2);
        assert!(!h.state().ui.formatting_affects_text);
        // Delete Swatch removes the swatch last clicked.
        h.get_by(|n| n.label().is_some_and(|l| crate::panels::swatches::names_swatch(&l, "Brand"))).click();
        h.run_steps(2);
        h.get_by_label("Delete Swatch").click();
        h.run_steps(2);
        assert!(h.state().session.doc().unwrap().doc.swatch("Brand").is_none());
    }

    /// A rename in place belongs to its document: with another one active, a swatch of the same
    /// name there gets no field, and nothing is renamed.
    #[test]
    fn a_rename_in_place_ends_with_its_document() {
        let mut app = DesignApp::new(designcraft_engine::Session::new(), crate::Services::default());
        app.run("file.new", json!({})).unwrap();
        let color = json!({"color": {"c": 14, "m": 8, "y": 95, "k": 0}});
        let name = app.run("swatch.create", color.clone()).unwrap()["name"].as_str().unwrap().to_string();
        let mut h = Harness::builder().with_size(vec2(300.0, 520.0)).build_ui_state(
            |ui, app: &mut DesignApp| {
                let id = egui::Id::unique("rename_test_fonts");
                if ui.data(|d| d.get_temp::<bool>(id)).is_none() {
                    crate::theme::install_fonts(ui.ctx(), "");
                    ui.data_mut(|d| d.insert_temp(id, true));
                    return;
                }
                super::show(app, ui);
            },
            app,
        );
        h.run_steps(3);
        let fields = h.query_all_by_role(egui::accesskit::Role::TextInput).count();
        let renaming = egui::Id::unique(super::RENAMING);
        h.ctx.data_mut(|d| d.insert_temp(renaming, (name.clone(), "Typed".to_string())));
        h.run_steps(2);
        assert!(h.query_all_by_role(egui::accesskit::Role::TextInput).count() > fields, "the name is a field");
        // Another document with a swatch of the same name.
        h.state_mut().run("file.new", json!({})).unwrap();
        h.state_mut().run("swatch.create", color).unwrap();
        h.run_steps(3);
        assert!(h.ctx.data(|d| d.get_temp::<(String, String)>(renaming)).is_none(), "the rename is dropped");
        let doc = &h.state().session.active().unwrap().doc;
        assert!(doc.swatch(&name).is_some() && doc.swatch("Typed").is_none());
    }

    /// The course's chapter 23: a swatch added as "C=14 M=8 Y=95 K=0" is renamed in the panel by
    /// clicking it, then clicking its name again: the name becomes a field, and Enter renames the
    /// swatch (and what uses it). A double-click still opens Swatch Options, Escape keeps the name,
    /// and [Black] can't be renamed.
    #[test]
    fn a_second_click_on_the_picked_swatch_renames_it_in_place() {
        use egui::accesskit::Role;
        let mut app = DesignApp::new(designcraft_engine::Session::new(), crate::Services::default());
        app.run("file.new", json!({})).unwrap();
        app.run("frame.create", json!({"rect": [36, 36, 200, 200], "content": "unassigned"})).unwrap();
        let added = app.run("swatch.create", json!({"color": {"c": 14, "m": 8, "y": 95, "k": 0}})).unwrap()["name"].as_str().unwrap().to_string();
        assert_eq!(added, "C=14 M=8 Y=95 K=0");
        // Frames 50 ms apart: the double-click time is six of them.
        let mut h = Harness::builder().with_size(vec2(300.0, 520.0)).with_step_dt(0.05).build_ui_state(
            |ui, app: &mut DesignApp| {
                let id = egui::Id::unique("test_fonts");
                if ui.data(|d| d.get_temp::<bool>(id)).is_none() {
                    crate::theme::install_fonts(ui.ctx(), "");
                    ui.data_mut(|d| d.insert_temp(id, true));
                    return;
                }
                show(app, ui);
            },
            app,
        );
        h.run_steps(3);
        // The panel's own fields (Tint); renaming adds one.
        let fields = h.query_all_by_role(Role::TextInput).count();
        let field = |h: &Harness<'_, DesignApp>| h.query_all_by_role(Role::TextInput).count() > fields;
        let has = |h: &Harness<'_, DesignApp>, n: &str| h.state().session.doc().unwrap().doc.swatch(n).is_some();
        let name_at = |h: &Harness<'_, DesignApp>, name: &str| {
            let row = h.query_all_by(|n| n.label().is_some_and(|l| names_swatch(&l, name))).next().expect("the swatch's row");
            row.rect().left_center() + vec2(40.0, 0.0)
        };
        let click = |h: &mut Harness<'_, DesignApp>, at: egui::Pos2| {
            h.hover_at(at);
            h.event(egui::Event::PointerButton { pos: at, button: egui::PointerButton::Primary, pressed: true, modifiers: egui::Modifiers::NONE });
            h.event(egui::Event::PointerButton { pos: at, button: egui::PointerButton::Primary, pressed: false, modifiers: egui::Modifiers::NONE });
            h.step();
        };
        // A click on the name: applied, and picked.
        let at = name_at(&h, &added);
        click(&mut h, at);
        h.run_steps(10);
        assert_eq!(items(h.state())[0].fill.swatch, added);
        assert!(!field(&h));
        // Again, after the double-click time: the name is a field, with the name selected.
        click(&mut h, at);
        h.run_steps(10);
        assert!(field(&h), "the name is a field");
        h.event(egui::Event::Text("Lemon Yellow Sun".into()));
        h.step();
        h.key_press(egui::Key::Enter);
        h.run_steps(3);
        assert!(has(&h, "Lemon Yellow Sun") && !has(&h, &added));
        assert_eq!(items(h.state())[0].fill.swatch, "Lemon Yellow Sun", "what uses it follows");
        // Escape keeps the name.
        let at = name_at(&h, "Lemon Yellow Sun");
        h.run_steps(10);
        click(&mut h, at);
        h.run_steps(10);
        assert!(field(&h));
        h.event(egui::Event::Text("Sour".into()));
        h.step();
        h.key_press(egui::Key::Escape);
        h.run_steps(3);
        assert!(has(&h, "Lemon Yellow Sun") && !field(&h));
        // A double-click on it opens Swatch Options, and doesn't rename.
        h.run_steps(20);
        click(&mut h, at);
        click(&mut h, at);
        h.run_steps(10);
        assert_eq!(h.state().ui.dialog.as_ref().map(|d| d.id.as_str()), Some("swatchOptions"));
        assert!(!field(&h), "no rename");
        // [Black] keeps its name.
        h.state_mut().ui.dialog = None;
        h.run_steps(20);
        let at = name_at(&h, "[Black]");
        click(&mut h, at);
        h.run_steps(10);
        click(&mut h, at);
        h.run_steps(10);
        assert!(!field(&h));
        // A click on another swatch while the rename waits: that one is picked, and nothing
        // renames.
        let picked = |h: &Harness<'_, DesignApp>| h.ctx.data(|d| d.get_temp::<String>(egui::Id::unique(super::PICKED)));
        let (lemon, paper) = (name_at(&h, "Lemon Yellow Sun"), name_at(&h, "[Paper]"));
        h.run_steps(20);
        click(&mut h, lemon);
        h.run_steps(10);
        click(&mut h, lemon);
        click(&mut h, paper);
        h.run_steps(10);
        assert!(!field(&h), "the waiting rename is cancelled");
        assert_eq!(picked(&h).as_deref(), Some("[Paper]"));
        // Renaming, a click on another swatch renames, and picks that one (for Delete Swatch).
        click(&mut h, lemon);
        h.run_steps(10);
        click(&mut h, lemon);
        h.run_steps(10);
        assert!(field(&h));
        h.event(egui::Event::Text("Lemon".into()));
        h.step();
        click(&mut h, paper);
        h.run_steps(5);
        assert!(has(&h, "Lemon") && !field(&h));
        assert_eq!(picked(&h).as_deref(), Some("[Paper]"), "the swatch clicked stays picked");
    }

    #[test]
    fn the_tint_field_keeps_each_objects_own_swatch() {
        let mut app = DesignApp::new(designcraft_engine::Session::new(), crate::Services::default());
        app.run("file.new", json!({})).unwrap();
        let a = app.run("frame.create", json!({"rect": [36, 36, 100, 100], "content": "unassigned"})).unwrap()["id"].as_u64().unwrap();
        let b = app.run("frame.create", json!({"rect": [136, 36, 200, 100], "content": "unassigned"})).unwrap()["id"].as_u64().unwrap();
        app.run("object.fill", json!({"ids": [a], "swatch": "C=100 M=0 Y=0 K=0"})).unwrap();
        app.run("object.fill", json!({"ids": [b], "swatch": "C=0 M=100 Y=0 K=0"})).unwrap();
        app.run("object.stroke", json!({"ids": [a, b], "swatch": "[Black]"})).unwrap();
        app.run("selection.set", json!({"ids": [a, b]})).unwrap();
        let mut h = harness(app);
        h.run_steps(3);
        // The field shows the first selected object's fill tint.
        assert!(h.query_all_by_value("100").next().is_some());
        set_tint(h.state_mut(), 40.0).unwrap();
        h.run_steps(2);
        assert!(h.query_all_by_value("40").next().is_some(), "the field follows");
        // The stroke in front: the strokes' tints.
        h.state_mut().ui.stroke_front = true;
        set_tint(h.state_mut(), 70.0).unwrap();
        assert!(items(h.state()).iter().all(|i| i.stroke.tint == 0.7));
        let it = items(h.state());
        assert_eq!((it[0].fill.swatch.as_str(), it[0].fill.tint), ("C=100 M=0 Y=0 K=0", 0.4));
        assert_eq!((it[1].fill.swatch.as_str(), it[1].fill.tint), ("C=0 M=100 Y=0 K=0", 0.4));
    }
}

#[cfg(test)]
mod row_mark_tests {
    use designcraft_color::swatch::{Swatch, default_swatches};
    use designcraft_color::{Color, ColorType, SwatchValue};

    use super::{RowMarks, RowMode, row_label, row_marks};

    /// A row's accessible name says what its marks show.
    #[test]
    fn swatch_rows_say_their_marks() {
        let all = default_swatches();
        let label = |name: &str| row_label("", name, row_marks(all.iter().find(|s| s.name == name).unwrap()));
        assert_eq!(label("[Black]"), "[Black], Process, CMYK, Cannot be edited");
        assert_eq!(label("[None]"), "[None], Cannot be edited");
        assert_eq!(label("[Paper]"), "[Paper]");
        let cyan = all.iter().find(|s| matches!(&s.value, SwatchValue::Color { .. }) && !s.locked).unwrap();
        let spot = Swatch {
            name: "Pantone".into(),
            value: SwatchValue::Color { color: Color::Rgb { r: 1.0, g: 0.0, b: 0.0 }, color_type: ColorType::Spot },
            locked: false,
            ..cyan.clone()
        };
        assert_eq!(row_label("it", &spot.name, row_marks(&spot)), "Pantone, Tinta piatta, RGB");
    }

    /// The Swatches panel's right-hand columns, as InDesign's: [None], [Registration] and [Black]
    /// can't be edited, [Paper] can; colours show their type and mode.
    #[test]
    fn swatch_rows_mark_type_mode_and_whether_they_can_be_edited() {
        let all = default_swatches();
        let marks = |name: &str| row_marks(all.iter().find(|s| s.name == name).unwrap());
        assert_eq!(marks("[None]"), RowMarks { not_editable: true, kind: None, mode: Some(RowMode::None) });
        assert_eq!(marks("[Registration]"), RowMarks { not_editable: true, kind: None, mode: Some(RowMode::Registration) });
        assert_eq!(marks("[Paper]"), RowMarks { not_editable: false, kind: None, mode: None });
        assert_eq!(marks("[Black]"), RowMarks { not_editable: true, kind: Some(ColorType::Process), mode: Some(RowMode::Cmyk) });
        let cyan = all.iter().find(|s| matches!(&s.value, SwatchValue::Color { .. }) && !s.locked).unwrap();
        assert_eq!(row_marks(cyan), RowMarks { not_editable: false, kind: Some(ColorType::Process), mode: Some(RowMode::Cmyk) });
        let spot = Swatch {
            name: "PANTONE".into(),
            value: SwatchValue::Color { color: Color::Rgb { r: 1.0, g: 0.0, b: 0.0 }, color_type: ColorType::Spot },
            locked: false,
            ..cyan.clone()
        };
        assert_eq!(row_marks(&spot), RowMarks { not_editable: false, kind: Some(ColorType::Spot), mode: Some(RowMode::Rgb) });
        // A locked Paper swatch under another name (a document from elsewhere) is still editable.
        let paper = all.iter().find(|s| s.name == designcraft_color::swatch::PAPER).unwrap();
        let renamed = designcraft_color::swatch::Swatch { name: "Papier".into(), ..paper.clone() };
        assert!(!row_marks(&renamed).not_editable);
    }
}
