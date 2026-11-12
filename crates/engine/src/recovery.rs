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
        let numbered = path.file_stem().and_then(std::ffi::OsStr::to_str).is_some_and(|s| s.parse::<u64>().is_ok());
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

/// The bytes of the recovery entry file at `path`, which must be a regular file: the folder was
/// checked, and a link planted among its entries would lead the read anywhere.
fn read_entry_file(path: &Path) -> std::io::Result<Vec<u8>> {
    use std::io::Read;
    let refuse = || std::io::Error::other(format!("{}: a recovery entry must be a regular file", path.display()));
    let before = std::fs::symlink_metadata(path)?;
    #[cfg(not(target_arch = "wasm32"))]
    if is_link_like(&before) {
        return Err(refuse());
    }
    if !before.file_type().is_file() {
        return Err(refuse());
    }
    #[cfg(windows)]
    let mut file = {
        use std::os::windows::fs::OpenOptionsExt;
        // A reparse point swapped in after the check is opened as itself, not followed.
        const FILE_FLAG_OPEN_REPARSE_POINT: u32 = 0x0020_0000;
        std::fs::OpenOptions::new().read(true).custom_flags(FILE_FLAG_OPEN_REPARSE_POINT).open(path)?
    };
    #[cfg(not(windows))]
    let mut file = std::fs::File::open(path)?;
    let opened = file.metadata()?;
    #[cfg(windows)]
    if is_link_like(&opened) {
        return Err(refuse());
    }
    if !opened.file_type().is_file() {
        return Err(refuse());
    }
    // The name was checked, then opened: make sure what was opened is what was checked.
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        if opened.dev() != before.dev() || opened.ino() != before.ino() {
            return Err(refuse());
        }
    }
    let mut bytes = Vec::new();
    file.read_to_end(&mut bytes)?;
    Ok(bytes)
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
    // Background writes still queued or running for it, in any folder (one named by
    // `file.recovery.save {dir}` is not in its entries until the write's result is taken): none
    // may publish after this point, and what one already published is removed below.
    #[cfg(not(target_arch = "wasm32"))]
    for dir in worker::cancel_document(uid, s.recovery_owner.0) {
        if !entries.iter().any(|(entry_dir, entry_uid)| *entry_uid == uid && same_directory(entry_dir, &dir)) {
            entries.push((dir, uid));
        }
    }
    for (dir, uid) in entries {
        // Session ownership governs deletion as well as writes: never consume another live
        // document's only source merely because its old uid matches this unrelated document.
        if !s.docs.iter().enumerate().any(|(other_index, owner)| {
            other_index != index && owner.recovery_entries.iter().any(|(entry_dir, entry_uid)| *entry_uid == uid && same_directory(entry_dir, &dir))
        }) {
            // A write of it still running must not publish its snapshot after this point.
            #[cfg(not(target_arch = "wasm32"))]
            worker::cancel(&dir, uid);
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
    // New Document's preview is never written.
    let preview = s.new_document_preview();
    for document in s.docs.iter().filter(|d| d.is_dirty() && preview != Some(d.uid)) {
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
        // New Document's preview is not a document until Create makes it one.
        if !d.is_dirty() || s.new_document_preview() == Some(d.uid) {
            // A background write this session queued of an older state must not publish after
            // this discard, and what one already wrote in another folder is removed too.
            #[cfg(not(target_arch = "wasm32"))]
            for other in worker::cancel_document(d.uid, s.recovery_owner.0) {
                discard(&other, d.uid);
            }
            discard(dir, d.uid);
            continue;
        }
        // The caller asked for this write now: nothing has cancelled it.
        // A background write of an older state must not land over this one.
        #[cfg(not(target_arch = "wasm32"))]
        worker::cancel(dir, d.uid);
        if let Some(e) = write_snapshot(&Snapshot::of(&access, dir, d), || false).error {
            return Err(e);
        }
        n += 1;
    }
    Ok(n)
}

/// Command-path checkpointing also records explicitly selected directories, without configuring
/// automatic recovery. The immutable public helper remains available for fixtures and callers.
pub(crate) fn save_tracked(s: &mut Session, dir: &Path) -> Result<usize> {
    reject_overfull_checkpoints(s, dir)?;
    let access = prepare_save(s, dir)?;
    let mut n = 0;
    let preview = s.new_document_preview();
    for index in 0..s.docs.len() {
        // New Document's preview is never written, so it has no checkpoint to discard either.
        if s.docs.get(index).is_some_and(|document| preview == Some(document.uid)) {
            continue;
        }
        if s.docs.get(index).is_some_and(|document| !document.is_dirty()) {
            discard_document(s, index, Some(dir));
            continue;
        }
        let Some(document) = s.docs.get(index) else { continue };
        // A background write of an older state must not land over this one.
        #[cfg(not(target_arch = "wasm32"))]
        worker::cancel(dir, document.uid);
        let entry = Snapshot::of(&access, dir, document);
        let written = write_snapshot(&entry, || false);
        // The primary is the recovery data; its metadata is optional. Track the primary before
        // returning a metadata error, so Save and Close remove the snapshot this run wrote.
        if written.published
            && let Some(document) = s.docs.get_mut(index)
            && !has_checkpoint(document, dir)
        {
            document.recovery_entries.push((dir.to_path_buf(), document.uid));
        }
        if let Some(e) = written.error {
            return Err(e);
        }
        n += 1;
    }
    Ok(n)
}

/// Reject a complete batch before writing rather than silently losing old ownership records.
fn reject_overfull_checkpoints(s: &Session, dir: &Path) -> Result<()> {
    for document in &s.docs {
        if !document.is_dirty() || has_checkpoint(document, dir) {
            continue;
        }
        // Background writes whose results are not taken yet hold checkpoints the entries do not.
        #[cfg(not(target_arch = "wasm32"))]
        let pending = worker::pending_folders(document.uid, s.recovery_owner.0)
            .into_iter()
            .map(|pending| directory_key(&pending).unwrap_or(pending))
            .collect::<std::collections::BTreeSet<_>>()
            .iter()
            .filter(|pending| !same_directory(pending, dir) && !has_checkpoint(document, pending))
            .count();
        #[cfg(target_arch = "wasm32")]
        let pending = 0;
        if document.recovery_entries.len() + pending >= MAX_RECOVERY_ENTRIES {
            return Err(EngineError::Other(
                "too many recovery checkpoints; save or close the document before choosing another recovery folder".into(),
            ));
        }
    }
    Ok(())
}

/// A document's recovery write, copied so a background thread can write it while the interface
/// stays responsive: no deflate, no copy of the placed files, happens on the interface thread.
struct Snapshot {
    dir: PathBuf,
    uid: u64,
    /// The document as it was when the write started (data-merge previews save the unfilled one).
    doc: std::sync::Arc<designcraft_doc::Document>,
    path: Option<String>,
    title: String,
    /// Whether this run may replace an entry already in the folder.
    own: bool,
    /// The folder's canonical key, for recording that a shared entry is this run's now.
    key: PathBuf,
    /// The session that queued a background write ([`Session::recovery_owner`]), which takes its
    /// result.
    #[cfg_attr(target_arch = "wasm32", allow(dead_code))]
    owner: u64,
}

/// A number for each session, so background results go back to the session that queued them.
/// Dropping it retires the number: results still queued for it are then dropped.
pub(crate) struct Owner(pub(crate) u64);

/// The numbers of the sessions that exist.
static LIVE_OWNERS: std::sync::Mutex<std::collections::BTreeSet<u64>> = std::sync::Mutex::new(std::collections::BTreeSet::new());

impl Owner {
    pub(crate) fn new() -> Self {
        static NEXT_OWNER: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(1);
        let id = NEXT_OWNER.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        LIVE_OWNERS.lock().unwrap_or_else(std::sync::PoisonError::into_inner).insert(id);
        Owner(id)
    }
}

impl Drop for Owner {
    fn drop(&mut self) {
        LIVE_OWNERS.lock().unwrap_or_else(std::sync::PoisonError::into_inner).remove(&self.0);
    }
}

impl Snapshot {
    fn of(access: &Access, dir: &Path, d: &DocState) -> Self {
        Snapshot {
            dir: dir.to_path_buf(),
            uid: d.uid,
            doc: d.preview_stash.clone().unwrap_or_else(|| d.doc.clone()),
            path: d.path.clone(),
            title: d.title(),
            // An entry this run did not write is never replaced, even one made since it checked.
            own: access.owns(d.uid),
            key: access.key.clone(),
            owner: 0,
        }
    }
}

/// What one recovery write left in the folder: whether the primary document was published, and
/// the error that stopped it or the optional metadata written after it. The primary is the
/// recovery data (see the module docs: metadata may be missing or stale), so a metadata failure
/// must not lose the checkpoint that lets Save and Close remove what this run wrote.
struct Written {
    published: bool,
    error: Option<EngineError>,
}

/// A writer that fails once `cancelled` says so, checked before each write of at most
/// [`STOP_CHECK_BYTES`].
struct StopWhen<'a, W, C> {
    out: &'a mut W,
    cancelled: &'a C,
}

/// The most [`StopWhen`] writes between two checks for a cancel.
const STOP_CHECK_BYTES: usize = 1 << 20;

impl<W: std::io::Write, C: Fn() -> bool> std::io::Write for StopWhen<'_, W, C> {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        if (self.cancelled)() {
            return Err(std::io::Error::other("recovery write cancelled"));
        }
        // A large placed file is written in pieces, so a cancel does not wait for all of it.
        self.out.write(&buf[..buf.len().min(STOP_CHECK_BYTES)])
    }

    fn flush(&mut self) -> std::io::Result<()> {
        self.out.flush()
    }
}

impl<W: std::io::Seek, C> std::io::Seek for StopWhen<'_, W, C> {
    fn seek(&mut self, pos: std::io::SeekFrom) -> std::io::Result<u64> {
        self.out.seek(pos)
    }
}

/// Publish a document's recovery entry before its optional metadata. `cancelled` says whether the
/// document was saved, reverted or closed since the write started: its snapshot is then dropped
/// instead of published, so a crash cannot offer back a copy older than the saved file.
fn write_snapshot(entry: &Snapshot, cancelled: impl Fn() -> bool) -> Written {
    #[cfg(any(test, feature = "testing"))]
    testing::gate(&entry.dir);
    let (doc, meta) = files(&entry.dir, entry.uid);
    // Set at the rename: an error after it (syncing the folder) leaves the entry on disk.
    let landed = std::cell::Cell::new(false);
    let published = publish_recovery_written(
        &doc,
        entry.own,
        // A cancel stops the write at its next chunk instead of after the whole package.
        |out| {
            designcraft_format::write(&mut StopWhen { out, cancelled: &cancelled }, &entry.doc, None).map_err(|e| EngineError::Other(e.to_string()))
        },
        &cancelled,
        // Written: in a folder that is not this run's own, the entry is now this run's to replace
        // and remove (Save and Close must clean up the snapshot they wrote). Recorded with the
        // rename, so a cleanup after it can remove the entry.
        || {
            landed.set(true);
            if !entry.own {
                WRITTEN.lock().unwrap_or_else(std::sync::PoisonError::into_inner).insert((entry.key.clone(), entry.uid));
            }
        },
    );
    match published {
        Ok(true) => {}
        // Cancelled while writing: no snapshot and no metadata for it. A write stopped part way
        // reports an error, which is the cancel's and not a failure.
        Ok(false) => return Written { published: false, error: None },
        Err(_) if cancelled() => return Written { published: false, error: None },
        // Renamed into place, then the folder sync failed: the entry is there to track and clean up.
        Err(e) if landed.get() => return Written { published: true, error: Some(e) },
        Err(e) => return Written { published: false, error: Some(e) },
    }
    let metadata = json!({"path": entry.path, "title": entry.title, "saved": designcraft_doc::vars::now()});
    // A cancel after the primary landed removes it; the metadata must not be left without it.
    let bytes = metadata.to_string();
    let error = publish_recovery_written(
        &meta,
        entry.own,
        |out| std::io::Write::write_all(out, bytes.as_bytes()).map_err(|e| EngineError::Other(format!("{}: {e}", meta.display()))),
        &cancelled,
        || {},
    )
    .err();
    Written { published: true, error }
}

/// Write recovery entries for dirty documents on a background thread and drop clean checkpoints
/// now. The interface thread only snapshots what to write: `designcraft_format::write` deflates
/// and copies placed files, which takes minutes on a catalogue with gigabytes of links. Returns
/// how many documents were queued.
pub(crate) fn start_tracked(s: &mut Session, dir: &Path) -> Result<usize> {
    reject_overfull_checkpoints(s, dir)?;
    let access = prepare_save(s, dir)?;
    // Nothing runs on the web's single thread: it would freeze the page just the same.
    #[cfg(target_arch = "wasm32")]
    {
        let _ = access;
        return save_tracked(s, dir);
    }
    #[cfg(not(target_arch = "wasm32"))]
    {
        let mut queued = 0;
        let preview = s.new_document_preview();
        for index in 0..s.docs.len() {
            if s.docs.get(index).is_some_and(|document| preview == Some(document.uid)) {
                continue;
            }
            if s.docs.get(index).is_some_and(|document| !document.is_dirty()) {
                discard_document(s, index, Some(dir));
                continue;
            }
            let Some(document) = s.docs.get(index) else { continue };
            let mut entry = Snapshot::of(&access, dir, document);
            entry.owner = s.recovery_owner.0;
            // A write still running for this document already holds a snapshot of it; the next
            // timer tick catches up with the edits made since.
            match worker::submit(entry) {
                Some(true) => queued += 1,
                Some(false) => {}
                None => {
                    // No thread: write this one here rather than lose the recovery data.
                    let written = write_snapshot(&Snapshot::of(&access, dir, document), || false);
                    if written.published {
                        record_checkpoint(s, dir.to_path_buf(), document.uid);
                    }
                    if let Some(e) = written.error {
                        return Err(e);
                    }
                    queued += 1;
                }
            }
        }
        Ok(queued)
    }
}

/// Apply one finished background write: record the checkpoint of a document still open and
/// unsaved, or drop the snapshot of one saved, reverted or closed while its write ran. Returns
/// whether a checkpoint was recorded.
#[cfg(not(target_arch = "wasm32"))]
fn record_checkpoint(s: &mut Session, dir: PathBuf, uid: u64) -> bool {
    match s.docs.iter_mut().find(|doc| doc.uid == uid) {
        Some(doc) if doc.is_dirty() => {
            if !has_checkpoint(doc, &dir) {
                doc.recovery_entries.push((dir, uid));
            }
            true
        }
        // Saved, reverted or closed while being written: its snapshot must not come back after a
        // crash, and the timer writes only when a document is dirty, so nothing else would
        // remove it.
        Some(_) | None => {
            discard(&dir, uid);
            false
        }
    }
}

/// Apply finished background writes. Returns what was written and what failed.
pub(crate) struct Finished {
    pub written: usize,
    pub errors: Vec<String>,
    pub busy: bool,
}

#[cfg(not(target_arch = "wasm32"))]
pub(crate) fn finished(s: &mut Session) -> Finished {
    let mut out = Finished { written: 0, errors: Vec::new(), busy: false };
    let Some(w) = worker::started() else { return out };
    // Before draining: the worker sends a result before it clears the document as running, so a writer idle now has every result in the channel already.
    let busy = w.busy();
    // Results go to the session that queued them: another session has none of those documents,
    // and would remove their snapshots as if they had been closed.
    let mine: Vec<worker::Done> = {
        let done = w.done.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
        let mut others = w.undelivered.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
        let mut all = std::mem::take(&mut *others);
        all.extend(done.try_iter());
        let (mine, rest): (Vec<_>, Vec<_>) = all.into_iter().partition(|d| d.owner == s.recovery_owner.0);
        // A session that is gone takes nothing: its results are dropped rather than kept for ever.
        let live = LIVE_OWNERS.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
        *others = rest.into_iter().filter(|d| live.contains(&d.owner)).collect();
        mine
    };
    for d in mine {
        // A primary that landed belongs to this run even if its optional metadata failed: record
        // it, so Save and Close remove the snapshot instead of leaving it in the folder for ever.
        if d.published && record_checkpoint(s, d.dir, d.uid) {
            out.written += 1;
        }
        if let Some(e) = d.error {
            out.errors.push(e);
        }
    }
    out.busy = busy;
    out
}

#[cfg(target_arch = "wasm32")]
pub(crate) fn finished(_s: &mut Session) -> Finished {
    // Nothing is written in the background on the web.
    Finished { written: 0, errors: Vec::new(), busy: false }
}

/// Automatic recovery's writer: one thread with its own queue, so an unsaved document with
/// gigabytes of placed files is written without stopping the interface.
#[cfg(not(target_arch = "wasm32"))]
mod worker {
    use std::collections::{BTreeMap, BTreeSet};
    use std::sync::mpsc::{Receiver, Sender};
    use std::sync::{Condvar, Mutex, OnceLock};

    use super::{Path, PathBuf, Result, Snapshot, write_snapshot};

    /// One document written, for the interface thread to record.
    pub(super) struct Done {
        pub(super) dir: PathBuf,
        pub(super) uid: u64,
        pub(super) error: Option<String>,
        /// The primary document reached the folder (its optional metadata may have failed): the
        /// checkpoint must still be recorded, or Save and Close could not remove it.
        pub(super) published: bool,
        /// The session that queued the write.
        pub(super) owner: u64,
    }

    pub(super) struct Writer {
        jobs: Sender<Snapshot>,
        pub(super) done: Mutex<Receiver<Done>>,
        /// Results taken from `done` by a session that did not queue them, kept for their own.
        pub(super) undelivered: Mutex<Vec<Done>>,
        /// Documents queued or being written, with the session that queued each: a second write
        /// of one still holds nothing newer.
        running: Mutex<BTreeMap<(PathBuf, u64), Queued>>,
        /// A document saved, reverted or closed while its write was queued or running: the
        /// snapshot it is producing must not be published.
        cancelled: Mutex<BTreeSet<(PathBuf, u64)>>,
        /// Notified each time a write finishes and leaves `running`.
        idle: Condvar,
    }

    /// A write in flight: the session that queued it, and its folder as that session named it.
    struct Queued {
        owner: u64,
        dir: PathBuf,
        /// A newer write of the same document and folder, queued after this one was cancelled:
        /// it is written as soon as this one stops.
        next: Option<Snapshot>,
    }

    static WRITER: OnceLock<Option<Writer>> = OnceLock::new();

    /// A write's key: the folder's canonical path, so two spellings of one folder are one write.
    fn key(dir: &Path, uid: u64) -> (PathBuf, u64) {
        (super::directory_key(dir).unwrap_or_else(|_| dir.to_path_buf()), uid)
    }

    impl Writer {
        pub(super) fn busy(&self) -> bool {
            self.running.lock().is_ok_and(|running| !running.is_empty())
        }

        /// `key` is the job's ([`Snapshot::key`], uid). Returns the write queued to follow it.
        fn finish(&self, key: &(PathBuf, u64)) -> Option<Snapshot> {
            let next = match self.running.lock() {
                Ok(mut running) => {
                    let next = running.get_mut(key).and_then(|q| q.next.take());
                    match (&next, running.get_mut(key)) {
                        (Some(n), Some(q)) => {
                            q.owner = n.owner;
                            q.dir = n.dir.clone();
                        }
                        _ => {
                            running.remove(key);
                        }
                    }
                    // The mark was for the write that stopped, not the one that follows it.
                    if let Ok(mut cancelled) = self.cancelled.lock() {
                        cancelled.remove(key);
                    }
                    next
                }
                Err(_) => None,
            };
            self.idle.notify_all();
            next
        }

        /// Block until no write is queued or running.
        #[cfg(any(test, feature = "testing"))]
        pub(super) fn wait_until_idle(&self) {
            let mut running = self.running.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
            while !running.is_empty() {
                running = self.idle.wait(running).unwrap_or_else(std::sync::PoisonError::into_inner);
            }
        }

        /// Do not publish the snapshot being written for `dir`/`uid`: the document was saved,
        /// reverted or closed. Only a write that is queued or running is marked, so the set stays
        /// as small as the work in flight.
        fn cancel(&self, dir: &Path, uid: u64) {
            let canonical = key(dir, uid);
            // A rename under way finishes first, so the caller's cleanup comes after it; a later
            // one sees the mark.
            let _publishing = super::PUBLISH.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
            // The folder's canonical path, or its name as given: a link in the path may lead
            // elsewhere now than when the write was queued.
            let Ok(mut running) = self.running.lock() else { return };
            let keys: Vec<(PathBuf, u64)> =
                running.iter().filter(|(k, q)| **k == canonical || (k.1 == uid && q.dir == dir)).map(|(k, _)| k.clone()).collect();
            // A write queued to follow it is older than what the caller has now, too.
            for k in &keys {
                if let Some(q) = running.get_mut(k) {
                    q.next = None;
                }
            }
            if let Ok(mut cancelled) = self.cancelled.lock() {
                cancelled.extend(keys);
            }
        }

        /// [`Writer::cancel`] for every folder a write of `uid` queued by session `owner` is queued
        /// or running for. Returns those folders, and the folders of its finished writes whose
        /// results nobody has taken.
        fn cancel_document(&self, uid: u64, owner: u64) -> Vec<PathBuf> {
            // Held while results are read: the writer checks for a cancel and sends its result
            // under it, so a result sent after this sees the cancel.
            let _publishing = super::PUBLISH.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
            // The canonical keys to mark, and the folders as the session named them, which is how
            // the cleanup finds this run's folder.
            let mut dirs = Vec::new();
            if let Ok(mut running) = self.running.lock() {
                let mut keys = Vec::new();
                for (key, q) in running.iter_mut().filter(|((_, u), _)| *u == uid) {
                    // A write another session queued to follow stays queued.
                    if q.next.as_ref().is_some_and(|n| n.owner == owner) {
                        q.next = None;
                    }
                    if q.owner == owner {
                        keys.push(key.clone());
                        dirs.push(q.dir.clone());
                    }
                }
                if let Ok(mut cancelled) = self.cancelled.lock() {
                    cancelled.extend(keys);
                }
            }
            // Results stay queued for their session; only their folders are read here.
            let done = self.done.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
            let mut undelivered = self.undelivered.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
            undelivered.extend(done.try_iter());
            for d in undelivered.iter_mut().filter(|d| d.uid == uid && d.owner == owner && d.published) {
                dirs.push(d.dir.clone());
                // The caller removes that snapshot: its result must not record it as a checkpoint
                // once the document has changed again.
                d.published = false;
            }
            dirs
        }

        /// The folders of writes of `uid` queued by session `owner` that are in flight, or finished
        /// and published with their results not taken yet: checkpoints its entries do not hold yet.
        fn pending_folders(&self, uid: u64, owner: u64) -> Vec<PathBuf> {
            // A cancelled write publishes nothing; a write queued to follow it does.
            let mut dirs: Vec<PathBuf> = self
                .running
                .lock()
                .map(|running| {
                    running
                        .iter()
                        .filter(|(key, q)| {
                            key.1 == uid && ((q.owner == owner && !self.cancelled(key)) || q.next.as_ref().is_some_and(|n| n.owner == owner))
                        })
                        .map(|((dir, _), _)| dir.clone())
                        .collect()
                })
                .unwrap_or_default();
            let done = self.done.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
            let mut undelivered = self.undelivered.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
            undelivered.extend(done.try_iter());
            dirs.extend(undelivered.iter().filter(|d| d.uid == uid && d.owner == owner && d.published).map(|d| d.dir.clone()));
            dirs
        }

        /// `key` as in [`Writer::finish`].
        fn cancelled(&self, key: &(PathBuf, u64)) -> bool {
            self.cancelled.lock().is_ok_and(|cancelled| cancelled.contains(key))
        }
    }

    /// The writer, started on first use (a thread that can't start leaves recovery synchronous).
    fn writer() -> Option<&'static Writer> {
        WRITER
            .get_or_init(|| {
                let (jobs, job_rx) = std::sync::mpsc::channel::<Snapshot>();
                let (done_tx, done_rx) = std::sync::mpsc::channel::<Done>();
                let spawned = std::thread::Builder::new().name("designcraft-recovery".into()).spawn(move || {
                    while let Ok(first) = job_rx.recv() {
                        let mut queued = Some(first);
                        while let Some(job) = queued.take() {
                            let (published, error) = guarded_write(&job);
                            let key = (job.key.clone(), job.uid);
                            let (dir, uid, owner) = (job.dir, job.uid, job.owner);
                            // The result before the document reads as idle: a writer that is not
                            // busy has already delivered it, so the interface never stops polling
                            // with an answer still on its way.
                            let sent = {
                                let _publishing = super::PUBLISH.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
                                // Cancelled after it was published: the canceller removes the snapshot.
                                let published = published && !started().is_some_and(|w| w.cancelled(&key));
                                done_tx.send(Done { dir, uid, error, published, owner }).is_ok()
                            };
                            queued = WRITER.get().and_then(Option::as_ref).and_then(|w| w.finish(&key));
                            if !sent {
                                return;
                            }
                        }
                    }
                });
                spawned.ok().map(|_| Writer {
                    jobs,
                    done: Mutex::new(done_rx),
                    undelivered: Mutex::new(Vec::new()),
                    running: Mutex::new(BTreeMap::new()),
                    cancelled: Mutex::new(BTreeSet::new()),
                    idle: Condvar::new(),
                })
            })
            .as_ref()
    }

    /// The writer if it has been started.
    pub(super) fn started() -> Option<&'static Writer> {
        WRITER.get().and_then(Option::as_ref)
    }

    /// Do not publish the snapshot being written for `dir`/`uid` (see [`Writer::cancel`]).
    pub(super) fn cancel(dir: &Path, uid: u64) {
        if let Some(w) = started() {
            w.cancel(dir, uid);
        }
    }

    /// See [`Writer::pending_folders`].
    pub(super) fn pending_folders(uid: u64, owner: u64) -> Vec<PathBuf> {
        started().map(|w| w.pending_folders(uid, owner)).unwrap_or_default()
    }

    /// Do not publish any write of `uid` in flight (see [`Writer::cancel_document`]).
    pub(super) fn cancel_document(uid: u64, owner: u64) -> Vec<PathBuf> {
        started().map(|w| w.cancel_document(uid, owner)).unwrap_or_default()
    }

    /// Whether that write is already marked as cancelled (see [`Writer::cancelled`]).
    #[cfg(test)]
    pub(super) fn cancelled(dir: &Path, uid: u64) -> bool {
        started().is_some_and(|w| w.cancelled(&key(dir, uid)))
    }

    /// Write one snapshot, turning a panic into its error: a panic that ended this thread would
    /// leave the document marked as running for ever (so it is never written again) and the
    /// interface asking for frames for ever. A safety net: a writer still must not panic.
    fn guarded_write(job: &Snapshot) -> (bool, Option<String>) {
        let key = (job.key.clone(), job.uid);
        match guarded(|| Ok(write_snapshot(job, || started().is_some_and(|w| w.cancelled(&key))))) {
            // A primary that landed is this run's to remove, even if its metadata failed.
            Ok(written) => (written.published, written.error.map(|e| e.to_string())),
            Err(e) => (false, Some(e.to_string())),
        }
    }

    /// [`guarded_write`]'s guard, for any write on this thread.
    pub(super) fn guarded<T>(write: impl FnOnce() -> Result<T>) -> Result<T> {
        std::panic::catch_unwind(std::panic::AssertUnwindSafe(write)).unwrap_or_else(|payload| {
            Err(crate::EngineError::Other(format!("recovery writer panicked: {}", crate::guard::panic_message(payload.as_ref()))))
        })
    }

    /// Queue a document's write. `None`: no writer thread (the caller writes it itself);
    /// `Some(false)`: one for this document is still queued or running.
    pub(super) fn submit(entry: Snapshot) -> Option<bool> {
        let w = writer()?;
        let mut running = w.running.lock().ok()?;
        let key = (entry.key.clone(), entry.uid);
        if let Some(q) = running.get_mut(&key) {
            // The write in flight was cancelled (the document was saved since), so it holds
            // nothing to recover: this one follows it, in place of any that was waiting.
            if w.cancelled.lock().is_ok_and(|cancelled| cancelled.contains(&key)) {
                q.next = Some(entry);
                return Some(true);
            }
            return Some(false);
        }
        running.insert(key.clone(), Queued { owner: entry.owner, dir: entry.dir.clone(), next: None });
        // A write is wanted again: whatever cancelled the last one does not apply to this one.
        if let Ok(mut cancelled) = w.cancelled.lock() {
            cancelled.remove(&key);
        }
        if w.jobs.send(entry).is_err() {
            running.remove(&key);
            return None;
        }
        Some(true)
    }
}

/// Handshakes for tests of the recovery writer, in this crate and, with the `testing` feature, in
/// the app's: hold a write at its start, and wait for the writer to finish.
#[cfg(any(test, feature = "testing"))]
pub mod testing {
    use std::path::{Path, PathBuf};
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::sync::mpsc::{Receiver, Sender};
    use std::sync::{Arc, Mutex, PoisonError};
    use std::thread::ThreadId;

    struct Gate {
        dir: PathBuf,
        holder: ThreadId,
        entered: Sender<()>,
        go: Receiver<()>,
        ran_on_holder: Arc<AtomicBool>,
    }

    static GATE: Mutex<Option<Gate>> = Mutex::new(None);

    /// The next recovery write into one folder, held at its start until this is released or
    /// dropped.
    pub struct HeldWrite {
        entered: Receiver<()>,
        go: Option<Sender<()>>,
        ran_on_holder: Arc<AtomicBool>,
    }

    /// Hold the next recovery write into `dir`. A write on another thread waits before it writes
    /// anything; a write on the calling thread does not wait, and [`HeldWrite::ran_here`] reports it.
    pub fn hold_next_write(dir: &Path) -> HeldWrite {
        let (entered_tx, entered) = std::sync::mpsc::channel();
        let (go, go_rx) = std::sync::mpsc::channel();
        let ran_on_holder = Arc::new(AtomicBool::new(false));
        let gate = Gate {
            dir: dir.to_path_buf(),
            holder: std::thread::current().id(),
            entered: entered_tx,
            go: go_rx,
            ran_on_holder: ran_on_holder.clone(),
        };
        *GATE.lock().unwrap_or_else(PoisonError::into_inner) = Some(gate);
        HeldWrite { entered, go: Some(go), ran_on_holder }
    }

    impl HeldWrite {
        /// The held write ran on the thread that called [`hold_next_write`].
        pub fn ran_here(&self) -> bool {
            self.ran_on_holder.load(Ordering::SeqCst)
        }

        /// Block until the write has reached the gate.
        pub fn wait_until_held(&self) {
            let _ = self.entered.recv();
        }

        /// Let the held write go on.
        pub fn release(mut self) {
            if let Some(go) = self.go.take() {
                let _ = go.send(());
            }
        }
    }

    impl Drop for HeldWrite {
        fn drop(&mut self) {
            // A gate no write reached is removed, so it cannot hold a later test's write.
            let mut slot = GATE.lock().unwrap_or_else(PoisonError::into_inner);
            if slot.as_ref().is_some_and(|gate| Arc::ptr_eq(&gate.ran_on_holder, &self.ran_on_holder)) {
                *slot = None;
            }
        }
    }

    /// Called at the start of every recovery write.
    pub(super) fn gate(dir: &Path) {
        let gate = {
            let mut slot = GATE.lock().unwrap_or_else(PoisonError::into_inner);
            if slot.as_ref().is_some_and(|gate| gate.dir == dir) { slot.take() } else { None }
        };
        let Some(gate) = gate else { return };
        if std::thread::current().id() == gate.holder {
            gate.ran_on_holder.store(true, Ordering::SeqCst);
            let _ = gate.entered.send(());
            return;
        }
        let _ = gate.entered.send(());
        // A dropped `HeldWrite` closes the channel, which also lets the write go on.
        let _ = gate.go.recv();
    }

    /// Block until the background writer has no write queued or running.
    pub fn wait_until_idle() {
        // Nothing is written in the background on the web.
        #[cfg(not(target_arch = "wasm32"))]
        if let Some(w) = super::worker::started() {
            w.wait_until_idle();
        }
    }

    static FAIL_SYNC: Mutex<Option<PathBuf>> = Mutex::new(None);

    /// Make the next folder sync after a recovery file is renamed into `dir` fail.
    pub fn fail_next_sync(dir: &Path) {
        *FAIL_SYNC.lock().unwrap_or_else(PoisonError::into_inner) = Some(dir.to_path_buf());
    }

    /// Whether this sync of `dir` is the one [`fail_next_sync`] asked to fail.
    pub(super) fn sync_fails(dir: &Path) -> bool {
        let mut slot = FAIL_SYNC.lock().unwrap_or_else(PoisonError::into_inner);
        if slot.as_deref() == Some(dir) {
            *slot = None;
            return true;
        }
        false
    }
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
    publish_recovery_written(
        path,
        replace,
        |out| out.write_all(bytes).map_err(|e| EngineError::Other(format!("{}: {e}", path.display()))),
        || false,
        || {},
    )
    .map(|_| ())
}

/// Serializes the last check for a cancelled write with the rename that publishes it, and with
/// [`worker::cancel`].
#[cfg(not(target_arch = "wasm32"))]
static PUBLISH: std::sync::Mutex<()> = std::sync::Mutex::new(());

/// [`publish_recovery_file`], with `write` filling the temporary file: a large document is written
/// straight to disk instead of being built in memory first. Returns whether it was published;
/// `cancelled` is asked after the bytes are written and before the rename, so a document saved or
/// closed meanwhile leaves nothing behind. `on_publish` runs right after the rename, before a
/// cancel can.
#[cfg(not(target_arch = "wasm32"))]
fn publish_recovery_written(
    path: &Path,
    replace: bool,
    write: impl FnOnce(&mut std::io::BufWriter<&mut std::fs::File>) -> Result<()>,
    cancelled: impl Fn() -> bool,
    on_publish: impl FnOnce(),
) -> Result<bool> {
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
    {
        let mut out = std::io::BufWriter::new(tmp.as_file_mut());
        write(&mut out)?;
        out.flush().map_err(|e| EngineError::Other(format!("{}: {e}", path.display())))?;
    }
    tmp.as_file_mut().sync_all().map_err(|e| EngineError::Other(format!("{}: {e}", path.display())))?;
    {
        // Held from the check to the rename: a cancel waits for a rename already under way (the
        // cleanup after it then removes the file), and a rename after a cancel sees it.
        let _publishing = PUBLISH.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
        // Last moment: the document was saved, reverted or closed while the bytes were written.
        // The temporary file is dropped, so nothing of the stale snapshot stays.
        if cancelled() {
            return Ok(false);
        }
        // Validate the non-followed leaf again immediately before renaming the randomized
        // temporary file to its fixed sibling destination.
        validate_recovery_dir(dir)?;
        let published = if replace { tmp.persist(path) } else { tmp.persist_noclobber(path) };
        published.map_err(|e| EngineError::Other(format!("{}: {}", path.display(), e.error)))?;
        on_publish();
    }
    sync_recovery_dir(dir)?;
    Ok(true)
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
    #[cfg(any(test, feature = "testing"))]
    if testing::sync_fails(dir) {
        return Err(EngineError::Other(format!("{}: sync failed", dir.display())));
    }

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

#[cfg(target_arch = "wasm32")]
fn publish_recovery_written(
    _path: &Path,
    _replace: bool,
    _write: impl FnOnce(&mut std::io::BufWriter<&mut std::fs::File>) -> Result<()>,
    _cancelled: impl Fn() -> bool,
    _on_publish: impl FnOnce(),
) -> Result<bool> {
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
        if matches!(path.extension().and_then(std::ffi::OsStr::to_str), Some("json" | "designcraft"))
            && let Some(uid) = path.file_stem().and_then(std::ffi::OsStr::to_str).and_then(|s| s.parse().ok())
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
        if p.extension().and_then(std::ffi::OsStr::to_str) != Some("designcraft") {
            continue;
        }
        let Some(uid) = p.file_stem().and_then(std::ffi::OsStr::to_str).and_then(|s| s.parse::<u64>().ok()) else { continue };
        // Metadata is a file on disk: anything but an object (corrupt, hand-edited) counts as none.
        let meta = read_entry_file(&p.with_extension("json"))
            .ok()
            .and_then(|t| serde_json::from_slice::<Value>(&t).ok())
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
        let Ok(bytes) = read_entry_file(&doc) else { continue };
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
        st.bump_revision();
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
mod tests;

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
