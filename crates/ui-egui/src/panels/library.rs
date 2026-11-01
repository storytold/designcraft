//! Object Library panel: the open library's items; add the selection, place or delete an item;
//! New / Open library.

use serde_json::json;

use crate::DesignApp;
use crate::app::file_dialogs::Bound;
use crate::theme::Tokens;

pub fn show(app: &mut DesignApp, ui: &mut egui::Ui) {
    let t = Tokens::get(ui.ctx());
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
        if ui.small_button(crate::rtl::widget(ui, crate::i18n::tr(&app.ui.language, "Clear"))).clicked() {
            let _ = app.run("conveyor.clear", json!({}));
        }
    }
    ui.separator();
    ui.horizontal(|ui| {
        if ui.button(crate::rtl::widget(ui, crate::i18n::tr(&app.ui.language, "New Library…"))).clicked() {
            let asked = app.ask_save("Library.dclib", Bound::App, |app, path| app.run("library.new", json!({"path": path})));
            report(app, "Library", asked);
        }
        if ui.button(crate::rtl::widget(ui, crate::i18n::tr(&app.ui.language, "Open…"))).clicked() {
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
    if ui.add_enabled(has_sel, egui::Button::new(crate::rtl::widget(ui, crate::i18n::tr(&app.ui.language, "Add Selection")))).clicked()
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
        ui.horizontal(|ui| {
            crate::rtl::label(ui, it["name"].as_str().unwrap_or(""));
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if ui.small_button(crate::rtl::widget(ui, crate::i18n::tr(&app.ui.language, "Delete"))).clicked() {
                    let _ = app.run("library.remove", json!({"index": i}));
                }
                if ui.small_button(crate::rtl::widget(ui, crate::i18n::tr(&app.ui.language, "Place"))).clicked()
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

fn book_open_buttons(app: &mut DesignApp, ui: &mut egui::Ui) {
    ui.horizontal(|ui| {
        if ui.button(crate::rtl::widget(ui, crate::i18n::tr(&app.ui.language, "New Book…"))).clicked() {
            let asked = app
                .ask_save("Book.dcbook", Bound::App, |app, path| app.run("book.new", json!({"path": path})).or_else(|e| report_err(app, "Book", e)));
            report(app, "Book", asked);
        }
        if ui.button(crate::rtl::widget(ui, crate::i18n::tr(&app.ui.language, "Open…"))).clicked() {
            let asked =
                app.ask_open("book", Bound::App, |app, path| app.run("book.open", json!({"path": path})).or_else(|e| report_err(app, "Book", e)));
            report(app, "Book", asked);
        }
    });
}

fn book_action_buttons(app: &mut DesignApp, ui: &mut egui::Ui) {
    ui.horizontal_wrapped(|ui| {
        if ui.button(crate::rtl::widget(ui, crate::i18n::tr(&app.ui.language, "Add Document…"))).clicked() {
            let asked =
                app.ask_open("open", Bound::App, |app, path| app.run("book.add", json!({"path": path})).or_else(|e| report_err(app, "Book", e)));
            report(app, "Book", asked);
        }
        if ui.button(crate::rtl::widget(ui, crate::i18n::tr(&app.ui.language, "Update Numbering"))).clicked() {
            let _ = app.run("book.paginate", json!({}));
        }
        let availability = designcraft_engine::find_command("book.refreshToc").map(|c| c.info(&app.session));
        let refresh = ui.add_enabled(
            availability.as_ref().is_some_and(|c| c.enabled),
            egui::Button::new(crate::rtl::widget(ui, crate::i18n::tr(&app.ui.language, "Refresh Table of Contents"))),
        );
        if refresh.on_disabled_hover_text(availability.and_then(|c| c.disabled_reason).unwrap_or_default()).clicked()
            && app.run("book.refreshToc", json!({})).is_ok()
        {
            app.status(crate::i18n::tr(&app.ui.language, "Book Table of Contents refreshed.").to_string());
        }
        if ui.button(crate::rtl::widget(ui, crate::i18n::tr(&app.ui.language, "Synchronize"))).clicked() {
            let _ = app.run("book.syncStyles", json!({}));
        }
        if ui.button(crate::rtl::widget(ui, crate::i18n::tr(&app.ui.language, "Export PDF…"))).clicked() {
            let asked = app.run("app.exportBookPdf", json!({}));
            report(app, "Book", asked);
        }
    });
}

#[derive(Clone)]
struct BookDrag {
    path: String,
    index: usize,
}

/// Convert a pointer's insertion gap in the original list to the command's final index.
fn book_drop_target(rows: &[egui::Rect], pointer: egui::Pos2, from: usize) -> Option<(usize, f32)> {
    let first = rows.first()?;
    let last = rows.last()?;
    if !egui::Rect::from_min_max(first.min, last.max).expand(8.0).contains(pointer) || from >= rows.len() {
        return None;
    }
    let gap = rows.iter().position(|r| pointer.y < r.center().y).unwrap_or(rows.len());
    let y = rows.get(gap).map_or(last.bottom(), |r| r.top());
    Some((if gap > from { gap.saturating_sub(1) } else { gap }, y))
}

/// Book panel: the open book's documents with their page ranges; numbering, synchronising and
/// exporting the whole book.
pub fn book(app: &mut DesignApp, ui: &mut egui::Ui) {
    let t = Tokens::get(ui.ctx());
    book_open_buttons(app, ui);
    let Ok(info) = app.session.execute("book.list", &json!({})) else {
        crate::rtl::label(ui, egui::RichText::new(crate::i18n::tr(&app.ui.language, "No book open.")).size(11.0).color(t.text_dim));
        return;
    };
    let source = info["styleSource"].as_u64().unwrap_or(0) as usize;
    ui.separator();
    if info["numberingNeedsUpdate"].as_bool() == Some(true) {
        crate::rtl::label(
            ui,
            egui::RichText::new(crate::i18n::tr(
                &app.ui.language,
                "Book numbering or TOC needs updating: save the TOC, Update Numbering or Refresh Table of Contents.",
            ))
            .color(t.accent),
        );
    }
    let book_path = info["path"].as_str().unwrap_or_default();
    let mut rows = Vec::new();
    for (i, d) in info["documents"].as_array().cloned().unwrap_or_default().iter().enumerate() {
        let path = d["path"].as_str().unwrap_or("").to_string();
        let name = std::path::Path::new(&path).file_stem().map(|n| n.to_string_lossy().to_string()).unwrap_or_default();
        let (first, pages) = (d["firstPage"].as_u64().unwrap_or(1), d["pages"].as_u64().unwrap_or(0));
        let row = ui.horizontal(|ui| {
            if ui
                .selectable_label(i == source, if i == source { "◆" } else { "◇" })
                .on_hover_ui(|ui| {
                    crate::rtl::label(ui, crate::i18n::tr(&app.ui.language, "Style source"));
                })
                .clicked()
            {
                let _ = app.run("book.styleSource", json!({"index": i}));
            }
            let response = ui
                .add(egui::Button::selectable(false, &name).sense(egui::Sense::click_and_drag()))
                .on_hover_text(crate::i18n::tr(&app.ui.language, "Click to open; drag to reorder"));
            response.dnd_set_drag_payload(BookDrag { path: book_path.to_string(), index: i });
            if response.clicked() {
                let _ = app.run("book.openDocument", json!({"index": i}));
            }
            crate::rtl::label(ui, egui::RichText::new(format!("{first}–{}", first + pages.saturating_sub(1))).size(10.5).color(t.text_dim));
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if ui.small_button(crate::rtl::widget(ui, crate::i18n::tr(&app.ui.language, "Remove"))).clicked() {
                    let _ = app.run("book.remove", json!({"index": i}));
                }
            });
        });
        rows.push(row.response.rect);
    }
    // Same payload + insertion-line pattern as the Layers panel; execute only on release.
    if let Some(drag) = egui::DragAndDrop::payload::<BookDrag>(ui.ctx())
        && drag.path == book_path
        && let Some(pointer) = ui.ctx().pointer_latest_pos()
        && let Some((to, y)) = book_drop_target(&rows, pointer, drag.index)
    {
        let x = ui.max_rect().x_range();
        ui.painter().line_segment([egui::pos2(x.min, y), egui::pos2(x.max, y)], egui::Stroke::new(2.0, t.accent));
        if ui.input(|i| i.pointer.any_released()) {
            egui::DragAndDrop::clear_payload(ui.ctx());
            let _ = app.run("book.move", json!({"from": drag.index, "to": to}));
        }
    }
    ui.separator();
    book_action_buttons(app, ui);
}

/// Scripts panel: saved command scripts — run (one undo step), edit, add, delete, load.
pub fn scripts(app: &mut DesignApp, ui: &mut egui::Ui) {
    let t = Tokens::get(ui.ctx());
    let sel_id = egui::Id::unique("scripts_sel");
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
    ui.horizontal(|ui| {
        if ui.add_enabled(!app.ui.scripts.is_empty(), egui::Button::new(crate::rtl::widget(ui, crate::i18n::tr(&app.ui.language, "Run")))).clicked() {
            run = Some(sel);
        }
        if ui.button(crate::rtl::widget(ui, crate::i18n::tr(&app.ui.language, "New"))).clicked() {
            app.ui.scripts.push((format!("Script {}", app.ui.scripts.len() + 1), "# command.id {json} per line\n".into()));
            sel = app.ui.scripts.len() - 1;
        }
        if ui.button(crate::rtl::widget(ui, crate::i18n::tr(&app.ui.language, "Load…"))).clicked() {
            let asked = app.ask_open("script", Bound::App, move |app, path| {
                load_script(app, &path, sel_id);
                Ok(serde_json::Value::Null)
            });
            report(app, "Scripts", asked);
        }
        if ui
            .add_enabled(!app.ui.scripts.is_empty(), egui::Button::new(crate::rtl::widget(ui, crate::i18n::tr(&app.ui.language, "Delete"))))
            .clicked()
            && sel < app.ui.scripts.len()
        {
            app.ui.scripts.remove(sel);
            sel = sel.saturating_sub(1);
        }
    });
    if let Some((name, text)) = app.ui.scripts.get_mut(sel) {
        ui.add(egui::TextEdit::singleline(name).desired_width(f32::INFINITY));
        ui.add(egui::TextEdit::multiline(text).code_editor().desired_rows(8).desired_width(f32::INFINITY));
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

#[cfg(test)]
mod book_drag_tests {
    use super::book_drop_target;
    #[test]
    fn insertion_indicator_and_final_index_match_each_gap() {
        let rows = (0..5).map(|i| egui::Rect::from_min_size(egui::pos2(0.0, i as f32 * 25.0), egui::vec2(200.0, 20.0))).collect::<Vec<_>>();
        assert_eq!(book_drop_target(&rows, egui::pos2(50.0, 23.0), 4), Some((1, 25.0)));
        assert_eq!(book_drop_target(&rows, egui::pos2(50.0, 120.0), 0), Some((4, 120.0)));
        assert_eq!(book_drop_target(&rows, egui::pos2(50.0, 0.0), 4), Some((0, 0.0)));
        assert_eq!(book_drop_target(&rows, egui::pos2(50.0, 49.0), 2), Some((2, 50.0)));
        assert!(book_drop_target(&rows, egui::pos2(250.0, 49.0), 2).is_none());
        assert!(book_drop_target(&[], egui::Pos2::ZERO, 0).is_none());
    }
}
