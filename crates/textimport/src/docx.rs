//! Word (.docx, Office Open XML) → story.

use std::collections::HashMap;
use std::io::Read;

use designcraft_doc::story::{BASIC_PARAGRAPH, NO_CHAR_STYLE};
use designcraft_doc::{CharAttrs, CharFormat, ListType, ParaAttrs, ParaFormat, Story, StoryId, Table};
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
    pub(crate) fn on(&self, name: &str) -> Option<bool> {
        self.val(name).map(|v| !matches!(v.as_str(), "0" | "false" | "none"))
    }
}

fn local(n: &[u8]) -> String {
    let s = String::from_utf8_lossy(n);
    s.rsplit(':').next().unwrap_or(&s).to_string()
}

pub(crate) fn parse(xml: &str) -> Result<El, ImportError> {
    let mut r = quick_xml::Reader::from_str(xml);
    let mut stack: Vec<El> = vec![El::default()];
    loop {
        match r.read_event().map_err(|e| ImportError::Corrupt(e.to_string()))? {
            Event::Start(e) => {
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
        a.underline = Some(u != "none" && u != "0" && u != "false");
    }
    if let Some(s) = r.on("strike") {
        a.strikethrough = Some(s);
    }
    if let Some(sz) = r.val("sz").and_then(|v| v.parse::<f64>().ok()) {
        a.size = Some(sz / 2.0);
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
    let tw = |v: Option<&str>| v.and_then(|v| v.parse::<f64>().ok()).map(|v| v / 20.0);
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
    warnings: Vec<String>,
    next_table: u64,
    numbering: Numbering,
    /// A paragraph style's list (`w:numPr` numId and ilvl, through `w:basedOn`), by style id.
    style_lists: HashMap<String, (Option<String>, Option<String>)>,
    /// The number the model's own list counter is at (it restarts after a paragraph that isn't
    /// in a list), to tell where a numbered paragraph needs an explicit Start At.
    model_counter: u32,
    /// The style of paragraphs without `w:pStyle` (Word's default paragraph style, usually Normal).
    default_para: String,
}

impl Ctx {
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
                    if let Some(n) = self.start_at(list.as_ref(), st.len() == at) {
                        st.format_paras(at..at, |f| f.para.start_at = Some(Some(n)));
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
                    first = false;
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
    /// model's counter (which follows the previous numbered paragraph, and restarts after one
    /// that isn't in a list) would give a different number.
    fn start_at(&mut self, list: Option<&ListPara>, empty: bool) -> Option<u32> {
        let Some(l) = list else {
            self.model_counter = 0;
            return None;
        };
        // Bullets and empty paragraphs leave the counter alone (they show no number).
        if l.attrs.list_type != Some(ListType::Numbers) || empty {
            return None;
        }
        let want = l.number.max(1);
        let next = self.model_counter.saturating_add(1);
        self.model_counter = want;
        (want != next).then_some(want)
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

    /// Runs (and hyperlinks, fields' results) of a paragraph.
    fn inline(&mut self, p: &El, st: &mut Story, allow_notes: bool) {
        for e in p.els() {
            match e.name.as_str() {
                "r" => self.run(e, st, allow_notes),
                // Tracked changes import as accepted: insertions and move destinations are kept,
                // deletions (`del`) and move sources (`moveFrom`) are dropped.
                "hyperlink" | "smartTag" | "ins" | "moveTo" | "fldSimple" | "customXml" => self.inline(e, st, allow_notes),
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
                    if let Some(note) = e.attr("id").and_then(|id| self.footnotes.get(id).cloned()) {
                        let mut ns = Story::new(StoryId(0));
                        let counter = std::mem::take(&mut self.model_counter);
                        self.blocks(&note, &mut ns, false);
                        self.model_counter = counter;
                        // Word puts the reference mark in the note text: we generate the number.
                        let len = st.len();
                        let id = st.insert_note(len, "", ParaFormat::default());
                        if let Some(n) = st.note_mut(id) {
                            n.text = ns;
                        }
                    }
                    continue;
                }
                "drawing" | "pict" | "object" => {
                    if !self.warnings.iter().any(|w| w.starts_with("Images")) {
                        self.warnings.push("Images and drawings in the Word file were not imported (place them separately).".into());
                    }
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
        warnings: vec![],
        next_table: 0,
        numbering: Numbering::default(),
        style_lists: HashMap::new(),
        model_counter: 0,
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
mod tests {
    use super::*;
    use std::io::Write;

    pub(crate) fn docx(document: &str, styles: &str, footnotes: &str) -> Vec<u8> {
        docx_parts(&[("word/document.xml", document), ("word/styles.xml", styles), ("word/footnotes.xml", footnotes)])
    }

    /// A zip of `(part name, XML)`; empty parts are left out.
    fn docx_parts(parts: &[(&str, &str)]) -> Vec<u8> {
        let mut w = zip::ZipWriter::new(std::io::Cursor::new(Vec::new()));
        let o = zip::write::SimpleFileOptions::default();
        for &(n, c) in parts {
            if c.is_empty() {
                continue;
            }
            w.start_file(n, o).unwrap();
            w.write_all(c.as_bytes()).unwrap();
        }
        w.finish().unwrap().into_inner()
    }

    const W: &str = r#"xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main""#;

    #[test]
    fn oversized_part_is_an_error_not_a_huge_read() {
        // A highly compressible part: 64 KiB of spaces inflates past a 4 KiB cap.
        let big = docx(&" ".repeat(64 * 1024), "", "");
        let mut zip = zip::ZipArchive::new(std::io::Cursor::new(big.as_slice())).unwrap();
        assert!(matches!(part_capped(&mut zip, "word/document.xml", 4096), Err(ImportError::Corrupt(_))));
        // At or under the cap reads fine; a missing part is `None`.
        assert_eq!(part_capped(&mut zip, "word/document.xml", 64 * 1024).unwrap().map(|s| s.len()), Some(64 * 1024));
        assert_eq!(part_capped(&mut zip, "word/missing.xml", 4096).unwrap(), None);
    }

    #[test]
    fn imports_styles_runs_footnotes_and_tables() {
        let styles = format!(
            r#"<w:styles {W}>
              <w:style w:type="paragraph" w:styleId="Normal"><w:name w:val="Normal"/></w:style>
              <w:style w:type="paragraph" w:styleId="Heading1"><w:name w:val="heading 1"/><w:basedOn w:val="Normal"/>
                <w:pPr><w:spacing w:before="240" w:after="120"/></w:pPr><w:rPr><w:b/><w:sz w:val="32"/></w:rPr></w:style>
              <w:style w:type="paragraph" w:styleId="Unused"><w:name w:val="Unused"/></w:style>
              <w:style w:type="character" w:styleId="Emph"><w:name w:val="Emphasis"/><w:rPr><w:i/></w:rPr></w:style>
            </w:styles>"#
        );
        let document = format!(
            r#"<w:document {W}><w:body>
              <w:p><w:pPr><w:pStyle w:val="Heading1"/></w:pPr><w:r><w:t>Title &amp; more</w:t></w:r></w:p>
              <w:p><w:r><w:t xml:space="preserve">Plain </w:t></w:r><w:r><w:rPr><w:b/></w:rPr><w:t>bold</w:t></w:r>
                <w:r><w:rPr><w:rStyle w:val="Emph"/></w:rPr><w:t> styled</w:t></w:r><w:r><w:tab/><w:t>x</w:t></w:r>
                <w:r><w:footnoteReference w:id="1"/></w:r></w:p>
              <w:tbl><w:tr><w:tc><w:p><w:r><w:t>A</w:t></w:r></w:p></w:tc><w:tc><w:p><w:r><w:t>B</w:t></w:r></w:p></w:tc></w:tr></w:tbl>
              <w:p><w:r><w:t>After</w:t></w:r></w:p>
            </w:body></w:document>"#
        );
        let notes = format!(
            r#"<w:footnotes {W}><w:footnote w:id="0"><w:p/></w:footnote>
              <w:footnote w:id="1"><w:p><w:r><w:t>The note.</w:t></w:r></w:p></w:footnote></w:footnotes>"#
        );
        let i = import(&docx(&document, &styles, &notes)).unwrap();
        let st = &i.story;
        st.check().unwrap();
        assert!(st.text.starts_with("Title & more\nPlain bold styled\tx"), "{:?}", st.text);
        assert!(st.text.ends_with("After"));
        assert_eq!(st.paras[0].style, "heading 1");
        assert_eq!(st.paras[1].style, "Normal");
        let at = st.text.find("bold").unwrap();
        assert_eq!(st.format_after(at).over.font_style.as_deref(), Some("Bold"));
        assert_eq!(st.format_after(st.text.find("styled").unwrap()).style, "Emphasis");
        assert_eq!(st.notes.len(), 1);
        assert_eq!(st.notes[0].text.text, "The note.");
        assert_eq!(st.tables.len(), 1);
        let names: Vec<&str> = i.para_styles.iter().map(|s| s.name.as_str()).collect();
        assert_eq!(names, ["Normal", "heading 1"], "unused styles are skipped");
        assert_eq!(i.para_styles[1].based_on.as_deref(), Some("Normal"));
        assert_eq!(i.para_styles[1].chars.size, Some(16.0));
        assert_eq!(i.para_styles[1].para.space_before, Some(12.0));
        assert_eq!(i.char_styles[0].chars.font_style.as_deref(), Some("Italic"));
    }

    /// A page break ends its paragraph: the text after it is a paragraph of its own (same style).
    #[test]
    fn page_break_ends_its_paragraph() {
        let styles = format!(r#"<w:styles {W}><w:style w:type="paragraph" w:styleId="H"><w:name w:val="Chapter"/></w:style></w:styles>"#);
        let document = format!(
            r#"<w:document {W}><w:body>
              <w:p><w:r><w:t>End of one</w:t></w:r><w:r><w:br w:type="page"/></w:r></w:p>
              <w:p><w:pPr><w:pStyle w:val="H"/></w:pPr><w:r><w:br w:type="page"/><w:t>Two</w:t><w:br w:type="column"/></w:r></w:p>
            </w:body></w:document>"#
        );
        let st = import(&docx(&document, &styles, "")).unwrap().story;
        st.check().unwrap();
        let (pb, cb) = (designcraft_doc::PAGE_BREAK, designcraft_doc::COLUMN_BREAK);
        assert_eq!(st.text, format!("End of one{pb}\n{pb}\nTwo{cb}\n"));
        let styles: Vec<&str> = st.paras.iter().map(|p| p.style.as_str()).collect();
        assert_eq!(styles, [BASIC_PARAGRAPH, "Chapter", "Chapter", "Chapter"]);
    }

    #[test]
    fn tracked_changes_import_as_accepted_including_moves() {
        // Regression (#256): text in a tracked move destination (`w:moveTo`) vanished on import.
        let document = format!(
            r#"<w:document {W}><w:body>
              <w:p><w:r><w:t xml:space="preserve">Kept text. </w:t></w:r>
                <w:moveTo w:id="1" w:author="A"><w:r><w:t xml:space="preserve">MOVED HERE.</w:t></w:r>
                  <w:del w:id="5" w:author="A"><w:r><w:delText>MOVED THEN DELETED</w:delText></w:r></w:del></w:moveTo>
                <w:r><w:t xml:space="preserve"> Plain </w:t></w:r>
                <w:ins w:id="2" w:author="A"><w:r><w:t>INSERTED</w:t></w:r></w:ins>
                <w:del w:id="3" w:author="A"><w:r><w:delText>DELETED</w:delText></w:r></w:del></w:p>
              <w:p><w:moveFrom w:id="4" w:author="A"><w:r><w:t>MOVED HERE.</w:t></w:r></w:moveFrom><w:r><w:t>End.</w:t></w:r></w:p>
            </w:body></w:document>"#
        );
        let i = import(&docx(&document, "", "")).unwrap();
        i.story.check().unwrap();
        assert_eq!(i.story.text, "Kept text. MOVED HERE. Plain INSERTED\nEnd.");
    }

    /// A paragraph in a Word list: `w:numPr` with `numId` and, unless `None`, `ilvl`.
    fn list_p(num_id: &str, ilvl: Option<&str>, text: &str) -> String {
        let ilvl = ilvl.map(|l| format!(r#"<w:ilvl w:val="{l}"/>"#)).unwrap_or_default();
        format!(r#"<w:p><w:pPr><w:numPr>{ilvl}<w:numId w:val="{num_id}"/></w:numPr></w:pPr><w:r><w:t>{text}</w:t></w:r></w:p>"#)
    }

    fn plain_p(text: &str) -> String {
        format!(r#"<w:p><w:r><w:t>{text}</w:t></w:r></w:p>"#)
    }

    fn lists_docx(numbering: &str, styles: &str, body: &[String]) -> Imported {
        let document = format!(r#"<w:document {W}><w:body>{}</w:body></w:document>"#, body.concat());
        let i = import(&docx_parts(&[("word/document.xml", &document), ("word/styles.xml", styles), ("word/numbering.xml", numbering)])).unwrap();
        i.story.check().unwrap();
        i
    }

    /// The label the model gives paragraph `i` when its number is `n`.
    fn label(st: &Story, i: usize, n: u32) -> String {
        let mut p = designcraft_doc::ParaProps::default();
        p.apply(&st.paras[i].para);
        match p.list_type {
            ListType::Numbers => p.number_label(n),
            ListType::Bullets => p.bullet_label(),
            ListType::None => String::new(),
        }
    }

    /// Regression (#626): pandoc's Markdown list example lost its bullets and numbers.
    #[test]
    fn bullets_and_numbers_from_numbering_xml() {
        let numbering = format!(
            r#"<w:numbering {W}>
              <w:abstractNum w:abstractNumId="990"><w:lvl w:ilvl="0"><w:start w:val="1"/><w:numFmt w:val="bullet"/><w:lvlText w:val="•"/>
                <w:pPr><w:ind w:left="480" w:hanging="480"/></w:pPr></w:lvl></w:abstractNum>
              <w:abstractNum w:abstractNumId="991"><w:lvl w:ilvl="0"><w:start w:val="1"/><w:numFmt w:val="decimal"/><w:lvlText w:val="%1."/>
                <w:pPr><w:ind w:left="480" w:hanging="480"/></w:pPr></w:lvl></w:abstractNum>
              <w:num w:numId="1000"><w:abstractNumId w:val="990"/></w:num>
              <w:num w:numId="1001"><w:abstractNumId w:val="991"/><w:lvlOverride w:ilvl="0"><w:startOverride w:val="1"/></w:lvlOverride></w:num>
            </w:numbering>"#
        );
        let body = [
            plain_p("Intro paragraph."),
            list_p("1000", Some("0"), "first item"),
            list_p("1000", Some("0"), "second item"),
            list_p("1001", Some("0"), "step one"),
            list_p("1001", Some("0"), "step two"),
        ];
        let st = lists_docx(&numbering, "", &body).story;
        let types: Vec<Option<ListType>> = st.paras.iter().map(|p| p.para.list_type).collect();
        assert_eq!(types, [None, Some(ListType::Bullets), Some(ListType::Bullets), Some(ListType::Numbers), Some(ListType::Numbers)]);
        assert_eq!(label(&st, 1, 1), "•\t");
        assert_eq!(st.paras[3].para.number_style, Some(designcraft_doc::NumberStyle::Arabic));
        assert_eq!((label(&st, 3, 1), label(&st, 4, 2)), ("1.\t".to_string(), "2.\t".to_string()));
        // The model's own counter already starts at 1 and carries on: no Start At needed.
        assert_eq!((st.paras[3].para.start_at, st.paras[4].para.start_at), (None, None));
        assert_eq!((st.paras[1].para.left_indent, st.paras[1].para.first_line_indent), (Some(24.0), Some(-24.0)));
    }

    /// Level 1 inside level 0, parent numbers in a label, numbering that carries on past a plain
    /// paragraph, and a `num` with a start override restarting.
    #[test]
    fn nested_levels_and_restarts() {
        let numbering = format!(
            r#"<w:numbering {W}>
              <w:abstractNum w:abstractNumId="1">
                <w:lvl w:ilvl="0"><w:start w:val="1"/><w:numFmt w:val="decimal"/><w:lvlText w:val="%1."/></w:lvl>
                <w:lvl w:ilvl="1"><w:start w:val="1"/><w:numFmt w:val="lowerLetter"/><w:lvlText w:val="%2)"/><w:suff w:val="space"/>
                  <w:pPr><w:ind w:left="1440" w:hanging="360"/></w:pPr></w:lvl>
                <w:lvl w:ilvl="2"><w:start w:val="1"/><w:numFmt w:val="upperRoman"/><w:lvlText w:val="%1.%2.%3"/></w:lvl>
              </w:abstractNum>
              <w:num w:numId="1"><w:abstractNumId w:val="1"/></w:num>
              <w:num w:numId="2"><w:abstractNumId w:val="1"/><w:lvlOverride w:ilvl="0"><w:startOverride w:val="1"/></w:lvlOverride></w:num>
              <w:num w:numId="3"><w:abstractNumId w:val="1"/></w:num>
            </w:numbering>"#
        );
        let body = [
            list_p("1", None, "A"),
            list_p("1", Some("1"), "B"),
            list_p("1", Some("1"), "C"),
            list_p("1", Some("2"), "D"),
            list_p("1", Some("0"), "E"),
            plain_p("Between"),
            list_p("1", Some("0"), "F"),
            list_p("2", Some("0"), "G"),
            list_p("3", Some("0"), "H"),
        ];
        let st = lists_docx(&numbering, "", &body).story;
        let start: Vec<Option<Option<u32>>> = st.paras.iter().map(|p| p.para.start_at).collect();
        // Word's numbers: 1, a, b, 1.b.I, 2, –, 3, 1 (own counter), 4 (shares numId 1's). E's 2
        // follows the model's counter (1 after D).
        assert_eq!(start, [None, Some(Some(1)), None, Some(Some(1)), None, None, Some(Some(3)), Some(Some(1)), Some(Some(4))]);
        assert_eq!(label(&st, 1, 1), "a) ");
        assert_eq!(label(&st, 3, 1), "1.b.I\t");
        assert_eq!(st.paras[1].para.left_indent, Some(72.0));
        assert_eq!(st.paras[1].para.first_line_indent, Some(-18.0));
    }

    /// A paragraph style's `w:numPr` (inherited through `w:basedOn`) makes its paragraphs a list;
    /// numId 0 on a paragraph takes it out of the list.
    #[test]
    fn style_lists_and_num_id_zero() {
        let numbering = format!(
            r#"<w:numbering {W}>
              <w:abstractNum w:abstractNumId="7">
                <w:lvl w:ilvl="0"><w:numFmt w:val="bullet"/><w:lvlText w:val="&#xF0B7;"/><w:rPr><w:rFonts w:ascii="Symbol" w:hAnsi="Symbol"/></w:rPr></w:lvl>
                <w:lvl w:ilvl="1"><w:numFmt w:val="bullet"/><w:lvlText w:val="o"/><w:rPr><w:rFonts w:ascii="Courier New"/></w:rPr></w:lvl>
              </w:abstractNum>
              <w:num w:numId="5"><w:abstractNumId w:val="7"/></w:num>
            </w:numbering>"#
        );
        let styles = format!(
            r#"<w:styles {W}>
              <w:style w:type="paragraph" w:styleId="ListBullet"><w:name w:val="List Bullet"/><w:pPr><w:numPr><w:numId w:val="5"/></w:numPr></w:pPr></w:style>
              <w:style w:type="paragraph" w:styleId="ListBullet2"><w:name w:val="List Bullet 2"/><w:basedOn w:val="ListBullet"/>
                <w:pPr><w:numPr><w:ilvl w:val="1"/></w:numPr></w:pPr></w:style>
            </w:styles>"#
        );
        let styled =
            |id: &str, extra: &str, text: &str| format!(r#"<w:p><w:pPr><w:pStyle w:val="{id}"/>{extra}</w:pPr><w:r><w:t>{text}</w:t></w:r></w:p>"#);
        let body = [
            styled("ListBullet", "", "one"),
            styled("ListBullet2", "", "two"),
            styled("ListBullet", r#"<w:numPr><w:ilvl w:val="0"/><w:numId w:val="0"/></w:numPr>"#, "not a list"),
        ];
        let st = lists_docx(&numbering, &styles, &body).story;
        assert_eq!(label(&st, 0, 1), "•\t");
        assert_eq!(label(&st, 1, 1), "◦\t");
        assert_eq!(st.paras[2].para.list_type, None);
        assert_eq!(st.paras[2].style, "List Bullet");
    }

    /// Hostile list definitions: missing abstractNums and levels, huge or negative levels and
    /// starts, a long label, a `w:basedOn` cycle. No panic; what can't be a list is plain.
    #[test]
    fn hostile_numbering_falls_back_to_plain_paragraphs() {
        let long = "x".repeat(10_000);
        let numbering = format!(
            r#"<w:numbering {W}>
              <w:abstractNum w:abstractNumId="1">
                <w:lvl w:ilvl="0"><w:start w:val="99999999999999"/><w:numFmt w:val="decimal"/><w:lvlText w:val="%1{long}%9%"/></w:lvl>
                <w:lvl w:ilvl="99999999999999"><w:numFmt w:val="decimal"/></w:lvl>
                <w:lvl w:ilvl="-1"><w:numFmt w:val="decimal"/></w:lvl>
                <w:lvl w:ilvl="3"><w:start w:val="-5"/><w:numFmt w:val="ordinalText"/><w:lvlText w:val="%4^"/><w:lvlRestart w:val="99"/></w:lvl>
              </w:abstractNum>
              <w:num w:numId="1"><w:abstractNumId w:val="1"/></w:num>
              <w:num w:numId="2"><w:abstractNumId w:val="404"/></w:num>
              <w:num w:numId="3"/>
              <w:num><w:abstractNumId w:val="1"/></w:num>
            </w:numbering>"#
        );
        let styles = format!(
            r#"<w:styles {W}>
              <w:style w:type="paragraph" w:styleId="A"><w:name w:val="A"/><w:basedOn w:val="B"/></w:style>
              <w:style w:type="paragraph" w:styleId="B"><w:name w:val="B"/><w:basedOn w:val="A"/></w:style>
            </w:styles>"#
        );
        let body = [
            list_p("2", None, "missing abstractNum"),
            list_p("3", None, "num without abstractNum"),
            list_p("404", None, "unknown numId"),
            list_p("1", Some("99999999999999"), "huge level"),
            list_p("1", Some("9"), "level 9"),
            list_p("1", Some("-1"), "negative level"),
            list_p("1", Some("1"), "undefined level"),
            r#"<w:p><w:pPr><w:pStyle w:val="A"/><w:numPr><w:ilvl w:val="0"/></w:numPr></w:pPr><w:r><w:t>cycle</w:t></w:r></w:p>"#.to_string(),
            list_p("1", None, "huge start"),
            list_p("1", Some("3"), "negative start"),
        ];
        let st = lists_docx(&numbering, &styles, &body).story;
        assert!(st.paras[..8].iter().all(|p| p.para.list_type.is_none()), "{:?}", st.paras);
        assert_eq!(st.paras[8].para.start_at, Some(Some(u32::MAX)));
        let expr = st.paras[8].para.number_expression.clone().unwrap_or_default();
        assert!(expr.starts_with("^#x") && expr.chars().count() < 80, "{expr}");
        // A start below 0 counts from 0, which the model shows as 1; `^` in a label is literal.
        assert_eq!(label(&st, 9, 1), "1^\t");
        // Unreadable list definitions leave plain paragraphs and a warning, not a failed import.
        let document = format!(r#"<w:document {W}><w:body>{}</w:body></w:document>"#, list_p("1", None, "x"));
        let i = import(&docx_parts(&[("word/document.xml", &document), ("word/numbering.xml", "<w:numbering></w:num>")])).unwrap();
        assert_eq!(i.story.paras[0].para.list_type, None);
        assert_eq!(i.warnings.len(), 1);
    }

    #[test]
    fn an_element_with_many_attributes_parses_in_linear_time() {
        // RUSTSEC-2026-0194: before quick-xml 0.41 the duplicate-attribute check compared each name with every
        // earlier one on the element, so a crafted .docx/.xlsx with one huge tag hung the import (O(N²)).
        let attrs: String = (0..100_000).map(|i| format!(" w:a{i}=\"{i}\"")).collect();
        let xml = format!("<w:document><w:body><w:p{attrs}/></w:body></w:document>");
        let started = std::time::Instant::now();
        let doc = parse(&xml).unwrap();
        assert!(started.elapsed() < std::time::Duration::from_secs(5), "parse took {:?}", started.elapsed());
        let p = doc.child("body").and_then(|b| b.child("p")).unwrap();
        assert_eq!(p.attr("a0"), Some("0"));
        assert_eq!(p.attr("a99999"), Some("99999"));
    }

    #[test]
    fn table_cells_read_text_like_body_paragraphs() {
        // Regression (#721): cells took every text node, so deleted text, move sources and field
        // instructions showed up in the table.
        let para = r#"<w:p><w:r><w:t xml:space="preserve">kept </w:t></w:r>
            <w:del w:id="1" w:author="A"><w:r><w:delText>GONE</w:delText></w:r></w:del>
            <w:moveFrom w:id="2" w:author="A"><w:r><w:t>MOVED AWAY</w:t></w:r></w:moveFrom>
            <w:r><w:fldChar w:fldCharType="begin"/></w:r><w:r><w:instrText xml:space="preserve"> PAGE </w:instrText></w:r>
            <w:r><w:fldChar w:fldCharType="separate"/></w:r><w:r><w:t>7</w:t></w:r><w:r><w:fldChar w:fldCharType="end"/></w:r></w:p>"#;
        let document = format!(r#"<w:document {W}><w:body>{para}<w:tbl><w:tr><w:tc>{para}<w:p/></w:tc></w:tr></w:tbl></w:body></w:document>"#);
        let i = import(&docx(&document, "", "")).unwrap();
        assert!(i.story.text.starts_with("kept 7"), "{:?}", i.story.text);
        let cell = &i.story.tables.values().next().unwrap().cell(0, 0).unwrap().text;
        assert_eq!(cell.text, "kept 7\n");
    }

    #[test]
    fn normal_is_its_own_style_over_the_document_defaults() {
        // Regression (#722): text formatted only by Normal and the document defaults lost it.
        let styles = format!(
            r#"<w:styles {W}><w:docDefaults><w:rPrDefault><w:rPr><w:sz w:val="22"/></w:rPr></w:rPrDefault>
              <w:pPrDefault><w:pPr><w:spacing w:after="160"/></w:pPr></w:pPrDefault></w:docDefaults>
              <w:style w:type="paragraph" w:default="1" w:styleId="Normal"><w:name w:val="Normal"/><w:rPr><w:rFonts w:ascii="Georgia"/><w:sz w:val="26"/></w:rPr></w:style>
              <w:style w:type="paragraph" w:styleId="Quote"><w:name w:val="Quote"/><w:basedOn w:val="Normal"/><w:rPr><w:i/></w:rPr></w:style>
            </w:styles>"#
        );
        let document = format!(
            r#"<w:document {W}><w:body><w:p><w:r><w:t>Plain</w:t></w:r></w:p>
            <w:p><w:pPr><w:pStyle w:val="Quote"/></w:pPr><w:r><w:t>Quoted</w:t></w:r></w:p></w:body></w:document>"#
        );
        let i = import(&docx(&document, &styles, "")).unwrap();
        assert_eq!(i.story.paras[0].style, "Normal");
        let normal = i.para_styles.iter().find(|s| s.name == "Normal").unwrap();
        assert_eq!(normal.chars.size, Some(13.0), "Normal's size wins over the default");
        assert_eq!(normal.chars.font_family.as_deref(), Some("Georgia"));
        assert_eq!(normal.para.space_after, Some(8.0), "from the document defaults");
        let quote = i.para_styles.iter().find(|s| s.name == "Quote").unwrap();
        assert_eq!(quote.based_on.as_deref(), Some("Normal"));
        assert_eq!(quote.chars.size, None, "inherited from Normal, not copied");
    }
}
