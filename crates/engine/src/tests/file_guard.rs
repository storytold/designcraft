//! The guarantee behind `--automation-read-root` and `--automation-write-root`: every file a
//! command reads or writes by path goes through a checked reader, in every crate a command can
//! reach, not only this one.
//!
//! In the engine that reader is the [`crate::io`] module. The lower crates cannot depend on the
//! engine (`cargo xtask layers`), so they either have no file access at all or take a reader or
//! an access check from the caller (`designcraft_idml::import_idml_with`,
//! `designcraft_fonts::FontDb::load_document_fonts`).
//!
//! The scan reads the source of every crate under `crates/` and flags what touches the file
//! system, however it is imported or spelled: `std::fs`, a bare `fs::` after `use std::fs` or
//! `use std::{fs, ...}`, `File::open`/`File::create`, `OpenOptions`, and the `Path` probes
//! (`.metadata(`, `.exists(`, `.canonicalize(`, `.read_dir(`...), as methods or as
//! `Path::exists`. The host applications under `apps/` are out of scope: they run with the
//! operator's own authority, and the roots are applied inside the process they start.

use std::path::{Path, PathBuf};

/// Files that may touch the file system, with why. Paths are relative to `crates/`. A file not
/// listed here, and not a test file or test module, must not.
const ALLOWED: &[(&str, &str)] = &[
    ("engine/src/io.rs", "the checked gateway: every read and write a command makes"),
    ("engine/src/file_access.rs", "the root check itself (resolves links with the file system)"),
    ("engine/src/atomic.rs", "how a write reaches the disk; commands call it only through io::write_file and io::replace_with"),
    (
        "render/src/preview_disk.rs",
        "disposable bounded previews in a directory explicitly configured by the native host; filenames are content hashes, never document-supplied paths. CLI and web leave persistence disabled",
    ),
    (
        "engine/src/recovery.rs",
        "the unsaved documents live in a folder the host application chooses (a command's own `dir` is checked in cmd/file.rs before it gets here)",
    ),
    (
        "engine/src/cmd/package.rs",
        "`OutputRoot` checks the folder against the write root before it creates anything and then only writes inside it. Its reads are of the font files that draw the document's text (installed fonts and the `Document Fonts` folder), never of a path a command names",
    ),
    (
        "fonts/src/fontdb/system.rs",
        "the installed-font scan reads the platform's fixed font folders (`system_font_dirs`: no command parameter or document field names one), and `load_document_fonts` asks the caller's `allow` check about its folder and each font file before opening either",
    ),
    (
        "mcp/src/tools.rs",
        "`screenshot` reads back the PNG the connected app just wrote. Connected mode cannot be combined with the roots (the CLI refuses the pair), and the app's own engine checks the path it writes",
    ),
];

fn rust_files(dir: &Path, files: &mut Vec<PathBuf>) {
    let Ok(entries) = std::fs::read_dir(dir) else { return };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            rust_files(&path, files);
        } else if path.extension().is_some_and(|e| e == "rs") {
            files.push(path);
        }
    }
}

/// Test files (`tests.rs`, `tests_*.rs`, `*_tests.rs`, anything under a `tests/` folder) build
/// their own fixtures on disk.
fn is_test_file(rel: &str) -> bool {
    let name = rel.rsplit('/').next().unwrap_or(rel);
    name.starts_with("tests") || name.ends_with("_tests.rs") || name == "testing.rs" || rel.split('/').any(|part| part == "tests")
}

/// The file's lines but its test modules: a module is skipped from its `#[cfg(test)]` (or
/// `#[cfg(all(test, ...))]`) to the closing brace at the same indent. A `mod x;` declaration
/// has no body to skip.
fn code_outside_tests(text: &str) -> Vec<(usize, &str)> {
    let lines: Vec<&str> = text.lines().collect();
    let mut code = vec![];
    let mut i = 0;
    while i < lines.len() {
        let attr = lines[i].trim();
        let module = lines.get(i + 1).map(|l| without_visibility(l.trim_start()));
        if attr.starts_with("#[cfg(") && attr.replace("not(test)", "").contains("test") && module.is_some_and(|m| m.starts_with("mod ")) {
            let header = lines[i + 1];
            i += 2;
            if header.trim_end().ends_with('{') {
                let indent = &header[..header.len() - header.trim_start().len()];
                let close = format!("{indent}}}");
                while i < lines.len() && lines[i].trim_end() != close {
                    i += 1;
                }
                i += 1;
            }
            continue;
        }
        code.push((i + 1, lines[i]));
        i += 1;
    }
    code
}

/// `item` without a leading `pub`, `pub(crate)`, `pub(super)` or `pub(in …)`: a test module
/// that sibling modules share is still a test module.
fn without_visibility(item: &str) -> &str {
    let Some(rest) = item.strip_prefix("pub") else { return item };
    let rest = if rest.starts_with('(') { rest.find(')').map_or(rest, |close| &rest[close + 1..]) } else { rest };
    if rest.starts_with(char::is_whitespace) { rest.trim_start() } else { item }
}

/// `line` without its `//` comment (a `//` inside a string or char is rare enough in this code
/// to leave alone: it only hides the rest of that line from the scan).
fn without_comment(line: &str) -> &str {
    line.find("//").map_or(line, |at| &line[..at])
}

fn is_ident(c: char) -> bool {
    c.is_alphanumeric() || c == '_'
}

/// Does `line` contain `word` not glued to a longer identifier on its left (`std::fs::` and
/// ` fs::` match `fs::`; `ofs::` does not)?
fn has_word(line: &str, word: &str) -> bool {
    line.match_indices(word).any(|(at, _)| !line[..at].chars().next_back().is_some_and(is_ident))
}

/// What in `line` reaches the file system, or `None`.
fn file_system_use(line: &str) -> Option<String> {
    let line = without_comment(line);
    // `std::fs::read`, `fs::read` after `use std::fs;` or `use std::{fs, io};`, `std::os::unix::fs::...`.
    for word in ["fs::", "File::open", "File::create", "File::options", "OpenOptions", "atomic::write_atomic"] {
        if has_word(line, word) {
            return Some(word.into());
        }
    }
    // The probes on a path, as `p.exists()` or `Path::exists`.
    for method in ["metadata", "symlink_metadata", "canonicalize", "read_dir", "try_exists", "exists", "is_file", "is_dir", "read_link"] {
        if line.contains(&format!(".{method}(")) || line.contains(&format!("Path::{method}")) {
            return Some(method.to_string());
        }
    }
    None
}

/// `use` statements (which can span lines) that bring the file system module in under any name:
/// `use std::{fs as files, io};` leaves no `fs::` to find at the call.
fn imports_fs(text: &str) -> Vec<usize> {
    let mut hits = vec![];
    let mut statement = String::new();
    let mut start = 0;
    for (n, line) in code_outside_tests(text) {
        let line = without_comment(line);
        if statement.is_empty() {
            if !without_visibility(line.trim_start()).starts_with("use ") {
                continue;
            }
            start = n;
        }
        statement.push_str(line);
        statement.push(' ');
        if line.contains(';') {
            let words: Vec<&str> = statement.split(|c: char| !is_ident(c)).collect();
            if words.contains(&"fs") && (words.contains(&"std") || words.contains(&"tokio") || words.contains(&"async_std")) {
                hits.push(start);
            }
            statement.clear();
        }
    }
    hits
}

/// Every offending line of the crate sources under `crates_dir`, minus [`ALLOWED`] and test code.
fn offenders(crates_dir: &Path) -> (Vec<String>, usize) {
    let mut files = vec![];
    rust_files(crates_dir, &mut files);
    let mut scanned = 0;
    let mut found = vec![];
    for path in files {
        let rel = path.strip_prefix(crates_dir).unwrap_or(&path).to_string_lossy().replace('\\', "/");
        // `crates/<name>/src/...` only: build scripts, benches and `tests/` are not shipped code.
        if rel.split('/').nth(1).is_none_or(|d| d != "src") || is_test_file(&rel) || ALLOWED.iter().any(|(allowed, _)| *allowed == rel) {
            continue;
        }
        let Ok(text) = std::fs::read_to_string(&path) else { continue };
        scanned += 1;
        for (n, line) in code_outside_tests(&text) {
            if let Some(what) = file_system_use(line) {
                found.push(format!("crates/{rel}:{n}: `{what}`: {}", line.trim()));
            }
        }
        for n in imports_fs(&text) {
            found.push(format!("crates/{rel}:{n}: imports the file system module"));
        }
    }
    (found, scanned)
}

fn crates_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("..")
}

/// Every command reads and writes files through [`crate::io`], in the engine and in every crate
/// below it: none may reach the file system itself, so the automation roots cannot be walked
/// around by a path that one of them opens. The exceptions are [`ALLOWED`], each with its reason.
#[test]
fn no_crate_touches_the_file_system_outside_the_checked_readers() {
    let (found, scanned) = offenders(&crates_dir());
    assert!(scanned > 100, "the scan found only {scanned} source files: has the layout moved?");
    assert!(
        found.is_empty(),
        "route these through crate::io (engine) or a reader the caller injects (lower crates), or add the file to ALLOWED with the reason it is safe: {found:#?}"
    );
}

/// The allow list names files that exist, so a rename cannot leave a stale exception behind (or
/// quietly exempt a new file of the same name).
#[test]
fn every_allowed_file_exists_and_still_needs_its_exception() {
    for (rel, reason) in ALLOWED {
        let path = crates_dir().join(rel);
        let text = std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("{rel}: {e}"));
        assert!(!reason.is_empty());
        let touches = code_outside_tests(&text).iter().any(|(_, line)| file_system_use(line).is_some()) || !imports_fs(&text).is_empty();
        assert!(touches, "{rel} no longer touches the file system: drop it from ALLOWED");
    }
}

/// The scan must not be foolable by how a call is spelled.
#[test]
fn the_scan_sees_every_spelling() {
    let flagged = [
        "let b = std::fs::read(p)?;",
        "let b = fs::read(p)?;",
        "    fs::write(p, b)",
        "let f = File::open(p)?;",
        "let f = std::fs::File::create(p)?;",
        "let f = OpenOptions::new().write(true).open(p)?;",
        "let m = path.metadata()?;",
        "let m = path.symlink_metadata()?;",
        "let c = path.canonicalize()?;",
        "for e in path.read_dir()? {}",
        "if path.try_exists()? {}",
        "if p.exists() {}",
        "if p.is_file() {}",
        "if p.is_dir() {}",
        "let l = p.read_link()?;",
        "files.retain(Path::exists);",
        "crate::atomic::write_atomic(p, b)?;",
        "(a, fs::read(x))",
    ];
    for line in flagged {
        assert!(file_system_use(line).is_some(), "not flagged: {line}");
    }
    let fine = [
        "let b = crate::io::read_file(p)?;",
        "if crate::io::is_dir(p) {}",
        "let m = crate::io::metadata(p)?;",
        "// std::fs::read(p) is what io wraps",
        "let n = NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed);",
        "let ofs::x = 1;",
    ];
    for line in fine {
        assert!(file_system_use(line).is_none(), "wrongly flagged: {line}");
    }
    for import in ["use std::fs;", "use std::{fs, io};", "use std::{\n    io,\n    fs as files,\n};", "use std::fs::File;"] {
        assert!(!imports_fs(import).is_empty(), "import not flagged: {import}");
    }
    assert!(imports_fs("use std::io::Read;\nuse std::path::Path;").is_empty());
    assert!(imports_fs("#[cfg(test)]\nmod tests {\n    use std::fs;\n}\n").is_empty(), "test modules are skipped");
    assert!(imports_fs("#[cfg(test)]\npub(super) mod shared {\n    use std::fs;\n}\n").is_empty(), "shared test modules are skipped");
    assert!(!imports_fs("#[cfg(not(test))]\npub(crate) mod real {\n    use std::fs;\n}\n").is_empty(), "cfg(not(test)) modules are scanned");
}
