//! Primary text frames retain all columns supported by the new-document command.
use designcraft_doc::Document;
use designcraft_engine::Session;
use serde_json::json;

fn assert_primary_columns(doc: &Document, expected: u32) {
    let mut frames = 0;
    for story in doc.stories.values() {
        for spec in designcraft_compose::frame_specs(doc, story.id) {
            assert_eq!(spec.opts.columns, expected);
            let columns = spec.columns();
            assert_eq!(columns.len(), expected as usize);
            assert!(columns.iter().all(|r| r.width() > 0. && [r.x0, r.x1, r.y0, r.y1].into_iter().all(f64::is_finite)));
            frames += 1;
        }
    }
    assert!(frames > 0, "the primary text frame must exist");
}

#[test]
fn primary_columns_survive_composition_native_reopen_and_idml_round_trip() {
    for columns in [41, 216] {
        let mut s = Session::new();
        s.execute("file.new", &json!({"columns":columns,"gutter":0,"primaryTextFrame":true})).unwrap();
        assert_primary_columns(&s.doc().unwrap().doc, columns);
        let bytes = s.execute("file.serialize", &json!({})).unwrap();
        let mut reopened = Session::new();
        reopened.execute("file.openBytes", &json!({"name":"primary-columns.designcraft","base64":bytes["base64"]})).unwrap();
        assert_primary_columns(&reopened.doc().unwrap().doc, columns);
        let idml = designcraft_idml::export_idml(&reopened.doc().unwrap().doc);
        let imported = designcraft_idml::import_idml(&idml).unwrap();
        assert_primary_columns(&imported, columns);
    }
}
