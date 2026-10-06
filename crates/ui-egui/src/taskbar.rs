//! Contextual Task Bar (InDesign 2026 §6): a floating 36 pt HUD just below the selection with the
//! most common actions for it. Window > Contextual Task Bar toggles it (on by default).

use egui::{Rect, Sense, Stroke, StrokeKind, Ui, pos2, vec2};
use serde_json::json;

use crate::theme::Tokens;
use crate::widgets::{self, FIELD_H, NumField};
use crate::{DesignApp, icons, panels};

pub const HEIGHT: f32 = 36.0;

/// Show the bar under `sel` (screen rect of the selection), kept inside `canvas`.
pub fn show(app: &mut DesignApp, ctx: &egui::Context, sel: Rect, canvas: Rect) {
    // (Two separate context calls: `is_pointer_over_egui` inside an `input` closure deadlocks.)
    if !app.ui.task_bar || (ctx.input(|i| i.pointer.any_down()) && !ctx.is_pointer_over_egui()) {
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
    let mut x = sel.center().x - w / 2.0;
    x = x.clamp(canvas.min.x + 8.0, (canvas.max.x - w - 8.0).max(canvas.min.x + 8.0));
    let mut y = sel.max.y + 14.0;
    if y + HEIGHT > canvas.max.y - 8.0 {
        y = (sel.min.y - HEIGHT - 14.0).max(canvas.min.y + 8.0);
    }
    if y + HEIGHT > canvas.max.y - 4.0 {
        y = canvas.max.y - HEIGHT - 8.0;
    }
    egui::Area::new(id).order(egui::Order::Middle).fixed_pos(pos2(x, y)).show(ctx, |ui| {
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
                    let (g, _) = ui.allocate_exact_size(vec2(12.0, 20.0), Sense::hover());
                    icons::paint(ui.painter(), g, "grip", t.icon);
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
    let fams = designcraft_fonts::FontDb::global().families();
    let cur = fams.iter().position(|f| *f == fam);
    if let Some(k) = widgets::dropdown_list(ui, if fam.is_empty() { "—" } else { &fam }, 150.0, &fams, cur) {
        let f = fams[k].clone();
        let styles = designcraft_fonts::FontDb::global().styles(&f);
        let style = if styles.iter().any(|s| s == "Regular") { "Regular".to_string() } else { styles.first().cloned().unwrap_or_default() };
        let _ = app.run("type.char", json!({"attrs": {"fontFamily": f, "fontStyle": style}}));
    }
    let styles = designcraft_fonts::FontDb::global().styles(&fam);
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
        let mut picked = None;
        egui::Popup::menu(&resp).show(|ui| {
            egui::ScrollArea::vertical().max_height(300.0).show(ui, |ui| {
                for s in &doc.swatches {
                    if ui.selectable_label(s.name == sw, &s.name).clicked() {
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
        let mut picked = None;
        egui::Popup::menu(&resp).show(|ui| {
            egui::ScrollArea::vertical().max_height(300.0).show(ui, |ui| {
                for s in &doc.swatches {
                    if ui.selectable_label(s.name == sw, &s.name).clicked() {
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
        if ui.button(crate::rtl::widget(ui, crate::i18n::tr(&app.ui.language, "Properties Panel"))).clicked() {
            let _ = app.run("window.panel", json!({"panel": "properties"}));
            ui.close();
        }
    });
}

fn sep(ui: &mut Ui) {
    let t = Tokens::get(ui.ctx());
    let (r, _) = ui.allocate_exact_size(vec2(1.0, 24.0), Sense::hover());
    ui.painter().line_segment([r.center_top(), r.center_bottom()], Stroke::new(1.0, t.border));
}
