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
        if let Some(reason) = stored_path_denial(id, params) {
            return Err(reason);
        }
        let Some((op, key)) = command_authority(id) else {
            // Fail closed: an unmapped `file.*`/`app.*` command that carries a path parameter is
            // refused rather than passed through (commands without one, like `file.new`, run).
            let carries_path = ["path", "dir"].iter().any(|k| params.get(*k).and_then(Value::as_str).is_some_and(|v| !v.is_empty()));
            if (id.starts_with("file.") || id.starts_with("app.")) && carries_path {
                return Err(format!("no automation path mapping for command `{id}`; refusing to run it with roots set"));
            }
            return Ok(());
        };
        match param_str(params, key) {
            Some(raw) if !raw.is_empty() => {
                let p = self.check(op, raw)?;
                set_param_str(params, key, &p);
                Ok(())
            }
            // The default fallback is an unverifiable location: the document's own file or a
            // save dialog whose result the guard never sees. Require an explicit path.
            None if requires_explicit_path(id) => {
                self.authority(op)?;
                Err(format!("`{id}` needs an explicit `{key}` when the automation roots are set"))
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

/// A (possibly nested, dotted) parameter's string value.
fn param_str<'a>(params: &'a Value, key: &str) -> Option<&'a str> {
    match key.split_once('.') {
        Some((head, tail)) => params.get(head)?.get(tail)?.as_str(),
        None => params.get(key)?.as_str(),
    }
}

/// Rewrite a (possibly nested, dotted) string parameter in place.
fn set_param_str(params: &mut Value, key: &str, value: &str) {
    match key.split_once('.') {
        Some((head, tail)) => {
            if let Some(obj) = params.get_mut(head).and_then(Value::as_object_mut) {
                obj.insert(tail.to_string(), json!(value));
            }
        }
        None => {
            if let Some(obj) = params.as_object_mut() {
                obj.insert(key.to_string(), json!(value));
            }
        }
    }
}

/// Commands whose default (missing path parameter) location cannot be verified: the
/// document's own file, or a save dialog whose picked path the guard never sees.
fn requires_explicit_path(id: &str) -> bool {
    matches!(
        id,
        "file.save"
            | "app.exportPng"
            | "app.exportIdml"
            | "app.exportEpub"
            | "app.exportInteractivePdf"
            | "app.exportFixedEpub"
            | "app.exportHtml"
            | "app.exportXml"
            | "app.exportText"
            | "app.exportPdf"
            | "app.saveSwatches"
    )
}

/// The `data.merge` payloads that keep the data source inside the request (mirrors the
/// engine's `payload_present`).
fn data_payload_present(params: &Value) -> bool {
    ["rows", "csv", "bytes", "base64", "path"].iter().any(|k| params.get(*k).is_some())
}

/// Commands refused while roots are set: their file access follows paths stored in the
/// document, the book or the session rather than paths carried in the request, so the guard
/// cannot verify them (fail closed — each message names the checked alternative).
fn stored_path_denial(id: &str, params: &Value) -> Option<String> {
    let msg = match id {
        "script.run" => "`script.run` executes nested commands that bypass the per-step check; run the steps with `execute`/`batch` instead",
        "app.save" => "`app.save` writes to the document's ambient location; use `save_document` with an explicit `path` instead",
        // Picker dialogs: the picked path never passes the guard, so with roots set the
        // parameterized command must be used instead.
        "app.openDialog" | "app.placeDialog" | "app.saveDialog" | "app.saveCopyDialog" | "app.packageDialog" | "app.loadSwatches" => {
            "file picker dialogs are refused while roots are set (the picked path cannot be checked); pass an explicit `path` to the command itself"
        }
        "data.source.update" => {
            "the data source path is stored in the document and cannot be verified; run `data.source.select` with an explicit `path` to re-read it"
        }
        "data.merge" if !data_payload_present(params) => {
            "`data.merge` with no inline data re-reads the source stored in the document, which cannot be verified; pass `csv`/`rows`/`bytes` or an explicit `path`"
        }
        "links.list" | "links.update" => {
            "link paths are stored in the document and cannot be verified; `links.relink` with an explicit `path` points a graphic at a file under a root"
        }
        "links.relinkFolder" if param_str(params, "dir").is_none_or(str::is_empty) => {
            "`links.relinkFolder` without `dir` searches the folders stored in the document; give `dir` (checked against the read root)"
        }
        "book.list" | "book.paginate" | "book.syncStyles" | "book.exportPdf" => {
            "the book stores document paths that cannot be verified; open the documents individually with `file.open` instead"
        }
        "book.add" | "book.remove" | "book.styleSource" => {
            "saving the book writes to its stored path, which the roots cannot verify; books cannot be edited while roots are set"
        }
        "library.add" | "library.remove" => "saving the library writes to its stored path, which the roots cannot verify",
        "file.package" => "packaging copies the files stored in the document, whose paths cannot be verified",
        "file.revert" => {
            "`file.revert` re-reads the document's stored path, which the roots cannot verify; use `file.open` with an explicit `path` instead"
        }
        _ => return None,
    };
    Some(format!("automation roots are set: {msg}"))
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
/// A symlink among the not-yet-existing components (a dangling link, whose target canonicalize
/// cannot reach) is followed explicitly by hand — otherwise the write would follow it out.
fn resolve(path: &Path) -> Result<PathBuf, String> {
    let mut cur = path.to_path_buf();
    for _ in 0..64 {
        // Canonicalize the deepest existing ancestor.
        let mut suffix: Vec<std::ffi::OsString> = Vec::new();
        let mut probe = cur.clone();
        let base = loop {
            match probe.canonicalize() {
                Ok(base) => break base,
                Err(_) => {
                    let Some(name) = probe.file_name() else {
                        return Err(path_error(&path.to_string_lossy(), "cannot resolve path"));
                    };
                    suffix.push(name.to_os_string());
                    match probe.parent() {
                        Some(p) => probe = p.to_path_buf(),
                        None => return Err(path_error(&path.to_string_lossy(), "cannot resolve path")),
                    }
                }
            }
        };
        // Re-append the not-yet-existing components, following any dangling symlink among them.
        let mut out = base;
        let mut link = None;
        for (i, part) in suffix.iter().rev().enumerate() {
            out.push(part);
            if let Ok(meta) = std::fs::symlink_metadata(&out)
                && meta.file_type().is_symlink()
            {
                link = Some((i, out.clone()));
                break;
            }
        }
        let Some((i, link)) = link else { return Ok(out) };
        // Restart from the link's target, keeping the components that followed the link.
        let target = match std::fs::read_link(&link) {
            Ok(t) => t,
            Err(e) => return Err(path_error(&path.to_string_lossy(), &format!("cannot read the symlink: {e}"))),
        };
        let mut next = match target.is_absolute() {
            true => target,
            false => match link.parent() {
                Some(p) => p.join(target),
                None => return Err(path_error(&path.to_string_lossy(), "cannot resolve path")),
            },
        };
        for part in suffix.iter().rev().skip(i + 1) {
            next.push(part);
        }
        cur = next;
    }
    Err(path_error(&path.to_string_lossy(), "path nesting is too deep"))
}

/// Which authority a path-taking command's parameter needs, and which parameter carries it
/// (a dotted key like `poster.path` reaches into a nested object). Every command here must
/// either be covered by this table or be refused by [`stored_path_denial`] —
/// [`every_path_taking_command_is_classified`] enforces that, and unknown `file.*`/`app.*`
/// commands carrying a path are refused while roots are set.
pub fn command_authority(id: &str) -> Option<(Op, &'static str)> {
    Some(match id {
        // Reads.
        "file.open" | "file.openIdml" | "file.place" | "place.load" | "place.styles" | "snippet.place" | "book.open" | "library.open"
        | "color.loadProfile" | "xml.loadDtd" | "file.importXml" | "swatch.load" | "table.placeGraphic" | "links.relink" | "data.source.select"
        | "data.fields" | "data.merge" => (Op::Read, "path"),
        // The media poster lives in a nested object: `poster: {path | base64+name}`.
        "media.options" => (Op::Read, "poster.path"),
        "file.recovery.list" | "file.recovery.open" | "links.relinkFolder" => (Op::Read, "dir"),
        // Connected-mode exports and swatch exchange write wherever their `path` says (or ask
        // a save dialog, whose result the guard never sees): check them like file.export*.
        "app.exportPng"
        | "app.exportIdml"
        | "app.exportEpub"
        | "app.exportInteractivePdf"
        | "app.exportFixedEpub"
        | "app.exportHtml"
        | "app.exportXml"
        | "app.exportText"
        | "app.exportPdf"
        | "app.saveSwatches" => (Op::Write, "path"),
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
        | "library.new"
        | "swatch.save" => (Op::Write, "path"),
        "links.copyTo" | "file.recovery.save" => (Op::Write, "dir"),
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

    /// Every command outside `file.*` whose documented parameters name a `path` or `dir`
    /// parameter (whole words — "direction", "directSelection" and prose like "compound
    /// paths" don't count) must be checked (authority), refused (stored-path denial) or
    /// explicitly listed here as taking a *non*-filesystem value of that name, so a new
    /// links/book/data or connected-mode export command cannot silently bypass the roots.
    #[test]
    fn every_other_path_taking_command_is_classified() {
        use designcraft_engine::command_specs;
        // Commands whose `path`/`dir` parameter is not a filesystem path: `dir` is a
        // direction, `path` is a vector path or an output/template field.
        const NOT_FILESYSTEM: &[&str] = &[
            "text.move",
            "type.onPath",
            "object.caption",
            "object.captionFromFilename",
            "object.captionFromXmp",
            "object.makeCompoundPath",
            "object.releaseCompoundPath",
            "xml.validate", // {path} in the *result*
        ];
        let names_path = |params: &str| params.split(|c: char| !c.is_alphanumeric()).any(|t| t == "path" || t == "dir");
        let is_classified = |id: &str, params: &str| {
            command_authority(id).is_some()
                || stored_path_denial(id, &json!({})).is_some()
                || NOT_FILESYSTEM.contains(&id)
                // `path.*` commands are vector geometry (anchors, subpaths), not files.
                || id.starts_with("path.")
                || !names_path(params)
        };
        let unclassified: Vec<&str> = command_specs()
            .iter()
            .filter(|c| !is_classified(c.id, c.params))
            .map(|c| c.id)
            .filter(|id| *id != "script.run") // refused outright while roots are set
            .collect();
        assert!(
            unclassified.is_empty(),
            "path/dir commands without an automation classification (map in command_authority, refuse in stored_path_denial, or list in NOT_FILESYSTEM): {unclassified:?}"
        );
    }

    /// The connected-mode `app.*` commands are UI-only (not in `command_specs`), so their
    /// classification is asserted here against the list in `ui-egui/src/menus.rs`.
    #[test]
    fn connected_app_writes_are_classified() {
        for id in [
            "app.exportPng",
            "app.exportIdml",
            "app.exportEpub",
            "app.exportInteractivePdf",
            "app.exportFixedEpub",
            "app.exportHtml",
            "app.exportXml",
            "app.exportText",
            "app.exportPdf",
            "app.saveSwatches",
        ] {
            assert_eq!(command_authority(id), Some((Op::Write, "path")), "{id} must be write-checked");
        }
        // The ambient-location and stored-path writers are refused instead.
        for id in ["app.save", "data.source.update", "links.list", "links.update", "book.list", "book.paginate", "file.package"] {
            assert!(stored_path_denial(id, &json!({})).is_some(), "{id} must be refused while roots are set");
        }
    }

    #[test]
    fn nested_poster_path_is_checked() {
        let r = tmp("poster");
        std::fs::write(r.join("p.png"), b"x").unwrap();
        let g = PathGuard::new(Some(&r), Some(&r)).unwrap();
        // `poster: {path}` is rewritten under the read root like any other path…
        let mut p = json!({"poster": {"path": "p.png"}});
        g.guard_command("media.options", &mut p).unwrap();
        assert_eq!(p["poster"]["path"].as_str(), Some(r.join("p.png").to_string_lossy().as_ref()));
        // …and an outside target is refused.
        let mut p = json!({"poster": {"path": "/etc/hostname"}});
        let e = g.guard_command("media.options", &mut p).unwrap_err();
        assert!(e.contains("outside the read root"), "{e}");
        // Inline posters and the null placeholder carry no path and pass through.
        let mut p = json!({"poster": {"base64": "AA==", "name": "p.png"}});
        g.guard_command("media.options", &mut p).unwrap();
        let mut p = json!({"poster": null});
        g.guard_command("media.options", &mut p).unwrap();
    }

    #[test]
    fn folder_parameters_are_checked() {
        let (r, w) = (tmp("folder-r"), tmp("folder-w"));
        let g = PathGuard::new(Some(&r), Some(&w)).unwrap();
        // links.copyTo writes to `dir`: confined to the write root.
        let mut p = json!({"dir": "Copies"});
        g.guard_command("links.copyTo", &mut p).unwrap();
        assert_eq!(p["dir"].as_str(), Some(w.join("Copies").to_string_lossy().as_ref()));
        let mut p = json!({"dir": "/tmp/dc-copy-escape"});
        let e = g.guard_command("links.copyTo", &mut p).unwrap_err();
        assert!(e.contains("outside the write root"), "{e}");
        // links.relinkFolder reads from `dir`: confined to the read root.
        let mut p = json!({"dir": "Assets"});
        g.guard_command("links.relinkFolder", &mut p).unwrap();
        assert_eq!(p["dir"].as_str(), Some(r.join("Assets").to_string_lossy().as_ref()));
        // Without `dir` it would search the folders stored in the document — refused.
        let e = g.guard_command("links.relinkFolder", &mut json!({})).unwrap_err();
        assert!(e.contains("stored in the document"), "{e}");
    }

    #[test]
    fn connected_app_export_paths_are_checked() {
        let w = tmp("app-export");
        let g = PathGuard::new(None, Some(&w)).unwrap();
        // An app.* export with a path is rewritten under the write root…
        let mut p = json!({"path": "page.png"});
        g.guard_command("app.exportPng", &mut p).unwrap();
        assert_eq!(p["path"].as_str(), Some(w.join("page.png").to_string_lossy().as_ref()));
        // …an absolute path outside is refused…
        let mut p = json!({"path": "/tmp/dc-app-escape.png"});
        let e = g.guard_command("app.exportPdf", &mut p).unwrap_err();
        assert!(e.contains("outside the write root"), "{e}");
        // …and without a path the save-dialog fallback (whose result the guard cannot see)
        // is refused: an explicit path is required.
        let e = g.guard_command("app.exportPng", &mut json!({})).unwrap_err();
        assert!(e.contains("explicit `path`"), "{e}");
        let e = g.guard_command("app.saveSwatches", &mut json!({})).unwrap_err();
        assert!(e.contains("explicit `path`"), "{e}");
        // app.save writes the document's ambient location: refused outright.
        let e = g.guard_command("app.save", &mut json!({})).unwrap_err();
        assert!(e.contains("ambient"), "{e}");
        // Unmapped app.* commands carrying a path are refused too (fail closed).
        let e = g.guard_command("app.somethingNew", &mut json!({"path": "x"})).unwrap_err();
        assert!(e.contains("no automation path mapping"), "{e}");
    }

    #[test]
    fn document_stored_paths_are_refused() {
        let w = tmp("stored");
        let g = PathGuard::new(Some(&w), Some(&w)).unwrap();
        // Commands that follow paths stored in the document (data source, links, book
        // entries) are refused, whatever the request says — the guard cannot see those.
        for id in [
            "data.source.update",
            "links.list",
            "links.update",
            "book.list",
            "book.paginate",
            "book.syncStyles",
            "book.exportPdf",
            "book.add",
            "book.remove",
            "book.styleSource",
            "library.add",
            "library.remove",
            "file.package",
            "file.revert",
        ] {
            let e = g.guard_command(id, &mut json!({})).unwrap_err();
            assert!(e.starts_with("automation roots are set:"), "{id}: {e}");
        }
        // data.merge with inline data needs no file and runs; without it, it would re-read
        // the stored source — refused (this is the file.exportText exfiltration chain).
        assert!(g.guard_command("data.merge", &mut json!({"csv": "a,b\n1,2"})).is_ok());
        let e = g.guard_command("data.merge", &mut json!({})).unwrap_err();
        assert!(e.contains("stored in the document"), "{e}");
        // With an explicit path it is checked like any other read.
        let mut p = json!({"path": "data.csv"});
        g.guard_command("data.merge", &mut p).unwrap();
        assert_eq!(p["path"].as_str(), Some(w.join("data.csv").to_string_lossy().as_ref()));
    }

    #[cfg(unix)]
    #[test]
    fn dangling_symlink_escape_is_refused() {
        let r = tmp("dangling-root");
        let outside = std::env::temp_dir().join(format!("dc-dangling-outside-{}", std::process::id()));
        let _ = std::fs::remove_file(&outside);
        // A symlink inside the root whose target does not exist yet: canonicalize cannot
        // follow it, so the write must be refused by following the link by hand.
        std::os::unix::fs::symlink(&outside, r.join("plant.txt")).unwrap();
        let g = PathGuard::new(Some(&r), Some(&r)).unwrap();
        let e = g.check(Op::Write, "plant.txt").unwrap_err();
        assert!(e.contains("outside the write root"), "{e}");
        assert!(!outside.exists(), "no write happened");
        // A dangling link *inside* the root is fine: the write lands under the root.
        std::os::unix::fs::symlink(r.join("inside.txt"), r.join("ok-link.txt")).unwrap();
        assert_eq!(g.check(Op::Write, "ok-link.txt").unwrap().as_str(), r.join("inside.txt").to_string_lossy());
    }
}
