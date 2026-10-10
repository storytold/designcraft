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
        if let Some(p) = l.get("shot").and_then(|v| v.as_str()) {
            step_n(&mut harness, 3);
            match harness.render() {
                Ok(img) => {
                    img.save(p).expect("save png");
                    println!("{p}");
                }
                Err(e) => eprintln!("render failed: {e}"),
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
    }
}

/// One frame, with the app's synthetic input (`ui.click`, `ui.drag`, `ui.key`) injected like the
/// windowed app's raw-input hook does.
fn step(harness: &mut egui_kittest::Harness<'_, DesignApp>) {
    let mut raw = std::mem::take(harness.input_mut());
    harness.state_mut().raw_input_hook(&mut raw);
    *harness.input_mut() = raw;
    harness.step();
}

fn step_n(harness: &mut egui_kittest::Harness<'_, DesignApp>, n: usize) {
    for _ in 0..n {
        step(harness);
    }
}
