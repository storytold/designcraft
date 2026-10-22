//! Effective raster resolution follows image, frame and ancestor transforms in spread space.
use std::sync::Arc;

use designcraft_doc::{Content, Intent, ItemId};
use designcraft_engine::{Session, cmd::base64_encode};
use designcraft_geom::Affine;
use serde_json::{Value, json};

fn place(s: &mut Session, name: &str, bytes: &[u8]) -> ItemId {
    s.execute("selection.set", &json!({"ids": []})).unwrap();
    let placed = s.execute("file.place", &json!({"base64": base64_encode(bytes), "name": name, "x": 0, "y": 0, "width": 144})).unwrap();
    ItemId(placed["id"].as_u64().unwrap())
}

fn raster() -> (Session, ItemId) {
    let mut s = Session::new();
    s.execute("file.new", &json!({"width": 1000, "height": 1000, "facingPages": false})).unwrap();
    // 600 × 300 pixels at 144 × 72 points: 300 ppi in both directions.
    let png = designcraft_render::Rendered { width: 600, height: 300, pixels: vec![200; 600 * 300 * 4] }.to_png();
    let id = place(&mut s, "photo.png", &png);
    assert_eq!(s.doc().unwrap().doc.item(id).unwrap().inner_bounds().size(), designcraft_geom::Size::new(144.0, 72.0));
    (s, id)
}

fn facing_raster() -> (Session, ItemId) {
    let mut s = Session::new();
    s.execute("file.new", &json!({"width": 612, "height": 792, "pages": 3, "facingPages": true})).unwrap();
    // Page 0 is the cover; spread 1 contains absolute pages 1 (left) and 2 (right).
    let d = &s.doc().unwrap().doc;
    assert_eq!(d.spreads.len(), 2);
    assert_eq!(d.spreads[0].pages.len(), 1);
    assert_eq!(d.spreads[1].pages.len(), 2);
    assert_eq!(d.first_page_of_spread(1), 1);
    assert_eq!(d.spreads[1].pages[0].x, 0.0);
    assert_eq!(d.spreads[1].pages[1].x, 612.0);
    let png = designcraft_render::Rendered { width: 600, height: 300, pixels: vec![200; 600 * 300 * 4] }.to_png();
    let placed = s
        .execute(
            "file.place",
            &json!({
                "base64": base64_encode(&png), "name": "photo.png", "spread": 1,
                "x": 0, "y": 0, "width": 144
            }),
        )
        .unwrap();
    let id = ItemId(placed["id"].as_u64().unwrap());
    let d = &s.doc().unwrap().doc;
    assert_eq!(d.find(id).unwrap().spread, designcraft_doc::SpreadRef::Doc(1));
    assert_eq!(d.item(id).unwrap().inner_bounds().size(), designcraft_geom::Size::new(144.0, 72.0));
    (s, id)
}

fn transform(s: &mut Session, id: ItemId, frame: Affine, image: Affine) {
    s.edit(|d, _| {
        let it = d.item_mut(id).unwrap();
        it.xf = frame;
        let Content::Graphic(g) = &mut it.content else { panic!("graphic fixture") };
        g.xf = image * g.xf;
        Ok(())
    })
    .unwrap();
}

fn group(s: &mut Session, child: ItemId, xf: Affine) -> ItemId {
    let spread = s.doc().unwrap().doc.find(child).unwrap().spread;
    let marker = s.execute("frame.create", &json!({"spread": spread, "rect": [0, 0, 1, 1]})).unwrap()["id"].clone();
    let group = s.execute("object.group", &json!({"ids": [child.0, marker]})).unwrap();
    let id = ItemId(group["id"].as_u64().unwrap());
    s.edit(|d, _| {
        d.item_mut(id).unwrap().xf = xf;
        Ok(())
    })
    .unwrap();
    id
}

fn resolutions(s: &mut Session, params: Value) -> Vec<Value> {
    let before = s.doc().unwrap();
    let doc = before.doc.clone();
    let selection = before.selection.clone();
    let state = (before.revision, before.history.undo.len(), before.history.redo.len(), s.journal.len());
    let result = s.execute("preflight.run", &params).unwrap();
    let after = s.doc().unwrap();
    assert!(Arc::ptr_eq(&doc, &after.doc), "preflight must leave the document and assets untouched");
    assert_eq!(selection, after.selection, "preflight must leave the selection untouched");
    assert_eq!(state, (after.revision, after.history.undo.len(), after.history.redo.len(), s.journal.len()));
    result["issues"].as_array().unwrap().iter().filter(|i| i["kind"] == "lowResolution").cloned().collect()
}

fn warning(s: &mut Session, id: ItemId, params: Value, ppi: u32, threshold: u32) {
    warning_on_page(s, id, params, ppi, threshold, 0);
}

fn warning_on_page(s: &mut Session, id: ItemId, params: Value, ppi: u32, threshold: u32, page: usize) {
    assert_eq!(
        resolutions(s, params),
        vec![json!({
            "severity": "warning", "kind": "lowResolution", "item": id.0, "page": page,
            "message": format!("photo.png: effective {ppi} ppi (< {threshold})")
        })]
    );
}

#[test]
fn either_raster_axis_can_fail_after_image_or_frame_scaling() {
    for scale in [Affine::scale_non_uniform(4.0, 1.0), Affine::scale_non_uniform(1.0, 4.0)] {
        for image_only in [false, true] {
            let (mut s, id) = raster();
            let (frame, image) = if image_only { (Affine::IDENTITY, scale) } else { (scale, Affine::IDENTITY) };
            transform(&mut s, id, frame, image);
            // One axis remains 300 ppi, the other is 75 ppi: report the worse axis once.
            warning(&mut s, id, json!({}), 75, 150);
        }
    }
}

#[test]
fn nested_groups_accumulate_scale_and_keep_image_frame_parent_order() {
    let (mut s, id) = raster();
    let inner = group(&mut s, id, Affine::scale(2.0));
    group(&mut s, inner, Affine::scale(2.0));
    warning(&mut s, id, json!({}), 75, 150);

    let (mut s, id) = raster();
    // Double image X, then turn the frame 90 degrees, then triple spread X.
    // The image axes become 288 and 216 pt, giving 150 and 100 ppi respectively.
    transform(&mut s, id, Affine::new([0.0, 1.0, -1.0, 0.0, 400.0, 20.0]), Affine::scale_non_uniform(2.0, 1.0));
    group(&mut s, id, Affine::scale_non_uniform(3.0, 1.0));
    warning(&mut s, id, json!({}), 100, 150);
}

#[test]
fn translated_scaled_nested_raster_reports_right_facing_page() {
    let (mut s, id) = facing_raster();
    let inner = group(&mut s, id, Affine::scale(2.0));
    group(&mut s, inner, Affine::translate((640.0, 0.0)) * Affine::scale(2.0));
    // Local frame center x=72 is on the left page. Ancestors move its bounds to
    // x=640..1216 on the right page, while scaling 300 ppi down to 75 ppi.
    warning_on_page(&mut s, id, json!({}), 75, 150, 2);
}

#[test]
fn translating_group_changes_warning_page_without_changing_resolution() {
    let (mut s, id) = facing_raster();
    // A nonidentity frame transform also detects applying the frame transform twice.
    transform(&mut s, id, Affine::translate((-240.0, 0.0)), Affine::IDENTITY);
    warning_on_page(&mut s, id, json!({"minPpi": 301}), 300, 301, 1);
    group(&mut s, id, Affine::translate((880.0, 0.0)));
    // Translation moves the center to x=712, but does not change either image axis.
    assert!(resolutions(&mut s, json!({})).is_empty());
    warning_on_page(&mut s, id, json!({"minPpi": 301}), 300, 301, 2);
}

#[test]
fn rotation_translation_and_reflection_do_not_change_resolution() {
    for xf in [
        Affine::translate((250.0, 300.0)) * Affine::rotate(std::f64::consts::FRAC_PI_4),
        Affine::translate((250.0, 300.0)) * Affine::scale_non_uniform(-1.0, 1.0),
    ] {
        let (mut s, id) = raster();
        transform(&mut s, id, xf, Affine::IDENTITY);
        // Measuring the rotated bounding box instead would incorrectly warn here.
        assert!(resolutions(&mut s, json!({"minPpi": 250})).is_empty());
        warning(&mut s, id, json!({"minPpi": 301}), 300, 301);
    }
}

#[test]
fn cropping_keeps_resolution_and_fitting_changes_it_with_content_scale() {
    let (mut s, id) = raster();
    s.execute("transform.resize", &json!({"ids": [id.0], "from": [0, 0, 144, 72], "to": [0, 0, 36, 18]})).unwrap();
    warning(&mut s, id, json!({"minPpi": 301}), 300, 301);
    s.execute("transform.resize", &json!({"ids": [id.0], "from": [0, 0, 36, 18], "to": [0, 0, 144, 216]})).unwrap();
    warning(&mut s, id, json!({"minPpi": 301}), 300, 301);

    s.execute("object.fit", &json!({"ids": [id.0], "mode": "fitProportionally"})).unwrap();
    warning(&mut s, id, json!({"minPpi": 301}), 300, 301);
    // Stretching just Y to 216 pt leaves X at 300 ppi but reduces Y to 100 ppi.
    s.execute("object.fit", &json!({"ids": [id.0], "mode": "fitContentToFrame"})).unwrap();
    warning(&mut s, id, json!({}), 100, 150);
    // Proportional fill is 432 × 216 pt: 100 ppi in both directions.
    s.execute("object.fit", &json!({"ids": [id.0], "mode": "fillProportionally"})).unwrap();
    warning(&mut s, id, json!({}), 100, 150);
    s.execute("object.fit", &json!({"ids": [id.0], "mode": "centerContent"})).unwrap();
    warning(&mut s, id, json!({}), 100, 150);
}

#[test]
fn threshold_is_strict_and_resolution_remains_print_only() {
    let (mut s, id) = raster();
    transform(&mut s, id, Affine::scale_non_uniform(1.0, 2.0), Affine::IDENTITY);
    assert!(resolutions(&mut s, json!({})).is_empty(), "150 ppi meets the default threshold");
    assert!(resolutions(&mut s, json!({"minPpi": 149})).is_empty());
    warning(&mut s, id, json!({"minPpi": 151}), 150, 151);
    for intent in [Intent::Web, Intent::Mobile] {
        s.edit(|d, _| {
            d.settings.intent = intent;
            Ok(())
        })
        .unwrap();
        assert!(resolutions(&mut s, json!({"minPpi": 151})).is_empty());
    }
}

#[test]
fn transformed_vector_previews_are_excluded_beside_rasters() {
    let (mut s, id) = raster();
    let svg = br#"<svg xmlns="http://www.w3.org/2000/svg" width="144" height="72"><rect width="144" height="72" fill="red"/></svg>"#;
    let vector = place(&mut s, "vector.svg", svg);
    let parent = s.execute("object.group", &json!({"ids": [id.0, vector.0]})).unwrap()["id"].as_u64().unwrap();
    s.edit(|d, _| {
        d.item_mut(ItemId(parent)).unwrap().xf = Affine::scale_non_uniform(1.0, 4.0);
        Ok(())
    })
    .unwrap();
    warning(&mut s, id, json!({}), 75, 150);
}
