//! Placed files (assets): one asset per distinct file, the assets a document uses, and the
//! compaction a save applies (duplicates merged, unused assets left out).
//!
//! An asset holds what is shared by every placement of a file (bytes, link, type, the PDF page
//! shown); each placement's frame data (transform, crop, fitting) lives on its [`Graphic`].

use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::sync::Arc;

use crate::story::Story;
use crate::{Asset, AssetId, Content, Document, Item};

/// Nesting limit for groups, anchored objects, table cells and notes. A document nested deeper
/// is treated as using every asset, so nothing it references is merged away or pruned.
const MAX_DEPTH: usize = 64;

impl Asset {
    /// Whether `other` holds the same file: the same type, page and bytes. Assets without bytes
    /// (a link that couldn't be read) are the same only when they link to the same path.
    pub fn same_file(&self, other: &Asset) -> bool {
        if self.mime != other.mime || self.page != other.page {
            return false;
        }
        match (self.data.is_empty(), other.data.is_empty()) {
            (false, false) => Arc::ptr_eq(&self.data, &other.data) || (self.data.len() == other.data.len() && *self.data == *other.data),
            (true, true) => self.link.is_some() && self.link == other.link,
            _ => false,
        }
    }
}

/// The asset in `assets` holding the same file as `asset`.
pub fn find_same_asset(assets: &BTreeMap<AssetId, Arc<Asset>>, asset: &Asset) -> Option<AssetId> {
    assets.values().find(|a| a.same_file(asset)).map(|a| a.id)
}

/// Add `asset` to `assets` unless they already hold the same file; returns the id placements
/// should reference. A reused asset without a link takes the new asset's link.
pub fn insert_asset(assets: &mut BTreeMap<AssetId, Arc<Asset>>, asset: Asset) -> AssetId {
    if let Some(id) = find_same_asset(assets, &asset) {
        if let Some(a) = assets.get_mut(&id)
            && a.link.is_none()
            && asset.link.is_some()
        {
            Arc::make_mut(a).link = asset.link;
        }
        return id;
    }
    let id = asset.id;
    assets.insert(id, Arc::new(asset));
    id
}

/// Calls `f` with every asset reference in `it` and its children; false when nesting exceeded
/// [`MAX_DEPTH`] (some references weren't visited).
fn item_refs(it: &Item, f: &mut impl FnMut(AssetId), depth: usize) -> bool {
    if depth > MAX_DEPTH {
        return false;
    }
    if let Content::Graphic(g) = &it.content {
        f(g.asset);
    }
    if let Some(p) = it.media.as_ref().and_then(|m| m.poster) {
        f(p);
    }
    it.children().iter().all(|c| item_refs(c, f, depth + 1))
}

/// Asset references in a story's anchored objects, graphic cells and notes (recursively).
fn story_refs(st: &Story, f: &mut impl FnMut(AssetId), depth: usize) -> bool {
    if depth > MAX_DEPTH {
        return false;
    }
    let mut complete = st.objects.iter().all(|o| item_refs(&o.item, f, depth + 1));
    for t in st.tables.values() {
        for c in &t.cells {
            if let Some(g) = &c.graphic {
                f(g.asset);
            }
            complete &= story_refs(&c.text, f, depth + 1);
        }
    }
    for n in st.notes.iter().chain(&st.endnotes) {
        complete &= story_refs(&n.text, f, depth + 1);
    }
    complete
}

/// Every asset reference in a story (its anchored objects, graphic cells and notes).
pub fn story_asset_refs(st: &Story) -> Vec<AssetId> {
    let mut out = Vec::new();
    story_refs(st, &mut |a| out.push(a), 0);
    out
}

/// Point a story's references to a key of `map` at its value.
pub fn remap_story_assets(st: &mut Story, map: &HashMap<AssetId, AssetId>) {
    if !map.is_empty() {
        remap_story(st, map, 0);
    }
}

fn item_uses(it: &Item, map: &HashMap<AssetId, AssetId>) -> bool {
    let mut hit = false;
    item_refs(it, &mut |a| hit |= map.contains_key(&a), 0);
    hit
}

fn story_uses(st: &Story, map: &HashMap<AssetId, AssetId>) -> bool {
    let mut hit = false;
    story_refs(st, &mut |a| hit |= map.contains_key(&a), 0);
    hit
}

fn remap_id(id: &mut AssetId, map: &HashMap<AssetId, AssetId>) {
    if let Some(n) = map.get(id) {
        *id = *n;
    }
}

/// Rewrites the references in `it`, copying only the shared items that change.
fn remap_item(it: &mut Arc<Item>, map: &HashMap<AssetId, AssetId>, depth: usize) {
    if depth > MAX_DEPTH || !item_uses(it, map) {
        return;
    }
    remap_item_in(Arc::make_mut(it), map, depth);
}

fn remap_item_in(it: &mut Item, map: &HashMap<AssetId, AssetId>, depth: usize) {
    if let Content::Graphic(g) = &mut it.content {
        remap_id(&mut g.asset, map);
    }
    if let Some(p) = it.media.as_mut().and_then(|m| m.poster.as_mut()) {
        remap_id(p, map);
    }
    if let Some(kids) = it.children_mut() {
        for k in kids {
            remap_item(k, map, depth + 1);
        }
    }
}

fn remap_story(st: &mut Story, map: &HashMap<AssetId, AssetId>, depth: usize) {
    if depth > MAX_DEPTH {
        return;
    }
    for o in &mut st.objects {
        if item_uses(&o.item, map) {
            remap_item_in(&mut Arc::make_mut(o).item, map, depth + 1);
        }
    }
    for t in st.tables.values_mut() {
        if !t.cells.iter().any(|c| c.graphic.as_ref().is_some_and(|g| map.contains_key(&g.asset)) || story_uses(&c.text, map)) {
            continue;
        }
        for c in &mut Arc::make_mut(t).cells {
            if let Some(g) = &mut c.graphic {
                remap_id(&mut g.asset, map);
            }
            remap_story(&mut c.text, map, depth + 1);
        }
    }
    for n in st.notes.iter_mut().chain(st.endnotes.iter_mut()) {
        if story_uses(&n.text, map) {
            remap_story(&mut Arc::make_mut(n).text, map, depth + 1);
        }
    }
}

impl Document {
    /// Add a placed file, reusing the asset of an identical file already in the document;
    /// returns the id to reference.
    pub fn add_asset(&mut self, asset: Asset) -> AssetId {
        insert_asset(&mut self.assets, asset)
    }

    /// Copy asset `id` of `src` into this document (or reuse the identical asset it already
    /// has); returns the id to reference here, `None` when `src` has no such asset.
    pub fn adopt_asset(&mut self, src: &Document, id: AssetId) -> Option<AssetId> {
        let a = src.assets.get(&id)?;
        if let Some(same) = find_same_asset(&self.assets, a) {
            return Some(same);
        }
        let fresh = AssetId(self.alloc());
        Some(self.add_asset(Asset { id: fresh, ..(**a).clone() }))
    }

    /// Calls `f` with every asset reference: graphics on document and parent spreads (in
    /// groups and multi-state objects too), media posters, anchored and inline objects, graphic
    /// table cells and notes. False when nesting exceeded the depth limit.
    fn asset_refs(&self, f: &mut impl FnMut(AssetId)) -> bool {
        let mut complete = true;
        for sp in self.spreads.iter().chain(&self.parents) {
            for it in &sp.items {
                complete &= item_refs(it, f, 0);
            }
        }
        for st in self.stories.values() {
            complete &= story_refs(st, f, 0);
        }
        complete
    }

    /// The assets something in the document references. When the document nests deeper than
    /// the traversal limit, every asset counts as used.
    pub fn used_assets(&self) -> BTreeSet<AssetId> {
        let mut used = BTreeSet::new();
        if !self.asset_refs(&mut |a| {
            used.insert(a);
        }) {
            return self.assets.keys().copied().collect();
        }
        used
    }

    /// Point every reference to a key of `map` at its value.
    pub fn remap_assets(&mut self, map: &HashMap<AssetId, AssetId>) {
        if map.is_empty() {
            return;
        }
        for sp in self.spreads.iter_mut().chain(self.parents.iter_mut()) {
            if sp.items.iter().any(|it| item_uses(it, map)) {
                for it in &mut Arc::make_mut(sp).items {
                    remap_item(it, map, 0);
                }
            }
        }
        for st in self.stories.values_mut() {
            if story_uses(st, map) {
                remap_story(Arc::make_mut(st), map, 0);
            }
        }
    }

    /// Merge assets holding the same file into the first of them (lowest id), rewriting the
    /// references; returns how many assets were removed.
    pub fn merge_duplicate_assets(&mut self) -> usize {
        if !self.asset_refs(&mut |_| {}) {
            return 0;
        }
        let mut reps: HashMap<(usize, &str, u32), Vec<&Arc<Asset>>> = HashMap::new();
        let mut map = HashMap::new();
        let mut links = Vec::new();
        for a in self.assets.values() {
            let bucket = reps.entry((a.data.len(), a.mime.as_str(), a.page)).or_default();
            match bucket.iter().find(|r| r.same_file(a)) {
                Some(r) => {
                    map.insert(a.id, r.id);
                    if r.link.is_none()
                        && let Some(l) = &a.link
                    {
                        links.push((r.id, l.clone()));
                    }
                }
                None => bucket.push(a),
            }
        }
        for (id, link) in links {
            if let Some(a) = self.assets.get_mut(&id)
                && a.link.is_none()
            {
                Arc::make_mut(a).link = Some(link);
            }
        }
        self.remap_assets(&map);
        self.assets.retain(|id, _| !map.contains_key(id));
        map.len()
    }

    /// Merge duplicate assets and drop the ones nothing references (what a saved file holds).
    pub fn compact_assets(&mut self) {
        self.merge_duplicate_assets();
        let used = self.used_assets();
        self.assets.retain(|id, _| used.contains(id));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Graphic, ItemId, LayerId, Shape, StoryId};
    use designcraft_geom::{Affine, Rect, shapes};

    fn asset(id: u64, data: &[u8]) -> Asset {
        Asset { id: AssetId(id), name: "p.png".into(), mime: "image/png".into(), link: None, data: Arc::new(data.to_vec()), pixels: None, page: 0 }
    }

    fn graphic_item(id: u64, asset: u64, crop: f64) -> Item {
        let mut it = Item::new(ItemId(id), LayerId(1), Shape::Rectangle, shapes::rectangle(Rect::new(0.0, 0.0, 10.0, 10.0)));
        it.content = Content::Graphic(Graphic {
            asset: AssetId(asset),
            size: (10.0, 10.0),
            xf: Affine::translate((crop, 0.0)),
            auto_fit: Default::default(),
            fit_align: 4,
            crop: [crop; 4],
        });
        it
    }

    #[test]
    fn identical_files_share_one_asset() {
        let mut d = Document::new(&Default::default());
        let a = d.add_asset(asset(1, b"same"));
        let b = d.add_asset(Asset { link: Some("/x/p.png".into()), ..asset(2, b"same") });
        let c = d.add_asset(asset(3, b"diff"));
        let p = d.add_asset(Asset { page: 1, ..asset(4, b"same") });
        assert_eq!(a, b);
        assert_ne!(a, c);
        assert_ne!(a, p);
        assert_eq!(d.assets.len(), 3);
        assert_eq!(d.assets[&a].link.as_deref(), Some("/x/p.png"));
        // Unreadable links match by path only.
        let m1 = d.add_asset(Asset { link: Some("/m/1.png".into()), ..asset(5, b"") });
        let m2 = d.add_asset(Asset { link: Some("/m/2.png".into()), ..asset(6, b"") });
        let m3 = d.add_asset(asset(7, b""));
        let m4 = d.add_asset(asset(8, b""));
        assert_ne!(m1, m2);
        assert_ne!(m3, m4);
    }

    #[test]
    fn compaction_merges_duplicates_and_drops_unused_assets() {
        let mut d = Document::new(&Default::default());
        for (id, data) in [(1, b"one"), (2, b"one"), (3, b"two"), (4, b"old")] {
            d.assets.insert(AssetId(id), Arc::new(asset(id, data)));
        }
        let sp = Arc::make_mut(d.spreads.first_mut().unwrap());
        sp.items.push(Arc::new(graphic_item(10, 1, 1.0)));
        sp.items.push(Arc::new(graphic_item(11, 2, 2.0)));
        // A graphic cell and an anchored object in a story.
        let mut st = Story::new(StoryId(20));
        st.objects.push(Arc::new(crate::AnchoredObject::new(graphic_item(12, 3, 3.0), Default::default())));
        d.stories.insert(StoryId(20), Arc::new(st));
        assert_eq!(d.used_assets(), [1, 2, 3].map(AssetId).into_iter().collect());

        d.compact_assets();
        assert_eq!(d.assets.keys().copied().collect::<Vec<_>>(), vec![AssetId(1), AssetId(3)]);
        let crops: Vec<(AssetId, f64)> = d.spreads[0].items.iter().filter_map(|i| i.graphic().map(|g| (g.asset, g.crop[0]))).collect();
        assert_eq!(crops, vec![(AssetId(1), 1.0), (AssetId(1), 2.0)]);
    }
}
