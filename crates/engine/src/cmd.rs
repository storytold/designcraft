//! The command registry. Ids follow InDesign's menu structure.

mod anchored;
pub mod anchors;
mod articles;
pub mod book;
mod buttons;
pub(crate) mod captions;
pub(crate) mod changes;
mod color_theme;
mod colorsettings;
mod conditions;
mod datamerge;
mod edit;
mod editorial;
mod endnotes;
mod export;
pub use designcraft_pdf::ExportReport as PdfExportReport;
pub use export::export_pdf_document;
pub(crate) use export::print_pdf;
mod file;
mod file_info;
pub(crate) mod find;
pub use edit::copied_objects_text;
pub use find::same_find_domain;
pub(crate) mod flex;
mod fonts;
pub(crate) mod guides;
mod index;
mod inspect;
mod interactive;
pub mod interchange;
mod layout;
pub mod library;
mod linked;
mod links;
mod liquid;
mod lists;
mod load_styles;
mod mathexpr;
mod media;
mod notes;
mod object;
mod overrides;
mod package;
pub mod params;
mod paste_svg;
mod path_type;
mod paths;
mod pdflayers;
mod place_text;
pub mod preflight;
mod prefs;
mod prepress;
mod print;
mod qr;
mod rulers;
mod scripts;
mod select;
pub mod spelling;
mod states;
mod strokes;
mod style;
pub mod table;
pub mod tabs;
pub mod text;
mod thread;
mod toc;
mod transitions;
mod variables;
mod xml;
mod xref;

use designcraft_doc::{ItemId, SpreadRef};
use designcraft_geom::{Point, Rect};
use serde::Serialize;
use serde_json::Value;

use crate::{EngineError, Result, Session};

pub type Run = fn(&mut Session, &Value) -> Result<Value>;
pub type Enabled = fn(&Session) -> std::result::Result<(), String>;

pub struct CommandSpec {
    pub id: &'static str,
    pub label: &'static str,
    /// Menu placement, e.g. `["Object", "Arrange"]`. Empty = not in menus.
    pub menu: &'static [&'static str],
    pub shortcut: Option<&'static str>,
    pub params: &'static str,
    pub enabled: Enabled,
    pub run: Run,
    pub journal: bool,
    /// Record an undo step when the document changes.
    pub undoable: bool,
}

#[derive(Clone, Debug, Serialize)]
pub struct CommandInfo {
    pub id: &'static str,
    pub label: &'static str,
    pub menu: Vec<&'static str>,
    pub shortcut: Option<&'static str>,
    pub params: &'static str,
    pub enabled: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub disabled_reason: Option<String>,
}

impl CommandSpec {
    pub fn info(&self, s: &Session) -> CommandInfo {
        let e = (self.enabled)(s);
        CommandInfo {
            id: self.id,
            label: self.label,
            menu: self.menu.to_vec(),
            shortcut: self.shortcut,
            params: self.params,
            enabled: e.is_ok(),
            disabled_reason: e.err(),
        }
    }
}

pub fn always(_: &Session) -> std::result::Result<(), String> {
    Ok(())
}
pub fn has_doc(s: &Session) -> std::result::Result<(), String> {
    s.active().map(|_| ()).ok_or_else(|| "no document open".into())
}
/// Why [`has_selection`] disables a command. A command that documents `ids` still runs when the
/// call names its objects (see [`named_targets`]).
pub const NOTHING_SELECTED: &str = "nothing selected";
/// Why Object › Group is disabled for a selection it can't group. Like [`NOTHING_SELECTED`], a
/// call that names its objects still runs, and the command checks those instead.
pub const CANT_GROUP_SELECTION: &str = "select two or more objects on one spread, all top-level or all in the same group";

/// Whether `reason` only concerns the selection, so a call that names its objects runs anyway.
pub(crate) fn selection_reason(reason: &str) -> bool {
    reason == NOTHING_SELECTED || reason == CANT_GROUP_SELECTION
}
pub fn has_selection(s: &Session) -> std::result::Result<(), String> {
    has_doc(s)?;
    if s.active().is_some_and(|d| !d.selection.items.is_empty()) { Ok(()) } else { Err(NOTHING_SELECTED.into()) }
}
pub fn has_text(s: &Session) -> std::result::Result<(), String> {
    has_doc(s)?;
    if s.active().is_some_and(|d| d.selection.text.is_some()) { Ok(()) } else { Err("no text insertion point".into()) }
}
pub fn has_text_or_frames(s: &Session) -> std::result::Result<(), String> {
    has_doc(s)?;
    if s.active().is_some_and(|d| d.selection.text.is_some() || d.selection.items.iter().any(|i| d.doc.item(*i).is_some_and(|x| x.is_text_frame()))) {
        Ok(())
    } else {
        Err("select text or a text frame".into())
    }
}

macro_rules! cmd {
    ($id:literal, $label:literal, [$($m:literal),*], $sc:expr, $params:literal, $en:expr, $run:expr) => {
        $crate::cmd::CommandSpec { id: $id, label: $label, menu: &[$($m),*], shortcut: $sc, params: $params, enabled: $en, run: $run, journal: true, undoable: true }
    };
    (query $id:literal, $label:literal, [$($m:literal),*], $sc:expr, $params:literal, $en:expr, $run:expr) => {
        $crate::cmd::CommandSpec { id: $id, label: $label, menu: &[$($m),*], shortcut: $sc, params: $params, enabled: $en, run: $run, journal: false, undoable: false }
    };
    (noundo $id:literal, $label:literal, [$($m:literal),*], $sc:expr, $params:literal, $en:expr, $run:expr) => {
        $crate::cmd::CommandSpec { id: $id, label: $label, menu: &[$($m),*], shortcut: $sc, params: $params, enabled: $en, run: $run, journal: true, undoable: false }
    };
}
pub(crate) use cmd;

pub fn command_specs() -> &'static [CommandSpec] {
    static SPECS: std::sync::OnceLock<Vec<CommandSpec>> = std::sync::OnceLock::new();
    SPECS.get_or_init(|| {
        let mut v = Vec::new();
        v.extend(file::specs());
        v.extend(file_info::specs());
        v.extend(interchange::specs());
        v.extend(export::specs());
        v.extend(edit::specs());
        v.extend(paste_svg::specs());
        v.extend(endnotes::specs());
        v.extend(editorial::specs());
        v.extend(object::specs());
        v.extend(anchors::specs());
        v.extend(articles::specs());
        v.extend(paths::specs());
        v.extend(flex::specs());
        v.extend(book::specs());
        v.extend(buttons::specs());
        v.extend(captions::specs());
        v.extend(colorsettings::specs());
        v.extend(color_theme::specs());
        v.extend(changes::specs());
        v.extend(conditions::specs());
        v.extend(library::specs());
        v.extend(lists::specs());
        v.extend(linked::specs());
        v.extend(mathexpr::specs());
        v.extend(media::specs());
        v.extend(pdflayers::specs());
        v.extend(liquid::specs());
        v.extend(xml::specs());
        v.extend(strokes::specs());
        v.extend(scripts::specs());
        v.extend(states::specs());
        v.extend(transitions::specs());
        v.extend(path_type::specs());
        v.extend(qr::specs());
        v.extend(select::specs());
        v.extend(package::specs());
        v.extend(print::specs());
        v.extend(text::specs());
        v.extend(thread::specs());
        v.extend(tabs::specs());
        v.extend(style::specs());
        v.extend(prepress::specs());
        v.extend(load_styles::specs());
        v.extend(table::specs());
        v.extend(layout::specs());
        v.extend(overrides::specs());
        v.extend(prefs::specs());
        v.extend(inspect::specs());
        v.extend(find::specs());
        v.extend(variables::specs());
        v.extend(toc::specs());
        v.extend(notes::specs());
        v.extend(xref::specs());
        v.extend(index::specs());
        v.extend(anchored::specs());
        v.extend(guides::specs());
        v.extend(rulers::specs());
        v.extend(links::specs());
        v.extend(fonts::specs());
        v.extend(spelling::specs());
        v.extend(datamerge::specs());
        v.extend(interactive::specs());
        v.extend(preflight::specs());
        v
    })
}

pub fn find_command(id: &str) -> Option<&'static CommandSpec> {
    command_specs().iter().find(|c| c.id == id)
}

// ---------- param helpers ----------

pub(crate) fn bad(cmd: &str, msg: impl Into<String>) -> EngineError {
    EngineError::BadParams { cmd: cmd.into(), msg: msg.into() }
}
/// `p` with `key` set to `v`. Parameters come from callers (control channel, MCP, scripts) and
/// may not be an object: `p[key] = v` panics on an array or a string, so start from `{}` then.
pub(crate) fn with_param(p: &Value, key: &str, v: Value) -> Value {
    let mut m = p.as_object().cloned().unwrap_or_default();
    m.insert(key.to_string(), v);
    Value::Object(m)
}
pub(crate) fn f64_or(p: &Value, key: &str, default: f64) -> f64 {
    p.get(key).and_then(Value::as_f64).unwrap_or(default)
}
pub(crate) fn bool_or(p: &Value, key: &str, default: bool) -> bool {
    p.get(key).and_then(Value::as_bool).unwrap_or(default)
}
pub(crate) fn str_param<'a>(p: &'a Value, key: &str) -> Option<&'a str> {
    p.get(key).and_then(Value::as_str)
}
/// Plain text from a parameter, with CR LF and lone CR (classic Mac and InDesign line ends) as
/// `\n`, the paragraph separator, as edit.paste and text import already do. Kept as they were,
/// the CRs joined the paragraphs into one. A vertical tab (Word's and PowerPoint's Shift+Enter,
/// Chr(11)) is a forced line break: kept, it was drawn as a missing-glyph box.
pub(crate) fn text_param(p: &Value, key: &str) -> String {
    str_param(p, key)
        .unwrap_or("")
        .replace("\r\n", "\n")
        .replace('\r', "\n")
        .replace('\u{B}', designcraft_doc::story::FORCED_LINE_BREAK.encode_utf8(&mut [0; 4]))
}
pub(crate) fn id_param(p: &Value, key: &str) -> Option<ItemId> {
    p.get(key).and_then(Value::as_u64).map(ItemId)
}
pub(crate) fn ids_param(p: &Value, key: &str) -> Option<Vec<ItemId>> {
    p.get(key).and_then(Value::as_array).map(|a| a.iter().filter_map(Value::as_u64).map(ItemId).collect())
}
pub(crate) fn point_param(p: &Value, key: &str) -> Option<Point> {
    let a = p.get(key)?.as_array()?;
    Some(Point::new(a.first()?.as_f64()?, a.get(1)?.as_f64()?))
}
/// `r`, computed from coordinate params, or an error when a value overflowed to infinity or NaN:
/// a document holding one can't be saved (#1050).
pub(crate) fn finite_rect(command: &str, r: Rect) -> Result<Rect> {
    if [r.x0, r.y0, r.x1, r.y1].iter().all(|v| v.is_finite()) { Ok(r) } else { Err(bad(command, "the position and size must stay finite numbers")) }
}

pub(crate) fn rect_param(p: &Value, key: &str) -> Option<Rect> {
    let a = p.get(key)?.as_array()?;
    let f = |i: usize| a.get(i).and_then(Value::as_f64);
    Some(Rect::new(f(0)?, f(1)?, f(2)?, f(3)?))
}
/// The `spread` param: a body spread index (`0`), a spread reference (`{"kind":"doc"|"parent","index":0}`)
/// or absent or null (= spread 0; commands that forward an optional `spread` pass null). Anything else is an error: read as spread 0, a script aiming at a parent
/// spread got its items on the first page without a word.
pub(crate) fn spread_param(p: &Value, command: &str) -> Result<SpreadRef> {
    let index = |v: &Value, key: &str| {
        v.as_u64().and_then(|n| usize::try_from(n).ok()).ok_or_else(|| bad(command, format!("`{key}` must be a non-negative integer")))
    };
    let Some(value) = p.get("spread").filter(|v| !v.is_null()) else { return Ok(SpreadRef::Doc(0)) };
    if value.is_number() {
        return Ok(SpreadRef::Doc(index(value, "spread")?));
    }
    let object = value.as_object().ok_or_else(|| bad(command, "`spread` must be a non-negative integer or {kind: doc|parent, index}"))?;
    let i = index(object.get("index").ok_or_else(|| bad(command, "missing `spread.index`"))?, "spread.index")?;
    match object.get("kind").and_then(Value::as_str) {
        Some("doc") => Ok(SpreadRef::Doc(i)),
        Some("parent") => Ok(SpreadRef::Parent(i)),
        _ => Err(bad(command, "`spread.kind` must be `doc` or `parent`")),
    }
}

/// The objects a call names with a non-empty `ids` or an `id`, for a command whose params
/// document `ids` (it acts on [`targets`]). `None` when the call names none.
pub(crate) fn named_targets(spec: &CommandSpec, p: &Value) -> Option<Vec<ItemId>> {
    if !spec.params.split(|c: char| !c.is_ascii_alphanumeric()).any(|w| w == "ids") {
        return None;
    }
    // As `targets` reads them: `ids`, when given, wins over `id` (an empty list names nothing).
    if let Some(ids) = ids_param(p, "ids") {
        return (!ids.is_empty()).then_some(ids);
    }
    id_param(p, "id").map(|i| vec![i])
}

/// Targets: `ids` / `id` params or the selection.
pub(crate) fn targets(s: &Session, p: &Value) -> Result<Vec<ItemId>> {
    if let Some(ids) = ids_param(p, "ids") {
        return Ok(ids);
    }
    if let Some(id) = id_param(p, "id") {
        return Ok(vec![id]);
    }
    Ok(s.doc()?.selection.items.clone())
}

pub(crate) fn ok() -> Result<Value> {
    Ok(Value::Null)
}

pub fn file_bytes(d: &designcraft_doc::Document) -> Result<Vec<u8>> {
    file::to_bytes(d)
}
pub fn file_from(b: &[u8]) -> Result<designcraft_doc::Document> {
    file::from_bytes(b)
}
pub(crate) use datamerge::sync_placeholders;
pub(crate) use file::load_document_fonts;
pub use file::{
    DOCUMENT_SNIFF_BYTES, RecentPreview, base64_decode, base64_encode, from_bytes, is_document, recent_preview, to_bytes, with_document_extension,
};
pub use layout::chapter_numbering;
pub use paste_svg::is_svg_markup;
pub(crate) use place_text::autoflow as place_text_autoflow;
