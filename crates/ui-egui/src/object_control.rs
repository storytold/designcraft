//! The object Control panel: fixed, compact two-row groups, independent of widget layout growth.

use designcraft_geom::{Unit, corners::CornerShape};
use egui::{Color32, Rect, Sense, Ui, vec2};
use serde_json::{Value, json};

use crate::panels::{self, properties};
use crate::widgets::{self, FIELD_H, NumField};
use crate::{DesignApp, icons, theme::Tokens};

const ROW_PITCH: f32 = FIELD_H + 4.0;
pub(crate) const HEIGHT: f32 = ROW_PITCH + FIELD_H;

fn group(ui: &mut Ui, id: &str, width: f32, draw: impl FnOnce(&mut Ui, Rect)) {
    let (rect, _) = ui.allocate_exact_size(vec2(width, HEIGHT), Sense::hover());
    ui.interact(rect, egui::Id::unique(("object_control", id)), Sense::hover());
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
    let resp = ui.interact(rect, ui.scope_id().with(("caption", name, row)), Sense::hover());
    resp.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Label, true, tip));
    resp.on_hover_ui(|ui| {
        crate::rtl::label(ui, tip);
    });
}

fn separator(ui: &mut Ui) {
    let (rect, _) = ui.allocate_exact_size(vec2(4.0, HEIGHT), Sense::hover());
    ui.painter().line_segment([rect.center_top(), rect.center_bottom()], egui::Stroke::new(1.0, Tokens::get(ui.ctx()).divider));
}

fn toggle(ui: &mut Ui, group: Rect, x: f32, key: &str, tip: &str) -> bool {
    let id = egui::Id::unique(key);
    let on = ui.data(|d| d.get_temp::<bool>(id)).unwrap_or(false);
    let rect = Rect::from_min_size(group.min + vec2(x, 12.5), vec2(20.0, FIELD_H));
    if properties::place(ui, rect, |ui| widgets::icon_toggle(ui, if on { "link" } else { "link-broken" }, on, tip)).clicked() {
        ui.data_mut(|d| d.insert_temp(id, !on));
        !on
    } else {
        on
    }
}

/// Auto-Fit is an ordinary bool proposal; the engine owns fitting and Undo.
fn auto_fit_checkbox(app: &DesignApp, ui: &mut Ui, value: &mut bool, label: &str) -> bool {
    // A held activation belongs to this exact document incarnation and ordered selection.
    // Do not let its release apply to a replacement document or a different set of frames.
    let target = app.session.active().map(|st| format!("{:?}", (st.uid, st.identity_epoch, &st.selection.items, &st.selection.text, &st.selection.cells)));
    let id = ui.scope_id().with(("object-control-auto-fit", target));
    #[cfg(not(target_arch = "wasm32"))]
    if app.framework_installed {
        use craft_ui_widgets::choice::Checkbox;
        use craft_ui_widgets::contract::{Bind, Props as _, Show as _};
        use craft_theme::ControlSize;

        let output = Checkbox::new(Bind::given(*value)).id(id).label(label.to_owned()).size(ControlSize::Sm).show(ui);
        if let Some(next) = output.events.committed().copied().filter(|next| *next != *value) {
            *value = next;
            return true;
        }
        return false;
    }
    ui.push_id(id, |ui| widgets::checkbox(ui, value, label).changed()).inner
}

/// Lay out the fixed two-row control groups for reference, geometry, transform, rotate/flip, appearance, wrap, corners, fitting, alignment and conditional columns.
pub(crate) fn show(app: &mut DesignApp, ui: &mut Ui) {
    let language = app.ui.language.clone();
    let tr = |text: &str| crate::i18n::tr(&language, text).to_owned();
    let units = app.session.active().map_or(Unit::Picas, |d| d.doc.settings.horizontal_units);
    let info = panels::sel_info(app);
    let selected = info.is_some();
    let reference = app.ui.ref_point.min(8);
    let geometry = info.as_ref().map(|i| i.page_rect);
    // One ruler guide selected: X (vertical guide) or Y (horizontal guide) is where it is.
    let guide = panels::guide_field(app);
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
                let on_guide = guide.filter(|g| g.row() == index);
                let value = if guide.is_some() { on_guide.map(|g| g.value) } else { values.map(|v| v[index]) };
                let enabled = selected || on_guide.is_some();
                let unit = on_guide.map_or(units, |g| g.unit);
                if let Some(v) = cell(ui, rect, x + 15.0, row, 86.0, enabled, |ui| {
                    NumField::measure(id, value, unit).width(86.0).spinner().enabled(enabled).show(ui)
                }) {
                    if let Some(g) = on_guide {
                        let _ = app.run("guide.move", g.move_to(v));
                        continue;
                    }
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
            // The chips open with nothing selected too, as they always have.
            cell(ui, rect, 0.0, row, 30.0, true, |ui| {
                let chip_resp = panels::swatch_picker(
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
                    let (color, gradient) = info.as_ref().map_or((Some(Color32::GRAY), None), |i| widgets::swatch_colors(doc, &i.stroke, 1.0));
                    let inner = Tokens::get(ui.ctx()).panel;
                    if gradient.is_some() {
                        widgets::paint_chip(ui.painter(), chip, color, gradient);
                        ui.painter().rect_filled(chip.shrink(5.4), 0.0, inner);
                    } else {
                        widgets::paint_stroke_chip(ui.painter(), chip, color, inner);
                    }
                }
                if let Some(resp) = chip_resp {
                    let label = tr(if stroke { "Stroke" } else { "Fill" });
                    resp.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Button, true, &label));
                    resp.on_hover_text(label);
                }
            });
        }
        icon(ui, rect, 36.0, 0, "panel-stroke", &tr("Stroke Weight"));
        if let Some(v) = cell(ui, rect, 56.0, 0, 74.0, selected, |ui| {
            NumField::points("csw", info.as_ref().map(|i| i.stroke_weight), 3).width(74.0).spinner().range(0.0, 1000.0).show(ui)
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
        // The displayed state intentionally follows the first selected graphic, not a new mixed-state policy.
        let mut auto = app
            .session
            .active()
            .and_then(|st| st.selection.items.first().and_then(|id| st.doc.item(*id)))
            .and_then(|it| it.graphic())
            .is_some_and(|g| g.auto_fit != designcraft_doc::Fitting::None);
        if cell(ui, rect, 0.0, 1, 116.0, graphic, |ui| auto_fit_checkbox(app, ui, &mut auto, &tr("Auto-Fit"))) {
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
        let caption = tr("Columns");
        let caption_w = ui.painter().layout_no_wrap(caption.clone(), egui::FontId::proportional(widgets::LABEL_SIZE), Color32::PLACEHOLDER).size().x;
        group(ui, "columns", (caption_w + 2.0).ceil().max(48.0), |ui, rect| {
            cell(ui, rect, 0.0, 0, rect.width(), true, |ui| widgets::caption(ui, &caption));
            if let Some(v) = cell(ui, rect, 0.0, 1, 48.0, true, |ui| {
                NumField::number("ccols", Some(columns as f64), "", 0).width(48.0).spinner().range(1.0, 40.0).show(ui)
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
        let group = ctx.read_response(egui::Id::unique(("object_control", "dimensions"))).unwrap().rect;
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
        let group = ctx.read_response(egui::Id::unique(("object_control", "dimensions"))).unwrap().rect;
        click(&ctx, &mut app, group.min + vec2(24.0, FIELD_H / 4.0));
        assert_eq!(app.ui.status, status, "disabled spinners do not attempt transforms");
        assert!(panels::sel_info(&app).is_none());
    }

    #[test]
    fn narrow_windows_can_scroll_to_the_alignment_controls() {
        let (ctx, mut app) = fixture(true);
        frame_with_width(&ctx, &mut app, vec![], 900.0);
        frame_with_width(&ctx, &mut app, vec![], 900.0);
        let id = egui::Id::unique(("object_control", "align"));
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
                    source: egui::MouseWheelSource::Unknown,
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

#[cfg(all(test, not(target_arch = "wasm32")))]
mod auto_fit_workflow_tests {
    use super::*;
    use craft_ui_widgets::runtime::inspect::{Element, Recorded, record};
    use designcraft_doc::{Document, Fitting, Item, ItemId};
    use egui::{Event, Key, Modifiers, PointerButton, UiBuilder};
    use egui_kittest::{Harness, kittest::{NodeT, Queryable}};
    use std::{collections::HashSet, sync::Arc};

    struct Rig {
        app: DesignApp,
        controls: Option<Arc<Recorded>>,
        enabled: bool,
        sizing: bool,
        canvas: bool,
        toolbar: Option<Rect>,
    }
    type Mark = (Arc<Document>, String, (usize, usize));

    fn step(h: &mut Harness<'_, Rig>) {
        let mut raw = std::mem::take(h.input_mut());
        let ctx = h.ctx.clone();
        h.state_mut().app.raw_input_hook(&ctx, &mut raw);
        *h.input_mut() = raw;
        h.step();
    }
    fn idle(h: &mut Harness<'_, Rig>, frames: usize) {
        for _ in 0..frames { step(h); }
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
    fn click_at(h: &mut Harness<'_, Rig>, pos: egui::Pos2) {
        pointer(h, pos, true);
        pointer(h, pos, false);
        idle(h, 2);
    }
    fn harness(mut app: DesignApp, installed: bool) -> Harness<'static, Rig> {
        app.run("window.workspace", json!({"name":"Advanced"})).unwrap();
        assert!(app.ui.control_bar);
        app.framework_opacity = true;
        let ctx = egui::Context::default();
        if installed { app.install_framework(&ctx); }
        assert_eq!(app.framework_installed, installed);
        app.raw_input_hook(&ctx, &mut egui::RawInput::default());
        let mut ready = false;
        let mut h = Harness::builder().with_context(ctx).with_size(vec2(1800.0, 900.0)).build_ui_state(
            move |ui, state: &mut Rig| {
                if !ready {
                    crate::theme::install_fonts(ui.ctx(), &state.app.ui.language);
                    crate::theme::apply(ui.ctx(), &Tokens::for_brightness(state.app.ui.brightness));
                    ready = true;
                    return;
                }
                crate::canvas::begin_frame(ui.ctx());
                let builder = if state.sizing { UiBuilder::new().sizing_pass() } else { UiBuilder::new() };
                ui.scope_builder(builder, |ui| {
                    ui.add_enabled_ui(state.enabled, |ui| {
                        // The shipping parent, its top Panel, horizontal scroller and fixed cells all run.
                        let parent_id = ui.scope_id().with("control_bar");
                        state.controls = record(ui, egui::Id::unique("object-auto-fit-workflow"), |ui| crate::chrome::control_bar(&mut state.app, ui)).1;
                        state.toolbar = egui::containers::panel::PanelState::load(ui.ctx(), parent_id).map(|panel| panel.outer_rect);
                        if state.canvas {
                            egui::CentralPanel::default().show(ui, |ui| crate::canvas::show(&mut state.app, ui));
                        }
                    });
                });
            },
            Rig { app, controls: None, enabled: true, sizing: false, canvas: false, toolbar: None },
        );
        h.input_mut().max_texture_side = Some(8192);
        idle(&mut h, 4);
        h
    }
    fn fixture() -> (DesignApp, [u64; 3]) {
        let mut session = designcraft_engine::Session::new();
        session.execute("file.new", &json!({})).unwrap();
        let png = designcraft_render::Rendered { width: 40, height: 20, pixels: vec![200; 40 * 20 * 4] }.to_png();
        let mut ids = Vec::new();
        for x in [100, 400, 700] {
            let id = session.execute("frame.create", &json!({"rect":[x,100,x+200,300]})).unwrap()["id"].as_u64().unwrap();
            session.execute("file.place", &json!({"base64":designcraft_engine::cmd::base64_encode(&png), "name":"auto-fit.png", "frame":id})).unwrap();
            ids.push(id);
        }
        // Keep both real history stacks populated, so an idle draw cannot hide a stray edit.
        session.execute("frame.create", &json!({"rect":[20,400,40,420]})).unwrap();
        session.execute("edit.undo", &json!({})).unwrap();
        session.execute("selection.set", &json!({"ids":[ids[0]]})).unwrap();
        assert!(!session.active().unwrap().history.undo.is_empty());
        assert_eq!(session.active().unwrap().history.redo.len(), 1);
        (DesignApp::new_for_panel_tests(session, crate::Services::default()), ids.try_into().unwrap())
    }
    fn mark(h: &Harness<'_, Rig>) -> Mark {
        let st = h.state().app.session.active().unwrap();
        (st.doc.clone(), format!("{:?}", (st.uid, st.identity_epoch, &st.selection.items, &st.selection.text, &st.selection.cells)),
            (st.history.undo.len(), st.history.redo.len()))
    }
    fn unchanged(h: &Harness<'_, Rig>, before: &Mark) {
        assert_eq!(mark(h), *before, "document, identity, selection and both real history stacks remain unchanged");
    }
    fn item(h: &Harness<'_, Rig>, id: u64) -> Item {
        h.state().app.session.active().unwrap().doc.item(ItemId(id)).unwrap().clone()
    }
    fn checkbox(h: &Harness<'_, Rig>) -> Element {
        let controls = h.state().controls.as_ref().unwrap();
        let found: Vec<_> = controls.elements.iter().filter(|e| e.component == "Checkbox" && e.part.is_none()
            && e.facts.as_ref().is_some_and(|f| f.name.as_deref() == Some("Auto-Fit"))).cloned().collect();
        assert_eq!(found.len(), 1, "one actual installed SDK Auto-Fit Checkbox in the complete toolbar");
        let field = found.into_iter().next().unwrap();
        assert_eq!(field.facts.as_ref().unwrap().role, format!("{:?}", egui::accesskit::Role::CheckBox));
        field
    }
    fn checked(h: &Harness<'_, Rig>, expected: bool) {
        let node = h.get_by(|n| n.role() == egui::accesskit::Role::CheckBox && n.label().as_deref() == Some("Auto-Fit"));
        assert_eq!(node.accesskit_node().toggled(), Some(if expected { egui::accesskit::Toggled::True } else { egui::accesskit::Toggled::False }));
    }
    fn fitting_cell(h: &Harness<'_, Rig>) -> Rect {
        let group = h.ctx.read_response(egui::Id::unique(("object_control", "fitting"))).unwrap().rect;
        assert_eq!(group.size(), vec2(116.0, HEIGHT));
        let cell = Rect::from_min_size(group.min + vec2(0.0, ROW_PITCH), vec2(116.0, FIELD_H));
        assert_eq!(cell.height(), 21.0);
        let parent = h.state().toolbar.expect("the shipping top Panel persisted its actual outer rectangle");
        assert!((parent.height() - 59.0).abs() < 1e-6, "measure the shipping parent, not a test-owned substitute: {parent:?}");
        assert!(group.contains_rect(cell) && parent.contains_rect(group) && parent.contains_rect(cell),
            "actual second-row cell stays inside the unchanged parent toolbar: {cell:?}, {parent:?}");
        cell
    }
    fn painted_label(h: &Harness<'_, Rig>) -> Rect {
        fn collect(shape: &egui::Shape, clip: Rect, labels: &mut Vec<(Rect, Rect)>) {
            match shape {
                egui::Shape::Text(t) if t.galley.job.text == "Auto-Fit" => labels.push((t.visual_bounding_rect(), clip)),
                egui::Shape::Vec(shapes) => for shape in shapes { collect(shape, clip, labels); },
                _ => {}
            }
        }
        let mut labels = Vec::new();
        for shape in &h.output().shapes { collect(&shape.shape, shape.clip_rect, &mut labels); }
        assert_eq!(labels.len(), 1, "measure the real full painted caption, not an assumed checkbox width");
        let (label, clip) = labels[0];
        assert!(label.is_finite() && label.width() > 0.0 && label.height() > 0.0);
        assert!(fitting_cell(h).expand(0.5).contains_rect(label) && clip.expand(0.5).contains_rect(label),
            "the complete caption is inside its real cell and paint clip, not merely a clipped taller row: {label:?}, {clip:?}");
        label
    }
    fn native_geometry(h: &Harness<'_, Rig>) -> (Element, Rect) {
        let field = checkbox(h);
        let cell = fitting_cell(h);
        assert!(cell.expand(0.5).contains_rect(field.rect), "the real SDK control fits the original 21-point cell: {:?}", field.rect);
        let label = painted_label(h);
        let controls = h.state().controls.as_ref().unwrap();
        let hits: Vec<_> = controls.hits.iter().filter(|hit| hit.owner == field.id && hit.part.is_none()).collect();
        assert!(!hits.is_empty(), "the SDK registered its real pointer hit target");
        assert!(hits.iter().all(|hit| cell.expand(0.5).contains_rect(hit.rect)), "hit targets do not intrude into the fitting actions above or outside the cell");
        assert!(hits.iter().any(|hit| hit.rect.contains(label.center())), "the visible caption is genuinely clickable");
        let node = h.get_by(|n| n.role() == egui::accesskit::Role::CheckBox && n.label().as_deref() == Some("Auto-Fit"));
        assert!(cell.expand(0.5).contains_rect(node.rect()) && field.rect.contains(node.rect().center()),
            "the SDK's named indicator AX node stays inside its real cell; the separate label claim is checked above");
        (field, label)
    }
    fn tab_to_checkbox(h: &mut Harness<'_, Rig>) {
        let mut visited = HashSet::new();
        loop {
            if h.get_by(|n| n.role() == egui::accesskit::Role::CheckBox && n.label().as_deref() == Some("Auto-Fit")).is_focused() {
                if h.state().app.framework_installed { assert_eq!(h.ctx.memory(|m| m.focused()), Some(checkbox(h).id)); }
                return;
            }
            assert!(visited.insert(h.ctx.memory(|m| m.focused())), "natural Tab traversal of the real toolbar reaches Auto-Fit");
            key(h, Key::Tab);
        }
    }
    fn journal_one(h: &Harness<'_, Rig>, start: usize, value: bool) {
        assert_eq!(&h.state().app.session.journal[start..], &[("object.fittingOptions".to_owned(), json!({"autoFit":value}))]);
    }
    fn click_auto_fit(h: &mut Harness<'_, Rig>) {
        let label = painted_label(h);
        click_at(h, label.center());
    }

    #[test]
    fn installed_auto_fit_enable_resize_move_disable_and_each_real_undo_redo() {
        let (app, ids) = fixture();
        let mut h = harness(app, true);
        let before = mark(&h);
        let untouched = [item(&h, ids[1]), item(&h, ids[2])];
        native_geometry(&h);
        checked(&h, false);
        let mut documents = vec![before.0.clone()];
        let start = h.state().app.session.journal.len();
        click_auto_fit(&mut h);
        journal_one(&h, start, true);
        checked(&h, true);
        assert_eq!(item(&h, ids[0]).graphic().unwrap().auto_fit, Fitting::FillProportionally);
        documents.push(mark(&h).0);

        // The real Selection-tool handle reaches transform.resize, which owns downstream refitting.
        // The unchanged W/H numeric path is transform.set and is deliberately not substituted for it.
        h.state_mut().canvas = true;
        idle(&mut h, 3);
        let frame_before = item(&h, ids[0]).bounds();
        let content_before_resize = item(&h, ids[0]).graphic().unwrap().xf;
        let st = h.state().app.session.active().unwrap();
        let layout = h.state().app.session.layout();
        let (spread_to_canvas, _) = crate::canvas::item_canvas_xf(&st.doc, &layout, ItemId(ids[0])).unwrap();
        let xf = crate::canvas::Xf::new(h.state().app.canvas_rect.unwrap(), h.state().app.view().unwrap());
        let from = xf.to_screen(spread_to_canvas * designcraft_geom::Point::new(frame_before.x1, frame_before.center().y));
        let to = from + vec2(80.0 * xf.zoom as f32, 0.0);
        assert!(h.state().app.canvas_rect.unwrap().contains(from) && h.state().app.canvas_rect.unwrap().contains(to));
        let start = h.state().app.session.journal.len();
        pointer(&mut h, from, true);
        for fraction in [0.25, 0.5, 0.75, 1.0] {
            event(&mut h, Event::PointerMoved(from + (to - from) * fraction));
        }
        pointer(&mut h, to, false);
        idle(&mut h, 3);
        let frame_after = item(&h, ids[0]).bounds();
        assert!((frame_after.width() - frame_before.width() - 80.0).abs() < 1e-3,
            "the actual frame handle resizes, rather than moving or scaling its content: {frame_before:?} -> {frame_after:?}");
        assert!((frame_after.height() - frame_before.height()).abs() < 1e-6);
        let commands = &h.state().app.session.journal[start..];
        assert_eq!(commands.len(), 1, "one committed command from the genuine multi-pass handle gesture");
        assert_eq!(commands[0], ("transform.resize".to_owned(), json!({
            "from":[frame_before.x0,frame_before.y0,frame_before.x1,frame_before.y1],
            "to":[frame_after.x0,frame_after.y0,frame_after.x1,frame_after.y1], "content":false,"distribute":false
        })));
        let resized = item(&h, ids[0]);
        let graphic = resized.graphic().unwrap();
        assert_eq!(graphic.xf, graphic.fitted(resized.inner_bounds(), graphic.auto_fit).unwrap(), "the real frame resize refits the graphic downstream");
        assert_ne!(graphic.xf, content_before_resize, "proves a changed graphic transform, not just the Auto-Fit flag");
        documents.push(mark(&h).0);

        // A real content-move command establishes the moved picture that turning Auto-Fit off must preserve.
        h.state_mut().app.run("transform.move", json!({"dx":7.0,"dy":9.0,"content":true})).unwrap();
        idle(&mut h, 2);
        let moved = item(&h, ids[0]).graphic().unwrap().xf;
        assert_ne!(moved, graphic.xf);
        documents.push(mark(&h).0);
        // The resize focused the real Canvas, whose native Tab lock intentionally keeps Tab.
        // Re-enter a different real toolbar editor by pointer before testing toolbar traversal.
        assert_eq!(h.state().app.session.tool_id(), "selection");
        let canvas_owner = h.ctx.memory(|m| m.focused()).expect("the completed Canvas gesture owns focus");
        let canvas_response = h.ctx.read_response(canvas_owner).expect("the focused Canvas response remains live");
        assert!(canvas_response.rect.contains_rect(h.state().app.canvas_rect.unwrap()),
            "Canvas focus covers the real view, including its ruler margin");
        let traversal_before = mark(&h);
        let traversal_start = h.state().app.session.journal.len();
        let entry = h.get_by_role_and_label(egui::accesskit::Role::TextInput, "cx").rect();
        assert!(h.state().toolbar.unwrap().contains_rect(entry), "the unique cx editor is in the actual toolbar");
        click_at(&mut h, entry.center());
        assert!(h.get_by_role_and_label(egui::accesskit::Role::TextInput, "cx").is_focused());
        assert!(h.ctx.text_edit_focused(), "ordinary pointer input entered the real numeric TextEdit");
        let entry_owner = h.ctx.memory(|m| m.focused()).expect("the toolbar editor owns actual focus");
        assert_ne!(entry_owner, canvas_owner);
        let entry_response = h.ctx.read_response(entry_owner).expect("the focused toolbar editor response is live");
        assert!(entry.expand(0.5).contains_rect(entry_response.rect) && entry_response.rect.expand(0.5).contains_rect(entry),
            "AX focus and egui focus identify the same inner editor, not its spinner or full field slot");
        unchanged(&h, &traversal_before);
        assert_eq!(h.state().app.session.journal.len(), traversal_start, "toolbar entry alone submits no command");
        tab_to_checkbox(&mut h);
        unchanged(&h, &traversal_before);
        assert_eq!(h.state().app.session.journal.len(), traversal_start, "natural toolbar traversal submits no command");
        let start = h.state().app.session.journal.len();
        key(&mut h, Key::Space);
        idle(&mut h, 2);
        journal_one(&h, start, false);
        checked(&h, false);
        assert_eq!(item(&h, ids[0]).graphic().unwrap().xf, moved, "disable alone must not recenter or refit moved content");
        assert_eq!(item(&h, ids[0]).graphic().unwrap().auto_fit, Fitting::None);
        documents.push(mark(&h).0);
        assert_eq!(mark(&h).2, (before.2.0 + 4, 0), "enable, resize, move and disable are four independent real edits");
        assert_eq!([item(&h, ids[1]), item(&h, ids[2])], untouched);
        for i in (0..4).rev() {
            h.state_mut().app.run("edit.undo", json!({})).unwrap();
            idle(&mut h, 2);
            assert_eq!(mark(&h).0, documents[i], "one real Undo restores precisely operation {i}'s preimage");
            assert_eq!(mark(&h).2, (before.2.0 + i, 4 - i));
        }
        for i in 1..=4 {
            h.state_mut().app.run("edit.redo", json!({})).unwrap();
            idle(&mut h, 2);
            assert_eq!(mark(&h).0, documents[i]);
            assert_eq!(mark(&h).2, (before.2.0 + i, 4 - i));
        }
        assert_eq!([item(&h, ids[1]), item(&h, ids[2])], untouched);
    }

    #[test]
    fn installed_auto_fit_real_parent_geometry_idle_label_and_hit_bounds() {
        let (app, _) = fixture();
        let mut h = harness(app, true);
        let before = mark(&h);
        let status = h.state().app.ui.status.clone();
        let start = h.state().app.session.journal.len();
        let old = native_geometry(&h).0.id;
        idle(&mut h, 8);
        assert_eq!(native_geometry(&h).0.id, old);
        unchanged(&h, &before);
        assert_eq!(h.state().app.ui.status, status);
        assert_eq!(h.state().app.session.journal.len(), start);
    }

    #[test]
    fn installed_auto_fit_keeps_first_selected_display_and_exact_selected_graphic_targets() {
        let (mut app, ids) = fixture();
        app.run("object.fittingOptions", json!({"ids":[ids[1]],"autoFit":true,"fitting":"fitProportionally"})).unwrap();
        app.run("selection.set", json!({"ids":[ids[0],ids[1]]})).unwrap();
        let mut h = harness(app, true);
        checked(&h, false); // Not mixed: the original first-selected-graphic policy.
        let untouched = item(&h, ids[2]);
        let before = mark(&h);
        let start = h.state().app.session.journal.len();
        click_auto_fit(&mut h);
        journal_one(&h, start, true);
        assert_eq!(item(&h, ids[0]).graphic().unwrap().auto_fit, Fitting::FillProportionally);
        assert_eq!(item(&h, ids[1]).graphic().unwrap().auto_fit, Fitting::FitProportionally, "keep an existing fitting mode");
        assert_eq!(item(&h, ids[2]), untouched);
        assert_eq!(mark(&h).2, (before.2.0 + 1, 0));
        h.state_mut().app.run("edit.undo", json!({})).unwrap();
        idle(&mut h, 2);
        assert_eq!(mark(&h).0, before.0);
        h.state_mut().app.run("selection.set", json!({"ids":[ids[1],ids[0]]})).unwrap();
        idle(&mut h, 2);
        checked(&h, true);
    }

    #[test]
    fn installed_auto_fit_natural_space_and_enter_each_submit_once() {
        let (app, ids) = fixture();
        let mut h = harness(app, true);
        for (key_name, value) in [(Key::Space, true), (Key::Enter, false)] {
            tab_to_checkbox(&mut h);
            let before = mark(&h);
            let start = h.state().app.session.journal.len();
            key(&mut h, key_name);
            idle(&mut h, 4);
            journal_one(&h, start, value);
            checked(&h, value);
            assert_eq!(mark(&h).2, (before.2.0 + 1, 0));
            assert_eq!(item(&h, ids[0]).graphic().unwrap().auto_fit != Fitting::None, value);
        }
    }

    #[test]
    fn installed_auto_fit_held_pointer_retires_on_selection_expansion_reorder_and_replacement() {
        let (app, ids) = fixture();
        let mut h = harness(app, true);
        for selected in [vec![ids[0],ids[1]], vec![ids[1],ids[0]], vec![ids[2]]] {
            let (old, label) = native_geometry(&h);
            let start = h.state().app.session.journal.len();
            pointer(&mut h, label.center(), true);
            assert_eq!(h.state().app.session.journal.len(), start, "activation waits for the release");
            h.state_mut().app.run("selection.set", json!({"ids":selected})).unwrap();
            idle(&mut h, 2);
            let before = mark(&h);
            let start = h.state().app.session.journal.len();
            assert_ne!(checkbox(&h).id, old.id);
            pointer(&mut h, label.center(), false);
            idle(&mut h, 2);
            unchanged(&h, &before);
            assert_eq!(h.state().app.session.journal.len(), start);
        }
        let start = h.state().app.session.journal.len();
        click_auto_fit(&mut h);
        journal_one(&h, start, true);
        assert_eq!(item(&h, ids[2]).graphic().unwrap().auto_fit, Fitting::FillProportionally);
        assert_eq!(item(&h, ids[0]).graphic().unwrap().auto_fit, Fitting::None);
    }

    #[test]
    fn installed_auto_fit_held_pointer_cannot_cross_documents_reusing_item_ids() {
        let (app, ids) = fixture();
        let mut h = harness(app, true);
        let original = mark(&h);
        let (old, label) = native_geometry(&h);
        pointer(&mut h, label.center(), true);
        h.state_mut().app.run("file.new", json!({})).unwrap();
        let other = h.state_mut().app.run("frame.create", json!({"rect":[100,100,300,300]})).unwrap()["id"].as_u64().unwrap();
        assert_eq!(other, ids[0], "real new documents reuse document-local IDs");
        let png = designcraft_render::Rendered { width:40, height:20, pixels:vec![200;40*20*4] }.to_png();
        h.state_mut().app.run("file.place", json!({"base64":designcraft_engine::cmd::base64_encode(&png),"name":"other.png","frame":other})).unwrap();
        idle(&mut h, 2);
        let before = mark(&h);
        let start = h.state().app.session.journal.len();
        assert_ne!(checkbox(&h).id, old.id);
        pointer(&mut h, label.center(), false);
        idle(&mut h, 2);
        unchanged(&h, &before);
        assert_eq!(h.state().app.session.journal.len(), start);
        h.state_mut().app.session.execute("file.close", &json!({})).unwrap();
        idle(&mut h, 2);
        unchanged(&h, &original);
        let start = h.state().app.session.journal.len();
        click_auto_fit(&mut h);
        journal_one(&h, start, true);
    }

    #[test]
    fn installed_auto_fit_held_pointer_cannot_cross_revert_with_same_uid_and_item_ids() {
        let (mut app, ids) = fixture();
        let dir = tempfile::tempdir().unwrap();
        app.run("file.saveAs", json!({"path":dir.path().join("AutoFit.designcraft")})).unwrap();
        app.run("transform.move", json!({"dx":7.0,"dy":9.0,"content":true})).unwrap();
        let mut h = harness(app, true);
        let uid = h.state().app.session.active().unwrap().uid;
        let epoch = h.state().app.session.active().unwrap().identity_epoch;
        let (old, label) = native_geometry(&h);
        pointer(&mut h, label.center(), true);
        h.state_mut().app.session.execute("file.revert", &json!({})).unwrap();
        h.state_mut().app.run("selection.set", json!({"ids":[ids[0]]})).unwrap();
        idle(&mut h, 2);
        assert_eq!(h.state().app.session.active().unwrap().uid, uid);
        assert_ne!(h.state().app.session.active().unwrap().identity_epoch, epoch);
        assert_ne!(checkbox(&h).id, old.id, "the new incarnation must not inherit a held activation merely because uid/items match");
        let before = mark(&h);
        let start = h.state().app.session.journal.len();
        pointer(&mut h, label.center(), false);
        idle(&mut h, 2);
        unchanged(&h, &before);
        assert_eq!(h.state().app.session.journal.len(), start);
        click_auto_fit(&mut h);
        journal_one(&h, start, true);
    }

    #[test]
    fn installed_auto_fit_disabled_empty_non_graphic_and_no_document_never_submit() {
        let (mut app, _) = fixture();
        let text = app.run("frame.create", json!({"rect":[100,400,200,500],"content":"text","text":"Body"})).unwrap()["id"].as_u64().unwrap();
        let mut h = harness(app, true);
        for selected in [vec![], vec![text]] {
            h.state_mut().app.run("selection.set", json!({"ids":selected})).unwrap();
            idle(&mut h, 2);
            assert!(!panels::sel_info(&h.state().app).is_some_and(|info| info.is_graphic));
            assert!(checkbox(&h).facts.unwrap().disabled);
            let label = painted_label(&h);
            let before = mark(&h);
            let start = h.state().app.session.journal.len();
            click_at(&mut h, label.center());
            idle(&mut h, 4);
            unchanged(&h, &before);
            assert_eq!(h.state().app.session.journal.len(), start);
        }
        let app = DesignApp::new_for_panel_tests(designcraft_engine::Session::new(), crate::Services::default());
        let mut empty = harness(app, true);
        assert!(checkbox(&empty).facts.unwrap().disabled);
        let label = painted_label(&empty);
        let start = empty.state().app.session.journal.len();
        click_at(&mut empty, label.center());
        key(&mut empty, Key::Tab);
        key(&mut empty, Key::Space);
        key(&mut empty, Key::Enter);
        assert!(empty.state().app.session.active().is_none());
        assert_eq!(empty.state().app.session.journal.len(), start);
    }

    #[test]
    fn installed_auto_fit_disabled_or_sizing_parent_cannot_commit_then_recovers() {
        let (app, _) = fixture();
        let mut h = harness(app, true);
        for sizing in [false, true] {
            let label = painted_label(&h);
            pointer(&mut h, label.center(), true);
            h.state_mut().enabled = sizing;
            h.state_mut().sizing = sizing;
            idle(&mut h, 2);
            let before = mark(&h);
            let start = h.state().app.session.journal.len();
            pointer(&mut h, label.center(), false);
            idle(&mut h, 2);
            unchanged(&h, &before);
            assert_eq!(h.state().app.session.journal.len(), start);
            h.state_mut().enabled = true;
            h.state_mut().sizing = false;
            idle(&mut h, 3);
        }
        let start = h.state().app.session.journal.len();
        click_auto_fit(&mut h);
        journal_one(&h, start, true);
    }

    #[test]
    fn uninstalled_auto_fit_keeps_legacy_checkbox_pointer_keyboard_command_and_undo() {
        let (app, ids) = fixture();
        let mut h = harness(app, false);
        assert!(h.state().controls.as_ref().unwrap().elements.iter().all(|e| e.component != "Checkbox"), "no uninstalled SDK control is drawn");
        let before = mark(&h);
        painted_label(&h);
        let start = h.state().app.session.journal.len();
        click_auto_fit(&mut h);
        journal_one(&h, start, true);
        checked(&h, true);
        assert_eq!(item(&h, ids[0]).graphic().unwrap().auto_fit, Fitting::FillProportionally);
        assert_eq!(mark(&h).2, (before.2.0 + 1, 0));
        h.state_mut().app.run("edit.undo", json!({})).unwrap();
        idle(&mut h, 2);
        assert_eq!(mark(&h).0, before.0);
        tab_to_checkbox(&mut h);
        let start = h.state().app.session.journal.len();
        key(&mut h, Key::Space);
        idle(&mut h, 2);
        journal_one(&h, start, true);
        checked(&h, true);
        assert!(!h.state().app.framework_installed);
    }

    #[test]
    fn uninstalled_auto_fit_held_pointer_retires_before_a_new_selected_frame_can_receive_it() {
        let (app, ids) = fixture();
        let mut h = harness(app, false);
        let label = painted_label(&h);
        pointer(&mut h, label.center(), true);
        h.state_mut().app.run("selection.set", json!({"ids":[ids[1]]})).unwrap();
        idle(&mut h, 2);
        let before = mark(&h);
        let start = h.state().app.session.journal.len();
        pointer(&mut h, label.center(), false);
        idle(&mut h, 2);
        unchanged(&h, &before);
        assert_eq!(h.state().app.session.journal.len(), start);
        click_auto_fit(&mut h);
        journal_one(&h, start, true);
        assert_eq!(item(&h, ids[0]).graphic().unwrap().auto_fit, Fitting::None);
        assert_eq!(item(&h, ids[1]).graphic().unwrap().auto_fit, Fitting::FillProportionally);
    }
}
