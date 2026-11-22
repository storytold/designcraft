//! A working space loaded with Edit › Color Settings › Load Profile… is active again after a
//! restart (#1474). Colour settings are process-wide, so this lives in the integration tests.

use designcraft_color::cms::{self, ColorSettings};
use designcraft_engine::Session;
use designcraft_ui_egui::{DesignApp, Services, UiState};
use serde_json::json;

/// The app's first frame, where saved settings are applied: the host's `logic` tick and the
/// first real `run_ui` pass (where fonts, theme and colour settings are installed).
fn start(app: &mut DesignApp) {
    let mut out = egui::Context::default().run_ui(egui::RawInput::default(), |ui| {
        app.logic(&ui.ctx().clone());
        app.ui(ui);
    });
    out.textures_delta.clear();
}

#[test]
fn loaded_working_space_survives_restart() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("restart-test.icc");
    let mut profile = cms::icc_out::generic_cmyk_profile().unwrap().clone();
    profile.description = "Restart Test CMYK".into();
    std::fs::write(&path, profile.with_encoded_bytes().to_bytes().as_slice()).unwrap();

    // What ui.json holds after Load Profile… and choosing that profile in a previous session.
    let chosen = ColorSettings { cmyk: "Restart Test CMYK".into(), ..Default::default() };
    let saved = json!({"colorProfiles": [path.to_string_lossy()], "colorSettings": chosen});

    // This process has never loaded the profile: only the built-in ones are known.
    assert!(!cms::profiles().iter().any(|p| p.name == chosen.cmyk));
    let mut app = DesignApp::new(Session::new(), Services::default());
    app.ui = serde_json::from_value::<UiState>(saved).unwrap();
    start(&mut app);
    let active = cms::active_settings();
    cms::set_active(&ColorSettings::default()).unwrap();
    assert_eq!(active, chosen, "the loaded working space is active again");

    // A saved profile that can't be loaded any more is reported, not silently replaced.
    let gone = json!({"colorProfiles": [dir.path().join("gone.icc").to_string_lossy()], "colorSettings": ColorSettings { cmyk: "Gone CMYK".into(), ..Default::default() }});
    let mut app = DesignApp::new(Session::new(), Services::default());
    app.ui = serde_json::from_value::<UiState>(gone).unwrap();
    start(&mut app);
    assert_eq!(cms::active_settings(), ColorSettings::default());
    assert!(app.ui.status.contains("Gone CMYK"), "{}", app.ui.status);
}
