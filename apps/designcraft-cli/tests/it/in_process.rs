//! `designcraft_cli::run` called in-process: the same program the binary is, with its streams captured, so the whole
//! of RFC-0018's surface (exit codes, which stream, `error:` lines) is covered without a subprocess per case.

use std::path::{Path, PathBuf};

/// Run the CLI; gives back the exit code, stdout and stderr.
fn run(args: &[&str]) -> (i32, String, String) {
    let (mut out, mut err) = (Vec::new(), Vec::new());
    let code = designcraft_cli::run(args.iter().copied(), &mut out, &mut err);
    (code, String::from_utf8(out).unwrap(), String::from_utf8(err).unwrap())
}

/// A directory of this test's own, removed on drop.
struct Dir(PathBuf);

impl Dir {
    fn new(name: &str) -> Dir {
        let dir = std::env::temp_dir().join(format!("dc-inproc-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        Dir(dir)
    }
    fn file(&self, name: &str) -> String {
        self.0.join(name).to_str().unwrap().to_owned()
    }
}

impl Drop for Dir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn closed_port() -> String {
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    drop(listener);
    port.to_string()
}

#[test]
fn help_and_version_go_to_stdout_with_exit_0() {
    for args in [&["--help"][..], &["-h"], &["help"], &["run", "--help"], &["--version"], &["-V"], &["script", "--version"]] {
        let (code, out, err) = run(args);
        assert_eq!((code, err.as_str()), (0, ""), "{args:?}");
        assert!(!out.is_empty(), "{args:?}");
    }
}

#[test]
fn usage_errors_exit_2_on_stderr_only() {
    for args in [&["bogus"][..], &["run", "--nope"], &["commands", "--json"], &["perf", "--pages", "x"], &["validate-pdf"], &["version"]] {
        let (code, out, err) = run(args);
        assert_eq!(code, 2, "{args:?}: {err}");
        assert_eq!(out, "", "{args:?}");
        assert!(err.starts_with("error:"), "{args:?}: {err}");
    }
}

/// No arguments asks for help the way `git` does, but as a usage failure: the usage is on stderr, exit 2.
#[test]
fn no_arguments_is_a_usage_failure_with_the_usage_on_stderr() {
    let (code, out, err) = run(&[]);
    assert_eq!((code, out.as_str()), (2, ""));
    assert!(err.contains("Usage: designcraft-cli"), "{err}");
}

#[test]
fn the_launcher_profile_arguments_are_refused_before_anything_parses_them() {
    for args in [&["--launch-profile", "nightly-v1", "run"][..], &["run", "--profile-root=/tmp/x"]] {
        let (code, out, err) = run(args);
        assert_eq!((code, out.as_str()), (2, ""), "{args:?}");
        assert!(err.contains("nightly-v1"), "{err}");
    }
    // A file name after `--` is a file, not a flag.
    let (code, _, err) = run(&["script", "--", "--launch-profile"]);
    assert_eq!(code, 1, "{err}");
    assert!(err.contains("--launch-profile"), "{err}");
}

#[test]
fn commands_describe_and_links() {
    let (code, out, _) = run(&["commands"]);
    assert_eq!(code, 0);
    let all: serde_json::Value = serde_json::from_str(&out).unwrap();
    let (code, out, _) = run(&["commands", "footnote"]);
    assert_eq!(code, 0);
    let some: serde_json::Value = serde_json::from_str(&out).unwrap();
    assert!(!some.as_array().unwrap().is_empty() && some.as_array().unwrap().len() < all.as_array().unwrap().len());

    let (code, out, _) = run(&["describe", "document.inspect"]);
    assert_eq!(code, 0, "{out}");
    assert!(out.contains("document.inspect"), "{out}");
    let (code, out, err) = run(&["describe", "no.such.command"]);
    assert_eq!((code, out.as_str()), (1, ""));
    assert!(err.starts_with("error:"), "{err}");

    let (code, out, err) = run(&["links"]);
    assert_eq!((code, err.as_str()), (0, ""));
    assert!(out.contains("discord"), "{out}");
}

#[test]
fn run_exports_the_sample_and_reports_on_stderr() {
    let dir = Dir::new("run");
    let (png, pdf, all) = (dir.file("a.png"), dir.file("a.pdf"), dir.file("pages"));
    let (code, out, err) = run(&["run", "--sample", "--export", &png, "--export", &pdf, "--cmd", "document.inspect", "--all-pages", &all]);
    assert_eq!(code, 0, "{err}");
    assert!(err.contains("a.png"), "{err}");
    assert!(Path::new(&png).is_file() && Path::new(&pdf).is_file());
    assert!(out.contains("document.inspect") || out.contains("pages") || !out.is_empty(), "{out}");

    let (code, out, err) = run(&["run", "--sample", "--page", "99", "--export", &png]);
    assert_eq!((code, out.as_str()), (1, ""), "{err}");
    assert!(err.starts_with("error:"), "{err}");

    let (code, _, err) = run(&["run", "--in", &dir.file("missing.designcraft"), "--export", &png]);
    assert_eq!(code, 1, "{err}");
}

#[test]
fn script_runs_steps_and_saves_and_exports() {
    let dir = Dir::new("script");
    let script = dir.file("s.dcs");
    std::fs::write(&script, "document.inspect\n").unwrap();
    let (doc, png) = (dir.file("s.designcraft"), dir.file("s.png"));
    let (code, out, err) = run(&["script", &script, "--sample", "--save", &doc, "--export", &png]);
    assert_eq!(code, 0, "{err}");
    let report: serde_json::Value = serde_json::from_str(&out).unwrap();
    assert_eq!(report["completed"], 1);
    assert!(Path::new(&doc).is_file() && Path::new(&png).is_file());

    std::fs::write(&script, "document.inspect\nno.such.command\n").unwrap();
    let (code, out, _) = run(&["script", &script]);
    assert_eq!(code, 1);
    assert!(out.contains("failedIndex"), "{out}");
    let (code, _, err) = run(&["script", &dir.file("missing.dcs")]);
    assert_eq!(code, 1, "{err}");
}

#[test]
fn connecting_to_a_closed_port_is_a_command_failure_not_a_usage_error() {
    let port = closed_port();
    for args in [&["script", "--connect", &port][..], &["mcp", "--connect", &port], &["app", "--connect", &port, "document.inspect"]] {
        let (code, out, err) = run(args);
        assert_eq!((code, out.as_str()), (1, ""), "{args:?}: {err}");
        assert!(err.starts_with("error:") && err.contains(&port), "{args:?}: {err}");
    }
}

#[test]
fn perf_and_bench_report_budgets() {
    let dir = Dir::new("perf");
    let dump = dir.file("dump");
    let (code, out, _) = run(&["perf", "--pages", "1", "--frames", "1", "--chars", "200", "--images", "0", "--runs", "1", "--dump", &dump]);
    assert_eq!(code, 0, "{out}");
    assert!(out.contains("DesignCraft performance budgets"), "{out}");
    assert!(Path::new(&dump).is_dir());

    let doc = dir.file("b.designcraft");
    let script = dir.file("s.dcs");
    std::fs::write(&script, "document.inspect\n").unwrap();
    let (code, _, err) = run(&["script", &script, "--sample", "--save", &doc]);
    assert_eq!(code, 0, "{err}");
    let (code, out, err) = run(&["bench", &doc, "--runs", "1"]);
    assert_eq!(code, 0, "{err}");
    assert!(out.contains("pages"), "{out}");
    let (code, _, err) = run(&["bench", &dir.file("missing.designcraft")]);
    assert_eq!(code, 1, "{err}");
}

#[test]
fn validate_pdf_reports_json_and_refuses_what_is_not_a_pdf() {
    let dir = Dir::new("pdf");
    let pdf = dir.file("a.pdf");
    let (code, _, err) = run(&["run", "--sample", "--export", &pdf]);
    assert_eq!(code, 0, "{err}");
    let (code, out, _) = run(&["validate-pdf", &pdf]);
    assert!(code == 0 || code == 1, "{code}");
    let report: serde_json::Value = serde_json::from_str(&out).unwrap();
    assert!(report.get("valid").is_some(), "{out}");
    let (code, _, err) = run(&["validate-pdf", &pdf, "--standard", "x1a"]);
    assert!(code == 0 || code == 1, "{err}");
    let (code, _, err) = run(&["validate-pdf", &dir.file("missing.pdf")]);
    assert_eq!(code, 1, "{err}");
}

/// A closed stdout (`designcraft-cli commands | head`) ends the program quietly.
#[test]
fn a_closed_pipe_ends_quietly() {
    struct Closed;
    impl std::io::Write for Closed {
        fn write(&mut self, _: &[u8]) -> std::io::Result<usize> {
            Err(std::io::ErrorKind::BrokenPipe.into())
        }
        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }
    let mut err = Vec::new();
    assert_eq!(designcraft_cli::run(["commands"], &mut Closed, &mut err), 0);
    assert!(err.is_empty());
    struct Broken;
    impl std::io::Write for Broken {
        fn write(&mut self, _: &[u8]) -> std::io::Result<usize> {
            Err(std::io::Error::other("disk full"))
        }
        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }
    assert_eq!(designcraft_cli::run(["commands"], &mut Broken, &mut err), 1);
    assert!(String::from_utf8(err).unwrap().starts_with("error:"));
}
