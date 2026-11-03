//! File and message dialogs that never block the frame (craft-dialogs).
//!
//! A blocking dialog runs the platform's modal loop inside the frame. On macOS an event that
//! arrives meanwhile (a file dragged from Finder and held over the window) re-enters winit's
//! handler and aborts the app. So a command that needs a path asks for it with
//! [`DesignApp::ask_open`] or [`DesignApp::ask_save`] and returns; the host shows the dialog at the
//! end of the frame ([`DesignApp::show_file_dialogs`]), and the continuation runs with the chosen
//! path on a later frame. A command given a path (control channel, scripts, tests) never asks.
//!
//! The browser build has no native dialogs: Open goes through `Services::open_async` and the
//! inbox, and Save hands the suggested name to `Services::download`.

use super::{DesignApp, Value, json};

/// What runs with the chosen path, on the frame the answer arrives.
#[cfg(not(target_arch = "wasm32"))]
pub(crate) type Then = Box<dyn FnOnce(&mut DesignApp, String) -> Result<Value, String>>;

/// Whether an answer still needs the document that was active when the dialog was asked for.
#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum Bound {
    /// The continuation edits or reads the active document: it runs in that document, and not
    /// at all once it has closed.
    Document,
    /// The continuation doesn't depend on a document (opening one, a library, a book).
    App,
}

/// Which dialog was asked for, by whom and for which document. Asking again with all three the
/// same (File › Open chosen twice) brings the open panel to the front instead of queueing a
/// second one; anything else queues.
#[cfg(not(target_arch = "wasm32"))]
#[derive(Clone, PartialEq)]
pub(crate) struct AskKey {
    request: craft_dialogs::Request,
    /// The document active when the dialog was asked for, when the answer depends on it.
    doc: Option<u64>,
    /// The code that asked (`#[track_caller]` through the asking helpers): Place, Place with
    /// Options and Choose Image share one dialog but not what they do with its answer.
    origin: &'static std::panic::Location<'static>,
}

/// A dialog's continuation: what it was asked for and what its answer does.
#[cfg(not(target_arch = "wasm32"))]
pub struct Asked {
    key: AskKey,
    /// The egui dialog open when it was asked for (Color Settings, Find): its answer may update or
    /// replace that one, but waits while any other is open.
    dialog: Option<String>,
    step: Step,
}

/// An answer waiting for the egui dialog opened since it was asked for to close, so it doesn't
/// replace it (an unsaved-changes question, Preferences with unsaved edits).
#[cfg(not(target_arch = "wasm32"))]
pub(crate) struct Held {
    doc: Option<u64>,
    dialog: Option<String>,
    then: Then,
    path: String,
}

#[cfg(not(target_arch = "wasm32"))]
enum Step {
    /// Open: run `then` with the file chosen.
    Open(Then),
    /// Save: `ext` is added to a name typed without it, then `then` runs, after the replace-file
    /// question when the added extension names a file that exists.
    Save { ext: Option<String>, then: Then },
    /// The replace-file question about `path`: Yes runs `then` with it.
    Replace { path: String, then: Then },
}

#[cfg(not(target_arch = "wasm32"))]
pub type Dialogs = craft_dialogs::Dialogs<Asked>;

/// A dialog that can't show (no window, no platform dialog) and the reason.
#[cfg(not(target_arch = "wasm32"))]
const DIALOG_UNAVAILABLE: &str = "The file dialog couldn't be shown";

/// The browser build has no dialog for this purpose.
#[cfg(target_arch = "wasm32")]
const NO_PICKER: &str = "needs a file picker";

/// The dialogs, with a wake-up that repaints once the first frame has given it the context.
#[cfg(not(target_arch = "wasm32"))]
pub(crate) fn new_dialogs(ctx: std::sync::Arc<std::sync::OnceLock<egui::Context>>) -> Dialogs {
    craft_dialogs::Dialogs::new(std::sync::Arc::new(move || {
        if let Some(ctx) = ctx.get() {
            ctx.request_repaint();
        }
    }))
}

impl DesignApp {
    /// Ask for a file to open for `purpose` (`open`, `place`, `relink`…: it sets the file types).
    /// `then` runs with its path on a later frame; Cancel runs nothing.
    #[track_caller]
    pub(crate) fn ask_open(
        &mut self,
        purpose: &str,
        bound: Bound,
        then: impl FnOnce(&mut DesignApp, String) -> Result<Value, String> + 'static,
    ) -> Result<Value, String> {
        #[cfg(not(target_arch = "wasm32"))]
        {
            let mut dialog = craft_dialogs::FileDialog::open();
            for filter in open_filters(purpose) {
                dialog = dialog.filter(filter.name, filter.extensions);
            }
            self.ask_file(dialog, bound, Step::Open(Box::new(then)));
            Ok(Value::Null)
        }
        #[cfg(target_arch = "wasm32")]
        {
            let _ = (purpose, bound, then);
            Err(NO_PICKER.into())
        }
    }

    /// Ask where to save `name`: beside the active document once it has been saved, else in the
    /// folder of the last file saved or exported (#1653). `then` runs with the path on a later
    /// frame; Cancel runs nothing. In a browser the name is the download's and `then` runs now.
    #[track_caller]
    pub(crate) fn ask_save(
        &mut self,
        name: &str,
        bound: Bound,
        then: impl FnOnce(&mut DesignApp, String) -> Result<Value, String> + 'static,
    ) -> Result<Value, String> {
        if self.services.download.is_some() {
            return then(self, name.to_string());
        }
        #[cfg(not(target_arch = "wasm32"))]
        {
            let suggested = self.suggested_save_path(name);
            let (folder, name) = save_dialog_start(&suggested);
            let ext = suggested_extension(&name).map(str::to_string);
            let mut dialog = craft_dialogs::FileDialog::save().file_name(name.as_str());
            if let Some(folder) = folder {
                dialog = dialog.directory(folder);
            }
            if let Some(ext) = &ext {
                dialog = dialog.filter(ext.to_ascii_uppercase(), &[ext.as_str()]);
            }
            self.ask_file(dialog, bound, Step::Save { ext, then: Box::new(then) });
            Ok(Value::Null)
        }
        #[cfg(target_arch = "wasm32")]
        {
            let _ = (bound, then);
            Err(NO_PICKER.into())
        }
    }

    /// `name` after the folder a save dialog starts in. Only the last part of a name goes after
    /// the folder: a title may hold a slash, and an empty name would make the folder itself the
    /// suggested file.
    #[cfg(not(target_arch = "wasm32"))]
    pub(crate) fn suggested_save_path(&self, name: &str) -> String {
        use std::path::{Path, PathBuf};
        let folder = self
            .session
            .active()
            .and_then(|d| d.path.as_deref())
            .and_then(folder_of)
            .or_else(|| self.ui.last_save_folder.as_ref().map(PathBuf::from));
        match (folder, Path::new(name).file_name()) {
            (Some(f), Some(file)) => f.join(file).to_string_lossy().to_string(),
            _ => name.to_string(),
        }
    }

    /// Queues `dialog` unless the same code already asked for the same one for the same document
    /// and it is still showing or waiting: choosing File › Open again while its panel is up brings
    /// that panel to the front instead of showing a second one.
    #[cfg(not(target_arch = "wasm32"))]
    #[track_caller]
    fn ask_file(&mut self, dialog: craft_dialogs::FileDialog, bound: Bound, step: Step) {
        let key =
            AskKey { request: craft_dialogs::Request::File(dialog.clone()), doc: self.bound_doc(bound), origin: std::panic::Location::caller() };
        if self.asked.contains(&key) {
            self.file_dialogs.raise();
            return;
        }
        self.asked.push(key.clone());
        let dialog_open = self.ui.dialog.as_ref().map(|d| d.id.clone());
        self.file_dialogs.ask(dialog, Asked { key, dialog: dialog_open, step });
    }

    /// A dialog is showing or waiting to show, or its answer hasn't run yet.
    pub(crate) fn file_dialogs_pending(&self) -> bool {
        #[cfg(not(target_arch = "wasm32"))]
        return !self.asked.is_empty() || !self.held_answers.is_empty();
        #[cfg(target_arch = "wasm32")]
        false
    }

    #[cfg(not(target_arch = "wasm32"))]
    fn bound_doc(&self, bound: Bound) -> Option<u64> {
        match bound {
            Bound::Document => self.session.active().map(|d| d.uid),
            Bound::App => None,
        }
    }

    /// Shows the next dialog asked for. The host calls it at the end of every frame with its
    /// window: on macOS Save and message boxes are sheets on it.
    #[cfg(not(target_arch = "wasm32"))]
    pub fn show_file_dialogs<W: raw_window_handle::HasWindowHandle + raw_window_handle::HasDisplayHandle>(&mut self, parent: Option<&W>) {
        self.file_dialogs.show_pending(parent);
    }

    /// Runs the continuations of the dialogs that have answered, and of answers held until the
    /// egui dialog in their way has closed.
    #[cfg(not(target_arch = "wasm32"))]
    pub(crate) fn handle_file_dialog_answers(&mut self) {
        use craft_dialogs::{Answer, MessageResult};
        for held in std::mem::take(&mut self.held_answers) {
            self.continue_in(held);
        }
        for (asked, answer) in self.file_dialogs.take_answers() {
            let Asked { key, dialog, step } = asked;
            if let Some(i) = self.asked.iter().position(|k| *k == key) {
                self.asked.remove(i);
            }
            let doc = key.doc;
            match (step, answer) {
                (_, Answer::Unavailable(why)) => self.status_error(DIALOG_UNAVAILABLE, why),
                (Step::Open(then), Answer::Paths(paths)) => {
                    if let Some(path) = paths.into_iter().next() {
                        self.continue_in(Held { doc, dialog, then, path: path.to_string_lossy().to_string() });
                    }
                }
                (Step::Save { ext, then }, Answer::Paths(paths)) => {
                    let Some(picked) = paths.into_iter().next() else { continue };
                    let path = picked_save_path(picked.clone(), ext.as_deref());
                    if path != picked && path.exists() {
                        self.ask_replace(key, dialog, path, then);
                    } else {
                        self.saved_to(Held { doc, dialog, then, path: path.to_string_lossy().to_string() });
                    }
                }
                (Step::Replace { path, then }, Answer::Message(MessageResult::Yes)) => self.saved_to(Held { doc, dialog, then, path }),
                // Cancel, or No to replacing the file: nothing happens.
                _ => {}
            }
        }
    }

    /// The extension added to the name typed names another file than the one the dialog checked,
    /// so replacing an existing one asks again.
    #[cfg(not(target_arch = "wasm32"))]
    fn ask_replace(&mut self, key: AskKey, dialog: Option<String>, path: std::path::PathBuf, then: Then) {
        let lang = &self.ui.language;
        let file = path.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default();
        let message = craft_dialogs::Message {
            level: craft_dialogs::MessageLevel::Warning,
            title: crate::i18n::tr(lang, "Replace File?").to_string(),
            description: crate::i18n::tr(lang, "“{file}” already exists. Do you want to replace it?").replace("{file}", &file),
            buttons: craft_dialogs::MessageButtons::YesNo,
        };
        let key = AskKey { request: craft_dialogs::Request::Message(message.clone()), ..key };
        self.asked.push(key.clone());
        let path = path.to_string_lossy().to_string();
        self.file_dialogs.ask_message(message, Asked { key, dialog, step: Step::Replace { path, then } });
    }

    /// A save path chosen: its folder is where the next save dialog starts.
    #[cfg(not(target_arch = "wasm32"))]
    fn saved_to(&mut self, answer: Held) {
        if let Some(folder) = folder_of(&answer.path) {
            self.ui.last_save_folder = Some(folder.to_string_lossy().to_string());
        }
        self.continue_in(answer);
    }

    /// Runs the answer's continuation in the document the dialog was asked for, made active again
    /// if the user moved to another meanwhile; nothing runs once it has closed. While an egui
    /// dialog opened since the asking is up, the answer waits for it to close.
    #[cfg(not(target_arch = "wasm32"))]
    fn continue_in(&mut self, answer: Held) {
        if self.ui.dialog.as_ref().is_some_and(|d| Some(&d.id) != answer.dialog.as_ref()) {
            self.held_answers.push(answer);
            return;
        }
        let Held { doc, then, path, .. } = answer;
        if let Some(uid) = doc {
            let Some(index) = self.session.documents().iter().position(|d| d.uid == uid) else {
                self.status(crate::i18n::tr(&self.ui.language, "Cancelled: the document the dialog was for has closed"));
                return;
            };
            if self.session.active().map(|d| d.uid) != Some(uid)
                && let Err(e) = self.run("file.activate", json!({"index": index}))
            {
                self.status(e);
                return;
            }
        }
        // As for a UI command: a dialog that replaces Find releases its scope.
        let was_find = self.ui.dialog.as_ref().is_some_and(|d| d.id == "findChange");
        if let Err(e) = then(self, path) {
            self.status(e);
        }
        if was_find && self.ui.dialog.as_ref().is_none_or(|d| d.id != "findChange") {
            self.session.reset_find_scope();
        }
    }
}

/// `p` with `key` set to the chosen path: a command re-run with the answer to its dialog.
pub(crate) fn with_path(p: &Value, key: &str, path: String) -> Value {
    let mut q = if p.is_object() { p.clone() } else { json!({}) };
    q[key] = json!(path);
    q
}

#[cfg(not(target_arch = "wasm32"))]
fn folder_of(path: &str) -> Option<std::path::PathBuf> {
    std::path::Path::new(path).parent().filter(|f| !f.as_os_str().is_empty()).map(std::path::Path::to_path_buf)
}

/// One file-type row in an open dialog.
#[cfg(not(target_arch = "wasm32"))]
struct OpenFilter {
    name: &'static str,
    extensions: &'static [&'static str],
}

#[cfg(not(target_arch = "wasm32"))]
const GRAPHICS: &[&str] = &["png", "jpg", "jpeg", "gif", "webp", "tif", "tiff", "bmp", "psd", "svg", "pdf", "ai", "eps"];

#[cfg(not(target_arch = "wasm32"))]
fn open_filters(purpose: &str) -> &'static [OpenFilter] {
    match purpose {
        "swatches" => &[OpenFilter { name: "Swatch Exchange (ASE)", extensions: &["ase"] }],
        "script" => &[OpenFilter { name: "Script", extensions: &["dcscript", "txt", "json"] }],
        "shortcuts" => &[OpenFilter { name: "Shortcut set (JSON, InDesign text)", extensions: &["json", "txt"] }],
        "icc" => &[OpenFilter { name: "ICC profile", extensions: &["icc", "icm"] }],
        "book" => &[OpenFilter { name: "Book", extensions: &["dcbook"] }],
        "xml" => &[OpenFilter { name: "XML", extensions: &["xml"] }],
        "dtd" => &[OpenFilter { name: "DTD", extensions: &["dtd"] }],
        "library" => &[OpenFilter { name: "Object Library", extensions: &["dclib"] }],
        "relink" => &[OpenFilter { name: "Graphics", extensions: GRAPHICS }],
        "dataMerge" => &[OpenFilter { name: "Data source (CSV, TSV, text, Excel)", extensions: &["csv", "tsv", "tab", "txt", "xlsx"] }],
        "place" => &[
            OpenFilter {
                name: "Graphics and text",
                extensions: &[
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
                    "eps",
                    "txt",
                    "docx",
                    "rtf",
                    "md",
                    "xlsx",
                    "idml",
                    "designcraft",
                    "mp4",
                    "m4v",
                    "mov",
                    "webm",
                    "mp3",
                    "m4a",
                    "wav",
                    "ogg",
                ],
            },
            OpenFilter { name: "Graphics", extensions: GRAPHICS },
            OpenFilter { name: "Text (Word, RTF, plain, Excel)", extensions: &["docx", "rtf", "txt", "md", "xlsx"] },
            OpenFilter { name: "Video and sound", extensions: &["mp4", "m4v", "mov", "webm", "mp3", "m4a", "wav", "ogg"] },
        ],
        _ => &[
            OpenFilter { name: "DesignCraft, IDML or InDesign", extensions: &["designcraft", "idml", "indd", "indt"] },
            OpenFilter { name: "DesignCraft", extensions: &["designcraft"] },
            OpenFilter { name: "InDesign Markup (IDML)", extensions: &["idml"] },
            OpenFilter { name: "InDesign document or template (INDD, INDT)", extensions: &["indd", "indt"] },
            // Files saved without an extension; `file.open` tells the formats apart by content.
            OpenFilter { name: "All files", extensions: &[ALL_FILES] },
        ],
    }
}

/// The "All files" pattern. The macOS panel merges every filter into one list of allowed types,
/// which takes type identifiers as well as extensions: `public.data` admits any file.
#[cfg(not(target_arch = "wasm32"))]
const ALL_FILES: &str = if cfg!(target_os = "macos") { "public.data" } else { "*" };

/// The extension of the file name a save dialog suggests (`Brochure.designcraft`), if it has one.
/// The dialog filters on it, so the macOS and Windows dialogs add it to a name typed without it.
#[cfg(not(target_arch = "wasm32"))]
fn suggested_extension(name: &str) -> Option<&str> {
    std::path::Path::new(name).extension().and_then(std::ffi::OsStr::to_str).filter(|e| !e.is_empty() && !e.contains(char::is_whitespace))
}

/// The file a save dialog's answer writes: `picked` with the suggested extension added when it has
/// none (Linux dialogs don't add it). A document always ends in `.designcraft`, as `file.save`
/// writes it.
#[cfg(not(target_arch = "wasm32"))]
fn picked_save_path(picked: std::path::PathBuf, ext: Option<&str>) -> std::path::PathBuf {
    let Some(ext) = ext else { return picked };
    if ext.eq_ignore_ascii_case("designcraft") {
        let p = picked.to_string_lossy();
        return designcraft_engine::cmd::with_document_extension(&p).into();
    }
    if picked.extension().is_some() || picked.file_name().is_none() {
        return picked;
    }
    let mut s = picked.into_os_string();
    s.push(".");
    s.push(ext);
    s.into()
}

/// The folder a save dialog starts in and the name it suggests. A suggestion with a folder in
/// front (the document's, or the last one used) starts there while that folder still exists;
/// otherwise the system picks.
#[cfg(not(target_arch = "wasm32"))]
fn save_dialog_start(suggested: &str) -> (Option<std::path::PathBuf>, String) {
    let path = std::path::Path::new(suggested);
    let name = path.file_name().map_or_else(|| suggested.to_string(), |n| n.to_string_lossy().to_string());
    let folder = path.parent().filter(|f| !f.as_os_str().is_empty() && f.is_dir()).map(std::path::Path::to_path_buf);
    (folder, name)
}

#[cfg(test)]
pub(crate) mod testing {
    use craft_dialogs::{Answer, MessageResult, Request};

    use super::DesignApp;

    /// No window: dialogs answered by an override need none.
    pub(crate) struct NoWindow;

    impl raw_window_handle::HasWindowHandle for NoWindow {
        fn window_handle(&self) -> Result<raw_window_handle::WindowHandle<'_>, raw_window_handle::HandleError> {
            Err(raw_window_handle::HandleError::Unavailable)
        }
    }

    impl raw_window_handle::HasDisplayHandle for NoWindow {
        fn display_handle(&self) -> Result<raw_window_handle::DisplayHandle<'_>, raw_window_handle::HandleError> {
            Err(raw_window_handle::HandleError::Unavailable)
        }
    }

    /// Every file dialog answers `path` (`None`: Cancel); every message box answers Yes.
    pub(crate) fn answer_with(path: Option<String>) -> impl FnMut(&Request) -> Answer + 'static {
        move |request| match (request, &path) {
            (Request::Message(_), _) => Answer::Message(MessageResult::Yes),
            (Request::File(_), Some(path)) => Answer::Paths(vec![path.into()]),
            (Request::File(_), None) => Answer::Cancelled,
        }
    }

    impl DesignApp {
        /// From now on, `answer` answers every dialog instead of a native one.
        pub(crate) fn answer_dialogs(&mut self, answer: impl FnMut(&Request) -> Answer + 'static) {
            self.file_dialogs.set_override(Some(Box::new(answer)));
        }

        /// Shows the dialogs asked for (the override answers them) and runs their answers, until
        /// none is left: what the frames after the asking one do.
        pub(crate) fn settle_dialogs(&mut self) {
            for _ in 0..8 {
                self.show_file_dialogs(None::<&NoWindow>);
                self.handle_file_dialog_answers();
                if !self.file_dialogs.is_open() && self.file_dialogs.queued() == 0 {
                    return;
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use super::testing::answer_with;
    use super::{ALL_FILES, open_filters, picked_save_path, save_dialog_start, suggested_extension};

    #[test]
    fn relink_open_dialog_lists_graphics() {
        let exts: Vec<&str> = open_filters("relink").iter().flat_map(|filter| filter.extensions.iter().copied()).collect();
        for ext in ["png", "jpg", "tif", "psd", "svg", "pdf", "ai", "eps"] {
            assert!(exts.contains(&ext), "{ext} is missing from the relink dialog: {exts:?}");
        }
        assert!(!exts.contains(&"designcraft") && !exts.contains(&"idml"));
    }

    #[test]
    fn data_merge_open_dialog_lists_table_extensions() {
        let exts: Vec<&str> = open_filters("dataMerge").iter().flat_map(|filter| filter.extensions.iter().copied()).collect();
        for ext in ["csv", "tsv", "tab", "txt", "xlsx"] {
            assert!(exts.contains(&ext), "{ext} is missing from the dataMerge dialog: {exts:?}");
        }
        assert!(!exts.iter().any(|ext| *ext == "designcraft" || *ext == "idml"), "dataMerge must not fall through to the document filters: {exts:?}");
        let place = open_filters("place");
        assert!(place.len() > 1, "the place dialog keeps a filter for each kind of file");
        assert!(place.iter().any(|filter| filter.extensions.contains(&"png")));
        let documents = open_filters("");
        for ext in ["designcraft", "idml", "indd", "indt"] {
            assert!(documents.first().is_some_and(|filter| filter.extensions.contains(&ext)), "{ext} is missing from the first Open filter");
        }
        assert!(documents.last().is_some_and(|filter| filter.extensions == [ALL_FILES]), "files without an extension can be picked");
    }

    #[test]
    fn save_dialog_answers_get_the_suggested_extension() {
        let save = |picked: &str, name: &str| picked_save_path(PathBuf::from(picked), suggested_extension(name));
        assert_eq!(save("/d/Brochure", "Untitled.designcraft"), PathBuf::from("/d/Brochure.designcraft"));
        assert_eq!(save("/d/Brochure.idml", "Untitled.designcraft"), PathBuf::from("/d/Brochure.idml.designcraft"));
        assert_eq!(save("/d/Brochure.designcraft", "Untitled.designcraft"), PathBuf::from("/d/Brochure.designcraft"));
        assert_eq!(save("/d/Brochure", "Untitled.pdf"), PathBuf::from("/d/Brochure.pdf"));
        assert_eq!(save("/d/Brochure.txt", "Untitled.rtf"), PathBuf::from("/d/Brochure.txt"), "another export format is kept");
        assert_eq!(save("/d/Package", "Untitled 1.2 Folder"), PathBuf::from("/d/Package"), "a folder has no extension");
    }

    /// #1653: the save dialog opens in the folder the suggestion names, unless it is gone.
    #[test]
    fn save_dialog_starts_in_the_suggested_folder_while_it_exists() {
        let folder = std::env::temp_dir();
        let suggested = folder.join("Brochure.pdf");
        assert_eq!(save_dialog_start(&suggested.to_string_lossy()), (Some(folder.clone()), "Brochure.pdf".to_string()));
        let gone = folder.join(format!("designcraft-gone-{}", std::process::id())).join("Brochure.pdf");
        assert_eq!(save_dialog_start(&gone.to_string_lossy()), (None, "Brochure.pdf".to_string()), "a folder deleted since: the system's own");
        assert_eq!(save_dialog_start("Untitled 1.2 Folder"), (None, "Untitled 1.2 Folder".to_string()), "a bare name: the system's own");
    }

    /// Place, Graphic Cell and Choose Image share the Place dialog, but each keeps its own
    /// request, as does Place asked for another document; Place asked again for the same one
    /// doesn't queue a second dialog.
    #[test]
    fn different_askers_and_documents_queue_their_own_dialogs() {
        let mut app = crate::DesignApp::new(designcraft_engine::Session::new(), crate::Services::default());
        app.answer_dialogs(answer_with(None));
        app.run("file.new", serde_json::json!({})).unwrap();
        app.run("app.placeDialog", serde_json::json!({})).unwrap();
        app.run("app.graphicCell", serde_json::json!({})).unwrap();
        assert_eq!(app.file_dialogs.queued(), 2);
        app.run("file.new", serde_json::json!({})).unwrap();
        app.run("app.placeDialog", serde_json::json!({})).unwrap();
        assert_eq!(app.file_dialogs.queued(), 3);
        app.run("app.placeDialog", serde_json::json!({})).unwrap();
        assert_eq!(app.file_dialogs.queued(), 3);
    }

    /// An answer that arrives while an egui dialog opened since is up (Preferences, an
    /// unsaved-changes question) waits for it to close instead of replacing it.
    #[test]
    fn an_answer_waits_for_a_dialog_opened_after_the_asking() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("Held.designcraft");
        let mut source = crate::DesignApp::new(designcraft_engine::Session::new(), crate::Services::default());
        source.run("file.new", serde_json::json!({})).unwrap();
        source.run("file.saveAs", serde_json::json!({"path": path.to_string_lossy()})).unwrap();

        let mut app = crate::DesignApp::new(designcraft_engine::Session::new(), crate::Services::default());
        app.answer_dialogs(answer_with(Some(path.to_string_lossy().to_string())));
        app.run("app.openDialog", serde_json::json!({})).unwrap();
        app.ui.dialog = Some(crate::dialogs::Dialog::new("preferences", serde_json::json!({})));
        app.settle_dialogs();
        assert!(app.session.documents().is_empty(), "the answer waits");
        assert_eq!(app.ui.dialog.as_ref().map(|d| d.id.as_str()), Some("preferences"));
        assert!(app.file_dialogs_pending(), "Quit still sees the answer coming");
        app.ui.dialog = None;
        app.handle_file_dialog_answers();
        assert_eq!(app.session.documents().len(), 1);
        assert!(!app.file_dialogs_pending());
    }

    /// File › Open without a path asks and returns at once; the document opens on a later frame,
    /// when the dialog's answer arrives. The frame never waits for the dialog.
    #[test]
    fn file_open_without_a_path_opens_the_chosen_document_when_the_dialog_answers() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("Chosen.designcraft");
        let mut source = crate::DesignApp::new(designcraft_engine::Session::new(), crate::Services::default());
        source.run("file.new", serde_json::json!({})).unwrap();
        source.run("file.saveAs", serde_json::json!({"path": path.to_string_lossy()})).unwrap();

        let mut app = crate::DesignApp::new(designcraft_engine::Session::new(), crate::Services::default());
        app.answer_dialogs(answer_with(Some(path.to_string_lossy().to_string())));
        app.run("app.openDialog", serde_json::json!({})).unwrap();
        assert!(app.session.documents().is_empty(), "nothing opens before the dialog has answered");
        // Asking again while the dialog is up doesn't queue a second one.
        app.run("app.openDialog", serde_json::json!({})).unwrap();
        assert_eq!(app.file_dialogs.queued(), 1);
        app.settle_dialogs();
        assert_eq!(app.session.documents().len(), 1, "the chosen document opened once");
        assert_eq!(app.session.active().and_then(|d| d.path.clone()).map(PathBuf::from), Some(path));
    }
}
