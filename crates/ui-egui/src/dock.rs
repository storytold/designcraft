//! The right dock: an expanded panel group (Properties · Pages · Layers) and a collapsed icon
//! column whose panels open as flyouts beside the dock.

use egui::{Color32, Sense, Stroke, vec2};

use crate::theme::{Tokens, semibold};
use crate::{DesignApp, icons, panels};

/// (id, label, icon) of the tabs in the expanded group.
pub const DOCK_TABS: &[(&str, &str, &str)] =
    &[("properties", "Properties", "panel-properties"), ("pages", "Pages", "panel-pages"), ("layers", "Layers", "panel-layers")];

/// Panels in the collapsed icon column.
pub const ICON_PANELS: &[(&str, &str, &str)] = &[
    ("stroke", "Stroke", "panel-stroke"),
    ("swatches", "Swatches", "panel-swatches"),
    ("color", "Color", "panel-color"),
    ("gradient", "Gradient", "tool-gradient"),
    ("effects", "Effects", "panel-effects"),
    ("paragraphStyles", "Paragraph Styles", "panel-pstyles"),
    ("characterStyles", "Character Styles", "panel-cstyles"),
    ("character", "Character", "panel-character"),
    ("glyphs", "Glyphs", "panel-character"),
    ("paragraph", "Paragraph", "panel-paragraph"),
    ("textWrap", "Text Wrap", "panel-wrap"),
    ("table", "Table", "panel-table"),
    ("align", "Align", "panel-align"),
    ("pathfinder", "Pathfinder", "tool-scissors"),
    ("links", "Links", "panel-links"),
    ("info", "Info", "panel-info"),
    ("preflight", "Preflight", "panel-preflight"),
    ("attributes", "Attributes", "panel-attributes"),
    ("conditions", "Conditional Text", "panel-conditions"),
    ("notes", "Notes", "panel-notes"),
    ("library", "Library", "panel-library"),
    ("hyperlinks", "Hyperlinks", "panel-hyperlinks"),
    ("bookmarks", "Bookmarks", "panel-bookmarks"),
    ("articles", "Articles", "panel-articles"),
    ("tags", "Tags", "panel-tags"),
    ("book", "Book", "panel-library"),
    ("states", "Object States", "panel-articles"),
    ("buttons", "Buttons and Forms", "panel-articles"),
    ("liquid", "Liquid Layout", "panel-pages"),
    ("media", "Media", "panel-articles"),
    ("transitions", "Page Transitions", "panel-pages"),
    ("trackChanges", "Track Changes", "panel-articles"),
    ("scripts", "Scripts", "panel-library"),
    ("dataMerge", "Data Merge", "panel-datamerge"),
];

pub fn show(app: &mut DesignApp, ui: &mut egui::Ui) {
    let t = Tokens::get(ui.ctx());
    // Expanded panel stack (outermost, 253 pt) — declared first so it sits at the far right.
    if app.ui.dock_expanded {
        egui::Panel::right("dock").default_size(253.0).size_range(220.0..=420.0).resizable(true).frame(egui::Frame::NONE.fill(t.panel)).show(
            ui,
            |ui| {
                dock_header(ui, &t, "»");
                // Tab strip (27 pt).
                let (strip, _) = ui.allocate_exact_size(vec2(ui.available_width(), 27.0), Sense::hover());
                ui.painter().rect_filled(strip, 0.0, t.tab_strip);
                let mut x = strip.min.x;
                for (id, label, _) in DOCK_TABS {
                    let active = app.ui.dock_tab == *id;
                    let g = crate::rtl::plain(
                        ui.ctx(),
                        crate::i18n::tr(&app.ui.language, label),
                        semibold(11.5),
                        if active { Color32::from_rgb(0xf3, 0xf3, 0xf3) } else { t.text_dim },
                    );
                    let w = g.size().x + 26.0;
                    let r = egui::Rect::from_min_size(egui::pos2(x, strip.min.y), vec2(w, 27.0));
                    if active {
                        ui.painter().rect_filled(r, 0.0, t.panel);
                    }
                    ui.painter().galley(egui::pos2(r.min.x + 13.0, r.center().y - g.size().y / 2.0), g, Color32::WHITE);
                    ui.painter().line_segment([r.right_top(), r.right_bottom()], Stroke::new(1.0, t.border));
                    if ui.interact(r, ui.id().with(("docktab", *id)), Sense::click()).clicked() {
                        app.ui.dock_tab = id.to_string();
                    }
                    x += w;
                }
                let menu_r = egui::Rect::from_min_size(egui::pos2(strip.max.x - 24.0, strip.min.y + 4.0), vec2(18.0, 18.0));
                icons::paint(ui.painter(), menu_r, "menu", t.icon);
                egui::Frame::NONE.inner_margin(egui::Margin { left: 10, right: 10, top: 4, bottom: 6 }).show(ui, |ui| {
                    egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| match app.ui.dock_tab.as_str() {
                        "pages" => panels::pages::show(app, ui),
                        "layers" => panels::layers::show(app, ui),
                        _ => panels::properties::show(app, ui),
                    });
                });
            },
        );
    }
    // Collapsed icon strip (37 pt) to the left of the panel stack.
    egui::Panel::right("icon_column").exact_size(37.0).resizable(false).frame(egui::Frame::NONE.fill(t.panel)).show(ui, |ui| {
        dock_header(ui, &t, "«");
        ui.add_space(4.0);
        ui.vertical_centered(|ui| {
            for (id, label, icon) in ICON_PANELS {
                let open = app.ui.open_panel.as_deref() == Some(*id);
                let floats = app.ui.floating.iter().any(|(p, _)| p == id);
                if icons::button(ui, icon, 28.0, open || floats, crate::i18n::tr(&app.ui.language, label)).clicked() {
                    if floats {
                        ui.ctx().move_to_top(egui::LayerId::new(egui::Order::Middle, egui::Id::new(("floating_panel", *id))));
                    } else {
                        app.ui.open_panel = if open { None } else { Some(id.to_string()) };
                    }
                }
                ui.add_space(1.0);
            }
        });
    });
}

/// 11 pt dock header strip with a collapse chevron.
fn dock_header(ui: &mut egui::Ui, t: &Tokens, chevron: &str) {
    let (r, _) = ui.allocate_exact_size(vec2(ui.available_width(), 11.0), Sense::hover());
    ui.painter().rect_filled(r, 0.0, t.tab_strip);
    ui.painter().text(egui::pos2(r.max.x - 8.0, r.center().y), egui::Align2::RIGHT_CENTER, chevron, egui::FontId::proportional(9.0), t.text_dim);
    ui.painter().line_segment([r.left_bottom(), r.right_bottom()], Stroke::new(1.0, t.border));
}

/// A panel opened from the icon column, shown as a floating flyout next to the dock.
pub fn flyout(app: &mut DesignApp, ctx: &egui::Context) {
    let Some(id) = app.ui.open_panel.clone() else { return };
    let t = Tokens::get(ctx);
    let label = crate::i18n::tr(&app.ui.language, ICON_PANELS.iter().find(|p| p.0 == id).map(|p| p.1).unwrap_or("Panel")).to_owned();
    let screen = ctx.content_rect();
    let dock_w = if app.ui.dock_expanded { ctx.memory(|m| m.area_rect(egui::Id::new("dock")).map(|r| r.width())).unwrap_or(280.0) } else { 0.0 };
    let pos = egui::pos2(screen.max.x - 37.0 - dock_w - 262.0, 120.0);
    let mut open = true;
    egui::Area::new(egui::Id::new("panel_flyout")).order(egui::Order::Foreground).fixed_pos(pos).show(ctx, |ui| {
        egui::Frame::popup(ui.style()).fill(t.panel).inner_margin(egui::Margin::same(0)).show(ui, |ui| {
            ui.set_width(256.0);
            // A previous shorter flyout must not constrain the next panel's scroll viewport.
            ui.set_max_height((screen.max.y - pos.y - 12.0).max(80.0));
            let (strip, _) = ui.allocate_exact_size(vec2(256.0, 26.0), Sense::hover());
            ui.painter().rect_filled(strip, 0.0, t.panel_darker);
            crate::rtl::paint(ui.painter(), strip.min + vec2(10.0, 13.0), egui::Align2::LEFT_CENTER, &label, semibold(12.0), t.text_strong);
            let close = egui::Rect::from_min_size(egui::pos2(strip.max.x - 22.0, strip.min.y + 4.0), vec2(18.0, 18.0));
            if ui.interact(close, ui.id().with("flyclose"), Sense::click()).clicked() {
                open = false;
            }
            ui.painter().text(close.center(), egui::Align2::CENTER_CENTER, "×", egui::FontId::proportional(15.0), t.text_dim);
            // Float: the button beside the close box, or drag the header away from the dock.
            let float_r = egui::Rect::from_min_size(egui::pos2(strip.max.x - 44.0, strip.min.y + 4.0), vec2(18.0, 18.0));
            let fr = ui.interact(float_r, ui.id().with("flyfloat"), Sense::click()).on_hover_ui(|ui| {
                crate::rtl::label(ui, crate::i18n::tr(&app.ui.language, "Float panel"));
            });
            float_glyph(ui.painter(), float_r, t.text_dim);
            let head = egui::Rect::from_min_max(strip.min, egui::pos2(float_r.min.x - 2.0, strip.max.y));
            let hr = ui.interact(head, ui.id().with("flyhead"), Sense::drag());
            let torn = hr.drag_stopped() && hr.total_drag_delta().is_some_and(|d| d.length() > 24.0);
            if fr.clicked() || torn {
                let at = if torn { ui.ctx().pointer_latest_pos().unwrap_or(pos) - vec2(20.0, 10.0) } else { pos - vec2(30.0, -30.0) };
                float_panel(app, &id, at);
                return;
            }
            egui::Frame::NONE.inner_margin(egui::Margin::same(10)).show(ui, |ui| {
                egui::ScrollArea::vertical().max_height(screen.height() - 220.0).show(ui, |ui| panel_body(app, ui, &id));
            });
        });
    });
    if !open {
        app.ui.open_panel = None;
    }
}

/// The contents of an icon-column panel.
pub fn panel_body(app: &mut DesignApp, ui: &mut egui::Ui, id: &str) {
    match id {
        "swatches" => panels::swatches::show(app, ui),
        "paragraphStyles" => panels::styles::paragraph(app, ui),
        "characterStyles" => panels::styles::character(app, ui),
        "stroke" => panels::properties::stroke_panel(app, ui),
        "character" => panels::properties::character_panel(app, ui),
        "glyphs" => panels::glyphs::show(app, ui),
        "paragraph" => panels::properties::paragraph_panel(app, ui),
        "textWrap" => panels::properties::wrap_panel(app, ui),
        "align" => panels::properties::align_panel(app, ui),
        "pathfinder" => panels::properties::pathfinder_panel(app, ui),
        "color" => panels::swatches::color_panel(app, ui),
        "gradient" => panels::swatches::gradient_panel(app, ui),
        "effects" => panels::properties::effects_panel(app, ui),
        "links" => panels::properties::links_panel(app, ui),
        "preflight" => panels::properties::preflight_panel(app, ui),
        "attributes" => panels::properties::attributes_panel(app, ui),
        "conditions" => panels::conditions::show(app, ui),
        "notes" => panels::notes::show(app, ui),
        "library" => panels::library::show(app, ui),
        "hyperlinks" => panels::interactive::hyperlinks(app, ui),
        "bookmarks" => panels::interactive::bookmarks(app, ui),
        "articles" => panels::interactive::articles(app, ui),
        "tags" => panels::interactive::tags(app, ui),
        "book" => panels::library::book(app, ui),
        "states" => panels::interactive::states(app, ui),
        "buttons" => panels::interactive::buttons(app, ui),
        "liquid" => panels::interactive::liquid(app, ui),
        "media" => panels::interactive::media(app, ui),
        "transitions" => panels::interactive::transitions(app, ui),
        "trackChanges" => panels::interactive::track_changes(app, ui),
        "scripts" => panels::library::scripts(app, ui),
        "dataMerge" => panels::datamerge::show(app, ui),
        "table" => panels::table::show(app, ui),
        _ => panels::properties::info_panel(app, ui),
    }
}

/// Tear panel `id` off the dock into a floating window at `at`.
pub fn float_panel(app: &mut DesignApp, id: &str, at: egui::Pos2) {
    if app.ui.open_panel.as_deref() == Some(id) {
        app.ui.open_panel = None;
    }
    app.ui.floating.retain(|(p, _)| p != id);
    app.ui.floating.push((id.to_string(), [at.x, at.y]));
}

/// Put a floating panel back in the dock's icon column.
pub fn dock_panel(app: &mut DesignApp, id: &str) {
    app.ui.floating.retain(|(p, _)| p != id);
}

/// Floating panels: movable, resizable windows; "Dock" returns one to the icon column.
pub fn floating(app: &mut DesignApp, ctx: &egui::Context) {
    let t = Tokens::get(ctx);
    for (id, at) in app.ui.floating.clone() {
        let label = crate::i18n::tr(&app.ui.language, ICON_PANELS.iter().find(|p| p.0 == id).map(|p| p.1).unwrap_or("Panel")).to_owned();
        let mut open = true;
        let mut dock = false;
        let r = egui::Window::new(label.clone())
            .id(egui::Id::new(("floating_panel", &id)))
            .title_bar(false)
            .default_pos(egui::pos2(at[0], at[1]))
            .default_width(256.0)
            .resizable(true)
            .frame(egui::Frame::window(&ctx.global_style()).fill(t.panel).inner_margin(egui::Margin::same(0)))
            .show(ctx, |ui| {
                // Header strip (drag it to move the panel): name, Dock, close.
                let (strip, _) = ui.allocate_exact_size(vec2(ui.available_width().max(200.0), 24.0), Sense::hover());
                ui.painter().rect_filled(strip, 0.0, t.panel_darker);
                crate::rtl::paint(ui.painter(), strip.min + vec2(10.0, 12.0), egui::Align2::LEFT_CENTER, &label, semibold(12.0), t.text_strong);
                let close = egui::Rect::from_min_size(egui::pos2(strip.max.x - 22.0, strip.min.y + 3.0), vec2(18.0, 18.0));
                if ui
                    .interact(close, ui.id().with("fclose"), Sense::click())
                    .on_hover_ui(|ui| {
                        crate::rtl::label(ui, crate::i18n::tr(&app.ui.language, "Close"));
                    })
                    .clicked()
                {
                    open = false;
                }
                ui.painter().text(close.center(), egui::Align2::CENTER_CENTER, "×", egui::FontId::proportional(15.0), t.text_dim);
                let dock_r = egui::Rect::from_min_size(egui::pos2(strip.max.x - 44.0, strip.min.y + 3.0), vec2(18.0, 18.0));
                if ui
                    .interact(dock_r, ui.id().with("fdock"), Sense::click())
                    .on_hover_ui(|ui| {
                        crate::rtl::label(ui, crate::i18n::tr(&app.ui.language, "Dock panel"));
                    })
                    .clicked()
                {
                    dock = true;
                }
                dock_glyph(ui.painter(), dock_r, t.text_dim);
                egui::Frame::NONE.inner_margin(egui::Margin::same(10)).show(ui, |ui| {
                    egui::ScrollArea::vertical().max_height(ctx.content_rect().height() - 160.0).show(ui, |ui| panel_body(app, ui, &id));
                });
            });
        // Remember where it was left.
        if let Some(r) = r
            && let Some(f) = app.ui.floating.iter_mut().find(|(p, _)| *p == id)
        {
            f.1 = [r.response.rect.min.x, r.response.rect.min.y];
        }
        if !open || dock {
            dock_panel(app, &id);
        }
    }
}

/// Two overlapping boxes: "float this panel".
fn float_glyph(p: &egui::Painter, r: egui::Rect, c: Color32) {
    let s = Stroke::new(1.0, c);
    p.rect_stroke(egui::Rect::from_min_size(r.min + vec2(3.5, 6.5), vec2(9.0, 8.0)), 0.0, s, egui::StrokeKind::Middle);
    p.rect_stroke(egui::Rect::from_min_size(r.min + vec2(6.5, 3.5), vec2(9.0, 8.0)), 0.0, s, egui::StrokeKind::Middle);
}

/// A box with a bar on its right: "back into the dock".
fn dock_glyph(p: &egui::Painter, r: egui::Rect, c: Color32) {
    let b = egui::Rect::from_min_size(r.min + vec2(3.5, 4.5), vec2(11.0, 9.0));
    p.rect_stroke(b, 0.0, Stroke::new(1.0, c), egui::StrokeKind::Middle);
    p.rect_filled(egui::Rect::from_min_max(egui::pos2(b.max.x - 3.5, b.min.y), b.max), 0.0, c);
}
