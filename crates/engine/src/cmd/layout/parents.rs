use super::{Result, SpreadId, Value, bad, json};

/// A parent spread named by prefix, label (`A-Parent`) or index.
pub(super) fn parent_spread_index(d: &designcraft_doc::Document, v: &Value) -> Option<usize> {
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
pub(super) fn parent_ref(cmd: &str, d: &designcraft_doc::Document, v: Option<&Value>) -> Result<Option<Option<SpreadId>>> {
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
pub(super) fn parent_options(s: &mut crate::Session, p: &Value) -> Result<Value> {
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

    /// Insert Pages before a page, or at the start of the document (`before: 0`), as InDesign's
    /// dialog offers; `after` and `before` together are refused without an undo step.
    #[test]
    fn pages_go_in_before_a_page_or_at_the_start() {
        let mut s = Session::new();
        // Facing: [1] [2 3].
        s.execute("file.new", &json!({"pages": 3})).unwrap();
        let b = s.execute("layout.parents.new", &json!({"prefix": "B"})).unwrap()["id"].as_u64().unwrap();
        let ids = |s: &Session| -> Vec<u64> {
            let d = &s.doc().unwrap().doc;
            (0..d.page_count()).map(|i| d.page(i).unwrap().id.0).collect()
        };
        let parent = |s: &Session, i: usize| s.doc().unwrap().doc.page(i).unwrap().parent.map(|p| p.0);
        let old = ids(&s);
        s.execute("layout.pages.insert", &json!({"before": 0, "parent": "B"})).unwrap();
        let now = ids(&s);
        assert_eq!(now.len(), 4);
        assert_eq!(now[1..], old[..], "the old pages follow the new first page");
        assert_eq!(parent(&s, 0), Some(b));
        // Before page 4 (index 3, the old page 3): two pages without a parent.
        s.execute("layout.pages.insert", &json!({"before": 3, "count": 2, "parent": null})).unwrap();
        let then = ids(&s);
        assert_eq!(then.len(), 6);
        assert_eq!(then[..3], now[..3]);
        assert_eq!(then[5], old[2]);
        assert_eq!((parent(&s, 3), parent(&s, 4)), (None, None));
        // Refused without a change or an undo step: both placements, even one of them null, and a
        // placement that isn't a page index.
        let undo = s.doc().unwrap().history.undo.len();
        for bad in [
            json!({"after": 0, "before": 1}),
            json!({"after": 0, "before": null}),
            json!({"after": null, "before": 1}),
            json!({"before": -1}),
            json!({"before": "0"}),
            json!({"before": null}),
            json!({"after": 1.5}),
            json!({"before": 6}),
            json!({"before": 99}),
        ] {
            assert!(s.execute("layout.pages.insert", &bad).is_err(), "{bad}");
            assert_eq!(s.doc().unwrap().history.undo.len(), undo, "{bad}");
            assert_eq!(ids(&s), then, "{bad}");
        }
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
        let img = rr.render(&s.doc().unwrap().doc, &s.cache, &placed, (400, 400), designcraft_geom::Affine::IDENTITY, &opts);
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
