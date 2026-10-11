//! Tests the real app resolver, saved state, shared menu and native viewport output.
use super::*;
use theme::{Brightness, Tokens};

fn frame(app: &mut DesignApp, ctx: &egui::Context, system: Option<egui::Theme>) -> egui::FullOutput {
    let mut output = ctx.run_ui(egui::RawInput { system_theme: system, ..Default::default() }, |ui| app.logic(ui.ctx()));
    // These tests inspect state, shapes and native commands without a renderer.
    output.textures_delta.clear();
    output
}

#[test]
fn system_transitions_keep_saved_choice_and_native_inheritance() {
    let ctx = egui::Context::default();
    let mut app = DesignApp::new(Session::new(), Services::default());
    app.run("window.brightness", json!({"brightness": "system"})).unwrap();
    for (index, expected) in [
        (Some(egui::Theme::Light), Brightness::Light),
        (Some(egui::Theme::Dark), Brightness::MediumDark),
        (None, Brightness::MediumDark),
        (Some(egui::Theme::Light), Brightness::Light),
    ]
    .into_iter()
    .enumerate()
    {
        let (system, expected) = expected;
        let output = frame(&mut app, &ctx, system);
        assert_eq!(app.ui.brightness, Brightness::System);
        assert_eq!(app.resolved_brightness, Some(expected));
        let tokens = Tokens::for_brightness(expected);
        assert_eq!(Tokens::get(&ctx).panel, tokens.panel);
        assert_eq!(Tokens::get(&ctx).pasteboard, tokens.pasteboard);
        assert_eq!(ctx.options(|o| o.theme_preference), egui::ThemePreference::System);
        for branch in [egui::Theme::Light, egui::Theme::Dark] {
            let style = ctx.style_of(branch);
            assert_eq!(style.text_styles.get(&egui::TextStyle::Body).unwrap().size, 11.0);
            assert_eq!(style.visuals.panel_fill, tokens.panel);
            assert_eq!(style.visuals.window_fill, tokens.panel);
            assert_eq!(style.visuals.override_text_color, Some(tokens.text));
        }
        if index == 0 {
            assert!(
                output.viewport_output.values().any(|viewport| viewport
                    .commands
                    .iter()
                    .any(|command| matches!(command, egui::ViewportCommand::SetTheme(egui::SystemTheme::SystemDefault)))),
                "the first real app pass emits SystemDefault"
            );
        }
        for viewport in output.viewport_output.values() {
            for command in &viewport.commands {
                if let egui::ViewportCommand::SetTheme(theme) = command {
                    assert_eq!(*theme, egui::SystemTheme::SystemDefault, "never pin native observation");
                }
            }
        }
        let saved = serde_json::to_vec(&app.ui).unwrap();
        let restored: UiState = serde_json::from_slice(&saved).unwrap();
        assert_eq!(restored.brightness, Brightness::System);
    }
    // Fresh/restarted contexts resolve the same saved System correctly at startup.
    for system in [egui::Theme::Light, egui::Theme::Dark] {
        let mut restarted = DesignApp::new(Session::new(), Services::default());
        restarted.ui = serde_json::from_slice(&serde_json::to_vec(&app.ui).unwrap()).unwrap();
        frame(&mut restarted, &egui::Context::default(), Some(system));
        assert_eq!(restarted.resolved_brightness, Some(Brightness::System.resolved(Some(system))));
    }
}

#[test]
fn all_five_legacy_palettes_and_default_ignore_os_changes() {
    assert_eq!(UiState::default().brightness, Brightness::MediumDark);
    for b in Brightness::ALL.into_iter().filter(|b| *b != Brightness::System) {
        let mut app = DesignApp::new(Session::new(), Services::default());
        app.ui = serde_json::from_value(json!({"brightness": serde_json::to_value(b).unwrap(), "rulers": false})).unwrap();
        assert!(!app.ui.rulers);
        let ctx = egui::Context::default();
        for system in [Some(egui::Theme::Light), Some(egui::Theme::Dark), None] {
            frame(&mut app, &ctx, system);
            assert_eq!(app.ui.brightness, b);
            assert_eq!(app.resolved_brightness, Some(b));
            assert_eq!(Tokens::get(&ctx).panel, Tokens::for_brightness(b).panel);
            assert_eq!(ctx.style_of(ctx.theme()).visuals.dark_mode, Tokens::for_brightness(b).dark);
        }
    }
}

#[test]
fn shared_native_and_in_window_menu_checks_the_saved_choice() {
    use menus::Item;
    let tree = menus::menu_tree();
    let window = &tree.iter().find(|(name, _)| *name == "Window").unwrap().1;
    let choices = window
        .iter()
        .find_map(|item| match item {
            Item::Sub(label, choices) if label == "Interface Color Theme" => Some(choices),
            _ => None,
        })
        .unwrap();
    assert_eq!(choices.len(), 6);
    let mut app = DesignApp::new(Session::new(), Services::default());
    for item in choices {
        let Item::Cmd { id, params, .. } = item else { panic!("theme is a command") };
        app.run(id, params.clone()).unwrap();
        assert_eq!(menus::checked(&app, id, params), Some(true));
        let selected = app.ui.brightness;
        frame(&mut app, &egui::Context::default(), Some(egui::Theme::Light));
        assert_eq!(app.ui.brightness, selected);
        for other in choices {
            if let Item::Cmd { id, params, .. } = other {
                assert_eq!(menus::checked(&app, id, params), Some(params["brightness"] == selected.id()));
            }
        }
    }
}

#[test]
fn host_updates_restyle_and_leave_document_and_view_settings_independent() {
    use std::sync::{Arc, Mutex};
    let system = Arc::new(Mutex::new(Some(egui::Theme::Light)));
    let reader = Arc::clone(&system);
    let mut app = DesignApp::new(Session::new(), Services { system_theme: Some(Box::new(move |_| *reader.lock().unwrap())), ..Default::default() });
    app.run("file.newSample", json!({})).unwrap();
    app.run("window.brightness", json!({"brightness": "system"})).unwrap();
    let document = app.run("file.serialize", json!({})).unwrap();
    let view_mode = app.ui.screen_mode;
    let ctx = egui::Context::default();
    for theme in [Some(egui::Theme::Light), Some(egui::Theme::Dark), None] {
        *system.lock().unwrap() = theme;
        frame(&mut app, &ctx, Some(egui::Theme::Light));
        assert_eq!(app.resolved_brightness, Some(Brightness::System.resolved(theme)));
        assert_eq!(app.run("file.serialize", json!({})).unwrap(), document);
        assert_eq!(app.ui.screen_mode, view_mode);
    }
}

#[test]
fn page_number_paint_tracks_system_palette_and_keeps_selected_badges_white() {
    let mut app = DesignApp::new(Session::new(), Services::default());
    app.run("file.newSample", json!({})).unwrap();
    app.run("window.brightness", json!({"brightness": "system"})).unwrap();
    let selected = app.session.page_label(0);
    let unselected = app.session.page_label(1);
    let ctx = egui::Context::default();
    frame(&mut app, &ctx, Some(egui::Theme::Light));
    for system in [egui::Theme::Light, egui::Theme::Dark] {
        let mut output = egui::FullOutput::default();
        for _ in 0..4 {
            output = ctx.run_ui(egui::RawInput { system_theme: Some(system), ..Default::default() }, |ui| {
                app.logic(ui.ctx());
                crate::panels::pages::show(&mut app, ui);
            });
            // Retain the paint shapes while intentionally discarding unused texture updates.
            output.textures_delta.clear();
        }
        let color = |label: &str| {
            output.shapes.iter().find_map(|shape| match &shape.shape {
                egui::Shape::Text(text) if text.galley.text() == label => text.galley.job.sections.first().map(|section| section.format.color),
                _ => None,
            })
        };
        assert_eq!(color(&unselected), Some(Tokens::for_brightness(Brightness::System.resolved(Some(system))).text));
        assert_eq!(color(&selected), Some(egui::Color32::WHITE));
    }
}
