//! Whether a linked graphic's file still holds what the document shows, without reading the file
//! every time someone asks.
//!
//! The document keeps a copy of each linked graphic. Comparing it with the file byte for byte on
//! every look would read every linked photo twice a second while the Links panel is open, so each
//! answer is remembered with the file's size and modification time then (its [`Stamp`]): while
//! the file's stamp stays the same, the answer stands and only the file's metadata is read. A file
//! whose size differs from the copy is modified without reading it. When the engine reads a linked
//! file itself (File › Place, Update Link, Relink) it remembers that the file matched what it read.
//!
//! What is remembered lives as long as the process and is never saved, so documents and their
//! file formats are unchanged. A document opened again remembers nothing: the first look at each
//! file of the copy's size reads it once. The UI makes that first look on a worker thread, and
//! asks only for [`LinkState::Unverified`] (metadata, never the contents) on the UI thread.
//!
//! An answer is keyed by the copy's bytes (their `Arc`, held weakly) and the file's path, so an
//! undo that brings back an older copy is compared against the file again rather than trusted.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, PoisonError, Weak};
use std::time::SystemTime;

/// How a linked graphic's file compares with the copy in the document.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LinkState {
    /// Not linked: the document holds the only copy.
    Embedded,
    /// The file isn't there (or isn't a regular file, or the link isn't followed).
    Missing,
    /// The file differs from the copy.
    Modified,
    /// The file holds the copy's bytes.
    Ok,
    /// Same size as the copy, but whether the bytes are the same takes reading the file.
    Unverified,
}

impl LinkState {
    /// The Links panel's name for it (`links.list` `status`); an unverified link reads as OK.
    pub fn name(self) -> &'static str {
        match self {
            LinkState::Embedded => "embedded",
            LinkState::Missing => "missing",
            LinkState::Modified => "modified",
            LinkState::Ok | LinkState::Unverified => "ok",
        }
    }
}

/// A file's size and modification time.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Stamp {
    len: u64,
    modified: SystemTime,
}

#[cfg_attr(target_arch = "wasm32", allow(dead_code))]
/// A remembered comparison: the file at `path` with `stamp` held `data` (`same`) or not.
struct Seen {
    data: Weak<Vec<u8>>,
    path: PathBuf,
    stamp: Stamp,
    same: bool,
}

/// Remembered comparisons by the address of the copy's bytes. Bounded: past this many, the ones
/// whose copy is gone are dropped, then all of them (each is only a saved read).
const MAX_SEEN: usize = 4096;

static SEEN: Mutex<Option<HashMap<usize, Seen>>> = Mutex::new(None);

#[cfg_attr(target_arch = "wasm32", allow(dead_code))]
fn key(data: &Arc<Vec<u8>>) -> usize {
    Arc::as_ptr(data).addr()
}

/// The size and time of the file at `path`, when it is a regular file with a time.
#[cfg(not(target_arch = "wasm32"))]
pub(crate) fn stamp(path: &Path) -> Option<Stamp> {
    let m = crate::io::file_metadata(path)?;
    Some(Stamp { len: m.len(), modified: m.modified().ok()? })
}

/// Can a stamp be trusted to change when the file does? Not while the file was changed in the last
/// two seconds: some file systems keep whole seconds, and another write in that second keeps the
/// time. A time in the future (clock skew, a file from another computer) counts as settled.
#[cfg_attr(target_arch = "wasm32", allow(dead_code))]
fn settled(s: &Stamp) -> bool {
    match SystemTime::now().duration_since(s.modified) {
        Ok(age) => age.as_secs() >= 2,
        Err(_) => true,
    }
}

/// Remember that the file at `path`, whose stamp before it was read was `before`, held `data`
/// (`same`) or not. A stamp too recent to trust isn't remembered.
#[cfg_attr(target_arch = "wasm32", allow(dead_code))]
pub(crate) fn remember(data: &Arc<Vec<u8>>, path: &Path, before: Option<Stamp>, same: bool) {
    let Some(stamp) = before.filter(settled) else { return };
    let mut seen = SEEN.lock().unwrap_or_else(PoisonError::into_inner);
    let map = seen.get_or_insert_with(HashMap::new);
    if map.len() >= MAX_SEEN {
        map.retain(|_, s| s.data.strong_count() > 0);
        if map.len() >= MAX_SEEN {
            map.clear();
        }
    }
    map.insert(key(data), Seen { data: Arc::downgrade(data), path: path.to_path_buf(), stamp, same });
}

/// What was remembered about `data` and the file at `path` with `now` as its stamp.
#[cfg_attr(target_arch = "wasm32", allow(dead_code))]
fn remembered(data: &Arc<Vec<u8>>, path: &Path, now: Stamp) -> Option<bool> {
    let seen = SEEN.lock().unwrap_or_else(PoisonError::into_inner);
    let s = seen.as_ref()?.get(&key(data))?;
    // The weak reference keeps the address from being reused and says whether the copy is there.
    (s.data.upgrade().is_some_and(|d| Arc::ptr_eq(&d, data)) && s.path == path && s.stamp == now).then_some(s.same)
}

/// Looks at the file at `path` against `data` without reading its contents (one metadata call).
#[cfg(not(target_arch = "wasm32"))]
pub(crate) fn look(data: &Arc<Vec<u8>>, path: &Path) -> LinkState {
    let Some(m) = crate::io::file_metadata(path) else { return LinkState::Missing };
    if u64::try_from(data.len()).ok() != Some(m.len()) {
        return LinkState::Modified;
    }
    let Some(now) = m.modified().ok().map(|modified| Stamp { len: m.len(), modified }) else { return LinkState::Unverified };
    match remembered(data, path, now) {
        Some(true) => LinkState::Ok,
        Some(false) => LinkState::Modified,
        None => LinkState::Unverified,
    }
}

#[cfg(test)]
thread_local! {
    /// Files whose contents [`compare`] read on this thread (tests count them).
    pub(crate) static READS: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
}

/// [`look`], then reading the file when looking can't tell; the answer is remembered. The file is
/// read in pieces and compared as it goes, so a large file that differs early isn't read whole.
#[cfg(not(target_arch = "wasm32"))]
pub(crate) fn compare(data: &Arc<Vec<u8>>, path: &Path) -> LinkState {
    use std::io::Read;
    let state = look(data, path);
    if state != LinkState::Unverified {
        return state;
    }
    #[cfg(test)]
    READS.with(|r| r.set(r.get().saturating_add(1)));
    let before = stamp(path);
    let Ok(mut f) = crate::io::open(path) else { return LinkState::Missing };
    let mut buf = vec![0u8; 256 * 1024];
    let mut at = 0usize;
    let same = loop {
        let n = match f.read(&mut buf) {
            Ok(n) => n,
            Err(e) if e.kind() == std::io::ErrorKind::Interrupted => continue,
            Err(_) => return LinkState::Missing,
        };
        if n == 0 {
            break at == data.len();
        }
        let end = at.saturating_add(n);
        match (data.get(at..end), buf.get(..n)) {
            (Some(want), Some(got)) if want == got => at = end,
            // Longer than the copy, or different.
            _ => break false,
        }
    };
    remember(data, path, before, same);
    if same { LinkState::Ok } else { LinkState::Modified }
}

#[cfg(target_arch = "wasm32")]
pub(crate) fn stamp(_path: &Path) -> Option<Stamp> {
    None
}

/// How the file a graphic is linked to compares with `data`, the copy in the document. `link` is
/// the stored link and `document` the document's file (relative links resolve against it).
/// `read`: read the file when its metadata can't tell (once per change of the file); without it
/// such a link is [`LinkState::Unverified`] and nothing but metadata is read, for the UI thread.
pub fn link_state(link: Option<&str>, document: Option<&Path>, data: &Arc<Vec<u8>>, read: bool) -> LinkState {
    let Some(link) = link else { return LinkState::Embedded };
    // Nothing was read from the file (a package's link that wasn't followed): missing, as in
    // Preflight.
    if data.is_empty() {
        return LinkState::Missing;
    }
    #[cfg(not(target_arch = "wasm32"))]
    {
        let Ok(path) = crate::io::document_path(link, document) else { return LinkState::Missing };
        if read { compare(data, &path) } else { look(data, &path) }
    }
    // The browser can't look at files: a link with a copy is taken as it is.
    #[cfg(target_arch = "wasm32")]
    {
        let _ = (link, document, read);
        LinkState::Ok
    }
}
