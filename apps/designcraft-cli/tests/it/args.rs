//! What the command line does with its arguments: RFC-0018's contract, through the binary.
//!
//! The arguments are clap-derived (`src/args.rs`), so a usage error exits 2, help and the version go to stdout with
//! exit 0, and an unknown option after a subcommand is refused rather than ignored. These tests assert the exit code, which stream the output went to, and that an error names the
//! argument it is about. They do not assert wording, so a message can be improved without
//! editing a test, except in the few places where the wording *is* the behaviour and the test
//! says so.
//!
//! `tests/cli.rs` holds what the subcommands then do: exports, scripts, the MCP server.

use std::process::{Command, Stdio};

/// `designcraft-cli` with stdin closed, so a subcommand that reads a script does not wait.
fn cli() -> Command {
    let mut c = Command::new(env!("CARGO_BIN_EXE_designcraft-cli"));
    c.stdin(Stdio::null());
    c
}

/// What one invocation did.
struct Ran {
    code: Option<i32>,
    out: String,
    err: String,
}

fn run(args: &[&str]) -> Ran {
    let out = cli().args(args).output().unwrap_or_else(|e| panic!("{args:?}: {e}"));
    Ran { code: out.status.code(), out: String::from_utf8_lossy(&out.stdout).into_owned(), err: String::from_utf8_lossy(&out.stderr).into_owned() }
}

impl Ran {
    /// Succeeded, said nothing on stderr; gives back what it printed.
    fn succeeded(self, what: &str) -> String {
        assert_eq!(self.code, Some(0), "{what}: {:?}", self.err);
        assert_eq!(self.err, "", "{what} wrote to stderr");
        self.out
    }

    /// Failed with the usage exit code (2), said nothing on stdout; gives back the complaint.
    fn failed(self, what: &str) -> String {
        assert_eq!(self.code, Some(2), "{what}: expected a usage failure, got {:?} / {:?}", self.out, self.err);
        assert_eq!(self.out, "", "{what} wrote to stdout");
        self.err
    }
}

/// A port with nothing listening on it, so `--connect` and `--port` fail the same way every time.
fn closed_port() -> String {
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    drop(listener);
    port.to_string()
}

/// A directory of this test's own, removed when the test ends.
fn temp_dir(name: &str) -> std::path::PathBuf {
    let dir = std::env::temp_dir().join(format!("dc-args-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

/// Every subcommand, in the order the usage block lists them.
const SUBCOMMANDS: [&str; 10] = ["run", "commands", "describe", "script", "app", "mcp", "perf", "bench", "validate-pdf", "links"];

#[test]
fn with_no_arguments_it_prints_the_usage_and_fails_with_2() {
    let err = run(&[]).failed("no arguments");
    assert!(err.contains("Usage: designcraft-cli"), "{err:?}");
    for sub in SUBCOMMANDS {
        assert!(err.contains(sub), "usage names {sub}: {err:?}");
    }
    // An empty argument is a command nobody has, not a request for help.
    run(&[""]).failed("an empty argument");
}

#[test]
fn help_goes_to_stdout_and_succeeds_in_all_three_spellings() {
    let long = run(&["--help"]).succeeded("--help");
    assert!(long.starts_with("Headless DesignCraft"), "{long:?}");
    assert!(long.contains("Community:") && long.contains("discord"), "{long:?}");
    assert!(long.contains("Exit status:"), "{long:?}");
    let short = run(&["-h"]).succeeded("-h");
    assert!(short.contains("Usage: designcraft-cli"), "{short:?}");
    let word = run(&["help"]).succeeded("help");
    assert!(word.contains("Usage: designcraft-cli"), "{word:?}");
    run(&["help", "run"]).succeeded("help run");
}

#[test]
fn an_unknown_command_fails_with_2_and_says_so() {
    for unknown in ["bogus", "-x", "--nope", "Run", "version"] {
        let err = run(&[unknown]).failed(unknown);
        assert!(err.contains("error:") && err.contains("Usage: designcraft-cli"), "{unknown}: {err:?}");
        // Asking for help about a command nobody has is still that command.
        for spelling in ["--help", "-h"] {
            let err = run(&[unknown, spelling]).failed(&format!("{unknown} {spelling}"));
            assert!(err.contains("error:"), "{unknown} {spelling}: {err:?}");
        }
    }
}

#[test]
fn the_version_is_the_crate_version_and_there_is_no_version_subcommand() {
    for flag in ["--version", "-V"] {
        let out = run(&[flag]).succeeded(flag);
        assert_eq!(out.trim(), format!("designcraft-cli {}", env!("CARGO_PKG_VERSION")), "{flag}");
    }
    // `--version` reaches the subcommands (propagate_version), under the subcommand's own name.
    let sub = run(&["run", "--version"]).succeeded("run --version");
    assert!(sub.starts_with("designcraft-cli-run "), "{sub:?}");
}

#[test]
fn every_subcommand_answers_help_without_running_anything() {
    for sub in SUBCOMMANDS {
        for flag in ["--help", "-h"] {
            let out = run(&[sub, flag]).succeeded(&format!("{sub} {flag}"));
            assert!(out.contains(&format!("Usage: designcraft-cli {sub}")), "{sub} {flag}: {out:?}");
        }
    }
}

#[test]
fn an_unknown_option_is_refused_by_name_in_every_subcommand() {
    for sub in ["run", "commands", "describe", "script", "app", "mcp", "perf", "bench", "validate-pdf", "links"] {
        let err = run(&[sub, "--no-such-option"]).failed(sub);
        assert!(err.contains("--no-such-option"), "{sub}: {err:?}");
    }
    // The subcommands that take a word no longer read an unknown option as that word, and a value goes after it.
    let err = run(&["commands", "--json"]).failed("commands --json");
    assert!(err.contains("--json"), "{err:?}");
    run(&["describe", "--nope"]).failed("describe --nope");
    // `--` ends the options, so a filter can start with a dash.
    run(&["commands", "--", "--json"]).succeeded("commands -- --json");
}

#[test]
fn a_flag_belongs_only_to_its_own_subcommand() {
    // `--sample` is a document source of `run`, `script` and `mcp`, and means nothing to `commands`.
    run(&["commands", "--sample"]).failed("commands --sample");
    run(&["perf", "--cmd", "x"]).failed("perf --cmd");
    run(&["validate-pdf", "x.pdf", "--scale", "2"]).failed("validate-pdf --scale");
}

#[test]
fn a_missing_value_names_its_option() {
    for (args, option) in [
        (&["run", "--in"][..], "--in"),
        (&["run", "--export"], "--export"),
        (&["run", "--page"], "--page"),
        (&["script", "--connect"], "--connect"),
        (&["perf", "--pages"], "--pages"),
        (&["bench"], "FILE"),
        (&["validate-pdf"], "FILE"),
        (&["mcp", "--automation-read-root"], "--automation-read-root"),
    ] {
        let err = run(args).failed(&format!("{args:?}"));
        assert!(err.contains(option), "{args:?}: {err:?}");
    }
}

#[test]
fn a_value_that_cannot_mean_anything_is_a_usage_error() {
    let err = run(&["run", "--sample", "--page", "zero"]).failed("--page zero");
    assert!(err.contains("--page"), "{err:?}");
    run(&["perf", "--pages", "many"]).failed("--pages many");
    run(&["validate-pdf", "x.pdf", "--standard", "x9"]).failed("--standard x9");
    run(&["run", "--sample", "--pdf-options", "{"]).failed("--pdf-options {");
}

#[test]
fn the_equals_form_works_for_every_option() {
    let port = closed_port();
    // The arguments parse (the connection is what fails, with exit 1, not 2).
    let out = run(&["script", &format!("--connect={port}")]);
    assert_eq!(out.code, Some(1), "{:?}", out.err);
    assert!(out.err.starts_with("error:") && out.err.contains(&port), "{:?}", out.err);
    let out = run(&["mcp", &format!("--connect={port}")]);
    assert_eq!(out.code, Some(1), "{:?}", out.err);
    assert!(out.err.starts_with("error:"), "{:?}", out.err);
}

#[test]
fn a_repeated_single_value_flag_is_refused_where_the_last_one_would_silently_win() {
    for args in
        [&["script", "--connect", "1", "--connect", "2"][..], &["perf", "--pages", "1", "--pages", "2"], &["mcp", "--connect", "1", "--connect", "2"]]
    {
        let err = run(args).failed(&format!("{args:?}"));
        assert!(err.contains("cannot be used multiple times") || err.contains("--"), "{args:?}: {err:?}");
    }
}

#[test]
fn the_repeatable_flags_all_take_effect_in_the_order_given() {
    let dir = temp_dir("repeat");
    let (a, b) = (dir.join("a.png"), dir.join("b.png"));
    let out = run(&["run", "--sample", "--export", a.to_str().unwrap(), "--scale", "2", "--export", b.to_str().unwrap()]);
    assert_eq!(out.code, Some(0), "{:?}", out.err);
    assert!(out.err.contains("a.png") && out.err.contains("b.png"), "{:?}", out.err);
    // The second export was made at twice the scale of the first.
    let width = |p: &std::path::Path| image::image_dimensions(p).unwrap().0;
    assert_eq!(width(&b), width(&a) * 2);
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn an_option_that_applies_to_nothing_says_so() {
    // Say what is wrong, and not on stdout.
    let err = run(&["run", "--sample", "--scale", "2"]).failed("--scale with no export after it");
    assert!(err.contains("--scale"), "{err:?}");
}
