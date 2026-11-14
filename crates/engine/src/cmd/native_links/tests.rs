#![cfg(not(target_arch = "wasm32"))]

use std::path::{Path, PathBuf};
use std::sync::Arc;

use designcraft_doc::{Asset, AssetData, AssetId, LinkedSource, Storage, Unavailable};
use serde_json::json;

use super::{LinkStatus, wait};
use crate::Session;
use crate::asset_io::{content_hash, fingerprint_from};

struct Dir(PathBuf);

impl Dir {
    fn new(name: &str) -> Self {
        let dir = std::env::temp_dir().join(format!("dc-native-links-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join("links")).unwrap();
        Dir(dir)
    }
}

impl Drop for Dir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn png(red: u8) -> Vec<u8> {
    designcraft_render::Rendered { width: 40, height: 20, pixels: [red, 90, 180, 255].repeat(800) }.to_png()
}

/// Place `bytes` in the active document (embedded, from base64) → its asset.
fn place(s: &mut Session, bytes: &[u8]) -> AssetId {
    let before: Vec<AssetId> = s.doc().unwrap().doc.assets.keys().copied().collect();
    s.execute("file.place", &json!({"base64": crate::cmd::base64_encode(bytes), "name": "art.png", "x": 0, "y": 0})).unwrap();
    *s.doc().unwrap().doc.assets.keys().find(|id| !before.contains(id)).unwrap()
}

/// Make asset `id` a link to `file`, as Place from a file records one: linked storage when
/// `linked`, else embedded with its link, as documents saved before linked storage have it.
fn link_to(s: &mut Session, id: AssetId, file: &Path, linked: bool) {
    let bytes = std::fs::read(file).unwrap();
    let st = s.doc_mut().unwrap();
    let a = Arc::make_mut(Arc::make_mut(&mut st.doc).assets.get_mut(&id).unwrap());
    a.link = Some(file.to_string_lossy().into_owned());
    a.link_fingerprint = Some(fingerprint_from(&std::fs::metadata(file).unwrap()));
    if linked {
        a.link_hash = Some(content_hash(&bytes));
        a.storage = Storage::Linked;
        a.data = AssetData::linked(LinkedSource::of(a, None).unwrap());
    }
}

/// A saved document at `dir/doc.designcraft` linking to `dir/links/art.png` → its asset.
fn saved_linked(dir: &Dir) -> (PathBuf, AssetId) {
    let image = dir.0.join("links/art.png");
    std::fs::write(&image, png(30)).unwrap();
    age(&image, 100);
    let mut s = Session::new();
    s.execute("file.new", &json!({})).unwrap();
    let id = place(&mut s, &png(30));
    link_to(&mut s, id, &image, true);
    let doc = dir.0.join("doc.designcraft");
    s.execute("file.save", &json!({"path": doc.to_string_lossy()})).unwrap();
    (doc, id)
}

/// Open the file at `path`, with its background check of linked files still to come.
fn opened(path: &Path) -> Session {
    let mut s = Session::new();
    s.execute("file.open", &json!({"path": path.to_string_lossy()})).unwrap();
    s
}

/// Open the file at `path` and wait for its background check of linked files.
fn open(path: &Path) -> Session {
    let mut s = opened(path);
    wait(&mut s);
    s
}

fn asset(s: &Session, id: AssetId) -> Arc<Asset> {
    Arc::clone(&s.doc().unwrap().doc.assets[&id])
}

fn status(s: &Session, id: AssetId) -> Option<LinkStatus> {
    s.doc().unwrap().links.status(&asset(s, id))
}

/// The names of the entries in the package at `path` that hold placed files.
fn stored_assets(path: &Path) -> Vec<String> {
    let zip = zip::ZipArchive::new(std::fs::File::open(path).unwrap()).unwrap();
    zip.file_names().filter(|n| n.starts_with("assets/")).map(str::to_string).collect()
}

/// Set the file's modification time `secs` seconds into the past.
fn age(path: &Path, secs: u64) {
    let file = std::fs::OpenOptions::new().write(true).open(path).unwrap();
    file.set_modified(std::time::SystemTime::now() - std::time::Duration::from_secs(secs)).unwrap();
}

/// The bytes of `path` with one byte changed: the same size, other bytes.
fn flip_a_byte(path: &Path) {
    let mut bytes = std::fs::read(path).unwrap();
    let mid = bytes.len() / 2;
    bytes[mid] ^= 0xff;
    std::fs::write(path, bytes).unwrap();
}

#[test]
fn a_linked_save_writes_no_original_and_records_the_relative_path() {
    let dir = Dir::new("save");
    let (doc, id) = saved_linked(&dir);
    assert!(stored_assets(&doc).is_empty(), "{:?}", stored_assets(&doc));
    let s = open(&doc);
    let a = asset(&s, id);
    assert_eq!((a.storage, a.link_relative.as_deref()), (Storage::Linked, Some("links/art.png")));
    assert!(a.data.is_linked() && a.data.resident().is_none(), "nothing is read on open, nor by its check");
    assert_eq!(status(&s, id), Some(LinkStatus::Ok), "the recorded size and time: no check needed");
    assert!(!s.doc().unwrap().links.checking());
    assert_eq!(a.data.load().unwrap().as_slice(), png(30));
}

#[test]
fn a_moved_folder_finds_its_files_by_relative_path() {
    let dir = Dir::new("moved");
    let (_, id) = saved_linked(&dir);
    let moved = Dir(dir.0.with_file_name(format!("{}-moved", dir.0.file_name().unwrap().to_string_lossy())));
    let _ = std::fs::remove_dir_all(&moved.0);
    std::fs::rename(&dir.0, &moved.0).unwrap();
    let s = open(&moved.0.join("doc.designcraft"));
    let a = asset(&s, id);
    assert_eq!(status(&s, id), Some(LinkStatus::Ok));
    assert_eq!(a.link.as_deref(), Some(moved.0.join("links/art.png").to_string_lossy().as_ref()), "the file found is the link now");
    assert!(!s.doc().unwrap().is_dirty());
    assert_eq!(a.data.load().unwrap().as_slice(), png(30));
}

#[test]
fn the_absolute_file_wins_and_otherwise_the_one_with_the_bytes() {
    let dir = Dir::new("both");
    let (_, id) = saved_linked(&dir);
    // A copy of the folder: both paths hold the bytes, the copy under a new time.
    let copy = Dir::new("both-copy");
    std::fs::copy(dir.0.join("doc.designcraft"), copy.0.join("doc.designcraft")).unwrap();
    std::fs::copy(dir.0.join("links/art.png"), copy.0.join("links/art.png")).unwrap();
    age(&copy.0.join("links/art.png"), 50);
    let s = open(&copy.0.join("doc.designcraft"));
    assert_eq!(status(&s, id), Some(LinkStatus::Ok));
    assert_eq!(asset(&s, id).link.as_deref(), Some(dir.0.join("links/art.png").to_string_lossy().as_ref()), "the absolute path is tried first");

    // The original changes (same size): the copy holds the bytes, found by its hash.
    flip_a_byte(&dir.0.join("links/art.png"));
    age(&dir.0.join("links/art.png"), 10);
    let mut s = opened(&copy.0.join("doc.designcraft"));
    assert_eq!(status(&s, id), Some(LinkStatus::Checking));
    wait(&mut s);
    let a = asset(&s, id);
    assert_eq!(status(&s, id), Some(LinkStatus::Ok));
    assert_eq!(a.link.as_deref(), Some(copy.0.join("links/art.png").to_string_lossy().as_ref()));
    assert_eq!(a.link_fingerprint, Some(fingerprint_from(&std::fs::metadata(copy.0.join("links/art.png")).unwrap())));
    assert_eq!(a.data.load().unwrap().as_slice(), png(30));
}

#[test]
fn a_touched_file_with_the_same_bytes_is_ok_and_keeps_its_new_time() {
    let dir = Dir::new("touched");
    let (doc, id) = saved_linked(&dir);
    let image = dir.0.join("links/art.png");
    age(&image, 1000);
    let mut s = opened(&doc);
    let key = asset(&s, id).data.key();
    assert_eq!(status(&s, id), Some(LinkStatus::Checking));
    wait(&mut s);
    let a = asset(&s, id);
    assert_eq!(status(&s, id), Some(LinkStatus::Ok));
    assert_eq!(a.link_fingerprint, Some(fingerprint_from(&std::fs::metadata(&image).unwrap())), "the new time is kept");
    assert_eq!(a.data.key(), key, "the same bytes keep their key");
    assert!(!s.doc().unwrap().is_dirty(), "a new time is no change to save");
    assert!(!s.doc().unwrap().links.checking());
}

#[test]
fn changed_bytes_are_modified_and_never_adopted() {
    let dir = Dir::new("changed");
    let (doc, id) = saved_linked(&dir);
    let image = dir.0.join("links/art.png");
    let recorded = asset(&open(&doc), id).link_hash.clone();
    flip_a_byte(&image);
    age(&image, 1000);
    let mut s = open(&doc);
    wait(&mut s);
    let a = asset(&s, id);
    assert_eq!(status(&s, id), Some(LinkStatus::Modified));
    assert_eq!(a.link_hash, recorded);
    assert_eq!(a.data.load().err(), Some(Unavailable::Modified));
    // Another size: modified without reading it.
    std::fs::write(&image, b"short").unwrap();
    assert_eq!(status(&open(&doc), id), Some(LinkStatus::Modified));
    std::fs::remove_file(&image).unwrap();
    assert_eq!(status(&open(&doc), id), Some(LinkStatus::Missing));
}

#[test]
fn migration_links_confirmed_images_and_drops_the_copy_only_after_confirmation() {
    let dir = Dir::new("migrate");
    let (same, changed) = (dir.0.join("links/same.png"), dir.0.join("links/changed.png"));
    std::fs::write(&same, png(30)).unwrap();
    std::fs::write(&changed, png(31)).unwrap();
    age(&same, 100);
    let mut s = Session::new();
    s.execute("file.new", &json!({})).unwrap();
    let (a, b) = (place(&mut s, &png(30)), place(&mut s, &png(31)));
    link_to(&mut s, a, &same, false);
    link_to(&mut s, b, &changed, false);
    let doc = dir.0.join("doc.designcraft");
    s.execute("file.save", &json!({"path": doc.to_string_lossy()})).unwrap();
    assert_eq!(stored_assets(&doc).len(), 2, "a document as saved before linked storage");
    // One file changes since (same size, other bytes).
    flip_a_byte(&changed);

    let mut s = opened(&doc);
    assert!(asset(&s, a).data.embedded_bytes().is_some() && asset(&s, a).storage == Storage::Embedded, "nothing changes before the comparison");
    wait(&mut s);
    let (left, kept) = (asset(&s, a), asset(&s, b));
    assert_eq!(left.storage, Storage::Linked);
    assert_eq!(left.link_hash.as_deref(), Some(content_hash(&png(30)).as_str()));
    assert!(left.proxy.is_some(), "a proxy for when the file can't be read");
    assert!(left.data.embedded_bytes().is_some(), "the bytes stay until the save");
    assert_eq!((kept.storage, kept.link_hash.as_deref(), kept.data.embedded_bytes().is_some()), (Storage::Embedded, None, true));
    assert!(!s.doc().unwrap().is_dirty());
    let r = s.execute("links.progress", &json!({})).unwrap();
    assert_eq!(r, json!({"checking": false, "notice": "1 image will be saved as a link to its file (Links ▸ Embed keeps a copy)"}));
    assert_eq!(s.execute("links.progress", &json!({})).unwrap(), json!({"checking": false}), "told once");

    let key = left.data.key();
    s.execute("file.save", &json!({})).unwrap();
    assert_eq!(stored_assets(&doc), [format!("assets/{}.png", b.0)], "only the changed file's image stays in the document");
    let saved = asset(&s, a);
    assert!(saved.data.is_linked() && saved.data.key() == key, "linked now, under the key its decodes are cached by");
    assert_eq!(saved.data.load().unwrap().as_slice(), png(30));
    // Reopened: a linked image, and the changed one is compared again.
    let mut s = open(&doc);
    wait(&mut s);
    assert_eq!((asset(&s, a).storage, status(&s, a)), (Storage::Linked, Some(LinkStatus::Ok)));
    assert_eq!(asset(&s, b).storage, Storage::Embedded);
}

#[test]
fn byte_opened_and_recovered_documents_are_not_migrated_and_recovery_holds_no_originals() {
    let dir = Dir::new("recovery");
    let image = dir.0.join("links/art.png");
    std::fs::write(&image, png(30)).unwrap();
    let mut s = Session::new();
    s.execute("file.new", &json!({})).unwrap();
    let legacy = place(&mut s, &png(30));
    link_to(&mut s, legacy, &image, false);
    let bytes = s.execute("file.serialize", &json!({})).unwrap();
    let mut t = Session::new();
    let r = t.execute("file.openBytes", &json!({"name": "doc.designcraft", "base64": bytes["base64"]})).unwrap();
    assert_eq!(r["linkChecks"], 0, "{r}");

    let (doc, id) = saved_linked(&dir);
    let mut s = open(&doc);
    s.execute("frame.create", &json!({"rect": [36, 36, 300, 200]})).unwrap();
    let recovery = dir.0.join("recovery");
    s.execute("file.recovery.save", &json!({"dir": recovery.to_string_lossy()})).unwrap();
    let entry = recovery.join(format!("{}.designcraft", s.doc().unwrap().uid));
    assert!(entry.exists());
    assert!(stored_assets(&entry).is_empty(), "{:?}", stored_assets(&entry));
    drop(s);
    let mut r = Session::new();
    r.execute("file.recovery.open", &json!({"dir": recovery.to_string_lossy()})).unwrap();
    wait(&mut r);
    let a = asset(&r, id);
    assert_eq!((a.storage, a.link_relative.as_deref()), (Storage::Linked, Some("links/art.png")));
    assert_eq!(a.data.linked_source().and_then(|s| s.document.clone()), Some(doc.clone()), "resolved against the document's file");
    assert_eq!(status(&r, id), Some(LinkStatus::Ok));
    assert_eq!(a.data.load().unwrap().as_slice(), png(30));
}

#[test]
fn save_a_copy_with_embedded_links_is_portable() {
    let dir = Dir::new("portable");
    let (doc, id) = saved_linked(&dir);
    let mut s = open(&doc);
    let copy = dir.0.join("copy.designcraft");
    s.execute("file.saveACopy", &json!({"path": copy.to_string_lossy(), "embedLinks": true})).unwrap();
    assert_eq!(stored_assets(&copy), [format!("assets/{}.png", id.0)]);
    assert!(asset(&s, id).data.is_linked(), "the document itself stays linked");
    std::fs::remove_file(dir.0.join("links/art.png")).unwrap();
    let t = open(&copy);
    let a = asset(&t, id);
    assert_eq!((a.storage, a.data.embedded_bytes().map(|b| b.to_vec())), (Storage::Embedded, Some(png(30))));
    // A missing file (its bytes no longer kept in memory): no portable copy, and a plain one
    // still saves.
    asset(&s, id).data.evict();
    let e = s.execute("file.saveACopy", &json!({"path": copy.to_string_lossy(), "embedLinks": true})).unwrap_err();
    assert!(e.to_string().contains("art.png") && e.to_string().contains("missing"), "{e}");
    s.execute("file.saveACopy", &json!({"path": copy.to_string_lossy()})).unwrap();
    assert!(stored_assets(&copy).is_empty());
    assert!(s.execute("file.saveACopy", &json!({"path": copy.to_string_lossy(), "embedLinks": 1})).is_err());
}

#[test]
fn the_open_looks_for_files_in_the_background() {
    let dir = Dir::new("background");
    let (doc, id) = saved_linked(&dir);
    let mut s = opened(&doc);
    assert_eq!(status(&s, id), Some(LinkStatus::Checking), "nothing is asked of the file system on open");
    assert!(s.doc().unwrap().links.checking());
    wait(&mut s);
    assert_eq!(status(&s, id), Some(LinkStatus::Ok));
    assert!(!s.doc().unwrap().is_dirty());
}

/// The open's statuses don't outlive a change to the file: Update reads a file edited since,
/// and a file missing at the open counts once it is back.
#[test]
fn a_file_changed_or_restored_after_the_open_is_seen() {
    let dir = Dir::new("after-open");
    let (doc, id) = saved_linked(&dir);
    let image = dir.0.join("links/art.png");
    let mut s = open(&doc);
    assert_eq!(status(&s, id), Some(LinkStatus::Ok));
    std::fs::write(&image, png(77)).unwrap();
    age(&image, 10);
    let r = s.execute("links.update", &json!({"asset": id.0})).unwrap();
    assert_eq!(r["updated"], 1, "{r}");
    assert_eq!(asset(&s, id).data.load().unwrap().as_slice(), png(77));

    let restored = Dir::new("restored");
    let (doc, id) = saved_linked(&restored);
    let image = restored.0.join("links/art.png");
    let original = std::fs::read(&image).unwrap();
    std::fs::remove_file(&image).unwrap();
    let mut s = open(&doc);
    assert_eq!(status(&s, id), Some(LinkStatus::Missing));
    std::fs::write(&image, &original).unwrap();
    age(&image, 10);
    let rows = s.execute("links.list", &json!({})).unwrap();
    let row = rows.as_array().unwrap().iter().find(|r| r["asset"] == id.0).cloned().unwrap();
    assert_eq!(row["status"], "ok", "{row}");
}

#[test]
fn preflight_reads_no_linked_original() {
    let dir = Dir::new("preflight");
    let (doc, id) = saved_linked(&dir);
    let mut s = open(&doc);
    let issues = s.execute("preflight.run", &json!({})).unwrap();
    assert!(!issues.to_string().contains("modifiedLink"), "{issues}");
    assert!(asset(&s, id).data.resident().is_none(), "the original isn't read");
    flip_a_byte(&dir.0.join("links/art.png"));
    age(&dir.0.join("links/art.png"), 10);
    let issues = s.execute("preflight.run", &json!({})).unwrap();
    assert!(issues.to_string().contains("modifiedLink"), "{issues}");
}

#[test]
fn recovery_keeps_images_not_yet_saved_as_links() {
    let dir = Dir::new("recover-migrated");
    let image = dir.0.join("links/art.png");
    std::fs::write(&image, png(30)).unwrap();
    age(&image, 100);
    let mut s = Session::new();
    s.execute("file.new", &json!({})).unwrap();
    let id = place(&mut s, &png(30));
    link_to(&mut s, id, &image, false);
    let doc = dir.0.join("doc.designcraft");
    s.execute("file.save", &json!({"path": doc.to_string_lossy()})).unwrap();
    let mut s = open(&doc);
    assert_eq!(asset(&s, id).storage, Storage::Linked, "found identical to its file, to be saved as a link");
    s.execute("frame.create", &json!({"rect": [36, 36, 300, 200]})).unwrap();
    let recovery = dir.0.join("recovery");
    s.execute("file.recovery.save", &json!({"dir": recovery.to_string_lossy()})).unwrap();
    let entry = recovery.join(format!("{}.designcraft", s.doc().unwrap().uid));
    assert_eq!(stored_assets(&entry), [format!("assets/{}.png", id.0)], "the bytes stay in the snapshot");
    drop(s);
    let mut r = Session::new();
    r.execute("file.recovery.open", &json!({"dir": recovery.to_string_lossy()})).unwrap();
    wait(&mut r);
    let a = asset(&r, id);
    assert_eq!((a.storage, a.data.embedded_bytes().map(|b| b.to_vec())), (Storage::Embedded, Some(png(30))));
}
