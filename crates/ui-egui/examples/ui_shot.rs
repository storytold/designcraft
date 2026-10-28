//! Headless UI screenshots: renders the whole DesignCraft window offscreen (wgpu) — works with a
//! locked screen or a hidden window, unlike `ui.screenshot`.
//!
//! `cargo run -p designcraft-ui-egui --example ui_shot -- script.jsonl`
//!
//! The script is JSON lines. Each line is a control-channel request (`{"method": …, "params": …}`,
//! see `docs/control-protocol.md`), or `{"shot": "/abs/out.png"}` to save the window as PNG, or
//! `{"steps": n}` to run extra frames. The window is 1440×900 pt at 2× and opens `file.newSample`
//! (unless the first line is `{"empty": true}`).

use designcraft_engine::Session;
use designcraft_ui_egui::{ControlRequest, DesignApp, Services};

static HOLD_SHARP: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

static READY: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

fn main() {
    let path = std::env::args().nth(1).expect("usage: ui_shot script.jsonl");
    let script = std::fs::read_to_string(&path).expect("read script");
    let services = Services {
        write: Some(Box::new(|p: &str, b: &[u8]| std::fs::write(p, b).map_err(|e| e.to_string()))),
        read: Some(Box::new(|p: &str| std::fs::read(p).map_err(|e| e.to_string()))),
        ..Default::default()
    };
    let (tx, rx) = std::sync::mpsc::channel::<ControlRequest>();
    let mut app = DesignApp::new(Session::new(), services).with_control(rx);
    app.integrated_titlebar = cfg!(target_os = "macos");
    app.custom_titlebar = cfg!(target_os = "windows");
    let lines: Vec<serde_json::Value> =
        script.lines().filter(|l| !l.trim().is_empty()).map(|l| serde_json::from_str(l).expect("json line")).collect();
    if lines.first().is_none_or(|l| l.get("empty").is_none()) {
        let _ = app.run("file.newSample", serde_json::json!({}));
    }
    let mut harness = egui_kittest::Harness::builder()
        .with_size(egui::vec2(1440.0, 900.0))
        .with_step_dt(1.0 / 60.0)
        .with_pixels_per_point(2.0)
        .with_max_steps(1_000_000)
        .wgpu()
        .build_ui_state(
            |ui, app: &mut DesignApp| {
                // The builder runs frames before `max_texture_side` can be raised: wait for it.
                if !READY.load(std::sync::atomic::Ordering::Relaxed) {
                    return;
                }
                let ctx = ui.ctx().clone();
                app.logic(&ctx);
                app.ui(ui);
            },
            app,
        );
    harness.input_mut().max_texture_side = Some(8192);
    READY.store(true, std::sync::atomic::Ordering::Relaxed);
    step_n(&mut harness, 6);
    for l in lines {
        if std::env::var_os("UI_SHOT_TRACE").is_some() {
            eprintln!("> {l}");
        }
        if let Some(hold) = l.get("holdSharpCanvas").and_then(|v| v.as_bool()) {
            HOLD_SHARP.store(hold, std::sync::atomic::Ordering::Relaxed);
            if !hold {
                harness.state_mut().canvas.pending = None;
            }
        } else if let Some(p) = l.get("shot").and_then(|v| v.as_str()) {
            step_n(&mut harness, 3);
            match harness.render() {
                Ok(img) => {
                    img.save(p).expect("save png");
                    println!("{p}");
                }
                Err(e) => eprintln!("render failed: {e}"),
            }
        } else if let Some(p) = l.get("state").and_then(|v| v.as_str()) {
            if let Err(error) = save_state(harness.state(), p) {
                eprintln!("state snapshot failed: {error}");
                std::process::exit(1);
            }
        } else if let Some(ms) = l.get("waitMs").and_then(|v| v.as_u64()) {
            // Background workers need actual wall time; synthetic frame count alone can
            // capture a pending canvas. Keep each scripted wait bounded to ten seconds.
            let start = std::time::Instant::now();
            while start.elapsed() < std::time::Duration::from_millis(ms.min(10_000)) {
                step(&mut harness);
                std::thread::sleep(std::time::Duration::from_millis(5));
            }
        } else if let Some(n) = l.get("steps").and_then(|v| v.as_u64()) {
            step_n(&mut harness, n as usize);
        } else if let Some(m) = l.get("method").and_then(|v| v.as_str()) {
            let (req, reply) = ControlRequest::new(m, l.get("params").cloned().unwrap_or_default());
            tx.send(req).expect("send");
            // Pointer/keyboard input is injected one event per frame.
            for _ in 0..40 {
                step(&mut harness);
                if let Ok(r) = reply.try_recv() {
                    let s = r.to_string();
                    println!("{m}: {}", &s[..s.len().min(300)]);
                    break;
                }
            }
            step_n(&mut harness, 2);
        }
        // Frames that aren't rendered keep their texture changes for the next render: a script
        // that edits a long document (every edit redraws the Pages panel's thumbnails) without
        // screenshots piled them up until the shot's render timed out on the GPU. Render each line.
        if l.get("shot").is_none() {
            let _ = harness.render();
        }
    }
}

/// One frame, with the app's synthetic input (`ui.click`, `ui.drag`, `ui.key`) injected like the
/// windowed app's raw-input hook does.
fn step(harness: &mut egui_kittest::Harness<'_, DesignApp>) {
    if HOLD_SHARP.load(std::sync::atomic::Ordering::Relaxed) {
        let app = harness.state_mut();
        if let (Some(st), Some(view), Some(shown)) = (app.session.active(), app.view(), app.canvas.shown) {
            // Test-only stand-in for a slow renderer: keep its old texture while preventing
            // the pending viewport from completing. No product controls or timing assumptions.
            app.canvas.pending = Some((
                u64::MAX,
                designcraft_ui_egui::Shown {
                    doc: (st.uid, std::sync::Arc::as_ptr(&st.doc) as u64, st.revision, shown.doc.3),
                    editing_parents: st.editing_parents,
                    origin: designcraft_geom::Point::new(-1e8, -1e8),
                    zoom: view.zoom,
                    size: (1e12, 1e12),
                },
            ));
        }
    }
    let mut raw = std::mem::take(harness.input_mut());
    let ctx = harness.ctx.clone();
    harness.state_mut().raw_input_hook(&ctx, &mut raw);
    *harness.input_mut() = raw;
    harness.step();
}

fn step_n(harness: &mut egui_kittest::Harness<'_, DesignApp>, n: usize) {
    for _ in 0..n {
        step(harness);
    }
}

/// Geometry-only snapshots keep private encoded artwork out of diagnostic JSON.
fn save_state(app: &DesignApp, path: &str) -> Result<(), Box<dyn std::error::Error>> {
    let state=app.session.active().map(|st|serde_json::json!({
        "interactionActive":st.interaction.is_some(),"selection":st.selection,
        "view":app.view(),"canvasRect":app.canvas_rect.map(|r|[r.left(),r.top(),r.width(),r.height()]),
        "sharpView":app.canvas.shown.map(|s|serde_json::json!({"origin":s.origin,"zoom":s.zoom,"size":s.size})),
        "perf":designcraft_ui_egui::control::inspect(app,&egui::Context::default())["perf"],
        "currentPage":designcraft_ui_egui::canvas::current_page(app).map(|p|p+1),
        "items":st.doc.spreads.iter().flat_map(|sp|sp.items.iter()).map(|it|serde_json::json!({"id":it.id.0,"xf":it.xf.as_coeffs(),"opacity":it.opacity})).collect::<Vec<_>>()
    }));
    std::fs::write(path, serde_json::to_vec_pretty(&state)?)?;
    Ok(())
}
