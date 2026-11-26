//! `object.stroke` keeps `tint` and `gapTint` within 0..1, so a saved document always reloads.
use designcraft_doc::ItemId;
use designcraft_engine::Session;
use serde_json::json;

/// A new frame, and whether `object.stroke` took `tint` and `gapTint` for it.
fn stroked(tint: f64, gap_tint: f64) -> (Session, ItemId, bool) {
    let mut s = Session::new();
    s.execute("file.new", &json!({})).unwrap();
    let created = s.execute("frame.create", &json!({"rect": [36, 36, 200, 200]})).unwrap();
    let id = ItemId(created["id"].as_u64().unwrap());
    let taken = s.execute("object.stroke", &json!({"ids": [id.0], "swatch": "[Black]", "tint": tint, "gapTint": gap_tint})).is_ok();
    (s, id, taken)
}

#[test]
fn out_of_range_stroke_tints_are_refused_and_the_saved_file_reloads() {
    for (tint, gap) in [(1e300, 1e300), (2.0, -1.0), (-1e300, 0.5), (0.5, 1.5)] {
        let (s, id, taken) = stroked(tint, gap);
        assert!(!taken, "tint {tint}, gapTint {gap}");
        assert_ne!(s.doc().unwrap().doc.item(id).unwrap().stroke.tint, tint as f32, "tint {tint}, gapTint {gap}: the stroke is unchanged");
    }
    for (tint, gap) in [(1.0, 1.0), (0.0, 0.0), (0.25, 0.75)] {
        let (s, id, taken) = stroked(tint, gap);
        assert!(taken, "tint {tint}, gapTint {gap}");
        let doc = &s.doc().unwrap().doc;
        let st = &doc.item(id).unwrap().stroke;
        assert_eq!((st.tint, st.gap_tint), (tint as f32, gap as f32), "tint {tint}, gapTint {gap}");
        let bytes = designcraft_format::save(doc).unwrap();
        designcraft_format::load(&bytes).expect("reload what we just saved");
    }
}
