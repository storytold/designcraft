use super::{
    CANVAS, Column, DOCK_LABELS, DesignApp, Floating, ICON_GROUPS, Layout, Node, PanelContent, Placed, Side, Value, area_id, cancel_gesture,
    close_visible, default_layout, dock_columns, hit_id, icon_column, in_iconic_column, json, layout_of, modal_open, open_window_drops, panel_menu,
    set_iconic, showing, tab_group, tab_id, valid, window_groups,
};
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
    assert!(app.ui.docking.as_ref().unwrap().floating.iter().flat_map(window_groups).any(|g| g.panels == ["properties"]));
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
            egui::CentralPanel::default().frame(egui::Frame::NONE).show(ui, |ui| crate::panel_docking::show(app, ui));
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
    assert!(layout.floating.iter().flat_map(window_groups).any(|g| g.panels.contains(&"layers".into())), "{layout:?}");
    let source = h.ctx.read_response(tab_id("layers")).unwrap().rect.center();
    let destination = h.ctx.read_response(tab_id("properties")).unwrap().rect.center();
    drag(&mut h, source, destination);
    let layout = h.state().ui.docking.as_ref().unwrap();
    assert!(!layout.floating.iter().flat_map(window_groups).any(|g| g.panels.contains(&"layers".into())));
    assert!(valid(layout));
    assert_eq!(h.state().ui.docking_focused.as_deref(), Some("layers"), "the user's drag keeps the focus on the panel it moved");
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
                        egui::CentralPanel::default().frame(egui::Frame::NONE).show(ui, |ui| crate::panel_docking::show(app, ui));
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
                    let tab = tab_id("swatches");
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
                        assert!(harness.query_all_by_label(crate::i18n::tr("ar", action)).count() >= 1, "the menu lists {action}");
                    }
                    harness.render().unwrap().save(directory.join(format!("designcraft-dock-context-{}-ar.png", theme.id()))).unwrap();
                }
            }
        }
    }
}

#[test]
fn escape_cancels_a_tab_drag_without_customizing_the_workspace() {
    let mut h = harness(app(), vec2(1280.0, 800.0));
    let source = h.query_all_by_label("Layers").find(|node| node.rect().height() <= 45.0).unwrap().rect().center();
    h.event(Event::PointerMoved(source));
    h.run_steps(1);
    h.event(Event::PointerButton { pos: source, button: PointerButton::Primary, pressed: true, modifiers: Modifiers::NONE });
    h.run_steps(1);
    h.event(Event::PointerMoved(source - vec2(90.0, 0.0)));
    h.run_steps(2);
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
        app.ui.docking.as_ref().unwrap().floating.iter().find(|window| window.panels().any(|p| p == "properties")).unwrap().rect,
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
    let stack = Node::Stack { entries: vec![craft_ui::docking::StackEntry { panel: "layers".into(), open: true, height: Some(100.0) }] };
    app.ui.docking = Some(Layout {
        root: Some(Node::Split {
            axis: craft_ui::layout::SplitAxis::Horizontal,
            size: craft_ui::layout::SplitSize::Ratio(0.7),
            first: Box::new(Node::Tabs { panels: vec![CANVAS.into()], active: 0 }),
            second: Box::new(stack),
        }),
        floating: Vec::new(),
    });
    app.run("window.panel.layout", json!({"action":{"SetStackOpen":{"panel":"layers","open":false}}})).unwrap();
    app.run("window.panel.layout", json!({"action":{"ResizeStack":{"panel":"layers","height":150}}})).unwrap();
    let Some(Node::Split { second, .. }) = &app.ui.docking.as_ref().unwrap().root else { panic!("expected split") };
    let Node::Stack { entries } = &**second else { panic!("expected stack") };
    assert!(!entries[0].open);
    assert_eq!(entries[0].height, Some(150.0));
}

fn group_members_for_test(layout: &Layout<String>, panel: &str) -> Vec<String> {
    tab_group(layout, panel)
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
    let from = h.ctx.read_response(tab_id("stroke")).unwrap().rect.center();
    let to = h.ctx.read_response(tab_id("properties")).unwrap().rect.center();
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
    assert!(in_iconic_column(app.ui.docking.as_ref().unwrap(), "stroke"));
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
    // Stroke stays in the iconic column and opens as its flyout.
    assert!(in_iconic_column(h.state().ui.docking.as_ref().unwrap(), "stroke"));
    assert_eq!(h.state().ui.docking_flyout.as_deref(), Some("stroke"));
    assert!(showing(h.state(), "stroke"));
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
fn first_customization_keeps_the_panel_the_workspace_opened() {
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
    cancel_gesture(&ctx);
    assert_eq!(h.state().ui.docking, destination);
    assert_eq!(h.state().ui.open_panel, open_panel);
    assert_eq!(h.state().ui.floating, floating);
    h.event(Event::PointerButton { pos: source, button: PointerButton::Primary, pressed: false, modifiers: Modifiers::NONE });
    h.run_steps(2);
    assert_eq!(h.state().ui.docking, destination);
}

#[test]
fn table_and_control_visibility_work_after_customization_without_document_edits() {
    let mut app = app();
    app.run("file.new", json!({})).unwrap();
    let doc = serde_json::to_value(&app.session.active().unwrap().doc).unwrap();
    let undo = app.session.active().unwrap().history.undo.len();
    app.run("window.panel.float", json!({"panel":"layers"})).unwrap();
    app.run("app.tablePanel", json!({})).unwrap();
    assert!(showing(&app, "table"));
    // The Table flyout closes; Table stays in its icon group.
    app.run("app.tablePanel", json!({})).unwrap();
    assert!(!showing(&app, "table"));
    assert!(in_iconic_column(app.ui.docking.as_ref().unwrap(), "table"));
    app.run("app.tablePanel", json!({})).unwrap();
    assert!(showing(&app, "table"));
    let ctx = egui::Context::default();
    let (req, _) = crate::control::ControlRequest::new("ui.set", json!({"closePanel":true,"dockExpanded":false}));
    let crate::control::Outcome::Done(result) = crate::control::handle(&mut app, &ctx, &req) else { panic!("expected synchronous control result") };
    assert_eq!(result["ok"], true);
    assert!(!app.ui.dock_expanded);
    assert!(!app.ui.docking.as_ref().unwrap().contains(&"table".into()));
    app.run("window.panel", json!({"panel":"properties"})).unwrap();
    assert_eq!(app.ui.docking_flyout.as_deref(), Some("properties"), "it opens as a flyout from the collapsed dock");
    assert_eq!(app.ui.docking_raise.as_deref(), Some("properties"));
    set_iconic(&mut app, false).unwrap();
    assert_eq!(app.ui.docking_flyout, None, "expanding the column closes its flyout");
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
    let source = h.ctx.read_response(tab_id("properties")).unwrap().rect.center();
    h.event(Event::PointerMoved(source));
    h.run_steps(1);
    h.event(Event::PointerButton { pos: source, button: PointerButton::Primary, pressed: true, modifiers: Modifiers::NONE });
    h.run_steps(1);
    h.event(Event::PointerMoved(source - vec2(90.0, 0.0)));
    h.run_steps(2);
    assert_eq!(h.ctx.dragged_id(), Some(tab_id("properties")), "test must start the owned tab gesture");
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
        assert_eq!(showing(&h.state().app, "table"), visible);
        assert!(h.state().app.ui.docking.as_ref().unwrap().contains(&"table".into()), "Table stays in its icon group");
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
    let tab = tab_id("properties");
    let from = h.ctx.read_response(tab).unwrap().rect.center();
    h.event(Event::PointerMoved(from));
    h.run_steps(1);
    h.event(Event::PointerButton { pos: from, button: PointerButton::Primary, pressed: true, modifiers: Modifiers::NONE });
    h.run_steps(1);
    h.event(Event::PointerMoved(from - vec2(90.0, 0.0)));
    h.run_steps(2);
    assert_eq!(h.ctx.dragged_id(), Some(tab), "must own a real tab drag before opening editor");
    assert!(h.ctx.memory(|m| m.area_rect(egui::Id::unique("story_editor"))).is_none());
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
    let measured = h.ctx.memory(|m| m.area_rect(egui::Id::unique("story_editor"))).unwrap();
    assert!(!measured.contains(release), "fixture exercises the conservative first-frame guard outside eventual editor bounds");
    assert_eq!(h.state().app.ui.docking, layout, "first editor frame must block release before it has a measured rectangle");
    assert_eq!(serde_json::to_value(&h.state().app.session.active().unwrap().doc).unwrap(), doc);
    h.run_steps(3);
    let editor = h.ctx.memory(|m| m.area_rect(egui::Id::unique("story_editor"))).unwrap();
    let screen = h.ctx.content_rect();
    let outside = [screen.left_bottom() + vec2(80.0, -80.0), screen.right_bottom() + vec2(-450.0, -80.0)]
        .into_iter()
        .find(|p| !editor.contains(*p))
        .expect("fixture has space outside editor");
    let before_outside = h.state().app.ui.docking.clone();
    let tab = tab_id("layers");
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
fn dock_menu_button_opens_the_active_panel_menu() {
    let mut app = app();
    app.run("file.new", json!({})).unwrap();
    app.ui.dock_tab = "pages".into();
    let pages = app.session.active().unwrap().doc.page_count();
    let mut h = harness(app, vec2(1280.0, 800.0));
    assert!(h.state().ui.docking.is_none());
    let tab = h.query_all_by_label("Pages").find(|node| node.rect().height() <= 45.0).unwrap().rect();
    // The outer dock column is at the window's right edge; the ≡ ends its tab strip.
    let at = egui::pos2(1280.0 - 12.0, tab.center().y);
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
    // A panel chosen from Window opens as a flyout beside its icon; the dock stays collapsed.
    app.run("window.panel", json!({"panel":"pages"})).unwrap();
    assert_eq!(app.ui.docking_flyout.as_deref(), Some("pages"));
    assert_eq!(app.ui.docking_raise.as_deref(), Some("pages"));
    assert!(showing(&app, "pages"));
    assert!(in_iconic_column(app.ui.docking.as_ref().unwrap(), "pages"));
    // Choosing it again closes the flyout, and a third time opens it again.
    app.run("window.panel", json!({"panel":"pages"})).unwrap();
    assert_eq!(app.ui.docking_flyout, None);
    assert!(!showing(&app, "pages"));
    app.run("window.panel", json!({"panel":"pages"})).unwrap();
    assert_eq!(app.ui.docking_flyout.as_deref(), Some("pages"));
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
    // The column bar's » button is also labelled Collapse to Icons.
    for item in ["Close", "Close Tab Group", "Minimize", "Collapse to Icons", "Float Panel"] {
        assert!(h.query_all_by_label(item).next().is_some(), "{item}");
    }
    assert_eq!(h.query_all_by_label("Minimize").count(), 1, "the tab menu lists Minimize once");
    assert!(h.query_all_by_label("Dock Panel").next().is_none(), "a docked panel has no Dock Panel");
    h.get_by_label("Close").click();
    h.run_steps(2);
    assert!(!h.state().ui.docking.as_ref().unwrap().contains(&"layers".to_string()));
}

/// A floating panel's tab menu and three-line menu have Dock Panel, which puts it back where it
/// floated from.
#[test]
fn dock_panel_in_a_floating_panels_menus_returns_it_to_the_dock() {
    let mut app = app();
    app.run("window.panel.activate", json!({"panel":"layers"})).unwrap();
    let docked = app.ui.docking.clone().unwrap();
    app.run("window.panel.float", json!({"panel":"layers","x":200,"y":150})).unwrap();
    let mut h = harness(app, vec2(1280.0, 800.0));
    let tab = h.ctx.read_response(tab_id("layers")).unwrap().rect;
    click(&mut h, tab.center(), PointerButton::Secondary);
    h.get_by_label("Dock Panel").click();
    h.run_steps(3);
    assert_eq!(h.state().ui.docking.as_ref(), Some(&docked), "the tab menu docks it back in its group");
    let mut app = h.into_state();
    app.run("window.panel.float", json!({"panel":"layers","x":200,"y":150})).unwrap();
    let mut menu = Harness::builder().with_size(vec2(300.0, 400.0)).build_ui_state(|ui, app: &mut DesignApp| panel_menu(app, ui, "layers"), app);
    menu.run_steps(1);
    menu.get_by_label("Dock Panel").click();
    menu.run_steps(2);
    assert_eq!(menu.state().ui.docking.as_ref(), Some(&docked), "so does the three-line menu");
}

/// Window › Styles › Paragraph Styles opens InDesign's floating Character Styles | Paragraph Styles
/// group, which stays open while other panels open (Learn Skills Daily's InDesign course, 7:55).
#[test]
fn paragraph_styles_open_floating_with_character_styles() {
    let mut app = app();
    app.run("window.panel", json!({"panel":"paragraphStyles"})).unwrap();
    let layout = app.ui.docking.clone().unwrap();
    let group = layout.floating.iter().flat_map(window_groups).find(|g| g.panels.iter().any(|p| p == "paragraphStyles")).expect("a floating group");
    assert_eq!(group.panels, ["characterStyles", "paragraphStyles"]);
    assert_eq!(group.panels[group.active], "paragraphStyles", "the one chosen is in front");
    // The dock still shows Properties in front.
    assert_eq!(layout.panel_state(&"properties".to_string()), craft_ui::docking::PanelState::Front, "{:?}", layout.root);
    assert_eq!(layout.floating.len(), 1, "one floating window holds both tabs");
    // Other panels from Window leave it open.
    app.run("window.panel", json!({"panel":"color"})).unwrap();
    app.run("window.panel", json!({"panel":"links"})).unwrap();
    let layout = app.ui.docking.clone().unwrap();
    assert!(layout.floating.iter().flat_map(window_groups).any(|g| g.panels == ["characterStyles", "paragraphStyles"]));
    // Once showing, Window › Character Styles brings that tab forward.
    app.run("window.panel", json!({"panel":"characterStyles"})).unwrap();
    let layout = app.ui.docking.clone().unwrap();
    let group = layout.floating.iter().flat_map(window_groups).find(|g| g.panels.iter().any(|p| p == "characterStyles")).unwrap();
    assert_eq!(group.panels[group.active], "characterStyles");
    assert_eq!(layout.panels().iter().filter(|p| p.as_str() == "characterStyles").count(), 1);
}

/// Window › Styles floats the styles icon group out of the iconic column; Dock Panel puts the
/// group back as one, at the same place in that column.
#[test]
fn dock_panel_returns_the_floated_styles_group_to_its_icon_group() {
    let mut app = app();
    let docked = app.ui.docking.clone().unwrap_or_else(|| default_layout(&app.ui));
    let styles = ["characterStyles", "paragraphStyles"].map(String::from).to_vec();
    let icon_groups = |layout: &Layout<String>| -> Vec<Vec<String>> {
        dock_columns(layout).into_iter().filter(|c| c.iconic).flat_map(|c| &c.groups).map(|g| g.panels.clone()).collect()
    };
    let before = icon_groups(&docked);
    let at = before.iter().position(|g| *g == styles).expect("the styles are an icon group");
    app.run("window.panel", json!({"panel":"paragraphStyles"})).unwrap();
    let layout = app.ui.docking.clone().unwrap();
    assert!(layout.floating.iter().flat_map(window_groups).any(|g| g.panels == styles), "{:?}", layout.floating);
    assert!(!icon_groups(&layout).contains(&styles));
    app.run("window.panel.dock", json!({"panel":"paragraphStyles"})).unwrap();
    let layout = app.ui.docking.clone().unwrap();
    assert!(layout.floating.is_empty(), "{:?}", layout.floating);
    assert_eq!(icon_groups(&layout), before, "both styles are back in their icon group, at the same place");
    assert_eq!(icon_groups(&layout).get(at), Some(&styles));
    assert!(valid(&layout));
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
    assert!(layout.floating.iter().flat_map(window_groups).all(|g| !g.panels.iter().any(|p| p == "paragraphStyles")), "{:?}", layout.floating);
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
/// keeps its three-line panel menu, and its tab menu lists InDesign's tab commands.
#[test]
fn pages_keeps_its_panel_menu_in_the_docking_layout() {
    let mut app = app();
    app.run("file.new", json!({})).unwrap();
    app.run("window.panel", json!({"panel":"paragraphStyles"})).unwrap();
    app.run("window.panel", json!({"panel":"pages"})).unwrap();
    assert!(app.ui.docking.is_some());
    let mut h = harness(app, vec2(1280.0, 800.0));
    let tab = h.query_all_by_label("Pages").find(|node| node.rect().height() <= 45.0).unwrap().rect();
    click(&mut h, egui::pos2(1280.0 - 12.0, tab.center().y), PointerButton::Primary);
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

fn group_with(layout: &Layout<String>, panel: &str) -> (bool, Vec<String>, usize) {
    if let Some(g) = layout.floating.iter().flat_map(window_groups).find(|g| g.panels.iter().any(|p| p == panel)) {
        return (true, g.panels, g.active);
    }
    let mut visit: Vec<&Node<String>> = layout.root.iter().collect();
    while let Some(node) = visit.pop() {
        match node {
            Node::Split { first, second, .. } => visit.extend([&**first, &**second]),
            Node::Tabs { panels, active } if panels.iter().any(|p| p == panel) => return (false, panels.clone(), *active),
            Node::Dock { columns, .. } => {
                if let Some(g) = columns.iter().flat_map(|c| &c.groups).find(|g| g.panels.iter().any(|p| p == panel)) {
                    return (false, g.panels.clone(), g.active);
                }
            }
            _ => {}
        }
    }
    panic!("{panel} is not in {layout:?}");
}

fn drag_bar(h: &Harness<'static, DesignApp>, first: &str) -> egui::Rect {
    h.ctx.read_response(hit_id(&craft_ui::docking::Hit::DragBar(first.to_string()))).expect("the group's drag bar").rect
}

/// The docked Character Styles | Paragraph Styles group moves as one by its drag bar: dropped on
/// the canvas it floats, dropped on another strip it joins as tabs. Order and the tab in front
/// are kept.
#[test]
fn dragging_a_docked_group_by_its_bar_moves_every_tab() {
    let mut app = app();
    app.run("window.panel", json!({"panel":"paragraphStyles"})).unwrap();
    let below = json!({"MoveGroup": {"panel": "characterStyles", "to": {"At": {"anchor": "properties", "placement": {"Group": {"below": true}}}}}});
    app.run("window.panel.layout", json!({"action": below})).unwrap();
    let styles = ["characterStyles", "paragraphStyles"];
    assert_eq!(group_with(app.ui.docking.as_ref().unwrap(), "characterStyles"), (false, styles.map(String::from).to_vec(), 1));

    let mut h = harness(app, vec2(1280.0, 800.0));
    let bar = drag_bar(&h, "characterStyles");
    drag(&mut h, bar.center(), egui::pos2(400.0, 400.0));
    let layout = h.state().ui.docking.as_ref().unwrap();
    assert_eq!(group_with(layout, "paragraphStyles"), (true, styles.map(String::from).to_vec(), 1), "{layout:?}");
    assert!(valid(layout));

    let bar = drag_bar(&h, "characterStyles");
    let properties = h.ctx.read_response(tab_id("properties")).unwrap().rect.center();
    drag(&mut h, bar.center(), properties);
    let layout = h.state().ui.docking.as_ref().unwrap();
    let (floating, panels, active) = group_with(layout, "characterStyles");
    assert!(!floating, "{layout:?}");
    let at = panels.iter().position(|p| p == "characterStyles").unwrap();
    assert_eq!(panels.get(at + 1).map(String::as_str), Some("paragraphStyles"), "{panels:?}");
    assert_eq!(panels[active], "paragraphStyles");
    assert!(layout.floating.is_empty(), "{:?}", layout.floating);
}

fn dock_of(layout: &Layout<String>) -> (Side, &[Column<String>]) {
    let Some(Node::Split { first, second, .. }) = &layout.root else { panic!("root is not a split: {layout:?}") };
    assert!(matches!(&**first, Node::Tabs { panels, .. } if panels == &[CANVAS.to_string()]), "{layout:?}");
    let Node::Dock { side, columns } = &**second else { panic!("second is not a dock: {layout:?}") };
    (*side, columns)
}

#[test]
fn default_workspaces_are_the_canvas_beside_an_end_dock() {
    let mut app = app();
    let layout = default_layout(&app.ui);
    assert!(valid(&layout));
    let (side, columns) = dock_of(&layout);
    assert_eq!(side, Side::End);
    let [outer, icons] = columns else { panic!("{columns:?}") };
    assert!(!outer.iconic);
    assert_eq!(outer.groups.len(), 1);
    assert_eq!(outer.groups[0].panels, ["properties", "pages", "layers"]);
    assert!(icons.iconic);
    let groups: Vec<Vec<&str>> = icons.groups.iter().map(|g| g.panels.iter().map(String::as_str).collect()).collect();
    let expected: Vec<Vec<&str>> = ICON_GROUPS.iter().map(|g| g.to_vec()).collect();
    assert_eq!(groups, expected, "the iconic column groups InDesign's panel families");
    // Essentials Classic: only its named icon groups, in one iconic column.
    app.run("window.workspace", json!({"name": "Essentials Classic"})).unwrap();
    let layout = default_layout(&app.ui);
    assert!(valid(&layout));
    let (_, columns) = dock_of(&layout);
    let [column] = columns else { panic!("{columns:?}") };
    assert!(column.iconic);
    let expected: Vec<Vec<String>> = app.ui.icon_groups.clone().unwrap();
    let groups: Vec<Vec<String>> = column.groups.iter().map(|g| g.panels.clone()).collect();
    assert_eq!(groups.len(), expected.len());
    assert!(groups.iter().flatten().all(|p| expected.iter().flatten().any(|e| e == p)));
}

/// Every dock panel that starts in the iconic column has one place in [`ICON_GROUPS`], so a new
/// panel is not left as a group of its own.
#[test]
fn every_icon_panel_is_in_one_default_icon_group() {
    for (id, _, _) in crate::dock::ICON_PANELS {
        assert_eq!(ICON_GROUPS.iter().flat_map(|g| g.iter()).filter(|p| *p == id).count(), 1, "{id}");
    }
    for id in ICON_GROUPS.iter().flat_map(|g| g.iter()) {
        assert!(crate::dock::ICON_PANELS.iter().any(|p| p.0 == *id), "{id} is not a dock panel");
    }
}

/// A panel the workspace places elsewhere leaves its icon group; a group it empties goes.
#[test]
fn placed_panels_leave_their_icon_groups() {
    let mut placed = Placed::default();
    for id in ["properties", "pages", "layers", "swatches", "effects"] {
        placed.take(id);
    }
    let column = icon_column(None, &mut placed).unwrap();
    let groups: Vec<Vec<&str>> = column.groups.iter().map(|g| g.panels.iter().map(String::as_str).collect()).collect();
    assert_eq!(groups.first().unwrap(), &["stroke", "color", "gradient"]);
    assert_eq!(groups.get(1).unwrap(), &["characterStyles", "paragraphStyles"]);
    assert_eq!(groups.len(), ICON_GROUPS.len() - 1);
}

#[test]
fn the_dock_is_at_the_right_in_english_and_the_left_in_arabic() {
    let tab = |h: &Harness<'static, DesignApp>| h.ctx.read_response(tab_id("properties")).map(|r| r.rect).unwrap();
    let h = harness(app(), vec2(1280.0, 800.0));
    assert!(tab(&h).left() > 640.0, "{:?}", tab(&h));
    let mut arabic = app();
    arabic.ui.language = "ar".into();
    let h = harness(arabic, vec2(1280.0, 800.0));
    assert!(tab(&h).right() < 640.0, "{:?}", tab(&h));
}

#[test]
fn ui_docking_reports_the_layout_and_the_focused_panel() {
    let mut h = harness(app(), vec2(1280.0, 800.0));
    let ctx = h.ctx.clone();
    let docking = |app: &mut DesignApp| {
        let (req, _rx) = crate::control::ControlRequest::new("ui.docking", json!({}));
        match crate::control::handle(app, &ctx, &req) {
            crate::control::Outcome::Done(v) => v,
            crate::control::Outcome::Screenshot { .. } => panic!("ui.docking took a screenshot"),
        }
    };
    let v = docking(h.state_mut());
    assert_eq!(v["ok"], true, "{v}");
    let result = &v["result"];
    assert_eq!(result["customized"], false);
    assert_eq!(result["rightToLeft"], false);
    assert!(result["focused"].is_null());
    let layout: Layout<String> = serde_json::from_value(result["layout"].clone()).unwrap();
    assert_eq!(layout, default_layout(&h.state().ui));
    let at = h.ctx.read_response(tab_id("layers")).unwrap().rect.center();
    click(&mut h, at, PointerButton::Primary);
    let v = docking(h.state_mut());
    assert_eq!(v["result"]["focused"], "layers", "{v}");
    // Closing the focused panel releases the focus.
    h.state_mut().run("window.panel.close", json!({"panel": "layers"})).unwrap();
    let v = docking(h.state_mut());
    assert!(v["result"]["focused"].is_null(), "{v}");
    assert_eq!(v["result"]["customized"], true);
    assert_eq!(v["result"]["hidden"], json!(["layers"]));
}

/// A reset replaces the panels: none of them stays focused, opened for `ui.set closePanel` or
/// queued to raise.
#[test]
fn workspace_reset_forgets_the_focused_opened_and_raised_panel() {
    let mut app = app();
    app.run("window.panel.float", json!({"panel": "layers"})).unwrap();
    app.ui.docking_focused = Some("layers".into());
    assert_eq!(app.ui.docking_opened.as_deref(), Some("layers"));
    assert_eq!(app.ui.docking_raise.as_deref(), Some("layers"));
    app.run("window.workspace.reset", json!({})).unwrap();
    assert_eq!(app.ui.docking_focused, None);
    assert_eq!(app.ui.docking_opened, None);
    assert_eq!(app.ui.docking_raise, None);
    close_visible(&mut app).unwrap();
    assert!(layout_of(&app).contains(&"layers".to_string()), "closePanel after a reset closes nothing");
}

/// A command that moves the focused panel takes the focus away and leaves it the panel last
/// opened; one that closes it forgets both.
#[test]
fn moving_or_closing_the_focused_panel_by_command_updates_the_panel_state() {
    let mut app = app();
    app.ui.docking_focused = Some("layers".into());
    app.run("window.panel.activate", json!({"panel": "pages"})).unwrap();
    assert_eq!(app.ui.docking_focused.as_deref(), Some("layers"), "a command that leaves Layers in place keeps its focus");
    app.ui.docking_opened = Some("layers".into());
    app.run("window.panel.move", json!({"panel": "layers", "anchor": "swatches", "zone": "tab"})).unwrap();
    assert_eq!(app.ui.docking_focused, None);
    assert_eq!(app.ui.docking_opened.as_deref(), Some("layers"));
    app.ui.docking_focused = Some("layers".into());
    app.run("window.panel.close", json!({"panel": "layers"})).unwrap();
    assert_eq!(app.ui.docking_focused, None);
    assert_eq!(app.ui.docking_opened, None);
    assert_eq!(app.ui.docking_raise, None);
}

/// A divider dragged with the pointer gives the keyboard back: the next arrow key nudges the
/// selection and leaves the dock's size alone.
#[test]
fn arrow_after_dragging_a_dock_divider_nudges_the_selection_not_the_dock() {
    let mut app = app();
    app.run("file.new", json!({"width":480,"height":360})).unwrap();
    let id = app.run("frame.create", json!({"rect":[80,70,330,230],"content":"none"})).unwrap()["id"].as_u64().unwrap();
    let mut h = Harness::builder().with_size(vec2(1280.0, 800.0)).build_ui_state(
        |ui, app: &mut DesignApp| {
            app.logic(ui.ctx());
            app.ui(ui);
        },
        app,
    );
    h.run_steps(5);
    let column = craft_ui::docking::Hit::Divider(craft_ui::docking::Divider::Column("properties".to_string()));
    let divider = hits(&h).into_iter().find_map(|(hit, rect)| (hit == column).then_some(rect)).expect("the main dock column's divider");
    drag(&mut h, divider.center(), divider.center() - vec2(40.0, 0.0));
    let resized = h.state().ui.docking.clone().expect("the drag customizes the layout");
    let bounds = |h: &Harness<'static, DesignApp>| h.state().session.active().unwrap().doc.item(designcraft_doc::ItemId(id)).unwrap().bounds();
    let before = bounds(&h);
    h.event(Event::Key { key: egui::Key::ArrowRight, physical_key: None, pressed: true, repeat: false, modifiers: Modifiers::NONE });
    h.run_steps(1);
    h.event(Event::Key { key: egui::Key::ArrowRight, physical_key: None, pressed: false, repeat: false, modifiers: Modifiers::NONE });
    h.run_steps(2);
    assert!(bounds(&h).x0 > before.x0, "the arrow nudges the selection");
    assert_eq!(h.state().ui.docking.as_ref(), Some(&resized), "the arrow doesn't resize the dock");
}

fn hits(h: &Harness<'static, DesignApp>) -> Vec<(craft_ui::docking::Hit<String>, egui::Rect)> {
    craft_ui::docking::DockArea::<String>::hit_rects(&h.ctx, area_id())
}

/// `window.panel.*` ids and DesignCraft's aliases go through craft-ui's `DockCommand`.
#[test]
fn shared_panel_commands_float_dock_and_reject_atomically() {
    let mut app = app();
    let docked = default_layout(&app.ui);
    app.run("window.floatPanel", json!({"panel": "Layers", "x": 60, "y": 90})).unwrap();
    let layout = app.ui.docking.clone().unwrap();
    let window = layout.floating.iter().find(|w| w.contains(&"layers".to_string())).expect("Layers floats");
    assert_eq!(window.rect, [60.0, 90.0, 300.0, 420.0], "at the given place, at its own size");
    app.run("window.dockPanel", json!({"panel": "layers"})).unwrap();
    let layout = app.ui.docking.clone().unwrap();
    assert!(layout.floating.is_empty());
    assert_eq!(
        layout.root,
        docked.root.map(|mut root| {
            // Back in its place, in front of its group.
            if let Node::Split { second, .. } = &mut root
                && let Node::Dock { columns, .. } = &mut **second
                && let Some(group) = columns.first_mut().and_then(|c| c.groups.first_mut())
            {
                group.active = 2;
            }
            root
        }),
        "Dock Panel returns it where it floated from"
    );
    // A docked panel that shares its group leaves it for a new dock at the start edge.
    let mut tabbed = self::app();
    tabbed.run("window.panel.dock", json!({"panel": "layers", "side": "Start"})).unwrap();
    let layout = tabbed.ui.docking.clone().unwrap();
    assert_eq!(tab_group(&layout, "pages"), ["properties", "pages"]);
    assert!(matches!(&layout.root, Some(Node::Split { first, .. }) if matches!(&**first, Node::Dock { side: Side::Start, .. })));
    assert_eq!(tab_group(&layout, "layers"), ["layers"]);
    // A whole group, by any of its panels, then into a new column of the end dock.
    app.run("window.panel.float", json!({"panel": "pages", "group": true, "x": 80, "y": 80, "width": 300, "height": 400})).unwrap();
    let layout = app.ui.docking.clone().unwrap();
    assert_eq!(layout.floating.len(), 1);
    assert_eq!(window_groups(&layout.floating[0])[0].panels, ["properties", "pages", "layers"]);
    app.run("window.panel.dock", json!({"panel": "pages", "group": true, "side": "End"})).unwrap();
    let layout = app.ui.docking.clone().unwrap();
    assert!(layout.floating.is_empty());
    assert_eq!(tab_group(&layout, "pages"), ["properties", "pages", "layers"]);
    // A lone panel to a new dock at the start edge.
    app.run("window.panel.float", json!({"panel": "stroke"})).unwrap();
    app.run("window.panel.dock", json!({"panel": "stroke", "side": "Start"})).unwrap();
    assert!(
        matches!(&app.ui.docking.as_ref().unwrap().root, Some(Node::Split { first, .. }) if matches!(&**first, Node::Dock { side: Side::Start, .. }))
    );
    // Docking it at that side again leaves it where it is.
    let before = app.ui.docking.clone();
    app.run("window.panel.dock", json!({"panel": "stroke", "side": "Start"})).unwrap();
    assert_eq!(app.ui.docking, before, "it already leads the start dock's last column");
    // Close Others and Minimize are shared commands too.
    app.run("window.panel.closeOthers", json!({"panel": "pages"})).unwrap();
    assert_eq!(tab_group(app.ui.docking.as_ref().unwrap(), "pages"), ["pages"]);
    assert!(app.ui.docking_hidden.contains_key("properties") && app.ui.docking_hidden.contains_key("layers"));
    app.run("window.panel.minimize", json!({"panel": "pages"})).unwrap();
    let location = app.ui.docking.as_ref().unwrap().location(&"pages".to_string()).unwrap();
    assert!(location.group.is_some_and(|g| g.minimized));
    // Rejected commands change nothing.
    let before = (app.ui.docking.clone(), app.ui.docking_hidden.clone());
    for (id, params) in [
        ("window.workspace.lock", json!({})),
        ("window.panel.window", json!({"panel": "pages"})),
        ("window.panel.move", json!({"panel": "stroke", "anchor": "pages", "zone": "left", "before": "pages"})),
        ("window.panel.dock", json!({"panel": "stroke", "side": "Middle"})),
        ("window.panel.dock", json!({"panel": "stroke", "zone": "left"})),
        ("window.dockPanel", json!({"panel": "properties", "zone": "bottom"})),
    ] {
        assert!(app.run(id, params).is_err(), "{id}");
        assert_eq!((app.ui.docking.clone(), app.ui.docking_hidden.clone()), before, "{id}");
    }
    // A zone is a side of an anchor; the error names the two ways to say where.
    let error = app.run("window.panel.dock", json!({"panel": "stroke", "zone": "right"})).unwrap_err();
    assert!(error.contains("anchor") && error.contains("side"), "{error}");
    // Every shared id is registered for the command list and MCP.
    for id in craft_ui::docking::DockCommand::<String>::IDS {
        assert!(crate::menus::UI_COMMANDS.iter().any(|c| c.0 == *id), "{id} is not registered");
    }
}

/// A tab released over the canvas floats there (craft-ui: the protected canvas takes no tabs or
/// splits); a lone panel released in the window's left edge band makes a new dock there.
#[test]
fn tabs_dropped_on_the_canvas_float_and_at_the_far_edge_make_a_dock() {
    let mut h = harness(app(), vec2(1280.0, 800.0));
    let tab = |h: &Harness<'static, DesignApp>, panel: &str| {
        hits(h).into_iter().find(|(hit, _)| *hit == craft_ui::docking::Hit::Tab(panel.into())).map(|(_, r)| r)
    };
    let layers = tab(&h, "layers").expect("the Layers tab");
    drag(&mut h, layers.center(), egui::pos2(500.0, 400.0));
    let layout = h.state().ui.docking.clone().unwrap();
    assert!(layout.floating.iter().any(|w| w.contains(&"layers".to_string())), "{layout:?}");
    // A new dock takes a panel that is alone in its group: the floating Layers, by its title bar.
    // The harness's area starts 8 pt in from the window edge.
    let title = hits(&h).into_iter().find(|(hit, _)| *hit == craft_ui::docking::Hit::Title("layers".into())).expect("the title bar").1;
    drag(&mut h, title.center(), egui::pos2(12.0, 400.0));
    let layout = h.state().ui.docking.clone().unwrap();
    assert!(valid(&layout));
    let mut docks = Vec::new();
    let mut nodes: Vec<&Node<String>> = layout.root.iter().collect();
    while let Some(node) = nodes.pop() {
        match node {
            Node::Split { first, second, .. } => nodes.extend([&**first, &**second]),
            Node::Dock { side, columns } => {
                docks.push((*side, columns.iter().flat_map(|c| &c.groups).flat_map(|g| g.panels.clone()).collect::<Vec<_>>()))
            }
            _ => {}
        }
    }
    assert!(docks.iter().any(|(side, panels)| *side == Side::Start && panels == &["layers"]), "{docks:?} {:?}", layout.floating);
}

/// `ui.docking` lists where each gesture starts: tabs, drag bars and the iconic column's icons.
#[test]
fn ui_docking_reports_hit_rects() {
    let h = harness(app(), vec2(1280.0, 800.0));
    let v = crate::panel_docking::inspect(h.state(), &h.ctx);
    let rects = v["hitRects"].as_array().expect("hitRects");
    let rect_of = |hit: Value| {
        let entry = rects.iter().find(|r| r["hit"] == hit).unwrap_or_else(|| panic!("no {hit} in {rects:?}"));
        let r: Vec<f32> = serde_json::from_value(entry["rect"].clone()).unwrap();
        egui::Rect::from_min_size(egui::pos2(r[0], r[1]), vec2(r[2], r[3]))
    };
    let tab = rect_of(json!({"Tab": "pages"}));
    let bar = rect_of(json!({"DragBar": "properties"}));
    let icon = rect_of(json!({"Icon": "stroke"}));
    assert!(tab.width() > 20.0 && bar.width() >= 24.0 && icon.width() > 20.0, "{tab:?} {bar:?} {icon:?}");
    assert!((tab.center().y - bar.center().y).abs() < 1.0, "the bar is in the tab strip");
    assert!(icon.left() > 640.0, "the iconic column is at the end edge");
}

/// Clicking an icon in the iconic column opens its panel as a flyout on the canvas side; a
/// second click closes it.
#[test]
fn clicking_an_iconic_column_icon_opens_its_flyout() {
    let mut h = harness(app(), vec2(1280.0, 800.0));
    let icon = hits(&h).into_iter().find(|(hit, _)| *hit == craft_ui::docking::Hit::Icon("swatches".into())).expect("the Swatches icon").1;
    click(&mut h, icon.center(), PointerButton::Primary);
    assert_eq!(h.state().ui.docking_flyout.as_deref(), Some("swatches"));
    let tab = hits(&h).into_iter().find(|(hit, _)| *hit == craft_ui::docking::Hit::Tab("swatches".into())).expect("the flyout's tab").1;
    assert!(tab.right() <= icon.left() + 1.0, "the flyout is beside the column, on the canvas side: {tab:?} {icon:?}");
    assert!(showing(h.state(), "swatches"));
    click(&mut h, icon.center(), PointerButton::Primary);
    assert!(h.state().ui.docking_flyout.is_none());
}

/// Iconic columns paint DesignCraft's own panel icons, in the colour craft-ui passes; a panel
/// without an icon paints nothing.
#[test]
fn panel_icons_are_painted_in_code() {
    let mut app = app();
    let mut content = PanelContent { app: &mut app };
    let ctx = egui::Context::default();
    let mut paint = |panel: &str| {
        let mut painted = false;
        let mut output = ctx.run_ui(egui::RawInput::default(), |ui| {
            let painter = ui.ctx().layer_painter(egui::LayerId::background());
            let rect = egui::Rect::from_min_size(Pos2::ZERO, vec2(20.0, 20.0));
            painted = craft_ui::docking::DockContent::paint_icon(&mut content, &painter, rect, &panel.to_string(), egui::Color32::RED);
        });
        output.textures_delta.clear();
        (painted, output.shapes.len())
    };
    let (painted, shapes) = paint("pages");
    assert!(painted && shapes > 0, "{painted} {shapes}");
    assert_eq!(paint(CANVAS), (false, 0));
}

/// Every docking label in DesignCraft's wording is translated in the shared catalog's languages.
#[test]
fn docking_labels_are_translated() {
    for language in ["de", "fr", "es", "ja", "zh"] {
        for (key, english) in DOCK_LABELS {
            assert_ne!(crate::i18n::tr(language, english), *english, "{key} in {language}");
        }
    }
}

/// With DesignCraft's title bar under macOS's transparent one, a floating window saved in the
/// title-bar strip is drawn below it, where its title bar can't move the app window.
#[test]
fn a_floating_window_saved_in_the_title_bar_strip_is_drawn_below_it() {
    let mut app = app();
    app.integrated_titlebar = true;
    app.run("window.panel.float", json!({"panel":"properties"})).unwrap();
    let window = app.ui.docking.as_mut().unwrap().floating.iter_mut().find(|w| w.panels().any(|p| p == "properties")).unwrap();
    window.rect = [200.0, 0.0, 280.0, 360.0];
    let h = harness(app, egui::vec2(1400.0, 900.0));
    let drawn = h.ctx.memory(|m| m.area_rect(crate::panel_docking::floating_layer("properties").id)).unwrap();
    assert!(drawn.min.y >= crate::chrome::MAC_APP_BAR - 0.5, "{drawn:?}");
}

/// A floating window holds splits like the dock: a panel docked below another one in the
/// window splits it, the divider between them drags, and the new size is saved.
#[test]
fn a_split_inside_a_floating_window_resizes_and_is_saved() {
    let mut app = app();
    app.run("window.panel.float", json!({"panel":"swatches", "x":300, "y":120, "width":300, "height":500})).unwrap();
    app.run("window.panel.move", json!({"panel":"color", "anchor":"swatches", "zone":"bottom"})).unwrap();
    let window = |app: &DesignApp| app.ui.docking.as_ref().unwrap().floating.iter().find(|w| w.contains("swatches")).cloned().unwrap();
    let ratio = |window: &Floating<String>| match window.root {
        Node::Split { size: craft_ui::layout::SplitSize::Ratio(r), .. } => r,
        ref root => panic!("expected a split window: {root:?}"),
    };
    assert_eq!(window_groups(&window(&app)).into_iter().map(|g| g.panels).collect::<Vec<_>>(), [["swatches"], ["color"]]);
    let before = ratio(&window(&app));
    let mut h = harness(app, vec2(1400.0, 900.0));
    let divider = hits(&h)
        .into_iter()
        .find_map(|(hit, rect)| {
            matches!(&hit, craft_ui::docking::Hit::Divider(craft_ui::docking::Divider::WindowSplit(_, path)) if path.is_empty()).then_some(rect)
        })
        .expect("the window's divider");
    drag(&mut h, divider.center(), divider.center() + vec2(0.0, 60.0));
    let after = ratio(&window(h.state()));
    assert!(after > before + 0.05, "{before} -> {after}");
    let saved = serde_json::to_vec(&h.state().ui).unwrap();
    let loaded = crate::UiState::from_preferences_json(&saved).unwrap();
    assert_eq!(loaded.docking, h.state().ui.docking);
}

/// The default layout is rebuilt every frame: its floating windows keep their ids, so their
/// native windows aren't opened again each frame.
#[test]
fn default_layout_floats_keep_their_window_ids() {
    let mut app = app();
    app.ui.floating = vec![("swatches".to_string(), [10.0, 20.0])];
    let ids = |app: &DesignApp| default_layout(&app.ui).floating.iter().map(|w| w.id).collect::<Vec<_>>();
    let first = ids(&app);
    assert_eq!(first.len(), 1, "Swatches floats");
    assert_eq!(ids(&app), first);
}

#[test]
fn dialogs_about_and_quick_apply_block_native_floating_windows() {
    let mut app = app();
    assert!(!modal_open(&app));
    app.ui.dialog = Some(crate::dialogs::Dialog::new("newDocument", json!({})));
    assert!(modal_open(&app), "dialog");
    app.ui.dialog = None;
    app.run("help.about", json!({})).unwrap();
    assert!(modal_open(&app), "About");
    app.run("help.about", json!({"open": false})).unwrap();
    assert!(!modal_open(&app));
    app.ui.palette = Some(String::new());
    assert!(modal_open(&app), "Quick Apply");
    app.ui.palette = None;
    assert!(!modal_open(&app));
}

#[cfg(not(target_arch = "wasm32"))]
#[test]
fn a_document_dropped_on_a_floating_panel_window_opens() {
    #[derive(Debug)]
    struct Dropped(std::path::PathBuf);
    impl egui::DroppedFile for Dropped {
        fn path(&self) -> &std::path::Path {
            &self.0
        }
        fn bytes(&self) -> Result<Vec<u8>, String> {
            std::fs::read(&self.0).map_err(|e| e.to_string())
        }
    }
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("dropped.designcraft");
    let mut saved = app();
    saved.run("file.new", json!({})).unwrap();
    saved.run("file.save", json!({"path": path})).unwrap();

    let mut app = app();
    assert!(app.session.documents().is_empty());
    let files = craft_ui::docking::WindowFiles {
        window: 1,
        viewport: egui::ViewportId::from_hash_of("float"),
        panel: "layers".to_owned(),
        pos: None,
        hovered: Vec::new(),
        dropped: vec![std::sync::Arc::new(Dropped(path.clone())) as egui::DroppedFileHandle],
    };
    open_window_drops(&mut app, &[files]);
    assert_eq!(app.session.documents().len(), 1, "the dropped document opens");
}
