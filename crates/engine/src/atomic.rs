//! How documents, exports and other user files reach the disk: through `craft-atomic`, which
//! never writes the destination in place. It stages the bytes in a temporary file beside the
//! destination, flushes it, renames it over the destination and flushes the folder (Unix), so a
//! failed save leaves the old file whole. A new file gets the permissions ordinary file creation
//! gives it (the umask applies), a replaced file keeps its permissions, a symbolic link keeps
//! pointing at the file it names, and Windows renames are retried while a scanner holds the
//! file.
//!
//! The web has no file system, so this module is not built for `wasm32`.

use std::fs::File;
use std::io;
use std::path::Path;

/// Atomically replace (or create) `path` with `bytes`. See the module docs.
pub fn write_atomic(path: &Path, bytes: &[u8]) -> io::Result<()> {
    craft_atomic::atomic_write(path, bytes)
}

/// Atomically replace (or create) `path` with what `fill` writes into the temporary file. When
/// `fill`, the flush or the rename fails, the temporary file is removed, `path` is untouched and
/// the error is returned. A panic in `fill` is returned as an error too, so the temporary file is
/// removed then as well.
pub fn write_atomic_with(path: &Path, fill: impl FnOnce(&mut File) -> io::Result<()>) -> io::Result<()> {
    craft_atomic::atomic_write_with(path, |file| {
        std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| fill(file)))
            .unwrap_or_else(|_| Err(io::Error::other("the file writer stopped unexpectedly")))
    })
}

#[cfg(test)]
mod tests {
    #[cfg(unix)]
    use std::fs::OpenOptions;
    use std::io::Write as _;
    use std::path::PathBuf;

    use craft_atomic::{RenameRetry, retry_rename};

    use super::*;

    fn dir(name: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("dc-atomic-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    /// The names in `d`, sorted.
    fn names(d: &Path) -> Vec<String> {
        let mut v: Vec<String> = std::fs::read_dir(d).unwrap().map(|e| e.unwrap().file_name().to_string_lossy().into_owned()).collect();
        v.sort();
        v
    }

    #[test]
    fn replaces_and_creates_without_leftovers() {
        let d = dir("replace");
        let path = d.join("doc.designcraft");
        write_atomic(&path, b"first").unwrap();
        assert_eq!(std::fs::read(&path).unwrap(), b"first");
        write_atomic(&path, b"second, longer").unwrap();
        assert_eq!(std::fs::read(&path).unwrap(), b"second, longer");
        write_atomic(&path, b"3").unwrap();
        assert_eq!(std::fs::read(&path).unwrap(), b"3");
        assert_eq!(names(&d), ["doc.designcraft"], "no temporary file is left");
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn a_failed_write_leaves_the_original_and_no_temporary_file() {
        let d = dir("fail");
        let path = d.join("doc.designcraft");
        std::fs::write(&path, b"original").unwrap();
        let r = write_atomic_with(&path, |f| {
            f.write_all(b"half a docu")?;
            Err(io::Error::other("disk full"))
        });
        let e = r.unwrap_err().to_string();
        assert!(e.contains("disk full") && e.contains("not changed"), "{e}");
        assert_eq!(std::fs::read(&path).unwrap(), b"original");
        assert_eq!(names(&d), ["doc.designcraft"], "the temporary file is removed");
        // A folder that doesn't exist: an error, nothing written.
        assert!(write_atomic(&d.join("missing").join("x.designcraft"), b"x").is_err());
        assert_eq!(names(&d), ["doc.designcraft"]);
        // A folder in the way: an error, the folder stays and nothing is left beside it.
        std::fs::create_dir(d.join("folder")).unwrap();
        assert!(write_atomic(&d.join("folder"), b"x").is_err());
        assert_eq!(names(&d), ["doc.designcraft", "folder"]);
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn a_long_destination_name_does_not_lengthen_the_temporary_name() {
        let d = dir("long-name");
        let name = format!("{}.designcraft", "a".repeat(235));
        let path = d.join(&name);
        std::fs::write(&path, b"old").unwrap();
        write_atomic(&path, b"new").unwrap();
        assert_eq!(std::fs::read(&path).unwrap(), b"new");
        assert_eq!(names(&d), [name]);
        let _ = std::fs::remove_dir_all(&d);
    }

    #[cfg(unix)]
    #[test]
    fn private_permissions_are_preserved_while_the_file_is_written() {
        use std::os::unix::fs::PermissionsExt as _;
        let d = dir("private-staging");
        let path = d.join("private.designcraft");
        std::fs::write(&path, b"old").unwrap();
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600)).unwrap();
        write_atomic_with(&path, |file| {
            assert_eq!(file.metadata()?.permissions().mode() & 0o777, 0o600);
            file.write_all(b"private document")?;
            assert_eq!(file.metadata()?.permissions().mode() & 0o777, 0o600);
            Ok(())
        })
        .unwrap();
        assert_eq!(std::fs::read(&path).unwrap(), b"private document");
        assert_eq!(std::fs::metadata(&path).unwrap().permissions().mode() & 0o777, 0o600);
        assert_eq!(names(&d), ["private.designcraft"]);
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn an_unwinding_writer_removes_its_partial_temporary_file() {
        let d = dir("unwinding-writer");
        let path = d.join("private.designcraft");
        std::fs::write(&path, b"old").unwrap();
        let result = write_atomic_with(&path, |file| {
            file.write_all(b"partial private document")?;
            panic!("synthetic writer panic");
        });
        assert!(result.is_err());
        assert_eq!(std::fs::read(&path).unwrap(), b"old");
        assert_eq!(names(&d), ["private.designcraft"]);
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn the_rename_is_retried_only_while_access_is_denied() {
        let mut calls = 0;
        let r = retry_rename(RenameRetry { attempts: 2, first_delay_ms: 0 }, || {
            calls += 1;
            if calls < 3 { Err(io::Error::from(io::ErrorKind::PermissionDenied)) } else { Ok(()) }
        });
        assert!(r.is_ok() && calls == 3);
        calls = 0;
        let r = retry_rename(RenameRetry { attempts: 2, first_delay_ms: 0 }, || {
            calls += 1;
            Err(io::Error::from(io::ErrorKind::PermissionDenied))
        });
        assert_eq!(r.map_err(|e| e.kind()), Err(io::ErrorKind::PermissionDenied));
        assert_eq!(calls, 3, "gives up after the last try");
        calls = 0;
        let r = retry_rename(RenameRetry { attempts: 2, first_delay_ms: 0 }, || {
            calls += 1;
            Err(io::Error::from(io::ErrorKind::CrossesDevices))
        });
        assert!(r.is_err() && calls == 1, "other errors are not retried");
    }

    #[cfg(unix)]
    #[test]
    fn new_files_follow_the_umask_and_replaced_files_keep_their_mode() {
        use std::os::unix::fs::PermissionsExt as _;
        let d = dir("new-mode");
        let mode = |p: &Path| std::fs::metadata(p).unwrap().permissions().mode() & 0o777;
        let plain = d.join("plain");
        File::create(&plain).unwrap();
        let path = d.join("new.designcraft");
        write_atomic(&path, b"new").unwrap();
        assert_eq!(mode(&path), mode(&plain), "a new file gets the mode ordinary creation gives");
        let streamed = d.join("streamed.designcraft");
        write_atomic_with(&streamed, |f| f.write_all(b"new")).unwrap();
        assert_eq!(mode(&streamed), mode(&plain));
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o640)).unwrap();
        write_atomic(&path, b"replaced").unwrap();
        assert_eq!(std::fs::read(&path).unwrap(), b"replaced");
        assert_eq!(mode(&path), 0o640, "a replaced file keeps its mode");
        let _ = std::fs::remove_dir_all(&d);
    }

    #[cfg(unix)]
    #[test]
    fn permissions_are_kept_and_unwritable_files_are_not_replaced() {
        use std::os::unix::fs::PermissionsExt as _;
        let d = dir("perms");
        let path = d.join("private.designcraft");
        std::fs::write(&path, b"old").unwrap();
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600)).unwrap();
        write_atomic(&path, b"new").unwrap();
        assert_eq!(std::fs::read(&path).unwrap(), b"new");
        assert_eq!(std::fs::metadata(&path).unwrap().permissions().mode() & 0o777, 0o600);
        // A read-only file stays as it is (root may write it anyway).
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o400)).unwrap();
        if OpenOptions::new().write(true).open(&path).is_err() {
            assert!(write_atomic(&path, b"newer").is_err());
            assert_eq!(std::fs::read(&path).unwrap(), b"new");
            assert_eq!(names(&d), ["private.designcraft"]);
        }
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600)).unwrap();
        let _ = std::fs::remove_dir_all(&d);
    }

    /// A folder the user can't write: the save fails and the original stays.
    #[cfg(unix)]
    #[test]
    fn an_unwritable_folder_fails_without_touching_the_file() {
        use std::os::unix::fs::PermissionsExt as _;
        let d = dir("ro-dir");
        let path = d.join("doc.designcraft");
        std::fs::write(&path, b"original").unwrap();
        std::fs::set_permissions(&d, std::fs::Permissions::from_mode(0o555)).unwrap();
        // Root writes anyway; the check only means something for an ordinary user.
        let probe = d.join("probe");
        let unwritable = std::fs::write(&probe, b"").is_err();
        if unwritable {
            assert!(write_atomic(&path, b"new").is_err());
            assert_eq!(std::fs::read(&path).unwrap(), b"original");
        }
        std::fs::set_permissions(&d, std::fs::Permissions::from_mode(0o755)).unwrap();
        let _ = std::fs::remove_file(&probe);
        assert_eq!(names(&d), ["doc.designcraft"]);
        let _ = std::fs::remove_dir_all(&d);
    }

    #[cfg(unix)]
    #[test]
    fn a_symbolic_link_keeps_pointing_at_the_replaced_file() {
        let d = dir("link");
        std::fs::create_dir(d.join("real")).unwrap();
        let target = d.join("real").join("doc.designcraft");
        std::fs::write(&target, b"old").unwrap();
        let link = d.join("link.designcraft");
        std::os::unix::fs::symlink("real/doc.designcraft", &link).unwrap();
        write_atomic(&link, b"new").unwrap();
        assert!(std::fs::symlink_metadata(&link).unwrap().file_type().is_symlink());
        assert_eq!(std::fs::read(&target).unwrap(), b"new");
        assert_eq!(names(&d), ["link.designcraft", "real"]);
        assert_eq!(names(&d.join("real")), ["doc.designcraft"]);
        // A loop of links is an error, not a hang.
        std::os::unix::fs::symlink("b", d.join("a")).unwrap();
        std::os::unix::fs::symlink("a", d.join("b")).unwrap();
        assert!(write_atomic(&d.join("a"), b"x").is_err());
        let _ = std::fs::remove_dir_all(&d);
    }
}
