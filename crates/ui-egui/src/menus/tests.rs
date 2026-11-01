use super::*;

/// The entries (command ids) of a menu bar menu, submenus included.
fn menu_ids(title: &str) -> Vec<&'static str> {
    let items = MENUS.iter().find(|(t, _)| *t == title).map(|(_, items)| *items).unwrap_or_default();
    items.iter().filter_map(|e| e.split_once(':').map(|(_, rest)| rest.split('|').next().unwrap_or(rest))).collect()
}

/// The Object menu's top level in InDesign's order and groups (as InDesign 2023 shows it in
/// Learn Skills Daily's course), with Paths holding Join, Open and Close Path, Convert Shape
/// only shapes, and a Convert Point submenu.
#[test]
fn the_object_menu_is_in_indesigns_order() {
    let items = MENUS.iter().find(|(t, _)| *t == "Object").map(|(_, items)| *items).unwrap();
    // The top-level entries: submenus by name, separators as "-", items by command id.
    let mut top = Vec::new();
    let mut depth = 0;
    for e in items {
        match *e {
            "<" => depth -= 1,
            e if e.starts_with('>') => {
                if depth == 0 {
                    top.push(e.trim_start_matches('>'));
                }
                depth += 1;
            }
            e if depth == 0 => top.push(e.split_once(':').map_or(e, |(_, r)| r.split('|').next().unwrap_or(r))),
            _ => {}
        }
    }
    assert_eq!(
        top,
        [
            "Transform",
            "Transform Again",
            "Arrange",
            "Select",
            "-",
            "object.group",
            "object.ungroup",
            "object.lock",
            "object.unlockAll",
            "object.hide",
            "object.showAll",
            "-",
            "app.qrCode",
            "-",
            "object.textFrameOptions",
            "object.primaryTextFrame",
            "Anchored Object",
            "-",
            "Fitting",
            "Content",
            "-",
            "Effects",
            "object.cornerOptions",
            "-",
            "app.layerOptions",
            "object.exportOptions",
            "Captions",
            "Clipping Path",
            "-",
            "Flex Layout",
            "-",
            "Paths",
            "Pathfinder",
            "Convert Shape",
            "Convert Point",
        ]
    );
    let ids = menu_ids("Object");
    for id in ["path.join", "path.reverse", "path.convertPoints"] {
        assert!(ids.contains(&id), "{id}");
    }
    let sub = |name: &str| {
        let i = items.iter().position(|e| *e == format!(">{name}")).unwrap();
        items[i + 1..].iter().take_while(|e| **e != "<").copied().collect::<Vec<_>>()
    };
    assert!(sub("Paths").iter().any(|e| e.contains("|Open Path|")) && sub("Paths").iter().any(|e| e.contains("|Close Path|")));
    assert!(!sub("Convert Shape").iter().any(|e| e.contains("openPath") || e.contains("closedPath")), "shapes only");
    assert_eq!(sub("Convert Point").len(), 4);
}

#[test]
fn view_items_are_in_the_view_menu_as_in_indesign() {
    let (view, typ) = (menu_ids("View"), menu_ids("Type"));
    for id in [
        "view.rotateSpread",
        "view.proofSetup",
        "view.proofColors",
        "view.flattenerPreview",
        "view.separations",
        "view.taggedFrames",
        "view.tagMarkers",
        "view.storyEditorDepthRuler",
    ] {
        assert!(view.contains(&id), "{id} is in View");
        assert!(!typ.contains(&id), "{id} isn't in Type");
    }
    // Screen Mode and Extras are View submenus; Show Hidden Characters stays in Type.
    assert!(view.contains(&"view.screenMode") && view.contains(&"view.frameEdges"));
    assert!(typ.contains(&"view.hiddenCharacters") && !view.contains(&"view.hiddenCharacters"));
    // W still toggles Normal / Preview without a menu entry.
    assert_eq!(ui_label("view.togglePreview").and_then(|c| c.1), Some("W"));
}

/// Object › Fitting in InDesign's groups: the proportional fits, the frame and content fits,
/// Clear Frame Fitting Options, then the dialog.
#[test]
fn the_fitting_menu_is_grouped_as_indesigns() {
    let items = MENUS.iter().find(|(t, _)| *t == "Object").map(|(_, items)| *items).unwrap();
    let start = items.iter().position(|e| *e == ">Fitting").unwrap() + 1;
    let end = start + items[start..].iter().position(|e| *e == "<").unwrap();
    assert_eq!(
        &items[start..end],
        [
            "ui:fit.fillFrameProportionally",
            "ui:fit.contentProportionally",
            "-",
            "ui:fit.frameToContent",
            "ui:fit.contentToFrame",
            "ui:fit.centerContent",
            "-",
            "cmd:object.clearFittingOptions",
            "-",
            "ui:app.fittingOptions",
        ]
    );
}

/// The submenu paths of a menu bar menu ("View/Screen Mode"), as Edit › Menus keys them.
fn submenu_paths(title: &str) -> Vec<String> {
    let items = MENUS.iter().find(|(t, _)| *t == title).map(|(_, items)| *items).unwrap_or_default();
    let mut stack = vec![title.to_string()];
    let mut out = Vec::new();
    for e in items {
        if let Some(name) = e.strip_prefix('>') {
            let path = format!("{}/{name}", stack.last().map_or("", String::as_str));
            out.push(path.clone());
            stack.push(path);
        } else if *e == "<" {
            stack.pop();
        }
    }
    out
}

/// A command's menu path names the menu entries that really hold it.
#[test]
fn define_lists_command_path_matches_the_type_menu() {
    let spec = designcraft_engine::command_specs().iter().find(|c| c.id == "list.define").expect("list.define is a command");
    assert_eq!(spec.menu[0], "Type");
    assert!(submenu_paths("Type").contains(&spec.menu.join("/")), "{:?} is not a Type submenu", spec.menu);
    assert!(menu_ids("Type").contains(&"list.define"));
}

#[test]
fn screen_mode_shows_the_current_mode() {
    let mut app = DesignApp::new(designcraft_engine::Session::new(), crate::Services::default());
    let mode = |app: &DesignApp, m: &str| checked(app, "view.screenMode", &json!({ "mode": m }));
    assert_eq!((mode(&app, "normal"), mode(&app, "preview")), (Some(true), Some(false)));
    app.ui.screen_mode = crate::ScreenMode::Preview;
    assert_eq!((mode(&app, "normal"), mode(&app, "preview")), (Some(false), Some(true)));
}

fn walk(items: &[Item], out: &mut Vec<(String, String, Value)>) {
    for it in items {
        match it {
            Item::Sub(_, c) => walk(c, out),
            Item::Cmd { label, id, params, .. } => out.push((label.clone(), id.clone(), params.clone())),
            Item::Sep => {}
        }
    }
}

/// Exercise the real dropdown, including clipping and wheel input, on a short viewport.
#[test]
fn oversized_edit_menu_scrolls_to_preferences() {
    let mut app = DesignApp::new(designcraft_engine::Session::new(), crate::Services::default());
    let ctx = egui::Context::default();
    let screen = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(700.0, 260.0));
    let mut tick = 0;
    let mut frame = |app: &mut DesignApp, events: Vec<egui::Event>| {
        tick += 1;
        let mut output =
            ctx.run_ui(egui::RawInput { screen_rect: Some(screen), time: Some(tick as f64 / 60.0), events, ..Default::default() }, |ui| {
                egui::MenuBar::new().ui(ui, |ui| menu_bar(app, ui));
            });
        output.textures_delta.clear();
        let mut labels = Vec::new();
        fn collect(shape: &egui::Shape, clip: egui::Rect, labels: &mut Vec<(String, egui::Rect, egui::Rect)>) {
            match shape {
                egui::Shape::Text(t) => labels.push((t.galley.job.text.clone(), t.visual_bounding_rect(), clip)),
                egui::Shape::Vec(shapes) => {
                    for shape in shapes {
                        collect(shape, clip, labels);
                    }
                }
                _ => {}
            }
        }
        for shape in output.shapes {
            collect(&shape.shape, shape.clip_rect, &mut labels);
        }
        labels
    };
    frame(&mut app, vec![]);
    let labels = frame(&mut app, vec![]);
    let edit = labels.iter().find(|(text, _, _)| text == "Edit").unwrap().1.center();
    for pressed in [true, false] {
        frame(
            &mut app,
            vec![
                egui::Event::PointerMoved(edit),
                egui::Event::PointerButton { pos: edit, button: egui::PointerButton::Primary, pressed, modifiers: egui::Modifiers::NONE },
            ],
        );
    }
    for _ in 0..3 {
        frame(&mut app, vec![]);
    }
    let layer = ctx.layer_id_at(egui::pos2(edit.x + 80.0, 100.0)).expect("menu layer");
    let bounds = ctx.memory(|m| m.area_rect(layer.id)).expect("menu bounds");
    let track = egui::pos2(bounds.right() - 9.0, bounds.bottom() - 25.0);
    for pressed in [true, false] {
        frame(
            &mut app,
            vec![
                egui::Event::PointerMoved(track),
                egui::Event::PointerButton { pos: track, button: egui::PointerButton::Primary, pressed, modifiers: egui::Modifiers::NONE },
            ],
        );
    }
    assert!(egui::Popup::is_any_open(&ctx), "clicking the scroll track must not dismiss the menu");
    frame(
        &mut app,
        vec![
            egui::Event::PointerMoved(egui::pos2(edit.x + 80.0, 100.0)),
            egui::Event::MouseWheel {
                unit: egui::MouseWheelUnit::Point,
                delta: egui::vec2(0.0, -3000.0),
                phase: egui::TouchPhase::Move,
                modifiers: egui::Modifiers::NONE,
                source: egui::MouseWheelSource::Unknown,
            },
        ],
    );
    let mut labels = Vec::new();
    for _ in 0..60 {
        labels = frame(&mut app, vec![]);
    }
    let (_, rect, clip) = labels.iter().find(|(text, _, _)| text == "Preferences…").expect("Preferences is rendered");
    assert!(screen.contains_rect(*rect) && clip.contains_rect(*rect), "Preferences must be visible after scrolling: {rect:?}, {clip:?}");
    let pos = rect.center();
    for pressed in [true, false] {
        frame(
            &mut app,
            vec![
                egui::Event::PointerMoved(pos),
                egui::Event::PointerButton { pos, button: egui::PointerButton::Primary, pressed, modifiers: egui::Modifiers::NONE },
            ],
        );
    }
    assert_eq!(app.ui.dialog.as_ref().map(|d| d.id.as_str()), Some("preferences"));
    assert!(!egui::Popup::is_any_open(&ctx), "choosing a command dismisses the menu");
}

#[test]
fn formatting_affects_text_sends_swatches_to_the_text() {
    let mut app = crate::DesignApp::new(designcraft_engine::Session::new(), crate::Services::default());
    app.run("file.new", json!({})).unwrap();
    for (name, color) in [("Cyan", "#00ffff"), ("Magenta", "#ff00ff")] {
        let created = app.run("swatch.create", json!({"name": name, "color": color})).unwrap();
        assert_eq!(created["name"], name);
    }
    let r = app.run("frame.create", json!({"rect": [72, 72, 300, 200], "content": "text", "text": "words", "caret": false})).unwrap();
    let (fid, sid) = (r["id"].as_u64().unwrap(), r["story"].as_u64().unwrap());
    app.run("selection.set", json!({"ids": [fid]})).unwrap();
    let text_fill = |app: &crate::DesignApp| {
        let st = app.session.active().unwrap().doc.story(designcraft_doc::StoryId(sid)).unwrap().clone();
        st.char_format_at(2).over.fill.clone()
    };
    let frame_fill = |app: &crate::DesignApp| app.session.active().unwrap().doc.item(designcraft_doc::ItemId(fid)).unwrap().fill.swatch.clone();
    assert!(!crate::panels::colors_affect_text(&app));
    crate::panels::apply_swatch(&mut app, false, "C=100 M=0 Y=0 K=0", None).unwrap();
    assert_eq!(frame_fill(&app), "C=100 M=0 Y=0 K=0");
    assert_eq!(text_fill(&app), None);
    let r = run_ui(&mut app, "app.formattingAffectsText", &json!({})).unwrap().unwrap();
    assert_eq!(r["on"], true, "toggles on");
    assert!(crate::panels::colors_affect_text(&app));
    crate::panels::apply_swatch(&mut app, false, "C=0 M=100 Y=0 K=0", Some(0.5)).unwrap();
    assert_eq!(frame_fill(&app), "C=100 M=0 Y=0 K=0", "the frame is left alone");
    assert_eq!(text_fill(&app).as_deref(), Some("C=0 M=100 Y=0 K=0"));
    assert_eq!(crate::panels::color_target(&mut app).map(|t| (t.0, t.1)), Some(("C=0 M=100 Y=0 K=0".to_string(), 0.5)));
}

#[test]
fn transform_menu_items_open_their_dialogs() {
    for id in ["transform.move", "transform.scale", "transform.rotate", "transform.shear"] {
        let mut app = crate::DesignApp::new(designcraft_engine::Session::new(), crate::Services::default());
        activate(&mut app, id, &Value::Null);
        let dialog = app.ui.dialog.as_ref().map(|d| d.id.clone());
        assert_eq!(dialog.as_deref(), Some(format!("cmd:{id}").as_str()), "{id} opens its dialog");
    }
}

#[test]
fn pdf_export_menu_item_opens_the_dialog_or_exports() {
    // No params: the menu item opens the options dialog.
    let mut app = crate::DesignApp::new(designcraft_engine::Session::new(), crate::Services::default());
    activate(&mut app, "app.exportPdf", &Value::Null);
    assert_eq!(app.ui.dialog.as_ref().map(|d| d.id.as_str()), Some("pdfExport"));
    // Explicit options: programmatic callers still export directly, opening no dialog.
    let mut app = crate::DesignApp::new(designcraft_engine::Session::new(), crate::Services::default());
    activate(&mut app, "app.exportPdf", &json!({"path": "/tmp/x.pdf"}));
    assert!(app.ui.dialog.is_none());
}

#[test]
fn snap_preferences_are_a_command() {
    let mut app = crate::DesignApp::new(designcraft_engine::Session::new(), crate::Services::default());
    let r = run_ui(&mut app, "view.snapPreferences", &json!({"smartSpacing": false, "zone": 8})).unwrap().unwrap();
    assert_eq!(r["smartSpacing"], false);
    assert_eq!(r["alignEdges"], true, "unnamed switches keep their value");
    assert_eq!(r["zone"], 8.0);
    assert!(!app.ui.smart_spacing && (app.ui.snap_zone - 8.0).abs() < 1e-9);
    let r = run_ui(&mut app, "view.snapPreferences", &json!({"zone": -3})).unwrap().unwrap();
    assert_eq!(r["zone"], 0.0, "a negative zone turns snapping off");
}

/// Every command with a shortcut in effect: (command key, shortcut), in dispatch order. UI
/// commands come first: where one shares keys with an engine command (⌘N, ⌘O…) it's the one
/// with the dialog.
fn effective_shortcuts(app: &DesignApp) -> Vec<(String, String)> {
    app.keymap.catalog().iter().filter_map(|b| crate::keys::shortcut(&app.keymap, &b.command).map(|s| (b.command.key(), s))).collect()
}

#[test]
#[cfg(not(target_arch = "wasm32"))]
fn shortcut_edits_wait_for_the_dialog_and_the_menu_bar_owns_its_paste_chord() {
    let mut app = crate::DesignApp::new(designcraft_engine::Session::new(), crate::Services::default());
    crate::shortcuts_dialog::open(&mut app, None);
    for (id, p) in [
        ("window.setShortcut", json!({"id": "app.palette", "shortcut": "F2"})),
        ("window.resetShortcuts", json!({})),
        ("window.importShortcuts", json!({"text": ""})),
    ] {
        assert!(run_ui(&mut app, id, &p).unwrap().is_err(), "{id} while the dialog is open");
    }
    crate::shortcuts_dialog::close(&mut app, false).unwrap();
    // A chord of the user's: the default one is the bar's from the start.
    run_ui(&mut app, "window.setShortcut", &json!({"id": "edit.pasteInPlace", "shortcut": "Cmd+Alt+V", "steal": true})).unwrap().unwrap();
    let held = egui::Modifiers::COMMAND | egui::Modifiers::ALT;
    app.native_menu = true;
    assert_eq!(paste_chord_command(&app, held), Some("edit.pasteInPlace"), "before the bar carries it");
    app.native_shortcuts.insert("edit.pasteInPlace".into(), "Cmd+Alt+V".into());
    assert_eq!(paste_chord_command(&app, held), None, "the bar's key equivalent runs it, once");
}

#[test]
fn custom_shortcuts_override_defaults() {
    let mut app = crate::DesignApp::new(designcraft_engine::Session::new(), crate::Services::default());
    assert_eq!(shortcut_of(&app, "app.preferences").as_deref(), Some("Cmd+K"));
    let r = run_ui(&mut app, "window.setShortcut", &json!({"id": "app.preferences", "shortcut": "Cmd+Alt+Shift+9"})).unwrap().unwrap();
    assert_eq!(r["shortcut"], "Cmd+Alt+Shift+9");
    assert!(effective_shortcuts(&app).contains(&("app.preferences".into(), "Cmd+Alt+Shift+9".into())));
    // Taking another command's keys reports the conflict.
    let r = run_ui(&mut app, "window.setShortcut", &json!({"id": "app.palette", "shortcut": "Cmd+Alt+Shift+9"})).unwrap().unwrap();
    assert_eq!(r["conflicts"], json!(["app.preferences"]));
    run_ui(&mut app, "window.setShortcut", &json!({"id": "app.palette", "shortcut": ""})).unwrap().unwrap();
    assert_eq!(shortcut_of(&app, "app.palette"), None);
    assert!(run_ui(&mut app, "window.setShortcut", &json!({"id": "app.palette", "shortcut": "Cmd+Nope"})).unwrap().is_err());
    run_ui(&mut app, "window.resetShortcuts", &json!({})).unwrap().unwrap();
    // ⌘N is the New Document dialog, not a bare file.new.
    let first_cmd_n = effective_shortcuts(&app).into_iter().find(|(_, s)| s == "Cmd+N").map(|(i, _)| i);
    assert_eq!(first_cmd_n.as_deref(), Some("app.newDocumentDialog"));
    assert_eq!(shortcut_of(&app, "app.palette").as_deref(), Some("Cmd+Enter"));
    assert_eq!(
        crate::keys::chord_text(egui::Modifiers { command: true, shift: true, ..Default::default() }, egui::Key::K).as_deref(),
        Some("Cmd+Shift+K")
    );
    if cfg!(target_os = "macos") {
        assert_eq!(shortcut_text("Cmd+Alt+3"), "⌥⌘3");
        assert_eq!(shortcut_text("Cmd+Shift+."), "⇧⌘.");
    } else {
        assert_eq!(shortcut_text("Cmd+Alt+3"), "Ctrl+Alt+3");
    }
}

/// One frame of key presses through the shortcut dispatcher.
fn press_keys(app: &mut crate::DesignApp, ctx: &egui::Context, keys: &[(egui::Key, egui::Modifiers)]) {
    let events = keys
        .iter()
        .flat_map(|&(key, modifiers)| [true, false].map(|pressed| egui::Event::Key { key, physical_key: None, pressed, repeat: false, modifiers }))
        .collect();
    ctx.run_ui(egui::RawInput { events, ..Default::default() }, |ui| shortcuts(app, &ui.ctx().clone())).textures_delta.clear();
}

#[test]
fn shortcuts_saved_as_a_map_by_older_versions_load_unchanged() {
    let saved = br#"{"shortcuts": {"app.preferences": "Cmd+Alt+Shift+9", "app.palette": "", "file.close": [""]}}"#;
    let mut app = crate::DesignApp::new(designcraft_engine::Session::new(), crate::Services::default());
    app.ui = crate::UiState::from_preferences_json(saved).unwrap();
    app.sync_keymap();
    assert_eq!(shortcut_of(&app, "app.preferences").as_deref(), Some("Cmd+Alt+Shift+9"));
    assert_eq!(shortcut_of(&app, "app.palette"), None, "a removed default stays removed");
    assert_eq!(shortcut_of(&app, "file.close"), None);
    assert_eq!(shortcut_of(&app, "app.save").as_deref(), Some("Cmd+S"));
    // Saved again it reads back the same, in the map form an older version reads.
    let resaved = serde_json::to_vec(&app.ui).unwrap();
    assert_eq!(crate::UiState::from_preferences_json(&resaved).unwrap().shortcuts, app.ui.shortcuts);
    let saved: Value = serde_json::from_slice(&resaved).unwrap();
    assert_eq!(saved["shortcuts"], json!({"app.preferences": "Cmd+Alt+Shift+9", "app.palette": "", "file.close": ""}));
}

#[test]
fn panel_shortcuts_open_their_panels() {
    let mut app = crate::DesignApp::new(designcraft_engine::Session::new(), crate::Services::default());
    app.session.execute("file.new", &json!({})).unwrap();
    let ctx = egui::Context::default();
    let shown = |app: &crate::DesignApp, p: &str| crate::panel_docking::showing(app, p);
    assert!(!shown(&app, "layers"));
    press_keys(&mut app, &ctx, &[(egui::Key::F7, egui::Modifiers::NONE)]);
    assert!(shown(&app, "layers"), "F7 opens Layers");
    // A panel without a default key takes one.
    let r = run_ui(&mut app, "window.setShortcut", &json!({"id": "window.panel", "params": {"panel": "links"}, "shortcut": "Shift+F12"}))
        .unwrap()
        .unwrap();
    assert_eq!(r["shortcut"], "Shift+F12");
    press_keys(&mut app, &ctx, &[(egui::Key::F12, egui::Modifiers::SHIFT)]);
    assert!(shown(&app, "links"), "the new key opens Links");
}

#[test]
fn a_reassigned_tool_key_selects_the_tool_and_the_old_key_does_not() {
    let mut app = crate::DesignApp::new(designcraft_engine::Session::new(), crate::Services::default());
    app.session.execute("file.new", &json!({})).unwrap();
    let ctx = egui::Context::default();
    let none = egui::Modifiers::NONE;
    run_ui(&mut app, "window.setShortcut", &json!({"id": "pen", "shortcut": "Y"})).unwrap().unwrap();
    press_keys(&mut app, &ctx, &[(egui::Key::P, none)]);
    assert_eq!(app.session.tool_id(), "selection", "P no longer selects the Pen");
    press_keys(&mut app, &ctx, &[(egui::Key::Y, none)]);
    assert_eq!(app.session.tool_id(), "pen");
    // Tools sharing a key take turns.
    press_keys(&mut app, &ctx, &[(egui::Key::B, none)]);
    assert_eq!(app.session.tool_id(), "contentCollector");
    press_keys(&mut app, &ctx, &[(egui::Key::B, none)]);
    assert_eq!(app.session.tool_id(), "contentPlacer");
}

#[test]
fn split_window_panes_have_their_own_views() {
    let mut app = crate::DesignApp::new(designcraft_engine::Session::new(), crate::Services::default());
    app.session.execute("file.new", &json!({})).unwrap();
    let ctx = egui::Context::default();
    let frame = |app: &mut crate::DesignApp| {
        let input =
            egui::RawInput { screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(1400.0, 900.0))), ..Default::default() };
        let mut out = ctx.run_ui(input, |ui| {
            app.logic(&ui.ctx().clone());
            app.ui(ui);
        });
        out.textures_delta.clear();
    };
    frame(&mut app);
    assert_eq!(run_ui(&mut app, "window.split", &json!({})).unwrap().unwrap(), json!(true));
    frame(&mut app);
    frame(&mut app);
    // Both panes have a canvas, each half as wide.
    let r0 = app.canvas_rect.unwrap();
    let r1 = app.other_pane.as_ref().and_then(|o| o.1).unwrap();
    assert!(r0.width() < 800.0 && r1.width() < 800.0 && (r0.center().x - r1.center().x).abs() > 300.0, "{r0:?} {r1:?}");
    // Zooming the right pane leaves the left alone.
    app.switch_pane(1);
    let z0 = app.view().unwrap().zoom;
    crate::canvas::set_zoom(&mut app, z0 * 3.0);
    app.switch_pane(0);
    assert!((app.view().unwrap().zoom - z0).abs() < 1e-9);
    app.switch_pane(1);
    assert!((app.view().unwrap().zoom - z0 * 3.0).abs() < 1e-6);
    assert_eq!(run_ui(&mut app, "window.split", &json!({})).unwrap().unwrap(), json!(false));
    assert_eq!(app.pane, 0);
    frame(&mut app);
}

#[test]
fn split_window_dispatches_text_once_to_the_shared_document() {
    for focus_pane in [0, 1] {
        let mut app = crate::DesignApp::new(designcraft_engine::Session::new(), crate::Services::default());
        app.run("file.new", json!({})).unwrap();
        let made = app.run("frame.create", json!({"rect": [36, 36, 300, 200], "content": "text"})).unwrap();
        let story = made["story"].as_u64().unwrap();
        app.session.set_tool("type");
        app.run("text.select", json!({"story": story, "anchor": 0, "focus": 0})).unwrap();
        assert!(app.session.wants_text());
        app.run("window.split", json!({"on": true})).unwrap();
        app.focus_pane = focus_pane;

        let ctx = egui::Context::default();
        let frame = |app: &mut crate::DesignApp, events| {
            let input = egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(1400.0, 900.0))),
                events,
                ..Default::default()
            };
            let mut out = ctx.run_ui(input, |ui| {
                app.logic(&ui.ctx().clone());
                app.ui(ui);
            });
            out.textures_delta.clear();
        };
        frame(&mut app, vec![]);
        frame(&mut app, vec![]);
        frame(&mut app, vec![]);
        let undo_before = app.session.active().unwrap().history.undo.len();
        frame(&mut app, vec![egui::Event::Text("A".into())]);
        let text = app.run("story.get", json!({"story": story})).unwrap();
        assert_eq!(text["text"], "A", "one key press with pane {focus_pane} focused");
        assert_eq!(app.session.active().unwrap().history.undo.len(), undo_before + 1);
        app.run("edit.undo", json!({})).unwrap();
        assert_eq!(app.run("story.get", json!({"story": story})).unwrap()["text"], "");

        // A press and key arriving together must use the newly focused pane, whichever
        // canvas is drawn first. Middle-click changes pane focus without moving the caret.
        let other = app.other_pane.as_ref().and_then(|p| p.1).unwrap().center();
        frame(&mut app, vec![egui::Event::PointerMoved(other)]);
        frame(
            &mut app,
            vec![
                egui::Event::PointerButton { pos: other, button: egui::PointerButton::Middle, pressed: true, modifiers: egui::Modifiers::NONE },
                egui::Event::Text("B".into()),
            ],
        );
        assert_eq!(app.focus_pane, 1 - focus_pane);
        assert_eq!(app.run("story.get", json!({"story": story})).unwrap()["text"], "B", "a key in the pane-switch frame is dispatched once");
        assert_eq!(app.session.active().unwrap().history.undo.len(), undo_before + 1);
    }
}

#[test]
fn split_window_click_focus_routes_menu_zoom_but_hover_scroll_stays_local() {
    let mut app = crate::DesignApp::new(designcraft_engine::Session::new(), crate::Services::default());
    app.run("file.new", json!({})).unwrap();
    app.run("window.split", json!({"on": true})).unwrap();
    let ctx = egui::Context::default();
    let mut time = 0.0;
    let mut frame = |app: &mut crate::DesignApp, events| {
        time += 0.1;
        let input = egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(1400.0, 900.0))),
            time: Some(time),
            events,
            ..Default::default()
        };
        let mut out = ctx.run_ui(input, |ui| {
            app.logic(&ui.ctx().clone());
            app.ui(ui);
        });
        out.textures_delta.clear();
    };
    for _ in 0..3 {
        frame(&mut app, vec![]);
    }
    let left = app.canvas_rect.unwrap().center();
    let right = app.other_pane.as_ref().and_then(|p| p.1).unwrap().center();
    let cameras = |app: &mut crate::DesignApp| {
        let current = app.pane;
        app.switch_pane(0);
        let left = *app.view().unwrap();
        app.switch_pane(1);
        let right = *app.view().unwrap();
        app.switch_pane(current);
        [left, right]
    };
    let button = |pos, pressed| egui::Event::PointerButton { pos, button: egui::PointerButton::Primary, pressed, modifiers: egui::Modifiers::NONE };
    frame(&mut app, vec![egui::Event::PointerMoved(right)]);
    frame(&mut app, vec![button(right, true)]);
    frame(&mut app, vec![button(right, false)]);
    assert_eq!((app.pane, app.focus_pane), (1, 1));
    let before = cameras(&mut app);
    app.run("view.zoom", json!({"zoom": 2.0})).unwrap();
    let zoomed = cameras(&mut app);
    assert_eq!(zoomed[0], before[0], "menu zoom leaves the other camera alone");
    assert!((zoomed[1].zoom - 2.0).abs() < 1e-9);

    frame(&mut app, vec![egui::Event::PointerMoved(left)]);
    frame(
        &mut app,
        vec![egui::Event::MouseWheel {
            unit: egui::MouseWheelUnit::Point,
            delta: egui::vec2(0.0, -40.0),
            phase: egui::TouchPhase::Move,
            modifiers: egui::Modifiers::NONE,
            source: egui::MouseWheelSource::Unknown,
        }],
    );
    let scrolled = cameras(&mut app);
    assert_eq!(scrolled[1], zoomed[1], "hover scroll leaves the focused camera alone");
    assert_ne!(scrolled[0].origin, zoomed[0].origin);
    assert_eq!((app.pane, app.focus_pane), (1, 1), "hovering does not move menu focus");

    frame(&mut app, vec![button(left, true)]);
    frame(&mut app, vec![button(left, false)]);
    assert_eq!((app.pane, app.focus_pane), (0, 0));
    app.run("view.zoom", json!({"zoom": 1.0})).unwrap();
    let final_views = cameras(&mut app);
    assert!((final_views[0].zoom - 1.0).abs() < 1e-9);
    assert_eq!(final_views[1], scrolled[1]);
}

#[test]
fn split_window_contextual_bars_have_independent_widget_ids() {
    fn id_warnings(shape: &egui::Shape, found: &mut Vec<String>) {
        match shape {
            egui::Shape::Vec(shapes) => shapes.iter().for_each(|shape| id_warnings(shape, found)),
            egui::Shape::Text(text) if text.galley.text().contains("use of widget ID") => found.push(text.galley.text().to_string()),
            _ => {}
        }
    }

    let mut app = crate::DesignApp::new(designcraft_engine::Session::new(), crate::Services::default());
    app.run("file.new", json!({})).unwrap();
    for rect in [[36, 36, 280, 180], [300, 36, 550, 180]] {
        app.run("frame.create", json!({"rect": rect, "content": "unassigned"})).unwrap();
    }
    app.run("edit.selectAll", json!({})).unwrap();
    app.run("window.split", json!({"on": true})).unwrap();
    let ctx = egui::Context::default();
    ctx.options_mut(|options| options.warn_on_id_clash = true);
    let mut warnings = Vec::new();
    for _ in 0..4 {
        let input =
            egui::RawInput { screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(1400.0, 900.0))), ..Default::default() };
        let mut output = ctx.run_ui(input, |ui| {
            app.logic(&ui.ctx().clone());
            app.ui(ui);
        });
        warnings.clear();
        for shape in &output.shapes {
            id_warnings(&shape.shape, &mut warnings);
        }
        output.textures_delta.clear();
    }
    assert!(warnings.is_empty(), "both bars reused the same interactive widget IDs: {warnings:?}");
    let bars: Vec<_> =
        (0_u8..2).map(|pane| ctx.memory(|memory| memory.area_rect(egui::Id::unique(("task_bar", egui::ViewportId::ROOT, pane)))).unwrap()).collect();
    assert!(!bars[0].intersects(bars[1]), "each pane keeps its own bar: {bars:?}");
}

#[test]
fn show_document_grid_toggles_from_the_grids_and_guides_menu() {
    let mut app = crate::DesignApp::new(designcraft_engine::Session::new(), crate::Services::default());
    let tree = menu_tree();
    let mut items = vec![];
    for (_, entries) in &tree {
        walk(entries, &mut items);
    }
    assert!(items.iter().any(|(label, id, _)| id == "view.documentGrid" && label == "Show/Hide Document Grid"));
    assert_eq!(shortcut_of(&app, "view.documentGrid").as_deref(), Some("Cmd+'"));
    assert_eq!(checked(&app, "view.documentGrid", &json!({})), Some(false));
    assert_eq!(run_ui(&mut app, "view.documentGrid", &json!({})).unwrap().unwrap(), json!(true));
    assert!(app.ui.document_grid);
    assert_eq!(checked(&app, "view.documentGrid", &json!({})), Some(true));
    run_ui(&mut app, "view.documentGrid", &json!({})).unwrap().unwrap();
    assert!(!app.ui.document_grid);
}

#[test]
fn interface_language_switches_and_validates() {
    let mut app = crate::DesignApp::new(designcraft_engine::Session::new(), crate::Services::default());
    run_ui(&mut app, "app.language", &json!({"lang": "de"})).unwrap().unwrap();
    assert_eq!(app.ui.language, "de");
    assert_eq!(checked(&app, "app.language", &json!({"lang": "de"})), Some(true));
    assert!(run_ui(&mut app, "app.language", &json!({"lang": "xx"})).unwrap().is_err());
    assert_eq!(app.ui.language, "de");
    // The language is set in Preferences, not from a menu; without `lang` the command opens it.
    let mut items = Vec::new();
    for (_, entries) in menu_tree() {
        walk(&entries, &mut items);
    }
    assert!(!items.iter().any(|(_, id, _)| id == "app.language"), "no menu item sets the language");
    run_ui(&mut app, "app.language", &json!({})).unwrap().unwrap();
    assert_eq!(app.ui.dialog.as_ref().map(|d| d.id.as_str()), Some("preferences"));
    assert_eq!(app.ui.language, "de");
}

#[test]
fn every_interface_language_is_selectable_persisted_and_keeps_document_data() {
    let mut app = crate::DesignApp::new(designcraft_engine::Session::new(), crate::Services::default());
    app.run("file.new", json!({"pages": 4, "facingPages": true})).unwrap();
    app.run("frame.create", json!({"rect": [36, 36, 200, 200], "content": "text", "text": "My words · Мої слова · Zażółć gęślą jaźń"})).unwrap();
    let before = serde_json::to_value(&app.session.doc().unwrap().doc).unwrap();
    for (code, _) in crate::i18n::LANGUAGES {
        run_ui(&mut app, "app.language", &json!({"lang": code})).unwrap().unwrap();
        assert_eq!(app.ui.language, *code);
        assert_eq!(checked(&app, "app.language", &json!({"lang": code})), Some(true), "{code}");
        assert_eq!(before, serde_json::to_value(&app.session.doc().unwrap().doc).unwrap(), "{code}");
        let bytes = serde_json::to_vec(&app.ui).unwrap();
        let restored: crate::UiState = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(restored.language, *code);
    }
}

#[test]
fn arabic_dialog_and_language_switch_keep_finite_widget_geometry() {
    let mut app = crate::DesignApp::new(designcraft_engine::Session::new(), crate::Services::default());
    let ctx = egui::Context::default();
    let frame = |app: &mut crate::DesignApp| {
        for _ in 0..4 {
            let mut output = ctx.run_ui(
                egui::RawInput {
                    screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(1440.0, 900.0))),
                    max_texture_side: Some(8192),
                    ..Default::default()
                },
                |ui| {
                    app.logic(&ui.ctx().clone());
                    app.ui(ui);
                },
            );
            output.textures_delta.clear();
        }
    };
    app.run("app.language", json!({"lang": "ar"})).unwrap();
    frame(&mut app);
    app.run("app.newDocumentDialog", json!({})).unwrap();
    frame(&mut app); // egui asserts that all allocated rectangles are finite.
    crate::dialogs::confirm(&mut app).unwrap();
    frame(&mut app);
    assert_eq!(app.session.documents().len(), 1);
    for lang in ["uk", "ru", "zh", "", "ar", "pt-br", "it", "cs", "pl", "ka"] {
        app.run("app.language", json!({"lang": lang})).unwrap();
        frame(&mut app);
    }
}

#[test]
fn story_direction_is_checked() {
    let mut app = crate::DesignApp::new(designcraft_engine::Session::new(), crate::Services::default());
    app.session.execute("file.new", &json!({})).unwrap();
    app.session.execute("frame.create", &json!({"rect": [72, 72, 200, 400], "content": "text", "text": "縦", "vertical": true})).unwrap();
    assert_eq!(checked(&app, "type.storyDirection", &json!({"vertical": true})), Some(true));
    assert_eq!(checked(&app, "type.storyDirection", &json!({"vertical": false})), Some(false));
}

#[test]
fn proof_setup_and_colors() {
    let mut app = crate::DesignApp::new(designcraft_engine::Session::new(), crate::Services::default());
    assert_eq!(checked(&app, "view.proofColors", &Value::Null), Some(false));
    let r = run_ui(&mut app, "view.proofSetup", &json!({"target": "deuteranopia"})).unwrap().unwrap();
    assert_eq!(r["on"], true);
    assert_eq!(checked(&app, "view.proofSetup", &json!({"target": "deuteranopia"})), Some(true));
    run_ui(&mut app, "view.proofSetup", &json!({"simulatePaper": "toggle"})).unwrap().unwrap();
    assert!(app.ui.proof_setup.simulate_paper);
    run_ui(&mut app, "view.proofColors", &json!({})).unwrap().unwrap();
    assert!(!app.ui.proof_colors);
    assert!(run_ui(&mut app, "view.proofSetup", &json!({"target": "nope"})).unwrap().is_err());
}

#[test]
fn lock_guides_and_lock_column_guides_are_checked_in_grids_and_guides() {
    let mut app = crate::DesignApp::new(designcraft_engine::Session::new(), crate::Services::default());
    app.session.execute("file.new", &json!({})).unwrap();
    let view = menu_ids("View");
    assert!(view.contains(&"view.lockGuides") && view.contains(&"view.lockColumnGuides"));
    assert_eq!(ui_label("view.lockGuides").and_then(|c| c.1), Some("Cmd+Alt+;"));
    // Lock Guides: off at first, toggles, and `on` sets it.
    assert_eq!(checked(&app, "view.lockGuides", &Value::Null), Some(false));
    assert_eq!(app.run("view.lockGuides", json!({})).unwrap(), json!(true));
    assert!(app.ui.guides_locked);
    assert_eq!(app.run("view.lockGuides", json!({"on": true})).unwrap(), json!(true));
    assert_eq!(checked(&app, "view.lockGuides", &Value::Null), Some(true));
    assert_eq!(app.run("view.lockGuides", json!({"on": false})).unwrap(), json!(false));
    // Lock Column Guides: the document's setting, on in new documents.
    assert_eq!(checked(&app, "view.lockColumnGuides", &Value::Null), Some(true));
    app.run("view.lockColumnGuides", json!({})).unwrap();
    assert_eq!(checked(&app, "view.lockColumnGuides", &Value::Null), Some(false));
}

#[test]
fn fitting_has_indesigns_shortcuts() {
    // LYH Studio episode 6 (7:00): "Fit Content to Frame (Ctrl + Shift + Alt + C)" fills the
    // photo frames proportionally.
    let mut app = crate::DesignApp::new(designcraft_engine::Session::new(), crate::Services::default());
    app.run("file.new", json!({})).unwrap();
    let png = designcraft_render::Rendered { width: 40, height: 20, pixels: vec![200; 40 * 20 * 4] }.to_png();
    let f = app.run("frame.create", json!({"rect": [100, 100, 300, 300]})).unwrap()["id"].as_u64().unwrap();
    app.run("file.place", json!({"base64": designcraft_engine::cmd::base64_encode(&png), "name": "a.png", "frame": f})).unwrap();
    app.run("selection.set", json!({"ids": [f]})).unwrap();
    let width = |app: &crate::DesignApp| {
        let it = app.session.doc().unwrap().doc.item(designcraft_doc::ItemId(f)).unwrap().clone();
        let designcraft_doc::Content::Graphic(g) = &it.content else { panic!("graphic") };
        it.xf.transform_rect_bbox(g.xf.transform_rect_bbox(designcraft_geom::Rect::new(0.0, 0.0, g.size.0, g.size.1))).width()
    };
    for (id, keys) in [
        ("fit.fillFrameProportionally", "Cmd+Alt+Shift+C"),
        ("fit.contentProportionally", "Cmd+Alt+Shift+E"),
        ("fit.frameToContent", "Cmd+Alt+C"),
        ("fit.contentToFrame", "Cmd+Alt+E"),
        ("fit.centerContent", "Cmd+Shift+E"),
    ] {
        assert_eq!(shortcut_of(&app, id).as_deref(), Some(keys));
    }
    let ctx = egui::Context::default();
    let press = |app: &mut crate::DesignApp, key: egui::Key| {
        let modifiers =
            egui::Modifiers { alt: true, ctrl: !cfg!(target_os = "macos"), shift: true, mac_cmd: cfg!(target_os = "macos"), command: true };
        let input = egui::RawInput {
            events: vec![egui::Event::Key { key, physical_key: None, pressed: true, repeat: false, modifiers }],
            ..Default::default()
        };
        let mut out = ctx.run_ui(input, |ui| shortcuts(app, &ui.ctx().clone()));
        out.textures_delta.clear();
    };
    press(&mut app, egui::Key::C);
    assert!((width(&app) - 400.0).abs() < 1e-6, "filled: {}", width(&app));
    press(&mut app, egui::Key::E);
    assert!((width(&app) - 200.0).abs() < 1e-6, "fitted: {}", width(&app));
}

#[test]
fn tab_and_presentation_hide_floating_panels_too() {
    let mut app = crate::DesignApp::new(designcraft_engine::Session::new(), crate::Services::default());
    app.session.execute("file.new", &json!({})).unwrap();
    let ctx = egui::Context::default();
    let frame = |app: &mut crate::DesignApp| {
        for _ in 0..2 {
            let input =
                egui::RawInput { screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(1400.0, 900.0))), ..Default::default() };
            let mut out = ctx.run_ui(input, |ui| {
                app.logic(&ui.ctx().clone());
                app.ui(ui);
            });
            out.textures_delta.clear();
        }
    };
    // The docked panel's tab and a floating panel group.
    let tab = crate::panel_docking::tab_id("preflight");
    let docked = || ctx.read_response(tab).is_some();
    let group = crate::panel_docking::floating_layer("preflight");
    let shown = |layer: egui::LayerId| ctx.memory(|m| m.areas().is_visible(&layer));
    let toggles = [
        ("window.hidePanels", json!({}), json!({})),
        ("window.hidePanelsExceptTools", json!({}), json!({})),
        ("view.screenMode", json!({"mode": "presentation"}), json!({"mode": "normal"})),
    ];
    app.run("window.panel", json!({"panel": "preflight"})).unwrap();
    frame(&mut app);
    assert!(docked());
    for (id, hide, show) in &toggles {
        app.run(id, hide.clone()).unwrap();
        frame(&mut app);
        assert!(!docked(), "{id} hides the dock");
        app.run(id, show.clone()).unwrap();
        frame(&mut app);
        assert!(docked(), "{id} again brings it back");
    }
    app.run("window.floatPanel", json!({"panel": "preflight", "x": 400, "y": 200})).unwrap();
    frame(&mut app);
    assert!(app.ui.docking.is_some() && shown(group));
    for (id, hide, show) in &toggles {
        app.run(id, hide.clone()).unwrap();
        frame(&mut app);
        assert!(!shown(group), "{id} hides the floating panel");
        app.run(id, show.clone()).unwrap();
        frame(&mut app);
        assert!(shown(group), "{id} again brings it back");
    }
}

#[test]
fn tab_hides_panels_and_documents_cycle() {
    let mut app = crate::DesignApp::new(designcraft_engine::Session::new(), crate::Services::default());
    assert_eq!(shortcut_of(&app, "window.hidePanels").as_deref(), Some("Tab"));
    run_ui(&mut app, "window.hidePanels", &json!({})).unwrap().unwrap();
    assert_eq!(app.ui.hidden_panels, 1);
    run_ui(&mut app, "window.hidePanelsExceptTools", &json!({})).unwrap().unwrap();
    assert_eq!(app.ui.hidden_panels, 2);
    run_ui(&mut app, "window.hidePanelsExceptTools", &json!({})).unwrap().unwrap();
    assert_eq!(app.ui.hidden_panels, 0);
    for _ in 0..3 {
        app.session.execute("file.new", &json!({})).unwrap();
    }
    assert_eq!(app.session.active_index(), Some(2));
    run_ui(&mut app, "window.nextDocument", &json!({})).unwrap().unwrap();
    assert_eq!(app.session.active_index(), Some(0));
    run_ui(&mut app, "window.previousDocument", &json!({})).unwrap().unwrap();
    assert_eq!(app.session.active_index(), Some(2));
}

#[test]
fn rotate_spread_view() {
    let mut app = crate::DesignApp::new(designcraft_engine::Session::new(), crate::Services::default());
    app.session.execute("file.new", &json!({"pages": 3})).unwrap();
    let ctx = egui::Context::default();
    let frame = |app: &mut crate::DesignApp| {
        let input =
            egui::RawInput { screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(1400.0, 900.0))), ..Default::default() };
        let mut out = ctx.run_ui(input, |ui| {
            app.logic(&ui.ctx().clone());
            app.ui(ui);
        });
        out.textures_delta.clear();
    };
    frame(&mut app);
    frame(&mut app);
    assert_eq!(run_ui(&mut app, "view.rotateSpread", &json!({"angle": 90})).unwrap().unwrap(), json!(90));
    frame(&mut app);
    // Only the spread in view turns: its canvas footprint swaps width and height.
    let d = app.session.active().unwrap().doc.clone();
    let turned: Vec<usize> = d.spreads.iter().enumerate().filter(|(_, sp)| sp.pages[0].view_rotation == 1).map(|(i, _)| i).collect();
    assert_eq!(turned.len(), 1);
    let layout = designcraft_tools::CanvasLayout::new(&d, false);
    let slot = layout.slots[turned[0]];
    let b = d.spreads[turned[0]].bounds();
    assert!((slot.bounds.width() - b.height()).abs() < 1e-6 && (slot.bounds.height() - b.width()).abs() < 1e-6);
    assert_eq!(checked(&app, "view.rotateSpread", &json!({"angle": 0})), Some(false));
    run_ui(&mut app, "view.rotateSpread", &json!({"angle": 0})).unwrap().unwrap();
    assert_eq!(checked(&app, "view.rotateSpread", &json!({"angle": 0})), Some(true));
    frame(&mut app);
}

#[test]
fn hand_tool_hold_starts_power_zoom() {
    let mut app = crate::DesignApp::new(designcraft_engine::Session::new(), crate::Services::default());
    app.session.execute("file.new", &json!({})).unwrap();
    let ctx = egui::Context::default();
    let frame = |app: &mut crate::DesignApp, t: f64, events: Vec<egui::Event>| {
        let input = egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(1400.0, 900.0))),
            time: Some(t),
            events,
            ..Default::default()
        };
        let mut out = ctx.run_ui(input, |ui| {
            app.logic(&ui.ctx().clone());
            app.ui(ui);
        });
        out.textures_delta.clear();
    };
    frame(&mut app, 0.0, vec![]);
    frame(&mut app, 0.05, vec![]);
    app.select_tool("hand");
    let r = app.canvas_rect.unwrap();
    let p = r.center();
    let z0 = app.view().unwrap().zoom;
    frame(&mut app, 0.1, vec![egui::Event::PointerMoved(p)]);
    frame(
        &mut app,
        0.2,
        vec![egui::Event::PointerButton { pos: p, button: egui::PointerButton::Primary, pressed: true, modifiers: Default::default() }],
    );
    frame(&mut app, 0.4, vec![]);
    assert!(app.power_zoom.is_none(), "not yet");
    frame(&mut app, 0.8, vec![]);
    assert!(app.power_zoom.is_some(), "holding still starts Power Zoom");
    frame(
        &mut app,
        0.9,
        vec![egui::Event::PointerButton { pos: p, button: egui::PointerButton::Primary, pressed: false, modifiers: Default::default() }],
    );
    assert!(app.power_zoom.is_none());
    assert!((app.view().unwrap().zoom - z0).abs() < 1e-9, "back at the zoom it started from");
}

#[test]
fn middle_drag_pans_with_any_tool() {
    let mut app = crate::DesignApp::new(designcraft_engine::Session::new(), crate::Services::default());
    app.session.execute("file.new", &json!({})).unwrap();
    let ctx = egui::Context::default();
    let frame = |app: &mut crate::DesignApp, events: Vec<egui::Event>| {
        let input = egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(1400.0, 900.0))),
            events,
            ..Default::default()
        };
        let mut out = ctx.run_ui(input, |ui| {
            app.logic(&ui.ctx().clone());
            app.ui(ui);
        });
        out.textures_delta.clear();
    };
    frame(&mut app, vec![]);
    frame(&mut app, vec![]);
    // A drawing tool: a primary drag would make a frame, a middle drag must not.
    app.select_tool("rectangleFrame");
    let p = app.canvas_rect.unwrap().center();
    let v0 = *app.view().unwrap();
    let items = |app: &crate::DesignApp| app.session.active().unwrap().doc.spreads.iter().map(|s| s.items.len()).sum::<usize>();
    let items0 = items(&app);
    let button = |pos, pressed| egui::Event::PointerButton { pos, button: egui::PointerButton::Middle, pressed, modifiers: Default::default() };
    frame(&mut app, vec![egui::Event::PointerMoved(p), button(p, true)]);
    for i in 1..=8 {
        frame(&mut app, vec![egui::Event::PointerMoved(p + egui::vec2(10.0 * i as f32, 5.0 * i as f32))]);
    }
    frame(&mut app, vec![button(p + egui::vec2(80.0, 40.0), false)]);
    let v = *app.view().unwrap();
    assert!((v.zoom - v0.zoom).abs() < 1e-9, "panning keeps the zoom");
    assert!(v.origin.x < v0.origin.x && v.origin.y < v0.origin.y, "dragging right/down moves the view: {v0:?} -> {v:?}");
    assert_eq!(items(&app), items0, "the tool saw nothing");
}

#[test]
fn layout_menu_goes_through_pages_and_spreads_with_their_shortcuts() {
    let mut app = crate::DesignApp::new(designcraft_engine::Session::new(), crate::Services::default());
    app.session.execute("file.new", &json!({"pages": 7, "facingPages": true})).unwrap();
    let ctx = egui::Context::default();
    let frame = |app: &mut crate::DesignApp, events: Vec<egui::Event>| {
        let input = egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(1400.0, 900.0))),
            events,
            ..Default::default()
        };
        let mut out = ctx.run_ui(input, |ui| {
            app.logic(&ui.ctx().clone());
            app.ui(ui);
        });
        out.textures_delta.clear();
    };
    let key = |k: egui::Key, modifiers: egui::Modifiers| {
        vec![
            egui::Event::Key { key: k, physical_key: None, pressed: true, repeat: false, modifiers },
            egui::Event::Key { key: k, physical_key: None, pressed: false, repeat: false, modifiers },
        ]
    };
    let page = |app: &crate::DesignApp| crate::canvas::current_page(app).unwrap();
    let spread = |app: &crate::DesignApp| {
        let st = app.session.active().unwrap();
        crate::canvas::current_slot(app, &designcraft_tools::layout::CanvasLayout::new(&st.doc, false)).unwrap()
    };
    frame(&mut app, vec![]);
    frame(&mut app, vec![]);
    // The Layout menu lists them, as InDesign's does, before Go to Page.
    let layout = MENUS.iter().find(|m| m.0 == "Layout").unwrap().1;
    let at = |id: &str| layout.iter().position(|e| e.starts_with(&format!("ui:{id}"))).unwrap();
    assert!(at("view.firstPage") < at("view.previousPage"));
    assert!(at("view.previousPage") < at("view.nextPage"));
    assert!(at("view.nextPage") < at("view.lastPage"));
    assert!(at("view.lastPage") < at("view.nextSpread"));
    assert!(at("view.nextSpread") < at("view.previousSpread"));
    assert!(at("view.previousSpread") < at("view.goToPage"));
    let shift = egui::Modifiers::SHIFT;
    let cmd_shift = egui::Modifiers { shift: true, command: true, ctrl: true, ..Default::default() };
    let alt = egui::Modifiers::ALT;
    // Pages 1 | 2-3 | 4-5 | 6-7.
    frame(&mut app, key(egui::Key::PageDown, shift));
    assert_eq!(page(&app), 1, "Next Page");
    frame(&mut app, key(egui::Key::PageDown, shift));
    assert_eq!(page(&app), 2, "Next Page goes to the right page of the spread");
    frame(&mut app, key(egui::Key::PageUp, shift));
    assert_eq!(page(&app), 1, "Previous Page");
    frame(&mut app, key(egui::Key::PageDown, cmd_shift));
    assert_eq!(page(&app), 6, "Last Page");
    frame(&mut app, key(egui::Key::PageDown, shift));
    assert_eq!(page(&app), 6, "Next Page stops at the last page");
    frame(&mut app, key(egui::Key::PageUp, cmd_shift));
    assert_eq!(page(&app), 0, "First Page");
    frame(&mut app, key(egui::Key::PageUp, shift));
    assert_eq!(page(&app), 0, "Previous Page stops at the first page");
    frame(&mut app, key(egui::Key::PageDown, alt));
    assert_eq!(spread(&app), 1, "Next Spread");
    frame(&mut app, key(egui::Key::PageDown, alt));
    frame(&mut app, key(egui::Key::PageDown, alt));
    frame(&mut app, key(egui::Key::PageDown, alt));
    assert_eq!(spread(&app), 3, "Next Spread stops at the last spread");
    frame(&mut app, key(egui::Key::PageUp, alt));
    assert_eq!(spread(&app), 2, "Previous Spread");
    // The spread is centred, the zoom kept.
    let zoom = app.view().unwrap().zoom;
    app.run("view.previousSpread", json!({})).unwrap();
    let st = app.session.active().unwrap();
    let slot = designcraft_tools::layout::CanvasLayout::new(&st.doc, false).slots[1];
    let (v, r) = (*app.view().unwrap(), app.canvas_rect.unwrap());
    let centre = (v.origin.x + r.width() as f64 / 2.0 / v.zoom, v.origin.y + r.height() as f64 / 2.0 / v.zoom);
    assert!((centre.0 - slot.bounds.center().x).abs() < 1e-6 && (centre.1 - slot.bounds.center().y).abs() < 1e-6);
    assert_eq!(v.zoom, zoom);
}

#[test]
fn go_back_and_go_forward_retrace_the_pages_viewed() {
    let mut app = crate::DesignApp::new(designcraft_engine::Session::new(), crate::Services::default());
    app.session.execute("file.new", &json!({"pages": 7, "facingPages": true})).unwrap();
    let ctx = egui::Context::default();
    let frame = |app: &mut crate::DesignApp, events: Vec<egui::Event>| {
        let input = egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(1400.0, 900.0))),
            events,
            ..Default::default()
        };
        let mut out = ctx.run_ui(input, |ui| {
            app.logic(&ui.ctx().clone());
            app.ui(ui);
        });
        out.textures_delta.clear();
    };
    let key = |k: egui::Key, modifiers: egui::Modifiers| {
        vec![
            egui::Event::Key { key: k, physical_key: None, pressed: true, repeat: false, modifiers },
            egui::Event::Key { key: k, physical_key: None, pressed: false, repeat: false, modifiers },
        ]
    };
    let page = |app: &crate::DesignApp| crate::canvas::current_page(app).unwrap();
    let can = |app: &crate::DesignApp| (menu_enabled(app, "view.goBack"), menu_enabled(app, "view.goForward"));
    frame(&mut app, vec![]);
    frame(&mut app, vec![]);
    // In the Layout menu after Go to Page.
    let layout = MENUS.iter().find(|m| m.0 == "Layout").unwrap().1;
    let at = |id: &str| layout.iter().position(|e| *e == format!("ui:{id}")).unwrap();
    assert_eq!(at("view.goBack"), at("view.goToPage") + 1);
    assert_eq!(at("view.goForward"), at("view.goBack") + 1);
    assert_eq!(can(&app), (false, false), "one page viewed: nowhere to go");
    let cmd = egui::Modifiers { command: true, ctrl: true, ..Default::default() };
    let cmd_shift = egui::Modifiers { shift: true, ..cmd };
    let shift = egui::Modifiers::SHIFT;
    // Pages viewed: 1, 5, 7.
    app.run("view.goToPage", json!({"page": 5})).unwrap();
    frame(&mut app, vec![]);
    frame(&mut app, key(egui::Key::PageDown, cmd_shift));
    assert_eq!(page(&app), 6);
    frame(&mut app, vec![]);
    assert_eq!(can(&app), (true, false));
    frame(&mut app, key(egui::Key::PageUp, cmd));
    assert_eq!(page(&app), 4, "Go Back");
    frame(&mut app, key(egui::Key::PageUp, cmd));
    assert_eq!(page(&app), 0, "Go Back again");
    frame(&mut app, vec![]);
    assert_eq!(can(&app), (false, true), "back at the first page viewed");
    frame(&mut app, key(egui::Key::PageDown, cmd));
    assert_eq!(page(&app), 4, "Go Forward");
    frame(&mut app, key(egui::Key::PageDown, cmd));
    assert_eq!(page(&app), 6, "Go Forward again");
    frame(&mut app, vec![]);
    assert_eq!(can(&app), (true, false));
    // After going back, viewing another page drops the pages ahead.
    frame(&mut app, key(egui::Key::PageUp, cmd));
    assert_eq!(page(&app), 4);
    frame(&mut app, key(egui::Key::PageDown, shift));
    assert_eq!(page(&app), 5, "Next Page");
    frame(&mut app, vec![]);
    assert_eq!(can(&app), (true, false), "nothing ahead after a new page");
    frame(&mut app, key(egui::Key::PageDown, cmd));
    assert_eq!(page(&app), 5, "Go Forward does nothing");
    frame(&mut app, key(egui::Key::PageUp, cmd));
    assert_eq!(page(&app), 4, "Go Back returns to the page before Next Page");
}

#[test]
fn go_back_waits_for_the_view_to_be_fitted() {
    let mut app = crate::DesignApp::new(designcraft_engine::Session::new(), crate::Services::default());
    app.session.execute("file.new", &json!({"pages": 7, "facingPages": true})).unwrap();
    let ctx = egui::Context::default();
    let frame = |app: &mut crate::DesignApp| {
        let input =
            egui::RawInput { screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(1400.0, 900.0))), ..Default::default() };
        let mut out = ctx.run_ui(input, |ui| {
            app.logic(&ui.ctx().clone());
            app.ui(ui);
        });
        out.textures_delta.clear();
    };
    frame(&mut app);
    frame(&mut app);
    app.run("view.goToPage", json!({"page": 5})).unwrap();
    frame(&mut app);
    assert!(menu_enabled(&app, "view.goBack"));
    // Leaving parent pages through the Pages panel: the canvas refits on the next frame.
    app.view_mut().unwrap().fitted = false;
    assert!(!menu_enabled(&app, "view.goBack"), "the refit would replace where Go Back went");
    crate::canvas::go_through_history(&mut app, -1);
    frame(&mut app);
    assert!(menu_enabled(&app, "view.goBack"), "the unfitted Go Back used no history entry");
}

#[test]
fn next_spread_at_the_last_spread_is_no_jump() {
    let mut app = crate::DesignApp::new(designcraft_engine::Session::new(), crate::Services::default());
    app.session.execute("file.new", &json!({"pages": 7, "facingPages": true})).unwrap();
    let ctx = egui::Context::default();
    let frame = |app: &mut crate::DesignApp, events: Vec<egui::Event>| {
        let input = egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(1400.0, 900.0))),
            events,
            ..Default::default()
        };
        let mut out = ctx.run_ui(input, |ui| {
            app.logic(&ui.ctx().clone());
            app.ui(ui);
        });
        out.textures_delta.clear();
    };
    let key = |k: egui::Key, modifiers: egui::Modifiers| {
        vec![
            egui::Event::Key { key: k, physical_key: None, pressed: true, repeat: false, modifiers },
            egui::Event::Key { key: k, physical_key: None, pressed: false, repeat: false, modifiers },
        ]
    };
    let page = |app: &crate::DesignApp| crate::canvas::current_page(app).unwrap();
    let cmd = egui::Modifiers { command: true, ctrl: true, ..Default::default() };
    let alt = egui::Modifiers::ALT;
    frame(&mut app, vec![]);
    frame(&mut app, vec![]);
    // Pages 1 | 2-3 | 4-5 | 6-7: page 6 is the left page of the last spread.
    app.run("view.goToPage", json!({"page": 6})).unwrap();
    frame(&mut app, vec![]);
    assert_eq!(page(&app), 5);
    // Next Spread has no spread to go to; centring the last one moves the view to its fold.
    frame(&mut app, key(egui::Key::PageDown, alt));
    frame(&mut app, key(egui::Key::PageDown, alt));
    assert_ne!(page(&app), 5, "the view moved to the right page");
    frame(&mut app, key(egui::Key::PageUp, cmd));
    assert_eq!(page(&app), 0, "Go Back returns to the page before Go to Page");
    assert!(!menu_enabled(&app, "view.goBack"));
}

#[test]
fn go_back_returns_to_the_view_a_jump_left_and_scrolling_adds_no_entry() {
    let mut app = crate::DesignApp::new(designcraft_engine::Session::new(), crate::Services::default());
    app.session.execute("file.new", &json!({"pages": 7, "facingPages": true})).unwrap();
    let ctx = egui::Context::default();
    let frame = |app: &mut crate::DesignApp, events: Vec<egui::Event>| {
        let input = egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(1400.0, 900.0))),
            events,
            ..Default::default()
        };
        let mut out = ctx.run_ui(input, |ui| {
            app.logic(&ui.ctx().clone());
            app.ui(ui);
        });
        out.textures_delta.clear();
    };
    let key = |k: egui::Key, modifiers: egui::Modifiers| {
        vec![
            egui::Event::Key { key: k, physical_key: None, pressed: true, repeat: false, modifiers },
            egui::Event::Key { key: k, physical_key: None, pressed: false, repeat: false, modifiers },
        ]
    };
    let spread = |app: &crate::DesignApp| {
        let st = app.session.active().unwrap();
        crate::canvas::current_slot(app, &designcraft_tools::layout::CanvasLayout::new(&st.doc, false)).unwrap()
    };
    let cmd = egui::Modifiers { command: true, ctrl: true, ..Default::default() };
    let alt = egui::Modifiers::ALT;
    frame(&mut app, vec![]);
    frame(&mut app, vec![]);
    // Pages 1 | 2-3 | 4-5 | 6-7. Spread 2-3 fitted: the view centre is on the fold.
    frame(&mut app, key(egui::Key::PageDown, alt));
    app.run("view.fitSpread", json!({})).unwrap();
    frame(&mut app, vec![]);
    let before = *app.view().unwrap();
    frame(&mut app, key(egui::Key::PageDown, alt));
    assert_eq!(spread(&app), 2, "Next Spread");
    frame(&mut app, key(egui::Key::PageUp, cmd));
    let v = *app.view().unwrap();
    assert!((v.origin - before.origin).hypot() < 1e-6, "Go Back shows the spread as it was: {:?} vs {:?}", v.origin, before.origin);
    assert_eq!(v.zoom, before.zoom);
    // Zoomed in after the jump: Go Back keeps the magnification in use, as InDesign's does, and
    // centres the same place.
    let centre = |app: &crate::DesignApp| {
        let (v, r) = (app.view().unwrap(), app.canvas_rect.unwrap());
        designcraft_geom::Point::new(v.origin.x + r.width() as f64 / 2.0 / v.zoom, v.origin.y + r.height() as f64 / 2.0 / v.zoom)
    };
    let place = centre(&app);
    frame(&mut app, key(egui::Key::PageDown, alt));
    app.run("view.zoomIn", json!({})).unwrap();
    let zoomed = app.view().unwrap().zoom;
    assert!(zoomed > before.zoom);
    frame(&mut app, key(egui::Key::PageUp, cmd));
    assert_eq!(app.view().unwrap().zoom, zoomed, "the zoom in use is kept");
    assert!((centre(&app) - place).hypot() < 1e-6, "the same place: {:?} vs {place:?}", centre(&app));
    // Scrolling down two spreads is not a jump: Go Forward keeps its page, and Go Back skips
    // from where the scroll ended straight to the page before the first jump.
    let slots = designcraft_tools::layout::CanvasLayout::new(&app.session.active().unwrap().doc, false).slots;
    app.view_mut().unwrap().origin.y += slots[3].bounds.center().y - slots[1].bounds.center().y;
    frame(&mut app, vec![]);
    assert_eq!(spread(&app), 3);
    assert!(menu_enabled(&app, "view.goForward"), "scrolling kept the page ahead");
    frame(&mut app, key(egui::Key::PageUp, cmd));
    assert_eq!(spread(&app), 0, "Go Back");
    assert!(!menu_enabled(&app, "view.goBack"), "the scroll added no entry");
    frame(&mut app, key(egui::Key::PageDown, cmd));
    assert_eq!(spread(&app), 3, "Go Forward returns to where the scroll ended");
    frame(&mut app, key(egui::Key::PageDown, cmd));
    assert_eq!(spread(&app), 2);
}

#[test]
fn go_back_after_presentation_returns_to_the_last_spread_presented() {
    let mut app = crate::DesignApp::new(designcraft_engine::Session::new(), crate::Services::default());
    app.session.execute("file.new", &json!({"pages": 7, "facingPages": true})).unwrap();
    let ctx = egui::Context::default();
    let frame = |app: &mut crate::DesignApp, events: Vec<egui::Event>| {
        let input = egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(1400.0, 900.0))),
            events,
            ..Default::default()
        };
        let mut out = ctx.run_ui(input, |ui| {
            app.logic(&ui.ctx().clone());
            app.ui(ui);
        });
        out.textures_delta.clear();
    };
    let key = |k: egui::Key, modifiers: egui::Modifiers| {
        vec![
            egui::Event::Key { key: k, physical_key: None, pressed: true, repeat: false, modifiers },
            egui::Event::Key { key: k, physical_key: None, pressed: false, repeat: false, modifiers },
        ]
    };
    let spread = |app: &crate::DesignApp| {
        let st = app.session.active().unwrap();
        crate::canvas::current_slot(app, &designcraft_tools::layout::CanvasLayout::new(&st.doc, false)).unwrap()
    };
    let cmd = egui::Modifiers { command: true, ctrl: true, ..Default::default() };
    frame(&mut app, vec![]);
    frame(&mut app, vec![]);
    // Scrolled off the page's centre, so the place Go Back returns to is not just the page.
    let view = app.view_mut().unwrap();
    view.origin.x += 40.0;
    view.origin.y += 60.0;
    frame(&mut app, vec![]);
    assert_eq!(spread(&app), 0);
    let before = *app.view().unwrap();
    // Presentation goes on to spreads 1 and 2; leaving it puts spread 0 back as it was.
    app.run("view.screenMode", json!({"mode": "presentation"})).unwrap();
    frame(&mut app, vec![]);
    frame(&mut app, key(egui::Key::ArrowRight, egui::Modifiers::NONE));
    frame(&mut app, key(egui::Key::ArrowRight, egui::Modifiers::NONE));
    assert_eq!(spread(&app), 2);
    app.run("view.screenMode", json!({"mode": "normal"})).unwrap();
    frame(&mut app, vec![]);
    assert_eq!(spread(&app), 0);
    assert!(!menu_enabled(&app, "view.goForward"));
    frame(&mut app, key(egui::Key::PageUp, cmd));
    assert_eq!(spread(&app), 2, "Go Back shows the last spread presented");
    frame(&mut app, key(egui::Key::PageUp, cmd));
    assert_eq!(spread(&app), 0, "Go Back again, the view Presentation was entered from");
    let v = *app.view().unwrap();
    assert!((v.origin - before.origin).hypot() < 1e-6, "where it was: {:?} vs {:?}", v.origin, before.origin);
    assert!(!menu_enabled(&app, "view.goBack"), "the presentation is one jump: spread 1 isn't kept");
    frame(&mut app, key(egui::Key::PageDown, cmd));
    assert_eq!(spread(&app), 2, "Go Forward");
}

#[test]
fn presentation_shows_one_spread_and_goes_through_them_with_keys_and_clicks() {
    let mut app = crate::DesignApp::new(designcraft_engine::Session::new(), crate::Services::default());
    app.session.execute("file.new", &json!({"pages": 7, "facingPages": true})).unwrap();
    app.session.execute("frame.create", &json!({"rect": [100, 100, 300, 300]})).unwrap();
    let id = app.session.active().unwrap().selection.items[0];
    let ctx = egui::Context::default();
    let frame = |app: &mut crate::DesignApp, events: Vec<egui::Event>| {
        let input = egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(1400.0, 900.0))),
            events,
            ..Default::default()
        };
        let mut out = ctx.run_ui(input, |ui| {
            app.logic(&ui.ctx().clone());
            app.ui(ui);
        });
        out.textures_delta.clear();
    };
    let key = |k: egui::Key| {
        vec![
            egui::Event::Key { key: k, physical_key: None, pressed: true, repeat: false, modifiers: Default::default() },
            egui::Event::Key { key: k, physical_key: None, pressed: false, repeat: false, modifiers: Default::default() },
        ]
    };
    let click = |pos: egui::Pos2, modifiers: egui::Modifiers| {
        vec![
            egui::Event::PointerMoved(pos),
            egui::Event::PointerButton { pos, button: egui::PointerButton::Primary, pressed: true, modifiers },
            egui::Event::PointerButton { pos, button: egui::PointerButton::Primary, pressed: false, modifiers },
        ]
    };
    let spread = |app: &crate::DesignApp| {
        let st = app.session.active().unwrap();
        crate::canvas::current_slot(app, &designcraft_tools::layout::CanvasLayout::new(&st.doc, false)).unwrap()
    };
    // The spread fills the screen: as wide or as tall as it, centred.
    let fills = |app: &crate::DesignApp| {
        let st = app.session.active().unwrap();
        let b = designcraft_tools::layout::CanvasLayout::new(&st.doc, false).slots[spread(app)].bounds;
        let (v, r) = (*app.view().unwrap(), app.canvas_rect.unwrap());
        let (w, h) = (b.width() * v.zoom, b.height() * v.zoom);
        let x0 = (b.x0 - v.origin.x) * v.zoom;
        let y0 = (b.y0 - v.origin.y) * v.zoom;
        ((w - r.width() as f64).abs() < 0.01 || (h - r.height() as f64).abs() < 0.01)
            && w <= r.width() as f64 + 0.01
            && h <= r.height() as f64 + 0.01
            && (x0 * 2.0 + w - r.width() as f64).abs() < 0.01
            && (y0 * 2.0 + h - r.height() as f64).abs() < 0.01
    };
    frame(&mut app, vec![]);
    frame(&mut app, vec![]);
    // The user's own view (zoomed in, scrolled) comes back after Presentation.
    let view = app.view_mut().unwrap();
    view.zoom = 2.0;
    view.origin.x += 150.0;
    view.origin.y += 300.0;
    let before = *app.view().unwrap();
    app.run("view.screenMode", json!({"mode": "presentation"})).unwrap();
    frame(&mut app, vec![]);
    frame(&mut app, vec![]);
    assert_eq!(app.canvas_rect.unwrap().size(), egui::vec2(1400.0, 900.0), "the whole window");
    assert_eq!(spread(&app), 0);
    assert!(fills(&app));
    let bounds = app.session.active().unwrap().doc.item(id).unwrap().bounds();
    frame(&mut app, key(egui::Key::ArrowRight));
    frame(&mut app, vec![]);
    assert_eq!(spread(&app), 1, "Right Arrow shows the next spread");
    assert!(fills(&app));
    assert_eq!(app.session.active().unwrap().doc.item(id).unwrap().bounds(), bounds, "the arrow doesn't nudge the selection");
    frame(&mut app, key(egui::Key::PageDown));
    assert_eq!(spread(&app), 2, "Page Down");
    frame(&mut app, key(egui::Key::ArrowLeft));
    assert_eq!(spread(&app), 1, "Left Arrow");
    frame(&mut app, key(egui::Key::End));
    assert_eq!(spread(&app), 3, "End");
    frame(&mut app, key(egui::Key::ArrowRight));
    assert_eq!(spread(&app), 3, "the last spread stays");
    frame(&mut app, key(egui::Key::Home));
    assert_eq!(spread(&app), 0, "Home");
    frame(&mut app, key(egui::Key::PageUp));
    assert_eq!(spread(&app), 0, "the first spread stays");
    let c = app.canvas_rect.unwrap().center();
    frame(&mut app, click(c, egui::Modifiers::NONE));
    frame(&mut app, vec![]);
    assert_eq!(spread(&app), 1, "a click shows the next spread");
    assert!(fills(&app));
    frame(&mut app, click(c, egui::Modifiers::SHIFT));
    frame(&mut app, vec![]);
    assert_eq!(spread(&app), 0, "Shift-click the previous one");
    assert_eq!(app.session.active().unwrap().selection.items, vec![id], "clicks don't select");
    assert_eq!(app.session.tool_id(), "selection");
    // Out of Presentation, the view is the one it was entered from, and the arrow keys nudge
    // again.
    app.run("view.screenMode", json!({"mode": "normal"})).unwrap();
    assert_eq!(*app.view().unwrap(), before);
    frame(&mut app, vec![]);
    frame(&mut app, key(egui::Key::ArrowRight));
    assert_ne!(app.session.active().unwrap().doc.item(id).unwrap().bounds(), bounds);
}

#[test]
fn tool_shortcuts_work_after_drawing_on_the_canvas() {
    // Clicking the canvas gives it keyboard focus; single-key tool shortcuts must still switch
    // tools without pressing Esc first (#1).
    let mut app = crate::DesignApp::new(designcraft_engine::Session::new(), crate::Services::default());
    app.session.execute("file.new", &json!({})).unwrap();
    let ctx = egui::Context::default();
    let frame = |app: &mut crate::DesignApp, events: Vec<egui::Event>| {
        let input = egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(1400.0, 900.0))),
            events,
            ..Default::default()
        };
        let mut out = ctx.run_ui(input, |ui| {
            app.logic(&ui.ctx().clone());
            app.ui(ui);
        });
        out.textures_delta.clear();
    };
    let key = |k: egui::Key| {
        vec![
            egui::Event::Key { key: k, physical_key: None, pressed: true, repeat: false, modifiers: Default::default() },
            egui::Event::Key { key: k, physical_key: None, pressed: false, repeat: false, modifiers: Default::default() },
        ]
    };
    let button = |pos: egui::Pos2, pressed: bool| egui::Event::PointerButton {
        pos,
        button: egui::PointerButton::Primary,
        pressed,
        modifiers: Default::default(),
    };
    frame(&mut app, vec![]);
    frame(&mut app, vec![]);
    frame(&mut app, key(egui::Key::F));
    assert_eq!(app.session.tool_id(), "rectangleFrame");
    // Draw a frame.
    let a = app.canvas_rect.unwrap().center();
    let b = a + egui::vec2(120.0, 80.0);
    frame(&mut app, vec![egui::Event::PointerMoved(a), button(a, true)]);
    for i in 1..=4 {
        frame(&mut app, vec![egui::Event::PointerMoved(a + (b - a) * (i as f32 / 4.0))]);
    }
    frame(&mut app, vec![button(b, false)]);
    frame(&mut app, vec![]);
    assert!(app.session.active().is_some_and(|d| !d.selection.items.is_empty()), "drew a frame");
    frame(&mut app, key(egui::Key::V));
    assert_eq!(app.session.tool_id(), "selection", "V switches tools straight after drawing");
    frame(&mut app, key(egui::Key::F));
    assert_eq!(app.session.tool_id(), "rectangleFrame");
}

#[test]
fn escape_and_shift_w_leave_presentation_for_the_previous_mode() {
    // Presentation hides every control, so the keyboard must get out of it (#493).
    let mut app = crate::DesignApp::new(designcraft_engine::Session::new(), crate::Services::default());
    app.session.execute("file.new", &json!({})).unwrap();
    app.session.execute("frame.create", &json!({"rect": [36, 36, 300, 200], "content": "graphic"})).unwrap();
    let ctx = egui::Context::default();
    let frame = |app: &mut crate::DesignApp, events: Vec<egui::Event>| {
        let input = egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(1400.0, 900.0))),
            events,
            ..Default::default()
        };
        let mut out = ctx.run_ui(input, |ui| {
            app.logic(&ui.ctx().clone());
            app.ui(ui);
        });
        out.textures_delta.clear();
    };
    let key = |k: egui::Key, modifiers: egui::Modifiers| {
        vec![
            egui::Event::Key { key: k, physical_key: None, pressed: true, repeat: false, modifiers },
            egui::Event::Key { key: k, physical_key: None, pressed: false, repeat: false, modifiers },
        ]
    };
    let selected = |app: &crate::DesignApp| app.session.active().map(|d| d.selection.items.len()).unwrap_or(0);
    frame(&mut app, vec![]);
    frame(&mut app, vec![]);
    app.run("view.screenMode", json!({"mode": "preview"})).unwrap();
    app.select_tool("rectangleFrame");
    assert_eq!(selected(&app), 1, "the new frame is selected");

    frame(&mut app, key(egui::Key::W, egui::Modifiers::SHIFT));
    assert_eq!(app.ui.screen_mode, ScreenMode::Presentation, "Shift+W enters Presentation");
    assert_eq!(app.session.tool_id(), "rectangleFrame", "Shift+W is not a tool shortcut");
    frame(&mut app, key(egui::Key::Escape, egui::Modifiers::NONE));
    assert_eq!(app.ui.screen_mode, ScreenMode::Preview, "Esc returns to the mode Presentation was entered from");
    assert_eq!(app.session.tool_id(), "rectangleFrame", "Esc only left Presentation");
    assert_eq!(selected(&app), 1, "Esc only left Presentation");

    frame(&mut app, key(egui::Key::W, egui::Modifiers::SHIFT));
    frame(&mut app, key(egui::Key::W, egui::Modifiers::SHIFT));
    assert_eq!(app.ui.screen_mode, ScreenMode::Preview, "Shift+W leaves Presentation for the previous mode");

    app.run("view.screenMode", json!({"mode": "presentation"})).unwrap();
    frame(&mut app, key(egui::Key::W, egui::Modifiers::NONE));
    assert_eq!(app.ui.screen_mode, ScreenMode::Normal, "W from any mode but Normal goes to Normal");

    // Outside Presentation, Esc keeps its usual meaning.
    frame(&mut app, key(egui::Key::Escape, egui::Modifiers::NONE));
    assert_eq!(app.ui.screen_mode, ScreenMode::Normal);
    assert_eq!(app.session.tool_id(), "selection");
}

#[test]
fn cmd_plus_zooms_the_canvas_not_the_window() {
    // A German keyboard has its own + key, so Cmd+= arrives as Plus; it must zoom the canvas like
    // Cmd+- does, not hand the key to egui's whole-window zoom.
    let mut app = crate::DesignApp::new(designcraft_engine::Session::new(), crate::Services::default());
    app.session.execute("file.new", &json!({})).unwrap();
    let ctx = egui::Context::default();
    let frame = |app: &mut crate::DesignApp, events: Vec<egui::Event>| {
        let input = egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(1400.0, 900.0))),
            events,
            ..Default::default()
        };
        let mut out = ctx.run_ui(input, |ui| {
            app.logic(&ui.ctx().clone());
            app.ui(ui);
        });
        out.textures_delta.clear();
    };
    let key = |k: egui::Key, modifiers: egui::Modifiers| {
        vec![
            egui::Event::Key { key: k, physical_key: None, pressed: true, repeat: false, modifiers },
            egui::Event::Key { key: k, physical_key: None, pressed: false, repeat: false, modifiers },
        ]
    };
    frame(&mut app, vec![]);
    frame(&mut app, vec![]);
    let scale = ctx.zoom_factor();
    for (k, m, zooms_in) in [
        (egui::Key::Plus, egui::Modifiers::COMMAND, true),
        (egui::Key::Plus, egui::Modifiers::COMMAND | egui::Modifiers::SHIFT, true),
        (egui::Key::Equals, egui::Modifiers::COMMAND, true),
        (egui::Key::Minus, egui::Modifiers::COMMAND, false),
    ] {
        let z0 = app.view().unwrap().zoom;
        frame(&mut app, key(k, m));
        frame(&mut app, vec![]);
        let z = app.view().unwrap().zoom;
        assert!(if zooms_in { z > z0 } else { z < z0 }, "{k:?} {m:?}: {z0} -> {z}");
        assert_eq!(ctx.zoom_factor(), scale, "{k:?} {m:?} leaves the UI scale alone");
    }
    // With a native menu (macOS) its accelerator takes Cmd+= itself, but never sees the Plus key
    // standing in for = (#781).
    app.native_shortcuts.insert("view.zoomIn".to_string(), shortcut_of(&app, "view.zoomIn").unwrap());
    for (k, m, zooms_in) in [
        (egui::Key::Plus, egui::Modifiers::COMMAND, true),
        (egui::Key::Plus, egui::Modifiers::COMMAND | egui::Modifiers::SHIFT, true),
        (egui::Key::Equals, egui::Modifiers::COMMAND, false),
    ] {
        let z0 = app.view().unwrap().zoom;
        frame(&mut app, key(k, m));
        frame(&mut app, vec![]);
        let z = app.view().unwrap().zoom;
        assert_eq!(z > z0, zooms_in, "native menu, {k:?} {m:?}: {z0} -> {z}");
    }
    // A shortcut the user set runs here until the native menu bar carries it as its key
    // equivalent; then only the bar runs it, once.
    run_ui(&mut app, "window.setShortcut", &json!({"id": "view.zoomOut", "shortcut": "Cmd+9"})).unwrap().unwrap();
    let zooms_out = |app: &mut crate::DesignApp, k: egui::Key| {
        let z0 = app.view().unwrap().zoom;
        frame(app, key(k, egui::Modifiers::COMMAND));
        frame(app, vec![]);
        app.view().unwrap().zoom < z0
    };
    assert!(zooms_out(&mut app, egui::Key::Num9), "no native bar: the app runs it");
    app.native_shortcuts.insert("view.zoomOut".to_string(), "Cmd+9".to_string());
    assert!(!zooms_out(&mut app, egui::Key::Num9), "the bar's key equivalent runs only there");
    // A second binding of a command whose first the bar holds runs here.
    let cmd = craft_keymap::CommandRef::new("view.zoomOut");
    let bindings = ["Cmd+F11", "Cmd+9"].map(|s| craft_keymap::Binding::parse(s).unwrap()).to_vec();
    app.keymap.set_bindings(&cmd, bindings, craft_keymap::Policy::Steal).unwrap();
    app.ui.shortcuts = app.keymap.overrides().clone();
    app.native_shortcuts.insert("view.zoomOut".to_string(), "Cmd+F11".to_string());
    assert!(!zooms_out(&mut app, egui::Key::F11), "the bar runs its own key");
    assert!(zooms_out(&mut app, egui::Key::Num9), "the second binding runs in the app");
}

#[test]
fn the_quick_apply_shortcut_opens_it_without_running_an_entry() {
    let mut app = crate::DesignApp::new(designcraft_engine::Session::new(), crate::Services::default());
    app.session.execute("file.new", &json!({})).unwrap();
    let ctx = egui::Context::default();
    let frame = |app: &mut crate::DesignApp, events: Vec<egui::Event>| {
        let input = egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(1400.0, 900.0))),
            events,
            ..Default::default()
        };
        let mut out = ctx.run_ui(input, |ui| {
            app.logic(&ui.ctx().clone());
            app.ui(ui);
        });
        out.textures_delta.clear();
    };
    let cmd_return =
        |pressed| egui::Event::Key { key: egui::Key::Enter, physical_key: None, pressed, repeat: false, modifiers: egui::Modifiers::COMMAND };
    frame(&mut app, vec![]);
    frame(&mut app, vec![]);
    frame(&mut app, vec![cmd_return(true), cmd_return(false)]);
    frame(&mut app, vec![]);
    assert_eq!(app.ui.palette.as_deref(), Some(""), "Quick Apply stays open");
    assert_eq!(app.session.documents().len(), 1, "the first entry (New Document) didn't run");
    // Pressing the shortcut again, or holding it until it repeats, doesn't run it either.
    frame(&mut app, vec![cmd_return(true), cmd_return(false)]);
    let repeat = egui::Event::Key { key: egui::Key::Enter, physical_key: None, pressed: true, repeat: true, modifiers: egui::Modifiers::COMMAND };
    frame(&mut app, vec![repeat, cmd_return(false)]);
    assert_eq!(app.ui.palette.as_deref(), Some(""), "Quick Apply stays open");
    assert_eq!(app.session.documents().len(), 1, "a second ⌘Return or a repeat doesn't run the first entry");
    // A plain Return still does.
    let enter = |pressed| egui::Event::Key { key: egui::Key::Enter, physical_key: None, pressed, repeat: false, modifiers: egui::Modifiers::NONE };
    frame(&mut app, vec![enter(true), enter(false)]);
    assert_eq!(app.ui.palette, None, "Return runs the first entry and closes Quick Apply");
    assert_eq!(app.session.documents().len(), 2);
}

#[test]
fn quick_apply_rows_are_left_aligned() {
    let mut app = crate::DesignApp::new(designcraft_engine::Session::new(), crate::Services::default());
    app.session.execute("file.new", &json!({})).unwrap();
    app.ui.palette = Some(String::new());
    let ctx = egui::Context::default();
    let mut shapes = Vec::new();
    for _ in 0..3 {
        let input =
            egui::RawInput { screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(1400.0, 900.0))), ..Default::default() };
        let mut out = ctx.run_ui(input, |ui| {
            app.logic(&ui.ctx().clone());
            app.ui(ui);
        });
        out.textures_delta.clear();
        shapes = out.shapes;
    }
    let label_x = |label: &str| {
        // The palette is painted last, over anything else with the same text.
        shapes.iter().rev().find_map(|s| match &s.shape {
            egui::Shape::Text(t) if t.galley.text() == label => Some(t.pos.x),
            _ => None,
        })
    };
    // Rows that show a shortcut or a style kind, and rows that show neither, among the first visible ones.
    let items = quick_apply_items(&app.session, "", false);
    let first = items.iter().take(10);
    let plain = first.clone().find(|it| it.shortcut.is_none() && it.kind.is_empty()).expect("a row without a shortcut");
    let with_sc = first.clone().find(|it| it.shortcut.is_some()).expect("a row with a shortcut");
    let (a, b) = (label_x(&plain.label).expect("plain row drawn"), label_x(&with_sc.label).expect("shortcut row drawn"));
    assert!((a - b).abs() < 0.5, "{:?} at x {a}, {:?} at x {b}", plain.label, with_sc.label);
}

#[test]
fn copy_and_paste_keys_work_for_objects_without_a_native_menu() {
    // Without a native menu the paste key arrives only as a paste event, which needs text on
    // the system clipboard (#164, #185).
    let mut app = crate::DesignApp::new(designcraft_engine::Session::new(), crate::Services::default());
    app.session.execute("file.new", &json!({})).unwrap();
    let ctx = egui::Context::default();
    let frame = |app: &mut crate::DesignApp, events: Vec<egui::Event>| {
        let input = egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(1400.0, 900.0))),
            events,
            ..Default::default()
        };
        let mut out = ctx.run_ui(input, |ui| {
            app.logic(&ui.ctx().clone());
            app.ui(ui);
        });
        out.textures_delta.clear();
        out.platform_output.commands
    };
    let count = |app: &crate::DesignApp| app.session.active().unwrap().doc.spreads[0].items.len();
    frame(&mut app, vec![]);
    frame(&mut app, vec![]);
    let id = app.session.execute("frame.create", &json!({"rect": [72, 72, 200, 160]})).unwrap()["id"].clone();
    app.session.execute("selection.set", &json!({"ids": [id]})).unwrap();
    let n = count(&app);
    let copied = frame(&mut app, vec![egui::Event::Copy])
        .into_iter()
        .find_map(|c| match c {
            egui::OutputCommand::CopyText(t) => Some(t),
            _ => None,
        })
        .expect("copying objects puts text on the system clipboard");
    assert!(!copied.is_empty());
    frame(&mut app, vec![egui::Event::Paste(copied.clone())]);
    assert_eq!(count(&app), n + 1, "the paste key pastes the copied rectangle");

    // Pasting into text anchors the copied object; the clipboard placeholder isn't typed.
    let r = app.session.execute("frame.create", &json!({"rect": [72, 300, 400, 400], "content": "text", "text": "Hello"})).unwrap();
    let story = r["story"].as_u64().unwrap();
    app.session.execute("tool.select", &json!({"tool": "type"})).unwrap();
    app.session.execute("text.select", &json!({"story": story, "anchor": 5, "focus": 5})).unwrap();
    frame(&mut app, vec![egui::Event::Paste(copied.clone())]);
    // Shift pastes without formatting.
    frame(&mut app, vec![egui::Event::ModifiersChanged(egui::Modifiers::SHIFT), egui::Event::Paste(copied)]);
    let text = app.session.active().unwrap().doc.story(designcraft_doc::StoryId(story)).unwrap().text.clone();
    assert!(text.starts_with("Hello") && !text.contains('\u{FFFC}'), "{text:?}");
    assert!(text.contains(designcraft_doc::OBJECT_MARK), "{text:?}");
}

#[test]
fn paste_with_no_insertion_point_makes_a_text_frame_from_other_apps_text() {
    // #518: text copied in another app pastes as a new, selected text frame; the text an
    // object copy stands for still pastes the objects. One paste each way, with and without
    // a native menu.
    let selected_text = |app: &crate::DesignApp| {
        let d = app.session.active().unwrap();
        let it = d.doc.item(*d.selection.items.first()?)?;
        Some(d.doc.story(it.text_frame()?.story)?.text.clone())
    };
    let ctx = egui::Context::default();
    let mut app = crate::DesignApp::new(designcraft_engine::Session::new(), crate::Services::default());
    app.session.execute("file.new", &json!({})).unwrap();
    native_frame(&mut app, &ctx, vec![], None);
    native_frame(&mut app, &ctx, vec![], None);
    // The paste keys without a native menu: a paste event.
    native_frame(&mut app, &ctx, vec![egui::Event::Paste("From elsewhere".into())], None);
    assert_eq!(document_items(&app), (1, 1), "pasted once");
    assert_eq!(selected_text(&app).as_deref(), Some("From elsewhere"));
    let id = app.session.execute("frame.create", &json!({"rect": [72, 72, 200, 160]})).unwrap()["id"].clone();
    app.session.execute("selection.set", &json!({"ids": [id]})).unwrap();
    let copied = native_frame(&mut app, &ctx, vec![egui::Event::Copy], None)
        .platform_output
        .commands
        .into_iter()
        .find_map(|c| match c {
            egui::OutputCommand::CopyText(t) => Some(t),
            _ => None,
        })
        .expect("copying objects puts text on the system clipboard");
    native_frame(&mut app, &ctx, vec![egui::Event::Paste(copied)], None);
    assert_eq!(document_items(&app), (3, 1));
    assert_eq!(selected_text(&app), None, "the copied rectangle, not a text frame");

    // A native menu: Paste reads the system clipboard.
    let clip = std::rc::Rc::new(std::cell::RefCell::new(Some("Menu text".to_string())));
    let read = clip.clone();
    let services = crate::Services { clipboard_text: Some(Box::new(move || read.borrow().clone())), ..Default::default() };
    let mut app = crate::DesignApp::new(designcraft_engine::Session::new(), services);
    app.native_menu = true;
    app.session.execute("file.new", &json!({})).unwrap();
    native_frame(&mut app, &ctx, vec![], None);
    native_frame(&mut app, &ctx, vec![], None);
    native_frame(&mut app, &ctx, vec![], Some("edit.paste"));
    native_frame(&mut app, &ctx, vec![], None);
    assert_eq!(document_items(&app), (1, 1), "pasted once");
    assert_eq!(selected_text(&app).as_deref(), Some("Menu text"));
    // Objects copied since that text was copied paste as objects.
    let id = app.session.execute("frame.create", &json!({"rect": [72, 72, 200, 160]})).unwrap()["id"].clone();
    app.session.execute("selection.set", &json!({"ids": [id]})).unwrap();
    native_frame(&mut app, &ctx, vec![], Some("edit.copy"));
    native_frame(&mut app, &ctx, vec![], Some("edit.paste"));
    assert_eq!(document_items(&app), (3, 1));
    assert_eq!(selected_text(&app), None, "the copied rectangle, not a text frame");
    // Newer text from another app wins.
    *clip.borrow_mut() = Some("Newer".into());
    native_frame(&mut app, &ctx, vec![], Some("edit.paste"));
    assert_eq!(document_items(&app), (4, 1));
    assert_eq!(selected_text(&app).as_deref(), Some("Newer"));
}

#[test]
fn sample_scripts_run() {
    let mut app = crate::DesignApp::new(designcraft_engine::Session::new(), crate::Services::default());
    app.session.execute("file.new", &json!({})).unwrap();
    for (name, text) in app.ui.scripts.clone() {
        let r = app.session.execute("script.run", &json!({"text": text}));
        assert!(r.is_ok(), "{name}: {r:?}");
    }
}

#[test]
fn new_window_shows_a_second_view() {
    let mut app = crate::DesignApp::new(designcraft_engine::Session::new(), crate::Services::default());
    app.session.execute("file.new", &json!({})).unwrap();
    let ctx = egui::Context::default();
    let frame = |app: &mut crate::DesignApp| {
        let input =
            egui::RawInput { screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(1400.0, 900.0))), ..Default::default() };
        let mut out = ctx.run_ui(input, |ui| {
            app.logic(&ui.ctx().clone());
            app.ui(ui);
        });
        out.textures_delta.clear();
    };
    frame(&mut app);
    assert_eq!(run_ui(&mut app, "window.newWindow", &json!({})).unwrap().unwrap(), json!(true));
    assert!(!app.split);
    frame(&mut app);
    frame(&mut app);
    // Pane 1 was drawn (its rect is held for it) and has its own view.
    assert!(app.other_pane.as_ref().and_then(|o| o.1).is_some() || app.pane == 1);
    app.switch_pane(1);
    assert!(app.view().is_some(), "the second window has a view");
    app.switch_pane(0);
    assert_eq!(checked(&app, "window.newWindow", &Value::Null), Some(true));
    run_ui(&mut app, "window.newWindow", &json!({"on": false})).unwrap().unwrap();
    frame(&mut app);
}

#[test]
fn panels_float_and_dock() {
    let mut app = crate::DesignApp::new(designcraft_engine::Session::new(), crate::Services::default());
    let r = run_ui(&mut app, "window.floatPanel", &json!({"panel": "swatches", "x": 50, "y": 60})).unwrap().unwrap();
    assert_eq!(r["floating"], json!(["swatches"]));
    let window = &app.ui.docking.as_ref().unwrap().floating[0];
    assert_eq!([window.rect[0], window.rect[1]], [50.0, 60.0]);
    // Showing a floating panel leaves it where it is.
    run_ui(&mut app, "window.panel", &json!({"panel": "swatches"})).unwrap().unwrap();
    assert_eq!(app.ui.open_panel, None);
    let r = run_ui(&mut app, "window.dockPanel", &json!({"panel": "swatches"})).unwrap().unwrap();
    assert_eq!(r["floating"], json!([]));
    assert!(app.ui.docking.as_ref().unwrap().floating.is_empty());
    assert!(run_ui(&mut app, "window.floatPanel", &json!({"panel": "nope"})).unwrap().is_err());
    // Float-only panels (Type › Tabs) have their own windows outside the layout.
    let r = run_ui(&mut app, "window.floatPanel", &json!({"panel": "tabs", "x": 10, "y": 20})).unwrap().unwrap();
    assert_eq!(r["floating"], json!(["tabs"]));
    assert_eq!(app.ui.floating, [("tabs".to_string(), [10.0, 20.0])]);
    run_ui(&mut app, "window.dockPanel", &json!({"panel": "tabs"})).unwrap().unwrap();
    assert!(app.ui.floating.is_empty());
}

/// One frame as the desktop app runs it: queued synthetic input, then the native menu item
/// chosen during the frame (`menu`), then the app's logic and ui.
fn native_frame(app: &mut crate::DesignApp, ctx: &egui::Context, events: Vec<egui::Event>, menu: Option<&str>) -> egui::FullOutput {
    let mut input =
        egui::RawInput { screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(1400.0, 900.0))), events, ..Default::default() };
    app.raw_input_hook(ctx, &mut input);
    let mut menu = menu;
    let mut out = ctx.run_ui(input, |ui| {
        let ctx = ui.ctx().clone();
        if let Some(id) = menu.take() {
            activate_native(app, &ctx, id, &Value::Null);
        }
        app.logic(&ctx);
        app.ui(ui);
    });
    out.textures_delta.clear();
    out
}

/// A document with two frames, nothing selected, and Quick Apply's search field focused
/// holding `text`.
fn app_with_focused_field(ctx: &egui::Context, text: &str) -> crate::DesignApp {
    let mut app = crate::DesignApp::new(designcraft_engine::Session::new(), crate::Services::default());
    app.session.execute("file.new", &json!({})).unwrap();
    app.session.execute("frame.create", &json!({"rect": [36, 36, 200, 200]})).unwrap();
    app.session.execute("frame.create", &json!({"rect": [236, 36, 400, 200]})).unwrap();
    app.session.execute("edit.deselectAll", &json!({})).unwrap();
    app.ui.palette = Some(text.into());
    for _ in 0..3 {
        native_frame(&mut app, ctx, vec![], None);
    }
    assert!(ctx.text_edit_focused());
    app
}

fn field_selection(ctx: &egui::Context) -> Option<(usize, usize)> {
    let id = ctx.memory(|m| m.focused())?;
    let r = egui::TextEdit::load_state(ctx, id)?.cursor.char_range()?;
    Some((r.primary.index.min(r.secondary.index).into(), r.primary.index.max(r.secondary.index).into()))
}

fn document_items(app: &crate::DesignApp) -> (usize, usize) {
    let d = app.session.active().unwrap();
    (d.doc.spreads.iter().flat_map(|sp| &sp.items).count(), d.selection.items.len())
}

#[test]
fn native_select_all_selects_the_focused_fields_text() {
    // The Mac menu bar takes ⌘A before the window sees it (#30).
    let ctx = egui::Context::default();
    let mut app = app_with_focused_field(&ctx, "frame");
    native_frame(&mut app, &ctx, vec![], Some("edit.selectAll"));
    native_frame(&mut app, &ctx, vec![], None);
    assert_eq!(field_selection(&ctx), Some((0, 5)), "the field's whole text is selected");
    assert_eq!(document_items(&app), (2, 0), "the document selection is unchanged");
}

#[test]
fn native_paste_and_copy_act_on_the_focused_field() {
    let ctx = egui::Context::default();
    let mut app = app_with_focused_field(&ctx, "frame");
    native_frame(&mut app, &ctx, vec![], Some("edit.selectAll"));
    native_frame(&mut app, &ctx, vec![], None);
    // Copy puts the field's selected text on the clipboard.
    native_frame(&mut app, &ctx, vec![], Some("edit.copy"));
    let out = native_frame(&mut app, &ctx, vec![], None);
    assert!(out.platform_output.commands.contains(&egui::OutputCommand::CopyText("frame".into())), "{:?}", out.platform_output.commands);
    // Paste asks the integration for the clipboard, which it hands to the field as a paste.
    let out = native_frame(&mut app, &ctx, vec![], Some("edit.paste"));
    let commands = &out.viewport_output[&egui::ViewportId::ROOT].commands;
    assert!(commands.contains(&egui::ViewportCommand::RequestPaste), "{commands:?}");
    native_frame(&mut app, &ctx, vec![egui::Event::Paste("text".into())], None);
    assert_eq!(app.ui.palette.as_deref(), Some("text"));
    assert_eq!(document_items(&app), (2, 0), "nothing was pasted into the document");
}

#[test]
fn native_copy_and_paste_use_the_system_clipboard_for_canvas_text() {
    let ctx = egui::Context::default();
    let mut app = crate::DesignApp::new(designcraft_engine::Session::new(), crate::Services::default());
    app.session.execute("file.new", &json!({})).unwrap();
    app.session.execute("frame.create", &json!({"rect": [36, 36, 200, 200], "content": "text", "text": "hello"})).unwrap();
    let story = *app.session.active().unwrap().doc.stories.keys().next().unwrap();
    app.select_tool("type");
    app.run("text.select", json!({"story": story.0, "anchor": 0, "focus": 5})).unwrap();
    for _ in 0..3 {
        native_frame(&mut app, &ctx, vec![], None);
    }
    assert!(app.session.wants_text());

    native_frame(&mut app, &ctx, vec![], Some("edit.copy"));
    let copied = native_frame(&mut app, &ctx, vec![], None);
    assert!(copied.platform_output.commands.contains(&egui::OutputCommand::CopyText("hello".into())));

    let paste = native_frame(&mut app, &ctx, vec![], Some("edit.paste"));
    let commands = &paste.viewport_output[&egui::ViewportId::ROOT].commands;
    assert!(commands.contains(&egui::ViewportCommand::RequestPaste), "{commands:?}");
    native_frame(&mut app, &ctx, vec![egui::Event::Paste("world".into())], None);
    assert_eq!(app.session.active().unwrap().doc.story(story).unwrap().text, "world");
}

#[test]
fn native_undo_and_redo_act_on_the_focused_field() {
    let ctx = egui::Context::default();
    let mut app = app_with_focused_field(&ctx, "");
    native_frame(&mut app, &ctx, vec![egui::Event::Text("abc".into())], None);
    assert_eq!(app.ui.palette.as_deref(), Some("abc"));
    native_frame(&mut app, &ctx, vec![], Some("edit.undo"));
    native_frame(&mut app, &ctx, vec![], None);
    assert_eq!(app.ui.palette.as_deref(), Some(""), "the field's typing is undone");
    assert_eq!(document_items(&app), (2, 0), "the document's last frame is still there");
    native_frame(&mut app, &ctx, vec![], Some("edit.redo"));
    native_frame(&mut app, &ctx, vec![], None);
    assert_eq!(app.ui.palette.as_deref(), Some("abc"));
    assert_eq!(document_items(&app), (2, 0));
}

#[test]
fn native_editing_items_are_enabled_for_a_focused_field() {
    let ctx = egui::Context::default();
    let mut app = app_with_focused_field(&ctx, "frame");
    // Nothing is selected in the document, so its Copy and Cut are off; the field's are on.
    assert!(!menu_enabled(&app, "edit.copy") && !menu_enabled(&app, "edit.cut"));
    for id in ["edit.selectAll", "edit.copy", "edit.cut", "edit.paste", "edit.undo", "edit.redo"] {
        assert!(native_menu_enabled(&app, &ctx, id), "{id}");
    }
    assert!(!native_menu_enabled(&app, &ctx, "edit.duplicate"), "other items follow the document");
    app.ui.palette = None;
    native_frame(&mut app, &ctx, vec![], None);
    native_frame(&mut app, &ctx, vec![], None);
    assert!(!native_menu_enabled(&app, &ctx, "edit.copy"), "without a focused field, as the document says");
}

#[test]
fn native_select_all_without_a_focused_field_selects_the_documents_items() {
    let ctx = egui::Context::default();
    let mut app = app_with_focused_field(&ctx, "frame");
    app.ui.palette = None;
    native_frame(&mut app, &ctx, vec![], None);
    native_frame(&mut app, &ctx, vec![], None);
    assert!(!ctx.text_edit_focused());
    native_frame(&mut app, &ctx, vec![], Some("edit.selectAll"));
    assert_eq!(document_items(&app), (2, 2));
}

#[test]
fn command_search_matches_labels_in_the_interface_language() {
    let mut app = DesignApp::new(designcraft_engine::Session::new(), crate::Services::default());
    app.ui.language = "de".into();
    let label = crate::i18n::tr("de", "Group");
    let items = command_search(&app, &label.to_lowercase());
    let group = items.iter().find(|i| i.id == "object.group").unwrap();
    assert_eq!(group.label, label);
    assert_eq!(group.shortcut.as_deref(), Some("Cmd+G"));
    assert!(command_search(&app, "group").iter().any(|i| i.id == "object.group"), "English labels match too");
    assert!(command_search(&app, "  ").is_empty());
    assert!(command_search(&app, "e").len() <= 14);
}

#[test]
fn quick_apply_lists_styles_then_commands() {
    let mut s = designcraft_engine::Session::new();
    s.execute("file.new", &json!({})).unwrap();
    s.execute("style.paragraph.create", &json!({"name": "Body Copy"})).unwrap();
    let items = quick_apply_items(&s, "body", false);
    assert_eq!(items[0].id, "style.paragraph.apply");
    assert_eq!(items[0].params, json!({"name": "Body Copy"}));
    assert_eq!(items[0].kind, "Paragraph Style");
    assert!(quick_apply_items(&s, "", false).iter().all(|i| i.kind.is_empty()));
    assert!(quick_apply_items(&s, "preferences", false).iter().any(|i| i.id == "app.preferences"));
}

#[test]
fn searched_ui_commands_run_without_parameters_and_keep_preferences() {
    // Without a value these reset a setting or need a place the test host always has.
    let searched: Vec<&str> = quick_commands().map(|c| c.0).collect();
    for id in ["app.flattener", "view.screenMode", "view.zoom", "window.taskBarPin"] {
        assert!(!searched.contains(&id), "{id} is searched");
    }
    // A placed PDF: a graphic with layers.
    let mut scratch = designcraft_engine::Session::new();
    scratch.execute("file.new", &json!({})).unwrap();
    let pdf = scratch.execute("file.exportPdf", &json!({})).unwrap()["base64"].clone();
    let contexts = ["nothing selected", "text insertion point", "text frame selected", "table cell", "placed PDF selected", "text selected"];
    // A two-page document, a saved workspace and non-default preferences, a host with window
    // controls, and file dialogs that are dismissed.
    let setup = |context: usize| {
        let services = crate::Services { clipboard_text: Some(Box::new(|| None)), ..Default::default() };
        let mut app = DesignApp::new(designcraft_engine::Session::new(), services);
        app.answer_dialogs(crate::app::file_dialogs::testing::answer_with(None));
        app.custom_titlebar = true;
        app.canvas_rect = Some(egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(900.0, 700.0)));
        app.run("file.new", json!({"pages": 2})).unwrap();
        app.session.prefs.recent_files.push("/nowhere/recent.dcraft".into());
        app.ui.shortcuts.insert("view.rulers", vec!["Ctrl+Alt+R".into()]);
        app.ui.hidden_menu_items.push("view.guides".into());
        app.ui.ui_scale = 1.25;
        app.ui.brightness = crate::theme::Brightness::Light;
        app.ui.task_bar_at = Some([200.0, 200.0]);
        run_ui(&mut app, "window.newWorkspace", &json!({"name": "Mine"})).unwrap().unwrap();
        match context {
            1 => {
                app.run("frame.create", json!({"rect": [72, 72, 400, 400], "content": "text", "text": "One", "caret": true})).unwrap();
            }
            2 => {
                let f = app.run("frame.create", json!({"rect": [72, 72, 400, 400], "content": "text", "text": "One"})).unwrap()["id"].clone();
                app.run("selection.set", json!({"ids": [f]})).unwrap();
            }
            3 => {
                app.run("frame.create", json!({"rect": [72, 72, 400, 400], "content": "text", "caret": true})).unwrap();
                app.run("table.insert", json!({"rows": 2, "cols": 2})).unwrap();
            }
            4 => {
                let f = app.run("frame.create", json!({"rect": [72, 72, 400, 400]})).unwrap()["id"].clone();
                app.run("file.place", json!({"base64": pdf, "name": "a.pdf", "frame": f})).unwrap();
                app.run("selection.set", json!({"ids": [f]})).unwrap();
            }
            // Convert Text to Table needs text selected.
            5 => {
                let f = app.run("frame.create", json!({"rect": [72, 72, 400, 400], "content": "text", "text": "One Two"})).unwrap();
                app.run("text.select", json!({"story": f["story"], "anchor": 0, "focus": 7})).unwrap();
            }
            _ => {}
        }
        app.ui.dialog = None;
        app
    };
    let prefs = |app: &DesignApp| {
        let u = &app.ui;
        json!({
            "shortcuts": u.shortcuts, "hiddenMenuItems": u.hidden_menu_items, "uiScale": u.ui_scale,
            "brightness": u.brightness, "workspace": u.workspace, "customWorkspaces": u.custom_workspaces,
            "docking": u.docking, "dockingHidden": u.docking_hidden, "floating": u.floating,
        })
    };
    let quick = quick_apply_items(&setup(0).session, "workspace", false);
    assert!(quick.iter().any(|i| i.id == "window.newWorkspace"));
    // Deleting needs a workspace name: the application bar's workspace menu lists them.
    assert!(!quick.iter().any(|i| i.id == "window.deleteWorkspace"));
    // Each command runs as Quick Apply runs an entry. One that needs a selection may fail
    // without it, but runs in one of the contexts; none changes a preference it wasn't asked to.
    // Opening a panel or collapsing the dock to icons changes where the panels are, as it should.
    let opens_a_panel = [
        "app.tabsPanel",
        "app.characterStylesPanel",
        "app.paragraphStylesPanel",
        "app.characterPanel",
        "app.paragraphPanel",
        "app.glyphsPanel",
        "app.tablePanel",
        "window.collapseToIcons",
    ];
    let arrangement = ["docking", "dockingHidden", "floating"];
    let mut failed = Vec::new();
    for (id, ..) in quick_commands().filter(|(id, ..)| UI_COMMANDS.iter().any(|c| c.0 == *id)) {
        let mut errors = Vec::new();
        for (i, context) in contexts.iter().enumerate() {
            let mut app = setup(i);
            let before = prefs(&app);
            match app.run(id, json!({})) {
                Err(e) => errors.push(format!("{context}: {e}")),
                Ok(_) => {
                    let after = prefs(&app);
                    let changed: Vec<&String> =
                        before.as_object().unwrap().iter().filter(|(k, v)| after.get(k.as_str()) != Some(*v)).map(|(k, _)| k).collect();
                    let changed: Vec<&String> =
                        changed.into_iter().filter(|k| !(opens_a_panel.contains(&id) && arrangement.contains(&k.as_str()))).collect();
                    if !changed.is_empty() {
                        failed.push(format!("{id} ({context}): changed {changed:?}"));
                    }
                }
            }
        }
        if errors.len() == contexts.len() {
            failed.push(format!("{id}: {errors:?}"));
        }
    }
    assert!(failed.is_empty(), "{failed:#?}");
}

#[test]
fn distribute_rows_menu_dispatches_an_undoable_edit() {
    let mut app = crate::DesignApp::new(designcraft_engine::Session::new(), crate::Services::default());
    app.run("file.new", json!({})).unwrap();
    let frame = app.run("frame.create", json!({"rect": [72, 72, 500, 700], "content": "text", "text": ""})).unwrap();
    app.run("text.select", json!({"story": frame["story"], "anchor": 0, "focus": 0})).unwrap();
    app.run("table.insert", json!({"rows": 2, "cols": 2})).unwrap();
    app.run("table.setRowHeight", json!({"row": 0, "height": 40, "mode": "exactly"})).unwrap();
    app.run("table.setRowHeight", json!({"row": 1, "height": 80, "mode": "exactly"})).unwrap();
    app.run("table.selectTable", json!({})).unwrap();
    activate(&mut app, "table.distributeRows", &Value::Null);
    assert!(app.ui.dialog.is_none());
    let info = app.run("table.get", json!({})).unwrap();
    assert_eq!(info["rows"][0]["height"], 60.0);
    assert_eq!(info["rows"][1]["height"], 60.0);
    app.run("edit.undo", json!({})).unwrap();
    assert_eq!(app.run("table.get", json!({})).unwrap()["rows"][0]["height"], 40.0);
}

#[test]
fn every_menu_entry_is_a_command() {
    let mut all = Vec::new();
    for (_, items) in menu_tree() {
        walk(&items, &mut all);
    }
    assert!(all.len() > 80, "{}", all.len());
    assert!(all.iter().any(|(label, id, _)| label == "Distribute Rows Evenly" && id == "table.distributeRows"));
    for (label, id, params) in &all {
        assert!(ui_label(id).is_some() || designcraft_engine::find_command(id).is_some(), "menu entry {label}: unknown command {id}");
        assert!(!label.is_empty() && label != id, "menu entry {id} has no label");
        if let Some(o) = params.as_object() {
            assert!(!o.is_empty(), "{id}: empty fixed params");
        }
    }
    // Nested submenus parse (Type › Insert Special Character › Symbols › …).
    let ty = menu_tree().into_iter().find(|(m, _)| *m == "Type").unwrap().1;
    let special = ty.iter().find_map(|i| match i {
        Item::Sub(n, c) if n == "Insert Special Character" => Some(c.clone()),
        _ => None,
    });
    assert!(special.is_some_and(|c| c.iter().any(|i| matches!(i, Item::Sub(n, _) if n == "Symbols"))));
    // Fixed-parameter variants keep their own labels.
    assert!(all.iter().any(|(l, id, _)| l == "Fill Frame Proportionally" && id == "fit.fillFrameProportionally"));
}

#[test]
fn context_menus_hold_commands_and_menu_bar_submenus() {
    let tree = menu_tree();
    for entries in [CONTEXT_OBJECT, CONTEXT_GRAPHIC_FRAME, CONTEXT_OBJECT_TAIL, CONTEXT_TEXT_FRAME, CONTEXT_TEXT, CONTEXT_CANVAS] {
        for path in entries.iter().filter_map(|e| e.strip_prefix("menu:")) {
            assert!(menu_bar_submenu(&tree, path).is_some(), "no menu bar submenu {path}");
        }
        let mut all = Vec::new();
        for (_, items) in context_parse(entries) {
            walk(&items, &mut all);
        }
        for (label, id, _) in &all {
            assert!(ui_label(id).is_some() || designcraft_engine::find_command(id).is_some(), "context entry {label}: unknown command {id}");
        }
    }
}

#[test]
fn the_table_menu_follows_indesigns_order() {
    // InDesign's Table menu with a text insertion point (Adobe's "How to make a table").
    let table = menu_tree().into_iter().find(|(m, _)| *m == "Table").unwrap().1;
    let labels: Vec<String> = table
        .iter()
        .map(|i| match i {
            Item::Sep => "-".to_string(),
            Item::Sub(n, _) => format!("{n} >"),
            Item::Cmd { label, .. } => label.clone(),
        })
        .collect();
    let expected = [
        "Insert Table…",
        "Convert Text to Table…",
        "Convert Table to Text…",
        "-",
        "Cell Options >",
        "-",
        "Insert >",
        "Delete >",
        "Select >",
        "-",
        "Merge Cells",
        "Unmerge Cells",
        "Split Cell Horizontally",
        "Split Cell Vertically",
        "Convert Cell to Graphic Cell…",
        "Convert Cell to Text Cell",
        "Convert Rows >",
        "-",
        "Distribute Rows Evenly",
        "Distribute Columns Evenly",
    ];
    assert_eq!(labels, expected);
    // Cmd+Alt+Shift+T opens the dialog, as InDesign's; the engine command has no shortcut of
    // its own. The Table panel is in Window › Type & Tables.
    assert_eq!(ui_label("app.insertTableDialog").and_then(|c| c.1), Some("Cmd+Alt+Shift+T"));
    assert_eq!(designcraft_engine::find_command("table.insert").unwrap().shortcut, None);
    assert_eq!(ui_label("app.tablePanel").and_then(|c| c.1), Some("Shift+F9"));
}

#[test]
fn the_type_menu_follows_indesigns_order() {
    // InDesign's Type menu, as Chris Nielsen opens it in "Numbering Page Sections" (2:09).
    let ty = menu_tree().into_iter().find(|(m, _)| *m == "Type").unwrap().1;
    let labels: Vec<String> = ty
        .iter()
        .map(|i| match i {
            Item::Sep => "-".to_string(),
            Item::Sub(n, _) => format!("{n} >"),
            Item::Cmd { label, .. } => label.clone(),
        })
        .collect();
    let expected = [
        "Size >",
        "-",
        "Character",
        "Paragraph",
        "Tabs",
        "Glyphs",
        "-",
        "Character Styles",
        "Paragraph Styles",
        "-",
        "Create Outlines",
        "Find/Replace Font…",
        "Reload Fonts",
        "Change Case >",
        "-",
        "Type on a Path >",
        "-",
        "Notes >",
        "Track Changes >",
        "-",
        "Insert Footnote",
        "Document Footnote Options…",
        "Insert Endnote",
    ];
    assert_eq!(&labels[..expected.len()], expected, "{labels:?}");
    let pos = |l: &str| labels.iter().position(|x| x == l).unwrap_or_else(|| panic!("{l} in {labels:?}"));
    assert!(pos("Hyperlinks & Cross-References >") < pos("Text Variables >"));
    assert!(pos("Text Variables >") < pos("Bulleted & Numbered Lists >"));
    assert!(pos("Bulleted & Numbered Lists >") < pos("Insert Special Character >"));
    assert!(pos("Insert Special Character >") < pos("Insert White Space >"));
    assert!(pos("Insert Break Character >") < pos("Fill with Placeholder Text"));
    assert!(pos("Fill with Placeholder Text") < pos("Show Hidden Characters"));
    // Alignment, styles and point sizes keep their shortcuts, out of the menu as in InDesign.
    for gone in ["Align Left", "Bold", "Increase Point Size", "Paragraph Rules…"] {
        assert!(!labels.iter().any(|l| l == gone), "{gone}");
    }
    let app = crate::DesignApp::new(designcraft_engine::Session::new(), crate::Services::default());
    let all = effective_shortcuts(&app);
    for (id, sc) in [
        ("type.alignLeft", "Cmd+Shift+L"),
        ("app.characterPanel", "Cmd+T"),
        ("app.paragraphPanel", "Cmd+Alt+T"),
        ("app.insertCurrentPageNumber", "Cmd+Alt+Shift+N"),
    ] {
        assert_eq!(all.iter().find(|(i, _)| i == id).map(|(_, s)| s.as_str()), Some(sc), "{id}");
    }
    // Markers › Current Page Number carries ⌥⇧⌘N.
    let markers = ty
        .iter()
        .find_map(|i| match i {
            Item::Sub(n, c) if n == "Insert Special Character" => {
                c.iter().find_map(|i| if let Item::Sub(n, c) = i { (n == "Markers").then(|| c.clone()) } else { None })
            }
            _ => None,
        })
        .unwrap();
    assert!(matches!(&markers[0], Item::Cmd { label, shortcut: Some("Cmd+Alt+Shift+N"), .. } if label == "Current Page Number"));
}

#[test]
fn type_panel_items_open_their_panels_and_the_marker_shortcut_inserts_a_page_number() {
    let mut app = crate::DesignApp::new(designcraft_engine::Session::new(), crate::Services::default());
    app.run("file.new", json!({})).unwrap();
    activate(&mut app, "app.characterPanel", &Value::Null);
    assert!(crate::panel_docking::showing(&app, "character"));
    // Paragraph Styles opens floating, grouped with Character Styles, as in InDesign.
    activate(&mut app, "app.paragraphStylesPanel", &Value::Null);
    let layout = app.ui.docking.as_ref().unwrap();
    assert!(
        layout
            .floating
            .iter()
            .flat_map(crate::panel_docking::window_groups)
            .any(|g| g.panels == ["characterStyles", "paragraphStyles"] && g.active == 1)
    );
    let story = app.run("frame.create", json!({"rect": [36, 36, 200, 100], "content": "text", "text": "Page "})).unwrap()["story"].clone();
    app.run("text.select", json!({"story": story, "anchor": 5, "focus": 5})).unwrap();
    activate(&mut app, "app.insertCurrentPageNumber", &Value::Null);
    let d = &app.session.doc().unwrap().doc;
    assert_eq!(d.story(designcraft_doc::StoryId(story.as_u64().unwrap())).unwrap().text, "Page \u{e000}");
}

#[test]
fn custom_workspaces_save_apply_and_delete() {
    let mut app = crate::DesignApp::new(designcraft_engine::Session::new(), crate::Services::default());
    app.ui.dock_tab = "swatches".into();
    app.ui.control_bar = true;
    run_ui(&mut app, "window.newWorkspace", &json!({"name": "Mine"})).unwrap().unwrap();
    run_ui(&mut app, "window.workspace", &json!({"name": "Essentials"})).unwrap().unwrap();
    app.ui.dock_tab = "properties".into();
    run_ui(&mut app, "window.workspace", &json!({"name": "Mine"})).unwrap().unwrap();
    assert_eq!((app.ui.dock_tab.as_str(), app.ui.control_bar, app.ui.workspace.as_str()), ("swatches", true, "Mine"));
    app.ui.dock_tab = "layers".into();
    run_ui(&mut app, "window.resetWorkspace", &json!({})).unwrap().unwrap();
    assert_eq!(app.ui.dock_tab, "swatches", "reset to the saved arrangement");
    run_ui(&mut app, "window.deleteWorkspace", &json!({"name": "Mine"})).unwrap().unwrap();
    assert!(app.ui.custom_workspaces.is_empty());
    assert_eq!(app.ui.workspace, "Essentials");
}

/// New Workspace › Capture (Learn Skills Daily's InDesign course, 11:40): Panel Locations and
/// Menu Customization, both on by default.
#[test]
fn new_workspace_captures_panel_locations_and_menu_customization() {
    let mut app = crate::DesignApp::new(designcraft_engine::Session::new(), crate::Services::default());
    // The dialog offers both, checked.
    run_ui(&mut app, "window.newWorkspace", &json!({})).unwrap().unwrap();
    let d = app.ui.dialog.take().unwrap();
    assert_eq!(
        (d.id.as_str(), d.fields["panelLocations"].clone(), d.fields["menuCustomization"].clone()),
        ("newWorkspace", json!(true), json!(true))
    );
    // Both captured.
    app.ui.dock_tab = "swatches".into();
    app.ui.hidden_menu_items = vec!["Type/Glyphs".into()];
    run_ui(&mut app, "window.newWorkspace", &json!({"name": "All"})).unwrap().unwrap();
    // Menus only: choosing it leaves the panels where they are.
    app.ui.hidden_menu_items = vec!["Layout/Pages".into()];
    run_ui(&mut app, "window.newWorkspace", &json!({"name": "Menus", "panelLocations": false})).unwrap().unwrap();
    // Panels only: the menus stay as they are.
    app.ui.dock_tab = "layers".into();
    run_ui(&mut app, "window.newWorkspace", &json!({"name": "Panels", "menuCustomization": false})).unwrap().unwrap();
    app.ui.dock_tab = "properties".into();
    app.ui.hidden_menu_items.clear();
    run_ui(&mut app, "window.workspace", &json!({"name": "Menus"})).unwrap().unwrap();
    assert_eq!((app.ui.dock_tab.as_str(), app.ui.hidden_menu_items.clone()), ("properties", vec!["Layout/Pages".to_string()]));
    run_ui(&mut app, "window.workspace", &json!({"name": "Panels"})).unwrap().unwrap();
    assert_eq!((app.ui.dock_tab.as_str(), app.ui.hidden_menu_items.clone()), ("layers", vec!["Layout/Pages".to_string()]));
    run_ui(&mut app, "window.workspace", &json!({"name": "All"})).unwrap().unwrap();
    assert_eq!((app.ui.dock_tab.as_str(), app.ui.hidden_menu_items.clone()), ("swatches", vec!["Type/Glyphs".to_string()]));
    // A workspace saved before the options existed restores its panels.
    let old: crate::SavedWorkspace = serde_json::from_value(json!({"name": "Old", "dockTab": "pages"})).unwrap();
    assert!(!old.keep_panels && old.hidden_menu_items.is_none());
}

#[test]
fn essentials_classic_is_the_control_panel_and_iconic_groups() {
    let mut app = crate::DesignApp::new(designcraft_engine::Session::new(), crate::Services::default());
    run_ui(&mut app, "window.workspace", &json!({"name": "Essentials Classic"})).unwrap().unwrap();
    assert!(app.ui.control_bar && !app.ui.dock_expanded);
    let groups = app.ui.icon_groups.clone().unwrap();
    assert_eq!(groups[0], ["pages", "layers", "links"]);
    assert_eq!(groups.last().unwrap(), &["gradient"]);
    assert_eq!(checked(&app, "window.workspace", &json!({"name": "Essentials Classic"})), Some(true));
    assert_eq!(checked(&app, "window.workspace", &json!({"name": "Essentials"})), Some(false));
    // Window › Pages opens Pages from its icon group, not the Properties stack.
    run_ui(&mut app, "window.panel", &json!({"panel": "pages"})).unwrap().unwrap();
    assert!(crate::panel_docking::showing(&app, "pages"));
    let layout = app.ui.docking.clone().unwrap();
    assert!(!layout.contains(&"properties".to_string()));
    assert_eq!(crate::panel_docking::tab_group(&layout, "pages"), ["pages", "layers", "links"]);
    // A workspace saved from it keeps the groups.
    run_ui(&mut app, "window.newWorkspace", &json!({"name": "Mine"})).unwrap().unwrap();
    run_ui(&mut app, "window.workspace", &json!({"name": "Essentials"})).unwrap().unwrap();
    assert_eq!((app.ui.icon_groups.as_ref(), app.ui.dock_expanded, app.ui.control_bar), (None, true, false));
    run_ui(&mut app, "window.workspace", &json!({"name": "Mine"})).unwrap().unwrap();
    assert_eq!(app.ui.icon_groups, Some(groups));
}

#[test]
fn typography_workspace_opens_its_panels_from_named_icon_groups_and_resets() {
    let mut app = crate::DesignApp::new(designcraft_engine::Session::new(), crate::Services::default());
    run_ui(&mut app, "window.workspace", &json!({"name":"Typography"})).unwrap().unwrap();
    assert!(app.ui.control_bar);
    assert!(!app.ui.dock_expanded);
    assert!(app.ui.open_panel.is_none());
    let groups = app.ui.icon_groups.clone().unwrap();
    assert_eq!(groups[0], ["pages", "layers", "swatches"]);
    assert_eq!(groups[4], ["paragraph", "paragraphStyles"]);
    assert_eq!(groups[5], ["character", "characterStyles"]);
    for panel in ["pages", "paragraphStyles", "characterStyles"] {
        run_ui(&mut app, "window.panel", &json!({"panel":panel})).unwrap().unwrap();
        assert!(crate::panel_docking::showing(&app, panel), "{panel}");
        assert!(app.ui.docking.as_ref().unwrap().floating.is_empty(), "{panel} opens from its icon group");
    }
    run_ui(&mut app, "window.resetWorkspace", &json!({})).unwrap().unwrap();
    assert_eq!(app.ui.icon_groups, Some(groups));
    assert!(app.ui.open_panel.is_none());
    assert!(!app.ui.dock_expanded);
    run_ui(&mut app, "window.workspace", &json!({"name":"Essentials"})).unwrap().unwrap();
    assert!(app.ui.icon_groups.is_none());
    assert!(app.ui.dock_expanded);
}

#[test]
fn the_window_menu_is_in_indesigns_order_and_reaches_every_panel() {
    let window = menu_tree().into_iter().find(|(m, _)| *m == "Window").unwrap().1;
    let names: Vec<&str> = window
        .iter()
        .filter_map(|i| match i {
            Item::Sub(n, _) => Some(n.as_str()),
            Item::Cmd { label, .. } => Some(label.as_str()),
            Item::Sep => None,
        })
        .collect();
    assert_eq!(&names[..4], ["Arrange", "Workspace", "Articles", "Color"]);
    let sub = |name: &str| {
        window
            .iter()
            .find_map(|i| match i {
                Item::Sub(n, c) if n == name => Some(c.clone()),
                _ => None,
            })
            .unwrap()
    };
    // Window › Workspace › Essentials Classic, Window › Color › Gradient & Swatches.
    assert!(
        sub("Workspace")
            .iter()
            .any(|i| matches!(i, Item::Cmd { id, params, .. } if id == "window.workspace" && params["name"] == "Essentials Classic"))
    );
    let color: Vec<String> = sub("Color").iter().filter_map(|i| if let Item::Cmd { label, .. } = i { Some(label.clone()) } else { None }).collect();
    assert_eq!(color, ["Color", "Gradient", "Swatches"]);
    // Every panel is in the menu.
    fn panels(items: &[Item], out: &mut Vec<String>) {
        for i in items {
            match i {
                Item::Sub(_, c) => panels(c, out),
                Item::Cmd { id, params, .. } if id == "window.panel" => out.extend(params["panel"].as_str().map(str::to_string)),
                _ => {}
            }
        }
    }
    let mut found = Vec::new();
    panels(&window, &mut found);
    for (id, _, _) in crate::dock::DOCK_TABS.iter().chain(crate::dock::ICON_PANELS) {
        assert!(found.iter().any(|p| p == id), "Window menu misses the {id} panel");
    }
}

#[test]
fn interactive_workspaces_show_their_panels() {
    let mut app = crate::DesignApp::new(designcraft_engine::Session::new(), crate::Services::default());
    app.ui.dock_expanded = false;
    run_ui(&mut app, "window.workspace", &json!({"name": "Interactive for PDF"})).unwrap().unwrap();
    assert_eq!((app.ui.dock_tab.as_str(), app.ui.dock_expanded, app.ui.open_panel.as_deref()), ("pages", true, Some("buttons")));
    run_ui(&mut app, "window.workspace", &json!({"name": "Digital Publishing"})).unwrap().unwrap();
    assert_eq!((app.ui.dock_tab.as_str(), app.ui.open_panel.as_deref()), ("pages", Some("liquid")));
    run_ui(&mut app, "window.workspace", &json!({"name": "Essentials"})).unwrap().unwrap();
    assert_eq!(app.ui.open_panel, None);
}

#[test]
fn hide_and_show_menu_items() {
    let mut app = crate::DesignApp::new(designcraft_engine::Session::new(), crate::Services::default());
    let items = all_menu_items();
    let (menu, label) = items.iter().find(|(m, _)| m == "Edit").cloned().expect("Edit items");
    let key = menu_key(&menu, &label);
    run_ui(&mut app, "window.hideMenuItem", &json!({"item": key})).unwrap().unwrap();
    assert!(app.ui.hidden_menu_items.contains(&key));
    run_ui(&mut app, "window.hideMenuItem", &json!({"item": key, "hidden": false})).unwrap().unwrap();
    assert!(app.ui.hidden_menu_items.is_empty());
    assert!(items.len() > 100, "every menu's items are listed: {}", items.len());
}

/// The Type menu as drawn: every item label.
fn drawn_type_menu(app: &mut DesignApp) -> Vec<String> {
    let ty = menu_tree().into_iter().find(|(m, _)| *m == "Type").unwrap().1;
    let ctx = egui::Context::default();
    let screen = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(400.0, 4000.0));
    let mut out = ctx.run_ui(egui::RawInput { screen_rect: Some(screen), ..Default::default() }, |ui| {
        menu_items(app, ui, &ty, "Type");
    });
    out.textures_delta.clear();
    fn collect(shape: &egui::Shape, out: &mut Vec<String>) {
        match shape {
            egui::Shape::Text(t) => out.push(t.galley.job.text.trim().to_string()),
            egui::Shape::Vec(v) => v.iter().for_each(|s| collect(s, out)),
            _ => {}
        }
    }
    let mut labels = Vec::new();
    out.shapes.iter().for_each(|s| collect(&s.shape, &mut labels));
    labels
}

/// Tate-Chu-Yoko, Ruby, Kenten and Warichu are in the Type menu of InDesign's Japanese edition
/// only: they show while Show CJK Features is on, which follows the interface language until
/// the user sets it. Hidden, they still run.
#[test]
fn cjk_menu_items_follow_show_cjk_features() {
    let mut app = DesignApp::new(designcraft_engine::Session::new(), crate::Services::default());
    app.run("file.new", json!({})).unwrap();
    let ty = menu_tree().into_iter().find(|(m, _)| *m == "Type").unwrap().1;
    let cjk: Vec<String> = ty
        .iter()
        .filter_map(|i| match i {
            Item::Cmd { label, id, .. } if CJK_ONLY.contains(&id.as_str()) => Some(label.clone()),
            _ => None,
        })
        .collect();
    assert_eq!(cjk.len(), 4, "{cjk:?}");
    let drawn = |app: &mut DesignApp| {
        let labels = drawn_type_menu(app);
        assert!(labels.iter().any(|l| l == crate::i18n::tr(&app.ui.language, "Story Direction")), "in both editions: {labels:?}");
        let shown: Vec<bool> = cjk.iter().map(|l| labels.iter().any(|d| d == crate::i18n::tr(&app.ui.language, l))).collect();
        assert!(shown.iter().all(|s| *s == shown[0]), "{shown:?}");
        shown[0]
    };
    for (lang, on) in [("", false), ("de", false), ("ja", true), ("zh", true), ("ko", true)] {
        app.ui.language = lang.into();
        assert_eq!(crate::cjk_features(&app), on, "{lang}");
        assert_eq!(drawn(&mut app), on, "{lang}");
    }
    app.run("prefs.set", json!({"cjkFeatures": false})).unwrap();
    assert!(!drawn(&mut app), "off in a Korean interface");
    app.ui.language = String::new();
    app.run("prefs.set", json!({"cjkFeatures": true})).unwrap();
    assert!(drawn(&mut app), "on in an English interface");
    assert!(quick_apply_items(&app.session, "ruby", crate::cjk_features(&app)).iter().any(|i| i.id == "app.rubyDialog"));
    app.run("prefs.set", json!({"cjkFeatures": false})).unwrap();
    assert!(!drawn(&mut app));
    assert!(!quick_apply_items(&app.session, "ruby", crate::cjk_features(&app)).iter().any(|i| i.id == "app.rubyDialog"));
    app.run("app.rubyDialog", json!({})).unwrap();
    assert_eq!(app.ui.dialog.as_ref().map(|d| d.id.as_str()), Some("ruby"), "hidden, not removed");
}

/// The CJK commands sit in a group of their own, after a separator, and hiding them leaves no
/// separator at the end of the menu.
#[test]
fn cjk_menu_items_have_their_own_group() {
    let mut app = DesignApp::new(designcraft_engine::Session::new(), crate::Services::default());
    let ty = menu_tree().into_iter().find(|(m, _)| *m == "Type").unwrap().1;
    let first = ty.iter().position(|i| matches!(i, Item::Cmd { id, .. } if CJK_ONLY.contains(&id.as_str()))).unwrap();
    assert!(matches!(ty[first - 1], Item::Sep), "separator before the CJK group");
    assert!(matches!(ty[first - 2], Item::Sub(..)), "the group comes after Story Direction");
    app.ui.language = String::new();
    app.run("prefs.set", json!({"cjkFeatures": false})).unwrap();
    assert!(!has_shown_item(&app, &ty[first..]), "nothing left after the separator");
    app.run("prefs.set", json!({"cjkFeatures": true})).unwrap();
    assert!(has_shown_item(&app, &ty[first..]));
}

/// Preferences › Type › Show CJK Features: left as it is, it keeps following the interface
/// language; changed, it is kept in prefs.json.
#[test]
fn show_cjk_features_is_a_persisted_preference() {
    let mut app = DesignApp::new(designcraft_engine::Session::new(), crate::Services::default());
    app.ui.language = "ja".into();
    run_ui(&mut app, "app.preferences", &json!({})).unwrap().unwrap();
    assert_eq!(app.ui.dialog.as_ref().unwrap().fields["cjkFeatures"], true);
    crate::dialogs::confirm(&mut app).unwrap();
    assert_eq!(app.session.prefs.cjk_features, None, "untouched: follows the language");
    run_ui(&mut app, "app.preferences", &json!({})).unwrap().unwrap();
    app.ui.dialog.as_mut().unwrap().fields.insert("cjkFeatures".into(), json!(false));
    crate::dialogs::confirm(&mut app).unwrap();
    assert_eq!(app.session.prefs.cjk_features, Some(false));
    assert!(!crate::cjk_features(&app));
    // prefs.json round trip; a file from before the setting follows the language.
    let saved = serde_json::to_vec(&app.session.prefs).unwrap();
    assert_eq!(serde_json::from_slice::<designcraft_engine::Prefs>(&saved).unwrap().cjk_features, Some(false));
    assert_eq!(serde_json::from_str::<designcraft_engine::Prefs>(r#"{"scaleStrokes": false}"#).unwrap().cjk_features, None);
    assert_eq!(app.run("prefs.set", json!({"cjkFeatures": null})).unwrap()["cjkFeatures"], Value::Null);
    assert!(crate::cjk_features(&app), "follows the Japanese interface again");
}

#[test]
fn menu_button_fits_its_anchor_and_resets_scroll_on_reopen() {
    // Workspace/panel menus may be in the middle; the status-bar zoom menu opens upwards.
    for anchor_y in [0.0, 130.0, 260.0] {
        let ctx = egui::Context::default();
        let screen = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(400.0, 300.0));
        let mut tick = 0;
        let mut frame = |events: Vec<egui::Event>| {
            tick += 1;
            let mut button = None;
            let mut first = None;
            let mut last = None;
            let mut output =
                ctx.run_ui(egui::RawInput { screen_rect: Some(screen), time: Some(tick as f64 / 60.0), events, ..Default::default() }, |ui| {
                    ui.add_space(anchor_y);
                    let response = menu_button(ui, "Long menu", |ui| {
                        for i in 0..60 {
                            let response = ui.button(format!("Item {i}"));
                            if i == 0 {
                                first = Some((response.rect, ui.clip_rect()));
                            }
                            if i == 59 {
                                last = Some((response.rect, ui.clip_rect()));
                            }
                        }
                    });
                    button = Some(response.response);
                });
            output.textures_delta.clear();
            (button.expect("menu button"), first, last)
        };
        let click =
            |pos, pressed| egui::Event::PointerButton { pos, button: egui::PointerButton::Primary, pressed, modifiers: egui::Modifiers::NONE };
        frame(vec![]);
        let (button, _, _) = frame(vec![]);
        let anchor = button.rect;
        for pressed in [true, false] {
            frame(vec![egui::Event::PointerMoved(anchor.center()), click(anchor.center(), pressed)]);
        }
        for _ in 0..3 {
            frame(vec![]);
        }
        let popup = ctx.memory(|memory| memory.area_rect(egui::Popup::default_response_id(&button))).expect("open menu");
        assert!(screen.contains_rect(popup), "menu at {anchor_y} fits in the viewport: {popup:?}");
        assert!(
            popup.top() >= anchor.bottom() - 1.0 || popup.bottom() <= anchor.top() + 1.0,
            "menu at {anchor_y} stays on one side of its button: {popup:?}, {anchor:?}"
        );
        let (_, first, last) = frame(vec![]);
        let (first, clip) = first.expect("first row");
        assert!(clip.contains_rect(first), "newly opened menu starts at the first row");
        assert!(!clip.intersects(last.expect("last row").0), "last row starts below the scroll area");

        frame(vec![
            egui::Event::PointerMoved(popup.center()),
            egui::Event::MouseWheel {
                unit: egui::MouseWheelUnit::Point,
                delta: egui::vec2(0.0, -5000.0),
                phase: egui::TouchPhase::Move,
                modifiers: egui::Modifiers::NONE,
                source: egui::MouseWheelSource::Unknown,
            },
        ]);
        for _ in 0..60 {
            frame(vec![]);
        }
        let (_, first, last) = frame(vec![]);
        let (last, clip) = last.expect("scrolled last row");
        assert!(popup.contains_rect(last) && clip.contains_rect(last), "last row is reachable: {last:?}, {popup:?}, {clip:?}");
        assert!(!clip.intersects(first.expect("scrolled first row").0), "menu really scrolled away from its start");

        let outside = egui::pos2(screen.right() - 10.0, screen.center().y);
        for pressed in [true, false] {
            frame(vec![egui::Event::PointerMoved(outside), click(outside, pressed)]);
        }
        assert!(!egui::Popup::is_any_open(&ctx), "outside click closes the menu");
        assert!(frame(vec![]).1.is_none(), "closed menu is no longer drawn");
        for pressed in [true, false] {
            frame(vec![egui::Event::PointerMoved(anchor.center()), click(anchor.center(), pressed)]);
        }
        for _ in 0..3 {
            frame(vec![]);
        }
        let (_, first, last) = frame(vec![]);
        let (first, clip) = first.expect("reopened first row");
        assert!(popup.contains_rect(first) && clip.contains_rect(first), "reopening resets to the first row: {first:?}, {clip:?}");
        assert!(!clip.intersects(last.expect("reopened last row").0), "reopening discards the old scroll offset");
    }
}

#[test]
fn tiny_menu_viewport_does_not_force_the_default_scroll_height() {
    let ctx = egui::Context::default();
    let screen = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(400.0, 100.0));
    let mut tick = 0;
    let mut frame = |events: Vec<egui::Event>| {
        tick += 1;
        let mut button = None;
        let mut output =
            ctx.run_ui(egui::RawInput { screen_rect: Some(screen), time: Some(tick as f64 / 60.0), events, ..Default::default() }, |ui| {
                ui.add_space(35.0);
                let response = menu_button(ui, "Long menu", |ui| {
                    for i in 0..20 {
                        let _ = ui.button(format!("Item {i}"));
                    }
                });
                button = Some(response.response);
            });
        output.textures_delta.clear();
        button.expect("menu button")
    };
    frame(vec![]);
    let button = frame(vec![]);
    let pos = button.rect.center();
    for pressed in [true, false] {
        frame(vec![
            egui::Event::PointerMoved(pos),
            egui::Event::PointerButton { pos, button: egui::PointerButton::Primary, pressed, modifiers: egui::Modifiers::NONE },
        ]);
    }
    for _ in 0..3 {
        frame(vec![]);
    }
    let popup = ctx.memory(|memory| memory.area_rect(egui::Popup::default_response_id(&button))).expect("open menu");
    let space = (screen.bottom() - button.rect.bottom()).max(button.rect.top() - screen.top());
    assert!(space < 64.0, "fixture has less space than egui's default minimum scroll height");
    // A complete command row may not fit, but the scroll area's minimum must not make
    // the popup exceed its height budget or cover the button that dismisses it.
    assert!(popup.height() <= space, "tiny menu respects its available height: {popup:?}, {space}");
    assert!(screen.contains_rect(popup), "tiny menu remains inside the viewport: {popup:?}");
    assert!(
        popup.top() >= button.rect.bottom() - 1.0 || popup.bottom() <= button.rect.top() + 1.0,
        "tiny menu stays beside its anchor: {popup:?}, {:?}",
        button.rect
    );
}

#[test]
fn clipped_submenu_does_not_open_when_hovering_outside_menu() {
    let mut app = DesignApp::new(designcraft_engine::Session::new(), crate::Services::default());
    let ctx = egui::Context::default();
    let screen = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(1000.0, 900.0));
    let entries = vec![Item::Sub("Hidden submenu".into(), vec![cmd_item("app.preferences", Value::Null)])];
    let mut tick = 0;
    let mut frame = |spacing: f32, events: Vec<egui::Event>| {
        tick += 1;
        let mut button_rect = egui::Rect::NOTHING;
        let mut hidden_submenu = None;
        let mut output =
            ctx.run_ui(egui::RawInput { screen_rect: Some(screen), time: Some(tick as f64 / 60.0), events, ..Default::default() }, |ui| {
                egui::MenuBar::new().ui(ui, |ui| {
                    let response = ui.menu_button("Menu", |ui| {
                        egui::ScrollArea::vertical().max_height(400.0).show(ui, |ui| {
                            ui.add_space(spacing);
                            let button_id = ui.next_auto_id();
                            menu_items(&mut app, ui, &entries, "Test");
                            let response = ui.ctx().read_response(button_id).expect("submenu button response");
                            let open = egui::containers::menu::MenuState::from_ui(ui, |state, _| state.open_item.is_some());
                            hidden_submenu = Some((response.rect, ui.clip_rect(), open));
                        });
                    });
                    button_rect = response.response.rect;
                });
            });
        output.textures_delta.clear();
        (button_rect, hidden_submenu)
    };
    frame(500.0, vec![]);
    let button = frame(500.0, vec![]).0.center();
    for pressed in [true, false] {
        frame(
            500.0,
            vec![
                egui::Event::PointerMoved(button),
                egui::Event::PointerButton { pos: button, button: egui::PointerButton::Primary, pressed, modifiers: egui::Modifiers::NONE },
            ],
        );
    }
    for _ in 0..3 {
        frame(500.0, vec![]);
    }
    let (_, Some((hidden, clip, open))) = frame(500.0, vec![]) else { panic!("test menu is open") };
    assert!(!open, "submenu starts closed");
    assert!(screen.contains_rect(hidden), "hidden row remains inside the screen: {hidden:?}");
    assert!(!clip.intersects(hidden), "submenu must be fully clipped: {hidden:?}, {clip:?}");
    let (_, Some((_, _, open))) = frame(500.0, vec![egui::Event::PointerMoved(hidden.center())]) else { panic!("test menu remains open") };
    assert!(!open, "hovering the clipped row's position outside the menu must not open its submenu");

    let partial_spacing = 500.0 - (hidden.top() - clip.bottom()) - 6.0;
    let (_, Some((partial, clip, open))) = frame(partial_spacing, vec![]) else { panic!("test menu remains open") };
    assert!(!open);
    assert!(partial.top() < clip.bottom() && partial.bottom() > clip.bottom(), "row must be partially clipped: {partial:?}, {clip:?}");
    let outside = egui::pos2(partial.center().x, clip.bottom() + 1.0);
    let (_, Some((_, _, open))) = frame(partial_spacing, vec![egui::Event::PointerMoved(outside)]) else { panic!("test menu remains open") };
    assert!(!open, "hovering the clipped portion of a partially visible row must not open its submenu");
    let visible = partial.intersect(clip).center();
    let (_, Some((_, _, open))) = frame(partial_spacing, vec![egui::Event::PointerMoved(visible)]) else { panic!("test menu remains open") };
    assert!(open, "hovering the visible portion still opens the submenu");
    let (_, Some((_, _, open))) = frame(500.0, vec![]) else { panic!("test menu remains open") };
    assert!(!open, "an open submenu closes when its row becomes fully clipped");
}

#[test]
fn native_labels_keep_their_ampersands() {
    assert_eq!(native_label("Numbering & Section Options…"), "Numbering && Section Options…");
    assert_eq!(native_label("A&&B"), "A&&&&B");
    assert_eq!(native_label("Pages"), "Pages");
}

#[test]
fn the_mac_window_menu_minimizes_with_cmd_m() {
    let window = native_menu_tree().into_iter().find(|(t, _)| *t == "Window").map(|(_, e)| e).unwrap_or_default();
    assert!(matches!(window.first(), Some(Item::Cmd { id, shortcut: Some("Cmd+M"), .. }) if id == "window.minimize"), "{:?}", window.first());
    assert!(matches!(window.get(1), Some(Item::Sep)));
    let mut app = crate::DesignApp::new(designcraft_engine::Session::new(), crate::Services::default());
    assert!(run_ui(&mut app, "window.minimize", &Value::Null).is_some_and(|r| r.is_err()), "no host window controls");
    app.native_menu = true;
    assert!(menu_enabled(&app, "window.minimize") && !menu_enabled(&app, "window.maximize"));
    run_ui(&mut app, "window.minimize", &Value::Null).unwrap().unwrap();
    assert_eq!(app.pending_window_commands, [egui::ViewportCommand::Minimized(true)]);
}

#[test]
fn mac_settings_follow_about_in_the_app_menu_and_leave_edit() {
    let pos = |command: &str| MAC_APP_MENU.iter().position(|m| matches!(m, AppMenuItem::Cmd { command: c, .. } if *c == command));
    let (Some(about), Some(settings)) = (pos("help.about"), pos("app.preferences")) else { panic!("About and Settings are in the app menu") };
    assert!(about < settings);
    assert_eq!(MAC_APP_MENU.get(settings - 1), Some(&AppMenuItem::Sep));
    // Language follows Settings, as in the suite's other apps, titled in the interface language.
    assert_eq!(MAC_APP_MENU.get(settings + 1), Some(&AppMenuItem::Language));
    assert_eq!(MAC_APP_MENU.get(settings + 2), Some(&AppMenuItem::Sep));
    let label = |command: &str| {
        MAC_APP_MENU.iter().find_map(|m| match m {
            AppMenuItem::Cmd { label, command: c, .. } if *c == command => Some(*label),
            _ => None,
        })
    };
    assert_eq!(label("app.preferences"), Some("Settings…"));
    let ids = MAC_APP_MENU.iter().filter_map(|m| if let AppMenuItem::Cmd { id, .. } = m { Some(*id) } else { None }).collect::<Vec<_>>();
    assert!(ids.iter().enumerate().all(|(i, id)| !ids[..i].contains(id)), "native ids are unique: {ids:?}");

    let edit = |tree: Vec<(&'static str, Vec<Item>)>| {
        let mut out = Vec::new();
        walk(&tree.into_iter().find(|(t, _)| *t == "Edit").map(|(_, e)| e).unwrap_or_default(), &mut out);
        out.into_iter().map(|(_, id, _)| id).collect::<Vec<_>>()
    };
    assert!(edit(menu_tree()).iter().any(|id| id == "app.preferences"), "the in-window Edit menu keeps Preferences");
    let native = native_menu_tree();
    let mut all = Vec::new();
    for (_, entries) in &native {
        walk(entries, &mut all);
    }
    assert!(!all.iter().any(|(_, id, _)| id == "app.preferences"), "the macOS menu bar has Settings only in the app menu");
    assert!(edit(native_menu_tree()).iter().any(|id| id == "app.keyboardShortcuts"));
    for (title, entries) in &native {
        assert!(!matches!(entries.first(), Some(Item::Sep)) && !matches!(entries.last(), Some(Item::Sep)), "{title}: separator at an end");
        assert!(entries.windows(2).all(|w| !matches!(w, [Item::Sep, Item::Sep])), "{title}: doubled separator");
    }
}

/// Web save: a download that couldn't start is an error and leaves the document unsaved; one
/// that started marks it saved.
#[test]
fn web_save_marks_the_document_saved_only_after_the_download_starts() {
    let dir = tempfile::tempdir().unwrap();
    // The title names a temporary path: on native, file.saveAs writes where the web build doesn't.
    let title = dir.path().join("doc").to_string_lossy().to_string();
    let fails = std::rc::Rc::new(std::cell::Cell::new(true));
    let f = fails.clone();
    let services = crate::Services {
        download: Some(Box::new(move |_: &str, _: &[u8]| if f.get() { Err("blocked".into()) } else { Ok(()) })),
        ..Default::default()
    };
    let mut app = DesignApp::new(designcraft_engine::Session::new(), services);
    app.run("file.new", json!({"title": title})).unwrap();
    app.run("frame.create", json!({"rect": [36, 36, 300, 200]})).unwrap();
    assert!(app.session.active().unwrap().is_dirty());

    let e = app.run("app.save", json!({})).unwrap_err();
    assert!(e.contains("blocked"), "{e}");
    assert!(app.session.active().unwrap().is_dirty(), "a failed download leaves the document unsaved");

    fails.set(false);
    app.run("app.save", json!({})).unwrap();
    assert!(!app.session.active().unwrap().is_dirty());
}

fn start_screen_texts(app: &mut crate::DesignApp) -> Vec<String> {
    let ctx = egui::Context::default();
    crate::theme::install_fonts(&ctx, "");
    let mut shapes = Vec::new();
    for _ in 0..3 {
        let input =
            egui::RawInput { screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(1400.0, 900.0))), ..Default::default() };
        let mut out = ctx.run_ui(input, |ui| {
            app.logic(&ui.ctx().clone());
            app.ui(ui);
        });
        out.textures_delta.clear();
        shapes = out.shapes;
    }
    shapes
        .iter()
        .filter_map(|s| match &s.shape {
            egui::Shape::Text(t) => Some(t.galley.text().to_string()),
            _ => None,
        })
        .collect()
}

/// File › Open Recent: the files first (`app.openRecent {index}`), then Clear Recent Files.
#[test]
fn open_recent_lists_the_files_then_clear() {
    let tree = menu_tree();
    let file = &tree.iter().find(|(m, _)| *m == "File").unwrap().1;
    let Some(Item::Sub(name, children)) = file.get(2) else { panic!("Open Recent after Open…: {:?}", file.get(2)) };
    assert_eq!(format!("File/{name}"), OPEN_RECENT);
    assert!(matches!(children.as_slice(), [Item::Cmd { id, .. }] if id == "file.clearRecent"));
    let mut app = crate::DesignApp::new(designcraft_engine::Session::new(), crate::Services::default());
    assert!(recent_items(&app).is_empty());
    assert!(!menu_enabled(&app, "file.clearRecent"));
    app.session.prefs.note_recent_file("/work/a.designcraft");
    app.session.prefs.note_recent_file("/work/b.idml");
    let items = recent_items(&app);
    let labels: Vec<_> = items
        .iter()
        .map(|i| match i {
            Item::Cmd { label, id, params, .. } => format!("{label} {id} {params}"),
            Item::Sep => "-".into(),
            Item::Sub(..) => "sub".into(),
        })
        .collect();
    assert_eq!(labels, ["b.idml app.openRecent {\"index\":0}", "a.designcraft app.openRecent {\"index\":1}", "-"]);
    assert!(menu_enabled(&app, "file.clearRecent"));
    assert!(menu_enabled(&app, "app.openRecent"));
}

/// A recent file that is gone says so in an alert (and the start screen still lists it).
#[test]
fn open_recent_of_a_missing_file_alerts_and_the_start_screen_lists_recent_files() {
    let mut app = crate::DesignApp::new(designcraft_engine::Session::new(), crate::Services::default());
    assert!(!start_screen_texts(&mut app).iter().any(|t| t == "Recent files"), "no section without recent files");
    app.session.prefs.note_recent_file("/nonexistent/dc-recent/gone.designcraft");
    assert!(app.run("app.openRecent", json!({"index": 0})).is_err());
    let d = app.ui.dialog.take().expect("an alert");
    assert_eq!(d.fields["title"], "Can't Open the File");
    assert_eq!(d.fields["file"], "gone.designcraft");
    assert!(d.fields["message"].as_str().unwrap().contains("can't be found"));
    assert!(app.run("app.openRecent", json!({"index": 9})).is_err(), "out of range: an error, not a panic");
    app.ui.dialog = None;
    let texts = start_screen_texts(&mut app);
    assert!(texts.iter().any(|t| t == "Recent files"), "{texts:?}");
    assert!(texts.iter().any(|t| t == "gone"), "the card names it without `.designcraft`: {texts:?}");
}

/// On a short window (1366 x 768 laptops) the start screen is taller than the window; the part
/// below the fold has to be reachable by scrolling instead of being cut off.
#[test]
fn start_screen_scrolls_when_the_window_is_too_short() {
    let mut app = crate::DesignApp::new(designcraft_engine::Session::new(), crate::Services::default());
    let ctx = egui::Context::default();
    crate::theme::install_fonts(&ctx, "");
    let size = egui::vec2(1366.0, 300.0);
    let mut tile_y = |events: Vec<egui::Event>| {
        let input = egui::RawInput { screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, size)), events, ..Default::default() };
        let mut out = ctx.run_ui(input, |ui| {
            app.logic(&ui.ctx().clone());
            app.ui(ui);
        });
        out.textures_delta.clear();
        out.shapes.iter().find_map(|s| match &s.shape {
            // The preset tiles paint their names even when scrolled out of view (labels skip that).
            egui::Shape::Text(t) if t.galley.text() == "Tabloid" => Some(t.pos.y),
            _ => None,
        })
    };
    let at = egui::pos2(600.0, 150.0);
    let mut before = None;
    for _ in 0..3 {
        before = tile_y(vec![egui::Event::PointerMoved(at)]);
    }
    let before = before.expect("the Tabloid tile is laid out");
    let wheel = egui::Event::MouseWheel {
        unit: egui::MouseWheelUnit::Point,
        delta: egui::vec2(0.0, -400.0),
        phase: egui::TouchPhase::Move,
        modifiers: egui::Modifiers::NONE,
        source: egui::MouseWheelSource::Unknown,
    };
    let mut after = tile_y(vec![egui::Event::PointerMoved(at), wheel]);
    for _ in 0..30 {
        after = tile_y(Vec::new());
    }
    let after = after.expect("the Tabloid tile is laid out");
    assert!(after < before - 50.0, "the start screen should scroll: {before} -> {after}");
}

// Regression test for #329: on the start screen (no document) the Help menu's links were greyed out.
#[test]
fn help_links_are_enabled_without_a_document() {
    let app = crate::DesignApp::new(designcraft_engine::Session::new(), crate::Services::default());
    assert!(app.session.active().is_none());
    let ids: Vec<&str> = UI_COMMANDS.iter().map(|c| c.0).filter(|id| id.starts_with("help.")).collect();
    assert!(ids.contains(&"help.discord") && ids.contains(&"help.github"), "{ids:?}");
    for id in ids {
        assert!(menu_enabled(&app, id), "{id} must work from the start screen");
    }
    assert!(!menu_enabled(&app, "view.rulers"), "other UI commands still need a document");
}

// Regression test for #248: the start screen offered no A6 sheet (it listed only the first ten presets).
#[test]
fn start_screen_offers_every_print_preset_including_a6() {
    let mut app = crate::DesignApp::new(designcraft_engine::Session::new(), crate::Services::default());
    let texts = start_screen_texts(&mut app);
    for p in designcraft_doc::build::PRESETS.iter().filter(|p| p.intent == designcraft_doc::Intent::Print) {
        assert!(texts.iter().any(|t| t == p.name), "start screen lacks the {} card: {texts:?}", p.name);
    }
    assert!(texts.iter().any(|t| t == "A6"), "{texts:?}");
    let a6 = designcraft_doc::build::PRESETS.iter().find(|p| p.name == "A6").unwrap();
    assert!((a6.width - 105.0 * 72.0 / 25.4).abs() < 0.01 && (a6.height - 148.0 * 72.0 / 25.4).abs() < 0.01, "{a6:?}");
    let page = crate::panels::properties::PAGE_PRESETS.iter().find(|p| p.0 == "A6").expect("Properties page-size list has A6");
    assert!((page.1 - a6.width).abs() < 0.01 && (page.2 - a6.height).abs() < 0.01, "{page:?} vs {a6:?}");
}

// Regression test for #249: a menu row only reacted over its text, not across the whole row.
#[test]
fn menu_rows_are_selectable_across_the_full_width() {
    let mut app = DesignApp::new(designcraft_engine::Session::new(), crate::Services::default());
    let ctx = egui::Context::default();
    let screen = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(600.0, 400.0));
    let entries = vec![cmd_item("app.preferences", Value::Null)];
    let mut tick = 0;
    let mut frame = |app: &mut DesignApp, events: Vec<egui::Event>| {
        tick += 1;
        let mut row = egui::Rect::NOTHING;
        let mut output =
            ctx.run_ui(egui::RawInput { screen_rect: Some(screen), time: Some(tick as f64 / 60.0), events, ..Default::default() }, |ui| {
                egui::Area::new(egui::Id::unique("menu_rows")).fixed_pos(egui::pos2(50.0, 50.0)).show(ui.ctx(), |ui| {
                    let before = ui.cursor().min;
                    menu_column(app, ui, &[("Test", entries.clone())]);
                    row = egui::Rect::from_min_max(before, ui.min_rect().max);
                });
            });
        output.textures_delta.clear();
        row
    };
    frame(&mut app, vec![]);
    let row = frame(&mut app, vec![]);
    assert!(row.width() >= 239.0, "the column is at least 240 wide: {row:?}");
    let far = egui::pos2(row.right() - 6.0, row.top() + 8.0);
    for pressed in [true, false] {
        frame(
            &mut app,
            vec![
                egui::Event::PointerMoved(far),
                egui::Event::PointerButton { pos: far, button: egui::PointerButton::Primary, pressed, modifiers: egui::Modifiers::NONE },
            ],
        );
    }
    assert!(app.ui.dialog.is_some(), "clicking the empty end of the Preferences… row must choose it");
}

/// Quick Apply and the menu search list no unavailable docking command and no label twice
/// among the UI commands.
#[test]
fn searched_ui_commands_are_available_and_named_once() {
    let ui_ids: Vec<&str> = UI_COMMANDS.iter().map(|c| c.0).collect();
    let searched: Vec<(&str, &str)> = quick_commands().filter(|c| ui_ids.contains(&c.0)).map(|c| (c.0, c.1)).collect();
    assert!(!searched.iter().any(|c| matches!(c.0, "window.panel.window" | "window.workspace.lock" | "window.workspace")), "{searched:?}");
    assert!(searched.iter().any(|c| c.0 == "window.collapseToIcons"), "Collapse to Icons is searched");
    // Quick Apply runs an entry as `app.run(id, {})`, so each docking command it lists must
    // work without params.
    let docking = |id: &str| {
        id.starts_with("window.panel")
            || id.starts_with("window.workspace")
            || id.starts_with("ui.dock")
            || matches!(id, "window.floatPanel" | "window.dockPanel" | "window.collapseToIcons" | "window.resetWorkspace")
    };
    let failing: Vec<(&str, String)> = searched
        .iter()
        .filter(|c| docking(c.0))
        .filter_map(|c| {
            let mut app = crate::DesignApp::new(designcraft_engine::Session::new(), crate::Services::default());
            app.run(c.0, json!({})).err().map(|e| (c.0, e))
        })
        .collect();
    assert!(failing.is_empty(), "searched docking commands that fail with {{}}: {failing:?}");
    let mut labels: Vec<&str> = searched.iter().map(|c| c.1).collect();
    labels.sort_unstable();
    let before = labels.len();
    labels.dedup();
    assert_eq!(labels.len(), before, "a label twice: {searched:?}");
}

/// Keys a floating panel's own OS window left over run the main window's tool shortcuts,
/// Escape and the active tool's keys.
#[test]
fn forwarded_keys_run_shortcuts() {
    use egui::{Event, Key, Modifiers};
    fn press(key: Key) -> Vec<Event> {
        [true, false].map(|pressed| Event::Key { key, physical_key: Some(key), pressed, repeat: false, modifiers: Modifiers::NONE }).to_vec()
    }
    let ctx = egui::Context::default();
    let mut app = crate::DesignApp::new(designcraft_engine::Session::new(), crate::Services::default());
    app.session.execute("file.new", &json!({})).unwrap();
    let frame = app.session.execute("frame.create", &json!({"rect": [36, 36, 200, 200]})).unwrap();
    app.session.execute("selection.set", &json!({"ids": [frame["id"]]})).unwrap();
    forwarded_shortcuts(&mut app, &ctx, press(Key::P), Modifiers::NONE);
    assert_ne!(app.session.tool_id(), "selection", "P picks a tool");
    forwarded_shortcuts(&mut app, &ctx, press(Key::Escape), Modifiers::NONE);
    assert_eq!(app.session.tool_id(), "selection", "Escape goes back to the Selection tool");
    app.session.execute("selection.set", &json!({"ids": [frame["id"]]})).unwrap();
    forwarded_shortcuts(&mut app, &ctx, press(Key::Delete), Modifiers::NONE);
    assert!(app.session.active().unwrap().doc.item(designcraft_doc::ItemId(frame["id"].as_u64().unwrap())).is_none(), "Delete clears the selection");
}

#[test]
fn file_menu_ends_with_quit_but_the_macos_menu_bar_leaves_it_to_the_app_menu() {
    let file = |tree: Vec<(&'static str, Vec<Item>)>| {
        let mut out = Vec::new();
        walk(&tree.into_iter().find(|(t, _)| *t == "File").map(|(_, e)| e).unwrap_or_default(), &mut out);
        out.into_iter().map(|(_, id, _)| id).collect::<Vec<_>>()
    };
    assert_eq!(file(menu_tree()).last().map(String::as_str), Some("app.quit"), "File ends with the quit item");
    assert!(!file(native_menu_tree()).iter().any(|id| id == "app.quit"), "macOS has Quit only in the app menu");
    let native = native_menu_tree();
    let file_items = native.iter().find(|(t, _)| *t == "File").map(|(_, e)| e);
    assert!(!matches!(file_items.and_then(|e| e.last()), Some(Item::Sep)), "no separator left at the end of File");
}
