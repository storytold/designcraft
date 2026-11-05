//! Ruler guides: add (dragged from the rulers), select, move, delete, list; Layout › Create
//! Guides.
//!
//! Guides are stored on their page in spread coordinates; a spread guide (dropped on the
//! pasteboard) lives on the spread's first page with `spread: true` and crosses the pasteboard.
//! Selected guides are [`Selection::guides`], addressed as (spread, page, index): the commands
//! that delete guides renumber it, and [`keep_selection_valid`] drops what another command made
//! stale.

use std::collections::{HashMap, HashSet};

use designcraft_doc::{Document, Guide, GuideSel, Orientation, Selection, SpreadRef};
use designcraft_geom::Rect;
use serde_json::{Value, json};

use super::{CommandSpec, bad, bool_or, cmd, has_doc, str_param};
use crate::{DocState, Result, Session};

const MAX_GUIDE_DIVISIONS: u64 = 1000;
const MAX_GUIDES_CREATED: usize = 10_000;
const MAX_GUIDE_GUTTER: f64 = 1_000_000.0;
/// The farthest a guide may be from the spread origin, in points, either way.
const MAX_GUIDE_POSITION: f64 = 1_000_000.0;
/// The most guides one `guides` list may name.
const MAX_GUIDES_NAMED: usize = 10_000;

pub fn specs() -> Vec<CommandSpec> {
    vec![
        cmd!(
            "guide.add",
            "New Guide",
            [],
            None,
            "{orientation: horizontal|vertical, position (spread coordinate, at most 1000000 pt either way), spread?, page? (index in the spread; default: the page under `at` or the position), at? (the other coordinate), spreadGuide?: bool, liquid?: bool (a liquid guide, as the Page tool draws from the rulers)} or {orientation: both, point: [x, y], spread?, page?, spreadGuide?: bool, liquid?: bool} → {page, index}; `both` adds a horizontal guide through y and a vertical guide through x as one undo step → {guides: [{page, index}, {page, index}]}. The new guides become the selection; objects are deselected",
            has_doc,
            add
        ),
        cmd!("guide.move", "Move Guide", [], None, "{spread?, page, index, position (at most 1000000 pt either way)}", has_doc, move_guide),
        cmd!(
            "guide.moveBy",
            "Move Guides",
            [],
            None,
            "{dx?, dy?, guides?: [{spread?, page, index}] (default: the selected guides)} — vertical guides move by dx, horizontal ones by dy, as one undo step",
            has_doc,
            move_by
        ),
        cmd!(
            "guide.delete",
            "Delete Guide",
            [],
            None,
            "{spread?, page, index}, or {} for the selected guides; an address without page or index is an error",
            has_doc,
            delete
        ),
        cmd!("guide.deleteAll", "Delete All Guides on Spread", ["View", "Grids & Guides"], None, "{spread?}", has_doc, |s, p| {
            let r = super::spread_param(p, "guide.deleteAll")?;
            s.edit(|d, sel| {
                let sp = d.spread_mut(r).ok_or_else(|| bad("guide.deleteAll", "no such spread"))?;
                let mut n = 0;
                for pg in &mut sp.pages {
                    n += pg.guides.len();
                    pg.guides.clear();
                }
                sel.guides.retain(|g| g.spread != r);
                Ok(json!({"deleted": n}))
            })
        }),
        cmd!(
            "view.lockColumnGuides",
            "Lock Column Guides",
            ["View", "Grids & Guides"],
            None,
            "{on?: bool (default: toggle)} → {locked} — locked column guides can't be dragged; saved with the document (IDML ColumnGuideLocked), on in new documents",
            has_doc,
            lock_column_guides
        ),
        cmd!(
            query "guide.list",
            "Guides",
            [],
            None,
            "{spread?} → [{page, index, orientation, position, spread, selected}]",
            has_doc,
            |s, p| {
                let st = s.doc()?;
                let r = super::spread_param(p, "guide.list")?;
                let sp = st.doc.spread(r).ok_or_else(|| bad("guide.list", "no such spread"))?;
                let picked: HashSet<GuideSel> = st.selection.guides.iter().copied().collect();
                let mut out = Vec::new();
                for (pi, pg) in sp.pages.iter().enumerate() {
                    for (i, g) in pg.guides.iter().enumerate() {
                        let selected = picked.contains(&GuideSel { spread: r, page: pi, index: i });
                        out.push(json!({"page": pi, "index": i, "orientation": g.orientation, "position": g.position, "spread": g.spread, "selected": selected}));
                    }
                }
                Ok(Value::Array(out))
            }
        ),
        cmd!(
            noundo "guide.select",
            "Select Guide",
            [],
            None,
            "{spread?, page, index, guides?: [{spread?, page, index}] (several, in place of page and index), add?: bool (keep the guides already selected), toggle?: bool (deselect a selected guide instead: Shift-click)} — selects ruler guides and deselects objects → {selected: n}",
            has_doc,
            select
        ),
        cmd!(
            noundo "guide.selectAll",
            "Select All Guides",
            ["Edit"],
            Some("Cmd+Alt+G"),
            "{spread?} — every guide on the spread that is not locked or on a locked or hidden layer → {selected: n}",
            has_doc,
            select_all
        ),
        cmd!(noundo "guide.deselect", "Deselect Guides", [], None, "{}", has_doc, |s, _| {
            let st = s.doc_mut()?;
            st.selection.guides.clear();
            st.bump_revision();
            super::ok()
        }),
        cmd!(
            "layout.createGuides",
            "Create Guides…",
            ["Layout"],
            None,
            "{rows?: 0..1000, columns?: 0..1000, rowGutter?: 12, columnGutter?: 12, fitTo?: margins|page, removeExisting?: false, spread?, page? (index in the spread; default all pages)} — at most 10000 guides per command",
            has_doc,
            create_guides
        ),
    ]
}

fn lock_column_guides(s: &mut Session, p: &Value) -> Result<Value> {
    let cur = s.doc()?.doc.settings.column_guides_locked;
    let on = p.get("on").and_then(Value::as_bool).unwrap_or(!cur);
    if on == cur {
        return Ok(json!({"locked": cur}));
    }
    s.edit(|d, _| {
        d.settings.column_guides_locked = on;
        Ok(json!({"locked": on}))
    })
}

fn checked_usize(value: &Value, key: &str, command: &str) -> Result<usize> {
    let n = value.as_u64().ok_or_else(|| bad(command, format!("`{key}` must be a non-negative integer")))?;
    usize::try_from(n).map_err(|_| bad(command, format!("`{key}` is too large for this platform")))
}

fn optional_usize(p: &Value, key: &str, command: &str) -> Result<Option<usize>> {
    p.get(key).map(|value| checked_usize(value, key, command)).transpose()
}

fn finite_number(p: &Value, key: &str, command: &str) -> Result<Option<f64>> {
    let Some(value) = p.get(key) else { return Ok(None) };
    let number = value.as_f64().ok_or_else(|| bad(command, format!("`{key}` must be a finite number")))?;
    if !number.is_finite() {
        return Err(bad(command, format!("`{key}` must be a finite number")));
    }
    Ok(Some(number))
}

/// `position` as a guide position: finite and within [`MAX_GUIDE_POSITION`] of the origin.
fn guide_position(position: f64, command: &str) -> Result<f64> {
    if !position.is_finite() || position.abs() > MAX_GUIDE_POSITION {
        return Err(bad(command, format!("a guide position must be between -{MAX_GUIDE_POSITION} and {MAX_GUIDE_POSITION}")));
    }
    Ok(position)
}

fn orientation(p: &Value) -> Result<Orientation> {
    match str_param(p, "orientation") {
        Some("horizontal") => Ok(Orientation::Horizontal),
        Some("vertical") => Ok(Orientation::Vertical),
        _ => Err(bad("guide", "orientation: horizontal|vertical")),
    }
}

/// The page of a spread at a spread x coordinate (nearest page).
fn page_at(d: &Document, r: SpreadRef, x: f64) -> usize {
    d.spread(r).and_then(|sp| sp.page_at_x(x)).unwrap_or(0)
}

fn add(s: &mut Session, p: &Value) -> Result<Value> {
    let r = super::spread_param(p, "guide.add")?;
    let spread_guide = p.get("spreadGuide").and_then(Value::as_bool).unwrap_or(false);
    let liquid = p.get("liquid").and_then(Value::as_bool).unwrap_or(false);
    let page = optional_usize(p, "page", "guide.add")?;
    // (orientation, position, x used to find the page)
    let guides: Vec<(Orientation, f64, f64)> = if str_param(p, "orientation") == Some("both") {
        let point = p.get("point").and_then(Value::as_array).ok_or_else(|| bad("guide.add", "orientation `both` needs point: [x, y]"))?;
        let coord = |i: usize| point.get(i).and_then(Value::as_f64).filter(|v| v.is_finite());
        let (Some(x), Some(y), 2) = (coord(0), coord(1), point.len()) else {
            return Err(bad("guide.add", "`point` must be two finite numbers [x, y]"));
        };
        vec![(Orientation::Horizontal, y, x), (Orientation::Vertical, x, x)]
    } else {
        let o = orientation(p)?;
        let pos = finite_number(p, "position", "guide.add")?.ok_or_else(|| bad("guide.add", "missing position"))?;
        let x = match o {
            Orientation::Vertical => pos,
            Orientation::Horizontal => finite_number(p, "at", "guide.add")?.unwrap_or(0.0),
        };
        vec![(o, pos, x)]
    };
    for (_, pos, _) in &guides {
        guide_position(*pos, "guide.add")?;
    }
    let d = &s.doc()?.doc;
    let placed: Vec<(Orientation, f64, usize)> = guides
        .into_iter()
        .map(|(o, pos, x)| {
            let pi = match page {
                Some(pi) => pi,
                None if spread_guide => 0,
                None => page_at(d, r, x),
            };
            (o, pos, pi)
        })
        .collect();
    let layer = s.doc()?.active_layer;
    s.edit(|d, sel| {
        let sp = d.spread_mut(r).ok_or_else(|| bad("guide.add", "no such spread"))?;
        let mut out = Vec::new();
        let mut added = Vec::new();
        for (o, pos, pi) in placed {
            let pg = sp.pages.get_mut(pi).ok_or_else(|| bad("guide.add", "no such page"))?;
            let index = pg.guides.len();
            pg.guides.push(Guide {
                orientation: o,
                position: pos,
                spread: spread_guide,
                locked: false,
                layer: Some(layer),
                liquid: liquid && !spread_guide,
            });
            out.push(json!({"page": pi, "index": index}));
            added.push(GuideSel { spread: r, page: pi, index });
        }
        // A new guide is selected, as in InDesign; nothing else stays selected.
        *sel = Selection { guides: added, ..Default::default() };
        match <[Value; 1]>::try_from(out) {
            Ok([one]) => Ok(one),
            Err(out) => Ok(json!({"guides": out})),
        }
    })
}

fn guide_ref(p: &Value, cmd: &str) -> Result<(SpreadRef, usize, usize)> {
    let pi = optional_usize(p, "page", cmd)?.ok_or_else(|| bad(cmd, "missing page"))?;
    let i = optional_usize(p, "index", cmd)?.ok_or_else(|| bad(cmd, "missing index"))?;
    Ok((super::spread_param(p, cmd)?, pi, i))
}

/// Commands must use the same guide/layer protection as pointer editing.
fn check_editable(d: &Document, r: SpreadRef, pi: usize, i: usize, command: &str) -> Result<()> {
    let g = d.spread(r).and_then(|sp| sp.pages.get(pi)).and_then(|pg| pg.guides.get(i)).ok_or_else(|| bad(command, "no such guide"))?;
    if !g.editable_in(d) {
        return Err(bad(command, "the guide or its layer is locked or hidden"));
    }
    Ok(())
}

fn move_guide(s: &mut Session, p: &Value) -> Result<Value> {
    let (r, pi, i) = guide_ref(p, "guide.move")?;
    let pos = finite_number(p, "position", "guide.move")?.ok_or_else(|| bad("guide.move", "missing position"))?;
    let pos = guide_position(pos, "guide.move")?;
    s.edit(|d, _| {
        check_editable(d, r, pi, i, "guide.move")?;
        let g = d
            .spread_mut(r)
            .and_then(|sp| sp.pages.get_mut(pi))
            .and_then(|pg| pg.guides.get_mut(i))
            .ok_or_else(|| bad("guide.move", "no such guide"))?;
        g.position = pos;
        Ok(Value::Null)
    })
}

fn delete(s: &mut Session, p: &Value) -> Result<Value> {
    // An address names one guide: a partial one (only `spread`, say) is an error, not the selection.
    let targets = if ["spread", "page", "index"].iter().all(|k| p.get(k).is_none()) {
        s.doc()?.selection.guides.clone()
    } else {
        let (spread, page, index) = guide_ref(p, "guide.delete")?;
        vec![GuideSel { spread, page, index }]
    };
    if targets.is_empty() {
        return Err(bad("guide.delete", "no guide selected"));
    }
    s.edit(|d, sel| {
        for g in &targets {
            check_editable(d, g.spread, g.page, g.index, "guide.delete")?;
        }
        let deleted = remove_guides(d, sel, &targets, "guide.delete")?;
        Ok(json!({"deleted": deleted}))
    })
}

/// Removes `targets` (existing guides; repeats count once) and renumbers the guide selection to
/// match. Returns how many guides it removed.
fn remove_guides(d: &mut Document, sel: &mut Selection, targets: &[GuideSel], command: &str) -> Result<usize> {
    let gone: HashSet<GuideSel> = targets.iter().copied().collect();
    // Per page, the removed indices in ascending order.
    let mut by_page: HashMap<(SpreadRef, usize), Vec<usize>> = HashMap::new();
    for g in &gone {
        by_page.entry((g.spread, g.page)).or_default().push(g.index);
    }
    for ((spread, page), indices) in &mut by_page {
        indices.sort_unstable();
        let pg = d.spread_mut(*spread).and_then(|sp| sp.pages.get_mut(*page)).ok_or_else(|| bad(command, "no such page"))?;
        for &i in indices.iter().rev() {
            if i >= pg.guides.len() {
                return Err(bad(command, "no such guide"));
            }
            pg.guides.remove(i);
        }
    }
    sel.guides = sel
        .guides
        .iter()
        .filter(|g| !gone.contains(g))
        .map(|g| {
            let below = by_page.get(&(g.spread, g.page)).map_or(0, |indices| indices.partition_point(|&i| i < g.index));
            GuideSel { index: g.index.saturating_sub(below), ..*g }
        })
        .collect();
    Ok(gone.len())
}

/// The guides a `guides: [{spread?, page, index}]` list names, if the params have one.
fn guides_param(p: &Value, command: &str) -> Result<Option<Vec<GuideSel>>> {
    let Some(list) = p.get("guides") else { return Ok(None) };
    let list = list.as_array().ok_or_else(|| bad(command, "`guides` must be a list of {spread?, page, index}"))?;
    if list.len() > MAX_GUIDES_NAMED {
        return Err(bad(command, format!("`guides` may name at most {MAX_GUIDES_NAMED} guides")));
    }
    list.iter()
        .map(|g| {
            let (spread, page, index) = guide_ref(g, command)?;
            Ok(GuideSel { spread, page, index })
        })
        .collect::<Result<Vec<_>>>()
        .map(Some)
}

/// The guide `g` addresses, if there is one.
fn guide_of(d: &Document, g: GuideSel) -> Option<&Guide> {
    d.spread(g.spread)?.pages.get(g.page)?.guides.get(g.index)
}

/// A guide that can be selected: it exists, and neither it nor its layer is locked or hidden.
pub(crate) fn selectable(d: &Document, g: GuideSel) -> bool {
    guide_of(d, g).is_some_and(|guide| guide.editable_in(d))
}

fn select(s: &mut Session, p: &Value) -> Result<Value> {
    let named = match guides_param(p, "guide.select")? {
        Some(list) => list,
        None => {
            let (spread, page, index) = guide_ref(p, "guide.select")?;
            vec![GuideSel { spread, page, index }]
        }
    };
    let (add, toggle) = (bool_or(p, "add", false), bool_or(p, "toggle", false));
    let st = s.doc_mut()?;
    for g in &named {
        check_editable(&st.doc, g.spread, g.page, g.index, "guide.select")?;
    }
    let mut guides = if add || toggle { st.selection.guides.clone() } else { Vec::new() };
    let mut have: HashSet<GuideSel> = guides.iter().copied().collect();
    let mut seen = HashSet::new();
    let mut dropped = HashSet::new();
    for g in named.into_iter().filter(|g| seen.insert(*g)) {
        if !have.insert(g) {
            if toggle {
                dropped.insert(g);
            }
        } else {
            guides.push(g);
        }
    }
    guides.retain(|g| !dropped.contains(g));
    let n = guides.len();
    st.selection = Selection { guides, ..Default::default() };
    st.bump_revision();
    Ok(json!({"selected": n}))
}

fn select_all(s: &mut Session, p: &Value) -> Result<Value> {
    let r = super::spread_param(p, "guide.selectAll")?;
    let st = s.doc_mut()?;
    let sp = st.doc.spread(r).ok_or_else(|| bad("guide.selectAll", "no such spread"))?;
    let guides: Vec<GuideSel> = sp
        .pages
        .iter()
        .enumerate()
        .flat_map(|(page, pg)| (0..pg.guides.len()).map(move |index| GuideSel { spread: r, page, index }))
        .filter(|g| selectable(&st.doc, *g))
        .collect();
    let n = guides.len();
    st.selection = Selection { guides, ..Default::default() };
    st.bump_revision();
    Ok(json!({"selected": n}))
}

fn move_by(s: &mut Session, p: &Value) -> Result<Value> {
    let dx = finite_number(p, "dx", "guide.moveBy")?.unwrap_or(0.0);
    let dy = finite_number(p, "dy", "guide.moveBy")?.unwrap_or(0.0);
    let mut targets = match guides_param(p, "guide.moveBy")? {
        Some(list) => list,
        None => s.doc()?.selection.guides.clone(),
    };
    let mut seen = HashSet::new();
    targets.retain(|g| seen.insert(*g));
    if targets.is_empty() {
        return Err(bad("guide.moveBy", "no guide selected"));
    }
    s.edit(|d, _| {
        for g in &targets {
            check_editable(d, g.spread, g.page, g.index, "guide.moveBy")?;
        }
        for g in &targets {
            let guide = d
                .spread_mut(g.spread)
                .and_then(|sp| sp.pages.get_mut(g.page))
                .and_then(|pg| pg.guides.get_mut(g.index))
                .ok_or_else(|| bad("guide.moveBy", "no such guide"))?;
            let to = guide.position
                + match guide.orientation {
                    Orientation::Vertical => dx,
                    Orientation::Horizontal => dy,
                };
            guide.position = guide_position(to, "guide.moveBy").map_err(|_| bad("guide.moveBy", "the guide would move out of range"))?;
        }
        Ok(json!({"moved": targets.len()}))
    })
}

/// After a command: guides stay selected only while nothing else is (objects, text, cells, a
/// page), and only those that are still there and can be selected. A command that changed
/// which guides a page holds without keeping the guide selection up to date clears it, since
/// its (page, index) addresses may name other guides now. `before` is the document and
/// selection before the command, when it ran on the same document.
pub(crate) fn keep_selection_valid(st: &mut DocState, before: Option<(&Document, &Selection)>, id: &str) {
    let sel = &mut st.selection;
    if sel.guides.is_empty() {
        return;
    }
    if !sel.items.is_empty() || sel.text.is_some() || sel.cells.is_some() || sel.page.is_some() {
        sel.guides.clear();
        return;
    }
    let kept_up = id.starts_with("guide.") || matches!(id, "edit.undo" | "edit.redo") || before.is_some_and(|(_, old)| old.guides != sel.guides);
    if !kept_up
        && let Some((old, _)) = before
        && !std::ptr::eq(old, st.doc.as_ref())
        && !same_guide_addresses(old, &st.doc)
    {
        sel.guides.clear();
        return;
    }
    let mut seen = HashSet::new();
    sel.guides.retain(|g| selectable(&st.doc, *g) && seen.insert(*g));
}

/// Every (spread, page, index) of `old` still names a guide of the same orientation on the same
/// page in `new`: guides were only added at the end of their pages, or moved.
fn same_guide_addresses(old: &Document, new: &Document) -> bool {
    let same = |a: &[std::sync::Arc<designcraft_doc::Spread>], b: &[std::sync::Arc<designcraft_doc::Spread>]| {
        a.len() == b.len()
            && a.iter().zip(b).all(|(a, b)| {
                a.pages.len() == b.pages.len()
                    && a.pages.iter().zip(&b.pages).all(|(a, b)| {
                        a.id == b.id
                            && a.guides.len() <= b.guides.len()
                            && a.guides.iter().zip(&b.guides).all(|(a, b)| a.orientation == b.orientation)
                    })
            })
    };
    same(&old.spreads, &new.spreads) && same(&old.parents, &new.parents)
}

fn guide_divisions(p: &Value, key: &str) -> Result<u64> {
    let Some(value) = p.get(key) else { return Ok(0) };
    let count = value.as_u64().ok_or_else(|| bad("layout.createGuides", format!("`{key}` must be an integer")))?;
    if count > MAX_GUIDE_DIVISIONS {
        return Err(bad("layout.createGuides", format!("`{key}` must not exceed {MAX_GUIDE_DIVISIONS}")));
    }
    Ok(count)
}

fn guide_gutter(p: &Value, key: &str) -> Result<f64> {
    let Some(value) = p.get(key) else { return Ok(12.0) };
    let gutter = value.as_f64().ok_or_else(|| bad("layout.createGuides", format!("`{key}` must be a finite number")))?;
    if !gutter.is_finite() || !(0.0..=MAX_GUIDE_GUTTER).contains(&gutter) {
        return Err(bad("layout.createGuides", format!("`{key}` must be between 0 and {MAX_GUIDE_GUTTER}")));
    }
    Ok(gutter)
}

fn guide_count(divisions: u64, gutter: f64) -> Result<usize> {
    let edges = divisions.saturating_sub(1);
    let count = if gutter > 0.0 { edges.checked_mul(2) } else { Some(edges) }
        .ok_or_else(|| bad("layout.createGuides", "the requested guide count is too large"))?;
    usize::try_from(count).map_err(|_| bad("layout.createGuides", "the requested guide count is too large for this platform"))
}

/// Evenly spaced rows/columns with gutters inside `area`: the guide positions.
fn grid_positions(a: f64, b: f64, n: u64, gutter: f64) -> Result<Vec<f64>> {
    if n < 2 {
        return Ok(vec![]);
    }
    if !a.is_finite() || !b.is_finite() || !gutter.is_finite() || n > MAX_GUIDE_DIVISIONS {
        return Err(bad("layout.createGuides", "guide geometry must be finite and within the supported range"));
    }
    let cell = ((b - a) - gutter * (n - 1) as f64) / n as f64;
    if !cell.is_finite() {
        return Err(bad("layout.createGuides", "guide geometry is outside the supported range"));
    }
    let mut v = Vec::with_capacity(guide_count(n, gutter)?);
    for k in 1..n {
        let edge = a + k as f64 * cell + (k - 1) as f64 * gutter;
        if !edge.is_finite() || (gutter > 0.0 && !(edge + gutter).is_finite()) {
            return Err(bad("layout.createGuides", "guide geometry is outside the supported range"));
        }
        v.push(edge);
        if gutter > 0.0 {
            v.push(edge + gutter);
        }
    }
    Ok(v)
}

fn create_guides(s: &mut Session, p: &Value) -> Result<Value> {
    let rows = guide_divisions(p, "rows")?;
    let cols = guide_divisions(p, "columns")?;
    let rg = guide_gutter(p, "rowGutter")?;
    let cg = guide_gutter(p, "columnGutter")?;
    let to_page = str_param(p, "fitTo") == Some("page");
    let remove = p.get("removeExisting").and_then(Value::as_bool).unwrap_or(false);
    let r = super::spread_param(p, "layout.createGuides")?;
    let only = optional_usize(p, "page", "layout.createGuides")?;
    let page_count = {
        let sp = s.doc()?.doc.spread(r).ok_or_else(|| bad("layout.createGuides", "no such spread"))?;
        match only {
            Some(page) => {
                if sp.pages.get(page).is_none() {
                    return Err(bad("layout.createGuides", "no such page"));
                }
                1
            }
            None => sp.pages.len(),
        }
    };
    let per_page = guide_count(rows, rg)?
        .checked_add(guide_count(cols, cg)?)
        .ok_or_else(|| bad("layout.createGuides", "the requested guide count is too large"))?;
    per_page
        .checked_mul(page_count)
        .filter(|count| *count <= MAX_GUIDES_CREATED)
        .ok_or_else(|| bad("layout.createGuides", format!("a single command may create at most {MAX_GUIDES_CREATED} guides")))?;
    let layer = s.doc()?.active_layer;
    s.edit(|d, sel| {
        if remove {
            // The cleared pages' guide addresses name the new guides now.
            sel.guides.retain(|g| g.spread != r || only.is_some_and(|o| o != g.page));
        }
        let sp = d.spread_mut(r).ok_or_else(|| bad("layout.createGuides", "no such spread"))?;
        let mut n = 0usize;
        for (pi, pg) in sp.pages.iter_mut().enumerate() {
            if only.is_some_and(|o| o != pi) {
                continue;
            }
            if remove {
                pg.guides.clear();
            }
            let area: Rect = if to_page { pg.bounds() } else { pg.margin_rect() };
            for y in grid_positions(area.y0, area.y1, rows, rg)? {
                pg.guides.push(Guide {
                    orientation: Orientation::Horizontal,
                    position: y,
                    spread: false,
                    locked: false,
                    layer: Some(layer),
                    liquid: false,
                });
                n += 1;
            }
            for x in grid_positions(area.x0, area.x1, cols, cg)? {
                pg.guides.push(Guide {
                    orientation: Orientation::Vertical,
                    position: x,
                    spread: false,
                    locked: false,
                    layer: Some(layer),
                    liquid: false,
                });
                n += 1;
            }
        }
        Ok(json!({"guides": n}))
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lock_column_guides_toggles_sets_and_undoes() {
        let mut s = Session::new();
        s.execute("file.new", &json!({})).unwrap();
        let locked = |s: &Session| s.doc().unwrap().doc.settings.column_guides_locked;
        let undo_steps = |s: &Session| s.doc().unwrap().history.undo.len();
        assert!(locked(&s), "locked in new documents");
        let before = undo_steps(&s);
        assert_eq!(s.execute("view.lockColumnGuides", &json!({})).unwrap()["locked"], false);
        assert!(!locked(&s));
        assert_eq!(undo_steps(&s), before + 1, "one undo step per toggle");
        s.execute("edit.undo", &json!({})).unwrap();
        assert!(locked(&s), "undo restores the lock");
        s.execute("edit.redo", &json!({})).unwrap();
        assert!(!locked(&s), "redo unlocks again");
        // Setting the current value changes nothing and adds no undo step.
        let steps = undo_steps(&s);
        assert_eq!(s.execute("view.lockColumnGuides", &json!({"on": false})).unwrap()["locked"], false);
        assert_eq!(undo_steps(&s), steps);
        assert_eq!(s.execute("view.lockColumnGuides", &json!({"on": true})).unwrap()["locked"], true);
        assert!(locked(&s));
        assert_eq!(s.execute("view.lockColumnGuides", &json!({})).unwrap()["locked"], false);
        // Saved with the document; files without the setting read as locked.
        let json = serde_json::to_string(&*s.doc().unwrap().doc).unwrap();
        assert!(json.contains("\"columnGuidesLocked\":false"), "{json}");
        let back: Document = serde_json::from_str(&json).unwrap();
        assert!(!back.settings.column_guides_locked);
        let old = json.replace("\"columnGuidesLocked\":false,", "").replace(",\"columnGuidesLocked\":false", "");
        assert!(!old.contains("columnGuidesLocked"));
        let back: Document = serde_json::from_str(&old).unwrap();
        assert!(back.settings.column_guides_locked);
    }

    #[test]
    fn guides_add_move_delete_and_grid() {
        let mut s = Session::new();
        s.execute("file.new", &json!({"pages": 3})).unwrap();
        // Spread 1 holds pages 2–3 side by side from x = 0.
        let r = s.execute("guide.add", &json!({"spread": 1, "orientation": "vertical", "position": 100.0})).unwrap();
        assert_eq!(r["page"], 0);
        let r = s.execute("guide.add", &json!({"spread": 1, "orientation": "horizontal", "position": 200.0, "at": 700.0})).unwrap();
        assert_eq!(r["page"], 1);
        s.execute("guide.move", &json!({"spread": 1, "page": 1, "index": 0, "position": 250.0})).unwrap();
        let l = s.execute("guide.list", &json!({"spread": 1})).unwrap();
        assert_eq!(l.as_array().unwrap().len(), 2);
        assert_eq!(l[1]["position"], 250.0);
        s.execute("guide.delete", &json!({"spread": 1, "page": 0, "index": 0})).unwrap();
        assert_eq!(s.execute("guide.list", &json!({"spread": 1})).unwrap().as_array().unwrap().len(), 1);
        // 3 columns with 12 pt gutters inside the margins: 4 guides; 2 rows without gutter: 1 guide.
        let r = s.execute("layout.createGuides", &json!({"spread": 0, "columns": 3, "rows": 2, "rowGutter": 0.0})).unwrap();
        assert_eq!(r["guides"], 5);
        let d = &s.doc().unwrap().doc;
        let pg = &d.spreads[0].pages[0];
        let m = pg.margin_rect();
        let v: Vec<f64> = pg.guides.iter().filter(|g| g.orientation == Orientation::Vertical).map(|g| g.position).collect();
        let cell = (m.width() - 24.0) / 3.0;
        assert!((v[0] - (m.x0 + cell)).abs() < 1e-9 && (v[1] - (m.x0 + cell + 12.0)).abs() < 1e-9);
        s.execute("guide.deleteAll", &json!({"spread": 0})).unwrap();
        assert!(s.doc().unwrap().doc.spreads[0].pages[0].guides.is_empty());
        s.execute("edit.undo", &json!({})).unwrap();
        assert_eq!(s.doc().unwrap().doc.spreads[0].pages[0].guides.len(), 5);
    }

    #[test]
    fn both_orientations_are_one_undo_step() {
        let mut s = Session::new();
        s.execute("file.new", &json!({})).unwrap();
        let r = s.execute("guide.add", &json!({"orientation": "both", "point": [120.0, 80.0], "spreadGuide": true})).unwrap();
        assert_eq!(r["guides"].as_array().unwrap().len(), 2);
        let l = s.execute("guide.list", &json!({})).unwrap();
        assert_eq!((l[0]["orientation"].as_str(), l[0]["position"].as_f64()), (Some("horizontal"), Some(80.0)));
        assert_eq!((l[1]["orientation"].as_str(), l[1]["position"].as_f64()), (Some("vertical"), Some(120.0)));
        s.execute("edit.undo", &json!({})).unwrap();
        assert_eq!(s.execute("guide.list", &json!({})).unwrap(), json!([]));
        assert!(s.execute("guide.add", &json!({"orientation": "both", "point": [1.0]})).is_err());
        assert!(s.execute("guide.add", &json!({"orientation": "both", "position": 1.0})).is_err());
    }

    #[test]
    fn create_guides_validates_generation_limits() {
        let mut s = Session::new();
        s.execute("file.new", &json!({})).unwrap();
        assert!(s.execute("layout.createGuides", &json!({"columns": MAX_GUIDE_DIVISIONS + 1})).is_err());
        assert!(s.execute("layout.createGuides", &json!({"columns": 2, "columnGutter": -1})).is_err());
        assert!(s.execute("layout.createGuides", &json!({"columns": "many"})).is_err());
        let r = s.execute("layout.createGuides", &json!({"columns": MAX_GUIDE_DIVISIONS, "columnGutter": 0})).unwrap();
        assert_eq!(r["guides"], MAX_GUIDE_DIVISIONS - 1);
    }

    #[test]
    fn mutating_guide_commands_reject_invalid_indices() {
        let mut s = Session::new();
        s.execute("file.new", &json!({})).unwrap();
        s.execute("guide.add", &json!({"orientation": "vertical", "position": 100})).unwrap();

        assert!(s.execute("guide.add", &json!({"orientation": "vertical", "position": 100, "page": -1})).is_err());
        assert!(s.execute("guide.move", &json!({"page": 0.5, "index": 0, "position": 200})).is_err());
        assert!(s.execute("guide.delete", &json!({"page": 0, "index": -1})).is_err());
        assert!(s.execute("guide.deleteAll", &json!({"spread": "0"})).is_err());
        assert!(s.execute("layout.createGuides", &json!({"spread": -1, "columns": 2})).is_err());
        assert!(s.execute("layout.createGuides", &json!({"page": u64::MAX, "columns": 2})).is_err());
        assert_eq!(s.execute("guide.list", &json!({})).unwrap().as_array().unwrap().len(), 1);

        s.execute("guide.add", &json!({"spread": {"kind": "doc", "index": 0}, "page": 0, "orientation": "horizontal", "position": 150})).unwrap();
        assert_eq!(s.execute("guide.list", &json!({})).unwrap().as_array().unwrap().len(), 2);
    }
}

#[cfg(test)]
mod selection_tests {
    use serde_json::{Value, json};

    use crate::Session;

    /// A page with vertical guides at 100, 200, 300 and a horizontal one at 400.
    fn guides() -> Session {
        let mut s = Session::new();
        s.execute("file.new", &json!({})).unwrap();
        for x in [100.0, 200.0, 300.0] {
            s.execute("guide.add", &json!({"orientation": "vertical", "position": x})).unwrap();
        }
        s.execute("guide.add", &json!({"orientation": "horizontal", "position": 400.0})).unwrap();
        s
    }

    fn selected(s: &Session) -> Vec<usize> {
        s.doc().unwrap().selection.guides.iter().map(|g| g.index).collect()
    }

    fn positions(s: &mut Session) -> Vec<f64> {
        let l = s.execute("guide.list", &json!({})).unwrap();
        l.as_array().unwrap().iter().map(|g| g["position"].as_f64().unwrap()).collect()
    }

    #[test]
    fn selecting_guides_clears_objects_and_the_other_way_round() {
        let mut s = guides();
        let id = s.execute("frame.create", &json!({"rect": [36, 36, 100, 100]})).unwrap()["id"].clone();
        s.execute("selection.set", &json!({"ids": [id]})).unwrap();
        s.execute("guide.select", &json!({"page": 0, "index": 1})).unwrap();
        assert!(s.doc().unwrap().selection.items.is_empty());
        assert_eq!(selected(&s), [1]);
        // Shift-click adds, Shift-click again removes.
        s.execute("guide.select", &json!({"page": 0, "index": 3, "toggle": true})).unwrap();
        assert_eq!(selected(&s), [1, 3]);
        s.execute("guide.select", &json!({"page": 0, "index": 1, "toggle": true})).unwrap();
        assert_eq!(selected(&s), [3]);
        let r = s.execute("guide.select", &json!({"guides": [{"page": 0, "index": 0}, {"page": 0, "index": 2}], "add": true})).unwrap();
        assert_eq!(r["selected"], 3);
        let l = s.execute("guide.list", &json!({})).unwrap();
        assert_eq!(l.as_array().unwrap().iter().map(|g| g["selected"] == true).collect::<Vec<_>>(), [true, false, true, true]);
        // Selecting an object (Shift-click too) deselects the guides.
        s.execute("selection.toggle", &json!({"id": id})).unwrap();
        assert!(selected(&s).is_empty());
        s.execute("guide.select", &json!({"page": 0, "index": 0})).unwrap();
        s.execute("edit.deselectAll", &json!({})).unwrap();
        assert!(selected(&s).is_empty());
        s.execute("guide.selectAll", &json!({})).unwrap();
        s.execute("guide.deselect", &json!({})).unwrap();
        assert!(selected(&s).is_empty());
        // No such guide.
        assert!(s.execute("guide.select", &json!({"page": 0, "index": 9})).is_err());
        assert!(s.execute("guide.select", &json!({"page": 3, "index": 0})).is_err());
        assert!(s.execute("guide.select", &json!({"guides": "all"})).is_err());
    }

    #[test]
    fn select_all_guides_skips_locked_ones() {
        let mut s = guides();
        let layer = s.execute("layer.new", &json!({"name": "Grid"})).unwrap()["id"].as_u64().unwrap();
        s.execute("layer.activate", &json!({"id": layer})).unwrap();
        s.execute("guide.add", &json!({"orientation": "vertical", "position": 500.0})).unwrap();
        s.execute("layer.set", &json!({"id": layer, "locked": true})).unwrap();
        assert_eq!(s.execute("guide.selectAll", &json!({})).unwrap()["selected"], 4);
        assert_eq!(selected(&s), [0, 1, 2, 3]);
        assert!(s.execute("guide.select", &json!({"page": 0, "index": 4})).is_err());
        // Locking the layer of a selected guide deselects it.
        s.execute("layer.set", &json!({"id": layer, "locked": false})).unwrap();
        s.execute("guide.selectAll", &json!({})).unwrap();
        s.execute("layer.set", &json!({"id": layer, "locked": true})).unwrap();
        assert_eq!(selected(&s), [0, 1, 2, 3]);
        let spec = crate::cmd::find_command("guide.selectAll").unwrap();
        assert_eq!((spec.menu, spec.shortcut), (&["Edit"][..], Some("Cmd+Alt+G")));
    }

    #[test]
    fn selected_guides_move_and_delete_as_one_undo_step() {
        let mut s = guides();
        s.execute("guide.select", &json!({"guides": [{"page": 0, "index": 0}, {"page": 0, "index": 2}, {"page": 0, "index": 3}]})).unwrap();
        // Vertical guides move by dx, horizontal ones by dy.
        s.execute("guide.moveBy", &json!({"dx": 10.0, "dy": -5.0})).unwrap();
        assert_eq!(positions(&mut s), [110.0, 200.0, 310.0, 395.0]);
        s.execute("edit.undo", &json!({})).unwrap();
        assert_eq!(positions(&mut s), [100.0, 200.0, 300.0, 400.0]);
        assert_eq!(selected(&s), [0, 2, 3]);
        // Delete (edit.clear, as the Delete key) takes all three; the guide left is the one at 200.
        assert_eq!(s.execute("edit.clear", &json!({})).unwrap()["deleted"], 3);
        assert_eq!(positions(&mut s), [200.0]);
        assert!(selected(&s).is_empty());
        s.execute("edit.undo", &json!({})).unwrap();
        assert_eq!(positions(&mut s), [100.0, 200.0, 300.0, 400.0]);
        assert_eq!(selected(&s), [0, 2, 3]);
        s.execute("edit.redo", &json!({})).unwrap();
        assert_eq!(positions(&mut s), [200.0]);
        assert!(s.execute("guide.moveBy", &json!({"dx": 1.0})).is_err(), "nothing selected");
        // A guide stays within 1000000 pt of the origin.
        assert!(s.execute("guide.moveBy", &json!({"dx": f64::MAX, "guides": [{"page": 0, "index": 0}]})).is_err());
        assert!(s.execute("guide.moveBy", &json!({"dx": 1e6, "guides": [{"page": 0, "index": 0}]})).is_err());
        assert_eq!(positions(&mut s), [200.0]);
        s.execute("guide.moveBy", &json!({"dx": -1e6 - 200.0, "guides": [{"page": 0, "index": 0}]})).unwrap();
        assert_eq!(positions(&mut s), [-1e6]);
        assert!(s.execute("guide.move", &json!({"page": 0, "index": 0, "position": 1e6 + 1.0})).is_err());
        assert!(s.execute("guide.add", &json!({"orientation": "vertical", "position": -2e6})).is_err());
        assert!(s.execute("guide.add", &json!({"orientation": "both", "point": [0.0, 1e300]})).is_err());
        assert_eq!(positions(&mut s), [-1e6]);
    }

    #[test]
    fn the_guide_selection_follows_deletes_and_drops_stale_guides() {
        let mut s = guides();
        s.execute("guide.select", &json!({"guides": [{"page": 0, "index": 2}, {"page": 0, "index": 3}]})).unwrap();
        // Deleting a guide before them renumbers them.
        s.execute("guide.delete", &json!({"page": 0, "index": 0})).unwrap();
        assert_eq!(selected(&s), [1, 2]);
        assert_eq!(positions(&mut s), [200.0, 300.0, 400.0]);
        // Deleting a selected one deselects it.
        s.execute("guide.delete", &json!({"page": 0, "index": 1})).unwrap();
        assert_eq!(selected(&s), [1]);
        // Undo puts back the selection that went with the document.
        s.execute("edit.undo", &json!({})).unwrap();
        assert_eq!(selected(&s), [1, 2]);
        // A new guide is the selection.
        s.execute("guide.add", &json!({"orientation": "vertical", "position": 50.0})).unwrap();
        assert_eq!(selected(&s), [3]);
        // Undoing the add to before the first delete: those addresses held other guides then.
        s.execute("edit.undo", &json!({})).unwrap();
        s.execute("edit.undo", &json!({})).unwrap();
        assert_eq!(selected(&s), [2, 3]);
        // Create Guides replacing the page's guides: their addresses name new guides, so the
        // selection goes.
        s.execute("layout.createGuides", &json!({"columns": 2, "removeExisting": true})).unwrap();
        assert!(selected(&s).is_empty());
        s.execute("edit.undo", &json!({})).unwrap();
        s.execute("guide.select", &json!({"page": 0, "index": 0})).unwrap();
        s.execute("layout.createGuides", &json!({"columns": 2, "columnGutter": 0})).unwrap();
        assert_eq!(selected(&s), [0], "guides only added: the addresses still hold");
        s.execute("edit.undo", &json!({})).unwrap();
        s.execute("guide.selectAll", &json!({})).unwrap();
        s.execute("guide.deleteAll", &json!({})).unwrap();
        assert!(selected(&s).is_empty());
        // Deleting the page goes with it.
        let mut s = Session::new();
        s.execute("file.new", &json!({"pages": 2})).unwrap();
        s.execute("guide.add", &json!({"spread": 1, "orientation": "vertical", "position": 100.0})).unwrap();
        s.execute("guide.selectAll", &json!({"spread": 1})).unwrap();
        assert_eq!(selected(&s), [0]);
        s.execute("layout.pages.delete", &json!({"pages": [1]})).unwrap();
        assert!(selected(&s).is_empty());
        let _: Value = s.execute("guide.list", &json!({})).unwrap();
    }
}

#[cfg(test)]
mod new_guide_tests {
    use designcraft_doc::GuideSel;
    use serde_json::json;

    use crate::Session;

    fn selected(s: &Session) -> Vec<GuideSel> {
        s.doc().unwrap().selection.guides.clone()
    }

    #[test]
    fn a_new_guide_is_the_selection() {
        let mut s = Session::new();
        s.execute("file.new", &json!({"pages": 2})).unwrap();
        let id = s.execute("frame.create", &json!({"rect": [36, 36, 100, 100]})).unwrap()["id"].clone();
        s.execute("selection.set", &json!({"ids": [id]})).unwrap();
        s.execute("guide.add", &json!({"orientation": "vertical", "position": 100.0})).unwrap();
        let spread = s.doc().unwrap().selection.guides[0].spread;
        assert_eq!(selected(&s), [GuideSel { spread, page: 0, index: 0 }]);
        assert!(s.doc().unwrap().selection.items.is_empty(), "objects are deselected");
        // Both ways (Cmd-drag from the ruler corner): the two new guides.
        s.execute("guide.add", &json!({"orientation": "both", "point": [50.0, 60.0], "spreadGuide": true})).unwrap();
        assert_eq!(selected(&s), [GuideSel { spread, page: 0, index: 1 }, GuideSel { spread, page: 0, index: 2 }]);
        // Undo gives back the selection from before.
        s.execute("edit.undo", &json!({})).unwrap();
        assert_eq!(selected(&s), [GuideSel { spread, page: 0, index: 0 }]);
    }

    #[test]
    fn delete_takes_a_whole_address_or_the_selection() {
        let mut s = Session::new();
        s.execute("file.new", &json!({"pages": 3})).unwrap();
        for spread in [0, 1] {
            s.execute("guide.add", &json!({"spread": spread, "orientation": "vertical", "position": 100.0})).unwrap();
        }
        s.execute("guide.select", &json!({"spread": 1, "page": 0, "index": 0})).unwrap();
        // A spread without page and index is not "the selection".
        assert!(s.execute("guide.delete", &json!({"spread": 0})).is_err());
        assert!(s.execute("guide.delete", &json!({"spread": 1, "page": 0})).is_err());
        assert_eq!(selected(&s).len(), 1);
        // A selection naming a guide twice, apart, deletes it once.
        s.execute("guide.add", &json!({"spread": 1, "orientation": "vertical", "position": 200.0})).unwrap();
        s.execute("guide.add", &json!({"spread": 1, "orientation": "vertical", "position": 300.0})).unwrap();
        let spread = selected(&s)[0].spread;
        let at = |index| GuideSel { spread, page: 0, index };
        s.doc_mut().unwrap().selection.guides = vec![at(0), at(2), at(0)];
        assert_eq!(s.execute("guide.delete", &json!({})).unwrap()["deleted"], 2);
        let left = s.execute("guide.list", &json!({"spread": 1})).unwrap();
        assert_eq!(left.as_array().unwrap().iter().map(|g| g["position"].as_f64().unwrap()).collect::<Vec<_>>(), [200.0]);
        assert_eq!(s.execute("guide.list", &json!({"spread": 0})).unwrap().as_array().unwrap().len(), 1);
    }
}

#[cfg(test)]
mod layer_tests {
    use serde_json::json;

    use crate::Session;

    #[test]
    fn guide_commands_respect_layer_locks() {
        let mut s = Session::new();
        s.execute("file.new", &json!({})).unwrap();
        let layer = s.execute("layer.new", &json!({"name": "Grid"})).unwrap()["id"].as_u64().unwrap();
        s.execute("layer.activate", &json!({"id": layer})).unwrap();
        s.execute("guide.add", &json!({"orientation": "vertical", "position": 100})).unwrap();
        s.execute("layer.set", &json!({"id": layer, "locked": true})).unwrap();
        for command in ["guide.move", "guide.delete"] {
            assert!(s.execute(command, &json!({"page": 0, "index": 0, "position": 200})).is_err(), "{command} changed a locked layer");
            assert_eq!(s.execute("guide.list", &json!({})).unwrap()[0]["position"], 100.0);
        }
        s.execute("layer.set", &json!({"id": layer, "locked": false})).unwrap();
        s.execute("guide.move", &json!({"page": 0, "index": 0, "position": 200})).unwrap();
        s.execute("guide.delete", &json!({"page": 0, "index": 0})).unwrap();
        assert_eq!(s.execute("guide.list", &json!({})).unwrap(), json!([]));
        s.execute("edit.undo", &json!({})).unwrap();
        assert_eq!(s.execute("guide.list", &json!({})).unwrap()[0]["position"], 200.0);
    }

    #[test]
    fn guides_follow_their_layer() {
        let mut s = Session::new();
        s.execute("file.new", &json!({})).unwrap();
        let l = s.execute("layer.new", &json!({"name": "Grid"})).unwrap()["id"].as_u64().unwrap();
        s.execute("layer.activate", &json!({"id": l})).unwrap();
        s.execute("guide.add", &json!({"orientation": "vertical", "position": 100})).unwrap();
        let g = |s: &Session| s.doc().unwrap().doc.spreads[0].pages[0].guides[0].clone();
        assert_eq!(g(&s).layer.map(|x| x.0), Some(l));
        assert!(g(&s).visible_in(&s.doc().unwrap().doc));
        s.execute("layer.set", &json!({"id": l, "visible": false})).unwrap();
        let d = s.doc().unwrap().doc.clone();
        assert!(!g(&s).visible_in(&d) && !g(&s).editable_in(&d));
        s.execute("layer.set", &json!({"id": l, "visible": true, "locked": true})).unwrap();
        let d = s.doc().unwrap().doc.clone();
        assert!(g(&s).visible_in(&d) && !g(&s).editable_in(&d));
    }

    #[test]
    fn create_guides_goes_to_the_named_spread_and_page_only() {
        use designcraft_doc::SpreadRef;
        let mut s = Session::new();
        s.execute("file.new", &json!({"pages": 3})).unwrap();
        // The UI passes the spread/page being worked on as a serialised SpreadRef (page 3).
        let spread = serde_json::to_value(SpreadRef::Doc(1)).unwrap();
        s.execute("layout.createGuides", &json!({"spread": spread, "page": 1, "columns": 2, "columnGutter": 0.0})).unwrap();
        let l = s.execute("guide.list", &json!({"spread": 1})).unwrap();
        let l = l.as_array().unwrap();
        assert!(!l.is_empty() && l.iter().all(|g| g["page"] == 1), "only page 3: {l:?}");
        let first = s.execute("guide.list", &json!({"spread": 0})).unwrap();
        assert!(first.as_array().unwrap().is_empty(), "nothing on the first page");
    }

    #[test]
    fn guides_on_a_parent_page_show_on_the_pages_that_use_it() {
        use designcraft_doc::{PageSide, SpreadRef};
        let mut s = Session::new();
        s.execute("file.new", &json!({"pages": 4, "facingPages": true})).unwrap();
        // A vertical guide 88 pt inside the parent's right page, and a horizontal one.
        let parent = serde_json::to_value(SpreadRef::Parent(0)).unwrap();
        let right_x = s.doc().unwrap().doc.parents[0].pages.last().unwrap().x;
        s.execute("guide.add", &json!({"spread": parent, "orientation": "vertical", "position": right_x + 88.0})).unwrap();
        s.execute("guide.add", &json!({"spread": parent, "orientation": "horizontal", "position": 200.0, "at": right_x + 10.0})).unwrap();
        let d = &s.doc().unwrap().doc;
        let mut seen = 0;
        for abs in 0..d.page_count() {
            let page = d.page(abs).unwrap();
            let (pi, qi) = d.parent_page_for(abs).unwrap();
            let inherited = d.inherited_guides(abs);
            if d.parents[pi].pages[qi].guides.is_empty() {
                assert!(inherited.is_empty(), "page {abs} uses the parent's left page, which has no guides");
                continue;
            }
            seen += 1;
            assert_eq!(page.side, PageSide::Right);
            let v = inherited.iter().find(|g| g.orientation == designcraft_doc::Orientation::Vertical).unwrap();
            // Moved to this page's own place in its spread: still 88 pt inside the page.
            assert!((v.position - (page.x + 88.0)).abs() < 1e-6, "page {abs}: {v:?} vs x {}", page.x);
            let h = inherited.iter().find(|g| g.orientation == designcraft_doc::Orientation::Horizontal).unwrap();
            assert!((h.position - 200.0).abs() < 1e-6);
        }
        assert!(seen >= 2, "pages on the parent's right side inherit: {seen}");
        // The guides stay on the parent: nothing is copied onto document pages.
        assert!((0..d.page_count()).all(|a| d.page(a).unwrap().guides.is_empty()));
    }
}
