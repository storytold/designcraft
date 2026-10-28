//! Reproducible headless calendar navigation/render benchmark. Uses supplied local files only.
use std::time::{Duration, Instant};

use designcraft_engine::Session;
use designcraft_ui_egui::{DesignApp, Services};
use serde_json::json;

fn frame(app: &mut DesignApp, ctx: &egui::Context) -> f64 {
    let start = Instant::now();
    let input = egui::RawInput {
        screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(1440.0, 900.0))),
        max_texture_side: Some(8192),
        ..Default::default()
    };
    let mut output = ctx.run_ui(input, |ui| {
        app.logic(ui.ctx());
        app.ui(ui);
    });
    output.textures_delta.clear();
    start.elapsed().as_secs_f64() * 1000.0
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut args = std::env::args().skip(1);
    let path = args.next().ok_or("usage: perf_calendar DOCUMENT [CACHE_DIRECTORY]")?;
    if let Some(path) = args.next() {
        designcraft_render::images::set_preview_directory(Some(path.into()));
    }
    let started = Instant::now();
    let mut session = Session::new();
    session.execute("file.open", &json!({"path": path}))?;
    let open_ms = started.elapsed().as_secs_f64() * 1000.0;
    let mut app = DesignApp::new(session, Services::default());
    let ctx = egui::Context::default();
    app.ui.dock_tab = "pages".into();
    app.ui.dock_expanded = true;
    let mut first_frames = Vec::new();
    for _ in 0..8 {
        first_frames.push(frame(&mut app, &ctx));
        std::thread::sleep(Duration::from_millis(5));
    }
    let mut visits = Vec::new();
    for pass in 0..2 {
        for page in [3, 7, 11, 15, 19, 23, 27, 3] {
            designcraft_ui_egui::canvas::go_to_page(&mut app, page);
            let start = Instant::now();
            let mut frames = vec![frame(&mut app, &ctx)];
            while app.canvas.pending.is_some() {
                if start.elapsed() > Duration::from_secs(90) {
                    return Err("canvas did not settle in 90 seconds".into());
                }
                std::thread::sleep(Duration::from_millis(5));
                frames.push(frame(&mut app, &ctx));
            }
            visits.push(json!({"pass":pass,"page":page+1,"settledMs":start.elapsed().as_secs_f64()*1000.0,"maxFrameMs":frames.iter().copied().fold(0.0,f64::max),"frames":frames.len()}));
        }
    }
    // Warm all page previews once, then repeat; isolates image cache reuse from UI scheduling.
    let doc = app.session.active().ok_or("no document")?.doc.clone();
    let mut renderer = designcraft_render::Renderer::new();
    let mut passes = Vec::new();
    for pass in 0..2 {
        let start = Instant::now();
        for page in 0..doc.page_count() {
            let _ = renderer.render_page(&doc, &app.session.cache, page, 0.5, false, &Default::default()).ok_or("page render failed")?;
        }
        passes.push(json!({"pass":pass,"ms":start.elapsed().as_secs_f64()*1000.0}));
    }
    println!(
        "{}",
        json!({"openMs":open_ms,"firstFramesMs":first_frames,"visits":visits,"allPages":passes,"inspect":designcraft_ui_egui::control::inspect(&app,&ctx)})
    );
    Ok(())
}
