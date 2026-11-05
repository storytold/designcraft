//! The browser shell: web `Services`, drag-and-drop, and the eframe web runner.

use super::browser_policy::{has_query_flag, mime_for};
use crate::graphics::{GraphicsState, RecoveryDocument, recovery_documents};
use designcraft_engine::Session;
use designcraft_ui_egui::{DesignApp, ImportRequest, ImportedFile, Inbox, Services};
use wasm_bindgen::JsCast as _;

const DOC_EXTS: &[&str] = &["designcraft", "idml", "indd", "indt"];
const IMAGE_EXTS: &[&str] = &[
    "png",
    "jpg",
    "jpeg",
    "gif",
    "webp",
    "tif",
    "tiff",
    "bmp",
    "psd",
    "svg",
    "pdf",
    "ai",
    "txt",
    "docx",
    "rtf",
    "md",
    "xlsx",
    "idml",
    "designcraft",
];
const CANVAS_ID: &str = "designcraft_canvas";
const LOADING_ID: &str = "designcraft_loading";

pub fn start() {
    eframe::WebLogger::init(log::LevelFilter::Info).ok();
    wasm_bindgen_futures::spawn_local(async {
        let Some(document) = web_sys::window().and_then(|w| w.document()) else {
            log::error!("no document");
            return;
        };
        if let Err(e) = create_error_panel(&document) {
            log::error!("couldn't create graphics error panel: {e:?}");
            if let Some(el) = document.get_element_by_id(LOADING_ID) {
                el.set_text_content(Some("DesignCraft couldn't start. Reload this page to try again."));
            }
            return;
        }
        let graphics = GraphicsState::default();
        let Some(canvas) = document.get_element_by_id(CANVAS_ID).and_then(|e| e.dyn_into::<web_sys::HtmlCanvasElement>().ok()) else {
            graphics_failed(&graphics, "DesignCraft failed to start", "Reload this page to try again.", &format!("Missing canvas {CANVAS_ID}."));
            return;
        };
        let mut options = eframe::WebOptions::default();
        if has_query_flag(&query(), "webgl")
            && let eframe::egui_wgpu::WgpuSetup::CreateNew(create) = &mut options.wgpu_options.wgpu_setup
        {
            create.instance_descriptor.backends = eframe::wgpu::Backends::GL;
        }
        let runner = eframe::WebRunner::new();
        let creator_graphics = graphics.clone();
        let result = runner
            .start(
                canvas.clone(),
                options,
                Box::new(move |cc| {
                    if let Some(rs) = &cc.wgpu_render_state {
                        log::info!("designcraft-web: wgpu backend {:?}", rs.adapter.get_info().backend);
                        let device_graphics = creator_graphics.clone();
                        // This also observes an already-lost WebGPU device through its lost promise.
                        rs.device.set_device_lost_callback(move |reason, message| {
                            graphics_failed(&device_graphics, GRAPHICS_TITLE, GRAPHICS_MESSAGE, &format!("Device loss ({reason:?}): {message}"));
                        });
                    }
                    let inbox: Inbox = Inbox::default();
                    let mut app = DesignApp::new(Session::new(), services(inbox.clone(), cc.egui_ctx.clone()));
                    if has_query_flag(&query(), "sample") {
                        let _ = app.run("file.newSample", serde_json::json!({}));
                    }
                    Ok(Box::new(WebShell { app, inbox, graphics: creator_graphics }))
                }),
            )
            .await;
        match result {
            Ok(()) => {
                if let Err(e) = install_recovery_controls(&runner, &canvas, &graphics) {
                    graphics_failed(&graphics, GRAPHICS_TITLE, GRAPHICS_MESSAGE, &format!("Couldn't install graphics recovery controls: {e:?}"));
                    if let Some(shell) = runner.app_mut::<WebShell>()
                        && let Err(e) = prepare_document_recovery(&runner, &shell.app.session)
                    {
                        recovery_controls_failed(&e);
                    }
                }
                // A loss callback can run during startup. Never hide its failure panel.
                if !graphics.has_failed()
                    && let Some(el) = document.get_element_by_id(LOADING_ID)
                {
                    el.remove();
                }
            }
            Err(e) => {
                graphics_failed(
                    &graphics,
                    "DesignCraft failed to start",
                    "Reload this page to try again. A browser with WebGPU or WebGL2 is required.",
                    &format!("{e:?}"),
                );
                if let Some(shell) = runner.app_mut::<WebShell>()
                    && let Err(e) = prepare_document_recovery(&runner, &shell.app.session)
                {
                    recovery_controls_failed(&e);
                }
            }
        }
    });
}

const ERROR_ID: &str = "designcraft_graphics_error";
const ERROR_TITLE_ID: &str = "designcraft_graphics_title";
const ERROR_MESSAGE_ID: &str = "designcraft_graphics_message";
const ERROR_DETAIL_ID: &str = "designcraft_graphics_detail";
const ERROR_ACTIONS_ID: &str = "designcraft_graphics_actions";
const ERROR_STATUS_ID: &str = "designcraft_graphics_status";
const ERROR_DOCUMENTS_ID: &str = "designcraft_graphics_documents";
const GRAPHICS_LOST_EVENT: &str = "designcraftgraphicslost";
const GRAPHICS_TITLE: &str = "Graphics connection lost";
const GRAPHICS_MESSAGE: &str = "Editing has stopped. Save a copy of each open document, then reload this page and reopen the copies. Reloading discards changes you have not downloaded.";

fn create_error_panel(document: &web_sys::Document) -> Result<(), wasm_bindgen::JsValue> {
    let panel: web_sys::HtmlElement = document.create_element("div")?.dyn_into()?;
    panel.set_id(ERROR_ID);
    panel.set_tab_index(-1);
    panel.set_attribute("role", "alertdialog")?;
    panel.set_attribute("aria-modal", "true")?;
    panel.set_attribute("aria-labelledby", ERROR_TITLE_ID)?;
    panel.set_attribute("aria-describedby", ERROR_MESSAGE_ID)?;
    // DOM content remains visible when the GPU can no longer paint the egui canvas.
    let colors = designcraft_ui_egui::theme::Tokens::for_brightness(designcraft_ui_egui::theme::Brightness::Dark);
    panel.style().set_css_text(&format!(
        "position:fixed;inset:0;z-index:1;display:none;flex-direction:column;align-items:center;justify-content:safe center;box-sizing:border-box;padding:24px;overflow:auto;background:{};color:{};font:14px system-ui,sans-serif;gap:12px;",
        css_color(colors.app_bar), css_color(colors.text),
    ));
    for (tag, id) in [("h1", ERROR_TITLE_ID), ("p", ERROR_MESSAGE_ID), ("div", ERROR_ACTIONS_ID), ("p", ERROR_STATUS_ID), ("pre", ERROR_DETAIL_ID)] {
        let child: web_sys::HtmlElement = document.create_element(tag)?.dyn_into()?;
        child.set_id(id);
        child.style().set_css_text("margin:0;max-width:640px;white-space:pre-wrap;overflow-wrap:anywhere;");
        if id == ERROR_ACTIONS_ID {
            child.style().set_css_text("display:flex;flex-direction:column;gap:8px;max-width:100%;");
            add_reload_control(document, &child)?;
        } else if id == ERROR_STATUS_ID {
            child.set_attribute("aria-live", "polite")?;
        }
        panel.append_child(&child)?;
    }
    document.body().ok_or_else(|| wasm_bindgen::JsValue::from_str("no body"))?.append_child(&panel)?;
    Ok(())
}

fn graphics_failed(graphics: &GraphicsState, title: &str, message: &str, detail: &str) {
    if !graphics.fail() {
        return;
    }
    log::error!("designcraft-web: {detail}");
    if let Err(e) = show_error(title, message, detail) {
        log::error!("couldn't show graphics error: {e:?}");
    }
    // A device callback may run while AppRunner is borrowed. Defer its notification;
    // the WebGL listener already holding AppRunner captures documents directly.
    if let Some(window) = web_sys::window()
        && let Some(canvas) = window.document().and_then(|doc| doc.get_element_by_id(CANVAS_ID))
    {
        let notify = wasm_bindgen::closure::Closure::once_into_js(move || {
            if let Err(e) = web_sys::Event::new(GRAPHICS_LOST_EVENT).and_then(|event| canvas.dispatch_event(&event)) {
                recovery_controls_failed(&e);
            }
        });
        window.queue_microtask(notify.unchecked_ref());
    }
}

fn show_error(title: &str, message: &str, detail: &str) -> Result<(), wasm_bindgen::JsValue> {
    let document = web_sys::window().and_then(|w| w.document()).ok_or_else(|| wasm_bindgen::JsValue::from_str("no document"))?;
    let panel: web_sys::HtmlElement =
        document.get_element_by_id(ERROR_ID).ok_or_else(|| wasm_bindgen::JsValue::from_str("no graphics error panel"))?.dyn_into()?;
    for (id, text) in [(ERROR_TITLE_ID, title), (ERROR_MESSAGE_ID, message), (ERROR_DETAIL_ID, detail)] {
        if let Some(el) = document.get_element_by_id(id) {
            el.set_text_content(Some(text));
        }
    }
    panel.style().set_property("display", "flex")?;
    if let Some(canvas) = document.get_element_by_id(CANVAS_ID).and_then(|el| el.dyn_into::<web_sys::HtmlElement>().ok()) {
        canvas.style().set_property("visibility", "hidden")?;
    }
    if let Some(loading) = document.get_element_by_id(LOADING_ID).and_then(|el| el.dyn_into::<web_sys::HtmlElement>().ok()) {
        loading.style().set_property("display", "none")?;
    }
    // Include eframe's hidden text input: neither pointer nor keyboard focus may reach the editor.
    let mut sibling = document.body().and_then(|body| body.first_element_child());
    while let Some(el) = sibling {
        sibling = el.next_element_sibling();
        if el.id() != ERROR_ID {
            if let Some(html) = el.dyn_ref::<web_sys::HtmlElement>() {
                html.set_inert(true);
            }
            el.set_attribute("aria-hidden", "true")?;
        }
    }
    panel.focus()?;
    Ok(())
}

/// Recovery listeners are owned by their DOM targets, independently of WebRunner.
fn add_dom_event_listener(
    target: &web_sys::EventTarget,
    event_name: &str,
    callback: impl FnMut(web_sys::Event) + 'static,
) -> Result<(), wasm_bindgen::JsValue> {
    let callback = wasm_bindgen::closure::Closure::wrap(Box::new(callback) as Box<dyn FnMut(web_sys::Event)>);
    target.add_event_listener_with_callback(event_name, callback.as_ref().unchecked_ref())?;
    // EventTarget retains the JS function; weak references reclaim its captures with the target.
    let _ = callback.into_js_value();
    Ok(())
}

fn css_color(color: egui::Color32) -> String {
    format!("#{:02x}{:02x}{:02x}", color.r(), color.g(), color.b())
}

fn recovery_button(document: &web_sys::Document) -> Result<web_sys::HtmlElement, wasm_bindgen::JsValue> {
    let button: web_sys::HtmlElement = document.create_element("button")?.dyn_into()?;
    button.set_attribute("type", "button")?;
    let colors = designcraft_ui_egui::theme::Tokens::for_brightness(designcraft_ui_egui::theme::Brightness::Dark);
    button.style().set_css_text(&format!(
        "box-sizing:border-box;max-width:100%;padding:6px 12px;border:1px solid {};border-radius:{}px;background:{};color:{};font:inherit;white-space:normal;overflow-wrap:anywhere;cursor:pointer;",
        css_color(colors.field_border), colors.radius, css_color(colors.button), css_color(colors.text),
    ));
    Ok(button)
}

fn add_reload_control(document: &web_sys::Document, actions: &web_sys::HtmlElement) -> Result<(), wasm_bindgen::JsValue> {
    let reload = recovery_button(document)?;
    reload.set_text_content(Some("Reload"));
    add_dom_event_listener(&reload, "click", move |event| {
        event.stop_propagation();
        if let Some(window) = web_sys::window()
            && let Err(e) = window.location().reload()
        {
            log::error!("couldn't reload: {e:?}");
        }
    })?;
    actions.append_child(&reload)?;
    Ok(())
}

fn install_recovery_controls(
    runner: &eframe::WebRunner,
    canvas: &web_sys::HtmlCanvasElement,
    graphics: &GraphicsState,
) -> Result<(), wasm_bindgen::JsValue> {
    let loss_runner = runner.clone();
    runner.add_event_listener(canvas, GRAPHICS_LOST_EVENT, move |_: web_sys::Event, app_runner| {
        let shell = app_runner.app_mut::<WebShell>();
        if shell.graphics.has_failed()
            && let Err(e) = prepare_document_recovery(&loss_runner, &shell.app.session)
        {
            recovery_controls_failed(&e);
        }
    })?;
    let context_graphics = graphics.clone();
    let context_runner = runner.clone();
    // wgpu's GLES backend does not observe browser WebGL context-loss events.
    runner.add_event_listener(canvas, "webglcontextlost", move |_: web_sys::Event, app_runner| {
        graphics_failed(&context_graphics, GRAPHICS_TITLE, GRAPHICS_MESSAGE, "The browser lost the WebGL graphics context.");
        if let Err(e) = prepare_document_recovery(&context_runner, &app_runner.app_mut::<WebShell>().app.session) {
            recovery_controls_failed(&e);
        }
    })?;

    // A WebGPU device can be lost while start() is still awaiting initialization.
    if graphics.has_failed()
        && let Some(shell) = runner.app_mut::<WebShell>()
    {
        prepare_document_recovery(runner, &shell.app.session)?;
    }
    Ok(())
}

fn prepare_document_recovery(runner: &eframe::WebRunner, session: &Session) -> Result<(), wasm_bindgen::JsValue> {
    let window = web_sys::window().ok_or_else(|| wasm_bindgen::JsValue::from_str("no window"))?;
    let documents = recovery_documents(session);
    if add_document_recovery_controls(documents)? {
        // Controls now own the snapshots. Release AppRunner's borrow before stopping painting
        // and removing its listeners/ResizeObserver; never discard documents on setup failure.
        let runner = runner.clone();
        let stop = wasm_bindgen::closure::Closure::once_into_js(move || runner.destroy());
        window.queue_microtask(stop.unchecked_ref());
    }
    Ok(())
}

fn recovery_controls_failed(error: &wasm_bindgen::JsValue) {
    log::error!("couldn't prepare recovery downloads: {error:?}");
    if let Some(status) = web_sys::window().and_then(|w| w.document()).and_then(|doc| doc.get_element_by_id(ERROR_STATUS_ID)) {
        status.set_text_content(Some("Couldn't prepare document downloads. Keep this page open to preserve unsaved documents."));
    }
}

fn add_document_recovery_controls(snapshots: Vec<RecoveryDocument>) -> Result<bool, wasm_bindgen::JsValue> {
    let document = web_sys::window().and_then(|w| w.document()).ok_or_else(|| wasm_bindgen::JsValue::from_str("no document"))?;
    let actions = document.get_element_by_id(ERROR_ACTIONS_ID).ok_or_else(|| wasm_bindgen::JsValue::from_str("no recovery actions"))?;
    if document.get_element_by_id(ERROR_DOCUMENTS_ID).is_some() {
        return Ok(false);
    }
    let documents: web_sys::HtmlElement = document.create_element("div")?.dyn_into()?;
    documents.set_id(ERROR_DOCUMENTS_ID);
    documents.style().set_css_text("display:flex;flex-direction:column;gap:8px;max-width:100%;");
    for snapshot in snapshots {
        let button = recovery_button(&document)?;
        button.set_text_content(Some(&format!("Save a Copy: {}", snapshot.title())));
        add_dom_event_listener(&button, "click", move |event| {
            event.stop_propagation();
            let result = snapshot.save_copy().and_then(|(name, bytes)| {
                download(&name, &bytes)?;
                Ok(format!("Requested download of {name}. Check your browser downloads before reloading."))
            });
            let text = result.unwrap_or_else(|e| e);
            if let Some(status) = web_sys::window().and_then(|w| w.document()).and_then(|doc| doc.get_element_by_id(ERROR_STATUS_ID)) {
                status.set_text_content(Some(&text));
            }
        })?;
        documents.append_child(&button)?;
    }
    // Build off-DOM so a setup error leaves the session intact and permits a clean retry.
    // The inserted element's id also prevents duplicate buttons on repeated loss notifications.
    actions.insert_before(&documents, actions.first_child().as_ref())?;
    Ok(true)
}

fn query() -> String {
    web_sys::window().and_then(|w| w.location().search().ok()).unwrap_or_default()
}

/// Wraps the app to read dropped files asynchronously (browsers can't read them synchronously)
/// and feed them through the inbox.
struct WebShell {
    app: DesignApp,
    inbox: Inbox,
    graphics: GraphicsState,
}

impl eframe::App for WebShell {
    fn logic(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        let graphics = self.graphics.clone();
        let _ = graphics.run_if_healthy(|| self.app.logic(ctx));
    }

    fn raw_input_hook(&mut self, ctx: &egui::Context, raw: &mut egui::RawInput) {
        let _ = self.graphics.run_if_healthy(|| {
            // Dropped files belong to this host iteration's raw input, not to stale logic input:
            // read them here once, before the batch is saved as pending for a hidden pass.
            for f in std::mem::take(&mut raw.dropped_files) {
                let request = self.app.import_request("drop");
                let inbox = self.inbox.clone();
                let ctx = ctx.clone();
                wasm_bindgen_futures::spawn_local(async move {
                    let name = f.path().file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_else(|| "dropped".into());
                    match f.bytes_async().await {
                        Ok(bytes) => {
                            inbox.lock().unwrap_or_else(|e| e.into_inner()).push(ImportedFile { request, name, bytes });
                            ctx.request_repaint();
                        }
                        Err(e) => log::error!("couldn't read dropped file {name}: {e}"),
                    }
                });
            }
            designcraft_ui_egui::drop_key_name_text(raw, ctx.text_edit_focused() || self.app.canvas_ime());
            self.app.raw_input_hook(ctx, raw);
        });
    }

    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        let _ = self.graphics.run_if_healthy(|| self.app.ui(ui));
    }

    fn as_any_mut(&mut self) -> Option<&mut dyn std::any::Any> {
        Some(self)
    }
}

fn services(inbox: Inbox, ctx: egui::Context) -> Services {
    let open_inbox = inbox.clone();
    Services {
        open_async: Some(Box::new(move |request: ImportRequest| {
            let inbox = open_inbox.clone();
            let ctx = ctx.clone();
            let dialog = if request.purpose == "swatches" {
                rfd::AsyncFileDialog::new().add_filter("Swatch Exchange (ASE)", &["ase"])
            } else if request.purpose == "shortcuts" {
                rfd::AsyncFileDialog::new().add_filter("Shortcut set (JSON, InDesign text)", &["json", "txt"])
            } else if request.purpose == "place" {
                rfd::AsyncFileDialog::new().add_filter("Graphics", IMAGE_EXTS)
            } else {
                rfd::AsyncFileDialog::new().add_filter("DesignCraft, IDML or InDesign", DOC_EXTS)
            };
            wasm_bindgen_futures::spawn_local(async move {
                let Some(file) = dialog.pick_file().await else {
                    return;
                };
                let bytes = file.read().await;
                inbox.lock().unwrap_or_else(|e| e.into_inner()).push(ImportedFile { request, name: file.file_name(), bytes });
                ctx.request_repaint();
            });
        })),
        write: Some(Box::new(|name: &str, bytes: &[u8]| download(name, bytes))),
        download: Some(Box::new(|name: &str, bytes: &[u8]| download(name, bytes))),
        inbox: Some(inbox),
        ..Default::default()
    }
}

/// Trigger a browser download of `bytes` named after the last component of `path`.
fn download(path: &str, bytes: &[u8]) -> Result<(), String> {
    let js = |e: wasm_bindgen::JsValue| format!("{e:?}");
    let name = std::path::Path::new(path).file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_else(|| "designcraft".into());
    let window = web_sys::window().ok_or("no window")?;
    let document = window.document().ok_or("no document")?;
    let parts = js_sys::Array::of1(&js_sys::Uint8Array::from(bytes));
    let opts = web_sys::BlobPropertyBag::new();
    opts.set_type(mime_for(&name));
    let blob = web_sys::Blob::new_with_u8_array_sequence_and_options(&parts, &opts).map_err(js)?;
    let url = web_sys::Url::create_object_url_with_blob(&blob).map_err(js)?;
    let a: web_sys::HtmlAnchorElement = document.create_element("a").map_err(js)?.dyn_into().map_err(|_| "not an anchor")?;
    a.set_href(&url);
    a.set_download(&name);
    a.style().set_property("display", "none").map_err(js)?;
    let body = document.body().ok_or("no body")?;
    body.append_child(&a).map_err(js)?;
    a.click();
    a.remove();
    // Revoke after the click has been dispatched; the download keeps its own reference.
    let revoke = wasm_bindgen::closure::Closure::once_into_js(move || {
        web_sys::Url::revoke_object_url(&url).ok();
    });
    // The download has started; a missed revoke only keeps the blob until the page closes.
    if let Err(e) = window.set_timeout_with_callback_and_timeout_and_arguments_0(revoke.unchecked_ref(), 10_000) {
        log::warn!("could not schedule revoking the download URL: {}", js(e));
    }
    Ok(())
}
