use super::{LAYER_COLORS, LayerId, Result, Session, Value, bad, ok};

/// Layer Options: every field in one undo step; a bad name or colour changes nothing.
pub(super) fn set_layer(s: &mut Session, p: &Value) -> Result<Value> {
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
