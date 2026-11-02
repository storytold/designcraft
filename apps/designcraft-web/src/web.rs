//! The browser shell: web `Services`, drag-and-drop, and the eframe web runner.

use super::browser_policy::{has_query_flag, mime_for};
use crate::graphics::{GraphicsState, RecoveryDocument, recovery_documents};
use designcraft_engine::Session;
use designcraft_ui_egui::{DesignApp, ImportRequest, ImportedFile, Inbox, Services};
use wasm_bindgen::JsCast as _;

const DOC_EXTS: &[&str] = &["designcraft", "idml", "indd", "indt"];
/// What Open… offers: the documents, and a zipped package (a document with its Document Fonts).
const OPEN_EXTS: &[&str] = &["designcraft", "idml", "indd", "indt", "zip"];
/// The font files a package's `Document Fonts` folder holds.
const FONT_EXTS: &[&str] = &["ttf", "otf", "ttc", "otc"];
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
                    install_folder_drop(inbox.clone(), cc.egui_ctx.clone());
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
    let (folder_inbox, folder_ctx) = (inbox.clone(), ctx.clone());
    Services {
        pick_folder: Some(Box::new(move |request: ImportRequest| {
            let (inbox, ctx) = (folder_inbox.clone(), folder_ctx.clone());
            wasm_bindgen_futures::spawn_local(async move {
                let files = pick_package_files().await;
                deliver_folder(&inbox, &ctx, &request, files);
            });
        })),
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
                rfd::AsyncFileDialog::new().add_filter("DesignCraft, IDML, InDesign or a zipped package", OPEN_EXTS)
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

/// The most files a folder hands over (picked or dropped), and the most bytes in all.
const MAX_PACKAGE_FILES: usize = 2000;
// The bytes are copied a few times on the way (base64 for the command, then decoded), so the
// cap keeps the peak well inside the browser's memory for a tab.
const MAX_PACKAGE_BYTES: f64 = 384.0 * 1024.0 * 1024.0;
/// The most entries (files and folders) looked at in a dropped folder, and how deep it is walked.
const MAX_PACKAGE_ENTRIES: usize = 20_000;
const MAX_PACKAGE_DEPTH: usize = 16;

/// Whether a file in a folder handed over is read: a document or a font file (a package's
/// document and its Document Fonts). Links and everything else are left unread, as are macOS
/// resource forks (`._Name.ttf`).
fn wanted_in_folder(name: &str) -> bool {
    !name.starts_with("._") && name.rsplit_once('.').is_some_and(|(_, ext)| DOC_EXTS.iter().chain(FONT_EXTS).any(|e| e.eq_ignore_ascii_case(ext)))
}

/// Hand a folder's files ((path in the folder, bytes)) to the app, all at once with `request`
/// (`folder:…`, see `DesignApp::pick_folder`).
fn deliver_folder(inbox: &Inbox, ctx: &egui::Context, request: &ImportRequest, files: Vec<(String, Vec<u8>)>) {
    if files.is_empty() {
        log::warn!("the folder holds no document or font file");
        return;
    }
    inbox.lock().unwrap_or_else(|e| e.into_inner()).extend(files.into_iter().map(|(name, bytes)| ImportedFile {
        request: request.clone(),
        name,
        bytes,
    }));
    ctx.request_repaint();
}

/// The documents and font files ([`wanted_in_folder`]) of a folder the user chooses with the
/// browser's folder picker (an `<input type=file webkitdirectory>`, which every current browser
/// supports), each named by its path in the folder. Only those files are read, up to
/// [`MAX_PACKAGE_FILES`] and [`MAX_PACKAGE_BYTES`]; empty when the picker is cancelled or can't be
/// shown.
async fn pick_package_files() -> Vec<(String, Vec<u8>)> {
    let Some(document) = web_sys::window().and_then(|w| w.document()) else { return Vec::new() };
    let Some(input) = document.create_element("input").ok().and_then(|e| e.dyn_into::<web_sys::HtmlInputElement>().ok()) else {
        return Vec::new();
    };
    input.set_type("file");
    input.set_multiple(true);
    if input.set_attribute("webkitdirectory", "").is_err() {
        return Vec::new();
    }
    // Settles when files are chosen or the picker is cancelled.
    let chosen = js_sys::Promise::new(&mut |resolve, _reject| {
        let done = wasm_bindgen::closure::Closure::<dyn FnMut()>::new(move || {
            let _ = resolve.call0(&wasm_bindgen::JsValue::UNDEFINED);
        });
        let _ = input.add_event_listener_with_callback("change", done.as_ref().unchecked_ref());
        let _ = input.add_event_listener_with_callback("cancel", done.as_ref().unchecked_ref());
        done.forget();
    });
    input.click();
    let _ = wasm_bindgen_futures::JsFuture::from(chosen).await;
    let Some(list) = input.files() else { return Vec::new() };
    let mut out = Vec::new();
    let mut bytes = 0.0;
    for i in 0..list.length() {
        let Some(file) = list.get(i) else { continue };
        let name = file.name();
        if !wanted_in_folder(&name) {
            continue;
        }
        if out.len() >= MAX_PACKAGE_FILES || bytes + file.size() > MAX_PACKAGE_BYTES {
            log::warn!("the folder holds more than can be read at once; the rest is left");
            break;
        }
        bytes += file.size();
        // Its path in the chosen folder says which files are the Document Fonts beside the document.
        let path = js_sys::Reflect::get(&file, &wasm_bindgen::JsValue::from_str("webkitRelativePath"))
            .ok()
            .and_then(|v| v.as_string())
            .filter(|p| !p.is_empty())
            .unwrap_or(name);
        if let Ok(buf) = wasm_bindgen_futures::JsFuture::from(file.array_buffer()).await {
            out.push((path, js_sys::Uint8Array::new(&buf).to_vec()));
        }
    }
    out
}

/// Property `key` of `v`, unless missing.
fn js_get(v: &wasm_bindgen::JsValue, key: &str) -> Option<wasm_bindgen::JsValue> {
    js_sys::Reflect::get(v, &wasm_bindgen::JsValue::from_str(key)).ok().filter(|v| !v.is_undefined() && !v.is_null())
}

/// Method `name` of `v`.
fn js_method(v: &wasm_bindgen::JsValue, name: &str) -> Option<js_sys::Function> {
    js_get(v, name)?.dyn_into::<js_sys::Function>().ok()
}

/// A folder dropped on the page: eframe hands over dropped files only (a folder arrives as an
/// unreadable file), so a drop holding a folder is taken here first, in the capture phase, and
/// its documents and font files go to the app as `folder:drop`. A drop of files only is left to
/// eframe. Every current browser gives a drop's entries (`webkitGetAsEntry`).
fn install_folder_drop(inbox: Inbox, ctx: egui::Context) {
    let Some(window) = web_sys::window() else { return };
    let on_drop = wasm_bindgen::closure::Closure::<dyn FnMut(web_sys::Event)>::new(move |event: web_sys::Event| {
        // The entries are only there while the event is handled: taken now, read after.
        let entries = dropped_folder_entries(&event);
        if entries.is_empty() {
            return;
        }
        event.prevent_default();
        event.stop_immediate_propagation();
        let (inbox, ctx) = (inbox.clone(), ctx.clone());
        wasm_bindgen_futures::spawn_local(async move {
            let files = read_dropped(entries).await;
            deliver_folder(&inbox, &ctx, &ImportRequest { purpose: "folder:drop".into(), ..Default::default() }, files);
        });
    });
    if window.add_event_listener_with_callback_and_bool("drop", on_drop.as_ref().unchecked_ref(), true).is_err() {
        log::warn!("dropped folders can't be read in this browser");
    }
    on_drop.forget();
}

/// The entries of a drop (`DataTransferItem.webkitGetAsEntry`) when one of them is a folder;
/// none otherwise.
fn dropped_folder_entries(event: &web_sys::Event) -> Vec<wasm_bindgen::JsValue> {
    let Some(items) = js_get(event.as_ref(), "dataTransfer").and_then(|dt| js_get(&dt, "items")) else { return Vec::new() };
    let len = js_get(&items, "length").and_then(|l| l.as_f64()).unwrap_or(0.0).clamp(0.0, MAX_PACKAGE_FILES as f64) as u32;
    let mut entries = Vec::new();
    let mut folder = false;
    for i in 0..len {
        let Ok(item) = js_sys::Reflect::get_u32(&items, i) else { continue };
        let Some(entry) = js_method(&item, "webkitGetAsEntry").and_then(|f| f.call0(&item).ok()).filter(|e| !e.is_null() && !e.is_undefined()) else {
            continue;
        };
        folder |= js_get(&entry, "isDirectory").and_then(|v| v.as_bool()).unwrap_or(false);
        entries.push(entry);
    }
    if folder { entries } else { Vec::new() }
}

/// Every entry of a dropped folder (`FileSystemDirectoryReader.readEntries`, read until a batch
/// comes back empty).
async fn read_directory(entry: &wasm_bindgen::JsValue) -> Vec<wasm_bindgen::JsValue> {
    let Some(reader) = js_method(entry, "createReader").and_then(|f| f.call0(entry).ok()) else { return Vec::new() };
    let Some(read) = js_method(&reader, "readEntries") else { return Vec::new() };
    let mut out = Vec::new();
    while out.len() < MAX_PACKAGE_ENTRIES {
        let batch = js_sys::Promise::new(&mut |resolve, reject| {
            let _ = read.call2(&reader, &resolve, &reject);
        });
        let Ok(batch) = wasm_bindgen_futures::JsFuture::from(batch).await else { break };
        let batch = js_sys::Array::from(&batch);
        if batch.length() == 0 {
            break;
        }
        out.extend(batch.iter());
    }
    out
}

/// The file of a dropped file entry (`FileSystemFileEntry.file`).
async fn entry_file(entry: &wasm_bindgen::JsValue) -> Option<web_sys::File> {
    let file = js_method(entry, "file")?;
    let promise = js_sys::Promise::new(&mut |resolve, reject| {
        let _ = file.call2(entry, &resolve, &reject);
    });
    wasm_bindgen_futures::JsFuture::from(promise).await.ok()?.dyn_into::<web_sys::File>().ok()
}

/// The documents and font files ([`wanted_in_folder`]) of dropped entries, folders walked, each
/// named by its path (`fullPath`, without the leading `/`), within the folder limits.
async fn read_dropped(roots: Vec<wasm_bindgen::JsValue>) -> Vec<(String, Vec<u8>)> {
    let mut out = Vec::new();
    let (mut bytes, mut seen) = (0.0, 0usize);
    let mut pending: Vec<(wasm_bindgen::JsValue, usize)> = roots.into_iter().map(|e| (e, 0)).collect();
    while let Some((entry, depth)) = pending.pop() {
        seen += 1;
        if seen > MAX_PACKAGE_ENTRIES {
            log::warn!("the folder holds more than can be read at once; the rest is left");
            break;
        }
        if js_get(&entry, "isDirectory").and_then(|v| v.as_bool()).unwrap_or(false) {
            if depth < MAX_PACKAGE_DEPTH {
                pending.extend(read_directory(&entry).await.into_iter().map(|e| (e, depth + 1)));
            }
            continue;
        }
        let name = js_get(&entry, "name").and_then(|v| v.as_string()).unwrap_or_default();
        if !wanted_in_folder(&name) {
            continue;
        }
        let Some(file) = entry_file(&entry).await else { continue };
        if out.len() >= MAX_PACKAGE_FILES || bytes + file.size() > MAX_PACKAGE_BYTES {
            log::warn!("the folder holds more than can be read at once; the rest is left");
            break;
        }
        bytes += file.size();
        let path = js_get(&entry, "fullPath").and_then(|v| v.as_string()).unwrap_or(name);
        if let Ok(buf) = wasm_bindgen_futures::JsFuture::from(file.array_buffer()).await {
            out.push((path.trim_start_matches('/').to_string(), js_sys::Uint8Array::new(&buf).to_vec()));
        }
    }
    out
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
