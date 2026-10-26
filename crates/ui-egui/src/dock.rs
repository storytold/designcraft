//! The panels: their ids, names, icons and contents, and the windows of the float-only panels.
//! `panel_docking` places the others in the docking layout.

use egui::{Sense, vec2};

use crate::theme::{Tokens, semibold};
use crate::{DesignApp, panels};

/// (id, label, icon) of the Properties · Pages · Layers group of the default dock.
pub const DOCK_TABS: &[(&str, &str, &str)] =
    &[("properties", "Properties", "panel-properties"), ("pages", "Pages", "panel-pages"), ("layers", "Layers", "panel-layers")];

/// (id, label, icon) of the other dock panels, which start in the iconic column.
pub const ICON_PANELS: &[(&str, &str, &str)] = &[
    ("stroke", "Stroke", "panel-stroke"),
    ("swatches", "Swatches", "panel-swatches"),
    ("color", "Color", "panel-color"),
    ("gradient", "Gradient", "tool-gradient"),
    ("effects", "Effects", "panel-effects"),
    ("paragraphStyles", "Paragraph Styles", "panel-pstyles"),
    ("characterStyles", "Character Styles", "panel-cstyles"),
    ("character", "Character", "panel-character"),
    ("typography", "Typography", "panel-typography"),
    ("glyphs", "Glyphs", "panel-character"),
    ("paragraph", "Paragraph", "panel-paragraph"),
    ("story", "Story", "panel-text-frame"),
    ("textWrap", "Text Wrap", "panel-wrap"),
    ("table", "Table", "panel-table"),
    ("align", "Align", "panel-align"),
    ("pathfinder", "Pathfinder", "tool-scissors"),
    ("flexLayout", "Flex Layout", "panel-flex"),
    ("links", "Links", "panel-links"),
    ("info", "Info", "panel-info"),
    ("preflight", "Preflight", "panel-preflight"),
    ("separations", "Separations Preview", "panel-swatches"),
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

/// Panels that only float, outside the docking layout (they open from a menu).
pub const FLOATING_PANELS: &[(&str, &str)] = &[("tabs", "Tabs")];

/// The English name of panel `id`.
pub fn panel_label(id: &str) -> &'static str {
    DOCK_TABS.iter().chain(ICON_PANELS).map(|p| (p.0, p.1)).chain(FLOATING_PANELS.iter().copied()).find(|p| p.0 == id).map_or("Panel", |p| p.1)
}

/// A panel's contents at most `max_h` tall, scrolling past that. The Pages panel scrolls its
/// parts itself and keeps its footer at the bottom, so it gets the height instead.
fn scrolled_body(app: &mut DesignApp, ui: &mut egui::Ui, id: &str, max_h: f32) {
    if scrolls_itself(id) {
        ui.allocate_ui(vec2(ui.available_width(), max_h.clamp(160.0, 560.0)), |ui| panel_body(app, ui, id));
    } else {
        egui::ScrollArea::vertical().max_height(max_h).show(ui, |ui| panel_body(app, ui, id));
    }
}

/// Panels that lay out their own scrolling parts (the Pages panel: parents, pages, footer).
pub fn scrolls_itself(id: &str) -> bool {
    id == "pages"
}

/// The contents of an icon-column panel.
pub fn panel_body(app: &mut DesignApp, ui: &mut egui::Ui, id: &str) {
    match id {
        "properties" => panels::properties::show(app, ui),
        "pages" => panels::pages::show(app, ui),
        "layers" => panels::layers::show(app, ui),
        "swatches" => panels::swatches::show(app, ui),
        "paragraphStyles" => panels::styles::paragraph(app, ui),
        "characterStyles" => panels::styles::character(app, ui),
        "stroke" => panels::properties::stroke_panel(app, ui),
        "character" => panels::properties::character_panel(app, ui),
        "typography" => panels::typography::show(app, ui),
        "glyphs" => panels::glyphs::show(app, ui),
        "paragraph" => panels::properties::paragraph_panel(app, ui),
        "story" => panels::story::show(app, ui),
        "textWrap" => panels::properties::wrap_panel(app, ui),
        "align" => panels::properties::align_panel(app, ui),
        "pathfinder" => panels::properties::pathfinder_panel(app, ui),
        "flexLayout" => panels::properties::flex_panel(app, ui),
        "color" => panels::swatches::color_panel(app, ui),
        "gradient" => panels::swatches::gradient_panel(app, ui),
        "effects" => panels::properties::effects_panel(app, ui),
        "links" => panels::properties::links_panel(app, ui),
        "preflight" => panels::properties::preflight_panel(app, ui),
        "attributes" => panels::properties::attributes_panel(app, ui),
        "conditions" => panels::conditions::show(app, ui),
        "notes" => panels::notes::show(app, ui),
        "separations" => panels::separations::show(app, ui),
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
        "tabs" => panels::tabs::show(app, ui),
        _ => panels::properties::info_panel(app, ui),
    }
}

/// Panels that only float, outside the dock and the docking layout (the Tabs panel).
pub(crate) fn float_only(id: &str) -> bool {
    FLOATING_PANELS.iter().any(|p| p.0 == id)
}

/// The float-only panels' windows (the Tabs panel). The docking layout draws the other
/// floating panels.
pub fn float_only_windows(app: &mut DesignApp, ctx: &egui::Context) {
    let t = Tokens::get(ctx);
    for (id, at) in app.ui.floating.clone() {
        if !float_only(&id) {
            continue;
        }
        let label = crate::i18n::tr(&app.ui.language, panel_label(&id)).to_owned();
        let mut open = true;
        let mut w = egui::Window::new(label.clone())
            .id(egui::Id::unique(("floating_panel", &id)))
            .title_bar(false)
            .default_pos(egui::pos2(at[0], at[1]))
            .default_width(if id == "tabs" { 560.0 } else { 256.0 })
            .resizable(true)
            .frame(egui::Frame::window(&ctx.global_style()).fill(t.panel).inner_margin(egui::Margin::same(0)));
        // The Tabs panel moves over the text frame it edits, at the column's width.
        if id == "tabs"
            && let Some((pos, width)) = app.ui.tabs_panel.place.take()
        {
            w = w.current_pos(pos).min_width(width).max_width(width);
        }
        let r = w.show(ctx, |ui| {
            // Header strip (drag it to move the panel): name and close.
            let (strip, _) = ui.allocate_exact_size(vec2(ui.available_width().max(200.0), 24.0), Sense::hover());
            ui.painter().rect_filled(strip, 0.0, t.panel_darker);
            crate::rtl::paint(ui.painter(), strip.min + vec2(10.0, 12.0), egui::Align2::LEFT_CENTER, &label, semibold(12.0), t.text_strong);
            let close = egui::Rect::from_min_size(egui::pos2(strip.max.x - 22.0, strip.min.y + 3.0), vec2(18.0, 18.0));
            if ui
                .interact(close, ui.scope_id().with("fclose"), Sense::click())
                .on_hover_ui(|ui| {
                    crate::rtl::label(ui, crate::i18n::tr(&app.ui.language, "Close"));
                })
                .clicked()
            {
                open = false;
            }
            ui.painter().text(close.center(), egui::Align2::CENTER_CENTER, "×", egui::FontId::proportional(15.0), t.text_dim);
            egui::Frame::NONE.inner_margin(egui::Margin::same(10)).show(ui, |ui| {
                scrolled_body(app, ui, &id, ctx.content_rect().height() - 160.0);
            });
        });
        // Remember where it was left.
        if let Some(r) = r
            && let Some(f) = app.ui.floating.iter_mut().find(|(p, _)| *p == id)
        {
            f.1 = [r.response.rect.min.x, r.response.rect.min.y];
        }
        if !open {
            app.ui.floating.retain(|(p, _)| *p != id);
        }
    }
}

#[cfg(test)]
mod visual_regressions {
    use super::*;
    use egui::Color32;

    fn scaled_input(size: egui::Vec2, scale: f32) -> egui::RawInput {
        let mut input = egui::RawInput { screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, size)), ..Default::default() };
        input.viewports.entry(egui::ViewportId::ROOT).or_default().native_pixels_per_point = Some(scale);
        input
    }

    /// Opt-in evidence for native screenshot review; ordinary test runs stay headless and fast.
    #[test]
    fn capture_dock_visual_fixtures() {
        let Some(directory) = std::env::var_os("CRAFT_UI_VISUAL_FIXTURES").map(std::path::PathBuf::from) else { return };
        std::fs::create_dir_all(&directory).unwrap();
        for brightness in crate::theme::Brightness::ALL {
            for width in [800.0, 1280.0] {
                for scale in [1.0, 1.5, 2.0] {
                    let app = DesignApp::new(designcraft_engine::Session::new(), Default::default());
                    let mut ready = false;
                    let mut harness =
                        egui_kittest::Harness::builder().with_size(vec2(width, 640.0)).with_pixels_per_point(scale).wgpu().build_ui_state(
                            move |ui, app: &mut DesignApp| {
                                if !ready {
                                    crate::theme::install_fonts(ui.ctx(), "en");
                                    crate::theme::apply(ui.ctx(), &Tokens::for_brightness(brightness));
                                    ready = true;
                                    return;
                                }
                                crate::panel_docking::show(app, ui);
                            },
                            app,
                        );
                    harness.input_mut().max_texture_side = Some(8192);
                    harness.run_steps(4);
                    let stem = format!("designcraft-dock-{}-{width}-{scale}x", brightness.id());
                    harness.render().unwrap().save(directory.join(format!("{stem}.png"))).unwrap();
                    let tab = crate::panel_docking::tab_id("properties");
                    let tab = harness.ctx.read_response(tab).unwrap().rect;
                    let region = serde_json::json!({
                        "screen": [width, 640.0], "scale": scale,
                        "regions": {"dock-tabs": [tab.left(), tab.top(), (width - tab.left()), tab.height()]}
                    });
                    std::fs::write(directory.join(format!("{stem}.json")), serde_json::to_vec_pretty(&region).unwrap()).unwrap();
                }
            }
        }
    }

    fn luminance(c: Color32) -> f32 {
        let linear = |v: u8| {
            let v = f32::from(v) / 255.0;
            if v <= 0.04045 { v / 12.92 } else { ((v + 0.055) / 1.055).powf(2.4) }
        };
        0.2126 * linear(c.r()) + 0.7152 * linear(c.g()) + 0.0722 * linear(c.b())
    }

    #[test]
    fn selected_dock_tab_has_readable_rendered_contrast_in_every_theme() {
        for brightness in crate::theme::Brightness::ALL {
            for width in [800.0, 1280.0] {
                for scale in [1.0, 1.5, 2.0] {
                    let ctx = egui::Context::default();
                    crate::theme::install_fonts(&ctx, "en");
                    let tokens = Tokens::for_brightness(brightness);
                    crate::theme::apply(&ctx, &tokens);
                    let mut app = DesignApp::new(designcraft_engine::Session::new(), Default::default());
                    let mut output = ctx.run_ui(scaled_input(vec2(width, 640.0), scale), |ui| crate::panel_docking::show(&mut app, ui));
                    output.textures_delta.clear();
                    let mut output = ctx.run_ui(scaled_input(vec2(width, 640.0), scale), |ui| crate::panel_docking::show(&mut app, ui));
                    output.textures_delta.clear();
                    assert_eq!(ctx.content_rect().size(), vec2(width, 640.0));
                    assert_eq!(ctx.pixels_per_point(), scale);
                    let text = output
                        .shapes
                        .iter()
                        .find_map(|shape| match &shape.shape {
                            egui::Shape::Text(text) if text.galley.job.text == "Properties" => Some(text),
                            _ => None,
                        })
                        .expect("the active Properties tab is painted");
                    let color = text.galley.job.sections.first().expect("tab text has a format").format.color;
                    let a = luminance(color);
                    let b = luminance(tokens.panel);
                    let contrast = (a.max(b) + 0.05) / (a.min(b) + 0.05);
                    assert!(contrast >= 4.5, "{brightness:?} at {width}: selected tab contrast is {contrast:.2}");
                }
            }
        }
    }
}
