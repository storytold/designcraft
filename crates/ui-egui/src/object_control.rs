//! The object Control panel: fixed, compact two-row groups, independent of widget layout growth.

use designcraft_geom::{Unit, corners::CornerShape};
use egui::{Rect, Sense, Ui, vec2};
use serde_json::{Value, json};

use crate::panels::{self, properties};
use crate::widgets::{self, FIELD_H, NumField};
use crate::{DesignApp, icons, theme::Tokens};

const ROW_PITCH: f32 = FIELD_H + 4.0;
pub(crate) const HEIGHT: f32 = ROW_PITCH + FIELD_H;

fn group(ui: &mut Ui, id: &str, width: f32, draw: impl FnOnce(&mut Ui, Rect)) {
    let (rect, _) = ui.allocate_exact_size(vec2(width, HEIGHT), Sense::hover());
    ui.interact(rect, egui::Id::new(("object_control", id)), Sense::hover());
    let mut child = ui.new_child(egui::UiBuilder::new().id_salt(id).max_rect(rect).layout(egui::Layout::left_to_right(egui::Align::Center)));
    draw(&mut child, rect);
}

fn cell<R>(ui: &mut Ui, group: Rect, x: f32, row: usize, width: f32, enabled: bool, draw: impl FnOnce(&mut Ui) -> R) -> R {
    let rect = Rect::from_min_size(group.min + vec2(x, row as f32 * ROW_PITCH), vec2(width, FIELD_H));
    properties::place(ui, rect, |ui| {
        ui.set_clip_rect(ui.clip_rect().intersect(rect));
        if !enabled {
            ui.disable();
        }
        draw(ui)
    })
}

fn icon(ui: &mut Ui, group: Rect, x: f32, row: usize, name: &str, tip: &str) {
    let rect = Rect::from_min_size(group.min + vec2(x, row as f32 * ROW_PITCH + 2.5), vec2(16.0, 16.0));
    icons::paint(ui.painter(), rect, name, Tokens::get(ui.ctx()).icon);
    ui.interact(rect, ui.id().with(("caption", name, row)), Sense::hover()).on_hover_ui(|ui| {
        crate::rtl::label(ui, tip);
    });
}

fn separator(ui: &mut Ui) {
    let (rect, _) = ui.allocate_exact_size(vec2(4.0, HEIGHT), Sense::hover());
    ui.painter().line_segment([rect.center_top(), rect.center_bottom()], egui::Stroke::new(1.0, Tokens::get(ui.ctx()).divider));
}

fn toggle(ui: &mut Ui, group: Rect, x: f32, key: &str, tip: &str) -> bool {
    let id = egui::Id::new(key);
    let on = ui.data(|d| d.get_temp::<bool>(id)).unwrap_or(false);
    let rect = Rect::from_min_size(group.min + vec2(x, 12.5), vec2(20.0, FIELD_H));
    if properties::place(ui, rect, |ui| widgets::icon_toggle(ui, if on { "link" } else { "link-broken" }, on, tip)).clicked() {
        ui.data_mut(|d| d.insert_temp(id, !on));
        !on
    } else {
        on
    }
}

pub(crate) fn show(app: &mut DesignApp, ui: &mut Ui) {
    let language = app.ui.language.clone();
    let tr = |text: &str| crate::i18n::tr(&language, text).to_owned();
    let units = app.session.active().map_or(Unit::Picas, |d| d.doc.settings.horizontal_units);
    let info = panels::sel_info(app);
    let selected = info.is_some();
    let reference = app.ui.ref_point.min(8);
    let geometry = info.as_ref().map(|i| i.page_rect);
    group(ui, "reference", 26.0, |ui, rect| {
        let proxy = Rect::from_center_size(rect.center(), vec2(26.0, 26.0));
        if let Some(point) = properties::ref_point_proxy(ui, proxy, reference) {
            app.ui.ref_point = point;
        }
    });
    group(ui, "dimensions", 236.0, |ui, rect| {
        let constrain = toggle(ui, rect, 216.0, "constrain_wh", &tr("Constrain proportions"));
        let (fx, fy) = ((reference % 3) as f64 / 2.0, (reference / 3) as f64 / 2.0);
        let values = geometry.map(|r| [r.x0 + r.width() * fx, r.y0 + r.height() * fy, r.width(), r.height()]);
        let mut set = serde_json::Map::new();
        for (row, fields) in
            [[("X:", "cx", "x", 0), ("W:", "cw", "width", 2)], [("Y:", "cy", "y", 1), ("H:", "ch", "height", 3)]].into_iter().enumerate()
        {
            for (column, (label, id, key, index)) in fields.into_iter().enumerate() {
                let x = column as f32 * 108.0;
                cell(ui, rect, x, row, 14.0, true, |ui| widgets::caption_w(ui, label, 14.0));
                let value = values.map(|v| v[index]);
                if let Some(v) = cell(ui, rect, x + 15.0, row, 86.0, selected, |ui| {
                    NumField::measure(id, value, units).width(86.0).spinner().enabled(selected).show(ui)
                }) {
                    set.insert(key.into(), json!(v));
                    if constrain
                        && column == 1
                        && let Some(old) = value.filter(|old| *old > 0.0)
                    {
                        let other = if key == "width" { ("height", 3) } else { ("width", 2) };
                        if let Some(values) = values {
                            set.insert(other.0.into(), json!(values[other.1] * v / old));
                        }
                    }
                }
            }
        }
        if !set.is_empty() {
            set.insert("ref".into(), json!(reference));
            let _ = app.run("transform.set", Value::Object(set));
        }
    });
    separator(ui);
    let transform = if selected { app.session.execute("transform.info", &json!({})).ok() } else { None };
    let get = |key: &str| transform.as_ref().and_then(|v| v.get(key)).and_then(Value::as_f64);
    group(ui, "transform", 214.0, |ui, rect| {
        let constrain = toggle(ui, rect, 102.0, "ctl_constrain_scale", &tr("Constrain proportions"));
        for (row, scale, angle, angle_icon) in [(0, "scaleX", "rotation", "tool-rotate"), (1, "scaleY", "shear", "tool-shear")] {
            icon(ui, rect, 0.0, row, "tool-scale", &tr(if row == 0 { "Scale X Percentage" } else { "Scale Y Percentage" }));
            if let Some(v) = cell(ui, rect, 20.0, row, 78.0, selected, |ui| {
                NumField::number(scale, get(scale), "%", 1).width(78.0).spinner().enabled(selected).show(ui)
            }) {
                let mut set = json!({scale: v, "ref": reference});
                if constrain && let Some(old) = get(scale).filter(|v| v.abs() > 0.01) {
                    let other = if row == 0 { "scaleY" } else { "scaleX" };
                    if let Some(value) = get(other) {
                        set[other] = json!(value * v / old);
                    }
                }
                let _ = app.run("transform.set", set);
            }
            icon(ui, rect, 126.0, row, angle_icon, &tr(if row == 0 { "Rotation Angle" } else { "Shear X Angle" }));
            if let Some(v) = cell(ui, rect, 146.0, row, 68.0, selected, |ui| {
                NumField::number(angle, get(angle), "°", 1).width(68.0).spinner().enabled(selected).show(ui)
            }) {
                let _ = app.run("transform.set", json!({angle: v, "ref": reference}));
            }
        }
    });
    separator(ui);
    group(ui, "rotate_flip", 44.0, |ui, rect| {
        for (row, angle, rotate, axis, flip) in
            [(0, 90, "tool-rotate", "horizontal", "flip-horizontal"), (1, -90, "rotate-clockwise", "vertical", "flip-vertical")]
        {
            let tip = tr(if row == 0 { "Rotate 90° Counterclockwise" } else { "Rotate 90° Clockwise" });
            if cell(ui, rect, 0.0, row, 20.0, selected, |ui| icons::button(ui, rotate, 20.0, false, &tip)).clicked() {
                let _ = app.run("transform.rotate", json!({"angle": angle, "ref": reference}));
            }
            if cell(ui, rect, 24.0, row, 20.0, selected, |ui| {
                icons::button(ui, flip, 20.0, false, &tr(if row == 0 { "Flip Horizontal" } else { "Flip Vertical" }))
            })
            .clicked()
            {
                let _ = app.run("transform.flip", json!({"axis": axis, "ref": reference}));
            }
        }
    });
    separator(ui);
    group(ui, "appearance", 224.0, |ui, rect| {
        for (row, stroke) in [(0, false), (1, true)] {
            cell(ui, rect, 0.0, row, 30.0, selected, |ui| {
                panels::swatch_picker(
                    app,
                    ui,
                    if stroke { "ctlstroke" } else { "ctlfill" },
                    info.as_ref().map(|i| if stroke { i.stroke.clone() } else { i.fill.clone() }),
                    |app, name| {
                        let _ = app.run(if stroke { "object.stroke" } else { "object.fill" }, json!({"swatch": name}));
                    },
                );
                let chip = Rect::from_min_size(rect.min + vec2(0.0, row as f32 * ROW_PITCH + 1.5), vec2(18.0, 18.0));
                if stroke && let Some(doc) = app.session.active().map(|st| &st.doc) {
                    let (color, gradient) = info.as_ref().map_or((Some(egui::Color32::GRAY), None), |i| widgets::swatch_colors(doc, &i.stroke, 1.0));
                    let inner = Tokens::get(ui.ctx()).panel;
                    if gradient.is_some() {
                        widgets::paint_chip(ui.painter(), chip, color, gradient);
                        ui.painter().rect_filled(chip.shrink(5.4), 0.0, inner);
                    } else {
                        widgets::paint_stroke_chip(ui.painter(), chip, color, inner);
                    }
                }
                ui.interact(chip, ui.id().with("swatch_tip"), Sense::hover()).on_hover_text(tr(if stroke { "Stroke" } else { "Fill" }));
            });
        }
        icon(ui, rect, 36.0, 0, "panel-stroke", &tr("Stroke Weight"));
        if let Some(v) = cell(ui, rect, 56.0, 0, 74.0, selected, |ui| {
            NumField::number("csw", info.as_ref().map(|i| i.stroke_weight), " pt", 3).width(74.0).spinner().range(0.0, 1000.0).show(ui)
        }) {
            let _ = app.run("object.stroke", json!({"weight": v}));
        }
        cell(ui, rect, 36.0, 1, 94.0, selected, |ui| {
            properties::stroke_type_dropdown(app, ui, info.as_ref().map_or("solid", |i| &i.stroke_kind), 94.0)
        });
        icon(ui, rect, 138.0, 0, "opacity", &tr("Opacity"));
        if let Some(v) = cell(ui, rect, 158.0, 0, 66.0, selected, |ui| {
            NumField::number("cop", info.as_ref().map(|i| i.opacity * 100.0), "%", 0).width(66.0).spinner().range(0.0, 100.0).show(ui)
        }) {
            let _ = app.run("object.opacity", json!({"opacity": v / 100.0}));
        }
        cell(ui, rect, 138.0, 1, 20.0, selected, |ui| {
            let resp = icons::button(ui, "fx", 20.0, info.as_ref().is_some_and(|i| i.shadow), &tr("Effects"));
            egui::Popup::menu(&resp).show(|ui| {
                if ui.selectable_label(info.as_ref().is_some_and(|i| i.shadow), crate::i18n::tr(&app.ui.language, "Drop Shadow")).clicked() {
                    let _ = app.run("object.dropShadow", json!({}));
                    ui.close();
                }
            });
        });
    });
    separator(ui);
    group(ui, "wrap", 68.0, |ui, rect| {
        for (index, (icon, mode, tip)) in [
            ("wrap-none", "none", "No Text Wrap"),
            ("wrap-bbox", "boundingBox", "Wrap Around Bounding Box"),
            ("wrap-contour", "contour", "Wrap Around Object Shape"),
            ("wrap-jump", "jumpObject", "Jump Object"),
            ("wrap-next", "jumpToNextColumn", "Jump to Next Column"),
        ]
        .into_iter()
        .enumerate()
        {
            if cell(ui, rect, (index % 3) as f32 * 24.0, index / 3, 20.0, selected, |ui| {
                icons::button(ui, icon, 20.0, info.as_ref().is_some_and(|i| i.wrap == mode), &tr(tip))
            })
            .clicked()
            {
                let _ = app.run("object.textWrap", json!({"mode": mode}));
            }
        }
    });
    separator(ui);
    group(ui, "corners", 102.0, |ui, rect| {
        let (shape, size) = info.as_ref().map_or((CornerShape::None, 0.0), |i| i.corner);
        icon(ui, rect, 0.0, 0, "corner", &tr("Corner Size"));
        if let Some(v) = cell(ui, rect, 22.0, 0, 80.0, selected, |ui| {
            NumField::measure("corner", info.as_ref().map(|i| i.corner.1), units).width(80.0).spinner().range(0.0, 1000.0).show(ui)
        }) {
            let shape = if shape == CornerShape::None { CornerShape::Rounded } else { shape };
            let _ = app.run("object.cornerOptions", json!({"shape": shape, "size": v}));
        }
        cell(ui, rect, 0.0, 1, 102.0, selected, |ui| properties::corner_type_dropdown(app, ui, shape, size, 102.0));
    });
    separator(ui);
    group(ui, "fitting", 116.0, |ui, rect| {
        let graphic = info.as_ref().is_some_and(|i| i.is_graphic);
        for (index, (icon, mode, tip)) in [
            ("fit-fill", "fillProportionally", "Fill Frame Proportionally"),
            ("fit-prop", "fitProportionally", "Fit Content Proportionally"),
            ("fit-content", "fitContentToFrame", "Fit Content to Frame"),
            ("fit-frame", "fitFrameToContent", "Fit Frame to Content"),
            ("fit-center", "centerContent", "Center Content"),
        ]
        .into_iter()
        .enumerate()
        {
            if cell(ui, rect, index as f32 * 24.0, 0, 20.0, graphic, |ui| icons::button(ui, icon, 20.0, false, &tr(tip))).clicked() {
                let _ = app.run("object.fit", json!({"mode": mode}));
            }
        }
        let mut auto = app
            .session
            .active()
            .and_then(|st| st.selection.items.first().and_then(|id| st.doc.item(*id)))
            .and_then(|it| it.graphic())
            .is_some_and(|g| g.auto_fit != designcraft_doc::Fitting::None);
        if cell(ui, rect, 0.0, 1, 116.0, graphic, |ui| widgets::checkbox(ui, &mut auto, crate::i18n::tr(&app.ui.language, "Auto-Fit"))).changed() {
            let _ = app.run("object.fittingOptions", json!({"autoFit": auto}));
        }
    });
    separator(ui);
    group(ui, "align", 68.0, |ui, rect| {
        for (index, (edge, tip)) in [
            ("left", "Align left edges"),
            ("hcenter", "Align horizontal centers"),
            ("right", "Align right edges"),
            ("top", "Align top edges"),
            ("vcenter", "Align vertical centers"),
            ("bottom", "Align bottom edges"),
        ]
        .into_iter()
        .enumerate()
        {
            if cell(ui, rect, (index % 3) as f32 * 24.0, index / 3, 20.0, selected, |ui| {
                icons::button(ui, &format!("objalign-{edge}"), 20.0, false, &tr(tip))
            })
            .clicked()
            {
                let _ = app.run("object.align", json!({"edge": edge}));
            }
        }
    });
    if let Some(columns) = info.as_ref().and_then(|i| i.columns) {
        separator(ui);
        group(ui, "columns", 48.0, |ui, rect| {
            icon(ui, rect, 0.0, 0, "text-columns", &tr("Columns"));
            if let Some(v) = cell(ui, rect, 0.0, 1, 48.0, true, |ui| {
                NumField::number("ccols", Some(columns as f64), "", 0).width(48.0).spinner().range(1.0, 100.0).show(ui)
            }) {
                let _ = app.run("object.textFrameOptions", json!({"columns": v as u64}));
            }
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn frame(ctx: &egui::Context, app: &mut DesignApp, events: Vec<egui::Event>) {
        frame_with_width(ctx, app, events, 1440.0);
    }

    fn frame_with_width(ctx: &egui::Context, app: &mut DesignApp, events: Vec<egui::Event>, width: f32) {
        let input = egui::RawInput {
            screen_rect: Some(Rect::from_min_size(egui::Pos2::ZERO, vec2(width, 900.0))),
            time: Some(ctx.input(|i| i.time) + 0.1),
            events,
            ..Default::default()
        };
        let mut output = ctx.run_ui(input, |ui| crate::chrome::control_bar(app, ui));
        output.textures_delta.clear();
    }

    fn fixture(selected: bool) -> (egui::Context, DesignApp) {
        let mut app = DesignApp::new(designcraft_engine::Session::new(), crate::Services::default());
        app.run("file.new", json!({})).unwrap();
        app.run("window.workspace", json!({"name": "Advanced"})).unwrap();
        assert!(app.ui.control_bar);
        if selected {
            app.run("frame.create", json!({"rect": [20, 30, 80, 90]})).unwrap();
            app.run("edit.selectAll", json!({})).unwrap();
        }
        let ctx = egui::Context::default();
        crate::theme::install_fonts(&ctx, "");
        crate::theme::apply(&ctx, &Tokens::for_brightness(app.ui.brightness));
        frame(&ctx, &mut app, vec![]);
        frame(&ctx, &mut app, vec![]);
        (ctx, app)
    }

    fn click(ctx: &egui::Context, app: &mut DesignApp, p: egui::Pos2) {
        frame(ctx, app, vec![egui::Event::PointerMoved(p)]);
        for pressed in [true, false] {
            frame(
                ctx,
                app,
                vec![egui::Event::PointerButton { pos: p, button: egui::PointerButton::Primary, pressed, modifiers: Default::default() }],
            );
        }
    }

    #[test]
    fn bottom_row_spinner_edits_height_inside_the_control_panel() {
        let (ctx, mut app) = fixture(true);
        let before = panels::sel_info(&app).unwrap().page_rect.height();
        let group = ctx.read_response(egui::Id::new(("object_control", "dimensions"))).unwrap().rect;
        assert!(group.bottom() <= 59.0, "second row is inside the panel: {group:?}");
        let height_up = group.min + vec2(123.0 + widgets::SEG_W / 2.0, ROW_PITCH + FIELD_H / 4.0);
        click(&ctx, &mut app, height_up);
        let after = panels::sel_info(&app).unwrap().page_rect.height();
        assert!((after - before - 1.0).abs() < 1e-6, "height spinner reaches transform.set: {before} -> {after}");
    }

    #[test]
    fn empty_selection_keeps_numeric_controls_inactive() {
        let (ctx, mut app) = fixture(false);
        let status = app.ui.status.clone();
        let group = ctx.read_response(egui::Id::new(("object_control", "dimensions"))).unwrap().rect;
        click(&ctx, &mut app, group.min + vec2(24.0, FIELD_H / 4.0));
        assert_eq!(app.ui.status, status, "disabled spinners do not attempt transforms");
        assert!(panels::sel_info(&app).is_none());
    }

    #[test]
    fn narrow_windows_can_scroll_to_the_alignment_controls() {
        let (ctx, mut app) = fixture(true);
        frame_with_width(&ctx, &mut app, vec![], 900.0);
        frame_with_width(&ctx, &mut app, vec![], 900.0);
        let id = egui::Id::new(("object_control", "align"));
        let before = ctx.read_response(id).unwrap().rect;
        assert!(before.right() > 900.0, "the toolbar overflows at the minimum width");
        frame_with_width(
            &ctx,
            &mut app,
            vec![
                egui::Event::PointerMoved(egui::pos2(700.0, 30.0)),
                egui::Event::MouseWheel {
                    unit: egui::MouseWheelUnit::Point,
                    delta: vec2(-500.0, 0.0),
                    phase: egui::TouchPhase::Move,
                    modifiers: Default::default(),
                },
            ],
            900.0,
        );
        for _ in 0..6 {
            frame_with_width(&ctx, &mut app, vec![], 900.0);
        }
        let after = ctx.read_response(id).unwrap().rect;
        assert!(after.left() < before.left() && after.right() <= 900.0, "scrolling reaches the last group: {before:?} -> {after:?}");
    }
}
