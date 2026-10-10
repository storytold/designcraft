//! Programmatic control of the running app (agents, tests, MCP).
//!
//! Methods (JSON-lines over the host's transport):
//! - `engine.execute {command, params}` / `ui.menu.invoke {command|id, params}`: run any command
//! - `engine.commands`: engine + UI commands with enablement
//! - `document.inspect`: document summary; `ui.inspect`: UI state
//! - `ui.menu.list`: flattened menu tree
//! - `ui.tool.select {tool}`, `ui.tool.list`
//! - `ui.pointer {events:[{kind: down|drag|up|move|doubleclick, x, y, space?: "doc"|"screen"}], mods?}`:
//!   drive the active tool through the same path as the mouse
//! - `ui.key {key, shift?, alt?, cmd?}` / `ui.text {text}`: synthetic keyboard input
//! - `ui.move {x, y}` / `ui.click {x, y, button?, count?, shift?…}` / `ui.drag {x, y, toX, toY, steps?}`:
//!   real egui pointer input in screen points (reaches every widget: panels, flyouts, dialogs)
//! - `ui.set {brightness?, panel?, dockTab?, rulers?, outline?, …}`
//! - `ui.dialog.set {field, value}` / `ui.dialog.confirm` / `ui.dialog.cancel`
//! - `ui.resize {width, height}`, `ui.focus`, `ui.screenshot {path?}`
//! - `ui.render {path?, scale?}`: render the active artboard headlessly (PNG)
//! - `app.open {path}` / `app.save {path?}` / `app.export {path, page?, scale?}` (PNG) / `app.quit`

use std::sync::mpsc::Sender;

use designcraft_tools::{Mods, PointerEvent, PointerKind};
use serde_json::{Value, json};

use crate::DesignApp;
use crate::canvas::Xf;
use crate::menus;

pub type ControlResponse = Value;

pub struct ControlRequest {
    pub method: String,
    pub params: Value,
    pub reply: Sender<ControlResponse>,
}

impl ControlRequest {
    pub fn new(method: impl Into<String>, params: Value) -> (Self, std::sync::mpsc::Receiver<ControlResponse>) {
        let (tx, rx) = std::sync::mpsc::channel();
        (Self { method: method.into(), params, reply: tx }, rx)
    }
}

pub enum Outcome {
    Done(Value),
    Screenshot { path: Option<String> },
}

fn ok(v: Value) -> Outcome {
    Outcome::Done(json!({"ok": true, "result": v}))
}
fn err(e: impl std::fmt::Display) -> Outcome {
    Outcome::Done(json!({"ok": false, "error": e.to_string()}))
}
fn wrap(r: Result<Value, String>) -> Outcome {
    match r {
        Ok(v) => ok(v),
        Err(e) => err(e),
    }
}

pub fn all_commands(app: &DesignApp) -> Value {
    let mut v: Vec<Value> = app.session.commands().into_iter().map(|c| serde_json::to_value(c).unwrap_or_default()).collect();
    for (id, label, sc, params) in crate::menus::UI_COMMANDS {
        v.push(json!({"id": id, "label": label, "shortcut": sc, "params": params, "enabled": crate::menus::enabled(app, id), "ui": true}));
    }
    Value::Array(v)
}

pub fn inspect(app: &DesignApp, ctx: &egui::Context) -> Value {
    let r = ctx.content_rect();
    json!({
        "tool": app.session.tool_id(),
        "ui": serde_json::to_value(&app.ui).unwrap_or_default(),
        "view": app.view().map(|v| serde_json::to_value(v).unwrap_or_default()),
        "canvasRect": app.canvas_rect.map(|c| json!([c.left(), c.top(), c.width(), c.height()])),
        "window": [r.width(), r.height()],
        "documents": app.session.documents().iter().map(|d| json!({"title": d.title(), "dirty": d.is_dirty()})).collect::<Vec<_>>(),
        "currentPage": crate::canvas::current_page(app),
        "dialog": app.ui.dialog.as_ref().map(|d| serde_json::to_value(d).unwrap_or_default()),
        "activeDocument": app.session.active_index(),
        "perf": {"frameMs": app.perf.frame_ms, "renderMs": app.perf.render_ms, "fps": app.perf.fps, "patches": app.canvas.patches},
    })
}

fn key_from(name: &str) -> Option<egui::Key> {
    egui::Key::from_name(name).or(match name.to_ascii_lowercase().as_str() {
        "enter" | "return" => Some(egui::Key::Enter),
        "esc" | "escape" => Some(egui::Key::Escape),
        "delete" => Some(egui::Key::Delete),
        "backspace" => Some(egui::Key::Backspace),
        "left" => Some(egui::Key::ArrowLeft),
        "right" => Some(egui::Key::ArrowRight),
        "up" => Some(egui::Key::ArrowUp),
        "down" => Some(egui::Key::ArrowDown),
        "space" => Some(egui::Key::Space),
        "tab" => Some(egui::Key::Tab),
        _ => None,
    })
}

/// Handle one control request. Commands it runs leave the user's system clipboard alone.
pub fn handle(app: &mut DesignApp, ctx: &egui::Context, req: &ControlRequest) -> Outcome {
    let was = std::mem::replace(&mut app.in_control, true);
    let out = handle_request(app, ctx, req);
    app.in_control = was;
    out
}

fn handle_request(app: &mut DesignApp, ctx: &egui::Context, req: &ControlRequest) -> Outcome {
    let p = &req.params;
    let s = |k: &str| p.get(k).and_then(Value::as_str);
    match req.method.as_str() {
        "ui.menu.invoke" if p.get("params").is_none_or(Value::is_null) => {
            // Exactly like choosing the menu item (commands ending in "…" open their dialog).
            let Some(id) = s("command").or(s("id")) else { return err("missing `command`") };
            menus::activate(app, id, &Value::Null);
            ok(json!({"dialog": app.ui.dialog.as_ref().map(|d| d.id.clone())}))
        }
        "engine.execute" | "ui.menu.invoke" | "command" => {
            let Some(id) = s("command").or(s("id")) else { return err("missing `command`") };
            let params = p.get("params").cloned().unwrap_or(json!({}));
            let params = if params.is_null() { json!({}) } else { params };
            wrap(app.run(id, params))
        }
        "engine.commands" => ok(all_commands(app)),
        "document.inspect" => wrap(app.run("document.inspect", json!({}))),
        "ui.inspect" => ok(inspect(app, ctx)),
        "ui.menu.list" => ok(serde_json::to_value(menus::MENUS).unwrap_or_default()),
        "ui.tool.select" => wrap(app.run("tool.select", json!({"tool": s("tool").unwrap_or("")}))),
        "ui.tool.list" => ok(serde_json::to_value(designcraft_tools::TOOL_GROUPS).unwrap_or_default()),
        "ui.pointer" => {
            let Some(events) = p.get("events").and_then(Value::as_array) else { return err("missing `events`") };
            let base_mods: Mods = p.get("mods").and_then(|m| serde_json::from_value(m.clone()).ok()).unwrap_or_default();
            let view = app.view_info();
            let xf = app.canvas_rect.zip(app.view().copied()).map(|(rect, v)| Xf::new(rect, &v));
            let mut last = None;
            for e in events {
                let kind = match e.get("kind").and_then(Value::as_str).unwrap_or("") {
                    "down" => PointerKind::Down,
                    "drag" => PointerKind::Drag,
                    "up" => PointerKind::Up,
                    "move" => PointerKind::Move,
                    "doubleclick" | "dblclick" => PointerKind::DoubleClick,
                    other => return err(format!("unknown pointer kind `{other}`")),
                };
                let x = e.get("x").and_then(Value::as_f64).unwrap_or(0.0);
                let y = e.get("y").and_then(Value::as_f64).unwrap_or(0.0);
                let pos = if e.get("space").and_then(Value::as_str) == Some("screen") {
                    match xf {
                        Some(xf) => xf.to_canvas(egui::pos2(x as f32, y as f32)),
                        None => return err("no canvas yet"),
                    }
                } else {
                    designcraft_geom::Point::new(x, y)
                };
                let mods = e.get("mods").and_then(|m| serde_json::from_value(m.clone()).ok()).unwrap_or(base_mods);
                let clicks = e.get("clicks").and_then(Value::as_u64).map_or(1, |n| n.clamp(1, 255) as u8);
                if let Err(e) = app.session.pointer(&PointerEvent { kind, pos, mods, clicks }, view) {
                    return err(e);
                }
                app.after_engine();
                last = Some((pos, mods));
            }
            ctx.request_repaint();
            // The tool's cursor where the last event was.
            let cursor = last.map(|(pos, mods)| app.session.cursor(pos, mods, view));
            wrap(
                app.run("document.inspect", json!({})).map(|d| json!({"selection": d["selection"], "tool": app.session.tool_id(), "cursor": cursor})),
            )
        }
        "ui.key" => {
            let Some(k) = s("key").and_then(key_from) else { return err("unknown or missing `key`") };
            let b = |n: &str| p.get(n).and_then(Value::as_bool).unwrap_or(false);
            let m = egui::Modifiers {
                alt: b("alt"),
                ctrl: b("ctrl"),
                shift: b("shift"),
                mac_cmd: b("cmd") && cfg!(target_os = "macos"),
                command: b("cmd"),
            };
            app.synthetic.push(egui::Event::Key { key: k, physical_key: None, pressed: true, repeat: false, modifiers: m });
            app.synthetic.push(egui::Event::Key { key: k, physical_key: None, pressed: false, repeat: false, modifiers: m });
            if let Some(t) = s("text") {
                app.synthetic.push(egui::Event::Text(t.to_string()));
            }
            ctx.request_repaint();
            ok(Value::Null)
        }
        "ui.move" => {
            let pos = egui::pos2(p.get("x").and_then(Value::as_f64).unwrap_or(0.0) as f32, p.get("y").and_then(Value::as_f64).unwrap_or(0.0) as f32);
            app.synthetic.push(egui::Event::PointerMoved(pos));
            ctx.request_repaint();
            ok(Value::Null)
        }
        "ui.click" | "ui.drag" => {
            // Screen-space (egui points) pointer input through egui itself: reaches every widget.
            let f = |k: &str| p.get(k).and_then(Value::as_f64).unwrap_or(0.0) as f32;
            let button = match s("button") {
                Some("right") | Some("secondary") => egui::PointerButton::Secondary,
                Some("middle") => egui::PointerButton::Middle,
                _ => egui::PointerButton::Primary,
            };
            let b = |n: &str| p.get(n).and_then(Value::as_bool).unwrap_or(false);
            let modifiers = egui::Modifiers {
                alt: b("alt"),
                ctrl: b("ctrl"),
                shift: b("shift"),
                mac_cmd: b("cmd") && cfg!(target_os = "macos"),
                command: b("cmd"),
            };
            let a = egui::pos2(f("x"), f("y"));
            let end = if req.method == "ui.drag" { egui::pos2(f("toX"), f("toY")) } else { a };
            app.synthetic.push(egui::Event::PointerMoved(a));
            app.synthetic.push(egui::Event::PointerButton { pos: a, button, pressed: true, modifiers });
            if req.method == "ui.drag" {
                // From the caller: a huge count would queue events until memory runs out.
                let steps = p.get("steps").and_then(Value::as_u64).unwrap_or(8).clamp(1, 10_000);
                for i in 1..=steps {
                    let t = i as f32 / steps as f32;
                    app.synthetic.push(egui::Event::PointerMoved(a + (end - a) * t));
                }
            }
            app.synthetic.push(egui::Event::PointerButton { pos: end, button, pressed: false, modifiers });
            let count = p.get("count").and_then(Value::as_u64).unwrap_or(1);
            for _ in 1..count {
                app.synthetic.push(egui::Event::PointerButton { pos: end, button, pressed: true, modifiers });
                app.synthetic.push(egui::Event::PointerButton { pos: end, button, pressed: false, modifiers });
            }
            ctx.request_repaint();
            ok(Value::Null)
        }
        "ui.text" => {
            app.synthetic.push(egui::Event::Text(s("text").unwrap_or("").to_string()));
            ctx.request_repaint();
            ok(Value::Null)
        }
        "ui.set" => {
            let mut r = Ok(Value::Null);
            if let Some(b) = s("brightness") {
                r = app.run("window.brightness", json!({"brightness": b}));
            }
            if let Some(pn) = s("panel") {
                r = app.run("window.panel", json!({"panel": pn}));
            }
            if let Some(st) = s("status") {
                app.ui.status = st.to_string();
            }
            for (k, flag) in [
                ("rulers", "view.rulers"),
                ("frameEdges", "view.frameEdges"),
                ("guides", "view.guides"),
                ("baselineGrid", "view.baselineGrid"),
                ("textThreads", "view.textThreads"),
            ] {
                if let Some(want) = p.get(k).and_then(Value::as_bool) {
                    let cur = match k {
                        "rulers" => app.ui.rulers,
                        "frameEdges" => app.ui.frame_edges,
                        "guides" => app.ui.guides,
                        "baselineGrid" => app.ui.baseline_grid,
                        _ => app.ui.text_threads,
                    };
                    if cur != want {
                        r = app.run(flag, json!({}));
                    }
                }
            }
            if let Some(m) = s("screenMode") {
                r = app.run("view.screenMode", json!({"mode": m}));
            }
            if let Some(z) = p.get("zoom").and_then(Value::as_f64) {
                r = app.run("view.zoom", json!({"zoom": z}));
            }
            if let Some(pg) = p.get("page").and_then(Value::as_u64) {
                crate::canvas::go_to_page(app, (pg as usize).saturating_sub(1));
            }
            if let Some(f) = s("fit") {
                r = app.run(if f == "page" { "view.fitPage" } else { "view.fitSpread" }, json!({}));
            }
            if let Some(v) = p.get("controlBar").and_then(Value::as_bool) {
                app.ui.control_bar = v;
            }
            if p.get("closePanel").is_some() {
                app.ui.open_panel = None;
            }
            if let Some(v) = p.get("dockExpanded").and_then(Value::as_bool) {
                app.ui.dock_expanded = v;
            }
            wrap(r)
        }
        "ui.dialog.set" => match app.ui.dialog.as_mut() {
            Some(d) => {
                let Some(f) = s("field") else { return err("missing `field`") };
                d.fields.insert(f.to_string(), p.get("value").cloned().unwrap_or(Value::Null));
                ok(serde_json::to_value(&*d).unwrap_or_default())
            }
            None => err("no dialog open"),
        },
        "ui.dialog.open" => {
            let Some(id) = s("id") else { return err("missing `id`") };
            let d = crate::dialogs::Dialog::new(id, p.get("fields").cloned().unwrap_or(Value::Null));
            let v = serde_json::to_value(&d).unwrap_or_default();
            app.ui.dialog = Some(d);
            ok(v)
        }
        "ui.dialog.confirm" => wrap(crate::dialogs::confirm(app)),
        "ui.dialog.cancel" => {
            app.ui.dialog = None;
            ok(Value::Null)
        }
        "ui.resize" => {
            let w = p.get("width").and_then(Value::as_f64).unwrap_or(1440.0) as f32;
            let h = p.get("height").and_then(Value::as_f64).unwrap_or(900.0) as f32;
            ctx.send_viewport_cmd(egui::ViewportCommand::InnerSize(egui::vec2(w, h)));
            ok(Value::Null)
        }
        "ui.focus" => {
            ctx.send_viewport_cmd(egui::ViewportCommand::Focus);
            ok(Value::Null)
        }
        "ui.screenshot" => {
            if p.get("focus").and_then(Value::as_bool).unwrap_or(true) {
                ctx.send_viewport_cmd(egui::ViewportCommand::Focus);
            }
            ctx.request_repaint();
            Outcome::Screenshot { path: s("path").map(str::to_string) }
        }
        "ui.render" => {
            let Some(st) = app.session.active() else { return err("no document") };
            let page = p.get("page").and_then(Value::as_u64).map(|v| v as usize).or_else(|| crate::canvas::current_page(app)).unwrap_or(0);
            let scale = p.get("scale").and_then(Value::as_f64).unwrap_or(1.0);
            let Some(img) = app.canvas.renderer.render_page(
                &st.doc,
                &app.session.cache,
                page,
                scale,
                p.get("bleed").and_then(Value::as_bool).unwrap_or(false),
                &designcraft_render::RenderOptions { printing_only: true, ..Default::default() },
            ) else {
                return err("no such page");
            };
            let png = img.to_png();
            match s("path") {
                Some(path) => match app.services.write.as_mut() {
                    Some(w) => wrap(w(path, &png).map(|_| json!({"path": path, "width": img.width, "height": img.height}))),
                    None => err("no writer"),
                },
                None => ok(json!({"width": img.width, "height": img.height, "pngBase64": designcraft_engine::cmd::base64_encode(&png)})),
            }
        }
        "app.open" => wrap(app.run("file.open", json!({"path": s("path")}))),
        "app.save" => wrap(app.run("file.save", json!({"path": s("path")}))),
        "app.export" => wrap(app.run("app.exportPng", json!({"path": s("path"), "page": p.get("page"), "scale": p.get("scale")}))),
        "app.quit" => {
            ctx.send_viewport_cmd(egui::ViewportCommand::Close);
            ok(Value::Null)
        }
        other => err(format!("unknown method `{other}`")),
    }
}

pub fn save_screenshot(app: &mut DesignApp, image: &egui::ColorImage, path: Option<&str>) -> Value {
    let [w, h] = image.size;
    let Some(path) = path else {
        return json!({"ok": true, "result": {"width": w, "height": h}});
    };
    let rgba: Vec<u8> = image.pixels.iter().flat_map(|c| c.to_array()).collect();
    let img = designcraft_render::Rendered { width: w as u32, height: h as u32, pixels: rgba };
    let png = img.to_png();
    match app.services.write.as_mut() {
        Some(wr) => match wr(path, &png) {
            Ok(()) => json!({"ok": true, "result": {"path": path, "width": w, "height": h}}),
            Err(e) => json!({"ok": false, "error": e}),
        },
        None => json!({"ok": false, "error": "no writer configured"}),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    };

    #[test]
    fn control_paste_never_reads_the_system_clipboard() {
        let calls = Arc::new(AtomicUsize::new(0));
        let seen = calls.clone();
        let services = crate::Services {
            clipboard_text: Some(Box::new(move || {
                seen.fetch_add(1, Ordering::SeqCst);
                Some("SECRET".into())
            })),
            ..Default::default()
        };
        let mut app = DesignApp::new(designcraft_engine::Session::new(), services);
        app.run("file.new", json!({})).unwrap();
        let r = app.run("frame.create", json!({"rect": [72, 72, 300, 200], "content": "text", "text": "words"})).unwrap();
        let sid = r["story"].as_u64().unwrap();
        assert!(app.session.active().unwrap().selection.text.is_some(), "the new frame is being typed in");
        let text = |app: &DesignApp| app.session.active().unwrap().doc.story(designcraft_doc::StoryId(sid)).unwrap().text.clone();
        let ctx = egui::Context::default();
        let (req, _rx) = ControlRequest::new("engine.execute", json!({"command": "edit.paste"}));
        let _ = handle(&mut app, &ctx, &req);
        assert_eq!(calls.load(Ordering::SeqCst), 0, "the control channel must not read the system clipboard");
        assert!(!text(&app).contains("SECRET"));
        assert!(!app.in_control);
        // The same command chosen by the user does paste the system clipboard.
        let _ = app.run("edit.paste", json!({}));
        assert_eq!(calls.load(Ordering::SeqCst), 1);
        assert!(text(&app).contains("SECRET"));
    }
}
