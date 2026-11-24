//! Synthetic artwork created through our public commands and document APIs; no external fixtures.

use std::sync::Arc;

use designcraft_doc::{AnchoredObject, Document, Item, ItemId, LayerId, Story, Table};
use designcraft_engine::Session;
use designcraft_render::Renderer;
use serde_json::json;

fn new_session() -> Session {
    let mut s = Session::new();
    s.execute("file.new", &json!({"pages": 1, "facingPages": false, "width": 240, "height": 240})).unwrap();
    s
}

fn rectangle(s: &mut Session, rect: [i32; 4]) -> u64 {
    let id = s.execute("frame.create", &json!({"rect": rect, "content": "unassigned"})).unwrap()["id"].as_u64().unwrap();
    s.execute("object.fill", &json!({"ids": [id], "swatch": "[Black]"})).unwrap();
    id
}

fn group(s: &mut Session, ids: &[u64]) -> u64 {
    s.execute("selection.set", &json!({"ids": ids})).unwrap();
    s.execute("object.group", &json!({})).unwrap()["id"].as_u64().unwrap()
}

fn dark_coverage(s: &Session) -> usize {
    let image = Renderer::new().render_page(&s.doc().unwrap().doc, &s.cache, 0, 1.0, false, &Default::default()).unwrap();
    image.pixels.as_chunks::<4>().0.iter().filter(|p| p[..3].iter().any(|c| *c < 220)).count()
}

fn items(d: &Document) -> Vec<&Item> {
    let mut out = Vec::new();
    for sp in d.spreads.iter().chain(&d.parents) {
        for it in &sp.items {
            it.walk(&mut |i| out.push(i));
        }
    }
    out.extend(d.anchored_items().into_iter().map(|(_, i)| i));
    out
}

fn assert_remapped(before: &Document, after: &Document, source: LayerId, destination: LayerId) {
    let old_items = items(before);
    let new_items = items(after);
    assert_eq!(old_items.len(), new_items.len(), "merging must not remove descendants");
    for old in old_items {
        let new = new_items.iter().find(|i| i.id == old.id).unwrap();
        assert_eq!(new.layer, if old.layer == source { destination } else { old.layer }, "only removed-layer references move");
        assert!(after.layer(new.layer).is_some(), "every descendant must keep a live layer");
        assert_eq!(new.path, old.path);
        assert_eq!(new.xf, old.xf);
        assert_eq!(new.fill, old.fill);
        assert_eq!(new.stroke, old.stroke);
        assert_eq!(new.bounds(), old.bounds());
    }
    after.check().unwrap();
}

#[test]
fn merging_grouped_artwork_survives_ungroup_undo_redo_and_native_reopen() {
    let mut s = new_session();
    let destination = s.doc().unwrap().active_layer;
    let source = LayerId(s.execute("layer.new", &json!({"name": "Grouped artwork"})).unwrap()["id"].as_u64().unwrap());
    let a = rectangle(&mut s, [30, 30, 90, 90]);
    let b = rectangle(&mut s, [110, 30, 170, 90]);
    let g = group(&mut s, &[a, b]);
    let before_merge = s.doc().unwrap().doc.clone();
    let baseline = dark_coverage(&s);
    assert!(baseline > 0, "fixture must contain visible artwork");

    s.execute("layer.merge", &json!({"ids": [destination.0, source.0], "into": destination.0})).unwrap();
    let after_merge = s.doc().unwrap().doc.clone();
    assert_remapped(&before_merge, &after_merge, source, destination);
    s.execute("object.ungroup", &json!({"ids": [g]})).unwrap();
    let after_ungroup = s.doc().unwrap().doc.clone();
    for id in [a, b] {
        let mut expected = before_merge.item(ItemId(id)).unwrap().clone();
        expected.layer = destination;
        assert_eq!(after_ungroup.item(ItemId(id)), Some(&expected), "promoted artwork preserves identity and geometry");
    }
    assert!(dark_coverage(&s) >= baseline / 2, "Ungroup must not hide the merged artwork");

    s.execute("edit.undo", &json!({})).unwrap();
    assert_eq!(s.doc().unwrap().doc, after_merge);
    s.execute("edit.undo", &json!({})).unwrap();
    assert_eq!(s.doc().unwrap().doc, before_merge);
    s.execute("edit.redo", &json!({})).unwrap();
    assert_eq!(s.doc().unwrap().doc, after_merge);
    s.execute("edit.redo", &json!({})).unwrap();
    assert_eq!(s.doc().unwrap().doc, after_ungroup);

    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("merged-artwork.designcraft");
    s.execute("file.saveAs", &json!({"path": path})).unwrap();
    let mut reopened = Session::new();
    reopened.execute("file.open", &json!({"path": path})).unwrap();
    let doc = &reopened.doc().unwrap().doc;
    for id in [a, b] {
        let it = doc.item(ItemId(id)).unwrap();
        assert_eq!(it, after_ungroup.item(ItemId(id)).unwrap());
        assert!(doc.layer(it.layer).is_some());
    }
    assert!(dark_coverage(&reopened) >= baseline / 2, "native reopen must preserve visible artwork");
}

#[test]
fn merging_reaches_nested_groups_on_surviving_document_and_parent_layers() {
    let mut s = new_session();
    let destination = s.doc().unwrap().active_layer;
    let survivor = LayerId(s.execute("layer.new", &json!({"name": "Keep separate"})).unwrap()["id"].as_u64().unwrap());
    let a = rectangle(&mut s, [10, 10, 30, 30]);
    let source = LayerId(s.execute("layer.new", &json!({"name": "Merge away"})).unwrap()["id"].as_u64().unwrap());
    let b = rectangle(&mut s, [40, 10, 60, 30]);
    let inner = group(&mut s, &[a, b]);
    s.execute("layer.activate", &json!({"id": destination.0})).unwrap();
    let c = rectangle(&mut s, [70, 10, 90, 30]);
    let outer = group(&mut s, &[inner, c]);
    // The root is on a surviving layer, but a grandchild references the layer being deleted.
    assert_eq!(s.doc().unwrap().doc.item(ItemId(outer)).unwrap().layer, survivor);
    s.execute("layer.activate", &json!({"id": survivor.0})).unwrap();
    let p = s.execute("frame.create", &json!({"spread": {"kind": "parent", "index": 0}, "rect": [10, 50, 30, 70]})).unwrap()["id"].as_u64().unwrap();
    s.execute("layer.activate", &json!({"id": source.0})).unwrap();
    let q = s.execute("frame.create", &json!({"spread": {"kind": "parent", "index": 0}, "rect": [40, 50, 60, 70]})).unwrap()["id"].as_u64().unwrap();
    group(&mut s, &[p, q]);
    let before = s.doc().unwrap().doc.clone();

    assert_eq!(s.doc().unwrap().active_layer, source);

    s.execute("layer.merge", &json!({"ids": [source.0], "into": destination.0})).unwrap();
    let merged = s.doc().unwrap().doc.clone();
    assert_remapped(&before, &merged, source, destination);
    // Drawing continues on the layer that received the merged objects, not on whichever layer is on top.
    assert_eq!(s.doc().unwrap().active_layer, destination);
    s.execute("edit.undo", &json!({})).unwrap();
    assert_eq!(s.doc().unwrap().doc, before);
    assert!(s.doc().unwrap().doc.layer(s.doc().unwrap().active_layer).is_some(), "Undo leaves a live active layer");
    s.execute("edit.redo", &json!({})).unwrap();
    assert_eq!(s.doc().unwrap().doc, merged);
    assert_eq!(s.doc().unwrap().active_layer, destination);
    s.execute("object.ungroup", &json!({"ids": [outer]})).unwrap();
    s.execute("object.ungroup", &json!({"ids": [inner]})).unwrap();
    let ungrouped = &s.doc().unwrap().doc;
    assert_eq!(ungrouped.item(ItemId(a)).unwrap().layer, survivor);
    assert_eq!(ungrouped.item(ItemId(b)).unwrap().layer, destination);
    assert_eq!(ungrouped.item(ItemId(c)).unwrap().layer, destination);
}

#[test]
fn merging_copies_only_containers_that_reference_a_merged_layer() {
    let mut s = Session::new();
    s.execute("file.new", &json!({"pages": 2, "facingPages": false})).unwrap();
    let destination = s.doc().unwrap().active_layer;
    rectangle(&mut s, [10, 10, 30, 30]);
    let text = s.execute("frame.create", &json!({"rect": [10, 50, 200, 300], "content": "text", "text": "Untouched"})).unwrap();
    let (frame, story) = (ItemId(text["id"].as_u64().unwrap()), designcraft_doc::StoryId(text["story"].as_u64().unwrap()));
    // An auto-size frame left at a size that doesn't fit its text, as a file can open: an edit that
    // needlessly copies its story makes Auto-Size refit it and move artwork on an untouched layer.
    s.edit(|d, _| {
        if let Some(designcraft_doc::Content::Text(tf)) = d.item_mut(frame).map(|i| &mut i.content) {
            tf.options.auto_size = designcraft_doc::AutoSize::HeightOnly;
        }
        Ok(())
    })
    .unwrap();
    assert!(designcraft_engine::autosize::fitted(&s.doc().unwrap().doc, frame, story).is_some(), "the fixture frame must be stale");
    let source = LayerId(s.execute("layer.new", &json!({"name": "Merge away"})).unwrap()["id"].as_u64().unwrap());
    s.execute("frame.create", &json!({"spread": 1, "rect": [10, 10, 30, 30]})).unwrap();
    let before = s.doc().unwrap().doc.clone();

    s.execute("layer.merge", &json!({"ids": [source.0], "into": destination.0})).unwrap();
    let after = s.doc().unwrap().doc.clone();
    assert_remapped(&before, &after, source, destination);
    // Untouched spreads, parents and stories stay shared with the undo snapshot instead of being deep-copied.
    assert!(Arc::ptr_eq(&before.spreads[0], &after.spreads[0]));
    assert!(before.parents.iter().zip(&after.parents).all(|(b, a)| Arc::ptr_eq(b, a)));
    assert!(before.stories.iter().all(|(id, st)| Arc::ptr_eq(st, &after.stories[id])));
    assert!(!Arc::ptr_eq(&before.spreads[1], &after.spreads[1]), "the spread holding the merged object changes");
    assert_eq!(after.item(frame), before.item(frame), "Auto-Size must not refit a frame the merge didn't touch");
}

#[test]
fn merging_remaps_anchored_descendants_in_cells_footnotes_and_endnotes() {
    let mut s = new_session();
    let destination = s.doc().unwrap().active_layer;
    let host = s.execute("frame.create", &json!({"rect": [10, 100, 200, 220], "content": "text", "text": "Host"})).unwrap();
    let sid = designcraft_doc::StoryId(host["story"].as_u64().unwrap());
    let survivor = LayerId(s.execute("layer.new", &json!({"name": "Keep anchored"})).unwrap()["id"].as_u64().unwrap());
    let a = rectangle(&mut s, [10, 10, 20, 20]);
    let source = LayerId(s.execute("layer.new", &json!({"name": "Merge anchored"})).unwrap()["id"].as_u64().unwrap());
    let b = rectangle(&mut s, [30, 10, 40, 20]);
    let g = group(&mut s, &[a, b]);
    s.execute("anchored.insert", &json!({"ids": [g], "story": sid.0, "pos": 0})).unwrap();
    let cell_item = rectangle(&mut s, [50, 10, 60, 20]);
    let footnote_item = rectangle(&mut s, [70, 10, 80, 20]);
    let endnote_item = rectangle(&mut s, [90, 10, 100, 20]);
    // Construct the same nested story containers used by native import and existing layer tests.
    s.edit(|d, _| {
        let take = |d: &mut Document, id| {
            let it = d.item(ItemId(id)).unwrap().clone();
            Arc::make_mut(&mut d.spreads[0]).items.retain(|i| i.id.0 != id);
            AnchoredObject::new(it, Default::default())
        };
        let cell_object = take(d, cell_item);
        let footnote_object = take(d, footnote_item);
        let endnote_object = take(d, endnote_item);
        let mut table = Table::new(d.alloc(), 1, 1, 0, 0, 40.0);
        let cell = &mut table.cells[0].text;
        cell.insert_object(0, cell_object);
        cell.insert_note(cell.len(), "Footnote", Default::default()).unwrap();
        let note: &mut Story = &mut Arc::make_mut(&mut cell.notes[0]).text;
        note.insert_object(0, footnote_object);
        note.insert_endnote(note.len(), "Endnote", Default::default()).unwrap();
        Arc::make_mut(&mut note.endnotes[0]).text.insert_object(0, endnote_object);
        d.story_mut(sid).unwrap().insert_table(0, table);
        Ok(())
    })
    .unwrap();
    let before = s.doc().unwrap().doc.clone();
    before.check().unwrap();

    s.execute("layer.merge", &json!({"ids": [source.0], "into": destination.0})).unwrap();
    let merged = s.doc().unwrap().doc.clone();
    assert_remapped(&before, &merged, source, destination);
    assert_eq!(merged.anchored_item(ItemId(a)).unwrap().layer, survivor);
    for id in [b, cell_item, footnote_item, endnote_item] {
        assert_eq!(merged.anchored_item(ItemId(id)).unwrap().layer, destination);
    }
    s.execute("edit.undo", &json!({})).unwrap();
    assert_eq!(s.doc().unwrap().doc, before);
    s.execute("edit.redo", &json!({})).unwrap();
    assert_eq!(s.doc().unwrap().doc, merged);
    let reopened = designcraft_format::load(&designcraft_format::save(&merged).unwrap()).unwrap();
    assert_remapped(&before, &reopened, source, destination);
}
