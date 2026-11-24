//! App-owned nightly profile fences; no stable fallback, migration or process environment changes.

use std::ffi::OsString;
use std::path::{Component, Path, PathBuf};

use crate::profile::{PORTABLE_DATA_DIR, PORTABLE_MARKER};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Platform {
    Windows,
    Linux,
    Mac,
}

impl Platform {
    fn current() -> Result<Self, String> {
        match std::env::consts::OS {
            "windows" => Ok(Self::Windows),
            "linux" => Ok(Self::Linux),
            "macos" => Ok(Self::Mac),
            _ => Err("nightly-v1 is not defined for this operating system".into()),
        }
    }

    fn ignores_case(self) -> bool {
        self != Self::Linux
    }
}

fn absolute(path: PathBuf, label: &str) -> Result<PathBuf, String> {
    if !path.is_absolute() || path.components().any(|c| matches!(c, Component::ParentDir)) {
        return Err(format!("{label} must be absolute and contain no parent traversal"));
    }
    Ok(path)
}

fn required_base(env: &impl Fn(&str) -> Option<OsString>, key: &str) -> Result<PathBuf, String> {
    let value = env(key).filter(|v| !v.is_empty()).ok_or_else(|| format!("cannot establish the stable {key} boundary"))?;
    absolute(PathBuf::from(value), key)
}

fn xdg_base(env: &impl Fn(&str) -> Option<OsString>, key: &str, fallback: &str) -> Result<PathBuf, String> {
    match env(key).filter(|v| !v.is_empty()) {
        Some(value) => absolute(PathBuf::from(value), key),
        None => Ok(required_base(env, "HOME")?.join(fallback)),
    }
}

/// Resolve existing ancestors before creating anything. A dangling link or inaccessible ancestor
/// is not a missing directory that may be replaced or followed later.
fn canonical_future(path: &Path) -> Result<PathBuf, String> {
    let mut parent = path;
    let mut missing = Vec::new();
    loop {
        match std::fs::symlink_metadata(parent) {
            Ok(_) => break,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                missing.push(parent.file_name().ok_or("the profile root has no existing ancestor")?.to_os_string());
                parent = parent.parent().ok_or("the profile root has no existing ancestor")?;
            }
            Err(error) => return Err(format!("cannot inspect the profile boundary: {error}")),
        }
    }
    let mut resolved = std::fs::canonicalize(parent).map_err(|e| format!("cannot resolve the profile boundary: {e}"))?;
    for part in missing.into_iter().rev() {
        resolved.push(part);
    }
    Ok(resolved)
}

fn overlaps(a: &Path, b: &Path, platform: Platform) -> bool {
    let parts = |p: &Path| {
        p.components()
            .map(|c| {
                let value = c.as_os_str().to_string_lossy().into_owned();
                if platform.ignores_case() { value.to_lowercase() } else { value }
            })
            .collect::<Vec<_>>()
    };
    let (a, b) = (parts(a), parts(b));
    a.starts_with(&b) || b.starts_with(&a)
}

fn installation_boundary(platform: Platform, exe: &Path) -> Result<PathBuf, String> {
    let parent = exe.parent().ok_or("cannot establish the installation boundary")?;
    if platform == Platform::Mac {
        // A profile in Resources is still inside the versioned install, not beside MacOS.
        if let Some(bundle) = exe.ancestors().find(|p| p.extension().is_some_and(|ext| ext.eq_ignore_ascii_case("app"))) {
            return Ok(bundle.to_path_buf());
        }
    }
    Ok(parent.to_path_buf())
}

fn forbidden_roots(platform: Platform, env: &impl Fn(&str) -> Option<OsString>, exe: &Path) -> Result<Vec<PathBuf>, String> {
    let mut roots = match platform {
        Platform::Windows => {
            let mut roots = ["APPDATA", "LOCALAPPDATA"]
                .into_iter()
                .map(|key| required_base(env, key).map(|p| p.join("DesignCraft")))
                .collect::<Result<Vec<_>, _>>()?;
            // The MSI's per-machine folder (install-policy.wxi, ProgramFiles6432Folder) and a
            // per-user install folder. A custom folder is not known here; the executable's own
            // folder is fenced below.
            roots.push(required_base(env, "LOCALAPPDATA")?.join("Programs").join("ArtCraft").join("DesignCraft"));
            for key in ["ProgramW6432", "ProgramFiles", "ProgramFiles(x86)"] {
                if let Some(base) = env(key).filter(|v| !v.is_empty()) {
                    roots.push(absolute(PathBuf::from(base), key)?.join("ArtCraft").join("DesignCraft"));
                }
            }
            roots
        }
        Platform::Linux => {
            let mut roots =
                [("XDG_CONFIG_HOME", ".config"), ("XDG_DATA_HOME", ".local/share"), ("XDG_CACHE_HOME", ".cache"), ("XDG_STATE_HOME", ".local/state")]
                    .into_iter()
                    .map(|(key, fallback)| xdg_base(env, key, fallback).map(|p| p.join("designcraft")))
                    .collect::<Result<Vec<_>, _>>()?;
            roots.extend(["/usr/bin/designcraft", "/usr/local/bin/designcraft", "/opt/designcraft"].map(PathBuf::from));
            // The stable Flatpak keeps its data in its own per-app folder.
            roots.push(required_base(env, "HOME")?.join(".var/app/ai.storyteller.designcraft"));
            roots
        }
        Platform::Mac => {
            let home = required_base(env, "HOME")?;
            let library = home.join("Library");
            vec![
                library.join("Application Support/DesignCraft"),
                library.join("Caches/DesignCraft"),
                library.join("Caches/ai.storyteller.designcraft"),
                library.join("Preferences/DesignCraft.plist"),
                library.join("Preferences/ai.storyteller.designcraft.plist"),
                library.join("Saved Application State/ai.storyteller.designcraft.savedState"),
                home.join("Applications/DesignCraft.app"),
                PathBuf::from("/Applications/DesignCraft.app"),
            ]
        }
    };
    roots.push(installation_boundary(platform, exe)?);
    roots.into_iter().map(|p| canonical_future(&p)).collect()
}

fn private_subdirectory(root: &Path, name: impl AsRef<Path>, label: &str) -> Result<PathBuf, String> {
    let chosen = canonical_future(&absolute(root.to_path_buf(), "the nightly profile root")?)?;
    let child = canonical_future(&chosen.join(name))?;
    if child == chosen || !child.starts_with(&chosen) {
        return Err(format!("the nightly {label} must stay inside its selected profile"));
    }
    Ok(child)
}

/// Every existing entry directly in the profile (logs, settings and so on) must resolve inside it,
/// so a link there cannot point Nightly's writes at another app's data.
fn entries_stay_inside(root: &Path) -> Result<(), String> {
    let entries = match std::fs::read_dir(root) {
        Ok(entries) => entries,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(()),
        Err(error) => return Err(format!("cannot inspect the nightly profile: {error}")),
    };
    for entry in entries {
        let entry = entry.map_err(|e| format!("cannot inspect the nightly profile: {e}"))?;
        private_subdirectory(root, entry.file_name(), "profile entry")?;
    }
    Ok(())
}

/// Recognise the shared portable data namespace in both the supplied and resolved path.
/// This is not an inventory of arbitrary custom stable overrides or a Launcher ownership proof.
fn is_portable_layout(path: &Path, platform: Platform) -> bool {
    // A folder under a portable copy belongs to that copy too. Above the chosen folder itself, a
    // marker only counts beside a DesignCraft executable, so an unrelated portable.txt higher up
    // (in a user folder or at a drive root) does not refuse the profile.
    path.ancestors().enumerate().any(|(depth, ancestor)| {
        ancestor.join(PORTABLE_MARKER).is_file()
            && (depth == 0 || ["designcraft.exe", "designcraft", "DesignCraft.app"].iter().any(|exe| ancestor.join(exe).exists()))
    }) || path.components().any(|component| match component {
        Component::Normal(name) if platform.ignores_case() => name.to_string_lossy().eq_ignore_ascii_case(PORTABLE_DATA_DIR),
        Component::Normal(name) => name == PORTABLE_DATA_DIR,
        _ => false,
    })
}

/// The writable hook is the app's existing probe (`profile::ensure_writable`). Keeping it injected
/// lets filesystem boundary tests exercise the real policy without starting the app or changing
/// global state.
pub fn validate(
    root: &Path,
    env: impl Fn(&str) -> Option<OsString>,
    exe: Option<&Path>,
    writable: impl FnOnce(&Path) -> std::io::Result<()>,
) -> Result<PathBuf, String> {
    validate_for(Platform::current()?, root, env, exe, writable)
}

fn validate_for(
    platform: Platform,
    root: &Path,
    env: impl Fn(&str) -> Option<OsString>,
    exe: Option<&Path>,
    writable: impl FnOnce(&Path) -> std::io::Result<()>,
) -> Result<PathBuf, String> {
    absolute(root.to_path_buf(), "the nightly profile root")?;
    if env("DESIGNCRAFT_CONFIG_DIR").is_some_and(|v| !v.is_empty()) {
        // The stable override would silently point every write at another app's data.
        return Err("nightly-v1 cannot be combined with DESIGNCRAFT_CONFIG_DIR".into());
    }
    // The nightly bundle's own sandbox is the one Flatpak that may reach this code: its per-app
    // home is private. Any other id means a native package (or the stable bundle) started here,
    // whose runtime socket namespace is not this profile's.
    let foreign_sandbox =
        env("FLATPAK_ID").filter(|value| !value.is_empty()).is_some_and(|id| id != designcraft_ui_egui::channel::NIGHTLY_PACKAGE_ID);
    if platform != Platform::Windows && foreign_sandbox {
        return Err("nightly-v1 starts only in its own Flatpak bundle, not inside another app's sandbox".into());
    }
    let chosen = canonical_future(root)?;
    if is_portable_layout(root, platform) || is_portable_layout(&chosen, platform) {
        return Err("the nightly root must not use a portable DesignCraft installation or its DesignCraftData settings folder".into());
    }
    let exe = absolute(exe.ok_or("cannot establish the installation boundary")?.to_path_buf(), "the executable")?;
    let forbidden = forbidden_roots(platform, &env, &exe)?;
    if forbidden.iter().any(|stable| overlaps(&chosen, stable, platform)) {
        return Err("the nightly root must not overlap stable data or an installation".into());
    }
    // Existing aliases under the profile must not redirect a write outside it.
    entries_stay_inside(&chosen)?;
    // Recovery can read, adopt, overwrite and sweep entries. Never let its alias leave this
    // private profile, including toward a stable portable directory not in our known roots.
    private_subdirectory(&chosen, "Recovery", "recovery folder")?;
    let mut create = std::fs::DirBuilder::new();
    create.recursive(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::DirBuilderExt as _;
        create.mode(0o700);
    }
    create.create(&chosen).map_err(|e| format!("cannot create the nightly profile: {e}"))?;
    let confirmed = std::fs::canonicalize(&chosen).map_err(|e| format!("cannot resolve the created nightly profile: {e}"))?;
    if confirmed != chosen || forbidden.iter().any(|stable| overlaps(&confirmed, stable, platform)) {
        return Err("the nightly profile changed while it was being validated".into());
    }
    private_subdirectory(&confirmed, "Recovery", "recovery folder")?;
    entries_stay_inside(&confirmed)?;
    writable(&confirmed).map_err(|e| format!("the nightly profile is not writable: {e}"))?;
    Ok(confirmed)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeMap;
    use std::sync::atomic::{AtomicU64, Ordering};

    struct Fixture {
        root: PathBuf,
        env: BTreeMap<String, OsString>,
        exe: PathBuf,
    }

    impl Fixture {
        fn new() -> Self {
            static NEXT: AtomicU64 = AtomicU64::new(0);
            let root = loop {
                let candidate =
                    std::env::temp_dir().join(format!("designcraft-profile-policy-{}-{}", std::process::id(), NEXT.fetch_add(1, Ordering::Relaxed)));
                match std::fs::create_dir(&candidate) {
                    Ok(()) => break std::fs::canonicalize(candidate).unwrap(),
                    Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => continue,
                    Err(error) => panic!("cannot create owned fixture: {error}"),
                }
            };
            let home = root.join("home");
            let exe = root.join("install/designcraft");
            std::fs::create_dir_all(&home).unwrap();
            std::fs::create_dir_all(exe.parent().unwrap()).unwrap();
            std::fs::write(&exe, b"fixture executable, never run").unwrap();
            let env = [("HOME", home), ("APPDATA", root.join("roaming")), ("LOCALAPPDATA", root.join("local"))]
                .into_iter()
                .map(|(key, value)| (key.into(), value.into_os_string()))
                .collect();
            Self { root, env, exe }
        }

        fn get(&self, key: &str) -> Option<OsString> {
            self.env.get(key).cloned()
        }

        fn check(&self, platform: Platform, root: &Path) -> Result<PathBuf, String> {
            validate_for(
                platform,
                root,
                |key| self.get(key),
                Some(&self.exe),
                |dir| {
                    let probe = dir.join("exclusive-test-probe");
                    let file = std::fs::OpenOptions::new().write(true).create_new(true).open(&probe)?;
                    drop(file);
                    std::fs::remove_file(probe)
                },
            )
        }

        fn home(&self) -> PathBuf {
            PathBuf::from(self.get("HOME").unwrap())
        }
    }

    impl Drop for Fixture {
        fn drop(&mut self) {
            // Only the directory successfully created by this fixture is owned for removal.
            std::fs::remove_dir_all(&self.root).unwrap();
        }
    }

    #[test]
    fn every_stable_data_root_is_refused_on_every_platform() {
        let fixture = Fixture::new();
        let home = fixture.home();
        let cases = [
            (Platform::Windows, fixture.root.join("roaming/DesignCraft")),
            (Platform::Windows, fixture.root.join("local/DesignCraft")),
            (Platform::Windows, fixture.root.join("local/Programs/ArtCraft/DesignCraft")),
            (Platform::Linux, home.join(".config/designcraft")),
            (Platform::Linux, home.join(".local/share/designcraft")),
            (Platform::Linux, home.join(".cache/designcraft")),
            (Platform::Linux, home.join(".local/state/designcraft")),
            (Platform::Linux, home.join(".var/app/ai.storyteller.designcraft")),
            (Platform::Linux, PathBuf::from("/opt/designcraft")),
            (Platform::Mac, home.join("Library/Application Support/DesignCraft")),
            (Platform::Mac, home.join("Library/Caches/DesignCraft")),
            (Platform::Mac, home.join("Library/Preferences/ai.storyteller.designcraft.plist")),
            (Platform::Mac, home.join("Library/Saved Application State/ai.storyteller.designcraft.savedState")),
            (Platform::Mac, home.join("Applications/DesignCraft.app")),
        ];
        for (platform, stable) in cases {
            for candidate in [stable.clone(), stable.join("nested nightly")] {
                let probed = std::cell::Cell::new(false);
                let result = validate_for(
                    platform,
                    &candidate,
                    |key| fixture.get(key),
                    Some(&fixture.exe),
                    |_| {
                        probed.set(true);
                        Ok(())
                    },
                );
                assert!(result.is_err(), "{platform:?} accepted {candidate:?}");
                assert!(!probed.get(), "a refused root must not be probed for writing");
            }
        }
    }

    #[test]
    fn the_installation_folder_and_grandparent_overlaps_are_refused() {
        let fixture = Fixture::new();
        let install = fixture.exe.parent().unwrap().to_path_buf();
        for platform in [Platform::Windows, Platform::Linux, Platform::Mac] {
            assert!(fixture.check(platform, &install).is_err(), "the install folder itself");
            assert!(fixture.check(platform, &install.join("profile")).is_err(), "a folder inside the install");
            assert!(fixture.check(platform, &fixture.root.join("nightly")).is_ok(), "a sibling of the install is private");
        }
    }

    #[test]
    fn prefix_and_case_lookalikes_are_not_stable_overlaps() {
        let fixture = Fixture::new();
        for platform in [Platform::Windows, Platform::Linux, Platform::Mac] {
            // `DesignCraftBackups` only starts with the stable folder's name.
            assert!(fixture.check(platform, &fixture.home().join("Library/Application Support/DesignCraftBackups/nightly")).is_ok());
            assert!(fixture.check(platform, &fixture.root.join("data-craft/nightly")).is_ok());
        }
        // Case folding differs by platform: Windows and macOS fold, Linux does not.
        let folded = fixture.root.join("roaming/designcraft");
        assert!(fixture.check(Platform::Windows, &folded).is_err());
        assert!(fixture.check(Platform::Linux, &folded).is_ok(), "a different case is a different Linux path");
    }

    #[test]
    fn portable_settings_from_another_install_are_refused_before_any_probe() {
        let fixture = Fixture::new();
        let stable_install = fixture.root.join("stable portable install");
        let stable_data = stable_install.join(PORTABLE_DATA_DIR);
        std::fs::create_dir_all(&stable_data).unwrap();
        std::fs::write(stable_install.join(PORTABLE_MARKER), b"owned portable fixture").unwrap();
        std::fs::write(stable_data.join("ui.json"), b"stable settings must not change").unwrap();
        for platform in [Platform::Windows, Platform::Linux, Platform::Mac] {
            for candidate in [&stable_data, &stable_data.join("nested nightly")] {
                let probed = std::cell::Cell::new(false);
                let result = validate_for(
                    platform,
                    candidate,
                    |key| fixture.get(key),
                    Some(&fixture.exe),
                    |_| {
                        probed.set(true);
                        Ok(())
                    },
                );
                assert!(result.is_err(), "a different portable install is not a private Nightly profile");
                assert!(!probed.get(), "reject portable data before a writable probe");
                assert!(!stable_data.join("nested nightly").exists());
                assert_eq!(std::fs::read(stable_data.join("ui.json")).unwrap(), b"stable settings must not change");
            }
        }
        let private = fixture.root.join("DesignCraftData-backup-name/nightly");
        assert!(fixture.check(Platform::Linux, &private).is_ok(), "a merely similar name is not a portable settings component");
    }

    #[test]
    fn the_stable_settings_override_and_a_flatpak_instance_are_refused() {
        let fixture = Fixture::new();
        let target = fixture.root.join("nightly");
        for value in ["/somewhere/else", ""] {
            let env = |key: &str| if key == "DESIGNCRAFT_CONFIG_DIR" { Some(OsString::from(value)) } else { fixture.get(key) };
            let result = validate_for(Platform::Linux, &target, env, Some(&fixture.exe), |_| Ok(()));
            if value.is_empty() {
                assert!(result.is_ok(), "an empty override is not a stable data redirection");
            } else {
                let error = result.unwrap_err();
                assert!(error.contains("DESIGNCRAFT_CONFIG_DIR"), "{error}");
            }
        }
        let flatpak = |key: &str| if key == "FLATPAK_ID" { Some(OsString::from("ai.storyteller.designcraft")) } else { fixture.get(key) };
        assert!(validate_for(Platform::Linux, &target, flatpak, Some(&fixture.exe), |_| Ok(())).is_err());
        assert!(validate_for(Platform::Windows, &target, flatpak, Some(&fixture.exe), |_| Ok(())).is_ok(), "Windows has no Flatpak namespace");
        // The nightly bundle's own sandbox is private; a lookalike id is still another app's.
        let nightly_bundle =
            |key: &str| if key == "FLATPAK_ID" { Some(OsString::from(designcraft_ui_egui::channel::NIGHTLY_PACKAGE_ID)) } else { fixture.get(key) };
        assert!(validate_for(Platform::Linux, &target, nightly_bundle, Some(&fixture.exe), |_| Ok(())).is_ok());
        let lookalike =
            |key: &str| if key == "FLATPAK_ID" { Some(OsString::from("ai.storyteller.designcraft.nightly-v1")) } else { fixture.get(key) };
        assert!(validate_for(Platform::Linux, &target, lookalike, Some(&fixture.exe), |_| Ok(())).is_err());
    }

    #[cfg(unix)]
    #[test]
    fn recovery_aliases_cannot_redirect_to_stable_or_outward_data() {
        let fixture = Fixture::new();
        let stable = fixture.home().join(".local/share/designcraft/Recovery");
        std::fs::create_dir_all(&stable).unwrap();
        std::fs::write(stable.join("stable-autosave"), b"stable recovery bytes").unwrap();
        for target in [stable.clone(), fixture.root.to_path_buf(), fixture.root.join("Recovery/../../elsewhere")] {
            let profile = fixture.root.join(format!("profile-{}", target.file_name().unwrap().to_string_lossy().replace('.', "_")));
            std::fs::create_dir_all(&profile).unwrap();
            std::os::unix::fs::symlink(&target, profile.join("Recovery")).unwrap();
            assert!(fixture.check(Platform::Linux, &profile).is_err(), "{target:?}");
            assert_eq!(std::fs::read(stable.join("stable-autosave")).unwrap(), b"stable recovery bytes");
        }
    }

    #[cfg(unix)]
    #[test]
    fn a_created_profile_is_private_and_a_failed_write_probe_never_falls_back() {
        let fixture = Fixture::new();
        let probe = fixture.root.join("profile for probe");
        let error = validate_for(Platform::Linux, &probe, |key| fixture.get(key), Some(&fixture.exe), |_| Err(std::io::Error::other("read-only")))
            .unwrap_err();
        assert!(error.contains("not writable"), "{error}");
        use std::os::unix::fs::PermissionsExt;
        assert_eq!(std::fs::metadata(&probe).unwrap().permissions().mode() & 0o777, 0o700);
    }

    #[test]
    fn relative_and_parent_traversing_are_refused_and_link_ancestors_are_resolved() {
        let fixture = Fixture::new();
        for candidate in [PathBuf::from("relative/nightly"), fixture.root.join("a/../nightly")] {
            assert!(fixture.check(Platform::Linux, &candidate).is_err(), "{candidate:?}");
        }
        #[cfg(unix)]
        {
            let elsewhere = fixture.root.join("elsewhere");
            std::fs::create_dir_all(&elsewhere).unwrap();
            std::os::unix::fs::symlink(&elsewhere, fixture.root.join("linked")).unwrap();
            // `canonical_future` resolves the existing ancestors before anything is created, so a
            // root reached through a link is validated and created at the link's real location.
            // That location is what the fence checks, and it is where every write then lands.
            let resolved = fixture.check(Platform::Linux, &fixture.root.join("linked/nightly")).unwrap();
            assert_eq!(resolved, std::fs::canonicalize(elsewhere.join("nightly")).unwrap());
            assert!(resolved.is_dir());
            assert_eq!(std::fs::canonicalize(fixture.root.join("linked")).unwrap(), elsewhere);
        }
    }

    #[test]
    fn a_valid_profile_is_returned_resolved_and_created() {
        let fixture = Fixture::new();
        let chosen = fixture.root.join("launcher data/profiles/designcraft");
        let resolved = fixture.check(Platform::Linux, &chosen).unwrap();
        assert_eq!(resolved, std::fs::canonicalize(&chosen).unwrap());
        assert!(resolved.is_dir());
    }
}
