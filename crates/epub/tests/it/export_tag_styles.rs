use std::io::{Cursor, Read};

use designcraft_doc::build::NewDocument;
use designcraft_doc::geom::Rect;
use designcraft_doc::{Align, CharAttrs, CharacterStyle, Document, ExportTag, ParaAttrs, ParaFormat, ParagraphStyle, SpreadRef, story};
use designcraft_epub::{EpubOptions, HtmlOptions, export_epub, export_html};

fn fixture() -> Document {
    let mut doc = Document::new(&NewDocument::default());
    let styles = doc.styles_mut();
    styles.paragraph.iter_mut().find(|p| p.name == story::BASIC_PARAGRAPH).unwrap().chars.size = Some(12.0);
    styles.paragraph.push(ParagraphStyle {
        name: "Chapter Title".into(),
        based_on: None,
        next_style: None,
        para: ParaAttrs { align: Some(Align::Center), ..Default::default() },
        chars: CharAttrs { size: Some(24.0), ..Default::default() },
        shortcut: String::new(),
    });
    styles.character.push(CharacterStyle {
        name: "Key Term".into(),
        based_on: None,
        chars: CharAttrs { size: Some(18.0), ..Default::default() },
        shortcut: String::new(),
    });
    let layer = doc.default_layer();
    doc.add_text_frame(
        SpreadRef::Doc(0),
        Rect::new(36.0, 36.0, 300.0, 100.0),
        layer,
        "Heading",
        ParaFormat { style: "Chapter Title".into(), ..Default::default() },
    )
    .unwrap();
    let (_, sid) = doc.add_text_frame(SpreadRef::Doc(0), Rect::new(36.0, 120.0, 300.0, 200.0), layer, "Accent plain", ParaFormat::default()).unwrap();
    doc.story_mut(sid).unwrap().format_chars(0..6, |f| f.style = "Key Term".into());
    doc
}

fn map(doc: &mut Document, paragraph_tag: &str, paragraph_class: &str, character_tag: &str, character_class: &str) {
    let tags = &mut doc.styles_mut().export_tags;
    tags.insert("p:Chapter Title".into(), ExportTag { tag: paragraph_tag.into(), class: paragraph_class.into(), ..Default::default() });
    tags.insert("c:Key Term".into(), ExportTag { tag: character_tag.into(), class: character_class.into(), ..Default::default() });
}

fn package(doc: &Document) -> (String, String) {
    let mut zip = zip::ZipArchive::new(Cursor::new(export_epub(doc, &EpubOptions::default()).unwrap())).unwrap();
    let mut read = |path: &str| {
        let mut text = String::new();
        zip.by_name(path).unwrap().read_to_string(&mut text).unwrap();
        text
    };
    let xhtml = read("OEBPS/content.xhtml");
    let css = read("OEBPS/style.css");
    let opf = read("OEBPS/content.opf");
    assert!(xhtml.contains("<link rel=\"stylesheet\" type=\"text/css\" href=\"style.css\"/>"));
    assert!(opf.contains("href=\"style.css\" media-type=\"text/css\""));
    (without_heading_ids(&xhtml), css)
}

/// The markup without the `id`s headings get as table-of-contents targets (` id="s0-p0"`).
fn without_heading_ids(html: &str) -> String {
    let mut out = String::with_capacity(html.len());
    let mut rest = html;
    while let Some(i) = rest.find(" id=\"s") {
        out.push_str(&rest[..i]);
        let after = &rest[i + 6..];
        rest = after.find('"').map_or("", |j| &after[j + 1..]);
    }
    out.push_str(rest);
    out
}

fn rule<'a>(css: &'a str, selector: &str) -> &'a str {
    css.lines().find_map(|line| line.strip_prefix(&format!("{selector} {{ "))).unwrap_or_else(|| panic!("missing {selector} in {css}"))
}

#[test]
fn mapped_tags_and_classes_keep_their_packaged_style_declarations() {
    let mut doc = fixture();
    map(&mut doc, "h1", "chapter", "em", "term");
    let (xhtml, css) = package(&doc);
    assert!(xhtml.contains("<h1 class=\"chapter\">Heading</h1>"), "{xhtml}");
    assert!(xhtml.contains("<em class=\"term\">Accent</em> plain"), "{xhtml}");
    assert!(rule(&css, "h1.chapter").contains("font-size: 2.000em;"));
    assert!(rule(&css, "h1.chapter").contains("text-align: center;"));
    assert!(rule(&css, "em.term").contains("font-size: 1.500em;"));
    assert!(!css.contains("p.chapter-title {") && !css.contains("span.key-term {"));
    assert!(rule(&css, "p.basic-paragraph").contains("font-size: 1.000em;"));
    // HTML uses the same mapped stylesheet inline, while EPUB links its packaged CSS.
    let html = export_html(&doc, &HtmlOptions::default());
    assert!(html.contains(&css));
    assert!(html.contains("<h1 class=\"chapter\">Heading</h1>"));
}

#[test]
fn tag_only_class_only_and_unsupported_tags_keep_existing_fallbacks() {
    for (pt, pc, ct, cc, paragraph, character) in [
        ("h2", "", "em", "", "h2.chapter-title", "em.key-term"),
        ("", "chapter", "", "term", "p.chapter", "span.term"),
        ("section", "", "h2", "", "p.chapter-title", "span.key-term"),
        ("", "", "", "", "p.chapter-title", "span.key-term"),
    ] {
        let mut doc = fixture();
        map(&mut doc, pt, pc, ct, cc);
        let (xhtml, css) = package(&doc);
        for (selector, text, size) in [(paragraph, "Heading", "2.000em"), (character, "Accent", "1.500em")] {
            let (tag, class) = selector.split_once('.').unwrap();
            assert!(xhtml.contains(&format!("<{tag} class=\"{class}\">{text}</{tag}>")), "{xhtml}");
            assert!(rule(&css, selector).contains(&format!("font-size: {size};")));
        }
    }
    let (xhtml, css) = package(&fixture());
    assert!(xhtml.contains("<p class=\"chapter-title\">Heading</p>"));
    assert!(rule(&css, "p.chapter-title").contains("font-size: 2.000em;"));
    assert!(rule(&css, "span.key-term").contains("font-size: 1.500em;"));
}

#[test]
fn custom_class_tokens_match_without_renaming_them() {
    for (class, attribute, selector) in [
        ("lead wide", "lead wide", ".lead.wide"),
        ("lead\twide\n", "lead\twide\n", ".lead.wide"),
        ("chapter:lead", "chapter:lead", r".chapter\3a lead"),
        ("2nd-edition", "2nd-edition", r".\32 nd-edition"),
        ("-2nd", "-2nd", r".-\32 nd"),
        ("-", "-", r".\2d "),
        ("見出し.café", "見出し.café", r".見出し\2e café"),
        ("edition \"wide\"", "edition &quot;wide&quot;", r".edition.\22 wide\22 "),
    ] {
        let mut doc = fixture();
        map(&mut doc, "h2", class, "em", class);
        let (xhtml, css) = package(&doc);
        assert!(xhtml.contains(&format!("<h2 class=\"{attribute}\">Heading</h2>")), "{xhtml}");
        assert!(xhtml.contains(&format!("<em class=\"{attribute}\">Accent</em>")), "{xhtml}");
        assert!(rule(&css, &format!("h2{selector}")).contains("font-size: 2.000em;"));
        assert!(rule(&css, &format!("em{selector}")).contains("font-size: 1.500em;"));
    }
}

#[test]
fn whitespace_only_custom_classes_do_not_apply_rules_to_every_element() {
    let mut doc = fixture();
    map(&mut doc, "p", " \t", "span", " \n");
    let (xhtml, css) = package(&doc);
    assert!(xhtml.contains("<p class=\" \t\">Heading</p>"));
    assert!(xhtml.contains("<span class=\" \n\">Accent</span>"));
    assert!(!css.lines().any(|line| line.starts_with("p {") || line.starts_with("span {")));
    assert!(!css.contains("font-size: 2.000em;") && !css.contains("font-size: 1.500em;"));
    assert!(rule(&css, "p.basic-paragraph").contains("font-size: 1.000em;"));
}
