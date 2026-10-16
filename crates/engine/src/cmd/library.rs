//! Object Library: a file of reusable objects (each a snippet: items with their stories, styles,
//! swatches and images). New, open, add the selection, place, remove; saved as `.dclib` JSON.

use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use super::{CommandSpec, always, bad, cmd, has_doc, has_selection, ok, str_param};
use crate::{Result, Session};

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LibraryItem {
    pub name: String,
    pub description: String,
    /// A `.designcraft` snippet, base64.
    pub snippet: String,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Library {
    #[serde(skip)]
    pub path: Option<String>,
    pub items: Vec<LibraryItem>,
}

impl Library {
    fn save(&self) -> Result<()> {
        #[cfg(not(target_arch = "wasm32"))]
        if let Some(p) = &self.path {
            let bytes = serde_json::to_vec_pretty(self).map_err(|e| crate::EngineError::Other(e.to_string()))?;
            crate::io::write_file(p, &bytes).map_err(crate::EngineError::Other)?;
        }
        Ok(())
    }
}

fn has_library(s: &Session) -> std::result::Result<(), String> {
    if s.library.is_some() { Ok(()) } else { Err("no library open".into()) }
}

fn no_library() -> crate::EngineError {
    bad("library", "no library open")
}

pub fn specs() -> Vec<CommandSpec> {
    vec![
        cmd!(noundo "library.new", "New Library", ["File", "New"], None, "{path?} — an empty Object Library (saved to `path` when given)", always, |s, p| {
            let lib = Library { path: str_param(p, "path").map(str::to_string), items: vec![] };
            lib.save()?;
            s.library = Some(lib);
            ok()
        }),
        cmd!(noundo "library.open", "Open Library", [], None, "{path | json} → {items}", always, |s, p| {
            let (text, path) = match (str_param(p, "json"), str_param(p, "path")) {
                (Some(j), _) => (j.to_string(), None),
                (None, Some(path)) => {
                    #[cfg(not(target_arch = "wasm32"))]
                    let t = crate::io::read_to_string(path).map_err(crate::EngineError::Other)?;
                    #[cfg(target_arch = "wasm32")]
                    let t = String::new();
                    (t, Some(path.to_string()))
                }
                _ => return Err(bad("library.open", "`path` or `json` required")),
            };
            let mut lib: Library = serde_json::from_str(&text).map_err(|e| bad("library.open", format!("not a library: {e}")))?;
            lib.path = path;
            let n = lib.items.len();
            s.library = Some(lib);
            Ok(json!({"items": n}))
        }),
        cmd!(query "library.list", "Library Items", [], None, "{} → [{index, name, description}]", has_library, |s, _| {
            let lib = s.library.as_ref().ok_or_else(no_library)?;
            Ok(Value::Array(lib.items.iter().enumerate().map(|(i, it)| json!({"index": i, "name": it.name, "description": it.description})).collect()))
        }),
        cmd!(query "library.json", "Library Contents", [], None, "{} → the library as JSON text (to save where there's no file system)", has_library, |s, _| {
            Ok(json!({"json": serde_json::to_string(s.library.as_ref().ok_or_else(no_library)?).unwrap_or_default()}))
        }),
        cmd!(noundo "library.add", "Add Item", [], None, "{name?, description?} — the selection → {index}", has_selection, |s, p| {
            has_library(s).map_err(|e| bad("library.add", e))?;
            let snip = super::file::snippet_bytes(s)?;
            let (name, description) = {
                let st = s.doc()?;
                let first = st.selection.items.first().and_then(|id| st.doc.item(*id));
                let kind = first.map(|it| if !it.label.is_empty() { it.label.clone() } else { it.default_label().trim_matches(|c| c == '<' || c == '>').to_string() }).unwrap_or_default();
                let n = st.selection.items.len();
                (str_param(p, "name").map(str::to_string).unwrap_or_else(|| if n > 1 { format!("{n} objects") } else { kind.clone() }), str_param(p, "description").unwrap_or("").to_string())
            };
            let mut lib = s.library.as_ref().ok_or_else(no_library)?.clone();
            let index = lib.items.len();
            lib.items.push(LibraryItem { name, description, snippet: super::file::base64_encode(&snip) });
            lib.save()?;
            s.library = Some(lib);
            Ok(json!({"index": index}))
        }),
        cmd!(
            "library.place",
            "Place Item",
            [],
            None,
            "{index, spread?, x?, y?} — at its original position unless x/y (top-left) are given",
            has_doc,
            |s, p| {
                let lib = s.library.as_ref().ok_or_else(|| bad("library.place", "no library open"))?;
                let i = p.get("index").and_then(Value::as_u64).ok_or_else(|| bad("library.place", "`index` required"))? as usize;
                let it = lib.items.get(i).ok_or_else(|| bad("library.place", format!("no item {i}")))?;
                let q = super::with_param(p, "base64", json!(it.snippet));
                super::file::snippet_place(s, &q)
            }
        ),
        cmd!(noundo "library.remove", "Delete Item", [], None, "{index}", has_library, |s, p| {
            let mut lib = s.library.as_ref().ok_or_else(no_library)?.clone();
            let i = p.get("index").and_then(Value::as_u64).ok_or_else(|| bad("library.remove", "`index` required"))? as usize;
            if i >= lib.items.len() {
                return Err(bad("library.remove", format!("no item {i}")));
            }
            lib.items.remove(i);
            lib.save()?;
            s.library = Some(lib);
            ok()
        }),
        cmd!(noundo "conveyor.collect", "Collect", [], None, "{ids? (default: the selection)} — each object onto the Content Collector conveyor → {count}", has_selection, |s, p| {
            let ids = super::targets(s, p)?;
            let keep = s.active().map(|d| d.selection.clone());
            for id in ids {
                let name = s.doc()?.doc.item(id).map(|it| it.default_label().trim_matches(|c| c == '<' || c == '>').to_string()).unwrap_or_default();
                if let Ok(st) = s.doc_mut() {
                    st.selection = designcraft_doc::Selection::items(vec![id]);
                }
                let snip = super::file::snippet_bytes(s)?;
                s.conveyor.push((name, snip));
            }
            if let (Some(k), Ok(st)) = (keep, s.doc_mut()) {
                st.selection = k;
            }
            Ok(json!({"count": s.conveyor.len()}))
        }),
        cmd!(
            "conveyor.place",
            "Place",
            [],
            None,
            "{index? (0), spread?, x?, y?, keep?: bool (stay on the conveyor)} — the Content Placer: the collected object at x/y (top-left)",
            has_doc,
            |s, p| {
                let i = p.get("index").and_then(Value::as_u64).unwrap_or(0) as usize;
                let (_, snip) = s.conveyor.get(i).cloned().ok_or_else(|| bad("conveyor.place", "the conveyor is empty"))?;
                let q = super::with_param(p, "base64", json!(super::file::base64_encode(&snip)));
                let r = super::file::snippet_place(s, &q)?;
                if !p.get("keep").and_then(Value::as_bool).unwrap_or(false) {
                    s.conveyor.remove(i);
                }
                Ok(r)
            }
        ),
        cmd!(query "conveyor.list", "Conveyor", [], None, "{} → [name]", always, |s, _| Ok(json!(s.conveyor.iter().map(|c| c.0.clone()).collect::<Vec<_>>()))),
        cmd!(noundo "conveyor.clear", "Clear Conveyor", [], None, "{}", always, |s, _| {
            s.conveyor.clear();
            ok()
        }),
        cmd!(noundo "library.close", "Close Library", [], None, "{}", has_library, |s, _| {
            s.library = None;
            ok()
        }),
    ]
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use crate::Session;

    #[test]
    fn library_round_trip() {
        let dir = std::env::temp_dir().join(format!("dc-lib-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("brand.dclib").to_string_lossy().to_string();
        let mut s = Session::new();
        s.execute("file.new", &json!({})).unwrap();
        s.execute("library.new", &json!({"path": path})).unwrap();
        let r = s.execute("frame.create", &json!({"rect": [72, 72, 172, 122], "content": "text", "text": "Logo", "caret": false})).unwrap();
        s.execute("library.add", &json!({"name": "Logo block"})).unwrap();
        // A fresh session opens the saved library and places the item into another document.
        let mut t = Session::new();
        t.execute("file.new", &json!({})).unwrap();
        t.execute("library.open", &json!({"path": path})).unwrap();
        assert_eq!(t.execute("library.list", &json!({})).unwrap()[0]["name"], "Logo block");
        let placed = t.execute("library.place", &json!({"index": 0, "x": 300, "y": 300})).unwrap();
        let id = designcraft_doc::ItemId(placed["ids"][0].as_u64().unwrap());
        let d = &t.doc().unwrap().doc;
        let it = d.item(id).unwrap();
        assert_eq!((it.bounds().x0, it.bounds().y0), (300.0, 300.0));
        let sid = it.text_frame().unwrap().story;
        assert_eq!(d.stories[&sid].text, "Logo");
        let _ = r;
        t.execute("library.remove", &json!({"index": 0})).unwrap();
        assert!(t.execute("library.list", &json!({})).unwrap().as_array().unwrap().is_empty());
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn content_collector_and_placer() {
        let mut s = Session::new();
        s.execute("file.new", &json!({"pages": 2})).unwrap();
        let a = s.execute("frame.create", &json!({"rect": [72, 72, 172, 122], "content": "text", "text": "Reused", "caret": false})).unwrap()["id"]
            .clone();
        let b = s.execute("frame.create", &json!({"rect": [200, 72, 260, 122]})).unwrap()["id"].clone();
        assert_eq!(s.execute("conveyor.collect", &json!({"ids": [a, b]})).unwrap()["count"], 2);
        let r = s.execute("conveyor.place", &json!({"spread": 1, "x": 300, "y": 300})).unwrap();
        let id = designcraft_doc::ItemId(r["ids"][0].as_u64().unwrap());
        let d = &s.doc().unwrap().doc;
        assert_eq!(d.item(id).unwrap().bounds().x0, 300.0);
        assert!(d.find(id).is_some_and(|l| l.spread == designcraft_doc::SpreadRef::Doc(1)));
        assert_eq!(s.execute("conveyor.list", &json!({})).unwrap().as_array().unwrap().len(), 1, "placed objects leave the conveyor");
        s.execute("conveyor.place", &json!({"keep": true})).unwrap();
        assert_eq!(s.execute("conveyor.list", &json!({})).unwrap().as_array().unwrap().len(), 1);
    }
}
