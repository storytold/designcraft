//! Importing a keyboard shortcut set that InDesign's Keyboard Shortcuts dialog saved as text
//! (Show Set): `window.importShortcuts`.
//!
//! The file lists items under `Product area : <area>` headers, one per line:
//! `<name> --- [none defined]` or `<name> --- <context>: <keys>, <context>: <keys>`. Item names
//! use `: ` between submenu levels. Only the Default context applies (DesignCraft has no
//! shortcut contexts); a type command with no Default entry takes its Text entry.

use std::collections::{BTreeSet, HashMap};

use serde_json::{Value, json};

use crate::DesignApp;
use crate::menus::{self, Item};

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
    area: String,
    name: String,
    entries: Vec<Entry>,
}

struct ParsedSet {
    name: String,
    items: Vec<SetItem>,
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

fn parse(text: &str) -> Result<ParsedSet, String> {
    let text = text.strip_prefix('\u{feff}').unwrap_or(text);
    let mut set = ParsedSet { name: String::new(), items: Vec::new() };
    let mut area = String::new();
    let mut header_seen = false;
    for line in text.lines() {
        let line = line.trim_end_matches('\r');
        let trimmed = line.trim();
        if let Some(name) = header(trimmed, "Shortcut Set") {
            set.name = name.to_string();
            header_seen = true;
        } else if let Some(a) = header(trimmed, "Product area") {
            area = a.to_string();
            header_seen = true;
        } else if let Some((name, keys)) = trimmed.split_once(" --- ") {
            let name = name.trim();
            if name.is_empty() {
                continue;
            }
            set.items.push(SetItem { area: area.clone(), name: name.to_string(), entries: entries(keys) });
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
        match pieces.last_mut() {
            Some(last) if !piece.contains(": ") => {
                last.push_str(", ");
                last.push_str(piece);
            }
            _ => pieces.push(piece.to_string()),
        }
    }
    pieces
        .into_iter()
        .take(MAX_ENTRIES)
        .filter_map(|p| {
            let (context, keys) = p.split_once(": ")?;
            Some(Entry { context: context.trim().to_string(), keys: keys.trim().to_string() })
        })
        .collect()
}

/// Whether the keys use Mac modifier names, where Ctrl is the Control key rather than the
/// command modifier.
fn is_mac_set(set: &ParsedSet) -> bool {
    set.items.iter().flat_map(|i| &i.entries).any(|e| {
        let k = e.keys.to_lowercase();
        k.contains("cmd") || k.contains("command") || k.contains('⌘') || k.contains("opt") || k.contains('⌥')
    })
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Mod {
    Command,
    Alt,
    Shift,
    Control,
}

/// Keys as InDesign writes them (`Shift+Ctrl+S`, `Ctrl+Num 5`, `Page Down`, `⇧⌘S`) as a
/// DesignCraft shortcut string (`Cmd+Shift+S`). Ctrl is the command modifier in a Windows set.
fn read_keys(keys: &str, mac: bool) -> Result<String, &'static str> {
    const SYMBOLS: &[(&str, Mod)] = &[("⌘", Mod::Command), ("⌥", Mod::Alt), ("⇧", Mod::Shift), ("⌃", Mod::Control)];
    const WORDS: &[(&str, Mod)] = &[
        ("command", Mod::Command),
        ("cmd", Mod::Command),
        ("option", Mod::Alt),
        ("opt", Mod::Alt),
        ("alt", Mod::Alt),
        ("shift", Mod::Shift),
        ("control", Mod::Control),
        ("ctrl", Mod::Control),
    ];
    let mut m = egui::Modifiers::NONE;
    let mut set = |md: Mod| match md {
        Mod::Command => m.command = true,
        Mod::Alt => m.alt = true,
        Mod::Shift => m.shift = true,
        Mod::Control if mac => m.ctrl = true,
        Mod::Control => m.command = true,
    };
    let mut rest = keys.trim();
    'strip: loop {
        for (sym, md) in SYMBOLS {
            if let Some(r) = rest.strip_prefix(sym) {
                set(*md);
                rest = r.strip_prefix('+').unwrap_or(r).trim_start();
                continue 'strip;
            }
        }
        for (word, md) in WORDS {
            let Some(head) = rest.get(..word.len()) else { continue };
            let Some(after) = rest.get(word.len()..) else { continue };
            // The word must be followed by a separator and a key: "Alt" alone, or "Altx", is a key name.
            let mut chars = after.chars();
            if head.eq_ignore_ascii_case(word) && matches!(chars.next(), Some('+' | ' ' | '-')) && !chars.as_str().is_empty() {
                set(*md);
                rest = chars.as_str().trim_start();
                continue 'strip;
            }
        }
        break;
    }
    if m.ctrl {
        return Err("Control-key shortcuts are not supported");
    }
    let key = key_named(rest.trim()).ok_or("unknown key")?;
    let sc = menus::shortcut_string(m, key);
    // Only a string the shortcut dispatcher reads back as these keys is imported.
    match menus::parse_shortcut(&sc) {
        Some((pm, pk)) if pk == key && same_mods(pm, m) => Ok(sc),
        _ => Err("unknown key"),
    }
}

fn same_mods(a: egui::Modifiers, b: egui::Modifiers) -> bool {
    a.command == b.command && a.alt == b.alt && a.shift == b.shift && a.ctrl == b.ctrl
}

/// A key by the name InDesign gives it (Windows words, Mac symbols), case- and space-insensitive.
fn key_named(name: &str) -> Option<egui::Key> {
    use egui::Key;
    if name.is_empty() {
        return None;
    }
    let squashed: String = name.chars().filter(|c| !c.is_whitespace()).collect::<String>().to_lowercase();
    // The numeric keypad: egui doesn't tell its keys apart from the main ones.
    let n = squashed.strip_prefix("numpad").or_else(|| squashed.strip_prefix("num").filter(|r| !r.is_empty() && *r != "lock")).unwrap_or(&squashed);
    let key = match n {
        "up" | "uparrow" | "arrowup" | "↑" => Key::ArrowUp,
        "down" | "downarrow" | "arrowdown" | "↓" => Key::ArrowDown,
        "left" | "leftarrow" | "arrowleft" | "←" => Key::ArrowLeft,
        "right" | "rightarrow" | "arrowright" | "→" => Key::ArrowRight,
        "pageup" | "pgup" | "⇞" => Key::PageUp,
        "pagedown" | "pgdn" | "⇟" => Key::PageDown,
        "home" | "↖" => Key::Home,
        "end" | "↘" => Key::End,
        "esc" | "escape" | "⎋" => Key::Escape,
        "tab" | "⇥" => Key::Tab,
        "space" | "spacebar" => Key::Space,
        "enter" | "return" | "↩" | "⏎" | "⌤" => Key::Enter,
        "backspace" | "⌫" => Key::Backspace,
        "delete" | "del" | "forwarddelete" | "⌦" => Key::Delete,
        "insert" | "ins" | "help" => Key::Insert,
        _ => return Key::from_name(n).or_else(|| Key::from_name(&n.to_uppercase())).or_else(|| Key::from_name(name)),
    };
    Some(key)
}

/// Matching form of a label or menu name: no ellipsis, no `&`, single spaces, lower case, and
/// no Show/Hide in front (InDesign names a toggle by what choosing it does now).
fn norm(s: &str) -> String {
    let s = s.replace('…', "").replace("...", "").replace('&', "");
    let s = s.split_whitespace().collect::<Vec<_>>().join(" ").to_lowercase();
    ["show/hide ", "show ", "hide "].iter().find_map(|p| s.strip_prefix(p).filter(|r| !r.is_empty())).map_or(s.clone(), str::to_string)
}

/// DesignCraft's commands by menu path and label.
#[derive(Default)]
struct Index {
    /// `menu\nsubmenu\nlabel` → ids, in the menu bar.
    path: HashMap<String, BTreeSet<String>>,
    /// The same from the engine commands' own menu placement, which a UI command (the one with
    /// the dialog) may stand in for in the menu bar.
    engine_path: HashMap<String, BTreeSet<String>>,
    /// (menu, label) → ids, at any depth of that menu.
    in_menu: HashMap<(String, String), BTreeSet<String>>,
    /// label → ids, anywhere.
    label: HashMap<String, BTreeSet<String>>,
}

impl Index {
    fn build() -> Self {
        fn walk(ix: &mut Index, items: &[Item], path: &mut Vec<String>) {
            for it in items {
                match it {
                    Item::Sub(name, children) => {
                        path.push(norm(name));
                        walk(ix, children, path);
                        path.pop();
                    }
                    // A shortcut runs the command without parameters; a fixed-parameter variant can't hold one.
                    Item::Cmd { label, id, params, .. } if params.is_null() => {
                        let label = norm(label);
                        let top = path.first().cloned().unwrap_or_default();
                        add_path(&mut ix.path, path, &label, id);
                        ix.in_menu.entry((top, label.clone())).or_default().insert(id.clone());
                        ix.label.entry(label).or_default().insert(id.clone());
                    }
                    _ => {}
                }
            }
        }
        let mut ix = Index::default();
        for (menu, entries) in menus::menu_tree() {
            walk(&mut ix, &entries, &mut vec![norm(menu)]);
        }
        for c in designcraft_engine::command_specs() {
            let label = norm(c.label);
            if !c.menu.is_empty() {
                let path: Vec<String> = c.menu.iter().map(|m| norm(m)).collect();
                add_path(&mut ix.engine_path, &path, &label, c.id);
            }
            ix.label.entry(label).or_default().insert(c.id.to_string());
        }
        for (id, label, _, _) in menus::UI_COMMANDS {
            ix.label.entry(norm(label)).or_default().insert((*id).to_string());
        }
        ix
    }

    /// The command an item names, trying the most specific match first. Matches by label alone
    /// only count when exactly one command has that label.
    fn find(&self, area: &str, name: &str) -> Option<String> {
        let one = |s: Option<&BTreeSet<String>>| s.filter(|s| s.len() == 1).and_then(|s| s.first().cloned());
        let segments: Vec<String> = name.split(": ").map(norm).collect();
        let last = segments.last().cloned().unwrap_or_default();
        let area = norm(area);
        if let Some(menu) = area.strip_suffix(" menu").filter(|m| !m.contains(':')) {
            let mut key = menu.to_string();
            for s in &segments {
                key.push('\n');
                key.push_str(s);
            }
            if let Some(id) = one(self.path.get(&key)) {
                return Some(id);
            }
            if segments.len() > 1
                && let Some(id) = one(self.in_menu.get(&(menu.to_string(), segments.join(" "))))
            {
                return Some(id);
            }
            if let Some(id) = one(self.engine_path.get(&key)) {
                return Some(id);
            }
            if let Some(id) = one(self.in_menu.get(&(menu.to_string(), last.clone()))) {
                return Some(id);
            }
            // Menus are arranged differently: the item's own label, if no other command has it.
            return one(self.label.get(&last));
        }
        if segments.len() == 1 { one(self.label.get(&last)) } else { None }
    }
}

fn add_path(map: &mut HashMap<String, BTreeSet<String>>, path: &[String], label: &str, id: &str) {
    let mut key = path.join("\n");
    key.push('\n');
    key.push_str(label);
    map.entry(key).or_default().insert(id.to_string());
}

/// What the set says about one DesignCraft command, over every item matched to it.
struct Decision {
    id: String,
    item: String,
    /// The first readable shortcut.
    shortcut: Option<String>,
    /// Every matched item said `[none defined]`.
    all_none: bool,
}

fn command_label(id: &str) -> String {
    menus::ui_label(id)
        .map(|(l, _)| l.to_string())
        .or_else(|| designcraft_engine::find_command(id).map(|c| c.label.to_string()))
        .unwrap_or_else(|| id.to_string())
}

fn keys_of(sc: &str) -> Option<(bool, bool, bool, bool, egui::Key)> {
    menus::parse_shortcut(sc).map(|(m, k)| (m.command, m.alt, m.shift, m.ctrl, k))
}

/// Imports a set: every matched command takes the set's shortcut (or none), another command
/// holding an imported shortcut loses it, and the rest keep theirs. Returns the report.
pub fn import(app: &mut DesignApp, text: &str) -> Result<Value, String> {
    if text.len() > MAX_BYTES {
        return Err(format!("the set is larger than {} MB", MAX_BYTES >> 20));
    }
    let set = parse(text)?;
    let mac = is_mac_set(&set);
    let ix = Index::build();
    let mut decisions: Vec<Decision> = Vec::new();
    let mut unmatched = Vec::new();
    let mut unmatched_without_shortcut = 0usize;
    let mut unreadable = Vec::new();
    for item in &set.items {
        let shown = if item.area.is_empty() { item.name.clone() } else { format!("{} › {}", item.area, item.name) };
        let all_keys = item.entries.iter().map(|e| format!("{}: {}", e.context, e.keys)).collect::<Vec<_>>().join(", ");
        let Some(id) = ix.find(&item.area, &item.name) else {
            if item.entries.is_empty() {
                unmatched_without_shortcut += 1;
            } else {
                let tool = norm(&item.area) == "tools"
                    && designcraft_tools::TOOL_GROUPS.iter().flat_map(|g| g.iter()).any(|t| norm(t.label) == norm(&item.name));
                let mut row = json!({"item": shown, "keys": all_keys});
                if tool {
                    row["reason"] = json!("tool keys are fixed");
                }
                unmatched.push(row);
            }
            continue;
        };
        let in_context = |ctx: &'static str| item.entries.iter().filter(move |e| e.context.eq_ignore_ascii_case(ctx)).collect::<Vec<&Entry>>();
        let text_command = id.starts_with("type.") || id.starts_with("text.");
        let mut considered = in_context("Default");
        if considered.is_empty() && text_command {
            considered = in_context("Text");
        }
        let mut read = None;
        let mut why = "";
        for e in &considered {
            match read_keys(&e.keys, mac) {
                Ok(sc) => {
                    read = Some(sc);
                    break;
                }
                Err(w) => why = w,
            }
        }
        if read.is_none() && !considered.is_empty() {
            unreadable.push(json!({"item": shown, "keys": all_keys, "reason": why}));
        }
        let pos = match decisions.iter().position(|d| d.id == id) {
            Some(p) => p,
            None => {
                decisions.push(Decision { id: id.clone(), item: shown.clone(), shortcut: None, all_none: true });
                decisions.len() - 1
            }
        };
        let Some(d) = decisions.get_mut(pos) else { continue };
        d.all_none &= item.entries.is_empty();
        if d.shortcut.is_none()
            && let Some(sc) = read
        {
            d.shortcut = Some(sc);
            d.item = shown;
        }
    }

    let before: HashMap<String, Option<String>> = menus::effective_shortcuts(app).into_iter().map(|(id, sc)| (id, Some(sc))).collect();
    let mut imported = 0usize;
    let mut taken: Vec<((bool, bool, bool, bool, egui::Key), String)> = Vec::new();
    let mut skipped = Vec::new();
    let mut decided: BTreeSet<String> = BTreeSet::new();
    // When two items in the set want the same keys, the later one takes them (the rule shared by
    // the Craft apps' importers).
    let mut winner: HashMap<(bool, bool, bool, bool, egui::Key), usize> = HashMap::new();
    for (i, d) in decisions.iter().enumerate() {
        if let Some(keys) = d.shortcut.as_deref().and_then(keys_of) {
            winner.insert(keys, i);
        }
    }
    for (i, d) in decisions.iter().enumerate() {
        match &d.shortcut {
            Some(sc) => {
                let Some(keys) = keys_of(sc) else { continue };
                if let Some(w) = winner.get(&keys).filter(|&&w| w != i).and_then(|&w| decisions.get(w)) {
                    skipped.push(json!({"id": d.id, "item": d.item, "shortcut": sc, "takenBy": w.id}));
                    continue;
                }
                taken.push((keys, d.id.clone()));
                if menus::default_shortcut(&d.id).and_then(keys_of) == Some(keys) {
                    app.ui.shortcuts.remove(&d.id);
                } else {
                    app.ui.shortcuts.insert(d.id.clone(), sc.clone());
                }
            }
            None if d.all_none => {
                if menus::default_shortcut(&d.id).is_none() {
                    app.ui.shortcuts.remove(&d.id);
                } else {
                    app.ui.shortcuts.insert(d.id.clone(), String::new());
                }
            }
            None => continue,
        }
        imported += 1;
        decided.insert(d.id.clone());
    }
    let mut conflicts = Vec::new();
    for (id, sc) in menus::effective_shortcuts(app) {
        if decided.contains(&id) {
            continue;
        }
        // A command sharing its default keys with the one taking them (file.new and the New
        // Document dialog) keeps them: the dispatcher already prefers the other.
        let twin = |by: &str| {
            let default = |id: &str| menus::default_shortcut(id).and_then(keys_of);
            default(&id).is_some() && default(&id) == default(by)
        };
        if let Some(keys) = keys_of(&sc)
            && taken.iter().any(|(k, by)| *k == keys && !twin(by))
        {
            app.ui.shortcuts.insert(id.clone(), String::new());
            conflicts.push(json!({"id": id, "label": command_label(&id), "shortcut": sc}));
        }
    }
    let changed: Vec<Value> = decisions
        .iter()
        .filter(|d| decided.contains(&d.id))
        .filter_map(|d| {
            let from = before.get(&d.id).cloned().flatten();
            let to = menus::shortcut_of(app, &d.id);
            (from != to).then(|| json!({"id": d.id, "label": command_label(&d.id), "item": d.item, "from": from, "to": to}))
        })
        .collect();
    Ok(json!({
        "set": set.name,
        "imported": imported,
        "changed": changed,
        "conflictsCleared": conflicts,
        "skipped": skipped,
        "unmatched": unmatched,
        "unmatchedWithoutShortcut": unmatched_without_shortcut,
        "unreadable": unreadable,
    }))
}

/// One line for the Keyboard Shortcuts dialog about an import's report.
pub fn summary(lang: &str, report: &Value) -> String {
    let n = |k: &str| report[k].as_u64().unwrap_or_default().to_string();
    let len = |k: &str| report[k].as_array().map_or(0, Vec::len).to_string();
    crate::i18n::tr(lang, "Imported {count} shortcuts from “{set}”; {unmatched} items not matched, {unreadable} unreadable.")
        .replace("{count}", &n("imported"))
        .replace("{set}", report["set"].as_str().unwrap_or_default())
        .replace("{unmatched}", &len("unmatched"))
        .replace("{unreadable}", &len("unreadable"))
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
            \tClose --- Default: Shift+Ctrl+Num 5, Default: Ctrl+F4\r\n\
            \tSave --- Default: Ctrl+Hyperspace\r\n\
            \tRevert --- Default: Shift+Ctrl+F9\r\n\
            \tSave As... --- Default: Shift+Ctrl+F9\r\n\
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
        assert_eq!(r["unmatched"], json!([{"item": "Edit Menu › Frobnicate Everything", "keys": "Default: Ctrl+F12"}]));
        assert_eq!(r["unmatchedWithoutShortcut"], 1);
        assert_eq!(r["unreadable"][0]["item"], "File Menu › Save");
        assert_eq!(menus::shortcut_of(&app, "app.save").as_deref(), Some("Cmd+S"));
        // Two items want the same keys: the later one takes them.
        assert_eq!(menus::shortcut_of(&app, "app.saveDialog").as_deref(), Some("Cmd+Shift+F9"));
        assert_eq!(r["skipped"][0]["takenBy"], "app.saveDialog", "{r}");
        assert_eq!(r["imported"], 4);
        // Reset Shortcuts is the way back.
        menus::run_ui(&mut app, "window.resetShortcuts", &json!({})).unwrap().unwrap();
        assert_eq!(menus::shortcut_of(&app, "app.palette").as_deref(), Some("Cmd+Return"));
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
