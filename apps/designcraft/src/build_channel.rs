//! The build-time nightly identity shared by the build script and its pure regression tests.
pub fn is_nightly_version(version: &str) -> bool {
    let Some((base, suffix)) = version.split_once("-nightly.") else { return false };
    let digits = |s: &str| !s.is_empty() && s.bytes().all(|b| b.is_ascii_digit());
    let base: Vec<_> = base.split('.').collect();
    let parts: Vec<_> = suffix.split('.').collect();
    base.len() == 3
        && base.iter().all(|s| digits(s))
        && parts.len() == 4
        && parts[0].len() == 8
        && digits(parts[0])
        && digits(parts[1])
        && digits(parts[2])
        && parts[3].strip_prefix('g').is_some_and(|sha| sha.len() == 12 && sha.bytes().all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b)))
}

/// The `X.Y.Z-nightly.YYYYMMDD.<run>.<attempt>.g<sha12>` feed version's date digits, or `None`.
/// Used by the build script; the app itself only needs the channel and icon rules.
#[allow(dead_code)]
pub fn nightly_date_digits(version: &str) -> Option<&str> {
    let suffix = version.split_once("-nightly.")?.1;
    suffix.split('.').next().filter(|date| date.len() == 8)
}

/// The commit the feed version names, without its `g` prefix.
#[allow(dead_code)]
pub fn nightly_commit(version: &str) -> Option<&str> {
    version.rsplit_once(".g").map(|(_, sha)| sha).filter(|sha| sha.len() == 12)
}

pub fn windows_icon_path(nightly: bool) -> &'static str {
    if nightly { "../../assets/app-icon/designcraft-nightly.ico" } else { "../../assets/app-icon/designcraft.ico" }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn windows_resource_uses_the_flavor_specific_icon_without_changing_stable() {
        assert_eq!(windows_icon_path(false), "../../assets/app-icon/designcraft.ico");
        assert_eq!(windows_icon_path(true), "../../assets/app-icon/designcraft-nightly.ico");
    }

    #[test]
    fn explicit_feed_identity_is_required_for_a_nightly_build() {
        assert!(is_nightly_version("0.2.1-nightly.20261124.17.1.gabcdef012345"));
        for invalid in [
            "",
            "0.2.1",
            "0.2.1-nightly",
            "0.2.1-nightly.20261124",
            "0.2.1-nightly.20261124.17.1.gABCDEF012345",
            "0.2.1-nightly.20261124.17.1.gabcdef012345\n",
        ] {
            assert!(!is_nightly_version(invalid), "{invalid:?}");
        }
    }

    #[test]
    fn version_cannot_inject_a_path_or_an_extra_build_directive() {
        for invalid in [
            "../0.2.1-nightly.20261124.1.1.gabcdef012345",
            "0.2.1-nightly.20261124.1.1.gabcdef012345/other",
            "0.2.1-nightly.20261124.1.1.gabcdef012345\ncargo:rustc-cfg=other",
        ] {
            assert!(!is_nightly_version(invalid));
        }
    }

    #[test]
    fn the_feed_identity_yields_its_date_and_commit() {
        assert_eq!(nightly_date_digits("0.2.1-nightly.20261124.17.1.gabcdef012345"), Some("20261124"));
        assert_eq!(nightly_commit("0.2.1-nightly.20261124.17.1.gabcdef012345"), Some("abcdef012345"));
        assert_eq!(nightly_date_digits("0.2.1"), None);
        assert_eq!(nightly_commit("0.2.1"), None);
    }
}
