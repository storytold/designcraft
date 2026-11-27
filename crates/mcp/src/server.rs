//! JSON-RPC 2.0 framing and the MCP lifecycle / tools / resources methods, backed by
//! `craft-mcp-server`.

use std::io::{BufRead, Read, Write};

pub use craft_mcp_server::PROTOCOL_VERSION;
use craft_mcp_server::{Contents, Incoming, Info, Options, Tools};
use serde_json::{Value, json};

use crate::backend::Backend;
use crate::tools::{call_tool, tool_definitions};

const INVALID_PARAMS: i64 = -32602;

/// Most messages in one JSON-RPC batch.
const MAX_BATCH: usize = 1000;

/// Most bytes one request line may hold, including its terminator when present.
/// Keep the legacy EOF allowance without relaxing the raw-byte limit.
const MAX_LINE_BYTES: usize = 4 << 20;

/// Count raw bytes without duplicating the shared framing or buffering implementation.
struct CountedInput<R> {
    inner: R,
    consumed: usize,
}

impl<R: BufRead> Read for CountedInput<R> {
    fn read(&mut self, output: &mut [u8]) -> std::io::Result<usize> {
        let count = self.inner.read(output)?;
        self.consumed = self.consumed.saturating_add(count);
        Ok(count)
    }
}

impl<R: BufRead> BufRead for CountedInput<R> {
    fn fill_buf(&mut self) -> std::io::Result<&[u8]> {
        self.inner.fill_buf()
    }

    fn consume(&mut self, count: usize) {
        self.inner.consume(count);
        self.consumed = self.consumed.saturating_add(count);
    }
}

const INSTRUCTIONS: &str = "DesignCraft is a page-layout app (an InDesign clone). Coordinates are points in spread \
space (y down; on a single-page spread the page's top-left is (0,0); a default Letter page is 612×792). Every action \
is a command: find ids and parameters with list_commands and run them with execute (or several with batch). Typical \
flow: new_document → execute frame.create {rect:[x0,y0,x1,y1], content:\"text\", text:\"…\"} → set_story_text / \
execute type.char / style.paragraph.apply → render_page to look at the result. inspect_document lists pages, items \
(ids, bounds), stories (overset) and styles. place_image puts a picture on the page.";

/// Resource URIs.
pub const DOC_URI: &str = "designcraft://document";
pub const COMMANDS_URI: &str = "designcraft://commands";

struct DesignCraftTools {
    backend: Box<dyn Backend>,
}

impl Tools for DesignCraftTools {
    fn instructions(&self) -> String {
        format!(
            "{INSTRUCTIONS} Backend: {}. Community: {} (Discord), {} (app page), {} (source). The `app.links` command returns these links.",
            self.backend.describe(),
            designcraft_engine::links::DISCORD,
            designcraft_engine::links::APP_PAGE,
            designcraft_engine::links::GITHUB
        )
    }

    fn list(&mut self) -> Vec<Value> {
        tool_definitions()
    }

    fn call(&mut self, name: &str, args: &Value) -> Value {
        call_tool(self.backend.as_mut(), name, args).to_value()
    }

    fn resources(&self) -> Vec<Value> {
        vec![
            json!({"uri": DOC_URI, "name": "document", "title": "Active document", "description": "Pages, spreads, items, stories, styles, swatches and selection of the active document (document.inspect)", "mimeType": "application/json"}),
            json!({"uri": COMMANDS_URI, "name": "commands", "title": "Commands", "description": "Every command: id, label, menu path, shortcut, params, enabled (engine.commands)", "mimeType": "application/json"}),
        ]
    }

    fn read_resource(&mut self, uri: &str) -> Option<Result<Contents, String>> {
        let method = match uri {
            DOC_URI => "document.inspect",
            COMMANDS_URI => "engine.commands",
            _ => return None,
        };
        Some(
            self.backend
                .call(method, json!({}))
                .map(|v| Contents::Text { mime_type: "application/json".into(), text: serde_json::to_string_pretty(&v).unwrap_or_default() }),
        )
    }
}

/// An MCP server bound to one backend.
pub struct Server(craft_mcp_server::Server<DesignCraftTools>);

impl Server {
    pub fn new(backend: Box<dyn Backend>) -> Self {
        let info = Info { name: "designcraft", title: "DesignCraft", version: env!("CARGO_PKG_VERSION"), max_line: MAX_LINE_BYTES };
        let options =
            Options { website_url: Some(designcraft_engine::links::APP_PAGE.to_string()), tool_errors_as_results: true, strict_request_ids: true };
        Self(craft_mcp_server::Server::new(info, DesignCraftTools { backend }).with_options(options))
    }

    pub fn backend(&mut self) -> &mut dyn Backend {
        self.0.tools().backend.as_mut()
    }

    /// Whether the client has sent `notifications/initialized`.
    pub fn is_initialized(&self) -> bool {
        self.0.is_initialized()
    }

    /// Serve newline-delimited JSON-RPC until `input` closes. Logs go to stderr only (stdout is
    /// the protocol stream).
    pub fn serve(&mut self, input: impl BufRead, mut output: impl Write) -> std::io::Result<()> {
        let mut input = CountedInput { inner: input, consumed: 0 };
        while let Some(incoming) = craft_mcp_server::read_message(&mut input, MAX_LINE_BYTES)? {
            // The shared limit excludes LF; DesignCraft's historical limit includes it.
            let incoming = if input.consumed > MAX_LINE_BYTES {
                Incoming::Refused(json!({"jsonrpc":"2.0", "id":null, "error":{"code":-32600,"message":"request line is too long"}}).to_string())
            } else {
                incoming
            };
            input.consumed = 0;
            let reply = match incoming {
                Incoming::Line(line) => self.handle_line(&line),
                Incoming::Refused(reply) => Some(reply),
            };
            if let Some(reply) = reply {
                output.write_all(reply.as_bytes())?;
                output.write_all(b"\n")?;
                output.flush()?;
            }
        }
        Ok(())
    }

    /// Handle one line; returns the reply line (None for notifications and blank lines).
    pub fn handle_line(&mut self, line: &str) -> Option<String> {
        if line.len() > MAX_LINE_BYTES {
            return self.0.handle_line(line);
        }
        let trimmed = line.trim();
        if trimmed.is_empty() {
            return None;
        }
        match serde_json::from_str::<Value>(trimmed) {
            Ok(Value::Array(batch)) if !batch.is_empty() && batch.len() <= MAX_BATCH => {
                let replies: Vec<Value> = batch.into_iter().filter_map(|m| self.handle(m)).collect();
                (!replies.is_empty()).then_some(Value::Array(replies).to_string())
            }
            Ok(Value::Array(_)) | Err(_) => self.0.handle_line(line),
            Ok(msg) => self.handle(msg).map(|r| r.to_string()),
        }
    }

    /// Handle one JSON-RPC message; `None` for notifications and responses.
    pub fn handle(&mut self, msg: Value) -> Option<Value> {
        if let Value::Object(o) = &msg
            && o.get("method").and_then(Value::as_str) == Some("tools/call")
            && let Some(id) = o.get("id")
            && (id.is_string() || id.is_number())
        {
            let params = o.get("params").unwrap_or(&Value::Null);
            if let Some(name) = params.get("name").and_then(Value::as_str) {
                let args = params.get("arguments").unwrap_or(&Value::Null);
                if let Err(m) = check_arguments(name, args) {
                    return Some(json!({"jsonrpc": "2.0", "id": id, "error": {"code": INVALID_PARAMS, "message": m}}));
                }
            }
        }
        if msg.get("method").and_then(Value::as_str) == Some("resources/read")
            && let Some(id) = msg.get("id").filter(|id| id.is_string() || id.is_number())
            && msg.get("params").and_then(|params| params.get("uri")).and_then(Value::as_str).is_none()
        {
            return Some(json!({"jsonrpc":"2.0", "id":id, "error":{"code":INVALID_PARAMS,"message":"missing `uri`"}}));
        }
        self.0.handle(msg)
    }
}

/// Arguments a tool doesn't take, and command params a command doesn't take, are invalid params
/// (#776): a misspelt key is refused instead of being ignored. Nested objects with listed
/// properties (a `batch` step, a `pointer` event, `mods`) are checked the same way. `execute` and
/// `batch` are checked against the params descriptions of the commands they run, every step
/// before the first runs.
fn check_arguments(name: &str, args: &Value) -> Result<(), String> {
    let Some(arguments) = args.as_object() else { return Ok(()) };
    if let Some(def) = tool_definitions().into_iter().find(|t| t.get("name").and_then(Value::as_str) == Some(name))
        && let Some(schema) = def.get("inputSchema")
        && let Some(key) = unknown_key(schema, args, "", 0)
    {
        return Err(format!("unknown argument `{key}` for `{name}`"));
    }
    let command = |step: &Value| -> Result<(), String> {
        match (step.get("command").and_then(Value::as_str), step.get("params")) {
            (Some(id), Some(params)) => designcraft_engine::check_params(id, params).map_err(|e| e.to_string()),
            _ => Ok(()),
        }
    };
    match name {
        "execute" => command(args),
        "batch" => {
            let steps = match (arguments.get("commands").and_then(Value::as_array), arguments.get("script").and_then(Value::as_str)) {
                (Some(steps), _) => steps.clone(),
                // A script that doesn't parse is the tool's to report.
                (None, Some(text)) => match designcraft_engine::script::parse(text) {
                    Ok(steps) => steps.into_iter().map(|s| json!({"command": s.command, "params": s.params})).collect(),
                    Err(_) => return Ok(()),
                },
                (None, None) => return Ok(()),
            };
            for (i, step) in steps.iter().enumerate() {
                command(step).map_err(|e| format!("step {i}: {e}"))?;
            }
            Ok(())
        }
        _ => Ok(()),
    }
}

/// The path of the first key in `value` that `schema` doesn't list (`commands[0].parms`). Objects
/// without listed properties (command `params`, `dialog_set`'s `value`) take any key; their
/// contents are checked elsewhere or by the receiver.
fn unknown_key(schema: &Value, value: &Value, at: &str, depth: usize) -> Option<String> {
    // The tool schemas are a few levels deep; this only bounds hostile nesting in arguments.
    if depth > 8 {
        return None;
    }
    match value {
        Value::Object(o) => {
            let properties = schema.get("properties").and_then(Value::as_object)?;
            for (k, v) in o {
                let path = if at.is_empty() { k.clone() } else { format!("{at}.{k}") };
                let Some(sub) = properties.get(k) else { return Some(path) };
                if let Some(bad) = unknown_key(sub, v, &path, depth + 1) {
                    return Some(bad);
                }
            }
            None
        }
        Value::Array(items) => {
            let sub = schema.get("items")?;
            items.iter().enumerate().find_map(|(i, v)| unknown_key(sub, v, &format!("{at}[{i}]"), depth + 1))
        }
        _ => None,
    }
}
