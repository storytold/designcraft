//! Successors, on the framework form, of the native Object Style dialog tests the wave 4 cleanup parked: a new style from the
//! selection and its options, a style that nests a paragraph style and sets text frame options, a refused OK, and bare numbers in
//! the document's ruler unit. They run the real dialog host over an installed framework (`table_layout_kit`).

use super::*;
use crate::dialogs::table_layout_kit as kit;
use egui::accesskit::Role;
use egui::vec2;
use egui_kittest::kittest::Queryable as _;
use serde_json::json;

fn app_with_frame() -> (DesignApp, u64) {
    let mut app = kit::app(1, false);
    let id = app.run("frame.create", json!({"rect": [72, 72, 172, 172], "content": "graphic"})).unwrap()["id"].as_u64().unwrap();
    app.run("selection.set", json!({"ids": [id]})).unwrap();
    (app, id)
}

fn app_with_text_frame(units: &str) -> DesignApp {
    let mut app = kit::app(1, false);
    app.run("document.preferences", json!({"horizontalUnits": units, "verticalUnits": units})).unwrap();
    let id = app.run("frame.create", json!({"rect": [36, 36, 336, 336], "content": "text", "caret": false})).unwrap()["id"].as_u64().unwrap();
    app.run("selection.set", json!({"ids": [id]})).unwrap();
    app
}

/// Whether a node is named with `prefix`.
fn shows_like(h: &egui_kittest::Harness<'static, DesignApp>, prefix: &str) -> bool {
    h.query_all_by_label_contains(prefix).next().is_some()
}

fn st<'a>(h: &'a mut egui_kittest::Harness<'static, DesignApp>) -> &'a mut DesignApp {
    h.state_mut()
}

fn ok(h: &mut egui_kittest::Harness<'static, DesignApp>) {
    kit::click(h, &[Role::Button], "OK", 0);
}

/// Successor of `a_new_object_style_from_the_selection_and_its_options` (the course's chapter 15): New Object Style from a frame with
/// rounded corners, a paper stroke and a drop shadow, named and applied to the selection; then Object Style Options with a thicker
/// stroke: the frames using the style follow.
#[test]
fn object_style_form_makes_a_style_from_the_selection_and_edits_its_options() {
    let (mut app, a) = app_with_frame();
    app.run("object.cornerOptions", json!({"shape": "rounded", "size": 12, "ids": [a]})).unwrap();
    app.run("object.stroke", json!({"swatch": "[Paper]", "weight": 2.5, "ids": [a]})).unwrap();
    app.run("object.dropShadow", json!({"on": true, "ids": [a]})).unwrap();
    open_new(&mut app);
    let mut h = kit::harness_sized(app, vec2(1400.0, 900.0));
    for t in [
        "New Object Style",
        "Style Name:",
        "Basic Attributes",
        "Fill",
        "Stroke & Corner Options",
        "Effects for:",
        "Drop Shadow",
        "Style Settings:",
        "Apply Style to Selection",
    ] {
        assert!(kit::shows(&h, t) || shows_like(&h, t), "{t}");
    }
    assert!(shows_like(&h, "Stroke: [Paper], 2.5 pt"));
    assert!(shows_like(&h, "Effects: Drop Shadow"));
    let d = st(&mut h).ui.dialog.as_mut().unwrap();
    d.fields.insert("name".into(), json!("Rounded Corner Image, White Stroke"));
    d.fields.insert("page".into(), json!("corners"));
    kit::steps(&mut h, 4);
    // Rounded is the entry the Shape picker shows: its value, not a name.
    assert!(kit::picker_shows(&h, "Shape", "Rounded"), "the corners page shows the selection's Rounded corners");
    ok(&mut h);
    assert!(h.state().ui.dialog.is_none());
    let name = "Rounded Corner Image, White Stroke";
    let doc = |h: &egui_kittest::Harness<'static, DesignApp>| h.state().session.active().unwrap().doc.clone();
    let item = |h: &egui_kittest::Harness<'static, DesignApp>, id: u64| doc(h).item(designcraft_doc::ItemId(id)).unwrap().clone();
    assert_eq!(item(&h, a).object_style, name, "applied to the selection");
    let os = doc(&h).styles.object_style(name).cloned().unwrap();
    assert!(os.corners.is_some() && os.effects.as_ref().unwrap().drop_shadow.on && os.stroke.as_ref().unwrap().weight == 2.5);

    let b = st(&mut h).run("frame.create", json!({"rect": [200, 72, 300, 172], "content": "graphic"})).unwrap()["id"].as_u64().unwrap();
    st(&mut h).run("style.object.apply", json!({"name": name, "ids": [b]})).unwrap();
    open_edit(st(&mut h), name).unwrap();
    kit::steps(&mut h, 4);
    let d = st(&mut h).ui.dialog.as_mut().unwrap();
    assert_eq!(title(d), "Object Style Options");
    set(d, &["stroke", "weight"], json!(5.0));
    set(d, &["inc", "fill"], json!(false));
    kit::steps(&mut h, 3);
    ok(&mut h);
    assert_eq!((item(&h, a).stroke.weight, item(&h, b).stroke.weight), (5.0, 5.0));
    assert!(doc(&h).styles.object_style(name).unwrap().fill.is_none(), "Fill unchecked: no longer in the style");
    assert!(doc(&h).styles.object_style(name).unwrap().effects.as_ref().unwrap().drop_shadow.on, "effects kept");
}

/// Successor of `an_object_style_nests_a_paragraph_style_and_sets_text_frame_options` (the course's chapter 39): a new object style from
/// a text frame nests the "Drop Cap First Paragraph" style, so the frame's text takes it; Object Style Options > Text Frame General
/// Options then gives the style's frames insets and two columns; a word for the gutter or a fraction of a column keeps the dialog
/// open with the reason.
#[test]
fn object_style_form_nests_a_paragraph_style_and_sets_text_frame_options() {
    let mut app = kit::app(1, false);
    app.run("style.paragraph.create", json!({"name": "Body Text"})).unwrap();
    app.run("style.paragraph.create", json!({"name": "Drop Cap First Paragraph", "nextStyle": "Body Text", "para": {"dropCapLines": 3}})).unwrap();
    let r = app.run("frame.create", json!({"rect": [36, 36, 336, 336], "content": "text", "caret": false})).unwrap();
    let id = r["id"].as_u64().unwrap();
    app.run("selection.set", json!({"ids": [id]})).unwrap();
    open_new(&mut app);
    let d = app.ui.dialog.as_ref().unwrap();
    assert!(included(d, "paragraph") && included(d, "textFrame"), "a text frame's categories are in a new style");
    assert_eq!(d.s("paragraphStyle"), "", "the paragraph style is left to choose");
    let mut h = kit::harness_sized(app, vec2(1400.0, 900.0));
    for t in ["Paragraph Styles", "Text Frame General Options"] {
        assert!(kit::shows(&h, t) || shows_like(&h, t), "{t}");
    }
    let d = st(&mut h).ui.dialog.as_mut().unwrap();
    d.fields.insert("name".into(), json!("Text Box Drop Cap"));
    d.fields.insert("page".into(), json!("paragraph"));
    d.fields.insert("paragraphStyle".into(), json!("Drop Cap First Paragraph"));
    kit::steps(&mut h, 4);
    for t in ["Paragraph Style:", "Apply Next Style"] {
        assert!(kit::shows(&h, t) || shows_like(&h, t), "{t}");
    }
    ok(&mut h);
    assert!(h.state().ui.dialog.is_none());
    let doc = |h: &egui_kittest::Harness<'static, DesignApp>| h.state().session.active().unwrap().doc.clone();
    let story = |h: &egui_kittest::Harness<'static, DesignApp>| {
        let d = doc(h);
        let sid = d.item(designcraft_doc::ItemId(id)).unwrap().text_frame().unwrap().story;
        d.story(sid).unwrap().clone()
    };
    assert_eq!(doc(&h).item(designcraft_doc::ItemId(id)).unwrap().object_style, "Text Box Drop Cap");
    assert_eq!(story(&h).paras[0].style, "Drop Cap First Paragraph", "the frame's text takes the nested style");
    st(&mut h).run("type.fillWithPlaceholder", json!({"frame": id})).unwrap();
    assert!(story(&h).paras.iter().all(|p| p.style == "Drop Cap First Paragraph"));

    open_edit(st(&mut h), "Text Box Drop Cap").unwrap();
    kit::steps(&mut h, 4);
    assert!(shows_like(&h, "Paragraph Styles: Drop Cap First Paragraph"), "Style Settings");
    let d = st(&mut h).ui.dialog.as_mut().unwrap();
    assert_eq!(d.s("paragraphStyle"), "Drop Cap First Paragraph");
    d.fields.insert("page".into(), json!("textFrame"));
    for k in ["insetTop", "insetLeft", "insetBottom", "insetRight"] {
        d.fields.insert(k.into(), json!("4 pt"));
    }
    d.fields.insert("columns".into(), json!("2"));
    kit::steps(&mut h, 4);
    for t in ["Inset Spacing", "Vertical Justification", "Ignore Text Wrap"] {
        assert!(kit::shows(&h, t) || shows_like(&h, t), "{t}");
    }
    ok(&mut h);
    assert!(h.state().ui.dialog.is_none());
    let o = doc(&h).item(designcraft_doc::ItemId(id)).unwrap().text_frame().unwrap().options.clone();
    assert_eq!((o.columns, o.inset), (2, [4.0; 4]));
    let os = doc(&h).styles.object_style("Text Box Drop Cap").cloned().unwrap();
    assert_eq!((os.paragraph_style.as_deref(), os.text_frame.map(|t| t.columns)), (Some("Drop Cap First Paragraph"), Some(2)));

    // A word for the gutter, or a fraction of a column, keeps the dialog open with the reason.
    for (field, value, reason) in [("gutter", "wide", "gutter"), ("columns", "2.5", "columns")] {
        open_edit(st(&mut h), "Text Box Drop Cap").unwrap();
        kit::steps(&mut h, 3);
        st(&mut h).ui.dialog.as_mut().unwrap().fields.insert(field.into(), json!(value));
        kit::steps(&mut h, 2);
        ok(&mut h);
        let d = h.state().ui.dialog.as_ref().expect("the dialog stays open");
        assert!(d.s("status").contains(reason), "{field}: {}", d.s("status"));
        st(&mut h).ui.dialog = None;
        kit::steps(&mut h, 2);
    }
    let os = doc(&h).styles.object_style("Text Box Drop Cap").cloned().unwrap();
    assert_eq!(os.text_frame.map(|t| t.columns), Some(2), "2.5 columns changed nothing");
}

/// Successor of `a_refused_ok_keeps_the_dialog_open_with_the_reason`: a refused OK (an empty name, a name another style has, in Object
/// Style Options and in New Object Style) keeps the dialog and its edits, with the reason shown.
#[test]
fn object_style_form_refused_ok_keeps_the_dialog_open_with_the_reason() {
    let (mut app, _) = app_with_frame();
    app.run("style.object.create", json!({"name": "Taken", "fromSelection": false})).unwrap();
    app.run("style.object.create", json!({"name": "Mine", "fromSelection": false})).unwrap();
    open_new(&mut app);
    let mut h = kit::harness_sized(app, vec2(1400.0, 900.0));
    let d = st(&mut h).ui.dialog.as_mut().unwrap();
    d.fields.insert("name".into(), json!("  "));
    set(d, &["stroke", "weight"], json!(7.0));
    kit::steps(&mut h, 3);
    ok(&mut h);
    let d = h.state().ui.dialog.as_ref().expect("still open");
    assert_eq!(d.s("status"), "Enter a style name.");
    assert_eq!(get(d, &["stroke", "weight"]), &json!(7.0), "the edits are kept");
    assert!(kit::shows(&h, "Enter a style name.") || shows_like(&h, "Enter a style name."), "the reason is shown");
    open_edit(st(&mut h), "Mine").unwrap();
    kit::steps(&mut h, 3);
    st(&mut h).ui.dialog.as_mut().unwrap().fields.insert("name".into(), json!("Taken"));
    kit::steps(&mut h, 2);
    ok(&mut h);
    let d = h.state().ui.dialog.as_ref().expect("still open");
    assert!(d.s("status").contains("already named"), "{}", d.s("status"));
    // New Object Style with a name another style has makes no style under another name.
    st(&mut h).ui.dialog = None;
    kit::steps(&mut h, 2);
    let count = |h: &egui_kittest::Harness<'static, DesignApp>| h.state().session.active().unwrap().doc.styles.object.len();
    let before = count(&h);
    open_new(st(&mut h));
    kit::steps(&mut h, 3);
    st(&mut h).ui.dialog.as_mut().unwrap().fields.insert("name".into(), json!("Taken"));
    kit::steps(&mut h, 2);
    ok(&mut h);
    let d = h.state().ui.dialog.as_ref().expect("still open");
    assert!(d.s("status").contains("already named"), "{}", d.s("status"));
    assert_eq!(count(&h), before, "no style was made");
}

/// Successor of `bare_numbers_are_in_the_documents_ruler_unit`: in a document measured in millimeters, a bare number in a Text Frame
/// General Options field is millimeters, in New Object Style and in Object Style Options.
#[test]
fn object_style_form_bare_numbers_are_in_the_documents_ruler_unit() {
    let mut app = app_with_text_frame("millimeters");
    let mm = 72.0 / 25.4;
    open_new(&mut app);
    let mut h = kit::harness_sized(app, vec2(1400.0, 900.0));
    let frame = |h: &egui_kittest::Harness<'static, DesignApp>| {
        h.state().session.active().unwrap().doc.styles.object_style("Metric").and_then(|s| s.text_frame.clone()).unwrap()
    };
    let d = st(&mut h).ui.dialog.as_mut().unwrap();
    d.fields.insert("name".into(), json!("Metric"));
    for k in ["insetTop", "insetLeft", "insetBottom", "insetRight"] {
        d.fields.insert(k.into(), json!("6"));
    }
    kit::steps(&mut h, 3);
    ok(&mut h);
    assert!(h.state().ui.dialog.is_none());
    assert!(frame(&h).inset.iter().all(|v| (v - 6.0 * mm).abs() < 1e-6), "{:?}", frame(&h).inset);
    open_edit(st(&mut h), "Metric").unwrap();
    kit::steps(&mut h, 3);
    let d = st(&mut h).ui.dialog.as_mut().unwrap();
    d.fields.insert("columns".into(), json!("2"));
    d.fields.insert("gutter".into(), json!("5"));
    kit::steps(&mut h, 3);
    ok(&mut h);
    assert!(h.state().ui.dialog.is_none());
    assert!((frame(&h).gutter - 5.0 * mm).abs() < 1e-6, "{}", frame(&h).gutter);
}
