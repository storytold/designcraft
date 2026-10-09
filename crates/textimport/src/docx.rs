//! Word (.docx, Office Open XML) → story.

use designcraft_doc::story::{BASIC_PARAGRAPH, NO_CHAR_STYLE, TABLE_ANCHOR};
use designcraft_doc::{CharAttrs, CharFormat, ParaAttrs, ParaFormat, Story, StoryId, Table};
use quick_xml::events::Event;
use std::collections::{HashMap, HashSet};

use crate::archive::{open, part};
use crate::budget::{OUTPUT_LIMITS, OutputBudget, OutputLimits};
use crate::{ImportError, Imported, ImportedStyle};

const MAX_XML_DEPTH: usize = 256;
const MAX_XML_EVENTS: usize = 1_000_000;
const MAX_XML_NODES: usize = 500_000;

#[derive(Clone, Copy)]
struct XmlLimits {
    depth: usize,
    events: usize,
    nodes: usize,
}

const XML_LIMITS: XmlLimits = XmlLimits { depth: MAX_XML_DEPTH, events: MAX_XML_EVENTS, nodes: MAX_XML_NODES };

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
    fn val(&self, name: &str) -> Option<String> {
        self.child(name).map(|e| e.attr("val").unwrap_or("true").to_string())
    }
    fn on(&self, name: &str) -> Option<bool> {
        self.val(name).map(|v| !matches!(v.as_str(), "0" | "false" | "none"))
    }
}

fn local(n: &[u8]) -> String {
    let s = String::from_utf8_lossy(n);
    s.rsplit(':').next().unwrap_or(&s).to_string()
}

pub(crate) fn parse(xml: &str) -> Result<El, ImportError> {
    parse_with_limits(xml, XML_LIMITS)
}

fn parse_with_limits(xml: &str, limits: XmlLimits) -> Result<El, ImportError> {
    let mut r = quick_xml::Reader::from_str(xml);
    let mut stack: Vec<El> = vec![El::default()];
    let mut events = 0usize;
    let mut nodes = 0usize;
    loop {
        let event = r.read_event().map_err(|e| ImportError::Corrupt(e.to_string()))?;
        events = events.saturating_add(1);
        if events > limits.events {
            return Err(ImportError::Corrupt(format!("XML contains more than {} events", limits.events)));
        }
        match event {
            Event::Start(e) => {
                if stack.len() > limits.depth {
                    return Err(ImportError::Corrupt(format!("XML nesting exceeds {} elements", limits.depth)));
                }
                nodes = nodes.saturating_add(1);
                if nodes > limits.nodes {
                    return Err(ImportError::Corrupt(format!("XML contains more than {} nodes", limits.nodes)));
                }
                let el = El {
                    name: local(e.name().as_ref()),
                    attrs: e
                        .attributes()
                        .flatten()
                        .map(|a| (local(a.key.as_ref()), a.normalized_value(Default::default()).map(|v| v.to_string()).unwrap_or_default()))
                        .collect(),
                    kids: vec![],
                };
                stack.push(el);
            }
            Event::Empty(e) => {
                if stack.len() > limits.depth {
                    return Err(ImportError::Corrupt(format!("XML nesting exceeds {} elements", limits.depth)));
                }
                nodes = nodes.saturating_add(1);
                if nodes > limits.nodes {
                    return Err(ImportError::Corrupt(format!("XML contains more than {} nodes", limits.nodes)));
                }
                let el = El {
                    name: local(e.name().as_ref()),
                    attrs: e
                        .attributes()
                        .flatten()
                        .map(|a| (local(a.key.as_ref()), a.normalized_value(Default::default()).map(|v| v.to_string()).unwrap_or_default()))
                        .collect(),
                    kids: vec![],
                };
                if let Some(p) = stack.last_mut() {
                    p.kids.push(Node::El(el));
                }
            }
            Event::End(_) => {
                if stack.len() <= 1 {
                    return Err(ImportError::Corrupt("XML contains an unexpected closing element".into()));
                }
                let el = stack.pop().unwrap_or_default();
                match stack.last_mut() {
                    Some(p) => p.kids.push(Node::El(el)),
                    None => return Ok(el),
                }
            }
            Event::Text(t) => {
                nodes = nodes.saturating_add(1);
                if nodes > limits.nodes {
                    return Err(ImportError::Corrupt(format!("XML contains more than {} nodes", limits.nodes)));
                }
                let s = t.decode().map(|s| s.to_string()).unwrap_or_default();
                let s = quick_xml::escape::unescape(&s).map(|s| s.to_string()).unwrap_or(s);
                if let Some(p) = stack.last_mut() {
                    p.kids.push(Node::Text(s));
                }
            }
            Event::GeneralRef(e) => {
                nodes = nodes.saturating_add(1);
                if nodes > limits.nodes {
                    return Err(ImportError::Corrupt(format!("XML contains more than {} nodes", limits.nodes)));
                }
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
    if stack.len() != 1 {
        return Err(ImportError::Corrupt("XML ended before all elements were closed".into()));
    }
    let mut root = stack.pop().unwrap_or_default();
    root.kids
        .drain(..)
        .find_map(|n| if let Node::El(e) = n { Some(e) } else { None })
        .ok_or_else(|| ImportError::Corrupt("XML has no root element".into()))
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
    budget: OutputBudget,
}

impl Ctx {
    fn para_style(&self, ppr: Option<&El>) -> String {
        ppr.and_then(|p| p.val("pStyle")).and_then(|id| self.para_names.get(&id).cloned()).unwrap_or_else(|| BASIC_PARAGRAPH.into())
    }

    /// Append the paragraphs and tables of `body` to `st`.
    fn blocks(&mut self, body: &El, st: &mut Story, allow_notes: bool) -> Result<(), ImportError> {
        let mut first = st.is_empty();
        for e in body.els() {
            match e.name.as_str() {
                "p" => {
                    self.budget.add_items(1)?;
                    if !first {
                        self.budget.add_text(1)?;
                        let len = st.len();
                        st.insert(len, "\n");
                    }
                    first = false;
                    let ppr = e.child("pPr");
                    let style = self.para_style(ppr);
                    let attrs = para_attrs(ppr);
                    let at = st.len();
                    st.format_paras(at..at, |f| *f = ParaFormat { style: style.clone(), para: attrs.clone(), ..Default::default() });
                    self.inline(e, st, allow_notes)?;
                }
                "tbl" => {
                    let mut rows = Vec::new();
                    for r in e.els().filter(|r| r.name == "tr") {
                        let mut row = Vec::new();
                        for c in r.els().filter(|c| c.name == "tc") {
                            let text = c.els().filter(|p| p.name == "p").map(El::text).collect::<Vec<_>>().join("\n");
                            self.budget.add_items(1)?;
                            self.budget.add_text(text.len())?;
                            row.push(text);
                        }
                        rows.push(row);
                    }
                    if rows.is_empty() {
                        continue;
                    }
                    self.budget.add_items(1)?;
                    self.budget.add_text(TABLE_ANCHOR.len_utf8())?;
                    self.next_table = self.next_table.checked_add(1).ok_or_else(|| ImportError::Corrupt("table id overflow".into()))?;
                    let t = Table::from_strings(self.next_table, &rows, 300.0, &ParaFormat::default(), &CharFormat::default());
                    let len = st.len();
                    st.insert_table(len, t);
                    first = false;
                }
                "sdt" => {
                    if let Some(c) = e.child("sdtContent") {
                        self.blocks(c, st, allow_notes)?;
                    }
                }
                _ => {}
            }
        }
        Ok(())
    }

    /// Runs (and hyperlinks, fields' results) of a paragraph.
    fn inline(&mut self, p: &El, st: &mut Story, allow_notes: bool) -> Result<(), ImportError> {
        for e in p.els() {
            match e.name.as_str() {
                "r" => self.run(e, st, allow_notes)?,
                "hyperlink" | "smartTag" | "ins" | "fldSimple" | "customXml" => self.inline(e, st, allow_notes)?,
                _ => {}
            }
        }
        Ok(())
    }

    fn run(&mut self, r: &El, st: &mut Story, allow_notes: bool) -> Result<(), ImportError> {
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
                        self.budget.add_items(1)?;
                        let mut ns = Story::new(StoryId(0));
                        self.blocks(&note, &mut ns, false)?;
                        // Word puts the reference mark in the note text: we generate the number.
                        self.budget.add_text(designcraft_doc::FOOTNOTE_REF.len_utf8())?;
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
                self.budget.add_items(1)?;
                self.budget.add_text(text.len())?;
                let len = st.len();
                st.insert_with(len, &text, fmt.clone());
            }
        }
        Ok(())
    }
}

fn charge_retained_style(style: &ImportedStyle, budget: &mut OutputBudget) -> Result<(), ImportError> {
    budget.add_items(1)?;
    for text in [Some(style.name.as_str()), style.based_on.as_deref(), style.chars.font_family.as_deref(), style.chars.font_style.as_deref()]
        .into_iter()
        .flatten()
    {
        budget.add_text(text.len())?;
    }
    Ok(())
}

/// Keep used styles and their inheritance closure in linear time. A seen set bounds cyclic or
/// repeated `basedOn` edges, while the name index avoids rescanning every style for each ancestor.
fn keep_styles(list: Vec<ImportedStyle>, used: &HashSet<String>, budget: &mut OutputBudget) -> Result<Vec<ImportedStyle>, ImportError> {
    let mut parents: HashMap<&str, Vec<&str>> = HashMap::new();
    for style in &list {
        if let Some(parent) = style.based_on.as_deref() {
            parents.entry(style.name.as_str()).or_default().push(parent);
        }
    }

    let mut pending: Vec<&str> = used.iter().map(String::as_str).collect();
    let mut seen = HashSet::new();
    while let Some(name) = pending.pop() {
        if !seen.insert(name) {
            continue;
        }
        if let Some(bases) = parents.get(name) {
            pending.extend(bases.iter().copied());
        }
    }

    let mut retained = vec![false; list.len()];
    for (index, style) in list.iter().enumerate() {
        if seen.contains(style.name.as_str()) {
            charge_retained_style(style, budget)?;
            retained[index] = true;
        }
    }
    drop(seen);
    drop(parents);
    Ok(list.into_iter().zip(retained).filter_map(|(style, keep)| keep.then_some(style)).collect())
}

pub fn import(bytes: &[u8]) -> Result<Imported, ImportError> {
    import_with_limits(bytes, OUTPUT_LIMITS)
}

fn import_with_limits(bytes: &[u8], output_limits: OutputLimits) -> Result<Imported, ImportError> {
    let mut zip = open(bytes)?;
    let doc = part(&mut zip, "word/document.xml")?.ok_or_else(|| ImportError::Corrupt("no word/document.xml (not a Word document)".into()))?;
    let doc = parse(&doc)?;
    // Styles.
    let mut ctx = Ctx {
        para_names: HashMap::new(),
        char_names: HashMap::new(),
        footnotes: HashMap::new(),
        warnings: vec![],
        next_table: 0,
        budget: OutputBudget::new(output_limits),
    };
    let mut para_styles = Vec::new();
    let mut char_styles = Vec::new();
    if let Some(styles) = part(&mut zip, "word/styles.xml")?.map(|s| parse(&s)).transpose()? {
        let ids: HashMap<String, String> =
            styles.els().filter(|s| s.name == "style").filter_map(|s| Some((s.attr("styleId")?.to_string(), s.val("name")?))).collect();
        for s in styles.els().filter(|s| s.name == "style") {
            let (Some(id), Some(name)) = (s.attr("styleId"), s.val("name")) else { continue };
            let kind = s.attr("type").unwrap_or("paragraph");
            // Word's Normal / default paragraph font are our [Basic Paragraph] / [None].
            let name = match (kind, name.as_str()) {
                ("paragraph", "Normal") => BASIC_PARAGRAPH.to_string(),
                ("character", "Default Paragraph Font") => NO_CHAR_STYLE.to_string(),
                _ => name.clone(),
            };
            let based_on = s.val("basedOn").and_then(|b| ids.get(&b).cloned()).map(|b| if b == "Normal" { BASIC_PARAGRAPH.to_string() } else { b });
            let imp = ImportedStyle { name: name.clone(), based_on, para: para_attrs(s.child("pPr")), chars: run_attrs(s.child("rPr")) };
            match kind {
                "paragraph" => {
                    ctx.para_names.insert(id.to_string(), name.clone());
                    if name != BASIC_PARAGRAPH {
                        para_styles.push(imp);
                    }
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
    ctx.blocks(body, &mut st, true)?;
    // Only styles that are used, plus what they are based on.
    let used_p: HashSet<String> = st.paras.iter().map(|p| p.style.clone()).collect();
    let used_c: HashSet<String> = st.runs().map(|(_, f)| f.style.clone()).collect();
    let para_styles = keep_styles(para_styles, &used_p, &mut ctx.budget)?;
    let char_styles = keep_styles(char_styles, &used_c, &mut ctx.budget)?;
    Ok(Imported { story: st, para_styles, char_styles, warnings: ctx.warnings })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    pub(crate) fn docx(document: &str, styles: &str, footnotes: &str) -> Vec<u8> {
        let mut w = zip::ZipWriter::new(std::io::Cursor::new(Vec::new()));
        let o = zip::write::SimpleFileOptions::default();
        for (n, c) in [("word/document.xml", document), ("word/styles.xml", styles), ("word/footnotes.xml", footnotes)] {
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
        assert_eq!(st.paras[1].style, BASIC_PARAGRAPH);
        let at = st.text.find("bold").unwrap();
        assert_eq!(st.format_after(at).over.font_style.as_deref(), Some("Bold"));
        assert_eq!(st.format_after(st.text.find("styled").unwrap()).style, "Emphasis");
        assert_eq!(st.notes.len(), 1);
        assert_eq!(st.notes[0].text.text, "The note.");
        assert_eq!(st.tables.len(), 1);
        let names: Vec<&str> = i.para_styles.iter().map(|s| s.name.as_str()).collect();
        assert_eq!(names, ["heading 1"], "unused styles are skipped");
        assert_eq!(i.para_styles[0].chars.size, Some(16.0));
        assert_eq!(i.para_styles[0].para.space_before, Some(12.0));
        assert_eq!(i.char_styles[0].chars.font_style.as_deref(), Some("Italic"));
    }

    #[test]
    fn xml_resource_limits_cover_depth_event_and_node_counts() {
        let limits = |depth, events, nodes| XmlLimits { depth, events, nodes };

        assert!(parse_with_limits("<a><b><c/></b></a>", limits(2, 20, 20)).is_err());
        assert!(parse_with_limits("<a><b/><c/></a>", limits(10, 3, 20)).is_err());
        assert!(parse_with_limits("<a>one<b/>two</a>", limits(10, 20, 3)).is_err());
        assert!(parse_with_limits("<a><b/></a>", limits(2, 4, 2)).is_ok());
    }

    #[test]
    fn repeated_footnotes_share_a_cumulative_output_budget() {
        let note = format!(r#"<w:footnotes {W}><w:footnote w:id="1"><w:p><w:r><w:t>note</w:t></w:r></w:p></w:footnote></w:footnotes>"#);
        let document = |references: &str| format!(r#"<w:document {W}><w:body><w:p><w:r>{references}</w:r></w:p></w:body></w:document>"#);
        let reference = r#"<w:footnoteReference w:id="1"/>"#;
        let limits = OutputLimits { text_bytes: 10, items: 100 };

        assert!(import_with_limits(&docx(&document(reference), "", &note), limits).is_ok());
        assert!(matches!(
            import_with_limits(&docx(&document(&format!("{reference}{reference}")), "", &note), limits),
            Err(ImportError::Corrupt(message)) if message.contains("text exceeds")
        ));
    }

    #[test]
    fn retained_styles_and_their_strings_share_the_output_budget() {
        let styles = format!(
            r#"<w:styles {W}>
              <w:style w:type="paragraph" w:styleId="BaseId"><w:name w:val="Base"/></w:style>
              <w:style w:type="paragraph" w:styleId="ChildId"><w:name w:val="Child"/><w:basedOn w:val="BaseId"/>
                <w:rPr><w:rFonts w:ascii="Family"/><w:b/></w:rPr></w:style>
            </w:styles>"#
        );
        let document =
            format!(r#"<w:document {W}><w:body><w:p><w:pPr><w:pStyle w:val="ChildId"/></w:pPr><w:r><w:t>x</w:t></w:r></w:p></w:body></w:document>"#);
        let bytes = docx(&document, &styles, "");

        assert!(import_with_limits(&bytes, OutputLimits { text_bytes: 24, items: 4 }).is_ok());
        assert!(matches!(
            import_with_limits(&bytes, OutputLimits { text_bytes: 23, items: 4 }),
            Err(ImportError::Corrupt(message)) if message.contains("text exceeds")
        ));
        assert!(matches!(
            import_with_limits(&bytes, OutputLimits { text_bytes: 24, items: 3 }),
            Err(ImportError::Corrupt(message)) if message.contains("items")
        ));
    }

    #[test]
    fn style_inheritance_cycles_are_visited_once() {
        let style = |name: &str, based_on: &str| ImportedStyle { name: name.into(), based_on: Some(based_on.into()), ..Default::default() };
        let list = vec![style("A", "B"), style("B", "A"), style("unused", "A")];
        let used = HashSet::from(["A".to_string()]);
        let mut budget = OutputBudget::new(OutputLimits { text_bytes: 4, items: 2 });

        let kept = keep_styles(list, &used, &mut budget).unwrap();
        assert_eq!(kept.iter().map(|style| style.name.as_str()).collect::<Vec<_>>(), ["A", "B"]);
    }
}
