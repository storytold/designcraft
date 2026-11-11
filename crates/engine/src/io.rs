//! Reading and writing the files a command names by path.
//!
//! Every such read and write goes through this module, so the automation roots
//! ([`crate::file_access`]) are checked in one place. A command module that called `std::fs`
//! itself would bypass them: a test in `file_access` keeps that from happening.
//!
//! A path the roots refuse reads as a file that isn't there where the caller only asks whether
//! it is ([`is_file`], [`is_dir`], [`exists`], [`metadata`]), and as the roots' own message where
//! it asks for the bytes.

pub use craft_docpath::Refused;
pub use std::fs::Metadata;
use std::path::{Path, PathBuf};

/// The path to open for a path stored in a document (a link, a data source, a book chapter),
/// resolved against `document`, the path of the file it came from, when there is one.
///
/// Paths to other computers (other than the document's own network share), devices and URLs are
/// refused, and so is a result that isn't absolute: without a document, a relative path would
/// depend on the working directory, and a Windows path names no file on other platforms. A
/// refused path is never handed to the file system; callers treat it as a missing file.
pub fn document_path(stored: &str, document: Option<&Path>) -> Result<PathBuf, Refused> {
    let document = document.map(|d| std::path::absolute(d).unwrap_or_else(|_| d.to_path_buf()));
    craft_docpath::resolve_absolute(stored, document.as_deref())
}

/// [`document_path`] for a path stored in a file inside `folder` (a data file's image names).
pub fn folder_path(stored: &str, folder: &Path) -> Result<PathBuf, Refused> {
    // Resolved against a document in the folder: only the document's folder is used.
    document_path(stored, Some(&folder.join("-")))
}

#[cfg(test)]
thread_local! {
    static TOUCHED: std::cell::RefCell<Option<Vec<PathBuf>>> = const { std::cell::RefCell::new(None) };
}

/// Run `f` and list every path it handed to this module, in order (on this thread).
#[cfg(test)]
pub(crate) fn touched<T>(f: impl FnOnce() -> T) -> (T, Vec<PathBuf>) {
    let previous = TOUCHED.with(|t| t.replace(Some(Vec::new())));
    let out = f();
    let seen = TOUCHED.with(|t| t.replace(previous)).unwrap_or_default();
    (out, seen)
}

fn touch(path: &Path) {
    #[cfg(test)]
    TOUCHED.with(|t| {
        if let Some(seen) = t.borrow_mut().as_mut() {
            seen.push(path.to_path_buf());
        }
    });
    #[cfg(not(test))]
    let _ = path;
}

/// The bytes of the file at `path`, inside the automation read root when there is one.
pub fn read_file(path: impl AsRef<Path>) -> Result<Vec<u8>, String> {
    let path = path.as_ref();
    touch(path);
    crate::file_access::check_read(path)?;
    std::fs::read(path).map_err(|e| format!("{}: {e}", path.display()))
}

/// [`read_file`] with the file system's own error, for callers that tell "missing" from other
/// failures. A refused path is a `PermissionDenied` error carrying the roots' message.
pub fn read(path: impl AsRef<Path>) -> std::io::Result<Vec<u8>> {
    let path = path.as_ref();
    touch(path);
    crate::file_access::check_read(path).map_err(|e| std::io::Error::new(std::io::ErrorKind::PermissionDenied, e))?;
    std::fs::read(path)
}

/// [`read`] that stops past `cap` bytes: at most `cap + 1` bytes are read, so a device or a file
/// that grows while it is read can't exhaust memory. The caller compares the length with `cap`.
pub fn read_capped(path: impl AsRef<Path>, cap: u64) -> std::io::Result<Vec<u8>> {
    use std::io::Read;
    let path = path.as_ref();
    touch(path);
    crate::file_access::check_read(path).map_err(|e| std::io::Error::new(std::io::ErrorKind::PermissionDenied, e))?;
    let mut b = Vec::new();
    std::fs::File::open(path)?.take(cap.saturating_add(1)).read_to_end(&mut b)?;
    Ok(b)
}

/// [`read_capped`] of a regular file only, for a path that comes out of a document (a linked
/// graphic or text file). The file is opened without waiting and the opened file is checked, so
/// a FIFO or device put in place of the file after a caller looked at its metadata is refused
/// instead of blocking the thread that reads it (a link check's worker) or being read.
pub fn read_regular_capped(path: impl AsRef<Path>, cap: u64) -> std::io::Result<Vec<u8>> {
    use std::io::Read;
    let path = path.as_ref();
    touch(path);
    crate::file_access::check_read(path).map_err(|e| std::io::Error::new(std::io::ErrorKind::PermissionDenied, e))?;
    let mut options = std::fs::OpenOptions::new();
    options.read(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.custom_flags(O_NONBLOCK);
    }
    let f = options.open(path)?;
    if !f.metadata()?.is_file() {
        return Err(std::io::Error::new(std::io::ErrorKind::InvalidInput, format!("{}: not a regular file", path.display())));
    }
    let mut b = Vec::new();
    f.take(cap.saturating_add(1)).read_to_end(&mut b)?;
    Ok(b)
}

/// Whether `path` is an application files can be opened in (Edit › Edit With): an application
/// bundle (`.app` folder) on macOS, an `.exe` file on Windows, an executable regular file
/// elsewhere. Links are followed.
#[cfg(not(target_arch = "wasm32"))]
pub fn is_application(path: impl AsRef<Path>) -> bool {
    let path = path.as_ref();
    let Ok(meta) = metadata(path) else { return false };
    let ext = path.extension().map(|e| e.to_string_lossy().to_ascii_lowercase());
    #[cfg(target_os = "macos")]
    return meta.is_dir() && ext.as_deref() == Some("app");
    #[cfg(target_os = "windows")]
    return meta.is_file() && ext.as_deref() == Some("exe");
    #[cfg(not(any(target_os = "macos", target_os = "windows")))]
    {
        let _ = ext;
        meta.is_file() && is_executable_mode(&meta)
    }
}

/// Whether the file at `path` is marked executable (any execute bit, on Unix): a system opener
/// such as `xdg-open` may run it whatever its name says. Never on other platforms.
pub fn is_executable(path: impl AsRef<Path>) -> bool {
    metadata(path).is_ok_and(|m| is_executable_mode(&m))
}

#[cfg(unix)]
fn is_executable_mode(meta: &Metadata) -> bool {
    use std::os::unix::fs::PermissionsExt;
    meta.permissions().mode() & 0o111 != 0
}

#[cfg(not(unix))]
fn is_executable_mode(_meta: &Metadata) -> bool {
    false
}

/// `O_NONBLOCK`: opening a FIFO with it returns at once instead of waiting for a writer. On a
/// regular file it changes nothing. (std has no constant for it; these are the platforms' values.)
#[cfg(any(target_os = "linux", target_os = "android"))]
const O_NONBLOCK: i32 = 0o4000;
#[cfg(any(target_os = "macos", target_os = "ios", target_os = "freebsd", target_os = "openbsd", target_os = "netbsd", target_os = "dragonfly"))]
const O_NONBLOCK: i32 = 0x4;
#[cfg(all(
    unix,
    not(any(
        target_os = "linux",
        target_os = "android",
        target_os = "macos",
        target_os = "ios",
        target_os = "freebsd",
        target_os = "openbsd",
        target_os = "netbsd",
        target_os = "dragonfly"
    ))
))]
const O_NONBLOCK: i32 = 0;

/// The file at `path` opened for reading, inside the automation read root when there is one, for
/// callers that read only part of it. A refused path is a `PermissionDenied` error carrying the
/// roots' message.
pub fn open(path: impl AsRef<Path>) -> std::io::Result<std::fs::File> {
    let path = path.as_ref();
    touch(path);
    crate::file_access::check_read(path).map_err(|e| std::io::Error::new(std::io::ErrorKind::PermissionDenied, e))?;
    std::fs::File::open(path)
}

/// The text of the file at `path`, inside the automation read root when there is one.
pub fn read_to_string(path: impl AsRef<Path>) -> Result<String, String> {
    let path = path.as_ref();
    touch(path);
    crate::file_access::check_read(path)?;
    std::fs::read_to_string(path).map_err(|e| format!("{}: {e}", path.display()))
}

/// Make the folder at `path` and the folders above it, inside the automation write root when
/// there is one.
#[cfg(not(target_arch = "wasm32"))]
pub fn create_dir_all(path: impl AsRef<Path>) -> Result<(), String> {
    let path = path.as_ref();
    crate::file_access::check_write(path)?;
    std::fs::create_dir_all(path).map_err(|e| format!("{}: {e}", path.display()))
}

/// Write `bytes` to the file at `path`, inside the automation write root when there is one.
pub fn write_file(path: impl AsRef<Path>, bytes: &[u8]) -> Result<(), String> {
    let path = path.as_ref();
    touch(path);
    crate::file_access::check_write(path)?;
    // Replaced atomically where there is a real file system (see `atomic`).
    #[cfg(not(target_arch = "wasm32"))]
    let written = crate::atomic::write_atomic(path, bytes);
    #[cfg(target_arch = "wasm32")]
    let written = std::fs::write(path, bytes);
    written.map_err(|e| format!("{}: {e}", path.display()))
}

/// Atomically replace the file at `path` with the contents of `staged`, a private temporary file
/// the engine made itself (so it is not checked), inside the automation write root when there is
/// one.
#[cfg(not(target_arch = "wasm32"))]
pub fn replace_with(path: impl AsRef<Path>, staged: impl AsRef<Path>) -> std::io::Result<()> {
    let path = path.as_ref();
    touch(path);
    crate::file_access::check_write(path).map_err(|e| std::io::Error::new(std::io::ErrorKind::PermissionDenied, e))?;
    let mut input = std::fs::File::open(staged)?;
    crate::atomic::write_atomic_with(path, |file| std::io::copy(&mut input, file).map(|_| ()))
}

/// Could the file at `path` be opened for writing (without truncating it)? Inside the automation
/// write root when there is one.
pub fn writable(path: impl AsRef<Path>) -> std::io::Result<()> {
    let path = path.as_ref();
    touch(path);
    crate::file_access::check_write(path).map_err(|e| std::io::Error::new(std::io::ErrorKind::PermissionDenied, e))?;
    std::fs::OpenOptions::new().write(true).open(path).map(drop)
}

/// The canonical form of `path` (links resolved), inside the automation read root when there is
/// one.
pub fn canonicalize(path: impl AsRef<Path>) -> std::io::Result<std::path::PathBuf> {
    let path = path.as_ref();
    touch(path);
    crate::file_access::check_read(path).map_err(|e| std::io::Error::new(std::io::ErrorKind::PermissionDenied, e))?;
    std::fs::canonicalize(path)
}

/// What the file system says about `path` (links followed), inside the automation read root when
/// there is one. A refused path is a `PermissionDenied` error carrying the roots' message.
pub fn metadata(path: impl AsRef<Path>) -> std::io::Result<Metadata> {
    let path = path.as_ref();
    touch(path);
    crate::file_access::check_read(path).map_err(|e| std::io::Error::new(std::io::ErrorKind::PermissionDenied, e))?;
    std::fs::metadata(path)
}

/// Is there a file at `path` that automation may read?
pub fn is_file(path: impl AsRef<Path>) -> bool {
    metadata(path).is_ok_and(|m| m.is_file())
}

/// Is there a folder at `path` that automation may read?
pub fn is_dir(path: impl AsRef<Path>) -> bool {
    metadata(path).is_ok_and(|m| m.is_dir())
}

/// Is there anything at `path` that automation may read?
pub fn exists(path: impl AsRef<Path>) -> bool {
    metadata(path).is_ok()
}

/// The entries of the folder at `path`, inside the automation read root when there is one.
pub fn read_dir(path: impl AsRef<Path>) -> std::io::Result<std::fs::ReadDir> {
    let path = path.as_ref();
    touch(path);
    crate::file_access::check_read(path).map_err(|e| std::io::Error::new(std::io::ErrorKind::PermissionDenied, e))?;
    std::fs::read_dir(path)
}
