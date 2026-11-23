#![cfg(not(target_arch = "wasm32"))]

//! Properties › Transform's X/Y/W/H on the craft UI framework, in the real section of a real document, driven with
//! real pointer and key events.
//!
//! One scenario matters here: a person types a width into the framework's field, the panel's own `transform.set` runs,
//! the frame in the document is that wide, and `edit.undo` puts it back in one step. The others are the guards the
//! same section needs: a locked frame's fields take no edit, a document unit the framework's number field cannot name
//! (picas as `1p6`, ciceros) is still typed in a framework field in the app's own spelling, and the field rects are
//! this frame's only.
//!
//! Geometry is the framework's own: every field is drawn inside `inspect::record`, and the rect these tests press is
//! the element whose access node is the field's text part (the record's hit area), never a guessed percentage of the
//! panel.

use egui_kittest::kittest::Queryable as _;
use crate::DesignApp;
use crate::panels::sel_info;
use designcraft_geom::Rect;
use egui::{Event, Key, Modifiers, PointerButton, vec2};
use egui_kittest::Harness;
use serde_json::json;

/// The frame the tests transform.
///
/// The rulers are in points, so a bare number typed into a field is points (the framework shows the
/// document's own unit); the reference point is the panel's default, the top-left corner, so a width
/// leaves the left edge where it is.
fn app_with_frame() -> (DesignApp, designcraft_doc::ItemId) {
    let mut app = DesignApp::new(designcraft_engine::Session::new(), crate::Services::default());
    app.run("file.new", json!({"title": "Transform"})).expect("a new document");
    {
        let st = app.session.doc_mut().expect("a document");
        let doc = std::sync::Arc::make_mut(&mut st.doc);
        (doc.settings.horizontal_units, doc.settings.vertical_units) = (designcraft_geom::Unit::Points, designcraft_geom::Unit::Points);
    }
    let id = designcraft_doc::ItemId(app.run("frame.create", json!({"rect": [72, 72, 272, 172]})).expect("a frame")["id"].as_u64().expect("an id"));
    app.run("selection.set", json!({"ids": [id.0]})).expect("the frame is selected");
    assert_eq!(app.ui.ref_point, 0, "the panel's reference point is the top-left corner");
    (app, id)
}

/// The Properties panel on a context that has the framework's runtime.
///
/// The runtime goes in before the harness's first pass (it refuses to install inside one), exactly as the desktop
/// shell does it.
fn properties_harness(mut app: DesignApp) -> Harness<'static, DesignApp> {
    let ctx = egui::Context::default();
    app.framework_transform = true;
    app.install_framework(&ctx);
    let mut ready = false;
    let mut h = Harness::builder().with_context(ctx).with_size(vec2(320.0, 900.0)).build_ui_state(
        move |ui, app: &mut DesignApp| {
            if !ready {
                crate::theme::install_fonts(ui.ctx(), "");
                crate::theme::apply(ui.ctx(), &crate::theme::Tokens::for_brightness(Default::default()));
                ready = true;
                return;
            }
            crate::panels::properties::show(app, ui);
        },
        app,
    );
    h.input_mut().max_texture_side = Some(8192);
    h.run_steps(3);
    h
}

/// The rect of the transform field `key` from the last pass, as the framework laid it out.
fn field_rect(h: &Harness<'_, DesignApp>, key: &str) -> egui::Rect {
    h.state()
        .framework_fields
        .iter()
        .rev()
        .find(|(k, _)| k == key)
        .map(|(_, r)| *r)
        .unwrap_or_else(|| panic!("the framework's {key} field is not on screen: {:?}", h.state().framework_fields))
}

/// A click into the field at `rect`, select all, type `text`, then Return: what a person does to type a
/// value into it.
fn type_value(h: &mut Harness<'_, DesignApp>, rect: egui::Rect, text: &str) {
    let at = rect.center();
    h.event(Event::PointerMoved(at));
    h.event(Event::PointerButton { pos: at, button: PointerButton::Primary, pressed: true, modifiers: Modifiers::NONE });
    h.event(Event::PointerButton { pos: at, button: PointerButton::Primary, pressed: false, modifiers: Modifiers::NONE });
    h.run_steps(2);
    h.event(Event::Key { key: Key::A, physical_key: None, pressed: true, repeat: false, modifiers: Modifiers::COMMAND });
    for c in text.chars() {
        h.event(Event::Text(c.to_string()));
    }
    h.event(Event::Key { key: Key::Enter, physical_key: None, pressed: true, repeat: false, modifiers: Modifiers::NONE });
    h.event(Event::Key { key: Key::Enter, physical_key: None, pressed: false, repeat: false, modifiers: Modifiers::NONE });
    h.run_steps(3);
}

/// The selected frame's bounds, from the document itself.
fn bounds(h: &Harness<'_, DesignApp>, id: designcraft_doc::ItemId) -> Rect {
    h.state().session.doc().expect("a document").doc.item(id).expect("the frame").bounds()
}

/// The undo steps the document's history holds.
fn undo_steps(h: &Harness<'_, DesignApp>) -> usize {
    h.state().session.doc().expect("a document").history.undo.len()
}

#[test]
fn typing_a_width_moves_the_frame_and_one_undo_puts_it_back() {
    let (app, id) = app_with_frame();
    let mut h = properties_harness(app);
    // The panel draws the framework's four fields for the selection, at their real geometry.
    for key in ["x", "y", "width", "height"] {
        let r = field_rect(&h, key);
        assert!(r.width() > 0.0 && r.height() > 0.0, "{key}: {r:?}");
    }
    // Points are a unit the framework's number field names: the width is that field, with its own step keys.
    assert_eq!(h.get_all_by_role_and_label(egui::accesskit::Role::SpinButton, "W").count(), 1, "the width is a framework number field");
    let before = bounds(&h, id);
    assert!((before.width() - 200.0).abs() < 1e-3, "the frame starts 200 pt wide: {before:?}");
    let steps = undo_steps(&h);
    let field = field_rect(&h, "width");
    type_value(&mut h, field, "150");
    let after = bounds(&h, id);
    assert!((after.width() - 150.0).abs() < 1e-3, "the typed width reached the document: {after:?}");
    assert!((after.x0 - before.x0).abs() < 1e-6, "the left edge stays, about the reference point: {before:?} → {after:?}");
    assert_eq!(undo_steps(&h) - steps, 1, "one gesture, one undo step ({steps} → {})", undo_steps(&h));
    h.state_mut().run("edit.undo", json!({})).expect("undo");
    h.run_steps(3);
    let back = bounds(&h, id);
    assert!((back.width() - 200.0).abs() < 1e-3, "undo restores the frame: {back:?}");
    assert_eq!(undo_steps(&h), steps, "and takes the step back");
}

#[test]
fn a_locked_frames_fields_take_no_edit() {
    let (mut app, id) = app_with_frame();
    app.run("object.lock", json!({"ids": [id.0]})).expect("lock");
    // Lock deselects, as Cmd+L does; a person then clicks the locked frame again.
    app.run("selection.set", json!({"ids": [id.0]})).expect("the locked frame is selected again");
    assert!(sel_info(&app).expect("a selection").locked, "the selection is locked");
    let mut h = properties_harness(app);
    // The fields are still drawn (the person sees where the frame is) but they are disabled.
    let rect = field_rect(&h, "width");
    let before = bounds(&h, id);
    let steps = undo_steps(&h);
    type_value(&mut h, rect, "150");
    assert_eq!(bounds(&h, id), before, "a locked frame does not move");
    assert_eq!(undo_steps(&h), steps, "and no undo step is pushed");
    // The disabled field is the guard: the command itself would move the locked frame.
    h.state_mut().run("transform.set", json!({"width": 150.0, "ref": 0})).expect("transform.set");
    assert!((bounds(&h, id).width() - 150.0).abs() < 1e-3, "transform.set moves a locked frame, so the UI guard is what held");
}

/// Successor of the test that kept the native fields for such a unit: the framework's text field over the app's own
/// measure spelling now draws them, and what is typed there is read as the rest of the app reads it.
#[test]
fn a_unit_the_number_field_cannot_name_is_typed_in_the_apps_own_spelling() {
    let (mut app, id) = app_with_frame();
    {
        let st = app.session.doc_mut().expect("a document");
        let doc = std::sync::Arc::make_mut(&mut st.doc);
        doc.settings.horizontal_units = designcraft_geom::Unit::Ciceros;
    }
    let mut h = properties_harness(app);
    assert_eq!(h.state().framework_fields.len(), 4, "the framework draws the four fields for ciceros too: {:?}", h.state().framework_fields);
    // A cicero has no kind in the framework's number field: the width is the framework's text field instead.
    assert_eq!(h.get_all_by_role_and_label(egui::accesskit::Role::SpinButton, "W").count(), 0, "no number field for ciceros");
    assert_eq!(h.get_all_by_role_and_label(egui::accesskit::Role::TextInput, "W").count(), 1, "the width is a framework text field");
    let steps = undo_steps(&h);
    let field = field_rect(&h, "width");
    type_value(&mut h, field, "10");
    let expected = designcraft_geom::units::parse_measure("10", designcraft_geom::Unit::Ciceros).expect("ten ciceros");
    let after = bounds(&h, id);
    assert!((after.width() - expected).abs() < 1e-3, "a bare number is ciceros, as the app reads it: {after:?} (want {expected} pt)");
    assert_eq!(undo_steps(&h) - steps, 1, "one gesture, one undo step ({steps} → {})", undo_steps(&h));
}

#[test]
fn a_pica_document_reads_the_apps_pica_spelling() {
    let (mut app, id) = app_with_frame();
    {
        let st = app.session.doc_mut().expect("a document");
        let doc = std::sync::Arc::make_mut(&mut st.doc);
        (doc.settings.horizontal_units, doc.settings.vertical_units) = (designcraft_geom::Unit::Picas, designcraft_geom::Unit::Picas);
    }
    let mut h = properties_harness(app);
    let before = bounds(&h, id);
    let steps = undo_steps(&h);
    let field = field_rect(&h, "width");
    // One pica and six points: 18 pt, as every measurement field of the app reads it.
    type_value(&mut h, field, "1p6");
    let after = bounds(&h, id);
    assert!((after.width() - 18.0).abs() < 1e-3, "`1p6` is 18 pt: {after:?}");
    assert!((after.x0 - before.x0).abs() < 1e-6, "the left edge stays, about the reference point: {before:?} → {after:?}");
    assert_eq!(undo_steps(&h) - steps, 1, "one gesture, one undo step ({steps} → {})", undo_steps(&h));
}

#[test]
fn constrain_links_width_and_height_but_not_x_or_y() {
    let (app, id) = app_with_frame();
    let mut h = properties_harness(app);
    // The panel's own link toggle, switched on the way the panel stores it.
    h.ctx.data_mut(|d| d.insert_temp(egui::Id::unique("constrain_wh"), true));
    h.run_steps(2);
    let before = bounds(&h, id);
    // X moves the frame and leaves both sides alone, even with the link on.
    let x = field_rect(&h, "x");
    type_value(&mut h, x, "100");
    let moved = bounds(&h, id);
    assert!((moved.x0 - 100.0).abs() < 1e-3, "X reached the document: {moved:?}");
    assert!(
        (moved.width() - before.width()).abs() < 1e-3 && (moved.height() - before.height()).abs() < 1e-3,
        "X kept the size: {before:?} → {moved:?}"
    );
    // W keeps the proportions: 200x100 at width 100 is 100x50.
    let w = field_rect(&h, "width");
    type_value(&mut h, w, "100");
    let scaled = bounds(&h, id);
    assert!((scaled.width() - 100.0).abs() < 1e-3 && (scaled.height() - 50.0).abs() < 1e-3, "the link scaled the height: {scaled:?}");
}

/// The whole window, as the desktop shell runs it each frame (`logic` then `ui`), with the framework's runtime.
fn window_harness(mut app: DesignApp) -> Harness<'static, DesignApp> {
    let ctx = egui::Context::default();
    app.framework_transform = true;
    app.install_framework(&ctx);
    let mut h = Harness::builder().with_context(ctx).with_size(vec2(1440.0, 900.0)).build_ui_state(
        |ui, app: &mut DesignApp| {
            let ctx = ui.ctx().clone();
            app.logic(&ctx);
            app.ui(ui);
        },
        app,
    );
    h.input_mut().max_texture_side = Some(8192);
    h.run_steps(6);
    h
}

#[test]
fn the_field_rects_are_this_frames_and_go_when_the_fields_do() {
    let (app, _id) = app_with_frame();
    let mut h = window_harness(app);
    assert_eq!(h.state().framework_fields.len(), 4, "the four fields are on screen: {:?}", h.state().framework_fields);
    // Picas: the fields change from number fields to text fields over the app's spelling, and no rect from the frames
    // before stays beside this frame's four.
    h.state_mut().run("document.preferences", json!({"horizontalUnits": "picas", "verticalUnits": "picas"})).expect("picas");
    h.run_steps(3);
    assert_eq!(h.state().framework_fields.len(), 4, "this frame's four fields only, after the change of field: {:?}", h.state().framework_fields);
    // Back to points, then nothing selected: the Transform section is gone, and so are the rects.
    h.state_mut().run("document.preferences", json!({"horizontalUnits": "points", "verticalUnits": "points"})).expect("points");
    h.run_steps(3);
    assert_eq!(h.state().framework_fields.len(), 4, "the fields are there with points");
    h.state_mut().run("edit.deselectAll", json!({})).expect("deselect");
    h.run_steps(3);
    assert!(h.state().framework_fields.is_empty(), "stale rects with no selection: {:?}", h.state().framework_fields);
}
