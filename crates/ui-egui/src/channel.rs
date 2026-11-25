//! The window and About identity of the selected channel: Stable, or the Launcher's nightly-v1.
//!
//! The desktop selects it once, before any window exists (`main.rs`), with the version the build
//! script baked in. The web build never selects one and stays stable. Every OS name (Wayland app
//! id, Windows AUMID, About, captions) comes from here, so the two channels cannot disagree.

use std::sync::OnceLock;
use std::sync::atomic::{AtomicU8, Ordering};

/// The stable `.desktop` file name, hicolor icon name and Windows identity.
pub const APP_ID: &str = "ai.storyteller.designcraft";
/// The nightly package, bundle and Launcher identity (`ai.storyteller.designcraft.nightly`).
pub const NIGHTLY_PACKAGE_ID: &str = "ai.storyteller.designcraft.nightly";
/// The nightly window/Wayland app id and Windows AUMID; deliberately not the package id.
pub const NIGHTLY_APP_ID: &str = "ai.storyteller.designcraft.nightly-v1";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Channel {
    Stable,
    NightlyV1,
}

impl Channel {
    pub fn is_nightly(self) -> bool {
        self == Self::NightlyV1
    }

    /// The OS window identity: stable's `.desktop` name, or the nightly-v1 window id.
    pub fn app_id(self) -> &'static str {
        match self {
            Self::Stable => APP_ID,
            Self::NightlyV1 => NIGHTLY_APP_ID,
        }
    }

    /// The name About, captions and dialogs show.
    pub fn name(self) -> &'static str {
        match self {
            Self::Stable => "DesignCraft",
            Self::NightlyV1 => "DesignCraft Nightly",
        }
    }
}

/// 0 = not selected (stable), 1 = stable, 2 = nightly-v1.
static SELECTED: AtomicU8 = AtomicU8::new(0);
static IDENTITY: OnceLock<(&'static str, Option<&'static str>)> = OnceLock::new();

/// Select the channel once, with the version and (nightly only) commit the build baked in.
pub fn select(channel: Channel, version: &'static str, commit: Option<&'static str>) -> Result<(), String> {
    if version.is_empty() {
        return Err("the build channel needs a version".into());
    }
    if channel.is_nightly() != commit.is_some() {
        return Err("only a nightly channel carries a source commit".into());
    }
    let value = if channel.is_nightly() { 2 } else { 1 };
    SELECTED.compare_exchange(0, value, Ordering::AcqRel, Ordering::Acquire).map_err(|_| "the build channel was already selected".to_string())?;
    IDENTITY.set((version, commit)).map_err(|_| "the build channel was already selected".to_string())
}

pub fn current() -> Channel {
    if SELECTED.load(Ordering::Acquire) == 2 { Channel::NightlyV1 } else { Channel::Stable }
}

pub fn is_nightly() -> bool {
    current().is_nightly()
}

pub fn name() -> &'static str {
    current().name()
}

/// The exact build version: the workspace version for stable, the feed identity for nightly.
pub fn version() -> &'static str {
    IDENTITY.get().map_or(env!("CARGO_PKG_VERSION"), |(version, _)| *version)
}

/// The 40-hex source commit of a nightly build, if the build was given one.
pub fn commit() -> Option<&'static str> {
    IDENTITY.get().and_then(|(_, commit)| *commit)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A version and a commit must match the channel, and the errors are returned rather than
    /// stored, so a refusal leaves the selection untouched.
    #[test]
    fn select_refuses_a_missing_version_and_a_commit_that_does_not_match_the_channel() {
        assert_eq!(select(Channel::Stable, "", None).unwrap_err(), "the build channel needs a version");
        assert_eq!(
            select(Channel::Stable, "0.2.1", Some("0123456789abcdef0123456789abcdef01234567")).unwrap_err(),
            "only a nightly channel carries a source commit"
        );
        assert_eq!(
            select(Channel::NightlyV1, "0.2.1-nightly.20261124.17.1.gabcdef012345", None).unwrap_err(),
            "only a nightly channel carries a source commit"
        );
        assert_eq!(current(), Channel::Stable, "a refused selection leaves the channel as it was");
    }

    #[test]
    fn the_default_is_the_stable_identity() {
        assert_eq!(current(), Channel::Stable, "the web build and tests never select a channel");
        assert_eq!(name(), "DesignCraft");
        assert_eq!(version(), env!("CARGO_PKG_VERSION"));
        assert_eq!(commit(), None);
    }

    #[test]
    fn the_nightly_identities_are_distinct_from_stable() {
        assert_ne!(Channel::NightlyV1.app_id(), Channel::Stable.app_id());
        assert_ne!(NIGHTLY_PACKAGE_ID, NIGHTLY_APP_ID);
        assert_eq!(Channel::Stable.app_id(), APP_ID);
        assert_eq!(Channel::NightlyV1.name(), "DesignCraft Nightly");
        assert!(Channel::NightlyV1.is_nightly() && !Channel::Stable.is_nightly());
    }
}
