//! Object Library panel: the open library's items; add the selection, place or delete an item;
//! New / Open library.

use serde_json::json;

use crate::DesignApp;
use crate::app::file_dialogs::Bound;
use crate::theme::Tokens;

pub fn show(app: &mut DesignApp, ui: &mut egui::Ui) {
    let t = Tokens::get(ui.ctx());
    #[cfg(not(target_arch = "wasm32"))]
    let framework = app.framework_installed;
    #[cfg(target_arch = "wasm32")]
    let framework = false;
    let action = |ui: &mut egui::Ui, id: egui::Id, label: &str, enabled: bool, small: bool| {
        #[cfg(not(target_arch = "wasm32"))]
        if framework {
            use craft_ui_widgets::contract::{Props as _, Show as _};
            return craft_ui_widgets::buttons::Button::new(label.to_owned())
                .id(id)
                .enabled(enabled)
                .size(if small { craft_theme::ControlSize::Sm } else { craft_theme::ControlSize::Md })
                .show(ui)
                .clicked();
        }
        #[cfg(target_arch = "wasm32")]
        let _ = (id, framework);
        let button = egui::Button::new(crate::rtl::widget(ui, label));
        ui.add_enabled(enabled, if small { button.small() } else { button }).clicked()
    };
    // Content Collector conveyor.
    let conveyor = app.session.conveyor.iter().map(|c| c.0.clone()).collect::<Vec<_>>();
    crate::rtl::label(ui, egui::RichText::new(format!("{} ({})", crate::i18n::tr(&app.ui.language, "Conveyor"), conveyor.len())).strong());
    if conveyor.is_empty() {
        crate::rtl::label(
            ui,
            crate::rtl::widget(
                ui,
                egui::RichText::new(crate::i18n::tr(
                    &app.ui.language,
                    "Collect objects with the Content Collector (B); place them with the Content Placer.",
                ))
                .size(10.5)
                .color(t.text_dim),
            ),
        );
    } else {
        crate::rtl::label(ui, egui::RichText::new(conveyor.join(", ")).size(10.5));
        let clear_id = ui.scope_id().with("library_conveyor_clear");
        if action(ui, clear_id, crate::i18n::tr(&app.ui.language, "Clear"), true, true) {
            let _ = app.run("conveyor.clear", json!({}));
        }
    }
    ui.separator();
    ui.horizontal(|ui| {
        let new_id = ui.scope_id().with("object_library_new");
        let open_id = ui.scope_id().with("object_library_open");
        if action(ui, new_id, crate::i18n::tr(&app.ui.language, "New Library…"), true, false) {
            let asked = app.ask_save("Library.dclib", Bound::App, |app, path| app.run("library.new", json!({"path": path})));
            report(app, "Library", asked);
        }
        if action(ui, open_id, crate::i18n::tr(&app.ui.language, "Open…"), true, false) {
            let asked = app.ask_open("library", Bound::App, |app, path| {
                app.run("library.open", json!({"path": path})).or_else(|e| report_err(app, "Library", e))
            });
            report(app, "Library", asked);
        }
    });
    let Ok(items) = app.session.execute("library.list", &json!({})) else {
        crate::rtl::label(ui, egui::RichText::new(crate::i18n::tr(&app.ui.language, "No library open.")).size(11.0).color(t.text_dim));
        return;
    };
    let has_sel = app.session.active().is_some_and(|d| !d.selection.items.is_empty());
    let add_id = ui.scope_id().with("object_library_add");
    if action(ui, add_id, crate::i18n::tr(&app.ui.language, "Add Selection"), has_sel, false)
        && let Err(e) = app.run("library.add", json!({}))
    {
        app.status_error("Library", e);
    }
    ui.separator();
    let items = items.as_array().cloned().unwrap_or_default();
    if items.is_empty() {
        crate::rtl::label(
            ui,
            crate::rtl::widget(
                ui,
                egui::RichText::new(crate::i18n::tr(&app.ui.language, "The library is empty. Select objects and Add Selection."))
                    .size(11.0)
                    .color(t.text_dim),
            ),
        );
    }
    for it in items {
        let i = it["index"].clone();
        // The list's actual command identity, not the display name or an invented item key.
        let item_id = ui.scope_id().with(("object_library_item", i.to_string()));
        ui.horizontal(|ui| {
            crate::rtl::label(ui, it["name"].as_str().unwrap_or(""));
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if action(ui, item_id.with("delete"), crate::i18n::tr(&app.ui.language, "Delete"), true, true) {
                    let _ = app.run("library.remove", json!({"index": i}));
                }
                if action(ui, item_id.with("place"), crate::i18n::tr(&app.ui.language, "Place"), true, true)
                    && let Err(e) = app.run("library.place", json!({"index": i}))
                {
                    app.status_error("Library", e);
                }
            });
        });
    }
}

/// A dialog that couldn't be asked for, in the status line as `feature`'s error.
fn report(app: &mut DesignApp, feature: &str, asked: Result<serde_json::Value, String>) {
    if let Err(e) = asked {
        app.status_error(feature, e);
    }
}

/// A command run with a dialog's answer failed: the status line names `feature`.
fn report_err(app: &mut DesignApp, feature: &str, e: String) -> Result<serde_json::Value, String> {
    app.status_error(feature, e);
    Ok(serde_json::Value::Null)
}

/// Scripts › Load…: the chosen script becomes a new, selected one. Read through the host's
/// reader, like every other file the UI opens: this crate has no file system of its own.
fn load_script(app: &mut DesignApp, path: &str, sel_id: egui::Id) {
    let read = app.services.read.as_mut().map_or_else(|| Err("no file access".to_string()), |r| r(path));
    match read.and_then(|bytes| String::from_utf8(bytes).map_err(|e| e.to_string())) {
        Ok(text) => {
            let name = std::path::Path::new(path).file_stem().map(|n| n.to_string_lossy().to_string()).unwrap_or_else(|| "Script".into());
            app.ui.scripts.push((name, text));
            let sel = app.ui.scripts.len().saturating_sub(1);
            if let Some(ctx) = &app.egui_ctx {
                ctx.data_mut(|d| d.insert_temp(sel_id, sel));
            }
        }
        Err(e) => app.status_error("Scripts", e),
    }
}

/// SDK Book faces keep their painted captions while exposing the complete action and chapter identity.
fn book_sdk_button(
    app: &DesignApp,
    ui: &mut egui::Ui,
    id: egui::Id,
    label: &str,
    painted: &str,
    small: bool,
    selected: Option<bool>,
) -> Option<bool> {
    #[cfg(not(target_arch = "wasm32"))]
    if app.framework_installed {
        use craft_ui_widgets::contract::{Props as _, Show as _};
        use craft_theme::ControlSize;
        let mut button = craft_ui_widgets::buttons::Button::new(label.to_owned()).painted(painted.to_owned()).id(id).tooltip(label.to_owned());
        if small {
            button = button.size(ControlSize::Sm);
        }
        if let Some(selected) = selected {
            button = button.selected(selected);
        }
        return Some(button.show(ui).clicked());
    }
    #[cfg(target_arch = "wasm32")]
    let _ = (app, ui, id, label, painted, small, selected);
    None
}

fn book_button(app: &DesignApp, ui: &mut egui::Ui, salt: &'static str, label: &str) -> bool {
    let target = app.session.book.as_ref().map(|book| (&book.path, &book.documents));
    let id = ui.scope_id().with(("book_action", salt, target));
    book_sdk_button(app, ui, id, label, label, false, None).unwrap_or_else(|| ui.button(crate::rtl::widget(ui, label)).clicked())
}

fn book_row_name(language: &str, action: &str, path: &str, index: usize) -> String {
    format!("{}: {path} ({})", crate::i18n::tr(language, action), index + 1)
}

fn book_open_buttons(app: &mut DesignApp, ui: &mut egui::Ui) {
    ui.horizontal(|ui| {
        if book_button(app, ui, "book_new", crate::i18n::tr(&app.ui.language, "New Book…")) {
            let asked = app
                .ask_save("Book.dcbook", Bound::App, |app, path| app.run("book.new", json!({"path": path})).or_else(|e| report_err(app, "Book", e)));
            report(app, "Book", asked);
        }
        if book_button(app, ui, "book_open", crate::i18n::tr(&app.ui.language, "Open…")) {
            let asked =
                app.ask_open("book", Bound::App, |app, path| app.run("book.open", json!({"path": path})).or_else(|e| report_err(app, "Book", e)));
            report(app, "Book", asked);
        }
    });
}

fn book_action_buttons(app: &mut DesignApp, ui: &mut egui::Ui) {
    ui.horizontal_wrapped(|ui| {
        if book_button(app, ui, "book_add", crate::i18n::tr(&app.ui.language, "Add Document…")) {
            let asked =
                app.ask_open("open", Bound::App, |app, path| app.run("book.add", json!({"path": path})).or_else(|e| report_err(app, "Book", e)));
            report(app, "Book", asked);
        }
        if book_button(app, ui, "book_paginate", crate::i18n::tr(&app.ui.language, "Update Numbering")) {
            let _ = app.run("book.paginate", json!({}));
        }
        if book_button(app, ui, "book_sync", crate::i18n::tr(&app.ui.language, "Synchronize")) {
            let _ = app.run("book.syncStyles", json!({}));
        }
        if book_button(app, ui, "book_export", crate::i18n::tr(&app.ui.language, "Export PDF…")) {
            let asked = app.ask_save("Book.pdf", Bound::App, |app, path| {
                app.run("book.exportPdf", json!({"path": path})).or_else(|e| report_err(app, "Book", e))
            });
            report(app, "Book", asked);
        }
    });
}

/// Book panel: the open book's documents with their page ranges; numbering, synchronising and
/// exporting the whole book.
pub fn book(app: &mut DesignApp, ui: &mut egui::Ui) {
    #[cfg(not(target_arch = "wasm32"))]
    if app.framework_installed {
        use craft_ui_widgets::runtime::env::{Direction, with_env};
        let direction = if crate::i18n::is_rtl(&app.ui.language) { Direction::Rtl } else { Direction::Ltr };
        with_env(ui, |scope| scope.direction = Some(direction), |ui| book_contents(app, ui));
        return;
    }
    book_contents(app, ui);
}

fn book_contents(app: &mut DesignApp, ui: &mut egui::Ui) {
    let t = Tokens::get(ui.ctx());
    book_open_buttons(app, ui);
    let Ok(info) = app.session.execute("book.list", &json!({})) else {
        crate::rtl::label(ui, egui::RichText::new(crate::i18n::tr(&app.ui.language, "No book open.")).size(11.0).color(t.text_dim));
        return;
    };
    let source = info["styleSource"].as_u64().unwrap_or(0) as usize;
    // A changed book or chapter sequence must not retarget an in-flight row press.
    let target = app.session.book.as_ref().map(|book| (&book.path, &book.documents));
    let book_id = ui.scope_id().with(("book_chapters", target));
    ui.separator();
    for (i, d) in info["documents"].as_array().cloned().unwrap_or_default().iter().enumerate() {
        let path = d["path"].as_str().unwrap_or("").to_string();
        let name = std::path::Path::new(&path).file_stem().map(|n| n.to_string_lossy().to_string()).unwrap_or_default();
        let (first, pages) = (d["firstPage"].as_u64().unwrap_or(1), d["pages"].as_u64().unwrap_or(0));
        ui.horizontal(|ui| {
            if book_sdk_button(
                app,
                ui,
                book_id.with((i, &path, "source")),
                &book_row_name(&app.ui.language, "Style source", &path, i),
                if i == source { "◆" } else { "◇" },
                true,
                Some(i == source),
            )
            .unwrap_or_else(|| {
                ui.selectable_label(i == source, if i == source { "◆" } else { "◇" })
                    .on_hover_ui(|ui| {
                        crate::rtl::label(ui, crate::i18n::tr(&app.ui.language, "Style source"));
                    })
                    .clicked()
            }) {
                let _ = app.run("book.styleSource", json!({"index": i}));
            }
            if book_sdk_button(app, ui, book_id.with((i, &path, "open")), &book_row_name(&app.ui.language, "Open", &path, i), &name, false, None)
                .unwrap_or_else(|| {
                    ui.selectable_label(false, &name)
                        .on_hover_ui(|ui| {
                            crate::rtl::label(ui, crate::i18n::tr(&app.ui.language, "Open"));
                        })
                        .clicked()
                })
            {
                let _ = app.run("file.open", json!({"path": path}));
            }
            crate::rtl::label(ui, egui::RichText::new(format!("{first}–{}", first + pages.saturating_sub(1))).size(10.5).color(t.text_dim));
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if book_sdk_button(
                    app,
                    ui,
                    book_id.with((i, &path, "remove")),
                    &book_row_name(&app.ui.language, "Remove", &path, i),
                    crate::i18n::tr(&app.ui.language, "Remove"),
                    true,
                    None,
                )
                .unwrap_or_else(|| ui.small_button(crate::rtl::widget(ui, crate::i18n::tr(&app.ui.language, "Remove"))).clicked())
                {
                    let _ = app.run("book.remove", json!({"index": i}));
                }
            });
        });
    }
    ui.separator();
    book_action_buttons(app, ui);
}

/// Script actions retain the actual current script and document/selection identity, not an ordinal alone.
fn scripts_action(app: &DesignApp, ui: &mut egui::Ui, id: egui::Id, label: &str, enabled: bool) -> bool {
    #[cfg(not(target_arch = "wasm32"))]
    if app.framework_installed {
        use craft_ui_widgets::contract::{Props as _, Show as _};
        return craft_ui_widgets::buttons::Button::new(label.to_owned())
            .id(id)
            .enabled(enabled)
            .size(craft_theme::ControlSize::Sm)
            .show(ui)
            .clicked();
    }
    #[cfg(target_arch = "wasm32")]
    let _ = (app, id);
    ui.add_enabled(enabled, egui::Button::new(crate::rtl::widget(ui, label))).clicked()
}

/// Plain, unbounded write-through editors: the persisted UI strings remain the only model.
fn scripts_sdk_editors(ui: &mut egui::Ui, id: egui::Id, name: &mut String, text: &mut String, labels: (&str, &str), framework: bool) -> bool {
    #[cfg(not(target_arch = "wasm32"))]
    if framework {
        use craft_ui_widgets::contract::{Bind, Props as _, Show as _, Sizing};
        craft_ui_widgets::text::TextField::new(labels.0.to_owned(), Bind::bound(name))
            .id(id.with("name"))
            .limit(None)
            // framework-gap(keeps_on_escape): restore `.keeps_on_escape()` once craft-libs has it
            .sizing(Sizing::FillWidth)
            .show(ui);
        craft_ui_widgets::text::TextField::new(labels.1.to_owned(), Bind::bound(text))
            .id(id.with("text"))
            .limit(None)
            .lines(8)
            .mono()
            .tab_inserts(true)
            // framework-gap(keeps_on_escape): restore `.keeps_on_escape()` once craft-libs has it
            .sizing(Sizing::FillWidth)
            .show(ui);
        return true;
    }
    #[cfg(target_arch = "wasm32")]
    let _ = (ui, id, name, text, labels, framework);
    false
}

/// Scripts panel: saved command scripts — run (one undo step), edit, add, delete, load.
pub fn scripts(app: &mut DesignApp, ui: &mut egui::Ui) {
    let t = Tokens::get(ui.ctx());
    let sel_id = egui::Id::unique("scripts_sel");
    #[cfg(not(target_arch = "wasm32"))]
    let framework = app.framework_installed;
    #[cfg(target_arch = "wasm32")]
    let framework = false;
    let mut sel: usize = ui.data(|d| d.get_temp(sel_id)).unwrap_or(0);
    if app.ui.scripts.is_empty() {
        crate::rtl::label(
            ui,
            crate::rtl::widget(ui, egui::RichText::new(crate::i18n::tr(&app.ui.language, "No scripts. Add one below.")).size(11.0).color(t.text_dim)),
        );
    }
    let mut run: Option<usize> = None;
    for (i, (name, _)) in app.ui.scripts.iter().enumerate() {
        let r = ui.selectable_label(i == sel, name);
        if r.clicked() {
            sel = i;
        }
        if r.double_clicked() {
            run = Some(i);
        }
    }
    ui.separator();
    let target = app
        .session
        .active()
        .map(|st| format!("{:?}", (st.uid, st.identity_epoch, st.revision, &st.selection.items, &st.selection.text, &st.selection.cells)));
    // Hash only the selected full script, not every hidden script or document content/history.
    let actions = ui.scope_id().with(("scripts_actions", target, sel, app.ui.scripts.len(), app.ui.scripts.get(sel)));
    ui.horizontal(|ui| {
        if scripts_action(app, ui, actions.with("run"), crate::i18n::tr(&app.ui.language, "Run"), !app.ui.scripts.is_empty()) {
            run = Some(sel);
        }
        if scripts_action(app, ui, actions.with("new"), crate::i18n::tr(&app.ui.language, "New"), true) {
            app.ui.scripts.push((format!("Script {}", app.ui.scripts.len() + 1), "# command.id {json} per line\n".into()));
            sel = app.ui.scripts.len() - 1;
        }
        if scripts_action(app, ui, actions.with("load"), crate::i18n::tr(&app.ui.language, "Load…"), true) {
            let asked = app.ask_open("script", Bound::App, move |app, path| {
                load_script(app, &path, sel_id);
                Ok(serde_json::Value::Null)
            });
            report(app, "Scripts", asked);
        }
        if scripts_action(app, ui, actions.with("delete"), crate::i18n::tr(&app.ui.language, "Delete"), !app.ui.scripts.is_empty())
            && sel < app.ui.scripts.len()
        {
            app.ui.scripts.remove(sel);
            sel = sel.saturating_sub(1);
        }
    });
    // The shelf and its editors are app-owned, not document-owned. Preserve the original selected-slot policy.
    let editor = ui.scope_id().with(("scripts_editor", sel, app.ui.scripts.len()));
    let labels = (crate::i18n::tr(&app.ui.language, "Name"), crate::i18n::tr(&app.ui.language, "Scripts"));
    if let Some((name, text)) = app.ui.scripts.get_mut(sel)
        && !scripts_sdk_editors(ui, editor, name, text, labels, framework)
    {
        ui.add(egui::TextEdit::singleline(name).desired_width(f32::INFINITY)).accessible_name(labels.0);
        ui.add(egui::TextEdit::multiline(text).code_editor().desired_rows(8).desired_width(f32::INFINITY)).accessible_name(labels.1);
    }
    if let Some(i) = run
        && let Some((name, text)) = app.ui.scripts.get(i).cloned()
    {
        match app.run("script.run", json!({"text": text})) {
            Ok(r) => app.status(
                crate::i18n::tr(&app.ui.language, "{name}: {count} step(s) done")
                    .replace("{name}", &name)
                    .replace("{count}", &r["steps"].to_string()),
            ),
            Err(e) => app.status(format!("{name}: {e}")),
        }
    }
    ui.data_mut(|d| d.insert_temp(sel_id, sel));
}

#[cfg(all(test, not(target_arch = "wasm32")))]
mod framework_action_tests {
    use super::*;
    use craft_ui_widgets::runtime::inspect::{Element, Recorded, record};
    use designcraft_doc::ItemId;
    use egui::{Event, Key, Modifiers, PointerButton, UiBuilder, vec2};
    use egui_kittest::Harness;
    use std::collections::HashSet;
    use std::sync::Arc;

    struct Window {
        app: DesignApp,
        controls: Option<Arc<Recorded>>,
        enabled: bool,
        sizing: bool,
    }

    fn step(h: &mut Harness<'_, Window>) {
        let mut raw = std::mem::take(h.input_mut());
        let ctx = h.ctx.clone();
        h.state_mut().app.raw_input_hook(&ctx, &mut raw);
        *h.input_mut() = raw;
        h.step();
    }

    fn idle(h: &mut Harness<'_, Window>, frames: usize) {
        for _ in 0..frames {
            step(h);
        }
    }

    fn event(h: &mut Harness<'_, Window>, event: Event) {
        h.input_mut().events.push(event);
        step(h);
    }

    fn key(h: &mut Harness<'_, Window>, key: Key) {
        for pressed in [true, false] {
            event(h, Event::Key { key, physical_key: None, pressed, repeat: false, modifiers: Modifiers::NONE });
        }
    }

    fn click(h: &mut Harness<'_, Window>, element: &Element) {
        let pos = element.rect.center();
        event(h, Event::PointerMoved(pos));
        for pressed in [true, false] {
            event(h, Event::PointerButton { pos, button: PointerButton::Primary, pressed, modifiers: Modifiers::NONE });
        }
        idle(h, 2);
    }

    fn buttons(h: &Harness<'_, Window>, name: &str) -> Vec<Element> {
        let name = crate::i18n::tr(&h.state().app.ui.language, name);
        let mut found: Vec<_> = h.state().controls.as_ref().unwrap().elements.iter().filter(|element| {
            element.facts.as_ref().is_some_and(|facts| facts.role == "Button" && facts.name.as_deref() == Some(name))
        }).cloned().collect();
        found.sort_by(|a, b| a.rect.top().total_cmp(&b.rect.top()));
        found
    }

    fn only(h: &Harness<'_, Window>, name: &str) -> Element {
        let found = buttons(h, name);
        assert_eq!(found.len(), 1, "one genuinely unique Button named {name}");
        found[0].clone()
    }

    fn tab_to(h: &mut Harness<'_, Window>, target: &Element) {
        let mut visited = HashSet::new();
        loop {
            let focused = h.ctx.memory(|memory| memory.focused());
            if focused == Some(target.id) {
                return;
            }
            assert!(visited.insert(focused), "a real Tab cycle did not reach the observed Library button {:?}", target.facts);
            key(h, Key::Tab);
        }
    }

    fn harness(mut app: DesignApp) -> Harness<'static, Window> {
        app.framework_opacity = true;
        let ctx = egui::Context::default();
        app.install_framework(&ctx);
        app.raw_input_hook(&ctx, &mut egui::RawInput::default());
        let mut ready = false;
        let mut h = Harness::builder().with_context(ctx).with_size(vec2(420.0, 520.0)).build_ui_state(
            move |ui, state: &mut Window| {
                if !ready {
                    crate::theme::install_fonts(ui.ctx(), &state.app.ui.language);
                    ready = true;
                    return;
                }
                let builder = if state.sizing { UiBuilder::new().sizing_pass() } else { UiBuilder::new() };
                ui.scope_builder(builder, |ui| {
                    ui.add_enabled_ui(state.enabled, |ui| {
                        state.controls = record(ui, egui::Id::unique("object-library-actions-test"), |ui| show(&mut state.app, ui)).1;
                    });
                });
            },
            Window { app, controls: None, enabled: true, sizing: false },
        );
        h.input_mut().max_texture_side = Some(8192);
        idle(&mut h, 4);
        h
    }

    fn fixture() -> (Harness<'static, Window>, ItemId) {
        let mut app = DesignApp::new_for_panel_tests(designcraft_engine::Session::new(), crate::Services::default());
        app.run("file.new", json!({})).unwrap();
        app.run("library.new", json!({})).unwrap();
        let mut first = None;
        for (rect, description) in [([0, 0, 10, 10], "first"), ([20, 0, 50, 10], "second")] {
            let id = ItemId(app.run("frame.create", json!({"rect": rect})).unwrap()["id"].as_u64().unwrap());
            first.get_or_insert(id);
            app.run("library.add", json!({"name": "Duplicate", "description": description})).unwrap();
        }
        let first = first.unwrap();
        app.run("conveyor.collect", json!({"ids": [first.0]})).unwrap();
        app.run("selection.set", json!({"ids": []})).unwrap();
        assert!(app.session.library.as_ref().unwrap().path.is_none(), "this fixture can never write a library file");
        (harness(app), first)
    }

    #[test]
    fn installed_duplicate_library_rows_dispatch_exact_indices_once() {
        let (mut h, _) = fixture();
        // library.list returns actual indices 0 then 1; duplicate names never become control identities.
        let places = buttons(&h, "Place");
        let deletes = buttons(&h, "Delete");
        assert_eq!((places.len(), deletes.len()), (2, 2));
        assert_ne!(places[0].id, places[1].id);
        assert_ne!(deletes[0].id, deletes[1].id);
        let before = h.state().app.session.active().unwrap().doc.clone();
        let undo = h.state().app.session.active().unwrap().history.undo.len();
        let journal = h.state().app.session.journal.len();
        click(&mut h, &places[1]);
        assert_eq!(&h.state().app.session.journal[journal..], &[("library.place".to_string(), json!({"index": 1}))]);
        let active = h.state().app.session.active().unwrap();
        assert_eq!(active.history.undo.len(), undo + 1);
        assert_eq!(active.doc.spreads[0].items.len(), before.spreads[0].items.len() + 1);
        let placed = active.doc.item(active.selection.items[0]).unwrap().bounds();
        assert_eq!(placed.x1 - placed.x0, 30.0, "the second same-named item's real snippet was placed");
        idle(&mut h, 5);
        assert_eq!(h.state().app.session.journal.len(), journal + 1, "idle frames never place a second copy");
        h.state_mut().app.run("edit.undo", json!({})).unwrap();
        assert_eq!(h.state().app.session.active().unwrap().doc.as_ref(), before.as_ref());
        idle(&mut h, 2);
        let journal = h.state().app.session.journal.len();
        click(&mut h, &deletes[1]);
        assert_eq!(&h.state().app.session.journal[journal..], &[("library.remove".to_string(), json!({"index": 1}))]);
        let items = &h.state().app.session.library.as_ref().unwrap().items;
        assert_eq!(items.len(), 1);
        assert_eq!(items[0].description, "first", "the other same-named row remains");
        assert_eq!(h.state().app.session.active().unwrap().history.undo.len(), undo, "library removal keeps its existing non-document undo policy");
        idle(&mut h, 5);
        assert_eq!(h.state().app.session.journal.len(), journal + 1, "idle frames never delete the remaining row");
        assert_eq!(h.state().app.session.library.as_ref().unwrap().items.len(), 1);
        assert!(h.state().app.session.library.as_ref().unwrap().path.is_none());
    }

    #[test]
    fn installed_add_selection_disabled_rules_and_natural_keyboard_actions_are_preserved() {
        for document in [false, true] {
            let mut app = DesignApp::new_for_panel_tests(designcraft_engine::Session::new(), crate::Services::default());
            app.run("library.new", json!({})).unwrap();
            if document {
                app.run("file.new", json!({})).unwrap();
            }
            let mut h = harness(app);
            let add = only(&h, "Add Selection");
            assert!(add.facts.as_ref().unwrap().disabled, "no active document OR no selection disables Add Selection");
            let journal = h.state().app.session.journal.len();
            click(&mut h, &add);
            idle(&mut h, 3);
            assert_eq!(h.state().app.session.journal.len(), journal);
            assert!(h.state().app.session.library.as_ref().unwrap().items.is_empty());
        }
        let (mut h, first) = fixture();
        let clear = only(&h, "Clear");
        let journal = h.state().app.session.journal.len();
        tab_to(&mut h, &clear);
        assert_eq!(h.state().app.session.journal.len(), journal, "natural Tab only navigates");
        key(&mut h, Key::Space);
        assert_eq!(&h.state().app.session.journal[journal..], &[("conveyor.clear".to_string(), json!({}))]);
        assert!(h.state().app.session.conveyor.is_empty());
        idle(&mut h, 4);
        assert_eq!(h.state().app.session.journal.len(), journal + 1);
        h.state_mut().app.run("selection.set", json!({"ids": [first.0]})).unwrap();
        idle(&mut h, 2);
        let add = only(&h, "Add Selection");
        assert!(!add.facts.as_ref().unwrap().disabled);
        let before = h.state().app.session.library.as_ref().unwrap().items.len();
        let journal = h.state().app.session.journal.len();
        tab_to(&mut h, &add);
        key(&mut h, Key::Enter);
        assert_eq!(&h.state().app.session.journal[journal..], &[("library.add".to_string(), json!({}))]);
        assert_eq!(h.state().app.session.library.as_ref().unwrap().items.len(), before + 1);
        idle(&mut h, 5);
        assert_eq!(h.state().app.session.journal.len(), journal + 1);
        assert_eq!(h.state().app.session.library.as_ref().unwrap().items.len(), before + 1);
    }

    #[test]
    fn installed_library_choosers_cancel_or_report_error_without_speculative_or_disabled_effects() {
        use crate::app::file_dialogs::testing::answer_with;
        let (mut h, _) = fixture();
        h.state_mut().app.answer_dialogs(answer_with(None));
        let original_library = h.state_mut().app.session.execute("library.json", &json!({})).unwrap();
        let before = h.state().app.session.active().unwrap().doc.clone();
        let journal = h.state().app.session.journal.len();
        let open = only(&h, "Open…");
        h.state_mut().enabled = false;
        idle(&mut h, 2);
        assert!(only(&h, "Open…").facts.as_ref().unwrap().disabled, "the SDK inherits disabled scope state");
        click(&mut h, &open);
        assert!(!h.state().app.file_dialogs_pending(), "disabled native buttons never queue a chooser");
        assert_eq!(h.state().app.file_dialogs.queued(), 0, "disabled probe leaves no queued chooser");
        h.state_mut().enabled = true;
        idle(&mut h, 2);
        let new = only(&h, "New Library…");
        tab_to(&mut h, &new);
        h.state_mut().sizing = true;
        key(&mut h, Key::Enter);
        assert!(!h.state().app.file_dialogs_pending(), "a sizing pass never queues an IO continuation");
        assert_eq!(h.state().app.file_dialogs.queued(), 0, "sizing probe leaves no queued chooser");
        h.state_mut().sizing = false;
        idle(&mut h, 2);
        for name in ["New Library…", "Open…"] {
            let button = only(&h, name);
            click(&mut h, &button);
            assert_eq!(h.state().app.file_dialogs.queued(), 1, "one activation asks once");
            h.state_mut().app.settle_dialogs();
            assert!(!h.state().app.file_dialogs_pending());
            idle(&mut h, 2);
        }
        assert_eq!(h.state().app.session.journal.len(), journal, "canceled choosers never execute their library command");
        assert_eq!(h.state_mut().app.session.execute("library.json", &json!({})).unwrap(), original_library);
        assert_eq!(h.state().app.session.active().unwrap().doc.as_ref(), before.as_ref());
        h.state_mut().app.answer_dialogs(|_| craft_dialogs::Answer::Unavailable("fixture unavailable".to_owned()));
        let open = only(&h, "Open…");
        click(&mut h, &open);
        h.state_mut().app.settle_dialogs();
        assert!(h.state().app.ui.status.contains("fixture unavailable"));
        assert!(!h.state().app.file_dialogs_pending());
        assert_eq!(h.state().app.session.journal.len(), journal);
        assert_eq!(h.state_mut().app.session.execute("library.json", &json!({})).unwrap(), original_library);
    }
}

#[cfg(all(test, not(target_arch = "wasm32")))]
mod book_workflow_tests {
    use super::*;
    use crate::app::file_dialogs::testing::answer_with;
    use craft_ui_widgets::runtime::inspect::{Element, Recorded, record};
    use egui::{Event, Key, Modifiers, PointerButton, UiBuilder, vec2};
    use egui_kittest::{
        Harness,
        kittest::{NodeT, Queryable},
    };
    use std::{collections::HashSet, sync::Arc};

    struct Rig {
        app: DesignApp,
        controls: Option<Arc<Recorded>>,
        enabled: bool,
        sizing: bool,
    }
    struct Fixture {
        _dir: tempfile::TempDir,
        chapters: Vec<String>,
        book: String,
    }
    fn step(h: &mut Harness<'_, Rig>) {
        let mut raw = std::mem::take(h.input_mut());
        let ctx = h.ctx.clone();
        h.state_mut().app.raw_input_hook(&ctx, &mut raw);
        *h.input_mut() = raw;
        h.step();
    }
    fn idle(h: &mut Harness<'_, Rig>, frames: usize) {
        for _ in 0..frames {
            step(h);
        }
    }
    fn event(h: &mut Harness<'_, Rig>, event: Event) {
        h.input_mut().events.push(event);
        step(h);
    }
    fn key(h: &mut Harness<'_, Rig>, key: Key) {
        for pressed in [true, false] {
            event(h, Event::Key { key, physical_key: None, pressed, repeat: false, modifiers: Modifiers::NONE });
        }
    }
    fn element(h: &Harness<'_, Rig>, name: &str) -> Element {
        let found: Vec<_> = h
            .state()
            .controls
            .as_ref()
            .unwrap()
            .elements
            .iter()
            .filter(|e| e.component == "Button" && e.part.is_none() && e.facts.as_ref().is_some_and(|f| f.name.as_deref() == Some(name)))
            .cloned()
            .collect();
        assert_eq!(found.len(), 1, "one complete named Book Button: {name}");
        let field = found.into_iter().next().unwrap();
        let facts = field.facts.as_ref().unwrap();
        assert_eq!(facts.role, format!("{:?}", egui::accesskit::Role::Button));
        let node = h.get_by(|n| n.role() == egui::accesskit::Role::Button && n.label().as_deref() == Some(name));
        let rect = node.rect();
        assert!(rect.width() > 0.0 && rect.height() > 0.0 && field.rect.contains(rect.center()), "actual public AX hit target");
        field
    }
    fn pointer(h: &mut Harness<'_, Rig>, pos: egui::Pos2, pressed: bool) {
        event(h, Event::PointerMoved(pos));
        event(h, Event::PointerButton { pos, button: PointerButton::Primary, pressed, modifiers: Modifiers::NONE });
    }
    fn click(h: &mut Harness<'_, Rig>, name: &str) {
        element(h, name);
        let pos = h.get_by(|n| n.role() == egui::accesskit::Role::Button && n.label().as_deref() == Some(name)).rect().center();
        pointer(h, pos, true);
        pointer(h, pos, false);
    }
    fn tab_to(h: &mut Harness<'_, Rig>, name: &str) {
        let target = element(h, name);
        let mut visited = HashSet::new();
        loop {
            let node = h.get_by(|n| n.role() == egui::accesskit::Role::Button && n.label().as_deref() == Some(name));
            if node.is_focused() {
                assert_eq!(h.ctx.memory(|m| m.focused()), Some(target.id));
                return;
            }
            assert!(visited.insert(h.ctx.memory(|m| m.focused())), "a genuine Tab cycle must reach {name}");
            key(h, Key::Tab);
        }
    }
    fn harness(mut app: DesignApp, installed: bool) -> Harness<'static, Rig> {
        app.framework_opacity = true;
        let ctx = egui::Context::default();
        if installed {
            app.install_framework(&ctx);
        }
        app.raw_input_hook(&ctx, &mut egui::RawInput::default());
        let mut ready = false;
        let mut h = Harness::builder().with_context(ctx).with_size(vec2(680.0, 540.0)).build_ui_state(
            move |ui, state: &mut Rig| {
                if !ready {
                    crate::theme::install_fonts(ui.ctx(), &state.app.ui.language);
                    ready = true;
                    return;
                }
                let builder = if state.sizing { UiBuilder::new().sizing_pass() } else { UiBuilder::new() };
                ui.scope_builder(builder, |ui| {
                    ui.add_enabled_ui(state.enabled, |ui| {
                        state.controls = record(ui, egui::Id::unique("book-workflow"), |ui| book(&mut state.app, ui)).1;
                    });
                });
            },
            Rig { app, controls: None, enabled: true, sizing: false },
        );
        h.input_mut().max_texture_side = Some(8192);
        idle(&mut h, 4);
        h
    }
    fn fixture(duplicate_path: bool) -> (Fixture, DesignApp) {
        let dir = tempfile::tempdir().unwrap();
        let mut session = designcraft_engine::Session::new();
        let mut chapters = Vec::new();
        for (folder, pages, style) in [("one & α", 1, "Book source"), ("two", 2, "Other source")] {
            let parent = dir.path().join(folder);
            std::fs::create_dir(&parent).unwrap();
            let path = parent.join("Chapter.designcraft").to_string_lossy().into_owned();
            session.execute("file.new", &json!({"pages":pages})).unwrap();
            session.execute("style.paragraph.create", &json!({"name":style,"chars":{"size":20}})).unwrap();
            session.execute("file.saveAs", &json!({"path":path})).unwrap();
            session.execute("file.close", &json!({})).unwrap();
            chapters.push(path);
        }
        let path = dir.path().join("Book.dcbook").to_string_lossy().into_owned();
        session.execute("book.new", &json!({"path":path})).unwrap();
        for chapter in &chapters {
            session.execute("book.add", &json!({"path":chapter})).unwrap();
        }
        if duplicate_path {
            session.execute("book.add", &json!({"path":chapters[0]})).unwrap();
            chapters.push(chapters[0].clone());
        }
        session.execute("file.new", &json!({})).unwrap();
        for rect in [[0, 0, 10, 10], [20, 0, 30, 10]] {
            session.execute("frame.create", &json!({"rect":rect})).unwrap();
        }
        session.execute("edit.undo", &json!({})).unwrap();
        session.execute("file.saveAs", &json!({"path":dir.path().join("Unrelated.designcraft")})).unwrap();
        let active = session.active().unwrap();
        assert_eq!((active.history.undo.len(), active.history.redo.len()), (1, 1));
        let mut app = DesignApp::new_for_panel_tests(session, crate::Services::default());
        app.answer_dialogs(answer_with(None));
        (Fixture { _dir: dir, chapters, book: path }, app)
    }
    fn row(h: &Harness<'_, Rig>, action: &str, path: &str, index: usize) -> String {
        book_row_name(&h.state().app.ui.language, action, path, index)
    }
    fn mark(h: &Harness<'_, Rig>) -> (Arc<designcraft_doc::Document>, String, (usize, usize)) {
        let st = h.state().app.session.active().unwrap();
        (
            st.doc.clone(),
            format!("{:?}", (st.uid, st.identity_epoch, &st.selection.items, &st.selection.text, &st.selection.cells)),
            (st.history.undo.len(), st.history.redo.len()),
        )
    }
    fn unchanged(h: &Harness<'_, Rig>, before: &(Arc<designcraft_doc::Document>, String, (usize, usize))) {
        let now = mark(h);
        assert!(Arc::ptr_eq(&now.0, &before.0));
        assert_eq!((now.1, now.2), (before.1.clone(), before.2));
    }
    fn disk_book(path: &str) -> serde_json::Value {
        serde_json::from_slice(&std::fs::read(path).unwrap()).unwrap()
    }
    fn chapter(path: &str) -> Arc<designcraft_doc::Document> {
        let mut session = designcraft_engine::Session::new();
        session.execute("file.open", &json!({"path":path})).unwrap();
        session.active().unwrap().doc.clone()
    }

    #[test]
    fn installed_book_style_source_and_remove_preserve_duplicate_identity_and_document_undo() {
        let (f, app) = fixture(true);
        let mut h = harness(app, true);
        let before = mark(&h);
        let source = row(&h, "Style source", &f.chapters[2], 2);
        let other = row(&h, "Style source", &f.chapters[0], 0);
        assert_ne!(element(&h, &source).id, element(&h, &other).id);
        let start = h.state().app.session.journal.len();
        tab_to(&mut h, &source);
        key(&mut h, Key::Space);
        assert_eq!(&h.state().app.session.journal[start..], &[("book.styleSource".to_owned(), json!({"index":2}))]);
        assert_eq!(h.state().app.session.book.as_ref().unwrap().style_source, 2);
        assert_eq!(disk_book(&f.book)["styleSource"], 2);
        unchanged(&h, &before);
        let remove = row(&h, "Remove", &f.chapters[1], 1);
        click(&mut h, &remove);
        assert_eq!(
            &h.state().app.session.journal[start..],
            &[("book.styleSource".to_owned(), json!({"index":2})), ("book.remove".to_owned(), json!({"index":1}))]
        );
        assert_eq!(h.state().app.session.book.as_ref().unwrap().documents, [f.chapters[0].clone(), f.chapters[2].clone()]);
        assert_eq!(h.state().app.session.book.as_ref().unwrap().style_source, 1);
        assert_eq!(disk_book(&f.book)["documents"], json!([f.chapters[0], f.chapters[2]]));
        unchanged(&h, &before);
        idle(&mut h, 3);
        assert_eq!(h.state().app.session.journal.len(), start + 2);
    }

    #[test]
    fn installed_book_open_row_uses_full_path_not_duplicate_basename() {
        let (f, app) = fixture(false);
        let mut h = harness(app, true);
        let name = row(&h, "Open", &f.chapters[1], 1);
        let start = h.state().app.session.journal.len();
        tab_to(&mut h, &name);
        key(&mut h, Key::Enter);
        assert_eq!(&h.state().app.session.journal[start..], &[("file.open".to_owned(), json!({"path":f.chapters[1]}))]);
        assert_eq!(h.state().app.session.active().unwrap().path.as_deref(), Some(f.chapters[1].as_str()));
        assert_eq!(h.state().app.session.book.as_ref().unwrap().documents, f.chapters);
    }

    #[test]
    fn installed_book_held_remove_cannot_follow_a_changed_chapter_sequence() {
        let (f, app) = fixture(true);
        let mut h = harness(app, true);
        let held = row(&h, "Remove", &f.chapters[1], 1);
        let old = element(&h, &held);
        let pos = h.get_by(|n| n.role() == egui::accesskit::Role::Button && n.label().as_deref() == Some(held.as_str())).rect().center();
        pointer(&mut h, pos, true);
        let first = row(&h, "Remove", &f.chapters[0], 0);
        tab_to(&mut h, &first);
        let start = h.state().app.session.journal.len();
        key(&mut h, Key::Space);
        assert_eq!(&h.state().app.session.journal[start..], &[("book.remove".to_owned(), json!({"index":0}))]);
        let fresh = row(&h, "Remove", &f.chapters[1], 0);
        assert_ne!(element(&h, &fresh).id, old.id);
        pointer(&mut h, pos, false);
        idle(&mut h, 2);
        assert_eq!(h.state().app.session.journal.len(), start + 1, "release cannot remove the row now under the pointer");
        assert_eq!(h.state().app.session.book.as_ref().unwrap().documents, [f.chapters[1].clone(), f.chapters[2].clone()]);
        click(&mut h, &fresh);
        assert_eq!(
            &h.state().app.session.journal[start..],
            &[("book.remove".to_owned(), json!({"index":0})), ("book.remove".to_owned(), json!({"index":0}))]
        );
        assert_eq!(h.state().app.session.book.as_ref().unwrap().documents, [f.chapters[2].clone()]);
    }

    #[test]
    fn installed_book_choosers_cancel_and_unavailable_keep_files_model_and_history() {
        let (f, app) = fixture(false);
        let mut h = harness(app, true);
        let before = mark(&h);
        let old_book = std::fs::read(&f.book).unwrap();
        let old_chapters: Vec<_> = f.chapters.iter().map(|p| std::fs::read(p).unwrap()).collect();
        let start = h.state().app.session.journal.len();
        for caption in ["New Book…", "Open…", "Add Document…", "Export PDF…"] {
            click(&mut h, caption);
            assert_eq!(h.state().app.file_dialogs.queued(), 1);
            h.state_mut().app.settle_dialogs();
            assert!(!h.state().app.file_dialogs_pending());
            idle(&mut h, 2);
        }
        assert_eq!(h.state().app.session.journal.len(), start);
        h.state_mut().app.answer_dialogs(|_| craft_dialogs::Answer::Unavailable("Book fixture unavailable".to_owned()));
        click(&mut h, "Export PDF…");
        h.state_mut().app.settle_dialogs();
        assert!(h.state().app.ui.status.contains("Book fixture unavailable"));
        assert_eq!(h.state().app.session.journal.len(), start);
        unchanged(&h, &before);
        assert_eq!(std::fs::read(&f.book).unwrap(), old_book);
        for (path, bytes) in f.chapters.iter().zip(old_chapters) {
            assert_eq!(std::fs::read(path).unwrap(), bytes);
        }
    }

    #[test]
    fn installed_book_new_open_add_use_real_answered_callbacks_once() {
        let (f, app) = fixture(false);
        let mut h = harness(app, true);
        let before = mark(&h);
        let created = f._dir.path().join("New Book.dcbook").to_string_lossy().into_owned();
        let start = h.state().app.session.journal.len();
        h.state_mut().app.answer_dialogs(answer_with(Some(created.clone())));
        click(&mut h, "New Book…");
        h.state_mut().app.settle_dialogs();
        idle(&mut h, 2);
        assert_eq!(h.state().app.session.book.as_ref().unwrap().path, created);
        assert!(h.state().app.session.book.as_ref().unwrap().documents.is_empty());
        h.state_mut().app.answer_dialogs(answer_with(Some(f.book.clone())));
        click(&mut h, "Open…");
        h.state_mut().app.settle_dialogs();
        idle(&mut h, 2);
        assert_eq!(h.state().app.session.book.as_ref().unwrap().documents, f.chapters);
        h.state_mut().app.answer_dialogs(answer_with(Some(f.chapters[0].clone())));
        click(&mut h, "Add Document…");
        h.state_mut().app.settle_dialogs();
        idle(&mut h, 2);
        assert_eq!(
            &h.state().app.session.journal[start..],
            &[
                ("book.new".to_owned(), json!({"path":created})),
                ("book.open".to_owned(), json!({"path":f.book})),
                ("book.add".to_owned(), json!({"path":f.chapters[0]}))
            ]
        );
        assert_eq!(h.state().app.session.book.as_ref().unwrap().documents, [f.chapters[0].clone(), f.chapters[1].clone(), f.chapters[0].clone()]);
        unchanged(&h, &before);
        idle(&mut h, 3);
        assert_eq!(h.state().app.session.journal.len(), start + 3);
    }

    #[test]
    fn installed_book_paginate_sync_and_export_touch_closed_chapters_not_active_document() {
        let (f, app) = fixture(false);
        let mut h = harness(app, true);
        let before = mark(&h);
        let start = h.state().app.session.journal.len();
        tab_to(&mut h, "Update Numbering");
        key(&mut h, Key::Enter);
        assert_eq!(chapter(&f.chapters[0]).page_name(0), "1");
        assert_eq!(chapter(&f.chapters[1]).page_name(0), "2");
        click(&mut h, "Synchronize");
        assert!(chapter(&f.chapters[1]).styles.para("Book source").is_some());
        let pdf = f._dir.path().join("Book.pdf").to_string_lossy().into_owned();
        h.state_mut().app.answer_dialogs(answer_with(Some(pdf.clone())));
        click(&mut h, "Export PDF…");
        h.state_mut().app.settle_dialogs();
        let bytes = std::fs::read(&pdf).unwrap();
        assert!(bytes.starts_with(b"%PDF-"));
        assert_eq!(designcraft_render::pdf_page_count(&bytes), Some(3));
        assert_eq!(
            &h.state().app.session.journal[start..],
            &[("book.paginate".to_owned(), json!({})), ("book.syncStyles".to_owned(), json!({})), ("book.exportPdf".to_owned(), json!({"path":pdf}))]
        );
        unchanged(&h, &before);
    }

    #[test]
    fn installed_book_write_refusal_preserves_rows_source_files_and_document_then_recovers() {
        let (f, app) = fixture(false);
        let mut h = harness(app, true);
        let before = mark(&h);
        let backup = f._dir.path().join("Saved.dcbook");
        std::fs::rename(&f.book, &backup).unwrap();
        std::fs::create_dir(&f.book).unwrap();
        let original = std::fs::read(&backup).unwrap();
        let start = h.state().app.session.journal.len();
        let source = row(&h, "Style source", &f.chapters[1], 1);
        let status = h.state().app.ui.status.clone();
        click(&mut h, &source);
        assert_ne!(h.state().app.ui.status, status, "a real failed book save is surfaced");
        assert_eq!(h.state().app.session.book.as_ref().unwrap().style_source, 0);
        let remove = row(&h, "Remove", &f.chapters[1], 1);
        click(&mut h, &remove);
        assert_eq!(h.state().app.session.book.as_ref().unwrap().documents, f.chapters);
        assert_eq!(std::fs::read(&backup).unwrap(), original);
        assert_eq!(h.state().app.session.journal.len(), start);
        unchanged(&h, &before);
        std::fs::remove_dir(&f.book).unwrap();
        std::fs::rename(&backup, &f.book).unwrap();
        click(&mut h, &source);
        assert_eq!(h.state().app.session.book.as_ref().unwrap().style_source, 1);
        assert_eq!(&h.state().app.session.journal[start..], &[("book.styleSource".to_owned(), json!({"index":1}))]);
    }

    #[test]
    fn installed_book_disabled_and_sizing_passes_cannot_dispatch_or_queue_dialogs() {
        let (f, app) = fixture(false);
        let mut h = harness(app, true);
        let before = mark(&h);
        let old = std::fs::read(&f.book).unwrap();
        let start = h.state().app.session.journal.len();
        h.state_mut().enabled = false;
        idle(&mut h, 2);
        for name in ["New Book…".to_owned(), row(&h, "Remove", &f.chapters[1], 1)] {
            assert!(element(&h, &name).facts.as_ref().unwrap().disabled);
            click(&mut h, &name);
        }
        assert_eq!(h.state().app.file_dialogs.queued(), 0);
        assert_eq!(h.state().app.session.journal.len(), start);
        h.state_mut().enabled = true;
        idle(&mut h, 2);
        tab_to(&mut h, "New Book…");
        h.state_mut().sizing = true;
        key(&mut h, Key::Enter);
        assert_eq!(h.state().app.file_dialogs.queued(), 0);
        assert_eq!(h.state().app.session.journal.len(), start);
        assert_eq!(std::fs::read(&f.book).unwrap(), old);
        unchanged(&h, &before);
    }

    #[test]
    fn installed_book_no_book_has_only_open_actions_and_full_translated_row_names() {
        let (_f, mut app) = fixture(false);
        app.run("book.close", json!({})).unwrap();
        let h = harness(app, true);
        element(&h, "New Book…");
        element(&h, "Open…");
        assert_eq!(h.state().controls.as_ref().unwrap().elements.iter().filter(|e| e.component == "Button" && e.part.is_none()).count(), 2);
        let (f, mut app) = fixture(true);
        app.ui.language = "de".to_owned();
        let h = harness(app, true);
        for (i, path) in f.chapters.iter().enumerate() {
            for action in ["Style source", "Open", "Remove"] {
                element(&h, &row(&h, action, path, i));
            }
        }
    }

    #[test]
    fn uninstalled_book_native_rows_and_real_remove_payload_remain_available() {
        let (f, app) = fixture(false);
        let mut h = harness(app, false);
        assert!(!h.state().app.framework_installed);
        assert!(!h.state().controls.as_ref().is_some_and(|r| r.elements.iter().any(|e| e.component == "Button")));
        let pos = {
            let nodes: Vec<_> = h.query_all_by(|n| n.role() == egui::accesskit::Role::Button && n.label().as_deref() == Some("Remove")).collect();
            assert_eq!(nodes.len(), 2);
            nodes[1].rect().center()
        };
        let start = h.state().app.session.journal.len();
        pointer(&mut h, pos, true);
        pointer(&mut h, pos, false);
        assert_eq!(&h.state().app.session.journal[start..], &[("book.remove".to_owned(), json!({"index":1}))]);
        assert_eq!(h.state().app.session.book.as_ref().unwrap().documents, [f.chapters[0].clone()]);
    }

    #[test]
    fn installed_book_held_remove_cannot_cross_books_with_identical_chapter_paths() {
        let (f, app) = fixture(false);
        let alternate = f._dir.path().join("Other.dcbook").to_string_lossy().into_owned();
        let mut other = designcraft_engine::Session::new();
        other.execute("book.new", &json!({"path":alternate})).unwrap();
        for path in &f.chapters {
            other.execute("book.add", &json!({"path":path})).unwrap();
        }
        let mut h = harness(app, true);
        let before = mark(&h);
        let bytes = std::fs::read(&f.book).unwrap();
        let name = row(&h, "Remove", &f.chapters[1], 1);
        let old = element(&h, &name);
        let pos = h.get_by(|n| n.role() == egui::accesskit::Role::Button && n.label().as_deref() == Some(name.as_str())).rect().center();
        pointer(&mut h, pos, true);
        let start = h.state().app.session.journal.len();
        // Another ordinary command entry point opens a different real Book while this press is held.
        h.state_mut().app.run("book.open", json!({"path":alternate})).unwrap();
        idle(&mut h, 2);
        assert_ne!(element(&h, &name).id, old.id);
        pointer(&mut h, pos, false);
        idle(&mut h, 2);
        assert_eq!(&h.state().app.session.journal[start..], &[("book.open".to_owned(), json!({"path":alternate}))]);
        assert_eq!(h.state().app.session.book.as_ref().unwrap().documents, f.chapters);
        assert_eq!(std::fs::read(&f.book).unwrap(), bytes);
        unchanged(&h, &before);
    }

    #[test]
    fn installed_book_numbering_refuses_an_open_chapter_without_partial_disk_or_document_changes() {
        let (f, app) = fixture(false);
        let mut h = harness(app, true);
        let open = row(&h, "Open", &f.chapters[0], 0);
        click(&mut h, &open);
        assert_eq!(h.state().app.session.active().unwrap().path.as_deref(), Some(f.chapters[0].as_str()));
        let before = mark(&h);
        let bytes: Vec<_> = f.chapters.iter().map(|p| std::fs::read(p).unwrap()).collect();
        let book_bytes = std::fs::read(&f.book).unwrap();
        let start = h.state().app.session.journal.len();
        let status = h.state().app.ui.status.clone();
        click(&mut h, "Update Numbering");
        assert_ne!(h.state().app.ui.status, status, "the real command's refusal is surfaced");
        assert_eq!(h.state().app.session.journal.len(), start, "a refused command is not a successful journal entry");
        for (path, bytes) in f.chapters.iter().zip(bytes) {
            assert_eq!(std::fs::read(path).unwrap(), bytes);
        }
        assert_eq!(std::fs::read(&f.book).unwrap(), book_bytes);
        unchanged(&h, &before);
    }
}

#[cfg(all(test, not(target_arch = "wasm32")))]
mod scripts_shelf_workflow_tests {
    use super::*;
    use craft_ui_widgets::runtime::inspect::{Element, Recorded, record};
    use egui::{Event, Key, Modifiers, PointerButton, UiBuilder, vec2};
    use egui_kittest::{
        Harness,
        kittest::{NodeT, Queryable},
    };
    use std::{collections::HashSet, sync::Arc};

    struct Window {
        app: DesignApp,
        controls: Option<Arc<Recorded>>,
        enabled: bool,
        sizing: bool,
        real_parent: bool,
        scroll_viewport: Option<egui::Rect>,
    }

    fn step(h: &mut Harness<'_, Window>) {
        let mut raw = std::mem::take(h.input_mut());
        let ctx = h.ctx.clone();
        h.state_mut().app.raw_input_hook(&ctx, &mut raw);
        *h.input_mut() = raw;
        h.step();
    }
    fn idle(h: &mut Harness<'_, Window>, frames: usize) {
        for _ in 0..frames {
            step(h);
        }
    }
    fn event(h: &mut Harness<'_, Window>, event: Event) {
        h.input_mut().events.push(event);
        step(h);
    }
    fn key_mod(h: &mut Harness<'_, Window>, key: Key, modifiers: Modifiers) {
        for pressed in [true, false] {
            event(h, Event::Key { key, physical_key: None, pressed, repeat: false, modifiers });
        }
    }
    fn key(h: &mut Harness<'_, Window>, key: Key) {
        key_mod(h, key, Modifiers::NONE);
    }
    fn pointer(h: &mut Harness<'_, Window>, pos: egui::Pos2, pressed: bool) {
        event(h, Event::PointerButton { pos, button: PointerButton::Primary, pressed, modifiers: Modifiers::NONE });
    }
    fn click_at(h: &mut Harness<'_, Window>, pos: egui::Pos2) {
        event(h, Event::PointerMoved(pos));
        pointer(h, pos, true);
        pointer(h, pos, false);
        idle(h, 2);
    }
    fn label(h: &Harness<'_, Window>, caption: &str) -> String {
        crate::i18n::tr(&h.state().app.ui.language, caption).to_owned()
    }
    fn element(h: &Harness<'_, Window>, caption: &str, component: &str) -> Element {
        let full = label(h, caption);
        let found: Vec<_> = h
            .state()
            .controls
            .as_ref()
            .unwrap()
            .elements
            .iter()
            .filter(|e| e.component == component && e.part.is_none() && e.facts.as_ref().is_some_and(|f| f.name.as_deref() == Some(full.as_str())))
            .cloned()
            .collect();
        assert_eq!(found.len(), 1, "one current SDK {component} with complete name {full}");
        found[0].clone()
    }
    fn click(h: &mut Harness<'_, Window>, caption: &str) {
        let e = element(h, caption, "Button");
        let full = label(h, caption);
        let actual = h.get_by(|n| n.role() == egui::accesskit::Role::Button && n.label().as_deref() == Some(full.as_str()));
        assert!(!actual.accesskit_node().is_disabled());
        assert!(actual.rect().contains(e.rect.center()));
        let pos = actual.rect().center();
        click_at(h, pos);
    }
    fn tab_to(h: &mut Harness<'_, Window>, caption: &str) {
        let full = label(h, caption);
        let mut seen = HashSet::new();
        loop {
            let _current = element(h, caption, "Button");
            {
                let node = h.get_by(|n| n.role() == egui::accesskit::Role::Button && n.label().as_deref() == Some(full.as_str()));
                assert!(!node.accesskit_node().is_disabled());
                if node.accesskit_node().is_focused() {
                    return;
                }
            }
            let focused: Vec<_> = {
                let root = h.root();
                std::iter::once(root.clone())
                    .chain(root.children_recursive())
                    .filter_map(|n| {
                        let access = n.accesskit_node();
                        access.is_focused().then(|| format!("{:?}", access.id()))
                    })
                    .collect()
            };
            assert!(seen.insert(focused), "one actual AX focus cycle did not reach {full}");
            key(h, Key::Tab);
        }
    }
    fn observe_edit_focus(h: &Harness<'_, Window>, phase: &str, caption: &str, full: &str, role: egui::accesskit::Role, pos: egui::Pos2) {
        let semantic: Vec<_> = h
            .state()
            .controls
            .as_ref()
            .into_iter()
            .flat_map(|c| &c.elements)
            .filter(|e| e.component == "TextField" && e.part.is_none() && e.facts.as_ref().is_some_and(|f| f.name.as_deref() == Some(full)))
            .map(|e| (e.id, e.rect, e.content))
            .collect();
        let root = h.root();
        let mut nodes = Vec::new();
        for node in std::iter::once(root.clone()).chain(root.children_recursive()) {
            let access = node.accesskit_node();
            let matches_field = access.role() == role && access.label().as_deref() == Some(full);
            let bounds = access.bounding_box();
            nodes.push(format!(
                "id={:?} role={:?} focused={} matches_exact_field={matches_field} bounds_physical_tree_container_px={bounds:?}",
                access.id(),
                access.role(),
                access.is_focused()
            ));
        }
        let input = h.ctx.input(|i| (i.viewport_rect(), i.content_rect(), i.pointer.latest_pos()));
        let native_focus = h.ctx.memory(|m| m.focused());
        let pixels_per_point = h.ctx.pixels_per_point();
        eprintln!(
            "Scripts edit focus: {phase} caption={caption:?} full={full:?} expected_role={role:?} actual_click_ui_points={pos:?} semantic_id_rect_content_region_layer_points={semantic:?} all_actual_AX_nodes={nodes:?} native_focus={native_focus:?} viewport_content_pointer_ui_points={input:?} pixels_per_point={pixels_per_point}"
        );
    }
    fn reveal_original_script_field(h: &mut Harness<'_, Window>, caption: &str) {
        let Some(viewport) = h.state().scroll_viewport else { return };
        let full = label(h, caption);
        let role = if caption == "Scripts" { egui::accesskit::Role::MultilineTextInput } else { egui::accesskit::Role::TextInput };
        let semantic_id = element(h, caption, "TextField").id;
        let (field_id, pos) = {
            let node = h.get_by(|n| n.role() == role && n.label().as_deref() == Some(full.as_str()));
            (node.accesskit_node().id(), node.rect().center())
        };
        if viewport.contains(pos) {
            return;
        }
        observe_edit_focus(h, "before native wheel visibility input", caption, &full, role, pos);
        event(h, Event::PointerMoved(viewport.center()));
        event(
            h,
            Event::MouseWheel {
                unit: egui::MouseWheelUnit::Point,
                delta: vec2(0.0, viewport.center().y - pos.y),
                phase: egui::TouchPhase::Move,
                source: egui::MouseWheelSource::Wheel,
                modifiers: Modifiers::NONE,
            },
        );
        // ScrollArea consumes the wheel in end(); the next normal pass publishes the moved content.
        step(h);
        assert_eq!(element(h, caption, "TextField").id, semantic_id, "scrolling must preserve the same semantic field");
        let new_pos = {
            let node = h.get_by(|n| n.role() == role && n.label().as_deref() == Some(full.as_str()));
            assert_eq!(node.accesskit_node().id(), field_id, "scrolling must preserve the same full native field identity");
            node.rect().center()
        };
        observe_edit_focus(h, "after native wheel and required next-layout pass", caption, &full, role, new_pos);
        assert!(h.state().scroll_viewport.unwrap().contains(new_pos), "the original physical field must be visible before the unchanged click");
    }
    fn edit(h: &mut Harness<'_, Window>, caption: &str, text: &str) {
        reveal_original_script_field(h, caption);
        let full = label(h, caption);
        let semantic = element(h, caption, "TextField");
        let role = if caption == "Scripts" { egui::accesskit::Role::MultilineTextInput } else { egui::accesskit::Role::TextInput };
        let pos = {
            let node = h.get_by(|n| n.role() == role && n.label().as_deref() == Some(full.as_str()));
            assert!(!node.accesskit_node().is_disabled());
            assert!(node.rect().contains(semantic.rect.center()));
            node.rect().center()
        };
        observe_edit_focus(h, "before physical click", caption, &full, role, pos);
        click_at(h, pos);
        observe_edit_focus(h, "after original click and idle2", caption, &full, role, pos);
        assert!(h.get_by(|n| n.role() == role && n.label().as_deref() == Some(full.as_str())).accesskit_node().is_focused());
        key_mod(h, Key::A, Modifiers::COMMAND);
        event(h, Event::Paste(text.to_owned()));
    }
    enum OriginalScriptGlyphs {
        UnboundedName,
        LoadedUtf8(String),
        LocalizedNames,
    }

    fn original_script_glyph_texts(case: OriginalScriptGlyphs, language: &str) -> Vec<String> {
        let mut texts: Vec<String> =
            ["Run", "New", "Load…", "Delete", "Name", "Scripts"].into_iter().map(|caption| crate::i18n::tr(language, caption).to_owned()).collect();
        match case {
            OriginalScriptGlyphs::UnboundedName => {
                texts.push(format!("नाम & وصف {}", "長".repeat(1100)));
                texts.push(format!("# {}\n{}\n", "語".repeat(1100), two()));
            }
            OriginalScriptGlyphs::LoadedUtf8(path) => {
                let stem = std::path::Path::new(&path).file_stem().unwrap().to_str().unwrap().to_owned();
                assert_eq!(stem, "مخطط & Script", "the real original UTF8 picker path must remain complete");
                texts.push(path);
                texts.push(stem);
                texts.push(format!("# {}\n{}", "字".repeat(1100), two()));
            }
            OriginalScriptGlyphs::LocalizedNames => {
                texts.push("नाम & وصف".into());
                texts.push("Script & نص.designcraft".into());
            }
        }
        texts
    }

    /// The genuine existing platform face is a case-scoped precondition, never an OS install or default font fix.
    fn install_original_script_arabic(ctx: &egui::Context, language: &str, texts: &[String]) {
        #[cfg(target_os = "macos")]
        let family = "Geeza Pro";
        #[cfg(target_os = "windows")]
        let family = "Segoe UI";
        #[cfg(not(any(target_os = "macos", target_os = "windows")))]
        let family = "DejaVu Sans";
        let db = designcraft_fonts::FontDb::global();
        assert!(db.has_family(family), "the genuine existing {family} is required; no skipped Unicode or tofu");
        let face = db.face(family, "Regular");
        assert!(face.family.eq_ignore_ascii_case(family), "the actual fixture must resolve exactly to {family}, not {}", face.family);
        assert!(!face.is_placeholder() && face.style.eq_ignore_ascii_case("Regular"));
        #[cfg(target_os = "macos")]
        assert_eq!(face.index(), 0, "the observed actual Geeza Pro Regular is TTC face0");
        for c in ['\u{0627}', '\u{0625}', '\u{0637}', '\u{0639}'] {
            assert!(face.covers(c), "{family} requires the real U+{:04X} witness", u32::from(c));
        }
        for text in texts {
            for c in text.chars().filter(|c| designcraft_fonts::is_rtl(*c)) {
                assert!(face.covers(c), "{family} lacks U+{:04X} in the complete original {text}", u32::from(c));
            }
        }
        let mut definitions = ctx.fonts(|fonts| fonts.definitions().clone());
        let fallback = crate::theme::SystemFont::new("Arab", face.family.clone(), face.data().to_vec(), face.index());
        crate::theme::add_system_fallbacks(&mut definitions, &[fallback], language);
        ctx.set_fonts(definitions);
        eprintln!(
            "Scripts genuine original-data Arabic fixture: {} {}, face {}, {} existing bytes; full original RTL glyphs checked",
            face.family,
            face.style,
            face.index(),
            face.data().len()
        );
    }

    /// Assert all original Hindi/Arabic/CJK/name/filename/caption glyphs through the actual active app families.
    fn assert_original_script_glyphs(ctx: &egui::Context, texts: &[String]) {
        let families: Vec<_> = ctx.fonts(|fonts| fonts.definitions().families.keys().cloned().collect());
        assert!(families.contains(&egui::FontFamily::Proportional) && families.contains(&egui::FontFamily::Monospace));
        ctx.fonts_mut(|fonts| {
            for family in families {
                let font = egui::FontId::new(13.0, family);
                for text in texts {
                    for c in text.chars().filter(|c| !c.is_control()) {
                        assert!(fonts.has_glyph(&font, c), "actual {font:?} lacks U+{:04X} from the full unchanged original {text}", u32::from(c));
                    }
                }
            }
        });
    }

    fn harness_with_original_glyphs(
        mut app: DesignApp,
        installed: bool,
        parent: bool,
        width: f32,
        case: OriginalScriptGlyphs,
    ) -> Harness<'static, Window> {
        let texts = original_script_glyph_texts(case, &app.ui.language);
        app.framework_transform = false;
        app.framework_opacity = installed;
        let ctx = egui::Context::default();
        if installed {
            app.install_framework(&ctx);
        }
        assert_eq!(app.framework_installed, installed, "the case-local install choice selects the real editor route");
        app.egui_ctx = Some(ctx.clone());
        app.raw_input_hook(&ctx, &mut egui::RawInput::default());
        let mut fonts = false;
        let mut original_face_ready = false;
        let mut original_glyphs_ready = false;
        let mut h = Harness::builder().with_context(ctx).with_size(vec2(width, 640.0)).build_ui_state(
            move |ui, w: &mut Window| {
                if !fonts {
                    crate::theme::install_fonts(ui.ctx(), &w.app.ui.language);
                    fonts = true;
                    return;
                }
                if !original_face_ready {
                    install_original_script_arabic(ui.ctx(), &w.app.ui.language, &texts);
                    original_face_ready = true;
                    return;
                }
                if !original_glyphs_ready {
                    assert_original_script_glyphs(ui.ctx(), &texts);
                    original_glyphs_ready = true;
                }
                let builder = if w.sizing { UiBuilder::new().sizing_pass() } else { UiBuilder::new() };
                ui.scope_builder(builder, |ui| {
                    ui.add_enabled_ui(w.enabled, |ui| {
                        w.controls = record(ui, egui::Id::unique("scripts-shelf-workflow"), |ui| {
                            // The real Dock host scrolls this body; preserve that premise for the original unbounded corpus.
                            let scrolled =
                                egui::ScrollArea::vertical().id_salt(("shared-panel-body", "scripts")).auto_shrink([false, true]).show(ui, |ui| {
                                    if w.real_parent {
                                        crate::dock::panel_body(&mut w.app, ui, "scripts");
                                    } else {
                                        scripts(&mut w.app, ui);
                                    }
                                });
                            w.scroll_viewport = Some(scrolled.inner_rect);
                        })
                        .1;
                    });
                });
            },
            Window { app, controls: None, enabled: true, sizing: false, real_parent: parent, scroll_viewport: None },
        );
        h.input_mut().max_texture_side = Some(8192);
        idle(&mut h, 4);
        h
    }

    fn harness(app: DesignApp, installed: bool, parent: bool, width: f32) -> Harness<'static, Window> {
        harness_with_step_dt(app, installed, parent, width, None)
    }
    fn harness_with_step_dt(mut app: DesignApp, installed: bool, parent: bool, width: f32, step_dt: Option<f32>) -> Harness<'static, Window> {
        app.framework_transform = false;
        app.framework_opacity = installed;
        let ctx = egui::Context::default();
        if installed {
            app.install_framework(&ctx);
        }
        assert_eq!(app.framework_installed, installed, "the case-local install choice selects the real editor route");
        app.egui_ctx = Some(ctx.clone());
        app.raw_input_hook(&ctx, &mut egui::RawInput::default());
        let mut fonts = false;
        let builder = Harness::builder().with_context(ctx).with_size(vec2(width, 640.0));
        let builder = if let Some(dt) = step_dt { builder.with_step_dt(dt) } else { builder };
        let mut h = builder.build_ui_state(
            move |ui, w: &mut Window| {
                if !fonts {
                    crate::theme::install_fonts(ui.ctx(), &w.app.ui.language);
                    fonts = true;
                    return;
                }
                let builder = if w.sizing { UiBuilder::new().sizing_pass() } else { UiBuilder::new() };
                ui.scope_builder(builder, |ui| {
                    ui.add_enabled_ui(w.enabled, |ui| {
                        w.controls = record(ui, egui::Id::unique("scripts-shelf-workflow"), |ui| {
                            if w.real_parent {
                                crate::dock::panel_body(&mut w.app, ui, "scripts");
                            } else {
                                scripts(&mut w.app, ui);
                            }
                        })
                        .1;
                    });
                });
            },
            Window { app, controls: None, enabled: true, sizing: false, real_parent: parent, scroll_viewport: None },
        );
        h.input_mut().max_texture_side = Some(8192);
        idle(&mut h, 4);
        h
    }
    fn app() -> DesignApp {
        let mut app = DesignApp::new_for_panel_tests(designcraft_engine::Session::new(), crate::Services::default());
        app.run("file.new", json!({})).unwrap();
        app.ui.scripts = vec![("Duplicate".into(), one()), ("Duplicate".into(), two())];
        app
    }
    fn one() -> String {
        format!("# {}\nframe.create {{\"rect\":[10,10,40,40]}}", "shared complete source prefix ".repeat(4))
    }
    fn two() -> String {
        format!("{}\nframe.create {{\"rect\":[60,10,90,40]}}", one())
    }
    type Mark = (Arc<designcraft_doc::Document>, String, (usize, usize));
    fn mark(h: &Harness<'_, Window>) -> Mark {
        let st = h.state().app.session.active().unwrap();
        (
            st.doc.clone(),
            format!("{:?}", (st.uid, st.identity_epoch, &st.selection.items, &st.selection.text, &st.selection.cells)),
            (st.history.undo.len(), st.history.redo.len()),
        )
    }
    fn unchanged(h: &Harness<'_, Window>, before: &Mark) {
        let now = mark(h);
        assert_eq!(now.0.as_ref(), before.0.as_ref());
        assert_eq!(now.1, before.1);
        assert_eq!(now.2, before.2);
    }
    fn count(h: &Harness<'_, Window>) -> usize {
        h.state().app.session.active().unwrap().doc.spreads.iter().map(|s| s.items.len()).sum()
    }
    fn runs(h: &Harness<'_, Window>, start: usize) -> Vec<serde_json::Value> {
        h.state().app.session.journal[start..].iter().filter(|(id, _)| id == "script.run").map(|(_, p)| p.clone()).collect()
    }
    fn second_row(h: &Harness<'_, Window>) -> egui::Pos2 {
        let rows: Vec<_> = h.get_all_by_label("Duplicate").collect();
        assert_eq!(rows.len(), 2, "the unchanged native shelf has two genuinely duplicate names");
        rows[1].rect().center()
    }

    #[test]
    fn scripts_new_edit_unbounded_plain_strings_escape_and_ui_persistence_keep_document_history() {
        let mut h = harness_with_original_glyphs(app(), true, true, 340.0, OriginalScriptGlyphs::UnboundedName);
        let before = mark(&h);
        let original = h.state().app.ui.scripts.clone();
        let start = h.state().app.session.journal.len();
        click(&mut h, "New");
        assert_eq!(h.state().app.ui.scripts.len(), 3);
        assert_eq!(&h.state().app.ui.scripts[..2], original.as_slice());
        assert_eq!(h.state().app.ui.scripts[2], ("Script 3".into(), "# command.id {json} per line\n".into()));
        let full_name = format!("नाम & وصف {}", "長".repeat(1100));
        edit(&mut h, "Name", &full_name);
        assert_eq!(h.state().app.ui.scripts[2].0, full_name, "plain edits write through before blur");
        key(&mut h, Key::Escape);
        assert_eq!(h.state().app.ui.scripts[2].0, full_name);
        let text = format!("# {}\n{}\n", "語".repeat(1100), two());
        edit(&mut h, "Scripts", &text);
        assert_eq!(h.state().app.ui.scripts[2].1, text);
        key(&mut h, Key::Escape);
        assert_eq!(h.state().app.ui.scripts[2].1, text);
        let saved = serde_json::to_vec(&h.state().app.ui).unwrap();
        let restored: crate::UiState = serde_json::from_slice(&saved).unwrap();
        assert_eq!(restored.scripts, h.state().app.ui.scripts);
        unchanged(&h, &before);
        assert_eq!(h.state().app.session.journal.len(), start);
    }

    #[test]
    fn scripts_real_parent_pointer_runs_exact_full_body_once_with_one_undo_and_redo() {
        let mut a = app();
        a.ui.scripts[0].1 = two();
        let mut h = harness(a, true, true, 340.0);
        let before = mark(&h);
        let start = h.state().app.session.journal.len();
        click(&mut h, "Run");
        assert_eq!(runs(&h, start), vec![json!({"text":two()})]);
        assert_eq!(count(&h), 2);
        assert_eq!(mark(&h).2, (before.2.0 + 1, 0));
        assert!(h.state().app.ui.status.contains("Duplicate") && h.state().app.ui.status.contains('2'));
        let after = mark(&h).0;
        h.state_mut().app.run("edit.undo", json!({})).unwrap();
        idle(&mut h, 2);
        assert_eq!(mark(&h).0.as_ref(), before.0.as_ref());
        h.state_mut().app.run("edit.redo", json!({})).unwrap();
        idle(&mut h, 2);
        assert_eq!(mark(&h).0.as_ref(), after.as_ref());
        let journal = h.state().app.session.journal.len();
        idle(&mut h, 4);
        assert_eq!(h.state().app.session.journal.len(), journal);
        assert_eq!(count(&h), 2);
    }

    #[test]
    fn scripts_natural_keyboard_space_and_enter_each_run_only_one_exact_script() {
        for activation in [Key::Space, Key::Enter] {
            let mut h = harness(app(), true, false, 420.0);
            let before = mark(&h);
            let start = h.state().app.session.journal.len();
            tab_to(&mut h, "Run");
            key(&mut h, activation);
            idle(&mut h, 2);
            assert_eq!(runs(&h, start), vec![json!({"text":one()})]);
            assert_eq!(count(&h), 1);
            assert_eq!(mark(&h).2, (before.2.0 + 1, 0));
            let n = h.state().app.session.journal.len();
            idle(&mut h, 3);
            assert_eq!(h.state().app.session.journal.len(), n);
        }
    }

    #[test]
    fn scripts_duplicate_native_row_selection_and_original_double_click_run_the_exact_slot() {
        let mut h = harness(app(), true, false, 420.0);
        let before = mark(&h);
        let start = h.state().app.session.journal.len();
        let pos = second_row(&h);
        click_at(&mut h, pos);
        unchanged(&h, &before);
        assert_eq!(h.state().app.session.journal.len(), start);
        click(&mut h, "Run");
        assert_eq!(runs(&h, start), vec![json!({"text":two()})]);
        assert_eq!(count(&h), 2);
        let mut double = harness_with_step_dt(app(), true, false, 420.0, Some(1.0 / 60.0));
        let start = double.state().app.session.journal.len();
        let pos = second_row(&double);
        click_at(&mut double, pos);
        click_at(&mut double, pos);
        assert_eq!(runs(&double, start), vec![json!({"text":two()})]);
        assert_eq!(count(&double), 2);
    }

    #[test]
    fn scripts_delete_only_selected_slot_and_empty_shelf_disabled_actions_preserve_domain() {
        let mut h = harness(app(), true, true, 340.0);
        let before = mark(&h);
        let original = h.state().app.ui.scripts.clone();
        let start = h.state().app.session.journal.len();
        let pos = second_row(&h);
        click_at(&mut h, pos);
        click(&mut h, "Delete");
        assert_eq!(h.state().app.ui.scripts, vec![original[0].clone()]);
        click(&mut h, "Delete");
        assert!(h.state().app.ui.scripts.is_empty());
        for caption in ["Run", "Delete"] {
            assert!(element(&h, caption, "Button").facts.as_ref().unwrap().disabled);
        }
        assert_eq!(h.query_all_by_role(egui::accesskit::Role::TextInput).count(), 0);
        unchanged(&h, &before);
        assert_eq!(h.state().app.session.journal.len(), start);
        let e = element(&h, "Run", "Button");
        click_at(&mut h, e.rect.center());
        unchanged(&h, &before);
        assert_eq!(h.state().app.session.journal.len(), start);
        click(&mut h, "New");
        assert_eq!(h.state().app.ui.scripts[0].0, "Script 1");
        unchanged(&h, &before);
    }

    #[test]
    fn scripts_load_uses_real_answered_app_bound_picker_and_host_reader_selecting_complete_utf8_source() {
        use crate::app::file_dialogs::testing::answer_with;
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("مخطط & Script.txt");
        let body = format!("# {}\n{}", "字".repeat(1100), two());
        std::fs::write(&path, body.as_bytes()).unwrap();
        let mut a = app();
        a.services.read = Some(Box::new(|p| std::fs::read(p).map_err(|e| e.to_string())));
        a.answer_dialogs(answer_with(Some(path.to_string_lossy().into_owned())));
        let mut h = harness_with_original_glyphs(a, true, true, 340.0, OriginalScriptGlyphs::LoadedUtf8(path.to_string_lossy().into_owned()));
        let before = mark(&h);
        let old = h.state().app.ui.scripts.clone();
        let start = h.state().app.session.journal.len();
        click(&mut h, "Load…");
        assert_eq!(h.state().app.file_dialogs.queued(), 1);
        assert_eq!(h.state().app.ui.scripts, old);
        h.state_mut().app.settle_dialogs();
        idle(&mut h, 2);
        assert!(!h.state().app.file_dialogs_pending());
        assert_eq!(&h.state().app.ui.scripts[..2], old.as_slice());
        assert_eq!(h.state().app.ui.scripts[2], ("مخطط & Script".into(), body.clone()));
        assert_eq!(h.get_by_role_and_label(egui::accesskit::Role::TextInput, &label(&h, "Name")).value().as_deref(), Some("مخطط & Script"));
        unchanged(&h, &before);
        assert_eq!(h.state().app.session.journal.len(), start);
        click(&mut h, "Run");
        assert_eq!(runs(&h, start), vec![json!({"text":body})]);
        assert_eq!(count(&h), 2);
    }

    #[test]
    fn scripts_canceled_unavailable_missing_and_non_utf8_loads_leave_model_and_history_exact() {
        use crate::app::file_dialogs::testing::answer_with;
        for mode in 0..4 {
            let mut a = app();
            a.services.read = Some(Box::new(move |_| if mode == 3 { Ok(vec![0xff]) } else { Err("reader unavailable".into()) }));
            if mode == 1 {
                a.answer_dialogs(|_| craft_dialogs::Answer::Unavailable("picker unavailable".into()));
            } else {
                a.answer_dialogs(answer_with((mode != 0).then(|| "Unavailable.script".to_owned())));
            }
            let mut h = harness(a, true, false, 420.0);
            let before = mark(&h);
            let old = h.state().app.ui.scripts.clone();
            let start = h.state().app.session.journal.len();
            click(&mut h, "Load…");
            h.state_mut().app.settle_dialogs();
            idle(&mut h, 2);
            assert!(!h.state().app.file_dialogs_pending());
            assert_eq!(h.state().app.ui.scripts, old);
            unchanged(&h, &before);
            assert_eq!(h.state().app.session.journal.len(), start);
            if mode > 0 {
                assert!(!h.state().app.ui.status.is_empty());
            }
        }
    }

    #[test]
    fn scripts_real_failed_prefix_rolls_back_document_selection_and_history_without_rewriting_the_script() {
        let mut a = app();
        let text = format!("{}\nnope.command", one());
        a.ui.scripts[0].1 = text.clone();
        let mut h = harness(a, true, true, 340.0);
        let before = mark(&h);
        let shelf = h.state().app.ui.scripts.clone();
        click(&mut h, "Run");
        unchanged(&h, &before);
        assert_eq!(h.state().app.ui.scripts, shelf);
        assert!(h.state().app.ui.status.contains("step 1") && h.state().app.ui.status.contains("nope.command"));
        let journal = h.state().app.session.journal.len();
        idle(&mut h, 4);
        unchanged(&h, &before);
        assert_eq!(h.state().app.session.journal.len(), journal);
    }

    #[test]
    fn scripts_disabled_parent_and_sdk_sizing_retire_input_and_accept_a_fresh_run() {
        let mut h = harness(app(), true, true, 340.0);
        let e = element(&h, "Run", "Button");
        event(&mut h, Event::PointerMoved(e.rect.center()));
        pointer(&mut h, e.rect.center(), true);
        h.state_mut().enabled = false;
        idle(&mut h, 2);
        let before = mark(&h);
        let shelf = h.state().app.ui.scripts.clone();
        let start = h.state().app.session.journal.len();
        assert!(element(&h, "Run", "Button").facts.as_ref().unwrap().disabled);
        pointer(&mut h, e.rect.center(), false);
        key(&mut h, Key::Space);
        key(&mut h, Key::Enter);
        unchanged(&h, &before);
        assert_eq!(h.state().app.ui.scripts, shelf);
        assert_eq!(h.state().app.session.journal.len(), start);
        h.state_mut().enabled = true;
        idle(&mut h, 2);
        h.state_mut().sizing = true;
        key(&mut h, Key::Enter);
        idle(&mut h, 2);
        unchanged(&h, &before);
        assert_eq!(h.state().app.session.journal.len(), start);
        h.state_mut().sizing = false;
        idle(&mut h, 2);
        click(&mut h, "Run");
        assert_eq!(count(&h), 1);
        assert_eq!(runs(&h, start), vec![json!({"text":one()})]);
    }

    #[test]
    fn scripts_held_run_cannot_follow_selected_body_full_content_or_document_selection_changes() {
        for change in 0..3 {
            let mut h = harness(app(), true, false, 420.0);
            let e = element(&h, "Run", "Button");
            event(&mut h, Event::PointerMoved(e.rect.center()));
            pointer(&mut h, e.rect.center(), true);
            match change {
                0 => h.state_mut().app.ui.scripts[0].1 = two(),
                1 => {
                    let ctx = h.ctx.clone();
                    ctx.data_mut(|d| d.insert_temp(egui::Id::unique("scripts_sel"), 1usize));
                }
                _ => {
                    h.state_mut().app.run("frame.create", json!({"rect":[120,10,150,40]})).unwrap();
                }
            }
            idle(&mut h, 2);
            let before = mark(&h);
            let start = h.state().app.session.journal.len();
            assert_ne!(element(&h, "Run", "Button").id, e.id);
            pointer(&mut h, e.rect.center(), false);
            idle(&mut h, 2);
            unchanged(&h, &before);
            assert_eq!(h.state().app.session.journal.len(), start);
            click(&mut h, "Run");
            assert_eq!(runs(&h, start).len(), 1);
        }
    }

    #[test]
    fn scripts_held_run_rejects_recreated_document_with_same_uid_and_constructor_owned_new_epoch() {
        let mut h = harness(app(), true, false, 420.0);
        let e = element(&h, "Run", "Button");
        let uid = h.state().app.session.active().unwrap().uid;
        let epoch = h.state().app.session.active().unwrap().identity_epoch;
        let doc = h.state().app.session.active().unwrap().doc.as_ref().clone();
        event(&mut h, Event::PointerMoved(e.rect.center()));
        pointer(&mut h, e.rect.center(), true);
        let mut replacement = designcraft_engine::DocState::new(doc, None);
        assert_ne!(replacement.identity_epoch, epoch);
        replacement.uid = uid;
        *h.state_mut().app.session.active_mut().unwrap() = replacement;
        idle(&mut h, 2);
        let before = mark(&h);
        let start = h.state().app.session.journal.len();
        assert_ne!(element(&h, "Run", "Button").id, e.id);
        pointer(&mut h, e.rect.center(), false);
        idle(&mut h, 2);
        unchanged(&h, &before);
        assert_eq!(h.state().app.session.journal.len(), start);
        click(&mut h, "Run");
        assert_eq!(runs(&h, start), vec![json!({"text":one()})]);
        assert_eq!(count(&h), 1);
    }

    #[test]
    fn scripts_german_arabic_parent_names_and_document_save_reopen_preserve_the_persisted_shelf() {
        for language in ["de", "ar"] {
            let mut a = app();
            a.ui.language = language.into();
            let mut h = harness_with_original_glyphs(a, true, true, 340.0, OriginalScriptGlyphs::LocalizedNames);
            for caption in ["Run", "New", "Load…", "Delete"] {
                element(&h, caption, "Button");
            }
            tab_to(&mut h, "Run");
            key(&mut h, Key::Space);
            idle(&mut h, 2);
            assert_eq!(count(&h), 1);
            edit(&mut h, "Name", "नाम & وصف");
            key(&mut h, Key::Escape);
            let shelf = h.state().app.ui.scripts.clone();
            let dir = tempfile::tempdir().unwrap();
            let path = dir.path().join("Script & نص.designcraft");
            let expected = mark(&h).0;
            h.state_mut().app.run("file.saveAs", json!({"path":path})).unwrap();
            h.state_mut().app.run("file.open", json!({"path":path})).unwrap();
            idle(&mut h, 2);
            assert!(path.is_file());
            assert_eq!(h.state().app.session.active().unwrap().doc.spreads, expected.spreads);
            assert_eq!(h.state().app.ui.scripts, shelf);
            let restored: crate::UiState = serde_json::from_slice(&serde_json::to_vec(&h.state().app.ui).unwrap()).unwrap();
            assert_eq!(restored.scripts, shelf);
        }
    }

    #[test]
    fn scripts_uninstalled_native_controls_and_code_editor_keep_real_selection_and_run_without_sdk_facts() {
        let mut h = harness(app(), false, true, 340.0);
        assert!(h.state().controls.as_ref().unwrap().elements.is_empty());
        let singleline = h.get_all_by_role(egui::accesskit::Role::TextInput).count();
        let multiline = h.get_all_by_role(egui::accesskit::Role::MultilineTextInput).count();
        assert_eq!(singleline, 1);
        assert_eq!(multiline, 1);
        assert_eq!(singleline + multiline, 2);
        let before = mark(&h);
        let pos = second_row(&h);
        click_at(&mut h, pos);
        unchanged(&h, &before);
        let run = h.get_by_role_and_label(egui::accesskit::Role::Button, "Run").rect().center();
        let start = h.state().app.session.journal.len();
        click_at(&mut h, run);
        assert_eq!(runs(&h, start), vec![json!({"text":two()})]);
        assert_eq!(count(&h), 2);
        assert_eq!(mark(&h).2, (before.2.0 + 1, 0));
    }

    #[test]
    fn scripts_sdk_and_native_code_editors_keep_tab_shift_tab_and_multiline_text_without_running() {
        for installed in [true, false] {
            let mut h = harness(app(), installed, false, 420.0);
            let before = mark(&h);
            let start = h.state().app.session.journal.len();
            if installed {
                edit(&mut h, "Scripts", "\t# line");
            } else {
                let body = one();
                let full = label(&h, "Scripts");
                let pos = h
                    .get_by(|n| {
                        n.role() == egui::accesskit::Role::MultilineTextInput
                            && n.label().as_deref() == Some(full.as_str())
                            && n.value().as_deref() == Some(body.as_str())
                    })
                    .rect()
                    .center();
                click_at(&mut h, pos);
                key_mod(&mut h, Key::A, Modifiers::COMMAND);
                event(&mut h, Event::Paste("\t# line".into()));
            }
            assert_eq!(h.state().app.ui.scripts[0].1, "\t# line");
            key_mod(&mut h, Key::Tab, Modifiers { shift: true, ..Modifiers::NONE });
            assert_eq!(h.state().app.ui.scripts[0].1, "# line");
            key(&mut h, Key::Home);
            key(&mut h, Key::Tab);
            assert_eq!(h.state().app.ui.scripts[0].1, "\t# line");
            key(&mut h, Key::End);
            key(&mut h, Key::Enter);
            event(&mut h, Event::Text("# two".into()));
            assert_eq!(h.state().app.ui.scripts[0].1, "\t# line\n# two");
            unchanged(&h, &before);
            assert_eq!(h.state().app.session.journal.len(), start);
        }
    }
}
