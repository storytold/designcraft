//! Workspace tooling: `cargo xtask <command>`.
//!
//! Pure Rust (std + serde_json). External tools (`cargo`, `curl`, `tar`) are
//! invoked through `std::process::Command`.

mod assets;
mod ico;
mod layers;
mod parity;
mod pdf_deps;
mod stats;
mod version;

use std::io::{BufRead, BufReader};
use std::path::{Path, PathBuf};
use std::process::{Command, ExitCode, Stdio};

const USAGE: &str = "\
usage: cargo xtask <command>

commands:
  assets          check that every icon/image/font/asset is attributed in ASSETS.md
  layers          enforce the crate dependency layering (plan/architecture.md §3)
  pdf-deps        verify the pinned PDF backend, including Krilla dependencies
  wasm            cargo check --target wasm32-unknown-unknown for the wasm-safe crates (one call)
  test            workspace tests (cargo nextest when installed, else cargo test), then doctests
  i18n-report     list the interface strings each language shows in English (always passes)
  ci              fmt --check, pdf-deps, clippy -D warnings, test, i18n-report, doctest, assets,
                  layers, wasm (stops at first failure)
  corpus [--download]
                  show where test corpora live; --download fetches PngSuite into corpus/pngsuite
  stats [--exact] count tests and lines per crate (--exact: ask the test harness via `-- --list`)
  parity          recompute the InDesign feature-parity summary in docs/parity-checklist.md
  ico <out.ico> <in.png>...
                  pack square PNGs (<= 256 px) into a Windows .ico (see packaging/icons.sh)
";

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let rest: Vec<&str> = args.iter().skip(1).map(String::as_str).collect();
    let result = match args.first().map(String::as_str) {
        Some("assets") => assets::run(&root()),
        Some("layers") => cmd_layers(),
        Some("pdf-deps") => pdf_deps::run(&root()),
        Some("wasm") => cmd_wasm(),
        Some("test") => cmd_test().and_then(|()| cmd_doctest()),
        Some("i18n-report") => cmd_i18n_report(),
        Some("ci") => cmd_ci(),
        Some("corpus") => cmd_corpus(rest.contains(&"--download")),
        Some("stats") => stats::run(&root(), rest.contains(&"--exact")),
        Some("parity") => parity::run(&root()),
        Some("ico") => ico::run(&rest),
        Some("version") => version::run(&root(), &rest),
        Some("-h" | "--help" | "help") | None => {
            print!("{USAGE}");
            Ok(())
        }
        Some(other) => Err(format!("unknown command `{other}`\n\n{USAGE}")),
    };
    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("error: {e}");
            ExitCode::FAILURE
        }
    }
}

/// Workspace root (parent of the xtask crate).
pub fn root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).parent().expect("xtask has a parent dir").to_path_buf()
}

pub fn cargo() -> Command {
    let mut c = Command::new(std::env::var("CARGO").unwrap_or_else(|_| "cargo".into()));
    c.current_dir(root());
    c
}

fn run(mut cmd: Command, what: &str) -> Result<(), String> {
    eprintln!("$ {what}");
    let status = cmd.status().map_err(|e| format!("{what}: failed to spawn: {e}"))?;
    if status.success() { Ok(()) } else { Err(format!("{what}: exited with {status}")) }
}

pub fn metadata() -> Result<serde_json::Value, String> {
    let out = cargo().args(["metadata", "--format-version", "1", "--no-deps"]).output().map_err(|e| format!("cargo metadata: {e}"))?;
    if !out.status.success() {
        return Err(format!("cargo metadata failed:\n{}", String::from_utf8_lossy(&out.stderr)));
    }
    serde_json::from_slice(&out.stdout).map_err(|e| format!("cargo metadata: bad JSON: {e}"))
}

fn cmd_layers() -> Result<(), String> {
    let crates = layers::from_metadata(&metadata()?)?;
    println!("Dependency layering (plan/architecture.md §3)\n");
    println!("{:<28} {:<14} workspace deps", "crate", "layer");
    for c in &crates {
        let ws: Vec<String> = c
            .deps
            .iter()
            .filter(|d| d.workspace)
            .map(|d| {
                let k = match d.kind {
                    layers::DepKind::Normal => "",
                    layers::DepKind::Dev => " (dev)",
                    layers::DepKind::Build => " (build)",
                };
                format!("{}{k}", layers::short_name(&d.name))
            })
            .collect();
        println!("{:<28} {:<14} {}", c.name, layers::describe(layers::classify(&c.name)), ws.join(", "));
    }
    let violations = layers::check(&crates);
    println!();
    if violations.is_empty() {
        println!("OK: {} crates, no layering violations.", crates.len());
        Ok(())
    } else {
        println!("{} violation(s):", violations.len());
        for v in &violations {
            println!("  - {v}");
        }
        Err(format!("{} layering violation(s)", violations.len()))
    }
}

/// Workspace packages that must build for wasm32: all L0–L5 crates plus
/// the egui shell.
fn wasm_set() -> Result<Vec<String>, String> {
    let crates = layers::from_metadata(&metadata()?)?;
    Ok(crates
        .into_iter()
        .filter(|c| match layers::classify(&c.name) {
            Some(layers::Class::Layer(l)) => l <= 5 || layers::short_name(&c.name) == "ui-egui",
            Some(layers::Class::Standalone) => true,
            _ => false,
        })
        .map(|c| c.name)
        .collect())
}

/// One `cargo check` for the whole set: each crate is checked once, and
/// `--keep-going` still reports every failing crate. Features unify across the
/// set, so each crate is checked with the union of the features the set enables.
fn cmd_wasm() -> Result<(), String> {
    let set = wasm_set()?;
    if set.is_empty() {
        return Err("no wasm-safe crates found in the workspace metadata".into());
    }
    let mut c = cargo();
    c.args(["check", "--target", "wasm32-unknown-unknown", "--keep-going"]);
    for pkg in &set {
        c.args(["-p", pkg]);
    }
    run(c, &format!("cargo check --target wasm32-unknown-unknown --keep-going -p {}", set.join(" -p ")))?;
    println!("\nwasm32-unknown-unknown check: ok ({} crates: {})", set.len(), set.join(", "));
    Ok(())
}

/// Test targets for the test build. Examples are left out so they aren't
/// linked; clippy `--all-targets` still compiles them.
const TEST_TARGETS: [&str; 4] = ["--workspace", "--lib", "--bins", "--tests"];

fn nextest_available() -> bool {
    cargo().args(["nextest", "--version"]).stdout(Stdio::null()).stderr(Stdio::null()).status().is_ok_and(|s| s.success())
}

fn cmd_test() -> Result<(), String> {
    let mut c = cargo();
    let tool = if nextest_available() {
        c.args(["nextest", "run"]);
        "nextest run"
    } else {
        eprintln!("note: cargo-nextest is not installed, running cargo test (cargo install cargo-nextest --locked)");
        c.arg("test");
        "test"
    };
    c.args(TEST_TARGETS);
    run(c, &format!("cargo {tool} {}", TEST_TARGETS.join(" ")))
}

/// Runs the ignored `missing_translations_report` test with its output shown. It takes the test
/// step's targets, so it reuses that build.
fn cmd_i18n_report() -> Result<(), String> {
    const REPORT: &str = "i18n::tests::missing_translations_report";
    let mut c = cargo();
    if nextest_available() {
        c.args(["nextest", "run", "--cargo-quiet"]).args(TEST_TARGETS);
        c.args(["--run-ignored", "only", "--no-capture", "--status-level", "fail", "--final-status-level", "fail", "-E"]);
        c.arg(format!("package(designcraft-ui-egui) & test(={REPORT})"));
    } else {
        c.args(["test", "-q"]).args(TEST_TARGETS).args(["--", "--ignored", "--exact", REPORT, "--nocapture"]);
    }
    run(c, &format!("missing translations ({REPORT})"))
}

/// nextest does not run doctests.
fn cmd_doctest() -> Result<(), String> {
    let mut c = cargo();
    c.args(["test", "--workspace", "--doc"]);
    run(c, "cargo test --workspace --doc")
}

fn cmd_ci() -> Result<(), String> {
    type Step = (&'static str, Box<dyn Fn() -> Result<(), String>>);
    let steps: Vec<Step> = vec![
        (
            "fmt",
            Box::new(|| {
                let mut c = cargo();
                c.args(["fmt", "--all", "--", "--check"]);
                run(c, "cargo fmt --all -- --check")
            }),
        ),
        ("pdf-deps", Box::new(|| pdf_deps::run(&root()))),
        (
            "clippy",
            Box::new(|| {
                let mut c = cargo();
                c.args(["clippy", "--workspace", "--all-targets", "--", "-D", "warnings"]);
                run(c, "cargo clippy --workspace --all-targets -- -D warnings")
            }),
        ),
        ("test", Box::new(cmd_test)),
        ("i18n-report", Box::new(cmd_i18n_report)),
        ("doctest", Box::new(cmd_doctest)),
        ("assets", Box::new(|| assets::run(&root()))),
        ("layers", Box::new(cmd_layers)),
        ("wasm", Box::new(cmd_wasm)),
    ];
    let mut done = Vec::new();
    for (name, f) in &steps {
        eprintln!("\n=== ci: {name} ===");
        if let Err(e) = f() {
            println!("\nCI summary:");
            for d in &done {
                println!("  ok    {d}");
            }
            println!("  FAIL  {name}: {e}");
            for (n, _) in steps.iter().skip(done.len() + 1) {
                println!("  skip  {n}");
            }
            return Err(format!("ci failed at `{name}`"));
        }
        done.push(*name);
    }
    println!("\nCI summary: all {} steps passed ({})", done.len(), done.join(", "));
    Ok(())
}

const PNGSUITE_URL: &str = "http://www.schaik.com/pngsuite/PngSuite-2017jul19.tgz";
/// The real archive is 68 KB, 178 entries and 256 KB expanded; the site has no HTTPS, so what
/// arrives is bounded instead of trusted: compressed size, entries, and expanded bytes.
const PNGSUITE_MAX_ARCHIVE: u64 = 1 << 20;
const PNGSUITE_MAX_ENTRIES: usize = 1000;
const PNGSUITE_MAX_EXTRACTED: u64 = 16 << 20;

/// Bytes of the files under `dir`; `u64::MAX` when any part of it cannot be read, so the caller refuses it.
fn dir_bytes(dir: &std::path::Path) -> u64 {
    let mut total = 0u64;
    let mut stack = vec![dir.to_path_buf()];
    while let Some(d) = stack.pop() {
        let Ok(rd) = std::fs::read_dir(&d) else { return u64::MAX };
        for e in rd {
            let Ok(e) = e else { return u64::MAX };
            match e.metadata() {
                Ok(m) if m.is_dir() => stack.push(e.path()),
                Ok(m) => total = total.saturating_add(m.len()),
                Err(_) => return u64::MAX,
            }
        }
    }
    total
}

/// One `tar -tvzf` listing line: a file's size, or a directory (which takes no bytes of its own).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum TarEntry {
    File(u64),
    Dir,
}

fn is_tar_date(token: &str) -> bool {
    if token.len() == 10 {
        let b = token.as_bytes();
        return b[4] == b'-' && b[7] == b'-' && b.iter().enumerate().all(|(i, c)| i == 4 || i == 7 || c.is_ascii_digit());
    }
    matches!(token, "Jan" | "Feb" | "Mar" | "Apr" | "May" | "Jun" | "Jul" | "Aug" | "Sep" | "Oct" | "Nov" | "Dec")
}

/// Parse one `tar -tvzf` line. GNU tar prints `mode owner/group size date time name`; BSD tar
/// prints `mode owner group size date month day time name`, so the size is the field before the
/// date in both. Anything but a regular file or directory (links, devices, fifos) is refused.
fn tar_entry(line: &str) -> Option<TarEntry> {
    let fields: Vec<&str> = line.split_whitespace().collect();
    let kind = fields.first()?.chars().next()?;
    let date = fields.iter().position(|f| is_tar_date(f))?;
    let size = fields.get(date.checked_sub(1)?)?.parse::<u64>().ok()?;
    match kind {
        '-' => Some(TarEntry::File(size)),
        'd' => Some(TarEntry::Dir),
        _ => None,
    }
}

/// Stream `tar -tvzf`, bounding the listing, its entry count and the bytes it would extract before
/// any of them is written.
fn check_tar(tgz: &Path) -> Result<(), String> {
    let mut child =
        Command::new("tar").args(["-tvzf"]).arg(tgz).stdout(Stdio::piped()).spawn().map_err(|e| format!("tar -tvzf: failed to spawn: {e}"))?;
    let Some(stdout) = child.stdout.take() else {
        let _ = child.kill();
        let _ = child.wait();
        return Err("tar -tvzf: no listing output".into());
    };
    let mut entries = 0usize;
    let mut bytes = 0u64;
    let mut failure = None;
    for line in BufReader::new(stdout).lines() {
        let line = match line {
            Ok(line) => line,
            Err(e) => {
                failure = Some(format!("tar -tvzf: read failed: {e}"));
                break;
            }
        };
        entries = entries.saturating_add(1);
        if entries > PNGSUITE_MAX_ENTRIES {
            failure = Some(format!("the PngSuite download isn't a tar.gz of at most {PNGSUITE_MAX_ENTRIES} entries"));
            break;
        }
        match tar_entry(&line) {
            Some(TarEntry::File(n)) => {
                bytes = bytes.saturating_add(n);
                if bytes > PNGSUITE_MAX_EXTRACTED {
                    failure = Some(format!("the PngSuite archive expands to more than {PNGSUITE_MAX_EXTRACTED} bytes"));
                    break;
                }
            }
            Some(TarEntry::Dir) => {}
            None => {
                failure = Some("the PngSuite download holds an entry that is not a file or directory".into());
                break;
            }
        }
    }
    if failure.is_some() {
        let _ = child.kill();
    }
    let status = child.wait().map_err(|e| format!("tar -tvzf: wait failed: {e}"))?;
    if let Some(e) = failure {
        return Err(e);
    }
    if !status.success() {
        return Err(format!("tar -tvzf: exited with {status}"));
    }
    Ok(())
}

fn cmd_corpus(download: bool) -> Result<(), String> {
    let corpus = root().join("corpus");
    println!(
        "Test corpora live under {} (git-ignored, never committed).
Tests that use a corpus skip cleanly when it is absent.

  corpus/pngsuite/   PngSuite (public domain) — designcraft-codecs compares every file
                     against the `image` crate. Fetch: cargo xtask corpus --download
  corpus/psd/        PSD samples from MIT/BSD projects (ag-psd, psd-tools test data).
                     Copy files in manually; licences must be MIT/BSD/CC0.
  corpus/tiff/       libtiff pics (optional)
  corpus/exr/        OpenEXR sample images (optional)
  corpus/raw/        raw.pixls.us samples, CC0 (optional)
",
        corpus.display()
    );
    if !download {
        return Ok(());
    }
    let dest = corpus.join("pngsuite");
    std::fs::create_dir_all(&corpus).map_err(|e| format!("create {}: {e}", corpus.display()))?;
    let tgz = corpus.join("PngSuite-2017jul19.tgz");
    let mut curl = Command::new("curl");
    curl.args(["-fsSL", "--max-filesize"]).arg(PNGSUITE_MAX_ARCHIVE.to_string()).arg("-o").arg(&tgz).arg(PNGSUITE_URL);
    run(curl, &format!("curl {PNGSUITE_URL}"))?;
    let refuse = |why: String| {
        let _ = std::fs::remove_file(&tgz);
        Err(why)
    };
    if std::fs::metadata(&tgz).map_or(true, |m| m.len() > PNGSUITE_MAX_ARCHIVE) {
        return refuse(format!("the PngSuite download is larger than {PNGSUITE_MAX_ARCHIVE} bytes"));
    }
    if let Err(e) = check_tar(&tgz) {
        return refuse(e);
    }
    // Extract into a staging directory and only publish it once every check has passed, so a
    // failed download or a hostile archive never damages an existing corpus.
    let staging = corpus.join(format!("pngsuite.staging-{}", std::process::id()));
    if staging.exists() {
        std::fs::remove_dir_all(&staging).map_err(|e| format!("remove stale {}: {e}", staging.display()))?;
    }
    std::fs::create_dir_all(&staging).map_err(|e| format!("create {}: {e}", staging.display()))?;
    let discard_staging = || {
        let _ = std::fs::remove_dir_all(&staging);
    };
    let mut tar = Command::new("tar");
    tar.arg("-xzf").arg(&tgz).arg("-C").arg(&staging);
    if let Err(e) = run(tar, "tar -xzf PngSuite-2017jul19.tgz") {
        discard_staging();
        return refuse(e);
    }
    let _ = std::fs::remove_file(&tgz);
    if dir_bytes(&staging) > PNGSUITE_MAX_EXTRACTED {
        discard_staging();
        return Err(format!("the PngSuite archive expanded to more than {PNGSUITE_MAX_EXTRACTED} bytes; removed {}", staging.display()));
    }
    let n = std::fs::read_dir(&staging).map(|d| d.flatten().filter(|e| e.path().extension().is_some_and(|x| x == "png")).count()).unwrap_or(0);
    if n == 0 {
        discard_staging();
        return Err("no PNG files extracted".into());
    }
    let backup = corpus.join(format!("pngsuite.old-{}", std::process::id()));
    if backup.exists() {
        std::fs::remove_dir_all(&backup).map_err(|e| format!("remove stale {}: {e}", backup.display()))?;
    }
    if dest.exists() {
        std::fs::rename(&dest, &backup).map_err(|e| format!("move {} aside: {e}", dest.display()))?;
    }
    if let Err(e) = std::fs::rename(&staging, &dest) {
        if backup.exists() {
            let _ = std::fs::rename(&backup, &dest);
        }
        discard_staging();
        return Err(format!("publish {}: {e}", dest.display()));
    }
    let _ = std::fs::remove_dir_all(&backup);
    println!("PngSuite: {n} PNG files in {}", dest.display());
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::{TarEntry, tar_entry};

    #[test]
    fn parses_gnu_and_bsd_verbose_tar_listings() {
        assert_eq!(tar_entry("-rw-r--r-- francesco/francesco 5 2026-11-06 23:28 a.file"), Some(TarEntry::File(5)));
        assert_eq!(tar_entry("-rw-r--r--  0 user group  1234 Nov  6 23:28 dir/b file.png"), Some(TarEntry::File(1234)));
        assert_eq!(tar_entry("drwxr-xr-x 1000/1000 0 2026-11-06 23:28 dir/"), Some(TarEntry::Dir));
        assert_eq!(tar_entry("lrwxrwxrwx 1000/1000 0 2026-11-06 23:28 link -> a.file"), None);
        assert_eq!(tar_entry("not a tar listing"), None);
    }
}
