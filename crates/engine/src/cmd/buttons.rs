//! Window › Interactive › Buttons and Forms: objects that act when clicked in an interactive PDF
//! (go to a page, the next / previous / first / last page, or a URL).

use designcraft_doc::ButtonAction;
use serde_json::{Value, json};

use super::{CommandSpec, bad, cmd, has_selection, targets};
use crate::Result;

fn parse(p: &Value) -> Result<Option<ButtonAction>> {
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
        "url" => {
            let url = p.get("url").and_then(Value::as_str).ok_or_else(|| bad("button.set", "`url` required"))?;
            if !designcraft_doc::is_safe_link_url(url) {
                return Err(bad("button.set", "`url` must start with http://, https:// or mailto:"));
            }
            ButtonAction::GoToUrl { url: url.to_string() }
        }
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
            "{ids?, action: page|firstPage|lastPage|nextPage|previousPage|url|none, page?, url?} — the selected objects act on release in interactive PDF",
            has_selection,
            |s, p| {
                let ids = targets(s, p)?;
                let action = parse(p)?;
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

    #[test]
    fn buttons_refuse_non_web_schemes() {
        let mut s = Session::new();
        s.execute("file.new", &json!({"pages": 1})).unwrap();
        let id = s.execute("frame.create", &json!({"rect": [72, 72, 200, 120]})).unwrap()["id"].clone();
        for bad in ["javascript:alert(1)", "file:///etc/passwd", "\\\\attacker\\share"] {
            let e = s.execute("button.set", &json!({"ids": [id.clone()], "action": "url", "url": bad})).expect_err("refused").to_string();
            assert!(e.contains("http://"), "{bad}: {e}");
        }
        s.execute("button.set", &json!({"ids": [id], "action": "url", "url": "https://example.com"})).unwrap();
    }
}
