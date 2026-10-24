//! Edits render on the UI thread while that is quick, and on the worker once it is slow, so a
//! heavy document doesn't stall the UI (clicks piling up, panels not answering).

use super::*;

fn frame(app: &mut DesignApp, ctx: &egui::Context) {
    let rect = Rect::from_min_size(Pos2::ZERO, vec2(400.0, 300.0));
    let view = View { zoom: 1.0, origin: Point::new(0.0, 0.0), fitted: true, rotation: 0 };
    let mut out = ctx.run_ui(Default::default(), |ui| {
        let st = app.session.active().unwrap();
        let layout = CanvasLayout::new(&st.doc, st.editing_parents);
        render_texture(app, ui.ctx(), rect, &Xf::new(rect, &view), &layout, false);
    });
    out.textures_delta.clear();
}

/// The texture's document is the current one.
fn current(app: &DesignApp) -> bool {
    let st = app.session.active().unwrap();
    app.canvas.shown.is_some_and(|s| s.doc == (st.uid, std::sync::Arc::as_ptr(&st.doc) as u64, st.revision, false))
}

#[test]
fn edits_render_on_the_worker_once_rendering_is_slow() {
    let mut app = DesignApp::new(designcraft_engine::Session::new(), crate::Services::default());
    app.run("file.new", json!({})).unwrap();
    let ctx = egui::Context::default();
    frame(&mut app, &ctx);
    assert!(current(&app), "the first frame renders at once");
    if app.canvas.worker.is_none() {
        // DESIGNCRAFT_SYNC_RENDER: no worker, everything renders here.
        return;
    }
    // Quick: an edit shows in the same frame.
    app.canvas.sync_ms = 0.0;
    app.canvas.patch_ms = 0.0;
    app.run("frame.create", json!({"rect": [20, 20, 120, 120]})).unwrap();
    frame(&mut app, &ctx);
    assert!(current(&app) && app.canvas.pending.is_none(), "a quick edit renders on the UI thread");
    // Slow: the edit goes to the worker; the UI thread doesn't wait for it.
    app.canvas.sync_ms = 10.0 * SYNC_BUDGET_MS;
    app.canvas.patch_ms = 10.0 * SYNC_BUDGET_MS;
    app.run("frame.create", json!({"rect": [140, 20, 240, 120]})).unwrap();
    frame(&mut app, &ctx);
    assert!(!current(&app) && app.canvas.pending.is_some(), "a slow edit renders on the worker");
    for _ in 0..500 {
        if app.canvas.pending.is_none() {
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(10));
        frame(&mut app, &ctx);
    }
    assert!(current(&app), "the worker's image arrives");
    assert!(app.canvas.sync_ms < 10.0 * SYNC_BUDGET_MS, "its time replaces the slow one");
}

/// A slow repaint of an edit's regions sends the next edit to the worker even when the last full
/// render was quick: a full render on the UI thread would stall at least as long.
#[test]
fn after_a_slow_patch_the_next_edit_goes_to_the_worker() {
    let mut app = DesignApp::new(designcraft_engine::Session::new(), crate::Services::default());
    app.run("file.new", json!({})).unwrap();
    let ctx = egui::Context::default();
    frame(&mut app, &ctx);
    if app.canvas.worker.is_none() {
        return;
    }
    app.canvas.sync_ms = 0.0;
    app.canvas.patch_ms = 10.0 * SYNC_BUDGET_MS;
    app.run("frame.create", json!({"rect": [20, 20, 120, 120]})).unwrap();
    frame(&mut app, &ctx);
    assert!(!current(&app) && app.canvas.pending.is_some(), "the edit renders on the worker");
}
