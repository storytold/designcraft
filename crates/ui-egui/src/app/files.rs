use super::{DesignApp, ImportRequest, ImportedFile, Value, canvas, dialogs, file_dialogs, json, shortcut_import};
#[cfg(test)]
use super::{Services, Session};
#[cfg(test)]
use crate::Inbox;
#[cfg(test)]
use crate::test_window;

impl DesignApp {
    /// Ask the host for a file to open (`open`) or place (`place`). A native dialog answers on a
    /// later frame ([`Self::open_picked`]); in a browser the file arrives through the inbox.
    #[track_caller]
    pub fn pick_and_open(&mut self, purpose: &str) -> Result<Value, String> {
        if let Some(open) = self.services.open_async.as_mut() {
            let request = ImportRequest { purpose: purpose.into(), target: self.session.active().map(|d| d.uid), wanted: Vec::new() };
            open(request);
            return Ok(Value::Null);
        }
        let bound = if purpose == "place" { file_dialogs::Bound::Document } else { file_dialogs::Bound::App };
        let purpose_owned = purpose.to_string();
        self.ask_open(purpose, bound, move |app, path| app.open_picked(&purpose_owned, path))
    }

    /// Open (`open`) or place (`place`) the file at `path`, chosen in a dialog or passed by a script.
    pub(crate) fn open_picked(&mut self, purpose: &str, path: String) -> Result<Value, String> {
        // Image Import Options: a multi-page PDF asks which page.
        if purpose == "place" && path.to_ascii_lowercase().ends_with(".pdf") {
            let target = self.session.active().map(|d| d.uid);
            let pages = self.services.read.as_mut().and_then(|r| r(&path).ok()).and_then(|b| designcraft_render::pdf_page_count(&b)).unwrap_or(1);
            if pages > 1 {
                self.ui.dialog = Some(dialogs::Dialog::new("pdfImport", json!({"path": path, "page": "1", "pages": pages, "target": target})));
                return Ok(Value::Null);
            }
            return self.place_file(json!({"path": path}));
        }
        if purpose == "place" {
            return self.place_file(json!({"path": path}));
        }
        self.open_file("file.open", json!({"path": path}))
    }

    /// Load Styles: ask the host for a document whose styles to load; `kind` is `paragraph`,
    /// `character` or empty for both. A native dialog's answer opens the dialog that chooses
    /// among the file's styles; in a browser they all load when the file arrives.
    #[track_caller]
    pub fn pick_styles(&mut self, kind: &str) -> Result<Value, String> {
        designcraft_engine::cmd::has_doc(&self.session)?;
        if let Some(open) = self.services.open_async.as_mut() {
            let purpose = if kind.is_empty() { "styles".to_string() } else { format!("styles:{kind}") };
            open(ImportRequest { purpose, target: self.session.active().map(|d| d.uid), wanted: Vec::new() });
            return Ok(Value::Null);
        }
        let kind = kind.to_string();
        self.ask_open("styles", file_dialogs::Bound::Document, move |app, path| {
            let found = app.run("style.load.list", json!({"path": path}))?;
            let of = |k: &str| if kind.is_empty() || kind == k { found[k].clone() } else { json!([]) };
            app.ui.dialog = Some(dialogs::Dialog::new(
                "loadStyles",
                json!({"path": path, "paragraph": of("paragraph"), "character": of("character"), "conflicts": "redefine"}),
            ));
            Ok(Value::Null)
        })
    }

    /// Ask an asynchronous host (web) for files for `purpose`; they arrive through the inbox.
    /// `relink:{asset}` relinks that graphic from the chosen file; `relinkFiles` relinks every
    /// missing graphic from the chosen files of the same names.
    pub fn request_files(&mut self, purpose: &str) {
        let mut request = self.import_request(purpose);
        if purpose == "relinkFiles" {
            // Only the missing graphics' files are read from the chosen folder.
            let rows = self.session.execute("links.list", &json!({})).ok().and_then(|v| v.as_array().cloned()).unwrap_or_default();
            request.wanted = rows
                .iter()
                .filter(|r| r["asset"].is_u64() && r["status"] == "missing")
                .filter_map(|r| r["path"].as_str().or(r["name"].as_str()))
                .map(|p| file_name_of(p).to_lowercase())
                .collect();
            // Nothing to relink: don't have the host read a whole folder for nothing.
            if request.wanted.is_empty() {
                self.status(crate::i18n::tr(&self.ui.language, "No missing graphics to relink"));
                return;
            }
        }
        if let Some(open) = self.services.open_async.as_mut() {
            open(request);
        }
    }

    pub fn import_request(&self, purpose: &str) -> ImportRequest {
        ImportRequest { purpose: purpose.into(), target: self.session.active().map(|d| d.uid), wanted: Vec::new() }
    }

    pub(crate) fn activate_import_target(&mut self, request: &ImportRequest) -> Result<(), String> {
        let Some(index) = request.target.and_then(|uid| self.session.documents().iter().position(|d| d.uid == uid)) else {
            let message = "Import cancelled: the originating document has closed";
            self.status(crate::i18n::tr(&self.ui.language, message));
            return Err(message.into());
        };
        self.run("file.activate", json!({"index": index}))?;
        Ok(())
    }

    pub(crate) fn cancel_pdf_import(&mut self) {
        self.pending_pdf = None;
    }

    pub(crate) fn confirm_pdf_import(&mut self, page: u64, crop: String) -> Result<Value, String> {
        let file = self.pending_pdf.take().ok_or("PDF import cancelled: no pending file")?;
        self.activate_import_target(&file.request)?;
        let params = json!({"name": file.name, "base64": designcraft_engine::cmd::base64_encode(&file.bytes), "pdfPage": page, "pdfCrop": crop});
        if file.request.purpose == "place" { self.place_file(params) } else { self.open_file("file.place", params) }
    }

    /// A single unresolved PDF owns its bytes. Later deliveries cancel explicitly rather than
    /// replacing its options. Cancellation, replacement and target closure release its payload.
    pub(super) fn drain_inbox(&mut self) {
        if let Some(file) = &self.pending_pdf {
            let target_exists = self.session.documents().iter().any(|d| Some(d.uid) == file.request.target);
            let owns_dialog =
                self.ui.dialog.as_ref().is_some_and(|d| d.id == "pdfImport" && d.fields.get("async").and_then(Value::as_bool) == Some(true));
            if !target_exists || !owns_dialog {
                self.pending_pdf = None;
                if !target_exists {
                    if owns_dialog {
                        self.ui.dialog = None;
                    }
                    self.status(crate::i18n::tr(&self.ui.language, "PDF import cancelled: the originating document has closed"));
                }
            }
        }
        let Some(inbox) = self.services.inbox.clone() else { return };
        let files = std::mem::take(&mut *inbox.lock().unwrap_or_else(std::sync::PoisonError::into_inner));
        let files = self.relink_from_inbox(files);
        for file in files {
            if file.request.purpose == "shortcuts" {
                shortcut_import::import_delivered(self, &file.bytes);
                continue;
            }
            if self.ui.dialog.is_some() {
                self.status(
                    crate::i18n::tr(&self.ui.language, "{name}: import cancelled while a dialog is open; choose the file again")
                        .replace("{name}", &file.name),
                );
                continue;
            }
            let name = file.name.clone();
            if let Err(e) = self.import_file(file) {
                self.status(format!("{name}: {e}"));
            }
        }
    }

    fn import_file(&mut self, file: ImportedFile) -> Result<Value, String> {
        let lower = file.name.to_ascii_lowercase();
        if let Some(kind) = file.request.purpose.strip_prefix("styles") {
            self.activate_import_target(&file.request)?;
            let mut params = json!({"name": file.name, "base64": designcraft_engine::cmd::base64_encode(&file.bytes)});
            match kind {
                ":paragraph" => params["character"] = json!([]),
                ":character" => params["paragraph"] = json!([]),
                _ => {}
            }
            return self.run("style.load", params);
        }
        let layout = opens_as_document(&file.name)
            || (std::path::Path::new(&file.name).extension().is_none() && designcraft_engine::cmd::is_document(&file.bytes));
        let place =
            file.request.purpose == "place" || (matches!(file.request.purpose.as_str(), "drop" | "open") && !layout && !lower.ends_with(".ase"));
        if place || lower.ends_with(".ase") || file.request.purpose == "swatches" {
            self.activate_import_target(&file.request)?;
        }
        if place && lower.ends_with(".pdf") {
            let pages = designcraft_render::pdf_page_count(&file.bytes).unwrap_or(1);
            if pages > 1 {
                self.ui.dialog =
                    Some(dialogs::Dialog::new("pdfImport", json!({"async": true, "name": file.name.clone(), "page": "1", "pages": pages})));
                self.pending_pdf = Some(file);
                return Ok(Value::Null);
            }
        }
        let b64 = designcraft_engine::cmd::base64_encode(&file.bytes);
        if lower.ends_with(".ase") || file.request.purpose == "swatches" {
            self.run("swatch.load", json!({"base64": b64}))
        } else if file.request.purpose == "place" {
            self.place_file(json!({"name": file.name, "base64": b64}))
        } else if place {
            self.open_file("file.place", json!({"name": file.name, "base64": b64}))
        } else {
            self.open_file("file.openBytes", json!({"name": file.name, "base64": b64}))
        }
    }

    /// A file dropped on the window: documents open, the swatches in a swatch exchange (.ase) file
    /// are loaded, and anything else is placed on the spread in view (a parent spread while parents
    /// are being edited). A file without an extension opens when its content is a DesignCraft or
    /// IDML document.
    pub fn open_dropped(&mut self, path: &str) -> Result<Value, String> {
        self.open_dropped_at(path, None)
    }

    /// Open or place each file dropped on a floating panel's own window. `pointer` is in the main
    /// window's coordinates.
    #[cfg(not(target_arch = "wasm32"))]
    pub(crate) fn open_dropped_files(&mut self, files: &[egui::DroppedFileHandle], pointer: Option<egui::Pos2>) {
        for f in files {
            let p = f.path().to_string_lossy().to_string();
            if !p.is_empty() {
                let _ = self.open_dropped_at(&p, pointer);
            }
        }
    }

    /// [`Self::open_dropped`] at the pointer: a graphic's top left lands where the file was
    /// dropped on the canvas, text goes to a new frame on the document page there.
    pub fn open_dropped_at(&mut self, path: &str, pointer: Option<egui::Pos2>) -> Result<Value, String> {
        if path.to_ascii_lowercase().ends_with(".ase") {
            return self.run("swatch.load", json!({"path": path}));
        }
        let document = opens_as_document(path)
            || (std::path::Path::new(path).extension().is_none()
                && designcraft_engine::io::read_capped(path, designcraft_engine::cmd::DOCUMENT_SNIFF_BYTES)
                    .is_ok_and(|b| designcraft_engine::cmd::is_document(&b)));
        if document {
            return self.open_file("file.open", json!({"path": path}));
        }
        let mut params = json!({"path": path});
        if let (Some((sr, at)), Some(o)) = (canvas::drop_point(self, pointer), params.as_object_mut()) {
            o.insert("spread".into(), serde_json::to_value(sr).unwrap_or(Value::Null));
            o.insert("x".into(), json!(at.x));
            o.insert("y".into(), json!(at.y));
            if let (designcraft_doc::SpreadRef::Doc(si), Some(st)) = (sr, self.session.active()) {
                let pi = st.doc.spreads.get(si).and_then(|s| s.page_at_x(at.x)).unwrap_or(0);
                o.insert("page".into(), json!(st.doc.first_page_of_spread(si) + pi + 1));
            }
        }
        self.open_file("file.place", params)
    }

    /// Queue files the operating system asked the app to open. They can arrive before the window
    /// exists (the event that launched the app); the first ready frame opens them, each as a new
    /// document through `file.open`, as File › Open does.
    pub fn open_documents_later(&mut self, paths: impl IntoIterator<Item = String>) {
        self.os_documents.extend(paths);
    }

    pub(crate) fn open_os_documents(&mut self) {
        for path in std::mem::take(&mut self.os_documents) {
            if let Err(e) = self.open_file("file.open", json!({"path": path})) {
                log::warn!("{path}: {e}");
            }
        }
    }

    /// File › Place: the file fills the selected empty or graphic frame; with none, an image loads
    /// into the place cursor (click to place at its natural size, drag to draw the frame).
    pub(crate) fn place_file(&mut self, mut params: Value) -> Result<Value, String> {
        if let Some(o) = params.as_object_mut() {
            o.insert("loadCursor".into(), json!(true));
        }
        self.open_file("file.place", params)
    }

    /// Open (`file.open`, `file.openBytes`) or place (`file.place`) a file the user chose. A file
    /// that can't be opened or placed gets an alert with the reason, as well as the status line.
    /// A successful import with warnings keeps the document and shows the warnings in an alert.
    pub fn open_file(&mut self, cmd: &str, params: Value) -> Result<Value, String> {
        let file = match params.get("path").and_then(Value::as_str) {
            Some(path) => std::path::Path::new(path).file_name().map_or_else(|| path.to_string(), |n| n.to_string_lossy().to_string()),
            None => params.get("name").and_then(Value::as_str).unwrap_or_default().to_string(),
        };
        self.open_named_file(cmd, params, file)
    }

    /// [`Self::open_file`] for a command whose params don't name the file: `file` is shown in the
    /// alert or warnings.
    pub fn open_named_file(&mut self, cmd: &str, params: Value, file: String) -> Result<Value, String> {
        let r = self.run(cmd, params);
        if let Err(e) = &r {
            let title = if cmd == "file.place" { "Can't Place the File" } else { "Can't Open the File" };
            if self.ui.dialog.as_ref().is_some_and(|d| d.id == "findChange") {
                self.session.reset_find_scope();
            }
            self.ui.dialog = Some(dialogs::Dialog::new("alert", json!({"title": title, "file": file, "message": e})));
        } else if let Ok(result) = &r {
            // Each warning with the frame it is about (`warningItems`), if any.
            let items = result.get("warningItems").and_then(Value::as_array);
            let (warnings, lines): (Vec<&str>, Vec<Value>) = result
                .get("warnings")
                .and_then(Value::as_array)
                .into_iter()
                .flatten()
                .enumerate()
                .filter_map(|(i, w)| Some((i, w.as_str()?)))
                .filter(|(_, warning)| !warning.is_empty())
                .map(|(i, w)| (w, json!({"text": w, "item": items.and_then(|a| a.get(i)).cloned().unwrap_or(Value::Null)})))
                .unzip();
            if !warnings.is_empty() {
                self.status(format!("{file}: {}", warnings.join("; ")));
                // Find Font, offered for the document's missing fonts, stays up: the status line
                // keeps the warnings.
                if self.ui.dialog.as_ref().is_none_or(|d| d.id != "findFont") {
                    self.ui.dialog = Some(dialogs::Dialog::new(
                        "alert",
                        json!({"title": "Import Warnings", "file": file, "message": warnings.join("\n"), "lines": lines}),
                    ));
                }
            }
        }
        r
    }

    /// Relink graphics from delivered files, returning the files left to import. Files chosen
    /// to relink (`relink:{asset}`, `relinkFiles`) and dropped images named like a missing
    /// graphic of the document they were dropped on relink, each request's files in one undo
    /// step; other dropped files are placed or opened as before.
    fn relink_from_inbox(&mut self, files: Vec<ImportedFile>) -> Vec<ImportedFile> {
        let mut rest = Vec::new();
        // (request, files) in arrival order; a request's files arrive together.
        let mut batches: Vec<(ImportRequest, Vec<ImportedFile>)> = Vec::new();
        for file in files {
            let relink = file.request.purpose.starts_with("relink")
                || (file.request.purpose == "drop"
                    && file.request.target.is_some()
                    && !opens_as_document(&file.name)
                    && designcraft_render::image_size(&file.bytes).is_some());
            if !relink {
                rest.push(file);
                continue;
            }
            match batches.iter_mut().find(|(r, _)| r.purpose == file.request.purpose && r.target == file.request.target) {
                Some((_, b)) => b.push(file),
                None => batches.push((file.request.clone(), vec![file])),
            }
        }
        for (request, batch) in batches {
            let drop = request.purpose == "drop";
            if self.activate_import_target(&request).is_err() {
                if drop {
                    rest.extend(batch);
                }
                continue;
            }
            // A file from a chosen folder is named by its path in it (which tells same-named
            // files apart).
            let list: Vec<Value> = batch
                .iter()
                .map(|f| json!({"name": file_name_of(&f.name), "path": f.name, "base64": designcraft_engine::cmd::base64_encode(&f.bytes)}))
                .collect();
            let mut params = json!({"files": list});
            if let Some(aid) = request.purpose.strip_prefix("relink:").and_then(|a| a.parse::<u64>().ok()) {
                params["asset"] = json!(aid);
            }
            match self.run("links.relinkFiles", params) {
                Ok(v) => {
                    let n = v["relinked"].as_u64().unwrap_or(0);
                    let unsure = v["ambiguous"].as_array().map_or(0, Vec::len);
                    if unsure > 0 {
                        // Files named like graphics they couldn't be told apart for: neither
                        // relinked nor placed.
                        self.status(
                            crate::i18n::tr(&self.ui.language, "Relinked {count}; {unsure} left missing: same-named files couldn't be told apart")
                                .replace("{count}", &n.to_string())
                                .replace("{unsure}", &unsure.to_string()),
                        );
                    } else if n > 0 || !drop {
                        self.status(crate::i18n::tr(&self.ui.language, "Relinked {count}").replace("{count}", &n.to_string()));
                    }
                    // Dropped images that named no missing graphic are placed as before.
                    if drop {
                        let unused: Vec<&str> = v["unused"].as_array().map(|a| a.iter().filter_map(Value::as_str).collect()).unwrap_or_default();
                        rest.extend(batch.into_iter().filter(|f| unused.iter().any(|u| file_name_of(&f.name) == *u)));
                    }
                }
                Err(e) => {
                    self.status(e);
                    if drop {
                        rest.extend(batch);
                    }
                }
            }
        }
        rest
    }

    /// Deliver window actions to the host without persisting them in UI preferences.
    pub(crate) fn flush_window_commands(&mut self, ctx: &egui::Context) {
        for command in std::mem::take(&mut self.pending_window_commands) {
            ctx.send_viewport_cmd(command);
        }
    }
}

/// The last component of a delivered file's name (either kind of separator).
fn file_name_of(name: &str) -> &str {
    name.rsplit(['/', '\\']).next().unwrap_or(name)
}

/// Files that open as documents (when dropped or picked) rather than being placed: DesignCraft,
/// IDML, and InDesign documents and templates (converted to IDML on open).
pub fn opens_as_document(name: &str) -> bool {
    let n = name.to_ascii_lowercase();
    [".designcraft", ".idml", ".indd", ".indt"].iter().any(|ext| n.ends_with(ext))
}

#[cfg(test)]
mod import_warning_tests {
    use super::*;
    use std::io::Write;

    fn idml(missing_parts: bool) -> Vec<u8> {
        let missing = if missing_parts { r#"<idPkg:Spread src="Spreads/missing.xml"/><idPkg:Story src="Stories/missing.xml"/>"# } else { "" };
        let map = format!(
            r#"<Document xmlns:idPkg="http://ns.adobe.com/AdobeInDesign/idml/1.0/packaging" Self="d"><idPkg:Spread src="Spreads/one.xml"/>{missing}</Document>"#
        );
        let spread = r#"<idPkg:Spread xmlns:idPkg="http://ns.adobe.com/AdobeInDesign/idml/1.0/packaging"><Spread Self="s"><Page Self="p" GeometricBounds="0 0 100 100" ItemTransform="1 0 0 1 0 0"/><Rectangle Self="r" ItemTransform="1 0 0 1 0 0"/></Spread></idPkg:Spread>"#;
        let mut zip = zip::ZipWriter::new(std::io::Cursor::new(Vec::new()));
        let options = zip::write::SimpleFileOptions::default().compression_method(zip::CompressionMethod::Stored);
        for (name, text) in [("mimetype", designcraft_idml::MIMETYPE), ("designmap.xml", &map), ("Spreads/one.xml", spread)] {
            zip.start_file(name, options).unwrap();
            zip.write_all(text.as_bytes()).unwrap();
        }
        zip.finish().unwrap().into_inner()
    }

    fn app() -> DesignApp {
        let mut app = DesignApp::new(Session::new(), Services { inbox: Some(Inbox::default()), ..Default::default() });
        app.run("file.new", json!({"title": "Existing work"})).unwrap();
        app.run("frame.create", json!({"rect": [10, 10, 50, 50]})).unwrap();
        app
    }

    fn assert_partial_import_warning(app: &mut DesignApp, file: &str) {
        let dialog = app.ui.dialog.as_ref().expect("partial import must show an alert");
        assert_eq!(dialog.id, "alert");
        assert_eq!(dialog.fields["title"], "Import Warnings");
        assert_eq!(dialog.fields["file"], file);
        assert!(dialog.fields["lines"].as_array().unwrap().iter().all(|l| l["item"].is_null()), "missing parts name no frame");
        for part in ["Spreads/missing.xml", "Stories/missing.xml"] {
            assert!(dialog.fields["message"].as_str().unwrap().contains(part), "every missing part is named");
            assert!(app.ui.status.contains(part), "the status line retains the warning");
        }
        assert_eq!(app.session.documents().len(), 2);
        assert_eq!(app.session.documents()[0].doc.title, "Existing work");
        assert!(app.session.documents()[0].is_dirty(), "existing unsaved work stays intact");
        assert_eq!(app.session.documents()[0].doc.spreads[0].items.len(), 1);
        assert_eq!(app.session.doc().unwrap().doc.spreads.len(), 1);
        assert_eq!(app.session.doc().unwrap().doc.spreads[0].items.len(), 1, "available document content is kept");
        dialogs::confirm(app).unwrap();
        assert!(app.ui.dialog.is_none(), "OK dismisses the warning");
        assert_eq!(app.session.documents().len(), 2, "dismissing the warning keeps the imported document");
    }

    #[test]
    fn byte_open_reports_partial_import_warnings_without_losing_documents() {
        let mut app = app();
        let result =
            app.open_file("file.openBytes", json!({"name": "partial.idml", "base64": designcraft_engine::cmd::base64_encode(&idml(true))})).unwrap();
        assert_eq!(result["warnings"].as_array().unwrap().len(), 2);
        assert_partial_import_warning(&mut app, "partial.idml");
    }

    #[test]
    fn native_open_reports_partial_import_warnings_without_losing_documents() {
        let path = std::env::temp_dir().join(format!("designcraft-partial-import-{}", std::process::id())).join("partial.idml");
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(&path, idml(true)).unwrap();
        let picked = path.to_string_lossy().to_string();
        let mut app = app();
        let result = app.run("app.openDialog", json!({"path": picked}));
        std::fs::remove_file(&path).unwrap();
        std::fs::remove_dir(path.parent().unwrap()).unwrap();
        assert_eq!(result.unwrap()["warnings"].as_array().unwrap().len(), 2);
        assert_partial_import_warning(&mut app, "partial.idml");
    }

    #[test]
    fn inbox_open_reports_partial_import_warnings_without_losing_documents() {
        let mut app = app();
        let request = app.import_request("open");
        app.services.inbox.as_ref().unwrap().lock().unwrap().push(ImportedFile { request, name: "partial.idml".into(), bytes: idml(true) });
        app.drain_inbox();
        assert_partial_import_warning(&mut app, "partial.idml");
    }

    #[test]
    fn find_font_on_open_outranks_the_warning_alert() {
        let mut app = app();
        app.run("frame.create", json!({"rect": [0, 0, 200, 200], "content": "text", "text": "Sample"})).unwrap();
        let d = std::sync::Arc::make_mut(&mut app.session.doc_mut().unwrap().doc);
        let sid = *d.stories.keys().next().unwrap();
        let story = d.story_mut(sid).unwrap();
        story.format_chars(0..6, |f| f.over.font_family = Some("Unavailable Test Family".into()));
        story.paras[0].para.grep_styles = Some(vec![designcraft_doc::GrepStyle { style: "Keep".into(), pattern: "[a-".into() }]);
        let path = std::env::temp_dir().join(format!("designcraft-font-warning-{}.designcraft", std::process::id()));
        app.run("file.save", json!({"path": path})).unwrap();
        let result = app.open_file("file.open", json!({"path": path}));
        std::fs::remove_file(&path).unwrap();
        assert_eq!(result.unwrap()["warnings"].as_array().unwrap().len(), 1);
        assert_eq!(app.ui.dialog.as_ref().unwrap().id, "findFont");
        assert!(app.ui.status.contains("GREP style not applied"), "{}", app.ui.status);
    }

    #[test]
    fn complete_import_does_not_show_a_warning() {
        let mut app = app();
        let result = app
            .open_file("file.openBytes", json!({"name": "complete.idml", "base64": designcraft_engine::cmd::base64_encode(&idml(false))}))
            .unwrap();
        assert_eq!(result["warnings"], json!([]));
        assert!(app.ui.dialog.is_none());
        assert!(app.ui.status.is_empty());
        assert_eq!(app.session.documents().len(), 2);
    }
}

#[cfg(test)]
pub(crate) mod browser_import_tests {
    use super::*;

    fn app() -> DesignApp {
        let mut app = DesignApp::new(Session::new(), Services { inbox: Some(Inbox::default()), ..Default::default() });
        app.run("file.new", json!({"title": "A"})).unwrap();
        app
    }
    /// The placed graphic's link made missing, as an IDML opened in the browser without its
    /// Links folder leaves it.
    fn make_missing(app: &mut DesignApp) -> designcraft_doc::AssetId {
        let aid = *app.session.doc().unwrap().doc.assets.keys().next().unwrap();
        app.session
            .edit(|d, _| {
                let a = std::sync::Arc::make_mut(d.assets.get_mut(&aid).unwrap());
                a.link = Some("/Users/someone/Links/a.svg".into());
                a.name = "a.svg".into();
                a.data = std::sync::Arc::new(Vec::new());
                Ok(())
            })
            .unwrap();
        aid
    }
    fn items(app: &DesignApp) -> usize {
        app.session.doc().unwrap().doc.spreads.iter().map(|s| s.items.len()).sum()
    }

    /// With no missing graphic, Relink to Folder doesn't ask the host for a folder at all.
    #[test]
    fn relink_to_folder_needs_a_missing_graphic() {
        let asked = std::rc::Rc::new(std::cell::RefCell::new(Vec::new()));
        let mut app = app();
        let seen = asked.clone();
        app.services.open_async = Some(Box::new(move |r: ImportRequest| seen.borrow_mut().push(r)));
        let b64 = designcraft_engine::cmd::base64_encode(&image());
        app.run("file.place", json!({"name": "a.svg", "base64": b64, "x": 50, "y": 50})).unwrap();
        app.request_files("relinkFiles");
        assert!(asked.borrow().is_empty());
        assert_eq!(app.ui.status, "No missing graphics to relink");
    }

    /// Relink to Folder asks the host only for the missing graphics' files, and a chosen folder's
    /// files arrive named by their paths in it, which tell same-named files apart.
    #[test]
    fn relink_to_folder_asks_for_the_missing_names_and_matches_by_path() {
        let asked = std::rc::Rc::new(std::cell::RefCell::new(Vec::new()));
        let mut app = app();
        let seen = asked.clone();
        app.services.open_async = Some(Box::new(move |r: ImportRequest| seen.borrow_mut().push(r)));
        let b64 = designcraft_engine::cmd::base64_encode(&image());
        app.run("file.place", json!({"name": "a.svg", "base64": b64, "x": 50, "y": 50})).unwrap();
        let aid = make_missing(&mut app);
        app.request_files("relinkFiles");
        let request = asked.borrow_mut().pop().unwrap();
        assert_eq!(request.wanted, ["a.svg"]);
        // The link is /Users/someone/Links/a.svg: the file in a folder named Links is the one.
        let wrong = br##"<svg xmlns="http://www.w3.org/2000/svg" width="20" height="20"/>"##.to_vec();
        for (name, bytes) in [("Volume/Old/a.svg", wrong), ("Volume/Links/a.svg", image())] {
            app.services.inbox.as_ref().unwrap().lock().unwrap().push(ImportedFile { request: request.clone(), name: name.into(), bytes });
        }
        app.drain_inbox();
        let a = &app.session.doc().unwrap().doc.assets[&aid];
        assert_eq!(*a.data, image(), "the Links folder's file: {}", app.ui.status);
    }

    /// The web build relinks from files it is handed: chosen with Relink to Folder… (several at
    /// once) or dropped. A dropped image named like a missing graphic relinks it instead of
    /// being placed; one named like nothing is placed as before.
    #[test]
    fn delivered_files_relink_missing_graphics() {
        let mut app = app();
        let b64 = designcraft_engine::cmd::base64_encode(&image());
        app.run("file.place", json!({"name": "a.svg", "base64": b64, "x": 50, "y": 50})).unwrap();
        let aid = make_missing(&mut app);
        let n = items(&app);
        let has_data = |app: &DesignApp| !app.session.doc().unwrap().doc.assets[&aid].data.is_empty();
        let request = app.import_request("relinkFiles");
        deliver(&mut app, request, "a.svg", image());
        assert!(has_data(&app), "{}", app.ui.status);
        assert_eq!(app.session.doc().unwrap().doc.assets[&aid].link.as_deref(), Some("/Users/someone/Links/a.svg"));
        make_missing(&mut app);
        let request = app.import_request("drop");
        deliver(&mut app, request, "A.SVG", image());
        assert!(has_data(&app), "a dropped file named like the missing graphic relinks it");
        assert_eq!(items(&app), n, "nothing new is placed");
        // Nothing selected, so a placed image gets a frame of its own.
        app.run("edit.deselectAll", json!({})).unwrap();
        let request = app.import_request("drop");
        deliver(&mut app, request, "other.svg", image());
        assert_eq!(items(&app), n + 1, "a dropped image that names no missing graphic is placed");
        // Relink… for one graphic: the chosen file, whatever its name.
        make_missing(&mut app);
        let request = app.import_request(&format!("relink:{}", aid.0));
        deliver(&mut app, request, "renamed.svg", image());
        assert_eq!(app.session.doc().unwrap().doc.assets[&aid].link.as_deref(), Some("renamed.svg"));
    }

    fn deliver(app: &mut DesignApp, request: ImportRequest, name: &str, bytes: Vec<u8>) {
        app.services.inbox.as_ref().unwrap().lock().unwrap().push(ImportedFile { request, name: name.into(), bytes });
        app.drain_inbox();
    }

    /// A document chosen or dropped in the browser is titled by its file name less one extension,
    /// as on the desktop: the extension comes off once, in the engine.
    #[test]
    fn browser_opens_title_a_document_by_its_name_without_one_extension() {
        let mut app = app();
        let native = designcraft_engine::cmd::base64_decode(app.run("file.serialize", json!({})).unwrap()["base64"].as_str().unwrap());
        for purpose in ["open", "drop"] {
            for (name, title) in [
                ("Brochure.designcraft", "Brochure"),
                ("Brochure.designcraft.designcraft", "Brochure.designcraft"),
                ("Brochure.v2.designcraft", "Brochure.v2"),
                ("日本.DESIGNCRAFT", "日本"),
                (".designcraft", ".designcraft"),
                ("Brochure", "Brochure"),
            ] {
                let request = app.import_request(purpose);
                deliver(&mut app, request, name, native.clone());
                assert_eq!(app.session.doc().unwrap().doc.title, title, "{purpose}: {name}");
            }
        }
        let idml = designcraft_idml::export_idml(&app.session.doc().unwrap().doc);
        for purpose in ["open", "drop"] {
            let request = app.import_request(purpose);
            deliver(&mut app, request, "Brochure.idml", idml.clone());
            assert_eq!(app.session.doc().unwrap().doc.title, "Brochure", "{purpose}: IDML");
        }
    }
    fn image() -> Vec<u8> {
        br##"<svg xmlns="http://www.w3.org/2000/svg" width="10" height="10"><rect width="10" height="10" fill="#ff0000"/></svg>"##.to_vec()
    }
    fn pdf_pages(count: usize) -> Vec<u8> {
        let kids = (0..count).map(|i| format!("{} 0 R", 3 + i * 2)).collect::<Vec<_>>().join(" ");
        let mut objects = vec!["<< /Type /Catalog /Pages 2 0 R >>".to_string(), format!("<< /Type /Pages /Kids [{kids}] /Count {count} >>")];
        for (i, color) in ["1 0 0", "0 1 0", "0 0 1"].iter().take(count).enumerate() {
            let content = format!("{color} rg 0 0 200 200 re f\n");
            objects.push(format!("<< /Type /Page /Parent 2 0 R /MediaBox [0 0 200 200] /Resources << >> /Contents {} 0 R >>", 4 + i * 2));
            objects.push(format!("<< /Length {} >>\nstream\n{content}endstream", content.len()));
        }
        let mut result = "%PDF-1.4\n".to_string();
        let mut offsets = Vec::new();
        for (i, object) in objects.iter().enumerate() {
            offsets.push(result.len());
            result.push_str(&format!("{} 0 obj\n{object}\nendobj\n", i + 1));
        }
        let xref = result.len();
        result.push_str(&format!("xref\n0 {}\n0000000000 65535 f \n", offsets.len() + 1));
        for offset in offsets {
            result.push_str(&format!("{offset:010} 00000 n \n"));
        }
        result.push_str(&format!("trailer\n<< /Size {} /Root 1 0 R >>\nstartxref\n{xref}\n%%EOF\n", objects.len() + 1));
        result.into_bytes()
    }
    pub(crate) fn pdf() -> Vec<u8> {
        pdf_pages(3)
    }

    #[test]
    fn picker_receives_context_before_returning_and_cancel_does_not_edit() {
        let captured = std::sync::Arc::new(std::sync::Mutex::new(None));
        let copy = captured.clone();
        let mut app = app();
        app.services.open_async = Some(Box::new(move |request| *copy.lock().unwrap() = Some(request)));
        let uid = app.session.doc().unwrap().uid;
        app.pick_and_open("place").unwrap();
        app.run("file.new", json!({"title": "B"})).unwrap();
        let request = captured.lock().unwrap().take().unwrap();
        assert_eq!(request.purpose, "place");
        assert_eq!(request.target, Some(uid));
        assert!(app.session.documents().iter().all(|d| d.doc.assets.is_empty()));
        deliver(&mut app, request, "broken.pdf", b"not a PDF".to_vec());
        assert!(app.session.documents().iter().all(|d| d.doc.assets.is_empty()));
        assert!(app.pending_pdf.is_none());
    }

    #[test]
    fn delayed_place_activates_original_document_and_undo_belongs_to_it() {
        let mut app = app();
        let request = app.import_request("place");
        app.run("file.new", json!({"title": "B"})).unwrap();
        deliver(&mut app, request, "red.svg", image());
        assert_eq!(app.session.active_index(), Some(0));
        assert_eq!(app.session.documents()[0].doc.assets.len(), 1);
        assert!(app.session.documents()[1].doc.assets.is_empty());
        app.run("edit.undo", json!({})).unwrap();
        assert!(app.session.doc().unwrap().doc.assets.is_empty());
        app.run("edit.redo", json!({})).unwrap();
        assert_eq!(app.session.doc().unwrap().doc.assets.len(), 1);
    }

    #[test]
    fn closed_target_is_not_replaced_by_a_reused_tab_index() {
        let mut app = app();
        let request = app.import_request("place");
        app.run("file.close", json!({})).unwrap();
        app.run("file.new", json!({"title": "replacement"})).unwrap();
        deliver(&mut app, request, "red.svg", image());
        assert!(app.session.doc().unwrap().doc.assets.is_empty());
    }

    #[test]
    fn closing_earlier_tab_does_not_shift_import_target() {
        let mut app = app();
        app.run("file.new", json!({"title": "target"})).unwrap();
        let request = app.import_request("place");
        app.run("file.new", json!({"title": "other"})).unwrap();
        app.run("file.close", json!({"index": 0})).unwrap();
        deliver(&mut app, request, "red.svg", image());
        assert_eq!(app.session.doc().unwrap().doc.title, "target");
        assert_eq!(app.session.documents()[0].doc.assets.len(), 1);
        assert!(app.session.documents()[1].doc.assets.is_empty());
    }

    #[test]
    fn layout_place_and_open_preserve_explicit_intent() {
        let mut source = Session::new();
        source.execute("file.new", &json!({"title": "source"})).unwrap();
        source.execute("frame.create", &json!({"rect": [10, 10, 50, 50], "content": "text", "text": "placed"})).unwrap();
        let native = designcraft_engine::cmd::to_bytes(&source.doc().unwrap().doc).unwrap();
        let idml = designcraft_idml::export_idml(&source.doc().unwrap().doc);
        for (name, bytes) in [("source.designcraft", native), ("source.idml", idml)] {
            let mut app = app();
            let request = app.import_request("place");
            deliver(&mut app, request, name, bytes.clone());
            assert_eq!(app.session.documents().len(), 1);
            assert_eq!(app.session.doc().unwrap().doc.title, "A");
            assert!(app.session.doc().unwrap().doc.stories.values().any(|s| s.text == "placed"));
            app.run("edit.undo", json!({})).unwrap();
            assert!(app.session.doc().unwrap().doc.stories.is_empty());
            app.run("edit.redo", json!({})).unwrap();
            assert!(!app.session.doc().unwrap().doc.stories.is_empty());
            let request = app.import_request("open");
            deliver(&mut app, request, name, bytes);
            assert_eq!(app.session.documents().len(), 2);
        }
    }

    #[test]
    fn pdf_options_keep_bytes_target_and_selected_page() {
        let mut app = app();
        let bytes = pdf();
        let request = app.import_request("place");
        deliver(&mut app, request, "three.pdf", bytes.clone());
        assert!(app.session.doc().unwrap().doc.assets.is_empty());
        assert_eq!(app.ui.dialog.as_ref().unwrap().id, "pdfImport");
        app.run("file.new", json!({"title": "B"})).unwrap();
        app.ui.dialog.as_mut().unwrap().fields.insert("page".into(), json!("2"));
        dialogs::confirm(&mut app).unwrap();
        assert_eq!(app.session.active_index(), Some(0));
        let asset = app.session.doc().unwrap().doc.assets.values().next().unwrap();
        assert_eq!(asset.page, 1);
        assert_eq!(*asset.data, bytes);
        assert!(app.pending_pdf.is_none());
        assert!(app.session.documents()[1].doc.assets.is_empty());
        app.run("place.drop", json!({"x": 36, "y": 36})).unwrap();
        let exported = app.run("file.exportPdf", json!({})).unwrap();
        let exported = designcraft_engine::cmd::base64_decode(exported["base64"].as_str().unwrap());
        let pixels = designcraft_render::render_pdf_page(&exported, 0, 400).unwrap();
        assert!(pixels.data().iter().filter(|p| p.g > 240 && p.r < 10 && p.b < 10).count() > 1000);
        assert_eq!(pixels.data().iter().filter(|p| p.r > 240 && p.g < 10 && p.b < 10).count(), 0);
    }

    #[test]
    fn pdf_options_refuse_a_page_range_instead_of_placing_page_one() {
        for spec in ["4-5", "2,5", "2-3", "2.5", "-3", "0", "nan", "inf"] {
            let mut app = app();
            let request = app.import_request("place");
            deliver(&mut app, request, "three.pdf", pdf());
            app.ui.dialog.as_mut().unwrap().fields.insert("page".into(), json!(spec));
            assert!(dialogs::confirm(&mut app).is_err(), "{spec}");
            assert!(app.session.doc().unwrap().doc.assets.is_empty(), "{spec}");
            assert_eq!(app.ui.dialog.as_ref().unwrap().id, "pdfImport", "{spec}");
        }
    }

    #[test]
    fn place_loads_the_cursor_unless_a_frame_can_take_the_graphic() {
        let mut app = app();
        let items = |app: &DesignApp| app.session.doc().unwrap().doc.spreads[0].items.len();
        // Nothing selected: the graphic waits in the place cursor; Escape unloads it.
        let request = app.import_request("place");
        deliver(&mut app, request, "red.svg", image());
        assert_eq!(app.session.tool_id(), "placeGun");
        assert!(app.session.loaded.is_some());
        assert_eq!(items(&app), 0);
        let mods = designcraft_tools::Mods::default();
        app.session.tool_key(designcraft_tools::ToolKey::Escape, mods, designcraft_engine::ViewInfo::at_zoom(1.0)).unwrap();
        assert_eq!(app.session.tool_id(), "selection");
        assert!(app.session.loaded.is_none());
        // A click with the loaded cursor places the graphic at its natural size.
        let request = app.import_request("place");
        deliver(&mut app, request, "red.svg", image());
        app.run("place.drop", json!({"x": 100, "y": 100})).unwrap();
        let it = app.session.doc().unwrap().doc.spreads[0].items.last().unwrap();
        assert_eq!(it.bounds(), designcraft_geom::Rect::new(100.0, 100.0, 107.5, 107.5), "10 CSS px = 7.5 pt");
        // An empty graphic frame selected: the graphic fills it.
        let f = app.run("frame.create", json!({"rect": [200, 200, 300, 300], "content": "graphic"})).unwrap()["id"].as_u64().unwrap();
        app.run("selection.set", json!({"ids": [f]})).unwrap();
        let request = app.import_request("place");
        deliver(&mut app, request, "red.svg", image());
        assert_eq!(app.session.tool_id(), "selection");
        assert!(app.session.loaded.is_none());
        assert_eq!(items(&app), 2);
        let frame = app.session.doc().unwrap().doc.item(designcraft_doc::ItemId(f)).unwrap();
        assert!(matches!(frame.content, designcraft_doc::Content::Graphic(_)));
    }

    #[test]
    fn pdf_options_reject_oversized_page_without_editing() {
        let mut app = app();
        for page in [0_u64, 4, 4_294_967_296, 4_294_967_297, u64::MAX] {
            let request = app.import_request("place");
            deliver(&mut app, request, "three.pdf", pdf());
            app.ui.dialog.as_mut().unwrap().fields.insert("page".into(), json!(page.to_string()));
            // The native dialog clamps zero to one; positive invalid pages must fail.
            if page == 0 {
                assert!(app.confirm_pdf_import(page, "crop".into()).is_err());
            } else {
                assert!(dialogs::confirm(&mut app).is_err());
            }
            app.ui.dialog = None;
            assert!(app.session.doc().unwrap().doc.assets.is_empty());
            assert!(app.pending_pdf.is_none());
        }
        let request = app.import_request("place");
        deliver(&mut app, request, "three.pdf", pdf());
        app.ui.dialog.as_mut().unwrap().fields.insert("page".into(), json!("2"));
        dialogs::confirm(&mut app).unwrap();
        assert_eq!(app.session.doc().unwrap().doc.assets.values().next().unwrap().page, 1);
    }

    #[test]
    fn drop_routing_and_nonlayout_open_remain_compatible() {
        let mut app = app();
        let native = designcraft_engine::cmd::to_bytes(&app.session.doc().unwrap().doc).unwrap();
        let request = app.import_request("drop");
        deliver(&mut app, request, "document.designcraft", native);
        assert_eq!(app.session.documents().len(), 2);
        let request = app.import_request("drop");
        app.run("file.activate", json!({"index": 0})).unwrap();
        deliver(&mut app, request, "red.svg", image());
        assert_eq!(app.session.active_index(), Some(1));
        assert_eq!(app.session.doc().unwrap().doc.assets.len(), 1);
        let request = app.import_request("open");
        deliver(&mut app, request, "single.pdf", pdf_pages(1));
        assert_eq!(app.session.documents().len(), 2);
        assert!(app.pending_pdf.is_none());
        assert!(app.ui.dialog.is_none());
    }

    #[test]
    fn ase_swatches_keep_purpose_and_target() {
        let mut source = Session::new();
        source.execute("file.new", &json!({})).unwrap();
        source.execute("swatch.create", &json!({"name": "Import Teal", "color": "#108080"})).unwrap();
        let data = source.execute("swatch.save", &json!({"names": ["Import Teal"]})).unwrap();
        let bytes = designcraft_engine::cmd::base64_decode(data["base64"].as_str().unwrap());
        for (purpose, name) in [("swatches", "picker-file"), ("drop", "palette.ase")] {
            let mut app = app();
            let request = app.import_request(purpose);
            app.run("file.new", json!({"title": "B"})).unwrap();
            deliver(&mut app, request, name, bytes.clone());
            assert_eq!(app.session.active_index(), Some(0));
            assert!(app.session.documents()[0].doc.swatches.iter().any(|s| s.name == "Import Teal"));
            assert!(!app.session.documents()[1].doc.swatches.iter().any(|s| s.name == "Import Teal"));
        }
    }

    #[test]
    fn native_pdf_dialog_keeps_path_and_uid_at_confirmation() {
        let mut app = app();
        let path = std::env::temp_dir().join(format!("designcraft-native-import-{}-{}.pdf", std::process::id(), app.session.doc().unwrap().uid));
        std::fs::write(&path, pdf()).unwrap();
        let pick_path = path.to_string_lossy().to_string();
        app.answer_dialogs(crate::app::file_dialogs::testing::answer_with(Some(pick_path)));
        app.services.read = Some(Box::new(|path| std::fs::read(path).map_err(|e| e.to_string())));
        app.pick_and_open("place").unwrap();
        app.settle_dialogs();
        assert_eq!(app.ui.dialog.as_ref().unwrap().id, "pdfImport");
        app.run("file.new", json!({"title": "B"})).unwrap();
        app.ui.dialog.as_mut().unwrap().fields.insert("page".into(), json!("2"));
        dialogs::confirm(&mut app).unwrap();
        assert_eq!(app.session.active_index(), Some(0));
        assert_eq!(app.session.doc().unwrap().doc.assets.values().next().unwrap().page, 1);
        assert!(app.session.documents()[1].doc.assets.is_empty());
        std::fs::remove_file(path).unwrap();
    }

    #[test]
    fn pending_pdf_cancel_close_supersession_and_second_delivery_release_bytes() {
        let mut app = app();
        let request = app.import_request("place");
        deliver(&mut app, request.clone(), "first.pdf", pdf());
        deliver(&mut app, request.clone(), "second.svg", image());
        assert_eq!(app.pending_pdf.as_ref().unwrap().name, "first.pdf");
        assert!(app.session.doc().unwrap().doc.assets.is_empty());
        app.ui.dialog = None;
        app.drain_inbox();
        assert!(app.pending_pdf.is_none());
        deliver(&mut app, request.clone(), "first.pdf", pdf());
        app.ui.dialog = Some(dialogs::Dialog::new("newDocument", json!({})));
        app.drain_inbox();
        assert!(app.pending_pdf.is_none());
        app.ui.dialog = None;
        deliver(&mut app, request, "first.pdf", pdf());
        app.run("file.close", json!({})).unwrap();
        app.run("file.new", json!({})).unwrap();
        assert!(dialogs::confirm(&mut app).is_err());
        assert!(app.pending_pdf.is_none());
        assert!(app.session.doc().unwrap().doc.assets.is_empty());
    }
}

#[cfg(test)]
mod load_styles_tests {
    use super::*;

    /// A client's document on disk: Head based on Body, and a Lead character style.
    fn client_bytes() -> Vec<u8> {
        let mut s = Session::new();
        s.execute("file.new", &json!({})).unwrap();
        s.execute("style.paragraph.create", &json!({"name": "Body", "chars": {"size": 10}})).unwrap();
        s.execute("style.paragraph.create", &json!({"name": "Head", "basedOn": "Body", "chars": {"size": 20}})).unwrap();
        s.execute("style.character.create", &json!({"name": "Lead", "chars": {"size": 9}})).unwrap();
        designcraft_engine::cmd::base64_decode(s.execute("file.serialize", &json!({})).unwrap()["base64"].as_str().unwrap())
    }

    /// An app with a document that has its own 8 pt Body, and a picker that answers with the client's file.
    fn app_and_file(test: &str) -> (DesignApp, std::path::PathBuf) {
        let path = std::env::temp_dir().join(format!("designcraft-load-styles-{}-{test}.designcraft", std::process::id()));
        std::fs::write(&path, client_bytes()).unwrap();
        let picked = path.to_string_lossy().to_string();
        let mut app = DesignApp::new(Session::new(), Services::default());
        app.answer_dialogs(crate::app::file_dialogs::testing::answer_with(Some(picked)));
        app.run("file.new", json!({})).unwrap();
        app.run("style.paragraph.create", json!({"name": "Body", "chars": {"size": 8}})).unwrap();
        (app, path)
    }

    fn names(app: &DesignApp) -> (Vec<String>, Vec<String>) {
        let st = &app.session.doc().unwrap().doc.styles;
        (st.paragraph.iter().skip(2).map(|s| s.name.clone()).collect(), st.character.iter().skip(1).map(|s| s.name.clone()).collect())
    }

    #[test]
    fn load_all_text_styles_asks_which_styles_of_the_picked_file_to_load() {
        let (mut app, path) = app_and_file("all");
        app.run("app.loadTextStyles", json!({})).unwrap();
        app.settle_dialogs();
        let d = app.ui.dialog.as_mut().expect("the Load Styles dialog");
        let row = |name: &str, conflict: bool| json!({"name": name, "conflict": conflict});
        assert_eq!(d.id, "loadStyles");
        assert_eq!(d.fields["paragraph"], json!([row("[Basic Paragraph]", false), row("Body", true), row("Head", false)]));
        assert_eq!(d.fields["character"], json!([row("Lead", false)]));
        // Lead is unchecked; the rest loads, and Body takes the file's definition.
        d.fields["character"][0]["on"] = json!(false);
        dialogs::confirm(&mut app).unwrap();
        std::fs::remove_file(&path).unwrap();
        assert!(app.ui.dialog.is_none());
        assert_eq!(names(&app), (vec!["Body".to_string(), "Head".to_string()], vec![]));
        assert_eq!(app.session.doc().unwrap().doc.styles.para("Body").unwrap().chars.size, Some(10.0));
    }

    #[test]
    fn the_dialog_applies_the_chosen_conflict_rule() {
        let (mut app, path) = app_and_file("rename");
        app.run("app.loadParagraphStyles", json!({})).unwrap();
        app.settle_dialogs();
        let d = app.ui.dialog.as_mut().unwrap();
        assert_eq!(d.fields["character"], json!([]), "Load Paragraph Styles offers no character styles");
        assert_eq!(d.fields["conflicts"], "redefine");
        d.fields.insert("conflicts".into(), json!("autoRename"));
        dialogs::confirm(&mut app).unwrap();
        std::fs::remove_file(&path).unwrap();
        assert_eq!(names(&app), (vec!["Body".to_string(), "Body 2".to_string(), "Head".to_string()], vec![]));
        assert_eq!(app.session.doc().unwrap().doc.styles.para("Body").unwrap().chars.size, Some(8.0));
    }

    #[test]
    fn load_character_styles_offers_only_character_styles() {
        let (mut app, path) = app_and_file("character");
        app.run("app.loadCharacterStyles", json!({})).unwrap();
        app.settle_dialogs();
        assert_eq!(app.ui.dialog.as_ref().unwrap().fields["paragraph"], json!([]));
        dialogs::confirm(&mut app).unwrap();
        std::fs::remove_file(&path).unwrap();
        assert_eq!(names(&app), (vec!["Body".to_string()], vec!["Lead".to_string()]));
    }

    /// A script can put anything into the dialog's rows: drawing and ticking them must not panic.
    #[test]
    fn the_dialog_draws_and_ticks_rows_that_are_no_style_rows() {
        use egui_kittest::kittest::Queryable as _;
        let mut h = test_window::open(DesignApp::new(Session::new(), Services::default()), egui::vec2(1200.0, 800.0));
        h.state_mut().app.ui.dialog = Some(dialogs::Dialog::new(
            "loadStyles",
            json!({"path": "x", "conflicts": 3, "paragraph": [1, "Body", null, {"name": 7}, {"name": "Head", "conflict": true}], "character": "none"}),
        ));
        h.run_steps(3);
        let button = h.get_by_label("Deselect All").rect().center();
        test_window::click_at(&mut h, button);
        let d = h.state().app.ui.dialog.as_ref().unwrap();
        assert_eq!(d.fields["paragraph"][4]["on"], false);
        assert_eq!(d.fields["paragraph"][0], 1, "what is no style row stays as it is");
        assert!(dialogs::confirm(&mut h.state_mut().app).is_err(), "there is no file `x`");
    }

    #[test]
    fn no_file_or_no_document_opens_no_dialog() {
        let (mut app, path) = app_and_file("none");
        std::fs::remove_file(&path).unwrap();
        // The picked file can't be read: the status line says so.
        app.run("app.loadTextStyles", json!({})).unwrap();
        app.settle_dialogs();
        assert!(app.ui.dialog.as_ref().is_none_or(|d| d.id != "loadStyles"));
        assert!(!app.ui.status.is_empty());
        // The picker is cancelled.
        app.ui.dialog = None;
        app.answer_dialogs(crate::app::file_dialogs::testing::answer_with(None));
        app.run("app.loadTextStyles", json!({})).unwrap();
        app.settle_dialogs();
        assert!(app.ui.dialog.is_none());
        // No document: the picker isn't shown at all.
        app.run("file.close", json!({})).unwrap();
        app.answer_dialogs(|_| panic!("asked for a file without a document"));
        assert!(app.run("app.loadTextStyles", json!({})).is_err());
        app.settle_dialogs();
    }

    #[test]
    fn styles_from_an_asynchronous_picker_load_into_the_document_that_asked() {
        let asked = std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
        let seen = asked.clone();
        let services = Services {
            inbox: Some(Inbox::default()),
            open_async: Some(Box::new(move |request| seen.lock().unwrap().push(request))),
            ..Default::default()
        };
        let mut app = DesignApp::new(Session::new(), services);
        app.run("file.new", json!({"title": "A"})).unwrap();
        app.run("app.loadCharacterStyles", json!({})).unwrap();
        let request = asked.lock().unwrap().pop().expect("the picker was asked");
        assert_eq!(request.purpose, "styles:character");
        // Another document is in front when the file arrives.
        app.run("file.new", json!({"title": "B"})).unwrap();
        app.services.inbox.as_ref().unwrap().lock().unwrap().push(ImportedFile { request, name: "client.designcraft".into(), bytes: client_bytes() });
        app.drain_inbox();
        assert_eq!(app.session.active_index(), Some(0));
        assert_eq!(names(&app), (vec![], vec!["Lead".to_string()]));
        assert!(app.session.documents()[1].doc.styles.char_style("Lead").is_none());
    }
}
