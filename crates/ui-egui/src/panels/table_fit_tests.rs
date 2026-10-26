//! Table panel: the Fit to Frame Width checkbox (#1025).

use egui::vec2;
use egui_kittest::Harness;
use egui_kittest::kittest::Queryable;
use serde_json::json;

use crate::DesignApp;

fn fit_to_frame(app: &mut DesignApp) -> bool {
    app.run("table.get", json!({})).unwrap()["options"]["fitToFrame"].as_bool().unwrap_or(false)
}

/// Clicking the checkbox turns the option on, clicking again turns it off.
#[test]
fn fit_to_frame_width_checkbox_toggles_the_table_option() {
    let mut app = DesignApp::new(designcraft_engine::Session::new(), crate::Services::default());
    app.run("file.new", json!({})).unwrap();
    let sid = app.run("frame.create", json!({"rect": [36, 36, 400, 400], "content": "text", "text": ""})).unwrap()["story"].as_u64().unwrap();
    app.run("text.select", json!({"story": sid, "anchor": 0, "focus": 0})).unwrap();
    app.run("table.insert", json!({"rows": 2, "cols": 2})).unwrap();
    let mut h = Harness::builder().with_size(vec2(360.0, 900.0)).build_ui_state(|ui, app: &mut DesignApp| super::show(app, ui), app);
    h.run_steps(3);
    assert!(!fit_to_frame(h.state_mut()));
    h.get_by_label("Fit to Frame Width").click();
    h.run_steps(3);
    assert!(fit_to_frame(h.state_mut()), "the checkbox turns the option on");
    h.get_by_label("Fit to Frame Width").click();
    h.run_steps(3);
    assert!(!fit_to_frame(h.state_mut()), "and off again");
}
