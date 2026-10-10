//! Fill one data-merge record into a document, and copy template items for later records.
//!
//! Placeholders are addressed through an id map so each record's copy can be filled from the
//! unfilled template. A placeholder whose frame or story is not in the map is left as it is.

use std::collections::HashMap;
use std::path::Path;
use std::sync::Arc;

use designcraft_doc::{
    AnchorPosition, AnchoredObject, Asset, AssetId, Content, Fill, Fitting, Graphic, Hyperlink, HyperlinkDest, HyperlinkSource, Item, ItemId,
    MergeOptions, OBJECT_MARK, PlaceholderAnchor, PlaceholderRole, Shape, SpreadRef, Story, StoryId, is_virtual_field,
};
use designcraft_geom::{Affine, Rect, Vec2};

use super::parse::image_candidates;
use super::records::{CellRecord, part_has_field, resolve_field};
use crate::EngineError;

#[derive(Clone, Debug, Default)]
pub struct Maps {
    pub items: HashMap<ItemId, ItemId>,
    pub stories: HashMap<StoryId, StoryId>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct MissingImage {
    pub record: u32,
    pub field: String,
    pub path: String,
}

#[derive(Clone, Debug, Default)]
pub struct FillReport {
    pub missing: Vec<MissingImage>,
    pub warnings: Vec<String>,
}

pub const PARENT_WARNING: &str = "A parent-page placeholder was left unfilled.";

/// Document-spread items and their stories, each mapped to itself.
pub fn identity_map(doc: &designcraft_doc::Document) -> Maps {
    let mut maps = Maps::default();
    for sp in &doc.spreads {
        for it in &sp.items {
            it.walk(&mut |i| {
                maps.items.insert(i.id, i.id);
                if let Content::Text(tf) = &i.content {
                    maps.stories.insert(tf.story, tf.story);
                }
            });
        }
    }
    maps
}

/// Deep-copy one top-level item. Children stay in their own space; only this item is translated.
pub fn copy_item(
    dst: &mut designcraft_doc::Document,
    src: &designcraft_doc::Document,
    it: &Item,
    to: SpreadRef,
    off: Vec2,
    maps: &mut Maps,
) -> Result<(), EngineError> {
    let mut copy = it.clone();
    copy.xf = Affine::translate(off) * copy.xf;
    renumber(dst, src, &mut copy, maps);
    if dst.layer(copy.layer).is_none() {
        copy.layer = dst.default_layer();
    }
    dst.insert_item(to, copy, None)?;
    Ok(())
}

fn clear_grid(it: &mut Item) {
    it.data_grid = None;
    if let Content::Group { items } = &mut it.content {
        for child in items {
            clear_grid(Arc::make_mut(child));
        }
    }
}

/// Stories and assets a set of items needs, copied out so those items can be placed back into the same document.
#[derive(Clone, Debug, Default)]
pub struct Snapshot {
    pub stories: HashMap<StoryId, Arc<Story>>,
    pub assets: HashMap<AssetId, Arc<Asset>>,
}

pub fn snapshot_of(doc: &designcraft_doc::Document, items: &[Item]) -> Snapshot {
    let mut snap = Snapshot::default();
    for it in items {
        it.walk(&mut |item| match &item.content {
            Content::Text(tf) => {
                if let Some(st) = doc.story(tf.story) {
                    snap.stories.insert(tf.story, Arc::new(st.clone()));
                }
            }
            Content::Graphic(g) => {
                if let Some(asset) = doc.assets.get(&g.asset) {
                    snap.assets.insert(g.asset, asset.clone());
                }
            }
            Content::Group { .. } | Content::Unassigned => {}
        });
    }
    snap
}

/// Place `it` using stories and assets from `snap` rather than from another document.
pub fn place_from_snapshot(
    dst: &mut designcraft_doc::Document,
    snap: &Snapshot,
    it: &Item,
    to: SpreadRef,
    xf: Affine,
    maps: &mut Maps,
) -> Result<(), EngineError> {
    let mut copy = it.clone();
    copy.xf = xf;
    clear_grid(&mut copy);
    renumber_snap(dst, snap, &mut copy, maps);
    if dst.layer(copy.layer).is_none() {
        copy.layer = dst.default_layer();
    }
    dst.insert_item(to, copy, None)?;
    Ok(())
}

fn renumber_snap(dst: &mut designcraft_doc::Document, snap: &Snapshot, it: &mut Item, maps: &mut Maps) {
    let old = it.id;
    let new_id = ItemId(dst.alloc());
    maps.items.insert(old, new_id);
    it.id = new_id;
    match &mut it.content {
        Content::Text(tf) => {
            let new_sid = if let Some(n) = maps.stories.get(&tf.story) {
                *n
            } else {
                let n = StoryId(dst.alloc());
                let mut st = snap.stories.get(&tf.story).map(|st| (**st).clone()).unwrap_or_else(|| Story::new(n));
                st.id = n;
                st.frames.clear();
                dst.stories.insert(n, Arc::new(st));
                maps.stories.insert(tf.story, n);
                n
            };
            tf.story = new_sid;
            if let Some(st) = dst.story_mut(new_sid) {
                st.frames.push(new_id);
            }
        }
        Content::Graphic(g) => {
            if let Some(asset) = snap.assets.get(&g.asset) {
                dst.assets.entry(g.asset).or_insert_with(|| asset.clone());
            }
        }
        Content::Group { items } => {
            for child in items.iter_mut() {
                renumber_snap(dst, snap, Arc::make_mut(child), maps);
            }
        }
        Content::Unassigned => {}
    }
}

fn renumber(dst: &mut designcraft_doc::Document, src: &designcraft_doc::Document, it: &mut Item, maps: &mut Maps) {
    let old = it.id;
    let new_id = ItemId(dst.alloc());
    maps.items.insert(old, new_id);
    it.id = new_id;
    match &mut it.content {
        Content::Text(tf) => {
            let new_sid = if let Some(n) = maps.stories.get(&tf.story) {
                *n
            } else {
                let n = StoryId(dst.alloc());
                let mut st = src.story(tf.story).cloned().unwrap_or_else(|| Story::new(n));
                st.id = n;
                st.frames.clear();
                dst.stories.insert(n, Arc::new(st));
                maps.stories.insert(tf.story, n);
                n
            };
            tf.story = new_sid;
            if let Some(st) = dst.story_mut(new_sid) {
                st.frames.push(new_id);
            }
        }
        Content::Graphic(g) => {
            if let Some(a) = src.assets.get(&g.asset) {
                dst.assets.entry(g.asset).or_insert_with(|| a.clone());
            }
        }
        Content::Group { items } => {
            for child in items.iter_mut() {
                renumber(dst, src, Arc::make_mut(child), maps);
            }
        }
        Content::Unassigned => {}
    }
}

struct Rep {
    story: StoryId,
    start: usize,
    end: usize,
    text: String,
}

struct InlineJob {
    story: StoryId,
    start: usize,
    end: usize,
    item: Item,
}

pub fn fill_mode_of(rec: &CellRecord) -> FillMode {
    if rec.parts.is_empty() {
        FillMode::Blank
    } else if rec.parts.iter().all(|part| part.source_id.is_none()) {
        FillMode::Named
    } else {
        FillMode::Linked
    }
}

/// How a record meets the template's placeholders.
#[derive(Clone, Copy, Debug)]
pub enum FillMode {
    /// Inline payload: match the field name and ignore which source a placeholder names.
    Named,
    /// Linked record. Each placeholder uses the part from its own source. Other sources are blank.
    Linked,
    /// Empty grid cell: blank every placeholder in `maps`, with no warning.
    Blank,
}

enum Resolved {
    /// Leave the placeholder as it is.
    Skip,
    /// Blank it, with no warning.
    Quiet,
    Value {
        text: String,
        data_dir: Option<std::path::PathBuf>,
    },
}

fn resolve_placeholder(doc: &designcraft_doc::Document, rec: &CellRecord, mode: FillMode, source_id: u64, field: &str) -> Resolved {
    match mode {
        FillMode::Blank => Resolved::Quiet,
        FillMode::Named => {
            let Some(part) = rec.parts.first() else { return Resolved::Skip };
            if !part_has_field(part, field) && !is_virtual_field(field) {
                return Resolved::Skip;
            }
            Resolved::Value { text: resolve_field(part, rec.number, field), data_dir: part.data_dir.clone() }
        }
        FillMode::Linked => {
            if let Some(part) = rec.parts.iter().find(|part| part.source_id == Some(source_id)) {
                if !part.matched {
                    return Resolved::Quiet;
                }
                if !part_has_field(part, field) && !is_virtual_field(field) {
                    return Resolved::Skip;
                }
                return Resolved::Value { text: resolve_field(part, rec.number, field), data_dir: part.data_dir.clone() };
            }
            let source_known = doc.data_merge.sources.iter().any(|src| src.id == source_id);
            if !source_known
                && let Some(part) = rec.parts.iter().find(|part| part.matched && (part_has_field(part, field) || is_virtual_field(field)))
            {
                return Resolved::Value { text: resolve_field(part, rec.number, field), data_dir: part.data_dir.clone() };
            }
            if source_known { Resolved::Quiet } else { Resolved::Skip }
        }
    }
}

/// Fill `rec` into the items `maps` points at. `rec.number` is the 1-based number shown in warnings.
pub fn fill_row(
    doc: &mut designcraft_doc::Document,
    rec: &CellRecord,
    options: &MergeOptions,
    maps: &Maps,
    doc_dir: Option<&Path>,
    mode: FillMode,
) -> FillReport {
    let mut report = FillReport::default();
    let mut reps: Vec<Rep> = Vec::new();
    let mut links: Vec<(PlaceholderAnchor, HyperlinkDest, String)> = Vec::new();
    let mut inlines: Vec<InlineJob> = Vec::new();
    let placeholders = doc.data_merge.placeholders.clone();
    let record = rec.number;
    for ph in &placeholders {
        let resolved = resolve_placeholder(doc, rec, mode, ph.source_id, &ph.field);
        let quiet = matches!(resolved, Resolved::Quiet);
        let Resolved::Value { text: cell, data_dir } = (if quiet { Resolved::Value { text: String::new(), data_dir: None } } else { resolved })
        else {
            continue;
        };
        match &ph.anchor {
            PlaceholderAnchor::Text { story, start, end } => {
                let Some(&mapped) = maps.stories.get(story) else {
                    continue;
                };
                match ph.role {
                    PlaceholderRole::Text => reps.push(Rep { story: mapped, start: *start, end: *end, text: cell }),
                    PlaceholderRole::Hyperlink => {
                        if !quiet && let Some(dest) = classify_link(&cell) {
                            links.push((PlaceholderAnchor::Text { story: mapped, start: *start, end: *end }, dest, ph.field.clone()));
                        }
                    }
                    PlaceholderRole::Image => {
                        let loaded = if quiet { None } else { load_picture(&cell, data_dir.as_deref(), doc_dir) };
                        if !quiet && loaded.is_none() {
                            report.missing.push(MissingImage { record, field: ph.field.clone(), path: cell.clone() });
                            report.warnings.push(format!("Record {record}, field {}: missing image {cell}.", ph.field));
                        }
                        let layer = story_layer(doc, mapped);
                        let item = inline_picture_item(doc, loaded.as_ref(), layer, options.link_images);
                        inlines.push(InlineJob { story: mapped, start: *start, end: *end, item });
                        reps.push(Rep { story: mapped, start: *start, end: *end, text: OBJECT_MARK.to_string() });
                    }
                    PlaceholderRole::Qr => {}
                }
            }
            PlaceholderAnchor::Item { id } => {
                let Some(&mapped) = maps.items.get(id) else {
                    continue;
                };
                match ph.role {
                    PlaceholderRole::Image => {
                        if quiet {
                            clear_frame(doc, mapped);
                        } else {
                            place_image(doc, mapped, &ph.field, &cell, record, options, data_dir.as_deref(), doc_dir, &mut report);
                        }
                    }
                    PlaceholderRole::Qr => {
                        if quiet {
                            clear_frame(doc, mapped);
                        } else {
                            place_qr(doc, mapped, &ph.field, &cell, record, &mut report);
                        }
                    }
                    PlaceholderRole::Hyperlink => {
                        if !quiet && let Some(dest) = classify_link(&cell) {
                            links.push((PlaceholderAnchor::Item { id: mapped }, dest, ph.field.clone()));
                        }
                    }
                    PlaceholderRole::Text => {}
                }
            }
        }
    }
    if matches!(mode, FillMode::Blank) {
        typed_blank(doc, maps, &known_fields(doc), &mut reps);
    } else {
        typed_reps(doc, rec, mode, maps, &placeholders, &mut reps);
    }
    apply_reps(doc, &reps);
    attach_inlines(doc, &inlines, &reps);
    for (anchor, dest, field) in links {
        let anchor = match anchor {
            PlaceholderAnchor::Text { story, start, end } => {
                let (start, end) = adjust_range(story, start, end, &reps);
                PlaceholderAnchor::Text { story, start, end }
            }
            other => other,
        };
        if !create_link(doc, anchor, dest) {
            report.warnings.push(format!("Record {record}, field {field}: the hyperlink could not be created."));
        }
    }
    report
}

/// Parent items that would paint on document page `abs` and carry a placeholder or a `<<field>>` marker.
fn parent_targets(doc: &designcraft_doc::Document, abs: usize) -> Vec<Item> {
    let Some(page) = doc.page(abs) else { return Vec::new() };
    if !page.show_parent_items {
        return Vec::new();
    }
    let Some((ppi, ppage)) = doc.parent_page_for(abs) else { return Vec::new() };
    let Some(parent) = doc.parents.get(ppi) else { return Vec::new() };
    let mut out = Vec::new();
    for it in &parent.items {
        if it.hidden || page.overridden.contains(&it.id) {
            continue;
        }
        if !doc.layer(it.layer).is_some_and(|layer| layer.visible) {
            continue;
        }
        if parent.pages.len() > 1 && parent.page_at_x(it.bounds().center().x) != Some(ppage) {
            continue;
        }
        if item_has_placeholder(doc, it) {
            out.push((**it).clone());
        }
    }
    out
}

fn item_has_placeholder(doc: &designcraft_doc::Document, it: &Item) -> bool {
    let mut yes = false;
    it.walk(&mut |item| {
        if doc.data_merge.placeholders.iter().any(|ph| match &ph.anchor {
            PlaceholderAnchor::Item { id } => *id == item.id,
            PlaceholderAnchor::Text { story, .. } => matches!(&item.content, Content::Text(tf) if tf.story == *story),
        }) {
            yes = true;
        }
        if let Content::Text(tf) = &item.content
            && doc.story(tf.story).is_some_and(|st| st.text.contains("<<"))
        {
            yes = true;
        }
    });
    yes
}

fn override_parent_items(doc: &mut designcraft_doc::Document, abs: usize, ids: &[ItemId]) {
    let Some((si, pi)) = doc.page_loc(abs) else { return };
    let Some(sp) = doc.spreads.get_mut(si) else { return };
    let sp = Arc::make_mut(sp);
    let Some(page) = sp.pages.get_mut(pi) else { return };
    for id in ids {
        if !page.overridden.contains(id) {
            page.overridden.push(*id);
        }
    }
}

/// Copy placeholder-bearing parent items onto each output page and fill those copies.
/// The parent spread's own stories stay as they are.
pub fn fill_parent_copies(
    doc: &mut designcraft_doc::Document,
    pages: &[(usize, &CellRecord)],
    options: &MergeOptions,
    doc_dir: Option<&Path>,
) -> Result<FillReport, EngineError> {
    let mut report = FillReport::default();
    for (abs, rec) in pages {
        let items = parent_targets(doc, *abs);
        if items.is_empty() {
            continue;
        }
        let originals: Vec<ItemId> = items.iter().map(|it| it.id).collect();
        let snap = snapshot_of(doc, &items);
        let Some((si, _)) = doc.page_loc(*abs) else { continue };
        let page_x = doc.page(*abs).map(|p| p.x).unwrap_or(0.0);
        let parent_x =
            doc.parent_page_for(*abs).and_then(|(ppi, ppage)| doc.parents.get(ppi).and_then(|sp| sp.pages.get(ppage)).map(|p| p.x)).unwrap_or(0.0);
        let dx = page_x - parent_x;
        let mut maps = Maps::default();
        for it in &items {
            let xf = Affine::translate((dx, 0.0)) * it.xf;
            place_from_snapshot(doc, &snap, it, SpreadRef::Doc(si), xf, &mut maps)?;
        }
        override_parent_items(doc, *abs, &originals);
        let part = fill_row(doc, rec, options, &maps, doc_dir, fill_mode_of(rec));
        report.missing.extend(part.missing);
        report.warnings.extend(part.warnings);
    }
    Ok(report)
}

/// Fill parent placeholders where they sit. Preview uses this on a clone it can throw away.
pub fn fill_parents_in_place(doc: &mut designcraft_doc::Document, rec: &CellRecord, options: &MergeOptions, doc_dir: Option<&Path>) -> FillReport {
    let mut maps = Maps::default();
    for sp in &doc.parents {
        for it in &sp.items {
            it.walk(&mut |item| {
                maps.items.insert(item.id, item.id);
                if let Content::Text(tf) = &item.content {
                    maps.stories.insert(tf.story, tf.story);
                }
            });
        }
    }
    if maps.items.is_empty() && maps.stories.is_empty() {
        return FillReport::default();
    }
    fill_row(doc, rec, options, &maps, doc_dir, fill_mode_of(rec))
}

/// Pages in `start..end` that were given a record.
///
/// A record with no document items covers its whole span, so a parent-only template still fills.
/// When some pages in the span hold document items, only those pages were given the record.
/// A blank sibling page was not.
pub fn pages_given_record(doc: &designcraft_doc::Document, start: usize, end: usize) -> Vec<usize> {
    let span: Vec<usize> = (start..end).filter(|abs| doc.page(*abs).is_some()).collect();
    let with_items: Vec<usize> = span.iter().copied().filter(|&abs| page_has_items(doc, abs)).collect();
    if with_items.is_empty() { span } else { with_items }
}

fn page_has_items(doc: &designcraft_doc::Document, abs: usize) -> bool {
    let Some((si, pi)) = doc.page_loc(abs) else { return false };
    let Some(sp) = doc.spreads.get(si) else { return false };
    sp.items.iter().any(|it| sp.page_at_x(it.bounds().center().x) == Some(pi))
}

/// Fill parent placeholders from `rec` on the pages that record covers.
/// Returns whether a page that shows a parent placeholder was given no record.
pub fn fill_shown_parents(
    doc: &mut designcraft_doc::Document,
    rec: &CellRecord,
    options: &MergeOptions,
    doc_dir: Option<&Path>,
) -> Result<bool, EngineError> {
    let given = pages_given_record(doc, 0, doc.page_count());
    let missing = (0..doc.page_count()).any(|abs| !given.contains(&abs) && !parent_targets(doc, abs).is_empty());
    if missing {
        let pairs: Vec<(usize, &CellRecord)> = given.iter().map(|&page| (page, rec)).collect();
        if !pairs.is_empty() {
            fill_parent_copies(doc, &pairs, options, doc_dir)?;
        }
        Ok(true)
    } else {
        let _ = fill_parents_in_place(doc, rec, options, doc_dir);
        Ok(false)
    }
}

/// A document page still shows a parent placeholder because that page was given no record.
pub fn has_unfilled_parent(doc: &designcraft_doc::Document) -> bool {
    (0..doc.page_count()).any(|abs| !parent_targets(doc, abs).is_empty())
}

fn known_fields(doc: &designcraft_doc::Document) -> Vec<String> {
    let mut names = Vec::new();
    for src in &doc.data_merge.sources {
        for field in &src.fields {
            if !names.iter().any(|name| name == &field.name) {
                names.push(field.name.clone());
            }
        }
    }
    for ph in &doc.data_merge.placeholders {
        if !names.iter().any(|name| name == &ph.field) {
            names.push(ph.field.clone());
        }
    }
    names
}

fn typed_blank(doc: &designcraft_doc::Document, maps: &Maps, names: &[String], reps: &mut Vec<Rep>) {
    let mut stories: Vec<(StoryId, StoryId)> = maps.stories.iter().map(|(a, b)| (*a, *b)).collect();
    stories.sort_by_key(|(a, _)| a.0);
    stories.dedup();
    for (_original, mapped) in stories {
        let Some(st) = doc.story(mapped) else { continue };
        for (start, end, name) in find_markers(&st.text) {
            if names.iter().any(|known| known == &name) {
                reps.push(Rep { story: mapped, start, end, text: String::new() });
            }
        }
    }
}

fn clear_frame(doc: &mut designcraft_doc::Document, item: ItemId) {
    if let Some(it) = doc.item_mut(item) {
        it.content = Content::Unassigned;
    }
}

fn typed_reps(
    doc: &designcraft_doc::Document,
    rec: &CellRecord,
    mode: FillMode,
    maps: &Maps,
    placeholders: &[designcraft_doc::Placeholder],
    reps: &mut Vec<Rep>,
) {
    let mut stories: Vec<(StoryId, StoryId)> = maps.stories.iter().map(|(a, b)| (*a, *b)).collect();
    stories.sort_by_key(|(a, _)| a.0);
    stories.dedup();
    for (original, mapped) in stories {
        let Some(st) = doc.story(mapped) else { continue };
        let covers: Vec<(usize, usize)> = placeholders
            .iter()
            .filter_map(|ph| match (&ph.role, &ph.anchor) {
                (PlaceholderRole::Text | PlaceholderRole::Image, PlaceholderAnchor::Text { story, start, end }) if *story == original => {
                    Some((*start, *end))
                }
                _ => None,
            })
            .collect();
        for (start, end, name) in find_markers(&st.text) {
            if covers.iter().any(|(a, b)| start < *b && end > *a) {
                continue;
            }
            if let Some(text) = marker_text(doc, rec, mode, &name) {
                reps.push(Rep { story: mapped, start, end, text });
            }
        }
    }
}

/// `Some` replaces the marker. `None` leaves it, which is a name this record does not know.
fn marker_text(doc: &designcraft_doc::Document, rec: &CellRecord, mode: FillMode, name: &str) -> Option<String> {
    match mode {
        FillMode::Blank => None,
        FillMode::Named => {
            let part = rec.parts.first()?;
            if part_has_field(part, name) || is_virtual_field(name) { Some(resolve_field(part, rec.number, name)) } else { None }
        }
        FillMode::Linked => {
            if let Some(part) = rec.parts.iter().find(|part| part.matched && part_has_field(part, name)) {
                return Some(resolve_field(part, rec.number, name));
            }
            if is_virtual_field(name)
                && let Some(part) = rec.parts.iter().find(|part| part.matched)
            {
                return Some(resolve_field(part, rec.number, name));
            }
            if doc.data_merge.sources.iter().any(|src| src.fields.iter().any(|field| field.name == name)) { Some(String::new()) } else { None }
        }
    }
}

fn find_markers(text: &str) -> Vec<(usize, usize, String)> {
    let mut out = Vec::new();
    let mut i = 0;
    while i < text.len() {
        if text[i..].starts_with("<<")
            && let Some(rel) = text[i + 2..].find(">>")
        {
            let name = &text[i + 2..i + 2 + rel];
            if !name.is_empty() && !name.contains(['<', '>', '\n']) {
                let end = i + 2 + rel + 2;
                out.push((i, end, name.to_string()));
                i = end;
                continue;
            }
        }
        i += text[i..].chars().next().map(|c| c.len_utf8()).unwrap_or(1);
    }
    out
}

fn story_layer(doc: &designcraft_doc::Document, story: StoryId) -> designcraft_doc::LayerId {
    doc.story(story).and_then(|st| st.frames.first().copied()).and_then(|id| doc.item(id)).map(|it| it.layer).unwrap_or_else(|| doc.default_layer())
}

fn clamp_long_side(px: (u32, u32)) -> (f64, f64) {
    let w = f64::from(px.0);
    let h = f64::from(px.1);
    let long = w.max(h);
    if !long.is_finite() || long <= 144.0 || long <= 0.0 {
        return (w.max(0.0), h.max(0.0));
    }
    let scale = 144.0 / long;
    (w * scale, h * scale)
}

fn inline_picture_item(
    doc: &mut designcraft_doc::Document,
    loaded: Option<&LoadedPicture>,
    layer: designcraft_doc::LayerId,
    link_images: bool,
) -> Item {
    let (w, h, graphic) = if let Some(pic) = loaded {
        let (w, h) = clamp_long_side(pic.px);
        let aid = AssetId(doc.alloc());
        let link = link_images.then(|| pic.link.clone()).flatten();
        doc.assets.insert(
            aid,
            Arc::new(Asset {
                id: aid,
                name: pic.name.clone(),
                mime: designcraft_render::image_mime(&pic.bytes).to_string(),
                link,
                data: Arc::new(pic.bytes.clone()),
                pixels: Some(pic.px),
                page: 0,
            }),
        );
        let graphic = Graphic { asset: aid, size: (w, h), xf: Affine::IDENTITY, auto_fit: Fitting::None, fit_align: 0, crop: [0.0; 4] };
        (w, h, Some(graphic))
    } else {
        (0.0, 0.0, None)
    };
    let id = ItemId(doc.alloc());
    let mut item = Item::new(id, layer, Shape::Rectangle, designcraft_geom::shapes::rectangle(Rect::new(0.0, 0.0, w, h)));
    item.content = match graphic {
        Some(g) => Content::Graphic(g),
        None => Content::Unassigned,
    };
    item
}

fn attach_inlines(doc: &mut designcraft_doc::Document, jobs: &[InlineJob], reps: &[Rep]) {
    for job in jobs {
        let (at, _) = adjust_range(job.story, job.start, job.end, reps);
        let Some(st) = doc.story_mut(job.story) else { continue };
        let at = at.min(st.text.len());
        if !st.text[at..].starts_with(OBJECT_MARK) {
            continue;
        }
        let index = st.text[..at].matches(OBJECT_MARK).count();
        if let Some(slot) = st.objects.get_mut(index) {
            *slot = Arc::new(AnchoredObject::new(job.item.clone(), AnchorPosition::Inline { y_offset: 0.0 }));
        }
    }
}

fn apply_reps(doc: &mut designcraft_doc::Document, reps: &[Rep]) {
    let mut stories: Vec<StoryId> = reps.iter().map(|r| r.story).collect();
    stories.sort();
    stories.dedup();
    for sid in stories {
        let mut mine: Vec<&Rep> = reps.iter().filter(|r| r.story == sid).collect();
        mine.sort_by_key(|r| std::cmp::Reverse(r.start));
        let Some(st) = doc.story_mut(sid) else { continue };
        for r in mine {
            if r.start <= r.end && r.end <= st.text.len() {
                st.replace(r.start..r.end, &r.text);
            }
        }
    }
}

fn adjust_range(story: StoryId, start: usize, end: usize, reps: &[Rep]) -> (usize, usize) {
    let mut mine: Vec<&Rep> = reps.iter().filter(|r| r.story == story).collect();
    mine.sort_by_key(|r| r.start);
    let mut s = start;
    let mut e = end;
    for r in mine {
        let old = r.end.saturating_sub(r.start);
        let new = r.text.len();
        let delta = new as isize - old as isize;
        if r.end <= start {
            s = add_delta(s, delta);
            e = add_delta(e, delta);
        } else if r.start == start && r.end == end {
            e = s.saturating_add(new);
        } else if r.start >= end {
        } else if r.end <= end {
            e = add_delta(e, delta);
        }
    }
    (s, e)
}

pub fn add_delta(pos: usize, delta: isize) -> usize {
    if delta >= 0 { pos.saturating_add(delta as usize) } else { pos.saturating_sub(delta.unsigned_abs()) }
}

fn create_link(doc: &mut designcraft_doc::Document, anchor: PlaceholderAnchor, dest: HyperlinkDest) -> bool {
    let source = match anchor {
        PlaceholderAnchor::Item { id } => {
            if doc.item(id).is_none() {
                return false;
            }
            HyperlinkSource::Item { id }
        }
        PlaceholderAnchor::Text { story, start, end } => {
            let Some(st) = doc.story(story) else { return false };
            if start > end || end > st.text.len() {
                return false;
            }
            HyperlinkSource::Text { story, start, end }
        }
    };
    let id = doc.alloc();
    let name = match &dest {
        HyperlinkDest::Url(u) | HyperlinkDest::Email(u) => u.clone(),
        HyperlinkDest::Page(p) => format!("Page {}", p + 1),
    };
    doc.hyperlinks.push(Hyperlink { id, name, source, dest });
    true
}

struct LoadedPicture {
    bytes: Vec<u8>,
    name: String,
    link: Option<String>,
    px: (u32, u32),
}

fn load_picture(cell: &str, data_dir: Option<&Path>, doc_dir: Option<&Path>) -> Option<LoadedPicture> {
    if let Some(url) = super::fetch::remote_url(cell) {
        let bytes = super::fetch::fetch_url(url).ok()?;
        if bytes.is_empty() || bytes.len() as u64 > super::MAX_IMAGE_FILE_BYTES {
            return None;
        }
        let px = designcraft_render::image_size(&bytes)?;
        let name = url.rsplit('/').next().filter(|s| !s.is_empty()).unwrap_or("image").to_string();
        return Some(LoadedPicture { bytes, name, link: Some(url.to_string()), px });
    }
    let cands = image_candidates(cell, data_dir, doc_dir);
    let (path, bytes) = cands.iter().find_map(|p| {
        // Refuse huge files before reading them; a too-large image counts as missing.
        let len = std::fs::metadata(p).ok()?.len();
        if len == 0 || len > super::MAX_IMAGE_FILE_BYTES {
            return None;
        }
        std::fs::read(p).ok().filter(|b| !b.is_empty()).map(|b| (p.clone(), b))
    })?;
    let px = designcraft_render::image_size(&bytes)?;
    let name = path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_else(|| "image".to_string());
    Some(LoadedPicture { bytes, name, link: Some(path.to_string_lossy().into_owned()), px })
}

fn place_image(
    doc: &mut designcraft_doc::Document,
    item: ItemId,
    field: &str,
    cell: &str,
    record: u32,
    options: &MergeOptions,
    data_dir: Option<&Path>,
    doc_dir: Option<&Path>,
    report: &mut FillReport,
) {
    let Some(loaded) = load_picture(cell, data_dir, doc_dir) else {
        if let Some(it) = doc.item_mut(item) {
            it.content = Content::Unassigned;
        }
        report.missing.push(MissingImage { record, field: field.to_string(), path: cell.to_string() });
        report.warnings.push(format!("Record {record}, field {field}: missing image {cell}."));
        return;
    };
    let (pw, ph) = loaded.px;
    let bytes = loaded.bytes;
    let Some(inner) = doc.item(item).map(|it| it.inner_bounds()) else {
        report.warnings.push(format!("Record {record}, field {field}: missing image {cell}."));
        report.missing.push(MissingImage { record, field: field.to_string(), path: cell.to_string() });
        return;
    };
    let (nw, nh) = (f64::from(pw), f64::from(ph));
    let mode = fitting_of(&options.fitting);
    let aid = AssetId(doc.alloc());
    let name = if loaded.name.is_empty() { field.to_string() } else { loaded.name.clone() };
    let link = options.link_images.then(|| loaded.link.clone()).flatten();
    doc.assets.insert(
        aid,
        Arc::new(Asset {
            id: aid,
            name,
            mime: designcraft_render::image_mime(&bytes).to_string(),
            link,
            data: Arc::new(bytes),
            pixels: Some((pw, ph)),
            page: 0,
        }),
    );
    let mut g = Graphic { asset: aid, size: (nw, nh), xf: Affine::translate((inner.x0, inner.y0)), auto_fit: mode, fit_align: 0, crop: [0.0; 4] };
    if let Some(xf) = g.fitted(inner, mode) {
        g.xf = xf;
    }
    if options.center && nw > 0.0 && nh > 0.0 {
        let bb = g.xf.transform_rect_bbox(Rect::new(0.0, 0.0, nw, nh));
        let c = inner.center();
        let m = bb.center();
        g.xf = Affine::translate((c.x - m.x, c.y - m.y)) * g.xf;
    }
    if let Some(it) = doc.item_mut(item) {
        it.content = Content::Graphic(g);
    }
}

fn place_qr(doc: &mut designcraft_doc::Document, item: ItemId, field: &str, cell: &str, record: u32, report: &mut FillReport) {
    if cell.is_empty() {
        report.warnings.push(format!("Record {record}, field {field}: the QR cell is empty."));
        return;
    }
    let Some(it) = doc.item(item).cloned() else {
        report.warnings.push(format!("Record {record}, field {field}: the QR content does not fit."));
        return;
    };
    let Some(path) = designcraft_geom::qr::qr_path(cell, it.inner_bounds()) else {
        report.warnings.push(format!("Record {record}, field {field}: the QR content does not fit."));
        return;
    };
    let id = ItemId(doc.alloc());
    let mut code = Item::new(id, it.layer, Shape::Path, path);
    code.fill = Fill::swatch(designcraft_color::swatch::BLACK);
    code.alt_text = format!("QR code: {cell}");
    if let Some(frame) = doc.item_mut(item) {
        frame.content = Content::Group { items: vec![Arc::new(code)] };
    }
}

fn fitting_of(s: &str) -> Fitting {
    match s {
        "fillProportionally" => Fitting::FillProportionally,
        "fitContentToFrame" => Fitting::FitContentToFrame,
        "none" => Fitting::None,
        _ => Fitting::FitProportionally,
    }
}

fn classify_link(cell: &str) -> Option<HyperlinkDest> {
    let value = cell.trim();
    if value.is_empty() {
        return None;
    }
    // An explicit scheme must be a web or mail one; `javascript:`, `file:` and the like stay text.
    if let Some((scheme, _)) = value.split_once(':') {
        let is_scheme = scheme.chars().next().is_some_and(|c| c.is_ascii_alphabetic())
            && scheme.chars().all(|c| c.is_ascii_alphanumeric() || matches!(c, '+' | '-' | '.'));
        if is_scheme {
            let scheme = scheme.to_ascii_lowercase();
            return matches!(scheme.as_str(), "http" | "https" | "ftp" | "mailto").then(|| HyperlinkDest::Url(value.to_string()));
        }
    }
    if value.contains("://") || value.chars().any(char::is_whitespace) {
        return Some(HyperlinkDest::Url(value.to_string()));
    }
    if value.matches('@').count() == 1 && !value.contains(':') {
        return Some(HyperlinkDest::Email(value.to_string()));
    }
    Some(HyperlinkDest::Url(value.to_string()))
}

#[cfg(test)]
mod link_tests {
    use super::*;

    #[test]
    fn only_web_and_mail_schemes_become_links() {
        for ok in ["https://example.com", "HTTP://example.com", "ftp://files.example.com", "mailto:a@example.com", "www.example.com"] {
            assert!(matches!(classify_link(ok), Some(HyperlinkDest::Url(_))), "{ok}");
        }
        assert!(matches!(classify_link("a@example.com"), Some(HyperlinkDest::Email(_))));
        for bad in ["javascript:alert(1)", "file:///etc/passwd", "data:text/html,hi", "vbscript:x", ""] {
            assert!(classify_link(bad).is_none(), "{bad}");
        }
    }
}
