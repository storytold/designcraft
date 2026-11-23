#![cfg(not(target_arch = "wasm32"))]

//! Diagnostic (not a gate): where does Tab go when a craft framework field holds the keyboard in this host?
//!
//! It prints, for one Tab press: the pending raw input before the host hook, after the hook, the focus before
//! and after the pass, and — through `DESIGNCRAFT_TAB_TRACE` — what the framework field itself saw that pass.

use crate::DesignApp;
use egui::{Event, Key, Modifiers, PointerButton, vec2};
use egui_kittest::Harness;
use serde_json::json;

fn window_harness(mut app: DesignApp) -> Harness<'static, DesignApp> {
    let ctx = egui::Context::default();
    app.framework_opacity = true;
    app.install_framework(&ctx);
    let mut h = Harness::builder().with_context(ctx).with_size(vec2(1440.0, 900.0)).build_ui_state(
        |ui, app: &mut DesignApp| {
            // The real seam: what the pass actually received, before any of the app's code runs.
            if std::env::var("DESIGNCRAFT_TAB_TRACE").is_ok() {
                let events: Vec<String> = ui.input(|i| i.events.iter().map(|e| format!("{e:?}")).collect());
                if events.iter().any(|e| e.contains("Tab")) {
                    eprintln!("TRACE pass start: focus={:?} events={events:?}", ui.ctx().memory(|m| m.focused()));
                }
            }
            let ctx = ui.ctx().clone();
            app.logic(&ctx);
            app.ui(ui);
        },
        app,
    );
    h.input_mut().max_texture_side = Some(8192);
    for _ in 0..6 {
        step(&mut h);
    }
    h
}

fn step(h: &mut Harness<'_, DesignApp>) {
    let mut raw = std::mem::take(h.input_mut());
    let ctx = h.ctx.clone();
    h.state_mut().raw_input_hook(&ctx, &mut raw);
    *h.input_mut() = raw;
    h.step();
}

fn steps(h: &mut Harness<'_, DesignApp>, n: usize) {
    for _ in 0..n {
        step(h);
    }
}

#[test]
fn where_tab_goes_when_the_framework_field_holds_the_keyboard() {
    let mut app = DesignApp::new(designcraft_engine::Session::new(), crate::Services::default());
    app.run("file.new", json!({"title": "Tab trace"})).expect("a new document");
    let id = designcraft_doc::ItemId(app.run("frame.create", json!({"rect": [72, 72, 272, 172]})).expect("a frame")["id"].as_u64().expect("an id"));
    app.run("selection.set", json!({"ids": [id.0]})).expect("selected");
    let mut h = window_harness(app);
    let field = h.state().framework_opacity_fields.iter().rev().find(|(k, _)| k == "opacity").map(|(_, r)| *r).expect("the pilot field");
    let at = field.center();
    h.event(Event::PointerMoved(at));
    h.event(Event::PointerButton { pos: at, button: PointerButton::Primary, pressed: true, modifiers: Modifiers::NONE });
    h.event(Event::PointerButton { pos: at, button: PointerButton::Primary, pressed: false, modifiers: Modifiers::NONE });
    steps(&mut h, 3);
    h.event(Event::Key { key: Key::A, physical_key: None, pressed: true, repeat: false, modifiers: Modifiers::COMMAND });
    steps(&mut h, 1);
    for c in "40".chars() {
        h.event(Event::Text(c.to_string()));
    }
    steps(&mut h, 2);
    eprintln!("TRACE focus with a draft: {:?}", h.ctx.memory(|m| m.focused()));

    // The Tab press, stage by stage.
    h.event(Event::Key { key: Key::Tab, physical_key: None, pressed: true, repeat: false, modifiers: Modifiers::NONE });
    let pending: Vec<String> = h.input_mut().events.iter().map(|e| format!("{e:?}")).collect();
    eprintln!("TRACE pending before hook (harness-side view, may be pre-merge): {pending:?}");
    let mut raw = std::mem::take(h.input_mut());
    let ctx = h.ctx.clone();
    h.state_mut().raw_input_hook(&ctx, &mut raw);
    let after: Vec<String> = raw.events.iter().map(|e| format!("{e:?}")).collect();
    eprintln!("TRACE pending after hook: {after:?}");
    *h.input_mut() = raw;
    eprintln!("TRACE focus before the pass: {:?}", h.ctx.memory(|m| m.focused()));
    h.step();
    let opacity = f64::from(h.state().session.doc().expect("doc").doc.item(id).expect("frame").opacity);
    eprintln!(
        "TRACE after the pass: focus={:?} opacity={opacity} undo={} text_edit_focused={}",
        h.ctx.memory(|m| m.focused()),
        h.state().session.doc().expect("doc").history.undo.len(),
        h.ctx.text_edit_focused()
    );
    // Then a couple more passes, so a deferred commit would show.
    steps(&mut h, 2);
    eprintln!(
        "TRACE two passes later: focus={:?} opacity={} undo={}",
        h.ctx.memory(|m| m.focused()),
        f64::from(h.state().session.doc().expect("doc").doc.item(id).expect("frame").opacity),
        h.state().session.doc().expect("doc").history.undo.len()
    );
}
