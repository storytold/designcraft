//! Layout menu and Pages/Layers panels: pages, spreads, parents, margins & columns, layers.

use designcraft_doc::{LAYER_COLORS, Layer, LayerId, Margins, SpreadId};
use serde_json::{Value, json};

use super::{CommandSpec, bad, cmd, has_doc, ok, str_param};
use crate::{Result, Session};

/// A start page number parameter: a whole number in 1..=[`designcraft_doc::MAX_PAGE_NUMBER`].
fn start_number(cmd: &str, key: &str, v: &Value) -> Result<u32> {
    v.as_u64()
        .and_then(|n| u32::try_from(n).ok())
        .filter(|n| (1..=designcraft_doc::MAX_PAGE_NUMBER).contains(n))
        .ok_or_else(|| bad(cmd, format!("`{key}` must be a whole number from 1 to {}", designcraft_doc::MAX_PAGE_NUMBER)))
}

/// Each page's margin box, by spread and page id.
fn margin_boxes(d: &designcraft_doc::Document) -> Vec<(designcraft_doc::SpreadRef, designcraft_doc::PageId, designcraft_geom::Rect)> {
    d.spread_refs()
        .filter_map(|r| d.spread(r).map(|sp| (r, sp)))
        .flat_map(|(r, sp)| sp.pages.iter().map(move |pg| (r, pg.id, pg.margin_rect())))
        .collect()
}

/// Layout Adjustment: objects on each page move and resize from its old margin box to its new
/// one (content keeps its size; frames that auto-fit refit).
fn adjust_layout(d: &mut designcraft_doc::Document, before: &[(designcraft_doc::SpreadRef, designcraft_doc::PageId, designcraft_geom::Rect)]) {
    use designcraft_geom::Affine;
    let after = margin_boxes(d);
    for (r, pid, old) in before {
        let Some((_, _, new)) = after.iter().find(|(r2, p2, _)| r2 == r && p2 == pid) else { continue };
        if old == new || old.width() < 1e-6 || old.height() < 1e-6 {
            continue;
        }
        let m = Affine::translate((new.x0, new.y0))
            * Affine::scale_non_uniform(new.width() / old.width(), new.height() / old.height())
            * Affine::translate((-old.x0, -old.y0));
        let Some(sp) = d.spread(*r) else { continue };
        // The page's objects (by centre, measured against the old boxes).
        let ids: Vec<designcraft_doc::ItemId> = sp
            .items
            .iter()
            .filter(|it| {
                let c = it.bounds().center();
                // Pages share a spread side by side: the margin box's page is the one under the centre's x.
                before
                    .iter()
                    .filter(|(r2, _, _)| r2 == r)
                    .min_by(|a, b| dist(a.2, c.x).total_cmp(&dist(b.2, c.x)))
                    .is_some_and(|(_, p2, _)| p2 == pid)
            })
            .map(|it| it.id)
            .collect();
        for id in ids {
            let Some(it) = d.item_mut(id) else { continue };
            if matches!(it.content, designcraft_doc::Content::Group { .. }) {
                it.xf = m * it.xf;
            } else {
                let inner = it.xf.inverse() * m * it.xf;
                it.path.transform(inner);
                if let Some(w) = &mut it.wrap.path {
                    w.transform(inner);
                }
                let ib = it.inner_bounds();
                if let designcraft_doc::Content::Graphic(g) = &mut it.content {
                    // Content keeps its size but moves with the frame; auto-fit refits.
                    let o = inner * designcraft_geom::Point::ZERO;
                    g.xf = Affine::translate(o.to_vec2()) * g.xf;
                    if let Some(xf) = g.fitted(ib, g.auto_fit) {
                        g.xf = xf;
                    }
                }
            }
        }
    }
}

/// Horizontal distance from `x` to a margin box (0 inside).
fn dist(r: designcraft_geom::Rect, x: f64) -> f64 {
    if x < r.x0 {
        r.x0 - x
    } else if x > r.x1 {
        x - r.x1
    } else {
        0.0
    }
}

/// Apply the `margins`, `columns` and `gutter` params of `layout.marginsAndColumns` to one page.
fn set_margins_columns(pg: &mut designcraft_doc::Page, p: &Value) {
    let width_before = pg.margin_rect().width();
    match p.get("margins") {
        Some(Value::Number(n)) => pg.margins = Margins::uniform(n.as_f64().unwrap_or(36.0)),
        Some(v @ Value::Object(_)) => {
            if let Ok(m) = serde_json::from_value(v.clone()) {
                pg.margins = m;
            }
        }
        _ => {}
    }
    scale_columns(pg, width_before);
    // A new count or gutter re-divides the columns evenly: dragged column positions are what
    // the new value replaces. The dialog always sends both, so an unchanged value keeps them.
    if let Some(c) = p.get("columns").and_then(Value::as_u64) {
        let c = u32::try_from(c).unwrap_or(u32::MAX).clamp(1, 216);
        if c != pg.columns.count {
            pg.columns.count = c;
            pg.columns.positions = None;
        }
    }
    if let Some(g) = p.get("gutter").and_then(Value::as_f64).filter(|g| g.is_finite()) {
        let g = g.max(0.0);
        if (g - pg.columns.gutter).abs() > GUTTER_TOLERANCE {
            pg.columns.gutter = g;
            pg.columns.positions = None;
        }
    }
}

/// Dragged column positions are measured across the margin box. When the box was
/// `width_before` wide and now has another width, they scale with it, as InDesign's do; a set
/// that doesn't scale into a valid one goes back to the even split rather than stay hidden.
fn scale_columns(pg: &mut designcraft_doc::Page, width_before: f64) {
    let width = pg.margin_rect().width();
    if (width - width_before).abs() <= GUTTER_TOLERANCE {
        return;
    }
    let Some(positions) = pg.columns.positions.as_mut() else { return };
    let factor = width / width_before;
    if factor.is_finite() && factor > 0.0 {
        positions.iter_mut().for_each(|p| *p *= factor);
        // The ends sit exactly on the margins whatever the rounding.
        if let Some(first) = positions.first_mut() {
            *first = 0.0;
        }
        if let Some(last) = positions.last_mut() {
            *last = width;
        }
    }
    if pg.columns.valid_positions(width).is_none() {
        pg.columns.positions = None;
    }
}

/// Gives `pg` the page width `width`, scaling its dragged columns with the margin box.
fn set_page_width(pg: &mut designcraft_doc::Page, width: f64) {
    let before = pg.margin_rect().width();
    pg.width = width;
    scale_columns(pg, before);
}

/// How far a gutter may differ from the page's and still count as unchanged, in points. The
/// dialog shows the gutter in the ruler unit, rounded (inches to four decimals: up to 0.0036 pt
/// off), so an OK that leaves the field alone sends back a slightly different number.
const GUTTER_TOLERANCE: f64 = 0.01;

pub fn specs() -> Vec<CommandSpec> {
    vec![
        cmd!(
            "layout.pages.insert",
            "Insert Pages…",
            ["Layout", "Pages"],
            Some("Cmd+Shift+P"),
            "{count?: 1 (1 to 9999), after?: 0-based page index (default: the last page), parent?: id|\"A-Parent\"|\"A\"|null}",
            has_doc,
            |s, p| {
                const ID: &str = "layout.pages.insert";
                // A count that is given but is not a number of pages used to insert one page:
                // `0`, `-1` and `1.5` all came out as 1, and 10000 was quietly capped.
                let count = match p.get("count").filter(|v| !v.is_null()) {
                    None => 1,
                    Some(v) => match v.as_u64().filter(|n| (1..=9999).contains(n)).map(|n| n as usize) {
                        Some(n) => n,
                        None => return Err(bad(ID, format!("count {v}: 1 to 9999 pages"))),
                    },
                };
                let st = s.doc()?;
                let n = st.doc.page_count();
                // An `after` past the last page appends, which `insert_after_u64_max_appends_at_the_end`
                // pins deliberately.
                let after = p.get("after").and_then(Value::as_u64).map(|v| usize::try_from(v).unwrap_or(usize::MAX)).unwrap_or(n.saturating_sub(1));
                let parent = parent_ref(ID, &st.doc, p.get("parent"))?.unwrap_or_else(|| st.doc.parents.first().map(|x| x.id));
                s.edit(|d, _| {
                    let ids = d.insert_pages(Some(after), count, parent)?;
                    Ok(json!({"pages": ids.len(), "total": d.page_count()}))
                })
            }
        ),
        cmd!(
            "layout.rotateSpreadView",
            "Rotate Spread",
            [],
            None,
            "{spread?: index (0), angle: 90 (clockwise) | -90 | 180 | 0 (clear)} — turn one spread on screen (output is unaffected)",
            has_doc,
            |s, p| {
                let si = p.get("spread").and_then(Value::as_u64).unwrap_or(0) as usize;
                let angle = match p.get("angle") {
                    None => 90,
                    Some(v) => v.as_i64().ok_or_else(|| bad("layout.rotateSpreadView", "angle must be an integer multiple of 90 degrees"))?,
                };
                if angle % 90 != 0 {
                    return Err(bad("layout.rotateSpreadView", "angle must be an integer multiple of 90 degrees"));
                }
                s.edit(|d, _| {
                    let sp = d.spreads.get_mut(si).ok_or_else(|| bad("layout.rotateSpreadView", "no such spread"))?;
                    let pg = std::sync::Arc::make_mut(sp).pages.first_mut().ok_or_else(|| bad("layout.rotateSpreadView", "empty spread"))?;
                    pg.view_rotation = if angle == 0 { 0 } else { ((pg.view_rotation as i64 + angle.div_euclid(90)).rem_euclid(4)) as u8 };
                    Ok(json!({"rotation": pg.view_rotation as u32 * 90}))
                })
            }
        ),
        cmd!("layout.pages.delete", "Delete Pages", ["Layout", "Pages"], None, "{pages: [index]}", has_doc, |s, p| {
            let pages: Vec<usize> = p
                .get("pages")
                .and_then(Value::as_array)
                .map(|a| a.iter().filter_map(Value::as_u64).map(|v| v as usize).collect())
                .unwrap_or_default();
            s.edit(|d, sel| {
                d.delete_pages(&pages)?;
                sel.items.retain(|i| d.item(*i).is_some());
                sel.text = sel.text.filter(|t| d.story(t.story).is_some());
                ok()
            })
        }),
        cmd!("layout.pages.move", "Move Pages…", ["Layout", "Pages"], None, "{from, to}", has_doc, |s, p| {
            let from = p.get("from").and_then(Value::as_u64).ok_or_else(|| bad("layout.pages.move", "missing from"))? as usize;
            let to = p.get("to").and_then(Value::as_u64).ok_or_else(|| bad("layout.pages.move", "missing to"))? as usize;
            s.edit(|d, _| {
                d.move_page(from, to)?;
                ok()
            })
        }),
        cmd!(
            "layout.pages.duplicateSpread",
            "Duplicate Spread",
            ["Layout", "Pages"],
            None,
            "{spread, atEnd?: bool (Duplicate Spread at End of Document; the Pages panel's New Page button takes a dropped page there), to?: page index the copied pages move to in the same undo step (the Pages panel's Alt-drag)} → {page: the copy's first page (0-based)} — the spread's pages (size, margins, columns, guides, liquid rule) and items",
            has_doc,
            |s, p| {
                let si = p.get("spread").and_then(Value::as_u64).unwrap_or(0) as usize;
                let at_end = p.get("atEnd").and_then(Value::as_bool).unwrap_or(false);
                let to = p.get("to").and_then(Value::as_u64).map(|t| t as usize);
                s.edit(|d, _| {
                    let sp = d.spreads.get(si).cloned().ok_or_else(|| bad("layout.pages.duplicateSpread", "no such spread"))?;
                    // The copies land at `to`..`to + n` of the document with them in it.
                    if to.is_some_and(|t| t > d.page_count()) {
                        return Err(bad("layout.pages.duplicateSpread", "no such page"));
                    }
                    let after = if at_end { d.page_count() - 1 } else { d.first_page_of_spread(si) + sp.pages.len() - 1 };
                    let parent = sp.pages.first().and_then(|p| p.parent);
                    d.insert_pages(Some(after), sp.pages.len(), parent)?;
                    // The copies are the pages they copy: size, margins, columns, guides, liquid rule.
                    for (k, src) in sp.pages.iter().enumerate() {
                        let Some((nsi, npi)) = d.page_loc(after + 1 + k) else { continue };
                        let nsp = std::sync::Arc::make_mut(&mut d.spreads[nsi]);
                        let pg = &mut nsp.pages[npi];
                        let dx = pg.x - src.x;
                        // A copy on the other side of the spine has its dragged columns mirrored.
                        let columns = pg.columns_from(src);
                        (pg.width, pg.height, pg.margins, pg.columns, pg.liquid) = (src.width, src.height, src.margins, columns, src.liquid);
                        pg.guides = src.guides.clone();
                        for g in pg.guides.iter_mut().filter(|g| g.orientation == designcraft_doc::Orientation::Vertical) {
                            g.position += dx;
                        }
                        nsp.relayout();
                    }
                    // Copy items onto the new spread(s) at the same positions.
                    let src = d.clone();
                    let ids: Vec<_> = sp.items.iter().map(|i| i.id).collect();
                    let (nsi, _) = d.page_loc(after + 1).ok_or_else(|| bad("layout.pages.duplicateSpread", "insert failed"))?;
                    super::object::duplicate_from(d, &src, &ids, designcraft_doc::SpreadRef::Doc(nsi), designcraft_geom::Vec2::ZERO)?;
                    let first = after + 1;
                    let Some(to) = to else { return Ok(json!({"page": first})) };
                    // Moving a page shifts the ones between; order the moves so each copy keeps
                    // its place among the others.
                    let n = sp.pages.len();
                    if first < to {
                        for k in (0..n).rev() {
                            d.move_page(first + k, to + k)?;
                        }
                    } else {
                        for k in 0..n {
                            d.move_page(first + k, to + k)?;
                        }
                    }
                    Ok(json!({"page": to}))
                })
            }
        ),
        cmd!(
            "layout.pages.applyParent",
            "Apply Parent to Pages…",
            ["Layout", "Pages"],
            None,
            "{pages: [index], parent: id|\"A-Parent\"|\"A\"|null (null or \"[None]\" clears)}",
            has_doc,
            |s, p| {
                let pages: Vec<usize> = p
                    .get("pages")
                    .and_then(Value::as_array)
                    .map(|a| a.iter().filter_map(Value::as_u64).map(|v| v as usize).collect())
                    .unwrap_or_default();
                let parent = parent_ref("layout.pages.applyParent", &s.doc()?.doc, p.get("parent"))?.unwrap_or(None);
                s.edit(|d, _| {
                    d.apply_parent(&pages, parent)?;
                    ok()
                })
            }
        ),
        cmd!("layout.parents.new", "New Parent…", ["Layout", "Pages"], None, "{prefix?, name?: \"Parent\"}", has_doc, |s, p| {
            let name = str_param(p, "name").unwrap_or("Parent").to_string();
            s.edit(|d, _| {
                let prefix = str_param(p, "prefix").map(str::to_string).unwrap_or_else(|| d.next_parent_prefix());
                let src = d.page(0).cloned();
                let (columns, margins) =
                    src.as_ref().map_or_else(|| (designcraft_doc::Columns::default(), Margins::uniform(36.0)), |p| (p.columns.clone(), p.margins));
                let id = d.add_parent(&prefix, &name, columns.count, columns.gutter, margins);
                // Dragged column guides carry over to the parent pages they still fit, mirrored
                // on the parent page across the spine from the first page.
                if let (Some(src), Some(sp)) = (&src, d.parents.iter_mut().find(|x| x.id == id)) {
                    for pg in &mut std::sync::Arc::make_mut(sp).pages {
                        let columns = pg.columns_from(src);
                        if columns.valid_positions(pg.margin_rect().width()).is_some() {
                            pg.columns.positions = columns.positions;
                        }
                    }
                }
                Ok(json!({"id": id.0, "prefix": prefix}))
            })
        }),
        cmd!(
            "layout.parents.delete",
            "Delete Parent",
            [],
            None,
            "{id: parent spread id} — unapply the parent and keep local overrides as ordinary items",
            has_doc,
            |s, p| {
                let id = p.get("id").and_then(Value::as_u64).map(SpreadId).ok_or_else(|| bad("layout.parents.delete", "missing parent id"))?;
                s.edit(|d, sel| {
                    d.delete_parent(id)?;
                    *sel = Default::default();
                    ok()
                })?;
                let st = s.doc_mut()?;
                if st.doc.parents.is_empty() {
                    st.editing_parents = false;
                }
                ok()
            }
        ),
        cmd!(
            noundo "layout.parents.edit",
            "Edit Parents",
            [],
            None,
            "{on?: bool, parent?: id|\"A-Parent\"|\"A\"} — show parent spreads on the canvas; `parent` shows that parent (and implies on). Turning on without `parent` shows the first parent",
            has_doc,
            |s, p| {
                const ID: &str = "layout.parents.edit";
                let doc = s.doc()?.doc.clone();
                let parent = match p.get("parent") {
                    None | Some(Value::Null) => None,
                    v => match parent_ref(ID, &doc, v)? {
                        Some(Some(id)) => Some(id),
                        _ => return Err(bad(ID, "`parent` names a parent to show")),
                    },
                };
                let st = s.doc_mut()?;
                let was = st.editing_parents;
                let on = p.get("on").and_then(Value::as_bool).unwrap_or(parent.is_some() || !was);
                st.editing_parents = on;
                st.selection = Default::default();
                st.bump_revision();
                // The parent to show: the one asked for, or the first when editing starts.
                let index = match parent {
                    Some(id) => doc.parent_index(id),
                    None if !was && !doc.parents.is_empty() => Some(0),
                    None => None,
                };
                let shown = index.filter(|_| on);
                if let Some(i) = shown {
                    s.ui_requests.push(crate::UiRequest::View { params: json!({"showSpread": designcraft_doc::SpreadRef::Parent(i)}) });
                }
                Ok(json!({"on": on, "parent": shown.and_then(|i| doc.parents.get(i)).map(|x| x.id.0)}))
            }
        ),
        cmd!(
            "layout.parents.options",
            "Parent Options…",
            [],
            None,
            "{parent: id|\"B-Parent\"|\"B\", basedOn: id|\"A-Parent\"|\"A\"|null (= [None])} — the parent shows its base's items under its own, and so do the pages that use it → {parent, basedOn}",
            has_doc,
            parent_options
        ),
        cmd!(
            "layout.marginsAndColumns",
            "Margins and Columns…",
            ["Layout"],
            None,
            "{pages?: [index] (default all), margins?: number|{top,bottom,inside,outside}, columns?, gutter?, adjustLayout?: bool (objects follow the margins), parent? (a parent's prefix, label or index)} — document pages take these as their own margins; with `parent` the parent's pages change instead (`pages` then indexes them), and so do the document pages without their own",
            has_doc,
            |s, p| {
                let pages: Option<Vec<usize>> =
                    p.get("pages").and_then(Value::as_array).map(|a| a.iter().filter_map(Value::as_u64).map(|v| v as usize).collect());
                let parent = match p.get("parent") {
                    None | Some(Value::Null) => None,
                    Some(v) => Some(parent_spread_index(&s.doc()?.doc, v).ok_or_else(|| bad("layout.marginsAndColumns", "no such parent"))?),
                };
                let p = p.clone();
                let adjust = p.get("adjustLayout").and_then(Value::as_bool).unwrap_or(false);
                s.edit(|d, _| {
                    let before = adjust.then(|| margin_boxes(d));
                    if let Some(pi) = parent {
                        let n = d.parents.get(pi).map_or(0, |sp| sp.pages.len());
                        let sp = d.parents.get_mut(pi).map(std::sync::Arc::make_mut);
                        if let Some(sp) = sp {
                            for qi in pages.clone().unwrap_or_else(|| (0..n).collect()) {
                                if let Some(pg) = sp.pages.get_mut(qi) {
                                    set_margins_columns(pg, &p);
                                }
                            }
                        }
                        d.sync_parent_margins();
                    } else {
                        let all: Vec<usize> = pages.clone().unwrap_or_else(|| (0..d.page_count()).collect());
                        for abs in all {
                            let Some((si, pi)) = d.page_loc(abs) else { continue };
                            let Some(pg) = d.spreads.get_mut(si).map(std::sync::Arc::make_mut).and_then(|sp| sp.pages.get_mut(pi)) else { continue };
                            set_margins_columns(pg, &p);
                            pg.local_margins = true;
                        }
                    }
                    if let Some(b) = before {
                        adjust_layout(d, &b);
                    }
                    ok()
                })
            }
        ),
        cmd!(
            "layout.moveColumnGuide",
            "Move Column Guide",
            [],
            None,
            "{spread?, parent? (a parent's prefix, label or index: its spread instead of `spread`), page (index in the spread), gutter (0 = between the first two columns), position (the gutter's new left edge, spread x)} → {positions (each column's left and right edge from the margin box's left edge), left (where the gutter's left edge went, spread x)} — both guides of the gutter move, keeping its width, and stop at the neighbouring guides and the margins; a document page takes the result as its own columns, a parent page passes it to the pages that follow it; refused while column guides are locked",
            column_guides_unlocked,
            move_column_guide
        ),
        cmd!(
            "layout.documentSetup",
            "Document Setup…",
            ["File"],
            Some("Cmd+Alt+P"),
            "{width?, height?, pages?: count, startPage?: n, facingPages?, binding?: leftToRight|rightToLeft, intent?: print|web|mobile, margins?: n | {top, bottom, inside, outside} (every page and parent page), bleed?, slug?: n | [top, bottom, inside, outside], adjustLayout?: bool (objects follow the new page size and margins)} → the document setup (margins: the first page's)",
            has_doc,
            |s, p| {
                let p = p.clone();
                if p.as_object().is_none_or(|o| o.is_empty()) {
                    return Ok(document_setup(&s.doc()?.doc));
                }
                let edges = |k: &str| -> Option<[f64; 4]> {
                    match p.get(k)? {
                        Value::Array(a) if a.len() == 4 => {
                            let v: Vec<f64> = a.iter().filter_map(Value::as_f64).map(|v| v.max(0.0)).collect();
                            (v.len() == 4).then(|| [v[0], v[1], v[2], v[3]])
                        }
                        v => v.as_f64().map(|b| [b.max(0.0); 4]),
                    }
                };
                let (bleed, slug) = (edges("bleed"), edges("slug"));
                for k in ["width", "height"] {
                    if let Some(v) = p.get(k)
                        && !v.as_f64().is_some_and(|x| x > 0.0 && x <= 15552.0)
                    {
                        return Err(bad("layout.documentSetup", format!("{k}: page size out of range (0 < size ≤ 216 in)")));
                    }
                }
                let margins: Option<Margins> = match p.get("margins") {
                    None => None,
                    Some(Value::Number(n)) => n.as_f64().map(Margins::uniform),
                    Some(v) => serde_json::from_value(v.clone()).ok(),
                };
                if margins.is_some_and(|m| ![m.top, m.bottom, m.inside, m.outside].iter().all(|v| v.is_finite() && (0.0..=15552.0).contains(v))) {
                    return Err(bad("layout.documentSetup", "margins: 0 to 216 in"));
                }
                if p.get("margins").is_some() && margins.is_none() {
                    return Err(bad("layout.documentSetup", "margins: a number or {top, bottom, inside, outside}"));
                }
                let intent: Option<designcraft_doc::Intent> = p.get("intent").and_then(|v| serde_json::from_value(v.clone()).ok());
                let pages = p.get("pages").and_then(Value::as_u64).map(|n| n.clamp(1, 9999) as usize);
                let adjust = p.get("adjustLayout").and_then(Value::as_bool).unwrap_or(false);
                s.edit(|d, _| {
                    let before = adjust.then(|| margin_boxes(d));
                    if let Some(f) = p.get("facingPages").and_then(Value::as_bool) {
                        d.settings.facing_pages = f;
                    }
                    if let Some(b) = p.get("binding").and_then(Value::as_str) {
                        d.settings.right_to_left_binding = b == "rightToLeft";
                        d.order_parent_pages();
                    }
                    if let Some(b) = bleed {
                        d.settings.bleed = b;
                    }
                    if let Some(b) = slug {
                        d.settings.slug = b;
                    }
                    if let Some(i) = intent {
                        d.settings.intent = i;
                    }
                    if let Some(n) = p.get("startPage").map(|v| start_number("layout.documentSetup", "startPage", v)).transpose()? {
                        if !d.sections.iter().any(|x| x.start == 0) {
                            d.sections.insert(
                                0,
                                designcraft_doc::Section {
                                    start: 0,
                                    start_number: None,
                                    style: Default::default(),
                                    prefix: String::new(),
                                    marker: String::new(),
                                    include_prefix: false,
                                },
                            );
                        }
                        if let Some(first) = d.sections.iter_mut().find(|x| x.start == 0) {
                            first.start_number = Some(n);
                        }
                    }
                    // Number of Pages: add at the end (with the last page's parent) or remove from the end.
                    if let Some(n) = pages {
                        let have = d.page_count();
                        if n > have {
                            let parent = d.page_loc(have - 1).and_then(|(si, pi)| d.spreads[si].pages[pi].parent);
                            d.insert_pages(Some(have - 1), n - have, parent)?;
                        } else if n < have {
                            d.delete_pages(&(n..have).collect::<Vec<_>>())?;
                        }
                    }
                    let w = p.get("width").and_then(Value::as_f64);
                    let h = p.get("height").and_then(Value::as_f64);
                    // Pages with a liquid rule re-flow their objects (Adjust Layout aside).
                    let liquid_before: Vec<(usize, usize, designcraft_geom::Rect)> = if adjust {
                        vec![]
                    } else {
                        d.spreads
                            .iter()
                            .enumerate()
                            .flat_map(|(si, sp)| {
                                sp.pages.iter().enumerate().filter(|(_, pg)| !pg.liquid.is_off()).map(move |(pi, pg)| (si, pi, pg.bounds()))
                            })
                            .collect()
                    };
                    if w.is_some() || h.is_some() {
                        d.settings.page_width = w.unwrap_or(d.settings.page_width);
                        d.settings.page_height = h.unwrap_or(d.settings.page_height);
                        for sp in d.spreads.iter_mut().chain(d.parents.iter_mut()) {
                            let sp = std::sync::Arc::make_mut(sp);
                            for pg in &mut sp.pages {
                                set_page_width(pg, d.settings.page_width);
                                pg.height = d.settings.page_height;
                            }
                            sp.relayout();
                        }
                    }
                    if let Some(m) = margins {
                        for sp in d.spreads.iter_mut().chain(d.parents.iter_mut()) {
                            for pg in &mut std::sync::Arc::make_mut(sp).pages {
                                pg.margins = m;
                            }
                        }
                        // Pages that now match their parent page follow its later changes; local
                        // columns stay local.
                        d.unmark_matching_local_margins();
                        d.sync_parent_margins();
                    }
                    d.repaginate();
                    if let Some(b) = before {
                        adjust_layout(d, &b);
                    }
                    for (si, pi, old) in liquid_before {
                        super::liquid::apply(d, designcraft_doc::SpreadRef::Doc(si), pi, old);
                    }
                    Ok(document_setup(d))
                })
            }
        ),
        cmd!(
            "layout.pages.toSpread",
            "Add Page to Spread",
            ["Layout", "Pages"],
            None,
            "{page (1-based), spread (0-based)} — the page joins that spread (up to 10 pages; the spread stops shuffling)",
            has_doc,
            |s, p| {
                let page =
                    p.get("page").and_then(Value::as_u64).filter(|n| *n >= 1).ok_or_else(|| bad("layout.pages.toSpread", "missing page"))? as usize;
                let spread = p.get("spread").and_then(Value::as_u64).ok_or_else(|| bad("layout.pages.toSpread", "missing spread"))? as usize;
                s.edit(|d, _| {
                    d.page_to_spread(page - 1, spread)?;
                    Ok(json!({"spreads": d.spreads.len()}))
                })
            }
        ),
        cmd!(
            "layout.spreadShuffle",
            "Allow Selected Spread to Shuffle",
            ["Layout", "Pages"],
            None,
            "{spread (0-based), allow: bool} — off keeps the spread's pages together when pages are added or removed",
            has_doc,
            |s, p| {
                let si = p.get("spread").and_then(Value::as_u64).unwrap_or(0) as usize;
                let allow = p.get("allow").and_then(Value::as_bool).unwrap_or(true);
                s.edit(|d, _| {
                    let sp = d.spreads.get_mut(si).ok_or_else(|| bad("layout.spreadShuffle", "no such spread"))?;
                    std::sync::Arc::make_mut(sp).allow_shuffle = allow;
                    if allow {
                        d.repaginate();
                    }
                    ok()
                })
            }
        ),
        cmd!(
            "layout.pageSize",
            "Page Size",
            [],
            None,
            "{pages: [1-based page numbers], width?, height?, preset?: \"A4\"|\"Letter\"|…, anchor?: 0–8 (reference point that stays put, row-major from the top-left; default 0)} — pages of their own size (Page tool); objects keep their place relative to the anchor, or follow the page's liquid rule",
            has_doc,
            page_size
        ),
        cmd!(
            "layout.pageOptions",
            "Page Options",
            [],
            None,
            "{pages: [1-based], showParentOverlay?: bool} — Page tool options (Show Parent Page Overlay: the parent page's size drawn over the page)",
            has_doc,
            page_options
        ),
        cmd!(
            noundo "layout.selectPage",
            "Select Page",
            [],
            None,
            "{page: 1-based | null, ids?: [item ids] (objects selected with it: the Page tool on an object-based page)} — the page the Page tool selects (its handles, the Control panel)",
            has_doc,
            |s, p| {
            let n = s.doc()?.doc.page_count();
            let page = match p.get("page") {
                None | Some(Value::Null) => None,
                Some(v) => Some(v.as_u64().filter(|v| *v >= 1 && *v as usize <= n).ok_or_else(|| bad("layout.selectPage", format!("`page`: 1–{n}")))? as usize - 1),
            };
            let ids: Vec<designcraft_doc::ItemId> = {
                let d = &s.doc()?.doc;
                p.get("ids")
                    .and_then(Value::as_array)
                    .map(|a| a.iter().filter_map(Value::as_u64).map(designcraft_doc::ItemId).filter(|id| d.item(*id).is_some()).collect())
                    .unwrap_or_default()
            };
            let st = s.doc_mut()?;
            if page.is_some() {
                st.selection = designcraft_doc::Selection::items(ids);
            }
            st.selection.page = page;
            st.bump_revision();
            Ok(json!({"page": page.map(|p| p + 1)}))
        }
        ),
        cmd!(
            "layout.section",
            "Numbering & Section Options…",
            ["Layout"],
            None,
            "{page (1-based; starts a section there), startNumber?: n (1–99999)|null (continue), style?: arabic|upperRoman|lowerRoman|upperLetters|lowerLetters|arabicLeadingZero|arabicThreeDigits|arabicFourDigits|kanji, prefix? (up to 8 characters; no + or comma), includePrefix?, marker?, remove?: bool}",
            has_doc,
            |s, p| {
                let edit = SectionEdit::parse("layout.section", s, p)?;
                s.edit(|d, _| {
                    edit.apply(d);
                    Ok(json!({"names": (0..d.page_count()).map(|i| d.page_name(i)).collect::<Vec<_>>()}))
                })
            }
        ),
        cmd!(
            "layout.chapterNumbering",
            "Document Chapter Numbering",
            [],
            None,
            "{style?: arabic|upperRoman|lowerRoman|upperLetters|lowerLetters|arabicLeadingZero|arabicThreeDigits|arabicFourDigits, start?: n (Start Chapter Numbering at), source?: automatic|userDefined|sameAsPrevious} → {chapterNumber, label, style, source, book: the open book holding the document, or null}; no parameters reports the setting",
            has_doc,
            |s, p| {
                if p.as_object().is_none_or(|o| o.is_empty()) {
                    return chapter_numbering(s);
                }
                let edit = ChapterEdit::parse("layout.chapterNumbering", p)?;
                s.edit(|d, _| {
                    edit.apply(d);
                    ok()
                })?;
                chapter_numbering(s)
            }
        ),
        cmd!(
            "layout.numberingOptions",
            "Apply Numbering & Section Options",
            [],
            None,
            "{page, …the settings as layout.section, chapter?: {…the settings as layout.chapterNumbering}} — the section and the document's chapter numbering in one undo step; if either is refused nothing changes",
            has_doc,
            |s, p| {
                let section = SectionEdit::parse("layout.numberingOptions", s, p)?;
                let chapter = match p.get("chapter") {
                    Some(c) => Some(ChapterEdit::parse("layout.numberingOptions", c)?),
                    None => None,
                };
                s.edit(|d, _| {
                    section.apply(d);
                    if let Some(chapter) = &chapter {
                        chapter.apply(d);
                    }
                    Ok(json!({"names": (0..d.page_count()).map(|i| d.page_name(i)).collect::<Vec<_>>(), "chapter": d.chapter_label()}))
                })
            }
        ),
        cmd!("layer.move", "Move Layer", [], None, "{id, to: index (0 = top/frontmost)}", has_doc, |s, p| {
            let id = LayerId(p.get("id").and_then(Value::as_u64).ok_or_else(|| bad("layer.move", "missing id"))?);
            let to = p.get("to").and_then(Value::as_u64).ok_or_else(|| bad("layer.move", "missing to"))?;
            s.edit(|d, _| {
                let i = d.layers.iter().position(|l| l.id == id).ok_or_else(|| bad("layer.move", "no such layer"))?;
                let to = usize::try_from(to).ok().filter(|t| *t < d.layers.len()).ok_or_else(|| bad("layer.move", "to out of range"))?;
                let l = d.layers.remove(i);
                d.layers.insert(to, l);
                ok()
            })
        }),
        cmd!(noundo "layer.selectItems", "Select All on Layer", [], None, "{id}", has_doc, |s, p| {
            let id = LayerId(p.get("id").and_then(Value::as_u64).unwrap_or(0));
            let st = s.doc_mut()?;
            let ids: Vec<_> = st.doc.spreads.iter().flat_map(|sp| sp.items.iter().filter(|i| i.layer == id && !i.locked).map(|i| i.id)).collect();
            st.selection = designcraft_doc::Selection::items(ids);
            st.bump_revision();
            ok()
        }),
        cmd!("layer.new", "New Layer…", [], None, "{name?}", has_doc, |s, p| {
            let name = str_param(p, "name").map(str::to_string);
            let r = s.edit(|d, _| {
                let id = LayerId(d.alloc());
                let n = d.layers.len();
                let name = name.clone().unwrap_or_else(|| format!("Layer {}", n + 1));
                d.layers.insert(
                    0,
                    Layer {
                        id,
                        name,
                        color: LAYER_COLORS[n % LAYER_COLORS.len()].1,
                        visible: true,
                        locked: false,
                        printable: true,
                        show_guides: true,
                        suppress_wrap_when_hidden: false,
                    },
                );
                Ok(id)
            })?;
            s.doc_mut()?.active_layer = r;
            Ok(json!({"id": r.0}))
        }),
        cmd!(
            "layer.set",
            "Layer Options…",
            [],
            None,
            "{id, name?, color?: [r,g,b] | layer colour name (\"Light Blue\", \"Red\", …), visible?, locked?, printable?, showGuides?, suppressWrapWhenHidden?} — names are unique and not empty",
            has_doc,
            set_layer
        ),
        cmd!("layer.delete", "Delete Layer", [], None, "{id}", has_doc, |s, p| {
            let id = LayerId(p.get("id").and_then(Value::as_u64).unwrap_or(0));
            s.edit(|d, sel| {
                if d.layers.len() < 2 {
                    return Err(bad("layer.delete", "a document needs at least one layer"));
                }
                let doomed: Vec<_> =
                    d.spreads.iter().chain(d.parents.iter()).flat_map(|sp| sp.items.iter().filter(|i| i.layer == id).map(|i| i.id)).collect();
                for i in doomed {
                    let _ = d.remove_item(i);
                }
                d.layers.retain(|l| l.id != id);
                *sel = Default::default();
                ok()
            })
        }),
        cmd!(
            "layer.merge",
            "Merge Layers",
            [],
            None,
            "{ids: [layer ids], into?: layer id (default: the first)} — their objects move to `into`, the other layers are deleted",
            has_doc,
            |s, p| {
                let ids: Vec<LayerId> =
                    p.get("ids").and_then(Value::as_array).map(|a| a.iter().filter_map(Value::as_u64).map(LayerId).collect()).unwrap_or_default();
                let into = p
                    .get("into")
                    .and_then(Value::as_u64)
                    .map(LayerId)
                    .or_else(|| ids.first().copied())
                    .ok_or_else(|| bad("layer.merge", "no layers"))?;
                s.edit(|d, _| {
                    if d.layer(into).is_none() || ids.iter().any(|l| d.layer(*l).is_none()) {
                        return Err(bad("layer.merge", "no such layer"));
                    }
                    let gone: Vec<LayerId> = ids.iter().copied().filter(|l| *l != into).collect();
                    for sp in d.spreads.iter_mut().chain(d.parents.iter_mut()) {
                        if !sp.items.iter().any(|i| gone.contains(&i.layer)) {
                            continue;
                        }
                        for it in &mut std::sync::Arc::make_mut(sp).items {
                            if gone.contains(&it.layer) {
                                std::sync::Arc::make_mut(it).layer = into;
                            }
                        }
                    }
                    d.layers.retain(|l| !gone.contains(&l.id));
                    Ok(json!({"merged": gone.len()}))
                })
            }
        ),
        cmd!("layer.deleteUnused", "Delete Unused Layers", [], None, "{} — layers without objects (one layer always stays)", has_doc, |s, _| {
            s.edit(|d, _| {
                let used: std::collections::HashSet<LayerId> =
                    d.spreads.iter().chain(d.parents.iter()).flat_map(|sp| sp.items.iter().map(|i| i.layer)).collect();
                let before = d.layers.len();
                if used.is_empty() {
                    // Nothing anywhere: keep the first layer.
                    d.layers.truncate(1);
                } else {
                    d.layers.retain(|l| used.contains(&l.id));
                }
                Ok(json!({"deleted": before - d.layers.len()}))
            })
        }),
        cmd!(
            "layer.others",
            "Hide/Lock Others",
            [],
            None,
            "{id, hide?: bool, lock?: bool, show?: true (Show All Layers), unlock?: true (Unlock All Layers)} — Hide Others / Lock Others act on every layer but `id`",
            has_doc,
            |s, p| {
                let id = LayerId(p.get("id").and_then(Value::as_u64).unwrap_or(0));
                let f = |k: &str| p.get(k).and_then(Value::as_bool).unwrap_or(false);
                let (hide, lock, show, unlock) = (f("hide"), f("lock"), f("show"), f("unlock"));
                s.edit(|d, _| {
                    for l in &mut d.layers {
                        if show {
                            l.visible = true;
                        }
                        if unlock {
                            l.locked = false;
                        }
                        if l.id != id {
                            if hide {
                                l.visible = false;
                            }
                            if lock {
                                l.locked = true;
                            }
                        }
                    }
                    ok()
                })
            }
        ),
        cmd!(noundo "layer.activate", "Set Active Layer", [], None, "{id}", has_doc, |s, p| {
            let id = LayerId(p.get("id").and_then(Value::as_u64).unwrap_or(0));
            let st = s.doc_mut()?;
            if st.doc.layer(id).is_none() {
                return Err(bad("layer.activate", "no such layer"));
            }
            st.active_layer = id;
            st.bump_revision();
            ok()
        }),
    ]
}

/// Column guides can be dragged: a document is open and View › Grids & Guides › Lock Column
/// Guides is off.
fn column_guides_unlocked(s: &Session) -> std::result::Result<(), String> {
    has_doc(s)?;
    match s.active() {
        Some(st) if st.doc.settings.column_guides_locked => Err("column guides are locked (View › Grids & Guides › Lock Column Guides)".into()),
        _ => Ok(()),
    }
}

/// A column guide dragged with the Selection tool: gutter `gutter` of a page moves to `position`.
fn move_column_guide(s: &mut Session, p: &Value) -> Result<Value> {
    const CMD: &str = "layout.moveColumnGuide";
    let index = |key: &str| -> Result<usize> {
        let v = p.get(key).ok_or_else(|| bad(CMD, format!("missing `{key}`")))?;
        v.as_u64().and_then(|n| usize::try_from(n).ok()).ok_or_else(|| bad(CMD, format!("`{key}` must be a non-negative integer")))
    };
    let (page, gutter) = (index("page")?, index("gutter")?);
    let position =
        p.get("position").and_then(Value::as_f64).filter(|v| v.is_finite()).ok_or_else(|| bad(CMD, "`position` must be a finite number"))?;
    let r = match p.get("parent") {
        None | Some(Value::Null) => super::spread_param(p, CMD)?,
        Some(v) => designcraft_doc::SpreadRef::Parent(parent_spread_index(&s.doc()?.doc, v).ok_or_else(|| bad(CMD, "no such parent"))?),
    };
    s.edit(|d, _| {
        if d.settings.column_guides_locked {
            return Err(bad(CMD, "column guides are locked"));
        }
        let pg = d.spread_mut(r).ok_or_else(|| bad(CMD, "no such spread"))?.pages.get_mut(page).ok_or_else(|| bad(CMD, "no such page"))?;
        let m = pg.margin_rect();
        let count = pg.columns.count;
        if gutter.checked_add(1).is_none_or(|g| g >= count as usize) {
            return Err(bad(CMD, format!("the page has {} gutters", count.saturating_sub(1))));
        }
        let positions =
            pg.columns.with_gutter_moved(m.width(), gutter, position - m.x0).ok_or_else(|| bad(CMD, "the columns cannot take that position"))?;
        let left = positions.get(gutter * 2 + 1).map_or(position, |v| m.x0 + v);
        pg.columns.positions = Some(positions.clone());
        match r {
            designcraft_doc::SpreadRef::Doc(_) => pg.local_margins = true,
            designcraft_doc::SpreadRef::Parent(_) => d.sync_parent_margins(),
        }
        Ok(json!({"positions": positions, "left": left}))
    })
}

/// A parent spread named by prefix, label (`A-Parent`) or index.
fn parent_spread_index(d: &designcraft_doc::Document, v: &Value) -> Option<usize> {
    match v {
        Value::Number(n) => n.as_u64().and_then(|i| usize::try_from(i).ok()).filter(|i| *i < d.parents.len()),
        Value::String(s) => d.parents.iter().position(|p| p.parent.as_ref().is_some_and(|i| i.prefix == *s || i.label() == *s)),
        _ => None,
    }
}

/// A parent named by a command parameter: its id (a number), its full label (`"A-Parent"`), or
/// its prefix (`"A"`) when only one parent has it. `null` or `"[None]"` → `Some(None)` (no parent);
/// missing → `None` (the command's default). Unknown or ambiguous names are errors, so a typo never
/// clears a page's parent.
fn parent_ref(cmd: &str, d: &designcraft_doc::Document, v: Option<&Value>) -> Result<Option<Option<SpreadId>>> {
    let Some(v) = v else { return Ok(None) };
    match v {
        Value::Null => Ok(Some(None)),
        Value::String(s) if s == "[None]" => Ok(Some(None)),
        Value::Number(n) => {
            let id = n.as_u64().ok_or_else(|| bad(cmd, format!("no parent with id {n}")))?;
            d.parents.iter().find(|p| p.id.0 == id).map(|p| Some(Some(p.id))).ok_or_else(|| bad(cmd, format!("no parent with id {id}")))
        }
        Value::String(s) => {
            let by = |f: &dyn Fn(&designcraft_doc::ParentInfo) -> bool| -> Vec<SpreadId> {
                d.parents.iter().filter(|p| p.parent.as_ref().is_some_and(f)).map(|p| p.id).collect()
            };
            let mut hits = by(&|i| i.label() == *s);
            if hits.is_empty() {
                hits = by(&|i| i.prefix == *s);
            }
            match hits.as_slice() {
                [id] => Ok(Some(Some(*id))),
                [] => Err(bad(cmd, format!("no parent named `{s}`"))),
                _ => Err(bad(cmd, format!("more than one parent is named `{s}`: give its id"))),
            }
        }
        _ => Err(bad(cmd, "`parent` is a parent id, a parent name or null")),
    }
}

/// Layout › Pages › Parent Options: Based on Parent. A base that would make a cycle is refused.
fn parent_options(s: &mut crate::Session, p: &Value) -> Result<Value> {
    const CMD: &str = "layout.parents.options";
    let d = &s.doc()?.doc;
    let Some(Some(id)) = parent_ref(CMD, d, p.get("parent"))? else { return Err(bad(CMD, "`parent` names the parent to change")) };
    let pi = d.parent_index(id).ok_or_else(|| bad(CMD, format!("no parent with id {}", id.0)))?;
    let base = parent_ref(CMD, d, p.get("basedOn"))?.ok_or_else(|| bad(CMD, "missing `basedOn` (a parent, or null for [None])"))?;
    let label = |d: &designcraft_doc::Document, i: usize| d.parents.get(i).and_then(|x| x.parent.as_ref()).map(|i| i.label());
    if let Some(b) = base
        && d.parent_base_cycles(pi, b)
    {
        let (name, b) = (label(d, pi).unwrap_or_default(), d.parent_index(b).and_then(|i| label(d, i)).unwrap_or_default());
        return Err(bad(CMD, format!("{name} cannot be based on {b}: that would make a cycle")));
    }
    let out = json!({"parent": label(d, pi), "basedOn": base.and_then(|b| d.parent_index(b)).and_then(|i| label(d, i))});
    if d.parents.get(pi).and_then(|x| x.parent.as_ref()).and_then(|i| i.based_on) == base {
        return Ok(out);
    }
    s.edit(|d, _| {
        let info = d.parents.get_mut(pi).map(std::sync::Arc::make_mut).and_then(|x| x.parent.as_mut()).ok_or_else(|| bad(CMD, "no such parent"))?;
        info.based_on = base;
        Ok(out)
    })
}

#[allow(dead_code)]
fn _r(_: Result<()>) {}

/// Layout › Page Size (the Page tool): resize pages one by one. Left pages grow to the left
/// (the spine stays put); objects move with their page.
fn page_size(s: &mut crate::Session, p: &Value) -> Result<Value> {
    let pages: Vec<usize> =
        p.get("pages").and_then(Value::as_array).map(|a| a.iter().filter_map(Value::as_u64).map(|n| n as usize).collect()).unwrap_or_default();
    let n = s.doc()?.doc.page_count();
    if pages.is_empty() || pages.iter().any(|x| *x == 0 || *x > n) {
        return Err(bad("layout.pageSize", "give `pages` (1-based)"));
    }
    let (mut w, mut h) = (p.get("width").and_then(Value::as_f64), p.get("height").and_then(Value::as_f64));
    if let Some(name) = str_param(p, "preset") {
        let pr = designcraft_doc::build::PRESETS
            .iter()
            .find(|x| x.name.eq_ignore_ascii_case(name))
            .ok_or_else(|| bad("layout.pageSize", format!("no preset `{name}`")))?;
        w = Some(pr.width);
        h = Some(pr.height);
    }
    let anchor = match p.get("anchor") {
        None | Some(Value::Null) => 0,
        Some(v) => v.as_u64().filter(|a| *a <= 8).ok_or_else(|| bad("layout.pageSize", "`anchor`: 0–8"))? as usize,
    };
    s.edit(|d, _| {
        resize_pages_from(d, &pages, w, h, anchor);
        Ok(json!({"pages": pages.len()}))
    })
}

fn page_options(s: &mut crate::Session, p: &Value) -> Result<Value> {
    let pages: Vec<usize> =
        p.get("pages").and_then(Value::as_array).map(|a| a.iter().filter_map(Value::as_u64).map(|n| n as usize).collect()).unwrap_or_default();
    let n = s.doc()?.doc.page_count();
    if pages.is_empty() || pages.iter().any(|x| *x == 0 || *x > n) {
        return Err(bad("layout.pageOptions", "give `pages` (1-based)"));
    }
    let overlay = p.get("showParentOverlay").and_then(Value::as_bool);
    s.edit(|d, _| {
        for page in &pages {
            let Some((si, pi)) = d.page_loc(page - 1) else { continue };
            let pg = &mut std::sync::Arc::make_mut(&mut d.spreads[si]).pages[pi];
            if let Some(v) = overlay {
                pg.show_parent_overlay = v;
            }
        }
        Ok(json!({"pages": pages.len()}))
    })
}

/// Give `pages` (1-based) their own size. Objects stay on their page; pages with a liquid rule
/// re-flow their objects by it.
pub(crate) fn resize_pages(d: &mut designcraft_doc::Document, pages: &[usize], w: Option<f64>, h: Option<f64>) {
    resize_pages_from(d, pages, w, h, 0);
}

/// [`resize_pages`] keeping reference point `anchor` (0–8, row-major from the top-left) in place:
/// the page grows away from it, so objects keep their place relative to it.
pub(crate) fn resize_pages_from(d: &mut designcraft_doc::Document, pages: &[usize], w: Option<f64>, h: Option<f64>, anchor: usize) {
    let (fx, fy) = ((anchor % 3) as f64 / 2.0, (anchor / 3).min(2) as f64 / 2.0);
    {
        for page in pages {
            let Some((si, pi)) = d.page_loc(page - 1) else { continue };
            let sp = std::sync::Arc::make_mut(&mut d.spreads[si]);
            let old: Vec<f64> = sp.pages.iter().map(|x| x.x).collect();
            let old_bounds = sp.pages[pi].bounds();
            // The page's own objects (outer pages take the pasteboard beside them).
            let last = sp.pages.len() - 1;
            let (lo, hi) = (if pi == 0 { f64::NEG_INFINITY } else { old_bounds.x0 }, if pi == last { f64::INFINITY } else { old_bounds.x1 });
            let own: Vec<usize> = sp
                .items
                .iter()
                .enumerate()
                .filter(|(_, it)| {
                    let c = it.bounds().center().x;
                    c >= lo && c < hi
                })
                .map(|(i, _)| i)
                .collect();
            let pg = &mut sp.pages[pi];
            let dw = w.map_or(0.0, |w| w.max(1.0) - pg.width);
            let dh = h.map_or(0.0, |h| h.max(1.0) - pg.height);
            if let Some(w) = w {
                set_page_width(pg, w.max(1.0));
            }
            if let Some(h) = h {
                pg.height = h.max(1.0);
            }
            let left = pg.side == designcraft_doc::PageSide::Left;
            sp.relayout();
            // Keep the spine: shift the whole spread back when a left page grows.
            if left && dw != 0.0 {
                for q in &mut sp.pages {
                    q.x -= dw;
                }
            }
            let new: Vec<f64> = sp.pages.iter().map(|x| x.x).collect();
            // Objects follow their page.
            let shifts: Vec<(f64, f64, f64)> =
                sp.pages.iter().enumerate().map(|(k, q)| (old[k], old[k] + q.width - if k == pi { dw } else { 0.0 }, new[k] - old[k])).collect();
            for it in &mut sp.items {
                let cx = it.bounds().center().x;
                if let Some((_, _, dx)) = shifts.iter().find(|(a, b, _)| cx >= *a && cx < *b)
                    && *dx != 0.0
                {
                    let it = std::sync::Arc::make_mut(it);
                    it.xf = designcraft_geom::Affine::translate((*dx, 0.0)) * it.xf;
                }
            }
            // Page guides move with their page.
            for (k, q) in sp.pages.iter_mut().enumerate() {
                let dx = new[k] - old[k];
                if dx != 0.0 {
                    for g in q.guides.iter_mut().filter(|g| g.orientation == designcraft_doc::Orientation::Vertical) {
                        g.position += dx;
                    }
                }
            }
            // Growing away from the anchor: the page's objects and guides keep their place
            // relative to it.
            let shift = designcraft_geom::Vec2::new(fx * dw, fy * dh);
            if shift != designcraft_geom::Vec2::ZERO {
                for i in &own {
                    if let Some(it) = sp.items.get_mut(*i) {
                        let it = std::sync::Arc::make_mut(it);
                        it.xf = designcraft_geom::Affine::translate(shift) * it.xf;
                    }
                }
                for g in &mut sp.pages[pi].guides {
                    g.position += if g.orientation == designcraft_doc::Orientation::Vertical { shift.x } else { shift.y };
                }
            }
            // Liquid rules: objects were carried along with the page's left edge (and the anchor).
            let moved = old_bounds + designcraft_geom::Vec2::new(new[pi] - old[pi], 0.0) + shift;
            super::liquid::apply(d, designcraft_doc::SpreadRef::Doc(si), pi, moved);
        }
    }
}

/// File › Document Setup values.
fn document_setup(d: &designcraft_doc::Document) -> Value {
    let st = &d.settings;
    let start = d.sections.iter().find(|x| x.start == 0).and_then(|x| x.start_number).unwrap_or(1);
    json!({"width": st.page_width, "height": st.page_height, "pages": d.page_count(), "startPage": start, "facingPages": st.facing_pages,
        "binding": if st.right_to_left_binding { "rightToLeft" } else { "leftToRight" }, "intent": st.intent, "bleed": st.bleed, "slug": st.slug,
        "margins": d.page(0).map(|p| p.margins), "primaryTextFrame": st.primary_text_frame})
}

/// The largest chapter number (as for page numbers).
const MAX_CHAPTER: u64 = designcraft_doc::MAX_PAGE_NUMBER as u64;

/// A `layout.section` edit, checked before the document changes.
struct SectionEdit {
    /// Absolute index of the section's first page.
    start: usize,
    remove: bool,
    /// Some(None): continue from the previous section.
    start_number: Option<Option<u32>>,
    style: Option<designcraft_doc::NumberStyle>,
    prefix: Option<String>,
    include_prefix: Option<bool>,
    marker: Option<String>,
}

impl SectionEdit {
    fn parse(cmd: &str, s: &Session, p: &Value) -> Result<Self> {
        let page = p.get("page").and_then(Value::as_u64).ok_or_else(|| bad(cmd, "missing page"))?;
        let n = s.doc()?.doc.page_count() as u64;
        if page == 0 || page > n {
            return Err(bad(cmd, format!("no page {page}")));
        }
        let prefix = p.get("prefix").and_then(Value::as_str).map(str::to_string);
        if prefix.as_ref().is_some_and(|x| x.chars().count() > 8 || x.contains(['+', ','])) {
            return Err(bad(cmd, "section prefix: up to 8 characters, without + or ,"));
        }
        let start_number = match p.get("startNumber") {
            None => None,
            Some(Value::Null) => Some(None),
            Some(v) => Some(Some(start_number(cmd, "startNumber", v)?)),
        };
        let style = match p.get("style") {
            None => None,
            Some(v) => match serde_json::from_value(v.clone()) {
                Ok(designcraft_doc::NumberStyle::Symbols) => return Err(bad(cmd, "symbols don't number pages")),
                Ok(style) => Some(style),
                Err(e) => return Err(bad(cmd, e.to_string())),
            },
        };
        Ok(SectionEdit {
            start: (page - 1) as usize,
            remove: p.get("remove").and_then(Value::as_bool).unwrap_or(false),
            start_number,
            style,
            prefix,
            include_prefix: p.get("includePrefix").and_then(Value::as_bool),
            marker: p.get("marker").and_then(Value::as_str).map(str::to_string),
        })
    }

    fn apply(&self, d: &mut designcraft_doc::Document) {
        let start = self.start;
        // Page 1 always starts a section.
        if self.remove && start != 0 {
            d.sections.retain(|x| x.start != start);
            // The pages after it follow the previous section's numbers again.
            d.repaginate();
            return;
        }
        let mut sec = d.sections.iter().find(|x| x.start == start).cloned().unwrap_or(designcraft_doc::Section {
            start,
            start_number: None,
            style: Default::default(),
            prefix: String::new(),
            marker: String::new(),
            include_prefix: false,
        });
        if let Some(n) = self.start_number {
            sec.start_number = n;
        }
        if let Some(style) = self.style {
            sec.style = style;
        }
        if let Some(prefix) = &self.prefix {
            sec.prefix = prefix.clone();
        }
        if let Some(on) = self.include_prefix {
            sec.include_prefix = on;
        }
        if let Some(marker) = &self.marker {
            sec.marker = marker.clone();
        }
        d.sections.retain(|x| x.start != start);
        d.sections.push(sec);
        d.sections.sort_by_key(|x| x.start);
        // Facing pages take their sides from their numbers: odd on the right.
        d.repaginate();
    }
}

/// A `layout.chapterNumbering` edit, checked before the document changes.
struct ChapterEdit {
    style: Option<designcraft_doc::NumberStyle>,
    source: Option<designcraft_doc::ChapterSource>,
    start: Option<u32>,
}

impl ChapterEdit {
    fn parse(cmd: &str, p: &Value) -> Result<Self> {
        let style = match p.get("style") {
            Some(v) => match serde_json::from_value::<designcraft_doc::NumberStyle>(v.clone()) {
                Ok(designcraft_doc::NumberStyle::Symbols) | Err(_) => return Err(bad(cmd, format!("unknown chapter style {v}"))),
                Ok(style) => Some(style),
            },
            None => None,
        };
        let mut source = match p.get("source") {
            Some(v) => Some(
                serde_json::from_value::<designcraft_doc::ChapterSource>(v.clone())
                    .map_err(|_| bad(cmd, format!("unknown chapter number source {v}")))?,
            ),
            None => None,
        };
        let start = match p.get("start") {
            Some(v) => {
                Some(v.as_u64().filter(|n| (1..=MAX_CHAPTER).contains(n)).ok_or_else(|| bad(cmd, format!("chapter start: 1 to {MAX_CHAPTER}")))?
                    as u32)
            }
            None => None,
        };
        // A start number alone means Start Chapter Numbering at.
        if start.is_some() && source.is_none() {
            source = Some(designcraft_doc::ChapterSource::UserDefined);
        }
        Ok(ChapterEdit { style, source, start })
    }

    fn apply(&self, d: &mut designcraft_doc::Document) {
        if let Some(style) = self.style {
            d.settings.chapter_style = style;
        }
        if let Some(source) = self.source {
            d.settings.chapter_source = source;
        }
        if let Some(n) = self.start {
            d.settings.chapter_number = n;
        }
    }
}

/// The active document's chapter numbering, and the open book that holds it (what
/// `layout.chapterNumbering {}` reports, read without running a command).
pub fn chapter_numbering(s: &Session) -> Result<Value> {
    let st = s.doc()?;
    let set = &st.doc.settings;
    let book = s
        .book
        .as_ref()
        .filter(|b| st.path.as_deref().is_some_and(|p| b.documents.iter().any(|d| std::path::Path::new(d) == std::path::Path::new(p))))
        .map(|b| std::path::Path::new(&b.path).file_stem().map_or_else(|| b.path.clone(), |n| n.to_string_lossy().to_string()));
    Ok(
        json!({"chapterNumber": set.chapter_number.max(1), "label": st.doc.chapter_label(), "style": set.chapter_style, "source": set.chapter_source, "book": book}),
    )
}

/// Layer Options: every field in one undo step; a bad name or colour changes nothing.
fn set_layer(s: &mut Session, p: &Value) -> Result<Value> {
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

#[cfg(test)]
mod setup_tests {
    use serde_json::json;

    #[test]
    fn chapter_numbering_sets_style_and_start() {
        let mut s = crate::Session::new();
        s.execute("file.new", &json!({})).unwrap();
        let r = s.execute("layout.chapterNumbering", &json!({})).unwrap();
        assert_eq!(r, json!({"chapterNumber": 1, "label": "1", "style": "arabic", "source": "automatic", "book": null}));
        let r = s.execute("layout.chapterNumbering", &json!({"start": 4, "style": "upperRoman"})).unwrap();
        assert_eq!((&r["label"], &r["source"]), (&json!("IV"), &json!("userDefined")), "a start number means Start Chapter Numbering at");
        s.execute("variables.define", &json!({"name": "Chapter", "type": "chapterNumber"})).unwrap();
        let d = &s.doc().unwrap().doc;
        let chapter = d.variable("Chapter").unwrap();
        assert_eq!(d.variable_value(chapter, Some(0)).as_deref(), Some("IV"), "the Chapter Number variable uses the style");
        for bad in [json!({"style": "symbols"}), json!({"start": 0}), json!({"start": 100_000}), json!({"source": "nextBook"})] {
            assert!(s.execute("layout.chapterNumbering", &bad).is_err(), "{bad}");
        }
        s.execute("layout.chapterNumbering", &json!({"source": "sameAsPrevious"})).unwrap();
        s.execute("edit.undo", &json!({})).unwrap();
        assert_eq!(s.execute("layout.chapterNumbering", &json!({})).unwrap()["source"], "userDefined", "undoable");
    }

    #[test]
    fn document_setup_sets_every_pages_margins_in_one_step() {
        let mut s = crate::Session::new();
        s.execute("file.new", &json!({"pages": 3, "margins": 36})).unwrap();
        let f = s.execute("frame.create", &json!({"rect": [36, 36, 576, 756], "content": "text"})).unwrap();
        let r = s.execute("layout.documentSetup", &json!({})).unwrap();
        assert_eq!(r["margins"], json!({"top": 36.0, "bottom": 36.0, "inside": 36.0, "outside": 36.0}));
        let m = json!({"top": 72, "bottom": 54, "inside": 72, "outside": 72});
        let r = s.execute("layout.documentSetup", &json!({"margins": m, "bleed": 9, "adjustLayout": true})).unwrap();
        assert_eq!(r["margins"]["bottom"], 54.0);
        let d = &s.doc().unwrap().doc;
        assert!(
            d.spreads.iter().chain(&d.parents).flat_map(|sp| &sp.pages).all(|pg| pg.margins.top == 72.0 && pg.margins.bottom == 54.0),
            "every page and parent page"
        );
        let b = d.item(designcraft_doc::ItemId(f["id"].as_u64().unwrap())).unwrap().bounds();
        assert!((b.y0 - 72.0).abs() < 1e-6, "Adjust Layout: the frame follows the margins ({b:?})");
        // One step: undo brings back the margins and the bleed.
        s.execute("edit.undo", &json!({})).unwrap();
        let r = s.execute("layout.documentSetup", &json!({})).unwrap();
        assert_eq!((r["margins"]["top"].clone(), r["bleed"][0].clone()), (json!(36.0), json!(0.0)));
        for bad in [
            json!({"width": 0}),
            json!({"height": -5}),
            json!({"width": 1e9}),
            json!({"width": "wide"}),
            json!({"margins": -1}),
            json!({"margins": {"top": 1}}),
        ] {
            assert!(s.execute("layout.documentSetup", &bad).is_err(), "{bad}");
        }
        assert_eq!(s.execute("layout.documentSetup", &json!({})).unwrap()["width"], 612.0, "refused values change nothing");
    }

    #[test]
    fn document_setup_pages_start_bleed_slug() {
        let mut s = crate::Session::new();
        s.execute("file.new", &json!({"pages": 2})).unwrap();
        let r = s.execute("layout.documentSetup", &json!({})).unwrap();
        assert_eq!(r["pages"], 2);
        assert_eq!(r["startPage"], 1);
        let r = s.execute("layout.documentSetup", &json!({"pages": 5, "startPage": 2, "bleed": 9, "slug": [0, 18, 0, 0], "intent": "web"})).unwrap();
        assert_eq!(r["pages"], 5);
        assert_eq!(r["startPage"], 2);
        assert_eq!(r["bleed"], json!([9.0, 9.0, 9.0, 9.0]));
        assert_eq!(r["slug"], json!([0.0, 18.0, 0.0, 0.0]));
        assert_eq!(r["intent"], "web");
        let d = &s.doc().unwrap().doc;
        assert_eq!(d.page_name(0), "2");
        assert_eq!(d.page(0).unwrap().side, designcraft_doc::PageSide::Left);
        assert_eq!(d.spreads[0].pages.len(), 2, "pages 2–3 form the first spread");
        let r = s.execute("layout.documentSetup", &json!({"pages": 3})).unwrap();
        assert_eq!(r["pages"], 3);
        s.doc().unwrap().doc.check().unwrap();
    }

    #[test]
    fn right_to_left_binding_reverses_spreads() {
        use designcraft_doc::PageSide::{Left, Right};
        let mut s = crate::Session::new();
        s.execute("file.new", &json!({"pages": 4})).unwrap();
        // A frame on page 2 stays on page 2.
        let w = s.doc().unwrap().doc.settings.page_width;
        let f = s.execute("frame.create", &json!({"rect": [w + 72.0, 72, w + 200.0, 200], "content": "text", "text": "two"})).unwrap();
        let r = s.execute("layout.documentSetup", &json!({"binding": "rightToLeft"})).unwrap();
        assert_eq!(r["binding"], "rightToLeft");
        let d = &s.doc().unwrap().doc;
        let shape: Vec<Vec<(designcraft_doc::PageSide, f64)>> = d.spreads.iter().map(|sp| sp.pages.iter().map(|p| (p.side, p.x)).collect()).collect();
        // Page 1 alone on the left; pages 2–3 with 2 on the right; page 4 alone on the right.
        assert_eq!(shape, vec![vec![(Left, 0.0)], vec![(Right, w), (Left, 0.0)], vec![(Right, 0.0)]]);
        let (si, _) = d.page_loc(1).unwrap();
        assert_eq!(si, 1);
        let it = d.item(designcraft_doc::ItemId(f["id"].as_u64().unwrap())).unwrap();
        assert_eq!(d.spreads[si].page_at_x(it.bounds().center().x), Some(0), "the frame is still on page 2");
        assert!(it.bounds().x0 >= w, "page 2 is the right page now");
        d.check().unwrap();
        // And back.
        s.execute("layout.documentSetup", &json!({"binding": "leftToRight"})).unwrap();
        let d = &s.doc().unwrap().doc;
        assert_eq!(d.spreads[1].pages.iter().map(|p| (p.side, p.x)).collect::<Vec<_>>(), vec![(Left, 0.0), (Right, w)]);
    }

    /// Bound right to left, a parent spread lists its right page first; each document page still
    /// shows the parent page on its own side, at its own position, also after an IDML round trip.
    #[test]
    fn right_to_left_binding_shows_the_parent_page_of_the_same_side() {
        use designcraft_doc::PageSide::{Left, Right};
        let mut s = crate::Session::new();
        s.execute("file.new", &json!({"pages": 4})).unwrap();
        s.execute("layout.documentSetup", &json!({"binding": "rightToLeft"})).unwrap();
        let w = s.doc().unwrap().doc.settings.page_width;
        let parent = json!({"kind": "parent", "index": 0});
        for (rect, text) in [(json!([72, 36, 200, 60]), "left head"), (json!([w + 72.0, 36, w + 200.0, 60]), "right head")] {
            s.execute("frame.create", &json!({"spread": parent, "rect": rect, "content": "text", "text": text})).unwrap();
        }
        let d = s.doc().unwrap().doc.clone();
        assert_eq!(d.parents[0].pages.iter().map(|p| (p.side, p.x)).collect::<Vec<_>>(), vec![(Right, w), (Left, 0.0)]);
        let docs = [(*d).clone(), designcraft_idml::import_idml(&designcraft_idml::export_idml(&d)).unwrap()];
        for d in docs {
            let mut s = crate::Session::new();
            s.add_document(crate::DocState::new(d, None));
            for (page, side, text) in [(0, Left, "left head"), (1, Right, "right head"), (2, Left, "left head"), (3, Right, "right head")] {
                let r = s.execute("layout.overrideParentItems", &json!({"page": page})).unwrap();
                let d = &s.doc().unwrap().doc;
                let ids = r["ids"].as_array().unwrap();
                assert_eq!(ids.len(), 1, "page {}", page + 1);
                let it = d.item(designcraft_doc::ItemId(ids[0].as_u64().unwrap())).unwrap();
                let story = d.story(it.text_frame().unwrap().story).unwrap();
                assert_eq!((d.page(page).unwrap().side, story.text.trim_end()), (side, text), "page {}", page + 1);
                assert_eq!(d.page_of_item(it.id), Some(page), "page {}", page + 1);
            }
        }
    }

    /// Multi-column frames filled the left column first in a right-to-left document (#100):
    /// stories made in a right-to-left-bound document start their columns on the right.
    #[test]
    fn new_stories_follow_the_binding_direction() {
        use designcraft_doc::TextDirection::{LeftToRight, RightToLeft};
        let text = (1..=12).map(|i| format!("الفقرة {i}")).collect::<Vec<_>>().join("\n");
        for (binding, expected) in [("rightToLeft", RightToLeft), ("leftToRight", LeftToRight)] {
            let mut s = crate::Session::new();
            s.execute("file.new", &json!({"preset": "A4", "pages": 1})).unwrap();
            s.execute("layout.documentSetup", &json!({"binding": binding})).unwrap();
            let f = s.execute("frame.create", &json!({"rect": [40, 170, 555, 260], "content": "text", "text": text})).unwrap();
            s.execute("object.textFrameOptions", &json!({"ids": [f["id"]], "columns": 2, "gutter": 20})).unwrap();
            let sid = designcraft_doc::StoryId(f["story"].as_u64().unwrap());
            let st = s.doc().unwrap();
            assert_eq!(st.doc.story(sid).unwrap().direction, expected, "{binding}");
            let cs = s.cache.get(&st.doc, sid, None);
            let fr = &cs.frames[0];
            let (first, last) = (fr.lines.first().unwrap(), fr.lines.last().unwrap());
            assert!(fr.columns.len() == 2 && first.x0 != last.x0, "the text fills both columns: {:?}", fr.columns);
            assert_eq!(first.x0 > last.x0, expected == RightToLeft, "{binding}: the first line is in the first column");
            // An empty frame given text content starts a story too.
            let g = s.execute("frame.create", &json!({"rect": [40, 400, 555, 500]})).unwrap();
            s.execute("object.content", &json!({"ids": [g["id"]], "type": "text"})).unwrap();
            let d = &s.doc().unwrap().doc;
            let tf = d.item(designcraft_doc::ItemId(g["id"].as_u64().unwrap())).unwrap().text_frame().unwrap().story;
            assert_eq!(d.story(tf).unwrap().direction, expected, "{binding}: object.content");
        }
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
}

#[cfg(test)]
mod page_size_tests {
    use serde_json::json;

    #[test]
    fn page_size_per_page_keeps_objects_on_their_page() {
        let mut s = crate::Session::new();
        s.execute("file.new", &json!({"pages": 3})).unwrap();
        // Page 3 is the right page of spread 2 (pages 2–3); an object on it.
        let d = s.doc().unwrap().doc.clone();
        let (si, pi) = d.page_loc(2).unwrap();
        let x0 = d.spreads[si].pages[pi].x;
        let id = s.execute("frame.create", &json!({"spread": si, "rect": [x0 + 100.0, 100, x0 + 200.0, 200]})).unwrap()["id"].as_u64().unwrap();
        s.execute("layout.pageSize", &json!({"pages": [2], "width": 300, "height": 500})).unwrap();
        let d = s.doc().unwrap().doc.clone();
        let left = &d.spreads[si].pages[0];
        assert_eq!((left.width, left.height), (300.0, 500.0));
        assert_eq!(d.spreads[si].pages[1].x, x0, "the spine stays");
        let it = d.item(designcraft_doc::ItemId(id)).unwrap();
        assert_eq!(it.bounds().x0, x0 + 100.0);
        assert_eq!(d.page_of_item(it.id), Some(2));
        s.execute("layout.pageSize", &json!({"pages": [3], "preset": "A4"})).unwrap();
        let d = s.doc().unwrap().doc.clone();
        assert!((d.spreads[si].pages[1].width - 595.276).abs() < 0.01);
        assert!(s.execute("layout.pageSize", &json!({"pages": [9], "width": 100})).is_err());
    }
}

#[cfg(test)]
mod spread_tests {
    use serde_json::json;

    #[test]
    fn three_page_spread_survives_page_inserts() {
        let mut s = crate::Session::new();
        s.execute("file.new", &json!({"pages": 5})).unwrap();
        // [1] [2 3] [4 5]: page 4 joins spread 1 → [1] [2 3 4] [5].
        let d = s.doc().unwrap().doc.clone();
        let (si, pi) = d.page_loc(3).unwrap();
        let x = d.spreads[si].pages[pi].x;
        let id = s.execute("frame.create", &json!({"spread": si, "rect": [x + 50.0, 50, x + 150.0, 150]})).unwrap()["id"].as_u64().unwrap();
        s.execute("layout.pages.toSpread", &json!({"page": 4, "spread": 1})).unwrap();
        let d = s.doc().unwrap().doc.clone();
        assert_eq!(d.spreads[1].pages.len(), 3);
        assert!(!d.spreads[1].allow_shuffle);
        assert_eq!(d.page_of_item(designcraft_doc::ItemId(id)), Some(3), "the object went with its page");
        // Adding a page at the start reshuffles the others but keeps the 3-page spread.
        s.execute("layout.pages.insert", &json!({"after": 0, "count": 1})).unwrap();
        let d = s.doc().unwrap().doc.clone();
        assert!(d.spreads.iter().any(|sp| sp.pages.len() == 3 && !sp.allow_shuffle));
        assert_eq!(d.page_count(), 6);
        d.check().unwrap();
    }
}

#[cfg(test)]
mod page_numbering_view_tests {
    use serde_json::json;

    use crate::Session;

    #[test]
    fn section_and_absolute_page_numbering_view() {
        let mut s = Session::new();
        s.execute("file.new", &json!({"pages": 4})).unwrap();
        s.execute("layout.section", &json!({"page": 1, "startNumber": 1, "style": "lowerRoman"})).unwrap();
        s.execute("layout.section", &json!({"page": 3, "startNumber": 1})).unwrap();
        assert_eq!((0..4).map(|i| s.page_label(i)).collect::<Vec<_>>(), ["i", "ii", "1", "2"]);
        assert_eq!(s.resolve_page("ii"), Some(1));
        assert_eq!(s.resolve_page("1"), Some(2), "section name first");
        assert_eq!(s.resolve_page("+1"), Some(0), "+n is a position");
        s.execute("prefs.set", &json!({"absolutePageNumbers": true})).unwrap();
        assert_eq!((0..4).map(|i| s.page_label(i)).collect::<Vec<_>>(), ["1", "2", "3", "4"]);
        assert_eq!(s.resolve_page("1"), Some(0));
        assert_eq!(s.resolve_page("9"), None);
    }

    #[test]
    fn section_start_numbers_are_bounded() {
        let mut s = Session::new();
        s.execute("file.new", &json!({"pages": 2})).unwrap();
        for n in [json!(0), json!(100_000), json!(u32::MAX), json!(u64::from(u32::MAX) + 2), json!(-1), json!(2.5), json!("7")] {
            let e = s.execute("layout.section", &json!({"page": 1, "startNumber": n})).unwrap_err().to_string();
            assert!(e.contains("startNumber"), "{n}: {e}");
            assert!(s.execute("layout.documentSetup", &json!({"startPage": n})).is_err(), "{n}");
        }
        assert_eq!((0..2).map(|i| s.page_label(i)).collect::<Vec<_>>(), ["1", "2"]);
        let r = s.execute("layout.section", &json!({"page": 1, "startNumber": 99_999})).unwrap();
        assert_eq!(r["names"], json!(["99999", "100000"]));
    }

    #[test]
    fn numbering_options_apply_in_one_undo_step_or_not_at_all() {
        let mut s = crate::Session::new();
        s.execute("file.new", &json!({"pages": 4})).unwrap();
        let names = |s: &crate::Session| (0..4).map(|i| s.doc().unwrap().doc.page_name(i)).collect::<Vec<_>>();
        let r = s
            .execute(
                "layout.numberingOptions",
                &json!({"page": 3, "startNumber": 5, "style": "lowerRoman", "prefix": "A-", "includePrefix": true, "chapter": {"start": 4, "style": "upperRoman"}}),
            )
            .unwrap();
        assert_eq!((r["names"].clone(), r["chapter"].clone()), (json!(["1", "2", "A-v", "A-vi"]), json!("IV")));
        s.execute("edit.undo", &json!({})).unwrap();
        assert_eq!(names(&s), ["1", "2", "3", "4"], "one undo reverts the section…");
        assert_eq!(s.doc().unwrap().doc.chapter_label(), "1", "…and the chapter numbering");
        // A refused part leaves the other undone too.
        for bad in [
            json!({"page": 3, "startNumber": 5, "chapter": {"start": 100_000}}),
            json!({"page": 3, "startNumber": 4_294_967_295u64, "chapter": {"start": 2}}),
            json!({"page": 3, "prefix": "A+B", "chapter": {"start": 2}}),
            json!({"page": 9, "chapter": {"start": 2}}),
        ] {
            assert!(s.execute("layout.numberingOptions", &bad).is_err(), "{bad}");
            assert_eq!(names(&s), ["1", "2", "3", "4"], "{bad}");
            assert_eq!(s.doc().unwrap().doc.settings.chapter_source, designcraft_doc::ChapterSource::Automatic, "{bad}");
        }
        // Removing a section keeps the chapter part.
        s.execute("layout.section", &json!({"page": 3, "startNumber": 1})).unwrap();
        s.execute("layout.numberingOptions", &json!({"page": 3, "remove": true, "chapter": {"source": "userDefined", "start": 2}})).unwrap();
        assert_eq!(names(&s), ["1", "2", "3", "4"]);
        assert_eq!(s.doc().unwrap().doc.chapter_label(), "2");
    }

    #[test]
    fn section_prefixes_are_short_and_avoid_plus_and_comma() {
        let mut s = crate::Session::new();
        s.execute("file.new", &json!({"pages": 2})).unwrap();
        for prefix in ["A-", "Appendix", "§ 1"] {
            s.execute("layout.section", &json!({"page": 2, "prefix": prefix})).unwrap();
        }
        for prefix in ["Appendix1", "A+", "1,2"] {
            assert!(s.execute("layout.section", &json!({"page": 2, "prefix": prefix})).is_err(), "{prefix}");
        }
        assert!(s.execute("layout.section", &json!({"page": 2, "style": "symbols"})).is_err());
        assert_eq!(s.doc().unwrap().doc.sections.iter().find(|x| x.start == 1).unwrap().prefix, "§ 1", "a refused prefix changes nothing");
    }

    /// `layout.section` edits one named setting; the others stay as the section was (#717).
    #[test]
    fn updating_a_section_keeps_settings_the_command_does_not_name() {
        let mut s = Session::new();
        s.execute("file.new", &json!({"pages": 4})).unwrap();
        s.execute(
            "layout.section",
            &json!({"page": 2, "startNumber": 1, "style": "lowerRoman", "prefix": "A-", "includePrefix": true, "marker": "First"}),
        )
        .unwrap();
        assert_eq!(s.page_label(1), "A-i");
        s.execute("layout.section", &json!({"page": 2, "marker": "Second"})).unwrap();
        let d = s.doc().unwrap().doc.clone();
        let sec = d.sections.iter().find(|x| x.start == 1).expect("the page-2 section is still the only one there");
        assert_eq!(sec.marker, "Second");
        assert_eq!(sec.start_number, Some(1), "startNumber is kept");
        assert_eq!(sec.style, designcraft_doc::NumberStyle::LowerRoman, "style is kept");
        assert_eq!(sec.prefix, "A-", "prefix is kept");
        assert!(sec.include_prefix, "includePrefix is kept");
        assert_eq!(s.page_label(1), "A-i");
    }
}

#[cfg(test)]
mod adjust_tests {
    use designcraft_doc::Margins;
    use serde_json::json;

    use crate::Session;

    #[test]
    fn adjust_layout_follows_the_new_page_size_and_margins() {
        let mut s = Session::new();
        s.execute("file.new", &json!({"pages": 1, "facingPages": false})).unwrap();
        // Letter, 36 pt margins: a frame filling the left half of the margin box.
        let id = s.execute("frame.create", &json!({"rect": [36, 36, 306, 756]})).unwrap()["id"].as_u64().unwrap();
        s.execute("layout.documentSetup", &json!({"width": 1224, "adjustLayout": true})).unwrap();
        let b = s.doc().unwrap().doc.item(designcraft_doc::ItemId(id)).unwrap().bounds();
        assert!((b.x0 - 36.0).abs() < 1e-6 && (b.x1 - 612.0).abs() < 1e-6, "half of the wider margin box: {b:?}");
        s.execute("layout.marginsAndColumns", &json!({"margins": 72, "adjustLayout": true})).unwrap();
        let b = s.doc().unwrap().doc.item(designcraft_doc::ItemId(id)).unwrap().bounds();
        assert!((b.x0 - 72.0).abs() < 1e-6 && (b.y0 - 72.0).abs() < 1e-6 && (b.y1 - 720.0).abs() < 1e-6, "{b:?}");
        // Without the option objects stay put.
        s.execute("layout.marginsAndColumns", &json!({"margins": 36})).unwrap();
        let b2 = s.doc().unwrap().doc.item(designcraft_doc::ItemId(id)).unwrap().bounds();
        assert_eq!(b, b2);
    }

    #[test]
    fn parent_margins_change_the_parent_and_follow_unoverridden_pages() {
        let mut s = Session::new();
        s.execute("file.new", &json!({"pages": 3, "facingPages": false})).unwrap();
        // Page 3 has its own margins (an override); pages 1 and 2 still follow the parent.
        s.execute("layout.marginsAndColumns", &json!({"pages": [2], "margins": 100})).unwrap();
        let parent_before = s.doc().unwrap().doc.parents[0].pages[0].margins;
        assert_eq!(parent_before, Margins::uniform(36.0));
        s.execute("layout.marginsAndColumns", &json!({"parent": 0, "margins": 60, "columns": 2})).unwrap();
        let d = &s.doc().unwrap().doc;
        // The parent page itself changed (this is what was drawn on screen).
        assert_eq!(d.parents[0].pages[0].margins, Margins::uniform(60.0));
        assert_eq!(d.parents[0].pages[0].columns.count, 2);
        // Pages without their own margins follow; the page given its own keeps them.
        assert_eq!(d.page(0).unwrap().margins, Margins::uniform(60.0));
        assert_eq!(d.page(1).unwrap().margins, Margins::uniform(60.0));
        assert_eq!(d.page(2).unwrap().margins, Margins::uniform(100.0));
        // An unknown parent is an error, not a silent no-op.
        assert!(s.execute("layout.marginsAndColumns", &json!({"parent": 9, "margins": 10})).is_err());
    }

    /// Margins and Columns re-divides the columns: a new count or gutter replaces column guides
    /// that were dragged (an imported Custom columns page), rather than being ignored by them.
    #[test]
    fn a_new_column_count_replaces_dragged_column_positions() {
        let mut s = Session::new();
        s.execute("file.new", &json!({"pages": 1, "facingPages": false})).unwrap();
        // There is no command for dragging a column guide yet, so set what an import would leave.
        let drag = |s: &mut Session| {
            s.edit(|d, _| {
                let pg = &mut std::sync::Arc::make_mut(&mut d.spreads[0]).pages[0];
                pg.columns = designcraft_doc::Columns { count: 2, gutter: 12.0, positions: Some(vec![0.0, 100.0, 130.0, 540.0]) };
                Ok(())
            })
            .unwrap();
        };

        drag(&mut s);
        let dragged = s.doc().unwrap().doc.page(0).unwrap().column_rects();
        assert_eq!(dragged.len(), 2);
        assert!((dragged[0].x1 - dragged[0].x0 - 100.0).abs() < 1e-6, "the dragged gutter is in use: {dragged:?}");

        // A new count: evenly divided again, and the stale positions are gone.
        s.execute("layout.marginsAndColumns", &json!({"columns": 3})).unwrap();
        let pg = s.doc().unwrap().doc.page(0).unwrap().clone();
        assert_eq!((pg.columns.count, pg.columns.positions.clone()), (3, None));
        assert_eq!(pg.column_rects().len(), 3);

        // So does a new gutter on its own.
        drag(&mut s);
        s.execute("layout.marginsAndColumns", &json!({"gutter": 24})).unwrap();
        let pg = s.doc().unwrap().doc.page(0).unwrap().clone();
        assert_eq!(pg.columns.positions, None);
        assert!((pg.column_rects()[1].x0 - pg.column_rects()[0].x1 - 24.0).abs() < 1e-6);

        // Margins that keep the margin box's width leave them alone; a new width scales them.
        drag(&mut s);
        s.execute("layout.marginsAndColumns", &json!({"margins": {"top": 72, "bottom": 72, "inside": 36, "outside": 36}})).unwrap();
        assert_eq!(s.doc().unwrap().doc.page(0).unwrap().columns.positions, Some(vec![0.0, 100.0, 130.0, 540.0]));
        s.execute("layout.marginsAndColumns", &json!({"margins": 72})).unwrap();
        assert_scaled(s.doc().unwrap().doc.page(0).unwrap().columns.positions.clone(), 468.0);
    }

    /// The params the Margins and Columns dialog sends on OK: every margin, the count and the
    /// gutter, whether or not the user changed them.
    fn dialog_params(margin: f64, columns: u32, gutter: f64) -> serde_json::Value {
        json!({"margins": {"top": margin, "bottom": margin, "inside": margin, "outside": margin},
            "columns": columns, "gutter": gutter, "adjustLayout": false})
    }

    const DRAGGED: [f64; 4] = [0.0, 100.0, 130.0, 540.0];

    fn dragged() -> designcraft_doc::Columns {
        designcraft_doc::Columns { count: 2, gutter: 12.0, positions: Some(DRAGGED.to_vec()) }
    }

    /// `positions` are [`DRAGGED`] scaled from its 540 pt margin box to one `width` wide.
    #[track_caller]
    fn assert_scaled(positions: Option<Vec<f64>>, width: f64) {
        let want: Vec<f64> = DRAGGED.iter().map(|p| p * width / 540.0).collect();
        let got = positions.unwrap_or_default();
        assert!(got.len() == want.len() && got.iter().zip(&want).all(|(g, w)| (g - w).abs() < 1e-6), "{got:?} ≠ {want:?}");
    }

    /// An OK in the dialog keeps dragged columns unless the count or gutter it sends differs.
    #[test]
    fn the_dialog_keeps_dragged_columns_unless_the_count_or_gutter_changes() {
        let mut s = Session::new();
        s.execute("file.new", &json!({"pages": 1, "facingPages": false})).unwrap();
        let drag = |s: &mut Session| {
            s.edit(|d, _| {
                std::sync::Arc::make_mut(&mut d.spreads[0]).pages[0].columns = dragged();
                Ok(())
            })
            .unwrap();
        };
        let positions = |s: &Session| s.doc().unwrap().doc.page(0).unwrap().columns.positions.clone();

        // The same count and gutter (the gutter as an inch field rounds it) keep them in use.
        drag(&mut s);
        s.execute("layout.marginsAndColumns", &dialog_params(36.0, 2, 12.0)).unwrap();
        assert_eq!(positions(&s), Some(DRAGGED.to_vec()));
        s.execute("layout.marginsAndColumns", &dialog_params(36.0, 2, 0.1667 * 72.0)).unwrap();
        assert_eq!(positions(&s), Some(DRAGGED.to_vec()));
        let rects = s.doc().unwrap().doc.page(0).unwrap().column_rects();
        assert!((rects[0].x1 - rects[0].x0 - 100.0).abs() < 1e-6, "the dragged columns are in use: {rects:?}");

        // Top and bottom margins leave the margin box's width alone, and the columns with it.
        let mut taller = dialog_params(36.0, 2, 12.0);
        taller["margins"]["top"] = json!(72.0);
        taller["margins"]["bottom"] = json!(72.0);
        s.execute("layout.marginsAndColumns", &taller).unwrap();
        assert_eq!(positions(&s), Some(DRAGGED.to_vec()));

        // A new width scales them with the margin box, and the old width scales them back.
        s.execute("layout.marginsAndColumns", &dialog_params(72.0, 2, 12.0)).unwrap();
        assert_scaled(positions(&s), 468.0);
        let rects = s.doc().unwrap().doc.page(0).unwrap().column_rects();
        assert!((rects[0].x1 - rects[0].x0 - 100.0 * 468.0 / 540.0).abs() < 1e-6, "the scaled columns are in use: {rects:?}");
        s.execute("layout.marginsAndColumns", &dialog_params(36.0, 2, 12.0)).unwrap();
        assert_scaled(positions(&s), 540.0);

        // A new count resets them.
        drag(&mut s);
        s.execute("layout.marginsAndColumns", &dialog_params(36.0, 3, 12.0)).unwrap();
        let pg = s.doc().unwrap().doc.page(0).unwrap().clone();
        assert_eq!((pg.columns.count, pg.columns.positions), (3, None));

        // So does a new gutter.
        drag(&mut s);
        s.execute("layout.marginsAndColumns", &dialog_params(36.0, 2, 18.0)).unwrap();
        let pg = s.doc().unwrap().doc.page(0).unwrap().clone();
        assert_eq!((pg.columns.gutter, pg.columns.positions), (18.0, None));
    }

    /// A page of another width (Page Size, Document Setup) scales its dragged columns.
    #[test]
    fn a_new_page_width_scales_dragged_columns() {
        let mut s = Session::new();
        s.execute("file.new", &json!({"pages": 1, "facingPages": false})).unwrap();
        let drag = |s: &mut Session| {
            s.edit(|d, _| {
                // A dragged page's columns are its own (`local_margins`), as the drag will make them.
                let pg = &mut std::sync::Arc::make_mut(&mut d.spreads[0]).pages[0];
                pg.columns = dragged();
                pg.local_margins = true;
                Ok(())
            })
            .unwrap();
        };
        let positions = |s: &Session| s.doc().unwrap().doc.page(0).unwrap().columns.positions.clone();

        drag(&mut s);
        s.execute("layout.pageSize", &json!({"pages": [1], "height": 900})).unwrap();
        assert_eq!(positions(&s), Some(DRAGGED.to_vec()), "a new height keeps them");
        s.execute("layout.pageSize", &json!({"pages": [1], "width": 700})).unwrap();
        assert_scaled(positions(&s), 628.0);
        s.execute("layout.pageSize", &json!({"pages": [1], "width": 612})).unwrap();
        assert_scaled(positions(&s), 540.0);

        drag(&mut s);
        s.execute("layout.documentSetup", &json!({"width": 700})).unwrap();
        assert_scaled(positions(&s), 628.0);
    }

    /// Parent pages follow the same rule, and the document pages that follow them see it.
    #[test]
    fn the_dialog_keeps_a_parents_dragged_columns_unless_the_count_or_gutter_changes() {
        let mut s = Session::new();
        s.execute("file.new", &json!({"pages": 1, "facingPages": false})).unwrap();
        let drag = |s: &mut Session| {
            s.edit(|d, _| {
                std::sync::Arc::make_mut(&mut d.parents[0]).pages[0].columns = dragged();
                d.sync_parent_margins();
                Ok(())
            })
            .unwrap();
        };
        let positions = |s: &Session| {
            let d = &s.doc().unwrap().doc;
            (d.parents[0].pages[0].columns.positions.clone(), d.page(0).unwrap().columns.positions.clone())
        };
        let on_parent = |m: f64, c: u32, g: f64| {
            let mut p = dialog_params(m, c, g);
            p["parent"] = json!(0);
            p
        };

        drag(&mut s);
        assert_eq!(positions(&s), (Some(DRAGGED.to_vec()), Some(DRAGGED.to_vec())));
        s.execute("layout.marginsAndColumns", &on_parent(36.0, 2, 12.0)).unwrap();
        assert_eq!(positions(&s), (Some(DRAGGED.to_vec()), Some(DRAGGED.to_vec())));
        s.execute("layout.marginsAndColumns", &on_parent(48.0, 2, 12.0)).unwrap();
        let (parent, follower) = positions(&s);
        assert_scaled(parent, 516.0);
        assert_scaled(follower, 516.0);

        drag(&mut s);
        s.execute("layout.marginsAndColumns", &on_parent(36.0, 3, 12.0)).unwrap();
        assert_eq!(positions(&s), (None, None));

        drag(&mut s);
        s.execute("layout.marginsAndColumns", &on_parent(36.0, 2, 24.0)).unwrap();
        assert_eq!(positions(&s), (None, None));
    }
}

#[cfg(test)]
mod section_side_tests {
    use serde_json::json;

    use crate::Session;

    /// Each spread's pages as (name, side), side `L`, `R` or `S`.
    fn spreads(s: &Session) -> Vec<Vec<(String, char)>> {
        let d = &s.doc().unwrap().doc;
        let mut abs = 0;
        d.spreads
            .iter()
            .map(|sp| {
                sp.pages
                    .iter()
                    .map(|p| {
                        let side = match p.side {
                            designcraft_doc::PageSide::Left => 'L',
                            designcraft_doc::PageSide::Right => 'R',
                            designcraft_doc::PageSide::Single => 'S',
                        };
                        abs += 1;
                        (d.page_name(abs - 1), side)
                    })
                    .collect()
            })
            .collect()
    }

    fn pages(v: &[&[(&str, char)]]) -> Vec<Vec<(String, char)>> {
        v.iter().map(|sp| sp.iter().map(|(n, c)| (n.to_string(), *c)).collect()).collect()
    }

    #[test]
    fn a_section_starting_at_one_on_a_left_page_moves_it_to_the_right() {
        // Chris Nielsen's "Numbering Page Sections" (7:03): with the spread 8–9 selected, the
        // section starts on page 8, which becomes a lone right-hand page 1; the pages after it
        // shuffle and their objects go with them. Undo puts them back.
        let mut s = Session::new();
        s.execute("file.new", &json!({"width": 612, "height": 792, "facingPages": true, "pages": 10})).unwrap();
        let id = s.execute("frame.create", &json!({"spread": 4, "rect": [700, 100, 800, 200], "content": "text", "text": "TOPIC"})).unwrap()["id"]
            .as_u64()
            .unwrap();
        let page_of = |s: &Session| {
            let d = &s.doc().unwrap().doc;
            d.page_of_item(designcraft_doc::ItemId(id)).unwrap()
        };
        assert_eq!(page_of(&s), 8, "page 9, the right page of the spread 8–9");
        let before = spreads(&s);
        s.execute("layout.section", &json!({"page": 8, "startNumber": 1})).unwrap();
        assert_eq!(
            spreads(&s),
            pages(&[
                &[("1", 'R')],
                &[("2", 'L'), ("3", 'R')],
                &[("4", 'L'), ("5", 'R')],
                &[("6", 'L'), ("7", 'R')],
                &[("1", 'R')],
                &[("2", 'L'), ("3", 'R')],
            ])
        );
        assert_eq!(page_of(&s), 8, "the object stays on its page, now the left page of a spread");
        s.execute("edit.undo", &json!({})).unwrap();
        assert_eq!(spreads(&s), before);
        // On a right page (the video's page 9) nothing moves; an even start on a right page moves
        // it to the left.
        s.execute("layout.section", &json!({"page": 9, "startNumber": 1})).unwrap();
        assert_eq!(spreads(&s)[4], pages(&[&[("8", 'L'), ("1", 'R')]])[0]);
        s.execute("layout.section", &json!({"page": 9, "startNumber": 2})).unwrap();
        assert_eq!(spreads(&s)[4..], pages(&[&[("8", 'L')], &[("2", 'L'), ("3", 'R')]]));
        // Removing the section numbers the pages on from 9 again.
        s.execute("layout.section", &json!({"page": 9, "remove": true})).unwrap();
        assert_eq!(spreads(&s), before);
    }

    #[test]
    fn single_sided_documents_keep_their_pages() {
        let mut s = Session::new();
        s.execute("file.new", &json!({"facingPages": false, "pages": 4})).unwrap();
        s.execute("layout.section", &json!({"page": 2, "startNumber": 1})).unwrap();
        let sp = spreads(&s);
        assert_eq!(sp.len(), 4);
        assert!(sp.iter().all(|p| p.len() == 1 && p[0].1 == 'S'), "{sp:?}");
    }
}

#[cfg(test)]
mod spread_rotation_input_tests {
    use serde_json::json;

    #[test]
    fn quarter_turns_work_and_other_angles_are_rejected_without_mutation() {
        let mut s = crate::Session::new();
        s.execute("file.new", &json!({})).unwrap();
        let rotation = |s: &crate::Session| s.doc().unwrap().doc.spreads[0].pages[0].view_rotation;
        for bad in [json!(45), json!(-45), json!(91), json!("90"), json!(0.5)] {
            assert!(s.execute("layout.rotateSpreadView", &json!({"angle": bad})).is_err(), "{bad}");
            assert_eq!(rotation(&s), 0, "invalid input must not change the view");
        }
        s.execute("layout.rotateSpreadView", &json!({"angle": 90})).unwrap();
        assert_eq!(rotation(&s), 1);
        s.execute("layout.rotateSpreadView", &json!({"angle": -90})).unwrap();
        assert_eq!(rotation(&s), 0);
        s.execute("layout.rotateSpreadView", &json!({"angle": 180})).unwrap();
        assert_eq!(rotation(&s), 2);
        s.execute("layout.rotateSpreadView", &json!({"angle": 0})).unwrap();
        assert_eq!(rotation(&s), 0, "zero resets the spread view");
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

#[cfg(test)]
mod parent_margin_tests {
    use designcraft_doc::{Document, Margins};
    use serde_json::json;

    use crate::Session;

    fn margins(d: &Document) -> Vec<(Margins, u32, bool)> {
        (0..d.page_count()).filter_map(|i| d.page(i)).map(|p| (p.margins, p.columns.count, p.local_margins)).collect()
    }

    #[test]
    fn document_setup_margins_let_pages_follow_their_parent_page_again() {
        let mut s = Session::new();
        s.execute("file.new", &json!({"pages": 3})).unwrap();
        s.execute("layout.marginsAndColumns", &json!({"pages": [1], "margins": 100})).unwrap();
        s.execute("layout.marginsAndColumns", &json!({"pages": [2], "margins": 100, "columns": 3})).unwrap();
        s.execute("layout.documentSetup", &json!({"margins": 20})).unwrap();
        let m = margins(&s.doc().unwrap().doc);
        let twenty = Margins::uniform(20.0);
        assert_eq!(m, vec![(twenty, 1, false), (twenty, 1, false), (twenty, 3, true)], "own columns stay local");
        s.execute("layout.marginsAndColumns", &json!({"parent": "A", "margins": 50})).unwrap();
        let m = margins(&s.doc().unwrap().doc);
        assert_eq!(m[1], (Margins::uniform(50.0), 1, false), "page 2 follows the parent page again");
        assert_eq!(m[2], (twenty, 3, true));
    }

    #[test]
    fn pages_follow_their_parent_pages_margins_unless_set_locally() {
        let mut s = Session::new();
        // Facing: [1] [2 3]; page 3 gets its own margins.
        s.execute("file.new", &json!({"pages": 3})).unwrap();
        s.execute("layout.marginsAndColumns", &json!({"pages": [2], "margins": 100})).unwrap();
        // A's left page: only the left document page follows.
        let left = json!({"top": 10, "bottom": 20, "inside": 30, "outside": 40});
        s.execute("layout.marginsAndColumns", &json!({"parent": "A", "pages": [0], "margins": left, "columns": 2})).unwrap();
        let l = Margins { top: 10.0, bottom: 20.0, inside: 30.0, outside: 40.0 };
        let m = margins(&s.doc().unwrap().doc);
        assert_eq!(m, vec![(Margins::uniform(36.0), 1, false), (l, 2, false), (Margins::uniform(100.0), 1, true)]);
        // A's right page: page 1 follows, page 3 keeps its own.
        s.execute("layout.marginsAndColumns", &json!({"parent": "A-Parent", "pages": [1], "margins": 50})).unwrap();
        let m = margins(&s.doc().unwrap().doc);
        assert_eq!(m, vec![(Margins::uniform(50.0), 1, false), (l, 2, false), (Margins::uniform(100.0), 1, true)]);
        // A page inserted after page 1 pushes page 2 to the right: it takes the right parent page's.
        s.execute("layout.pages.insert", &json!({"after": 0, "count": 1, "parent": "A"})).unwrap();
        let m = margins(&s.doc().unwrap().doc);
        assert_eq!(m[1], (l, 2, false), "the new left page");
        assert_eq!(m[2], (Margins::uniform(50.0), 1, false), "old page 2, now a right page");
        assert_eq!(m[3], (Margins::uniform(100.0), 1, true));
        assert!(s.execute("layout.marginsAndColumns", &json!({"parent": "Z", "margins": 10})).is_err());
    }

    #[test]
    fn applying_a_parent_gives_its_margins_and_local_margins_stay_local() {
        let mut s = Session::new();
        // Facing: [1] [2 3].
        s.execute("file.new", &json!({"pages": 3})).unwrap();
        s.execute("layout.parents.new", &json!({"prefix": "B"})).unwrap();
        s.execute("layout.marginsAndColumns", &json!({"parent": "B", "margins": 70, "columns": 3})).unwrap();
        s.execute("layout.pages.applyParent", &json!({"pages": [1], "parent": "B"})).unwrap();
        let m = margins(&s.doc().unwrap().doc);
        assert_eq!(m[1], (Margins::uniform(70.0), 3, false), "the page takes B's margins");
        // Page 3 is given margins equal to A's: they are its own all the same.
        s.execute("layout.marginsAndColumns", &json!({"pages": [2], "margins": 36})).unwrap();
        s.execute("layout.marginsAndColumns", &json!({"parent": "A", "margins": 20})).unwrap();
        let m = margins(&s.doc().unwrap().doc);
        assert_eq!(m[0], (Margins::uniform(20.0), 1, false));
        assert_eq!(m[1], (Margins::uniform(70.0), 3, false), "B's page is not A's");
        assert_eq!(m[2], (Margins::uniform(36.0), 1, true));
        // Back to A: page 2 follows A again.
        s.execute("layout.pages.applyParent", &json!({"pages": [1], "parent": "A"})).unwrap();
        assert_eq!(margins(&s.doc().unwrap().doc)[1], (Margins::uniform(20.0), 1, false));
    }

    #[test]
    fn parent_guides_show_on_pages_but_stay_on_the_parent() {
        let mut s = Session::new();
        s.execute("file.new", &json!({"pages": 2})).unwrap();
        // A vertical guide 100 pt into A's right page (x = 612 in the parent spread).
        s.execute("guide.add", &json!({"spread": {"kind": "parent", "index": 0}, "page": 1, "orientation": "vertical", "position": 712})).unwrap();
        let d = &s.doc().unwrap().doc;
        let right = d.page(0).unwrap();
        let shown: Vec<f64> = d.parent_guides(right).map(|g| g.position).collect();
        assert_eq!(shown, vec![100.0], "moved to the document page's place");
        assert_eq!(d.parent_guides(d.page(1).unwrap()).count(), 0, "the left page shows A's left page's guides");
        // Not the document page's own: not listed, moved or deleted from there.
        assert_eq!(s.execute("guide.list", &json!({"spread": 0})).unwrap(), json!([]));
        assert!(s.execute("guide.move", &json!({"spread": 0, "page": 0, "index": 0, "position": 50})).is_err());
        assert!(s.execute("guide.delete", &json!({"spread": 0, "page": 0, "index": 0})).is_err());
    }
}

#[cfg(test)]
mod parent_tests {
    use serde_json::json;

    fn parent_of(s: &crate::Session, page: usize) -> Option<u64> {
        s.doc().unwrap().doc.page(page).unwrap().parent.map(|p| p.0)
    }

    #[test]
    fn parents_with_one_prefix_stay_distinct() {
        let mut s = crate::Session::new();
        s.execute("file.new", &json!({"pages": 2})).unwrap();
        let a = s.execute("layout.parents.new", &json!({"prefix": "IMG", "name": "Parent"})).unwrap()["id"].as_u64().unwrap();
        let b = s.execute("layout.parents.new", &json!({"prefix": "IMG", "name": "Half"})).unwrap()["id"].as_u64().unwrap();
        s.execute("layout.pages.applyParent", &json!({"pages": [0], "parent": b})).unwrap();
        assert_eq!(parent_of(&s, 0), Some(b));
        s.execute("layout.pages.applyParent", &json!({"pages": [1], "parent": "IMG-Half"})).unwrap();
        assert_eq!(parent_of(&s, 1), Some(b));
        s.execute("layout.pages.applyParent", &json!({"pages": [1], "parent": "IMG-Parent"})).unwrap();
        assert_eq!(parent_of(&s, 1), Some(a));
        // A prefix two parents share, or a name no parent has, is an error and changes nothing.
        assert!(s.execute("layout.pages.applyParent", &json!({"pages": [1], "parent": "IMG"})).is_err());
        assert!(s.execute("layout.pages.applyParent", &json!({"pages": [1], "parent": "Nope"})).is_err());
        assert!(s.execute("layout.pages.applyParent", &json!({"pages": [1], "parent": 999_999})).is_err());
        assert_eq!(parent_of(&s, 1), Some(a));
        s.execute("layout.pages.applyParent", &json!({"pages": [1], "parent": "[None]"})).unwrap();
        assert_eq!(parent_of(&s, 1), None);
    }

    #[test]
    fn edit_parents_shows_the_named_parent() {
        let mut s = crate::Session::new();
        s.execute("file.new", &json!({})).unwrap();
        let ids: Vec<u64> =
            ["B", "C", "D"].iter().map(|p| s.execute("layout.parents.new", &json!({"prefix": p})).unwrap()["id"].as_u64().unwrap()).collect();
        s.ui_requests.clear();
        let r = s.execute("layout.parents.edit", &json!({"parent": ids[2]})).unwrap();
        assert_eq!(r, json!({"on": true, "parent": ids[2]}));
        assert!(s.doc().unwrap().editing_parents);
        let n = s.doc().unwrap().doc.parent_index(designcraft_doc::SpreadId(ids[2])).unwrap();
        let shown: Vec<_> = s
            .ui_requests
            .iter()
            .filter_map(|r| match r {
                crate::UiRequest::View { params } => params.get("showSpread").cloned(),
                _ => None,
            })
            .collect();
        assert_eq!(shown, vec![json!({"kind": "parent", "index": n})]);
        let r = s.execute("layout.parents.edit", &json!({"on": true, "parent": "C-Parent"})).unwrap();
        assert_eq!(r["parent"], ids[1]);
        assert!(s.execute("layout.parents.edit", &json!({"parent": "Z-Parent"})).is_err());
        let r = s.execute("layout.parents.edit", &json!({"on": false})).unwrap();
        assert_eq!(r, json!({"on": false, "parent": null}));
        assert!(!s.doc().unwrap().editing_parents);
    }

    /// A new parent takes the first page's columns, dragged column guides included: the right
    /// parent page as the first (right) page has them, the left one mirrored across the spine.
    /// `document.inspect` reports where those columns are.
    #[test]
    fn a_new_parent_keeps_dragged_columns() {
        let mut s = crate::Session::new();
        s.execute("file.new", &json!({"pages": 1})).unwrap();
        let dragged = designcraft_doc::Columns { count: 2, gutter: 12.0, positions: Some(vec![0.0, 100.0, 130.0, 540.0]) };
        // There is no command for dragging a column guide yet, so set what an import would leave.
        s.edit(|d, _| {
            let pg = &mut std::sync::Arc::make_mut(&mut d.spreads[0]).pages[0];
            (pg.columns, pg.local_margins) = (dragged.clone(), true);
            Ok(())
        })
        .unwrap();
        let id = s.execute("layout.parents.new", &json!({"prefix": "B"})).unwrap()["id"].as_u64().unwrap();
        let d = &s.doc().unwrap().doc;
        let parent = d.parents.iter().find(|p| p.id.0 == id).unwrap();
        assert_eq!(parent.pages.len(), 2);
        let side = |s: designcraft_doc::PageSide| parent.pages.iter().find(|p| p.side == s).unwrap();
        let (left, right) = (side(designcraft_doc::PageSide::Left), side(designcraft_doc::PageSide::Right));
        assert_eq!(right.columns, dragged);
        assert_eq!(right.column_rects().iter().map(|r| r.width()).collect::<Vec<_>>(), vec![100.0, 410.0]);
        assert_eq!(left.columns.positions, Some(vec![0.0, 410.0, 440.0, 540.0]));
        assert_eq!(left.column_rects().iter().map(|r| r.width()).collect::<Vec<_>>(), vec![410.0, 100.0]);

        let inspect = s.execute("document.inspect", &json!({})).unwrap();
        let page = &inspect["spreads"][0]["pages"][0];
        assert_eq!((page["columns"].clone(), page["customColumns"].clone()), (json!(2), json!(true)), "{page}");
        assert_eq!(page["columnRects"][0], json!([36.0, 36.0, 136.0, 756.0]), "{page}");
    }

    fn columns_of(s: &crate::Session, abs: usize) -> (Option<Vec<f64>>, bool) {
        let pg = s.doc().unwrap().doc.page(abs).unwrap().clone();
        (pg.columns.positions, pg.local_margins)
    }

    /// Three columns across a 540 pt margin box (36 pt margins), column guides unlocked.
    fn three_columns(pages: u64) -> crate::Session {
        let mut s = crate::Session::new();
        s.execute("file.new", &json!({"pages": pages})).unwrap();
        s.execute("layout.marginsAndColumns", &json!({"parent": "A", "columns": 3, "gutter": 12})).unwrap();
        s.execute("view.lockColumnGuides", &json!({"on": false})).unwrap();
        s
    }

    /// Dragging a column guide moves its gutter's two guides together, stops at the neighbouring
    /// gutter and the margin, makes a following page's columns its own, and undoes in one step.
    #[test]
    fn move_column_guide_moves_the_gutter_pair_within_its_neighbours() {
        let mut s = three_columns(1);
        assert_eq!(columns_of(&s, 0), (None, false), "the page follows A");
        let mv =
            |s: &mut crate::Session, gutter: u64, x: f64| s.execute("layout.moveColumnGuide", &json!({"page": 0, "gutter": gutter, "position": x}));
        let r = mv(&mut s, 0, 236.0).unwrap();
        assert_eq!(r, json!({"positions": [0.0, 200.0, 212.0, 356.0, 368.0, 540.0], "left": 236.0}));
        assert_eq!(columns_of(&s, 0), (Some(vec![0.0, 200.0, 212.0, 356.0, 368.0, 540.0]), true));
        let inspect = s.execute("document.inspect", &json!({})).unwrap();
        assert_eq!(inspect["spreads"][0]["pages"][0]["columnRects"][0], json!([36.0, 36.0, 236.0, 756.0]));
        // Past the next gutter: it stops there, and the middle column has no width.
        assert_eq!(mv(&mut s, 0, 1000.0).unwrap()["left"], json!(36.0 + 344.0));
        // Past the right margin: the last gutter stops 12 pt (its width) inside it.
        assert_eq!(mv(&mut s, 1, 1e12).unwrap()["positions"], json!([0.0, 344.0, 356.0, 528.0, 540.0, 540.0]));
        // Past the left margin.
        assert_eq!(mv(&mut s, 0, -1e12).unwrap()["positions"], json!([0.0, 0.0, 12.0, 528.0, 540.0, 540.0]));
        for _ in 0..3 {
            s.execute("edit.undo", &json!({})).unwrap();
        }
        assert_eq!(columns_of(&s, 0), (Some(vec![0.0, 200.0, 212.0, 356.0, 368.0, 540.0]), true));
        s.execute("edit.undo", &json!({})).unwrap();
        assert_eq!(columns_of(&s, 0), (None, false), "one undo step per drag");
    }

    /// On a parent page the drag changes the parent, and the document pages that follow it get
    /// the new columns; a page with columns of its own keeps them.
    #[test]
    fn move_column_guide_on_a_parent_reaches_the_pages_that_follow_it() {
        let mut s = three_columns(3);
        // Page 1, the left page of spread 1, gets columns of its own.
        let x = s.doc().unwrap().doc.spreads[1].pages[0].margin_rect().x0 + 300.0;
        s.execute("layout.moveColumnGuide", &json!({"spread": 1, "page": 0, "gutter": 1, "position": x})).unwrap();
        let own = columns_of(&s, 1);
        assert!(own.1 && own.0.is_some());
        let d = &s.doc().unwrap().doc;
        // A's right page: the one page 0 (a right page) and page 2 follow.
        let x0 = d.parents[0].pages[1].margin_rect().x0;
        s.execute("layout.moveColumnGuide", &json!({"parent": "A", "page": 1, "gutter": 0, "position": x0 + 100.0})).unwrap();
        let moved = vec![0.0, 100.0, 112.0, 356.0, 368.0, 540.0];
        assert_eq!(s.doc().unwrap().doc.parents[0].pages[1].columns.positions, Some(moved.clone()));
        assert_eq!(columns_of(&s, 0), (Some(moved.clone()), false));
        assert_eq!(columns_of(&s, 2), (Some(moved), false));
        assert_eq!(columns_of(&s, 1), own, "page 1's columns are its own");
        // The same parent page by spread reference.
        let spread = json!({"kind": "parent", "index": 0});
        s.execute("layout.moveColumnGuide", &json!({"spread": spread, "page": 1, "gutter": 0, "position": x0 + 50.0})).unwrap();
        assert_eq!(columns_of(&s, 0).0.unwrap()[1], 50.0);
    }

    /// A drag's preview in one document is put back there when another document became active.
    #[test]
    fn a_column_guide_preview_is_cancelled_in_its_own_document() {
        let mut s = three_columns(1);
        let uid = s.doc().unwrap().uid;
        s.begin_interaction("Move Column Guide").unwrap();
        s.preview("layout.moveColumnGuide", json!({"page": 0, "gutter": 0, "position": 236})).unwrap();
        s.execute("file.new", &json!({})).unwrap();
        s.cancel_interaction_of(uid);
        s.set_active(0);
        assert!(s.doc().unwrap().interaction.is_none());
        assert_eq!(columns_of(&s, 0), (None, false));
    }

    #[test]
    fn move_column_guide_refuses_while_locked_and_hostile_params() {
        let mut s = three_columns(1);
        s.execute("view.lockColumnGuides", &json!({"on": true})).unwrap();
        assert!(s.execute("layout.moveColumnGuide", &json!({"page": 0, "gutter": 0, "position": 200})).is_err());
        assert_eq!(columns_of(&s, 0), (None, false));
        s.execute("view.lockColumnGuides", &json!({"on": false})).unwrap();
        for p in [
            json!({"page": 0, "gutter": 0}),
            json!({"page": 0, "gutter": 0, "position": f64::NAN}),
            json!({"page": 0, "gutter": 0, "position": "200"}),
            json!({"page": 0, "gutter": 2, "position": 200}),
            json!({"page": 0, "gutter": u64::MAX, "position": 200}),
            json!({"page": 0, "gutter": -1, "position": 200}),
            json!({"page": 0, "gutter": 0.5, "position": 200}),
            json!({"page": 9, "gutter": 0, "position": 200}),
            json!({"page": u64::MAX, "gutter": 0, "position": 200}),
            json!({"gutter": 0, "position": 200}),
            json!({"spread": 99, "page": 0, "gutter": 0, "position": 200}),
            json!({"parent": "Z", "page": 0, "gutter": 0, "position": 200}),
            json!({"page": 0, "gutter": 0, "position": f64::MAX}),
        ] {
            let r = s.execute("layout.moveColumnGuide", &p);
            // A huge but finite position is clamped to the margin; everything else is refused.
            assert!(r.is_err() || p["position"] == json!(f64::MAX), "{p}: {r:?}");
        }
    }
}

#[cfg(test)]
mod parent_base_tests {
    use serde_json::{Value, json};

    fn rect(s: &mut crate::Session, spread: Value, r: [f64; 4], swatch: &str) -> u64 {
        let id = s.execute("frame.create", &json!({"spread": spread, "rect": r, "content": "unassigned"})).unwrap()["id"].as_u64().unwrap();
        s.execute("object.fill", &json!({"ids": [id], "swatch": swatch})).unwrap();
        id
    }

    fn page(s: &crate::Session, abs: usize) -> designcraft_render::Rendered {
        let mut rr = designcraft_render::Renderer::new();
        rr.threads = 0;
        rr.render_page(&s.doc().unwrap().doc, &s.cache, abs, 1.0, false, &Default::default()).unwrap()
    }

    /// Parent A (black), parent B based on A (cyan, half over A's square), and a page item (paper)
    /// over B's: the page draws A, then B, then its own items.
    #[test]
    fn based_on_parent_draws_base_items_under_parent_items() {
        let mut s = crate::Session::new();
        s.execute("file.new", &json!({"width": 400, "height": 400, "pages": 2, "facingPages": false, "margins": 0})).unwrap();
        s.execute("layout.parents.new", &json!({"prefix": "B", "name": "Opening"})).unwrap();
        rect(&mut s, json!({"kind": "parent", "index": 0}), [0.0, 0.0, 200.0, 200.0], "[Black]");
        rect(&mut s, json!({"kind": "parent", "index": 1}), [100.0, 0.0, 300.0, 200.0], "C=100 M=0 Y=0 K=0");
        rect(&mut s, json!(1), [150.0, 0.0, 190.0, 40.0], "[Paper]");
        s.execute("layout.pages.applyParent", &json!({"pages": [1], "parent": "B"})).unwrap();
        let black = |p: [u8; 4]| p[0] < 80 && p[1] < 80 && p[2] < 80;
        let cyan = |p: [u8; 4]| p[0] < 80 && p[1] > 120 && p[2] > 180;
        let paper = |p: [u8; 4]| p[0] > 240 && p[1] > 240 && p[2] > 240;
        assert!(paper(page(&s, 1).pixel(50, 100)), "B alone does not show A's items");

        let r = s.execute("layout.parents.options", &json!({"parent": "B", "basedOn": "A"})).unwrap();
        assert_eq!(r, json!({"parent": "B-Opening", "basedOn": "A-Parent"}));
        let img = page(&s, 1);
        assert!(black(img.pixel(50, 100)), "A's item: {:?}", img.pixel(50, 100));
        assert!(cyan(img.pixel(150, 150)), "B's item over A's: {:?}", img.pixel(150, 150));
        assert!(paper(img.pixel(170, 20)), "the page's item over B's: {:?}", img.pixel(170, 20));
        assert!(black(page(&s, 0).pixel(50, 100)) && !cyan(page(&s, 0).pixel(250, 100)), "page 1 uses A only");
        // Parent B itself shows A's items under its own.
        let mut rr = designcraft_render::Renderer::new();
        rr.threads = 0;
        let placed = [designcraft_render::Placed { spread: designcraft_doc::SpreadRef::Parent(1), xf: designcraft_geom::Affine::IDENTITY }];
        let opts = designcraft_render::RenderOptions { background: Some([255, 255, 255, 255]), ..Default::default() };
        let img = rr.render(&s.doc().unwrap().doc, &s.cache, &placed, 400, 400, designcraft_geom::Affine::IDENTITY, &opts);
        assert!(black(img.pixel(50, 100)) && cyan(img.pixel(150, 150)), "{:?} {:?}", img.pixel(50, 100), img.pixel(150, 150));
        let inspect = s.execute("document.inspect", &json!({})).unwrap();
        assert_eq!(inspect["parents"][1]["basedOn"], "A-Parent");
        assert_eq!(inspect["parents"][0]["basedOn"], Value::Null);

        // Cycles are refused.
        assert!(s.execute("layout.parents.options", &json!({"parent": "A", "basedOn": "B"})).is_err());
        assert!(s.execute("layout.parents.options", &json!({"parent": "B", "basedOn": "B-Opening"})).is_err());
        assert!(s.execute("layout.parents.options", &json!({"parent": "B", "basedOn": "Z"})).is_err());

        s.execute("edit.undo", &json!({})).unwrap();
        assert!(paper(page(&s, 1).pixel(50, 100)), "undo clears Based on Parent");
        s.execute("edit.redo", &json!({})).unwrap();
        s.execute("layout.parents.options", &json!({"parent": "B", "basedOn": null})).unwrap();
        assert_eq!(s.execute("document.inspect", &json!({})).unwrap()["parents"][1]["basedOn"], Value::Null);
    }

    /// A's page-number marker shown on parent XYZ (based on A) reads XYZ, as markers on XYZ's own items do.
    #[test]
    fn base_parent_markers_read_the_showing_parent() {
        let mut s = crate::Session::new();
        s.execute("file.new", &json!({"pages": 1, "facingPages": false})).unwrap();
        s.execute("layout.parents.new", &json!({"prefix": "XYZ"})).unwrap();
        s.execute("layout.parents.options", &json!({"parent": "XYZ", "basedOn": "A"})).unwrap();
        let marker = designcraft_doc::story::PAGE_NUMBER.to_string();
        let r = s
            .execute("frame.create", &json!({"spread": {"kind": "parent", "index": 0}, "rect": [72, 30, 500, 60], "content": "text", "text": marker}))
            .unwrap();
        let d = s.doc().unwrap().doc.clone();
        let sid = d.item(designcraft_doc::ItemId(r["id"].as_u64().unwrap())).unwrap().text_frame().unwrap().story;
        let glyphs = |on| -> usize {
            let cs = s.cache.get_on(&d, sid, on);
            cs.frames.iter().flat_map(|f| f.lines.iter()).map(|l| l.glyphs.iter().filter(|g| g.visible && g.adv > 0.0).count()).sum()
        };
        assert_eq!(glyphs(designcraft_compose::ShownOn::Own), 1, "A");
        assert_eq!(glyphs(designcraft_compose::ShownOn::Parent(1)), 3, "XYZ");
    }

    /// A file whose parents are based on each other opens, with the cycle broken and reported.
    #[test]
    fn opening_a_parent_cycle_breaks_it_with_a_warning() {
        let mut s = crate::Session::new();
        s.execute("file.new", &json!({"pages": 1})).unwrap();
        s.execute("layout.parents.new", &json!({"prefix": "B"})).unwrap();
        let mut d = s.doc().unwrap().doc.as_ref().clone();
        let (a, b) = (d.parents[0].id, d.parents[1].id);
        std::sync::Arc::make_mut(&mut d.parents[0]).parent.as_mut().unwrap().based_on = Some(b);
        std::sync::Arc::make_mut(&mut d.parents[1]).parent.as_mut().unwrap().based_on = Some(a);
        let b64 = crate::cmd::base64_encode(&crate::cmd::to_bytes(&d).unwrap());
        let r = s.execute("file.openBytes", &json!({"name": "cycle", "base64": b64})).unwrap();
        assert!(r["warnings"].to_string().contains("cycle"), "{r}");
        let d = &s.doc().unwrap().doc;
        assert_eq!(d.parents[0].parent.as_ref().unwrap().based_on, None);
        assert_eq!(d.parents[1].parent.as_ref().unwrap().based_on, Some(a));
    }
}

#[cfg(test)]
#[path = "parent_delete_tests.rs"]
mod parent_delete_tests;

#[cfg(test)]
mod insert_count_tests {
    use serde_json::json;

    use crate::Session;

    /// A count that is given but is not a number of pages used to insert one page anyway:
    /// `{"count": 0}` added a page, `-1` and `1.5` added one each, and `10000` was quietly capped
    /// to 9999. A position past the last page appends, which is deliberate.
    #[test]
    fn a_page_count_insert_pages_cannot_mean_is_refused() {
        let mut s = Session::new();
        s.execute("file.new", &json!({"pages": 2, "facingPages": false})).unwrap();
        let total = |s: &mut Session| s.execute("layout.pages.insert", &json!({"count": 1})).unwrap()["total"].as_u64().unwrap();
        assert_eq!(total(&mut s), 3, "one page added");

        for params in [json!({"count": 0}), json!({"count": -1}), json!({"count": 1.5}), json!({"count": 10000}), json!({"count": "two"})] {
            let e = s.execute("layout.pages.insert", &params).expect_err("refused").to_string();
            assert!(e.contains("1 to 9999 pages"), "{params}: {e}");
        }
        // Nothing was inserted by any of those.
        assert_eq!(s.doc().unwrap().doc.page_count(), 3);
        // A position past the last page still appends, which `insert_pages_bounds.rs` pins.
        let r = s.execute("layout.pages.insert", &json!({"after": 99})).unwrap();
        assert_eq!((r["pages"].as_u64(), r["total"].as_u64()), (Some(1), Some(4)));
        // The positions it does have still work: after the first page of three.
        let r = s.execute("layout.pages.insert", &json!({"count": 2, "after": 0})).unwrap();
        assert_eq!((r["pages"].as_u64(), r["total"].as_u64()), (Some(2), Some(6)));
    }
}
