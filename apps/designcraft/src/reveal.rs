//! Links panel › Show in Folder: open a linked file's folder in the system file manager, with the
//! file selected where the platform can (Finder, Explorer, and Linux file managers that implement
//! the freedesktop `org.freedesktop.FileManager1` interface: Files, Dolphin, Nemo, Caja, Thunar …).

use std::path::Path;
use std::process::{Command, Stdio};

/// Show `path` in the file manager. A missing file still opens its folder, so a moved or renamed
/// link can be looked for; a missing folder is an error.
pub fn reveal(path: &str) -> Result<(), String> {
    let file = Path::new(path);
    let folder = file.parent().filter(|d| !d.as_os_str().is_empty()).ok_or_else(|| format!("“{path}” has no folder"))?;
    if !folder.is_dir() {
        return Err(format!("The folder “{}” doesn't exist", folder.display()));
    }
    show(file, folder, file.is_file())
}

/// Start `cmd` without a console and reap it on a helper thread, so no zombie stays behind.
#[cfg(any(target_os = "macos", windows))]
fn spawn(cmd: &mut Command) -> Result<(), String> {
    let mut child = cmd.stdin(Stdio::null()).stdout(Stdio::null()).stderr(Stdio::null()).spawn().map_err(|e| e.to_string())?;
    std::thread::spawn(move || child.wait());
    Ok(())
}

#[cfg(target_os = "macos")]
fn show(file: &Path, folder: &Path, exists: bool) -> Result<(), String> {
    // A missing file's folder opens in Finder by name: a plain `open` would launch a folder
    // that is an app bundle instead of showing it.
    if exists { spawn(Command::new("open").arg("-R").arg(file)) } else { spawn(Command::new("open").arg("-a").arg("Finder").arg(folder)) }
}

#[cfg(windows)]
fn show(file: &Path, folder: &Path, exists: bool) -> Result<(), String> {
    use std::os::windows::process::CommandExt;
    if exists {
        // Explorer reads `/select,` and the quoted path as one argument; Rust's quoting would split it.
        spawn(Command::new("explorer").raw_arg(format!("/select,\"{}\"", file.display())))
    } else {
        spawn(Command::new("explorer").arg(folder))
    }
}

#[cfg(all(unix, not(target_os = "macos")))]
fn show(file: &Path, folder: &Path, exists: bool) -> Result<(), String> {
    use std::os::unix::ffi::OsStrExt;
    // The D-Bus call waits up to five seconds for the file manager's answer, so it runs off the
    // UI thread and can't report back. What can be known up front is reported here: with none
    // of the programs it would use installed, Show in Folder can't work at all.
    let dbus = exists && (on_path("gdbus") || on_path("dbus-send"));
    if !dbus && !on_path("xdg-open") {
        return Err("Can't open the folder: xdg-open isn't installed".into());
    }
    let uri = file_uri(file.as_os_str().as_bytes());
    let folder = folder.to_path_buf();
    std::thread::Builder::new()
        .name("reveal".into())
        .spawn(move || {
            if dbus && show_items(&uri) {
                return;
            }
            match Command::new("xdg-open").arg(&folder).stdin(Stdio::null()).stdout(Stdio::null()).stderr(Stdio::null()).status() {
                Ok(s) if s.success() => {}
                Ok(s) => log::warn!("Show in Folder: xdg-open {}: {s}", folder.display()),
                Err(e) => log::warn!("Show in Folder: xdg-open {}: {e}", folder.display()),
            }
        })
        .map(|_| ())
        .map_err(|e| e.to_string())
}

/// Whether `program` is a file in one of the `PATH` folders, where [`Command`] looks for it.
#[cfg(all(unix, not(target_os = "macos")))]
fn on_path(program: &str) -> bool {
    std::env::var_os("PATH").is_some_and(|dirs| std::env::split_paths(&dirs).any(|d| d.join(program).is_file()))
}

/// Ask the session's file manager to open the folder with `uri` selected; false if none answered.
#[cfg(all(unix, not(target_os = "macos")))]
fn show_items(uri: &str) -> bool {
    let quiet = |cmd: &mut Command| cmd.stdin(Stdio::null()).stdout(Stdio::null()).stderr(Stdio::null()).status().is_ok_and(|s| s.success());
    quiet(Command::new("gdbus").args([
        "call",
        "--session",
        "--timeout",
        "5",
        "--dest",
        "org.freedesktop.FileManager1",
        "--object-path",
        "/org/freedesktop/FileManager1",
        "--method",
        "org.freedesktop.FileManager1.ShowItems",
        &format!("['{uri}']"),
        "",
    ])) || quiet(Command::new("dbus-send").args([
        "--session",
        "--print-reply",
        "--reply-timeout=5000",
        "--dest=org.freedesktop.FileManager1",
        "/org/freedesktop/FileManager1",
        "org.freedesktop.FileManager1.ShowItems",
        &format!("array:string:{uri}"),
        "string:",
    ]))
}

/// A `file://` URI for an absolute path: every byte outside the unreserved set and `/` is
/// percent-encoded, so the URI holds no quote or comma that would end the D-Bus argument early.
#[cfg_attr(not(all(unix, not(target_os = "macos"))), allow(dead_code))]
fn file_uri(path: &[u8]) -> String {
    let mut uri = String::from("file://");
    for &b in path {
        if b.is_ascii_alphanumeric() || matches!(b, b'/' | b'-' | b'.' | b'_' | b'~') {
            uri.push(char::from(b));
        } else {
            uri.push_str(&format!("%{b:02X}"));
        }
    }
    uri
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn file_uri_escapes_spaces_quotes_commas_and_umlauts() {
        assert_eq!(
            file_uri("/home/me/Bilder/Nachshooting Vollauflösung (2).jpg".as_bytes()),
            "file:///home/me/Bilder/Nachshooting%20Vollaufl%C3%B6sung%20%282%29.jpg"
        );
        assert_eq!(file_uri(b"/a/it's,x.pdf"), "file:///a/it%27s%2Cx.pdf");
    }

    #[cfg(all(unix, not(target_os = "macos")))]
    #[test]
    fn programs_are_looked_up_on_the_path() {
        assert!(on_path("sh"));
        assert!(!on_path("designcraft-no-such-program"));
    }

    #[test]
    fn a_missing_folder_is_an_error_not_a_launch() {
        let err = reveal("/no/such/folder/for/designcraft/photo.jpg").unwrap_err();
        assert!(err.contains("/no/such/folder/for/designcraft"), "{err}");
        assert!(reveal("photo.jpg").is_err(), "a bare name has no folder");
    }
}
