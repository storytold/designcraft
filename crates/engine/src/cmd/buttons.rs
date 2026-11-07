//! Window › Interactive › Buttons and Forms: objects that act when clicked in an interactive PDF
//! (go to a page, the next / previous / first / last page, a URL, a text anchor, or a state of a
//! multi-state object).

use designcraft_doc::{ButtonAction, ItemId};
use serde_json::{Value, json};

use super::{CommandSpec, bad, cmd, has_selection, targets};
use crate::{Result, Session};

/// The multi-state object a state action targets, with its state names: `object` when given
/// (it must name a multi-state object), else the document's only one.
fn state_object(s: &Session, p: &Value) -> Result<(ItemId, Vec<String>)> {
    let d = &s.doc()?.doc;
    let mso = |id: ItemId| d.item(id).filter(|it| !it.states.is_empty()).map(|it| (id, it.states.clone()));
    if let Some(v) = p.get("object") {
        let id = v.as_u64().map(ItemId).ok_or_else(|| bad("button.set", "`object` must be an object id (number)"))?;
        return mso(id).ok_or_else(|| bad("button.set", format!("{id} is not a multi-state object")));
    }
    let mut all = d.all_items().into_iter().filter_map(mso);
    match (all.next(), all.next()) {
        (Some(one), None) => Ok(one),
        (None, _) => Err(bad("button.set", "no multi-state object in the document (Object States: Convert Selection to Multi-State Object)")),
        _ => Err(bad("button.set", "`object` required: the document has several multi-state objects")),
    }
}

/// `wrap`: whether Next / Previous State go round the ends (off unless asked).
fn wrap(p: &Value) -> bool {
    p.get("wrap").and_then(Value::as_bool).unwrap_or(false)
}

/// The text anchor `anchor` names, checked against every story (`anchor.list` names them).
fn anchor(s: &Session, p: &Value) -> Result<u64> {
    let id = p.get("anchor").and_then(Value::as_u64).ok_or_else(|| bad("button.set", "`anchor` (text anchor id) required"))?;
    if !s.doc()?.doc.stories.values().any(|st| st.anchors.iter().any(|a| a.id == id)) {
        return Err(bad("button.set", format!("no text anchor {id} (anchor.list names them)")));
    }
    Ok(id)
}

/// The action `button.set`'s params describe; `None` for `action: none`.
fn parse(s: &Session, p: &Value) -> Result<Option<ButtonAction>> {
    let a = p.get("action").and_then(Value::as_str).unwrap_or("nextPage");
    Ok(Some(match a {
        "none" => return Ok(None),
        "page" => ButtonAction::GoToPage {
            page: p.get("page").and_then(Value::as_u64).ok_or_else(|| bad("button.set", "`page` (0-based) required"))? as usize,
        },
        "firstPage" => ButtonAction::GoToFirstPage,
        "lastPage" => ButtonAction::GoToLastPage,
        "nextPage" => ButtonAction::GoToNextPage,
        "previousPage" => ButtonAction::GoToPreviousPage,
        "url" => ButtonAction::GoToUrl { url: p.get("url").and_then(Value::as_str).ok_or_else(|| bad("button.set", "`url` required"))?.to_string() },
        "destination" => ButtonAction::GoToDestination { anchor: anchor(s, p)? },
        "state" => {
            let (object, states) = state_object(s, p)?;
            let state = match (p.get("state").and_then(Value::as_u64), p.get("state").and_then(Value::as_str)) {
                (Some(i), _) => i as usize,
                (None, Some(n)) => states.iter().position(|x| x == n).ok_or_else(|| bad("button.set", format!("no state `{n}`")))?,
                _ => return Err(bad("button.set", "`state` (index or name) required")),
            };
            if state >= states.len() {
                return Err(bad("button.set", format!("no state {state}")));
            }
            ButtonAction::GoToState { object, state }
        }
        "nextState" => ButtonAction::GoToNextState { object: state_object(s, p)?.0, wrap: wrap(p) },
        "previousState" => ButtonAction::GoToPreviousState { object: state_object(s, p)?.0, wrap: wrap(p) },
        o => return Err(bad("button.set", format!("unknown action `{o}`"))),
    }))
}

pub fn specs() -> Vec<CommandSpec> {
    vec![
        cmd!(
            "button.set",
            "Convert to Button",
            ["Object", "Interactive"],
            None,
            "{ids?, action: page|firstPage|lastPage|nextPage|previousPage|url|destination|state|nextState|previousState|none, page? (0-based), url?, anchor? (text anchor id), object? (multi-state object; default: the document's only one), state? (index or name), wrap?} — the selected objects act on release in interactive PDF (page, URL and destination actions export as PDF links; state actions are kept in the document for interactive export)",
            has_selection,
            |s, p| {
                let ids = targets(s, p)?;
                let action = parse(s, p)?;
                s.edit(|d, _| {
                    for id in &ids {
                        if let Some(it) = d.item_mut(*id) {
                            it.button = action.clone();
                        }
                    }
                    Ok(json!({"buttons": ids.len()}))
                })
            }
        ),
        cmd!("button.clear", "Convert to Object", ["Object", "Interactive"], None, "{ids?}", has_selection, |s, p| {
            s.execute("button.set", &super::with_param(p, "action", json!("none")))
        }),
        cmd!(
            "form.set",
            "Convert to Form Field",
            ["Object", "Interactive"],
            None,
            "{ids?, kind: textField|checkBox|comboBox|listBox|signature, name?, value?, options?: [string], multiline?, required?, fontSize?} — interactive PDF form fields",
            has_selection,
            |s, p| {
                let ids = targets(s, p)?;
                let kind: designcraft_doc::FieldKind = serde_json::from_value(p.get("kind").cloned().unwrap_or(json!("textField")))
                    .map_err(|_| bad("form.set", "`kind`: textField|checkBox|comboBox|listBox|signature"))?;
                let p = p.clone();
                s.edit(|d, _| {
                    let mut taken: Vec<String> =
                        d.all_items().into_iter().filter_map(|i| d.item(i)?.form_field.as_ref().map(|f| f.name.clone())).collect();
                    for (k, id) in ids.iter().enumerate() {
                        let Some(it) = d.item_mut(*id) else { continue };
                        let mut f = it.form_field.clone().unwrap_or(designcraft_doc::FormField {
                            kind,
                            name: String::new(),
                            value: String::new(),
                            options: vec![],
                            multiline: false,
                            required: false,
                            font_size: 0.0,
                        });
                        f.kind = kind;
                        if let Some(v) = p.get("name").and_then(Value::as_str) {
                            f.name = if ids.len() > 1 { format!("{v}{}", k + 1) } else { v.to_string() };
                        }
                        if f.name.is_empty() {
                            // A unique default name.
                            let base = match kind {
                                designcraft_doc::FieldKind::TextField => "Text Field",
                                designcraft_doc::FieldKind::CheckBox => "Check Box",
                                designcraft_doc::FieldKind::ComboBox => "Combo Box",
                                designcraft_doc::FieldKind::ListBox => "List Box",
                                designcraft_doc::FieldKind::Signature => "Signature",
                            };
                            let mut n = 1;
                            while taken.contains(&format!("{base} {n}")) {
                                n += 1;
                            }
                            f.name = format!("{base} {n}");
                        }
                        taken.push(f.name.clone());
                        if let Some(v) = p.get("value").and_then(Value::as_str) {
                            f.value = v.to_string();
                        }
                        if let Some(v) = p.get("options").and_then(Value::as_array) {
                            f.options = v.iter().filter_map(Value::as_str).map(str::to_string).collect();
                        }
                        if let Some(v) = p.get("multiline").and_then(Value::as_bool) {
                            f.multiline = v;
                        }
                        if let Some(v) = p.get("required").and_then(Value::as_bool) {
                            f.required = v;
                        }
                        if let Some(v) = p.get("fontSize").and_then(Value::as_f64) {
                            f.font_size = v.max(0.0);
                        }
                        it.form_field = Some(f);
                    }
                    Ok(json!({"fields": ids.len()}))
                })
            }
        ),
        cmd!("form.clear", "Convert to Object", [], None, "{ids?} — no longer a form field", has_selection, |s, p| {
            let ids = targets(s, p)?;
            s.edit(|d, _| {
                for id in &ids {
                    if let Some(it) = d.item_mut(*id) {
                        it.form_field = None;
                    }
                }
                Ok(Value::Null)
            })
        }),
        cmd!(query "form.list", "Form Fields", [], None, "{} → [{id, kind, name, value, options}]", super::has_doc, |s, _| {
            let d = &s.doc()?.doc;
            Ok(Value::Array(
                d.all_items().into_iter().filter_map(|id| { let it = d.item(id)?; let f = it.form_field.as_ref()?; Some(json!({"id": id.0, "kind": f.kind, "name": f.name, "value": f.value, "options": f.options})) }).collect(),
            ))
        }),
        cmd!(query "button.list", "Buttons", [], None, "{} → [{id, name, action}]", super::has_doc, |s, _| {
            let d = &s.doc()?.doc;
            let mut out = Vec::new();
            for it in d.all_items().into_iter().filter_map(|id| d.item(id)) {
                if let Some(b) = &it.button {
                    out.push(json!({"id": it.id.0, "name": it.name, "action": b}));
                }
            }
            Ok(Value::Array(out))
        }),
    ]
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use crate::Session;

    /// Page and URL buttons export as link annotations; form fields as AcroForm widgets.
    #[test]
    fn buttons_become_pdf_links() {
        let mut s = Session::new();
        s.execute("file.new", &json!({"pages": 3})).unwrap();
        let id = s.execute("frame.create", &json!({"rect": [72, 72, 200, 120]})).unwrap()["id"].clone();
        s.execute("button.set", &json!({"ids": [id], "action": "lastPage"})).unwrap();
        assert_eq!(s.execute("button.list", &json!({})).unwrap()[0]["action"]["kind"], "goToLastPage");
        let links = |s: &mut Session| {
            let r = s.execute("file.exportPdf", &json!({})).unwrap();
            let bytes = super::super::file::base64_decode(r["base64"].as_str().unwrap());
            String::from_utf8_lossy(&bytes).matches("/Link").count()
        };
        let with = links(&mut s);
        s.execute("button.clear", &json!({"ids": [id]})).unwrap();
        assert!(with > links(&mut s), "the button exports a link annotation");
        // Form fields: a text field and a check box become AcroForm widgets.
        let t = s.execute("frame.create", &json!({"rect": [72, 300, 300, 330]})).unwrap()["id"].clone();
        let c = s.execute("frame.create", &json!({"rect": [72, 350, 90, 368]})).unwrap()["id"].clone();
        s.execute("form.set", &json!({"ids": [t], "kind": "textField", "name": "Your name", "value": "Ada"})).unwrap();
        s.execute("form.set", &json!({"ids": [c], "kind": "checkBox", "value": "On"})).unwrap();
        assert_eq!(s.execute("form.list", &json!({})).unwrap().as_array().unwrap().len(), 2);
        let r = s.execute("file.exportPdf", &json!({})).unwrap();
        let bytes = super::super::file::base64_decode(r["base64"].as_str().unwrap());
        let text = String::from_utf8_lossy(&bytes);
        assert!(text.contains("/AcroForm<</Fields[") && text.contains("/FT/Tx") && text.contains("(Your name)") && text.contains("/AS/On"), "fields");
        assert_eq!(designcraft_render::pdf_page_count(&bytes), Some(3), "the update still reads");
        assert!(s.execute("button.list", &json!({})).unwrap().as_array().unwrap().is_empty());
    }

    /// #1798: buttons reach a multi-state object's states and a text anchor, not only pages and URLs.
    #[test]
    fn buttons_go_to_states_and_destinations() {
        let mut s = Session::new();
        s.execute("file.new", &json!({"pages": 2, "facingPages": false})).unwrap();
        let button = s.execute("frame.create", &json!({"rect": [72, 72, 200, 120]})).unwrap()["id"].clone();
        // Without a multi-state object there is nothing to go to.
        let e = s.execute("button.set", &json!({"ids": [button], "action": "nextState"})).expect_err("no multi-state object").to_string();
        assert!(e.contains("no multi-state object"), "{e}");

        let a = s.execute("frame.create", &json!({"rect": [100, 200, 200, 300]})).unwrap()["id"].clone();
        let b = s.execute("frame.create", &json!({"rect": [100, 200, 200, 300]})).unwrap()["id"].clone();
        let mso = s.execute("states.create", &json!({"ids": [a, b]})).unwrap()["id"].clone();

        // The document's only multi-state object is the default target.
        s.execute("button.set", &json!({"ids": [button], "action": "nextState", "wrap": true})).unwrap();
        let act = s.execute("button.list", &json!({})).unwrap()[0]["action"].clone();
        assert_eq!(act, json!({"kind": "goToNextState", "object": mso, "wrap": true}));
        s.execute("button.set", &json!({"ids": [button], "action": "previousState"})).unwrap();
        assert_eq!(s.execute("button.list", &json!({})).unwrap()[0]["action"]["wrap"], false, "wrap defaults to off");
        // A state by name or by index; unknown states and objects are refused, and so is an
        // `object` that isn't an id (it doesn't fall back to the default).
        s.execute("button.set", &json!({"ids": [button], "action": "state", "object": mso, "state": "State 2"})).unwrap();
        assert_eq!(s.execute("button.list", &json!({})).unwrap()[0]["action"], json!({"kind": "goToState", "object": mso, "state": 1}));
        s.execute("button.set", &json!({"ids": [button], "action": "state", "state": 0})).unwrap();
        assert_eq!(s.execute("button.list", &json!({})).unwrap()[0]["action"]["state"], 0);
        assert!(s.execute("button.set", &json!({"ids": [button], "action": "state", "state": "State 9"})).is_err());
        assert!(s.execute("button.set", &json!({"ids": [button], "action": "state", "state": 2})).is_err());
        assert!(
            s.execute("button.set", &json!({"ids": [button], "action": "state", "object": button, "state": 0})).is_err(),
            "a plain frame is not a multi-state object"
        );
        assert!(s.execute("button.set", &json!({"ids": [button], "action": "state"})).is_err(), "`state` required");
        let e = s.execute("button.set", &json!({"ids": [button], "action": "nextState", "object": "42"})).expect_err("not an id").to_string();
        assert!(e.contains("`object` must be"), "{e}");
        // With two multi-state objects the target has to be named.
        let c = s.execute("frame.create", &json!({"rect": [300, 200, 400, 300]})).unwrap()["id"].clone();
        let d = s.execute("frame.create", &json!({"rect": [300, 200, 400, 300]})).unwrap()["id"].clone();
        s.execute("states.create", &json!({"ids": [c, d]})).unwrap();
        let e = s.execute("button.set", &json!({"ids": [button], "action": "nextState"})).expect_err("ambiguous").to_string();
        assert!(e.contains("`object` required"), "{e}");
        s.execute("button.set", &json!({"ids": [button], "action": "nextState", "object": mso})).unwrap();

        // Go To Destination: a text anchor's page becomes the link target in the PDF; an unknown
        // anchor is refused rather than saved and dropped at export.
        let text = s.execute("frame.create", &json!({"spread": 1, "rect": [72, 72, 400, 200], "content": "text", "text": "Appendix"})).unwrap();
        s.execute("text.select", &json!({"story": text["story"], "anchor": 0, "focus": 0})).unwrap();
        let anchor = s.execute("anchor.create", &json!({"name": "Appendix"})).unwrap()["id"].clone();
        assert!(s.execute("button.set", &json!({"ids": [button], "action": "destination"})).is_err(), "`anchor` required");
        let e =
            s.execute("button.set", &json!({"ids": [button], "action": "destination", "anchor": 999_999})).expect_err("no such anchor").to_string();
        assert!(e.contains("no text anchor"), "{e}");
        let links = |s: &mut Session| {
            let r = s.execute("file.exportPdf", &json!({})).unwrap();
            let bytes = super::super::file::base64_decode(r["base64"].as_str().unwrap());
            String::from_utf8_lossy(&bytes).matches("/Link").count()
        };
        s.execute("button.clear", &json!({"ids": [button]})).unwrap();
        let without = links(&mut s);
        s.execute("button.set", &json!({"ids": [button], "action": "destination", "anchor": anchor})).unwrap();
        assert_eq!(s.execute("button.list", &json!({})).unwrap()[0]["action"], json!({"kind": "goToDestination", "anchor": anchor}));
        assert!(links(&mut s) > without, "the destination button exports a link annotation");
    }
}
