//! Word (.docx, Office Open XML) → story.

use std::collections::HashMap;
use std::io::Read;

use designcraft_doc::story::{BASIC_PARAGRAPH, NO_CHAR_STYLE};
use designcraft_doc::{CharAttrs, CharFormat, ParaAttrs, ParaFormat, Story, StoryId, Table};
use quick_xml::events::Event;

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
                        .map(|a| (local(a.key.as_ref()), a.unescape_value().map(|v| v.to_string()).unwrap_or_default()))
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
                        .map(|a| (local(a.key.as_ref()), a.unescape_value().map(|v| v.to_string()).unwrap_or_default()))
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
}

impl Ctx {
    fn para_style(&self, ppr: Option<&El>) -> String {
        ppr.and_then(|p| p.val("pStyle")).and_then(|id| self.para_names.get(&id).cloned()).unwrap_or_else(|| BASIC_PARAGRAPH.into())
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
                    let attrs = para_attrs(ppr);
                    let at = st.len();
                    st.format_paras(at..at, |f| *f = ParaFormat { style: style.clone(), para: attrs.clone(), ..Default::default() });
                    self.inline(e, st, allow_notes);
                }
                "tbl" => {
                    let rows: Vec<Vec<String>> = e
                        .els()
                        .filter(|r| r.name == "tr")
                        .map(|r| {
                            r.els()
                                .filter(|c| c.name == "tc")
                                .map(|c| c.els().filter(|p| p.name == "p").map(|p| p.text()).collect::<Vec<_>>().join("\n"))
                                .collect()
                        })
                        .collect();
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
                        self.blocks(&note, &mut ns, false);
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

pub fn import(bytes: &[u8]) -> Result<Imported, ImportError> {
    let mut zip = zip::ZipArchive::new(std::io::Cursor::new(bytes)).map_err(|e| ImportError::Corrupt(e.to_string()))?;
    let doc = part(&mut zip, "word/document.xml")?.ok_or_else(|| ImportError::Corrupt("no word/document.xml (not a Word document)".into()))?;
    let doc = parse(&doc)?;
    // Styles.
    let mut ctx = Ctx { para_names: HashMap::new(), char_names: HashMap::new(), footnotes: HashMap::new(), warnings: vec![], next_table: 0 };
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
    ctx.blocks(body, &mut st, true);
    // Only styles that are used, plus what they are based on.
    let used_p: std::collections::HashSet<String> = st.paras.iter().map(|p| p.style.clone()).collect();
    let used_c: std::collections::HashSet<String> = st.runs().map(|(_, f)| f.style.clone()).collect();
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
}
