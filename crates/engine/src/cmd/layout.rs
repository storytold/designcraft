//! Layout menu and Pages/Layers panels: pages, spreads, parents, margins & columns, layers.

use designcraft_doc::{LAYER_COLORS, Layer, LayerId, Margins, SpreadId};
use serde_json::{Value, json};

use super::{CommandSpec, bad, cmd, has_doc, ok, str_param};
use crate::{Result, Session};

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

pub fn specs() -> Vec<CommandSpec> {
    vec![
        cmd!(
            "layout.pages.insert",
            "Insert Pages…",
            ["Layout", "Pages"],
            Some("Cmd+Shift+P"),
            "{count?: 1, after?: page index (default: last), parent?: \"A\"|null}",
            has_doc,
            |s, p| {
                let count = p.get("count").and_then(Value::as_u64).unwrap_or(1).clamp(1, 9999) as usize;
                let st = s.doc()?;
                let n = st.doc.page_count();
                let after = p.get("after").and_then(Value::as_u64).map(|v| v as usize).unwrap_or(n - 1);
                let parent = parent_ref(&st.doc, p.get("parent")).unwrap_or_else(|| st.doc.parents.first().map(|x| x.id));
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
                let angle = p.get("angle").and_then(Value::as_i64).unwrap_or(90);
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
        cmd!("layout.pages.duplicateSpread", "Duplicate Spread", ["Layout", "Pages"], None, "{spread}", has_doc, |s, p| {
            let si = p.get("spread").and_then(Value::as_u64).unwrap_or(0) as usize;
            s.edit(|d, _| {
                let sp = d.spreads.get(si).cloned().ok_or_else(|| bad("layout.pages.duplicateSpread", "no such spread"))?;
                let after = d.first_page_of_spread(si) + sp.pages.len() - 1;
                let parent = sp.pages.first().and_then(|p| p.parent);
                d.insert_pages(Some(after), sp.pages.len(), parent)?;
                // Copy items onto the new spread(s) at the same positions.
                let src = d.clone();
                let ids: Vec<_> = sp.items.iter().map(|i| i.id).collect();
                let (nsi, _) = d.page_loc(after + 1).ok_or_else(|| bad("layout.pages.duplicateSpread", "insert failed"))?;
                super::object::duplicate_from(d, &src, &ids, designcraft_doc::SpreadRef::Doc(nsi), designcraft_geom::Vec2::ZERO)?;
                ok()
            })
        }),
        cmd!(
            "layout.pages.applyParent",
            "Apply Parent to Pages…",
            ["Layout", "Pages"],
            None,
            "{pages: [index], parent: \"A\"|null}",
            has_doc,
            |s, p| {
                let pages: Vec<usize> = p
                    .get("pages")
                    .and_then(Value::as_array)
                    .map(|a| a.iter().filter_map(Value::as_u64).map(|v| v as usize).collect())
                    .unwrap_or_default();
                let parent = parent_ref(&s.doc()?.doc, p.get("parent")).unwrap_or(None);
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
                let m = d.page(0).map(|p| (p.columns.count, p.columns.gutter, p.margins)).unwrap_or((1, 12.0, Margins::uniform(36.0)));
                let id = d.add_parent(&prefix, &name, m.0, m.1, m.2);
                Ok(json!({"id": id.0, "prefix": prefix}))
            })
        }),
        cmd!(noundo "layout.parents.edit", "Edit Parents", [], None, "{on: bool} — show parent spreads on the canvas", has_doc, |s, p| {
            let st = s.doc_mut()?;
            st.editing_parents = p.get("on").and_then(Value::as_bool).unwrap_or(!st.editing_parents);
            st.selection = Default::default();
            st.revision += 1;
            ok()
        }),
        cmd!(
            "layout.marginsAndColumns",
            "Margins and Columns…",
            ["Layout"],
            None,
            "{pages?: [index] (default all), margins?: number|{top,bottom,inside,outside}, columns?, gutter?, adjustLayout?: bool (objects follow the margins)}",
            has_doc,
            |s, p| {
                let pages: Option<Vec<usize>> =
                    p.get("pages").and_then(Value::as_array).map(|a| a.iter().filter_map(Value::as_u64).map(|v| v as usize).collect());
                let p = p.clone();
                let adjust = p.get("adjustLayout").and_then(Value::as_bool).unwrap_or(false);
                s.edit(|d, _| {
                    let before = adjust.then(|| margin_boxes(d));
                    let all: Vec<usize> = pages.clone().unwrap_or_else(|| (0..d.page_count()).collect());
                    for abs in all {
                        let Some((si, pi)) = d.page_loc(abs) else { continue };
                        let pg = &mut std::sync::Arc::make_mut(&mut d.spreads[si]).pages[pi];
                        match p.get("margins") {
                            Some(Value::Number(n)) => pg.margins = Margins::uniform(n.as_f64().unwrap_or(36.0)),
                            Some(v @ Value::Object(_)) => {
                                if let Ok(m) = serde_json::from_value(v.clone()) {
                                    pg.margins = m;
                                }
                            }
                            _ => {}
                        }
                        if let Some(c) = p.get("columns").and_then(Value::as_u64) {
                            pg.columns.count = (c as u32).clamp(1, 216);
                        }
                        if let Some(g) = p.get("gutter").and_then(Value::as_f64) {
                            pg.columns.gutter = g.max(0.0);
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
            "layout.documentSetup",
            "Document Setup…",
            ["File"],
            Some("Cmd+Alt+P"),
            "{width?, height?, pages?: count, startPage?: n, facingPages?, binding?: leftToRight|rightToLeft, intent?: print|web|mobile, bleed?, slug?: n | [top, bottom, inside, outside], adjustLayout?: bool (objects follow the new page size)} → the document setup",
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
                    if let Some(n) = p.get("startPage").and_then(Value::as_u64) {
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
                            first.start_number = Some(n.max(1) as u32);
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
                                pg.width = d.settings.page_width;
                                pg.height = d.settings.page_height;
                            }
                            sp.relayout();
                        }
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
            "{pages: [1-based page numbers], width?, height?, preset?: \"A4\"|\"Letter\"|…} — pages of their own size (Page tool); objects on the pages keep their place on them",
            has_doc,
            page_size
        ),
        cmd!(
            "layout.section",
            "Numbering & Section Options…",
            ["Layout"],
            None,
            "{page (1-based; starts a section there), startNumber?: n|null (continue), style?: arabic|upperRoman|lowerRoman|upperLetters|lowerLetters, prefix?, includePrefix?, marker?, remove?: bool}",
            has_doc,
            |s, p| {
                let page = p.get("page").and_then(Value::as_u64).ok_or_else(|| bad("layout.section", "missing page"))? as usize;
                let n = s.doc()?.doc.page_count();
                if page == 0 || page > n {
                    return Err(bad("layout.section", format!("no page {page}")));
                }
                let start = page - 1;
                let p = p.clone();
                s.edit(|d, _| {
                    d.sections.retain(|x| x.start != start || start == 0);
                    if p.get("remove").and_then(Value::as_bool).unwrap_or(false) && start != 0 {
                        return ok();
                    }
                    let mut sec = d.sections.iter().find(|x| x.start == start).cloned().unwrap_or(designcraft_doc::Section {
                        start,
                        start_number: None,
                        style: Default::default(),
                        prefix: String::new(),
                        marker: String::new(),
                        include_prefix: false,
                    });
                    match p.get("startNumber") {
                        Some(Value::Null) => sec.start_number = None,
                        Some(v) => sec.start_number = v.as_u64().map(|v| v.max(1) as u32),
                        None => {}
                    }
                    if let Some(v) = p.get("style") {
                        sec.style = serde_json::from_value(v.clone()).map_err(|e| bad("layout.section", e.to_string()))?;
                    }
                    if let Some(v) = p.get("prefix").and_then(Value::as_str) {
                        sec.prefix = v.into();
                    }
                    if let Some(v) = p.get("includePrefix").and_then(Value::as_bool) {
                        sec.include_prefix = v;
                    }
                    if let Some(v) = p.get("marker").and_then(Value::as_str) {
                        sec.marker = v.into();
                    }
                    d.sections.retain(|x| x.start != start);
                    d.sections.push(sec);
                    d.sections.sort_by_key(|x| x.start);
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
                let style = match p.get("style") {
                    Some(v) => match serde_json::from_value::<designcraft_doc::NumberStyle>(v.clone()) {
                        Ok(designcraft_doc::NumberStyle::Symbols) | Err(_) => {
                            return Err(bad("layout.chapterNumbering", format!("unknown chapter style {v}")));
                        }
                        Ok(style) => Some(style),
                    },
                    None => None,
                };
                let mut source = match p.get("source") {
                    Some(v) => Some(
                        serde_json::from_value::<designcraft_doc::ChapterSource>(v.clone())
                            .map_err(|_| bad("layout.chapterNumbering", format!("unknown chapter number source {v}")))?,
                    ),
                    None => None,
                };
                let start = match p.get("start") {
                    Some(v) => Some(
                        v.as_u64()
                            .filter(|n| (1..=MAX_CHAPTER).contains(n))
                            .ok_or_else(|| bad("layout.chapterNumbering", format!("start: 1 to {MAX_CHAPTER}")))? as u32,
                    ),
                    None => None,
                };
                // A start number alone means Start Chapter Numbering at.
                if start.is_some() && source.is_none() {
                    source = Some(designcraft_doc::ChapterSource::UserDefined);
                }
                s.edit(|d, _| {
                    if let Some(style) = style {
                        d.settings.chapter_style = style;
                    }
                    if let Some(source) = source {
                        d.settings.chapter_source = source;
                    }
                    if let Some(n) = start {
                        d.settings.chapter_number = n;
                    }
                    ok()
                })?;
                chapter_numbering(s)
            }
        ),
        cmd!("layer.move", "Move Layer", [], None, "{id, to: index (0 = top/frontmost)}", has_doc, |s, p| {
            let id = LayerId(p.get("id").and_then(Value::as_u64).unwrap_or(0));
            let to = p.get("to").and_then(Value::as_u64).unwrap_or(0) as usize;
            s.edit(|d, _| {
                let i = d.layers.iter().position(|l| l.id == id).ok_or_else(|| bad("layer.move", "no such layer"))?;
                let l = d.layers.remove(i);
                let to = to.min(d.layers.len());
                d.layers.insert(to, l);
                ok()
            })
        }),
        cmd!(noundo "layer.selectItems", "Select All on Layer", [], None, "{id}", has_doc, |s, p| {
            let id = LayerId(p.get("id").and_then(Value::as_u64).unwrap_or(0));
            let st = s.doc_mut()?;
            let ids: Vec<_> = st.doc.spreads.iter().flat_map(|sp| sp.items.iter().filter(|i| i.layer == id && !i.locked).map(|i| i.id)).collect();
            st.selection = designcraft_doc::Selection::items(ids);
            st.revision += 1;
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
        cmd!("layer.set", "Layer Options…", [], None, "{id, name?, visible?, locked?, printable?, color?: [r,g,b]}", has_doc, |s, p| {
            let id = LayerId(p.get("id").and_then(Value::as_u64).unwrap_or(0));
            let p = p.clone();
            s.edit(|d, _| {
                let l = d.layer_mut(id).ok_or_else(|| bad("layer.set", "no such layer"))?;
                if let Some(v) = p.get("name").and_then(Value::as_str) {
                    l.name = v.into();
                }
                if let Some(v) = p.get("visible").and_then(Value::as_bool) {
                    l.visible = v;
                }
                if let Some(v) = p.get("locked").and_then(Value::as_bool) {
                    l.locked = v;
                }
                if let Some(v) = p.get("printable").and_then(Value::as_bool) {
                    l.printable = v;
                }
                if let Some(c) = p.get("color").and_then(|c| serde_json::from_value(c.clone()).ok()) {
                    l.color = c;
                }
                ok()
            })
        }),
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
            st.revision += 1;
            ok()
        }),
    ]
}

/// `"A"` → that parent's id; `null` → Some(None) (= [None]); missing → None (use default).
fn parent_ref(d: &designcraft_doc::Document, v: Option<&Value>) -> Option<Option<SpreadId>> {
    match v? {
        Value::Null => Some(None),
        Value::String(s) if s == "[None]" => Some(None),
        Value::String(s) => Some(d.parents.iter().find(|p| p.parent.as_ref().is_some_and(|i| i.prefix == *s || i.label() == *s)).map(|p| p.id)),
        _ => None,
    }
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
    s.edit(|d, _| {
        resize_pages(d, &pages, w, h);
        Ok(json!({"pages": pages.len()}))
    })
}

/// Give `pages` (1-based) their own size. Objects stay on their page; pages with a liquid rule
/// re-flow their objects by it.
pub(crate) fn resize_pages(d: &mut designcraft_doc::Document, pages: &[usize], w: Option<f64>, h: Option<f64>) {
    {
        for page in pages {
            let Some((si, pi)) = d.page_loc(page - 1) else { continue };
            let sp = std::sync::Arc::make_mut(&mut d.spreads[si]);
            let old: Vec<f64> = sp.pages.iter().map(|x| x.x).collect();
            let old_bounds = sp.pages[pi].bounds();
            let pg = &mut sp.pages[pi];
            let dw = w.map_or(0.0, |w| w.max(1.0) - pg.width);
            if let Some(w) = w {
                pg.width = w.max(1.0);
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
            // Liquid rules: objects were carried along with the page's left edge.
            let moved = old_bounds + designcraft_geom::Vec2::new(new[pi] - old[pi], 0.0);
            super::liquid::apply(d, designcraft_doc::SpreadRef::Doc(si), pi, moved);
        }
    }
}

/// File › Document Setup values.
fn document_setup(d: &designcraft_doc::Document) -> Value {
    let st = &d.settings;
    let start = d.sections.iter().find(|x| x.start == 0).and_then(|x| x.start_number).unwrap_or(1);
    json!({"width": st.page_width, "height": st.page_height, "pages": d.page_count(), "startPage": start, "facingPages": st.facing_pages,
        "binding": if st.right_to_left_binding { "rightToLeft" } else { "leftToRight" }, "intent": st.intent, "bleed": st.bleed, "slug": st.slug})
}

/// The largest chapter number (as for page numbers).
const MAX_CHAPTER: u64 = 99_999;

/// The active document's chapter numbering, and the open book that holds it.
fn chapter_numbering(s: &Session) -> Result<Value> {
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
}

#[cfg(test)]
mod adjust_tests {
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
}
