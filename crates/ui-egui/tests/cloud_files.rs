//! The `CloudBackend` / `ShareBackend` contract, driven through the real
//! interface against an in-process mock.
//!
//! `storytold/craft-libs#160` is open and unmerged, so `craft-ui`'s own
//! `cloud` feature and `craft-cloud` are not in this workspace yet. Until the
//! pins move, `designcraft_ui_egui::cloud` states the contract itself (see
//! that module's doc comment), and this is the mock a host would plug into
//! it: no concrete provider lives in the crate under test, and none lives
//! here either.

use designcraft_engine::Session;
use designcraft_ui_egui::cloud::{CloudBackend, ShareBackend};
use designcraft_ui_egui::{DesignApp, Services};
use egui_kittest::kittest::Queryable as _;
use serde_json::json;

type Harness = egui_kittest::Harness<'static, DesignApp>;

/// Frames to run before reading the interface back; the application bar lays
/// out again whenever its right-hand controls change width.
const SETTLE: usize = 6;

/// A cloud backend that answers from a fixed account, with no transport of
/// its own.
struct MockCloud {
    account: Option<&'static str>,
}

impl CloudBackend for MockCloud {
    fn account(&self) -> Option<String> {
        self.account.map(str::to_owned)
    }
}

/// A share backend that only names itself.
struct MockShare {
    label: &'static str,
}

impl ShareBackend for MockShare {
    fn label(&self) -> String {
        self.label.to_owned()
    }
}

fn harness() -> Harness {
    let builder = egui_kittest::Harness::builder().with_size(egui::vec2(1280.0, 900.0));
    let mut h = builder.build_ui_state(
        |ui, app: &mut DesignApp| {
            let ctx = ui.ctx().clone();
            app.logic(&ctx);
            app.ui(ui);
        },
        {
            let mut app = DesignApp::new(Session::new(), Services::default());
            app.run("file.new", json!({"pages": 1, "facingPages": false})).unwrap();
            app
        },
    );
    h.run_steps(SETTLE);
    h
}

#[test]
fn nothing_cloud_shaped_is_offered_without_a_backend() {
    let mut h = harness();
    assert!(!designcraft_ui_egui::cloud::available(h.state()));
    assert!(!designcraft_ui_egui::cloud::shares(h.state()));
    assert!(!designcraft_ui_egui::menus::menu_enabled(h.state(), "cloud.open"), "the File menu entry is disabled without a backend");
    assert_eq!(h.state_mut().run("cloud.open", json!({})).unwrap_err(), designcraft_ui_egui::cloud::UNAVAILABLE);
    assert!(h.query_by_label("Signed in to the cloud").is_none());
}

#[test]
fn a_connected_cloud_backend_is_available_and_reports_its_account() {
    let mut h = harness();
    h.state_mut().cloud_backend = Some(Box::new(MockCloud { account: Some("Ada Lovelace") }));
    h.run_steps(SETTLE);
    assert!(designcraft_ui_egui::cloud::available(h.state()));
    assert!(designcraft_ui_egui::menus::menu_enabled(h.state(), "cloud.open"), "the File menu entry is enabled once a backend is connected");
    let answer = h.state_mut().run("cloud.open", json!({})).unwrap();
    assert_eq!(answer["account"], json!("Ada Lovelace"));
    assert!(h.state().ui.status.contains("Ada Lovelace"), "the status line names the signed-in account");
}

#[test]
fn a_connected_backend_with_no_account_yet_reports_signed_out() {
    let mut h = harness();
    h.state_mut().cloud_backend = Some(Box::new(MockCloud { account: None }));
    h.run_steps(SETTLE);
    assert!(designcraft_ui_egui::cloud::available(h.state()));
    let answer = h.state_mut().run("cloud.open", json!({})).unwrap();
    assert_eq!(answer["account"], serde_json::Value::Null);
}

#[test]
fn a_share_backend_is_reported_separately_from_a_cloud_backend() {
    let mut h = harness();
    assert!(!designcraft_ui_egui::cloud::shares(h.state()));
    h.state_mut().share_backend = Some(Box::new(MockShare { label: "Example Cloud" }));
    h.run_steps(SETTLE);
    assert!(designcraft_ui_egui::cloud::shares(h.state()), "a share backend needs no cloud backend beside it");
    assert!(!designcraft_ui_egui::cloud::available(h.state()), "and the reverse: a cloud backend needs no share backend");
}
