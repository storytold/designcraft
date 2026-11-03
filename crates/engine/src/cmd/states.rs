//! Window › Interactive › Object States: a group whose objects are alternative states, one shown.

use designcraft_doc::{ItemId, Shape};
use serde_json::{Value, json};

use super::{CommandSpec, bad, cmd, has_doc, has_selection};
use crate::{Result, Session};

fn target(s: &Session, p: &Value) -> Result<ItemId> {
    p.get("id")
        .and_then(Value::as_u64)
        .map(ItemId)
        .or_else(|| s.active().and_then(|d| d.selection.items.first().copied()))
        .ok_or_else(|| bad("states", "select a multi-state object"))
}

pub fn specs() -> Vec<CommandSpec> {
    vec![
        cmd!(
            "states.create",
            "Convert Selection to Multi-State Object",
            ["Window", "Interactive", "Object States"],
            None,
            "{ids? (default: the selection; one group's objects or several objects become states)} → {id, states}",
            has_selection,
            |s, p| {
                let ids = super::targets(s, p)?;
                let single_group = ids.len() == 1 && s.doc()?.doc.item(ids[0]).is_some_and(|it| it.shape == Shape::Group && it.children().len() > 1);
                let gid = if single_group {
                    ids[0]
                } else if ids.len() > 1 {
                    ItemId(s.execute("object.group", &json!({"ids": ids.iter().map(|i| i.0).collect::<Vec<_>>()}))?["id"].as_u64().unwrap_or(0))
                } else {
                    return Err(bad("states.create", "select two or more objects (or a group)"));
                };
                s.edit(|d, _| {
                    let g = d.item_mut(gid).ok_or_else(|| bad("states.create", "no such group"))?;
                    let n = g.children().len();
                    g.states = (1..=n).map(|i| format!("State {i}")).collect();
                    g.active_state = 0;
                    Ok(json!({"id": gid.0, "states": g.states}))
                })
            }
        ),
        cmd!(query "states.list", "Object States", [], None, "{id?} → {states, active}", has_doc, |s, p| {
            let id = target(s, p)?;
            let it = s.doc()?.doc.item(id).ok_or_else(|| bad("states.list", "no such object"))?;
            Ok(json!({"states": it.states, "active": it.active_state}))
        }),
        cmd!("states.show", "Show State", [], None, "{id?, index? | name?}", has_doc, |s, p| {
            let id = target(s, p)?;
            let idx = p.get("index").and_then(Value::as_u64).map(|v| v as usize);
            let name = p.get("name").and_then(Value::as_str).map(str::to_string);
            s.edit(|d, _| {
                let it = d.item_mut(id).ok_or_else(|| bad("states.show", "no such object"))?;
                if it.states.is_empty() {
                    return Err(bad("states.show", "not a multi-state object"));
                }
                let i = match (idx, name) {
                    (Some(i), _) => i,
                    (None, Some(n)) => it.states.iter().position(|x| *x == n).ok_or_else(|| bad("states.show", format!("no state `{n}`")))?,
                    _ => return Err(bad("states.show", "`index` or `name` required")),
                };
                if i >= it.states.len() {
                    return Err(bad("states.show", format!("no state {i}")));
                }
                it.active_state = i;
                Ok(json!({"active": i}))
            })
        }),
        cmd!("states.rename", "State Options", [], None, "{id?, index, name}", has_selection, |s, p| {
            let id = target(s, p)?;
            let i = p.get("index").and_then(Value::as_u64).ok_or_else(|| bad("states.rename", "`index` required"))? as usize;
            let name = p.get("name").and_then(Value::as_str).unwrap_or("").to_string();
            s.edit(|d, _| {
                let it = d.item_mut(id).ok_or_else(|| bad("states.rename", "no such object"))?;
                let st = it.states.get_mut(i).ok_or_else(|| bad("states.rename", format!("no state {i}")))?;
                *st = name;
                Ok(Value::Null)
            })
        }),
        cmd!("states.release", "Release State to Objects", [], None, "{id?} — back to a plain group (every state shows)", has_selection, |s, p| {
            let id = target(s, p)?;
            s.edit(|d, _| {
                let it = d.item_mut(id).ok_or_else(|| bad("states.release", "no such object"))?;
                it.states.clear();
                it.active_state = 0;
                Ok(Value::Null)
            })
        }),
    ]
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use crate::Session;

    #[test]
    fn multi_state_object_shows_one_state() {
        let mut s = Session::new();
        s.execute("file.new", &json!({})).unwrap();
        let a = s.execute("frame.create", &json!({"rect": [100, 100, 200, 200]})).unwrap()["id"].clone();
        let b = s.execute("frame.create", &json!({"rect": [100, 100, 200, 200]})).unwrap()["id"].clone();
        s.execute("object.fill", &json!({"swatch": "[Black]", "ids": [a]})).unwrap();
        s.execute("object.fill", &json!({"swatch": "C=100 M=0 Y=0 K=0", "ids": [b]})).unwrap();
        let r = s.execute("states.create", &json!({"ids": [a, b]})).unwrap();
        let id = r["id"].clone();
        assert_eq!(r["states"].as_array().unwrap().len(), 2);
        let px = |s: &Session| {
            let d = s.doc().unwrap().doc.clone();
            let mut rr = designcraft_render::Renderer::new();
            rr.threads = 0;
            rr.render_page(&d, &s.cache, 0, 1.0, false, &Default::default()).unwrap().pixel(150, 150)
        };
        let first = px(&s);
        assert!(first[0] < 80 && first[2] < 80, "state 1 (black) shows, not cyan on top: {first:?}");
        s.execute("states.show", &json!({"id": id, "name": "State 2"})).unwrap();
        let second = px(&s);
        assert!(second[2] > 150 && second[0] < 100, "state 2 (cyan): {second:?}");
        s.execute("states.release", &json!({"id": id})).unwrap();
        assert!(s.execute("states.list", &json!({"id": id})).unwrap()["states"].as_array().unwrap().is_empty());
    }
}
