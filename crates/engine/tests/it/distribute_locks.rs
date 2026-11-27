//! Distribution preserves explicit item locks without changing reference/spacing calculations.
use std::sync::Arc;

use designcraft_doc::ItemId;
use designcraft_engine::Session;
use designcraft_geom::Affine;
use serde_json::{Value, json};

const STARTS: [f64; 4] = [10.0, 70.0, 150.0, 310.0];
const SIZES: [f64; 4] = [20.0, 30.0, 40.0, 50.0];
// An original, synthetic 1x1 RGBA PNG; used by both a locked and an unlocked frame.
const PIXEL: &str = "iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAYAAAAfFcSJAAAADUlEQVR4nGPYYqPxHwAE2AIYBVnQbwAAAABJRU5ErkJggg==";

fn fixture(horizontal: bool, locks: [bool; 4]) -> (Session, Vec<u64>) {
    let mut s = Session::new();
    s.execute("file.new", &json!({})).unwrap();
    let mut ids = Vec::new();
    for (i, (&start, &size)) in STARTS.iter().zip(&SIZES).enumerate() {
        let cross = 40.0 + i as f64 * 35.0;
        let rect = if horizontal { [start, cross, start + size, cross + 25.0] } else { [cross, start, cross + 25.0, start + size] };
        let graphic = i == 1 || i == 2;
        let created =
            s.execute("frame.create", &json!({"rect": rect, "content": if graphic {"unassigned"} else {"text"}, "text": "Keep this text"})).unwrap();
        let id = created["id"].as_u64().unwrap();
        ids.push(id);
        if graphic {
            s.execute("file.place", &json!({"frame": id, "base64": PIXEL, "name": "pixel.png"})).unwrap();
        }
    }
    // An unselected control must retain all geometry and content too.
    s.execute("frame.create", &json!({"rect": [400, 400, 450, 450], "content": "text", "text": "Outside selection"})).unwrap();
    s.execute("object.fill", &json!({"ids": ids, "swatch": "[Black]", "tint": 0.5})).unwrap();
    let locked: Vec<u64> = ids.iter().zip(locks).filter_map(|(&id, lock)| lock.then_some(id)).collect();
    if !locked.is_empty() {
        s.execute("object.lock", &json!({"ids": locked})).unwrap();
    }
    // Layers selection may include locked items. Deliberately avoid geometric order.
    s.execute("selection.set", &json!({"ids": [ids[3], ids[1], ids[0], ids[2]]})).unwrap();
    assert!(!s.doc().unwrap().doc.assets.is_empty());
    s.doc().unwrap().doc.check().unwrap();
    (s, ids)
}

fn modes() -> [(Value, [f64; 4]); 3] {
    [
        // Endpoint centers 20 and 335 give a center step of 105.
        (json!({"by": "centers"}), [10.0, 110.0, 210.0, 310.0]),
        // Outer span 350 minus total widths 140 gives three gaps of 70.
        (json!({"by": "spacing"}), [10.0, 100.0, 200.0, 310.0]),
        // An explicit spacing value selects fixed-gap mode without requiring `by`.
        (json!({"spacing": 15}), [10.0, 45.0, 90.0, 145.0]),
    ]
}

fn check_distribution(horizontal: bool, mut params: Value, positions: [f64; 4], locks: [bool; 4], explicit_ids: bool) {
    let (mut s, ids) = fixture(horizontal, locks);
    params["axis"] = json!(if horizontal { "horizontal" } else { "vertical" });
    if explicit_ids {
        params["ids"] = json!([ids[2], ids[0], ids[3], ids[1]]);
    }
    let before = s.doc().unwrap().doc.clone();
    let selection = s.doc().unwrap().selection.clone();
    let undo_len = s.doc().unwrap().history.undo.len();
    let mut expected = (*before).clone();
    for (i, &id) in ids.iter().enumerate() {
        if !locks[i] {
            let delta = positions[i] - STARTS[i];
            let translation = if horizontal { (delta, 0.0) } else { (0.0, delta) };
            let item = expected.item_mut(ItemId(id)).unwrap();
            item.xf = Affine::translate(translation) * item.xf;
        }
    }
    s.execute("object.distribute", &params).unwrap();
    let after = s.doc().unwrap().doc.clone();
    for (i, &id) in ids.iter().enumerate() {
        let item = after.item(ItemId(id)).unwrap();
        let b = item.bounds();
        let start = if horizontal { b.x0 } else { b.y0 };
        assert_eq!(start, if locks[i] { STARTS[i] } else { positions[i] }, "item {i}: {params}, locks={locks:?}");
        if locks[i] {
            assert_eq!(item, before.item(ItemId(id)).unwrap(), "locked item's transform, path, content and flags are unchanged");
        }
    }
    // Only the specified translations may change: all paths, sizes, other-axis coordinates,
    // paint, content, stories, layer membership, item order and embedded asset bytes are exact.
    assert_eq!(*after, expected, "{params}, locks={locks:?}");
    assert_eq!(s.doc().unwrap().selection, selection);
    after.check().unwrap();
    if expected != *before {
        assert_eq!(s.doc().unwrap().history.undo.len(), undo_len + 1);
        s.execute("edit.undo", &json!({})).unwrap();
        assert!(Arc::ptr_eq(&s.doc().unwrap().doc, &before));
        assert_eq!(s.doc().unwrap().selection, selection);
        s.execute("edit.redo", &json!({})).unwrap();
        assert!(Arc::ptr_eq(&s.doc().unwrap().doc, &after));
        assert_eq!(s.doc().unwrap().selection, selection);
    }
}

#[test]
fn distribute_preserves_locked_items_and_moves_unlocked_controls() {
    for horizontal in [true, false] {
        for (params, positions) in modes() {
            // Locked interior object still participates in the numeric reference calculation.
            check_distribution(horizontal, params.clone(), positions, [false, true, false, false], false);
            // Locked endpoints and an interior object; only the unlocked interior control moves.
            check_distribution(horizontal, params, positions, [true, true, false, true], true);
        }
    }
}

#[test]
fn distribute_unlocked_spacing_modes_are_unchanged() {
    for horizontal in [true, false] {
        for (params, positions) in modes() {
            check_distribution(horizontal, params, positions, [false; 4], true);
        }
    }
}

#[test]
fn distribute_all_locked_leaves_document_and_selection_unchanged() {
    for horizontal in [true, false] {
        for (params, positions) in modes() {
            check_distribution(horizontal, params, positions, [true; 4], false);
        }
    }
}
