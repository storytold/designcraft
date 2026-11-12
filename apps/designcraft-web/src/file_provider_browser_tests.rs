//! Original synthetic layout/embedded SVG; one page owns both temporary JS fault injections.
//! Uses the real window API, publisher, native format and DesignApp logic consumer.

use std::cell::{Cell, RefCell};
use std::rc::Rc;
use std::sync::Arc;

use designcraft_engine::Session;
use designcraft_ui_egui::{DesignApp, Services};
use js_sys::{Array, Function, Object, Reflect, Uint8Array};
use serde_json::json;
use wasm_bindgen::{JsCast, JsValue, closure::Closure};
use wasm_bindgen_test::{wasm_bindgen_test, wasm_bindgen_test_configure};

use super::{API, CURRENT_CONTEXT, EVENT, error, install, object, publish};

wasm_bindgen_test_configure!(run_in_browser);

const SVG: &[u8] = br##"<svg xmlns="http://www.w3.org/2000/svg" width="8" height="8"><rect width="8" height="8" fill="#2468ac"/></svg>"##;

// Fault injection stays in Rust. The actual Function::call0 catches a recoverable exception;
// restore the exact own descriptor (or remove our shadow) before propagating that Result.
// Fatal WASM traps, true memory exhaustion and process termination are outside this fixture.
fn with_replacement(target: &JsValue, key: &str, replacement: &JsValue, callback: &Function) -> Result<Result<JsValue, JsValue>, JsValue> {
    let object = Reflect::get(&js_sys::global(), &"Object".into())?;
    let key = JsValue::from_str(key);
    let descriptor = call(&object, "getOwnPropertyDescriptor", &Array::of2(target, &key))?;
    let original = Reflect::get(target, &key)?;
    let outcome = match Reflect::set(target, &key, replacement) {
        Ok(true) => callback.call0(&JsValue::UNDEFINED),
        Ok(false) => Err(error("injected method is not writable")),
        Err(error) => Err(error),
    };
    if descriptor.is_undefined() {
        if !Reflect::delete_property(target.unchecked_ref::<Object>(), &key)? {
            return Err(error("could not remove injected method"));
        }
    } else {
        call(&object, "defineProperty", &Array::of3(target, &key, &descriptor))?;
    }
    let restored = call(&object, "getOwnPropertyDescriptor", &Array::of2(target, &key))?;
    if !Object::is(&Reflect::get(target, &key)?, &original) || restored.is_undefined() != descriptor.is_undefined() {
        return Err(error("injected property was not restored exactly"));
    }
    if !descriptor.is_undefined() {
        for attribute in ["value", "get", "set", "writable", "enumerable", "configurable"] {
            if !Object::is(&Reflect::get(&restored, &attribute.into())?, &Reflect::get(&descriptor, &attribute.into())?) {
                return Err(error("injected property descriptor changed"));
            }
        }
    }
    Ok(outcome)
}

fn fault_receipt(calls: u32, requests: &Array) -> Result<JsValue, JsValue> {
    let receipt = Object::new();
    for (key, value) in
        [("calls", JsValue::from_f64(f64::from(calls))), ("restored", JsValue::from_bool(true)), ("requests", JsValue::from(requests.clone()))]
    {
        if !Reflect::set(&receipt, &key.into(), &value)? {
            return Err(error("could not record restored fault injection"));
        }
    }
    Ok(receipt.into())
}

fn copy_failure(callback: &Function) -> Result<JsValue, JsValue> {
    let calls = Rc::new(Cell::new(0_u32));
    let observed = calls.clone();
    let fault = Closure::<dyn FnMut(JsValue) -> Result<JsValue, JsValue>>::new(move |_| {
        observed.set(observed.get().saturating_add(1));
        Err(js_sys::RangeError::new("injected DesignCraft byte copy failure").into())
    });
    let array = Reflect::get(&js_sys::global(), &"Uint8Array".into())?;
    with_replacement(&array, "from", fault.as_ref(), callback)??;
    fault_receipt(calls.get(), &Array::new())
}

fn request_failure(callback: &Function) -> Result<JsValue, JsValue> {
    let window = web_sys::window().ok_or_else(|| error("no test window"))?;
    let original = Reflect::get(&window, &"dispatchEvent".into())?.dyn_into::<Function>()?;
    let receiver: JsValue = window.into();
    let target = receiver.clone();
    let calls = Rc::new(Cell::new(0_u32));
    let observed = calls.clone();
    let requests = Array::new();
    let rejected = requests.clone();
    let fault = Closure::<dyn FnMut(JsValue) -> Result<JsValue, JsValue>>::new(move |event: JsValue| {
        if Reflect::get(&event, &"type".into())?.as_string().as_deref() == Some(EVENT) {
            let detail = Reflect::get(&event, &"detail".into())?;
            let kind = Reflect::get(&detail, &"type".into())?.as_string();
            if matches!(kind.as_deref(), Some("saveDialog" | "openRequested")) {
                observed.set(observed.get().saturating_add(1));
                let request = Object::new();
                for name in ["type", "requestId"] {
                    if !Reflect::set(&request, &name.into(), &Reflect::get(&detail, &name.into())?)? {
                        return Err(error("could not record rejected request"));
                    }
                }
                rejected.push(&request);
                return Err(js_sys::Error::new("injected DesignCraft request dispatch failure").into());
            }
        }
        original.call1(&receiver, &event)
    });
    with_replacement(&target, "dispatchEvent", fault.as_ref(), callback)??;
    fault_receipt(calls.get(), &requests)
}

struct Events {
    window: web_sys::Window,
    listener: Closure<dyn FnMut(web_sys::Event)>,
    details: Rc<RefCell<Vec<JsValue>>>,
    blocked_unregistrations: Rc<RefCell<Vec<bool>>>,
    installed: bool,
}

impl Events {
    fn new() -> Result<Self, JsValue> {
        let window = web_sys::window().ok_or_else(|| error("no test window"))?;
        if Reflect::has(&window, &API.into())? {
            return Err(error("fixture requires its own blank page API"));
        }
        let details = Rc::new(RefCell::new(Vec::new()));
        let received = details.clone();
        let blocked_unregistrations = Rc::new(RefCell::new(Vec::new()));
        let blocked = blocked_unregistrations.clone();
        let current_window = window.clone();
        let listener = Closure::<dyn FnMut(web_sys::Event)>::new(move |event: web_sys::Event| {
            if let Some(event) = event.dyn_ref::<web_sys::CustomEvent>() {
                let detail = event.detail();
                let terminal = Reflect::get(&detail, &"type".into()).ok().and_then(|kind| kind.as_string()).is_some_and(|kind| {
                    matches!(kind.as_str(), "saved" | "downloaded" | "saveFailed" | "saveCancelled" | "opened" | "openFailed" | "openCancelled")
                });
                if terminal {
                    let result: Result<JsValue, JsValue> = (|| {
                        let api = Reflect::get(&current_window, &API.into())?;
                        Reflect::get(&api, &"unregisterProvider".into())?.dyn_into::<Function>()?.call0(&api)
                    })();
                    blocked.borrow_mut().push(
                        result
                            .err()
                            .and_then(|value| value.as_string())
                            .is_some_and(|message| message == "finish or cancel pending provider operations before unregistering"),
                    );
                }
                received.borrow_mut().push(detail);
            }
        });
        window.add_event_listener_with_callback(EVENT, listener.as_ref().unchecked_ref())?;
        Ok(Self { window, listener, details, blocked_unregistrations, installed: false })
    }

    fn matching(&self, kind: &str) -> Result<Vec<JsValue>, JsValue> {
        let mut found = Vec::new();
        for detail in self.details.borrow().iter() {
            if Reflect::get(detail, &"type".into())?.as_string().as_deref() == Some(kind) {
                found.push(detail.clone());
            }
        }
        Ok(found)
    }
}

impl Drop for Events {
    fn drop(&mut self) {
        let _ = self.window.remove_event_listener_with_callback(EVENT, self.listener.as_ref().unchecked_ref());
        if self.installed {
            let _ = Reflect::delete_property(self.window.unchecked_ref::<Object>(), &API.into());
            CURRENT_CONTEXT.with(|current| *current.borrow_mut() = None);
        }
    }
}

fn call(api: &JsValue, method: &str, args: &Array) -> Result<JsValue, JsValue> {
    Reflect::get(api, &method.into())?.dyn_into::<Function>()?.apply(api, args)
}

fn field(value: &JsValue, name: &str) -> Result<String, JsValue> {
    Reflect::get(value, &name.into())?.as_string().ok_or_else(|| error(format!("missing {name}")))
}

fn frame(app: &mut DesignApp, ctx: &egui::Context) {
    let mut output = ctx.run_ui(egui::RawInput::default(), |ui| app.logic(ui.ctx()));
    output.textures_delta.clear();
}

#[wasm_bindgen_test]
fn page_local_provider_delivery_failure_preserves_native_resources_and_recovers() -> Result<(), JsValue> {
    let mut events = Events::new()?;
    let mut session = Session::new();
    session.execute("file.new", &json!({})).map_err(error)?;
    let mut app = DesignApp::new(session, Services::default());
    let host = app.file_providers.clone();
    let ctx = egui::Context::default();
    // Events::new established that this page has no API. Own cleanup before install:
    // it sets CURRENT_CONTEXT before fallible work and may publish the API before ready fails.
    events.installed = true;
    install(host.clone(), ctx.clone())?;
    let api = Reflect::get(&events.window, &API.into())?;
    let connection = call(&api, "registerProvider", &Array::of3(&"test".into(), &"Test storage".into(), &"account".into()))?;
    frame(&mut app, &ctx);
    app.session.execute("file.place", &json!({"name":"Original.svg","base64":designcraft_engine::cmd::base64_encode(SVG)})).map_err(error)?;
    let captured = app.session.active().ok_or_else(|| error("missing document"))?.clone();
    let request = app.run("app.save", json!({})).map_err(error)?;
    let failed_request = request["requestId"].as_str().ok_or_else(|| error("missing request ID"))?.to_string();
    publish(&host, &ctx);
    let dialogs = events.matching("saveDialog")?;
    assert_eq!(dialogs.len(), 1);
    assert_eq!(field(&dialogs[0], "requestId")?, failed_request);
    call(&api, "resolveSaveDialog", &Array::of3(&failed_request.clone().into(), &"provider".into(), &"Captured.designcraft".into()))?;
    let current = app.session.active_mut().ok_or_else(|| error("missing document"))?;
    Arc::make_mut(&mut current.doc).title = "Newer layout survives".into();
    current.bump_revision();
    let newer = current.clone();
    let pending_host = host.clone();
    let pending_ctx = ctx.clone();
    let publisher = Closure::<dyn FnMut()>::new(move || publish(&pending_host, &pending_ctx));
    let fault = copy_failure(publisher.as_ref().unchecked_ref())?;
    assert_eq!(Reflect::get(&fault, &"calls".into())?.as_f64(), Some(1.0));
    assert_eq!(Reflect::get(&fault, &"restored".into())?.as_bool(), Some(true));
    assert!(events.matching("save")?.is_empty());
    assert!(host.pending_for(captured.uid));
    assert!(call(&api, "saveResult", &Array::of3(&failed_request.clone().into(), &"saved".into(), &"late".into())).is_err());
    frame(&mut app, &ctx);
    let current = app.session.active().ok_or_else(|| error("missing document after failure"))?;
    assert_eq!(current.uid, captured.uid);
    assert_eq!(current.revision, newer.revision);
    assert!(Arc::ptr_eq(&current.doc, &newer.doc));
    assert!(Arc::ptr_eq(&current.saved_doc, &captured.saved_doc));
    assert_eq!(current.saved_revision, captured.saved_revision);
    assert!(current.is_dirty());
    assert!(!host.pending_for(captured.uid), "the actual DesignApp consumer must release the failed capture");
    assert!(host.snapshot()["resources"].as_array().is_some_and(Vec::is_empty));
    publish(&host, &ctx);
    let failures = events.matching("saveFailed")?;
    assert_eq!(failures.len(), 1);
    assert_eq!(field(&failures[0], "requestId")?, failed_request);
    assert_eq!(field(&failures[0], "documentId")?, captured.uid.to_string());
    assert_eq!(events.blocked_unregistrations.borrow().as_slice(), &[true]);

    // Retry crosses the actual window event with a complete native project and embedded SVG.
    let retry = app.run("app.save", json!({})).map_err(error)?;
    let retry = retry["requestId"].as_str().ok_or_else(|| error("missing retry ID"))?.to_string();
    publish(&host, &ctx);
    call(&api, "resolveSaveDialog", &Array::of3(&retry.clone().into(), &"provider".into(), &"Retry.designcraft".into()))?;
    publish(&host, &ctx);
    let saves = events.matching("save")?;
    assert_eq!(saves.len(), 1);
    assert_eq!(field(&saves[0], "requestId")?, retry);
    assert_eq!(field(&saves[0], "filename")?, "Retry.designcraft");
    assert_eq!(field(&saves[0], "revision")?, newer.revision.to_string());
    let bytes = Reflect::get(&saves[0], &"bytes".into())?.dyn_into::<Uint8Array>()?.to_vec();
    assert!(bytes.starts_with(b"PK"));
    let restored = designcraft_format::load(&bytes).map_err(error)?;
    assert_eq!(serde_json::to_value(&restored).map_err(error)?, serde_json::to_value(newer.doc.as_ref()).map_err(error)?);
    let asset = restored.assets.values().find(|asset| asset.name == "Original.svg").ok_or_else(|| error("embedded SVG missing"))?;
    assert_eq!(asset.data.as_slice(), SVG);
    call(&api, "saveResult", &Array::of3(&retry.clone().into(), &"saved".into(), &"saved-resource".into()))?;
    frame(&mut app, &ctx);
    assert!(!app.session.active().ok_or_else(|| error("missing saved document"))?.is_dirty());
    assert_eq!(host.bound_document("saved-resource"), Some(captured.uid));
    publish(&host, &ctx);
    let saved = events.matching("saved")?;
    assert_eq!(saved.len(), 1);
    assert_eq!(field(&saved[0], "requestId")?, retry);

    // Use a distinct unbound resource: this must run the real native decoder, not focus a tab.
    let resources = object(&json!([
        {"id":"saved-resource","name":"Retry.designcraft"},
        {"id":"fresh-open","name":"Restored.designcraft"}
    ]))?;
    call(&api, "setResources", &Array::of2(&connection, &resources))?;
    let read = call(&api, "requestOpen", &Array::of1(&"fresh-open".into()))?;
    publish(&host, &ctx);
    let reads = events.matching("openRequested")?;
    assert_eq!(reads.len(), 1);
    assert_eq!(Reflect::get(&reads[0], &"requestId".into())?, read);
    assert_eq!(field(&reads[0], "resourceId")?, "fresh-open");
    let transferred = Uint8Array::from(bytes.as_slice());
    call(&api, "openResource", &Array::of3(&read, &"Restored.designcraft".into(), &transferred))?;
    frame(&mut app, &ctx);
    let opened = app.session.active().ok_or_else(|| error("native open produced no document"))?.clone();
    assert_ne!(opened.uid, captured.uid);
    let mut expected = newer.doc.as_ref().clone();
    expected.title = "Restored".into();
    assert_eq!(serde_json::to_value(opened.doc.as_ref()).map_err(error)?, serde_json::to_value(expected).map_err(error)?);
    assert_eq!(
        opened.doc.assets.values().find(|asset| asset.name == "Original.svg").ok_or_else(|| error("opened SVG missing"))?.data.as_slice(),
        SVG
    );
    assert_eq!(host.bound_document("fresh-open"), Some(opened.uid));
    publish(&host, &ctx);
    let admissions = events.matching("opened")?;
    assert_eq!(admissions.len(), 1);
    assert_eq!(Reflect::get(&admissions[0], &"requestId".into())?, read);
    assert_eq!(field(&admissions[0], "documentId")?, opened.uid.to_string());

    let choice = app.run("app.save", json!({})).map_err(error)?;
    let choice = choice["requestId"].as_str().ok_or_else(|| error("missing choice ID"))?.to_string();
    let read = call(&api, "requestOpen", &Array::of1(&"saved-resource".into()))?;
    assert!(host.pending_for(captured.uid), "the outstanding read protects the bound original alias");
    let fault = request_failure(publisher.as_ref().unchecked_ref())?;
    assert_eq!(Reflect::get(&fault, &"calls".into())?.as_f64(), Some(2.0));
    assert_eq!(Reflect::get(&fault, &"restored".into())?.as_bool(), Some(true));
    let rejected = Reflect::get(&fault, &"requests".into())?.dyn_into::<Array>()?;
    assert_eq!(rejected.length(), 2);
    assert_eq!(field(&rejected.get(0), "requestId")?, choice);
    assert_eq!(Reflect::get(&rejected.get(1), &"requestId".into())?, read);
    frame(&mut app, &ctx);
    publish(&host, &ctx);
    let failures = events.matching("saveFailed")?;
    assert_eq!(failures.len(), 2);
    assert_eq!(field(&failures[1], "requestId")?, choice);
    let read_failures = events.matching("openFailed")?;
    assert_eq!(read_failures.len(), 1);
    assert_eq!(Reflect::get(&read_failures[0], &"requestId".into())?, read);
    assert_eq!(field(&read_failures[0], "resourceId")?, "saved-resource");
    assert!(!host.pending_for(captured.uid) && !host.pending_for(opened.uid));
    let current = app.session.active().ok_or_else(|| error("missing current document"))?;
    assert_eq!(current.uid, opened.uid);
    assert!(Arc::ptr_eq(&current.doc, &opened.doc) && Arc::ptr_eq(&current.saved_doc, &opened.saved_doc));
    assert_eq!(current.saved_revision, opened.saved_revision);
    assert!(!current.is_dirty());
    assert!(call(&api, "resolveSaveDialog", &Array::of3(&choice.into(), &"provider".into(), &"Late.designcraft".into())).is_err());
    assert!(call(&api, "cancelOpen", &Array::of2(&read, &"late".into())).is_err());

    let choice = app.run("app.save", json!({})).map_err(error)?;
    let choice = choice["requestId"].as_str().ok_or_else(|| error("missing retry choice ID"))?.to_string();
    publish(&host, &ctx);
    call(&api, "resolveSaveDialog", &Array::of3(&choice.into(), &"cancel".into(), &"".into()))?;
    frame(&mut app, &ctx);
    publish(&host, &ctx);
    let cancelled = events.matching("saveCancelled")?;
    assert_eq!(cancelled.len(), 1);
    let read = call(&api, "requestOpen", &Array::of1(&"saved-resource".into()))?;
    publish(&host, &ctx);
    call(&api, "cancelOpen", &Array::of2(&read, &"".into()))?;
    publish(&host, &ctx);
    assert_eq!(events.matching("openCancelled")?.len(), 1);
    assert!(!events.blocked_unregistrations.borrow().is_empty());
    assert!(
        events.blocked_unregistrations.borrow().iter().all(|blocked| *blocked),
        "actual window callbacks must not unregister an in-flight terminal"
    );
    assert!(!host.busy());
    assert_eq!(call(&api, "unregisterProvider", &Array::new())?.as_bool(), Some(true));
    Ok(())
}
