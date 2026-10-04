//! Structure and XML: tags on frames (Tags panel), paragraph styles mapped to tags, the structure
//! tree, XML export and import (content flows into the frames with matching tags).

use designcraft_doc::{Content, Document, Item, ItemId, ParaFormat, Story, StoryId, XmlTag};
use serde_json::{Value, json};

use super::{CommandSpec, bad, cmd, has_doc, ids_param, ok, str_param};
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
    let mut keyed: Vec<((usize, i64, i64), ItemId)> = Vec::new();
    for (si, sp) in d.spreads.iter().enumerate() {
        let first = d.first_page_of_spread(si);
        for top in &sp.items {
            top.walk(&mut |it: &Item| {
                if !it.xml_tag.is_empty() {
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

fn esc(s: &str) -> String {
    s.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;").replace('"', "&quot;")
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

/// One element under the root: its name and either text or (child name, text) paragraphs.
struct Element {
    name: String,
    text: String,
    children: Vec<(String, String)>,
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
                    2 => out.push(Element { name, text: String::new(), children: vec![] }),
                    3 => child = Some((name, String::new())),
                    _ => {}
                }
            }
            Event::Empty(e) if depth == 1 => {
                out.push(Element { name: String::from_utf8_lossy(e.local_name().as_ref()).to_string(), text: String::new(), children: vec![] });
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
                    _ => unreachable!(),
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
            "xml.mapStyle",
            "Map Styles to Tags",
            [],
            None,
            "{style: paragraph style, tag | null} — that style's paragraphs export as `<tag>` elements and import back with it",
            has_doc,
            |s, p| {
                let style = str_param(p, "style").unwrap_or("").to_string();
                let tag = str_param(p, "tag").map(str::to_string);
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
                    std::fs::write(path, text.as_bytes()).map_err(|e| crate::EngineError::Other(format!("{path}: {e}")))?;
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
            "{path | text} — each element's content goes into the next text frame with that tag (child elements of mapped tags become paragraphs in their style) → {placed, unmatched}",
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
            let t = std::fs::read_to_string(path).map_err(|e| crate::EngineError::Other(format!("{path}: {e}")))?;
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
    let d = &s.doc()?.doc;
    // Frames by tag, in reading order; each element takes the next free one.
    let mut slots: Vec<(String, StoryId, bool)> =
        tagged(d).into_iter().filter_map(|id| d.item(id)).filter_map(|it| it.text_frame().map(|tf| (it.xml_tag.clone(), tf.story, false))).collect();
    let reverse: Vec<(String, String)> = d.xml.style_map.iter().map(|(st, t)| (t.clone(), st.clone())).collect();
    let mut fills: Vec<(StoryId, Vec<(Option<String>, String)>)> = Vec::new();
    let mut unmatched = 0;
    for el in elements {
        let Some(slot) = slots.iter_mut().find(|(t, _, used)| *t == el.name && !used) else {
            unmatched += 1;
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
    let placed = fills.len();
    s.edit(|d, _| {
        for (sid, paras) in fills {
            let Some(st) = d.story_mut(sid) else { continue };
            let base = st.paras.first().cloned().unwrap_or_default();
            let text = paras.iter().map(|(_, t)| t.as_str()).collect::<Vec<_>>().join("\n");
            let frames = std::mem::take(&mut st.frames);
            let mut fresh = Story::with_text(sid, &text, ParaFormat { table: None, ..base.clone() });
            for (i, (style, _)) in paras.iter().enumerate() {
                if let (Some(sty), Some(pf)) = (style, fresh.paras.get_mut(i)) {
                    pf.style = sty.clone();
                }
            }
            fresh.frames = frames;
            fresh.rev = st.rev + 1;
            *st = fresh;
        }
        Ok(json!({"placed": placed, "unmatched": unmatched}))
    })
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
