//! Contextual Task Bar (InDesign 2026 §6): a floating 36 pt HUD just below the selection with the
//! most common actions for it. Window > Contextual Task Bar toggles it (on by default).

use egui::{Rect, Sense, Stroke, StrokeKind, Ui, pos2, vec2};
use serde_json::json;

use crate::theme::Tokens;
use crate::widgets::{self, FIELD_H, NumField};
use crate::{DesignApp, icons, panels};

pub const HEIGHT: f32 = 36.0;

/// The grip that moves the bar.
const GRIP: &str = "task_bar_grip";

/// Show the bar under `sel` (screen rect of the selection), or where it is pinned, kept inside
/// `canvas`.
pub fn show(app: &mut DesignApp, ctx: &egui::Context, sel: Rect, canvas: Rect) {
    let grip_id = egui::Id::new(GRIP);
    // (Two separate context calls: `is_pointer_over_egui` inside an `input` closure deadlocks.)
    let dragging = ctx.dragged_id() == Some(grip_id);
    if !app.ui.task_bar || (ctx.input(|i| i.pointer.any_down()) && !ctx.is_pointer_over_egui() && !dragging) {
        return;
    }
    let Some(st) = app.session.active() else { return };
    let has_text = st.selection.text.is_some();
    if st.selection.items.is_empty() && !has_text {
        return;
    }
    let t = Tokens::get(ctx);
    let id = egui::Id::new("task_bar");
    // Width from the previous frame (content-sized).
    let w = ctx.memory(|m| m.area_rect(id).map(|r| r.width())).unwrap_or(420.0);
    let at = match app.ui.task_bar_pin {
        Some([x, y]) => inside(canvas.min + vec2(x, y), w, canvas),
        None => {
            let mut x = sel.center().x - w / 2.0;
            x = x.clamp(canvas.min.x + 8.0, (canvas.max.x - w - 8.0).max(canvas.min.x + 8.0));
            let mut y = sel.max.y + 14.0;
            if y + HEIGHT > canvas.max.y - 8.0 {
                y = (sel.min.y - HEIGHT - 14.0).max(canvas.min.y + 8.0);
            }
            if y + HEIGHT > canvas.max.y - 4.0 {
                y = canvas.max.y - HEIGHT - 8.0;
            }
            pos2(x, y)
        }
    };
    app.ui.task_bar_at = Some([at.x - canvas.min.x, at.y - canvas.min.y]);
    egui::Area::new(id).order(egui::Order::Middle).fixed_pos(at).show(ctx, |ui| {
        egui::Frame::NONE
            .fill(t.panel)
            .stroke(Stroke::new(1.0, t.border))
            .corner_radius(6)
            .shadow(egui::epaint::Shadow { offset: [0, 2], blur: 8, spread: 0, color: egui::Color32::from_black_alpha(80) })
            .inner_margin(egui::Margin { left: 4, right: 8, top: 0, bottom: 0 })
            .show(ui, |ui| {
                ui.set_height(HEIGHT);
                ui.horizontal_centered(|ui| {
                    ui.spacing_mut().item_spacing.x = 6.0;
                    // Dragging the grip moves the bar, and pins it where it is dropped.
                    let (g, _) = ui.allocate_exact_size(vec2(12.0, 20.0), Sense::hover());
                    let grip = ui.interact(g, grip_id, Sense::drag());
                    icons::paint(ui.painter(), g, "grip", if grip.hovered() || grip.dragged() { t.text_strong } else { t.icon });
                    if grip.dragged() {
                        ui.ctx().set_cursor_icon(egui::CursorIcon::Grabbing);
                        let to = inside(at + grip.drag_delta(), w, canvas) - canvas.min;
                        let _ = app.run("window.taskBarPin", json!({"at": [to.x, to.y]}));
                    } else if grip.hovered() {
                        ui.ctx().set_cursor_icon(egui::CursorIcon::Grab);
                    }
                    let text_frame = panels::sel_info(app).is_some_and(|i| i.is_text && i.count == 1);
                    if has_text || text_frame {
                        text_controls(app, ui, has_text);
                    } else {
                        object_controls(app, ui);
                    }
                    more(app, ui);
                });
            });
    });
}

/// `at` moved as little as needed for a bar `w` wide to sit inside `canvas` (8 pt in).
fn inside(at: egui::Pos2, w: f32, canvas: Rect) -> egui::Pos2 {
    let x = at.x.min(canvas.max.x - w - 8.0).max(canvas.min.x + 8.0);
    let y = at.y.min(canvas.max.y - HEIGHT - 8.0).max(canvas.min.y + 8.0);
    pos2(x, y)
}

fn split_button(ui: &mut Ui, icon: &str, label: &str) -> (egui::Response, egui::Response) {
    let t = Tokens::get(ui.ctx());
    let g = crate::rtl::plain(ui.ctx(), label, egui::FontId::proportional(11.0), t.text);
    let w = 26.0 + g.size().x + 8.0 + widgets::SEG_W;
    let (r, _) = ui.allocate_exact_size(vec2(w, FIELD_H), Sense::hover());
    let main = Rect::from_min_max(r.min, pos2(r.max.x - widgets::SEG_W, r.max.y));
    let seg = Rect::from_min_max(pos2(main.max.x, r.min.y), r.max);
    let mr = ui.interact(main, ui.id().with(("split", label)), Sense::click());
    let sr = ui.interact(seg, ui.id().with(("splitseg", label)), Sense::click());
    for (rr, resp) in [(main, &mr), (seg, &sr)] {
        if resp.hovered() {
            ui.painter().rect_filled(rr, 0.0, t.hover);
        }
    }
    icons::paint(ui.painter(), Rect::from_min_size(main.min + vec2(5.0, 2.5), vec2(16.0, 16.0)), icon, t.icon);
    ui.painter().galley(pos2(main.min.x + 26.0, main.center().y - g.size().y / 2.0), g, t.text);
    widgets::chevron_down(ui.painter(), seg.center(), t.icon);
    ui.painter().rect_stroke(r, 0.0, Stroke::new(1.0, t.field_border), StrokeKind::Inside);
    ui.painter().line_segment([seg.left_top(), seg.left_bottom()], Stroke::new(1.0, t.field_border));
    (mr, sr)
}

fn text_controls(app: &mut DesignApp, ui: &mut Ui, in_text: bool) {
    let t = Tokens::get(ui.ctx());
    if !in_text {
        let (main, seg) = split_button(ui, "fit-text", crate::i18n::tr(&app.ui.language, "Fit Text"));
        if main
            .on_hover_ui(|ui| {
                crate::rtl::label(ui, crate::i18n::tr(&app.ui.language, "Fit frame height to text"));
            })
            .clicked()
        {
            let _ = app.run("object.textFrameOptions", json!({"autoSize": "heightOnly"}));
        }
        egui::Popup::menu(&seg).show(|ui| {
            for (label, v) in [("Off", "off"), ("Height Only", "heightOnly"), ("Width Only", "widthOnly"), ("Height and Width", "heightAndWidth")] {
                if ui.button(crate::rtl::widget(ui, crate::i18n::tr(&app.ui.language, label))).clicked() {
                    let _ = app.run("object.textFrameOptions", json!({"autoSize": v}));
                    ui.close();
                }
            }
        });
        sep(ui);
    }
    let Some(a) = panels::text_attrs(app) else { return };
    let c = a["chars"].clone();
    let fam = c["fontFamily"].as_str().unwrap_or("").to_string();
    let sty = c["fontStyle"].as_str().unwrap_or("").to_string();
    let fonts = panels::fonts(app);
    let menu = panels::font_menu(app);
    let shown = if fam.is_empty() { "—".to_string() } else { panels::font_label(app, &menu, &fam) };
    let field = widgets::dropdown(ui, &shown, 150.0);
    if let Some(f) = panels::font_popup(app, &field, &menu, &fam) {
        panels::apply_font_family(app, &f);
    }
    let styles = fonts.styles(&fam);
    let cur = styles.iter().position(|s| *s == sty);
    if let Some(k) = widgets::dropdown_list(ui, if sty.is_empty() { "—" } else { &sty }, 90.0, &styles, cur) {
        let _ = app.run("type.char", json!({"attrs": {"fontStyle": styles[k]}}));
    }
    let (r, _) = ui.allocate_exact_size(vec2(18.0, 18.0), Sense::hover());
    icons::paint(ui.painter(), r, "font-size", t.icon);
    const SIZES: &[f64] = &[6.0, 8.0, 9.0, 10.0, 11.0, 12.0, 14.0, 18.0, 24.0, 30.0, 36.0, 48.0, 60.0, 72.0];
    if let Some(v) = NumField::number("tb_size", c["size"].as_f64(), " pt", 2).width(70.0).presets(SIZES).range(0.1, 1296.0).show(ui) {
        let _ = app.run("type.char", json!({"attrs": {"size": v}}));
    }
    sep(ui);
    // Text fill.
    if let Some(doc) = app.session.active().map(|d| d.doc.clone()) {
        let sw = c["fill"].as_str().unwrap_or("[Black]").to_string();
        let (col, g) = widgets::swatch_colors(&doc, &sw, 1.0);
        let resp = widgets::swatch_chip(ui, 17.0, col, g).on_hover_text(format!("Fill: {sw}"));
        resp.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, true, "Task Bar fill"));
        let mut picked = None;
        egui::Popup::menu(&resp).show(|ui| {
            ui.set_min_width(180.0);
            ui.set_max_width(280.0);
            egui::ScrollArea::vertical().max_height(300.0).show(ui, |ui| {
                for s in &doc.swatches {
                    if panels::swatch_menu_row(ui, &doc, &s.name, &sw) {
                        picked = Some(s.name.clone());
                        ui.close();
                    }
                }
            });
        });
        if let Some(p) = picked {
            let _ = app.run("type.char", json!({"attrs": {"fill": p}}));
        }
    }
    // Paragraph alignment.
    let al = a["para"]["align"].as_str().unwrap_or("left").to_string();
    let icon = match al.as_str() {
        "center" => "palign-center",
        "right" => "palign-right",
        "leftJustified" | "centerJustified" | "rightJustified" | "fullyJustified" => "palign-justify-left",
        _ => "palign-left",
    };
    let resp = widgets::icon_toggle(ui, icon, false, crate::i18n::tr(&app.ui.language, "Paragraph alignment"));
    egui::Popup::menu(&resp).show(|ui| {
        for (label, v) in [
            ("Align Left", "left"),
            ("Align Center", "center"),
            ("Align Right", "right"),
            ("Justify", "leftJustified"),
            ("Justify All", "fullyJustified"),
        ] {
            if ui.selectable_label(al == v, crate::rtl::widget(ui, crate::i18n::tr(&app.ui.language, label))).clicked() {
                let _ = app.run("type.para", json!({"attrs": {"align": v}}));
                ui.close();
            }
        }
    });
    if widgets::icon_toggle(ui, "frame-options", false, crate::i18n::tr(&app.ui.language, "Text Frame Options")).clicked() {
        app.ui.dialog = Some(crate::dialogs::Dialog::new("textFrameOptions", json!({})));
    }
}

fn object_controls(app: &mut DesignApp, ui: &mut Ui) {
    let Some(i) = panels::sel_info(app) else { return };
    let Some(doc) = app.session.active().map(|d| d.doc.clone()) else { return };
    if i.is_graphic {
        for (icon, mode, tip) in [
            ("fit-fill", "fillProportionally", "Fill Frame Proportionally"),
            ("fit-prop", "fitProportionally", "Fit Content Proportionally"),
            ("fit-frame", "fitFrameToContent", "Fit Frame to Content"),
            ("fit-center", "centerContent", "Center Content"),
        ] {
            if widgets::icon_toggle(ui, icon, false, crate::i18n::tr(&app.ui.language, tip)).clicked() {
                let _ = app.run("object.fit", json!({"mode": mode}));
            }
        }
        sep(ui);
    }
    for (k, (cmd, sw)) in [("object.fill", i.fill.clone()), ("object.stroke", i.stroke.clone())].into_iter().enumerate() {
        let (col, g) = widgets::swatch_colors(&doc, &sw, 1.0);
        let (r, resp) = ui.allocate_exact_size(vec2(17.0, 17.0), Sense::click());
        if k == 0 {
            widgets::paint_chip(ui.painter(), r, col, g);
        } else {
            widgets::paint_stroke_chip(ui.painter(), r, col, egui::Color32::from_gray(20));
        }
        let resp = resp.on_hover_text(format!("{}: {sw}", if k == 0 { "Fill" } else { "Stroke" }));
        resp.widget_info(|| {
            egui::WidgetInfo::labeled(egui::WidgetType::Button, true, format!("Task Bar {}", if k == 0 { "fill" } else { "stroke" }))
        });
        let mut picked = None;
        egui::Popup::menu(&resp).show(|ui| {
            ui.set_min_width(180.0);
            ui.set_max_width(280.0);
            egui::ScrollArea::vertical().max_height(300.0).show(ui, |ui| {
                for s in &doc.swatches {
                    if panels::swatch_menu_row(ui, &doc, &s.name, &sw) {
                        picked = Some(s.name.clone());
                        ui.close();
                    }
                }
            });
        });
        if let Some(p) = picked {
            let _ = app.run(cmd, json!({"swatch": p}));
        }
    }
    const WEIGHTS: &[f64] = &[0.25, 0.5, 1.0, 2.0, 3.0, 4.0, 6.0, 8.0, 10.0];
    if let Some(v) = NumField::number("tb_sw", Some(i.stroke_weight), " pt", 3).width(64.0).spinner().presets(WEIGHTS).range(0.0, 800.0).show(ui) {
        let _ = app.run("object.stroke", json!({"weight": v}));
    }
    sep(ui);
    for (cmd, icon, tip) in [("object.bringToFront", "objalign-top", "Bring to Front"), ("object.sendToBack", "objalign-bottom", "Send to Back")] {
        if widgets::icon_toggle(ui, icon, false, crate::i18n::tr(&app.ui.language, tip)).clicked() {
            let _ = app.run(cmd, json!({}));
        }
    }
    if i.count > 1 && widgets::outline_button(ui, crate::i18n::tr(&app.ui.language, "Group"), 52.0).clicked() {
        let _ = app.run("object.group", json!({}));
    }
}

fn more(app: &mut DesignApp, ui: &mut Ui) {
    let t = Tokens::get(ui.ctx());
    let (r, resp) = ui.allocate_exact_size(vec2(22.0, FIELD_H), Sense::click());
    if resp.hovered() {
        ui.painter().rect_filled(r, 2.0, t.hover);
    }
    for k in 0..3 {
        ui.painter().circle_filled(pos2(r.min.x + 4.0 + k as f32 * 7.0, r.center().y), 1.8, t.icon);
    }
    let resp = resp.on_hover_ui(|ui| {
        crate::rtl::label(ui, crate::i18n::tr(&app.ui.language, "More Options"));
    });
    egui::Popup::menu(&resp).show(|ui| {
        if ui.button(crate::rtl::widget(ui, crate::i18n::tr(&app.ui.language, "Hide Bar"))).clicked() {
            app.ui.task_bar = false;
            ui.close();
        }
        let pinned = app.ui.task_bar_pin.is_some();
        let pin = crate::i18n::tr(&app.ui.language, "Pin Bar Position");
        if ui.button(crate::rtl::widget(ui, if pinned { format!("✓ {pin}") } else { format!("   {pin}") })).clicked() {
            let _ = app.run("window.taskBarPin", json!({"on": !pinned}));
            ui.close();
        }
        if ui.add_enabled(pinned, egui::Button::new(crate::rtl::widget(ui, crate::i18n::tr(&app.ui.language, "Reset Bar Position")))).clicked() {
            let _ = app.run("window.taskBarReset", json!({}));
            ui.close();
        }
    });
}

fn sep(ui: &mut Ui) {
    let t = Tokens::get(ui.ctx());
    let (r, _) = ui.allocate_exact_size(vec2(1.0, 24.0), Sense::hover());
    ui.painter().line_segment([r.center_top(), r.center_bottom()], Stroke::new(1.0, t.border));
}

#[cfg(test)]
mod tests {
    use egui::{pos2, vec2};
    use egui_kittest::Harness;
    use egui_kittest::kittest::Queryable;
    use serde_json::json;

    use crate::test_window::{self, Window, click_at};

    /// A selected frame, with the task bar under it.
    fn window() -> (Harness<'static, Window>, u64) {
        let app = crate::DesignApp::new(designcraft_engine::Session::new(), crate::Services::default());
        let mut h = test_window::open(app, vec2(1440.0, 900.0));
        let app = &mut h.state_mut().app;
        let id = app.run("frame.create", json!({"rect": [72, 72, 300, 200], "content": "text", "text": "Hello"})).unwrap()["id"].as_u64().unwrap();
        app.run("selection.set", json!({"ids": [id]})).unwrap();
        h.run_steps(4);
        (h, id)
    }

    fn bar(h: &Harness<'static, Window>) -> egui::Rect {
        h.ctx.memory(|m| m.area_rect(egui::Id::new("task_bar"))).unwrap()
    }

    fn grip(h: &Harness<'static, Window>) -> egui::Pos2 {
        let b = bar(h);
        pos2(b.min.x + 10.0, b.center().y)
    }

    fn drag(h: &mut Harness<'static, Window>, from: egui::Pos2, by: egui::Vec2) {
        h.hover_at(from);
        h.drag_at(from);
        h.run_steps(2);
        for k in 1..=8 {
            h.hover_at(from + by * (k as f32 / 8.0));
            h.run_steps(1);
        }
        h.drop_at(from + by);
        h.run_steps(4);
    }

    fn more(h: &mut Harness<'static, Window>) {
        let b = bar(h);
        click_at(h, pos2(b.max.x - 19.0, b.center().y));
    }

    #[test]
    fn pinning_is_a_command() {
        let mut app = crate::DesignApp::new(designcraft_engine::Session::new(), crate::Services::default());
        let run = |app: &mut crate::DesignApp, id: &str, p| crate::menus::run_ui(app, id, &p).unwrap();
        assert!(run(&mut app, "window.taskBarPin", json!({})).is_err(), "nowhere to pin a bar that isn't showing");
        assert!(run(&mut app, "window.taskBarPin", json!({"at": [1, "x"]})).is_err());
        run(&mut app, "window.taskBarPin", json!({"at": [40, 60]})).unwrap();
        assert_eq!(app.ui.task_bar_pin, Some([40.0, 60.0]));
        assert_eq!(crate::menus::checked(&app, "window.taskBarPin", &json!({})), Some(true));
        run(&mut app, "window.taskBarPin", json!({})).unwrap();
        assert_eq!(app.ui.task_bar_pin, None, "toggled off");
        app.ui.task_bar_at = Some([10.0, 20.0]);
        run(&mut app, "window.taskBarPin", json!({"on": true})).unwrap();
        assert_eq!(app.ui.task_bar_pin, Some([10.0, 20.0]), "pinned where it shows");
        run(&mut app, "window.taskBarReset", json!({})).unwrap();
        assert_eq!(app.ui.task_bar_pin, None);
        // A workspace keeps the bar's place.
        app.ui.task_bar_pin = Some([5.0, 6.0]);
        run(&mut app, "window.newWorkspace", json!({"name": "Mine"})).unwrap();
        app.ui.task_bar_pin = None;
        run(&mut app, "window.workspace", json!({"name": "Mine"})).unwrap();
        assert_eq!(app.ui.task_bar_pin, Some([5.0, 6.0]));
    }

    #[test]
    fn the_grip_moves_the_bar_and_it_stays_there() {
        let (mut h, id) = window();
        let before = bar(&h);
        let g = grip(&h);
        drag(&mut h, g, vec2(200.0, 150.0));
        let after = bar(&h);
        assert!((after.min - before.min - vec2(200.0, 150.0)).length() < 2.0, "dragged from {before:?} to {after:?}");
        assert!(h.state().app.ui.task_bar_pin.is_some(), "pinned where it was dropped");
        // A moved selection leaves it where it is.
        h.state_mut().app.run("transform.set", json!({"ids": [id], "y": 300})).unwrap();
        h.run_steps(4);
        assert_eq!(bar(&h).min, after.min);
        // Dragged past the canvas, it stays inside.
        let canvas = h.state().app.canvas_rect.unwrap();
        let g = grip(&h);
        drag(&mut h, g, vec2(5000.0, 5000.0));
        assert!(canvas.contains_rect(bar(&h)), "{:?} outside the canvas {canvas:?}", bar(&h));
        // Reset Bar Position: back under the selection.
        more(&mut h);
        let at = h.get_by_label("Reset Bar Position").rect().center();
        click_at(&mut h, at);
        assert_eq!(h.state().app.ui.task_bar_pin, None);
        // Pin Bar Position keeps it where it is.
        more(&mut h);
        let at = h.get_by_label_contains("Pin Bar Position").rect().center();
        click_at(&mut h, at);
        let at = bar(&h).min - canvas.min;
        let [x, y] = h.state().app.ui.task_bar_pin.unwrap();
        assert!((vec2(x, y) - at).length() < 1.0, "pinned at {x}, {y}; showing at {at:?}");
        // Hide Bar.
        more(&mut h);
        let at = h.get_by_label("Hide Bar").rect().center();
        click_at(&mut h, at);
        assert!(!h.state().app.ui.task_bar);
    }
}
