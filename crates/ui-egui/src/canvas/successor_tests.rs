//! Successors, on the installed framework, of the native canvas tests for the context menu: the same steps and checks in the
//! shipping window with the craft UI runtime, whose context menus are the framework's.

use egui_kittest::{
    Harness,
    kittest::{NodeT as _, Queryable},
};

use super::*;

/// Build the four-fps app harness used by canvas tests.
fn harness(app: DesignApp) -> Harness<'static, DesignApp> {
    harness_with_step(app, 1.0 / 4.0)
}

/// A window whose frames last `step_dt` seconds. Wheel motion is tested at a realistic frame
/// rate: egui ends a scroll action after 150 ms without wheel events, so quarter-second
/// frames would split every motion into separate actions.
fn harness_with_step(app: DesignApp, step_dt: f32) -> Harness<'static, DesignApp> {
    let mut h = crate::test_window::builder::<DesignApp>().with_size(vec2(1440.0, 900.0)).with_step_dt(step_dt).build_ui_state(
        |ui, app: &mut DesignApp| {
            let ctx = ui.ctx().clone();
            app.logic(&ctx);
            app.ui(ui);
        },
        app,
    );
    h.run_steps(4);
    h
}

fn right_click(h: &mut Harness<'_, DesignApp>, p: Pos2) {
    h.hover_at(p);
    h.step();
    for pressed in [true, false] {
        h.event(egui::Event::PointerButton { pos: p, button: egui::PointerButton::Secondary, pressed, modifiers: Default::default() });
    }
    h.run_steps(3);
}

/// Screen position of spread 0's point (x, y).
fn screen_of(h: &Harness<'_, DesignApp>, x: f64, y: f64) -> Pos2 {
    let app = h.state();
    let st = app.session.active().unwrap();
    let xf = Xf::new(app.canvas_rect.unwrap(), app.view().unwrap());
    xf.to_screen(CanvasLayout::new(&st.doc, st.editing_parents).xf(SpreadRef::Doc(0)) * Point::new(x, y))
}

/// A shown row of the canvas context menu, named by its label: the framework menu's rows are menu items (a check row
/// is a `MenuItemCheckBox`, its state in `toggled`; the shortcut is the row's keyboard shortcut, not part of its name).
/// A row counts only while neither it nor a node above it is hidden (a closing menu keeps its rows under a hidden root
/// while it fades), and only left of the canvas's right edge (not the Properties panel's rows).
fn menu_row<'h>(h: &'h Harness<'_, DesignApp>, label: &'h str) -> Option<egui_kittest::Node<'h>> {
    use egui_kittest::kittest::NodeT;
    let right = f64::from(h.state().canvas_rect.map_or(f32::INFINITY, |r| r.max.x));
    h.query_all_by(move |n| {
        matches!(n.role(), egui::accesskit::Role::MenuItem | egui::accesskit::Role::MenuItemCheckBox | egui::accesskit::Role::MenuItemRadio)
            && n.bounding_box().is_none_or(|b| b.x0 < right)
            && n.label().is_some_and(|l| l == label)
    })
    .find(|node| {
        let mut shown = !node.accesskit_node().is_hidden();
        let mut at = node.parent();
        while shown && let Some(parent) = at {
            shown = !parent.accesskit_node().is_hidden();
            at = parent.parent();
        }
        shown
    })
}

/// A pointer click on the context menu row `label`.
fn click_row(h: &mut Harness<'_, DesignApp>, label: &str) {
    let at = menu_row(h, label).unwrap_or_else(|| panic!("no menu row {label}")).rect().center();
    h.hover_at(at);
    h.drag_at(at);
    h.drop_at(at);
}

fn enabled(h: &Harness<'_, DesignApp>, label: &str) -> bool {
    !menu_row(h, label).unwrap_or_else(|| panic!("no menu row {label}")).accesskit_node().is_disabled()
}

fn spread_items(h: &Harness<'_, DesignApp>) -> Vec<u64> {
    h.state().session.doc().unwrap().doc.spreads[0].items.iter().map(|i| i.id.0).collect()
}

fn selected(h: &Harness<'_, DesignApp>) -> Vec<u64> {
    h.state().session.doc().unwrap().selection.items.iter().map(|i| i.0).collect()
}

/// Successor of `right_clicking_an_object_selects_it_and_its_menu_arranges_it` on the installed framework: the same steps and checks, in a window whose context has the craft UI runtime; a menu row is a shown MenuItem, MenuItemCheckBox or MenuItemRadio node.
#[test]
fn right_clicking_an_object_selects_it_and_its_menu_arranges_it() {
    let mut app = DesignApp::new(designcraft_engine::Session::new(), crate::Services::default());
    app.run("file.new", json!({})).unwrap();
    let a = app.run("frame.create", json!({"rect": [36, 36, 200, 200]})).unwrap()["id"].as_u64().unwrap();
    let b = app.run("frame.create", json!({"rect": [150, 150, 300, 300]})).unwrap()["id"].as_u64().unwrap();
    let mut h = harness(app);
    assert_eq!(selected(&h), [b]);
    let on_a = screen_of(&h, 60.0, 60.0);
    right_click(&mut h, on_a);
    assert_eq!(selected(&h), [a], "the object under the pointer is selected first");
    // An empty frame's menu: Content, but no Fitting (nothing to fit) and no text frame entries.
    assert!(menu_row(&h, "Content").is_some());
    assert!(menu_row(&h, "Fitting").is_none() && menu_row(&h, "Text Frame Options…").is_none());
    // Paste is available with nothing copied here: the system clipboard may hold another app's text.
    assert!(enabled(&h, "Copy") && enabled(&h, "Paste"));
    click_row(&mut h, "Arrange");
    h.run_steps(3);
    click_row(&mut h, "Bring to Front");
    h.run_steps(3);
    assert_eq!(spread_items(&h), [b, a]);
    assert!(menu_row(&h, "Arrange").is_none(), "choosing an entry closes the menu");
}

/// Successor of `right_clicking_empty_canvas_offers_paste_after_a_copy` on the installed framework: the same steps and checks, in a window whose context has the craft UI runtime; a menu row is a shown MenuItem, MenuItemCheckBox or MenuItemRadio node.
#[test]
fn right_clicking_empty_canvas_offers_paste_after_a_copy() {
    let mut app = DesignApp::new(designcraft_engine::Session::new(), crate::Services::default());
    app.run("file.new", json!({})).unwrap();
    let a = app.run("frame.create", json!({"rect": [36, 36, 200, 200]})).unwrap()["id"].as_u64().unwrap();
    app.run("edit.copy", json!({})).unwrap();
    app.select_tool("rectangleFrame");
    let mut h = harness(app);
    let empty = screen_of(&h, 400.0, 500.0);
    right_click(&mut h, empty);
    assert_eq!(selected(&h), [a], "a right-click on nothing keeps the selection");
    assert!(menu_row(&h, "Arrange").is_none(), "the page menu, not the object menu");
    assert!(enabled(&h, "Paste in Place"));
    assert!(
        menu_row(&h, "Show/Hide Rulers").is_some_and(|row| {
            use egui_kittest::kittest::NodeT;
            row.accesskit_node().toggled() == Some(egui::accesskit::Toggled::True)
        }),
        "toggles show their state"
    );
    // Escape closes the menu and does nothing else: the tool would deselect, the shortcuts
    // would switch to the Selection tool.
    h.key_press(egui::Key::Escape);
    h.run_steps(3);
    assert!(menu_row(&h, "Paste in Place").is_none());
    assert_eq!((h.state().session.tool_id(), selected(&h)), ("rectangleFrame", vec![a]));
    right_click(&mut h, empty);
    click_row(&mut h, "Paste");
    h.run_steps(3);
    assert_eq!(spread_items(&h).len(), 2);
}

/// Successor of `right_clicking_another_object_while_the_menu_is_open_reopens_it_there` on the installed framework: the same steps and checks, in a window whose context has the craft UI runtime; a menu row is a shown MenuItem, MenuItemCheckBox or MenuItemRadio node.
#[test]
fn right_clicking_another_object_while_the_menu_is_open_reopens_it_there() {
    let mut app = DesignApp::new(designcraft_engine::Session::new(), crate::Services::default());
    app.run("file.new", json!({})).unwrap();
    let a = app.run("frame.create", json!({"rect": [36, 36, 200, 200]})).unwrap()["id"].as_u64().unwrap();
    let b = app.run("frame.create", json!({"rect": [36, 600, 200, 760]})).unwrap()["id"].as_u64().unwrap();
    let mut h = harness(app);
    // The menu opens down from A, with the pointer on its first row (no submenu open); B is below it.
    let (on_a, on_b) = (screen_of(&h, 60.0, 60.0), screen_of(&h, 50.0, 700.0));
    right_click(&mut h, on_a);
    assert_eq!(selected(&h), [a]);
    assert!(menu_row(&h, "Arrange").is_some());
    right_click(&mut h, on_b);
    assert_eq!(selected(&h), [b]);
    assert!(menu_row(&h, "Arrange").is_some(), "the menu reopens at the new click");
}

/// Successor of `a_context_menu_in_one_pane_keeps_the_keys_from_the_other` on the installed framework: the same steps and checks, in a window whose context has the craft UI runtime; a menu row is a shown MenuItem, MenuItemCheckBox or MenuItemRadio node.
#[test]
fn a_context_menu_in_one_pane_keeps_the_keys_from_the_other() {
    let mut app = DesignApp::new(designcraft_engine::Session::new(), crate::Services::default());
    app.run("file.new", json!({})).unwrap();
    let a = app.run("frame.create", json!({"rect": [36, 36, 100, 100]})).unwrap()["id"].as_u64().unwrap();
    app.select_tool("rectangleFrame");
    app.split = true;
    let mut h = harness(app);
    // The focused pane is the left one; this point is near its right edge, away from the frame.
    let left = h.state().canvas_rect.unwrap();
    right_click(&mut h, pos2(left.max.x - 40.0, left.center().y));
    assert!(menu_row(&h, "Paste in Place").is_some());
    // Escape closes the left pane's menu before the right pane handles the keys.
    h.key_press(egui::Key::Escape);
    h.run_steps(3);
    assert!(menu_row(&h, "Paste in Place").is_none());
    assert_eq!((h.state().session.tool_id(), selected(&h)), ("rectangleFrame", vec![a]));
}

/// Successor of `right_clicking_text_keeps_the_selected_text_or_places_the_caret` on the installed framework: the same steps and checks, in a window whose context has the craft UI runtime; a menu row is a shown MenuItem, MenuItemCheckBox or MenuItemRadio node.
#[test]
fn right_clicking_text_keeps_the_selected_text_or_places_the_caret() {
    let mut app = DesignApp::new(designcraft_engine::Session::new(), crate::Services::default());
    app.run("file.new", json!({})).unwrap();
    app.run("frame.create", json!({"rect": [36, 36, 400, 200], "content": "text"})).unwrap();
    app.select_tool("type");
    app.run("text.insert", json!({"text": "Hello world"})).unwrap();
    let sid = *app.session.doc().unwrap().doc.stories.keys().next().unwrap();
    app.run("text.select", json!({"story": sid.0, "anchor": 0, "focus": 5})).unwrap();
    let mut h = harness(app);
    let text_sel = |h: &Harness<'_, DesignApp>| h.state().session.doc().unwrap().selection.text.as_ref().map(|t| (t.anchor, t.focus));
    // Inside "Hello": the selection stays.
    let inside = screen_of(&h, 45.0, 44.0);
    right_click(&mut h, inside);
    assert_eq!(text_sel(&h), Some((0, 5)));
    assert!(menu_row(&h, "Paste without Formatting").is_some(), "the text menu");
    h.key_press(egui::Key::Escape);
    h.run_steps(3);
    // Past the end of the line: the caret goes there.
    let past_end = screen_of(&h, 300.0, 44.0);
    right_click(&mut h, past_end);
    assert_eq!(text_sel(&h), Some((11, 11)));
    assert!(menu_row(&h, "Paste without Formatting").is_some());
}

/// Successor of `rows_hidden_in_the_menu_bar_are_hidden_in_context_menus` on the installed framework: the same steps and checks, in a window whose context has the craft UI runtime; a menu row is a shown MenuItem, MenuItemCheckBox or MenuItemRadio node.
#[test]
fn rows_hidden_in_the_menu_bar_are_hidden_in_context_menus() {
    let mut app = DesignApp::new(designcraft_engine::Session::new(), crate::Services::default());
    app.run("file.new", json!({})).unwrap();
    app.run("frame.create", json!({"rect": [36, 36, 200, 200]})).unwrap();
    app.ui.hidden_menu_items.push(crate::menus::menu_key("Object/Arrange", "Bring to Front"));
    let mut h = harness(app);
    let on_frame = screen_of(&h, 60.0, 60.0);
    right_click(&mut h, on_frame);
    click_row(&mut h, "Arrange");
    h.run_steps(3);
    assert!(menu_row(&h, "Bring to Front").is_none());
    assert!(menu_row(&h, "Send to Back").is_some());
    assert!(menu_row(&h, "Show All Menu Items").is_some());
}

/// One frame of the whole app with `events`; returns what it asks of the system IME.
fn ime_frame(app: &mut DesignApp, ctx: &egui::Context, events: Vec<egui::Event>) -> Option<egui::output::IMEOutput> {
    let raw = egui::RawInput { screen_rect: Some(Rect::from_min_size(Pos2::ZERO, vec2(1400.0, 900.0))), events, ..Default::default() };
    let mut out = ctx.run_ui(raw, |ui| {
        app.logic(ui.ctx());
        app.ui(ui);
    });
    out.textures_delta.clear();
    out.platform_output.ime
}

/// The Type tool's caret at the end of a new text frame holding `text`, the app's first frames
/// drawn; returns the story id.
fn typing_app(app: &mut DesignApp, ctx: &egui::Context, text: &str) -> Value {
    app.run("file.new", json!({})).unwrap();
    let f = app.session.execute("frame.create", &json!({"rect": [36, 36, 300, 200], "content": "text", "text": text})).unwrap();
    let sid = f["story"].clone();
    app.session.execute("text.select", &json!({"story": sid, "anchor": text.len(), "focus": text.len()})).unwrap();
    app.select_tool("type");
    for _ in 0..4 {
        ime_frame(app, ctx, vec![]);
    }
    sid
}

fn story_text(app: &mut DesignApp, sid: &Value) -> String {
    app.session.execute("story.get", &json!({"story": sid})).unwrap()["text"].as_str().unwrap().to_string()
}

fn preedit(t: &str, c: usize) -> egui::Event {
    egui::Event::Ime(egui::ImeEvent::Preedit { text: t.into(), active_range_chars: Some(c..c) })
}

#[test]
#[allow(deprecated)] // `ImeEvent::Enabled` / `Disabled`: egui no longer sends them, other integrations may.
fn ime_events_compose_in_the_story_with_the_candidate_window_at_the_caret() {
    let mut app = DesignApp::new(designcraft_engine::Session::new(), crate::Services::default());
    let ctx = egui::Context::default();
    let sid = typing_app(&mut app, &ctx, "曲:");
    // At a text caret the IME is allowed, its window on the caret as the canvas draws it.
    let ime = ime_frame(&mut app, &ctx, vec![]).expect("IME allowed at a text caret");
    let (top, bottom) = {
        let st = app.session.active().unwrap();
        let cs = app.session.cache.get(&st.doc, designcraft_doc::StoryId(sid.as_u64().unwrap()), None);
        let (fi, x, bl, asc, desc) = compose::caret(&cs, "曲:".len()).unwrap();
        let fid = cs.frames[fi].frame;
        let (a, _) = item_canvas_xf(&st.doc, &CanvasLayout::new(&st.doc, st.editing_parents), fid).unwrap();
        let m = a * st.doc.text_xf(st.doc.item(fid).unwrap());
        let xf = Xf::new(app.canvas_rect.unwrap(), app.view().unwrap());
        (xf.to_screen(m * Point::new(x, bl - asc)), xf.to_screen(m * Point::new(x, bl + desc)))
    };
    assert!(ime.rect.expand(1.0).contains(top) && ime.rect.expand(1.0).contains(bottom), "{:?} vs {top:?}–{bottom:?}", ime.rect);
    assert_eq!(ime.cursor_rect, ime.rect);
    assert!(!ime.should_interrupt_composition);
    assert!(app.canvas_ime(), "the web build keeps whole words typed at the caret");
    // "ni" → に → 日, with the events winit sends.
    ime_frame(&mut app, &ctx, vec![egui::Event::Ime(egui::ImeEvent::Enabled), preedit("ni", 2)]);
    assert_eq!(story_text(&mut app, &sid), "曲:ni", "marked text shows in place");
    assert!(app.session.tool_composing());
    // Keys, plain text, the clipboard and Undo (the native menu's ⌘Z) wait for the IME.
    let steps = app.session.doc().unwrap().history.undo.len();
    let key = |key, pressed| egui::Event::Key { key, physical_key: None, pressed, repeat: false, modifiers: Default::default() };
    ime_frame(
        &mut app,
        &ctx,
        vec![key(egui::Key::Backspace, true), key(egui::Key::Backspace, false), egui::Event::Text("x".into()), egui::Event::Paste("y".into())],
    );
    crate::menus::activate(&mut app, "edit.undo", &Value::Null);
    assert!(!crate::menus::enabled(&app, "edit.undo"));
    assert_eq!(story_text(&mut app, &sid), "曲:ni");
    let ime2 = ime_frame(&mut app, &ctx, vec![preedit("に", 1)]).expect("IME allowed while composing");
    assert_eq!(story_text(&mut app, &sid), "曲:に");
    assert!(ime2.rect.center().x > ime.rect.center().x, "the window follows the IME's cursor: {:?} {:?}", ime2.rect, ime.rect);
    ime_frame(&mut app, &ctx, vec![egui::Event::Ime(egui::ImeEvent::Commit("日".into())), egui::Event::Ime(egui::ImeEvent::Disabled)]);
    assert_eq!(story_text(&mut app, &sid), "曲:日");
    assert!(!app.session.tool_composing());
    assert_eq!(app.session.doc().unwrap().history.undo.len(), steps + 1, "the commit is one undo step");
    assert_eq!(app.session.journal.last(), Some(&("text.insert".to_string(), json!({"text": "日"}))));
    // After the commit, typing and Undo work again.
    ime_frame(&mut app, &ctx, vec![egui::Event::Text("!".into())]);
    assert_eq!(story_text(&mut app, &sid), "曲:日!");
    crate::menus::activate(&mut app, "edit.undo", &Value::Null);
    crate::menus::activate(&mut app, "edit.undo", &Value::Null);
    assert_eq!(story_text(&mut app, &sid), "曲:");
    // Without a text caret the IME is off: single-key tool shortcuts keep working.
    app.select_tool("selection");
    assert!(ime_frame(&mut app, &ctx, vec![]).is_none());
    assert!(!app.canvas_ime());
}

/// Successor of `enter_or_tab_in_a_panel_field_leave_the_selected_text_alone` on the installed framework: the same steps and checks, in a window whose context has the craft UI runtime; a menu row is a shown MenuItem, MenuItemCheckBox or MenuItemRadio node.
#[test]
fn enter_or_tab_in_a_panel_field_leave_the_selected_text_alone() {
    use egui_kittest::kittest::NodeT as _;
    for key in [egui::Key::Enter, egui::Key::Tab] {
        let mut app = DesignApp::new(designcraft_engine::Session::new(), crate::Services::default());
        app.run("file.new", json!({})).unwrap();
        let f = app.run("frame.create", json!({"rect": [36, 36, 300, 200], "content": "text", "text": "Hello"})).unwrap();
        app.select_tool("type");
        app.run("text.select", json!({"story": f["story"], "anchor": 1, "focus": 4})).unwrap();
        assert!(app.session.wants_text());
        let mut h = harness(app);
        // The Control panel's Font Size field, the topmost of that name (the Properties panel has one too): the
        // framework's number field, a spin button showing the size.
        let size = h
            .query_all_by_label("Font Size")
            .filter(|n| matches!(n.accesskit_node().role(), egui::accesskit::Role::SpinButton | egui::accesskit::Role::TextInput))
            .map(|n| n.rect())
            .min_by(|a, b| a.min.y.total_cmp(&b.min.y).then(a.min.x.total_cmp(&b.min.x)))
            .expect("the Control panel's Font Size field");
        let at = size.center();
        h.hover_at(at);
        h.drag_at(at);
        h.drop_at(at);
        h.run_steps(1);
        h.key_press_modifiers(egui::Modifiers::COMMAND, egui::Key::A);
        h.run_steps(1);
        h.event(egui::Event::Text("18".into()));
        h.run_steps(1);
        h.key_press(key);
        h.run_steps(3);
        let text = h.state_mut().session.execute("story.get", &json!({"story": f["story"]})).unwrap()["text"].clone();
        assert_eq!(text, "Hello", "{key:?} confirms the field and leaves the story alone");
        let size = crate::panels::text_attrs(h.state_mut()).unwrap()["chars"]["size"].clone();
        assert_eq!(size, 18.0, "{key:?} applies the typed size");
    }
}
