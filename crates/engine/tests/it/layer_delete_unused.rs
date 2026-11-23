//! Synthetic artwork created through our public commands and document APIs; no external fixtures.

use std::sync::Arc;

use designcraft_doc::{AnchoredObject, Document, ItemId, LayerId, Story, StoryId, Table};
use designcraft_engine::Session;
use designcraft_render::Renderer;
use serde_json::json;

fn new_session() -> Session {
    let mut s = Session::new();
    s.execute("file.new", &json!({"pages": 1, "facingPages": false, "width": 240, "height": 240})).unwrap();
    s
}

fn new_layer(s: &mut Session, name: &str) -> LayerId {
    LayerId(s.execute("layer.new", &json!({"name": name})).unwrap()["id"].as_u64().unwrap())
}

fn rectangle(s: &mut Session, rect: [i32; 4]) -> u64 {
    let id = s.execute("frame.create", &json!({"rect": rect, "content": "unassigned"})).unwrap()["id"].as_u64().unwrap();
    s.execute("object.fill", &json!({"ids": [id], "swatch": "[Black]"})).unwrap();
    id
}

fn dark_coverage(s: &Session) -> usize {
    let image = Renderer::new().render_page(&s.doc().unwrap().doc, &s.cache, 0, 1.0, false, &Default::default()).unwrap();
    image.pixels.as_chunks::<4>().0.iter().filter(|p| p[..3].iter().any(|c| *c < 220)).count()
}

/// Every object on spreads, parents and in stories (group children and anchored objects included) is on a layer the
/// document has.
fn assert_every_object_on_a_live_layer(d: &Document) {
    for sp in d.spreads.iter().chain(&d.parents) {
        for it in &sp.items {
            it.walk(&mut |i| assert!(d.layer(i.layer).is_some(), "object {} is on missing layer {:?}", i.id.0, i.layer));
        }
    }
    for (_, i) in d.anchored_items() {
        assert!(d.layer(i.layer).is_some(), "anchored object {} is on missing layer {:?}", i.id.0, i.layer);
    }
    d.check().unwrap();
}

#[test]
fn a_layer_used_only_by_a_grouped_object_is_kept() {
    let mut s = new_session();
    let base = s.doc().unwrap().active_layer;
    let small = rectangle(&mut s, [10, 10, 20, 20]);
    let child_layer = new_layer(&mut s, "Grouped only");
    let large = rectangle(&mut s, [30, 30, 210, 210]);
    s.execute("selection.set", &json!({"ids": [small, large]})).unwrap();
    let group = s.execute("object.group", &json!({})).unwrap()["id"].as_u64().unwrap();
    // The group takes the layer of its backmost object, so only its child is on the other layer.
    let d = &s.doc().unwrap().doc;
    assert_eq!(d.item(ItemId(group)).unwrap().layer, base);
    assert_eq!(d.item(ItemId(large)).unwrap().layer, child_layer);
    let baseline = dark_coverage(&s);
    assert!(baseline > 0, "the fixture must show artwork");

    s.execute("layer.deleteUnused", &json!({})).unwrap();
    let d = &s.doc().unwrap().doc;
    assert!(d.layer(child_layer).is_some(), "the grouped object's layer is in use");
    assert_every_object_on_a_live_layer(d);
    // Ungrouping puts the child on the page on its own layer: it must still draw.
    s.execute("object.ungroup", &json!({"ids": [group]})).unwrap();
    assert_every_object_on_a_live_layer(&s.doc().unwrap().doc);
    assert!(dark_coverage(&s) >= baseline * 3 / 4, "the large grouped rectangle must still draw");

    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("delete-unused.designcraft");
    s.execute("file.saveAs", &json!({"path": path})).unwrap();
    let mut reopened = Session::new();
    reopened.execute("file.open", &json!({"path": path})).unwrap();
    assert_every_object_on_a_live_layer(&reopened.doc().unwrap().doc);
    assert!(dark_coverage(&reopened) >= baseline * 3 / 4, "the reopened document must still draw it");
}

#[test]
fn layers_used_only_by_anchored_objects_are_kept() {
    let mut s = new_session();
    let host = s.execute("frame.create", &json!({"rect": [10, 100, 200, 220], "content": "text", "text": "Host"})).unwrap();
    let sid = StoryId(host["story"].as_u64().unwrap());
    let inline_layer = new_layer(&mut s, "Anchored only");
    let inline = rectangle(&mut s, [10, 10, 20, 20]);
    s.execute("anchored.insert", &json!({"ids": [inline], "story": sid.0, "pos": 0})).unwrap();
    let nested_layer = new_layer(&mut s, "Anchored in an endnote of a footnote of a cell");
    let nested = rectangle(&mut s, [30, 10, 40, 20]);
    // Move the second object into the nested story containers native import makes.
    s.edit(|d, _| {
        let it = d.item(ItemId(nested)).unwrap().clone();
        Arc::make_mut(&mut d.spreads[0]).items.retain(|i| i.id.0 != nested);
        let mut table = Table::new(d.alloc(), 1, 1, 0, 0, 40.0);
        let cell = &mut table.cells[0].text;
        cell.insert_note(cell.len(), "Footnote", Default::default()).unwrap();
        let note: &mut Story = &mut Arc::make_mut(&mut cell.notes[0]).text;
        note.insert_endnote(note.len(), "Endnote", Default::default()).unwrap();
        Arc::make_mut(&mut note.endnotes[0]).text.insert_object(0, AnchoredObject::new(it, Default::default()));
        d.story_mut(sid).unwrap().insert_table(0, table);
        Ok(())
    })
    .unwrap();
    let unused = new_layer(&mut s, "Unused");
    let d = &s.doc().unwrap().doc;
    d.check().unwrap();
    assert_eq!(d.anchored_item(ItemId(inline)).unwrap().layer, inline_layer);
    assert_eq!(d.anchored_item(ItemId(nested)).unwrap().layer, nested_layer);

    s.execute("layer.deleteUnused", &json!({})).unwrap();
    let d = s.doc().unwrap().doc.clone();
    assert!(d.layer(inline_layer).is_some(), "the anchored object's layer is in use");
    assert!(d.layer(nested_layer).is_some(), "the nested anchored object's layer is in use");
    assert!(d.layer(unused).is_none(), "a layer nothing is on still goes");
    assert_every_object_on_a_live_layer(&d);
    let reopened = designcraft_format::load(&designcraft_format::save(&d).unwrap()).unwrap();
    assert_every_object_on_a_live_layer(&reopened);
}

#[test]
fn unused_layers_go_and_one_layer_always_stays() {
    let mut s = new_session();
    let base = s.doc().unwrap().active_layer;
    rectangle(&mut s, [10, 10, 20, 20]);
    let unused = new_layer(&mut s, "Unused");
    s.execute("layer.deleteUnused", &json!({})).unwrap();
    let d = &s.doc().unwrap().doc;
    assert!(d.layer(unused).is_none());
    assert!(d.layer(base).is_some());

    // With nothing anywhere, every layer is unused but the document keeps one.
    let mut s = new_session();
    new_layer(&mut s, "Empty");
    new_layer(&mut s, "Also empty");
    s.execute("layer.deleteUnused", &json!({})).unwrap();
    let d = &s.doc().unwrap().doc;
    assert_eq!(d.layers.len(), 1);
    assert!(d.layer(s.doc().unwrap().active_layer).is_some(), "drawing continues on the remaining layer");
}
