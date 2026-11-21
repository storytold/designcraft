//! Presentation Mode must ask the host window for true full screen, and give it back on leaving.

use designcraft_engine::Session;
use designcraft_ui_egui::{DesignApp, Services};
use serde_json::json;

/// One logic tick; returns the commands sent to the root window.
fn tick(app: &mut DesignApp, ctx: &egui::Context) -> Vec<egui::ViewportCommand> {
    let mut out = ctx.run_ui(egui::RawInput::default(), |ui| app.logic(ui.ctx()));
    out.textures_delta.clear();
    out.viewport_output.get(&egui::ViewportId::ROOT).map(|v| v.commands.clone()).unwrap_or_default()
}

#[test]
fn presentation_enters_and_leaves_true_full_screen() {
    let ctx = egui::Context::default();
    let mut app = DesignApp::new(Session::new(), Services::default());
    app.run("file.new", json!({})).unwrap();
    assert!(!tick(&mut app, &ctx).iter().any(|c| matches!(c, egui::ViewportCommand::Fullscreen(_))));

    app.run("view.togglePresentation", json!({})).unwrap();
    let cmds = tick(&mut app, &ctx);
    assert!(cmds.contains(&egui::ViewportCommand::Fullscreen(true)), "entering Presentation goes full screen: {cmds:?}");

    app.run("view.togglePresentation", json!({})).unwrap();
    let cmds = tick(&mut app, &ctx);
    assert!(cmds.contains(&egui::ViewportCommand::Fullscreen(false)), "leaving Presentation restores the window: {cmds:?}");

    app.run("view.screenMode", json!({"mode": "presentation"})).unwrap();
    assert!(tick(&mut app, &ctx).contains(&egui::ViewportCommand::Fullscreen(true)));
    app.run("view.screenMode", json!({"mode": "presentation"})).unwrap();
    assert!(!tick(&mut app, &ctx).iter().any(|c| matches!(c, egui::ViewportCommand::Fullscreen(_))), "staying in Presentation sends nothing");
    app.run("view.screenMode", json!({"mode": "normal"})).unwrap();
    assert!(tick(&mut app, &ctx).contains(&egui::ViewportCommand::Fullscreen(false)));
}
