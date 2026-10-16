//! Crash recovery (InDesign's automatic recovery data): unsaved changes of every open document are
//! written to a recovery folder (`file.recovery.save`, called on a timer by the app); saving or
//! closing a document removes its file; after a crash `file.recovery.open` reopens what was left
//! there as unsaved documents that remember where they were saved.
//!
//! A recovery entry is `<uid>.designcraft` (the document) plus `<uid>.json` (`{"path", "title",
//! "saved"}`: the original file, the title, and when it was written).
//!
//! Document uids start at 1 in every process, so each running app keeps its entries in a folder
//! of its own in the recovery folder (`run_dir`) and holds the lock in it for as long as it runs.
//! An app that starts moves into its own folder what apps that have ended left in theirs, and
//! leaves the folder of one still running alone.
//!
//! A folder given to `file.recovery.save` that is not this run's own may hold other runs' entries
//! under the same uids. This run writes there only entries it finds free, replaces and removes
//! only those it wrote or recovered, and refuses the folder of a run whose lock is still held.
//!
//! Each file is replaced atomically, but the pair is not a transaction. The document is published
//! and made durable before its metadata so a crash can leave metadata missing or stale, never a
//! listed metadata record without a complete primary document. Listing and opening recovery data
//! therefore treat metadata as optional.

#[cfg(not(target_arch = "wasm32"))]
use std::io::Write;
use std::path::{Path, PathBuf};

use serde_json::{Value, json};

use crate::{DocState, EngineError, Result, Session};

/// The platform's per-user recovery folder.
pub fn default_dir() -> Option<PathBuf> {
    let home = std::env::var_os("HOME").map(PathBuf::from);
    if cfg!(target_os = "macos") {
        return home.map(|h| h.join("Library/Application Support/DesignCraft/Recovery"));
    }
    if cfg!(target_os = "windows") {
        return std::env::var_os("APPDATA").map(|a| PathBuf::from(a).join("DesignCraft").join("Recovery"));
    }
    std::env::var_os("XDG_DATA_HOME").map(PathBuf::from).or_else(|| home.map(|h| h.join(".local/share"))).map(|d| d.join("designcraft/recovery"))
}

/// This process's folder in a recovery folder, with the lock that says it is still running.
#[cfg(not(target_arch = "wasm32"))]
struct Run {
    root: PathBuf,
    dir: PathBuf,
    lock: std::fs::File,
    #[cfg(unix)]
    directory: std::fs::File,
}

#[cfg(not(target_arch = "wasm32"))]
static RUNS: std::sync::Mutex<Vec<Run>> = std::sync::Mutex::new(Vec::new());

/// The lock in a run's folder.
#[cfg(not(target_arch = "wasm32"))]
const LOCK: &str = "lock";

/// This run's own folder in the recovery folder `root`, made on the first call. Document uids
/// start at 1 in every process, so two running apps writing to one folder wrote over each
/// other's entries. The folder's lock is held until the process ends: an app that starts leaves
/// the folder of one that is still running alone.
#[cfg(not(target_arch = "wasm32"))]
pub fn run_dir(root: &Path) -> Result<PathBuf> {
    let mut runs = RUNS.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
    if let Some(run) = runs.iter().find(|r| r.root == root) {
        return Ok(run.dir.clone());
    }
    secure_recovery_dir(root)?;
    for n in 0..64 {
        let dir = root.join(format!("run-{}-{n}", std::process::id()));
        // Only a folder this run makes is its own. One that is there is another app's, or was
        // left by a run that ended under this process id.
        #[cfg(unix)]
        let made = {
            use std::os::unix::fs::DirBuilderExt;

            let mut builder = std::fs::DirBuilder::new();
            builder.mode(0o700).create(&dir)
        };
        #[cfg(not(unix))]
        let made = std::fs::create_dir(&dir);
        match made {
            Ok(()) => {}
            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => continue,
            Err(e) => return Err(EngineError::Other(format!("{}: {e}", dir.display()))),
        }
        let locked = validate_recovery_dir(&dir).and_then(|()| {
            let lock = new_lock(&dir).map_err(|e| EngineError::Other(format!("{}: {e}", dir.display())))?;
            match lock.try_lock() {
                Ok(()) => {
                    validate_lock(&dir, &lock)?;
                    Ok(Some(lock))
                }
                Err(std::fs::TryLockError::WouldBlock) => Ok(None),
                Err(std::fs::TryLockError::Error(e)) => Err(EngineError::Other(format!("{}: {e}", dir.display()))),
            }
        });
        match locked {
            // An app that is starting can take a folder this new for one a run left, and remove
            // it before the lock is held: then the lock is gone, or that app holds it.
            Ok(Some(lock)) => {
                #[cfg(unix)]
                let directory = std::fs::File::open(&dir).map_err(|e| EngineError::Other(format!("{}: {e}", dir.display())))?;
                runs.push(Run {
                    root: root.to_path_buf(),
                    dir: dir.clone(),
                    lock,
                    #[cfg(unix)]
                    directory,
                });
                return Ok(dir);
            }
            Ok(_) => {}
            Err(e) => {
                // Never unlink a lock that another starting run may now own.
                let _ = std::fs::remove_dir(&dir);
                return Err(e);
            }
        }
    }
    Err(EngineError::Other(format!("{}: no free folder for this run", root.display())))
}

#[cfg(not(target_arch = "wasm32"))]
fn new_lock(dir: &Path) -> std::io::Result<std::fs::File> {
    let mut new = std::fs::OpenOptions::new();
    new.read(true).write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        new.mode(0o600);
    }
    let mut file = new.open(dir.join(LOCK))?;
    // A marker supplements Unix inode validation and identifies locks on other platforms.
    // Windows uses live handle identity because its exclusive lock prevents separate-handle reads.
    // Empty older-version locks remain valid.
    static NEXT_LOCK: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    let now = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_err(std::io::Error::other)?.as_nanos();
    let marker = format!("{}:{}:{now}", std::process::id(), NEXT_LOCK.fetch_add(1, std::sync::atomic::Ordering::Relaxed));
    file.write_all(marker.as_bytes())?;
    file.sync_all()?;
    Ok(file)
}

/// Check the current lock name without accepting a symlink or a lock held through an unlinked
/// handle. An advisory lock on an old handle does not protect a replacement at the same name.
#[cfg(not(target_arch = "wasm32"))]
fn validate_lock(dir: &Path, lock: &std::fs::File) -> Result<()> {
    let path = dir.join(LOCK);
    let fail = || EngineError::Other(format!("{}: this run no longer owns its recovery lock", path.display()));
    let before = std::fs::symlink_metadata(&path).map_err(|_| fail())?;
    if is_link_like(&before) || !before.file_type().is_file() {
        return Err(fail());
    }
    let named = std::fs::File::open(&path).map_err(|_| fail())?;
    let held = lock.metadata().map_err(|_| fail())?;
    let current = named.metadata().map_err(|_| fail())?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        if held.dev() != before.dev() || held.ino() != before.ino() || held.dev() != current.dev() || held.ino() != current.ino() {
            return Err(fail());
        }
    }
    #[cfg(not(unix))]
    let _ = (held, current);
    // Windows locks prevent reads through separately opened handles, even in this process.
    // Compare the held and named file identities without releasing the recovery lock.
    #[cfg(windows)]
    let held_identity = {
        let held = same_file::Handle::from_file(lock.try_clone().map_err(|_| fail())?).map_err(|_| fail())?;
        let current = same_file::Handle::from_file(named.try_clone().map_err(|_| fail())?).map_err(|_| fail())?;
        if held != current {
            return Err(fail());
        }
        held
    };
    #[cfg(not(windows))]
    if lock_marker(lock).map_err(|_| fail())? != lock_marker(&named).map_err(|_| fail())? {
        return Err(fail());
    }
    let after = std::fs::symlink_metadata(&path).map_err(|_| fail())?;
    if is_link_like(&after) || !after.file_type().is_file() {
        return Err(fail());
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        if held.dev() != after.dev() || held.ino() != after.ino() {
            return Err(fail());
        }
    }
    #[cfg(windows)]
    {
        let after_named = std::fs::File::open(&path).map_err(|_| fail())?;
        let after_identity = same_file::Handle::from_file(after_named).map_err(|_| fail())?;
        if held_identity != after_identity {
            return Err(fail());
        }
    }
    Ok(())
}

#[cfg(all(not(target_arch = "wasm32"), not(windows)))]
fn lock_marker(file: &std::fs::File) -> std::io::Result<Vec<u8>> {
    use std::io::{Read, Seek};
    let mut file = file.try_clone()?;
    file.seek(std::io::SeekFrom::Start(0))?;
    let mut bytes = Vec::new();
    file.take(129).read_to_end(&mut bytes)?;
    if bytes.len() > 128 {
        return Err(std::io::Error::other("invalid recovery lock marker"));
    }
    Ok(bytes)
}

/// A run's folder that was removed while it runs is made again by the next save: so is its lock.
/// Returns whether `dir` is this run's folder.
#[cfg(not(target_arch = "wasm32"))]
fn relock(dir: &Path, created: bool) -> Result<bool> {
    let mut runs = RUNS.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
    let Some(run) = runs.iter_mut().find(|r| r.dir == dir) else { return Ok(false) };
    validate_recovery_dir(&run.root)?;
    #[cfg(unix)]
    let directory = std::fs::symlink_metadata(dir).map_err(|e| EngineError::Other(format!("{}: {e}", dir.display())))?;
    // Reclaim a newly recreated folder only when this operation actually created it. Existing
    // replacements, including ones without a lock, may contain somebody else's documents.
    let same_directory = {
        #[cfg(unix)]
        {
            use std::os::unix::fs::MetadataExt;
            let held = run.directory.metadata().map_err(|e| EngineError::Other(format!("{}: {e}", dir.display())))?;
            held.dev() == directory.dev() && held.ino() == directory.ino()
        }
        #[cfg(not(unix))]
        {
            // Without stable directory identity, a valid held lock is the proof of ownership.
            // A missing lock cannot authorize an existing folder, even on coarse-timestamp disks.
            validate_lock(dir, &run.lock).is_ok()
        }
    };
    if !created && !same_directory {
        return Err(EngineError::Other(format!("{}: this run no longer owns its recovery folder", dir.display())));
    }
    match std::fs::symlink_metadata(dir.join(LOCK)) {
        Ok(_) => return validate_lock(dir, &run.lock).map(|()| true),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
        Err(e) => return Err(EngineError::Other(format!("{}: {e}", dir.display()))),
    }
    let lock = new_lock(dir).map_err(|e| EngineError::Other(format!("{}: {e}", dir.display())))?;
    lock.try_lock().map_err(|e| EngineError::Other(format!("{}: {e}", dir.display())))?;
    validate_lock(dir, &lock)?;
    #[cfg(unix)]
    let replacement_directory = std::fs::File::open(dir).map_err(|e| EngineError::Other(format!("{}: {e}", dir.display())))?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        let held = replacement_directory.metadata().map_err(|e| EngineError::Other(format!("{}: {e}", dir.display())))?;
        if held.dev() != directory.dev() || held.ino() != directory.ino() {
            return Err(EngineError::Other(format!("{}: recovery folder changed while restoring its lock", dir.display())));
        }
    }
    validate_lock(dir, &lock)?;
    run.lock = lock;
    #[cfg(unix)]
    {
        run.directory = replacement_directory;
    }
    Ok(true)
}

/// Entries this process wrote or recovered in folders that are not its run's own: (canonical
/// folder, uid). Document uids start at 1 in every process, so a shared folder's entry under
/// one of this run's uids may be another run's.
static WRITTEN: std::sync::Mutex<std::collections::BTreeSet<(PathBuf, u64)>> = std::sync::Mutex::new(std::collections::BTreeSet::new());

/// What this run may do in a recovery folder.
struct Access {
    /// This run's own folder (`run_dir`): every entry in it is this run's.
    own: bool,
    key: PathBuf,
}

impl Access {
    fn owns(&self, uid: u64) -> bool {
        self.own || WRITTEN.lock().unwrap_or_else(std::sync::PoisonError::into_inner).contains(&(self.key.clone(), uid))
    }

    fn record(&self, uid: u64) {
        if !self.own {
            WRITTEN.lock().unwrap_or_else(std::sync::PoisonError::into_inner).insert((self.key.clone(), uid));
        }
    }

    fn forget(&self, uid: u64) {
        WRITTEN.lock().unwrap_or_else(std::sync::PoisonError::into_inner).remove(&(self.key.clone(), uid));
    }

    /// Refuse an entry of another run before anything is written: this run never replaces it.
    fn check_free(&self, dir: &Path, uid: u64) -> Result<()> {
        if self.owns(uid) {
            return Ok(());
        }
        let (doc, meta) = files(dir, uid);
        for path in [doc, meta] {
            match std::fs::symlink_metadata(&path) {
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
                _ => {
                    return Err(EngineError::Other(format!(
                        "{}: recovery data this run did not write is there; recover it or choose another recovery folder",
                        path.display()
                    )));
                }
            }
        }
        Ok(())
    }
}

/// Check what this run may do in `dir`. A folder that is not this run's own may be the folder
/// of another running app, whose lock that app holds: this run leaves it alone.
fn access(dir: &Path, created: bool) -> Result<Access> {
    #[cfg(not(target_arch = "wasm32"))]
    let own = relock(dir, created)?;
    #[cfg(not(target_arch = "wasm32"))]
    if !own {
        check_unheld(dir)?;
    }
    #[cfg(target_arch = "wasm32")]
    let own = {
        let _ = created;
        false
    };
    Ok(Access { own, key: directory_key(dir)? })
}

#[cfg(not(target_arch = "wasm32"))]
fn check_unheld(dir: &Path) -> Result<()> {
    let path = dir.join(LOCK);
    let in_use =
        || EngineError::Other(format!("{}: another running DesignCraft keeps its recovery data here; choose another recovery folder", dir.display()));
    match std::fs::symlink_metadata(&path) {
        Ok(metadata) if is_link_like(&metadata) || !metadata.file_type().is_file() => return Err(in_use()),
        Ok(_) => {}
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(()),
        Err(e) => return Err(EngineError::Other(format!("{}: {e}", path.display()))),
    }
    // Opened for writing: over NFS an exclusive lock on a file opened for reading is refused.
    let lock = std::fs::OpenOptions::new().read(true).write(true).open(&path).map_err(|e| EngineError::Other(format!("{}: {e}", path.display())))?;
    match lock.try_lock() {
        // Free: the run that held it has ended. Dropping the handle releases it again.
        Ok(()) => Ok(()),
        Err(std::fs::TryLockError::WouldBlock) => Err(in_use()),
        Err(std::fs::TryLockError::Error(e)) => Err(EngineError::Other(format!("{}: {e}", path.display()))),
    }
}

/// Move into `own`, this run's folder, what is left to recover beside it: the entries of runs
/// that have ended, and those written straight into the recovery folder before runs had folders.
#[cfg(not(target_arch = "wasm32"))]
fn take_over(own: &Path) {
    let root = {
        let runs = RUNS.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
        let Some(run) = runs.iter().find(|r| r.dir == own) else { return };
        run.root.clone()
    };
    move_entries(&root, own);
    let Ok(rd) = std::fs::read_dir(&root) else { return };
    for e in rd.flatten() {
        let dir = e.path();
        if dir == own || !e.file_name().to_string_lossy().starts_with("run-") || validate_recovery_dir(&dir).is_err() {
            continue;
        }
        // Opened for writing: over NFS an exclusive lock on a file opened for reading is refused.
        let Ok(metadata) = std::fs::symlink_metadata(dir.join(LOCK)) else { continue };
        if is_link_like(&metadata) || !metadata.file_type().is_file() {
            continue;
        }
        let Ok(lock) = std::fs::OpenOptions::new().read(true).write(true).open(dir.join(LOCK)) else { continue };
        match lock.try_lock() {
            // Free: the run has ended. Holding it keeps another starting app out.
            Ok(()) => {}
            Err(std::fs::TryLockError::WouldBlock) => continue,
            Err(std::fs::TryLockError::Error(e)) => {
                log::warn!("recovery data in {} is left there: {e}", dir.display());
                continue;
            }
        }
        if let Err(e) = validate_lock(&dir, &lock) {
            log::warn!("recovery data in {} is left there: {e}", dir.display());
            continue;
        }
        move_entries(&dir, own);
        remove_emptied(&dir);
    }
}

/// Remove the folder of a run that ended, with what the app left in it beside documents. A
/// document still in it, or a file the app did not write, keeps the folder as it is.
#[cfg(not(target_arch = "wasm32"))]
fn remove_emptied(dir: &Path) {
    let Ok(rd) = std::fs::read_dir(dir) else { return };
    let mut left = Vec::new();
    for e in rd {
        let Ok(e) = e else { return };
        let path = e.path();
        let name = e.file_name();
        let name = name.to_string_lossy();
        let numbered = path.file_stem().and_then(|s| s.to_str()).is_some_and(|s| s.parse::<u64>().is_ok());
        // Metadata whose document has gone, and a file a crash left half written.
        if (numbered && name.ends_with(".json")) || (name.starts_with(".designcraft-recovery-") && name.ends_with(".tmp")) {
            left.push(path);
        } else if name != LOCK {
            return;
        }
    }
    for path in left {
        let _ = std::fs::remove_file(path);
    }
    // The lock last: until then the folder is still that of a run that ended.
    let _ = std::fs::remove_file(dir.join(LOCK));
    let _ = std::fs::remove_dir(dir);
}

#[cfg(not(target_arch = "wasm32"))]
fn move_entries(from: &Path, own: &Path) {
    for (uid, _) in list(from) {
        let (doc, meta) = files(from, uid);
        // Under a uid no document of this run has or gets.
        let (own_doc, own_meta) = files(own, crate::NEXT_UID.fetch_add(1, std::sync::atomic::Ordering::Relaxed));
        // The document first: its metadata is optional.
        if std::fs::rename(&doc, &own_doc).is_ok() {
            let _ = std::fs::rename(&meta, &own_meta);
        }
    }
}

fn files(dir: &Path, uid: u64) -> (PathBuf, PathBuf) {
    (dir.join(format!("{uid}.designcraft")), dir.join(format!("{uid}.json")))
}

/// Remove a document's recovery entry (it was saved or closed). In a folder that is not this
/// run's own, only an entry this run wrote or recovered is removed.
pub fn discard(dir: &Path, uid: u64) {
    #[cfg(not(target_arch = "wasm32"))]
    if validate_recovery_dir(dir).is_err() {
        return;
    }
    let Ok(access) = access(dir, false) else { return };
    if !access.owns(uid) {
        return;
    }
    let (d, m) = files(dir, uid);
    let _ = std::fs::remove_file(d);
    let _ = std::fs::remove_file(m);
    access.forget(uid);
}

/// Consume this document's checkpoint provenance after Save/Close/Revert. A default-directory
/// uid may also be another live recovered document's retained source after a failed republish.
pub(crate) fn discard_document(s: &mut Session, index: usize, default_dir: Option<&Path>) {
    let Some(doc) = s.docs.get_mut(index) else { return };
    let uid = doc.uid;
    // A completed Save/Close/Revert consumes the provenance, even when removal fails. A stale
    // source uid must never become a future claim on another document's checkpoint.
    let mut entries = std::mem::take(&mut doc.recovery_entries);
    if let Some(dir) = default_dir
        && !entries.iter().any(|(entry_dir, entry_uid)| *entry_uid == uid && same_directory(entry_dir, dir))
    {
        entries.push((dir.to_path_buf(), uid));
    }
    for (dir, uid) in entries {
        // Session ownership governs deletion as well as writes: never consume another live
        // document's only source merely because its old uid matches this unrelated document.
        if !s.docs.iter().enumerate().any(|(other_index, owner)| {
            other_index != index && owner.recovery_entries.iter().any(|(entry_dir, entry_uid)| *entry_uid == uid && same_directory(entry_dir, &dir))
        }) {
            discard(&dir, uid);
        }
    }
}

const MAX_RECOVERY_ENTRIES: usize = 16;

fn same_directory(a: &Path, b: &Path) -> bool {
    a == b || std::fs::canonicalize(a).ok().zip(std::fs::canonicalize(b).ok()).is_some_and(|(a, b)| a == b)
}

fn has_checkpoint(doc: &DocState, dir: &Path) -> bool {
    doc.recovery_entries.iter().any(|(entry_dir, uid)| *uid == doc.uid && same_directory(entry_dir, dir))
}

fn protect_retained_sources(s: &Session, dir: &Path) -> Result<()> {
    for document in &s.docs {
        if let Some(owner) = s.docs.iter().find(|owner| {
            owner.uid != document.uid && owner.recovery_entries.iter().any(|(entry_dir, uid)| *uid == document.uid && same_directory(entry_dir, dir))
        }) {
            return Err(EngineError::Other(format!("save or close recovered document {} before reusing its recovery folder", owner.title())));
        }
    }
    Ok(())
}

fn prepare_save(s: &Session, dir: &Path) -> Result<Access> {
    // Check before any write or deletion: a failed republish can retain a source whose uid
    // coincides with an unrelated live document in a manually configured recovery directory.
    protect_retained_sources(s, dir)?;
    let created = secure_recovery_dir(dir)?;
    let access = access(dir, created)?;
    for document in s.docs.iter().filter(|d| d.is_dirty()) {
        access.check_free(dir, document.uid)?;
    }
    Ok(access)
}

/// Write recovery entries for dirty documents and drop clean checkpoints. Returns how many
/// were written. Command callers use the tracked variant without configuring automatic recovery.
pub fn save(s: &Session, dir: &Path) -> Result<usize> {
    let access = prepare_save(s, dir)?;
    let mut n = 0;
    for d in &s.docs {
        if !d.is_dirty() {
            discard(dir, d.uid);
            continue;
        }
        write_entry(&access, dir, d)?;
        n += 1;
    }
    Ok(n)
}

/// Command-path checkpointing also records explicitly selected directories, without configuring
/// automatic recovery. The immutable public helper remains available for fixtures and callers.
pub(crate) fn save_tracked(s: &mut Session, dir: &Path) -> Result<usize> {
    // Reject the complete batch before writing rather than silently losing old ownership records.
    for document in &s.docs {
        if document.is_dirty() && !has_checkpoint(document, dir) && document.recovery_entries.len() >= MAX_RECOVERY_ENTRIES {
            return Err(EngineError::Other(
                "too many recovery checkpoints; save or close the document before choosing another recovery folder".into(),
            ));
        }
    }
    let access = prepare_save(s, dir)?;
    let mut n = 0;
    for index in 0..s.docs.len() {
        if s.docs.get(index).is_some_and(|document| !document.is_dirty()) {
            discard_document(s, index, Some(dir));
            continue;
        }
        let Some(document) = s.docs.get_mut(index) else { continue };
        let (doc, meta) = files(dir, document.uid);
        let bytes = designcraft_format::save(document.preview_stash.as_deref().unwrap_or(document.doc.as_ref()))
            .map_err(|e| EngineError::Other(e.to_string()))?;
        // An entry this run did not write is never replaced, even one made since it checked.
        let replace = access.owns(document.uid);
        publish_recovery_file(&doc, &bytes, replace)?;
        access.record(document.uid);
        // The primary is useful even if optional metadata fails next. Track it immediately.
        if !has_checkpoint(document, dir) {
            document.recovery_entries.push((dir.to_path_buf(), document.uid));
        }
        let metadata = json!({"path": document.path, "title": document.title(), "saved": designcraft_doc::vars::now()});
        publish_recovery_file(&meta, metadata.to_string().as_bytes(), replace)?;
        n += 1;
    }
    Ok(n)
}

/// Publish a document's recovery entry before its optional metadata.
fn write_entry(access: &Access, dir: &Path, d: &DocState) -> Result<()> {
    let (doc, meta) = files(dir, d.uid);
    let bytes = designcraft_format::save(d.preview_stash.as_deref().unwrap_or(d.doc.as_ref())).map_err(|e| EngineError::Other(e.to_string()))?;
    let replace = access.owns(d.uid);
    publish_recovery_file(&doc, &bytes, replace)?;
    access.record(d.uid);
    let metadata = json!({"path": d.path, "title": d.title(), "saved": designcraft_doc::vars::now()});
    publish_recovery_file(&meta, metadata.to_string().as_bytes(), replace)
}

#[cfg(not(target_arch = "wasm32"))]
fn secure_recovery_dir(dir: &Path) -> Result<bool> {
    match std::fs::symlink_metadata(dir) {
        Ok(_) => return validate_recovery_dir(dir).map(|()| false),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
        Err(e) => return Err(EngineError::Other(format!("{}: {e}", dir.display()))),
    }

    // Create parent directories privately without changing existing caller-owned permissions.
    // Create the leaf separately so a creation race can never be mistaken for our own folder.
    #[cfg(unix)]
    let create_result = {
        use std::os::unix::fs::DirBuilderExt;
        if let Some(parent) = dir.parent().filter(|p| !p.as_os_str().is_empty()) {
            std::fs::DirBuilder::new()
                .recursive(true)
                .mode(0o700)
                .create(parent)
                .map_err(|e| EngineError::Other(format!("{}: {e}", dir.display())))?;
        }
        std::fs::DirBuilder::new().mode(0o700).create(dir)
    };
    #[cfg(not(unix))]
    let create_result = {
        if let Some(parent) = dir.parent().filter(|p| !p.as_os_str().is_empty()) {
            std::fs::create_dir_all(parent).map_err(|e| EngineError::Other(format!("{}: {e}", dir.display())))?;
        }
        std::fs::create_dir(dir)
    };
    let created = match create_result {
        Ok(()) => true,
        Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => false,
        Err(e) => return Err(EngineError::Other(format!("{}: {e}", dir.display()))),
    };
    validate_recovery_dir(dir)?;
    Ok(created)
}

#[cfg(target_arch = "wasm32")]
fn secure_recovery_dir(_dir: &Path) -> Result<bool> {
    Err(EngineError::Other("recovery files are unavailable on the web".into()))
}

#[cfg(not(target_arch = "wasm32"))]
fn publish_recovery_file(path: &Path, bytes: &[u8], replace: bool) -> Result<()> {
    let Some(dir) = path.parent() else {
        return Err(EngineError::Other(format!("{} has no parent folder", path.display())));
    };
    validate_recovery_dir(dir)?;
    let mut tmp = tempfile::Builder::new()
        .prefix(".designcraft-recovery-")
        .suffix(".tmp")
        .tempfile_in(dir)
        .map_err(|e| EngineError::Other(format!("{}: {e}", path.display())))?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        tmp.as_file().set_permissions(std::fs::Permissions::from_mode(0o600)).map_err(|e| EngineError::Other(format!("{}: {e}", path.display())))?;
    }
    tmp.as_file_mut().write_all(bytes).map_err(|e| EngineError::Other(format!("{}: {e}", path.display())))?;
    tmp.as_file_mut().sync_all().map_err(|e| EngineError::Other(format!("{}: {e}", path.display())))?;
    // Validate the non-followed leaf again immediately before renaming the randomized temporary
    // file to its fixed sibling destination.
    validate_recovery_dir(dir)?;
    let published = if replace { tmp.persist(path) } else { tmp.persist_noclobber(path) };
    published.map_err(|e| EngineError::Other(format!("{}: {}", path.display(), e.error)))?;
    sync_recovery_dir(dir)?;
    Ok(())
}

#[cfg(not(target_arch = "wasm32"))]
fn validate_recovery_dir(dir: &Path) -> Result<()> {
    let metadata = std::fs::symlink_metadata(dir).map_err(|e| EngineError::Other(format!("{}: {e}", dir.display())))?;
    if is_link_like(&metadata) {
        return Err(EngineError::Other(format!("{} must not be a symbolic link or reparse point", dir.display())));
    }
    if !metadata.file_type().is_dir() {
        return Err(EngineError::Other(format!("{} is not a directory", dir.display())));
    }
    Ok(())
}

#[cfg(all(not(target_arch = "wasm32"), windows))]
fn is_link_like(metadata: &std::fs::Metadata) -> bool {
    use std::os::windows::fs::MetadataExt;

    // This catches all reparse-point leaves, including junctions, rather than relying solely on
    // FileType::is_symlink().
    const FILE_ATTRIBUTE_REPARSE_POINT: u32 = 0x400;
    metadata.file_attributes() & FILE_ATTRIBUTE_REPARSE_POINT != 0
}

#[cfg(all(not(target_arch = "wasm32"), not(windows)))]
fn is_link_like(metadata: &std::fs::Metadata) -> bool {
    metadata.file_type().is_symlink()
}

#[cfg(not(target_arch = "wasm32"))]
fn sync_recovery_dir(dir: &Path) -> Result<()> {
    validate_recovery_dir(dir)?;

    #[cfg(unix)]
    {
        let directory = std::fs::File::open(dir).map_err(|e| EngineError::Other(format!("{}: {e}", dir.display())))?;
        directory.sync_all().map_err(|e| EngineError::Other(format!("{}: {e}", dir.display())))?;
    }

    #[cfg(not(unix))]
    {
        // Rust does not expose a portable durable-directory handle. Try where the platform
        // supports opening directories, but do not turn a fully persisted file into a reported
        // failure where it does not.
        if let Ok(directory) = std::fs::File::open(dir) {
            let _ = directory.sync_all();
        }
    }

    Ok(())
}

#[cfg(target_arch = "wasm32")]
fn publish_recovery_file(_path: &Path, _bytes: &[u8], _replace: bool) -> Result<()> {
    Err(EngineError::Other("recovery files are unavailable on the web".into()))
}

/// Canonical keys are only for alias comparison; operational paths stay unchanged so run
/// ownership checks still find the directory registered by the native profile.
fn directory_key(dir: &Path) -> Result<PathBuf> {
    std::fs::canonicalize(dir).map_err(|e| EngineError::Other(format!("{}: {e}", dir.display())))
}

/// Include metadata-only entries as occupied, so publication cannot attach a new document to
/// somebody else's orphan metadata. No filename-derived maximum changes the uid allocator.
fn occupied_uids(dir: &Path) -> Result<std::collections::HashSet<u64>> {
    let entries = std::fs::read_dir(dir).map_err(|e| EngineError::Other(format!("{}: {e}", dir.display())))?;
    let mut ids = std::collections::HashSet::new();
    for entry in entries {
        let entry = entry.map_err(|e| EngineError::Other(format!("{}: {e}", dir.display())))?;
        let path = entry.path();
        if matches!(path.extension().and_then(|s| s.to_str()), Some("json" | "designcraft"))
            && let Some(uid) = path.file_stem().and_then(|s| s.to_str()).and_then(|s| s.parse().ok())
        {
            ids.insert(uid);
        }
    }
    Ok(ids)
}

fn reserve_uid(state: &mut DocState, occupied: &mut std::collections::HashSet<u64>) -> Result<()> {
    // At most occupied.len() consecutive candidates can collide. The extra attempt must be free.
    for _ in 0..=occupied.len() {
        if occupied.insert(state.uid) {
            return Ok(());
        }
        state.uid = crate::NEXT_UID.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    }
    Err(EngineError::Other("no free recovery document uid".into()))
}

/// Recovery entries in `dir`: (uid, metadata).
pub fn list(dir: &Path) -> Vec<(u64, Value)> {
    let mut out = Vec::new();
    #[cfg(not(target_arch = "wasm32"))]
    if validate_recovery_dir(dir).is_err() || relock(dir, false).is_err() {
        return out;
    }
    let Ok(rd) = std::fs::read_dir(dir) else { return out };
    for e in rd.flatten() {
        let p = e.path();
        if p.extension().and_then(|x| x.to_str()) != Some("designcraft") {
            continue;
        }
        let Some(uid) = p.file_stem().and_then(|s| s.to_str()).and_then(|s| s.parse::<u64>().ok()) else { continue };
        // Metadata is a file on disk: anything but an object (corrupt, hand-edited) counts as none.
        let meta = std::fs::read_to_string(p.with_extension("json"))
            .ok()
            .and_then(|t| serde_json::from_str::<Value>(&t).ok())
            .filter(Value::is_object)
            .unwrap_or(json!({}));
        out.push((uid, meta));
    }
    out.sort_by_key(|e| e.0);
    out
}

/// Reopen recovery entries as unsaved documents. Publish each recovered document under its new
/// uid before removing the original, so another crash before the next timer preserves the edits.
/// Entries belonging to documents already open in this session's recovery folder are left alone.
/// Returns the opened documents' indices.
pub fn open(s: &mut Session, dir: &Path) -> Result<Vec<usize>> {
    open_with_publisher(s, dir, |path, bytes| publish_recovery_file(path, bytes, false))
}

fn open_with_publisher(s: &mut Session, dir: &Path, mut publish: impl FnMut(&Path, &[u8]) -> Result<()>) -> Result<Vec<usize>> {
    #[cfg(not(target_arch = "wasm32"))]
    validate_recovery_dir(dir)?;
    let source = access(dir, false)?;
    let destination = s.recovery_dir.clone().unwrap_or_else(|| dir.to_path_buf());
    let created = secure_recovery_dir(&destination)?;
    let target = access(&destination, created)?;
    let source_key = source.key.clone();
    let destination_key = target.key.clone();
    let mut opened = Vec::new();
    // For this run's own folder, first bring in what runs that ended left in theirs.
    #[cfg(not(target_arch = "wasm32"))]
    take_over(dir);
    let entries = list(dir);
    // Reserve every source entry before opening the first one: publication must never overwrite
    // a source still waiting in this list, even after a fresh process restarts its allocator at 1.
    let mut occupied = occupied_uids(dir)?;
    if source_key != destination_key {
        occupied.extend(occupied_uids(&destination)?);
    }
    occupied.extend(s.docs.iter().map(|d| d.uid));
    for (uid, meta) in entries {
        if s.docs.iter().any(|d| {
            d.recovery_entries.iter().any(|(entry_dir, entry_uid)| *entry_uid == uid && directory_key(entry_dir).is_ok_and(|key| key == source_key))
        }) {
            continue;
        }
        #[cfg(not(target_arch = "wasm32"))]
        validate_recovery_dir(dir)?;
        let (doc, _) = files(dir, uid);
        let Ok(bytes) = std::fs::read(&doc) else { continue };
        let mut d = match crate::cmd::from_bytes(&bytes) {
            Ok(d) => d,
            Err(_) => continue,
        };
        let path = meta.get("path").and_then(Value::as_str).map(str::to_string);
        // Skipped font files are logged; the document opens without them.
        let fonts = path.as_deref().and_then(|p| crate::cmd::load_document_fonts(&mut d, p).0);
        let mut st = DocState::new(d, path);
        reserve_uid(&mut st, &mut occupied)?;
        st.recovery_entries.push((dir.to_path_buf(), uid));
        // Recovered, the entry is this run's: Save and Close of its document may remove it.
        source.record(uid);
        st.fonts = fonts;
        // Unsaved: the copy on disk (if any) is older than what was recovered.
        st.saved_doc = std::sync::Arc::new((*st.doc).clone());
        st.revision += 1;
        let index = s.add_document(st);
        opened.push(index);
        // New uids are published without replacement, and the original is consumed only after
        // both files succeed. Track a published primary even if metadata fails, so Save/Close can
        // clean up every snapshot this document actually owns rather than an unrelated same uid.
        if let Some(state) = s.docs.get_mut(index) {
            let (document, metadata) = files(&destination, state.uid);
            let published = designcraft_format::save(state.doc.as_ref())
                .map_err(|e| EngineError::Other(e.to_string()))
                .and_then(|bytes| publish(&document, &bytes));
            match published {
                Ok(()) => {
                    target.record(state.uid);
                    state.recovery_entries.push((destination.clone(), state.uid));
                    let meta = json!({"path": state.path, "title": state.title(), "saved": designcraft_doc::vars::now()});
                    match publish(&metadata, meta.to_string().as_bytes()) {
                        Ok(()) => {
                            discard(dir, uid);
                            state.recovery_entries = vec![(destination.clone(), state.uid)];
                        }
                        Err(e) => log::warn!("recovery entry {uid} kept: {e}"),
                    }
                }
                Err(e) => log::warn!("recovery entry {uid} kept: {e}"),
            }
        }
    }
    Ok(opened)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn dirty_session() -> Session {
        let mut session = Session::new();
        session.execute("file.new", &json!({})).unwrap();
        session.execute("frame.create", &json!({"rect": [10, 10, 100, 100]})).unwrap();
        session
    }

    #[test]
    fn recovers_unsaved_documents() {
        let dir = std::env::temp_dir().join(format!("dc-recovery-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let mut s = Session::new();
        s.execute("file.new", &json!({})).unwrap();
        s.execute("file.new", &json!({})).unwrap();
        // Only the edited document is written.
        s.execute("frame.create", &json!({"rect": [10, 10, 100, 100]})).unwrap();
        assert_eq!(save(&s, &dir).unwrap(), 1);
        assert_eq!(list(&dir).len(), 1);
        // "Crash": a new session reopens it, unsaved, with the item.
        let mut s2 = Session::new();
        let opened = open(&mut s2, &dir).unwrap();
        assert_eq!(opened.len(), 1);
        let st = s2.doc().unwrap();
        assert!(st.is_dirty());
        assert_eq!(st.doc.spreads[0].items.len(), 1);
        assert_eq!(list(&dir).iter().map(|e| e.0).collect::<Vec<_>>(), [st.uid], "the entry follows its reopened document");
        discard(&dir, st.uid);
        // Saving a document drops its entry.
        s.recovery_dir = Some(dir.clone());
        assert_eq!(save(&s, &dir).unwrap(), 1);
        let path = dir.join("saved.designcraft");
        s.execute("file.saveAs", &json!({"path": path.to_string_lossy()})).unwrap();
        assert!(list(&dir).is_empty());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_recovered_document_survives_a_second_crash_before_autosave() {
        let dir = tempfile::tempdir().unwrap();
        let original = unsaved("edits preserved after a second crash");
        assert_eq!(save(&original, dir.path()).unwrap(), 1);
        let mut reopened = Session::new();
        assert_eq!(open(&mut reopened, dir.path()).unwrap().len(), 1);
        drop(reopened);
        let mut recovered_again = Session::new();
        assert_eq!(open(&mut recovered_again, dir.path()).unwrap().len(), 1);
        assert_eq!(texts(&recovered_again), ["edits preserved after a second crash"]);
        assert!(recovered_again.documents().iter().all(DocState::is_dirty));
        assert_eq!(list(dir.path()).len(), 1);
    }

    #[test]
    fn opening_recovery_again_does_not_duplicate_live_documents() {
        let dir = tempfile::tempdir().unwrap();
        assert_eq!(save(&unsaved("one recovered document"), dir.path()).unwrap(), 1);
        let mut reopened = Session::new();
        reopened.recovery_dir = Some(dir.path().to_path_buf());
        assert_eq!(open(&mut reopened, dir.path()).unwrap().len(), 1);
        assert!(open(&mut reopened, dir.path()).unwrap().is_empty());
        assert_eq!(texts(&reopened), ["one recovered document"]);
        assert_eq!(list(dir.path()).len(), 1);
    }

    /// Directory collisions fail even for privileged runners. Creating the blocker in the
    /// publisher occurs after UID reservation, which would otherwise skip an occupied path.
    fn open_with_failed_republication(s: &mut Session, dir: &Path) -> Result<Vec<usize>> {
        let mut attempts = 0;
        let opened = open_with_publisher(s, dir, |path, bytes| {
            assert_eq!(path.extension().and_then(|extension| extension.to_str()), Some("designcraft"));
            std::fs::create_dir(path).unwrap();
            let result = publish_recovery_file(path, bytes, false);
            std::fs::remove_dir(path).unwrap();
            assert!(result.is_err(), "a directory must obstruct actual recovery publication");
            attempts += 1;
            result
        });
        assert_eq!(attempts, 1, "the fixture must exercise the actual failed publication");
        opened
    }

    #[test]
    fn a_failed_recovery_rewrite_preserves_the_original() {
        let dir = tempfile::tempdir().unwrap();
        assert_eq!(save(&unsaved("keep the original"), dir.path()).unwrap(), 1);
        let entries = list(dir.path());
        let original = files(dir.path(), entries[0].0).0;
        let bytes = std::fs::read(&original).unwrap();
        let mut reopened = Session::new();
        let result = open_with_failed_republication(&mut reopened, dir.path());
        assert_eq!(result.unwrap().len(), 1);
        assert_eq!(texts(&reopened), ["keep the original"]);
        assert_eq!(std::fs::read(original).unwrap(), bytes);
        assert_eq!(list(dir.path()).len(), 1);
        // Its source uid differs from the reopened uid, but explicit Close still discards it.
        assert!(open(&mut reopened, dir.path()).unwrap().is_empty());
        reopened.close_document(0);
        assert!(list(dir.path()).is_empty());
    }

    #[cfg(unix)]
    #[test]
    fn recovery_data_is_private_to_the_user() {
        use std::os::unix::fs::PermissionsExt;

        let root = tempfile::tempdir().unwrap();
        let dir = root.path().join("recovery");
        let s = dirty_session();
        assert_eq!(save(&s, &dir).unwrap(), 1);

        assert_eq!(std::fs::metadata(&dir).unwrap().permissions().mode() & 0o777, 0o700);
        let entries: Vec<_> = std::fs::read_dir(&dir).unwrap().collect();
        assert_eq!(entries.len(), 2);
        for entry in entries {
            let entry = entry.unwrap();
            assert_eq!(entry.metadata().unwrap().permissions().mode() & 0o777, 0o600);
        }
    }

    #[test]
    fn existing_recovery_directory_is_used() {
        let root = tempfile::tempdir().unwrap();
        let dir = root.path().join("recovery");
        std::fs::create_dir(&dir).unwrap();

        assert_eq!(save(&dirty_session(), &dir).unwrap(), 1);
        assert_eq!(list(&dir).len(), 1);
        assert!(std::fs::symlink_metadata(&dir).unwrap().file_type().is_dir());
    }

    #[test]
    fn non_directory_recovery_path_is_rejected() {
        let root = tempfile::tempdir().unwrap();
        let path = root.path().join("recovery");
        std::fs::write(&path, b"not a directory").unwrap();

        assert!(save(&dirty_session(), &path).is_err());
        assert_eq!(std::fs::read(&path).unwrap(), b"not a directory");
    }

    #[cfg(unix)]
    #[test]
    fn existing_recovery_directory_permissions_are_preserved() {
        use std::os::unix::fs::PermissionsExt;

        let root = tempfile::tempdir().unwrap();
        let dir = root.path().join("recovery");
        std::fs::create_dir(&dir).unwrap();
        std::fs::set_permissions(&dir, std::fs::Permissions::from_mode(0o750)).unwrap();

        assert_eq!(save(&dirty_session(), &dir).unwrap(), 1);
        assert_eq!(std::fs::metadata(&dir).unwrap().permissions().mode() & 0o777, 0o750);
    }

    #[cfg(unix)]
    #[test]
    fn symlink_recovery_directory_is_rejected() {
        use std::os::unix::fs::symlink;

        let root = tempfile::tempdir().unwrap();
        let destination = root.path().join("destination");
        let link = root.path().join("recovery");
        std::fs::create_dir(&destination).unwrap();
        symlink(&destination, &link).unwrap();

        assert!(save(&dirty_session(), &link).is_err());
        assert!(std::fs::read_dir(&destination).unwrap().next().is_none());
        std::fs::write(destination.join("7.designcraft"), b"unchanged").unwrap();
        std::fs::write(destination.join("7.json"), b"{}").unwrap();
        assert!(list(&link).is_empty());
        discard(&link, 7);
        assert_eq!(std::fs::read(destination.join("7.designcraft")).unwrap(), b"unchanged");
        assert!(open(&mut Session::new(), &link).is_err());
        assert!(std::fs::symlink_metadata(&link).unwrap().file_type().is_symlink());
    }

    #[test]
    fn a_run_has_a_locked_folder_of_its_own() {
        let root = tempfile::tempdir().unwrap();
        let dir = root.path().join("recovery");
        let own = run_dir(&dir).unwrap();
        assert_eq!(own.parent(), Some(dir.as_path()));
        assert_eq!(run_dir(&dir).unwrap(), own, "one for the whole run");
        let lock = std::fs::File::open(own.join(LOCK)).unwrap();
        assert!(matches!(lock.try_lock(), Err(std::fs::TryLockError::WouldBlock)), "held while the run lives");
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            assert_eq!(std::fs::metadata(&own).unwrap().permissions().mode() & 0o777, 0o700);
            assert_eq!(std::fs::metadata(own.join(LOCK)).unwrap().permissions().mode() & 0o777, 0o600);
        }
    }

    fn failed_republication_session() -> (Session, tempfile::TempDir, u64) {
        let root = tempfile::tempdir().unwrap();
        let mut session = Session::new();
        session.execute("file.new", &json!({})).unwrap();
        let live_uid = session.docs[0].uid;
        let mut original = unsaved("retained old work");
        original.docs[0].uid = live_uid;
        save(&original, root.path()).unwrap();
        session.recovery_dir = Some(root.path().to_path_buf());
        let recovered = open_with_failed_republication(&mut session, root.path());
        assert_eq!(recovered.unwrap().len(), 1);
        assert_ne!(session.docs[1].uid, live_uid);
        (session, root, live_uid)
    }

    #[test]
    fn retained_source_survives_a_coincident_live_uid_checkpoint_failure() {
        let (mut session, root, live_uid) = failed_republication_session();
        let original = std::fs::read(files(root.path(), live_uid).0).unwrap();
        let recovered_uid = session.docs[1].uid;
        std::fs::create_dir(files(root.path(), recovered_uid).0).unwrap();
        session.execute("file.activate", &json!({"index": 0})).unwrap();
        session.execute("frame.create", &json!({"rect": [0,0,100,100], "content": "text", "text": "other live work"})).unwrap();
        assert!(session.execute("file.recovery.save", &json!({"dir": root.path().to_string_lossy()})).is_err());
        assert_eq!(std::fs::read(files(root.path(), live_uid).0).unwrap(), original);
    }

    #[test]
    fn clean_autosave_preserves_another_documents_retained_source() {
        let (mut session, root, live_uid) = failed_republication_session();
        let checkpoint = files(root.path(), live_uid).0;
        let original = std::fs::read(&checkpoint).unwrap();
        assert!(!session.docs[0].is_dirty());
        assert!(session.docs[1].is_dirty());
        assert!(session.execute("file.recovery.save", &json!({})).is_err());
        assert_eq!(std::fs::read(checkpoint).unwrap(), original);
    }

    #[test]
    fn closing_a_coincident_document_preserves_another_documents_retained_source() {
        let (mut session, root, live_uid) = failed_republication_session();
        let checkpoint = files(root.path(), live_uid).0;
        let original = std::fs::read(&checkpoint).unwrap();
        session.execute("file.close", &json!({"index": 0})).unwrap();
        assert_eq!(std::fs::read(checkpoint).unwrap(), original);
    }

    #[test]
    fn saving_a_coincident_document_preserves_another_documents_retained_source() {
        let (mut session, root, live_uid) = failed_republication_session();
        let checkpoint = files(root.path(), live_uid).0;
        let original = std::fs::read(&checkpoint).unwrap();
        session.execute("file.activate", &json!({"index": 0})).unwrap();
        session.execute("file.saveAs", &json!({"path": root.path().join("saved-other.designcraft").to_string_lossy()})).unwrap();
        assert_eq!(std::fs::read(checkpoint).unwrap(), original);
    }

    #[test]
    fn reverting_a_coincident_document_preserves_another_documents_retained_source() {
        let (mut session, root, live_uid) = failed_republication_session();
        let checkpoint = files(root.path(), live_uid).0;
        let original = std::fs::read(&checkpoint).unwrap();
        let saved = root.path().join("saved-other.designcraft");
        std::fs::write(&saved, designcraft_format::save(&session.docs[0].doc).unwrap()).unwrap();
        session.docs[0].path = Some(saved.to_string_lossy().into_owned());
        session.execute("file.activate", &json!({"index": 0})).unwrap();
        session.execute("frame.create", &json!({"rect": [0,0,100,100]})).unwrap();
        session.execute("file.revert", &json!({})).unwrap();
        assert_eq!(std::fs::read(checkpoint).unwrap(), original);
    }

    #[test]
    fn retained_source_provenance_is_consumed_before_its_uid_is_reused() {
        let (mut session, root, live_uid) = failed_republication_session();
        session.execute("file.saveAs", &json!({"path": root.path().join("saved.designcraft").to_string_lossy()})).unwrap();
        session.execute("file.activate", &json!({"index": 0})).unwrap();
        session.execute("frame.create", &json!({"rect": [0,0,100,100], "content": "text", "text": "other live work"})).unwrap();
        session.execute("file.recovery.save", &json!({"dir": root.path().to_string_lossy()})).unwrap();
        let checkpoint = files(root.path(), live_uid).0;
        let expected = std::fs::read(&checkpoint).unwrap();
        session.execute("file.close", &json!({"index": 1})).unwrap();
        assert_eq!(
            std::fs::read(checkpoint).unwrap(),
            expected,
            "closing the saved recovered document must not delete another document's checkpoint"
        );
    }

    #[test]
    fn explicit_checkpoint_tracking_is_bounded_and_deduplicated_before_writes() {
        let root = tempfile::tempdir().unwrap();
        let mut session = unsaved("bounded checkpoints");
        for n in 0..MAX_RECOVERY_ENTRIES {
            session.execute("file.recovery.save", &json!({"dir": root.path().join(n.to_string()).to_string_lossy()})).unwrap();
        }
        let excess = root.path().join("excess");
        assert!(session.execute("file.recovery.save", &json!({"dir": excess.to_string_lossy()})).is_err());
        assert!(!excess.exists(), "reject before creating or writing the next directory");
        let first_alias = root.path().join("0").join(".");
        session.execute("file.recovery.save", &json!({"dir": first_alias.to_string_lossy()})).unwrap();
        for n in 0..MAX_RECOVERY_ENTRIES {
            assert_eq!(persisted_texts(&root.path().join(n.to_string())), ["bounded checkpoints"]);
        }
        assert!(session.recovery_dir.is_none());
        session.execute("file.close", &json!({})).unwrap();
        for n in 0..MAX_RECOVERY_ENTRIES {
            assert!(list(&root.path().join(n.to_string())).is_empty());
        }
    }

    fn restart_fixture() -> tempfile::TempDir {
        let root = tempfile::tempdir().unwrap();
        let mut first = unsaved("restart first");
        first.docs[0].uid = 1;
        save(&first, root.path()).unwrap();
        let mut second = unsaved("restart second");
        second.docs[0].uid = 2;
        save(&second, root.path()).unwrap();
        root
    }

    fn persisted_texts(dir: &Path) -> Vec<String> {
        let mut result = Vec::new();
        for (uid, _) in list(dir) {
            let doc = crate::cmd::from_bytes(&std::fs::read(files(dir, uid).0).unwrap()).unwrap();
            result.extend(doc.stories.values().map(|story| story.text.clone()));
        }
        result.sort();
        result
    }

    #[test]
    #[ignore = "subprocess helper, executed by the restart recovery regression tests"]
    fn restart_recovery_child() {
        let source = PathBuf::from(std::env::var("DESIGNCRAFT_TEST_RECOVERY_SOURCE").unwrap());
        let mode = std::env::var("DESIGNCRAFT_TEST_RECOVERY_MODE").unwrap();
        if mode == "occupied_destination" {
            let owned_root = tempfile::tempdir().unwrap();
            let destination = run_dir(owned_root.path()).unwrap();
            let mut existing = unsaved("already in destination");
            existing.docs[0].uid = 3;
            save(&existing, &destination).unwrap();
            let (existing_doc, existing_meta) = files(&destination, 3);
            let doc_bytes = std::fs::read(&existing_doc).unwrap();
            let meta_bytes = std::fs::read(&existing_meta).unwrap();
            let mut session = Session::new();
            session.recovery_dir = Some(destination.clone());
            let opened = open(&mut session, &source).unwrap();
            assert_eq!(opened.len(), 2);
            assert_eq!(texts(&session), ["restart first", "restart second"]);
            assert!(session.docs.iter().all(|document| document.uid != 3));
            assert_eq!(persisted_texts(&destination), ["already in destination", "restart first", "restart second"]);
            assert!(list(&source).is_empty());
            session.execute("file.saveAs", &json!({"path": source.join("saved.designcraft").to_string_lossy()})).unwrap();
            session.execute("file.close", &json!({"index": opened[0]})).unwrap();
            assert_eq!(list(&destination).len(), 1);
            assert_eq!(std::fs::read(existing_doc).unwrap(), doc_bytes);
            assert_eq!(std::fs::read(existing_meta).unwrap(), meta_bytes);
            return;
        }
        let mut session = Session::new();
        if mode == "advance" || mode == "coincident_live" {
            session.execute("file.new", &json!({})).unwrap();
            assert_eq!(session.docs[0].uid, 1);
        }
        let owned_root;
        let destination = match mode.as_str() {
            "cross_dir" => {
                owned_root = tempfile::tempdir().unwrap();
                let own = run_dir(owned_root.path()).unwrap();
                session.recovery_dir = Some(own.clone());
                own
            }
            "alias" => {
                let alias = source.join(".");
                session.recovery_dir = Some(alias.clone());
                alias
            }
            "coincident_live" => {
                session.recovery_dir = Some(source.clone());
                source.clone()
            }
            _ => source.clone(),
        };
        let opened = open(&mut session, &source).unwrap();
        assert_eq!(opened.len(), 2);
        assert_eq!(texts(&session), ["restart first", "restart second"]);
        assert_eq!(persisted_texts(&destination), ["restart first", "restart second"]);
        assert!(session.docs.iter().all(|d| d.uid > 0 && d.uid < u64::MAX));
        if matches!(mode.as_str(), "cross_dir" | "manual" | "alias" | "revert") {
            let was_disabled = session.recovery_dir.is_none();
            if mode == "cross_dir" {
                assert!(list(&source).is_empty());
            }
            // A copy does not consume recovery. A successful Save and a Close do.
            session.execute("file.saveACopy", &json!({"path": source.join("copy.designcraft").to_string_lossy()})).unwrap();
            assert_eq!(list(&destination).len(), 2);
            session.execute("file.saveAs", &json!({"path": source.join("saved.designcraft").to_string_lossy()})).unwrap();
            assert_eq!(list(&destination).len(), 1);
            if mode == "revert" {
                session.execute("frame.create", &json!({"rect": [0, 0, 100, 100], "content": "text", "text": "after saving"})).unwrap();
                session.execute("file.recovery.save", &json!({"dir": destination.to_string_lossy()})).unwrap();
                assert_eq!(list(&destination).len(), 2);
                session.execute("file.revert", &json!({})).unwrap();
                assert_eq!(list(&destination).len(), 1);
            }
            session.execute("file.close", &json!({"index": opened[0]})).unwrap();
            assert!(list(&destination).is_empty());
            assert!(list(&source).is_empty());
            assert_eq!(session.recovery_dir.is_none(), was_disabled, "manual recovery must not configure autosave");
        }
    }

    fn run_restart_case(mode: &str) {
        let source = restart_fixture();
        if mode == "max_uid" {
            let (doc, meta) = files(source.path(), 2);
            let (max_doc, max_meta) = files(source.path(), u64::MAX);
            std::fs::rename(doc, max_doc).unwrap();
            std::fs::rename(meta, max_meta).unwrap();
        }
        let output = std::process::Command::new(std::env::current_exe().unwrap())
            .args(["--exact", "recovery::tests::restart_recovery_child", "--ignored", "--nocapture"])
            .env("DESIGNCRAFT_TEST_RECOVERY_SOURCE", source.path())
            .env("DESIGNCRAFT_TEST_RECOVERY_MODE", mode)
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "fresh child failed: {}{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
        let child_stdout = String::from_utf8_lossy(&output.stdout);
        assert!(child_stdout.contains("test result: ok. 1 passed; 0 failed;"), "the fresh-process helper did not run exactly once: {child_stdout}");
    }

    #[test]
    fn restart_uid_collision_cannot_delete_the_new_snapshot() {
        run_restart_case("fresh");
    }
    #[test]
    fn restart_uid_collision_cannot_overwrite_an_unopened_entry() {
        run_restart_case("advance");
    }
    #[test]
    fn cross_directory_recovery_uses_the_configured_destination() {
        run_restart_case("cross_dir");
    }
    #[test]
    fn cross_directory_recovery_preserves_occupied_destination_entries() {
        run_restart_case("occupied_destination");
    }
    #[test]
    fn manual_recovery_save_and_close_clean_up_without_enabling_autosave() {
        run_restart_case("manual");
    }
    #[test]
    fn recovery_directory_aliases_do_not_delete_new_snapshots() {
        run_restart_case("alias");
    }
    #[test]
    fn a_maximum_source_uid_does_not_advance_or_overflow_the_allocator() {
        run_restart_case("max_uid");
    }
    #[test]
    fn a_coincident_live_uid_is_not_recovered_entry_provenance() {
        run_restart_case("coincident_live");
    }
    #[test]
    fn manual_recovery_revert_removes_its_previous_checkpoint() {
        run_restart_case("revert");
    }

    /// The recovery folder was cleared while the app runs: the next save makes the run's folder
    /// again, and its lock with it.
    #[cfg(unix)]
    #[test]
    fn a_removed_run_folder_is_made_again_with_its_lock() {
        let root = tempfile::tempdir().unwrap();
        let own = run_dir(root.path()).unwrap();
        std::fs::remove_dir_all(&own).unwrap();
        assert_eq!(save(&dirty_session(), &own).unwrap(), 1);
        let lock = std::fs::File::open(own.join(LOCK)).unwrap();
        assert!(matches!(lock.try_lock(), Err(std::fs::TryLockError::WouldBlock)));
    }

    /// Replacing the name of a live run's lock must not authorize writes under the old handle.
    #[cfg(unix)]
    #[test]
    fn save_rejects_a_run_whose_lock_was_replaced() {
        let root = tempfile::tempdir().unwrap();
        let own = run_dir(root.path()).unwrap();
        std::fs::remove_file(own.join(LOCK)).unwrap();
        let replacement = new_lock(&own).unwrap();
        replacement.try_lock().unwrap();
        assert!(save(&dirty_session(), &own).is_err());
        assert!(list(&own).is_empty(), "another owner's folder must not receive our recovery data");
    }

    #[cfg(windows)]
    #[test]
    fn an_exclusively_locked_windows_recovery_file_validates() {
        let root = tempfile::tempdir().unwrap();
        let lock = new_lock(root.path()).unwrap();
        lock.try_lock().unwrap();
        validate_lock(root.path(), &lock).unwrap();
        let named = std::fs::File::open(root.path().join(LOCK)).unwrap();
        assert!(matches!(named.try_lock(), Err(std::fs::TryLockError::WouldBlock)), "validation must retain the exclusive lock");
    }

    #[cfg(windows)]
    #[test]
    fn windows_recovery_rejects_a_replacement_with_the_same_marker() {
        let root = tempfile::tempdir().unwrap();
        let lock = new_lock(root.path()).unwrap();
        let marker = std::fs::read(root.path().join(LOCK)).unwrap();
        lock.try_lock().unwrap();
        let moved = root.path().join("original-lock");
        std::fs::rename(root.path().join(LOCK), &moved).unwrap();
        std::fs::write(root.path().join(LOCK), marker).unwrap();
        assert!(validate_lock(root.path(), &lock).is_err(), "matching marker bytes do not make another file our lock");
        std::fs::remove_file(root.path().join(LOCK)).unwrap();
        assert!(validate_lock(root.path(), &lock).is_err(), "a missing lock must not validate");
    }

    #[cfg(windows)]
    #[test]
    fn windows_recovery_does_not_write_or_discard_a_replacement_runs_entries() {
        let root = tempfile::tempdir().unwrap();
        let own = run_dir(root.path()).unwrap();
        std::fs::rename(own.join(LOCK), own.join("original-lock")).unwrap();
        let replacement = new_lock(&own).unwrap();
        replacement.try_lock().unwrap();
        let session = dirty_session();
        let (doc, meta) = files(&own, session.docs[0].uid);
        std::fs::write(&doc, b"another owner's document").unwrap();
        std::fs::write(&meta, b"{}").unwrap();
        assert!(save(&session, &own).is_err());
        discard(&own, session.docs[0].uid);
        assert_eq!(std::fs::read(&doc).unwrap(), b"another owner's document");
        assert_eq!(std::fs::read(&meta).unwrap(), b"{}");
    }

    /// A blocked attempt to recreate a run's lock must stop the save before it writes documents.
    #[cfg(unix)]
    #[test]
    fn save_rejects_a_run_whose_lock_cannot_be_recreated() {
        let root = tempfile::tempdir().unwrap();
        let own = run_dir(root.path()).unwrap();
        std::fs::remove_file(own.join(LOCK)).unwrap();
        std::fs::create_dir(own.join(LOCK)).unwrap();
        assert!(save(&dirty_session(), &own).is_err());
        assert!(list(&own).is_empty());
    }

    #[cfg(unix)]
    #[test]
    fn discarding_a_document_does_not_touch_a_replaced_runs_entries() {
        let root = tempfile::tempdir().unwrap();
        let own = run_dir(root.path()).unwrap();
        std::fs::remove_file(own.join(LOCK)).unwrap();
        let replacement = new_lock(&own).unwrap();
        replacement.try_lock().unwrap();
        let (doc, meta) = files(&own, 1);
        std::fs::write(&doc, b"the replacement owner's document").unwrap();
        std::fs::write(&meta, b"{}").unwrap();
        discard(&own, 1);
        assert_eq!(std::fs::read(doc).unwrap(), b"the replacement owner's document");
        assert!(meta.exists());
    }

    #[cfg(unix)]
    #[test]
    fn saving_a_run_rejects_a_symlinked_lock() {
        let root = tempfile::tempdir().unwrap();
        let own = run_dir(root.path()).unwrap();
        std::fs::remove_file(own.join(LOCK)).unwrap();
        let unrelated = root.path().join("unrelated");
        std::fs::write(&unrelated, b"unrelated file").unwrap();
        std::os::unix::fs::symlink(&unrelated, own.join(LOCK)).unwrap();
        assert!(save(&dirty_session(), &own).is_err());
        assert_eq!(std::fs::read(unrelated).unwrap(), b"unrelated file");
    }

    #[cfg(unix)]
    #[test]
    fn a_replaced_run_directory_is_not_claimed_or_overwritten() {
        let root = tempfile::tempdir().unwrap();
        let own = run_dir(root.path()).unwrap();
        let session = dirty_session();
        let uid = session.docs[0].uid;
        std::fs::remove_dir_all(&own).unwrap();
        std::fs::create_dir(&own).unwrap();
        let (doc, _) = files(&own, uid);
        std::fs::write(&doc, b"another owner's document").unwrap();
        assert!(save(&session, &own).is_err());
        assert_eq!(std::fs::read(doc).unwrap(), b"another owner's document");
        assert!(!own.join(LOCK).exists());
    }

    #[cfg(unix)]
    #[test]
    fn a_run_gets_no_folder_in_a_symlinked_recovery_folder() {
        use std::os::unix::fs::symlink;

        let root = tempfile::tempdir().unwrap();
        let destination = root.path().join("destination");
        let link = root.path().join("recovery");
        std::fs::create_dir(&destination).unwrap();
        symlink(&destination, &link).unwrap();

        assert!(run_dir(&link).is_err());
        assert!(std::fs::read_dir(&destination).unwrap().next().is_none());
    }

    fn unsaved(text: &str) -> Session {
        let mut session = Session::new();
        session.execute("file.new", &json!({})).unwrap();
        session.execute("frame.create", &json!({"rect": [10, 10, 100, 100], "content": "text", "text": text})).unwrap();
        session
    }

    fn texts(s: &Session) -> Vec<String> {
        let mut texts: Vec<String> = s.documents().iter().flat_map(|d| d.doc.stories.values().map(|st| st.text.clone())).collect();
        texts.sort();
        texts
    }

    /// What a run that has ended leaves: its folder with its lock, free now, and the entry of
    /// one unsaved document, numbered `uid` as that process numbered it.
    fn run_left(root: &Path, name: &str, text: &str, uid: u64) -> PathBuf {
        let dir = root.join(name);
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join(LOCK), b"").unwrap();
        let mut s = unsaved(text);
        s.docs[0].uid = uid;
        assert_eq!(save(&s, &dir).unwrap(), 1);
        dir
    }

    /// Every process numbers its first document 1: two running apps both wrote `1.designcraft`
    /// into the one recovery folder, and only the document written last was recovered.
    #[test]
    fn documents_of_two_apps_that_ended_are_both_recovered() {
        let root = tempfile::tempdir().unwrap();
        let first = run_left(root.path(), "run-7-0", "first app", 1);
        let second = run_left(root.path(), "run-8-0", "second app", 1);
        let own = run_dir(root.path()).unwrap();
        let mut s = Session::new();
        assert_eq!(open(&mut s, &own).unwrap().len(), 2);
        assert_eq!(texts(&s), ["first app", "second app"]);
        assert!(s.documents().iter().all(DocState::is_dirty));
        assert!(!first.exists() && !second.exists(), "nothing is left of the runs that ended");
    }

    /// An app that started reopened the unsaved documents of one still running, and removed
    /// its entries.
    #[test]
    fn a_running_apps_recovery_data_is_left_alone() {
        let root = tempfile::tempdir().unwrap();
        let running = run_left(root.path(), "run-7-0", "still open", 1);
        let lock = std::fs::File::open(running.join(LOCK)).unwrap();
        lock.try_lock().unwrap();
        let own = run_dir(root.path()).unwrap();
        let mut s = Session::new();
        assert!(open(&mut s, &own).unwrap().is_empty());
        assert_eq!(list(&running).len(), 1);
        // Once it has ended, its document is recovered.
        drop(lock);
        assert_eq!(open(&mut s, &own).unwrap().len(), 1);
        assert_eq!(texts(&s), ["still open"]);
        assert!(!running.exists());
    }

    /// A process id comes round again: the folder a run left under it is not this run's.
    #[test]
    fn a_folder_left_under_this_process_id_is_not_reused() {
        let root = tempfile::tempdir().unwrap();
        let left = run_left(root.path(), &format!("run-{}-0", std::process::id()), "left behind", 1);
        let own = run_dir(root.path()).unwrap();
        assert_ne!(own, left);
        let mut s = Session::new();
        assert_eq!(open(&mut s, &own).unwrap().len(), 1);
        assert_eq!(texts(&s), ["left behind"]);
    }

    /// Entries written straight into the recovery folder, as before runs had folders.
    #[test]
    fn recovery_data_of_an_earlier_version_is_recovered() {
        let root = tempfile::tempdir().unwrap();
        assert_eq!(save(&unsaved("earlier version"), root.path()).unwrap(), 1);
        let own = run_dir(root.path()).unwrap();
        let mut s = Session::new();
        assert_eq!(open(&mut s, &own).unwrap().len(), 1);
        assert_eq!(texts(&s), ["earlier version"]);
        assert!(list(root.path()).is_empty());
    }

    /// An entry that can't be opened stays on disk, in this run's folder now.
    #[test]
    fn an_unreadable_entry_of_a_run_that_ended_is_kept() {
        let root = tempfile::tempdir().unwrap();
        let left = run_left(root.path(), "run-7-0", "readable", 1);
        std::fs::write(left.join("2.designcraft"), b"not a document").unwrap();
        let own = run_dir(root.path()).unwrap();
        let mut s = Session::new();
        assert_eq!(open(&mut s, &own).unwrap().len(), 1);
        assert!(list(&own).iter().any(|e| std::fs::read(files(&own, e.0).0).unwrap() == b"not a document"));
        assert!(!left.exists());
    }

    /// Only a folder named as a run's, with a lock that is free, is a run that ended.
    #[test]
    fn what_is_no_run_that_ended_is_passed_over() {
        let root = tempfile::tempdir().unwrap();
        std::fs::write(root.path().join("run-1-0"), b"a file").unwrap();
        // An app that is starting has made its folder and not yet its lock.
        let starting = root.path().join("run-2-0");
        std::fs::create_dir(&starting).unwrap();
        let other = run_left(root.path(), "backup", "not a run", 1);
        #[cfg(unix)]
        let (_elsewhere, linked) = {
            let elsewhere = tempfile::tempdir().unwrap();
            let linked = run_left(elsewhere.path(), "run-3-0", "behind a link", 1);
            std::os::unix::fs::symlink(&linked, root.path().join("run-3-0")).unwrap();
            (elsewhere, linked)
        };
        let own = run_dir(root.path()).unwrap();
        let mut s = Session::new();
        assert!(open(&mut s, &own).unwrap().is_empty());
        assert!(starting.exists());
        assert_eq!(list(&other).len(), 1);
        #[cfg(unix)]
        assert!(list(&linked).len() == 1 && root.path().join("run-3-0").is_symlink());
    }

    /// The metadata goes with its document: a document taken over remembers its file.
    #[test]
    fn a_document_taken_over_remembers_its_file() {
        let root = tempfile::tempdir().unwrap();
        let file = root.path().join("layout.designcraft");
        let recovery = root.path().join("recovery");
        let left = recovery.join("run-7-0");
        std::fs::create_dir_all(&left).unwrap();
        std::fs::write(left.join(LOCK), b"").unwrap();
        let mut a = unsaved("saved once");
        a.execute("file.saveAs", &json!({"path": file.to_string_lossy()})).unwrap();
        a.execute("frame.create", &json!({"rect": [10, 10, 100, 100]})).unwrap();
        assert_eq!(save(&a, &left).unwrap(), 1);
        let own = run_dir(&recovery).unwrap();
        let mut s = Session::new();
        assert_eq!(open(&mut s, &own).unwrap().len(), 1);
        assert_eq!(s.doc().unwrap().path.as_deref(), Some(file.to_string_lossy().as_ref()));
    }

    /// The folder of a run that ended goes with what the app left in it. A file the app did not
    /// write keeps the folder, and is not deleted.
    #[test]
    fn only_what_the_app_left_goes_with_the_folder_of_a_run_that_ended() {
        let root = tempfile::tempdir().unwrap();
        let emptied = run_left(root.path(), "run-7-0", "moved", 1);
        std::fs::write(emptied.join("9.json"), b"{}").unwrap();
        std::fs::write(emptied.join(".designcraft-recovery-abc.tmp"), b"half written").unwrap();
        let kept = run_left(root.path(), "run-8-0", "moved too", 1);
        std::fs::write(kept.join("copy.designcraft"), b"a copy someone put here").unwrap();
        let own = run_dir(root.path()).unwrap();
        let mut s = Session::new();
        assert_eq!(open(&mut s, &own).unwrap().len(), 2);
        assert!(!emptied.exists());
        assert_eq!(std::fs::read(kept.join("copy.designcraft")).unwrap(), b"a copy someone put here");
        assert!(kept.join(LOCK).exists(), "still the folder of a run that ended");
    }

    /// A folder that is there is another app's or was left behind, lock or no lock: a run's own
    /// is one it made itself.
    #[test]
    fn a_folder_that_is_there_is_not_made_this_runs() {
        let root = tempfile::tempdir().unwrap();
        let left = root.path().join(format!("run-{}-0", std::process::id()));
        std::fs::create_dir(&left).unwrap();
        std::fs::write(left.join("1.designcraft"), b"left with no lock").unwrap();
        let own = run_dir(root.path()).unwrap();
        assert_ne!(own, left);
        assert!(!left.join(LOCK).exists());
        assert_eq!(std::fs::read(left.join("1.designcraft")).unwrap(), b"left with no lock");
    }

    /// Another run's entry under uid 7 in a chosen folder, with a document of this run numbered
    /// 7 too: document uids start at 1 in every process.
    fn chosen_folder_with_another_runs_entry() -> (tempfile::TempDir, PathBuf, PathBuf) {
        let root = tempfile::tempdir().unwrap();
        let (doc, meta) = files(root.path(), 7);
        std::fs::write(&doc, b"another run's document").unwrap();
        std::fs::write(&meta, b"{\"title\": \"theirs\"}").unwrap();
        (root, doc, meta)
    }

    #[test]
    fn a_clean_document_leaves_another_runs_entry_in_a_chosen_folder() {
        let (root, doc, meta) = chosen_folder_with_another_runs_entry();
        let mut s = Session::new();
        s.execute("file.new", &json!({})).unwrap();
        s.docs[0].uid = 7;
        assert!(!s.docs[0].is_dirty());
        s.execute("file.recovery.save", &json!({"dir": root.path().to_string_lossy()})).unwrap();
        s.execute("file.close", &json!({})).unwrap();
        assert_eq!(std::fs::read(doc).unwrap(), b"another run's document");
        assert_eq!(std::fs::read(meta).unwrap(), b"{\"title\": \"theirs\"}");
    }

    #[test]
    fn a_dirty_document_does_not_overwrite_another_runs_entry_in_a_chosen_folder() {
        let (root, doc, meta) = chosen_folder_with_another_runs_entry();
        let mut s = unsaved("this run's work");
        s.docs[0].uid = 7;
        assert!(s.execute("file.recovery.save", &json!({"dir": root.path().to_string_lossy()})).is_err());
        assert!(save(&s, root.path()).is_err());
        assert_eq!(std::fs::read(doc).unwrap(), b"another run's document");
        assert_eq!(std::fs::read(meta).unwrap(), b"{\"title\": \"theirs\"}");
    }

    /// A chosen folder that is a running app's run folder is that app's.
    #[test]
    fn a_folder_another_running_app_holds_is_refused() {
        let root = tempfile::tempdir().unwrap();
        let held = new_lock(root.path()).unwrap();
        held.try_lock().unwrap();
        assert!(save(&dirty_session(), root.path()).is_err());
        assert!(list(root.path()).is_empty());
    }
}

#[cfg(test)]
mod file_tests {
    use super::*;

    /// `file.recovery.list` adds `uid` to each entry's metadata: metadata that wasn't an object
    /// (a corrupt `.json` beside the recovery file) panicked on `m["uid"] = …`.
    #[test]
    fn corrupt_recovery_metadata_is_ignored() {
        let dir = std::env::temp_dir().join(format!("dc-recovery-corrupt-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("7.designcraft"), b"not a document").unwrap();
        std::fs::write(dir.join("7.json"), b"[1, 2]").unwrap();
        let mut s = Session::new();
        s.recovery_dir = Some(dir.clone());
        let r = s.execute("file.recovery.list", &json!({})).unwrap();
        assert_eq!(r[0]["uid"], json!(7));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn revert_and_save_a_copy() {
        let dir = std::env::temp_dir().join(format!("dc-revert-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let (a, copy) = (dir.join("a.designcraft"), dir.join("copy.designcraft"));
        let mut s = Session::new();
        s.execute("file.new", &json!({})).unwrap();
        assert!(s.execute("file.revert", &json!({})).is_err(), "never saved");
        s.execute("file.saveAs", &json!({"path": a.to_string_lossy()})).unwrap();
        s.execute("frame.create", &json!({"rect": [10, 10, 100, 100]})).unwrap();
        // A copy keeps the document unsaved and on its own file.
        s.execute("file.saveACopy", &json!({"path": copy.to_string_lossy()})).unwrap();
        assert!(copy.exists());
        assert!(s.doc().unwrap().is_dirty());
        assert_eq!(s.doc().unwrap().path.as_deref(), Some(a.to_string_lossy().as_ref()));
        // Revert drops the frame.
        s.execute("file.revert", &json!({})).unwrap();
        assert!(s.doc().unwrap().doc.spreads[0].items.is_empty());
        assert!(!s.doc().unwrap().is_dirty());
        let _ = std::fs::remove_dir_all(&dir);
    }
}
