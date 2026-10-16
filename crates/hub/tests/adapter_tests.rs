//! The adapter's ported convergence suite (CraftHub spec §3.6 phase B: "with its entire
//! convergence tests ported as a literal suite — the family's most valuable testing asset").
//!
//! Every test works over the *real* designcraft document, driven through its public API, and the
//! guarantee under test is the pilot's own: `apply(diff(a, b), a) == b`, with positions, for real
//! edits — plus the two-client convergence of the generic state when both sides edit, and
//! DesignCraft's own rows: the repagination roundtrip (spread churn) and the `PageRange`
//! region-lock property.

// Tests may unwrap: a failing assertion is the point of a test (the family's clippy rule).
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use craft_collab::adapter::HubAdapter;
use designcraft_doc::build::NewDocument;
use designcraft_doc::geom::{Affine, Point, Rect, shapes};
use designcraft_doc::{Content, Document, Fill, Item, ItemId, Layer, ParaFormat, Section, Shape, SpreadRef, TextFrame};
use designcraft_hub::{DesigncraftHub, ROOT};
use std::sync::Arc;

fn sample() -> Document {
    // Three non-facing pages (one spread each) + the A-Parent, a text frame, two rectangles
    // and a group of two — the shape of a real small layout.
    let mut doc = Document::new(&NewDocument { title: "roundtrip".into(), pages: 3, facing_pages: false, ..Default::default() });
    let layer = doc.default_layer();
    let _ = doc.add_text_frame(SpreadRef::Doc(0), Rect::new(36.0, 36.0, 300.0, 200.0), layer, "Hello", ParaFormat::default()).unwrap();
    let one = Item::new(ItemId(doc.alloc()), layer, Shape::Rectangle, shapes::rectangle(Rect::new(50.0, 250.0, 150.0, 350.0)));
    doc.insert_item(SpreadRef::Doc(0), one, None).unwrap();
    let two = Item::new(ItemId(doc.alloc()), layer, Shape::Oval, shapes::ellipse(Rect::new(200.0, 250.0, 280.0, 330.0)));
    doc.insert_item(SpreadRef::Doc(0), two, None).unwrap();
    let a = Item::new(ItemId(doc.alloc()), layer, Shape::Rectangle, shapes::rectangle(Rect::new(0.0, 0.0, 40.0, 40.0)));
    let b = Item::new(ItemId(doc.alloc()), layer, Shape::Polygon, shapes::polygon(Point::new(80.0, 20.0), 20.0, 6, 0.0));
    let mut group = Item::new(ItemId(doc.alloc()), layer, Shape::Group, Default::default());
    group.content = Content::Group { items: vec![Arc::new(a), Arc::new(b)] };
    group.xf = Affine::translate((400.0, 100.0));
    doc.insert_item(SpreadRef::Doc(2), group, None).unwrap();
    doc
}

fn item_by_id(doc: &Document, raw: u64) -> &Item {
    doc.item(ItemId(raw)).unwrap()
}

fn assert_same_state(hub: &DesigncraftHub, a: &Document, b: &Document, what: &str) {
    let (va, vb) = (hub.view(a), hub.view(b));
    assert_eq!(va.nodes.len(), vb.nodes.len(), "{what}: node count");
    for (na, nb) in va.nodes.iter().zip(vb.nodes.iter()) {
        assert_eq!(na.id, nb.id, "{what}: node id");
        assert_eq!(na.kind, nb.kind, "{what}: kind of {na:?}");
        assert_eq!(na.parent, nb.parent, "{what}: parent of {na:?}");
        assert_eq!(na.position, nb.position, "{what}: position of {na:?}");
        assert_eq!(na.props, nb.props, "{what}: props of {na:?}");
    }
    assert_eq!(va.entries, vb.entries, "{what}: entries");
    assert_eq!(va.doc, vb.doc, "{what}: doc props");
}

/// The core promise, kept: applying the diff of two documents to the first makes it the second.
#[test]
fn apply_diff_roundtrip() {
    let hub = DesigncraftHub::new();
    let a = sample();
    let mut b = a.clone();

    // Field edits on a frame and a group child.
    let frame = b.all_items()[1];
    b.item_mut(frame).unwrap().name = "Renamed".into();
    b.item_mut(frame).unwrap().opacity = 0.5;
    b.item_mut(frame).unwrap().fill = Fill::swatch("[Black]");
    let _group_id = b.spreads[2].items[0].id;
    let child = b.spreads[2].items[0].children()[1].id;
    b.item_mut(child).unwrap().locked = true;

    // Creation, deletion, z-order move.
    let fresh = Item::new(ItemId(b.alloc()), b.default_layer(), Shape::Rectangle, shapes::rectangle(Rect::new(10.0, 400.0, 90.0, 460.0)));
    b.insert_item(SpreadRef::Doc(0), fresh, Some(0)).unwrap();
    b.remove_item(b.all_items()[2]).unwrap();
    let sp = Arc::make_mut(&mut b.spreads[0]);
    let moved = sp.items.pop().unwrap();
    Arc::make_mut(&mut b.spreads[1]).items.push(moved);

    // A story edit (a realistic one: a caret insert, keeping the runs consistent).
    let story = *b.stories.keys().next().unwrap();
    b.story_mut(story).unwrap().insert(5, ", redesigned");

    // Document level.
    b.title = "renamed file".into();
    b.settings.bleed = [9.0, 9.0, 9.0, 9.0];

    let changes = hub.diff(&a, &b);
    assert!(!changes.is_empty());

    let mut applied = a.clone();
    hub.apply(&mut applied, &changes);
    assert_same_state(&hub, &applied, &b, "roundtrip");
    // The roundtrip is stable: diffing again finds nothing.
    assert!(hub.diff(&applied, &b).is_empty(), "the applied state diffs clean against the target");
    // And the applied document is a *valid* document: unique ids, consistent threads.
    applied.check().unwrap();
}

/// Repagination churns spread ids (new spreads on shuffle); the diff must treat that honestly —
/// spread deletes and creates, page parent moves — and still roundtrip.
#[test]
fn repagination_roundtrip() {
    let hub = DesigncraftHub::new();
    let a = sample();
    let mut b = a.clone();

    // Insert two pages in the middle (splices into a spread, then repaginates).
    b.insert_pages(Some(0), 2, b.parents.first().map(|p| p.id)).unwrap();
    // Move the last page to the front (flattens to single-page spreads and rebuilds).
    b.move_page(b.page_count() - 1, 0).unwrap();

    let changes = hub.diff(&a, &b);
    let creates = changes.iter().filter(|c| matches!(c, craft_collab::change::Change::Create { .. })).count();
    let deletes = changes.iter().filter(|c| matches!(c, craft_collab::change::Change::Delete { .. })).count();
    assert!(creates > 0, "repagination creates spreads (fresh ids)");
    assert!(deletes > 0, "repagination deletes spreads (fresh ids)");

    let mut applied = a.clone();
    hub.apply(&mut applied, &changes);
    assert_same_state(&hub, &applied, &b, "repagination");
    assert!(hub.diff(&applied, &b).is_empty());
    assert_eq!(applied.page_count(), b.page_count());
    applied.check().unwrap();
}

/// An insertion in the middle costs one position key — the siblings keep theirs.
#[test]
fn insertion_in_the_middle_costs_one_key() {
    let hub = DesigncraftHub::new();
    let a = sample();
    let mut b = a.clone();
    let fresh = Item::new(ItemId(b.alloc()), b.default_layer(), Shape::Rectangle, shapes::rectangle(Rect::new(10.0, 10.0, 60.0, 60.0)));
    let fresh_id = fresh.id;
    b.insert_item(SpreadRef::Doc(0), fresh, Some(2)).unwrap();

    let changes = hub.diff(&a, &b);
    let positions = changes.iter().filter(|c| matches!(c, craft_collab::change::Change::Set { prop, .. } if prop.as_ref() == "position")).count();
    let creates = changes.iter().filter(|c| matches!(c, craft_collab::change::Change::Create { .. })).count();
    assert_eq!(creates, 1, "one created item");
    assert_eq!(positions, 1, "exactly one position write: the inserted item");

    let mut applied = a.clone();
    hub.apply(&mut applied, &changes);
    assert_same_state(&hub, &applied, &b, "insert");
    // And the middle item really sits in the middle.
    assert_eq!(applied.spreads[0].items[2].id.0, fresh_id.0);
}

/// Two clients editing different fields of the same frame converge (the server orders the
/// batches; last-writer-wins per property, and neither side loses the other's write).
#[test]
fn two_clients_converge() {
    let hub = DesigncraftHub::new();
    let base = sample();

    let mut local = base.clone();
    let mut remote = base.clone();
    let frame = local.all_items()[1];
    local.item_mut(frame).unwrap().name = "Local name".into();
    remote.item_mut(frame).unwrap().opacity = 0.25;

    let local_batch = hub.diff(&base, &local);
    let remote_batch = hub.diff(&base, &remote);
    assert_eq!(local_batch.len(), 1, "one field write locally: {local_batch:?}");
    assert_eq!(remote_batch.len(), 1, "one field write remotely: {remote_batch:?}");

    // Both sides receive both batches (the server echoes each to everyone).
    hub.apply(&mut local, &remote_batch);
    hub.apply(&mut remote, &local_batch);
    assert_same_state(&hub, &local, &remote, "convergence");
}

/// A subtree delete is one change, and deleting a deleted object does nothing.
#[test]
fn subtree_delete_is_one_change() {
    let hub = DesigncraftHub::new();
    let a = sample();
    let mut b = a.clone();
    let group_id = b.spreads[2].items[0].id;
    b.remove_item(group_id).unwrap();

    let changes = hub.diff(&a, &b);
    let deletes = changes.iter().filter(|c| matches!(c, craft_collab::change::Change::Delete { .. })).count();
    assert_eq!(deletes, 1, "one Delete for the whole group");

    let mut applied = a.clone();
    hub.apply(&mut applied, &changes);
    assert_same_state(&hub, &applied, &b, "delete");
    // Tolerant re-apply: deleting a deleted object does nothing (skipped, not an error).
    let report = hub.apply(&mut applied, &changes);
    assert_eq!(report.applied, 0);
    applied.check().unwrap();
}

/// Document-level lists sync per entry with their order (the keyed-list discipline).
#[test]
fn document_entries_sync_with_order() {
    let hub = DesigncraftHub::new();
    let a = sample();
    let mut b = a.clone();
    let second_layer = designcraft_doc::LayerId(b.alloc());
    b.layers.push(Layer {
        id: second_layer,
        name: "Second".into(),
        color: [255, 0, 0],
        visible: true,
        locked: false,
        printable: true,
        show_guides: true,
        suppress_wrap_when_hidden: false,
    });
    let ink_base = b.swatches[2].clone();
    b.swatches.push(designcraft_doc::color::Swatch { name: "Brand ink".into(), ..ink_base });
    b.swatches.swap(1, 2);
    b.sections.push(Section { start: 2, start_number: Some(1), ..b.sections[0].clone() });

    let changes = hub.diff(&a, &b);
    let puts = changes.iter().filter(|c| matches!(c, craft_collab::change::Change::Put { .. })).count();
    let orders = changes
        .iter()
        .filter(|c| matches!(c, craft_collab::change::Change::SetDoc { prop, .. } if matches!(prop.as_ref(), "layerOrder" | "swatchOrder" | "sectionOrder")))
        .count();
    assert_eq!(puts, 3, "one Put per new layer, swatch and section");
    assert_eq!(orders, 3, "one order property per list whose key list changed: the new layer, the swatch swap, the appended section");

    let mut applied = a.clone();
    hub.apply(&mut applied, &changes);
    assert_same_state(&hub, &applied, &b, "entries");
    assert_eq!(applied.layers.last().unwrap().name, "Second");
    assert_eq!(applied.swatches[1].name, b.swatches[1].name, "the swatch swap landed");
    assert_eq!(applied.swatches[2].name, b.swatches[2].name, "both swapped slots landed");
    assert_eq!(applied.swatches.last().unwrap().name, "Brand ink");
}

/// The schema table: kinds and parents validate, unknown ones refuse with a reason.
#[test]
fn schema_validates_kinds_and_parents() {
    use craft_collab::adapter::DocSchema;
    use craft_collab::change::Change;
    use craft_collab::replica::Replica;
    use std::sync::Arc;

    let schema = designcraft_hub::schema::DesignSchema::default();
    let replica = Replica::default();
    let ctx = craft_collab::adapter::ValidationCtx { replica: &replica, author_space: 1, limits: &craft_collab::Limits::default() };

    // An item under the root: refused (only spreads attach to the root).
    let item_at_root = Change::Create {
        id: craft_collab::ids::NodeId::compose(1, 1),
        kind: Arc::from("rectangle"),
        parent: ROOT,
        position: "a0".into(),
        props: vec![],
    };
    assert!(schema.validate(&item_at_root, &ctx).is_err());

    // A spread under the root: accepted.
    let spread_at_root = Change::Create {
        id: craft_collab::ids::NodeId::compose(1, 2),
        kind: Arc::from("spread"),
        parent: ROOT,
        position: "a1".into(),
        props: vec![],
    };
    assert!(schema.validate(&spread_at_root, &ctx).is_ok());

    // An unknown kind: refused with a reason.
    let unknown_kind = Change::Create {
        id: craft_collab::ids::NodeId::compose(1, 3),
        kind: Arc::from("hologram"),
        parent: ROOT,
        position: "a2".into(),
        props: vec![],
    };
    assert!(schema.validate(&unknown_kind, &ctx).is_err());

    // The regionLocks document property validates its shape: a PageRange lock passes...
    let locks_ok = Change::SetDoc {
        prop: Arc::from("regionLocks"),
        value: craft_collab::Value::Json(serde_json::json!([{
            "scope": { "PageRange": { "first": 1, "last": 3 } },
            "subjects": ["editor"],
            "effect": "ReadOnly",
            "label": "Locked spread",
            "color": null
        }])),
    };
    assert!(schema.validate(&locks_ok, &ctx).is_ok());
    // ...and a malformed one refuses.
    let locks_bad =
        Change::SetDoc { prop: Arc::from("regionLocks"), value: craft_collab::Value::Json(serde_json::json!([{ "effect": "ReadOnly" }])) };
    assert!(schema.validate(&locks_bad, &ctx).is_err());
}

/// §6.7 for DesignCraft: region locks sync as document properties, of the `PageRange` shape,
/// and the adapter holds the list until the model grows its own field.
#[test]
fn region_locks_sync_as_document_properties() {
    let hub = DesigncraftHub::new();
    let mut doc = sample();

    let locks = serde_json::json!([
        {
            "scope": { "PageRange": { "first": 1, "last": 2 } },
            "subjects": ["editor"],
            "effect": "ReadOnly",
            "label": "Approved pages",
            "color": "#4a90d9"
        },
        {
            "scope": { "Subtree": 42 },
            "subjects": [],
            "effect": "SuggestOnly",
            "label": "Group edits need review",
            "color": null
        }
    ]);
    let change = craft_collab::change::Change::SetDoc { prop: std::sync::Arc::from("regionLocks"), value: craft_collab::Value::Json(locks.clone()) };
    let report = hub.apply(&mut doc, &[change]);
    assert_eq!(report.applied, 1);

    // The adapter holds the list; a peer's session layer reads it to tint and guard.
    let held = hub.region_locks();
    assert_eq!(held.len(), 2);
    let page_lock = &held[0];
    assert!(page_lock.covers_page(1) && page_lock.covers_page(2), "the range is inclusive");
    assert!(!page_lock.covers_page(0) && !page_lock.covers_page(3), "outside the range stays editable");
    assert!(!held[1].covers_page(1), "a subtree lock covers no page directly");

    // The view carries the list as the regionLocks document property (what a snapshot syncs).
    let view = hub.view(&doc);
    let prop = view.doc.iter().find(|(k, _)| k == "regionLocks");
    assert!(prop.is_some(), "regionLocks rides the document view");
    assert_eq!(prop.map(|(_, v)| v.to_json()), Some(locks.clone()));

    // A second peer's adapter converges by applying the same property write.
    let peer = DesigncraftHub::new();
    let mut peer_doc = sample();
    let change = craft_collab::change::Change::SetDoc { prop: std::sync::Arc::from("regionLocks"), value: craft_collab::Value::Json(locks) };
    peer.apply(&mut peer_doc, &[change]);
    assert_eq!(peer.region_locks(), held);
}

/// The command catalog resolves the legacy names onto their `hub.*` twins (§18.10).
#[test]
fn command_catalog_resolves_legacy_names() {
    assert_eq!(designcraft_hub::commands::resolve("collab.share").map(|c| c.name), Some("hub.share"));
    assert_eq!(designcraft_hub::commands::resolve("hub.peers").map(|c| c.name), Some("hub.peers"));
    assert!(designcraft_hub::commands::resolve("hub.teleport").is_none());
}

/// A group's content property never smuggles a subtree: the wire form is elided, and a value
/// carrying `items` is refused.
#[test]
fn group_content_rides_without_its_children() {
    let hub = DesigncraftHub::new();
    let a = sample();
    let mut b = a.clone();
    let group_id = b.spreads[2].items[0].id;
    let child = b.spreads[2].items[0].children()[0].id;
    b.item_mut(child).unwrap().hidden = true; // deep inside the group

    let changes = hub.diff(&a, &b);
    // The child's own write is there; the group's content property is not re-sent.
    assert!(changes.iter().any(|c| matches!(c, craft_collab::change::Change::Set { id, prop, .. } if id.0 == child.0 && prop.as_ref() == "hidden")));
    assert!(
        !changes.iter().any(|c| matches!(c, craft_collab::change::Change::Set { id, prop, .. } if id.0 == group_id.0 && prop.as_ref() == "content"))
    );

    // A content value carrying items is refused, never applied.
    let mut hostile = sample();
    let report = hub.apply(
        &mut hostile,
        &[craft_collab::change::Change::Set {
            id: DesigncraftHub::hub_id(group_id.0),
            prop: std::sync::Arc::from("content"),
            value: craft_collab::Value::Json(serde_json::json!({ "type": "group", "items": [] })),
        }],
    );
    assert_eq!(report.applied, 0);
    assert_eq!(report.skipped, 1);
}

/// The elided `content` property still round-trips a real content change (a frame that becomes
/// a text frame), and the frame's story entry follows in the same batch.
#[test]
fn content_change_roundtrips_with_its_story() {
    let hub = DesigncraftHub::new();
    let a = sample();
    let mut b = a.clone();
    let oval = b.spreads[0].items.iter().find(|i| i.shape == Shape::Oval).map(|i| i.id).unwrap();
    let sid = designcraft_doc::StoryId(b.alloc());
    let mut story = designcraft_doc::Story::with_text(sid, "Typed into the oval", ParaFormat::default());
    story.frames.push(oval);
    b.stories.insert(sid, Arc::new(story));
    b.item_mut(oval).unwrap().content = Content::Text(TextFrame { story: sid, options: Default::default() });

    let changes = hub.diff(&a, &b);
    let mut applied = a.clone();
    hub.apply(&mut applied, &changes);
    assert_same_state(&hub, &applied, &b, "content change");
    assert!(item_by_id(&applied, oval.0).is_text_frame());
    applied.check().unwrap();
}
