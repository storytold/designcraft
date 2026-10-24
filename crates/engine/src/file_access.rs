//! The folders automation may read and write.
//!
//! `designcraft-cli mcp --automation-read-root <dir> --automation-write-root <dir>` confines the
//! files an agent's commands reach; each flag may be given more than once (#1552). [`AutomationRoots`]
//! holds the folders of each kind. While they are in force, every path a command reads goes through
//! [`check_read`] and every path it writes through [`check_write`]:
//!
//! - The path is made absolute against the working directory, as the file system reads it. Its
//!   links are followed: for a file that doesn't exist yet, the links of its nearest existing
//!   folder, the rest of the path being plain names (`..` below a missing folder is refused, and so
//!   is a link that leads nowhere).
//! - The result must lie inside one of the roots of its kind, compared by whole components
//!   (`/work2` isn't inside `/work`), without regard to case on Windows. Two roots don't make their
//!   common parent one.
//! - Read and write access are separate: a root given alone grants only its own kind of access.
//!
//! Roots are in force on a thread inside [`confine`], and in the whole process after
//! [`confine_process`] (`designcraft-cli mcp`), except inside [`unconfined`]. Without roots
//! nothing is checked.
//!
//! A path is checked, then opened: a process that can change the folders in between (swapping in
//! a link) can still win that race. The roots keep an agent to the files it was given; they don't
//! guard against other programs on the computer.

use std::cell::RefCell;
use std::ffi::OsString;
use std::path::{Component, Path, PathBuf};
use std::sync::{Arc, OnceLock};

/// Read access or write access.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Access {
    Read,
    Write,
}

/// The folders automation may read and write; a kind with no root grants no access of its kind.
#[derive(Debug)]
pub struct AutomationRoots {
    read: Vec<Root>,
    write: Vec<Root>,
}

#[derive(Debug)]
struct Root {
    /// The folder made absolute, links followed: what paths are compared with.
    resolved: PathBuf,
    /// The same without Windows' `\\?\` prefix, for people.
    shown: PathBuf,
}

impl Root {
    /// The folder at `path`, which must exist.
    fn open(path: &Path, access: Access) -> Result<Self, String> {
        let kind = match access {
            Access::Read => "read",
            Access::Write => "write",
        };
        let resolved = std::fs::canonicalize(path)
            .and_then(|p| if p.is_dir() { Ok(p) } else { Err(std::io::ErrorKind::NotADirectory.into()) })
            .map_err(|e| format!("cannot open the automation {kind} root `{}`: {e}", path.display()))?;
        Ok(Self { shown: plain(resolved.clone()), resolved })
    }
}

impl AutomationRoots {
    /// The roots `read` and `write`, which must be existing folders (one given twice counts once);
    /// `None` when neither kind has any (nothing is confined then).
    pub fn new(read: &[&Path], write: &[&Path]) -> Result<Option<Arc<Self>>, String> {
        if read.is_empty() && write.is_empty() {
            return Ok(None);
        }
        let open = |paths: &[&Path], access: Access| -> Result<Vec<Root>, String> {
            let mut roots: Vec<Root> = Vec::with_capacity(paths.len());
            for path in paths {
                let root = Root::open(path, access)?;
                if !roots.iter().any(|r| r.resolved == root.resolved) {
                    roots.push(root);
                }
            }
            Ok(roots)
        };
        Ok(Some(Arc::new(Self { read: open(read, Access::Read)?, write: open(write, Access::Write)? })))
    }

    /// The roots for people: `read A, B; write C` (`none` for a kind without any), absolute with
    /// their links followed.
    pub fn describe(&self) -> String {
        let list = |roots: &[Root]| {
            if roots.is_empty() {
                return "none".to_string();
            }
            roots.iter().map(|r| r.shown.display().to_string()).collect::<Vec<_>>().join(", ")
        };
        format!("read {}; write {}", list(&self.read), list(&self.write))
    }

    /// May the file or folder at `path` be read (or written)? It must lie inside one of the roots
    /// of that kind. `Err` says why not.
    pub fn check(&self, path: impl AsRef<Path>, access: Access) -> Result<(), String> {
        let path = path.as_ref();
        let roots = match access {
            Access::Read => &self.read,
            Access::Write => &self.write,
        };
        if roots.is_empty() {
            return match access {
                Access::Read => Err(format!("automation filesystem access is not granted: read authority is absent: {}", path.display())),
                Access::Write => Err(format!("automation filesystem access is not granted: write authority is absent: {}", path.display())),
            };
        }
        let resolved = resolve(path)?;
        if roots.iter().any(|r| inside(&resolved, &r.resolved)) {
            return Ok(());
        }
        let shown: Vec<String> = roots.iter().map(|r| r.shown.display().to_string()).collect();
        match (shown.as_slice(), access) {
            ([root], Access::Read) => Err(format!("automation path rejected: outside the read root {root}: {}", path.display())),
            ([root], Access::Write) => Err(format!("automation path rejected: outside the write root {root}: {}", path.display())),
            (_, Access::Read) => Err(format!("automation path rejected: outside the read roots {}: {}", shown.join(", "), path.display())),
            (_, Access::Write) => Err(format!("automation path rejected: outside the write roots {}: {}", shown.join(", "), path.display())),
        }
    }
}

/// Where the roots are in force on a thread ([`confine`], [`unconfined`]).
#[derive(Clone, Debug)]
enum Scope {
    Confined(Arc<AutomationRoots>),
    Unconfined,
}

thread_local! {
    /// This thread's scope; `None`: the process's roots ([`confine_process`]).
    static SCOPE: RefCell<Option<Scope>> = const { RefCell::new(None) };
}

/// The roots in force in every thread outside [`confine`] and [`unconfined`].
static PROCESS: OnceLock<Arc<AutomationRoots>> = OnceLock::new();

/// Put `roots` in force in the whole process, for good. `false` when roots were in force already
/// (they stay as they were).
pub fn confine_process(roots: Arc<AutomationRoots>) -> bool {
    PROCESS.set(roots).is_ok()
}

/// Puts the scope it replaced back when dropped (also when `f` panics).
struct Restore(Option<Scope>);

impl Drop for Restore {
    fn drop(&mut self) {
        let previous = self.0.take();
        SCOPE.with(|s| *s.borrow_mut() = previous);
    }
}

fn with_scope<T>(scope: Option<Scope>, f: impl FnOnce() -> T) -> T {
    let _restore = Restore(SCOPE.with(|s| s.replace(scope)));
    f()
}

/// Run `f` with `roots` in force on this thread (`None`: as things stand).
pub fn confine<T>(roots: Option<&Arc<AutomationRoots>>, f: impl FnOnce() -> T) -> T {
    match roots {
        Some(r) => with_scope(Some(Scope::Confined(r.clone())), f),
        None => f(),
    }
}

/// Run `f` with no roots in force on this thread: what the person at the keyboard does, and the
/// app's own folders.
pub fn unconfined<T>(f: impl FnOnce() -> T) -> T {
    with_scope(Some(Scope::Unconfined), f)
}

/// The roots in force on this thread, if any.
pub fn current() -> Option<Arc<AutomationRoots>> {
    match SCOPE.with(|s| s.borrow().clone()) {
        Some(Scope::Confined(r)) => Some(r),
        Some(Scope::Unconfined) => None,
        None => PROCESS.get().cloned(),
    }
}

/// `path` without Windows' `\\?\` prefix (which `canonicalize` adds), for people to read.
fn plain(path: PathBuf) -> PathBuf {
    if !cfg!(windows) {
        return path;
    }
    let text = path.to_string_lossy();
    if let Some(rest) = text.strip_prefix(r"\\?\UNC\") {
        return PathBuf::from(format!(r"\\{rest}"));
    }
    match text.strip_prefix(r"\\?\") {
        Some(rest) => PathBuf::from(rest),
        None => path,
    }
}

/// May `path` be read here? Always, without roots in force.
pub fn check_read(path: impl AsRef<Path>) -> Result<(), String> {
    current().map_or(Ok(()), |r| r.check(path, Access::Read))
}

/// May `path` be written here? Always, without roots in force.
pub fn check_write(path: impl AsRef<Path>) -> Result<(), String> {
    current().map_or(Ok(()), |r| r.check(path, Access::Write))
}

/// `path` made absolute with its links followed; for a path that doesn't exist (yet), its nearest
/// existing folder's links followed and the rest appended.
fn resolve(path: &Path) -> Result<PathBuf, String> {
    if path.as_os_str().is_empty() {
        return Err("automation path rejected: path is empty".into());
    }
    // Checked as given: making `…\NUL` absolute gives the device path `\\.\NUL` on Windows.
    if cfg!(windows) && path.components().any(|c| matches!(c, Component::Normal(n) if is_device_name(&n.to_string_lossy()))) {
        return Err(format!("automation path rejected: reserved device names are not allowed: {}", path.display()));
    }
    let absolute = std::path::absolute(path).map_err(|e| format!("automation path rejected: cannot resolve {}: {e}", path.display()))?;
    // The names below the nearest existing folder, last first.
    let mut missing: Vec<OsString> = vec![];
    let mut at = absolute.as_path();
    loop {
        match std::fs::canonicalize(at) {
            Ok(mut found) => {
                found.extend(missing.iter().rev());
                return Ok(found);
            }
            Err(e) => match std::fs::symlink_metadata(at) {
                Ok(m) if m.file_type().is_symlink() => {
                    return Err(format!("automation path rejected: a link that leads nowhere: {}", path.display()));
                }
                // There, yet its real path can't be had (no permission): nothing to compare.
                Ok(_) => return Err(format!("automation path rejected: cannot resolve {}: {e}", path.display())),
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
                Err(e) => return Err(format!("automation path rejected: cannot resolve {}: {e}", path.display())),
            },
        }
        let parent = at.parent();
        match (at.components().next_back(), parent) {
            (Some(Component::Normal(name)), Some(parent)) => {
                missing.push(name.to_os_string());
                at = parent;
            }
            (Some(Component::ParentDir), _) => {
                return Err(format!("automation path rejected: parent traversal below a folder that doesn't exist: {}", path.display()));
            }
            // Down to the root (or drive) and it doesn't exist either.
            _ => {
                let e = std::io::Error::from(std::io::ErrorKind::NotFound);
                return Err(format!("automation path rejected: cannot resolve {}: {e}", path.display()));
            }
        }
    }
}

/// Is `path` (resolved) `root` or inside it, component by component (without regard to case on
/// Windows, whose file systems ignore it)?
fn inside(path: &Path, root: &Path) -> bool {
    let mut path = path.components();
    root.components().all(|r| path.next().is_some_and(|p| same_component(p, r)))
}

fn same_component(a: Component, b: Component) -> bool {
    if a == b {
        return true;
    }
    if !cfg!(windows) {
        return false;
    }
    match (a.as_os_str().to_str(), b.as_os_str().to_str()) {
        (Some(a), Some(b)) => a.to_lowercase() == b.to_lowercase(),
        _ => false,
    }
}

/// A Windows device name (`CON`, `NUL`, `COM1`… also with an extension), which names no file.
fn is_device_name(component: &str) -> bool {
    let stem = component.split('.').next().unwrap_or(component).trim_end_matches(' ').to_ascii_uppercase();
    let numbered = |prefix: &str| stem.strip_prefix(prefix).is_some_and(|n| matches!(n, "1" | "2" | "3" | "4" | "5" | "6" | "7" | "8" | "9"));
    matches!(stem.as_str(), "CON" | "PRN" | "AUX" | "NUL" | "CLOCK$" | "CONIN$" | "CONOUT$") || numbered("COM") || numbered("LPT")
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A fresh folder holding `in` (read root), `out` (write root) and `outside`.
    fn folders(name: &str) -> (PathBuf, Arc<AutomationRoots>) {
        let base = std::env::temp_dir().join(format!("dc-file-access-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&base);
        for d in ["in/sub", "out", "outside", "in2"] {
            std::fs::create_dir_all(base.join(d)).unwrap();
        }
        std::fs::write(base.join("in/a.designcraft"), "x").unwrap();
        std::fs::write(base.join("outside/secret.designcraft"), "x").unwrap();
        let roots = AutomationRoots::new(&[&base.join("in")], &[&base.join("out")]).unwrap().unwrap();
        (base, roots)
    }

    fn s(p: PathBuf) -> String {
        p.to_string_lossy().into_owned()
    }

    #[test]
    fn paths_inside_the_roots_pass_and_others_dont() {
        let (base, roots) = folders("inside");
        assert_eq!(roots.check(s(base.join("in/a.designcraft")), Access::Read), Ok(()));
        assert_eq!(roots.check(s(base.join("in")), Access::Read), Ok(()), "the root itself");
        assert_eq!(roots.check(s(base.join("in/sub/new.designcraft")), Access::Read), Ok(()), "missing files are checked by their folder");
        assert_eq!(roots.check(s(base.join("out/new/deeper/x.png")), Access::Write), Ok(()), "and by their nearest folder");
        let e = roots.check(s(base.join("outside/secret.designcraft")), Access::Read).unwrap_err();
        assert!(e.starts_with("automation path rejected: outside the read root"), "{e}");
        // Read and write access are separate.
        let e = roots.check(s(base.join("in/a.designcraft")), Access::Write).unwrap_err();
        assert!(e.contains("outside the write root"), "{e}");
        assert!(roots.check(s(base.join("out/x.png")), Access::Read).unwrap_err().contains("outside the read root"));
        let _ = std::fs::remove_dir_all(base);
    }

    #[test]
    fn a_sibling_with_the_same_prefix_is_outside() {
        let (base, roots) = folders("prefix");
        std::fs::write(base.join("in2/b.designcraft"), "x").unwrap();
        assert!(roots.check(s(base.join("in2/b.designcraft")), Access::Read).is_err(), "in2 is not inside in");
        assert!(roots.check(format!("{}2/c.designcraft", s(base.join("in"))), Access::Read).is_err());
        let _ = std::fs::remove_dir_all(base);
    }

    #[test]
    fn parent_traversal_is_resolved_or_refused() {
        let (base, roots) = folders("dotdot");
        let sep = std::path::MAIN_SEPARATOR;
        let up = format!("{}{sep}..{sep}outside{sep}secret.designcraft", s(base.join("in")));
        assert!(roots.check(&up, Access::Read).unwrap_err().contains("outside the read root"), "`..` out of the root");
        let back = format!("{}{sep}..{sep}a.designcraft", s(base.join("in/sub")));
        assert_eq!(roots.check(&back, Access::Read), Ok(()), "`..` that stays inside");
        let missing = format!("{}{sep}nope{sep}..{sep}..{sep}outside{sep}x.designcraft", s(base.join("out")));
        let e = roots.check(&missing, Access::Write).unwrap_err();
        // Windows resolves `..` by name first (so does opening the file there): then it's outside.
        assert!(e.contains("parent traversal below a folder that doesn't exist") || e.contains("outside the write root"), "{e}");
        let _ = std::fs::remove_dir_all(base);
    }

    #[test]
    fn relative_paths_resolve_against_the_working_directory() {
        let (base, roots) = folders("relative");
        // The tests' working directory is the crate's folder: not inside the roots.
        assert!(roots.check("a.designcraft", Access::Read).unwrap_err().contains("outside the read root"));
        assert!(roots.check("", Access::Read).unwrap_err().contains("path is empty"));
        let _ = std::fs::remove_dir_all(base);
    }

    #[test]
    fn a_missing_root_is_an_error_and_none_is_no_confinement() {
        let (base, _) = folders("missing");
        let e = AutomationRoots::new(&[&base.join("nope")], &[]).unwrap_err();
        assert!(e.starts_with("cannot open the automation read root"), "{e}");
        let e = AutomationRoots::new(&[], &[&base.join("in/a.designcraft")]).unwrap_err();
        assert!(
            e.starts_with("cannot open the automation write root")
                && e.ends_with(&std::io::Error::from(std::io::ErrorKind::NotADirectory).to_string()),
            "{e}"
        );
        assert!(AutomationRoots::new(&[Path::new("")], &[]).unwrap_err().starts_with("cannot open the automation read root ``"));
        assert!(AutomationRoots::new(&[], &[]).unwrap().is_none());
        // One root alone grants only its own access.
        let read_only = AutomationRoots::new(&[&base.join("in")], &[]).unwrap().unwrap();
        let e = read_only.check(s(base.join("in/x.designcraft")), Access::Write).unwrap_err();
        assert!(e.contains("write authority is absent"), "{e}");
        let _ = std::fs::remove_dir_all(base);
    }

    #[test]
    fn links_are_followed_before_comparing() {
        let (base, roots) = folders("links");
        #[cfg(unix)]
        let made = |target: &Path, link: &Path| std::os::unix::fs::symlink(target, link).is_ok();
        #[cfg(windows)]
        let made = |target: &Path, link: &Path| {
            let made =
                if target.is_dir() { std::os::windows::fs::symlink_dir(target, link) } else { std::os::windows::fs::symlink_file(target, link) };
            made.is_ok()
        };
        #[cfg(not(any(unix, windows)))]
        let made = |_: &Path, _: &Path| false;
        // Windows needs Developer Mode or elevation to make links: nothing to test without them.
        if !made(&base.join("outside/secret.designcraft"), &base.join("in/leak.designcraft")) {
            eprintln!("skipped: symbolic links can't be made here");
            let _ = std::fs::remove_dir_all(base);
            return;
        }
        assert!(roots.check(s(base.join("in/leak.designcraft")), Access::Read).unwrap_err().contains("outside the read root"), "a file link out");
        assert!(made(&base.join("outside"), &base.join("out/away")));
        let e = roots.check(s(base.join("out/away/new.png")), Access::Write).unwrap_err();
        assert!(e.contains("outside the write root"), "a folder link out: {e}");
        assert!(made(&base.join("outside/gone.designcraft"), &base.join("out/dangling.png")));
        let e = roots.check(s(base.join("out/dangling.png")), Access::Write).unwrap_err();
        assert!(e.contains("a link that leads nowhere"), "writing would create the file outside: {e}");
        // A link to a folder inside the root stays inside.
        assert!(made(&base.join("in/sub"), &base.join("in/alias")));
        assert_eq!(roots.check(s(base.join("in/alias/x.designcraft")), Access::Read), Ok(()));
        // A root given through a link is the folder it leads to.
        assert!(made(&base.join("in"), &base.join("in-link")));
        let linked = AutomationRoots::new(&[&base.join("in-link")], &[]).unwrap().unwrap();
        assert_eq!(linked.check(s(base.join("in/a.designcraft")), Access::Read), Ok(()));
        let _ = std::fs::remove_dir_all(base);
    }

    /// `in` and `shared` (read roots), `out` and `scratch` (write roots) beside `outside`, the
    /// siblings `in2` and `scratch2` whose names start like a root's, and a file in their parent.
    /// The folder is returned resolved (`/var` is `/private/var` on macOS), as the roots show it, so
    /// a path that passes only because it was resolved is the case being tested and nothing else.
    /// It has no `\\?\` prefix on Windows, where the roots are shown without it and `..` is not
    /// resolved in a path that has it.
    fn several(name: &str) -> (PathBuf, Arc<AutomationRoots>) {
        let (base, _) = folders(name);
        let base = plain(std::fs::canonicalize(base).unwrap());
        for d in ["shared", "scratch", "scratch2"] {
            std::fs::create_dir_all(base.join(d)).unwrap();
        }
        for f in ["shared/b.designcraft", "in2/c.designcraft", "parent.designcraft"] {
            std::fs::write(base.join(f), "x").unwrap();
        }
        let roots = AutomationRoots::new(&[&base.join("in"), &base.join("shared")], &[&base.join("out"), &base.join("scratch")]).unwrap().unwrap();
        (base, roots)
    }

    /// #1552: several roots of a kind: a path inside any of them passes, one inside none is refused
    /// naming them all, and read and write stay separate.
    #[test]
    fn several_roots_of_a_kind_each_grant_their_access() {
        let (base, roots) = several("several");
        assert_eq!(roots.check(s(base.join("in/a.designcraft")), Access::Read), Ok(()));
        assert_eq!(roots.check(s(base.join("shared/b.designcraft")), Access::Read), Ok(()));
        assert_eq!(roots.check(s(base.join("out/x.png")), Access::Write), Ok(()));
        assert_eq!(roots.check(s(base.join("scratch/new/x.png")), Access::Write), Ok(()));
        let e = roots.check(s(base.join("outside/secret.designcraft")), Access::Read).unwrap_err();
        let listed = format!("outside the read roots {}, {}: ", s(base.join("in")), s(base.join("shared")));
        assert!(e.starts_with("automation path rejected: ") && e.contains(&listed), "{e}");
        assert!(roots.check(s(base.join("outside/x.png")), Access::Write).unwrap_err().contains("outside the write roots"));
        // Two sibling roots don't make their parent a root.
        assert!(roots.check(s(base.join("parent.designcraft")), Access::Read).unwrap_err().contains("outside the read roots"));
        assert!(roots.check(s(base.clone()), Access::Read).is_err());
        assert!(roots.check(s(base.join("x.png")), Access::Write).unwrap_err().contains("outside the write roots"));
        // Read and write stay separate: a read root is not a write root, nor the other way round.
        assert!(roots.check(s(base.join("shared/b.designcraft")), Access::Write).unwrap_err().contains("outside the write roots"));
        assert!(roots.check(s(base.join("in/x.png")), Access::Write).is_err());
        assert!(roots.check(s(base.join("scratch/x.png")), Access::Read).unwrap_err().contains("outside the read roots"));
        assert!(roots.check(s(base.join("out")), Access::Read).is_err());
        let _ = std::fs::remove_dir_all(base);
    }

    #[test]
    fn several_roots_compare_whole_folder_names() {
        let (base, roots) = several("several-prefix");
        assert!(roots.check(s(base.join("in2/c.designcraft")), Access::Read).unwrap_err().contains("outside the read roots"), "in2 is not inside in");
        assert!(roots.check(format!("{}2/x.designcraft", s(base.join("shared"))), Access::Read).is_err());
        assert!(
            roots.check(s(base.join("scratch2/x.png")), Access::Write).unwrap_err().contains("outside the write roots"),
            "scratch2 is not inside scratch"
        );
        let _ = std::fs::remove_dir_all(base);
    }

    #[test]
    fn parent_traversal_between_several_roots() {
        let (base, roots) = several("several-dotdot");
        let sep = std::path::MAIN_SEPARATOR;
        let up = |root: &str, rest: &str| format!("{}{sep}..{sep}{rest}", s(base.join(root)));
        // `..` out of one root is outside, also when it lands next to another root.
        assert!(roots.check(up("shared", &format!("outside{sep}secret.designcraft")), Access::Read).unwrap_err().contains("outside the read roots"));
        assert!(roots.check(up("shared", "parent.designcraft"), Access::Read).is_err());
        assert!(roots.check(up("scratch", &format!("outside{sep}x.png")), Access::Write).unwrap_err().contains("outside the write roots"));
        // `..` from one root into another of the same kind stays inside.
        assert_eq!(roots.check(up("shared", &format!("in{sep}a.designcraft")), Access::Read), Ok(()));
        assert_eq!(roots.check(up("scratch", &format!("out{sep}x.png")), Access::Write), Ok(()));
        // ...but not into a root of the other kind.
        assert!(roots.check(up("scratch", &format!("in{sep}x.png")), Access::Write).unwrap_err().contains("outside the write roots"));
        assert!(roots.check(up("shared", &format!("out{sep}x.png")), Access::Read).unwrap_err().contains("outside the read roots"));
        let _ = std::fs::remove_dir_all(base);
    }

    #[test]
    fn several_roots_are_each_checked_and_counted_once() {
        let (base, _) = folders("several-open");
        // A missing folder among several is an error, wherever it stands.
        for read in [[base.join("in"), base.join("nope")], [base.join("nope"), base.join("in")]] {
            let e = AutomationRoots::new(&[&read[0], &read[1]], &[]).unwrap_err();
            assert!(e.starts_with("cannot open the automation read root") && e.contains("nope"), "{e}");
        }
        let e = AutomationRoots::new(&[&base.join("in")], &[&base.join("out"), &base.join("in/a.designcraft")]).unwrap_err();
        assert!(e.starts_with("cannot open the automation write root"), "{e}");
        // A folder given twice (here once through `..`) counts once.
        let twice = AutomationRoots::new(&[&base.join("in"), &base.join("in/sub/.."), &base.join("in")], &[]).unwrap().unwrap();
        let real = plain(std::fs::canonicalize(&base).unwrap());
        assert_eq!(twice.describe(), format!("read {}; write none", s(real.join("in"))));
        let e = twice.check(s(base.join("outside/x")), Access::Read).unwrap_err();
        assert!(e.contains("outside the read root ") && !e.contains("roots"), "{e}");
        let _ = std::fs::remove_dir_all(base);
    }

    /// A link inside one root that leads into another root of the same kind stays inside; one that
    /// leads out of all of them, or into a root of the other kind, is outside.
    #[cfg(unix)]
    #[test]
    fn links_between_several_roots() {
        let (base, roots) = several("several-links");
        let link = |target: &str, at: &str| std::os::unix::fs::symlink(base.join(target), base.join(at)).unwrap();
        link("shared/b.designcraft", "in/to-shared.designcraft");
        link("outside/secret.designcraft", "shared/leak.designcraft");
        link("", "shared/up");
        link("out", "in/to-out");
        link("outside", "scratch/away");
        link("in", "scratch/to-in");
        assert_eq!(roots.check(s(base.join("in/to-shared.designcraft")), Access::Read), Ok(()));
        assert!(roots.check(s(base.join("shared/leak.designcraft")), Access::Read).unwrap_err().contains("outside the read roots"));
        assert!(roots.check(s(base.join("shared/up/outside/secret.designcraft")), Access::Read).unwrap_err().contains("outside the read roots"));
        assert!(roots.check(s(base.join("shared/up/parent.designcraft")), Access::Read).is_err());
        assert!(roots.check(s(base.join("in/to-out/x.png")), Access::Read).unwrap_err().contains("outside the read roots"));
        assert!(roots.check(s(base.join("scratch/away/x.png")), Access::Write).unwrap_err().contains("outside the write roots"));
        assert!(roots.check(s(base.join("scratch/to-in/x.png")), Access::Write).unwrap_err().contains("outside the write roots"));
        let _ = std::fs::remove_dir_all(base);
    }

    #[cfg(windows)]
    #[test]
    fn windows_ignores_case_and_refuses_devices() {
        let (base, roots) = folders("case");
        let upper = s(base.join("IN/a.designcraft"));
        assert_eq!(roots.check(&upper, Access::Read), Ok(()), "{upper}");
        assert_eq!(roots.check(s(base.join("In/Sub/New.designcraft")), Access::Read), Ok(()));
        let e = roots.check(s(base.join("out/NUL")), Access::Write).unwrap_err();
        assert!(e.contains("reserved device names"), "{e}");
        assert!(roots.check(s(base.join("out/com1.txt")), Access::Write).is_err());
        assert_eq!(roots.check(s(base.join("out/console.txt")), Access::Write), Ok(()));
        let _ = std::fs::remove_dir_all(base);
    }

    #[test]
    fn scopes_nest_and_stay_on_their_thread() {
        let (base, roots) = folders("scopes");
        let outside = s(base.join("outside/secret.designcraft"));
        assert_eq!(check_read(&outside), Ok(()), "nothing is confined by default");
        confine(Some(&roots), || {
            assert!(check_read(&outside).is_err());
            assert_eq!(unconfined(|| check_read(&outside)), Ok(()));
            assert!(check_read(&outside).is_err(), "restored after unconfined");
            let o = outside.clone();
            let there = std::thread::spawn(move || check_read(&o).is_ok()).join().unwrap();
            assert!(there, "another thread is not confined by this thread's scope");
        });
        assert_eq!(check_read(&outside), Ok(()), "restored after confine");
        let r = std::panic::catch_unwind(|| confine(Some(&roots), || panic!("boom")));
        assert!(r.is_err());
        assert_eq!(check_read(&outside), Ok(()), "restored after a panic");
        assert_eq!(confine(None, || check_read(&outside)), Ok(()));
        let _ = std::fs::remove_dir_all(base);
    }

    #[test]
    fn device_names() {
        for d in ["CON", "nul", "Com1", "LPT9.txt", "aux.designcraft", "NUL "] {
            assert!(is_device_name(d), "{d}");
        }
        for f in ["console", "com10", "COM0", "lpt", "a.designcraft", "nul-x"] {
            assert!(!is_device_name(f), "{f}");
        }
    }
}
