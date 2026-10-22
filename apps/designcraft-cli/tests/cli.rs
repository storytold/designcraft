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

/// `--export` wrote a PNG of the page under every name it did not know: `out.html`, `out.txt` and
/// `OUT.PDF` were page images, and so was `out.designcraft` from `script` (#432).
#[test]
fn export_writes_the_file_type_its_extension_names() {
    let frame = r#"{"rect": [72, 72, 400, 200], "content": "text", "text": "Hello"}"#;
    let types: [(&str, &[u8]); 13] = [
        // No extension: a PNG, as before.
        ("a", b"\x89PNG"),
        ("a.png", b"\x89PNG"),
        ("a.jpg", b"\xFF\xD8"),
        ("A.JPEG", b"\xFF\xD8"),
        ("a.pdf", b"%PDF"),
        ("A.PDF", b"%PDF"),
        ("a.designcraft", b"PK"),
        ("a.idml", b"PK"),
        ("a.epub", b"PK"),
        ("a.html", b"<!DOCTYPE html>"),
        ("a.htm", b"<!DOCTYPE html>"),
        ("a.rtf", b"{\\rtf"),
        ("a.txt", b"Hello"),
    ];
    for mode in ["run", "script"] {
        let dir = std::env::temp_dir().join(format!("dc-cli-types-{mode}-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let script = dir.join("page.dcs");
        std::fs::write(&script, format!("frame.create {frame}\n")).unwrap();
        let mut args: Vec<String> = match mode {
            "run" => vec!["run".into(), "--cmd".into(), format!("frame.create={frame}")],
            _ => vec!["script".into(), script.to_str().unwrap().into()],
        };
        for (name, _) in types {
            args.extend(["--export".into(), dir.join(name).to_str().unwrap().into()]);
        }
        let out = cli().args(&args).output().unwrap();
        assert!(out.status.success(), "{mode}: {}", String::from_utf8_lossy(&out.stderr));
        for (name, start) in types {
            let bytes = std::fs::read(dir.join(name)).unwrap();
            assert!(bytes.starts_with(start), "{mode} --export {name} wrote {:?}", String::from_utf8_lossy(&bytes[..bytes.len().min(16)]));
        }
        let _ = std::fs::remove_dir_all(&dir);
    }
}

/// A file type `--export` does not write is an error, before anything runs or is written.
#[test]
fn export_to_an_unknown_file_type_is_an_error() {
    let dir = std::env::temp_dir().join(format!("dc-cli-unknown-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let (png, doc, unknown) = (dir.join("a.png"), dir.join("a.designcraft"), dir.join("a.xyz"));
    let (png, doc, unknown) = (png.to_str().unwrap(), doc.to_str().unwrap(), unknown.to_str().unwrap());
    for args in [["run", "--sample", "--export", png, "--export", unknown], ["script", "-", "--save", doc, "--export", unknown]] {
        let out = cli().args(args).output().unwrap();
        let err = String::from_utf8_lossy(&out.stderr);
        assert!(!out.status.success(), "{args:?}: {err}");
        assert!(err.contains(unknown) && err.contains(".html"), "{args:?}: {err}");
    }
    assert!(std::fs::read_dir(&dir).unwrap().next().is_none(), "nothing is written when a file type is unknown");
    let _ = std::fs::remove_dir_all(&dir);
}

/// With `--connect` the exports happen in the running app. `--export copy.designcraft` saves a
/// copy there: Save As would make the copy the file the open document saves to from then on.
#[test]
fn script_export_of_the_document_in_the_running_app_saves_a_copy() {
    use std::io::{BufRead, BufReader};
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port().to_string();
    // Stands in for the app: answers every request and keeps what was asked.
    let app = std::thread::spawn(move || {
        let (stream, _) = listener.accept().unwrap();
        let mut reply = stream.try_clone().unwrap();
        let mut asked = Vec::new();
        for line in BufReader::new(stream).lines() {
            let request: serde_json::Value = serde_json::from_str(&line.unwrap()).unwrap();
            asked.push((request["params"]["command"].clone(), request["params"]["params"]["path"].clone()));
            writeln!(reply, r#"{{"ok": true, "result": {{}}}}"#).unwrap();
        }
        asked
    });
    let out = cli().args(["script", "-", "--connect", &port, "--export", "copy.designcraft"]).output().unwrap();
    assert!(out.status.success(), "{}", String::from_utf8_lossy(&out.stderr));
    assert_eq!(app.join().unwrap(), [(serde_json::json!("file.saveACopy"), serde_json::json!("copy.designcraft"))]);
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

/// #776: a key the command doesn't take fails the run (`--cmd`) or the step (`script`) instead of
/// being ignored.
#[test]
fn unknown_params_fail() {
    let out = cli().args(["run", "--cmd", r#"style.list={"bogus":1}"#]).output().unwrap();
    assert!(!out.status.success());
    assert!(String::from_utf8_lossy(&out.stderr).contains("unknown parameter `bogus`; the command takes no parameters"));
    let mut child = cli().args(["script", "-"]).stdin(Stdio::piped()).stdout(Stdio::piped()).stderr(Stdio::piped()).spawn().unwrap();
    child.stdin.take().unwrap().write_all(b"document.inspect\nfile.new {\"widht\": 300}\n").unwrap();
    let out = child.wait_with_output().unwrap();
    assert!(!out.status.success());
    let v: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
    assert_eq!(v["failedIndex"], 1);
    assert!(v.to_string().contains("did you mean `width`?"), "{v}");
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

/// `--pdf-options` that was JSON but no object (`[1]`) panicked when the PDF's path was added to it.
/// `null` means no options (see `pdf_options_that_are_not_an_object_are_an_error_not_a_panic`).
#[test]
fn run_pdf_options_that_are_no_object_are_an_error() {
    let pdf = std::env::temp_dir().join(format!("dc-cli-options-{}.pdf", std::process::id()));
    for options in ["[1]", "\"all\"", "1"] {
        let out = cli().args(["run", "--sample", "--pdf-options", options, "--export", pdf.to_str().unwrap()]).output().unwrap();
        let err = String::from_utf8_lossy(&out.stderr);
        assert!(!out.status.success(), "{options}: {err}");
        assert!(err.contains("--pdf-options") && !err.contains("panicked"), "{options}: {err}");
    }
    assert!(!pdf.exists());
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

/// `--scale 0`, a negative value or `nan` wrote a 1×1 PNG and exited 0; `inf` said "no such page".
#[test]
fn run_scale_that_is_not_finite_and_positive_is_an_error() {
    let dir = std::env::temp_dir().join(format!("dc-cli-scale-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let png = dir.join("a.png");
    for scale in ["nan", "0", "-2", "inf", "-inf"] {
        let out = cli().args(["run", "--sample", "--scale", scale, "--export", png.to_str().unwrap()]).output().unwrap();
        let err = String::from_utf8_lossy(&out.stderr);
        assert!(!out.status.success(), "--scale {scale}: {err}");
        assert!(err.contains("bad --scale"), "--scale {scale}: {err}");
    }
    assert!(!png.exists(), "nothing is exported when the scale is wrong");
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

/// Every file a run writes is reported on stdout as one JSON line, where a `--cmd` result already
/// goes, so a script can read what was written without merging in stderr (#1214). The lines meant
/// for a person stay on stderr.
#[test]
fn files_written_are_reported_on_stdout_as_json() {
    let dir = std::env::temp_dir().join(format!("dc-cli-wrote-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let pages = dir.join("pages");
    let out = cli().args(["run", "--sample", "--all-pages", pages.to_str().unwrap()]).output().unwrap();
    assert!(out.status.success(), "{}", String::from_utf8_lossy(&out.stderr));

    let stdout = String::from_utf8_lossy(&out.stdout);
    let lines: Vec<serde_json::Value> = stdout.lines().map(|l| serde_json::from_str(l).expect("each line is JSON")).collect();
    assert!(!lines.is_empty(), "a page export says what it wrote: {stdout:?}");
    assert_eq!(lines.len(), std::fs::read_dir(&pages).unwrap().count(), "one line per file written");
    for (n, line) in lines.iter().enumerate() {
        let path = line["wrote"].as_str().expect("the path it wrote");
        assert!(std::path::Path::new(path).is_file(), "{path} exists");
        assert_eq!(path, pages.join(format!("page-{:03}.png", n + 1)).to_string_lossy(), "in page order");
        assert!(line["width"].as_u64().is_some_and(|w| w > 0) && line["height"].as_u64().is_some_and(|h| h > 0), "{line}");
    }

    // The human lines are still on stderr, and nothing machine-readable is mixed into them.
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert_eq!(stderr.lines().filter(|l| l.starts_with("wrote ")).count(), lines.len());
    assert!(!stderr.lines().any(|l| l.starts_with('{')), "stderr carries no JSON: {stderr:?}");

    // A PDF export says the same, with what a PDF has to report.
    let pdf = dir.join("out.pdf");
    let out = cli().args(["run", "--sample", "--export", pdf.to_str().unwrap()]).output().unwrap();
    assert!(out.status.success(), "{}", String::from_utf8_lossy(&out.stderr));
    let line: serde_json::Value = serde_json::from_str(String::from_utf8_lossy(&out.stdout).trim()).expect("JSON");
    assert_eq!(line["wrote"].as_str(), Some(pdf.to_string_lossy().as_ref()));
    assert!(line["pages"].as_u64().is_some_and(|p| p > 0) && line["bytes"].as_u64().is_some_and(|b| b > 0), "{line}");
    // `warnings` is an array whether the export reported any or not, as the documented shape of
    // the line says: the result leaves it out when there are none.
    assert!(line["warnings"].is_array(), "{line}");

    // A deck and a saved document report in the same shape, with what each has to say.
    let pptx = dir.join("out.pptx");
    let saved = dir.join("out.designcraft");
    let out = cli().args(["run", "--sample", "--export", pptx.to_str().unwrap(), "--export", saved.to_str().unwrap()]).output().unwrap();
    assert!(out.status.success(), "{}", String::from_utf8_lossy(&out.stderr));
    let stdout = String::from_utf8_lossy(&out.stdout);
    let lines: Vec<serde_json::Value> = stdout.lines().map(|l| serde_json::from_str(l).expect("each line is JSON")).collect();
    assert_eq!(lines.len(), 2, "one line per file, in the order the exports were given: {stdout:?}");
    assert_eq!(lines[0]["wrote"].as_str(), Some(pptx.to_string_lossy().as_ref()));
    assert!(lines[0]["slides"].as_u64().is_some_and(|n| n > 0) && lines[0]["bytes"].as_u64().is_some_and(|b| b > 0), "{}", lines[0]);
    assert!(lines[0]["warnings"].is_array(), "{}", lines[0]);
    assert_eq!(lines[1]["wrote"].as_str(), Some(saved.to_string_lossy().as_ref()));
    assert_eq!(lines[1]["saved"], serde_json::json!(true), "{}", lines[1]);
    assert!(pptx.is_file() && saved.is_file());

    // A --cmd that writes a file reports it as well: the command result is its own line, and the
    // written file gets the same record an --export would have given it.
    let cmd_pdf = dir.join("cmd.pdf");
    let out = cli().args(["run", "--sample", "--cmd", &format!("file.exportPdf={{\"path\": {:?}}}", cmd_pdf.to_string_lossy())]).output().unwrap();
    assert!(out.status.success(), "{}", String::from_utf8_lossy(&out.stderr));
    let stdout = String::from_utf8_lossy(&out.stdout);
    let lines: Vec<serde_json::Value> = stdout.lines().map(|l| serde_json::from_str(l).expect("each line is JSON")).collect();
    assert_eq!(lines.len(), 2, "the command result, then what it wrote: {stdout:?}");
    assert!(lines[0]["path"].is_string() && lines[0]["wrote"].is_null(), "the command's own result first: {}", lines[0]);
    assert_eq!(lines[1]["wrote"].as_str(), Some(cmd_pdf.to_string_lossy().as_ref()), "{}", lines[1]);
    assert!(lines[1]["bytes"].as_u64().is_some_and(|b| b > 0) && lines[1]["warnings"].is_array(), "{}", lines[1]);
    assert!(cmd_pdf.is_file());
    assert!(String::from_utf8_lossy(&out.stderr).lines().any(|l| l.starts_with("wrote ")), "and a human line on stderr");

    // The last line shape: an export that runs a command for the file type (.html, .rtf, .txt,
    // .idml, .epub). Those report bytes and the warnings the export gives -- an EPUB or HTML
    // export says what it left out -- and nothing else (#1304 review).
    // `.rtf` and `.txt` go through `file.exportText`, which needs a text target ("edit text or
    // select a text frame"), so they are not reachable from a bare `run --sample`.
    for name in ["out.html", "out.idml", "out.epub"] {
        let file = dir.join(name);
        let out = cli().args(["run", "--sample", "--export", file.to_str().unwrap()]).output().unwrap();
        assert!(out.status.success(), "{name}: {}", String::from_utf8_lossy(&out.stderr));
        let stdout = String::from_utf8_lossy(&out.stdout);
        let line: serde_json::Value = serde_json::from_str(stdout.trim()).unwrap_or_else(|e| panic!("{name}: {stdout:?}: {e}"));
        assert_eq!(line["wrote"].as_str(), Some(file.to_string_lossy().as_ref()), "{name}: {line}");
        assert!(line["bytes"].as_u64().is_some_and(|b| b > 0), "{name}: {line}");
        // `warnings` is an array whichever way the command answers: the HTML and EPUB exports
        // report what they left out, the text export reports nothing at all.
        assert!(line["warnings"].is_array(), "{name}: {line}");
        assert!(file.is_file(), "{name} exists");
        // And the human line is still the one on stderr.
        let stderr = String::from_utf8_lossy(&out.stderr);
        assert!(stderr.lines().any(|l| l.starts_with("wrote ")), "{name}: {stderr:?}");
        assert!(!stderr.lines().any(|l| l.starts_with('{')), "{name}: stderr carries no JSON: {stderr:?}");
    }

    // A command that writes a whole folder answers with the files it wrote, not one path, and
    // every one of them is reported (#1304 review).
    let pkg = dir.join("pkg");
    let out = cli().args(["run", "--sample", "--cmd", &format!("file.package={{\"dir\": {:?}}}", pkg.to_string_lossy())]).output().unwrap();
    assert!(out.status.success(), "{}", String::from_utf8_lossy(&out.stderr));
    let stdout = String::from_utf8_lossy(&out.stdout);
    let lines: Vec<serde_json::Value> = stdout.lines().map(|l| serde_json::from_str(l).expect("each line is JSON")).collect();
    let (results, written): (Vec<_>, Vec<_>) = lines.iter().partition(|l| l["wrote"].is_null());
    assert_eq!(results.len(), 1, "the command's own result: {stdout:?}");
    let files = results[0]["files"].as_array().expect("the files it wrote");
    assert!(files.len() > 1, "the sample packages several files: {}", results[0]["files"]);
    assert_eq!(written.len(), files.len(), "one record per file written: {stdout:?}");
    for (record, file) in written.iter().zip(files) {
        assert_eq!(record["wrote"], *file, "in the order the command reports them");
        assert!(std::path::Path::new(file.as_str().unwrap_or("")).is_file(), "{file} exists");
    }

    // An EPUB or HTML export goes through its command, whose result can carry warnings: the line
    // keeps them, as the PDF and deck lines do (#1304 review).
    let epub = dir.join("out.epub");
    let out = cli().args(["run", "--sample", "--export", epub.to_str().unwrap()]).output().unwrap();
    assert!(out.status.success(), "{}", String::from_utf8_lossy(&out.stderr));
    let line: serde_json::Value = serde_json::from_str(String::from_utf8_lossy(&out.stdout).trim()).expect("JSON");
    assert_eq!(line["wrote"].as_str(), Some(epub.to_string_lossy().as_ref()));
    assert!(line["bytes"].as_u64().is_some_and(|b| b > 0) && line["warnings"].is_array(), "{line}");

    // A --cmd that writes nothing still reports only its result.
    let out = cli().args(["run", "--sample", "--cmd", "document.list"]).output().unwrap();
    assert!(out.status.success(), "{}", String::from_utf8_lossy(&out.stderr));
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert_eq!(stdout.lines().count(), 1, "{stdout:?}");
    assert!(!stdout.contains("wrote"), "{stdout:?}");
    let _ = std::fs::remove_dir_all(&dir);
}
