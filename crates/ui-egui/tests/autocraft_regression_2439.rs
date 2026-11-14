use designcraft_engine::Session;
use designcraft_ui_egui::dialogs::{confirm, open_document_setup};
use designcraft_ui_egui::{DesignApp, Services};
use serde_json::json;

fn sizes(app: &DesignApp) -> (Vec<(f64, f64)>, Vec<(f64, f64)>) {
    let doc = &app.session.active().unwrap().doc;
    let pages = doc.spreads.iter().flat_map(|s| &s.pages).map(|p| (p.width, p.height)).collect();
    let parents = doc.parents.iter().flat_map(|sp| sp.pages.iter()).map(|p| (p.width, p.height)).collect();
    (pages, parents)
}

#[test]
fn document_setup_bleed_keeps_resized_pages_and_parents() {
    let mut app = DesignApp::new(Session::new(), Services::default());
    app.run("file.new", json!({"pages": 3})).unwrap();
    app.run("layout.pageSize", json!({"pages": [2], "width": 300, "height": 400})).unwrap();
    let parent = app.session.active().unwrap().doc.parents[0].id.0;
    app.run("layout.parentPageSize", json!({"parent": parent, "page": 0, "width": 200, "height": 250})).unwrap();
    let before = sizes(&app);
    assert_eq!(before.0[1], (300.0, 400.0));
    assert_eq!(before.1[0], (200.0, 250.0));

    open_document_setup(&mut app);
    let d = app.ui.dialog.as_mut().unwrap();
    for e in ["Top", "Bottom", "Inside", "Outside"] {
        d.fields.insert(format!("bleed{e}"), json!("9 pt"));
    }
    confirm(&mut app).unwrap();

    assert_eq!(app.session.active().unwrap().doc.settings.bleed, [9.0; 4]);
    assert_eq!(sizes(&app), before, "bleed alone must not resize pages or parents");
}
