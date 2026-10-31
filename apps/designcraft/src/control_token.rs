//! The token a control-channel client presents before it can run anything.
//!
//! `--control <port>` reaches every engine command: opening, saving and exporting with arbitrary
//! paths, and quitting without saving. The channel is opt-in and loopback only, but loopback is
//! not a boundary between users -- on a shared machine, or beside other local software, any
//! process that can connect could act with the user's file permissions (#1585).
//!
//! Each launch makes a fresh token, writes it to the profile directory readable only by its owner,
//! and prints where it is. A connection's first line must be that token.

use std::io::Write;
use std::path::{Path, PathBuf};

/// A client may pass the token here instead of reading the file.
pub const TOKEN_ENV: &str = "DESIGNCRAFT_CONTROL_TOKEN";
/// The file's name inside the profile directory.
pub const TOKEN_FILE: &str = "control-token";
/// Token length in hex characters (128 bits).
const TOKEN_HEX: usize = 32;

/// A fresh token from the operating system's random source, or `None` when it cannot be read --
/// in which case the channel must not start, rather than start without a token.
pub fn new_token() -> Option<String> {
    let mut bytes = [0u8; TOKEN_HEX / 2];
    getrandom::fill(&mut bytes).ok()?;
    Some(bytes.iter().map(|b| format!("{b:02x}")).collect())
}

/// `<profile>/control-token`.
pub fn token_path(config_dir: &Path) -> PathBuf {
    config_dir.join(TOKEN_FILE)
}

/// Write `token` where only its owner can read it, replacing any previous one.
pub fn write_token(config_dir: &Path, token: &str) -> std::io::Result<PathBuf> {
    std::fs::create_dir_all(config_dir)?;
    let path = token_path(config_dir);
    // Create with the mode set, rather than writing and then narrowing: a token must never exist
    // readable, however briefly.
    let mut options = std::fs::OpenOptions::new();
    options.write(true).create(true).truncate(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let mut file = options.open(&path)?;
    // `mode` applies only where `open` creates the file, so a token file left behind by an
    // earlier run keeps whatever mode it had. Narrowed on the handle while it is still empty,
    // so the token is never in a file others can read (noted in review).
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        file.set_permissions(std::fs::Permissions::from_mode(0o600))?;
    }
    file.write_all(token.as_bytes())?;
    file.flush()?;
    Ok(path)
}

/// Whether `given` is `expected`, compared without giving away where they differ.
pub fn matches(expected: &str, given: &str) -> bool {
    let (a, b) = (expected.as_bytes(), given.as_bytes());
    // The length is not a secret; the bytes are, so every one of them is looked at.
    if a.len() != b.len() {
        return false;
    }
    a.iter().zip(b).fold(0u8, |acc, (x, y)| acc | (x ^ y)) == 0
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_token_is_128_random_bits_in_hex() {
        let a = new_token().expect("the OS random source");
        let b = new_token().expect("the OS random source");
        assert_eq!(a.len(), TOKEN_HEX);
        assert!(a.chars().all(|c| c.is_ascii_hexdigit()));
        assert_ne!(a, b, "a fresh token each time");
    }

    #[test]
    fn a_token_matches_only_itself() {
        let t = "0123456789abcdef0123456789abcdef";
        assert!(matches(t, t));
        assert!(!matches(t, &t[..31]), "a prefix is not the token");
        assert!(!matches(t, &format!("{t}0")), "nor is a longer string");
        assert!(!matches(t, "0123456789abcdef0123456789abcdee"), "nor one byte off");
        assert!(!matches(t, ""), "nor nothing");
    }

    #[test]
    fn the_token_file_is_written_for_its_owner_only() {
        let dir = std::env::temp_dir().join(format!("dc-token-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let token = new_token().unwrap();
        let path = write_token(&dir, &token).expect("the token is written");
        assert_eq!(std::fs::read_to_string(&path).unwrap(), token);
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = std::fs::metadata(&path).unwrap().permissions().mode() & 0o777;
            assert_eq!(mode, 0o600, "only its owner can read the token: {mode:o}");
        }
        // Writing again replaces it, and leaves no second file behind.
        let next = new_token().unwrap();
        write_token(&dir, &next).unwrap();
        assert_eq!(std::fs::read_to_string(&path).unwrap(), next);
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// `OpenOptionsExt::mode` applies only where `open` creates the file, so a token file an
    /// earlier run left world-readable kept that mode and the next token went into it (noted in
    /// review).
    #[cfg(unix)]
    #[test]
    fn a_token_file_left_readable_by_others_is_narrowed_before_the_token_goes_in() {
        use std::os::unix::fs::PermissionsExt;
        let dir = std::env::temp_dir().join(format!("dc-token-mode-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();

        // A file from an earlier run that anyone can read.
        let path = dir.join(TOKEN_FILE);
        std::fs::write(&path, "old").unwrap();
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o644)).unwrap();
        assert_eq!(std::fs::metadata(&path).unwrap().permissions().mode() & 0o777, 0o644, "the file starts wide open");

        let token = new_token().unwrap();
        assert_eq!(write_token(&dir, &token).unwrap(), path);
        let mode = std::fs::metadata(&path).unwrap().permissions().mode() & 0o777;
        assert_eq!(mode, 0o600, "the token is not left in a file others can read: {mode:o}");
        assert_eq!(std::fs::read_to_string(&path).unwrap(), token);
        let _ = std::fs::remove_dir_all(&dir);
    }
}
