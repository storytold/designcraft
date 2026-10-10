//! Window chrome: application bar (with menus), Control panel, document tabs, status bar, start screen.

use designcraft_doc::{Align, Content};
use designcraft_geom::Unit;
use egui::{Color32, Sense, Stroke, vec2};
use serde_json::json;

use crate::panels::{self, SelInfo};
use crate::theme::{Tokens, semibold};
use crate::widgets::{caption, measure, number};
use crate::{DesignApp, icons};

pub fn app_bar(app: &mut DesignApp, ui: &mut egui::Ui) {
    let t = Tokens::get(ui.ctx());
    let left = if app.integrated_titlebar { 78 } else { 8 };
    // 36 pt bar + 7 pt lower band (InDesign 2026), then a 1 pt border.
    let resp = egui::Panel::top("app_bar")
        .exact_size(43.0)
        .frame(egui::Frame::NONE.fill(t.app_bar).inner_margin(egui::Margin { left, right: 10, top: 0, bottom: 7 }))
        .show(ui, |ui| {
            let full = ui.max_rect();
            let mut menus_end = full.min.x;
            let mut tools_start = full.max.x;
            // The right-hand controls' width, measured last frame: in a window too narrow for them
            // and the menus they follow the menus, and the bar scrolls sideways.
            let tools_w_id = ui.id().with("tools_width");
            let tools_w: f32 = ui.data(|d| d.get_temp(tools_w_id)).unwrap_or(0.0);
            crate::widgets::overflow_scrolling(ui);
            egui::ScrollArea::horizontal().id_salt("app_bar_scroll").auto_shrink([false, false]).show(ui, |ui| {
                ui.horizontal_centered(|ui| {
                    if icons::button(ui, "home", 24.0, app.session.active().is_none(), crate::i18n::tr(&app.ui.language, "Home")).clicked() {
                        app.session_home();
                    }
                    ui.add_space(6.0);
                    if !app.native_menu {
                        ui.style_mut().visuals.widgets.inactive.weak_bg_fill = Color32::TRANSPARENT;
                        ui.style_mut().visuals.widgets.inactive.bg_stroke = Stroke::NONE;
                        ui.style_mut().visuals.widgets.hovered.bg_stroke = Stroke::NONE;
                        ui.style_mut().spacing.button_padding = vec2(7.0, 3.0);
                        crate::menus::menu_bar(app, ui);
                    }
                    menus_end = ui.min_rect().max.x;
                    let size = vec2(ui.available_width().max(tools_w), ui.available_height());
                    let tools = ui.allocate_ui_with_layout(size, egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        ui.add_space(4.0);
                        // Search field.
                        let (r, _) = ui.allocate_exact_size(vec2(125.0, 18.0), Sense::click());
                        ui.painter().rect(r, 1.0, t.input, Stroke::new(1.0, t.field_border), egui::StrokeKind::Inside);
                        icons::paint(ui.painter(), egui::Rect::from_min_size(r.min + vec2(3.0, 2.0), vec2(14.0, 14.0)), "search", t.icon);
                        crate::rtl::paint(
                            ui.painter(),
                            r.min + vec2(20.0, 9.0),
                            egui::Align2::LEFT_CENTER,
                            crate::i18n::tr(&app.ui.language, "Search"),
                            egui::FontId::proportional(11.0),
                            t.text_dim,
                        );
                        ui.add_space(8.0);
                        let current = app.ui.workspace.clone();
                        let shown = crate::i18n::workspace_name(&app.ui.language, &current);
                        ui.menu_button(crate::rtl::widget(ui, egui::RichText::new(format!("{shown} ▾")).font(semibold(11.5)).color(t.text)), |ui| {
                            let customs: Vec<String> = app.ui.custom_workspaces.iter().map(|w| w.name.clone()).collect();
                            for w in
                                ["Essentials", "Advanced", "Book", "Digital Publishing", "Interactive for PDF", "Printing and Proofing", "Typography"]
                                    .into_iter()
                                    .map(str::to_string)
                                    .chain(customs.iter().cloned())
                            {
                                if ui
                                    .selectable_label(w == current, crate::rtl::widget(ui, crate::i18n::workspace_name(&app.ui.language, &w)))
                                    .clicked()
                                {
                                    let _ = app.run("window.workspace", json!({"name": w}));
                                    ui.close();
                                }
                            }
                            ui.separator();
                            if ui
                                .button(crate::rtl::widget(
                                    ui,
                                    format!(
                                        "{} {}",
                                        crate::i18n::tr(&app.ui.language, "Reset"),
                                        crate::i18n::workspace_name(&app.ui.language, &current)
                                    ),
                                ))
                                .clicked()
                            {
                                let _ = app.run("window.resetWorkspace", json!({}));
                                ui.close();
                            }
                            if ui.button(crate::rtl::widget(ui, crate::i18n::tr(&app.ui.language, "New Workspace…"))).clicked() {
                                let _ = app.run("window.newWorkspace", json!({}));
                                ui.close();
                            }
                            if !customs.is_empty() {
                                ui.menu_button(crate::rtl::widget(ui, crate::i18n::tr(&app.ui.language, "Delete Workspace")), |ui| {
                                    for w in &customs {
                                        if ui.button(w).clicked() {
                                            let _ = app.run("window.deleteWorkspace", json!({"name": w}));
                                            ui.close();
                                        }
                                    }
                                });
                            }
                        });
                        ui.add_space(6.0);
                        if icons::button(ui, "share", 22.0, false, crate::i18n::tr(&app.ui.language, "Share")).clicked() {
                            app.status("Export a PDF, IDML or package to share — no cloud account needed.");
                        }
                        ui.add_space(8.0);
                        // Always one click away: the ArtCraft community Discord.
                        if crate::about::discord_button(ui, "Discord", vec2(78.0, 22.0)) {
                            let _ = app.run("help.discord", json!({}));
                        }
                        tools_start = ui.min_rect().min.x;
                        ui.min_rect().width()
                    });
                    if (tools.inner - tools_w).abs() > 0.5 {
                        ui.data_mut(|d| d.insert_temp(tools_w_id, tools.inner));
                        ui.ctx().request_discard("app bar: the right-hand controls changed width");
                    }
                });
            });
            // Centred title.
            let title = match app.session.active() {
                Some(d) => format!("DesignCraft — {}", crate::rtl::isolate(&d.title())),
                None => "DesignCraft".to_string(),
            };
            // Centred between the menus and the right-hand controls; left out when it won't fit.
            let galley = crate::rtl::plain(ui.ctx(), &title, egui::FontId::proportional(11.5), t.text);
            let w = galley.size().x;
            let cx = full.center().x;
            if cx - w / 2.0 > menus_end + 12.0 && cx + w / 2.0 < tools_start - 12.0 {
                ui.painter().galley(egui::pos2(cx - w / 2.0, full.min.y + 18.0 - galley.size().y / 2.0), galley, t.text);
            }
        });
    let r = resp.response.rect;
    ui.painter().rect_filled(egui::Rect::from_min_max(egui::pos2(r.min.x, r.max.y - 7.0), r.max), 0.0, t.pasteboard);
    ui.painter().line_segment([egui::pos2(r.min.x, r.max.y), egui::pos2(r.max.x, r.max.y)], Stroke::new(1.0, t.border));
}

impl DesignApp {
    fn session_home(&mut self) {
        self.ui.status = "Home".into();
    }
}

fn vsep(ui: &mut egui::Ui) {
    let t = Tokens::get(ui.ctx());
    let (r, _) = ui.allocate_exact_size(vec2(9.0, 46.0), Sense::hover());
    ui.painter().line_segment([r.center_top() + vec2(0.0, 4.0), r.center_bottom() - vec2(0.0, 4.0)], Stroke::new(1.0, t.divider));
}

/// Two-row context-sensitive Control panel.
pub fn control_bar(app: &mut DesignApp, ui: &mut egui::Ui) {
    let t = Tokens::get(ui.ctx());
    egui::Panel::top("control_bar")
        .exact_size(59.0)
        .frame(
            egui::Frame::NONE.fill(t.panel).inner_margin(egui::Margin { left: 8, right: 8, top: 5, bottom: 3 }).stroke(Stroke::new(1.0, t.divider)),
        )
        .show(ui, |ui| {
            // Wider than the window: the bar scrolls sideways.
            crate::widgets::overflow_scrolling(ui);
            egui::ScrollArea::horizontal().id_salt("control_bar_scroll").auto_shrink([false, true]).show(ui, |ui| {
                ui.horizontal_top(|ui| {
                    let text_mode =
                        matches!(app.session.tool_id(), "type" | "verticalType") || app.session.active().is_some_and(|d| d.selection.text.is_some());
                    if text_mode {
                        control_text(app, ui);
                    } else {
                        control_object(app, ui);
                    }
                });
            });
        });
}

/// A field label drawn as a tool icon (hover for its name).
fn icon_caption(ui: &mut egui::Ui, icon: &str, tip: &str) {
    let (r, resp) = ui.allocate_exact_size(vec2(16.0, 16.0), Sense::hover());
    icons::paint(ui.painter(), r, icon, Tokens::get(ui.ctx()).icon);
    resp.on_hover_ui(|ui| {
        crate::rtl::label(ui, tip);
    });
}

fn control_object(app: &mut DesignApp, ui: &mut egui::Ui) {
    let units = app.session.active().map(|d| d.doc.settings.horizontal_units).unwrap_or(Unit::Picas);
    let info = panels::sel_info(app);
    // Reference point proxy.
    let (r, _) = ui.allocate_exact_size(vec2(30.0, 44.0), Sense::hover());
    let t = Tokens::get(ui.ctx());
    icons::paint(ui.painter(), egui::Rect::from_center_size(r.center(), vec2(26.0, 26.0)), "ref-point", t.icon);
    ui.painter().rect_filled(egui::Rect::from_center_size(r.center() + vec2(-7.8, -7.8), vec2(4.0, 4.0)), 0.0, t.text_strong);
    let (x, y, w, h) = match &info {
        Some(SelInfo { page_rect, .. }) => (Some(page_rect.x0), Some(page_rect.y0), Some(page_rect.width()), Some(page_rect.height())),
        None => (None, None, None, None),
    };
    egui::Grid::new("ctl_xywh").num_columns(4).spacing(vec2(4.0, 4.0)).show(ui, |ui| {
        caption(ui, crate::i18n::tr(&app.ui.language, "X:"));
        if let Some(v) = measure(ui, "cx", x, units, 64.0) {
            let _ = app.run("transform.set", json!({"x": v}));
        }
        caption(ui, crate::i18n::tr(&app.ui.language, "W:"));
        if let Some(v) = measure(ui, "cw", w, units, 64.0) {
            let _ = app.run("transform.set", json!({"width": v}));
        }
        ui.end_row();
        caption(ui, crate::i18n::tr(&app.ui.language, "Y:"));
        if let Some(v) = measure(ui, "cy", y, units, 64.0) {
            let _ = app.run("transform.set", json!({"y": v}));
        }
        caption(ui, crate::i18n::tr(&app.ui.language, "H:"));
        if let Some(v) = measure(ui, "ch", h, units, 64.0) {
            let _ = app.run("transform.set", json!({"height": v}));
        }
        ui.end_row();
    });
    vsep(ui);
    // Scale, rotation and shear (absolute; see Transformations are Totals).
    let tv = if info.is_some() { app.session.execute("transform.info", &json!({})).ok() } else { None };
    let tv_get = |k: &str| tv.as_ref().and_then(|v| v[k].as_f64());
    egui::Grid::new("ctl_srs").num_columns(4).spacing(vec2(4.0, 4.0)).show(ui, |ui| {
        icon_caption(ui, "tool-scale", crate::i18n::tr(&app.ui.language, "Scale X Percentage"));
        if let Some(v) = number(ui, "csx", tv_get("scaleX"), "%", 56.0, 1) {
            let _ = app.run("transform.set", json!({"scaleX": v}));
        }
        icon_caption(ui, "tool-rotate", crate::i18n::tr(&app.ui.language, "Rotation Angle"));
        if let Some(v) = number(ui, "crot", tv_get("rotation"), "°", 50.0, 1) {
            let _ = app.run("transform.set", json!({"rotation": v}));
        }
        ui.end_row();
        icon_caption(ui, "tool-scale", crate::i18n::tr(&app.ui.language, "Scale Y Percentage"));
        if let Some(v) = number(ui, "csy", tv_get("scaleY"), "%", 56.0, 1) {
            let _ = app.run("transform.set", json!({"scaleY": v}));
        }
        icon_caption(ui, "tool-shear", crate::i18n::tr(&app.ui.language, "Shear X Angle"));
        if let Some(v) = number(ui, "cshr", tv_get("shear"), "°", 50.0, 1) {
            let _ = app.run("transform.set", json!({"shear": v}));
        }
        ui.end_row();
    });
    vsep(ui);
    // Rotate / flip.
    ui.vertical(|ui| {
        ui.horizontal(|ui| {
            if icons::button(ui, "tool-rotate", 20.0, false, crate::i18n::tr(&app.ui.language, "Rotate 90° Counterclockwise")).clicked() {
                let _ = app.run("transform.rotate", json!({"angle": 90}));
            }
            if ui
                .small_button("⇋")
                .on_hover_ui(|ui| {
                    crate::rtl::label(ui, crate::i18n::tr(&app.ui.language, "Flip Horizontal"));
                })
                .clicked()
            {
                let _ = app.run("transform.flip", json!({"axis": "horizontal"}));
            }
        });
        ui.horizontal(|ui| {
            if ui
                .small_button("↻")
                .on_hover_ui(|ui| {
                    crate::rtl::label(ui, crate::i18n::tr(&app.ui.language, "Rotate 90° Clockwise"));
                })
                .clicked()
            {
                let _ = app.run("transform.rotate", json!({"angle": -90}));
            }
            if ui
                .small_button("⇵")
                .on_hover_ui(|ui| {
                    crate::rtl::label(ui, crate::i18n::tr(&app.ui.language, "Flip Vertical"));
                })
                .clicked()
            {
                let _ = app.run("transform.flip", json!({"axis": "vertical"}));
            }
        });
    });
    vsep(ui);
    // Fill / stroke.
    ui.vertical(|ui| {
        ui.horizontal(|ui| {
            caption(ui, crate::i18n::tr(&app.ui.language, "Fill"));
            panels::swatch_picker(app, ui, "ctlfill", info.as_ref().map(|i| i.fill.clone()), |app, name| {
                let _ = app.run("object.fill", json!({"swatch": name}));
            });
        });
        ui.horizontal(|ui| {
            caption(ui, crate::i18n::tr(&app.ui.language, "Stroke"));
            panels::swatch_picker(app, ui, "ctlstroke", info.as_ref().map(|i| i.stroke.clone()), |app, name| {
                let _ = app.run("object.stroke", json!({"swatch": name}));
            });
            if let Some(v) = number(ui, "csw", info.as_ref().map(|i| i.stroke_weight), " pt", 46.0, 3) {
                let _ = app.run("object.stroke", json!({"weight": v}));
            }
        });
    });
    vsep(ui);
    // Opacity, fx.
    ui.vertical(|ui| {
        ui.horizontal(|ui| {
            caption(ui, crate::i18n::tr(&app.ui.language, "Opacity"));
            if let Some(v) = number(ui, "cop", info.as_ref().map(|i| i.opacity * 100.0), "%", 42.0, 0) {
                let _ = app.run("object.opacity", json!({"opacity": v / 100.0}));
            }
        });
        ui.horizontal(|ui| {
            if icons::button(ui, "panel-effects", 20.0, info.as_ref().is_some_and(|i| i.shadow), crate::i18n::tr(&app.ui.language, "Drop Shadow"))
                .clicked()
            {
                let _ = app.run("object.dropShadow", json!({}));
            }
        });
    });
    vsep(ui);
    // Fitting.
    ui.vertical(|ui| {
        ui.horizontal(|ui| {
            for (icon, mode, tip) in [
                ("fit-fill", "fillProportionally", "Fill Frame Proportionally"),
                ("fit-prop", "fitProportionally", "Fit Content Proportionally"),
                ("fit-content", "fitContentToFrame", "Fit Content to Frame"),
            ] {
                if icons::button(ui, icon, 20.0, false, tip).clicked() {
                    let _ = app.run("object.fit", json!({"mode": mode}));
                }
            }
        });
        ui.horizontal(|ui| {
            for (icon, mode, tip) in [("fit-frame", "fitFrameToContent", "Fit Frame to Content"), ("fit-center", "centerContent", "Center Content")] {
                if icons::button(ui, icon, 20.0, false, tip).clicked() {
                    let _ = app.run("object.fit", json!({"mode": mode}));
                }
            }
        });
    });
    vsep(ui);
    // Text wrap.
    ui.vertical(|ui| {
        ui.horizontal(|ui| {
            for (icon, mode, tip) in [
                ("wrap-none", "none", "No Text Wrap"),
                ("wrap-bbox", "boundingBox", "Wrap Around Bounding Box"),
                ("wrap-contour", "contour", "Wrap Around Object Shape"),
            ] {
                let on = info.as_ref().is_some_and(|i| i.wrap == mode);
                if icons::button(ui, icon, 20.0, on, tip).clicked() {
                    let _ = app.run("object.textWrap", json!({"mode": mode}));
                }
            }
        });
        ui.horizontal(|ui| {
            for (icon, mode, tip) in [("wrap-jump", "jumpObject", "Jump Object"), ("wrap-next", "jumpToNextColumn", "Jump to Next Column")] {
                let on = info.as_ref().is_some_and(|i| i.wrap == mode);
                if icons::button(ui, icon, 20.0, on, tip).clicked() {
                    let _ = app.run("object.textWrap", json!({"mode": mode}));
                }
            }
        });
    });
    if let Some(i) = &info
        && let Some(cols) = i.columns
    {
        vsep(ui);
        ui.vertical(|ui| {
            caption(ui, crate::i18n::tr(&app.ui.language, "Columns"));
            if let Some(v) = number(ui, "ccols", Some(cols as f64), "", 36.0, 0) {
                let _ = app.run("object.textFrameOptions", json!({"columns": v.max(1.0) as u64}));
            }
        });
    }
}

fn control_text(app: &mut DesignApp, ui: &mut egui::Ui) {
    let t = Tokens::get(ui.ctx());
    let attrs = panels::text_attrs(app);
    let chars = attrs.as_ref().map(|a| a["chars"].clone()).unwrap_or_default();
    let para = attrs.as_ref().map(|a| a["para"].clone()).unwrap_or_default();
    ui.vertical(|ui| {
        ui.horizontal(|ui| {
            ui.label(egui::RichText::new("A").font(semibold(15.0)).color(t.text_strong));
        });
        ui.horizontal(|ui| {
            ui.label(egui::RichText::new("¶").font(semibold(15.0)).color(t.text_dim));
        });
    });
    vsep(ui);
    ui.vertical(|ui| {
        let fam = chars["fontFamily"].as_str().unwrap_or("").to_string();
        let sty = chars["fontStyle"].as_str().unwrap_or("").to_string();
        panels::font_family_picker(app, ui, &fam, 170.0);
        panels::font_style_picker(app, ui, &fam, &sty, 170.0);
    });
    vsep(ui);
    egui::Grid::new("ctl_char").num_columns(4).spacing(vec2(4.0, 3.0)).show(ui, |ui| {
        caption(ui, "𝐓");
        if let Some(v) = number(ui, "csize", chars["size"].as_f64(), " pt", 54.0, 2) {
            let _ = app.run("type.char", json!({"attrs": {"size": v}}));
        }
        caption(ui, crate::i18n::tr(&app.ui.language, "VA"));
        if let Some(v) = number(ui, "ctrack", chars["tracking"].as_f64(), "", 48.0, 0) {
            let _ = app.run("type.char", json!({"attrs": {"tracking": v}}));
        }
        ui.end_row();
        caption(ui, "Ā");
        let lead = match chars["leading"]["kind"].as_str() {
            Some("points") => chars["leading"]["value"].as_f64(),
            _ => chars["size"].as_f64().map(|s| s * para["autoLeading"].as_f64().unwrap_or(1.2)),
        };
        if let Some(v) = number(ui, "clead", lead, " pt", 54.0, 2) {
            let _ = app.run("type.char", json!({"attrs": {"leading": {"kind": "points", "value": v}}}));
        }
        caption(ui, crate::i18n::tr(&app.ui.language, "IT"));
        if let Some(v) = number(ui, "chs", chars["hScale"].as_f64().map(|v| v * 100.0), "%", 48.0, 0) {
            let _ = app.run("type.char", json!({"attrs": {"hScale": v / 100.0}}));
        }
        ui.end_row();
    });
    vsep(ui);
    ui.vertical(|ui| {
        let cur: Align = serde_json::from_value(para["align"].clone()).unwrap_or_default();
        ui.horizontal(|ui| {
            for (a, icon) in [(Align::Left, "align-left"), (Align::Center, "align-center"), (Align::Right, "align-right")] {
                if icons::button(ui, icon, 20.0, cur == a, a.label()).clicked() {
                    let _ = app.run("type.para", json!({"attrs": {"align": a}}));
                }
            }
        });
        ui.horizontal(|ui| {
            for (a, icon) in [(Align::LeftJustified, "align-justify"), (Align::FullyJustified, "align-justify-all")] {
                if icons::button(ui, icon, 20.0, cur == a, a.label()).clicked() {
                    let _ = app.run("type.para", json!({"attrs": {"align": a}}));
                }
            }
        });
    });
    vsep(ui);
    let units = app.session.active().map(|d| d.doc.settings.horizontal_units).unwrap_or(Unit::Picas);
    egui::Grid::new("ctl_para").num_columns(4).spacing(vec2(4.0, 3.0)).show(ui, |ui| {
        caption(ui, "→|");
        if let Some(v) = measure(ui, "cli", para["leftIndent"].as_f64(), units, 54.0) {
            let _ = app.run("type.para", json!({"attrs": {"leftIndent": v}}));
        }
        caption(ui, "↑¶");
        if let Some(v) = measure(ui, "csb", para["spaceBefore"].as_f64(), units, 54.0) {
            let _ = app.run("type.para", json!({"attrs": {"spaceBefore": v}}));
        }
        ui.end_row();
        caption(ui, "→¶");
        if let Some(v) = measure(ui, "cfi", para["firstLineIndent"].as_f64(), units, 54.0) {
            let _ = app.run("type.para", json!({"attrs": {"firstLineIndent": v}}));
        }
        caption(ui, "↓¶");
        if let Some(v) = measure(ui, "csa", para["spaceAfter"].as_f64(), units, 54.0) {
            let _ = app.run("type.para", json!({"attrs": {"spaceAfter": v}}));
        }
        ui.end_row();
    });
    vsep(ui);
    ui.vertical(|ui| {
        let ps = attrs.as_ref().and_then(|a| a["paragraphStyle"].as_str()).unwrap_or("").to_string();
        let ov = attrs.as_ref().and_then(|a| a["paraOverrides"].as_u64()).unwrap_or(0)
            + attrs.as_ref().and_then(|a| a["charOverrides"].as_u64()).unwrap_or(0);
        panels::para_style_picker(app, ui, &ps, ov > 0, 170.0);
        let hy = para["hyphenate"].as_bool().unwrap_or(true);
        let mut h = hy;
        if ui.checkbox(&mut h, crate::rtl::widget(ui, crate::i18n::tr(&app.ui.language, "Hyphenate"))).changed() {
            let _ = app.run("type.para", json!({"attrs": {"hyphenate": h}}));
        }
    });
}

pub fn doc_tabs(app: &mut DesignApp, ui: &mut egui::Ui) {
    let t = Tokens::get(ui.ctx());
    let (bar, _) = ui.allocate_exact_size(vec2(ui.available_width(), 28.0), Sense::hover());
    ui.painter().rect_filled(bar, 0.0, t.tab_strip);
    ui.painter().line_segment([bar.left_bottom(), bar.right_bottom()], Stroke::new(1.0, t.border));
    let mut x = bar.min.x;
    let active = app.session.active_index();
    let zoom = app.view().map(|v| v.zoom).unwrap_or(1.0);
    let mut activate = None;
    let mut close = None;
    let titles: Vec<(String, bool)> = app.session.documents().iter().map(|d| (d.title(), d.is_dirty())).collect();
    for (i, (title, dirty)) in titles.iter().enumerate() {
        let is_active = Some(i) == active;
        // `*` = unsaved; the view-mode suffix only in Preview (InDesign shows its GPU mode there).
        let suffix = if is_active && app.ui.screen_mode == crate::ScreenMode::Preview { " [Preview]" } else { "" };
        let label = format!("{}{} @ {:.0}%{suffix}", if *dirty { "*" } else { "" }, crate::rtl::isolate(title), zoom * 100.0);
        let galley = crate::rtl::plain(ui.ctx(), &label, semibold(11.5), if is_active { t.text_strong } else { t.text_dim });
        let w = (galley.size().x + 44.0).max(if is_active { 210.0 } else { 150.0 });
        let r = egui::Rect::from_min_size(egui::pos2(x, bar.min.y), vec2(w, 28.0));
        let resp = ui.interact(r, ui.id().with(("doctab", i)), Sense::click());
        ui.painter().rect_filled(r, 0.0, if is_active { t.panel } else { t.tab_strip });
        let cr = egui::Rect::from_center_size(egui::pos2(r.min.x + 18.0, r.center().y), vec2(14.0, 14.0));
        let cresp = ui.interact(cr, ui.id().with(("docclose", i)), Sense::click());
        ui.painter().text(
            cr.center(),
            egui::Align2::CENTER_CENTER,
            "×",
            egui::FontId::proportional(14.0),
            if cresp.hovered() { t.text_strong } else { t.text_dim },
        );
        ui.painter().galley(egui::pos2(r.min.x + 32.0, r.center().y - galley.size().y / 2.0), galley, Color32::WHITE);
        ui.painter().line_segment([r.right_top(), r.right_bottom()], Stroke::new(1.0, t.border));
        if cresp.clicked() {
            close = Some(i);
        } else if resp.clicked() {
            activate = Some(i);
        }
        x += w;
    }
    if let Some(i) = close {
        let _ = app.run("file.close", json!({"index": i}));
    } else if let Some(i) = activate {
        let _ = app.run("file.activate", json!({"index": i}));
    }
}

pub fn status_bar(app: &mut DesignApp, ui: &mut egui::Ui) {
    let t = Tokens::get(ui.ctx());
    let resp = egui::Panel::bottom("status_bar")
        .exact_size(17.0)
        .frame(egui::Frame::NONE.fill(t.panel).inner_margin(egui::Margin::symmetric(8, 0)))
        .show(ui, |ui| {
            ui.spacing_mut().item_spacing.x = 4.0;
            ui.horizontal_centered(|ui| {
                let small = egui::FontId::proportional(10.5);
                if app.session.active().is_none() {
                    ui.label(egui::RichText::new(&app.ui.status).font(small).color(t.text_dim));
                    return;
                }
                let z = app.view().map(|v| v.zoom).unwrap_or(1.0);
                ui.menu_button(egui::RichText::new(format!("{:.2}% ▾", z * 100.0)).font(small.clone()), |ui| {
                    for p in [0.05, 0.125, 0.25, 0.5, 0.75, 1.0, 2.0, 4.0, 8.0, 16.0] {
                        if ui.button(format!("{}%", p * 100.0)).clicked() {
                            let _ = app.run("view.zoom", json!({"zoom": p}));
                            ui.close();
                        }
                    }
                });
                ui.add_space(8.0);
                let n = app.session.active().map(|d| d.doc.page_count()).unwrap_or(1);
                let cur = crate::canvas::current_page(app).unwrap_or(0);
                if icons::button(ui, "first", 14.0, false, crate::i18n::tr(&app.ui.language, "First Spread")).clicked() {
                    crate::canvas::go_to_page(app, 0);
                }
                if icons::button(ui, "prev", 14.0, false, crate::i18n::tr(&app.ui.language, "Previous Spread")).clicked() {
                    crate::canvas::go_to_page(app, cur.saturating_sub(1));
                }
                let name = app.session.page_label(cur);
                let (fr, fresp) = ui.allocate_exact_size(vec2(80.0, 14.0), Sense::click());
                ui.painter().rect(fr, 0.0, t.input, Stroke::new(1.0, t.field_border), egui::StrokeKind::Inside);
                ui.painter().text(fr.left_center() + vec2(5.0, 0.0), egui::Align2::LEFT_CENTER, &name, small.clone(), t.text);
                ui.painter().text(fr.right_center() - vec2(5.0, 0.0), egui::Align2::RIGHT_CENTER, "▾", small.clone(), t.text);
                egui::Popup::menu(&fresp).show(|ui| {
                    let names: Vec<String> = (0..n).map(|i| app.session.page_label(i)).collect();
                    egui::ScrollArea::vertical().max_height(300.0).show(ui, |ui| {
                        for (i, nm) in names.iter().enumerate() {
                            if ui.button(nm).clicked() {
                                crate::canvas::go_to_page(app, i);
                                ui.close();
                            }
                        }
                    });
                });
                if icons::button(ui, "next", 14.0, false, crate::i18n::tr(&app.ui.language, "Next Spread")).clicked() {
                    crate::canvas::go_to_page(app, (cur + 1).min(n - 1));
                }
                if icons::button(ui, "last", 14.0, false, crate::i18n::tr(&app.ui.language, "Last Spread")).clicked() {
                    crate::canvas::go_to_page(app, n - 1);
                }
                ui.add_space(12.0);
                crate::rtl::label(ui, egui::RichText::new(crate::i18n::tr(&app.ui.language, "[Basic] (working) ▾")).font(small.clone()));
                ui.add_space(8.0);
                // Preflight: overset text is an error.
                let errors = panels::preflight_errors(app);
                let (r, _) = ui.allocate_exact_size(vec2(9.0, 9.0), Sense::hover());
                ui.painter().circle_filled(
                    r.center(),
                    4.0,
                    if errors == 0 { Color32::from_rgb(60, 200, 90) } else { Color32::from_rgb(235, 50, 50) },
                );
                crate::rtl::label(
                    ui,
                    egui::RichText::new(if errors == 0 {
                        format!("{} ▾", crate::i18n::tr(&app.ui.language, "No errors"))
                    } else {
                        format!(
                            "{} ▾",
                            crate::i18n::count_label(
                                &app.ui.language,
                                if errors == 1 && app.ui.language != "uk" { "error" } else { "errors" },
                                errors
                            )
                        )
                    })
                    .font(small.clone()),
                );
                ui.add_space(12.0);
                ui.label(egui::RichText::new(&app.ui.status).font(small).color(t.text_dim));
            });
        });
    let r = resp.response.rect;
    ui.painter().line_segment([r.left_top(), r.right_top()], Stroke::new(1.0, t.border));
}

pub fn start_screen(app: &mut DesignApp, ui: &mut egui::Ui) {
    let t = Tokens::get(ui.ctx());
    let r = ui.available_rect_before_wrap();
    ui.painter().rect_filled(r, 0.0, t.panel_darker);
    ui.scope_builder(
        egui::UiBuilder::new().max_rect(r.shrink(48.0)).layout(egui::Layout::top_down(if crate::i18n::is_rtl(&app.ui.language) {
            egui::Align::Max
        } else {
            egui::Align::Min
        })),
        |ui| {
            ui.label(crate::rtl::widget(
                ui,
                egui::RichText::new(crate::i18n::tr(&app.ui.language, "Welcome to DesignCraft")).font(semibold(28.0)).color(t.text_strong),
            ));
            ui.label(crate::rtl::widget(
                ui,
                egui::RichText::new(crate::i18n::tr(&app.ui.language, "Page layout for print and screen — fast, open and scriptable."))
                    .size(15.0)
                    .color(t.text_dim),
            ));
            ui.add_space(24.0);
            ui.horizontal(|ui| {
                if ui
                    .add(
                        egui::Button::new(crate::rtl::widget(
                            ui,
                            egui::RichText::new(crate::i18n::tr(&app.ui.language, "  New file  ")).size(14.0).color(Color32::WHITE),
                        ))
                        .fill(t.accent_strong)
                        .corner_radius(16.0)
                        .min_size(vec2(0.0, 32.0)),
                    )
                    .clicked()
                {
                    let _ = app.run("app.newDocumentDialog", json!({}));
                }
                if ui
                    .add(
                        egui::Button::new(crate::rtl::widget(ui, egui::RichText::new(crate::i18n::tr(&app.ui.language, "  Open  ")).size(14.0)))
                            .corner_radius(16.0)
                            .min_size(vec2(0.0, 32.0)),
                    )
                    .clicked()
                {
                    let _ = app.run("app.openDialog", json!({}));
                }
                if ui
                    .add(
                        egui::Button::new(crate::rtl::widget(
                            ui,
                            egui::RichText::new(crate::i18n::tr(&app.ui.language, "  Open sample magazine  ")).size(14.0),
                        ))
                        .corner_radius(16.0)
                        .min_size(vec2(0.0, 32.0)),
                    )
                    .clicked()
                {
                    let _ = app.run("file.newSample", json!({}));
                }
            });
            ui.add_space(20.0);
            community_card(app, ui);
            ui.add_space(24.0);
            ui.label(crate::rtl::widget(
                ui,
                egui::RichText::new(crate::i18n::tr(&app.ui.language, "Start a new document")).font(semibold(15.0)).color(t.text_strong),
            ));
            ui.add_space(8.0);
            ui.horizontal_wrapped(|ui| {
                for p in designcraft_doc::build::PRESETS.iter().filter(|p| p.intent == designcraft_doc::Intent::Print) {
                    let (cr, resp) = ui.allocate_exact_size(vec2(120.0, 150.0), Sense::click());
                    let hov = resp.hovered();
                    ui.painter().rect_filled(cr, 6.0, if hov { t.hover } else { t.panel });
                    let k = (80.0 / p.width.max(p.height)) as f32;
                    let pr = egui::Rect::from_center_size(cr.center() - vec2(0.0, 14.0), vec2(p.width as f32 * k, p.height as f32 * k));
                    ui.painter().rect_filled(pr, 0.0, Color32::from_gray(245));
                    ui.painter().text(egui::pos2(cr.center().x, cr.max.y - 26.0), egui::Align2::CENTER_CENTER, p.name, semibold(12.0), t.text_strong);
                    ui.painter().text(
                        egui::pos2(cr.center().x, cr.max.y - 11.0),
                        egui::Align2::CENTER_CENTER,
                        format!("{} × {}", designcraft_geom::format_measure(p.width, p.units), designcraft_geom::format_measure(p.height, p.units)),
                        egui::FontId::proportional(10.0),
                        t.text_dim,
                    );
                    if resp.clicked() {
                        let _ = app.run("file.new", json!({"preset": p.name}));
                    }
                }
            });
        },
    );
    let _ = Content::Unassigned;
}

/// Start screen: the ArtCraft community (Discord first) and project links.
fn community_card(app: &mut DesignApp, ui: &mut egui::Ui) {
    let t = Tokens::get(ui.ctx());
    egui::Frame::NONE.fill(t.panel).corner_radius(10.0).inner_margin(egui::Margin::same(14)).show(ui, |ui| {
        ui.set_max_width(640.0);
        ui.horizontal(|ui| {
            let (r, _) = ui.allocate_exact_size(vec2(44.0, 44.0), Sense::hover());
            crate::about::paint_mark(ui, r, crate::about::BRAND);
            ui.add_space(8.0);
            ui.vertical(|ui| {
                ui.label(crate::rtl::widget(
                    ui,
                    egui::RichText::new(crate::i18n::tr(&app.ui.language, "Join the ArtCraft community")).font(semibold(14.0)).color(t.text_strong),
                ));
                ui.label(crate::rtl::widget(
                    ui,
                    egui::RichText::new(crate::i18n::tr(&app.ui.language, "Get help, share your layouts and shape what we build next."))
                        .size(12.0)
                        .color(t.text_dim),
                ));
                ui.add_space(4.0);
                ui.horizontal(|ui| {
                    for (text, id) in [("DesignCraft page", "help.appPage"), ("GitHub", "help.github"), ("getartcraft.com", "help.website")] {
                        let url = designcraft_engine::links::get(&id[5..]).unwrap_or_default();
                        let l = ui
                            .add(egui::Label::new(egui::RichText::new(text).size(12.0).color(t.accent).underline()).sense(Sense::click()))
                            .on_hover_text(url)
                            .on_hover_cursor(egui::CursorIcon::PointingHand);
                        if l.clicked() {
                            let _ = app.run(id, json!({}));
                        }
                        ui.add_space(8.0);
                    }
                });
            });
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if crate::about::discord_button(ui, crate::i18n::tr(&app.ui.language, "Join our Discord"), vec2(190.0, 38.0)) {
                    let _ = app.run("help.discord", json!({}));
                }
            });
        });
    });
}

#[cfg(test)]
mod tests {
    use egui::{pos2, vec2};
    use egui_kittest::kittest::Queryable;

    use crate::test_window::{self, wheel};

    /// The Control panel's last control with nothing selected.
    const LAST_CONTROL: &str = "Jump to Next Column";
    const FIRST_CONTROL: &str = "Rotate 90° Counterclockwise";

    fn app() -> crate::DesignApp {
        let mut app = crate::DesignApp::new(designcraft_engine::Session::new(), crate::Services::default());
        app.ui.control_bar = true;
        app
    }

    fn rect(h: &egui_kittest::Harness<'static, test_window::Window>, label: &str) -> egui::Rect {
        h.get_by_label(label).rect()
    }

    #[test]
    fn a_narrow_window_scrolls_the_control_bar() {
        let w = 700.0;
        let mut h = test_window::open(app(), vec2(w, 600.0));
        let bar = test_window::panel_rect(&h, "control_bar");
        assert!(rect(&h, LAST_CONTROL).max.x > w, "the controls are wider than the window");
        // A plain mouse wheel scrolls the bar sideways.
        let first = rect(&h, FIRST_CONTROL).center();
        wheel(&mut h, first, vec2(0.0, -3000.0));
        let last = rect(&h, LAST_CONTROL);
        let window = egui::Rect::from_min_size(pos2(0.0, 0.0), vec2(w, 600.0));
        assert!(window.contains_rect(last) && bar.contains_rect(last), "{last:?} outside the bar {bar:?}");
        assert_eq!(test_window::panel_rect(&h, "control_bar").height(), bar.height(), "the bar keeps its height");
        // A popup opened from a scrolled control opens at that control.
        let fill = h.get_by_label("Fill").rect();
        let chip = pos2(fill.max.x + 15.0, fill.center().y);
        h.hover_at(chip);
        h.drag_at(chip);
        h.drop_at(chip);
        h.run_steps(4);
        let item = rect(&h, "[Black]");
        assert!(item.min.y > fill.max.y && (item.min.x - chip.x).abs() < 120.0, "popup item at {item:?}, control at {chip:?}");
    }

    #[test]
    fn a_narrow_window_scrolls_the_app_bar() {
        let w = 640.0;
        let mut h = test_window::open(app(), vec2(w, 600.0));
        let (help, share) = (rect(&h, "Help"), rect(&h, "Share"));
        assert!(help.max.x <= share.min.x, "the menus ({help:?}) and the workspace controls ({share:?}) overlap");
        wheel(&mut h, help.center(), vec2(0.0, -3000.0));
        let share = rect(&h, "Share");
        let window = egui::Rect::from_min_size(pos2(0.0, 0.0), vec2(w, 600.0));
        assert!(window.contains_rect(share), "{share:?} outside the window");
    }

    #[test]
    fn a_wide_window_does_not_scroll_the_bars() {
        let mut h = test_window::open(app(), vec2(1400.0, 900.0));
        let before = [rect(&h, FIRST_CONTROL), rect(&h, LAST_CONTROL), rect(&h, "Help"), rect(&h, "Share")];
        assert!(before.iter().all(|r| r.max.x <= 1400.0), "{before:?}");
        wheel(&mut h, before[0].center(), vec2(0.0, -3000.0));
        wheel(&mut h, before[2].center(), vec2(0.0, -3000.0));
        let after = [rect(&h, FIRST_CONTROL), rect(&h, LAST_CONTROL), rect(&h, "Help"), rect(&h, "Share")];
        assert_eq!(after, before);
    }

    #[test]
    fn a_scrolled_bar_keeps_its_captions_on_one_line() {
        let mut app = app();
        app.run("file.new", serde_json::json!({})).unwrap();
        let f = app.run("frame.create", serde_json::json!({"rect": [72, 72, 300, 200], "content": "text", "text": "Hello"})).unwrap();
        app.run("selection.set", serde_json::json!({"ids": [f["id"]]})).unwrap();
        let mut h = test_window::open(app, vec2(700.0, 600.0));
        let bar = test_window::panel_rect(&h, "control_bar").center();
        wheel(&mut h, bar, vec2(0.0, -3000.0));
        // The text frame's last group: its caption gets only what is left of the row.
        let columns = h.get_by_label("Columns").rect();
        assert!(columns.height() < 20.0 && columns.width() > columns.height(), "Columns wraps: {columns:?}");
    }
}
