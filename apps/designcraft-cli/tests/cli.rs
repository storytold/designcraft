//! The agent-facing command line: scripts with references, describe, filtered command lists.

use std::io::Write;
use std::process::{Command, Stdio};

fn cli() -> Command {
    Command::new(env!("CARGO_BIN_EXE_designcraft-cli"))
}

#[test]
fn script_from_stdin_with_references_saves_and_exports() {
    let dir = std::env::temp_dir().join(format!("dc-cli-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let (doc, png) = (dir.join("a.designcraft"), dir.join("a.png"));
    let mut child = cli()
        .args(["script", "-", "--save", doc.to_str().unwrap(), "--export", png.to_str().unwrap()])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .unwrap();
    child
        .stdin
        .take()
        .unwrap()
        .write_all(
            b"# comment\nframe.create {\"rect\": [72, 72, 300, 200], \"content\": \"text\", \"text\": \"Hi\"}\nstory.get {\"story\": \"$0.story\"}\n",
        )
        .unwrap();
    let out = child.wait_with_output().unwrap();
    assert!(out.status.success(), "{}", String::from_utf8_lossy(&out.stderr));
    let v: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
    assert_eq!(v["completed"], 2);
    assert_eq!(v["results"][1]["text"], "Hi");
    assert!(doc.exists() && png.exists());
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn failing_script_exits_nonzero_with_the_step() {
    let mut child = cli().args(["script", "-"]).stdin(Stdio::piped()).stdout(Stdio::piped()).stderr(Stdio::piped()).spawn().unwrap();
    child.stdin.take().unwrap().write_all(b"document.inspect\nno.such.command\n").unwrap();
    let out = child.wait_with_output().unwrap();
    assert!(!out.status.success());
    let v: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
    assert_eq!(v["failedIndex"], 1);
}

#[test]
fn run_cmd_references_and_describe() {
    let out = cli()
        .args([
            "run",
            "--cmd",
            r#"frame.create={"rect":[72,72,300,200],"content":"text","text":"Hello"}"#,
            "--cmd",
            r#"story.get={"story":"$0.story"}"#,
        ])
        .output()
        .unwrap();
    assert!(out.status.success(), "{}", String::from_utf8_lossy(&out.stderr));
    assert!(String::from_utf8_lossy(&out.stdout).contains("\"text\":\"Hello\""));
    let out = cli().args(["describe", "footnote.insert"]).output().unwrap();
    let v: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
    assert_eq!(v["label"], "Insert Footnote");
    let out = cli().args(["commands", "footnote."]).output().unwrap();
    let v: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
    assert!(v.as_array().unwrap().iter().all(|c| c.to_string().contains("footnote")));
}

/// `designcraft-cli --help` printed usage to stderr and exited 1; asking for help is not an
/// error (#243). It now goes to stdout and succeeds, while an unknown command keeps the same
/// usage on stderr and fails.
#[test]
fn help_goes_to_stdout_and_unknown_args_to_stderr() {
    for flag in ["--help", "-h", "help"] {
        let out = cli().arg(flag).output().unwrap();
        let stdout = String::from_utf8_lossy(&out.stdout);
        assert!(out.status.success(), "{flag}: {:?}", out.status);
        assert!(stdout.starts_with("usage: designcraft-cli"), "{flag}: {stdout:?}");
        assert!(stdout.contains("designcraft-cli --version"), "{flag}: {stdout:?}");
        assert!(out.stderr.is_empty(), "{flag} wrote to stderr: {}", String::from_utf8_lossy(&out.stderr));
    }
    // An unknown option is still an error: usage on stderr, nothing on stdout, non-zero exit.
    let out = cli().arg("--no-such-option").output().unwrap();
    assert!(!out.status.success(), "{:?}", out.status);
    assert!(out.stdout.is_empty(), "unexpected stdout: {}", String::from_utf8_lossy(&out.stdout));
    assert!(String::from_utf8_lossy(&out.stderr).starts_with("usage: designcraft-cli"));
}

/// `script --help` was "unknown script option `--help`"; the line syntax was only in
/// `describe script.run` (#654).
#[test]
fn script_help_documents_the_syntax() {
    for flag in ["--help", "-h"] {
        let out = cli().args(["script", flag]).output().unwrap();
        let stdout = String::from_utf8_lossy(&out.stdout);
        assert!(out.status.success(), "{flag}: {:?} {}", out.status, String::from_utf8_lossy(&out.stderr));
        assert!(stdout.starts_with("usage: designcraft-cli script"), "{flag}: {stdout:?}");
        for needle in ["0-based", "$1.story", "$3.ids.0", "\\u2028", "ignore"] {
            assert!(stdout.contains(needle), "{flag}: no {needle:?} in {stdout:?}");
        }
    }
}

/// `designcraft-cli commands | head -1` panicked with "failed printing to
/// stdout: Broken pipe (os error 32)" and exit status 101.
#[test]
fn closed_stdout_ends_quietly() {
    use std::io::Read;
    let mut child = cli().arg("commands").stdout(Stdio::piped()).stderr(Stdio::piped()).spawn().unwrap();
    let mut first = [0u8; 16];
    child.stdout.as_mut().unwrap().read_exact(&mut first).unwrap();
    drop(child.stdout.take());
    let out = child.wait_with_output().unwrap();
    let err = String::from_utf8_lossy(&out.stderr);
    assert!(out.status.success() && !err.contains("panicked"), "{:?}: {err}", out.status);
}

/// `--scale`, `--page` and `--pdf-options` apply to the exports after them; one with no export
/// after it was silently ignored.
#[test]
fn run_export_options_come_before_their_export() {
    let dir = std::env::temp_dir().join(format!("dc-cli-order-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let (a, b) = (dir.join("a.png"), dir.join("b.png"));
    let (a, b) = (a.to_str().unwrap(), b.to_str().unwrap());
    for (opt, val) in [("--scale", "0.5"), ("--page", "1"), ("--pdf-options", r#"{"pages":"1"}"#)] {
        let out = cli().args(["run", "--sample", "--export", a, opt, val]).output().unwrap();
        let err = String::from_utf8_lossy(&out.stderr);
        assert!(!out.status.success(), "{opt} after the last --export: {err}");
        assert!(err.contains(opt) && err.contains("before"), "{err}");
    }
    assert!(!std::path::Path::new(a).exists(), "nothing is exported when the arguments are wrong");
    let out = cli().args(["run", "--sample", "--scale", "0.5", "--export", a, "--scale", "0.25", "--export", b]).output().unwrap();
    let err = String::from_utf8_lossy(&out.stderr);
    assert!(out.status.success(), "{err}");
    assert!(err.contains("wrote") && std::path::Path::new(a).exists() && std::path::Path::new(b).exists(), "{err}");
    let _ = std::fs::remove_dir_all(&dir);
}

/// `--pdf-options` must be a JSON object; any other JSON value used to panic when the PDF path was set on it.
#[test]
fn pdf_options_that_are_not_an_object_are_an_error_not_a_panic() {
    let dir = std::env::temp_dir().join(format!("dc-cli-pdfopts-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let pdf = dir.join("out.pdf");
    let pdf = pdf.to_str().unwrap();
    for val in ["[]", "true", "1", r#""x""#] {
        let out = cli().args(["run", "--sample", "--pdf-options", val, "--export", pdf]).output().unwrap();
        let err = String::from_utf8_lossy(&out.stderr);
        assert_eq!(out.status.code(), Some(1), "{val}: {err}");
        assert!(err.contains("--pdf-options") && !err.contains("panicked"), "{val}: {err}");
    }
    let out = cli().args(["run", "--sample", "--pdf-options", "null", "--export", pdf]).output().unwrap();
    assert!(out.status.success(), "{}", String::from_utf8_lossy(&out.stderr));
    let _ = std::fs::remove_dir_all(&dir);
}

#[cfg(unix)]
#[test]
fn raster_export_preserves_an_existing_reader_and_private_symlink_target() {
    use std::io::Read;
    use std::os::unix::fs::{PermissionsExt, symlink};
    struct Fixture(std::path::PathBuf);
    impl Drop for Fixture {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }
    let stamp = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos();
    let dir = std::env::temp_dir().join(format!("designcraft-cli-atomic-{}-{stamp}", std::process::id()));
    std::fs::create_dir(&dir).unwrap();
    let fixture = Fixture(dir);
    let target = fixture.0.join(format!("{}.png", "a".repeat(243)));
    let old = b"previous private export";
    std::fs::write(&target, old).unwrap();
    std::fs::set_permissions(&target, std::fs::Permissions::from_mode(0o600)).unwrap();
    let alias = fixture.0.join("linked.png");
    symlink(target.file_name().unwrap(), &alias).unwrap();
    let mut reader = std::fs::File::open(&target).unwrap();
    let out = cli().args(["run", "--sample", "--scale", "0.1", "--export", alias.to_str().unwrap()]).output().unwrap();
    assert!(out.status.success(), "{}", String::from_utf8_lossy(&out.stderr));
    let mut observed = Vec::new();
    reader.read_to_end(&mut observed).unwrap();
    assert!(observed == old, "CLI export changed bytes under an existing reader");
    assert!(std::fs::read(&target).unwrap().starts_with(b"\x89PNG\r\n\x1a\n"));
    assert_eq!(std::fs::metadata(&target).unwrap().permissions().mode() & 0o777, 0o600);
    assert!(std::fs::symlink_metadata(alias).unwrap().file_type().is_symlink());
}

/// Send `calls` (tool name, arguments) to `designcraft-cli mcp <args>`; returns whether it exited
/// cleanly, its stderr, and the stdout lines after `initialize`, one per call.
fn mcp_session(args: &[&str], calls: &[(&str, serde_json::Value)]) -> (bool, String, Vec<serde_json::Value>) {
    let mut child = cli().arg("mcp").args(args).stdin(Stdio::piped()).stdout(Stdio::piped()).stderr(Stdio::piped()).spawn().unwrap();
    {
        let stdin = child.stdin.as_mut().unwrap();
        writeln!(stdin, r#"{{"jsonrpc":"2.0","id":1,"method":"initialize","params":{{"protocolVersion":"2025-06-18"}}}}"#).unwrap();
        for (i, (name, arguments)) in calls.iter().enumerate() {
            let call = serde_json::json!({"jsonrpc": "2.0", "id": i + 2, "method": "tools/call", "params": {"name": name, "arguments": arguments}});
            writeln!(stdin, "{call}").unwrap();
        }
    }
    let out = child.wait_with_output().unwrap();
    let stdout = String::from_utf8(out.stdout).unwrap();
    let replies = stdout.lines().skip(1).map(|l| serde_json::from_str(l).unwrap()).collect();
    (out.status.success(), String::from_utf8_lossy(&out.stderr).into_owned(), replies)
}

fn text(reply: &serde_json::Value) -> &str {
    reply["result"]["content"][0]["text"].as_str().unwrap()
}

/// A `.designcraft` file with one page, made with the CLI itself.
fn make_document(path: &std::path::Path) {
    let out = cli().args(["run", "--cmd", "file.new={}", "--cmd"]).arg(format!("file.save={}", serde_json::json!({"path": path}))).output().unwrap();
    assert!(out.status.success(), "{}", String::from_utf8_lossy(&out.stderr));
    assert!(path.exists());
}

#[test]
fn mcp_automation_roots_confine_the_server() {
    let base = std::env::temp_dir().join(format!("dc-cli-roots-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&base);
    let (work, elsewhere) = (base.join("work"), base.join("elsewhere"));
    std::fs::create_dir_all(&work).unwrap();
    std::fs::create_dir_all(&elsewhere).unwrap();
    make_document(&work.join("in.designcraft"));
    make_document(&elsewhere.join("secret.designcraft"));
    let work_s = work.to_str().unwrap();
    let secret = elsewhere.join("secret.designcraft").to_str().unwrap().to_string();

    // Both spellings of the flags; inside works, outside is refused with a readable message.
    let read_flag = format!("--automation-read-root={work_s}");
    let calls = [
        ("open_document", serde_json::json!({"path": work.join("in.designcraft").to_str().unwrap()})),
        ("open_document", serde_json::json!({"path": secret})),
        ("save_document", serde_json::json!({"path": work.join("saved.designcraft").to_str().unwrap()})),
        ("save_document", serde_json::json!({"path": elsewhere.join("saved.designcraft").to_str().unwrap()})),
        ("export_png", serde_json::json!({"path": elsewhere.join("page.png").to_str().unwrap()})),
        ("render_page", serde_json::json!({"path": elsewhere.join("render.png").to_str().unwrap()})),
    ];
    let (success, stderr, replies) = mcp_session(&[&read_flag, "--automation-write-root", work_s], &calls);
    assert!(success, "{stderr}");
    assert!(stderr.contains("files confined"), "{stderr}");
    assert_eq!(replies[0]["result"]["isError"], false, "{}", replies[0]);
    assert_eq!(replies[1]["result"]["isError"], true);
    assert!(text(&replies[1]).contains("outside the read root"), "{}", text(&replies[1]));
    assert_eq!(replies[2]["result"]["isError"], false, "{}", replies[2]);
    assert!(work.join("saved.designcraft").exists());
    for reply in &replies[3..] {
        assert_eq!(reply["result"]["isError"], true, "{reply}");
        assert!(text(reply).contains("outside the write root"), "{}", text(reply));
    }
    for name in ["saved.designcraft", "page.png", "render.png"] {
        assert!(!elsewhere.join(name).exists(), "{name} was written outside the write root");
    }

    // A root left out grants none of its access.
    let save = [("new_document", serde_json::json!({})), calls[2].clone()];
    let (_, _, replies) = mcp_session(&[&read_flag], &save);
    assert_eq!(replies[1]["result"]["isError"], true);
    assert!(text(&replies[1]).contains("write authority is absent"), "{}", text(&replies[1]));
    let (_, _, replies) = mcp_session(&["--automation-write-root", work_s], &calls[..1]);
    assert_eq!(replies[0]["result"]["isError"], true);
    assert!(text(&replies[0]).contains("read authority is absent"), "{}", text(&replies[0]));

    // No flags: nothing is confined.
    let (success, _, replies) = mcp_session(&[], &calls[1..2]);
    assert!(success);
    assert_eq!(replies[0]["result"]["isError"], false, "{}", replies[0]);
    let _ = std::fs::remove_dir_all(base);
}

#[test]
fn mcp_automation_root_flags_are_checked_up_front() {
    let base = std::env::temp_dir().join(format!("dc-cli-roots-args-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&base);
    std::fs::create_dir_all(&base).unwrap();
    let dir = base.to_str().unwrap();
    let missing = base.join("missing");
    let fails = |args: &[&str], needle: &str| {
        let mut full = vec!["mcp"];
        full.extend_from_slice(args);
        let o = cli().args(&full).output().unwrap();
        assert!(!o.status.success(), "{args:?}");
        let err = String::from_utf8_lossy(&o.stderr).into_owned();
        assert!(err.contains(needle), "{args:?}: {err}");
    };
    // The flags confine the in-process server, so they cannot be combined with --connect.
    fails(&["--connect", "7979", "--automation-read-root", dir], "--connect");
    fails(&["--automation-write-root", dir, "--connect", "7979"], "--connect");
    fails(&["--automation-read-root", dir, "--automation-read-root", dir], "given twice");
    fails(&["--automation-write-root", dir, "--automation-write-root", dir], "given twice");
    fails(&["--automation-read-root", dir, "--automation-read-root=x"], "given twice");
    fails(&["--automation-read-root"], "needs a folder");
    fails(&["--automation-write-root", "--sample"], "needs a folder");
    fails(&["--automation-read-root", missing.to_str().unwrap()], "cannot open the automation read root");
    fails(&["--automation-write-root="], "needs a non-empty folder");
    // A file is not a folder.
    std::fs::write(base.join("f.txt"), "x").unwrap();
    fails(&["--automation-write-root", base.join("f.txt").to_str().unwrap()], "cannot open the automation write root");
    let _ = std::fs::remove_dir_all(base);
}

#[test]
fn usage_lists_the_automation_root_flags() {
    let out = cli().arg("--help").output().unwrap();
    let usage = String::from_utf8_lossy(&out.stdout);
    assert!(usage.contains("--automation-read-root DIR") && usage.contains("--automation-write-root DIR"), "{usage}");
}
