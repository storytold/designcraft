//! Object › Generate QR Code: a vector QR code (a compound path) as a new object, inside the
//! selected frame, or replacing a selected code (Edit QR Code). `object.qrCodeRead` reads a
//! selected code back into the fields that made it, so the dialog can edit it.

use std::collections::BTreeMap;

use designcraft_doc::{Content, Fill, Item, ItemId, Placeholder, PlaceholderAnchor, PlaceholderRole, Selection, Shape};
use designcraft_geom::Rect;
use serde_json::{Value, json};

use super::{CommandSpec, bad, cmd, has_doc, rect_param, spread_param, str_param};
use crate::{Result, Session};

/// Alt text prefix that marks a generated code (and carries its content for editing).
const MARK: &str = "QR code: ";

pub fn specs() -> Vec<CommandSpec> {
    vec![
        cmd!(
            "object.qrCode",
            "Generate QR Code…",
            ["Object"],
            None,
            "{type?: url|text|tel|sms|email|vcard (default url), content (url/text), number? (tel/sms), message?, to?, subject?, body?, name?, org?, title?, phone? (work), mobile?, fax?, email?, url?, address? (vcard; any text param may name Data Merge fields as <<Field>>: the code then goes in a frame as a merge placeholder), color? (swatch, [Black]), rect? (new object; else into the selected frame, or re-encode a selected code), spread?} → {id, content}",
            has_doc,
            qr_code
        ),
        cmd!(
            query "object.qrCodeRead",
            "Read QR Code",
            [],
            None,
            "{id?} (default: the selected code, or the code in the selected frame) → {id, encoded, color, merge (from Data Merge fields), type, …the object.qrCode fields that encode it}",
            has_doc,
            qr_read
        ),
    ]
}

/// The vCard business-card fields: (param, property).
const VCARD: [(&str, &str); 9] = [
    ("org", "ORG"),
    ("title", "TITLE"),
    ("phone", "TEL;TYPE=WORK,VOICE"),
    ("mobile", "TEL;TYPE=CELL"),
    ("fax", "TEL;TYPE=WORK,FAX"),
    ("email", "EMAIL"),
    ("url", "URL"),
    ("address", "ADR;TYPE=WORK"),
    ("name", "FN"),
];

/// Escapes MECARD/MATMSG text (`\`, `;`, `:`, `,`).
fn esc_me(s: &str) -> String {
    s.replace('\\', r"\\").replace(';', r"\;").replace(':', r"\:").replace(',', r"\,")
}

/// Escapes vCard 3.0 text (`\`, `,`, `;`, new lines).
fn esc_vcard(s: &str) -> String {
    s.replace('\\', r"\\").replace(',', r"\,").replace(';', r"\;").replace("\r\n", r"\n").replace('\n', r"\n")
}

/// The string a scanner reads, per InDesign's and Affinity's QR types.
pub(crate) fn encode(p: &Value) -> std::result::Result<String, String> {
    let g = |k: &str| str_param(p, k).unwrap_or("").trim().to_string();
    let out = match str_param(p, "type").unwrap_or("url") {
        "url" | "text" => g("content"),
        "tel" if g("number").is_empty() => String::new(),
        "tel" => format!("TEL:{}", g("number")),
        "sms" => format!("SMSTO:{}:{}", g("number"), g("message")),
        "email" => format!("MATMSG:TO:{};SUB:{};BODY:{};;", esc_me(&g("to")), esc_me(&g("subject")), esc_me(&g("body"))),
        "vcard" => {
            // vCard 3.0, which iOS and Android read with labelled work, mobile and fax numbers.
            let name = g("name");
            let (given, family) = name.rsplit_once(' ').map_or(("", name.as_str()), |(a, b)| (a.trim(), b));
            let mut v = format!("BEGIN:VCARD\r\nVERSION:3.0\r\nN:{};{};;;\r\nFN:{}\r\n", esc_vcard(family), esc_vcard(given), esc_vcard(&name));
            for (k, prop) in VCARD.iter().filter(|(k, _)| *k != "name") {
                let val = g(k);
                if val.is_empty() {
                    continue;
                }
                // An address is one street line (the vCard's third ADR component).
                let val = if *k == "address" { format!(";;{};;;;", esc_vcard(&val)) } else { esc_vcard(&val) };
                v.push_str(&format!("{prop}:{val}\r\n"));
            }
            v + "END:VCARD"
        }
        t => return Err(format!("unknown type `{t}` (url, text, tel, sms, email, vcard)")),
    };
    if out.is_empty() { Err("nothing to encode".into()) } else { Ok(out) }
}

/// Splits `s` at each `sep` that isn't backslash-escaped, unescaping the parts (`\n` is a new
/// line).
fn split_escaped(s: &str, sep: char) -> Vec<String> {
    let mut parts = vec![String::new()];
    let mut chars = s.chars();
    while let Some(c) = chars.next() {
        let Some(cur) = parts.last_mut() else { break };
        match c {
            '\\' => match chars.next() {
                Some('n' | 'N') => cur.push('\n'),
                Some(e) => cur.push(e),
                None => {}
            },
            c if c == sep => parts.push(String::new()),
            c => cur.push(c),
        }
    }
    parts
}

/// The value after `key` in a MECARD-style `KEY:value;` list.
fn me_field(fields: &[String], key: &str) -> String {
    fields.iter().find_map(|f| f.strip_prefix(key)).unwrap_or("").to_string()
}

/// Reads an encoded string back into the [`encode`] fields that make it.
fn decode(text: &str) -> Value {
    if let Some(rest) = text.strip_prefix("TEL:") {
        return json!({"type": "tel", "number": rest});
    }
    if let Some(rest) = text.strip_prefix("SMSTO:") {
        let (number, message) = rest.split_once(':').unwrap_or((rest, ""));
        return json!({"type": "sms", "number": number, "message": message});
    }
    if let Some(rest) = text.strip_prefix("MATMSG:") {
        let f = split_escaped(rest, ';');
        return json!({"type": "email", "to": me_field(&f, "TO:"), "subject": me_field(&f, "SUB:"), "body": me_field(&f, "BODY:")});
    }
    if let Some(rest) = text.strip_prefix("MECARD:") {
        let f = split_escaped(rest, ';');
        return json!({"type": "vcard", "name": me_field(&f, "N:"), "org": me_field(&f, "ORG:"), "phone": me_field(&f, "TEL:"),
            "email": me_field(&f, "EMAIL:"), "url": me_field(&f, "URL:")});
    }
    if text.starts_with("BEGIN:VCARD") {
        let mut out = json!({"type": "vcard"});
        // Unfold continuation lines (a new line then a space or tab).
        let unfolded = text.replace("\r\n", "\n").replace("\n ", "").replace("\n\t", "");
        for line in unfolded.lines() {
            let Some((prop, val)) = line.split_once(':') else { continue };
            let upper = prop.to_ascii_uppercase();
            let key = VCARD.iter().find(|(_, p)| p.eq_ignore_ascii_case(&upper)).map(|(k, _)| *k).or_else(|| {
                // Other writers' TEL types: a cell, a fax, or any other number.
                let name = upper.split(';').next().unwrap_or("");
                match name {
                    "TEL" if upper.contains("CELL") => Some("mobile"),
                    "TEL" if upper.contains("FAX") => Some("fax"),
                    "TEL" => Some("phone"),
                    "ADR" => Some("address"),
                    "ORG" | "TITLE" | "EMAIL" | "URL" | "FN" => VCARD.iter().find(|(_, p)| p.starts_with(name)).map(|(k, _)| *k),
                    _ => None,
                }
            });
            let Some(key) = key else { continue };
            if out.get(key).is_some() {
                continue;
            }
            let val = if key == "address" {
                split_escaped(val, ';').into_iter().filter(|s| !s.is_empty()).collect::<Vec<_>>().join(", ")
            } else {
                split_escaped(val, ';').join(" ")
            };
            out[key] = json!(val.trim());
        }
        return out;
    }
    let looks_like_url = text.contains("://") || text.starts_with("www.") || text.starts_with("mailto:");
    json!({"type": if looks_like_url { "url" } else { "text" }, "content": text})
}

fn qr_item(id: ItemId, layer: designcraft_doc::LayerId, text: &str, r: Rect, color: &str) -> std::result::Result<Item, String> {
    let path = designcraft_geom::qr::qr_path(text, r).ok_or("too much content for a QR code")?;
    let mut it = Item::new(id, layer, Shape::Path, path);
    it.fill = Fill::swatch(color);
    it.alt_text = format!("{MARK}{text}");
    Ok(it)
}

/// The `object.qrCode` params a code's content comes from (a Data Merge template keeps these).
const PARAMS: [&str; 17] = [
    "type", "content", "number", "message", "to", "subject", "body", "name", "org", "title", "phone", "mobile", "fax", "email", "url", "address",
    "color",
];

/// The params `kind` encodes (a dialog may pass the other types' fields too).
fn type_fields(kind: &str) -> &'static [&'static str] {
    match kind {
        "tel" => &["number"],
        "sms" => &["number", "message"],
        "email" => &["to", "subject", "body"],
        "vcard" => &["name", "org", "title", "phone", "mobile", "fax", "email", "url", "address"],
        _ => &["content"],
    }
}

/// The Data Merge fields `text` names as `<<Field>>`, of those `known` says exist.
fn field_refs<'a>(text: &'a str, known: &dyn Fn(&str) -> bool) -> Vec<&'a str> {
    let mut out = Vec::new();
    let mut rest = text;
    while let Some(i) = rest.find("<<") {
        let after = rest.get(i + 2..).unwrap_or("");
        let Some(j) = after.find(">>") else { break };
        let name = after.get(..j).unwrap_or("");
        if known(name) {
            out.push(name);
            rest = after.get(j + 2..).unwrap_or("");
        } else {
            rest = after;
        }
    }
    out
}

/// `template` with each `<<Field>>` that `cell` knows replaced by that record's value, as
/// [`encode`] params.
pub(crate) fn fill_template<'c>(template: &BTreeMap<String, String>, cell: impl Fn(&str) -> Option<&'c str>) -> Value {
    let mut out = serde_json::Map::new();
    for (k, v) in template {
        let mut filled = String::with_capacity(v.len());
        let mut rest = v.as_str();
        while let Some(i) = rest.find("<<") {
            let after = rest.get(i + 2..).unwrap_or("");
            let value = after.find(">>").and_then(|j| Some((j, cell(after.get(..j)?)?)));
            filled.push_str(rest.get(..i).unwrap_or(""));
            match value {
                Some((j, value)) => {
                    filled.push_str(value);
                    rest = after.get(j + 2..).unwrap_or("");
                }
                None => {
                    filled.push_str("<<");
                    rest = after;
                }
            }
        }
        filled.push_str(rest);
        out.insert(k.clone(), json!(filled));
    }
    Value::Object(out)
}

/// The template a frame's merged QR code comes from, if it has one.
fn merge_template(d: &designcraft_doc::Document, frame: ItemId) -> Option<&BTreeMap<String, String>> {
    d.data_merge
        .placeholders
        .iter()
        .find(|ph| ph.role == PlaceholderRole::Qr && ph.anchor == PlaceholderAnchor::Item { id: frame } && !ph.qr.is_empty())
        .map(|ph| &ph.qr)
}

fn qr_code(s: &mut Session, p: &Value) -> Result<Value> {
    const ID: &str = "object.qrCode";
    let text = encode(p).map_err(|e| bad(ID, e))?;
    let color = str_param(p, "color").unwrap_or(designcraft_color::swatch::BLACK).to_string();
    let rect = rect_param(p, "rect");
    let sr = spread_param(p, ID)?;
    let st = s.doc()?;
    let lid = st.active_layer;
    let sel = st.selection.items.clone();
    s.edit(|d, selection| {
        if d.swatch(&color).is_none() {
            return Err(bad(ID, format!("no swatch `{color}`")));
        }
        // `<<Field>>` in any field makes the code a Data Merge placeholder: each record gets its own.
        let kind = str_param(p, "type").unwrap_or("url");
        let template: BTreeMap<String, String> = PARAMS
            .iter()
            .filter(|k| matches!(**k, "type" | "color") || type_fields(kind).contains(*k))
            .filter_map(|k| Some((k.to_string(), str_param(p, k)?.to_string())))
            .collect();
        let (source_id, first_ref) = {
            let source = d.data_merge.sources.first();
            let known = |name: &str| source.is_some_and(|src| src.fields.iter().any(|f| f.name == name));
            let first = PARAMS.iter().filter_map(|k| template.get(*k)).find_map(|v| field_refs(v, &known).first().map(|n| n.to_string()));
            (source.map(|src| src.id), first)
        };
        let target = if rect.is_some() { None } else { sel.first().copied() };
        let id = match (rect, target) {
            (Some(r), _) if first_ref.is_some() => {
                // A merged code needs a frame to fill, record by record.
                let id = ItemId(d.alloc());
                let mut frame = Item::new(id, lid, Shape::Rectangle, designcraft_geom::shapes::rectangle(r));
                let code = qr_item(ItemId(d.alloc()), lid, &text, r, &color).map_err(|e| bad(ID, e))?;
                frame.content = Content::Group { items: vec![std::sync::Arc::new(code)] };
                d.insert_item(sr, frame, None)?;
                id
            }
            (Some(r), _) => {
                let id = ItemId(d.alloc());
                d.insert_item(sr, qr_item(id, lid, &text, r, &color).map_err(|e| bad(ID, e))?, None)?;
                id
            }
            (None, Some(t)) => {
                let it = d.item(t).ok_or_else(|| bad(ID, "no such object"))?.clone();
                if it.alt_text.starts_with(MARK) {
                    if first_ref.is_some() {
                        return Err(bad(ID, "Data Merge fields need a frame: select the code's frame, or an empty one"));
                    }
                    // Edit QR Code: same place and size.
                    let b = it.path.bounds().unwrap_or(Rect::ZERO);
                    let side = b.width().max(b.height());
                    let n = designcraft_geom::qr::qr_size(it.alt_text.trim_start_matches(MARK)).unwrap_or(21);
                    let quiet = side / (n + 4) as f64 * 2.0;
                    let r = Rect::from_center_size(b.center(), (side + 2.0 * quiet, side + 2.0 * quiet));
                    let fresh = qr_item(t, it.layer, &text, r, &color).map_err(|e| bad(ID, e))?;
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
                    let code = qr_item(id, it.layer, &text, it.inner_bounds(), &color).map_err(|e| bad(ID, e))?;
                    let f = d.item_mut(t).ok_or(designcraft_doc::DocError::NoItem(t))?;
                    f.content = Content::Group { items: vec![std::sync::Arc::new(code)] };
                    t
                }
            }
            (None, None) => return Err(bad(ID, "give `rect` or select a frame")),
        };
        // The frame's merged code follows this one: replaced, added, or (no fields) dropped.
        d.data_merge.placeholders.retain(|ph| !(ph.role == PlaceholderRole::Qr && ph.anchor == PlaceholderAnchor::Item { id } && !ph.qr.is_empty()));
        if let (Some(field), Some(source_id)) = (first_ref, source_id) {
            if let Some(f) = d.item_mut(id) {
                f.label = format!("<<{field}>>");
            }
            let ph = d.alloc();
            d.data_merge.placeholders.push(Placeholder {
                id: ph,
                source_id,
                field,
                role: PlaceholderRole::Qr,
                anchor: PlaceholderAnchor::Item { id },
                qr: template,
            });
        }
        *selection = Selection::items(vec![id]);
        Ok(json!({"id": id.0, "content": text}))
    })
}

/// The code `id` names, or the selected one (a code, or a frame holding one).
fn find_code(s: &Session, p: &Value) -> Result<(ItemId, Item)> {
    const ID: &str = "object.qrCodeRead";
    let st = s.doc()?;
    let id = match p.get("id").and_then(Value::as_u64) {
        Some(n) => ItemId(n),
        None => *st.selection.items.first().ok_or_else(|| bad(ID, "select a QR code"))?,
    };
    let it = st.doc.item(id).ok_or_else(|| bad(ID, "no such object"))?;
    if it.alt_text.starts_with(MARK) {
        return Ok((id, it.clone()));
    }
    if let Content::Group { items } = &it.content
        && let Some(code) = items.iter().find(|c| c.alt_text.starts_with(MARK))
    {
        return Ok((id, (**code).clone()));
    }
    Err(bad(ID, "not a QR code"))
}

fn qr_read(s: &mut Session, p: &Value) -> Result<Value> {
    let (frame, it) = find_code(s, p)?;
    let text = it.alt_text.trim_start_matches(MARK);
    // A merged code reads back as its template, `<<Field>>` references and all.
    let template = s.doc().ok().and_then(|st| merge_template(&st.doc, frame)).cloned();
    let mut out = match &template {
        Some(t) => fill_template(t, |_| None),
        None => decode(text),
    };
    out["merge"] = json!(template.is_some());
    out["id"] = json!(it.id.0);
    out["encoded"] = json!(text);
    out["color"] = json!(it.fill.swatch);
    Ok(out)
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

    #[test]
    fn business_cards_carry_work_and_mobile_numbers_and_read_back() {
        let mut s = Session::new();
        s.execute("file.new", &json!({})).unwrap();
        let fields = json!({"type": "vcard", "name": "Ada Lovelace", "org": "Analytical; Engines", "title": "Programmer",
            "phone": "+44 20 7946 0000", "mobile": "+44 7700 900000", "fax": "+44 20 7946 0001", "email": "ada@example.com",
            "url": "https://example.com", "address": "12 St James's Square, London"});
        let mut p = fields.clone();
        p["rect"] = json!([72, 72, 216, 216]);
        let r = s.execute("object.qrCode", &p).unwrap();
        let v = r["content"].as_str().unwrap();
        assert!(v.starts_with("BEGIN:VCARD\r\nVERSION:3.0\r\nN:Lovelace;Ada;;;\r\nFN:Ada Lovelace\r\n"), "{v}");
        assert!(v.contains("TEL;TYPE=WORK,VOICE:+44 20 7946 0000\r\n") && v.contains("TEL;TYPE=CELL:+44 7700 900000\r\n"), "{v}");
        assert!(v.contains("ORG:Analytical\\; Engines\r\n") && v.contains("ADR;TYPE=WORK:;;12 St James's Square\\, London;;;;\r\n"), "{v}");
        assert!(v.ends_with("END:VCARD"));
        // Read it back: the dialog's fields.
        let back = s.execute("object.qrCodeRead", &json!({})).unwrap();
        for (k, want) in fields.as_object().unwrap() {
            assert_eq!(&back[k], want, "{k}");
        }
        assert_eq!(back["color"], "[Black]");
        assert_eq!(back["id"], r["id"]);
    }

    #[test]
    fn every_type_reads_back_from_its_code() {
        for p in [
            json!({"type": "url", "content": "https://example.com/a?b=c"}),
            json!({"type": "text", "content": "Hello; world"}),
            json!({"type": "tel", "number": "+1 555 0100"}),
            json!({"type": "sms", "number": "+1 555 0100", "message": "Hi: there"}),
            json!({"type": "email", "to": "a@b.c", "subject": "Hi; you", "body": "Line: one"}),
        ] {
            let text = super::encode(&p).unwrap();
            assert_eq!(super::decode(&text), p, "{text}");
        }
        // The old MECARD business cards still read.
        let old = super::decode("MECARD:N:Ada;TEL:+44 1;EMAIL:a@b.c;;");
        assert_eq!((old["type"].as_str(), old["name"].as_str(), old["phone"].as_str()), (Some("vcard"), Some("Ada"), Some("+44 1")));
        assert!(super::encode(&json!({"type": "tel"})).is_err());
    }

    #[test]
    fn hostile_codes_decode_without_panicking() {
        for t in ["", "\\", "BEGIN:VCARD", "BEGIN:VCARD\n:\nTEL\n\\\nADR:;;\\", "MECARD:\\", "MATMSG:", "SMSTO:", "TEL:", "BEGIN:VCARD\r\n \r\n\t"] {
            let _ = super::decode(t);
        }
        let mut s = Session::new();
        s.execute("file.new", &json!({})).unwrap();
        assert!(s.execute("object.qrCodeRead", &json!({})).is_err());
        let f = s.execute("frame.create", &json!({"rect": [0, 0, 50, 50]})).unwrap()["id"].as_u64().unwrap();
        assert!(s.execute("object.qrCodeRead", &json!({"id": f})).is_err());
        assert!(s.execute("object.qrCodeRead", &json!({"id": 999_999})).is_err());
    }
}
