use super::{Color32, DesignApp, Sense, Stroke, Tokens, icons, json, panels, semibold, vec2};

/// The widest a document tab grows: a longer name is shortened so its close button and the
/// tabs after it stay in view.
pub(super) const DOC_TAB_MAX_W: f32 = 260.0;

/// Width of the scroll arrows and the document list button at the ends of an overflowing tab bar.
const DOC_TAB_BUTTON_W: f32 = 22.0;

/// Close button and padding around a tab's title.
const DOC_TAB_PAD: f32 = 44.0;

/// Draw bounded document tabs with active, dirty, preview and zoom state, plus activation and close actions.
pub fn doc_tabs(app: &mut DesignApp, ui: &mut egui::Ui) {
    #[cfg(not(target_arch = "wasm32"))]
    if app.framework_installed {
        framework::doc_tabs(app, ui);
        return;
    }
    let t = Tokens::get(ui.ctx());
    let (bar, _) = ui.allocate_exact_size(vec2(ui.available_width(), 28.0), Sense::hover());
    let max_w = DOC_TAB_MAX_W.min(bar.width());
    let close_label = crate::i18n::tr(&app.ui.language, "Close");
    ui.painter().rect_filled(bar, 0.0, t.tab_strip);
    ui.painter().line_segment([bar.left_bottom(), bar.right_bottom()], Stroke::new(1.0, t.border));
    // Tabs past the bar scroll sideways: trackpad, shift or a plain wheel, and the active tab is kept in view.
    let scroll_id = ui.scope_id().with("doctabs_scroll");
    let (mut offset, total_w, seen_active, seen_w): (f32, f32, Option<u64>, f32) = ui.data(|d| d.get_temp(scroll_id)).unwrap_or_default();
    if ui.rect_contains_pointer(bar) {
        let d = ui.input(|i| i.smooth_scroll_delta);
        offset -= if d.x.abs() > d.y.abs() { d.x } else { d.y };
    }
    // Arrows and the document list take the bar's ends once the tabs overflow.
    let overflow = total_w > bar.width();
    let tabs =
        if overflow { egui::Rect::from_min_max(bar.min + vec2(DOC_TAB_BUTTON_W, 0.0), bar.max - vec2(2.0 * DOC_TAB_BUTTON_W, 0.0)) } else { bar };
    offset = offset.clamp(0.0, (total_w - tabs.width()).max(0.0));
    let painter = ui.painter().with_clip_rect(tabs);
    let origin = tabs.min.x - offset;
    let mut x = origin;
    let active = app.session.active_index();
    let zoom = app.view().map(|v| v.zoom).unwrap_or(1.0);
    let mut activate = None;
    let mut close = None;
    let mut edges = Vec::new();
    let titles: Vec<(String, bool, u64)> = app.session.documents().iter().map(|d| (d.title(), d.is_dirty(), d.uid)).collect();
    for (i, (title, dirty, uid)) in titles.iter().enumerate() {
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
        if is_active && (Some(*uid) != seen_active || (tabs.width() - seen_w).abs() > 0.5) {
            offset += (r.max.x - tabs.max.x).max(0.0) + (r.min.x - tabs.min.x).min(0.0);
            ui.request_repaint();
        }
        let resp = ui.interact(r.intersect(tabs), ui.scope_id().with(("doctab", i)), Sense::click());
        resp.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Button, true, title));
        let resp = if elided {
            resp.on_hover_ui(|ui| {
                let _ = crate::rtl::label(ui, title);
            })
        } else {
            resp
        };
        painter.rect_filled(r, 0.0, if is_active { t.panel } else { t.tab_strip });
        let cr = egui::Rect::from_center_size(egui::pos2(r.min.x + 18.0, r.center().y), vec2(14.0, 14.0));
        let cresp = ui.interact(cr.intersect(tabs), ui.scope_id().with(("docclose", i)), Sense::click());
        cresp.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Button, true, format!("{close_label} {title}")));
        painter.text(
            cr.center(),
            egui::Align2::CENTER_CENTER,
            "×",
            egui::FontId::proportional(14.0),
            if cresp.hovered() { t.text_strong } else { t.text_dim },
        );
        let name_w = galley.size().x;
        // Clipped too, for a window narrower than the zoom label.
        let text = painter.with_clip_rect(r.intersect(tabs));
        text.galley(egui::pos2(r.min.x + 32.0, r.center().y - galley.size().y / 2.0), galley, Color32::WHITE);
        text.galley(egui::pos2(r.min.x + 32.0 + name_w, r.center().y - zoom_label.size().y / 2.0), zoom_label, Color32::WHITE);
        painter.line_segment([r.right_top(), r.right_bottom()], Stroke::new(1.0, t.border));
        if cresp.clicked() {
            close = Some(i);
        } else if resp.clicked() {
            activate = Some(i);
        }
        edges.push((r.min.x, r.max.x));
        x += w;
    }
    if overflow {
        let button = |i: f32| {
            egui::Rect::from_min_size(
                egui::pos2(if i < 0.0 { bar.min.x } else { bar.max.x - (i + 1.0) * DOC_TAB_BUTTON_W }, bar.min.y),
                vec2(DOC_TAB_BUTTON_W, 28.0),
            )
        };
        let max_offset = (x - origin - tabs.width()).max(0.0);
        for (rect, glyph, label, dir, enabled) in
            [(button(-1.0), "‹", "Scroll Tabs Left", -1.0, offset > 0.0), (button(1.0), "›", "Scroll Tabs Right", 1.0, offset < max_offset)]
        {
            let label = crate::i18n::tr(&app.ui.language, label);
            let resp = ui.interact(rect, ui.scope_id().with(("doctab_scroll", glyph)), Sense::click()).on_hover_text(label);
            resp.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Button, enabled, label));
            let color = if !enabled {
                t.text_dim.gamma_multiply(0.4)
            } else if resp.hovered() {
                t.text_strong
            } else {
                t.text_dim
            };
            ui.painter().text(rect.center(), egui::Align2::CENTER_CENTER, glyph, egui::FontId::proportional(16.0), color);
            if enabled && resp.clicked() {
                // One tab at a time: the next one hidden on that side comes fully into view.
                offset += if dir > 0.0 {
                    edges.iter().find(|e| e.1 > tabs.max.x + 0.5).map(|e| e.1 - tabs.max.x)
                } else {
                    edges.iter().rev().find(|e| e.0 < tabs.min.x - 0.5).map(|e| e.0 - tabs.min.x)
                }
                .unwrap_or(0.0);
                ui.request_repaint();
            }
        }
        let mut list =
            ui.new_child(egui::UiBuilder::new().max_rect(button(0.0)).layout(egui::Layout::centered_and_justified(egui::Direction::LeftToRight)));
        let widgets = &mut list.visuals_mut().widgets;
        for state in [&mut widgets.inactive, &mut widgets.hovered, &mut widgets.active, &mut widgets.open] {
            state.weak_bg_fill = Color32::TRANSPARENT;
            state.bg_stroke = Stroke::NONE;
        }
        crate::menus::menu_button(&mut list, egui::RichText::new("▾").size(14.0).color(t.text_dim), |ui| {
            for (i, (title, ..)) in titles.iter().enumerate() {
                if ui.selectable_label(Some(i) == active, crate::rtl::isolate(title)).clicked() {
                    activate = Some(i);
                    ui.close();
                }
            }
        });
    }
    // The arrows come or go once this frame's width is known: the active tab is brought into view again in the new layout.
    let overflows = x - origin > bar.width();
    if overflows != overflow {
        ui.request_repaint();
    }
    let seen = if overflows == overflow { active.and_then(|i| titles.get(i)).map(|t| t.2) } else { seen_active };
    ui.data_mut(|d| d.insert_temp(scroll_id, (offset, x - origin, seen, tabs.width())));
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
    #[cfg(not(target_arch = "wasm32"))]
    if app.framework_installed {
        framework::status_bar(app, ui);
        return;
    }
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
                let errors = panels::preflight_errors(app, ui.ctx());
                let (r, dot) = ui.allocate_exact_size(vec2(9.0, 9.0), Sense::click());
                ui.painter().circle_filled(
                    r.center(),
                    4.0,
                    match errors {
                        None => t.text_dim,
                        Some(0) => Color32::from_rgb(60, 200, 90),
                        Some(_) => Color32::from_rgb(235, 50, 50),
                    },
                );
                let count = match errors {
                    // The active document's first check hasn't ended.
                    None => format!("{} ▾", crate::i18n::tr(&app.ui.language, "Checking")),
                    Some(0) => format!("{} ▾", crate::i18n::tr(&app.ui.language, "No errors")),
                    Some(errors) => format!(
                        "{} ▾",
                        crate::i18n::count_label(
                            &app.ui.language,
                            if errors == 1 && !crate::i18n::category_first(&app.ui.language) { "error" } else { "errors" },
                            errors
                        )
                    ),
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

/// The document tabs and the status bar on the framework's components: the same commands, the same page box and
/// Preflight menu, and the same refusal to run anything for a click that a longer press turns into something else.
#[cfg(not(target_arch = "wasm32"))]
mod framework {
    use super::{DesignApp, json, panels};
    use crate::panels::tabs::TabGlyph;
    use craft_ui_widgets::bars::{Bar, BarKind};
    use craft_ui_widgets::buttons::{Button, IconButton};
    use craft_ui_widgets::contract::{Props as _, Show as _, ShowContainer as _, Sizing, Trigger};
    use craft_ui_widgets::display::{Divider, Picture, Shade, Text};
    use craft_ui_widgets::list::ThumbAspect;
    use craft_ui_widgets::menu::{Menu, MenuItem};
    use craft_ui_widgets::primitives::IconSpec;
    use craft_ui_widgets::style::{TypeRole, Variant};
    use craft_ui_widgets::tabs::{DocumentTab, DocumentTabs, DocumentTabsEvent};

    /// The document tab strip: a tab per document with its title, the zoom of the view and the unsaved mark, the close
    /// button, and the overflow menu the strip has when the tabs run past its end.
    pub(super) fn doc_tabs(app: &mut DesignApp, ui: &mut egui::Ui) {
        let lang = app.ui.language.clone();
        let tr = |s: &str| crate::i18n::tr(&lang, s).to_string();
        let zoom = app.view().map(|v| v.zoom).unwrap_or(1.0);
        let active = app.session.active_index();
        let docs: Vec<(u64, String, bool)> = app.session.documents().iter().map(|d| (d.uid, d.title(), d.is_dirty())).collect();
        let preview = app.ui.screen_mode == crate::ScreenMode::Preview;
        let tabs: Vec<DocumentTab<u64>> = docs
            .iter()
            .enumerate()
            .map(|(i, (uid, title, dirty))| {
                // The view-mode suffix only in Preview (InDesign shows its GPU mode there).
                let suffix = if Some(i) == active && preview { " [Preview]" } else { "" };
                let label = format!("{} @ {:.0}%{suffix}", crate::rtl::isolate(title), zoom * 100.0);
                DocumentTab::new(*uid, label).modified(*dirty).close_label(format!("{} {title}", tr("Close")))
            })
            .collect();
        let selected = active.and_then(|i| docs.get(i)).map(|d| d.0);
        let mut strip = DocumentTabs::new(tr("Documents"), &tabs);
        if let Some(uid) = selected {
            strip = strip.selected(uid);
        }
        let events = strip.show(ui).events;
        let index_of = |uid: u64| docs.iter().position(|d| d.0 == uid);
        for event in events {
            match event {
                DocumentTabsEvent::Selected(uid) | DocumentTabsEvent::Picked(uid) => {
                    if let Some(i) = index_of(uid) {
                        let _ = app.run("file.activate", json!({"index": i}));
                    }
                }
                DocumentTabsEvent::CloseRequested(uid) => {
                    if let Some(i) = index_of(uid) {
                        crate::menus::close_document(app, Some(i));
                    }
                }
                _ => {}
            }
        }
    }

    /// What a pass of the status bar asks for.
    enum Ask {
        Zoom(f64),
        Page(usize),
        PageBox(usize),
        Preflight,
    }

    pub(super) fn status_bar(app: &mut DesignApp, ui: &mut egui::Ui) {
        let t = crate::theme::Tokens::get(ui.ctx());
        let lang = app.ui.language.clone();
        let tr = |s: &str| crate::i18n::tr(&lang, s).to_string();
        let has_doc = app.session.active().is_some();
        let zoom = app.view().map(|v| v.zoom).unwrap_or(1.0);
        let pages = app.session.active().map(|d| d.doc.page_count()).unwrap_or(1);
        let cur = crate::canvas::current_page(app).unwrap_or(0);
        let page_box = crate::canvas::page_box(app);
        let errors = if has_doc { panels::preflight_errors(app, ui.ctx()) } else { None };
        let status = app.ui.status.clone();
        let mut asks: Vec<Ask> = Vec::new();
        let preflight_count = match errors {
            // The active document's first check hasn't ended.
            None => format!("{} ▾", tr("Checking")),
            Some(0) => format!("{} ▾", tr("No errors")),
            Some(n) => {
                format!("{} ▾", crate::i18n::count_label(&lang, if n == 1 && !crate::i18n::category_first(&lang) { "error" } else { "errors" }, n))
            }
        };
        let dot = match errors {
            None => t.text_dim,
            Some(0) => egui::Color32::from_rgb(60, 200, 90),
            Some(_) => egui::Color32::from_rgb(235, 50, 50),
        };
        egui::Panel::bottom("status_bar").frame(egui::Frame::NONE).resizable(false).show(ui, |ui| {
            Bar::new(tr("Status"), BarKind::Status).show(ui, |ui, _| {
                if !has_doc {
                    Text::new(status.clone()).role(TypeRole::Caption).shade(Shade::Muted).sizing(Sizing::FillWidth).show(ui);
                    return;
                }
                let zoom_label = tr("Zoom");
                let opener = Button::new(zoom_label.clone())
                    .painted(format!("{:.2}% ▾", zoom * 100.0))
                    .id_salt("status_zoom")
                    .variant(Variant::GHOST)
                    .show(ui);
                let _ = Menu::new(zoom_label).id_salt("status_zoom_menu").opened_by(&opener.response, Trigger::Click).show(ui, |ui, _| {
                    for p in [0.05, 0.125, 0.25, 0.5, 0.75, 1.0, 2.0, 4.0, 8.0, 16.0] {
                        if MenuItem::new(format!("{}%", p * 100.0)).id_salt(format!("zoom_{p}")).show(ui).ran {
                            asks.push(Ask::Zoom(p));
                        }
                    }
                });
                Divider::vertical().show(ui);
                let nav = |ui: &mut egui::Ui, glyph: &'static str, label: &str| {
                    let face = TabGlyph(glyph);
                    IconButton::new(IconSpec::glyph(&face))
                        .id_salt(("status_nav", glyph))
                        .label(label.to_owned())
                        .tooltip(label.to_owned())
                        .show(ui)
                        .clicked()
                };
                if nav(ui, "first", &tr("First Spread")) {
                    asks.push(Ask::Page(0));
                }
                if nav(ui, "prev", &tr("Previous Spread")) {
                    asks.push(Ask::Page(cur.saturating_sub(1)));
                }
                // The page box, as InDesign's: the page in view, or the parent while one is shown ("A-Parent"); its menu
                // lists the pages, then the parents.
                let name = page_box.as_ref().map_or_else(String::new, |b| b.current_name().to_string());
                let label = tr("Page");
                let page = Button::new(label.clone()).painted(format!("{name} ▾")).id_salt("status_page").variant(Variant::SECONDARY).show(ui);
                if let Some(b) = &page_box {
                    let _ = Menu::new(label).id_salt("status_page_menu").opened_by(&page.response, Trigger::Click).show(ui, |ui, _| {
                        for (i, nm) in b.names.iter().enumerate() {
                            if i == b.pages && i > 0 {
                                Divider::horizontal().show(ui);
                            }
                            if MenuItem::new(nm.clone()).id_salt(("page", i)).check(Some(i == b.current)).show(ui).ran {
                                asks.push(Ask::PageBox(i));
                            }
                        }
                    });
                }
                if nav(ui, "next", &tr("Next Spread")) {
                    asks.push(Ask::Page((cur + 1).min(pages.saturating_sub(1))));
                }
                if nav(ui, "last", &tr("Last Spread")) {
                    asks.push(Ask::Page(pages.saturating_sub(1)));
                }
                Divider::vertical().show(ui);
                Text::new(tr("[Basic] (working) ▾")).role(TypeRole::Caption).show(ui);
                // Preflight: overset text is an error.
                Picture::painter("", ThumbAspect::SQUARE, move |cx, rect| {
                    cx.painter.circle_filled(rect.center(), 4.0, dot);
                })
                .id_salt("status_preflight_light")
                .show(ui);
                // As in InDesign, the count opens a menu that leads to the Preflight panel; a double click shows the panel
                // straight away.
                let count_label = tr("Preflight");
                let count =
                    Button::new(count_label.clone()).painted(preflight_count.clone()).id_salt("status_preflight").variant(Variant::GHOST).show(ui);
                if count.clicks >= 2 {
                    asks.push(Ask::Preflight);
                }
                let _ = Menu::new(count_label).id_salt("status_preflight_menu").opened_by(&count.response, Trigger::Click).show(ui, |ui, _| {
                    if MenuItem::new(tr("Preflight Panel…")).id_salt("preflight_panel").show(ui).ran {
                        asks.push(Ask::Preflight);
                    }
                });
                Text::new(status.clone()).role(TypeRole::Caption).shade(Shade::Muted).sizing(Sizing::FillWidth).show(ui);
            });
        });
        for ask in asks {
            match ask {
                Ask::Zoom(p) => {
                    let _ = app.run("view.zoom", json!({"zoom": p}));
                }
                Ask::Page(i) => crate::canvas::go_to_page(app, i),
                Ask::PageBox(i) => {
                    if let Some(b) = &page_box {
                        crate::canvas::page_box_go(app, b, i);
                    }
                }
                Ask::Preflight => super::show_preflight(app),
            }
        }
    }
}
