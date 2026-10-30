use crate::{PdfOptions, export_pdf};
use designcraft_compose::Cache;
use designcraft_doc::{Document, ParaFormat, SpreadRef, Table};
use designcraft_geom::Rect;

#[test]
fn tagged_inline_table_has_cell_structure_and_real_text_marked_content() {
    check_table("before after", 7);
}

#[test]
fn tagged_table_only_story_has_merged_header_and_cell_paragraphs() {
    check_table("", 0);
}

#[test]
fn tagged_data_cell_preserves_row_span() {
    let mut doc = Document::new(&Default::default());
    let (_, story_id) = doc
        .add_text_frame(SpreadRef::Doc(0), Rect::new(36.0, 36.0, 576.0, 300.0), doc.default_layer(), "", ParaFormat::default())
        .expect("text frame");
    let story = doc.story_mut(story_id).expect("story");
    story.insert_table(0, Table::new(1, 2, 2, 1, 0, 200.0));
    let table = story.tables.get_mut(&1).expect("table");
    let table = std::sync::Arc::make_mut(table);
    table.merge(designcraft_doc::CellRange::new(1, 0, 2, 0)).expect("merge data cell");
    table.cell_mut(1, 0).expect("owning cell").text.insert(0, "Merged data");

    let pdf = export_pdf(&doc, &Cache::new(), &PdfOptions { tagged: true, compress: false, ..Default::default() }).expect("export");
    let source = String::from_utf8_lossy(&pdf);
    assert!(source.split("endobj").any(|object| object.contains("/S/TD") && object.contains("/RowSpan 2")), "the data cell must retain its row span");
}

#[test]
fn tagged_threaded_table_repeated_headers_and_footers_are_artifacts() {
    let mut doc = Document::new(&Default::default());
    let layer = doc.default_layer();
    let (first, sid) = doc.add_text_frame(SpreadRef::Doc(0), Rect::new(36.0, 36.0, 156.0, 135.0), layer, "", ParaFormat::default()).unwrap();
    let (second, _) = doc.add_text_frame(SpreadRef::Doc(0), Rect::new(36.0, 236.0, 156.0, 335.0), layer, "", ParaFormat::default()).unwrap();
    doc.thread(first, second).unwrap();
    let mut table = Table::new(77, 4, 1, 1, 1, 100.0);
    for row in &mut table.rows {
        row.height = 20.0;
        row.mode = designcraft_doc::RowHeightMode::Exactly;
    }
    for (cell, text) in table.cells.iter_mut().zip(["Header", "Alpha", "Bravo", "Charlie", "Delta", "Footer"]) {
        cell.insets = [0.0; 4];
        cell.text.insert(0, text);
    }
    doc.story_mut(sid).unwrap().insert_table(0, table);
    let composed = designcraft_compose::compose_story(&doc, sid, &Default::default());
    assert!(!composed.is_overset());
    assert_eq!(composed.frames.len(), 2);
    for (index, frame) in composed.frames.iter().enumerate() {
        let fragment = &frame.tables[0];
        assert_eq!(fragment.cell(0, 0).unwrap().repeated, index == 1);
        assert_eq!(fragment.cell(5, 0).unwrap().repeated, index == 0);
    }
    let pdf = export_pdf(&doc, &Cache::new(), &PdfOptions { tagged: true, compress: false, ..Default::default() }).unwrap();
    let source = String::from_utf8_lossy(&pdf);
    assert_eq!(source.matches("/S/Table").count(), 1);
    assert_eq!(source.matches("/S/TR").count(), 6, "each source row belongs to the same table once");
    assert_eq!(source.matches("/S/TH").count(), 1);
    assert_eq!(source.matches("/S/TD").count(), 5);
    assert_eq!(source.matches("/MCID").count(), 6, "only the original header/footer and four body cells are marked content");
    let objects: std::collections::HashMap<usize, &str> = source
        .split("endobj")
        .filter_map(|object| {
            let (prefix, body) = object.rsplit_once(" 0 obj")?;
            Some((prefix.split_whitespace().last()?.parse().ok()?, body))
        })
        .collect();
    let header = *objects.iter().find(|(_, body)| body.contains("/S/TH")).unwrap().0;
    let header_paragraphs: Vec<_> = objects
        .values()
        .filter(|body| {
            body.contains("/S/P")
                && body.split_once("/P ").and_then(|(_, rest)| rest.split_whitespace().next()?.parse::<usize>().ok()) == Some(header)
        })
        .collect();
    assert_eq!(header_paragraphs.len(), 1, "the header has one paragraph");
    let marked = header_paragraphs[0].split_once("/K[").unwrap().1.split_once(']').unwrap().0;
    assert_eq!(marked.split_whitespace().count(), 1, "the TH paragraph references only the original header run");
    let extracted = crate::tests::extract_text(&pdf).concat();
    assert_eq!(extracted.matches("Header").count(), 2, "repeated header remains real text");
    assert_eq!(extracted.matches("Footer").count(), 2, "repeated footer remains real text");
    for text in ["Alpha", "Bravo", "Charlie", "Delta"] {
        assert_eq!(extracted.matches(text).count(), 1, "each body row is painted once");
    }
}

fn check_table(text: &str, anchor: usize) {
    let mut doc = Document::new(&Default::default());
    let (_, story_id) = doc
        .add_text_frame(SpreadRef::Doc(0), Rect::new(36.0, 36.0, 576.0, 300.0), doc.default_layer(), text, ParaFormat::default())
        .expect("text frame");
    let story = doc.story_mut(story_id).expect("story");
    story.insert_table(anchor, Table::new(1, 2, 2, 1, 0, 200.0));
    let table = story.tables.get_mut(&1).expect("table");
    let table = std::sync::Arc::make_mut(table);
    table.merge(designcraft_doc::CellRange::new(0, 0, 0, 1)).expect("merge header");
    for (index, text) in ["Header", "", "Second\nParagraph", "42", "Last", "Cell"].into_iter().enumerate() {
        if let Some(cell) = table.cells.get_mut(index) {
            cell.text.insert(0, text);
        }
    }
    let pdf = export_pdf(&doc, &Cache::new(), &PdfOptions { tagged: true, compress: false, ..Default::default() }).expect("export");
    let source = String::from_utf8_lossy(&pdf);
    let objects: std::collections::HashMap<usize, &str> = source
        .split("endobj")
        .filter_map(|object| {
            let (prefix, body) = object.rsplit_once(" 0 obj")?;
            Some((prefix.split_whitespace().last()?.parse().ok()?, body))
        })
        .collect();
    let parent = |body: &str| body.split_once("/P ").and_then(|(_, rest)| rest.split_whitespace().next()?.parse::<usize>().ok());
    let table_id = *objects.iter().find(|(_, body)| body.contains("/S/Table")).expect("table object").0;
    if !text.is_empty() {
        let document_id = parent(objects[&table_id]).expect("table parent");
        let document = objects[&document_id];
        let children = document.split_once("/K[").expect("document children").1.split_once(']').expect("children array").0;
        let child_ids: Vec<usize> =
            children.split_whitespace().collect::<Vec<_>>().chunks(3).map(|reference| reference[0].parse().expect("child reference")).collect();
        assert_eq!(child_ids.len(), 3, "body paragraphs and table must be siblings");
        assert_eq!(child_ids[1], table_id, "table must be between preceding and following paragraphs");
        assert!(objects[&child_ids[0]].contains("/S/P") && objects[&child_ids[2]].contains("/S/P"));
    }
    let rows: Vec<_> = objects.iter().filter(|(_, body)| body.contains("/S/TR") && parent(body) == Some(table_id)).map(|(id, _)| *id).collect();
    assert_eq!(rows.len(), 3, "rows must be children of the table");
    let cells: Vec<_> = objects
        .iter()
        .filter(|(_, body)| (body.contains("/S/TH") || body.contains("/S/TD")) && parent(body).is_some_and(|id| rows.contains(&id)))
        .map(|(id, _)| *id)
        .collect();
    assert_eq!(cells.len(), 5, "only owning cells must be children of rows");
    let header = objects.values().find(|body| body.contains("/S/TH")).expect("merged header cell");
    assert!(header.contains("/ColSpan 2"), "the owning header cell must retain its column span: {header}");
    assert!(!source.contains("/RowSpan 1") && !source.contains("/ColSpan 1"), "unit spans are implicit");
    let paragraphs: Vec<_> = objects.values().filter(|body| body.contains("/S/P") && parent(body).is_some_and(|id| cells.contains(&id))).collect();
    assert_eq!(paragraphs.len(), 6, "cell paragraphs must be children of cells, not the document");
    for paragraph in paragraphs {
        assert!(paragraph.contains("/K") && paragraph.contains("/Pg"), "cell paragraph must reference page content");
    }

    assert!(source.contains("/Table"), "table structure element missing");
    assert!(source.contains("/TR"), "row structure element missing");
    assert!(source.contains("/TH") && source.contains("/TD"), "header/data cell structure elements missing");
    assert!(source.matches("/MCID").count() >= 6, "cell paragraphs must have marked-content identifiers");
    assert_eq!(source.matches("/S/TH").count(), 1, "covered header cell must not be tagged twice");
    let extracted = crate::tests::extract_text(&pdf).into_iter().collect::<String>();
    for content in ["Header", "Second", "Paragraph", "42", "Last", "Cell"] {
        assert!(extracted.contains(content), "cell content {content:?} must remain real searchable text");
    }
    let table_position = source.find("/Table").expect("table position");
    if !text.is_empty() {
        assert!(source[..table_position].contains("/P"), "preceding story paragraph must precede the table");
    }
    assert!(source[table_position..].contains("/P"), "table cells and following story paragraph must retain paragraph structure");
}
