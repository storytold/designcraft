//! Text frame auto-size: frames fit their text after edits, in the same undo step, and on import.

use designcraft_doc::{AutoSize, ItemId, StoryId};
use designcraft_geom::{Point, Rect};
use serde_json::{Value, json};

use crate::Session;

const LONG: &str = "Auto-sized frames grow as the text gets longer and shrink again when it is deleted, \
                    so the frame always fits its text exactly, line after line after line.";

fn session() -> Session {
    let mut s = Session::new();
    s.execute("file.new", &json!({})).unwrap();
    s
}

/// A text frame with the caret in it and the given auto-size options: (frame, story).
fn frame(s: &mut Session, rect: [f64; 4], opts: Value) -> (ItemId, StoryId) {
    let r = s.execute("frame.create", &json!({"rect": rect, "content": "text"})).unwrap();
    let (id, sid) = (r["id"].as_u64().unwrap(), r["story"].as_u64().unwrap());
    let mut p = opts;
    p["ids"] = json!([id]);
    s.execute("object.textFrameOptions", &p).unwrap();
    s.execute("text.select", &json!({"story": sid, "anchor": 0, "focus": 0})).unwrap();
    (ItemId(id), StoryId(sid))
}

fn bounds(s: &Session, id: ItemId) -> Rect {
    s.doc().unwrap().doc.item(id).unwrap().bounds()
}

fn overset(s: &Session, sid: StoryId) -> bool {
    designcraft_compose::compose_story(&s.doc().unwrap().doc, sid, &Default::default()).is_overset()
}

fn clear(s: &mut Session, sid: StoryId) {
    let len = s.doc().unwrap().doc.story(sid).unwrap().text.len();
    s.execute("text.select", &json!({"story": sid.0, "anchor": 0, "focus": len})).unwrap();
    s.execute("text.delete", &json!({})).unwrap();
}

#[test]
fn height_only_grows_and_shrinks_from_the_top() {
    let mut s = session();
    let (id, sid) = frame(&mut s, [36.0, 36.0, 186.0, 56.0], json!({"autoSize": "heightOnly", "autoSizeReference": "topCenter"}));
    let empty = bounds(&s, id);
    assert!(empty.height() > 5.0 && empty.height() < 20.0, "an empty frame keeps one line: {empty:?}");
    s.execute("text.insert", &json!({"text": LONG})).unwrap();
    let full = bounds(&s, id);
    assert!(full.height() > 40.0, "{full:?}");
    assert!((full.y0 - 36.0).abs() < 1e-6 && (full.x0 - 36.0).abs() < 1e-6 && (full.width() - 150.0).abs() < 1e-6);
    assert!(!overset(&s, sid));
    // Exact fit: a hair shorter and the last line no longer fits.
    let cs = designcraft_compose::compose_story(&s.doc().unwrap().doc, sid, &Default::default());
    let ft = cs.frame(id).unwrap();
    assert!((ft.content_height - full.height()).abs() < 0.05, "{} vs {}", ft.content_height, full.height());
    clear(&mut s, sid);
    let after = bounds(&s, id);
    assert!((after.height() - empty.height()).abs() < 1e-6 && (after.y0 - 36.0).abs() < 1e-6, "{after:?}");
}

#[test]
fn bottom_reference_keeps_the_bottom_edge() {
    let mut s = session();
    let (id, sid) = frame(&mut s, [36.0, 300.0, 186.0, 320.0], json!({"autoSize": "heightOnly", "autoSizeReference": "bottomLeft"}));
    let y1 = bounds(&s, id).y1;
    assert!((y1 - 320.0).abs() < 1e-6);
    s.execute("text.insert", &json!({"text": LONG})).unwrap();
    let b = bounds(&s, id);
    assert!((b.y1 - 320.0).abs() < 1e-6 && b.y0 < 290.0, "{b:?}");
    assert!(!overset(&s, sid));
}

#[test]
fn width_only_with_no_line_breaks_widens_to_the_longest_line() {
    let mut s = session();
    let (id, sid) = frame(&mut s, [36.0, 36.0, 56.0, 120.0], json!({"autoSize": "widthOnly", "autoSizeReference": "topLeft", "noLineBreaks": true}));
    s.execute("text.insert", &json!({"text": "Short\nA much longer second line"})).unwrap();
    let b = bounds(&s, id);
    assert!(b.width() > 100.0 && (b.x0 - 36.0).abs() < 1e-6 && (b.height() - 84.0).abs() < 1e-6, "{b:?}");
    let cs = designcraft_compose::compose_story(&s.doc().unwrap().doc, sid, &Default::default());
    assert_eq!(cs.line_count(), 2, "each paragraph stays on one line");
    assert!(!cs.is_overset());
}

#[test]
fn minimum_height_is_respected() {
    let mut s = session();
    let (id, sid) = frame(&mut s, [36.0, 36.0, 186.0, 56.0], json!({"autoSize": "heightOnly", "useMinimumHeight": true, "minimumHeight": 72}));
    assert!((bounds(&s, id).height() - 72.0).abs() < 1e-6);
    s.execute("text.insert", &json!({"text": format!("{LONG} {LONG} {LONG}")})).unwrap();
    assert!(bounds(&s, id).height() > 72.0);
    clear(&mut s, sid);
    assert!((bounds(&s, id).height() - 72.0).abs() < 1e-6);
}

#[test]
fn one_undo_restores_text_and_size() {
    let mut s = session();
    let (id, sid) = frame(&mut s, [36.0, 36.0, 186.0, 56.0], json!({"autoSize": "heightOnly"}));
    let before = bounds(&s, id);
    s.execute("text.insert", &json!({"text": LONG})).unwrap();
    assert!(bounds(&s, id).height() > before.height());
    s.execute("edit.undo", &json!({})).unwrap();
    assert_eq!(s.doc().unwrap().doc.story(sid).unwrap().text, "");
    assert_eq!(bounds(&s, id), before);
}

#[test]
fn rotated_frame_keeps_its_reference_point_in_place() {
    let mut s = session();
    let (id, _) = frame(&mut s, [100.0, 100.0, 250.0, 120.0], json!({"autoSize": "heightOnly", "autoSizeReference": "topLeft"}));
    s.execute("transform.rotate", &json!({"angle": 30, "ids": [id.0]})).unwrap();
    let corner = |s: &Session| {
        let it = s.doc().unwrap().doc.item(id).unwrap().clone();
        let r = it.inner_bounds();
        (it.xf * Point::new(r.x0, r.y0), r)
    };
    let (p0, r0) = corner(&s);
    s.execute("text.insert", &json!({"text": LONG})).unwrap();
    let (p1, r1) = corner(&s);
    assert!(r1.height() > r0.height() + 20.0);
    assert!((p1 - p0).hypot() < 1e-6, "{p0:?} → {p1:?}");
}

#[test]
fn a_threaded_frame_before_the_last_does_not_auto_size() {
    let mut s = session();
    let (a, sid) = frame(&mut s, [36.0, 36.0, 186.0, 56.0], json!({"autoSize": "heightOnly"}));
    let b = s.execute("frame.create", &json!({"rect": [36.0, 200.0, 186.0, 260.0], "content": "unassigned"})).unwrap()["id"].as_u64().unwrap();
    s.edit(|d, _| Ok(d.thread(a, ItemId(b))?)).unwrap();
    // Setting it up resized `a` while it was alone; from now on it keeps its size.
    let h = bounds(&s, a).height();
    s.execute("text.select", &json!({"story": sid.0, "anchor": 0, "focus": 0})).unwrap();
    s.execute("text.insert", &json!({"text": format!("{LONG} {LONG}")})).unwrap();
    assert!((bounds(&s, a).height() - h).abs() < 1e-6);
}

#[test]
fn opening_leaves_auto_size_frames_as_they_are() {
    // A 36 pt label in a frame 0.8 × its size tall: overset until an edit fits it.
    let mut s = session();
    let r = s.execute("frame.create", &json!({"rect": [36.0, 36.0, 300.0, 64.8], "content": "text"})).unwrap();
    let (id, sid) = (ItemId(r["id"].as_u64().unwrap()), StoryId(r["story"].as_u64().unwrap()));
    let orig = bounds(&s, id);
    s.execute("text.insert", &json!({"text": "Brand Bible"})).unwrap();
    s.execute("text.select", &json!({"story": sid.0, "anchor": 0, "focus": 11})).unwrap();
    s.execute("type.char", &json!({"attrs": {"size": 36}})).unwrap();
    // Set the option without running a command, as a file written elsewhere would have it.
    s.edit(|d, _| {
        if let Some(tf) = d.item_mut(id).and_then(designcraft_doc::Item::text_frame_mut) {
            tf.options.auto_size = AutoSize::HeightOnly;
            tf.options.auto_size_ref = 1;
        }
        Ok(())
    })
    .unwrap();
    assert!(overset(&s, sid));
    let idml = s.execute("file.exportIdml", &json!({})).unwrap()["base64"].clone();
    s.execute("file.openIdml", &json!({"base64": idml, "name": "labels.idml"})).unwrap();
    let d = &s.doc().unwrap().doc;
    let (fid, story) = d.stories.values().find(|st| st.text == "Brand Bible").map(|st| (st.frames[0], st.id)).unwrap();
    let b = bounds(&s, fid);
    assert!((b.height() - orig.height()).abs() < 1e-3 && (b.y0 - orig.y0).abs() < 1e-3, "{b:?}");
    assert!(overset(&s, story));
    // The first edit to the story fits the frame.
    s.execute("text.select", &json!({"story": story.0, "anchor": 11, "focus": 11})).unwrap();
    s.execute("text.insert", &json!({"text": "!"})).unwrap();
    assert!(!overset(&s, story));
    let b = bounds(&s, fid);
    assert!(b.height() > 36.0 && (b.y0 - orig.y0).abs() < 1e-3, "{b:?}");
}

#[test]
fn idml_keeps_the_auto_size_options() {
    let mut s = session();
    let opts = json!({"autoSize": "heightAndWidth", "autoSizeReference": "bottomRight", "useMinimumHeight": true, "minimumHeight": 30,
        "useMinimumWidth": true, "minimumWidth": 40, "noLineBreaks": true});
    let (id, _) = frame(&mut s, [36.0, 36.0, 186.0, 56.0], opts);
    let d = &s.doc().unwrap().doc;
    let back = designcraft_idml::import_idml(&designcraft_idml::export_idml(d)).unwrap();
    let options = |d: &designcraft_doc::Document, id: ItemId| d.item(id).and_then(|i| i.text_frame()).map(|t| t.options.clone());
    let a = options(d, id).unwrap();
    let b = back.stories.values().next().and_then(|st| options(&back, st.frames[0])).unwrap();
    assert_eq!(a.auto_size_ref, 8);
    assert_eq!(
        (
            b.auto_size,
            b.auto_size_ref,
            b.auto_size_use_min_height,
            b.auto_size_min_height,
            b.auto_size_use_min_width,
            b.auto_size_min_width,
            b.auto_size_no_line_breaks
        ),
        (a.auto_size, a.auto_size_ref, true, 30.0, true, 40.0, true)
    );
}

#[test]
fn fit_frame_to_content_leaves_an_auto_size_frame_as_auto_size_set_it() {
    let mut s = session();
    let (id, _) = frame(&mut s, [36.0, 36.0, 186.0, 56.0], json!({"autoSize": "heightOnly"}));
    s.execute("text.insert", &json!({"text": LONG})).unwrap();
    let fitted = bounds(&s, id);
    s.execute("selection.set", &json!({"ids": [id.0]})).unwrap();
    s.execute("object.fit", &json!({"ids": [id.0], "mode": "fitFrameToContent"})).unwrap();
    assert_eq!(bounds(&s, id), fitted);
}
