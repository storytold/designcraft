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

/// The parent items that belong on a document page, across the whole parent chain, each with the
/// offset from parent spread to document spread.
fn parent_items(d: &Document, page: usize) -> Option<Vec<(ItemId, Vec2)>> {
    let (si, pi) = d.page_loc(page)?;
    let pg = d.spreads.get(si)?.pages.get(pi)?;
    let chain = d.parent_chain_for(page);
    if chain.is_empty() {
        return None;
    }
    let mut items = Vec::new();
    for (ppi, ppage) in chain {
        let parent = d.parents.get(ppi)?;
        let off = Vec2::new(pg.x - parent.pages.get(ppage)?.x, 0.0);
        items.extend(
            parent
                .items
                .iter()
                .filter(|it| parent.pages.len() <= 1 || parent.page_at_x(it.bounds().center().x) == Some(ppage))
                .map(|it| (it.id, off)),
        );
    }
    Some(items)
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
        let ids: Vec<ItemId> = picked.iter().map(|(i, _)| *i).collect();
        if ids.is_empty() {
            return Ok(json!({"ids": []}));
        }
        let src = d.clone();
        // Copies on the document spread (stories and images come along).
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
}
