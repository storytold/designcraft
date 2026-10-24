//! Programmatic control of the running app (agents, tests, MCP).
//!
//! Methods (JSON-lines over the host's transport):
//! - `engine.execute {command, params}` / `ui.menu.invoke {command|id, params}`: run any command
//! - `engine.commands`: engine + UI commands with enablement
//! - `document.inspect`: document summary; `ui.inspect`: UI state
//! - `ui.docking`: the panel layout in use and the focused panel
//! - `ui.menu.list`: flattened menu tree
//! - `ui.tool.select {tool}`, `ui.tool.list`
//! - `ui.pointer {events:[{kind: down|drag|up|move|doubleclick, x, y, space?: "doc"|"screen"}], mods?}`:
//!   drive the active tool through the same path as the mouse
//! - `ui.key {key, shift?, alt?, cmd?}`: synthetic keyboard input
//! - `ui.text {text}`: insert at the explicit caret, or type into a focused UI field
//! - `ui.ime {preedit?, active?: [start, end], commit?}`: input method events, as the system IME
//!   sends them (marked text with its converting clause in characters, then the committed text)
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
        "docking": crate::panel_docking::inspect(app, ctx),
        "window": [r.width(), r.height()],
        "documents": app
            .session
            .documents()
            .iter()
            .map(|d| {
                let mut v = json!({"title": d.title(), "dirty": d.is_dirty()});
                if app.session.new_document_preview() == Some(d.uid) {
                    v["preview"] = json!(true);
                }
                v
            })
            .collect::<Vec<_>>(),
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

// Retain room for the existing maximum drag (10,000 moves plus move/down/up), but
// bound the whole gesture and already queued input before generating any events.
const MAX_PENDING_POINTER_EVENTS: usize = 10_003;

fn pointer_counts(p: &Value, drag: bool, pending: usize) -> Result<(u64, u64), String> {
    let positive = |name: &str, default: u64| match p.get(name) {
        None => Ok(default),
        Some(v) => v.as_u64().filter(|n| *n > 0).ok_or_else(|| format!("`{name}` must be a positive integer")),
    };
    let count = positive("count", 1)?;
    if count > 255 {
        return Err("`count` must be an integer from 1 through 255".into());
    }
    let steps = if drag { positive("steps", 8)? } else { 0 };
    // One initial move, two button events per click, and each drag step. Check the
    // arithmetic too: hostile counts must never overflow into a small budget.
    let events = count.checked_mul(2).and_then(|n| n.checked_add(steps)).and_then(|n| n.checked_add(1));
    let available = MAX_PENDING_POINTER_EVENTS.saturating_sub(pending) as u64;
    if events.is_none_or(|n| n > available) {
        return Err(format!("pointer input would exceed the {MAX_PENDING_POINTER_EVENTS} pending-event limit"));
    }
    Ok((count, steps))
}

/// Handle one control request. Commands it runs leave the user's system clipboard alone.
pub fn handle(app: &mut DesignApp, ctx: &egui::Context, req: &ControlRequest) -> Outcome {
    app.install_clipboard_guard(ctx);
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
            // A key the command's description doesn't name is an error, not a default (#776).
            if let Err(e) = designcraft_engine::check_params(id, &params) {
                return err(e.to_string());
            }
            wrap(app.run(id, params))
        }
        "engine.commands" => ok(all_commands(app)),
        "document.inspect" => wrap(app.run("document.inspect", json!({}))),
        "ui.inspect" => ok(inspect(app, ctx)),
        "ui.docking" => ok(crate::panel_docking::inspect(app, ctx)),
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
            app.control_synthetic.push(egui::Event::Key { key: k, physical_key: None, pressed: true, repeat: false, modifiers: m });
            app.control_synthetic.push(egui::Event::Key { key: k, physical_key: None, pressed: false, repeat: false, modifiers: m });
            if let Some(t) = s("text") {
                app.control_synthetic.push(egui::Event::Text(t.to_string()));
            }
            ctx.request_repaint();
            ok(Value::Null)
        }
        // What Ctrl/Cmd+V delivers from the system clipboard: text, or SVG markup copied in an
        // illustration app (pasted as editable paths when no text is being edited).
        "ui.paste" => {
            let Some(text) = s("text") else { return err("missing `text`") };
            app.control_synthetic.push(egui::Event::Paste(text.to_string()));
            ctx.request_repaint();
            ok(Value::Null)
        }
        "ui.move" => {
            let pos = egui::pos2(p.get("x").and_then(Value::as_f64).unwrap_or(0.0) as f32, p.get("y").and_then(Value::as_f64).unwrap_or(0.0) as f32);
            app.control_synthetic.push(egui::Event::PointerMoved(pos));
            ctx.request_repaint();
            ok(Value::Null)
        }
        "ui.click" | "ui.drag" => {
            // Screen-space (egui points) pointer input through egui itself: reaches every widget.
            let (count, steps) = match pointer_counts(p, req.method == "ui.drag", app.control_synthetic.len().saturating_add(app.synthetic.len())) {
                Ok(counts) => counts,
                Err(e) => return err(e),
            };
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
            app.control_synthetic.push(egui::Event::PointerMoved(a));
            app.control_synthetic.push(egui::Event::PointerButton { pos: a, button, pressed: true, modifiers });
            if req.method == "ui.drag" {
                for i in 1..=steps {
                    let t = i as f32 / steps as f32;
                    app.control_synthetic.push(egui::Event::PointerMoved(a + (end - a) * t));
                }
            }
            app.control_synthetic.push(egui::Event::PointerButton { pos: end, button, pressed: false, modifiers });
            for _ in 1..count {
                app.control_synthetic.push(egui::Event::PointerButton { pos: end, button, pressed: true, modifiers });
                app.control_synthetic.push(egui::Event::PointerButton { pos: end, button, pressed: false, modifiers });
            }
            ctx.request_repaint();
            ok(Value::Null)
        }
        "ui.text" => {
            let Some(text) = s("text") else { return err("missing `text`") };
            // Focused widgets retain the synthetic keyboard route. Document automation uses
            // the explicit caret independently of the active tool, unlike ordinary typing.
            if ctx.text_edit_focused() || app.ui.dialog.is_some() || app.ui.palette.is_some() {
                app.control_synthetic.push(egui::Event::Text(text.to_string()));
                ctx.request_repaint();
                ok(Value::Null)
            } else {
                if app.session.active().is_none_or(|st| st.selection.text.is_none_or(|t| st.doc.text_story(t.story, t.cell).is_none())) {
                    return err("no valid text insertion point: use text.select or text.placeCaret first");
                }
                wrap(app.run("text.insert", json!({"text": text})))
            }
        }
        "ui.ime" => {
            let n = |v: &Value| v.as_u64().and_then(|n| usize::try_from(n).ok());
            if let Some(text) = s("preedit") {
                let active = p.get("active").and_then(Value::as_array).and_then(|a| Some(n(a.first()?)?..n(a.get(1)?)?));
                app.control_synthetic.push(egui::Event::Ime(egui::ImeEvent::Preedit { text: text.to_string(), active_range_chars: active }));
            }
            if let Some(text) = s("commit") {
                app.control_synthetic.push(egui::Event::Ime(egui::ImeEvent::Commit(text.to_string())));
            }
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
                ("documentGrid", "view.documentGrid"),
                ("textThreads", "view.textThreads"),
            ] {
                if let Some(want) = p.get(k).and_then(Value::as_bool) {
                    let cur = match k {
                        "rulers" => app.ui.rulers,
                        "frameEdges" => app.ui.frame_edges,
                        "guides" => app.ui.guides,
                        "baselineGrid" => app.ui.baseline_grid,
                        "documentGrid" => app.ui.document_grid,
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
                r = crate::panel_docking::close_visible(app).map(|_| Value::Null);
            }
            if let Some(v) = p.get("dockExpanded").and_then(Value::as_bool) {
                r = crate::panel_docking::set_iconic(app, !v);
            }
            wrap(r)
        }
        "ui.dialog.set" => match app.ui.dialog.as_mut() {
            Some(d) => {
                let Some(f) = s("field") else { return err("missing `field`") };
                let value = p.get("value").cloned().unwrap_or(Value::Null);
                if d.set_field(f, value) {
                    app.session.reset_find_scope();
                }
                ok(serde_json::to_value(&*d).unwrap_or_default())
            }
            None => err("no dialog open"),
        },
        "ui.dialog.open" => {
            let Some(id) = s("id") else { return err("missing `id`") };
            // Replacing a modal cancels its transaction just like its Cancel button.
            // In particular, an uncommitted style preview must not leak into the new dialog.
            crate::dialogs::cancel(app);
            if id == "findChange" || app.ui.dialog.as_ref().is_some_and(|d| d.id == "findChange") {
                app.session.reset_find_scope();
            }
            let d = crate::dialogs::Dialog::new(id, p.get("fields").cloned().unwrap_or(Value::Null));
            let v = serde_json::to_value(&d).unwrap_or_default();
            app.ui.dialog = Some(d);
            ok(v)
        }
        "ui.dialog.confirm" => wrap(crate::dialogs::confirm(app)),
        "ui.dialog.cancel" => {
            crate::dialogs::cancel(app);
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
            let page = match page_index(p, crate::canvas::current_page(app).unwrap_or(0), st.doc.page_count()) {
                Ok(page) => page,
                Err(e) => return err(e),
            };
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
        "app.export" => wrap(app.run("app.exportPng", p.clone())),
        "app.quit" => {
            // Automation must not wait on a prompt: quit without asking about unsaved changes.
            app.quit_without_asking = true;
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

/// Parse explicit page indices without treating invalid values as omission.
pub(crate) fn page_index(p: &Value, default: usize, count: usize) -> Result<usize, String> {
    let page = match p.get("page") {
        None => default,
        Some(v) => v.as_u64().and_then(|n| usize::try_from(n).ok()).ok_or("`page` must be a nonnegative integer representable as a page index")?,
    };
    if page >= count {
        return Err(format!("no page {page} (the document has {count} pages, indices are 0-based)"));
    }
    Ok(page)
}

#[cfg(test)]
mod automation_contract_tests {
    use super::*;

    #[test]
    fn replacing_a_style_preview_dialog_restores_the_document_and_undo_recording() {
        let mut app = app();
        let ctx = egui::Context::default();
        let before = app.session.doc().unwrap().doc.clone();
        let history = app.session.doc().unwrap().history.undo.len();
        app.session.begin_interaction("Style Options").unwrap();
        app.session.preview("style.paragraph.edit", json!({"name":designcraft_doc::BASIC_PARAGRAPH,"chars":{"size":24}})).unwrap();
        app.ui.dialog = Some(crate::dialogs::Dialog::new(
            "paragraphStyleOptions",
            json!({
                "name":designcraft_doc::BASIC_PARAGRAPH,"preview":true,"_previewed":{"chars":{"size":24}}
            }),
        ));
        assert_ne!(app.session.doc().unwrap().doc, before);
        let response = call(&mut app, &ctx, "ui.dialog.open", json!({"id":"characterStyleOptions","fields":{"new":true}}));
        assert_eq!(response["ok"], true);
        assert_eq!(app.session.doc().unwrap().doc, before);
        assert!(app.session.doc().unwrap().interaction.is_none());
        assert_eq!(app.session.doc().unwrap().history.undo.len(), history);
        app.run("style.paragraph.edit", json!({"name":designcraft_doc::BASIC_PARAGRAPH,"chars":{"size":18}})).unwrap();
        assert_eq!(app.session.doc().unwrap().history.undo.len(), history + 1);
    }
    fn app() -> DesignApp {
        let mut app = DesignApp::new(designcraft_engine::Session::new(), crate::Services::default());
        app.session.execute("file.new", &json!({"pages":2})).unwrap();
        app
    }
    fn call(app: &mut DesignApp, ctx: &egui::Context, method: &str, params: Value) -> Value {
        let (req, _) = ControlRequest::new(method, params);
        match handle(app, ctx, &req) {
            Outcome::Done(v) => v,
            Outcome::Screenshot { .. } => panic!("unexpected screenshot"),
        }
    }
    fn frame(app: &mut DesignApp, ctx: &egui::Context, events: Vec<egui::Event>) {
        let mut output = ctx.run_ui(
            egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(1400.0, 900.0))),
                events,
                ..Default::default()
            },
            |ui| {
                app.logic(ui.ctx());
                app.ui(ui);
            },
        );
        output.textures_delta.clear();
    }
    #[test]
    fn oversized_pointer_requests_leave_pending_events_unchanged() {
        let mut app = app();
        let ctx = egui::Context::default();
        let pending = vec![egui::Event::PointerMoved(egui::pos2(12.0, 34.0))];
        for (method, params) in [
            ("ui.click", json!({"count": 5002})),
            ("ui.drag", json!({"steps": 10000, "count": 2})),
            ("ui.drag", json!({"steps": 10001})),
            ("ui.click", json!({"count": u64::MAX})),
            ("ui.drag", json!({"steps": u64::MAX})),
            ("ui.drag", json!({"count": u64::MAX, "steps": u64::MAX})),
        ] {
            app.control_synthetic = pending.clone();
            let reply = call(&mut app, &ctx, method, params);
            assert_eq!(reply["ok"], false, "{method}: {reply}");
            assert_eq!(app.control_synthetic, pending, "a rejected gesture must not leave a press queued");
        }
    }
    #[test]
    fn pointer_counts_require_positive_json_integers() {
        let mut app = app();
        let ctx = egui::Context::default();
        let pending = vec![egui::Event::PointerMoved(egui::pos2(12.0, 34.0))];
        app.control_synthetic = pending.clone();
        for value in [json!(0), json!(-1), json!(1.5), Value::Null, json!("2"), json!(true), json!(18446744073709551616_f64)] {
            for (method, field) in [("ui.click", "count"), ("ui.drag", "count"), ("ui.drag", "steps")] {
                let mut params = json!({});
                params[field] = value.clone();
                let reply = call(&mut app, &ctx, method, params);
                assert_eq!(reply["ok"], false, "{method}, {field}={value}: {reply}");
                assert!(reply["error"].as_str().unwrap().contains(field));
                assert_eq!(app.control_synthetic, pending);
            }
        }
    }
    #[test]
    fn pointer_gestures_accept_the_exact_event_budget() {
        let mut app = app();
        let ctx = egui::Context::default();
        for (method, params) in [
            ("ui.click", json!({"count": 255})),
            ("ui.drag", json!({"steps": 9492, "count": 255})),
            ("ui.drag", json!({"steps": 10000})),
            ("ui.drag", json!({"steps": 9998, "count": 2})),
        ] {
            // A full click count fits on top of 9,492 pending events.
            let pending = if method == "ui.click" { 10_003 - 511 } else { 0 };
            app.control_synthetic = vec![egui::Event::PointerMoved(egui::pos2(12.0, 34.0)); pending];
            assert_eq!(call(&mut app, &ctx, method, params)["ok"], true, "{method}");
            assert_eq!(app.control_synthetic.len(), 10003);
            assert!(matches!(app.control_synthetic.last(), Some(egui::Event::PointerButton { pressed: false, .. })));
            let queued = app.control_synthetic.clone();
            assert_eq!(call(&mut app, &ctx, "ui.click", json!({}))["ok"], false);
            assert_eq!(app.control_synthetic, queued, "a full queue retains its release events");
        }
    }
    #[test]
    fn pointer_budget_counts_pending_input_and_recovers_when_it_drains() {
        let mut app = app();
        let ctx = egui::Context::default();
        assert_eq!(call(&mut app, &ctx, "ui.drag", json!({"steps": 9998}))["ok"], true);
        assert_eq!(app.control_synthetic.len(), 10001);
        let queued = app.control_synthetic.clone();
        assert_eq!(call(&mut app, &ctx, "ui.click", json!({}))["ok"], false);
        assert_eq!(app.control_synthetic, queued);
        // The ordinary input hook consumes one pointer event per frame, freeing
        // enough room for a complete click without dropping earlier input.
        app.raw_input_hook(&mut egui::RawInput::default());
        assert_eq!(app.control_synthetic.len(), 10000);
        assert_eq!(call(&mut app, &ctx, "ui.click", json!({}))["ok"], true);
        assert_eq!(app.control_synthetic.len(), 10003);

        app.control_synthetic.clear();
        assert_eq!(call(&mut app, &ctx, "ui.move", json!({"x": 12, "y": 34}))["ok"], true);
        let queued = app.control_synthetic.clone();
        assert_eq!(call(&mut app, &ctx, "ui.drag", json!({"steps": 10000}))["ok"], false);
        assert_eq!(app.control_synthetic, queued);
        assert_eq!(call(&mut app, &ctx, "ui.drag", json!({"steps": 9999}))["ok"], true);
        assert_eq!(app.control_synthetic.len(), 10003);
    }
    #[test]
    fn ordinary_pointer_gestures_keep_their_defaults_and_event_order() {
        let mut app = app();
        let ctx = egui::Context::default();
        let a = egui::pos2(10.0, 20.0);
        let end = egui::pos2(30.0, 40.0);
        assert_eq!(call(&mut app, &ctx, "ui.click", json!({"x": 10, "y": 20}))["ok"], true);
        assert_eq!(
            app.control_synthetic,
            vec![
                egui::Event::PointerMoved(a),
                egui::Event::PointerButton { pos: a, button: egui::PointerButton::Primary, pressed: true, modifiers: egui::Modifiers::NONE },
                egui::Event::PointerButton { pos: a, button: egui::PointerButton::Primary, pressed: false, modifiers: egui::Modifiers::NONE },
            ]
        );
        app.control_synthetic.clear();
        assert_eq!(call(&mut app, &ctx, "ui.drag", json!({"x": 10, "y": 20, "toX": 30, "toY": 40}))["ok"], true);
        assert_eq!(app.control_synthetic.len(), 11, "default drag has eight steps");
        assert_eq!(app.control_synthetic.get(9), Some(&egui::Event::PointerMoved(end)));

        app.control_synthetic.clear();
        assert_eq!(
            call(
                &mut app,
                &ctx,
                "ui.drag",
                json!({"x": 10, "y": 20, "toX": 30, "toY": 40, "steps": 2, "count": 2, "button": "right", "shift": true})
            )["ok"],
            true
        );
        let modifiers = egui::Modifiers { shift: true, ..Default::default() };
        let button = egui::PointerButton::Secondary;
        assert_eq!(
            app.control_synthetic,
            vec![
                egui::Event::PointerMoved(a),
                egui::Event::PointerButton { pos: a, button, pressed: true, modifiers },
                egui::Event::PointerMoved(egui::pos2(20.0, 30.0)),
                egui::Event::PointerMoved(end),
                egui::Event::PointerButton { pos: end, button, pressed: false, modifiers },
                egui::Event::PointerButton { pos: end, button, pressed: true, modifiers },
                egui::Event::PointerButton { pos: end, button, pressed: false, modifiers },
            ]
        );
    }
    #[test]
    fn control_typing_uses_explicit_caret_but_retains_widget_events() {
        let mut app = app();
        let ctx = egui::Context::default();
        assert_eq!(call(&mut app, &ctx, "ui.text", json!({"text":"X"}))["ok"], false);
        let f = app.session.execute("frame.create", &json!({"rect":[36,36,300,200],"content":"text","text":"Hello"})).unwrap();
        app.session.execute("text.select", &json!({"story":f["story"],"anchor":5})).unwrap();
        assert_eq!(app.session.tool_id(), "selection");
        assert_eq!(call(&mut app, &ctx, "ui.text", json!({"text":"X"}))["ok"], true);
        assert_eq!(app.session.execute("story.get", &json!({"story":f["story"]})).unwrap()["text"], "HelloX");
        app.session.execute("edit.undo", &json!({})).unwrap();
        assert_eq!(app.session.execute("story.get", &json!({"story":f["story"]})).unwrap()["text"], "Hello");
        app.select_tool("type");
        assert!(app.session.wants_text(), "the document would receive ordinary typing without widget focus");
        app.ui.palette = Some(String::new());
        for _ in 0..3 {
            frame(&mut app, &ctx, vec![]);
        }
        assert!(ctx.text_edit_focused());
        assert_eq!(call(&mut app, &ctx, "ui.text", json!({"text":"field"}))["ok"], true);
        let events = std::mem::take(&mut app.control_synthetic);
        frame(&mut app, &ctx, events);
        assert_eq!(app.ui.palette.as_deref(), Some("field"));
        assert_eq!(app.session.execute("story.get", &json!({"story":f["story"]})).unwrap()["text"], "Hello");
    }
    #[test]
    fn control_pages_validate_before_writer_and_preserve_omission() {
        let mut app = app();
        let ctx = egui::Context::default();
        let writes = std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
        let sink = writes.clone();
        app.services.write = Some(Box::new(move |_, bytes| {
            sink.lock().unwrap().push(bytes.to_vec());
            Ok(())
        }));
        app.session.execute("frame.create", &json!({"rect":[36,36,300,200]})).unwrap();
        app.canvas_rect = Some(egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(1400.0, 900.0)));
        crate::canvas::go_to_page(&mut app, 1);
        assert_eq!(crate::canvas::current_page(&app), Some(1));
        for method in ["ui.render", "app.export"] {
            for page in
                [json!(-1), json!(-2), json!(1.5), Value::Null, json!("0"), json!(true), json!(u64::MAX), json!(18446744073709551616_f64), json!(99)]
            {
                assert_eq!(call(&mut app, &ctx, method, json!({"page":page,"path":"unchanged.png","scale":0.1}))["ok"], false, "{method}: {page}");
                assert!(writes.lock().unwrap().is_empty());
            }
            assert_eq!(call(&mut app, &ctx, method, json!({"path":"omitted.png","scale":0.1}))["ok"], true);
            assert_eq!(call(&mut app, &ctx, method, json!({"page":1,"path":"explicit.png","scale":0.1}))["ok"], true);
            let mut bytes = writes.lock().unwrap();
            assert_eq!(bytes.len(), 2);
            assert_eq!(bytes[0], bytes[1]);
            bytes.clear();
        }
        assert!(app.run("app.exportPng", json!({"page":Value::Null,"path":"unchanged.png"})).is_err());
        assert!(writes.lock().unwrap().is_empty());
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

#[cfg(test)]
mod deferred_clipboard_tests {
    use super::*;
    use std::sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    };

    #[test]
    fn synthetic_paste_keeps_mock_clipboard_private() {
        let reads = Arc::new(AtomicUsize::new(0));
        let observed = reads.clone();
        let mut app = DesignApp::new(
            designcraft_engine::Session::new(),
            crate::Services {
                clipboard_text: Some(Box::new(move || {
                    observed.fetch_add(1, Ordering::SeqCst);
                    Some("SYNTHETIC_CLIPBOARD_SENTINEL".to_string())
                })),
                ..Default::default()
            },
        );
        app.run("file.new", json!({})).unwrap();
        let frame = app.run("frame.create", json!({"content":"text", "text":"Original", "rect":[72,72,200,100], "caret":false})).unwrap();
        app.run("selection.set", json!({"ids":[frame["id"]]})).unwrap();
        assert!(!app.session.wants_text());
        let ctx = egui::Context::default();
        let (req, _) = ControlRequest::new("ui.key", json!({"key":"V", "cmd":true}));
        assert!(matches!(handle(&mut app, &ctx, &req), Outcome::Done(v) if v["ok"]==true));
        assert!(!app.in_control);
        for step in 0..3 {
            let mut raw = egui::RawInput { time: Some(step as f64 * 0.05), ..Default::default() };
            app.raw_input_hook(&mut raw);
            ctx.run_ui(raw, |ui| menus::shortcuts(&mut app, ui.ctx())).textures_delta.clear();
        }
        let texts: Vec<String> = app.session.active().unwrap().doc.stories.values().map(|story| story.text.clone()).collect();
        assert_eq!(reads.load(Ordering::SeqCst), 0, "synthetic command read mocked system clipboard; stories={texts:?}");
        assert!(!texts.iter().any(|text| text.contains("SYNTHETIC_CLIPBOARD_SENTINEL")));
        // Once control input is drained, an ordinary user paste still reads its clipboard.
        let mut raw = egui::RawInput::default();
        app.raw_input_hook(&mut raw);
        app.run("edit.paste", json!({})).unwrap();
        assert_eq!(reads.load(Ordering::SeqCst), 1);
    }
    #[test]
    fn deferred_control_widget_copy_leaves_native_clipboard_output_alone() {
        let mut app = DesignApp::new(designcraft_engine::Session::new(), crate::Services::default());
        let ctx = egui::Context::default();
        let (req, _) = ControlRequest::new("ui.move", json!({"x":10,"y":10}));
        let _ = handle(&mut app, &ctx, &req);
        let mut raw = egui::RawInput::default();
        app.raw_input_hook(&mut raw);
        let mut private = ctx.run_ui(raw, |ui| ui.ctx().copy_text("control field".into()));
        assert!(!private.platform_output.commands.iter().any(|cmd| matches!(cmd, egui::OutputCommand::CopyText(_))));
        private.textures_delta.clear();
        let mut raw = egui::RawInput::default();
        app.raw_input_hook(&mut raw);
        let mut native = ctx.run_ui(raw, |ui| ui.ctx().copy_text("native field".into()));
        assert!(native.platform_output.commands.iter().any(|cmd| matches!(cmd, egui::OutputCommand::CopyText(text) if text=="native field")));
        native.textures_delta.clear();
    }

    #[test]
    fn control_click_rejects_unbounded_counts_before_queuing_input() {
        let mut app = DesignApp::new(designcraft_engine::Session::new(), crate::Services::default());
        let ctx = egui::Context::default();
        for count in [json!(u64::MAX), json!(256), json!(0), json!(-1), json!(1.5)] {
            let (req, _) = ControlRequest::new("ui.click", json!({"x":10,"y":10,"count":count}));
            assert!(matches!(handle(&mut app, &ctx, &req), Outcome::Done(v) if v["ok"]==false));
            assert!(app.control_synthetic.is_empty());
            assert!(app.synthetic.is_empty());
            assert!(!app.in_control);
        }
    }

    #[test]
    fn control_ime_and_paste_keep_their_order_with_control_keys() {
        let mut app = DesignApp::new(designcraft_engine::Session::new(), crate::Services::default());
        let ctx = egui::Context::default();
        for (method, params) in [("ui.ime", json!({"preedit":"ni"})), ("ui.key", json!({"key":"A"})), ("ui.paste", json!({"text":"p"}))] {
            let (req, _) = ControlRequest::new(method, params);
            assert!(matches!(handle(&mut app, &ctx, &req), Outcome::Done(v) if v["ok"]==true), "{method}");
        }
        assert!(app.synthetic.is_empty(), "native menu events stay in their own queue");
        let kinds: Vec<_> = app
            .control_synthetic
            .iter()
            .map(|e| match e {
                egui::Event::Ime(_) => "ime",
                egui::Event::Key { .. } => "key",
                egui::Event::Paste(_) => "paste",
                _ => "other",
            })
            .collect();
        assert_eq!(kinds, ["ime", "key", "key", "paste"]);
    }
}
