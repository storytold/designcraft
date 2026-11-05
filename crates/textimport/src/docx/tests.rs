use super::*;
use std::io::Write;

#[test]
fn cdata_text() {
    let el = parse("<a><t><![CDATA[x < y]]></t></a>").unwrap();
    assert_eq!(el.child("t").unwrap().text(), "x < y");
}

/// Issue #712: nesting past the budget is an error from the parser, not a stack overflow later.
#[test]
fn nesting_past_the_budget_is_refused() {
    let nested = |n: usize| format!("{}{}", "<w:p>".repeat(n), "</w:p>".repeat(n));
    let ok = parse(&nested(MAX_DEPTH)).expect("the budget itself parses");
    assert_eq!(ok.name, "p");
    let err = parse(&nested(MAX_DEPTH + 1)).expect_err("one level more is refused");
    assert!(err.to_string().contains("nested deeper"), "{err}");
    assert!(parse(&nested(100_000)).is_err());
}

#[test]
fn table_adds_no_empty_paragraph() {
    let document = format!(
        r#"<w:document {W}><w:body>
              <w:p><w:r><w:t>Before</w:t></w:r></w:p>
              <w:tbl><w:tr><w:tc><w:p><w:r><w:t>A</w:t></w:r></w:p></w:tc></w:tr></w:tbl>
              <w:p><w:r><w:t>After</w:t></w:r></w:p>
            </w:body></w:document>"#
    );
    let st = import(&docx(&document, "", "")).unwrap().story;
    st.check().unwrap();
    let paras: Vec<&str> = st.para_ranges().into_iter().map(|r| &st.text[r]).collect();
    assert_eq!(paras.len(), 3, "{paras:?}");
    assert_eq!(paras[0], "Before");
    assert_eq!(paras[2], "After");
}

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

/// A DOCX can hold any number where a measure belongs: `1e300` twips became a 5e298 pt
/// indent, and `w:sz` of `1e300` a 5e299 pt type size. Measures are clamped to the widest
/// page and a size the engine cannot mean is dropped (#1593).
#[test]
fn a_measure_or_size_a_file_cannot_mean_is_bounded() {
    use designcraft_doc::attrs::MAX_MEASURE;
    let doc = format!(
        r#"<w:document {W}><w:body><w:p><w:pPr>
                 <w:spacing w:before="1e300" w:after="-1e300"/><w:ind w:left="1e300" w:firstLine="-1e300"/>
               </w:pPr><w:r><w:rPr><w:sz w:val="1e300"/></w:rPr><w:t>big</w:t></w:r></w:p></w:body></w:document>"#
    );
    let i = import(&docx(&doc, "", "")).unwrap();
    let p = &i.story.paras[0].para;
    for (what, v) in
        [("space before", p.space_before), ("space after", p.space_after), ("left indent", p.left_indent), ("first line", p.first_line_indent)]
    {
        let v = v.expect("the value is kept, bounded");
        assert!(v.is_finite() && v.abs() <= MAX_MEASURE, "{what} is within the widest page: {v}");
    }
    let size = i.story.char_format_at(i.story.text.find("big").unwrap() + 1).over.size;
    assert_eq!(size, None, "a size the engine cannot mean is dropped, so the style's size stands");
}

/// A footnote reference clones its note and walks it again, so one large footnote referenced
/// many times multiplied: the audit's case is a 1 MB note referenced 100k times, around
/// 100 GB, which aborts rather than fails (#1584). The text is kept at the first reference;
/// later ones get the mark and an empty note, and the report says so.
#[test]
fn a_footnote_referenced_many_times_is_imported_once() {
    let note_text = "x".repeat(2_000);
    let footnotes = format!(r#"<w:footnotes {W}><w:footnote w:id="2"><w:p><w:r><w:t>{note_text}</w:t></w:r></w:p></w:footnote></w:footnotes>"#);
    // One note, referenced 50 times.
    let refs = r#"<w:r><w:footnoteReference w:id="2"/></w:r>"#.repeat(50);
    let doc = format!(r#"<w:document {W}><w:body><w:p>{refs}</w:p></w:body></w:document>"#);
    let i = import(&docx(&doc, "", &footnotes)).unwrap();

    // Every reference is still in the text: the marks are the document's content.
    assert_eq!(i.story.notes.len(), 50, "all 50 references are kept");
    let with_text = i.story.notes.iter().filter(|n| !n.text.text.is_empty()).count();
    assert_eq!(with_text, 1, "only the first reference carries the note's text");
    let total: usize = i.story.notes.iter().map(|n| n.text.text.len()).sum();
    assert!(total <= note_text.len() + 64, "the note text is stored once, not 50 times: {total} bytes");
    assert!(i.warnings.iter().any(|w| w.contains("referenced more than once")), "the report says what it did: {:?}", i.warnings);

    // A document whose notes each have their own id is unaffected.
    let many = (2..6).map(|id| format!(r#"<w:footnote w:id="{id}"><w:p><w:r><w:t>note {id}</w:t></w:r></w:p></w:footnote>"#)).collect::<String>();
    let footnotes = format!(r#"<w:footnotes {W}>{many}</w:footnotes>"#);
    let refs = (2..6).map(|id| format!(r#"<w:r><w:footnoteReference w:id="{id}"/></w:r>"#)).collect::<String>();
    let doc = format!(r#"<w:document {W}><w:body><w:p>{refs}</w:p></w:body></w:document>"#);
    let i = import(&docx(&doc, "", &footnotes)).unwrap();
    assert_eq!(i.story.notes.len(), 4);
    assert!(i.story.notes.iter().all(|n| n.text.text.contains("note")), "each distinct note keeps its text");
    assert!(!i.warnings.iter().any(|w| w.contains("referenced more than once")), "{:?}", i.warnings);
}

/// A footnote's table keeps its cell text in stories of its own, so the budget counted a
/// table of any size as the couple of bytes of its anchor and the cap did not hold (noted in
/// review of #1584). Eighteen notes of ~600 KB of cell text each are 10.8 MB of table text
/// against an 8 MB budget.
#[test]
fn a_footnote_table_counts_toward_the_note_budget() {
    let big = "x".repeat(600_000);
    let cell = format!(r#"<w:tbl><w:tr><w:tc><w:p><w:r><w:t>{big}</w:t></w:r></w:p></w:tc></w:tr></w:tbl>"#);
    let notes: String = (2..20).map(|id| format!(r#"<w:footnote w:id="{id}">{cell}</w:footnote>"#)).collect();
    let footnotes = format!(r#"<w:footnotes {W}>{notes}</w:footnotes>"#);
    let refs: String = (2..20).map(|id| format!(r#"<w:r><w:footnoteReference w:id="{id}"/></w:r>"#)).collect();
    let doc = format!(r#"<w:document {W}><w:body><w:p>{refs}</w:p></w:body></w:document>"#);
    let i = import(&docx(&doc, "", &footnotes)).unwrap();

    // Every reference is still a mark in the text.
    assert_eq!(i.story.notes.len(), 18, "all 18 references are kept");
    // What was retained is the budget plus at most the one note that was already being built
    // when it ran out -- not all 10.8 MB.
    let held: usize = i.story.notes.iter().map(|n| note_bytes(&n.text, MAX_NOTE_DEPTH)).sum();
    assert!(held <= (8 << 20) + big.len() + 1024, "the cell text is inside the budget: {held} bytes");
    assert!(held > big.len(), "and the notes that fit were imported in full: {held} bytes");
    assert!(i.warnings.iter().any(|w| w.contains("longer than 8 MB")), "the budget warning is reported: {:?}", i.warnings);
}

/// The report is one line per distinct footnote id, and the file chooses how many ids it
/// has: 400 ids referenced twice each meant 400 warnings, every one of them pushed after a
/// `Vec::contains` scan of all the earlier ones (noted in review of #1584). Both the report
/// and the set behind it stop at the cap.
#[test]
fn the_report_does_not_grow_with_the_document() {
    let ids = 2..402;
    // Small notes, so the byte budget is not what stops this -- the ids are.
    let notes: String = ids.clone().map(|id| format!(r#"<w:footnote w:id="{id}"><w:p><w:r><w:t>note {id}</w:t></w:r></w:p></w:footnote>"#)).collect();
    let footnotes = format!(r#"<w:footnotes {W}>{notes}</w:footnotes>"#);
    let refs: String = ids
        .clone()
        .flat_map(|id| {
            let one = format!(r#"<w:r><w:footnoteReference w:id="{id}"/></w:r>"#);
            [one.clone(), one]
        })
        .collect();
    let doc = format!(r#"<w:document {W}><w:body><w:p>{refs}</w:p></w:body></w:document>"#);
    let i = import(&docx(&doc, "", &footnotes)).unwrap();

    // The document is imported in full: every reference is a mark in the text.
    assert_eq!(i.story.notes.len(), 800, "all 800 references are kept");
    let duplicates = i.warnings.iter().filter(|w| w.contains("referenced more than once")).count();
    assert!(duplicates >= 1, "the repeats are reported: {:?}", i.warnings);
    assert!(i.warnings.len() <= MAX_WARNINGS + 1, "400 duplicated ids do not make 400 warnings: {} warnings", i.warnings.len());
    // And the last line says the list was cut, rather than ending without explanation.
    assert!(i.warnings.iter().any(|w| w.contains("the rest are not listed")), "{:?}", i.warnings);
}

/// When the budget runs out before a note's *first* reference, that reference gets an empty
/// note -- and must not be recorded as the one that kept the text, or a later reference to the
/// same id would be told the text is at a reference that has none (noted in review of #1584).
#[test]
fn a_note_whose_first_reference_is_past_the_budget_is_not_recorded_as_kept() {
    // Notes totalling more than the 8 MB budget, so the later ids are never built.
    let big = "x".repeat(600_000);
    let notes: String = (2..20).map(|id| format!(r#"<w:footnote w:id="{id}"><w:p><w:r><w:t>{big}</w:t></w:r></w:p></w:footnote>"#)).collect();
    let footnotes = format!(r#"<w:footnotes {W}>{notes}</w:footnotes>"#);
    // Each id referenced twice, in order.
    let refs: String = (2..20)
        .flat_map(|id| {
            let one = format!(r#"<w:r><w:footnoteReference w:id="{id}"/></w:r>"#);
            [one.clone(), one]
        })
        .collect();
    let doc = format!(r#"<w:document {W}><w:body><w:p>{refs}</w:p></w:body></w:document>"#);
    let i = import(&docx(&doc, "", &footnotes)).unwrap();

    // Every reference is kept, and the budget is respected.
    assert_eq!(i.story.notes.len(), 36, "all 36 references are in the text");
    let total: usize = i.story.notes.iter().map(|n| n.text.text.len()).sum();
    assert!(total <= (8 << 20) + big.len(), "the note text stays inside the budget: {total} bytes");

    // The "referenced more than once" warning is only about ids whose text was actually kept.
    // A note that was never built cannot have kept its text at a first reference.
    let kept: std::collections::HashSet<&str> = i.story.notes.iter().filter(|n| !n.text.text.is_empty()).map(|n| n.text.text.as_str()).collect();
    if i.warnings.iter().any(|w| w.contains("referenced more than once")) {
        assert!(!kept.is_empty(), "that warning implies some text was kept: {:?}", i.warnings);
    }
    assert!(i.warnings.iter().any(|w| w.contains("longer than 8 MB")), "the budget warning is the one this document earns: {:?}", i.warnings);
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

/// A content control inside a paragraph (`w:sdt`) keeps its text, also around a hyperlink.
#[test]
fn inline_content_control_text_is_kept() {
    let document = format!(
        r#"<w:document {W}><w:body>
              <w:p><w:r><w:t xml:space="preserve">Date: </w:t></w:r><w:sdt><w:sdtPr><w:alias w:val="d"/></w:sdtPr><w:sdtContent><w:r><w:t>1 May 2026</w:t></w:r></w:sdtContent></w:sdt><w:r><w:t>.</w:t></w:r></w:p>
              <w:p><w:sdt><w:sdtContent><w:hyperlink><w:r><w:t>Link</w:t></w:r></w:hyperlink></w:sdtContent></w:sdt></w:p>
            </w:body></w:document>"#
    );
    let st = import(&docx(&document, "", "")).unwrap().story;
    st.check().unwrap();
    assert_eq!(st.text, "Date: 1 May 2026.\nLink");
}

/// `str::parse::<f64>` accepts "NaN"/"inf"; sizes, spacing and indents must ignore them.
#[test]
fn non_finite_sizes_spacing_and_indents_are_ignored() {
    let document = format!(
        r#"<w:document {W}><w:body>
              <w:p><w:pPr><w:spacing w:before="NaN" w:after="inf"/><w:ind w:left="-inf" w:right="NaN" w:firstLine="-inf"/></w:pPr>
                <w:r><w:rPr><w:sz w:val="NaN"/></w:rPr><w:t>a</w:t></w:r><w:r><w:rPr><w:sz w:val="inf"/></w:rPr><w:t>b</w:t></w:r></w:p>
              <w:p><w:pPr><w:ind w:hanging="inf"/></w:pPr><w:r><w:t>c</w:t></w:r></w:p>
            </w:body></w:document>"#
    );
    let st = import(&docx(&document, "", "")).unwrap().story;
    let fin = |v: Option<f64>| v.is_none_or(f64::is_finite);
    for p in &st.paras {
        let p = &p.para;
        assert!(fin(p.space_before) && fin(p.space_after), "{p:?}");
        assert!(fin(p.left_indent) && fin(p.right_indent) && fin(p.first_line_indent), "{p:?}");
    }
    for at in [0, 1] {
        assert!(fin(st.format_after(at).over.size), "{:?}", st.format_after(at).over.size);
    }
}

#[test]
fn out_of_range_sizes_are_dropped() {
    let document = format!(
        r#"<w:document {W}><w:body>
              <w:p><w:r><w:rPr><w:sz w:val="0"/></w:rPr><w:t>a</w:t></w:r><w:r><w:rPr><w:sz w:val="99999999"/></w:rPr><w:t>b</w:t></w:r></w:p>
            </w:body></w:document>"#
    );
    let st = import(&docx(&document, "", "")).unwrap().story;
    for at in [0, 1] {
        assert_eq!(st.format_after(at).over.size, None, "run {at}");
    }
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

/// The label composition gives paragraph `i`: numbered paragraphs count per level on one
/// counter that a paragraph outside a list restarts; bullets and empty paragraphs leave it.
fn label(st: &Story, i: usize) -> String {
    let mut counter = designcraft_doc::ListCounter::default();
    let mut label = String::new();
    for (f, r) in st.paras.iter().zip(st.para_ranges()).take(i + 1) {
        let mut p = designcraft_doc::ParaProps::default();
        p.apply(&f.para);
        label = match p.list_type {
            _ if r.is_empty() => String::new(),
            ListType::Numbers => {
                counter.advance(&p);
                p.number_label_in(&counter)
            }
            ListType::Bullets => p.bullet_label(),
            ListType::None => {
                counter = designcraft_doc::ListCounter::default();
                String::new()
            }
        };
    }
    label
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
    assert_eq!(label(&st, 1), "•\t");
    assert_eq!(st.paras[3].para.number_style, Some(designcraft_doc::NumberStyle::Arabic));
    assert_eq!((label(&st, 3), label(&st, 4)), ("1.\t".to_string(), "2.\t".to_string()));
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
    // Word's numbers: 1, a, b, 1.b.I, 2, –, 3, 1 (own counter), 4 (shares numId 1's). The
    // model numbers each level on its own, so only the paragraphs after the plain one need
    // Start At.
    assert_eq!(start, [None, None, None, None, None, None, Some(Some(3)), Some(Some(1)), Some(Some(4))]);
    let levels: Vec<Option<u32>> = st.paras.iter().map(|p| p.para.list_level).collect();
    assert_eq!(levels, [Some(1), Some(2), Some(2), Some(3), Some(1), None, Some(1), Some(1), Some(1)]);
    assert_eq!(label(&st, 1), "a) ");
    // Parent numbers are the model's level references.
    assert_eq!(st.paras[3].para.number_expression.as_deref(), Some("^1.^2.^#^t"));
    assert_eq!(label(&st, 3), "1.b.I\t");
    let labels: Vec<String> = (0..st.paras.len()).map(|i| label(&st, i)).collect();
    assert_eq!(labels, ["1.\t", "a) ", "b) ", "1.b.I\t", "2.\t", "", "3.\t", "1.\t", "4.\t"]);
    assert_eq!(st.paras[1].para.left_indent, Some(72.0));
    assert_eq!(st.paras[1].para.first_line_indent, Some(-18.0));
}

/// Parent numbers the model's levels can't give are written as text: a list that starts at
/// level 1 (level 0 never numbered), and a legal-style label (decimal parents).
#[test]
fn parent_numbers_the_model_lacks_are_text() {
    let numbering = format!(
        r#"<w:numbering {W}>
              <w:abstractNum w:abstractNumId="1">
                <w:lvl w:ilvl="0"><w:start w:val="1"/><w:numFmt w:val="upperRoman"/><w:lvlText w:val="%1."/></w:lvl>
                <w:lvl w:ilvl="1"><w:start w:val="1"/><w:numFmt w:val="decimal"/><w:lvlText w:val="%1.%2"/></w:lvl>
                <w:lvl w:ilvl="2"><w:start w:val="1"/><w:isLgl/><w:numFmt w:val="decimal"/><w:lvlText w:val="%1.%2.%3"/></w:lvl>
              </w:abstractNum>
              <w:num w:numId="1"><w:abstractNumId w:val="1"/></w:num>
            </w:numbering>"#
    );
    let body = [list_p("1", Some("1"), "A"), list_p("1", Some("0"), "B"), list_p("1", Some("1"), "C"), list_p("1", Some("2"), "D")];
    let st = lists_docx(&numbering, "", &body).story;
    let labels: Vec<String> = (0..st.paras.len()).map(|i| label(&st, i)).collect();
    assert_eq!(labels, ["I.1\t", "I.\t", "I.1\t", "1.1.1\t"]);
    assert_eq!(st.paras[0].para.number_expression.as_deref(), Some("I.^#^t"));
    assert_eq!(st.paras[2].para.number_expression.as_deref(), Some("^1.^#^t"));
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
    assert_eq!(label(&st, 0), "•\t");
    assert_eq!(label(&st, 1), "◦\t");
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
    assert_eq!(label(&st, 9), "1^\t");
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

/// `ST_OnOff` is a boolean plus the words `on` and `off`: an explicit `off` is off.
#[test]
fn off_valued_toggles_stay_off() {
    let document = format!(
        r#"<w:document {W}><w:body><w:p>
              <w:r><w:rPr><w:b w:val="off"/><w:i w:val="0"/><w:strike w:val="off"/><w:u w:val="off"/><w:caps w:val="off"/></w:rPr><w:t>plain</w:t></w:r>
              <w:r><w:rPr><w:b w:val="on"/></w:rPr><w:t>bold</w:t></w:r>
            </w:p></w:body></w:document>"#
    );
    let st = import(&docx(&document, "", "")).unwrap().story;
    st.check().unwrap();
    let plain = st.format_after(st.text.find("plain").unwrap()).clone().over;
    assert_eq!(plain.font_style.as_deref(), Some("Regular"), "an explicit `off` is not bold");
    assert_eq!(plain.strikethrough, Some(false));
    assert_eq!(plain.underline, Some(false));
    assert_eq!(plain.capitalization, None, "no all-caps");
    let on = st.format_after(st.text.find("bold").unwrap()).clone().over;
    assert_eq!(on.font_style.as_deref(), Some("Bold"), "`on` is still on");
}
