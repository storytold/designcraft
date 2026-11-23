//! Original synthetic artwork; no external fixtures or fonts are required.
//! These tests read exported PDFs back to pixels, rather than trusting the export warning.
//! PDF encode/readback can exceed the usual integration-test budget; pages stay at 240×240.

use std::sync::Arc;

use designcraft_engine::{Session, cmd::base64_decode};
use serde_json::json;

fn artwork(parent: bool) -> Session {
    let mut s = Session::new();
    s.execute("file.new", &json!({"pages": 1, "facingPages": false, "width": 240, "height": 240})).unwrap();
    rectangle(&mut s, false, [30, 100, 150, 200]);
    if parent {
        let parent = s.doc().unwrap().doc.parents[0].id.0;
        s.execute("layout.pages.applyParent", &json!({"pages": [0], "parent": parent})).unwrap();
        rectangle(&mut s, true, [30, 30, 90, 70]);
        s.execute("object.opacity", &json!({"opacity": 0.5})).unwrap();
    }
    s
}

fn rectangle(s: &mut Session, parent: bool, rect: [u32; 4]) {
    let spread = if parent { json!({"kind": "parent", "index": 0}) } else { json!({"kind": "doc", "index": 0}) };
    s.execute("frame.create", &json!({"spread": spread, "rect": rect, "content": "unassigned"})).unwrap();
    s.execute("object.fill", &json!({"swatch": "[Black]"})).unwrap();
}

fn layer(s: &mut Session, name: &str) -> u64 {
    s.execute("layer.new", &json!({"name": name})).unwrap()["id"].as_u64().unwrap()
}

// Read premultiplied PDF pixels on white, as for the operated PDF-readback evidence.
fn exported(s: &mut Session, flatten: bool) -> Vec<[u8; 3]> {
    let before = s.doc().unwrap().clone();
    let document_before = serde_json::to_value(&before.doc).unwrap();
    let selection_before = serde_json::to_value(&before.selection).unwrap();
    let out = s.execute("file.exportPdf", &json!({"flatten": if flatten { json!("low") } else { json!(false) }})).unwrap();
    let after = s.doc().unwrap();
    assert!(Arc::ptr_eq(&before.doc, &after.doc), "export must keep the source document allocation");
    assert_eq!(document_before, serde_json::to_value(&after.doc).unwrap(), "export must preserve all source artwork and layer settings");
    assert_eq!(selection_before, serde_json::to_value(&after.selection).unwrap());
    assert_eq!(before.active_layer, after.active_layer);
    assert_eq!(before.history.undo.len(), after.history.undo.len());
    assert_eq!(before.history.redo.len(), after.history.redo.len());
    assert_eq!(before.is_dirty(), after.is_dirty());
    let bytes = base64_decode(out["base64"].as_str().unwrap());
    let image = designcraft_render::render_pdf_page(&bytes, 0, 240).expect("read back the actual exported PDF");
    image
        .data()
        .iter()
        .map(|p| {
            let white = 255 - p.a;
            [p.r.saturating_add(white), p.g.saturating_add(white), p.b.saturating_add(white)]
        })
        .collect()
}

// Separate exports are compared by appearance, not byte-for-byte: allow a few pixels of
// rasteriser or encoder drift, far fewer than any of the rectangles drawn here covers.
fn assert_same_print(a: &[[u8; 3]], b: &[[u8; 3]], what: &str) {
    assert_eq!(a.len(), b.len(), "{what}");
    let differing = a.iter().zip(b).filter(|(p, q)| p.iter().zip(q.iter()).any(|(x, y)| x.abs_diff(*y) > 32)).count();
    assert!(differing <= a.len() / 200, "{what}: {differing} pixels differ");
}

fn dark(image: &[[u8; 3]], rect: [usize; 4]) -> usize {
    let [x0, y0, x1, y1] = rect;
    (y0..y1).flat_map(|y| (x0..x1).map(move |x| image[y * 240 + x])).filter(|p| p.iter().any(|c| *c < 220)).count()
}

#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
#[cfg_attr(not(target_arch = "wasm32"), test)]
fn transparent_parent_artwork_survives_empty_nonprinting_and_hidden_top_layers() {
    let mut s = artwork(true);
    let baseline = exported(&mut s, true);
    assert!(dark(&baseline, [40, 110, 140, 190]) > 0, "printable page artwork must be visible");
    assert!(dark(&baseline, [40, 40, 80, 60]) > 0, "the applied transparent parent must print too");
    let notes = layer(&mut s, "Notes");
    s.execute("layer.set", &json!({"id": notes, "printable": false})).unwrap();
    assert_same_print(&exported(&mut s, true), &baseline, "an empty nonprinting top layer must not blank the PDF");
    let hidden = layer(&mut s, "Hidden printable layer");
    s.execute("layer.set", &json!({"id": hidden, "visible": false})).unwrap();
    assert_same_print(&exported(&mut s, true), &baseline, "the raster destination must also be visible");
}

#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
#[cfg_attr(not(target_arch = "wasm32"), test)]
fn whole_page_flattening_does_not_bake_nonprinting_page_or_parent_notes_into_pdf() {
    let mut s = artwork(true);
    let baseline = exported(&mut s, true);
    let notes = layer(&mut s, "Notes");
    rectangle(&mut s, false, [170, 170, 220, 220]);
    rectangle(&mut s, true, [170, 30, 220, 70]);
    s.execute("layer.set", &json!({"id": notes, "printable": false})).unwrap();
    // Keep the original destination printable, so the separate blank-output defect cannot
    // hide a leak from the raster-input filter.
    layer(&mut s, "Printable output");
    let excluded = exported(&mut s, true);
    assert_same_print(&excluded, &baseline, "nonprinting Notes must not be baked into the exported image");
    assert_eq!(dark(&excluded, [180, 180, 210, 210]) + dark(&excluded, [180, 40, 210, 60]), 0, "no page or parent Notes ink");
    s.execute("layer.set", &json!({"id": notes, "printable": true})).unwrap();
    let printed = exported(&mut s, true);
    assert!(dark(&printed, [180, 180, 210, 210]) > dark(&excluded, [180, 180, 210, 210]), "the page Notes control must be detectable when printable");
    assert!(dark(&printed, [180, 40, 210, 60]) > dark(&excluded, [180, 40, 210, 60]), "the parent Notes control must be detectable when printable");
}

#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
#[cfg_attr(not(target_arch = "wasm32"), test)]
fn a_document_without_an_exportable_layer_stays_blank_with_or_without_flattening() {
    let mut s = artwork(true);
    let original = s.doc().unwrap().doc.layers[0].id.0;
    s.execute("layer.set", &json!({"id": original, "printable": false})).unwrap();
    let hidden = layer(&mut s, "Hidden printable layer");
    s.execute("layer.set", &json!({"id": hidden, "visible": false})).unwrap();
    let ordinary = exported(&mut s, false);
    assert_eq!(dark(&ordinary, [0, 0, 240, 240]), 0, "the ordinary print pipeline excludes both layers");
    assert_eq!(dark(&exported(&mut s, true), [0, 0, 240, 240]), 0, "flattening must not make any excluded content printable");
}

#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
#[cfg_attr(not(target_arch = "wasm32"), test)]
fn ordinary_printable_artwork_keeps_its_pdf_appearance_when_flattening_is_requested() {
    let mut s = artwork(false);
    let ordinary = exported(&mut s, false);
    assert!(dark(&ordinary, [40, 110, 140, 190]) > 0);
    assert_same_print(&exported(&mut s, true), &ordinary, "opaque artwork needs no flattening and must keep printing");
}
