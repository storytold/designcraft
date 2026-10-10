//! An in-process engine session that answers the engine-level control-channel methods itself.

use designcraft_engine::{Session, ViewInfo};
use designcraft_geom::Point;
use designcraft_render::{RenderOptions, Renderer};
use designcraft_tools::{Mods, PointerEvent, PointerKind, TOOL_GROUPS, ToolKey};
use serde_json::{Value, json};

use crate::backend::Backend;

/// Headless backend: a [`Session`] plus a CPU [`Renderer`] for page renders and PNG export.
pub struct Headless {
    pub session: Session,
    pub renderer: Renderer,
}

impl Default for Headless {
    fn default() -> Self {
        Self::new()
    }
}

/// Hint appended to errors for methods that need a window.
pub(crate) const NEEDS_APP: &str = "it needs the desktop app: start `designcraft --control 7979` and run the MCP server \
with `designcraft-cli mcp --connect 7979`";

const VIEW: ViewInfo = ViewInfo::at_zoom(1.0);

fn s<'a>(p: &'a Value, k: &str) -> Option<&'a str> {
    p.get(k).and_then(Value::as_str)
}

pub(crate) fn pointer_kind(k: &str) -> Option<PointerKind> {
    Some(match k {
        "down" => PointerKind::Down,
        "drag" => PointerKind::Drag,
        "up" => PointerKind::Up,
        "move" => PointerKind::Move,
        "doubleclick" | "dblclick" => PointerKind::DoubleClick,
        _ => return None,
    })
}

fn tool_key(name: &str) -> Option<ToolKey> {
    Some(match name.to_ascii_lowercase().as_str() {
        "enter" | "return" => ToolKey::Enter,
        "esc" | "escape" => ToolKey::Escape,
        "backspace" => ToolKey::Backspace,
        "delete" => ToolKey::Delete,
        "up" | "arrowup" => ToolKey::Up,
        "down" | "arrowdown" => ToolKey::Down,
        "left" | "arrowleft" => ToolKey::Left,
        "right" | "arrowright" => ToolKey::Right,
        "home" => ToolKey::Home,
        "end" => ToolKey::End,
        "pageup" => ToolKey::PageUp,
        "pagedown" => ToolKey::PageDown,
        "tab" => ToolKey::Tab,
        _ => return None,
    })
}

/// Does a shortcut like `Cmd+Shift+]` match `key` + `mods`?
fn shortcut_matches(shortcut: &str, key: &str, mods: Mods) -> bool {
    let parts: Vec<&str> = shortcut.split('+').collect();
    // "Cmd+=": the last part is the key even if it's punctuation; "Cmd++" would split oddly but
    // no shortcut uses it.
    let Some((k, ms)) = parts.split_last() else { return false };
    let has = |m: &str| ms.iter().any(|x| x.eq_ignore_ascii_case(m));
    k.eq_ignore_ascii_case(key)
        && has("cmd") == mods.cmd
        && has("shift") == mods.shift
        && (has("alt") || has("option")) == mods.alt
        && has("ctrl") == mods.ctrl
}

fn mods_from(p: &Value) -> Mods {
    let b = |n: &str| p.get(n).and_then(Value::as_bool).unwrap_or(false);
    Mods { shift: b("shift"), alt: b("alt"), cmd: b("cmd"), ctrl: b("ctrl"), space: false }
}

impl Headless {
    /// A session with no document (the first `file.new` / `file.open` creates one).
    pub fn new() -> Self {
        Self { session: Session::new(), renderer: Renderer::new() }
    }

    /// A session with a fresh default (Letter) document, ready to lay out.
    pub fn with_document() -> Self {
        let mut h = Self::new();
        if let Err(e) = h.session.execute("file.new", &json!({})) {
            log::error!("file.new failed: {e}");
        }
        h
    }

    fn exec(&mut self, id: &str, params: &Value) -> Result<Value, String> {
        let r = self.session.execute(id, params).map_err(|e| {
            let ui_only = ["app.", "view.", "window."].iter().any(|p| id.starts_with(p));
            if ui_only && designcraft_engine::find_command(id).is_none() { format!("`{id}` is a UI command; {NEEDS_APP}") } else { e.to_string() }
        });
        // Requests for a UI (file pickers, dialogs) have nobody to serve them here.
        self.session.ui_requests.clear();
        r
    }

    fn selection(&self) -> Value {
        self.session.active().map(|d| serde_json::to_value(&d.selection).unwrap_or_default()).unwrap_or(Value::Null)
    }

    fn pointer(&mut self, p: &Value) -> Result<Value, String> {
        let events = p.get("events").and_then(Value::as_array).ok_or("missing `events`")?;
        let base: Mods = p.get("mods").and_then(|m| serde_json::from_value(m.clone()).ok()).unwrap_or_default();
        if self.session.active().is_none() {
            return Err("no document open (use new_document first)".into());
        }
        for e in events {
            let kind = s(e, "kind").unwrap_or("");
            let kind = pointer_kind(kind).ok_or_else(|| format!("unknown pointer kind `{kind}` (down|drag|up|move|doubleclick)"))?;
            if s(e, "space") == Some("screen") {
                return Err(format!("screen-space pointer events: {NEEDS_APP}. Use spread coordinates instead"));
            }
            let x = e.get("x").and_then(Value::as_f64).ok_or("pointer event needs numeric `x`")?;
            let y = e.get("y").and_then(Value::as_f64).ok_or("pointer event needs numeric `y`")?;
            let mods = e.get("mods").and_then(|m| serde_json::from_value(m.clone()).ok()).unwrap_or(base);
            self.session.pointer(&PointerEvent { kind, pos: Point::new(x, y), mods }, VIEW).map_err(|e| e.to_string())?;
        }
        let requests: Vec<Value> = self.session.ui_requests.drain(..).map(|r| serde_json::to_value(r).unwrap_or_default()).collect();
        Ok(json!({"selection": self.selection(), "tool": self.session.tool_id(), "requests": requests}))
    }

    /// Keyboard input: the active tool (caret movement, Enter/Tab/Backspace while typing), then
    /// command shortcuts, then tool shortcuts, then plain text while typing.
    fn key(&mut self, p: &Value) -> Result<Value, String> {
        let key = s(p, "key").ok_or("missing `key`")?;
        let mods = mods_from(p);
        let typing = self.session.wants_text();
        let tk = tool_key(key);
        if let Some(k) = tk
            && (typing || self.session.tool_busy())
            && self.session.tool_key(k, mods, VIEW).map_err(|e| e.to_string())?
        {
            self.session.ui_requests.clear();
            return Ok(json!({"handledBy": "tool", "tool": self.session.tool_id(), "selection": self.selection()}));
        }
        // While typing, plain keys are text, not shortcuts.
        let plain = !mods.cmd && !mods.ctrl && !mods.alt;
        if !(typing && plain)
            && let Some(c) = designcraft_engine::command_specs().iter().find(|c| c.shortcut.is_some_and(|sc| shortcut_matches(sc, key, mods)))
        {
            let r = self.exec(c.id, &json!({}))?;
            return Ok(json!({"handledBy": "command", "command": c.id, "result": r}));
        }
        if !typing && let Some(t) = TOOL_GROUPS.iter().flat_map(|g| g.iter()).find(|t| t.shortcut.is_some_and(|sc| shortcut_matches(sc, key, mods))) {
            self.session.set_tool(t.id);
            return Ok(json!({"handledBy": "tool.select", "tool": t.id}));
        }
        if let Some(k) = tk
            && self.session.tool_key(k, mods, VIEW).map_err(|e| e.to_string())?
        {
            self.session.ui_requests.clear();
            return Ok(json!({"handledBy": "tool", "tool": self.session.tool_id(), "selection": self.selection()}));
        }
        if typing && plain {
            let text = s(p, "text").map(str::to_string).or_else(|| {
                let mut c = key.chars();
                match (c.next(), c.next()) {
                    (Some(ch), None) => Some(if mods.shift { ch.to_uppercase().collect() } else { ch.to_lowercase().collect() }),
                    _ if key.eq_ignore_ascii_case("space") => Some(" ".into()),
                    _ => None,
                }
            });
            if let Some(t) = text {
                self.exec("text.insert", &json!({"text": t}))?;
                return Ok(json!({"handledBy": "text", "text": t}));
            }
        }
        Err(format!("key `{key}` does nothing here (no tool, command or tool shortcut bound to it)"))
    }

    fn text(&mut self, p: &Value) -> Result<Value, String> {
        let t = s(p, "text").ok_or("missing `text`")?;
        if self.session.active().is_none_or(|st| st.selection.text.is_none_or(|t| st.doc.text_story(t.story, t.cell).is_none())) {
            return Err("no text insertion point: click into a text frame with the type tool first (pointer with tool \"type\"), \
                or select text with execute text.select, or use set_story_text"
                .into());
        }
        self.exec("text.insert", &json!({"text": t}))?;
        Ok(json!({"inserted": t.chars().count()}))
    }

    fn render(&mut self, p: &Value) -> Result<Value, String> {
        let st = self.session.active().ok_or("no document open")?;
        let page = page_index(p, st.doc.page_count())?;
        let scale = p.get("scale").and_then(Value::as_f64).unwrap_or(1.0).clamp(0.05, 16.0);
        let bleed = p.get("bleed").and_then(Value::as_bool).unwrap_or(false);
        let img = self
            .renderer
            .render_page(&st.doc, &self.session.cache, page, scale, bleed, &RenderOptions { printing_only: true, ..Default::default() })
            .ok_or_else(|| format!("no page {page} (the document has {} pages, indices are 0-based)", st.doc.page_count()))?;
        let png = img.to_png();
        match s(p, "path") {
            Some(path) => {
                std::fs::write(path, &png).map_err(|e| format!("write {path}: {e}"))?;
                Ok(json!({"path": path, "width": img.width, "height": img.height}))
            }
            None => Ok(json!({"width": img.width, "height": img.height, "pngBase64": designcraft_engine::cmd::base64_encode(&png)})),
        }
    }

    /// `app.export {path, page?, scale?}`: PNG (or JPEG by extension) of one page, like the
    /// app's Export Page as PNG.
    fn export(&mut self, p: &Value) -> Result<Value, String> {
        let path = s(p, "path").ok_or("missing `path`")?;
        let st = self.session.active().ok_or("no document open")?;
        let page = page_index(p, st.doc.page_count())?;
        let scale = p.get("scale").and_then(Value::as_f64).unwrap_or(2.0).clamp(0.05, 16.0);
        let img = self
            .renderer
            .render_page(&st.doc, &self.session.cache, page, scale, false, &RenderOptions { printing_only: true, ..Default::default() })
            .ok_or_else(|| format!("no page {page}"))?;
        let lower = path.to_ascii_lowercase();
        let bytes = if lower.ends_with(".jpg") || lower.ends_with(".jpeg") { img.to_jpeg(90) } else { img.to_png() };
        std::fs::write(path, &bytes).map_err(|e| format!("write {path}: {e}"))?;
        Ok(json!({"path": path, "width": img.width, "height": img.height}))
    }
}

impl Backend for Headless {
    fn call(&mut self, method: &str, params: Value) -> Result<Value, String> {
        let p = &params;
        match method {
            "engine.execute" | "ui.menu.invoke" | "command" => {
                let id = s(p, "command").or(s(p, "id")).ok_or("missing `command`")?.to_string();
                let params = match p.get("params") {
                    None | Some(Value::Null) => json!({}),
                    Some(v) => v.clone(),
                };
                self.exec(&id, &params)
            }
            "engine.commands" => Ok(serde_json::to_value(self.session.commands()).unwrap_or_default()),
            "document.inspect" => self.exec("document.inspect", &json!({})),
            "ui.tool.select" => self.exec("tool.select", &json!({"tool": s(p, "tool").unwrap_or("")})),
            "ui.tool.list" => Ok(serde_json::to_value(TOOL_GROUPS).unwrap_or_default()),
            "ui.pointer" => self.pointer(p),
            "ui.key" => self.key(p),
            "ui.text" => self.text(p),
            "ui.render" => self.render(p),
            "app.open" => self.exec("file.open", &json!({"path": s(p, "path")})),
            "app.save" => self.exec("file.save", &json!({"path": s(p, "path")})),
            "app.export" => self.export(p),
            "app.quit" => Ok(Value::Null),
            m if m.starts_with("ui.") => Err(format!("`{m}` is not available headless; {NEEDS_APP}")),
            other => Err(format!("unknown method `{other}`")),
        }
    }

    fn has_ui(&self) -> bool {
        false
    }

    fn describe(&self) -> String {
        "headless (in-process engine, no window)".into()
    }
}

/// A supplied page must be a representable unsigned index; only omission selects page zero.
fn page_index(p: &Value, count: usize) -> Result<usize, String> {
    let page = match p.get("page") {
        None => 0,
        Some(v) => v.as_u64().and_then(|n| usize::try_from(n).ok()).ok_or("`page` must be a nonnegative integer representable as a page index")?,
    };
    if page >= count {
        return Err(format!("no page {page} (the document has {count} pages, indices are 0-based)"));
    }
    Ok(page)
}
