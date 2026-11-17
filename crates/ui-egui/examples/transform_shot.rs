//! Headless proof of the Transform pilot in the whole DesignCraft window: a real pointer click into the
//! craft framework's W field, typed text and Return (egui events, as a person sends them), the frame's
//! bounds and undo depth read from the document, and the window rendered offscreen (wgpu) before,
//! after the edit and after `edit.undo`.
//!
//! `DESIGNCRAFT_FRAMEWORK_TRANSFORM=1 cargo run -p designcraft-ui-egui --example transform_shot -- <out-dir>`

use designcraft_engine::Session;
use designcraft_ui_egui::{ControlRequest, DesignApp, Services};
use egui::{Event, Key, Modifiers, PointerButton};
use serde_json::{Value, json};

static READY: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

type H = egui_kittest::Harness<'static, DesignApp>;

fn main() {
    let out = std::path::PathBuf::from(std::env::args().nth(1).expect("usage: transform_shot <out-dir>"));
    std::fs::create_dir_all(&out).expect("out dir");
    let (tx, rx) = std::sync::mpsc::channel::<ControlRequest>();
    let mut app = DesignApp::new(Session::new(), Services::default()).with_control(rx);
    app.run("file.newSample", json!({})).expect("sample");
    app.run("document.preferences", json!({"horizontalUnits": "points", "verticalUnits": "points"})).expect("points");
    let id = app.run("frame.create", json!({"rect": [72, 72, 372, 272]})).expect("frame")["id"].as_u64().expect("id");
    app.run("selection.set", json!({"ids": [id]})).expect("select");
    let ctx = egui::Context::default();
    app.install_framework(&ctx);
    let mut h: H = egui_kittest::Harness::builder()
        .with_context(ctx)
        .with_size(egui::vec2(1440.0, 900.0))
        .with_step_dt(1.0 / 60.0)
        .with_pixels_per_point(2.0)
        .with_max_steps(1_000_000)
        .wgpu()
        .build_ui_state(
            |ui, app: &mut DesignApp| {
                if !READY.load(std::sync::atomic::Ordering::Relaxed) {
                    return;
                }
                let ctx = ui.ctx().clone();
                app.logic(&ctx);
                app.ui(ui);
            },
            app,
        );
    h.input_mut().max_texture_side = Some(8192);
    READY.store(true, std::sync::atomic::Ordering::Relaxed);
    steps(&mut h, 12);
    let fields = control(&mut h, &tx, "ui.inspect", json!({}))["frameworkTransform"].clone();
    println!("frameworkTransform: {fields}");
    let w = fields.get("width").and_then(Value::as_array).expect("the pilot's W field is on screen");
    let r: Vec<f32> = w.iter().map(|v| v.as_f64().unwrap_or(0.0) as f32).collect();
    let at = egui::pos2(r[0] + r[2] / 2.0, r[1] + r[3] / 2.0);
    let report = |h: &H, label: &str| {
        let st = h.state().session.doc().expect("doc");
        let b = st.doc.item(designcraft_doc::ItemId(id)).expect("frame").bounds();
        println!("{label}: bounds x0={:.3} y0={:.3} w={:.3} h={:.3} undo={}", b.x0, b.y0, b.width(), b.height(), st.history.undo.len());
    };
    report(&h, "before");
    shot(&mut h, &out.join("1-before.png"));
    for e in [
        Event::PointerMoved(at),
        Event::PointerButton { pos: at, button: PointerButton::Primary, pressed: true, modifiers: Modifiers::NONE },
        Event::PointerButton { pos: at, button: PointerButton::Primary, pressed: false, modifiers: Modifiers::NONE },
    ] {
        h.event(e);
    }
    steps(&mut h, 3);
    h.event(Event::Key { key: Key::A, physical_key: None, pressed: true, repeat: false, modifiers: Modifiers::COMMAND });
    for c in "150".chars() {
        h.event(Event::Text(c.to_string()));
    }
    shot(&mut h, &out.join("2-typing.png"));
    h.event(Event::Key { key: Key::Enter, physical_key: None, pressed: true, repeat: false, modifiers: Modifiers::NONE });
    h.event(Event::Key { key: Key::Enter, physical_key: None, pressed: false, repeat: false, modifiers: Modifiers::NONE });
    steps(&mut h, 6);
    report(&h, "after W=150 + Return");
    shot(&mut h, &out.join("3-after-edit.png"));
    h.state_mut().run("edit.undo", json!({})).expect("undo");
    steps(&mut h, 6);
    report(&h, "after edit.undo");
    shot(&mut h, &out.join("4-after-undo.png"));
}

fn control(h: &mut H, tx: &std::sync::mpsc::Sender<ControlRequest>, method: &str, params: Value) -> Value {
    let (req, reply) = ControlRequest::new(method, params);
    tx.send(req).expect("send");
    for _ in 0..40 {
        steps(h, 1);
        if let Ok(r) = reply.try_recv() {
            return r.get("result").cloned().unwrap_or(r);
        }
    }
    panic!("no reply to {method}");
}

fn shot(h: &mut H, path: &std::path::Path) {
    steps(h, 3);
    h.render().expect("render").save(path).expect("save png");
    println!("{}", path.display());
}

fn steps(h: &mut H, n: usize) {
    for _ in 0..n {
        let mut raw = std::mem::take(h.input_mut());
        let ctx = h.ctx.clone();
        h.state_mut().raw_input_hook(&ctx, &mut raw);
        *h.input_mut() = raw;
        h.step();
    }
}
