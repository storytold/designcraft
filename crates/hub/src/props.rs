//! The synchronized property tables (spec §2.3: "every field the file saves is a property").
//!
//! DesignCraft's model has three synchronized object types — [`Item`] (page items, including
//! groups), [`Page`] and [`Spread`] — each enumerated field by field with a compile-time guard:
//! a struct gaining a field that doesn't sync is a build error, not a silent divergence. Big or
//! list-valued fields (paths, effects, story formatting) ride as one last-writer-wins value —
//! the v2 decision, kept verbatim from the figcraft pilot.
//!
//! Two fields needed honest renames on the wire, both to keep the generic tree's own
//! `parent`/`position` properties unambiguous (spec §2.3: the tree position is not a field):
//!
//! * `Spread::parent` (the parent-spread *metadata*, `A-Parent`) syncs as `parentInfo`;
//! * `Page::parent` (the applied parent spread id) syncs as `parentSpread`.
//!
//! One field needed honest surgery: `Item::content`. Its `Group` variant *contains the
//! children* — serializing it wholesale would re-send a whole subtree on every child edit (the
//! dense-path hazard the family measures in vectorcraft). So `content` syncs with the group
//! payload elided: a group is `{"type":"group"}` and nothing else, and the children ride as
//! their own objects. The guard still covers the field — it cannot silently drift.

use craft_collab::Value;
use designcraft_doc::{Item, Page, Spread};

/// One synchronized object type's property table: fields and the property names they sync under.
///
/// `$skip` lists the identity/children fields the guard must cover but that do not sync: `id`
/// (the object's identity), and for `Spread` the `pages`/`items` lists (children sync as their
/// own objects, never as fields).
macro_rules! obj_props {
    ($guard:ident, $ty:ident, $props:ident, $value:ident, $set:ident, $differs:ident, ($($skip:ident),*); $(($field:ident, $name:literal)),+ $(,)?) => {
        /// Every field of the object that syncs, by property name.
        pub const $props: &[&str] = &[$($name),+];

        /// The value of field property `prop`; `None` when `prop` is not a field of this type.
        ///
        /// The tree position is not a field: the adapter owns the position keys and the tree
        /// lives in the document (spec §2.3).
        pub fn $value(obj: &$ty, prop: &str) -> Option<Value> {
            match prop {
                $($name => Some(Value::Json(serde_json::to_value(&obj.$field).ok()?)),)+
                _ => None,
            }
        }

        /// Write field property `prop`; `None` when `prop` is not a field of this type.
        ///
        /// A value of the wrong shape is an error, not a panic — shipped code never crashes.
        pub fn $set(obj: &mut $ty, prop: &str, value: &Value) -> Option<Result<(), String>> {
            match prop {
                $($name => Some(
                    serde_json::from_value(value.to_json())
                        .map(|v| obj.$field = v)
                        .map_err(|e| e.to_string()),
                ),)+
                _ => None,
            }
        }

        /// Whether field property `prop` differs between `a` and `b`; `None` if not a field.
        pub fn $differs(a: &$ty, b: &$ty, prop: &str) -> Option<bool> {
            match prop {
                $($name => Some(a.$field != b.$field),)+
                _ => None,
            }
        }

        /// Fails to compile when the struct gains a field that isn't listed as a property above
        /// (or in `$skip`) — the v2 guard, ported verbatim: a new saved field must decide how it
        /// syncs.
        #[allow(dead_code)]
        fn $guard(n: &$ty) {
            let $ty { $($field: _),+, $($skip: _),* } = n;
        }
    };
}

obj_props! {
    every_item_field_is_a_property, Item, ITEM_PROPS, item_value, set_item_field, item_field_differs, (id, content);
    (name, "name"),
    (layer, "layer"),
    (shape, "shape"),
    (path, "path"),
    (xf, "xf"),
    (fill, "fill"),
    (stroke, "stroke"),
    (corners, "corners"),
    (opacity, "opacity"),
    (blend, "blend"),
    (effects, "effects"),
    (wrap, "wrap"),
    (locked, "locked"),
    (hidden, "hidden"),
    (nonprinting, "nonprinting"),
    (object_style, "objectStyle"),
    (overrides, "overrides"),
    (label, "label"),
    (alt_text, "altText"),
    (liquid, "liquid"),
    (export_options, "exportOptions"),
    (form_field, "formField"),
    (live_caption, "liveCaption"),
    (pdf_hidden_layers, "pdfHiddenLayers"),
    (media, "media"),
    (button, "button"),
    (states, "states"),
    (active_state, "activeState"),
    (xml_tag, "xmlTag"),
    (isolate, "isolate"),
    (knockout, "knockout"),
}

obj_props! {
    every_page_field_is_a_property, Page, PAGE_PROPS, page_value, set_page_field, page_field_differs, (id);
    (width, "width"),
    (height, "height"),
    (x, "x"),
    (margins, "margins"),
    (columns, "columns"),
    (parent, "parentSpread"),
    (side, "side"),
    (overridden, "overridden"),
    (guides, "guides"),
    (show_parent_items, "showParentItems"),
    (liquid, "liquid"),
    (transition, "transition"),
    (view_rotation, "viewRotation"),
    (show_parent_overlay, "showParentOverlay"),
}

obj_props! {
    every_spread_field_is_a_property, Spread, SPREAD_PROPS, spread_value, set_spread_field, spread_field_differs, (id, pages, items);
    (parent, "parentInfo"),
    (allow_shuffle, "allowShuffle"),
}

/// The `Item::content` property — with the group payload elided (see the module docs).
///
/// A `Group` content serializes as `{"type":"group"}` with **no** `items` key: the children are
/// their own objects, so an edit deep inside a group never re-serializes the subtree. The other
/// variants (`unassigned`, `text`, `graphic`) ride as their own serde shapes, whole.
pub fn content_value(item: &Item) -> Option<Value> {
    match &item.content {
        designcraft_doc::Content::Group { .. } => Some(Value::Json(serde_json::json!({ "type": "group" }))),
        other => serde_json::to_value(other).ok().map(Value::Json),
    }
}

/// Write the `Item::content` property (the elided-group rule enforced, not assumed).
///
/// A group value that carries an `items` key is refused: the children belong to their own
/// objects, and a property write must not smuggle a subtree in behind the schema's back. A
/// group value without one keeps the item's existing children (or starts the group empty) —
/// the membership itself syncs through `Create`/`parent` writes, never through this property.
pub fn set_content(item: &mut Item, value: &Value) -> Result<(), String> {
    let json = value.to_json();
    if json.get("type").and_then(|t| t.as_str()) == Some("group") {
        if json.get("items").is_some() {
            return Err("a group content carries no items on the wire: children sync as their own objects".into());
        }
        if !matches!(item.content, designcraft_doc::Content::Group { .. }) {
            item.content = designcraft_doc::Content::Group { items: Vec::new() };
        }
        return Ok(());
    }
    let parsed: designcraft_doc::Content = serde_json::from_value(json).map_err(|e| format!("content: {e}"))?;
    if matches!(parsed, designcraft_doc::Content::Group { .. }) {
        return Err("a group content carries no items on the wire: children sync as their own objects".into());
    }
    item.content = parsed;
    Ok(())
}

/// Whether the `content` property differs between two items — comparing the **elided** forms, so
/// a change deep inside a group is not the parent's content change (the child's own diff
/// carries it).
pub fn content_differs(a: &Item, b: &Item) -> bool {
    match (content_value(a), content_value(b)) {
        (Some(x), Some(y)) => x != y,
        // Serialization never fails for these shapes in practice; treat failure as "differs" so
        // the change is re-sent rather than silently dropped.
        _ => true,
    }
}

/// The full property list of an item (the table fields plus the elided `content` property).
pub fn all_item_props() -> Vec<&'static str> {
    let mut v = ITEM_PROPS.to_vec();
    v.push("content");
    v
}

/// The document-level properties (the scalars and blobs that sync as one value each).
///
/// * `title`, `settings`, `styles` — the Document Setup, grids and the style lists: document
///   blobs, last-writer-wins as a whole (the pilot's `documentColors` decision, applied to
///   DesignCraft's central blobs; a per-style split can land later without a wire change).
/// * `nextId` — the id counter, merged monotonically (never regresses), so a peer that only
///   receives creations can still allocate without colliding.
/// * `regionLocks` — §6.7: locks sync as document properties. The lock types live in the
///   `crafthub-authz` crate of the shared checkout, not in `craft-collab`; their shape is
///   mirrored (and documented) in [`crate::locks`], and the list is held by the adapter until
///   the model grows its own field (see that module).
///
/// Not synced, honestly: `created`/`modified` (local save metadata), `font_scope` (runtime), and
/// the embedded `assets` — placed images ride the ASSET channel (content-addressed, §18.4
/// row 5's photocraft work), never inside batches; a peer without the asset shows the missing
/// link, exactly like a broken Links panel entry.
pub const DOC_PROPS: &[&str] = &["title", "settings", "styles", "nextId", "regionLocks"];

/// The per-entry document lists and their order properties (entries with identity sync per
/// entry, §2.3; the orders sync only where the panel order is real state).
pub const ENTRY_LISTS: &[(&str, &str)] = &[
    ("story", "storyOrder"), // keyed `Story:<n>`; the map's order is the id order, so the order property stays unused (see schema)
    ("layer", "layerOrder"), // keyed `Layer:<n>`; the Layers panel order is real state
    ("swatch", "swatchOrder"), // keyed `Swatch:<name>`; the Swatches panel order is real state
    ("section", "sectionOrder"), // keyed `Section:<start>`
];

/// Write a document-level property; `None` when `prop` is not a synced scalar.
///
/// `nextId` merges monotonically (an id counter only ever needs to grow), and `regionLocks`
/// is adapter state — routed by the adapter, not here.
pub fn set_doc_prop(doc: &mut designcraft_doc::Document, prop: &str, value: &Value) -> Option<Result<(), String>> {
    let json = value.to_json();
    match prop {
        "title" => Some(serde_json::from_value::<String>(json).map(|v| doc.title = v).map_err(|e| e.to_string())),
        "settings" => Some(serde_json::from_value::<designcraft_doc::DocSettings>(json).map(|v| doc.settings = v).map_err(|e| e.to_string())),
        "styles" => {
            Some(serde_json::from_value::<designcraft_doc::Styles>(json).map(|v| doc.styles = std::sync::Arc::new(v)).map_err(|e| e.to_string()))
        }
        "nextId" => Some(serde_json::from_value::<u64>(json).map(|v| doc.next_id = doc.next_id.max(v)).map_err(|e| e.to_string())),
        _ => None,
    }
}

/// The value of a document-level property; `None` when `prop` is not a synced scalar.
/// `regionLocks` is filled in by the adapter (it is adapter-held state, see [`crate::locks`]).
pub fn doc_prop_value(doc: &designcraft_doc::Document, prop: &str) -> Option<Value> {
    match prop {
        "title" => Some(Value::Json(serde_json::to_value(&doc.title).ok()?)),
        "settings" => Some(Value::Json(serde_json::to_value(&doc.settings).ok()?)),
        "styles" => Some(Value::Json(serde_json::to_value(&doc.styles).ok()?)),
        "nextId" => Some(Value::Json(serde_json::to_value(doc.next_id).ok()?)),
        _ => None,
    }
}
