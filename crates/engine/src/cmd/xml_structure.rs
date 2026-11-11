//! The document's XML structure (`XmlSettings::elements`, read from InDesign) as XML: export in
//! the form InDesign's File › Export (XML) writes (the root, story elements holding their text
//! with inline elements, tables as `aid:` tables, untagged ones in tagged text as CALS tables,
//! paragraphs ending in U+2029), and import of the texts of elements matched by their `Id`.

use std::collections::{BTreeMap, HashMap, HashSet};
use std::ops::Range;

use designcraft_compose::xref::XrefIndex;
use designcraft_doc::{Document, Story, StoryId, Table, XmlContent, XmlElement};
use serde_json::{Value, json};

use crate::{Result, Session};

const AID: &str = "http://ns.adobe.com/AdobeInDesign/4.0/";

/// Text content: `&`, `<` and `>` escaped (InDesign leaves quotes and tabs as they are).
fn esc_text(s: &str, out: &mut String) {
    for c in s.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            c => out.push(c),
        }
    }
}

/// An attribute value: markup characters, quotes and white space other than spaces escaped.
fn esc_attr(s: &str, out: &mut String) {
    for c in s.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '\t' => out.push_str("&#x9;"),
            '\n' => out.push_str("&#xa;"),
            '\r' => out.push_str("&#xd;"),
            c => out.push(c),
        }
    }
}

fn open_tag(out: &mut String, tag: &str, lead: &[(&str, String)], attrs: &[(String, String)]) {
    out.push('<');
    out.push_str(tag);
    for (k, v) in lead.iter().map(|(k, v)| (*k, v.as_str())).chain(attrs.iter().map(|(k, v)| (k.as_str(), v.as_str()))) {
        out.push(' ');
        out.push_str(k);
        out.push_str("=\"");
        esc_attr(v, out);
        out.push('"');
    }
    out.push('>');
}

/// Lookups from content to element, made once per export.
struct Index<'d> {
    d: &'d Document,
    /// Anchor pages for cross-references' text (`None`: their text shows `?` for the page).
    xrefs: Option<&'d XrefIndex>,
    story: HashMap<StoryId, u64>,
    table: HashMap<u64, u64>,
    cell: HashMap<(u64, usize), u64>,
}

impl<'d> Index<'d> {
    fn new(d: &'d Document, xrefs: Option<&'d XrefIndex>) -> Self {
        let mut ix = Index { d, xrefs, story: HashMap::new(), table: HashMap::new(), cell: HashMap::new() };
        for (id, e) in &d.xml.elements {
            match e.content {
                XmlContent::Story { story } => {
                    ix.story.entry(story).or_insert(*id);
                }
                XmlContent::Table { table, .. } => {
                    ix.table.entry(table).or_insert(*id);
                }
                XmlContent::Cell { table, cell } => {
                    ix.cell.entry((table, cell)).or_insert(*id);
                }
                XmlContent::None | XmlContent::Text => {}
            }
        }
        ix
    }

    fn el(&self, id: u64) -> Option<&'d XmlElement> {
        self.d.xml.elements.get(&id)
    }

    /// The inline elements around text tagged `id`, outermost first.
    fn inline_chain(&self, id: u64) -> Vec<u64> {
        chain_of(self.d, id)
    }

    fn element(&self, id: u64, out: &mut String, depth: usize) {
        let Some(e) = self.el(id) else { return };
        if depth > 64 {
            return;
        }
        match e.content {
            XmlContent::Story { story } => {
                let Some(st) = self.d.stories.get(&story) else { return };
                open_tag(out, &e.tag, &[], &e.attrs);
                self.story(st, out, depth);
                close(out, &e.tag);
            }
            XmlContent::None => {
                open_tag(out, &e.tag, &[], &e.attrs);
                for c in &e.children {
                    self.element(*c, out, depth + 1);
                }
                close(out, &e.tag);
            }
            // Found in their story or table.
            XmlContent::Table { .. } | XmlContent::Cell { .. } | XmlContent::Text => {}
        }
    }

    /// A story's text: inline elements opened and closed around their runs, paragraphs ending in
    /// U+2029, tables and anchored stories written where they are anchored.
    fn story(&self, st: &Story, out: &mut String, depth: usize) {
        let mut open: Vec<u64> = Vec::new();
        let paras = st.para_ranges();
        // Breaks the import added around a table's anchor paragraph: left out.
        let mut skip_breaks = std::collections::HashSet::new();
        for (pi, r) in paras.iter().enumerate() {
            let Some(tid) = st.paras.get(pi).and_then(|p| p.table) else { continue };
            if let Some(XmlContent::Table { added_break_before, added_break_after, .. }) =
                self.table.get(&tid).and_then(|e| self.el(*e)).map(|e| e.content)
            {
                if added_break_before && r.start > 0 {
                    skip_breaks.insert(r.start - 1);
                }
                if added_break_after {
                    skip_breaks.insert(r.end);
                }
            }
        }
        for (range, f) in st.runs() {
            let chain = f.over.xml_element.filter(|id| *id != 0).map(|id| self.inline_chain(id)).unwrap_or_default();
            let keep = open.iter().zip(&chain).take_while(|(a, b)| a == b).count();
            while open.len() > keep {
                if let Some(id) = open.pop()
                    && let Some(e) = self.el(id)
                {
                    close(out, &e.tag);
                }
            }
            for id in &chain[keep..] {
                if let Some(e) = self.el(*id) {
                    open_tag(out, &e.tag, &[], &e.attrs);
                    open.push(*id);
                }
            }
            for (i, c) in st.text[range.clone()].char_indices() {
                let at = range.start + i;
                match c {
                    '\n' if skip_breaks.contains(&at) => {}
                    '\n' => out.push('\u{2029}'),
                    designcraft_doc::story::TABLE_ANCHOR => {
                        let pi = st.para_at(at);
                        if let Some(t) = st.paras.get(pi).and_then(|p| p.table).and_then(|id| st.tables.get(&id)) {
                            self.table(t, out, depth);
                        }
                    }
                    designcraft_doc::OBJECT_MARK => {
                        // An anchored text frame whose story is an element (InDesign's Story in a cell).
                        if let Some(sid) = st.object_at(at).and_then(|o| o.item.text_frame()).map(|tf| tf.story)
                            && let Some(e) = self.story.get(&sid)
                        {
                            self.element(*e, out, depth + 1);
                        }
                    }
                    // A cross-reference's text, as InDesign writes it (the page it points to).
                    designcraft_doc::XREF_MARK => {
                        if let Some(x) = st.xref_at(at) {
                            esc_text(&designcraft_compose::xref::xref_text(self.d, x, self.xrefs), out);
                        }
                    }
                    // Other markers (anchors, notes, index entries) aren't text.
                    c if ('\u{E000}'..='\u{E1FF}').contains(&c) => {}
                    c => {
                        let mut b = [0u8; 4];
                        esc_text(c.encode_utf8(&mut b), out);
                    }
                }
            }
        }
        while let Some(id) = open.pop() {
            if let Some(e) = self.el(id) {
                close(out, &e.tag);
            }
        }
    }

    fn table(&self, t: &Table, out: &mut String, depth: usize) {
        let Some(e) = self.table.get(&t.id).and_then(|id| self.el(*id)) else {
            cals(self, t, out, depth);
            return;
        };
        let lead = [
            ("xmlns:aid", AID.to_string()),
            ("aid:table", "table".into()),
            ("aid:trows", t.nrows().to_string()),
            ("aid:tcols", t.ncols().to_string()),
        ];
        open_tag(out, &e.tag, &lead, &e.attrs);
        let (nr, nc) = (t.nrows(), t.ncols());
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
                let mut lead = vec![("aid:table", "cell".to_string())];
                if r < head {
                    lead.push(("aid:theader", String::new()));
                } else if r >= nr - foot {
                    lead.push(("aid:tfooter", String::new()));
                }
                lead.extend([("aid:crows", rs.to_string()), ("aid:ccols", cs.to_string()), ("aid:ccolwidth", width.to_string())]);
                let el = self.cell.get(&(t.id, r * nc + c)).and_then(|id| self.el(*id));
                let tag = el.map_or("Cell", |e| e.tag.as_str());
                open_tag(out, tag, &lead, el.map_or(&[][..], |e| &e.attrs));
                self.story(&cell.text, out, depth + 1);
                close(out, tag);
            }
        }
        close(out, &e.tag);
    }
}

fn close(out: &mut String, tag: &str) {
    out.push_str("</");
    out.push_str(tag);
    out.push('>');
}

/// An untagged table in tagged text, as InDesign writes one: a CALS table.
fn cals(ix: &Index, t: &Table, out: &mut String, depth: usize) {
    let (nr, nc) = (t.nrows(), t.ncols());
    // The table's outside (top, left, bottom, right) as CALS `frame`: a side counts when every
    // cell along it has a stroke there (the cell's own edge, or the table border it doesn't override).
    let owners = t.owners();
    let outer = |i: usize, cell: &designcraft_doc::Cell| {
        if cell.border_overrides[i] { cell.strokes[i].weight } else { t.options.border_for(i).weight.max(cell.strokes[i].weight) }
    };
    let side = |i: usize| {
        let along: Vec<(usize, usize)> = match i {
            0 => (0..nc).map(|c| (0, c)).collect(),
            2 => (0..nc).map(|c| (nr - 1, c)).collect(),
            1 => (0..nr).map(|r| (r, 0)).collect(),
            _ => (0..nr).map(|r| (r, nc - 1)).collect(),
        };
        nr > 0
            && nc > 0
            && along.iter().all(|&(r, c)| owners.get(r * nc + c).and_then(|&(or, oc)| t.cell(or, oc)).is_some_and(|cell| outer(i, cell) > 0.0))
    };
    let frame = match (side(0), side(2), side(1), side(3)) {
        (true, true, true, true) => "all",
        (true, true, _, _) => "topbot",
        (false, false, true, true) => "sides",
        (true, false, _, _) => "top",
        (false, true, _, _) => "bottom",
        _ => "none",
    };
    out.push_str(&format!("<table frame=\"{frame}\">\n"));
    out.push_str(&format!("<tgroup cols=\"{nc}\">\n"));
    for (i, col) in t.columns.iter().enumerate() {
        out.push_str(&format!("<colspec colname=\"c{}\" colwidth=\"{}pt\">\n</colspec>\n", i + 1, col.width));
    }
    out.push_str("<tbody>\n");
    for r in 0..nr {
        out.push_str("<row>\n");
        for c in 0..nc {
            if owners.get(r * nc + c) != Some(&(r, c)) {
                continue;
            }
            let Some(cell) = t.cell(r, c) else { continue };
            let align = match cell.text.paras.first().map(|p| ix.d.styles.resolve_para(p).0.align) {
                Some(designcraft_doc::Align::Center | designcraft_doc::Align::CenterJustified) => "center",
                Some(designcraft_doc::Align::Right | designcraft_doc::Align::RightJustified) => "right",
                Some(designcraft_doc::Align::LeftJustified | designcraft_doc::Align::FullyJustified) => "justify",
                _ => "left",
            };
            let valign = match cell.vj {
                designcraft_doc::VerticalJustification::Center => "middle",
                designcraft_doc::VerticalJustification::Bottom => "bottom",
                _ => "top",
            };
            let rs = (cell.row_span.max(1) as usize).min(nr - r);
            let cs = (cell.col_span.max(1) as usize).min(nc - c);
            // An edge on the table's outside takes the table border unless the cell overrides it.
            let edge = |i: usize, outside: bool| {
                if outside && !cell.border_overrides[i] { t.options.border_for(i).weight } else { cell.strokes[i].weight }
            };
            // CALS separators: written as 0 where the cell has no stroke, left out where it has one.
            out.push_str("<entry");
            if edge(3, c + cs == nc) <= 0.0 {
                out.push_str(" colsep=\"0\"");
            }
            if edge(2, r + rs == nr) <= 0.0 {
                out.push_str(" rowsep=\"0\"");
            }
            // A merged cell: the columns it spans by their colspec names, the rows below it it takes.
            if cs > 1 {
                out.push_str(&format!(" namest=\"c{}\" nameend=\"c{}\"", c + 1, c + cs));
            }
            if rs > 1 {
                out.push_str(&format!(" morerows=\"{}\"", rs - 1));
            }
            out.push_str(&format!(" align=\"{align}\" valign=\"{valign}\">"));
            ix.story(&cell.text, out, depth + 1);
            out.push_str("</entry>\n");
        }
        out.push_str("</row>\n");
    }
    out.push_str("</tbody>\n</tgroup>\n</table>");
}

/// The document's XML structure as InDesign exports it; `xrefs` gives cross-references the page
/// they point to.
pub fn export(d: &Document, xrefs: Option<&XrefIndex>) -> String {
    let ix = Index::new(d, xrefs);
    let root = if d.xml.root.is_empty() { "Root" } else { d.xml.root.as_str() };
    let mut out = String::from("<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?>\n");
    out.push('<');
    out.push_str(root);
    out.push('>');
    for id in &d.xml.root_children {
        ix.element(*id, &mut out, 0);
    }
    close(&mut out, root);
    out.push('\n');
    out
}

// ---------- outline ----------

/// How much of an element's text the outline shows.
const OUTLINE_TEXT: usize = 60;

/// The document's XML structure as the Structure pane lists it: every element in document order,
/// `depth` levels under the root, with the start of its text and the frame showing it (an inline
/// element's: its story's). Tables and cells are left out: `xml.cells` lists a frame's.
pub fn outline(d: &Document) -> Vec<Value> {
    let ix = Index::new(d, None);
    let mut frames: HashMap<StoryId, u64> = HashMap::new();
    for id in d.all_items() {
        if let Some(tf) = d.item(id).and_then(|it| it.text_frame()) {
            frames.entry(tf.story).or_insert(id.0);
        }
    }
    let mut out = Vec::new();
    for id in &d.xml.root_children {
        outline_element(&ix, &frames, *id, 0, &mut out);
    }
    out
}

fn outline_element(ix: &Index, frames: &HashMap<StoryId, u64>, id: u64, depth: usize, out: &mut Vec<Value>) {
    let Some(e) = ix.el(id) else { return };
    if depth > 64 {
        return;
    }
    match e.content {
        XmlContent::Story { story } => {
            let Some(st) = ix.d.stories.get(&story) else { return };
            let frame = frames.get(&story).copied();
            out.push(json!({"element": id, "tag": e.tag, "depth": depth, "kind": "story", "text": outline_text(&st.text), "frame": frame}));
            outline_inline(ix, st, frame, depth + 1, out);
        }
        XmlContent::None => {
            out.push(json!({"element": id, "tag": e.tag, "depth": depth, "kind": "element", "text": "", "frame": null}));
            for c in &e.children {
                outline_element(ix, frames, *c, depth + 1, out);
            }
        }
        XmlContent::Table { .. } | XmlContent::Cell { .. } | XmlContent::Text => {}
    }
}

/// A story's inline elements, each where its text first appears, with the text it marks.
fn outline_inline(ix: &Index, st: &Story, frame: Option<u64>, depth: usize, out: &mut Vec<Value>) {
    let mut at: HashMap<u64, (usize, String)> = HashMap::new();
    for (range, f) in st.runs() {
        let chain = f.over.xml_element.filter(|id| *id != 0).map(|id| ix.inline_chain(id)).unwrap_or_default();
        for (level, id) in chain.iter().enumerate() {
            let entry = at.entry(*id).or_insert_with(|| {
                let tag = ix.el(*id).map_or("", |e| e.tag.as_str());
                out.push(json!({"element": id, "tag": tag, "depth": depth + level, "kind": "text", "text": "", "frame": frame}));
                (out.len() - 1, String::new())
            });
            if entry.1.chars().count() < OUTLINE_TEXT {
                entry.1.push_str(&st.text[range.clone()]);
            }
        }
    }
    for (i, text) in at.into_values() {
        out[i]["text"] = json!(outline_text(&text));
    }
}

/// The start of `text` on one line: markers left out, breaks as spaces.
fn outline_text(text: &str) -> String {
    let t: String = text
        .chars()
        .filter(|c| !('\u{E000}'..='\u{E1FF}').contains(c) && *c != '\u{FEFF}')
        .map(|c| if c.is_whitespace() { ' ' } else { c })
        .take(OUTLINE_TEXT)
        .collect();
    t.trim().to_string()
}

// ---------- import by Id ----------

/// The texts of the elements with an `Id` attribute in `xml`, paragraphs separated by `\n`
/// (InDesign writes U+2029).
#[derive(Default)]
struct IdTexts {
    /// Id → the element's own text: text inside an element nested in it with an Id of its own
    /// belongs to that one (the innermost element with an Id).
    own: BTreeMap<String, String>,
    /// Id → its own text in pieces around the elements with an Id of their own in it (not a table's
    /// text, which is its cells'): what a parent puts around its children, which fill themselves.
    parts: BTreeMap<String, Vec<Piece>>,
}

/// A piece of a parent's text in the XML: its own text, or the place of a child (by its Id).
#[derive(Clone, Debug, PartialEq)]
enum Piece {
    Text(String),
    Child(String),
}

/// The innermost open element with an Id, unless a table lies between it and what comes now.
fn owner(stack: &[(Option<String>, bool)]) -> Option<&String> {
    let mut in_table = false;
    for (id, table) in stack.iter().rev() {
        if let Some(id) = id {
            return (!in_table).then_some(id);
        }
        in_table |= *table;
    }
    None
}

/// Whether `xml` addresses elements by an `Id` (so `file.importXml` fills them by Id).
pub(super) fn addresses_ids(xml: &str) -> bool {
    texts_by_id(xml).is_ok_and(|t| !t.own.is_empty())
}

/// The CALS tables in `xml`, as export writes an untagged table, by the place of their start among
/// the elements: `<table frame="…">` whose first element is a `tgroup`, with no text of its own (not
/// any element named table).
fn cals_tables(xml: &str) -> std::result::Result<HashSet<usize>, String> {
    use quick_xml::events::Event;
    // Open elements: their place, whether they may be CALS, whether their first element was a
    // tgroup (none yet: `None`), and whether text of their own came.
    let mut stack: Vec<(usize, bool, Option<bool>, bool)> = Vec::new();
    let (mut out, mut n) = (HashSet::new(), 0usize);
    let mut r = quick_xml::Reader::from_str(xml);
    loop {
        match r.read_event().map_err(|e| e.to_string())? {
            ev @ (Event::Start(_) | Event::Empty(_)) => {
                let (Event::Start(e) | Event::Empty(e)) = &ev else { continue };
                if let Some(top) = stack.last_mut() {
                    top.2.get_or_insert(e.local_name().as_ref() == b"tgroup");
                }
                if matches!(ev, Event::Start(_)) {
                    let cand = e.local_name().as_ref() == b"table" && e.attributes().flatten().any(|a| a.key.as_ref() == b"frame");
                    stack.push((n, cand, None, false));
                }
                n += 1;
            }
            Event::End(_) => {
                if let Some((at, true, Some(true), false)) = stack.pop() {
                    out.insert(at);
                }
            }
            Event::Text(t) => {
                if let Some(top) = stack.last_mut() {
                    top.3 |= !t.decode().map_err(|e| e.to_string())?.trim().is_empty();
                }
            }
            Event::GeneralRef(_) | Event::CData(_) => {
                if let Some(top) = stack.last_mut() {
                    top.3 = true;
                }
            }
            Event::Eof => break,
            _ => {}
        }
    }
    Ok(out)
}

fn texts_by_id(xml: &str) -> std::result::Result<IdTexts, String> {
    use quick_xml::events::Event;
    let cals = cals_tables(xml)?;
    let mut r = quick_xml::Reader::from_str(xml);
    // Open elements: the Id of each one that has one, and whether it is a table (a tagged one,
    // aid:table="table", or an untagged one written as CALS).
    let mut stack: Vec<(Option<String>, bool)> = Vec::new();
    let mut texts = IdTexts::default();
    let mut n = 0usize;
    let mut id_of = |e: &quick_xml::events::BytesStart| -> std::result::Result<(Option<String>, bool), String> {
        let (mut id, mut table) = (None, cals.contains(&n));
        n += 1;
        for a in e.attributes() {
            let a = a.map_err(|e| e.to_string())?;
            let value = a.normalized_value(quick_xml::XmlVersion::Implicit1_0).map_err(|e| e.to_string())?;
            if a.key.local_name().as_ref() == b"Id" {
                id = Some(value.to_string());
            } else if a.key.as_ref() == b"aid:table" && value == "table" {
                table = true;
            }
        }
        Ok((id, table))
    };
    loop {
        match r.read_event().map_err(|e| e.to_string())? {
            Event::Start(e) => {
                let (id, table) = id_of(&e)?;
                if let Some(i) = &id {
                    texts.own.entry(i.clone()).or_default();
                    texts.parts.entry(i.clone()).or_default();
                    // A table is no child of its parent's text: its text is its cells'.
                    if !table && let Some(p) = owner(&stack) {
                        texts.parts.entry(p.clone()).or_default().push(Piece::Child(i.clone()));
                    }
                }
                stack.push((id, table));
            }
            Event::Empty(e) => {
                if let (Some(i), table) = id_of(&e)? {
                    texts.own.entry(i.clone()).or_default();
                    texts.parts.entry(i.clone()).or_default();
                    if !table && let Some(p) = owner(&stack) {
                        texts.parts.entry(p.clone()).or_default().push(Piece::Child(i));
                    }
                }
            }
            Event::End(_) => {
                stack.pop();
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
                let s = s.replace('\u{2029}', "\n");
                if let Some((Some(id), _)) = stack.iter().rev().find(|(i, _)| i.is_some())
                    && let Some(t) = texts.own.get_mut(id)
                {
                    t.push_str(&s);
                }
                if let Some(o) = owner(&stack)
                    && let Some(parts) = texts.parts.get_mut(o)
                {
                    match parts.last_mut() {
                        Some(Piece::Text(t)) => t.push_str(&s),
                        _ => parts.push(Piece::Text(s)),
                    }
                }
            }
            Event::Eof => break,
            _ => {}
        }
    }
    Ok(texts)
}

/// Each inline element's text in `st`: the runs tagged with it, from its first to its last.
fn element_ranges(st: &Story) -> Vec<(u64, Range<usize>)> {
    let mut out: Vec<(u64, Range<usize>)> = Vec::new();
    let mut at: HashMap<u64, usize> = HashMap::new();
    for (r, f) in st.runs() {
        let Some(id) = f.over.xml_element.filter(|id| *id != 0) else { continue };
        match at.get(&id) {
            Some(&i) => out[i].1.end = r.end,
            None => {
                at.insert(id, out.len());
                out.push((id, r));
            }
        }
    }
    out
}

/// Lengths (in chars) of the common start of `a` and `b`, and of the common end of what is left.
fn common_ends(a: &[char], b: &[char]) -> (usize, usize) {
    let pre = a.iter().zip(b).take_while(|(x, y)| x == y).count();
    let suf = a[pre..].iter().rev().zip(b[pre..].iter().rev()).take_while(|(x, y)| x == y).count();
    (pre, suf)
}

/// All chars of a cross-reference's text stand for its one marker (`at`: each char's story bytes):
/// a common start (`pre`) or end (`suf`) that stops inside such a text moves to its edge, so the
/// replacement holds the whole new text of the reference ("12" → "13" replaces "12", not "2").
fn to_marker_edges(mut pre: usize, mut suf: usize, at: &[Range<usize>]) -> (usize, usize) {
    while pre > 0 && pre < at.len() && at[pre - 1] == at[pre] {
        pre -= 1;
    }
    while suf > 0 && suf < at.len() && at[at.len() - suf] == at[at.len() - suf - 1] {
        suf -= 1;
    }
    (pre, suf)
}

/// Puts `new` into `range` of `st`, replacing only the part that differs, so the formatting of
/// what stays stays and changed text takes the format of the first character it replaces.
/// Cross-references keep their place when their text (as export writes it) is still in `new`;
/// other markers (footnotes, anchors…, not in the XML) always stay.
fn replace_text(st: &mut Story, range: Range<usize>, new: &str, d: &Document, xrefs: Option<&XrefIndex>) {
    // The element's text as export writes it, char by char, each with its byte range in the story.
    let mut view: Vec<char> = Vec::new();
    let mut at: Vec<Range<usize>> = Vec::new();
    for (i, c) in st.text[range.clone()].char_indices() {
        let pos = range.start + i;
        let bytes = pos..pos + c.len_utf8();
        if c == designcraft_doc::XREF_MARK {
            if let Some(x) = st.xref_at(pos) {
                for xc in designcraft_compose::xref::xref_text(d, x, xrefs).chars() {
                    view.push(xc);
                    at.push(bytes.clone());
                }
            }
        } else if ('\u{E000}'..='\u{E1FF}').contains(&c) {
            // Other markers aren't in the XML: they stay where they are.
            continue;
        } else {
            view.push(c);
            at.push(bytes);
        }
    }
    let new: Vec<char> = new.chars().collect();
    let (pre, suf) = common_ends(&view, &new);
    if pre == view.len() && pre == new.len() {
        return;
    }
    let (pre, suf) = to_marker_edges(pre, suf, &at);
    // The story bytes the change covers, widened to whole markers.
    let a = at.get(pre).map_or_else(|| at.last().map_or(range.start, |r| r.end), |r| r.start);
    let b = if suf == 0 { at.last().map_or(range.end, |r| r.end) } else { at[view.len() - suf].start };
    let (a, b) = (a.min(b), a.max(b));
    let repl: String = new[pre..new.len() - suf].iter().collect();
    // The markers in the changed part: cross-references (with their text) and the others (footnotes,
    // anchors, index entries…, which the XML doesn't show).
    let markers: Vec<(usize, usize, Option<String>)> = st.text[a..b]
        .char_indices()
        .filter_map(|(i, c)| {
            let pos = a + i;
            if c == designcraft_doc::XREF_MARK {
                st.xref_at(pos).map(|x| (pos, c.len_utf8(), Some(designcraft_compose::xref::xref_text(d, x, xrefs))))
            } else {
                ('\u{E000}'..='\u{E1FF}').contains(&c).then_some((pos, c.len_utf8(), None))
            }
        })
        .collect();
    // The replacement split around the kept markers, in order: a cross-reference stays where its text
    // is found in the replacement (else it goes with the replaced text); every other marker stays,
    // after the new text up to the next reference that stays.
    let mut pieces: Vec<String> = Vec::new();
    let mut marks: Vec<(usize, usize)> = Vec::new();
    let mut rest = repl.as_str();
    for (n, (pos, len, text)) in markers.iter().enumerate() {
        let k = match text {
            Some(t) => match rest.find(t.as_str()).filter(|_| !t.is_empty()) {
                Some(k) => {
                    pieces.push(rest[..k].to_string());
                    marks.push((*pos, *len));
                    rest = &rest[k + t.len()..];
                    continue;
                }
                None => continue,
            },
            None => {
                markers[n + 1..].iter().find_map(|(_, _, t)| t.as_deref().filter(|t| !t.is_empty()).and_then(|t| rest.find(t))).unwrap_or(rest.len())
            }
        };
        pieces.push(rest[..k].to_string());
        marks.push((*pos, *len));
        rest = &rest[k..];
    }
    pieces.push(rest.to_string());
    // Replace the stretches between the kept marks, last first so the earlier ones stay put.
    let mut bounds = vec![a];
    for (m, len) in &marks {
        bounds.push(*m);
        bounds.push(m + len);
    }
    bounds.push(b);
    let stretches: Vec<Range<usize>> = bounds.chunks(2).map(|w| w[0]..w[1]).collect();
    for (r, text) in stretches.into_iter().zip(pieces).rev() {
        if r.start < r.end {
            st.replace(r, &text);
        } else if !text.is_empty() {
            // Nothing to replace: insert inside the element, in the format of the text before.
            let fmt = st.char_format_at(r.start).clone();
            st.insert_with(r.start, &text, fmt);
        }
    }
}

/// Puts the texts in `xml` into the elements whose `Id` they carry, in every story and table
/// cell. Elements of the XML without an Id are its structure and change nothing.
pub fn import_by_id(s: &mut Session, xml: &str) -> Result<Value> {
    const ID: &str = "file.importXml";
    let texts = texts_by_id(xml).map_err(|e| super::bad(ID, format!("not well-formed XML: {e}")))?;
    let d = &s.doc()?.doc;
    // The document's elements by their Id.
    let mut by_id: HashMap<&str, u64> = HashMap::new();
    for (id, e) in &d.xml.elements {
        if let Some(v) = e.attr("Id") {
            by_id.entry(v).or_insert(*id);
        }
    }
    // Inline elements take their own text, first the innermost; an inline element with inline
    // elements in it then all its text (theirs included: its range runs across them), and story and
    // cell elements all theirs last, so none undoes another. A table's text goes into its cells.
    let mut want: HashMap<u64, String> = HashMap::new();
    let mut outer: HashMap<u64, Parent> = HashMap::new();
    let mut whole: Vec<(XmlContent, u64, Parent)> = Vec::new();
    let mut unmatched: Vec<String> = Vec::new();
    let mut tables: Vec<String> = Vec::new();
    let has_inline = |e: u64| d.xml.elements.values().any(|c| c.parent == e && c.content == XmlContent::Text);
    for (k, t) in &texts.own {
        let Some(&e) = by_id.get(k.as_str()) else {
            unmatched.push(k.clone());
            continue;
        };
        let parent = || Parent { parts: texts.parts.get(k).cloned().unwrap_or_default() };
        match d.xml.elements.get(&e).map(|el| el.content) {
            Some(c @ (XmlContent::Story { .. } | XmlContent::Cell { .. })) => whole.push((c, e, parent())),
            Some(XmlContent::Table { .. }) => tables.push(k.clone()),
            _ if has_inline(e) => {
                outer.insert(e, parent());
            }
            _ => {
                want.insert(e, t.clone());
            }
        }
    }
    let matched = texts.own.len() - unmatched.len();
    let xrefs = s.cache.xref_index(d);
    let mut placed = 0usize;
    let mut failed: Vec<u64> = Vec::new();
    let mut mismatched: Vec<u64> = Vec::new();
    s.edit(|d, _| {
        let snapshot = d.clone();
        let ids: Vec<StoryId> = d.stories.keys().copied().collect();
        for sid in &ids {
            let holds = d.story(*sid).is_some_and(|st| holds_any(st, &want));
            if holds && let Some(st) = d.story_mut(*sid) {
                placed += fill(st, &want, &snapshot, Some(&xrefs), &mut failed);
            }
        }
        for sid in &ids {
            let holds = d.story(*sid).is_some_and(|st| holds_parent(st, &outer, &snapshot));
            if holds && let Some(st) = d.story_mut(*sid) {
                placed += fill_parents(st, &outer, &snapshot, Some(&xrefs), &mut mismatched);
            }
        }
        for (content, e, p) in &whole {
            let target = match *content {
                XmlContent::Story { story } => d.story_mut(story),
                XmlContent::Cell { table, cell } => ids
                    .iter()
                    .find_map(|sid| d.story(*sid).is_some_and(|st| holds_table(st, table)).then_some(*sid))
                    .and_then(|sid| d.story_mut(sid).and_then(|st| cell_text_mut(st, table, cell))),
                _ => None,
            };
            if let Some(st) = target {
                let end = st.text.len();
                if put_parts(st, 0..end, None, p, &snapshot, Some(&xrefs)) {
                    st.rev = st.rev.wrapping_add(1);
                    placed += 1;
                } else {
                    mismatched.push(*e);
                }
            }
        }
        Ok(())
    })?;
    let mut failed: Vec<Value> = failed
        .iter()
        .filter_map(|e| d_attr_id(s, *e))
        .map(|id| json!({"id": id, "error": "an inline element can't be emptied: it would lose its place in the text"}))
        .collect();
    failed.extend(
        mismatched
            .iter()
            .filter_map(|e| d_attr_id(s, *e))
            .map(|id| json!({"id": id, "error": "its elements with Ids aren't the document's (in that order): left as it was"})),
    );
    failed.extend(tables.iter().map(|id| json!({"id": id, "error": "a table's text goes into its cells: address the cells by their Ids"})));
    Ok(json!({
        "placed": placed,
        "matched": matched,
        "unmatched": unmatched.len(),
        "unmatchedIds": unmatched.iter().take(50).collect::<Vec<_>>(),
        "failed": failed,
    }))
}

/// The `Id` attribute of element `e`.
fn d_attr_id(s: &Session, e: u64) -> Option<String> {
    s.doc().ok()?.doc.xml.elements.get(&e)?.attr("Id").map(str::to_string)
}

/// Whether `st` (or a table in one of its cells) holds the table `table`.
fn holds_table(st: &Story, table: u64) -> bool {
    st.tables.contains_key(&table) || st.tables.values().any(|t| t.cells.iter().any(|c| holds_table(&c.text, table)))
}

/// The text of cell `cell` of table `table` in `st` (or in a table nested in its cells).
fn cell_text_mut(st: &mut Story, table: u64, cell: usize) -> Option<&mut Story> {
    let tid = st.tables.iter().find(|(id, t)| **id == table || t.cells.iter().any(|c| holds_table(&c.text, table))).map(|(id, _)| *id)?;
    let t = std::sync::Arc::make_mut(st.tables.get_mut(&tid)?);
    st.rev = st.rev.wrapping_add(1);
    if tid == table {
        return t.cells.get_mut(cell).map(|c| &mut c.text);
    }
    t.cells.iter_mut().find(|c| holds_table(&c.text, table)).and_then(|c| cell_text_mut(&mut c.text, table, cell))
}

/// The wanted elements' texts put into `st` and its table cells; returns how many. An element whose
/// new text is empty keeps its text (and its place) and goes to `failed`.
fn fill(st: &mut Story, want: &HashMap<u64, String>, d: &Document, xrefs: Option<&XrefIndex>, failed: &mut Vec<u64>) -> usize {
    let mut n = 0;
    let mut todo: Vec<(u64, Range<usize>)> = element_ranges(st).into_iter().filter(|(id, _)| want.contains_key(id)).collect();
    todo.sort_by_key(|(_, r)| std::cmp::Reverse(r.start));
    for (id, r) in todo {
        if let Some(t) = want.get(&id) {
            if t.is_empty() && !r.is_empty() {
                failed.push(id);
                continue;
            }
            replace_text(st, r, t, d, xrefs);
            n += 1;
        }
    }
    // Cells: only tables holding a wanted element are copied for the change.
    let table_ids: Vec<u64> = st.tables.keys().copied().collect();
    for tid in table_ids {
        if !st.tables.get(&tid).is_some_and(|t| t.cells.iter().any(|c| holds_any(&c.text, want))) {
            continue;
        }
        if let Some(t) = st.tables.get_mut(&tid) {
            let t = std::sync::Arc::make_mut(t);
            for cell in &mut t.cells {
                n += fill(&mut cell.text, want, d, xrefs, failed);
            }
        }
    }
    if n > 0 {
        st.rev = st.rev.wrapping_add(1);
    }
    n
}

/// Whether `st` or a table in it holds text of a wanted element.
fn holds_any<V>(st: &Story, want: &HashMap<u64, V>) -> bool {
    st.runs().any(|(_, f)| f.over.xml_element.is_some_and(|id| want.contains_key(&id)))
        || st.tables.values().any(|t| t.cells.iter().any(|c| holds_any(&c.text, want)))
}

/// Whether `st` or a table in it holds text inside a wanted parent (its own, or only its children's).
fn holds_parent<V>(st: &Story, want: &HashMap<u64, V>, d: &Document) -> bool {
    st.runs().any(|(_, f)| f.over.xml_element.is_some_and(|id| chain_of(d, id).iter().any(|e| want.contains_key(e))))
        || st.tables.values().any(|t| t.cells.iter().any(|c| holds_parent(&c.text, want, d)))
}

/// A parent element's text in the XML: its own pieces around its children with Ids.
struct Parent {
    parts: Vec<Piece>,
}

/// The inline elements with children: each one's own pieces put around its children in `st` and its
/// table cells, the innermost first (each on the stretches as the ones before left them); returns
/// how many. A parent whose children aren't the XML's goes to `mismatched`.
fn fill_parents(st: &mut Story, want: &HashMap<u64, Parent>, d: &Document, xrefs: Option<&XrefIndex>, mismatched: &mut Vec<u64>) -> usize {
    let mut n = 0;
    let mut done: Vec<u64> = Vec::new();
    loop {
        let next = element_spans(st, d)
            .into_iter()
            .filter(|(id, _)| want.contains_key(id) && !done.contains(id))
            .max_by_key(|(id, r)| (chain_of(d, *id).len(), r.start));
        let Some((id, r)) = next else { break };
        done.push(id);
        if let Some(p) = want.get(&id) {
            if put_parts(st, r, Some(id), p, d, xrefs) {
                n += 1;
            } else {
                mismatched.push(id);
            }
        }
    }
    let table_ids: Vec<u64> = st.tables.keys().copied().collect();
    for tid in table_ids {
        if !st.tables.get(&tid).is_some_and(|t| t.cells.iter().any(|c| holds_parent(&c.text, want, d))) {
            continue;
        }
        if let Some(t) = st.tables.get_mut(&tid) {
            let t = std::sync::Arc::make_mut(t);
            for cell in &mut t.cells {
                n += fill_parents(&mut cell.text, want, d, xrefs, mismatched);
            }
        }
    }
    if n > 0 {
        st.rev = st.rev.wrapping_add(1);
    }
    n
}

/// The inline elements around text tagged `id`, outermost first, up to its story's or cell's element.
/// Elements without content of their own between (a page item's, which isn't kept) count too inside a
/// story or cell, not under the root, which writes those itself.
fn chain_of(d: &Document, id: u64) -> Vec<u64> {
    let (mut chain, mut between, mut inside) = (Vec::new(), Vec::new(), false);
    let mut cur = id;
    while let Some(e) = d.xml.elements.get(&cur) {
        if chain.len() + between.len() > 64 {
            break;
        }
        match e.content {
            XmlContent::Text => {
                chain.append(&mut between);
                chain.push(cur);
            }
            XmlContent::None if !d.xml.root_children.contains(&cur) => between.push(cur),
            XmlContent::Story { .. } | XmlContent::Table { .. } | XmlContent::Cell { .. } => {
                inside = true;
                break;
            }
            XmlContent::None => break,
        }
        cur = e.parent;
    }
    if inside {
        chain.append(&mut between);
    }
    chain.reverse();
    chain
}

/// Each element's whole stretch in `st`: from the first to the last run whose chain holds it (its
/// own text and that of the elements in it).
fn element_spans(st: &Story, d: &Document) -> Vec<(u64, Range<usize>)> {
    let mut out: Vec<(u64, Range<usize>)> = Vec::new();
    let mut at: HashMap<u64, usize> = HashMap::new();
    for (r, f) in st.runs() {
        let Some(id) = f.over.xml_element.filter(|id| *id != 0) else { continue };
        for e in chain_of(d, id) {
            match at.get(&e) {
                Some(&i) => out[i].1.end = r.end,
                None => {
                    at.insert(e, out.len());
                    out.push((e, r.clone()));
                }
            }
        }
    }
    out
}

/// The element below `parent` (an inline element, or with `None` the story or cell itself) that has
/// an Id of its own and holds the text tagged `id`, if any.
fn child_of(d: &Document, id: u64, parent: Option<u64>) -> Option<u64> {
    let chain = chain_of(d, id);
    let below = match parent {
        Some(p) => &chain[chain.iter().position(|e| *e == p)? + 1..],
        None => &chain[..],
    };
    below.iter().copied().find(|e| d.xml.elements.get(e).is_some_and(|el| el.attr("Id").is_some()))
}

/// A parent's own pieces put into `range` of `st` around its children (its elements with an Id of
/// their own), whose text, format and element stay as their own import left them. When the
/// document's children aren't the XML's, in that order, nothing changes and it returns false. A
/// child the document knows but holds no text of here has no stretch to keep: the pieces around it join.
fn put_parts(st: &mut Story, range: Range<usize>, parent: Option<u64>, p: &Parent, d: &Document, xrefs: Option<&XrefIndex>) -> bool {
    // The children's text in the story, in order.
    let mut children: Vec<(u64, Range<usize>)> = Vec::new();
    for (r, f) in st.runs() {
        let (a, b) = (r.start.max(range.start), r.end.min(range.end));
        if a >= b {
            continue;
        }
        let Some(child) = f.over.xml_element.and_then(|id| child_of(d, id, parent)) else { continue };
        match children.last_mut() {
            Some((c, s)) if *c == child && s.end == a => s.end = b,
            _ => children.push((child, a..b)),
        }
    }
    let here: Vec<&str> = children.iter().filter_map(|(c, _)| d.xml.elements.get(c).and_then(|e| e.attr("Id"))).collect();
    // The XML's own pieces around its children.
    let known = |c: &str| d.xml.elements.values().any(|e| e.attr("Id") == Some(c));
    let mut pieces = vec![String::new()];
    let mut xml_children: Vec<&str> = Vec::new();
    for part in &p.parts {
        match part {
            Piece::Text(t) => pieces.last_mut().map_or((), |last| last.push_str(t)),
            Piece::Child(c) if here.contains(&c.as_str()) || !known(c) => {
                xml_children.push(c);
                pieces.push(String::new());
            }
            Piece::Child(_) => {}
        }
    }
    if xml_children != here {
        return false;
    }
    // The parent's own stretches between its children, last first so the earlier ones stay put.
    let mut own = Vec::new();
    let mut at = range.start;
    for (_, c) in &children {
        own.push(at..c.start);
        at = c.end;
    }
    own.push(at..range.end);
    for (r, piece) in own.into_iter().zip(pieces).rev() {
        if !r.is_empty() {
            let (before, start, end) = (st.text.len(), r.start, r.end);
            replace_text(st, r, &piece, d, xrefs);
            // The stretch as the change left it.
            own_element(st, start..(end + st.text.len()).saturating_sub(before), parent, d);
        } else if !piece.is_empty() {
            // Between two children (or at an edge): the parent's text, not a child's.
            let mut fmt = st.char_format_at(r.start).clone();
            fmt.over.xml_element = parent;
            st.insert_with(r.start, &piece, fmt);
        }
    }
    true
}

/// Text in a parent's own stretch `range` of `st` that took a neighbour's element (a child's, or
/// one outside the parent) goes back to the parent; elements without an Id inside it stay.
fn own_element(st: &mut Story, range: Range<usize>, parent: Option<u64>, d: &Document) {
    let leaked: Vec<Range<usize>> = st
        .runs()
        .filter(|(r, _)| r.start < range.end && r.end > range.start)
        .filter(|(_, f)| match f.over.xml_element.filter(|id| *id != 0) {
            Some(id) => child_of(d, id, parent).is_some() || parent.is_some_and(|p| !chain_of(d, id).contains(&p)),
            None => parent.is_some(),
        })
        .map(|(r, _)| r.start.max(range.start)..r.end.min(range.end))
        .collect();
    for r in leaked {
        st.format_chars(r, |f| f.over.xml_element = parent);
    }
}

#[cfg(test)]
mod tests {
    use designcraft_doc::{StoryId, XmlContent, XmlElement};
    use serde_json::json;

    use crate::Session;

    /// "Hallo Welt" in a story element, "Hallo" bold and in a Translation element with Id 7.
    fn tagged() -> (Session, StoryId) {
        let mut s = Session::new();
        s.execute("file.new", &json!({})).unwrap();
        let f = s.execute("frame.create", &json!({"rect": [72, 72, 400, 200], "content": "text", "text": "Hallo Welt", "caret": false})).unwrap();
        let sid = StoryId(f["story"].as_u64().unwrap());
        s.execute("text.select", &json!({"story": sid.0, "anchor": 0, "focus": 5})).unwrap();
        s.execute("type.char", &json!({"attrs": {"fontStyle": "Bold"}})).unwrap();
        s.edit(|d, _| {
            let el = |tag: &str, attrs: &[(&str, &str)], parent: u64, content: XmlContent| XmlElement {
                tag: tag.into(),
                attrs: attrs.iter().map(|(k, v)| (k.to_string(), v.to_string())).collect(),
                parent,
                content,
                children: Vec::new(),
            };
            d.xml.root = "Root".into();
            d.xml.elements.insert(1, el("Story", &[], 0, XmlContent::Story { story: sid }));
            d.xml.elements.insert(2, el("Translation", &[("Id", "7"), ("SourceText", "Hallo \"x\" <y>")], 1, XmlContent::Text));
            d.xml.root_children = vec![1];
            if let Some(st) = d.story_mut(sid) {
                st.format_chars(0..5, |f| f.over.xml_element = Some(2));
            }
            Ok(())
        })
        .unwrap();
        (s, sid)
    }

    fn export(s: &mut Session) -> String {
        s.execute("file.exportXml", &json!({})).unwrap()["text"].as_str().unwrap().to_string()
    }

    #[test]
    fn exports_the_structure_as_indesign_writes_it() {
        let (mut s, _) = tagged();
        assert_eq!(
            export(&mut s),
            "<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?>\n<Root><Story><Translation Id=\"7\" SourceText=\"Hallo &quot;x&quot; &lt;y&gt;\">Hallo</Translation> Welt</Story></Root>\n"
        );
    }

    /// The Tags panel's Structure lists the elements read from InDesign, not only tagged frames.
    #[test]
    fn structure_lists_the_elements_with_their_text_and_frame() {
        let (mut s, _) = tagged();
        let st = s.execute("xml.structure", &json!({})).unwrap();
        let frame = s.doc().unwrap().doc.all_items()[0].0;
        assert_eq!(
            st["tree"],
            json!([
                {"element": 1, "tag": "Story", "depth": 0, "kind": "story", "text": "Hallo Welt", "frame": frame},
                {"element": 2, "tag": "Translation", "depth": 1, "kind": "text", "text": "Hallo", "frame": frame},
            ])
        );
    }

    #[test]
    fn import_puts_texts_into_the_elements_by_id_and_keeps_their_format() {
        let (mut s, sid) = tagged();
        let xml =
            "<Root><Story><Translation Id=\"7\">Hello</Translation> anything here is structure</Story><Translation Id=\"99\">x</Translation></Root>";
        let r = s.execute("file.importXml", &json!({"text": xml})).unwrap();
        assert_eq!((r["placed"].as_u64(), r["unmatched"].as_u64()), (Some(1), Some(1)), "{r}");
        assert_eq!(r["unmatchedIds"], json!(["99"]));
        let d = &s.doc().unwrap().doc;
        let st = d.story(sid).unwrap();
        assert_eq!(st.text, "Hello Welt", "only the element's text changes");
        // The new text stays bold and in the element; the rest is untouched.
        let f = st.format_after(1);
        assert_eq!((f.over.font_style.as_deref(), f.over.xml_element), (Some("Bold"), Some(2)));
        assert_eq!(st.format_after(6).over.xml_element, None);
        // Exported again, the XML has the new text; one Undo takes the import back.
        assert!(export(&mut s).contains(">Hello</Translation> Welt<"));
        s.execute("edit.undo", &json!({})).unwrap();
        assert_eq!(s.doc().unwrap().doc.story(sid).unwrap().text, "Hallo Welt");
    }

    #[test]
    fn unchanged_text_changes_nothing_and_paragraphs_come_back() {
        let (mut s, sid) = tagged();
        let before = s.doc().unwrap().doc.story(sid).unwrap().clone();
        s.execute("file.importXml", &json!({"text": "<Root><Translation Id=\"7\">Hallo</Translation></Root>"})).unwrap();
        assert_eq!(s.doc().unwrap().doc.story(sid).unwrap().chars, before.chars, "runs untouched");
        // U+2029 (InDesign's paragraph end) becomes a paragraph break.
        s.execute("file.importXml", &json!({"text": "<Root><Translation Id=\"7\">Ha\u{2029}llo</Translation></Root>"})).unwrap();
        assert_eq!(s.doc().unwrap().doc.story(sid).unwrap().text, "Ha\nllo Welt");
    }

    /// A story element with an Id takes all its text, its nested elements' included: an unchanged
    /// export changes nothing, and new texts for both arrive in both.
    #[test]
    fn a_story_element_keeps_the_text_of_its_nested_elements() {
        let (mut s, sid) = tagged();
        s.edit(|d, _| {
            if let Some(e) = d.xml.elements.get_mut(&1) {
                e.attrs = vec![("Id".into(), "S".into())];
            }
            Ok(())
        })
        .unwrap();
        let before = s.doc().unwrap().doc.story(sid).unwrap().clone();
        let xml = export(&mut s);
        s.execute("file.importXml", &json!({"text": xml})).unwrap();
        assert_eq!(s.doc().unwrap().doc.story(sid).unwrap().chars, before.chars, "an unchanged export changes nothing");
        let xml = "<Root><Story Id=\"S\"><Translation Id=\"7\">Hello</Translation> World</Story></Root>";
        let r = s.execute("file.importXml", &json!({"text": xml})).unwrap();
        assert_eq!(r["placed"], json!(2), "{r}");
        let st = s.doc().unwrap().doc.story(sid).unwrap().clone();
        assert_eq!(st.text, "Hello World");
        let f = st.format_after(1);
        assert_eq!((f.over.font_style.as_deref(), f.over.xml_element), (Some("Bold"), Some(2)), "the nested element keeps its text and format");
    }

    /// Emptying an inline element would lose its place in the text: it is reported, and stays fillable.
    #[test]
    fn an_inline_element_is_not_emptied_and_stays_fillable() {
        let (mut s, sid) = tagged();
        let r = s.execute("file.importXml", &json!({"text": "<Root><Translation Id=\"7\"></Translation></Root>"})).unwrap();
        assert_eq!((r["placed"].as_u64(), r["failed"][0]["id"].as_str()), (Some(0), Some("7")), "{r}");
        assert_eq!(s.doc().unwrap().doc.story(sid).unwrap().text, "Hallo Welt");
        s.execute("file.importXml", &json!({"text": "<Root><Translation Id=\"7\">Hello</Translation></Root>"})).unwrap();
        assert_eq!(s.doc().unwrap().doc.story(sid).unwrap().text, "Hello Welt");
    }

    /// XML without Ids still fills tagged frames in a document with InDesign's structure.
    #[test]
    fn xml_without_ids_still_fills_tagged_frames() {
        let (mut s, sid) = tagged();
        let frame = s.doc().unwrap().doc.all_items()[0].0;
        s.execute("xml.tag", &json!({"tag": "headline", "ids": [frame]})).unwrap();
        let r = s.execute("file.importXml", &json!({"text": "<Root><headline>New</headline></Root>"})).unwrap();
        assert_eq!(r["placed"], json!(1), "{r}");
        assert_eq!(s.doc().unwrap().doc.story(sid).unwrap().text, "New");
    }

    /// A tagged story with a table after its text; `tag` makes the table and its first cell elements
    /// (the cell with Id "C").
    fn with_table(tag: bool) -> (Session, StoryId, u64) {
        let (mut s, sid) = tagged();
        s.execute("text.select", &json!({"story": sid.0, "anchor": 10, "focus": 10})).unwrap();
        let table = s.execute("table.insert", &json!({"rows": 2, "cols": 2})).unwrap()["table"].as_u64().unwrap();
        s.execute("text.insert", &json!({"text": "A"})).unwrap();
        s.execute("table.nextCell", &json!({})).unwrap();
        s.execute("text.insert", &json!({"text": "B"})).unwrap();
        if tag {
            s.edit(|d, _| {
                let el = |tag: &str, attrs: Vec<(String, String)>, parent: u64, content| XmlElement {
                    tag: tag.into(),
                    attrs,
                    parent,
                    content,
                    children: Vec::new(),
                };
                d.xml
                    .elements
                    .insert(3, el("Table", Vec::new(), 1, XmlContent::Table { table, added_break_before: false, added_break_after: false }));
                d.xml.elements.insert(4, el("Cell", vec![("Id".into(), "C".into())], 3, XmlContent::Cell { table, cell: 0 }));
                Ok(())
            })
            .unwrap();
        }
        (s, sid, table)
    }

    /// A cell element with an Id takes its text.
    #[test]
    fn import_fills_a_cell_by_its_id() {
        let (mut s, sid, table) = with_table(true);
        let xml = "<Root><Story><Table aid:table=\"table\"><Cell Id=\"C\" aid:table=\"cell\">Zelle</Cell></Table></Story></Root>";
        let r = s.execute("file.importXml", &json!({"text": xml})).unwrap();
        assert_eq!((r["placed"].as_u64(), r["unmatched"].as_u64()), (Some(1), Some(0)), "{r}");
        let st = s.doc().unwrap().doc.story(sid).unwrap().clone();
        assert_eq!(st.tables[&table].cells[0].text.text, "Zelle");
        assert_eq!(st.tables[&table].cells[1].text.text, "B", "the other cell is untouched");
    }

    /// An untagged table in tagged text is CALS; a merged cell names the columns it spans.
    #[test]
    fn cals_entries_keep_their_spans() {
        let (mut s, sid, table) = with_table(false);
        s.execute("table.merge", &json!({"story": sid.0, "table": table, "row": 0, "cols": [0, 1]})).unwrap();
        let xml = export(&mut s);
        assert!(xml.contains("namest=\"c1\" nameend=\"c2\""), "{xml}");
        // and one spanning both rows says how many rows below it it takes
        s.execute("edit.undo", &json!({})).unwrap();
        s.execute("table.merge", &json!({"story": sid.0, "table": table, "rows": [0, 1], "col": 0})).unwrap();
        let xml = export(&mut s);
        assert!(xml.contains("morerows=\"1\""), "{xml}");
    }

    /// The tagged fixture whose story element has the Id "S".
    fn story_with_id() -> (Session, StoryId) {
        let (mut s, sid) = tagged();
        s.edit(|d, _| {
            if let Some(e) = d.xml.elements.get_mut(&1) {
                e.attrs = vec![("Id".into(), "S".into())];
            }
            Ok(())
        })
        .unwrap();
        (s, sid)
    }

    /// A story's new text and its element's both arrive, each in its own place: the element keeps
    /// its text, format and tag.
    #[test]
    fn a_story_and_its_element_both_changing_keep_apart() {
        let (mut s, sid) = story_with_id();
        let xml = "<Root><Story Id=\"S\">A <Translation Id=\"7\">Hello</Translation> Y</Story></Root>";
        s.execute("file.importXml", &json!({"text": xml})).unwrap();
        let st = s.doc().unwrap().doc.story(sid).unwrap().clone();
        assert_eq!(st.text, "A Hello Y");
        let f = st.format_after(3);
        assert_eq!((f.over.font_style.as_deref(), f.over.xml_element), (Some("Bold"), Some(2)), "the element keeps format and tag");
        assert_eq!((st.format_after(1).over.xml_element, st.format_after(8).over.xml_element), (None, None), "the story's text is its own");
    }

    /// An element emptied inside a story that changes too: the element stays (and is reported), the
    /// story's own text changes around it, and the element can be filled later.
    #[test]
    fn an_emptied_element_stays_when_its_story_changes() {
        let (mut s, sid) = story_with_id();
        let xml = "<Root><Story Id=\"S\"><Translation Id=\"7\"></Translation> World</Story></Root>";
        let r = s.execute("file.importXml", &json!({"text": xml})).unwrap();
        assert_eq!(r["failed"][0]["id"], json!("7"), "{r}");
        assert_eq!(s.doc().unwrap().doc.story(sid).unwrap().text, "Hallo World");
        s.execute("file.importXml", &json!({"text": "<Root><Translation Id=\"7\">Hello</Translation></Root>"})).unwrap();
        assert_eq!(s.doc().unwrap().doc.story(sid).unwrap().text, "Hello World");
    }

    #[test]
    fn a_common_end_inside_a_cross_reference_moves_to_its_edge() {
        // "x" then a reference showing "12": both its chars stand for the one marker at bytes 1..4.
        let at = [0..1, 1..4, 1..4];
        // Old "x12" against new "x13": the common start "x1" stops inside the reference.
        assert_eq!(super::to_marker_edges(2, 0, &at), (1, 0));
        // Old "x12" against new "x02": the common end "2" stops inside it too.
        assert_eq!(super::to_marker_edges(1, 1, &at), (1, 0));
        assert_eq!(super::to_marker_edges(1, 0, &at), (1, 0), "an edge already at the marker stays");
    }

    /// A story element with an Id and an untagged (CALS) table: the table's text isn't the story's,
    /// so an unchanged export changes neither.
    #[test]
    fn a_story_id_leaves_its_cals_table_alone() {
        let (mut s, sid, table) = with_table(false);
        s.edit(|d, _| {
            if let Some(e) = d.xml.elements.get_mut(&1) {
                e.attrs = vec![("Id".into(), "S".into())];
            }
            Ok(())
        })
        .unwrap();
        let before = s.doc().unwrap().doc.story(sid).unwrap().clone();
        let xml = export(&mut s);
        assert!(xml.contains("<table "), "the table is CALS: {xml}");
        s.execute("file.importXml", &json!({"text": xml})).unwrap();
        let st = s.doc().unwrap().doc.story(sid).unwrap().clone();
        assert_eq!((&st.text, &st.chars), (&before.text, &before.chars));
        assert_eq!(st.tables[&table].cells[1].text.text, "B");
    }

    /// A table element's Id is reported: its text belongs to its cells.
    #[test]
    fn a_table_id_is_reported() {
        let (mut s, _, _) = with_table(true);
        s.edit(|d, _| {
            if let Some(e) = d.xml.elements.get_mut(&3) {
                e.attrs = vec![("Id".into(), "T".into())];
            }
            Ok(())
        })
        .unwrap();
        let r = s.execute("file.importXml", &json!({"text": "<Root><Story><Table Id=\"T\" aid:table=\"table\">x</Table></Story></Root>"})).unwrap();
        assert_eq!(r["failed"][0]["id"], json!("T"), "{r}");
    }

    /// An inline element with one inside it: re-importing the export changes nothing, and new texts
    /// for both arrive in both.
    #[test]
    fn an_inline_element_keeps_the_text_of_one_inside_it() {
        let (mut s, sid) = tagged();
        // "Hallo Welt": element 3 (Id P) over "o We", element 4 (Id C) inside it over " ".
        s.edit(|d, _| {
            let el = |tag: &str, id: &str, parent: u64| XmlElement {
                tag: tag.into(),
                attrs: vec![("Id".into(), id.into())],
                parent,
                content: XmlContent::Text,
                children: Vec::new(),
            };
            d.xml.elements.insert(3, el("Outer", "P", 1));
            d.xml.elements.insert(4, el("Inner", "C", 3));
            if let Some(st) = d.story_mut(sid) {
                st.format_chars(6..8, |f| f.over.xml_element = Some(3));
                st.format_chars(5..6, |f| f.over.xml_element = Some(4));
                st.format_chars(4..5, |f| f.over.xml_element = Some(3));
            }
            Ok(())
        })
        .unwrap();
        // element 2 (Id 7) keeps "Hall"
        let before = s.doc().unwrap().doc.story(sid).unwrap().clone();
        let xml = export(&mut s);
        s.execute("file.importXml", &json!({"text": xml})).unwrap();
        assert_eq!(s.doc().unwrap().doc.story(sid).unwrap().chars, before.chars, "an unchanged export changes nothing");
        let r = s.execute("file.importXml", &json!({"text": "<Root><Outer Id=\"P\">o<Inner Id=\"C\">-</Inner>We</Outer></Root>"})).unwrap();
        assert_eq!(r["failed"], json!([]), "{r}");
        let st = s.doc().unwrap().doc.story(sid).unwrap().clone();
        assert_eq!(st.text, "Hallo-Welt");
        assert_eq!(st.format_after(5).over.xml_element, Some(4), "the inner element keeps its new text");
    }

    /// A marker the XML doesn't show (an anchor) stays when the text around it changes.
    #[test]
    fn a_hidden_marker_stays_when_the_text_around_it_changes() {
        let (mut s, sid) = tagged();
        s.edit(|d, _| {
            if let Some(st) = d.story_mut(sid) {
                st.insert_anchor(2, "here");
            }
            Ok(())
        })
        .unwrap();
        s.execute("file.importXml", &json!({"text": "<Root><Translation Id=\"7\">Hello</Translation></Root>"})).unwrap();
        let st = s.doc().unwrap().doc.story(sid).unwrap().clone();
        assert_eq!(st.anchors.len(), 1, "the anchor is kept");
        assert_eq!(st.text.replace(designcraft_doc::ANCHOR_MARK, ""), "Hello Welt");
    }

    /// Ids that match no element leave the XML to the tagged frames.
    #[test]
    fn ids_matching_nothing_leave_xml_to_tagged_frames() {
        let (mut s, sid) = tagged();
        let frame = s.doc().unwrap().doc.all_items()[0].0;
        s.execute("xml.tag", &json!({"tag": "headline", "ids": [frame]})).unwrap();
        let r = s.execute("file.importXml", &json!({"text": "<Root><headline Id=\"x\">New</headline></Root>"})).unwrap();
        assert_eq!(r["placed"], json!(1), "{r}");
        assert_eq!(s.doc().unwrap().doc.story(sid).unwrap().text, "New");
    }

    /// Images by tag in XML that fills elements by Id still go to tagged frames: one without its
    /// frame is reported.
    #[test]
    fn images_by_tag_follow_an_import_by_id() {
        let (mut s, sid) = tagged();
        let xml = "<Root><Translation Id=\"7\">Hello</Translation><photo href=\"photo.png\"/></Root>";
        let r = s.execute("file.importXml", &json!({"text": xml})).unwrap();
        assert_eq!(s.doc().unwrap().doc.story(sid).unwrap().text, "Hello Welt");
        assert_eq!((r["placed"].as_u64(), r["unmatchedIdCount"].as_u64(), r["unmatchedTags"].clone()), (Some(1), Some(0), json!(["photo"])), "{r}");
        // an image inside an element with an Id is found too
        let (mut s, _) = story_with_id();
        let xml = "<Root><Story Id=\"S\">Hallo Welt<photo href=\"photo.png\"/></Story></Root>";
        let r = s.execute("file.importXml", &json!({"text": xml})).unwrap();
        assert_eq!(r["unmatchedTags"], json!(["photo"]), "{r}");
    }

    /// A story with a table element that has an Id: the table is no child of the story's text, so
    /// re-importing the export fills the story.
    #[test]
    fn a_table_id_doesnt_stop_its_story() {
        let (mut s, sid, _) = with_table(true);
        s.edit(|d, _| {
            for (k, id) in [(1, "S"), (3, "T")] {
                if let Some(e) = d.xml.elements.get_mut(&k) {
                    e.attrs = vec![("Id".into(), id.into())];
                }
            }
            Ok(())
        })
        .unwrap();
        let before = s.doc().unwrap().doc.story(sid).unwrap().clone();
        let xml = export(&mut s);
        let r = s.execute("file.importXml", &json!({"text": xml})).unwrap();
        assert!(r["failed"].as_array().unwrap().iter().all(|f| f["id"] != json!("S")), "{r}");
        assert_eq!(s.doc().unwrap().doc.story(sid).unwrap().text, before.text);
    }

    /// Three inline elements nested ("Hallo Welt": P over "o" and "t", Q in it over " " and "l", C
    /// in that over "We"), all changing: each takes its own text, the innermost first.
    fn nested() -> (Session, StoryId) {
        let (mut s, sid) = tagged();
        s.edit(|d, _| {
            let el = |tag: &str, id: &str, parent: u64| XmlElement {
                tag: tag.into(),
                attrs: vec![("Id".into(), id.into())],
                parent,
                content: XmlContent::Text,
                children: Vec::new(),
            };
            d.xml.elements.insert(3, el("Outer", "P", 1));
            d.xml.elements.insert(4, el("Mid", "Q", 3));
            d.xml.elements.insert(5, el("Inner", "C", 4));
            if let Some(st) = d.story_mut(sid) {
                st.format_chars(4..5, |f| f.over.xml_element = Some(3));
                st.format_chars(9..10, |f| f.over.xml_element = Some(3));
                st.format_chars(5..6, |f| f.over.xml_element = Some(4));
                st.format_chars(8..9, |f| f.over.xml_element = Some(4));
                st.format_chars(6..8, |f| f.over.xml_element = Some(5));
            }
            Ok(())
        })
        .unwrap();
        (s, sid)
    }

    #[test]
    fn nested_parents_each_take_their_text() {
        let (mut s, sid) = nested();
        let xml = "<Root><Outer Id=\"P\">o<Mid Id=\"Q\">__<Inner Id=\"C\">We</Inner>LL</Mid>T</Outer></Root>";
        let r = s.execute("file.importXml", &json!({"text": xml})).unwrap();
        assert_eq!(r["failed"], json!([]), "{r}");
        let st = s.doc().unwrap().doc.story(sid).unwrap().clone();
        assert_eq!(st.text, "Hallo__WeLLT");
        let at = |i: usize| st.format_after(i).over.xml_element;
        assert_eq!((at(4), at(5), at(7), at(9), at(11)), (Some(3), Some(4), Some(5), Some(4), Some(3)));
    }

    /// A parent whose own text was emptied (only its child's text left) still takes new text.
    #[test]
    fn an_emptied_parent_takes_text_again() {
        let (mut s, sid) = nested();
        s.execute("file.importXml", &json!({"text": "<Root><Mid Id=\"Q\"><Inner Id=\"C\">We</Inner></Mid></Root>"})).unwrap();
        assert_eq!(s.doc().unwrap().doc.story(sid).unwrap().text, "HalloWet");
        let r = s.execute("file.importXml", &json!({"text": "<Root><Mid Id=\"Q\">-<Inner Id=\"C\">We</Inner>-</Mid></Root>"})).unwrap();
        assert_eq!(r["failed"], json!([]), "{r}");
        assert_eq!(s.doc().unwrap().doc.story(sid).unwrap().text, "Hallo-We-t");
    }

    /// "Hallo Welt" with element 3 (Id P) over `parent` and its element 4 (Id C) over "Welt".
    fn parent_and_child(parent: Option<std::ops::Range<usize>>) -> (Session, StoryId) {
        let (mut s, sid) = tagged();
        s.edit(|d, _| {
            let el = |tag: &str, id: &str, parent: u64| XmlElement {
                tag: tag.into(),
                attrs: vec![("Id".into(), id.into())],
                parent,
                content: XmlContent::Text,
                children: Vec::new(),
            };
            d.xml.elements.insert(3, el("Outer", "P", 1));
            d.xml.elements.insert(4, el("Inner", "C", 3));
            if let Some(st) = d.story_mut(sid) {
                if let Some(r) = parent.clone() {
                    st.format_chars(r, |f| f.over.xml_element = Some(3));
                }
                st.format_chars(6..10, |f| f.over.xml_element = Some(4));
            }
            Ok(())
        })
        .unwrap();
        (s, sid)
    }

    /// A parent with no text of its own, only its child's, takes text around it.
    #[test]
    fn a_parent_with_only_its_childs_text_takes_text() {
        let (mut s, sid) = parent_and_child(None);
        let r = s.execute("file.importXml", &json!({"text": "<Root><Outer Id=\"P\">[<Inner Id=\"C\">Welt</Inner>]</Outer></Root>"})).unwrap();
        assert_eq!(r["failed"], json!([]), "{r}");
        let st = s.doc().unwrap().doc.story(sid).unwrap().clone();
        assert_eq!(st.text, "Hallo [Welt]");
        let at = |i: usize| st.format_after(i).over.xml_element;
        assert_eq!((at(6), at(7), at(11)), (Some(3), Some(4), Some(3)));
    }

    /// A child at the end of its parent's text keeps its place; the parent's new text goes around it.
    #[test]
    fn a_child_at_the_end_of_its_parent_stays() {
        let (mut s, sid) = parent_and_child(Some(5..6));
        let before = s.doc().unwrap().doc.story(sid).unwrap().clone();
        let xml = export(&mut s);
        s.execute("file.importXml", &json!({"text": xml})).unwrap();
        assert_eq!(s.doc().unwrap().doc.story(sid).unwrap().chars, before.chars, "an unchanged export changes nothing");
        let r = s.execute("file.importXml", &json!({"text": "<Root><Outer Id=\"P\">, <Inner Id=\"C\">Welt</Inner>!</Outer></Root>"})).unwrap();
        assert_eq!(r["failed"], json!([]), "{r}");
        let st = s.doc().unwrap().doc.story(sid).unwrap().clone();
        assert_eq!(st.text, "Hallo, Welt!");
        let at = |i: usize| st.format_after(i).over.xml_element;
        assert_eq!((at(5), at(7), at(11)), (Some(3), Some(4), Some(3)));
    }

    /// Children in another order than the document's: the parent is reported and left as it was.
    #[test]
    fn children_out_of_order_leave_their_parent() {
        let (mut s, sid) = story_with_id();
        s.edit(|d, _| {
            d.xml.elements.insert(
                3,
                XmlElement {
                    tag: "Translation".into(),
                    attrs: vec![("Id".into(), "8".into())],
                    parent: 1,
                    content: XmlContent::Text,
                    children: Vec::new(),
                },
            );
            if let Some(st) = d.story_mut(sid) {
                st.format_chars(6..10, |f| f.over.xml_element = Some(3));
            }
            Ok(())
        })
        .unwrap();
        let xml = "<Root><Story Id=\"S\"><Translation Id=\"8\">Welt</Translation> <Translation Id=\"7\">Hallo</Translation>!</Story></Root>";
        let r = s.execute("file.importXml", &json!({"text": xml})).unwrap();
        assert_eq!(r["failed"][0]["id"], json!("S"), "{r}");
        assert_eq!(s.doc().unwrap().doc.story(sid).unwrap().text, "Hallo Welt");
    }

    /// A child the document doesn't know: the parent is left as it was, its known (emptied) child
    /// stays and can be filled later.
    #[test]
    fn an_unknown_child_leaves_its_parent() {
        let (mut s, sid) = story_with_id();
        let xml = "<Root><Story Id=\"S\"><Translation Id=\"7\"></Translation><X Id=\"99\">Extra</X> World</Story></Root>";
        let r = s.execute("file.importXml", &json!({"text": xml})).unwrap();
        let failed: Vec<&serde_json::Value> = r["failed"].as_array().unwrap().iter().map(|f| &f["id"]).collect();
        assert_eq!(failed, [&json!("7"), &json!("S")], "{r}");
        assert_eq!(s.doc().unwrap().doc.story(sid).unwrap().text, "Hallo Welt");
        s.execute("file.importXml", &json!({"text": "<Root><Translation Id=\"7\">Hello</Translation></Root>"})).unwrap();
        assert_eq!(s.doc().unwrap().doc.story(sid).unwrap().text, "Hello Welt");
    }

    /// Only CALS markup (`<table frame="…">`) is a table; an element merely named so is text.
    #[test]
    fn only_cals_markup_counts_as_a_table() {
        let full = |xml: &str| {
            super::texts_by_id(xml).unwrap().parts["S"]
                .iter()
                .filter_map(|p| match p {
                    super::Piece::Text(t) => Some(t.as_str()),
                    super::Piece::Child(_) => None,
                })
                .collect::<String>()
        };
        assert_eq!(full("<Root><Story Id=\"S\">A<table>Hi</table>B</Story></Root>"), "AHiB");
        assert_eq!(full("<Root><Story Id=\"S\">A<table frame=\"all\">Hi</table>B</Story></Root>"), "AHiB", "an inline table with a frame");
        assert_eq!(full("<Root><Story Id=\"S\">A<table frame=\"all\"><b>Hi</b></table>B</Story></Root>"), "AHiB", "its first element isn't a tgroup");
        assert_eq!(
            full(
                "<Root><Story Id=\"S\">A<table frame=\"all\"><tgroup cols=\"1\"><tbody><row><entry>Hi</entry></row></tbody></tgroup></table>B</Story></Root>"
            ),
            "AB"
        );
    }

    /// An element at the start of its parent's text: re-importing the export changes nothing, and
    /// the parent's new text goes beside it.
    #[test]
    fn a_child_at_the_edge_of_its_parent_stays() {
        let (mut s, sid) = tagged();
        // "Hallo Welt": element 3 (Id P) over "o Welt", its element 4 (Id C) over its first char "o".
        s.edit(|d, _| {
            let el = |tag: &str, id: &str, parent: u64| XmlElement {
                tag: tag.into(),
                attrs: vec![("Id".into(), id.into())],
                parent,
                content: XmlContent::Text,
                children: Vec::new(),
            };
            d.xml.elements.insert(3, el("Outer", "P", 1));
            d.xml.elements.insert(4, el("Inner", "C", 3));
            if let Some(st) = d.story_mut(sid) {
                st.format_chars(5..10, |f| f.over.xml_element = Some(3));
                st.format_chars(4..5, |f| f.over.xml_element = Some(4));
            }
            Ok(())
        })
        .unwrap();
        let before = s.doc().unwrap().doc.story(sid).unwrap().clone();
        let xml = export(&mut s);
        s.execute("file.importXml", &json!({"text": xml})).unwrap();
        assert_eq!(s.doc().unwrap().doc.story(sid).unwrap().chars, before.chars, "an unchanged export changes nothing");
        s.execute("file.importXml", &json!({"text": "<Root><Outer Id=\"P\"><Inner Id=\"C\">o</Inner> World</Outer></Root>"})).unwrap();
        let st = s.doc().unwrap().doc.story(sid).unwrap().clone();
        assert_eq!(st.text, "Hallo World");
        assert_eq!(st.format_after(4).over.xml_element, Some(4), "the inner element keeps its place");
    }

    /// An element without content of its own (a page item's) inside a story is exported around its
    /// elements' text, as the IDML export writes it.
    #[test]
    fn an_element_without_content_is_exported_around_its_elements() {
        let (mut s, sid) = tagged();
        s.edit(|d, _| {
            let el = |tag: &str, parent: u64, content| XmlElement { tag: tag.into(), attrs: Vec::new(), parent, content, children: Vec::new() };
            d.xml.elements.insert(3, el("Pic", 1, XmlContent::None));
            d.xml.elements.insert(4, el("Caption", 3, XmlContent::Text));
            if let Some(st) = d.story_mut(sid) {
                st.format_chars(6..10, |f| f.over.xml_element = Some(4));
            }
            Ok(())
        })
        .unwrap();
        assert!(export(&mut s).contains("<Pic><Caption>Welt</Caption></Pic>"), "{}", export(&mut s));
    }
}
