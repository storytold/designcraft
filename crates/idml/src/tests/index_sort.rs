//! Original synthetic fixtures: no Adobe-generated files or implementation details.
//! Behavioral reference: https://developer.adobe.com/indesign/uxp/dom/api/t/topic/

use std::io::Read;

use designcraft_doc::IndexRef;
use designcraft_doc::index::{self, IndexRange, IndexSpec, Resolved};

use super::*;
use crate::xml::El;

fn strings(values: &[&str]) -> Vec<String> {
    values.iter().map(|v| (*v).to_string()).collect()
}

fn reference(topics: &[&str], sort: &[&str], range: IndexRange) -> IndexRef {
    IndexRef { topics: strings(topics), sort: strings(sort), range }
}

fn references(doc: &Document) -> Vec<IndexRef> {
    doc.stories.values().flat_map(|s| s.index_refs.iter().map(|r| r.as_ref().clone())).collect()
}

fn native(refs: Vec<IndexRef>) -> Document {
    let mut doc = Document::new(&NewDocument::default());
    let layer = doc.default_layer();
    let (_, sid) = doc.add_text_frame(SpreadRef::Doc(0), Rect::new(36.0, 36.0, 400.0, 400.0), layer, "Source", ParaFormat::default()).unwrap();
    let story = doc.story_mut(sid).unwrap();
    for r in refs {
        story.insert_index_ref(story.len(), r);
    }
    doc.check().unwrap();
    doc
}

fn import_xml(index: &str, refs: &str) -> Document {
    let map = format!(
        r#"<Document xmlns:idPkg="http://ns.adobe.com/AdobeInDesign/idml/1.0/packaging">
          {index}
          <Layer Self="L" Name="Art"/>
          <idPkg:Story src="Stories/s.xml"/>
          <idPkg:Spread src="Spreads/s.xml"/>
        </Document>"#
    );
    let story = format!(
        r#"<idPkg:Story xmlns:idPkg="http://ns.adobe.com/AdobeInDesign/idml/1.0/packaging">
          <Story Self="s"><ParagraphStyleRange><CharacterStyleRange>
            <Content>Source</Content>{refs}
          </CharacterStyleRange></ParagraphStyleRange></Story>
        </idPkg:Story>"#
    );
    let spread = r#"<idPkg:Spread xmlns:idPkg="http://ns.adobe.com/AdobeInDesign/idml/1.0/packaging">
      <Spread Self="sp"><Page Self="p" GeometricBounds="0 0 240 240"/>
        <TextFrame Self="frame" ParentStory="s" ItemLayer="L">
          <Properties><PathGeometry><GeometryPathType><PathPointArray>
            <PathPointType Anchor="0 0"/><PathPointType Anchor="200 0"/>
            <PathPointType Anchor="200 200"/><PathPointType Anchor="0 200"/>
          </PathPointArray></GeometryPathType></PathGeometry></Properties>
        </TextFrame>
      </Spread>
    </idPkg:Spread>"#;
    import_idml(&zip_files(&[("designmap.xml", &map), ("Stories/s.xml", &story), ("Spreads/s.xml", spread)])).unwrap()
}

/// Inspect the exported XML directly, without calling the IDML importer.
fn exported_index(doc: &Document) -> El {
    let bytes = export_idml(doc);
    let mut zip = zip::ZipArchive::new(std::io::Cursor::new(bytes)).unwrap();
    let mut map = String::new();
    zip.by_name("designmap.xml").unwrap().read_to_string(&mut map).unwrap();
    crate::xml::parse(map.as_bytes()).unwrap().find("Index").unwrap().clone()
}

fn topic<'a>(index: &'a El, path: &[&str]) -> &'a El {
    let mut el = index;
    for name in path {
        el = el.find_all("Topic").find(|t| t.get("Name") == Some(*name)).unwrap();
    }
    el
}

#[test]
fn imports_synthetic_sort_keys_for_nested_topics_and_cross_references() {
    let doc = import_xml(
        r#"<Index Self="ix">
          <Topic Self="z" Name="Zulu" SortOrder="Alpha"/>
          <Topic Self="b" Name="Beta"/>
          <Topic Self="empty" Name="Empty" SortOrder=""/>
          <Topic Self="parent" Name="Parent" SortOrder="">
            <Topic Self="child" Name="É &amp; &lt;leaf&gt;" SortOrder=" A&amp;&lt;é&gt; ">
              <CrossReference Self="see" ReferencedTopic="b" CrossReferenceType="See"/>
              <CrossReference Self="also" ReferencedTopic="z" CrossReferenceType="SeeAlso"/>
            </Topic>
          </Topic>
          <Topic Self="zoo" Name="Zoo" SortOrder="Aardvark">
            <Topic Self="leaf" Name="Leaf"/>
          </Topic>
        </Index>"#,
        r#"<PageReference Self="r1" ReferencedTopic="z" PageReferenceType="CurrentPage"/>
          <PageReference Self="r2" ReferencedTopic="z" PageReferenceType="ForNextNParagraphs" PageReferenceLimit="4"/>
          <PageReference Self="r3" ReferencedTopic="b" PageReferenceType="CurrentPage"/>
          <PageReference Self="r4" ReferencedTopic="empty" PageReferenceType="SuppressPageNumbers"/>
          <PageReference Self="r5" ReferencedTopic="child" PageReferenceType="CurrentPage"/>
          <PageReference Self="r6" ReferencedTopic="leaf" PageReferenceType="CurrentPage"/>"#,
    );
    doc.check().unwrap();
    let refs = references(&doc);
    assert_eq!(refs.len(), 8);
    for r in [
        reference(&["Zulu"], &["Alpha"], IndexRange::CurrentPage),
        reference(&["Zulu"], &["Alpha"], IndexRange::NextParagraphs(4)),
        reference(&["Beta"], &[], IndexRange::CurrentPage),
        reference(&["Empty"], &[], IndexRange::SuppressPageRange),
        reference(&["Parent", "É & <leaf>"], &["", " A&<é> "], IndexRange::CurrentPage),
        reference(&["Parent", "É & <leaf>"], &["", " A&<é> "], IndexRange::See("Beta".into())),
        reference(&["Parent", "É & <leaf>"], &["", " A&<é> "], IndexRange::SeeAlso("Zulu".into())),
        reference(&["Zoo", "Leaf"], &["Aardvark"], IndexRange::CurrentPage),
    ] {
        assert!(refs.contains(&r), "missing {r:?} in {refs:?}");
    }
    let story = doc.stories.values().next().unwrap();
    assert_eq!(story.text.replace(designcraft_doc::INDEX_MARK, ""), "Source");
}

#[test]
fn exports_sort_keys_as_topic_attributes_without_changing_names() {
    let doc = native(vec![
        reference(&["Zulu"], &["Alpha"], IndexRange::CurrentPage),
        reference(&["Zulu"], &["Alpha"], IndexRange::NextParagraphs(2)),
        reference(&["Beta"], &[], IndexRange::CurrentPage),
        reference(&["Empty"], &[""], IndexRange::CurrentPage),
        reference(&["Parent", "É & <leaf>"], &["", " A&<é> "], IndexRange::CurrentPage),
        reference(&["Alias"], &["Able"], IndexRange::See("Zulu".into())),
        reference(&["Related"], &["Azure"], IndexRange::SeeAlso("Zulu".into())),
        reference(&["Parent", "Alias"], &["", "Able"], IndexRange::See("Zulu".into())),
        reference(&["Parent", "Related"], &["", "Azure"], IndexRange::SeeAlso("Zulu".into())),
    ]);
    let index = exported_index(&doc);
    assert_eq!(index.find_all("Topic").count(), 6, "repeated references and cross-reference targets share their topic");
    assert_eq!(topic(&index, &["Zulu"]).get("SortOrder"), Some("Alpha"));
    assert_eq!(topic(&index, &["Beta"]).get("SortOrder"), Some(""));
    assert_eq!(topic(&index, &["Empty"]).get("SortOrder"), Some(""));
    assert_eq!(topic(&index, &["Parent"]).get("SortOrder"), Some(""));
    assert_eq!(topic(&index, &["Parent", "É & <leaf>"]).get("SortOrder"), Some(" A&<é> "));
    for (name, key, kind) in [("Alias", "Able", "See"), ("Related", "Azure", "SeeAlso")] {
        for path in [&[name][..], &["Parent", name][..]] {
            let t = topic(&index, path);
            assert_eq!(t.get("SortOrder"), Some(key));
            let cross_ref = t.find("CrossReference").unwrap();
            assert_eq!(cross_ref.get("CrossReferenceType"), Some(kind));
            assert_eq!(cross_ref.get("ReferencedTopic"), topic(&index, &["Zulu"]).get("Self"));
        }
    }
}

/// These references all belong to the same page. Fix the page resolution to isolate index
/// ordering from fonts/composition, and use the real index generator for its text and headings.
fn generated_index(doc: &Document) -> Vec<(String, String)> {
    let refs: Vec<_> = references(doc).into_iter().map(|r| Resolved { r, pages: Some((0, 0)) }).collect();
    index::build(&IndexSpec { title: String::new(), ..Default::default() }, &refs, &|p| (p + 1).to_string())
}

#[test]
fn round_trip_preserves_generated_index_order_and_headings() {
    let doc = native(vec![
        reference(&["Zulu"], &["Alpha"], IndexRange::CurrentPage),
        reference(&["Zulu"], &["Alpha"], IndexRange::CurrentPage),
        reference(&["Beta"], &[], IndexRange::CurrentPage),
        reference(&["Parent", "Zulu"], &["", "Alpha"], IndexRange::CurrentPage),
        reference(&["Parent", "Beta"], &[], IndexRange::CurrentPage),
        reference(&["Alias"], &["Able"], IndexRange::See("Zulu".into())),
        reference(&["Related"], &["Delta"], IndexRange::SeeAlso("Beta".into())),
        reference(&["Parent", "Alias"], &["", "Able"], IndexRange::See("Zulu".into())),
        reference(&["Parent", "Related"], &["", "Azure"], IndexRange::SeeAlso("Beta".into())),
    ]);
    let before = generated_index(&doc);
    let expected: Vec<_> = [
        ("Index Section Head", "A"),
        ("Index Level 1", "Alias  See Zulu"),
        ("Index Level 1", "Zulu  1"),
        ("Index Section Head", "B"),
        ("Index Level 1", "Beta  1"),
        ("Index Section Head", "D"),
        ("Index Level 1", "Related  1. See also Beta"),
        ("Index Section Head", "P"),
        ("Index Level 1", "Parent"),
        ("Index Level 2", "Alias  See Zulu"),
        ("Index Level 2", "Zulu  1"),
        ("Index Level 2", "Related  1. See also Beta"),
        ("Index Level 2", "Beta  1"),
    ]
    .into_iter()
    .map(|(style, text)| (style.to_string(), text.to_string()))
    .collect();
    assert_eq!(before, expected);
    let back = import_idml(&export_idml(&doc)).unwrap();
    back.check().unwrap();
    assert_eq!(generated_index(&back), before);
    assert_eq!(back.stories.values().next().unwrap().text.replace(designcraft_doc::INDEX_MARK, ""), "Source");
}

#[test]
fn conflicting_sort_keys_keep_the_first_serialized_value_including_defaults() {
    fn check(first: &[&str], second: &[&str], expected: &str) {
        let doc = native(vec![reference(&["Shared"], first, IndexRange::CurrentPage), reference(&["Shared"], second, IndexRange::CurrentPage)]);
        let index = exported_index(&doc);
        assert_eq!(index.find_all("Topic").count(), 1, "conflicts do not manufacture topics");
        assert_eq!(topic(&index, &["Shared"]).get("SortOrder"), Some(expected));
        let back = import_idml(&export_idml(&doc)).unwrap();
        let refs = references(&back);
        assert_eq!(refs.len(), 2);
        for r in refs {
            assert_eq!(r.sort.first().map_or("", String::as_str), expected);
        }
        assert_eq!(references(&doc)[0].sort, strings(first), "export does not edit native references");
        assert_eq!(references(&doc)[1].sort, strings(second));
    }
    check(&["Alpha"], &["Omega"], "Alpha");
    check(&["Omega"], &["Alpha"], "Omega");
    check(&[], &["Alpha"], "");
    check(&[""], &["Alpha"], "");
    check(&["Alpha"], &[], "Alpha");
    check(&["Alpha"], &[""], "Alpha");
}

#[test]
fn shared_ancestors_and_identical_leaf_names_keep_keys_by_full_path() {
    let doc = native(vec![
        reference(&["One", "Zulu"], &["Alpha", "First"], IndexRange::CurrentPage),
        reference(&["One", "Zulu"], &["Beta", "Second"], IndexRange::CurrentPage),
        reference(&["One", "Beta"], &["Gamma", "Third"], IndexRange::CurrentPage),
        reference(&["Two", "Zulu"], &["Delta", "Fourth"], IndexRange::CurrentPage),
    ]);
    let index = exported_index(&doc);
    assert_eq!(topic(&index, &["One"]).get("SortOrder"), Some("Alpha"));
    assert_eq!(topic(&index, &["One", "Zulu"]).get("SortOrder"), Some("First"));
    assert_eq!(topic(&index, &["One", "Beta"]).get("SortOrder"), Some("Third"));
    assert_eq!(topic(&index, &["Two"]).get("SortOrder"), Some("Delta"));
    assert_eq!(topic(&index, &["Two", "Zulu"]).get("SortOrder"), Some("Fourth"));
}
