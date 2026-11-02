use super::state::{shortcut_of, shortcut_text};
use super::ui_commands::{CJK_ONLY, UI_COMMANDS, shown};
use super::{DesignApp, Value, json};

/// Quick Apply (⌘Return): the document's styles and every command, searched by name.
pub fn palette(app: &mut DesignApp, ctx: &egui::Context) {
    let Some(mut q) = app.ui.palette.clone() else { return };
    let mut close = false;
    let mut run: Option<(String, Value)> = None;
    egui::Modal::new(egui::Id::unique("palette")).show(ctx, |ui| {
        ui.set_width(480.0);
        let r = ui.add(
            egui::TextEdit::singleline(&mut q)
                .hint_text(crate::rtl::widget(ui, crate::i18n::tr(&app.ui.language, "Search styles and commands…")))
                .desired_width(f32::INFINITY),
        );
        r.request_focus();
        let items = quick_apply_items(&app.session, &q, crate::cjk_features(app));
        // Only a fresh Return runs the first entry: ⌘Return pressed again or held down (repeats) is
        // the shortcut, not a choice.
        let enter = ui.input(|i| {
            i.events
                .iter()
                .any(|e| matches!(e, egui::Event::Key { key: egui::Key::Enter, pressed: true, repeat: false, modifiers, .. } if !modifiers.command))
        });
        egui::ScrollArea::vertical().max_height(340.0).show(ui, |ui| {
            for (i, it) in items.iter().enumerate() {
                let mut b = egui::Button::new(it.label.as_str()).frame(false);
                if let Some(sc) = &it.shortcut {
                    b = b.shortcut_text(shortcut_text(sc));
                } else if !it.kind.is_empty() {
                    b = b.shortcut_text(crate::i18n::tr(&app.ui.language, it.kind));
                }
                // The button aligns its label by the layout it's in: left, like the rows with a shortcut.
                let size = egui::vec2(460.0, 22.0);
                let row =
                    ui.allocate_ui_with_layout(size, egui::Layout::left_to_right(egui::Align::Center).with_main_align(egui::Align::Min), |ui| {
                        ui.add(b.min_size(size))
                    });
                if row.inner.clicked() || (i == 0 && enter) {
                    run = Some((it.id.clone(), it.params.clone()));
                }
            }
        });
        if ui.input(|i| i.key_pressed(egui::Key::Escape)) {
            close = true;
        }
    });
    app.ui.palette = if close || run.is_some() { None } else { Some(q) };
    if let Some((id, params)) = run {
        let _ = app.run(&id, params);
    }
}

/// One Quick Apply entry: a style to apply or a command to run.
#[derive(Clone, Debug, PartialEq)]
pub struct QuickItem {
    pub label: String,
    pub id: String,
    pub params: Value,
    pub shortcut: Option<String>,
    /// "Paragraph Style", "Character Style", "Object Style", or "" for commands.
    pub kind: &'static str,
}

/// Quick Apply matches (styles first, as InDesign lists them), at most 14.
pub fn quick_apply_items(session: &designcraft_engine::Session, query: &str, cjk: bool) -> Vec<QuickItem> {
    let ql = query.to_lowercase();
    let hit = |label: &str, id: &str| quick_match(&ql, &[label, id]);
    let mut out = Vec::new();
    if let Some(st) = session.active() {
        let styles = &st.doc.styles;
        let mut add = |name: &str, id: &str, kind: &'static str| {
            if name.starts_with('[') || !hit(name, "") {
                return;
            }
            out.push(QuickItem { label: name.to_string(), id: id.into(), params: json!({"name": name}), shortcut: None, kind });
        };
        for p in &styles.paragraph {
            add(&p.name, "style.paragraph.apply", "Paragraph Style");
        }
        for c in &styles.character {
            add(&c.name, "style.character.apply", "Character Style");
        }
        for o in &styles.object {
            add(&o.name, "style.object.apply", "Object Style");
        }
    }
    // Styles need a query, else they would crowd out the commands.
    if ql.is_empty() {
        out.clear();
    }
    let commands = quick_commands().filter(|(id, l, _)| hit(l, id) && (cjk || !CJK_ONLY.contains(id))).map(|(id, l, sc)| QuickItem {
        label: l.to_string(),
        id: id.to_string(),
        params: json!({}),
        shortcut: sc.map(str::to_string),
        kind: "",
    });
    out.extend(commands);
    out.truncate(14);
    out
}

/// UI commands search leaves out: they only run with a parameter search can't supply and have no
/// dialog to ask for it, or without one they reset a setting. Each stays in the menu, panel or
/// dialog that gives it the value (the workspace menus, Keyboard Shortcuts, Preferences…).
const NOT_SEARCHED: &[&str] = &[
    // The workspace to switch to or delete.
    "window.workspace",
    "window.deleteWorkspace",
    // A command and its shortcut, a menu item, a shortcut set; Reset Shortcuts clears every
    // custom shortcut, and the Keyboard Shortcuts dialog offers it.
    "window.setShortcut",
    "window.resetShortcuts",
    "window.importShortcuts",
    "window.hideMenuItem",
    // Without a value these reset the interface to 100% and the dark theme.
    "window.uiScale",
    "window.brightness",
    // A panel, and where it goes.
    "window.floatPanel",
    "window.dockPanel",
    "window.panel.layout",
    "window.panel.move",
    "window.panel.float",
    "window.panel.dock",
    "window.panel.activate",
    "window.panel.close",
    "window.panel.closeGroup",
    // A window edge, an app page, a recent file, a swatch or color group.
    "window.resize",
    "help.app",
    "app.openRecent",
    "app.swatchOptionsDialog",
    "app.colorGroupOptionsDialog",
    // Without a value these clear the flattener preset, go to Normal screen mode or 100% zoom.
    "app.flattener",
    "view.screenMode",
    "view.zoom",
    // Where to pin the Contextual Task Bar.
    "window.taskBarPin",
    // Shared docking commands DesignCraft doesn't provide, those that need a panel or a name,
    // and older ids that share a label with a searched docking command.
    "window.panel",
    "window.panel.window",
    "window.workspace.lock",
    "window.panel.closeOthers",
    "window.panel.minimize",
    "window.panel.iconic",
    "window.workspace.new",
    "window.workspace.delete",
    "ui.dock",
    "ui.dock.reset",
    "window.resetWorkspace",
];

/// The commands Quick Apply and the application bar's search find: (id, label, default shortcut).
pub(super) fn quick_commands() -> impl Iterator<Item = (&'static str, &'static str, Option<&'static str>)> {
    designcraft_engine::command_specs()
        .iter()
        .filter(|c| !c.menu.is_empty() || c.shortcut.is_some())
        .map(|c| (c.id, c.label, c.shortcut))
        .chain(UI_COMMANDS.iter().filter(|c| !NOT_SEARCHED.contains(&c.0)).map(|c| (c.0, c.1, c.2)))
}

/// Whether any of `texts` contains the lowercased query (an empty query matches everything).
fn quick_match(query_lower: &str, texts: &[&str]) -> bool {
    query_lower.is_empty() || texts.iter().any(|t| t.to_lowercase().contains(query_lower))
}

/// The application bar's search: the commands whose label in the interface language, English
/// label or id contains `query`, labelled in the interface language with the shortcut in effect.
/// Nothing for an empty query; at most 14.
pub fn command_search(app: &DesignApp, query: &str) -> Vec<QuickItem> {
    let ql = query.trim().to_lowercase();
    if ql.is_empty() {
        return Vec::new();
    }
    let lang = app.ui.language.as_str();
    let mut out: Vec<QuickItem> = Vec::new();
    for (id, label, _) in quick_commands() {
        let shown = crate::i18n::tr(lang, label);
        if out.len() >= 14 || !self::shown(app, id) || out.iter().any(|it| it.id == id) || !quick_match(&ql, &[shown, label, id]) {
            continue;
        }
        out.push(QuickItem { label: shown.to_string(), id: id.to_string(), params: Value::Null, shortcut: shortcut_of(app, id), kind: "" });
    }
    out
}
