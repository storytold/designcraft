//! Reading and writing the files a command names by path.
//!
//! Every such read and write goes through this module, so the automation roots
//! ([`crate::file_access`]) are checked in one place. A command module that called `std::fs`
//! itself would bypass them: a test in `file_access` keeps that from happening.
//!
//! A path the roots refuse reads as a file that isn't there where the caller only asks whether
//! it is ([`is_file`], [`is_dir`], [`exists`], [`metadata`]), and as the roots' own message where
//! it asks for the bytes.

pub use std::fs::Metadata;
use std::path::Path;

/// The bytes of the file at `path`, inside the automation read root when there is one.
pub fn read_file(path: impl AsRef<Path>) -> Result<Vec<u8>, String> {
    let path = path.as_ref();
    crate::file_access::check_read(path)?;
    std::fs::read(path).map_err(|e| format!("{}: {e}", path.display()))
}

/// [`read_file`] with the file system's own error, for callers that tell "missing" from other
/// failures. A refused path is a `PermissionDenied` error carrying the roots' message.
pub fn read(path: impl AsRef<Path>) -> std::io::Result<Vec<u8>> {
    let path = path.as_ref();
    crate::file_access::check_read(path).map_err(|e| std::io::Error::new(std::io::ErrorKind::PermissionDenied, e))?;
    std::fs::read(path)
}

/// [`read`] that stops past `cap` bytes: at most `cap + 1` bytes are read, so a device or a file
/// that grows while it is read can't exhaust memory. The caller compares the length with `cap`.
pub fn read_capped(path: impl AsRef<Path>, cap: u64) -> std::io::Result<Vec<u8>> {
    use std::io::Read;
    let path = path.as_ref();
    crate::file_access::check_read(path).map_err(|e| std::io::Error::new(std::io::ErrorKind::PermissionDenied, e))?;
    let mut b = Vec::new();
    std::fs::File::open(path)?.take(cap.saturating_add(1)).read_to_end(&mut b)?;
    Ok(b)
}

/// The text of the file at `path`, inside the automation read root when there is one.
pub fn read_to_string(path: impl AsRef<Path>) -> Result<String, String> {
    let path = path.as_ref();
    crate::file_access::check_read(path)?;
    std::fs::read_to_string(path).map_err(|e| format!("{}: {e}", path.display()))
}

/// Write `bytes` to the file at `path`, inside the automation write root when there is one.
pub fn write_file(path: impl AsRef<Path>, bytes: &[u8]) -> Result<(), String> {
    let path = path.as_ref();
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
    crate::file_access::check_write(path).map_err(|e| std::io::Error::new(std::io::ErrorKind::PermissionDenied, e))?;
    let mut input = std::fs::File::open(staged)?;
    crate::atomic::write_atomic_with(path, |file| std::io::copy(&mut input, file).map(|_| ()))
}

/// Could the file at `path` be opened for writing (without truncating it)? Inside the automation
/// write root when there is one.
pub fn writable(path: impl AsRef<Path>) -> std::io::Result<()> {
    let path = path.as_ref();
    crate::file_access::check_write(path).map_err(|e| std::io::Error::new(std::io::ErrorKind::PermissionDenied, e))?;
    std::fs::OpenOptions::new().write(true).open(path).map(drop)
}

/// The canonical form of `path` (links resolved), inside the automation read root when there is
/// one.
pub fn canonicalize(path: impl AsRef<Path>) -> std::io::Result<std::path::PathBuf> {
    let path = path.as_ref();
    crate::file_access::check_read(path).map_err(|e| std::io::Error::new(std::io::ErrorKind::PermissionDenied, e))?;
    std::fs::canonicalize(path)
}

/// What the file system says about `path` (links followed), inside the automation read root when
/// there is one. A refused path is a `PermissionDenied` error carrying the roots' message.
pub fn metadata(path: impl AsRef<Path>) -> std::io::Result<Metadata> {
    let path = path.as_ref();
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
    crate::file_access::check_read(path).map_err(|e| std::io::Error::new(std::io::ErrorKind::PermissionDenied, e))?;
    std::fs::read_dir(path)
}
