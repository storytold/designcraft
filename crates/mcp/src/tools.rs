//! MCP tool definitions (names, descriptions, JSON Schemas) and their implementations on top of a
//! [`Backend`].

use serde_json::{Map, Value, json};

use crate::backend::Backend;
use crate::headless::NEEDS_APP;

/// The result of `tools/call`: MCP content blocks plus the `isError` flag.
#[derive(Clone, Debug, PartialEq)]
pub struct ToolResult {
    pub content: Vec<Value>,
    pub is_error: bool,
}

impl ToolResult {
    pub fn text(t: impl Into<String>) -> Self {
        Self { content: vec![json!({"type": "text", "text": t.into()})], is_error: false }
    }
    pub fn json(v: &Value) -> Self {
        Self::text(serde_json::to_string_pretty(v).unwrap_or_default())
    }
    pub fn error(t: impl Into<String>) -> Self {
        Self { content: vec![json!({"type": "text", "text": t.into()})], is_error: true }
    }
    /// A PNG image block followed by a JSON text block with details.
    pub fn image(png_base64: String, info: &Value) -> Self {
        Self {
            content: vec![json!({"type": "image", "data": png_base64, "mimeType": "image/png"}), json!({"type": "text", "text": info.to_string()})],
            is_error: false,
        }
    }
    pub fn to_value(&self) -> Value {
        json!({"content": self.content, "isError": self.is_error})
    }
}

// ---------- schemas ----------

fn num(desc: &str) -> Value {
    json!({"type": "number", "description": desc})
}
fn int(desc: &str) -> Value {
    json!({"type": "integer", "minimum": 0, "description": desc})
}
fn string(desc: &str) -> Value {
    json!({"type": "string", "description": desc})
}
fn boolean(desc: &str) -> Value {
    json!({"type": "boolean", "description": desc})
}
fn params_schema() -> Value {
    json!({"type": "object", "description": "Command parameters (see the `params` field of list_commands)"})
}
fn mods_schema() -> Value {
    json!({
        "type": "object",
        "description": "Modifier keys held (cmd = Command on macOS / Ctrl elsewhere; alt = Option)",
        "properties": {"shift": {"type": "boolean"}, "alt": {"type": "boolean"}, "cmd": {"type": "boolean"}, "ctrl": {"type": "boolean"}},
        "additionalProperties": false
    })
}
fn obj(props: Value, required: &[&str]) -> Value {
    let mut o = json!({"type": "object", "properties": props});
    if !required.is_empty() {
        o["required"] = json!(required);
    }
    o
}
fn tool(name: &str, title: &str, desc: &str, schema: Value, read_only: bool) -> Value {
    json!({
        "name": name,
        "title": title,
        "description": desc,
        "inputSchema": schema,
        "annotations": {"title": title, "readOnlyHint": read_only, "openWorldHint": false},
    })
}

const APP_ONLY: &str = " Desktop app only (`designcraft-cli mcp --connect PORT`).";

/// All tools, in `tools/list` order.
pub fn tool_definitions() -> Vec<Value> {
    let empty = || obj(json!({}), &[]);
    let app_only = |d: &str| format!("{d}{APP_ONLY}");
    vec![
        // ----- commands -----
        tool(
            "list_commands",
            "List commands",
            "List DesignCraft commands: id, label, menu path, shortcut, parameter description and whether it can run right now. \
             Every user-visible action is a command (frame.create, text.insert, type.char, style.paragraph.apply, layout.pages.insert, \
             object.fill, transform.move, edit.undo…); run them with execute or batch.",
            obj(
                json!({
                    "filter": string("Case-insensitive substring matched against id, label and menu path (e.g. \"frame\", \"style\", \"Layout\")"),
                    "enabledOnly": boolean("Only commands that can run right now"),
                }),
                &[],
            ),
            true,
        ),
        tool(
            "execute",
            "Execute command",
            "Run any DesignCraft command by id with JSON params (ids and params from list_commands). Examples: \
             {\"command\":\"frame.create\",\"params\":{\"rect\":[36,36,576,300],\"content\":\"text\",\"text\":\"Hello\"}} → {id, story}; \
             {\"command\":\"type.char\",\"params\":{\"size\":24}}; {\"command\":\"edit.undo\"}. Coordinates are points in spread space. \
             In the desktop app the UI-only commands (view.*, window.*, help.*, edit.dynamicSpelling, and app.* except app.links, \
             which works everywhere) work too.",
            obj(json!({"command": string("Command id, e.g. frame.create"), "params": params_schema()}), &["command"]),
            false,
        ),
        tool(
            "batch",
            "Execute commands",
            "Run several commands in order, stopping at the first error. Returns each result; on failure also the failing index \
             and message (earlier commands stay applied; use edit.undo to revert them). Later steps can use earlier results: a \
             parameter string \"$N.path\" is replaced by that value of step N's result (0-based; \"$last.path\" for the previous \
             step), e.g. after frame.create → {\"id\": 8, \"story\": 9}, {\"story\": \"$0.story\"} passes 9; \"${N.path}\" \
             interpolates into longer strings. Give `commands`, or `script` text (one `command.id {json}` per line).",
            obj(
                json!({
                    "commands": {
                        "type": "array",
                        "minItems": 1,
                        "description": "Commands to run in order",
                        "items": obj(json!({"command": string("Command id"), "params": params_schema()}), &["command"]),
                    },
                    "script": string("Alternative to `commands`: lines of `command.id {json params}` (# comments allowed)"),
                }),
                &[],
            ),
            false,
        ),
        // ----- document -----
        tool(
            "inspect_document",
            "Inspect document",
            "Summary of the active document: settings, pages (index, name, bounds, margins, columns), spreads with items (id, kind, \
             bounds, fill, stroke, story), stories (id, frames, length in UTF-8 bytes, overset, preview), layers, \
             paragraph/character styles, swatches, selection, active tool.",
            empty(),
            true,
        ),
        tool(
            "get_story",
            "Get story",
            "Full text of a story plus its length, frames, paragraph count, line count and overset position (length and positions \
             are UTF-8 byte offsets, the unit text.select takes). Identify it by `story` id or by a text `frame` id (default: the \
             selection).",
            obj(json!({"story": int("Story id"), "frame": int("Text frame id")}), &[]),
            true,
        ),
        tool(
            "set_story_text",
            "Set story text",
            "Replace the whole text of a story (keeps the first paragraph's and character's formatting). Use \\n for paragraph breaks. \
             Identify the story by `story` id or by a text `frame` id.",
            obj(json!({"story": int("Story id"), "frame": int("Text frame id"), "text": string("New text")}), &["text"]),
            false,
        ),
        tool(
            "new_document",
            "New document",
            "Create a new document and make it active. Sizes are in points (72 pt = 1 in). sample:true opens the multi-page \
             magazine sample instead.",
            obj(
                json!({
                    "preset": string("Page size preset, e.g. \"Letter\", \"A4\", \"Tabloid\""),
                    "width": num("Page width (pt)"),
                    "height": num("Page height (pt)"),
                    "pages": {"type": "integer", "minimum": 1, "description": "Number of pages"},
                    "facingPages": boolean("Facing pages (spreads)"),
                    "columns": {"type": "integer", "minimum": 1, "description": "Column count"},
                    "gutter": num("Column gutter (pt)"),
                    "margins": {"description": "Uniform margin (pt) or {top, bottom, inside, outside}", "anyOf": [{"type": "number"}, {"type": "object"}]},
                    "bleed": num("Bleed on all sides (pt)"),
                    "title": string("Document title"),
                    "sample": boolean("Open the sample magazine instead"),
                }),
                &[],
            ),
            false,
        ),
        tool(
            "open_document",
            "Open document",
            "Open a .designcraft file as a new, active document.",
            obj(json!({"path": string("File path")}), &["path"]),
            false,
        ),
        tool(
            "save_document",
            "Save document",
            "Save the active document in the native .designcraft format.",
            obj(json!({"path": string("Destination (default: the document's current path)")}), &[]),
            false,
        ),
        tool(
            "place_image",
            "Place image",
            "Place an image (PNG, JPEG, WebP, GIF, TIFF) from a file `path` or `base64` data. It goes into `frame` (or the selected \
             empty/graphic frame), otherwise into a new frame at x, y (default: the page's top-left margin) with an optional width.",
            obj(
                json!({
                    "path": string("Image file path"),
                    "base64": string("Image bytes, base64 (alternative to path)"),
                    "name": string("Name for base64 images"),
                    "frame": int("Frame id to place into"),
                    "spread": int("Spread index for a new frame (default 0)"),
                    "x": num("Left of the new frame (spread pt)"),
                    "y": num("Top of the new frame (spread pt)"),
                    "width": num("Width of the new frame (pt); height follows the aspect ratio"),
                }),
                &[],
            ),
            false,
        ),
        // ----- looking -----
        tool(
            "render_page",
            "Render page",
            "Render one page of the active document (printing items only, like Preview) and return it as a PNG image. Optionally \
             also write it to `path`.",
            obj(
                json!({
                    "page": {"type": "integer", "minimum": 0, "description": "Page index, 0-based (default 0; in the desktop app default = the current page)"},
                    "scale": num("Pixels per point (default 1 = 72 ppi)"),
                    "bleed": boolean("Include the bleed area"),
                    "path": string("Also write the PNG here"),
                }),
                &[],
            ),
            true,
        ),
        tool(
            "export_png",
            "Export PNG",
            "Export one page as a PNG (or JPEG if the path ends in .jpg) file. Returns the path and pixel size.",
            obj(
                json!({"path": string("Destination file"), "page": {"type": "integer", "minimum": 0, "description": "Page index, 0-based (default 0; in the desktop app default = the current page)"}, "scale": num("Pixels per point (default 2)")}),
                &["path"],
            ),
            false,
        ),
        tool(
            "screenshot",
            "Screenshot",
            &app_only("Capture the whole app window (menus, panels, canvas) and return it as a PNG image. Optionally also keep it at `path`."),
            obj(json!({"path": string("Also write the PNG here")}), &[]),
            true,
        ),
        // ----- input -----
        tool(
            "select_tool",
            "Select tool",
            "Switch the active tool (as clicking it in the Tools panel). Ids: selection, directSelection, type, rectangleFrame, \
             ellipseFrame, polygonFrame, rectangle, ellipse, polygon, line, hand, zoom, …",
            obj(json!({"tool": string("Tool id")}), &["tool"]),
            false,
        ),
        tool(
            "pointer",
            "Pointer gesture",
            "Drive the active tool with mouse events, exactly like the mouse would. Coordinates are spread points (space \"doc\", \
             default) or, in the desktop app, screen points (space \"screen\"). Example: a text frame with the type tool: \
             {tool:\"type\", events:[{kind:\"down\",x:72,y:72},{kind:\"drag\",x:300,y:200},{kind:\"up\",x:300,y:200}]} leaves a caret \
             in the new frame, ready for type_text. Returns the selection and the active tool.",
            obj(
                json!({
                    "tool": string("Optional tool to select before the gesture"),
                    "events": {
                        "type": "array",
                        "minItems": 1,
                        "items": obj(json!({
                            "kind": {"type": "string", "enum": ["down", "drag", "up", "move", "doubleclick"]},
                            "x": num("x (pt)"),
                            "y": num("y (pt)"),
                            "space": {"type": "string", "enum": ["doc", "screen"], "description": "Coordinate space (default doc)"},
                            "mods": mods_schema(),
                        }), &["kind", "x", "y"]),
                    },
                    "mods": mods_schema(),
                }),
                &["events"],
            ),
            false,
        ),
        tool(
            "key",
            "Press key",
            "Press a key with modifiers, e.g. {key:\"Z\", cmd:true} (undo), {key:\"Escape\"}, {key:\"Enter\"}, {key:\"Left\", shift:true}. \
             In the desktop app it goes through the real keyboard path; headless it goes to the active tool (caret keys while typing), \
             then command shortcuts, then tool shortcuts.",
            obj(
                json!({
                    "key": string("Key name: A–Z, 0–9, Enter, Escape, Delete, Backspace, Tab, Space, Left/Right/Up/Down, Home, End, [ ] …"),
                    "shift": {"type": "boolean"}, "alt": {"type": "boolean"}, "cmd": {"type": "boolean"}, "ctrl": {"type": "boolean"},
                }),
                &["key"],
            ),
            false,
        ),
        tool(
            "type_text",
            "Type text",
            "Type text at the text insertion point, independently of the active tool, or into the focused dialog field in \
             the desktop app. Put a caret first with pointer (tool \"type\") or execute text.placeCaret / text.select.",
            obj(json!({"text": string("Text to type (\\n = new paragraph)")}), &["text"]),
            false,
        ),
        tool(
            "click",
            "Click",
            &app_only(
                "Click at a screen point (egui points, top-left of the window = 0,0) through the real pointer path: reaches menus, \
                 panels, flyouts and dialogs. Find coordinates with ui_inspect and screenshot.",
            ),
            obj(
                json!({
                    "x": num("Screen x"), "y": num("Screen y"),
                    "button": {"type": "string", "enum": ["left", "right"], "description": "Default left"},
                    "count": {"type": "integer", "minimum": 1, "description": "2 = double-click"},
                    "shift": {"type": "boolean"}, "alt": {"type": "boolean"}, "cmd": {"type": "boolean"}, "ctrl": {"type": "boolean"},
                }),
                &["x", "y"],
            ),
            false,
        ),
        tool(
            "drag",
            "Drag",
            &app_only("Press at (x, y), move to (toX, toY) in `steps` moves and release, in screen points through the real pointer path."),
            obj(
                json!({
                    "x": num("Start x"), "y": num("Start y"), "toX": num("End x"), "toY": num("End y"),
                    "steps": {"type": "integer", "minimum": 1, "description": "Intermediate moves (default 8)"},
                    "shift": {"type": "boolean"}, "alt": {"type": "boolean"}, "cmd": {"type": "boolean"},
                }),
                &["x", "y", "toX", "toY"],
            ),
            false,
        ),
        // ----- UI -----
        tool(
            "menu_list",
            "List menus",
            &app_only("The menu bar tree (menus, entries as cmd:/ui: command ids, separators, submenus)."),
            empty(),
            true,
        ),
        tool(
            "ui_inspect",
            "Inspect UI",
            &app_only("UI state: active tool, panels, view (zoom, origin), canvas rect in screen points, window size, documents, open dialog, perf."),
            empty(),
            true,
        ),
        tool(
            "ui_set",
            "Set UI state",
            &app_only(
                "Change UI state: panel to open, brightness, rulers/guides/frameEdges/baselineGrid/textThreads, screenMode, zoom, page (1-based), fit (page|spread).",
            ),
            obj(
                json!({
                    "panel": string("Panel: properties, pages, layers, swatches, paragraphStyles, characterStyles, stroke, character, paragraph, textWrap, links"),
                    "brightness": {"type": "string", "enum": ["dark", "mediumDark", "mediumLight", "light"]},
                    "rulers": {"type": "boolean"}, "guides": {"type": "boolean"}, "frameEdges": {"type": "boolean"},
                    "baselineGrid": {"type": "boolean"}, "textThreads": {"type": "boolean"},
                    "screenMode": {"type": "string", "enum": ["normal", "preview", "bleed", "slug", "presentation"]},
                    "zoom": num("1.0 = 100%"),
                    "page": {"type": "integer", "minimum": 1, "description": "Go to page (1-based)"},
                    "fit": {"type": "string", "enum": ["page", "spread"]},
                }),
                &[],
            ),
            false,
        ),
        tool(
            "dialog_set",
            "Set dialog field",
            &app_only("Set a field of the open dialog (see ui_inspect → dialog for its fields)."),
            obj(json!({"field": string("Field name"), "value": {"description": "New value"}}), &["field", "value"]),
            false,
        ),
        tool("dialog_confirm", "Confirm dialog", &app_only("Press OK in the open dialog."), empty(), false),
        tool("dialog_cancel", "Cancel dialog", &app_only("Press Cancel in the open dialog."), empty(), false),
    ]
}

// ---------- implementation ----------

type Args = Map<String, Value>;

fn req_str<'a>(a: &'a Args, k: &str) -> Result<&'a str, String> {
    a.get(k).and_then(Value::as_str).filter(|s| !s.is_empty()).ok_or_else(|| format!("missing string argument `{k}`"))
}

fn exec(b: &mut dyn Backend, cmd: &str, params: Value) -> Result<Value, String> {
    b.call("engine.execute", json!({"command": cmd, "params": params}))
}

fn need_ui(b: &dyn Backend, tool: &str) -> Result<(), String> {
    if b.has_ui() { Ok(()) } else { Err(format!("`{tool}` is not available headless; {NEEDS_APP}")) }
}

/// The given keys of `a` that are present, as a JSON object.
fn pick(a: &Args, keys: &[&str]) -> Value {
    Value::Object(keys.iter().filter_map(|k| a.get(*k).filter(|v| !v.is_null()).map(|v| (k.to_string(), v.clone()))).collect())
}

/// Page omission selects a default, but an explicitly supplied null must reach validation.
fn page_params(a: &Args, keys: &[&str]) -> Value {
    let mut p = pick(a, keys);
    if let Some(page) = a.get("page") {
        p["page"] = page.clone();
    }
    p
}

fn command_params(v: Option<&Value>) -> Result<Value, String> {
    match v {
        None | Some(Value::Null) => Ok(json!({})),
        Some(v @ Value::Object(_)) => Ok(v.clone()),
        Some(_) => Err("`params` must be an object".into()),
    }
}

fn filter_commands(all: Value, a: &Args) -> Value {
    let needle = a.get("filter").and_then(Value::as_str).map(str::to_lowercase).filter(|s| !s.is_empty());
    let enabled_only = a.get("enabledOnly").and_then(Value::as_bool).unwrap_or(false);
    let Value::Array(list) = all else { return all };
    let hit = |c: &Value| {
        if enabled_only && c.get("enabled").and_then(Value::as_bool) == Some(false) {
            return false;
        }
        let Some(n) = &needle else { return true };
        let field = |k: &str| c.get(k).and_then(Value::as_str).unwrap_or("").to_lowercase();
        let menu = c
            .get("menu")
            .and_then(Value::as_array)
            .map(|m| m.iter().filter_map(Value::as_str).collect::<Vec<_>>().join(" "))
            .unwrap_or_default()
            .to_lowercase();
        field("id").contains(n) || field("label").contains(n) || menu.contains(n)
    };
    Value::Array(list.into_iter().filter(hit).collect())
}

fn batch(b: &mut dyn Backend, a: &Args) -> Result<ToolResult, String> {
    use designcraft_engine::script;
    let steps = match (a.get("commands").and_then(Value::as_array), a.get("script").and_then(Value::as_str)) {
        (Some(c), _) if !c.is_empty() => script::parse(&Value::Array(c.clone()).to_string())?,
        (_, Some(t)) => script::parse(t)?,
        _ => return Err("give a non-empty `commands` array or `script` text".into()),
    };
    if steps.is_empty() {
        return Err("the script has no steps".into());
    }
    let report = script::run(&steps, |id, p| exec(b, id, p));
    let v = report.to_json();
    Ok(if report.failed.is_some() { ToolResult { is_error: true, ..ToolResult::json(&v) } } else { ToolResult::json(&v) })
}

/// The story id from `story` or the text frame `frame` (else the selection).
fn story_id(b: &mut dyn Backend, a: &Args) -> Result<Value, String> {
    if let Some(s) = a.get("story").filter(|v| v.is_u64()) {
        return Ok(s.clone());
    }
    let r = exec(b, "story.get", pick(a, &["frame"]))?;
    r.get("id").cloned().ok_or_else(|| "no story found (give `story` or a text `frame`)".into())
}

fn render_page(b: &mut dyn Backend, a: &Args) -> Result<ToolResult, String> {
    let r = b.call("ui.render", page_params(a, &["page", "scale", "bleed"]))?;
    let b64 = r.get("pngBase64").and_then(Value::as_str).ok_or("renderer returned no image")?.to_string();
    let path = a.get("path").and_then(Value::as_str);
    if let Some(path) = path {
        let png = designcraft_engine::cmd::base64_decode(&b64);
        std::fs::write(path, png).map_err(|e| format!("write {path}: {e}"))?;
    }
    Ok(ToolResult::image(b64, &json!({"width": r.get("width"), "height": r.get("height"), "path": path})))
}

fn screenshot(b: &mut dyn Backend, a: &Args) -> Result<ToolResult, String> {
    need_ui(b, "screenshot")?;
    // The app writes the capture to disk (loopback: same machine), then we read it back.
    let keep = a.get("path").and_then(Value::as_str);
    let target = match keep {
        Some(p) => p.to_string(),
        None => std::env::temp_dir()
            .join(format!("designcraft-mcp-{}-{}.png", std::process::id(), std::time::SystemTime::UNIX_EPOCH.elapsed().map_or(0, |d| d.as_millis())))
            .to_string_lossy()
            .to_string(),
    };
    let r = b.call("ui.screenshot", json!({"path": target}))?;
    let png = std::fs::read(&target).map_err(|e| format!("read {target}: {e}"))?;
    if keep.is_none() {
        std::fs::remove_file(&target).ok();
    }
    Ok(ToolResult::image(designcraft_engine::cmd::base64_encode(&png), &json!({"width": r.get("width"), "height": r.get("height"), "path": keep})))
}

fn pointer(b: &mut dyn Backend, a: &Args) -> Result<Value, String> {
    let events = a.get("events").and_then(Value::as_array).filter(|e| !e.is_empty()).ok_or("`events` must be a non-empty array")?;
    for (i, e) in events.iter().enumerate() {
        let kind = e.get("kind").and_then(Value::as_str).unwrap_or("");
        if crate::headless::pointer_kind(kind).is_none() {
            return Err(format!("event {i}: unknown kind `{kind}` (down, drag, up, move, doubleclick)"));
        }
        if !e.get("x").is_some_and(Value::is_number) || !e.get("y").is_some_and(Value::is_number) {
            return Err(format!("event {i}: numeric `x` and `y` required"));
        }
    }
    if let Some(t) = a.get("tool").and_then(Value::as_str) {
        b.call("ui.tool.select", json!({"tool": t}))?;
    }
    b.call("ui.pointer", pick(a, &["events", "mods"]))
}

fn dispatch(b: &mut dyn Backend, name: &str, a: &Args) -> Result<ToolResult, String> {
    let j = |v: Value| Ok(ToolResult::json(&v));
    match name {
        "list_commands" => {
            let all = b.call("engine.commands", json!({}))?;
            let list = filter_commands(all, a);
            j(json!({"count": list.as_array().map_or(0, Vec::len), "commands": list}))
        }
        "execute" => {
            let cmd = req_str(a, "command")?;
            let params = command_params(a.get("params"))?;
            j(exec(b, cmd, params)?)
        }
        "batch" => batch(b, a),
        "inspect_document" => j(b.call("document.inspect", json!({}))?),
        "get_story" => j(exec(b, "story.get", pick(a, &["story", "frame"]))?),
        "set_story_text" => {
            let text = a.get("text").and_then(Value::as_str).ok_or("missing string argument `text`")?.to_string();
            let sid = story_id(b, a)?;
            exec(b, "story.setText", json!({"story": sid, "text": text}))?;
            j(exec(b, "story.get", json!({"story": sid}))?)
        }
        "new_document" => {
            if a.get("sample").and_then(Value::as_bool) == Some(true) {
                return j(exec(b, "file.newSample", json!({}))?);
            }
            let mut p = a.clone();
            p.remove("sample");
            j(exec(b, "file.new", Value::Object(p))?)
        }
        "open_document" => j(b.call("app.open", json!({"path": req_str(a, "path")?}))?),
        "save_document" => j(exec(b, "file.save", pick(a, &["path"]))?),
        "place_image" => {
            if !a.contains_key("path") && !a.contains_key("base64") {
                return Err("give `path` or `base64`".into());
            }
            j(exec(b, "file.place", pick(a, &["path", "base64", "name", "frame", "spread", "x", "y", "width"]))?)
        }
        "render_page" => render_page(b, a),
        "export_png" => {
            req_str(a, "path")?;
            j(b.call("app.export", page_params(a, &["path", "page", "scale"]))?)
        }
        "screenshot" => screenshot(b, a),
        "select_tool" => j(b.call("ui.tool.select", json!({"tool": req_str(a, "tool")?}))?),
        "pointer" => j(pointer(b, a)?),
        "key" => {
            req_str(a, "key")?;
            j(b.call("ui.key", pick(a, &["key", "shift", "alt", "cmd", "ctrl"]))?)
        }
        "type_text" => j(b.call("ui.text", json!({"text": req_str(a, "text")?}))?),
        "click" | "drag" => {
            need_ui(b, name)?;
            let keys: &[&str] = &["x", "y", "toX", "toY", "steps", "button", "count", "shift", "alt", "cmd", "ctrl"];
            j(b.call(if name == "click" { "ui.click" } else { "ui.drag" }, pick(a, keys))?)
        }
        "menu_list" => {
            need_ui(b, name)?;
            j(b.call("ui.menu.list", json!({}))?)
        }
        "ui_inspect" => {
            need_ui(b, name)?;
            j(b.call("ui.inspect", json!({}))?)
        }
        "ui_set" => {
            need_ui(b, name)?;
            j(b.call("ui.set", Value::Object(a.clone()))?)
        }
        "dialog_set" => {
            need_ui(b, name)?;
            j(b.call("ui.dialog.set", json!({"field": req_str(a, "field")?, "value": a.get("value")}))?)
        }
        "dialog_confirm" => {
            need_ui(b, name)?;
            j(b.call("ui.dialog.confirm", json!({}))?)
        }
        "dialog_cancel" => {
            need_ui(b, name)?;
            j(b.call("ui.dialog.cancel", json!({}))?)
        }
        other => Err(format!("unknown tool `{other}` (see tools/list)")),
    }
}

/// Run one tool. Failures (unknown tool, bad arguments, command errors) come back as an
/// `isError` result so the model can read and correct them.
pub fn call_tool(b: &mut dyn Backend, name: &str, args: &Value) -> ToolResult {
    let empty = Map::new();
    let a = match args {
        Value::Object(o) => o,
        Value::Null => &empty,
        _ => return ToolResult::error("tool arguments must be a JSON object"),
    };
    match dispatch(b, name, a) {
        Ok(r) => r,
        Err(e) => ToolResult::error(format!("{name}: {e}")),
    }
}
