use std::collections::{HashMap, HashSet};
use std::sync::Arc;

use designcraft_doc::{Content, Guide, Hyperlink, HyperlinkSource, Item, ItemId, SpreadRef, Story, StoryId};

use super::{LAYER_COLORS, Layer, LayerId, Result, Session, Value, bad, json, ok};

/// Layer Options: every field in one undo step; a bad name or colour changes nothing.
pub(super) fn set_layer(s: &mut Session, p: &Value) -> Result<Value> {
    const ID: &str = "layer.set";
    let id = LayerId(p.get("id").and_then(Value::as_u64).ok_or_else(|| bad(ID, "missing id"))?);
    let name = match p.get("name") {
        Some(v) => Some(v.as_str().map(str::trim).filter(|n| !n.is_empty()).ok_or_else(|| bad(ID, "a layer needs a name"))?.to_string()),
        None => None,
    };
    let color: Option<[u8; 3]> = match p.get("color") {
        None => None,
        Some(Value::String(n)) => Some(
            LAYER_COLORS
                .iter()
                .find(|(k, _)| k.eq_ignore_ascii_case(n))
                .map(|(_, c)| *c)
                .ok_or_else(|| bad(ID, format!("unknown layer colour `{n}`")))?,
        ),
        Some(v) => Some(serde_json::from_value(v.clone()).map_err(|_| bad(ID, "color is [r,g,b] (0–255) or a layer colour name"))?),
    };
    let flag = |k: &str| match p.get(k) {
        None => Ok(None),
        Some(v) => v.as_bool().map(Some).ok_or_else(|| bad(ID, format!("`{k}` is true or false"))),
    };
    let (visible, locked, printable) = (flag("visible")?, flag("locked")?, flag("printable")?);
    let (guides, wrap) = (flag("showGuides")?, flag("suppressWrapWhenHidden")?);
    s.edit(|d, _| {
        if let Some(n) = &name
            && d.layers.iter().any(|l| l.id != id && l.name == *n)
        {
            return Err(bad(ID, format!("a layer named `{n}` exists")));
        }
        let l = d.layer_mut(id).ok_or_else(|| bad(ID, "no such layer"))?;
        if let Some(n) = name {
            l.name = n;
        }
        if let Some(c) = color {
            l.color = c;
        }
        for (slot, v) in [
            (&mut l.visible, visible),
            (&mut l.locked, locked),
            (&mut l.printable, printable),
            (&mut l.show_guides, guides),
            (&mut l.suppress_wrap_when_hidden, wrap),
        ] {
            if let Some(v) = v {
                *slot = v;
            }
        }
        ok()
    })
}

/// Duplicate Layer: a new layer above `id` with its settings and a copy of every object on it, on document and parent
/// spreads, in one undo step. Copied text frames stay threaded to the other copied frames of their story, and each copy
/// of a story holds all of its text. A copied override of a parent item overrides that item's copy, or is detached when
/// the parent item is on another layer. Pages hide a parent item's copy where they hide the parent item. Hyperlinks on
/// copied objects and text get copies that go to the same destinations.
pub(super) fn duplicate_layer(s: &mut Session, p: &Value) -> Result<Value> {
    const ID: &str = "layer.duplicate";
    let id = LayerId(p.get("id").and_then(Value::as_u64).ok_or_else(|| bad(ID, "missing id"))?);
    let r = s.edit(|d, _| {
        let i = d.layers.iter().position(|l| l.id == id).ok_or_else(|| bad(ID, "no such layer"))?;
        let mut seen = std::collections::HashSet::new();
        if d.spreads.iter().chain(&d.parents).flat_map(|sp| sp.items.iter()).any(|it| it.layer == id && too_deep(it, d, &mut seen)) {
            return Err(bad(ID, "the layer's objects are nested too deeply to copy"));
        }
        let new_id = LayerId(d.alloc());
        let mut layer = d.layers[i].clone();
        layer.id = new_id;
        layer.name = copy_name(&d.layers, &layer.name);
        d.layers.insert(i, layer);
        let src = d.clone();
        let mut stories = HashMap::new();
        let mut ids = HashMap::new();
        let refs = (0..src.parents.len()).map(SpreadRef::Parent).chain((0..src.spreads.len()).map(SpreadRef::Doc));
        let mut copies = Vec::new();
        for r in refs {
            let Some(sp) = src.spread(r) else { continue };
            for it in sp.items.iter().filter(|it| it.layer == id) {
                let mut copy = Item::clone(it);
                super::super::object::renumber(d, &src, &mut copy, &mut stories);
                map_ids(it, &copy, &mut ids);
                copies.push((r, copy));
            }
        }
        // Every copy has its id before overrides are pointed at the copied parent items.
        for (r, mut copy) in copies {
            move_copy(&mut copy, new_id, &ids);
            d.insert_item(r, copy, None)?;
        }
        // A story copy holds the same anchored objects as the original: they get ids of their own, and the stories of
        // anchored text frames get copies in turn. The story copies are taken in story order.
        let mut pending: Vec<StoryId> = stories.values().copied().collect();
        pending.sort_unstable();
        pending.reverse();
        while let Some(new) = pending.pop() {
            let Some(mut st) = d.story(new).cloned() else { continue };
            renumber_anchored(d, &mut st, new_id, &mut stories, &mut ids, &mut pending);
            d.stories.insert(new, Arc::new(st));
        }
        // Each story copy gets its frames in the original order, text anchors of its own, which its cross-references
        // and those of the other copies point at, and tables with ids of their own, the tables in its cells and notes
        // included, allocated in story order.
        let mut anchors = HashMap::new();
        let mut used = HashSet::new();
        for st in d.stories.values() {
            anchor_ids(st, &mut used);
        }
        let mut order: Vec<(StoryId, StoryId)> = stories.iter().map(|(old, new)| (*old, *new)).collect();
        order.sort_unstable_by_key(|(_, new)| *new);
        for (old, new) in &order {
            let Some(mut st) = d.story(*new).cloned() else { continue };
            if let Some((parent, revision)) = st.link {
                st.link = Some((stories.get(&parent).copied().unwrap_or(parent), revision));
            }
            st.frames = src.story(*old).map(|o| o.frames.iter().filter_map(|f| ids.get(f).copied()).collect()).unwrap_or_default();
            for (k, a) in (1..).zip(st.anchors.iter_mut()) {
                let id = (new.0 << 24) + k;
                anchors.insert(a.id, id);
                used.insert(id);
                Arc::make_mut(a).id = id;
            }
            renumber_tables(d, &mut st);
            renumber_nested_anchors(d, &mut st, &mut anchors, &mut used);
            d.stories.insert(*new, Arc::new(st));
        }
        for new in stories.values() {
            if let Some(st) = d.story_mut(*new) {
                remap_xrefs(st, &anchors);
            }
        }
        // The hyperlinks of copied objects and copied text get copies with ids of their own and the same destinations.
        let links: Vec<_> = src
            .hyperlinks
            .iter()
            .filter_map(|h| {
                let source = match &h.source {
                    HyperlinkSource::Item { id } => HyperlinkSource::Item { id: *ids.get(id)? },
                    HyperlinkSource::Text { story, start, end } => HyperlinkSource::Text { story: *stories.get(story)?, start: *start, end: *end },
                };
                Some((h, source))
            })
            .collect();
        for (h, source) in links {
            let id = d.alloc();
            d.hyperlinks.push(Hyperlink { id, source, ..h.clone() });
        }
        // A page, document or parent, hides a parent item's copy where it hides the parent item, and its ruler guides on
        // the layer get copies on the new layer.
        let hidden = |pg: &designcraft_doc::Page| -> Vec<ItemId> { pg.overridden.iter().filter_map(|o| ids.get(o).copied()).collect() };
        for sp in d.spreads.iter_mut().chain(d.parents.iter_mut()) {
            if sp.pages.iter().all(|pg| hidden(pg).is_empty() && pg.guides.iter().all(|g| g.layer != Some(id))) {
                continue;
            }
            for pg in &mut Arc::make_mut(sp).pages {
                let copies = hidden(pg);
                pg.overridden.extend(copies);
                let guides: Vec<_> = pg.guides.iter().filter(|g| g.layer == Some(id)).map(|g| Guide { layer: Some(new_id), ..g.clone() }).collect();
                pg.guides.extend(guides);
            }
        }
        Ok(new_id)
    })?;
    s.doc_mut()?.active_layer = r;
    Ok(json!({"id": r.0}))
}

/// The name of a copy of layer `name`: "<name> copy", then "<name> copy 2" and up while a layer has the name.
fn copy_name(layers: &[Layer], name: &str) -> String {
    let base = format!("{name} copy");
    let taken = |n: &str| layers.iter().any(|l| l.name == n);
    if !taken(&base) {
        return base;
    }
    (2..).map(|k| format!("{base} {k}")).find(|n| !taken(n)).unwrap_or(base)
}

/// Record each id of `orig` and its descendants against the matching id in `copy`.
fn map_ids(orig: &Item, copy: &Item, ids: &mut HashMap<ItemId, ItemId>) {
    ids.insert(orig.id, copy.id);
    for (o, c) in orig.children().iter().zip(copy.children()) {
        map_ids(o, c, ids);
    }
}

/// A layer whose groups, anchored objects, table cells and notes nest deeper than this within one story or object is
/// not duplicated.
const MAX_COPY_DEPTH: usize = 64;

/// Whether `it`, or a story its text frames reach directly or through anchored text frames, nests groups, anchored
/// objects, table cells and notes deeper than [`MAX_COPY_DEPTH`]. Stories in `seen` were checked already and are skipped.
fn too_deep(it: &Item, d: &designcraft_doc::Document, seen: &mut std::collections::HashSet<StoryId>) -> bool {
    fn item(it: &Item, depth: usize, next: &mut Vec<StoryId>) -> bool {
        if let Some(t) = it.text_frame() {
            next.push(t.story);
        }
        depth > MAX_COPY_DEPTH || it.children().iter().any(|c| item(c, depth + 1, next))
    }
    fn story(st: &Story, depth: usize, next: &mut Vec<StoryId>) -> bool {
        depth > MAX_COPY_DEPTH
            || st.objects.iter().any(|o| item(&o.item, depth + 1, next))
            || st.tables.values().flat_map(|t| &t.cells).any(|c| story(&c.text, depth + 1, next))
            || st.notes.iter().chain(&st.endnotes).any(|n| story(&n.text, depth + 1, next))
    }
    let mut next = Vec::new();
    if item(it, 0, &mut next) {
        return true;
    }
    while let Some(sid) = next.pop() {
        if seen.insert(sid)
            && let Some(st) = d.story(sid)
            && story(st, 0, &mut next)
        {
            return true;
        }
    }
    false
}

/// Give the items anchored in `st`, and in the stories of its table cells and notes, new ids from `d` (recorded in `ids`)
/// and layer `to`. An anchored text frame's story gets one copy, recorded in `stories`, that every copied frame of the
/// story shares; a new copy goes on `pending`, and its frame list is filled in afterwards. [`too_deep`] bounds the nesting.
fn renumber_anchored(
    d: &mut designcraft_doc::Document,
    st: &mut Story,
    to: LayerId,
    stories: &mut HashMap<StoryId, StoryId>,
    ids: &mut HashMap<ItemId, ItemId>,
    pending: &mut Vec<StoryId>,
) {
    for o in &mut st.objects {
        renumber_anchored_item(d, &mut Arc::make_mut(o).item, to, stories, ids, pending);
    }
    for t in st.tables.values_mut() {
        for c in &mut Arc::make_mut(t).cells {
            renumber_anchored(d, &mut c.text, to, stories, ids, pending);
        }
    }
    for n in st.notes.iter_mut().chain(st.endnotes.iter_mut()) {
        renumber_anchored(d, &mut Arc::make_mut(n).text, to, stories, ids, pending);
    }
}

/// Give the tables of `st`, and the tables in the stories of its table cells and notes, new ids from `d`, and point the
/// paragraphs that hold them at the new ids. [`too_deep`] bounds the nesting.
fn renumber_tables(d: &mut designcraft_doc::Document, st: &mut Story) {
    let mut tables = HashMap::new();
    for (t, mut table) in std::mem::take(&mut st.tables) {
        let n = d.alloc();
        let table_mut = Arc::make_mut(&mut table);
        table_mut.id = n;
        for c in &mut table_mut.cells {
            renumber_tables(d, &mut c.text);
        }
        tables.insert(t, n);
        st.tables.insert(n, table);
    }
    for pf in &mut st.paras {
        pf.table = pf.table.and_then(|t| tables.get(&t).copied());
    }
    for n in st.notes.iter_mut().chain(st.endnotes.iter_mut()) {
        renumber_tables(d, &mut Arc::make_mut(n).text);
    }
}

/// Add the ids of the text anchors in `st`, and in the stories of its table cells and notes at any depth, to `used`.
/// The walk keeps its own stack, so a story on another layer that nests deeper than [`too_deep`] allows is read too.
fn anchor_ids(st: &Story, used: &mut HashSet<u64>) {
    let mut stack = vec![st];
    while let Some(st) = stack.pop() {
        used.extend(st.anchors.iter().map(|a| a.id));
        stack.extend(st.tables.values().flat_map(|t| t.cells.iter().map(|c| &c.text)));
        stack.extend(st.notes.iter().chain(st.endnotes.iter()).map(|n| &n.text));
    }
}

/// Give the text anchors in the stories of the table cells and notes of `st` new ids from `d` that are not in `used`
/// (recorded in `anchors` and `used`). [`too_deep`] bounds the nesting.
fn renumber_nested_anchors(d: &mut designcraft_doc::Document, st: &mut Story, anchors: &mut HashMap<u64, u64>, used: &mut HashSet<u64>) {
    fn child(d: &mut designcraft_doc::Document, st: &mut Story, anchors: &mut HashMap<u64, u64>, used: &mut HashSet<u64>) {
        for anchor in &mut st.anchors {
            let id = loop {
                let id = d.alloc();
                if used.insert(id) {
                    break id;
                }
            };
            anchors.insert(anchor.id, id);
            Arc::make_mut(anchor).id = id;
        }
        renumber_nested_anchors(d, st, anchors, used);
    }
    for table in st.tables.values_mut() {
        for cell in &mut Arc::make_mut(table).cells {
            child(d, &mut cell.text, anchors, used);
        }
    }
    for note in st.notes.iter_mut().chain(st.endnotes.iter_mut()) {
        child(d, &mut Arc::make_mut(note).text, anchors, used);
    }
}

/// Point the cross-references in `st`, and in the stories of its table cells and notes, at the anchors `map` gives for
/// their targets. [`too_deep`] bounds the nesting.
fn remap_xrefs(st: &mut Story, map: &HashMap<u64, u64>) {
    for x in &mut st.xrefs {
        if let Some(n) = map.get(&x.target) {
            Arc::make_mut(x).target = *n;
        }
    }
    for t in st.tables.values_mut() {
        for c in &mut Arc::make_mut(t).cells {
            remap_xrefs(&mut c.text, map);
        }
    }
    for n in st.notes.iter_mut().chain(st.endnotes.iter_mut()) {
        remap_xrefs(&mut Arc::make_mut(n).text, map);
    }
}

/// [`renumber_anchored`] for one anchored item and its descendants.
fn renumber_anchored_item(
    d: &mut designcraft_doc::Document,
    it: &mut Item,
    to: LayerId,
    stories: &mut HashMap<StoryId, StoryId>,
    ids: &mut HashMap<ItemId, ItemId>,
    pending: &mut Vec<StoryId>,
) {
    let id = ItemId(d.alloc());
    ids.insert(it.id, id);
    it.id = id;
    it.layer = to;
    if let Content::Text(tf) = &mut it.content {
        tf.story = match stories.get(&tf.story) {
            Some(n) => *n,
            None => {
                let n = StoryId(d.alloc());
                let mut copy = d.story(tf.story).cloned().unwrap_or_else(|| Story::new(n));
                copy.id = n;
                copy.frames.clear();
                d.stories.insert(n, Arc::new(copy));
                stories.insert(tf.story, n);
                pending.push(n);
                n
            }
        };
    }
    for c in it.children_mut().into_iter().flatten() {
        renumber_anchored_item(d, Arc::make_mut(c), to, stories, ids, pending);
    }
}

/// Put `it` and its descendants on layer `to`, and point overrides at the copied parent items and live captions at the
/// copied objects. A copy whose parent item was not copied is detached from it, so the original override stays the only
/// one of that item.
fn move_copy(it: &mut Item, to: LayerId, ids: &HashMap<ItemId, ItemId>) {
    it.layer = to;
    it.overrides = it.overrides.and_then(|o| ids.get(&o).copied());
    if let Some(c) = &mut it.live_caption
        && let Some(n) = ids.get(&c.source)
    {
        c.source = *n;
    }
    for c in it.children_mut().into_iter().flatten() {
        move_copy(Arc::make_mut(c), to, ids);
    }
}

#[cfg(test)]
mod layer_tests {
    use serde_json::json;

    #[test]
    fn layer_move_reorders_and_rejects_bad_input() {
        let mut s = crate::Session::new();
        s.execute("file.new", &json!({})).unwrap();
        let l1 = s.doc().unwrap().doc.layers[0].id.0;
        let l2 = s.execute("layer.new", &json!({})).unwrap()["id"].as_u64().unwrap();
        let order = |s: &crate::Session| s.doc().unwrap().doc.layers.iter().map(|l| l.id.0).collect::<Vec<_>>();
        assert_eq!(order(&s), [l2, l1]);
        for bad in [json!({"id": l1, "to": 2}), json!({"id": 999, "to": 0}), json!({"id": l1}), json!({"to": 0})] {
            assert!(s.execute("layer.move", &bad).is_err(), "{bad}");
            assert_eq!(order(&s), [l2, l1]);
        }
        s.execute("layer.move", &json!({"id": l1, "to": 0})).unwrap();
        assert_eq!(order(&s), [l1, l2]);
    }

    #[test]
    fn merge_delete_unused_hide_and_lock_others() {
        let mut s = crate::Session::new();
        s.execute("file.new", &json!({})).unwrap();
        let l1 = s.doc().unwrap().doc.layers[0].id.0;
        let l2 = s.execute("layer.new", &json!({"name": "Art"})).unwrap()["id"].as_u64().unwrap();
        let l3 = s.execute("layer.new", &json!({"name": "Empty"})).unwrap()["id"].as_u64().unwrap();
        s.execute("layer.activate", &json!({"id": l2})).unwrap();
        let f = s.execute("frame.create", &json!({"rect": [0, 0, 10, 10]})).unwrap()["id"].as_u64().unwrap();
        s.execute("layer.others", &json!({"id": l2, "hide": true, "lock": true})).unwrap();
        let d = s.doc().unwrap().doc.clone();
        assert!(d.layers.iter().all(|l| (l.id.0 == l2) == l.visible && (l.id.0 != l2) == l.locked));
        s.execute("layer.others", &json!({"id": l2, "show": true, "unlock": true})).unwrap();
        assert!(s.doc().unwrap().doc.layers.iter().all(|l| l.visible && !l.locked));
        let r = s.execute("layer.deleteUnused", &json!({})).unwrap();
        assert_eq!(r["deleted"], 2, "the empty default layer and Empty");
        let _ = (l1, l3);
        let l4 = s.execute("layer.new", &json!({"name": "Into"})).unwrap()["id"].as_u64().unwrap();
        s.execute("layer.merge", &json!({"ids": [l4, l2]})).unwrap();
        let d = &s.doc().unwrap().doc;
        assert_eq!(d.layers.len(), 1);
        assert_eq!(d.item(designcraft_doc::ItemId(f)).unwrap().layer.0, l4);
    }

    fn on_layer(d: &designcraft_doc::Document, layer: u64) -> Vec<std::sync::Arc<designcraft_doc::Item>> {
        d.spreads.iter().chain(&d.parents).flat_map(|sp| sp.items.iter().filter(|i| i.layer.0 == layer).cloned()).collect()
    }

    #[test]
    fn duplicate_layer_copies_settings_and_objects_above_the_original() {
        let mut s = crate::Session::new();
        s.execute("file.new", &json!({"pages": 2, "facingPages": false})).unwrap();
        let base = s.doc().unwrap().doc.layers[0].id.0;
        let art = s.execute("layer.new", &json!({"name": "Art"})).unwrap()["id"].as_u64().unwrap();
        s.execute("layer.set", &json!({"id": art, "color": "Red", "printable": false, "showGuides": false})).unwrap();
        let rect = s.execute("frame.create", &json!({"rect": [10, 10, 50, 50]})).unwrap()["id"].as_u64().unwrap();
        s.execute("frame.create", &json!({"spread": 1, "rect": [10, 10, 60, 60]})).unwrap();
        s.execute("guide.add", &json!({"orientation": "horizontal", "position": 200, "page": 0, "liquid": true})).unwrap();
        s.execute("layer.activate", &json!({"id": base})).unwrap();
        s.execute("frame.create", &json!({"rect": [100, 100, 150, 150]})).unwrap();
        s.execute("guide.add", &json!({"orientation": "vertical", "position": 300, "page": 0})).unwrap();
        s.execute("layer.set", &json!({"id": art, "locked": true})).unwrap();
        let before = s.doc().unwrap().doc.clone();
        let copy = s.execute("layer.duplicate", &json!({"id": art})).unwrap()["id"].as_u64().unwrap();
        let d = s.doc().unwrap().doc.clone();
        assert_eq!(d.layers.iter().map(|l| l.id.0).collect::<Vec<_>>(), [copy, art, base], "the copy sits right above the original");
        let (l, o) = (d.layer(designcraft_doc::LayerId(copy)).unwrap(), d.layer(designcraft_doc::LayerId(art)).unwrap());
        assert_eq!(l.name, "Art copy");
        assert_eq!((l.color, l.visible, l.locked, l.printable, l.show_guides), (o.color, o.visible, o.locked, o.printable, o.show_guides));
        assert_eq!(s.doc().unwrap().active_layer.0, copy);
        let (orig, copies) = (on_layer(&d, art), on_layer(&d, copy));
        assert_eq!(copies.len(), 2, "both spreads' objects");
        for (a, b) in orig.iter().zip(&copies) {
            assert_ne!(a.id, b.id);
            assert_eq!((a.xf, a.path.clone()), (b.xf, b.path.clone()));
            assert_eq!(d.find(a.id).unwrap().spread, d.find(b.id).unwrap().spread);
        }
        assert_eq!(on_layer(&d, base).len(), 1, "other layers' objects stay as they were");
        let guides = &d.spreads[0].pages[0].guides;
        assert_eq!(guides.len(), 3, "Art's guide gets a copy; the base layer's does not");
        let (g, c) = (&guides[0], &guides[2]);
        assert_eq!((g.layer, c.layer), (Some(designcraft_doc::LayerId(art)), Some(designcraft_doc::LayerId(copy))));
        assert_eq!((g.orientation, g.position, g.liquid), (c.orientation, c.position, c.liquid));
        assert!(d.item(designcraft_doc::ItemId(rect)).is_some());
        // A second copy gets a name of its own; bad ids change nothing.
        s.execute("layer.duplicate", &json!({"id": art})).unwrap();
        assert_eq!(s.doc().unwrap().doc.layers[1].name, "Art copy 2");
        s.execute("edit.undo", &json!({})).unwrap();
        for bad in [json!({"id": 999}), json!({})] {
            assert!(s.execute("layer.duplicate", &bad).is_err(), "{bad}");
        }
        // One undo step.
        s.execute("edit.undo", &json!({})).unwrap();
        let d = &s.doc().unwrap().doc;
        assert_eq!(d.layers, before.layers);
        assert_eq!(d.all_items(), before.all_items());
    }

    #[test]
    fn duplicate_layer_keeps_linked_story_parents_with_their_copies() {
        for copy_parent in [true, false] {
            let mut s = crate::Session::new();
            s.execute("file.new", &json!({})).unwrap();
            let parent = s.execute("frame.create", &json!({"rect": [0, 0, 100, 100], "content": "text", "text": "Original"})).unwrap();
            let layer = if copy_parent {
                s.doc().unwrap().active_layer.0
            } else {
                s.execute("layer.new", &json!({"name": "Linked"})).unwrap()["id"].as_u64().unwrap()
            };
            let child = s.execute("story.placeAndLink", &json!({"story": parent["story"], "rect": [100, 0, 200, 100]})).unwrap();
            let copy = s.execute("layer.duplicate", &json!({"id": layer})).unwrap()["id"].as_u64().unwrap();
            let d = &s.doc().unwrap().doc;
            let copies = on_layer(d, copy);
            let copied_stories: Vec<_> = copies.iter().map(|item| d.story(item.text_frame().unwrap().story).unwrap()).collect();
            let copied_child = copied_stories.iter().find(|story| story.link.is_some()).unwrap().id;
            let expected_parent = if copy_parent {
                copied_stories.iter().find(|story| story.link.is_none()).unwrap().id
            } else {
                designcraft_doc::StoryId(parent["story"].as_u64().unwrap())
            };
            assert_eq!(d.story(copied_child).unwrap().link.unwrap().0, expected_parent);
            s.execute("story.setText", &json!({"story": expected_parent.0, "text": "Edited source"})).unwrap();
            s.execute("story.updateLink", &json!({"story": copied_child.0})).unwrap();
            assert_eq!(s.doc().unwrap().doc.story(copied_child).unwrap().text, "Edited source");
            assert_eq!(s.doc().unwrap().doc.story(designcraft_doc::StoryId(child["story"].as_u64().unwrap())).unwrap().text, "Original");
            if copy_parent {
                s.execute("story.setText", &json!({"story": parent["story"], "text": "Edited original"})).unwrap();
                s.execute("story.updateLink", &json!({"story": copied_child.0})).unwrap();
                assert_eq!(s.doc().unwrap().doc.story(copied_child).unwrap().text, "Edited source");
            }
        }
    }

    #[test]
    fn duplicate_layer_keeps_threads_on_the_layer_only() {
        let mut s = crate::Session::new();
        s.execute("file.new", &json!({})).unwrap();
        let base = s.doc().unwrap().doc.layers[0].id.0;
        let text = |s: &mut crate::Session, rect: [f64; 4], t: &str| {
            s.execute("frame.create", &json!({"rect": rect, "content": "text", "text": t})).unwrap()["id"].as_u64().unwrap()
        };
        // a → b on Base, c on Art at the end of the same thread.
        let b = text(&mut s, [10.0, 200.0, 100.0, 240.0], "");
        let a = text(&mut s, [10.0, 10.0, 100.0, 50.0], "One two three four five six seven eight nine ten eleven twelve");
        s.execute("text.thread", &json!({"from": a, "to": b})).unwrap();
        let art = s.execute("layer.new", &json!({"name": "Art"})).unwrap()["id"].as_u64().unwrap();
        let c = text(&mut s, [300.0, 10.0, 400.0, 50.0], "");
        s.execute("text.thread", &json!({"from": b, "to": c})).unwrap();
        let _ = art;
        let copy = s.execute("layer.duplicate", &json!({"id": base})).unwrap()["id"].as_u64().unwrap();
        let d = s.doc().unwrap().doc.clone();
        let story = |id: u64| d.item(designcraft_doc::ItemId(id)).and_then(|i| i.text_frame()).map(|t| t.story).unwrap();
        let orig = d.story(story(a)).unwrap();
        assert_eq!(orig.frames.iter().map(|f| f.0).collect::<Vec<_>>(), [a, b, c], "the original thread is unchanged");
        let copies = on_layer(&d, copy);
        assert_eq!(copies.len(), 2);
        let sid = copies[0].text_frame().unwrap().story;
        assert_eq!(copies[1].text_frame().unwrap().story, sid, "the copies share one story");
        let st = d.story(sid).unwrap();
        assert_ne!(sid, orig.id);
        assert_eq!(st.text, orig.text, "the copy holds the whole text");
        // Thread order as in the original (a's copy first, though b's copy was made first), without c.
        let pos = |id: designcraft_doc::ItemId| d.find(id).map(|l| l.path.clone());
        let order: Vec<_> = st.frames.iter().map(|f| d.item(*f).unwrap().xf).collect();
        let want: Vec<_> = [a, b].iter().map(|f| d.item(designcraft_doc::ItemId(*f)).unwrap().xf).collect();
        assert_eq!(order, want);
        assert!(st.frames.iter().all(|f| pos(*f).is_some()));
    }

    #[test]
    fn duplicate_layer_copies_parent_items_and_their_overrides() {
        let mut s = crate::Session::new();
        s.execute("file.new", &json!({"pages": 3, "facingPages": false})).unwrap();
        let base = s.doc().unwrap().doc.layers[0].id.0;
        let folio = json!({"spread": {"kind": "parent", "index": 0}, "rect": [72, 720, 300, 750], "content": "text", "text": "Folio"});
        let parent = s.execute("frame.create", &folio).unwrap()["id"].as_u64().unwrap();
        let o = s.execute("layout.overrideParentItems", &json!({"page": 1})).unwrap()["ids"][0].as_u64().unwrap();
        // Page 3's override is deleted, which keeps the parent item hidden there.
        let gone = s.execute("layout.overrideParentItems", &json!({"page": 2})).unwrap()["ids"][0].as_u64().unwrap();
        s.execute("selection.set", &json!({"ids": [gone]})).unwrap();
        s.execute("edit.clear", &json!({})).unwrap();
        // Parent B, based on A, hides the folio on its page.
        s.execute("layout.parents.new", &json!({"prefix": "B"})).unwrap();
        s.edit(|d, _| {
            let a = d.parents[0].id;
            let bp = std::sync::Arc::make_mut(&mut d.parents[1]);
            bp.parent.as_mut().unwrap().based_on = Some(a);
            bp.pages[0].parent = Some(a);
            bp.pages[0].overridden.push(designcraft_doc::ItemId(parent));
            Ok(())
        })
        .unwrap();
        let copy = s.execute("layer.duplicate", &json!({"id": base})).unwrap()["id"].as_u64().unwrap();
        let d = s.doc().unwrap().doc.clone();
        let copies = on_layer(&d, copy);
        assert_eq!(copies.len(), 2, "the parent item and page 2's override");
        let p2 = copies.iter().find(|i| matches!(d.find(i.id).unwrap().spread, designcraft_doc::SpreadRef::Parent(0))).unwrap();
        let o2 = copies.iter().find(|i| i.overrides.is_some()).unwrap();
        assert_ne!((p2.id.0, o2.id.0), (parent, o));
        assert_eq!(o2.overrides, Some(p2.id), "the override's copy overrides the parent item's copy");
        assert_eq!(d.page_of_item(o2.id), Some(1));
        let overridden = |page: usize| {
            let (si, pi) = d.page_loc(page).unwrap();
            d.spreads[si].pages[pi].overridden.clone()
        };
        assert_eq!(overridden(1), [designcraft_doc::ItemId(parent), p2.id], "page 2 hides both parent items");
        assert_eq!(overridden(2), [designcraft_doc::ItemId(parent), p2.id], "page 3 too, without an override");
        assert!(overridden(0).is_empty(), "page 1 shows both");
        assert_eq!(d.parents[1].pages[0].overridden, [designcraft_doc::ItemId(parent), p2.id], "parent B hides both");
    }

    #[test]
    fn duplicate_layer_moves_whole_groups_and_finds_later_parent_items() {
        let mut s = crate::Session::new();
        s.execute("file.new", &json!({})).unwrap();
        let base = s.doc().unwrap().doc.layers[0].id.0;
        let a = s.execute("frame.create", &json!({"rect": [0, 0, 10, 10]})).unwrap()["id"].as_u64().unwrap();
        s.execute("layer.new", &json!({"name": "Art"})).unwrap();
        let b = s.execute("frame.create", &json!({"rect": [20, 0, 30, 10]})).unwrap()["id"].as_u64().unwrap();
        // The group takes the base layer from its first member; b stays on Art inside it.
        let g = s.execute("object.group", &json!({"ids": [a, b]})).unwrap()["id"].as_u64().unwrap();
        s.execute("layer.activate", &json!({"id": base})).unwrap();
        // Parent A's item overrides one on parent B, which comes after it in the document.
        s.execute("layout.parents.new", &json!({"prefix": "B"})).unwrap();
        let on_b =
            s.execute("frame.create", &json!({"spread": {"kind": "parent", "index": 1}, "rect": [0, 0, 50, 50]})).unwrap()["id"].as_u64().unwrap();
        let on_a =
            s.execute("frame.create", &json!({"spread": {"kind": "parent", "index": 0}, "rect": [0, 0, 50, 50]})).unwrap()["id"].as_u64().unwrap();
        s.edit(|d, _| {
            d.item_mut(designcraft_doc::ItemId(on_a)).unwrap().overrides = Some(designcraft_doc::ItemId(on_b));
            Ok(())
        })
        .unwrap();
        let d = s.doc().unwrap().doc.clone();
        let layer_of = |id: u64| d.item(designcraft_doc::ItemId(id)).unwrap().layer.0;
        assert_eq!((layer_of(g), layer_of(b)), (base, d.layers.iter().find(|l| l.name == "Art").unwrap().id.0));
        let copy = s.execute("layer.duplicate", &json!({"id": base})).unwrap()["id"].as_u64().unwrap();
        let d = s.doc().unwrap().doc.clone();
        let copies = on_layer(&d, copy);
        let group = copies.iter().find(|i| !i.children().is_empty()).unwrap();
        assert!(group.children().iter().all(|c| c.layer.0 == copy), "the copied group is on the new layer as a whole");
        let in_parent =
            |i: usize| copies.iter().find(|c| matches!(d.find(c.id).unwrap().spread, designcraft_doc::SpreadRef::Parent(p) if p == i)).unwrap();
        assert_eq!(in_parent(0).overrides, Some(in_parent(1).id), "A's copy overrides B's copy");
    }

    #[test]
    fn duplicate_layer_gives_anchored_objects_their_own_ids() {
        let mut s = crate::Session::new();
        s.execute("file.new", &json!({})).unwrap();
        let base = s.doc().unwrap().doc.layers[0].id.0;
        let text = json!({"rect": [72, 72, 400, 400], "content": "text", "text": "Before after."});
        let f = s.execute("frame.create", &text).unwrap()["id"].as_u64().unwrap();
        let sid = s.doc().unwrap().doc.item(designcraft_doc::ItemId(f)).unwrap().text_frame().unwrap().story;
        let b = s.execute("frame.create", &json!({"rect": [450, 450, 474, 490]})).unwrap()["id"].as_u64().unwrap();
        s.execute("anchored.insert", &json!({"ids": [b], "story": sid.0, "pos": 7})).unwrap();
        // Two anchored text frames threaded in one story, as IDML import makes them.
        let threaded = s
            .edit(|d, _| {
                use designcraft_doc::{AnchoredObject, ParaFormat};
                let (r, r2) = (designcraft_geom::Rect::new(0.0, 0.0, 60.0, 30.0), designcraft_geom::Rect::new(0.0, 0.0, 80.0, 40.0));
                let (first, story) = d.add_text_frame(designcraft_doc::SpreadRef::Doc(0), r, d.default_layer(), "Symbol", ParaFormat::default())?;
                let (second, spare) = d.add_text_frame(designcraft_doc::SpreadRef::Doc(0), r2, d.default_layer(), "", ParaFormat::default())?;
                let mut items = Vec::new();
                for f in [first, second] {
                    let mut it = d.item(f).cloned().ok_or(designcraft_doc::DocError::NoItem(f))?;
                    if let designcraft_doc::Content::Text(tf) = &mut it.content {
                        tf.story = story;
                    }
                    items.push(it);
                }
                d.stories.remove(&spare);
                d.story_mut(story).ok_or(designcraft_doc::DocError::NoStory(story))?.frames = vec![first, second];
                std::sync::Arc::make_mut(&mut d.spreads[0]).items.retain(|i| i.id != first && i.id != second);
                let st = d.story_mut(sid).ok_or(designcraft_doc::DocError::NoStory(sid))?;
                for it in items.into_iter().rev() {
                    st.insert_object(0, AnchoredObject::new(it, Default::default()));
                }
                Ok(story)
            })
            .unwrap();
        s.doc().unwrap().doc.check().unwrap();
        let copy = s.execute("layer.duplicate", &json!({"id": base})).unwrap()["id"].as_u64().unwrap();
        let d = &s.doc().unwrap().doc;
        d.check().unwrap();
        let symbols: Vec<_> = d.anchored_items().into_iter().filter_map(|(_, i)| i.text_frame().map(|t| (i.id, t.story))).collect();
        assert_eq!(symbols.len(), 4);
        let copied = symbols.iter().map(|s| s.1).find(|s| *s != threaded).unwrap();
        let st = d.story(copied).unwrap();
        assert_eq!(st.text, "Symbol");
        assert_eq!(st.frames.len(), 2, "the copies share one story");
        let order = |story: &designcraft_doc::Story| story.frames.iter().map(|f| d.anchored_item(*f).unwrap().path.clone()).collect::<Vec<_>>();
        assert_eq!(order(st), order(d.story(threaded).unwrap()), "in the original thread order");
        let anchored: Vec<_> = d.anchored_items().into_iter().filter(|(_, i)| i.text_frame().is_none()).map(|(_, i)| (i.id, i.layer.0)).collect();
        assert_eq!(anchored.len(), 2);
        assert_ne!(anchored[0].0, anchored[1].0, "the copy's anchored object has an id of its own");
        let mut layers: Vec<_> = anchored.iter().map(|a| a.1).collect();
        layers.sort_unstable();
        let mut want = vec![base, copy];
        want.sort_unstable();
        assert_eq!(layers, want, "the copy's anchored object is on the new layer");
    }

    #[test]
    fn duplicate_layer_points_copied_live_captions_at_the_copied_objects() {
        let mut s = crate::Session::new();
        s.execute("file.new", &json!({})).unwrap();
        let base = s.doc().unwrap().doc.layers[0].id.0;
        let a = s.execute("frame.create", &json!({"rect": [72, 72, 272, 172]})).unwrap()["id"].as_u64().unwrap();
        s.execute("object.altText", &json!({"text": "First", "ids": [a]})).unwrap();
        s.execute("object.caption", &json!({"ids": [a], "text": "Photo: {altText}", "live": true})).unwrap();
        let copy = s.execute("layer.duplicate", &json!({"id": base})).unwrap()["id"].as_u64().unwrap();
        let d = &s.doc().unwrap().doc;
        let copies = on_layer(d, copy);
        let frame = copies.iter().find(|i| i.live_caption.is_none()).unwrap();
        let caption = copies.iter().find_map(|i| i.live_caption.as_ref()).unwrap();
        assert_eq!(caption.source, frame.id, "the caption's copy follows the frame's copy");
        assert_ne!(frame.id.0, a);
    }

    #[test]
    fn duplicate_layer_copies_the_hyperlinks_of_its_objects_and_text() {
        use designcraft_doc::{HyperlinkDest, HyperlinkSource};
        let mut s = crate::Session::new();
        s.execute("file.new", &json!({"pages": 2})).unwrap();
        let base = s.doc().unwrap().doc.layers[0].id.0;
        let a = s.execute("frame.create", &json!({"rect": [72, 72, 172, 172]})).unwrap()["id"].as_u64().unwrap();
        s.execute("hyperlink.create", &json!({"ids": [a], "url": "https://example.com/a", "name": "Logo"})).unwrap();
        let t = s.execute("frame.create", &json!({"rect": [72, 200, 400, 300], "content": "text", "text": "Read more here"})).unwrap();
        let sid = s.doc().unwrap().doc.item(designcraft_doc::ItemId(t["id"].as_u64().unwrap())).unwrap().text_frame().unwrap().story;
        s.execute("text.select", &json!({"story": sid.0, "anchor": 5, "focus": 9})).unwrap();
        s.execute("hyperlink.create", &json!({"page": 2})).unwrap();
        s.execute("layer.new", &json!({"name": "Art"})).unwrap();
        let other = s.execute("frame.create", &json!({"rect": [300, 72, 400, 172]})).unwrap()["id"].as_u64().unwrap();
        s.execute("hyperlink.create", &json!({"ids": [other], "email": "info@example.com"})).unwrap();
        let before = s.doc().unwrap().doc.hyperlinks.clone();
        let copy = s.execute("layer.duplicate", &json!({"id": base})).unwrap()["id"].as_u64().unwrap();
        let d = s.doc().unwrap().doc.clone();
        assert_eq!(d.hyperlinks[..3], before[..], "the original hyperlinks are unchanged");
        let copies = on_layer(&d, copy);
        let frame = copies.iter().find(|i| i.text_frame().is_none()).unwrap().id;
        let story = copies.iter().find_map(|i| i.text_frame()).unwrap().story;
        let added = &d.hyperlinks[3..];
        assert_eq!(added.len(), 2, "the other layer's hyperlink gets no copy");
        assert_eq!((&added[0].name, &added[0].source, &added[0].dest), (&before[0].name, &HyperlinkSource::Item { id: frame }, &before[0].dest));
        assert_eq!(added[1].source, HyperlinkSource::Text { story, start: 5, end: 9 });
        assert_eq!((&added[1].name, &added[1].dest), (&before[1].name, &HyperlinkDest::Page(1)));
        let ids: std::collections::HashSet<u64> = d.hyperlinks.iter().map(|h| h.id).collect();
        assert_eq!(ids.len(), 5, "each copy has an id of its own");
        s.execute("edit.undo", &json!({})).unwrap();
        assert_eq!(s.doc().unwrap().doc.hyperlinks, before, "one undo step removes the copies");
    }

    #[test]
    fn duplicate_layer_gives_copied_text_anchors_their_own_ids() {
        let mut s = crate::Session::new();
        s.execute("file.new", &json!({})).unwrap();
        let base = s.doc().unwrap().doc.layers[0].id.0;
        let text = json!({"rect": [72, 72, 400, 400], "content": "text", "text": "Heading\rSee "});
        let f = s.execute("frame.create", &text).unwrap()["id"].as_u64().unwrap();
        let sid = s.doc().unwrap().doc.item(designcraft_doc::ItemId(f)).unwrap().text_frame().unwrap().story;
        let anchor = s
            .edit(|d, _| {
                let a = d.paragraph_anchor(sid, 0).ok_or(designcraft_doc::DocError::NoStory(sid))?;
                let format = d.xref_formats[0].name.clone();
                let st = d.story_mut(sid).ok_or(designcraft_doc::DocError::NoStory(sid))?;
                let end = st.len();
                st.insert_xref(end, designcraft_doc::CrossRef { target: a, format });
                Ok(a)
            })
            .unwrap();
        let copy = s.execute("layer.duplicate", &json!({"id": base})).unwrap()["id"].as_u64().unwrap();
        let d = &s.doc().unwrap().doc;
        let copied = on_layer(d, copy)[0].text_frame().unwrap().story;
        let (orig, st) = (d.story(sid).unwrap(), d.story(copied).unwrap());
        assert_eq!((orig.anchors[0].id, orig.xrefs[0].target), (anchor, anchor), "the original is unchanged");
        assert_ne!(st.anchors[0].id, anchor, "the copy's anchor has an id of its own");
        assert_eq!(st.anchors[0].name, orig.anchors[0].name);
        assert_eq!(st.xrefs[0].target, st.anchors[0].id, "the copied cross-reference points at the copied paragraph");
        assert_eq!(d.find_anchor(st.anchors[0].id).map(|a| a.0), Some(copied));
    }

    #[test]
    fn duplicate_layer_remaps_anchors_in_cells_and_notes() {
        use designcraft_doc::Story;
        use std::sync::Arc;

        fn add_anchor(st: &mut Story, id: u64) {
            st.insert_anchor(0, "Nested heading");
            Arc::make_mut(&mut st.anchors[0]).id = id;
            st.insert_xref(st.len(), designcraft_doc::CrossRef { target: id, format: String::new() });
        }
        fn collect(st: &Story, values: &mut Vec<(u64, u64)>) {
            for (a, x) in st.anchors.iter().zip(st.xrefs.iter()) {
                values.push((a.id, x.target));
            }
            for table in st.tables.values() {
                for cell in &table.cells {
                    collect(&cell.text, values);
                }
            }
            for note in st.notes.iter().chain(st.endnotes.iter()) {
                collect(&note.text, values);
            }
        }
        let mut s = crate::Session::new();
        s.execute("file.new", &json!({})).unwrap();
        let base = s.doc().unwrap().doc.layers[0].id.0;
        let f = s.execute("frame.create", &json!({"rect": [72, 72, 400, 400], "content": "text", "text": "Rows"})).unwrap()["id"].as_u64().unwrap();
        let sid = s.doc().unwrap().doc.item(designcraft_doc::ItemId(f)).unwrap().text_frame().unwrap().story;
        let d = Arc::make_mut(&mut s.doc_mut().unwrap().doc);
        let table_id = d.alloc();
        let nested_id = d.alloc();
        let anchor_ids = [d.alloc(), d.alloc(), d.alloc(), d.alloc()];
        let mut table = designcraft_doc::Table::new(table_id, 1, 1, 0, 0, 40.0);
        let cell = &mut table.cells[0].text;
        add_anchor(cell, anchor_ids[0]);
        let mut nested = designcraft_doc::Table::new(nested_id, 1, 1, 0, 0, 20.0);
        add_anchor(&mut nested.cells[0].text, anchor_ids[1]);
        cell.insert_table(cell.len(), nested);
        cell.insert_note(cell.len(), "Footnote", Default::default()).unwrap();
        add_anchor(&mut Arc::make_mut(&mut cell.notes[0]).text, anchor_ids[2]);
        cell.insert_endnote(cell.len(), "Endnote", Default::default()).unwrap();
        add_anchor(&mut Arc::make_mut(&mut cell.endnotes[0]).text, anchor_ids[3]);
        d.story_mut(sid).unwrap().insert_table(0, table);
        let mut original = Vec::new();
        collect(d.story(sid).unwrap(), &mut original);
        assert_eq!(original.len(), 4);
        let copy = s.execute("layer.duplicate", &json!({"id": base})).unwrap()["id"].as_u64().unwrap();
        let d = &s.doc().unwrap().doc;
        let copied = on_layer(d, copy)[0].text_frame().unwrap().story;
        let mut after = Vec::new();
        collect(d.story(copied).unwrap(), &mut after);
        assert_eq!(after.len(), original.len());
        let unique: std::collections::HashSet<_> = after.iter().map(|(id, _)| *id).collect();
        assert_eq!(unique.len(), after.len());
        for (id, target) in &after {
            assert!(!anchor_ids.contains(id), "copied nested anchor retains original id {id}");
            assert_eq!(id, target, "copied reference points to its copied anchor");
        }
        let mut unchanged = Vec::new();
        collect(d.story(sid).unwrap(), &mut unchanged);
        assert_eq!(unchanged, original);
        s.execute("edit.undo", &json!({})).unwrap();
        s.execute("edit.redo", &json!({})).unwrap();
        let mut redone = Vec::new();
        collect(s.doc().unwrap().doc.story(copied).unwrap(), &mut redone);
        assert_eq!(redone, after);
    }

    #[test]
    fn duplicate_layer_gives_copied_cell_anchors_ids_no_other_anchor_has() {
        use std::sync::Arc;

        fn ids(st: &designcraft_doc::Story, out: &mut Vec<u64>) {
            out.extend(st.anchors.iter().map(|a| a.id));
            for table in st.tables.values() {
                for cell in &table.cells {
                    ids(&cell.text, out);
                }
            }
        }
        let mut s = crate::Session::new();
        s.execute("file.new", &json!({})).unwrap();
        let base = s.doc().unwrap().doc.layers[0].id.0;
        let f = s.execute("frame.create", &json!({"rect": [72, 72, 400, 400], "content": "text", "text": "Rows"})).unwrap()["id"].as_u64().unwrap();
        let sid = s.doc().unwrap().doc.item(designcraft_doc::ItemId(f)).unwrap().text_frame().unwrap().story;
        let d = Arc::make_mut(&mut s.doc_mut().unwrap().doc);
        let table_id = d.alloc();
        // The cell's anchors take the ids the document hands out next, as anchors made in a cell story can.
        let start = d.alloc();
        let mut table = designcraft_doc::Table::new(table_id, 1, 1, 0, 0, 40.0);
        let cell = &mut table.cells[0].text;
        for k in 1..=64 {
            cell.insert_anchor(0, "Heading");
            Arc::make_mut(&mut cell.anchors[0]).id = start + k;
        }
        d.story_mut(sid).unwrap().insert_table(0, table);
        let mut original = Vec::new();
        ids(d.story(sid).unwrap(), &mut original);
        assert_eq!(original.len(), 64);
        let copy = s.execute("layer.duplicate", &json!({"id": base})).unwrap()["id"].as_u64().unwrap();
        let d = &s.doc().unwrap().doc;
        let copied = on_layer(d, copy)[0].text_frame().unwrap().story;
        let mut after = Vec::new();
        ids(d.story(copied).unwrap(), &mut after);
        assert_eq!(after.len(), original.len());
        for id in &after {
            assert!(!original.contains(id), "copied cell anchor {id} has the id of an original anchor");
        }
    }

    #[test]
    fn duplicate_layer_gives_copied_tables_their_own_ids() {
        let mut s = crate::Session::new();
        s.execute("file.new", &json!({})).unwrap();
        let base = s.doc().unwrap().doc.layers[0].id.0;
        let text = json!({"rect": [72, 72, 400, 400], "content": "text", "text": "Rows"});
        let f = s.execute("frame.create", &text).unwrap()["id"].as_u64().unwrap();
        let sid = s.doc().unwrap().doc.item(designcraft_doc::ItemId(f)).unwrap().text_frame().unwrap().story;
        s.execute("text.select", &json!({"story": sid.0, "anchor": 4, "focus": 4})).unwrap();
        let table = s.execute("table.insert", &json!({"rows": 1, "cols": 2})).unwrap()["table"].as_u64().unwrap();
        let copy = s.execute("layer.duplicate", &json!({"id": base})).unwrap()["id"].as_u64().unwrap();
        let d = &s.doc().unwrap().doc;
        let copied = on_layer(d, copy)[0].text_frame().unwrap().story;
        let st = d.story(copied).unwrap();
        let tables: Vec<u64> = st.tables.keys().copied().collect();
        assert_eq!(tables.len(), 1);
        let new = tables[0];
        assert_ne!(new, table, "the copy's table has an id of its own");
        assert_eq!(st.tables[&new].id, new);
        assert_eq!(st.paras.iter().filter_map(|pf| pf.table).collect::<Vec<_>>(), [new], "the copied paragraph holds the copied table");
        d.check().unwrap();
        s.execute("table.insertRow", &json!({"table": new})).unwrap();
        let d = &s.doc().unwrap().doc;
        assert_eq!(d.story(sid).unwrap().tables[&table].nrows(), 1, "the original table is unchanged");
        assert_eq!(d.story(copied).unwrap().tables[&new].nrows(), 2, "a command naming the copy's table changes the copy");
    }

    #[test]
    fn duplicate_layer_gives_tables_in_copied_cells_their_own_ids() {
        let mut s = crate::Session::new();
        s.execute("file.new", &json!({})).unwrap();
        let base = s.doc().unwrap().doc.layers[0].id.0;
        let text = json!({"rect": [72, 72, 400, 400], "content": "text", "text": "Rows"});
        let f = s.execute("frame.create", &text).unwrap()["id"].as_u64().unwrap();
        let sid = s.doc().unwrap().doc.item(designcraft_doc::ItemId(f)).unwrap().text_frame().unwrap().story;
        s.execute("text.select", &json!({"story": sid.0, "anchor": 4, "focus": 4})).unwrap();
        let table = s.execute("table.insert", &json!({"rows": 1, "cols": 2})).unwrap()["table"].as_u64().unwrap();
        let d = std::sync::Arc::make_mut(&mut s.doc_mut().unwrap().doc);
        let inner = d.alloc();
        let deepest = d.alloc();
        let mut nested = designcraft_doc::Table::new(inner, 1, 1, 0, 0, 40.0);
        nested.cells[0].text.insert_table(0, designcraft_doc::Table::new(deepest, 1, 1, 0, 0, 20.0));
        d.story_mut(sid).unwrap().table_mut(table).unwrap().cells[0].text.insert_table(0, nested);
        let copy = s.execute("layer.duplicate", &json!({"id": base})).unwrap()["id"].as_u64().unwrap();
        let d = &s.doc().unwrap().doc;
        let copied = on_layer(d, copy)[0].text_frame().unwrap().story;
        let outer = d.story(copied).unwrap().tables.values().next().unwrap();
        let cell = &outer.cells[0].text;
        let new_inner = *cell.tables.keys().next().unwrap();
        assert_ne!(new_inner, inner, "a table in a copied cell has an id of its own");
        assert_eq!(cell.tables[&new_inner].id, new_inner);
        assert_eq!(cell.paras.iter().filter_map(|pf| pf.table).collect::<Vec<_>>(), [new_inner], "the copied cell holds the copied table");
        let inner_cell = &cell.tables[&new_inner].cells[0].text;
        let new_deepest = *inner_cell.tables.keys().next().unwrap();
        assert_ne!(new_deepest, deepest, "a table nested deeper has an id of its own");
        assert_eq!(inner_cell.paras.iter().filter_map(|pf| pf.table).collect::<Vec<_>>(), [new_deepest]);
        let ids = [outer.id, new_inner, new_deepest, table, inner, deepest];
        assert_eq!(ids.iter().collect::<std::collections::HashSet<_>>().len(), ids.len(), "every table id is unique: {ids:?}");
        let orig = d.story(sid).unwrap().tables[&table].cells[0].text.tables.keys().copied().collect::<Vec<_>>();
        assert_eq!(orig, [inner], "the original's nested table keeps its id");
    }

    #[test]
    fn duplicate_layer_numbers_copied_tables_and_anchored_objects_in_story_order() {
        let mut s = crate::Session::new();
        s.execute("file.new", &json!({})).unwrap();
        let base = s.doc().unwrap().doc.layers[0].id.0;
        for k in 0..8 {
            let y = 72 + 80 * k;
            let text = json!({"rect": [72, y, 400, y + 60], "content": "text", "text": "Rows"});
            let f = s.execute("frame.create", &text).unwrap()["id"].as_u64().unwrap();
            let sid = s.doc().unwrap().doc.item(designcraft_doc::ItemId(f)).unwrap().text_frame().unwrap().story;
            s.execute("text.select", &json!({"story": sid.0, "anchor": 4, "focus": 4})).unwrap();
            s.execute("table.insert", &json!({"rows": 1, "cols": 2})).unwrap();
            let b = s.execute("frame.create", &json!({"rect": [450, y, 474, y + 40]})).unwrap()["id"].as_u64().unwrap();
            s.execute("anchored.insert", &json!({"ids": [b], "story": sid.0, "pos": 0})).unwrap();
        }
        let copy = s.execute("layer.duplicate", &json!({"id": base})).unwrap()["id"].as_u64().unwrap();
        let d = &s.doc().unwrap().doc;
        d.check().unwrap();
        let mut stories: Vec<_> = on_layer(d, copy).iter().filter_map(|i| i.text_frame().map(|t| t.story)).collect();
        stories.sort_unstable();
        assert_eq!(stories.len(), 8);
        let tables: Vec<u64> = stories.iter().map(|sid| *d.story(*sid).unwrap().tables.keys().next().unwrap()).collect();
        let objects: Vec<u64> = stories.iter().map(|sid| d.story(*sid).unwrap().objects[0].item.id.0).collect();
        assert!(tables.is_sorted(), "the copied tables take ids in story order: {tables:?}");
        assert!(objects.is_sorted(), "the copied anchored objects take ids in story order: {objects:?}");
    }

    /// 40 stories, each holding two anchored text frames threaded in the next: 2^40 paths, 40 stories.
    #[test]
    fn duplicate_layer_checks_each_shared_story_once() {
        let mut s = crate::Session::new();
        s.execute("file.new", &json!({})).unwrap();
        let base = s.doc().unwrap().doc.layers[0].id.0;
        let f = s.execute("frame.create", &json!({"rect": [72, 72, 400, 400], "content": "text", "text": "Top"})).unwrap()["id"].as_u64().unwrap();
        let top = s.doc().unwrap().doc.item(designcraft_doc::ItemId(f)).unwrap().text_frame().unwrap().story;
        s.edit(|d, _| {
            use designcraft_doc::{AnchoredObject, ParaFormat};
            let r = designcraft_geom::Rect::new(0.0, 0.0, 60.0, 30.0);
            let mut outer = top;
            for _ in 0..40 {
                let (a, inner) = d.add_text_frame(designcraft_doc::SpreadRef::Doc(0), r, d.default_layer(), "Link", ParaFormat::default())?;
                let (b, spare) = d.add_text_frame(designcraft_doc::SpreadRef::Doc(0), r, d.default_layer(), "", ParaFormat::default())?;
                let mut items = Vec::new();
                for f in [a, b] {
                    let mut it = d.item(f).cloned().ok_or(designcraft_doc::DocError::NoItem(f))?;
                    if let designcraft_doc::Content::Text(tf) = &mut it.content {
                        tf.story = inner;
                    }
                    items.push(it);
                }
                d.stories.remove(&spare);
                d.story_mut(inner).ok_or(designcraft_doc::DocError::NoStory(inner))?.frames = vec![a, b];
                std::sync::Arc::make_mut(&mut d.spreads[0]).items.retain(|i| i.id != a && i.id != b);
                let st = d.story_mut(outer).ok_or(designcraft_doc::DocError::NoStory(outer))?;
                for it in items.into_iter().rev() {
                    st.insert_object(0, AnchoredObject::new(it, Default::default()));
                }
                outer = inner;
            }
            Ok(())
        })
        .unwrap();
        s.doc().unwrap().doc.check().unwrap();
        let before = s.doc().unwrap().doc.stories.len();
        s.execute("layer.duplicate", &json!({"id": base})).unwrap();
        let d = &s.doc().unwrap().doc;
        d.check().unwrap();
        assert_eq!(d.stories.len(), 2 * before, "one copy of each story");
    }

    #[test]
    fn duplicate_layer_refuses_objects_nested_too_deeply() {
        let mut s = crate::Session::new();
        s.execute("file.new", &json!({})).unwrap();
        let base = s.doc().unwrap().doc.layers[0].id.0;
        let mut g = s.execute("frame.create", &json!({"rect": [0, 0, 10, 10]})).unwrap()["id"].as_u64().unwrap();
        for _ in 0..super::MAX_COPY_DEPTH + 1 {
            let f = s.execute("frame.create", &json!({"rect": [0, 0, 10, 10]})).unwrap()["id"].as_u64().unwrap();
            g = s.execute("object.group", &json!({"ids": [g, f]})).unwrap()["id"].as_u64().unwrap();
        }
        let before = s.doc().unwrap().doc.layers.clone();
        assert!(s.execute("layer.duplicate", &json!({"id": base})).is_err());
        assert_eq!(s.doc().unwrap().doc.layers, before, "nothing changes");
    }

    #[test]
    fn duplicate_layer_detaches_overrides_of_parent_items_on_other_layers() {
        let mut s = crate::Session::new();
        s.execute("file.new", &json!({"pages": 2, "facingPages": false})).unwrap();
        let base = s.doc().unwrap().doc.layers[0].id.0;
        s.execute("layer.new", &json!({"name": "Art"})).unwrap();
        let logo = json!({"spread": {"kind": "parent", "index": 0}, "rect": [72, 72, 144, 144]});
        let parent = s.execute("frame.create", &logo).unwrap()["id"].as_u64().unwrap();
        let o = s.execute("layout.overrideParentItems", &json!({"page": 1})).unwrap()["ids"][0].as_u64().unwrap();
        s.execute("selection.set", &json!({"ids": [o]})).unwrap();
        s.execute("object.setLayer", &json!({"layer": base})).unwrap();
        let copy = s.execute("layer.duplicate", &json!({"id": base})).unwrap()["id"].as_u64().unwrap();
        let d = &s.doc().unwrap().doc;
        let copies = on_layer(d, copy);
        assert_eq!(copies.len(), 1, "the override only: the parent item is on Art");
        assert_eq!(copies[0].overrides, None, "a plain object, so the original override stays the only one");
        assert_eq!(d.item(designcraft_doc::ItemId(o)).unwrap().overrides, Some(designcraft_doc::ItemId(parent)));
        let (si, pi) = d.page_loc(1).unwrap();
        assert_eq!(d.spreads[si].pages[pi].overridden, [designcraft_doc::ItemId(parent)]);
    }
}

#[cfg(test)]
mod layer_options_tests {
    use serde_json::json;

    #[test]
    fn layer_set_sets_every_field_and_rejects_bad_names() {
        let mut s = crate::Session::new();
        s.execute("file.new", &json!({})).unwrap();
        let first = s.doc().unwrap().doc.layers[0].id.0;
        let id = s.execute("layer.new", &json!({"name": "Art"})).unwrap()["id"].as_u64().unwrap();
        let all = json!({"id": id, "name": "  Artwork ", "color": "Gold", "visible": false, "locked": true, "printable": false,
            "showGuides": false, "suppressWrapWhenHidden": true});
        s.execute("layer.set", &all).unwrap();
        let l = s.doc().unwrap().doc.layer(designcraft_doc::LayerId(id)).cloned().unwrap();
        assert_eq!(l.name, "Artwork");
        assert_eq!(l.color, [255, 153, 0]);
        assert!(!l.visible && l.locked && !l.printable && !l.show_guides && l.suppress_wrap_when_hidden);
        s.execute("edit.undo", &json!({})).unwrap();
        assert_eq!(s.doc().unwrap().doc.layer(designcraft_doc::LayerId(id)).unwrap().name, "Art", "one undo step");
        let first_name = s.doc().unwrap().doc.layers.iter().find(|l| l.id.0 == first).unwrap().name.clone();
        let before = s.doc().unwrap().doc.layers.clone();
        for p in [
            json!({"id": id, "name": ""}),
            json!({"id": id, "name": "   "}),
            json!({"id": id, "name": first_name}),
            json!({"id": id, "name": "Fine", "color": "Plaid"}),
            json!({"id": id, "color": [300, 0, 0]}),
            json!({"id": id, "visible": "yes"}),
            json!({"id": 999_999, "name": "Ghost"}),
        ] {
            assert!(s.execute("layer.set", &p).is_err(), "{p}");
        }
        assert_eq!(s.doc().unwrap().doc.layers, before);
        // Keeping its own name is fine.
        s.execute("layer.set", &json!({"id": id, "name": "Art", "color": [1, 2, 3]})).unwrap();
    }
}
