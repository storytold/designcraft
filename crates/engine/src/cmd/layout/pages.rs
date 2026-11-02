use super::margins::set_page_width;
use super::{Result, Value, bad, json, str_param};

#[allow(dead_code)]
fn _r(_: Result<()>) {}

/// Layout › Page Size (the Page tool): resize pages one by one. Left pages grow to the left
/// (the spine stays put); objects move with their page.
pub(super) fn page_size(s: &mut crate::Session, p: &Value) -> Result<Value> {
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

pub(super) fn page_options(s: &mut crate::Session, p: &Value) -> Result<Value> {
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
            crate::cmd::liquid::apply(d, designcraft_doc::SpreadRef::Doc(si), pi, moved);
        }
    }
}

/// File › Document Setup values.
pub(super) fn document_setup(d: &designcraft_doc::Document) -> Value {
    let st = &d.settings;
    let start = d.sections.iter().find(|x| x.start == 0).and_then(|x| x.start_number).unwrap_or(1);
    json!({"width": st.page_width, "height": st.page_height, "pages": d.page_count(), "startPage": start, "facingPages": st.facing_pages,
        "binding": if st.right_to_left_binding { "rightToLeft" } else { "leftToRight" }, "intent": st.intent, "bleed": st.bleed, "slug": st.slug,
        "margins": d.page(0).map(|p| p.margins), "primaryTextFrame": st.primary_text_frame})
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
