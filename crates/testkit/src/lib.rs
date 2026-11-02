//! DesignCraft test helpers.
//!
//! Shared fixtures for crate and app tests live here so they stay in one
//! place instead of being duplicated across crates.
//!
//! This crate may only be used as a dev-dependency (enforced by `cargo xtask layers`).
// Test support only (a dev-dependency of every crate that uses it): a failed setup or assertion
// must panic, like `assert!`, so the shipped-code ban on panicking doesn't apply here.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
#![forbid(unsafe_code)]

/// A per-process temporary directory for test output (created on first use). It is made with
/// `create_dir` (owner-only on Unix), and a directory that already exists is reused only when it is a
/// real directory, not a link, with no access for others and writable by this user: one pre-made in a shared temp folder is refused.
pub fn temp_dir(tag: &str) -> std::path::PathBuf {
    let dir = std::env::temp_dir().join(format!("designcraft-testkit-{tag}-{}", std::process::id()));
    // Unix sets the mode on it; elsewhere it stays as made.
    #[cfg_attr(not(unix), allow(unused_mut))]
    let mut b = std::fs::DirBuilder::new();
    #[cfg(unix)]
    std::os::unix::fs::DirBuilderExt::mode(&mut b, 0o700);
    if let Err(e) = b.create(&dir) {
        let m = std::fs::symlink_metadata(&dir).expect("temp dir");
        assert!(e.kind() == std::io::ErrorKind::AlreadyExists && m.is_dir(), "create temp dir: {e}");
        #[cfg(unix)]
        assert!(std::os::unix::fs::PermissionsExt::mode(&m.permissions()) & 0o077 == 0, "{}: not private", dir.display());
        // A private directory made by another user passes the mode check but is closed to this one:
        // writing a probe file tells it apart from ours without needing the user ids.
        // Named for this call: tests running alongside probe the same directory.
        static PROBES: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
        let probe = dir.join(format!(".probe-{}", PROBES.fetch_add(1, std::sync::atomic::Ordering::Relaxed)));
        assert!(std::fs::write(&probe, b"").and_then(|()| std::fs::remove_file(&probe)).is_ok(), "{}: not owned by this user", dir.display());
    }
    dir
}

thread_local! {
    static QUIET: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
}

/// Run `f`, catching a panic without printing it (other threads' panics still print). Returns the
/// panic message on panic.
pub fn catch_quiet<T>(f: impl FnOnce() -> T) -> Result<T, String> {
    static HOOK: std::sync::Once = std::sync::Once::new();
    HOOK.call_once(|| {
        let prev = std::panic::take_hook();
        std::panic::set_hook(Box::new(move |info| {
            if !QUIET.with(|q| q.get()) {
                prev(info);
            }
        }));
    });
    QUIET.with(|q| q.set(true));
    let r = std::panic::catch_unwind(std::panic::AssertUnwindSafe(f));
    QUIET.with(|q| q.set(false));
    r.map_err(|p| p.downcast_ref::<String>().cloned().or_else(|| p.downcast_ref::<&str>().map(|s| s.to_string())).unwrap_or_else(|| "panic".into()))
}
