//! macOS: DesignCraft's menu tree as the native system menu bar (like InDesign on the Mac).
//! Items dispatch through the same command path as the in-window menus; enablement and check
//! marks are refreshed a few times per second.

use std::collections::HashMap;
use std::str::FromStr;

use designcraft_ui_egui::DesignApp;
use designcraft_ui_egui::menus::{self, Item};
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
    last_refresh: f64,
}

/// Accelerator for "Cmd+Shift+]" (modifier-less shortcuts stay in the app so typing works).
fn accel(sc: &str) -> Option<Accelerator> {
    if !(sc.contains("Cmd") || sc.contains("Ctrl") || sc.contains("Alt")) {
        return None;
    }
    Accelerator::from_str(&sc.replace("Cmd", "CMD").replace("Alt", "ALT").replace("Shift", "SHIFT").replace("Ctrl", "CTRL")).ok()
}

impl NativeMenu {
    pub fn install(app: &mut DesignApp) -> Self {
        let menu = Menu::new();
        let mut items = HashMap::new();
        let mut counter = 0usize;
        // Application menu.
        let app_menu = Submenu::new("DesignCraft", true);
        // Our own About splash (with the community links) instead of the stock panel.
        let about = MenuItem::with_id("dc-about", "About DesignCraft", true, None);
        let discord = MenuItem::with_id("dc-discord", "Join the ArtCraft Discord…", true, None);
        let _ = app_menu.append_items(&[
            &about,
            &discord,
            &PredefinedMenuItem::separator(),
            &PredefinedMenuItem::services(None),
            &PredefinedMenuItem::separator(),
            &PredefinedMenuItem::hide(None),
            &PredefinedMenuItem::hide_others(None),
            &PredefinedMenuItem::show_all(None),
            &PredefinedMenuItem::separator(),
            &PredefinedMenuItem::quit(None),
        ]);
        items.insert("dc-about".into(), ("help.about".into(), serde_json::Value::Null, Handle::Plain(about)));
        items.insert("dc-discord".into(), ("help.discord".into(), serde_json::Value::Null, Handle::Plain(discord)));
        let _ = menu.append(&app_menu);
        for (title, entries) in menus::menu_tree() {
            let sub = Submenu::new(designcraft_ui_egui::i18n::tr(&app.ui.language, title), true);
            build(app, &sub, &entries, &mut items, &mut counter);
            let _ = menu.append(&sub);
        }
        menu.init_for_nsapp();
        app.native_menu = true;
        app.native_shortcuts = items
            .values()
            .filter_map(|(id, p, _)| {
                let sc = menus::ui_label(id).and_then(|(_, sc)| sc).or_else(|| designcraft_engine::find_command(id).and_then(|c| c.shortcut))?;
                (p.is_null() && accel(sc).is_some()).then(|| id.clone())
            })
            .collect();
        Self { _menu: menu, items, last_refresh: 0.0 }
    }

    pub fn poll(&mut self, app: &mut DesignApp, ctx: &egui::Context) {
        while let Ok(ev) = MenuEvent::receiver().try_recv() {
            if let Some((cmd, params, _)) = self.items.get(ev.id.as_ref()) {
                menus::activate_native(app, ctx, &cmd.clone(), &params.clone());
            }
        }
        let now = designcraft_ui_egui::now_ms();
        if now - self.last_refresh < 250.0 {
            return;
        }
        self.last_refresh = now;
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

fn build(app: &DesignApp, parent: &Submenu, entries: &[Item], items: &mut HashMap<String, (String, Value, Handle)>, counter: &mut usize) {
    for e in entries {
        match e {
            Item::Sep => {
                let _ = parent.append(&PredefinedMenuItem::separator());
            }
            Item::Sub(label, children) => {
                let sub = Submenu::new(designcraft_ui_egui::i18n::tr(&app.ui.language, label), true);
                build(app, &sub, children, items, counter);
                let _ = parent.append(&sub);
            }
            // The menu bar is built once, so Show CJK Features applies here from the next launch.
            Item::Cmd { id, .. } if !menus::shown(app, id) => {}
            Item::Cmd { label, id, params, shortcut } => {
                *counter += 1;
                let mid = format!("dc{counter}");
                let sc = if params.is_null() { shortcut.and_then(accel) } else { None };
                let label = designcraft_ui_egui::i18n::tr(&app.ui.language, label);
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
