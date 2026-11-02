//! Links › Reveal in Finder (Reveal in Explorer on Windows, Reveal in File Manager on Linux):
//! shows a placed graphic's linked file in the system's file manager, selected in its folder. A
//! missing file's folder is opened instead, when it is there.
//!
//! The engine says what to show ([`designcraft_engine::cmd::link_reveal_target`]); showing it is
//! the host's job, through [`crate::Services::reveal`] (the desktop app installs
//! [`system_reveal`]; the browser build has none, so the command is disabled there and tests
//! never start a process). [`reveal_commands`] builds the processes for each platform without
//! starting them.

use std::ffi::OsString;
use std::fmt::Write as _;
use std::path::Path;

use designcraft_engine::cmd::LinkReveal;
use serde_json::{Value, json};

use crate::DesignApp;

/// The command.
pub const REVEAL: &str = "links.reveal";

/// Its label on this platform (the reference app's on macOS and Windows).
#[cfg(target_os = "macos")]
pub const LABEL: &str = "Reveal in Finder";
#[cfg(target_os = "windows")]
pub const LABEL: &str = "Reveal in Explorer";
#[cfg(not(any(target_os = "macos", target_os = "windows")))]
pub const LABEL: &str = "Reveal in File Manager";

/// Every platform's label, for the translation tables.
pub const LABELS: [&str; 3] = ["Reveal in Finder", "Reveal in Explorer", "Reveal in File Manager"];

/// Shows `path` in the file manager: selected in its folder when `select`, else the folder opened.
pub type RevealFn = Box<dyn FnMut(&Path, bool) -> Result<(), String>>;

/// Why the command can't run in this host.
const UNAVAILABLE: &str = "showing files in the file manager isn't available here";

/// The platforms [`reveal_commands`] knows.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Os {
    Mac,
    Windows,
    /// Linux and the other freedesktop systems.
    Unix,
}

impl Os {
    /// The platform this is built for.
    pub fn current() -> Os {
        if cfg!(target_os = "macos") {
            Os::Mac
        } else if cfg!(target_os = "windows") {
            Os::Windows
        } else {
            Os::Unix
        }
    }
}

/// A process to start: the program and its arguments, passed as they are (no shell).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Process {
    pub program: OsString,
    pub args: Vec<OsString>,
}

fn process(program: &str, args: impl IntoIterator<Item = OsString>) -> Process {
    Process { program: program.into(), args: args.into_iter().collect() }
}

/// `path` as a `file://` URI, every byte but unreserved ones and `/` percent-encoded (so a comma,
/// which `dbus-send` reads as an array separator, or a quote can't change the call).
pub fn file_uri(path: &Path) -> String {
    let bytes = path.as_os_str().as_encoded_bytes();
    let mut out = String::from("file://");
    for &b in bytes {
        if b.is_ascii_alphanumeric() || matches!(b, b'-' | b'.' | b'_' | b'~' | b'/') {
            out.push(char::from(b));
        } else {
            let _ = write!(out, "%{b:02X}");
        }
    }
    out
}

/// The processes that show `path` on `os`, to try in order until one succeeds: selected in its
/// folder when `select`, else the folder opened. `path` must be absolute, so it can't be read as
/// one of the program's options.
pub fn reveal_commands(os: Os, path: &Path, select: bool) -> Result<Vec<Process>, String> {
    let absolute = match os {
        // `C:\…` or `\\server\…`; a Unix-style path isn't one.
        Os::Windows => {
            let s = path.to_string_lossy();
            matches!(s.as_bytes(), [drive, b':', b'\\' | b'/', ..] if drive.is_ascii_alphabetic()) || s.starts_with("\\\\")
        }
        _ => path.to_string_lossy().starts_with('/'),
    };
    if !absolute {
        return Err(format!("{}: not an absolute path", path.display()));
    }
    let p = || path.as_os_str().to_os_string();
    Ok(match os {
        Os::Mac => {
            if select {
                vec![process("/usr/bin/open", ["-R".into(), p()])]
            } else {
                vec![process("/usr/bin/open", [p()])]
            }
        }
        // `/select,` and the path as two arguments: the path is quoted on its own when it has
        // spaces, which Explorer reads (it doesn't read `"/select,C:\a b"`).
        Os::Windows => {
            if select {
                vec![process("explorer.exe", ["/select,".into(), p()])]
            } else {
                vec![process("explorer.exe", [p()])]
            }
        }
        Os::Unix => {
            let mut v = Vec::new();
            if select {
                let uri = file_uri(path);
                // The freedesktop file manager interface, through whichever D-Bus tool there is.
                v.push(process(
                    "dbus-send",
                    [
                        "--session".into(),
                        "--print-reply".into(),
                        "--dest=org.freedesktop.FileManager1".into(),
                        "--type=method_call".into(),
                        "/org/freedesktop/FileManager1".into(),
                        "org.freedesktop.FileManager1.ShowItems".into(),
                        format!("array:string:{uri}").into(),
                        "string:".into(),
                    ],
                ));
                v.push(process(
                    "gdbus",
                    [
                        "call".into(),
                        "--session".into(),
                        "--dest".into(),
                        "org.freedesktop.FileManager1".into(),
                        "--object-path".into(),
                        "/org/freedesktop/FileManager1".into(),
                        "--method".into(),
                        "org.freedesktop.FileManager1.ShowItems".into(),
                        // A GVariant array of one string: the URI has no quote left to end it.
                        format!("['{uri}']").into(),
                        "".into(),
                    ],
                ));
            }
            // No file manager interface: the folder, opened.
            let folder = if select { path.parent().unwrap_or(path) } else { path };
            v.push(process("xdg-open", [folder.as_os_str().to_os_string()]));
            v
        }
    })
}

/// Shows `path` in the file manager: the desktop host's [`crate::Services::reveal`]. The
/// processes are started and waited for on their own thread, so the frame never waits.
#[cfg(not(target_arch = "wasm32"))]
pub fn system_reveal(path: &Path, select: bool) -> Result<(), String> {
    use std::process::{Command, Stdio};
    let commands = reveal_commands(Os::current(), path, select)?;
    let shown = path.display().to_string();
    std::thread::Builder::new()
        .name("reveal".into())
        .spawn(move || {
            for c in commands {
                let status = Command::new(&c.program).args(&c.args).stdin(Stdio::null()).stdout(Stdio::null()).stderr(Stdio::null()).status();
                match status {
                    Ok(s) if s.success() => return,
                    // Explorer's exit status says nothing about success.
                    Ok(_) if c.program == "explorer.exe" => return,
                    _ => {}
                }
            }
            log::warn!("couldn't show {shown} in the file manager");
        })
        .map(|_| ())
        .map_err(|e| format!("couldn't show {}: {e}", path.display()))
}

/// The params the engine reads: the Links panel's `asset`, or `ids`.
fn target(p: &Value) -> Value {
    let mut out = serde_json::Map::new();
    for k in ["asset", "ids"] {
        if let Some(v) = p.get(k) {
            out.insert(k.into(), v.clone());
        }
    }
    Value::Object(out)
}

/// Whether Reveal can run for `p` (`{asset}`, `{ids}`, or the selection): the host can show files
/// and a named graphic's file (or a missing file's folder) is on disk.
pub fn available_for(app: &DesignApp, p: &Value) -> bool {
    app.services.reveal.is_some() && designcraft_engine::cmd::link_reveal_target(&app.session, &target(p)).is_ok()
}

/// [`available_for`] the selection (the menus).
pub fn available(app: &DesignApp) -> bool {
    available_for(app, &Value::Null)
}

/// Run `links.reveal`; `None` if `id` isn't it.
pub(crate) fn run(app: &mut DesignApp, id: &str, p: &Value) -> Option<Result<Value, String>> {
    (id == REVEAL).then(|| reveal(app, p))
}

fn reveal(app: &mut DesignApp, p: &Value) -> Result<Value, String> {
    let what = designcraft_engine::cmd::link_reveal_target(&app.session, &target(p)).map_err(|e| e.to_string())?;
    let show = app.services.reveal.as_mut().ok_or(UNAVAILABLE)?;
    let (path, select) = match &what {
        LinkReveal::File(f) => (f, true),
        LinkReveal::Folder(d) => (d, false),
    };
    show(path, select)?;
    Ok(json!({ "path": path.to_string_lossy(), "selected": select }))
}

#[cfg(test)]
mod tests;
