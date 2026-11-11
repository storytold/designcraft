use std::io::Write;

use designcraft_doc::Story;
use designcraft_textimport::import;

const W: &str = r#"xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main""#;

fn docx(document: &str) -> Vec<u8> {
    let mut w = zip::ZipWriter::new(std::io::Cursor::new(Vec::new()));
    w.start_file("word/document.xml", zip::write::SimpleFileOptions::default()).unwrap();
    w.write_all(document.as_bytes()).unwrap();
    w.finish().unwrap().into_inner()
}

fn style_at(s: &Story, pos: usize) -> Option<String> {
    s.char_format_of(pos).over.font_style.clone()
}

/// Bold and italic runs keep their formatting inside table cells, as they do in body text.
#[test]
fn character_formatting_survives_in_table_cells() {
    let doc = format!(
        r#"<w:document {W}><w:body>
          <w:p><w:r><w:rPr><w:b/></w:rPr><w:t>Body</w:t></w:r></w:p>
          <w:tbl>
            <w:tr>
              <w:tc><w:p><w:r><w:rPr><w:b/></w:rPr><w:t>Bold zebra</w:t></w:r></w:p></w:tc>
              <w:tc><w:p><w:r><w:rPr><w:i/></w:rPr><w:t>Italic heron</w:t></w:r></w:p></w:tc>
            </w:tr>
            <w:tr>
              <w:tc>
                <w:p><w:r><w:t>plain</w:t></w:r><w:r><w:rPr><w:b/><w:i/></w:rPr><w:t>both</w:t></w:r></w:p>
                <w:p><w:r><w:rPr><w:i/></w:rPr><w:t>second</w:t></w:r></w:p>
              </w:tc>
              <w:tc><w:p><w:r><w:t>x</w:t></w:r></w:p></w:tc>
            </w:tr>
          </w:tbl>
        </w:body></w:document>"#
    );
    let imported = import("cells.docx", &docx(&doc)).unwrap();
    let st = &imported.story;
    st.check().unwrap();
    assert_eq!(style_at(st, 0).as_deref(), Some("Bold"), "body text");

    let table = st.tables.values().next().expect("a table");
    table.check().unwrap();
    let cell = |r: usize, c: usize| &table.cell(r, c).unwrap().text;

    let zebra = cell(0, 0);
    assert_eq!(zebra.text, "Bold zebra");
    assert_eq!(style_at(zebra, 0).as_deref(), Some("Bold"));
    assert_eq!(style_at(zebra, 9).as_deref(), Some("Bold"));

    let heron = cell(0, 1);
    assert_eq!(heron.text, "Italic heron");
    assert_eq!(style_at(heron, 0).as_deref(), Some("Italic"));

    let mixed = cell(1, 0);
    assert_eq!(mixed.text, "plainboth\nsecond");
    assert_eq!(style_at(mixed, 0), None, "plain run stays plain");
    assert_eq!(style_at(mixed, 5).as_deref(), Some("Bold Italic"));
    assert_eq!(style_at(mixed, 10).as_deref(), Some("Italic"));

    assert_eq!(style_at(cell(1, 1), 0), None);
}
