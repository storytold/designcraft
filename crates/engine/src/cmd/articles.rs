//! Window › Articles: named reading-order lists of objects (EPUB / HTML export order).

use designcraft_doc::{Article, ItemId};
use serde_json::{Value, json};

use super::{CommandSpec, bad, cmd, has_doc, ids_param, ok, str_param};
use crate::Session;

fn sel_ids(s: &Session, p: &Value) -> Vec<ItemId> {
    ids_param(p, "ids").unwrap_or_else(|| s.active().map(|d| d.selection.items.clone()).unwrap_or_default())
}

pub fn specs() -> Vec<CommandSpec> {
    vec![
        cmd!(query "article.list", "Articles", [], None, "{} → [{name, items, export}]", has_doc, |s, _| Ok(serde_json::to_value(&s.doc()?.doc.articles).unwrap_or_default())),
        cmd!("article.new", "New Article", ["Window", "Articles"], None, "{name?, ids? (default: the selection)} → {name}", has_doc, |s, p| {
            let ids = sel_ids(s, p);
            let want = str_param(p, "name").map(str::to_string);
            s.edit(|d, _| {
                let name = match want {
                    Some(n) if d.articles.iter().any(|a| a.name == n) => return Err(bad("article.new", format!("an article named `{n}` exists"))),
                    Some(n) => n,
                    None => (1..=d.articles.len() + 1)
                        .map(|i| format!("Article {i}"))
                        .find(|n| !d.articles.iter().any(|a| a.name == *n))
                        .ok_or_else(|| bad("article.new", "no free article name"))?,
                };
                d.articles.push(Article { name: name.clone(), items: ids.into_iter().filter(|i| d.item(*i).is_some()).collect(), export: true });
                Ok(json!({"name": name}))
            })
        }),
        cmd!("article.add", "Add Selection to Article", [], None, "{name, ids? (default: the selection)} — appended in order", has_doc, |s, p| {
            let ids = sel_ids(s, p);
            let name = str_param(p, "name").unwrap_or("").to_string();
            s.edit(|d, _| {
                let valid: Vec<ItemId> = ids.into_iter().filter(|i| d.item(*i).is_some()).collect();
                let a = d.articles.iter_mut().find(|a| a.name == name).ok_or_else(|| bad("article.add", format!("no article `{name}`")))?;
                for i in valid {
                    if !a.items.contains(&i) {
                        a.items.push(i);
                    }
                }
                ok()
            })
        }),
        cmd!("article.remove", "Remove from Article", [], None, "{name, ids}", has_doc, |s, p| {
            let ids = sel_ids(s, p);
            let name = str_param(p, "name").unwrap_or("").to_string();
            s.edit(|d, _| {
                let a = d.articles.iter_mut().find(|a| a.name == name).ok_or_else(|| bad("article.remove", format!("no article `{name}`")))?;
                a.items.retain(|i| !ids.contains(i));
                ok()
            })
        }),
        cmd!("article.options", "Article Options", [], None, "{name, to?: new name, export?: bool, order?: [ids] (reorder)}", has_doc, |s, p| {
            let name = str_param(p, "name").unwrap_or("").to_string();
            let to = str_param(p, "to").map(str::to_string);
            let export = p.get("export").and_then(Value::as_bool);
            let order = ids_param(p, "order");
            s.edit(|d, _| {
                let current = d.articles.iter().find(|a| a.name == name).ok_or_else(|| bad("article.options", format!("no article `{name}`")))?;
                // Commands identify articles by name; a duplicate would make edits ambiguous.
                if let Some(t) = &to
                    && *t != current.name
                    && d.articles.iter().any(|a| a.name == *t)
                {
                    return Err(bad("article.options", format!("an article named `{t}` exists")));
                }
                let a = d.articles.iter_mut().find(|a| a.name == name).ok_or_else(|| bad("article.options", format!("no article `{name}`")))?;
                if let Some(e) = export {
                    a.export = e;
                }
                if let Some(o) = order {
                    let mut items: Vec<ItemId> = o.into_iter().filter(|i| a.items.contains(i)).collect();
                    items.extend(a.items.iter().filter(|i| !items.contains(i)).copied().collect::<Vec<_>>());
                    a.items = items;
                }
                if let Some(t) = to {
                    a.name = t;
                }
                ok()
            })
        }),
        cmd!("article.delete", "Delete Article", [], None, "{name}", has_doc, |s, p| {
            let name = str_param(p, "name").unwrap_or("").to_string();
            s.edit(|d, _| {
                let n = d.articles.len();
                d.articles.retain(|a| a.name != name);
                if n == d.articles.len() {
                    return Err(bad("article.delete", format!("no article `{name}`")));
                }
                ok()
            })
        }),
    ]
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use crate::Session;

    #[test]
    fn articles_set_the_export_reading_order() {
        let mut s = Session::new();
        s.execute("file.new", &json!({})).unwrap();
        let a =
            s.execute("frame.create", &json!({"rect": [72, 72, 300, 100], "content": "text", "text": "Top story", "caret": false})).unwrap()["id"]
                .clone();
        let b =
            s.execute("frame.create", &json!({"rect": [72, 400, 300, 430], "content": "text", "text": "Lower story", "caret": false})).unwrap()["id"]
                .clone();
        let html = |s: &mut Session| s.execute("file.exportHtml", &json!({})).unwrap()["text"].as_str().unwrap().to_string();
        let h = html(&mut s);
        assert!(h.find("Top").unwrap() < h.find("Lower").unwrap(), "page order without articles");
        s.execute("article.new", &json!({"name": "Main", "ids": [b, a]})).unwrap();
        let h = html(&mut s);
        assert!(h.find("Lower").unwrap() < h.find("Top").unwrap(), "article order");
        s.execute("article.remove", &json!({"name": "Main", "ids": [a]})).unwrap();
        assert!(!html(&mut s).contains("Top story"), "only articles' objects are exported");
        s.execute("article.options", &json!({"name": "Main", "export": false})).unwrap();
        assert!(html(&mut s).contains("Top story"), "no exported articles: page order again");
    }

    #[test]
    fn article_renames_keep_names_unique_and_reject_conflicts_atomically() {
        let mut s = Session::new();
        s.execute("file.new", &json!({})).unwrap();
        s.execute("article.new", &json!({"name": "Main", "ids": []})).unwrap();
        s.execute("article.new", &json!({"name": "Other", "ids": []})).unwrap();
        let before = s.execute("article.list", &json!({})).unwrap();
        let undo = s.doc().unwrap().history.undo.len();

        let err = s.execute("article.options", &json!({"name": "Main", "to": "Other", "export": false})).unwrap_err();
        assert!(err.to_string().contains("an article named `Other` exists"));
        assert_eq!(s.execute("article.list", &json!({})).unwrap(), before);
        assert_eq!(s.doc().unwrap().history.undo.len(), undo);

        let err = s.execute("article.options", &json!({"name": "Missing", "to": "Other"})).unwrap_err();
        assert!(err.to_string().contains("no article `Missing`"));
        assert_eq!(s.execute("article.list", &json!({})).unwrap(), before);

        s.execute("article.options", &json!({"name": "Main", "to": "Main", "export": false})).unwrap();
        let same_name = s.execute("article.list", &json!({})).unwrap();
        assert_eq!(same_name[0]["name"], "Main");
        assert_eq!(same_name[0]["export"], false);
        s.execute("edit.undo", &json!({})).unwrap();
        assert_eq!(s.execute("article.list", &json!({})).unwrap(), before);

        s.execute("article.options", &json!({"name": "Main", "to": "Renamed"})).unwrap();
        assert_eq!(s.execute("article.list", &json!({})).unwrap()[0]["name"], "Renamed");
        assert_eq!(s.doc().unwrap().history.undo.len(), undo + 1);
        s.execute("edit.undo", &json!({})).unwrap();
        assert_eq!(s.execute("article.list", &json!({})).unwrap(), before);
    }
}
