//! Contextual Task Bar (InDesign 2026 §6): a floating 36 pt HUD just below the selection with the
//! most common actions for it. Window > Contextual Task Bar toggles it (on by default).

use egui::{Rect, Sense, Stroke, Ui, pos2, vec2};
use serde_json::json;

use crate::theme::Tokens;
use crate::widgets;
use crate::{DesignApp, panels};

pub const HEIGHT: f32 = 36.0;

/// The grip that moves the bar.
const GRIP: &str = "task_bar_grip";

/// Show the bar under `sel` (screen rect of the selection), or where it is pinned, kept inside
/// `canvas`.
pub fn show(app: &mut DesignApp, ctx: &egui::Context, sel: Rect, canvas: Rect) {
    let grip_id = egui::Id::unique((GRIP, ctx.viewport_id(), app.pane));
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
    // The same selection can be shown in two canvases; each bar needs its own area and widgets.
    let id = egui::Id::unique(("task_bar", ctx.viewport_id(), app.pane));
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
    // A shared persisted pin is relative to each canvas. The transient pin target belongs
    // to the focused pane, so rendering the sibling cannot overwrite it.
    if app.pane == app.focus_pane {
        app.ui.task_bar_at = Some([at.x - canvas.min.x, at.y - canvas.min.y]);
    }
    egui::Area::new(id).order(egui::Order::Middle).fixed_pos(at).show(ctx, |ui| {
        egui::Frame::NONE
            .fill(t.panel)
            .stroke(Stroke::new(1.0, t.border))
            .corner_radius(6)
            .shadow(egui::epaint::Shadow { offset: [0, 2], blur: 8, spread: 0, color: egui::Color32::from_black_alpha(80) })
            .inner_margin(egui::Margin { left: 4, right: 8, top: 0, bottom: 0 })
            .show(ui, |ui| {
                ui.set_height(HEIGHT);
                let text_frame = panels::sel_info(app).is_some_and(|i| i.is_text && i.count == 1);
                let place = [at.x - canvas.min.x, at.y - canvas.min.y];
                framework::body(app, ui, (grip_id, at, w, canvas), (has_text, text_frame), place);
            });
    });
}

/// `at` moved as little as needed for a bar `w` wide to sit inside `canvas` (8 pt in).
fn inside(at: egui::Pos2, w: f32, canvas: Rect) -> egui::Pos2 {
    let x = at.x.min(canvas.max.x - w - 8.0).max(canvas.min.x + 8.0);
    let y = at.y.min(canvas.max.y - HEIGHT - 8.0).max(canvas.min.y + 8.0);
    pos2(x, y)
}

/// The bar's members on the framework: the same grip, controls and More menu, with the same commands.
mod framework {
    use super::{DesignApp, Rect, Sense, Tokens, Ui, inside, json, panels, vec2, widgets};
    use craft_ui_widgets::bars::{Bar, BarKind};
    use craft_ui_widgets::buttons::{Button, IconButton, SplitButton};
    use craft_ui_widgets::contract::{Bind, Entry, Props as _, Show as _, ShowContainer as _, Sizing, Trigger};
    use craft_ui_widgets::display::{Divider, Picture};
    use craft_ui_widgets::list::ThumbAspect;
    use craft_ui_widgets::menu::{Menu, MenuItem};
    use craft_ui_widgets::primitives::{FacePaint, IconSpec};
    use craft_ui_widgets::style::Variant;
    use craft_ui_widgets::text::{NumberField, NumberPick};

    /// The bar's artwork: the app's icons, and the three dots of the More menu.
    #[derive(craft_ui::contract::MemoProps)]
    struct TabGlyph(&'static str);

    impl craft_ui_widgets::primitives::Glyph for TabGlyph {
        fn paint(&self, painter: &egui::Painter, _tokens: &craft_ui_widgets::style::UiTokens, rect: Rect, color: egui::Color32, _mirrored: bool) {
            if self.0 == "task-more" {
                for k in 0..3 {
                    painter.circle_filled(egui::pos2(rect.min.x + 3.0 + k as f32 * 5.0, rect.center().y), 1.8, color);
                }
            } else {
                crate::icons::paint(painter, rect, self.0, color);
            }
        }
    }

    /// What a pass of the bar asks for, run after its components are shown.
    enum Ask {
        Run(&'static str, serde_json::Value),
        Hide,
        Dialog(&'static str),
        Font(panels::FontPick),
    }

    /// A button with the app's artwork on it; whether it was clicked.
    fn icon(ui: &mut Ui, salt: &str, glyph: &'static str, label: &str) -> craft_ui_widgets::buttons::ButtonOutput {
        let face = TabGlyph(glyph);
        IconButton::new(IconSpec::glyph(&face)).id_salt(salt.to_owned()).label(label.to_owned()).tooltip(label.to_owned()).show(ui)
    }

    /// A swatch button (its chip) and the menu of the document's swatches it opens: the swatch picked.
    fn swatch_menu(ui: &mut Ui, salt: &str, label: &str, tip: &str, doc: &designcraft_doc::Document, current: &str, stroke: bool) -> Option<String> {
        let (col, grad) = widgets::swatch_colors(doc, current, 1.0);
        let inner = egui::Color32::from_gray(20);
        let opener = Button::new(label.to_owned())
            .painted(" ".to_owned())
            .id_salt(format!("{salt}_button"))
            .tooltip(tip.to_owned())
            .variant(Variant::GHOST)
            .picture(
                ThumbAspect::SQUARE,
                Picture::painter("", ThumbAspect::SQUARE, move |cx, rect| {
                    if stroke {
                        widgets::paint_stroke_chip(&cx.painter, rect, col, inner);
                    } else {
                        widgets::paint_chip(&cx.painter, rect, col, grad);
                    }
                }),
            )
            .show(ui);
        let mut picked = None;
        let _ = Menu::new(label.to_owned()).id_salt(format!("{salt}_menu")).opened_by(&opener.response, Trigger::Click).show(ui, |ui, _| {
            for sw in &doc.swatches {
                let (c, g) = widgets::swatch_colors(doc, &sw.name, 1.0);
                // The face is named by what it paints, so a swatch edited under the same name repaints.
                let key = {
                    use std::hash::{Hash, Hasher};
                    let mut hasher = std::collections::hash_map::DefaultHasher::new();
                    (&sw.name, c, g).hash(&mut hasher);
                    hasher.finish()
                };
                let face = FacePaint::new(key, move |cx, rect| widgets::paint_chip(&cx.painter, rect, c, g));
                let row =
                    MenuItem::new(sw.name.clone()).id_salt(("swatch", sw.name.clone())).check(sw.name == current).sample(ThumbAspect::SQUARE, face);
                if row.show(ui).ran {
                    picked = Some(sw.name.clone());
                }
            }
        });
        picked
    }

    pub(super) fn body(
        app: &mut DesignApp,
        ui: &mut Ui,
        (grip_id, at, w, canvas): (egui::Id, egui::Pos2, f32, Rect),
        (has_text, text_frame): (bool, bool),
        place: [f32; 2],
    ) {
        let t = Tokens::get(ui.ctx());
        let lang = app.ui.language.clone();
        let tr = |s: &str| crate::i18n::tr(&lang, s).to_string();
        let mut asks: Vec<Ask> = Vec::new();
        let pinned = app.ui.task_bar_pin.is_some();
        ui.horizontal_centered(|ui| {
            ui.spacing_mut().item_spacing.x = 6.0;
            // Dragging the grip moves the bar, and pins it where it is dropped. The grip is the egui area's own, before the
            // bar: its press is the area's, not a member of the bar's roving group.
            let (g, _) = ui.allocate_exact_size(vec2(12.0, 20.0), Sense::hover());
            let grip = ui.interact(g, grip_id, Sense::drag());
            let ink = if grip.hovered() || grip.dragged() { t.text_strong } else { t.icon };
            Picture::painter(tr("Move"), ThumbAspect::SQUARE, move |cx, rect| crate::icons::paint(&cx.painter, rect, "grip", ink))
                .id_salt("task_bar_grip_art")
                .show_at(ui, g);
            if grip.dragged() {
                ui.ctx().set_cursor_icon(egui::CursorIcon::Grabbing);
                let to = inside(at + grip.drag_delta(), w, canvas) - canvas.min;
                asks.push(Ask::Run("window.taskBarPin", json!({"at": [to.x, to.y]})));
            } else if grip.hovered() {
                ui.ctx().set_cursor_icon(egui::CursorIcon::Grab);
            }
            Bar::new(tr("Contextual Task Bar"), BarKind::Tool).sizing(Sizing::Content).show(ui, |ui, _| {
                if has_text || text_frame {
                    text_controls(app, ui, has_text, &tr, &mut asks);
                } else {
                    object_controls(app, ui, &tr, &mut asks);
                }
                // More Options.
                let more = icon(ui, "task_bar_more", "task-more", &tr("More Options"));
                let _ = Menu::new(tr("More Options")).id_salt("task_bar_more_menu").opened_by(&more.response, Trigger::Click).show(ui, |ui, _| {
                    if MenuItem::new(tr("Hide Bar")).id_salt("hide").show(ui).ran {
                        asks.push(Ask::Hide);
                    }
                    let pin = MenuItem::new(tr("Pin Bar Position")).id_salt("pin").check(pinned).show(ui);
                    if pin.ran {
                        asks.push(Ask::Run("window.taskBarPin", if pinned { json!({"on": false}) } else { json!({"on": true, "at": place}) }));
                    }
                    if MenuItem::new(tr("Reset Bar Position")).id_salt("reset").enabled(pinned).show(ui).ran {
                        asks.push(Ask::Run("window.taskBarReset", json!({})));
                    }
                });
            });
        });
        for ask in asks {
            match ask {
                Ask::Run(id, params) => {
                    let _ = app.run(id, params);
                }
                Ask::Hide => app.ui.task_bar = false,
                Ask::Dialog(id) => app.ui.dialog = Some(crate::dialogs::Dialog::new(id, json!({}))),
                Ask::Font(f) => panels::apply_font(app, &f),
            }
        }
    }

    fn text_controls(app: &mut DesignApp, ui: &mut Ui, in_text: bool, tr: &dyn Fn(&str) -> String, asks: &mut Vec<Ask>) {
        if !in_text {
            let modes = [("off", "Off"), ("heightOnly", "Height Only"), ("widthOnly", "Width Only"), ("heightAndWidth", "Height and Width")];
            let entries: Vec<Entry<String>> = modes.iter().map(|(v, l)| Entry::new((*v).to_owned(), tr(l))).collect();
            let face = TabGlyph("fit-text");
            let out = SplitButton::new(tr("Fit Text"), &entries)
                .icon(IconSpec::glyph(&face))
                .id_salt("task_bar_fit_text")
                .tooltip(tr("Fit frame height to text"))
                .show(ui);
            if out.response.clicked() {
                asks.push(Ask::Run("object.textFrameOptions", json!({"autoSize": "heightOnly"})));
            }
            if let Some(mode) = out.chosen {
                asks.push(Ask::Run("object.textFrameOptions", json!({"autoSize": mode})));
            }
            Divider::vertical().show(ui);
        }
        let Some(a) = panels::text_attrs(app) else { return };
        let c = a["chars"].clone();
        let fam = c["fontFamily"].as_str().unwrap_or("").to_string();
        let sty = c["fontStyle"].as_str().unwrap_or("").to_string();
        if let Some(f) = panels::font_combo(app, ui, "task_bar_font", &fam, 150.0) {
            asks.push(Ask::Font(f));
        }
        panels::font_style_picker(app, ui, &fam, &sty, 90.0);
        let size_label = tr("Font Size");
        const SIZES: &[f64] = &[6.0, 8.0, 9.0, 10.0, 11.0, 12.0, 14.0, 18.0, 24.0, 30.0, 36.0, 48.0, 60.0, 72.0];
        let presets: Vec<Entry<NumberPick>> = SIZES.iter().map(|v| Entry::new(NumberPick::Number(*v), format!("{v}"))).collect();
        let out = NumberField::new(size_label.clone(), Bind::given(c["size"].as_f64()))
            .id_salt("task_bar_size")
            .unit("pt".to_owned())
            .range(0.1..=1296.0)
            .step(1.0)
            .presets(&presets)
            .show(ui);
        if let Some(v) = out.edit.events.committed().copied().flatten().filter(|v| v.is_finite()) {
            asks.push(Ask::Run("type.char", json!({"attrs": {"size": v}})));
        }
        Divider::vertical().show(ui);
        // Text fill.
        if let Some(doc) = app.session.active().map(|d| d.doc.clone()) {
            let sw = c["fill"].as_str().unwrap_or("[Black]").to_string();
            let tip = format!("{}: {sw}", tr("Fill"));
            if let Some(p) = swatch_menu(ui, "task_bar_fill", "Task Bar fill", &tip, &doc, &sw, false) {
                asks.push(Ask::Run("type.char", json!({"attrs": {"fill": p}})));
            }
        }
        // Paragraph alignment.
        let al = a["para"]["align"].as_str().unwrap_or("left").to_string();
        let glyph = match al.as_str() {
            "center" => "palign-center",
            "right" => "palign-right",
            "leftJustified" | "centerJustified" | "rightJustified" | "fullyJustified" => "palign-justify-left",
            _ => "palign-left",
        };
        let opener = icon(ui, "task_bar_align", glyph, &tr("Paragraph alignment"));
        let _ = Menu::new(tr("Paragraph alignment")).id_salt("task_bar_align_menu").opened_by(&opener.response, Trigger::Click).show(ui, |ui, _| {
            for (label, v) in [
                ("Align Left", "left"),
                ("Align Center", "center"),
                ("Align Right", "right"),
                ("Justify", "leftJustified"),
                ("Justify All", "fullyJustified"),
            ] {
                if MenuItem::new(tr(label)).id_salt(v).check(al == v).show(ui).ran {
                    asks.push(Ask::Run("type.para", json!({"attrs": {"align": v}})));
                }
            }
        });
        if icon(ui, "task_bar_frame_options", "frame-options", &tr("Text Frame Options")).clicked() {
            asks.push(Ask::Dialog("textFrameOptions"));
        }
    }

    fn object_controls(app: &mut DesignApp, ui: &mut Ui, tr: &dyn Fn(&str) -> String, asks: &mut Vec<Ask>) {
        let Some(i) = panels::sel_info(app) else { return };
        let Some(doc) = app.session.active().map(|d| d.doc.clone()) else { return };
        if i.is_graphic {
            for (glyph, mode, tip) in [
                ("fit-fill", "fillProportionally", "Fill Frame Proportionally"),
                ("fit-prop", "fitProportionally", "Fit Content Proportionally"),
                ("fit-frame", "fitFrameToContent", "Fit Frame to Content"),
                ("fit-center", "centerContent", "Center Content"),
            ] {
                if icon(ui, &format!("task_bar_{mode}"), glyph, &tr(tip)).clicked() {
                    asks.push(Ask::Run("object.fit", json!({"mode": mode})));
                }
            }
            Divider::vertical().show(ui);
        }
        for (k, (cmd, sw)) in [("object.fill", i.fill.clone()), ("object.stroke", i.stroke.clone())].into_iter().enumerate() {
            let tip = format!("{}: {sw}", if k == 0 { "Fill" } else { "Stroke" });
            let name = format!("Task Bar {}", if k == 0 { "fill" } else { "stroke" });
            if let Some(p) = swatch_menu(ui, &format!("task_bar_object_{k}"), &name, &tip, &doc, &sw, k == 1) {
                asks.push(Ask::Run(cmd, json!({"swatch": p})));
            }
        }
        const WEIGHTS: &[f64] = &[0.25, 0.5, 1.0, 2.0, 3.0, 4.0, 6.0, 8.0, 10.0];
        let presets: Vec<Entry<NumberPick>> = WEIGHTS.iter().map(|v| Entry::new(NumberPick::Number(*v), format!("{v}"))).collect();
        let out = NumberField::new(tr("Stroke Weight"), Bind::given(Some(i.stroke_weight)))
            .id_salt("task_bar_stroke_weight")
            .unit("pt".to_owned())
            .range(0.0..=800.0)
            .step(1.0)
            .presets(&presets)
            .show(ui);
        if let Some(v) = out.edit.events.committed().copied().flatten().filter(|v| v.is_finite()) {
            asks.push(Ask::Run("object.stroke", json!({"weight": v})));
        }
        Divider::vertical().show(ui);
        for (cmd, glyph, tip) in [("object.bringToFront", "objalign-top", "Bring to Front"), ("object.sendToBack", "objalign-bottom", "Send to Back")]
        {
            if icon(ui, &format!("task_bar_{cmd}"), glyph, &tr(tip)).clicked() {
                asks.push(Ask::Run(cmd, json!({})));
            }
        }
        if i.count > 1 && Button::new(tr("Group")).id_salt("task_bar_group").show(ui).clicked() {
            asks.push(Ask::Run("object.group", json!({})));
        }
    }
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
        h.ctx.memory(|m| m.area_rect(egui::Id::unique(("task_bar", h.ctx.viewport_id(), h.state().app.pane)))).unwrap()
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
    fn split_pane_grips_handle_only_their_own_drag() {
        let (mut h, _) = window();
        h.set_size(vec2(2200.0, 900.0));
        // A compact object HUD fits in each pane, leaving room to exercise both axes
        // without the intentional canvas-edge clamp masking horizontal movement.
        let id = h.state_mut().app.run("frame.create", json!({"rect":[72,72,300,200],"content":"none"})).unwrap()["id"].as_u64().unwrap();
        h.state_mut().app.run("selection.set", json!({"ids":[id]})).unwrap();
        h.state_mut().app.run("window.split", json!({"on":true})).unwrap();
        h.run_steps(6);
        let area =
            |h: &Harness<'static, Window>, pane| h.ctx.memory(|m| m.area_rect(egui::Id::unique(("task_bar", h.ctx.viewport_id(), pane)))).unwrap();
        for pane in [0_u8, 1] {
            h.state_mut().app.run("window.taskBarPin", json!({"at":[100,120]})).unwrap();
            h.run_steps(4);
            let before = area(&h, pane);
            let from = pos2(before.min.x + 10.0, before.center().y);
            let delta = vec2(24.0, 28.0);
            drag(&mut h, from, delta);
            let after = area(&h, pane);
            assert!((after.min - before.min - delta).length() < 2.0, "pane {pane} drag was handled by a sibling grip: {before:?} -> {after:?}");
            assert!(h.state().app.ui.task_bar_pin.is_some());
        }
    }

    #[test]
    fn popup_pin_uses_the_invoking_hud_position_after_focus_moves_to_sibling() {
        let (mut h, _) = window();
        let id = h.state_mut().app.run("frame.create", json!({"rect":[72,72,300,200],"content":"none"})).unwrap()["id"].as_u64().unwrap();
        h.state_mut().app.run("selection.set", json!({"ids":[id]})).unwrap();
        h.state_mut().app.run("window.split", json!({"on":true})).unwrap();
        h.run_steps(6);
        h.state_mut().app.switch_pane(1);
        h.state_mut().app.run("view.zoom", json!({"zoom":0.5})).unwrap();
        h.state_mut().app.switch_pane(0);
        h.run_steps(4);
        let expected = h.state().app.ui.task_bar_at.unwrap();
        let left = h.ctx.memory(|m| m.area_rect(egui::Id::unique(("task_bar", h.ctx.viewport_id(), 0_u8)))).unwrap();
        click_at(&mut h, pos2(left.max.x - 19.0, left.center().y));
        let pin = h.get_by_label_contains("Pin Bar Position").rect();
        let at = pos2(pin.max.x - 2.0, pin.center().y);
        let sibling = h.state().app.other_pane.as_ref().and_then(|(_, rect)| *rect).unwrap();
        assert!(sibling.contains(at), "fixture popup must extend into sibling canvas: {at:?} outside {sibling:?}");
        h.event(egui::Event::PointerMoved(at));
        h.event(egui::Event::PointerButton { pos: at, button: egui::PointerButton::Primary, pressed: true, modifiers: egui::Modifiers::NONE });
        h.run_steps(1);
        assert_eq!(h.state().app.focus_pane, 1);
        assert_ne!(h.state().app.ui.task_bar_at, Some(expected), "fixture HUD positions must differ");
        h.event(egui::Event::PointerButton { pos: at, button: egui::PointerButton::Primary, pressed: false, modifiers: egui::Modifiers::NONE });
        h.run_steps(4);
        assert_eq!(h.state().app.ui.task_bar_pin, Some(expected), "Pin must use the invoking HUD position");
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
        // TASKBAR-DEBUG: temporary, to see what the click on More reached.
        {
            use egui_kittest::kittest::NodeT as _;
            eprintln!("TASKBAR-DEBUG bar {:?} canvas {canvas:?} pin {:?}", bar(&h), h.state().app.ui.task_bar_pin);
            for n in h.query_all_by(|n| n.label().is_some_and(|l| l.contains("Bar") || l.contains("More") || l.contains("Pin") || l.contains("Hide")))
            {
                eprintln!("TASKBAR-DEBUG   {:?} {:?} {:?}", n.accesskit_node().role(), n.label(), n.rect());
            }
        }
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
