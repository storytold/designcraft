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
/// when that is a valid XML name no other frame has, otherwise `text_1`, `image_1`, … in reading order.
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
    let retagged: std::collections::HashSet<ItemId> = ids.iter().copied().collect();
    // Tags that stay, so neither `text_3` nor a frame's name is given twice.
    let mut used: std::collections::HashSet<String> = in_reading_order(d, |it| !it.xml_tag.is_empty())
        .into_iter()
        .filter(|i| !retagged.contains(i))
        .filter_map(|i| d.item(i))
        .map(|it| it.xml_tag.clone())
        .collect();
    let mut next = [("text", 0usize), ("image", 0usize)];
    let mut plan: Vec<(ItemId, String, &'static str)> = Vec::new();
    for id in ids {
        let Some(it) = d.item(id) else { continue };
        let Some(kind) = frame_kind(it) else { continue };
        let name = it.name.trim();
        let tag = if use_names && valid_name(name) && !used.contains(name) {
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

/// The cells of `st`'s tables in story order, row by row (a merged cell once), each followed by
/// those of tables in it. Cells of nested tables (`depth` > 0) are addressed by the story's cell
/// `top` holding them; `frame` finds where a story cell is laid out.
fn list_cells(st: &Story, top: Option<(u64, usize, usize)>, depth: usize, frame: &dyn Fn(u64, usize, usize) -> Option<u64>, out: &mut Vec<Value>) {
    for tid in st.paras.iter().filter_map(|p| p.table) {
        let Some(t) = st.tables.get(&tid) else { continue };
        let (nr, nc) = (t.nrows(), t.ncols());
        let owners = t.owners();
        for i in (0..nr * nc).filter(|&i| owners.get(i) == Some(&(i / nc, i % nc))) {
            let Some(cell) = t.cell(i / nc, i % nc) else { continue };
            let (tt, r, c) = top.unwrap_or((tid, i / nc, i % nc));
            let text: String = plain(&cell.text.text).replace('\n', " ").chars().take(60).collect();
            let id = (!cell.source_id.is_empty()).then(|| cell.source_id.clone());
            out.push(json!({"table": tt, "row": r, "col": c, "depth": depth, "id": id, "text": text, "frame": frame(tt, r, c)}));
            list_cells(&cell.text, Some((tt, r, c)), depth + 1, frame, out);
        }
    }
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

/// A story's text with its markers dropped, forced line breaks kept.
fn plain_keep_breaks(t: &str) -> String {
    t.chars().filter(|c| !('\u{E000}'..='\u{E1FF}').contains(c)).collect()
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

/// Character style name → its element name: [`style_element`], with `_2`, `_3`, … after a name an
/// earlier style (in the styles' order) has already, so each style keeps its own element and
/// import finds the style again.
type StyleNames = std::collections::HashMap<String, String>;

fn style_names(d: &Document) -> StyleNames {
    let mut used = std::collections::HashSet::new();
    let mut out = StyleNames::new();
    for c in &d.styles.character {
        let base = style_element(&c.name);
        let mut name = base.clone();
        let mut k = 1;
        while !used.insert(name.clone()) {
            k += 1;
            name = format!("{base}_{k}");
        }
        out.insert(c.name.clone(), name);
    }
    out
}

/// A run's character style, if it has one.
fn char_style(f: &designcraft_doc::CharFormat) -> Option<&str> {
    Some(f.style.as_str()).filter(|s| !s.is_empty() && *s != designcraft_doc::story::NO_CHAR_STYLE)
}

/// A paragraph's text with inline-tagged runs as elements, and character-styled runs (inside
/// those) as elements named after their style.
/// `runs` are the story's runs in order (collected once per story): only those reaching the
/// paragraph are visited, found by binary search.
fn para_xml(
    st: &designcraft_doc::Story,
    runs: &[(std::ops::Range<usize>, &designcraft_doc::CharFormat)],
    r: std::ops::Range<usize>,
    names: &StyleNames,
) -> String {
    let mut out = String::new();
    let (mut open, mut open_style): (Option<String>, Option<String>) = (None, None);
    let first = runs.partition_point(|(rr, _)| rr.end <= r.start);
    for (rr, f) in runs.iter().skip(first).take_while(|(rr, _)| rr.start < r.end) {
        let (a, b) = (rr.start.max(r.start), rr.end.min(r.end));
        if a >= b {
            continue;
        }
        let tag = f.over.xml_tag.clone().filter(|t| !t.is_empty());
        let style = char_style(f).map(|c| names.get(c).cloned().unwrap_or_else(|| style_element(c)));
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
        // A forced line break stays one (U+2028, as InDesign writes it): a newline would be a
        // paragraph to import.
        out.push_str(&esc(&plain_keep_breaks(&st.text[a..b])));
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

/// Tables in cells that export writes, at most: each adds a `<Table>` and a `<Cell>` level, and
/// import refuses XML nested deeper than [`MAX_XML_DEPTH`] (the root, the frame's element and a
/// mapped paragraph come first).
const MAX_TABLE_NESTING: usize = (MAX_XML_DEPTH - 4) / 2;

/// Paragraph `i` (range `r`) of `st`: its text, or its table as InDesign writes one. `depth`:
/// how many tables hold `st`; a table nested deeper than [`MAX_TABLE_NESTING`] is left out.
fn para_or_table_xml(
    st: &Story,
    runs: &[(std::ops::Range<usize>, &designcraft_doc::CharFormat)],
    i: usize,
    r: std::ops::Range<usize>,
    depth: usize,
    names: &StyleNames,
) -> String {
    match st.paras.get(i).and_then(|p| p.table).and_then(|id| st.tables.get(&id)) {
        Some(t) if depth < MAX_TABLE_NESTING => table_xml(t, depth + 1, names),
        Some(_) => String::new(),
        None => para_xml(st, runs, r, names),
    }
}

/// A table as InDesign's XML export writes one: `aid:` table and cells, each cell's paragraphs
/// on lines of their own (tables in cells too).
fn table_xml(t: &designcraft_doc::Table, depth: usize, names: &StyleNames) -> String {
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
            let runs: Vec<_> = cell.text.runs().collect();
            let text: Vec<String> =
                cell.text.para_ranges().into_iter().enumerate().map(|(i, r)| para_or_table_xml(&cell.text, &runs, i, r, depth, names)).collect();
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
    let names = style_names(d);
    for id in tagged(d) {
        let Some(it) = d.item(id) else { continue };
        let tag = &it.xml_tag;
        match &it.content {
            Content::Text(tf) => {
                let Some(st) = d.story(tf.story).filter(|_| written.insert(tf.story)) else { continue };
                let mapped = |style: &str| d.xml.style_map.iter().find(|(s, _)| s == style).map(|(_, t)| t.clone());
                // Paragraph text with inline tags already escaped.
                let paras: Vec<(Option<String>, String)> = {
                    let runs: Vec<_> = st.runs().collect();
                    st.para_ranges()
                        .into_iter()
                        .enumerate()
                        .map(|(i, r)| (mapped(&st.paras[i].style), para_or_table_xml(st, &runs, i, r, 0, &names)))
                        .collect()
                };
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
                out.push_str(&format!("  <{tag} href=\"file://{}\"/>\n", esc(&href_escape(&name))));
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
#[derive(Clone, Default)]
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
    /// `xml_layout`: the element's own text, whose hand-written indentation lines go (see
    /// [`unindent`]).
    fn paras(&self, style: Option<String>, trim: impl Fn(char) -> bool + Copy, xml_layout: bool) -> Vec<XPara> {
        let all = if xml_layout { unindent(&self.text) } else { self.text.trim_matches(trim) };
        // `all` is a slice of the text: where it starts.
        let lead = all.as_ptr() as usize - self.text.as_ptr() as usize;
        let mut at = lead;
        let mut lines = Vec::new();
        for line in all.split('\n') {
            let start = at + line.len() - line.trim_start_matches(trim).len();
            lines.push((start, line.trim_matches(trim)));
            at += line.len() + 1;
        }
        let runs = self.runs_of(lines.iter().map(|&(start, text)| start..start + text.len()));
        lines.into_iter().zip(runs).map(|((_, text), runs)| XPara { style: style.clone(), text: text.to_string(), runs }).collect()
    }

    /// The runs within each of `spans` (ascending, apart), in offsets from its start: one sweep
    /// over the runs, which keep their order (inner ones first) within a span.
    fn runs_of(&self, spans: impl Iterator<Item = std::ops::Range<usize>>) -> Vec<Vec<(std::ops::Range<usize>, String)>> {
        let mut by_start: Vec<usize> = (0..self.runs.len()).collect();
        by_start.sort_by_key(|&k| self.runs[k].0.start);
        let (mut next, mut active, mut out) = (0, Vec::new(), Vec::new());
        for span in spans {
            while next < by_start.len() && self.runs[by_start[next]].0.start < span.end {
                active.push(by_start[next]);
                next += 1;
            }
            active.retain(|&k| self.runs[k].0.end > span.start);
            let mut here: Vec<usize> = active.iter().copied().filter(|&k| self.runs[k].0.start < span.end).collect();
            here.sort_unstable();
            out.push(
                here.into_iter()
                    .map(|k| {
                        let (r, n) = &self.runs[k];
                        (r.start.max(span.start) - span.start..r.end.min(span.end) - span.start, n.clone())
                    })
                    .collect(),
            );
        }
        out
    }

    /// A mapped paragraph's content: its text (trimmed) as one paragraph, split where a table
    /// sits in it, each table a paragraph of its own (its anchor), in order.
    fn blocks(&self, style: Option<String>) -> Vec<XPara> {
        let anchor = designcraft_doc::story::TABLE_ANCHOR;
        if !self.text.contains(anchor) {
            return vec![self.para(style)];
        }
        let mut pieces = Vec::new();
        let mut at = 0;
        for (i, _) in self.text.match_indices(anchor) {
            pieces.push(at..i);
            pieces.push(i..i + anchor.len_utf8());
            at = i + anchor.len_utf8();
        }
        pieces.push(at..self.text.len());
        let spans: Vec<std::ops::Range<usize>> = pieces
            .into_iter()
            .filter_map(|r| {
                let t = &self.text[r.clone()];
                let start = r.start + t.len() - t.trim_start().len();
                let trimmed = t.trim();
                (!trimmed.is_empty()).then_some(start..start + trimmed.len())
            })
            .collect();
        let runs = self.runs_of(spans.iter().cloned());
        spans.into_iter().zip(runs).map(|(r, runs)| XPara { style: style.clone(), text: self.text[r].to_string(), runs }).collect()
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
#[derive(Clone, Default)]
struct XTable {
    cells: Vec<Body>,
}

/// The cells of an XML table, those of tables in them too.
fn cell_count(t: &XTable) -> usize {
    t.cells.iter().map(|c| 1 + c.tables.iter().map(cell_count).sum::<usize>()).sum()
}

/// A table as export writes one (`aid:table="table"`); any other element named `Table` (a mapped
/// paragraph tag, say) is text.
fn is_table(e: &craft_xml::El) -> bool {
    e.name == "Table" && e.attr("table") == Some("table")
}

struct Image {
    href: String,
    fit: Option<String>,
    crop: Option<String>,
}

/// The element's image: its `href` (also `xlink:href`), `fit` and `crop`.
fn image_attrs(e: &craft_xml::El) -> Option<Image> {
    let get = |name: &str| e.attr_local(name).map(str::to_string);
    get("href").map(|href| Image { href, fit: get("fit"), crop: get("crop") })
}

/// A file path as it goes into a `file://` href: `%` and spaces escaped, so [`href_path`]'s one
/// decoding gives the path back (`rate%20.png` stays itself).
fn href_escape(path: &str) -> String {
    path.replace('%', "%25").replace(' ', "%20")
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
        Some(b) if path.is_relative() && !decoded.starts_with(['/', '\\']) && !has_drive(&decoded) => b.join(path).to_string_lossy().to_string(),
        _ => decoded,
    }
}

/// `C:/x.png` or `C:\x.png`: absolute, also where `Path` knows no drive letters.
fn has_drive(p: &str) -> bool {
    let b = p.as_bytes();
    b.len() > 2 && b[0].is_ascii_alphabetic() && b[1] == b':' && matches!(b[2], b'/' | b'\\')
}

/// `%XX` escapes decoded; a `%` without two hex digits stays as it is.
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

/// How deep elements may nest: deeper XML is refused rather than built (and dropped) recursively.
const MAX_XML_DEPTH: usize = 64;

/// The elements under the root. Elements `inline` names are runs in the text holding them at
/// any depth (character styles), never child elements.
fn parse(xml: &str, inline: &dyn Fn(&str) -> bool) -> std::result::Result<Vec<Element>, String> {
    let mut options = craft_xml::Options::local();
    options.strict = true;
    // The root and its elements' children sit two levels down; deeper XML is refused.
    options.limits.max_depth = MAX_XML_DEPTH;
    let root = options.parse(xml.as_bytes()).map_err(|e| e.to_string())?;
    Ok(root
        .elements()
        .map(|element| {
            let mut body = Body::default();
            // Child elements in order; a table right inside the element is one too (no name),
            // so it keeps its place among mapped paragraphs.
            let mut children = Vec::new();
            for node in &element.children {
                match node {
                    craft_xml::Node::El(child) if !is_table(child) && !inline(&child.name) => {
                        let mut b = Body::default();
                        body_of(child, &mut b);
                        children.push((child.name.clone(), b));
                    }
                    craft_xml::Node::El(child) if is_table(child) => {
                        let mut b = Body::default();
                        node_into(node, &mut b);
                        node_into(node, &mut body);
                        children.push((String::new(), b));
                    }
                    node => node_into(node, &mut body),
                }
            }
            // Only tables: the body has them, in their place in its text.
            if children.iter().all(|(n, _)| n.is_empty()) {
                children.clear();
            }
            Element { name: element.name.clone(), body, children, image: image_attrs(element) }
        })
        .collect())
}

/// The text and tables of `e`'s content into `body`.
fn body_of(e: &craft_xml::El, body: &mut Body) {
    for node in &e.children {
        node_into(node, body);
    }
}

/// One node into `body`: text as it is, a table as an anchor plus its cells, any other element
/// (an inline one) by its content.
fn node_into(node: &craft_xml::Node, body: &mut Body) {
    match node {
        craft_xml::Node::Text(text) | craft_xml::Node::CData(text) => body.text.push_str(text),
        craft_xml::Node::El(e) if is_table(e) => {
            // The table's place in the text holding it; text between its cells belongs to no paragraph.
            body.text.push(designcraft_doc::story::TABLE_ANCHOR);
            let cells = e
                .elements()
                .filter(|c| c.name == "Cell")
                .map(|c| {
                    let mut b = Body { id: c.attr("id").map(str::to_string), ..Body::default() };
                    body_of(c, &mut b);
                    b
                })
                .collect();
            body.tables.push(XTable { cells });
        }
        // An inline element: a run over the text it holds (inner runs first).
        craft_xml::Node::El(e) => {
            let start = body.text.len();
            body_of(e, body);
            if start < body.text.len() {
                let end = body.text.len();
                body.runs.push((start..end, e.name.clone()));
            }
        }
        craft_xml::Node::Pi { .. } | craft_xml::Node::Comment(_) => {}
    }
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
            "{kinds?: [text, image] (default both), overwrite?: bool (retag tagged frames; default false), useNames?: bool (a valid XML Layers panel name is the tag unless another frame has it; default true)} — untagged frames on document pages get their name or text_1, image_1, … in reading order → {tagged: [{id, tag, kind}]}",
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
        cmd!(query "xml.structure", "Structure", [], None, "{} → {root, elements: [{tag, id, kind, text?, tables?}]} in reading order (`tables`: how many the frame's story has)", has_doc, |s, _| {
            let d = &s.doc()?.doc;
            let elements: Vec<Value> = tagged(d)
                .into_iter()
                .filter_map(|id| d.item(id))
                .map(|it| {
                    let (kind, text, tables) = match &it.content {
                        Content::Text(tf) => ("text", d.story(tf.story).map(|st| plain(&st.text)), d.story(tf.story).map(|st| st.tables.len())),
                        Content::Graphic(_) => ("graphic", None, None),
                        _ => ("object", None, None),
                    };
                    json!({"tag": it.xml_tag, "id": it.id.0, "kind": kind, "text": text, "tables": tables})
                })
                .collect();
            Ok(json!({"root": root_name(d), "elements": elements}))
        }),
        cmd!(query "xml.cells", "Table Cells", [], None, "{id} → {story, cells: [{table, row, col, depth, id?, text, frame?}]}: the cells of the tables in a text frame's story, row by row (a merged cell once), each followed by the cells of tables in it (`depth` 1, …: `table`, `row` and `col` are then the story's cell holding them), with the frame each is laid out in", has_doc, |s, p| {
            const ID: &str = "xml.cells";
            let d = &s.doc()?.doc;
            let frame = p.get("id").and_then(Value::as_u64).map(ItemId).ok_or_else(|| bad(ID, "give the text frame's id"))?;
            let sid = d.item(frame).and_then(|it| it.text_frame()).map(|tf| tf.story).ok_or_else(|| bad(ID, "not a text frame"))?;
            let st = d.story(sid).ok_or_else(|| bad(ID, "no story"))?;
            // Where each cell is laid out: the first frame showing it.
            let composed = s.cache.get(d, sid, None);
            let mut shown: std::collections::HashMap<(u64, usize, usize), ItemId> = std::collections::HashMap::new();
            for ft in &composed.frames {
                for frag in &ft.tables {
                    for c in &frag.cells {
                        shown.entry((frag.table, c.row, c.col)).or_insert(ft.frame);
                    }
                }
            }
            let mut cells = Vec::new();
            list_cells(st, None, 0, &|t, r, c| shown.get(&(t, r, c)).map(|f| f.0), &mut cells);
            Ok(json!({"story": sid.0, "cells": cells}))
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
                    // The tags there are, then those added: one set, not a search per element.
                    let mut known: std::collections::HashSet<String> = d.xml.tags.iter().map(|t| t.name.clone()).collect();
                    for n in &names {
                        if valid_name(n) && known.insert(n.clone()) {
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
            "{path | text, imageDir?} — each element's content goes into the next text frame with that tag (child elements of mapped tags become paragraphs in their style); an element with `href` (a file:// URL or path; relative ones against imageDir, else the XML file's folder) places that image into the next graphic frame with its tag (`fit`: an object.fit mode, default fillProportionally; `crop`: a PDF box such as contentVisible) → {placed, unmatched, unmatchedTags: [tags without a free frame], lostCells: the XML's table cells with no cell to go into (an id the table hasn't, a table the story has no place for), failed: [{tag, href, error}]}",
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
    let char_styles: std::collections::HashMap<String, String> = style_names(&s.doc()?.doc).into_iter().map(|(style, el)| (el, style)).collect();
    // An element of a mapped paragraph tag is a paragraph, even where a character style has its name.
    let mapped: std::collections::HashSet<String> = s.doc()?.doc.xml.style_map.iter().map(|(_, t)| t.clone()).collect();
    let elements = parse(&text, &|n| char_styles.contains_key(n) && !mapped.contains(n)).map_err(|e| bad(ID, format!("not well-formed XML: {e}")))?;
    // XML with character-styled runs says where each style goes; XML without (written before
    // export wrote them) leaves the styles as they are.
    // An element with character-styled runs says where each style goes; one without (XML written
    // before export wrote them) leaves its story's styles as they are.
    let styled = |n: &str| char_styles.contains_key(n);
    // Relative image paths: against `imageDir`, else the XML file's folder.
    let base = str_param(p, "imageDir").map(std::path::PathBuf::from).or_else(|| {
        str_param(p, "text").is_none().then(|| str_param(p, "path")).flatten().and_then(|x| std::path::Path::new(x).parent().map(Into::into))
    });
    let d = &s.doc()?.doc;
    // Frames by tag, in reading order; each element takes the next free one.
    // A story threaded through several tagged frames takes one element, at its first frame (as
    // export writes it).
    let mut stories = std::collections::HashSet::new();
    let mut slots: Vec<(String, StoryId, bool)> = tagged(d)
        .into_iter()
        .filter_map(|id| d.item(id))
        .filter_map(|it| it.text_frame().map(|tf| (it.xml_tag.clone(), tf.story, false)))
        .filter(|(_, sid, _)| stories.insert(*sid))
        .collect();
    let mut image_slots: Vec<(String, ItemId, bool)> = tagged(d)
        .into_iter()
        .filter_map(|id| d.item(id))
        .filter(|it| frame_kind(it) == Some("image"))
        .map(|it| (it.xml_tag.clone(), it.id, false))
        .collect();
    let reverse: Vec<(String, String)> = d.xml.style_map.iter().map(|(st, t)| (t.clone(), st.clone())).collect();
    let mut fills: Vec<(StoryId, Vec<XPara>, Vec<XTable>, bool)> = Vec::new();
    // Images, and empty elements of a graphic frame's tag, by tag in XML order: each takes the
    // next free graphic frame of its tag (an image only once it is placed in it).
    let mut images: Vec<(String, Option<Image>)> = Vec::new();
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
            images.push((el.name, Some(image)));
            continue;
        }
        // An empty element of a graphic frame's tag (an empty frame, as export writes one): nothing
        // to put in it, but it takes its frame in turn.
        if el.body.text.trim().is_empty()
            && el.children.is_empty()
            && images.iter().filter(|(t, _)| *t == el.name).count() < image_slots.iter().filter(|(t, ..)| *t == el.name).count()
        {
            images.push((el.name, None));
            continue;
        }
        let Some(slot) = slots.iter_mut().find(|(t, _, used)| *t == el.name && !used) else {
            miss(&el.name);
            continue;
        };
        slot.2 = true;
        let with_styles = el.body.has_style(&styled) || el.children.iter().any(|(_, b)| b.has_style(&styled));
        // Paragraphs, and the tables their anchors stand for, in order. Lines lose the XML's
        // indentation (spaces, tabs), not the text's own white space (no-break spaces).
        let indent = |c: char| matches!(c, ' ' | '\t' | '\r');
        let (paras, tables): (Vec<XPara>, Vec<XTable>) = if el.children.is_empty() {
            (el.body.paras(None, indent, true), el.body.tables)
        } else {
            let mut tables = Vec::new();
            let paras = el
                .children
                .into_iter()
                .flat_map(|(t, b)| {
                    let p = b.blocks(reverse.iter().find(|(tag, _)| *tag == t).map(|(_, st)| st.clone()));
                    tables.extend(b.tables);
                    p
                })
                .collect();
            (paras, tables)
        };
        fills.push((slot.1, paras, tables, with_styles));
    }
    let mut placed = fills.len();
    let mut fx = Fill { cs: Some(&char_styles), lost_cells: 0 };
    if !fills.is_empty() {
        fill_stories(s, fills, &mut fx)?;
    }
    // Each image fills its frame proportionally unless `fit` says otherwise; `crop` picks a PDF's
    // box. One that can't be placed is reported and the rest of the import goes on.
    // A frame whose image failed stays free for the next element of its tag.
    let mut failed: Vec<Value> = Vec::new();
    for (tag, image) in images {
        // The next free frame of the tag that can take an image: a locked one (or one on a locked
        // layer) only when no other is left, so it doesn't hold back the frames after it.
        let at = {
            let d = &s.doc()?.doc;
            let locked = |id: ItemId| d.item(id).is_some_and(|it| it.locked || d.layer(it.layer).is_some_and(|l| l.locked));
            let free = |(t, id, used): &&(String, ItemId, bool)| *t == tag && !used && !locked(*id);
            image_slots.iter().position(|x| free(&x)).or_else(|| image_slots.iter().position(|(t, _, used)| *t == tag && !used))
        };
        let Some(slot) = at.map(|i| &mut image_slots[i]) else {
            miss(&tag);
            continue;
        };
        let Some(image) = image else {
            slot.2 = true;
            continue;
        };
        let frame = slot.1;
        let path = href_path(&image.href, base.as_deref());
        // The file the frame holds already (as export wrote it): unchanged, nothing to place.
        let holds = {
            let d = &s.doc()?.doc;
            d.item(frame)
                .and_then(|it| match &it.content {
                    Content::Graphic(g) => d.assets.get(&g.asset),
                    _ => None,
                })
                .map(|a| (a.link.clone(), a.name.clone()))
        };
        // A linked image by its file; an embedded one (no link) by its name only while no such file
        // is there to load instead.
        let holds = holds.and_then(|(link, name)| link.or_else(|| (!std::path::Path::new(&path).exists()).then_some(name)));
        if image.fit.is_none() && image.crop.is_none() && holds.is_some_and(|l| image.href == format!("file://{l}") || path == l) {
            slot.2 = true;
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
        // Fit leaves a locked frame (or one on a locked layer) as it is: a fit asked for there
        // would not happen, so the image isn't placed either.
        let locked = {
            let d = &s.doc()?.doc;
            d.item(frame).is_some_and(|it| it.locked || d.layer(it.layer).is_some_and(|l| l.locked))
        };
        if image.fit.is_some() && locked {
            failed.push(json!({"tag": tag, "href": image.href, "error": bad(ID, "the frame is locked, so its fit can't be applied").to_string()}));
            continue;
        }
        let done = s.execute("file.place", &place).and_then(|_| match &image.fit {
            Some(mode) => s.execute("object.fit", &json!({"mode": mode, "ids": [frame.0]})),
            None => Ok(Value::Null),
        });
        match done {
            Ok(_) => {
                slot.2 = true;
                placed += 1;
            }
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

/// An element's text without the XML's own layout around it: hand-written XML puts the text on
/// indented lines of its own (`<c>⏎····Heading⏎</c>`), and the blank first and last lines go.
/// Text as export writes it (never indented) keeps its empty paragraphs.
fn unindent(text: &str) -> &str {
    let indent = |c: char| matches!(c, ' ' | '\t' | '\r');
    let t = text.trim_matches(indent);
    let lines: Vec<&str> = text.split('\n').collect();
    let wrapped = lines.len() >= 3
        && lines.first().is_some_and(|l| l.chars().all(indent))
        && lines.last().is_some_and(|l| l.chars().all(indent))
        && lines[1..lines.len() - 1].iter().any(|l| l.starts_with([' ', '\t']));
    if !wrapped {
        return t;
    }
    let t = t.strip_prefix('\n').unwrap_or(t);
    t.strip_suffix('\n').unwrap_or(t)
}

/// Replaces each story's text with the paragraphs, in their mapped styles.
/// Replaces each story's text with the paragraphs, in their mapped styles; character styles only
/// in a story whose element has styled runs.
fn fill_stories(s: &mut Session, fills: Vec<(StoryId, Vec<XPara>, Vec<XTable>, bool)>, fx: &mut Fill) -> Result<()> {
    let cs = fx.cs;
    s.edit(|d, _| {
        for (sid, paras, tables, with_styles) in fills {
            if let Some(st) = d.story_mut(sid) {
                fx.cs = cs.filter(|_| with_styles);
                fill_story(st, &paras, &tables, fx);
            }
        }
        fx.cs = cs;
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

/// Puts the paragraphs into the story in place, so the template's formatting stays: each XML
/// paragraph replaces the story's next text paragraph (only the part that differs, which takes
/// the format of the first character it replaces), extra paragraphs copy the last one's formats,
/// surplus old paragraphs are removed. A trailing empty paragraph (the story ending in a return)
/// stays. Tables are no text: an XML table fills the story's table there, cell by cell, when the
/// story has one at that place and is left out otherwise; a story table the XML leaves out stays
/// as it is. Neither takes a text paragraph's place. With `fx.cs`, each paragraph's characters
/// take the character styles of the XML's runs, and no style outside them.
fn fill_story(st: &mut Story, paras: &[XPara], tables: &[XTable], fx: &mut Fill) {
    let anchor = designcraft_doc::story::TABLE_ANCHOR;
    // The story's paragraph ranges, read once and moved along as edits change lengths.
    let mut ranges = st.para_ranges();
    let keep_last = ranges.len() > 1 && ranges.last().is_some_and(|r| r.is_empty());
    let old_n = ranges.len() - usize::from(keep_last);
    // A last empty paragraph in the XML is the story's own trailing return.
    let paras = match paras.split_last() {
        Some((last, rest)) if keep_last && last.text.is_empty() && paras.len() == ranges.len() => rest,
        _ => paras,
    };
    let is_story_table = |st: &Story, d: usize| d < old_n && st.paras.get(d).is_some_and(|p| p.table.is_some());
    let mut next_table = tables.iter();
    // The story paragraph the next XML paragraph goes to, and where each XML paragraph went.
    let mut d = 0;
    let mut placed: Vec<(usize, usize)> = Vec::new();
    let mut extra = String::new();
    for (xi, XPara { text: new, .. }) in paras.iter().enumerate() {
        if new.trim() == anchor.to_string() {
            let x = next_table.next();
            if is_story_table(st, d) {
                if let (Some(x), Some(tid)) = (x, st.paras[d].table)
                    && let Some(t) = st.tables.get_mut(&tid)
                {
                    fill_table(std::sync::Arc::make_mut(t), x, fx);
                }
                d += 1;
            } else {
                // The story has no table here: the XML's stays out, its cells lost.
                fx.lost_cells += x.map_or(0, cell_count);
            }
            continue;
        }
        // Story tables the XML leaves out stay where they are.
        while is_story_table(st, d) {
            d += 1;
        }
        // Table anchors are no text to put in.
        let stripped;
        let new = if new.contains(anchor) {
            stripped = new.replace(anchor, "");
            &stripped
        } else {
            new
        };
        if d < old_n {
            let r = ranges[d].clone();
            let (pre, suf) = common_ends(&st.text[r.clone()], new);
            let (a, b) = (r.start + pre, r.end - suf);
            let repl = &new[pre..new.len() - suf];
            if a < b {
                st.replace(a..b, repl);
            } else if !repl.is_empty() {
                let fmt = if pre > 0 { st.char_format_at(a).clone() } else { st.format_after(a).clone() };
                st.insert_with(a, repl, fmt);
            }
            // Later paragraphs move by what this one grew or shrank.
            let delta = repl.len() as isize - (b - a) as isize;
            if delta != 0 {
                ranges[d].end = ranges[d].end.saturating_add_signed(delta);
                for r in &mut ranges[d + 1..] {
                    *r = r.start.saturating_add_signed(delta)..r.end.saturating_add_signed(delta);
                }
            }
            placed.push((d, xi));
            d += 1;
        } else {
            extra.push('\n');
            extra.push_str(new);
            placed.push((old_n + placed.iter().filter(|(i, _)| *i >= old_n).count(), xi));
        }
    }
    // New paragraphs after the story's last one, at once.
    if !extra.is_empty() {
        if let Some(last) = old_n.checked_sub(1).and_then(|i| ranges.get(i)) {
            st.insert(last.end, &extra);
        } else {
            st.insert(0, &extra[1..]);
        }
    }
    // Surplus paragraphs go, last first, except tables (whose paragraphs aren't text) and the
    // first one (a story keeps a paragraph).
    for j in (d.max(1)..old_n).rev() {
        if st.paras.get(j).is_some_and(|p| p.table.is_some()) {
            continue;
        }
        let rs = st.para_ranges();
        if let (Some(prev), Some(me)) = (rs.get(j - 1), rs.get(j)) {
            st.delete(prev.end..me.end);
        }
    }
    for &(i, xi) in &placed {
        if let (Some(sty), Some(pf)) = (&paras[xi].style, st.paras.get_mut(i))
            && pf.table.is_none()
        {
            pf.style = sty.clone();
        }
    }
    if let Some(cs) = fx.cs {
        let ranges = st.para_ranges();
        for &(i, xi) in &placed {
            let p = &paras[xi];
            let Some(r) = ranges.get(i).filter(|r| st.paras[i].table.is_none() && st.text[(*r).clone()] == p.text) else { continue };
            let none = designcraft_doc::story::NO_CHAR_STYLE;
            st.format_chars(r.clone(), |f| f.style = none.into());
            // Outer runs first, so inner ones win.
            // Runs come inner first; for the same span the outer one goes first too.
            let mut runs: Vec<_> = p.runs.iter().enumerate().filter_map(|(k, (x, n))| cs.get(n).map(|sty| (k, x.clone(), sty))).collect();
            runs.sort_by_key(|(k, x, _)| (x.start, std::cmp::Reverse(x.end), std::cmp::Reverse(*k)));
            for (_, x, sty) in runs {
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
        fill_story(&mut cell.text, &xc.paras(None, |_| false, false), &xc.tables, fx);
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

    /// A Layers name another frame already has, as a tag or as a name, isn't given again: that
    /// frame gets a numbered tag. `kinds` picks which frames are tagged.
    #[test]
    fn auto_tag_gives_each_name_once() {
        let mut s = Session::new();
        s.execute("file.new", &json!({})).unwrap();
        let frame = |s: &mut Session, y: i32, content: &str, name: &str| {
            let id =
                s.execute("frame.create", &json!({"rect": [72, y, 400, y + 40], "content": content, "text": "x", "caret": false})).unwrap()["id"]
                    .clone();
            s.execute("object.rename", &json!({"id": id, "name": name})).unwrap();
            id
        };
        let kept = frame(&mut s, 72, "text", "kept");
        s.execute("xml.tag", &json!({"tag": "headline", "ids": [kept]})).unwrap();
        let a = frame(&mut s, 150, "text", "intro");
        let b = frame(&mut s, 250, "text", "intro");
        let c = frame(&mut s, 350, "text", "headline");
        let photo = frame(&mut s, 450, "graphic", "photo");
        let r = s.execute("xml.autoTag", &json!({"kinds": ["text"]})).unwrap();
        let tags: Vec<(serde_json::Value, String)> =
            r["tagged"].as_array().unwrap().iter().map(|t| (t["id"].clone(), t["tag"].as_str().unwrap().to_string())).collect();
        assert_eq!(tags, vec![(a, "intro".into()), (b, "text_1".into()), (c, "text_2".into())]);
        assert!(s.doc().unwrap().doc.item(designcraft_doc::ItemId(photo.as_u64().unwrap())).unwrap().xml_tag.is_empty(), "only text frames");
    }

    #[test]
    fn href_paths() {
        use super::{href_path, percent_decode};
        use std::path::Path;
        let base = Path::new("/xml/folder");
        let joined = |rel: &str| base.join(rel).to_string_lossy().to_string();
        for (href, want) in [
            ("file:///C:/img/a.png", "C:/img/a.png".to_string()),
            ("file:///home/me/a%20b.png", "/home/me/a b.png".into()),
            ("C:\\img\\a.png", "C:\\img\\a.png".into()),
            ("\\\\server\\share\\a.png", "\\\\server\\share\\a.png".into()),
            ("/abs/a.png", "/abs/a.png".into()),
            ("img/a%20b.png", joined("img/a b.png")),
            ("img\\a.png", joined("img\\a.png")),
            ("  a.png  ", joined("a.png")),
        ] {
            assert_eq!(href_path(href, Some(base)), want, "{href}");
        }
        assert_eq!(href_path("img/a.png", None), "img/a.png", "no base: as it is");
        assert_eq!(percent_decode("100%"), "100%");
        assert_eq!(percent_decode("a%zzb%2"), "a%zzb%2", "invalid escapes stay");
        assert_eq!(percent_decode("%C3%A4%41"), "äA");
    }

    #[test]
    fn image_attrs_read_href_and_xlink_href() {
        let attrs = |xml: &str| {
            let mut options = craft_xml::Options::local();
            options.strict = true;
            let root = options.parse(xml.as_bytes()).unwrap();
            super::image_attrs(&root).map(|i| (i.href, i.fit, i.crop))
        };
        assert_eq!(attrs(r#"<photo href="a&amp;b.png" fit="fitContent"/>"#), Some(("a&b.png".into(), Some("fitContent".into()), None)));
        assert_eq!(
            attrs(r#"<photo xmlns:xlink="http://www.w3.org/1999/xlink" xlink:href="file:///x.pdf" crop="contentVisible"/>"#),
            Some(("file:///x.pdf".into(), None, Some("contentVisible".into())))
        );
        assert_eq!(attrs("<photo/>"), None);
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

    /// Styles whose names give the same element name (`A B`, `A_B`) each get their own, and come
    /// back as themselves.
    #[test]
    fn character_styles_keep_their_own_element_names() {
        use designcraft_doc::StoryId;
        let mut s = Session::new();
        s.execute("file.new", &json!({})).unwrap();
        s.execute("style.character.create", &json!({"name": "A B"})).unwrap();
        s.execute("style.character.create", &json!({"name": "A_B"})).unwrap();
        let f = s.execute("frame.create", &json!({"rect": [72, 72, 500, 200], "content": "text", "text": "one two", "caret": false})).unwrap();
        let sid = StoryId(f["story"].as_u64().unwrap());
        s.edit(|d, _| {
            let st = d.story_mut(sid).unwrap();
            st.format_chars(0..3, |c| c.style = "A B".into());
            st.format_chars(4..7, |c| c.style = "A_B".into());
            Ok(())
        })
        .unwrap();
        s.execute("xml.tag", &json!({"tag": "body", "ids": [f["id"]]})).unwrap();
        let x = s.execute("file.exportXml", &json!({})).unwrap()["text"].as_str().unwrap().to_string();
        assert!(x.contains("<A_B>one</A_B> <A_B_2>two</A_B_2>"), "{x}");
        s.execute("file.importXml", &json!({"text": x.replace("one", "uno").replace("two", "dos")})).unwrap();
        let st = s.doc().unwrap().doc.story(sid).unwrap().clone();
        assert_eq!((st.format_after(0).style.as_str(), st.format_after(4).style.as_str()), ("A B", "A_B"));
    }

    /// A paragraph tag a character style also has as its name stays a paragraph.
    #[test]
    fn a_mapped_paragraph_tag_wins_over_a_character_style_name() {
        use designcraft_doc::StoryId;
        let mut s = Session::new();
        s.execute("file.new", &json!({})).unwrap();
        s.execute("style.character.create", &json!({"name": "bold"})).unwrap();
        s.execute("style.paragraph.create", &json!({"name": "Head"})).unwrap();
        let f = s.execute("frame.create", &json!({"rect": [72, 72, 500, 200], "content": "text", "text": "x", "caret": false})).unwrap();
        let sid = StoryId(f["story"].as_u64().unwrap());
        s.execute("xml.tag", &json!({"tag": "body", "ids": [f["id"]]})).unwrap();
        s.execute("xml.mapStyle", &json!({"style": "Head", "tag": "bold"})).unwrap();
        s.execute("file.importXml", &json!({"text": "<Root><body><bold>First</bold><bold>Second</bold></body></Root>"})).unwrap();
        let st = s.doc().unwrap().doc.story(sid).unwrap().clone();
        assert_eq!(st.text, "First\nSecond");
        assert!(st.paras.iter().all(|p| p.style == "Head"));
    }

    /// Two stories with one tag, the first threaded through two frames: each story takes its own
    /// element.
    #[test]
    fn a_threaded_story_takes_one_element_of_its_tag() {
        use designcraft_doc::StoryId;
        let mut s = Session::new();
        s.execute("file.new", &json!({})).unwrap();
        let a1 = s.execute("frame.create", &json!({"rect": [72, 72, 500, 200], "content": "text", "text": "A", "caret": false})).unwrap();
        let a2 = s.execute("frame.create", &json!({"rect": [72, 300, 500, 400], "content": "unassigned"})).unwrap();
        s.execute("text.thread", &json!({"from": a1["id"], "to": a2["id"]})).unwrap();
        let b1 = s.execute("frame.create", &json!({"rect": [72, 500, 500, 600], "content": "text", "text": "B", "caret": false})).unwrap();
        s.execute("xml.tag", &json!({"tag": "body", "ids": [a1["id"], a2["id"], b1["id"]]})).unwrap();
        s.execute("file.importXml", &json!({"text": "<Root><body>A2</body><body>B2</body></Root>"})).unwrap();
        let text = |f: &serde_json::Value| s.doc().unwrap().doc.story(StoryId(f["story"].as_u64().unwrap())).unwrap().text.clone();
        assert_eq!((text(&a1), text(&b1)), ("A2".to_string(), "B2".to_string()));
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

    /// A story of mapped paragraphs with a table right inside the element: export writes the table
    /// between them, and import puts its cells back. A table inside a mapped paragraph splits it,
    /// and an XML table the story has no place for counts its cells as lost.
    #[test]
    fn tables_among_mapped_paragraphs() {
        use designcraft_doc::{ParaFormat, Story, StoryId};
        let mut s = Session::new();
        s.execute("file.new", &json!({})).unwrap();
        s.execute("style.paragraph.create", &json!({"name": "Head"})).unwrap();
        let f = s.execute("frame.create", &json!({"rect": [72, 72, 500, 400], "content": "text", "text": "Title", "caret": false})).unwrap();
        let sid = StoryId(f["story"].as_u64().unwrap());
        s.execute("text.select", &json!({"story": sid.0, "anchor": 0, "focus": 0})).unwrap();
        s.execute("style.paragraph.apply", &json!({"name": "Head"})).unwrap();
        s.execute("text.select", &json!({"story": sid.0, "anchor": 5, "focus": 5})).unwrap();
        let tid = s.execute("table.insert", &json!({"rows": 1, "cols": 1})).unwrap()["table"].as_u64().unwrap();
        s.edit(|d, _| {
            let t = std::sync::Arc::make_mut(d.story_mut(sid).unwrap().tables.get_mut(&tid).unwrap());
            t.cells[0].text = Story::with_text(StoryId(0), "Zelle", ParaFormat::default());
            Ok(())
        })
        .unwrap();
        s.execute("xml.tag", &json!({"tag": "body", "ids": [f["id"]]})).unwrap();
        s.execute("xml.mapStyle", &json!({"style": "Head", "tag": "title"})).unwrap();
        let cell = |s: &Session| s.doc().unwrap().doc.story(sid).unwrap().tables[&tid].cells[0].text.text.clone();
        let table = "<Table aid:table=\"table\"><Cell>Cell</Cell></Table>";
        let r = s.execute("file.importXml", &json!({"text": format!("<Root><body><title>Heading</title>{table}</body></Root>")})).unwrap();
        assert_eq!(cell(&s), "Cell", "a direct table among mapped paragraphs: {r}");
        assert!(s.doc().unwrap().doc.story(sid).unwrap().text.starts_with("Heading"));
        let r = s
            .execute(
                "file.importXml",
                &json!({"text": format!("<Root><body><title>Head{}</title></body></Root>", table.replace("Cell<", "Zelle 2<"))}),
            )
            .unwrap();
        assert_eq!(cell(&s), "Zelle 2", "a table inside a mapped paragraph: {r}");
        assert_eq!(r["lostCells"], 0, "{r}");
        let two = format!("<Root><body><title>Head</title>{table}{table}</body></Root>");
        let r = s.execute("file.importXml", &json!({"text": two})).unwrap();
        assert_eq!(r["lostCells"], 1, "the second table has no place: {r}");
    }

    /// An element without a frame doesn't decide another story's character styles.
    #[test]
    fn unmatched_elements_leave_other_stories_styles_alone() {
        use designcraft_doc::StoryId;
        let mut s = Session::new();
        s.execute("file.new", &json!({})).unwrap();
        s.execute("style.character.create", &json!({"name": "bold"})).unwrap();
        let f = s.execute("frame.create", &json!({"rect": [72, 72, 500, 200], "content": "text", "text": "Keep bold", "caret": false})).unwrap();
        let sid = StoryId(f["story"].as_u64().unwrap());
        s.edit(|d, _| {
            d.story_mut(sid).unwrap().format_chars(5..9, |c| c.style = "bold".into());
            Ok(())
        })
        .unwrap();
        s.execute("xml.tag", &json!({"tag": "body", "ids": [f["id"]]})).unwrap();
        let r = s.execute("file.importXml", &json!({"text": "<Root><body>Keep bold</body><extra><bold>Unused</bold></extra></Root>"})).unwrap();
        assert_eq!(r["unmatched"], 1, "{r}");
        assert_eq!(s.doc().unwrap().doc.story(sid).unwrap().format_after(5).style, "bold");
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
        // The Structure lists the frame's tables; its cells, with their IDs and frame, go to.
        assert_eq!(s.execute("xml.structure", &json!({})).unwrap()["elements"][0]["tables"], 1);
        let cells = s.execute("xml.cells", &json!({"id": f["id"]})).unwrap();
        assert_eq!(
            cells["cells"][1],
            json!({"table": tid, "row": 0, "col": 1, "depth": 0, "id": "ua1i2", "text": "Right", "frame": f["id"]}),
            "{cells}"
        );
        s.execute("table.select", &json!({"story": sid.0, "table": tid, "row": 0, "col": 1})).unwrap();
        let sel = s.doc().unwrap().selection.text.unwrap();
        assert_eq!(sel.cell.map(|c| (c.row, c.col)), Some((0, 1)));
    }

    /// Text after a table the XML leaves out goes to the paragraph after the table, and a forced
    /// line break in a cell comes back as one, not as a second paragraph.
    #[test]
    fn tables_left_out_keep_the_text_after_them_in_place() {
        use designcraft_doc::{ParaFormat, Story, StoryId};
        let mut s = Session::new();
        s.execute("file.new", &json!({})).unwrap();
        let f = s.execute("frame.create", &json!({"rect": [72, 72, 500, 400], "content": "text", "text": "A\nB", "caret": false})).unwrap();
        let sid = StoryId(f["story"].as_u64().unwrap());
        s.execute("text.select", &json!({"story": sid.0, "anchor": 1, "focus": 1})).unwrap();
        let tid = s.execute("table.insert", &json!({"rows": 1, "cols": 1})).unwrap()["table"].as_u64().unwrap();
        s.edit(|d, _| {
            let t = std::sync::Arc::make_mut(d.story_mut(sid).unwrap().tables.get_mut(&tid).unwrap());
            t.cells[0].text = Story::with_text(StoryId(0), "x\u{2028}y", ParaFormat::default());
            Ok(())
        })
        .unwrap();
        s.execute("xml.tag", &json!({"tag": "body", "ids": [f["id"]]})).unwrap();
        let x = s.execute("file.exportXml", &json!({})).unwrap()["text"].as_str().unwrap().to_string();
        s.execute("file.importXml", &json!({"text": x})).unwrap();
        let cell = |s: &Session| s.doc().unwrap().doc.story(sid).unwrap().tables[&tid].cells[0].text.text.clone();
        assert_eq!(cell(&s), "x\u{2028}y", "one paragraph with its forced break");
        let before = s.doc().unwrap().doc.story(sid).unwrap().text.clone();
        s.execute("file.importXml", &json!({"text": "<Root><body>A\nB translated</body></Root>"})).unwrap();
        let st = s.doc().unwrap().doc.story(sid).unwrap().clone();
        assert!(st.text.ends_with("B translated"), "{:?} (was {before:?})", st.text);
        assert!(st.tables.contains_key(&tid), "the table stays");
    }

    /// An element named `Table` without export's table marker is a mapped paragraph like any other.
    #[test]
    fn a_mapped_table_tag_is_text() {
        let mut s = Session::new();
        s.execute("file.new", &json!({})).unwrap();
        s.execute("style.paragraph.create", &json!({"name": "Title"})).unwrap();
        let a = s.execute("frame.create", &json!({"rect": [72, 72, 400, 150], "content": "text", "text": "Old title", "caret": false})).unwrap();
        s.execute("text.select", &json!({"story": a["story"], "anchor": 0, "focus": 0})).unwrap();
        s.execute("style.paragraph.apply", &json!({"name": "Title"})).unwrap();
        s.execute("xml.tag", &json!({"tag": "story", "ids": [a["id"]]})).unwrap();
        s.execute("xml.mapStyle", &json!({"style": "Title", "tag": "Table"})).unwrap();
        s.execute("file.importXml", &json!({"text": "<Root><story><Table>Revised title</Table></story></Root>"})).unwrap();
        let st = s.doc().unwrap().doc.stories[&designcraft_doc::StoryId(a["story"].as_u64().unwrap())].clone();
        assert_eq!(st.text, "Revised title");
    }

    #[test]
    fn xml_indentation_around_text_goes_exported_blank_paragraphs_stay() {
        assert_eq!(super::unindent("\n    Heading\n"), "    Heading", "hand-indented: one paragraph");
        assert_eq!(super::unindent("\n    Heading\n  "), "    Heading");
        assert_eq!(super::unindent("\nHeading\n"), "\nHeading\n", "as export writes it: the empty paragraphs stay");
        assert_eq!(super::unindent("Heading"), "Heading");
    }

    /// An empty element and an image of the same tag take their frames in XML order.
    #[test]
    fn empty_and_image_elements_take_frames_in_order() {
        use designcraft_doc::{Content, ItemId};
        let dir = std::env::temp_dir().join(format!("dc-xml-order-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let png = designcraft_render::Rendered { width: 20, height: 20, pixels: vec![200u8; 20 * 20 * 4] }.to_png();
        std::fs::write(dir.join("ok.png"), &png).unwrap();
        let mut s = Session::new();
        s.execute("file.new", &json!({})).unwrap();
        let frame = |s: &mut Session, x: f64| {
            ItemId(s.execute("frame.create", &json!({"rect": [x, 72.0, x + 100.0, 172.0], "content": "graphic"})).unwrap()["id"].as_u64().unwrap())
        };
        let (first, second) = (frame(&mut s, 72.0), frame(&mut s, 300.0));
        s.execute("xml.tag", &json!({"tag": "photo", "ids": [first.0, second.0]})).unwrap();
        let xml = "<Root><photo/><photo href=\"ok.png\"/></Root>";
        let r = s.execute("file.importXml", &json!({"text": xml, "imageDir": dir.to_string_lossy()})).unwrap();
        assert_eq!(r["placed"].as_u64(), Some(1), "{r}");
        let holds = |id: ItemId| matches!(s.doc().unwrap().doc.item(id).unwrap().content, Content::Graphic(_));
        assert!(!holds(first) && holds(second), "the image goes to the second frame");
        let _ = std::fs::remove_dir_all(&dir);
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

    /// Hostile nesting is refused, not walked: deeper than the limit is an error.
    #[test]
    fn deep_nesting_is_refused() {
        let deep = format!("<Root><a>{}x{}</a></Root>", "<b>".repeat(super::MAX_XML_DEPTH), "</b>".repeat(super::MAX_XML_DEPTH));
        assert!(super::parse(&deep, &|_| false).err().is_some_and(|e| e.contains("nested deeper")));
        let ok = format!("<Root><a>{}x{}</a></Root>", "<b>".repeat(10), "</b>".repeat(10));
        assert!(super::parse(&ok, &|_| false).is_ok());
        let mut s = Session::new();
        s.execute("file.new", &json!({})).unwrap();
        assert!(s.execute("file.importXml", &json!({"text": deep})).is_err());
    }

    /// A frame takes an image only once it is placed: after a failed href, the next element of
    /// the tag goes into the frame that stayed empty.
    #[cfg(not(target_arch = "wasm32"))]
    /// Export escapes `%` and spaces in a path, import decodes them once: the path comes back.
    #[test]
    fn exported_hrefs_decode_to_their_path() {
        for path in ["C:/img/rate%20.png", "/home/me/a b%.png", "plain.png"] {
            assert_eq!(super::href_path(&format!("file://{}", super::href_escape(path)), None), path);
        }
    }

    /// A fit asked for on a locked frame can't happen: the image is reported, not placed.
    #[test]
    fn a_fit_on_a_locked_frame_fails() {
        use designcraft_doc::ItemId;
        let dir = std::env::temp_dir().join(format!("dc-xml-locked-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let png = designcraft_render::Rendered { width: 20, height: 20, pixels: vec![200u8; 20 * 20 * 4] }.to_png();
        std::fs::write(dir.join("ok.png"), &png).unwrap();
        let mut s = Session::new();
        s.execute("file.new", &json!({})).unwrap();
        let f = ItemId(s.execute("frame.create", &json!({"rect": [72, 72, 272, 272], "content": "graphic"})).unwrap()["id"].as_u64().unwrap());
        s.execute("xml.tag", &json!({"tag": "photo", "ids": [f.0]})).unwrap();
        s.execute("object.lock", &json!({"ids": [f.0]})).unwrap();
        let xml = "<Root><photo href=\"ok.png\" fit=\"fitProportionally\"/></Root>";
        let r = s.execute("file.importXml", &json!({"text": xml, "imageDir": dir.to_string_lossy()})).unwrap();
        assert_eq!(r["placed"].as_u64(), Some(0), "{r}");
        assert!(r["failed"][0]["error"].as_str().is_some_and(|e| e.contains("locked")), "{r}");
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// A locked frame doesn't hold back the next frame of its tag.
    #[test]
    fn a_locked_frame_leaves_the_image_to_the_next_one() {
        use designcraft_doc::{Content, ItemId};
        let dir = std::env::temp_dir().join(format!("dc-xml-skip-locked-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let png = designcraft_render::Rendered { width: 20, height: 20, pixels: vec![200u8; 20 * 20 * 4] }.to_png();
        std::fs::write(dir.join("ok.png"), &png).unwrap();
        let mut s = Session::new();
        s.execute("file.new", &json!({})).unwrap();
        let frame = |s: &mut Session, x: f64| {
            ItemId(s.execute("frame.create", &json!({"rect": [x, 72.0, x + 100.0, 172.0], "content": "graphic"})).unwrap()["id"].as_u64().unwrap())
        };
        let (first, second) = (frame(&mut s, 72.0), frame(&mut s, 300.0));
        s.execute("xml.tag", &json!({"tag": "photo", "ids": [first.0, second.0]})).unwrap();
        s.execute("object.lock", &json!({"ids": [first.0]})).unwrap();
        let r = s.execute("file.importXml", &json!({"text": "<Root><photo href=\"ok.png\"/></Root>", "imageDir": dir.to_string_lossy()})).unwrap();
        assert_eq!(r["placed"].as_u64(), Some(1), "{r}");
        assert!(matches!(s.doc().unwrap().doc.item(second).unwrap().content, Content::Graphic(_)), "the unlocked frame takes it");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_failed_image_leaves_its_frame_to_the_next_element() {
        use designcraft_doc::{Content, ItemId};
        let dir = std::env::temp_dir().join(format!("dc-xml-retry-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let png = designcraft_render::Rendered { width: 20, height: 20, pixels: vec![200u8; 20 * 20 * 4] }.to_png();
        std::fs::write(dir.join("ok.png"), &png).unwrap();
        let mut s = Session::new();
        s.execute("file.new", &json!({})).unwrap();
        let first = ItemId(s.execute("frame.create", &json!({"rect": [72, 72, 272, 272], "content": "graphic"})).unwrap()["id"].as_u64().unwrap());
        s.execute("xml.tag", &json!({"tag": "photo", "ids": [first.0]})).unwrap();
        let xml = "<Root><photo href=\"missing.png\"/><photo href=\"ok.png\"/></Root>";
        let r = s.execute("file.importXml", &json!({"text": xml, "imageDir": dir.to_string_lossy()})).unwrap();
        assert_eq!((r["placed"].as_u64(), r["unmatched"].as_u64()), (Some(1), Some(0)), "{r}");
        assert_eq!(r["failed"][0]["href"], "missing.png", "{r}");
        assert!(matches!(s.doc().unwrap().doc.item(first).unwrap().content, Content::Graphic(_)), "the second image fills the frame");
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
