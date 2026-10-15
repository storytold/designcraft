//! Tables can follow their text frame's width ("Fit to frame width", #1025): the columns scale
//! proportionally to the measure, wider or narrower, and the option is saved with the document.
use designcraft_compose::{ComposeOptions, compose_story, find_cell};
use designcraft_doc::StoryId;
use designcraft_engine::Session;
use serde_json::{Value, json};

/// A text frame from x 72 to `x1` (no insets, one column) holding a 1×2 table with natural
/// column widths 60 and 140 (200 pt in all).
fn table_in_frame(x1: f64) -> (Session, u64, u64) {
    let mut s = Session::new();
    s.execute("file.new", &json!({})).unwrap();
    let r = s.execute("frame.create", &json!({"rect": [72, 72, x1, 400], "content": "text", "text": ""})).unwrap();
    let frame = r["id"].as_u64().unwrap();
    let sid = r["story"].as_u64().unwrap();
    s.execute("text.select", &json!({"story": sid, "anchor": 0, "focus": 0})).unwrap();
    s.execute("table.insert", &json!({"rows": 1, "cols": 2})).unwrap();
    s.execute("table.select", &json!({"story": sid, "rows": [0, 0], "cols": [0, 0]})).unwrap();
    s.execute("table.setColumnWidth", &json!({"width": 60})).unwrap();
    s.execute("table.select", &json!({"story": sid, "rows": [0, 0], "cols": [1, 1]})).unwrap();
    s.execute("table.setColumnWidth", &json!({"width": 140})).unwrap();
    (s, frame, sid)
}

fn table_id(s: &mut Session, sid: u64) -> u64 {
    s.execute("table.get", &json!({"story": sid})).unwrap()["table"].as_u64().unwrap()
}

/// The composed width of the table and of its two columns.
fn composed(s: &mut Session, sid: u64) -> (f64, f64, f64) {
    let tid = table_id(s, sid);
    let d = &s.doc().unwrap().doc;
    let cs = compose_story(d, StoryId(sid), &ComposeOptions::default());
    let (_, frag, a) = find_cell(&cs, tid, 0, 0).unwrap();
    let (_, _, b) = find_cell(&cs, tid, 0, 1).unwrap();
    (frag.rect.width(), a.rect.width(), b.rect.width())
}

fn close(a: f64, b: f64) -> bool {
    (a - b).abs() < 1e-6
}

#[test]
fn fit_to_frame_width_scales_columns_with_the_frame_both_ways() {
    let (mut s, frame, sid) = table_in_frame(472.0); // a 400 pt measure
    // Off (the default): the narrower table keeps its natural width.
    let (w, a, b) = composed(&mut s, sid);
    assert!(close(w, 200.0) && close(a, 60.0) && close(b, 140.0), "{w} {a} {b}");
    assert_eq!(s.execute("table.get", &json!({"story": sid})).unwrap()["options"].get("fitToFrame"), None, "off is not reported");

    let r = s.execute("table.options", &json!({"story": sid, "fitToFrame": true})).unwrap();
    assert_eq!(r["options"]["fitToFrame"], json!(true));
    let (w, a, b) = composed(&mut s, sid);
    assert!(close(w, 400.0) && close(a, 120.0) && close(b, 280.0), "fills the measure in proportion: {w} {a} {b}");

    // The table follows the frame: wider …
    s.execute("transform.resize", &json!({"ids": [frame], "from": [72, 72, 472, 400], "to": [72, 72, 572, 400]})).unwrap();
    let (w, a, b) = composed(&mut s, sid);
    assert!(close(w, 500.0) && close(a, 150.0) && close(b, 350.0), "{w} {a} {b}");
    // … and narrower than its natural width.
    s.execute("transform.resize", &json!({"ids": [frame], "from": [72, 72, 572, 400], "to": [72, 72, 172, 400]})).unwrap();
    let (w, a, b) = composed(&mut s, sid);
    assert!(close(w, 100.0) && close(a, 30.0) && close(b, 70.0), "{w} {a} {b}");
    // The stored widths are untouched: they are the proportions.
    let t = s.execute("table.get", &json!({"story": sid})).unwrap();
    let widths: Vec<f64> = t["columns"].as_array().unwrap().iter().map(|c| c["width"].as_f64().unwrap()).collect();
    assert_eq!(widths, [60.0, 140.0]);
}

#[test]
fn fit_to_frame_width_is_saved_undone_and_validated() {
    let (mut s, _, sid) = table_in_frame(472.0);
    s.execute("table.options", &json!({"story": sid, "fitToFrame": true})).unwrap();
    let tid = table_id(&mut s, sid);

    let bytes = s.execute("file.serialize", &json!({})).unwrap();
    let mut reopened = Session::new();
    reopened.execute("file.openBytes", &json!({"name": "fit.designcraft", "base64": bytes["base64"]})).unwrap();
    assert_eq!(reopened.execute("table.get", &json!({"story": sid, "table": tid})).unwrap()["options"]["fitToFrame"], json!(true));
    let d = &reopened.doc().unwrap().doc;
    let cs = compose_story(d, StoryId(sid), &ComposeOptions::default());
    assert!(close(find_cell(&cs, tid, 0, 0).unwrap().1.rect.width(), 400.0));

    s.execute("edit.undo", &json!({})).unwrap();
    assert_eq!(s.execute("table.get", &json!({"story": sid})).unwrap()["options"].get("fitToFrame"), None, "off is not reported");
    assert!(close(composed(&mut s, sid).0, 200.0));

    // Not a boolean: an error that changes nothing, not even the other options of the call.
    for bad in [json!("yes"), json!(1), Value::Null] {
        assert!(s.execute("table.options", &json!({"story": sid, "repeatHeader": false, "fitToFrame": bad})).is_err(), "{bad}");
    }
    assert_eq!(s.execute("table.get", &json!({"story": sid})).unwrap()["options"]["repeatHeader"], json!(true));
    assert_eq!(s.execute("table.get", &json!({"story": sid})).unwrap()["options"].get("fitToFrame"), None, "off is not reported");
}

#[test]
fn documents_without_the_option_load_with_it_off() {
    let o: designcraft_doc::TableOptions = serde_json::from_value(json!({"repeatHeader": true})).unwrap();
    assert!(!o.fit_to_frame);
    // Off is the default and is not written, so older readers see the same file.
    let v = serde_json::to_value(designcraft_doc::TableOptions::default()).unwrap();
    assert!(v.get("fitToFrame").is_none(), "{v}");
}

#[test]
fn applying_a_table_style_keeps_fit_to_frame_width() {
    let (mut s, _, sid) = table_in_frame(472.0);
    s.execute("table.options", &json!({"story": sid, "fitToFrame": true})).unwrap();
    s.execute("style.table.create", &json!({"name": "Data", "border": {"weight": 2, "color": "[Black]"}})).unwrap();
    s.execute("style.table.apply", &json!({"name": "Data"})).unwrap();
    let o = &s.execute("table.get", &json!({"story": sid})).unwrap()["options"];
    assert_eq!((o["fitToFrame"].clone(), o["border"]["weight"].clone()), (json!(true), json!(2.0)));
}
