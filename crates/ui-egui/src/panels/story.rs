//! Window › Type & Tables › Story: the story-wide options.

use serde_json::{Value, json};

use crate::DesignApp;
use crate::widgets::{self, NumField};

/// The Story panel: Optical Margin Alignment for the stories of the text selection or the
/// selected text frames, and the type size its hang amounts are set for. Without text or a text
/// frame selected, the controls are disabled.
pub fn show(app: &mut DesignApp, ui: &mut egui::Ui) {
    let options = super::text_attrs(app).map(|a| a["storyOptions"].clone());
    let enabled = options.is_some();
    let options = options.unwrap_or(Value::Null);
    let lang = app.ui.language.clone();
    let tr = |s: &str| crate::i18n::tr(&lang, s).to_string();
    ui.add_enabled_ui(enabled, |ui| {
        ui.horizontal_wrapped(|ui| {
            // Stories that differ show unchecked; a click turns it on for all of them.
            let mut on = options["opticalMargin"].as_bool().unwrap_or(false);
            if widgets::checkbox(ui, &mut on, &tr("Optical Margin Alignment")).changed() {
                let _ = app.run("story.options", json!({"opticalMargin": on}));
            }
            let field = NumField::number("storyOpticalMarginSize", options["opticalMarginSize"].as_f64(), " pt", 2)
                .width(72.0)
                .range(designcraft_doc::OpticalMargin::MIN_SIZE, designcraft_doc::OpticalMargin::MAX_SIZE);
            if let Some(size) = crate::chrome::icon_field(ui, "font-size", &tr("Optical Margin Size"), field) {
                let _ = app.run("story.options", json!({"opticalMarginSize": size}));
            }
        });
    });
}

#[cfg(test)]
mod tests {
    use egui_kittest::{
        Harness,
        kittest::{NodeT, Queryable},
    };

    use super::*;

    fn harness(app: DesignApp) -> Harness<'static, DesignApp> {
        let mut h = Harness::builder().with_size(egui::vec2(260.0, 200.0)).build_ui_state(
            |ui, app: &mut DesignApp| {
                let id = egui::Id::unique("test_fonts");
                if ui.data(|d| d.get_temp::<bool>(id)).is_none() {
                    crate::theme::install_fonts(ui.ctx(), "");
                    ui.data_mut(|d| d.insert_temp(id, true));
                    return;
                }
                show(app, ui);
            },
            app,
        );
        h.run_steps(3);
        h
    }

    fn optical_margin(h: &Harness<'static, DesignApp>, story: u64) -> designcraft_doc::OpticalMargin {
        h.state().session.active().unwrap().doc.story(designcraft_doc::StoryId(story)).unwrap().optical_margin
    }

    fn size_field_text(h: &Harness<'static, DesignApp>) -> Option<String> {
        h.get_all_by_role(egui::accesskit::Role::TextInput).next().unwrap().value()
    }

    #[test]
    fn story_panel_sets_and_shows_the_storys_optical_margin() {
        let mut app = DesignApp::new(designcraft_engine::Session::new(), crate::Services::default());
        app.run("file.new", json!({})).unwrap();
        let story =
            app.run("frame.create", json!({"rect": [72, 72, 300, 200], "content": "text", "text": "“Hung”", "caret": true})).unwrap()["story"]
                .as_u64()
                .unwrap();
        app.run("edit.selectAll", json!({})).unwrap();
        let mut h = harness(app);
        assert_eq!(size_field_text(&h).as_deref(), Some("12 pt"));
        h.get_by_label("Optical Margin Alignment").click();
        h.run_steps(3);
        assert!(optical_margin(&h, story).on);
        assert_eq!(h.get_by_label("Optical Margin Alignment").accesskit_node().toggled(), Some(egui::accesskit::Toggled::True));
        h.get_all_by_role(egui::accesskit::Role::TextInput).next().unwrap().click();
        h.run_steps(2);
        h.event(egui::Event::Key { key: egui::Key::A, physical_key: None, pressed: true, repeat: false, modifiers: egui::Modifiers::COMMAND });
        h.event(egui::Event::Text("18".into()));
        h.key_press(egui::Key::Enter);
        h.run_steps(3);
        assert_eq!(optical_margin(&h, story), designcraft_doc::OpticalMargin { on: true, size: 18.0 });
        // A value set elsewhere shows in the panel.
        h.state_mut().run("story.options", json!({"opticalMarginSize": 30})).unwrap();
        h.run_steps(3);
        assert_eq!(size_field_text(&h).as_deref(), Some("30 pt"));
    }

    #[test]
    fn story_panel_is_disabled_without_text() {
        let mut app = DesignApp::new(designcraft_engine::Session::new(), crate::Services::default());
        app.run("file.new", json!({})).unwrap();
        let mut h = harness(app);
        assert!(h.get_by_label("Optical Margin Alignment").accesskit_node().is_disabled());
        h.get_by_label("Optical Margin Alignment").click();
        h.run_steps(3);
        assert_eq!(h.get_by_label("Optical Margin Alignment").accesskit_node().toggled(), Some(egui::accesskit::Toggled::False));
    }
}
