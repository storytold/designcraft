//! The `postMessage` bridge to a same-origin parent page (`?host=parent`), for pages that embed
//! DesignCraft in an `<iframe>` (such as a file manager). It mirrors the other craft apps:
//!
//! - `designcraft:ready` (to the parent): `{ version }`, the app is listening; send a file now.
//! - `designcraft:open` (from the parent): `{ name, bytes }`, bytes an `ArrayBuffer` or
//!   `Uint8Array`. It opens like a file chosen with File › Open (`.designcraft`, `.idml`…).
//! - `designcraft:save` (to the parent): `{ name, bytes }` (a transferred `ArrayBuffer`) for every
//!   file DesignCraft would otherwise download. Save and Save As send the `.designcraft` document
//!   and mark it saved; exports (PDF, PNG, IDML…) add `export: true`. Each carries an `id`.
//! - `designcraft:saved` (from the parent): `{ id }`, that save is stored. Until it arrives a
//!   document save keeps the app reported dirty; after 30 s without it the app posts
//!   `designcraft:failed { id, error }` and stays dirty.
//! - `designcraft:dirty` (to the parent): `{ dirty }` whenever unsaved changes appear or go away
//!   in any open document.
//! - `designcraft:failed` (to the parent): `{ error }` when the app couldn't start, or when an
//!   opened file is larger than [`MAX_OPEN_BYTES`] (it isn't copied or opened), or a save went
//!   unconfirmed.
//!
//! The bridge is installed only for a same-origin parent, or for a parent on the origin named by
//! `?host_origin=` (a bare `scheme://host[:port]` origin); messages go to that exact origin and only
//! the parent window on it is heard. Any other parent gets no bridge (saves download as usual). In host mode `?sample` is ignored.
//! DesignCraft has no user name to set, so `?author=` is ignored.

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use designcraft_ui_egui::{ImportRequest, ImportedFile, Inbox, Services};
use wasm_bindgen::JsCast as _;
use wasm_bindgen::JsValue;
use wasm_bindgen::closure::Closure;

use crate::bridge::SaveAcks;

/// Largest file the parent may send with `designcraft:open`: the same 256 MiB the engine allows
/// for the other files it reads (linked files, data-merge data and images).
const MAX_OPEN_BYTES: u64 = 256 * 1024 * 1024;

#[derive(Clone)]
pub struct Bridge {
    window: web_sys::Window,
    parent: web_sys::Window,
    origin: String,
    /// The unsaved-changes state last reported to the parent.
    reported: Rc<Cell<Option<bool>>>,
    /// Saves waiting for `designcraft:saved`.
    acks: Rc<RefCell<SaveAcks>>,
}

impl Bridge {
    /// `None` when the page isn't framed, or the parent is cross-origin and not the origin
    /// `host_origin` allows (see [`crate::bridge::target_origin`]).
    pub fn new(host_origin: Option<&str>) -> Option<Self> {
        let window = web_sys::window()?;
        let parent = window.parent().ok().flatten()?;
        if js_sys::Object::is(&parent, &window) {
            return None;
        }
        let own = window.location().origin().ok()?;
        // Reading another origin's location throws: treat that as cross-origin.
        let parent_origin = parent.location().origin().ok();
        let origin = crate::bridge::target_origin(&own, parent_origin.as_deref(), host_origin)?;
        Some(Self { window, parent, origin, reported: Rc::default(), acks: Rc::default() })
    }

    /// Send saves and exports to the parent instead of downloading them, and hear files from it.
    pub fn connect(&self, services: &mut Services, inbox: Inbox, ctx: egui::Context) {
        self.listen(inbox, ctx);
        let bridge = self.clone();
        services.download = Some(Box::new(move |name: &str, bytes: &[u8]| bridge.post_save(name, bytes, false)));
        let bridge = self.clone();
        services.write = Some(Box::new(move |name: &str, bytes: &[u8]| bridge.post_save(name, bytes, true)));
    }

    pub fn post_ready(&self) {
        self.send(&[("type", "designcraft:ready".into()), ("version", env!("CARGO_PKG_VERSION").into())]);
    }

    pub fn post_failed(&self, error: &str) {
        self.send(&[("type", "designcraft:failed".into()), ("error", error.into())]);
    }

    /// Report unsaved changes (once a frame), held dirty while a document save is unconfirmed,
    /// and fail saves the parent didn't confirm within 30 s.
    pub fn post_dirty(&self, ctx: &egui::Context, engine_dirty: bool) {
        let expired = self.acks.borrow_mut().expire(js_sys::Date::now());
        for (id, name) in expired {
            self.send(&[
                ("type", "designcraft:failed".into()),
                ("id", JsValue::from_f64(id as f64)),
                ("error", format!("{name}: the host page didn't confirm the save").into()),
            ]);
        }
        let (dirty, waiting) = {
            let acks = self.acks.borrow();
            (acks.dirty(engine_dirty), acks.waiting())
        };
        if waiting {
            ctx.request_repaint_after(std::time::Duration::from_secs(1));
        }
        if self.reported.get() != Some(dirty) && self.send(&[("type", "designcraft:dirty".into()), ("dirty", dirty.into())]) {
            self.reported.set(Some(dirty));
        }
    }

    fn post_save(&self, path: &str, bytes: &[u8], export: bool) -> Result<(), String> {
        let name = std::path::Path::new(path).file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_else(|| path.to_string());
        let buffer = js_sys::Uint8Array::from(bytes).buffer();
        let document = !export && name.ends_with(&format!(".{}", designcraft_format::EXTENSION));
        let id = self.acks.borrow_mut().posted(&name, document, js_sys::Date::now());
        let mut fields = vec![
            ("type", "designcraft:save".into()),
            ("id", JsValue::from_f64(id as f64)),
            ("name", name.as_str().into()),
            ("bytes", JsValue::from(buffer.clone())),
        ];
        if export {
            fields.push(("export", true.into()));
        }
        let sent = message(&fields).ok_or_else(|| "couldn't build the message".to_string()).and_then(|m| {
            self.parent
                .post_message_with_transfer(&m, &self.origin, &js_sys::Array::of1(&buffer))
                .map_err(|e| e.as_string().unwrap_or_else(|| format!("{e:?}")))
        });
        if sent.is_err() {
            self.acks.borrow_mut().failed(id);
        }
        sent
    }

    fn listen(&self, inbox: Inbox, ctx: egui::Context) {
        let bridge = self.clone();
        let parent = self.parent.clone();
        let origin = self.origin.clone();
        let on_message = Closure::<dyn FnMut(web_sys::MessageEvent)>::new(move |event: web_sys::MessageEvent| {
            if event.origin() != origin || !event.source().is_some_and(|s| js_sys::Object::is(&s, &parent)) {
                return;
            }
            let data = event.data();
            match string(&data, "type").as_deref() {
                Some("designcraft:open") => {}
                Some("designcraft:saved") => {
                    let id = js_sys::Reflect::get(&data, &JsValue::from_str("id")).ok().and_then(|v| v.as_f64());
                    if let Some(id) = id.filter(|id| id.is_finite() && *id >= 0.0 && id.fract() == 0.0) {
                        bridge.acks.borrow_mut().saved(id as u64);
                        ctx.request_repaint();
                    }
                    return;
                }
                _ => return,
            }
            let name = string(&data, "name").filter(|n| !n.trim().is_empty()).unwrap_or_else(|| "Untitled.designcraft".to_string());
            let Some(array) = byte_array(&data) else { return };
            // Refuse oversize files before copying them out of the parent's buffer.
            if u64::from(array.byte_length()) > MAX_OPEN_BYTES {
                bridge.post_failed(&format!("{name} is {} bytes, larger than the {MAX_OPEN_BYTES}-byte limit for opened files", array.byte_length()));
                return;
            }
            let bytes = array.to_vec();
            let request = ImportRequest { purpose: "open".into(), target: None };
            inbox.lock().unwrap_or_else(|e| e.into_inner()).push(ImportedFile { request, name, bytes });
            ctx.request_repaint();
        });
        if self.window.add_event_listener_with_callback("message", on_message.as_ref().unchecked_ref()).is_ok() {
            // The listener lives as long as the page.
            on_message.forget();
        }
    }

    /// Post a message to the parent; false if it couldn't be sent.
    fn send(&self, fields: &[(&str, JsValue)]) -> bool {
        message(fields).is_some_and(|m| self.parent.post_message(&m, &self.origin).is_ok())
    }
}

/// The value of `key` in the page's query string.
pub fn query_param(query: &str, key: &str) -> Option<String> {
    let query = query.strip_prefix('?').unwrap_or(query);
    form_urlencoded::parse(query.as_bytes()).find(|(k, _)| k == key).map(|(_, v)| v.into_owned())
}

fn message(fields: &[(&str, JsValue)]) -> Option<js_sys::Object> {
    let m = js_sys::Object::new();
    for (key, value) in fields {
        js_sys::Reflect::set(&m, &JsValue::from_str(key), value).ok()?;
    }
    Some(m)
}

fn string(data: &JsValue, key: &str) -> Option<String> {
    js_sys::Reflect::get(data, &JsValue::from_str(key)).ok()?.as_string()
}

/// `bytes` (an `ArrayBuffer` or `Uint8Array`) as a view, not yet copied.
fn byte_array(data: &JsValue) -> Option<js_sys::Uint8Array> {
    let bytes = js_sys::Reflect::get(data, &JsValue::from_str("bytes")).ok()?;
    if let Some(array) = bytes.dyn_ref::<js_sys::Uint8Array>() {
        Some(array.clone())
    } else if bytes.is_instance_of::<js_sys::ArrayBuffer>() {
        Some(js_sys::Uint8Array::new(&bytes))
    } else {
        None
    }
}
