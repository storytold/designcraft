//! Structure and XML: tags on frames (Tags panel), paragraph styles mapped to tags, the structure
//! tree, XML export and import (content flows into the frames with matching tags).

use designcraft_doc::{Content, Document, Item, ItemId, Story, StoryId, XmlTag};
use serde_json::{Value, json};

use super::{CommandSpec, bad, bool_or, cmd, has_doc, ids_param, ok, str_param};
use crate::{Result, Session};

const COLORS: [[u8; 3]; 6] = [[230, 120, 40], [60, 140, 220], [70, 170, 90], [200, 70, 160], [150, 110, 220], [200, 170, 40]];

fn root_name(d: &Document) -> String {
    if d.xml.root.is_empty() { "Root".into() } else { d.xml.root.clone() }
}

fn valid_name(n: &str) -> bool {
    let mut c = n.chars();
    c.next().is_some_and(|f| f.is_alphabetic() || f == '_') && n.chars().all(|ch| ch.is_alphanumeric() || matches!(ch, '_' | '-' | '.'))
}

/// Tagged objects in reading order (pages, then top to bottom, left to right), nested ones included.
fn tagged(d: &Document) -> Vec<ItemId> {
    in_reading_order(d, |it| !it.xml_tag.is_empty())
}

/// Document-page objects that pass `keep`, in reading order, nested ones included.
fn in_reading_order(d: &Document, keep: impl Fn(&Item) -> bool) -> Vec<ItemId> {
    let mut keyed: Vec<((usize, i64, i64), ItemId)> = Vec::new();
    for (si, sp) in d.spreads.iter().enumerate() {
        let first = d.first_page_of_spread(si);
        for top in &sp.items {
            top.walk(&mut |it: &Item| {
                if keep(it) {
                    let b = it.bounds();
                    let page = first + sp.page_at_x(b.center().x).unwrap_or(0);
                    keyed.push(((page, (b.y0 * 10.0) as i64, (b.x0 * 10.0) as i64), it.id));
                }
            });
        }
    }
    keyed.sort_by_key(|(k, _)| *k);
    keyed.into_iter().map(|(_, i)| i).collect()
}

/// `text` for text frames, `image` for frames holding a graphic and empty graphic frames, `None`
/// for anything else: a plain shape is no frame, whatever its object style (InDesign gives every
/// rectangle the graphics frame style).
fn frame_kind(it: &Item) -> Option<&'static str> {
    match &it.content {
        Content::Text(_) => Some("text"),
        Content::Graphic(_) => Some("image"),
        Content::Unassigned if it.graphic_frame => Some("image"),
        _ => None,
    }
}

/// `xml.autoTag`: tags text and graphic frames on the document pages, by their Layers panel name
/// when that is a valid XML name, otherwise `text_1`, `image_1`, … in reading order.
fn auto_tag(s: &mut Session, p: &Value) -> Result<Value> {
    const ID: &str = "xml.autoTag";
    let kinds: Vec<String> = match p.get("kinds") {
        None | Some(Value::Null) => vec!["text".into(), "image".into()],
        Some(Value::Array(a)) => a.iter().filter_map(Value::as_str).map(str::to_string).collect(),
        Some(_) => return Err(bad(ID, "`kinds` is a list: [\"text\", \"image\"]")),
    };
    if let Some(k) = kinds.iter().find(|k| !matches!(k.as_str(), "text" | "image")) {
        return Err(bad(ID, format!("unknown kind `{k}` (text, image)")));
    }
    let overwrite = bool_or(p, "overwrite", false);
    let use_names = bool_or(p, "useNames", true);
    let d = &s.doc()?.doc;
    let wanted = |it: &Item| frame_kind(it).is_some_and(|k| kinds.iter().any(|w| w == k)) && (overwrite || it.xml_tag.is_empty());
    let ids = in_reading_order(d, wanted);
    // Numbers already taken by tags that stay, so `text_3` isn't given twice.
    let mut used: std::collections::HashSet<String> = in_reading_order(d, |it| !it.xml_tag.is_empty())
        .into_iter()
        .filter(|i| !ids.contains(i))
        .filter_map(|i| d.item(i))
        .map(|it| it.xml_tag.clone())
        .collect();
    let mut next = [("text", 0usize), ("image", 0usize)];
    let mut plan: Vec<(ItemId, String, &'static str)> = Vec::new();
    for id in ids {
        let Some(it) = d.item(id) else { continue };
        let Some(kind) = frame_kind(it) else { continue };
        let name = it.name.trim();
        let tag = if use_names && valid_name(name) {
            name.to_string()
        } else {
            let counter = next.iter_mut().find(|(k, _)| *k == kind).map(|(_, n)| n);
            let Some(n) = counter else { continue };
            loop {
                *n += 1;
                let t = format!("{kind}_{n}");
                if !used.contains(&t) {
                    break t;
                }
            }
        };
        used.insert(tag.clone());
        plan.push((id, tag, kind));
    }
    s.edit(|d, _| {
        for (id, tag, _) in &plan {
            if !d.xml.tags.iter().any(|x| x.name == *tag) {
                let color = COLORS[d.xml.tags.len() % COLORS.len()];
                d.xml.tags.push(XmlTag { name: tag.clone(), color });
            }
            if let Some(it) = d.item_mut(*id) {
                it.xml_tag = tag.clone();
            }
        }
        let tagged: Vec<Value> = plan.iter().map(|(id, tag, kind)| json!({"id": id.0, "tag": tag, "kind": kind})).collect();
        Ok(json!({"tagged": tagged}))
    })
}

/// Escapes markup characters and drops characters XML 1.0 cannot hold.
fn esc(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '\t' | '\n' | '\r' => out.push(c),
            c if (c as u32) < 0x20 || c == '\u{FFFE}' || c == '\u{FFFF}' => {}
            c => out.push(c),
        }
    }
    out
}

/// Plain text of a story (markers dropped, forced breaks as newlines).
fn plain(t: &str) -> String {
    t.chars().filter(|c| !('\u{E000}'..='\u{E1FF}').contains(c)).map(|c| if c == '\u{2028}' { '\n' } else { c }).collect()
}

/// A paragraph's text with inline-tagged runs as elements.
fn para_xml(st: &designcraft_doc::Story, r: std::ops::Range<usize>) -> String {
    let mut out = String::new();
    let mut open: Option<String> = None;
    for (rr, f) in st.runs() {
        let (a, b) = (rr.start.max(r.start), rr.end.min(r.end));
        if a >= b {
            continue;
        }
        let tag = f.over.xml_tag.clone().filter(|t| !t.is_empty());
        if tag != open {
            if let Some(t) = open.take() {
                out.push_str(&format!("</{t}>"));
            }
            if let Some(t) = &tag {
                out.push_str(&format!("<{t}>"));
            }
            open = tag;
        }
        out.push_str(&esc(&plain(&st.text[a..b])));
    }
    if let Some(t) = open {
        out.push_str(&format!("</{t}>"));
    }
    out
}

/// The document's tagged content as XML.
pub fn export_xml(d: &Document) -> String {
    let mut out = format!("<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?>\n<{}>\n", root_name(d));
    for id in tagged(d) {
        let Some(it) = d.item(id) else { continue };
        let tag = &it.xml_tag;
        match &it.content {
            Content::Text(tf) => {
                let Some(st) = d.story(tf.story) else { continue };
                let mapped = |style: &str| d.xml.style_map.iter().find(|(s, _)| s == style).map(|(_, t)| t.clone());
                // Paragraph text with inline tags already escaped.
                let paras: Vec<(Option<String>, String)> =
                    st.para_ranges().into_iter().enumerate().map(|(i, r)| (mapped(&st.paras[i].style), para_xml(st, r))).collect();
                if paras.iter().any(|(m, _)| m.is_some()) {
                    out.push_str(&format!("  <{tag}>\n"));
                    for (m, text) in paras {
                        match m {
                            Some(t) => out.push_str(&format!("    <{t}>{text}</{t}>\n")),
                            None => out.push_str(&format!("    {text}\n")),
                        }
                    }
                    out.push_str(&format!("  </{tag}>\n"));
                } else {
                    out.push_str(&format!("  <{tag}>{}</{tag}>\n", paras.into_iter().map(|(_, t)| t).collect::<Vec<_>>().join("\n")));
                }
            }
            Content::Graphic(g) => {
                let name = d.assets.get(&g.asset).map(|a| a.link.clone().unwrap_or_else(|| a.name.clone())).unwrap_or_default();
                out.push_str(&format!("  <{tag} href=\"file://{}\"/>\n", esc(&name)));
            }
            _ => out.push_str(&format!("  <{tag}/>\n")),
        }
    }
    out.push_str(&format!("</{}>\n", root_name(d)));
    out
}

/// One element under the root: its name and either text or (child name, text) paragraphs, or an
/// image to place (its `href`, and how: `fit`, an `object.fit` mode, and `crop`, a PDF box).
struct Element {
    name: String,
    text: String,
    children: Vec<(String, String)>,
    image: Option<Image>,
}

struct Image {
    href: String,
    fit: Option<String>,
    crop: Option<String>,
}

/// The element's image: its `href` (also `xlink:href`), `fit` and `crop`, unescaped.
fn image_attrs(e: &quick_xml::events::BytesStart) -> std::result::Result<Option<Image>, String> {
    let (mut href, mut fit, mut crop) = (None, None, None);
    for a in e.attributes() {
        let a = a.map_err(|e| e.to_string())?;
        let slot = match a.key.local_name().as_ref() {
            b"href" => &mut href,
            b"fit" => &mut fit,
            b"crop" => &mut crop,
            _ => continue,
        };
        *slot = Some(a.normalized_value(quick_xml::XmlVersion::Implicit1_0).map_err(|e| e.to_string())?.to_string());
    }
    Ok(href.map(|href| Image { href, fit, crop }))
}

/// A file path from an `href`: `file://` URLs (with `%20` escapes) or plain paths, relative ones
/// against `base`.
fn href_path(href: &str, base: Option<&std::path::Path>) -> String {
    let mut p = href.trim();
    if let Some(rest) = p.strip_prefix("file://") {
        p = rest;
        // `file:///C:/x.png` → `C:/x.png`; `file:///home/x.png` stays absolute.
        if p.len() > 2 && p.starts_with('/') && p.as_bytes()[2] == b':' {
            p = &p[1..];
        }
    }
    let decoded = percent_decode(p);
    let path = std::path::Path::new(&decoded);
    match base {
        Some(b) if path.is_relative() && !decoded.starts_with('/') && !decoded.starts_with('\\') => b.join(path).to_string_lossy().to_string(),
        _ => decoded,
    }
}

fn percent_decode(s: &str) -> String {
    let b = s.as_bytes();
    let mut out = Vec::with_capacity(b.len());
    let mut i = 0;
    while i < b.len() {
        let hex = |c: u8| (c as char).to_digit(16);
        match (b[i], b.get(i + 1).copied().and_then(hex), b.get(i + 2).copied().and_then(hex)) {
            (b'%', Some(h), Some(l)) => {
                out.push((h * 16 + l) as u8);
                i += 3;
            }
            (c, _, _) => {
                out.push(c);
                i += 1;
            }
        }
    }
    String::from_utf8_lossy(&out).to_string()
}

fn parse(xml: &str) -> std::result::Result<Vec<Element>, String> {
    use quick_xml::events::Event;
    let mut r = quick_xml::Reader::from_str(xml);
    let mut depth = 0usize;
    let mut out: Vec<Element> = Vec::new();
    let mut child: Option<(String, String)> = None;
    loop {
        match r.read_event().map_err(|e| e.to_string())? {
            Event::Start(e) => {
                depth += 1;
                let name = String::from_utf8_lossy(e.local_name().as_ref()).to_string();
                match depth {
                    2 => out.push(Element { name, text: String::new(), children: vec![], image: image_attrs(&e)? }),
                    3 => child = Some((name, String::new())),
                    _ => {}
                }
            }
            Event::Empty(e) if depth == 1 => {
                out.push(Element {
                    name: String::from_utf8_lossy(e.local_name().as_ref()).to_string(),
                    text: String::new(),
                    children: vec![],
                    image: image_attrs(&e)?,
                });
            }
            ev @ (Event::Text(_) | Event::GeneralRef(_) | Event::CData(_)) => {
                let s = match ev {
                    Event::Text(t) => t.decode().map_err(|e| e.to_string())?.to_string(),
                    Event::CData(t) => String::from_utf8_lossy(&t).to_string(),
                    Event::GeneralRef(g) => match g.resolve_char_ref().map_err(|e| e.to_string())? {
                        Some(c) => c.to_string(),
                        None => match g.decode().map_err(|e| e.to_string())?.as_ref() {
                            "amp" => "&".into(),
                            "lt" => "<".into(),
                            "gt" => ">".into(),
                            "quot" => "\"".into(),
                            "apos" => "'".into(),
                            _ => String::new(),
                        },
                    },
                    _ => String::new(),
                };
                match (depth, child.as_mut(), out.last_mut()) {
                    (3, Some(c), _) => c.1.push_str(&s),
                    (2, _, Some(el)) => el.text.push_str(&s),
                    _ => {}
                }
            }
            Event::End(_) => {
                if depth == 3
                    && let (Some(c), Some(el)) = (child.take(), out.last_mut())
                {
                    el.children.push(c);
                }
                depth = depth.saturating_sub(1);
            }
            Event::Eof => break,
            _ => {}
        }
    }
    Ok(out)
}

pub fn specs() -> Vec<CommandSpec> {
    vec![
        cmd!(query "xml.tags", "Tags", [], None, "{} → {root, tags: [{name, color}], styleMap: [[style, tag]], dtd: bool}", has_doc, |s, _| {
            let d = &s.doc()?.doc;
            Ok(json!({"root": root_name(d), "tags": d.xml.tags, "styleMap": d.xml.style_map, "dtd": !d.xml.dtd.is_empty()}))
        }),
        cmd!("xml.newTag", "New Tag", ["Window", "Utilities", "Tags"], None, "{name, color?: [r, g, b]}", has_doc, |s, p| {
            let name = str_param(p, "name").unwrap_or("").trim().to_string();
            if !valid_name(&name) {
                return Err(bad("xml.newTag", format!("`{name}` isn't a valid XML name")));
            }
            s.edit(|d, _| {
                if !d.xml.tags.iter().any(|t| t.name == name) {
                    let color = COLORS[d.xml.tags.len() % COLORS.len()];
                    d.xml.tags.push(XmlTag { name: name.clone(), color });
                }
                Ok(json!({"name": name}))
            })
        }),
        cmd!("xml.deleteTag", "Delete Tag", [], None, "{name} — tagged objects are untagged", has_doc, |s, p| {
            let name = str_param(p, "name").unwrap_or("").to_string();
            let ids: Vec<ItemId> = {
                let d = &s.doc()?.doc;
                tagged(d).into_iter().filter(|i| d.item(*i).is_some_and(|it| it.xml_tag == name)).collect()
            };
            s.edit(|d, _| {
                d.xml.tags.retain(|t| t.name != name);
                d.xml.style_map.retain(|(_, t)| *t != name);
                for id in &ids {
                    if let Some(it) = d.item_mut(*id) {
                        it.xml_tag.clear();
                    }
                }
                ok()
            })
        }),
        cmd!("xml.tagText", "Tag Text", [], None, "{tag | null (untag)} — the selected text becomes an inline element", super::has_doc, |s, p| {
            let tag = str_param(p, "tag").map(str::to_string);
            if let Some(t) = &tag
                && !valid_name(t)
            {
                return Err(bad("xml.tagText", format!("`{t}` isn't a valid XML name")));
            }
            if s.doc()?.selection.text.is_none_or(|t| t.range().is_empty()) {
                return Err(bad("xml.tagText", "select some text"));
            }
            if let Some(t) = &tag {
                s.edit(|d, _| {
                    if !d.xml.tags.iter().any(|x| x.name == *t) {
                        let color = COLORS[d.xml.tags.len() % COLORS.len()];
                        d.xml.tags.push(XmlTag { name: t.clone(), color });
                    }
                    ok()
                })?;
            }
            s.execute("type.char", &json!({"xmlTag": tag.unwrap_or_default()}))
        }),
        cmd!("xml.tag", "Tag Frame", [], None, "{tag (made if new), ids? (default: the selection)} — `tag: null` untags", has_doc, |s, p| {
            let tag = str_param(p, "tag").map(str::to_string);
            if let Some(t) = &tag
                && !valid_name(t)
            {
                return Err(bad("xml.tag", format!("`{t}` isn't a valid XML name")));
            }
            let ids = ids_param(p, "ids").unwrap_or_else(|| s.active().map(|d| d.selection.items.clone()).unwrap_or_default());
            if ids.is_empty() {
                return Err(bad("xml.tag", "select objects to tag"));
            }
            s.edit(|d, _| {
                if let Some(t) = &tag
                    && !d.xml.tags.iter().any(|x| x.name == *t)
                {
                    let color = COLORS[d.xml.tags.len() % COLORS.len()];
                    d.xml.tags.push(XmlTag { name: t.clone(), color });
                }
                for id in &ids {
                    if let Some(it) = d.item_mut(*id) {
                        it.xml_tag = tag.clone().unwrap_or_default();
                    }
                }
                ok()
            })
        }),
        cmd!(
            "xml.autoTag",
            "Auto Tag Frames",
            [],
            None,
            "{kinds?: [text, image] (default both), overwrite?: bool (retag tagged frames; default false), useNames?: bool (a valid XML Layers panel name is the tag; default true)} — untagged frames on document pages get their name or text_1, image_1, … in reading order → {tagged: [{id, tag, kind}]}",
            has_doc,
            auto_tag
        ),
        cmd!(
            "xml.mapStyle",
            "Map Styles to Tags",
            [],
            None,
            "{style: paragraph style, tag | null} — that style's paragraphs export as `<tag>` elements and import back with it",
            has_doc,
            |s, p| {
                let style = str_param(p, "style").unwrap_or("").to_string();
                let tag = str_param(p, "tag").map(str::to_string);
                if let Some(t) = &tag
                    && !valid_name(t)
                {
                    return Err(bad("xml.mapStyle", format!("`{t}` isn't a valid XML name")));
                }
                if s.doc()?.doc.styles.para(&style).is_none() {
                    return Err(bad("xml.mapStyle", format!("no paragraph style `{style}`")));
                }
                s.edit(|d, _| {
                    d.xml.style_map.retain(|(st, _)| *st != style);
                    if let Some(t) = tag {
                        if !d.xml.tags.iter().any(|x| x.name == t) {
                            let color = COLORS[d.xml.tags.len() % COLORS.len()];
                            d.xml.tags.push(XmlTag { name: t.clone(), color });
                        }
                        d.xml.style_map.push((style.clone(), t));
                    }
                    ok()
                })
            }
        ),
        cmd!(query "xml.structure", "Structure", [], None, "{} → {root, elements: [{tag, id, kind, text?}]} in reading order", has_doc, |s, _| {
            let d = &s.doc()?.doc;
            let elements: Vec<Value> = tagged(d)
                .into_iter()
                .filter_map(|id| d.item(id))
                .map(|it| {
                    let (kind, text) = match &it.content {
                        Content::Text(tf) => ("text", d.story(tf.story).map(|st| plain(&st.text))),
                        Content::Graphic(_) => ("graphic", None),
                        _ => ("object", None),
                    };
                    json!({"tag": it.xml_tag, "id": it.id.0, "kind": kind, "text": text})
                })
                .collect();
            Ok(json!({"root": root_name(d), "elements": elements}))
        }),
        cmd!(noundo "file.exportXml", "Export XML…", ["File"], None, "{path?} — the tagged content in reading order → {path, bytes} (no path: {text})", has_doc, |s, p| {
            let text = export_xml(&s.doc()?.doc);
            match str_param(p, "path") {
                Some(path) => {
                    #[cfg(not(target_arch = "wasm32"))]
                    crate::io::write_file(path, text.as_bytes()).map_err(crate::EngineError::Other)?;
                    Ok(json!({"path": path, "bytes": text.len()}))
                }
                None => Ok(json!({"text": text, "bytes": text.len()})),
            }
        }),
        cmd!(
            "xml.loadDtd",
            "Load DTD…",
            ["Window", "Utilities", "Tags"],
            None,
            "{path | text} — keep the DTD for validation and add a tag for each declared element; the root takes the first element's name unless already named → {elements}",
            has_doc,
            |s, p| {
                let text = text_or_path(p, "xml.loadDtd")?;
                let dtd = crate::dtd::parse(&text).map_err(|e| bad("xml.loadDtd", e))?;
                let names: Vec<String> = dtd.element_names().map(str::to_string).collect();
                s.edit(|d, _| {
                    for n in &names {
                        if valid_name(n) && !d.xml.tags.iter().any(|t| t.name == *n) {
                            let color = COLORS[d.xml.tags.len() % COLORS.len()];
                            d.xml.tags.push(XmlTag { name: n.clone(), color });
                        }
                    }
                    if d.xml.root.is_empty()
                        && let Some(first) = names.first()
                    {
                        d.xml.root = first.clone();
                    }
                    d.xml.dtd = text.clone();
                    Ok(json!({"elements": names.len()}))
                })
            }
        ),
        cmd!(
            "xml.deleteDtd",
            "Delete DTD",
            [],
            None,
            "{}",
            |s: &Session| { if s.doc().is_ok_and(|d| !d.doc.xml.dtd.is_empty()) { Ok(()) } else { Err("no DTD loaded".into()) } },
            |s, _| s.edit(|d, _| {
                d.xml.dtd.clear();
                ok()
            })
        ),
        cmd!(
            query "xml.validate",
            "Validate from Root Element",
            [],
            None,
            "{} — check the structure (as exported) against the loaded DTD → {valid, problems: [{path, message}]}",
            |s: &Session| {
                if s.doc().is_ok_and(|d| !d.doc.xml.dtd.is_empty()) { Ok(()) } else { Err("load a DTD first".into()) }
            },
            |s, _| {
                let d = &s.doc()?.doc;
                let dtd = crate::dtd::parse(&d.xml.dtd).map_err(|e| bad("xml.validate", e))?;
                let problems = crate::dtd::validate(&dtd, &export_xml(d));
                Ok(json!({"valid": problems.is_empty(), "problems": problems}))
            }
        ),
        cmd!(
            "file.importXml",
            "Import XML…",
            ["File"],
            None,
            "{path | text, imageDir?} — each element's content goes into the next text frame with that tag (child elements of mapped tags become paragraphs in their style); an element with `href` (a file:// URL or path; relative ones against imageDir, else the XML file's folder) places that image into the next graphic frame with its tag (`fit`: an object.fit mode, default fillProportionally; `crop`: a PDF box such as contentVisible) → {placed, unmatched, unmatchedTags: [tags without a free frame], failed: [{tag, href, error}]}",
            has_doc,
            import_xml
        ),
    ]
}

/// The `text` parameter, or the file at `path`.
fn text_or_path(p: &Value, id: &str) -> Result<String> {
    Ok(match (str_param(p, "text"), str_param(p, "path")) {
        (Some(t), _) => t.to_string(),
        (None, Some(path)) => {
            #[cfg(not(target_arch = "wasm32"))]
            let t = crate::io::read_to_string(path).map_err(crate::EngineError::Other)?;
            #[cfg(target_arch = "wasm32")]
            let t = {
                let _ = path;
                String::new()
            };
            t
        }
        _ => return Err(bad(id, "`path` or `text` required")),
    })
}

fn import_xml(s: &mut Session, p: &Value) -> Result<Value> {
    const ID: &str = "file.importXml";
    let text = text_or_path(p, ID)?;
    let elements = parse(&text).map_err(|e| bad(ID, format!("not well-formed XML: {e}")))?;
    // Relative image paths: against `imageDir`, else the XML file's folder.
    let base = str_param(p, "imageDir").map(std::path::PathBuf::from).or_else(|| {
        str_param(p, "text").is_none().then(|| str_param(p, "path")).flatten().and_then(|x| std::path::Path::new(x).parent().map(Into::into))
    });
    let d = &s.doc()?.doc;
    // Frames by tag, in reading order; each element takes the next free one.
    let mut slots: Vec<(String, StoryId, bool)> =
        tagged(d).into_iter().filter_map(|id| d.item(id)).filter_map(|it| it.text_frame().map(|tf| (it.xml_tag.clone(), tf.story, false))).collect();
    let mut image_slots: Vec<(String, ItemId, bool)> = tagged(d)
        .into_iter()
        .filter_map(|id| d.item(id))
        .filter(|it| frame_kind(it) == Some("image"))
        .map(|it| (it.xml_tag.clone(), it.id, false))
        .collect();
    let reverse: Vec<(String, String)> = d.xml.style_map.iter().map(|(st, t)| (t.clone(), st.clone())).collect();
    let mut fills: Vec<(StoryId, Vec<(Option<String>, String)>)> = Vec::new();
    let mut images: Vec<(ItemId, String, Image)> = Vec::new();
    // Elements without a free frame of their tag, and those tags (once each, in order).
    let mut unmatched = 0;
    let mut unmatched_tags: Vec<String> = Vec::new();
    let mut miss = |name: &str| {
        unmatched += 1;
        if !unmatched_tags.iter().any(|t| t == name) {
            unmatched_tags.push(name.to_string());
        }
    };
    for mut el in elements {
        if let Some(image) = el.image.take() {
            let Some(slot) = image_slots.iter_mut().find(|(t, _, used)| *t == el.name && !used) else {
                miss(&el.name);
                continue;
            };
            slot.2 = true;
            images.push((slot.1, el.name, image));
            continue;
        }
        let Some(slot) = slots.iter_mut().find(|(t, _, used)| *t == el.name && !used) else {
            miss(&el.name);
            continue;
        };
        slot.2 = true;
        let paras: Vec<(Option<String>, String)> = if el.children.is_empty() {
            el.text.trim().split('\n').map(|l| (None, l.trim().to_string())).collect()
        } else {
            el.children
                .into_iter()
                .map(|(t, txt)| (reverse.iter().find(|(tag, _)| *tag == t).map(|(_, st)| st.clone()), txt.trim().to_string()))
                .collect()
        };
        fills.push((slot.1, paras));
    }
    let mut placed = fills.len();
    if !fills.is_empty() {
        fill_stories(s, fills)?;
    }
    // Each image fills its frame proportionally unless `fit` says otherwise; `crop` picks a PDF's
    // box. One that can't be placed is reported and the rest of the import goes on.
    let mut failed: Vec<Value> = Vec::new();
    for (frame, tag, image) in images {
        let path = href_path(&image.href, base.as_deref());
        let mut place = json!({"path": path, "frame": frame.0});
        if let Some(crop) = &image.crop {
            place["pdfCrop"] = json!(crop);
        }
        // An unknown fit fails before anything is placed, so `failed` means not placed.
        if let Some(mode) = image.fit.as_deref().filter(|m| !super::object::fit_mode_known(m)) {
            failed.push(json!({"tag": tag, "href": image.href, "error": bad(ID, format!("unknown fit `{mode}`")).to_string()}));
            continue;
        }
        let done = s.execute("file.place", &place).and_then(|_| match &image.fit {
            Some(mode) => s.execute("object.fit", &json!({"mode": mode, "ids": [frame.0]})),
            None => Ok(Value::Null),
        });
        match done {
            Ok(_) => placed += 1,
            Err(e) => failed.push(json!({"tag": tag, "href": image.href, "error": e.to_string()})),
        }
    }
    Ok(json!({"placed": placed, "unmatched": unmatched, "unmatchedTags": unmatched_tags, "failed": failed}))
}

/// Replaces each story's text with the paragraphs, in their mapped styles.
fn fill_stories(s: &mut Session, fills: Vec<(StoryId, Vec<(Option<String>, String)>)>) -> Result<()> {
    s.edit(|d, _| {
        for (sid, paras) in fills {
            if let Some(st) = d.story_mut(sid) {
                fill_story(st, &paras);
            }
        }
        Ok(())
    })
}

/// Byte lengths of the longest common start of `a` and `b`, and of the longest common end of
/// what is left, on character boundaries.
fn common_ends(a: &str, b: &str) -> (usize, usize) {
    let pre: usize = a.chars().zip(b.chars()).take_while(|(x, y)| x == y).map(|(x, _)| x.len_utf8()).sum();
    let suf: usize = a[pre..].chars().rev().zip(b[pre..].chars().rev()).take_while(|(x, y)| x == y).map(|(x, _)| x.len_utf8()).sum();
    (pre, suf)
}

/// Puts the paragraphs into the story in place, so the template's formatting stays: paragraph
/// `i` replaces old paragraph `i` (only the part that differs, which takes the format of the
/// first character it replaces), extra paragraphs copy the last one's formats, surplus old
/// paragraphs are removed. A trailing empty paragraph (the story ending in a return) stays.
fn fill_story(st: &mut Story, paras: &[(Option<String>, String)]) {
    let ranges = st.para_ranges();
    let keep_last = ranges.len() > 1 && ranges.last().is_some_and(|r| r.is_empty());
    let old_n = ranges.len() - usize::from(keep_last);
    for (i, (_, new)) in paras.iter().enumerate() {
        let ranges = st.para_ranges();
        if i < old_n {
            let r = ranges[i].clone();
            let (pre, suf) = common_ends(&st.text[r.clone()], new);
            let (a, b) = (r.start + pre, r.end - suf);
            let repl = &new[pre..new.len() - suf];
            if a < b {
                st.replace(a..b, repl);
            } else if !repl.is_empty() {
                let fmt = if pre > 0 { st.char_format_at(a).clone() } else { st.format_after(a).clone() };
                st.insert_with(a, repl, fmt);
            }
        } else if let Some(prev) = i.checked_sub(1).and_then(|p| ranges.get(p)) {
            st.insert(prev.end, &format!("\n{new}"));
        }
    }
    if old_n > paras.len() {
        let ranges = st.para_ranges();
        if let (Some(keep), Some(last)) = (paras.len().checked_sub(1).and_then(|k| ranges.get(k)), ranges.get(old_n - 1)) {
            st.delete(keep.end..last.end);
        }
    }
    for (i, (style, _)) in paras.iter().enumerate() {
        if let (Some(sty), Some(pf)) = (style, st.paras.get_mut(i)) {
            pf.style = sty.clone();
        }
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use crate::Session;

    #[test]
    fn dtd_loads_tags_and_validates_the_structure() {
        let mut s = Session::new();
        s.execute("file.new", &json!({})).unwrap();
        assert!(s.execute("xml.validate", &json!({})).is_err(), "no DTD yet");
        let dtd = "<!ELEMENT article (headline, story+)><!ELEMENT headline (#PCDATA)><!ELEMENT story (#PCDATA | term)*><!ELEMENT term (#PCDATA)>";
        let r = s.execute("xml.loadDtd", &json!({"text": dtd})).unwrap();
        assert_eq!(r["elements"], 4);
        let tags = s.execute("xml.tags", &json!({})).unwrap();
        assert_eq!(tags["root"], "article");
        assert_eq!(tags["tags"].as_array().unwrap().len(), 4);
        let a = s.execute("frame.create", &json!({"rect": [72, 72, 400, 150], "content": "text", "text": "Body", "caret": false})).unwrap();
        s.execute("xml.tag", &json!({"tag": "story", "ids": [a["id"]]})).unwrap();
        let v = s.execute("xml.validate", &json!({})).unwrap();
        assert_eq!(v["valid"], false);
        assert!(v["problems"][0]["message"].as_str().unwrap().contains("the DTD asks for (headline, story+)"), "{v}");
        let h = s.execute("frame.create", &json!({"rect": [72, 20, 400, 60], "content": "text", "text": "Head", "caret": false})).unwrap();
        s.execute("xml.tag", &json!({"tag": "headline", "ids": [h["id"]]})).unwrap();
        assert_eq!(s.execute("xml.validate", &json!({})).unwrap()["valid"], true);
        assert!(s.execute("xml.loadDtd", &json!({"text": "<!ELEMENT a (b,|c)>"})).is_err());
        s.execute("xml.deleteDtd", &json!({})).unwrap();
        assert!(s.execute("xml.validate", &json!({})).is_err());
    }

    #[test]
    fn export_xml_drops_forbidden_control_chars() {
        let mut s = Session::new();
        s.execute("file.new", &json!({})).unwrap();
        let a = s
            .execute("frame.create", &json!({"rect": [72, 72, 400, 150], "content": "text", "text": "A\u{1}B\u{8}C\u{FFFE}\u{FFFF}", "caret": false}))
            .unwrap();
        s.execute("xml.tag", &json!({"tag": "story", "ids": [a["id"]]})).unwrap();
        let x = s.execute("file.exportXml", &json!({})).unwrap()["text"].as_str().unwrap().to_string();
        assert!(x.contains("<story>ABC</story>"), "{x:?}");
    }

    #[test]
    fn map_style_rejects_invalid_tag_name() {
        let mut s = Session::new();
        s.execute("file.new", &json!({})).unwrap();
        s.execute("style.paragraph.create", &json!({"name": "Head"})).unwrap();
        assert!(s.execute("xml.mapStyle", &json!({"style": "Head", "tag": "a b><x"})).is_err());
        let d = &s.doc().unwrap().doc;
        assert!(d.xml.style_map.is_empty());
        assert!(d.xml.tags.iter().all(|t| t.name != "a b><x"));
        s.execute("xml.mapStyle", &json!({"style": "Head", "tag": "title"})).unwrap();
        assert_eq!(s.doc().unwrap().doc.xml.style_map.len(), 1);
    }

    #[test]
    fn auto_tag_names_text_and_graphic_frames() {
        let mut s = Session::new();
        s.execute("file.new", &json!({})).unwrap();
        let frame = |s: &mut Session, rect: [i32; 4], content: &str| {
            s.execute("frame.create", &json!({"rect": rect, "content": content, "text": "x", "caret": false})).unwrap()["id"].clone()
        };
        let body = frame(&mut s, [72, 300, 400, 350], "text");
        let head = frame(&mut s, [72, 72, 400, 120], "text");
        let photo = frame(&mut s, [72, 400, 300, 600], "graphic");
        let kept = frame(&mut s, [72, 700, 400, 740], "text");
        let shape = frame(&mut s, [450, 72, 500, 120], "unassigned");
        s.execute("object.rename", &json!({"id": head, "name": "headline"})).unwrap();
        s.execute("object.rename", &json!({"id": body, "name": "not a tag"})).unwrap();
        // A plain shape in the graphics frame style (as InDesign gives every rectangle) is no frame.
        s.edit(|d, _| {
            let style = d.styles.default_graphic_frame.clone();
            if let Some(it) = d.item_mut(designcraft_doc::ItemId(shape.as_u64().unwrap())) {
                it.object_style = style;
            }
            Ok(())
        })
        .unwrap();
        s.execute("xml.tag", &json!({"tag": "text_1", "ids": [kept]})).unwrap();
        let r = s.execute("xml.autoTag", &json!({})).unwrap();
        let tags: Vec<(serde_json::Value, String)> =
            r["tagged"].as_array().unwrap().iter().map(|t| (t["id"].clone(), t["tag"].as_str().unwrap().to_string())).collect();
        // Reading order; a valid name is the tag; `text_1` is taken, so the counter skips it.
        assert_eq!(tags, vec![(head.clone(), "headline".into()), (body.clone(), "text_2".into()), (photo.clone(), "image_1".into())]);
        let d = &s.doc().unwrap().doc;
        assert!(d.item(designcraft_doc::ItemId(shape.as_u64().unwrap())).unwrap().xml_tag.is_empty(), "a plain shape stays untagged");
        assert_eq!(d.item(designcraft_doc::ItemId(kept.as_u64().unwrap())).unwrap().xml_tag, "text_1");
        assert!(d.xml.tags.iter().any(|t| t.name == "headline") && d.xml.tags.iter().any(|t| t.name == "image_1"));
        // Running again tags nothing new; overwrite with only images retags the photo.
        assert_eq!(s.execute("xml.autoTag", &json!({})).unwrap()["tagged"].as_array().unwrap().len(), 0);
        let r = s.execute("xml.autoTag", &json!({"kinds": ["image"], "overwrite": true, "useNames": false})).unwrap();
        assert_eq!(r["tagged"][0]["tag"], "image_1");
        assert!(s.execute("xml.autoTag", &json!({"kinds": ["table"]})).is_err());
        // The tagged frames take imported XML.
        let r = s.execute("file.importXml", &json!({"text": "<Root><headline>New head</headline><text_2>New body</text_2></Root>"})).unwrap();
        assert_eq!(r["placed"], 2);
    }

    #[test]
    fn import_xml_keeps_the_frames_character_formatting() {
        use designcraft_doc::StoryId;
        let mut s = Session::new();
        s.execute("file.new", &json!({})).unwrap();
        let f = s
            .execute("frame.create", &json!({"rect": [72, 72, 400, 300], "content": "text", "text": "Contains: 300 g/L\nSecond\n", "caret": false}))
            .unwrap();
        let sid = StoryId(f["story"].as_u64().unwrap());
        s.execute("text.select", &json!({"story": sid.0, "anchor": 0, "focus": 25})).unwrap();
        s.execute("type.char", &json!({"attrs": {"size": 20.0}})).unwrap();
        s.execute("text.select", &json!({"story": sid.0, "anchor": 0, "focus": 9})).unwrap();
        s.execute("type.char", &json!({"attrs": {"fontStyle": "Bold"}})).unwrap();
        s.execute("xml.tag", &json!({"tag": "c", "ids": [f["id"]]})).unwrap();
        let before = s.doc().unwrap().doc.stories[&sid].clone();
        let story = |s: &Session| s.doc().unwrap().doc.stories[&sid].clone();
        let size = |st: &designcraft_doc::Story, at: usize| st.format_after(at).over.size;
        let style = |st: &designcraft_doc::Story, at: usize| st.format_after(at).over.font_style.clone();

        // The same text changes nothing.
        s.execute("file.importXml", &json!({"text": "<Root><c>Contains: 300 g/L\nSecond</c></Root>"})).unwrap();
        assert_eq!(story(&s).text, before.text);
        assert_eq!(story(&s).chars, before.chars, "runs unchanged");

        // A changed number keeps the bold label and the size; a new paragraph copies the last.
        s.execute("file.importXml", &json!({"text": "<Root><c>Contains: 400 g/L\nSecond\nThird</c></Root>"})).unwrap();
        let st = story(&s);
        assert_eq!(st.text, "Contains: 400 g/L\nSecond\nThird\n", "the trailing return stays");
        assert_eq!(style(&st, 0), before.format_after(0).over.font_style, "label stays bold");
        assert_ne!(style(&st, 10), style(&st, 0), "the number stays regular");
        assert_eq!(size(&st, 10), Some(20.0));
        assert_eq!(size(&st, st.text.find("Third").unwrap()), Some(20.0), "a new paragraph takes the last one's format");

        // Fewer paragraphs: the surplus goes, formatting stays.
        s.execute("file.importXml", &json!({"text": "<Root><c>Enthält: 300 g/L</c></Root>"})).unwrap();
        let st = story(&s);
        assert_eq!(st.text, "Enthält: 300 g/L\n");
        assert_eq!(size(&st, 0), Some(20.0));
        assert_eq!(st.paras.len(), 2);
    }

    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    fn import_xml_image_fit_attribute() {
        use designcraft_doc::{Content, ItemId};
        let dir = std::env::temp_dir().join(format!("dc-xml-fit-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let png = designcraft_render::Rendered { width: 40, height: 20, pixels: vec![200u8; 40 * 20 * 4] }.to_png();
        std::fs::write(dir.join("wide.png"), &png).unwrap();
        let mut s = Session::new();
        s.execute("file.new", &json!({})).unwrap();
        let frame = |s: &mut Session, y: i32| {
            ItemId(s.execute("frame.create", &json!({"rect": [72, y, 272, y + 200], "content": "graphic"})).unwrap()["id"].as_u64().unwrap())
        };
        let (fill, fit, bad) = (frame(&mut s, 72), frame(&mut s, 300), frame(&mut s, 520));
        for (id, tag) in [(fill, "a"), (fit, "b"), (bad, "c")] {
            s.execute("xml.tag", &json!({"tag": tag, "ids": [id.0]})).unwrap();
        }
        let xml = "<Root><a href=\"wide.png\"/><b href=\"wide.png\" fit=\"fitProportionally\"/><c href=\"wide.png\" fit=\"squash\"/></Root>";
        let r = s.execute("file.importXml", &json!({"text": xml, "imageDir": dir.to_string_lossy()})).unwrap();
        assert_eq!(r["placed"], 2, "{r}");
        assert_eq!(r["failed"][0]["tag"], "c", "{r}");
        // An unknown fit places nothing: the frame stays empty, as the report says.
        assert!(r["failed"][0]["error"].as_str().unwrap().contains("unknown fit `squash`"), "{r}");
        assert!(!matches!(s.doc().unwrap().doc.item(bad).unwrap().content, Content::Graphic(_)), "not placed");
        let width = |s: &Session, id: ItemId| {
            let d = &s.doc().unwrap().doc;
            let Content::Graphic(g) = &d.item(id).unwrap().content else { panic!("graphic") };
            g.xf.as_coeffs()[0] * g.size.0
        };
        // A 2:1 image in a square frame: filling covers the height, fitting shows the whole width.
        assert!((width(&s, fill) - 400.0).abs() < 0.5, "{}", width(&s, fill));
        assert!((width(&s, fit) - 200.0).abs() < 0.5, "{}", width(&s, fit));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    fn import_xml_places_images_from_href() {
        use designcraft_doc::{Content, ItemId};
        let dir = std::env::temp_dir().join(format!("dc-xml-href-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join("img")).unwrap();
        let png = designcraft_render::Rendered { width: 40, height: 20, pixels: vec![200u8; 40 * 20 * 4] }.to_png();
        std::fs::write(dir.join("img").join("cover photo.png"), &png).unwrap();
        std::fs::write(dir.join("logo.png"), &png).unwrap();
        let mut s = Session::new();
        s.execute("file.new", &json!({})).unwrap();
        let frame = |s: &mut Session, rect: [i32; 4], content: &str| {
            ItemId(
                s.execute("frame.create", &json!({"rect": rect, "content": content, "text": "x", "caret": false})).unwrap()["id"].as_u64().unwrap(),
            )
        };
        let head = frame(&mut s, [72, 72, 400, 120], "text");
        let photo = frame(&mut s, [72, 200, 300, 400], "graphic");
        let logo = frame(&mut s, [72, 500, 172, 550], "graphic");
        let spare = frame(&mut s, [72, 600, 172, 650], "graphic");
        s.execute("xml.tag", &json!({"tag": "headline", "ids": [head.0]})).unwrap();
        s.execute("xml.tag", &json!({"tag": "photo", "ids": [photo.0, spare.0]})).unwrap();
        s.execute("xml.tag", &json!({"tag": "logo", "ids": [logo.0]})).unwrap();
        let logo_url = format!("file:///{}", dir.join("logo.png").to_string_lossy().replace('\\', "/"));
        let xml = format!(
            "<Root><headline>Spring</headline><photo href=\"img/cover%20photo.png\"/><logo href=\"{logo_url}\"/><photo href=\"missing.png\"/><headline href=\"img/x.png\"/></Root>"
        );
        let path = dir.join("content.xml");
        std::fs::write(&path, xml).unwrap();
        let r = s.execute("file.importXml", &json!({"path": path.to_string_lossy()})).unwrap();
        // Text, a relative href against the XML's folder and a file:// URL; the missing file is
        // reported, and a text frame doesn't take an image.
        assert_eq!((r["placed"].as_u64(), r["unmatched"].as_u64()), (Some(3), Some(1)), "{r}");
        assert_eq!(r["failed"][0]["href"], "missing.png", "{r}");
        let d = &s.doc().unwrap().doc;
        for id in [photo, logo] {
            assert!(matches!(d.item(id).unwrap().content, Content::Graphic(_)), "{id:?} holds the image");
        }
        assert!(!matches!(d.item(spare).unwrap().content, Content::Graphic(_)));
        let Content::Graphic(g) = &d.item(photo).unwrap().content else { panic!("graphic") };
        assert!(d.assets.get(&g.asset).unwrap().link.as_deref().unwrap().ends_with("cover photo.png"));
        // One undo takes back the whole import.
        s.execute("edit.undo", &json!({})).unwrap();
        let d = &s.doc().unwrap().doc;
        assert!(!matches!(d.item(photo).unwrap().content, Content::Graphic(_)));
        assert_eq!(d.stories[&d.item(head).unwrap().text_frame().unwrap().story].text, "x");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn tag_export_and_import_xml() {
        let mut s = Session::new();
        s.execute("file.new", &json!({})).unwrap();
        s.execute("style.paragraph.create", &json!({"name": "Head"})).unwrap();
        let a = s
            .execute("frame.create", &json!({"rect": [72, 72, 400, 150], "content": "text", "text": "Title A\nBody & more", "caret": false}))
            .unwrap();
        s.execute("text.select", &json!({"story": a["story"], "anchor": 0, "focus": 0})).unwrap();
        s.execute("style.paragraph.apply", &json!({"name": "Head"})).unwrap();
        let b = s.execute("frame.create", &json!({"rect": [72, 300, 400, 350], "content": "text", "text": "Caption", "caret": false})).unwrap();
        s.execute("xml.tag", &json!({"tag": "story", "ids": [a["id"]]})).unwrap();
        s.execute("xml.tag", &json!({"tag": "caption", "ids": [b["id"]]})).unwrap();
        s.execute("xml.mapStyle", &json!({"style": "Head", "tag": "title"})).unwrap();
        let x = s.execute("file.exportXml", &json!({})).unwrap()["text"].as_str().unwrap().to_string();
        assert!(
            x.contains("<story>")
                && x.contains("<title>Title A</title>")
                && x.contains("Body &amp; more")
                && x.contains("<caption>Caption</caption>"),
            "{x}"
        );
        assert!(x.find("<story>").unwrap() < x.find("<caption>").unwrap(), "reading order");
        let st = s.execute("xml.structure", &json!({})).unwrap();
        assert_eq!(st["elements"].as_array().unwrap().len(), 2);
        // Import new content into the same structure.
        let r = s
            .execute(
                "file.importXml",
                &json!({"text": "<Root><story><title>New title</title><p>New body</p></story><caption>New caption</caption><extra/></Root>"}),
            )
            .unwrap();
        assert_eq!((r["placed"].as_u64(), r["unmatched"].as_u64()), (Some(2), Some(1)));
        assert_eq!(r["unmatchedTags"], json!(["extra"]));
        let d = &s.doc().unwrap().doc;
        let sa = d.stories[&designcraft_doc::StoryId(a["story"].as_u64().unwrap())].clone();
        assert_eq!(sa.text, "New title\nNew body");
        assert_eq!(sa.paras[0].style, "Head", "mapped tag → paragraph style");
        assert_eq!(d.stories[&designcraft_doc::StoryId(b["story"].as_u64().unwrap())].text, "New caption");
        assert!(s.execute("xml.tag", &json!({"tag": "1bad", "ids": [b["id"]]})).is_err());
        // Inline tagging: part of a paragraph becomes an element.
        let f = s
            .execute("frame.create", &json!({"rect": [72, 500, 400, 560], "content": "text", "text": "See the glossary entry", "caret": false}))
            .unwrap();
        s.execute("xml.tag", &json!({"tag": "note", "ids": [f["id"]]})).unwrap();
        s.execute("text.select", &json!({"story": f["story"], "anchor": 8, "focus": 16})).unwrap();
        s.execute("xml.tagText", &json!({"tag": "term"})).unwrap();
        let x = s.execute("file.exportXml", &json!({})).unwrap()["text"].as_str().unwrap().to_string();
        assert!(x.contains("<note>See the <term>glossary</term> entry</note>"), "{x}");
        // Tag markers draw in the tag's colour (screen view only).
        let d = s.doc().unwrap().doc.clone();
        let col = d.xml.tags.iter().find(|t| t.name == "term").unwrap().color;
        let mut rr = designcraft_render::Renderer::new();
        rr.threads = 0;
        let count = |opts: &designcraft_render::RenderOptions, rr: &mut designcraft_render::Renderer| {
            let img = rr.render_page(&d, &s.cache, 0, 2.0, false, opts).unwrap();
            img.pixels
                .as_chunks::<4>()
                .0
                .iter()
                .filter(|p| {
                    (p[0] as i32 - col[0] as i32).abs() < 30 && (p[1] as i32 - col[1] as i32).abs() < 30 && (p[2] as i32 - col[2] as i32).abs() < 30
                })
                .count()
        };
        let plain = count(&Default::default(), &mut rr);
        let marked = count(&designcraft_render::RenderOptions { tag_markers: true, ..Default::default() }, &mut rr);
        assert!(marked > plain + 10, "{plain} {marked}");
    }
}
