use std::io::{BufRead, BufReader, Write};

use designcraft_engine::cmd::{base64_decode, base64_encode};
use serde_json::{Value, json};

use crate::{Backend, Headless, PROTOCOL_VERSION, Remote, Server, control_addr, tool_definitions};

fn server() -> Server {
    Server::new(Box::new(Headless::with_document()))
}

fn rpc(s: &mut Server, id: u64, method: &str, params: Value) -> Value {
    let line = json!({"jsonrpc": "2.0", "id": id, "method": method, "params": params}).to_string();
    let reply = s.handle_line(&line).expect("reply");
    let v: Value = serde_json::from_str(&reply).expect("reply is JSON");
    assert_eq!(v["jsonrpc"], "2.0");
    assert_eq!(v["id"], id);
    v
}

fn call(s: &mut Server, name: &str, args: Value) -> Value {
    let v = rpc(s, 99, "tools/call", json!({"name": name, "arguments": args}));
    assert!(v.get("error").is_none(), "{v}");
    v["result"].clone()
}

/// Call a tool that must succeed; returns its JSON text payload.
fn ok(s: &mut Server, name: &str, args: Value) -> Value {
    let r = call(s, name, args);
    assert_eq!(r["isError"], false, "{name}: {r}");
    serde_json::from_str(&text_of(&r)).unwrap_or(Value::Null)
}

fn text_of(result: &Value) -> String {
    result["content"].as_array().unwrap().iter().filter(|c| c["type"] == "text").map(|c| c["text"].as_str().unwrap().to_string()).collect()
}

fn image_of(result: &Value) -> Vec<u8> {
    let img = result["content"].as_array().unwrap().iter().find(|c| c["type"] == "image").expect("image block");
    assert_eq!(img["mimeType"], "image/png");
    base64_decode(img["data"].as_str().unwrap())
}

fn tmp(name: &str) -> std::path::PathBuf {
    let dir = std::env::temp_dir().join(format!("designcraft-mcp-test-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    dir.join(name)
}

const PNG_MAGIC: &[u8] = b"\x89PNG\r\n\x1a\n";

// ---------- protocol ----------

#[test]
fn framing_basics() {
    let mut s = server();
    assert_eq!(s.handle_line("   "), None);
    assert_eq!(s.handle_line(r#"{"jsonrpc":"2.0","method":"notifications/initialized"}"#), None);
    assert!(s.is_initialized());
    let v: Value = serde_json::from_str(&s.handle_line("{not json").unwrap()).unwrap();
    assert_eq!(v["error"]["code"], -32700);
    assert_eq!(v["id"], Value::Null);
    let v: Value = serde_json::from_str(&s.handle_line(r#"{"jsonrpc":"2.0","id":"abc","method":"nope"}"#).unwrap()).unwrap();
    assert_eq!(v["error"]["code"], -32601);
    assert_eq!(v["id"], "abc");
    let v: Value = serde_json::from_str(&s.handle_line(r#"{"jsonrpc":"2.0","id":3}"#).unwrap()).unwrap();
    assert_eq!(v["error"]["code"], -32600);
    assert_eq!(s.handle_line(r#"{"jsonrpc":"2.0","id":9,"result":{}}"#), None);
    assert_eq!(rpc(&mut s, 4, "ping", json!({}))["result"], json!({}));
    let v: Value = serde_json::from_str(
        &s.handle_line(r#"[{"jsonrpc":"2.0","id":1,"method":"ping"},{"jsonrpc":"2.0","method":"notifications/initialized"}]"#).unwrap(),
    )
    .unwrap();
    assert_eq!(v.as_array().unwrap().len(), 1);
    // tools/call without a name is a protocol error, not a tool error.
    assert_eq!(rpc(&mut s, 5, "tools/call", json!({}))["error"]["code"], -32602);
}

#[test]
fn protocol_round_trip_over_serve() {
    let mut s = server();
    let input = [
        json!({"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":PROTOCOL_VERSION,"capabilities":{},"clientInfo":{"name":"t","version":"0"}}}),
        json!({"jsonrpc":"2.0","method":"notifications/initialized"}),
        json!({"jsonrpc":"2.0","id":2,"method":"tools/list"}),
        json!({"jsonrpc":"2.0","id":3,"method":"tools/call","params":{"name":"list_commands","arguments":{"filter":"frame.create"}}}),
        json!({"jsonrpc":"2.0","id":4,"method":"tools/call","params":{"name":"execute","arguments":{"command":"frame.create","params":{"rect":[36,36,300,200],"content":"text","text":"Hi"}}}}),
        json!({"jsonrpc":"2.0","id":5,"method":"resources/read","params":{"uri":"designcraft://document"}}),
    ]
    .map(|v| v.to_string())
    .join("\n");
    let mut out = Vec::new();
    s.serve(input.as_bytes(), &mut out).unwrap();
    let lines: Vec<Value> = String::from_utf8(out).unwrap().lines().map(|l| serde_json::from_str(l).unwrap()).collect();
    assert_eq!(lines.len(), 5, "one reply per request, none for the notification");
    assert_eq!(lines[0]["result"]["protocolVersion"], PROTOCOL_VERSION);
    assert_eq!(lines[0]["result"]["serverInfo"]["name"], "designcraft");
    assert!(lines[0]["result"]["capabilities"]["tools"].is_object());
    assert!(lines[0]["result"]["instructions"].as_str().unwrap().contains("headless"));
    let tools = lines[1]["result"]["tools"].as_array().unwrap();
    assert_eq!(tools.len(), tool_definitions().len());
    let listed: Value = serde_json::from_str(&text_of(&lines[2]["result"])).unwrap();
    assert_eq!(listed["count"], 1);
    assert_eq!(listed["commands"][0]["id"], "frame.create");
    let created: Value = serde_json::from_str(&text_of(&lines[3]["result"])).unwrap();
    assert!(created["id"].is_u64() && created["story"].is_u64(), "{created}");
    let doc: Value = serde_json::from_str(lines[4]["result"]["contents"][0]["text"].as_str().unwrap()).unwrap();
    assert_eq!(doc["stories"][0]["preview"], "Hi");
}

#[test]
fn initialize_negotiates_version() {
    let mut s = server();
    let v = rpc(&mut s, 1, "initialize", json!({"protocolVersion": "2024-11-05"}));
    assert_eq!(v["result"]["protocolVersion"], "2024-11-05");
    let v = rpc(&mut s, 2, "initialize", json!({"protocolVersion": "1999-01-01"}));
    assert_eq!(v["result"]["protocolVersion"], PROTOCOL_VERSION);
}

#[test]
fn tool_definitions_are_well_formed() {
    let defs = tool_definitions();
    let mut names = std::collections::HashSet::new();
    for t in &defs {
        let name = t["name"].as_str().unwrap();
        assert!(names.insert(name), "duplicate tool {name}");
        assert!(name.chars().all(|c| c.is_ascii_lowercase() || c == '_'), "{name}");
        assert!(t["description"].as_str().unwrap().len() > 20, "{name} needs a description");
        assert_eq!(t["inputSchema"]["type"], "object", "{name}");
        for r in t["inputSchema"]["required"].as_array().into_iter().flatten() {
            assert!(t["inputSchema"]["properties"].get(r.as_str().unwrap()).is_some(), "{name}: required `{r}` not in properties");
        }
    }
    for want in [
        "list_commands",
        "execute",
        "batch",
        "inspect_document",
        "get_story",
        "set_story_text",
        "new_document",
        "open_document",
        "save_document",
        "render_page",
        "export_png",
        "screenshot",
        "pointer",
        "key",
        "type_text",
        "click",
        "drag",
        "select_tool",
        "menu_list",
        "ui_inspect",
        "dialog_set",
        "dialog_confirm",
        "dialog_cancel",
        "place_image",
    ] {
        assert!(names.contains(want), "missing tool {want}");
    }
    // Every tool is dispatched (no "unknown tool" for listed names).
    let mut s = server();
    for name in names {
        let r = call(&mut s, name, json!({}));
        assert!(!text_of(&r).contains("unknown tool"), "{name} not dispatched");
    }
}

// ---------- layout through the tools ----------

#[test]
fn builds_a_small_layout_and_renders_it() {
    let mut s = server();
    let d = ok(&mut s, "new_document", json!({"width": 400, "height": 300, "margins": 24, "title": "Flyer"}));
    assert_eq!(d["index"], 1);
    let f = ok(&mut s, "execute", json!({"command": "frame.create", "params": {"rect": [24, 24, 376, 150], "content": "text"}}));
    let (frame, story) = (f["id"].as_u64().unwrap(), f["story"].as_u64().unwrap());
    let st = ok(&mut s, "set_story_text", json!({"frame": frame, "text": "Summer Fair\nSaturday in the park"}));
    assert_eq!(st["id"], story);
    assert_eq!(st["paragraphs"], 2);
    ok(&mut s, "execute", json!({"command": "selection.set", "params": {"ids": [frame]}}));
    ok(&mut s, "execute", json!({"command": "type.char", "params": {"size": 28}}));
    let got = ok(&mut s, "get_story", json!({"story": story}));
    assert_eq!(got["text"], "Summer Fair\nSaturday in the park");
    assert!(got["lines"].as_u64().unwrap() >= 2);

    let r = call(&mut s, "render_page", json!({"page": 0, "scale": 1}));
    assert_eq!(r["isError"], false, "{r}");
    let png = image_of(&r);
    assert!(png.starts_with(PNG_MAGIC));
    let info: Value = serde_json::from_str(&text_of(&r)).unwrap();
    assert_eq!((info["width"].as_u64(), info["height"].as_u64()), (Some(400), Some(300)));
    // The page isn't blank: dark text pixels on white paper.
    let img = image::load_from_memory(&png).unwrap().to_rgba8();
    assert_eq!(img.dimensions(), (400, 300));
    assert!(img.pixels().any(|p| p.0[0] < 128 && p.0[3] > 200), "no text was drawn");

    let doc = ok(&mut s, "inspect_document", json!({}));
    assert_eq!(doc["title"], "Flyer");
    assert_eq!(doc["spreads"][0]["items"][0]["id"], frame);
}

#[test]
fn type_tool_gesture_then_typing() {
    let mut s = server();
    let r = ok(
        &mut s,
        "pointer",
        json!({"tool": "type", "events": [{"kind": "down", "x": 72, "y": 72}, {"kind": "drag", "x": 300, "y": 200}, {"kind": "up", "x": 300, "y": 200}]}),
    );
    assert_eq!(r["tool"], "type");
    ok(&mut s, "type_text", json!({"text": "Hello"}));
    ok(&mut s, "key", json!({"key": "Enter"}));
    ok(&mut s, "type_text", json!({"text": "World"}));
    let doc = ok(&mut s, "inspect_document", json!({}));
    let story = doc["stories"][0]["id"].as_u64().unwrap();
    assert_eq!(ok(&mut s, "get_story", json!({"story": story}))["text"], "Hello\nWorld");
    // Escape leaves the text and selects the frame; Cmd+Z undoes through the shortcut.
    let k = ok(&mut s, "key", json!({"key": "Escape"}));
    assert_eq!(k["handledBy"], "tool");
    let k = ok(&mut s, "key", json!({"key": "Z", "cmd": true}));
    assert_eq!(k["command"], "edit.undo");
    // Tool shortcut when not typing.
    assert_eq!(ok(&mut s, "key", json!({"key": "V"}))["tool"], "selection");
}

#[test]
fn batch_stops_on_first_error() {
    let mut s = server();
    let r = call(
        &mut s,
        "batch",
        json!({"commands": [
            {"command": "frame.create", "params": {"rect": [0, 0, 100, 100]}},
            {"command": "no.such.command"},
            {"command": "frame.create", "params": {"rect": [0, 0, 50, 50]}},
        ]}),
    );
    assert_eq!(r["isError"], true);
    let v: Value = serde_json::from_str(&text_of(&r)).unwrap();
    assert_eq!(v["failedIndex"], 1);
    assert_eq!(v["completed"], 1);
    assert!(v["error"].as_str().unwrap().contains("no.such.command"));
    let doc = ok(&mut s, "inspect_document", json!({}));
    assert_eq!(doc["spreads"][0]["items"].as_array().unwrap().len(), 1, "the third command must not run");

    let v = ok(&mut s, "batch", json!({"commands": [{"command": "edit.undo"}, {"command": "document.history"}]}));
    assert_eq!(v["completed"], 2);
}

#[test]
fn ui_only_tools_explain_how_to_connect() {
    let mut s = server();
    for (name, args) in [
        ("screenshot", json!({})),
        ("click", json!({"x": 1, "y": 1})),
        ("drag", json!({"x": 1, "y": 1, "toX": 5, "toY": 5})),
        ("menu_list", json!({})),
        ("ui_inspect", json!({})),
        ("ui_set", json!({"zoom": 2})),
        ("dialog_set", json!({"field": "width", "value": 10})),
        ("dialog_confirm", json!({})),
        ("dialog_cancel", json!({})),
    ] {
        let r = call(&mut s, name, args);
        assert_eq!(r["isError"], true, "{name}");
        assert!(text_of(&r).contains("--connect"), "{name}: {}", text_of(&r));
    }
    let r = call(&mut s, "execute", json!({"command": "view.zoomIn"}));
    assert_eq!(r["isError"], true);
    assert!(text_of(&r).contains("UI command"));
    // Typing without a caret explains what to do.
    let r = call(&mut s, "type_text", json!({"text": "x"}));
    assert_eq!(r["isError"], true);
    assert!(text_of(&r).contains("insertion point"));
}

#[test]
fn files_place_and_export() {
    let mut s = server();
    // A 4×2 red PNG made by the renderer's encoder.
    let px = designcraft_render::Rendered { width: 4, height: 2, pixels: [255, 0, 0, 255].repeat(8) };
    let png = px.to_png();
    let placed = ok(&mut s, "place_image", json!({"base64": base64_encode(&png), "name": "red.png", "x": 50, "y": 60, "width": 100}));
    assert!(placed["id"].is_u64() && placed["asset"].is_u64(), "{placed}");
    let img_path = tmp("red.png");
    std::fs::write(&img_path, &png).unwrap();
    // With a graphic frame selected, Place replaces its content; deselect for a new frame.
    ok(&mut s, "execute", json!({"command": "edit.deselectAll"}));
    assert!(ok(&mut s, "place_image", json!({"path": img_path}))["id"].is_u64());
    assert_eq!(call(&mut s, "place_image", json!({}))["isError"], true);

    let out = tmp("page.png");
    let e = ok(&mut s, "export_png", json!({"path": out, "scale": 0.5}));
    assert_eq!(e["width"], 306);
    assert!(std::fs::read(&out).unwrap().starts_with(PNG_MAGIC));
    let r = call(&mut s, "render_page", json!({"path": tmp("render.png")}));
    assert_eq!(r["isError"], false);
    assert!(std::fs::read(tmp("render.png")).unwrap().starts_with(PNG_MAGIC));

    let doc_path = tmp("saved.designcraft");
    let saved = ok(&mut s, "save_document", json!({"path": doc_path}));
    assert_eq!(saved["path"], doc_path.to_string_lossy().as_ref());
    let opened = ok(&mut s, "open_document", json!({"path": doc_path}));
    assert!(opened["index"].is_u64());
    let doc = ok(&mut s, "inspect_document", json!({}));
    assert_eq!(doc["spreads"][0]["items"].as_array().unwrap().len(), 2);
    assert_eq!(doc["dirty"], false);

    let sample = ok(&mut s, "new_document", json!({"sample": true}));
    assert!(sample["index"].is_u64());
    assert!(ok(&mut s, "inspect_document", json!({}))["pageCount"].as_u64().unwrap() > 1);
    let r = call(&mut s, "render_page", json!({"page": 999}));
    assert_eq!(r["isError"], true);
}

#[test]
fn list_commands_filters() {
    let mut s = server();
    let all = ok(&mut s, "list_commands", json!({}));
    let n = all["count"].as_u64().unwrap();
    assert!(n > 50, "{n}");
    let some = ok(&mut s, "list_commands", json!({"filter": "Arrange"}));
    assert!(some["count"].as_u64().unwrap() >= 4);
    let enabled = ok(&mut s, "list_commands", json!({"enabledOnly": true}));
    assert!(enabled["count"].as_u64().unwrap() < n, "nothing is selected, so some commands are disabled");
    let r = call(&mut s, "nope", json!({}));
    assert_eq!(r["isError"], true);
    assert_eq!(call(&mut s, "execute", json!([1]))["isError"], true);
}

// ---------- connect mode ----------

/// A fake app on a loopback port answering a few control methods, recording what it got.
fn fake_app() -> (String, std::sync::mpsc::Receiver<Value>) {
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = listener.local_addr().unwrap().to_string();
    let (tx, rx) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        let Ok((stream, _)) = listener.accept() else { return };
        let mut out = stream.try_clone().unwrap();
        for line in BufReader::new(stream).lines() {
            let Ok(line) = line else { break };
            let req: Value = serde_json::from_str(&line).unwrap();
            let p = &req["params"];
            let reply = match req["method"].as_str().unwrap() {
                "ui.screenshot" => {
                    let px = designcraft_render::Rendered { width: 3, height: 3, pixels: vec![0; 36] };
                    std::fs::write(p["path"].as_str().unwrap(), px.to_png()).unwrap();
                    json!({"ok": true, "result": {"path": p["path"], "width": 3, "height": 3}})
                }
                "ui.click" => json!({"ok": true, "result": null}),
                "ui.inspect" => json!({"ok": true, "result": {"tool": "selection"}}),
                _ => json!({"ok": false, "error": "nope"}),
            };
            tx.send(req.clone()).unwrap();
            let mut reply = reply;
            reply["id"] = req["id"].clone();
            writeln!(out, "{reply}").unwrap();
        }
    });
    (addr, rx)
}

#[test]
fn connect_mode_forwards_to_the_app() {
    let (addr, seen) = fake_app();
    let remote = Remote::connect(&addr).unwrap();
    assert!(remote.has_ui());
    let mut s = Server::new(Box::new(remote));
    let r = call(&mut s, "screenshot", json!({}));
    assert_eq!(r["isError"], false, "{r}");
    assert!(image_of(&r).starts_with(PNG_MAGIC));
    let got = seen.recv().unwrap();
    assert_eq!(got["method"], "ui.screenshot");
    assert!(!std::path::Path::new(got["params"]["path"].as_str().unwrap()).exists(), "temp screenshot is removed");

    ok(&mut s, "click", json!({"x": 10, "y": 20, "count": 2}));
    let got = seen.recv().unwrap();
    assert_eq!((got["method"].as_str(), got["params"]["x"].as_i64(), got["params"]["count"].as_i64()), (Some("ui.click"), Some(10), Some(2)));
    assert_eq!(ok(&mut s, "ui_inspect", json!({}))["tool"], "selection");
    let r = call(&mut s, "menu_list", json!({}));
    assert_eq!(r["isError"], true);
    assert!(text_of(&r).contains("nope"), "app errors are passed through");
}

#[test]
fn control_addr_accepts_port_or_address() {
    assert_eq!(control_addr("7979"), "127.0.0.1:7979");
    assert_eq!(control_addr("localhost:8000"), "localhost:8000");
    assert!(Remote::connect("127.0.0.1:1").is_err());
}

#[test]
fn batch_steps_use_earlier_results() {
    let mut s = server();
    let v = ok(
        &mut s,
        "batch",
        json!({"commands": [
            {"command": "frame.create", "params": {"rect": [72, 72, 300, 200], "content": "text", "text": "Hello"}},
            {"command": "text.select", "params": {"story": "$0.story", "anchor": 5, "focus": 5}},
            {"command": "footnote.insert", "params": {"text": "A note for frame ${0.id}."}},
            {"command": "footnote.list", "params": {"story": "$0.story"}},
        ]}),
    );
    assert_eq!(v["completed"], 4);
    let id = v["results"][0]["id"].as_u64().unwrap();
    assert_eq!(v["results"][3][0]["text"], format!("A note for frame {id}."));
    // The same as script text.
    let v = ok(
        &mut s,
        "batch",
        json!({"script": "# one per line\nframe.create {\"rect\": [0, 0, 50, 50]}\nobject.rename {\"ids\": [\"$0.id\"], \"name\": \"Box\"}"}),
    );
    assert_eq!(v["completed"], 2);
}

#[test]
fn conventions_core_tools_and_annotations() {
    let mut s = server();
    let defs = tool_definitions();
    for name in
        ["command_list", "command_run", "command_batch", "doc_inspect", "render_preview", "ui_screenshot", "list_commands", "execute", "batch"]
    {
        assert!(defs.iter().any(|t| t["name"] == name), "missing {name}");
    }
    for t in &defs {
        for hint in ["readOnlyHint", "destructiveHint", "idempotentHint", "openWorldHint"] {
            assert!(t["annotations"][hint].is_boolean(), "{}: {hint}", t["name"]);
        }
    }
    for name in ["save_document", "export_png", "render_page", "screenshot"] {
        let t = defs.iter().find(|t| t["name"] == name).unwrap();
        assert_eq!(t["annotations"]["readOnlyHint"], false, "{name} can write a file");
        assert_eq!(t["annotations"]["destructiveHint"], false);
        assert_eq!(t["annotations"]["idempotentHint"], true);
    }
    assert!(ok(&mut s, "command_list", json!({"filter":"frame", "enabled_only":true})).as_array().unwrap().iter().all(|c| c["enabled"] == true));
    assert_eq!(ok(&mut s, "doc_inspect", json!({})), ok(&mut s, "inspect_document", json!({})));
    assert_eq!(call(&mut s, "command_run", json!({"id":"no.such.command"}))["isError"], true);
    for stop in [true, false] {
        let r = call(
            &mut s,
            "command_batch",
            json!({"steps":[{"id":"document.history"},{"id":"no.such.command"},{"id":"document.history"}],"stop_on_error":stop}),
        );
        assert_eq!(r["isError"], true);
        let v: Value = serde_json::from_str(&text_of(&r)).unwrap();
        assert_eq!(v["completed"], if stop { 1 } else { 2 });
        assert_eq!(v["failed"], 1);
        assert_eq!(v["results"][1]["ok"], false);
    }
    let before = ok(&mut s, "doc_inspect", json!({}));
    let r = call(&mut s, "render_preview", json!({"max_side":64}));
    assert_eq!(r["isError"], false, "{r}");
    let img = image::load_from_memory(&image_of(&r)).unwrap();
    assert!(img.width().max(img.height()) <= 64);
    assert_eq!(before, ok(&mut s, "doc_inspect", json!({})));
}

#[test]
fn conventions_unknown_arguments_are_protocol_errors() {
    let mut s = server();
    for t in tool_definitions() {
        let r = rpc(&mut s, 1, "tools/call", json!({"name":t["name"],"arguments":{"bogus_arg":1}}));
        assert_eq!(r["error"]["code"], -32602, "{r}");
        assert!(r["error"]["message"].as_str().unwrap().contains("bogus_arg"));
        assert!(r["error"]["message"].as_str().unwrap().contains("expected:"));
    }
    // Engine params remain open: the catalog describes them as prose.
    assert_eq!(call(&mut s, "command_run", json!({"id":"document.history","params":{"future_param":true}}))["isError"], false);
}

#[test]
fn conventions_panicking_backend_keeps_serving() {
    struct Once(bool);
    impl Backend for Once {
        fn call(&mut self, _: &str, _: Value) -> Result<Value, String> {
            if !self.0 {
                self.0 = true;
                panic!("synthetic backend failure");
            }
            Ok(json!({"recovered":true}))
        }
        fn has_ui(&self) -> bool {
            false
        }
        fn describe(&self) -> String {
            "test".into()
        }
    }
    let mut s = Server::new(Box::new(Once(false)));
    let r = call(&mut s, "inspect_document", json!({}));
    assert_eq!(r["isError"], true);
    assert!(text_of(&r).contains("synthetic backend failure"));
    assert_eq!(ok(&mut s, "inspect_document", json!({}))["recovered"], true);
}

#[test]
fn conventions_modern_results_preserve_legacy_shape() {
    let mut s = server();
    for (method, params) in [
        ("tools/list", json!({})),
        ("resources/list", json!({})),
        ("resources/templates/list", json!({})),
        ("resources/read", json!({"uri":"designcraft://document"})),
        ("resources/read", json!({"uri":"designcraft://commands"})),
    ] {
        let legacy = rpc(&mut s, 1, method, params.clone());
        assert!(legacy["result"].get("resultType").is_none());
        let mut modern = params;
        modern["_meta"] = json!({"io.modelcontextprotocol/protocolVersion":"2026-07-28"});
        let r = rpc(&mut s, 2, method, modern);
        assert_eq!(r["result"]["resultType"], "complete", "{r}");
        assert_eq!(r["result"]["cacheScope"], "private");
        assert_eq!(r["result"]["ttlMs"], if method == "resources/read" { 0 } else { 600_000 });
        let mut stripped = r["result"].clone();
        for key in ["resultType", "cacheScope", "ttlMs"] {
            stripped.as_object_mut().unwrap().remove(key);
        }
        assert_eq!(stripped, legacy["result"]);
    }
    let r = rpc(&mut s, 3, "initialize", json!({"protocolVersion":"2026-07-28"}));
    assert_eq!(r["result"]["protocolVersion"], "2026-07-28");
    assert_eq!(rpc(&mut s, 4, "tools/list", json!({}))["result"]["resultType"], "complete");
    assert!(s.handle(json!({"jsonrpc":"2.0","method":"notifications/cancelled","params":{"requestId":999}})).is_none());
    let r = rpc(&mut s, 5, "tools/call", json!({"name":"doc_inspect","arguments":{},"_meta":{"progressToken":"quick"}}));
    assert_eq!(r["result"]["isError"], false);
    let document = ok(&mut s, "doc_inspect", json!({}));
    let resource = rpc(&mut s, 6, "resources/read", json!({"uri":"designcraft://document"}));
    let text: Value = serde_json::from_str(resource["result"]["contents"][0]["text"].as_str().unwrap()).unwrap();
    assert_eq!(text, document);
}
