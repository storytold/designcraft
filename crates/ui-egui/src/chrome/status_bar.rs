use super::{DesignApp, json, panels};

/// The widest a document tab grows, which the tab tests hold the tab strip to.
#[cfg(test)]
pub(super) const DOC_TAB_MAX_W: f32 = 260.0;

/// Draw bounded document tabs with active, dirty, preview and zoom state, plus activation and close actions.
pub fn doc_tabs(app: &mut DesignApp, ui: &mut egui::Ui) {
    framework::doc_tabs(app, ui);
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
    framework::status_bar(app, ui);
}

/// The document tabs and the status bar on the framework's components: the same commands, the same page box and
/// Preflight menu, and the same refusal to run anything for a click that a longer press turns into something else.
mod framework {
    use super::{DesignApp, json, panels};
    use crate::panels::tabs::TabGlyph;
    use craft_ui_widgets::bars::{Bar, BarKind};
    use craft_ui_widgets::buttons::{Button, IconButton};
    use craft_ui_widgets::contract::{Bind, Entry, Props as _, Show as _, ShowContainer as _, Sizing, Trigger};
    use craft_ui_widgets::display::{Divider, Picture, Shade, Text};
    use craft_ui_widgets::list::ThumbAspect;
    use craft_ui_widgets::menu::{Menu, MenuItem};
    use craft_ui_widgets::pickers::Select;
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
        // The bar is as tall as its small controls: a panel given less (the native bar's 17 pt) hides members that don't fit it whole.
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
                // The page box, as InDesign's: the page in view, or the parent while one is shown ("A-Parent"); its list
                // holds the pages, then the parents. It is a select, so its accessible value is the name shown.
                if let Some(b) = &page_box {
                    let entries: Vec<Entry<usize>> = b.names.iter().enumerate().map(|(i, nm)| Entry::new(i, nm.clone())).collect();
                    let picked =
                        Select::new(tr("Page"), Bind::given(b.current), &entries).id_salt("status_page").show(ui).events.committed().copied();
                    if let Some(i) = picked {
                        asks.push(Ask::PageBox(i));
                    }
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
                Ask::Preflight => {
                    // A double click on the count shows the panel at once and leaves no menu open behind it (the first click
                    // opened it; the second would close it, but the click that shows the panel is the one the menu keeps).
                    craft_ui_widgets::runtime::overlay::close_popups(ui.ctx());
                    super::show_preflight(app);
                }
            }
        }
    }
}
