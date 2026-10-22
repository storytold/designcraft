//! Parent item overrides (Layout › Pages): copy parent-page items onto a document page so they
//! can be edited there (the parent's version is hidden on that page), remove those local
//! overrides, or detach them from the parent.

use designcraft_doc::{Document, ItemId, SpreadRef};
use designcraft_geom::Vec2;
use serde_json::{Value, json};

use super::{CommandSpec, bad, cmd, has_doc};
use crate::{Result, Session};

pub fn specs() -> Vec<CommandSpec> {
    vec![
        cmd!(
            "layout.overrideParentItems",
            "Override All Parent Page Items",
            ["Layout", "Pages"],
            Some("Cmd+Alt+Shift+L"),
            "{page (0-based), ids?: parent items (default: all on the page's parent page)} → {ids} the page's editable copies",
            has_doc,
            override_items
        ),
        cmd!(
            "layout.removeOverrides",
            "Remove All Local Overrides",
            ["Layout", "Pages"],
            None,
            "{page (0-based)} — deletes the page's overridden copies; the parent items show again",
            has_doc,
            remove_overrides
        ),
        cmd!(
            "layout.detachAll",
            "Detach All Objects from Parent",
            ["Layout", "Pages"],
            None,
            "{page (0-based)} — the page's overridden copies become ordinary items",
            has_doc,
            detach_all
        ),
    ]
}

fn page_param(s: &Session, p: &Value, cmd: &str) -> Result<usize> {
    let d = &s.doc()?.doc;
    let page = p.get("page").and_then(Value::as_u64).map(|v| v as usize).or_else(|| {
        let st = s.active()?;
        st.selection.items.first().and_then(|i| st.doc.page_of_item(*i))
    });
    let page = page.unwrap_or(0);
    if page >= d.page_count() {
        return Err(bad(cmd, format!("no page {page}")));
    }
    Ok(page)
}

/// The parent items that show on a document page (its parent's and that parent's bases', back to
/// front), each with the offset from its parent spread to the document spread.
fn parent_items(d: &Document, page: usize) -> Option<Vec<(ItemId, Vec2)>> {
    let (si, pi) = d.page_loc(page)?;
    let pg = d.spreads.get(si)?.pages.get(pi)?;
    let shown = d.shown_parent_pages(SpreadRef::Doc(si), pi);
    if shown.is_empty() {
        return None;
    }
    let mut out = Vec::new();
    for (ppi, ppage) in shown {
        let Some(parent) = d.parents.get(ppi) else { continue };
        let Some(ppg) = parent.pages.get(ppage) else { continue };
        let off = Vec2::new(pg.x - ppg.x, 0.0);
        out.extend(
            parent
                .items
                .iter()
                .filter(|it| parent.pages.len() <= 1 || parent.page_at_x(it.bounds().center().x) == Some(ppage))
                .map(|it| (it.id, off)),
        );
    }
    Some(out)
}

fn override_items(s: &mut Session, p: &Value) -> Result<Value> {
    let page = page_param(s, p, "layout.overrideParentItems")?;
    let wanted: Option<Vec<ItemId>> = super::ids_param(p, "ids");
    s.edit(|d, sel| {
        let all = parent_items(d, page).ok_or_else(|| bad("layout.overrideParentItems", "the page has no parent"))?;
        let (si, pi) = d.page_loc(page).ok_or_else(|| bad("layout.overrideParentItems", "no such page"))?;
        let already = d.spreads[si].pages[pi].overridden.clone();
        let picked: Vec<(ItemId, Vec2)> =
            all.into_iter().filter(|(i, _)| wanted.as_ref().is_none_or(|w| w.contains(i)) && !already.contains(i)).collect();
        if picked.is_empty() {
            return Ok(json!({"ids": []}));
        }
        let ids: Vec<ItemId> = picked.iter().map(|(i, _)| *i).collect();
        let src = d.clone();
        // Copies on the document spread (stories and images come along), base parents' first.
        let mut new = Vec::new();
        for (id, off) in &picked {
            let copy = super::object::duplicate_from(d, &src, &[*id], SpreadRef::Doc(si), *off)?;
            if let Some(nid) = copy.first() {
                if let Some(it) = d.item_mut(*nid) {
                    it.overrides = Some(*id);
                }
                new.push(*nid);
            }
        }
        let pg = &mut std::sync::Arc::make_mut(&mut d.spreads[si]).pages[pi];
        pg.overridden.extend(ids.iter().copied());
        *sel = designcraft_doc::Selection::items(new.clone());
        Ok(json!({"ids": new.iter().map(|i| i.0).collect::<Vec<_>>()}))
    })
}

/// Items on a document page that override parent items.
fn overriding(d: &Document, page: usize) -> Vec<ItemId> {
    let Some((si, _)) = d.page_loc(page) else { return vec![] };
    d.spreads[si].items.iter().filter(|it| it.overrides.is_some() && d.page_of_item(it.id) == Some(page)).map(|it| it.id).collect()
}

fn remove_overrides(s: &mut Session, p: &Value) -> Result<Value> {
    let page = page_param(s, p, "layout.removeOverrides")?;
    s.edit(|d, sel| {
        let ids = overriding(d, page);
        for id in &ids {
            d.remove_item(*id)?;
        }
        if let Some((si, pi)) = d.page_loc(page) {
            std::sync::Arc::make_mut(&mut d.spreads[si]).pages[pi].overridden.clear();
        }
        sel.items.retain(|i| !ids.contains(i));
        Ok(json!({"removed": ids.len()}))
    })
}

fn detach_all(s: &mut Session, p: &Value) -> Result<Value> {
    let page = page_param(s, p, "layout.detachAll")?;
    s.edit(|d, _| {
        let ids = overriding(d, page);
        for id in &ids {
            if let Some(it) = d.item_mut(*id) {
                it.overrides = None;
            }
        }
        Ok(json!({"detached": ids.len()}))
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn override_remove_and_detach_parent_items() {
        let mut s = Session::new();
        s.execute("file.new", &json!({"pages": 3, "facingPages": false})).unwrap();
        // A folio frame on the parent.
        let r = s
            .execute(
                "frame.create",
                &json!({"spread": {"kind": "parent", "index": 0}, "rect": [72, 720, 300, 750], "content": "text", "text": "Folio"}),
            )
            .unwrap();
        let parent_id = ItemId(r["id"].as_u64().unwrap());
        let r = s.execute("layout.overrideParentItems", &json!({"page": 1})).unwrap();
        let ids = r["ids"].as_array().unwrap();
        assert_eq!(ids.len(), 1);
        let copy = ItemId(ids[0].as_u64().unwrap());
        let d = s.doc().unwrap().doc.clone();
        let it = d.item(copy).unwrap();
        assert_eq!(it.overrides, Some(parent_id));
        assert_eq!(d.page_of_item(copy), Some(1));
        let (si, pi) = d.page_loc(1).unwrap();
        assert_eq!(d.spreads[si].pages[pi].overridden, vec![parent_id]);
        // Its own story: editing it doesn't touch the parent's text.
        let sid = it.text_frame().unwrap().story;
        assert_ne!(Some(sid), d.item(parent_id).and_then(|p| p.text_frame()).map(|t| t.story));
        // Overriding again is a no-op.
        assert_eq!(s.execute("layout.overrideParentItems", &json!({"page": 1})).unwrap()["ids"].as_array().unwrap().len(), 0);
        // Detach keeps the copy as an ordinary item; remove deletes overrides only.
        s.execute("layout.detachAll", &json!({"page": 1})).unwrap();
        assert_eq!(s.doc().unwrap().doc.item(copy).unwrap().overrides, None);
        s.execute("layout.overrideParentItems", &json!({"page": 2})).unwrap();
        let r = s.execute("layout.removeOverrides", &json!({"page": 2})).unwrap();
        assert_eq!(r["removed"], 1);
        let d = s.doc().unwrap().doc.clone();
        let (si, pi) = d.page_loc(2).unwrap();
        assert!(d.spreads[si].pages[pi].overridden.is_empty());
        d.check().unwrap();
    }

    /// On a page whose parent B is based on A, A's items can be overridden too.
    #[test]
    fn override_items_from_a_base_parent() {
        let mut s = Session::new();
        s.execute("file.new", &json!({"pages": 2, "facingPages": false})).unwrap();
        s.execute("layout.parents.new", &json!({"prefix": "B"})).unwrap();
        let on = |s: &mut Session, index: usize, rect: [f64; 4]| {
            let r = s.execute("frame.create", &json!({"spread": {"kind": "parent", "index": index}, "rect": rect, "content": "unassigned"})).unwrap();
            ItemId(r["id"].as_u64().unwrap())
        };
        let a_item = on(&mut s, 0, [72.0, 72.0, 200.0, 200.0]);
        let b_item = on(&mut s, 1, [300.0, 72.0, 400.0, 200.0]);
        s.execute("layout.parents.options", &json!({"parent": "B", "basedOn": "A"})).unwrap();
        s.execute("layout.pages.applyParent", &json!({"pages": [1], "parent": "B"})).unwrap();
        let r = s.execute("layout.overrideParentItems", &json!({"page": 1, "ids": [a_item.0]})).unwrap();
        let copy = ItemId(r["ids"][0].as_u64().unwrap());
        let d = s.doc().unwrap().doc.clone();
        assert_eq!(d.item(copy).unwrap().overrides, Some(a_item));
        assert_eq!(d.page_of_item(copy), Some(1));
        let b = d.item(copy).unwrap().bounds();
        assert!((b.x0 - 72.0).abs() < 1e-9 && (b.y0 - 72.0).abs() < 1e-9, "{b:?}");
        // Overriding the rest picks up B's item only.
        let r = s.execute("layout.overrideParentItems", &json!({"page": 1})).unwrap();
        let rest: Vec<ItemId> = r["ids"].as_array().unwrap().iter().map(|v| ItemId(v.as_u64().unwrap())).collect();
        let d = s.doc().unwrap().doc.clone();
        assert_eq!(rest.iter().map(|i| d.item(*i).unwrap().overrides).collect::<Vec<_>>(), vec![Some(b_item)]);
        d.check().unwrap();
    }

    /// Facing pages: 2-page B based on 2-page A. Each document page offers only the items on its
    /// side of A, and the copy lands where the item showed: page 1 (a single right page at x 0)
    /// shows A's right-page items 612 pt to the left.
    #[test]
    fn override_base_items_on_facing_pages_keeps_their_place() {
        let mut s = Session::new();
        s.execute("file.new", &json!({"pages": 3, "facingPages": true})).unwrap();
        s.execute("layout.parents.new", &json!({"prefix": "B"})).unwrap();
        s.execute("layout.parents.options", &json!({"parent": "B", "basedOn": "A"})).unwrap();
        s.execute("layout.pages.applyParent", &json!({"pages": [0, 1, 2], "parent": "B"})).unwrap();
        let on_a = |s: &mut Session, rect: [f64; 4]| {
            let r = s.execute("frame.create", &json!({"spread": {"kind": "parent", "index": 0}, "rect": rect, "content": "unassigned"})).unwrap();
            ItemId(r["id"].as_u64().unwrap())
        };
        let left = on_a(&mut s, [72.0, 72.0, 200.0, 200.0]);
        let right = on_a(&mut s, [684.0, 72.0, 812.0, 200.0]);
        let copy_of = |s: &mut Session, page: usize, id: ItemId| -> Option<ItemId> {
            let r = s.execute("layout.overrideParentItems", &json!({"page": page, "ids": [id.0]})).unwrap();
            r["ids"].as_array().unwrap().first().map(|v| ItemId(v.as_u64().unwrap()))
        };
        assert_eq!(copy_of(&mut s, 0, left), None, "A's left-page item does not show on a right page");
        let c = copy_of(&mut s, 0, right).unwrap();
        let x0 = |s: &Session, id: ItemId| s.doc().unwrap().doc.item(id).unwrap().bounds().x0;
        assert!((x0(&s, c) - 72.0).abs() < 1e-9, "{}", x0(&s, c));
        // Page 2 is the left page of the second spread, page 3 its right page at x 612.
        let c = copy_of(&mut s, 1, left).unwrap();
        assert!((x0(&s, c) - 72.0).abs() < 1e-9, "{}", x0(&s, c));
        assert_eq!(s.doc().unwrap().doc.page_of_item(c), Some(1));
        let c = copy_of(&mut s, 2, right).unwrap();
        assert!((x0(&s, c) - 684.0).abs() < 1e-9, "{}", x0(&s, c));
        assert_eq!(s.doc().unwrap().doc.page_of_item(c), Some(2));
        s.doc().unwrap().doc.check().unwrap();
    }
}
