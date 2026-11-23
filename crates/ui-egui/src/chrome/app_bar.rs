use super::{DesignApp, Sense, json};

pub(super) fn search_id() -> egui::Id {
    egui::Id::unique("app_bar_search")
}

/// Whether the application bar's search field had the keyboard last frame. Escape takes egui's
/// focus away before any widget runs, so that press must not reach the global shortcuts.
pub fn search_had_focus(ctx: &egui::Context) -> bool {
    if craft_ui_widgets::runtime::focus::has(ctx, search_id()) {
        return true;
    }
    ctx.memory(|m| m.had_focus_last_frame(search_id()))
}

/// The search field's state between frames: the query, the highlighted match and whether the matches
/// are shown.
#[derive(Clone, Default)]
struct SearchState {
    query: String,
    highlight: usize,
    open: bool,
}

/// Lay out the application bar: the shell's title bar with the menus, the command search and the workspace, Share and Discord controls.
pub fn app_bar(app: &mut DesignApp, ui: &mut egui::Ui) {
    framework::app_bar(app, ui);
}

/// Undecorated Windows windows need resize grips as well as caption controls.
pub(crate) fn resize_borders(app: &mut DesignApp, ui: &mut egui::Ui) {
    if ui.input(|i| i.viewport().maximized.unwrap_or(false) || i.viewport().fullscreen.unwrap_or(false)) {
        return;
    }
    let r = ui.ctx().content_rect();
    let edge = 4.0;
    let corner = 10.0;
    let rect = |x0, y0, x1, y1| egui::Rect::from_min_max(egui::pos2(x0, y0), egui::pos2(x1, y1));
    for (name, region, cursor) in [
        ("northWest", rect(r.left(), r.top(), r.left() + corner, r.top() + corner), egui::CursorIcon::ResizeNwSe),
        ("northEast", rect(r.right() - corner, r.top(), r.right(), r.top() + corner), egui::CursorIcon::ResizeNeSw),
        ("southWest", rect(r.left(), r.bottom() - corner, r.left() + corner, r.bottom()), egui::CursorIcon::ResizeNeSw),
        ("southEast", rect(r.right() - corner, r.bottom() - corner, r.right(), r.bottom()), egui::CursorIcon::ResizeNwSe),
        ("north", rect(r.left() + corner, r.top(), r.right() - corner, r.top() + edge), egui::CursorIcon::ResizeVertical),
        ("south", rect(r.left() + corner, r.bottom() - edge, r.right() - corner, r.bottom()), egui::CursorIcon::ResizeVertical),
        ("west", rect(r.left(), r.top() + corner, r.left() + edge, r.bottom() - corner), egui::CursorIcon::ResizeHorizontal),
        ("east", rect(r.right() - edge, r.top() + corner, r.right(), r.bottom() - corner), egui::CursorIcon::ResizeHorizontal),
    ] {
        let response = ui.interact(region, egui::Id::unique(("window_resize", name)), Sense::drag()).on_hover_cursor(cursor);
        if response.drag_started_by(egui::PointerButton::Primary) {
            let _ = app.run("window.resize", json!({"edge": name}));
            app.flush_window_commands(ui.ctx());
        }
    }
}

/// The application bar on the framework: the shell's title bar (the window's move band, caption buttons and the room for the
/// traffic lights follow the window's frame), the menu bar, and the command search, workspace menu, Share and Discord
/// buttons, with the same commands.
mod framework {
    use super::{DesignApp, SearchState, json, search_id};
    use crate::menus::{self, Item};
    use crate::panels::tabs::TabGlyph;
    use craft_ui_shell::shell::{Title, TitleBar};
    use craft_ui_widgets::buttons::{Button, IconButton};
    use craft_ui_widgets::contract::{Bind, Props as _, Show as _, ShowContainer as _, Trigger};
    use craft_ui_widgets::display::Divider;
    use craft_ui_widgets::menu::{Menu, MenuBar, MenuItem, MenuTitle};
    use craft_ui_widgets::popover::Popover;
    use craft_ui_widgets::primitives::IconSpec;
    use craft_ui_widgets::runtime::focus::{self, Initial};
    use craft_ui_widgets::style::Variant;
    use craft_ui_widgets::text::TextField;
    use std::cell::RefCell;

    /// What the bar asks for, run once its components are shown.
    enum Ask {
        Run(&'static str, serde_json::Value),
        Workspace(String),
        DeleteWorkspace(String),
        Command(String, serde_json::Value),
        Home,
        Share,
        ShowAllMenuItems,
    }

    type Asks<'a> = &'a RefCell<Vec<Ask>>;

    /// One menu's rows. Returns how many items were hidden (Edit › Menus).
    fn rows(app: &DesignApp, ui: &mut egui::Ui, items: &[Item], path: &str, asks: Asks<'_>) -> usize {
        let lang = &app.ui.language;
        let mut hidden = 0;
        for (i, it) in items.iter().enumerate() {
            match it {
                Item::Sep => {
                    if menus::has_shown_item(app, &items[i + 1..]) {
                        Divider::horizontal().show(ui);
                    }
                }
                Item::Sub(name, children) => {
                    let sub = format!("{path}/{name}");
                    let shown = crate::i18n::tr(lang, name).to_owned();
                    let _ = Menu::submenu_row(ui, shown, |ui, _| {
                        if sub == menus::OPEN_RECENT {
                            for recent in menus::recent_items(app) {
                                match recent {
                                    Item::Cmd { label, id, params, .. } => {
                                        let file = params["index"]
                                            .as_u64()
                                            .and_then(|n| app.session.prefs.recent_files.get(n as usize))
                                            .cloned()
                                            .unwrap_or_default();
                                        let gone = !crate::chrome::recent_file_stat(ui.ctx(), &file).exists;
                                        let row = MenuItem::new(label.clone()).id_salt(("recent", file.clone())).tooltip(file).enabled(true);
                                        // A file that is gone is dimmed (choosing it says so).
                                        let row = if gone { row.detail(crate::i18n::tr(lang, "Not found").to_owned()) } else { row };
                                        if row.show(ui).ran {
                                            asks.borrow_mut().push(Ask::Command(id.clone(), params.clone()));
                                        }
                                    }
                                    Item::Sep => {
                                        Divider::horizontal().show(ui);
                                    }
                                    Item::Sub(..) => {}
                                }
                            }
                        }
                        rows(app, ui, children, &sub, asks);
                    });
                }
                Item::Cmd { id, .. } if !menus::shown(app, id) => {}
                Item::Cmd { label, .. } if !app.ui.show_full_menus && app.ui.hidden_menu_items.contains(&menus::menu_key(path, label)) => hidden += 1,
                Item::Cmd { label, id, params, .. } => {
                    let text = match id.as_str() {
                        "window.workspace" => format!("[{}]", crate::i18n::workspace_name(lang, label)),
                        "window.resetWorkspace" => {
                            format!("{} {}", crate::i18n::tr(lang, "Reset"), crate::i18n::workspace_name(lang, &app.ui.workspace))
                        }
                        _ => crate::i18n::tr(lang, label).to_owned(),
                    };
                    let mut row = MenuItem::new(text).id_salt(("cmd", path.to_owned(), i)).enabled(menus::menu_enabled(app, id));
                    // A command that is not a toggle has no mark.
                    if let Some(on) = menus::checked(app, id, params) {
                        row = row.check(on);
                    }
                    if let Some(sc) = menus::shortcut_label(app, id, params) {
                        row = row.shortcut(sc);
                    }
                    if row.show(ui).ran {
                        asks.borrow_mut().push(Ask::Command(id.clone(), params.clone()));
                    }
                }
            }
        }
        hidden
    }

    /// The menu bar: a title for each menu of the tree (the Windows and Linux in-window bar; macOS uses the native menu).
    fn menu_bar(app: &DesignApp, ui: &mut egui::Ui, asks: Asks<'_>) {
        let lang = app.ui.language.clone();
        let titles: Vec<MenuTitle<'_>> = menus::menu_tree()
            .into_iter()
            .map(|(menu, entries)| {
                let label = crate::i18n::tr(&lang, menu).to_owned();
                MenuTitle::new(label, move |ui: &mut egui::Ui| {
                    let hidden = rows(app, ui, &entries, menu, asks);
                    if hidden > 0 && !app.ui.show_full_menus {
                        Divider::horizontal().show(ui);
                        if MenuItem::new(crate::i18n::tr(&app.ui.language, "Show All Menu Items").to_owned()).id_salt("show_all").show(ui).ran {
                            asks.borrow_mut().push(Ask::ShowAllMenuItems);
                        }
                    }
                })
            })
            .collect();
        MenuBar::new(crate::i18n::tr(&lang, "Menu").to_owned(), titles).show(ui);
    }

    /// The command search: finds menu commands by their labels. ↑/↓ move through the matches; Return or a click runs one as its
    /// menu item would; running one or Escape clears the field. While it has the keyboard, no key reaches the shortcuts or the document.
    fn search(app: &DesignApp, ui: &mut egui::Ui, asks: Asks<'_>) {
        let lang = app.ui.language.clone();
        let id = search_id();
        let state_id = id.with("state");
        let mut st: SearchState = ui.data(|d| d.get_temp(state_id)).unwrap_or_default();
        let focused = focus::has(ui.ctx(), id);
        let none = egui::Modifiers::NONE;
        let (down, up, enter, escape) = if focused {
            ui.input_mut(|i| {
                (
                    i.consume_key(none, egui::Key::ArrowDown),
                    i.consume_key(none, egui::Key::ArrowUp),
                    i.consume_key(none, egui::Key::Enter),
                    i.consume_key(none, egui::Key::Escape),
                )
            })
        } else {
            (false, false, false, false)
        };
        let before = st.query.clone();
        let hint = crate::i18n::tr(&lang, "Search").to_owned();
        let field = TextField::new(hint.clone(), Bind::bound(&mut st.query))
            .id(id)
            .placeholder(hint)
            .tooltip(crate::i18n::tr(&lang, "Search commands").to_owned())
            .limit(None)
            .size(craft_theme::ControlSize::Sm)
            .show(ui);
        if st.query != before {
            st.highlight = 0;
            st.open = true;
        }
        if focused && !st.open && !st.query.is_empty() {
            st.open = true;
        }
        let items = if st.open { menus::command_search(app, &st.query) } else { Vec::new() };
        let last = items.len().saturating_sub(1);
        if down {
            st.highlight = st.highlight.saturating_add(1);
        }
        if up {
            st.highlight = st.highlight.saturating_sub(1);
        }
        st.highlight = st.highlight.min(last);
        let mut run: Option<(String, serde_json::Value)> = None;
        if enter && let Some(it) = items.get(st.highlight).filter(|it| menus::menu_enabled(app, &it.id)) {
            run = Some((it.id.clone(), serde_json::Value::Null));
        }
        if st.open && !st.query.trim().is_empty() {
            let mut open = true;
            let _ = Popover::new(crate::i18n::tr(&lang, "Search commands").to_owned(), Bind::bound(&mut open))
                .id(id.with("matches"))
                .opened_by(&field.response)
                .initial_focus(Initial::Keep)
                .show(ui, |ui, _| {
                    ui.set_width(300.0);
                    let _ = Menu::inline(crate::i18n::tr(&lang, "Search commands").to_owned()).show(ui, |ui, _| {
                        if items.is_empty() {
                            craft_ui_widgets::display::Text::new(crate::i18n::tr(&lang, "No matching commands").to_owned())
                                .shade(craft_ui_widgets::display::Shade::Muted)
                                .show(ui);
                        }
                        for (k, it) in items.iter().enumerate() {
                            let mut row = MenuItem::new(it.label.clone())
                                .id_salt(("match", k))
                                .radio_mark(k == st.highlight)
                                .enabled(menus::menu_enabled(app, &it.id));
                            if let Some(sc) = &it.shortcut {
                                row = row.shortcut(menus::shortcut_text(sc));
                            }
                            if row.show(ui).ran {
                                run = Some((it.id.clone(), serde_json::Value::Null));
                            }
                        }
                    });
                });
            if !open {
                st.open = false;
            }
        }
        if escape || run.is_some() {
            st.query.clear();
            st.open = false;
        } else if !focused {
            // A click elsewhere or Tab took the keyboard: the matches close, the query stays.
            st.open = false;
        }
        ui.data_mut(|d| d.insert_temp(state_id, st));
        if let Some((cmd, params)) = run {
            asks.borrow_mut().push(Ask::Command(cmd, params));
        }
    }

    /// The workspace menu: the workspaces, Reset, New Workspace and Delete Workspace.
    fn workspace_menu(app: &DesignApp, ui: &mut egui::Ui, asks: Asks<'_>) {
        let lang = app.ui.language.clone();
        let tr = |s: &str| crate::i18n::tr(&lang, s).to_string();
        let current = app.ui.workspace.clone();
        let shown = crate::i18n::workspace_name(&lang, &current);
        let opener = Button::new(tr("Workspace")).painted(format!("{shown} ▾")).id_salt("app_bar_workspace").variant(Variant::GHOST).show(ui);
        let customs: Vec<String> = app.ui.custom_workspaces.iter().map(|w| w.name.clone()).collect();
        let _ = Menu::new(tr("Workspace")).id_salt("app_bar_workspace_menu").opened_by(&opener.response, Trigger::Click).show(ui, |ui, _| {
            for w in menus::WORKSPACES.iter().map(|w| w.to_string()).chain(customs.iter().cloned()) {
                let row = MenuItem::new(crate::i18n::workspace_name(&lang, &w).to_owned()).id_salt(("workspace", w.clone())).check(w == current);
                if row.show(ui).ran {
                    asks.borrow_mut().push(Ask::Workspace(w.clone()));
                }
            }
            Divider::horizontal().show(ui);
            let reset = format!("{} {}", tr("Reset"), crate::i18n::workspace_name(&lang, &current));
            if MenuItem::new(reset).id_salt("reset").show(ui).ran {
                asks.borrow_mut().push(Ask::Run("window.resetWorkspace", json!({})));
            }
            if MenuItem::new(tr("New Workspace…")).id_salt("new").show(ui).ran {
                asks.borrow_mut().push(Ask::Run("window.newWorkspace", json!({})));
            }
            if !customs.is_empty() {
                let _ = Menu::submenu_row(ui, tr("Delete Workspace"), |ui, _| {
                    for w in &customs {
                        if MenuItem::new(w.clone()).id_salt(("delete", w.clone())).show(ui).ran {
                            asks.borrow_mut().push(Ask::DeleteWorkspace(w.clone()));
                        }
                    }
                });
            }
        });
    }

    pub(super) fn app_bar(app: &mut DesignApp, ui: &mut egui::Ui) {
        let t = crate::theme::Tokens::get(ui.ctx());
        let lang = app.ui.language.clone();
        let tr = |s: &str| crate::i18n::tr(&lang, s).to_string();
        let title = match app.session.active() {
            Some(d) => (format!("DesignCraft — {}", crate::rtl::isolate(&d.title())), d.is_dirty()),
            None => ("DesignCraft".to_string(), false),
        };
        let title = if title.1 { Title::unsaved(title.0) } else { Title::new(title.0) };
        let native_menu = app.native_menu;
        let no_doc = app.session.active().is_none();
        let asks: RefCell<Vec<Ask>> = RefCell::new(Vec::new());
        let app_ref: &DesignApp = app;
        let mut close = false;
        let resp = egui::Panel::top("app_bar").frame(egui::Frame::NONE).resizable(false).show(ui, |ui| {
            let out = TitleBar::new().show_titled(
                ui,
                title,
                |ui, _| {
                    let home = {
                        let face = TabGlyph("home");
                        IconButton::new(IconSpec::glyph(&face))
                            .id_salt("app_bar_home")
                            .label(tr("Home"))
                            .tooltip(tr("Home"))
                            .selected(no_doc)
                            .show(ui)
                    };
                    if home.clicked() {
                        asks.borrow_mut().push(Ask::Home);
                    }
                    if !native_menu {
                        menu_bar(app_ref, ui, &asks);
                    }
                },
                |ui, _| {
                    search(app_ref, ui, &asks);
                    workspace_menu(app_ref, ui, &asks);
                    let share = {
                        let face = TabGlyph("share");
                        IconButton::new(IconSpec::glyph(&face)).id_salt("app_bar_share").label(tr("Share")).tooltip(tr("Share")).show(ui)
                    };
                    if share.clicked() {
                        asks.borrow_mut().push(Ask::Share);
                    }
                    // Always one click away: the ArtCraft community Discord.
                    if crate::about::discord_framework(ui, "Discord".to_owned(), "app_bar_discord") {
                        asks.borrow_mut().push(Ask::Run("help.discord", json!({})));
                    }
                },
            );
            close = out.close_requested;
        });
        let r = resp.response.rect;
        ui.painter().line_segment([egui::pos2(r.min.x, r.max.y), egui::pos2(r.max.x, r.max.y)], egui::Stroke::new(1.0, t.border));
        for ask in asks.into_inner() {
            match ask {
                Ask::Run(id, params) => {
                    let _ = app.run(id, params);
                }
                Ask::Workspace(name) => {
                    let _ = app.run("window.workspace", json!({"name": name}));
                }
                Ask::DeleteWorkspace(name) => {
                    let _ = app.run("window.deleteWorkspace", json!({"name": name}));
                }
                Ask::Command(id, params) => menus::activate(app, &id, &params),
                Ask::Home => app.session_home(),
                Ask::Share => app.status(crate::i18n::tr(&lang, "Export a PDF, IDML or package to share — no cloud account needed.")),
                Ask::ShowAllMenuItems => app.ui.show_full_menus = true,
            }
        }
        // Close never closes the window by itself: the app runs its unsaved-documents path.
        if close {
            let _ = app.run("window.close", json!({}));
        }
        app.flush_window_commands(ui.ctx());
    }
}
