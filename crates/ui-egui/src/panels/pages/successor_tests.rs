//! Successors, on the framework path, of the Pages panel thumbnail tests that the framework twins cover only in part: the
//! one-refresh-per-frame budget after an edit, and the eviction of the thumbnails of deleted pages.

use super::*;
use egui::vec2;
use serde_json::json;

/// Successor of `an_edit_redraws_only_thumbnails_in_view_one_per_frame`, which the framework twin
/// `framework_path::only_the_thumbnails_in_view_are_rendered` covers for the first half only: on the installed framework, after
/// an edit the previous thumbnails stay until the document has been unchanged for `THUMB_QUIET_SECS`, then one thumbnail is
/// drawn again per frame, and the thumbnails of deleted pages are dropped.
#[test]
fn the_framework_panel_redraws_one_thumbnail_per_frame_after_an_edit_and_drops_deleted_pages() {
    let mut app = DesignApp::new_for_panel_tests(designcraft_engine::Session::new(), crate::Services::default());
    app.canvas_rect = Some(egui::Rect::from_min_size(egui::Pos2::ZERO, vec2(1000.0, 800.0)));
    app.run("file.new", json!({"pages": 60, "facingPages": false})).unwrap();
    let ctx = egui::Context::default();
    crate::theme::install_fonts(&ctx, "");
    app.install_framework(&ctx);
    let frame = |app: &mut DesignApp, time: f64| {
        let before = THUMB_RENDERS.with(std::cell::Cell::get);
        let mut raw = egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, vec2(280.0, 520.0))),
            max_texture_side: Some(8192),
            time: Some(time),
            ..Default::default()
        };
        app.raw_input_hook(&ctx, &mut raw);
        ctx.run_ui(raw, |ui| show(app, ui)).textures_delta.clear();
        THUMB_RENDERS.with(std::cell::Cell::get) - before
    };
    let shown: usize = (0..6).map(|_| frame(&mut app, 0.0)).sum();
    assert!(shown > 0 && shown < 60, "only the rows in view are rendered: {shown}");
    assert_eq!(frame(&mut app, 0.0), 0, "an unchanged document renders nothing");

    app.run("frame.create", json!({"rect": [36, 36, 200, 200]})).unwrap();
    assert_eq!(frame(&mut app, 1.0), 0, "the edit's frame keeps the previous thumbnails");
    assert_eq!(frame(&mut app, 1.0 + THUMB_QUIET_SECS / 2.0), 0, "still editing");
    let quiet = 1.0 + THUMB_QUIET_SECS + 0.01;
    let mut redrawn = 0;
    for _ in 0..shown {
        let n = frame(&mut app, quiet);
        assert_eq!(n, THUMB_REFRESHES_PER_FRAME, "one thumbnail per frame");
        redrawn += n;
    }
    assert_eq!(redrawn, shown);
    assert_eq!(frame(&mut app, quiet), 0, "every thumbnail in view is current");

    // Deleted pages lose their thumbnails.
    let uid = app.session.active().unwrap().uid;
    let cached = |abs: usize| {
        THUMBS.lock().unwrap_or_else(std::sync::PoisonError::into_inner).as_ref().unwrap().keys().any(|k| k.0 == uid && k.1 == ThumbOf::Page(abs))
    };
    assert!(cached(2));
    app.run("layout.pages.delete", json!({"pages": (2..60).collect::<Vec<_>>()})).unwrap();
    frame(&mut app, quiet + 1.0);
    assert!((2..60).all(|abs| !cached(abs)), "no thumbnail of a deleted page is kept");
}
