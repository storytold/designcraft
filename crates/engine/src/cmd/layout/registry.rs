use super::layers::set_layer;
use super::margins::{adjust_layout, column_guides_unlocked, margin_boxes, move_column_guide, set_margins_columns, set_page_width};
use super::pages::{document_setup, page_options, page_size};
use super::parents::{parent_options, parent_ref, parent_spread_index};
use super::sections::{ChapterEdit, SectionEdit, chapter_numbering, start_number};
use super::{CommandSpec, LAYER_COLORS, Layer, LayerId, Margins, SpreadId, Value, bad, cmd, has_doc, json, ok, str_param};

pub fn specs() -> Vec<CommandSpec> {
    vec![
        cmd!(
            "layout.pages.insert",
            "Insert Pages…",
            ["Layout", "Pages"],
            Some("Cmd+Shift+P"),
            "{count?: 1 (1 to 9999), after?: 0-based page index (default: the last page; past it: at the end), before?: 0-based page index (instead of `after`; 0: at the start), parent?: id|\"A-Parent\"|\"A\"|null}",
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
                // A placement given (null included) must be a page index; only when neither is
                // given do the pages go after the last.
                if p.get("after").is_some() && p.get("before").is_some() {
                    return Err(bad("layout.pages.insert", "give `after` or `before`, not both"));
                }
                // `before` names a page of the document; an `after` past the last page appends
                // (`insert_pages_bounds.rs` pins that).
                let index = |k: &str| match p.get(k) {
                    None => Ok(None),
                    Some(v) => v
                        .as_u64()
                        .map(|v| usize::try_from(v).unwrap_or(usize::MAX))
                        .filter(|&i| k == "after" || i < n)
                        .map(|i| Some(i.min(n.saturating_sub(1))))
                        .ok_or_else(|| bad(ID, format!("`{k}` must be the index of one of the {n} pages, not {v}"))),
                };
                // The page the new ones follow; none: they go first.
                let after = match (index("after")?, index("before")?) {
                    (_, Some(b)) => b.checked_sub(1),
                    (a, None) => Some(a.unwrap_or(n.saturating_sub(1))),
                };
                let parent = parent_ref(ID, &st.doc, p.get("parent"))?.unwrap_or_else(|| st.doc.parents.first().map(|x| x.id));
                s.edit(|d, _| {
                    let ids = d.insert_pages(after, count, parent)?;
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
                    crate::cmd::object::duplicate_from(d, &src, &ids, designcraft_doc::SpreadRef::Doc(nsi), designcraft_geom::Vec2::ZERO)?;
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
                        crate::cmd::liquid::apply(d, designcraft_doc::SpreadRef::Doc(si), pi, old);
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
