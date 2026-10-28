//! Reproducible large-background drag benchmark. Operates on an in-memory copy only.
use designcraft_doc::Content;
use designcraft_engine::Session;
use designcraft_ui_egui::{DesignApp, Services};
use serde_json::json;
use std::time::{Duration, Instant};
fn frame(app: &mut DesignApp, ctx: &egui::Context) -> f64 {
    let start = Instant::now();
    let mut out = ctx.run_ui(
        egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(1440.0, 900.0))),
            max_texture_side: Some(8192),
            ..Default::default()
        },
        |ui| {
            app.logic(ui.ctx());
            app.ui(ui);
        },
    );
    out.textures_delta.clear();
    start.elapsed().as_secs_f64() * 1000.0
}
fn settle(app: &mut DesignApp, ctx: &egui::Context) -> Result<Vec<f64>, Box<dyn std::error::Error>> {
    let start = Instant::now();
    let mut times = vec![frame(app, ctx)];
    while app.canvas.pending.is_some() || app.canvas_rect.is_none() {
        if start.elapsed() > Duration::from_secs(60) {
            return Err("canvas settle timeout".into());
        }
        std::thread::sleep(Duration::from_millis(5));
        times.push(frame(app, ctx));
    }
    Ok(times)
}
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut args = std::env::args().skip(1);
    let path = args.next().ok_or("usage: perf_drag DOCUMENT [PREVIEW_CACHE]")?;
    if let Some(dir) = args.next() {
        designcraft_render::images::set_preview_directory(Some(dir.into()));
    }
    let mut s = Session::new();
    s.execute("file.open", &json!({"path":path}))?;
    let st = s.active().ok_or("no document")?;
    let mut first = 0;
    let mut found = None;
    for (si, sp) in st.doc.spreads.iter().enumerate() {
        if first <= 3 && first + sp.pages.len() > 3 {
            found = sp
                .items
                .iter()
                .filter(|i| matches!(i.content, Content::Graphic(_)))
                .max_by(|a, b| a.bounds().area().total_cmp(&b.bounds().area()))
                .map(|i| (si, i.id));
            break;
        }
        first += sp.pages.len();
    }
    let (_, id) = found.ok_or("page 4 has no graphic")?;
    s.edit(|d, _| {
        d.item_mut(id).ok_or(designcraft_doc::DocError::NoItem(id))?.locked = false;
        Ok(())
    })?;
    s.execute("selection.set", &json!({"ids":[id.0]}))?;
    let mut app = DesignApp::new(s, Services::default());
    let ctx = egui::Context::default();
    // Font setup can skip the first UI frame; wait for a real canvas before navigation.
    let startup = settle(&mut app, &ctx)?;
    designcraft_ui_egui::canvas::go_to_page(&mut app, 3);
    let initial = settle(&mut app, &ctx)?;
    // Give asynchronous selection preparation real wall time, identical for both builds.
    let warm = Instant::now();
    while warm.elapsed() < Duration::from_secs(2) {
        frame(&mut app, &ctx);
        std::thread::sleep(Duration::from_millis(10));
    }
    if designcraft_ui_egui::canvas::current_page(&app) != Some(3) {
        return Err(format!("benchmark view did not reach page 4: {:?}", designcraft_ui_egui::canvas::current_page(&app)).into());
    }
    let original = app.session.active().ok_or("no document")?.doc.clone();
    app.session.begin_interaction("Move large background")?;
    let mut previews = vec![];
    let mut ui_times = vec![];
    for n in 1..=24 {
        let start = Instant::now();
        app.session.preview("transform.move", json!({"dx":n as f64*3.0,"dy":n as f64*1.5}))?;
        previews.push(start.elapsed().as_secs_f64() * 1000.0);
        ui_times.push(frame(&mut app, &ctx));
        std::thread::sleep(Duration::from_millis(5));
    }
    let before_commit = designcraft_ui_egui::control::inspect(&app, &ctx);
    app.session.commit_interaction()?;
    let final_draw = settle(&mut app, &ctx)?;
    let moved = app.session.active().ok_or("no document")?.doc.clone();
    let old = original.item(id).ok_or("original item missing")?.xf.as_coeffs();
    let new = moved.item(id).ok_or("moved item missing")?.xf.as_coeffs();
    assert!((new[4] - old[4] - 72.0).abs() < 1e-6 && (new[5] - old[5] - 36.0).abs() < 1e-6);
    app.session.execute("edit.undo", &json!({}))?;
    assert_eq!(app.session.active().ok_or("no document")?.doc.item(id).ok_or("undo item missing")?.xf.as_coeffs(), old);
    settle(&mut app, &ctx)?;
    app.session.begin_interaction("Cancel move")?;
    app.session.preview("transform.move", json!({"dx":13.0,"dy":17.0}))?;
    frame(&mut app, &ctx);
    app.session.cancel_interaction();
    settle(&mut app, &ctx)?;
    assert_eq!(app.session.active().ok_or("no document")?.doc.item(id).ok_or("cancel item missing")?.xf.as_coeffs(), old);
    println!(
        "{}",
        json!({"item":id.0,"startupFramesMs":startup,"visiblePage":designcraft_ui_egui::canvas::current_page(&app).map(|p|p+1),"initialFramesMs":initial,"previewCommandMs":previews,"dragFrameMs":ui_times,"commitFramesMs":final_draw,"beforeCommit":before_commit["perf"],"finalInspect":designcraft_ui_egui::control::inspect(&app,&ctx)["perf"],"finalTransformVerified":true,"undoVerified":true,"cancelVerified":true})
    );
    Ok(())
}
