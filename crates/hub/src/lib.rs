//! # designcraft-hub — DesignCraft's CraftHub client path
//!
//! The [`HubAdapter`] that maps DesignCraft's [`Document`](designcraft_doc::Document) onto
//! `craft-collab`'s generalized object/property model (the CraftHub specification §3.6 phase B,
//! §18.4 row 4 — the pages/layout app of the family). Nothing here is a collaboration core:
//! every tested semantic — the object/property table, the fractional sibling keys, one batch
//! per user action, the server-ordered reconciliation, per-user undo — lives in the shared
//! core, and this crate is the app-side binding that speaks it.
//!
//! What belongs here, per the family rule (§3.6: "adapters for an app ship with the app"):
//!
//! * [`props`] — the synchronized property tables for the three object types ([`Item`], [`Page`],
//!   [`Spread`]), with the compile-time guard: a struct gaining a field that doesn't sync is a
//!   build error. The `Item::content` property syncs with the group payload **elided** — the
//!   children ride as their own objects, so an edit deep inside a group never re-serializes the
//!   subtree.
//! * [`DesigncraftHub`] — the adapter: `view`/`apply`/`diff` over the real document, the sibling
//!   position keys (kept beside the document, exactly where a replica would keep them), and the
//!   presence projection.
//! * [`commands`] — the unified `hub.*` command catalog (same names in every Craft app, §18.3)
//!   with the `collab.*` compatibility mapping.
//! * [`deeplink`] — `designcraft://join?...` links (§18.5).
//! * [`schema`] — the document knowledge the hub server validates batches against: which kinds
//!   exist, which parents accept children, the per-entry document lists and their orders, and
//!   the honest recompute answer (DesignCraft's layout passes are explicit commands today).
//! * [`locks`] — the `PageRange` region-lock shape (§6.7: locks sync as document properties;
//!   DesignCraft is the adoption table's page-range row).
//!
//! ## The tree, mapped honestly
//!
//! DesignCraft's model is spread-shaped, not frame-shaped: a document holds **spreads**
//! (document spreads and parent — "master" — spreads), each spread holds **pages** and
//! top-level **items** in z-order, and items nest through `Content::Group` (groups, and frames
//! with pasted-in content). The generic tree maps it directly:
//!
//! * the file root's children are the spreads — `spread` and `parentSpread` kinds, the two
//!   real lists;
//! * a spread's children are its pages (the `page` kind) and its top-level items;
//! * an item's children are its group contents.
//!
//! Items hang off the *spread*, not the page: the model's page-item association is geometric
//! (an item's centre sits on a page), not structural, and the page commands move items with
//! their page by translating `xf` — which the diff sees as a plain field write. Repagination
//! rebuilds spreads with fresh ids; the diff treats that honestly as what it is — spread
//! deletes and creates plus parent moves for the pages and items that survived.
//!
//! Stories (the threaded text containers) sync as document-level **entries**, one per story,
//! last-writer-wins per entry — the v2 decision for big values, kept verbatim from the pilot.
//! The character-level (`textcrdt`) upgrade is WordCraft's row of the table; when DesignCraft
//! opts a story into it, the entry splits without a wire break.
//!
//! ## What does not sync, on purpose
//!
//! * **Assets** (placed images, PDFs): the ASSET channel is content-addressed and lands with
//!   the photocraft row (§18.4 row 5); a peer without the asset sees the missing link, exactly
//!   like a broken Links panel entry.
//! * **Composition** (line breaking, overset): derived data in the render-side compose cache,
//!   keyed by each story's `rev` — recomputed per client, never journalled.
//! * `created`/`modified`/`font_scope`: local metadata, not shared state.
//!
//! What deliberately does **not** live here at all: networking, sessions, the UI. The session
//! wiring (a v3 `NetLink` behind the app's `crafthub` feature flag) lands as the follow-up
//! once this contract is reviewed; nothing here changes any user-visible behavior.

#![deny(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::unimplemented, clippy::todo, clippy::unreachable)]
#![forbid(unsafe_code)]

pub mod commands;
pub mod deeplink;
pub mod locks;
pub mod presence;
pub mod props;
pub mod schema;

use std::collections::BTreeMap;
use std::sync::{Arc, Mutex};

use craft_collab::adapter::{ApplyReport, DocumentView, HubAdapter, UiState, ViewNode};
use craft_collab::change::Change;
use craft_collab::frac;
use craft_collab::ids::NodeId as HubId;
use craft_collab::value::Value;
use designcraft_doc::geom::PathData;
use designcraft_doc::{Columns, Content, Document, Item, ItemId, Margins, Page, PageId, PageSide, ParentInfo, Shape, Spread, SpreadId, SpreadRef};

/// The root every spread hangs under in the generic tree (space 0, counter 0 — never a real id:
/// DesignCraft's ids count up from 1, and `0` is the core's root marker).
pub const ROOT: HubId = HubId(0);

/// The object kinds (the real discriminants of the model).
pub const KINDS: &[&str] = &["spread", "parentSpread", "page", "textFrame", "graphic", "group", "rectangle", "ellipse", "polygon", "line", "path"];

/// The kinds that accept children: spreads take pages and items; groups take items. A page
/// accepts none — items live in spread space (the association is geometric, not structural).
pub const CONTAINERS: &[&str] = &["spread", "parentSpread", "group"];

/// The adapter: document binding, sibling keys, the region-lock list and the presence
/// projection in one value.
///
/// The sibling position keys are adapter state (spec §2.3: "existing documents take keys by
/// their current order at first sync"), and so is the region-lock list (§6.7: locks sync as
/// document properties; the model has no lock field yet — see [`locks`]). Both sit behind
/// mutexes so the adapter stays shareable; a session owns one adapter per open document and
/// the family's document discipline is single-threaded per session, so contention is not a
/// concern — correctness is.
pub struct DesigncraftHub {
    schema: schema::DesignSchema,
    positions: Mutex<BTreeMap<u64, String>>,
    locks: Mutex<Vec<locks::RegionLock>>,
}

impl DesigncraftHub {
    /// A fresh adapter with the default limits, an empty key map and no locks.
    #[must_use]
    pub fn new() -> Self {
        Self { schema: schema::DesignSchema::default(), positions: Mutex::new(BTreeMap::new()), locks: Mutex::new(Vec::new()) }
    }

    /// The sibling key of the object with raw id `raw`, if one has been assigned.
    #[must_use]
    pub fn position_key(&self, raw: u64) -> Option<String> {
        self.positions.lock().unwrap_or_else(|e| e.into_inner()).get(&raw).cloned()
    }

    /// The region locks currently in force (§6.7 — the `regionLocks` document property's
    /// backing state; see [`locks`] for where the list lives and why).
    #[must_use]
    pub fn region_locks(&self) -> Vec<locks::RegionLock> {
        self.locks.lock().unwrap_or_else(|e| e.into_inner()).clone()
    }

    /// The generic id of a document id (DesignCraft's ids share one counter per document, so
    /// the raw value *is* the object identity across kinds).
    ///
    /// Session-ready ids stay under `MAX_NEXT_ID` (2^53) while the core packs the client space
    /// into the high 32 bits — so the session wiring can switch `Document::alloc` to
    /// space-prefixed ids and this mapping does not change.
    #[must_use]
    pub fn hub_id(raw: u64) -> HubId {
        HubId(raw)
    }

    /// The kind string of a spread (`parentSpread` for the parent — "master" — list).
    #[must_use]
    pub fn kind_of_spread(spread: &Spread) -> &'static str {
        if spread.parent.is_some() { "parentSpread" } else { "spread" }
    }

    /// The kind string of an item: the content takes precedence (the Layers-panel label's own
    /// precedence), then the drawn shape.
    #[must_use]
    pub fn kind_of_item(item: &Item) -> &'static str {
        match &item.content {
            Content::Text(_) => "textFrame",
            Content::Graphic(_) => "graphic",
            Content::Group { .. } => "group",
            Content::Unassigned => match item.shape {
                Shape::Rectangle => "rectangle",
                Shape::Oval => "ellipse",
                Shape::Polygon => "polygon",
                Shape::GraphicLine => "line",
                Shape::Path | Shape::Group => "path",
            },
        }
    }

    /// The default shape for an item kind string (a `Create` without a `shape` property — the
    /// diff always sends one, so this only feeds defaults and tests).
    #[must_use]
    pub fn default_shape(kind: &str) -> Option<Shape> {
        let shape = match kind {
            "textFrame" | "graphic" | "rectangle" => Shape::Rectangle,
            "ellipse" => Shape::Oval,
            "polygon" => Shape::Polygon,
            "line" => Shape::GraphicLine,
            "path" => Shape::Path,
            "group" => Shape::Group,
            _ => return None,
        };
        Some(shape)
    }

    /// A default page for a `Create` (width and height from the document setup; the diff always
    /// carries every page property, so this only feeds defaults).
    fn default_page(doc: &Document, raw: u64) -> Page {
        Page {
            id: PageId(raw),
            width: doc.settings.page_width,
            height: doc.settings.page_height,
            x: 0.0,
            margins: Margins::uniform(36.0),
            columns: Columns::default(),
            parent: None,
            side: PageSide::Single,
            overridden: Vec::new(),
            guides: Vec::new(),
            show_parent_items: true,
            liquid: Default::default(),
            transition: None,
            view_rotation: 0,
            show_parent_overlay: false,
        }
    }

    // ---- Locating objects in the real model ---------------------------------------------

    /// Where the spread with raw id `raw` lives (document spreads, then parents).
    fn find_spread(doc: &Document, raw: u64) -> Option<(SpreadRef, usize)> {
        doc.spreads
            .iter()
            .position(|s| s.id.0 == raw)
            .map(|i| (SpreadRef::Doc(i), i))
            .or_else(|| doc.parents.iter().position(|s| s.id.0 == raw).map(|i| (SpreadRef::Parent(i), i)))
    }

    /// Where the page with raw id `raw` lives.
    fn find_page(doc: &Document, raw: u64) -> Option<(SpreadRef, usize)> {
        doc.spread_refs().find_map(|r| doc.spread(r).and_then(|sp| sp.pages.iter().position(|p| p.id.0 == raw).map(|pi| (r, pi))))
    }

    /// The `Arc` of the item with raw id `raw`, anywhere in the tree.
    fn find_item_arc(doc: &Document, raw: u64) -> Option<&Arc<Item>> {
        doc.spreads.iter().chain(doc.parents.iter()).find_map(|sp| find_item_in(&sp.items, raw))
    }

    /// The parent of the object with raw id `raw` in the generic tree.
    fn parent_of(doc: &Document, raw: u64) -> Option<HubId> {
        if let Some((r, _)) = Self::find_spread(doc, raw) {
            return doc.spread(r).map(|_| ROOT);
        }
        if let Some((r, _)) = Self::find_page(doc, raw) {
            return doc.spread(r).map(|sp| Self::hub_id(sp.id.0));
        }
        let loc = doc.find(ItemId(raw))?;
        let sp = doc.spread(loc.spread)?;
        if loc.path.len() == 1 {
            return Some(Self::hub_id(sp.id.0));
        }
        // Nested: the ancestor one step up the path.
        let mut it: &Item = &sp.items[*loc.path.first()?];
        for &i in &loc.path[1..loc.path.len() - 1] {
            it = it.children().get(i)?;
        }
        Some(Self::hub_id(it.id.0))
    }

    /// Whether the item with raw id `raw` contains the item `maybe_descendant` (the move cycle
    /// guard — the core refuses cycles; the adapter must not build one in the model either).
    fn item_contains(doc: &Document, raw: u64, maybe_descendant: ItemId) -> bool {
        let Some(arc) = Self::find_item_arc(doc, raw) else { return false };
        let mut found = false;
        arc.walk(&mut |i| {
            if i.id == maybe_descendant {
                found = true;
            }
        });
        found
    }

    /// The children of `parent` in document order (pages before items under a spread; document
    /// spreads before parents under the root).
    fn children_of(doc: &Document, parent: HubId) -> Vec<u64> {
        if parent == ROOT {
            return doc.spreads.iter().map(|s| s.id.0).chain(doc.parents.iter().map(|s| s.id.0)).collect();
        }
        if let Some((r, _)) = Self::find_spread(doc, parent.0)
            && let Some(sp) = doc.spread(r)
        {
            return sp.pages.iter().map(|p| p.id.0).chain(sp.items.iter().map(|i| i.id.0)).collect();
        }
        if let Some(arc) = Self::find_item_arc(doc, parent.0) {
            return arc.children().iter().map(|c| c.id.0).collect();
        }
        Vec::new()
    }

    // ---- Position keys -------------------------------------------------------------------

    /// Assign sibling keys to every node that has none, by current order (the first-sync rule).
    fn first_sync_keys(&self, doc: &Document) {
        let mut positions = self.positions.lock().unwrap_or_else(|e| e.into_inner());
        let mut assign = |ids: &[u64], positions: &mut BTreeMap<u64, String>| {
            let missing: Vec<u64> = ids.iter().copied().filter(|c| !positions.contains_key(c)).collect();
            if !missing.is_empty()
                && let Ok(keys) = frac::n_between(None, None, missing.len())
            {
                for (id, key) in missing.into_iter().zip(keys) {
                    positions.insert(id, key);
                }
            }
        };
        assign(&Self::children_of(doc, ROOT), &mut positions);
        for spread in doc.spreads.iter().chain(doc.parents.iter()) {
            let ids: Vec<u64> = spread.pages.iter().map(|p| p.id.0).chain(spread.items.iter().map(|i| i.id.0)).collect();
            assign(&ids, &mut positions);
            for arc in &spread.items {
                assign_group_keys(arc, &mut assign, &mut positions);
            }
        }
    }

    /// Record `key` for `raw`.
    fn set_key(&self, raw: u64, key: String) {
        self.positions.lock().unwrap_or_else(|e| e.into_inner()).insert(raw, key);
    }

    /// Drop the keys of a subtree (before the document forgets it).
    fn drop_keys(&self, doc: &Document, raw: u64) {
        let mut doomed = vec![raw];
        let mut stack = Self::children_of(doc, HubId(raw));
        while let Some(next) = stack.pop() {
            stack.extend(Self::children_of(doc, HubId(next)));
            doomed.push(next);
        }
        let mut positions = self.positions.lock().unwrap_or_else(|e| e.into_inner());
        for d in doomed {
            positions.remove(&d);
        }
    }

    /// Sort one parent's lists by `(key, id)` after a key change, in the document itself.
    /// Pages and items are disjoint orderings of one key space, so sorting each list by key
    /// matches sorting the combined list and splitting it.
    fn reorder_children(doc: &mut Document, parent: HubId, keys: &BTreeMap<u64, String>) {
        let rank = |id: &u64| (keys.get(id).cloned().unwrap_or_default(), *id);
        if parent == ROOT {
            doc.spreads.sort_by_key(|s| rank(&s.id.0));
            doc.parents.sort_by_key(|s| rank(&s.id.0));
            return;
        }
        if let Some((r, _)) = Self::find_spread(doc, parent.0)
            && let Some(sp) = doc.spread_mut(r)
        {
            sp.pages.sort_by_key(|p| rank(&p.id.0));
            sp.items.sort_by_key(|i| rank(&i.id.0));
            return;
        }
        if let Some(loc) = doc.find(ItemId(parent.0))
            && let Some(sp) = doc.spread_mut(loc.spread)
        {
            let mut it: &mut Item = Arc::make_mut(&mut sp.items[loc.path[0]]);
            for &i in &loc.path[1..] {
                let Some(children) = it.children_mut() else { return };
                it = Arc::make_mut(&mut children[i]);
            }
            if let Some(children) = it.children_mut() {
                children.sort_by_key(|c| rank(&c.id.0));
            }
        }
    }

    /// The insertion index for `(key, id)` among `members` ordered by `(key, id)`.
    fn insertion_index(keys: &BTreeMap<u64, String>, members: &[u64], id: u64, key: &str) -> usize {
        members.iter().filter(|m| (keys.get(*m).cloned().unwrap_or_default(), **m) < (key.to_owned(), id)).count()
    }

    // ---- Extraction and insertion (moves) -------------------------------------------------

    /// Take the item with raw id `raw` out of the tree (pure extraction — no story cleanup, no
    /// deletion semantics; a move keeps everything alive).
    fn take_item(doc: &mut Document, raw: u64) -> Option<Arc<Item>> {
        let loc = doc.find(ItemId(raw))?;
        if loc.path.len() == 1 {
            let i = loc.path[0];
            let sp = doc.spread_mut(loc.spread)?;
            return if i < sp.items.len() { Some(sp.items.remove(i)) } else { None };
        }
        let last = *loc.path.last()?;
        let sp = doc.spread_mut(loc.spread)?;
        let mut it: &mut Item = Arc::make_mut(&mut sp.items[loc.path[0]]);
        for &i in &loc.path[1..loc.path.len() - 1] {
            let children = it.children_mut()?;
            it = Arc::make_mut(children.get_mut(i)?);
        }
        let children = it.children_mut()?;
        if last >= children.len() {
            return None;
        }
        Some(children.remove(last))
    }

    /// Insert `arc` into the children of the group item with raw id `parent`, at `at`.
    fn insert_into_group(doc: &mut Document, parent: u64, arc: Arc<Item>, at: usize) -> bool {
        let Some(loc) = doc.find(ItemId(parent)) else { return false };
        let Some(sp) = doc.spread_mut(loc.spread) else { return false };
        let mut it: &mut Item = Arc::make_mut(&mut sp.items[loc.path[0]]);
        for &i in &loc.path[1..] {
            let Some(children) = it.children_mut() else { return false };
            let Some(child) = children.get_mut(i) else { return false };
            it = Arc::make_mut(child);
        }
        let Some(children) = it.children_mut() else { return false };
        let at = at.min(children.len());
        children.insert(at, arc);
        true
    }

    // ---- The order diff --------------------------------------------------------------------

    /// Diff one parent's child order between two documents, assigning fresh keys to the moved
    /// and inserted children while the longest rising run of old keys keeps its keys (the v2
    /// discipline: only the children that must move get new keys, so an insertion in the middle
    /// costs one key, not a renumbering). Emits `Set(position)` and `Set(parent)` into `out`.
    fn diff_order(&self, before: &Document, after: &Document, parent: HubId, out: &mut Vec<Change>) {
        let old = Self::children_of(before, parent);
        let new = Self::children_of(after, parent);
        if old == new {
            return;
        }
        let mut positions = self.positions.lock().unwrap_or_else(|e| e.into_inner());
        // First-sync rule: children of the before-order without keys take them by order.
        let unkeyed: Vec<u64> = old.iter().copied().filter(|c| !positions.contains_key(c)).collect();
        if !unkeyed.is_empty()
            && let Ok(fresh) = frac::n_between(None, None, unkeyed.len())
        {
            for (id, key) in unkeyed.into_iter().zip(fresh) {
                positions.insert(id, key);
            }
        }
        // The longest strictly-rising run of keys over the new order, counting only children
        // that already have keys AND already belonged to this parent (a child new to this
        // parent — created or moved in — always takes a fresh key).
        let has_key = |c: &u64| positions.contains_key(c);
        let old_member = |c: &u64| old.contains(c);
        let n = new.len();
        let mut len = vec![0usize; n];
        let mut prev = vec![usize::MAX; n];
        let mut best_end = None;
        let mut best_len = 0usize;
        for i in 0..n {
            if has_key(&new[i]) && old_member(&new[i]) {
                len[i] = 1;
                for j in 0..i {
                    if has_key(&new[j])
                        && old_member(&new[j])
                        && positions.get(&new[j]).is_some_and(|a| positions.get(&new[i]).is_some_and(|b| a < b))
                        && len[j] + 1 > len[i]
                    {
                        len[i] = len[j] + 1;
                        prev[i] = j;
                    }
                }
                if len[i] > best_len {
                    best_len = len[i];
                    best_end = Some(i);
                }
            }
        }
        let mut kept = vec![false; n];
        let mut walk = best_end;
        while let Some(i) = walk {
            kept[i] = true;
            walk = if prev[i] == usize::MAX { None } else { Some(prev[i]) };
        }
        // Walk the new order; assign fresh keys to runs of not-kept children between the kept
        // neighbours (the key of the kept child before the run and the one after it).
        fn flush(
            pending: &mut Vec<u64>,
            prev_key: Option<&str>,
            next_key: Option<&str>,
            positions: &mut BTreeMap<u64, String>,
            out: &mut Vec<Change>,
        ) {
            if pending.is_empty() {
                return;
            }
            let count = pending.len();
            if let Ok(fresh) = frac::n_between(prev_key, next_key, count) {
                for (id, key) in pending.drain(..).zip(fresh) {
                    positions.insert(id, key.clone());
                    out.push(Change::Set { id: HubId(id), prop: Arc::from("position"), value: Value::Json(serde_json::Value::String(key)) });
                }
            }
        }
        let mut last_kept_key: Option<String> = None;
        let mut pending: Vec<u64> = Vec::new();
        for (i, c) in new.iter().enumerate() {
            if kept[i] {
                let next_key = positions.get(c).cloned();
                flush(&mut pending, last_kept_key.as_deref(), next_key.as_deref(), &mut positions, out);
                last_kept_key = positions.get(c).cloned();
            } else {
                pending.push(*c);
            }
        }
        flush(&mut pending, last_kept_key.as_deref(), None, &mut positions, out);
        // Children that changed parent get the parent write after their key.
        for c in &new {
            let from = Self::parent_of(before, *c);
            let to = Self::parent_of(after, *c);
            if from != to {
                let p = to.unwrap_or(ROOT);
                out.push(Change::Set { id: HubId(*c), prop: Arc::from("parent"), value: Value::Json(serde_json::Value::Number(p.0.into())) });
            }
        }
    }
}

/// Recursively assign first-sync keys to a group's children.
fn assign_group_keys(arc: &Arc<Item>, assign: &mut dyn FnMut(&[u64], &mut BTreeMap<u64, String>), positions: &mut BTreeMap<u64, String>) {
    let ids: Vec<u64> = arc.children().iter().map(|c| c.id.0).collect();
    assign(&ids, positions);
    for c in arc.children() {
        assign_group_keys(c, assign, positions);
    }
}

/// Find an item's `Arc` in a list of top-level items (recursive).
fn find_item_in(items: &[Arc<Item>], raw: u64) -> Option<&Arc<Item>> {
    for arc in items {
        if arc.id.0 == raw {
            return Some(arc);
        }
        if let Some(found) = find_item_in(arc.children(), raw) {
            return Some(found);
        }
    }
    None
}

impl Default for DesigncraftHub {
    fn default() -> Self {
        Self::new()
    }
}

impl HubAdapter for DesigncraftHub {
    type Doc = Document;

    fn view(&self, doc: &Document) -> DocumentView {
        self.first_sync_keys(doc);
        let positions = self.positions.lock().unwrap_or_else(|e| e.into_inner());
        let key = |id: &u64| positions.get(id).cloned().unwrap_or_default();
        let mut nodes = Vec::new();
        let mut push = |id: u64, kind: &'static str, parent: HubId, props: Vec<(Arc<str>, Value)>| {
            nodes.push(ViewNode { id: HubId(id), kind: Arc::from(kind), parent, position: key(&id), props });
        };
        for spread in doc.spreads.iter().chain(doc.parents.iter()) {
            let sid = spread.id.0;
            let props = props::SPREAD_PROPS.iter().filter_map(|p| props::spread_value(spread, p).map(|v| (Arc::from(*p), v))).collect();
            push(sid, Self::kind_of_spread(spread), ROOT, props);
            for page in &spread.pages {
                let props = props::PAGE_PROPS.iter().filter_map(|p| props::page_value(page, p).map(|v| (Arc::from(*p), v))).collect();
                push(page.id.0, "page", HubId(sid), props);
            }
            let mut stack: Vec<(&Arc<Item>, u64)> = spread.items.iter().map(|i| (i, sid)).rev().collect();
            while let Some((arc, parent)) = stack.pop() {
                let mut props: Vec<(Arc<str>, Value)> =
                    props::ITEM_PROPS.iter().filter_map(|p| props::item_value(arc, p).map(|v| (Arc::from(*p), v))).collect();
                if let Some(v) = props::content_value(arc) {
                    props.push((Arc::from("content"), v));
                }
                push(arc.id.0, Self::kind_of_item(arc), HubId(parent), props);
                stack.extend(arc.children().iter().map(|c| (c, arc.id.0)).rev());
            }
        }
        let entries = schema::entry_values(doc);
        let mut doc_props: Vec<(String, Value)> =
            ["title", "settings", "styles", "nextId"].iter().filter_map(|p| props::doc_prop_value(doc, p).map(|v| ((*p).to_owned(), v))).collect();
        {
            let locks = self.locks.lock().unwrap_or_else(|e| e.into_inner());
            if let Ok(v) = serde_json::to_value(&*locks).map(Value::Json) {
                doc_props.push((locks::RegionLock::PROP.to_owned(), v));
            }
        }
        DocumentView { nodes, entries, doc: doc_props }
    }

    fn apply(&self, doc: &mut Document, changes: &[Change]) -> ApplyReport {
        let mut report = ApplyReport::default();
        for change in changes {
            match change {
                Change::Create { id, kind, parent, position, props } => {
                    let raw = id.0;
                    match kind.as_ref() {
                        "spread" | "parentSpread" => {
                            if *parent != ROOT {
                                report.skipped += 1;
                                continue;
                            }
                            let mut spread = Spread {
                                id: SpreadId(raw),
                                pages: Vec::new(),
                                items: Vec::new(),
                                parent: (kind.as_ref() == "parentSpread").then(|| ParentInfo {
                                    prefix: "A".into(),
                                    name: "Parent".into(),
                                    based_on: None,
                                }),
                                allow_shuffle: true,
                            };
                            for (p, v) in props {
                                if p.as_ref() == "parent" || p.as_ref() == "position" {
                                    continue;
                                }
                                match props::set_spread_field(&mut spread, p, v) {
                                    Some(Ok(())) => report.applied += 1,
                                    Some(Err(_)) | None => report.skipped += 1,
                                }
                            }
                            self.set_key(raw, position.clone());
                            let keys = self.positions.lock().unwrap_or_else(|e| e.into_inner()).clone();
                            let is_parent = kind.as_ref() == "parentSpread";
                            let members: Vec<u64> =
                                if is_parent { doc.parents.iter().map(|s| s.id.0).collect() } else { doc.spreads.iter().map(|s| s.id.0).collect() };
                            let at = Self::insertion_index(&keys, &members, raw, position);
                            if is_parent {
                                doc.parents.insert(at.min(doc.parents.len()), Arc::new(spread));
                            } else {
                                doc.spreads.insert(at.min(doc.spreads.len()), Arc::new(spread));
                            }
                            Self::reorder_children(doc, ROOT, &keys);
                            report.applied += 1;
                        }
                        "page" => {
                            if *parent == ROOT {
                                report.skipped += 1;
                                continue;
                            }
                            let Some((r, _)) = Self::find_spread(doc, parent.0) else {
                                report.skipped += 1;
                                continue;
                            };
                            let mut page = Self::default_page(doc, raw);
                            for (p, v) in props {
                                if p.as_ref() == "parent" || p.as_ref() == "position" {
                                    continue;
                                }
                                match props::set_page_field(&mut page, p, v) {
                                    Some(Ok(())) => report.applied += 1,
                                    Some(Err(_)) | None => report.skipped += 1,
                                }
                            }
                            self.set_key(raw, position.clone());
                            let keys = self.positions.lock().unwrap_or_else(|e| e.into_inner()).clone();
                            let members: Vec<u64> = doc.spread(r).map_or_else(Vec::new, |sp| sp.pages.iter().map(|p| p.id.0).collect());
                            let at = Self::insertion_index(&keys, &members, raw, position);
                            let Some(sp) = doc.spread_mut(r) else {
                                report.skipped += 1;
                                continue;
                            };
                            sp.pages.insert(at.min(sp.pages.len()), page);
                            Self::reorder_children(doc, *parent, &keys);
                            report.applied += 1;
                        }
                        item_kind => {
                            let Some(shape) = Self::default_shape(item_kind) else {
                                report.skipped += 1;
                                continue;
                            };
                            if *parent == ROOT {
                                report.skipped += 1;
                                continue;
                            }
                            let mut item = Item::new(ItemId(raw), doc.default_layer(), shape, PathData::default());
                            if item_kind == "group" {
                                item.content = Content::Group { items: Vec::new() };
                            }
                            for (p, v) in props {
                                if p.as_ref() == "parent" || p.as_ref() == "position" {
                                    continue;
                                }
                                if p.as_ref() == "content" {
                                    match props::set_content(&mut item, v) {
                                        Ok(()) => report.applied += 1,
                                        Err(_) => report.skipped += 1,
                                    }
                                } else {
                                    match props::set_item_field(&mut item, p, v) {
                                        Some(Ok(())) => report.applied += 1,
                                        Some(Err(_)) | None => report.skipped += 1,
                                    }
                                }
                            }
                            self.set_key(raw, position.clone());
                            let keys = self.positions.lock().unwrap_or_else(|e| e.into_inner()).clone();
                            let inserted = if let Some((r, _)) = Self::find_spread(doc, parent.0) {
                                let members: Vec<u64> = doc.spread(r).map_or_else(Vec::new, |sp| sp.items.iter().map(|i| i.id.0).collect());
                                let at = Self::insertion_index(&keys, &members, raw, position);
                                let Some(sp) = doc.spread_mut(r) else {
                                    report.skipped += 1;
                                    continue;
                                };
                                sp.items.insert(at.min(sp.items.len()), Arc::new(item));
                                true
                            } else if doc.find(ItemId(parent.0)).is_some() {
                                let members: Vec<u64> =
                                    Self::find_item_arc(doc, parent.0).map_or_else(Vec::new, |g| g.children().iter().map(|c| c.id.0).collect());
                                let at = Self::insertion_index(&keys, &members, raw, position);
                                Self::insert_into_group(doc, parent.0, Arc::new(item), at)
                            } else {
                                false
                            };
                            if inserted {
                                Self::reorder_children(doc, *parent, &keys);
                                report.applied += 1;
                            } else {
                                report.skipped += 1;
                            }
                        }
                    }
                }
                Change::Set { id, prop, value } => {
                    let raw = id.0;
                    if prop.as_ref() == "position" {
                        let Value::Json(serde_json::Value::String(key)) = value else {
                            report.skipped += 1;
                            continue;
                        };
                        let Some(owner) = Self::parent_of(doc, raw) else {
                            report.skipped += 1;
                            continue;
                        };
                        self.set_key(raw, key.clone());
                        let keys = self.positions.lock().unwrap_or_else(|e| e.into_inner()).clone();
                        Self::reorder_children(doc, owner, &keys);
                        report.applied += 1;
                    } else if prop.as_ref() == "parent" {
                        let Some(p) = value.to_json().as_u64().map(HubId) else {
                            report.skipped += 1;
                            continue;
                        };
                        if p == ROOT || p.0 == raw || Self::find_spread(doc, raw).is_some() {
                            report.skipped += 1; // spreads never move; nothing parents itself
                            continue;
                        }
                        if Self::find_page(doc, raw).is_some() {
                            let Some((r, _)) = Self::find_spread(doc, p.0) else {
                                report.skipped += 1;
                                continue;
                            };
                            let Some((old_r, pi)) = Self::find_page(doc, raw) else {
                                report.skipped += 1;
                                continue;
                            };
                            let page = doc.spread_mut(old_r).and_then(|sp| (pi < sp.pages.len()).then(|| sp.pages.remove(pi)));
                            let Some(page) = page else {
                                report.skipped += 1;
                                continue;
                            };
                            let keys = self.positions.lock().unwrap_or_else(|e| e.into_inner()).clone();
                            let key = keys.get(&raw).cloned().unwrap_or_default();
                            let members: Vec<u64> = doc.spread(r).map_or_else(Vec::new, |sp| sp.pages.iter().map(|x| x.id.0).collect());
                            let at = Self::insertion_index(&keys, &members, raw, &key);
                            let Some(sp) = doc.spread_mut(r) else {
                                report.skipped += 1;
                                continue;
                            };
                            sp.pages.insert(at.min(sp.pages.len()), page);
                            Self::reorder_children(doc, p, &keys);
                            report.applied += 1;
                        } else if doc.find(ItemId(raw)).is_some() {
                            // No cycles: never move an item into its own subtree.
                            if Self::item_contains(doc, raw, ItemId(p.0)) {
                                report.skipped += 1;
                                continue;
                            }
                            let Some(arc) = Self::take_item(doc, raw) else {
                                report.skipped += 1;
                                continue;
                            };
                            let keys = self.positions.lock().unwrap_or_else(|e| e.into_inner()).clone();
                            let key = keys.get(&raw).cloned().unwrap_or_default();
                            let inserted = if let Some((r, _)) = Self::find_spread(doc, p.0) {
                                let members: Vec<u64> = doc.spread(r).map_or_else(Vec::new, |sp| sp.items.iter().map(|i| i.id.0).collect());
                                let at = Self::insertion_index(&keys, &members, raw, &key);
                                let Some(sp) = doc.spread_mut(r) else {
                                    report.skipped += 1;
                                    continue;
                                };
                                sp.items.insert(at.min(sp.items.len()), arc);
                                true
                            } else if doc.find(ItemId(p.0)).is_some() {
                                let members: Vec<u64> =
                                    Self::find_item_arc(doc, p.0).map_or_else(Vec::new, |g| g.children().iter().map(|c| c.id.0).collect());
                                let at = Self::insertion_index(&keys, &members, raw, &key);
                                Self::insert_into_group(doc, p.0, arc, at)
                            } else {
                                false
                            };
                            if inserted {
                                Self::reorder_children(doc, p, &keys);
                                report.applied += 1;
                            } else {
                                report.skipped += 1;
                            }
                        } else {
                            report.skipped += 1;
                        }
                    } else if doc.find(ItemId(raw)).is_some() {
                        let Some(item) = doc.item_mut(ItemId(raw)) else {
                            report.skipped += 1;
                            continue;
                        };
                        if prop.as_ref() == "content" {
                            match props::set_content(item, value) {
                                Ok(()) => report.applied += 1,
                                Err(_) => report.skipped += 1,
                            }
                        } else {
                            match props::set_item_field(item, prop, value) {
                                Some(Ok(())) => report.applied += 1,
                                Some(Err(_)) | None => report.skipped += 1,
                            }
                        }
                    } else if let Some((r, pi)) = Self::find_page(doc, raw) {
                        let Some(sp) = doc.spread_mut(r) else {
                            report.skipped += 1;
                            continue;
                        };
                        let Some(page) = sp.pages.get_mut(pi) else {
                            report.skipped += 1;
                            continue;
                        };
                        match props::set_page_field(page, prop, value) {
                            Some(Ok(())) => report.applied += 1,
                            Some(Err(_)) | None => report.skipped += 1,
                        }
                    } else if let Some((r, _)) = Self::find_spread(doc, raw) {
                        let Some(sp) = doc.spread_mut(r) else {
                            report.skipped += 1;
                            continue;
                        };
                        match props::set_spread_field(sp, prop, value) {
                            Some(Ok(())) => report.applied += 1,
                            Some(Err(_)) | None => report.skipped += 1,
                        }
                    } else {
                        report.skipped += 1;
                    }
                }
                Change::Delete { id } => {
                    let raw = id.0;
                    if let Some((r, si)) = Self::find_spread(doc, raw) {
                        self.drop_keys(doc, raw);
                        // Text frames inside carry story threads: give them the same cleanup a
                        // local removal does before the spread goes.
                        let item_ids: Vec<u64> = doc.spread(r).map(|sp| sp.items.iter().map(|i| i.id.0).collect()).unwrap_or_default();
                        for iid in item_ids {
                            let _ = doc.remove_item(ItemId(iid));
                        }
                        match r {
                            SpreadRef::Doc(_) => {
                                doc.spreads.remove(si);
                            }
                            SpreadRef::Parent(_) => {
                                doc.parents.remove(si);
                            }
                        }
                        report.applied += 1;
                    } else if let Some((r, pi)) = Self::find_page(doc, raw) {
                        self.drop_keys(doc, raw);
                        if let Some(sp) = doc.spread_mut(r)
                            && pi < sp.pages.len()
                        {
                            sp.pages.remove(pi);
                            report.applied += 1;
                        }
                    } else if doc.find(ItemId(raw)).is_some() {
                        self.drop_keys(doc, raw);
                        if doc.remove_item(ItemId(raw)).is_ok() {
                            report.applied += 1;
                        }
                    }
                    // Deleting a deleted object does nothing (v2 tolerance, kept).
                }
                Change::SetDoc { prop, value } => {
                    if prop.as_ref() == locks::RegionLock::PROP {
                        match locks::parse_locks(value) {
                            Ok(list) => {
                                *self.locks.lock().unwrap_or_else(|e| e.into_inner()) = list;
                                report.applied += 1;
                            }
                            Err(_) => report.skipped += 1,
                        }
                        continue;
                    }
                    let routed = props::set_doc_prop(doc, prop, value).or_else(|| schema::set_doc_order(doc, prop, value));
                    match routed {
                        Some(Ok(())) => report.applied += 1,
                        Some(Err(_)) | None => report.skipped += 1,
                    }
                }
                Change::Put { entry, value } => match schema::put_entry(doc, entry, value) {
                    Ok(()) => report.applied += 1,
                    Err(_) => report.skipped += 1,
                },
                Change::Remove { entry } => match schema::remove_entry(doc, entry) {
                    Ok(()) => report.applied += 1,
                    Err(_) => report.skipped += 1,
                },
            }
        }
        report
    }

    fn diff(&self, before: &Document, after: &Document) -> Vec<Change> {
        let mut sets = Vec::new();
        let mut creates = Vec::new();

        // Field sets and creations, walking the after-tree parents-first (the DFS order the
        // batch must carry: a `Create` never precedes its parent's).
        for spread in after.spreads.iter().chain(after.parents.iter()) {
            let sid = spread.id.0;
            match Self::find_spread(before, sid).and_then(|(r, _)| before.spread(r)) {
                Some(b) => {
                    for prop in props::SPREAD_PROPS {
                        if props::spread_field_differs(b, spread, prop).unwrap_or(false)
                            && let Some(v) = props::spread_value(spread, prop)
                        {
                            sets.push(Change::Set { id: HubId(sid), prop: Arc::from(*prop), value: v });
                        }
                    }
                }
                None => {
                    let props_vec = props::SPREAD_PROPS.iter().filter_map(|p| props::spread_value(spread, p).map(|v| (Arc::from(*p), v))).collect();
                    creates.push(Change::Create {
                        id: HubId(sid),
                        kind: Arc::from(Self::kind_of_spread(spread)),
                        parent: ROOT,
                        position: String::new(),
                        props: props_vec,
                    });
                }
            }
            for page in &spread.pages {
                let pid = page.id.0;
                match Self::find_page(before, pid).and_then(|(r, pi)| before.spread(r).and_then(|sp| sp.pages.get(pi))) {
                    Some(b) => {
                        for prop in props::PAGE_PROPS {
                            if props::page_field_differs(b, page, prop).unwrap_or(false)
                                && let Some(v) = props::page_value(page, prop)
                            {
                                sets.push(Change::Set { id: HubId(pid), prop: Arc::from(*prop), value: v });
                            }
                        }
                    }
                    None => {
                        let props_vec = props::PAGE_PROPS.iter().filter_map(|p| props::page_value(page, p).map(|v| (Arc::from(*p), v))).collect();
                        creates.push(Change::Create {
                            id: HubId(pid),
                            kind: Arc::from("page"),
                            parent: HubId(sid),
                            position: String::new(),
                            props: props_vec,
                        });
                    }
                }
            }
            let mut stack: Vec<(&Arc<Item>, u64)> = spread.items.iter().map(|i| (i, sid)).rev().collect();
            while let Some((arc, parent)) = stack.pop() {
                let iid = arc.id.0;
                match Self::find_item_arc(before, iid) {
                    Some(b) if Arc::ptr_eq(b, arc) => {}
                    Some(b) => {
                        for prop in props::ITEM_PROPS {
                            if props::item_field_differs(b, arc, prop).unwrap_or(false)
                                && let Some(v) = props::item_value(arc, prop)
                            {
                                sets.push(Change::Set { id: HubId(iid), prop: Arc::from(*prop), value: v });
                            }
                        }
                        if props::content_differs(b, arc)
                            && let Some(v) = props::content_value(arc)
                        {
                            sets.push(Change::Set { id: HubId(iid), prop: Arc::from("content"), value: v });
                        }
                    }
                    None => {
                        let mut props_vec: Vec<(Arc<str>, Value)> =
                            props::ITEM_PROPS.iter().filter_map(|p| props::item_value(arc, p).map(|v| (Arc::from(*p), v))).collect();
                        if let Some(v) = props::content_value(arc) {
                            props_vec.push((Arc::from("content"), v));
                        }
                        creates.push(Change::Create {
                            id: HubId(iid),
                            kind: Arc::from(Self::kind_of_item(arc)),
                            parent: HubId(parent),
                            position: String::new(),
                            props: props_vec,
                        });
                    }
                }
                stack.extend(arc.children().iter().map(|c| (c, arc.id.0)).rev());
            }
        }

        // Deletions: one Delete per subtree root (a gone page under a gone spread needs none,
        // and neither does a gone item under a gone top-level item).
        let before_ids: std::collections::BTreeSet<u64> = all_node_ids(before).collect();
        let after_ids: std::collections::BTreeSet<u64> = all_node_ids(after).collect();
        let gone: Vec<u64> = before_ids.difference(&after_ids).copied().collect();
        let mut deletes: Vec<Change> = Vec::new();
        for &raw in &gone {
            if Self::find_spread(before, raw).is_some() {
                deletes.push(Change::Delete { id: HubId(raw) });
            } else if let Some((r, _)) = Self::find_page(before, raw) {
                // Only when the owning spread survived; otherwise its Delete covers the page.
                let spread_gone = before.spread(r).is_some_and(|sp| !after_ids.contains(&sp.id.0));
                if !spread_gone {
                    deletes.push(Change::Delete { id: HubId(raw) });
                }
            } else if let Some(loc) = before.find(ItemId(raw)) {
                // Only when the top-level item containing it survived (or it *is* top-level).
                let top = before.spread(loc.spread).and_then(|sp| sp.items.get(loc.top())).map_or(raw, |i| i.id.0);
                if top == raw || !gone.contains(&top) {
                    deletes.push(Change::Delete { id: HubId(raw) });
                }
            }
        }

        // Order and moves per parent (root first, then every spread and group that exists in
        // either document).
        let mut order_changes = Vec::new();
        let mut parents: Vec<HubId> = vec![ROOT];
        for raw in spread_ids(before).chain(spread_ids(after)) {
            let id = HubId(raw);
            if !parents.contains(&id) {
                parents.push(id);
            }
        }
        for raw in group_ids(before).chain(group_ids(after)) {
            let id = HubId(raw);
            if !parents.contains(&id) {
                parents.push(id);
            }
        }
        for parent in parents {
            if parent == ROOT || after_ids.contains(&parent.0) {
                self.diff_order(before, after, parent, &mut order_changes);
            }
        }

        // The creations' keys, now that the order pass has assigned them.
        let positions = self.positions.lock().unwrap_or_else(|e| e.into_inner());
        for create in &mut creates {
            if let Change::Create { id, position, .. } = create {
                *position = positions.get(&id.0).cloned().unwrap_or_default();
            }
        }

        // Document-level scalars and entries.
        let mut doc_level = Vec::new();
        for prop in ["title", "settings", "styles", "nextId"] {
            if props::doc_prop_value(before, prop) != props::doc_prop_value(after, prop)
                && let Some(v) = props::doc_prop_value(after, prop)
            {
                doc_level.push(Change::SetDoc { prop: Arc::from(prop), value: v });
            }
        }
        doc_level.extend(schema::diff_entries(before, after));

        // Batch order (§2.3): field writes, creations (parents first), moves, deletes, doc level.
        let mut batch = sets;
        batch.append(&mut creates);
        batch.append(&mut order_changes);
        batch.extend(deletes);
        batch.append(&mut doc_level);
        batch
    }

    fn presence(&self, doc: &Document, ui: &UiState) -> Option<craft_collab::presence::PresenceMsg> {
        let page = ui.page.or_else(|| doc.spreads.first().and_then(|s| s.pages.first()).map(|p| p.id.0))?;
        Some(craft_collab::presence::PresenceMsg {
            cursor: ui.cursor,
            page: Some(page),
            selection: ui.selection.clone(),
            viewport: ui.viewport,
            editing_text: ui.editing_text,
            playhead: None,    // DesignCraft has no timeline
            track_range: None, // DesignCraft has no tracks
            generating: None,  // DesignCraft runs no generation jobs
        })
    }

    fn schema(&self) -> &dyn craft_collab::adapter::DocSchema {
        &self.schema
    }
}

/// Every object id in the document (spreads, pages, items — pre-order).
fn all_node_ids(doc: &Document) -> impl Iterator<Item = u64> + '_ {
    let spreads: Vec<u64> = spread_ids(doc).collect();
    let pages: Vec<u64> = doc.spreads.iter().chain(doc.parents.iter()).flat_map(|sp| sp.pages.iter().map(|p| p.id.0)).collect();
    let items: Vec<u64> = doc.spreads.iter().chain(doc.parents.iter()).flat_map(|sp| sp.items.iter().flat_map(subtree_ids)).collect();
    spreads.into_iter().chain(pages).chain(items)
}

/// The id of an item and everything under it.
fn subtree_ids(arc: &Arc<Item>) -> Vec<u64> {
    let mut out = Vec::new();
    arc.walk(&mut |i| out.push(i.id.0));
    out
}

/// The spread ids (document spreads, then parents).
fn spread_ids(doc: &Document) -> impl Iterator<Item = u64> + '_ {
    doc.spreads.iter().chain(doc.parents.iter()).map(|s| s.id.0)
}

/// The ids of items that hold children (the group parents).
fn group_ids(doc: &Document) -> impl Iterator<Item = u64> + '_ {
    let mut v: Vec<u64> = Vec::new();
    for sp in doc.spreads.iter().chain(doc.parents.iter()) {
        for arc in &sp.items {
            collect_groups(arc, &mut v);
        }
    }
    v.into_iter()
}

fn collect_groups(arc: &Arc<Item>, out: &mut Vec<u64>) {
    if !arc.children().is_empty() {
        out.push(arc.id.0);
    }
    for c in arc.children() {
        collect_groups(c, out);
    }
}
