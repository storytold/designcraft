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
            top.walk_placed(&mut |it: &Item, outer| {
                if keep(it) {
                    let b = it.placed_bounds(outer);
                    let page = first + sp.page_at_x(b.center().x).unwrap_or(0);
                    keyed.push(((page, (b.y0 * 10.0) as i64, (b.x0 * 10.0) as i64), it.id));
                }
            });
        }
    }
    keyed.sort_by_key(|(k, _)| *k);
    keyed.into_iter().map(|(_, i)| i).collect()
}

/// Objects on the document pages whose centre lies on a page, not on the pasteboard.
fn on_pages(d: &Document) -> std::collections::HashSet<ItemId> {
    let mut out = std::collections::HashSet::new();
    for sp in &d.spreads {
        for top in &sp.items {
            // Placed by their groups' transforms: a group moved onto a page brings its frames.
            top.walk_placed(&mut |it: &Item, outer| {
                let c = it.placed_bounds(outer).center();
                if sp.pages.iter().any(|pg| c.x >= pg.x && c.x <= pg.x + pg.width && c.y >= 0.0 && c.y <= pg.height) {
                    out.insert(it.id);
                }
            });
        }
    }
    out
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
    // Frames on the pasteboard aren't the page's: they would take numbers before frames on it.
    let on_page = on_pages(d);
    let ids: Vec<ItemId> = in_reading_order(d, wanted).into_iter().filter(|id| on_page.contains(id)).collect();
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

/// A character style's element name: the style's name, characters a name can't hold as `_`.
fn style_element(style: &str) -> String {
    let mut n: String = style.chars().map(|c| if c.is_alphanumeric() || matches!(c, '_' | '-' | '.') { c } else { '_' }).collect();
    if !valid_name(&n) {
        n.insert(0, '_');
    }
    n
}

/// A run's character style, if it has one.
fn char_style(f: &designcraft_doc::CharFormat) -> Option<&str> {
    Some(f.style.as_str()).filter(|s| !s.is_empty() && *s != designcraft_doc::story::NO_CHAR_STYLE)
}

/// A paragraph's text with inline-tagged runs as elements, and character-styled runs (inside
/// those) as elements named after their style.
fn para_xml(st: &designcraft_doc::Story, r: std::ops::Range<usize>) -> String {
    let mut out = String::new();
    let (mut open, mut open_style): (Option<String>, Option<String>) = (None, None);
    for (rr, f) in st.runs() {
        let (a, b) = (rr.start.max(r.start), rr.end.min(r.end));
        if a >= b {
            continue;
        }
        let tag = f.over.xml_tag.clone().filter(|t| !t.is_empty());
        let style = char_style(f).map(style_element);
        if tag != open || style != open_style {
            if let Some(s) = open_style.take() {
                out.push_str(&format!("</{s}>"));
            }
            if tag != open {
                if let Some(t) = open.take() {
                    out.push_str(&format!("</{t}>"));
                }
                if let Some(t) = &tag {
                    out.push_str(&format!("<{t}>"));
                }
                open = tag;
            }
            if let Some(s) = &style {
                out.push_str(&format!("<{s}>"));
            }
            open_style = style;
        }
        out.push_str(&esc(&plain(&st.text[a..b])));
    }
    if let Some(s) = open_style {
        out.push_str(&format!("</{s}>"));
    }
    if let Some(t) = open {
        out.push_str(&format!("</{t}>"));
    }
    out
}

const AID: &str = "http://ns.adobe.com/AdobeInDesign/4.0/";

/// Paragraph `i` (range `r`) of `st`: its text, or its table as InDesign writes one.
fn para_or_table_xml(st: &Story, i: usize, r: std::ops::Range<usize>) -> String {
    match st.paras.get(i).and_then(|p| p.table).and_then(|id| st.tables.get(&id)) {
        Some(t) => table_xml(t),
        None => para_xml(st, r),
    }
}

/// A table as InDesign's XML export writes one: `aid:` table and cells, each cell's paragraphs
/// on lines of their own (tables in cells too).
fn table_xml(t: &designcraft_doc::Table) -> String {
    let (nr, nc) = (t.nrows(), t.ncols());
    let mut out = format!("<Table xmlns:aid=\"{AID}\" aid:table=\"table\" aid:trows=\"{nr}\" aid:tcols=\"{nc}\">");
    let owners = t.owners();
    let (head, foot) = (t.header_rows(), t.footer_rows());
    for r in 0..nr {
        for c in 0..nc {
            if owners.get(r * nc + c) != Some(&(r, c)) {
                continue;
            }
            let Some(cell) = t.cell(r, c) else { continue };
            let rs = (cell.row_span.max(1) as usize).min(nr - r);
            let cs = (cell.col_span.max(1) as usize).min(nc - c);
            let width: f64 = t.columns[c..c + cs].iter().map(|col| col.width).sum();
            let section = if r < head {
                " aid:theader=\"\""
            } else if r >= nr - foot {
                " aid:tfooter=\"\""
            } else {
                ""
            };
            let id = if cell.source_id.is_empty() { String::new() } else { format!(" id=\"{}\"", esc(&cell.source_id)) };
            out.push_str(&format!("<Cell aid:table=\"cell\"{section} aid:crows=\"{rs}\" aid:ccols=\"{cs}\" aid:ccolwidth=\"{width}\"{id}>"));
            let text: Vec<String> = cell.text.para_ranges().into_iter().enumerate().map(|(i, r)| para_or_table_xml(&cell.text, i, r)).collect();
            out.push_str(&text.join("\n"));
            out.push_str("</Cell>");
        }
    }
    out.push_str("</Table>");
    out
}

/// The document's tagged content as XML.
pub fn export_xml(d: &Document) -> String {
    let mut out = format!("<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?>\n<{}>\n", root_name(d));
    // A story threaded through several tagged frames is written once, at its first.
    let mut written = std::collections::HashSet::new();
    for id in tagged(d) {
        let Some(it) = d.item(id) else { continue };
        let tag = &it.xml_tag;
        match &it.content {
            Content::Text(tf) => {
                let Some(st) = d.story(tf.story).filter(|_| written.insert(tf.story)) else { continue };
                let mapped = |style: &str| d.xml.style_map.iter().find(|(s, _)| s == style).map(|(_, t)| t.clone());
                // Paragraph text with inline tags already escaped.
                let paras: Vec<(Option<String>, String)> =
                    st.para_ranges().into_iter().enumerate().map(|(i, r)| (mapped(&st.paras[i].style), para_or_table_xml(st, i, r))).collect();
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
    body: Body,
    children: Vec<(String, Body)>,
    image: Option<Image>,
}

/// Text and the tables in it: each table's place is a table anchor in `text`, in order.
#[derive(Default)]
struct Body {
    text: String,
    tables: Vec<XTable>,
    /// A table cell's ID (its `id`).
    id: Option<String>,
    /// Inline elements: the text they hold (byte range of `text`) and their names, inner ones
    /// first.
    runs: Vec<(std::ops::Range<usize>, String)>,
}

impl Body {
    /// Whether the body, or a cell of a table in it, has a run `styled` takes.
    fn has_style(&self, styled: &dyn Fn(&str) -> bool) -> bool {
        self.runs.iter().any(|(_, n)| styled(n)) || self.tables.iter().flat_map(|t| &t.cells).any(|c| c.has_style(styled))
    }

    /// The body's paragraphs (its lines, `trim` trimmed off each and off the whole), each with
    /// its runs in paragraph offsets.
    fn paras(&self, style: Option<String>, trim: impl Fn(char) -> bool + Copy) -> Vec<XPara> {
        let lead = self.text.len() - self.text.trim_start_matches(trim).len();
        let all = self.text.trim_matches(trim);
        let mut at = lead;
        let mut out = Vec::new();
        for line in all.split('\n') {
            let start = at + line.len() - line.trim_start_matches(trim).len();
            let text = line.trim_matches(trim);
            out.push(XPara { style: style.clone(), text: text.to_string(), runs: self.runs_in(start, text.len()) });
            at += line.len() + 1;
        }
        out
    }

    /// The whole body as one paragraph, white space trimmed.
    fn para(&self, style: Option<String>) -> XPara {
        let text = self.text.trim();
        let start = self.text.len() - self.text.trim_start().len();
        XPara { style, text: text.to_string(), runs: self.runs_in(start, text.len()) }
    }

    /// The runs within `len` bytes from `start`, in offsets from `start`.
    fn runs_in(&self, start: usize, len: usize) -> Vec<(std::ops::Range<usize>, String)> {
        let end = start + len;
        self.runs
            .iter()
            .filter(|(r, _)| r.start < end && r.end > start)
            .map(|(r, n)| (r.start.max(start) - start..r.end.min(end) - start, n.clone()))
            .collect()
    }
}

/// A paragraph to put in: its mapped paragraph style, text and inline runs.
struct XPara {
    style: Option<String>,
    text: String,
    runs: Vec<(std::ops::Range<usize>, String)>,
}

/// A `<Table>`: its `<Cell>`s in order.
#[derive(Default)]
struct XTable {
    cells: Vec<Body>,
}

/// What is open while parsing below an element.
enum Open {
    /// A child element of the element (a mapped paragraph).
    Child(String, Body),
    Table(XTable),
    Cell(Body),
    /// Anything else (an inline element): its text belongs to what holds it, from byte `.1` on.
    Other(String, usize),
}

/// The body text and tables go into: the innermost open cell or child, else the element's.
fn open_body<'a>(stack: &'a mut [Open], el: &'a mut Element) -> &'a mut Body {
    for o in stack.iter_mut().rev() {
        match o {
            Open::Cell(b) | Open::Child(_, b) => return b,
            Open::Table(_) | Open::Other(..) => {}
        }
    }
    &mut el.body
}

fn is_table(e: &quick_xml::events::BytesStart) -> bool {
    e.local_name().as_ref() == b"Table"
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

/// A table cell's `id`, unescaped.
fn cell_id(e: &quick_xml::events::BytesStart) -> std::result::Result<Option<String>, String> {
    for a in e.attributes() {
        let a = a.map_err(|e| e.to_string())?;
        if a.key.as_ref() == b"id" {
            return Ok(Some(a.normalized_value(quick_xml::XmlVersion::Implicit1_0).map_err(|e| e.to_string())?.to_string()));
        }
    }
    Ok(None)
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

/// The elements under the root. Elements `inline` names are runs in the text holding them at
/// any depth (character styles), never child elements.
fn parse(xml: &str, inline: &dyn Fn(&str) -> bool) -> std::result::Result<Vec<Element>, String> {
    use quick_xml::events::Event;
    let mut r = quick_xml::Reader::from_str(xml);
    let mut depth = 0usize;
    let mut out: Vec<Element> = Vec::new();
    // What is open below the current element (depth 3 and deeper).
    let mut stack: Vec<Open> = Vec::new();
    let anchor = designcraft_doc::story::TABLE_ANCHOR.to_string();
    loop {
        match r.read_event().map_err(|e| e.to_string())? {
            Event::Start(e) => {
                depth += 1;
                let name = String::from_utf8_lossy(e.local_name().as_ref()).to_string();
                if depth == 2 {
                    stack.clear();
                    out.push(Element { name, body: Body::default(), children: vec![], image: image_attrs(&e)? });
                    continue;
                }
                let Some(el) = out.last_mut().filter(|_| depth > 2) else { continue };
                let open = if is_table(&e) {
                    // The table's place in the text holding it.
                    open_body(&mut stack, el).text.push_str(&anchor);
                    Open::Table(XTable::default())
                } else if name == "Cell" && matches!(stack.last(), Some(Open::Table(_))) {
                    Open::Cell(Body { id: cell_id(&e)?, ..Body::default() })
                } else if depth == 3 && !inline(&name) {
                    Open::Child(name, Body::default())
                } else {
                    let start = open_body(&mut stack, el).text.len();
                    Open::Other(name, start)
                };
                stack.push(open);
            }
            Event::Empty(e) if depth == 1 => {
                out.push(Element {
                    name: String::from_utf8_lossy(e.local_name().as_ref()).to_string(),
                    body: Body::default(),
                    children: vec![],
                    image: image_attrs(&e)?,
                });
            }
            // An empty cell still takes its place.
            Event::Empty(e) if e.local_name().as_ref() == b"Cell" => {
                if let Some(Open::Table(t)) = stack.last_mut() {
                    t.cells.push(Body { id: cell_id(&e)?, ..Body::default() });
                }
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
                // Text between a table's cells (white space) belongs to no paragraph.
                if depth >= 2
                    && !matches!(stack.last(), Some(Open::Table(_)))
                    && let Some(el) = out.last_mut()
                {
                    open_body(&mut stack, el).text.push_str(&s);
                }
            }
            Event::End(_) => {
                if depth > 2
                    && let Some(open) = stack.pop()
                    && let Some(el) = out.last_mut()
                {
                    match open {
                        Open::Table(t) => open_body(&mut stack, el).tables.push(t),
                        Open::Cell(b) => {
                            if let Some(Open::Table(t)) = stack.last_mut() {
                                t.cells.push(b);
                            }
                        }
                        Open::Child(n, b) => el.children.push((n, b)),
                        Open::Other(n, start) => {
                            let b = open_body(&mut stack, el);
                            if start < b.text.len() {
                                let end = b.text.len();
                                b.runs.push((start..end, n));
                            }
                        }
                    }
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
    // Character styles by their element name.
    let char_styles: std::collections::HashMap<String, String> =
        s.doc()?.doc.styles.character.iter().map(|c| (style_element(&c.name), c.name.clone())).collect();
    let elements = parse(&text, &|n| char_styles.contains_key(n)).map_err(|e| bad(ID, format!("not well-formed XML: {e}")))?;
    // XML with character-styled runs says where each style goes; XML without (written before
    // export wrote them) leaves the styles as they are.
    let styled = |n: &str| char_styles.contains_key(n);
    let char_styles =
        elements.iter().any(|e| e.body.has_style(&styled) || e.children.iter().any(|(_, b)| b.has_style(&styled))).then_some(&char_styles);
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
    let mut fills: Vec<(StoryId, Vec<XPara>, Vec<XTable>)> = Vec::new();
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
        // An empty element of a graphic frame's tag (an empty frame, as export writes one): nothing
        // to put in it.
        if el.body.text.trim().is_empty()
            && el.children.is_empty()
            && let Some(slot) = image_slots.iter_mut().find(|(t, _, used)| *t == el.name && !used)
        {
            slot.2 = true;
            continue;
        }
        let Some(slot) = slots.iter_mut().find(|(t, _, used)| *t == el.name && !used) else {
            miss(&el.name);
            continue;
        };
        slot.2 = true;
        // A story an earlier element filled (another frame of its thread) keeps that text.
        if fills.iter().any(|(sid, ..)| *sid == slot.1) {
            continue;
        }
        // Paragraphs, and the tables their anchors stand for, in order. Lines lose the XML's
        // indentation (spaces, tabs), not the text's own white space (no-break spaces).
        let indent = |c: char| matches!(c, ' ' | '\t' | '\r');
        let (paras, tables): (Vec<XPara>, Vec<XTable>) = if el.children.is_empty() {
            (el.body.paras(None, indent), el.body.tables)
        } else {
            let mut tables = Vec::new();
            let paras = el
                .children
                .into_iter()
                .map(|(t, b)| {
                    let p = b.para(reverse.iter().find(|(tag, _)| *tag == t).map(|(_, st)| st.clone()));
                    tables.extend(b.tables);
                    p
                })
                .collect();
            (paras, tables)
        };
        fills.push((slot.1, paras, tables));
    }
    let mut placed = fills.len();
    let mut fx = Fill { cs: char_styles, lost_cells: 0 };
    if !fills.is_empty() {
        fill_stories(s, fills, &mut fx)?;
    }
    // Each image fills its frame proportionally unless `fit` says otherwise; `crop` picks a PDF's
    // box. One that can't be placed is reported and the rest of the import goes on.
    let mut failed: Vec<Value> = Vec::new();
    for (frame, tag, image) in images {
        let path = href_path(&image.href, base.as_deref());
        // The file the frame holds already (as export wrote it): unchanged, nothing to place.
        let holds = {
            let d = &s.doc()?.doc;
            d.item(frame)
                .and_then(|it| match &it.content {
                    Content::Graphic(g) => d.assets.get(&g.asset),
                    _ => None,
                })
                .and_then(|a| a.link.clone().or_else(|| Some(a.name.clone())))
        };
        if image.fit.is_none() && image.crop.is_none() && holds.is_some_and(|l| image.href == format!("file://{l}") || path == l) {
            placed += 1;
            continue;
        }
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
    Ok(json!({"placed": placed, "unmatched": unmatched, "unmatchedTags": unmatched_tags, "lostCells": fx.lost_cells, "failed": failed}))
}

/// Character styles by element name, when the XML says where they go.
type CharStyles = std::collections::HashMap<String, String>;

/// How an import fills stories: `cs` as for [`fill_story`]; counts the XML's cells with no cell
/// to go into.
struct Fill<'a> {
    cs: Option<&'a CharStyles>,
    lost_cells: usize,
}

/// Replaces each story's text with the paragraphs, in their mapped styles.
fn fill_stories(s: &mut Session, fills: Vec<(StoryId, Vec<XPara>, Vec<XTable>)>, fx: &mut Fill) -> Result<()> {
    s.edit(|d, _| {
        for (sid, paras, tables) in fills {
            if let Some(st) = d.story_mut(sid) {
                fill_story(st, &paras, &tables, fx);
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
/// A table's paragraph is never text: it takes the next of `tables` when the XML has a table
/// there, cell by cell, and is left as it is otherwise. With `fx.cs`, each paragraph's characters
/// take the character styles of the XML's runs, and no style outside them.
fn fill_story(st: &mut Story, paras: &[XPara], tables: &[XTable], fx: &mut Fill) {
    let anchor = designcraft_doc::story::TABLE_ANCHOR;
    let ranges = st.para_ranges();
    let keep_last = ranges.len() > 1 && ranges.last().is_some_and(|r| r.is_empty());
    let old_n = ranges.len() - usize::from(keep_last);
    // A last empty paragraph in the XML is the story's own trailing return.
    let paras = match paras.split_last() {
        Some((last, rest)) if keep_last && last.text.is_empty() && paras.len() == ranges.len() => rest,
        _ => paras,
    };
    let mut next_table = tables.iter();
    for (i, XPara { text: new, .. }) in paras.iter().enumerate() {
        let ranges = st.para_ranges();
        let is_table = new.trim() == anchor.to_string();
        if let Some(tid) = st.paras.get(i).filter(|_| i < old_n).and_then(|p| p.table) {
            if is_table
                && let Some(x) = next_table.next()
                && let Some(t) = st.tables.get_mut(&tid)
            {
                fill_table(std::sync::Arc::make_mut(t), x, fx);
            }
            continue;
        }
        if is_table {
            // A table the story has no place for here: its paragraph stays out.
            next_table.next();
            continue;
        }
        // Table anchors are no text to put in.
        let stripped;
        let new = if new.contains(anchor) {
            stripped = new.replace(anchor, "");
            &stripped
        } else {
            new
        };
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
    // Surplus paragraphs go, last first, except tables (whose paragraphs aren't text).
    for j in (paras.len().max(1)..old_n).rev() {
        if st.paras.get(j).is_some_and(|p| p.table.is_some()) {
            continue;
        }
        let ranges = st.para_ranges();
        if let (Some(prev), Some(me)) = (ranges.get(j - 1), ranges.get(j)) {
            st.delete(prev.end..me.end);
        }
    }
    for (i, p) in paras.iter().enumerate() {
        if let (Some(sty), Some(pf)) = (&p.style, st.paras.get_mut(i))
            && pf.table.is_none()
        {
            pf.style = sty.clone();
        }
    }
    if let Some(cs) = fx.cs {
        let ranges = st.para_ranges();
        for (i, p) in paras.iter().enumerate() {
            let Some(r) = ranges.get(i).filter(|r| st.paras[i].table.is_none() && st.text[(*r).clone()] == p.text) else { continue };
            let none = designcraft_doc::story::NO_CHAR_STYLE;
            st.format_chars(r.clone(), |f| f.style = none.into());
            // Outer runs first, so inner ones win.
            let mut runs: Vec<_> = p.runs.iter().filter_map(|(x, n)| cs.get(n).map(|sty| (x.clone(), sty))).collect();
            runs.sort_by_key(|(x, _)| (x.start, std::cmp::Reverse(x.end)));
            for (x, sty) in runs {
                st.format_chars(r.start + x.start..r.start + x.end, |f| f.style = sty.clone());
            }
        }
    }
}

/// The XML table's cells put into `t`'s cells, each in place like a story: a cell with an `id`
/// into the cell of that ID (none there: it is lost), one without into the next cell in order
/// (row by row, a merged cell once).
fn fill_table(t: &mut designcraft_doc::Table, x: &XTable, fx: &mut Fill) {
    let (nr, nc) = (t.nrows(), t.ncols());
    let owners = t.owners();
    let order: Vec<(usize, usize)> = (0..nr * nc).filter(|&i| owners.get(i) == Some(&(i / nc, i % nc))).map(|i| (i / nc, i % nc)).collect();
    // Cells by ID; an ID two cells have (a copied row) names neither.
    let mut by_id: std::collections::HashMap<&str, Option<(usize, usize)>> = std::collections::HashMap::new();
    for &(r, c) in &order {
        if let Some(cell) = t.cell(r, c).filter(|cell| !cell.source_id.is_empty()) {
            by_id.entry(cell.source_id.as_str()).and_modify(|at| *at = None).or_insert(Some((r, c)));
        }
    }
    let targets: Vec<Option<(usize, usize)>> = {
        let mut next = order.iter();
        x.cells
            .iter()
            .map(|xc| match &xc.id {
                Some(id) => by_id.get(id.as_str()).copied().flatten(),
                None => next.next().copied(),
            })
            .collect()
    };
    for (xc, at) in x.cells.iter().zip(targets) {
        let Some(cell) = at.and_then(|(r, c)| t.cell_mut(r, c)) else {
            fx.lost_cells += 1;
            continue;
        };
        // Export writes a cell's paragraphs as they are: nothing to trim.
        fill_story(&mut cell.text, &xc.paras(None, |_| false), &xc.tables, fx);
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
        // A frame on the pasteboard isn't the page's; a text frame made a graphic frame is one.
        let aside = frame(&mut s, [-300, 72, -100, 120], "text");
        let converted = frame(&mut s, [450, 300, 550, 400], "text");
        s.execute("object.content", &json!({"type": "graphic", "ids": [converted]})).unwrap();
        let r = s.execute("xml.autoTag", &json!({})).unwrap();
        let tagged: Vec<serde_json::Value> = r["tagged"].as_array().unwrap().iter().map(|t| t["id"].clone()).collect();
        assert_eq!(tagged, vec![converted.clone()], "{r}");
        assert_eq!(r["tagged"][0]["kind"], "image");
        assert!(!tagged.contains(&aside));
        // Made unassigned again, it's a plain shape.
        s.execute("object.content", &json!({"type": "unassigned", "ids": [converted]})).unwrap();
        assert!(!s.doc().unwrap().doc.item(designcraft_doc::ItemId(converted.as_u64().unwrap())).unwrap().graphic_frame);
    }

    /// Frames in a group take the group's place: a group moved from the pasteboard onto a page
    /// brings its frames, which autoTag then tags.
    #[test]
    fn auto_tag_places_grouped_frames_by_their_group() {
        let mut s = Session::new();
        s.execute("file.new", &json!({})).unwrap();
        let frame = |s: &mut Session, rect: [i32; 4]| {
            s.execute("frame.create", &json!({"rect": rect, "content": "text", "text": "x", "caret": false})).unwrap()["id"].clone()
        };
        let (a, b) = (frame(&mut s, [-400, 72, -300, 120]), frame(&mut s, [-400, 200, -300, 250]));
        let g = s.execute("object.group", &json!({"ids": [a, b]})).unwrap()["id"].clone();
        s.execute("transform.move", &json!({"ids": [g], "dx": 500, "dy": 0})).unwrap();
        let d = &s.doc().unwrap().doc;
        let group = d.item(designcraft_doc::ItemId(g.as_u64().unwrap())).unwrap();
        assert!(group.xf.translation().x.abs() > 1.0, "the move sits in the group's transform");
        let r = s.execute("xml.autoTag", &json!({})).unwrap();
        let tagged: Vec<serde_json::Value> = r["tagged"].as_array().unwrap().iter().map(|t| t["id"].clone()).collect();
        assert_eq!(tagged, vec![a, b], "{r}");
    }

    /// Character-styled runs are elements named after their style: import puts each style where
    /// the XML has it (a translation moves it), and XML without such elements leaves styles be.
    /// A story threaded through two tagged frames is written once.
    #[test]
    fn character_styles_and_threads_in_xml() {
        use designcraft_doc::StoryId;
        let mut s = Session::new();
        s.execute("file.new", &json!({})).unwrap();
        s.execute("style.character.create", &json!({"name": "bold"})).unwrap();
        let f = s
            .execute("frame.create", &json!({"rect": [72, 72, 500, 200], "content": "text", "text": "Nur innen. Nicht außen! Ende.", "caret": false}))
            .unwrap();
        let sid = StoryId(f["story"].as_u64().unwrap());
        let g = s.execute("frame.create", &json!({"rect": [72, 300, 500, 400], "content": "unassigned"})).unwrap();
        s.execute("text.thread", &json!({"from": f["id"], "to": g["id"]})).unwrap();
        s.edit(|d, _| {
            d.story_mut(sid).unwrap().format_chars(11..24, |c| c.style = "bold".into());
            Ok(())
        })
        .unwrap();
        s.execute("xml.tag", &json!({"tag": "body", "ids": [f["id"], g["id"]]})).unwrap();
        let x = s.execute("file.exportXml", &json!({})).unwrap()["text"].as_str().unwrap().to_string();
        assert!(x.contains("<body>Nur innen. <bold>Nicht außen!</bold> Ende.</body>"), "{x}");
        assert_eq!(x.matches("<body>").count(), 1, "the thread's story once: {x}");
        let bold = |s: &Session| {
            let st = s.doc().unwrap().doc.story(sid).unwrap().clone();
            st.runs().filter(|(_, f)| f.style == "bold").map(|(r, _)| st.text[r].to_string()).collect::<Vec<_>>()
        };
        s.execute("file.importXml", &json!({"text": x})).unwrap();
        assert_eq!(bold(&s), ["Nicht außen!"]);
        s.execute("file.importXml", &json!({"text": "<Root><body><bold>Only</bold> inside. Not outside! End.</body></Root>"})).unwrap();
        assert_eq!(s.doc().unwrap().doc.story(sid).unwrap().text, "Only inside. Not outside! End.");
        assert_eq!(bold(&s), ["Only"], "the style moves with the translation");
        s.execute("file.importXml", &json!({"text": "<Root><body>Only within. Not outside! End.</body></Root>"})).unwrap();
        assert_eq!(bold(&s), ["Only"], "XML without styled runs keeps the styles");
    }

    /// Cells from a file keep its IDs: export names each cell by its `id`, and import puts a cell
    /// into the cell of its ID wherever it is in the XML; an ID the table hasn't is lost.
    #[test]
    fn table_cells_are_matched_by_id() {
        use designcraft_doc::{ParaFormat, Story, StoryId};
        let mut s = Session::new();
        s.execute("file.new", &json!({})).unwrap();
        let f = s.execute("frame.create", &json!({"rect": [72, 72, 500, 400], "content": "text", "text": "T", "caret": false})).unwrap();
        let sid = StoryId(f["story"].as_u64().unwrap());
        s.execute("text.select", &json!({"story": sid.0, "anchor": 1, "focus": 1})).unwrap();
        let tid = s.execute("table.insert", &json!({"rows": 1, "cols": 2})).unwrap()["table"].as_u64().unwrap();
        s.edit(|d, _| {
            let t = std::sync::Arc::make_mut(d.story_mut(sid).unwrap().tables.get_mut(&tid).unwrap());
            for (i, (id, text)) in [("ua1i1", "Links"), ("ua1i2", "Rechts")].into_iter().enumerate() {
                t.cells[i].source_id = id.into();
                t.cells[i].text = Story::with_text(StoryId(0), text, ParaFormat::default());
            }
            Ok(())
        })
        .unwrap();
        s.execute("xml.tag", &json!({"tag": "body", "ids": [f["id"]]})).unwrap();
        let x = s.execute("file.exportXml", &json!({})).unwrap()["text"].as_str().unwrap().to_string();
        assert!(x.contains(" id=\"ua1i1\">Links</Cell>") && x.contains(" id=\"ua1i2\">Rechts</Cell>"), "{x}");
        let table = "<Table aid:table=\"table\"><Cell id=\"ua1i2\">Right</Cell><Cell id=\"ua9i9\">Gone</Cell><Cell id=\"ua1i1\">Left</Cell></Table>";
        let r = s.execute("file.importXml", &json!({"text": format!("<Root><body>T\n{table}</body></Root>")})).unwrap();
        assert_eq!(r["lostCells"], 1, "{r}");
        let t = s.doc().unwrap().doc.story(sid).unwrap().tables[&tid].clone();
        assert_eq!((t.cells[0].text.text.as_str(), t.cells[1].text.text.as_str()), ("Left", "Right"));
    }

    /// Tables in a tagged frame are part of its XML: export writes them as InDesign does, import
    /// puts the cells' texts back in place, and a table the XML leaves out stays as it is.
    #[test]
    fn tables_in_tagged_frames_export_and_import() {
        use designcraft_doc::{ParaFormat, Story, StoryId};
        let mut s = Session::new();
        s.execute("file.new", &json!({})).unwrap();
        let f = s.execute("frame.create", &json!({"rect": [72, 72, 500, 400], "content": "text", "text": "Intro", "caret": false})).unwrap();
        let sid = StoryId(f["story"].as_u64().unwrap());
        s.execute("text.select", &json!({"story": sid.0, "anchor": 5, "focus": 5})).unwrap();
        let tid = s.execute("table.insert", &json!({"rows": 1, "cols": 2})).unwrap()["table"].as_u64().unwrap();
        s.edit(|d, _| {
            let t = std::sync::Arc::make_mut(d.story_mut(sid).unwrap().tables.get_mut(&tid).unwrap());
            let mut price = Story::with_text(StoryId(0), "Preis 54 ", ParaFormat::default());
            price.format_chars(0..5, |f| f.over.font_style = Some("Bold".into()));
            t.cells[0].text = price;
            t.cells[1].text = Story::with_text(StoryId(0), "Motiv 498", ParaFormat::default());
            Ok(())
        })
        .unwrap();
        s.execute("xml.tag", &json!({"tag": "body", "ids": [f["id"]]})).unwrap();
        let x = s.execute("file.exportXml", &json!({})).unwrap()["text"].as_str().unwrap().to_string();
        assert!(x.contains("aid:table=\"table\" aid:trows=\"1\" aid:tcols=\"2\""), "{x}");
        assert!(x.contains("aid:crows=\"1\" aid:ccols=\"1\""), "{x}");
        assert!(x.contains(">Preis 54 </Cell>") && x.contains(">Motiv 498</Cell>"), "the cells' text, white space kept: {x}");
        // The same XML changes nothing; translated cells take their text, keeping their format.
        let cells = |s: &Session| {
            let st = s.doc().unwrap().doc.story(sid).unwrap().clone();
            let t = st.tables[&tid].clone();
            (st.text.clone(), t.cells.iter().map(|c| c.text.clone()).collect::<Vec<_>>())
        };
        let before = cells(&s);
        s.execute("file.importXml", &json!({"text": x})).unwrap();
        assert_eq!(cells(&s).1.iter().map(|c| c.chars.clone()).collect::<Vec<_>>(), before.1.iter().map(|c| c.chars.clone()).collect::<Vec<_>>());
        let translated = x.replace(">Preis 54 <", ">Price 54 <").replace("Motiv 498", "Design 498");
        s.execute("file.importXml", &json!({"text": translated})).unwrap();
        let (text, after) = cells(&s);
        assert_eq!(text, before.0, "the frame's own text and the table's place stay");
        assert_eq!((after[0].text.as_str(), after[1].text.as_str()), ("Price 54 ", "Design 498"));
        assert_eq!(after[0].format_after(0).over.font_style.as_deref(), Some("Bold"), "the label stays bold");
        // XML without the table: the frame's text is put in, the table stays.
        s.execute("file.importXml", &json!({"text": "<Root><body>Introduction</body></Root>"})).unwrap();
        let st = s.doc().unwrap().doc.story(sid).unwrap().clone();
        assert!(st.text.starts_with("Introduction") && st.tables.contains_key(&tid), "{:?}", st.text);
        assert_eq!(st.tables[&tid].cells[1].text.text, "Design 498");
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
    fn import_xml_keeps_text_of_inline_tags_in_mapped_paragraphs() {
        let mut s = Session::new();
        s.execute("file.new", &json!({})).unwrap();
        s.execute("style.paragraph.create", &json!({"name": "Head"})).unwrap();
        let a = s
            .execute("frame.create", &json!({"rect": [72, 72, 400, 150], "content": "text", "text": "See the glossary entry", "caret": false}))
            .unwrap();
        s.execute("text.select", &json!({"story": a["story"], "anchor": 0, "focus": 0})).unwrap();
        s.execute("style.paragraph.apply", &json!({"name": "Head"})).unwrap();
        s.execute("xml.tag", &json!({"tag": "story", "ids": [a["id"]]})).unwrap();
        s.execute("xml.mapStyle", &json!({"style": "Head", "tag": "title"})).unwrap();
        s.execute("text.select", &json!({"story": a["story"], "anchor": 8, "focus": 16})).unwrap();
        s.execute("xml.tagText", &json!({"tag": "term"})).unwrap();
        let x = s.execute("file.exportXml", &json!({})).unwrap()["text"].as_str().unwrap().to_string();
        assert!(x.contains("<title>See the <term>glossary</term> entry</title>"), "{x}");
        s.execute("file.importXml", &json!({"text": x})).unwrap();
        let d = &s.doc().unwrap().doc;
        assert_eq!(d.stories[&designcraft_doc::StoryId(a["story"].as_u64().unwrap())].text, "See the glossary entry");
    }

    #[test]
    fn parse_keeps_text_nested_below_a_child_element() {
        let els = super::parse("<root><story><note>See the <term>x</term> y</note></story></root>", &|_| false).unwrap();
        assert_eq!(els.len(), 1);
        let children: Vec<(&str, &str)> = els[0].children.iter().map(|(n, b)| (n.as_str(), b.text.as_str())).collect();
        assert_eq!(children, [("note", "See the x y")]);
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
