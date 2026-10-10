//! Preferences › File Handling › Update Links Changed on Disk: a placed graphic whose file another
//! app saves (VectorCraft, PhotoCraft, Illustrator, Photoshop) is updated without Window › Links ›
//! Update Link.
//!
//! The active document's linked files are compared by size and modification time with what they
//! were when last seen; a file whose stamp moved is updated with `links.update`, which only
//! touches a link whose bytes really differ and is an undo step like a click on Update Link
//! (undoing it leaves the link modified until the file changes again). The desktop app takes the
//! stamps every two seconds and when its window comes back to the front, on a worker thread so
//! a file on a slow network share never holds up the interface ([`Session::start_link_scan`],
//! [`Session::poll_link_scan`]); `links.updateChanged` takes them on the spot (CLI, agents).
//!
//! Stamps are kept per document: one in the background is looked at when it comes to the
//! front. A file seen for the first time is only remembered (opening a document whose links
//! changed meanwhile leaves them to the Links panel); one that can't be read yet (another app is
//! still writing it) keeps its old stamp, so the next look tries again.

use std::collections::{HashMap, HashSet};
use std::sync::{Arc, Mutex, PoisonError};

use designcraft_doc::{AssetId, Document};
use serde_json::{Value, json};

use crate::{Result, Session};

/// A file's size and modification time (nanoseconds since 1970).
pub type Stamp = (u64, u128);

/// Stamps of a document's linked files taken on a worker thread: (document uid, [(graphic,
/// file, stamp)]).
pub type LinkScan = Arc<Mutex<Option<(u64, Vec<(AssetId, String, Option<Stamp>)>)>>>;

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

/// The linked graphics of `d` and their files.
fn linked(d: &Document) -> Vec<(AssetId, String)> {
    d.assets.values().filter_map(|a| a.link.clone().map(|l| (a.id, l))).collect()
}

/// `links.updateChanged`: stamp the active document's linked files now and update the ones
/// that changed.
pub(crate) fn update_changed(s: &mut Session, _: &Value) -> Result<Value> {
    let st = s.doc()?;
    let (uid, files) = (st.uid, linked(&st.doc));
    let now = files.into_iter().map(|(a, p)| (a, p.clone(), stamp(&p))).collect();
    s.apply_link_stamps(uid, now)
}

impl Session {
    /// Start stamping the active document's linked files on a worker thread, unless a look is
    /// still running (or the build has no files to look at: the web app).
    pub fn start_link_scan(&mut self) {
        if self.link_scan.is_some() || cfg!(target_arch = "wasm32") {
            return;
        }
        let Some(st) = self.active() else { return };
        let (uid, files) = (st.uid, linked(&st.doc));
        if files.is_empty() {
            return;
        }
        let slot: LinkScan = Arc::default();
        let out = slot.clone();
        let spawned = std::thread::Builder::new().name("link-watch".into()).spawn(move || {
            let stamps = files.into_iter().map(|(a, p)| (a, p.clone(), stamp(&p))).collect();
            *out.lock().unwrap_or_else(PoisonError::into_inner) = Some((uid, stamps));
        });
        if spawned.is_ok() {
            self.link_scan = Some(slot);
        }
    }

    /// Apply a finished look (`None` while there is none, or it is still running).
    pub fn poll_link_scan(&mut self) -> Option<Result<Value>> {
        let (uid, stamps) = self.link_scan.as_ref()?.lock().unwrap_or_else(PoisonError::into_inner).take()?;
        self.link_scan = None;
        Some(self.apply_link_stamps(uid, stamps))
    }

    /// Compare the stamps of document `uid`'s linked files with the ones last seen and update
    /// the graphics whose files changed. Returns `{updated}`.
    fn apply_link_stamps(&mut self, uid: u64, now: Vec<(AssetId, String, Option<Stamp>)>) -> Result<Value> {
        // The document went to the back meanwhile: it's looked at when it's in front again.
        if self.active().map(|d| d.uid) != Some(uid) {
            return Ok(json!({"updated": 0}));
        }
        // This document's last stamps; those of closed documents are dropped.
        let open: HashSet<u64> = self.documents().iter().map(|d| d.uid).collect();
        let mut before: HashMap<String, Stamp> = HashMap::new();
        self.link_stamps.retain(|(u, p), s| {
            if *u == uid {
                before.insert(p.clone(), *s);
                false
            } else {
                open.contains(u)
            }
        });
        let mut changed = Vec::new();
        for (asset, path, stamp) in now {
            match (before.get(&path), stamp) {
                // Not there right now (an app saving by delete and rename): keep the last stamp.
                (Some(old), None) => {
                    self.link_stamps.insert((uid, path), *old);
                }
                (old, Some(new)) => {
                    if old.is_some_and(|o| *o != new) {
                        changed.push((asset, path.clone()));
                    }
                    self.link_stamps.insert((uid, path), new);
                }
                (None, None) => {}
            }
        }
        let mut updated = 0;
        for (asset, path) in changed {
            match self.execute("links.update", &json!({"asset": asset.0})) {
                Ok(r) => updated += r["updated"].as_u64().unwrap_or(0),
                // Not readable yet (still being written): the next look tries again.
                Err(_) => {
                    if let Some(old) = before.get(&path) {
                        self.link_stamps.insert((uid, path), *old);
                    }
                }
            }
        }
        Ok(json!({"updated": updated}))
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

    #[test]
    fn a_link_whose_file_changed_updates_as_an_undo_step() {
        let d = dir("update");
        let a = d.join("a.png");
        std::fs::write(&a, png(10, 10)).unwrap();
        let mut s = Session::new();
        placed(&mut s, &a);
        // First look: remembered, nothing to update.
        assert_eq!(s.execute("links.updateChanged", &json!({})).unwrap(), json!({"updated": 0}));
        save(&a, &png(30, 10));
        assert_eq!(link(&mut s)["status"], "modified");
        assert_eq!(s.execute("links.updateChanged", &json!({})).unwrap(), json!({"updated": 1}));
        let l = link(&mut s);
        assert_eq!((l["status"].as_str(), &l["pixels"]), (Some("ok"), &json!([30, 10])));
        assert!(s.doc().unwrap().is_dirty(), "the document keeps the new copy: it's modified");
        // Undo puts the old copy back; the link stays modified until the file changes again.
        s.execute("edit.undo", &json!({})).unwrap();
        assert_eq!(link(&mut s)["status"], "modified");
        assert_eq!(s.execute("links.updateChanged", &json!({})).unwrap(), json!({"updated": 0}));
        save(&a, &png(40, 10));
        assert_eq!(s.execute("links.updateChanged", &json!({})).unwrap(), json!({"updated": 1}));
        assert_eq!(link(&mut s)["pixels"], json!([40, 10]));
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn a_file_still_being_written_is_tried_again() {
        let d = dir("partial");
        let a = d.join("a.png");
        std::fs::write(&a, png(10, 10)).unwrap();
        let mut s = Session::new();
        placed(&mut s, &a);
        s.execute("links.updateChanged", &json!({})).unwrap();
        // Another app has only written the start of the file so far.
        let full = png(20, 20);
        save(&a, &full[..16]);
        assert_eq!(s.execute("links.updateChanged", &json!({})).unwrap(), json!({"updated": 0}));
        assert_eq!(link(&mut s)["pixels"], json!([10, 10]), "the last good copy stays");
        save(&a, &full);
        assert_eq!(s.execute("links.updateChanged", &json!({})).unwrap(), json!({"updated": 1}));
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
        s.execute("links.updateChanged", &json!({})).unwrap();
        let first = s.active_index().unwrap();
        s.execute("file.new", &json!({})).unwrap();
        save(&a, &png(30, 10));
        assert_eq!(s.execute("links.updateChanged", &json!({})).unwrap(), json!({"updated": 0}), "the new document links nothing");
        s.set_active(first);
        assert_eq!(s.execute("links.updateChanged", &json!({})).unwrap(), json!({"updated": 1}));
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
        let look = |s: &mut Session| {
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
        assert_eq!(look(&mut s), json!({"updated": 0}));
        save(&a, &png(30, 10));
        assert_eq!(look(&mut s), json!({"updated": 1}));
        assert!(s.link_scan.is_none());
        let _ = std::fs::remove_dir_all(&d);
    }
}
