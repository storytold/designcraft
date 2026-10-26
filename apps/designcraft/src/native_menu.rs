//! macOS: DesignCraft's menu tree as the native system menu bar (like InDesign on the Mac).
//! Items dispatch through the same command path as the in-window menus; enablement and check
//! marks are refreshed a few times per second.

use std::collections::HashMap;
use std::str::FromStr;
use std::sync::mpsc::{Receiver, channel};

use designcraft_ui_egui::DesignApp;
use designcraft_ui_egui::menus::{self, AppMenuItem, Item};
use muda::accelerator::Accelerator;
use muda::{CheckMenuItem, Menu, MenuEvent, MenuItem, PredefinedMenuItem, Submenu};
use serde_json::Value;

enum Handle {
    Plain(MenuItem),
    Check(CheckMenuItem),
}

pub struct NativeMenu {
    _menu: Menu,
    items: HashMap<String, (String, Value, Handle)>,
    events: Receiver<MenuEvent>,
    last_refresh: f64,
    /// The interface language the titles are in, whether CJK features are shown and whether the
    /// Keyboard Shortcuts dialog records keys (the menu then has no key equivalents, so the keys
    /// reach the dialog): the menu is rebuilt when any of them changes.
    shown_for: (String, bool, bool),
    /// File › Open Recent, rebuilt when the list of recent files changes.
    recent: Option<RecentMenu>,
    /// The commands with key equivalents and the shortcut each was given: the menu is rebuilt
    /// when one changes, so a shortcut moved to another command leaves its old item.
    keys: Vec<(String, Option<String>)>,
}

struct RecentMenu {
    submenu: Submenu,
    /// The files shown, and the native items standing for them (with their trailing separator).
    shown: Vec<String>,
    ids: Vec<String>,
    separator: Option<PredefinedMenuItem>,
}

/// Accelerator for "Cmd+Shift+]" (modifier-less shortcuts stay in the app so typing works).
fn accel(sc: &str) -> Option<Accelerator> {
    if !(sc.contains("Cmd") || sc.contains("Ctrl") || sc.contains("Alt")) {
        return None;
    }
    Accelerator::from_str(&sc.replace("Cmd", "CMD").replace("Alt", "ALT").replace("Shift", "SHIFT").replace("Ctrl", "CTRL")).ok()
}

/// A menu or item label in the interface language, as the native menu bar takes it.
fn menu_title(app: &DesignApp, label: &str) -> String {
    menus::native_label(designcraft_ui_egui::i18n::tr(&app.ui.language, label))
}

/// A command's shortcut in effect (the user's, else the default), as the in-window menus show it.
fn shortcut(app: &DesignApp, id: &str) -> Option<String> {
    menus::shortcut_of(app, id)
}

impl NativeMenu {
    pub fn install(app: &mut DesignApp, ctx: &egui::Context) -> Self {
        // A click in the menu bar reaches the app with no input over the window, so nothing would
        // draw the frame that runs the command until the pointer or a key comes back: wake egui.
        // muda keeps the first handler for the life of the process, so this channel serves every
        // menu bar built later (`build_bar`).
        let (tx, events) = channel();
        let notify = app.native_action_notifier(ctx.clone());
        MenuEvent::set_event_handler(Some(move |e: MenuEvent| {
            if tx.send(e).is_ok() {
                notify();
            }
        }));
        Self::build_bar(app, events)
    }

    /// The menu bar for the current language and settings, fed by `events`.
    fn build_bar(app: &mut DesignApp, events: Receiver<MenuEvent>) -> Self {
        let menu = Menu::new();
        let mut items = HashMap::new();
        let mut counter = 0usize;
        let mut recent = None;
        // Application menu: our own About splash (with the community links) instead of the stock
        // panel, and Settings… (Edit › Preferences… on the other platforms).
        let app_menu = Submenu::new("DesignCraft", true);
        for entry in menus::MAC_APP_MENU {
            let _ = match *entry {
                AppMenuItem::Cmd { id, label, command } => {
                    let sc = shortcut(app, command).filter(|_| !app.shortcut_recording()).as_deref().and_then(accel);
                    let item = MenuItem::with_id(id, menu_title(app, label), true, sc);
                    let appended = app_menu.append(&item);
                    items.insert(id.into(), (command.into(), Value::Null, Handle::Plain(item)));
                    appended
                }
                AppMenuItem::Language => {
                    let sub = Submenu::new(menu_title(app, "Language"), true);
                    for (code, name) in designcraft_ui_egui::i18n::LANGUAGES {
                        let id = format!("dc-language-{}", if code.is_empty() { "en" } else { code });
                        let params = serde_json::json!({ "lang": code });
                        let checked = menus::checked(app, "app.language", &params).unwrap_or(false);
                        let item = CheckMenuItem::with_id(id.clone(), menus::native_label(name), true, checked, None);
                        let _ = sub.append(&item);
                        items.insert(id, ("app.language".into(), params, Handle::Check(item)));
                    }
                    app_menu.append(&sub)
                }
                AppMenuItem::Sep => app_menu.append(&PredefinedMenuItem::separator()),
                AppMenuItem::Services => app_menu.append(&PredefinedMenuItem::services(None)),
                AppMenuItem::Hide => app_menu.append(&PredefinedMenuItem::hide(None)),
                AppMenuItem::HideOthers => app_menu.append(&PredefinedMenuItem::hide_others(None)),
                AppMenuItem::ShowAll => app_menu.append(&PredefinedMenuItem::show_all(None)),
            };
        }
        let _ = menu.append(&app_menu);
        for (title, entries) in menus::native_menu_tree() {
            let sub = Submenu::new(menu_title(app, title), true);
            build(app, &sub, &entries, &mut items, &mut counter, title, &mut recent);
            let _ = menu.append(&sub);
        }
        menu.init_for_nsapp();
        app.native_menu = true;
        let keys: Vec<(String, Option<String>)> =
            items.values().filter(|(_, p, _)| p.is_null()).map(|(id, _, _)| (id.clone(), shortcut(app, id))).collect();
        let recording = app.shortcut_recording();
        app.native_shortcuts = keys
            .iter()
            .filter_map(|(id, sc)| sc.as_ref().filter(|sc| !recording && accel(sc).is_some()).map(|sc| (id.clone(), sc.clone())))
            .collect();
        let recent = recent.map(|submenu| RecentMenu { submenu, shown: Vec::new(), ids: Vec::new(), separator: None });
        Self {
            _menu: menu,
            items,
            events,
            last_refresh: 0.0,
            shown_for: (app.ui.language.clone(), designcraft_ui_egui::cjk_features(app), app.shortcut_recording()),
            recent,
            keys,
        }
    }

    /// Put the current recent files at the top of File › Open Recent when they changed.
    fn refresh_recent(&mut self, app: &DesignApp) {
        let Some(r) = self.recent.as_mut() else { return };
        if r.shown == app.session.prefs.recent_files {
            return;
        }
        for id in r.ids.drain(..) {
            if let Some((_, _, Handle::Plain(item))) = self.items.remove(&id) {
                let _ = r.submenu.remove(&item);
            }
        }
        if let Some(sep) = r.separator.take() {
            let _ = r.submenu.remove(&sep);
        }
        let mut position = 0;
        for it in menus::recent_items(app) {
            match it {
                Item::Cmd { label, id, params, .. } => {
                    let mid = format!("dc-recent{position}");
                    let item = MenuItem::with_id(mid.clone(), menus::native_label(&label), true, None);
                    let _ = r.submenu.insert(&item, position);
                    self.items.insert(mid.clone(), (id, params, Handle::Plain(item)));
                    r.ids.push(mid);
                }
                Item::Sep => {
                    let sep = PredefinedMenuItem::separator();
                    let _ = r.submenu.insert(&sep, position);
                    r.separator = Some(sep);
                }
                Item::Sub(..) => continue,
            }
            position += 1;
        }
        r.shown = app.session.prefs.recent_files.clone();
    }

    pub fn poll(&mut self, app: &mut DesignApp, ctx: &egui::Context) {
        app.poll_native_actions(ctx, || {
            loop {
                let ev = self.events.try_recv().ok()?;
                if let Some((cmd, params, _)) = self.items.get(ev.id.as_ref()) {
                    return Some((cmd.clone(), params.clone()));
                }
            }
        });
        // Interface Language and Show CJK Features (set in Preferences or by `app.language`, or put
        // back by Cancel): the system menu bar follows at once, like the panels (the new menu
        // replaces the old one as the application's menu before that one is dropped). While the
        // Keyboard Shortcuts dialog records, the bar has no key equivalents.
        if app.ui.language != self.shown_for.0
            || designcraft_ui_egui::cjk_features(app) != self.shown_for.1
            || app.shortcut_recording() != self.shown_for.2
        {
            let events = std::mem::replace(&mut self.events, channel().1);
            *self = Self::build_bar(app, events);
        }
        let now = designcraft_ui_egui::now_ms();
        if now - self.last_refresh < 250.0 {
            return;
        }
        self.last_refresh = now;
        // A shortcut set in the Keyboard Shortcuts dialog: macOS gives key equivalents to the
        // menu bar before the window sees the key, so the bar must carry the new keys.
        if self.keys.iter().any(|(id, sc)| shortcut(app, id) != *sc) {
            let events = std::mem::replace(&mut self.events, channel().1);
            *self = Self::build_bar(app, events);
            return;
        }
        self.refresh_recent(app);
        for (cmd, params, h) in self.items.values() {
            let en = menus::native_menu_enabled(app, ctx, cmd);
            match h {
                Handle::Plain(i) => i.set_enabled(en),
                Handle::Check(c) => {
                    c.set_enabled(en);
                    c.set_checked(menus::checked(app, cmd, params).unwrap_or(false));
                }
            }
        }
    }
}

fn build(
    app: &DesignApp,
    parent: &Submenu,
    entries: &[Item],
    items: &mut HashMap<String, (String, Value, Handle)>,
    counter: &mut usize,
    path: &str,
    recent: &mut Option<Submenu>,
) {
    for e in entries {
        match e {
            Item::Sep => {
                let _ = parent.append(&PredefinedMenuItem::separator());
            }
            Item::Sub(label, children) => {
                let sub = Submenu::new(menu_title(app, label), true);
                let path = format!("{path}/{label}");
                build(app, &sub, children, items, counter, &path, recent);
                if path == menus::OPEN_RECENT {
                    *recent = Some(sub.clone());
                }
                let _ = parent.append(&sub);
            }
            // Hidden CJK commands; `NativeMenu::poll` rebuilds the bar when Show CJK Features changes.
            Item::Cmd { id, .. } if !menus::shown(app, id) => {}
            Item::Cmd { label, id, params, .. } => {
                *counter += 1;
                let mid = format!("dc{counter}");
                let sc = if params.is_null() && !app.shortcut_recording() { shortcut(app, id).as_deref().and_then(accel) } else { None };
                let label = menu_title(app, label);
                let handle = if menus::checked(app, id, params).is_some() {
                    let c = CheckMenuItem::with_id(mid.clone(), label, true, false, sc);
                    let _ = parent.append(&c);
                    Handle::Check(c)
                } else {
                    let i = MenuItem::with_id(mid.clone(), label, true, sc);
                    let _ = parent.append(&i);
                    Handle::Plain(i)
                };
                items.insert(mid, (id.clone(), params.clone(), handle));
            }
        }
    }
}
