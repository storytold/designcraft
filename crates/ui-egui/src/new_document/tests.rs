use super::*;

fn app() -> DesignApp {
    let mut app = DesignApp::new(designcraft_engine::Session::new(), crate::Services::default());
    app.run("app.newDocumentDialog", json!({})).unwrap();
    app
}

/// The preset cards' size line reads in the dialog's units.
///
/// Probe: the cards themselves are no longer sized in points — the grid draws a card's picture its thumbnail side times
/// the zoom (`form::CARD_ZOOM`), as the framework's grids do — so this test keeps to the size line under the page, and
/// the geometry is the grid's.
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

/// The dialog host over the app's window (with a document), the New Document dialog open and its Save Document Preset
/// popover open.
fn window_with_preset_popup() -> egui_kittest::Harness<'static, DesignApp> {
    use crate::dialogs::table_layout_kit as kit;
    use egui::accesskit::Role;
    let mut app = kit::app(1, false);
    app.run("app.newDocumentDialog", json!({})).unwrap();
    let mut h = kit::harness_sized(app, vec2(1440.0, 900.0));
    kit::click(&mut h, &[Role::Button], "Save Document Preset", 0);
    assert!(kit::shows(&h, "Preset Name"), "the popup opened");
    h
}

/// The centre of the preset card named `name`: its tile, whose accessible name is the name then its size
/// (`{name}, {details}`), not the details' text of the same words.
fn card_at(h: &egui_kittest::Harness<'_, DesignApp>, name: &str) -> egui::Pos2 {
    use egui::accesskit::Role;
    use egui_kittest::kittest::{NodeT as _, Queryable as _};
    let joined = format!("{name}, ");
    h.query_all_by(|node| {
        !matches!(node.role(), Role::Label | Role::Tab) && node.label().is_some_and(|label| label == name || label.starts_with(&joined))
    })
    .next()
    .expect("a card of that name")
    .rect()
    .center()
}

#[test]
fn enter_in_the_preset_popup_saves_the_preset_and_keeps_the_dialog() {
    use crate::dialogs::table_layout_kit as kit;
    use egui::{Key, Modifiers};
    let mut h = window_with_preset_popup();
    kit::type_into(&mut h, "Preset Name", 0, "Brochure");
    kit::key(&mut h, Key::Enter, Modifiers::NONE);
    {
        let app = h.state_mut();
        let d = app.ui.dialog.as_ref().expect("Enter saves the preset; it doesn't create the document");
        assert_eq!(app.session.documents().len(), 1, "no document was made (the window opens with one)");
        assert_eq!(app.session.prefs.document_presets.iter().map(|p| p.title.as_str()).collect::<Vec<_>>(), ["Brochure"]);
        assert_eq!((d.s("tab"), d.s("preset")), ("saved".into(), "saved:Brochure".into()));
        // The saved preset makes documents by name, as file.new {preset} does from anywhere.
        app.run("file.new", json!({"preset": "Brochure"})).unwrap();
    }
    assert!(!kit::shows(&h, "Preset Name"), "the popup closed");
}

#[test]
fn saved_preset_delete_buttons_keep_the_preview_and_never_create_a_document() {
    use crate::dialogs::table_layout_kit as kit;
    use egui::accesskit::Role;
    let mut app = kit::app(1, false);
    app.run("file.savePreset", json!({"name":"Portrait draft","pages":3,"facingPages":false})).unwrap();
    app.run("file.savePreset", json!({"name":"Landscape draft","width":792,"height":612,"pages":2})).unwrap();
    app.run("app.newDocumentDialog", json!({})).unwrap();
    let mut h = kit::harness_sized(app, vec2(1440.0, 900.0));
    // The setup's own `file.new` is in the journal: only what the dialog runs from here counts.
    let journal = h.state().session.journal.len();
    kit::click(&mut h, &[Role::Tab], "Saved", 0);
    let portrait = card_at(&h, "Portrait draft");
    kit::click_at(&mut h, portrait);
    kit::click(&mut h, &[Role::CheckBox], "Preview", 0);
    kit::steps(&mut h, 20);
    assert!(!kit::disabled(&h, &[Role::Button], "Delete Preset", 0), "the selected preset has its action");
    // The factory card cannot be deleted.
    let factory = card_at(&h, "[Default]");
    kit::click_at(&mut h, factory);
    assert!(kit::disabled(&h, &[Role::Button], "Delete Preset", 0), "the factory card cannot be deleted");
    let portrait = card_at(&h, "Portrait draft");
    kit::click_at(&mut h, portrait);
    kit::steps(&mut h, 10);
    let before = h.state().session.active().unwrap().doc.clone();
    let preview = h.state().session.new_document_preview();
    let count = h.state().session.documents().len();
    let fields = params(kit::draft(&h));
    kit::click(&mut h, &[Role::Button], "Delete Preset", 0);
    assert_eq!(h.state().session.prefs.document_presets.iter().map(|p| p.title.as_str()).collect::<Vec<_>>(), ["Landscape draft"]);
    {
        let app = h.state();
        assert_eq!(app.session.new_document_preview(), preview);
        assert_eq!(app.session.documents().len(), count);
        assert_eq!(*app.session.active().unwrap().doc, *before);
        assert_eq!(params(app.ui.dialog.as_ref().unwrap()), fields, "deletion does not select another preset or create the preview");
    }
    // The other preset goes the same way, and no document is made.
    let landscape = card_at(&h, "Landscape draft");
    kit::click_at(&mut h, landscape);
    kit::click(&mut h, &[Role::Button], "Delete Preset", 0);
    let app = h.state();
    assert!(app.session.prefs.document_presets.is_empty());
    assert_eq!(app.session.documents().len(), count);
    assert!(app.session.journal[journal..].iter().all(|(id, _)| id != "file.new"), "no document was created");
}

#[test]
fn escape_in_the_preset_popup_closes_only_the_popup() {
    use crate::dialogs::table_layout_kit as kit;
    use egui::{Key, Modifiers};
    let mut h = window_with_preset_popup();
    kit::key(&mut h, Key::Escape, Modifiers::NONE);
    assert!(!kit::shows(&h, "Preset Name"), "Escape closed the popup");
    assert!(h.state().ui.dialog.is_some(), "and left the dialog open");
    // With no popup open, Escape closes the dialog as before.
    kit::key(&mut h, Key::Escape, Modifiers::NONE);
    assert!(h.state().ui.dialog.is_none());
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
