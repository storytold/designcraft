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
