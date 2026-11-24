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
    app.answer_dialogs(move |request| match request {
        craft_dialogs::Request::File(dialog) => {
            asked.lock().unwrap().push(dialog.file_name.clone().unwrap_or_default());
            craft_dialogs::Answer::Paths(vec!["/tmp/designcraft-file-info.pdf".into()])
        }
        craft_dialogs::Request::Message(_) => craft_dialogs::Answer::Message(craft_dialogs::MessageResult::Yes),
    });
    open_pdf_export(&mut app);
    confirm(&mut app).unwrap();
    app.settle_dialogs();
    assert_eq!(names.lock().unwrap().as_slice(), ["flyer-v3.pdf"]);
    let pdf = String::from_utf8_lossy(&writes.lock().unwrap()[0]).into_owned();
    assert!(pdf.contains("/Title(Spring Catalog)") && !pdf.contains("/Title(flyer-v3)"), "the PDF's title is File Info's");
    // A title given to the export is the PDF's title, even when it is the document's name.
    open_pdf_export(&mut app);
    setup_fields(&mut app, json!({"title": "flyer-v3"}));
    confirm(&mut app).unwrap();
    app.settle_dialogs();
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
fn cell_options_ok_edits_nothing_when_the_picked_stroke_type_is_the_cells() {
    let mut app = DesignApp::new(designcraft_engine::Session::new(), crate::Services::default());
    app.run("file.new", json!({})).unwrap();
    app.run("frame.create", json!({"rect": [72, 72, 400, 400], "content": "text", "caret": true})).unwrap();
    app.run("table.insert", json!({"rows": 2, "cols": 2})).unwrap();
    app.run("table.select", json!({"what": "table"})).unwrap();
    app.run("app.cellOptionsDialog", json!({})).unwrap();
    let d = app.ui.dialog.as_mut().unwrap();
    assert_eq!(d.fields["seed"]["type"], json!({"kind": "solid"}));
    // The Type list picks a stroke type by name.
    d.fields.insert("type".into(), json!("solid"));
    let (undo, revision) = {
        let st = app.session.active().unwrap();
        (st.history.undo.len(), st.revision)
    };
    confirm(&mut app).unwrap();
    let st = app.session.active().unwrap();
    assert_eq!((st.history.undo.len(), st.revision), (undo, revision), "no edit and no undo step");
}

#[test]
fn cell_options_edge_change_reseeds_a_picked_stroke_type_that_was_the_cells() {
    let mut app = DesignApp::new(designcraft_engine::Session::new(), crate::Services::default());
    app.run("file.new", json!({})).unwrap();
    app.run("frame.create", json!({"rect": [72, 72, 400, 400], "content": "text", "caret": true})).unwrap();
    app.run("table.insert", json!({"rows": 2, "cols": 2})).unwrap();
    app.run("table.select", json!({"what": "table"})).unwrap();
    app.run("table.setCell", json!({"stroke": {"type": "dotted", "edges": "innerHorizontal"}})).unwrap();
    app.run("app.cellOptionsDialog", json!({})).unwrap();
    let mut d = app.ui.dialog.take().unwrap();
    cell_options_edges_changed(&mut app, &mut d, &[true, true, true, true, false, false]);
    assert_eq!(d.fields["seed"]["type"], json!({"kind": "solid"}));
    // Solid picked by name for the outer edges is what they show; the inner horizontal edges
    // chosen next show their own type.
    d.fields.insert("type".into(), json!("solid"));
    cell_options_edges_changed(&mut app, &mut d, &[false, false, false, false, true, false]);
    assert_eq!(d.fields["type"], json!({"kind": "dotted"}));
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
fn cell_options_menu_items_need_a_table_target() {
    let mut app = DesignApp::new(designcraft_engine::Session::new(), crate::Services::default());
    app.run("file.new", json!({})).unwrap();
    app.run("frame.create", json!({"rect": [72, 72, 400, 400], "content": "text", "caret": true})).unwrap();
    let items = ["app.cellOptionsDialog", "app.diagonalLinesDialog"];
    for id in items {
        assert!(!crate::menus::menu_enabled(&app, id), "{id} without a table");
    }
    app.run("table.insert", json!({"rows": 2, "cols": 2})).unwrap();
    for id in items {
        assert!(crate::menus::menu_enabled(&app, id), "{id} in a table");
    }
}

/// A document holding a 2×2 table with cell (0, 0) selected; returns the table id.
fn table_document_for_diagonals(app: &mut DesignApp) -> u64 {
    app.run("file.new", json!({})).unwrap();
    app.run("frame.create", json!({"rect": [72, 72, 400, 400], "content": "text", "caret": true})).unwrap();
    let tid = app.run("table.insert", json!({"rows": 2, "cols": 2})).unwrap()["table"].as_u64().unwrap();
    app.run("table.select", json!({"rows": [0, 0], "cols": [0, 0]})).unwrap();
    tid
}

/// The diagonal lines of the cells of table `tid` in document `doc`, row by row.
fn diagonal_lines_of(app: &DesignApp, doc: usize, tid: u64) -> Vec<designcraft_doc::DiagonalLines> {
    let st = &app.session.documents()[doc];
    let t = st.doc.stories.values().find_map(|s| s.tables.get(&tid)).unwrap();
    t.cells.iter().map(|c| c.diagonal.lines).collect()
}

#[test]
fn diagonal_lines_dialog_applies_to_the_cells_and_document_it_opened_for() {
    use designcraft_doc::DiagonalLines::{Crossed, None as No, TopRight};
    let mut app = DesignApp::new(designcraft_engine::Session::new(), crate::Services::default());
    let a = table_document_for_diagonals(&mut app);
    // The selection moves to another cell while the dialog is open: OK still edits cell (0, 0).
    app.run("app.diagonalLinesDialog", json!({})).unwrap();
    app.run("table.select", json!({"rows": [1, 1], "cols": [1, 1]})).unwrap();
    app.ui.dialog.as_mut().unwrap().fields.insert("lines".into(), json!("crossed"));
    confirm(&mut app).unwrap();
    assert!(app.ui.dialog.is_none());
    assert_eq!(diagonal_lines_of(&app, 0, a), [Crossed, No, No, No]);
    // Another document with a table selection becomes active: OK is refused and changes neither
    // document, and the dialog stays open.
    app.run("table.select", json!({"rows": [0, 0], "cols": [0, 0]})).unwrap();
    app.run("app.diagonalLinesDialog", json!({})).unwrap();
    let b = table_document_for_diagonals(&mut app);
    app.ui.dialog.as_mut().unwrap().fields.insert("lines".into(), json!("topRight"));
    assert!(confirm(&mut app).is_err());
    assert!(app.ui.dialog.is_some(), "the dialog stays open while its document is open");
    assert_eq!(diagonal_lines_of(&app, 1, b), [No; 4]);
    assert_eq!(diagonal_lines_of(&app, 0, a), [Crossed, No, No, No]);
    // Back in the first document, OK applies there.
    app.run("file.activate", json!({"index": 0})).unwrap();
    confirm(&mut app).unwrap();
    assert_eq!(diagonal_lines_of(&app, 0, a), [TopRight, No, No, No]);
    // A row is added above the target cell while the dialog is open: the document changed and the
    // saved range would name another cell, so OK is refused, nothing changes and the dialog closes.
    app.run("app.diagonalLinesDialog", json!({})).unwrap();
    app.ui.dialog.as_mut().unwrap().fields.insert("lines".into(), json!("crossed"));
    app.run("table.insertRowAbove", json!({})).unwrap();
    assert!(confirm(&mut app).is_err());
    assert!(app.ui.dialog.is_none(), "a dialog whose table changed closes");
    // The new row takes the formatting of the row below it.
    assert_eq!(diagonal_lines_of(&app, 0, a), [TopRight, No, TopRight, No, No, No]);
    // The first document is closed while the dialog is open: OK is refused and the dialog closes.
    app.run("app.diagonalLinesDialog", json!({})).unwrap();
    app.ui.dialog.as_mut().unwrap().fields.insert("lines".into(), json!("crossed"));
    app.run("file.close", json!({"index": 0})).unwrap();
    assert!(confirm(&mut app).is_err());
    assert!(app.ui.dialog.is_none(), "a dialog whose document was closed closes");
    assert_eq!(diagonal_lines_of(&app, 0, b), [No; 4]);
}

#[test]
fn diagonal_lines_dialog_is_refused_when_new_contents_reuse_the_old_address() {
    use designcraft_doc::DiagonalLines::None as No;
    let mut app = DesignApp::new(designcraft_engine::Session::new(), crate::Services::default());
    let tid = table_document_for_diagonals(&mut app);
    app.run("app.diagonalLinesDialog", json!({})).unwrap();
    app.ui.dialog.as_mut().unwrap().fields.insert("lines".into(), json!("crossed"));
    // A row is added above the target cell, and the edit is undone: the contents the dialog
    // opened with are back, and the undo stack no longer holds them.
    app.run("table.insertRowAbove", json!({})).unwrap();
    let edited = (*app.session.active().unwrap().doc).clone();
    app.session.execute("edit.undo", &json!({})).unwrap();
    // The edited contents are written in place when nothing else holds the old contents, at the
    // address an allocation that reuses freed memory would give them. The saved cell range now
    // names the new row.
    let st = app.session.active_mut().unwrap();
    *std::sync::Arc::make_mut(&mut st.doc) = edited;
    assert!(confirm(&mut app).is_err());
    assert!(app.ui.dialog.is_none(), "a dialog whose table changed closes");
    assert_eq!(diagonal_lines_of(&app, 0, tid), [No; 6]);
}

#[test]
fn diagonal_lines_dialog_ok_edits_nothing_when_the_picked_stroke_type_is_the_cells() {
    let mut app = DesignApp::new(designcraft_engine::Session::new(), crate::Services::default());
    table_document_for_diagonals(&mut app);
    app.run("app.diagonalLinesDialog", json!({})).unwrap();
    let d = app.ui.dialog.as_mut().unwrap();
    assert_eq!(d.fields["seed"]["type"], json!({"kind": "solid"}));
    // The Type list picks a stroke type by name.
    d.fields.insert("type".into(), json!("solid"));
    let (undo, revision) = {
        let st = app.session.active().unwrap();
        (st.history.undo.len(), st.revision)
    };
    confirm(&mut app).unwrap();
    let st = app.session.active().unwrap();
    assert_eq!((st.history.undo.len(), st.revision), (undo, revision), "no edit and no undo step");
}

#[cfg(not(target_arch = "wasm32"))]
#[test]
fn diagonal_lines_dialog_is_refused_after_its_document_is_reverted() {
    use designcraft_doc::DiagonalLines::None as No;
    let mut app = DesignApp::new(designcraft_engine::Session::new(), crate::Services::default());
    let tid = table_document_for_diagonals(&mut app);
    let dir = tempfile::tempdir().unwrap();
    app.run("file.saveAs", json!({"path": dir.path().join("diagonal.designcraft")})).unwrap();
    app.run("table.setCell", json!({"row": 1, "col": 1, "text": "unsaved"})).unwrap();
    app.run("app.diagonalLinesDialog", json!({})).unwrap();
    app.ui.dialog.as_mut().unwrap().fields.insert("lines".into(), json!("crossed"));
    app.session.execute("file.revert", &json!({})).unwrap();
    // The reverted document holds the same table; a selection in it does not let OK through.
    let story = app.session.active().unwrap().doc.stories.iter().find(|(_, s)| s.tables.contains_key(&tid)).map(|(k, _)| k.0).unwrap();
    app.run("text.select", json!({"story": story, "anchor": 0, "focus": 0})).unwrap();
    app.run("table.select", json!({"rows": [0, 0], "cols": [0, 0]})).unwrap();
    assert!(confirm(&mut app).is_err());
    assert!(app.ui.dialog.is_none(), "a dialog whose document was reverted closes");
    assert_eq!(diagonal_lines_of(&app, 0, tid), [No; 4]);
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

fn fill(app: &DesignApp, id: u64) -> designcraft_color::Color {
    let d = &app.session.active().unwrap().doc;
    d.resolve_color(&d.item(designcraft_doc::ItemId(id)).unwrap().fill.swatch, 1.0).unwrap()
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
    let ctx = crate::test_window::context();
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
    let ctx = crate::test_window::context();
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
    let ctx = crate::test_window::context();
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
fn paragraph_style_options_keep_a_bad_grep_pattern_open_with_the_error() {
    let (mut app, _) = ruled_paragraph();
    let ctx = crate::test_window::context();
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
    let ctx = crate::test_window::context();
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

#[test]
fn justification_dialog_applies_to_a_local_paragraph_and_cancel_changes_nothing() {
    let (mut app, story) = two_paragraphs();
    let ctx = crate::test_window::context();
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

    // Cancel (Escape) leaves the paragraph alone. The shipping window runs the framework's forms on the installed runtime,
    // which takes the Escape for the dialog; the runtime strips it from egui's input for the legacy form.
    app.install_framework(&ctx);
    let installed = |app: &mut DesignApp, events: Vec<egui::Event>| {
        let mut input = egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(1400.0, 900.0))),
            events,
            ..Default::default()
        };
        app.raw_input_hook(&ctx, &mut input);
        let mut out = ctx.run_ui(input, |ui| {
            app.logic(&ui.ctx().clone());
            app.ui(ui);
        });
        out.textures_delta.clear();
    };
    app.run("text.select", json!({"story": story, "anchor": 0, "focus": 0})).unwrap();
    app.run("app.justificationDialog", json!({})).unwrap();
    for _ in 0..3 {
        installed(&mut app, vec![]);
    }
    app.ui.dialog.as_mut().unwrap().fields.insert("wordSpaceDesired".into(), json!(1.2));
    installed(&mut app, vec![]);
    let before = undo_steps(&app);
    let escape = egui::Event::Key { key: egui::Key::Escape, physical_key: None, pressed: true, repeat: false, modifiers: Default::default() };
    installed(&mut app, vec![escape]);
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
    let ctx = crate::test_window::context();
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

/// Paragraph Style Options › Justification sets Auto Leading and Single Word Justification on the
/// style, and refuses an Auto Leading out of range.
#[test]
fn paragraph_style_options_justification_sets_auto_leading_and_single_word_justification() {
    let mut app = DesignApp::new(designcraft_engine::Session::new(), crate::Services::default());
    app.run("file.new", json!({})).unwrap();
    app.run("style.paragraph.create", json!({"name": "Headline"})).unwrap();
    let ctx = crate::test_window::context();
    app.ui.dialog = Some(Dialog::new("paragraphStyleOptions", json!({"name": "Headline", "section": "justify"})));
    draw(&mut app, &ctx);
    app.ui.dialog.as_mut().unwrap().fields.insert("p.autoLeading".into(), json!(6.0));
    assert!(confirm(&mut app).unwrap_err().contains("Auto Leading"));
    let style = |app: &DesignApp| app.session.doc().unwrap().doc.styles.para("Headline").unwrap().para.clone();
    assert_eq!(style(&app).auto_leading, None, "nothing saved");
    app.ui.dialog.as_mut().unwrap().fields.insert("p.autoLeading".into(), json!(1.0));
    app.ui.dialog.as_mut().unwrap().fields.insert("p.singleWordJustify".into(), json!("right"));
    draw(&mut app, &ctx);
    confirm(&mut app).unwrap();
    let para = style(&app);
    assert_eq!((para.auto_leading, para.single_word_justify), (Some(1.0), Some(designcraft_doc::Align::Right)));
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

/// The frames Text Frame Options took while typing belong to the document it opened on: when
/// another document becomes active before OK, OK changes nothing there.
#[test]
fn text_frame_options_from_the_type_tool_stay_with_their_document() {
    let mut app = crate::DesignApp::new(designcraft_engine::Session::new(), crate::Services::default());
    let make = |app: &mut crate::DesignApp| {
        app.run("file.new", json!({})).unwrap();
        let r = app.run("frame.create", json!({"rect": [36, 36, 300, 300], "content": "text", "text": "Hello"})).unwrap();
        let (id, story) = (r["id"].as_u64().unwrap(), r["story"].as_u64().unwrap());
        app.run("text.select", json!({"story": story, "anchor": 2})).unwrap();
        id
    };
    let a = make(&mut app);
    let mut d = Dialog::new("textFrameOptions", json!({}));
    seed_text_frame_options(&app, &mut d);
    assert_eq!(d.fields.get("_targets"), Some(&json!([a])));
    d.fields.insert("columns".into(), json!(3));
    // A second document, with a caret in a frame that has the same id, becomes active.
    let b = make(&mut app);
    assert_eq!(a, b);
    app.ui.dialog = Some(d);
    let err = confirm(&mut app).unwrap_err().to_string();
    assert!(err.contains("no longer the active one"), "{err}");
    let columns = app.session.active().unwrap().doc.item(designcraft_doc::ItemId(b)).and_then(|i| i.text_frame().map(|t| t.options.columns));
    assert_eq!(columns, Some(1), "the other document's frame is unchanged");
}

/// Revert replaces the document while Text Frame Options taken while typing stay open: OK is
/// refused, since the saved frame ids can name another frame in the reverted document.
#[cfg(not(target_arch = "wasm32"))]
#[test]
fn text_frame_options_from_the_type_tool_are_refused_after_revert() {
    let mut app = crate::DesignApp::new(designcraft_engine::Session::new(), crate::Services::default());
    app.run("file.new", json!({})).unwrap();
    let dir = tempfile::tempdir().unwrap();
    app.run("file.saveAs", json!({"path": dir.path().join("revert.designcraft")})).unwrap();
    let a = app.run("frame.create", json!({"rect": [36, 36, 300, 120], "content": "text", "text": "Unsaved"})).unwrap();
    app.run("text.select", json!({"story": a["story"], "anchor": 1})).unwrap();
    let mut d = Dialog::new("textFrameOptions", json!({}));
    seed_text_frame_options(&app, &mut d);
    d.fields.insert("columns".into(), json!(3));
    app.run("file.revert", json!({})).unwrap();
    let b = app.run("frame.create", json!({"rect": [36, 200, 300, 300], "content": "text", "text": "After"})).unwrap();
    assert_eq!(a["id"], b["id"], "the reverted document gives a new frame the same id");
    app.ui.dialog = Some(d);
    let err = confirm(&mut app).unwrap_err().to_string();
    assert!(err.contains("no longer the active one"), "{err}");
    assert!(app.ui.dialog.is_some(), "the dialog stays open");
    let b = designcraft_doc::ItemId(b["id"].as_u64().unwrap());
    let columns = app.session.active().unwrap().doc.item(b).and_then(|i| i.text_frame().map(|t| t.options.columns));
    assert_eq!(columns, Some(1), "the new frame is unchanged");
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

/// A non-rectangular frame (an oval, or a rectangle with rounded corners) shows one Inset, the
/// distance kept from its outline, and OK sends it as one value for all four sides. A rectangle
/// keeps the four side fields.
#[test]
fn text_frame_options_set_one_inset_for_a_non_rectangular_frame() {
    let mut app = crate::DesignApp::new(designcraft_engine::Session::new(), crate::Services::default());
    app.run("file.new", json!({})).unwrap();
    let oval = app.run("frame.create", json!({"rect": [36, 36, 336, 236], "shape": "ellipse", "content": "text", "text": "Hello"})).unwrap();
    let oval = designcraft_doc::ItemId(oval["id"].as_u64().unwrap());
    let rect = app.run("frame.create", json!({"rect": [36, 300, 336, 500], "content": "text", "text": "Hello"})).unwrap();
    let rect = designcraft_doc::ItemId(rect["id"].as_u64().unwrap());
    app.run("object.textFrameOptions", json!({"ids": [oval.0, rect.0], "inset": [6, 2, 3, 4]})).unwrap();
    let opts = |app: &DesignApp, id| app.session.doc().unwrap().doc.item(id).unwrap().text_frame().unwrap().options.clone();
    let open = |app: &mut DesignApp, id: designcraft_doc::ItemId| {
        app.run("selection.set", json!({"ids": [id.0]})).unwrap();
        let mut d = Dialog::new("textFrameOptions", json!({}));
        seed_text_frame_options(app, &mut d);
        d
    };
    // The oval shows its top inset as the one Inset.
    let d = open(&mut app, oval);
    assert!(d.b("_shaped"));
    assert_eq!(d.s("inset"), "0p6");
    // OK without edits changes nothing, and adds no undo step.
    let undo = app.session.doc().unwrap().history.undo.len();
    app.ui.dialog = Some(d);
    assert_eq!(confirm(&mut app).unwrap(), Value::Null);
    assert_eq!(app.session.doc().unwrap().history.undo.len(), undo);
    // Inset set to 1p0: all four sides take it.
    let mut d = open(&mut app, oval);
    d.fields.insert("inset".into(), json!("1p0"));
    app.ui.dialog = Some(d);
    confirm(&mut app).unwrap();
    assert_eq!(opts(&app, oval).inset, [12.0; 4]);
    assert_eq!(app.session.doc().unwrap().history.undo.len(), undo + 1, "one undo step");
    // A value that isn't a measure is refused and the frame keeps its inset.
    let mut d = open(&mut app, oval);
    d.fields.insert("inset".into(), json!("not a measure"));
    app.ui.dialog = Some(d);
    assert!(confirm(&mut app).unwrap_err().to_string().contains("the inset is not a measure"));
    assert_eq!(opts(&app, oval).inset, [12.0; 4]);
    // A rectangle shows the four sides; with rounded corners it is not rectangular either.
    let d = open(&mut app, rect);
    assert!(!d.b("_shaped") && !d.fields.contains_key("inset"), "a rectangle shows the four sides");
    app.run("object.cornerOptions", json!({"ids": [rect.0], "shape": "rounded", "size": 12})).unwrap();
    let d = open(&mut app, rect);
    assert!(d.b("_shaped"));
    assert_eq!(d.s("inset"), "0p6");
}

/// A selection that mixes a non-rectangular frame with a rectangle shows the four sides, even with
/// the oval first, and a changed side leaves the rectangle's other sides as they were.
#[test]
fn text_frame_options_show_four_sides_for_a_mixed_selection() {
    let mut app = crate::DesignApp::new(designcraft_engine::Session::new(), crate::Services::default());
    app.run("file.new", json!({})).unwrap();
    let oval = app.run("frame.create", json!({"rect": [36, 36, 336, 236], "shape": "ellipse", "content": "text", "text": "Hello"})).unwrap();
    let oval = designcraft_doc::ItemId(oval["id"].as_u64().unwrap());
    let rect = app.run("frame.create", json!({"rect": [36, 300, 336, 500], "content": "text", "text": "Hello"})).unwrap();
    let rect = designcraft_doc::ItemId(rect["id"].as_u64().unwrap());
    app.run("object.textFrameOptions", json!({"ids": [oval.0, rect.0], "inset": [6, 2, 3, 4]})).unwrap();
    let opts = |app: &DesignApp, id| app.session.doc().unwrap().doc.item(id).unwrap().text_frame().unwrap().options.clone();
    app.run("selection.set", json!({"ids": [oval.0, rect.0]})).unwrap();
    let mut d = Dialog::new("textFrameOptions", json!({}));
    seed_text_frame_options(&app, &mut d);
    assert!(!d.b("_shaped") && !d.fields.contains_key("inset"), "a mixed selection shows the four sides");
    d.fields.insert("insetTop".into(), json!("1p0"));
    app.ui.dialog = Some(d);
    confirm(&mut app).unwrap();
    assert_eq!(opts(&app, rect).inset, [12.0, 2.0, 3.0, 4.0]);
    assert_eq!(opts(&app, oval).inset, [12.0, 2.0, 3.0, 4.0]);
    // Two non-rectangular frames still show the one Inset.
    app.run("object.cornerOptions", json!({"ids": [rect.0], "shape": "rounded", "size": 12})).unwrap();
    let mut d = Dialog::new("textFrameOptions", json!({}));
    seed_text_frame_options(&app, &mut d);
    assert!(d.b("_shaped"));
}

/// With the Type tool, text selected through an oval and a threaded rectangle shows the four
/// sides even when the caret's frame is the oval, and a changed side leaves the rectangle's other
/// sides as they were.
#[test]
fn text_frame_options_show_four_sides_for_text_through_an_oval_and_a_rectangle() {
    let mut app = crate::DesignApp::new(designcraft_engine::Session::new(), crate::Services::default());
    app.run("file.new", json!({})).unwrap();
    let text = (0..40).map(|i| format!("Line {i} of the threaded story.")).collect::<Vec<_>>().join(" ");
    let oval = app.run("frame.create", json!({"rect": [36, 36, 236, 136], "shape": "ellipse", "content": "text", "text": text})).unwrap();
    let sid = oval["story"].as_u64().unwrap();
    let oval = designcraft_doc::ItemId(oval["id"].as_u64().unwrap());
    let rect = app.run("frame.create", json!({"rect": [36, 200, 336, 700], "content": "text", "text": "", "caret": false})).unwrap();
    let rect = designcraft_doc::ItemId(rect["id"].as_u64().unwrap());
    app.run("text.thread", json!({"from": oval.0, "to": rect.0})).unwrap();
    app.run("object.textFrameOptions", json!({"ids": [oval.0, rect.0], "inset": [6, 2, 3, 4]})).unwrap();
    let opts = |app: &DesignApp, id| app.session.doc().unwrap().doc.item(id).unwrap().text_frame().unwrap().options.clone();
    let select_through_both = |app: &mut DesignApp| {
        app.run("text.select", json!({"story": sid, "anchor": 0, "focus": text.len()})).unwrap();
        // The caret's stored frame is the oval.
        if let Some(t) = app.session.active_mut().unwrap().selection.text.as_mut() {
            t.frame = Some(oval);
        }
        assert_eq!(designcraft_engine::cmd::text_frame_options_frames(&app.session), vec![oval, rect], "the selected text runs through both frames");
    };
    select_through_both(&mut app);
    let mut d = Dialog::new("textFrameOptions", json!({}));
    seed_text_frame_options(&app, &mut d);
    assert!(!d.b("_shaped") && !d.fields.contains_key("inset"), "text through a rectangle shows the four sides");
    d.fields.insert("insetTop".into(), json!("1p0"));
    app.ui.dialog = Some(d);
    confirm(&mut app).unwrap();
    assert_eq!(opts(&app, rect).inset, [12.0, 2.0, 3.0, 4.0]);
    assert_eq!(opts(&app, oval).inset, [12.0, 2.0, 3.0, 4.0]);
    // An even oval does not link the sides while the rectangle's sides are uneven.
    app.run("object.textFrameOptions", json!({"ids": [oval.0], "inset": [5, 5, 5, 5]})).unwrap();
    select_through_both(&mut app);
    let mut d = Dialog::new("textFrameOptions", json!({}));
    seed_text_frame_options(&app, &mut d);
    assert!(!d.b("insetLinked"), "a frame with uneven sides among the targets leaves the link off");
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
fn text_frame_options_link_the_insets_only_when_every_selected_frame_has_even_sides() {
    let mut app = crate::DesignApp::new(designcraft_engine::Session::new(), crate::Services::default());
    app.run("file.new", json!({})).unwrap();
    let frame = |app: &mut DesignApp, rect: [u32; 4], inset: Value| {
        let r = app.run("frame.create", json!({"rect": rect, "content": "text", "text": "Hello"})).unwrap();
        let id = r["id"].as_u64().unwrap();
        app.run("object.textFrameOptions", json!({"ids": [id], "inset": inset})).unwrap();
        (designcraft_doc::ItemId(id), r["story"].clone())
    };
    let (even, _) = frame(&mut app, [36, 36, 236, 236], json!(6));
    let (uneven, uneven_story) = frame(&mut app, [300, 36, 500, 236], json!([1, 2, 3, 4]));
    let (other_even, other_even_story) = frame(&mut app, [36, 300, 236, 500], json!(9));
    let inset = |app: &DesignApp, id| app.session.doc().unwrap().doc.item(id).unwrap().text_frame().unwrap().options.inset;
    let seeded = |app: &DesignApp| {
        let mut d = Dialog::new("textFrameOptions", json!({}));
        seed_text_frame_options(app, &mut d);
        d
    };
    // The frame with even sides is selected first: the uneven one still turns the link off.
    app.run("selection.set", json!({"ids": [even.0, uneven.0]})).unwrap();
    let mut d = seeded(&app);
    assert_eq!(d.s("insetTop"), "0p6");
    assert!(!d.b("insetLinked"), "a selected frame with uneven sides leaves the link off");
    // Top edited while unlinked: only Top changes, and each frame keeps its other sides.
    d.fields.insert("insetTop".into(), json!("1p0"));
    app.ui.dialog = Some(d);
    confirm(&mut app).unwrap();
    assert_eq!(inset(&app, even), [12.0, 6.0, 6.0, 6.0]);
    assert_eq!(inset(&app, uneven), [12.0, 2.0, 3.0, 4.0]);
    // A frame with even sides alone keeps the link on; selected with the uneven one, the link is off.
    app.run("selection.set", json!({"ids": [other_even.0]})).unwrap();
    assert!(seeded(&app).b("insetLinked"));
    app.run("selection.set", json!({"ids": [other_even.0, uneven.0]})).unwrap();
    assert!(!seeded(&app).b("insetLinked"));
    // A caret: the frames the dialog changes decide.
    app.run("text.select", json!({"story": uneven_story, "anchor": 0, "focus": 0})).unwrap();
    assert!(!seeded(&app).b("insetLinked"));
    app.run("text.select", json!({"story": other_even_story, "anchor": 0, "focus": 0})).unwrap();
    assert!(seeded(&app).b("insetLinked"));
    // A story threaded on into the uneven frame: a caret placed in the first frame, or a caret
    // with no stored frame drawn in the first frame, changes only that frame.
    let (first, story) = frame(&mut app, [300, 300, 500, 500], json!(6));
    let (last, _) = frame(&mut app, [300, 520, 500, 700], json!([1, 2, 3, 4]));
    app.run("text.thread", json!({"from": first.0, "to": last.0})).unwrap();
    app.run("text.placeCaret", json!({"frame": first.0, "point": [310, 310]})).unwrap();
    assert!(seeded(&app).b("insetLinked"));
    app.run("text.select", json!({"story": story, "anchor": 0, "focus": 0})).unwrap();
    assert!(seeded(&app).b("insetLinked"));
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

/// EPUB Export Options › Cover Image: a PNG or JPEG file named there (typed, or picked with
/// Choose…) is the cover; left empty, the first page is, as before (#1934).
#[test]
fn epub_export_options_take_a_cover_image_file() {
    use std::io::Read;
    use std::sync::{Arc, Mutex};
    let mut app = test_app();
    let dir = tempfile::tempdir().unwrap();
    let png = dir.path().join("cover.png");
    let bytes = b"\x89PNG\r\n\x1a\ncover".to_vec();
    std::fs::write(&png, &bytes).unwrap();
    let written: Arc<Mutex<Vec<u8>>> = Arc::new(Mutex::new(Vec::new()));
    let sink = written.clone();
    app.services.write = Some(Box::new(move |_: &str, b: &[u8]| {
        *sink.lock().unwrap() = b.to_vec();
        Ok(())
    }));
    app.answer_dialogs(crate::app::file_dialogs::testing::answer_with(Some(dir.path().join("book.epub").to_string_lossy().into_owned())));
    app.run("app.exportEpub", json!({})).unwrap();
    assert_eq!(app.ui.dialog.as_ref().unwrap().s("coverImage"), "", "no file by default: the first page is the cover");
    // Choose… puts the picked file into the field; OK passes it to the export.
    let path = png.to_string_lossy().into_owned();
    set_epub_cover(&mut app, path.clone()).unwrap();
    assert_eq!(app.ui.dialog.as_ref().unwrap().s("coverImage"), path);
    confirm(&mut app).unwrap();
    app.settle_dialogs();
    let out = written.lock().unwrap().clone();
    assert!(!out.is_empty(), "the EPUB was written");
    let mut z = zip::ZipArchive::new(std::io::Cursor::new(out)).unwrap();
    let mut cover = Vec::new();
    z.by_name("OEBPS/images/cover.png").unwrap().read_to_end(&mut cover).unwrap();
    assert_eq!(cover, bytes, "the file is the cover image");
    let mut opf = String::new();
    z.by_name("OEBPS/content.opf").unwrap().read_to_string(&mut opf).unwrap();
    assert!(opf.contains("properties=\"cover-image\""), "{opf}");
    // The field is per export: the next dialog starts empty again.
    app.run("app.exportEpub", json!({})).unwrap();
    assert_eq!(app.ui.dialog.as_ref().unwrap().s("coverImage"), "");
    cancel(&mut app);
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
    app.answer_dialogs(crate::app::file_dialogs::testing::answer_with(Some("/tmp/designcraft-test.pdf".into())));

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
    app.settle_dialogs();

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
    app.answer_dialogs(crate::app::file_dialogs::testing::answer_with(None));

    app.ui.dialog = Some(Dialog::new("pdfExport", json!({})));
    let r = confirm(&mut app).unwrap();
    assert_eq!(r, Value::Null);
    app.settle_dialogs();
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
    // The selected text's own Italic (its Emphasis style) comes along, as InDesign's New
    // Character Style takes it.
    assert_eq!((st.based_on, st.chars), (None, CharAttrs { size: Some(9.0), font_style: Some("Italic".into()), ..Default::default() }));
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
    // The selected text's own Italic comes along, as InDesign's New Paragraph Style takes it.
    assert_eq!(st.chars, CharAttrs { size: Some(20.0), font_style: Some("Italic".into()), ..Default::default() });
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
    let ctx = crate::test_window::context();
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

/// The eyedropper takes the colour the canvas shows: with Proof Colors the proofed colour, and
/// with the ink-limit preview the warning colour over the limit.
#[test]
fn the_eyedropper_samples_what_proof_colors_and_the_ink_limit_show() {
    use egui_kittest::Harness;
    let mut app = DesignApp::new(designcraft_engine::Session::new(), crate::Services::default());
    app.run("file.new", json!({})).unwrap();
    app.run("swatch.create", json!({"name": "Vivid Green", "color": [0, 255, 0]})).unwrap();
    let id = app.run("frame.create", json!({"rect": [36, 36, 560, 700], "content": "unassigned"})).unwrap()["id"].as_u64().unwrap();
    app.run("object.fill", json!({"ids": [id], "swatch": "Vivid Green"})).unwrap();
    let mut h = crate::test_window::builder().with_size(egui::vec2(1400.0, 900.0)).build_ui_state(
        |ui, app: &mut DesignApp| {
            let ctx = ui.ctx().clone();
            app.logic(&ctx);
            app.ui(ui);
        },
        app,
    );
    h.run_steps(6);
    let target = {
        let app = h.state();
        let doc = &app.session.doc().unwrap().doc;
        let layout = designcraft_tools::CanvasLayout::new(doc, false);
        let v = *app.view().unwrap();
        crate::canvas::Xf::new(app.canvas_rect.unwrap(), &v).to_screen(layout.slots[0].xf * designcraft_geom::Point::new(300.0, 600.0))
    };
    let ctx = h.ctx.clone();
    let sample = |h: &mut Harness<'_, DesignApp>| crate::canvas::color_at(h.state_mut(), &ctx, target).expect("on the page");
    // What a view makes of the plain colour, as the canvas texture gets it.
    let shown = |c: [u8; 3], view: &dyn Fn(&mut designcraft_render::Rendered)| {
        let mut img = designcraft_render::Rendered { width: 1, height: 1, pixels: vec![c[0], c[1], c[2], 255] };
        view(&mut img);
        [img.pixels[0], img.pixels[1], img.pixels[2]]
    };
    let plain = sample(&mut h);
    h.state_mut().ui.proof_colors = true;
    let setup = h.state().ui.proof_setup.clone();
    let proofed = sample(&mut h);
    assert_eq!(proofed, shown(plain, &|img| designcraft_render::proof_view(img, &setup)));
    assert_ne!(proofed, plain, "the proof changes this green");
}

/// The course's chapter 23: the Color Picker's eyedropper, dragged from the picker onto the
/// page and let go there, takes the colour the page shows.
#[test]
fn the_color_pickers_eyedropper_takes_a_colour_off_the_page() {
    use egui_kittest::{Harness, kittest::Queryable};
    let mut app = DesignApp::new(designcraft_engine::Session::new(), crate::Services::default());
    app.run("file.new", json!({})).unwrap();
    let id = app.run("frame.create", json!({"rect": [36, 36, 560, 700], "content": "unassigned"})).unwrap()["id"].as_u64().unwrap();
    app.run("swatch.create", json!({"name": "Eyedrop Red", "color": {"c": 0, "m": 100, "y": 100, "k": 0}})).unwrap();
    app.run("object.fill", json!({"ids": [id], "swatch": "Eyedrop Red"})).unwrap();
    app.ui.dialog = Some(Dialog::new("colorPicker", json!({"target": "fill", "hex": "#000000"})));
    let mut h = crate::test_window::builder().with_size(egui::vec2(1400.0, 900.0)).build_ui_state(
        |ui, app: &mut DesignApp| {
            let ctx = ui.ctx().clone();
            app.logic(&ctx);
            app.ui(ui);
        },
        app,
    );
    h.run_steps(6);
    // Where the frame shows on screen.
    let target = {
        let app = h.state();
        let doc = &app.session.doc().unwrap().doc;
        let layout = designcraft_tools::CanvasLayout::new(doc, false);
        let v = *app.view().unwrap();
        crate::canvas::Xf::new(app.canvas_rect.unwrap(), &v).to_screen(layout.slots[0].xf * designcraft_geom::Point::new(300.0, 600.0))
    };
    let from = h.get_by_role_and_label(egui::accesskit::Role::Button, "Sample color from the screen").rect().center();
    h.hover_at(from);
    h.drag_at(from);
    h.run_steps(1);
    for k in 1..=4 {
        h.hover_at(from + (target - from) * (k as f32 / 4.0));
        h.run_steps(1);
    }
    h.drop_at(target);
    h.run_steps(3);
    let d = h.state().ui.dialog.as_ref().expect("the picker stays open");
    let [r, g, b, _] = h.state().session.doc().unwrap().doc.resolve_color("Eyedrop Red", 1.0).unwrap().to_rgba8(1.0);
    let got: Vec<f64> = d.fields["rgb"].as_array().unwrap().iter().map(|v| v.as_f64().unwrap()).collect();
    for (a, e) in got.iter().zip([r, g, b]) {
        assert!((a - f64::from(e)).abs() <= 3.0, "{got:?} vs {:?}", [r, g, b]);
    }
    assert!(d.b("edited"));
    // Let go off the page (here, on the dialog's own field area): no colour, nothing changes.
    let hex = d.s("hex");
    h.hover_at(from);
    h.drag_at(from);
    h.run_steps(1);
    h.hover_at(from + egui::vec2(-40.0, 0.0));
    h.run_steps(1);
    h.drop_at(egui::pos2(2.0, 2.0));
    h.run_steps(3);
    assert_eq!(h.state().ui.dialog.as_ref().unwrap().s("hex"), hex);
    // Let go over the picker's own colour field, with the page under it: nothing either.
    let field = picker_area(&h, "Blue and green").center();
    let ctx = h.ctx.clone();
    assert!(crate::canvas::color_at(h.state_mut(), &ctx, field).is_some(), "the page lies under the field");
    h.hover_at(from);
    h.drag_at(from);
    h.run_steps(1);
    h.hover_at(field);
    h.run_steps(1);
    h.drop_at(field);
    h.run_steps(3);
    assert_eq!(h.state().ui.dialog.as_ref().unwrap().s("hex"), hex, "the picker isn't sampled");
}


/// The Color Picker laid out as InDesign's: a field and a slider for the component whose radio
/// is on, the new colour over the old one, and the values in every space.
#[test]
fn the_color_picker_keeps_a_typed_cmyk_value_the_colour_shown_does_not_change_with() {
    use table_layout_kit as kit;
    let mut app = kit::app(1, false);
    // The colour the inks give, as the picker keeps typed inks only while they give the colour it shows.
    let hex = designcraft_color::Color::Cmyk { c: 0.0, m: 0.0, y: 0.0, k: 1.0 }.to_hex();
    app.ui.dialog = Some(Dialog::new("colorPicker", json!({"target": "fill", "hex": hex, "cmyk": [0.0, 0.0, 0.0, 100.0], "text": false})));
    let mut h = kit::harness_sized(app, egui::vec2(1400.0, 900.0));
    // A C typed at 100% K is kept, whether or not the colour shown changes with it.
    let rgb = kit::draft(&h).fields.get("rgb").cloned();
    kit::type_and_commit(&mut h, "Cyan", 0, "50");
    let d = kit::draft(&h);
    assert_eq!(d.fields["cmyk"], json!([50.0, 0.0, 0.0, 100.0]), "rgb {:?} (was {rgb:?})", d.fields.get("rgb"));
    assert!(d.b("edited"));
}


/// The colour area of the Color Picker named by its two components (the Red mode's is "Blue and green").
fn picker_area(h: &egui_kittest::Harness<'_, DesignApp>, name: &str) -> egui::Rect {
    use egui_kittest::kittest::Queryable;
    h.query_all_by_label(name)
        .map(|n| n.rect())
        .max_by(|a, b| a.area().total_cmp(&b.area()))
        .unwrap_or_else(|| panic!("no colour area named {name:?}"))
}

#[test]
fn the_color_picker_picks_on_its_field_and_slider_and_adds_a_cmyk_swatch() {
    use egui_kittest::kittest::Queryable;
    use table_layout_kit as kit;
    let mut app = kit::app(1, false);
    app.run("frame.create", json!({"rect": [72, 72, 200, 200], "shape": "rectangle"})).unwrap();
    app.run("object.color", json!({"color":designcraft_color::Color::from_hex("#7c1315").unwrap(),"target":"fill"})).unwrap();
    app.ui.dialog = Some(Dialog::new("colorPicker", json!({"target": "fill", "hex": "#7c1315", "text": false})));
    let mut h = kit::harness_sized(app, egui::vec2(1400.0, 900.0));
    // R at first: R up the slider, B across and G up the field.
    let field = picker_area(&h, "Blue and green");
    kit::click_at(&mut h, egui::pos2(field.right() - 1.0, field.top() + 1.0));
    let p = picked_of(kit::draft(&h));
    assert!((p.rgb[0] - 124.0 / 255.0).abs() < 0.01 && p.rgb[1] > 0.97 && p.rgb[2] > 0.97, "{p:?}: R kept, G and B full");
    // The slider sets R alone.
    let slider = h.get_by_role_and_label(egui::accesskit::Role::Slider, "Red").rect();
    kit::click_at(&mut h, egui::pos2(slider.center().x, slider.bottom() - 1.0));
    let p = picked_of(kit::draft(&h));
    assert!(p.rgb[0] < 0.02 && p.rgb[1] > 0.97, "{p:?}");
    // Hue on the slider: the field is saturation across, brightness up.
    store_picked(h.state_mut().ui.dialog.as_mut().unwrap(), &Picked::from_rgb([124.0 / 255.0, 19.0 / 255.0, 21.0 / 255.0], 0.0));
    h.state_mut().ui.dialog.as_mut().unwrap().fields.insert("component".into(), json!("h"));
    kit::steps(&mut h, 2);
    let field = picker_area(&h, "Saturation and brightness");
    kit::click_at(&mut h, egui::pos2(field.left() + 1.0, field.top() + 1.0));
    let p = picked_of(kit::draft(&h));
    assert!(p.hsb[1] < 0.02 && p.hsb[2] > 0.98 && (p.hsb[0] - 359.0).abs() < 2.0, "{p:?}: no saturation, full brightness, the hue kept");
    // Lab shows its own view.
    h.state_mut().ui.dialog.as_mut().unwrap().fields.insert("component".into(), json!("l"));
    kit::steps(&mut h, 2);
    let _ = picker_area(&h, "Green-red and blue-yellow");
    // Add CMYK Swatch adds the colour as a CMYK swatch; OK applies it.
    store_picked(h.state_mut().ui.dialog.as_mut().unwrap(), &Picked::from_rgb([124.0 / 255.0, 19.0 / 255.0, 21.0 / 255.0], 0.0));
    kit::steps(&mut h, 2);
    let before = h.state().session.active().unwrap().doc.swatches.len();
    kit::click(&mut h, &[egui::accesskit::Role::Button], "Add CMYK Swatch", 0);
    let doc = &h.state().session.active().unwrap().doc;
    assert_eq!(doc.swatches.len(), before + 1);
    assert!(matches!(
        doc.swatches.last().unwrap().value,
        designcraft_color::swatch::SwatchValue::Color { color: designcraft_color::Color::Cmyk { .. }, .. }
    ));
    // The percentages the C/M/Y/K fields show, exactly (not the colour converted back).
    h.state_mut().ui.dialog.as_mut().unwrap().fields.insert("cmyk".into(), json!([0.0, 50.0, 100.0, 0.0]));
    kit::steps(&mut h, 2);
    kit::click(&mut h, &[egui::accesskit::Role::Button], "Add CMYK Swatch", 0);
    let doc = &h.state().session.active().unwrap().doc;
    let designcraft_color::swatch::SwatchValue::Color { color: designcraft_color::Color::Cmyk { c, m, y, k }, .. } =
        doc.swatches.last().unwrap().value
    else {
        panic!("a CMYK swatch")
    };
    assert_eq!([c, m, y, k].map(|v| (v * 100.0).round()), [0.0, 50.0, 100.0, 0.0]);
    // Its own OK applies it.
    kit::click(&mut h, &[egui::accesskit::Role::Button], "OK", 0);
    assert!(h.state().ui.dialog.is_none(), "OK closes it");
    let st = h.state().session.active().unwrap();
    let id = st.selection.items[0];
    let fill = st.doc.item(id).unwrap().fill.swatch.clone();
    assert_eq!(st.doc.resolve_color(&fill, 1.0).unwrap().to_hex(), "#7c1315");
}


/// The Color Picker reopens on the component it was left on, and the choice survives the
/// preferences file; an opener that names a component still gets it.
#[test]
fn the_color_picker_remembers_its_component() {
    use egui::accesskit::Role;
    use table_layout_kit as kit;
    let mut app = kit::app(1, false);
    app.run("frame.create", json!({"rect": [72, 72, 200, 200], "shape": "rectangle"})).unwrap();
    assert_eq!(app.ui.color_picker_component, "r", "R at first, as before");
    app.ui.dialog = Some(Dialog::new("colorPicker", json!({"target": "fill", "hex": "#7c1315", "text": false})));
    let mut h = kit::harness_sized(app, egui::vec2(1400.0, 900.0));
    assert_eq!(kit::draft(&h).s("component"), "r");
    kit::click(&mut h, &[Role::RadioButton], "Hue", 0);
    assert_eq!(kit::draft(&h).s("component"), "h");
    assert_eq!(h.state().ui.color_picker_component, "h", "the choice is kept outside the dialog");
    kit::click(&mut h, &[Role::Button], "Cancel", 0);
    assert!(h.state().ui.dialog.is_none());
    // Reopened: H, not R.
    h.state_mut().ui.dialog = Some(Dialog::new("colorPicker", json!({"target": "fill", "hex": "#7c1315", "text": false})));
    kit::steps(&mut h, 3);
    assert_eq!(kit::draft(&h).s("component"), "h");
    kit::click(&mut h, &[Role::Button], "Cancel", 0);
    // An opener's own choice wins.
    h.state_mut().ui.dialog = Some(Dialog::new("colorPicker", json!({"target": "fill", "hex": "#7c1315", "text": false, "component": "l"})));
    kit::steps(&mut h, 3);
    assert_eq!(kit::draft(&h).s("component"), "l");
    // Saved with the preferences, and read back; a file without it is R.
    let saved = serde_json::to_vec(&h.state().ui).unwrap();
    assert_eq!(crate::UiState::from_preferences_json(&saved).unwrap().color_picker_component, "h");
    assert_eq!(crate::UiState::from_preferences_json(b"{}").unwrap().color_picker_component, "r");
}


#[test]
fn create_guides_dialog_covers_the_spread_in_view() {
    let mut app = facing_app(6);
    // As in InDesign, both pages of the facing spread in view get the guides.
    crate::canvas::go_to_page(&mut app, 1);
    crate::menus::activate(&mut app, "layout.createGuides", &Value::Null);
    assert_eq!(app.ui.dialog.as_ref().unwrap().s("target"), "spread");
    ctl(&mut app, "ui.dialog.set", json!({"field": "columns", "value": "2"}));
    assert_eq!(ctl(&mut app, "ui.dialog.confirm", json!({}))["ok"], true);
    assert_eq!(guide_counts(&app), vec![0, 2, 2, 0, 0, 0]);
    // "This page" narrows them to the page in view.
    crate::canvas::go_to_page(&mut app, 4);
    crate::menus::activate(&mut app, "layout.createGuides", &Value::Null);
    ctl(&mut app, "ui.dialog.set", json!({"field": "columns", "value": "3"}));
    ctl(&mut app, "ui.dialog.set", json!({"field": "target", "value": "page"}));
    assert_eq!(ctl(&mut app, "ui.dialog.confirm", json!({}))["ok"], true);
    assert_eq!(guide_counts(&app), vec![0, 2, 2, 0, 4, 0]);
}

#[test]
fn go_to_row_dialog_selects_the_row_and_shows_its_page() {
    let mut app = DesignApp::new(designcraft_engine::Session::new(), crate::Services::default());
    app.run("file.new", json!({"pages": 2, "facingPages": false})).unwrap();
    app.canvas_rect = Some(egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(1400.0, 900.0)));
    let a = app.run("frame.create", json!({"rect": [72, 72, 400, 200], "content": "text", "text": ""})).unwrap();
    let b = app.run("frame.create", json!({"spread": 1, "rect": [72, 72, 400, 600], "content": "unassigned"})).unwrap();
    app.run("text.thread", json!({"from": a["id"], "to": b["id"]})).unwrap();
    app.run("text.select", json!({"story": a["story"], "anchor": 0, "focus": 0})).unwrap();
    app.run("table.insert", json!({"rows": 20, "cols": 2, "headerRows": 1})).unwrap();
    for row in 0..21 {
        app.run("table.setRowHeight", json!({"row": row, "height": 20, "mode": "exactly"})).unwrap();
    }
    app.run("table.select", json!({"rows": [2, 2]})).unwrap();
    crate::canvas::go_to_page(&mut app, 0);
    crate::menus::activate(&mut app, "table.goToRow", &Value::Null);
    let d = app.ui.dialog.clone().unwrap();
    assert_eq!(d.id, "goToRow");
    assert_eq!((d.s("section"), d.s("row")), ("body".to_string(), "2".to_string()), "it opens on the selected row");
    assert!(d.b("hasHeader") && !d.b("hasFooter"));
    // A row that is not a whole number keeps the dialog open with the reason, and the view stays.
    ctl(&mut app, "ui.dialog.set", json!({"field": "row", "value": "x"}));
    assert_eq!(ctl(&mut app, "ui.dialog.confirm", json!({}))["ok"], false);
    assert!(app.ui.dialog.as_ref().is_some_and(|d| d.id == "goToRow" && d.fields.contains_key("status")));
    assert_eq!(crate::canvas::current_page(&app), Some(0));
    ctl(&mut app, "ui.dialog.set", json!({"field": "row", "value": "20"}));
    assert_eq!(ctl(&mut app, "ui.dialog.confirm", json!({}))["ok"], true);
    assert!(app.ui.dialog.is_none());
    let range = app.session.active().unwrap().selection.cells.unwrap().range;
    assert_eq!((range.r0, range.r1, range.c0, range.c1), (20, 20, 0, 1));
    assert_eq!(crate::canvas::current_page(&app), Some(1), "the view goes to the page that shows the row");
}

/// Go to Row's OK scrolls the row to the middle of the view at the same zoom: a row off screen on the
/// page in view, and a row on another page away from that page's center.
#[test]
fn go_to_row_dialog_scrolls_the_row_to_the_middle_of_the_view() {
    let mut app = DesignApp::new(designcraft_engine::Session::new(), crate::Services::default());
    app.run("file.new", json!({"pages": 2, "facingPages": false})).unwrap();
    app.canvas_rect = Some(egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(1400.0, 900.0)));
    let a = app.run("frame.create", json!({"rect": [72, 72, 400, 200], "content": "text", "text": ""})).unwrap();
    let b = app.run("frame.create", json!({"spread": 1, "rect": [72, 72, 400, 600], "content": "unassigned"})).unwrap();
    app.run("text.thread", json!({"from": a["id"], "to": b["id"]})).unwrap();
    app.run("text.select", json!({"story": a["story"], "anchor": 0, "focus": 0})).unwrap();
    app.run("table.insert", json!({"rows": 20, "cols": 2, "headerRows": 1})).unwrap();
    for row in 0..21 {
        app.run("table.setRowHeight", json!({"row": row, "height": 20, "mode": "exactly"})).unwrap();
    }
    app.run("table.select", json!({"rows": [1, 1]})).unwrap();
    crate::canvas::go_to_page(&mut app, 0);
    crate::canvas::set_zoom(&mut app, 4.0);
    // The row's middle and the view's middle, from top to bottom, on the canvas.
    let middles = |app: &mut DesignApp, row: u64| {
        let r = app.run("table.goToRow", json!({"row": row})).unwrap();
        let spread: designcraft_doc::SpreadRef = serde_json::from_value(r["spread"].clone()).unwrap();
        let n: Vec<f64> = r["rowBounds"].as_array().unwrap().iter().map(|v| v.as_f64().unwrap()).collect();
        let st = app.session.active().unwrap();
        let slot = designcraft_tools::CanvasLayout::new(&st.doc, false).slot(spread).unwrap().xf;
        let row_y = slot.transform_rect_bbox(designcraft_geom::Rect::new(n[0], n[1], n[2], n[3])).center().y;
        let (v, rect) = (app.view().unwrap(), app.canvas_rect.unwrap());
        (row_y, v.origin.y + rect.height() as f64 / 2.0 / v.zoom)
    };
    let x = app.view().unwrap().origin.x;
    for row in ["3", "20"] {
        crate::menus::activate(&mut app, "table.goToRow", &Value::Null);
        ctl(&mut app, "ui.dialog.set", json!({"field": "row", "value": row}));
        assert_eq!(ctl(&mut app, "ui.dialog.confirm", json!({}))["ok"], true);
        let (row_y, view_y) = middles(&mut app, row.parse().unwrap());
        assert!((row_y - view_y).abs() < 0.5, "row {row} is in the middle of the view: {row_y} against {view_y}");
        let v = app.view().unwrap();
        assert!((v.zoom - 4.0).abs() < 1e-9, "the zoom stays");
        assert!((v.origin.x - x).abs() < 1e-9, "the row was in view across, so the view does not scroll sideways");
    }
    assert_eq!(crate::canvas::current_page(&app), Some(1));
}

/// Go to Row's OK goes to the row of the table in the document the dialog was opened for, even
/// when another document became active meanwhile; when that document has closed, nothing is selected.
#[test]
fn go_to_row_dialog_goes_to_the_document_it_was_opened_for() {
    let mut app = DesignApp::new(designcraft_engine::Session::new(), crate::Services::default());
    let table_doc = |app: &mut DesignApp, title: &str| {
        app.run("file.new", json!({"title": title})).unwrap();
        let r = app.run("frame.create", json!({"rect": [72, 72, 500, 700], "content": "text", "text": ""})).unwrap();
        app.run("text.select", json!({"story": r["story"], "anchor": 0, "focus": 0})).unwrap();
        app.run("table.insert", json!({"rows": 3, "cols": 2})).unwrap();
    };
    table_doc(&mut app, "first");
    crate::menus::activate(&mut app, "table.goToRow", &Value::Null);
    table_doc(&mut app, "second");
    setup_fields(&mut app, json!({"row": "3"}));
    confirm(&mut app).unwrap();
    let active = app.session.active().unwrap();
    assert_eq!(active.doc.title, "first", "the dialog's document becomes active");
    assert_eq!(active.selection.cells.map(|c| (c.range.r0, c.range.r1)), Some((2, 2)));
    let second = app.session.documents().iter().find(|d| d.doc.title == "second").unwrap();
    assert!(second.selection.cells.is_none(), "the other document's selection stays");

    crate::menus::activate(&mut app, "table.goToRow", &Value::Null);
    app.run("file.close", json!({})).unwrap();
    let before = app.session.active().unwrap().selection.clone();
    setup_fields(&mut app, json!({"row": "1"}));
    assert_eq!(confirm(&mut app), Ok(Value::Null));
    assert!(app.ui.dialog.is_none());
    assert_eq!(app.session.active().unwrap().selection, before, "the dialog's document closed");
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

/// The course's chapter 40: Paragraph Border's four weights are linked as InDesign's, so a
/// side typed in sets all four; with the link off ("Make all settings the same") a side is set
/// alone, as the lesson gives the border only a thick left side. The corners link the same way.
#[test]
fn paragraph_border_sides_and_corners_link_as_indesigns() {
    use table_layout_kit as kit;
    let (mut app, _) = ruled_paragraph();
    app.ui.dialog = Some(Dialog::new("paragraphStyleOptions", json!({"name": "Ruled Child", "section": "border", "p.borderOn": true})));
    let mut h = kit::harness_sized(app, egui::vec2(1400.0, 1000.0));
    let field = |h: &egui_kittest::Harness<'static, DesignApp>, key: &str| kit::draft(h).fields.get(&format!("p.{key}")).cloned().unwrap_or_default();
    // The four weights start the same, so linked: Top sets them all.
    kit::type_and_commit(&mut h, "Top:", 0, "3");
    assert_eq!(field(&h, "borderWeights"), json!([3.0, 3.0, 3.0, 3.0]));
    // The link of the weights (0), corners (1) or offsets (2), in reading order.
    let link = |h: &mut egui_kittest::Harness<'static, DesignApp>, n: usize| {
        use egui_kittest::kittest::{NodeT, Queryable};
        let mut links: Vec<egui::Rect> = h
            .query_all_by_label("Make all settings the same")
            .filter(|node| kit::TOGGLE.contains(&node.accesskit_node().role()))
            .map(|node| node.rect())
            .collect();
        assert_eq!(links.len(), 3, "weights, corners and offsets each have a link");
        // Top to bottom on the page: the weights', the corners', the offsets'.
        links.sort_by(|a, b| a.center().y.total_cmp(&b.center().y));
        kit::click_at(h, links[n].center());
    };
    // Unlinked, Left alone takes 10 pt.
    link(&mut h, 0);
    assert_eq!(kit::draft(&h).fields.get("_linked.p.borderWeights").or(kit::draft(&h).fields.get("_linked.borderWeights")), Some(&json!(false)), "the click turned the weights' link off: {:?}", kit::draft(&h).fields.keys().filter(|k| k.contains("link")).collect::<Vec<_>>());
    kit::type_and_commit(&mut h, "Left:", 0, "10");
    assert_eq!(field(&h, "borderWeights"), json!([3.0, 10.0, 3.0, 3.0]));
    // Linked again, the four take Top's weight; unlinked once more for the thick left side.
    link(&mut h, 0);
    assert_eq!(field(&h, "borderWeights"), json!([3.0, 3.0, 3.0, 3.0]));
    link(&mut h, 0);
    kit::type_and_commit(&mut h, "Left:", 0, "10");
    assert_eq!(field(&h, "borderWeights"), json!([3.0, 10.0, 3.0, 3.0]));
    // The corners start the same too: Top Left's size goes to all four.
    kit::type_and_commit(&mut h, "Top Left Corner", 0, "0p4");
    let corners = field(&h, "borderCorners");
    assert!((0..4).all(|i| corners[i]["size"] == json!(4.0)), "{corners}");
    // Unlinked, Top Right alone takes 0p8; linked again, all four take Top Left's 4 pt.
    link(&mut h, 1);
    kit::type_and_commit(&mut h, "Top Right Corner", 0, "0p8");
    assert_eq!(field(&h, "borderCorners")[1]["size"], json!(8.0));
    assert_eq!(field(&h, "borderCorners")[0]["size"], json!(4.0));
    link(&mut h, 1);
    let corners = field(&h, "borderCorners");
    assert!((0..4).all(|i| corners[i] == corners[0] && corners[i]["size"] == json!(4.0)), "{corners}");
    confirm(h.state_mut()).unwrap();
    let style = h.state().session.doc().unwrap().doc.styles.para("Ruled Child").cloned().unwrap();
    assert_eq!(style.para.border_weights, Some([3.0, 10.0, 3.0, 3.0]));
}


/// The course's chapter 41: Paragraph Shading as InDesign's. The page is the one the section list has chosen;
/// the tint has Overprint beside it; the corners are a two-by-two grid whose shape menus show
/// the shapes. The lesson rounds the top two corners at 10 pt, unlinked, and moves the
/// offsets.
#[test]
fn paragraph_shading_page_is_laid_out_as_indesigns() {
    use designcraft_geom::corners::CornerShape;
    use table_layout_kit as kit;
    let (mut app, _) = ruled_paragraph();
    app.ui.dialog = Some(Dialog::new("paragraphStyleOptions", json!({"name": "Ruled Child", "section": "shading", "p.shadingOn": true})));
    let mut h = kit::harness_sized(app, egui::vec2(1400.0, 1000.0));
    let field = |h: &egui_kittest::Harness<'static, DesignApp>, key: &str| kit::draft(h).fields.get(&format!("p.{key}")).cloned().unwrap_or_default();
    // The section list names the page it shows (the framework dialog has no separate page heading).
    assert!(kit::shows(&h, "Paragraph Shading"), "the section list names the page");
    kit::click(&mut h, kit::TOGGLE, "Overprint", 0);
    assert_eq!(field(&h, "shadingOverprint"), json!(true));
    // The corners' link (0) and the offsets' (1).
    let link = |h: &mut egui_kittest::Harness<'static, DesignApp>, n: usize| {
        use egui_kittest::kittest::{NodeT, Queryable};
        let links = h.query_all_by_label("Make all settings the same").filter(|node| kit::TOGGLE.contains(&node.accesskit_node().role())).count();
        assert_eq!(links, 2, "corners and offsets each have a link");
        kit::click(h, kit::TOGGLE, "Make all settings the same", n);
    };
    link(&mut h, 0);
    for corner in ["Top Left Corner", "Top Right Corner"] {
        kit::type_and_commit(&mut h, corner, 0, "10 pt");
        kit::pick(&mut h, &format!("{corner} Corner Shape"), "Rounded");
    }
    let corners: [designcraft_geom::corners::Corner; 4] = serde_json::from_value(field(&h, "shadingCorners")).unwrap();
    assert_eq!(
        corners.map(|c| (c.shape, c.size)),
        [(CornerShape::Rounded, 10.0), (CornerShape::Rounded, 10.0), (CornerShape::None, 0.0), (CornerShape::None, 0.0)]
    );
    // The offsets, unlinked: Top alone.
    link(&mut h, 1);
    kit::type_and_commit(&mut h, "Top:", 0, "3 pt");
    assert_eq!(field(&h, "shadingOffsets"), json!([3.0, 0.0, 0.0, 0.0]));
    confirm(h.state_mut()).unwrap();
    let style = h.state().session.doc().unwrap().doc.styles.para("Ruled Child").cloned().unwrap();
    assert_eq!((style.para.shading_on, style.para.shading_overprint), (Some(true), Some(true)));
    assert_eq!(style.para.shading_corners.map(|c| c[1].shape), Some(CornerShape::Rounded));
}


/// The course's chapter 20: a paragraph style's rule below becomes wavy, 3 pt, with a gap
/// colour, from Paragraph Rules › Type and Gap Color.
#[test]
fn paragraph_style_rules_take_a_type_and_a_gap_colour() {
    let (mut app, _) = ruled_paragraph();
    let ctx = crate::test_window::context();
    app.ui.dialog = Some(Dialog::new("paragraphStyleOptions", json!({"name": "Ruled Child", "section": "rules", "rule": "ruleBelow"})));
    draw(&mut app, &ctx);
    if let Some(d) = app.ui.dialog.as_mut() {
        d.fields.insert("p.ruleBelow.type".into(), json!({"kind": "wavy"}));
        d.fields.insert("p.ruleBelow.weight".into(), json!(3.0));
        d.fields.insert("p.ruleBelow.gapColor".into(), json!("[Paper]"));
    }
    draw(&mut app, &ctx);
    confirm(&mut app).unwrap();
    let r = app.session.doc().unwrap().doc.styles.resolve_para_style("Ruled Child").0.rule_below;
    assert_eq!((r.kind, r.weight, r.gap_color.as_str()), (designcraft_doc::StrokeType::Wavy, 3.0, "[Paper]"));
}

/// A rule's Gap Tint shows 100% at its default (the rule's JSON leaves a full gap tint out).
#[test]
fn a_rules_default_gap_tint_shows_100() {
    use egui_kittest::kittest::{NodeT, Queryable};
    let (mut app, _) = ruled_paragraph();
    app.run(
        "style.paragraph.edit",
        json!({"name": "Ruled Child", "para": {"ruleBelow": {"type": {"kind": "dashed", "pattern": [4.0, 2.0]}, "gapColor": "[Paper]"}}}),
    )
    .unwrap();
    app.ui.dialog = Some(Dialog::new("paragraphStyleOptions", json!({"name": "Ruled Child", "section": "rules", "rule": "ruleBelow"})));
    let h = table_layout_kit::harness_sized(app, egui::vec2(1400.0, 1000.0));
    let field = h
        .query_all_by_label("Gap Tint:")
        .find(|n| table_layout_kit::FIELD.contains(&n.accesskit_node().role()))
        .and_then(|n| n.value());
    // The field spells its unit with or without a space ("100%", "100 %"): what it shows is the whole 100.
    assert_eq!(field.map(|text| text.split_whitespace().collect::<String>()).as_deref(), Some("100%"));
}


/// The course's chapter 20: the "Recipe Categories" title formatted on the page (size, All
/// Caps, tracking, space after), then New Paragraph Style from it: the dialog starts from the
/// text, and with Apply Style to Selection the paragraph takes the new style. Next Style and
/// Reset To Base are set from General.
#[test]
fn a_new_paragraph_style_starts_from_the_selected_text() {
    let mut app = DesignApp::new(designcraft_engine::Session::new(), crate::Services::default());
    app.run("file.new", json!({})).unwrap();
    let f = app.run("frame.create", json!({"rect": [36, 36, 400, 300], "content": "text", "text": "Egg Recipes\rBoil them."})).unwrap();
    app.run("text.select", json!({"story": f["story"], "anchor": 0, "focus": 11})).unwrap();
    app.run("type.char", json!({"attrs": {"size": 20, "capitalization": "allCaps", "tracking": 50}})).unwrap();
    app.run("type.para", json!({"attrs": {"spaceAfter": 6}})).unwrap();
    open_new_style(&mut app, true);
    let d = app.ui.dialog.clone().unwrap();
    assert_eq!(
        (d.fields["c.size"].clone(), d.fields["c.capitalization"].clone(), d.fields["p.spaceAfter"].clone()),
        (json!(20.0), json!("allCaps"), json!(6.0))
    );
    assert!(d.b("apply") && !d.fields.contains_key("c.fontFamily"), "only what differs from [No Paragraph Style]");
    if let Some(d) = app.ui.dialog.as_mut() {
        d.fields.insert("rename".into(), json!("Recipe Categories"));
        d.fields.insert("nextStyle".into(), json!(designcraft_doc::BASIC_PARAGRAPH));
    }
    confirm(&mut app).unwrap();
    let a = app.run("type.selectionAttrs", json!({})).unwrap();
    assert_eq!(a["paragraphStyle"], "Recipe Categories", "applied to the selection");
    assert_eq!(a["paraOverrides"].as_u64().unwrap_or(0) + a["charOverrides"].as_u64().unwrap_or(0), 0);
    let style = |app: &DesignApp| app.session.doc().unwrap().doc.styles.para("Recipe Categories").cloned().unwrap();
    assert_eq!(style(&app).next_style.as_deref(), Some(designcraft_doc::BASIC_PARAGRAPH));
    assert_eq!(style(&app).chars.size, Some(20.0));
    // Reset To Base: the style keeps nothing of its own, apart from the edits made then.
    app.ui.dialog = Some(Dialog::new("paragraphStyleOptions", json!({"name": "Recipe Categories", "basedOn": designcraft_doc::BASIC_PARAGRAPH})));
    if let Some(d) = app.ui.dialog.as_mut() {
        d.fields.insert("resetToBase".into(), json!(true));
        d.fields.insert("c.tracking".into(), json!(10.0));
    }
    confirm(&mut app).unwrap();
    let s = style(&app);
    assert_eq!((s.chars.size, s.chars.tracking, s.para.space_after), (None, Some(10.0), None), "{s:?}");
    assert_eq!(s.based_on.as_deref(), Some(designcraft_doc::BASIC_PARAGRAPH));
}

/// A Based On changed in the dialog (without Reset To Base) is what the fields are checked
/// against: the style's own settings over the new parent.
#[test]
fn a_changed_based_on_is_what_the_fields_are_checked_against() {
    let mut app = DesignApp::new(designcraft_engine::Session::new(), crate::Services::default());
    app.run("file.new", json!({})).unwrap();
    app.run("style.paragraph.create", json!({"name": "Loose", "para": {"wordSpaceMin": 1.2, "wordSpaceDesired": 1.3}})).unwrap();
    app.run("style.paragraph.create", json!({"name": "Child", "basedOn": "Loose"})).unwrap();
    let mut d = Dialog::new("paragraphStyleOptions", json!({"name": "Child"}));
    d.fields.insert("p.wordSpaceDesired".into(), json!(1.0));
    assert!(check_paragraph_style_justification(&app, &d).is_err(), "below Loose's minimum");
    d.fields.insert("basedOn".into(), json!(designcraft_doc::BASIC_PARAGRAPH));
    assert_eq!(check_paragraph_style_justification(&app, &d), Ok(()), "[Basic Paragraph]'s minimum is lower");
}

/// With Reset To Base, the justification fields are checked against the base style (the
/// style's own minimum no longer applies), and Preview shows the reset on the page.
#[test]
fn reset_to_base_checks_and_previews_against_the_base() {
    let mut app = DesignApp::new(designcraft_engine::Session::new(), crate::Services::default());
    app.run("file.new", json!({})).unwrap();
    let f = app.run("frame.create", json!({"rect": [36, 36, 400, 300], "content": "text", "text": "Loose words"})).unwrap();
    app.run("text.select", json!({"story": f["story"], "anchor": 0, "focus": 5})).unwrap();
    app.run("style.paragraph.create", json!({"name": "Loose", "basedOn": designcraft_doc::BASIC_PARAGRAPH, "para": {"wordSpaceMin": 1.2, "wordSpaceDesired": 1.3}, "chars": {"size": 30}}))
        .unwrap();
    app.run("style.paragraph.apply", json!({"name": "Loose"})).unwrap();
    let mut d = Dialog::new("paragraphStyleOptions", json!({"name": "Loose"}));
    d.fields.insert("p.wordSpaceDesired".into(), json!(1.1));
    assert!(check_paragraph_style_justification(&app, &d).is_err(), "below the style's own minimum");
    d.fields.insert("resetToBase".into(), json!(true));
    assert_eq!(check_paragraph_style_justification(&app, &d), Ok(()), "the base's minimum is 100%");
    d.fields.insert("preview".into(), json!(true));
    style_options_preview(&mut app, &mut d);
    assert_eq!(d.fields[PREVIEWED]["resetToBase"], true);
    let size = app.run("type.selectionAttrs", json!({})).unwrap()["chars"]["size"].clone();
    assert_eq!(size, json!(12.0), "the page shows the style reset to its base");
    app.session.cancel_interaction();
}

/// What Change Format Settings sets goes to find.change: a paragraph style and character
/// attributes as well as a character style; the box sums them up.
#[test]
fn a_change_format_goes_with_change_all() {
    let mut d = Dialog::new("findChange", json!({"find": "cook"}));
    d.fields.insert(CHANGE_FORMAT.into(), json!({"charStyle": "eggs", "paraStyle": "Method", "chars": {"underline": true}}));
    let p = find_change_params(&d);
    assert_eq!((p["charStyle"].clone(), p["paraStyle"].clone(), p["attrs"].clone()), (json!("eggs"), json!("Method"), json!({"underline": true})));
    assert_eq!(format_summary("en", d.fields.get(CHANGE_FORMAT)), "Character Style: eggs + Paragraph Style: Method + Underline: on");
    assert!(find_change_params(&Dialog::new("findChange", json!({"find": "cook"}))).get("attrs").is_none());
    // A Find Format goes with every search.
    d.fields.insert(FIND_FORMAT.into(), json!({"paraStyle": "Method"}));
    assert_eq!(find_change_params(&d)["findFormat"], json!({"paraStyle": "Method"}));
}

/// Insert Table: the dimensions as typed, and the Table Style chosen is applied to the new
/// table; [Basic Table] (the default) leaves it unstyled.
#[test]
fn insert_table_applies_its_dimensions_and_table_style() {
    let mut app = DesignApp::new(designcraft_engine::Session::new(), crate::Services::default());
    app.run("file.new", json!({})).unwrap();
    app.run("style.table.create", json!({"name": "Grid"})).unwrap();
    for (style, expected) in [("Grid", "Grid"), (designcraft_doc::BASIC_TABLE, "")] {
        app.run("frame.create", json!({"rect": [72, 72, 372, 372], "content": "text"})).unwrap();
        assert_eq!(app.run("app.insertTableDialog", json!({})).unwrap(), Value::Null);
        for (field, value) in [("bodyRows", json!(5)), ("columns", json!(3)), ("headerRows", json!(1)), ("tableStyle", json!(style))] {
            ctl(&mut app, "ui.dialog.set", json!({"field": field, "value": value}));
        }
        assert_eq!(ctl(&mut app, "ui.dialog.confirm", json!({}))["ok"], true);
        let t = app.run("table.get", json!({})).unwrap();
        let len = |k: &str| t[k].as_array().map_or(0, Vec::len);
        assert_eq!((len("rows"), len("columns"), t["headerRows"].as_u64()), (6, 3, Some(1)), "{t}");
        assert_eq!(t["style"].as_str().unwrap_or(""), expected, "{t}");
        // One Insert Table, one undo.
        app.run("edit.undo", json!({})).unwrap();
        assert!(app.run("table.get", json!({})).is_err(), "the table is gone");
    }
    // A null style is no style: the table is made, unstyled.
    app.run("frame.create", json!({"rect": [72, 72, 372, 372], "content": "text"})).unwrap();
    app.run("app.insertTableDialog", json!({})).unwrap();
    ctl(&mut app, "ui.dialog.set", json!({"field": "tableStyle", "value": null}));
    assert_eq!(ctl(&mut app, "ui.dialog.confirm", json!({}))["ok"], true);
    assert_eq!(app.run("table.get", json!({})).unwrap()["style"].as_str().unwrap_or(""), "");
    app.run("edit.undo", json!({})).unwrap();
    // A style that no longer exists changes nothing.
    let tables = |app: &DesignApp| app.session.active().unwrap().doc.stories.values().map(|s| s.tables.len()).sum::<usize>();
    app.run("frame.create", json!({"rect": [72, 72, 372, 372], "content": "text"})).unwrap();
    app.run("app.insertTableDialog", json!({})).unwrap();
    ctl(&mut app, "ui.dialog.set", json!({"field": "tableStyle", "value": "Gone"}));
    assert_eq!(ctl(&mut app, "ui.dialog.confirm", json!({}))["ok"], false);
    assert_eq!(tables(&app), 0);
    let d = app.ui.dialog.as_ref().expect("the dialog stays open to be corrected");
    assert_eq!((d.id.as_str(), d.s("tableStyle")), ("insertTable", "Gone".to_string()));
    // So does a style that isn't a name: the command refuses it, rather than making the table
    // unstyled.
    ctl(&mut app, "ui.dialog.set", json!({"field": "tableStyle", "value": 42}));
    assert_eq!(ctl(&mut app, "ui.dialog.confirm", json!({}))["ok"], false);
    assert_eq!(tables(&app), 0);
    assert_eq!(app.ui.dialog.as_ref().map(|d| d.id.as_str()), Some("insertTable"), "the dialog stays open");
}

/// Convert Text to Table: available with text selected (not with a caret alone); its
/// separators and Number of Columns shape the table. Convert Table to Text takes it back with
/// the separators chosen there.
#[test]
fn convert_dialogs_use_their_separators() {
    let mut app = DesignApp::new(designcraft_engine::Session::new(), crate::Services::default());
    app.run("file.new", json!({})).unwrap();
    let text = "Name:\nAddress:\nCity:\nState:";
    let sid = app.run("frame.create", json!({"rect": [72, 72, 372, 372], "content": "text", "text": text})).unwrap()["story"].clone();
    app.run("text.select", json!({"story": sid, "anchor": 0, "focus": 0})).unwrap();
    assert!(!crate::menus::menu_enabled(&app, "app.convertTextToTableDialog"), "a caret alone");
    assert!(!crate::menus::menu_enabled(&app, "app.convertTableToTextDialog"), "no table");
    app.run("text.select", json!({"story": sid, "anchor": 0, "focus": text.len()})).unwrap();
    assert!(crate::menus::menu_enabled(&app, "app.convertTextToTableDialog"));
    app.run("app.convertTextToTableDialog", json!({})).unwrap();
    assert_eq!(app.ui.dialog.as_ref().map(|d| d.id.as_str()), Some("convertTextToTable"));
    // "comma" and a typed "," are the same separator, so Number of Columns applies; a count of
    // 0 is refused, not taken as 1, and the dialog stays open.
    ctl(&mut app, "ui.dialog.set", json!({"field": "columnSeparator", "value": "comma"}));
    ctl(&mut app, "ui.dialog.set", json!({"field": "rowSeparator", "value": ","}));
    assert!(same_separators(app.ui.dialog.as_ref().unwrap()));
    ctl(&mut app, "ui.dialog.set", json!({"field": "columns", "value": 0}));
    assert_eq!(ctl(&mut app, "ui.dialog.confirm", json!({}))["ok"], false);
    assert_eq!(app.ui.dialog.as_ref().map(|d| d.id.as_str()), Some("convertTextToTable"));
    // An emptied separator: the command refuses it and the dialog stays open, as typed.
    ctl(&mut app, "ui.dialog.set", json!({"field": "rowSeparator", "value": ""}));
    assert_eq!(ctl(&mut app, "ui.dialog.confirm", json!({}))["ok"], false);
    assert_eq!(app.ui.dialog.as_ref().map(|d| (d.id.as_str(), d.s("rowSeparator"))), Some(("convertTextToTable", String::new())));
    for (field, value) in [("columnSeparator", json!("paragraph")), ("rowSeparator", json!("paragraph")), ("columns", json!(2))] {
        ctl(&mut app, "ui.dialog.set", json!({"field": field, "value": value}));
    }
    assert_eq!(ctl(&mut app, "ui.dialog.confirm", json!({}))["ok"], true);
    let t = app.run("table.get", json!({})).unwrap();
    let len = |k: &str| t[k].as_array().map_or(0, Vec::len);
    assert_eq!((len("rows"), len("columns")), (2, 2), "{t}");
    assert!(crate::menus::menu_enabled(&app, "app.convertTableToTextDialog"));
    app.run("app.convertTableToTextDialog", json!({})).unwrap();
    ctl(&mut app, "ui.dialog.set", json!({"field": "columnSeparator", "value": ", "}));
    assert_eq!(ctl(&mut app, "ui.dialog.confirm", json!({}))["ok"], true);
    let story = app.session.active().unwrap().doc.story(designcraft_doc::StoryId(sid.as_u64().unwrap())).unwrap().text.clone();
    // (The empty paragraph after a table that ends its story stays after the text.)
    assert_eq!(story.trim_end(), "Name:, Address:\nCity:, State:");
}

/// The Rectangle dialog a frame-tool click opens shows the width in the horizontal ruler unit and
/// the height in the vertical one, not always picas, and reads bare numbers the same way (#1964).
#[test]
fn frame_size_dialog_follows_the_ruler_units() {
    let mut app = DesignApp::new(designcraft_engine::Session::new(), crate::Services::default());
    app.run("file.new", json!({})).unwrap();
    app.run("document.preferences", json!({"horizontalUnits": "inches", "verticalUnits": "millimeters"})).unwrap();
    // As the frame tool asks: a click at (36, 36), 72 pt by 72 pt.
    app.session.ui_requests.push(designcraft_engine::UiRequest::Dialog {
        id: "frameSize".into(),
        params: json!({"spread": 0, "shape": "rectangle", "content": "graphic", "x": 36.0, "y": 36.0, "width": 72.0, "height": 72.0}),
    });
    app.after_engine();
    let d = app.ui.dialog.as_ref().expect("the Rectangle dialog");
    assert_eq!((d.id.as_str(), d.unit), ("frameSize", Unit::Inches));
    assert_eq!(d.s("width"), format_measure(72.0, Unit::Inches));
    assert_eq!(d.s("height"), format_measure(72.0, Unit::Millimeters));
    assert!(!d.s("width").contains('p') && !d.s("height").contains('p'), "no picas: {} x {}", d.s("width"), d.s("height"));
    // Bare numbers typed in each field are in that field's unit: 2 in by 50 mm.
    let d = app.ui.dialog.as_mut().unwrap();
    d.fields.insert("width".into(), json!("2"));
    d.fields.insert("height".into(), json!("50"));
    let before = app.session.doc().unwrap().doc.clone();
    confirm(&mut app).unwrap();
    let after = &app.session.doc().unwrap().doc;
    let added: Vec<_> = after.spreads[0].items.iter().filter(|i| !before.spreads[0].items.iter().any(|b| b.id == i.id)).collect();
    assert_eq!(added.len(), 1, "one frame");
    let b = added[0].bounds();
    assert!((b.width() - 144.0).abs() < 0.01 && (b.height() - 50.0 / 25.4 * 72.0).abs() < 0.01, "{b:?}");
    // The properties panel's link opens it in the same units.
    let d = frame_size_dialog(&app, json!({"width": 36.0, "height": 36.0}));
    assert_eq!((d.s("width"), d.s("height")), (format_measure(36.0, Unit::Inches), format_measure(36.0, Unit::Millimeters)));
    // Every ruler unit: the shown size reads back as the size, so OK leaves a 72 pt frame 72 pt.
    let units = serde_json::to_value(Unit::ALL).unwrap();
    for (unit, name) in Unit::ALL.iter().zip(units.as_array().unwrap()) {
        app.run("document.preferences", json!({"horizontalUnits": name, "verticalUnits": name})).unwrap();
        let d = frame_size_dialog(&app, json!({"width": 72.0, "height": 145.0}));
        assert_eq!(d.unit, *unit);
        for (k, pt) in [("width", 72.0), ("height", 145.0)] {
            let back = d.m_in(k, *unit).unwrap_or(f64::NAN);
            assert!((back - pt).abs() < 0.05, "{name}: {k} shows {:?}, reads back {back}", d.s(k));
        }
    }
}

/// #2055 review: the Rectangle dialog reads the height in the vertical unit it showed when it
/// opened. Rulers switched while it is open (the control channel) leave a typed 50 at 50 mm.
#[test]
fn frame_size_dialog_keeps_its_units_while_open() {
    let mut app = DesignApp::new(designcraft_engine::Session::new(), crate::Services::default());
    app.run("file.new", json!({})).unwrap();
    app.run("document.preferences", json!({"horizontalUnits": "millimeters", "verticalUnits": "millimeters"})).unwrap();
    app.session.ui_requests.push(designcraft_engine::UiRequest::Dialog {
        id: "frameSize".into(),
        params: json!({"spread": 0, "shape": "rectangle", "content": "graphic", "x": 36.0, "y": 36.0, "width": 72.0, "height": 72.0}),
    });
    app.after_engine();
    let d = app.ui.dialog.as_mut().expect("the Rectangle dialog");
    d.fields.insert("width".into(), json!("50"));
    d.fields.insert("height".into(), json!("50"));
    app.run("document.preferences", json!({"horizontalUnits": "inches", "verticalUnits": "inches"})).unwrap();
    let before = app.session.doc().unwrap().doc.clone();
    confirm(&mut app).unwrap();
    let after = &app.session.doc().unwrap().doc;
    let added: Vec<_> = after.spreads[0].items.iter().filter(|i| !before.spreads[0].items.iter().any(|b| b.id == i.id)).collect();
    assert_eq!(added.len(), 1, "one frame");
    let b = added[0].bounds();
    let mm50 = 50.0 / 25.4 * 72.0;
    assert!((b.width() - mm50).abs() < 0.01 && (b.height() - mm50).abs() < 0.01, "50 mm by 50 mm (about 141.73 pt): {b:?}");
    // A dialog made without the vertical unit (`Dialog::new`) reads the height in its own unit.
    let mut d = Dialog::new("frameSize", json!({"width": "2", "height": "3"}));
    d.unit = Unit::Inches;
    assert_eq!(d.vertical_unit, None);
    app.ui.dialog = Some(d);
    let before = app.session.doc().unwrap().doc.clone();
    confirm(&mut app).unwrap();
    let after = &app.session.doc().unwrap().doc;
    let added: Vec<_> = after.spreads[0].items.iter().filter(|i| !before.spreads[0].items.iter().any(|b| b.id == i.id)).collect();
    assert_eq!(added.len(), 1, "one frame");
    let b = added[0].bounds();
    assert!((b.width() - 144.0).abs() < 0.01 && (b.height() - 216.0).abs() < 0.01, "{b:?}");
}

#[test]
fn parent_resize_exact_dialog_keeps_its_target_and_rejects_invalid_input() {
    use designcraft_tools::{PointerEvent, PointerKind};
    let mut app = DesignApp::new(designcraft_engine::Session::new(), crate::Services::default());
    app.run("file.new", json!({"width":400,"height":600,"pages":3,"facingPages":true})).unwrap();
    app.run("layout.parents.new", json!({"prefix":"B"})).unwrap();
    app.run("layout.parents.edit", json!({"on":true})).unwrap();
    app.session.set_tool("page");
    let before = app.session.doc().unwrap().doc.clone();
    let sr = designcraft_doc::SpreadRef::Parent(1);
    let p = app.session.layout().spread_page_rect(&before, sr, 1).unwrap().center();
    app.session.pointer(&PointerEvent::new(PointerKind::DoubleClick, p.x, p.y), designcraft_engine::ViewInfo::at_zoom(1.0)).unwrap();
    let request = app.session.ui_requests.pop().unwrap();
    let designcraft_engine::UiRequest::Dialog { id, params } = request else { panic!("expected exact-size dialog") };
    app.ui.dialog = Some(Dialog::new(&id, params));
    let d = app.ui.dialog.as_mut().unwrap();
    d.fields.insert("width".into(), json!("530"));
    d.fields.insert("height".into(), json!("710"));
    confirm(&mut app).unwrap();
    let doc = &app.session.doc().unwrap().doc;
    assert_eq!((doc.parents[1].pages[1].width, doc.parents[1].pages[1].height), (530.0, 710.0));
    assert_eq!(doc.parents[0], before.parents[0]);
    assert_eq!(doc.spreads, before.spreads);
    let saved = doc.clone();
    app.ui.dialog = Some(Dialog::new("cmd:layout.parentPageSize", json!({"parent":saved.parents[1].id.0,"page":1,"width":"NaN"})));
    assert!(confirm(&mut app).is_err());
    assert!(app.ui.dialog.is_some());
    assert!(std::sync::Arc::ptr_eq(&saved, &app.session.doc().unwrap().doc));
}

/// A rule as the dialog shows it: `current[which]` (the resolved rule, inherited values included)
/// with the fields edited in the dialog (`{prefix}{which}.{field}`) on top.
fn shown_rule(d: &Dialog, prefix: &str, which: &str, current: &Value) -> Value {
    let mut r = match current.get(which) {
        Some(Value::Object(m)) => m.clone(),
        _ => Map::new(),
    };
    let edited = format!("{prefix}{which}.");
    for (k, v) in &d.fields {
        if let Some(field) = k.strip_prefix(&edited) {
            r.insert(field.into(), v.clone());
        }
    }
    Value::Object(r)
}
