//! App-private Nightly session health, not telemetry or a Launcher protocol.
//! A live owner holds a separate persistent lease; secondary launches never rewrite its record.
//! Dropping the guard does not clear the marker. Only the app's controlled return path does.

use std::fs::{File, OpenOptions};
use std::io::{self, Read, Write as _};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, PoisonError};

const RECORD: &str = "nightly-session.json";
const LEASE: &str = "nightly-session.lock";
const TEMPORARY: &str = "nightly-session.json.staging";
const MAX_METADATA_BYTES: usize = 256;

pub struct Session {
    _lease: File,
    path: PathBuf,
    own_record: Vec<u8>,
    previous_unclean: bool,
}

fn regular_or_missing(path: &Path) -> io::Result<bool> {
    match std::fs::symlink_metadata(path) {
        Ok(metadata) if metadata.file_type().is_file() && !metadata.file_type().is_symlink() => Ok(true),
        Ok(_) => Err(io::Error::other("Nightly session state must be a regular file, not an alias or directory")),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(false),
        Err(error) => Err(error),
    }
}

/// Stage beside the destination and rename over it: a raced link at the record name is replaced,
/// never followed, and a reader sees either the old record or the whole new one.
fn write_record(root: &Path, name: &str, bytes: &[u8]) -> io::Result<()> {
    let staging = root.join(format!("{TEMPORARY}.{}", std::process::id()));
    let _ = std::fs::remove_file(&staging);
    let mut file = OpenOptions::new().write(true).create_new(true).open(&staging)?;
    let written = file.write_all(bytes).and_then(|()| file.sync_all());
    drop(file);
    match written.and_then(|()| std::fs::rename(&staging, root.join(name))) {
        Ok(()) => Ok(()),
        Err(error) => {
            let _ = std::fs::remove_file(&staging);
            Err(error)
        }
    }
}

/// The owner's session, shared by the window's exit path and the code after the event loop.
/// On macOS a quit ends the process inside AppKit's terminate, after eframe's `on_exit` and before
/// `eframe::run_native` could return, so `on_exit` finishes it there; the return path still runs
/// for a start that created no window.
pub type Shared = Arc<Mutex<Option<Session>>>;

pub fn shared() -> Shared {
    shared_with(None)
}

pub fn shared_with(session: Option<Session>) -> Shared {
    Arc::new(Mutex::new(session))
}

/// The controlled return ([`Session::finish`]) by whichever path comes first; later calls do nothing.
pub fn finish_once(shared: &Shared, unclean_notice_shown: bool) {
    let watch = shared.lock().unwrap_or_else(PoisonError::into_inner).take();
    if let Some(watch) = watch
        && let Err(error) = watch.finish(unclean_notice_shown)
    {
        log::warn!("Nightly session state was left intact: {error}");
    }
}

impl Session {
    /// The warning belongs to this window: a busy secondary has no health-record coverage,
    /// while Stable and a lease-owning Nightly window preserve their existing result.
    pub fn begin_with_health(root: Option<&Path>, version: &str, commit: Option<&str>) -> io::Result<(Option<Self>, bool)> {
        let watch = Self::begin(root, version, commit)?;
        let unavailable = root.is_some() && watch.is_none();
        Ok((watch, unavailable))
    }

    /// `None` is stable or another owner of this profile. Neither case touches the record.
    /// The supplied root must already be the validated private Nightly profile.
    pub fn begin(root: Option<&Path>, version: &str, commit: Option<&str>) -> io::Result<Option<Self>> {
        let Some(root) = root else { return Ok(None) };
        if version.is_empty() || version.len() > MAX_METADATA_BYTES || commit.is_some_and(|value| value.len() > MAX_METADATA_BYTES) {
            return Err(io::Error::new(io::ErrorKind::InvalidInput, "Nightly session provenance is missing or too large"));
        }
        let lease_path = root.join(LEASE);
        regular_or_missing(&lease_path)?;
        let lease = OpenOptions::new().read(true).write(true).create(true).truncate(false).open(lease_path)?;
        match lease.try_lock() {
            Ok(()) => {}
            Err(std::fs::TryLockError::WouldBlock) => return Ok(None),
            Err(std::fs::TryLockError::Error(error)) => return Err(error),
        }
        let path = root.join(RECORD);
        let previous_unclean = regular_or_missing(&path)?;
        let own_record = serde_json::to_vec(&serde_json::json!({
            "schema_version": 1,
            "channel": "nightly-v1",
            "version": version,
            "commit": commit.filter(|value| !value.is_empty()),
        }))?;
        write_record(root, RECORD, &own_record)?;
        Ok(Some(Self { _lease: lease, path, own_record, previous_unclean }))
    }

    pub fn previous_unclean(&self) -> bool {
        self.previous_unclean
    }

    /// Controlled return. If an earlier session's interruption was detected but no window showed
    /// it yet (a graphics failure before the app callback), the record is left for the next owner
    /// to report; otherwise this is [`Self::complete`].
    pub fn finish(self, unclean_notice_shown: bool) -> io::Result<()> {
        if self.previous_unclean && !unclean_notice_shown {
            return Ok(());
        }
        self.complete()
    }

    /// Controlled event-loop completion includes a handled start-up error, not only exit 0.
    /// An unwind, abort, forced exit or killed process never calls this method.
    pub fn complete(self) -> io::Result<()> {
        if !regular_or_missing(&self.path)? {
            return Ok(());
        }
        let mut current = Vec::new();
        File::open(&self.path)?.take(self.own_record.len() as u64 + 1).read_to_end(&mut current)?;
        if current != self.own_record {
            return Err(io::Error::other("Nightly session state changed; leaving it intact"));
        }
        std::fs::remove_file(&self.path)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicU64, Ordering};

    struct Fixture(PathBuf);
    impl Fixture {
        fn new() -> Self {
            static NEXT: AtomicU64 = AtomicU64::new(0);
            loop {
                let path =
                    std::env::temp_dir().join(format!("designcraft-nightly-session-{}-{}", std::process::id(), NEXT.fetch_add(1, Ordering::Relaxed)));
                match std::fs::create_dir(&path) {
                    Ok(()) => return Self(path),
                    Err(error) if error.kind() == io::ErrorKind::AlreadyExists => continue,
                    Err(error) => panic!("cannot create owned fixture: {error}"),
                }
            }
        }
    }
    impl Drop for Fixture {
        fn drop(&mut self) {
            std::fs::remove_dir_all(&self.0).unwrap();
        }
    }

    #[test]
    fn busy_secondary_reports_unavailable_without_replacing_the_owner_record() {
        let fixture = Fixture::new();
        let (stable, warning) = Session::begin_with_health(None, "stable", None).unwrap();
        assert!(stable.is_none() && !warning);
        let (first, warning) = Session::begin_with_health(Some(&fixture.0), "nightly", Some("owned-commit")).unwrap();
        assert!(!warning);
        let first = first.unwrap();
        let record = std::fs::read(fixture.0.join(RECORD)).unwrap();
        let (secondary, warning) = Session::begin_with_health(Some(&fixture.0), "secondary", None).unwrap();
        assert!(secondary.is_none() && warning, "a secondary window cannot claim recorded exit/crash coverage");
        assert_eq!(std::fs::read(fixture.0.join(RECORD)).unwrap(), record);
        first.complete().unwrap();
        assert!(!fixture.0.join(RECORD).exists());
    }

    #[test]
    fn controlled_completion_is_clean_but_an_unfinished_owner_is_reported_next_time() {
        let fixture = Fixture::new();
        let first = Session::begin(Some(&fixture.0), "first", Some("recorded-commit")).unwrap().unwrap();
        assert!(!first.previous_unclean());
        let own = std::fs::read(fixture.0.join(RECORD)).unwrap();
        assert!(Session::begin(Some(&fixture.0), "secondary", None).unwrap().is_none());
        assert_eq!(std::fs::read(fixture.0.join(RECORD)).unwrap(), own, "a busy secondary must not replace the owner's record");
        first.complete().unwrap();
        assert!(!fixture.0.join(RECORD).exists());
        let unfinished = Session::begin(Some(&fixture.0), "second", None).unwrap().unwrap();
        assert!(!unfinished.previous_unclean());
        drop(unfinished); // Unwind/abnormal-path ownership release, not a controlled completion.
        let next = Session::begin(Some(&fixture.0), "third", None).unwrap().unwrap();
        assert!(next.previous_unclean());
        next.complete().unwrap();
        assert!(!fixture.0.join(RECORD).exists());
    }

    #[test]
    fn an_interruption_no_window_showed_survives_a_handled_startup_failure() {
        let fixture = Fixture::new();
        drop(Session::begin(Some(&fixture.0), "crashed", None).unwrap().unwrap());
        let failed = Session::begin(Some(&fixture.0), "gpu-failed", None).unwrap().unwrap();
        assert!(failed.previous_unclean());
        failed.finish(false).unwrap();
        let retry = Session::begin(Some(&fixture.0), "gpu-retry", None).unwrap().unwrap();
        assert!(retry.previous_unclean());
        retry.finish(true).unwrap();
        assert!(!fixture.0.join(RECORD).exists(), "a session that showed the notice clears its record");
        let clean = Session::begin(Some(&fixture.0), "clean", None).unwrap().unwrap();
        assert!(!clean.previous_unclean());
        clean.finish(false).unwrap();
        assert!(!fixture.0.join(RECORD).exists());
    }

    #[test]
    fn the_exit_path_and_the_return_path_finish_the_session_once() {
        let fixture = Fixture::new();
        let shared = shared();
        *shared.lock().unwrap() = Session::begin(Some(&fixture.0), "quitting", None).unwrap();
        finish_once(&shared, false);
        assert!(!fixture.0.join(RECORD).exists(), "a quit that ran the exit path is a clean exit");
        let successor = Session::begin(Some(&fixture.0), "successor", None).unwrap().unwrap();
        assert!(!successor.previous_unclean());
        let record = std::fs::read(fixture.0.join(RECORD)).unwrap();
        finish_once(&shared, false);
        assert_eq!(std::fs::read(fixture.0.join(RECORD)).unwrap(), record, "the second finish must not touch a successor's record");
        successor.complete().unwrap();
    }

    #[test]
    fn cleanup_never_removes_an_unknown_changed_record() {
        let fixture = Fixture::new();
        let watch = Session::begin(Some(&fixture.0), "nightly", None).unwrap().unwrap();
        std::fs::write(fixture.0.join(RECORD), b"replacement owned by someone else").unwrap();
        assert!(watch.complete().is_err());
        assert_eq!(std::fs::read(fixture.0.join(RECORD)).unwrap(), b"replacement owned by someone else");
    }

    #[test]
    fn provenance_is_bounded_and_never_collects_documents_paths_or_diagnostics() {
        let fixture = Fixture::new();
        assert!(Session::begin(Some(&fixture.0), &"x".repeat(MAX_METADATA_BYTES + 1), None).is_err());
        assert_eq!(std::fs::read_dir(&fixture.0).unwrap().count(), 0);
        assert!(Session::begin(None, "", None).unwrap().is_none(), "stable has no session-health side effects");
        let watch = Session::begin(Some(&fixture.0), "nightly", None).unwrap().unwrap();
        let data: serde_json::Value = serde_json::from_slice(&std::fs::read(fixture.0.join(RECORD)).unwrap()).unwrap();
        let fields = data.as_object().unwrap().keys().map(String::as_str).collect::<std::collections::BTreeSet<_>>();
        assert_eq!(fields, ["schema_version", "channel", "version", "commit"].into_iter().collect());
        assert!(data["commit"].is_null(), "missing provenance must not be invented");
        watch.complete().unwrap();
        assert_eq!(std::fs::read_dir(&fixture.0).unwrap().count(), 1, "only the lease remains");
    }

    #[test]
    #[cfg(unix)]
    fn reserved_state_aliases_do_not_modify_external_files() {
        let fixture = Fixture::new();
        let external = fixture.0.join("stable-document");
        std::fs::write(&external, b"stable bytes").unwrap();
        for name in [LEASE, RECORD] {
            let alias = fixture.0.join(name);
            std::os::unix::fs::symlink(&external, &alias).unwrap();
            assert!(Session::begin(Some(&fixture.0), "nightly", None).is_err());
            assert_eq!(std::fs::read(&external).unwrap(), b"stable bytes");
            std::fs::remove_file(alias).unwrap();
        }
    }

    #[test]
    #[cfg(unix)]
    fn a_raced_record_link_is_replaced_rather_than_followed() {
        let fixture = Fixture::new();
        let external = fixture.0.join("stable-document");
        std::fs::write(&external, b"stable bytes").unwrap();
        // The record is absent when the owner begins, so the write path cannot refuse it; the
        // rename over a link that appears afterwards must not follow it.
        write_record(&fixture.0, RECORD, b"{}").unwrap();
        std::fs::remove_file(fixture.0.join(RECORD)).unwrap();
        std::os::unix::fs::symlink(&external, fixture.0.join(RECORD)).unwrap();
        write_record(&fixture.0, RECORD, b"nightly record").unwrap();
        assert_eq!(std::fs::read(&external).unwrap(), b"stable bytes");
        assert!(!std::fs::symlink_metadata(fixture.0.join(RECORD)).unwrap().file_type().is_symlink());
        assert_eq!(std::fs::read(fixture.0.join(RECORD)).unwrap(), b"nightly record");
    }
}
