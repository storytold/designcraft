//! The document knowledge the hub validates against (spec §3.6 phase A: `DocSchema`): which
//! kinds exist, which parents accept children, the per-entry document lists with their orders,
//! and — DesignCraft's own row in the adoption table — the `PageRange` region-lock shape the
//! `regionLocks` document property must fit (§6.7, §18.4 row 4).

use craft_collab::adapter::ValidationCtx;
use craft_collab::change::{Change, EntryId};
use craft_collab::limits::Limits;
use craft_collab::value::Value;
use designcraft_color::Swatch;
use designcraft_doc::{Document, Layer, Section, StoryId};

use crate::props;

/// The designcraft document schema.
#[derive(Debug, Default)]
pub struct DesignSchema {
    limits: Limits,
}

impl craft_collab::adapter::DocSchema for DesignSchema {
    fn validate(&self, change: &Change, ctx: &ValidationCtx<'_>) -> Result<(), String> {
        match change {
            Change::Create { kind, parent, props, .. } => {
                if !crate::KINDS.contains(&kind.as_ref()) {
                    return Err(format!("unknown kind '{kind}'"));
                }
                if *parent == crate::ROOT {
                    // Only spreads attach to the file root — document spreads and parent
                    // ("master") spreads alike; pages and items hang off spreads.
                    if !matches!(kind.as_ref(), "spread" | "parentSpread") {
                        return Err(format!("'{kind}' does not attach to the file root; only spreads do"));
                    }
                } else {
                    let p = ctx.replica.node(*parent).ok_or_else(|| format!("unknown parent {parent:?}"))?;
                    let ok = match p.kind.as_ref() {
                        "spread" | "parentSpread" => matches!(
                            kind.as_ref(),
                            "page" | "textFrame" | "graphic" | "group" | "rectangle" | "ellipse" | "polygon" | "line" | "path"
                        ),
                        // A group accepts items; a page accepts none (items live in spread
                        // space — the page-item association is geometric, not structural).
                        "group" => matches!(kind.as_ref(), "textFrame" | "graphic" | "group" | "rectangle" | "ellipse" | "polygon" | "line" | "path"),
                        _ => false,
                    };
                    if !ok {
                        return Err(format!("a '{}' accepts no '{}'", p.kind, kind));
                    }
                }
                for (p, _) in props {
                    if p.as_ref() != "parent" && p.as_ref() != "position" && !kind_props(kind.as_ref()).contains(&p.as_ref()) {
                        return Err(format!("unknown property '{p}' for a '{kind}'"));
                    }
                }
                Ok(())
            }
            Change::Set { id, prop, .. } => {
                if prop.as_ref() == "parent" || prop.as_ref() == "position" {
                    return Ok(());
                }
                // The core itself checks that ids exist; when the replica knows the node we add
                // the kind's own property table on top.
                if let Some(node) = ctx.replica.node(*id) {
                    let kind = node.kind.as_ref();
                    if !kind_props(kind).contains(&prop.as_ref()) {
                        return Err(format!("unknown property '{prop}' for a '{kind}'"));
                    }
                }
                Ok(())
            }
            Change::Delete { .. } => Ok(()),
            Change::SetDoc { prop, value } => {
                if is_order_prop(prop) {
                    return Ok(());
                }
                match prop.as_ref() {
                    "title" | "settings" | "styles" | "nextId" => Ok(()),
                    // §6.7: locks sync as document properties; the shape is the contract.
                    crate::locks::RegionLock::PROP => match crate::locks::parse_locks(value) {
                        Ok(_) => Ok(()),
                        Err(e) => Err(e),
                    },
                    _ => Err(format!("unknown document property '{prop}' (value {value:?})")),
                }
            }
            Change::Put { entry, .. } | Change::Remove { entry } => {
                if entry_kind(&entry.0).is_some() {
                    Ok(())
                } else {
                    Err(format!("unknown entry kind in '{entry:?}'"))
                }
            }
        }
    }

    fn recompute(&self, _touched: &[craft_collab::ids::NodeId]) -> Vec<Change> {
        // The honest answer, like the figcraft pilot's: DesignCraft's layout is recompute-heavy,
        // but both of its derived passes run as explicit engine machinery today, not as document
        // properties that normalize on their own —
        //
        // * `Spread::relayout` (the pages' `x` positions from their widths and the binding) is
        //   called by the page commands in `crates/engine/src/cmd/layout.rs`, not by the model;
        // * text composition (line breaking, overset) lives in the render-side compose cache,
        //   keyed by each story's `rev` — it is derived data that never syncs at all.
        //
        // So the derived positions sync as *plain properties*: a local relayout's output shows
        // up in the diff as `Set(x)` changes on the touched pages, and the receiving client
        // writes exactly those values. When a genuinely derived property lands (auto-growing
        // frames, live captions), its recompute belongs here and nowhere else (spec §5.7: the
        // author alone recomputes, so no two clients ever fight over a derived value).
        Vec::new()
    }

    fn limits(&self) -> &Limits {
        &self.limits
    }
}

/// The property table of a kind string.
fn kind_props(kind: &str) -> &'static [&'static str] {
    match kind {
        "spread" | "parentSpread" => props::SPREAD_PROPS,
        "page" => props::PAGE_PROPS,
        _ => {
            // The item kinds share one table (ITEM_PROPS plus the elided `content` property).
            // A static table for the concatenation keeps validate allocation-free.
            static ITEM_ALL: std::sync::OnceLock<Vec<&'static str>> = std::sync::OnceLock::new();
            ITEM_ALL.get_or_init(props::all_item_props)
        }
    }
}

/// Which list an entry id belongs to.
fn entry_kind(entry: &str) -> Option<&'static str> {
    if entry.starts_with("Story:") {
        Some("story")
    } else if entry.starts_with("Layer:") {
        Some("layer")
    } else if entry.starts_with("Swatch:") {
        Some("swatch")
    } else if entry.starts_with("Section:") {
        Some("section")
    } else {
        None
    }
}

/// Whether `prop` is one of the per-list order properties.
fn is_order_prop(prop: &str) -> bool {
    matches!(prop, "layerOrder" | "swatchOrder" | "sectionOrder")
}

/// Upsert one document-level entry; wrong shapes are errors, never panics.
pub fn put_entry(doc: &mut Document, entry: &EntryId, value: &Value) -> Result<(), String> {
    let json = value.to_json();
    let key = entry.0.as_ref();
    match entry_kind(key) {
        Some("story") => {
            let story: designcraft_doc::Story = serde_json::from_value(json).map_err(|e| e.to_string())?;
            let sid = story.id;
            if format!("Story:{}", sid.0) != key {
                return Err(format!("story id {sid} does not match entry '{key}'"));
            }
            doc.stories.insert(sid, std::sync::Arc::new(story));
            Ok(())
        }
        Some("layer") => {
            let layer: Layer = serde_json::from_value(json).map_err(|e| e.to_string())?;
            let n = key.strip_prefix("Layer:").and_then(|t| t.parse::<u64>().ok()).ok_or_else(|| format!("malformed layer id '{key}'"))?;
            if layer.id.0 != n {
                return Err(format!("layer id {} does not match entry '{key}'", layer.id.0));
            }
            match doc.layers.iter_mut().find(|l| l.id.0 == n) {
                Some(slot) => *slot = layer,
                None => doc.layers.push(layer),
            }
            Ok(())
        }
        Some("swatch") => {
            let swatch: Swatch = serde_json::from_value(json).map_err(|e| e.to_string())?;
            let name = key.strip_prefix("Swatch:").unwrap_or(key);
            if swatch.name != name {
                return Err(format!("swatch name '{}' does not match entry '{key}'", swatch.name));
            }
            match doc.swatches.iter_mut().find(|s| s.name == name) {
                Some(slot) => *slot = swatch,
                None => doc.swatches.push(swatch),
            }
            Ok(())
        }
        Some("section") => {
            let section: Section = serde_json::from_value(json).map_err(|e| e.to_string())?;
            let start = key.strip_prefix("Section:").and_then(|t| t.parse::<usize>().ok()).ok_or_else(|| format!("malformed section id '{key}'"))?;
            if section.start != start {
                return Err(format!("section start {} does not match entry '{key}'", section.start));
            }
            match doc.sections.iter_mut().find(|s| s.start == start) {
                Some(slot) => *slot = section,
                None => doc.sections.push(section),
            }
            Ok(())
        }
        _ => Err(format!("unknown entry kind in '{key}'")),
    }
}

/// Remove one document-level entry (absent entries are fine — the tolerant v2 rule).
pub fn remove_entry(doc: &mut Document, entry: &EntryId) -> Result<(), String> {
    let key = entry.0.as_ref();
    match entry_kind(key) {
        Some("story") => {
            let n = key.strip_prefix("Story:").and_then(|t| t.parse().ok()).ok_or_else(|| format!("malformed story id '{key}'"))?;
            doc.stories.remove(&StoryId(n));
            Ok(())
        }
        Some("layer") => {
            doc.layers.retain(|l| format!("Layer:{}", l.id.0) != key);
            Ok(())
        }
        Some("swatch") => {
            let name = key.strip_prefix("Swatch:").unwrap_or(key);
            doc.swatches.retain(|s| s.name != name);
            Ok(())
        }
        Some("section") => {
            doc.sections.retain(|s| format!("Section:{}", s.start) != key);
            Ok(())
        }
        _ => Err(format!("unknown entry kind in '{key}'")),
    }
}

/// Reorder one document-level list from an order property's value (entries the array misses keep
/// their relative order at the end — lenient, like the client replay).
pub fn set_doc_order(doc: &mut Document, prop: &str, value: &Value) -> Option<Result<(), String>> {
    let order: Vec<String> = match serde_json::from_value(value.to_json()) {
        Ok(v) => v,
        Err(e) => return Some(Err(e.to_string())),
    };
    match prop {
        "layerOrder" => Some(reorder(&mut doc.layers, |l| format!("Layer:{}", l.id.0), &order)),
        "swatchOrder" => Some(reorder(&mut doc.swatches, |s| format!("Swatch:{}", s.name), &order)),
        "sectionOrder" => Some(reorder(&mut doc.sections, |s| format!("Section:{}", s.start), &order)),
        _ => None,
    }
}

/// Reorder `list` so the entries whose keys appear in `order` come first in that order.
fn reorder<T, K: PartialEq + Clone>(list: &mut Vec<T>, key: fn(&T) -> K, order: &[K]) -> Result<(), String> {
    let mut decorated: Vec<(usize, T)> = std::mem::take(list).into_iter().map(|item| (order.len() + 1, item)).collect();
    for (slot, item) in decorated.iter_mut() {
        let k = key(item);
        if let Some(i) = order.iter().position(|o| *o == k) {
            *slot = i;
        }
    }
    decorated.sort_by_key(|(slot, _)| *slot);
    *list = decorated.into_iter().map(|(_, item)| item).collect();
    Ok(())
}

/// The document-level entries of `doc`, as `(entry id, value)` pairs for the view.
pub fn entry_values(doc: &Document) -> Vec<(String, Value)> {
    let mut out = Vec::new();
    for story in doc.stories.values() {
        if let Some(v) = serde_json::to_value(&**story).ok().map(Value::Json) {
            out.push((format!("Story:{}", story.id.0), v));
        }
    }
    for layer in &doc.layers {
        if let Some(v) = serde_json::to_value(layer).ok().map(Value::Json) {
            out.push((format!("Layer:{}", layer.id.0), v));
        }
    }
    for swatch in &doc.swatches {
        if let Some(v) = serde_json::to_value(swatch).ok().map(Value::Json) {
            out.push((format!("Swatch:{}", swatch.name), v));
        }
    }
    for section in &doc.sections {
        if let Some(v) = serde_json::to_value(section).ok().map(Value::Json) {
            out.push((format!("Section:{}", section.start), v));
        }
    }
    out
}

/// Diff the document-level lists between two documents: per-entry `Put`/`Remove`, plus one
/// order property per list whose order is real state (the story map's order is its id order —
/// derived, not state — so stories carry no order property).
pub fn diff_entries(before: &Document, after: &Document) -> Vec<Change> {
    let mut out = Vec::new();
    let layers = |d: &Document| d.layers.iter().map(|l| (format!("Layer:{}", l.id.0), ser(l))).collect::<Vec<_>>();
    diff_list(&layers(before), &layers(after), Some("layerOrder"), &mut out);
    let swatches = |d: &Document| d.swatches.iter().map(|s| (format!("Swatch:{}", s.name), ser(s))).collect::<Vec<_>>();
    diff_list(&swatches(before), &swatches(after), Some("swatchOrder"), &mut out);
    let sections = |d: &Document| d.sections.iter().map(|s| (format!("Section:{}", s.start), ser(s))).collect::<Vec<_>>();
    diff_list(&sections(before), &sections(after), Some("sectionOrder"), &mut out);
    // Stories: a map keyed by id — no order property, just per-entry Puts and Removes.
    let stories = |d: &Document| d.stories.values().map(|s| (format!("Story:{}", s.id.0), ser(&**s))).collect::<Vec<_>>();
    diff_list(&stories(before), &stories(after), None, &mut out);
    out
}

/// Serialize one entry value (serialization failure becomes `None`, which re-sends nothing —
/// these types are plain data, so it cannot happen in practice).
fn ser<T: serde::Serialize>(v: &T) -> Option<Value> {
    serde_json::to_value(v).ok().map(Value::Json)
}

/// Diff one keyed list (as serialized pairs): value `Put`s for new or changed entries, `Remove`s
/// for gone ones, and one `SetDoc` when the order changed and `order_prop` is `Some` (only the
/// lists whose panel order is real state carry one).
fn diff_list(b: &[(String, Option<Value>)], a: &[(String, Option<Value>)], order_prop: Option<&str>, out: &mut Vec<Change>) {
    let b_keys: Vec<&str> = b.iter().map(|(k, _)| k.as_str()).collect();
    let a_keys: Vec<&str> = a.iter().map(|(k, _)| k.as_str()).collect();
    if b_keys != a_keys
        && let Some(prop) = order_prop
    {
        out.push(Change::SetDoc { prop: prop.into(), value: Value::Json(serde_json::json!(a_keys)) });
    }
    for (k, v) in a {
        let changed = match (b.iter().find(|(bk, _)| bk == k), v) {
            (Some((_, Some(bv))), Some(av)) => bv != av,
            (Some((_, None)), Some(_)) => true,
            (None, Some(_)) => true,
            (_, None) => continue,
        };
        if changed && let Some(v) = v {
            out.push(Change::Put { entry: EntryId(k.as_str().into()), value: v.clone() });
        }
    }
    for (k, _) in b {
        if !a_keys.contains(&k.as_str()) {
            out.push(Change::Remove { entry: EntryId(k.as_str().into()) });
        }
    }
}
