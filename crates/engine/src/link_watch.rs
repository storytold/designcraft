//! Preferences › File Handling › Update Links Changed on Disk: a placed graphic whose file another
//! app saves (VectorCraft, PhotoCraft, Illustrator, Photoshop) is updated without Window › Links ›
//! Update Link.
//!
//! The active document's linked files are compared by size and modification time with what they
//! were when last seen, and a file whose stamp moved is read again (its pixel size checked as
//! Update Link does), all on a worker thread: neither a large file nor one on a slow network
//! share holds up the interface ([`Session::start_link_scan`], [`Session::poll_link_scan`]). The
//! desktop app looks every two seconds and when its window comes back to the front;
//! `links.updateChanged` looks on the spot (CLI, agents).
//!
//! Applying a new copy is not an undo step: the file changed, not the layout, so Undo keeps
//! undoing the user's own edits. The copy replaces the old one in the document and in the undo
//! and redo states that hold it, so undoing an earlier edit keeps the new pixels. The document
//! is modified (it keeps its graphics' bytes). Nothing is applied during a drag or another
//! interaction; the next look does it.
//!
//! Stamps are kept per document: one in the background is looked at when it comes to the front.
//! A file seen for the first time is only remembered (opening a document whose links changed
//! meanwhile leaves them to the Links panel). A file that can't be read or isn't an image (one
//! still being written, an unsupported EPS) is tried again only once it changes again, so it isn't
//! read over and over.

use std::collections::HashSet;
use std::sync::{Arc, Mutex, PoisonError};

use designcraft_doc::{Asset, AssetId, Document};
use serde_json::{Value, json};

use crate::{Result, Session};

/// A file's size and modification time (nanoseconds since 1970).
pub type Stamp = (u64, u128);

/// What a look found for one linked graphic.
#[derive(Clone, Debug)]
pub struct Look {
    pub asset: AssetId,
    pub path: String,
    /// The file's stamp now (`None`: not there right now).
    pub stamp: Option<Stamp>,
    /// The file's new bytes and pixel size, when its stamp moved and it reads as an image whose
    /// bytes differ from the document's copy.
    pub fresh: Option<(Arc<Vec<u8>>, (u32, u32))>,
}

/// A look taken on a worker thread: (document uid, what it found).
pub type LinkScan = Arc<Mutex<Option<(u64, Vec<Look>)>>>;

/// One linked graphic to look at: its file, the document's copy and the stamp last seen.
type Watched = (AssetId, String, Arc<Vec<u8>>, Option<Stamp>);

#[cfg(not(target_arch = "wasm32"))]
fn stamp(path: &str) -> Option<Stamp> {
    let meta = std::fs::metadata(path).ok()?;
    let modified = meta.modified().ok()?.duration_since(std::time::UNIX_EPOCH).ok()?;
    Some((meta.len(), modified.as_nanos()))
}

#[cfg(target_arch = "wasm32")]
fn stamp(_: &str) -> Option<Stamp> {
    None
}

/// The file's bytes and pixel size, when it reads as an image different from `copy`.
#[cfg(not(target_arch = "wasm32"))]
fn read_changed(path: &str, copy: &[u8]) -> Option<(Arc<Vec<u8>>, (u32, u32))> {
    let bytes = std::fs::read(path).ok()?;
    if bytes == copy {
        return None;
    }
    let px = designcraft_render::image_size(&bytes)?;
    Some((Arc::new(bytes), px))
}

#[cfg(target_arch = "wasm32")]
fn read_changed(_: &str, _: &[u8]) -> Option<(Arc<Vec<u8>>, (u32, u32))> {
    None
}

/// Stamp `files` and read the ones whose stamp moved (on a worker thread, or on the spot).
fn look(files: Vec<Watched>) -> Vec<Look> {
    files
        .into_iter()
        .map(|(asset, path, copy, before)| {
            let now = stamp(&path);
            let moved = matches!((before, now), (Some(b), Some(n)) if b != n);
            let fresh = if moved { read_changed(&path, &copy) } else { None };
            Look { asset, path, stamp: now, fresh }
        })
        .collect()
}

/// `links.updateChanged`: look at the active document's linked files now and update the ones that
/// changed.
pub(crate) fn update_changed(s: &mut Session, _: &Value) -> Result<Value> {
    let (uid, files) = s.watched().ok_or(crate::EngineError::NoDocument)?;
    let looks = look(files);
    s.apply_looks(uid, looks)
}

impl Session {
    /// The active document's uid and linked graphics, with the stamps last seen.
    fn watched(&self) -> Option<(u64, Vec<Watched>)> {
        let st = self.active()?;
        let files = st
            .doc
            .assets
            .values()
            .filter_map(|a| {
                let path = a.link.clone()?;
                let before = self.link_stamps.get(&(st.uid, path.clone())).copied();
                Some((a.id, path, a.data.clone(), before))
            })
            .collect();
        Some((st.uid, files))
    }

    /// Start looking at the active document's linked files on a worker thread, unless a look is
    /// still running (or the build has no files to look at: the web app).
    pub fn start_link_scan(&mut self) {
        if self.link_scan.is_some() || cfg!(target_arch = "wasm32") {
            return;
        }
        let Some((uid, files)) = self.watched() else { return };
        if files.is_empty() {
            return;
        }
        let slot: LinkScan = Arc::default();
        let out = slot.clone();
        let spawned = std::thread::Builder::new().name("link-watch".into()).spawn(move || {
            let looks = look(files);
            *out.lock().unwrap_or_else(PoisonError::into_inner) = Some((uid, looks));
        });
        if spawned.is_ok() {
            self.link_scan = Some(slot);
        }
    }

    /// Apply a finished look (`None` while there is none, or it is still running).
    pub fn poll_link_scan(&mut self) -> Option<Result<Value>> {
        let (uid, looks) = self.link_scan.as_ref()?.lock().unwrap_or_else(PoisonError::into_inner).take()?;
        self.link_scan = None;
        Some(self.apply_looks(uid, looks))
    }

    /// Remember the stamps a look found and put the new copies it read in document `uid`.
    /// Returns `{updated}`.
    fn apply_looks(&mut self, uid: u64, looks: Vec<Look>) -> Result<Value> {
        let none = json!({"updated": 0});
        // The document went to the back meanwhile, or the user is in the middle of a drag: the
        // stamps stay as they were, so a later look finds the change again.
        let Some(i) = self.docs.iter().position(|d| d.uid == uid).filter(|i| self.active == Some(*i)) else { return Ok(none) };
        if self.docs.get(i).is_some_and(|d| d.interaction.is_some()) {
            return Ok(none);
        }
        // Stamps of closed documents are dropped; a file not there right now keeps its last one.
        let open: HashSet<u64> = self.docs.iter().map(|d| d.uid).collect();
        self.link_stamps.retain(|(u, _), _| open.contains(u));
        for l in &looks {
            if let Some(s) = l.stamp {
                self.link_stamps.insert((uid, l.path.clone()), s);
            }
        }
        let fresh: Vec<(AssetId, String, Arc<Vec<u8>>, (u32, u32))> =
            looks.into_iter().filter_map(|l| l.fresh.map(|(bytes, px)| (l.asset, l.path, bytes, px))).collect();
        let Some(st) = self.docs.get_mut(i) else { return Ok(none) };
        let mut updated = 0;
        for (asset, path, bytes, px) in fresh {
            let Some(old) = st.doc.assets.get(&asset).filter(|a| a.link.as_deref() == Some(path.as_str())).cloned() else { continue };
            let new = Arc::new(Asset { data: bytes, pixels: Some(px), ..(*old).clone() });
            replace(Arc::make_mut(&mut st.doc), &old, &new);
            for e in st.history.undo.iter_mut().chain(st.history.redo.iter_mut()) {
                if e.doc.assets.get(&asset).is_some_and(|a| Arc::ptr_eq(a, &old)) {
                    replace(Arc::make_mut(&mut e.doc), &old, &new);
                }
            }
            updated += 1;
        }
        if updated > 0 {
            st.revision += 1;
        }
        Ok(json!({"updated": updated}))
    }
}

/// Put `new` in place of graphic `old` in `d`.
fn replace(d: &mut Document, old: &Arc<Asset>, new: &Arc<Asset>) {
    if let Some(a) = d.assets.get_mut(&old.id) {
        *a = new.clone();
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use crate::Session;

    fn png(w: u32, h: u32) -> Vec<u8> {
        designcraft_render::Rendered { width: w, height: h, pixels: vec![200u8; (w * h * 4) as usize] }.to_png()
    }

    /// Write `bytes` to `path` so its modification time moves even on coarse file systems.
    fn save(path: &std::path::Path, bytes: &[u8]) {
        std::thread::sleep(std::time::Duration::from_millis(30));
        std::fs::write(path, bytes).unwrap();
    }

    fn dir(name: &str) -> std::path::PathBuf {
        let d = std::env::temp_dir().join(format!("dc-link-watch-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    fn placed(s: &mut Session, path: &std::path::Path) {
        s.execute("file.new", &json!({})).unwrap();
        s.execute("file.place", &json!({"path": path.to_string_lossy(), "x": 72, "y": 72, "width": 72})).unwrap();
    }

    fn link(s: &mut Session) -> serde_json::Value {
        s.execute("links.list", &json!({})).unwrap()[0].clone()
    }

    fn look(s: &mut Session) -> serde_json::Value {
        s.execute("links.updateChanged", &json!({})).unwrap()
    }

    #[test]
    fn a_changed_file_updates_without_an_undo_step() {
        let d = dir("update");
        let a = d.join("a.png");
        std::fs::write(&a, png(10, 10)).unwrap();
        let mut s = Session::new();
        placed(&mut s, &a);
        s.execute("frame.create", &json!({"rect": [300, 300, 400, 400], "content": "text"})).unwrap();
        let steps = s.doc().unwrap().history.undo.len();
        // First look: remembered, nothing to update.
        assert_eq!(look(&mut s), json!({"updated": 0}));
        save(&a, &png(30, 10));
        assert_eq!(link(&mut s)["status"], "modified");
        assert_eq!(look(&mut s), json!({"updated": 1}));
        let l = link(&mut s);
        assert_eq!((l["status"].as_str(), &l["pixels"]), (Some("ok"), &json!([30, 10])));
        assert_eq!(s.doc().unwrap().history.undo.len(), steps, "the file changed, not the layout");
        assert!(s.doc().unwrap().is_dirty(), "the document keeps the new copy: it's modified");
        // Undo undoes the user's last edit (the frame), and the graphic keeps its new pixels.
        s.execute("edit.undo", &json!({})).unwrap();
        let l = link(&mut s);
        assert_eq!((l["status"].as_str(), &l["pixels"]), (Some("ok"), &json!([30, 10])));
        assert_eq!(look(&mut s), json!({"updated": 0}), "acted on once");
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn a_file_that_cant_be_read_is_tried_again_once_it_changes() {
        let d = dir("partial");
        let a = d.join("a.png");
        std::fs::write(&a, png(10, 10)).unwrap();
        let mut s = Session::new();
        placed(&mut s, &a);
        look(&mut s);
        // Another app has only written the start of the file so far.
        let full = png(20, 20);
        save(&a, &full[..16]);
        assert_eq!(look(&mut s), json!({"updated": 0}));
        assert_eq!(link(&mut s)["pixels"], json!([10, 10]), "the last good copy stays");
        // Not read again while it doesn't change.
        assert_eq!(look(&mut s), json!({"updated": 0}));
        save(&a, &full);
        assert_eq!(look(&mut s), json!({"updated": 1}));
        assert_eq!(link(&mut s)["pixels"], json!([20, 20]));
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn a_document_in_the_back_catches_up_in_front() {
        let d = dir("back");
        let a = d.join("a.png");
        std::fs::write(&a, png(10, 10)).unwrap();
        let mut s = Session::new();
        placed(&mut s, &a);
        look(&mut s);
        let first = s.active_index().unwrap();
        s.execute("file.new", &json!({})).unwrap();
        save(&a, &png(30, 10));
        assert_eq!(look(&mut s), json!({"updated": 0}), "the new document links nothing");
        s.set_active(first);
        assert_eq!(look(&mut s), json!({"updated": 1}));
        assert_eq!(link(&mut s)["pixels"], json!([30, 10]));
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn a_background_look_updates_changed_links() {
        let d = dir("scan");
        let a = d.join("a.png");
        std::fs::write(&a, png(10, 10)).unwrap();
        let mut s = Session::new();
        placed(&mut s, &a);
        let background = |s: &mut Session| {
            s.start_link_scan();
            let t0 = std::time::Instant::now();
            loop {
                if let Some(r) = s.poll_link_scan() {
                    return r.unwrap();
                }
                assert!(t0.elapsed().as_secs() < 10, "the look never finished");
                std::thread::yield_now();
            }
        };
        assert_eq!(background(&mut s), json!({"updated": 0}));
        save(&a, &png(30, 10));
        assert_eq!(background(&mut s), json!({"updated": 1}));
        assert!(s.link_scan.is_none());
        let _ = std::fs::remove_dir_all(&d);
    }
}
