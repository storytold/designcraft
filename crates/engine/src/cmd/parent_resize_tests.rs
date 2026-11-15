use std::sync::Arc;

use designcraft_doc::{Columns, LiquidRule, PageSide, SpreadId};
use designcraft_geom::Rect;
use serde_json::json;

use crate::Session;

fn parent_id(s: &Session, index: usize) -> u64 {
    s.doc().unwrap().doc.parents[index].id.0
}

fn parent_page(s: &Session, index: usize, page: usize) -> designcraft_doc::Page {
    s.doc().unwrap().doc.parents[index].pages[page].clone()
}

fn parent_frame(s: &mut Session, x: f64) -> u64 {
    s.execute(
        "frame.create",
        &json!({
            "spread": {"kind": "parent", "index": 0},
            "rect": [x, 80.0, x + 100.0, 180.0]
        }),
    )
    .unwrap()["id"]
        .as_u64()
        .unwrap()
}

fn bounds(s: &Session, id: u64) -> Rect {
    s.doc().unwrap().doc.item(designcraft_doc::ItemId(id)).unwrap().bounds()
}

fn guide_positions(s: &Session, page: usize) -> Vec<f64> {
    s.doc().unwrap().doc.parents[0].pages[page].guides.iter().map(|g| g.position).collect()
}

fn prepare_parent_pages(s: &mut Session, binding: &str, rule: LiquidRule) -> (u64, u64, u64, usize, usize) {
    s.execute("file.new", &json!({"pages": 2, "facingPages": true, "width": 400.0, "height": 400.0})).unwrap();
    s.execute("layout.documentSetup", &json!({"binding": binding})).unwrap();
    assert_eq!(s.doc().unwrap().doc.settings.right_to_left_binding, binding == "rightToLeft");
    assert_eq!(s.doc().unwrap().doc.parents[0].pages[0].side, if binding == "rightToLeft" { PageSide::Right } else { PageSide::Left });
    let target = s.doc().unwrap().doc.parents[0].pages.iter().position(|p| p.side == PageSide::Right).unwrap();
    let other = 1 - target;
    let target_page = parent_page(s, 0, target);
    let other_page = parent_page(s, 0, other);
    let target_item = parent_frame(s, target_page.x + 80.0);
    let other_item = parent_frame(s, other_page.x + 80.0);
    s.execute(
        "guide.add",
        &json!({"spread": {"kind": "parent", "index": 0}, "page": target, "orientation": "vertical", "position": target_page.x + 120.0}),
    )
    .unwrap();
    s.execute(
        "guide.add",
        &json!({"spread": {"kind": "parent", "index": 0}, "page": target, "orientation": "horizontal", "position": 120.0, "at": target_page.x + 10.0}),
    )
    .unwrap();
    s.execute(
        "guide.add",
        &json!({"spread": {"kind": "parent", "index": 0}, "page": other, "orientation": "vertical", "position": other_page.x + 120.0}),
    )
    .unwrap();
    s.execute(
        "guide.add",
        &json!({"spread": {"kind": "parent", "index": 0}, "page": other, "orientation": "horizontal", "position": 120.0, "at": other_page.x + 10.0}),
    )
    .unwrap();
    s.edit(|d, _| {
        let sp = Arc::make_mut(&mut d.parents[0]);
        let positions = |page: &designcraft_doc::Page| {
            let w = page.margin_rect().width();
            vec![0.0, 100.0, 120.0, w]
        };
        for page in &mut sp.pages {
            page.columns = Columns { count: 2, gutter: 20.0, positions: Some(positions(page)) };
        }
        sp.pages[target].liquid = rule;
        Ok(())
    })
    .unwrap();
    (parent_id(s, 0), target_item, other_item, target, other)
}

#[test]
fn parent_resize_changes_only_the_target_page_in_ltr_and_rtl() {
    for binding in ["leftToRight", "rightToLeft"] {
        for rule in [
            LiquidRule::Off,
            LiquidRule::Scale,
            LiquidRule::ReCenter,
            LiquidRule::ObjectBased,
            LiquidRule::GuideBased,
            LiquidRule::ControlledByParent,
        ] {
            let mut s = Session::new();
            let (parent, target_item, other_item, target, other) = prepare_parent_pages(&mut s, binding, rule);
            let before_target = parent_page(&s, 0, target);
            let before_other = parent_page(&s, 0, other);
            let before_target_item = bounds(&s, target_item);
            let before_other_item = bounds(&s, other_item);
            let before_target_guides = guide_positions(&s, target);
            let before_other_guides = guide_positions(&s, other);
            let before_target_columns = before_target.columns.positions.clone();
            let before_other_columns = before_other.columns.positions.clone();

            s.execute("layout.selectParentPage", &json!({"parent": parent, "page": target})).unwrap();
            assert_eq!(s.doc().unwrap().selection.parent_page, Some((SpreadId(parent), target)));
            assert!(s.doc().unwrap().selection.items.is_empty());
            s.execute("layout.parentPageSize", &json!({"parent": parent, "page": target, "width": 500.0, "height": 500.0, "anchor": 8})).unwrap();

            let after_target = parent_page(&s, 0, target);
            let after_other = parent_page(&s, 0, other);
            assert_eq!((after_target.width, after_target.height), (500.0, 500.0), "{binding} {rule:?}");
            assert_eq!((after_other.width, after_other.height), (before_other.width, before_other.height), "{binding} {rule:?}");
            assert_ne!(bounds(&s, target_item), before_target_item, "target object did not follow the resize: {binding} {rule:?}");
            assert_eq!(bounds(&s, other_item), before_other_item, "other page object moved: {binding} {rule:?}");
            assert_ne!(guide_positions(&s, target), before_target_guides, "target guides did not follow the resize: {binding} {rule:?}");
            assert_eq!(guide_positions(&s, other), before_other_guides, "other page guides moved: {binding} {rule:?}");
            assert_ne!(after_target.columns.positions, before_target_columns, "target columns did not scale: {binding} {rule:?}");
            assert_eq!(after_other.columns.positions, before_other_columns, "other page columns changed: {binding} {rule:?}");
            assert_eq!(s.doc().unwrap().doc.settings.page_width, 400.0);
            assert_eq!(s.doc().unwrap().doc.settings.page_height, 400.0);
            s.doc().unwrap().doc.check().unwrap();
        }
    }
}

#[test]
fn parent_resize_is_independent_of_applied_and_based_on_parents_and_undoable() {
    let mut s = Session::new();
    s.execute("file.new", &json!({"pages": 1, "facingPages": false, "width": 400.0, "height": 400.0})).unwrap();
    let a = parent_id(&s, 0);
    let b = s.execute("layout.parents.new", &json!({"prefix": "B", "name": "Child"})).unwrap()["id"].as_u64().unwrap();
    s.execute("layout.parents.options", &json!({"parent": b, "basedOn": a})).unwrap();
    s.execute("layout.pages.applyParent", &json!({"pages": [0], "parent": b})).unwrap();
    let before = s.doc().unwrap().doc.as_ref().clone();
    let undo_before = s.doc().unwrap().history.undo.len();

    s.execute("layout.parentPageSize", &json!({"parent": a, "page": 0, "width": 0.5, "anchor": 0})).unwrap();
    let d = &s.doc().unwrap().doc;
    assert_eq!(d.parents[0].pages[0].width, 0.5);
    assert_eq!(d.parents[1].pages[0].width, before.parents[1].pages[0].width);
    assert_eq!(d.spreads[0].pages[0].width, before.spreads[0].pages[0].width);
    assert_eq!(d.parents[1].parent.as_ref().unwrap().based_on, Some(SpreadId(a)));
    assert_eq!(d.settings.page_width, before.settings.page_width);
    d.check().unwrap();
    assert_eq!(s.doc().unwrap().history.undo.len(), undo_before + 1);

    s.execute("edit.undo", &json!({})).unwrap();
    assert_eq!(s.doc().unwrap().doc.parents[0].pages[0].width, before.parents[0].pages[0].width);
    s.execute("edit.redo", &json!({})).unwrap();
    assert_eq!(s.doc().unwrap().doc.parents[0].pages[0].width, 0.5);
    s.doc().unwrap().doc.check().unwrap();
}

#[test]
fn parent_resize_survives_native_and_idml_round_trips() {
    let mut s = Session::new();
    s.execute("file.new", &json!({"pages": 1, "facingPages": false, "width": 400.0, "height": 400.0})).unwrap();
    let a = parent_id(&s, 0);
    s.execute("layout.parentPageSize", &json!({"parent": a, "page": 0, "width": 0.5, "height": 0.75})).unwrap();
    let d = s.doc().unwrap().doc.as_ref().clone();
    d.check().unwrap();

    let native = crate::cmd::from_bytes(&crate::cmd::to_bytes(&d).unwrap()).unwrap();
    assert_eq!((native.parents[0].pages[0].width, native.parents[0].pages[0].height), (0.5, 0.75));
    native.check().unwrap();

    let idml = designcraft_idml::export_idml(&d);
    let reopened = designcraft_idml::import_idml(&idml).unwrap();
    assert_eq!((reopened.parents[0].pages[0].width, reopened.parents[0].pages[0].height), (0.5, 0.75));
    reopened.check().unwrap();
}

#[test]
fn parent_resize_rejects_invalid_inputs_atomically() {
    let mut s = Session::new();
    s.execute("file.new", &json!({"pages": 1, "facingPages": false})).unwrap();
    let parent = parent_id(&s, 0);
    let before = s.doc().unwrap().doc.as_ref().clone();
    let undo_before = s.doc().unwrap().history.undo.len();
    let invalid = [
        json!({"parent": parent, "page": 0, "width": null}),
        json!({"parent": parent, "page": 0, "width": "400"}),
        json!({"parent": parent, "page": 0, "width": "NaN"}),
        json!({"parent": parent, "page": 0, "width": "Infinity"}),
        json!({"parent": parent, "page": 0, "width": 0}),
        json!({"parent": parent, "page": 0, "width": -1}),
        json!({"parent": parent, "page": 0, "width": 15552.1}),
        json!({"parent": parent, "page": 0, "width": 400, "anchor": 9}),
        json!({"parent": parent, "page": 1, "width": 400}),
        json!({"parent": 999_999, "page": 0, "width": 400}),
    ];
    for params in invalid {
        assert!(s.execute("layout.parentPageSize", &params).is_err(), "accepted {params}");
        assert_eq!(s.doc().unwrap().doc.as_ref(), &before, "mutated after {params}");
        assert_eq!(s.doc().unwrap().history.undo.len(), undo_before, "added undo after {params}");
    }
    assert!(s.execute("layout.parentPageSize", &json!({"parent": parent, "page": 0})).is_err());
    assert_eq!(s.doc().unwrap().doc.as_ref(), &before);
    assert_eq!(s.doc().unwrap().history.undo.len(), undo_before);
    s.doc().unwrap().doc.check().unwrap();
}

#[test]
fn parent_and_document_page_selection_validate_and_clear_each_other() {
    let mut s = Session::new();
    s.execute("file.new", &json!({"pages": 1, "facingPages": false})).unwrap();
    let parent = parent_id(&s, 0);
    let item = s.execute("frame.create", &json!({"rect": [40, 40, 100, 100]})).unwrap()["id"].as_u64().unwrap();
    assert!(!s.doc().unwrap().selection.items.is_empty());
    s.execute("layout.selectParentPage", &json!({"parent": parent, "page": 0})).unwrap();
    assert_eq!(s.doc().unwrap().selection.parent_page, Some((SpreadId(parent), 0)));
    assert!(s.doc().unwrap().selection.items.is_empty());
    assert!(s.execute("layout.selectParentPage", &json!({"parent": parent, "page": 1})).is_err());
    assert_eq!(s.doc().unwrap().selection.parent_page, Some((SpreadId(parent), 0)));
    s.execute("layout.selectPage", &json!({"page": 1, "ids": [item]})).unwrap();
    assert_eq!(s.doc().unwrap().selection.page, Some(0));
    assert_eq!(s.doc().unwrap().selection.parent_page, None);
    s.execute("layout.selectParentPage", &json!({"parent": null})).unwrap();
    assert_eq!(s.doc().unwrap().selection, Default::default());
}

#[test]
fn parent_resize_repeated_edits_keep_the_spine_and_other_page_objects() {
    for binding in ["leftToRight", "rightToLeft"] {
        for pi in 0..2 {
            let mut s = Session::new();
            s.execute("file.new", &json!({"width":400,"height":600,"pages":3})).unwrap();
            s.execute("layout.documentSetup", &json!({"binding":binding})).unwrap();
            let id = parent_id(&s, 0);
            let before = s.doc().unwrap().doc.clone();
            let spine = before.parents[0].spine_x();
            let other = 1 - pi;
            let ox = before.parents[0].pages[other].x + 80.0;
            let object = parent_frame(&mut s, ox);
            let old_object = bounds(&s, object);
            for params in
                [json!({"parent":id,"page":pi,"width":500}), json!({"parent":id,"page":pi,"height":700}), json!({"parent":id,"page":pi,"width":550})]
            {
                s.execute("layout.parentPageSize", &params).unwrap();
                assert_eq!(s.doc().unwrap().doc.parents[0].spine_x(), spine, "spine moved: {binding}, page {pi}, {params}");
                assert_eq!(bounds(&s, object), old_object, "other-page object moved: {binding}, page {pi}");
            }
        }
    }
}

#[test]
fn parent_resize_keeps_margin_and_column_inheritance_without_resizing_applied_pages() {
    let mut s = Session::new();
    s.execute("file.new", &json!({"width":400,"height":600,"pages":3})).unwrap();
    let id = parent_id(&s, 0);
    s.edit(|d, _| {
        let pg = &mut Arc::make_mut(&mut d.parents[0]).pages[1];
        pg.columns = Columns { count: 2, gutter: 20.0, positions: Some(vec![0.0, 100.0, 120.0, 328.0]) };
        d.sync_parent_margins();
        Arc::make_mut(&mut d.spreads[1]).pages[1].local_margins = true;
        Ok(())
    })
    .unwrap();
    let before = s.doc().unwrap().doc.clone();
    s.execute("layout.parentPageSize", &json!({"parent":id,"page":1,"width":500})).unwrap();
    let d = &s.doc().unwrap().doc;
    assert_eq!(d.page(0).unwrap().columns, d.parents[0].pages[1].columns);
    assert_eq!(d.page(0).unwrap().margins, d.parents[0].pages[1].margins);
    assert_eq!((d.page(0).unwrap().width, d.page(0).unwrap().height), (400.0, 600.0));
    assert_eq!(d.page(2).unwrap(), before.page(2).unwrap());
    d.check().unwrap();
    s.execute("layout.pages.applyParent", &json!({"pages":[0],"parent":null})).unwrap();
    s.execute("layout.pages.applyParent", &json!({"pages":[0],"parent":id})).unwrap();
    assert_eq!((s.doc().unwrap().doc.page(0).unwrap().width, s.doc().unwrap().doc.page(0).unwrap().height), (400.0, 600.0));
}

#[test]
fn parent_resize_facing_sizes_survive_idml_in_both_bindings() {
    for binding in ["leftToRight", "rightToLeft"] {
        let mut s = Session::new();
        s.execute("file.new", &json!({"width":400,"height":600,"pages":3,"facingPages":true})).unwrap();
        s.execute("layout.documentSetup", &json!({"binding":binding})).unwrap();
        let id = parent_id(&s, 0);
        for (pi, w, h) in [(0, 500, 700), (1, 300, 550)] {
            s.execute("layout.parentPageSize", &json!({"parent":id,"page":pi,"width":w,"height":h})).unwrap();
        }
        let d = &s.doc().unwrap().doc;
        let reopened = designcraft_idml::import_idml(&designcraft_idml::export_idml(d)).unwrap();
        assert_eq!(reopened.settings.right_to_left_binding, d.settings.right_to_left_binding);
        for page in &d.parents[0].pages {
            let restored = reopened.parents[0].pages.iter().find(|p| p.side == page.side).unwrap();
            assert_eq!((restored.width, restored.height), (page.width, page.height), "{binding}, {:?}", page.side);
        }
        for abs in 0..d.page_count() {
            let (old, new) = (d.page(abs).unwrap(), reopened.page(abs).unwrap());
            assert_eq!((new.width, new.height), (old.width, old.height));
        }
        reopened.check().unwrap();
    }
}
