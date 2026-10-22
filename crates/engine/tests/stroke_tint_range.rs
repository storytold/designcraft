//! `object.stroke` keeps `tint` and `gapTint` within 0..1, so a saved document always reloads.
use designcraft_doc::ItemId;
use designcraft_engine::Session;
use serde_json::json;

fn stroked(tint: f64, gap_tint: f64) -> (Session, ItemId) {
    let mut s = Session::new();
    s.execute("file.new", &json!({})).unwrap();
    let created = s.execute("frame.create", &json!({"rect": [36, 36, 200, 200]})).unwrap();
    let id = ItemId(created["id"].as_u64().unwrap());
    s.execute("object.stroke", &json!({"ids": [id.0], "swatch": "[Black]", "tint": tint, "gapTint": gap_tint})).unwrap();
    (s, id)
}

#[test]
fn out_of_range_stroke_tints_are_clamped_and_the_saved_file_reloads() {
    for (tint, gap, want_tint, want_gap) in [(1e300, 1e300, 1.0, 1.0), (2.0, -1.0, 1.0, 0.0), (-1e300, 0.5, 0.0, 0.5)] {
        let (s, id) = stroked(tint, gap);
        let doc = &s.doc().unwrap().doc;
        let st = &doc.item(id).unwrap().stroke;
        assert_eq!((st.tint, st.gap_tint), (want_tint, want_gap), "tint {tint}, gapTint {gap}");
        let bytes = designcraft_format::save(doc).unwrap();
        designcraft_format::load(&bytes).expect("reload what we just saved");
    }
}
