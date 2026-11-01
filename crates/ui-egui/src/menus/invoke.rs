use super::state::waits_for_ime;
use super::ui_commands::ui_label;
use super::{DesignApp, Value, json};

/// Close document `index` (the active one when `None`) the way the user asks for it: with the
/// tab's ×, File ▸ Close or its shortcut. A document with unsaved changes asks first, because
/// closing also discards its recovery data. Scripts and agents use `file.close`, which never asks.
pub fn close_document(app: &mut DesignApp, index: Option<usize>) {
    let Some(i) = index.or(app.session.active_index()) else { return };
    match app.session.documents().get(i) {
        Some(d) if d.is_dirty() => {
            app.ui.dialog = Some(crate::dialogs::Dialog::new("closeDocument", json!({"uid": d.uid, "title": d.title(), "discard": false})));
        }
        _ => {
            let _ = app.run("file.close", json!({"index": i}));
        }
    }
}

/// A menu item was chosen. An engine command whose label ends in "…" and that takes parameters
/// opens a dialog built from its parameter documentation (see [`crate::dialogs::command_fields`]).
pub fn activate(app: &mut DesignApp, id: &str, params: &Value) {
    if waits_for_ime(app, id) {
        return;
    }
    let was_find = app.ui.dialog.as_ref().is_some_and(|d| d.id == "findChange");
    activate_inner(app, id, params);
    if was_find && app.ui.dialog.as_ref().is_none_or(|d| d.id != "findChange") {
        app.session.reset_find_scope();
    }
}

fn activate_inner(app: &mut DesignApp, id: &str, params: &Value) {
    // Object › Effects › Drop Shadow… (and its shortcut): the Effects dialog on that page.
    if params.is_null()
        && let Some(page) = crate::effects_dialog::page_of_command(id)
    {
        crate::effects_dialog::open(app, page);
        return;
    }
    if params.is_null() && id == "swatch.addColorTheme" {
        crate::dialogs::open_color_theme(app, json!({}));
        return;
    }
    if params.is_null() && id == "file.close" {
        close_document(app, None);
        return;
    }
    if params.is_null() && id == "layout.documentSetup" {
        crate::dialogs::open_document_setup(app);
        return;
    }
    if params.is_null() && id == "file.info" {
        crate::dialogs::open_file_info(app);
        return;
    }
    if params.is_null() && id == "toc.generate" {
        crate::dialogs::open_toc(app);
        return;
    }
    if params.is_null() && id == "file.print" {
        crate::dialogs::open_print(app);
        return;
    }
    if params.is_null() && id == "layout.section" {
        crate::section_options::open(app);
        return;
    }
    if params.is_null() && id == "layout.pages.insert" {
        let cur = crate::canvas::current_page(app).unwrap_or(0);
        crate::dialogs::open_insert_pages(app, cur);
        return;
    }
    if params.is_null() && id == "layout.createGuides" {
        crate::dialogs::open_create_guides(app);
        return;
    }
    if params.is_null() && id == "layout.marginsAndColumns" {
        crate::dialogs::open_margins_and_columns(app);
        return;
    }
    if params.is_null() && id == "object.textFrameOptions" {
        app.ui.dialog = Some(crate::dialogs::Dialog::new("textFrameOptions", json!({})));
        return;
    }
    // New Paragraph/Character Style…: the style options for the new style.
    if params.is_null() && matches!(id, "style.paragraph.create" | "style.character.create") {
        crate::dialogs::open_new_style(app, id == "style.paragraph.create");
        return;
    }
    // Paragraph/Character Style Options…: those of the selected text's style.
    if params.is_null() && matches!(id, "style.paragraph.edit" | "style.character.edit") {
        let para = id == "style.paragraph.edit";
        let cur = crate::panels::text_attrs(app).and_then(|a| a[if para { "paragraphStyle" } else { "characterStyle" }].as_str().map(str::to_string));
        if let Some(name) = cur {
            crate::dialogs::open_style_options(app, para, &name);
            return;
        }
    }
    if params.is_null()
        && ui_label(id).is_none()
        && let Some(c) = designcraft_engine::find_command(id)
        && !crate::dialogs::command_fields(c.params).is_empty()
        && (c.label.ends_with('…') || crate::dialogs::command_fields(c.params).iter().any(|f| !f.optional))
    {
        app.ui.dialog = Some(crate::dialogs::Dialog::new(&format!("cmd:{id}"), json!({})));
        return;
    }
    // Select All Guides: on the spread in view, while guides are shown and not locked.
    if params.is_null() && id == "guide.selectAll" {
        if app.ui.guides
            && !app.ui.guides_locked
            && let Some(spread) = guide_spread(app)
        {
            let _ = app.run(id, json!({"spread": spread}));
        }
        return;
    }
    let p = if params.is_null() { json!({}) } else { params.clone() };
    // Update Link asks before replacing edits made to linked text in the layout.
    if id == "links.update" {
        crate::panels::properties::update_links(app, p);
        return;
    }
    let _ = app.run(id, p);
}

/// The spread the guide commands of the menus act on: the one in view (a parent spread while
/// parents are edited).
pub(super) fn guide_spread(app: &DesignApp) -> Option<Value> {
    let st = app.session.active()?;
    let page = crate::canvas::current_page(app).unwrap_or(0);
    Some(if st.editing_parents {
        let layout = designcraft_tools::CanvasLayout::new(&st.doc, true);
        let shown = crate::canvas::current_slot(app, &layout).and_then(|i| layout.slots.get(i)).map(|s| s.spread);
        json!(shown.unwrap_or(designcraft_doc::SpreadRef::Parent(0)))
    } else {
        json!(st.doc.page_loc(page).map_or(0, |l| l.0))
    })
}

/// A native menu bar item was chosen (macOS): like [`activate`], except that the menu bar takes
/// ⌘A, ⌘C, ⌘X, ⌘V, ⌘Z and ⇧⌘Z before the window sees them, so while a text field has keyboard
/// focus, Select All, Copy, Cut, Paste, Undo and Redo act on that field, as in any Mac app.
pub fn activate_native(app: &mut DesignApp, ctx: &egui::Context, id: &str, params: &Value) {
    if params.is_null() && ctx.text_edit_focused() && forward_to_text_edit(app, ctx, id) {
        return;
    }
    // The canvas text editor is not an egui TextEdit, so its clipboard events must pass through
    // egui for the desktop integration to read/write the macOS clipboard.
    if params.is_null() && app.session.wants_text() && matches!(id, "edit.copy" | "edit.cut" | "edit.paste") && forward_to_text_edit(app, ctx, id) {
        return;
    }
    activate(app, id, params);
}

/// The editing commands a focused text field takes from the native menu bar.
pub(super) const TEXT_FIELD_COMMANDS: [&str; 6] = ["edit.selectAll", "edit.copy", "edit.cut", "edit.paste", "edit.undo", "edit.redo"];

/// Hand a standard editing command to the focused text field as next frame's input; false when
/// `id` isn't one of them.
fn forward_to_text_edit(app: &mut DesignApp, ctx: &egui::Context, id: &str) -> bool {
    let key = |key: egui::Key, modifiers: egui::Modifiers| {
        [true, false].map(|pressed| egui::Event::Key { key, physical_key: None, pressed, repeat: false, modifiers })
    };
    match id {
        "edit.selectAll" => app.synthetic.extend(key(egui::Key::A, egui::Modifiers::COMMAND)),
        "edit.undo" => app.synthetic.extend(key(egui::Key::Z, egui::Modifiers::COMMAND)),
        "edit.redo" => app.synthetic.extend(key(egui::Key::Z, egui::Modifiers::COMMAND | egui::Modifiers::SHIFT)),
        "edit.copy" => app.synthetic.push(egui::Event::Copy),
        "edit.cut" => app.synthetic.push(egui::Event::Cut),
        // Only the integration reads the system clipboard: it answers with an `Event::Paste`.
        "edit.paste" => ctx.send_viewport_cmd(egui::ViewportCommand::RequestPaste),
        _ => return false,
    }
    ctx.request_repaint();
    true
}
