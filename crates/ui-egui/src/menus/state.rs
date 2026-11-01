use super::invoke::TEXT_FIELD_COMMANDS;
use super::ui_commands::ui_label;
use super::{DesignApp, Value};

/// Commands that act on the text being typed wait while an IME composes at the caret (its marked
/// text isn't typed yet): Undo and Redo (the native menu takes ⌘Z ahead of the IME), the clipboard
/// and the selection.
pub(crate) fn waits_for_ime(app: &DesignApp, id: &str) -> bool {
    app.session.tool_composing()
        && (matches!(
            id,
            "edit.undo" | "edit.redo" | "edit.cut" | "edit.copy" | "edit.clear" | "edit.duplicate" | "edit.selectAll" | "edit.deselectAll"
        ) || id.starts_with("edit.paste"))
}

/// The commands that act on the desktop window itself.
pub(super) fn is_window_control(id: &str) -> bool {
    matches!(id, "window.minimize" | "window.maximize" | "window.close" | "window.drag" | "window.resize")
}

/// Whether the host can run a window command: all of them with our own title bar, and Minimize
/// with the macOS menu bar (Window › Minimize, ⌘M).
pub(super) fn window_control_available(app: &DesignApp, id: &str) -> bool {
    app.custom_titlebar || (app.native_menu && id == "window.minimize")
}

/// Is a command enabled (for menu greying)?
pub fn enabled(app: &DesignApp, id: &str) -> bool {
    if waits_for_ime(app, id) {
        return false;
    }
    if is_window_control(id) {
        return window_control_available(app, id);
    }
    // Shared docking commands DesignCraft does not provide (native panel windows, Lock Workspace).
    if matches!(id, "window.panel.window" | "window.workspace.lock") {
        return false;
    }
    match designcraft_engine::find_command(id) {
        Some(c) => (c.enabled)(&app.session).is_ok(),
        None => true,
    }
}

/// The default shortcut of a command.
pub fn default_shortcut(id: &str) -> Option<&'static str> {
    designcraft_engine::find_command(id).and_then(|c| c.shortcut).or_else(|| ui_label(id).and_then(|(_, s)| s))
}

/// The shortcut in effect for a command (the user's, else the default), as stored ("Cmd+Alt+K").
pub fn shortcut_of(app: &DesignApp, id: &str) -> Option<String> {
    crate::keys::shortcut(&app.keymap, &craft_keymap::CommandRef::new(id))
}

/// The shortcut a menu row shows: its own binding (a panel's), else its command's.
pub fn shortcut_label(app: &DesignApp, id: &str, params: &Value) -> Option<String> {
    let cmd = crate::keys::command_ref(id, params);
    let cmd = if app.keymap.bindable(&cmd).is_some() { cmd } else { craft_keymap::CommandRef::new(id) };
    crate::keys::shortcut_display(&app.keymap, &cmd)
}

/// The shortcut of a tool, for its button's tooltip.
pub fn tool_shortcut_label(app: &DesignApp, tool: &str) -> Option<String> {
    crate::keys::shortcut_display(&app.keymap, &crate::keys::tool(tool))
}

/// A shortcut for display: ⌃⌥⇧⌘K on macOS, Ctrl+Alt+Shift+K elsewhere.
pub fn shortcut_text(sc: &str) -> String {
    match craft_keymap::Binding::parse(sc) {
        Ok(b) => b.trigger.display(crate::keys::platform()),
        Err(_) => sc.to_string(),
    }
}

/// Check state of a toggle command (None = not a toggle).
pub fn checked(app: &DesignApp, id: &str, params: &Value) -> Option<bool> {
    Some(match id {
        "window.workspace" => params.get("name").and_then(Value::as_str) == Some(app.ui.workspace.as_str()),
        "view.rulers" => app.ui.rulers,
        "view.frameEdges" => app.ui.frame_edges,
        "view.overprintPreview" => app.ui.overprint_preview,
        "view.separations" => {
            let sep = app.ui.separations.as_ref();
            match (params.get("plate"), params.get("inkLimit")) {
                (Some(Value::Null), _) => sep.is_none(),
                (Some(Value::String(s)), _) if s == "all" => sep.is_some_and(|x| x.plate.is_none()),
                (Some(Value::String(s)), _) => sep.and_then(|x| x.plate.as_deref()).is_some_and(|p| p.eq_ignore_ascii_case(s)),
                (None, Some(v)) => sep.and_then(|x| x.ink_limit).is_some_and(|l| Some(f64::from(l)) == v.as_f64()),
                _ => false,
            }
        }
        "view.fastDisplay" => app.ui.display_quality == designcraft_render::DisplayQuality::Fast,
        "view.typicalDisplay" => app.ui.display_quality == designcraft_render::DisplayQuality::Typical,
        "view.highQualityDisplay" => app.ui.display_quality == designcraft_render::DisplayQuality::High,
        "view.guides" => app.ui.guides,
        "view.lockGuides" => app.ui.guides_locked,
        "view.lockColumnGuides" => app.session.active().is_some_and(|d| d.doc.settings.column_guides_locked),
        "view.snapToGuides" => app.ui.snap_to_guides,
        "view.snapToDocumentGrid" => app.ui.snap_to_document_grid,
        "view.smartGuides" => app.ui.smart_guides,
        "view.baselineGrid" => app.ui.baseline_grid,
        "view.documentGrid" => app.ui.document_grid,
        "view.textThreads" => app.ui.text_threads,
        "view.hiddenCharacters" => app.ui.hidden_characters,
        "view.taggedFrames" => app.ui.tagged_frames,
        "view.storyEditorDepthRuler" => app.ui.story_editor_depth_ruler,
        "changes.track" => app.session.active().is_some_and(|d| d.doc.settings.track_changes),
        // Pressed, as the Bulleted and Numbered List buttons: every paragraph the command applies
        // to (selected text, cells or text frames) has it.
        "list.bullets" | "list.numbers" => {
            app.session.selection_has_list(if id == "list.bullets" { designcraft_doc::ListType::Bullets } else { designcraft_doc::ListType::Numbers })
        }
        "window.controlBar" => app.ui.control_bar,
        "window.split" => app.split,
        "window.newWindow" => app.second_window,
        "view.proofColors" => app.ui.proof_colors,
        "view.flattenerPreview" => app.ui.flattener_preview,
        "view.tagMarkers" => app.ui.tag_markers,
        "view.screenMode" => {
            let mode = match params.get("mode").and_then(Value::as_str) {
                Some("normal") => crate::ScreenMode::Normal,
                Some("preview") => crate::ScreenMode::Preview,
                Some("bleed") => crate::ScreenMode::Bleed,
                Some("slug") => crate::ScreenMode::Slug,
                Some("presentation") => crate::ScreenMode::Presentation,
                _ => return None,
            };
            app.ui.screen_mode == mode
        }
        "view.rotateSpread" => match params.get("angle").and_then(Value::as_i64) {
            Some(0) => app.session.active().is_some_and(|st| st.doc.spreads.iter().all(|sp| sp.pages.first().is_none_or(|p| p.view_rotation == 0))),
            _ => return None,
        },
        "view.proofSetup" => match (params.get("target").and_then(Value::as_str), params.get("simulatePaper")) {
            (Some(t), _) => app.ui.proof_setup.target.id() == t,
            (None, Some(_)) => app.ui.proof_setup.simulate_paper,
            _ => return None,
        },
        "edit.dynamicSpelling" => app.ui.dynamic_spelling,
        "app.language" => app.ui.language == params.get("lang").and_then(Value::as_str).unwrap_or(""),
        "app.flattener" => app.ui.flattener == params.get("preset").and_then(Value::as_str).unwrap_or(""),
        "window.taskBar" => app.ui.task_bar,
        "window.taskBarPin" => app.ui.task_bar_pin.is_some(),
        "window.toolsDoubleColumn" => app.ui.tools_double_column,
        "view.togglePreview" => app.ui.screen_mode == crate::ScreenMode::Preview,
        "view.togglePresentation" => app.ui.screen_mode == crate::ScreenMode::Presentation,
        "window.brightness" => params.get("brightness").and_then(Value::as_str) == Some(app.ui.brightness.id()),
        "type.storyDirection" => {
            let st = app.session.active()?;
            let sid = st
                .selection
                .text
                .map(|t| t.story)
                .or_else(|| st.selection.items.iter().find_map(|i| st.doc.item(*i)?.text_frame().map(|t| t.story)))?;
            st.doc.story(sid)?.vertical == params.get("vertical").and_then(Value::as_bool)?
        }
        _ => return None,
    })
}

/// Enablement for menu display (UI commands need a document unless they're app/window-level).
pub fn menu_enabled(app: &DesignApp, id: &str) -> bool {
    if waits_for_ime(app, id) {
        return false;
    }
    if is_window_control(id) {
        return window_control_available(app, id);
    }
    if matches!(id, "app.justificationDialog" | "app.hyphenationDialog") {
        return enabled(app, "type.para");
    }
    if id.starts_with("fit.") {
        return enabled(app, "object.fit");
    }
    if id == "guide.selectAll" && (!app.ui.guides || app.ui.guides_locked) {
        return false;
    }
    // As InDesign's: Convert Text to Table wants selected text outside a table, Convert Table to
    // Text a table.
    if id == "app.convertTextToTableDialog" {
        let selected = app.session.active().and_then(|d| d.selection.text).is_some_and(|t| t.cell.is_none() && !t.is_caret());
        return selected && enabled(app, "table.convertFromText");
    }
    if id == "app.convertTableToTextDialog" {
        return enabled(app, "table.convertToText");
    }
    if id == "view.goBack" || id == "view.goForward" {
        return crate::canvas::can_go_through_history(app, if id == "view.goBack" { -1 } else { 1 });
    }
    if ui_label(id).is_some() {
        return app.session.active().is_some() || id.starts_with("app.") || id.starts_with("window.") || id.starts_with("help.");
    }
    enabled(app, id)
}

/// Whether a native menu bar item can be chosen: as in the in-window menus, but the editing
/// commands a focused text field takes are always available to it.
pub fn native_menu_enabled(app: &DesignApp, ctx: &egui::Context, id: &str) -> bool {
    (ctx.text_edit_focused() && TEXT_FIELD_COMMANDS.contains(&id)) || menu_enabled(app, id)
}
