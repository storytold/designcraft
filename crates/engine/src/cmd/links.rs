//! Window › Links: placed graphics with their status, effective resolution and uses; relink,
//! update, embed, go to.
//!
//! A graphic placed from a file links to it ([`Storage::Linked`]): the document keeps the file's
//! path (absolute, and relative to the document), size, time and content hash, and a proxy
//! image; the bytes are read from the file when needed. Its status is OK while a file at one of
//! those paths holds the recorded bytes (same size and time, or the same hash), Modified when
//! files are there with other bytes, and Missing otherwise. Embed copies the bytes into the
//! document and keeps the link record, so Unembed can link to the file again. An embedded copy
//! from a document saved before linked storage (a link without a hash) is compared with its file
//! byte for byte.
//!
//! A linked text file (File › Place with Create Links) remembers its file's size and hash on the
//! story instead; updating it imports the file again into the story.
//!
//! A link is followed only where [`crate::io::document_path`] allows it, resolved against the
//! document's own file; a refused link is missing, with the reason in `links.list`.

use std::path::Path;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use designcraft_compose::Cache;
use designcraft_doc::{
    Asset, AssetData, AssetId, Content, Document, Fingerprint, Item, ItemId, LinkedSource, Selection, Storage, Story, StoryId, TextFileLink,
};
use designcraft_geom::Affine;
use serde_json::{Value, json};

use super::{CommandSpec, bad, cmd, has_doc, str_param};
use crate::{EngineError, Result, Session};

pub fn specs() -> Vec<CommandSpec> {
    vec![
        cmd!(query "links.list", "Links", [], None, "{} → [{asset, name, path, storage: linked|embedded (where a save keeps the bytes), status: ok|modified|missing|embedded|checking (a linked file still being hashed), refused?: why a missing link isn't followed (a path on another computer, a device), pixels, pdfCrop (placed PDFs: the box shown), uses: [{id, page, ppi}]}], then linked text files: [{story, name, path, status (also tooLarge: unchanged size, too big to check), edited (the story was changed in the layout), uses: [{id, page}]}]", has_doc, |s, _| {
            // Taken out for the call so the document needn't be cloned on every poll.
            let mut checks = std::mem::take(&mut s.link_checks);
            let links = s.doc().map(|d| list(&d.doc, &s.cache, d.path.as_deref().map(Path::new), Some(&mut checks), None, Some(&d.links)).unwrap_or_default());
            s.link_checks = checks;
            Ok(Value::Array(links?))
        }),
        cmd!(
            "links.relink",
            "Relink…",
            ["Window", "Links"],
            None,
            "{asset | story, path} — use another file for every placement of the graphic (keeps the selected PDF page; error if missing), or import another text file into the linked story",
            has_doc,
            relink
        ),
        cmd!(
            "links.relinkFolder",
            "Relink to Folder…",
            ["Window", "Links"],
            None,
            "{dir?: folder to look in (default: each link's own folder), extension?: e.g. \"tif\" (Relink File Extension), assets?: [ids], missingOnly?: bool (default true with `dir`)} → {relinked, notFound: [names]}",
            has_doc,
            relink_folder
        ),
        cmd!(
            "links.relinkFiles",
            "Relink to Files",
            [],
            None,
            "{files: [{name, base64, path?: the file's path within a chosen folder}], asset?: id} — relink from files handed over by name (the web build's file and folder pickers, which give no path on disk): with `asset`, that graphic from the first file; otherwise each missing graphic from the file whose path ends most like its link (the same name at least, any case), all in one undo step — a graphic those can't tell apart (two equally likely files, or one file named only like graphics linking to different files) stays missing. A file named like the link keeps the link's path → {relinked, notFound: [names no file matched], ambiguous: [names left missing], failed: [{name, error}: matching files that couldn't be read, their graphics left missing], unused: [files that matched nothing]}",
            has_doc,
            relink_files
        ),
        cmd!(
            "links.update",
            "Update Link",
            ["Window", "Links"],
            None,
            "{asset? | story?} — reload modified links from their files (all when neither); a linked text file is imported again into its story with the import options it was placed with, replacing edits made in the layout → {updated, warnings}",
            has_doc,
            update
        ),
        cmd!(
            "links.embed",
            "Embed Link",
            ["Window", "Links"],
            None,
            "{asset | story} — copy a linked graphic's file into the document (its link is kept for Unembed); fails when the file is missing or has changed (update or relink it first). A text file: unlink the story",
            has_doc,
            embed
        ),
        cmd!(
            "links.unembed",
            "Unembed Link…",
            ["Window", "Links"],
            None,
            "{asset, dir?} — link an embedded graphic to a file instead of keeping it in the document: its original file when that still holds the same bytes, else a file written into `dir` (an existing one with the same bytes is reused) → {path}. Not on the web",
            can_unembed,
            unembed
        ),
        cmd!(noundo "links.goTo", "Go To Link", ["Window", "Links"], None, "{asset | story} — selects the frames showing the graphic or the linked text", has_doc, |s, p| {
            let st = s.doc_mut()?;
            let ids = match story_param(p) {
                Some(sid) => st.doc.story(sid).map(|s| s.frames.clone()).unwrap_or_default(),
                None => uses(&st.doc, asset_param(p, "links.goTo")?),
            };
            if ids.is_empty() {
                return Err(bad("links.goTo", "the graphic isn't placed"));
            }
            st.selection = Selection::items(ids.clone());
            st.bump_revision();
            Ok(json!({"ids": ids.iter().map(|i| i.0).collect::<Vec<_>>()}))
        }),
    ]
}

fn asset_param(p: &Value, cmd: &str) -> Result<AssetId> {
    p.get("asset").and_then(Value::as_u64).map(AssetId).ok_or_else(|| bad(cmd, "missing asset"))
}

fn story_param(p: &Value) -> Option<StoryId> {
    p.get("story").and_then(Value::as_u64).map(StoryId)
}

/// FNV-1a (64-bit) of `bytes`, in hex: a linked text file's fingerprint.
pub(crate) fn fnv_hex(bytes: &[u8]) -> String {
    let h = bytes.iter().fold(0xcbf2_9ce4_8422_2325_u64, |h, b| (h ^ u64::from(*b)).wrapping_mul(0x0000_0100_0000_01b3));
    format!("{h:016x}")
}

/// Fingerprint of a story's text and formatting (edits made in the layout change it).
pub(crate) fn story_hash(st: &Story) -> String {
    fnv_hex(serde_json::to_string(&(&st.text, &st.paras, &st.chars)).unwrap_or_default().as_bytes())
}

/// Frames (graphic content) showing an asset, top-level or in groups.
fn uses(d: &Document, aid: AssetId) -> Vec<ItemId> {
    let mut out = Vec::new();
    for sp in d.spreads.iter().chain(&d.parents) {
        for it in &sp.items {
            it.walk(&mut |i| {
                if let Content::Graphic(g) = &i.content
                    && g.asset == aid
                {
                    out.push(i.id);
                }
            });
        }
    }
    out
}

/// Why a link stored in the document at `document` isn't followed, if it isn't.
pub fn refused(link: &str, document: Option<&Path>) -> Option<crate::io::Refused> {
    crate::io::document_path(link, document).err()
}

/// Whether an asset's linked file is gone ([`status_cached`] "missing" without reading the file). An
/// asset can hold data while its file is missing: the document's preview of it. `document` is
/// the document's file, which relative links are resolved against.
pub fn link_missing(a: &designcraft_doc::Asset, document: Option<&Path>) -> bool {
    let Some(link) = &a.link else { return false };
    #[cfg(not(target_arch = "wasm32"))]
    {
        if a.storage == Storage::Linked {
            return match LinkedSource::of(a, document) {
                Some(source) => !crate::asset_io::candidates(&source).iter().any(crate::io::is_file),
                None => true,
            };
        }
        !crate::io::document_path(link, document).is_ok_and(crate::io::exists)
    }
    #[cfg(target_arch = "wasm32")]
    {
        let _ = (link, document);
        false
    }
}

/// Linked images checked this session: (asset, the identity of its bytes, the file its link
/// resolves to) → (the file's size and time when it was read, whether it matched the bytes). The
/// resolved file, not the link text, is the key: a relative link names another file once the
/// document is saved elsewhere. The bytes' [`designcraft_doc::SourceKey`] (as the render cache
/// keys them) changes with every relink or update and is never reused, so a remembered answer
/// can never describe other bytes.
pub(crate) type ImageChecks = std::collections::HashMap<(AssetId, designcraft_doc::SourceKey, std::path::PathBuf), (Fingerprint, bool)>;

/// The size and time to keep with `bytes` just read from a linked file, `before` being the
/// file's from before the read. Kept only when they can vouch for the bytes later: the size is
/// the bytes' (the file didn't change while it was read) and the time is [`settled`] (another
/// write in the same second, a file still being synced, would keep it). A linked text file's time
/// is kept on the same terms.
pub(crate) fn loaded_fingerprint(before: Option<Fingerprint>, bytes: &[u8]) -> Option<Fingerprint> {
    before.filter(|f| f.size == bytes.len() as u64 && settled(f))
}

/// The modification time kept with a linked file whose time couldn't vouch for its bytes when
/// they were read ([`loaded_fingerprint`]): no file has it, so the file is told by its hash until
/// a check finds it holding the bytes and keeps its real time ([`remember_link_files`]).
pub(crate) const UNVERIFIED_MTIME: i64 = i64::MIN;

/// The file bytes for a graphic were just read from.
pub(crate) struct FileLink {
    /// The file's absolute path.
    path: String,
    /// Its size and time from before the read.
    before: Option<Fingerprint>,
}

impl FileLink {
    pub(crate) fn of(path: &Path, before: Option<Fingerprint>) -> Self {
        let path = std::path::absolute(path).unwrap_or_else(|_| path.to_path_buf()).to_string_lossy().into_owned();
        FileLink { path, before }
    }
}

/// The proxy a linked graphic is drawn with while its file can't be read: none for video and
/// sound, or a file that doesn't decode.
pub(super) fn proxy_of(a: &Asset, bytes: &[u8]) -> Option<designcraft_doc::Proxy> {
    if a.mime.starts_with("video/") || a.mime.starts_with("audio/") {
        return None;
    }
    let p = designcraft_render::proxy::make_proxy(bytes, a.page)?;
    Some(designcraft_doc::Proxy { mime: p.mime.into(), pixels: p.pixels, data: AssetData::embedded(p.data) })
}

/// Give `a` the bytes of a graphic being placed, relinked or updated (its `mime` and `page` set).
/// Bytes read from a file (`file`) link to it ([`link_to_file`]), unless `embed` or on the web,
/// where they are embedded with the link record kept. Other bytes (pasted, dropped, base64) are
/// embedded without a link.
pub(crate) fn give_bytes(a: &mut Asset, bytes: impl Into<Arc<Vec<u8>>>, file: Option<FileLink>, embed: bool, document: Option<&Path>) {
    let bytes: Arc<Vec<u8>> = bytes.into();
    a.proxy = None;
    let Some(file) = file else {
        a.set_link(None, None);
        a.link_relative = None;
        a.link_hash = None;
        a.storage = Storage::Embedded;
        a.data = AssetData::embedded(bytes);
        return;
    };
    if embed || cfg!(target_arch = "wasm32") {
        record_file(a, &bytes, file, document);
        a.link_hash = Some(crate::asset_io::content_hash(&bytes));
        a.storage = Storage::Embedded;
        a.data = AssetData::embedded(bytes);
        return;
    }
    link_to_file(a, bytes, file, document, None);
}

/// [`give_bytes`] for a graphic being placed, leaving out the work that grows with the file: the
/// graphic links to its file at once, with the file's size and time as its fingerprint and the
/// bytes just read kept in memory for drawing, and its content hash and proxy are made in the
/// background ([`super::native_links::prepare`] the returned work once the asset is in the
/// document; [`add_prepared`] adds them). A link without a hash is complete: its status goes by
/// size and time until the hash is in. Placing many press-size images would otherwise decode and
/// hash each one before the app responds again. Embedded placements, bytes without a file and
/// the web (no threads) are [`give_bytes`].
#[must_use = "the hash and proxy are made only once the returned work is queued"]
pub(crate) fn give_placed(
    a: &mut Asset,
    bytes: impl Into<Arc<Vec<u8>>>,
    file: Option<FileLink>,
    embed: bool,
    document: Option<&Path>,
) -> Option<super::native_links::Pending> {
    let bytes: Arc<Vec<u8>> = bytes.into();
    let file = match file {
        Some(file) if !embed && cfg!(not(target_arch = "wasm32")) => file,
        file => {
            give_bytes(a, bytes, file, embed, document);
            return None;
        }
    };
    record_file(a, &bytes, file, document);
    a.link_hash = None;
    link_recorded(a, Arc::clone(&bytes), designcraft_doc::SourceKey::fresh(), document, |_, _| None);
    Some(super::native_links::Pending::of(a, bytes))
}

/// Link `a` to `file`, whose bytes (`bytes`) were just read: the file's record (paths, size and
/// time, content hash) and a proxy to draw while the file can't be read (`preview` when there is
/// one, else made from the bytes). The bytes stay in memory for the first draw.
pub(crate) fn link_to_file(a: &mut Asset, bytes: Arc<Vec<u8>>, file: FileLink, document: Option<&Path>, preview: Option<designcraft_doc::Proxy>) {
    record_file(a, &bytes, file, document);
    a.link_hash = Some(crate::asset_io::content_hash(&bytes));
    link_recorded(a, bytes, designcraft_doc::SourceKey::fresh(), document, |a, bytes| preview.or_else(|| proxy_of(a, bytes)));
}

/// Add to a graphic placed with [`give_placed`] (or opened with a link that has no hash) its
/// content `hash` and `proxy`, made from the bytes it has under its current key: the hash when it
/// has none, the proxy when it is linked and has none. Its storage stays as it is (a graphic
/// embedded since with Links › Embed stays embedded). → whether `a` changed.
pub(super) fn add_prepared(a: &mut Asset, hash: &str, proxy: Option<&designcraft_doc::Proxy>) -> bool {
    let mut changed = false;
    if a.link_hash.is_none() {
        a.link_hash = Some(hash.to_owned());
        // The same bytes: the key stays, so nothing decoded from them is decoded again.
        if let Some(mut source) = a.data.linked_source().cloned() {
            source.hash = Some(hash.to_owned());
            a.data = AssetData::linked_under(a.data.key(), source);
        }
        changed = true;
    }
    if a.storage == Storage::Linked
        && a.proxy.is_none()
        && let Some(proxy) = proxy
    {
        a.proxy = Some(proxy.clone());
        changed = true;
    }
    changed
}

/// `a`, whose link record is complete, as a link to its file under `key`: drawn with `proxy`'s
/// image while the file can't be read, and `bytes` (just read from the file) kept in memory for
/// the first draw. Embedded under `key` when the record doesn't name a file.
fn link_recorded(
    a: &mut Asset,
    bytes: Arc<Vec<u8>>,
    key: designcraft_doc::SourceKey,
    document: Option<&Path>,
    proxy: impl FnOnce(&Asset, &[u8]) -> Option<designcraft_doc::Proxy>,
) {
    a.link_fingerprint = Some(a.link_fingerprint.unwrap_or(Fingerprint { size: bytes.len() as u64, mtime: UNVERIFIED_MTIME }));
    let Some(source) = LinkedSource::of(a, document) else {
        a.storage = Storage::Embedded;
        a.proxy = None;
        a.data = AssetData::embedded_under(key, bytes);
        return;
    };
    a.storage = Storage::Linked;
    a.proxy = proxy(a, &bytes);
    a.data = AssetData::linked_under(key, source);
    a.data.keep_resident(bytes);
}

/// Keep with `a` the record of `file`, which `bytes` were read from: its absolute path, its path
/// relative to the `document` file and the size and time that vouch for the bytes
/// ([`loaded_fingerprint`]). The caller sets the content hash.
fn record_file(a: &mut Asset, bytes: &[u8], file: FileLink, document: Option<&Path>) {
    a.link_fingerprint = loaded_fingerprint(file.before, bytes);
    a.link_relative = document.and_then(|d| super::native_links::relative_to(d, Path::new(&file.path)));
    a.link = Some(file.path);
}

/// Does `bytes` (a file's) hold `a`'s bytes: its recorded hash, or, without one, its bytes?
#[cfg(not(target_arch = "wasm32"))]
fn holds(a: &Asset, bytes: &[u8]) -> bool {
    match &a.link_hash {
        Some(hash) => *hash == crate::asset_io::content_hash(bytes),
        None => a.data.resident().is_some_and(|d| *d == bytes),
    }
}

/// Status of a linked graphic: OK when a file at its absolute or relative path has the recorded
/// size and time (nothing is read), or the recorded size and the recorded bytes (hashed once per
/// new time, `checks`); Modified when files are there and none holds them; Missing when none is.
fn linked_status(a: &Asset, document: Option<&Path>, checks: &mut ImageChecks) -> (&'static str, Option<Fingerprint>) {
    let Some(source) = LinkedSource::of(a, document) else { return ("missing", None) };
    #[cfg(not(target_arch = "wasm32"))]
    {
        let mut status = "missing";
        for path in crate::asset_io::candidates(&source) {
            let Some(meta) = crate::io::file_metadata(&path) else { continue };
            let now = crate::asset_io::fingerprint_from(&meta);
            if Some(now) == source.fingerprint && settled(&now) {
                return ("ok", None);
            }
            status = "modified";
            if now.size != source.len {
                continue;
            }
            let key = (a.id, a.data.key(), path.clone());
            let same = match checks.get(&key) {
                Some((at, same)) if *at == now => *same,
                _ => {
                    let Ok(bytes) = crate::io::read_capped(&path, source.len) else { continue };
                    let same = bytes.len() as u64 == source.len && holds(a, &bytes);
                    if settled(&now) {
                        checks.insert(key, (now, same));
                    }
                    same
                }
            };
            if same {
                return ("ok", settled(&now).then_some(now));
            }
        }
        (status, None)
    }
    #[cfg(target_arch = "wasm32")]
    {
        let _ = (source, checks);
        ("missing", None)
    }
}

/// Whether a linked image's files are there and none holds its bytes ([`linked_status`]
/// "modified"): Preflight's check, which reads a file only as the Links panel does, once per new
/// size and time (`checks`).
pub(crate) fn link_modified(a: &Asset, document: Option<&Path>, checks: &mut LinkListChecks) -> bool {
    a.storage == Storage::Linked && linked_status(a, document, &mut checks.images).0 == "modified"
}

/// Is the linked file still the one the asset's bytes were loaded from, going by the size and
/// time kept then ([`designcraft_doc::Asset::link_fingerprint`], [`loaded_fingerprint`])? Nothing
/// is read when it is, also on the first check after a document is opened, which matters on a
/// network share. A fingerprint of another size than the bytes isn't theirs and doesn't count; a
/// new time or no fingerprint leave it to the byte comparison.
#[cfg(not(target_arch = "wasm32"))]
fn loaded_unchanged(a: &designcraft_doc::Asset, now: Fingerprint) -> bool {
    a.link_fingerprint.is_some_and(|loaded| loaded.size == a.data.len() && loaded == now && settled(&now))
}

/// Link status of a placed graphic (`ok`, `modified`, `missing`, `embedded`), reading the file
/// only when its size is the asset's and its time is neither the one the bytes were loaded at
/// ([`loaded_unchanged`]) nor the one remembered from the last read (`checks`): the Links panel
/// polls, and a document of press-size images would otherwise be read whole on every poll. A file
/// changed in the last two seconds is never remembered (see [`settled`]).
#[cfg(test)]
pub(crate) fn status_cached(a: &designcraft_doc::Asset, document: Option<&Path>, checks: &mut ImageChecks) -> &'static str {
    image_status(a, document, checks, None).0
}

/// Whether the automation read roots let this session read a file at one of `a`'s link paths
/// (always without roots in force, and then nothing is asked of the file system).
fn may_read_link(a: &Asset, document: Option<&Path>) -> bool {
    #[cfg(not(target_arch = "wasm32"))]
    {
        LinkedSource::of(a, document).is_some_and(|s| crate::asset_io::candidates(&s).iter().any(|p| crate::file_access::check_read(p).is_ok()))
    }
    #[cfg(target_arch = "wasm32")]
    {
        let _ = (a, document);
        true
    }
}

/// Whether `a`'s bytes are the document's own copy, with no file to compare them with: embedded
/// with no link, or with the link record Embed keeps (it has a hash; a copy saved before linked
/// storage has none, and is compared with its file).
fn embedded_copy(a: &Asset) -> bool {
    a.storage == Storage::Embedded && (a.link.is_none() || a.link_hash.is_some())
}

/// [`status_cached`], and the file's size and time when a read found the asset's bytes in it
/// although the fingerprint kept with the asset says nothing or something else: what
/// [`remember_link_files`] keeps, so the file isn't read again in another session. While the
/// background check the open of the document started (`open`) still has the asset's file to look
/// at, it is `checking`; after that the file is looked at as it is now, which may have changed
/// since.
fn image_status(
    a: &designcraft_doc::Asset,
    document: Option<&Path>,
    checks: &mut ImageChecks,
    open: Option<&super::native_links::OpenLinks>,
) -> (&'static str, Option<Fingerprint>) {
    if embedded_copy(a) {
        return ("embedded", None);
    }
    if a.storage == Storage::Linked {
        if open.is_some_and(|o| o.checking() && o.status(a) == Some(super::native_links::LinkStatus::Checking)) {
            // A file this session may not read (outside the automation read roots) is missing,
            // whatever the background check finds.
            return (if may_read_link(a, document) { "checking" } else { "missing" }, None);
        }
        return linked_status(a, document, checks);
    }
    let Some(link) = &a.link else { return ("embedded", None) };
    // Nothing was read from the file (a package's link that wasn't followed): missing, as in
    // Preflight.
    if a.data.is_empty() {
        return ("missing", None);
    }
    #[cfg(not(target_arch = "wasm32"))]
    {
        let Ok(path) = crate::io::document_path(link, document) else { return ("missing", None) };
        if !crate::io::is_file(&path) {
            return ("missing", None);
        }
        let now = match crate::io::metadata(&path) {
            Err(_) => return ("missing", None),
            Ok(m) => super::datamerge::fingerprint_from(&m),
        };
        if loaded_unchanged(a, now) {
            return ("ok", None);
        }
        if now.size != a.data.len() {
            return ("modified", None);
        }
        let key = (a.id, a.data.key(), path.clone());
        if let Some((at, same)) = checks.get(&key)
            && *at == now
        {
            return if *same { ("ok", Some(now)) } else { ("modified", None) };
        }
        let same = match crate::io::read_capped(&path, a.data.len()) {
            Ok(b) => a.data.load().is_ok_and(|d| b == *d),
            Err(_) => return ("missing", None),
        };
        // The time from before the read: a change during it is seen next time.
        let settled = settled(&now);
        if settled {
            checks.insert(key, (now, same));
        }
        if same { ("ok", settled.then_some(now)) } else { ("modified", None) }
    }
    #[cfg(target_arch = "wasm32")]
    {
        let _ = (link, document, checks);
        ("ok", None)
    }
}

/// The largest linked text file read (the Data Merge source limit). Only regular files are read,
/// so a link to a FIFO or a device (e.g. `/dev/zero`) can't block or fill memory.
const MAX_TEXT_LINK_BYTES: u64 = super::datamerge::MAX_DATA_FILE_BYTES;

/// Can a linked text file this size be checked and updated?
pub(crate) fn checkable_size(len: usize) -> bool {
    len as u64 <= MAX_TEXT_LINK_BYTES
}

/// Can a size and time be trusted to change when the file does? Times are whole seconds, so not
/// while the file was changed in the last two: another change in that second keeps the time. A
/// time in the future (clock skew, a file copied from another machine) counts as settled, or such
/// a file would be read on every poll; a later save gives it a time unlike the remembered one.
/// A file without a time ([`crate::asset_io::has_time`]) never is.
pub(crate) fn settled(f: &Fingerprint) -> bool {
    #[cfg(not(target_arch = "wasm32"))]
    {
        if !crate::asset_io::has_time(f) {
            return false;
        }
        let now = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| i64::try_from(d.as_secs()).unwrap_or(i64::MAX));
        f.mtime > now || now.saturating_sub(f.mtime) >= 2
    }
    #[cfg(target_arch = "wasm32")]
    {
        let _ = f;
        false
    }
}

/// Checked linked text files: path → (size and time, hash of the contents then).
pub(crate) type TextChecks = std::collections::HashMap<String, (Fingerprint, String)>;

/// Link status of a linked text file: `tooLarge` when it is the recorded size but over
/// [`MAX_TEXT_LINK_BYTES`], so its contents aren't checked. The file is read only when its size
/// is the recorded one but its time isn't, and then once per new time (`checks`): a file saved
/// or synced again without changes stays OK. `document` is the document's file.
pub(crate) fn text_status(l: &TextFileLink, checks: &mut TextChecks, document: Option<&Path>) -> &'static str {
    #[cfg(not(target_arch = "wasm32"))]
    {
        let Ok(path) = crate::io::document_path(&l.path, document) else { return "missing" };
        if !crate::io::is_file(&path) {
            return "missing";
        }
        let now = match crate::io::metadata(&path) {
            Err(_) => return "missing",
            Ok(m) => super::datamerge::fingerprint_from(&m),
        };
        if now.size != l.size {
            return "modified";
        }
        if l.mtime != 0 && now.mtime == l.mtime {
            return "ok";
        }
        let hash = match checks.get(&l.path) {
            Some((at, hash)) if *at == now => hash.clone(),
            _ if now.size > MAX_TEXT_LINK_BYTES => return "tooLarge",
            _ => match read_text(&path) {
                Ok((b, at)) => {
                    let hash = fnv_hex(&b);
                    // The time from before the read: a change during it is seen next time.
                    if settled(&at) {
                        checks.insert(l.path.clone(), (at, hash.clone()));
                    }
                    hash
                }
                Err(_) => return "missing",
            },
        };
        if hash == l.hash { "ok" } else { "modified" }
    }
    #[cfg(target_arch = "wasm32")]
    {
        let _ = (l, checks, document);
        "ok"
    }
}

fn file_name(path: &str) -> String {
    std::path::Path::new(path).file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_else(|| path.to_string())
}

/// A graphic showing an asset: the item, its page, and the transform from its parent's space to
/// the spread.
struct Shown<'a> {
    item: &'a Item,
    page: Option<usize>,
    parent_xf: Affine,
}

/// Where an asset shows: frames on spreads (see [`uses`]) and anchored or inline objects, whose
/// page and position come from the frame their story lays them out in.
fn shown<'a>(d: &'a Document, cache: &Cache, aid: AssetId) -> Vec<Shown<'a>> {
    fn walk<'a>(it: &'a Item, xf: Affine, aid: AssetId, page: Option<usize>, out: &mut Vec<Shown<'a>>) {
        if let Content::Graphic(g) = &it.content
            && g.asset == aid
        {
            out.push(Shown { item: it, page, parent_xf: xf });
        }
        for c in it.children() {
            walk(c, xf * it.xf, aid, page, out);
        }
    }
    let mut out: Vec<Shown> = uses(d, aid)
        .into_iter()
        .filter_map(|id| {
            let loc = d.find(id)?;
            Some(Shown { item: d.item_at(&loc)?, page: d.page_of_item(id), parent_xf: d.parent_xf(&loc) })
        })
        .collect();
    for st in d.stories.values() {
        let mut composed = None;
        for (k, o) in st.objects.iter().enumerate() {
            let mut found = false;
            o.item.walk(&mut |i| found |= matches!(&i.content, Content::Graphic(g) if g.asset == aid));
            if !found {
                continue;
            }
            let cs = composed.get_or_insert_with(|| cache.get(d, st.id, None));
            let placed = cs.frames.iter().find_map(|ft| ft.objects.iter().find(|p| p.index == k).map(|p| (ft.frame, p.origin)));
            // Overset: no page; the resolution of the object as it would show.
            let (page, xf) = match placed {
                Some((fid, origin)) => {
                    let frame_xf =
                        d.find(fid).map_or(Affine::IDENTITY, |loc| d.parent_xf(&loc)) * d.item(fid).map_or(Affine::IDENTITY, |i| d.text_xf(i));
                    (d.page_of_item(fid), frame_xf * Affine::translate(origin.to_vec2()))
                }
                None => (None, Affine::IDENTITY),
            };
            walk(&o.item, xf, aid, page, &mut out);
        }
    }
    out
}

/// What a check of a document's links remembers between runs: the linked text files and images
/// already compared with the document ([`TextChecks`], [`ImageChecks`]), so a poll reads nothing
/// unchanged. The [`Session`] keeps one for `links.list`; a caller that lists a document's links
/// itself ([`list_document_links`]) keeps its own.
#[derive(Default)]
pub struct LinkListChecks {
    text: TextChecks,
    images: ImageChecks,
    /// Linked images the latest list read and found to hold the document's bytes, with the file's
    /// size and time then ([`LinkListChecks::take_verified`]).
    verified: Vec<(AssetId, Fingerprint)>,
}

impl LinkListChecks {
    /// The linked images the latest list found unchanged by comparing bytes, because the size
    /// and time kept with them said nothing or something else: [`remember_link_files`] keeps the
    /// file's current ones with them.
    pub fn take_verified(&mut self) -> Vec<(AssetId, Fingerprint)> {
        std::mem::take(&mut self.verified)
    }

    /// Add what `other` remembers (the checks of another worker thread).
    pub fn absorb(&mut self, other: LinkListChecks) {
        self.text.extend(other.text);
        self.images.extend(other.images);
    }

    /// How many files are remembered.
    pub fn len(&self) -> usize {
        self.text.len() + self.images.len()
    }

    /// Is nothing remembered yet?
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }
}

/// The rows of `links.list` without looking at a linked file: `checking` stands where a file
/// would be read or asked for its size and time. A native UI shows these at once and fills in
/// what [`list_document_links`] finds on a worker thread, where a share that doesn't answer
/// can't stall it.
pub fn preview_document_links(d: &Document, cache: &Cache, document: Option<&Path>) -> Vec<Value> {
    list(d, cache, document, None, None, None).unwrap_or_default()
}

/// The rows of `links.list` for a document outside a [`Session`], so a native UI can check the
/// linked files on a worker thread. `checks` is what the worker remembers between calls. Another
/// thread sets `stop` to end the check before its next file (the one being asked may be on a
/// share that doesn't answer, and hundreds more may follow it): nothing is returned then.
pub fn list_document_links(
    d: &Document,
    cache: &Cache,
    document: Option<&Path>,
    checks: &mut LinkListChecks,
    stop: &AtomicBool,
) -> Option<Vec<Value>> {
    list(d, cache, document, Some(checks), Some(stop), None)
}

/// Keep, with each linked image in `files`, the size and time its file had when a check read it
/// and found the document's bytes in it ([`LinkListChecks::take_verified`]). Later checks, and
/// the first one of another session, then don't read it: copying, restoring or checking out a
/// project folder gives every file a new time. `doc` is the document the check ran on: nothing
/// is kept when the active document isn't that one any more (other bytes may be linked by now),
/// or during a gesture. Nothing placed changes, so a saved document stays saved; the times go
/// into its file with the next save. → how many were kept
pub fn remember_link_files(s: &mut Session, doc: &Arc<Document>, files: &[(AssetId, Fingerprint)]) -> usize {
    let Ok(st) = s.doc_mut() else { return 0 };
    if !Arc::ptr_eq(&st.doc, doc) || st.interaction.is_some() {
        return 0;
    }
    let stale = |a: &designcraft_doc::Asset, f: &Fingerprint| a.link.is_some() && a.data.len() == f.size && a.link_fingerprint != Some(*f);
    let fresh: Vec<(AssetId, Fingerprint)> = files.iter().filter(|(id, f)| st.doc.assets.get(id).is_some_and(|a| stale(a, f))).copied().collect();
    if fresh.is_empty() {
        return 0;
    }
    let saved = !st.is_dirty();
    let document = st.path.clone();
    let d = Arc::make_mut(&mut st.doc);
    for (id, f) in &fresh {
        if let Some(a) = d.assets.get_mut(id) {
            let a = Arc::make_mut(a);
            a.link_fingerprint = Some(*f);
            // Linked bytes are read by that time too (the same bytes, found in the file: the key
            // stays, also for a link with no hash yet).
            if a.data.is_linked()
                && let Some(source) = LinkedSource::of(a, document.as_deref().map(Path::new))
            {
                a.data = AssetData::linked_under(a.data.key(), source);
            }
        }
    }
    if saved {
        st.saved_doc = Arc::clone(&st.doc);
    }
    fresh.len()
}

/// `links.list`: every used placed graphic, then every linked text file, each with its status.
/// `files` is what was read this session (a poll reads nothing unchanged); without it no file is
/// looked at and a status that needs one is `checking` ([`preview_document_links`]). `None` when
/// `stop` was set before the last link was reached.
fn list(
    d: &Document,
    cache: &Cache,
    document: Option<&Path>,
    files: Option<&mut LinkListChecks>,
    stop: Option<&AtomicBool>,
    open: Option<&super::native_links::OpenLinks>,
) -> Option<Vec<Value>> {
    let stopped = || stop.is_some_and(|stop| stop.load(Ordering::Relaxed));
    let (mut checks, mut images, mut verified) = match files {
        Some(files) => {
            files.verified.clear();
            (Some(&mut files.text), Some(&mut files.images), Some(&mut files.verified))
        }
        None => (None, None, None),
    };
    let texts = d.stories.values().filter_map(|st| Some((st, st.file_link.as_ref()?))).map(|(st, l)| {
        if stopped() {
            return None;
        }
        let uses: Vec<Value> = st.frames.iter().map(|id| json!({"id": id.0, "page": d.page_of_item(*id).map(|p| d.page_name(p))})).collect();
        let why = refused(&l.path, document).map(|r| r.to_string());
        let status = match checks.as_deref_mut() {
            Some(checks) => text_status(l, checks, document),
            None if why.is_some() => "missing",
            None => "checking",
        };
        Some(json!({"story": st.id.0, "name": file_name(&l.path), "path": l.path, "status": status, "refused": why, "edited": story_hash(st) != l.story_hash, "uses": uses}))
    });
    let used = d.used_assets();
    d.assets
        .values()
        .filter(|a| used.contains(&a.id))
        .map(|a| {
            if stopped() {
                return None;
            }
            let uses: Vec<Value> = shown(d, cache, a.id)
                .into_iter()
                .map(|u| {
                    let it = u.item;
                    // Effective resolution: image pixels over the size it is shown at.
                    let ppi = (|| {
                        let Content::Graphic(g) = &it.content else { return None };
                        // Vector (placed PDF or SVG): no effective resolution.
                        if a.mime == "application/pdf" || a.mime == "image/svg+xml" {
                            return None;
                        }
                        let (pw, _) = a.pixels?;
                        let xf = u.parent_xf * it.xf * g.xf;
                        let shown_w = xf.transform_rect_bbox(designcraft_geom::Rect::new(0.0, 0.0, g.size.0, g.size.1)).width();
                        (shown_w > 0.0).then(|| (pw as f64 / (shown_w / 72.0)).round())
                    })();
                    let page = u.page.map(|p| d.page_name(p));
                    json!({"id": it.id.0, "page": page, "ppi": ppi})
                })
                .collect();
            let pdf_crop = (a.mime == "application/pdf").then(|| a.pdf_crop.name());
            let why = a.link.as_deref().and_then(|l| refused(l, document)).map(|r| r.to_string());
            let status = match images.as_deref_mut() {
                Some(images) => {
                    let (status, file) = image_status(a, document, images, open);
                    if let (Some(verified), Some(file)) = (verified.as_deref_mut(), file) {
                        verified.push((a.id, file));
                    }
                    status
                }
                None if embedded_copy(a) => "embedded",
                None if a.data.is_empty() || why.is_some() => "missing",
                None => open.and_then(|o| o.status(a)).map_or("checking", |s| s.as_str()),
            };
            let storage = match a.storage {
                Storage::Linked => "linked",
                Storage::Embedded => "embedded",
            };
            Some(json!({"asset": a.id.0, "name": a.name, "path": a.link, "storage": storage, "status": status, "refused": why, "pixels": a.pixels, "pdfCrop": pdf_crop, "uses": uses}))
        })
        .chain(texts)
        .collect()
}

/// A linked text file's bytes, with its size and time from before they were read.
#[cfg(not(target_arch = "wasm32"))]
fn read_text(path: impl AsRef<Path>) -> Result<(Vec<u8>, Fingerprint)> {
    let path = path.as_ref();
    let err = |e: std::io::Error| EngineError::Other(format!("{}: {e}", path.display()));
    // The roots' own message when they refuse the path.
    crate::file_access::check_read(path).map_err(EngineError::Other)?;
    let m = crate::io::metadata(path).map_err(err)?;
    if !crate::io::is_file(path) {
        return Err(EngineError::Other(format!("{}: not a file", path.display())));
    }
    let too_large = || EngineError::Other(format!("{}: larger than {} MiB", path.display(), MAX_TEXT_LINK_BYTES / (1024 * 1024)));
    if m.len() > MAX_TEXT_LINK_BYTES {
        return Err(too_large());
    }
    // At most one byte over the limit, in case the file grew since.
    let b = crate::io::read_capped(path, MAX_TEXT_LINK_BYTES).map_err(err)?;
    if b.len() as u64 > MAX_TEXT_LINK_BYTES {
        return Err(too_large());
    }
    Ok((b, super::datamerge::fingerprint_from(&m)))
}

#[cfg(target_arch = "wasm32")]
fn read_text(path: impl AsRef<Path>) -> Result<(Vec<u8>, Fingerprint)> {
    Err(EngineError::Other(format!("{}: files can't be read on the web", path.as_ref().display())))
}

/// Import a linked text file again; a warning says when edits made in the layout were replaced.
fn reimport(d: &mut Document, sid: StoryId, path: &str, bytes: &[u8], before: Fingerprint) -> Result<Vec<String>> {
    let edited = d.story(sid).is_some_and(|st| st.file_link.as_ref().is_some_and(|l| story_hash(st) != l.story_hash));
    let mut warnings = super::place_text::reimport(d, sid, path, bytes, Some(before))?;
    if edited {
        warnings.insert(0, format!("{}: edits made to story {} in the layout were replaced by the file's text", file_name(path), sid.0));
    }
    Ok(warnings)
}

#[cfg(not(target_arch = "wasm32"))]
fn reload(path: impl AsRef<Path>) -> Result<(Vec<u8>, (u32, u32))> {
    reload_capped(path, super::interchange::MAX_LINKED_FILE_BYTES)
}

/// A linked graphic read again from disk, and its pixel size, reading at most `cap` bytes.
///
/// How large a linked file is belongs to whoever wrote the document, not to whoever opens it: the
/// path comes out of the document, and Update, Relink and Relink Folder all follow it. An
/// unbounded `read` of a link pointing at a multi-GB file exhausted memory (#1596). The cap is
/// the one packaging already applies to a linked file, and a file over it is refused by name
/// rather than read.
#[cfg(not(target_arch = "wasm32"))]
fn reload_capped(path: impl AsRef<Path>, cap: u64) -> Result<(Vec<u8>, (u32, u32))> {
    let path = path.as_ref();
    let b = crate::io::read_capped(path, cap).map_err(|e| EngineError::Other(format!("{}: {e}", path.display())))?;
    if b.len() as u64 > cap {
        return Err(bad("links", format!("{}: the linked file is larger than {} MB", path.display(), cap / (1024 * 1024))));
    }
    if designcraft_render::is_pdf(&b) {
        designcraft_render::check_pdf_input_size(b.len()).map_err(|e| bad("links", format!("{}: {e}", path.display())))?;
    }
    let px = designcraft_render::image_size(&b).ok_or_else(|| bad("links", format!("{}: unsupported or corrupt image", path.display())))?;
    Ok((b, px))
}

#[cfg(target_arch = "wasm32")]
fn reload(path: impl AsRef<Path>) -> Result<(Vec<u8>, (u32, u32))> {
    Err(EngineError::Other(format!("{}: files can't be read on the web", path.as_ref().display())))
}

/// The replacement for `a`: bytes read from the file at `read`, linked to it (its absolute path,
/// and its path relative to the `document` file) with a new hash and proxy.
fn relinked_asset(a: &designcraft_doc::Asset, read: &Path, document: Option<&Path>) -> Result<Arc<designcraft_doc::Asset>> {
    // The size and time from before the read: a change during it is seen by the next check.
    let before = super::datamerge::fingerprint_of(read);
    let (bytes, px) = reload(read)?;
    let mut a = relinked_from(a, &bytes, px, &read.display().to_string())?;
    a.name = file_name(&read.to_string_lossy());
    let file = FileLink::of(read, before);
    give_bytes(&mut a, bytes, Some(file), false, document);
    Ok(Arc::new(a))
}

/// [`relinked_asset`] from a file handed over as bytes with its name (the browser's file and
/// folder pickers give no path). The bytes become the document's own copy. A file of the same
/// name as the link keeps the link's record, so the document still knows the file on the computer
/// the link came from; another name becomes the link.
fn relinked_bytes(a: &designcraft_doc::Asset, name: &str, bytes: Vec<u8>) -> Result<Arc<designcraft_doc::Asset>> {
    let px = designcraft_render::image_size(&bytes).ok_or_else(|| bad("links", format!("{name}: unsupported or corrupt image")))?;
    let mut a = relinked_from(a, &bytes, px, name)?;
    let same = a.link.as_deref().is_some_and(|old| link_file_name(old).eq_ignore_ascii_case(name));
    if !same {
        a.set_link(Some(name.to_string()), None);
        a.link_relative = None;
    } else {
        // The file's size and time on that computer aren't these bytes'.
        let link = a.link.clone();
        a.set_link(link, None);
    }
    // The link kept may come from another platform: named after its last part, either separator.
    a.name = a.link.as_deref().map_or_else(|| name.to_string(), |l| link_file_name(l).to_string());
    a.proxy = None;
    a.link_hash = Some(crate::asset_io::content_hash(&bytes));
    a.storage = Storage::Embedded;
    a.data = AssetData::embedded(Arc::new(bytes));
    Ok(Arc::new(a))
}

/// The file name at the end of a link (either kind of separator).
fn link_file_name(link: &str) -> &str {
    link.rsplit(['/', '\\']).next().unwrap_or(link)
}

/// `a` with a new file's type, page and pixel size (`px`), its PDF crop found on the new bytes
/// (`label` names the file in errors). The caller gives it the bytes and its name.
fn relinked_from(a: &designcraft_doc::Asset, bytes: &[u8], px: (u32, u32), label: &str) -> Result<designcraft_doc::Asset> {
    let page = if designcraft_render::is_pdf(bytes) { a.page } else { 0 };
    let px = if page > 0 {
        designcraft_render::pdf_page_size(bytes, page as usize)
            .map(|(w, h)| (w.round().max(1.0) as u32, h.round().max(1.0) as u32))
            .ok_or_else(|| bad("links", format!("{label}: can't read PDF page {}", u64::from(page) + 1)))?
    } else {
        px
    };
    let mut a = a.clone();
    a.mime = designcraft_render::image_mime(bytes).to_string();
    a.pixels = Some(px);
    a.page = page;
    // A placed PDF keeps its crop choice; its box is found on the new file.
    super::file::crop_pdf_from(&mut a, bytes);
    Ok(a)
}

/// The file Update reads `a`'s bytes from: the first of a linked graphic's paths that holds a
/// file, or where the link of an embedded copy resolves.
fn update_source(a: &Asset, document: Option<&Path>) -> Option<std::path::PathBuf> {
    if a.storage == Storage::Linked {
        let source = LinkedSource::of(a, document)?;
        return crate::asset_io::candidates(&source).into_iter().find(|p| crate::io::is_file(p));
    }
    crate::io::document_path(a.link.as_deref()?, document).ok()
}

/// `links.relink`: a placed graphic or linked text file takes its bytes from `path` from now on.
fn relink(s: &mut Session, p: &Value) -> Result<Value> {
    let path = str_param(p, "path").ok_or_else(|| bad("links.relink", "missing path"))?.to_string();
    if let Some(sid) = story_param(p) {
        let (bytes, before) = read_text(&path)?;
        let name = file_name(&path);
        return s.edit(|d, _| {
            let warnings = reimport(d, sid, &path, &bytes, before)?;
            Ok(json!({"name": name, "warnings": warnings}))
        });
    }
    let aid = asset_param(p, "links.relink")?;
    let st = s.doc()?;
    let a = st.doc.assets.get(&aid).ok_or_else(|| bad("links.relink", "no such graphic"))?;
    // Build the replacement before the edit: a file that cannot be read leaves the document as it was.
    let fresh = relinked_asset(a, Path::new(&path), st.path.as_deref().map(Path::new))?;
    let r = s.edit(|d, _| {
        let name = fresh.name.clone();
        d.assets.insert(aid, fresh);
        Ok(json!({"name": name}))
    });
    s.link_checks.images.clear();
    designcraft_doc::asset_data::links_changed();
    r
}

/// `links.relinkFolder`: relink graphics to same-named files in a folder and/or with another
/// extension (the missing ones by default).
fn relink_folder(s: &mut Session, p: &Value) -> Result<Value> {
    let dir = str_param(p, "dir").map(std::path::PathBuf::from);
    if let Some(dir) = &dir {
        // Refused outright: candidates outside the automation read root would only look missing.
        crate::file_access::check_read(dir).map_err(EngineError::Other)?;
    }
    let ext = str_param(p, "extension").map(|e| e.trim_start_matches('.').to_string());
    if ext.as_ref().is_some_and(|e| e.contains(['/', '\\', '\0'])) {
        return Err(bad("links.relinkFolder", "extension must not contain path separators or NUL"));
    }
    if dir.is_none() && ext.is_none() {
        return Err(bad("links.relinkFolder", "give `dir` and/or `extension`"));
    }
    let missing_only = p.get("missingOnly").and_then(Value::as_bool).unwrap_or(dir.is_some() && ext.is_none());
    let only: Option<Vec<u64>> = p.get("assets").and_then(Value::as_array).map(|a| a.iter().filter_map(Value::as_u64).collect());
    let mut images = std::mem::take(&mut s.link_checks.images);
    let st = s.doc()?;
    let (d, document) = (&st.doc, st.path.as_deref().map(Path::new));
    let mut todo: Vec<(AssetId, std::path::PathBuf)> = Vec::new();
    let mut not_found = Vec::new();
    for a in d.assets.values() {
        if only.as_ref().is_some_and(|o| !o.contains(&a.id.0))
            || (missing_only && image_status(a, document, &mut images, Some(&st.links)).0 != "missing")
        {
            continue;
        }
        let old = std::path::PathBuf::from(a.link.clone().unwrap_or_else(|| a.name.clone()));
        let mut name = old.file_name().map(std::ffi::OsStr::to_os_string).unwrap_or_else(|| a.name.clone().into());
        if let Some(e) = &ext {
            name = std::path::Path::new(&name).with_extension(e).into_os_string();
        }
        // The link's own folder only where the link may be followed; the name only as a plain
        // file name (not a device).
        let folder = match &dir {
            Some(dir) => Some(dir.clone()),
            None => a.link.as_deref().and_then(|l| crate::io::document_path(l, document).ok()).and_then(|p| p.parent().map(Path::to_path_buf)),
        };
        let plain_name = name.to_str().filter(|n| !n.contains(['/', '\\']));
        // A name that resolves to another file name (a `file:` URL such as `file:%2Ftmp%2Fx.png`,
        // a drive-relative `C:x.png`) or a folder the stored-path rules read differently (`a\b`
        // on Unix, `alias/..` through a symlink) could lead outside the folder, so only the name
        // itself, directly in the folder as the file system resolves it, counts. The link keeps
        // the folder as given.
        let cand = folder.zip(plain_name).and_then(|(f, n)| {
            let c = crate::io::folder_path(n, &f).ok()?;
            let real = |p: &Path| crate::io::canonicalize(p).ok();
            let same_folder = c.parent().and_then(real).is_some_and(|p| real(&f) == Some(p));
            (c.file_name().is_some_and(|c| c == n) && same_folder).then_some(c)
        });
        if let Some(cand) = cand.filter(|c| crate::io::exists(c)) {
            todo.push((a.id, cand));
        } else {
            not_found.push(name.to_string_lossy().to_string());
        }
    }
    // Load every replacement before committing any of them: a bad file must not leave an
    // unrecorded partial edit. Successful batches commit once and have one undo step.
    let mut fresh = Vec::new();
    for (aid, path) in todo {
        let a = d.assets.get(&aid).ok_or_else(|| bad("links.relinkFolder", "no such graphic"))?;
        fresh.push((aid, relinked_asset(a, &path, document)?));
    }
    let n = fresh.len();
    if n > 0 {
        s.edit(|d, _| {
            for (aid, a) in fresh {
                d.assets.insert(aid, a);
            }
            Ok(())
        })?;
        images.clear();
        designcraft_doc::asset_data::links_changed();
    }
    s.link_checks.images = images;
    Ok(json!({"relinked": n, "notFound": not_found}))
}

/// The most files one `links.relinkFiles` takes (a Links folder is rarely more than a few hundred).
const MAX_RELINK_FILES: usize = 2000;

/// The path components of a link or a file's path in a chosen folder, either separator.
fn path_parts(path: &str) -> Vec<&str> {
    path.split(['/', '\\']).filter(|c| !c.is_empty()).collect()
}

/// How many trailing path components `file` shares with `link` (any case): 1 for the file name
/// alone, more when its folders match the link's too.
fn shared_tail(file: &str, link: &str) -> usize {
    path_parts(file).iter().rev().zip(path_parts(link).iter().rev()).take_while(|(a, b)| a.eq_ignore_ascii_case(b)).count()
}

/// Which handed-over file each missing graphic takes (`links.relinkFiles` without `asset`).
/// A graphic takes the file whose path ends most like its link: the same name at least. Where
/// that can't tell files apart — two files of the graphic's name equally like its link, or one
/// file named only like two graphics whose links name different files — the graphic is left
/// missing and reported, rather than given another file's picture. Graphics linking to the same
/// file share it.
/// Also returns which files are named like some missing graphic (matched or not), so a file left
/// ambiguous isn't reported as matching nothing.
fn match_files(missing: &[(AssetId, String)], files: &[(String, String)]) -> (Vec<(AssetId, usize)>, Vec<String>, Vec<String>, Vec<bool>) {
    let mut named = vec![false; files.len()];
    let mut chosen = Vec::new();
    let mut not_found = Vec::new();
    let mut ambiguous = Vec::new();
    let mut picks: Vec<(AssetId, usize, usize, &str)> = Vec::new();
    for (aid, link) in missing {
        let scores: Vec<(usize, usize)> =
            files.iter().enumerate().map(|(k, (_, path))| (k, shared_tail(path, link))).filter(|(_, n)| *n > 0).collect();
        for (k, _) in &scores {
            if let Some(n) = named.get_mut(*k) {
                *n = true;
            }
        }
        let best = scores.iter().map(|(_, n)| *n).max().unwrap_or(0);
        let top: Vec<usize> = scores.iter().filter(|(_, n)| *n == best).map(|(k, _)| *k).collect();
        match top.as_slice() {
            [] => not_found.push(link_file_name(link).to_string()),
            [k] => picks.push((*aid, *k, best, link.as_str())),
            _ => ambiguous.push(link_file_name(link).to_string()),
        }
    }
    let same = |x: &str, y: &str| path_parts(x).iter().map(|c| c.to_ascii_lowercase()).eq(path_parts(y).iter().map(|c| c.to_ascii_lowercase()));
    for (aid, k, score, link) in &picks {
        let rivals = picks.iter().filter(|(other, k2, _, _)| other != aid && k2 == k);
        let top = rivals.clone().map(|(_, _, n, _)| *n).max().unwrap_or(0);
        if top > *score {
            // The file ends more like another graphic's link: it is that graphic's.
            not_found.push(link_file_name(link).to_string());
        } else if rivals.filter(|(_, _, n, _)| *n == *score).any(|(_, _, _, link2)| !same(link2, link)) {
            // Equally like graphics linking to different files: which it is can't be told.
            ambiguous.push(link_file_name(link).to_string());
        } else {
            chosen.push((*aid, *k));
        }
    }
    (chosen, not_found, ambiguous, named)
}

fn relink_files(s: &mut Session, p: &Value) -> Result<Value> {
    const ID: &str = "links.relinkFiles";
    let list = p.get("files").and_then(Value::as_array).ok_or_else(|| bad(ID, "missing files"))?;
    if list.len() > MAX_RELINK_FILES {
        return Err(bad(ID, format!("at most {MAX_RELINK_FILES} files")));
    }
    // (file name, path within the chosen folder: the name when there is none, base64)
    let mut files: Vec<(String, String)> = Vec::new();
    let mut data: Vec<&str> = Vec::new();
    for f in list {
        let name = f.get("name").and_then(Value::as_str).ok_or_else(|| bad(ID, "each file needs a name"))?;
        let b64 = f.get("base64").and_then(Value::as_str).ok_or_else(|| bad(ID, "each file needs base64"))?;
        let path = f.get("path").and_then(Value::as_str).filter(|p| !p.is_empty()).unwrap_or(name);
        files.push((link_file_name(name).to_string(), path.to_string()));
        data.push(b64);
    }
    let st = s.doc()?;
    let (d, document) = (&st.doc, st.path.as_deref().map(Path::new));
    // Every replacement is built before the edit: a bad file leaves the document as it was.
    let mut fresh = Vec::new();
    let mut used = vec![false; files.len()];
    let (mut not_found, mut ambiguous, mut failed) = (Vec::new(), Vec::new(), Vec::new());
    if let Some(aid) = p.get("asset").and_then(Value::as_u64).map(AssetId) {
        let a = d.assets.get(&aid).ok_or_else(|| bad(ID, "no such graphic"))?;
        let ((name, _), b64) = files.first().zip(data.first()).ok_or_else(|| bad(ID, "no file"))?;
        fresh.push((aid, relinked_bytes(a, name, super::base64_decode(b64))?));
        if let Some(u) = used.first_mut() {
            *u = true;
        }
    } else {
        let placed = d.used_assets();
        let mut checks = ImageChecks::new();
        // Graphics still placed: a replaced graphic kept for undo isn't relinked behind the page.
        let missing: Vec<(AssetId, String)> = d
            .assets
            .values()
            .filter(|a| placed.contains(&a.id) && image_status(a, document, &mut checks, None).0 == "missing")
            .map(|a| (a.id, a.link.clone().unwrap_or_else(|| a.name.clone())))
            .collect();
        let (chosen, named);
        (chosen, not_found, ambiguous, named) = match_files(&missing, &files);
        // A file named like a missing graphic was meant for it, even when left ambiguous.
        for (u, n) in used.iter_mut().zip(&named) {
            *u |= *n;
        }
        for (aid, k) in chosen {
            let (Some(a), Some((name, _)), Some(b64)) = (d.assets.get(&aid), files.get(k), data.get(k)) else { continue };
            if let Some(u) = used.get_mut(k) {
                *u = true;
            }
            // A file that can't be read leaves its graphic missing and the others relinking.
            match relinked_bytes(a, name, super::base64_decode(b64)) {
                Ok(fresh_asset) => fresh.push((aid, fresh_asset)),
                Err(e) => failed.push(json!({"name": name, "error": e.to_string()})),
            }
        }
    }
    let unused: Vec<String> = files.iter().zip(&used).filter(|(_, u)| !**u).map(|((n, _), _)| n.clone()).collect();
    let n = fresh.len();
    if n > 0 {
        s.edit(|d, _| {
            for (aid, a) in fresh {
                d.assets.insert(aid, a);
            }
            Ok(())
        })?;
        s.link_checks.images.clear();
        designcraft_doc::asset_data::links_changed();
    }
    Ok(json!({"relinked": n, "notFound": not_found, "ambiguous": ambiguous, "failed": failed, "unused": unused}))
}

/// Unembed reads and writes files: not on the web.
fn can_unembed(s: &Session) -> std::result::Result<(), String> {
    has_doc(s)?;
    if cfg!(target_arch = "wasm32") {
        return Err("files can't be linked on the web".into());
    }
    Ok(())
}

/// `links.embed`: the graphic's bytes go into the document, checked against the link record.
fn embed(s: &mut Session, p: &Value) -> Result<Value> {
    const ID: &str = "links.embed";
    if let Some(sid) = story_param(p) {
        return s.edit(|d, _| {
            d.story_mut(sid).ok_or_else(|| bad(ID, "no such story"))?.file_link = None;
            Ok(Value::Null)
        });
    }
    let aid = asset_param(p, ID)?;
    let st = s.doc()?;
    let document = st.path.as_deref().map(Path::new);
    let a = st.doc.assets.get(&aid).ok_or_else(|| bad(ID, "no such graphic"))?;
    if a.storage == Storage::Linked {
        if cfg!(target_arch = "wasm32") {
            return Err(bad(ID, format!("{}: linked files can't be read on the web", a.name)));
        }
        // Checked even when the bytes are in memory: what is embedded is what the file holds.
        match linked_status(a, document, &mut ImageChecks::default()).0 {
            "missing" => return Err(bad(ID, format!("{}: the linked file is missing; relink it, then embed it", a.name))),
            "modified" => return Err(bad(ID, format!("{}: the linked file has changed; update or relink it, then embed it", a.name))),
            _ => {}
        }
    }
    let bytes = a.data.load().map_err(|e| bad(ID, format!("{}: {e}", a.name)))?;
    // A link without a hash would be taken for a copy saved before linked storage.
    let hash = a.link_hash.clone().or_else(|| a.link.is_some().then(|| crate::asset_io::content_hash(&bytes)));
    let key = a.data.key();
    s.edit(|d, _| {
        let a = Arc::make_mut(d.assets.get_mut(&aid).ok_or_else(|| bad(ID, "no such graphic"))?);
        // The same bytes: what was decoded from them stays usable.
        a.data = AssetData::embedded_under(key, bytes);
        a.storage = Storage::Embedded;
        a.proxy = None;
        a.link_hash = hash;
        Ok(Value::Null)
    })?;
    Ok(Value::Null)
}

/// The file in `folder` named like the original (or `name` with an extension for `mime`) that
/// holds `bytes`: an existing one with exactly these bytes, else a new one written there.
#[cfg(not(target_arch = "wasm32"))]
fn write_into(folder: &Path, name: &str, mime: &str, bytes: &[u8]) -> Result<std::path::PathBuf> {
    const ID: &str = "links.unembed";
    if !crate::io::is_dir(folder) {
        return Err(bad(ID, format!("{}: not a folder", folder.display())));
    }
    let file = Path::new(name).file_name().map(|n| n.to_string_lossy().into_owned()).filter(|n| !n.is_empty()).unwrap_or_else(|| "graphic".into());
    let mut file = std::path::PathBuf::from(file);
    if file.extension().is_none() {
        let ext = match mime {
            "image/jpeg" => "jpg",
            "image/gif" => "gif",
            "image/tiff" => "tif",
            "image/webp" => "webp",
            "image/svg+xml" => "svg",
            "application/pdf" => "pdf",
            "application/postscript" => "eps",
            "image/png" => "png",
            _ => "bin",
        };
        file.set_extension(ext);
    }
    let stem = file.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default();
    let ext = file.extension().map(|e| e.to_string_lossy().into_owned()).unwrap_or_default();
    for n in 0..1000u32 {
        let candidate = if n == 0 { folder.join(&file) } else { folder.join(format!("{stem}-{n}.{ext}")) };
        if !crate::io::exists(&candidate) {
            crate::io::write_file(&candidate, bytes).map_err(EngineError::Other)?;
            return Ok(candidate);
        }
        if file_holds(&candidate, bytes) {
            return Ok(candidate);
        }
    }
    Err(bad(ID, format!("{}: no free file name for {}", folder.display(), file.display())))
}

/// Does the regular file at `path` hold exactly `bytes`?
#[cfg(not(target_arch = "wasm32"))]
fn file_holds(path: &Path, bytes: &[u8]) -> bool {
    crate::io::file_metadata(path).is_some_and(|m| m.len() == bytes.len() as u64)
        && crate::io::read_capped(path, bytes.len() as u64).is_ok_and(|b| b == bytes)
}

/// `links.unembed`: an embedded graphic links to its original file, or to a copy written into `dir`.
fn unembed(s: &mut Session, p: &Value) -> Result<Value> {
    const ID: &str = "links.unembed";
    can_unembed(s).map_err(|e| bad(ID, e))?;
    let aid = asset_param(p, ID)?;
    let st = s.doc()?;
    let document = st.path.clone();
    let document = document.as_deref().map(Path::new);
    let a = st.doc.assets.get(&aid).ok_or_else(|| bad(ID, "no such graphic"))?;
    if a.storage == Storage::Linked {
        return Err(bad(ID, format!("{} is already linked", a.name)));
    }
    let bytes = a
        .data
        .embedded_bytes()
        .cloned()
        .filter(|b| !b.is_empty())
        .ok_or_else(|| bad(ID, format!("{}: the document holds no bytes for it", a.name)))?;
    #[cfg(not(target_arch = "wasm32"))]
    {
        let original = a.link.as_ref().and_then(|link| {
            let source = LinkedSource {
                path: link.clone(),
                relative: a.link_relative.clone(),
                document: document.map(Path::to_path_buf),
                len: bytes.len() as u64,
                fingerprint: None,
                hash: None,
            };
            crate::asset_io::candidates(&source).into_iter().find(|p| file_holds(p, &bytes))
        });
        let target = match (original, str_param(p, "dir")) {
            (Some(original), _) => original,
            (None, Some(dir)) => {
                let dir = Path::new(dir);
                crate::file_access::check_write(dir).map_err(EngineError::Other)?;
                write_into(dir, &a.name, &a.mime, &bytes)?
            }
            (None, None) => return Err(bad(ID, format!("{}: its original file is missing or has changed; give `dir` to write it there", a.name))),
        };
        let before = super::datamerge::fingerprint_of(&target);
        let file = FileLink::of(&target, before);
        let key = a.data.key();
        let mut linked = (**a).clone();
        give_bytes(&mut linked, Arc::clone(&bytes), Some(file), false, document);
        // The same bytes: what was decoded from them stays usable.
        if let Some(source) = linked.data.linked_source().cloned() {
            linked.data = AssetData::linked_under(key, source);
            linked.data.keep_resident(bytes);
        }
        let path = linked.link.clone();
        let r = s.edit(|d, _| {
            d.assets.insert(aid, Arc::new(linked));
            Ok(json!({"path": path}))
        });
        s.link_checks.images.clear();
        designcraft_doc::asset_data::links_changed();
        r
    }
    #[cfg(target_arch = "wasm32")]
    {
        let _ = (document, bytes);
        Err(bad(ID, "files can't be linked on the web"))
    }
}

/// `links.update`: re-read the modified links (one, or every one) from where they resolve.
fn update(s: &mut Session, p: &Value) -> Result<Value> {
    let (d, document, open) = {
        let st = s.doc()?;
        (st.doc.clone(), st.path.clone(), st.links.clone())
    };
    let document = document.as_deref().map(Path::new);
    let LinkListChecks { text: checks, images, .. } = &mut s.link_checks;
    let (only_asset, only_story) = (p.get("asset").and_then(Value::as_u64), story_param(p));
    let all = only_asset.is_none() && only_story.is_none();
    // A link craft-docpath refuses is missing, so it is never read or refreshed.
    let resolved = |link: &str| crate::io::document_path(link, document).ok();
    let texts: Vec<(StoryId, String, std::path::PathBuf)> = d
        .stories
        .values()
        .filter(|st| all || only_story == Some(st.id))
        .filter_map(|st| {
            let l = st.file_link.as_ref().filter(|l| text_status(l, checks, document) == "modified")?;
            Some((st.id, l.path.clone(), resolved(&l.path)?))
        })
        .collect();
    let mut fresh = Vec::new();
    for a in
        d.assets.values().filter(|a| all || only_asset == Some(a.id.0)).filter(|a| image_status(a, document, images, Some(&open)).0 == "modified")
    {
        if let Some(at) = update_source(a, document) {
            fresh.push((a.id, relinked_asset(a, &at, document)?));
        }
    }
    drop(open);
    let mut fresh_texts = Vec::new();
    for (sid, path, at) in texts {
        let (bytes, before) = read_text(&at)?;
        fresh_texts.push((sid, path, bytes, before));
    }
    let n = fresh.len() + fresh_texts.len();
    if n == 0 {
        return Ok(json!({"updated": 0}));
    }
    let r = s.edit(|d, _| {
        for (aid, a) in fresh {
            d.assets.insert(aid, a);
        }
        let mut warnings = Vec::new();
        for (sid, path, bytes, before) in &fresh_texts {
            warnings.extend(reimport(d, *sid, path, bytes, *before)?);
        }
        Ok(json!({"updated": n, "warnings": warnings}))
    });
    s.link_checks.images.clear();
    designcraft_doc::asset_data::links_changed();
    r
}

#[cfg(test)]
mod tests;

#[cfg(test)]
mod relink_folder_tests {
    use std::sync::Arc;

    use serde_json::json;

    use crate::Session;

    fn two_missing_links(case: &str) -> (Session, std::path::PathBuf) {
        let dir = std::env::temp_dir().join(format!("dc-relink-{case}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join("new")).unwrap();
        let mut s = Session::new();
        s.execute("file.new", &json!({})).unwrap();
        for name in ["a.svg", "b.svg"] {
            let path = dir.join(name);
            std::fs::write(&path, r#"<svg xmlns="http://www.w3.org/2000/svg" width="10" height="10"/>"#).unwrap();
            s.doc_mut().unwrap().selection = Default::default();
            s.execute("file.place", &json!({"path": path.to_string_lossy()})).unwrap();
            crate::cmd::native_links::wait(&mut s);
            std::fs::rename(&path, dir.join("new").join(name)).unwrap();
        }
        (s, dir)
    }

    #[test]
    fn folder_relink_failure_keeps_document_and_history() {
        let (mut s, dir) = two_missing_links("atomic");
        std::fs::write(dir.join("new/b.svg"), b"not an image").unwrap();
        let st = s.doc().unwrap();
        let before = st.doc.clone();
        let revision = st.revision;
        let undo = st.history.undo.len();
        let journal = s.journal.len();
        let err = s.execute("links.relinkFolder", &json!({"dir": dir.join("new").to_string_lossy()})).unwrap_err();
        assert!(err.to_string().contains("unsupported or corrupt image"), "{err}");
        let st = s.doc().unwrap();
        assert!(Arc::ptr_eq(&st.doc, &before), "a failed batch must not leave its first link changed");
        assert_eq!(st.revision, revision);
        assert_eq!(st.history.undo.len(), undo);
        assert_eq!(s.journal.len(), journal);
        s.execute("frame.create", &json!({"rect": [0, 0, 20, 20]})).unwrap();
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn folder_relink_is_one_undo_step() {
        let (mut s, dir) = two_missing_links("undo");
        let st = s.doc().unwrap();
        let before = st.doc.clone();
        let revision = st.revision;
        let undo = st.history.undo.len();
        let r = s.execute("links.relinkFolder", &json!({"dir": dir.join("new").to_string_lossy()})).unwrap();
        assert_eq!(r["relinked"], 2);
        let st = s.doc().unwrap();
        // `revision` is drawn from a process-wide counter, so a test running beside this one
        // can take the next value: that the batch commits *once* is what the single undo step is.
        assert_ne!(st.revision, revision, "the batch commits");
        assert_eq!(st.history.undo.len(), undo + 1, "exactly one undo step");
        s.execute("edit.undo", &json!({})).unwrap();
        assert!(Arc::ptr_eq(&s.doc().unwrap().doc, &before));
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn relink_extension_rejects_path_separators_without_panicking() {
        let (mut s, dir) = two_missing_links("extension");
        for extension in ["png/other", "png\\other", "png\0other"] {
            // Call directly so the command guard cannot hide a panic in Path::with_extension.
            let err = super::relink_folder(&mut s, &json!({"extension": extension})).unwrap_err();
            assert!(matches!(err, crate::EngineError::BadParams { .. }), "{err}");
        }
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn relink_missing_links_to_a_folder_and_by_extension() {
        let dir = std::env::temp_dir().join(format!("dc-relink-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join("new")).unwrap();
        let svg =
            |c: &str| format!(r##"<svg xmlns="http://www.w3.org/2000/svg" width="10" height="10"><rect width="10" height="10" fill="{c}"/></svg>"##);
        let orig = dir.join("logo.svg");
        std::fs::write(&orig, svg("red")).unwrap();
        let mut s = Session::new();
        s.execute("file.new", &json!({})).unwrap();
        s.execute("file.place", &json!({"path": orig.to_string_lossy()})).unwrap();
        crate::cmd::native_links::wait(&mut s);
        // The file moves away: relink from the new folder.
        std::fs::rename(&orig, dir.join("new").join("logo.svg")).unwrap();
        let r = s.execute("links.relinkFolder", &json!({"dir": dir.join("new").to_string_lossy()})).unwrap();
        assert_eq!(r["relinked"], 1);
        let link = s.doc().unwrap().doc.assets.values().next().unwrap().link.clone().unwrap();
        assert!(link.contains("new"), "{link}");
        // Relink File Extension: the same name as .png doesn't exist → reported.
        let r = s.execute("links.relinkFolder", &json!({"extension": "png"})).unwrap();
        assert_eq!(r["relinked"], 0);
        assert_eq!(r["notFound"], json!(["logo.png"]));
        let _ = std::fs::remove_dir_all(&dir);
    }
}

#[cfg(test)]
mod shared_asset_tests {
    use super::*;

    fn png(w: u32, h: u32) -> Vec<u8> {
        designcraft_render::Rendered { width: w, height: h, pixels: vec![200u8; (w * h * 4) as usize] }.to_png()
    }

    /// Two placed graphics whose linked files were never read (an IDML opened in the browser
    /// without its Links folder): missing, with no picture.
    fn two_missing() -> (Session, AssetId, AssetId) {
        let mut s = Session::new();
        s.execute("file.new", &json!({})).unwrap();
        for (name, x) in [("a.png", 72), ("b.png", 200)] {
            let b = super::super::file::base64_encode(&png(4, 4 + x as u32 / 100));
            s.execute("file.place", &json!({"base64": b, "name": name, "x": x, "y": 72})).unwrap();
            s.execute("edit.deselectAll", &json!({})).unwrap();
        }
        let ids: Vec<AssetId> = s.doc().unwrap().doc.assets.keys().copied().collect();
        s.edit(|d, _| {
            for (k, id) in ids.iter().enumerate() {
                let a = Arc::make_mut(d.assets.get_mut(id).unwrap());
                a.link = Some(format!("/Users/someone/Links/Pic{k}.PNG"));
                a.name = format!("Pic{k}.PNG");
                a.data = AssetData::embedded(Vec::new());
            }
            Ok(())
        })
        .unwrap();
        (s, ids[0], ids[1])
    }

    /// Two missing graphics linked to `links`, as [`two_missing`] but with the given links.
    fn missing_with(links: [&str; 2]) -> (Session, AssetId, AssetId) {
        let (mut s, a, b) = two_missing();
        s.edit(|d, _| {
            for (id, link) in [(a, links[0]), (b, links[1])] {
                let x = Arc::make_mut(d.assets.get_mut(&id).unwrap());
                x.link = Some(link.into());
                x.name = link_file_name(link).into();
            }
            Ok(())
        })
        .unwrap();
        (s, a, b)
    }

    /// Only graphics still placed are relinked: a replaced graphic kept for undo is missing too,
    /// but relinking it would change nothing on the page. The journal keeps the files' names,
    /// not their contents.
    #[test]
    fn relinks_only_placed_graphics_and_replays_from_the_journal() {
        let (mut s, a, b) = two_missing();
        // b's frame is deleted: its asset stays (for undo) but nothing shows it.
        s.edit(|d, _| {
            for sp in &mut d.spreads {
                std::sync::Arc::make_mut(sp).items.retain(|it| !matches!(&it.content, Content::Graphic(g) if g.asset == b));
            }
            Ok(())
        })
        .unwrap();
        let file = |name: &str| json!({"name": name, "base64": super::super::file::base64_encode(&png(6, 6))});
        let r = s.execute("links.relinkFiles", &json!({"files": [file("pic0.png"), file("pic1.png")]})).unwrap();
        assert_eq!(r["relinked"], 1);
        assert_eq!(r["unused"], json!(["pic1.png"]), "nothing placed is named pic1");
        let d = &s.doc().unwrap().doc;
        assert!(!d.assets[&a].data.is_empty() && d.assets[&b].data.is_empty());
        // The journal keeps the files whole, as it does for `file.place {base64}`: replayed, the
        // entry relinks again.
        let (id, params) = s.journal.last().unwrap().clone();
        assert_eq!(id, "links.relinkFiles");
        s.execute("edit.undo", &json!({})).unwrap();
        assert!(s.doc().unwrap().doc.assets[&a].data.is_empty());
        assert_eq!(s.execute(&id, &params).unwrap()["relinked"], 1);
        assert!(!s.doc().unwrap().doc.assets[&a].data.is_empty());
    }

    /// A matching file that can't be read (here not an image at all) is reported and leaves its
    /// graphic missing; the other files still relink theirs.
    #[test]
    fn an_unreadable_file_leaves_the_others_relinking() {
        let (mut s, a, b) = two_missing();
        let good = json!({"name": "pic0.png", "base64": super::super::file::base64_encode(&png(6, 6))});
        let bad_file = json!({"name": "pic1.png", "base64": super::super::file::base64_encode(b"not an image")});
        let r = s.execute("links.relinkFiles", &json!({"files": [good, bad_file]})).unwrap();
        assert_eq!(r["relinked"], 1, "{r}");
        assert_eq!(r["failed"].as_array().map(|f| f.iter().map(|f| f["name"].clone()).collect::<Vec<_>>()), Some(vec![json!("pic1.png")]), "{r}");
        let d = &s.doc().unwrap().doc;
        assert!(!d.assets[&a].data.is_empty() && d.assets[&b].data.is_empty());
    }

    /// Relinking to a file on disk names the graphic after that file as the platform reads
    /// paths: a backslash is part of a file name on Linux and macOS.
    #[cfg(unix)]
    #[test]
    fn a_path_relink_names_the_graphic_by_the_platform_s_file_name() {
        let (mut s, a, _) = two_missing();
        let dir = std::env::temp_dir().join(format!("dc-relink-name-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("cover\\v2.png");
        std::fs::write(&path, png(4, 4)).unwrap();
        s.execute("links.relink", &json!({"asset": a.0, "path": path.to_string_lossy()})).unwrap();
        assert_eq!(s.doc().unwrap().doc.assets[&a].name, "cover\\v2.png");
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// Files of the same name in different folders of a chosen folder: each missing graphic takes
    /// the one whose path ends like its link; where nothing tells them apart, the graphics stay
    /// missing and are reported rather than given another graphic's picture. Graphics linking to
    /// the same file share it.
    #[test]
    fn same_named_files_relink_by_folder_or_not_at_all() {
        let file = |name: &str, path: &str, w: u32| json!({"name": name, "path": path, "base64": super::super::file::base64_encode(&png(w, w))});
        let px = |s: &Session, id: AssetId| s.doc().unwrap().doc.assets[&id].pixels;
        let links = ["/Users/x/Vol38/Links/brand-a/logo.png", "/Users/x/Vol38/Links/brand-b/logo.png"];
        // Their folders tell them apart.
        let (mut s, a, b) = missing_with(links);
        let r = s
            .execute(
                "links.relinkFiles",
                &json!({"files": [file("logo.png", "Links/brand-b/logo.png", 5), file("logo.png", "Links/brand-a/logo.png", 6)]}),
            )
            .unwrap();
        assert_eq!(r["relinked"], 2);
        assert_eq!((px(&s, a), px(&s, b)), (Some((6, 6)), Some((5, 5))));
        // Two files named alike with no folders: neither graphic can tell which is its own.
        let (mut s, a, b) = missing_with(links);
        let r = s.execute("links.relinkFiles", &json!({"files": [file("logo.png", "logo.png", 5), file("logo.png", "logo.png", 6)]})).unwrap();
        assert_eq!(r["relinked"], 0);
        assert_eq!(r["ambiguous"], json!(["logo.png", "logo.png"]));
        assert_eq!(r["unused"], json!([]), "files named like the graphics were meant for them");
        assert!(s.doc().unwrap().doc.assets[&a].data.is_empty() && s.doc().unwrap().doc.assets[&b].data.is_empty());
        // One file named like both, which link to different files: it can't be both.
        let (mut s, a, _) = missing_with(links);
        let r = s.execute("links.relinkFiles", &json!({"files": [file("logo.png", "logo.png", 5)]})).unwrap();
        assert_eq!((r["relinked"].as_u64(), r["ambiguous"].as_array().map(Vec::len)), (Some(0), Some(2)));
        assert!(s.doc().unwrap().doc.assets[&a].data.is_empty());
        // ...but with its folder it is brand-b's, and brand-a's stays missing.
        let r = s.execute("links.relinkFiles", &json!({"files": [file("logo.png", "brand-b/logo.png", 5)]})).unwrap();
        assert_eq!(r["relinked"], 1);
        assert_eq!((r["notFound"].clone(), r["ambiguous"].clone()), (json!(["logo.png"]), json!([])), "brand-b's file isn't brand-a's");
        // Two graphics linking to the same file (another page of it, say) share it.
        let (mut s, a, b) = missing_with(["/Users/x/Links/logo.png", "/Users/x/Links/logo.png"]);
        let r = s.execute("links.relinkFiles", &json!({"files": [file("logo.png", "logo.png", 5)]})).unwrap();
        assert_eq!(r["relinked"], 2);
        assert_eq!((px(&s, a), px(&s, b)), (Some((5, 5)), Some((5, 5))));
    }

    /// Relinking from files handed over by name (the web build): each missing graphic takes the
    /// file of its name in any case, keeping its link's path, in one undo step; a file that
    /// matches nothing and a graphic no file matched are reported.
    #[test]
    fn relinks_missing_graphics_from_files_by_name() {
        let (mut s, a, b) = two_missing();
        let st = s.doc().unwrap();
        assert!(st.doc.assets.values().all(|x| status_cached(x, None, &mut ImageChecks::new()) == "missing"));
        let file = |name: &str, w: u32| json!({"name": name, "base64": super::super::file::base64_encode(&png(w, w))});
        let r = s.execute("links.relinkFiles", &json!({"files": [file("pic0.png", 6), file("other.png", 3)]})).unwrap();
        assert_eq!(r["relinked"], 1);
        assert_eq!(r["notFound"], json!(["Pic1.PNG"]));
        assert_eq!(r["unused"], json!(["other.png"]));
        let d = &s.doc().unwrap().doc;
        assert_eq!(d.assets[&a].pixels, Some((6, 6)));
        assert_eq!(d.assets[&a].link.as_deref(), Some("/Users/someone/Links/Pic0.PNG"), "the same file keeps its path");
        assert!(d.assets[&b].data.is_empty());
        s.execute("edit.undo", &json!({})).unwrap();
        assert!(s.doc().unwrap().doc.assets[&a].data.is_empty(), "one undo step");
        // One graphic from a chosen file of another name: that name becomes the link.
        s.execute("links.relinkFiles", &json!({"asset": b.0, "files": [file("new.png", 5)]})).unwrap();
        let d = &s.doc().unwrap().doc;
        assert_eq!((d.assets[&b].pixels, d.assets[&b].link.as_deref(), d.assets[&b].name.as_str()), (Some((5, 5)), Some("new.png"), "new.png"));
        // A file that isn't an image changes nothing: reported for a folder, an error for one
        // chosen file.
        let before = s.doc().unwrap().doc.assets.clone();
        let bad = json!({"name": "pic0.png", "base64": super::super::file::base64_encode(b"not an image")});
        let r = s.execute("links.relinkFiles", &json!({"files": [bad.clone()]})).unwrap();
        assert_eq!((r["relinked"].clone(), r["failed"][0]["name"].clone()), (json!(0), json!("pic0.png")), "{r}");
        assert!(s.execute("links.relinkFiles", &json!({"asset": a.0, "files": [bad]})).is_err());
        assert_eq!(s.doc().unwrap().doc.assets, before);
        assert!(s.execute("links.relinkFiles", &json!({"files": "x"})).is_err());
    }

    #[test]
    fn placements_of_one_file_share_an_asset_and_saves_drop_replaced_ones() {
        let mut s = Session::new();
        s.execute("file.new", &json!({})).unwrap();
        let a = super::super::file::base64_encode(&png(10, 10));
        s.execute("file.place", &json!({"base64": a, "name": "a.png", "x": 72, "y": 72})).unwrap();
        s.execute("edit.deselectAll", &json!({})).unwrap();
        s.execute("file.place", &json!({"base64": a, "name": "a.png", "x": 200, "y": 72})).unwrap();
        let l = s.execute("links.list", &json!({})).unwrap();
        assert_eq!(l.as_array().unwrap().len(), 1);
        assert_eq!(l[0]["uses"].as_array().unwrap().len(), 2);
        assert_eq!(s.doc().unwrap().doc.assets.len(), 1);

        // A new image into a frame: the old asset stays in memory (undo) but not in the file.
        let f = s.execute("frame.create", &json!({"rect": [100, 300, 200, 400]})).unwrap()["id"].as_u64().unwrap();
        let b = super::super::file::base64_encode(&png(20, 10));
        let c = super::super::file::base64_encode(&png(30, 10));
        s.execute("file.place", &json!({"base64": b, "name": "b.png", "frame": f})).unwrap();
        s.execute("file.place", &json!({"base64": c, "name": "c.png", "frame": f})).unwrap();
        let d = &s.doc().unwrap().doc;
        assert_eq!(d.assets.len(), 3);
        assert_eq!(s.execute("links.list", &json!({})).unwrap().as_array().unwrap().len(), 2);
        let back = designcraft_format::load(&designcraft_format::save(&s.doc().unwrap().doc).unwrap()).unwrap();
        let mut names: Vec<&str> = back.assets.values().map(|a| a.name.as_str()).collect();
        names.sort();
        assert_eq!(names, ["a.png", "c.png"]);
    }

    #[test]
    fn files_with_identical_bytes_at_different_paths_keep_their_own_links() {
        let dir = std::env::temp_dir().join(format!("dc-same-bytes-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let (a, b) = (dir.join("a.png"), dir.join("b.png"));
        std::fs::write(&a, png(10, 10)).unwrap();
        std::fs::write(&b, png(10, 10)).unwrap();
        let mut s = Session::new();
        s.execute("file.new", &json!({})).unwrap();
        for (path, x) in [(&a, 72), (&b, 200)] {
            s.execute("edit.deselectAll", &json!({})).unwrap();
            s.execute("file.place", &json!({"path": path.to_string_lossy(), "x": x, "y": 72})).unwrap();
            crate::cmd::native_links::wait(&mut s);
        }
        assert_eq!(s.doc().unwrap().doc.assets.len(), 2);
        let rows = |s: &mut Session| -> Vec<(String, Value)> {
            let l = s.execute("links.list", &json!({})).unwrap();
            l.as_array().unwrap().iter().map(|r| (r["path"].as_str().unwrap().to_string(), r["pixels"].clone())).collect()
        };
        let paths = |r: &[(String, Value)]| r.iter().map(|(p, _)| p.clone()).collect::<Vec<_>>();
        let before = rows(&mut s);
        assert_eq!(paths(&before), [a.to_string_lossy(), b.to_string_lossy()]);

        // b.png changes on disk: updating it leaves a.png's placement alone.
        std::fs::write(&b, png(20, 10)).unwrap();
        assert_eq!(s.execute("links.update", &json!({})).unwrap()["updated"], 1);
        let after = rows(&mut s);
        assert_eq!(paths(&after), paths(&before));
        assert_eq!((after[0].1.clone(), after[1].1.clone()), (json!([10, 10]), json!([20, 10])));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn pasted_graphics_reuse_the_identical_asset_of_the_target() {
        use designcraft_doc::build::NewDocument;
        use designcraft_doc::{Asset, Graphic, Item, ItemId, Shape, SpreadRef};
        let asset = |id: u64, data: Vec<u8>| Asset {
            id: AssetId(id),
            name: "p.png".into(),
            mime: "image/png".into(),
            data: designcraft_doc::AssetData::embedded(data),
            ..Default::default()
        };
        let mut src = Document::new(&NewDocument::default());
        let mut dst = Document::new(&NewDocument::default());
        let (pa, pb) = (png(4, 4), png(8, 4));
        // The source's asset id is taken in the target by a different file.
        let sid = AssetId(src.alloc());
        src.assets.insert(sid, Arc::new(asset(sid.0, pa.clone())));
        dst.assets.insert(sid, Arc::new(asset(sid.0, pb)));
        dst.alloc();
        let same = AssetId(dst.alloc());
        assert_ne!(same, sid);
        dst.assets.insert(same, Arc::new(asset(same.0, pa)));
        let iid = ItemId(src.alloc());
        let mut it = Item::new(
            iid,
            src.default_layer(),
            Shape::Rectangle,
            designcraft_geom::shapes::rectangle(designcraft_geom::Rect::new(0.0, 0.0, 9.0, 9.0)),
        );
        it.content = Content::Graphic(Graphic {
            layer_options: None,
            asset: sid,
            size: (4.0, 4.0),
            xf: designcraft_geom::Affine::IDENTITY,
            clip: None,
            auto_fit: Default::default(),
            fit_align: 4,
            crop: [1.0; 4],
            wrap: Default::default(),
            fill: Default::default(),
        });
        src.insert_item(SpreadRef::Doc(0), it, None).unwrap();
        let ids = super::super::object::duplicate_from(&mut dst, &src, &[iid], SpreadRef::Doc(0), designcraft_geom::Vec2::ZERO).unwrap();
        let g = dst.item(ids[0]).and_then(|i| i.graphic()).unwrap();
        assert_eq!((g.asset, g.crop), (same, [1.0; 4]));
        assert_eq!(dst.assets.len(), 2);
    }

    #[test]
    fn pasted_missing_graphic_does_not_show_an_unrelated_target_asset() {
        use designcraft_doc::build::NewDocument;
        use designcraft_doc::{Asset, Graphic, Item, ItemId, Shape, SpreadRef};
        let mut src = Document::new(&NewDocument::default());
        let mut dst = Document::new(&NewDocument::default());
        // The source graphic points at an asset the source does not have; the target uses that id for another image.
        let missing = AssetId(src.alloc());
        dst.assets.insert(
            missing,
            Arc::new(Asset {
                id: missing,
                name: "p.png".into(),
                mime: "image/png".into(),
                link: None,
                data: designcraft_doc::AssetData::embedded(png(8, 4)),
                pixels: None,
                ..Default::default()
            }),
        );
        let iid = ItemId(src.alloc());
        let mut it = Item::new(
            iid,
            src.default_layer(),
            Shape::Rectangle,
            designcraft_geom::shapes::rectangle(designcraft_geom::Rect::new(0.0, 0.0, 9.0, 9.0)),
        );
        it.content = Content::Graphic(Graphic {
            layer_options: None,
            asset: missing,
            size: (4.0, 4.0),
            xf: designcraft_geom::Affine::IDENTITY,
            clip: None,
            auto_fit: Default::default(),
            fit_align: 4,
            crop: [1.0; 4],
            wrap: Default::default(),
            fill: Default::default(),
        });
        src.insert_item(SpreadRef::Doc(0), it, None).unwrap();
        let ids = super::super::object::duplicate_from(&mut dst, &src, &[iid], SpreadRef::Doc(0), designcraft_geom::Vec2::ZERO).unwrap();
        let g = dst.item(ids[0]).and_then(|i| i.graphic()).unwrap();
        assert_ne!(g.asset, missing);
        assert!(!dst.assets.contains_key(&g.asset));
    }

    #[test]
    fn pasted_missing_graphic_never_takes_an_asset_placed_later() {
        use designcraft_doc::build::NewDocument;
        use designcraft_doc::{Graphic, Item, ItemId, Shape, SpreadRef};
        let mut src = Document::new(&NewDocument::default());
        let mut dst = Document::new(&NewDocument::default());
        // The source's missing id is ahead of the target's counter: the target would allocate it to its next image.
        for _ in 0..50 {
            src.alloc();
        }
        let missing = AssetId(src.alloc());
        let iid = ItemId(src.alloc());
        let mut it = Item::new(
            iid,
            src.default_layer(),
            Shape::Rectangle,
            designcraft_geom::shapes::rectangle(designcraft_geom::Rect::new(0.0, 0.0, 9.0, 9.0)),
        );
        it.content = Content::Graphic(Graphic {
            layer_options: None,
            asset: missing,
            size: (4.0, 4.0),
            xf: designcraft_geom::Affine::IDENTITY,
            clip: None,
            auto_fit: Default::default(),
            fit_align: 4,
            crop: [1.0; 4],
            wrap: Default::default(),
            fill: Default::default(),
        });
        src.insert_item(SpreadRef::Doc(0), it, None).unwrap();
        let ids = super::super::object::duplicate_from(&mut dst, &src, &[iid], SpreadRef::Doc(0), designcraft_geom::Vec2::ZERO).unwrap();
        let g = dst.item(ids[0]).and_then(|i| i.graphic()).unwrap().asset;
        assert!((0..100).all(|_| AssetId(dst.alloc()) != g), "a later image in the target takes the pasted graphic's id");
    }
}

#[cfg(test)]
mod text_link_tests {
    use serde_json::{Value, json};

    use crate::Session;
    use designcraft_doc::{ItemId, StoryId};

    /// A small Word file: a Title paragraph, then Body paragraphs (`body` is WordprocessingML).
    fn docx(body: &str) -> Vec<u8> {
        const W: &str = r#"xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main""#;
        let styles = format!(
            r#"<w:styles {W}><w:style w:type="paragraph" w:styleId="Title"><w:name w:val="Title"/><w:rPr><w:sz w:val="56"/></w:rPr></w:style>
            <w:style w:type="paragraph" w:styleId="Body"><w:name w:val="Body"/><w:rPr><w:sz w:val="20"/></w:rPr></w:style></w:styles>"#
        );
        let doc = format!(
            r#"<w:document {W}><w:body><w:p><w:pPr><w:pStyle w:val="Title"/></w:pPr><w:r><w:t>Head</w:t></w:r></w:p>
            <w:p><w:pPr><w:pStyle w:val="Body"/></w:pPr>{body}</w:p></w:body></w:document>"#
        );
        crate::cmd::place_text::word_package(&doc, &styles)
    }

    fn text_rows(s: &mut Session) -> Vec<Value> {
        let l = s.execute("links.list", &json!({})).unwrap();
        l.as_array().unwrap().iter().filter(|r| r.get("story").is_some()).cloned().collect()
    }

    fn story(s: &Session, sid: StoryId) -> designcraft_doc::Story {
        s.doc().unwrap().doc.story(sid).unwrap().clone()
    }

    fn styles(st: &designcraft_doc::Story) -> Vec<String> {
        st.paras.iter().map(|p| p.style.clone()).collect()
    }

    fn assert_link_update_replaces_and_warns(s: &mut Session, path: &std::path::Path, sid: StoryId, a: u64, b: u64) {
        // The file changes (with tracked changes): Modified; Update imports it as if accepted, with
        // the saved style map, into the same frames, without new renamed styles.
        std::fs::write(
            path,
            docx(r#"<w:r><w:t xml:space="preserve">Second </w:t></w:r><w:ins w:id="1" w:author="A"><w:r><w:t>revised </w:t></w:r></w:ins><w:del w:id="2" w:author="A"><w:r><w:delText>old </w:delText></w:r></w:del><w:r><w:t>draft.</w:t></w:r>"#),
        )
        .unwrap();
        assert_eq!(text_rows(s)[0]["status"], "modified");
        let r = s.execute("links.update", &json!({"story": sid.0})).unwrap();
        assert_eq!(r["updated"], 1);
        assert_eq!(r["warnings"], json!([]), "no edits in the layout to replace");
        let updated = story(s, sid);
        assert_eq!(updated.text, "Head\nSecond revised draft.");
        assert_eq!(styles(&updated), ["Headline", "Body_wrd_1"]);
        assert_eq!(updated.frames, [ItemId(a), ItemId(b)], "frames and thread kept");
        assert!(s.doc().unwrap().doc.styles.para("Body_wrd_2").is_none());
        assert!(s.doc().unwrap().doc.styles.para("Title").is_none(), "mapped styles aren't imported");
        assert_eq!(text_rows(s)[0]["status"], "ok");

        // An edit in the layout, then the file changes again: Update replaces the edit and warns.
        s.execute("story.setText", &json!({"story": sid.0, "text": "Fixed in layout"})).unwrap();
        assert_eq!(text_rows(s)[0]["edited"], true);
        std::fs::write(path, docx("<w:r><w:t>Third draft.</w:t></w:r>")).unwrap();
        let r = s.execute("links.update", &json!({})).unwrap();
        assert_eq!(r["updated"], 1);
        let w = r["warnings"][0].as_str().unwrap();
        assert!(w.contains("chapter.docx") && w.contains("replaced"), "{w}");
        assert_eq!(story(s, sid).text, "Head\nThird draft.");
        assert_eq!(text_rows(s)[0]["edited"], false);
        // Undo brings the edit back.
        s.execute("edit.undo", &json!({})).unwrap();
        assert_eq!(story(s, sid).text, "Fixed in layout");
        s.execute("edit.redo", &json!({})).unwrap();

        // Saved and opened again: the link and its options are kept.
        let back = designcraft_format::load(&designcraft_format::save(&s.doc().unwrap().doc).unwrap()).unwrap();
        assert_eq!(back.story(sid).unwrap().file_link, story(s, sid).file_link);
        assert_eq!(super::text_status(back.story(sid).unwrap().file_link.as_ref().unwrap(), &mut Default::default(), None), "ok");
    }

    #[test]
    fn linked_word_file_status_update_relink_and_unlink() {
        let dir = std::env::temp_dir().join(format!("dc-text-link-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join("moved")).unwrap();
        let path = dir.join("chapter.docx");
        std::fs::write(&path, docx("<w:r><w:t>First draft.</w:t></w:r>")).unwrap();
        let mut s = Session::new();
        s.execute("file.new", &json!({"pages": 2})).unwrap();
        s.execute("style.paragraph.create", &json!({"name": "Body", "chars": {"size": 9}})).unwrap();
        s.execute("style.paragraph.create", &json!({"name": "Headline", "chars": {"size": 30}})).unwrap();
        // Two threaded frames.
        let a = s.execute("frame.create", &json!({"rect": [72, 72, 300, 200], "content": "text"})).unwrap()["id"].as_u64().unwrap();
        let b = s.execute("frame.create", &json!({"rect": [72, 72, 300, 200], "spread": 1, "content": "text"})).unwrap()["id"].as_u64().unwrap();
        s.edit(|d, _| Ok(d.thread(ItemId(a), ItemId(b))?)).unwrap();
        s.execute("edit.deselectAll", &json!({})).unwrap();

        // Place and link, with a style map and Auto Rename.
        let r = s
            .execute(
                "file.place",
                &json!({"path": path.to_string_lossy(), "frame": a, "link": true, "styleMap": {"Title": "Headline"}, "styleConflicts": "autoRename"}),
            )
            .unwrap();
        let sid = StoryId(r["story"].as_u64().unwrap());
        let placed = story(&s, sid);
        assert_eq!(placed.text, "Head\nFirst draft.");
        assert_eq!(styles(&placed), ["Headline", "Body_wrd_1"]);
        let link = placed.file_link.clone().unwrap();
        assert_eq!(link.path, path.to_string_lossy());
        assert_eq!(link.style_conflicts, "autoRename");
        assert_eq!(link.style_map.get("Title").map(String::as_str), Some("Headline"));
        assert_eq!(link.style_map.get("Body").map(String::as_str), Some("Body_wrd_1"), "Auto Rename's choice is kept");
        let rows = text_rows(&mut s);
        assert_eq!(rows.len(), 1);
        assert_eq!((rows[0]["story"].as_u64(), rows[0]["name"].as_str()), (Some(sid.0), Some("chapter.docx")));
        assert_eq!((rows[0]["status"].as_str(), rows[0]["edited"].as_bool()), (Some("ok"), Some(false)));
        assert_eq!(rows[0]["uses"].as_array().unwrap().len(), 2, "both threaded frames");

        assert_link_update_replaces_and_warns(&mut s, &path, sid, a, b);

        // The file moves: Missing; Relink to it imports it into the story.
        let moved = dir.join("moved").join("chapter.docx");
        std::fs::write(&moved, docx("<w:r><w:t>Moved draft.</w:t></w:r>")).unwrap();
        std::fs::remove_file(&path).unwrap();
        assert_eq!(text_rows(&mut s)[0]["status"], "missing");
        s.execute("links.relink", &json!({"story": sid.0, "path": moved.to_string_lossy()})).unwrap();
        let rows = text_rows(&mut s);
        assert_eq!((rows[0]["status"].as_str(), rows[0]["path"].as_str()), (Some("ok"), Some(moved.to_string_lossy().as_ref())));
        assert_eq!(story(&s, sid).text, "Head\nMoved draft.");

        // Go To selects the story's frames; Unlink keeps the text and drops the file.
        let r = s.execute("links.goTo", &json!({"story": sid.0})).unwrap();
        assert_eq!(r["ids"], json!([a, b]));
        s.execute("links.embed", &json!({"story": sid.0})).unwrap();
        assert!(text_rows(&mut s).is_empty());
        assert_eq!(story(&s, sid).text, "Head\nMoved draft.");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[cfg(unix)]
    #[test]
    fn unchanged_linked_text_files_are_not_read_again() {
        use std::os::unix::fs::PermissionsExt;
        let dir = std::env::temp_dir().join(format!("dc-text-link-poll-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("note.txt");
        std::fs::write(&path, "Hello").unwrap();
        // Unreadable: a status that reads the file would say missing.
        let readable = |on: bool| std::fs::set_permissions(&path, std::fs::Permissions::from_mode(if on { 0o644 } else { 0o000 })).unwrap();
        // A time `secs` ago (a file changed in the last two seconds is always read).
        let touch = |secs: u64| {
            let f = std::fs::File::options().write(true).open(&path).unwrap();
            f.set_modified(std::time::SystemTime::now() - std::time::Duration::from_secs(secs)).unwrap();
        };
        touch(60);
        let mut s = Session::new();
        s.execute("file.new", &json!({})).unwrap();
        s.execute("file.place", &json!({"path": path.to_string_lossy(), "link": true})).unwrap();
        crate::cmd::native_links::wait(&mut s);
        let status = |s: &mut Session| text_rows(s)[0]["status"].as_str().unwrap().to_string();

        // Size and time as placed: not read.
        readable(false);
        if std::fs::read(&path).is_ok() {
            // Running as root: permissions don't stop reads, so this can't be observed.
            return;
        }
        assert_eq!(status(&mut s), "ok");
        // Saved again unchanged (a new time): read once, then not again.
        readable(true);
        touch(30);
        assert_eq!(status(&mut s), "ok");
        readable(false);
        assert_eq!(status(&mut s), "ok", "the second poll doesn't read it");
        // Changed, same size, new time: modified.
        readable(true);
        std::fs::write(&path, "Jello").unwrap();
        touch(20);
        assert_eq!(status(&mut s), "modified");
        // Changed again in the same second, same size: still seen (just-changed files are read).
        std::fs::write(&path, "Hello").unwrap();
        assert_eq!(status(&mut s), "ok");
        std::fs::write(&path, "Jello").unwrap();
        assert_eq!(status(&mut s), "modified");
        touch(10);
        // Updated: the new size and time are kept, so the next poll doesn't read it.
        s.execute("links.update", &json!({})).unwrap();
        readable(false);
        assert_eq!(status(&mut s), "ok");
        // A time in the future (clock skew): read once, then not on every poll.
        readable(true);
        let f = std::fs::File::options().write(true).open(&path).unwrap();
        f.set_modified(std::time::SystemTime::now() + std::time::Duration::from_secs(3600)).unwrap();
        assert_eq!(status(&mut s), "ok");
        readable(false);
        assert_eq!(status(&mut s), "ok", "a future time is remembered");
        readable(true);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn placed_plain_text_drops_story_markers() {
        let mut s = Session::new();
        s.execute("file.new", &json!({})).unwrap();
        let b64 = crate::cmd::base64_encode("a\u{E009}b\u{E000}c\u{E002}d".as_bytes());
        let r = s.execute("file.place", &json!({"base64": b64, "name": "n.txt", "rect": [72, 72, 300, 200]})).unwrap();
        let st = s.doc().unwrap().doc.story(StoryId(r["story"].as_u64().unwrap())).unwrap();
        assert_eq!(st.text, "abcd");
    }

    #[test]
    fn create_links_when_placing_text_files_preference() {
        let dir = std::env::temp_dir().join(format!("dc-text-link-pref-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("notes.txt");
        std::fs::write(&path, "Plain text").unwrap();
        let place = |s: &mut Session, extra: Value| {
            let mut p = json!({"path": path.to_string_lossy(), "rect": [72, 72, 300, 200]});
            p.as_object_mut().unwrap().extend(extra.as_object().unwrap().clone());
            s.execute("edit.deselectAll", &json!({})).unwrap();
            let r = s.execute("file.place", &p).unwrap();
            (s.doc().unwrap().doc.story(StoryId(r["story"].as_u64().unwrap())).unwrap().file_link.is_some(), r)
        };
        let mut s = Session::new();
        s.execute("file.new", &json!({})).unwrap();
        assert!(!place(&mut s, json!({})).0, "off by default");
        assert!(place(&mut s, json!({"link": true})).0);
        s.execute("prefs.set", &json!({"linkTextFiles": true})).unwrap();
        assert!(place(&mut s, json!({})).0, "the preference links");
        assert!(!place(&mut s, json!({"link": false})).0, "`link` overrides it");
        // Placed from bytes: no file to link.
        let b64 = crate::cmd::base64_encode(b"Plain text");
        let r = s.execute("file.place", &json!({"base64": b64, "name": "notes.txt", "rect": [72, 300, 300, 400]})).unwrap();
        assert!(s.doc().unwrap().doc.story(StoryId(r["story"].as_u64().unwrap())).unwrap().file_link.is_none());
        // Into an insertion point: not linked, with a warning.
        let fr = s.execute("frame.create", &json!({"rect": [72, 500, 300, 600], "content": "text", "text": "Start  end"})).unwrap();
        let sid = fr["story"].as_u64().unwrap();
        s.execute("text.select", &json!({"story": sid, "anchor": 6, "focus": 6})).unwrap();
        let r = s.execute("file.place", &json!({"path": path.to_string_lossy()})).unwrap();
        crate::cmd::native_links::wait(&mut s);
        assert!(r["warnings"][0].as_str().unwrap().contains("not linked"), "{r}");
        assert!(s.doc().unwrap().doc.story(StoryId(sid)).unwrap().file_link.is_none());
        let _ = std::fs::remove_dir_all(&dir);
    }
}

#[cfg(unix)]
#[cfg(test)]
mod text_link_read_tests {
    use std::time::Duration;

    use designcraft_doc::TextFileLink;

    use super::{MAX_TEXT_LINK_BYTES, read_text, text_status};

    /// `f` on another thread: a call that blocks fails the test instead of hanging it.
    fn promptly<T: Send + 'static>(f: impl FnOnce() -> T + Send + 'static) -> T {
        let (tx, rx) = std::sync::mpsc::channel();
        std::thread::spawn(move || {
            let _ = tx.send(f());
        });
        // A timeout means the call blocked.
        rx.recv_timeout(Duration::from_secs(10)).unwrap()
    }

    fn status_of(path: &std::path::Path, size: u64) -> &'static str {
        let l = TextFileLink { path: path.to_string_lossy().to_string(), size, hash: "0".into(), ..Default::default() };
        promptly(move || text_status(&l, &mut Default::default(), None))
    }

    fn read(path: &std::path::Path) -> Result<usize, String> {
        let p = path.to_string_lossy().to_string();
        promptly(move || read_text(&p).map(|(b, _)| b.len()).map_err(|e| e.to_string()))
    }

    #[test]
    fn links_to_fifos_devices_folders_and_huge_files_are_not_read() {
        let dir = std::env::temp_dir().join(format!("dc-text-link-read-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        // A FIFO with no writer: reading it would block for ever.
        let fifo = dir.join("pipe.docx");
        assert!(std::process::Command::new("mkfifo").arg(&fifo).status().unwrap().success());
        assert_eq!(status_of(&fifo, 0), "missing");
        assert!(read(&fifo).is_err());
        // A device that never ends, recorded with size 0 (as its metadata reports).
        let zero = std::path::Path::new("/dev/zero");
        assert_eq!(status_of(zero, 0), "missing");
        assert!(read(zero).is_err());
        // A folder.
        assert_eq!(status_of(&dir, 0), "missing");
        assert!(read(&dir).is_err());
        // Over the limit (sparse, so cheap): not read; the same size is reported as too large
        // to check rather than OK, another size as modified.
        let big = dir.join("big.txt");
        std::fs::File::create(&big).unwrap().set_len(MAX_TEXT_LINK_BYTES + 1).unwrap();
        assert_eq!(status_of(&big, MAX_TEXT_LINK_BYTES + 1), "tooLarge");
        assert_eq!(status_of(&big, 10), "modified");
        assert!(read(&big).unwrap_err().contains("larger than"));
        assert!(!super::checkable_size(MAX_TEXT_LINK_BYTES as usize + 1), "placing it doesn't link it");
        let _ = std::fs::remove_dir_all(&dir);
    }
}
