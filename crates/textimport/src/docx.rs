//! Word (.docx, Office Open XML) → story.

use std::collections::HashMap;
use std::io::Read;

use designcraft_doc::story::{BASIC_PARAGRAPH, NO_CHAR_STYLE};
use designcraft_doc::{CharAttrs, CharFormat, ListCounter, ListType, ParaAttrs, ParaFormat, ParaProps, Story, StoryId, Table};
use quick_xml::events::Event;

use crate::docx_numbering::{ListPara, Numbering};
use crate::{ImportError, Imported, ImportedStyle};

/// A minimal element tree (local names, attributes by local name).
#[derive(Debug, Default, Clone)]
pub(crate) struct El {
    pub(crate) name: String,
    attrs: Vec<(String, String)>,
    kids: Vec<Node>,
}

#[derive(Debug, Clone)]
enum Node {
    El(El),
    Text(String),
}

impl El {
    pub(crate) fn attr(&self, k: &str) -> Option<&str> {
        self.attrs.iter().find(|(n, _)| n == k).map(|(_, v)| v.as_str())
    }
    pub(crate) fn els(&self) -> impl Iterator<Item = &El> {
        self.kids.iter().filter_map(|n| if let Node::El(e) = n { Some(e) } else { None })
    }
    pub(crate) fn child(&self, name: &str) -> Option<&El> {
        self.els().find(|e| e.name == name)
    }
    pub(crate) fn text(&self) -> String {
        self.kids
            .iter()
            .map(|n| match n {
                Node::Text(t) => t.clone(),
                Node::El(e) => e.text(),
            })
            .collect()
    }
    /// `<w:x w:val="…"/>` → the value; a bare `<w:b/>` counts as "on".
    pub(crate) fn val(&self, name: &str) -> Option<String> {
        self.child(name).map(|e| e.attr("val").unwrap_or("true").to_string())
    }
    /// A toggle property (`ST_OnOff`: a boolean, or the words `on` and `off`). None when unset.
    pub(crate) fn on(&self, name: &str) -> Option<bool> {
        self.val(name).map(|v| !matches!(v.as_str(), "0" | "false" | "none" | "off"))
    }
}

fn local(n: &[u8]) -> String {
    let s = String::from_utf8_lossy(n);
    s.rsplit(':').next().unwrap_or(&s).to_string()
}

/// How deep elements may nest. Word's own XML stays well under a hundred levels; the tree is read
/// recursively afterwards (`El::text`, the block walk) and dropped recursively, so a document
/// nested without bound must be refused here, not run into the stack (#712).
pub(crate) const MAX_DEPTH: usize = 256;

pub(crate) fn parse(xml: &str) -> Result<El, ImportError> {
    let mut r = quick_xml::Reader::from_str(xml);
    let mut stack: Vec<El> = vec![El::default()];
    loop {
        match r.read_event().map_err(|e| ImportError::Corrupt(e.to_string()))? {
            Event::Start(e) => {
                if stack.len() > MAX_DEPTH {
                    return Err(ImportError::Corrupt(format!("XML elements nested deeper than {MAX_DEPTH} levels")));
                }
                let el = El {
                    name: local(e.name().as_ref()),
                    attrs: e
                        .attributes()
                        .flatten()
                        .map(|a| {
                            (local(a.key.as_ref()), a.normalized_value(quick_xml::XmlVersion::Implicit1_0).map(|v| v.to_string()).unwrap_or_default())
                        })
                        .collect(),
                    kids: vec![],
                };
                stack.push(el);
            }
            Event::Empty(e) => {
                let el = El {
                    name: local(e.name().as_ref()),
                    attrs: e
                        .attributes()
                        .flatten()
                        .map(|a| {
                            (local(a.key.as_ref()), a.normalized_value(quick_xml::XmlVersion::Implicit1_0).map(|v| v.to_string()).unwrap_or_default())
                        })
                        .collect(),
                    kids: vec![],
                };
                if let Some(p) = stack.last_mut() {
                    p.kids.push(Node::El(el));
                }
            }
            Event::End(_) => {
                let el = stack.pop().unwrap_or_default();
                match stack.last_mut() {
                    Some(p) => p.kids.push(Node::El(el)),
                    None => return Ok(el),
                }
            }
            Event::Text(t) => {
                let s = t.decode().map(|s| s.to_string()).unwrap_or_default();
                let s = quick_xml::escape::unescape(&s).map(|s| s.to_string()).unwrap_or(s);
                if let Some(p) = stack.last_mut() {
                    p.kids.push(Node::Text(s));
                }
            }
            Event::CData(t) => {
                let s = t.decode().map(|s| s.to_string()).unwrap_or_default();
                if let Some(p) = stack.last_mut() {
                    p.kids.push(Node::Text(s));
                }
            }
            Event::GeneralRef(e) => {
                let name = String::from_utf8_lossy(&e).to_string();
                let s = match name.as_str() {
                    "amp" => "&".to_string(),
                    "lt" => "<".into(),
                    "gt" => ">".into(),
                    "quot" => "\"".into(),
                    "apos" => "'".into(),
                    n if n.starts_with("#x") => u32::from_str_radix(&n[2..], 16).ok().and_then(char::from_u32).map(String::from).unwrap_or_default(),
                    n if n.starts_with('#') => n[1..].parse::<u32>().ok().and_then(char::from_u32).map(String::from).unwrap_or_default(),
                    _ => String::new(),
                };
                if let Some(p) = stack.last_mut() {
                    p.kids.push(Node::Text(s));
                }
            }
            Event::Eof => break,
            _ => {}
        }
    }
    let mut root = stack.pop().unwrap_or_default();
    Ok(root.kids.drain(..).find_map(|n| if let Node::El(e) = n { Some(e) } else { None }).unwrap_or_default())
}

/// Largest decompressed zip part read (a small file can inflate to gigabytes).
pub(crate) const PART_LIMIT: u64 = 64 * 1024 * 1024;

/// A zip part as text. `Ok(None)` when the part is missing or isn't text; an error when it
/// decompresses to more than [`PART_LIMIT`].
pub(crate) fn part(zip: &mut zip::ZipArchive<std::io::Cursor<&[u8]>>, name: &str) -> Result<Option<String>, ImportError> {
    part_capped(zip, name, PART_LIMIT)
}

pub(crate) fn part_capped(zip: &mut zip::ZipArchive<std::io::Cursor<&[u8]>>, name: &str, limit: u64) -> Result<Option<String>, ImportError> {
    let Ok(f) = zip.by_name(name) else { return Ok(None) };
    let mut s = String::new();
    // Read one byte past the limit to tell "exactly at the limit" from "over it".
    if f.take(limit.saturating_add(1)).read_to_string(&mut s).is_err() {
        return Ok(None);
    }
    if s.len() as u64 > limit {
        return Err(ImportError::Corrupt(format!("{name} is larger than {} MiB uncompressed", limit / (1024 * 1024))));
    }
    Ok(Some(s))
}

/// Character attributes from `<w:rPr>`.
fn run_attrs(rpr: Option<&El>) -> CharAttrs {
    let mut a = CharAttrs::default();
    let Some(r) = rpr else { return a };
    let bold = r.on("b");
    let italic = r.on("i");
    a.font_style = match (bold, italic) {
        (Some(true), Some(true)) => Some("Bold Italic".into()),
        (Some(true), _) => Some("Bold".into()),
        (_, Some(true)) => Some("Italic".into()),
        (Some(false), Some(false)) | (Some(false), None) | (None, Some(false)) => Some("Regular".into()),
        _ => None,
    };
    if let Some(u) = r.val("u") {
        a.underline = Some(!matches!(u.as_str(), "none" | "0" | "false" | "off"));
    }
    if let Some(s) = r.on("strike") {
        a.strikethrough = Some(s);
    }
    // Half-points; a size the engine cannot mean is dropped (#1593).
    if let Some(pt) = r.val("sz").and_then(|v| v.parse::<f64>().ok()).map(|sz| sz / 2.0).and_then(designcraft_doc::attrs::imported_type_size) {
        a.size = Some(pt);
    }
    if let Some(f) = r.child("rFonts").and_then(|f| f.attr("ascii").or(f.attr("hAnsi"))) {
        a.font_family = Some(f.to_string());
    }
    if r.on("caps") == Some(true) {
        a.capitalization = Some(designcraft_doc::Capitalization::AllCaps);
    }
    if let Some(v) = r.val("vertAlign") {
        a.position = Some(match v.as_str() {
            "superscript" => designcraft_doc::Position::Superscript,
            "subscript" => designcraft_doc::Position::Subscript,
            _ => designcraft_doc::Position::Normal,
        });
    }
    a
}

/// Paragraph attributes from `<w:pPr>` (spacing in twips = 1/20 pt, indents likewise).
fn para_attrs(ppr: Option<&El>) -> ParaAttrs {
    let mut a = ParaAttrs::default();
    let Some(p) = ppr else { return a };
    // Twips, clamped: a file can hold 1e300, which became a 5e298 pt indent (#1593).
    let tw = |v: Option<&str>| v.and_then(|v| v.parse::<f64>().ok()).map(|v| v / 20.0).and_then(designcraft_doc::attrs::imported_measure);
    if let Some(sp) = p.child("spacing") {
        a.space_before = tw(sp.attr("before"));
        a.space_after = tw(sp.attr("after"));
    }
    if let Some(ind) = p.child("ind") {
        a.left_indent = tw(ind.attr("left").or(ind.attr("start")));
        a.right_indent = tw(ind.attr("right").or(ind.attr("end")));
        a.first_line_indent = tw(ind.attr("firstLine")).or(tw(ind.attr("hanging")).map(|h| -h));
    }
    if let Some(j) = p.val("jc") {
        a.align = Some(match j.as_str() {
            "center" => designcraft_doc::Align::Center,
            "right" | "end" => designcraft_doc::Align::Right,
            "both" | "distribute" => designcraft_doc::Align::LeftJustified,
            _ => designcraft_doc::Align::Left,
        });
    }
    a
}

struct Ctx {
    para_names: HashMap<String, String>,
    char_names: HashMap<String, String>,
    footnotes: HashMap<String, El>,
    /// Footnote ids already imported, and how much note text has been built. A reference clones
    /// its note and walks it again, so one 1 MB footnote referenced 100k times meant ~100 GB
    /// (#1584). Word gives each note its own id and references it once; a second reference to the
    /// same id gets the mark without the text.
    notes_done: std::collections::HashSet<String>,
    note_bytes: usize,
    warnings: Vec<String>,
    /// What has already been said, so saying it again is a lookup rather than a scan of
    /// everything said so far. A warning that names a footnote id is one per id, and the
    /// document chooses how many ids it has (noted in review).
    warned: std::collections::HashSet<String>,
    next_table: u64,
    numbering: Numbering,
    /// A paragraph style's list (`w:numPr` numId and ilvl, through `w:basedOn`), by style id.
    style_lists: HashMap<String, (Option<String>, Option<String>)>,
    /// The number the model's own list counter is at (it restarts after a paragraph that isn't
    /// in a list), to tell where a numbered paragraph needs an explicit Start At.
    /// The model's unnamed-list numbering as composition will count it.
    model_counter: ListCounter,
    /// The style of paragraphs without `w:pStyle` (Word's default paragraph style, usually Normal).
    default_para: String,
}

/// A note's tables may hold tables of their own. The document model stops nesting at 64, so
/// counting no deeper than that reaches every cell a note can actually have.
const MAX_NOTE_DEPTH: usize = 64;

/// The text `s` holds, including its tables' cell text. A table's cells each keep their text in
/// a story of their own rather than in `s.text`, which holds only the anchor, so counting
/// `s.text` scored a table of any size as a couple of bytes (noted in review).
fn note_bytes(s: &Story, depth: usize) -> usize {
    let mut n = s.text.len();
    if depth == 0 {
        return n;
    }
    for t in s.tables.values() {
        for c in &t.cells {
            n = n.saturating_add(note_bytes(&c.text, depth - 1));
        }
    }
    n
}

/// Warnings one import reports. A document that earns more than this has earned the point.
const MAX_WARNINGS: usize = 100;

impl Ctx {
    /// Report `message` once. Beyond [`MAX_WARNINGS`] distinct messages nothing further is kept,
    /// not even in the set, so neither the report nor the memory behind it grows with the
    /// document -- a footnote warning names an id, and a file can hold as many ids as it likes.
    fn warn(&mut self, message: String) {
        if self.warnings.len() > MAX_WARNINGS {
            return;
        }
        if !self.warned.insert(message.clone()) {
            return;
        }
        if self.warnings.len() == MAX_WARNINGS {
            self.warnings.push(format!("The Word file has more than {MAX_WARNINGS} kinds of problem: the rest are not listed."));
        } else {
            self.warnings.push(message);
        }
    }

    fn para_style(&self, ppr: Option<&El>) -> String {
        ppr.and_then(|p| p.val("pStyle")).and_then(|id| self.para_names.get(&id).cloned()).unwrap_or_else(|| self.default_para.clone())
    }

    /// Append the paragraphs and tables of `body` to `st`.
    fn blocks(&mut self, body: &El, st: &mut Story, allow_notes: bool) {
        let mut first = st.is_empty();
        for e in body.els() {
            match e.name.as_str() {
                "p" => {
                    if !first {
                        let len = st.len();
                        st.insert(len, "\n");
                    }
                    first = false;
                    let ppr = e.child("pPr");
                    let style = self.para_style(ppr);
                    let mut attrs = para_attrs(ppr);
                    let list = self.list(ppr);
                    if let Some(l) = &list {
                        // The level's indents, unless the paragraph sets its own.
                        let direct = attrs;
                        attrs = l.attrs.clone();
                        attrs.merge(&direct);
                    }
                    let at = st.len();
                    st.format_paras(at..at, |f| *f = ParaFormat { style: style.clone(), para: attrs.clone(), ..Default::default() });
                    self.inline(e, st, allow_notes);
                    let (start_at, expression) = self.start_at(list.as_ref(), st.len() == at);
                    if let Some(n) = start_at {
                        st.format_paras(at..at, |f| f.para.start_at = Some(Some(n)));
                    }
                    if let Some(e) = expression {
                        st.format_paras(at..at, |f| f.para.number_expression = Some(e.clone()));
                    }
                }
                "tbl" => {
                    let mut rows: Vec<Vec<String>> = Vec::new();
                    for r in e.els().filter(|r| r.name == "tr") {
                        let mut row = Vec::new();
                        for c in r.els().filter(|c| c.name == "tc") {
                            let mut paras = Vec::new();
                            self.cell_paras(c, &mut paras);
                            row.push(paras.join("\n"));
                        }
                        rows.push(row);
                    }
                    if rows.is_empty() {
                        continue;
                    }
                    self.next_table += 1;
                    let t = Table::from_strings(self.next_table, &rows, 300.0, &ParaFormat::default(), &CharFormat::default());
                    let len = st.len();
                    st.insert_table(len, t);
                    first = st.text.ends_with('\n');
                }
                "sdt" => {
                    if let Some(c) = e.child("sdtContent") {
                        self.blocks(c, st, allow_notes);
                    }
                }
                _ => {}
            }
        }
    }

    /// The text of each paragraph in a table cell, read by the same run rules as body text.
    /// Cells are plain text, so a page or column break in a cell ends its paragraph.
    fn cell_paras(&mut self, el: &El, out: &mut Vec<String>) {
        for e in el.els() {
            match e.name.as_str() {
                "p" => {
                    let mut st = Story::new(StoryId(0));
                    self.inline(e, &mut st, false);
                    let is_break = |c: char| matches!(c, designcraft_doc::PAGE_BREAK | designcraft_doc::COLUMN_BREAK);
                    out.push(st.text.chars().map(|c| if is_break(c) { '\n' } else { c }).collect());
                }
                "sdt" => {
                    if let Some(c) = e.child("sdtContent") {
                        self.cell_paras(c, out);
                    }
                }
                _ => {}
            }
        }
    }

    /// The list a paragraph is in (its own `w:numPr`, else its style's), counting it.
    fn list(&mut self, ppr: Option<&El>) -> Option<ListPara> {
        let direct = ppr.and_then(|p| p.child("numPr"));
        let style = ppr.and_then(|p| p.val("pStyle")).and_then(|id| self.style_lists.get(&id));
        let num_id = direct.and_then(|n| n.val("numId")).or_else(|| style.and_then(|s| s.0.clone()))?;
        let ilvl = direct.and_then(|n| n.val("ilvl")).or_else(|| style.and_then(|s| s.1.clone()));
        // A level that isn't a small number is no list.
        let ilvl = ilvl.map_or(Some(0), |l| l.trim().parse::<u32>().ok())?;
        self.numbering.next(num_id.trim(), ilvl)
    }

    /// Start At for a paragraph, so that the model numbers it as Word does: only where the
    /// model's counter (per level; restarted after a paragraph that isn't in a list) would give a
    /// different number. And the number expression with parent levels as text, where the model's
    /// numbers for those levels differ from Word's.
    fn start_at(&mut self, list: Option<&ListPara>, empty: bool) -> (Option<u32>, Option<String>) {
        let Some(l) = list else {
            self.model_counter = ListCounter::default();
            return (None, None);
        };
        // Bullets and empty paragraphs leave the counter alone (they show no number).
        if l.attrs.list_type != Some(ListType::Numbers) || empty {
            return (None, None);
        }
        let want = l.number.max(1);
        let mut pp = ParaProps::default();
        pp.apply(&l.attrs);
        let mut model = self.model_counter.clone();
        let start = (model.advance(&pp) != want).then_some(want);
        if start.is_some() {
            model = self.model_counter.clone();
            pp.start_at = start;
            model.advance(&pp);
        }
        let level_text = |k: u32| ParaProps { number_expression: format!("^{k}"), ..ParaProps::default() }.number_label_in(&model);
        let expression = l.parents.iter().any(|(k, text)| level_text(*k) != *text).then(|| l.fixed_expression.clone()).flatten();
        self.model_counter = model;
        (start, expression)
    }

    /// Runs (and hyperlinks, fields' results) of a paragraph.
    fn inline(&mut self, p: &El, st: &mut Story, allow_notes: bool) {
        for e in p.els() {
            match e.name.as_str() {
                "r" => self.run(e, st, allow_notes),
                // Tracked changes import as accepted: insertions and move destinations are kept,
                // deletions (`del`) and move sources (`moveFrom`) are dropped.
                "hyperlink" | "smartTag" | "ins" | "moveTo" | "fldSimple" | "customXml" => self.inline(e, st, allow_notes),
                // A content control (date picker, drop-down, plain text) keeps its runs in `sdtContent`.
                "sdt" => {
                    if let Some(c) = e.child("sdtContent") {
                        self.inline(c, st, allow_notes);
                    }
                }
                _ => {}
            }
        }
    }

    fn run(&mut self, r: &El, st: &mut Story, allow_notes: bool) {
        let rpr = r.child("rPr");
        let style = rpr.and_then(|p| p.val("rStyle")).and_then(|id| self.char_names.get(&id).cloned()).unwrap_or_else(|| NO_CHAR_STYLE.into());
        let fmt = CharFormat { style, over: run_attrs(rpr) };
        for e in r.els() {
            let text = match e.name.as_str() {
                "t" => e.text(),
                "tab" => "\t".into(),
                "br" => match e.attr("type") {
                    Some("page") => designcraft_doc::PAGE_BREAK.to_string(),
                    Some("column") => designcraft_doc::COLUMN_BREAK.to_string(),
                    _ => designcraft_doc::FORCED_LINE_BREAK.to_string(),
                },
                "noBreakHyphen" => "\u{2011}".into(),
                "softHyphen" => "\u{AD}".into(),
                "footnoteReference" if allow_notes => {
                    const MAX_NOTE_BYTES: usize = 8 << 20;
                    let id = e.attr("id").unwrap_or_default().to_string();
                    // The note's text is built once per id, and only while the budget lasts. A
                    // repeat reference, or one past the budget, still gets its mark and its
                    // number -- the reference is part of the text -- with an empty note (#1584).
                    //
                    // The id is recorded only once its text has actually been kept, further down:
                    // recording it here meant a first reference arriving after the budget was
                    // spent counted as "done" with an empty note, and the next reference to it
                    // then claimed the text was kept at the first one (noted in review).
                    let again = !id.is_empty() && self.notes_done.contains(&id);
                    let spent = self.note_bytes >= MAX_NOTE_BYTES;
                    if again || spent {
                        let len = st.len();
                        if st.insert_note(len, "", ParaFormat::default()).is_some() {
                            let message = if again {
                                format!("Footnote {id} is referenced more than once in the Word file: the text is kept at its first reference.")
                            } else {
                                format!("The Word file's footnotes are longer than {} MB: the rest were imported empty.", MAX_NOTE_BYTES / (1 << 20))
                            };
                            self.warn(message);
                        }
                        continue;
                    }
                    if let Some(note) = e.attr("id").and_then(|id| self.footnotes.get(id).cloned()) {
                        let mut ns = Story::new(StoryId(0));
                        let counter = std::mem::take(&mut self.model_counter);
                        self.blocks(&note, &mut ns, false);
                        self.model_counter = counter;
                        self.note_bytes = self.note_bytes.saturating_add(note_bytes(&ns, MAX_NOTE_DEPTH));
                        if !id.is_empty() {
                            self.notes_done.insert(id.clone());
                        }
                        // Word puts the reference mark in the note text: we generate the number.
                        let len = st.len();
                        match st.insert_note(len, "", ParaFormat::default()) {
                            Some(id) => {
                                if let Some(n) = st.note_mut(id) {
                                    n.text = ns;
                                }
                            }
                            None => self.warn("A footnote in the Word file could not be imported.".into()),
                        }
                    }
                    continue;
                }
                "drawing" | "pict" | "object" => {
                    self.warn("Images and drawings in the Word file were not imported (place them separately).".into());
                    continue;
                }
                _ => continue,
            };
            if !text.is_empty() {
                let len = st.len();
                st.insert_with(len, &text, fmt.clone());
            }
        }
    }
}

/// Largest `word/numbering.xml` read.
const NUMBERING_LIMIT: u64 = 16 * 1024 * 1024;
/// Deepest `w:basedOn` chain followed (a cycle stops here too).
const MAX_STYLE_DEPTH: usize = 32;

/// Each paragraph style's list: the first `w:numId` and `w:ilvl` on its `w:basedOn` chain.
fn style_lists(styles: &El) -> HashMap<String, (Option<String>, Option<String>)> {
    let by_id: HashMap<&str, &El> = styles.els().filter(|s| s.name == "style").filter_map(|s| Some((s.attr("styleId")?, s))).collect();
    let mut out = HashMap::new();
    for (id, s) in &by_id {
        let (mut num_id, mut ilvl) = (None, None);
        let mut cur = Some(*s);
        for _ in 0..MAX_STYLE_DEPTH {
            let Some(c) = cur else { break };
            if let Some(n) = c.child("pPr").and_then(|p| p.child("numPr")) {
                num_id = num_id.or_else(|| n.val("numId"));
                ilvl = ilvl.or_else(|| n.val("ilvl"));
            }
            if num_id.is_some() && ilvl.is_some() {
                break;
            }
            cur = c.val("basedOn").and_then(|b| by_id.get(b.as_str()).copied());
        }
        if num_id.is_some() {
            out.insert(id.to_string(), (num_id, ilvl));
        }
    }
    out
}

pub fn import(bytes: &[u8]) -> Result<Imported, ImportError> {
    let mut zip = zip::ZipArchive::new(std::io::Cursor::new(bytes)).map_err(|e| ImportError::Corrupt(e.to_string()))?;
    let doc = part(&mut zip, "word/document.xml")?.ok_or_else(|| ImportError::Corrupt("no word/document.xml (not a Word document)".into()))?;
    let doc = parse(&doc)?;
    // Styles.
    let mut ctx = Ctx {
        para_names: HashMap::new(),
        char_names: HashMap::new(),
        footnotes: HashMap::new(),
        notes_done: std::collections::HashSet::new(),
        note_bytes: 0,
        warnings: vec![],
        warned: std::collections::HashSet::new(),
        next_table: 0,
        numbering: Numbering::default(),
        style_lists: HashMap::new(),
        model_counter: ListCounter::default(),
        default_para: BASIC_PARAGRAPH.into(),
    };
    // Lists that can't be read leave plain paragraphs, not a failed import.
    match part_capped(&mut zip, "word/numbering.xml", NUMBERING_LIMIT).and_then(|s| s.map(|s| parse(&s)).transpose()) {
        Ok(Some(n)) => ctx.numbering = Numbering::parse(&n),
        Ok(None) => {}
        Err(_) => ctx.warnings.push("The Word file's list definitions could not be read: lists were imported as plain paragraphs.".into()),
    }
    let mut para_styles = Vec::new();
    let mut char_styles = Vec::new();
    if let Some(styles) = part(&mut zip, "word/styles.xml")?.map(|s| parse(&s)).transpose()? {
        let ids: HashMap<String, String> =
            styles.els().filter(|s| s.name == "style").filter_map(|s| Some((s.attr("styleId")?.to_string(), s.val("name")?))).collect();
        ctx.style_lists = style_lists(&styles);
        // Word's document defaults sit beneath every paragraph style that isn't based on another.
        let defaults = styles.child("docDefaults");
        let default_para = para_attrs(defaults.and_then(|d| d.child("pPrDefault")).and_then(|d| d.child("pPr")));
        let default_chars = run_attrs(defaults.and_then(|d| d.child("rPrDefault")).and_then(|d| d.child("rPr")));
        let is_para = |s: &&El| s.name == "style" && s.attr("type").unwrap_or("paragraph") == "paragraph";
        let default_id = styles
            .els()
            .filter(is_para)
            .find(|s| matches!(s.attr("default"), Some("1" | "true" | "on")))
            .or_else(|| styles.els().filter(is_para).find(|s| s.attr("styleId") == Some("Normal")))
            .and_then(|s| s.attr("styleId"));
        if let Some(name) = default_id.and_then(|id| ids.get(id)) {
            ctx.default_para = name.clone();
        }
        for s in styles.els().filter(|s| s.name == "style") {
            let (Some(id), Some(name)) = (s.attr("styleId"), s.val("name")) else { continue };
            let kind = s.attr("type").unwrap_or("paragraph");
            // Word's default paragraph font is "no character style": our [None].
            let name = match (kind, name.as_str()) {
                ("character", "Default Paragraph Font") => NO_CHAR_STYLE.to_string(),
                _ => name.clone(),
            };
            let based_on = s.val("basedOn").and_then(|b| ids.get(&b).cloned());
            let mut imp = ImportedStyle { name: name.clone(), based_on, para: para_attrs(s.child("pPr")), chars: run_attrs(s.child("rPr")) };
            match kind {
                "paragraph" => {
                    ctx.para_names.insert(id.to_string(), name.clone());
                    if imp.based_on.is_none() {
                        let (mut para, mut chars) = (default_para.clone(), default_chars.clone());
                        para.merge(&imp.para);
                        chars.merge(&imp.chars);
                        (imp.para, imp.chars) = (para, chars);
                    }
                    para_styles.push(imp);
                }
                "character" => {
                    ctx.char_names.insert(id.to_string(), name.clone());
                    if name != NO_CHAR_STYLE {
                        char_styles.push(imp);
                    }
                }
                _ => {}
            }
        }
    }
    // Footnotes (ids -1 / 0 are the separators).
    if let Some(f) = part(&mut zip, "word/footnotes.xml")?.map(|s| parse(&s)).transpose()? {
        for n in f.els().filter(|n| n.name == "footnote") {
            if let Some(id) = n.attr("id").filter(|id| id.parse::<i64>().is_ok_and(|v| v > 0)) {
                ctx.footnotes.insert(id.to_string(), n.clone());
            }
        }
    }
    let body = doc.child("body").ok_or_else(|| ImportError::Corrupt("no document body".into()))?;
    let mut st = Story::new(StoryId(0));
    ctx.blocks(body, &mut st, true);
    // A page or column break ends its paragraph.
    st.end_paragraphs_at_breaks();
    // Only styles that are used, plus what they are based on.
    let stories = || std::iter::once(&st).chain(st.notes.iter().map(|n| &n.text));
    let used_p: std::collections::HashSet<String> = stories().flat_map(|s| s.paras.iter().map(|p| p.style.clone())).collect();
    let used_c: std::collections::HashSet<String> = stories().flat_map(|s| s.runs().map(|(_, f)| f.style.clone())).collect();
    let keep = |list: Vec<ImportedStyle>, used: &std::collections::HashSet<String>| {
        let mut want: std::collections::HashSet<String> = used.clone();
        loop {
            let more: Vec<String> =
                list.iter().filter(|s| want.contains(&s.name)).filter_map(|s| s.based_on.clone()).filter(|b| !want.contains(b)).collect();
            if more.is_empty() {
                break;
            }
            want.extend(more);
        }
        list.into_iter().filter(|s| want.contains(&s.name)).collect::<Vec<_>>()
    };
    Ok(Imported { story: st, para_styles: keep(para_styles, &used_p), char_styles: keep(char_styles, &used_c), warnings: ctx.warnings })
}

#[cfg(test)]
mod tests;
