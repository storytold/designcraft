use super::state::{ImportRequest, ImportedFile};

pub type ReadFn = Box<dyn Fn(&str) -> Result<Vec<u8>, String>>;
pub type BackgroundWriteFn = std::sync::Arc<dyn Fn(&str, &[u8]) -> Result<(), String> + Send + Sync>;

pub type WriteFn = Box<dyn FnMut(&str, &[u8]) -> Result<(), String>>;

pub type OpenAsyncFn = Box<dyn FnMut(ImportRequest)>;

/// Starts a browser download; an error means no file was handed to the user.
pub type DownloadFn = Box<dyn FnMut(&str, &[u8]) -> Result<(), String>>;

pub type ClipboardFn = Box<dyn FnMut() -> Option<String>>;

/// An image on the system clipboard.
pub enum ClipboardImage {
    /// A bitmap (a screenshot, an image copied in a browser): unpremultiplied RGBA, rows top to
    /// bottom.
    Pixels { width: u32, height: u32, rgba: Vec<u8> },
    /// An image file copied in a file manager: its name and contents.
    File { name: String, bytes: Vec<u8> },
}

impl ClipboardImage {
    /// An image egui delivered with Ctrl/Cmd+V (`Event::PasteImage`). The pixels of an image
    /// past the paste limit are not copied, and pasting it is refused by its size.
    pub fn from_color_image(image: &egui::ColorImage) -> Self {
        let [w, h] = image.size.map(|n| u32::try_from(n).unwrap_or(u32::MAX));
        let rgba = if designcraft_engine::cmd::check_pasted_size(w, h).is_ok() {
            image.pixels.iter().flat_map(|c| c.to_srgba_unmultiplied()).collect()
        } else {
            Vec::new()
        };
        ClipboardImage::Pixels { width: w, height: h, rgba }
    }
}

/// The system clipboard's image, for Paste chosen from a menu. It returns within a short bound
/// even when the application that owns the clipboard is slow to answer (`None` then).
pub type ClipboardImageFn = Box<dyn FnMut() -> Option<ClipboardImage>>;

/// A number the system raises with every copy to the clipboard (the macOS pasteboard's change
/// count), read without waiting for the clipboard's owner.
pub type ClipboardChangeFn = Box<dyn FnMut() -> Option<i64>>;

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
    /// Optional thread-safe writer for native background exports.
    pub background_write: Option<BackgroundWriteFn>,
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
    /// The system clipboard's image (a bitmap, else the first image file of a copied file list),
    /// for Paste chosen from a menu when the clipboard holds no text or only an image's address.
    pub clipboard_image: Option<ClipboardImageFn>,
    /// The clipboard's change count, so Paste can tell an image copied after an object Copy from
    /// one that was there before it. Needed where Copy leaves the system clipboard alone (a
    /// native menu bar); `None` elsewhere.
    pub clipboard_change: Option<ClipboardChangeFn>,
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
