//! Swatches and Color panels.

use designcraft_color::{ColorType, SwatchValue};
use egui::{Sense, vec2};
use serde_json::json;

use crate::theme::Tokens;
use crate::{DesignApp, icons};

pub fn show(app: &mut DesignApp, ui: &mut egui::Ui) {
    #[cfg(not(target_arch = "wasm32"))]
    if app.framework_installed {
        framework::show(app, ui);
        return;
    }
    prepare_rename(app, ui);
    let t = Tokens::get(ui.ctx());
    let Some(st) = app.session.active() else { return };
    let doc = st.doc.clone();
    // Another document (switched to, or moved to this one's place by a close) drops a rename in
    // place: a swatch of the same name there isn't the one being renamed.
    let uid = (st.uid, st.identity_epoch);
    if ui.data(|d| d.get_temp::<(u64, u64)>(egui::Id::unique(RENAME_DOC))) != Some(uid) {
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
    let grouped: std::collections::HashSet<&str> = doc.color_groups.iter().flat_map(|g| g.swatches.iter().map(String::as_str)).collect();
    // Swatches by name (the first of a repeated name), so a row's lookup isn't a search of the list.
    let mut by_name: std::collections::HashMap<&str, &designcraft_color::swatch::Swatch> = std::collections::HashMap::new();
    for w in &doc.swatches {
        by_name.entry(w.name.as_str()).or_insert(w);
    }
    // A gradient dragged from the Gradient panel's thumbnail is dropped on the list, colour
    // groups included.
    let (_, dropped) = ui.dnd_drop_zone::<GradientDrag, _>(egui::Frame::NONE, |ui| {
        for sw in doc.swatches.iter().filter(|w| !w.hidden && !grouped.contains(w.name.as_str())) {
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
                    if let Some(sw) = by_name.get(n.as_str()).copied() {
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
                .inner
                .clicked();
            if del
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

/// The editor belongs to one document incarnation and one actual source name, not object selection.
#[derive(Clone, Default)]
struct RenameInput {
    document: (u64, u64),
    source: String,
    last_frame: u64,
}

const RENAME_INPUT: &str = "swatches_rename_input";

fn retire_rename(ui: &egui::Ui) {
    ui.data_mut(|data| {
        data.remove_temp::<(String, String)>(egui::Id::unique(RENAMING));
        data.remove_temp::<(String, f64)>(egui::Id::unique(RENAME_CLICK));
        data.remove_temp::<RenameInput>(egui::Id::unique(RENAME_INPUT));
    });
}

/// An observed missing/locked/hidden source or inert parent cannot later replay its old field input.
fn prepare_rename(app: &DesignApp, ui: &egui::Ui) {
    let Some(st) = app.session.active() else {
        retire_rename(ui);
        return;
    };
    // A measurement within a live frame is not a new lifetime. It must not edit or retire the draft.
    if ui.is_sizing_pass() {
        return;
    }
    if !ui.is_enabled() || !ui.is_visible() {
        retire_rename(ui);
        return;
    }
    let Some((name, _)) = ui.data(|data| data.get_temp::<(String, String)>(egui::Id::unique(RENAMING))) else {
        ui.data_mut(|data| data.remove_temp::<RenameInput>(egui::Id::unique(RENAME_INPUT)));
        return;
    };
    let source_live = st.doc.swatch(&name).is_some_and(|sw| !sw.locked && !sw.hidden);
    let groups: Vec<_> = st.doc.color_groups.iter().filter(|group| group.swatches.contains(&name)).collect();
    let row_live = groups.is_empty()
        || groups.iter().any(|group| ui.data(|data| data.get_temp::<bool>(egui::Id::unique(("color_group_open", &group.name)))).unwrap_or(true));
    let frame = ui.ctx().cumulative_frame_nr();
    let owner_live = ui.data(|data| data.get_temp::<RenameInput>(egui::Id::unique(RENAME_INPUT))).is_none_or(|owner| {
        owner.document == (st.uid, st.identity_epoch)
            && owner.source == name
            && (owner.last_frame == frame || owner.last_frame.checked_add(1) == Some(frame))
    });
    if !source_live || !row_live || !owner_live {
        retire_rename(ui);
    }
}

/// The name field of a swatch being renamed in place, over `rect`; false when `sw` isn't. Enter or
/// a click elsewhere renames it (`swatch.options`), Escape leaves it as it was.
fn rename_field(app: &mut DesignApp, ui: &mut egui::Ui, sw: &designcraft_color::swatch::Swatch, rect: egui::Rect) -> bool {
    let key = egui::Id::unique(RENAMING);
    let Some((name, mut text)) = ui.data(|d| d.get_temp::<(String, String)>(key)).filter(|(n, _)| *n == sw.name) else { return false };
    if ui.is_sizing_pass() {
        return true;
    }
    let Some(st) = app.session.active() else {
        retire_rename(ui);
        return false;
    };
    let document = (st.uid, st.identity_epoch);
    let owner_key = egui::Id::unique(RENAME_INPUT);
    let frame = ui.ctx().cumulative_frame_nr();
    let mut owner =
        ui.data(|data| data.get_temp::<RenameInput>(owner_key)).unwrap_or_else(|| RenameInput { document, source: name.clone(), last_frame: frame });
    owner.last_frame = frame;
    ui.data_mut(|data| data.insert_temp(owner_key, owner));
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
    let (finished, escaped) = (resp.lost_focus(), ui.input(|i| i.key_pressed(egui::Key::Escape)));
    if finished {
        ui.data_mut(|d| {
            d.remove_temp::<(String, String)>(key);
            d.remove_temp::<RenameInput>(egui::Id::unique(RENAME_INPUT));
        });
        let to = text.trim();
        if !escaped && !to.is_empty() && to != name {
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
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub enum ColorMode {
    Cmyk,
    Rgb,
    Lab,
}

/// Color panel: fill/stroke proxy, sliders in the chosen model (or a tint slider for a named
/// swatch), the spectrum ramp, Add to Swatches. Edits apply unnamed colours (`object.color`).
pub fn color_panel(app: &mut DesignApp, ui: &mut egui::Ui) {
    #[cfg(not(target_arch = "wasm32"))]
    if app.framework_installed {
        framework::color_panel(app, ui);
        return;
    }
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
            let combo = craft_ui_legacy::ComboBox::from_id_salt("color_mode")
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
    let combo = craft_ui_legacy::ComboBox::from_id_salt("grad_type").selected_text(crate::rtl::widget(ui, shown)).width(104.0).show_ui(ui, |ui| {
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
    #[cfg(not(target_arch = "wasm32"))]
    if app.framework_installed {
        framework::gradient_panel(app, ui);
        return;
    }
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

/// The Swatches, Color and Gradient panels on the craft UI framework, shown while it is installed
/// (`DesignApp::framework_installed`): the swatch list and its folders are one framework `List` (a tree, one pick,
/// rename in place, drags to the Gradient panel's ramp, drops of the Gradient panel's thumbnail, the row menus), the
/// header, footer and panel menu are framework buttons, fields and menus, and the colour mode and gradient type are
/// `Select`s. The ramp of the Gradient panel, its thumbnail and the channel track faces are app-defined components.
#[cfg(not(target_arch = "wasm32"))]
mod framework {
    use super::{
        ColorMode, GRAD_SEL, GradientDrag, PICKED, RampDrag, RampSel, RowMarks, RowMode, SwatchDrag, cmyk_ramp, color_json, color32, live_drag,
        paint_color_type, paint_mode, paint_not_editable, picker_fields, ramp_release, row_marks, set_stop, set_stop_color, set_tint, stops_json,
        thumbnail,
    };
    use crate::framework_transform::Artwork;
    use crate::i18n::tr;
    use crate::theme::Tokens;
    use crate::{DesignApp, icons};
    use craft_behavior::{Axis, DisplayOrder, EditEvent, ItemSource, Metrics, NumberCodec, PartKey, PointerButton, PressPhase, Selection, Sense};
    use craft_color_ui::ui::{ColorPopover, ColorRamp, PickedColor, RampKind, RampPick};
    use craft_theme::Srgba;
    use craft_ui_widgets::buttons::{Button, IconButton, ToggleButton};
    use craft_ui_widgets::contract::{
        ActivateOn, Bind, CollectionEvent, Common, Component, Cx, DropKind, DropOperation, DropPlace, Entry, Input, ListDrag, MeasureCx, NoValue,
        Outcome, PaintUi, PartsCx, Pointer as _, PopupKind, Props as _, Remember as _, Role, SelectMode, Show as _, ShowCollection as _,
        ShowContainer as _, Sizing, Trigger,
    };
    use craft_ui_widgets::display::{Divider, Icon, Shade, Text};
    use craft_ui_widgets::layout_components::Row;
    use craft_ui_widgets::list::{List, ListRow};
    use craft_ui_widgets::menu::{Menu, MenuItem};
    use craft_ui_widgets::pickers::Select;
    use craft_ui_widgets::primitives::{ContentPainterCx, Glyph, IconSpec, PaintShape, StrokeWidth};
    use craft_ui_widgets::runtime::drag::{DragPreview, DropMark, DropTarget};
    use craft_ui_widgets::runtime::env::{Env, Label, UnitKind};
    use craft_ui_widgets::runtime::focus::KeyUse;
    use craft_ui_widgets::style::{ComponentStyle, DataColor, StyleColor, StyleCx, UiTokens};
    use craft_ui_widgets::text::NumberField;
    use craft_ui_widgets::value_drag::{Slider, SliderThumb, TrackFace};
    use designcraft_color::{ColorType, SwatchValue};
    use egui::{Color32, CornerRadius, Pos2, Rect, Ui, Vec2, pos2, vec2};
    use serde_json::{Value, json};
    use std::borrow::Cow;

    /// The footer's three actions, drawn with the app's own artwork.
    #[derive(Clone, Copy, craft_ui::contract::MemoProps)]
    enum FooterAction {
        Group,
        New,
        Delete,
    }

    impl FooterAction {
        fn key(self) -> &'static str {
            match self {
                Self::Group => "new-group",
                Self::New => "new-swatch",
                Self::Delete => "delete-swatch",
            }
        }
    }

    impl Glyph for FooterAction {
        fn paint(&self, painter: &egui::Painter, _tokens: &UiTokens, rect: Rect, color: Color32, _mirrored: bool) {
            match self {
                Self::Group => folder(painter, rect, color),
                Self::New => icons::paint(painter, rect, "plus", color),
                Self::Delete => icons::paint(painter, rect, "trash", color),
            }
        }
    }

    /// The folder the colour groups and New Color Group show: the existing artwork, not an icon provider's.
    fn folder(painter: &egui::Painter, rect: Rect, color: Color32) {
        let f = Rect::from_center_size(rect.center(), vec2(14.0, 10.0));
        painter.rect_stroke(f, 1.5, egui::Stroke::new(1.2, color), egui::StrokeKind::Inside);
        painter.line_segment([f.left_top() + vec2(1.0, 3.0), f.right_top() + vec2(-1.0, 3.0)], egui::Stroke::new(1.0, color));
    }

    /// A colour group's folder.
    static FOLDER: FooterAction = FooterAction::Group;

    /// The marks at the end of a swatch row, as the native columns draw them; `Blank` keeps an empty column's room.
    #[derive(Clone, Copy, craft_ui::contract::MemoProps)]
    enum Mark {
        NotEditable,
        Process,
        Spot,
        Cmyk,
        Rgb,
        Gray,
        NoColor,
        Registration,
        Blank,
    }

    impl Glyph for Mark {
        fn paint(&self, painter: &egui::Painter, _tokens: &UiTokens, rect: Rect, color: Color32, _mirrored: bool) {
            match self {
                Mark::NotEditable => paint_not_editable(painter, rect, color),
                Mark::Process => paint_color_type(painter, rect, ColorType::Process, color),
                Mark::Spot => paint_color_type(painter, rect, ColorType::Spot, color),
                Mark::Cmyk => paint_mode(painter, rect, RowMode::Cmyk, color),
                Mark::Rgb => paint_mode(painter, rect, RowMode::Rgb, color),
                Mark::Gray => paint_mode(painter, rect, RowMode::Gray, color),
                Mark::NoColor => paint_mode(painter, rect, RowMode::None, color),
                Mark::Registration => paint_mode(painter, rect, RowMode::Registration, color),
                Mark::Blank => {}
            }
        }
    }

    static NOT_EDITABLE: Mark = Mark::NotEditable;
    static PROCESS: Mark = Mark::Process;
    static SPOT: Mark = Mark::Spot;
    static CMYK: Mark = Mark::Cmyk;
    static RGB: Mark = Mark::Rgb;
    static GRAY: Mark = Mark::Gray;
    static NO_COLOR: Mark = Mark::NoColor;
    static REGISTRATION: Mark = Mark::Registration;
    static BLANK: Mark = Mark::Blank;

    /// The Swatches panel's Fill/Stroke proxy: the stroke as a frame of its colour, the one in front drawn last.
    #[derive(craft_ui::contract::MemoProps)]
    struct Proxy {
        fill: Option<Color32>,
        fill_gradient: Option<(Color32, Color32)>,
        stroke: Option<Color32>,
        stroke_gradient: Option<(Color32, Color32)>,
        stroke_front: bool,
    }

    impl Glyph for Proxy {
        fn paint(&self, painter: &egui::Painter, _tokens: &UiTokens, rect: Rect, _color: Color32, _mirrored: bool) {
            let t = Tokens::get(painter.ctx());
            // The native proxy's 27-point drawing, scaled to the button's icon.
            let k = rect.width().min(rect.height()) / 27.0;
            let at = |x: f32, y: f32| Rect::from_min_size(rect.min + vec2(x, y) * k, Vec2::splat(16.0 * k));
            let (fill_r, stroke_r) = (at(1.0, 2.0), at(10.0, 10.0));
            let paint = |stroke: bool| {
                if stroke {
                    crate::widgets::paint_chip(painter, stroke_r, self.stroke, self.stroke_gradient);
                    painter.rect_filled(stroke_r.shrink(4.0 * k), 0.0, t.panel);
                    painter.rect_stroke(stroke_r.shrink(4.0 * k), 0.0, egui::Stroke::new(1.0, t.text_dim), egui::StrokeKind::Outside);
                    painter.rect_stroke(stroke_r, 0.0, egui::Stroke::new(1.0, t.text_dim), egui::StrokeKind::Outside);
                } else {
                    crate::widgets::paint_chip(painter, fill_r, self.fill, self.fill_gradient);
                    painter.rect_stroke(fill_r, 0.0, egui::Stroke::new(1.0, t.text_dim), egui::StrokeKind::Outside);
                }
            };
            paint(!self.stroke_front);
            paint(self.stroke_front);
        }
    }

    /// A Color panel proxy chip: the swatch's colour or gradient, ringed in the accent while it is in front.
    #[derive(craft_ui::contract::MemoProps)]
    struct ColorChip {
        color: Option<Color32>,
        gradient: Option<(Color32, Color32)>,
        front: bool,
    }

    impl Glyph for ColorChip {
        fn paint(&self, painter: &egui::Painter, _tokens: &UiTokens, rect: Rect, _color: Color32, _mirrored: bool) {
            crate::widgets::paint_chip(painter, rect.shrink(2.0), self.color, self.gradient);
            if self.front {
                painter.rect_stroke(rect, 2.0, egui::Stroke::new(1.5, Tokens::get(painter.ctx()).accent), egui::StrokeKind::Inside);
            }
        }
    }

    // ---------------------------------------------------------------------------------------------------------------
    // Swatches panel
    // ---------------------------------------------------------------------------------------------------------------

    /// A row of the swatch list: a swatch (top level, or in the colour group `group`) or a colour group's folder.
    #[derive(Clone, Debug, PartialEq, Eq, Hash)]
    enum RowKey {
        Swatch { group: Option<String>, name: String },
        Group(String),
    }

    impl RowKey {
        fn swatch(&self) -> Option<&str> {
            match self {
                RowKey::Swatch { name, .. } => Some(name),
                RowKey::Group(_) => None,
            }
        }

        /// The row's depth in the tree: a colour group's swatches stand under its folder.
        fn depth(&self) -> usize {
            match self {
                RowKey::Swatch { group: Some(_), .. } => 1,
                RowKey::Swatch { group: None, .. } | RowKey::Group(_) => 0,
            }
        }
    }

    /// What a row shows.
    enum RowKind {
        Swatch { chip: (Option<Color32>, Option<(Color32, Color32)>), marks: RowMarks, note: String },
        Group { open: bool },
    }

    struct RowData {
        key: RowKey,
        kind: RowKind,
    }

    /// The rows in display order: the swatches outside colour groups, then each group and, while it is open, its
    /// swatches.
    struct Rows(Vec<RowData>);

    impl DisplayOrder<RowKey> for Rows {
        fn len(&self) -> usize {
            self.0.len()
        }

        fn key(&self, at: usize) -> RowKey {
            self.0[at].key.clone()
        }

        fn index_of(&self, key: &RowKey) -> Option<usize> {
            self.0.iter().position(|row| row.key == *key)
        }

        fn depth(&self, at: usize) -> usize {
            self.0.get(at).map_or(0, |row| row.key.depth())
        }

        fn has_children(&self, at: usize) -> bool {
            self.0.get(at).is_some_and(|row| matches!(row.kind, RowKind::Group { .. }))
        }

        fn open(&self, at: usize) -> bool {
            self.0.get(at).is_some_and(|row| matches!(row.kind, RowKind::Group { open: true }))
        }

        fn text(&self, at: usize) -> Option<Cow<'_, str>> {
            self.0.get(at).map(|row| match &row.key {
                RowKey::Swatch { name, .. } => Cow::Borrowed(name.as_str()),
                RowKey::Group(name) => Cow::Borrowed(name.as_str()),
            })
        }
    }

    impl ItemSource<RowKey> for Rows {}

    impl Rows {
        /// The first row of the swatch `name`.
        fn of_swatch(&self, name: &str) -> Option<RowKey> {
            self.0.iter().find(|row| row.key.swatch() == Some(name)).map(|row| row.key.clone())
        }
    }

    /// The temp-data key of a colour group's open state; the native list keeps the same.
    fn open_id(group: &str) -> egui::Id {
        egui::Id::unique(("color_group_open", group))
    }

    fn rows(ui: &Ui, lang: &str, doc: &designcraft_doc::Document) -> Rows {
        let grouped: std::collections::HashSet<&str> = doc.color_groups.iter().flat_map(|g| g.swatches.iter().map(String::as_str)).collect();
        let mut by_name: std::collections::HashMap<&str, &designcraft_color::swatch::Swatch> = std::collections::HashMap::new();
        for w in &doc.swatches {
            by_name.entry(w.name.as_str()).or_insert(w);
        }
        let swatch = |sw: &designcraft_color::swatch::Swatch, group: Option<&str>| {
            let note = match &sw.value {
                SwatchValue::Gradient { .. } => tr(lang, "Gradient").to_owned(),
                SwatchValue::Tint { tint, .. } => format!("{:.0}%", tint * 100.0),
                _ => String::new(),
            };
            RowData {
                key: RowKey::Swatch { group: group.map(str::to_owned), name: sw.name.clone() },
                kind: RowKind::Swatch { chip: crate::widgets::swatch_colors(doc, &sw.name, 1.0), marks: row_marks(sw), note },
            }
        };
        let mut out: Vec<RowData> =
            doc.swatches.iter().filter(|w| !w.hidden && !grouped.contains(w.name.as_str())).map(|sw| swatch(sw, None)).collect();
        for g in &doc.color_groups {
            let open = ui.data(|d| d.get_temp::<bool>(open_id(&g.name))).unwrap_or(true);
            out.push(RowData { key: RowKey::Group(g.name.clone()), kind: RowKind::Group { open } });
            if open {
                out.extend(g.swatches.iter().filter_map(|n| by_name.get(n.as_str()).copied()).map(|sw| swatch(sw, Some(&g.name))));
            }
        }
        Rows(out)
    }

    /// The marks at the end of a swatch row, outermost first: the colour mode, the colour type and whether it can be
    /// edited. A row with any mark keeps every column, so the marks stand in line from row to row.
    fn with_marks<'r>(mut row: ListRow<'r>, lang: &str, marks: RowMarks) -> ListRow<'r> {
        if marks.mode.is_none() && marks.kind.is_none() && !marks.not_editable {
            return row;
        }
        let blank = || Icon::new(IconSpec::glyph(&BLANK));
        row = match marks.mode {
            Some(RowMode::Cmyk) => row.trailing_icon(IconSpec::glyph(&CMYK), tr(lang, "CMYK").to_owned()),
            Some(RowMode::Rgb) => row.trailing_icon(IconSpec::glyph(&RGB), tr(lang, "RGB").to_owned()),
            Some(RowMode::Gray) => row.trailing_icon(IconSpec::glyph(&GRAY), tr(lang, "Gray").to_owned()),
            Some(RowMode::None) => row.trailing_icon(IconSpec::glyph(&NO_COLOR), tr(lang, "None").to_owned()),
            Some(RowMode::Registration) => row.trailing_icon(IconSpec::glyph(&REGISTRATION), tr(lang, "Registration").to_owned()),
            None => row.trailing_space(blank()),
        };
        row = match marks.kind {
            Some(ColorType::Process) => row.trailing_icon(IconSpec::glyph(&PROCESS), tr(lang, "Process").to_owned()),
            Some(ColorType::Spot) => row.trailing_icon(IconSpec::glyph(&SPOT), tr(lang, "Spot").to_owned()),
            None => row.trailing_space(blank()),
        };
        if marks.not_editable {
            row.trailing_icon(IconSpec::glyph(&NOT_EDITABLE), tr(lang, "Cannot be edited").to_owned())
        } else {
            row.trailing_space(blank())
        }
    }

    /// What a row's menu or the panel menu asks for, run after the menus are shown.
    enum Action {
        Run(&'static str, Value),
        ApplyStroke(String),
        ColorTheme,
    }

    fn run_actions(app: &mut DesignApp, actions: Vec<Action>) {
        for action in actions {
            match action {
                Action::Run(id, params) => {
                    let _ = app.run(id, params);
                }
                Action::ApplyStroke(name) => {
                    let _ = crate::panels::apply_swatch(app, true, &name, None);
                }
                Action::ColorTheme => crate::menus::activate(app, "swatch.addColorTheme", &Value::Null),
            }
        }
    }

    /// The Swatches panel: the proxy, Formatting Affects and Tint row, the swatch list and the footer.
    pub(super) fn show(app: &mut DesignApp, ui: &mut Ui) {
        let Some(st) = app.session.active() else { return };
        let doc = st.doc.clone();
        let identity = (st.uid, st.identity_epoch);
        let lang = app.ui.language.clone();
        let target = crate::panels::color_target_or_defaults(app);
        header(app, ui, &doc, target.as_ref());
        let fill_cur = target.map(|(fill, _, stroke, _)| if app.ui.stroke_front { stroke } else { fill });
        let rows = rows(ui, &lang, &doc);
        let picked: Option<String> = ui.data(|d| d.get_temp(egui::Id::unique(PICKED)));
        let mut selection = picked.as_deref().and_then(|name| rows.of_swatch(name)).map_or_else(Selection::default, Selection::single);
        let before = selection.clone();
        let renamable = |key: &RowKey| key.swatch().and_then(|name| doc.swatch(name)).is_some_and(|sw| !sw.locked);
        let list_id = ui.id().with(("swatches-list", identity));
        let out = List::<RowKey>::new(tr(&lang, "Swatches").to_owned())
            .id(list_id)
            .select(SelectMode::Single)
            .activate(ActivateOn::Click)
            .tree(true)
            .natural_height(true)
            .sizing(Sizing::FillWidth)
            .rename(true)
            .slow_click_rename(true)
            .can_rename(renamable)
            .current(fill_cur.as_deref().and_then(|name| rows.of_swatch(name)))
            .drag_payload(|drag: &ListDrag<RowKey>| drag.grabbed.swatch().map(|name| SwatchDrag(name.to_owned())))
            .takes_drop(DropKind::new::<GradientDrag>(DropPlace::Onto, |_, _| true))
            .takes_drop(DropKind::new::<GradientDrag>(DropPlace::Between, |_, _| true))
            .selection(Bind::bound(&mut selection))
            .show_items(ui, &rows, &mut |_, icx| {
                let Some(data) = rows.0.get(icx.at) else { return ListRow::spacer() };
                let name = match &data.key {
                    RowKey::Swatch { name, .. } | RowKey::Group(name) => name.clone(),
                };
                let row = ListRow::new(name).depth(data.key.depth()).state(icx.row.clone());
                match &data.kind {
                    RowKind::Swatch { chip: (color, gradient), marks, note } => {
                        let (color, gradient) = (*color, *gradient);
                        let row = row.lead_painter(move |cx: &ContentPainterCx<'_>, slot: Rect| {
                            let well = Rect::from_center_size(slot.center(), Vec2::splat(slot.width().min(slot.height())));
                            crate::widgets::paint_chip(&cx.painter, well, color, gradient);
                        });
                        let row = if note.is_empty() { row } else { row.trailing(note.clone()) };
                        with_marks(row, &lang, *marks)
                    }
                    RowKind::Group { open } => row.group(*open).icon(IconSpec::glyph(&FOLDER)),
                }
            });
        let menu_id = list_id.with("row-menu");
        let mut actions = Vec::new();
        // The renames the pass committed, and the groups it folded: a rename commits only where its row is still there.
        let (mut renames, mut folded) = (Vec::new(), Vec::new());
        for event in &out.events {
            match event {
                CollectionEvent::Activate(RowKey::Group(name)) => {
                    // A double click's first click folded the group: it stays as it was.
                    if out.activation_clicks >= 2 {
                        actions.push(Action::Run("app.colorGroupOptionsDialog", json!({"name": name})));
                    } else {
                        let open = ui.data(|d| d.get_temp::<bool>(open_id(name))).unwrap_or(true);
                        ui.data_mut(|d| d.insert_temp(open_id(name), !open));
                        if open {
                            folded.push(name.as_str());
                        }
                    }
                }
                CollectionEvent::Activate(RowKey::Swatch { name, .. }) => {
                    if out.activation_clicks >= 2 {
                        // Double-click: Swatch Options (the first click has applied the swatch).
                        actions.push(Action::Run("app.swatchOptionsDialog", json!({"name": name})));
                    } else {
                        ui.data_mut(|d| d.insert_temp(egui::Id::unique(PICKED), name.clone()));
                        let stroke = app.ui.stroke_front;
                        let _ = crate::panels::apply_swatch(app, stroke, name, None);
                    }
                }
                CollectionEvent::Open(change) => {
                    let groups: Vec<&str> = if change.all { doc.color_groups.iter().map(|g| g.name.as_str()).collect() } else { Vec::new() };
                    if let RowKey::Group(name) = &change.key {
                        ui.data_mut(|d| d.insert_temp(open_id(name), change.open));
                    }
                    for name in groups {
                        ui.data_mut(|d| d.insert_temp(open_id(name), change.open));
                    }
                }
                CollectionEvent::Rename(EditEvent::Commit { after, .. }) => renames.push(after),
                CollectionEvent::ContextMenu(request) => {
                    let key = request.key.clone();
                    ui.data_mut(|d| d.insert_temp(menu_id, key));
                }
                CollectionEvent::Dropped { payload, .. } => {
                    // An unnamed gradient becomes "New Gradient Swatch"; one that is a swatch already stays one.
                    if let Some(g) = payload.get::<GradientDrag>()
                        && doc.swatch(&g.0).is_some_and(|w| w.hidden)
                    {
                        actions.push(Action::Run("swatch.new", json!({"swatch": g.0})));
                    }
                }
                _ => {}
            }
        }
        for after in renames {
            let Some(name) = after.key.swatch() else { continue };
            // A name field that was retired (the panel disabled or being measured, the swatch gone, renamed outside, hidden
            // or locked, or its folder folded away) applies nothing.
            let alive = ui.is_enabled()
                && !ui.is_sizing_pass()
                && doc.swatch(name).is_some_and(|sw| !sw.locked && !sw.hidden)
                && !matches!(&after.key, RowKey::Swatch { group: Some(group), .. } if folded.contains(&group.as_str()));
            if alive {
                rename(app, ui, Some(name), &after.name);
            }
        }
        // A pick by the keys (the arrows move the pick) is the swatch Delete Swatch removes; a pick that ended on a
        // folder leaves the swatch picked before.
        if selection != before
            && let Some(name) = selection.items.first().and_then(RowKey::swatch)
        {
            ui.data_mut(|d| d.insert_temp(egui::Id::unique(PICKED), name.to_owned()));
        }
        if let Some(key) = ui.data(|d| d.get_temp::<RowKey>(menu_id)) {
            let shown = Menu::context(tr(&lang, "Swatches").to_owned(), &out.response)
                .id(menu_id.with("menu"))
                .show(ui, |ui, _| row_menu(ui, &lang, &doc, &key));
            actions.extend(shown.inner.flatten());
            if shown.output.closed.is_some() {
                ui.data_mut(|d| d.remove::<RowKey>(menu_id));
            }
        }
        run_actions(app, actions);
        ui.add_space(6.0);
        footer(app, ui, &doc, identity, fill_cur.as_deref());
    }

    /// A rename in place committed `to` for the swatch `from`: blank or unchanged text renames nothing; a refusal (a
    /// name in use) goes to the status bar.
    fn rename(app: &mut DesignApp, ui: &Ui, from: Option<&str>, to: &str) {
        let Some(from) = from else { return };
        let to = to.trim();
        if to.is_empty() || to == from {
            return;
        }
        match app.run("swatch.options", json!({"name": from, "to": to})) {
            // Still picked under its new name, unless the click that ended the rename picked another swatch.
            Ok(_) => ui.data_mut(|d| {
                if d.get_temp::<String>(egui::Id::unique(PICKED)).is_none_or(|p| p == from) {
                    d.insert_temp(egui::Id::unique(PICKED), to.to_owned());
                }
            }),
            Err(e) => app.status_error("Swatch Options", e),
        }
    }

    /// A row's menu: a swatch's (options, apply to the stroke, its colour group, delete) or a colour group's.
    fn row_menu(ui: &mut Ui, lang: &str, doc: &designcraft_doc::Document, key: &RowKey) -> Option<Action> {
        let item = |ui: &mut Ui, label: &str| MenuItem::new(tr(lang, label).to_owned()).id_salt(label.to_owned()).show(ui).ran;
        match key {
            RowKey::Group(name) => {
                if item(ui, "Color Group Options…") {
                    return Some(Action::Run("app.colorGroupOptionsDialog", json!({"name": name})));
                }
                if item(ui, "Ungroup Color Group") {
                    return Some(Action::Run("swatch.ungroupColorGroup", json!({"name": name})));
                }
                None
            }
            RowKey::Swatch { name, .. } => {
                let mut picked = None;
                if item(ui, "Swatch Options…") {
                    picked = Some(Action::Run("app.swatchOptionsDialog", json!({"name": name})));
                }
                if item(ui, "Apply to Stroke") {
                    picked = Some(Action::ApplyStroke(name.clone()));
                }
                if doc.swatch(name).is_some_and(|sw| !sw.locked) {
                    let _ = Menu::submenu_row(ui, tr(lang, "Move to Color Group").to_owned(), |ui, _| {
                        if MenuItem::new(tr(lang, "(Top Level)").to_owned()).id_salt("top-level").show(ui).ran {
                            picked = Some(Action::Run("swatch.moveToGroup", json!({"swatches": [name], "group": null})));
                        }
                        for g in &doc.color_groups {
                            if MenuItem::new(g.name.clone()).id_salt(("group", g.name.clone())).show(ui).ran {
                                picked = Some(Action::Run("swatch.moveToGroup", json!({"swatches": [name], "group": g.name})));
                            }
                        }
                    });
                    if item(ui, "New Color Group with Swatch") {
                        picked = Some(Action::Run("swatch.newColorGroup", json!({"swatches": [name]})));
                    }
                    Divider::horizontal().show(ui);
                    if item(ui, "Delete Swatch") {
                        picked = Some(Action::Run("swatch.delete", json!({"name": name})));
                    }
                }
                picked
            }
        }
    }

    /// The top row, as InDesign's: a Fill/Stroke proxy (a click brings the other to the front), Formatting Affects
    /// Container / Text, and the tint of the colour in front.
    fn header(app: &mut DesignApp, ui: &mut Ui, doc: &designcraft_doc::Document, target: Option<&(String, f32, String, f32)>) {
        let lang = app.ui.language.clone();
        let (fill_sw, stroke_sw) = target.map_or(("[None]", "[Black]"), |(f, _, s, _)| (f.as_str(), s.as_str()));
        let text = crate::panels::colors_affect_text(app) || app.ui.formatting_affects_text;
        let stroke_front = app.ui.stroke_front;
        // With nothing selected InDesign's field reads 100.
        let tint = target.map_or(Some(100.0), |(_, ft, _, st)| Some((f64::from(if stroke_front { *st } else { *ft }) * 100.0).round()));
        let (fill, fill_gradient) = crate::widgets::swatch_colors(doc, fill_sw, 1.0);
        let (stroke, stroke_gradient) = crate::widgets::swatch_colors(doc, stroke_sw, 1.0);
        let proxy = Proxy { fill, fill_gradient, stroke, stroke_gradient, stroke_front };
        ui.add_space(3.0);
        let shown = Row::new().id_salt("swatches-header").show(ui, |ui, _| {
            let tip = tr(&lang, if stroke_front { "Stroke" } else { "Fill" }).to_owned();
            let toggle = IconButton::new(IconSpec::glyph(&proxy)).id_salt("swatches-proxy").label(tip.clone()).tooltip(tip).show(ui).clicked();
            let mut affects = None;
            for (k, (icon, tip)) in
                [("format-container", "Formatting affects container (J)"), ("format-text", "Formatting affects text (J)")].into_iter().enumerate()
            {
                let artwork = Artwork(icon);
                let out = ToggleButton::new(tr(&lang, tip).to_owned(), Bind::given(text == (k == 1)))
                    .icon(IconSpec::glyph(&artwork))
                    .icon_only()
                    .tooltip(tr(&lang, tip).to_owned())
                    .id_salt(("swatches-affects", k))
                    .show(ui);
                if out.events.committed().is_some() {
                    affects = Some(k == 1);
                }
            }
            Text::new(tr(&lang, "Tint:").to_owned()).show(ui);
            let tinted = NumberField::new(tr(&lang, "Tint").to_owned(), Bind::given(tint))
                .id_salt("swatches_tint")
                .unit_kind(UnitKind::Percent)
                .range(0.0..=100.0)
                .fixed_decimals(0)
                .show(ui)
                .edit
                .events
                .committed()
                .copied()
                .flatten();
            (toggle, affects, tinted)
        });
        let Some((toggle, affects, tinted)) = shown.inner else { return };
        if toggle {
            let _ = app.run("color.toggleFillStroke", json!({}));
        }
        if let Some(on) = affects {
            let _ = app.run("app.formattingAffectsText", json!({"on": on}));
        }
        if let Some(v) = tinted.filter(|v| v.is_finite()) {
            let _ = set_tint(app, v);
        }
    }

    /// InDesign's bottom row: the panel menu, New Color Group, New Swatch and Delete Swatch (the one last picked).
    fn footer(app: &mut DesignApp, ui: &mut Ui, doc: &designcraft_doc::Document, identity: (u64, u64), fill_cur: Option<&str>) {
        let lang = app.ui.language.clone();
        let picked: Option<String> = ui.data(|d| d.get_temp(egui::Id::unique(PICKED))).filter(|n: &String| doc.swatch(n).is_some_and(|w| !w.locked));
        let front = if app.ui.stroke_front { "stroke" } else { "fill" };
        let button = |ui: &mut Ui, action: FooterAction, target: Option<&str>, label: &str, enabled: bool| {
            IconButton::new(IconSpec::glyph(&action))
                .id_salt(("swatches-footer", identity, action.key(), target.map(str::to_owned)))
                .label(tr(&lang, label).to_owned())
                .tooltip(tr(&lang, label).to_owned())
                .enabled(enabled)
                .show(ui)
                .clicked()
        };
        let mut actions = Vec::new();
        let mut deleted = false;
        ui.horizontal(|ui| {
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if button(ui, FooterAction::Delete, picked.as_deref(), "Delete Swatch", picked.is_some()) {
                    deleted = true;
                }
                // The selection's fill or stroke (whichever is in front) becomes a swatch; with nothing selected, a
                // new colour.
                if button(ui, FooterAction::New, Some(front), "New Swatch", true) {
                    actions.push(Action::Run("swatch.new", json!({"target": front})));
                }
                if button(ui, FooterAction::Group, None, "New Color Group", true) {
                    actions.push(Action::Run("swatch.newColorGroup", json!({})));
                }
                let current = fill_cur.filter(|n| doc.swatch(n).is_some_and(|w| !w.hidden)).map(str::to_owned);
                actions.extend(panel_menu(ui, &mut app.session, &lang, identity, current));
            });
        });
        if deleted
            && let Some(name) = &picked
            && app.run("swatch.delete", json!({"name": name})).is_ok()
        {
            ui.data_mut(|d| d.remove::<String>(egui::Id::unique(PICKED)));
        }
        run_actions(app, actions);
    }

    /// The panel menu: Swatch Options of the swatch in front, Color Theme, the Ink Manager, New Color Group and the
    /// swatch files.
    fn panel_menu(ui: &mut Ui, session: &mut designcraft_engine::Session, lang: &str, identity: (u64, u64), current: Option<String>) -> Vec<Action> {
        let opener = IconButton::new(IconSpec::glyph(&Artwork("menu")))
            .id_salt(("swatches-menu-opener", identity))
            .label(tr(lang, "Swatches").to_owned())
            .tooltip(tr(lang, "Swatches").to_owned())
            .popup(PopupKind::Menu)
            .show(ui);
        let shown =
            Menu::new(tr(lang, "Swatches").to_owned()).id_salt(("swatches-menu", identity)).opened_by(&opener, Trigger::Click).show(ui, |ui, _| {
                let mut picked = Vec::new();
                let item = |ui: &mut Ui, label: &str, enabled: bool| {
                    MenuItem::new(tr(lang, label).to_owned()).id_salt(label.to_owned()).enabled(enabled).show(ui).ran
                };
                if item(ui, "Swatch Options…", current.is_some()) {
                    picked.push(Action::Run("app.swatchOptionsDialog", json!({"name": current})));
                }
                if item(ui, "Color Theme…", true) {
                    picked.push(Action::ColorTheme);
                }
                let _ = Menu::submenu_row(ui, tr(lang, "Ink Manager").to_owned(), |ui, _| {
                    let Ok(l) = session.execute("ink.list", &json!({})) else { return };
                    let all = l["allToProcess"].as_bool().unwrap_or(false);
                    if MenuItem::new(tr(lang, "All Spots to Process").to_owned()).id_salt("all-to-process").check(all).show(ui).ran {
                        picked.push(Action::Run("ink.options", json!({"allToProcess": !all})));
                    }
                    Divider::horizontal().show(ui);
                    let inks = l["inks"].as_array().cloned().unwrap_or_default();
                    if inks.is_empty() {
                        let _ = MenuItem::new(tr(lang, "No spot inks").to_owned()).id_salt("no-inks").enabled(false).show(ui);
                    }
                    for ink in inks {
                        let name = ink["name"].as_str().unwrap_or("").to_string();
                        let process = ink["process"].as_bool().unwrap_or(false);
                        if MenuItem::new(format!("{name} → process")).id_salt(("ink", name.clone())).check(process).enabled(!all).show(ui).ran {
                            picked.push(Action::Run("ink.options", json!({"ink": name, "toProcess": !process})));
                        }
                    }
                });
                if item(ui, "New Color Group", true) {
                    picked.push(Action::Run("swatch.newColorGroup", json!({})));
                }
                if item(ui, "Load Swatches…", true) {
                    picked.push(Action::Run("app.loadSwatches", json!({})));
                }
                if item(ui, "Save Swatches for Exchange…", true) {
                    picked.push(Action::Run("app.saveSwatches", json!({})));
                }
                picked
            });
        shown.inner.unwrap_or_default()
    }

    // ---------------------------------------------------------------------------------------------------------------
    // Color panel
    // ---------------------------------------------------------------------------------------------------------------

    /// The look of a channel's track face: the line round its edge and its own size when nothing sets it.
    #[derive(Clone, Debug, PartialEq)]
    pub(super) struct ChannelRampStyle {
        edge: StyleColor,
        length: f32,
        thickness: f32,
    }

    impl ComponentStyle for ChannelRampStyle {
        fn metrics(&self) -> Metrics {
            Metrics::new()
        }
    }

    /// The colours of one channel from its lowest value to its highest, the face of the channel's slider track.
    #[derive(Clone, craft_ui_widgets::contract::Component)]
    #[component(content_painter)]
    pub(super) struct ChannelRamp {
        #[common]
        common: Common,
        stops: Vec<Color32>,
    }

    impl ChannelRamp {
        fn new(stops: Vec<Color32>) -> Self {
            ChannelRamp { common: Common::new(), stops }
        }
    }

    impl Component for ChannelRamp {
        const ROLE: Role = Role::IMAGE;
        const PARTS: bool = false;
        const STOPS: bool = false;
        const OVERLAYS: bool = false;
        type Style = ChannelRampStyle;
        type Output = egui::Response;

        fn style(cx: &StyleCx<'_>, _: ChannelRampStyleKey) -> ChannelRampStyle {
            ChannelRampStyle {
                edge: cx.color(cx.tokens.color.line.control_edge),
                length: cx.tokens.size.length.track,
                thickness: cx.tokens.size.ramp.bar,
            }
        }

        fn sense(&self) -> Sense {
            Sense::empty()
        }

        fn measure(&mut self, cx: &MeasureCx<'_, Self>) -> Vec2 {
            Vec2::new(cx.style.length, cx.style.thickness)
        }

        fn paint(&mut self, ui: &mut PaintUi<'_>, cx: &Cx<'_, Self>) {
            let rect = cx.rect;
            let last = self.stops.len().saturating_sub(1).max(1) as f32;
            let stops: Vec<(f32, DataColor)> = self.stops.iter().enumerate().map(|(i, c)| (i as f32 / last, DataColor::new(*c))).collect();
            let painter = ui.content_painter(cx);
            painter.ramp(rect, CornerRadius::ZERO, &stops, Axis::X);
            ui.draw().stroke(&PaintShape::Rect(rect, CornerRadius::ZERO), StrokeWidth::Thin, cx.style.edge);
        }

        fn finish(self, cx: Cx<'_, Self>) -> egui::Response {
            cx.response.clone()
        }
    }

    /// One channel: its name, a slider over the channel's colours with the pointer under the track, and its number,
    /// 0 to `scale`. The value it committed this pass (0 to 1): a slider applies on release, the field on a commit.
    fn channel(ui: &mut Ui, id: (&str, usize), label: &str, v: f32, scale: f32, percent: bool, ramp: impl Fn(f32) -> Color32) -> Option<f32> {
        let n = 24;
        let stops: Vec<Color32> = (0..=n).map(|i| ramp(i as f32 / n as f32)).collect();
        let value = f64::from((v * scale).round());
        let shown = Row::new().id_salt(("channel", id)).show(ui, |ui, _| {
            Text::new(label.to_owned()).show(ui);
            let slid = Slider::new(Bind::given(value), 0.0..=f64::from(scale))
                .step(1.0)
                .thumb(SliderThumb::Triangle)
                .track_face(TrackFace::new(ChannelRamp::new(stops.clone())))
                .label(label.to_owned())
                .id_salt(("channel-slider", id))
                .show(ui)
                .events
                .committed()
                .copied();
            let field = NumberField::new(label.to_owned(), Bind::given(Some(value)))
                .id_salt(("channel-field", id))
                .range(0.0..=f64::from(scale))
                .fixed_decimals(0);
            let field = if percent { field.unit_kind(UnitKind::Percent) } else { field };
            let typed = field.show(ui).edit.events.committed().copied().flatten();
            slid.or(typed)
        });
        shown.inner.flatten().filter(|v| v.is_finite()).map(|v| (v.clamp(0.0, f64::from(scale)) as f32) / scale)
    }

    fn rgb_ramp(k: usize) -> impl Fn(f32) -> Color32 {
        move |x| {
            let mut p = [0.0; 3];
            p[k] = x;
            Color32::from_rgb((p[0] * 255.0) as u8, (p[1] * 255.0) as u8, (p[2] * 255.0) as u8)
        }
    }

    /// The channels of `c` in `mode`: the colour they committed, as `object.color`'s model form and as a colour.
    fn channels(ui: &mut Ui, salt: &str, mode: ColorMode, c: designcraft_color::Color) -> Option<(Value, designcraft_color::Color)> {
        use designcraft_color::Color;
        match mode {
            ColorMode::Cmyk => {
                let mut v = c.to_cmyk();
                let mut done = false;
                for (k, label) in ["C", "M", "Y", "K"].iter().enumerate() {
                    if let Some(x) = channel(ui, (salt, k), label, v[k], 100.0, true, |x| cmyk_ramp(k, x)) {
                        v[k] = x;
                        done = true;
                    }
                }
                done.then(|| (json!({"model": "cmyk", "c": v[0], "m": v[1], "y": v[2], "k": v[3]}), Color::cmyk(v[0], v[1], v[2], v[3])))
            }
            ColorMode::Rgb => {
                let mut v = c.to_rgb();
                let mut done = false;
                for (k, label) in ["R", "G", "B"].iter().enumerate() {
                    if let Some(x) = channel(ui, (salt, k), label, v[k], 255.0, false, rgb_ramp(k)) {
                        v[k] = x;
                        done = true;
                    }
                }
                done.then(|| (json!({"model": "rgb", "r": v[0], "g": v[1], "b": v[2]}), Color::rgb(v[0], v[1], v[2])))
            }
            ColorMode::Lab => {
                let lab = c.to_lab();
                Text::new(format!("L {:.0}   a {:.0}   b {:.0}", lab.l, lab.a, lab.b)).shade(Shade::Muted).show(ui);
                None
            }
        }
    }

    /// The spectrum ramp, hue across and light to dark down; the colour picked this pass (RGB, 0 to 1, in 1/255
    /// steps).
    fn spectrum_pick(ui: &mut Ui, lang: &str, salt: &str) -> Option<[f32; 3]> {
        let out =
            ColorRamp::new(tr(lang, "Color spectrum").to_owned(), RampKind::Spectrum, Bind::given(RampPick::new(Srgba::rgb(0.0, 0.0, 0.0), false)))
                .boxes(false)
                .id_salt(("color-spectrum", salt.to_owned()))
                .sizing(Sizing::FillWidth)
                .show(ui);
        let pick = out.edit.events.committed()?;
        Some([pick.color.r, pick.color.g, pick.color.b].map(|v| (v.clamp(0.0, 1.0) * 255.0).round() / 255.0))
    }

    /// The Color panel: the fill and stroke chips (a click brings one to the front, a double click opens the Color
    /// Picker), the colour mode, the tint of a named swatch, the channels, the spectrum and Add to Swatches.
    pub(super) fn color_panel(app: &mut DesignApp, ui: &mut Ui) {
        let target = crate::panels::color_target_or_defaults(app);
        let lang = app.ui.language.clone();
        let Some((fill, fill_tint, stroke_sw, stroke_tint)) = target else {
            Text::new(tr(&lang, "Select an object or text to color its fill or stroke.").to_owned()).shade(Shade::Muted).show(ui);
            return;
        };
        let text = crate::panels::colors_affect_text(app);
        let Some(doc) = app.session.active().map(|d| d.doc.clone()) else { return };
        let mode_id = egui::Id::unique("color_panel_mode");
        let default_mode = if doc.settings.intent == designcraft_doc::Intent::Print { ColorMode::Cmyk } else { ColorMode::Rgb };
        let mut mode: ColorMode = ui.data(|d| d.get_temp(mode_id)).unwrap_or(default_mode);
        let mut stroke = app.ui.stroke_front;
        let name = if stroke { stroke_sw.clone() } else { fill.clone() };
        let shown = Row::new().id_salt("color-panel-proxy").show(ui, |ui, _| {
            let mut picked: Option<(bool, bool)> = None;
            for (k, label) in [(false, "Fill"), (true, "Stroke")] {
                let n = if k { &stroke_sw } else { &fill };
                let (color, gradient) = crate::widgets::swatch_colors(&doc, n, 1.0);
                let chip = ColorChip { color, gradient, front: k == stroke };
                let tip = tr(&lang, "{label}: {name} (double-click: Color Picker)").replace("{label}", tr(&lang, label)).replace("{name}", n);
                let out =
                    IconButton::new(IconSpec::glyph(&chip)).id_salt(("color-proxy", k)).label(tr(&lang, label).to_owned()).tooltip(tip).show(ui);
                if out.clicked() {
                    picked = Some((k, out.clicks >= 2));
                }
            }
            Text::new(name.clone()).shade(Shade::Muted).show(ui);
            let entries = [Entry::new(ColorMode::Lab, "Lab"), Entry::new(ColorMode::Cmyk, "CMYK"), Entry::new(ColorMode::Rgb, "RGB")];
            let chosen = Select::new(tr(&lang, "Color Mode").to_owned(), Bind::given(mode), &entries)
                .id_salt("color_mode")
                .show(ui)
                .events
                .committed()
                .copied();
            (picked, chosen)
        });
        let (picked, chosen) = shown.inner.unwrap_or((None, None));
        if let Some((k, double)) = picked {
            stroke = k;
            if double {
                let n = if k { &stroke_sw } else { &fill };
                let mut fields = picker_fields(doc.resolve_color(n, 1.0), k);
                fields["text"] = json!(text);
                app.ui.dialog = Some(crate::dialogs::Dialog::new("colorPicker", fields));
            }
        }
        if let Some(m) = chosen {
            mode = m;
        }
        app.ui.stroke_front = stroke;
        ui.data_mut(|d| d.insert_temp(mode_id, mode));
        // The chip clicked this pass is the one edited from here on.
        let name = if stroke { stroke_sw.clone() } else { fill.clone() };
        let target = if stroke { "stroke" } else { "fill" };
        // A gradient: the stop selected in the Gradient panel, as InDesign's Color panel shows it.
        if !text && let Some(g) = designcraft_color::swatch::resolve_gradient(&doc.swatches, &name).cloned() {
            gradient_stop(app, ui, &g, target, mode);
            return;
        }
        let sw = doc.swatch(&name).cloned();
        let Some(c) = doc.resolve_color(&name, 1.0) else {
            Text::new(tr(&lang, "[None] — choose a colour on the ramp.").to_owned()).shade(Shade::Muted).show(ui);
            spectrum(app, ui, target);
            return;
        };
        // A named swatch shows its tint; mixing its values makes an unnamed colour.
        if let Some(sw) = sw.as_ref().filter(|w| !w.hidden && !w.locked) {
            let cur = f64::from(if stroke { stroke_tint } else { fill_tint }) * 100.0;
            let shown = Row::new().id_salt("color-tint").show(ui, |ui, _| {
                Text::new("T").show(ui);
                // The slider applies on release; the field applies a typed value, clamped, on a commit.
                let slid = Slider::new(Bind::given(cur.round()), 0.0..=100.0)
                    .step(1.0)
                    .label(tr(&lang, "Tint").to_owned())
                    .id_salt("color_tint_slider")
                    .show(ui)
                    .events
                    .committed()
                    .copied();
                let typed = NumberField::new(tr(&lang, "Tint").to_owned(), Bind::given(Some(cur)))
                    .id_salt("color_tint")
                    .unit_kind(UnitKind::Percent)
                    .range(0.0..=100.0)
                    .fixed_decimals(0)
                    .show(ui)
                    .edit
                    .events
                    .committed()
                    .copied()
                    .flatten();
                slid.or(typed)
            });
            if let Some(v) = shown.inner.flatten().filter(|v| v.is_finite()) {
                let _ = crate::panels::apply_swatch(app, stroke, &sw.name, Some((v.clamp(0.0, 100.0) / 100.0) as f32));
            }
        }
        if let Some((color, _)) = channels(ui, "color", mode, c) {
            let _ = app.run("object.color", json!({"color": color, "target": target, "text": text}));
        }
        spectrum(app, ui, target);
        let unnamed = sw.as_ref().is_some_and(|w| w.hidden);
        if Button::new(tr(&lang, "Add to Swatches").to_owned()).id_salt("add-to-swatches").enabled(unnamed).show(ui).clicked() {
            let _ = app.run("swatch.addToSwatches", json!({"swatch": name, "target": target}));
        }
    }

    /// The spectrum: a pick applies an unnamed colour.
    fn spectrum(app: &mut DesignApp, ui: &mut Ui, target: &str) {
        let lang = app.ui.language.clone();
        if let Some([r, g, b]) = spectrum_pick(ui, &lang, "color") {
            let text = crate::panels::colors_affect_text(app);
            let _ = app.run("object.color", json!({"color": {"model": "rgb", "r": r, "g": g, "b": b}, "target": target, "text": text}));
        }
    }

    /// The Color panel on a gradient fill (or stroke): the stop selected in the Gradient panel (the stop before a
    /// selected midpoint), in the panel's colour mode. Its fields and the spectrum colour that stop.
    fn gradient_stop(app: &mut DesignApp, ui: &mut Ui, g: &designcraft_color::Gradient, target: &str, mode: ColorMode) {
        use designcraft_color::Color;
        let lang = app.ui.language.clone();
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
        Text::new(values).shade(Shade::Muted).show(ui);
        let mut new = channels(ui, "stop", mode, c).map(|(_, c)| c);
        if let Some([r, gr, b]) = spectrum_pick(ui, &lang, "stop") {
            new = Some(Color::rgb(r, gr, b));
        }
        if let Some(c) = new {
            let mut stops = stops_json(g);
            set_stop_color(&mut stops, i, color_json(&c));
            let _ = app.run("object.gradient", json!({"stops": stops, "target": target}));
        }
    }

    // ---------------------------------------------------------------------------------------------------------------
    // Gradient panel
    // ---------------------------------------------------------------------------------------------------------------

    /// The pointer's part of the thumbnail and of the ramp.
    const AREA: PartKey = PartKey::new("area");

    /// The look of the thumbnail and the ramp: no surface of their own, the painting is theirs.
    #[derive(Clone, Debug, PartialEq)]
    pub(super) struct GradientPartStyle;

    impl ComponentStyle for GradientPartStyle {
        fn metrics(&self) -> Metrics {
            Metrics::new()
        }
    }

    /// What the thumbnail reports: its response and whether it was clicked.
    #[derive(Debug)]
    pub(super) struct ThumbOutput {
        response: egui::Response,
        clicked: bool,
    }

    impl Outcome for ThumbOutput {
        type Value = NoValue;

        fn response(&self) -> &egui::Response {
            &self.response
        }
    }

    /// The Gradient panel's thumbnail: the gradient in a square. A click applies the gradient shown (InDesign's way to
    /// start one); a drag carries the selection's gradient (`swatch`) to the Swatches panel, which keeps it as a
    /// swatch.
    #[derive(craft_ui_widgets::contract::Component, craft_ui_widgets::contract::Props)]
    #[component(content_painter)]
    pub(super) struct GradientThumb {
        #[common]
        common: Common,
        gradient: designcraft_color::Gradient,
        swatch: Option<String>,
        clicked: bool,
    }

    impl GradientThumb {
        fn new(label: impl Into<Label>, gradient: designcraft_color::Gradient, swatch: Option<String>) -> Self {
            GradientThumb { common: Common::new(), gradient, swatch, clicked: false }.label(label)
        }
    }

    impl Component for GradientThumb {
        const ROLE: Role = Role::BUTTON;
        const OVERLAYS: bool = false;
        const KEYS: KeyUse = KeyUse::of(Input::Pointer);
        type Style = GradientPartStyle;
        type Output = ThumbOutput;

        fn style(_: &StyleCx<'_>, _: GradientThumbStyleKey) -> GradientPartStyle {
            GradientPartStyle
        }

        fn measure(&mut self, _: &MeasureCx<'_, Self>) -> Vec2 {
            vec2(34.0, 34.0)
        }

        fn sense(&self) -> Sense {
            Sense::HOVER
        }

        fn parts(&mut self, rect: Rect, cx: &mut PartsCx<'_, Self>) {
            cx.part(AREA, rect).sense(Sense::CLICK | Sense::DRAG);
        }

        fn update(&mut self, cx: &mut Cx<'_, Self>) {
            match cx.part(AREA).press.phase {
                PressPhase::DragStart { .. } => {
                    if let Some(swatch) = self.swatch.clone() {
                        cx.start_drag(GradientDrag(swatch), DragPreview::frame(vec2(18.0, 18.0), None));
                    }
                }
                PressPhase::Release { start, click: true, .. } if start.button == PointerButton::Primary => self.clicked = true,
                _ => {}
            }
        }

        fn paint(&mut self, ui: &mut PaintUi<'_>, cx: &Cx<'_, Self>) {
            let content = ui.content_painter(cx);
            let t = Tokens::get(content.painter.ctx());
            thumbnail(&content.painter, cx.rect.shrink(1.0), &self.gradient, t.field_border);
        }

        fn finish(self, cx: Cx<'_, Self>) -> ThumbOutput {
            ThumbOutput { response: cx.response.clone(), clicked: self.clicked }
        }
    }

    /// What the pointer did on the ramp this pass.
    #[derive(Clone, Copy, Debug)]
    enum RampEdit {
        /// A marker was clicked or began a drag.
        Select(RampSel),
        /// A dragged marker was let go at an offset (0 to 1), off the panel for a stop that is removed.
        Release(RampSel, f32, bool),
        /// A click below the ramp, away from the stops: a new stop there.
        Add(f32),
        /// A secondary click on a stop: its colour.
        StopColor(usize),
    }

    /// The ramp's drag kept between passes.
    #[derive(Clone, Default)]
    struct RampHeld {
        drag: Option<RampDrag>,
    }

    /// Where the ramp's parts are: the whole area and the colour bar in it.
    #[derive(Clone, Copy)]
    struct RampGeometry {
        area: Rect,
        ramp: Rect,
    }

    impl RampGeometry {
        fn new(area: Rect) -> Self {
            RampGeometry { area, ramp: Rect::from_min_size(area.min + vec2(6.0, 10.0), vec2((area.width() - 12.0).max(1.0), 14.0)) }
        }

        fn x_of(&self, o: f32) -> f32 {
            self.ramp.left() + self.ramp.width() * o
        }

        fn o_of(&self, x: f32) -> f32 {
            ((x - self.ramp.left()) / self.ramp.width()).clamp(0.0, 1.0)
        }

        fn mid_x(&self, g: &designcraft_color::Gradient, i: usize) -> Option<f32> {
            match (g.stops.get(i), g.stops.get(i + 1)) {
                (Some(a), Some(b)) => Some(self.x_of(a.offset + (b.offset - a.offset) * a.midpoint)),
                _ => None,
            }
        }

        /// The marker under the pointer: a stop below the ramp, a diamond above it.
        fn hit(&self, g: &designcraft_color::Gradient, p: Pos2) -> Option<RampSel> {
            let last = g.stops.len().saturating_sub(1);
            if p.y < self.ramp.top() {
                (0..last)
                    .filter_map(|i| self.mid_x(g, i).map(|x| (i, (x - p.x).abs())))
                    .filter(|(_, d)| *d < 6.0)
                    .min_by(|a, b| a.1.total_cmp(&b.1))
                    .map(|(i, _)| RampSel::Mid(i))
            } else {
                g.stops
                    .iter()
                    .enumerate()
                    .map(|(i, s)| (i, (self.x_of(s.offset) - p.x).abs()))
                    .filter(|(_, d)| *d < 7.0)
                    .min_by(|a, b| a.1.total_cmp(&b.1))
                    .map(|(i, _)| RampSel::Stop(i))
            }
        }

        /// Where a swatch dropped at `p` goes: onto a stop (that stop), else a new stop at its offset.
        fn drop_at(&self, g: &designcraft_color::Gradient, p: Pos2) -> (Option<usize>, f32) {
            match self.hit(g, pos2(p.x, self.ramp.bottom() + 4.0)) {
                Some(RampSel::Stop(i)) => match g.stops.get(i) {
                    Some(s) => (Some(i), s.offset),
                    None => (None, self.o_of(p.x)),
                },
                _ => (None, self.o_of(p.x)),
            }
        }

        /// A drag of `h` at the pointer `p`: its offset, and whether a stop is off the panel.
        fn follow(&self, g: &designcraft_color::Gradient, h: RampSel, p: Pos2) -> RampDrag {
            match h {
                RampSel::Stop(_) => (h, self.o_of(p.x), p.y > self.area.bottom() + 16.0 && g.stops.len() > 2),
                RampSel::Mid(i) => {
                    let m = match (g.stops.get(i), g.stops.get(i + 1)) {
                        (Some(a), Some(b)) if b.offset - a.offset > 1e-6 => ((self.o_of(p.x) - a.offset) / (b.offset - a.offset)).clamp(0.13, 0.87),
                        _ => 0.5,
                    };
                    (h, m, false)
                }
            }
        }
    }

    /// The drop target of swatches dragged from the Swatches panel onto the ramp.
    struct RampDrop<'g> {
        geometry: RampGeometry,
        gradient: &'g designcraft_color::Gradient,
    }

    impl DropTarget<SwatchDrag> for RampDrop<'_> {
        type Drop = (SwatchDrag, Option<usize>, f32);

        fn accept(&self, payload: &SwatchDrag, at: Pos2, _: DropOperation) -> Option<Self::Drop> {
            let (stop, o) = self.geometry.drop_at(self.gradient, at);
            Some((payload.clone(), stop, o))
        }

        fn mark(&self, drop: &Self::Drop) -> DropMark {
            let x = self.geometry.x_of(drop.2);
            DropMark::line(pos2(x, self.geometry.ramp.top() - 3.0), pos2(x, self.geometry.ramp.bottom() + 14.0))
        }
    }

    /// What the ramp reports: its response, the pointer's edit and a swatch dropped on it (onto a stop, or at an
    /// offset).
    #[derive(Debug)]
    pub(super) struct RampOutput {
        response: egui::Response,
        edit: Option<RampEdit>,
        dropped: Option<(SwatchDrag, Option<usize>, f32)>,
    }

    impl Outcome for RampOutput {
        type Value = NoValue;

        fn response(&self) -> &egui::Response {
            &self.response
        }
    }

    /// The gradient's ramp, as InDesign's: midpoint diamonds above it and stops below it. Drag a stop or a diamond
    /// to move it, click below the ramp to add a stop, drag a stop off to remove it, drop a swatch on a stop to colour
    /// it or elsewhere to add a stop, and right-click a stop for its colour. It has no keyboard handling, as the native
    /// ramp has none.
    #[derive(craft_ui_widgets::contract::Component, craft_ui_widgets::contract::Props)]
    #[component(content_painter)]
    pub(super) struct GradientRampEditor {
        #[common]
        common: Common,
        gradient: designcraft_color::Gradient,
        sel: RampSel,
        edit: Option<RampEdit>,
        drag: Option<RampDrag>,
        dropped: Option<(SwatchDrag, Option<usize>, f32)>,
    }

    impl GradientRampEditor {
        fn new(label: impl Into<Label>, gradient: designcraft_color::Gradient, sel: RampSel) -> Self {
            GradientRampEditor { common: Common::new(), gradient, sel, edit: None, drag: None, dropped: None }.label(label)
        }
    }

    impl Component for GradientRampEditor {
        const ROLE: Role = Role::GROUP;
        const OVERLAYS: bool = false;
        const KEYS: KeyUse = KeyUse::of(Input::Pointer);
        type Style = GradientPartStyle;
        type Output = RampOutput;

        fn style(_: &StyleCx<'_>, _: GradientRampEditorStyleKey) -> GradientPartStyle {
            GradientPartStyle
        }

        fn measure(&mut self, _: &MeasureCx<'_, Self>) -> Vec2 {
            vec2(120.0, 40.0)
        }

        fn sense(&self) -> Sense {
            Sense::HOVER
        }

        fn parts(&mut self, rect: Rect, cx: &mut PartsCx<'_, Self>) {
            cx.part(AREA, rect).sense(Sense::CLICK | Sense::DRAG | Sense::CONTEXT_CLICK);
        }

        fn update(&mut self, cx: &mut Cx<'_, Self>) {
            let geo = RampGeometry::new(cx.rect);
            let g = &self.gradient;
            let mut held = cx.memory::<RampHeld>().clone();
            // A drag kept from a longer gradient (the selection or its gradient changed since) is dropped.
            held.drag = live_drag(held.drag, g.stops.len());
            match cx.part(AREA).press.phase {
                PressPhase::DragStart { start, pos, .. } if start.button == PointerButton::Primary => {
                    if let Some(h) = geo.hit(g, start.origin)
                        && let Some(o) = match h {
                            RampSel::Stop(i) => g.stops.get(i).map(|s| s.offset),
                            RampSel::Mid(i) => g.stops.get(i).map(|s| s.midpoint),
                        }
                    {
                        self.edit = Some(RampEdit::Select(h));
                        held.drag = Some((h, o, false));
                    }
                    if let Some((h, _, _)) = held.drag {
                        held.drag = Some(geo.follow(g, h, pos));
                    }
                }
                PressPhase::Drag { pos, .. } => {
                    if let Some((h, _, _)) = held.drag {
                        held.drag = Some(geo.follow(g, h, pos));
                    }
                }
                PressPhase::Release { start, click, .. } => {
                    if let Some((h, o, off)) = held.drag.take() {
                        self.edit = Some(RampEdit::Release(h, o, off));
                    } else if click && start.button == PointerButton::Primary {
                        self.edit = match geo.hit(g, start.origin) {
                            Some(h) => Some(RampEdit::Select(h)),
                            None if start.origin.y > geo.ramp.bottom() => Some(RampEdit::Add(geo.o_of(start.origin.x))),
                            None => None,
                        };
                    } else if click
                        && start.button == PointerButton::Secondary
                        && let Some(RampSel::Stop(i)) = geo.hit(g, start.origin)
                    {
                        self.edit = Some(RampEdit::StopColor(i));
                    }
                }
                PressPhase::Cancel { .. } => held.drag = None,
                _ => {}
            }
            let target = RampDrop { geometry: geo, gradient: g };
            self.dropped = cx.drop_target::<SwatchDrag, _>(&target);
            self.drag = held.drag;
            *cx.memory::<RampHeld>() = held;
        }

        fn paint(&mut self, ui: &mut PaintUi<'_>, cx: &Cx<'_, Self>) {
            let content = ui.content_painter(cx);
            paint_ramp_editor(&content.painter, RampGeometry::new(cx.rect), &self.gradient, self.sel, self.drag);
        }

        fn finish(self, cx: Cx<'_, Self>) -> RampOutput {
            RampOutput { response: cx.response.clone(), edit: self.edit, dropped: self.dropped }
        }
    }

    /// The ramp in bands with its edge, the diamonds and the stops; a dragged marker at its drag's place, a stop
    /// dragged off not at all.
    fn paint_ramp_editor(painter: &egui::Painter, geo: RampGeometry, g: &designcraft_color::Gradient, sel: RampSel, dragging: Option<RampDrag>) {
        let t = Tokens::get(painter.ctx());
        let ramp = geo.ramp;
        super::paint_ramp(painter, ramp, g);
        painter.rect_stroke(ramp, 0.0, egui::Stroke::new(1.0, t.field_border), egui::StrokeKind::Outside);
        let last = g.stops.len().saturating_sub(1);
        for i in 0..last {
            let x = match dragging {
                Some((RampSel::Mid(j), m, _)) if j == i => match (g.stops.get(i), g.stops.get(i + 1)) {
                    (Some(a), Some(b)) => geo.x_of(a.offset + (b.offset - a.offset) * m),
                    _ => ramp.left(),
                },
                _ => geo.mid_x(g, i).unwrap_or(ramp.left()),
            };
            let (cy, r) = (ramp.top() - 6.0, 4.0);
            let pts = vec![pos2(x, cy - r), pos2(x + r, cy), pos2(x, cy + r), pos2(x - r, cy)];
            let on = sel == RampSel::Mid(i);
            painter.add(egui::Shape::convex_polygon(pts, if on { t.text } else { Color32::TRANSPARENT }, egui::Stroke::new(1.0, t.text_dim)));
        }
        for (i, s) in g.stops.iter().enumerate() {
            let (o, off) = match dragging {
                Some((RampSel::Stop(j), o, off)) if j == i => (o, off),
                _ => (s.offset, false),
            };
            if off {
                continue;
            }
            let x = geo.x_of(o);
            let top = ramp.bottom() + 2.0;
            let on = sel == RampSel::Stop(i);
            let edge = if on { t.text_strong } else { t.text_dim };
            painter.add(egui::Shape::convex_polygon(
                vec![pos2(x, top), pos2(x + 5.0, top + 5.0), pos2(x - 5.0, top + 5.0)],
                if on { t.text_strong } else { t.panel },
                egui::Stroke::new(1.0, edge),
            ));
            let sq = Rect::from_min_size(pos2(x - 5.0, top + 5.0), vec2(10.0, 9.0));
            painter.rect_filled(sq, 0.0, color32(&s.color, 1.0));
            painter.rect_stroke(sq, 0.0, egui::Stroke::new(if on { 1.5 } else { 1.0 }, edge), egui::StrokeKind::Inside);
        }
    }

    /// The Gradient panel: thumbnail, Type, Location and Angle, Reverse, and the ramp. Until the fill is a gradient
    /// the ramp shows the default White → Black, and using it gives the fill a gradient.
    pub(super) fn gradient_panel(app: &mut DesignApp, ui: &mut Ui) {
        let lang = app.ui.language.clone();
        let Some(st) = app.session.active() else { return };
        let identity = (st.uid, st.identity_epoch);
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
        let mut sel: RampSel = ui.data(|d| d.get_temp(sel_id)).unwrap_or(RampSel::Stop(0));
        let last = g.stops.len().saturating_sub(1);
        sel = match sel {
            RampSel::Stop(i) => RampSel::Stop(i.min(last)),
            RampSel::Mid(i) if i < last => RampSel::Mid(i),
            RampSel::Mid(_) => RampSel::Stop(0),
        };
        let mut send: Option<Value> = None;
        let shown = Row::new().id_salt("gradient-panel-top").show(ui, |ui, _| {
            let mut send: Option<Value> = None;
            let thumb = GradientThumb::new(tr(&lang, "Gradient").to_owned(), g.clone(), is_gradient.then(|| fill.swatch.clone()))
                .id_salt(("gradient-thumb", identity))
                .enabled(has_item)
                .show(ui);
            if thumb.clicked && !is_gradient {
                send = Some(json!({}));
            }
            craft_ui_widgets::layout_components::Stack::new().id_salt("gradient-panel-fields").show(ui, |ui, _| {
                Row::new().id_salt("gradient-type").show(ui, |ui, _| {
                    Text::new(tr(&lang, "Type:").to_owned()).show(ui);
                    let entries = [Entry::new("linear", tr(&lang, "Linear").to_owned()), Entry::new("radial", tr(&lang, "Radial").to_owned())];
                    let kind: craft_behavior::Mixed<&'static str> = if !is_gradient {
                        craft_behavior::Mixed::Mixed
                    } else if radial {
                        craft_behavior::Mixed::Same("radial")
                    } else {
                        craft_behavior::Mixed::Same("linear")
                    };
                    if let Some(kind) = Select::new(tr(&lang, "Type").to_owned(), Bind::given(kind), &entries)
                        .id_salt("grad_type")
                        .enabled(has_item)
                        .show(ui)
                        .events
                        .committed()
                        .copied()
                    {
                        send = Some(json!({"kind": kind}));
                    }
                });
                Row::new().id_salt("gradient-location").show(ui, |ui, _| {
                    Text::new(tr(&lang, "Location:").to_owned()).show(ui);
                    let loc = match sel {
                        RampSel::Stop(i) => g.stops.get(i).map(|s| s.offset as f64 * 100.0),
                        RampSel::Mid(i) => g.stops.get(i).map(|s| s.midpoint as f64 * 100.0),
                    };
                    let units = Env::of(ui).units();
                    let codec = NumberCodec::new(units.reader(Some(UnitKind::Percent)), 3);
                    let typed = NumberField::new(tr(&lang, "Location").to_owned(), Bind::given(loc.filter(|_| is_gradient)))
                        .id_salt("grad_loc")
                        .unit_kind(UnitKind::Percent)
                        .codec(codec)
                        .enabled(has_item)
                        .show(ui)
                        .edit
                        .events
                        .committed()
                        .copied()
                        .flatten();
                    if let Some(v) = typed.filter(|v| v.is_finite()) {
                        let mut stops = stops_json(&g);
                        match sel {
                            RampSel::Stop(i) => set_stop(&mut stops, i, "location", json!(v.clamp(0.0, 100.0))),
                            RampSel::Mid(i) => set_stop(&mut stops, i, "midpoint", json!(v.clamp(13.0, 87.0))),
                        }
                        send = Some(json!({"stops": stops}));
                    }
                    Text::new(tr(&lang, "Angle:").to_owned()).show(ui);
                    let angle =
                        fill.gradient_vector.map(|[x0, y0, x1, y1]| (-(y1 - y0)).atan2(x1 - x0).to_degrees()).or(fill.gradient_angle).unwrap_or(0.0);
                    let angled = NumberField::new(tr(&lang, "Angle").to_owned(), Bind::given(is_gradient.then_some(angle)))
                        .id_salt("grad_angle")
                        .unit_kind(UnitKind::Degrees)
                        .enabled(has_item && is_gradient && !radial)
                        .show(ui)
                        .edit
                        .events
                        .committed()
                        .copied()
                        .flatten();
                    if let Some(v) = angled.filter(|v| v.is_finite()) {
                        send = Some(json!({"angle": v}));
                    }
                });
                Row::new().id_salt("gradient-reverse").show(ui, |ui, _| {
                    Text::new(tr(&lang, "Reverse").to_owned()).show(ui);
                    let tip = tr(&lang, "Reverse the gradient").to_owned();
                    if IconButton::new(IconSpec::glyph(&Artwork("swap")))
                        .id_salt("grad_reverse")
                        .label(tip.clone())
                        .tooltip(tip)
                        .enabled(has_item)
                        .show(ui)
                        .clicked()
                        && is_gradient
                    {
                        send = Some(json!({"reverse": true}));
                    }
                });
            });
            send
        });
        if let Some(Some(v)) = shown.inner {
            send = Some(v);
        }
        ui.add_space(6.0);
        let ramp = GradientRampEditor::new(tr(&lang, "Gradient ramp").to_owned(), g.clone(), sel)
            .id_salt(("gradient-ramp", identity))
            .enabled(has_item)
            .sizing(Sizing::FillWidth)
            .show(ui);
        let color_id = egui::Id::unique("grad_stop_color");
        let mut color_open: Option<usize> = ui.data(|d| d.get_temp(color_id));
        match ramp.edit {
            Some(RampEdit::Select(h)) => sel = h,
            Some(RampEdit::Release(h, o, off)) => {
                let (stops, kept) = ramp_release(&g, h, o, off);
                if let Some(kept) = kept {
                    sel = kept;
                }
                send = Some(json!({"stops": stops}));
            }
            Some(RampEdit::Add(o)) => {
                let (c, a) = g.sample(o);
                let mut stops = stops_json(&g);
                stops.push(json!({"location": o as f64 * 100.0, "color": color_json(&c), "opacity": a as f64 * 100.0, "midpoint": 50.0}));
                send = Some(json!({"stops": stops}));
                sel = RampSel::Stop(g.stops.iter().filter(|s| s.offset <= o).count());
            }
            // A stop's colour (InDesign takes it from the Swatches or Color panel), for a gradient only.
            Some(RampEdit::StopColor(i)) => {
                sel = RampSel::Stop(i);
                if is_gradient {
                    color_open = Some(i);
                }
            }
            None => {}
        }
        // A swatch dropped from the Swatches panel: on a stop it colours that stop, elsewhere on the ramp it's a new
        // stop there.
        if let Some((sw, at, o)) = &ramp.dropped {
            let mut stops = stops_json(&g);
            match at {
                Some(i) => {
                    if let Some(s) = stops.get_mut(*i).and_then(Value::as_object_mut) {
                        s.insert("swatch".to_string(), json!(sw.0));
                        s.remove("color");
                    }
                    sel = RampSel::Stop(*i);
                }
                None => {
                    stops.push(json!({"location": *o as f64 * 100.0, "swatch": sw.0, "midpoint": 50.0}));
                    sel = RampSel::Stop(g.stops.iter().filter(|s| s.offset <= *o).count());
                }
            }
            send = Some(json!({"stops": stops}));
        }
        if let Some(i) = color_open.filter(|i| is_gradient && *i < g.stops.len()) {
            let [r, gg, bb] = g.stops[i].color.to_rgb();
            let mut open = true;
            let out = ColorPopover::new(tr(&lang, "Stop Color:").to_owned(), Bind::given(PickedColor::rgb(Srgba::rgb(r, gg, bb))))
                .anchor(&ramp.response, Bind::bound(&mut open))
                .show(ui);
            if let Some(picked) = out.events.committed() {
                let rgb = [picked.color.r, picked.color.g, picked.color.b].map(|v| (v.clamp(0.0, 1.0) * 255.0).round() as u8);
                let mut stops = stops_json(&g);
                set_stop_color(&mut stops, i, json!(format!("#{:02x}{:02x}{:02x}", rgb[0], rgb[1], rgb[2])));
                send = Some(json!({"stops": stops}));
            }
            color_open = (open && out.open).then_some(i);
        } else {
            color_open = None;
        }
        ui.data_mut(|d| {
            d.insert_temp(sel_id, sel);
            match color_open {
                Some(i) => {
                    d.insert_temp(color_id, i);
                }
                None => {
                    d.remove::<usize>(color_id);
                }
            }
        });
        if let Some(mut p) = send.filter(|_| has_item) {
            if let Some(o) = p.as_object_mut() {
                o.insert("target".to_string(), json!(if stroke_front { "stroke" } else { "fill" }));
            }
            let _ = app.run("object.gradient", p);
        }
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
        // The thumbnail: the framework's button named Gradient, else (native) the square left of the Type row.
        let from = match h.query_all_by_role_and_label(egui::accesskit::Role::Button, "Gradient").next() {
            Some(thumb) => thumb.rect().center(),
            None => {
                let ty = h.get_by_label("Type:").rect();
                egui::pos2(ty.min.x - 21.0, ty.min.y + 12.0)
            }
        };
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

#[cfg(all(test, not(target_arch = "wasm32")))]
mod footer_action_tests {
    use super::*;
    use craft_ui_widgets::runtime::inspect::{Element, Recorded};
    use egui::{Event, Key, Modifiers, PointerButton};
    use egui_kittest::Harness;
    use egui_kittest::kittest::Queryable;
    use serde_json::Value;
    use std::sync::Arc;

    struct State {
        app: DesignApp,
        recorded: Option<Arc<Recorded>>,
    }

    fn app_with_swatches() -> DesignApp {
        let mut app = DesignApp::new(designcraft_engine::Session::new(), crate::Services::default());
        app.run("file.new", json!({})).unwrap();
        app.run("frame.create", json!({"rect": [72, 72, 300, 300], "content": "unassigned"})).unwrap();
        app.run("swatch.create", json!({"name": "Port Fill", "color": "#108080"})).unwrap();
        app.run("swatch.create", json!({"name": "Port Stroke", "color": "#c03060"})).unwrap();
        app
    }

    fn harness(mut app: DesignApp, installed: bool) -> Harness<'static, State> {
        let ctx = egui::Context::default();
        crate::theme::install_fonts(&ctx, "");
        if installed {
            app.framework_opacity = true;
            app.install_framework(&ctx);
            assert!(app.framework_installed);
        }
        app.raw_input_hook(&ctx, &mut egui::RawInput::default());
        let mut h = Harness::builder().with_context(ctx).with_size(vec2(360.0, 900.0)).build_ui_state(
            |ui, state: &mut State| {
                let region = ui.scope_id();
                let (_, recorded) = craft_ui_widgets::runtime::inspect::record(ui, region, |ui| show(&mut state.app, ui));
                if recorded.is_some() {
                    state.recorded = recorded;
                }
            },
            State { app, recorded: None },
        );
        h.input_mut().max_texture_side = Some(8192);
        steps(&mut h, 3);
        h
    }

    fn steps(h: &mut Harness<'_, State>, count: usize) {
        for _ in 0..count {
            let mut raw = std::mem::take(h.input_mut());
            let ctx = h.ctx.clone();
            h.state_mut().app.raw_input_hook(&ctx, &mut raw);
            *h.input_mut() = raw;
            h.step();
        }
    }

    fn event(h: &mut Harness<'_, State>, event: Event) {
        h.input_mut().events.push(event);
        steps(h, 1);
    }

    fn key(h: &mut Harness<'_, State>, key: Key) {
        for pressed in [true, false] {
            event(h, Event::Key { key, physical_key: None, pressed, repeat: false, modifiers: Modifiers::NONE });
        }
        steps(h, 2);
    }

    fn click_at(h: &mut Harness<'_, State>, pos: egui::Pos2) {
        event(h, Event::PointerMoved(pos));
        for pressed in [true, false] {
            event(h, Event::PointerButton { pos, button: PointerButton::Primary, pressed, modifiers: Modifiers::NONE });
        }
        steps(h, 2);
    }

    fn toolbar(h: &Harness<'_, State>, label: &str) -> Element {
        let name = crate::i18n::tr(&h.state().app.ui.language, label);
        let found: Vec<_> = h
            .state()
            .recorded
            .as_ref()
            .unwrap()
            .elements
            .iter()
            .filter(|element| element.component == "IconButton" && element.facts.as_ref().is_some_and(|facts| facts.name.as_deref() == Some(name)))
            .cloned()
            .collect();
        assert_eq!(found.len(), 1, "one actual compact IconButton named {name}");
        found[0].clone()
    }

    fn click_toolbar(h: &mut Harness<'_, State>, label: &str) {
        let target = toolbar(h, label);
        let rect = h.state().recorded.as_ref().unwrap().hits.iter().find(|hit| hit.owner == target.id).map_or(target.rect, |hit| hit.rect);
        click_at(h, rect.center());
    }

    fn pick_row(h: &mut Harness<'_, State>, name: &str) {
        let rect = h.get_by(|node| node.label().is_some_and(|label| names_swatch(&label, name))).rect();
        // The chip is part of the real row; choosing it cannot start the name's slow-click rename.
        click_at(h, egui::pos2(rect.left() + 10.0, rect.center().y));
        assert_eq!(picked(h).as_deref(), Some(name));
    }

    fn picked(h: &Harness<'_, State>) -> Option<String> {
        h.ctx.data(|d| d.get_temp(egui::Id::unique(PICKED)))
    }

    fn tab_to(h: &mut Harness<'_, State>, label: &str) {
        let name = crate::i18n::tr(&h.state().app.ui.language, label).to_owned();
        let native = h.state().recorded.as_ref().unwrap().elements.iter().filter(|element| element.stop.is_some()).count();
        let fields = h.query_all_by(|node| matches!(node.role(), egui::accesskit::Role::TextInput | egui::accesskit::Role::ComboBox)).count();
        for _ in 0..=native + fields {
            if h.query_by(|node| node.role() == egui::accesskit::Role::Button && node.label().as_deref() == Some(name.as_str()))
                .is_some_and(|node| node.is_focused())
            {
                return;
            }
            key(h, Key::Tab);
        }
        panic!("natural Tab did not reach {name} in one current control cycle");
    }

    fn calls(h: &Harness<'_, State>, from: usize, command: &str) -> Vec<Value> {
        h.state().app.session.journal[from..].iter().filter(|(id, _)| id == command).map(|(_, params)| params.clone()).collect()
    }

    fn undo_len(h: &Harness<'_, State>) -> usize {
        h.state().app.session.active().unwrap().history.undo.len()
    }

    fn undo_once(h: &mut Harness<'_, State>) {
        h.state_mut().app.run("edit.undo", json!({})).unwrap();
        steps(h, 2);
    }

    #[test]
    fn installed_new_swatch_uses_real_row_fill_or_stroke_and_one_undo_without_replay() {
        let mut h = harness(app_with_swatches(), true);
        pick_row(&mut h, "Port Fill");
        let before = h.state().app.session.active().unwrap().doc.swatches.clone();
        let undo = undo_len(&h);
        let journal = h.state().app.session.journal.len();
        click_toolbar(&mut h, "New Swatch");
        let doc = &h.state().app.session.active().unwrap().doc;
        assert_eq!(doc.swatch("Port Fill copy").unwrap().value, doc.swatch("Port Fill").unwrap().value);
        assert_eq!(calls(&h, journal, "swatch.new"), [json!({"target": "fill"})]);
        assert_eq!(undo_len(&h), undo + 1);
        steps(&mut h, 3);
        assert_eq!(calls(&h, journal, "swatch.new").len(), 1);
        undo_once(&mut h);
        assert_eq!(h.state().app.session.active().unwrap().doc.swatches, before);
        let proxy = h.get_by_label("Fill").rect();
        click_at(&mut h, proxy.center());
        assert!(h.state().app.ui.stroke_front);
        pick_row(&mut h, "Port Stroke");
        let before = h.state().app.session.active().unwrap().doc.swatches.clone();
        let undo = undo_len(&h);
        let journal = h.state().app.session.journal.len();
        tab_to(&mut h, "New Swatch");
        key(&mut h, Key::Enter);
        let doc = &h.state().app.session.active().unwrap().doc;
        assert_eq!(doc.swatch("Port Stroke copy").unwrap().value, doc.swatch("Port Stroke").unwrap().value);
        assert_eq!(calls(&h, journal, "swatch.new"), [json!({"target": "stroke"})]);
        assert_eq!(undo_len(&h), undo + 1);
        undo_once(&mut h);
        assert_eq!(h.state().app.session.active().unwrap().doc.swatches, before);
    }

    #[test]
    fn installed_group_is_compact_localized_and_keyboard_creation_is_one_undo() {
        let mut app = app_with_swatches();
        app.ui.language = "de".into();
        let mut h = harness(app, true);
        let controls: Vec<_> = ["New Color Group", "New Swatch", "Delete Swatch"].iter().map(|label| toolbar(&h, label)).collect();
        for control in &controls {
            assert!(control.rect.width() <= control.rect.height() * 2.0, "localized accessible text is not painted as a wide toolbar label");
            assert!((control.rect.width() - controls[0].rect.width()).abs() < 1.0, "all three remain icon-sized controls");
        }
        let before = h.state().app.session.active().unwrap().doc.color_groups.clone();
        let swatches = h.state().app.session.active().unwrap().doc.swatches.clone();
        let undo = undo_len(&h);
        let journal = h.state().app.session.journal.len();
        tab_to(&mut h, "New Color Group");
        key(&mut h, Key::Space);
        assert_eq!(calls(&h, journal, "swatch.newColorGroup"), [json!({})]);
        assert_eq!(h.state().app.session.active().unwrap().doc.color_groups.len(), before.len() + 1);
        assert_eq!(h.state().app.session.active().unwrap().doc.swatches, swatches);
        assert_eq!(undo_len(&h), undo + 1);
        steps(&mut h, 3);
        assert_eq!(calls(&h, journal, "swatch.newColorGroup").len(), 1);
        undo_once(&mut h);
        assert_eq!(h.state().app.session.active().unwrap().doc.color_groups, before);
    }

    #[test]
    fn installed_delete_uses_the_picked_row_and_only_success_clears_it() {
        let mut h = harness(app_with_swatches(), true);
        assert!(toolbar(&h, "Delete Swatch").facts.as_ref().unwrap().disabled);
        let journal = h.state().app.session.journal.len();
        let undo = undo_len(&h);
        click_toolbar(&mut h, "Delete Swatch");
        assert!(calls(&h, journal, "swatch.delete").is_empty());
        assert_eq!(undo_len(&h), undo);
        assert!(picked(&h).is_none());
        pick_row(&mut h, "Port Stroke");
        let before = h.state().app.session.active().unwrap().doc.swatches.clone();
        let undo = undo_len(&h);
        let journal = h.state().app.session.journal.len();
        assert!(!toolbar(&h, "Delete Swatch").facts.as_ref().unwrap().disabled);
        tab_to(&mut h, "Delete Swatch");
        key(&mut h, Key::Space);
        assert_eq!(calls(&h, journal, "swatch.delete"), [json!({"name": "Port Stroke"})]);
        assert!(h.state().app.session.active().unwrap().doc.swatch("Port Stroke").is_none());
        assert!(h.state().app.session.active().unwrap().doc.swatch("Port Fill").is_some());
        assert!(picked(&h).is_none());
        assert!(toolbar(&h, "Delete Swatch").facts.as_ref().unwrap().disabled);
        assert_eq!(undo_len(&h), undo + 1);
        steps(&mut h, 3);
        assert_eq!(calls(&h, journal, "swatch.delete").len(), 1);
        undo_once(&mut h);
        assert_eq!(h.state().app.session.active().unwrap().doc.swatches, before);
        assert!(picked(&h).is_none(), "Undo restores the swatch, not a new Delete intent");
    }

    #[test]
    fn installed_locked_and_absent_picks_stay_gated_and_new_swatch_refusal_is_inert() {
        let mut h = harness(app_with_swatches(), true);
        pick_row(&mut h, "[Black]");
        let doc = h.state().app.session.active().unwrap().doc.clone();
        let undo = undo_len(&h);
        let journal = h.state().app.session.journal.len();
        assert!(toolbar(&h, "Delete Swatch").facts.as_ref().unwrap().disabled);
        click_toolbar(&mut h, "Delete Swatch");
        assert_eq!(picked(&h).as_deref(), Some("[Black]"));
        assert!(calls(&h, journal, "swatch.delete").is_empty());
        assert_eq!(undo_len(&h), undo);
        assert!(Arc::ptr_eq(&h.state().app.session.active().unwrap().doc, &doc));
        pick_row(&mut h, "[None]");
        let doc = h.state().app.session.active().unwrap().doc.clone();
        let undo = undo_len(&h);
        let status = h.state().app.ui.status.clone();
        click_toolbar(&mut h, "New Swatch");
        assert!(calls(&h, journal, "swatch.new").is_empty(), "the real engine refuses copying [None]");
        assert!(Arc::ptr_eq(&h.state().app.session.active().unwrap().doc, &doc));
        assert_eq!(undo_len(&h), undo);
        assert_ne!(h.state().app.ui.status, status, "the original app.run error still reaches the status bar");
        assert!(h.state().app.ui.status.contains("[None]"));
        assert_eq!(picked(&h).as_deref(), Some("[None]"));
        pick_row(&mut h, "Port Fill");
        h.state_mut().app.run("swatch.delete", json!({"name": "Port Fill"})).unwrap();
        steps(&mut h, 2);
        let journal = h.state().app.session.journal.len();
        let undo = undo_len(&h);
        assert!(toolbar(&h, "Delete Swatch").facts.as_ref().unwrap().disabled);
        click_toolbar(&mut h, "Delete Swatch");
        assert!(calls(&h, journal, "swatch.delete").is_empty());
        assert_eq!(undo_len(&h), undo);
        assert_eq!(picked(&h).as_deref(), Some("Port Fill"), "a gated absent pick is not a successful footer deletion");
    }

    #[test]
    fn installed_held_footer_actions_cannot_cross_documents_with_identically_named_swatches() {
        let mut app = app_with_swatches();
        app.run("file.new", json!({})).unwrap();
        app.run("frame.create", json!({"rect": [72, 72, 300, 300], "content": "unassigned"})).unwrap();
        app.run("swatch.create", json!({"name": "Port Fill", "color": "#cc0088"})).unwrap();
        app.session.set_active(0);
        let mut h = harness(app, true);
        pick_row(&mut h, "Port Fill");
        for (label, command) in [("Delete Swatch", "swatch.delete"), ("New Swatch", "swatch.new"), ("New Color Group", "swatch.newColorGroup")] {
            h.state_mut().app.session.set_active(0);
            steps(&mut h, 2);
            let journal = h.state().app.session.journal.len();
            let original = h.state().app.session.documents()[0].doc.clone();
            let target = toolbar(&h, label);
            let pos = target.rect.center();
            event(&mut h, Event::PointerMoved(pos));
            event(&mut h, Event::PointerButton { pos, button: PointerButton::Primary, pressed: true, modifiers: Modifiers::NONE });
            h.state_mut().app.session.set_active(1);
            steps(&mut h, 2);
            assert_ne!(toolbar(&h, label).id, target.id);
            let next = h.state().app.session.active().unwrap().doc.clone();
            let undo = undo_len(&h);
            event(&mut h, Event::PointerButton { pos, button: PointerButton::Primary, pressed: false, modifiers: Modifiers::NONE });
            steps(&mut h, 2);
            assert!(calls(&h, journal, command).is_empty(), "a previous document's press cannot invoke {command}");
            assert!(Arc::ptr_eq(&h.state().app.session.documents()[0].doc, &original));
            assert!(Arc::ptr_eq(&h.state().app.session.active().unwrap().doc, &next));
            assert_eq!(undo_len(&h), undo);
        }
        // The original global picked-name policy is retained: a fresh action in document two can
        // delete its same-named swatch, but must never mutate document one's swatch.
        let original = h.state().app.session.documents()[0].doc.clone();
        let before = h.state().app.session.active().unwrap().doc.swatches.clone();
        let journal = h.state().app.session.journal.len();
        click_toolbar(&mut h, "Delete Swatch");
        assert_eq!(calls(&h, journal, "swatch.delete"), [json!({"name": "Port Fill"})]);
        assert!(h.state().app.session.active().unwrap().doc.swatch("Port Fill").is_none());
        assert!(Arc::ptr_eq(&h.state().app.session.documents()[0].doc, &original));
        undo_once(&mut h);
        assert_eq!(h.state().app.session.active().unwrap().doc.swatches, before);
    }

    #[test]
    fn installed_delete_press_cannot_retarget_to_a_different_picked_swatch() {
        let mut h = harness(app_with_swatches(), true);
        pick_row(&mut h, "Port Fill");
        let target = toolbar(&h, "Delete Swatch");
        let pos = target.rect.center();
        event(&mut h, Event::PointerMoved(pos));
        event(&mut h, Event::PointerButton { pos, button: PointerButton::Primary, pressed: true, modifiers: Modifiers::NONE });
        // A legal external command changes the picked swatch's name while the real pointer is held.
        h.state_mut().app.run("swatch.options", json!({"name": "Port Fill", "to": "Renamed Fill"})).unwrap();
        steps(&mut h, 2);
        assert!(toolbar(&h, "Delete Swatch").facts.as_ref().unwrap().disabled);
        let journal = h.state().app.session.journal.len();
        let undo = undo_len(&h);
        event(&mut h, Event::PointerButton { pos, button: PointerButton::Primary, pressed: false, modifiers: Modifiers::NONE });
        steps(&mut h, 2);
        assert!(calls(&h, journal, "swatch.delete").is_empty());
        assert_eq!(undo_len(&h), undo);
        assert_eq!(picked(&h).as_deref(), Some("Port Fill"));
        assert!(h.state().app.session.active().unwrap().doc.swatch("Renamed Fill").is_some());
        assert!(h.state().app.session.active().unwrap().doc.swatch("Port Stroke").is_some());
    }

    #[test]
    fn uninstalled_footer_preserves_real_row_pick_and_existing_icon_delete() {
        let mut h = harness(app_with_swatches(), false);
        assert!(!h.state().app.framework_installed);
        pick_row(&mut h, "Port Fill");
        let journal = h.state().app.session.journal.len();
        let undo = undo_len(&h);
        let rect = h.get_by_label("Delete Swatch").rect();
        click_at(&mut h, rect.center());
        assert_eq!(calls(&h, journal, "swatch.delete"), [json!({"name": "Port Fill"})]);
        assert!(h.state().app.session.active().unwrap().doc.swatch("Port Fill").is_none());
        assert_eq!(undo_len(&h), undo + 1);
        assert!(picked(&h).is_none());
    }
}

#[cfg(all(test, not(target_arch = "wasm32")))]
mod ui223_swatches_rename_apply_tests {
    use super::*;
    use craft_ui_widgets::runtime::inspect::{Element, Recorded, record};
    use designcraft_doc::{Document, ItemId};
    use egui::{Event, Key, Modifiers, PointerButton, UiBuilder};
    use egui_kittest::{
        Harness,
        kittest::{NodeT, Queryable},
    };
    use serde_json::Value;
    use std::sync::Arc;

    struct Rig {
        app: DesignApp,
        controls: Option<Arc<Recorded>>,
        enabled: bool,
        sizing: bool,
        visible: bool,
        height: f32,
        viewport: egui::Rect,
        offset: egui::Vec2,
        outside: egui::Rect,
    }
    struct Snapshot {
        doc: Arc<Document>,
        saved: Arc<Document>,
        owner: (u64, u64),
        revisions: (u64, u64),
        selection: String,
        history: (usize, usize),
        journal: usize,
    }
    fn frame(h: &mut Harness<'_, Rig>, event: Option<Event>) {
        let mut raw = std::mem::take(h.input_mut());
        if let Some(event) = event {
            raw.events.push(event);
        }
        let ctx = h.ctx.clone();
        h.state_mut().app.raw_input_hook(&ctx, &mut raw);
        *h.input_mut() = raw;
        h.step();
    }
    fn idle(h: &mut Harness<'_, Rig>, count: usize) {
        for _ in 0..count {
            frame(h, None);
        }
    }
    fn key(h: &mut Harness<'_, Rig>, key: Key) {
        for pressed in [true, false] {
            frame(h, Some(Event::Key { key, physical_key: None, pressed, repeat: false, modifiers: Modifiers::NONE }));
        }
        idle(h, 2);
    }
    fn press(h: &mut Harness<'_, Rig>, at: egui::Pos2, pressed: bool) {
        frame(h, Some(Event::PointerButton { pos: at, button: PointerButton::Primary, pressed, modifiers: Modifiers::NONE }));
    }
    fn click(h: &mut Harness<'_, Rig>, at: egui::Pos2) {
        frame(h, Some(Event::PointerMoved(at)));
        press(h, at, true);
        press(h, at, false);
        idle(h, 2);
    }
    fn wait_click_interval(h: &mut Harness<'_, Rig>) {
        let seconds = h.ctx.options(|o| o.input_options.max_double_click_delay);
        idle(h, (seconds / 0.05).ceil() as usize + 2);
    }
    fn row(h: &Harness<'_, Rig>, name: &str) -> egui::Rect {
        // The framework's swatch list is a tree of rows; the native list's rows are buttons.
        let role = if h.state().app.framework_installed { egui::accesskit::Role::TreeItem } else { egui::accesskit::Role::Button };
        h.get_by(|node| {
            let access = node;
            access.role() == role && access.label().is_some_and(|label| names_swatch(&label, name))
        })
        .rect()
    }
    fn reveal(h: &mut Harness<'_, Rig>, name: &str) -> egui::Pos2 {
        let mut rect = row(h, name);
        if !h.state().viewport.contains(rect.center()) {
            let viewport = h.state().viewport;
            frame(h, Some(Event::PointerMoved(viewport.center())));
            let delta = vec2(0.0, viewport.center().y - rect.center().y);
            for phase in [egui::TouchPhase::Start, egui::TouchPhase::Move, egui::TouchPhase::End] {
                frame(
                    h,
                    Some(Event::MouseWheel {
                        unit: egui::MouseWheelUnit::Point,
                        delta: if phase == egui::TouchPhase::Move { delta } else { egui::Vec2::ZERO },
                        phase,
                        modifiers: Modifiers::NONE,
                        source: egui::MouseWheelSource::Wheel,
                    }),
                );
            }
            idle(h, 2);
            rect = row(h, name);
        }
        // The middle of the row is in the name's column whichever way the row reads; the ends hold the chip and the marks.
        let at = rect.center();
        assert!(h.state().viewport.contains(at), "the same real row must be physically visible");
        at
    }
    /// The framework list's rename field is named after the name it renames.
    fn editor_name(_: &Harness<'_, Rig>, source: &str) -> String {
        source.to_owned()
    }
    fn editor(h: &Harness<'_, Rig>, source: &str) -> Element {
        let name = editor_name(h, source);
        let fields: Vec<_> = h
            .state()
            .controls
            .as_ref()
            .unwrap()
            .elements
            .iter()
            .filter(|element| {
                element.component == "TextField" && element.facts.as_ref().is_some_and(|facts| facts.name.as_deref() == Some(name.as_str()))
            })
            .cloned()
            .collect();
        assert_eq!(fields.len(), 1, "the real installed name editor, not a header field");
        let field = fields[0].clone();
        assert!(h.state().viewport.contains_rect(field.rect));
        assert!(row(h, source).contains_rect(field.rect), "the original row's actual name allocation");
        field
    }
    /// The open rename: the name it renames and the text typed. Installed, the list's rename field (its name and value);
    /// native, the panel's own draft.
    fn draft(h: &Harness<'_, Rig>) -> Option<(String, String)> {
        if !h.state().app.framework_installed {
            return h.ctx.data(|data| data.get_temp(egui::Id::unique(RENAMING)));
        }
        let controls = h.state().controls.as_ref()?;
        let field = controls.elements.iter().find(|element| element.component == "TextField")?;
        let facts = field.facts.as_ref()?;
        Some((facts.name.clone().unwrap_or_default(), facts.value.clone().unwrap_or_default()))
    }
    fn open(h: &mut Harness<'_, Rig>, source: &str) {
        let picked = h.ctx.data(|data| data.get_temp::<String>(egui::Id::unique(PICKED)));
        if picked.as_deref() != Some(source) {
            let at = reveal(h, source);
            click(h, at);
            wait_click_interval(h);
        }
        let at = reveal(h, source);
        click(h, at);
        wait_click_interval(h);
        assert_eq!(draft(h).as_ref().map(|value| value.0.as_str()), Some(source));
        if h.state().app.framework_installed {
            editor(h, source);
        } else {
            let rect = row(h, source);
            assert!(h.query_all_by_role(egui::accesskit::Role::TextInput).any(|node| rect.contains(node.rect().center())));
        }
    }
    fn type_name(h: &mut Harness<'_, Rig>, name: &str) {
        frame(h, Some(Event::Text(name.to_owned())));
        idle(h, 2);
        assert_eq!(draft(h).unwrap().1, name, "normal one-time select-on-focus replaced the full original name");
    }
    fn snapshot(h: &Harness<'_, Rig>) -> Snapshot {
        let st = h.state().app.session.active().unwrap();
        Snapshot {
            doc: st.doc.clone(),
            saved: st.saved_doc.clone(),
            owner: (st.uid, st.identity_epoch),
            revisions: (st.revision, st.saved_revision),
            selection: format!("{:?}", st.selection),
            history: (st.history.undo.len(), st.history.redo.len()),
            journal: h.state().app.session.journal.len(),
        }
    }
    fn unchanged(h: &Harness<'_, Rig>, before: &Snapshot) {
        let now = snapshot(h);
        assert!(Arc::ptr_eq(&now.doc, &before.doc));
        assert!(Arc::ptr_eq(&now.saved, &before.saved));
        assert_eq!(
            (now.owner, now.revisions, now.selection, now.history, now.journal),
            (before.owner, before.revisions, before.selection.clone(), before.history, before.journal)
        );
    }
    fn renamed(before: &Document, from: &str, to: &str) -> Document {
        let mut expected = before.clone();
        let swatch = expected.swatches.iter_mut().find(|swatch| swatch.name == from).unwrap();
        swatch.name = to.to_owned();
        swatch.named = true;
        expected.rename_swatch_refs(from, to);
        expected.refresh_gradient_stops();
        expected
    }
    fn rename_calls(h: &Harness<'_, Rig>, start: usize) -> Vec<Value> {
        h.state().app.session.journal[start..].iter().filter(|(command, _)| command == "swatch.options").map(|(_, args)| args.clone()).collect()
    }
    fn fixture() -> (DesignApp, [u64; 2]) {
        let mut s = designcraft_engine::Session::new();
        s.execute("file.new", &json!({})).unwrap();
        for (name, color) in [("Brand", "#d03010"), ("Other", "#108080")] {
            s.execute("swatch.create", &json!({"name":name,"color":color})).unwrap();
        }
        let made = s.execute("frame.create", &json!({"rect":[30,30,180,130],"content":"text","text":"Hello"})).unwrap();
        let first = made["id"].as_u64().unwrap();
        s.execute("object.fill", &json!({"ids":[first],"swatch":"Brand"})).unwrap();
        s.execute("object.stroke", &json!({"ids":[first],"swatch":"Brand"})).unwrap();
        s.execute("text.select", &json!({"story":made["story"],"anchor":0,"focus":3})).unwrap();
        s.execute("type.char", &json!({"attrs":{"fill":"Brand"}})).unwrap();
        s.execute("swatch.newColorGroup", &json!({"name":"Brand Colors","swatches":["Brand"]})).unwrap();
        s.execute("style.character.create", &json!({"name":"Accent"})).unwrap();
        s.edit(|doc, _| {
            doc.styles_mut().char_style_mut("Accent").unwrap().chars.fill = Some("Brand".into());
            doc.styles_mut().para_mut(designcraft_doc::BASIC_PARAGRAPH).unwrap().para.shading_color = Some("Brand".into());
            doc.swatches.push(designcraft_color::Swatch {
                name: "Brand Half".into(),
                value: SwatchValue::Tint { base: "Brand".into(), tint: 0.5 },
                locked: false,
                named: true,
                hidden: false,
            });
            Ok(())
        })
        .unwrap();
        let second = s.execute("frame.create", &json!({"rect":[200,30,270,110],"content":"unassigned"})).unwrap()["id"].as_u64().unwrap();
        s.execute("object.fill", &json!({"ids":[second],"swatch":"Other"})).unwrap();
        s.execute("selection.set", &json!({"ids":[first]})).unwrap();
        s.execute("frame.create", &json!({"rect":[300,30,340,80]})).unwrap();
        s.execute("edit.undo", &json!({})).unwrap();
        assert!(!s.active().unwrap().history.undo.is_empty());
        assert_eq!(s.active().unwrap().history.redo.len(), 1);
        (DesignApp::new_for_panel_tests(s, crate::Services::default()), [first, second])
    }
    fn harness(mut app: DesignApp, installed: bool) -> Harness<'static, Rig> {
        app.framework_transform = false;
        app.framework_opacity = installed;
        let ctx = egui::Context::default();
        if installed {
            app.install_framework(&ctx);
        }
        assert_eq!(app.framework_installed, installed);
        app.egui_ctx = Some(ctx.clone());
        app.raw_input_hook(&ctx, &mut egui::RawInput::default());
        let mut fonts = false;
        let mut h = Harness::builder().with_context(ctx).with_size(vec2(380.0, 720.0)).with_step_dt(0.05).build_ui_state(
            move |ui, state: &mut Rig| {
                if !fonts {
                    crate::theme::install_fonts(ui.ctx(), &state.app.ui.language);
                    fonts = true;
                    return;
                }
                state.controls = None;
                if !state.visible {
                    return;
                }
                let builder = if state.sizing { UiBuilder::new().sizing_pass() } else { UiBuilder::new() };
                ui.scope_builder(builder, |ui| {
                    ui.add_enabled_ui(state.enabled, |ui| {
                        state.controls = record(ui, egui::Id::unique("swatches-rename-apply-workflow"), |ui| {
                            egui::Frame::NONE.inner_margin(egui::Margin::same(8)).show(ui, |ui| {
                                let output = egui::ScrollArea::vertical()
                                    .id_salt(("shared-panel-body", "swatches"))
                                    .max_height(state.height)
                                    .auto_shrink([false, true])
                                    .show(ui, |ui| crate::dock::panel_body(&mut state.app, ui, "swatches"));
                                state.viewport = output.inner_rect;
                                state.offset = output.state.offset();
                            });
                        })
                        .1;
                        state.outside = ui.button("Next native control").rect;
                    });
                });
            },
            Rig {
                app,
                controls: None,
                enabled: true,
                sizing: false,
                visible: true,
                height: 600.0,
                viewport: egui::Rect::NOTHING,
                offset: egui::Vec2::ZERO,
                outside: egui::Rect::NOTHING,
            },
        );
        h.input_mut().max_texture_side = Some(8192);
        idle(&mut h, 4);
        h
    }

    #[test]
    fn installed_rename_then_live_row_apply_two_undos_and_redos_preserve_the_complete_document() {
        let (app, ids) = fixture();
        let mut h = harness(app, true);
        open(&mut h, "Brand");
        let before = snapshot(&h);
        let name = "  Signal & α  ";
        type_name(&mut h, name);
        unchanged(&h, &before);
        key(&mut h, Key::Enter);
        assert!(draft(&h).is_none());
        assert_eq!(rename_calls(&h, before.journal), vec![json!({"name":"Brand","to":"Signal & α"})]);
        let expected = renamed(&before.doc, "Brand", "Signal & α");
        let after = snapshot(&h);
        assert_eq!(after.doc.as_ref(), &expected);
        assert_eq!(after.history, (before.history.0 + 1, 0));
        assert_eq!(after.owner, before.owner);
        assert!(Arc::ptr_eq(&after.saved, &before.saved));
        h.state_mut().app.run("selection.set", json!({"ids":[ids[1]]})).unwrap();
        idle(&mut h, 2);
        let apply_before = snapshot(&h);
        let at = reveal(&mut h, "Signal & α");
        click(&mut h, at);
        let mut applied = expected.clone();
        applied.item_mut(ItemId(ids[1])).unwrap().fill.swatch = "Signal & α".into();
        assert_eq!(h.state().app.session.active().unwrap().doc.as_ref(), &applied);
        assert_eq!(&h.state().app.session.journal[apply_before.journal..], &[("object.fill".to_owned(), json!({"swatch":"Signal & α"}))]);
        assert_eq!(snapshot(&h).history, (before.history.0 + 2, 0));
        for document in [&expected, before.doc.as_ref()] {
            h.state_mut().app.run("edit.undo", json!({})).unwrap();
            idle(&mut h, 2);
            assert_eq!(h.state().app.session.active().unwrap().doc.as_ref(), document);
        }
        for document in [&expected, &applied] {
            h.state_mut().app.run("edit.redo", json!({})).unwrap();
            idle(&mut h, 2);
            assert_eq!(h.state().app.session.active().unwrap().doc.as_ref(), document);
        }
        let settled = snapshot(&h);
        idle(&mut h, 8);
        unchanged(&h, &settled);
    }

    #[test]
    fn installed_clean_pointer_loss_closes_the_actual_editor_without_a_command_or_model_edit() {
        let (app, _) = fixture();
        let mut h = harness(app, true);
        open(&mut h, "Brand");
        let before = snapshot(&h);
        let at = h.state().outside.center();
        click(&mut h, at);
        assert!(draft(&h).is_none());
        unchanged(&h, &before);
        assert!(rename_calls(&h, before.journal).is_empty());
        idle(&mut h, 5);
        unchanged(&h, &before);
    }

    #[test]
    fn installed_dirty_pointer_loss_renames_once_and_preserves_the_other_row_picked_and_applied() {
        let (app, ids) = fixture();
        let mut h = harness(app, true);
        open(&mut h, "Brand");
        let before = snapshot(&h);
        type_name(&mut h, "  New Brand  ");
        let at = reveal(&mut h, "Other");
        click(&mut h, at);
        idle(&mut h, 2);
        assert!(draft(&h).is_none());
        assert_eq!(rename_calls(&h, before.journal), vec![json!({"name":"Brand","to":"New Brand"})]);
        let mut expected = renamed(&before.doc, "Brand", "New Brand");
        expected.item_mut(ItemId(ids[0])).unwrap().fill.swatch = "Other".into();
        assert_eq!(h.state().app.session.active().unwrap().doc.as_ref(), &expected);
        assert_eq!(h.ctx.data(|data| data.get_temp::<String>(egui::Id::unique(PICKED))).as_deref(), Some("Other"));
        let after = snapshot(&h);
        idle(&mut h, 5);
        unchanged(&h, &after);
    }

    #[test]
    fn installed_blank_or_trim_unchanged_enter_closes_without_rename_history_or_retry() {
        for name in ["Brand", "  Brand  ", " \u{2003} "] {
            let (app, _) = fixture();
            let mut h = harness(app, true);
            open(&mut h, "Brand");
            let before = snapshot(&h);
            type_name(&mut h, name);
            unchanged(&h, &before);
            key(&mut h, Key::Enter);
            assert!(draft(&h).is_none());
            unchanged(&h, &before);
            idle(&mut h, 5);
            unchanged(&h, &before);
        }
    }

    #[test]
    fn installed_duplicate_name_refusal_closes_once_without_document_identity_or_history_changes() {
        let (app, _) = fixture();
        let mut h = harness(app, true);
        open(&mut h, "Brand");
        let before = snapshot(&h);
        type_name(&mut h, "Other");
        key(&mut h, Key::Enter);
        assert!(draft(&h).is_none());
        unchanged(&h, &before);
        assert!(h.state().app.ui.status.contains("exists"));
        idle(&mut h, 6);
        unchanged(&h, &before);
    }

    #[test]
    fn installed_dirty_escape_cancels_and_clean_idle_escape_matches_the_actual_native_route() {
        for installed in [false, true] {
            let (app, _) = fixture();
            let mut h = harness(app, installed);
            open(&mut h, "Brand");
            let before = snapshot(&h);
            type_name(&mut h, "Abandoned");
            key(&mut h, Key::Escape);
            assert!(draft(&h).is_none());
            unchanged(&h, &before);
        }
        let mut outcomes = Vec::new();
        for installed in [false, true] {
            let (app, _) = fixture();
            let mut h = harness(app, installed);
            open(&mut h, "Brand");
            let before = snapshot(&h);
            key(&mut h, Key::Escape);
            unchanged(&h, &before);
            outcomes.push(draft(&h).is_none());
        }
        assert_eq!(outcomes[0], outcomes[1], "actual clean-Escape app/native comparison, not a resolver or false/false inference");
    }

    #[test]
    fn installed_tab_closes_clean_text_and_commits_dirty_text_once_without_pointer_cause_inference() {
        for to in ["Brand", "Tabbed Brand"] {
            let (app, _) = fixture();
            let mut h = harness(app, true);
            open(&mut h, "Brand");
            let before = snapshot(&h);
            if to != "Brand" {
                type_name(&mut h, to);
            }
            key(&mut h, Key::Tab);
            assert!(draft(&h).is_none());
            if to == "Brand" {
                unchanged(&h, &before);
            } else {
                assert_eq!(rename_calls(&h, before.journal), vec![json!({"name":"Brand","to":to})]);
                assert_eq!(h.state().app.session.active().unwrap().doc.as_ref(), &renamed(&before.doc, "Brand", to));
                assert_eq!(snapshot(&h).history, (before.history.0 + 1, 0));
            }
            let settled = snapshot(&h);
            idle(&mut h, 5);
            unchanged(&h, &settled);
        }
    }

    #[test]
    fn installed_global_rename_survives_live_object_selection_changes_with_the_same_editor_identity() {
        let (app, ids) = fixture();
        let mut h = harness(app, true);
        open(&mut h, "Brand");
        let field = editor(&h, "Brand");
        type_name(&mut h, "Global Brand");
        h.state_mut().app.run("selection.set", json!({"ids":[ids[1]]})).unwrap();
        idle(&mut h, 2);
        assert_eq!(editor(&h, "Brand").id, field.id, "object selection is not the named swatch's lifetime");
        let before = snapshot(&h);
        key(&mut h, Key::Enter);
        let after = snapshot(&h);
        assert_eq!(after.selection, before.selection);
        assert_eq!(after.doc.as_ref(), &renamed(&before.doc, "Brand", "Global Brand"));
        assert_eq!(after.doc.item(ItemId(ids[1])).unwrap().fill.swatch, "Other");
        assert_eq!(rename_calls(&h, before.journal), vec![json!({"name":"Brand","to":"Global Brand"})]);
        let at = reveal(&mut h, "Global Brand");
        click(&mut h, at);
        assert_eq!(h.state().app.session.active().unwrap().doc.item(ItemId(ids[1])).unwrap().fill.swatch, "Global Brand");
    }

    #[test]
    fn installed_held_editor_input_retires_after_observed_disabled_sizing_and_absent_parent_lifetimes() {
        for change in ["disabled", "sizing", "absent"] {
            let (app, _) = fixture();
            let mut h = harness(app, true);
            open(&mut h, "Brand");
            type_name(&mut h, "Retired");
            let at = editor(&h, "Brand").rect.center();
            frame(&mut h, Some(Event::PointerMoved(at)));
            press(&mut h, at, true);
            match change {
                "disabled" => h.state_mut().enabled = false,
                "sizing" => h.state_mut().sizing = true,
                _ => h.state_mut().visible = false,
            }
            let before = snapshot(&h);
            idle(&mut h, 3);
            unchanged(&h, &before);
            press(&mut h, at, false);
            h.state_mut().enabled = true;
            h.state_mut().sizing = false;
            h.state_mut().visible = true;
            idle(&mut h, 3);
            key(&mut h, Key::Enter);
            key(&mut h, Key::Space);
            assert!(draft(&h).is_none(), "{change} must retire the old editor");
            unchanged(&h, &before);
            open(&mut h, "Brand");
            type_name(&mut h, "Fresh");
            let fresh = snapshot(&h);
            key(&mut h, Key::Enter);
            assert_eq!(rename_calls(&h, fresh.journal), vec![json!({"name":"Brand","to":"Fresh"})]);
        }
    }

    #[test]
    fn installed_held_editor_input_retires_after_observed_hidden_locked_removed_or_externally_renamed_sources() {
        for change in ["hidden", "locked", "removed", "renamed"] {
            let (app, _) = fixture();
            let mut h = harness(app, true);
            open(&mut h, "Brand");
            type_name(&mut h, "Retired");
            let at = editor(&h, "Brand").rect.center();
            frame(&mut h, Some(Event::PointerMoved(at)));
            press(&mut h, at, true);
            match change {
                "removed" => {
                    h.state_mut().app.run("swatch.delete", json!({"name":"Brand"})).unwrap();
                }
                "renamed" => {
                    h.state_mut().app.run("swatch.options", json!({"name":"Brand","to":"External"})).unwrap();
                }
                _ => {
                    h.state_mut()
                        .app
                        .session
                        .edit(|doc, _| {
                            let swatch = doc.swatches.iter_mut().find(|swatch| swatch.name == "Brand").unwrap();
                            if change == "hidden" {
                                swatch.hidden = true;
                            } else {
                                swatch.locked = true;
                            }
                            Ok(())
                        })
                        .unwrap();
                }
            }
            let before = snapshot(&h);
            idle(&mut h, 2);
            press(&mut h, at, false);
            key(&mut h, Key::Enter);
            key(&mut h, Key::Space);
            assert!(draft(&h).is_none(), "{change} must retire the observed source");
            unchanged(&h, &before);
            assert!(rename_calls(&h, before.journal).is_empty());
        }
    }

    #[test]
    fn installed_physical_group_collapse_retires_the_editor_before_a_later_return_can_apply_its_old_draft() {
        let (app, _) = fixture();
        let mut h = harness(app, true);
        open(&mut h, "Brand");
        type_name(&mut h, "Retired");
        let before = snapshot(&h);
        let group = h.get_by_label("Brand Colors").rect().center();
        click(&mut h, group);
        idle(&mut h, 2);
        assert!(draft(&h).is_none());
        unchanged(&h, &before);
        wait_click_interval(&mut h);
        click(&mut h, group);
        key(&mut h, Key::Enter);
        unchanged(&h, &before);
        open(&mut h, "Brand");
        type_name(&mut h, "Returned");
        let fresh = snapshot(&h);
        key(&mut h, Key::Enter);
        assert_eq!(rename_calls(&h, fresh.journal), vec![json!({"name":"Brand","to":"Returned"})]);
    }

    #[test]
    fn installed_held_editor_input_retires_on_document_swap_same_uid_epoch_replacement_and_close() {
        for change in ["document", "epoch", "close"] {
            let (app, _) = fixture();
            let mut h = harness(app, true);
            open(&mut h, "Brand");
            type_name(&mut h, "Retired");
            let at = editor(&h, "Brand").rect.center();
            frame(&mut h, Some(Event::PointerMoved(at)));
            press(&mut h, at, true);
            match change {
                "document" => {
                    h.state_mut().app.run("file.new", json!({})).unwrap();
                    h.state_mut().app.run("swatch.create", json!({"name":"Brand","color":"#00aabb"})).unwrap();
                }
                "epoch" => {
                    let old = h.state().app.session.active().unwrap();
                    let mut replacement = designcraft_engine::DocState::new((*old.doc).clone(), old.path.clone());
                    replacement.uid = old.uid;
                    replacement.selection = old.selection.clone();
                    assert_ne!(replacement.identity_epoch, old.identity_epoch);
                    *h.state_mut().app.session.active_mut().unwrap() = replacement;
                }
                _ => {
                    h.state_mut().app.run("file.close", json!({})).unwrap();
                    assert!(h.state().app.session.active().is_none());
                }
            }
            if change == "close" {
                let journal = h.state().app.session.journal.len();
                idle(&mut h, 2);
                press(&mut h, at, false);
                key(&mut h, Key::Enter);
                assert_eq!(h.state().app.session.journal.len(), journal);
                assert!(draft(&h).is_none());
                h.state_mut().app.run("file.new", json!({})).unwrap();
                h.state_mut().app.run("swatch.create", json!({"name":"Brand","color":"#00aabb"})).unwrap();
                let before = snapshot(&h);
                idle(&mut h, 3);
                key(&mut h, Key::Enter);
                unchanged(&h, &before);
            } else {
                let before = snapshot(&h);
                idle(&mut h, 2);
                press(&mut h, at, false);
                key(&mut h, Key::Enter);
                key(&mut h, Key::Space);
                assert!(draft(&h).is_none());
                unchanged(&h, &before);
            }
        }
    }

    #[test]
    fn installed_real_double_click_still_opens_options_and_locked_names_never_start_an_editor() {
        let (app, _) = fixture();
        let mut h = harness(app, true);
        let at = reveal(&mut h, "Brand");
        click(&mut h, at);
        wait_click_interval(&mut h);
        let start = h.state().app.session.journal.len();
        frame(&mut h, Some(Event::PointerMoved(at)));
        for _ in 0..2 {
            press(&mut h, at, true);
            press(&mut h, at, false);
        }
        idle(&mut h, 3);
        assert_eq!(h.state().app.ui.dialog.as_ref().map(|dialog| dialog.id.as_str()), Some("swatchOptions"));
        assert!(draft(&h).is_none());
        assert!(rename_calls(&h, start).is_empty());
        h.state_mut().app.ui.dialog = None;
        wait_click_interval(&mut h);
        let at = reveal(&mut h, "[Black]");
        click(&mut h, at);
        wait_click_interval(&mut h);
        click(&mut h, at);
        wait_click_interval(&mut h);
        assert!(draft(&h).is_none());
        assert!(h.state().app.session.active().unwrap().doc.swatch("[Black]").unwrap().locked);
    }

    #[test]
    fn installed_real_scroll_host_and_full_unicode_names_have_no_sdk_default_length_truncation_in_german_and_rtl() {
        for language in ["de", "ar"] {
            let (mut app, _) = fixture();
            app.ui.language = language.into();
            let mut h = harness(app, true);
            h.state_mut().height = 220.0;
            idle(&mut h, 3);
            open(&mut h, "Brand");
            assert!(h.state().offset.y > 0.0, "real wheel revealed the grouped row");
            let before = snapshot(&h);
            let full = format!("{} & α آخر", "Long名".repeat(230));
            assert!(full.chars().count() > 1024);
            type_name(&mut h, &full);
            unchanged(&h, &before);
            assert!(
                h.query_all_by_role(egui::accesskit::Role::TextInput).any(|node| node.accesskit_node().value().as_deref() == Some(full.as_str())),
                "full public text value, not clipped paint or a fallback label"
            );
            key(&mut h, Key::Enter);
            assert_eq!(rename_calls(&h, before.journal), vec![json!({"name":"Brand","to":full})]);
            assert_eq!(h.state().app.session.active().unwrap().doc.as_ref(), &renamed(&before.doc, "Brand", &full));
            open(&mut h, &full);
            type_name(&mut h, "Short");
            let replace = snapshot(&h);
            key(&mut h, Key::Enter);
            assert_eq!(
                rename_calls(&h, replace.journal),
                vec![json!({"name":full,"to":"Short"})],
                "full Unicode source identity and one-shot replacement selection"
            );
        }
    }

    #[test]
    fn uninstalled_native_rename_apply_undo_fallback_remains_a_genuine_input_workflow() {
        let (app, ids) = fixture();
        let mut h = harness(app, false);
        open(&mut h, "Brand");
        let before = snapshot(&h);
        assert!(h.state().controls.as_ref().is_none_or(|recorded| recorded.elements.iter().all(|element| element.component != "TextField")));
        type_name(&mut h, "Native Brand");
        key(&mut h, Key::Enter);
        assert!(draft(&h).is_none());
        let expected = renamed(&before.doc, "Brand", "Native Brand");
        assert_eq!(h.state().app.session.active().unwrap().doc.as_ref(), &expected);
        assert_eq!(rename_calls(&h, before.journal), vec![json!({"name":"Brand","to":"Native Brand"})]);
        h.state_mut().app.run("selection.set", json!({"ids":[ids[1]]})).unwrap();
        idle(&mut h, 2);
        let at = reveal(&mut h, "Native Brand");
        click(&mut h, at);
        assert_eq!(h.state().app.session.active().unwrap().doc.item(ItemId(ids[1])).unwrap().fill.swatch, "Native Brand");
        h.state_mut().app.run("edit.undo", json!({})).unwrap();
        idle(&mut h, 2);
        assert_eq!(h.state().app.session.active().unwrap().doc.as_ref(), &expected);
        h.state_mut().app.run("edit.undo", json!({})).unwrap();
        idle(&mut h, 2);
        assert_eq!(h.state().app.session.active().unwrap().doc.as_ref(), before.doc.as_ref());
    }
}

/// The framework panels' own interactions: the colour mode and gradient type `Select`s, the channel fields, the row
/// menus, the folders, the ramp and the drags between the Swatches and Gradient panels, driven by real pointer and
/// key events through the installed framework.
#[cfg(all(test, not(target_arch = "wasm32")))]
mod framework_panel_tests {
    use super::*;
    use egui::accesskit::Role;
    use egui::{Event, Key, Modifiers, PointerButton};
    use egui_kittest::Harness;
    use egui_kittest::kittest::Queryable;
    use serde_json::Value;

    type Panel = fn(&mut DesignApp, &mut egui::Ui);

    struct State {
        app: DesignApp,
        panels: Vec<Panel>,
    }

    /// A frame with its fill set, and the Swatches panel's own swatches.
    fn app_with_frame() -> DesignApp {
        let mut app = DesignApp::new(designcraft_engine::Session::new(), crate::Services::default());
        app.run("file.new", json!({})).unwrap();
        app.run("frame.create", json!({"rect": [72, 72, 300, 300], "content": "unassigned"})).unwrap();
        app.run("swatch.create", json!({"name": "Port Fill", "color": "#108080"})).unwrap();
        app
    }

    /// `panels` side by side, on the installed framework.
    fn harness(mut app: DesignApp, panels: Vec<Panel>) -> Harness<'static, State> {
        let ctx = egui::Context::default();
        crate::theme::install_fonts(&ctx, "");
        app.framework_opacity = true;
        app.install_framework(&ctx);
        assert!(app.framework_installed);
        app.raw_input_hook(&ctx, &mut egui::RawInput::default());
        // Frames 50 ms apart: a double click's two presses fall within the double-click time.
        let mut h = Harness::builder().with_context(ctx).with_size(vec2(760.0, 900.0)).with_step_dt(0.05).build_ui_state(
            |ui, state: &mut State| {
                let panels = state.panels.clone();
                ui.horizontal_top(|ui| {
                    for (k, panel) in panels.into_iter().enumerate() {
                        ui.push_id(k, |ui| {
                            ui.vertical(|ui| {
                                ui.set_width(360.0);
                                panel(&mut state.app, ui);
                            });
                        });
                    }
                });
            },
            State { app, panels },
        );
        h.input_mut().max_texture_side = Some(8192);
        steps(&mut h, 3);
        h
    }

    fn steps(h: &mut Harness<'_, State>, count: usize) {
        for _ in 0..count {
            let mut raw = std::mem::take(h.input_mut());
            let ctx = h.ctx.clone();
            h.state_mut().app.raw_input_hook(&ctx, &mut raw);
            *h.input_mut() = raw;
            h.step();
        }
    }

    fn event(h: &mut Harness<'_, State>, event: Event) {
        h.input_mut().events.push(event);
        steps(h, 1);
    }

    fn press(h: &mut Harness<'_, State>, pos: egui::Pos2, button: PointerButton, pressed: bool) {
        event(h, Event::PointerButton { pos, button, pressed, modifiers: Modifiers::NONE });
    }

    fn click_with(h: &mut Harness<'_, State>, pos: egui::Pos2, button: PointerButton) {
        event(h, Event::PointerMoved(pos));
        press(h, pos, button, true);
        press(h, pos, button, false);
        steps(h, 2);
    }

    fn click_at(h: &mut Harness<'_, State>, pos: egui::Pos2) {
        click_with(h, pos, PointerButton::Primary);
    }

    /// Longer than the double-click time, so the next click is a click of its own.
    fn wait(h: &mut Harness<'_, State>) {
        steps(h, 40);
    }

    fn key(h: &mut Harness<'_, State>, key: Key, modifiers: Modifiers) {
        for pressed in [true, false] {
            event(h, Event::Key { key, physical_key: None, pressed, repeat: false, modifiers });
        }
        steps(h, 2);
    }

    /// A press at `from`, a drag in steps to `to` and the release there.
    fn drag(h: &mut Harness<'_, State>, from: egui::Pos2, to: egui::Pos2) {
        event(h, Event::PointerMoved(from));
        press(h, from, PointerButton::Primary, true);
        for i in 1..=10 {
            event(h, Event::PointerMoved(from + (to - from) * (i as f32 / 10.0)));
        }
        press(h, to, PointerButton::Primary, false);
        steps(h, 4);
    }

    fn rect_of(h: &Harness<'_, State>, role: Role, label: &str) -> egui::Rect {
        h.get_by_role_and_label(role, label).rect()
    }

    fn row(h: &Harness<'_, State>, name: &str) -> Option<egui::Rect> {
        h.query_by(|node| node.role() == Role::TreeItem && node.label().is_some_and(|label| names_swatch(&label, name))).map(|node| node.rect())
    }

    /// Picks `option` from the `Select` named `label`.
    fn pick(h: &mut Harness<'_, State>, label: &str, option: &str) {
        let at = rect_of(h, Role::ComboBox, label).center();
        click_at(h, at);
        let at = rect_of(h, Role::ListBoxOption, option).center();
        click_at(h, at);
    }

    fn calls(h: &Harness<'_, State>, from: usize, command: &str) -> Vec<Value> {
        h.state().app.session.journal[from..].iter().filter(|(id, _)| id == command).map(|(_, params)| params.clone()).collect()
    }

    fn undo_len(h: &Harness<'_, State>) -> usize {
        h.state().app.session.active().unwrap().history.undo.len()
    }

    fn gradient(h: &Harness<'_, State>) -> Option<designcraft_color::Gradient> {
        let st = h.state().app.session.active().unwrap();
        let id = st.selection.items[0];
        designcraft_color::swatch::resolve_gradient(&st.doc.swatches, &st.doc.item(id).unwrap().fill.swatch).cloned()
    }

    fn fill(h: &Harness<'_, State>) -> designcraft_color::Color {
        let st = h.state().app.session.active().unwrap();
        let id = st.selection.items[0];
        st.doc.resolve_color(&st.doc.item(id).unwrap().fill.swatch, 1.0).unwrap()
    }

    fn spin_buttons(h: &Harness<'_, State>, label: &str) -> usize {
        h.query_all_by(|node| node.role() == Role::SpinButton && node.label().as_deref() == Some(label)).count()
    }

    /// The Color panel's mode is a framework Select: picking a model shows that model's channels, and the mode is
    /// kept for the next pass.
    #[test]
    fn installed_colour_mode_select_shows_the_picked_models_channels() {
        let mut app = app_with_frame();
        app.run("object.color", json!({"color": {"c": 0, "m": 50, "y": 100, "k": 0}})).unwrap();
        let mut h = harness(app, vec![color_panel as Panel]);
        pick(&mut h, "Color Mode", "RGB");
        assert!(["R", "G", "B"].iter().all(|channel| spin_buttons(&h, channel) == 1), "the RGB channels");
        assert_eq!(spin_buttons(&h, "M"), 0);
        pick(&mut h, "Color Mode", "CMYK");
        assert!(["C", "M", "Y", "K"].iter().all(|channel| spin_buttons(&h, channel) == 1), "the CMYK channels");
        assert_eq!(h.ctx.data(|d| d.get_temp::<ColorMode>(egui::Id::unique("color_panel_mode"))), Some(ColorMode::Cmyk));
    }

    /// A typed channel value applies as a percent on Enter, as one `object.color` and one undo step: M 1 is 1 %.
    #[test]
    fn installed_typed_channel_value_applies_once_as_a_percent() {
        let mut app = app_with_frame();
        app.run("object.color", json!({"color": {"c": 0, "m": 50, "y": 100, "k": 0}})).unwrap();
        let mut h = harness(app, vec![color_panel as Panel]);
        pick(&mut h, "Color Mode", "CMYK");
        let journal = h.state().app.session.journal.len();
        let undo = undo_len(&h);
        let field = h.get_by(|node| node.role() == Role::SpinButton && node.label().as_deref() == Some("M")).rect().center();
        click_at(&mut h, field);
        key(&mut h, Key::A, Modifiers::COMMAND);
        event(&mut h, Event::Text("1".into()));
        steps(&mut h, 2);
        let designcraft_color::Color::Cmyk { m, .. } = fill(&h) else { panic!("not CMYK") };
        assert!((m - 0.5).abs() < 1e-6, "nothing applies while typing: {m}");
        key(&mut h, Key::Enter, Modifiers::NONE);
        let designcraft_color::Color::Cmyk { c, m, y, k } = fill(&h) else { panic!("not CMYK") };
        assert!(c == 0.0 && (m - 0.01).abs() < 1e-6 && (y - 1.0).abs() < 1e-6 && k == 0.0, "{c} {m} {y} {k}");
        assert_eq!(calls(&h, journal, "object.color").len(), 1);
        assert_eq!(undo_len(&h), undo + 1);
    }

    /// A row's menu moves its swatch into a colour group: one `swatch.moveToGroup`, one undo step, and the row then
    /// stands under the group's folder.
    #[test]
    fn installed_row_menu_moves_the_swatch_into_a_colour_group() {
        let mut app = app_with_frame();
        let group = app.run("swatch.newColorGroup", json!({})).unwrap()["name"].as_str().unwrap().to_string();
        let mut h = harness(app, vec![show as Panel]);
        let before = row(&h, "Port Fill").expect("the swatch's row");
        let journal = h.state().app.session.journal.len();
        let undo = undo_len(&h);
        click_with(&mut h, before.center(), PointerButton::Secondary);
        let at = rect_of(&h, Role::MenuItem, "Move to Color Group").center();
        click_at(&mut h, at);
        let at = rect_of(&h, Role::MenuItem, &group).center();
        click_at(&mut h, at);
        assert_eq!(calls(&h, journal, "swatch.moveToGroup"), [json!({"swatches": ["Port Fill"], "group": group})]);
        assert_eq!(undo_len(&h), undo + 1);
        let doc = &h.state().app.session.active().unwrap().doc;
        assert!(doc.color_groups.iter().any(|g| g.name == group && g.swatches.iter().any(|n| n == "Port Fill")));
        let folder = rect_of(&h, Role::TreeItem, &group);
        let after = row(&h, "Port Fill").expect("the swatch's row in its group");
        assert!(after.top() >= folder.bottom(), "the row stands under its folder");
    }

    /// A click on a folder folds it and the next unfolds it; a double click opens Color Group Options and leaves the
    /// folder as its first click left it.
    #[test]
    fn installed_folder_click_folds_and_double_click_opens_the_groups_options() {
        let mut app = app_with_frame();
        let group = app.run("swatch.newColorGroup", json!({"swatches": ["Port Fill"]})).unwrap()["name"].as_str().unwrap().to_string();
        let mut h = harness(app, vec![show as Panel]);
        assert!(row(&h, "Port Fill").is_some());
        let folder = rect_of(&h, Role::TreeItem, &group).center();
        click_at(&mut h, folder);
        assert!(row(&h, "Port Fill").is_none(), "folded");
        wait(&mut h);
        click_at(&mut h, folder);
        assert!(row(&h, "Port Fill").is_some(), "unfolded");
        wait(&mut h);
        event(&mut h, Event::PointerMoved(folder));
        for _ in 0..2 {
            press(&mut h, folder, PointerButton::Primary, true);
            press(&mut h, folder, PointerButton::Primary, false);
        }
        steps(&mut h, 3);
        assert!(h.state().app.ui.dialog.is_some(), "Color Group Options");
        assert!(row(&h, "Port Fill").is_none(), "the double click's first click folded it");
    }

    /// The Gradient panel's Type is a framework Select: Radial makes the selection's gradient radial.
    #[test]
    fn installed_gradient_type_select_sets_the_kind() {
        let mut app = app_with_frame();
        app.run("object.gradient", json!({})).unwrap();
        let mut h = harness(app, vec![gradient_panel as Panel]);
        let journal = h.state().app.session.journal.len();
        pick(&mut h, "Type", "Radial");
        assert_eq!(gradient(&h).unwrap().kind, designcraft_color::GradientKind::Radial);
        assert_eq!(calls(&h, journal, "object.gradient"), [json!({"kind": "radial", "target": "fill"})]);
    }

    /// A click under the ramp, away from the stops, adds a stop there; before the fill is a gradient that gives it the
    /// default one with the new stop.
    #[test]
    fn installed_click_under_the_ramp_adds_a_stop() {
        let mut h = harness(app_with_frame(), vec![gradient_panel as Panel]);
        assert!(gradient(&h).is_none());
        let ramp = rect_of(&h, Role::Group, "Gradient ramp");
        click_at(&mut h, egui::pos2(ramp.center().x, ramp.bottom() - 4.0));
        let g = gradient(&h).expect("the click gives the fill a gradient");
        assert_eq!(g.stops.len(), 3, "White, the new stop, Black");
        assert!(g.stops[0].offset < g.stops[1].offset && g.stops[1].offset < g.stops[2].offset);
    }

    /// A swatch row dragged from the Swatches list onto the middle of the Gradient panel's ramp is a new stop there.
    #[test]
    fn installed_swatch_dragged_onto_the_ramp_is_a_new_stop() {
        let mut h = harness(app_with_frame(), vec![gradient_panel as Panel, show as Panel]);
        let from = row(&h, "C=100 M=0 Y=0 K=0").expect("cyan's row");
        let ramp = rect_of(&h, Role::Group, "Gradient ramp");
        drag(&mut h, egui::pos2(from.left() + 10.0, from.center().y), egui::pos2(ramp.center().x, ramp.top() + 17.0));
        let g = gradient(&h).expect("the drop gives the fill a gradient");
        assert_eq!(g.stops.len(), 3, "White, cyan, Black");
        assert_eq!(g.stops[1].color, designcraft_color::Color::cmyk(1.0, 0.0, 0.0, 0.0));
        assert!((g.stops[1].offset - 0.5).abs() < 0.05, "{}", g.stops[1].offset);
    }

    /// The Gradient panel's thumbnail dragged onto the Swatches list keeps the selection's gradient as a swatch, which
    /// the object then uses by name.
    #[test]
    fn installed_gradient_thumbnail_dropped_on_the_list_becomes_a_swatch() {
        let mut app = app_with_frame();
        app.run("object.gradient", json!({})).unwrap();
        let mut h = harness(app, vec![gradient_panel as Panel, show as Panel]);
        let named = |h: &Harness<'_, State>| h.state().app.session.active().unwrap().doc.swatches.iter().filter(|w| !w.hidden).count();
        let before = named(&h);
        let thumb = rect_of(&h, Role::Button, "Gradient").center();
        let to = row(&h, "[Black]").expect("a row of the list").center();
        drag(&mut h, thumb, to);
        assert_eq!(named(&h), before + 1);
        let st = h.state().app.session.active().unwrap();
        assert!(matches!(st.doc.swatch("New Gradient Swatch").map(|w| &w.value), Some(SwatchValue::Gradient { .. })));
        assert_eq!(st.doc.item(st.selection.items[0]).unwrap().fill.swatch, "New Gradient Swatch");
    }
}
