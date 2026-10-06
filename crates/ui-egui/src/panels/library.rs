//! Object Library panel: the open library's items; add the selection, place or delete an item;
//! New / Open library.

use serde_json::json;

use crate::DesignApp;
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
            let path = app.services.pick_save.as_mut().and_then(|f| f("Library.dclib"));
            let _ = app.run("library.new", json!({"path": path}));
        }
        if ui.button(crate::rtl::widget(ui, crate::i18n::tr(&app.ui.language, "Open…"))).clicked()
            && let Some(path) = app.services.pick_open.as_mut().and_then(|f| f("library"))
            && let Err(e) = app.run("library.open", json!({"path": path}))
        {
            app.status(format!("Library: {e}"));
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
        app.status(format!("Library: {e}"));
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
                    app.status(format!("Library: {e}"));
                }
            });
        });
    }
}

/// Book panel: the open book's documents with their page ranges; numbering, synchronising and
/// exporting the whole book.
pub fn book(app: &mut DesignApp, ui: &mut egui::Ui) {
    let t = Tokens::get(ui.ctx());
    ui.horizontal(|ui| {
        if ui.button(crate::rtl::widget(ui, crate::i18n::tr(&app.ui.language, "New Book…"))).clicked()
            && let Some(path) = app.services.pick_save.as_mut().and_then(|f| f("Book.dcbook"))
            && let Err(e) = app.run("book.new", json!({"path": path}))
        {
            app.status(format!("Book: {e}"));
        }
        if ui.button(crate::rtl::widget(ui, crate::i18n::tr(&app.ui.language, "Open…"))).clicked()
            && let Some(path) = app.services.pick_open.as_mut().and_then(|f| f("book"))
            && let Err(e) = app.run("book.open", json!({"path": path}))
        {
            app.status(format!("Book: {e}"));
        }
    });
    let Ok(info) = app.session.execute("book.list", &json!({})) else {
        crate::rtl::label(ui, egui::RichText::new(crate::i18n::tr(&app.ui.language, "No book open.")).size(11.0).color(t.text_dim));
        return;
    };
    let source = info["styleSource"].as_u64().unwrap_or(0) as usize;
    ui.separator();
    for (i, d) in info["documents"].as_array().cloned().unwrap_or_default().iter().enumerate() {
        let path = d["path"].as_str().unwrap_or("").to_string();
        let name = std::path::Path::new(&path).file_stem().map(|n| n.to_string_lossy().to_string()).unwrap_or_default();
        let (first, pages) = (d["firstPage"].as_u64().unwrap_or(1), d["pages"].as_u64().unwrap_or(0));
        ui.horizontal(|ui| {
            if ui
                .selectable_label(i == source, if i == source { "◆" } else { "◇" })
                .on_hover_ui(|ui| {
                    crate::rtl::label(ui, crate::i18n::tr(&app.ui.language, "Style source"));
                })
                .clicked()
            {
                let _ = app.run("book.styleSource", json!({"index": i}));
            }
            if ui
                .selectable_label(false, &name)
                .on_hover_ui(|ui| {
                    crate::rtl::label(ui, crate::i18n::tr(&app.ui.language, "Open"));
                })
                .clicked()
            {
                let _ = app.run("file.open", json!({"path": path}));
            }
            crate::rtl::label(ui, egui::RichText::new(format!("{first}–{}", first + pages.saturating_sub(1))).size(10.5).color(t.text_dim));
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if ui.small_button(crate::rtl::widget(ui, crate::i18n::tr(&app.ui.language, "Remove"))).clicked() {
                    let _ = app.run("book.remove", json!({"index": i}));
                }
            });
        });
    }
    ui.separator();
    ui.horizontal_wrapped(|ui| {
        if ui.button(crate::rtl::widget(ui, crate::i18n::tr(&app.ui.language, "Add Document…"))).clicked()
            && let Some(path) = app.services.pick_open.as_mut().and_then(|f| f("open"))
            && let Err(e) = app.run("book.add", json!({"path": path}))
        {
            app.status(format!("Book: {e}"));
        }
        if ui.button(crate::rtl::widget(ui, crate::i18n::tr(&app.ui.language, "Update Numbering"))).clicked() {
            let _ = app.run("book.paginate", json!({}));
        }
        if ui.button(crate::rtl::widget(ui, crate::i18n::tr(&app.ui.language, "Synchronize"))).clicked() {
            let _ = app.run("book.syncStyles", json!({}));
        }
        if ui.button(crate::rtl::widget(ui, crate::i18n::tr(&app.ui.language, "Export PDF…"))).clicked()
            && let Some(path) = app.services.pick_save.as_mut().and_then(|f| f("Book.pdf"))
            && let Err(e) = app.run("book.exportPdf", json!({"path": path}))
        {
            app.status(format!("Book: {e}"));
        }
    });
}

/// Scripts panel: saved command scripts — run (one undo step), edit, add, delete, load.
pub fn scripts(app: &mut DesignApp, ui: &mut egui::Ui) {
    let t = Tokens::get(ui.ctx());
    let sel_id = egui::Id::new("scripts_sel");
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
        if ui.button(crate::rtl::widget(ui, crate::i18n::tr(&app.ui.language, "Load…"))).clicked()
            && let Some(path) = app.services.pick_open.as_mut().and_then(|f| f("script"))
        {
            #[cfg(not(target_arch = "wasm32"))]
            match std::fs::read_to_string(&path) {
                Ok(text) => {
                    let name = std::path::Path::new(&path).file_stem().map(|n| n.to_string_lossy().to_string()).unwrap_or_else(|| "Script".into());
                    app.ui.scripts.push((name, text));
                    sel = app.ui.scripts.len() - 1;
                }
                Err(e) => app.status(format!("Scripts: {e}")),
            }
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
            Ok(r) => app.status(format!("{name}: {} step(s) done", r["steps"])),
            Err(e) => app.status(format!("{name}: {e}")),
        }
    }
    ui.data_mut(|d| d.insert_temp(sel_id, sel));
}
