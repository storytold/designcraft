//! Importing a keyboard shortcut set that InDesign's Keyboard Shortcuts dialog saved as text
//! (Show Set): `window.importShortcuts`.
//!
//! The file lists items under `Product area : <area>` headers, one per line:
//! `<name> --- [none defined]` or `<name> --- <context>: <keys>, <context>: <keys>`. Item names
//! use `: ` between submenu levels. This module reads the file into a
//! [`craft_keymap::ForeignSet`]; `craft_keymap::import` matches and applies it. Default keys bind
//! in the command's home scope; an item without Default keys takes its Text keys.

use craft_keymap::import::{MAX_IMPORT_ENTRIES, MAX_IMPORT_TEXT};
use craft_keymap::{ContextRule, Flavor, ForeignEntry, ForeignKey, ForeignSet, ImportOptions, ImportReport};
use serde_json::{Value, json};

use crate::DesignApp;

/// Largest set file read. InDesign's full set is a few hundred kilobytes (twice that as UTF-16).
pub const MAX_BYTES: usize = 8 << 20;
/// Entries read per item: a real item has one or two per context.
const MAX_ENTRIES: usize = 64;

/// One `<context>: <keys>` entry of an item.
struct Entry {
    context: String,
    keys: String,
}

struct SetItem {
    /// Index into [`ParsedSet::areas`]; `None` before the first `Product area` line.
    area: Option<usize>,
    name: String,
    entries: Vec<Entry>,
    /// 1-based line in the file.
    line: usize,
}

struct ParsedSet {
    name: String,
    areas: Vec<String>,
    items: Vec<SetItem>,
    /// Items past [`MAX_IMPORT_ENTRIES`], not read.
    dropped: usize,
}

/// Text from the bytes of a set file: UTF-8 (with or without a BOM) or UTF-16 (by its BOM, or
/// by the zero bytes of ASCII text without one).
pub fn decode(bytes: &[u8]) -> Result<String, String> {
    if bytes.len() > MAX_BYTES {
        return Err(format!("the file is larger than {} MB", MAX_BYTES >> 20));
    }
    let utf16 = |rest: &[u8], le: bool| -> String {
        let units = rest.as_chunks::<2>().0.iter().map(|&c| if le { u16::from_le_bytes(c) } else { u16::from_be_bytes(c) });
        char::decode_utf16(units).map(|r| r.unwrap_or(char::REPLACEMENT_CHARACTER)).collect()
    };
    Ok(match bytes {
        [0xFF, 0xFE, rest @ ..] => utf16(rest, true),
        [0xFE, 0xFF, rest @ ..] => utf16(rest, false),
        [0xEF, 0xBB, 0xBF, rest @ ..] => String::from_utf8_lossy(rest).into_owned(),
        [a, 0, ..] if *a != 0 => utf16(bytes, true),
        [0, b, ..] if *b != 0 => utf16(bytes, false),
        _ => String::from_utf8_lossy(bytes).into_owned(),
    })
}

/// Reads and decodes a set file, refusing one over [`MAX_BYTES`].
pub fn read_file(path: &str) -> Result<String, String> {
    let cap = u64::try_from(MAX_BYTES).unwrap_or(u64::MAX);
    let bytes = designcraft_engine::io::read_capped(path, cap).map_err(|e| format!("can't read {path}: {e}"))?;
    decode(&bytes)
}

/// `Label : value` with any spacing around the colon, when the line starts with `label`.
fn header<'a>(line: &'a str, label: &str) -> Option<&'a str> {
    let rest = line.get(..label.len()).filter(|h| h.eq_ignore_ascii_case(label)).and_then(|_| line.get(label.len()..))?;
    rest.trim_start().strip_prefix(':').map(str::trim)
}

/// The first [`MAX_IMPORT_TEXT`] chars of `s`.
fn capped(s: &str) -> &str {
    s.char_indices().nth(MAX_IMPORT_TEXT).and_then(|(end, _)| s.get(..end)).unwrap_or(s)
}

fn parse(text: &str) -> Result<ParsedSet, String> {
    let text = text.strip_prefix('\u{feff}').unwrap_or(text);
    let mut set = ParsedSet { name: String::new(), areas: Vec::new(), items: Vec::new(), dropped: 0 };
    let mut header_seen = false;
    for (n, line) in text.lines().enumerate() {
        let trimmed = line.trim_end_matches('\r').trim();
        if let Some(name) = header(trimmed, "Shortcut Set") {
            set.name = capped(name).to_string();
            header_seen = true;
        } else if let Some(a) = header(trimmed, "Product area") {
            set.areas.push(capped(a).to_string());
            header_seen = true;
        } else if let Some((name, keys)) = trimmed.split_once(" --- ") {
            let name = name.trim();
            if name.is_empty() {
                continue;
            }
            if set.items.len() >= MAX_IMPORT_ENTRIES {
                set.dropped += 1;
                continue;
            }
            let area = set.areas.len().checked_sub(1);
            set.items.push(SetItem { area, name: name.to_string(), entries: entries(keys), line: n.saturating_add(1) });
        }
    }
    if set.items.is_empty() && !header_seen {
        return Err("not a shortcut set: no `Product area` or `<item> --- <shortcuts>` lines".into());
    }
    Ok(set)
}

/// The entries after ` --- `: none for `[none defined]`. A comma is also a key (`Ctrl+,`), so a
/// piece without `: ` belongs to the entry before it.
fn entries(keys: &str) -> Vec<Entry> {
    let keys = keys.trim();
    if keys.eq_ignore_ascii_case("[none defined]") || keys.is_empty() {
        return Vec::new();
    }
    let mut pieces: Vec<String> = Vec::new();
    for piece in keys.split(", ") {
        let full = pieces.len() >= MAX_ENTRIES;
        match pieces.last_mut() {
            Some(last) if !piece.contains(": ") => {
                last.push_str(", ");
                last.push_str(piece);
            }
            _ if full => break,
            _ => pieces.push(piece.to_string()),
        }
    }
    pieces
        .into_iter()
        .filter_map(|p| {
            let (context, keys) = p.split_once(": ")?;
            Some(Entry { context: context.trim().to_string(), keys: keys.trim().to_string() })
        })
        .collect()
}

/// `s` without a trailing `suffix`, compared without ASCII case.
fn strip_suffix_ignore_case<'a>(s: &'a str, suffix: &str) -> Option<&'a str> {
    let at = s.len().checked_sub(suffix.len())?;
    s.get(at..).filter(|end| end.eq_ignore_ascii_case(suffix)).and_then(|_| s.get(..at))
}

/// The set as `craft_keymap::import` reads it. An area named `<menu> Menu` is that menu; the
/// Tools area is the Tools menu, where items named `… Tool` select tools. Items of the Panel Menus
/// area name their panel first (`Character: All Caps`) and match a command under that panel.
/// Items of other areas (text and tables, views) have no menu, so they match only by a one-segment
/// name. The keys are read with Mac modifier names when any key in the set uses one.
fn foreign(set: &ParsedSet) -> ForeignSet {
    #[derive(Clone, Copy, Default)]
    struct Area<'a> {
        menu: Option<&'a str>,
        tools: bool,
        panel_menu: bool,
    }
    let flavor = Flavor::detect(set.items.iter().flat_map(|i| &i.entries).map(|e| e.keys.as_str()));
    let areas: Vec<Area> = set
        .areas
        .iter()
        .map(|a| {
            let a = a.trim();
            if a.eq_ignore_ascii_case("tools") {
                Area { menu: Some(a), tools: true, panel_menu: false }
            } else if a.eq_ignore_ascii_case("panel menus") {
                Area { panel_menu: true, ..Area::default() }
            } else {
                let menu = strip_suffix_ignore_case(a, " menu").map(str::trim).filter(|m| !m.is_empty() && !m.contains(':'));
                Area { menu, ..Area::default() }
            }
        })
        .collect();
    let entries = set
        .items
        .iter()
        .map(|item| {
            let area = item.area.and_then(|a| areas.get(a)).copied().unwrap_or_default();
            let mut entry = ForeignEntry::new(area.menu, item.name.split(": ").map(str::trim)).at_line(item.line);
            if area.panel_menu {
                entry = entry.in_panel_menu();
            }
            entry.is_tool = area.tools && strip_suffix_ignore_case(&item.name, " tool").is_some();
            entry.keys = item.entries.iter().map(|e| ForeignKey::read(Some(&e.context), &e.keys, flavor)).collect();
            entry
        })
        .collect();
    ForeignSet { name: Some(set.name.clone()), entries }
}

/// Imports a set: every matched command takes the set's shortcuts (or none, when every item
/// matched to it has none), other commands lose keys it takes (unless they share them as
/// defaults), and the rest keep theirs. Returns the report.
pub fn import(app: &mut DesignApp, text: &str) -> Result<Value, String> {
    if text.len() > MAX_BYTES {
        return Err(format!("the set is larger than {} MB", MAX_BYTES >> 20));
    }
    let parsed = parse(text)?;
    let set = foreign(&parsed);
    let options = ImportOptions {
        contexts: vec![("Default".into(), ContextRule::Home), (crate::keys::TEXT_CONTEXT.into(), ContextRule::HomeWithoutHomeEntry)],
        suggest: true,
        ..ImportOptions::default()
    };
    app.sync_keymap();
    let mut report = craft_keymap::plan(&set, &app.keymap, &options).apply(&mut app.keymap);
    report.truncated = report.truncated.saturating_add(parsed.dropped);
    crate::keys::store(app);
    Ok(report_json(&report))
}

/// An enum of the report as its serde name (`noMatch`, `numpad`).
fn name_of<T: serde::Serialize>(v: &T) -> Value {
    serde_json::to_value(v).unwrap_or(Value::Null)
}

/// The report as `window.importShortcuts` returns it. Commands are command keys (`file.save`,
/// `window.panel {"panel":"layers"}`) under `id`.
fn report_json(r: &ImportReport) -> Value {
    json!({
        "set": r.set,
        "imported": r.imported,
        "changed": Value::Array(r.changed.iter().map(|c| json!({
            "id": c.command.key(), "label": c.label, "item": c.item, "line": c.line, "from": c.from, "to": c.to,
        })).collect()),
        "conflictsCleared": Value::Array(r.conflicts_cleared.iter().map(|c| json!({
            "id": c.command.key(), "label": c.label, "shortcut": c.shortcut,
        })).collect()),
        "skipped": Value::Array(r.skipped.iter().map(|s| json!({
            "id": s.command.key(), "item": s.item, "line": s.line, "shortcut": s.shortcut,
            "takenBy": s.kept_by.as_ref().map(|c| c.key()), "reason": name_of(&s.reason),
        })).collect()),
        "failed": Value::Array(r.failed.iter().map(|f| json!({
            "id": f.command.key(), "label": f.label, "change": name_of(&f.change), "shortcuts": f.shortcuts,
        })).collect()),
        "unmatched": Value::Array(r.unmatched.iter().map(|u| json!({
            "item": u.item, "line": u.line, "keys": u.keys, "reason": name_of(&u.reason),
        })).collect()),
        "unmatchedWithoutShortcut": r.unmatched_without_shortcut,
        "unreadable": Value::Array(r.unreadable.iter().map(|u| json!({
            "item": u.item, "line": u.line, "keys": u.keys, "reason": u.reason.to_string(),
        })).collect()),
        "approximated": Value::Array(r.approximated.iter().map(|a| json!({
            "item": a.item, "line": a.line, "keys": a.keys, "note": name_of(&a.note),
        })).collect()),
        "suggestions": Value::Array(r.suggestions.iter().map(|s| json!({
            "item": s.item, "line": s.line, "candidates": s.candidates.iter().map(|c| c.key()).collect::<Vec<_>>(),
        })).collect()),
        "matchedWithoutChange": Value::Array(r.matched_without_change.iter().map(|m| json!({
            "id": m.command.key(), "label": m.label, "item": m.item, "line": m.line, "reason": name_of(&m.reason),
        })).collect()),
        "truncated": r.truncated,
    })
}

/// One line for the Keyboard Shortcuts dialog about an import's report.
pub fn summary(lang: &str, report: &Value) -> String {
    let n = |k: &str| report[k].as_u64().unwrap_or_default().to_string();
    let len = |k: &str| report[k].as_array().map_or(0, Vec::len).to_string();
    // Rows for keys in contexts DesignCraft lacks belong to items that did match.
    let unmatched = report["unmatched"].as_array().map_or(0, |rows| rows.iter().filter(|r| r["reason"] != "context").count());
    let mut out = crate::i18n::tr(lang, "Imported {count} shortcuts from “{set}”; {unmatched} items not matched, {unreadable} unreadable.")
        .replace("{count}", &n("imported"))
        .replace("{set}", report["set"].as_str().unwrap_or_default())
        .replace("{unmatched}", &unmatched.to_string())
        .replace("{unreadable}", &len("unreadable"));
    let reserved = report["skipped"].as_array().map_or(0, |rows| rows.iter().filter(|r| r["reason"] == "reserved").count());
    if reserved > 0 {
        out.push(' ');
        out.push_str(
            &crate::i18n::tr(lang, "{count} shortcuts are reserved by the system and were left out.").replace("{count}", &reserved.to_string()),
        );
    }
    out
}

/// The Keyboard Shortcuts dialog's Import button: a native file picker imports at once and
/// returns the summary; in a browser the chosen file arrives later through the import inbox.
pub fn pick_and_import(app: &mut DesignApp) -> Option<String> {
    if let Some(pick) = app.services.pick_open.as_mut() {
        let path = pick("shortcuts")?;
        return Some(match app.run("window.importShortcuts", json!({"path": path})) {
            Ok(report) => summary(&app.ui.language, &report),
            Err(e) => e,
        });
    }
    let request = app.import_request("shortcuts");
    if let Some(open) = app.services.open_async.as_mut() {
        open(request);
    }
    None
}

/// A set file chosen in the browser: import it and say how it went in the open Keyboard
/// Shortcuts dialog, or the status bar.
pub fn import_delivered(app: &mut DesignApp, bytes: &[u8]) {
    let msg = match decode(bytes).and_then(|text| app.run("window.importShortcuts", json!({"text": text}))) {
        Ok(report) => summary(&app.ui.language, &report),
        Err(e) => e,
    };
    match app.ui.dialog.as_mut().filter(|d| d.id == "keyboardShortcuts") {
        Some(d) => {
            d.fields.insert("message".into(), json!(msg));
        }
        None => app.status(msg),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::menus;

    fn app() -> DesignApp {
        DesignApp::new(designcraft_engine::Session::new(), crate::Services::default())
    }

    #[test]
    fn imports_a_windows_set_and_reports_what_it_could_not_use() {
        let mut app = app();
        // Something else holds Ctrl+Alt+K before the import; the set gives it to Check Spelling.
        menus::run_ui(&mut app, "window.setShortcut", &json!({"id": "app.palette", "shortcut": "Cmd+Alt+K"})).unwrap().unwrap();
        let set = "\u{feff}Shortcut Set : My Set\r\n\r\n\
            Product area : Edit Menu\r\n\
            \tSpelling: Check Spelling... --- Default: Ctrl+Alt+K, Text: F7\r\n\
            \tDuplicate --- [none defined]\r\n\
            \tDuplicate --- [none defined]\r\n\
            \tFrobnicate Everything --- Default: Ctrl+F12\r\n\
            \tNever Heard Of It --- [none defined]\r\n\
            Product area : File Menu\r\n\
            \tClose --- Default: Shift+Ctrl+Num 5, Default: Ctrl+F4, Tables: Ctrl+3\r\n\
            \tSave --- Default: Ctrl+Hyperspace\r\n\
            \tRevert --- Default: Shift+Ctrl+F9\r\n\
            \tSave As... --- Default: Shift+Ctrl+F9\r\n\
            Product area : Window Menu\r\n\
            \tColor: Swatches --- Default: Shift+Ctrl+F2\r\n\
            Product area : Tools\r\n\
            \tPen Tool --- Default: Shift+Ctrl+F3\r\n\
            Product area : File Menu\r\n\
            \tExit --- Default: Ctrl+Alt+F13\r\n\
            garbage line without a separator\r\n";
        let r = menus::run_ui(&mut app, "window.importShortcuts", &json!({"text": set})).unwrap().unwrap();
        assert_eq!(r["set"], "My Set");
        // A submenu item, with Ctrl as the command modifier.
        assert_eq!(menus::shortcut_of(&app, "spelling.check").as_deref(), Some("Cmd+Alt+K"));
        assert_eq!(menus::shortcut_of(&app, "file.close").as_deref(), Some("Cmd+Shift+5"));
        // `[none defined]` in every occurrence clears a matched command.
        assert_eq!(menus::shortcut_of(&app, "edit.duplicate"), None);
        // The command that held the imported keys loses them.
        assert_eq!(menus::shortcut_of(&app, "app.palette"), None);
        assert!(r["conflictsCleared"].as_array().unwrap().iter().any(|c| c["id"] == "app.palette"), "{r}");
        // Unknown items with shortcuts and unreadable keys are reported; Save keeps its shortcut.
        assert_eq!(
            r["unmatched"],
            json!([
                {"item": "Edit > Frobnicate Everything", "line": 7, "keys": "Default: Ctrl+F12", "reason": "noMatch"},
                {"item": "File > Close", "line": 10, "keys": "Tables: Ctrl+3", "reason": "context"},
            ])
        );
        // Keys in a context DesignCraft lacks don't make their matched item count as unmatched.
        assert!(summary("en", &r).contains("; 1 items not matched"), "{}", summary("en", &r));
        assert_eq!(r["unmatchedWithoutShortcut"], 1);
        assert_eq!(r["unreadable"][0]["item"], "File > Save");
        assert_eq!(menus::shortcut_of(&app, "app.save").as_deref(), Some("Cmd+S"));
        // Two items want the same keys: the later one takes them.
        assert_eq!(menus::shortcut_of(&app, "app.saveDialog").as_deref(), Some("Cmd+Shift+F9"));
        assert_eq!(r["skipped"][0]["takenBy"], "app.saveDialog", "{r}");
        // A Window menu panel item and a Tools item apply to the panel and the tool.
        assert_eq!(crate::keys::shortcut(&app.keymap, &crate::keys::panel("swatches")).as_deref(), Some("Cmd+Shift+F2"));
        assert_eq!(crate::keys::shortcut(&app.keymap, &crate::keys::tool("pen")).as_deref(), Some("Cmd+Shift+F3"));
        // InDesign's name for a command, as an alias.
        assert_eq!(menus::shortcut_of(&app, "app.quit").as_deref(), Some("Cmd+Alt+F13"));
        // The numeric keypad key is bound as the main key of the same name, and reported.
        assert_eq!(r["approximated"][0]["item"], "File > Close", "{r}");
        assert_eq!(r["imported"], 7, "{r}");
        // Reset Shortcuts is the way back.
        menus::run_ui(&mut app, "window.resetShortcuts", &json!({})).unwrap().unwrap();
        assert_eq!(menus::shortcut_of(&app, "app.palette").as_deref(), Some("Cmd+Enter"));
    }

    /// Two keys for one command still save a ui.json that earlier versions read: they deserialize
    /// `shortcuts` as a map of one text per command and drop the whole file otherwise.
    #[test]
    fn a_command_with_two_imported_shortcuts_saves_a_ui_json_older_versions_read() {
        let mut app = app();
        let set = "Shortcut Set : Two\r\nProduct area : File Menu\r\n\tClose --- Default: Shift+Ctrl+Num 5, Default: Ctrl+F4\r\n";
        menus::run_ui(&mut app, "window.importShortcuts", &json!({"text": set})).unwrap().unwrap();
        let keys = app.ui.shortcuts.get("file.close").unwrap().to_vec();
        assert_eq!(keys.len(), 2, "{keys:?}");
        let saved = serde_json::to_vec_pretty(&app.ui).unwrap();
        let fields: Value = serde_json::from_slice(&saved).unwrap();
        let old: std::collections::BTreeMap<String, String> = serde_json::from_value(fields["shortcuts"].clone()).unwrap();
        assert_eq!(old.get("file.close"), Some(&keys[0]));
        assert_eq!(fields["shortcutsMore"], json!({"file.close": [keys[1]]}));
        let loaded = crate::UiState::from_preferences_json(&saved).unwrap();
        assert_eq!(loaded.shortcuts, app.ui.shortcuts);
        // The array form an earlier build of the shared keymap wrote.
        let draft = json!({"shortcuts": [{"command": "file.close", "keys": keys}, {"command": "app.palette", "keys": []}]});
        let loaded = crate::UiState::from_preferences_json(&serde_json::to_vec(&draft).unwrap()).unwrap();
        assert_eq!(loaded.shortcuts.get("file.close"), Some(keys.as_slice()));
        assert_eq!(loaded.shortcuts.get("app.palette"), Some(&[][..]));
        // Extra keys for a command an earlier version reset are left out.
        let reset = json!({"shortcuts": {}, "shortcutsMore": {"file.close": ["Cmd+F4"]}});
        assert!(crate::UiState::from_preferences_json(&serde_json::to_vec(&reset).unwrap()).unwrap().shortcuts.is_empty());
    }

    /// A set's key on a chord the system takes is left out and the summary says so.
    #[test]
    fn a_reserved_chord_in_a_set_is_left_out() {
        let mut app = app();
        // Ctrl in a Windows set is ⌘ on a Mac.
        let keys = if crate::keys::platform().is_mac() { "Ctrl+Tab" } else { "Alt+F4" };
        let set = format!("Shortcut Set : OS\r\nProduct area : File Menu\r\n\tExit --- Default: {keys}\r\n");
        let r = menus::run_ui(&mut app, "window.importShortcuts", &json!({ "text": set })).unwrap().unwrap();
        assert_eq!(r["skipped"][0]["reason"], "reserved", "{r}");
        assert_eq!(r["imported"], 0, "{r}");
        assert_eq!(menus::shortcut_of(&app, "app.quit"), menus::default_shortcut("app.quit").map(str::to_string));
        assert!(summary("en", &r).ends_with("1 shortcuts are reserved by the system and were left out."), "{}", summary("en", &r));
    }

    #[test]
    fn a_panel_item_shares_its_key_with_the_command_that_opens_the_panel() {
        let mut app = app();
        let set = "Shortcut Set : Panels\r\nProduct area : Window Menu\r\n\tType & Tables: Character --- Default: Ctrl+T\r\n";
        let r = menus::run_ui(&mut app, "window.importShortcuts", &json!({"text": set})).unwrap().unwrap();
        assert_eq!(crate::keys::shortcut(&app.keymap, &crate::keys::panel("character")).as_deref(), Some("Cmd+T"));
        assert_eq!(menus::shortcut_of(&app, "app.characterPanel").as_deref(), Some("Cmd+T"));
        assert!(!r["conflictsCleared"].as_array().into_iter().flatten().any(|c| c["id"] == "app.characterPanel"), "{r}");
    }

    #[test]
    fn hostile_input_returns_a_report_or_an_error() {
        let mut app = app();
        for text in ["", "\0\0\0", "--- ---", " --- Default: Ctrl+", "Product area : \n\t: --- Default: +, Default: ,,,", "x --- Default: ⌘⇧"] {
            let _ = menus::run_ui(&mut app, "window.importShortcuts", &json!({"text": text})).unwrap();
        }
        assert!(menus::run_ui(&mut app, "window.importShortcuts", &json!({})).unwrap().is_err());
        assert!(menus::run_ui(&mut app, "window.importShortcuts", &json!({"path": "/nonexistent/set.txt"})).unwrap().is_err());
        // UTF-16 with a BOM, and Mac modifier spellings.
        let mut bytes = vec![0xFF, 0xFE];
        bytes.extend("Shortcut Set : Mac\nProduct area : File Menu\n\tClose --- Default: ⇧⌘W\n".encode_utf16().flat_map(u16::to_le_bytes));
        let r = menus::run_ui(&mut app, "window.importShortcuts", &json!({"text": decode(&bytes).unwrap()})).unwrap().unwrap();
        assert_eq!(r["set"], "Mac");
        assert_eq!(menus::shortcut_of(&app, "file.close").as_deref(), Some("Cmd+Shift+W"));
    }
}
