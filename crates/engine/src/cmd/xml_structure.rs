//! The document's XML structure (`XmlSettings::elements`, read from InDesign) as XML: export in
//! the form InDesign's File › Export (XML) writes (the root, story elements holding their text
//! with inline elements, tables as `aid:` tables, untagged ones in tagged text as CALS tables,
//! paragraphs ending in U+2029), and import of the texts of elements matched by their `Id`.

use std::collections::{BTreeMap, HashMap};
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
        let mut chain = Vec::new();
        let mut cur = id;
        while let Some(e) = self.el(cur).filter(|e| e.content == XmlContent::Text) {
            chain.push(cur);
            if chain.len() > 64 {
                break;
            }
            cur = e.parent;
        }
        chain.reverse();
        chain
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

// ---------- import by Id ----------

/// The texts of the elements with an `Id` attribute in `xml`: Id → text, paragraphs separated by
/// `\n` (InDesign writes U+2029). Text inside an element nested in one with an Id belongs to the
/// innermost element with an Id.
fn texts_by_id(xml: &str) -> std::result::Result<BTreeMap<String, String>, String> {
    use quick_xml::events::Event;
    let mut r = quick_xml::Reader::from_str(xml);
    // Open elements: the Id of each one that has one.
    let mut stack: Vec<Option<String>> = Vec::new();
    let mut texts: BTreeMap<String, String> = BTreeMap::new();
    let id_of = |e: &quick_xml::events::BytesStart| -> std::result::Result<Option<String>, String> {
        for a in e.attributes() {
            let a = a.map_err(|e| e.to_string())?;
            if a.key.local_name().as_ref() == b"Id" {
                return Ok(Some(a.normalized_value(quick_xml::XmlVersion::Implicit1_0).map_err(|e| e.to_string())?.to_string()));
            }
        }
        Ok(None)
    };
    loop {
        match r.read_event().map_err(|e| e.to_string())? {
            Event::Start(e) => {
                let id = id_of(&e)?;
                if let Some(i) = &id {
                    texts.entry(i.clone()).or_default();
                }
                stack.push(id);
            }
            Event::Empty(e) => {
                if let Some(i) = id_of(&e)? {
                    texts.entry(i).or_default();
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
                if let Some(Some(id)) = stack.iter().rev().find(|i| i.is_some())
                    && let Some(t) = texts.get_mut(id)
                {
                    t.push_str(&s.replace('\u{2029}', "\n"));
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

/// Puts `new` into `range` of `st`, replacing only the part that differs, so the formatting of
/// what stays stays and changed text takes the format of the first character it replaces.
/// Cross-references keep their place when their text (as export writes it) is still in `new`;
/// other markers in the replaced part go with it.
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
    // The story bytes the change covers, widened to whole markers.
    let a = at.get(pre).map_or_else(|| at.last().map_or(range.start, |r| r.end), |r| r.start);
    let b = if suf == 0 { at.last().map_or(range.end, |r| r.end) } else { at[view.len() - suf].start };
    let (a, b) = (a.min(b), a.max(b));
    let repl: String = new[pre..new.len() - suf].iter().collect();
    // Cross-references in the changed part, kept where their text is found in the replacement.
    let mut keep: Vec<(usize, String)> = Vec::new();
    for (i, c) in st.text[a..b].char_indices() {
        if c == designcraft_doc::XREF_MARK
            && let Some(x) = st.xref_at(a + i)
        {
            keep.push((a + i, designcraft_compose::xref::xref_text(d, x, xrefs)));
        }
    }
    // The replacement split around the kept references' texts, in order.
    let mut pieces: Vec<String> = Vec::new();
    let mut marks: Vec<usize> = Vec::new();
    let mut rest = repl.as_str();
    for (pos, text) in &keep {
        if let Some(k) = rest.find(text.as_str()).filter(|_| !text.is_empty()) {
            pieces.push(rest[..k].to_string());
            marks.push(*pos);
            rest = &rest[k + text.len()..];
        }
    }
    pieces.push(rest.to_string());
    // Replace the stretches between the kept marks, last first so the earlier ones stay put.
    let mut bounds = vec![a];
    for m in &marks {
        bounds.push(*m);
        bounds.push(m + designcraft_doc::XREF_MARK.len_utf8());
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
    let mut want: HashMap<u64, String> = HashMap::new();
    let mut unmatched: Vec<String> = Vec::new();
    for (k, t) in &texts {
        match by_id.get(k.as_str()) {
            Some(e) => {
                want.insert(*e, t.clone());
            }
            None => unmatched.push(k.clone()),
        }
    }
    let xrefs = s.cache.xref_index(d);
    let mut placed = 0usize;
    s.edit(|d, _| {
        let snapshot = d.clone();
        // Elements standing for a whole story (a story-level element with an Id).
        let whole: Vec<(StoryId, String)> = want
            .iter()
            .filter_map(|(e, t)| match snapshot.xml.elements.get(e).map(|el| el.content) {
                Some(XmlContent::Story { story }) => Some((story, t.clone())),
                _ => None,
            })
            .collect();
        let ids: Vec<StoryId> = d.stories.keys().copied().collect();
        for sid in ids {
            let holds = d.story(sid).is_some_and(|st| holds_any(st, &want));
            if holds && let Some(st) = d.story_mut(sid) {
                placed += fill(st, &want, &snapshot, Some(&xrefs));
            }
        }
        for (sid, t) in whole {
            if let Some(st) = d.story_mut(sid) {
                let end = st.text.len();
                replace_text(st, 0..end, &t, &snapshot, Some(&xrefs));
                placed += 1;
            }
        }
        Ok(())
    })?;
    Ok(json!({
        "placed": placed,
        "unmatched": unmatched.len(),
        "unmatchedIds": unmatched.iter().take(50).collect::<Vec<_>>(),
        "failed": [],
    }))
}

/// The wanted elements' texts put into `st` and its table cells; returns how many.
fn fill(st: &mut Story, want: &HashMap<u64, String>, d: &Document, xrefs: Option<&XrefIndex>) -> usize {
    let mut n = 0;
    let mut todo: Vec<(u64, Range<usize>)> = element_ranges(st).into_iter().filter(|(id, _)| want.contains_key(id)).collect();
    todo.sort_by_key(|(_, r)| std::cmp::Reverse(r.start));
    for (id, r) in todo {
        if let Some(t) = want.get(&id) {
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
                n += fill(&mut cell.text, want, d, xrefs);
            }
        }
    }
    if n > 0 {
        st.rev = st.rev.wrapping_add(1);
    }
    n
}

/// Whether `st` or a table in it holds text of a wanted element.
fn holds_any(st: &Story, want: &HashMap<u64, String>) -> bool {
    st.runs().any(|(_, f)| f.over.xml_element.is_some_and(|id| want.contains_key(&id)))
        || st.tables.values().any(|t| t.cells.iter().any(|c| holds_any(&c.text, want)))
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
}
