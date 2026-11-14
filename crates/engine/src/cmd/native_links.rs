//! Linked assets in native documents ([`Storage::Linked`]): the paths a save or a recovery
//! entry records for them, which file an open finds each one in, the background check that a
//! file still holds the recorded bytes, and the move to links of documents saved while every
//! placed file was embedded.
//!
//! No original is read into memory on open, and none is written into a document or a recovery
//! entry; Save a Copy `{embedLinks: true}` is the one save that copies them in.
//!
//! A linked file is looked for at its absolute path, then at its path relative to the document
//! ([`crate::asset_io::candidates`]). A file with the recorded size and modification time holds
//! the bytes. One with the recorded size and another time is hashed on a background thread: the
//! first whose hash is the recorded one is the file, and its time is kept with the asset; when
//! none matches the link is Modified and the file's bytes are never used.
//!
//! Graphics placed from files link to them at once. The same thread makes their content hash
//! and proxy ([`prepare`]), and hashes linked files that an open found by size and time and that
//! have no hash yet. A save makes the proxies still missing ([`with_proxies`]), never a hash.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Condvar, Mutex, PoisonError};

use designcraft_doc::{Asset, AssetData, AssetId, Document, Fingerprint, LinkedSource, SourceKey, Storage};
use serde_json::{Value, json};

use super::{CommandSpec, always, bad, cmd};
use crate::{DocState, Result, Session};

pub fn specs() -> Vec<CommandSpec> {
    vec![cmd!(noundo "links.progress", "Link Checks", [], None,
    "{} — apply what the background checks of opened documents' linked files found (the UI calls this while `checking`) → {checking: the active document's files are still being checked, remaining: how many of them, notice?: a message for the status line, given once}",
    always, |s, _| {
        s.poll_link_checks();
        let Ok(st) = s.doc_mut() else { return Ok(json!({"checking": false, "remaining": 0})) };
        let mut out = json!({"checking": st.links.checking(), "remaining": st.links.remaining()});
        if let Some(notice) = st.links.take_notice() {
            out["notice"] = Value::String(notice);
        }
        Ok(out)
    })]
}

/// What the open of a document found for a linked file.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LinkStatus {
    /// A file holds the recorded bytes.
    Ok,
    /// The background check is looking for the file, and hashes one of the recorded size whose
    /// time is new.
    Checking,
    /// Files are there, none with the recorded bytes.
    Modified,
    /// No file at either path, or one that may not be followed.
    Missing,
}

impl LinkStatus {
    /// The Links panel's word for it.
    pub fn as_str(self) -> &'static str {
        match self {
            LinkStatus::Ok => "ok",
            LinkStatus::Checking => "checking",
            LinkStatus::Modified => "modified",
            LinkStatus::Missing => "missing",
        }
    }
}

/// A document's linked files as its open found them ([`DocState::links`]), and the background
/// check still running for them. The check stops when the last copy of the document's state is
/// dropped (closed, reverted).
#[derive(Clone, Default)]
pub struct OpenLinks {
    status: BTreeMap<AssetId, (SourceKey, LinkStatus)>,
    job: Option<Arc<JobHandle>>,
    /// Embedded images the check found identical to their linked files, so far.
    migrated: usize,
    notice: Option<String>,
}

impl std::fmt::Debug for OpenLinks {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("OpenLinks").field("status", &self.status.len()).field("checking", &self.checking()).finish()
    }
}

impl OpenLinks {
    /// What the open found for `asset`, while its bytes are the ones it looked at: a relink or
    /// an update gives them a new key, and their status is the Links panel's own check then.
    pub fn status(&self, asset: &Asset) -> Option<LinkStatus> {
        self.status.get(&asset.id).filter(|(key, _)| *key == asset.data.key()).map(|(_, status)| *status)
    }

    /// Whether files are still being checked in the background.
    pub fn checking(&self) -> bool {
        self.job.is_some()
    }

    /// How many files the background check has still to look at, the one it is on included.
    pub fn remaining(&self) -> usize {
        self.job.as_ref().map_or(0, |handle| {
            let state = lock(&handle.0);
            state.work.len().saturating_add(usize::from(state.busy))
        })
    }

    /// The status-line message about images that will be saved as links, once.
    pub fn take_notice(&mut self) -> Option<String> {
        self.notice.take()
    }
}

/// One file check for the background thread.
#[cfg_attr(target_arch = "wasm32", allow(dead_code))]
enum Work {
    /// Find the file that holds `source`'s bytes: one at its absolute or relative path with the
    /// recorded size and time (hashed when the record has no hash), else one of the recorded size
    /// whose content hash is the recorded one.
    Find { asset: AssetId, key: SourceKey, source: LinkedSource },
    /// Compare the file at `source`'s path (the first of its candidates that has the size of the
    /// embedded `bytes`) with the embedded bytes of a document saved before linked storage.
    Migrate { asset: AssetId, key: SourceKey, bytes: Arc<Vec<u8>>, source: LinkedSource, page: u32 },
    /// Hash the `bytes` of graphics just placed from a file, and make their proxy with `proxy`.
    Prepare { key: SourceKey, bytes: Arc<Vec<u8>>, page: u32, proxy: bool },
}

/// What a [`Work`] found. `key` is the asset's bytes' key when the check started: a result for
/// bytes that were replaced since is dropped.
#[cfg_attr(target_arch = "wasm32", allow(dead_code))]
enum Outcome {
    /// The file at `path` holds the bytes; `fingerprint` is its size and time, when they can
    /// vouch for the bytes later ([`super::links::settled`]), and `hash` the content hash of a
    /// record that had none.
    Holds { asset: AssetId, key: SourceKey, path: PathBuf, fingerprint: Option<Fingerprint>, hash: Option<String> },
    /// Files are there; none holds the bytes.
    Differs { asset: AssetId, key: SourceKey },
    /// No file at either path, or one that may not be followed.
    Absent { asset: AssetId, key: SourceKey },
    /// The embedded bytes are the file's.
    Confirmed {
        asset: AssetId,
        key: SourceKey,
        path: PathBuf,
        fingerprint: Fingerprint,
        hash: String,
        proxy: Option<designcraft_render::proxy::ProxyImage>,
    },
    /// The file isn't (or can't be shown to be) the embedded bytes: they stay embedded.
    Unconfirmed,
    /// The content hash and proxy of placed graphics' bytes ([`Work::Prepare`]).
    Prepared { key: SourceKey, hash: String, proxy: Option<designcraft_render::proxy::ProxyImage> },
}

/// A graphic placed from a file whose content hash and proxy are still to be made
/// ([`super::links::give_placed`]); [`prepare`] has them made in the background.
#[cfg_attr(target_arch = "wasm32", allow(dead_code))]
pub(crate) struct Pending {
    key: SourceKey,
    bytes: Arc<Vec<u8>>,
    page: u32,
    proxy: bool,
}

impl Pending {
    /// The work for `a`, linked with `bytes` just read from its file. Video and sound get no proxy.
    pub(super) fn of(a: &Asset, bytes: Arc<Vec<u8>>) -> Self {
        Pending { key: a.data.key(), bytes, page: a.page, proxy: !media(a) }
    }
}

/// Video and sound, which have no image proxy.
fn media(a: &Asset) -> bool {
    a.mime.starts_with("video/") || a.mime.starts_with("audio/")
}

#[derive(Default)]
#[cfg_attr(target_arch = "wasm32", allow(dead_code))]
struct Job {
    cancel: AtomicBool,
    state: Mutex<JobState>,
    changed: Condvar,
}

#[derive(Default)]
#[cfg_attr(target_arch = "wasm32", allow(dead_code))]
struct JobState {
    work: Vec<Work>,
    results: Vec<Outcome>,
    done: bool,
    /// A piece of work taken from `work` is being done.
    busy: bool,
}

impl Job {
    #[cfg_attr(target_arch = "wasm32", allow(dead_code))]
    fn cancelled(&self) -> bool {
        self.cancel.load(Ordering::Relaxed)
    }
}

/// The document state's hold on its [`Job`]: dropping the last one stops the job.
#[cfg_attr(target_arch = "wasm32", allow(dead_code))]
struct JobHandle(Arc<Job>);

impl Drop for JobHandle {
    fn drop(&mut self) {
        self.0.cancel.store(true, Ordering::Relaxed);
    }
}

/// What [`resolve`] found, for [`start`].
#[derive(Default)]
pub(crate) struct Opened {
    status: BTreeMap<AssetId, (SourceKey, LinkStatus)>,
    work: Vec<Work>,
}

/// Point each linked asset of `d`, opened from the file `document`, at its file, and queue the
/// look for the file that holds its bytes for the background check ([`start`]): nothing is read
/// or asked for its size here, so a share that doesn't answer can't stall the open. A file found
/// at the relative path becomes the link. With `migrate` (a document opened from its file,
/// neither from bytes nor recovered), embedded assets of a document saved before linked storage
/// are queued for a byte comparison with their linked files.
pub(crate) fn resolve(d: &mut Document, document: Option<&Path>, migrate: bool) -> Opened {
    let mut opened = Opened::default();
    let ids: Vec<AssetId> = d.assets.keys().copied().collect();
    for id in ids {
        let Some(asset) = d.assets.get_mut(&id) else { continue };
        if asset.storage == Storage::Linked {
            let Some(source) = LinkedSource::of(asset, document) else {
                opened.status.insert(id, (asset.data.key(), LinkStatus::Missing));
                continue;
            };
            let a = Arc::make_mut(asset);
            a.data = a.data.with_source(source.clone());
            let key = a.data.key();
            // No files on the web: the link record is kept as it is.
            let status = if cfg!(target_arch = "wasm32") {
                LinkStatus::Missing
            } else {
                opened.work.push(Work::Find { asset: id, key, source });
                LinkStatus::Checking
            };
            opened.status.insert(id, (key, status));
        } else if migrate
            && let Some(document) = document
            && let Some(work) = migration(asset, document)
        {
            opened.work.push(work);
        }
    }
    opened
}

/// `path` (a file found to hold `a`'s bytes) as `a`'s link, with its path relative to the
/// document and its bytes (under their key) read from there.
fn adopt_path(a: &mut Asset, path: &Path, document: Option<&Path>) {
    let found = path.to_string_lossy().into_owned();
    if a.link.as_deref() == Some(found.as_str()) {
        return;
    }
    a.link = Some(found);
    if let Some(relative) = document.and_then(|d| relative_to(d, path)) {
        a.link_relative = Some(relative);
    }
    if let Some(source) = LinkedSource::of(a, document) {
        a.data = AssetData::linked_under(a.data.key(), source);
    }
}

/// `target` relative to the folder of the document file `document`, as link records store it.
pub(super) fn relative_to(document: &Path, target: &Path) -> Option<String> {
    let document = std::path::absolute(document).ok()?;
    let dir = document.parent()?;
    let relative = super::datamerge::relative_between(dir, target);
    // Stored with forward slashes, which every platform's resolver reads.
    #[cfg(windows)]
    let relative = relative.replace('\\', "/");
    Some(relative)
}

/// An embedded asset of a document saved before linked storage: it has a link and no content
/// hash, which every link recorded since carries.
#[cfg_attr(target_arch = "wasm32", allow(dead_code))]
fn legacy(a: &Asset) -> bool {
    a.storage == Storage::Embedded && a.link.is_some() && a.link_hash.is_none()
}

/// The byte comparison that would let a [`legacy`] asset be saved as a link.
fn migration(a: &Asset, document: &Path) -> Option<Work> {
    #[cfg(not(target_arch = "wasm32"))]
    {
        if !legacy(a) {
            return None;
        }
        let bytes = Arc::clone(a.data.embedded_bytes()?);
        if bytes.is_empty() {
            return None;
        }
        let source = LinkedSource {
            path: a.link.clone()?,
            relative: a.link_relative.clone(),
            document: Some(document.to_path_buf()),
            len: bytes.len() as u64,
            fingerprint: None,
            hash: None,
        };
        Some(Work::Migrate { asset: a.id, key: a.data.key(), bytes, source, page: a.page })
    }
    #[cfg(target_arch = "wasm32")]
    {
        let _ = (a, document);
        None
    }
}

/// Keep what [`resolve`] found with the document's state and start the background check.
/// → how many files it checks.
pub(crate) fn start(st: &mut DocState, opened: Opened) -> usize {
    st.links = OpenLinks { status: opened.status, ..Default::default() };
    let n = opened.work.len();
    if n == 0 {
        return 0;
    }
    #[cfg(not(target_arch = "wasm32"))]
    {
        st.links.job = Some(spawn(opened.work.into_iter().rev().collect(), Vec::new()));
    }
    #[cfg(target_arch = "wasm32")]
    let _ = opened.work;
    n
}

/// A background job for `work` (taken from the end), its results starting with `results`.
#[cfg(not(target_arch = "wasm32"))]
fn spawn(work: Vec<Work>, results: Vec<Outcome>) -> Arc<JobHandle> {
    let job = Arc::new(Job::default());
    {
        let mut state = lock(&job);
        state.work = work;
        state.results = results;
    }
    let roots = crate::file_access::current();
    let worker = Arc::clone(&job);
    let worker_roots = roots.clone();
    let spawned = std::thread::Builder::new().name("designcraft-links".into()).spawn(move || run_all(&worker, worker_roots.as_ref()));
    if let Err(e) = spawned {
        // No thread: check here rather than leave the links unchecked.
        log::warn!("checking linked files without a thread: {e}");
        run_all(&job, roots.as_ref());
    }
    Arc::new(JobHandle(job))
}

/// Have the content hash and proxy of graphics just placed into `st`'s document made in the
/// background, in the order given; [`Session::poll_link_checks`] adds them when they are in.
pub(crate) fn prepare(st: &mut DocState, pending: Vec<Pending>) {
    if pending.is_empty() {
        return;
    }
    #[cfg(not(target_arch = "wasm32"))]
    {
        // Taken from the end: the first placed is made first, before checks still waiting.
        let work: Vec<Work> =
            pending.into_iter().rev().map(|Pending { key, bytes, page, proxy }| Work::Prepare { key, bytes, page, proxy }).collect();
        let mut results = Vec::new();
        if let Some(handle) = &st.links.job {
            let mut state = lock(&handle.0);
            if !state.done {
                state.work.extend(work);
                return;
            }
            // The job has ended: a new one takes over what it found and nobody has applied yet.
            results = std::mem::take(&mut state.results);
        }
        st.links.job = Some(spawn(work, results));
    }
    #[cfg(target_arch = "wasm32")]
    let _ = st;
}

fn lock(job: &Job) -> std::sync::MutexGuard<'_, JobState> {
    job.state.lock().unwrap_or_else(PoisonError::into_inner)
}

#[cfg(not(target_arch = "wasm32"))]
fn run_all(job: &Job, roots: Option<&Arc<crate::file_access::AutomationRoots>>) {
    crate::file_access::confine(roots, || {
        while !job.cancelled() {
            // Ended in the same step the queue is found empty: work queued later starts a new job.
            let next = {
                let mut state = lock(job);
                let next = state.work.pop();
                state.done |= next.is_none();
                state.busy = next.is_some();
                next
            };
            let Some(work) = next else { break };
            // A file that trips a decoder (the proxy) leaves its asset as it was.
            let outcome = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| check(work, &job.cancel)));
            match outcome {
                Ok(Some(outcome)) => lock(job).results.push(outcome),
                Ok(None) => break,
                Err(payload) => log::warn!("checking a linked file: {}", crate::guard::panic_message(payload.as_ref())),
            }
            job.changed.notify_all();
        }
    });
    {
        let mut state = lock(job);
        state.done = true;
        state.busy = false;
    }
    job.changed.notify_all();
}

/// Bytes read between two looks at the cancel flag.
#[cfg(not(target_arch = "wasm32"))]
const CHUNK: usize = 1 << 20;

/// Run one check → `None` when cancelled.
#[cfg(not(target_arch = "wasm32"))]
fn check(work: Work, cancel: &AtomicBool) -> Option<Outcome> {
    match work {
        Work::Find { asset, key, source } => {
            let found: Vec<(PathBuf, Fingerprint)> =
                crate::asset_io::candidates(&source).into_iter().filter_map(|p| fingerprint(&p).map(|f| (p, f))).collect();
            if let Some((path, at)) = found.iter().find(|(_, f)| super::links::settled(f) && Some(*f) == source.fingerprint) {
                // A record without a hash gets the file's, read while it keeps that size and time.
                let hash = match source.hash {
                    Some(_) => None,
                    None => read_file(path, source.len, cancel, |_, _| true)?.filter(|_| fingerprint(path) == Some(*at)),
                };
                return Some(Outcome::Holds { asset, key, path: path.clone(), fingerprint: None, hash });
            }
            if found.is_empty() {
                return Some(Outcome::Absent { asset, key });
            }
            let Some(hash) = &source.hash else { return Some(Outcome::Differs { asset, key }) };
            for (path, before) in found.iter().filter(|(_, f)| f.size == source.len) {
                match read_file(path, source.len, cancel, |_, _| true)? {
                    Some(found) if found == *hash && fingerprint(path) == Some(*before) => {
                        let fingerprint = super::links::settled(before).then_some(*before);
                        return Some(Outcome::Holds { asset, key, path: path.clone(), fingerprint, hash: None });
                    }
                    _ => {}
                }
            }
            Some(Outcome::Differs { asset, key })
        }
        Work::Migrate { asset, key, bytes, source, page } => {
            let len = bytes.len() as u64;
            let Some((path, before)) =
                crate::asset_io::candidates(&source).into_iter().find_map(|p| fingerprint(&p).filter(|f| f.size == len).map(|f| (p, f)))
            else {
                return Some(Outcome::Unconfirmed);
            };
            let same = |at: usize, chunk: &[u8]| at.checked_add(chunk.len()).and_then(|end| bytes.get(at..end)) == Some(chunk);
            let read = read_file(&path, bytes.len() as u64, cancel, same)?;
            // Changed while it was read: not the bytes it had.
            if read.is_none() || fingerprint(&path) != Some(before) {
                return Some(Outcome::Unconfirmed);
            }
            let hash = crate::asset_io::content_hash(&bytes);
            let proxy = designcraft_render::proxy::make_proxy(&bytes, page);
            // A time that can't vouch for the bytes (changed in the last seconds, or none) is
            // kept as one no file has: saves then go by the hash, not by size and time.
            let fingerprint = if super::links::settled(&before) { before } else { Fingerprint { size: len, mtime: super::links::UNVERIFIED_MTIME } };
            Some(Outcome::Confirmed { asset, key, path, fingerprint, hash, proxy })
        }
        Work::Prepare { key, bytes, page, proxy } => {
            let hash = hash_bytes(&bytes, cancel)?;
            if cancel.load(Ordering::Relaxed) {
                return None;
            }
            let proxy = if proxy { designcraft_render::proxy::make_proxy(&bytes, page) } else { None };
            Some(Outcome::Prepared { key, hash, proxy })
        }
    }
}

/// [`crate::asset_io::content_hash`] of `bytes`, a chunk at a time → `None` when cancelled.
#[cfg(not(target_arch = "wasm32"))]
fn hash_bytes(bytes: &[u8], cancel: &AtomicBool) -> Option<String> {
    use sha2::Digest;
    let mut hasher = sha2::Sha256::new();
    for chunk in bytes.chunks(CHUNK) {
        if cancel.load(Ordering::Relaxed) {
            return None;
        }
        hasher.update(chunk);
    }
    Some(hex(hasher))
}

/// `sha256:` and the hex digits of what `hasher` was given.
#[cfg(not(target_arch = "wasm32"))]
fn hex(hasher: sha2::Sha256) -> String {
    use sha2::Digest;
    use std::fmt::Write;
    let mut hex = String::with_capacity(7 + 64);
    hex.push_str("sha256:");
    for b in hasher.finalize() {
        let _ = write!(hex, "{b:02x}");
    }
    hex
}

#[cfg(not(target_arch = "wasm32"))]
fn fingerprint(path: &Path) -> Option<Fingerprint> {
    crate::io::file_metadata(path).map(|m| crate::asset_io::fingerprint_from(&m))
}

/// Read the file at `path`, which must be `len` bytes, handing each chunk to `keep` (with its
/// offset) until it says no → the content hash of a file of `len` bytes that `keep` kept to the
/// end, `Some(None)` for any other file, `None` when cancelled.
#[cfg(not(target_arch = "wasm32"))]
fn read_file(path: &Path, len: u64, cancel: &AtomicBool, mut keep: impl FnMut(usize, &[u8]) -> bool) -> Option<Option<String>> {
    use sha2::Digest;
    use std::io::Read;
    let Ok(file) = crate::io::open(path) else { return Some(None) };
    // One byte past the size: a file that grew is seen without reading all of it.
    let mut file = file.take(len.saturating_add(1));
    let mut hasher = sha2::Sha256::new();
    let mut buf = vec![0u8; CHUNK];
    let mut at = 0usize;
    loop {
        if cancel.load(Ordering::Relaxed) {
            return None;
        }
        let n = match file.read(&mut buf) {
            Ok(0) => break,
            Ok(n) => n,
            Err(e) if e.kind() == std::io::ErrorKind::Interrupted => continue,
            Err(_) => return Some(None),
        };
        let Some(chunk) = buf.get(..n) else { return Some(None) };
        if !keep(at, chunk) {
            return Some(None);
        }
        hasher.update(chunk);
        at = at.saturating_add(n);
    }
    if at as u64 != len {
        return Some(None);
    }
    Some(Some(hex(hasher)))
}

impl Session {
    /// Apply what the background checks of the open documents' linked files found → whether a
    /// document changed. Nothing found changes what a document shows or makes it unsaved: a
    /// file's new time, the link to the file that was found, an embedded image that will be
    /// saved as a link. Results wait while a gesture or a data-merge preview runs.
    pub fn poll_link_checks(&mut self) -> bool {
        let mut changed = false;
        for st in &mut self.docs {
            changed |= apply(st);
        }
        if changed {
            designcraft_doc::asset_data::links_changed();
        }
        changed
    }
}

fn apply(st: &mut DocState) -> bool {
    let Some(job) = st.links.job.clone() else { return false };
    if st.interaction.is_some() || st.preview_stash.is_some() {
        return false;
    }
    let (results, done) = {
        let mut state = lock(&job.0);
        (std::mem::take(&mut state.results), state.done)
    };
    if done {
        st.links.job = None;
    }
    let document = st.path.as_deref().map(PathBuf::from);
    let clean = !st.is_dirty();
    let found = !results.is_empty();
    let mut changed = false;
    for outcome in results {
        changed |= apply_one(st, outcome, document.as_deref());
    }
    if changed && clean {
        st.saved_doc = Arc::clone(&st.doc);
    }
    if done && st.links.migrated > 0 {
        st.links.notice = Some(notice(st.links.migrated));
    }
    // A status changed, or the check ended: either way something to tell.
    found || done
}

/// The asset `id` of the live document to change, while its bytes are still `key`'s.
fn current(st: &mut DocState, id: AssetId, key: SourceKey) -> Option<&mut Asset> {
    if st.doc.assets.get(&id).is_none_or(|a| a.data.key() != key) {
        return None;
    }
    Arc::make_mut(&mut st.doc).assets.get_mut(&id).map(Arc::make_mut)
}

fn apply_one(st: &mut DocState, outcome: Outcome, document: Option<&Path>) -> bool {
    match outcome {
        Outcome::Holds { asset, key, path, fingerprint, hash } => {
            let held = held(st, asset, key, &path, fingerprint, document);
            let hashed = hash.is_some_and(|hash| add_everywhere(st, key, &hash, None));
            held || hashed
        }
        Outcome::Differs { asset, key } => {
            found(st, asset, key, LinkStatus::Modified);
            false
        }
        Outcome::Absent { asset, key } => {
            found(st, asset, key, LinkStatus::Missing);
            false
        }
        Outcome::Confirmed { asset, key, path, fingerprint, hash, proxy } => {
            let Some(a) = current(st, asset, key) else { return false };
            if a.storage != Storage::Embedded {
                return false;
            }
            a.link = Some(path.to_string_lossy().into_owned());
            if let Some(relative) = document.and_then(|d| relative_to(d, &path)) {
                a.link_relative = Some(relative);
            }
            a.link_fingerprint = Some(fingerprint);
            a.link_hash = Some(hash);
            a.proxy = proxy.map(doc_proxy);
            a.storage = Storage::Linked;
            st.links.status.insert(asset, (key, LinkStatus::Ok));
            st.links.migrated = st.links.migrated.saturating_add(1);
            true
        }
        Outcome::Unconfirmed => false,
        Outcome::Prepared { key, hash, proxy } => add_everywhere(st, key, &hash, proxy.map(doc_proxy).as_ref()),
    }
}

/// Keep `status` for asset `id`, while its bytes are still `key`'s.
fn found(st: &mut DocState, id: AssetId, key: SourceKey, status: LinkStatus) {
    if st.doc.assets.get(&id).is_some_and(|a| a.data.key() == key) {
        st.links.status.insert(id, (key, status));
    }
}

/// A proxy the background job made, as a document keeps it.
fn doc_proxy(p: designcraft_render::proxy::ProxyImage) -> designcraft_doc::Proxy {
    designcraft_doc::Proxy { mime: p.mime.into(), pixels: p.pixels, data: AssetData::embedded(p.data) }
}

/// The file at `path` holds asset `id`'s bytes (still `key`'s): its link, with `fingerprint`
/// when the file's time can vouch for them. → whether the document changed.
fn held(st: &mut DocState, id: AssetId, key: SourceKey, path: &Path, fingerprint: Option<Fingerprint>, document: Option<&Path>) -> bool {
    let unchanged = fingerprint.is_none()
        && st.doc.assets.get(&id).is_some_and(|a| a.data.key() == key && a.link.as_deref() == Some(path.to_string_lossy().as_ref()));
    if unchanged {
        // The file at the recorded path, with the recorded size and time: nothing to change.
        st.links.status.insert(id, (key, LinkStatus::Ok));
        return false;
    }
    let Some(a) = current(st, id, key) else { return false };
    if let Some(f) = fingerprint {
        a.link_fingerprint = Some(f);
    }
    adopt_path(a, path, document);
    // The file holds the same bytes: the key stays, also for a record with no hash.
    if let Some(source) = LinkedSource::of(a, document) {
        a.data = AssetData::linked_under(key, source);
    }
    st.links.status.insert(id, (key, LinkStatus::Ok));
    true
}

/// Add the content `hash` and `proxy` of the bytes `key` to the graphics that have those bytes
/// and lack them ([`super::links::add_prepared`]), in the live document, the undo history and the
/// last saved state alike: a redo of an undone placement, or an undo to before an open-time
/// check, then has them too. → whether the live document changed.
fn add_everywhere(st: &mut DocState, key: SourceKey, hash: &str, proxy: Option<&designcraft_doc::Proxy>) -> bool {
    // Each document changed once, its holders sharing the result: a history entry that was the
    // saved state still is.
    let mut done: Vec<(Arc<Document>, Arc<Document>)> = Vec::new();
    let mut add = |doc: &mut Arc<Document>| {
        if let Some((_, after)) = done.iter().find(|(before, _)| Arc::ptr_eq(before, doc)) {
            *doc = Arc::clone(after);
            return true;
        }
        let before = Arc::clone(doc);
        let changed = add_to(doc, key, hash, proxy);
        if changed {
            done.push((before, Arc::clone(doc)));
        }
        changed
    };
    let changed = add(&mut st.doc);
    for entry in st.history.undo.iter_mut().chain(st.history.redo.iter_mut()) {
        add(&mut entry.doc);
    }
    add(&mut st.saved_doc);
    changed
}

/// [`add_everywhere`] for one version of the document → whether `doc` changed.
fn add_to(doc: &mut Arc<Document>, key: SourceKey, hash: &str, proxy: Option<&designcraft_doc::Proxy>) -> bool {
    let wants = |a: &Asset| a.data.key() == key && (a.link_hash.is_none() || (proxy.is_some() && a.storage == Storage::Linked && a.proxy.is_none()));
    let ids: Vec<AssetId> = doc.assets.values().filter(|a| wants(a)).map(|a| a.id).collect();
    if ids.is_empty() {
        return false;
    }
    let d = Arc::make_mut(doc);
    for id in ids {
        if let Some(a) = d.assets.get_mut(&id).map(Arc::make_mut) {
            super::links::add_prepared(a, hash, proxy);
        }
    }
    true
}

/// The status line's message once `n` embedded images were found identical to their files.
fn notice(n: usize) -> String {
    if n == 1 {
        "1 image will be saved as a link to its file (Links ▸ Embed keeps a copy)".into()
    } else {
        format!("{n} images will be saved as links to their files (Links ▸ Embed keeps a copy)")
    }
}

/// `d` as saved to the file `destination`, when that changes it (else `None`; the live document
/// isn't touched). `from` is the file `d` was opened from or last saved to, which relative links
/// are resolved against: each link becomes the absolute path it names, with its path relative to
/// the destination's folder. An embedded image to be saved as a link (a document saved before
/// linked storage, [`resolve`]) whose file has changed since the check is embedded in this save
/// after all.
pub(crate) fn for_destination(d: &Document, from: Option<&Path>, destination: &Path) -> Option<Document> {
    rewrite(d, from, destination, false)
}

/// [`for_destination`] for a recovery snapshot of `d`, whose file is `document`. Embedded images
/// to be saved as links stay as the document's file has them (embedded, to be compared with their
/// files again), so a recovered document never depends on a file the user hasn't saved a link to.
pub(crate) fn for_recovery(d: &Document, document: &Path) -> Option<Document> {
    rewrite(d, Some(document), document, true)
}

fn rewrite(d: &Document, from: Option<&Path>, destination: &Path, recovery: bool) -> Option<Document> {
    let mut changes: Vec<(AssetId, Option<String>, Option<String>, bool)> = Vec::new();
    for a in d.assets.values() {
        let Some(link) = a.link.as_deref() else { continue };
        let absolute = crate::io::document_path(link, from).ok();
        let new_link = absolute.as_ref().map(|p| p.to_string_lossy().into_owned()).filter(|p| p != link);
        let relative = absolute.as_deref().and_then(|p| relative_to(destination, p)).filter(|r| a.link_relative.as_ref() != Some(r));
        let embed = a.storage == Storage::Linked && !a.data.is_linked() && (recovery || !still_there(a, from));
        if new_link.is_some() || relative.is_some() || embed {
            changes.push((a.id, new_link, relative, embed));
        }
    }
    if changes.is_empty() {
        return None;
    }
    let mut saved = d.clone();
    for (id, link, relative, embed) in changes {
        let Some(a) = saved.assets.get_mut(&id).map(Arc::make_mut) else { continue };
        if let Some(link) = link {
            a.link = Some(link);
        }
        if let Some(relative) = relative {
            a.link_relative = Some(relative);
        }
        if embed {
            a.storage = Storage::Embedded;
            if recovery {
                // As the document's file has it: the next open from that file compares it again.
                a.link_hash = None;
                a.proxy = None;
            }
        }
    }
    Some(saved)
}

/// Whether a file still has the size and time `a`'s link recorded when its bytes were found in it
/// (a time that can vouch for them, [`super::links::settled`]).
fn still_there(a: &Asset, document: Option<&Path>) -> bool {
    #[cfg(not(target_arch = "wasm32"))]
    {
        let Some(source) = LinkedSource::of(a, document) else { return false };
        crate::asset_io::candidates(&source)
            .iter()
            .any(|p| fingerprint(p).is_some_and(|f| super::links::settled(&f) && Some(f) == source.fingerprint))
    }
    #[cfg(target_arch = "wasm32")]
    {
        let _ = (a, document);
        false
    }
}

/// After `d` was saved to `destination`: linked assets read their files relative to it, and
/// embedded images saved as links let go of their bytes (read from the file when next needed;
/// what was decoded of them stays). → the document with that, when it changes anything.
pub(crate) fn after_save(d: &Document, destination: &Path) -> Option<Document> {
    let relinked: Vec<(AssetId, AssetData)> = d
        .assets
        .values()
        .filter(|a| a.storage == Storage::Linked && (cfg!(not(target_arch = "wasm32")) || a.data.is_linked()))
        .filter_map(|a| {
            let source = LinkedSource::of(a, Some(destination))?;
            if a.data.linked_source() == Some(&source) {
                return None;
            }
            // The record names the same bytes, now resolved against the destination: the key
            // stays, also for a link with no hash yet. Embedded bytes are linked only once their
            // hash was found in the file (`Confirmed`).
            Some((a.id, AssetData::linked_under(a.data.key(), source)))
        })
        .collect();
    if relinked.is_empty() {
        return None;
    }
    let mut out = d.clone();
    for (id, data) in relinked {
        if let Some(a) = out.assets.get_mut(&id) {
            Arc::make_mut(a).data = data;
        }
    }
    Some(out)
}

/// `d` as a save writes it, when that changes it: each linked graphic without a proxy (placed
/// moments ago, its proxy still being made in the background) gets one from its bytes, so a saved
/// link always has a preview to draw while its file can't be read. Nothing is hashed: a link
/// without a hash is saved without one. A graphic whose bytes can't be read or don't decode stays
/// without a proxy. `None` when no graphic needs one, or once `cancelled` says so.
pub(crate) fn with_proxies(d: &Document, cancelled: impl Fn() -> bool) -> Option<Document> {
    let wanting: Vec<AssetId> = d.assets.values().filter(|a| a.storage == Storage::Linked && a.proxy.is_none() && !media(a)).map(|a| a.id).collect();
    if wanting.is_empty() {
        return None;
    }
    let mut out = d.clone();
    let mut changed = false;
    for id in wanting {
        if cancelled() {
            return None;
        }
        let Some(asset) = out.assets.get_mut(&id) else { continue };
        let Ok(bytes) = asset.data.load() else { continue };
        // A file that trips a decoder leaves its graphic without a proxy, not the save undone.
        let made = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| super::links::proxy_of(asset, &bytes)));
        match made {
            Ok(Some(proxy)) => {
                Arc::make_mut(asset).proxy = Some(proxy);
                changed = true;
            }
            Ok(None) => {}
            Err(payload) => log::warn!("making the proxy of {}: {}", asset.name, crate::guard::panic_message(payload.as_ref())),
        }
    }
    changed.then_some(out)
}

/// `d` with every linked file it uses copied in (Save a Copy `{embedLinks: true}`): a document
/// that opens anywhere. Fails, naming the image, when a file is missing or has changed.
pub(crate) fn embed_linked(d: &mut Document, command: &str) -> Result<()> {
    for id in d.used_assets() {
        let Some(asset) = d.assets.get_mut(&id) else { continue };
        if asset.storage != Storage::Linked {
            continue;
        }
        let bytes =
            asset.data.load().map_err(|e| bad(command, format!("{}: {e}; relink or update it to save a copy with its linked files", asset.name)))?;
        let a = Arc::make_mut(asset);
        a.data = AssetData::embedded(bytes);
        a.storage = Storage::Embedded;
        a.proxy = None;
    }
    Ok(())
}

/// On the web, a save keeps linked files as links only: it can't read them to embed them.
pub(crate) fn web_save_warnings(d: &Document) -> Vec<String> {
    let n = d.assets.values().filter(|a| a.storage == Storage::Linked).count();
    if cfg!(target_arch = "wasm32") && n > 0 {
        vec![format!("{n} linked files are saved as links only: the web version can't read them to embed them")]
    } else {
        Vec::new()
    }
}

/// Wait until the background checks of `s`'s documents are done, then apply them.
#[cfg(test)]
pub(crate) fn wait(s: &mut Session) {
    for st in &s.docs {
        let Some(job) = st.links.job.clone() else { continue };
        let mut state = lock(&job.0);
        while !state.done {
            state = job.0.changed.wait_timeout(state, std::time::Duration::from_secs(10)).unwrap_or_else(PoisonError::into_inner).0;
        }
    }
    s.poll_link_checks();
}

#[cfg(test)]
mod tests;
