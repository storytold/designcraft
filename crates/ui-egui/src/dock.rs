//! The panels: their ids, names, icons and contents. `panel_docking` places them in the docking layout; the
//! float-only panels (Type › Tabs) open there as floating windows that never dock.

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
    ("objectStyles", "Object Styles", "frame-dashed"),
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

/// (id, label, icon) of the panels that only float (they open from a menu): floating windows of the docking layout
/// that take no other panel and never dock.
pub const FLOATING_PANELS: &[(&str, &str, &str)] = &[("tabs", "Tabs", "tab-left"), (crate::story_editor::PANEL, "Story Editor", "story-editor")];

/// Every panel: the default dock's, the iconic column's and the float-only ones.
pub fn all_panels() -> impl Iterator<Item = &'static (&'static str, &'static str, &'static str)> {
    DOCK_TABS.iter().chain(ICON_PANELS).chain(FLOATING_PANELS)
}

/// The English name of panel `id`.
pub fn panel_label(id: &str) -> &'static str {
    all_panels().find(|p| p.0 == id).map_or("Panel", |p| p.1)
}

/// The title panel `id` shows in place of its name while it says something that changes, such as a count; `None`
/// shows the panel's translated name. The dock reads the titles each frame and builds its panel specs again only when
/// one of them changed.
pub fn panel_title(app: &DesignApp, id: &str) -> Option<String> {
    match id {
        crate::story_editor::PANEL => crate::story_editor::title(app),
        _ => None,
    }
}

/// The panels showing a changing title ([`panel_title`]), with that title.
pub fn panel_titles(app: &DesignApp) -> Vec<(String, String)> {
    all_panels().filter_map(|(id, _, _)| panel_title(app, id).map(|title| ((*id).to_owned(), title))).collect()
}

/// Panels that lay out their own scrolling parts (the Pages panel: parents, pages, footer; the Story Editor, whose text
/// scrolls inside it).
pub fn scrolls_itself(id: &str) -> bool {
    matches!(id, "pages" | crate::story_editor::PANEL)
}

/// The contents of an icon-column panel.
pub fn panel_body(app: &mut DesignApp, ui: &mut egui::Ui, id: &str) {
    match id {
        "properties" => panels::properties::show(app, ui),
        crate::story_editor::PANEL => crate::story_editor::body(app, ui),
        "pages" => panels::pages::show(app, ui),
        "layers" => panels::layers::show(app, ui),
        "swatches" => panels::swatches::show(app, ui),
        "paragraphStyles" => panels::styles::paragraph(app, ui),
        "characterStyles" => panels::styles::character(app, ui),
        "objectStyles" => panels::styles::object(app, ui),
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

/// Panels that only float (the Tabs panel): the docking layout opens them as floating windows of their own and keeps
/// them out of the dock.
pub(crate) fn float_only(id: &str) -> bool {
    FLOATING_PANELS.iter().any(|p| p.0 == id)
}

#[cfg(test)]
mod visual_regressions {
    use super::*;
    use crate::theme::Tokens;
    use egui::{Color32, vec2};

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
                    let mut harness = crate::test_window::builder().with_size(vec2(width, 640.0)).with_pixels_per_point(scale).wgpu().build_ui_state(
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
                    let tab = {
                        use egui_kittest::kittest::Queryable;
                        harness
                            .query_all_by_label("Properties")
                            .map(|node| node.rect())
                            .find(|rect| rect.height() <= 45.0)
                            .expect("the Properties tab")
                    };
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
                    let ctx = crate::test_window::context();
                    crate::theme::install_fonts(&ctx, "en");
                    let tokens = Tokens::for_brightness(brightness);
                    crate::theme::apply(&ctx, &tokens);
                    let mut app = DesignApp::new(designcraft_engine::Session::new(), Default::default());
                    // The interface brightness the window shows, which the framework's dock follows through the frame's
                    // root values (`DesignApp::raw_input_hook`), as the shipping window gives them each frame.
                    app.ui.brightness = brightness;
                    let frame = |app: &mut DesignApp| {
                        let mut input = scaled_input(vec2(width, 640.0), scale);
                        app.raw_input_hook(&ctx, &mut input);
                        let mut output = ctx.run_ui(input, |ui| crate::panel_docking::show(app, ui));
                        output.textures_delta.clear();
                        output
                    };
                    frame(&mut app);
                    let output = frame(&mut app);
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
