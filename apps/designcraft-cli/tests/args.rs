//! What the command line does with its arguments, pinned.
//!
//! Every assertion here was measured, not intended: these tests say what the parser does today,
//! including where that is odd (an unknown option taken as a filter, a `--opt=value` form that
//! only the automation roots have). They exist so that moving to a derived parser — the work
//! photocraft#7071 lists, which this parser needs too — changes the surface where that is the
//! point and nowhere else.
//!
//! They assert the exit code, which stream the output went to, and that an error names the
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

    /// Failed with the usage exit code, said nothing on stdout; gives back the complaint.
    fn failed(self, what: &str) -> String {
        assert_eq!(self.code, Some(1), "{what}: expected a usage failure, got {:?} / {:?}", self.out, self.err);
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
fn with_no_arguments_it_prints_the_usage_block_and_fails() {
    let err = run(&[]).failed("no arguments");
    assert!(err.starts_with("usage: designcraft-cli run"), "{err:?}");
    // The usage block names every subcommand, which is how `<sub> --help` finds its lines.
    for sub in SUBCOMMANDS {
        assert!(err.contains(&format!("designcraft-cli {sub}")), "usage names {sub}: {err:?}");
    }
    // An empty argument is a command nobody has, not a request for help.
    run(&[""]).failed("an empty argument");
}

#[test]
fn help_goes_to_stdout_and_succeeds_in_all_three_spellings() {
    let long = run(&["--help"]).succeeded("--help");
    assert!(long.starts_with("usage: designcraft-cli run"), "{long:?}");
    // The community links are under the usage block, for someone who has nowhere else to ask.
    assert!(long.contains("Community:") && long.contains("discord"), "{long:?}");
    for spelling in ["-h", "help"] {
        assert_eq!(run(&[spelling]).succeeded(spelling), long, "{spelling} prints the same help");
    }
}

#[test]
fn an_unknown_command_fails_with_the_same_text_on_stderr() {
    let help = run(&["--help"]).succeeded("--help");
    for unknown in ["bogus", "-x", "--nope", "Run"] {
        let err = run(&[unknown]).failed(unknown);
        assert_eq!(err, help, "{unknown}: the same usage, on stderr");
    }
}

#[test]
fn the_version_is_the_crate_version() {
    let expected = format!("designcraft-cli {}\n", env!("CARGO_PKG_VERSION"));
    for spelling in ["--version", "-V", "version"] {
        assert_eq!(run(&[spelling]).succeeded(spelling), expected, "{spelling}");
    }
}

/// `<sub> --help` asks about that subcommand. It answers from the usage block, on stdout, and
/// nothing else happens: no export is written, no port is dialled, no measurement is taken, and a
/// file named earlier in the same line is not opened or complained about (photocraft#423).
#[test]
fn every_subcommand_answers_help_without_running_anything() {
    let dir = temp_dir("help");
    let png = dir.join("ran.png");
    let dead = closed_port();
    let cases: [(&str, Vec<&str>); 10] = [
        ("run", vec!["run", "--sample", "--export", png.to_str().unwrap()]),
        ("commands", vec!["commands"]),
        ("describe", vec!["describe", "file.open"]),
        ("script", vec!["script", "no-such-script.dcs"]),
        ("app", vec!["app", "--port", &dead, "document.list"]),
        ("mcp", vec!["mcp", "--connect", &dead]),
        ("perf", vec!["perf", "--pages", "1", "--runs", "1"]),
        ("bench", vec!["bench", "no-such-document.designcraft"]),
        ("validate-pdf", vec!["validate-pdf", "no-such-file.pdf"]),
        ("links", vec!["links"]),
    ];
    for (sub, args) in cases {
        for flag in ["--help", "-h"] {
            let mut args = args.clone();
            args.push(flag);
            let what = format!("{sub} {flag}");
            let out = run(&args).succeeded(&what);
            assert!(out.contains(&format!("designcraft-cli {sub}")), "{what} names it: {out:?}");
            assert!(!out.contains("unknown option"), "{what}: {out:?}");
            // None of the work the rest of the line asks for was done.
            assert!(!png.exists(), "{what} exported a file");
            assert!(!out.contains("cannot connect") && !out.contains("render threads"), "{what}: {out:?}");
            assert!(!out.contains("No such file"), "{what}: {out:?}");
        }
    }
    // `script`'s own help is longer than one usage line: it documents the script syntax, which is
    // otherwise only in `describe script.run` and the docs.
    let out = run(&["script", "--help"]).succeeded("script --help");
    assert!(out.contains("Script text, one step per line"), "{out:?}");
    let _ = std::fs::remove_dir_all(&dir);
}

/// The subcommands that have options refuse an option they do not know, and name it.
#[test]
fn an_unknown_option_is_refused_by_name() {
    let cases: [Vec<&str>; 5] = [
        vec!["run", "--nope"],
        vec!["script", "-", "--nope"],
        vec!["mcp", "--nope"],
        vec!["perf", "--nope"],
        vec!["bench", "f.designcraft", "--nope"],
    ];
    for args in cases {
        let err = run(&args).failed(&format!("{args:?}"));
        assert!(err.contains("--nope"), "{args:?}: {err:?}");
        assert!(err.contains("unknown"), "{args:?}: {err:?}");
    }
}

/// The subcommands whose argument is a word, not an option, take an unknown option as that word:
/// `commands --nope` filters the command list by `--nope` and finds nothing. Each of these would
/// be an error under the rule photocraft#7071 settles on; today they are not, and a change of
/// mind shows up here.
#[test]
fn the_subcommands_that_take_a_word_read_an_unknown_option_as_that_word() {
    // A filter that matches no command: an empty list, successfully.
    assert_eq!(run(&["commands", "--nope"]).succeeded("commands --nope").trim(), "[]");
    // A command id nobody has.
    let err = run(&["describe", "--nope"]).failed("describe --nope");
    assert!(err.contains("no command `--nope`"), "{err:?}");
    // A path nobody has.
    let err = run(&["validate-pdf", "--nope"]).failed("validate-pdf --nope");
    assert!(err.contains("--nope") && err.contains("No such file"), "{err:?}");
    // A method name for the running app, so the error is about reaching the app.
    let dead = closed_port();
    let err = run(&["app", "--port", &dead, "--nope"]).failed("app --nope");
    assert!(err.contains("cannot connect"), "{err:?}");
    // And `links`, which takes nothing, ignores what it is given.
    let out = run(&["links", "--nope"]).succeeded("links --nope");
    assert_eq!(out, run(&["links"]).succeeded("links"));
}

/// There is no `--opt=value` form. The two automation roots are the exception: they have one,
/// because a folder whose name starts with `-` has no other way in there.
#[test]
fn only_the_automation_roots_have_an_opt_equals_value_form() {
    let cases: [(&str, Vec<&str>); 3] = [
        ("--scale=2", vec!["run", "--scale=2", "--sample"]),
        ("--pages=2", vec!["perf", "--pages=2"]),
        ("--save=out.designcraft", vec!["script", "-", "--save=out.designcraft"]),
    ];
    for (token, args) in cases {
        let err = run(&args).failed(&format!("{args:?}"));
        assert!(err.contains("unknown"), "{args:?}: {err:?}");
        // The whole token is named, `=value` and all, so it is clear what was not understood.
        assert!(err.contains(token), "{args:?}: {err:?}");
    }
    // The roots read the value after `=`: an empty one is refused as a folder, which is the
    // parser having taken it rather than ignoring the form.
    let err = run(&["mcp", "--automation-read-root="]).failed("empty root");
    assert!(err.contains("--automation-read-root") && err.contains("folder"), "{err:?}");
    // Unlike every other option, a root given twice is an error rather than the last one winning,
    // in either spelling.
    for second in ["--automation-read-root=b", "--automation-read-root"] {
        let err = run(&["mcp", "--automation-read-root=a", second, "b"]).failed(second);
        assert!(err.contains("twice"), "{second}: {err:?}");
    }
}

/// An option that needs a value and has none says which option, and nothing runs.
#[test]
fn a_missing_value_names_its_option() {
    let dir = temp_dir("missing");
    let png = dir.join("out.png");
    let cases: [(&str, Vec<&str>); 9] = [
        ("--export", vec!["run", "--sample", "--export"]),
        ("--in", vec!["run", "--in"]),
        ("--all-pages", vec!["run", "--sample", "--all-pages"]),
        // Before an export, too: the order of the options is nothing to judge until this one has
        // a value, so it is not reported as an export it is not in front of.
        ("--scale", vec!["run", "--sample", "--scale", "--export", png.to_str().unwrap()]),
        ("--page", vec!["run", "--sample", "--page"]),
        ("--save", vec!["script", "-", "--save"]),
        ("--connect", vec!["script", "-", "--connect"]),
        ("--port", vec!["app", "--port"]),
        ("--pages", vec!["perf", "--pages"]),
    ];
    for (option, args) in cases {
        let err = run(&args).failed(&format!("{args:?}"));
        assert!(err.contains(option), "{args:?} names {option}: {err:?}");
        assert!(err.contains("needs"), "{args:?}: {err:?}");
    }
    assert!(!png.exists(), "nothing is exported");
    let _ = std::fs::remove_dir_all(&dir);
}

/// An option that takes no value does not swallow the token after it: that token is read as the
/// next argument, and refused if it is not one.
#[test]
fn a_flag_that_takes_no_value_leaves_the_next_token_alone() {
    let err = run(&["run", "--sample", "extra", "--export", "out.png"]).failed("run --sample extra");
    assert!(err.contains("extra"), "{err:?}");
    let err = run(&["perf", "--strict", "3"]).failed("perf --strict 3");
    assert!(err.contains('3'), "{err:?}");
}

/// A value flag given twice keeps the last value. (The automation roots are the exception, above.)
#[test]
fn a_repeated_value_flag_keeps_the_last_value() {
    let dir = temp_dir("repeat");
    let at = |args: &[&str], name: &str| -> Vec<u8> {
        let path = dir.join(name);
        let mut full: Vec<&str> = vec!["run", "--sample"];
        full.extend_from_slice(args);
        let out = path.to_str().unwrap();
        full.extend(["--export", out]);
        let ran = run(&full);
        assert_eq!(ran.code, Some(0), "{full:?}: {:?}", ran.err);
        std::fs::read(&path).unwrap()
    };
    // `--scale`: the size of the image says which value was used.
    let one = at(&[], "1.png");
    let three = at(&["--scale", "3"], "3.png");
    assert_eq!(at(&["--scale", "2", "--scale", "3"], "23.png"), three, "the last --scale won");
    assert_ne!(three, one, "and --scale 3 is not --scale 1");
    // `--page`: which page was exported says it.
    let second = at(&["--page", "1"], "p1.png");
    assert_ne!(second, one, "page 1 of the sample is not page 0");
    assert_eq!(at(&["--page", "1", "--page", "0"], "p10.png"), one, "the last --page won");
    // `--connect` and `--port`: the address the error names says it.
    let (a, b) = (closed_port(), closed_port());
    for args in [vec!["mcp", "--connect", &a, "--connect", &b], vec!["app", "--port", &a, "--port", &b, "document.list"]] {
        let err = run(&args).failed(&format!("{args:?}"));
        assert!(err.contains(&format!("127.0.0.1:{b}")), "{args:?} dialled the last port: {err:?}");
    }
    let _ = std::fs::remove_dir_all(&dir);
}

/// The flags that are meant to repeat each take effect, in order.
#[test]
fn the_repeatable_flags_all_take_effect() {
    let dir = temp_dir("each");
    let (a, b) = (dir.join("a.png"), dir.join("b.png"));
    let ran = run(&[
        "run",
        "--sample",
        "--cmd",
        "document.list",
        "--cmd",
        "document.list",
        "--export",
        a.to_str().unwrap(),
        "--export",
        b.to_str().unwrap(),
    ]);
    assert_eq!(ran.code, Some(0), "{:?}", ran.err);
    // One result line per `--cmd`, in the order given.
    let results: Vec<&str> = ran.out.lines().collect();
    assert_eq!(results.len(), 2, "{results:?}");
    assert_eq!(results[0], results[1]);
    // And one file per `--export`.
    assert_eq!(std::fs::read(&a).unwrap(), std::fs::read(&b).unwrap());
    assert_eq!(ran.err.lines().filter(|l| l.starts_with("wrote ")).count(), 2, "{:?}", ran.err);
    let _ = std::fs::remove_dir_all(&dir);
}

/// A subcommand given nothing to do does nothing, quietly or with its own usage line. `run` with
/// no arguments is the one that says nothing at all: there is no document to act on and nothing
/// was asked of it.
#[test]
fn a_subcommand_with_nothing_to_do_says_so_or_says_nothing() {
    assert_eq!(run(&["run"]).succeeded("run"), "");
    // `script` with no file reads stdin, which is closed here: an empty script, run.
    let out = run(&["script"]).succeeded("script");
    assert!(out.contains("\"completed\": 0"), "{out:?}");
    // The two that need an argument print the usage line for themselves.
    for sub in ["describe", "bench", "validate-pdf"] {
        let err = run(&[sub]).failed(sub);
        assert!(err.contains(&format!("designcraft-cli {sub}")), "{sub}: {err:?}");
    }
}
