//! Successors, on the installed framework, of the native tests of `panels/datamerge.rs`: each keeps its old steps and checks and runs them in a
//! window whose context has the craft UI runtime installed and the app's framework flag on, so the panel draws its framework
//! body. The old tests stay until each one is deleted with its native body.

use egui_kittest::{
    Harness,
    kittest::{NodeT, Queryable},
};

use super::*;

/// Records and Arrangement each expose exactly one named combo, and the accessibility check passes.
/// Successor of `arrangement_combo_is_accessible_and_distinct_from_the_records_combo` on the installed framework: the same steps and checks, in a window whose context has the craft UI runtime.
#[test]
fn arrangement_combo_is_accessible_and_distinct_from_the_records_combo() {
    let mut app = DesignApp::new(designcraft_engine::Session::new(), crate::Services::default());
    app.run("file.new", json!({})).unwrap();
    app.run("data.source.select", json!({"bytes": designcraft_engine::cmd::base64_encode(b"Name\nAda\nGrace\n"), "name": "people.csv"})).unwrap();
    app.run("data.options", json!({"perPage": "multiple"})).unwrap();
    let mut h = crate::test_window::builder::<DesignApp>().with_size(vec2(420.0, 700.0)).build_ui_state(
        |ui, app: &mut DesignApp| {
            let key = egui::Id::unique("datamerge_test_fonts");
            if ui.data(|d| d.get_temp::<bool>(key)).is_none() {
                crate::theme::install_fonts(ui.ctx(), "");
                ui.data_mut(|d| d.insert_temp(key, true));
                return;
            }
            show(app, ui);
        },
        app,
    );
    h.run_steps(3);
    let combos = |h: &Harness<'_, DesignApp>, label: &str| {
        h.query_all_by_label(label).filter(|n| n.accesskit_node().role() == egui::accesskit::Role::ComboBox).count()
    };
    assert_eq!(combos(&h, "Merge options"), 1, "records selection keeps its name");
    assert_eq!(combos(&h, "Arrangement"), 1, "the arrangement control has its own name");
    h.check_accessibility();
}
