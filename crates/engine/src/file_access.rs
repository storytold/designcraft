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
//!   (`/work2` isn't inside `/work`), in the case the file system stores (so a case-variant name
//!   reaches the same folder on an ordinary Windows volume, and a distinct sibling in a
//!   case-sensitive one stays outside). Two roots don't make their common parent one.
//! - On Windows a network path (`\\server\share`, `\\?\UNC\...`, WebDAV's `\\host@SSL\share`), a
//!   device path (`\\.\...`) or a verbatim path that isn't a disk is refused by its name before
//!   the file system is asked about it, unless a root is on that same server and share (or that
//!   same device or prefix). Looking such a path up would contact a host the agent chose. A link
//!   whose text leads to one is refused before it is followed.
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
        let folders: Vec<&Path> = roots.iter().map(|r| r.resolved.as_path()).collect();
        let resolved = resolve(path, &folders)?;
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
///
/// A Windows network, device or non-disk verbatim prefix is refused before any of that, unless one
/// of `roots` (resolved folders) already stands on the same server and share, device or prefix.
fn resolve(path: &Path, roots: &[&Path]) -> Result<PathBuf, String> {
    if path.as_os_str().is_empty() {
        return Err("automation path rejected: path is empty".into());
    }
    // Checked as given: making `…\NUL` absolute gives the device path `\\.\NUL` on Windows.
    if cfg!(windows) && path.components().any(|c| matches!(c, Component::Normal(n) if is_device_name(&n.to_string_lossy()))) {
        return Err(format!("automation path rejected: reserved device names are not allowed: {}", path.display()));
    }
    // Before `absolute` too: on Windows that call asks the system about the path.
    if let Some(why) = refused_prefix(path, roots) {
        return Err(format!("automation path rejected: {why}: {}", path.display()));
    }
    let absolute = std::path::absolute(path).map_err(|e| format!("automation path rejected: cannot resolve {}: {e}", path.display()))?;
    if let Some(why) = refused_prefix(&absolute, roots) {
        return Err(format!("automation path rejected: {why}: {}", path.display()));
    }
    // A link is judged by its own text: canonicalizing one that leads to a network path would
    // contact that host. Only Windows has such prefixes. One walk of the whole path covers every
    // shorter one the loop below tries.
    if cfg!(windows)
        && let Some(why) = link_leads_to_refused_prefix(&absolute, roots)
    {
        return Err(format!("automation path rejected: {why}: {}", path.display()));
    }
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

/// Is `path` (resolved) `root` or inside it, component by component? Both are canonical, so each
/// component carries the case the file system stores: a path that differs only in case names the
/// same folder on an ordinary Windows volume (and resolves to the same components), and a distinct
/// sibling in a case-sensitive folder (which must not pass as the root).
fn inside(path: &Path, root: &Path) -> bool {
    let mut path = path.components();
    root.components().all(|r| path.next() == Some(r))
}

/// A Windows device name (`CON`, `NUL`, `COM1`, `COM¹`… also with an extension or a colon), which names no file.
pub(crate) fn is_device_name(component: &str) -> bool {
    let stem = component.split(['.', ':']).next().unwrap_or(component).trim_end_matches(' ').to_ascii_uppercase();
    let numbered =
        |prefix: &str| stem.strip_prefix(prefix).is_some_and(|n| matches!(n, "1" | "2" | "3" | "4" | "5" | "6" | "7" | "8" | "9" | "¹" | "²" | "³"));
    matches!(stem.as_str(), "CON" | "PRN" | "AUX" | "NUL" | "CLOCK$" | "CONIN$" | "CONOUT$") || numbered("COM") || numbered("LPT")
}

/// A Windows path prefix that isn't a local disk: opening or even resolving a path that starts
/// with one can reach another computer.
#[derive(Debug)]
enum OffDisk {
    /// `\\server\share` or `\\?\UNC\server\share`; WebDAV's `\\host@SSL\share` is a server name too.
    Unc(OsString, OsString),
    /// `\\.\device`.
    Device(OsString),
    /// `\\?\` followed by anything but a drive or `UNC`.
    Verbatim(OsString),
}

impl OffDisk {
    /// The prefix of `path` when it isn't a local disk. A drive letter, a path without a prefix
    /// and every path off Windows give `None`.
    fn of(path: &Path) -> Option<Self> {
        use std::path::Prefix;
        let Some(Component::Prefix(prefix)) = path.components().next() else { return Self::unparsed_unc(path) };
        match prefix.kind() {
            Prefix::Disk(_) | Prefix::VerbatimDisk(_) => None,
            Prefix::UNC(server, share) | Prefix::VerbatimUNC(server, share) => Some(Self::Unc(server.to_os_string(), share.to_os_string())),
            Prefix::DeviceNS(device) => Some(Self::Device(device.to_os_string())),
            Prefix::Verbatim(rest) => Some(Self::Verbatim(rest.to_os_string())),
        }
    }

    /// A path that starts with two separators on Windows but isn't a whole `\\server\share` (`\\server`,
    /// `\\\x`): Windows still reads it as the name of a computer, so it is a UNC path with no share.
    fn unparsed_unc(path: &Path) -> Option<Self> {
        let bytes = path.as_os_str().as_encoded_bytes();
        let separator = |b: Option<&u8>| matches!(b, Some(b'\\' | b'/'));
        if cfg!(windows) && separator(bytes.first()) && separator(bytes.get(1)) { Some(Self::Unc(OsString::new(), OsString::new())) } else { None }
    }

    /// The refusal for a path with this prefix, which names the prefix.
    fn refusal(&self) -> &'static str {
        match self {
            Self::Unc(..) => "UNC paths are refused unless a root is on the same server and share",
            Self::Device(_) => "device paths are refused unless a root uses the same device",
            Self::Verbatim(_) => "verbatim paths are refused unless a root uses the same prefix",
        }
    }

    /// Whether `other` is the same server and share, device or prefix, the ASCII case aside. This
    /// decides only whether that host may be contacted; [`inside`] still decides the folder.
    fn same_as(&self, other: &Self) -> bool {
        let same = |a: &OsString, b: &OsString| a.eq_ignore_ascii_case(b);
        match (self, other) {
            (Self::Unc(server, share), Self::Unc(other_server, other_share)) => same(server, other_server) && same(share, other_share),
            (Self::Device(device), Self::Device(other)) => same(device, other),
            (Self::Verbatim(rest), Self::Verbatim(other)) => same(rest, other),
            _ => false,
        }
    }
}

/// Why `path` must not be looked up: it starts with a Windows network, device or non-disk
/// verbatim prefix and none of `roots` stands on that same prefix. A local path gives `None`.
fn refused_prefix(path: &Path, roots: &[&Path]) -> Option<&'static str> {
    let prefix = OffDisk::of(path)?;
    if roots.iter().filter_map(|root| OffDisk::of(root)).any(|root| root.same_as(&prefix)) { None } else { Some(prefix.refusal()) }
}

/// How many links in a row [`link_leads_to_refused_prefix`] follows before it gives up and refuses.
const MAX_LINK_HOPS: usize = 40;

/// How many spellings of a path one walk scans in all, so a hostile set of links can't make it
/// scan without end.
const MAX_SPELLINGS: usize = 1024;

/// The refusal for a link in `path`, taken from the link's own text.
///
/// Folders are inspected from the top down with [`std::fs::symlink_metadata`], which doesn't follow
/// a link, and a link's target is read with [`std::fs::read_link`]. A link to a network path is
/// therefore refused before [`std::fs::canonicalize`] could open that host, also when the link is
/// only an ancestor of the name. A link whose target can't be read, the one after
/// [`MAX_LINK_HOPS`] in a row, or a walk past [`MAX_SPELLINGS`] spellings, is refused rather than
/// opened.
fn link_leads_to_refused_prefix(path: &Path, roots: &[&Path]) -> Option<&'static str> {
    follow_links(path, roots, &mut std::collections::HashSet::new(), MAX_LINK_HOPS)
}

/// [`link_leads_to_refused_prefix`] for one spelling of the path. `judged` holds every spelling the
/// walk has scanned (so none is scanned twice, nor in a cycle forever) and `hops_left` the links
/// still allowed in this chain, which the other spelling of a target does not use up.
fn follow_links(path: &Path, roots: &[&Path], judged: &mut std::collections::HashSet<PathBuf>, hops_left: usize) -> Option<&'static str> {
    if !judged.insert(path.to_path_buf()) {
        return None;
    }
    if judged.len() > MAX_SPELLINGS {
        return Some("too many links");
    }
    let mut ancestors: Vec<&Path> = path.ancestors().collect();
    ancestors.reverse();
    for prefix in ancestors {
        if prefix.as_os_str().is_empty() {
            continue;
        }
        if let Some(why) = refused_prefix(prefix, roots) {
            return Some(why);
        }
        let meta = match std::fs::symlink_metadata(prefix) {
            Ok(meta) => meta,
            // Nothing is there, so no link is below it.
            Err(e) if matches!(e.kind(), std::io::ErrorKind::NotFound | std::io::ErrorKind::NotADirectory) => return None,
            // Anything else (no permission to look): a link below it can't be seen, so don't go on.
            Err(_) => return Some("a folder that can't be looked at"),
        };
        if !meta.file_type().is_symlink() {
            continue;
        }
        if hops_left == 0 {
            return Some("too many links");
        }
        let Ok(target) = std::fs::read_link(prefix) else { return Some("a link that leads nowhere") };
        // Judged as written: joined onto the link's folder, a bare `\\server` would keep that folder's drive.
        if let Some(why) = refused_prefix(&target, roots) {
            return Some(why);
        }
        let joined = match prefix.parent() {
            Some(parent) if !target.is_absolute() && !parent.as_os_str().is_empty() => parent.join(target),
            _ => target,
        };
        let next = match path.strip_prefix(prefix) {
            Ok(rest) if !rest.as_os_str().is_empty() => joined.join(rest),
            _ => joined,
        };
        if let Some(why) = refused_prefix(&next, roots) {
            return Some(why);
        }
        // Windows takes the `..` of a relative target by name, so `missing\..\remote` is `remote`
        // though `missing` isn't there: judge that spelling as well as the one written.
        let by_name = lexical(&next);
        if by_name != next
            && let Some(why) = follow_links(&by_name, roots, judged, hops_left - 1)
        {
            return Some(why);
        }
        return follow_links(&next, roots, judged, hops_left - 1);
    }
    None
}

/// `path` with its `.` and `..` taken by name, never above its root.
fn lexical(path: &Path) -> PathBuf {
    let mut out = PathBuf::new();
    for component in path.components() {
        match component {
            Component::ParentDir => {
                out.pop();
            }
            Component::CurDir => {}
            other => out.push(other),
        }
    }
    out
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
        let text = p.to_string_lossy();
        if !cfg!(windows) {
            return text.into_owned();
        }
        // A canonical path on Windows has a `\\?\` prefix, under which `..` is not a name:
        // send ordinary paths, as the roots show them.
        if let Some(rest) = text.strip_prefix(r"\\?\UNC\") {
            return format!(r"\\{rest}");
        }
        match text.strip_prefix(r"\\?\") {
            Some(rest) => rest.to_owned(),
            None => text.into_owned(),
        }
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

    /// On Windows an existing file spelled in another case is still inside its root, and `NUL`,
    /// `COM1` and `COM¹` are refused.
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
        let e = roots.check(s(base.join("out/COM¹.txt")), Access::Write).unwrap_err();
        assert!(e.contains("reserved device names"), "{e}");
        assert_eq!(roots.check(s(base.join("out/console.txt")), Access::Write), Ok(()));
        let _ = std::fs::remove_dir_all(base);
    }

    /// A path with no Windows prefix, a drive letter, and every path off Windows have no off-disk prefix to refuse.
    #[test]
    fn a_local_path_has_no_off_disk_prefix() {
        for p in ["/home/me/a.designcraft", "a.designcraft", r"C:\Projects\a.designcraft", r"\\?\C:\Projects\a.designcraft"] {
            assert!(OffDisk::of(Path::new(p)).is_none(), "{p}");
            assert_eq!(refused_prefix(Path::new(p), &[]), None, "{p}");
        }
    }

    /// A network, device or non-disk verbatim path is refused by its name before the file system is
    /// asked, so no host the agent names is contacted. The error names the prefix; it is not the
    /// error that opening the path would give. Nothing here touches the network.
    #[cfg(windows)]
    #[test]
    fn a_network_path_is_refused_before_it_is_opened() {
        let (base, roots) = folders("unc");
        let paths = [
            (r"\\203.0.113.5\s\x.designcraft", "UNC"),
            (r"\\?\UNC\203.0.113.5\s\x.designcraft", "UNC"),
            ("//203.0.113.5/s/x.designcraft", "UNC"),
            (r"\\host@SSL\share\x.designcraft", "UNC"),
            (r"\\host@SSL@443\share\x.designcraft", "UNC"),
            (r"\\203.0.113.5", "UNC"),
            (r"\\\203.0.113.5\s\x.designcraft", "UNC"),
            ("//203.0.113.5", "UNC"),
            (r"\\.\PhysicalDrive0", "device path"),
            (r"\\?\Volume{00000000-0000-0000-0000-000000000000}\x.designcraft", "verbatim path"),
        ];
        for (p, kind) in paths {
            for access in [Access::Read, Access::Write] {
                let e = roots.check(p, access).unwrap_err();
                assert!(e.starts_with("automation path rejected:"), "{p}: {e}");
                assert!(e.contains(kind), "{p}: {e}");
                assert!(!e.contains("cannot resolve") && !e.to_ascii_lowercase().contains("os error"), "{p}: {e}");
            }
        }
        let e = roots.check(s(base.join("outside/secret.designcraft")), Access::Read).unwrap_err();
        assert!(e.contains("outside the read root"), "a local path is still judged by its folder: {e}");
        let verbatim = format!(r"\\?\{}", s(base.join("in").join("a.designcraft")));
        assert_eq!(roots.check(&verbatim, Access::Read), Ok(()), "a verbatim disk path is a local path: {verbatim}");
        let _ = std::fs::remove_dir_all(base);
    }

    /// A root on a share lets that server and share through (either spelling, any ASCII case) and nothing else.
    #[cfg(windows)]
    #[test]
    fn a_root_on_a_share_lets_only_that_share_through() {
        let share = [Path::new(r"\\?\UNC\FileServer\Projects\work")];
        assert_eq!(refused_prefix(Path::new(r"\\fileserver\projects\a.designcraft"), &share), None);
        assert_eq!(refused_prefix(Path::new(r"\\?\UNC\FILESERVER\PROJECTS\a.designcraft"), &share), None);
        assert!(refused_prefix(Path::new(r"\\fileserver\secret\a.designcraft"), &share).is_some(), "another share");
        assert!(refused_prefix(Path::new(r"\\203.0.113.5\Projects\a.designcraft"), &share).is_some(), "another server");
        let work = [Path::new(r"\\?\UNC\work\Projects\w")];
        assert!(refused_prefix(Path::new("\\\\wor\u{212a}\\Projects\\a.designcraft"), &work).is_some(), "U+212A is not k");
        assert!(refused_prefix(Path::new(r"\\.\PhysicalDrive0"), &share).is_some(), "a share is not a device");
        let device = [Path::new(r"\\.\PhysicalDrive0")];
        assert_eq!(refused_prefix(Path::new(r"\\.\physicaldrive0"), &device), None, "the same device");
        assert!(refused_prefix(Path::new(r"\\.\PhysicalDrive1"), &device).is_some());
    }

    /// A link whose text is a bare `\\server` (no share) is refused as written: joined onto the link's folder
    /// it would read as a local path and pass.
    #[cfg(windows)]
    #[test]
    fn a_link_to_a_bare_server_name_is_refused() {
        let (base, roots) = folders("unc-link-bare");
        let link = base.join("in").join("bare");
        if std::os::windows::fs::symlink_dir(r"\\203.0.113.5", &link).is_err() {
            eprintln!("skipped: symbolic links can't be made here");
            let _ = std::fs::remove_dir_all(base);
            return;
        }
        let e = roots.check(s(link.join("x.designcraft")), Access::Read).unwrap_err();
        assert!(e.contains("UNC"), "{e}");
        assert!(!e.contains("cannot resolve") && !e.to_ascii_lowercase().contains("os error"), "{e}");
        let _ = std::fs::remove_dir_all(base);
    }

    /// Nine links that each go through `sub\..\` to the next are nine links, not a reason to give up: both
    /// spellings of each target are judged once.
    #[cfg(windows)]
    #[test]
    fn a_chain_of_links_with_parent_names_is_followed_in_few_steps() {
        let (base, roots) = folders("link-chain");
        let folder = base.join("in");
        std::fs::create_dir_all(folder.join("sub")).unwrap();
        for n in 1..=9 {
            let target = if n == 9 { "sub".to_string() } else { format!(r"sub\..\l{}", n + 1) };
            if std::os::windows::fs::symlink_dir(&target, folder.join(format!("l{n}"))).is_err() {
                eprintln!("skipped: symbolic links can't be made here");
                let _ = std::fs::remove_dir_all(base);
                return;
            }
        }
        assert_eq!(roots.check(s(folder.join("l1").join("x.designcraft")), Access::Read), Ok(()));
        let _ = std::fs::remove_dir_all(base);
    }

    /// A link whose relative target goes through a missing folder and back (`missing\..\remote`) is
    /// judged with the `..` taken by name too, as Windows takes it: `remote` is a link to a network path.
    #[cfg(windows)]
    #[test]
    fn a_relative_link_through_a_missing_folder_still_reaches_the_network_check() {
        let (base, roots) = folders("unc-link-dotdot");
        let (remote, alias) = (base.join("in").join("remote"), base.join("in").join("alias"));
        if std::os::windows::fs::symlink_dir(r"\\203.0.113.5\s", &remote).is_err() {
            eprintln!("skipped: symbolic links can't be made here");
            let _ = std::fs::remove_dir_all(base);
            return;
        }
        assert!(std::os::windows::fs::symlink_dir(r"missing\..\remote", &alias).is_ok());
        let e = roots.check(s(alias.join("x.designcraft")), Access::Read).unwrap_err();
        assert!(e.contains("UNC"), "{e}");
        assert!(!e.contains("cannot resolve") && !e.to_ascii_lowercase().contains("os error"), "{e}");
        let _ = std::fs::remove_dir_all(base);
    }

    /// A link whose text names a network path, or that has one as a folder on the way, is refused
    /// from that text before the link is followed: following it would contact the host.
    #[cfg(windows)]
    #[test]
    fn a_link_to_a_network_path_is_refused_before_it_is_followed() {
        let (base, roots) = folders("unc-link");
        let file_link = base.join("in/leak.designcraft");
        if std::os::windows::fs::symlink_file(r"\\203.0.113.5\s\x.designcraft", &file_link).is_err() {
            eprintln!("skipped: symbolic links can't be made here");
            let _ = std::fs::remove_dir_all(base);
            return;
        }
        let folder_link = base.join("in/folder-leak");
        assert!(std::os::windows::fs::symlink_dir(r"\\203.0.113.5\s", &folder_link).is_ok());
        for path in [file_link, folder_link.join("x.designcraft")] {
            let e = roots.check(s(path.clone()), Access::Read).unwrap_err();
            assert!(e.contains("UNC"), "{}: {e}", path.display());
            assert!(!e.contains("cannot resolve") && !e.to_ascii_lowercase().contains("os error"), "{}: {e}", path.display());
        }
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

    /// `COM¹` to `LPT³`, and a device name before a colon, are reserved. `COM⁴`, `com¹0`, `lpt¹x`
    /// and `console:x` are file names.
    #[test]
    fn inside_compares_components_exactly() {
        assert!(inside(Path::new("/work/in/a"), Path::new("/work/in")));
        assert!(inside(Path::new("/work/in"), Path::new("/work/in")));
        assert!(!inside(Path::new("/work/In/a"), Path::new("/work/in")), "a case-variant sibling is another folder");
        assert!(!inside(Path::new("/work/in2/a"), Path::new("/work/in")));
    }

    #[test]
    fn device_names() {
        for d in ["CON", "nul", "Com1", "LPT9.txt", "aux.designcraft", "NUL ", "COM¹", "Lpt²", "lpt³.txt", "COM1:", "nul:x", "COM¹:"] {
            assert!(is_device_name(d), "{d}");
        }
        for f in ["console", "com10", "COM0", "lpt", "a.designcraft", "nul-x", "COM⁴", "com¹0", "lpt¹x", "console:x"] {
            assert!(!is_device_name(f), "{f}");
        }
    }
}
