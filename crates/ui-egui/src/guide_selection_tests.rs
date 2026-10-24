//! Ruler guides selected like objects (#1282): click, Shift-click, marquee, drag, keys, and the
//! X/Y fields.

use egui::{Event, Modifiers, PointerButton};
use egui_kittest::Harness;

use super::{
    CanvasLayout, DesignApp, GuideDrag, GuidePress, Mods, Overlay, Point, Pos2, RULER, SpreadRef, ToolKey, Xf, guide_drag_delta, guide_spot, json,
    moving_guides, pos2, vec2,
};

fn harness(app: DesignApp) -> Harness<'static, DesignApp> {
    let mut h = Harness::builder().with_size(vec2(1440.0, 900.0)).build_ui_state(
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

/// A page with vertical guides at x 200 and 300 and a horizontal one at y 400.
fn with_guides() -> Harness<'static, DesignApp> {
    let mut app = DesignApp::new(designcraft_engine::Session::new(), crate::Services::default());
    app.run("file.new", json!({})).unwrap();
    for (orientation, position) in [("vertical", 200.0), ("vertical", 300.0), ("horizontal", 400.0)] {
        app.run("guide.add", json!({"orientation": orientation, "position": position})).unwrap();
    }
    harness(app)
}

/// Screen position of spread 0's point (x, y).
fn screen_of(h: &Harness<'_, DesignApp>, x: f64, y: f64) -> Pos2 {
    let app = h.state();
    let st = app.session.active().unwrap();
    let xf = Xf::new(app.canvas_rect.unwrap(), app.view().unwrap());
    xf.to_screen(CanvasLayout::new(&st.doc, st.editing_parents).xf(SpreadRef::Doc(0)) * Point::new(x, y))
}

fn press(h: &mut Harness<'_, DesignApp>, pos: Pos2, pressed: bool, mods: Modifiers) {
    h.event(Event::PointerButton { pos, button: PointerButton::Primary, pressed, modifiers: mods });
    h.run_steps(2);
}

fn click(h: &mut Harness<'_, DesignApp>, pos: Pos2, mods: Modifiers) {
    h.event(Event::ModifiersChanged(mods));
    h.hover_at(pos);
    h.step();
    press(h, pos, true, mods);
    press(h, pos, false, mods);
    h.event(Event::ModifiersChanged(Modifiers::NONE));
    h.run_steps(2);
}

fn drag(h: &mut Harness<'_, DesignApp>, a: Pos2, b: Pos2) {
    h.hover_at(a);
    h.step();
    press(h, a, true, Modifiers::NONE);
    for k in 1..=4 {
        h.hover_at(a + (b - a) * (k as f32 / 4.0));
        h.step();
    }
    press(h, b, false, Modifiers::NONE);
    h.run_steps(2);
}

fn selected(h: &Harness<'_, DesignApp>) -> Vec<usize> {
    h.state().session.doc().unwrap().selection.guides.iter().map(|g| g.index).collect()
}

fn positions(h: &Harness<'_, DesignApp>) -> Vec<f64> {
    h.state().session.doc().unwrap().doc.spreads[0].pages[0].guides.iter().map(|g| g.position).collect()
}

#[test]
fn clicking_guides_selects_them_and_shift_click_adds_or_removes() {
    let mut h = with_guides();
    let id = h.state_mut().run("frame.create", json!({"rect": [400, 100, 500, 200]})).unwrap()["id"].as_u64().unwrap();
    h.state_mut().run("selection.set", json!({"ids": [id]})).unwrap();
    let at = screen_of(&h, 200.0, 600.0);
    click(&mut h, at, Modifiers::NONE);
    assert_eq!(selected(&h), [0]);
    assert!(h.state().session.doc().unwrap().selection.items.is_empty(), "the object is deselected");
    assert_eq!(positions(&h), [200.0, 300.0, 400.0], "a click doesn't move the guide");
    let at = screen_of(&h, 100.0, 400.0);
    click(&mut h, at, Modifiers::SHIFT);
    assert_eq!(selected(&h), [0, 2]);
    let at = screen_of(&h, 200.0, 650.0);
    click(&mut h, at, Modifiers::SHIFT);
    assert_eq!(selected(&h), [2]);
    // A click on empty canvas, or on an object, clears the guide selection.
    let at = screen_of(&h, 550.0, 650.0);
    click(&mut h, at, Modifiers::NONE);
    assert!(selected(&h).is_empty());
    let at = screen_of(&h, 300.0, 600.0);
    click(&mut h, at, Modifiers::NONE);
    assert_eq!(selected(&h), [1]);
    let at = screen_of(&h, 450.0, 150.0);
    click(&mut h, at, Modifiers::NONE);
    assert!(selected(&h).is_empty());
    assert_eq!(h.state().session.doc().unwrap().selection.items, [designcraft_doc::ItemId(id)]);
    // With View › Lock Guides on, guides can't be selected, and selected ones are deselected.
    h.state_mut().run("guide.selectAll", json!({})).unwrap();
    h.state_mut().ui.guides_locked = true;
    h.run_steps(2);
    assert!(selected(&h).is_empty());
    let at = screen_of(&h, 200.0, 600.0);
    click(&mut h, at, Modifiers::NONE);
    assert!(selected(&h).is_empty());
}

#[test]
fn dragging_a_selected_guide_moves_all_selected_guides_as_one_step() {
    let mut h = with_guides();
    h.state_mut().run("guide.select", json!({"guides": [{"page": 0, "index": 0}, {"page": 0, "index": 2}]})).unwrap();
    let undo = h.state().session.doc().unwrap().history.undo.len();
    let (a, b) = (screen_of(&h, 200.0, 600.0), screen_of(&h, 250.0, 620.0));
    drag(&mut h, a, b);
    let p = positions(&h);
    assert!((p[0] - 250.0).abs() < 1.0 && p[1] == 300.0 && (p[2] - 420.0).abs() < 1.0, "{p:?}");
    assert_eq!(h.state().session.doc().unwrap().history.undo.len(), undo + 1);
    assert_eq!(selected(&h), [0, 2]);
    h.state_mut().run("edit.undo", json!({})).unwrap();
    assert_eq!(positions(&h), [200.0, 300.0, 400.0]);
    // Dragging an unselected guide selects it and moves only it.
    let (a, b) = (screen_of(&h, 300.0, 600.0), screen_of(&h, 350.0, 600.0));
    drag(&mut h, a, b);
    assert_eq!(selected(&h), [1]);
    let p = positions(&h);
    assert!(p[0] == 200.0 && (p[1] - 350.0).abs() < 1.0 && p[2] == 400.0, "{p:?}");
}

#[test]
fn a_marquee_selects_guides_only_when_it_touches_no_object() {
    let mut h = with_guides();
    // Across both vertical guides, below the horizontal one, on empty page.
    let (a, b) = (screen_of(&h, 150.0, 500.0), screen_of(&h, 350.0, 550.0));
    drag(&mut h, a, b);
    assert_eq!(selected(&h), [0, 1]);
    let id = h.state_mut().run("frame.create", json!({"rect": [220, 520, 260, 540]})).unwrap()["id"].as_u64().unwrap();
    h.state_mut().run("edit.deselectAll", json!({})).unwrap();
    let (a, b) = (screen_of(&h, 150.0, 500.0), screen_of(&h, 350.0, 550.0));
    drag(&mut h, a, b);
    assert!(selected(&h).is_empty());
    assert_eq!(h.state().session.doc().unwrap().selection.items, [designcraft_doc::ItemId(id)]);
}

#[test]
fn keys_nudge_and_delete_selected_guides() {
    let mut h = with_guides();
    h.state_mut().run("guide.select", json!({"guides": [{"page": 0, "index": 1}, {"page": 0, "index": 2}]})).unwrap();
    let inc = h.state().session.doc().unwrap().doc.settings.keyboard_increment;
    let vi = h.state().view_info();
    let key = |h: &mut Harness<'_, DesignApp>, k: ToolKey, shift: bool| {
        let m = Mods { shift, alt: false, cmd: false, ctrl: false, space: false };
        assert!(h.state_mut().session.tool_key(k, m, vi).unwrap());
    };
    key(&mut h, ToolKey::Right, false);
    key(&mut h, ToolKey::Down, true);
    assert_eq!(positions(&h), [200.0, 300.0 + inc, 400.0 + 10.0 * inc]);
    key(&mut h, ToolKey::Delete, false);
    assert_eq!(positions(&h), [200.0]);
    h.state_mut().run("edit.undo", json!({})).unwrap();
    assert_eq!(positions(&h).len(), 3);
    assert_eq!(selected(&h), [1, 2]);
    key(&mut h, ToolKey::Escape, false);
    assert!(selected(&h).is_empty());
}

#[test]
fn the_guide_field_measures_from_the_zero_point() {
    let mut h = with_guides();
    h.state_mut().run("view.zeroPoint", json!({"at": [100, 50]})).unwrap();
    h.state_mut().run("guide.select", json!({"page": 0, "index": 0})).unwrap();
    let f = crate::panels::guide_field(h.state()).unwrap();
    assert_eq!((f.row(), f.value), (0, 100.0));
    h.state_mut().run("guide.move", f.move_to(36.0)).unwrap();
    assert_eq!(positions(&h)[0], 136.0);
    h.state_mut().run("guide.select", json!({"page": 0, "index": 2})).unwrap();
    let f = crate::panels::guide_field(h.state()).unwrap();
    assert_eq!((f.row(), f.value), (1, 350.0));
    // Several guides: no field value.
    h.state_mut().run("guide.selectAll", json!({})).unwrap();
    assert!(crate::panels::guide_field(h.state()).is_none());
}

#[test]
fn select_all_guides_is_in_the_edit_menu_on_the_spread_in_view() {
    let mut h = with_guides();
    assert_eq!(crate::menus::default_shortcut("guide.selectAll"), Some("Cmd+Alt+G"));
    crate::menus::activate(h.state_mut(), "guide.selectAll", &serde_json::Value::Null);
    assert_eq!(selected(&h), [0, 1, 2]);
    h.state_mut().ui.guides = false;
    assert!(!crate::menus::menu_enabled(h.state(), "guide.selectAll"));
}

#[test]
fn a_guide_over_a_frame_takes_the_click() {
    let mut h = with_guides();
    h.state_mut().run("guide.deselect", json!({})).unwrap();
    // The vertical guide at x 200 crosses the frame.
    let id = h.state_mut().run("frame.create", json!({"rect": [150, 500, 260, 560]})).unwrap()["id"].as_u64().unwrap();
    h.state_mut().run("edit.deselectAll", json!({})).unwrap();
    let bounds = |h: &Harness<'_, DesignApp>| h.state().session.doc().unwrap().doc.spreads[0].items[0].bounds();
    let before = bounds(&h);
    let at = screen_of(&h, 200.0, 530.0);
    click(&mut h, at, Modifiers::NONE);
    assert_eq!(selected(&h), [0]);
    assert!(h.state().session.doc().unwrap().selection.items.is_empty(), "the frame is not selected");
    // Dragging it there moves the guide, not the frame.
    let (a, b) = (screen_of(&h, 200.0, 530.0), screen_of(&h, 230.0, 530.0));
    drag(&mut h, a, b);
    assert!((positions(&h)[0] - 230.0).abs() < 1.0, "{:?}", positions(&h));
    assert_eq!(bounds(&h), before);
    assert!(h.state().session.doc().unwrap().selection.items.is_empty());
    // Away from the guide, a click inside the frame selects it.
    let at = screen_of(&h, 160.0, 530.0);
    click(&mut h, at, Modifiers::NONE);
    assert_eq!(h.state().session.doc().unwrap().selection.items, [designcraft_doc::ItemId(id)]);
    assert!(selected(&h).is_empty());
}

#[test]
fn a_guide_dragged_out_of_a_ruler_is_the_selection() {
    let mut h = with_guides();
    h.state_mut().run("guide.select", json!({"page": 0, "index": 0})).unwrap();
    let rect = h.state().canvas_rect.unwrap();
    let ruler = pos2(rect.min.x + 200.0, rect.min.y - RULER / 2.0);
    let dest = screen_of(&h, 250.0, 500.0);
    drag(&mut h, ruler, dest);
    let p = positions(&h);
    assert!(p.len() == 4 && (p[3] - 500.0).abs() < 1.0, "{p:?}");
    assert_eq!(selected(&h), [3]);
}

#[test]
fn dragging_one_of_several_selected_guides_previews_them_all() {
    let mut h = with_guides();
    h.state_mut().run("guide.select", json!({"guides": [{"page": 0, "index": 0}, {"page": 0, "index": 2}]})).unwrap();
    let (a, b) = (screen_of(&h, 200.0, 600.0), screen_of(&h, 250.0, 620.0));
    let g =
        GuideDrag { orientation: designcraft_doc::Orientation::Vertical, from: Some((SpreadRef::Doc(0), 0, 0)), at: Some(b), mods: Modifiers::NONE };
    let press = GuidePress { origin: a, shift: false };
    let app = h.state();
    let xf = Xf::new(app.canvas_rect.unwrap(), app.view().unwrap());
    let m = moving_guides(app, &xf, app.canvas_rect.unwrap(), &g, press).expect("moving");
    assert!((m.dx - 50.0).abs() < 1.0 && (m.dy - 20.0).abs() < 1.0, "{} {}", m.dx, m.dy);
    let mut moved: Vec<usize> = m.guides.iter().map(|g| g.index).collect();
    moved.sort_unstable();
    assert_eq!(moved, [0, 2]);
    // The drop moves them by the same amounts.
    let (dx, dy) = guide_drag_delta(app, &xf, &g, a).unwrap();
    assert_eq!((dx, dy), (m.dx, m.dy));
}

/// Spread points per screen pixel at the harness's view.
fn pt_per_px(h: &Harness<'_, DesignApp>) -> f64 {
    let app = h.state();
    1.0 / Xf::new(app.canvas_rect.unwrap(), app.view().unwrap()).zoom
}

/// A letter page with two columns: the left column's right side.
fn with_columns() -> (Harness<'static, DesignApp>, f64) {
    let mut app = DesignApp::new(designcraft_engine::Session::new(), crate::Services::default());
    app.run("file.new", json!({"preset": "Letter", "columns": 2, "gutter": 18, "margins": 54})).unwrap();
    app.run("guide.add", json!({"orientation": "vertical", "position": 200.0})).unwrap();
    let side = app.session.doc().unwrap().doc.spreads[0].pages[0].column_rects()[0].x1;
    (harness(app), side)
}

/// A dragged guide snaps to a column guide as a moved object does (#1221).
#[test]
fn a_dragged_guide_snaps_to_a_column_guide() {
    let (mut h, side) = with_columns();
    let near = side + 1.5 * pt_per_px(&h);
    let (a, b) = (screen_of(&h, 200.0, 600.0), screen_of(&h, near, 600.0));
    drag(&mut h, a, b);
    assert_eq!(positions(&h), [side]);
}

/// Snap to Guides and Smart Guides off: the guide goes where the pointer is.
#[test]
fn with_snapping_off_a_dragged_guide_stays_at_the_pointer() {
    let (mut h, side) = with_columns();
    h.state_mut().ui.snap_to_guides = false;
    h.state_mut().ui.smart_guides = false;
    let near = side + 1.5 * pt_per_px(&h);
    let (a, b) = (screen_of(&h, 200.0, 600.0), screen_of(&h, near, 600.0));
    drag(&mut h, a, b);
    let p = positions(&h);
    assert!((p[0] - near).abs() < 0.1 && p[0] != side, "{p:?} vs {near}");
}

/// A frame's edge catches a dragged guide, and the preview shows the alignment mark.
#[test]
fn a_dragged_guide_snaps_to_a_frame_edge_with_a_smart_guide() {
    let mut h = with_guides();
    h.state_mut().run("frame.create", json!({"rect": [400, 100, 500, 200]})).unwrap();
    h.state_mut().run("edit.deselectAll", json!({})).unwrap();
    let near = 400.0 + 1.5 * pt_per_px(&h);
    let (a, b) = (screen_of(&h, 200.0, 600.0), screen_of(&h, near, 600.0));
    let g =
        GuideDrag { orientation: designcraft_doc::Orientation::Vertical, from: Some((SpreadRef::Doc(0), 0, 0)), at: Some(b), mods: Modifiers::NONE };
    {
        let app = h.state();
        let xf = Xf::new(app.canvas_rect.unwrap(), app.view().unwrap());
        let spot = guide_spot(app, &xf, &g).unwrap();
        assert_eq!(spot.sp.x, 400.0);
        assert!(spot.marks.iter().any(|m| matches!(m, Overlay::Guide { .. })), "{:?}", spot.marks);
        // Command suspends snapping.
        let spot = guide_spot(app, &xf, &GuideDrag { mods: Modifiers::COMMAND, ..g.clone() }).unwrap();
        assert!((spot.sp.x - near).abs() < 0.1 && spot.marks.is_empty());
    }
    drag(&mut h, a, b);
    assert_eq!(positions(&h), [400.0, 300.0, 400.0]);
}

/// A new guide dragged out of a ruler snaps too: here to the horizontal guide at y 400.
#[test]
fn a_guide_dragged_out_of_a_ruler_snaps() {
    let mut h = with_guides();
    let rect = h.state().canvas_rect.unwrap();
    let ruler = pos2(rect.min.x + 200.0, rect.min.y - RULER / 2.0);
    let dest = screen_of(&h, 250.0, 400.0 + 1.5 * pt_per_px(&h));
    drag(&mut h, ruler, dest);
    assert_eq!(positions(&h), [200.0, 300.0, 400.0, 400.0]);
}
