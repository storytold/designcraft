//! Properties ▸ Text Style ▸ New Style (⊞) captures the formatting of the selected text.

use designcraft_ui_egui::{DesignApp, Services};
use egui_kittest::{Harness, kittest::Queryable};
use serde_json::json;

#[test]
fn properties_new_style_button_captures_the_selection() {
    let mut app = DesignApp::new(designcraft_engine::Session::new(), Services::default());
    app.run("file.new", json!({})).unwrap();
    let frame = app.run("frame.create", json!({"rect": [36, 36, 300, 200], "content": "text", "text": "Test text"})).unwrap();
    app.run("text.select", json!({"story": frame["story"], "anchor": 0, "focus": 9})).unwrap();
    app.run("type.char", json!({"size": 30})).unwrap();

    let mut h = Harness::builder().with_size(egui::vec2(320.0, 900.0)).build_ui_state(
        |ui, app: &mut DesignApp| {
            let id = egui::Id::new("test_fonts_installed");
            if ui.data(|d| d.get_temp::<bool>(id)).is_none() {
                designcraft_ui_egui::theme::install_fonts(ui.ctx(), "");
                ui.data_mut(|d| d.insert_temp(id, true));
                return;
            }
            designcraft_ui_egui::panels::properties::show(app, ui);
        },
        app,
    );
    h.run_steps(3);
    h.get_by_label("Create new style").click();
    h.run_steps(3);

    let app = h.state_mut();
    let size = app
        .session
        .doc()
        .unwrap()
        .doc
        .styles
        .para("Paragraph Style 1")
        .map(|s| s.chars.size);
    assert_eq!(size, Some(Some(30.0)));
}
