//! Native settings and recovery paths, including isolated automation profiles.

use std::ffi::OsString;
use std::path::PathBuf;

pub fn config_dir() -> Option<PathBuf> {
    config_dir_with(std::env::consts::OS, |key| std::env::var_os(key))
}

fn config_dir_with(os: &str, env: impl Fn(&str) -> Option<OsString>) -> Option<PathBuf> {
    if let Some(root) = env("DESIGNCRAFT_CONFIG_DIR") {
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

pub fn recovery_dir() -> Option<PathBuf> {
    recovery_dir_with(std::env::var_os("DESIGNCRAFT_CONFIG_DIR"), designcraft_engine::recovery::default_dir)
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
            let dir = config_dir_with(os, |key| if key == "DESIGNCRAFT_CONFIG_DIR" { Some("/isolated".into()) } else { environment(key) });
            assert_eq!(dir, Some(PathBuf::from("/isolated")));
        }
        assert_eq!(recovery_dir_with(Some("/isolated".into()), || Some("/user/recovery".into())), Some(PathBuf::from("/isolated/Recovery")));
    }

    #[test]
    fn empty_override_never_reads_or_writes_the_default_profile() {
        assert_eq!(config_dir_with("macos", |key| { if key == "DESIGNCRAFT_CONFIG_DIR" { Some(OsString::new()) } else { environment(key) } }), None);
        assert_eq!(recovery_dir_with(Some(OsString::new()), || Some("/user/recovery".into())), None);
    }

    #[test]
    fn unset_override_preserves_existing_platform_paths() {
        assert_eq!(config_dir_with("macos", environment), Some("/user/Library/Application Support/DesignCraft".into()));
        assert_eq!(config_dir_with("windows", environment), Some("/roaming/DesignCraft".into()));
        assert_eq!(config_dir_with("linux", environment), Some("/xdg-config/designcraft".into()));
        assert_eq!(config_dir_with("linux", |key| if key == "HOME" { environment(key) } else { None }), Some("/user/.config/designcraft".into()));
        assert_eq!(recovery_dir_with(None, || Some("/existing/recovery".into())), Some("/existing/recovery".into()));
    }
}
