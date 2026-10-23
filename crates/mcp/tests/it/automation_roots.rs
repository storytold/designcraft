//! `designcraft-cli mcp --automation-read-root <dir> --automation-write-root <dir>`: through the MCP
//! protocol, every file a command reads comes from inside the read root and every file it writes
//! goes inside the write root. Paths that leave a root (`..`, another folder, a sibling with the
//! same prefix, a link) are refused, nothing from a refused file reaches the document, and nothing
//! is written outside.

use std::path::{Path, PathBuf};

use designcraft_engine::Session;
use designcraft_engine::file_access::AutomationRoots;
use designcraft_mcp::{Headless, Server};
use serde_json::{Value, json};

/// What the secret files hold, so a test can look for it in everything the agent is told.
const SECRET: &str = "TOP SECRET";

/// A 1x1 PNG, so `place_image` has something real to read.
const PNG: &[u8] = &[
    0x89, 0x50, 0x4e, 0x47, 0x0d, 0x0a, 0x1a, 0x0a, 0x00, 0x00, 0x00, 0x0d, 0x49, 0x48, 0x44, 0x52, 0x00, 0x00, 0x00, 0x01, 0x00, 0x00, 0x00, 0x01,
    0x08, 0x06, 0x00, 0x00, 0x00, 0x1f, 0x15, 0xc4, 0x89, 0x00, 0x00, 0x00, 0x0d, 0x49, 0x44, 0x41, 0x54, 0x78, 0x9c, 0x63, 0xf8, 0xcf, 0xc0, 0xf0,
    0x1f, 0x00, 0x05, 0x00, 0x01, 0xff, 0x89, 0x99, 0x3d, 0x1d, 0x00, 0x00, 0x00, 0x00, 0x49, 0x45, 0x4e, 0x44, 0xae, 0x42, 0x60, 0x82,
];

/// A `.designcraft` file of `pages` pages with `text` in a frame on the first, made outside any roots.
fn designcraft_file(path: &Path, pages: u32, text: &str) {
    let mut s = Session::new();
    s.execute("file.new", &json!({"pages": pages})).unwrap();
    s.execute("frame.create", &json!({"rect": [36, 36, 300, 100], "content": "text", "text": text})).unwrap();
    s.execute("file.save", &json!({"path": path.to_str().unwrap()})).unwrap();
}

/// A fresh folder holding `in` (the read root), `out` (the write root), `outside` and `in2` (a
/// sibling whose name starts with `in`), and a server confined to `in` and `out` (either may be
/// left out). `in` and `outside` each hold the same kinds of files, those outside carrying
/// [`SECRET`]. The server starts with a new one-page document.
fn confined(tag: &str, read: bool, write: bool) -> (Server, PathBuf) {
    let base = std::env::temp_dir().join(format!("dc-mcp-roots-{tag}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&base);
    for d in ["in", "out", "outside", "in2"] {
        std::fs::create_dir_all(base.join(d)).unwrap();
    }
    for (dir, text) in [("in", "Inside text"), ("outside", SECRET), ("in2", "Sibling")] {
        let dir = base.join(dir);
        designcraft_file(&dir.join("doc.designcraft"), if dir.ends_with("in") { 1 } else { 3 }, text);
        std::fs::write(dir.join("pic.png"), PNG).unwrap();
        std::fs::write(dir.join("data.csv"), format!("name,city\n{text},Oslo\n")).unwrap();
        std::fs::write(dir.join("swatches.ase"), b"not a swatch file").unwrap();
        std::fs::write(dir.join("profile.icc"), b"not a profile").unwrap();
        std::fs::write(dir.join("lib.json"), format!("{{\"items\": [], \"note\": \"{text}\"}}")).unwrap();
        std::fs::write(dir.join("shape.dtd"), format!("<!ELEMENT {} (#PCDATA)>", text.replace(' ', "_"))).unwrap();
        std::fs::write(dir.join("tags.xml"), format!("<root><p>{text}</p></root>")).unwrap();
    }
    let (in_dir, out_dir) = (base.join("in"), base.join("out"));
    let roots = AutomationRoots::new(read.then_some(in_dir.as_path()), write.then_some(out_dir.as_path())).unwrap();
    let mut server = Server::new(Box::new(Headless::with_document().with_automation_roots(roots)));
    ok(&mut server, "new_document", json!({}));
    (server, base)
}

fn s(p: &Path) -> String {
    p.to_string_lossy().into_owned()
}

/// `tools/call` -> (isError, every text block).
fn call(server: &mut Server, name: &str, args: Value) -> (bool, String) {
    let line = json!({"jsonrpc": "2.0", "id": 1, "method": "tools/call", "params": {"name": name, "arguments": args}}).to_string();
    let reply: Value = serde_json::from_str(&server.handle_line(&line).unwrap()).unwrap();
    let result = &reply["result"];
    let text = result["content"].as_array().unwrap().iter().filter_map(|c| c["text"].as_str()).collect::<Vec<_>>().join("\n");
    (result["isError"] == true, text)
}

fn ok(server: &mut Server, name: &str, args: Value) -> String {
    let (error, text) = call(server, name, args.clone());
    assert!(!error, "{name} {args}: {text}");
    text
}

/// A command through the `execute` tool.
fn run(server: &mut Server, command: &str, params: Value) -> (bool, String) {
    call(server, "execute", json!({"command": command, "params": params}))
}

fn run_ok(server: &mut Server, command: &str, params: Value) -> Value {
    let (error, text) = run(server, command, params.clone());
    assert!(!error, "{command} {params}: {text}");
    serde_json::from_str(&text).unwrap_or(Value::Null)
}

/// `command` is refused with `why` in its message, and the message says nothing of the file.
fn refused(server: &mut Server, command: &str, params: Value, why: &str) {
    let (error, text) = run(server, command, params.clone());
    assert!(error && text.contains(why), "{command} {params} should be refused ({why}): {text}");
    assert!(!text.contains(SECRET), "{command}: {text}");
}

/// The open document's page count, which tells the three-page files outside from the one-page one.
fn pages(server: &mut Server) -> usize {
    let doc: Value = serde_json::from_str(&ok(server, "inspect_document", json!({}))).unwrap();
    doc["pageCount"].as_u64().unwrap() as usize
}

fn outside_names(base: &Path) -> Vec<String> {
    let mut names: Vec<String> =
        std::fs::read_dir(base.join("outside")).unwrap().map(|e| e.unwrap().file_name().to_string_lossy().into_owned()).collect();
    names.sort();
    names
}

#[test]
fn documents_open_only_from_inside_the_read_root() {
    let (mut server, base) = confined("open", true, true);
    let sep = std::path::MAIN_SEPARATOR;
    ok(&mut server, "open_document", json!({"path": s(&base.join("in/doc.designcraft"))}));
    assert_eq!(pages(&mut server), 1, "the file inside the root is the open document");

    let escape = format!("{}{sep}..{sep}outside{sep}doc.designcraft", s(&base.join("in")));
    let paths = [s(&base.join("outside/doc.designcraft")), escape, s(&base.join("in2/doc.designcraft")), s(&base.join("out/new.designcraft"))];
    for path in paths {
        let (error, text) = call(&mut server, "open_document", json!({"path": path}));
        assert!(error && text.contains("outside the read root"), "{path}: {text}");
        assert!(!text.contains(SECRET), "{text}");
        refused(&mut server, "file.open", json!({"path": path}), "outside the read root");
        refused(&mut server, "file.openIdml", json!({"path": path}), "outside the read root");
    }
    // Nothing from the refused files reached the session: the open document is still the one it was.
    assert_eq!(pages(&mut server), 1);
    let _ = std::fs::remove_dir_all(base);
}

#[test]
fn every_command_that_reads_a_path_is_refused_outside_the_read_root() {
    let (mut server, base) = confined("reads", true, true);
    let outside = |name: &str| s(&base.join("outside").join(name));
    let inside = |name: &str| s(&base.join("in").join(name));

    // A placed image to relink and a data source to update, both from inside the root.
    ok(&mut server, "place_image", json!({"path": inside("pic.png"), "x": 36, "y": 36, "width": 72}));
    let links = run_ok(&mut server, "links.list", json!({}));
    let asset = links[0]["asset"].clone();
    let fields = run_ok(&mut server, "data.source.select", json!({"path": inside("data.csv")}));
    assert!(fields["fields"].to_string().contains("city"), "{fields}");
    let before = ok(&mut server, "inspect_document", json!({}));

    let reads: Vec<(&str, Value)> = vec![
        ("file.place", json!({"path": outside("pic.png")})),
        ("place.load", json!({"path": outside("pic.png")})),
        ("place.styles", json!({"path": outside("tags.xml")})),
        ("links.relink", json!({"asset": asset, "path": outside("pic.png")})),
        ("links.relinkFolder", json!({"dir": s(&base.join("outside"))})),
        ("media.options", json!({"poster": {"path": outside("pic.png")}})),
        ("color.loadProfile", json!({"path": outside("profile.icc")})),
        ("swatch.load", json!({"path": outside("swatches.ase")})),
        ("library.open", json!({"path": outside("lib.json")})),
        ("book.open", json!({"path": outside("lib.json")})),
        ("data.source.select", json!({"path": outside("data.csv")})),
        ("data.merge", json!({"path": outside("data.csv")})),
        ("xml.loadDtd", json!({"path": outside("shape.dtd")})),
        ("file.importXml", json!({"path": outside("tags.xml")})),
        ("file.recovery.list", json!({"dir": s(&base.join("outside"))})),
        ("file.recovery.open", json!({"dir": s(&base.join("outside"))})),
    ];
    for (command, params) in reads {
        refused(&mut server, command, params, "outside the read root");
    }
    let (error, text) = call(&mut server, "place_image", json!({"path": outside("pic.png")}));
    assert!(error && text.contains("outside the read root") && !text.contains(SECRET), "{text}");
    // The control-channel method behind `open_document`.
    let (error, text) = call(&mut server, "execute", json!({"command": "app.open", "params": {"path": outside("doc.designcraft")}}));
    assert!(error && !text.contains(SECRET), "app.open: {text}");
    assert_eq!(ok(&mut server, "inspect_document", json!({})), before, "the refused reads changed nothing");
    assert_eq!(run_ok(&mut server, "data.source.select", json!({"path": inside("data.csv")}))["records"], 1);

    // Inside the root the same commands get as far as reading the file, so the table above is not
    // refusing for some other reason: a file that isn't a profile or swatch file is reported as such.
    for (command, params) in [("color.loadProfile", json!({"path": inside("profile.icc")})), ("swatch.load", json!({"path": inside("swatches.ase")}))]
    {
        let (error, text) = run(&mut server, command, params);
        assert!(error && !text.contains("automation"), "{command}: {text}");
    }
    run_ok(&mut server, "xml.loadDtd", json!({"path": inside("shape.dtd")}));
    run_ok(&mut server, "file.importXml", json!({"path": inside("tags.xml")}));
    let _ = std::fs::remove_dir_all(base);
}

#[test]
fn files_named_inside_a_document_are_read_through_the_roots_too() {
    let (mut server, base) = confined("stored", true, true);
    // A document in the read root whose picture is linked to a file outside it, and whose data
    // source is a file outside it, made by someone without roots.
    let mut s = Session::new();
    s.execute("file.new", &json!({})).unwrap();
    std::fs::write(base.join("outside/linked.png"), PNG).unwrap();
    s.execute("file.place", &json!({"path": self::s(&base.join("outside/linked.png")), "x": 36, "y": 36, "width": 72})).unwrap();
    s.execute("data.source.select", &json!({"path": self::s(&base.join("outside/data.csv"))})).unwrap();
    s.execute("file.save", &json!({"path": self::s(&base.join("in/linked.designcraft"))})).unwrap();
    assert_eq!(s.execute("links.list", &json!({})).unwrap()[0]["status"], "ok");

    ok(&mut server, "open_document", json!({"path": self::s(&base.join("in/linked.designcraft"))}));
    let links = run_ok(&mut server, "links.list", json!({}));
    assert_eq!(links[0]["status"], "missing", "a link outside the read root reads as a file that isn't there: {links}");
    refused(&mut server, "data.source.update", json!({}), "outside the read root");
    let _ = std::fs::remove_dir_all(base);
}

#[test]
fn every_writing_command_is_refused_outside_the_write_root() {
    let (mut server, base) = confined("write", true, true);
    let outside = |name: &str| s(&base.join("outside").join(name));
    let inside = |name: &str| s(&base.join("out").join(name));
    let fixtures = outside_names(&base);
    // Something to export: a picture to package and a text frame, selected so `file.exportText`
    // and `snippet.export` have their input.
    ok(&mut server, "place_image", json!({"path": s(&base.join("in/pic.png")), "x": 36, "y": 200, "width": 72}));
    run_ok(&mut server, "frame.create", json!({"rect": [36, 300, 300, 100], "content": "text", "text": "Selected"}));
    run_ok(&mut server, "select.firstAbove", json!({}));

    // Every command that writes a file, with the path parameter it takes.
    let writes: Vec<(&str, &str)> = vec![
        ("file.save", "path"),
        ("file.saveAs", "path"),
        ("file.saveACopy", "path"),
        ("file.exportPdf", "path"),
        ("file.exportEpub", "path"),
        ("file.exportFixedEpub", "path"),
        ("file.exportHtml", "path"),
        ("file.exportText", "path"),
        ("file.exportXml", "path"),
        ("file.exportIdml", "path"),
        ("file.printBooklet", "path"),
        ("snippet.export", "path"),
        ("swatch.save", "path"),
        ("library.new", "path"),
        ("book.exportPdf", "path"),
        ("book.new", "path"),
        ("file.package", "dir"),
        ("links.copyTo", "dir"),
        ("file.recovery.save", "dir"),
    ];
    // A book with a chapter to export, so `book.exportPdf` gets as far as its path.
    run_ok(&mut server, "book.new", json!({"path": inside("first.dcbook")}));
    run_ok(&mut server, "book.add", json!({"path": s(&base.join("in/doc.designcraft"))}));
    for (command, key) in &writes {
        let target = if *key == "dir" { outside("folder") } else { outside(&format!("out-{command}")) };
        refused(&mut server, command, json!({*key: target}), "outside the write root");
    }
    for tool in ["save_document", "export_png", "render_page"] {
        let (error, text) = call(&mut server, tool, json!({"path": outside("tool.out")}));
        assert!(error && text.contains("outside the write root"), "{tool}: {text}");
    }
    // The control-channel methods behind `save_document` and `export_png`.
    for method in ["app.save", "app.export"] {
        let (error, text) = call(&mut server, "execute", json!({"command": method, "params": {"path": outside("app.out")}}));
        assert!(error, "{method}: {text}");
    }
    // A sibling folder with the same prefix, `..` out of the root, and the read root.
    let sep = std::path::MAIN_SEPARATOR;
    for path in [
        s(&base.join("out2/x.designcraft")),
        format!("{}{sep}..{sep}outside{sep}x.designcraft", s(&base.join("out"))),
        s(&base.join("in/x.designcraft")),
    ] {
        let (error, text) = call(&mut server, "save_document", json!({"path": path}));
        assert!(error && text.contains("outside the write root"), "{path}: {text}");
    }
    assert_eq!(outside_names(&base), fixtures, "nothing was written outside beside the files made at the start");
    assert!(!base.join("out2").exists() && !base.join("in/x.designcraft").exists());

    // Inside the write root every one of them works, and the files appear.
    for (command, key) in &writes {
        let target = if *key == "dir" { inside(&format!("dir-{command}")) } else { inside(&format!("out-{command}")) };
        run_ok(&mut server, command, json!({*key: target}));
    }
    for tool in ["export_png", "render_page"] {
        ok(&mut server, tool, json!({"path": inside(&format!("{tool}.png"))}));
        assert!(base.join("out").join(format!("{tool}.png")).exists(), "{tool}");
    }
    for file in
        ["out-file.save.designcraft", "out-file.exportPdf", "out-file.exportIdml", "out-book.exportPdf", "out-swatch.save", "out-snippet.export"]
    {
        assert!(base.join("out").join(file).exists(), "{file}");
    }
    assert!(base.join("out/dir-file.package/Instructions.txt").exists() && base.join("out/dir-links.copyTo/pic.png").exists());
    let _ = std::fs::remove_dir_all(base);
}

#[test]
fn one_root_grants_only_its_own_access() {
    let (mut read_only, base) = confined("read-only", true, false);
    ok(&mut read_only, "open_document", json!({"path": s(&base.join("in/doc.designcraft"))}));
    run_ok(&mut read_only, "select.firstAbove", json!({}));
    for (command, key) in [("file.save", "path"), ("file.exportPdf", "path"), ("file.package", "dir"), ("file.exportText", "path")] {
        refused(&mut read_only, command, json!({key: s(&base.join("out/x"))}), "write authority is absent");
    }
    for tool in ["save_document", "export_png", "render_page"] {
        let (error, text) = call(&mut read_only, tool, json!({"path": s(&base.join("out/x.png"))}));
        assert!(error && text.contains("write authority is absent"), "{tool}: {text}");
    }
    // Calls that return bytes instead of writing a path keep working.
    ok(&mut read_only, "render_page", json!({"scale": 0.25}));
    run_ok(&mut read_only, "file.exportPdf", json!({}));
    assert!(std::fs::read_dir(base.join("out")).unwrap().next().is_none());
    let _ = std::fs::remove_dir_all(base);

    let (mut write_only, base) = confined("write-only", false, true);
    for (command, key) in
        [("file.open", "path"), ("file.place", "path"), ("swatch.load", "path"), ("data.source.select", "path"), ("file.recovery.list", "dir")]
    {
        refused(&mut write_only, command, json!({key: s(&base.join("in/doc.designcraft"))}), "read authority is absent");
    }
    let (error, text) = call(&mut write_only, "place_image", json!({"path": s(&base.join("in/pic.png"))}));
    assert!(error && text.contains("read authority is absent"), "{text}");
    ok(&mut write_only, "save_document", json!({"path": s(&base.join("out/x.designcraft"))}));
    assert!(base.join("out/x.designcraft").exists());
    let _ = std::fs::remove_dir_all(base);
}

#[cfg(unix)]
#[test]
fn links_out_of_a_root_are_refused() {
    let (mut server, base) = confined("links", true, true);
    std::os::unix::fs::symlink(base.join("outside/doc.designcraft"), base.join("in/leak.designcraft")).unwrap();
    std::os::unix::fs::symlink(base.join("outside/data.csv"), base.join("in/leak.csv")).unwrap();
    std::os::unix::fs::symlink(base.join("outside"), base.join("out/away")).unwrap();
    std::os::unix::fs::symlink(base.join("outside/gone.designcraft"), base.join("out/dangling.designcraft")).unwrap();
    refused(&mut server, "file.open", json!({"path": s(&base.join("in/leak.designcraft"))}), "outside the read root");
    refused(&mut server, "data.source.select", json!({"path": s(&base.join("in/leak.csv"))}), "outside the read root");
    refused(&mut server, "file.saveAs", json!({"path": s(&base.join("out/away/x.designcraft"))}), "outside the write root");
    refused(&mut server, "file.exportPdf", json!({"path": s(&base.join("out/away/x.pdf"))}), "outside the write root");
    refused(&mut server, "file.saveAs", json!({"path": s(&base.join("out/dangling.designcraft"))}), "a link that leads nowhere");
    assert!(!base.join("outside/x.designcraft").exists() && !base.join("outside/x.pdf").exists() && !base.join("outside/gone.designcraft").exists());
    assert_eq!(pages(&mut server), 1, "nothing from the file the link led to reached the session");
    let _ = std::fs::remove_dir_all(base);
}

#[test]
fn without_roots_nothing_is_confined() {
    let base = std::env::temp_dir().join(format!("dc-mcp-roots-none-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&base);
    std::fs::create_dir_all(&base).unwrap();
    designcraft_file(&base.join("a.designcraft"), 3, "Anywhere");
    let mut server = Server::new(Box::new(Headless::with_document().with_automation_roots(AutomationRoots::new(None, None).unwrap())));
    ok(&mut server, "open_document", json!({"path": s(&base.join("a.designcraft"))}));
    assert_eq!(pages(&mut server), 3);
    ok(&mut server, "save_document", json!({"path": s(&base.join("b.designcraft"))}));
    ok(&mut server, "export_png", json!({"path": s(&base.join("b.png"))}));
    assert!(base.join("b.designcraft").exists() && base.join("b.png").exists());
    let _ = std::fs::remove_dir_all(base);
}
