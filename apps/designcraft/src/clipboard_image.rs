//! The system clipboard's image for Edit › Paste (`Services::clipboard_image`): a bitmap
//! (Windows `CF_DIBV5` or `PNG`, the macOS pasteboard's TIFF or PNG, `image/png` on X11), else the
//! first image file of files copied in a file manager. The read runs on its own thread, and Paste
//! waits for it at most [`WAIT`], so an application that is slow to hand over the clipboard does
//! not freeze the window. While a read that timed out is still waiting, Paste starts no other.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc;
use std::time::Duration;

use designcraft_ui_egui::ClipboardImage;

/// How long Paste waits for the clipboard's image.
const WAIT: Duration = Duration::from_millis(1500);

/// The largest image file Paste reads from a copied file list, in bytes.
const MAX_FILE_BYTES: u64 = 256 << 20;

/// A clipboard read is running.
static READING: AtomicBool = AtomicBool::new(false);

/// The clipboard's image, or `None` when it holds none or does not answer within [`WAIT`].
pub fn read() -> Option<ClipboardImage> {
    within(&READING, WAIT, read_now)
}

/// Clears the in-flight flag when the read's thread ends.
struct Reading(&'static AtomicBool);

impl Drop for Reading {
    fn drop(&mut self) {
        self.0.store(false, Ordering::Release);
    }
}

/// `read`'s result when it arrives within `wait`. A read that takes longer finishes on its own
/// thread and is dropped; until it does, `busy` stays set and no other read starts.
fn within<T: Send + 'static>(busy: &'static AtomicBool, wait: Duration, read: impl FnOnce() -> Option<T> + Send + 'static) -> Option<T> {
    if busy.swap(true, Ordering::AcqRel) {
        log::warn!("clipboard image: the previous read has not finished");
        return None;
    }
    let (tx, rx) = mpsc::channel();
    let spawned = std::thread::Builder::new().name("clipboard image".into()).spawn(move || {
        let _reading = Reading(busy);
        let _ = tx.send(read());
    });
    if let Err(e) = spawned {
        busy.store(false, Ordering::Release);
        log::warn!("clipboard image: no thread to read it: {e}");
        return None;
    }
    match rx.recv_timeout(wait) {
        Ok(image) => image,
        Err(mpsc::RecvTimeoutError::Timeout) => {
            log::warn!("clipboard image: the clipboard did not answer within {} ms", wait.as_millis());
            None
        }
        Err(mpsc::RecvTimeoutError::Disconnected) => None,
    }
}

/// The general pasteboard's change count, which AppKit raises with every copy.
#[cfg(target_os = "macos")]
pub fn change_count() -> Option<i64> {
    i64::try_from(objc2_app_kit::NSPasteboard::generalPasteboard().changeCount()).ok()
}

fn read_now() -> Option<ClipboardImage> {
    let mut clipboard = arboard::Clipboard::new().ok()?;
    if let Ok(image) = clipboard.get_image() {
        let width = u32::try_from(image.width).ok()?;
        let height = u32::try_from(image.height).ok()?;
        return Some(ClipboardImage::Pixels { width, height, rgba: image.bytes.into_owned() });
    }
    first_image_file(&clipboard.get().file_list().ok()?)
}

/// The first of `paths` that is an image file no larger than [`MAX_FILE_BYTES`], with its name.
fn first_image_file(paths: &[PathBuf]) -> Option<ClipboardImage> {
    paths.iter().find_map(|path| {
        let ext = path.extension()?.to_str()?.to_ascii_lowercase();
        if !designcraft_engine::cmd::IMAGE_EXTENSIONS.contains(&ext.as_str()) || std::fs::metadata(path).ok()?.len() > MAX_FILE_BYTES {
            return None;
        }
        // At most one byte past the limit is read, should the file grow after the size check.
        let bytes = designcraft_engine::io::read_capped(path, MAX_FILE_BYTES).ok().filter(|b| b.len() as u64 <= MAX_FILE_BYTES)?;
        Some(ClipboardImage::File { name: file_name(path), bytes })
    })
}

fn file_name(path: &Path) -> String {
    path.file_name().map_or_else(|| path.to_string_lossy().into_owned(), |n| n.to_string_lossy().into_owned())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_read_that_does_not_answer_is_given_up_and_no_second_one_starts_meanwhile() {
        static BUSY: AtomicBool = AtomicBool::new(false);
        let (release, blocked) = mpsc::channel::<()>();
        // The read waits for a message that is only sent after Paste has given up on it.
        assert_eq!(within(&BUSY, Duration::ZERO, move || blocked.recv().ok().map(|()| 1)), None);
        let (started, ran) = mpsc::channel::<()>();
        let second = within(&BUSY, Duration::from_secs(60), move || started.send(()).ok().map(|()| 2));
        assert_eq!(second, None, "refused while the first read waits");
        assert!(ran.try_recv().is_err(), "the second read never ran");
        let _ = release.send(());
        while BUSY.load(Ordering::Acquire) {
            std::thread::yield_now();
        }
        assert_eq!(within(&BUSY, Duration::from_secs(60), || Some(3)), Some(3), "an answer is taken as soon as it comes");
    }

    #[test]
    fn a_copied_file_list_pastes_its_first_image_file() {
        let stamp = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos();
        let dir = std::env::temp_dir().join(format!("designcraft-clipboard-files-{}-{stamp}", std::process::id()));
        std::fs::create_dir(&dir).unwrap();
        let (text, missing, png) = (dir.join("notes.txt"), dir.join("gone.png"), dir.join("Photo.PNG"));
        std::fs::write(&text, b"text").unwrap();
        std::fs::write(&png, b"png bytes").unwrap();
        let picked = first_image_file(&[text, missing, png.clone()]);
        let none = first_image_file(&[dir.join("a.txt")]);
        let _ = std::fs::remove_dir_all(&dir);
        match picked {
            Some(ClipboardImage::File { name, bytes }) => assert_eq!((name.as_str(), bytes.as_slice()), ("Photo.PNG", b"png bytes".as_slice())),
            _ => panic!("the image file is picked"),
        }
        assert!(none.is_none());
    }
}
