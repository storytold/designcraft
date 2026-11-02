use super::state::{ImportRequest, ImportedFile};

pub type ReadFn = Box<dyn Fn(&str) -> Result<Vec<u8>, String>>;

pub type WriteFn = Box<dyn FnMut(&str, &[u8]) -> Result<(), String>>;

pub type OpenAsyncFn = Box<dyn FnMut(ImportRequest)>;

/// Starts a browser download; an error means no file was handed to the user.
pub type DownloadFn = Box<dyn FnMut(&str, &[u8]) -> Result<(), String>>;

pub type ClipboardFn = Box<dyn FnMut() -> Option<String>>;

/// Files delivered asynchronously with their original operation and document identity.
pub type Inbox = std::sync::Arc<std::sync::Mutex<Vec<ImportedFile>>>;

/// Platform services injected by the host (desktop or web). File dialogs aren't among them: the
/// app asks for them itself and the host shows them (`DesignApp::show_file_dialogs`).
#[derive(Default)]
pub struct Services {
    pub read: Option<ReadFn>,
    pub write: Option<WriteFn>,
    /// Asynchronous open dialog for a purpose (`open`, `place`); the chosen file arrives later
    /// through [`Services::inbox`]. In a browser, which has no native dialogs.
    pub open_async: Option<OpenAsyncFn>,
    /// Hand bytes to the user as a named file (browser download). When set, Save uses it
    /// instead of writing to a path, and a file saved under a name goes to the downloads with no
    /// save dialog.
    pub download: Option<DownloadFn>,
    /// Files delivered asynchronously, drained every frame with their captured request context.
    pub inbox: Option<Inbox>,
    /// The system clipboard's text, for Paste chosen from a menu (egui only delivers it with the
    /// paste keys, which a native menu bar takes first).
    pub clipboard_text: Option<ClipboardFn>,
}
