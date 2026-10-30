//! Placed files (assets): one asset per distinct file, the assets a document uses, and the
//! compaction a save applies (duplicates merged, unused assets left out).
//!
//! An asset holds what is shared by every placement of a file (bytes, link, type, the PDF page
//! and crop shown); each placement's frame data (transform, crop, fitting) lives on its [`Graphic`].

use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::hash::BuildHasher;
use std::sync::Arc;

use crate::story::Story;
use crate::{Asset, AssetId, Content, Document, Item};

/// Nesting limit for groups, anchored objects, table cells and notes. A document nested deeper
/// is treated as using every asset, so nothing it references is merged away or pruned.
const MAX_DEPTH: usize = 64;

fn item_layer_options(it: &Item, depth: usize, overrides_only: bool) -> bool {
    depth > MAX_DEPTH
        || it.graphic().is_some_and(|g| g.layer_options.as_ref().is_some_and(|o| !overrides_only || o.has_visibility_overrides()))
        || it.children().iter().any(|child| item_layer_options(child, depth + 1, overrides_only))
}

fn story_layer_options(st: &Story, depth: usize, overrides_only: bool) -> bool {
    depth > MAX_DEPTH
        || st.objects.iter().any(|object| item_layer_options(&object.item, depth + 1, overrides_only))
        || st.tables.values().flat_map(|table| &table.cells).any(|cell| {
            cell.graphic.as_ref().is_some_and(|g| g.layer_options.as_ref().is_some_and(|o| !overrides_only || o.has_visibility_overrides()))
                || story_layer_options(&cell.text, depth + 1, overrides_only)
        })
        || st.notes.iter().chain(&st.endnotes).any(|note| story_layer_options(&note.text, depth + 1, overrides_only))
}

fn item_any(it: &Item, depth: usize, f: &impl Fn(&Item) -> bool) -> bool {
    depth > MAX_DEPTH || f(it) || it.children().iter().any(|child| item_any(child, depth + 1, f))
}

fn story_any(st: &Story, depth: usize, f: &impl Fn(&Item) -> bool) -> bool {
    depth > MAX_DEPTH
        || st.objects.iter().any(|object| item_any(&object.item, depth + 1, f))
        || st.tables.values().flat_map(|table| &table.cells).any(|cell| story_any(&cell.text, depth + 1, f))
        || st.notes.iter().chain(&st.endnotes).any(|note| story_any(&note.text, depth + 1, f))
}

impl Document {
    /// Whether saving needs a reader that retains per-placement graphic layer settings.
    /// Exceeding the traversal budget conservatively requires the newer format.
    pub fn has_graphic_layer_options(&self) -> bool {
        self.spreads.iter().chain(&self.parents).flat_map(|spread| &spread.items).any(|item| item_layer_options(item, 0, false))
            || self.stories.values().any(|story| story_layer_options(story, 0, false))
    }

    /// Whether any placed PDF, on document or parent spreads, in groups or anchored, explicitly
    /// shows every layer (an override with an empty hidden list). Older readers would take that
    /// for "use the file's defaults". Exceeding the traversal budget conservatively says yes.
    pub fn has_explicit_all_on_pdf_layers(&self) -> bool {
        let all_on = |it: &Item| it.pdf_layer_override && it.pdf_hidden_layers.is_empty();
        self.spreads.iter().chain(&self.parents).flat_map(|spread| &spread.items).any(|item| item_any(item, 0, &all_on))
            || self.stories.values().any(|story| story_any(story, 0, &all_on))
    }

    /// Whether any placed PDF, on document or parent spreads, in groups or anchored, has an
    /// Object Layer Options choice (hidden layers or explicit all-on). Exceeding the traversal
    /// budget conservatively says yes.
    pub fn has_pdf_layer_overrides(&self) -> bool {
        let chosen = |it: &Item| it.has_pdf_layer_override();
        self.spreads.iter().chain(&self.parents).flat_map(|spread| &spread.items).any(|item| item_any(item, 0, &chosen))
            || self.stories.values().any(|story| story_any(story, 0, &chosen))
    }

    /// Placements outside the ordinary document-spread frame walk used by preflight.
    pub fn has_unwalked_graphic_layer_visibility_overrides(&self) -> bool {
        self.parents.iter().flat_map(|spread| &spread.items).any(|item| item_layer_options(item, 0, true))
            || self.stories.values().any(|story| story_layer_options(story, 0, true))
    }

    /// Includes parent items, anchored objects, graphic cells and nested notes.
    pub fn has_graphic_layer_visibility_overrides(&self) -> bool {
        self.spreads.iter().chain(&self.parents).flat_map(|spread| &spread.items).any(|item| item_layer_options(item, 0, true))
            || self.stories.values().any(|story| story_layer_options(story, 0, true))
    }
}

impl Asset {
    /// Whether `other` holds the same file: the same type, PDF page and crop, link and bytes. Files at
    /// different paths stay apart even with identical bytes, since each can be updated or
    /// relinked on its own. Assets without bytes (a link that couldn't be read) are the same only
    /// when they link to the same path.
    pub fn same_file(&self, other: &Asset) -> bool {
        if self.mime != other.mime || self.page != other.page || self.pdf_crop != other.pdf_crop || self.pdf_box != other.pdf_box {
            return false;
        }
        if self.link != other.link {
            return false;
        }
        match (self.data.is_empty(), other.data.is_empty()) {
            (false, false) => self.same_bytes(other),
            (true, true) => self.link.is_some(),
            _ => false,
        }
    }

    fn same_bytes(&self, other: &Asset) -> bool {
        Arc::ptr_eq(&self.data, &other.data) || (self.data.len() == other.data.len() && *self.data == *other.data)
    }
}

/// The asset in `assets` holding the same file as `asset`.
pub fn find_same_asset(assets: &BTreeMap<AssetId, Arc<Asset>>, asset: &Asset) -> Option<AssetId> {
    assets.values().find(|a| a.same_file(asset)).map(|a| a.id)
}

/// Add `asset` to `assets` unless they already hold the same file; returns the id placements
/// should reference. A new asset with the bytes of an existing one shares its buffer.
pub fn insert_asset(assets: &mut BTreeMap<AssetId, Arc<Asset>>, mut asset: Asset) -> AssetId {
    if let Some(id) = find_same_asset(assets, &asset) {
        return id;
    }
    if !asset.data.is_empty()
        && let Some(a) = assets.values().find(|a| a.same_bytes(&asset))
    {
        asset.data = Arc::clone(&a.data);
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
        let fingerprint = std::collections::hash_map::RandomState::new();
        let mut hashes = HashMap::new();
        let mut reps: HashMap<_, Vec<&Arc<Asset>>> = HashMap::new();
        let mut map = HashMap::new();
        for a in self.assets.values() {
            // Empty, unlinked assets never compare equal. Don't build a quadratic bucket
            // of them, and hash a shared immutable buffer only once per compaction.
            if a.data.is_empty() && a.link.is_none() {
                continue;
            }
            let hash = *hashes.entry(Arc::as_ptr(&a.data)).or_insert_with(|| fingerprint.hash_one(a.data.as_slice()));
            let bucket = reps.entry((hash, a.mime.as_str(), a.page, a.link.as_deref())).or_default();
            // A fingerprint only selects candidates; equality still checks the actual file.
            match bucket.iter().find(|r| r.same_file(a)) {
                Some(r) => {
                    map.insert(a.id, r.id);
                }
                None => bucket.push(a),
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
        Asset { id: AssetId(id), name: "p.png".into(), mime: "image/png".into(), data: Arc::new(data.to_vec()), ..Default::default() }
    }

    #[test]
    fn equal_size_assets_keep_distinct_contents_and_merge_exact_copies() {
        let mut d = Document::new(&Default::default());
        for id in 1..=256u64 {
            let mut bytes = vec![42; 1024];
            bytes[1016..].copy_from_slice(&id.to_le_bytes());
            d.assets.insert(AssetId(id), Arc::new(asset(id, &bytes)));
            d.assets.insert(AssetId(id + 256), Arc::new(asset(id + 256, &bytes)));
        }
        assert_eq!(d.merge_duplicate_assets(), 256);
        assert_eq!(d.assets.len(), 256);
        for id in 1..=256u64 {
            assert_eq!(d.assets[&AssetId(id)].data[1016..], id.to_le_bytes());
        }
    }

    #[test]
    fn compaction_preserves_asset_identity_and_missing_data_rules() {
        let mut d = Document::new(&Default::default());
        let mut cases = vec![asset(1, b"same"), asset(2, b"same"), asset(3, b"same"), asset(4, b"same")];
        cases[1].mime = "application/pdf".into();
        cases[2].page = 1;
        cases[3].link = Some("/linked.png".into());
        cases.extend([asset(5, b""), asset(6, b""), asset(7, b""), asset(8, b"")]);
        cases[6].link = Some("/missing.png".into());
        cases[7].link = Some("/missing.png".into());
        for a in cases {
            d.assets.insert(a.id, Arc::new(a));
        }
        assert_eq!(d.merge_duplicate_assets(), 1);
        assert_eq!(d.assets.keys().copied().collect::<Vec<_>>(), (1..=7).map(AssetId).collect::<Vec<_>>());
    }

    /// Two crops of one PDF page are two assets: merging them would show one crop in both frames.
    #[test]
    fn pdf_assets_with_different_crops_stay_apart() {
        let mut d = Document::new(&Default::default());
        let pdf = |id: u64, pdf_crop: crate::PdfCrop| Asset { mime: "application/pdf".into(), pdf_crop, ..asset(id, b"%PDF-") };
        let crop = d.add_asset(pdf(1, crate::PdfCrop::Crop));
        let trim = d.add_asset(pdf(2, crate::PdfCrop::Trim));
        assert_ne!(crop, trim);
        assert_eq!(d.add_asset(pdf(3, crate::PdfCrop::Trim)), trim);
        assert_eq!(d.merge_duplicate_assets(), 0);
        assert_eq!(d.assets.len(), 2);
    }

    #[test]
    fn unresolved_pdf_assets_with_different_visible_boxes_stay_apart() {
        let mut doc = Document::new(&Default::default());
        let pdf = |id, pdf_box| Asset {
            mime: "application/pdf".into(),
            data: Arc::new(Vec::new()),
            link: Some("Links/page.pdf".into()),
            pdf_box,
            ..asset(id, b"")
        };
        let first = doc.add_asset(pdf(1, Some([0.0, 0.0, 0.5, 1.0])));
        let second = doc.add_asset(pdf(2, Some([0.5, 0.0, 1.0, 1.0])));
        assert_ne!(first, second);
        assert_eq!(doc.add_asset(pdf(3, Some([0.0, 0.0, 0.5, 1.0]))), first);
        assert_eq!(doc.merge_duplicate_assets(), 0);
        assert_eq!(doc.assets.len(), 2);
    }

    #[test]
    #[cfg(not(target_arch = "wasm32"))]
    #[ignore = "manual bounded timing comparison; no timing assertion"]
    fn equal_size_assets_timing() {
        for count in [500, 1000, 2000] {
            let mut d = Document::new(&Default::default());
            for id in 1..=count as u64 {
                let mut bytes = vec![42; 8192];
                bytes[8184..].copy_from_slice(&id.to_le_bytes());
                d.assets.insert(AssetId(id), Arc::new(asset(id, &bytes)));
            }
            let start = std::time::Instant::now();
            assert_eq!(d.merge_duplicate_assets(), 0);
            eprintln!("{count} distinct 8 KiB assets: {:?}", start.elapsed());
        }
    }

    fn graphic_item(id: u64, asset: u64, crop: f64) -> Item {
        let mut it = Item::new(ItemId(id), LayerId(1), Shape::Rectangle, shapes::rectangle(Rect::new(0.0, 0.0, 10.0, 10.0)));
        it.content = Content::Graphic(Graphic {
            layer_options: None,
            asset: AssetId(asset),
            size: (10.0, 10.0),
            xf: Affine::translate((crop, 0.0)),
            clip: None,
            auto_fit: Default::default(),
            fit_align: 4,
            crop: [crop; 4],
            wrap: Default::default(),
            fill: Default::default(),
        });
        it
    }

    #[test]
    fn identical_files_share_one_asset() {
        let mut d = Document::new(&Default::default());
        let a = d.add_asset(asset(1, b"same"));
        let b = d.add_asset(asset(2, b"same"));
        let c = d.add_asset(asset(3, b"diff"));
        let p = d.add_asset(Asset { page: 1, ..asset(4, b"same") });
        // One PDF page shown with two crop boxes: two assets (the box is the asset's).
        let pdf = |id, pdf_crop| Asset { mime: "application/pdf".into(), pdf_crop, ..asset(id, b"%PDF") };
        let crop = d.add_asset(pdf(9, crate::PdfCrop::Crop));
        let art = d.add_asset(pdf(10, crate::PdfCrop::Art));
        assert_ne!(crop, art);
        assert_eq!(d.add_asset(pdf(11, crate::PdfCrop::Art)), art);
        d.assets.retain(|id, _| ![crop, art].contains(id));
        assert_eq!(a, b);
        assert_ne!(a, c);
        assert_ne!(a, p);
        assert_eq!(d.assets.len(), 3);
        // Unreadable links match by path only.
        let m1 = d.add_asset(Asset { link: Some("/m/1.png".into()), ..asset(5, b"") });
        let m2 = d.add_asset(Asset { link: Some("/m/2.png".into()), ..asset(6, b"") });
        let m3 = d.add_asset(asset(7, b""));
        let m4 = d.add_asset(asset(8, b""));
        assert_ne!(m1, m2);
        assert_ne!(m3, m4);
    }

    #[test]
    fn files_at_different_paths_stay_separate_assets() {
        let mut d = Document::new(&Default::default());
        let linked = |id, path: &str| Asset { link: Some(path.into()), ..asset(id, b"same") };
        let embedded = d.add_asset(asset(1, b"same"));
        let a = d.add_asset(linked(2, "/p/a.png"));
        let b = d.add_asset(linked(3, "/q/b.png"));
        assert_eq!(d.add_asset(linked(4, "/p/a.png")), a, "the same path placed again");
        assert_eq!(d.assets.len(), 3);
        assert!(embedded != a && a != b && embedded != b);
        assert_eq!(d.assets[&embedded].link, None);
        assert_eq!(d.assets[&b].link.as_deref(), Some("/q/b.png"));
        assert!(Arc::ptr_eq(&d.assets[&a].data, &d.assets[&b].data), "identical bytes share one buffer");
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
