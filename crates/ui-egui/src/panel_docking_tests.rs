use super::*;
use egui::{Event, Modifiers, PointerButton, Pos2, vec2};
use egui_kittest::{Harness, kittest::Queryable};

fn app() -> DesignApp {
    DesignApp::new(designcraft_engine::Session::new(), Default::default())
}

#[test]
fn panel_commands_group_split_reorder_float_and_restore_without_duplicate_ownership() {
    let mut app = app();
    app.run("window.panel.move", json!({"panel":"swatches", "anchor":"layers", "zone":"top"})).unwrap();
    app.run("window.panel.move", json!({"panel":"properties", "anchor":"swatches", "before":"swatches"})).unwrap();
    let before_float = app.ui.docking.clone().unwrap();
    app.run("window.panel.float", json!({"panel":"properties", "x":50,"y":70,"width":330,"height":410})).unwrap();
    assert!(app.ui.docking.as_ref().unwrap().floating.iter().any(|g| g.panels == ["properties"]));
    app.run("window.panel.dock", json!({"panel":"properties"})).unwrap();
    assert_eq!(app.ui.docking.as_ref().unwrap(), &before_float);
    app.run("window.panel.close", json!({"panel":"properties"})).unwrap();
    assert!(!app.ui.docking.as_ref().unwrap().contains(&"properties".into()));
    app.run("window.panel.activate", json!({"panel":"properties"})).unwrap();
    assert_eq!(app.ui.docking.as_ref().unwrap(), &before_float);
    let layout = app.ui.docking.as_ref().unwrap();
    assert!(valid(layout));
    let unique: std::collections::HashSet<_> = layout.panels().into_iter().collect();
    assert_eq!(unique.len(), layout.panels().len());
    let saved = serde_json::to_value(&app.ui).unwrap();
    let restored: crate::UiState = serde_json::from_value(saved).unwrap();
    assert_eq!(restored.docking, app.ui.docking);
    assert_eq!(restored.docking_hidden, app.ui.docking_hidden);
}

#[test]
fn invalid_panel_requests_are_atomic_and_do_not_enter_a_custom_workspace() {
    let mut app = app();
    for (id, params) in [
        ("window.panel.move", json!({"panel":"layers","anchor":"unknown"})),
        ("window.panel.move", json!({"panel":"layers","anchor":"properties","before":"unknown"})),
        ("window.panel.move", json!({"panel":"layers","anchor":"properties","zone":42})),
        ("window.panel.float", json!({"panel":"layers","width":-10})),
        ("window.panel.float", json!({"panel":"unknown"})),
        ("window.panel.float", json!({"panel":"layers","x":"invalid"})),
    ] {
        let old = app.ui.docking.clone();
        let hidden = app.ui.docking_hidden.clone();
        assert!(app.run(id, params).is_err(), "{id}");
        assert_eq!(app.ui.docking, old);
        assert_eq!(app.ui.docking_hidden, hidden);
    }
}

fn harness(app: DesignApp, size: egui::Vec2) -> Harness<'static, DesignApp> {
    let mut ready = false;
    let mut harness = Harness::builder().with_size(size).with_step_dt(1.0 / 60.0).build_ui_state(
        move |ui, app: &mut DesignApp| {
            if !ready {
                crate::theme::install_fonts(ui.ctx(), &app.ui.language);
                crate::theme::apply(ui.ctx(), &crate::theme::Tokens::for_brightness(crate::theme::Brightness::MediumDark));
                ready = true;
                return;
            }
            crate::dock::show(app, ui);
            egui::CentralPanel::default().show(ui, |_| {});
        },
        app,
    );
    harness.run_steps(4);
    harness
}

fn drag(h: &mut Harness<'static, DesignApp>, from: Pos2, to: Pos2) {
    h.event(Event::PointerMoved(from));
    h.run_steps(1);
    h.event(Event::PointerButton { pos: from, button: PointerButton::Primary, pressed: true, modifiers: Modifiers::NONE });
    h.run_steps(1);
    for step in 1..=8 {
        h.event(Event::PointerMoved(from + (to - from) * (step as f32 / 8.0)));
        h.run_steps(1);
    }
    h.event(Event::PointerButton { pos: to, button: PointerButton::Primary, pressed: false, modifiers: Modifiers::NONE });
    h.run_steps(3);
}

#[test]
fn ordinary_panel_tab_drag_enters_shared_docking_and_can_redock_into_another_tab_group() {
    let mut h = harness(app(), vec2(1280.0, 800.0));
    let source = h.query_all_by_label("Layers").find(|node| node.rect().height() <= 45.0).unwrap().rect().center();
    drag(&mut h, source, egui::pos2(160.0, 140.0));
    let layout = h.state().ui.docking.as_ref().expect("ordinary tab drag starts shared docking");
    assert!(layout.floating.iter().any(|g| g.panels.contains(&"layers".into())), "{layout:?}");
    let source = h.ctx.read_response(egui::Id::new("designcraft-panel-docking").with(("tab", &"layers".to_string()))).unwrap().rect.center();
    let destination = h.ctx.read_response(egui::Id::new("designcraft-panel-docking").with(("tab", &"properties".to_string()))).unwrap().rect.center();
    drag(&mut h, source, destination);
    let layout = h.state().ui.docking.as_ref().unwrap();
    assert!(!layout.floating.iter().any(|g| g.panels.contains(&"layers".into())));
    assert!(valid(layout));
}

#[test]
fn saved_workspace_restores_the_custom_tree_and_hidden_panel_placement() {
    let mut app = app();

    app.run("window.panel.move", json!({"panel":"swatches","anchor":"layers","zone":"top"})).unwrap();
    app.run("window.panel.float", json!({"panel":"properties","x":50,"y":80})).unwrap();
    app.run("window.panel.close", json!({"panel":"swatches"})).unwrap();
    let layout = app.ui.docking.clone();
    let hidden = app.ui.docking_hidden.clone();
    app.run("window.newWorkspace", json!({"name":"Custom docking"})).unwrap();
    app.run("window.workspace", json!({"name":"Essentials"})).unwrap();
    assert!(app.ui.docking.is_none());
    app.run("window.workspace", json!({"name":"Custom docking"})).unwrap();
    assert_eq!(app.ui.docking, layout);
    assert_eq!(app.ui.docking_hidden, hidden);
}

/// Explicit offscreen evidence; this creates no native windows.
#[test]
fn capture_custom_panel_docking_visual_fixtures() {
    let Some(directory) = std::env::var_os("CRAFT_UI_DOCKING_FIXTURES").map(std::path::PathBuf::from) else { return };
    std::fs::create_dir_all(&directory).unwrap();
    for theme in crate::theme::Brightness::ALL {
        for width in [800.0, 1280.0] {
            for scale in [1.0, 1.5, 2.0] {
                let mut app = app();
                app.run("file.new", json!({})).unwrap();
                app.run("frame.create", json!({"rect":[40,40,280,180],"content":"text","text":"Synthetic layout — مرحبا"})).unwrap();
                let language = if width == 800.0 && scale == 1.0 { "ar" } else { "en" };
                app.run("app.language", json!({"lang":if language == "en" { "" } else { language }})).unwrap();
                app.run("window.panel.move", json!({"panel":"swatches", "anchor":"layers", "zone":"top"})).unwrap();
                app.run("window.panel.float", json!({"panel":"properties","x":30,"y":90,"width":300,"height":440})).unwrap();
                let mut ready = false;
                let mut harness = Harness::builder().with_size(vec2(width, 800.0)).with_pixels_per_point(scale).wgpu().build_ui_state(
                    move |ui, app: &mut DesignApp| {
                        if !ready {
                            crate::theme::install_fonts(ui.ctx(), &app.ui.language);
                            crate::theme::apply(ui.ctx(), &crate::theme::Tokens::for_brightness(theme));
                            ready = true;
                            return;
                        }
                        crate::dock::show(app, ui);
                        egui::CentralPanel::default().show(ui, |ui| {
                            ui.label("Custom panel workspace");
                            if app.ui.language == "ar" {
                                ui.label("Shaped action references:");
                                for action in ["Float Panel", "Group with", "Close Panel"] {
                                    crate::rtl::label(ui, crate::i18n::tr(&app.ui.language, action));
                                }
                            }
                        });
                    },
                    app,
                );
                harness.input_mut().max_texture_side = Some(8192);
                harness.run_steps(5);
                assert!(valid(harness.state().ui.docking.as_ref().unwrap()));
                harness
                    .render()
                    .unwrap()
                    .save(directory.join(format!("designcraft-shared-docking-{}-{language}-{width}-{scale}x.png", theme.id())))
                    .unwrap();
                if language == "ar" {
                    let tab = egui::Id::new("designcraft-panel-docking").with(("tab", &"swatches".to_string()));
                    let at = harness.ctx.read_response(tab).unwrap().rect.center();
                    harness.input_mut().events.extend([
                        Event::PointerMoved(at),
                        Event::PointerButton { pos: at, button: PointerButton::Secondary, pressed: true, modifiers: Modifiers::NONE },
                    ]);
                    harness.run_steps(1);
                    harness.input_mut().events.push(Event::PointerButton {
                        pos: at,
                        button: PointerButton::Secondary,
                        pressed: false,
                        modifiers: Modifiers::NONE,
                    });
                    harness.run_steps(2);
                    assert!(egui::Popup::is_any_open(&harness.ctx), "actual tab context menu must be open");
                    for action in ["Float Panel", "Group with", "Close Panel"] {
                        assert!(
                            harness.query_all_by_label(crate::i18n::tr("ar", action)).count() >= 2,
                            "both logical reference and actual menu label must exist"
                        );
                    }
                    harness.render().unwrap().save(directory.join(format!("designcraft-dock-context-{}-ar.png", theme.id()))).unwrap();
                }
            }
        }
    }
}

#[test]
fn escape_cancels_the_first_drag_without_replacing_the_legacy_workspace() {
    let mut h = harness(app(), vec2(1280.0, 800.0));
    let source = h.query_all_by_label("Layers").find(|node| node.rect().height() <= 45.0).unwrap().rect().center();
    h.event(Event::PointerMoved(source));
    h.run_steps(1);
    h.event(Event::PointerButton { pos: source, button: PointerButton::Primary, pressed: true, modifiers: Modifiers::NONE });
    h.run_steps(1);
    h.event(Event::PointerMoved(source - vec2(90.0, 0.0)));
    h.run_steps(2);
    assert!(h.state().ui.docking.is_some());
    h.event(Event::Key { key: egui::Key::Escape, physical_key: None, pressed: true, repeat: false, modifiers: Modifiers::NONE });
    h.run_steps(1);
    assert!(h.state().ui.docking.is_none());
    h.event(Event::PointerButton { pos: source, button: PointerButton::Primary, pressed: false, modifiers: Modifiers::NONE });
    h.run_steps(2);
    assert!(h.state().ui.docking.is_none());
}

#[test]
fn serialized_layout_actions_cover_geometry_reorder_accordion_and_atomic_rejection() {
    let mut app = app();
    app.run("window.panel.move", json!({"panel":"swatches","anchor":"layers","zone":"top"})).unwrap();
    app.run("window.panel.layout", json!({"action":{"ResizeSplit":{"path":[],"size":{"Ratio":0.35}}}})).unwrap();
    assert!(
        matches!(app.ui.docking.as_ref().unwrap().root, Some(Node::Split { size: craft_ui::layout::SplitSize::Ratio(value), .. }) if (value - 0.35).abs() < 0.001)
    );
    app.run("window.panel.float", json!({"panel":"properties"})).unwrap();
    app.run("window.panel.layout", json!({"action":{"MoveFloating":{"panel":"properties","rect":[44,66,360,450]}}})).unwrap();
    assert_eq!(
        app.ui.docking.as_ref().unwrap().floating.iter().find(|group| group.panels.iter().any(|p| p == "properties")).unwrap().rect,
        [44.0, 66.0, 360.0, 450.0]
    );
    app.run("window.panel.layout", json!({"action":{"Move":{"panel":"properties","anchor":"swatches","placement":{"Tab":{"before":"swatches"}}}}}))
        .unwrap();
    assert_eq!(group_members_for_test(app.ui.docking.as_ref().unwrap(), "swatches"), ["properties", "swatches"]);
    let before = app.ui.docking.clone();
    let hidden = app.ui.docking_hidden.clone();
    for action in [
        json!({"MoveFloating":{"panel":"properties","rect":[0,0,-5,30]}}),
        json!({"ResizeSplit":{"path":[true,true,true],"size":{"Ratio":0.2}}}),
        json!({"Open":{"panel":"unknown","anchor":null}}),
        json!({"Move":{"panel":"properties","anchor":"swatches","placement":{"Tab":{"before":"unknown"}}}}),
    ] {
        assert!(app.run("window.panel.layout", json!({"action":action})).is_err());
        assert_eq!(app.ui.docking, before);
        assert_eq!(app.ui.docking_hidden, hidden);
    }
    app.ui.docking = Some(Layout {
        root: Some(Node::Stack { entries: vec![craft_ui::docking::StackEntry { panel: "layers".into(), open: true, height: Some(100.0) }] }),
        floating: Vec::new(),
    });
    app.run("window.panel.layout", json!({"action":{"SetStackOpen":{"panel":"layers","open":false}}})).unwrap();
    app.run("window.panel.layout", json!({"action":{"ResizeStack":{"panel":"layers","height":150}}})).unwrap();
    let Some(Node::Stack { entries }) = &app.ui.docking.as_ref().unwrap().root else { panic!("expected stack") };
    assert!(!entries[0].open);
    assert_eq!(entries[0].height, Some(150.0));
}

fn group_members_for_test(layout: &Layout<String>, panel: &str) -> Vec<String> {
    let mut pending: Vec<_> = layout.root.iter().collect();
    while let Some(node) = pending.pop() {
        match node {
            Node::Tabs { panels, .. } if panels.iter().any(|p| p == panel) => return panels.clone(),
            Node::Split { first, second, .. } => pending.extend([first.as_ref(), second.as_ref()]),
            _ => {}
        }
    }
    Vec::new()
}

#[test]
fn populated_canvas_keeps_a_real_stroke_edit_after_floating_and_redocking() {
    let mut app = app();
    app.run("file.new", json!({"width":480,"height":360})).unwrap();
    let id = app.run("frame.create", json!({"rect":[80,70,330,230],"content":"none"})).unwrap()["id"].as_u64().unwrap();
    app.run("object.fill", json!({"swatch":"C=100 M=0 Y=0 K=0"})).unwrap();
    app.run("object.stroke", json!({"weight":8})).unwrap();
    app.run("window.panel.float", json!({"panel":"stroke","x":40,"y":140,"width":310,"height":490})).unwrap();
    let directory = std::env::var_os("CRAFT_UI_DOCKING_FIXTURES").map(std::path::PathBuf::from);
    let builder = Harness::builder().with_size(vec2(1280.0, 800.0));
    let builder = if directory.is_some() { builder.wgpu() } else { builder };
    let mut h = builder.build_ui_state(
        |ui, app: &mut DesignApp| {
            app.logic(ui.ctx());
            app.ui(ui);
        },
        app,
    );
    h.input_mut().max_texture_side = Some(8192);
    h.run_steps(5);
    let at = h.get_by_label("Bevel").rect().center();
    h.hover_at(at);
    h.run_steps(1);
    for pressed in [true, false] {
        h.event(Event::PointerButton { pos: at, button: PointerButton::Primary, pressed, modifiers: Modifiers::NONE });
        h.run_steps(1);
    }
    h.run_steps(2);
    assert_eq!(h.state().session.active().unwrap().doc.item(designcraft_doc::ItemId(id)).unwrap().stroke.join, designcraft_doc::Join::Bevel);
    if let Some(directory) = &directory {
        std::fs::create_dir_all(directory).unwrap();
        h.render().unwrap().save(directory.join("designcraft-populated-stroke-floating.png")).unwrap();
    }
    let from = h.ctx.read_response(egui::Id::new("designcraft-panel-docking").with(("tab", &"stroke".to_string()))).unwrap().rect.center();
    let to = h.ctx.read_response(egui::Id::new("designcraft-panel-docking").with(("tab", &"properties".to_string()))).unwrap().rect.center();
    drag(&mut h, from, to);
    assert!(h.state().ui.docking.as_ref().unwrap().floating.is_empty());
    assert_eq!(h.state().session.active().unwrap().doc.item(designcraft_doc::ItemId(id)).unwrap().stroke.join, designcraft_doc::Join::Bevel);
    if let Some(directory) = &directory {
        h.render().unwrap().save(directory.join("designcraft-populated-stroke-redocked.png")).unwrap();
    }
}

#[test]
fn properties_stroke_link_opens_the_stroke_panel_in_a_custom_workspace() {
    let mut app = app();
    app.run("file.new", json!({"width":480,"height":360})).unwrap();
    app.run("frame.create", json!({"rect":[80,70,330,230],"content":"none"})).unwrap();
    app.run("window.panel.float", json!({"panel":"layers"})).unwrap();
    // Floating Layers leaves Pages the active tab beside Properties.
    app.run("window.panel", json!({"panel":"properties"})).unwrap();
    assert!(!app.ui.docking.as_ref().unwrap().contains(&"stroke".into()));
    let mut h = Harness::builder().with_size(vec2(1280.0, 800.0)).build_ui_state(
        |ui, app: &mut DesignApp| {
            app.logic(ui.ctx());
            app.ui(ui);
        },
        app,
    );
    h.run_steps(5);
    let at = h.get_by_role_and_label(egui::accesskit::Role::Link, "Stroke").rect().center();
    h.hover_at(at);
    h.run_steps(1);
    for pressed in [true, false] {
        h.event(Event::PointerButton { pos: at, button: PointerButton::Primary, pressed, modifiers: Modifiers::NONE });
        h.run_steps(1);
    }
    h.run_steps(2);
    assert!(h.state().ui.docking.as_ref().unwrap().contains(&"stroke".into()));
    assert_eq!(h.state().ui.docking_opened.as_deref(), Some("stroke"));
}

#[test]
fn changing_workspace_during_a_drag_cancels_the_stale_gesture() {
    let mut app = app();
    app.run("window.panel.move", json!({"panel":"swatches","anchor":"layers","zone":"top"})).unwrap();
    let destination = app.ui.docking.clone();
    app.run("window.newWorkspace", json!({"name":"Destination"})).unwrap();
    app.run("window.workspace", json!({"name":"Essentials"})).unwrap();
    let mut h = harness(app, vec2(1280.0, 800.0));
    let source = h.query_all_by_label("Layers").find(|node| node.rect().height() <= 45.0).unwrap().rect().center();
    h.event(Event::PointerMoved(source));
    h.run_steps(1);
    h.event(Event::PointerButton { pos: source, button: PointerButton::Primary, pressed: true, modifiers: Modifiers::NONE });
    h.run_steps(1);
    h.event(Event::PointerMoved(source - vec2(90.0, 0.0)));
    h.run_steps(2);
    h.state_mut().run("window.workspace", json!({"name":"Destination"})).unwrap();
    h.event(Event::Key { key: egui::Key::Escape, physical_key: None, pressed: true, repeat: false, modifiers: Modifiers::NONE });
    h.run_steps(1);
    assert_eq!(h.state().ui.docking, destination);
    h.event(Event::PointerButton { pos: source, button: PointerButton::Primary, pressed: false, modifiers: Modifiers::NONE });
    h.run_steps(2);
    assert_eq!(h.state().ui.docking, destination);
}

#[test]
fn first_customization_preserves_the_visible_legacy_flyout() {
    let mut app = app();
    app.ui.open_panel = Some("paragraphStyles".into());
    app.ui.dock_expanded = false;
    app.run("window.panel.float", json!({"panel":"layers"})).unwrap();
    let layout = app.ui.docking.as_ref().unwrap();
    assert!(layout.contains(&"paragraphStyles".into()));
    assert!(valid(layout));
    assert_eq!(layout.panels().iter().filter(|p| p.as_str() == "paragraphStyles").count(), 1);
}

#[test]
fn hiding_before_first_destination_frame_does_not_restore_an_old_drag_snapshot() {
    let mut app = app();
    app.run("window.panel.move", json!({"panel":"swatches","anchor":"layers","zone":"top"})).unwrap();
    app.run("window.newWorkspace", json!({"name":"Destination"})).unwrap();
    app.run("window.workspace", json!({"name":"Essentials"})).unwrap();
    let mut h = harness(app, vec2(1280.0, 800.0));
    let source = h.query_all_by_label("Layers").find(|node| node.rect().height() <= 45.0).unwrap().rect().center();
    h.event(Event::PointerMoved(source));
    h.run_steps(1);
    h.event(Event::PointerButton { pos: source, button: PointerButton::Primary, pressed: true, modifiers: Modifiers::NONE });
    h.run_steps(1);
    h.event(Event::PointerMoved(source - vec2(90.0, 0.0)));
    h.run_steps(2);
    h.state_mut().run("window.workspace", json!({"name":"Destination"})).unwrap();
    let destination = h.state().ui.docking.clone();
    let open_panel = h.state().ui.open_panel.clone();
    let floating = h.state().ui.floating.clone();
    // The app calls this before dock rendering when panels are hidden or Presentation starts.
    let ctx = h.ctx.clone();
    cancel_gesture(h.state_mut(), &ctx);
    assert_eq!(h.state().ui.docking, destination);
    assert_eq!(h.state().ui.open_panel, open_panel);
    assert_eq!(h.state().ui.floating, floating);
    h.event(Event::PointerButton { pos: source, button: PointerButton::Primary, pressed: false, modifiers: Modifiers::NONE });
    h.run_steps(2);
    assert_eq!(h.state().ui.docking, destination);
}

#[test]
fn legacy_table_and_control_visibility_work_after_customization_without_document_edits() {
    let mut app = app();
    app.run("file.new", json!({})).unwrap();
    let doc = serde_json::to_value(&app.session.active().unwrap().doc).unwrap();
    let undo = app.session.active().unwrap().history.undo.len();
    app.run("window.panel.float", json!({"panel":"layers"})).unwrap();
    app.run("app.tablePanel", json!({})).unwrap();
    assert!(app.ui.docking.as_ref().unwrap().contains(&"table".into()));
    app.run("app.tablePanel", json!({})).unwrap();
    assert!(!app.ui.docking.as_ref().unwrap().contains(&"table".into()));
    app.run("app.tablePanel", json!({})).unwrap();
    let ctx = egui::Context::default();
    let (req, _) = crate::control::ControlRequest::new("ui.set", json!({"closePanel":true,"dockExpanded":false}));
    let crate::control::Outcome::Done(result) = crate::control::handle(&mut app, &ctx, &req) else { panic!("expected synchronous control result") };
    assert_eq!(result["ok"], true);
    assert!(!app.ui.dock_expanded);
    assert!(!app.ui.docking.as_ref().unwrap().contains(&"table".into()));
    app.run("window.panel", json!({"panel":"properties"})).unwrap();
    assert!(app.ui.dock_expanded);
    assert_eq!(app.ui.docking_raise.as_deref(), Some("properties"));
    assert_eq!(serde_json::to_value(&app.session.active().unwrap().doc).unwrap(), doc);
    assert_eq!(app.session.active().unwrap().history.undo.len(), undo);
}

#[test]
fn floating_activation_queues_one_transient_raise_and_workspace_switch_clears_it() {
    let mut app = app();
    app.run("window.panel.float", json!({"panel":"layers"})).unwrap();
    app.run("window.panel.activate", json!({"panel":"layers"})).unwrap();
    assert_eq!(app.ui.docking_raise.as_deref(), Some("layers"));
    assert!(serde_json::to_value(&app.ui).unwrap().get("docking_raise").is_none());
    app.run("window.workspace", json!({"name":"Typography"})).unwrap();
    assert!(app.ui.docking_raise.is_none());
}

#[test]
fn shaped_panel_label_keeps_the_logical_accessibility_text() {
    let ctx = egui::Context::default();
    let logical = "خصائص Pages";
    crate::theme::install_fonts(&ctx, "ar");
    let mut output = ctx.run_ui(egui::RawInput::default(), |ui| {
        let galley = crate::rtl::plain(ui.ctx(), logical, egui::FontId::proportional(12.0), egui::Color32::WHITE);
        assert_eq!(galley.job.text, logical);
    });
    output.textures_delta.clear();
}

#[test]
fn modal_overlay_blocks_a_real_tab_drag_release() {
    let mut app = app();
    app.run("window.panel.float", json!({"panel":"layers"})).unwrap();
    let mut h = harness(app, vec2(1280.0, 800.0));
    let area = egui::Id::new("designcraft-panel-docking");
    let source = h.ctx.read_response(area.with(("tab", &"properties".to_string()))).unwrap().rect.center();
    h.event(Event::PointerMoved(source));
    h.run_steps(1);
    h.event(Event::PointerButton { pos: source, button: PointerButton::Primary, pressed: true, modifiers: Modifiers::NONE });
    h.run_steps(1);
    h.event(Event::PointerMoved(source - vec2(90.0, 0.0)));
    h.run_steps(2);
    assert_eq!(h.ctx.dragged_id(), Some(area.with(("tab", &"properties".to_string()))), "test must start the owned tab gesture");
    let before = h.state().ui.docking.clone();
    h.state_mut().ui.dialog = Some(crate::dialogs::Dialog::new("newDocument", json!({})));
    let destination = egui::pos2(150.0, 150.0);
    h.event(Event::PointerMoved(destination));
    h.run_steps(1);
    h.event(Event::PointerButton { pos: destination, button: PointerButton::Primary, pressed: false, modifiers: Modifiers::NONE });
    h.run_steps(2);
    assert_eq!(h.state().ui.docking, before, "release under a modal must not float or dock the panel");
    h.state_mut().ui.dialog = None;
    h.run_steps(2);
    assert_eq!(h.state().ui.docking, before, "closing the overlay must not revive its blocked drag");
}

#[test]
fn shift_f9_toggles_table_after_customization_through_the_real_shortcut_path() {
    let mut app = app();
    app.run("file.new", json!({})).unwrap();
    app.run("window.panel.float", json!({"panel":"layers"})).unwrap();
    let before = serde_json::to_value(&app.session.active().unwrap().doc).unwrap();
    let mut h = crate::test_window::open(app, vec2(1440.0, 900.0));
    for visible in [true, false] {
        let modifiers = Modifiers { shift: true, ..Modifiers::NONE };
        h.event(Event::Key { key: egui::Key::F9, physical_key: None, pressed: true, repeat: false, modifiers });
        h.run_steps(1);
        h.event(Event::Key { key: egui::Key::F9, physical_key: None, pressed: false, repeat: false, modifiers });
        h.run_steps(2);
        assert_eq!(h.state().app.ui.docking.as_ref().unwrap().contains(&"table".into()), visible);
    }
    assert_eq!(serde_json::to_value(&h.state().app.session.active().unwrap().doc).unwrap(), before);
}

#[test]
fn first_story_editor_frame_conservatively_blocks_owned_tab_release() {
    let mut app = app();
    app.run("file.new", json!({})).unwrap();
    let story = app.run("frame.create", json!({"rect":[72,72,300,200],"content":"text","text":"Story editor overlay fixture"})).unwrap()["story"]
        .as_u64()
        .unwrap();
    app.run("window.panel.float", json!({"panel":"layers"})).unwrap();
    let doc = serde_json::to_value(&app.session.active().unwrap().doc).unwrap();
    let mut h = crate::test_window::open(app, vec2(1440.0, 900.0));
    let tab = egui::Id::new("designcraft-panel-docking").with(("tab", &"properties".to_string()));
    let from = h.ctx.read_response(tab).unwrap().rect.center();
    h.event(Event::PointerMoved(from));
    h.run_steps(1);
    h.event(Event::PointerButton { pos: from, button: PointerButton::Primary, pressed: true, modifiers: Modifiers::NONE });
    h.run_steps(1);
    h.event(Event::PointerMoved(from - vec2(90.0, 0.0)));
    h.run_steps(2);
    assert_eq!(h.ctx.dragged_id(), Some(tab), "must own a real tab drag before opening editor");
    assert!(h.ctx.memory(|m| m.area_rect(egui::Id::new("story_editor"))).is_none());
    let layout = h.state().app.ui.docking.clone();
    let frame_before = h.ctx.cumulative_frame_nr();
    h.state_mut().app.run("app.storyEditor", json!({"story":story})).unwrap();
    // The unmeasured first frame blocks the full content, even outside the eventual editor.
    let release = egui::pos2(150.0, 150.0);
    h.input_mut().events.extend([
        Event::PointerMoved(release),
        Event::PointerButton { pos: release, button: PointerButton::Primary, pressed: false, modifiers: Modifiers::NONE },
    ]);
    h.run_steps(1);
    assert_eq!(h.ctx.cumulative_frame_nr() - frame_before, 1, "release must occur in the first editor frame");
    let measured = h.ctx.memory(|m| m.area_rect(egui::Id::new("story_editor"))).unwrap();
    assert!(!measured.contains(release), "fixture exercises the conservative first-frame guard outside eventual editor bounds");
    assert_eq!(h.state().app.ui.docking, layout, "first editor frame must block release before it has a measured rectangle");
    assert_eq!(serde_json::to_value(&h.state().app.session.active().unwrap().doc).unwrap(), doc);
    h.run_steps(3);
    let editor = h.ctx.memory(|m| m.area_rect(egui::Id::new("story_editor"))).unwrap();
    let screen = h.ctx.content_rect();
    let outside = [screen.left_bottom() + vec2(80.0, -80.0), screen.right_bottom() + vec2(-450.0, -80.0)]
        .into_iter()
        .find(|p| !editor.contains(*p))
        .expect("fixture has space outside editor");
    let before_outside = h.state().app.ui.docking.clone();
    let tab = egui::Id::new("designcraft-panel-docking").with(("tab", &"layers".to_string()));
    let from = h.ctx.read_response(tab).unwrap().rect.center();
    h.event(Event::PointerMoved(from));
    h.run_steps(1);
    h.event(Event::PointerButton { pos: from, button: PointerButton::Primary, pressed: true, modifiers: Modifiers::NONE });
    h.run_steps(1);
    h.event(Event::PointerMoved(from - vec2(90.0, 0.0)));
    h.run_steps(2);
    assert_eq!(h.ctx.dragged_id(), Some(tab));
    h.event(Event::PointerMoved(outside));
    h.event(Event::PointerButton { pos: outside, button: PointerButton::Primary, pressed: false, modifiers: Modifiers::NONE });
    h.run_steps(2);
    assert_ne!(h.state().app.ui.docking, before_outside, "measured editor must permit ordinary releases outside its bounds");
    let after = h.state().app.ui.docking.clone();
    h.state_mut().app.run("app.storyEditor", json!({"story":story})).unwrap();
    h.run_steps(3);
    assert_eq!(h.state().app.ui.docking, after, "closing editor must not revive a blocked drag");
    assert_eq!(serde_json::to_value(&h.state().app.session.active().unwrap().doc).unwrap(), doc);
}

#[test]
fn capture_legacy_panel_menu_visual_fixture() {
    let Some(directory) = std::env::var_os("CRAFT_UI_DOCKING_FIXTURES").map(std::path::PathBuf::from) else { return };
    std::fs::create_dir_all(&directory).unwrap();
    let mut app = app();
    app.run("file.new", json!({})).unwrap();
    app.run("frame.create", json!({"rect":[40,40,280,180],"content":"text","text":"Legacy Arabic menu fixture"})).unwrap();
    app.run("app.language", json!({"lang":"ar"})).unwrap();
    let doc = serde_json::to_value(&app.session.active().unwrap().doc).unwrap();
    let mut ready = false;
    let mut h = Harness::builder().with_size(vec2(800.0, 800.0)).wgpu().build_ui_state(
        move |ui, app: &mut DesignApp| {
            if !ready {
                crate::theme::install_fonts(ui.ctx(), &app.ui.language);
                crate::theme::apply(ui.ctx(), &crate::theme::Tokens::for_brightness(crate::theme::Brightness::Dark));
                ready = true;
                return;
            }
            crate::dock::show(app, ui);
            egui::CentralPanel::default().show(ui, |ui| {
                ui.label("Shaped legacy menu references:");
                for label in ["Float Panel", "Layers"] {
                    crate::rtl::label(ui, crate::i18n::tr("ar", label));
                }
            });
        },
        app,
    );
    h.input_mut().max_texture_side = Some(8192);
    h.run_steps(5);
    assert!(h.state().ui.docking.is_none(), "fixture must use the actual legacy menu before customization");
    let at = h.get_by_label("Properties").rect().center();
    h.input_mut().events.extend([
        Event::PointerMoved(at),
        Event::PointerButton { pos: at, button: PointerButton::Secondary, pressed: true, modifiers: Modifiers::NONE },
    ]);
    h.run_steps(1);
    h.input_mut().events.push(Event::PointerButton { pos: at, button: PointerButton::Secondary, pressed: false, modifiers: Modifiers::NONE });
    h.run_steps(2);
    assert!(egui::Popup::is_any_open(&h.ctx));
    assert_eq!(h.query_all_by_label(crate::i18n::tr("ar", "Float Panel")).count(), 2, "reference and actual logical action must be accessible");
    h.render().unwrap().save(directory.join("designcraft-legacy-context-dark-ar.png")).unwrap();
    let group = h.get_by_label_contains(crate::i18n::tr("ar", "Group with")).rect().center();
    h.hover_at(group);
    h.run_steps(4);
    assert_eq!(h.query_all_by_label(crate::i18n::tr("ar", "Layers")).count(), 2, "reference and actual target must be accessible");
    h.render().unwrap().save(directory.join("designcraft-legacy-targets-dark-ar.png")).unwrap();
    assert!(h.state().ui.docking.is_none());
    let (last, last_label, _) = crate::dock::ICON_PANELS.last().unwrap();
    let logical = crate::i18n::tr("ar", last_label);
    let scroll_at = h.query_all_by_label(crate::i18n::tr("ar", "Layers")).find(|node| node.rect().left() > 500.0).unwrap().rect().center();
    let submenu_layer = h.ctx.layer_id_at(scroll_at).unwrap();
    let submenu = h.ctx.memory(|m| m.area_rect(submenu_layer.id)).unwrap();
    let before = h.query_all_by_label(logical).find(|node| submenu.x_range().contains(node.rect().center().x)).map(|node| node.rect());
    assert!(before.is_none_or(|rect| !submenu.contains_rect(rect)), "actual submenu must require scrolling to reach its last target");
    h.input_mut().events.extend([
        Event::PointerMoved(scroll_at),
        Event::MouseWheel { unit: egui::MouseWheelUnit::Point, delta: vec2(0.0, -1600.0), phase: egui::TouchPhase::Move, modifiers: Modifiers::NONE },
    ]);
    h.run_steps(12);
    let target = h.query_all_by_label(logical).find(|node| submenu.x_range().contains(node.rect().center().x)).unwrap().rect();
    assert!(submenu.contains_rect(target), "last target must be inside the actual submenu after scrolling");
    assert!(h.ctx.content_rect().contains_rect(target), "last target must be physically visible after wheel scrolling");
    h.render().unwrap().save(directory.join("designcraft-legacy-targets-scrolled-dark-ar.png")).unwrap();
    h.query_all_by_label(logical).find(|node| submenu.contains_rect(node.rect())).unwrap().click();
    h.run_steps(4);
    let layout = h.state().ui.docking.as_ref().unwrap();
    assert_eq!(layout.panels().iter().filter(|panel| panel.as_str() == "properties").count(), 1);
    assert_eq!(layout.panels().iter().filter(|panel| panel.as_str() == *last).count(), 1);
    let group = group_members_for_test(layout, last);
    assert!(group.iter().any(|panel| panel == "properties"));
    assert_eq!(group.last().map(String::as_str), Some("properties"), "clicked destination must receive the original panel after it");
    assert_eq!(serde_json::to_value(&h.state().session.active().unwrap().doc).unwrap(), doc);
    h.render().unwrap().save(directory.join("designcraft-legacy-last-target-selected-dark-ar.png")).unwrap();
}

#[test]
fn legacy_dock_menu_button_opens_the_active_panel_menu() {
    let mut app = app();
    app.run("file.new", json!({})).unwrap();
    app.ui.dock_tab = "pages".into();
    let pages = app.session.active().unwrap().doc.page_count();
    let mut h = harness(app, vec2(1280.0, 800.0));
    assert!(h.state().ui.docking.is_none());
    let tab = h.query_all_by_label("Pages").find(|node| node.rect().height() <= 45.0).unwrap().rect();
    let dock = egui::containers::panel::PanelState::load(&h.ctx, egui::Id::new("dock")).unwrap().outer_rect;
    let at = egui::pos2(dock.right() - 15.0, tab.center().y);
    h.event(Event::PointerMoved(at));
    h.run_steps(1);
    for pressed in [true, false] {
        h.event(Event::PointerButton { pos: at, button: PointerButton::Primary, pressed, modifiers: Modifiers::NONE });
        h.run_steps(1);
    }
    h.run_steps(2);
    assert!(egui::Popup::is_any_open(&h.ctx), "the three-line button must open a menu");
    h.get_by_label("Duplicate Spread").click();
    h.run_steps(3);
    assert_eq!(h.state().session.active().unwrap().doc.page_count(), pages + 1);
    assert!(!egui::Popup::is_any_open(&h.ctx));
}

/// InDesign's panel tab menu (Learn Skills Daily's InDesign course, 6:20): Close, Close Tab Group,
/// Minimize and Collapse to Icons. Close Tab Group hides the panels tabbed together.
#[test]
fn close_tab_group_hides_the_panels_tabbed_together() {
    let mut app = app();
    // The default dock: Properties, Pages and Layers are one tab group.
    let r = app.run("window.panel.closeGroup", json!({"panel":"pages"})).unwrap();
    assert_eq!(r["closed"], json!(["properties", "pages", "layers"]));
    let layout = app.ui.docking.clone().unwrap();
    assert!(["properties", "pages", "layers"].iter().all(|p| !layout.contains(&p.to_string())));
    // Window brings one back.
    app.run("window.panel.activate", json!({"panel":"pages"})).unwrap();
    assert!(app.ui.docking.as_ref().unwrap().contains(&"pages".to_string()));
    // A floating group goes as a whole; the dock keeps its own.
    app.run("window.panel.float", json!({"panel":"swatches","x":40,"y":60})).unwrap();
    app.run("window.panel.activate", json!({"panel":"color"})).unwrap();
    app.run("window.panel.move", json!({"panel":"color", "anchor":"swatches"})).unwrap();
    app.run("window.panel.closeGroup", json!({"panel":"color"})).unwrap();
    let layout = app.ui.docking.clone().unwrap();
    assert!(!layout.contains(&"color".to_string()) && !layout.contains(&"swatches".to_string()));
    assert!(layout.contains(&"pages".to_string()));
    assert!(app.run("window.panel.closeGroup", json!({"panel":"nope"})).is_err());
}

#[test]
fn collapse_to_icons_shrinks_the_dock_to_its_icons() {
    let mut app = app();
    assert!(app.ui.dock_expanded);
    app.run("window.collapseToIcons", json!({})).unwrap();
    assert!(!app.ui.dock_expanded);
    // A panel chosen from Window opens the dock again.
    app.run("window.panel", json!({"panel":"pages"})).unwrap();
    assert!(app.ui.dock_expanded);
}

#[test]
fn a_tabs_context_menu_lists_indesigns_tab_commands_and_close_works() {
    let mut h = harness(app(), vec2(1280.0, 800.0));
    let tab = h.query_all_by_label("Layers").find(|node| node.rect().height() <= 45.0).unwrap().rect().center();
    h.event(Event::PointerMoved(tab));
    h.run_steps(1);
    h.event(Event::PointerButton { pos: tab, button: PointerButton::Secondary, pressed: true, modifiers: Modifiers::NONE });
    h.run_steps(1);
    h.event(Event::PointerButton { pos: tab, button: PointerButton::Secondary, pressed: false, modifiers: Modifiers::NONE });
    h.run_steps(2);
    for item in ["Close", "Close Tab Group", "Minimize", "Collapse to Icons", "Float Panel"] {
        assert!(h.query_by_label(item).is_some(), "{item}");
    }
    h.get_by_label("Close").click();
    h.run_steps(2);
    assert!(!h.state().ui.docking.as_ref().unwrap().contains(&"layers".to_string()));
}

/// Window › Styles › Paragraph Styles opens InDesign's floating Character Styles | Paragraph Styles
/// group, which stays open while other panels open (Learn Skills Daily's InDesign course, 7:55).
#[test]
fn paragraph_styles_open_floating_with_character_styles() {
    let mut app = app();
    app.run("window.panel", json!({"panel":"paragraphStyles"})).unwrap();
    let layout = app.ui.docking.clone().unwrap();
    let group = layout.floating.iter().find(|g| g.panels.iter().any(|p| p == "paragraphStyles")).expect("a floating group");
    assert_eq!(group.panels, ["characterStyles", "paragraphStyles"]);
    assert_eq!(group.panels[group.active], "paragraphStyles", "the one chosen is in front");
    // The dock still shows Properties in front.
    assert!(matches!(&layout.root, Some(craft_ui::docking::Node::Tabs { panels, active }) if panels[*active] == "properties"), "{:?}", layout.root);
    // Other panels from Window leave it open.
    app.run("window.panel", json!({"panel":"color"})).unwrap();
    app.run("window.panel", json!({"panel":"links"})).unwrap();
    let layout = app.ui.docking.clone().unwrap();
    assert!(layout.floating.iter().any(|g| g.panels == ["characterStyles", "paragraphStyles"]));
    // Once showing, Window › Character Styles brings that tab forward.
    app.run("window.panel", json!({"panel":"characterStyles"})).unwrap();
    let layout = app.ui.docking.clone().unwrap();
    let group = layout.floating.iter().find(|g| g.panels.iter().any(|p| p == "characterStyles")).unwrap();
    assert_eq!(group.panels[group.active], "characterStyles");
    assert_eq!(layout.panels().iter().filter(|p| p.as_str() == "characterStyles").count(), 1);
}

/// Style panels the user docked and then closed come back in the dock, not floating.
#[test]
fn paragraph_styles_closed_from_the_dock_reopen_there() {
    let mut app = app();
    app.run("window.panel", json!({"panel":"paragraphStyles"})).unwrap();
    for panel in ["characterStyles", "paragraphStyles"] {
        app.run("window.panel.move", json!({"panel": panel, "anchor": "properties"})).unwrap();
    }
    for panel in ["characterStyles", "paragraphStyles"] {
        app.run("window.panel.close", json!({"panel": panel})).unwrap();
    }
    app.run("window.panel", json!({"panel":"paragraphStyles"})).unwrap();
    let layout = app.ui.docking.clone().unwrap();
    assert!(layout.contains(&"paragraphStyles".to_string()));
    assert!(layout.floating.iter().all(|g| !g.panels.iter().any(|p| p == "paragraphStyles")), "{:?}", layout.floating);
}

fn click(h: &mut Harness<'static, DesignApp>, at: Pos2, button: PointerButton) {
    h.event(Event::PointerMoved(at));
    h.run_steps(1);
    for pressed in [true, false] {
        h.event(Event::PointerButton { pos: at, button, pressed, modifiers: Modifiers::NONE });
        h.run_steps(1);
    }
    h.run_steps(2);
}

/// Window › Paragraph Styles switches to the docking layout. After Window › Pages the Pages panel
/// keeps its three-line panel menu, and its tab menu lists what the classic dock's does.
#[test]
fn pages_keeps_its_panel_menu_in_the_docking_layout() {
    let mut app = app();
    app.run("file.new", json!({})).unwrap();
    app.run("window.panel", json!({"panel":"paragraphStyles"})).unwrap();
    app.run("window.panel", json!({"panel":"pages"})).unwrap();
    assert!(app.ui.docking.is_some());
    let mut h = harness(app, vec2(1280.0, 800.0));
    let tab = h.query_all_by_label("Pages").find(|node| node.rect().height() <= 45.0).unwrap().rect();
    let dock = egui::containers::panel::PanelState::load(&h.ctx, egui::Id::new("shared-panel-dock")).unwrap().outer_rect;
    click(&mut h, egui::pos2(dock.right() - 12.0, tab.center().y), PointerButton::Primary);
    assert!(egui::Popup::is_any_open(&h.ctx), "the three-line button must open a menu");
    for item in ["Numbering & Section Options…", "Close Tab Group", "Float Panel"] {
        assert!(h.query_by_label(item).is_some(), "{item}");
    }
    h.get_by_label("Panel Options…").click();
    h.run_steps(3);
    assert!(h.state().ui.dialog.as_ref().is_some_and(|d| d.id == "pagesPanelOptions"));
    h.state_mut().ui.dialog = None;
    h.run_steps(2);

    click(&mut h, tab.center(), PointerButton::Secondary);
    for item in ["Close", "Close Tab Group", "Minimize", "Collapse to Icons", "Float Panel", "Group with"] {
        assert!(h.query_all_by_label(item).next().is_some(), "{item}");
    }
    assert!(h.query_by_label("Close Panel").is_none());
    assert!(h.query_by_label("Panel Options…").is_none(), "the panel's own commands stay in its panel menu");
}
