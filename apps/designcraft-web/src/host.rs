//! The `postMessage` bridge to a same-origin parent page (`?host=parent`), for pages that embed
//! DesignCraft in an `<iframe>` (such as a file manager). It mirrors the other craft apps:
//!
//! - `designcraft:ready` (to the parent): `{ version }`, the app is listening; send a file now.
//! - `designcraft:open` (from the parent): `{ name, bytes }`, bytes an `ArrayBuffer` or
//!   `Uint8Array`. It opens like a file chosen with File › Open (`.designcraft`, `.idml`…).
//! - `designcraft:save` (to the parent): `{ name, bytes }` (a transferred `ArrayBuffer`) for every
//!   file DesignCraft would otherwise download. Save and Save As send the `.designcraft` document
//!   and mark it saved; exports (PDF, PNG, IDML…) add `export: true`.
//! - `designcraft:dirty` (to the parent): `{ dirty }` whenever unsaved changes appear or go away
//!   in any open document.
//! - `designcraft:failed` (to the parent): `{ error }` when the app couldn't start, or when an
//!   opened file is larger than [`MAX_OPEN_BYTES`] (it isn't copied or opened).
//!
//! Only the parent window on the page's own origin is heard or answered; a parent on another
//! origin gets no bridge (saves download as usual). In host mode `?sample` is ignored.
//! DesignCraft has no user name to set, so `?author=` is ignored.

use std::cell::Cell;
use std::rc::Rc;

use designcraft_ui_egui::{ImportRequest, ImportedFile, Inbox, Services};
use wasm_bindgen::JsCast as _;
use wasm_bindgen::JsValue;
use wasm_bindgen::closure::Closure;

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
}

impl Bridge {
    /// `None` when the page isn't framed, or the parent is on another origin.
    pub fn new() -> Option<Self> {
        let window = web_sys::window()?;
        let parent = window.parent().ok().flatten()?;
        if js_sys::Object::is(&parent, &window) {
            return None;
        }
        let origin = window.location().origin().ok()?;
        // Reading another origin's location throws, which is `None` too.
        if parent.location().origin().ok()? != origin {
            return None;
        }
        Some(Self { window, parent, origin, reported: Rc::default() })
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

    pub fn post_dirty(&self, dirty: bool) {
        if self.reported.get() != Some(dirty) && self.send(&[("type", "designcraft:dirty".into()), ("dirty", dirty.into())]) {
            self.reported.set(Some(dirty));
        }
    }

    fn post_save(&self, path: &str, bytes: &[u8], export: bool) -> Result<(), String> {
        let name = std::path::Path::new(path).file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_else(|| path.to_string());
        let buffer = js_sys::Uint8Array::from(bytes).buffer();
        let mut fields = vec![("type", "designcraft:save".into()), ("name", name.as_str().into()), ("bytes", JsValue::from(buffer.clone()))];
        if export {
            fields.push(("export", true.into()));
        }
        let message = message(&fields).ok_or_else(|| "couldn't build the message".to_string())?;
        self.parent
            .post_message_with_transfer(&message, &self.origin, &js_sys::Array::of1(&buffer))
            .map_err(|e| e.as_string().unwrap_or_else(|| format!("{e:?}")))
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
            if string(&data, "type").as_deref() != Some("designcraft:open") {
                return;
            }
            let name = string(&data, "name").filter(|n| !n.trim().is_empty()).unwrap_or_else(|| "Untitled.designcraft".to_string());
            let Some(array) = byte_array(&data) else { return };
            // Refuse oversize files before copying them out of the parent's buffer.
            if u64::from(array.byte_length()) > MAX_OPEN_BYTES {
                bridge.post_failed(&format!(
                    "{name} is {} bytes, larger than the {MAX_OPEN_BYTES}-byte limit for opened files",
                    array.byte_length()
                ));
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
