use super::state::{ImportRequest, ImportedFile};

pub type ReadFn = Box<dyn Fn(&str) -> Result<Vec<u8>, String>>;

pub type WriteFn = Box<dyn FnMut(&str, &[u8]) -> Result<(), String>>;

pub type OpenAsyncFn = Box<dyn FnMut(ImportRequest)>;

/// Starts a browser download; an error means no file was handed to the user.
pub type DownloadFn = Box<dyn FnMut(&str, &[u8]) -> Result<(), String>>;

pub type ClipboardFn = Box<dyn FnMut() -> Option<String>>;
/// Writes an object copy's placeholder text and a PNG picture of it to the system clipboard
/// together, as one atomic write where the platform needs it (so the picture doesn't replace the
/// placeholder text this app's own Paste still needs, #2204).
pub type ClipboardObjectCopyFn = Box<dyn FnMut(&str, &[u8]) -> Result<(), String>>;

/// Keeps the window's frame as the app needs it; called once per frame, before it is laid out.
pub type WindowFrameFn = Box<dyn FnMut()>;

/// Where the pointer is on the screen now, in the system's points from the top left of the main
/// display, before the UI scale: the app divides them by egui's zoom factor to compare them with
/// the window's `inner_rect`. `None` when the system doesn't say.
pub type PointerFn = Box<dyn Fn() -> Option<egui::Pos2>>;

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
    /// [`ClipboardObjectCopyFn`]: `None` where the platform has no atomic multi-type write, which
    /// leaves an object copy's picture off the system clipboard (so only this app's own Paste
    /// reads it back) rather than risk losing the placeholder text.
    pub clipboard_set_object_copy: Option<ClipboardObjectCopyFn>,
    /// Keeps the undecorated window borderless, so the content and the pointer stay aligned
    /// (Windows custom title bar, #1295). `None` when the window is decorated or the platform
    /// needs no fixing.
    pub window_frame: Option<WindowFrameFn>,
    /// The pointer's place on the screen, asked when a file is dropped: a file dragged in from
    /// another application brings no pointer events with it (winit reports none during a drag on
    /// macOS), so without it a drop can't tell which frame it landed on. `None`: a drop without
    /// pointer events lands in the middle of the view.
    pub pointer: Option<PointerFn>,
}
