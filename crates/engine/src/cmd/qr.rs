//! Object › Generate QR Code: a vector QR code (a compound path) as a new object, inside the
//! selected frame, or replacing a selected code (Edit QR Code).

use designcraft_doc::{Content, Fill, Item, ItemId, Selection, Shape};
use designcraft_geom::Rect;
use serde_json::{Value, json};

use super::{CommandSpec, bad, cmd, has_doc, rect_param, spread_param, str_param};
use crate::{Result, Session};

/// Alt text prefix that marks a generated code (and carries its content for editing).
const MARK: &str = "QR code: ";

pub fn specs() -> Vec<CommandSpec> {
    vec![cmd!(
        "object.qrCode",
        "Generate QR Code…",
        ["Object"],
        None,
        "{type?: url|text|sms|email|vcard (default url), content (url/text), field? (data-merge column to bind as a qr placeholder; the column kind stays as stored), source? (source id), number?, message?, to?, subject?, body?, name?, phone?, email?, org?, url?, color? (swatch, [Black]), rect? (new object; else into the selected frame, or re-encode a selected code), spread?} → {id, content}",
        has_doc,
        qr_code
    )]
}

/// The string a scanner reads, per InDesign's QR types.
fn encode(p: &Value) -> std::result::Result<String, String> {
    let g = |k: &str| str_param(p, k).unwrap_or("").trim().to_string();
    let esc = |s: String| s.replace('\\', r"\\").replace(';', r"\;").replace(':', r"\:").replace(',', r"\,");
    let out = match str_param(p, "type").unwrap_or("url") {
        "url" | "text" => g("content"),
        "sms" => format!("SMSTO:{}:{}", g("number"), g("message")),
        "email" => format!("MATMSG:TO:{};SUB:{};BODY:{};;", esc(g("to")), esc(g("subject")), esc(g("body"))),
        "vcard" => {
            let mut v = format!("MECARD:N:{};", esc(g("name")));
            for (k, tag) in [("org", "ORG"), ("phone", "TEL"), ("email", "EMAIL"), ("url", "URL")] {
                if !g(k).is_empty() {
                    v.push_str(&format!("{tag}:{};", esc(g(k))));
                }
            }
            v + ";"
        }
        t => return Err(format!("unknown type `{t}` (url, text, sms, email, vcard)")),
    };
    if out.is_empty() { Err("nothing to encode".into()) } else { Ok(out) }
}

fn qr_item(id: ItemId, layer: designcraft_doc::LayerId, text: &str, r: Rect, color: &str) -> std::result::Result<Item, String> {
    let path = designcraft_geom::qr::qr_path(text, r).ok_or("too much content for a QR code")?;
    let mut it = Item::new(id, layer, Shape::Path, path);
    it.fill = Fill::swatch(color);
    it.alt_text = format!("{MARK}{text}");
    Ok(it)
}

fn empty_frame(id: ItemId, layer: designcraft_doc::LayerId, r: Rect) -> Item {
    let mut it = Item::new(id, layer, Shape::Rectangle, designcraft_geom::shapes::rectangle(r));
    it.content = Content::Unassigned;
    it
}

fn qr_code(s: &mut Session, p: &Value) -> Result<Value> {
    const ID: &str = "object.qrCode";
    let field = str_param(p, "field").map(str::to_string);
    let source = p.get("source").and_then(Value::as_u64);
    let text = match encode(p) {
        Ok(text) => Some(text),
        Err(e) if field.is_some() && e == "nothing to encode" => None,
        Err(e) => return Err(bad(ID, e)),
    };
    let color = str_param(p, "color").unwrap_or(designcraft_color::swatch::BLACK).to_string();
    let rect = rect_param(p, "rect");
    let sr = spread_param(p, "spread");
    let st = s.doc()?;
    let lid = st.active_layer;
    let sel = st.selection.items.clone();
    s.edit(|d, selection| {
        if text.is_some() && d.swatch(&color).is_none() {
            return Err(bad(ID, format!("no swatch `{color}`")));
        }
        let target = if rect.is_some() { None } else { sel.first().copied() };
        let id = match (text.as_deref(), rect, target) {
            (Some(text), Some(r), _) => {
                let id = ItemId(d.alloc());
                d.insert_item(sr, qr_item(id, lid, text, r, &color).map_err(|e| bad(ID, e))?, None)?;
                id
            }
            (None, Some(r), _) => {
                let id = ItemId(d.alloc());
                d.insert_item(sr, empty_frame(id, lid, r), None)?;
                id
            }
            (Some(text), None, Some(t)) => {
                let it = d.item(t).ok_or_else(|| bad(ID, "no such object"))?.clone();
                if it.alt_text.starts_with(MARK) {
                    // Edit QR Code: same place and size.
                    let b = it.path.bounds().unwrap_or(Rect::ZERO);
                    let side = b.width().max(b.height());
                    let n = designcraft_geom::qr::qr_size(it.alt_text.trim_start_matches(MARK)).unwrap_or(21);
                    let quiet = side / (n + 4) as f64 * 2.0;
                    let r = Rect::from_center_size(b.center(), (side + 2.0 * quiet, side + 2.0 * quiet));
                    let fresh = qr_item(t, it.layer, text, r, &color).map_err(|e| bad(ID, e))?;
                    let it = d.item_mut(t).ok_or(designcraft_doc::DocError::NoItem(t))?;
                    it.path = fresh.path;
                    it.fill = fresh.fill;
                    it.alt_text = fresh.alt_text;
                    t
                } else {
                    // Into the frame: the code fills it, as content.
                    if !matches!(it.content, Content::Unassigned | Content::Group { .. }) || it.shape == Shape::Group {
                        return Err(bad(ID, "select an empty frame, a QR code, or give `rect`"));
                    }
                    let id = ItemId(d.alloc());
                    let code = qr_item(id, it.layer, text, it.inner_bounds(), &color).map_err(|e| bad(ID, e))?;
                    let f = d.item_mut(t).ok_or(designcraft_doc::DocError::NoItem(t))?;
                    f.content = Content::Group { items: vec![std::sync::Arc::new(code)] };
                    t
                }
            }
            (None, None, Some(t)) => {
                let it = d.item(t).ok_or_else(|| bad(ID, "no such object"))?;
                if !matches!(it.content, Content::Unassigned | Content::Group { .. }) || it.shape == Shape::Group {
                    return Err(bad(ID, "select an empty frame, a QR code, or give `rect`"));
                }
                t
            }
            (None, None, None) | (Some(_), None, None) => return Err(bad(ID, "give `rect` or select a frame")),
        };
        if let Some(field) = field.as_deref() {
            super::datamerge::bind_qr_field(d, id, field, source).map_err(|e| bad(ID, e))?;
        }
        *selection = Selection::items(vec![id]);
        Ok(json!({"id": id.0, "content": text.unwrap_or_default()}))
    })
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use crate::Session;

    #[test]
    fn generate_and_edit_qr_codes() {
        let mut s = Session::new();
        s.execute("file.new", &json!({})).unwrap();
        let r = s.execute("object.qrCode", &json!({"content": "https://example.com", "rect": [72, 72, 216, 216]})).unwrap();
        let id = designcraft_doc::ItemId(r["id"].as_u64().unwrap());
        let item = |s: &Session| s.doc().unwrap().doc.item(id).unwrap().clone();
        let a = item(&s);
        assert!(a.path.subpaths.len() > 20 && a.alt_text.contains("example.com"));
        let b0 = a.bounds();
        assert!(b0.x0 >= 72.0 && b0.x1 <= 216.0);
        // Edit QR Code: new content, same place.
        let r = s.execute("object.qrCode", &json!({"type": "email", "to": "a@b.c", "subject": "Hi"})).unwrap();
        assert_eq!(r["content"], "MATMSG:TO:a@b.c;SUB:Hi;BODY:;;");
        let b1 = item(&s).bounds();
        assert!((b1.center() - b0.center()).hypot() < 1.0, "{b0:?} → {b1:?}");
        assert!((b1.width() - b0.width()).abs() < 4.0);
        // Into an empty frame.
        let f = s.execute("frame.create", &json!({"rect": [300, 300, 400, 400], "content": "unassigned"})).unwrap()["id"].as_u64().unwrap();
        s.execute("object.qrCode", &json!({"type": "vcard", "name": "Ada", "phone": "+44 1"})).unwrap();
        let fr = s.doc().unwrap().doc.item(designcraft_doc::ItemId(f)).unwrap().clone();
        assert!(fr.has_nested_items());
        assert!(s.execute("object.qrCode", &json!({"type": "fax", "content": "x", "rect": [0, 0, 9, 9]})).is_err());
    }
}
