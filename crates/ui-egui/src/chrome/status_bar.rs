use super::{Color32, DesignApp, Sense, Stroke, Tokens, icons, json, panels, semibold, vec2};

/// The widest a document tab grows: a longer name is shortened so its close button and the
/// tabs after it stay in view.
pub(super) const DOC_TAB_MAX_W: f32 = 260.0;

/// Close button and padding around a tab's title.
const DOC_TAB_PAD: f32 = 44.0;

/// Draw bounded document tabs with active, dirty, preview and zoom state, plus activation and close actions.
pub fn doc_tabs(app: &mut DesignApp, ui: &mut egui::Ui) {
    let t = Tokens::get(ui.ctx());
    let (bar, _) = ui.allocate_exact_size(vec2(ui.available_width(), 28.0), Sense::hover());
    let max_w = DOC_TAB_MAX_W.min(bar.width());
    let close_label = crate::i18n::tr(&app.ui.language, "Close");
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
        let color = if is_active { t.text_strong } else { t.text_dim };
        // The name gives way first; the dirty marker and zoom stay readable.
        let zoom_label = crate::rtl::plain(ui.ctx(), &format!(" @ {:.0}%{suffix}", zoom * 100.0), semibold(11.5), color);
        let name = format!("{}{}", if *dirty { "*" } else { "" }, crate::rtl::isolate(title));
        let name_max = max_w - DOC_TAB_PAD - zoom_label.size().x;
        let galley = crate::rtl::truncated(ui.ctx(), &name, semibold(11.5), color, name_max);
        let elided = galley.elided;
        let w = (galley.size().x + zoom_label.size().x + DOC_TAB_PAD).max(if is_active { 210.0 } else { 150.0 }).min(max_w);
        let r = egui::Rect::from_min_size(egui::pos2(x, bar.min.y), vec2(w, 28.0));
        let resp = ui.interact(r, ui.scope_id().with(("doctab", i)), Sense::click());
        resp.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Button, true, title));
        let resp = if elided {
            resp.on_hover_ui(|ui| {
                let _ = crate::rtl::label(ui, title);
            })
        } else {
            resp
        };
        ui.painter().rect_filled(r, 0.0, if is_active { t.panel } else { t.tab_strip });
        let cr = egui::Rect::from_center_size(egui::pos2(r.min.x + 18.0, r.center().y), vec2(14.0, 14.0));
        let cresp = ui.interact(cr, ui.scope_id().with(("docclose", i)), Sense::click());
        cresp.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Button, true, format!("{close_label} {title}")));
        ui.painter().text(
            cr.center(),
            egui::Align2::CENTER_CENTER,
            "×",
            egui::FontId::proportional(14.0),
            if cresp.hovered() { t.text_strong } else { t.text_dim },
        );
        let name_w = galley.size().x;
        // Clipped too, for a window narrower than the zoom label.
        let text = ui.painter().with_clip_rect(r);
        text.galley(egui::pos2(r.min.x + 32.0, r.center().y - galley.size().y / 2.0), galley, Color32::WHITE);
        text.galley(egui::pos2(r.min.x + 32.0 + name_w, r.center().y - zoom_label.size().y / 2.0), zoom_label, Color32::WHITE);
        ui.painter().line_segment([r.right_top(), r.right_bottom()], Stroke::new(1.0, t.border));
        if cresp.clicked() {
            close = Some(i);
        } else if resp.clicked() {
            activate = Some(i);
        }
        x += w;
    }
    if let Some(i) = close {
        crate::menus::close_document(app, Some(i));
    } else if let Some(i) = activate {
        let _ = app.run("file.activate", json!({"index": i}));
    }
}

/// Show the Preflight panel. An open Preflight flyout stays: `window.panel` would close it.
fn show_preflight(app: &mut DesignApp) {
    if app.ui.docking_flyout.as_deref() != Some("preflight")
        && let Err(e) = app.run("window.panel", json!({"panel": "preflight"}))
    {
        app.status(e);
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
                crate::menus::menu_button(ui, egui::RichText::new(format!("{:.2}% ▾", z * 100.0)).font(small.clone()), |ui| {
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
                // The page box, as InDesign's: the page in view, or the parent while one is shown
                // ("A-Parent"); its menu lists the pages, then the parents.
                let pb = crate::canvas::page_box(app);
                let name = pb.as_ref().map_or_else(String::new, |b| b.current_name().to_string());
                let (fr, fresp) = ui.allocate_exact_size(vec2(80.0, 14.0), Sense::click());
                ui.painter().rect(fr, 0.0, t.input, Stroke::new(1.0, t.field_border), egui::StrokeKind::Inside);
                // A long parent name is cut at the arrow.
                ui.painter().with_clip_rect(egui::Rect::from_min_max(fr.min, egui::pos2(fr.max.x - 14.0, fr.max.y))).text(
                    fr.left_center() + vec2(5.0, 0.0),
                    egui::Align2::LEFT_CENTER,
                    &name,
                    small.clone(),
                    t.text,
                );
                ui.painter().text(fr.right_center() - vec2(5.0, 0.0), egui::Align2::RIGHT_CENTER, "▾", small.clone(), t.text);
                let label = crate::i18n::tr(&app.ui.language, "Page");
                fresp.widget_info(|| {
                    let mut info = egui::WidgetInfo::labeled(egui::Role::ComboBox, true, label);
                    info.current_text_value = Some(name.clone());
                    info
                });
                let mut picked = None;
                if let Some(b) = &pb {
                    egui::Popup::menu(&fresp).show(|ui| {
                        egui::ScrollArea::vertical().max_height(300.0).show(ui, |ui| {
                            for (i, nm) in b.names.iter().enumerate() {
                                if i == b.pages && i > 0 {
                                    ui.separator();
                                }
                                if ui.selectable_label(i == b.current, nm).clicked() {
                                    picked = Some(i);
                                    ui.close();
                                }
                            }
                        });
                    });
                }
                if let (Some(b), Some(i)) = (&pb, picked) {
                    crate::canvas::page_box_go(app, b, i);
                }
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
                let (r, dot) = ui.allocate_exact_size(vec2(9.0, 9.0), Sense::click());
                ui.painter().circle_filled(
                    r.center(),
                    4.0,
                    if errors == 0 { Color32::from_rgb(60, 200, 90) } else { Color32::from_rgb(235, 50, 50) },
                );
                let count = if errors == 0 {
                    format!("{} ▾", crate::i18n::tr(&app.ui.language, "No errors"))
                } else {
                    format!(
                        "{} ▾",
                        crate::i18n::count_label(
                            &app.ui.language,
                            if errors == 1 && !crate::i18n::category_first(&app.ui.language) { "error" } else { "errors" },
                            errors
                        )
                    )
                };
                // As in InDesign, the count opens a menu that leads to the Preflight panel.
                let menu = crate::menus::menu_button(ui, egui::RichText::new(count).font(small.clone()), |ui| {
                    if ui.button(crate::i18n::tr(&app.ui.language, "Preflight Panel…")).clicked() {
                        show_preflight(app);
                        ui.close();
                    }
                });
                // Double-clicking the light or the count shows the panel straight away (the
                // second click has closed the menu the first one opened).
                if dot.double_clicked() || menu.response.double_clicked() {
                    show_preflight(app);
                }
                ui.add_space(12.0);
                ui.label(egui::RichText::new(&app.ui.status).font(small).color(t.text_dim));
            });
        });
    let r = resp.response.rect;
    ui.painter().line_segment([r.left_top(), r.right_top()], Stroke::new(1.0, t.border));
}
