use super::*;

#[test]
fn paragraph_style_settings_show_captured_color_and_pending_spacing_without_changing_the_document() {
    let mut app = DesignApp::new(designcraft_engine::Session::new(), crate::Services::default());
    app.run("file.new", json!({})).unwrap();
    let f = app.run("frame.create", json!({"rect":[36,36,300,200],"content":"text","text":"Heading"})).unwrap();
    app.run("text.select", json!({"story":f["story"],"anchor":0,"focus":7})).unwrap();
    app.run("type.char", json!({"fontFamily":"Source Sans 3","fontStyle":"Bold","size":16,"fill":"[Paper]"})).unwrap();
    app.run("style.paragraph.create", json!({"name":"Header","fromSelection":true})).unwrap();
    let doc = app.session.doc().unwrap().doc.clone();
    let style = doc.styles.para("Header").unwrap();
    let mut d = Dialog::new("paragraphStyleOptions", json!({"name":"Header"}));
    let summary = paragraph_style_summary("", &d, style, &doc.styles);
    for value in ["Source Sans 3", "Bold", "16 pt", "[Paper]"] {
        assert!(summary.contains(value), "{summary}");
    }
    assert!(summary.len() < 250, "default attributes do not swamp the captured settings: {summary}");
    d.fields.insert("c.fill".into(), json!("[Registration]"));
    d.fields.insert("p.spaceAfter".into(), json!(4.5));
    let summary = paragraph_style_summary("", &d, style, &doc.styles);
    assert!(summary.contains("[Registration]") && !summary.contains("[Paper]"), "{summary}");
    assert!(summary.contains("4.5 pt"), "{summary}");
    assert_eq!(app.session.doc().unwrap().doc, doc);
}

/// A dialog opened without the menu's fields has no `useDefaults` field at all. Confirming it
/// with no layer unticked must keep the file's defaults, not choose every layer explicitly:
/// an implicit all-on choice is rejected for a PDF without layers, so the command's error
/// tells the two apart.
#[test]
fn layer_options_confirm_without_the_menu_fields_keeps_file_defaults() {
    let mut app = DesignApp::new(designcraft_engine::Session::new(), crate::Services::default());
    app.run("file.new", json!({"width": 100, "height": 100, "pages": 1})).unwrap();
    let pdf = app.run("file.exportPdf", json!({})).unwrap()["base64"].clone();
    let placed = app.run("file.place", json!({"base64": pdf, "name": "plain.pdf", "x": 0, "y": 0, "width": 100})).unwrap();
    let id = designcraft_doc::ItemId(placed["id"].as_u64().unwrap());
    app.run("selection.set", json!({"ids": [placed["id"]]})).unwrap();
    app.ui.dialog = Some(Dialog::new("layerOptions", json!({})));
    confirm(&mut app).unwrap();
    assert!(!app.session.doc().unwrap().doc.item(id).unwrap().has_pdf_layer_override(), "an untouched dialog keeps the file defaults");
    // The menu's own dialog still asks for an explicit all-on choice when that is the state
    // it was opened in; a layerless PDF refuses it rather than accepting an empty override.
    app.ui.dialog = Some(Dialog::new("layerOptions", json!({"useDefaults": false, "layers": []})));
    let err = confirm(&mut app).unwrap_err();
    assert!(err.contains("could not be changed"), "{err}");
    assert!(!app.session.doc().unwrap().doc.item(id).unwrap().has_pdf_layer_override(), "a layerless PDF never takes an explicit choice");
}

#[test]
fn paragraph_style_settings_compare_with_the_chosen_parent() {
    let mut styles = designcraft_doc::Styles::default();
    styles.paragraph.push(designcraft_doc::ParagraphStyle {
        name: "Parent".into(),
        chars: designcraft_doc::CharAttrs { size: Some(20.0), ..Default::default() },
        based_on: None,
        next_style: None,
        para: Default::default(),
        shortcut: String::new(),
    });
    let style = designcraft_doc::ParagraphStyle {
        name: "Child".into(),
        based_on: Some("Parent".into()),
        next_style: None,
        para: Default::default(),
        chars: Default::default(),
        shortcut: String::new(),
    };
    let d = Dialog::new("paragraphStyleOptions", json!({"name":"Child"}));
    assert_eq!(paragraph_style_summary("", &d, &style, &styles), "Parent");
}

#[test]
fn step_and_repeat_dialog_previews_and_makes_the_copies_as_one_undo_step() {
    let mut app = DesignApp::new(designcraft_engine::Session::new(), crate::Services::default());
    app.run("file.new", json!({})).unwrap();
    let id = app.run("frame.create", json!({"rect": [72, 600, 540, 640]})).unwrap()["id"].as_u64().unwrap();
    app.run("selection.set", json!({"ids": [id]})).unwrap();
    let open = |app: &mut DesignApp| {
        crate::menus::activate(app, "edit.stepAndRepeat", &Value::Null);
        assert_eq!(app.ui.dialog.as_ref().map(|d| d.id.as_str()), Some(STEP_AND_REPEAT));
    };
    let set = |app: &mut DesignApp, k: &str, v: Value| {
        app.ui.dialog.as_mut().unwrap().fields.insert(k.into(), v);
    };
    let items = |app: &DesignApp| app.session.doc().unwrap().doc.spreads[0].items.len();
    let undo = |app: &DesignApp| app.session.doc().unwrap().history.undo.len();
    let (n0, u0) = (items(&app), undo(&app));
    let ctx = dialog_context("en");
    let screen = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(1200.0, 800.0));
    open(&mut app);
    dialog_frame(&mut app, &ctx, screen, vec![]);
    let labels = dialog_frame(&mut app, &ctx, screen, vec![]);
    for expected in ["Step and Repeat", "Repeat", "Count:", "Create as a grid", "Offset", "Vertical:", "Horizontal:", "Preview", "OK", "Cancel"] {
        visible_label(&labels, screen, expected);
    }
    // Preview: the copies show, and change with the fields, without an undo step.
    set(&mut app, "count", json!("5"));
    set(&mut app, "vertical", json!("-0.75 in"));
    set(&mut app, "horizontal", json!("0"));
    set(&mut app, "preview", json!(true));
    dialog_frame(&mut app, &ctx, screen, vec![]);
    assert_eq!((items(&app), undo(&app)), (n0 + 5, u0));
    set(&mut app, "count", json!("3"));
    dialog_frame(&mut app, &ctx, screen, vec![]);
    assert_eq!(items(&app), n0 + 3);
    set(&mut app, "preview", json!(false));
    dialog_frame(&mut app, &ctx, screen, vec![]);
    assert_eq!(items(&app), n0, "Preview off takes them away");
    set(&mut app, "preview", json!(true));
    dialog_frame(&mut app, &ctx, screen, vec![]);
    assert_eq!(items(&app), n0 + 3);
    // Cancel puts the document back.
    cancel(&mut app);
    assert!(app.ui.dialog.is_none());
    assert_eq!((items(&app), undo(&app)), (n0, u0));
    // OK after a preview makes the copies once, as one undo step, 0.75 in apart.
    app.run("selection.set", json!({"ids": [id]})).unwrap();
    open(&mut app);
    set(&mut app, "count", json!("2"));
    set(&mut app, "vertical", json!("-0.75 in"));
    set(&mut app, "horizontal", json!("0"));
    set(&mut app, "preview", json!(true));
    dialog_frame(&mut app, &ctx, screen, vec![]);
    let r = confirm(&mut app).unwrap();
    assert_eq!((items(&app), undo(&app)), (n0 + 2, u0 + 1));
    let last = r["ids"][1].as_u64().unwrap();
    let top = app.session.doc().unwrap().doc.item(designcraft_doc::ItemId(last)).unwrap().bounds().min_y();
    assert!((top - (600.0 - 108.0)).abs() < 1e-6, "{top}");
    // Create as a grid: Rows × Columns cells, the original being one.
    app.run("selection.set", json!({"ids": [id]})).unwrap();
    open(&mut app);
    set(&mut app, "grid", json!(true));
    set(&mut app, "rows", json!("2"));
    set(&mut app, "columns", json!("3"));
    let labels = dialog_frame(&mut app, &ctx, screen, vec![]);
    visible_label(&labels, screen, "Rows:");
    visible_label(&labels, screen, "Columns:");
    assert_eq!(confirm(&mut app).unwrap()["created"], 5);
    // A value that isn't a measure keeps the dialog open and says so.
    open(&mut app);
    set(&mut app, "vertical", json!("up"));
    assert!(confirm(&mut app).is_err());
    let d = app.ui.dialog.as_ref().unwrap();
    assert_eq!(d.s("status"), "Invalid value: up");
}

/// A three-page document in `units` with Document Setup open.
fn document_setup_app(units: &str) -> DesignApp {
    let mut app = DesignApp::new(designcraft_engine::Session::new(), crate::Services::default());
    app.run("file.new", json!({"pages": 3})).unwrap();
    app.run("document.preferences", json!({"horizontalUnits": units, "verticalUnits": units})).unwrap();
    crate::menus::activate(&mut app, "layout.documentSetup", &Value::Null);
    app
}

fn setup_fields(app: &mut DesignApp, fields: Value) {
    let d = app.ui.dialog.as_mut().unwrap();
    for (k, v) in fields.as_object().unwrap() {
        d.fields.insert(k.clone(), v.clone());
    }
}

/// File › File Info opens filled from the document and OK stores what was typed; Export PDF
/// then names the file after the document and takes the PDF's title from File Info.
#[test]
fn file_info_dialog_stores_the_fields_and_titles_the_pdf() {
    use std::sync::{Arc, Mutex};
    let mut app = DesignApp::new(designcraft_engine::Session::new(), crate::Services::default());
    app.run("file.new", json!({"title": "flyer-v3"})).unwrap();
    app.run("file.info", json!({"author": "Ana Ruiz", "keywords": ["spring", "shoes"]})).unwrap();
    crate::menus::activate(&mut app, "file.info", &Value::Null);
    let d = app.ui.dialog.as_ref().unwrap();
    assert_eq!(d.id, "fileInfo");
    assert_eq!((d.s("documentTitle"), d.s("author"), d.s("keywords")), (String::new(), "Ana Ruiz".into(), "spring; shoes".into()));
    setup_fields(&mut app, json!({"documentTitle": "Spring Catalog", "author": "", "keywords": "spring, catalog", "description": "Shoes"}));
    confirm(&mut app).unwrap();
    assert!(app.ui.dialog.is_none());
    let info = app.session.active().unwrap().doc.file_info.clone();
    assert_eq!((info.document_title.as_str(), info.author.as_str(), info.description.as_str()), ("Spring Catalog", "", "Shoes"));
    assert_eq!(info.keywords, ["spring", "catalog"]);
    // Keywords left as shown stay as they are, even one with a comma in it.
    app.run("file.info", json!({"keywords": ["Washington, DC", "map"]})).unwrap();
    crate::menus::activate(&mut app, "file.info", &Value::Null);
    setup_fields(&mut app, json!({"author": "Studio"}));
    confirm(&mut app).unwrap();
    let info = app.session.active().unwrap().doc.file_info.clone();
    assert_eq!((info.author.as_str(), info.keywords.clone()), ("Studio", vec!["Washington, DC".to_string(), "map".to_string()]));
    // Another keyword edited: the one with a comma in it still stays one keyword.
    crate::menus::activate(&mut app, "file.info", &Value::Null);
    assert_eq!(app.ui.dialog.as_ref().unwrap().s("keywords"), "Washington, DC; map");
    setup_fields(&mut app, json!({"keywords": "Washington, DC; atlas"}));
    confirm(&mut app).unwrap();
    assert_eq!(app.session.active().unwrap().doc.file_info.keywords, ["Washington, DC", "atlas"]);

    let writes: Arc<Mutex<Vec<Vec<u8>>>> = Arc::new(Mutex::new(Vec::new()));
    let sink = writes.clone();
    app.services.write = Some(Box::new(move |_: &str, bytes: &[u8]| {
        sink.lock().unwrap().push(bytes.to_vec());
        Ok(())
    }));
    let names: Arc<Mutex<Vec<String>>> = Arc::new(Mutex::new(Vec::new()));
    let asked = names.clone();
    app.services.pick_save = Some(Box::new(move |name: &str| {
        asked.lock().unwrap().push(name.to_string());
        Some("/tmp/designcraft-file-info.pdf".to_string())
    }));
    open_pdf_export(&mut app);
    confirm(&mut app).unwrap();
    assert_eq!(names.lock().unwrap().as_slice(), ["flyer-v3.pdf"]);
    let pdf = String::from_utf8_lossy(&writes.lock().unwrap()[0]).into_owned();
    assert!(pdf.contains("/Title(Spring Catalog)") && !pdf.contains("/Title(flyer-v3)"), "the PDF's title is File Info's");
    // A title given to the export is the PDF's title, even when it is the document's name.
    open_pdf_export(&mut app);
    setup_fields(&mut app, json!({"title": "flyer-v3"}));
    confirm(&mut app).unwrap();
    let asked = names.lock().unwrap().last().cloned().unwrap_or_default();
    assert_eq!(std::path::Path::new(&asked).file_name().and_then(|n| n.to_str()), Some("flyer-v3.pdf"), "{asked}");
    let pdf = String::from_utf8_lossy(&writes.lock().unwrap()[1]).into_owned();
    assert!(pdf.contains("/Title(flyer-v3)") && !pdf.contains("/Title(Spring Catalog)"), "the given title wins");
}

/// File Info's OK stores the fields in the document the dialog was opened for, even when
/// another document became active meanwhile; once that document has closed, OK stores nothing.
#[test]
fn file_info_dialog_stores_into_the_document_it_was_opened_for() {
    let mut app = DesignApp::new(designcraft_engine::Session::new(), crate::Services::default());
    app.run("file.new", json!({"title": "first"})).unwrap();
    crate::menus::activate(&mut app, "file.info", &Value::Null);
    app.run("file.new", json!({"title": "second"})).unwrap();
    setup_fields(&mut app, json!({"author": "Ana Ruiz"}));
    confirm(&mut app).unwrap();
    let docs = app.session.documents();
    let author = |title: &str| docs.iter().find(|d| d.doc.title == title).map(|d| d.doc.file_info.author.clone());
    assert_eq!((author("first").as_deref(), author("second").as_deref()), (Some("Ana Ruiz"), Some("")));
    assert_eq!(app.session.active().map(|d| d.doc.title.as_str()), Some("second"), "the active document stays active");

    crate::menus::activate(&mut app, "file.info", &Value::Null);
    app.run("file.close", json!({})).unwrap();
    setup_fields(&mut app, json!({"author": "Studio"}));
    confirm(&mut app).unwrap();
    assert!(app.session.documents().iter().all(|d| d.doc.file_info.author != "Studio"), "the dialog's document closed");
}

/// File Info opened through `ui.dialog.open` with only some fields stores only those fields;
/// the others keep what the document holds.
#[test]
fn file_info_opened_with_some_fields_keeps_the_others() {
    let mut app = DesignApp::new(designcraft_engine::Session::new(), crate::Services::default());
    app.run("file.new", json!({})).unwrap();
    app.run("file.info", json!({"documentTitle": "Spring Catalog", "keywords": ["spring", "shoes"]})).unwrap();
    assert_eq!(ctl(&mut app, "ui.dialog.open", json!({"id": "fileInfo", "fields": {"author": "Ana Ruiz", "description": ""}}))["ok"], true);
    confirm(&mut app).unwrap();
    let info = app.session.active().unwrap().doc.file_info.clone();
    assert_eq!((info.document_title.as_str(), info.author.as_str(), info.description.as_str()), ("Spring Catalog", "Ana Ruiz", ""));
    assert_eq!(info.keywords, ["spring", "shoes"]);
}

/// Fields given to `ui.dialog.open` as JSON other than text reach `file.info` as they are: a
/// keyword list stays a list, and `null` clears a field.
#[test]
fn file_info_opened_with_a_keyword_list_stores_the_list() {
    let mut app = DesignApp::new(designcraft_engine::Session::new(), crate::Services::default());
    app.run("file.new", json!({})).unwrap();
    app.run("file.info", json!({"author": "Ana Ruiz"})).unwrap();
    let fields = json!({"keywords": ["Washington, DC", "map"], "author": null});
    assert_eq!(ctl(&mut app, "ui.dialog.open", json!({"id": "fileInfo", "fields": fields}))["ok"], true);
    confirm(&mut app).unwrap();
    let info = app.session.active().unwrap().doc.file_info.clone();
    assert_eq!(info.keywords, ["Washington, DC", "map"]);
    assert_eq!(info.author, "");
}

/// File Info's OK for a document that is not the active one leaves the active document's
/// Data Merge preview showing the record it showed.
#[test]
fn file_info_for_another_document_keeps_the_active_preview() {
    let mut app = DesignApp::new(designcraft_engine::Session::new(), crate::Services::default());
    app.run("file.new", json!({"title": "first"})).unwrap();
    crate::menus::activate(&mut app, "file.info", &Value::Null);
    app.run("file.new", json!({"title": "second"})).unwrap();
    let csv = designcraft_engine::cmd::base64_encode(b"Name\nAda\nGrace\n");
    app.run("data.source.select", json!({"bytes": csv, "name": "people.csv"})).unwrap();
    app.run("data.preview", json!({"record": 2})).unwrap();
    setup_fields(&mut app, json!({"author": "Ana Ruiz"}));
    confirm(&mut app).unwrap();
    let active = app.session.active().unwrap();
    assert_eq!((active.doc.title.as_str(), active.preview_record), ("second", Some(2)), "the preview still shows record 2");
    let first = app.session.documents().iter().find(|d| d.doc.title == "first").map(|d| d.doc.file_info.author.clone());
    assert_eq!(first.as_deref(), Some("Ana Ruiz"));
}

#[test]
fn document_setup_ok_applies_margins_bleed_and_size_in_one_step() {
    let mut app = document_setup_app("millimeters");
    let d = app.ui.dialog.as_ref().unwrap();
    assert_eq!(d.id, "documentSetup");
    assert_eq!((d.s("marginTop"), d.b("marginLinked")), ("12.7 mm".to_string(), true), "the document's margins, in its units");
    setup_fields(
        &mut app,
        json!({"marginTop": "20 mm", "marginBottom": "20 mm", "marginInside": "20 mm", "marginOutside": "20 mm", "bleedTop": "3 mm",
        "bleedBottom": "3 mm", "bleedInside": "3 mm", "bleedOutside": "3 mm", "width": "210 mm", "height": "297 mm"}),
    );
    confirm(&mut app).unwrap();
    assert!(app.ui.dialog.is_none());
    let mm = designcraft_geom::units::PT_PER_MM;
    let doc = &app.session.active().unwrap().doc;
    assert!((doc.settings.page_width - 210.0 * mm).abs() < 1e-6 && (doc.settings.bleed[2] - 3.0 * mm).abs() < 1e-6);
    assert!(doc.spreads.iter().flat_map(|s| &s.pages).all(|p| (p.margins.top - 20.0 * mm).abs() < 1e-6), "every page");
    app.run("edit.undo", json!({})).unwrap();
    let doc = &app.session.active().unwrap().doc;
    assert!((doc.settings.page_width - 612.0).abs() < 1e-6 && doc.settings.bleed[0] == 0.0, "one undo step");
}

#[test]
fn document_setup_reads_a_bare_number_in_the_documents_units() {
    let mut app = document_setup_app("millimeters");
    setup_fields(&mut app, json!({"bleedTop": "5", "bleedBottom": "5", "bleedInside": "5", "bleedOutside": "5", "width": "210"}));
    confirm(&mut app).unwrap();
    let mm = designcraft_geom::units::PT_PER_MM;
    let settings = &app.session.active().unwrap().doc.settings;
    assert!(settings.bleed.iter().all(|b| (b - 5.0 * mm).abs() < 1e-6), "5 mm, not 5 picas: {:?}", settings.bleed);
    assert!((settings.page_width - 210.0 * mm).abs() < 1e-6, "{}", settings.page_width);
}

#[test]
fn document_setup_keeps_each_pages_margins_unless_they_are_edited() {
    let mut app = DesignApp::new(designcraft_engine::Session::new(), crate::Services::default());
    app.run("file.new", json!({"pages": 3, "margins": 36})).unwrap();
    app.run("layout.marginsAndColumns", json!({"pages": [1], "margins": 90})).unwrap();
    let margins =
        |app: &DesignApp| app.session.active().unwrap().doc.spreads.iter().flat_map(|s| &s.pages).map(|p| p.margins.top).collect::<Vec<_>>();
    // Only the page count changes: every page keeps its own margins.
    crate::menus::activate(&mut app, "layout.documentSetup", &Value::Null);
    setup_fields(&mut app, json!({"pages": "4"}));
    confirm(&mut app).unwrap();
    assert_eq!(margins(&app), [36.0, 90.0, 36.0, 36.0]);
    // Edited margins go to every page.
    crate::menus::activate(&mut app, "layout.documentSetup", &Value::Null);
    setup_fields(&mut app, json!({"marginTop": "1p0"}));
    confirm(&mut app).unwrap();
    assert_eq!(margins(&app), [12.0; 4]);
}

#[test]
fn document_setup_keeps_a_value_it_cannot_read_and_changes_nothing() {
    for (key, value) in [("width", "wide"), ("marginTop", "-"), ("pages", "0"), ("bleedTop", "x")] {
        let mut app = document_setup_app("points");
        setup_fields(&mut app, json!({key: value, "bleedInside": "9"}));
        assert!(confirm(&mut app).is_err(), "{key}");
        assert!(app.ui.dialog.is_some(), "{key}: the dialog stays open");
        assert_eq!(app.session.active().unwrap().doc.settings.bleed, [0.0; 4], "{key}: nothing changed");
    }
}

#[test]
fn document_setup_links_edges_and_adjust_layout_applies() {
    let mut app = document_setup_app("points");
    let f = app.run("frame.create", json!({"rect": [36, 36, 576, 756], "content": "text"})).unwrap();
    crate::menus::activate(&mut app, "layout.documentSetup", &Value::Null);
    let ctx = dialog_context("en");
    let screen = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(1178.0, 900.0));
    for _ in 0..3 {
        dialog_frame(&mut app, &ctx, screen, vec![]);
    }
    // Typing in a linked margin sets all four.
    let labels = dialog_frame(&mut app, &ctx, screen, vec![]);
    let top = labels.iter().filter(|(t, _, _)| t == "36 pt").map(|(_, r, _)| *r).next().expect("the top margin");
    click_dialog(&mut app, &ctx, screen, top.center());
    let select_all = egui::Event::Key { key: egui::Key::A, physical_key: None, pressed: true, repeat: false, modifiers: egui::Modifiers::COMMAND };
    dialog_frame(&mut app, &ctx, screen, vec![select_all, egui::Event::Text("72 pt".into())]);
    dialog_frame(&mut app, &ctx, screen, vec![]);
    let d = app.ui.dialog.as_ref().unwrap();
    assert!(["Top", "Bottom", "Inside", "Outside"].iter().all(|e| d.s(&format!("margin{e}")) == "72 pt"), "linked: {:?}", d.fields);
    // Adjust Layout applies at once, and the frame follows the margins.
    let labels = dialog_frame(&mut app, &ctx, screen, vec![]);
    click_dialog(&mut app, &ctx, screen, visible_label(&labels, screen, "Adjust Layout").center());
    assert!(app.ui.dialog.is_none(), "applied");
    let doc = &app.session.active().unwrap().doc;
    let b = doc.item(designcraft_doc::ItemId(f["id"].as_u64().unwrap())).unwrap().bounds();
    assert!((b.y0 - 72.0).abs() < 1e-6 && doc.page(0).unwrap().margins.outside == 72.0, "{b:?}");
}

#[test]
fn cell_options_dialog_draws_and_applies_only_changed_values_to_the_chosen_edges() {
    let mut app = DesignApp::new(designcraft_engine::Session::new(), crate::Services::default());
    app.run("file.new", json!({})).unwrap();
    app.run("frame.create", json!({"rect": [72, 72, 400, 400], "content": "text", "caret": true})).unwrap();
    let tid = app.run("table.insert", json!({"rows": 2, "cols": 2})).unwrap()["table"].as_u64().unwrap();
    app.run("table.select", json!({"what": "table"})).unwrap();
    app.run("app.cellOptionsDialog", json!({})).unwrap();
    let ctx = dialog_context("");
    dialog_frame(&mut app, &ctx, egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(900.0, 700.0)), vec![]);
    let d = app.ui.dialog.as_mut().unwrap();
    assert_eq!(d.id, "cellOptions");
    assert_eq!(d.fields["weight"], json!(1.0));
    d.fields.insert("edges".into(), json!(["top", "bottom", "left", "right"]));
    d.fields.insert("gapColor".into(), json!("[Paper]"));
    d.fields.insert("fill".into(), json!("[Black]"));
    d.fields.insert("fillTint".into(), json!(0.2));
    confirm(&mut app).unwrap();
    let st = app.session.active().unwrap();
    let t = st.doc.stories.values().find_map(|s| s.tables.get(&tid)).unwrap();
    let gap = |r: usize, c: usize, side: usize| t.cell(r, c).unwrap().strokes[side].gap_color.clone();
    assert_eq!([gap(0, 0, 0), gap(0, 0, 1), gap(1, 1, 2), gap(1, 1, 3)], ["[Paper]"; 4].map(String::from), "outer edges");
    assert_eq!([gap(0, 0, 2), gap(0, 0, 3)], ["[None]"; 2].map(String::from), "inner edges keep theirs");
    assert!(t.cells.iter().all(|c| c.fill == "[Black]" && (c.fill_tint - 0.2).abs() < 1e-6));
    assert!(t.cells.iter().all(|c| c.strokes.iter().all(|e| e.weight == 1.0)), "unchanged values are not applied");
}

#[test]
fn cell_options_edge_changes_reseed_every_stroke_field_and_survive_a_bad_seed() {
    let mut app = DesignApp::new(designcraft_engine::Session::new(), crate::Services::default());
    app.run("file.new", json!({})).unwrap();
    app.run("frame.create", json!({"rect": [72, 72, 400, 400], "content": "text", "caret": true})).unwrap();
    let tid = app.run("table.insert", json!({"rows": 2, "cols": 2})).unwrap()["table"].as_u64().unwrap();
    app.run("table.select", json!({"what": "table"})).unwrap();
    app.run("table.setCell", json!({"stroke": {"weight": 3, "edges": "innerHorizontal"}})).unwrap();
    app.run("app.cellOptionsDialog", json!({})).unwrap();
    let mut d = app.ui.dialog.take().unwrap();
    d.fields.insert("seed".into(), json!(5));
    let outer = [true, true, true, true, false, false];
    cell_options_edges_changed(&mut app, &mut d, &outer);
    assert_eq!((&d.fields["weight"], &d.fields["seed"]["weight"]), (&json!(1.0), &json!(1.0)));
    // A weight typed for the outer edges, then the inner horizontal edges (3 pt) chosen
    // and 1 pt typed: 1 pt differs from what those edges show, so OK applies it.
    d.fields.insert("weight".into(), json!(5.0));
    cell_options_edges_changed(&mut app, &mut d, &[false, false, false, false, true, false]);
    assert_eq!((&d.fields["weight"], &d.fields["seed"]["weight"]), (&json!(5.0), &json!(3.0)));
    d.fields.insert("weight".into(), json!(1.0));
    app.ui.dialog = Some(d);
    confirm(&mut app).unwrap();
    let st = app.session.active().unwrap();
    let t = st.doc.stories.values().find_map(|s| s.tables.get(&tid)).unwrap();
    assert_eq!([t.cell(0, 0).unwrap().strokes[2].weight, t.cell(1, 1).unwrap().strokes[0].weight], [1.0; 2]);
}

#[test]
fn disk_preview_preference_applies_and_survives_preferences_reload() {
    let mut app = DesignApp::new(designcraft_engine::Session::new(), crate::Services::default());
    assert!(app.ui.persist_image_previews);
    app.run("app.preferences", json!({})).unwrap();
    app.ui.dialog.as_mut().unwrap().fields.insert("persistImagePreviews".into(), json!(false));
    assert!(app.ui.persist_image_previews, "editing the dialog does not apply before OK");
    confirm(&mut app).unwrap();
    let saved = serde_json::to_vec(&app.ui).unwrap();
    assert!(!crate::UiState::from_preferences_json(&saved).unwrap().persist_image_previews);
    assert!(crate::UiState::from_preferences_json(b"{}").unwrap().persist_image_previews);
}

#[test]
fn preferences_ok_saves_a_story_editor_size_that_loads_back() {
    // `ui.dialog.set` takes any value; a size that isn't finite is saved as `null`.
    let sizes = [(json!("nan"), 20.0), (json!("inf"), 20.0), (json!(null), 20.0), (json!(1e39), 36.0), (json!(3), 8.0), (json!(11), 11.0)];
    for (size, want) in sizes {
        let mut app = DesignApp::new(designcraft_engine::Session::new(), crate::Services::default());
        app.run("file.new", json!({})).unwrap();
        app.ui.story_editor_size = 20.0;
        app.run("app.preferences", json!({})).unwrap();
        app.ui.dialog.as_mut().unwrap().fields.insert("storyEditorSize".into(), size);
        confirm(&mut app).unwrap();
        let saved = serde_json::to_vec(&app.ui).unwrap();
        assert_eq!(crate::UiState::from_preferences_json(&saved).unwrap().story_editor_size, want);
    }
}

#[test]
fn preferences_ok_sets_only_the_ruler_units_it_is_given() {
    // `ui.dialog.open` takes any fields, so the vertical units can be missing.
    for (vertical, want) in [(None, "inches"), (Some("points"), "points")] {
        let mut app = DesignApp::new(designcraft_engine::Session::new(), crate::Services::default());
        app.run("file.new", json!({})).unwrap();
        app.run("document.preferences", json!({"verticalUnits": "inches"})).unwrap();
        let mut fields = json!({"horizontalUnits": "millimeters", "bg.relativeTo": "topOfPage"});
        if let Some(vertical) = vertical {
            fields["verticalUnits"] = json!(vertical);
        }
        app.ui.dialog = Some(Dialog::new("preferences", fields));
        confirm(&mut app).unwrap();
        let units = app.run("document.preferences", json!({})).unwrap();
        assert_eq!((units["horizontalUnits"].as_str(), units["verticalUnits"].as_str()), (Some("millimeters"), Some(want)));
    }
}

#[test]
fn enter_and_escape_close_the_style_menu_not_numbering_and_section_options() {
    let mut app = DesignApp::new(designcraft_engine::Session::new(), crate::Services::default());
    app.run("file.new", json!({"pages": 2})).unwrap();
    crate::menus::activate(&mut app, "layout.section", &Value::Null);
    let ctx = dialog_context("en");
    let screen = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(1178.0, 814.0));
    for _ in 0..3 {
        dialog_frame(&mut app, &ctx, screen, vec![]);
    }
    let key = |key| egui::Event::Key { key, physical_key: None, pressed: true, repeat: false, modifiers: egui::Modifiers::NONE };
    for pressed in [egui::Key::Enter, egui::Key::Escape] {
        let labels = dialog_frame(&mut app, &ctx, screen, vec![]);
        // The section's Style menu (the first of the two).
        let style = labels.iter().filter(|(t, r, c)| t.trim() == "1, 2, 3, 4…" && c.contains_rect(*r)).map(|(_, r, _)| *r).next().unwrap();
        click_dialog(&mut app, &ctx, screen, style.center());
        dialog_frame(&mut app, &ctx, screen, vec![]);
        assert!(egui::Popup::is_any_open(&ctx), "the Style menu opened");
        dialog_frame(&mut app, &ctx, screen, vec![key(pressed)]);
        dialog_frame(&mut app, &ctx, screen, vec![]);
        assert!(app.ui.dialog.is_some(), "{pressed:?} with the menu open leaves the dialog open");
        assert!(app.session.active().unwrap().doc.sections.iter().all(|s| s.start_number != Some(0)));
        if egui::Popup::is_any_open(&ctx) {
            egui::Popup::close_all(&ctx);
            dialog_frame(&mut app, &ctx, screen, vec![]);
        }
    }
    // With no menu open, Escape closes the dialog.
    dialog_frame(&mut app, &ctx, screen, vec![key(egui::Key::Escape)]);
    assert!(app.ui.dialog.is_none());
}

#[test]
fn numbering_and_section_options_keep_their_actions_in_view() {
    for language in ["en", "ar"] {
        for size in [egui::vec2(1178.0, 814.0), egui::vec2(392.0, 271.0)] {
            let mut app = DesignApp::new(designcraft_engine::Session::new(), crate::Services::default());
            app.ui.language = language.into();
            app.run("file.new", json!({"pages": 2})).unwrap();
            crate::menus::activate(&mut app, "layout.section", &Value::Null);
            let ctx = dialog_context(language);
            let screen = egui::Rect::from_min_size(egui::Pos2::ZERO, size);
            for _ in 0..3 {
                dialog_frame(&mut app, &ctx, screen, vec![]);
            }
            let labels = dialog_frame(&mut app, &ctx, screen, vec![]);
            let tr = |s| crate::i18n::tr(language, s);
            for expected in [tr("Numbering & Section Options…").trim_end_matches('…'), tr("OK"), tr("Cancel"), tr("Start Section")] {
                visible_label(&labels, screen, expected);
            }
            if size.x > 1000.0 {
                // Every option fits without scrolling, and choosing one then OK applies it.
                visible_label(&labels, screen, &format!("{} {}", tr("Book Name:"), tr("N/A")));
                click_dialog(&mut app, &ctx, screen, visible_label(&labels, screen, tr("Automatic Page Numbering")).center());
                let labels = dialog_frame(&mut app, &ctx, screen, vec![]);
                click_dialog(&mut app, &ctx, screen, visible_label(&labels, screen, tr("OK")).center());
                assert!(app.ui.dialog.is_none(), "{language}: OK closes the dialog");
                let doc = &app.session.active().unwrap().doc;
                assert!(doc.sections.iter().all(|s| s.start != 0 || s.start_number.is_none()), "{language}: page 1 numbers automatically");
            }
        }
    }
}

fn select_all_key() -> egui::Event {
    egui::Event::Key {
        key: egui::Key::A,
        physical_key: None,
        pressed: true,
        repeat: false,
        modifiers: egui::Modifiers { ctrl: true, command: true, ..Default::default() },
    }
}

/// The Color Picker over a new document's selected frame, a few frames in.
fn picker(fields: Value) -> (DesignApp, egui::Context, egui::Rect, u64) {
    let mut app = DesignApp::new(designcraft_engine::Session::new(), crate::Services::default());
    app.run("file.new", json!({})).unwrap();
    let id = app.run("frame.create", json!({"rect": [36, 36, 200, 200]})).unwrap()["id"].as_u64().unwrap();
    app.run("selection.set", json!({"ids": [id]})).unwrap();
    app.ui.dialog = Some(Dialog::new("colorPicker", fields));
    let ctx = dialog_context("en");
    let screen = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(700.0, 500.0));
    for _ in 0..3 {
        dialog_frame(&mut app, &ctx, screen, vec![]);
    }
    (app, ctx, screen, id)
}

/// The rightmost painted `text`: the picker's own field, right of egui's colour editor (which
/// has R/G/B fields of its own).
fn rightmost(labels: &[(String, egui::Rect, egui::Rect)], text: &str) -> egui::Pos2 {
    labels
        .iter()
        .filter(|(t, _, _)| t.trim() == text)
        .map(|(_, r, _)| r.center())
        .max_by(|a, b| a.x.total_cmp(&b.x))
        .unwrap_or_else(|| panic!("{text} is painted"))
}

/// Types `text` one character per frame, with an idle frame after each.
fn type_slowly(app: &mut DesignApp, ctx: &egui::Context, screen: egui::Rect, text: &str) {
    for ch in text.chars() {
        dialog_frame(app, ctx, screen, vec![egui::Event::Text(ch.into())]);
        dialog_frame(app, ctx, screen, vec![]);
    }
}

fn fill(app: &DesignApp, id: u64) -> designcraft_color::Color {
    let d = &app.session.active().unwrap().doc;
    d.resolve_color(&d.item(designcraft_doc::ItemId(id)).unwrap().fill.swatch, 1.0).unwrap()
}

#[test]
fn the_hex_field_keeps_typed_text_and_commits_a_valid_code() {
    let (mut app, ctx, screen, _) = picker(json!({"target": "fill", "hex": "#ffffff"}));
    let labels = dialog_frame(&mut app, &ctx, screen, vec![]);
    click_dialog(&mut app, &ctx, screen, visible_label(&labels, screen, "ffffff").center());
    dialog_frame(&mut app, &ctx, screen, vec![select_all_key()]);
    type_slowly(&mut app, &ctx, screen, "#f5");
    let d = app.ui.dialog.as_ref().unwrap();
    assert_eq!((d.s("hexEdit").as_str(), d.s("hex").as_str()), ("#f5", "#ffffff"), "half-typed text stays; the colour waits");
    type_slowly(&mut app, &ctx, screen, "9019");
    let d = app.ui.dialog.as_ref().unwrap();
    assert_eq!((d.s("hexEdit").as_str(), d.s("hex").as_str()), ("#f59019", "#f59019"), "a 7-character #rrggbb commits");
    // Invalid text is replaced by the colour when focus leaves.
    dialog_frame(&mut app, &ctx, screen, vec![select_all_key()]);
    type_slowly(&mut app, &ctx, screen, "zz");
    let labels = dialog_frame(&mut app, &ctx, screen, vec![]);
    click_dialog(&mut app, &ctx, screen, visible_label(&labels, screen, "#").center());
    let labels = dialog_frame(&mut app, &ctx, screen, vec![]);
    visible_label(&labels, screen, "f59019");
    assert_eq!(app.ui.dialog.as_ref().unwrap().s("hex"), "#f59019");
}

#[test]
fn typed_channel_values_are_kept_while_typing() {
    let (mut app, ctx, screen, id) = picker(json!({"target": "fill", "hex": "#010203", "model": "rgb", "rgb": [1, 2, 3]}));
    let labels = dialog_frame(&mut app, &ctx, screen, vec![]);
    click_dialog(&mut app, &ctx, screen, rightmost(&labels, "1"));
    dialog_frame(&mut app, &ctx, screen, vec![select_all_key()]);
    type_slowly(&mut app, &ctx, screen, "12.5");
    let labels = dialog_frame(&mut app, &ctx, screen, vec![]);
    assert!(rightmost(&labels, "12.5").x > rightmost(&labels, "2").x - 1.0, "the typed text stays in the R field");
    assert_eq!(app.ui.dialog.as_ref().unwrap().s("hex"), "#0d0203");
    confirm(&mut app).unwrap();
    let designcraft_color::Color::Rgb { r, .. } = fill(&app, id) else { panic!("not RGB") };
    assert!((r - 12.5 / 255.0).abs() < 1e-6, "{r}");
}

#[test]
fn a_cmyk_colour_stays_cmyk_unless_rgb_values_are_typed() {
    let cmyk = json!({"target": "fill", "hex": "#ff8000", "model": "cmyk", "cmyk": [0, 50, 100, 0]});
    // Picked by hex: converted to CMYK.
    let (mut app, ctx, screen, id) = picker(cmyk.clone());
    let hex = app.ui.dialog.as_ref().unwrap().s("hex");
    let labels = dialog_frame(&mut app, &ctx, screen, vec![]);
    click_dialog(&mut app, &ctx, screen, visible_label(&labels, screen, hex.trim_start_matches('#')).center());
    dialog_frame(&mut app, &ctx, screen, vec![select_all_key()]);
    type_slowly(&mut app, &ctx, screen, "3366cc");
    confirm(&mut app).unwrap();
    assert!(matches!(fill(&app, id), designcraft_color::Color::Cmyk { .. }), "{:?}", fill(&app, id));
    // A typed C/M/Y/K percent is a percent: M 1 is 1 %.
    let (mut app, ctx, screen, id) = picker(cmyk.clone());
    let labels = dialog_frame(&mut app, &ctx, screen, vec![]);
    click_dialog(&mut app, &ctx, screen, rightmost(&labels, "50"));
    dialog_frame(&mut app, &ctx, screen, vec![select_all_key()]);
    type_slowly(&mut app, &ctx, screen, "1");
    confirm(&mut app).unwrap();
    let designcraft_color::Color::Cmyk { c, m, y, k } = fill(&app, id) else { panic!("not CMYK") };
    assert!(c == 0.0 && (m - 0.01).abs() < 1e-6 && (y - 1.0).abs() < 1e-6 && k == 0.0, "{c} {m} {y} {k}");
    // Typed R/G/B values: RGB.
    let (mut app, ctx, screen, id) = picker(cmyk);
    let r = app.ui.dialog.as_ref().unwrap().fields["rgb"][0].as_f64().unwrap();
    let labels = dialog_frame(&mut app, &ctx, screen, vec![]);
    click_dialog(&mut app, &ctx, screen, rightmost(&labels, &r.to_string()));
    dialog_frame(&mut app, &ctx, screen, vec![select_all_key()]);
    type_slowly(&mut app, &ctx, screen, "10");
    confirm(&mut app).unwrap();
    assert!(matches!(fill(&app, id), designcraft_color::Color::Rgb { .. }), "{:?}", fill(&app, id));
}

#[test]
fn ok_without_edits_leaves_the_fill_alone() {
    let (mut app, _, _, id) = picker(json!({"target": "fill", "hex": "#ff8000", "model": "cmyk", "cmyk": [0, 50, 100, 0]}));
    let before = app.session.active().unwrap().doc.item(designcraft_doc::ItemId(id)).unwrap().fill.clone();
    confirm(&mut app).unwrap();
    assert_eq!(app.session.active().unwrap().doc.item(designcraft_doc::ItemId(id)).unwrap().fill, before);
}

#[test]
fn dialogs_do_not_dim_the_page() {
    let (mut app, ctx, screen, _) = picker(json!({"target": "fill", "hex": "#ff8000"}));
    let time = ctx.input(|i| i.time) + 1.0 / 60.0;
    let output = ctx.run_ui(egui::RawInput { screen_rect: Some(screen), time: Some(time), ..Default::default() }, |ui| show(&mut app, ui.ctx()));
    fn covers(shape: &egui::Shape, screen: egui::Rect) -> bool {
        match shape {
            egui::Shape::Rect(r) => r.rect.contains_rect(screen) && r.fill.a() > 0,
            egui::Shape::Vec(v) => v.iter().any(|s| covers(s, screen)),
            _ => false,
        }
    }
    assert!(app.ui.dialog.is_some());
    assert!(!output.shapes.iter().any(|s| covers(&s.shape, screen)), "nothing paints over the whole window");
}

fn draw(app: &mut DesignApp, ctx: &egui::Context) {
    let input = egui::RawInput { screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(1400.0, 900.0))), ..Default::default() };
    let mut out = ctx.run_ui(input, |ui| {
        app.logic(&ui.ctx().clone());
        app.ui(ui);
    });
    out.textures_delta.clear();
}

/// A paragraph styled "Ruled Child", whose Rule Below comes from its parent "Ruled".
fn ruled_paragraph() -> (DesignApp, Value) {
    let mut app = DesignApp::new(designcraft_engine::Session::new(), crate::Services::default());
    app.run("file.new", json!({})).unwrap();
    let rule = json!({"on": true, "weight": 3.0, "color": "[Black]", "tint": 0.5, "columnWidth": false, "offset": 4.0, "leftIndent": 6.0, "rightIndent": 2.0});
    app.run("style.paragraph.create", json!({"name": "Ruled", "para": {"ruleBelow": rule}})).unwrap();
    app.run("style.paragraph.create", json!({"name": "Ruled Child", "basedOn": "Ruled"})).unwrap();
    let r = app.run("frame.create", json!({"rect": [72, 72, 300, 400], "content": "text", "text": "One"})).unwrap();
    app.run("text.select", json!({"story": r["story"], "anchor": 0, "focus": 0})).unwrap();
    app.run("style.paragraph.apply", json!({"name": "Ruled Child"})).unwrap();
    (app, rule)
}

fn undo_steps(app: &DesignApp) -> usize {
    app.session.doc().map_or(0, |st| st.history.undo.len())
}

#[test]
fn paragraph_rules_dialog_shows_the_rule_and_applies_one_field() {
    let (mut app, rule) = ruled_paragraph();
    let ctx = egui::Context::default();
    app.run("app.paragraphRulesDialog", json!({"rule": "ruleBelow"})).unwrap();
    draw(&mut app, &ctx);
    let d = app.ui.dialog.clone().unwrap();
    assert_eq!(d.id, "paragraphRules");
    let current = d.fields["current"].clone();
    assert_eq!(shown_rule(&d, "", "ruleBelow", &current), rule, "the inherited rule");
    assert_eq!(shown_rule(&d, "", "ruleAbove", &current)["on"], false);
    // Pick a colour, as the Color menu does.
    app.ui.dialog.as_mut().unwrap().fields.insert("ruleBelow.color".into(), json!("[Registration]"));
    draw(&mut app, &ctx);
    let before = undo_steps(&app);
    confirm(&mut app).unwrap();
    assert_eq!(undo_steps(&app), before + 1, "one undo step");
    let mut want = rule.clone();
    want["color"] = json!("[Registration]");
    assert_eq!(app.run("type.selectionAttrs", json!({})).unwrap()["para"]["ruleBelow"], want);
    app.run("edit.undo", json!({})).unwrap();
    assert_eq!(app.run("type.selectionAttrs", json!({})).unwrap()["para"]["ruleBelow"], rule);
}

#[test]
fn paragraph_style_options_edit_one_rule_field() {
    let (mut app, rule) = ruled_paragraph();
    let ctx = egui::Context::default();
    app.ui.dialog = Some(Dialog::new("paragraphStyleOptions", json!({"name": "Ruled Child", "section": "rules", "rule": "ruleBelow"})));
    draw(&mut app, &ctx);
    let resolved = |app: &DesignApp| serde_json::to_value(app.session.doc().unwrap().doc.styles.resolve_para_style("Ruled Child").0).unwrap();
    let d = app.ui.dialog.clone().unwrap();
    assert_eq!(shown_rule(&d, "p.", "ruleBelow", &resolved(&app)), rule, "inherited from the parent style");
    app.ui.dialog.as_mut().unwrap().fields.insert("p.ruleBelow.weight".into(), json!(1.5));
    draw(&mut app, &ctx);
    let before = undo_steps(&app);
    confirm(&mut app).unwrap();
    assert_eq!(undo_steps(&app), before + 1, "one undo step");
    let mut want = rule.clone();
    want["weight"] = json!(1.5);
    assert_eq!(resolved(&app)["ruleBelow"], want);
    assert_eq!(app.run("type.selectionAttrs", json!({})).unwrap()["para"]["ruleBelow"], want, "the paragraph follows its style");
    app.run("edit.undo", json!({})).unwrap();
    assert_eq!(resolved(&app)["ruleBelow"], rule);
}

/// Opens Paragraph Style Options on "Ruled Child" at each of `sections`, sets `fields` and
/// clicks OK; returns the resolved attributes of the paragraph styled with it.
fn style_section_edit(sections: &[&str], fields: Value) -> Value {
    let (mut app, _) = ruled_paragraph();
    let ctx = egui::Context::default();
    app.ui.dialog = Some(Dialog::new("paragraphStyleOptions", json!({"name": "Ruled Child"})));
    for section in sections {
        app.ui.dialog.as_mut().unwrap().fields.insert("section".into(), json!(section));
        draw(&mut app, &ctx);
    }
    for (k, v) in fields.as_object().unwrap() {
        app.ui.dialog.as_mut().unwrap().fields.insert(k.clone(), v.clone());
    }
    draw(&mut app, &ctx);
    let before = undo_steps(&app);
    confirm(&mut app).unwrap();
    assert_eq!(undo_steps(&app), before + 1, "one undo step");
    let style = app.session.doc().unwrap().doc.styles.para("Ruled Child").cloned().unwrap();
    let own = serde_json::to_value(&style.para).unwrap();
    for (k, v) in fields.as_object().unwrap() {
        assert_eq!(own[k.trim_start_matches("p.")], *v, "the style sets {k}");
    }
    app.run("type.selectionAttrs", json!({})).unwrap()["para"].clone()
}

#[test]
fn paragraph_style_options_keep_and_border_sections_reach_the_styled_paragraph() {
    let para = style_section_edit(
        &["keep", "border"],
        json!({"p.keepWithNext": 2, "p.keepLinesTogether": true, "p.keepAllLines": false, "p.keepFirst": 3,
            "p.startParagraph": "nextPage", "p.borderOn": true, "p.borderWeights": [2.0, 0.0, 2.0, 0.0]}),
    );
    assert_eq!(para["keepWithNext"], 2);
    assert_eq!(para["keepLinesTogether"], true);
    assert_eq!(para["keepAllLines"], false);
    assert_eq!(para["keepFirst"], 3);
    assert_eq!(para["startParagraph"], "nextPage");
    assert_eq!(para["borderOn"], true);
    assert_eq!(para["borderWeights"], json!([2.0, 0.0, 2.0, 0.0]));
    // Fields not edited keep the resolved value.
    assert_eq!(para["keepLast"], 2);
    assert_eq!(para["ruleBelow"]["weight"], 3.0);
}

#[test]
fn paragraph_style_options_bullets_and_span_sections_reach_the_styled_paragraph() {
    let para = style_section_edit(
        &["bullets", "span"],
        json!({"p.listType": "numbers", "p.numberStyle": "upperRoman", "p.numberExpression": "^#)^t", "p.startAt": 3,
            "p.spanColumns": {"kind": "split", "columns": 3}}),
    );
    assert_eq!(para["listType"], "numbers");
    assert_eq!(para["numberStyle"], "upperRoman");
    assert_eq!(para["numberExpression"], "^#)^t");
    assert_eq!(para["startAt"], 3);
    assert_eq!(para["spanColumns"], json!({"kind": "split", "columns": 3}));
    let props: designcraft_doc::ParaProps = serde_json::from_value(para).unwrap();
    assert_eq!(props.number_label(3), "III)\t");
}

#[test]
fn paragraph_style_options_ignore_optical_margin_is_set_on_ok() {
    use egui_kittest::{Harness, kittest::Queryable};
    let mut app = DesignApp::new(designcraft_engine::Session::new(), crate::Services::default());
    app.run("file.new", json!({})).unwrap();
    app.run("style.paragraph.create", json!({"name": "Flush"})).unwrap();
    app.ui.dialog = Some(Dialog::new("paragraphStyleOptions", json!({"name": "Flush", "section": "indents"})));
    let mut h = Harness::builder().with_size(egui::vec2(1000.0, 800.0)).build_ui_state(
        |ui, app: &mut DesignApp| {
            let key = egui::Id::unique("ignore_optical_margin_fonts");
            if ui.data(|d| d.get_temp::<bool>(key)).is_none() {
                crate::theme::install_fonts(ui.ctx(), "");
                ui.data_mut(|d| d.insert_temp(key, true));
                return;
            }
            show(app, ui.ctx());
        },
        app,
    );
    h.run_steps(3);
    h.get_by_label("Ignore Optical Margin").click();
    h.run_steps(3);
    h.get_by_label("  OK  ").click();
    h.run_steps(3);
    let app = h.state();
    assert!(app.ui.dialog.is_none());
    assert_eq!(app.session.doc().unwrap().doc.styles.para("Flush").unwrap().para.ignore_optical_margin, Some(true));
}

#[test]
fn paragraph_style_options_keep_a_bad_grep_pattern_open_with_the_error() {
    let (mut app, _) = ruled_paragraph();
    let ctx = egui::Context::default();
    app.ui.dialog = Some(Dialog::new("paragraphStyleOptions", json!({"name": "Ruled", "section": "general"})));
    draw(&mut app, &ctx);
    app.ui.dialog.as_mut().unwrap().fields.insert("p.grepStyles".into(), json!([{"style": "[None]", "pattern": "(?<=x"}]));
    let e = confirm(&mut app).unwrap_err();
    assert!(e.contains("bad GREP"), "{e}");
    let d = app.ui.dialog.clone().expect("the dialog stays open");
    assert_eq!((d.s("section").as_str(), d.s("status").contains("bad GREP")), ("grep", true));
    assert!(app.session.doc().unwrap().doc.styles.para("Ruled").unwrap().para.grep_styles.is_none(), "nothing saved");
    draw(&mut app, &ctx);
}

#[test]
fn paragraph_style_options_show_a_refused_rename_on_general_and_trim_a_new_name() {
    let (mut app, _) = ruled_paragraph();
    let ctx = egui::Context::default();
    app.ui.dialog = Some(Dialog::new("paragraphStyleOptions", json!({"name": "Ruled", "section": "rules"})));
    draw(&mut app, &ctx);
    app.ui.dialog.as_mut().unwrap().fields.insert("rename".into(), json!("Ruled Child"));
    let e = confirm(&mut app).unwrap_err();
    let d = app.ui.dialog.clone().expect("the dialog stays open");
    assert_eq!(d.s("section"), "general", "the name field is on General");
    assert!(d.s("status").contains("already exists"), "{e}");

    app.ui.dialog.as_mut().unwrap().fields.insert("rename".into(), json!("  Fresh "));
    app.ui.dialog.as_mut().unwrap().fields.insert("x.tag".into(), json!("h3"));
    confirm(&mut app).unwrap();
    let st = &app.session.doc().unwrap().doc.styles;
    assert!(st.para("Fresh").is_some());
    assert_eq!(st.export_tag("Fresh", false).map(|t| t.tag.as_str()), Some("h3"));
}

/// A frame with two paragraphs of [Basic Paragraph]; the first has a local Word Spacing minimum.
fn two_paragraphs() -> (DesignApp, Value) {
    let mut app = DesignApp::new(designcraft_engine::Session::new(), crate::Services::default());
    app.run("file.new", json!({})).unwrap();
    let r = app.run("frame.create", json!({"rect": [72, 72, 300, 400], "content": "text", "text": "One\nTwo"})).unwrap();
    app.run("text.select", json!({"story": r["story"], "anchor": 0, "focus": 0})).unwrap();
    app.run("type.para", json!({"attrs": {"wordSpaceMin": 0.7}})).unwrap();
    (app, r["story"].clone())
}

/// The resolved paragraph attribute `key` of the paragraph at byte `at`.
fn para_attr(app: &mut DesignApp, story: &Value, at: usize, key: &str) -> Value {
    app.run("text.select", json!({"story": story, "anchor": at, "focus": at})).unwrap();
    app.run("type.selectionAttrs", json!({})).unwrap()["para"][key].clone()
}

/// The hyphenation video (Graphd Tutorials, 0:39–1:13): the Paragraph panel menu's
/// Hyphenation… opens InDesign's Hyphenation Settings for the selected paragraphs. Preview
/// shows the slider moved to Fewer Hyphens on the page without an undo step, OK makes it one
/// step on the selected paragraph only, and Cancel puts it back.
#[test]
fn hyphenation_settings_preview_apply_and_cancel_on_the_selected_paragraph() {
    let (mut app, story) = two_paragraphs();
    let ctx = dialog_context("en");
    let screen = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(1400.0, 900.0));
    let sid = designcraft_doc::StoryId(story.as_u64().unwrap());
    let weight = |app: &DesignApp, para: usize| {
        let d = &app.session.doc().unwrap().doc;
        d.styles.resolve_para(&d.story(sid).unwrap().paras[para]).0.hyph_weight
    };
    assert!(crate::menus::menu_enabled(&app, "app.hyphenationDialog"));
    app.run("app.hyphenationDialog", json!({})).unwrap();
    for _ in 0..3 {
        dialog_frame(&mut app, &ctx, screen, vec![]);
    }
    let labels = dialog_frame(&mut app, &ctx, screen, vec![]);
    let d = app.ui.dialog.clone().unwrap();
    assert_eq!(
        (d.id.as_str(), d.fields["current"]["hyphWeight"].as_f64(), d.fields["current"]["hyphenate"].as_bool()),
        ("hyphenation", Some(0.5), Some(true))
    );
    for label in [
        "Hyphenation Settings",
        "Hyphenate",
        "Words with at Least:",
        "letters",
        "Hyphen Limit:",
        "hyphens",
        "Hyphenation Zone:",
        "Better Spacing",
        "Fewer Hyphens",
        "Hyphenate Capitalized Words",
        "Hyphenate Last Word",
        "Hyphenate Across Column",
        "Preview",
    ] {
        assert!(labels.iter().any(|(t, _, _)| t == label), "{label}: {labels:?}");
    }
    // Fewer Hyphens (9 of 10) with Preview: on the page, no undo step.
    let before = undo_steps(&app);
    let d = app.ui.dialog.as_mut().unwrap();
    d.fields.insert("hyphWeight".into(), json!(0.9));
    d.fields.insert("preview".into(), json!(true));
    dialog_frame(&mut app, &ctx, screen, vec![]);
    assert_eq!(weight(&app, 0), 0.9, "previewed");
    assert_eq!(undo_steps(&app), before, "a preview is no step");
    // Cancel: back as it was.
    cancel(&mut app);
    assert_eq!(weight(&app, 0), 0.5);
    // Again, and OK: one step, the selected paragraph only.
    app.run("app.hyphenationDialog", json!({})).unwrap();
    let d = app.ui.dialog.as_mut().unwrap();
    d.fields.insert("hyphWeight".into(), json!(0.9));
    d.fields.insert("preview".into(), json!(true));
    dialog_frame(&mut app, &ctx, screen, vec![]);
    confirm(&mut app).unwrap();
    assert_eq!((weight(&app, 0), weight(&app, 1)), (0.9, 0.5));
    assert_eq!(undo_steps(&app), before + 1, "one undo step");
    // Paragraph Style Options › Hyphenation has the same form.
    app.ui.dialog = Some(Dialog::new("paragraphStyleOptions", json!({"name": designcraft_doc::story::BASIC_PARAGRAPH, "section": "hyph"})));
    for _ in 0..3 {
        dialog_frame(&mut app, &ctx, screen, vec![]);
    }
    let labels = dialog_frame(&mut app, &ctx, screen, vec![]);
    for label in ["Hyphenation Zone:", "Better Spacing", "Fewer Hyphens", "Hyphenate Last Word", "Hyphenate Across Column"] {
        assert!(labels.iter().any(|(t, _, _)| t == label), "style options: {label}");
    }
    app.ui.dialog = None;
    app.run("edit.deselectAll", json!({})).unwrap();
    assert!(!crate::menus::menu_enabled(&app, "app.hyphenationDialog"), "no paragraph");
}

/// Hyphenate differs across the selection: it shows mixed, and the first click turns it on for
/// all. Settings the command refuses show why: a preview goes back and isn't retried until they
/// change, and OK keeps the dialog open.
/// A mixed Better Spacing ↔ Fewer Hyphens slider: a click on its middle step sets every
/// selected paragraph to it, though the knob already sat there.
#[test]
fn hyphenation_settings_mixed_slider_click_sets_the_step() {
    let (mut app, story) = two_paragraphs();
    let ctx = dialog_context("en");
    let screen = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(1400.0, 900.0));
    app.run("type.para", json!({"attrs": {"hyphWeight": 0.2}})).unwrap();
    app.run("text.select", json!({"story": story, "anchor": 0, "focus": 7})).unwrap();
    app.run("app.hyphenationDialog", json!({})).unwrap();
    for _ in 0..3 {
        dialog_frame(&mut app, &ctx, screen, vec![]);
    }
    let labels = dialog_frame(&mut app, &ctx, screen, vec![]);
    let rect = |l: &str| labels.iter().find(|(t, _, _)| t == l).map(|(_, r, _)| *r).unwrap();
    let (better, fewer) = (rect("Better Spacing"), rect("Fewer Hyphens"));
    click_dialog(&mut app, &ctx, screen, egui::pos2((better.right() + fewer.left()) / 2.0, better.center().y));
    assert_eq!(app.ui.dialog.as_ref().unwrap().fields.get("hyphWeight"), Some(&json!(0.5)));
}

#[test]
fn hyphenation_settings_mixed_checkbox_and_refused_settings() {
    let (mut app, story) = two_paragraphs();
    let ctx = dialog_context("en");
    let screen = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(1400.0, 900.0));
    app.run("type.para", json!({"attrs": {"hyphenate": false}})).unwrap();
    app.run("text.select", json!({"story": story, "anchor": 0, "focus": 7})).unwrap();
    app.run("app.hyphenationDialog", json!({})).unwrap();
    assert!(app.ui.dialog.as_ref().unwrap().fields["current"]["hyphenate"].is_null(), "mixed");
    for _ in 0..3 {
        dialog_frame(&mut app, &ctx, screen, vec![]);
    }
    let labels = dialog_frame(&mut app, &ctx, screen, vec![]);
    let at = labels.iter().find(|(t, _, _)| t == "Hyphenate").map(|(_, r, _)| r.left_center() - egui::vec2(10.0, 0.0)).unwrap();
    click_dialog(&mut app, &ctx, screen, at);
    assert_eq!(app.ui.dialog.as_ref().unwrap().fields.get("hyphenate"), Some(&json!(true)), "the first click turns it on");
    // A refused setting (as a control-channel caller could send it): no preview, the reason.
    let before = undo_steps(&app);
    let d = app.ui.dialog.as_mut().unwrap();
    d.fields.insert("hyphWeight".into(), json!("heavy"));
    d.fields.insert("preview".into(), json!(true));
    dialog_frame(&mut app, &ctx, screen, vec![]);
    let d = app.ui.dialog.as_ref().unwrap();
    assert!(!d.fields.contains_key(PREVIEWED) && d.fields.contains_key("status"), "{:?}", d.fields);
    assert!(app.session.doc().unwrap().interaction.is_none(), "the preview went back");
    dialog_frame(&mut app, &ctx, screen, vec![]);
    assert!(app.session.doc().unwrap().interaction.is_none(), "not tried again");
    // OK with it: an error, and the dialog stays open with the reason.
    assert!(confirm(&mut app).is_err());
    let d = app.ui.dialog.as_ref().expect("the dialog stays open");
    assert!(d.fields.contains_key("status"));
    assert_eq!(undo_steps(&app), before);
    // Corrected, it previews and applies.
    app.ui.dialog.as_mut().unwrap().fields.insert("hyphWeight".into(), json!(0.9));
    dialog_frame(&mut app, &ctx, screen, vec![]);
    assert!(!app.ui.dialog.as_ref().unwrap().fields.contains_key("status"));
    confirm(&mut app).unwrap();
    assert_eq!(para_attr(&mut app, &story, 0, "hyphenate"), true);
    assert_eq!(para_attr(&mut app, &story, 4, "hyphWeight"), 0.9);
}

#[test]
fn justification_dialog_applies_to_a_local_paragraph_and_cancel_changes_nothing() {
    let (mut app, story) = two_paragraphs();
    let ctx = egui::Context::default();
    assert!(crate::menus::menu_enabled(&app, "app.justificationDialog"));
    app.run("app.justificationDialog", json!({})).unwrap();
    draw(&mut app, &ctx);
    let d = app.ui.dialog.clone().unwrap();
    assert_eq!(d.id, "justification");
    assert_eq!(d.fields["current"]["wordSpaceMin"], 0.7);
    assert_eq!(d.fields["current"]["wordSpaceDesired"], 1.0);
    app.ui.dialog.as_mut().unwrap().fields.insert("wordSpaceDesired".into(), json!(1.1));
    draw(&mut app, &ctx);
    let before = undo_steps(&app);
    confirm(&mut app).unwrap();
    assert_eq!(undo_steps(&app), before + 1, "one undo step");
    assert_eq!(para_attr(&mut app, &story, 0, "wordSpaceDesired"), 1.1);
    assert_eq!(para_attr(&mut app, &story, 0, "wordSpaceMin"), 0.7, "unedited fields stay");
    assert_eq!(para_attr(&mut app, &story, 4, "wordSpaceDesired"), 1.0, "only the selected paragraph");
    let style = app.session.doc().unwrap().doc.styles.resolve_para_style(designcraft_doc::story::BASIC_PARAGRAPH).0.word_space_desired;
    assert_eq!(style, 1.0, "the paragraph style keeps its value");

    // Cancel (Escape) leaves the paragraph alone.
    app.run("text.select", json!({"story": story, "anchor": 0, "focus": 0})).unwrap();
    app.run("app.justificationDialog", json!({})).unwrap();
    app.ui.dialog.as_mut().unwrap().fields.insert("wordSpaceDesired".into(), json!(1.2));
    let before = undo_steps(&app);
    let escape = egui::Event::Key { key: egui::Key::Escape, physical_key: None, pressed: true, repeat: false, modifiers: Default::default() };
    let input = egui::RawInput {
        screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(1400.0, 900.0))),
        events: vec![escape],
        ..Default::default()
    };
    let mut out = ctx.run_ui(input, |ui| {
        app.logic(&ui.ctx().clone());
        app.ui(ui);
    });
    out.textures_delta.clear();
    assert!(app.ui.dialog.is_none(), "cancelled");
    assert_eq!(undo_steps(&app), before);
    assert_eq!(para_attr(&mut app, &story, 0, "wordSpaceDesired"), 1.1);

    app.run("edit.deselectAll", json!({})).unwrap();
    assert!(!crate::menus::menu_enabled(&app, "app.justificationDialog"), "nothing to justify");
    assert!(app.run("app.justificationDialog", json!({})).is_err());
}

#[test]
fn justification_dialog_leaves_mixed_fields_alone_and_refuses_bad_values() {
    let (mut app, story) = two_paragraphs();
    let ctx = egui::Context::default();
    app.run("text.select", json!({"story": story, "anchor": 0, "focus": 7})).unwrap();
    app.run("app.justificationDialog", json!({})).unwrap();
    draw(&mut app, &ctx);
    let d = app.ui.dialog.clone().unwrap();
    assert_eq!(d.fields["current"]["wordSpaceMin"], Value::Null, "mixed shows empty");
    assert_eq!(d.fields["current"]["wordSpaceMax"], 1.33);

    // Out of range, and minimum above desired: an error, the dialog stays open, nothing changes.
    let before = undo_steps(&app);
    for (key, v) in [("wordSpaceMax", 20.0), ("glyphScaleMin", 0.2), ("letterSpaceDesired", f64::NAN)] {
        app.ui.dialog = Some(d.clone());
        app.ui.dialog.as_mut().unwrap().fields.insert(key.into(), json!(v));
        assert!(confirm(&mut app).is_err(), "{key} = {v}");
        assert!(app.ui.dialog.as_ref().is_some_and(|d| d.fields.contains_key("justifyError")), "{key}: the dialog shows why");
    }
    app.ui.dialog = Some(d.clone());
    app.ui.dialog.as_mut().unwrap().fields.insert("letterSpaceMin".into(), json!(0.1));
    assert!(confirm(&mut app).is_err(), "letter spacing minimum above desired");
    draw(&mut app, &ctx);
    assert_eq!(undo_steps(&app), before);

    app.ui.dialog = Some(d);
    app.ui.dialog.as_mut().unwrap().fields.insert("wordSpaceMax".into(), json!(1.5));
    confirm(&mut app).unwrap();
    assert_eq!(undo_steps(&app), before + 1);
    for (at, min) in [(0, 0.7), (4, 0.8)] {
        assert_eq!(para_attr(&mut app, &story, at, "wordSpaceMax"), 1.5);
        assert_eq!(para_attr(&mut app, &story, at, "wordSpaceMin"), min, "each paragraph keeps its own minimum");
    }
}

fn command_app(id: &str, fields: Value) -> DesignApp {
    let mut app = DesignApp::new(designcraft_engine::Session::new(), crate::Services::default());
    app.ui.dialog = Some(Dialog::new(&format!("cmd:{id}"), fields));
    app
}

#[test]
fn generic_new_document_preserves_typed_values_and_text_entry() {
    for pages in [json!(2), json!(" 2 ")] {
        let mut app = command_app(
            "file.new",
            json!({
                "preset": "A4", "pages": pages, "facingPages": false, "gutter": 0,
                "margins": {"top": 10, "bottom": 20, "inside": 30, "outside": 40},
                "title": "  ", "status": "internal", "primaryTextFrame": true
            }),
        );
        confirm(&mut app).unwrap();
        let doc = &app.session.active().unwrap().doc;
        assert_eq!(doc.page_count(), 2);
        assert!(!doc.settings.facing_pages);
        assert!(doc.title.starts_with("Untitled-"));
        for page in doc.spreads.iter().flat_map(|s| &s.pages) {
            assert!((page.width - 595.2755905511812).abs() < 0.01);
            assert!((page.height - 841.8897637795277).abs() < 0.01);
            assert_eq!(page.columns.gutter, 0.0);
            assert_eq!(page.margins.top, 10.0);
            assert_eq!(page.margins.outside, 40.0);
        }
        // `primaryTextFrame` is documented since #776, so the dialog passes it on (`status` isn't).
        assert!(doc.spreads.iter().any(|s| !s.items.is_empty()), "documented fields reach the command");
        assert!(app.ui.dialog.is_none());
    }
}

#[test]
fn numbering_commands_document_their_fields() {
    let names = |id: &str| command_fields(designcraft_engine::find_command(id).unwrap().params).into_iter().map(|f| f.key).collect::<Vec<_>>();
    assert_eq!(names("layout.section"), ["page", "startNumber", "style", "prefix", "includePrefix", "marker", "remove"]);
    assert_eq!(names("layout.chapterNumbering"), ["style", "start", "source"]);
}

#[test]
fn generic_command_preserves_array_geometry() {
    let mut app = command_app("file.new", json!({}));
    confirm(&mut app).unwrap();
    app.ui.dialog = Some(Dialog::new("cmd:frame.create", json!({"rect": [10, 20, 110, 220]})));
    confirm(&mut app).unwrap();
    let doc = &app.session.active().unwrap().doc;
    let item = doc.spreads.iter().flat_map(|s| &s.items).next().unwrap();
    assert_eq!(item.bounds(), designcraft_geom::Rect::new(10.0, 20.0, 110.0, 220.0));
}

#[test]
fn generic_explicit_null_uses_the_commands_existing_semantics() {
    // file.new accepts null width by using the selected preset's width.
    for width in [Value::Null, json!("null")] {
        let mut app = command_app("file.new", json!({"preset": "A4", "width": width, "title": "  Sample layout  "}));
        confirm(&mut app).unwrap();
        let doc = &app.session.active().unwrap().doc;
        assert_eq!(doc.title, "Sample layout");
        assert!((doc.settings.page_width - 595.2755905511812).abs() < 0.01);
    }
}

#[test]
fn generic_invalid_number_retains_dialog_without_creating_document() {
    let mut app = command_app("file.new", json!({"width": 0}));
    assert!(confirm(&mut app).is_err());
    assert!(app.session.active().is_none());
    let dialog = app.ui.dialog.as_ref().unwrap();
    assert_eq!(dialog.fields["width"], 0);
    assert!(dialog.fields["status"].as_str().unwrap().contains("page size out of range"));
}

#[test]
fn style_options_preview_choice_is_remembered_on_reopen() {
    use egui_kittest::{Harness, kittest::Queryable};
    let mut app = DesignApp::new(designcraft_engine::Session::new(), crate::Services::default());
    app.run("file.new", json!({})).unwrap();
    app.run("style.paragraph.create", json!({"name":"Tutorial","chars":{"size":10}})).unwrap();
    open_style_options(&mut app, true, "Tutorial");
    let mut h = Harness::builder().with_size(egui::vec2(1000.0, 800.0)).build_ui_state(
        |ui, app: &mut DesignApp| {
            let key = egui::Id::unique("style_preview_remember_fonts");
            if ui.data(|d| d.get_temp::<bool>(key)).is_none() {
                crate::theme::install_fonts(ui.ctx(), "");
                ui.data_mut(|d| d.insert_temp(key, true));
                return;
            }
            show(app, ui.ctx());
        },
        app,
    );
    h.run_steps(3);
    h.get_by_label("Preview").click();
    h.run_steps(3);
    cancel(h.state_mut());
    open_style_options(h.state_mut(), true, "Tutorial");
    assert!(h.state().ui.dialog.as_ref().unwrap().b("preview"), "Preview stays checked on reopen");
}

#[test]
fn style_options_preview_updates_all_uses_cancels_and_commits_once() {
    use egui_kittest::{Harness, kittest::Queryable};
    for paragraph in [true, false] {
        let mut app = DesignApp::new(designcraft_engine::Session::new(), crate::Services::default());
        app.run("file.new", json!({})).unwrap();
        let f = app.run("frame.create", json!({"rect":[36,36,300,300],"content":"text","text":"First\nSecond"})).unwrap();
        app.run("text.select", json!({"story":f["story"],"anchor":0,"focus":12})).unwrap();
        let kind = if paragraph { "paragraph" } else { "character" };
        app.run(&format!("style.{kind}.create"), json!({"name":"Tutorial","chars":{"size":10}})).unwrap();
        app.run(&format!("style.{kind}.apply"), json!({"name":"Tutorial"})).unwrap();
        let before = app.session.doc().unwrap().doc.clone();
        let undo = app.session.doc().unwrap().history.undo.len();
        let journal = app.session.journal.len();
        open_style_options(&mut app, paragraph, "Tutorial");
        let mut h = Harness::builder().with_size(egui::vec2(1000.0, 800.0)).build_ui_state(
            |ui, app: &mut DesignApp| {
                let key = egui::Id::unique("style_preview_test_fonts");
                if ui.data(|d| d.get_temp::<bool>(key)).is_none() {
                    crate::theme::install_fonts(ui.ctx(), "");
                    ui.data_mut(|d| d.insert_temp(key, true));
                    return;
                }
                show(app, ui.ctx());
            },
            app,
        );
        h.run_steps(3);
        h.get_by_label("Preview").click();
        h.run_steps(3);
        h.state_mut().ui.dialog.as_mut().unwrap().fields.insert("c.size".into(), json!(18));
        h.run_steps(3);
        let app = h.state_mut();
        assert_eq!(app.run("type.selectionAttrs", json!({})).unwrap()["chars"]["size"], 18.0);
        assert_eq!(app.session.doc().unwrap().history.undo.len(), undo);
        assert_eq!(app.session.journal.len(), journal);
        // Repeated previews replace each other, never accumulate undo entries.
        app.ui.dialog.as_mut().unwrap().fields.insert("c.size".into(), json!(20));
        h.run_steps(3);
        assert_eq!(h.state_mut().run("type.selectionAttrs", json!({})).unwrap()["chars"]["size"], 20.0);
        h.get_by_label("Preview").click();
        h.run_steps(3);
        assert_eq!(h.state().session.doc().unwrap().doc, before);
        h.get_by_label("Preview").click();
        h.run_steps(3);
        cancel(h.state_mut());
        assert_eq!(h.state().session.doc().unwrap().doc, before);
        assert_eq!(h.state().session.doc().unwrap().history.undo.len(), undo);
        open_style_options(h.state_mut(), paragraph, "Tutorial");
        let d = h.state_mut().ui.dialog.as_mut().unwrap();
        d.fields.insert("preview".into(), json!(true));
        d.fields.insert("c.size".into(), json!(24));
        h.run_steps(3);
        confirm(h.state_mut()).unwrap();
        assert_eq!(h.state().session.doc().unwrap().history.undo.len(), undo + 1);
        // Both paragraphs/character runs still use the globally edited definition.
        for pos in [0, 6] {
            h.state_mut().run("text.select", json!({"story":f["story"],"anchor":pos,"focus":pos+1})).unwrap();
            assert_eq!(h.state_mut().run("type.selectionAttrs", json!({})).unwrap()["chars"]["size"], 24.0);
        }
        h.state_mut().run("edit.undo", json!({})).unwrap();
        assert_eq!(h.state().session.doc().unwrap().doc, before);
    }
}

#[test]
fn style_preview_refuses_justification_spacing_ok_would_refuse() {
    let mut app = DesignApp::new(designcraft_engine::Session::new(), crate::Services::default());
    app.run("file.new", json!({})).unwrap();
    let before = app.session.doc().unwrap().doc.clone();
    let mut d = Dialog::new("paragraphStyleOptions", json!({"name": designcraft_doc::BASIC_PARAGRAPH, "preview": true, "p.wordSpaceDesired": 1.1}));
    style_options_preview(&mut app, &mut d);
    assert_ne!(app.session.doc().unwrap().doc, before, "a valid value previews");
    // Minimum above Desired: the page goes back and the dialog says why.
    d.fields.insert("p.wordSpaceMin".into(), json!(1.5));
    style_options_preview(&mut app, &mut d);
    assert_eq!(app.session.doc().unwrap().doc, before);
    assert!(d.s("status").contains("Word Spacing"), "{:?}", d.fields.get("status"));
    assert!(app.session.doc().unwrap().interaction.is_none());
}

#[test]
fn invalid_style_preview_leaves_the_starting_document_and_can_be_cancelled() {
    let mut app = DesignApp::new(designcraft_engine::Session::new(), crate::Services::default());
    app.run("file.new", json!({})).unwrap();
    app.run("style.character.create", json!({"name":"Emphasis"})).unwrap();
    let before = app.session.doc().unwrap().doc.clone();
    let mut d = Dialog::new("characterStyleOptions", json!({"name":"Emphasis","preview":true,"basedOn":"Emphasis"}));
    style_options_preview(&mut app, &mut d);
    assert!(d.fields.contains_key("status"));
    assert_eq!(app.session.doc().unwrap().doc, before);
    app.ui.dialog = Some(d);
    cancel(&mut app);
    assert_eq!(app.session.doc().unwrap().doc, before);
    assert!(app.session.doc().unwrap().interaction.is_none());
}

#[test]
fn style_preview_rejects_conflicting_scalar_and_nested_rule_fields_without_panicking() {
    let mut app = DesignApp::new(designcraft_engine::Session::new(), crate::Services::default());
    app.run("file.new", json!({})).unwrap();
    let before = app.session.doc().unwrap().doc.clone();
    let mut dialog = Dialog::new(
        "paragraphStyleOptions",
        json!({
            "name": designcraft_doc::BASIC_PARAGRAPH, "preview": true,
            "p.ruleAbove": false, "p.ruleAbove.weight": 2
        }),
    );
    style_options_preview(&mut app, &mut dialog);
    assert!(dialog.fields.contains_key("status"));
    assert_eq!(app.session.doc().unwrap().doc, before);
    app.ui.dialog = Some(dialog);
    cancel(&mut app);
    assert!(app.session.doc().unwrap().interaction.is_none());
}

#[test]
fn text_frame_options_set_columns_baseline_and_wrap_as_indesign_does() {
    let mut app = crate::DesignApp::new(designcraft_engine::Session::new(), crate::Services::default());
    app.run("file.new", json!({})).unwrap();
    // A 300 pt frame, 2 columns, 12 pt gutter (picas in the dialog).
    let r = app.run("frame.create", json!({"rect": [36, 36, 336, 236], "content": "text", "text": "Hello"})).unwrap();
    let id = designcraft_doc::ItemId(r["id"].as_u64().unwrap());
    app.run("object.textFrameOptions", json!({"ids": [id.0], "columns": 2, "gutter": 12})).unwrap();
    let opts = |app: &DesignApp| app.session.doc().unwrap().doc.item(id).unwrap().text_frame().unwrap().options.clone();
    let mut d = Dialog::new("textFrameOptions", json!({}));
    seed_text_frame_options(&app, &mut d);
    // Fixed Number shows each column's width: (300 - 12) / 2 = 144 pt.
    assert_eq!((d.s("columnsKind"), d.s("columnWidth"), d.s("firstBaseline")), ("fixedNumber".into(), "12p0".into(), "ascent".into()));
    for (k, v) in [
        ("columnsKind", json!("fixedWidth")),
        ("columnWidth", json!("10p0")),
        ("firstBaseline", json!("capHeight")),
        ("firstBaselineMin", json!("0p6")),
        ("balanceColumns", json!(true)),
        ("ignoreWrap", json!(true)),
    ] {
        d.fields.insert(k.into(), v);
    }
    app.ui.dialog = Some(d);
    confirm(&mut app).unwrap();
    let o = opts(&app);
    assert_eq!(o.columns_kind, designcraft_doc::ColumnsKind::FixedWidth);
    assert_eq!((o.column_width, o.first_baseline_min, o.columns), (120.0, 6.0, 2));
    assert_eq!(o.first_baseline, designcraft_doc::FirstBaseline::CapHeight);
    assert!(o.balance_columns && o.ignore_wrap);
    // Fixed Number again: the width isn't sent.
    let mut d = Dialog::new("textFrameOptions", json!({}));
    seed_text_frame_options(&app, &mut d);
    assert_eq!(d.s("columnWidth"), "10p0", "Fixed Width shows its own width");
    d.fields.insert("columnsKind".into(), json!("fixedNumber"));
    app.ui.dialog = Some(d);
    confirm(&mut app).unwrap();
    assert_eq!((opts(&app).columns_kind, opts(&app).column_width), (designcraft_doc::ColumnsKind::FixedNumber, 120.0));
}

#[test]
fn baseline_grid_increment_error_explains_the_positive_measure_rule() {
    let mut app = crate::DesignApp::new(designcraft_engine::Session::new(), crate::Services::default());
    app.run("file.new", json!({})).unwrap();
    let r = app.run("frame.create", json!({"rect": [36, 36, 336, 236], "content": "text", "text": "Hello"})).unwrap();
    let id = designcraft_doc::ItemId(r["id"].as_u64().unwrap());
    for increment in ["0", "-1 pt", "not a measure"] {
        let mut d = Dialog::new("textFrameOptions", json!({}));
        seed_text_frame_options(&app, &mut d);
        d.fields.insert("customGrid".into(), json!(true));
        d.fields.insert("gridIncrement".into(), json!(increment));
        app.ui.dialog = Some(d);
        let error = confirm(&mut app).unwrap_err().to_string();
        assert!(error.contains("the baseline grid increment must be a measure greater than zero"), "{increment}: {error}");
        assert!(app.session.doc().unwrap().doc.item(id).unwrap().text_frame().unwrap().options.baseline_grid.is_none());
    }
}

#[test]
fn text_frame_options_set_a_custom_baseline_grid_relative_to() {
    use designcraft_doc::{FrameBaselineGrid, FrameGridRelative};
    let mut app = crate::DesignApp::new(designcraft_engine::Session::new(), crate::Services::default());
    app.run("file.new", json!({})).unwrap();
    let r = app.run("frame.create", json!({"rect": [36, 36, 336, 236], "content": "text", "text": "Hello"})).unwrap();
    let id = designcraft_doc::ItemId(r["id"].as_u64().unwrap());
    let grid = |app: &DesignApp| app.session.doc().unwrap().doc.item(id).unwrap().text_frame().unwrap().options.baseline_grid;
    let open = |app: &mut DesignApp, fields: &[(&str, Value)]| {
        let mut d = Dialog::new("textFrameOptions", json!({}));
        seed_text_frame_options(app, &mut d);
        for (k, v) in fields {
            d.fields.insert((*k).into(), v.clone());
        }
        app.ui.dialog = Some(d);
        confirm(app)
    };
    app.run("selection.set", json!({"ids": [id.0]})).unwrap();
    // Turned on, the grid takes the shown start and increment (the document grid's: 3p0 and 1p0).
    open(&mut app, &[("customGrid", json!(true)), ("gridRelativeTo", json!("frame"))]).unwrap();
    assert_eq!(grid(&app), Some(FrameBaselineGrid { start: 36.0, increment: 12.0, relative_to: FrameGridRelative::TopOfFrame }));
    // The dialog shows the frame's grid; changing Relative To keeps its start and increment.
    open(&mut app, &[("gridStart", json!("0p10"))]).unwrap();
    open(&mut app, &[("gridRelativeTo", json!("inset"))]).unwrap();
    assert_eq!(grid(&app), Some(FrameBaselineGrid { start: 10.0, increment: 12.0, relative_to: FrameGridRelative::TopInset }));
    open(&mut app, &[("customGrid", json!(false))]).unwrap();
    assert_eq!(grid(&app), None);
}

#[test]
fn text_frame_options_change_only_what_was_edited() {
    let mut app = crate::DesignApp::new(designcraft_engine::Session::new(), crate::Services::default());
    app.run("file.new", json!({})).unwrap();
    let r = app.run("frame.create", json!({"rect": [36, 36, 336, 236], "content": "text", "text": "Hello"})).unwrap();
    let id = designcraft_doc::ItemId(r["id"].as_u64().unwrap());
    app.run("object.textFrameOptions", json!({"ids": [id.0], "columns": 2, "inset": [1, 2, 3, 4]})).unwrap();
    let opts = |app: &DesignApp| app.session.doc().unwrap().doc.item(id).unwrap().text_frame().unwrap().options.clone();
    let before = opts(&app);
    let open = |app: &mut DesignApp, fields: &[(&str, Value)]| {
        app.ui.dialog = Some(Dialog::new("textFrameOptions", json!({})));
        let mut d = app.ui.dialog.take().unwrap();
        seed_text_frame_options(app, &mut d);
        for (k, v) in fields {
            d.fields.insert((*k).into(), v.clone());
        }
        app.ui.dialog = Some(d);
        confirm(app)
    };
    // OK without edits changes nothing (no undo step either).
    assert_eq!(open(&mut app, &[]).unwrap(), Value::Null);
    assert_eq!(opts(&app), before);
    // The rule's weight is in points; unequal insets the user didn't touch stay.
    open(&mut app, &[("columnRule", json!(true)), ("columnRuleWeight", json!("6"))]).unwrap();
    let o = opts(&app);
    assert!(o.column_rule && o.column_rule_weight == 6.0, "{o:?}");
    assert_eq!((o.inset, o.columns, o.gutter), (before.inset, before.columns, before.gutter));
    // One of the rule's insets edited: the other rule settings stay.
    open(&mut app, &[("columnRuleBottomInset", json!("0p9")), ("columnRuleTint", json!("40"))]).unwrap();
    let o = opts(&app);
    assert_eq!((o.column_rule_bottom_inset, o.column_rule_top_inset, o.column_rule_offset, o.column_rule_weight), (9.0, 0.0, 0.0, 6.0));
    assert!((o.column_rule_tint - 0.4).abs() < 1e-6 && o.column_rule);
    // One inset side edited: the others keep their values.
    open(&mut app, &[("insetLeft", json!("1p0"))]).unwrap();
    assert_eq!(opts(&app).inset, [1.0, 12.0, 3.0, 4.0]);
    open(&mut app, &[("balanceColumns", json!(true))]).unwrap();
    let o = opts(&app);
    assert!(o.balance_columns && o.column_rule && o.columns == 2, "{o:?}");
    // An unknown swatch is an error and the dialog stays open.
    let e = open(&mut app, &[("columnRuleColor", json!("Mauve"))]).unwrap_err();
    assert!(e.contains("Mauve") && app.ui.dialog.is_some(), "{e}");
    assert_eq!(opts(&app).column_rule_color, designcraft_color::swatch::BLACK);
    // Each OK is one undo step.
    for _ in 0..4 {
        app.run("edit.undo", json!({})).unwrap();
    }
    assert_eq!(opts(&app), before);
}

#[test]
fn toc_dialog_adds_a_style_and_ok_makes_the_table_of_contents() {
    let mut app = DesignApp::new(designcraft_engine::Session::new(), crate::Services::default());
    app.run("file.newSample", json!({})).unwrap();
    crate::menus::activate(&mut app, "toc.generate", &Value::Null);
    assert_eq!(app.ui.dialog.as_ref().map(|d| d.id.as_str()), Some(TOC));
    let ctx = dialog_context("en");
    let screen = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(1200.0, 800.0));
    dialog_frame(&mut app, &ctx, screen, vec![]);
    let labels = dialog_frame(&mut app, &ctx, screen, vec![]);
    for expected in
        ["Table of Contents", "Title:", "Include Paragraph Styles:", "Other Styles:", "« Add", "Remove »", "Replace Existing Table of Contents"]
    {
        visible_label(&labels, screen, expected);
    }
    click_dialog(&mut app, &ctx, screen, visible_label(&labels, screen, "Headline").center());
    let labels = dialog_frame(&mut app, &ctx, screen, vec![]);
    click_dialog(&mut app, &ctx, screen, visible_label(&labels, screen, "« Add").center());
    let labels = dialog_frame(&mut app, &ctx, screen, vec![]);
    for expected in ["Entry Style:", "TOC Level 1", "After Entry", "Between Entry and Number:", "Level:"] {
        visible_label(&labels, screen, expected);
    }
    let undo = app.session.doc().unwrap().history.undo.len();
    click_dialog(&mut app, &ctx, screen, visible_label(&labels, screen, "OK").center());
    assert!(app.ui.dialog.is_none());
    let d = &app.session.doc().unwrap().doc;
    let toc = d.toc.clone().unwrap();
    assert_eq!(toc.entries, vec![designcraft_doc::TocEntry::new("Headline", 1)]);
    let text = &d.story(toc.story).unwrap().text;
    assert!(text.starts_with("Contents\n") && text.contains("\nNotes on the Grid\t"), "{text}");
    assert_eq!(app.session.doc().unwrap().history.undo.len(), undo + 1);
    // Reopened, the dialog shows the table of contents and replaces it.
    crate::menus::activate(&mut app, "toc.generate", &Value::Null);
    let d = app.ui.dialog.as_ref().unwrap();
    assert!(d.b("replace"));
    assert_eq!(toc_rows(d).iter().map(|r| r.style.as_str()).collect::<Vec<_>>(), ["Headline"]);
}

#[test]
fn margins_and_columns_link_arithmetic_preview_and_one_undo_step() {
    // Adobe's "Set up a modern magazine layout" (1:41–3:04): linked margins to 0.4375 in, then
    // 5 columns with a 0.1875 in gutter, unlinked, and "*2" typed after the outside margin.
    let mut app = DesignApp::new(designcraft_engine::Session::new(), crate::Services::default());
    app.run("file.new", json!({"preset": "A4", "pages": 3, "facingPages": true, "margins": 36})).unwrap();
    app.canvas_rect = Some(egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(1000.0, 800.0)));
    open_margins_and_columns(&mut app);
    let ctx = dialog_context("en");
    let screen = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(1178.0, 814.0));
    let mut labels = vec![];
    for _ in 0..3 {
        labels = dialog_frame(&mut app, &ctx, screen, vec![]);
    }
    for expected in
        ["Margins and Columns", "Margins", "Top:", "Bottom:", "Inside:", "Outside:", "Columns", "Number:", "Gutter:", "Adjust Layout", "Preview"]
    {
        visible_label(&labels, screen, expected);
    }
    let d = app.ui.dialog.as_ref().unwrap();
    assert!(d.b("link"), "four equal margins open linked");
    // Linked: one margin typed is all four.
    app.ui.dialog.as_mut().unwrap().fields.insert("marginTop".into(), json!("0.4375 in"));
    dialog_frame(&mut app, &ctx, screen, vec![]);
    let d = app.ui.dialog.as_ref().unwrap();
    assert!(MARGIN_FIELDS.iter().all(|(k, _, _)| d.s(k) == "0.4375 in"), "{:?}", d.fields);
    // Preview: the page changes, with no undo step.
    let undo = app.session.doc().unwrap().history.undo.len();
    app.ui.dialog.as_mut().unwrap().fields.insert("preview".into(), json!(true));
    dialog_frame(&mut app, &ctx, screen, vec![]);
    let top = |app: &DesignApp| app.session.doc().unwrap().doc.spreads[0].pages[0].margins.top;
    assert!((top(&app) - 31.5).abs() < 1e-6, "previewed: {}", top(&app));
    assert_eq!(app.session.doc().unwrap().history.undo.len(), undo);
    // Unlinked (the link button), the outside margin alone: "*2" typed after it is worked out
    // when the field is left.
    app.ui.dialog.as_mut().unwrap().fields.insert("link".into(), json!(false));
    let labels = dialog_frame(&mut app, &ctx, screen, vec![]);
    let key = |k| egui::Event::Key { key: k, physical_key: None, pressed: true, repeat: false, modifiers: egui::Modifiers::NONE };
    let field = labels
        .iter()
        .filter(|(t, _, _)| t.trim() == "0.4375 in")
        .map(|(_, r, _)| *r)
        .max_by(|a, b| (a.max.x + a.max.y).total_cmp(&(b.max.x + b.max.y)))
        .unwrap();
    click_dialog(&mut app, &ctx, screen, field.center());
    dialog_frame(&mut app, &ctx, screen, vec![key(egui::Key::End), egui::Event::Text("*2".into())]);
    dialog_frame(&mut app, &ctx, screen, vec![key(egui::Key::Tab)]);
    dialog_frame(&mut app, &ctx, screen, vec![]);
    let d = app.ui.dialog.as_ref().unwrap();
    assert!(
        !d.s("marginOutside").contains('*') && (d.m("marginOutside").unwrap() - 63.0).abs() < 1e-6,
        "worked out on leaving the field: {}",
        d.s("marginOutside")
    );
    let d = app.ui.dialog.as_mut().unwrap();
    d.fields.insert("link".into(), json!(false));
    for (k, v) in [("marginOutside", "0.4375 in*2"), ("columns", "5"), ("gutter", "0.1875 in"), ("target", "all")] {
        d.fields.insert(k.into(), json!(v));
    }
    dialog_frame(&mut app, &ctx, screen, vec![]);
    confirm(&mut app).unwrap();
    let pg = app.session.doc().unwrap().doc.spreads[1].pages[0].clone();
    assert_eq!((pg.margins.top, pg.margins.outside, pg.columns.count), (31.5, 63.0, 5));
    assert!((pg.columns.gutter - 13.5).abs() < 1e-6);
    assert_eq!(app.session.doc().unwrap().history.undo.len(), undo + 1, "one undo step");
    // Cancel after a preview puts the page back.
    open_margins_and_columns(&mut app);
    let d = app.ui.dialog.as_mut().unwrap();
    d.fields.insert("preview".into(), json!(true));
    d.fields.insert("columns".into(), json!("2"));
    dialog_frame(&mut app, &ctx, screen, vec![]);
    cancel(&mut app);
    assert_eq!(app.session.doc().unwrap().doc.spreads[1].pages[0].columns.count, 5);
}

#[test]
fn create_guides_reads_like_indesigns_and_previews() {
    let mut app = DesignApp::new(designcraft_engine::Session::new(), crate::Services::default());
    app.run("file.new", json!({"pages": 1, "facingPages": false})).unwrap();
    app.canvas_rect = Some(egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(1000.0, 800.0)));
    open_create_guides(&mut app);
    let ctx = dialog_context("en");
    let screen = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(1178.0, 814.0));
    let mut labels = vec![];
    for _ in 0..3 {
        labels = dialog_frame(&mut app, &ctx, screen, vec![]);
    }
    for expected in ["Create Guides", "Rows", "Columns", "Options", "Fit Guides to:", "Margins", "Page", "Remove Existing Ruler Guides", "Preview"] {
        visible_label(&labels, screen, expected);
    }
    // Rows and Columns side by side.
    let (r, c) = (visible_label(&labels, screen, "Rows"), visible_label(&labels, screen, "Columns"));
    assert!((r.center().y - c.center().y).abs() < 2.0 && r.center().x < c.center().x);
    // Thirds, single guides, fitted to the page: clicking the Page radio.
    click_dialog(&mut app, &ctx, screen, visible_label(&labels, screen, "Page").center());
    let d = app.ui.dialog.as_mut().unwrap();
    assert_eq!(d.s("fitTo"), "page");
    d.fields.insert("rows".into(), json!("3"));
    d.fields.insert("rowGutter".into(), json!("0"));
    d.fields.insert("preview".into(), json!(true));
    dialog_frame(&mut app, &ctx, screen, vec![]);
    let guides = |app: &DesignApp| app.session.doc().unwrap().doc.spreads[0].pages[0].guides.len();
    assert_eq!(guides(&app), 2, "previewed");
    confirm(&mut app).unwrap();
    assert_eq!(guides(&app), 2, "made once");
    app.run("edit.undo", json!({})).unwrap();
    assert_eq!(guides(&app), 0, "one undo step");
}

fn dialog_context(language: &str) -> egui::Context {
    let ctx = egui::Context::default();
    crate::theme::install_fonts(&ctx, language);
    crate::theme::apply(&ctx, &crate::theme::Tokens::for_brightness(crate::theme::Brightness::default()));
    ctx
}

fn dialog_frame(app: &mut DesignApp, ctx: &egui::Context, screen: egui::Rect, events: Vec<egui::Event>) -> Vec<(String, egui::Rect, egui::Rect)> {
    let time = ctx.input(|i| i.time) + 1.0 / 60.0;
    let mut output = ctx.run_ui(egui::RawInput { screen_rect: Some(screen), time: Some(time), events, ..Default::default() }, |ui| {
        show(app, ui.ctx());
    });
    output.textures_delta.clear();
    fn collect(shape: &egui::Shape, clip: egui::Rect, labels: &mut Vec<(String, egui::Rect, egui::Rect)>) {
        match shape {
            egui::Shape::Text(t) => labels.push((t.galley.job.text.clone(), t.visual_bounding_rect(), clip)),
            egui::Shape::Vec(shapes) => {
                for s in shapes {
                    collect(s, clip, labels);
                }
            }
            _ => {}
        }
    }
    let mut labels = Vec::new();
    for shape in output.shapes {
        collect(&shape.shape, shape.clip_rect, &mut labels);
    }
    labels
}

fn visible_label(labels: &[(String, egui::Rect, egui::Rect)], screen: egui::Rect, expected: &str) -> egui::Rect {
    let (_, rect, clip) = labels.iter().find(|(text, _, _)| text.trim() == expected).unwrap_or_else(|| panic!("{expected} is painted"));
    assert!(screen.contains_rect(*rect), "{expected} must stay on screen: {rect:?}");
    assert!(clip.contains_rect(*rect), "{expected} must not be clipped: {rect:?}, {clip:?}");
    *rect
}

fn click_dialog(app: &mut DesignApp, ctx: &egui::Context, screen: egui::Rect, pos: egui::Pos2) {
    for pressed in [true, false] {
        dialog_frame(
            app,
            ctx,
            screen,
            vec![
                egui::Event::PointerMoved(pos),
                egui::Event::PointerButton { pos, button: egui::PointerButton::Primary, pressed, modifiers: egui::Modifiers::NONE },
            ],
        );
    }
}

fn scroll_dialog(app: &mut DesignApp, ctx: &egui::Context, screen: egui::Rect, pos: egui::Pos2, delta: egui::Vec2) {
    dialog_frame(
        app,
        ctx,
        screen,
        vec![
            egui::Event::PointerMoved(pos),
            egui::Event::MouseWheel {
                unit: egui::MouseWheelUnit::Point,
                delta,
                phase: egui::TouchPhase::Move,
                modifiers: egui::Modifiers::NONE,
                source: egui::MouseWheelSource::Unknown,
            },
        ],
    );
    // Let smooth wheel scrolling and scrollbar visibility settle before clicking.
    for _ in 0..30 {
        dialog_frame(app, ctx, screen, vec![]);
    }
}

fn scope_control(app: &mut DesignApp, ctx: &egui::Context, method: &str, params: Value) -> Value {
    let (request, _reply) = crate::control::ControlRequest::new(method, params);
    let crate::control::Outcome::Done(value) = crate::control::handle(app, ctx, &request) else { panic!("ordinary control response") };
    assert_eq!(value["ok"], true, "{value}");
    value["result"].clone()
}

fn scope_click(app: &mut DesignApp, ctx: &egui::Context, screen: egui::Rect, label: &str) {
    let labels = dialog_frame(app, ctx, screen, vec![]);
    let pos = visible_label(&labels, screen, label).center();
    click_dialog(app, ctx, screen, pos);
}

fn scope_dialog() -> (DesignApp, egui::Context, egui::Rect, designcraft_engine::doc::StoryId) {
    scope_dialog_with_selection("red red | red", 7)
}

fn scope_dialog_with_selection(text: &str, end: usize) -> (DesignApp, egui::Context, egui::Rect, designcraft_engine::doc::StoryId) {
    scope_dialog_with_tool(text, end, None)
}

fn scope_dialog_with_tool(text: &str, end: usize, tool: Option<&str>) -> (DesignApp, egui::Context, egui::Rect, designcraft_engine::doc::StoryId) {
    let mut app = DesignApp::new(designcraft_engine::Session::new(), crate::Services::default());
    app.ui.language = "en".into();
    app.run("file.new", json!({})).unwrap();
    let frame = app.run("frame.create", json!({"rect": [36, 36, 100, 100], "content": "text", "text": text})).unwrap();
    let sid = designcraft_engine::doc::StoryId(frame["story"].as_u64().unwrap());
    let ctx = dialog_context("en");
    if let Some(tool) = tool {
        scope_control(&mut app, &ctx, "ui.tool.select", json!({"tool": tool}));
    }
    app.run("text.select", json!({"story": sid.0, "anchor": 0, "focus": end})).unwrap();
    app.run("app.findChange", json!({})).unwrap();
    let screen = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(1178.0, 814.0));
    for (field, value) in [("find", "red"), ("change", "unused")] {
        scope_control(&mut app, &ctx, "ui.dialog.set", json!({"field": field, "value": value}));
    }
    for _ in 0..3 {
        dialog_frame(&mut app, &ctx, screen, vec![]);
    }
    scope_click(&mut app, &ctx, screen, "document");
    scope_click(&mut app, &ctx, screen, "selection");
    assert_eq!(app.ui.dialog.as_ref().unwrap().s("scope"), "selection");
    (app, ctx, screen, sid)
}

fn scope_selected(app: &DesignApp, sid: designcraft_engine::doc::StoryId, a: usize, b: usize) {
    let t = app.session.doc().unwrap().selection.text.unwrap();
    assert_eq!((t.story, t.range(), t.cell), (sid, a..b, None));
}

fn scope_state_unchanged(app: &DesignApp, before: &designcraft_engine::DocState) {
    let after = app.session.doc().unwrap();
    assert!(std::sync::Arc::ptr_eq(&before.doc, &after.doc));
    assert_eq!(after.uid, before.uid);
    assert_eq!(after.revision, before.revision);
    assert_eq!(after.selection, before.selection);
    assert_eq!(after.history.limit, before.history.limit);
    for (actual, expected) in [(&after.history.undo, &before.history.undo), (&after.history.redo, &before.history.redo)] {
        assert_eq!(actual.len(), expected.len());
        for (a, b) in actual.iter().zip(expected) {
            assert_eq!(a.label, b.label);
            assert_eq!(a.selection, b.selection);
            assert!(std::sync::Arc::ptr_eq(&a.doc, &b.doc));
        }
    }
}

#[test]
fn find_scope_dialog_control_effective_noops_retain_bounds() {
    for (field, value) in [("objectMode", json!(false)), ("story", Value::Null)] {
        let (mut app, ctx, screen, sid) = scope_dialog();
        assert!(!app.ui.dialog.as_ref().unwrap().fields.contains_key(field));
        scope_control(&mut app, &ctx, "ui.dialog.set", json!({"field": "change", "value": "blue"}));
        scope_click(&mut app, &ctx, screen, "Count");
        assert_eq!(app.ui.dialog.as_ref().unwrap().s("status"), "2 match(es)");
        scope_click(&mut app, &ctx, screen, "Find Next");
        scope_selected(&app, sid, 0, 3);
        let before = app.session.doc().unwrap().clone();
        scope_control(&mut app, &ctx, "ui.dialog.set", json!({"field": field, "value": value}));
        assert_eq!(app.ui.dialog.as_ref().unwrap().fields[field], value);
        scope_state_unchanged(&app, &before);
        scope_click(&mut app, &ctx, screen, "Count");
        assert_eq!(app.ui.dialog.as_ref().unwrap().s("status"), "2 match(es)");
        scope_click(&mut app, &ctx, screen, "Find Next");
        scope_selected(&app, sid, 4, 7);
        scope_click(&mut app, &ctx, screen, "Change All");
        assert_eq!(app.ui.dialog.as_ref().unwrap().s("status"), "2 replacement(s) made");
        assert_eq!(app.session.doc().unwrap().doc.story(sid).unwrap().text, "blue blue | red");
        assert_eq!(app.session.doc().unwrap().history.undo.len(), before.history.undo.len() + 1);
    }
}

#[test]
fn find_scope_dialog_explicit_story_ignored_scope_preserves_grep_cursor() {
    for physical in [false, true] {
        for (text, pattern, hits, changed) in [
            ("é", "é|(?:)", vec![(0, 2)], "X"),
            ("é🙂", "é|(?:)", vec![(0, 2), (6, 6)], "X🙂X"),
            ("é🙂", "(?:)", vec![(0, 0), (2, 2), (6, 6)], "XéX🙂X"),
        ] {
            let (mut app, ctx, screen, sid) = scope_dialog_with_selection(text, 0);
            for (field, value) in [("find", json!(pattern)), ("change", json!("X")), ("story", json!(sid.0))] {
                scope_control(&mut app, &ctx, "ui.dialog.set", json!({"field": field, "value": value}));
            }
            scope_click(&mut app, &ctx, screen, "GREP");
            scope_click(&mut app, &ctx, screen, "Count");
            assert_eq!(app.ui.dialog.as_ref().unwrap().s("status"), format!("{} match(es)", hits.len()));
            for &(a, b) in &hits {
                scope_click(&mut app, &ctx, screen, "Find Next");
                assert_eq!(app.ui.dialog.as_ref().unwrap().s("status"), "Found");
                scope_selected(&app, sid, a, b);
                let before = app.session.doc().unwrap().clone();
                let cur = app.ui.dialog.as_ref().unwrap().s("scope");
                let next = if cur == "selection" { "document" } else { "selection" };
                if physical {
                    scope_click(&mut app, &ctx, screen, &cur);
                    scope_click(&mut app, &ctx, screen, next);
                } else {
                    scope_control(&mut app, &ctx, "ui.dialog.set", json!({"field": "scope", "value": next}));
                }
                assert_eq!(app.ui.dialog.as_ref().unwrap().s("scope"), next);
                assert_eq!(app.ui.dialog.as_ref().unwrap().fields["story"], json!(sid.0));
                scope_state_unchanged(&app, &before);
                scope_click(&mut app, &ctx, screen, "Count");
                assert_eq!(app.ui.dialog.as_ref().unwrap().s("status"), format!("{} match(es)", hits.len()));
            }
            scope_click(&mut app, &ctx, screen, "Find Next");
            assert_eq!(app.ui.dialog.as_ref().unwrap().s("status"), "No matches");
            // An ignored scope change must not revive an exhausted endpoint either.
            let before = app.session.doc().unwrap().clone();
            let cur = app.ui.dialog.as_ref().unwrap().s("scope");
            let next = if cur == "selection" { "document" } else { "selection" };
            if physical {
                scope_click(&mut app, &ctx, screen, &cur);
                scope_click(&mut app, &ctx, screen, next);
            } else {
                scope_control(&mut app, &ctx, "ui.dialog.set", json!({"field": "scope", "value": next}));
            }
            scope_click(&mut app, &ctx, screen, "Find Next");
            assert_eq!(app.ui.dialog.as_ref().unwrap().s("status"), "No matches");
            scope_state_unchanged(&app, &before);
            scope_click(&mut app, &ctx, screen, "Change All");
            assert_eq!(app.ui.dialog.as_ref().unwrap().s("status"), format!("{} replacement(s) made", hits.len()));
            assert_eq!(app.session.doc().unwrap().doc.story(sid).unwrap().text, changed);
        }
    }
}

#[test]
fn find_scope_dialog_explicit_story_transitions_reset_cursor() {
    for remove_override in [false, true] {
        let (mut app, ctx, screen, sid) = scope_dialog_with_selection("é🙂", 0);
        let other = app.run("frame.create", json!({"rect": [180, 36, 100, 100], "content": "text", "text": "other"})).unwrap();
        app.run("text.select", json!({"story": sid.0, "anchor": 0, "focus": 0})).unwrap();
        for (field, value) in [("find", json!("(?:)")), ("story", json!(sid.0))] {
            scope_control(&mut app, &ctx, "ui.dialog.set", json!({"field": field, "value": value}));
        }
        scope_click(&mut app, &ctx, screen, "GREP");
        scope_click(&mut app, &ctx, screen, "Find Next");
        scope_selected(&app, sid, 0, 0);
        let before = app.session.doc().unwrap().clone();
        let value = if remove_override { Value::Null } else { other["story"].clone() };
        scope_control(&mut app, &ctx, "ui.dialog.set", json!({"field": "story", "value": value}));
        scope_control(&mut app, &ctx, "ui.dialog.set", json!({"field": "story", "value": sid.0}));
        scope_state_unchanged(&app, &before);
        // Away and back still releases continuation even if no Find command intervenes.
        scope_click(&mut app, &ctx, screen, "Find Next");
        scope_selected(&app, sid, 0, 0);
        scope_click(&mut app, &ctx, screen, "Find Next");
        scope_selected(&app, sid, 2, 2);
        assert_eq!(app.session.doc().unwrap().doc.story(sid).unwrap().text, "é🙂");
        let other_sid = designcraft_engine::doc::StoryId(other["story"].as_u64().unwrap());
        assert_eq!(app.session.doc().unwrap().doc.story(other_sid).unwrap().text, "other");
    }
}

#[test]
fn find_scope_dialog_control_mode_changes_release_bounds() {
    for object_mode in [false, true] {
        let (mut app, ctx, _, sid) = scope_dialog();
        if object_mode {
            scope_control(&mut app, &ctx, "ui.dialog.set", json!({"field": "objectMode", "value": true}));
        }
        // Direct Session consumers share the retained domain even while Object is shown.
        let p = json!({"find": "red", "change": "blue", "scope": "selection"});
        let count = scope_control(&mut app, &ctx, "engine.execute", json!({"command": "find.find", "params": p}));
        assert_eq!(count.as_array().unwrap().len(), 2);
        scope_control(&mut app, &ctx, "engine.execute", json!({"command": "find.next", "params": p}));
        scope_selected(&app, sid, 0, 3);
        let before = app.session.doc().unwrap().clone();
        scope_control(&mut app, &ctx, "ui.dialog.set", json!({"field": "objectMode", "value": !object_mode}));
        assert_eq!(app.ui.dialog.as_ref().unwrap().b("objectMode"), !object_mode);
        scope_state_unchanged(&app, &before);
        let count = scope_control(&mut app, &ctx, "engine.execute", json!({"command": "find.find", "params": p}));
        assert_eq!(count, json!([{"story": sid.0, "start": 0, "end": 3, "text": "red"}]));
        let changed = scope_control(&mut app, &ctx, "engine.execute", json!({"command": "find.change", "params": p}));
        assert_eq!(changed, json!({"count": 1}));
        assert_eq!(app.session.doc().unwrap().doc.story(sid).unwrap().text, "blue red | red");
    }
}

#[test]
fn find_scope_dialog_tool_replacement_releases_bounds() {
    for (tool, replacement) in [("page", "cmd:layout.pageSize"), ("rectangleFrame", "frameSize")] {
        let (mut app, ctx, screen, sid) = scope_dialog_with_tool("red red | red", 7, Some(tool));
        assert_eq!(app.session.tool_id(), tool);
        scope_click(&mut app, &ctx, screen, "Count");
        assert_eq!(app.ui.dialog.as_ref().unwrap().s("status"), "2 match(es)");
        scope_click(&mut app, &ctx, screen, "Find Next");
        scope_selected(&app, sid, 0, 3);
        let before = app.session.doc().unwrap().clone();
        let active = (app.session.active_index(), app.session.documents().len());
        let off = app.session.layout().xf(designcraft_engine::doc::SpreadRef::Doc(0)).translation();
        let x = off.x + before.doc.spreads[0].pages[0].x + 200.0;
        let y = off.y + 200.0;
        // The Page tool opens Page Size on a double-click (a click selects the page).
        let events = if tool == "page" {
            json!([{"kind": "doubleclick", "x": x, "y": y}])
        } else {
            json!([{"kind": "down", "x": x, "y": y}, {"kind": "up", "x": x, "y": y}])
        };
        // Real tool -> Action::Dialog -> after_engine; no injected UI request.
        scope_control(&mut app, &ctx, "ui.pointer", json!({"events": events}));
        assert_eq!(app.ui.dialog.as_ref().unwrap().id, replacement);
        assert!(app.session.ui_requests.is_empty());
        assert_eq!((app.session.active_index(), app.session.documents().len()), active);
        scope_state_unchanged(&app, &before);
        scope_click(&mut app, &ctx, screen, "Cancel");
        assert!(app.ui.dialog.is_none());
        assert_eq!((app.session.active_index(), app.session.documents().len()), active);
        scope_state_unchanged(&app, &before);
        let p = json!({"find": "red", "change": "blue", "scope": "selection"});
        let count = scope_control(&mut app, &ctx, "engine.execute", json!({"command": "find.find", "params": p}));
        assert_eq!(count, json!([{"story": sid.0, "start": 0, "end": 3, "text": "red"}]));
        scope_state_unchanged(&app, &before);
        let changed = scope_control(&mut app, &ctx, "engine.execute", json!({"command": "find.change", "params": p}));
        assert_eq!(changed, json!({"count": 1}));
        assert_eq!(app.session.doc().unwrap().doc.story(sid).unwrap().text, "blue red | red");
        assert_eq!(app.session.doc().unwrap().history.undo.len(), before.history.undo.len() + 1);
    }
}

#[test]
fn find_scope_dialog_tool_without_replacement_retains_bounds() {
    for tool in ["page", "rectangleFrame"] {
        let (mut app, ctx, screen, sid) = scope_dialog_with_tool("red red | red", 7, Some(tool));
        scope_control(&mut app, &ctx, "ui.dialog.set", json!({"field": "change", "value": "blue"}));
        scope_click(&mut app, &ctx, screen, "Count");
        assert_eq!(app.ui.dialog.as_ref().unwrap().s("status"), "2 match(es)");
        scope_click(&mut app, &ctx, screen, "Find Next");
        scope_selected(&app, sid, 0, 3);
        let before = app.session.doc().unwrap().clone();
        let fields = app.ui.dialog.as_ref().unwrap().fields.clone();
        scope_control(&mut app, &ctx, "ui.pointer", json!({"events": [{"kind": "move", "x": 200, "y": 200}]}));
        assert_eq!(app.ui.dialog.as_ref().unwrap().id, "findChange");
        assert_eq!(app.ui.dialog.as_ref().unwrap().fields, fields);
        scope_state_unchanged(&app, &before);
        scope_click(&mut app, &ctx, screen, "Count");
        assert_eq!(app.ui.dialog.as_ref().unwrap().s("status"), "2 match(es)");
        scope_click(&mut app, &ctx, screen, "Find Next");
        scope_selected(&app, sid, 4, 7);
        scope_click(&mut app, &ctx, screen, "Change All");
        assert_eq!(app.ui.dialog.as_ref().unwrap().s("status"), "2 replacement(s) made");
        assert_eq!(app.session.doc().unwrap().doc.story(sid).unwrap().text, "blue blue | red");
    }
}

#[cfg(not(target_arch = "wasm32"))]
#[test]
fn find_scope_dialog_native_pdf_replacement_releases_bounds() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("three.pdf");
    std::fs::write(&path, crate::browser_import_tests::pdf()).unwrap();
    for dismissal in ["Cancel", "Escape", "control"] {
        let (mut app, ctx, screen, sid) = scope_dialog();
        let picked = std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let read = std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let seen_pick = picked.clone();
        let seen_read = read.clone();
        let pick_path = path.to_string_lossy().to_string();
        let read_path = pick_path.clone();
        app.services.pick_open = Some(Box::new(move |purpose| {
            assert_eq!(purpose, "place");
            seen_pick.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            Some(pick_path.clone())
        }));
        app.services.read = Some(Box::new(move |path| {
            assert_eq!(path, read_path);
            seen_read.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            std::fs::read(path).map_err(|e| e.to_string())
        }));
        scope_click(&mut app, &ctx, screen, "Count");
        assert_eq!(app.ui.dialog.as_ref().unwrap().s("status"), "2 match(es)");
        scope_click(&mut app, &ctx, screen, "Find Next");
        scope_selected(&app, sid, 0, 3);
        let before = app.session.doc().unwrap().clone();
        let active = (app.session.active_index(), app.session.documents().len());
        // Exercise engine.execute -> UiRequest::Pick -> native host PDF replacement.
        scope_control(&mut app, &ctx, "engine.execute", json!({"command": "file.place", "params": {}}));
        assert_eq!(picked.load(std::sync::atomic::Ordering::SeqCst), 1);
        assert_eq!(read.load(std::sync::atomic::Ordering::SeqCst), 1);
        let dialog = app.ui.dialog.as_ref().unwrap();
        assert_eq!(dialog.id, "pdfImport");
        assert_eq!(dialog.s("path"), path.to_string_lossy());
        assert_eq!(dialog.fields["pages"], 3);
        assert_eq!(dialog.s("page"), "1");
        assert!(!dialog.b("async"));
        assert_eq!(dialog.fields["target"], json!(before.uid));
        assert!(app.session.ui_requests.is_empty());
        assert!(app.pending_pdf.is_none());
        assert_eq!((app.session.active_index(), app.session.documents().len()), active);
        scope_state_unchanged(&app, &before);
        match dismissal {
            "Cancel" => scope_click(&mut app, &ctx, screen, "Cancel"),
            "Escape" => {
                dialog_frame(
                    &mut app,
                    &ctx,
                    screen,
                    vec![egui::Event::Key {
                        key: egui::Key::Escape,
                        physical_key: None,
                        pressed: true,
                        repeat: false,
                        modifiers: egui::Modifiers::NONE,
                    }],
                );
            }
            "control" => {
                scope_control(&mut app, &ctx, "ui.dialog.cancel", json!({}));
            }
            _ => panic!("known dismissal"),
        }
        assert!(app.ui.dialog.is_none());
        assert!(app.pending_pdf.is_none());
        assert_eq!(picked.load(std::sync::atomic::Ordering::SeqCst), 1);
        assert_eq!(read.load(std::sync::atomic::Ordering::SeqCst), 1);
        assert_eq!((app.session.active_index(), app.session.documents().len()), active);
        scope_state_unchanged(&app, &before);
        let p = json!({"find": "red", "change": "blue", "scope": "selection"});
        let count = scope_control(&mut app, &ctx, "engine.execute", json!({"command": "find.find", "params": p}));
        assert_eq!(count, json!([{"story": sid.0, "start": 0, "end": 3, "text": "red"}]));
        scope_state_unchanged(&app, &before);
        let changed = scope_control(&mut app, &ctx, "engine.execute", json!({"command": "find.change", "params": p}));
        assert_eq!(changed, json!({"count": 1}));
        assert_eq!(app.session.doc().unwrap().doc.story(sid).unwrap().text, "blue red | red");
        assert_eq!(app.session.doc().unwrap().history.undo.len(), before.history.undo.len() + 1);
    }
}

#[cfg(not(target_arch = "wasm32"))]
#[test]
fn find_scope_dialog_native_picker_cancel_retains_bounds() {
    let (mut app, ctx, screen, sid) = scope_dialog();
    let picked = std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let seen = picked.clone();
    app.services.pick_open = Some(Box::new(move |purpose| {
        assert_eq!(purpose, "place");
        seen.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        None
    }));
    let read = std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let seen_read = read.clone();
    app.services.read = Some(Box::new(move |_| {
        seen_read.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        Err("canceled picker must not read".into())
    }));
    scope_control(&mut app, &ctx, "ui.dialog.set", json!({"field": "change", "value": "blue"}));
    scope_click(&mut app, &ctx, screen, "Count");
    assert_eq!(app.ui.dialog.as_ref().unwrap().s("status"), "2 match(es)");
    scope_click(&mut app, &ctx, screen, "Find Next");
    scope_selected(&app, sid, 0, 3);
    let before = app.session.doc().unwrap().clone();
    let active = (app.session.active_index(), app.session.documents().len());
    let fields = app.ui.dialog.as_ref().unwrap().fields.clone();
    scope_control(&mut app, &ctx, "engine.execute", json!({"command": "file.place", "params": {}}));
    assert_eq!(picked.load(std::sync::atomic::Ordering::SeqCst), 1);
    assert_eq!(read.load(std::sync::atomic::Ordering::SeqCst), 0);
    assert_eq!(app.ui.dialog.as_ref().unwrap().id, "findChange");
    assert_eq!(app.ui.dialog.as_ref().unwrap().fields, fields);
    assert_eq!((app.session.active_index(), app.session.documents().len()), active);
    scope_state_unchanged(&app, &before);
    scope_click(&mut app, &ctx, screen, "Count");
    assert_eq!(app.ui.dialog.as_ref().unwrap().s("status"), "2 match(es)");
    scope_click(&mut app, &ctx, screen, "Find Next");
    scope_selected(&app, sid, 4, 7);
    scope_click(&mut app, &ctx, screen, "Change All");
    assert_eq!(app.ui.dialog.as_ref().unwrap().s("status"), "2 replacement(s) made");
    assert_eq!(app.session.doc().unwrap().doc.story(sid).unwrap().text, "blue blue | red");
    assert_eq!(app.session.doc().unwrap().history.undo.len(), before.history.undo.len() + 1);
}

#[test]
fn find_scope_dialog_exact_baseline_buttons() {
    let (mut app, ctx, screen, sid) = scope_dialog_with_selection("red red", 3);
    scope_control(&mut app, &ctx, "ui.dialog.set", json!({"field": "change", "value": "blue"}));
    scope_click(&mut app, &ctx, screen, "Count");
    assert_eq!(app.ui.dialog.as_ref().unwrap().s("status"), "1 match(es)");
    scope_click(&mut app, &ctx, screen, "Find Next");
    scope_selected(&app, sid, 0, 3);
    scope_click(&mut app, &ctx, screen, "Change All");
    assert_eq!(app.ui.dialog.as_ref().unwrap().s("status"), "1 replacement(s) made");
    assert_eq!(app.session.doc().unwrap().doc.story(sid).unwrap().text, "blue red");
}

#[test]
fn find_scope_dialog_grep_tab_alone_restarts_original_selection() {
    let (mut app, ctx, screen, sid) = scope_dialog();
    scope_click(&mut app, &ctx, screen, "Count");
    assert_eq!(app.ui.dialog.as_ref().unwrap().s("status"), "2 match(es)");
    scope_click(&mut app, &ctx, screen, "Find Next");
    scope_selected(&app, sid, 0, 3);
    scope_click(&mut app, &ctx, screen, "GREP");
    assert!(app.ui.dialog.as_ref().unwrap().b("grep"));
    assert_eq!(app.ui.dialog.as_ref().unwrap().s("find"), "red");
    scope_click(&mut app, &ctx, screen, "Find Next");
    scope_selected(&app, sid, 0, 3);
    scope_click(&mut app, &ctx, screen, "Count");
    assert_eq!(app.ui.dialog.as_ref().unwrap().s("status"), "2 match(es)");
    scope_click(&mut app, &ctx, screen, "Find Next");
    scope_selected(&app, sid, 4, 7);
    scope_click(&mut app, &ctx, screen, "Text");
    assert!(!app.ui.dialog.as_ref().unwrap().b("grep"));
    scope_click(&mut app, &ctx, screen, "Find Next");
    scope_selected(&app, sid, 0, 3);
    scope_click(&mut app, &ctx, screen, "Count");
    assert_eq!(app.ui.dialog.as_ref().unwrap().s("status"), "2 match(es)");
}

#[test]
fn find_scope_dialog_buttons_retain_nonempty_hits_and_change_all() {
    let (mut app, ctx, screen, sid) = scope_dialog();
    let history = app.session.doc().unwrap().history.undo.len();
    scope_click(&mut app, &ctx, screen, "Count");
    assert_eq!(app.ui.dialog.as_ref().unwrap().s("status"), "2 match(es)");
    scope_click(&mut app, &ctx, screen, "Find Next");
    scope_selected(&app, sid, 0, 3);
    scope_control(&mut app, &ctx, "ui.dialog.set", json!({"field": "change", "value": "blue"}));
    scope_click(&mut app, &ctx, screen, "Count");
    assert_eq!(app.ui.dialog.as_ref().unwrap().s("status"), "2 match(es)");
    scope_click(&mut app, &ctx, screen, "Find Next");
    scope_selected(&app, sid, 4, 7);
    scope_click(&mut app, &ctx, screen, "Find Next");
    assert_eq!(app.ui.dialog.as_ref().unwrap().s("status"), "No matches");
    scope_click(&mut app, &ctx, screen, "Count");
    assert_eq!(app.ui.dialog.as_ref().unwrap().s("status"), "2 match(es)");
    assert_eq!(app.session.doc().unwrap().history.undo.len(), history);
    scope_click(&mut app, &ctx, screen, "Change All");
    assert_eq!(app.ui.dialog.as_ref().unwrap().s("status"), "2 replacement(s) made");
    assert_eq!(app.session.doc().unwrap().doc.story(sid).unwrap().text, "blue blue | red");
    assert_eq!(app.session.doc().unwrap().history.undo.len(), history + 1);
    scope_click(&mut app, &ctx, screen, "Cancel");
    assert!(app.ui.dialog.is_none());
    app.run("edit.undo", json!({})).unwrap();
    assert_eq!(app.session.doc().unwrap().doc.story(sid).unwrap().text, "red red | red");
    app.run("edit.redo", json!({})).unwrap();
    let native = app.run("file.serialize", json!({})).unwrap();
    app.run("file.openBytes", json!({"name": "scope.designcraft", "base64": native["base64"]})).unwrap();
    assert_eq!(app.session.doc().unwrap().doc.story(sid).unwrap().text, "blue blue | red");
}

fn scope_confirm_case(action: &str) {
    let (mut app, ctx, screen, sid) = scope_dialog();
    scope_click(&mut app, &ctx, screen, "Count");
    scope_click(&mut app, &ctx, screen, "Find Next");
    scope_selected(&app, sid, 0, 3);
    scope_control(&mut app, &ctx, "ui.dialog.set", json!({"field": "change", "value": "blue"}));
    match action {
        "OK" => scope_click(&mut app, &ctx, screen, "OK"),
        "Enter" => {
            dialog_frame(
                &mut app,
                &ctx,
                screen,
                vec![egui::Event::Key { key: egui::Key::Enter, physical_key: None, pressed: true, repeat: false, modifiers: egui::Modifiers::NONE }],
            );
        }
        "control" => {
            assert_eq!(scope_control(&mut app, &ctx, "ui.dialog.confirm", json!({}))["count"], 2);
        }
        _ => panic!("known confirmation route"),
    }
    assert!(app.ui.dialog.is_none());
    assert_eq!(app.session.doc().unwrap().doc.story(sid).unwrap().text, "blue blue | red");
    // The closed dialog cannot leave its old seven-byte range for a later direct query.
    let matches = app.run("find.find", json!({"find": "blue", "scope": "selection"})).unwrap();
    assert_eq!(matches.as_array().unwrap().len(), 0);
}

#[test]
fn find_scope_dialog_ok_consumes_retained_bounds() {
    scope_confirm_case("OK");
}

#[test]
fn find_scope_dialog_enter_consumes_retained_bounds() {
    scope_confirm_case("Enter");
}

#[test]
fn find_scope_dialog_control_confirm_consumes_retained_bounds() {
    scope_confirm_case("control");
}

#[test]
fn find_scope_dialog_query_reselection_cancel_and_reopen() {
    let (mut app, ctx, screen, sid) = scope_dialog();
    scope_click(&mut app, &ctx, screen, "Find Next");
    scope_selected(&app, sid, 0, 3);
    scope_control(&mut app, &ctx, "ui.dialog.set", json!({"field": "grep", "value": true}));
    scope_control(&mut app, &ctx, "ui.dialog.set", json!({"field": "find", "value": "r.d"}));
    scope_click(&mut app, &ctx, screen, "Find Next");
    scope_selected(&app, sid, 0, 3);
    scope_click(&mut app, &ctx, screen, "Count");
    assert_eq!(app.ui.dialog.as_ref().unwrap().s("status"), "2 match(es)");
    scope_control(&mut app, &ctx, "ui.dialog.cancel", json!({}));
    assert_eq!(app.run("find.find", json!({"find": "red", "scope": "selection"})).unwrap().as_array().unwrap().len(), 1);
    app.run("text.select", json!({"story": sid.0, "anchor": 0, "focus": 7})).unwrap();
    app.run("find.next", json!({"find": "red", "scope": "selection"})).unwrap();
    scope_control(&mut app, &ctx, "ui.dialog.open", json!({"id": "findChange", "fields": {"find": "red", "scope": "selection"}}));
    scope_click(&mut app, &ctx, screen, "Count");
    assert_eq!(app.ui.dialog.as_ref().unwrap().s("status"), "1 match(es)");
    app.run("text.select", json!({"story": sid.0, "anchor": 4, "focus": 7})).unwrap();
    scope_click(&mut app, &ctx, screen, "Find Next");
    scope_selected(&app, sid, 4, 7);
    app.run("app.findChange", json!({})).unwrap();
    for (field, value) in [("find", "red"), ("scope", "selection")] {
        scope_control(&mut app, &ctx, "ui.dialog.set", json!({"field": field, "value": value}));
    }
    scope_click(&mut app, &ctx, screen, "Count");
    assert_eq!(app.ui.dialog.as_ref().unwrap().s("status"), "1 match(es)");
}

#[test]
fn find_scope_dialog_word_reselection_limits_count_and_change_buttons() {
    let (mut app, ctx, screen, sid) = scope_dialog();
    scope_click(&mut app, &ctx, screen, "Count");
    assert_eq!(app.ui.dialog.as_ref().unwrap().s("status"), "2 match(es)");
    scope_click(&mut app, &ctx, screen, "Find Next");
    scope_selected(&app, sid, 0, 3);
    let selected = app.session.doc().unwrap().selection.clone();
    let frame = selected.text.unwrap().frame.unwrap();
    let point = {
        let st = app.session.doc().unwrap();
        let cs = app.session.cache.get(&st.doc, sid, None);
        let (fi, x, baseline, ascent, _) = designcraft_compose::caret(&cs, 1).unwrap();
        let inner = designcraft_geom::Point::new(x, baseline - ascent * 0.5);
        assert_eq!(cs.frames[fi].frame, frame);
        assert_eq!(designcraft_compose::hit(&cs, fi, inner), Some(1));
        let loc = st.doc.find(frame).unwrap();
        st.doc.parent_xf(&loc) * st.doc.text_xf(st.doc.item(frame).unwrap()) * inner
    };
    let before = app.session.doc().unwrap().doc.clone();
    let revision = app.session.doc().unwrap().revision;
    app.run("text.selectWord", json!({"frame": frame.0, "point": [point.x, point.y]})).unwrap();
    assert_eq!(app.session.doc().unwrap().selection, selected);
    assert!(app.session.doc().unwrap().revision > revision, "a new revision");
    assert!(std::sync::Arc::ptr_eq(&before, &app.session.doc().unwrap().doc));
    scope_click(&mut app, &ctx, screen, "Count");
    assert_eq!(app.ui.dialog.as_ref().unwrap().s("status"), "1 match(es)");
    scope_control(&mut app, &ctx, "ui.dialog.set", json!({"field": "change", "value": "blue"}));
    scope_click(&mut app, &ctx, screen, "Change All");
    assert_eq!(app.ui.dialog.as_ref().unwrap().s("status"), "1 replacement(s) made");
    assert_eq!(app.session.doc().unwrap().doc.story(sid).unwrap().text, "blue red | red");
}

#[cfg(not(target_arch = "wasm32"))]
#[test]
fn find_scope_dialog_failed_native_drop_alert_releases_bounds() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("broken.designcraft");
    std::fs::write(&path, b"not a native document").unwrap();
    for dismissal in ["OK", "Escape", "control"] {
        let (mut app, ctx, screen, sid) = scope_dialog();
        scope_click(&mut app, &ctx, screen, "Count");
        assert_eq!(app.ui.dialog.as_ref().unwrap().s("status"), "2 match(es)");
        scope_click(&mut app, &ctx, screen, "Find Next");
        scope_selected(&app, sid, 0, 3);
        let before = app.session.doc().unwrap().doc.clone();
        let selected = app.session.doc().unwrap().selection.clone();
        let revision = app.session.doc().unwrap().revision;
        let history = app.session.doc().unwrap().history.undo.len();
        let uid = app.session.doc().unwrap().uid;
        let error = app.open_dropped(&path.to_string_lossy()).unwrap_err();
        let alert = app.ui.dialog.as_ref().unwrap();
        assert_eq!(alert.id, "alert");
        assert_eq!(alert.s("title"), "Can't Open the File");
        assert_eq!(alert.s("message"), error);
        assert!(std::sync::Arc::ptr_eq(&before, &app.session.doc().unwrap().doc));
        assert_eq!(app.session.doc().unwrap().selection, selected);
        assert_eq!(app.session.doc().unwrap().revision, revision);
        assert_eq!(app.session.doc().unwrap().uid, uid);
        assert_eq!(app.session.doc().unwrap().history.undo.len(), history);
        match dismissal {
            "OK" => scope_click(&mut app, &ctx, screen, "OK"),
            "Escape" => {
                dialog_frame(
                    &mut app,
                    &ctx,
                    screen,
                    vec![egui::Event::Key {
                        key: egui::Key::Escape,
                        physical_key: None,
                        pressed: true,
                        repeat: false,
                        modifiers: egui::Modifiers::NONE,
                    }],
                );
            }
            "control" => {
                scope_control(&mut app, &ctx, "ui.dialog.cancel", json!({}));
            }
            _ => panic!("known dismissal"),
        }
        assert!(app.ui.dialog.is_none());
        let p = json!({"find": "red", "change": "blue", "scope": "selection"});
        assert_eq!(app.run("find.find", p.clone()).unwrap(), json!([{"story": sid.0, "start": 0, "end": 3, "text": "red"}]));
        assert_eq!(app.run("find.change", p).unwrap(), json!({"count": 1}));
        assert_eq!(app.session.doc().unwrap().doc.story(sid).unwrap().text, "blue red | red");
        assert_eq!(app.session.doc().unwrap().history.undo.len(), history + 1);
    }
}

#[test]
fn find_scope_dialog_child_errors_and_explicit_parent() {
    let (mut app, ctx, screen, sid) = scope_dialog();
    app.run("text.select", json!({"story": sid.0, "anchor": 13, "focus": 13})).unwrap();
    app.run("table.insert", json!({"rows": 1, "cols": 1})).unwrap();
    app.run("text.insert", json!({"text": "red child", "raw": true})).unwrap();
    let child = app.session.doc().unwrap().selection.text.unwrap();
    let before = app.session.doc().unwrap().doc.clone();
    let selection = app.session.doc().unwrap().selection.clone();
    let history = app.session.doc().unwrap().history.undo.len();
    for label in ["Count", "Find Next", "Change All"] {
        scope_click(&mut app, &ctx, screen, label);
        assert!(app.ui.dialog.as_ref().unwrap().s("status").contains("not supported"));
        assert!(std::sync::Arc::ptr_eq(&before, &app.session.doc().unwrap().doc));
        assert_eq!(app.session.doc().unwrap().selection, selection);
        assert_eq!(app.session.doc().unwrap().history.undo.len(), history);
    }
    scope_control(&mut app, &ctx, "ui.dialog.set", json!({"field": "change", "value": "blue"}));
    scope_control(&mut app, &ctx, "ui.dialog.set", json!({"field": "story", "value": sid.0}));
    scope_click(&mut app, &ctx, screen, "Count");
    assert_eq!(app.ui.dialog.as_ref().unwrap().s("status"), "3 match(es)");
    scope_click(&mut app, &ctx, screen, "Change All");
    assert_eq!(app.ui.dialog.as_ref().unwrap().s("status"), "3 replacement(s) made");
    assert_eq!(app.session.doc().unwrap().doc.text_story(sid, child.cell).unwrap().text, "red child");
    assert!(app.session.doc().unwrap().doc.story(sid).unwrap().text.starts_with("blue blue | blue"));
}

#[test]
fn find_scope_dialog_error_confirmation_and_escape_release_context() {
    for escape in [false, true] {
        let (mut app, ctx, screen, sid) = scope_dialog();
        scope_click(&mut app, &ctx, screen, "Find Next");
        scope_selected(&app, sid, 0, 3);
        if escape {
            dialog_frame(
                &mut app,
                &ctx,
                screen,
                vec![egui::Event::Key { key: egui::Key::Escape, physical_key: None, pressed: true, repeat: false, modifiers: egui::Modifiers::NONE }],
            );
        } else {
            scope_control(&mut app, &ctx, "ui.dialog.set", json!({"field": "find", "value": "["}));
            scope_control(&mut app, &ctx, "ui.dialog.set", json!({"field": "grep", "value": true}));
            scope_click(&mut app, &ctx, screen, "OK");
        }
        assert!(app.ui.dialog.is_none());
        assert_eq!(app.session.doc().unwrap().doc.story(sid).unwrap().text, "red red | red");
        assert_eq!(app.run("find.find", json!({"find": "red", "scope": "selection"})).unwrap().as_array().unwrap().len(), 1);
    }
}

#[test]
fn find_scope_dialog_domain_and_object_changes_release_original_range() {
    for route in ["scope", "object", "menu", "command"] {
        let (mut app, ctx, screen, _) = scope_dialog();
        scope_click(&mut app, &ctx, screen, "Find Next");
        match route {
            "scope" => {
                for scope in ["story", "selection"] {
                    scope_control(&mut app, &ctx, "ui.dialog.set", json!({"field": "scope", "value": scope}));
                }
            }
            "object" => {
                scope_click(&mut app, &ctx, screen, "Object");
                scope_click(&mut app, &ctx, screen, "Text");
            }
            "menu" => crate::menus::activate(&mut app, "app.newDocumentDialog", &Value::Null),
            "command" => {
                app.run("app.newDocumentDialog", json!({})).unwrap();
            }
            _ => panic!("known reset route"),
        }
        assert_eq!(app.run("find.find", json!({"find": "red", "scope": "selection"})).unwrap().as_array().unwrap().len(), 1);
    }
}

#[test]
fn dialog_title_and_actions_fit_scaled_viewports() {
    for language in ["en", "ar"] {
        for size in [egui::vec2(1178.0, 814.0), egui::vec2(589.0, 407.0), egui::vec2(392.0, 271.0), egui::vec2(450.0, 280.0)] {
            for (command, title, ok, cancel) in
                [("app.preferences", "Preferences", "OK", "Cancel"), ("app.newDocumentDialog", "New Document", "Create", "Close")]
            {
                let mut app = DesignApp::new(designcraft_engine::Session::new(), crate::Services::default());
                app.ui.language = language.into();
                app.run("file.new", json!({})).unwrap();
                app.run(command, json!({})).unwrap();
                let ctx = dialog_context(language);
                let screen = egui::Rect::from_min_size(egui::Pos2::ZERO, size);
                for _ in 0..3 {
                    dialog_frame(&mut app, &ctx, screen, vec![]);
                }
                let labels = dialog_frame(&mut app, &ctx, screen, vec![]);
                for expected in [title, ok, cancel] {
                    visible_label(&labels, screen, crate::i18n::tr(language, expected));
                }
            }
        }
    }
}

#[test]
fn preferences_first_setting_switches_interface_language() {
    let mut app = DesignApp::new(designcraft_engine::Session::new(), crate::Services::default());
    app.run("app.preferences", json!({})).unwrap();
    let ctx = dialog_context("");
    let screen = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(1178.0, 814.0));
    for _ in 0..3 {
        dialog_frame(&mut app, &ctx, screen, vec![]);
    }
    let labels = dialog_frame(&mut app, &ctx, screen, vec![]);
    let heading = visible_label(&labels, screen, "Interface Language");
    let combo = visible_label(&labels, screen, "English");
    let scaling = visible_label(&labels, screen, "When Scaling");
    assert!(heading.bottom() <= combo.top() && combo.bottom() <= scaling.top(), "the language comes first: {heading:?} {combo:?} {scaling:?}");
    let pane = labels.iter().filter(|(_, rect, _)| rect.left() >= heading.left() - 1.0 && rect.top() < heading.top());
    assert!(pane.clone().all(|(text, _, _)| text == "Preferences"), "nothing above it: {:?}", pane.collect::<Vec<_>>());

    // Choosing a language applies it at once; the dialog stays open.
    click_dialog(&mut app, &ctx, screen, combo.center());
    let labels = dialog_frame(&mut app, &ctx, screen, vec![]);
    for (_, name) in crate::i18n::LANGUAGES {
        assert!(labels.iter().any(|(text, _, _)| text == name), "{name} is listed by its own name");
    }
    let deutsch = visible_label(&labels, screen, "Deutsch").center();
    click_dialog(&mut app, &ctx, screen, deutsch);
    assert_eq!(app.ui.language, "de");
    assert_eq!(app.ui.dialog.as_ref().map(|d| d.id.as_str()), Some("preferences"));
    let labels = dialog_frame(&mut app, &ctx, screen, vec![]);
    let heading = visible_label(&labels, screen, crate::i18n::tr("de", "Interface Language"));
    let combo = visible_label(&labels, screen, "Deutsch");

    // The names stay endonyms in the German interface.
    click_dialog(&mut app, &ctx, screen, combo.center());
    let labels = dialog_frame(&mut app, &ctx, screen, vec![]);
    for name in ["English", "Français", "日本語", "Українська"] {
        assert!(labels.iter().any(|(text, _, _)| text == name), "{name}");
    }
    click_dialog(&mut app, &ctx, screen, heading.center());
    let labels = dialog_frame(&mut app, &ctx, screen, vec![]);
    assert!(!labels.iter().any(|(text, _, _)| text == "Français"), "the list closes");

    // The control channel's command and the combo show the same setting.
    app.run("app.language", json!({"lang": "ja"})).unwrap();
    let labels = dialog_frame(&mut app, &ctx, screen, vec![]);
    assert!(labels.iter().any(|(text, _, _)| text == "日本語"));
    assert!(labels.iter().any(|(text, _, _)| text == crate::i18n::tr("ja", "Interface Language")));

    // Cancel puts back the language the dialog opened with; OK keeps the choice.
    let cancel_button = visible_label(&labels, screen, crate::i18n::tr("ja", "Cancel")).center();
    click_dialog(&mut app, &ctx, screen, cancel_button);
    assert!(app.ui.dialog.is_none());
    assert_eq!(app.ui.language, "");
    app.run("app.preferences", json!({})).unwrap();
    app.run("app.language", json!({"lang": "it"})).unwrap();
    confirm(&mut app).unwrap();
    assert_eq!(app.ui.language, "it");
    app.run("app.preferences", json!({})).unwrap();
    app.run("app.language", json!({"lang": "uk"})).unwrap();
    cancel(&mut app);
    assert_eq!(app.ui.language, "it");
}

/// Preferences › Interface › Panels holds Auto-Collapse Iconic Panels, the same setting as the
/// iconic column's menu.
#[test]
fn preferences_set_auto_collapse_iconic_panels() {
    let mut app = DesignApp::new(designcraft_engine::Session::new(), crate::Services::default());
    app.run("app.preferences", json!({})).unwrap();
    assert_eq!(app.ui.dialog.as_ref().unwrap().fields["autoCollapseIconicPanels"], json!(false));
    app.ui.dialog.as_mut().unwrap().fields.insert("autoCollapseIconicPanels".into(), json!(true));
    confirm(&mut app).unwrap();
    assert!(app.ui.dock_auto_collapse);
    app.run("app.preferences", json!({})).unwrap();
    assert_eq!(app.ui.dialog.as_ref().unwrap().fields["autoCollapseIconicPanels"], json!(true));
}

#[test]
fn preferences_sidebar_scroll_keeps_selected_options_visible() {
    let mut app = DesignApp::new(designcraft_engine::Session::new(), crate::Services::default());
    app.run("file.new", json!({})).unwrap();
    app.run("app.preferences", json!({})).unwrap();
    let ctx = dialog_context("en");
    let screen = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(589.0, 407.0));
    for _ in 0..3 {
        dialog_frame(&mut app, &ctx, screen, vec![]);
    }
    let labels = dialog_frame(&mut app, &ctx, screen, vec![]);
    let sidebar = visible_label(&labels, screen, "General").center();
    assert!(!labels.iter().any(|(text, rect, clip)| text == "File Handling" && clip.contains_rect(*rect)), "last section starts below the sidebar");
    scroll_dialog(&mut app, &ctx, screen, sidebar, egui::vec2(0.0, -600.0));
    let labels = dialog_frame(&mut app, &ctx, screen, vec![]);
    let files = visible_label(&labels, screen, "File Handling").center();
    click_dialog(&mut app, &ctx, screen, files);
    assert_eq!(app.ui.dialog.as_ref().unwrap().s("section"), "files");
    let labels = dialog_frame(&mut app, &ctx, screen, vec![]);
    for expected in ["Document Recovery Data", "Save recovery data every:", "OK", "Cancel"] {
        visible_label(&labels, screen, expected);
    }
}

#[test]
fn preferences_horizontal_scroll_reaches_clipped_options() {
    let mut app = DesignApp::new(designcraft_engine::Session::new(), crate::Services::default());
    app.run("app.preferences", json!({})).unwrap();
    let ctx = dialog_context("en");
    let screen = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(450.0, 280.0));
    for _ in 0..3 {
        dialog_frame(&mut app, &ctx, screen, vec![]);
    }
    let labels = dialog_frame(&mut app, &ctx, screen, vec![]);
    let body = visible_label(&labels, screen, "When Scaling").center();
    // The last General option sits below this short viewport.
    scroll_dialog(&mut app, &ctx, screen, body, egui::vec2(0.0, -600.0));
    let labels = dialog_frame(&mut app, &ctx, screen, vec![]);
    let option = "Absolute Numbering (instead of Section Numbering)";
    let (_, rect, clip) = labels.iter().find(|(text, _, _)| text == option).unwrap();
    assert!(rect.right() > clip.right(), "rightmost option starts horizontally clipped: {rect:?}, {clip:?}");
    scroll_dialog(&mut app, &ctx, screen, body, egui::vec2(-600.0, 0.0));
    let labels = dialog_frame(&mut app, &ctx, screen, vec![]);
    for expected in [option, "Preferences", "OK", "Cancel"] {
        visible_label(&labels, screen, expected);
    }
    click_dialog(&mut app, &ctx, screen, visible_label(&labels, screen, option).center());
    assert!(app.ui.dialog.as_ref().unwrap().b("absolutePageNumbers"));
}

/// Scroll the dialog body down at `at` until `text` is painted unclipped on screen.
fn scroll_to_label(app: &mut DesignApp, ctx: &egui::Context, screen: egui::Rect, at: egui::Pos2, text: &str) -> egui::Rect {
    for _ in 0..40 {
        let labels = dialog_frame(app, ctx, screen, vec![]);
        if let Some((_, rect, _)) = labels.iter().find(|(t, r, clip)| t.trim() == text && screen.contains_rect(*r) && clip.contains_rect(*r)) {
            return *rect;
        }
        scroll_dialog(app, ctx, screen, at, egui::vec2(0.0, -60.0));
    }
    panic!("{text} never scrolled into view");
}

#[test]
fn new_document_vertical_scroll_reaches_every_field_and_preset() {
    let mut app = DesignApp::new(designcraft_engine::Session::new(), crate::Services::default());
    app.run("app.newDocumentDialog", json!({})).unwrap();
    let ctx = dialog_context("en");
    let screen = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(392.0, 271.0));
    for _ in 0..3 {
        dialog_frame(&mut app, &ctx, screen, vec![]);
    }
    let labels = dialog_frame(&mut app, &ctx, screen, vec![]);
    let body = visible_label(&labels, screen, "Width").center();
    let shown = |labels: &[(String, egui::Rect, egui::Rect)]| labels.iter().any(|(t, r, clip)| t == "Primary Text Frame" && clip.contains_rect(*r));
    assert!(!shown(&labels), "the details start vertically clipped");
    let ptf = scroll_to_label(&mut app, &ctx, screen, body, "Primary Text Frame");
    let labels = dialog_frame(&mut app, &ctx, screen, vec![]);
    for expected in ["New Document", "Create", "Close"] {
        visible_label(&labels, screen, expected);
    }
    click_dialog(&mut app, &ctx, screen, ptf.center());
    assert!(app.ui.dialog.as_ref().unwrap().b("primaryTextFrame"), "the caption toggles its checkbox");
    // Below the details: the preset tabs and cards.
    scroll_to_label(&mut app, &ctx, screen, body, "[Default]");
}

#[test]
fn arabic_new_document_fields_remain_reachable_and_editable() {
    for size in [egui::vec2(392.0, 271.0), egui::vec2(450.0, 280.0), egui::vec2(1178.0, 814.0)] {
        let mut app = DesignApp::new(designcraft_engine::Session::new(), crate::Services::default());
        app.ui.language = "ar".into();
        app.run("app.newDocumentDialog", json!({})).unwrap();
        let fields = [("height", "66p1"), ("gutter", "1p2"), ("marginBottom", "3p3"), ("marginOutside", "3p4")];
        for (field, value) in fields {
            app.ui.dialog.as_mut().unwrap().fields.insert(field.into(), json!(value));
        }
        let ctx = dialog_context("ar");
        let screen = egui::Rect::from_min_size(egui::Pos2::ZERO, size);
        for _ in 0..3 {
            dialog_frame(&mut app, &ctx, screen, vec![]);
        }
        // With craft-fonts' Noto Sans Arabic the rows above are taller, so at the smallest size the
        // Height field starts partly below the body's clip: scroll to it like the other fields.
        let labels = dialog_frame(&mut app, &ctx, screen, vec![]);
        let (_, _, body) = labels.iter().find(|(text, _, _)| text.trim() == "66p1").expect("66p1 is painted");
        let height = scroll_to_label(&mut app, &ctx, screen, body.center(), "66p1");
        click_dialog(&mut app, &ctx, screen, height.center());
        dialog_frame(
            &mut app,
            &ctx,
            screen,
            vec![
                egui::Event::Key {
                    key: egui::Key::A,
                    physical_key: None,
                    pressed: true,
                    repeat: false,
                    modifiers: egui::Modifiers { ctrl: true, command: true, ..Default::default() },
                },
                egui::Event::Text("88p8".into()),
            ],
        );
        assert_eq!(app.ui.dialog.as_ref().unwrap().s("height"), "88p8", "numeric fields accept edits");
        let caption = scroll_to_label(&mut app, &ctx, screen, height.center(), crate::i18n::tr("ar", "Primary Text Frame"));
        click_dialog(&mut app, &ctx, screen, caption.center());
        assert!(app.ui.dialog.as_ref().unwrap().b("primaryTextFrame"), "checkboxes remain clickable");
        for (_, value) in &fields[1..] {
            scroll_to_label(&mut app, &ctx, screen, height.center(), value);
        }
    }
}

#[test]
fn resized_dialogs_cancel_and_reopen_without_applying_edits() {
    for (command, title, field, ok, cancel) in
        [("app.preferences", "Preferences", "recoveryMinutes", "OK", "Cancel"), ("app.newDocumentDialog", "New Document", "pages", "Create", "Close")]
    {
        let mut app = DesignApp::new(designcraft_engine::Session::new(), crate::Services::default());
        app.run(command, json!({})).unwrap();
        let original = app.ui.dialog.as_ref().unwrap().fields[field].clone();
        app.ui.dialog.as_mut().unwrap().fields.insert(field.into(), json!(42));
        let ctx = dialog_context("en");
        for size in [egui::vec2(1178.0, 814.0), egui::vec2(392.0, 271.0), egui::vec2(589.0, 407.0)] {
            let screen = egui::Rect::from_min_size(egui::Pos2::ZERO, size);
            for _ in 0..3 {
                dialog_frame(&mut app, &ctx, screen, vec![]);
            }
            let labels = dialog_frame(&mut app, &ctx, screen, vec![]);
            for expected in [title, ok, cancel] {
                visible_label(&labels, screen, expected);
            }
        }
        let screen = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(589.0, 407.0));
        let labels = dialog_frame(&mut app, &ctx, screen, vec![]);
        click_dialog(&mut app, &ctx, screen, visible_label(&labels, screen, cancel).center());
        assert!(app.ui.dialog.is_none());
        assert!(app.session.active().is_none(), "cancelling must not create a document");
        app.run(command, json!({})).unwrap();
        assert_eq!(app.ui.dialog.as_ref().unwrap().fields[field], original, "cancelled edits must not survive reopening");
        for _ in 0..3 {
            dialog_frame(&mut app, &ctx, screen, vec![]);
        }
        let labels = dialog_frame(&mut app, &ctx, screen, vec![]);
        visible_label(&labels, screen, title);
        dialog_frame(
            &mut app,
            &ctx,
            screen,
            vec![egui::Event::Key { key: egui::Key::Escape, physical_key: None, pressed: true, repeat: false, modifiers: egui::Modifiers::NONE }],
        );
        assert!(app.ui.dialog.is_none(), "Escape must also close the reopened dialog");
    }
}

#[test]
fn paragraph_style_options_tabs_section_adds_a_stop_on_its_ruler() {
    use egui_kittest::kittest::Queryable;
    let (app, _) = ruled_paragraph();
    let mut h = crate::test_window::open(app, egui::vec2(1400.0, 900.0));
    h.state_mut().app.ui.dialog = Some(Dialog::new("paragraphStyleOptions", json!({"name": "Ruled Child", "section": "tabs"})));
    h.run_steps(4);
    h.get_by_label("Clear All");
    // The ruler sits below the row of alignment buttons; its strip takes the click.
    let b = h.get_by_label("Right-aligned tab").rect();
    h.get_by_label("Right-aligned tab").click();
    h.run_steps(2);
    let left = h.get_by_label("Left-aligned tab").rect();
    let at = egui::pos2(left.min.x + crate::panels::tabs::PAD + 100.0, b.max.y + 14.0);
    crate::test_window::click_at(&mut h, at);
    let d = h.state().app.ui.dialog.clone().unwrap();
    let tabs = d.fields.get("p.tabs").cloned().unwrap_or_default();
    assert_eq!(tabs.as_array().map(Vec::len), Some(1), "{tabs}");
    assert_eq!(tabs[0]["align"], "right");
    confirm(&mut h.state_mut().app).unwrap();
    let app = &mut h.state_mut().app;
    let style = app.session.execute("type.tabs.get", &json!({"style": "Ruled Child"})).unwrap();
    assert_eq!(style["tabs"], tabs);
    assert_eq!(app.session.execute("type.tabs.get", &json!({})).unwrap()["tabs"], tabs, "the paragraph follows its style");
}

#[test]
fn parses_command_params() {
    let f = command_fields("{name, type: custom|lastPageNumber|chapterNumber, text?, rule?: {on, weight}, flag?: bool} — creates");
    let keys: Vec<&str> = f.iter().map(|f| f.key.as_str()).collect();
    assert_eq!(keys, ["name", "type", "text", "rule", "flag"]);
    assert_eq!(f[1].choices, ["custom", "lastPageNumber", "chapterNumber"]);
    assert!(!f[0].optional && f[2].optional);
    assert!(f[4].boolean);
    assert!(command_fields("{}").is_empty());
    assert!(command_fields("no params").is_empty());
    // A parenthetical after the key describes the value.
    let f = command_fields("{angle (degrees, CCW), ids?}");
    let keys: Vec<&str> = f.iter().map(|f| f.key.as_str()).collect();
    assert_eq!(keys, ["angle", "ids"]);
    assert!(!f[0].optional && f[1].optional);
    assert_eq!(f[0].hint, "degrees, CCW");
    let f = command_fields("{scaleX? (%), ref?: 0..8, ids?: parent items (default: all)}");
    let keys: Vec<&str> = f.iter().map(|f| f.key.as_str()).collect();
    assert_eq!(keys, ["scaleX", "ref", "ids"]);
    assert!(f.iter().all(|f| f.optional));
    assert_eq!(f[0].hint, "%");
    assert_eq!(f[2].hint, "parent items (default: all)");
    // Every command with a "…" label parses without panicking.
    for c in designcraft_engine::command_specs() {
        let _ = command_fields(c.params);
    }
}

fn test_app() -> crate::DesignApp {
    let mut app = crate::DesignApp::new(designcraft_engine::Session::new(), crate::Services::default());
    app.session.execute("file.new", &json!({"pages": 1})).unwrap();
    app
}

#[test]
fn toc_dialog_keeps_the_character_styles_and_sorting_of_its_entries() {
    let mut app = test_app();
    app.session.execute("style.paragraph.create", &json!({"name": "Head"})).unwrap();
    app.session.execute("style.character.create", &json!({"name": "Number"})).unwrap();
    app.session.execute("style.character.create", &json!({"name": "Dots"})).unwrap();
    let sid = app.session.execute("frame.create", &json!({"rect": [36, 36, 300, 200], "content": "text", "text": "Pears\nApples"})).unwrap()["story"]
        .clone();
    app.session.execute("text.select", &json!({"story": sid, "anchor": 0, "focus": 8})).unwrap();
    app.session.execute("style.paragraph.apply", &json!({"name": "Head"})).unwrap();
    app.session.execute("layout.pages.insert", &json!({"count": 1})).unwrap();
    let entries = json!([{"style": "Head", "pageNumberStyle": "Number", "separatorStyle": "Dots", "sort": true}]);
    app.session.execute("toc.generate", &json!({"page": 2, "entries": entries})).unwrap();
    let before = app.session.active().unwrap().doc.toc.clone().unwrap();
    // The dialog shows the options, and OK (Replace Existing) writes them back unchanged.
    open_toc(&mut app);
    let d = app.ui.dialog.as_ref().unwrap();
    let row = &toc_rows(d)[0];
    assert_eq!((row.page_number_style.as_str(), row.separator_style.as_str(), row.sort), ("Number", "Dots", true));
    let chars: Vec<String> = serde_json::from_value(d.fields["charStyles"].clone()).unwrap();
    assert_eq!(chars, ["Number", "Dots"], "[None] is the menu's first choice, not a style");
    let p = toc_params(&app, d);
    assert_eq!(
        (&p["entries"][0]["pageNumberStyle"], &p["entries"][0]["separatorStyle"], &p["entries"][0]["sort"]),
        (&json!("Number"), &json!("Dots"), &json!(true))
    );
    confirm(&mut app).expect("OK regenerates the table of contents");
    let st = app.session.active().unwrap();
    assert_eq!(st.doc.toc.as_ref().unwrap().entries, before.entries);
    // [None] and unsorted are left out of the params.
    let mut d = Dialog::new(TOC, json!({"included": [{"style": "Head", "level": 1, "separator": "^t"}]}));
    d.fields.insert("replace".into(), json!(true));
    let p = toc_params(&app, &d);
    assert!(p["entries"][0].get("pageNumberStyle").is_none() && p["entries"][0].get("sort").is_none(), "{p}");
}

#[test]
fn paragraph_style_options_dialog_saves_edits() {
    let mut app = test_app();
    let name = app.session.execute("style.paragraph.create", &json!({"name": "Body Copy"})).unwrap()["name"].as_str().unwrap().to_string();
    app.ui.dialog = Some(Dialog::new("paragraphStyleOptions", json!({"name": name, "p.align": "center"})));
    confirm(&mut app).expect("OK saves the style edit");
    let st = app.session.active().unwrap();
    assert_eq!(st.doc.styles.para(&name).unwrap().para.align, Some(designcraft_doc::Align::Center));
}

#[test]
fn paragraph_style_options_align_to_grid_shows_and_sets_the_style_grid_alignment() {
    use designcraft_doc::GridAlign;
    use egui_kittest::kittest::Queryable;
    let mut app = test_app();
    app.session.execute("style.paragraph.create", &json!({"name": "Grid Body", "para": {"gridAlign": "firstLineOnly"}})).unwrap();
    app.ui.dialog = Some(Dialog::new("paragraphStyleOptions", json!({"name": "Grid Body", "section": "indents"})));
    let mut h = crate::test_window::open(app, egui::vec2(1400.0, 900.0));
    let grid = |h: &egui_kittest::Harness<'static, crate::test_window::Window>| {
        h.state().app.session.active().unwrap().doc.styles.para("Grid Body").unwrap().para.grid_align
    };
    h.run_steps(4);
    h.get_by_value("First Line Only");
    confirm(&mut h.state_mut().app).unwrap();
    assert_eq!(grid(&h), Some(GridAlign::FirstLineOnly), "OK without edits keeps the style's alignment");
    h.state_mut().app.ui.dialog = Some(Dialog::new("paragraphStyleOptions", json!({"name": "Grid Body", "section": "indents"})));
    h.run_steps(4);
    h.get_by_value("First Line Only").click();
    h.run_steps(3);
    h.get_by_label("All Lines").click();
    h.run_steps(3);
    confirm(&mut h.state_mut().app).unwrap();
    assert_eq!(grid(&h), Some(GridAlign::AllLines));
}

#[test]
fn delete_paragraph_style_dialog_removes_the_style_and_reassigns_text() {
    let mut app = test_app();
    let name = app.session.execute("style.paragraph.create", &json!({"name": "Caption"})).unwrap()["name"].as_str().unwrap().to_string();
    let sid = app.session.execute("frame.create", &json!({"rect": [36, 36, 300, 200], "content": "text"})).unwrap()["story"].as_u64().unwrap();
    app.session.execute("text.insert", &json!({"text": "Hello"})).unwrap();
    app.session.execute("text.select", &json!({"story": sid, "anchor": 0, "focus": 0})).unwrap();
    app.session.execute("style.paragraph.apply", &json!({"name": name})).unwrap();
    app.ui.dialog = Some(Dialog::new("deleteParagraphStyle", json!({"name": name, "replaceWith": designcraft_doc::BASIC_PARAGRAPH})));
    confirm(&mut app).expect("OK deletes the style");
    let st = app.session.active().unwrap();
    assert!(st.doc.styles.para(&name).is_none(), "style removed");
    let story = st.doc.stories.get(&designcraft_doc::StoryId(sid)).unwrap();
    assert_eq!(story.paras[0].style, designcraft_doc::BASIC_PARAGRAPH, "text reassigned to the replacement style");
}

#[test]
fn delete_paragraph_style_dialog_defaults_replacement_to_basic_paragraph() {
    let mut app = test_app();
    let name = app.session.execute("style.paragraph.create", &json!({"name": "Caption"})).unwrap()["name"].as_str().unwrap().to_string();
    // No `replaceWith` set — the dialog's default field.
    app.ui.dialog = Some(Dialog::new("deleteParagraphStyle", json!({"name": name})));
    confirm(&mut app).expect("OK deletes the style with the default replacement");
    let st = app.session.active().unwrap();
    assert!(st.doc.styles.para(&name).is_none());
}

#[test]
fn character_style_options_dialog_saves_edits() {
    let mut app = test_app();
    let name = app.session.execute("style.character.create", &json!({"name": "Key Term"})).unwrap()["name"].as_str().unwrap().to_string();
    app.ui.dialog = Some(Dialog::new("characterStyleOptions", json!({"name": name, "c.tracking": 20.0})));
    confirm(&mut app).expect("OK saves the style edit");
    let st = app.session.active().unwrap();
    assert_eq!(st.doc.styles.char_style(&name).unwrap().chars.tracking, Some(20.0));
}

#[test]
fn delete_character_style_dialog_removes_the_style_and_reassigns_text() {
    let mut app = test_app();
    let name = app.session.execute("style.character.create", &json!({"name": "Key Term"})).unwrap()["name"].as_str().unwrap().to_string();
    let sid = app.session.execute("frame.create", &json!({"rect": [36, 36, 300, 200], "content": "text"})).unwrap()["story"].as_u64().unwrap();
    app.session.execute("text.insert", &json!({"text": "Hello"})).unwrap();
    app.session.execute("text.select", &json!({"story": sid, "anchor": 0, "focus": 5})).unwrap();
    app.session.execute("style.character.apply", &json!({"name": name})).unwrap();
    app.ui.dialog = Some(Dialog::new("deleteCharacterStyle", json!({"name": name, "replaceWith": designcraft_doc::NO_CHAR_STYLE})));
    confirm(&mut app).expect("OK deletes the style");
    let st = app.session.active().unwrap();
    assert!(st.doc.styles.char_style(&name).is_none(), "style removed");
    let story = st.doc.stories.get(&designcraft_doc::StoryId(sid)).unwrap();
    assert_eq!(story.chars[0].format.style, designcraft_doc::NO_CHAR_STYLE, "text reassigned to the replacement style");
}

#[test]
fn delete_character_style_dialog_defaults_replacement_to_none() {
    let mut app = test_app();
    let name = app.session.execute("style.character.create", &json!({"name": "Key Term"})).unwrap()["name"].as_str().unwrap().to_string();
    // No `replaceWith` set — the dialog's default field.
    app.ui.dialog = Some(Dialog::new("deleteCharacterStyle", json!({"name": name})));
    confirm(&mut app).expect("OK deletes the style with the default replacement");
    let st = app.session.active().unwrap();
    assert!(st.doc.styles.char_style(&name).is_none());
}

/// File › Export EPUB… opens EPUB Export Options from the document's setting; OK stores a
/// changed Split Document as one undo step, and an unchanged one adds none.
#[test]
fn epub_export_passes_its_options_to_the_engine() {
    use std::io::Read;
    use std::sync::{Arc, Mutex};
    let mut app = test_app();
    app.session.execute("style.paragraph.create", &json!({"name": "Chapter"})).unwrap();
    let r = app.session.execute("frame.create", &json!({"rect": [36, 36, 300, 300], "content": "text", "text": "One\nTwo"})).unwrap();
    let sid = r["story"].as_u64().unwrap();
    for anchor in [0, 4] {
        app.session.execute("text.select", &json!({"story": sid, "anchor": anchor, "focus": anchor})).unwrap();
        app.session.execute("style.paragraph.apply", &json!({"name": "Chapter"})).unwrap();
    }
    let written: Arc<Mutex<Vec<u8>>> = Arc::new(Mutex::new(Vec::new()));
    let sink = written.clone();
    app.services.write = Some(Box::new(move |_: &str, bytes: &[u8]| {
        *sink.lock().unwrap() = bytes.to_vec();
        Ok(())
    }));
    crate::menus::export_epub(&mut app, &json!({"path": "book.epub", "split": {"style": "Chapter"}})).unwrap();
    let bytes = written.lock().unwrap().clone();
    let mut z = zip::ZipArchive::new(std::io::Cursor::new(bytes)).unwrap();
    let mut opf = String::new();
    z.by_name("OEBPS/content.opf").unwrap().read_to_string(&mut opf).unwrap();
    assert!(opf.contains("properties=\"cover-image\""), "the first page is the cover: {opf}");
    assert!(z.by_name("OEBPS/content2.xhtml").is_ok(), "the split override reaches the export");
    assert_eq!(app.session.active().unwrap().doc.settings.epub_split, designcraft_doc::EpubSplit::Off, "an override is not stored");
    crate::menus::export_epub(&mut app, &json!({"path": "book.epub", "cover": false})).unwrap();
    let bytes = written.lock().unwrap().clone();
    let mut z = zip::ZipArchive::new(std::io::Cursor::new(bytes)).unwrap();
    let mut opf = String::new();
    z.by_name("OEBPS/content.opf").unwrap().read_to_string(&mut opf).unwrap();
    assert!(!opf.contains("cover-image"), "a caller's cover: false stays: {opf}");
}

#[test]
fn epub_export_options_store_split_document() {
    use designcraft_doc::EpubSplit;
    let mut app = test_app();
    app.session.execute("style.paragraph.create", &json!({"name": "Chapter"})).unwrap();
    let undo_steps = |app: &crate::DesignApp| app.session.active().map(|st| st.history.undo.len());
    app.run("app.exportEpub", json!({})).unwrap();
    let d = app.ui.dialog.as_mut().unwrap();
    assert_eq!((d.id.as_str(), d.b("split"), d.s("style").as_str()), ("epubExport", false, designcraft_doc::BASIC_PARAGRAPH));
    d.fields.insert("split".into(), json!(true));
    d.fields.insert("style".into(), json!("Chapter"));
    let before = undo_steps(&app);
    confirm(&mut app).unwrap();
    assert_eq!(app.session.active().unwrap().doc.settings.epub_split, EpubSplit::Style("Chapter".into()));
    assert_eq!(undo_steps(&app), before.map(|n| n + 1));
    open_epub_export(&mut app);
    assert_eq!(app.ui.dialog.as_ref().map(|d| (d.b("split"), d.s("mode"), d.s("style"))), Some((true, "style".into(), "Chapter".into())));
    confirm(&mut app).unwrap();
    assert_eq!(undo_steps(&app), before.map(|n| n + 1), "unchanged: no undo step");
    open_epub_export(&mut app);
    app.ui.dialog.as_mut().unwrap().fields.insert("mode".into(), json!("exportTags"));
    confirm(&mut app).unwrap();
    assert_eq!(app.session.active().unwrap().doc.settings.epub_split, EpubSplit::ExportTags);
    open_epub_export(&mut app);
    cancel(&mut app);
    assert_eq!(app.session.active().unwrap().doc.settings.epub_split, EpubSplit::ExportTags, "Cancel stores nothing");
}

#[test]
fn pdf_export_defaults() {
    let d = Dialog::new("pdfExport", json!({}));
    let keys: Vec<&str> = d.fields.iter().map(|(k, _)| k.as_str()).collect();
    // serde_json::Map keeps keys sorted.
    let expected = [
        "bleed",
        "compressImages",
        "flatten",
        "marksBleed",
        "marksCrop",
        "marksOffset",
        "marksPageInfo",
        "marksWeight",
        "preset",
        "spreads",
        "standard",
        "tagged",
    ];
    assert_eq!(keys, expected);
    assert_eq!(d.fields["preset"].as_str(), Some("Desktop Printing"));
    assert_eq!(d.fields["standard"].as_str(), Some("none"));
    assert_eq!(d.fields["compressImages"].as_bool(), Some(false));
    assert_eq!(d.fields["flatten"].as_str(), Some(""));
    assert_eq!(d.fields["spreads"].as_bool(), Some(false));
    assert_eq!(d.fields["bleed"].as_bool(), Some(true));
    assert_eq!(d.fields["marksCrop"].as_bool(), Some(false));
    assert_eq!(d.fields["marksBleed"].as_bool(), Some(false));
    assert_eq!(d.fields["marksPageInfo"].as_bool(), Some(false));
    assert_eq!(d.fields["marksWeight"].as_str(), Some("0.25"));
    assert_eq!(d.fields["marksOffset"].as_str(), Some("6"));
    assert_eq!(d.fields["tagged"].as_bool(), Some(true));
}

#[test]
fn pdf_export_override_keeps_override() {
    let d = Dialog::new("pdfExport", json!({"bleed": false}));
    assert_eq!(d.fields["bleed"].as_bool(), Some(false));
    assert_eq!(d.fields["preset"].as_str(), Some("Desktop Printing"));
    assert_eq!(d.fields["tagged"].as_bool(), Some(true));
}

#[test]
fn pdf_export_preset_high_quality_print_keeps_flatten() {
    let mut d = Dialog::new("pdfExport", json!({}));
    d.fields.insert("flatten".into(), json!("medium"));
    apply_pdf_preset(&mut d, "Desktop Printing");
    assert_eq!(d.s("standard"), "none");
    assert!(!d.b("compressImages"));
    assert!(!d.b("marksCrop"));
    assert!(!d.b("marksBleed"));
    assert!(!d.b("marksPageInfo"));
    assert_eq!(d.s("flatten"), "medium", "a leave-unchanged preset must not touch flatten");
}

#[test]
fn pdf_export_preset_press_quality_keeps_flatten() {
    let mut d = Dialog::new("pdfExport", json!({}));
    d.fields.insert("flatten".into(), json!("high"));
    apply_pdf_preset(&mut d, "Commercial Printing");
    assert_eq!(d.s("standard"), "none");
    assert!(d.b("compressImages"));
    assert!(d.b("marksCrop"));
    assert!(d.b("marksBleed"));
    assert!(!d.b("marksPageInfo"));
    assert_eq!(d.s("flatten"), "high");
}

#[test]
fn pdf_export_preset_smallest_file_size_sets_flatten_low() {
    let mut d = Dialog::new("pdfExport", json!({}));
    apply_pdf_preset(&mut d, "Screen and Email");
    assert_eq!(d.s("standard"), "none");
    assert!(d.b("compressImages"));
    assert!(!d.b("marksCrop"));
    assert!(!d.b("marksBleed"));
    assert!(!d.b("marksPageInfo"));
    assert_eq!(d.s("flatten"), "low");
}

/// The PDF/X-1a preset exports X-1a with the High Resolution flattener and printer's marks.
#[test]
fn pdf_export_preset_pdf_x1a_flattens_at_high_resolution() {
    let mut d = Dialog::new("pdfExport", json!({}));
    apply_pdf_preset(&mut d, "PDF/X-1a:2003");
    assert_eq!(d.s("standard"), "x1a");
    assert_eq!(d.s("flatten"), "high");
    assert!(d.b("marksCrop") && d.b("marksBleed"));
    assert!(PDF_EXPORT_PRESETS.contains(&"PDF/X-1a:2003"));
}

#[test]
fn pdf_export_preset_pdf_x4_keeps_flatten() {
    let mut d = Dialog::new("pdfExport", json!({}));
    d.fields.insert("flatten".into(), json!("medium"));
    apply_pdf_preset(&mut d, "PDF/X-4");
    assert_eq!(d.s("standard"), "x4");
    assert!(!d.b("compressImages"));
    assert!(d.b("marksCrop"));
    assert!(d.b("marksBleed"));
    assert!(!d.b("marksPageInfo"));
    assert_eq!(d.s("flatten"), "medium");
}

#[test]
fn pdf_export_preset_pdf_a2b_keeps_flatten() {
    let mut d = Dialog::new("pdfExport", json!({}));
    d.fields.insert("flatten".into(), json!("low"));
    apply_pdf_preset(&mut d, "PDF/A-2b");
    assert_eq!(d.s("standard"), "a2b");
    assert!(!d.b("compressImages"));
    assert!(!d.b("marksCrop"));
    assert!(!d.b("marksBleed"));
    assert!(!d.b("marksPageInfo"));
    assert_eq!(d.s("flatten"), "low");
}

#[test]
fn open_pdf_export_seeds_fields_from_settings() {
    let mut app = crate::DesignApp::new(designcraft_engine::Session::new(), crate::Services::default());
    app.ui.pdf_export.standard = "x4".into();
    app.ui.pdf_export.tagged = false;
    app.ui.pdf_export.marks_crop = true;
    app.ui.pdf_export.flatten = String::new();
    app.ui.flattener = "medium".into();
    open_pdf_export(&mut app);
    let d = app.ui.dialog.as_ref().unwrap();
    assert_eq!(d.id, "pdfExport");
    assert_eq!(d.s("standard"), "x4");
    assert!(!d.b("tagged"));
    assert!(d.b("marksCrop"));
    assert_eq!(d.s("pages"), "all");
    assert_eq!(d.s("flatten"), "medium", "empty flatten falls back to the global flattener");
    assert_eq!(d.s("title"), "", "no document → empty title");

    // A non-empty persisted flatten is used verbatim, not the global fallback.
    app.ui.pdf_export.flatten = "low".into();
    app.ui.flattener = "medium".into();
    open_pdf_export(&mut app);
    let d = app.ui.dialog.as_ref().unwrap();
    assert_eq!(d.s("flatten"), "low");
}

#[test]
fn pdf_export_preset_custom_changes_nothing() {
    let mut d = Dialog::new("pdfExport", json!({}));
    d.fields.insert("standard".into(), json!("x4"));
    d.fields.insert("compressImages".into(), json!(true));
    d.fields.insert("marksCrop".into(), json!(true));
    d.fields.insert("marksBleed".into(), json!(true));
    d.fields.insert("marksPageInfo".into(), json!(true));
    d.fields.insert("flatten".into(), json!("high"));
    let before = d.fields.clone();
    apply_pdf_preset(&mut d, "Custom");
    assert_eq!(d.fields, before);
}

#[test]
fn confirm_pdf_export_writes_file_and_persists_settings() {
    use std::sync::{Arc, Mutex};
    let mut app = crate::DesignApp::new(designcraft_engine::Session::new(), crate::Services::default());
    app.session.execute("file.new", &json!({})).unwrap();
    let writes: Arc<Mutex<Vec<(String, Vec<u8>)>>> = Arc::new(Mutex::new(Vec::new()));
    let sink = writes.clone();
    app.services.write = Some(Box::new(move |path: &str, bytes: &[u8]| {
        sink.lock().unwrap().push((path.to_string(), bytes.to_vec()));
        Ok(())
    }));
    app.services.pick_save = Some(Box::new(|_name: &str| Some("/tmp/designcraft-test.pdf".to_string())));

    app.ui.dialog = Some(Dialog::new("pdfExport", json!({})));
    {
        let d = app.ui.dialog.as_mut().unwrap();
        d.fields.insert("standard".into(), json!("x4"));
        d.fields.insert("bleed".into(), json!(false));
        d.fields.insert("marksCrop".into(), json!(true));
        d.fields.insert("tagged".into(), json!(false));
        d.fields.insert("title".into(), json!("Report"));
    }
    confirm(&mut app).unwrap();

    let got = writes.lock().unwrap();
    assert_eq!(got.len(), 1, "confirming the dialog writes exactly one file");
    assert_eq!(got[0].0, "/tmp/designcraft-test.pdf");
    assert!(got[0].1.starts_with(b"%PDF"), "the written bytes are a PDF");
    drop(got);

    assert_eq!(app.ui.pdf_export.standard, "x4");
    assert!(!app.ui.pdf_export.bleed);
    assert!(app.ui.pdf_export.marks_crop);
    assert!(!app.ui.pdf_export.tagged);
}

#[test]
fn update_link_asks_before_replacing_edits_made_in_the_layout() {
    let dir = std::env::temp_dir().join(format!("dc-update-link-dialog-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("note.txt");
    std::fs::write(&path, "One").unwrap();
    let mut app = crate::DesignApp::new(designcraft_engine::Session::new(), crate::Services::default());
    app.session.execute("file.new", &json!({})).unwrap();
    let r = app.session.execute("file.place", &json!({"path": path.to_string_lossy(), "link": true, "rect": [72, 72, 300, 200]})).unwrap();
    let sid = r["story"].as_u64().unwrap();
    let text = |app: &crate::DesignApp| app.session.doc().unwrap().doc.story(designcraft_doc::StoryId(sid)).unwrap().text.clone();

    // Not edited in the layout: updates without asking.
    std::fs::write(&path, "Two").unwrap();
    crate::panels::properties::update_links(&mut app, json!({"story": sid}));
    assert!(app.ui.dialog.is_none());
    assert_eq!(text(&app), "Two");

    // Edited in the layout: asks first; Cancel changes nothing.
    app.session.execute("story.setText", &json!({"story": sid, "text": "Mine"})).unwrap();
    std::fs::write(&path, "Three").unwrap();
    crate::panels::properties::update_links(&mut app, json!({"story": sid}));
    let d = app.ui.dialog.as_ref().unwrap();
    assert_eq!((d.id.as_str(), d.s("files").as_str()), ("updateLink", "note.txt"));
    app.ui.dialog = None;
    assert_eq!(text(&app), "Mine");

    // Window › Links › Update Link (all links) asks too; confirming updates.
    crate::menus::activate(&mut app, "links.update", &Value::Null);
    assert_eq!(app.ui.dialog.as_ref().map(|d| d.id.as_str()), Some("updateLink"));
    confirm(&mut app).unwrap();
    assert!(app.ui.dialog.is_none());
    assert_eq!(text(&app), "Three");
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn confirm_pdf_export_cancelled_save_writes_nothing() {
    use std::sync::{Arc, Mutex};
    let mut app = crate::DesignApp::new(designcraft_engine::Session::new(), crate::Services::default());
    app.session.execute("file.new", &json!({})).unwrap();
    let writes: Arc<Mutex<Vec<(String, Vec<u8>)>>> = Arc::new(Mutex::new(Vec::new()));
    let sink = writes.clone();
    app.services.write = Some(Box::new(move |path: &str, bytes: &[u8]| {
        sink.lock().unwrap().push((path.to_string(), bytes.to_vec()));
        Ok(())
    }));
    app.services.pick_save = Some(Box::new(|_name: &str| None));

    app.ui.dialog = Some(Dialog::new("pdfExport", json!({})));
    let r = confirm(&mut app).unwrap();
    assert_eq!(r, Value::Null);
    assert!(writes.lock().unwrap().is_empty(), "cancelling the save picker must not write");
}

use designcraft_doc::{CharAttrs, CharacterStyle};

/// A document with character styles Base (bold) and Emphasis (italic) on the first two letters.
fn styled_app() -> DesignApp {
    let mut app = DesignApp::new(designcraft_engine::Session::new(), crate::Services::default());
    app.run("file.new", json!({})).unwrap();
    app.run("style.character.create", json!({"name": "Base", "chars": {"fontStyle": "Bold"}})).unwrap();
    app.run("style.character.create", json!({"name": "Emphasis", "chars": {"fontStyle": "Italic"}})).unwrap();
    let r = app.run("frame.create", json!({"rect": [72, 72, 300, 200], "content": "text", "text": "Hi there"})).unwrap();
    app.run("text.select", json!({"story": r["story"], "anchor": 0, "focus": 2})).unwrap();
    app.run("style.character.apply", json!({"name": "Emphasis"})).unwrap();
    app
}

/// One control-channel request, as the control port or MCP would send it.
fn ctl(app: &mut DesignApp, method: &str, params: Value) -> Value {
    let (req, _reply) = crate::control::ControlRequest::new(method, params);
    match crate::control::handle(app, &egui::Context::default(), &req) {
        crate::control::Outcome::Done(v) => v,
        crate::control::Outcome::Screenshot { .. } => Value::Null,
    }
}

fn char_style(app: &DesignApp, name: &str) -> Option<CharacterStyle> {
    app.session.active().and_then(|st| st.doc.styles.char_style(name).cloned())
}

#[test]
fn character_style_options_set_exactly_the_edited_attributes() {
    let mut app = styled_app();
    assert_eq!(ctl(&mut app, "ui.dialog.open", json!({"id": "characterStyleOptions", "fields": {"name": "Emphasis"}}))["ok"], true);
    ctl(&mut app, "ui.dialog.set", json!({"field": "c.size", "value": 14}));
    ctl(&mut app, "ui.dialog.set", json!({"field": "c.fill", "value": "[Paper]"}));
    ctl(&mut app, "ui.dialog.set", json!({"field": "basedOn", "value": "Base"}));
    assert_eq!(ctl(&mut app, "ui.dialog.confirm", json!({}))["ok"], true);
    assert!(app.ui.dialog.is_none());
    let st = char_style(&app, "Emphasis").unwrap();
    assert_eq!(st.based_on.as_deref(), Some("Base"));
    assert_eq!(st.chars, CharAttrs { font_style: Some("Italic".into()), size: Some(14.0), fill: Some("[Paper]".into()), ..Default::default() });
}

#[test]
fn character_style_options_unset_cleared_fields_and_rename_every_use() {
    let mut app = styled_app();
    ctl(&mut app, "ui.dialog.open", json!({"id": "characterStyleOptions", "fields": {"name": "Emphasis"}}));
    ctl(&mut app, "ui.dialog.set", json!({"field": "c.fontStyle", "value": null}));
    ctl(&mut app, "ui.dialog.set", json!({"field": "rename", "value": "Strong"}));
    assert_eq!(ctl(&mut app, "ui.dialog.confirm", json!({}))["ok"], true);
    assert!(char_style(&app, "Emphasis").is_none());
    assert!(char_style(&app, "Strong").unwrap().chars.is_empty());
    let st = app.session.active().unwrap();
    let story = st.doc.stories.values().next().unwrap();
    assert!(story.chars.iter().any(|r| r.format.style == "Strong"));
    // A taken name keeps the dialog open with the reason.
    ctl(&mut app, "ui.dialog.open", json!({"id": "characterStyleOptions", "fields": {"name": "Strong"}}));
    ctl(&mut app, "ui.dialog.set", json!({"field": "rename", "value": "Base"}));
    assert_eq!(ctl(&mut app, "ui.dialog.confirm", json!({}))["ok"], false);
    assert!(app.ui.dialog.as_ref().is_some_and(|d| !d.s("status").is_empty()));
    assert!(char_style(&app, "Strong").is_some());
}

#[test]
fn new_character_style_opens_its_options_and_creates_the_style() {
    let mut app = styled_app();
    crate::menus::activate(&mut app, "style.character.create", &Value::Null);
    let d = app.ui.dialog.clone().unwrap();
    assert_eq!((d.id.as_str(), d.b("new"), d.s("rename").as_str()), ("characterStyleOptions", true, "Character Style 1"));
    ctl(&mut app, "ui.dialog.set", json!({"field": "c.size", "value": 9}));
    assert_eq!(ctl(&mut app, "ui.dialog.confirm", json!({}))["ok"], true);
    let st = char_style(&app, "Character Style 1").unwrap();
    assert_eq!((st.based_on, st.chars), (None, CharAttrs { size: Some(9.0), ..Default::default() }));
    crate::menus::activate(&mut app, "style.character.create", &Value::Null);
    assert_eq!(app.ui.dialog.as_ref().unwrap().s("rename"), "Character Style 2");
}

#[test]
fn new_paragraph_style_opens_its_options_and_creates_the_style() {
    let mut app = styled_app();
    crate::menus::activate(&mut app, "style.paragraph.create", &Value::Null);
    let d = app.ui.dialog.clone().unwrap();
    assert_eq!((d.id.as_str(), d.b("new"), d.s("rename").as_str()), ("paragraphStyleOptions", true, "Paragraph Style 1"));
    ctl(&mut app, "ui.dialog.set", json!({"field": "c.size", "value": 20}));
    ctl(&mut app, "ui.dialog.set", json!({"field": "basedOn", "value": designcraft_doc::BASIC_PARAGRAPH}));
    assert_eq!(ctl(&mut app, "ui.dialog.confirm", json!({}))["ok"], true);
    let st = app.session.active().unwrap().doc.styles.para("Paragraph Style 1").cloned().unwrap();
    assert_eq!(st.based_on.as_deref(), Some(designcraft_doc::BASIC_PARAGRAPH));
    assert_eq!(st.chars, CharAttrs { size: Some(20.0), ..Default::default() });
}

#[test]
fn style_rows_open_their_options_except_none() {
    let mut app = styled_app();
    open_style_options(&mut app, false, "Emphasis");
    let d = app.ui.dialog.take().unwrap();
    assert_eq!((d.id.as_str(), d.s("name").as_str()), ("characterStyleOptions", "Emphasis"));
    open_style_options(&mut app, false, designcraft_doc::NO_CHAR_STYLE);
    assert!(app.ui.dialog.is_none());
    open_style_options(&mut app, true, designcraft_doc::BASIC_PARAGRAPH);
    assert_eq!(app.ui.dialog.take().unwrap().id, "paragraphStyleOptions");
}

#[test]
fn style_options_sections_draw() {
    let mut app = styled_app();
    let ctx = egui::Context::default();
    let frame = |app: &mut DesignApp| {
        for _ in 0..2 {
            let raw = egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(1440.0, 900.0))),
                max_texture_side: Some(8192),
                ..Default::default()
            };
            let mut out = ctx.run_ui(raw, |ui| {
                app.logic(&ui.ctx().clone());
                app.ui(ui);
            });
            out.textures_delta.clear();
        }
    };
    for lang in ["", "ar"] {
        app.run("app.language", json!({"lang": lang})).unwrap();
        for fields in [json!({"name": "Emphasis"}), json!({"new": true}), json!({"name": "Emphasis", "c.size": null, "c.underline": false})] {
            for section in ["general", "chars", "advanced", "color", "openType", "underline", "strikethrough"] {
                let mut f = fields.clone();
                f["section"] = json!(section);
                app.ui.dialog = Some(Dialog::new("characterStyleOptions", f));
                frame(&mut app);
                assert!(app.ui.dialog.is_some(), "{section} stays open");
            }
        }
        for section in
            ["general", "chars", "advanced", "border", "shading", "keep", "span", "bullets", "color", "openType", "underline", "strikethrough"]
        {
            for fields in [
                json!({"new": true}),
                json!({"new": true, "p.listType": "bullets", "p.spanColumns": {"kind": "span", "columns": 0}, "p.borderOn": true}),
                json!({"new": true, "p.listType": "numbers", "p.startAt": 1, "p.spanColumns": {"kind": "split", "columns": 2}, "p.keepLinesTogether": true}),
            ] {
                let mut f = fields;
                f["section"] = json!(section);
                app.ui.dialog = Some(Dialog::new("paragraphStyleOptions", f));
                frame(&mut app);
                assert!(app.ui.dialog.is_some(), "{section} stays open");
            }
        }
    }
}

#[test]
fn dragging_the_title_moves_the_dialog_and_dragging_the_body_does_not() {
    use egui::{Event, Modifiers, PointerButton, Pos2, vec2};
    use egui_kittest::{Harness, kittest::Queryable};
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
    let rect = |h: &Harness<'static, DesignApp>| h.ctx.memory(|m| m.area_rect(egui::Id::unique("dialog"))).unwrap();
    let mut app = DesignApp::new(designcraft_engine::Session::new(), crate::Services::default());
    app.run("file.new", json!({})).unwrap();
    app.ui.dialog = Some(Dialog::new("insertTable", Value::Null));
    let mut ready = false;
    let mut h = Harness::builder().with_size(vec2(1200.0, 800.0)).with_step_dt(1.0 / 60.0).build_ui_state(
        move |ui, app: &mut DesignApp| {
            if !ready {
                crate::theme::install_fonts(ui.ctx(), &app.ui.language);
                crate::theme::apply(ui.ctx(), &crate::theme::Tokens::for_brightness(crate::theme::Brightness::default()));
                ready = true;
                return;
            }
            show(app, ui.ctx());
        },
        app,
    );
    h.run_steps(4);
    let centred = rect(&h);
    assert!((centred.center() - egui::pos2(600.0, 400.0)).length() < 1.0, "{centred:?}");
    assert!(centred.width() < 1000.0 && centred.height() < 700.0, "the area is the dialog, not the backdrop: {centred:?}");

    let title = h.get_by_label("Create Table").rect().center();
    drag(&mut h, title, title + vec2(100.0, 60.0));
    let moved = rect(&h);
    assert!((moved.min - centred.min - vec2(100.0, 60.0)).length() < 2.0, "{centred:?} -> {moved:?}");

    let body = egui::pos2(moved.right() - 40.0, moved.center().y);
    drag(&mut h, body, body + vec2(-150.0, 80.0));
    assert_eq!(rect(&h), moved, "a drag in the body leaves the dialog in place");

    // The title row stays inside the top edge.
    let title = h.get_by_label("Create Table").rect().center();
    drag(&mut h, title, title - vec2(0.0, 1000.0));
    assert!(rect(&h).top().abs() < 1.0, "{:?}", rect(&h));

    // Opened again, the dialog starts centred.
    h.state_mut().ui.dialog = None;
    h.run_steps(2);
    h.state_mut().ui.dialog = Some(Dialog::new("insertTable", Value::Null));
    h.run_steps(4);
    assert!((rect(&h).center() - centred.center()).length() < 1.0, "{:?}", rect(&h));

    // Past the left edge, about 100 px of the title row stays inside.
    let title = h.get_by_label("Create Table").rect().center();
    drag(&mut h, title, title - vec2(900.0, 0.0));
    assert!((rect(&h).right() - 100.0).abs() < 2.0, "{:?}", rect(&h));
}

#[test]
fn the_new_document_window_drags_mostly_aside_and_keeps_its_preview() {
    use egui::{Event, Modifiers, PointerButton, vec2};
    use egui_kittest::{Harness, kittest::Queryable};
    let rect = |h: &Harness<'static, DesignApp>| h.ctx.memory(|m| m.area_rect(egui::Id::unique("dialog"))).unwrap();
    let mut app = DesignApp::new(designcraft_engine::Session::new(), crate::Services::default());
    app.run("file.new", json!({"title": "A"})).unwrap();
    app.ui.new_document_preview = true;
    app.run("app.newDocumentDialog", json!({})).unwrap();
    let mut ready = false;
    let mut h = Harness::builder().with_size(vec2(1400.0, 900.0)).with_step_dt(1.0 / 60.0).build_ui_state(
        move |ui, app: &mut DesignApp| {
            if !ready {
                crate::theme::install_fonts(ui.ctx(), &app.ui.language);
                crate::theme::apply(ui.ctx(), &crate::theme::Tokens::for_brightness(crate::theme::Brightness::default()));
                ready = true;
                return;
            }
            show(app, ui.ctx());
        },
        app,
    );
    h.run_steps(6);
    let preview = h.state().session.new_document_preview();
    assert!(preview.is_some(), "Preview is on");
    let centred = rect(&h);
    assert!(centred.width() > 900.0, "{centred:?}");
    let from = h.get_all_by_label("New Document").map(|n| n.rect().center()).min_by(|a, b| a.y.total_cmp(&b.y)).unwrap();
    let to = from + vec2(1300.0, 0.0);
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
    // All but 100 px of the window's width is past the right edge, over the preview.
    assert!((rect(&h).left() - 1300.0).abs() < 2.0, "{:?}", rect(&h));
    let app = h.state();
    assert_eq!(app.ui.dialog.as_ref().map(|d| d.id.as_str()), Some("newDocument"));
    assert_eq!(app.session.new_document_preview(), preview, "moving the window keeps the preview");
    assert_eq!(app.session.documents().len(), 2);
}

#[test]
fn margins_dialog_reads_bare_numbers_in_the_ruler_unit() {
    let mut app = DesignApp::new(designcraft_engine::Session::new(), crate::Services::default());
    app.run("file.new", json!({})).unwrap();
    app.run("document.preferences", json!({"horizontalUnits": "millimeters"})).unwrap();
    crate::menus::activate(&mut app, "layout.marginsAndColumns", &Value::Null);
    for k in ["marginTop", "marginBottom", "marginInside", "marginOutside"] {
        ctl(&mut app, "ui.dialog.set", json!({"field": k, "value": "15"}));
    }
    assert_eq!(ctl(&mut app, "ui.dialog.confirm", json!({}))["ok"], true);
    let top = app.session.active().unwrap().doc.page(0).unwrap().margins.top;
    assert!((top - 15.0 * 72.0 / 25.4).abs() < 0.01, "top margin was {top} pt");
}
fn facing_app(pages: u64) -> DesignApp {
    let mut app = DesignApp::new(designcraft_engine::Session::new(), crate::Services::default());
    app.run("file.new", json!({"pages": pages, "facingPages": true})).unwrap();
    app.canvas_rect = Some(egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(1400.0, 900.0)));
    app
}

fn guide_counts(app: &DesignApp) -> Vec<usize> {
    let doc = &app.session.active().unwrap().doc;
    (0..doc.page_count()).map(|abs| doc.page(abs).unwrap().guides.len()).collect()
}

#[test]
fn create_guides_dialog_targets_the_page_in_view() {
    let mut app = facing_app(6);
    crate::canvas::go_to_page(&mut app, 4);
    crate::menus::activate(&mut app, "layout.createGuides", &Value::Null);
    ctl(&mut app, "ui.dialog.set", json!({"field": "columns", "value": "3"}));
    assert_eq!(ctl(&mut app, "ui.dialog.confirm", json!({}))["ok"], true);
    assert_eq!(guide_counts(&app), vec![0, 0, 0, 0, 4, 0]);
    // "Both pages of the spread" covers the facing pair.
    crate::canvas::go_to_page(&mut app, 1);
    crate::menus::activate(&mut app, "layout.createGuides", &Value::Null);
    ctl(&mut app, "ui.dialog.set", json!({"field": "columns", "value": "2"}));
    ctl(&mut app, "ui.dialog.set", json!({"field": "target", "value": "spread"}));
    assert_eq!(ctl(&mut app, "ui.dialog.confirm", json!({}))["ok"], true);
    assert_eq!(guide_counts(&app), vec![0, 2, 2, 0, 4, 0]);
}

#[test]
fn parent_dialogs_cover_both_pages_of_the_parent_spread_by_default() {
    let mut app = facing_app(4);
    app.run("layout.parents.edit", json!({"on": true})).unwrap();
    crate::menus::activate(&mut app, "layout.marginsAndColumns", &Value::Null);
    assert_eq!(app.ui.dialog.as_ref().unwrap().s("target"), "spread");
    for k in ["marginTop", "marginBottom", "marginInside", "marginOutside"] {
        ctl(&mut app, "ui.dialog.set", json!({"field": k, "value": "20 pt"}));
    }
    assert_eq!(ctl(&mut app, "ui.dialog.confirm", json!({}))["ok"], true);
    crate::menus::activate(&mut app, "layout.createGuides", &Value::Null);
    ctl(&mut app, "ui.dialog.set", json!({"field": "columns", "value": "2"}));
    assert_eq!(ctl(&mut app, "ui.dialog.confirm", json!({}))["ok"], true);
    let doc = &app.session.active().unwrap().doc;
    for pg in &doc.parents[0].pages {
        assert_eq!((pg.margins.top, pg.margins.outside, pg.guides.len()), (20.0, 20.0, 2));
    }
    // Every document page shows the parent's guides.
    assert!((0..doc.page_count()).all(|abs| doc.inherited_guides(abs).len() == 2));
}

#[test]
fn other_dialogs_keep_reading_bare_numbers_as_picas() {
    let mut app = facing_app(2);
    app.run("document.preferences", json!({"horizontalUnits": "millimeters"})).unwrap();
    app.ui.dialog = Some(Dialog::new("newDocument", json!({"width": "15"})));
    assert_eq!(app.ui.dialog.as_ref().unwrap().m("width"), Some(180.0));
}
#[test]
fn character_style_options_set_the_digits() {
    let mut app = styled_app();
    ctl(&mut app, "ui.dialog.open", json!({"id": "characterStyleOptions", "fields": {"name": "Emphasis"}}));
    ctl(&mut app, "ui.dialog.set", json!({"field": "c.digits", "value": "farsi"}));
    assert_eq!(ctl(&mut app, "ui.dialog.confirm", json!({}))["ok"], true);
    assert_eq!(char_style(&app, "Emphasis").unwrap().chars.digits, Some(designcraft_doc::Digits::Farsi));
}
