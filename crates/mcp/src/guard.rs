//! Confinement for automation file paths: `--automation-read-root` / `--automation-write-root`,
//! matching PhotoCraft's flags so one client config works across the suite (#242).
//!
//! Semantics, matching PhotoCraft's `AuthorizedWorkspace`:
//! - roots are trusted launch-time configuration, canonicalized once at startup;
//! - without any root flag nothing changes (the server stays unrestricted);
//! - with at least one root, each authority is granted only by its own flag — an omitted
//!   root means "no file authority for that side" (fail closed);
//! - request paths are untrusted: relative paths resolve beneath the corresponding root,
//!   absolute paths must already be under it, and `..`, alternate separators, drive prefixes
//!   and symlink escapes are rejected before any I/O.

use std::path::{Path, PathBuf};

use serde_json::{Value, json};

const DENIED: &str = "automation filesystem access is not granted";

/// Which root a path must live under.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Op {
    Read,
    Write,
}

impl Op {
    pub fn as_str(self) -> &'static str {
        match self {
            Op::Read => "read",
            Op::Write => "write",
        }
    }
}

/// Launch-time file authority. Unrestricted (`active == false`) until a root flag is given.
#[derive(Clone, Debug, Default)]
pub struct PathGuard {
    active: bool,
    read: Option<PathBuf>,
    write: Option<PathBuf>,
}

impl PathGuard {
    /// No confinement: every path passes through unchanged (`designcraft-cli mcp` without roots).
    pub fn unrestricted() -> Self {
        Self::default()
    }

    /// Confinement with the given roots (each `None` = that authority is absent).
    /// Roots must exist and be directories; they are canonicalized once here so later
    /// checks cannot be fooled by a symlinked root.
    pub fn new(read_root: Option<&Path>, write_root: Option<&Path>) -> Result<Self, String> {
        Ok(Self { active: true, read: open_root(read_root, "read")?, write: open_root(write_root, "write")? })
    }

    pub fn active(&self) -> bool {
        self.active
    }

    /// Validate `raw` for `op` and resolve it to an absolute path under the matching root.
    /// Unrestricted guards return `raw` untouched.
    pub fn check(&self, op: Op, raw: &str) -> Result<String, String> {
        if !self.active {
            return Ok(raw.to_string());
        }
        let root = self.authority(op)?;
        let candidate = candidate_path(root, raw)?;
        let resolved = resolve(&candidate)?;
        if !resolved.starts_with(root) {
            return Err(format!("automation {} path is outside the {} root: `{raw}`", op.as_str(), op.as_str()));
        }
        Ok(resolved.to_string_lossy().into_owned())
    }

    /// Validate one `execute`/`batch` step: classify the command, check its path parameter
    /// (rewriting it to the resolved location) and deny the paths that bypass the roots.
    pub fn guard_command(&self, id: &str, params: &mut Value) -> Result<(), String> {
        if !self.active {
            return Ok(());
        }
        if id == "script.run" {
            return Err(
                "automation roots are set: `script.run` executes nested commands that bypass them; run the steps with `execute`/`batch` instead"
                    .into(),
            );
        }
        let Some((op, key)) = command_authority(id) else {
            // Fail closed: an unmapped `file.*` command that carries a path parameter is
            // refused rather than passed through (commands without one, like `file.new`, run).
            let carries_path = ["path", "dir"].iter().any(|k| params.get(*k).and_then(Value::as_str).is_some_and(|v| !v.is_empty()));
            if id.starts_with("file.") && carries_path {
                return Err(format!("no automation path mapping for command `{id}`; refusing to run it with roots set"));
            }
            return Ok(());
        };
        match params.get(key).and_then(Value::as_str) {
            Some(raw) if !raw.is_empty() => {
                let p = self.check(op, raw)?;
                params[key] = json!(p);
                Ok(())
            }
            // The document's current file could be anywhere; require an explicit path.
            None if id == "file.save" => {
                self.authority(op)?;
                Err("`file.save` needs an explicit `path` when the automation write root is set".into())
            }
            _ => Ok(()),
        }
    }

    fn authority(&self, op: Op) -> Result<&PathBuf, String> {
        let root = match op {
            Op::Read => self.read.as_ref(),
            Op::Write => self.write.as_ref(),
        };
        root.ok_or_else(|| format!("{DENIED}: {} authority is absent", op.as_str()))
    }
}

fn open_root(path: Option<&Path>, authority: &str) -> Result<Option<PathBuf>, String> {
    let Some(path) = path else { return Ok(None) };
    if path.as_os_str().is_empty() {
        return Err(format!("automation {authority} root is empty"));
    }
    let meta = std::fs::metadata(path).map_err(|e| format!("cannot open automation {authority} root `{}`: {e}", path.display()))?;
    if !meta.is_dir() {
        return Err(format!("automation {authority} root `{}` is not a directory", path.display()));
    }
    path.canonicalize().map(Some).map_err(|e| format!("cannot open automation {authority} root `{}`: {e}", path.display()))
}

/// Absolute candidate: `raw` itself when absolute, else `root` + relative `raw`.
fn candidate_path(root: &Path, raw: &str) -> Result<PathBuf, String> {
    if raw.is_empty() {
        return Err(path_error(raw, "path is empty"));
    }
    if raw.contains('\\') {
        return Err(path_error(raw, "alternate separators are not allowed; use `/`"));
    }
    if raw.contains(':') {
        return Err(path_error(raw, "drive, device and stream prefixes are not allowed"));
    }
    if raw.starts_with('/') {
        return Ok(PathBuf::from(raw));
    }
    for component in raw.split('/') {
        if component.is_empty() {
            return Err(path_error(raw, "empty path components are not allowed"));
        }
        if component == "." {
            return Err(path_error(raw, "`.` path components are not allowed"));
        }
        if component == ".." {
            return Err(path_error(raw, "parent traversal is not allowed"));
        }
        if component.ends_with(['.', ' ']) {
            return Err(path_error(raw, "path components ending in a dot or space are not allowed"));
        }
        if is_windows_device_name(component) {
            return Err(path_error(raw, "reserved device names are not allowed"));
        }
    }
    Ok(root.join(raw))
}

fn is_windows_device_name(component: &str) -> bool {
    let stem = component.split('.').next().unwrap_or(component).to_ascii_uppercase();
    matches!(stem.as_str(), "CON" | "PRN" | "AUX" | "NUL" | "CLOCK$" | "CONIN$" | "CONOUT$")
        || stem.strip_prefix("COM").is_some_and(|n| matches!(n, "1" | "2" | "3" | "4" | "5" | "6" | "7" | "8" | "9"))
        || stem.strip_prefix("LPT").is_some_and(|n| matches!(n, "1" | "2" | "3" | "4" | "5" | "6" | "7" | "8" | "9"))
}

fn path_error(path: &str, reason: &str) -> String {
    format!("automation path rejected: {reason}: `{path}`")
}

/// Resolve symlinks on the deepest existing ancestor and re-append the rest, bounded, so a
/// link inside the root cannot point I/O outside it and a not-yet-existing file still resolves.
fn resolve(path: &Path) -> Result<PathBuf, String> {
    let mut cur = path.to_path_buf();
    let mut suffix: Vec<std::ffi::OsString> = Vec::new();
    for _ in 0..64 {
        match cur.canonicalize() {
            Ok(base) => {
                let mut out = base;
                for part in suffix.iter().rev() {
                    out.push(part);
                }
                return Ok(out);
            }
            Err(_) => {
                let Some(name) = cur.file_name() else {
                    return Err(path_error(&path.to_string_lossy(), "cannot resolve path"));
                };
                suffix.push(name.to_os_string());
                match cur.parent() {
                    Some(p) => cur = p.to_path_buf(),
                    None => return Err(path_error(&path.to_string_lossy(), "cannot resolve path")),
                }
            }
        }
    }
    Err(path_error(&path.to_string_lossy(), "path nesting is too deep"))
}

/// Which authority a path-taking command's parameter needs, and which parameter carries it.
/// `file.*` commands must all be covered: [`command_authority_is_complete`] enforces that, and
/// unknown `file.*` commands are refused while roots are set.
pub fn command_authority(id: &str) -> Option<(Op, &'static str)> {
    Some(match id {
        // Reads.
        "file.open" | "file.openIdml" | "file.place" | "place.load" | "place.styles" | "snippet.place" | "book.open" | "book.add"
        | "library.open" | "color.loadProfile" | "media.options" | "xml.loadDtd" | "file.importXml" | "swatch.load" | "table.placeGraphic"
        | "links.relink" | "data.source.select" | "data.fields" | "data.merge" => (Op::Read, "path"),
        "file.recovery.list" => (Op::Read, "dir"),
        // Writes.
        "file.save"
        | "file.saveAs"
        | "file.saveACopy"
        | "file.exportIdml"
        | "file.exportPdf"
        | "file.exportEpub"
        | "file.exportFixedEpub"
        | "file.printBooklet"
        | "file.exportHtml"
        | "file.exportText"
        | "file.exportXml"
        | "snippet.export"
        | "book.new"
        | "book.exportPdf"
        | "library.new"
        | "swatch.save" => (Op::Write, "path"),
        _ => return None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tmp(name: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("dc-guard-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        d.canonicalize().unwrap()
    }

    #[test]
    fn unrestricted_passes_everything_through() {
        let g = PathGuard::unrestricted();
        assert!(!g.active());
        assert_eq!(g.check(Op::Read, "/etc/passwd").unwrap(), "/etc/passwd");
        assert_eq!(g.check(Op::Write, "../escape").unwrap(), "../escape");
        let mut p = json!({"path": "/anywhere"});
        assert!(g.guard_command("file.saveAs", &mut p).is_ok());
        assert_eq!(p["path"], "/anywhere");
    }

    #[test]
    fn relative_paths_resolve_under_the_matching_root() {
        let (r, w) = (tmp("read"), tmp("write"));
        std::fs::write(r.join("in.txt"), b"x").unwrap();
        let g = PathGuard::new(Some(&r), Some(&w)).unwrap();
        assert_eq!(g.check(Op::Read, "in.txt").unwrap().as_str(), r.join("in.txt").to_string_lossy());
        // The same relative path resolves under each side's own root.
        assert_eq!(g.check(Op::Write, "out.txt").unwrap().as_str(), w.join("out.txt").to_string_lossy());
    }

    #[test]
    fn omitted_root_means_no_authority() {
        let r = tmp("only-read");
        let g = PathGuard::new(Some(&r), None).unwrap();
        assert!(g.check(Op::Read, "anything").is_ok());
        let e = g.check(Op::Write, "anything").unwrap_err();
        assert!(e.contains("write authority is absent"), "{e}");
        // A read root alone also refuses writes with a path (`swatch.save`-style).
        let mut p = json!({"path": "x.png"});
        assert!(g.guard_command("file.saveAs", &mut p).unwrap_err().contains("write authority is absent"));
    }

    #[test]
    fn rejected_shapes() {
        let r = tmp("shapes");
        let g = PathGuard::new(Some(&r), None).unwrap();
        for bad in ["", "../x", "a/../b", "a//b", "./x", "a\\b", "C:/x", "x/", "sub/.", "sub/..", "dir ", "file.", "CON", "a/COM1.txt"] {
            assert!(g.check(Op::Read, bad).is_err(), "`{bad}` should be rejected");
        }
    }

    #[test]
    fn absolute_paths_must_be_under_the_root() {
        let r = tmp("abs");
        std::fs::write(r.join("ok.txt"), b"x").unwrap();
        let g = PathGuard::new(Some(&r), Some(&r)).unwrap();
        assert_eq!(g.check(Op::Read, r.join("ok.txt").to_str().unwrap()).unwrap().as_str(), r.join("ok.txt").to_string_lossy());
        let e = g.check(Op::Read, "/etc/passwd").unwrap_err();
        assert!(e.contains("outside the read root"), "{e}");
        let e = g.check(Op::Write, "/tmp/elsewhere").unwrap_err();
        assert!(e.contains("outside the write root"), "{e}");
    }

    #[cfg(unix)]
    #[test]
    fn symlink_escape_is_refused() {
        let r = tmp("link-root");
        let outside = tmp("link-outside");
        std::fs::write(outside.join("secret.txt"), b"x").unwrap();
        std::os::unix::fs::symlink(&outside, r.join("leak")).unwrap();
        let g = PathGuard::new(Some(&r), Some(&r)).unwrap();
        let e = g.check(Op::Read, "leak/secret.txt").unwrap_err();
        assert!(e.contains("outside the read root"), "{e}");
    }

    #[test]
    fn guard_command_rewrites_paths_and_denies_bypasses() {
        let w = tmp("rewrite");
        let g = PathGuard::new(None, Some(&w)).unwrap();
        let mut p = json!({"path": "out.designcraft"});
        g.guard_command("file.saveAs", &mut p).unwrap();
        assert_eq!(p["path"].as_str(), Some(w.join("out.designcraft").to_string_lossy().as_ref()));
        // Nested scripts bypass per-step guarding.
        let e = g.guard_command("script.run", &mut json!({"text": "file.open {\"path\":\"x\"}"})).unwrap_err();
        assert!(e.contains("bypass"), "{e}");
        // Unknown file.* commands carrying a path are refused rather than passed through.
        let e = g.guard_command("file.somethingNew", &mut json!({"path": "x"})).unwrap_err();
        assert!(e.contains("no automation path mapping"), "{e}");
        // Path-less file commands (file.new, file.close…) are unaffected.
        assert!(g.guard_command("file.new", &mut json!({})).is_ok());
        assert!(g.guard_command("file.newSample", &mut json!({})).is_ok());
        // `file.save` without a path would write to the document's ambient location.
        let e = g.guard_command("file.save", &mut json!({})).unwrap_err();
        assert!(e.contains("explicit `path`"), "{e}");
        // Non-file commands without a path parameter are untouched.
        assert!(g.guard_command("frame.create", &mut json!({"rect": [0, 0, 10, 10]})).is_ok());
    }

    #[test]
    fn every_path_taking_file_command_is_mapped() {
        use designcraft_engine::command_specs;
        // Every `file.*` command whose documented parameters mention a path must have an
        // authority, so a new export/open command cannot silently bypass the roots.
        let unmapped: Vec<&str> = command_specs()
            .iter()
            .filter(|c| c.id.starts_with("file.") && c.params.contains("path") && c.id != "script.run")
            .map(|c| c.id)
            .filter(|id| command_authority(id).is_none())
            .collect();
        assert!(unmapped.is_empty(), "file.* path commands without an automation authority (add them to command_authority): {unmapped:?}");
        // Every mapped command must exist (no stale entries).
        for id in ["file.open", "file.save", "file.exportPdf", "data.merge", "links.relink"] {
            assert!(command_specs().iter().any(|c| c.id == id), "{id} vanished");
            assert!(command_authority(id).is_some(), "{id} unmapped");
        }
    }
}
