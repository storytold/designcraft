use super::*;

fn app() -> DesignApp {
    let mut app = DesignApp::new(designcraft_engine::Session::new(), crate::Services::default());
    app.run("app.newDocumentDialog", json!({})).unwrap();
    app
}

#[test]
fn card_sizes_follow_the_dialog_units() {
    let card = NewDocument::from_preset("Business Card").unwrap();
    assert_eq!(size_text(&card, Unit::Millimeters), "88.9 × 50.8 mm");
    assert_ne!(size_text(&card, Unit::Millimeters), size_text(&card, Unit::Picas));
}

#[test]
fn units_reformat_the_measures_and_linked_edges_follow() {
    // InDesign's Units list.
    assert_eq!(NEW_DOCUMENT_UNITS.map(|u| u.label()), ["Points", "Picas", "Inches", "Millimeters", "Centimeters", "Ciceros", "Agates", "Pixels"]);
    let mut d = Dialog::new("newDocument", json!({}));
    set_units(&mut d, Unit::Millimeters);
    assert_eq!((d.s("width"), d.s("marginTop")), ("215.9 mm".into(), "12.7 mm".into()));
    d.fields.insert("marginInside".into(), json!("20 mm"));
    link_edges(&mut d, "margin", "marginInside");
    assert!(EDGES.iter().all(|e| d.s(&format!("margin{e}")) == "20 mm"));
    let p = params(&d);
    assert!((p["margins"]["outside"].as_f64().unwrap() - 20.0 * PT_PER_MM).abs() < 1e-9);
    // Typed nonsense falls back to the defaults instead of reaching the document.
    d.fields.insert("gutter".into(), json!("wide"));
    d.fields.insert("pages".into(), json!("-4"));
    let p = params(&d);
    assert_eq!((p["gutter"].as_f64(), p["pages"].as_u64()), (Some(12.0), Some(1)));
}

/// The app's window (with a document) with the New Document dialog and its Save Document Preset
/// popup open.
fn window_with_preset_popup() -> egui_kittest::Harness<'static, crate::test_window::Window> {
    use egui_kittest::kittest::Queryable as _;
    let mut app = DesignApp::new(designcraft_engine::Session::new(), crate::Services::default());
    app.run("app.newDocumentDialog", json!({})).unwrap();
    let mut h = crate::test_window::open(app, vec2(1440.0, 900.0));
    // The dialog takes a couple of frames to settle where it opens.
    h.run_steps(4);
    let button = h.get_by_label("Save Document Preset").rect().center();
    crate::test_window::click_at(&mut h, button);
    assert!(egui::Popup::is_any_open(&h.ctx), "the popup opened");
    h
}

#[test]
fn enter_in_the_preset_popup_saves_the_preset_and_keeps_the_dialog() {
    use egui_kittest::kittest::Queryable as _;
    let mut h = window_with_preset_popup();
    let field = h.get_by_label("Preset Name").rect().center();
    crate::test_window::click_at(&mut h, field);
    h.get_by_label("Preset Name").type_text("Brochure");
    h.run_steps(2);
    h.key_press(egui::Key::Enter);
    h.run_steps(4);
    let app = &mut h.state_mut().app;
    let d = app.ui.dialog.as_ref().expect("Enter saves the preset; it doesn't create the document");
    assert_eq!(app.session.documents().len(), 1, "no document was made (the window opens with one)");
    assert_eq!(app.session.prefs.document_presets.iter().map(|p| p.title.as_str()).collect::<Vec<_>>(), ["Brochure"]);
    assert_eq!((d.s("tab"), d.s("preset")), ("saved".into(), "saved:Brochure".into()));
    // The saved preset makes documents by name, as file.new {preset} does from anywhere.
    app.run("file.new", json!({"preset": "Brochure"})).unwrap();
    assert!(!egui::Popup::is_any_open(&h.ctx), "the popup closed");
}

#[test]
fn saved_preset_delete_buttons_keep_the_preview_and_never_create_a_document() {
    use egui_kittest::kittest::Queryable as _;
    let mut app = DesignApp::new(designcraft_engine::Session::new(), crate::Services::default());
    app.run("file.savePreset", json!({"name":"Portrait draft","pages":3,"facingPages":false})).unwrap();
    app.run("file.savePreset", json!({"name":"Landscape draft","width":792,"height":612,"pages":2})).unwrap();
    app.run("app.newDocumentDialog", json!({})).unwrap();
    let mut h = crate::test_window::open(app, vec2(1440.0, 900.0));
    h.run_steps(4);
    let saved = h.get_by_label("Saved").rect().center();
    crate::test_window::click_at(&mut h, saved);
    let portrait = h.get_by_label("Portrait draft").rect().center();
    crate::test_window::click_at(&mut h, portrait);
    h.get_by_label("Preview").click();
    h.run_steps(20);
    assert!(h.query_by_label("Delete Preset: Portrait draft").is_some(), "the selected preset has its action");
    assert!(h.query_by_label("Delete Preset: [Default]").is_none(), "the factory card cannot be deleted");
    let before = h.state().app.session.active().unwrap().doc.clone();
    let preview = h.state().app.session.new_document_preview();
    let count = h.state().app.session.documents().len();
    let fields = params(h.state().app.ui.dialog.as_ref().unwrap());
    let landscape = h.get_by_label("Landscape draft").rect().center();
    h.event(egui::Event::PointerMoved(landscape));
    h.run_steps(3);
    let remove = h.get_by_label("Delete Preset: Landscape draft").rect().center();
    crate::test_window::click_at(&mut h, remove);
    assert_eq!(h.state().app.session.prefs.document_presets.iter().map(|p| p.title.as_str()).collect::<Vec<_>>(), ["Portrait draft"]);
    let remove = h.get_by_label("Delete Preset: Portrait draft").rect().center();
    crate::test_window::click_at(&mut h, remove);
    let app = &h.state().app;
    assert!(app.session.prefs.document_presets.is_empty());
    assert_eq!(app.session.new_document_preview(), preview);
    assert_eq!(app.session.documents().len(), count);
    assert_eq!(*app.session.active().unwrap().doc, *before);
    assert_eq!(params(app.ui.dialog.as_ref().unwrap()), fields, "deletion does not select another preset or create the preview");
}

#[test]
fn escape_in_the_preset_popup_closes_only_the_popup() {
    let mut h = window_with_preset_popup();
    h.key_press(egui::Key::Escape);
    h.run_steps(4);
    assert!(!egui::Popup::is_any_open(&h.ctx), "Escape closed the popup");
    assert!(h.state().app.ui.dialog.is_some(), "and left the dialog open");
    // With no popup open, Escape closes the dialog as before.
    h.key_press(egui::Key::Escape);
    h.run_steps(4);
    assert!(h.state().app.ui.dialog.is_none());
}

#[test]
fn presets_are_saved_by_name_and_recent_documents_once() {
    let mut app = app();
    let mut d = app.ui.dialog.clone().unwrap();
    save_preset(&mut app, &mut d, " Newsletter ").unwrap();
    d.fields.insert("pages".into(), json!(8));
    save_preset(&mut app, &mut d, "Newsletter").unwrap();
    assert_eq!(app.session.prefs.document_presets.len(), 1, "the same name replaces the preset");
    assert!(save_preset(&mut app, &mut d, "Letter").is_err(), "a built-in name is refused");
    assert_eq!((d.s("tab"), d.s("preset")), ("saved".into(), "saved:Newsletter".into()));
    let saved = cards(&app, "saved");
    assert_eq!(saved.len(), 2);
    assert_eq!((saved[0].key.as_str(), saved[0].nd.pages), (DEFAULT_CARD, 1), "[Default] comes first, as in InDesign");
    assert_eq!((saved[1].label.as_str(), saved[1].nd.pages), ("Newsletter", 8));
    let p = params(&d);
    remember(&mut app, &p);
    remember(&mut app, &p);
    assert_eq!(app.ui.recent_new_documents.len(), 1, "a repeat moves to the front");
}
