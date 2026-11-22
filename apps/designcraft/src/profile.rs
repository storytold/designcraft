//! Native settings and recovery paths, including isolated automation profiles and portable installs.

use std::ffi::OsString;
use std::path::{Path, PathBuf};

/// A file with this name next to the executable makes the install portable: settings, logs and
/// recovery data are kept in [`PORTABLE_DATA_DIR`] beside it. The Windows portable zip ships one.
const PORTABLE_MARKER: &str = "portable.txt";
const PORTABLE_DATA_DIR: &str = "DesignCraftData";

pub fn config_dir() -> Option<PathBuf> {
    config_dir_with(std::env::consts::OS, |key| std::env::var_os(key), executable_dir().as_deref())
}

fn config_dir_with(os: &str, env: impl Fn(&str) -> Option<OsString>, exe_dir: Option<&Path>) -> Option<PathBuf> {
    if let Some(root) = root_with(&env, exe_dir) {
        // An explicitly empty override must not fall back to the user's profile.
        return (!root.is_empty()).then(|| PathBuf::from(root));
    }
    match os {
        "macos" => env("HOME").map(|p| PathBuf::from(p).join("Library/Application Support/DesignCraft")),
        "windows" => env("APPDATA").map(|p| PathBuf::from(p).join("DesignCraft")),
        _ => {
            env("XDG_CONFIG_HOME").map(PathBuf::from).or_else(|| env("HOME").map(|p| PathBuf::from(p).join(".config"))).map(|p| p.join("designcraft"))
        }
    }
}

/// The folder that holds settings and recovery data instead of the user's profile:
/// `DESIGNCRAFT_CONFIG_DIR` (empty: none at all), else a portable install's data folder.
fn root_with(env: impl Fn(&str) -> Option<OsString>, exe_dir: Option<&Path>) -> Option<OsString> {
    env("DESIGNCRAFT_CONFIG_DIR").or_else(|| exe_dir.and_then(portable_dir).map(PathBuf::into_os_string))
}

/// `<dir>/DesignCraftData` when `dir` holds [`PORTABLE_MARKER`], where `dir` is the folder the
/// user sees the app in: the executable's, or the one holding `DesignCraft.app` when the executable
/// sits inside a macOS bundle (`DesignCraft.app/Contents/MacOS`, signed, never edited by hand).
/// A folder named like the marker is no marker. A data folder that cannot be written (a portable
/// copy unzipped into Program Files) is no portable install either: settings and recovery would be
/// lost without a word, so the platform paths are used and [`unwritable_portable_dir`] says why.
fn portable_dir(exe_dir: &Path) -> Option<PathBuf> {
    marked_data_dir(exe_dir).filter(|data| writable(data))
}

fn marked_data_dir(exe_dir: &Path) -> Option<PathBuf> {
    let dir = app_bundle_parent(exe_dir).unwrap_or(exe_dir);
    dir.join(PORTABLE_MARKER).is_file().then(|| dir.join(PORTABLE_DATA_DIR))
}

/// Whether `dir` exists, or can be created, and takes a new file: read-only attributes and
/// permissions are only known by trying.
fn writable(dir: &Path) -> bool {
    if std::fs::create_dir_all(dir).is_err() {
        return false;
    }
    let probe = dir.join(format!(".write-test-{}", std::process::id()));
    let ok = std::fs::write(&probe, b"").is_ok();
    let _ = std::fs::remove_file(&probe);
    ok
}

/// The data folder of a portable install that could not be used because it is not writable, for
/// the start-up warning. None when `DESIGNCRAFT_CONFIG_DIR` is set: the portable folder is not
/// wanted then.
pub fn unwritable_portable_dir() -> Option<PathBuf> {
    unwritable_portable_dir_with(|key| std::env::var_os(key), executable_dir().as_deref())
}

/// The folder holding `X.app` when `exe_dir` is `X.app/Contents/MacOS`.
fn app_bundle_parent(exe_dir: &Path) -> Option<&Path> {
    let contents = exe_dir.parent().filter(|_| exe_dir.file_name() == Some("MacOS".as_ref()))?;
    let bundle = contents.parent().filter(|_| contents.file_name() == Some("Contents".as_ref()))?;
    bundle.extension().filter(|ext| *ext == "app").and(bundle.parent())
}

fn unwritable_portable_dir_with(env: impl Fn(&str) -> Option<OsString>, exe_dir: Option<&Path>) -> Option<PathBuf> {
    if env("DESIGNCRAFT_CONFIG_DIR").is_some() {
        return None;
    }
    exe_dir.and_then(marked_data_dir).filter(|data| !writable(data))
}

fn executable_dir() -> Option<PathBuf> {
    std::env::current_exe().ok()?.parent().map(Path::to_path_buf)
}

pub fn recovery_dir() -> Option<PathBuf> {
    recovery_dir_with(root_with(|key| std::env::var_os(key), executable_dir().as_deref()), designcraft_engine::recovery::default_dir)
        .and_then(own_run)
}

/// This run's folder in the recovery folder `root`. A second running DesignCraft has its own, so
/// neither writes over the other's documents nor takes them for a crash's.
fn own_run(root: PathBuf) -> Option<PathBuf> {
    match designcraft_engine::recovery::run_dir(&root) {
        Ok(dir) => Some(dir),
        Err(e) => {
            log::warn!("recovery is unavailable because this run cannot be isolated: {e}");
            None
        }
    }
}

fn recovery_dir_with(root: Option<OsString>, default: impl FnOnce() -> Option<PathBuf>) -> Option<PathBuf> {
    match root {
        Some(root) => (!root.is_empty()).then(|| PathBuf::from(root).join("Recovery")),
        None => default(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn environment(key: &str) -> Option<OsString> {
        match key {
            "HOME" => Some("/user".into()),
            "APPDATA" => Some("/roaming".into()),
            "XDG_CONFIG_HOME" => Some("/xdg-config".into()),
            _ => None,
        }
    }

    #[test]
    fn isolated_profile_overrides_every_platform_and_recovery() {
        for os in ["macos", "windows", "linux"] {
            let dir = config_dir_with(os, |key| if key == "DESIGNCRAFT_CONFIG_DIR" { Some("/isolated".into()) } else { environment(key) }, None);
            assert_eq!(dir, Some(PathBuf::from("/isolated")));
        }
        assert_eq!(recovery_dir_with(Some("/isolated".into()), || Some("/user/recovery".into())), Some(PathBuf::from("/isolated/Recovery")));
    }

    #[test]
    fn empty_override_never_reads_or_writes_the_default_profile() {
        assert_eq!(
            config_dir_with("macos", |key| { if key == "DESIGNCRAFT_CONFIG_DIR" { Some(OsString::new()) } else { environment(key) } }, None),
            None
        );
        assert_eq!(recovery_dir_with(Some(OsString::new()), || Some("/user/recovery".into())), None);
    }

    /// A folder standing in for the executable's; `marker` puts `portable.txt` in it, or not.
    fn install(name: &str, marker: impl FnOnce(&Path)) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("designcraft-profile-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        marker(&dir.join(PORTABLE_MARKER));
        dir
    }

    #[test]
    fn portable_marker_keeps_settings_and_recovery_next_to_the_executable() {
        let exe = install("portable", |marker| std::fs::write(marker, "").unwrap());
        for os in ["macos", "windows", "linux"] {
            assert_eq!(config_dir_with(os, environment, Some(&exe)), Some(exe.join("DesignCraftData")));
        }
        assert_eq!(
            recovery_dir_with(root_with(environment, Some(&exe)), || Some("/user/recovery".into())),
            Some(exe.join("DesignCraftData/Recovery"))
        );
        let _ = std::fs::remove_dir_all(&exe);
    }

    #[test]
    fn inside_a_macos_bundle_the_marker_sits_next_to_the_app() {
        let folder = install("bundle", |marker| std::fs::write(marker, "").unwrap());
        let exe = folder.join("DesignCraft.app/Contents/MacOS");
        std::fs::create_dir_all(&exe).unwrap();
        assert_eq!(config_dir_with("macos", environment, Some(&exe)), Some(folder.join("DesignCraftData")));
        // a marker dropped inside the bundle is not looked at
        std::fs::remove_file(folder.join(PORTABLE_MARKER)).unwrap();
        std::fs::write(exe.join(PORTABLE_MARKER), "").unwrap();
        assert_eq!(config_dir_with("macos", environment, Some(&exe)), Some("/user/Library/Application Support/DesignCraft".into()));
        let _ = std::fs::remove_dir_all(&folder);
    }

    #[test]
    fn without_a_marker_file_the_platform_paths_stay() {
        let plain = install("plain", |_| {});
        let folder = install("marker-folder", |marker| std::fs::create_dir(marker).unwrap());
        for exe in [&plain, &folder] {
            assert_eq!(config_dir_with("windows", environment, Some(exe)), Some("/roaming/DesignCraft".into()));
            assert_eq!(recovery_dir_with(root_with(environment, Some(exe)), || Some("/existing/recovery".into())), Some("/existing/recovery".into()));
        }
        let _ = std::fs::remove_dir_all(&plain);
        let _ = std::fs::remove_dir_all(&folder);
    }

    /// Unix only: permissions are how a test makes a folder read-only. Root writes through them,
    /// so under root there is nothing to show and the test returns.
    #[cfg(unix)]
    #[test]
    fn a_portable_install_that_cannot_write_its_data_uses_the_platform_paths() {
        use std::os::unix::fs::PermissionsExt;
        let set_mode = |dir: &Path, mode| std::fs::set_permissions(dir, std::fs::Permissions::from_mode(mode)).unwrap();
        // DesignCraftData cannot be created
        let exe = install("read-only", |marker| std::fs::write(marker, "").unwrap());
        set_mode(&exe, 0o555);
        if std::fs::write(exe.join("root"), "").is_ok() {
            eprintln!("running as root: read-only folders are writable, nothing to test");
            set_mode(&exe, 0o755);
            let _ = std::fs::remove_dir_all(&exe);
            return;
        }
        assert_eq!(config_dir_with("windows", environment, Some(&exe)), Some("/roaming/DesignCraft".into()));
        assert_eq!(recovery_dir_with(root_with(environment, Some(&exe)), || Some("/user/recovery".into())), Some("/user/recovery".into()));
        assert_eq!(unwritable_portable_dir_with(environment, Some(&exe)), Some(exe.join("DesignCraftData")));
        set_mode(&exe, 0o755);
        // DesignCraftData is there, but nothing can be written in it
        let data = exe.join(PORTABLE_DATA_DIR);
        std::fs::create_dir(&data).unwrap();
        set_mode(&data, 0o555);
        assert_eq!(config_dir_with("windows", environment, Some(&exe)), Some("/roaming/DesignCraft".into()));
        assert_eq!(unwritable_portable_dir_with(environment, Some(&exe)), Some(data.clone()));
        // the override still wins, so there is nothing to warn about
        assert_eq!(
            unwritable_portable_dir_with(|key| if key == "DESIGNCRAFT_CONFIG_DIR" { Some("/isolated".into()) } else { None }, Some(&exe)),
            None
        );
        set_mode(&data, 0o755);
        assert_eq!(config_dir_with("windows", environment, Some(&exe)), Some(data));
        assert_eq!(unwritable_portable_dir_with(environment, Some(&exe)), None);
        let _ = std::fs::remove_dir_all(&exe);
    }

    #[test]
    fn isolated_profile_wins_over_a_portable_install() {
        let exe = install("override", |marker| std::fs::write(marker, "").unwrap());
        let isolated = |key: &str| if key == "DESIGNCRAFT_CONFIG_DIR" { Some("/isolated".into()) } else { environment(key) };
        assert_eq!(config_dir_with("windows", isolated, Some(&exe)), Some("/isolated".into()));
        assert_eq!(recovery_dir_with(root_with(isolated, Some(&exe)), || None), Some("/isolated/Recovery".into()));
        let empty = |key: &str| if key == "DESIGNCRAFT_CONFIG_DIR" { Some(OsString::new()) } else { environment(key) };
        assert_eq!(config_dir_with("windows", empty, Some(&exe)), None);
        assert_eq!(recovery_dir_with(root_with(empty, Some(&exe)), || Some("/user/recovery".into())), None);
        let _ = std::fs::remove_dir_all(&exe);
    }

    #[test]
    fn unset_override_preserves_existing_platform_paths() {
        assert_eq!(config_dir_with("macos", environment, None), Some("/user/Library/Application Support/DesignCraft".into()));
        assert_eq!(config_dir_with("windows", environment, None), Some("/roaming/DesignCraft".into()));
        assert_eq!(config_dir_with("linux", environment, None), Some("/xdg-config/designcraft".into()));
        assert_eq!(
            config_dir_with("linux", |key| if key == "HOME" { environment(key) } else { None }, None),
            Some("/user/.config/designcraft".into())
        );
        assert_eq!(recovery_dir_with(None, || Some("/existing/recovery".into())), Some("/existing/recovery".into()));
    }

    #[test]
    fn a_run_has_its_own_recovery_folder_when_one_can_be_made() {
        let dir = std::env::temp_dir().join(format!("designcraft-profile-run-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let root = dir.join("Recovery");
        assert_eq!(own_run(root.clone()).as_deref().and_then(|dir| dir.parent()), Some(root.as_path()));
        // Failure must not fall back to the shared root.
        let blocked = dir.join("blocked");
        std::fs::write(&blocked, "a file where the folder should be").unwrap();
        assert_eq!(own_run(blocked.join("Recovery")), None, "isolation failure must not select the shared root");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn an_exhausted_run_namespace_never_falls_back_to_shared_recovery() {
        let root = std::env::temp_dir().join(format!("designcraft-profile-exhausted-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).unwrap();
        for n in 0..64 {
            std::fs::create_dir(root.join(format!("run-{}-{n}", std::process::id()))).unwrap();
        }
        assert_eq!(own_run(root.clone()), None, "a shared root would restore inter-process UID collisions");
        let _ = std::fs::remove_dir_all(&root);
    }
}
