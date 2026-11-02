use super::*;
use designcraft_doc::build::NewDocument;
use designcraft_doc::geom::{Affine, Rect, shapes};
use designcraft_doc::{Asset, AssetId, Content, Graphic, Item, ItemId, ParaFormat, Shape, SpreadRef, Story, StoryId, Table};
use std::io::Read;
use std::sync::Arc;

#[test]
fn exports_valid_package_in_reading_order() {
    let mut d = Document::new(&NewDocument { pages: 2, ..Default::default() });
    let lid = d.default_layer();
    d.add_text_frame(SpreadRef::Doc(0), Rect::new(36.0, 400.0, 300.0, 500.0), lid, "Second <b> & co", ParaFormat::default()).unwrap();
    d.add_text_frame(SpreadRef::Doc(0), Rect::new(36.0, 36.0, 300.0, 100.0), lid, "First\nline two", ParaFormat::default()).unwrap();
    let bytes = export_epub(&d, &EpubOptions::default()).unwrap();
    let mut z = zip::ZipArchive::new(std::io::Cursor::new(bytes)).unwrap();
    assert_eq!(z.by_index(0).unwrap().name(), "mimetype");
    let mut html = String::new();
    z.by_name("OEBPS/content.xhtml").unwrap().read_to_string(&mut html).unwrap();
    let a = html.find("First").unwrap();
    let b = html.find("Second &lt;b&gt; &amp; co").unwrap();
    assert!(a < b, "reading order");
    assert!(html.contains("<p class=\"basic-paragraph\">"));
    let mut css = String::new();
    z.by_name("OEBPS/style.css").unwrap().read_to_string(&mut css).unwrap();
    assert!(css.contains("p.basic-paragraph"));
    assert!(z.by_name("OEBPS/nav.xhtml").is_ok());
}

/// #766: anchored graphics are exported where they are anchored (inline in the text, an
/// above-line object as a block of its own), and their images go into the package.
#[test]
fn anchored_objects_are_exported_in_place() {
    let mut d = Document::new(&NewDocument::default());
    let lid = d.default_layer();
    let (_, sid) = d.add_text_frame(SpreadRef::Doc(0), Rect::new(36.0, 36.0, 300.0, 200.0), lid, "AB\nCD", ParaFormat::default()).unwrap();
    let aid = AssetId(d.alloc());
    d.assets.insert(
        aid,
        Arc::new(Asset {
            page: 0,
            id: aid,
            name: "dot.png".into(),
            mime: "image/png".into(),
            link: None,
            data: Arc::new(vec![7]),
            pixels: Some((1, 1)),
            ..Default::default()
        }),
    );
    let graphic = |d: &mut Document, alt: &str| {
        let mut item = Item::new(ItemId(d.alloc()), lid, Shape::Rectangle, shapes::rectangle(Rect::new(0.0, 0.0, 10.0, 10.0)));
        item.alt_text = alt.into();
        item.content = Content::Graphic(Graphic {
            layer_options: None,
            asset: aid,
            size: (1.0, 1.0),
            xf: Affine::IDENTITY,
            auto_fit: Default::default(),
            fit_align: 4,
            crop: [0.0; 4],
            clip: None,
            wrap: Default::default(),
            fill: Default::default(),
        });
        item
    };
    let inline = designcraft_doc::AnchoredObject::new(graphic(&mut d, "inline"), designcraft_doc::AnchorPosition::Inline { y_offset: 0.0 });
    let above = designcraft_doc::AnchoredObject::new(
        graphic(&mut d, "above"),
        designcraft_doc::AnchorPosition::AboveLine { align: designcraft_doc::anchored::AnchorAlign::Center, space_before: 0.0, space_after: 0.0 },
    );
    let st = d.story_mut(sid).unwrap();
    st.insert_object(1, inline);
    // Byte offsets: "A" (1), the mark (3), "B\nC" (3): the second object goes between C and D.
    st.insert_object(7, above);
    // An anchored image without alt text is decorative and reported, like a placed one.
    let bare = designcraft_doc::AnchoredObject::new(graphic(&mut d, ""), designcraft_doc::AnchorPosition::Inline { y_offset: 0.0 });
    let end = d.story(sid).unwrap().text.len();
    d.story_mut(sid).unwrap().insert_object(end, bare);
    assert_eq!(images_without_alt(&d), vec!["dot.png".to_string()]);
    let bytes = export_epub(&d, &EpubOptions::default()).unwrap();
    let mut z = zip::ZipArchive::new(std::io::Cursor::new(bytes)).unwrap();
    let mut html = String::new();
    z.by_name("OEBPS/content.xhtml").unwrap().read_to_string(&mut html).unwrap();
    let img = |alt: &str| format!("<img src=\"images/dot-{}.png\" alt=\"{alt}\"/>", aid.0);
    assert!(html.contains(&format!("A{}B</p>", img("inline"))), "{html}");
    assert!(html.contains(&format!("C<span class=\"anchored\" style=\"display:block;text-align:center\">{}</span>D", img("above"))), "{html}");
    assert!(z.by_name(&format!("OEBPS/images/dot-{}.png", aid.0)).is_ok(), "the image is in the package");
    let page = export_html(&d, &HtmlOptions::default());
    assert!(page.contains("alt=\"inline\"") && page.contains("alt=\"above\""), "{page}");
    // story_html on its own still leaves objects out.
    assert!(!story_html(&d, sid).contains("<img"));
}

#[test]
fn iso8601_timestamps() {
    for (t, want) in [
        (0, "1970-01-01T00:00:00Z"),
        (951_782_400, "2000-02-29T00:00:00Z"),
        (1_791_891_000, "2026-10-13T11:30:00Z"),
        (-86_400, "1969-12-31T00:00:00Z"),
        // Years past 9999 keep the plain zero-padded year (no `+`).
        (253_402_300_800, "10000-01-01T00:00:00Z"),
        // Beyond chrono's range the old `civil_from_days` still converted; pin that output
        // (never the epoch) in both directions and at the `i64` extremes.
        (10_000_000_000_000, "318857-05-20T17:46:40Z"),
        (-10_000_000_000_000, "-314918-08-13T06:13:20Z"),
        (i64::MAX, "292277026596-12-04T15:30:07Z"),
        (i64::MIN, "-292277022657-01-27T08:29:52Z"),
    ] {
        assert_eq!(iso8601(t), want);
    }
}

/// #771, #773: dcterms:modified is a real time, the default identifier is a stable urn:uuid
/// that differs between documents with the same title, and a JPEG cover keeps its type.
#[test]
fn package_metadata_and_cover_image() {
    let opf = |d: &Document, opts: &EpubOptions| {
        let mut z = zip::ZipArchive::new(std::io::Cursor::new(export_epub(d, opts).unwrap())).unwrap();
        let mut s = String::new();
        z.by_name("OEBPS/content.opf").unwrap().read_to_string(&mut s).unwrap();
        let names: Vec<String> = z.file_names().map(str::to_string).collect();
        (s, names)
    };
    let id_of = |opf: &str| opf.split("<dc:identifier id=\"bookid\">").nth(1).and_then(|r| r.split('<').next()).unwrap_or_default().to_string();
    let mut a = Document::new(&NewDocument::default());
    a.title = "Same".into();
    a.created = 1_700_000_000;
    let mut b = a.clone();
    b.created = 1_700_000_001;
    let (oa, _) = opf(&a, &EpubOptions::default());
    let (oa2, _) = opf(&a, &EpubOptions::default());
    let (ob, _) = opf(&b, &EpubOptions::default());
    assert!(id_of(&oa).starts_with("urn:uuid:") && id_of(&oa).len() == 45, "{}", id_of(&oa));
    assert_eq!(id_of(&oa), id_of(&oa2), "stable from export to export");
    assert_ne!(id_of(&oa), id_of(&ob), "not shared by two books with the same title");
    // Modified: the given time, else the document's save (or creation) time.
    assert!(oa.contains("<meta property=\"dcterms:modified\">2023-11-14T22:13:20Z</meta>"), "{oa}");
    let at = EpubOptions { modified: Some(1_791_891_000), ..Default::default() };
    assert!(opf(&a, &at).0.contains("2026-10-13T11:30:00Z"));
    // A JPEG cover is packaged as such.
    let jpeg = EpubOptions { cover: Some(vec![0xFF, 0xD8, 0xFF, 0xE0, 1, 2]), ..Default::default() };
    let (o, names) = opf(&a, &jpeg);
    assert!(o.contains("href=\"images/cover.jpg\" media-type=\"image/jpeg\" properties=\"cover-image\""), "{o}");
    assert!(names.contains(&"OEBPS/images/cover.jpg".to_string()), "{names:?}");
}

fn st_typed(d: &mut Document, sid: StoryId) {
    if let Some(st) = d.story_mut(sid) {
        st.paras[8].style = "Typed".into();
    }
}

/// #767: consecutive `li` paragraphs share one list (`ol` for a numbered style), and
/// consecutive `pre` paragraphs of one style one block with their lines on separate lines.
#[test]
fn lists_and_preformatted_blocks_are_grouped() {
    let mut d = Document::new(&NewDocument::default());
    let lid = d.default_layer();
    for (name, tag, list) in [
        ("Bullet", "li", Some(designcraft_doc::ListType::Bullets)),
        ("Number", "li", Some(designcraft_doc::ListType::Numbers)),
        ("Typed", "li", None),
        ("Code", "pre", None),
    ] {
        let para = designcraft_doc::ParaAttrs { list_type: list, ..Default::default() };
        d.styles_mut().paragraph.push(designcraft_doc::ParagraphStyle {
            name: name.into(),
            based_on: None,
            next_style: None,
            para,
            chars: CharAttrs::default(),
            shortcut: String::new(),
        });
        d.styles_mut()
            .export_tags
            .insert(format!("p:{name}"), designcraft_doc::ExportTag { tag: tag.into(), class: String::new(), ..Default::default() });
    }
    let text = "Intro\nfirst\nsecond\none\ntwo\nx = 1\ny = 2\nEnd\n• typed";
    let (_, sid) = d.add_text_frame(SpreadRef::Doc(0), Rect::new(36.0, 36.0, 300.0, 300.0), lid, text, ParaFormat::default()).unwrap();
    let st = d.story_mut(sid).unwrap();
    for (i, style) in ["Bullet", "Bullet", "Number", "Number", "Code", "Code"].into_iter().enumerate() {
        st.paras[i + 1].style = style.into();
    }
    let html = story_html(&d, sid);
    let li = |c: &str, t: &str| format!("<li class=\"{c}\">{t}</li>");
    assert!(
        html.contains(&format!(
            "<ul>\n{}\n{}\n</ul>\n<ol>\n{}\n{}\n</ol>\n",
            li("bullet", "first"),
            li("bullet", "second"),
            li("number", "one"),
            li("number", "two")
        )),
        "{html}"
    );
    assert!(html.contains("<pre class=\"code\">x = 1\ny = 2</pre>\n<p class=\"basic-paragraph\">End</p>"), "{html}");
    assert_eq!(html.matches("<pre").count(), 1, "{html}");
    // A style without a list type of its own (typed bullets) gets no second marker.
    st_typed(&mut d, sid);
    let html = story_html(&d, sid);
    assert!(html.contains("<ul style=\"list-style:none\">\n<li class=\"typed\">• typed</li>\n</ul>\n"), "{html}");
}

/// #769, #770: the navigation lists the h1–h3 headings (nested by level) and links to them,
/// with its title in the book's language; without headings, each story's first paragraph.
#[test]
fn navigation_follows_the_headings() {
    let mut d = Document::new(&NewDocument { pages: 2, ..Default::default() });
    let lid = d.default_layer();
    for (name, tag) in [("Chapter", "h1"), ("Section", "h2")] {
        d.styles_mut().paragraph.push(designcraft_doc::ParagraphStyle {
            name: name.into(),
            based_on: None,
            next_style: None,
            para: Default::default(),
            chars: CharAttrs::default(),
            shortcut: String::new(),
        });
        d.styles_mut()
            .export_tags
            .insert(format!("p:{name}"), designcraft_doc::ExportTag { tag: tag.into(), class: String::new(), ..Default::default() });
    }
    let text = "One\nBody\nOne point one\nMore\nTwo\nText";
    let (_, sid) = d.add_text_frame(SpreadRef::Doc(0), Rect::new(36.0, 36.0, 300.0, 300.0), lid, text, ParaFormat::default()).unwrap();
    let st = d.story_mut(sid).unwrap();
    for (i, style) in [(0, "Chapter"), (2, "Section"), (4, "Chapter")] {
        st.paras[i].style = style.into();
    }
    // A story that starts with body text adds nothing to the table of contents.
    d.add_text_frame(SpreadRef::Doc(0), Rect::new(36.0, 400.0, 300.0, 500.0), lid, "Text after the figure.", ParaFormat::default()).unwrap();
    let read = |d: &Document, opts: &EpubOptions, path: &str| {
        let mut z = zip::ZipArchive::new(std::io::Cursor::new(export_epub(d, opts).unwrap())).unwrap();
        let mut s = String::new();
        z.by_name(path).unwrap().read_to_string(&mut s).unwrap();
        s
    };
    let nl = EpubOptions { language: "nl".into(), ..Default::default() };
    let nav = read(&d, &nl, "OEBPS/nav.xhtml");
    let link = |f: &str, t: &str, l: &str| format!("<a href=\"{f}#{t}\">{l}</a>");
    // Split Document is off: every heading is in the one content file.
    let want = format!(
        "<h1>Inhoud</h1><ol><li>{}<ol><li>{}</li></ol></li><li>{}</li></ol>",
        link("content.xhtml", "s0-p0", "One"),
        link("content.xhtml", "s0-p2", "One point one"),
        link("content.xhtml", "s0-p4", "Two")
    );
    assert!(nav.contains(&want), "{nav}");
    assert!(!nav.contains("Text after the figure"), "{nav}");
    let content = read(&d, &nl, "OEBPS/content.xhtml");
    assert!(content.contains("<h1 id=\"s0-p0\" class=\"chapter\">One</h1>") && content.contains("<h2 id=\"s0-p2\""), "{content}");
    let custom = EpubOptions { toc_title: Some("In dit boek".into()), ..Default::default() };
    assert!(read(&d, &custom, "OEBPS/nav.xhtml").contains("<h1>In dit boek</h1>"));
    // Without headings: one entry per story, its first paragraph.
    let mut plain = Document::new(&NewDocument::default());
    let lid = plain.default_layer();
    plain.add_text_frame(SpreadRef::Doc(0), Rect::new(36.0, 36.0, 300.0, 100.0), lid, "Opening\nrest", ParaFormat::default()).unwrap();
    let nav = read(&plain, &EpubOptions::default(), "OEBPS/nav.xhtml");
    assert!(nav.contains(&format!("<h1>Contents</h1><ol><li>{}</li></ol>", link("content.xhtml", "s0", "Opening"))), "{nav}");
    // A heading shallower than the one before it but deeper than its parent stays under the
    // parent: h1 A, h3 B, h2 C nest B and C under A.
    let files: BTreeMap<String, String> = "ABCD".chars().map(|c| (c.to_string(), c.to_string())).collect();
    let e = |t: &str, l: u8| (t.to_string(), t.to_string(), l);
    assert_eq!(
        nav_list(&[e("A", 1), e("B", 3), e("C", 2), e("D", 1)], &files),
        format!(
            "<li>{}<ol><li>{}</li><li>{}</li></ol></li><li>{}</li>",
            link("A", "A", "A"),
            link("B", "B", "B"),
            link("C", "C", "C"),
            link("D", "D", "D")
        )
    );
    assert_eq!(
        nav_list(&[e("A", 1), e("B", 2), e("C", 3), e("D", 1)], &files),
        format!(
            "<li>{}<ol><li>{}<ol><li>{}</li></ol></li></ol></li><li>{}</li>",
            link("A", "A", "A"),
            link("B", "B", "B"),
            link("C", "C", "C"),
            link("D", "D", "D")
        )
    );
}

/// Split Document off (the default) gives one content file, h1s and all. Single Paragraph
/// Style starts a file before each paragraph of that style, with well-formed sections; the
/// spine and manifest list the files in order.
#[test]
fn chapters_split_before_a_single_paragraph_style() {
    let mut d = Document::new(&NewDocument { pages: 3, ..Default::default() });
    let lid = d.default_layer();
    d.styles_mut().paragraph.push(designcraft_doc::ParagraphStyle {
        name: "Chapter".into(),
        based_on: None,
        next_style: None,
        para: Default::default(),
        chars: CharAttrs::default(),
        shortcut: String::new(),
    });
    d.styles_mut()
        .export_tags
        .insert("p:Chapter".into(), designcraft_doc::ExportTag { tag: "h1".into(), class: String::new(), ..Default::default() });
    // A title page without a heading, then a story that opens with a chapter and has a
    // second one in the middle.
    d.add_text_frame(SpreadRef::Doc(0), Rect::new(36.0, 36.0, 300.0, 100.0), lid, "Title page", ParaFormat::default()).unwrap();
    let (_, sid) = d
        .add_text_frame(SpreadRef::Doc(0), Rect::new(36.0, 200.0, 300.0, 400.0), lid, "One\nFirst text\nTwo\nSecond text", ParaFormat::default())
        .unwrap();
    let st = d.story_mut(sid).unwrap();
    st.paras[0].style = "Chapter".into();
    st.paras[2].style = "Chapter".into();
    let one_file = |d: &Document, opts: &EpubOptions| {
        let mut z = zip::ZipArchive::new(std::io::Cursor::new(export_epub(d, opts).unwrap())).unwrap();
        let mut c = String::new();
        z.by_name("OEBPS/content.xhtml").unwrap().read_to_string(&mut c).unwrap();
        z.by_name("OEBPS/content2.xhtml").is_err() && c.contains("Title page") && c.contains("Second text") && !c.contains(CHAPTER_MARK)
    };
    assert!(one_file(&d, &EpubOptions::default()), "off by default");
    d.settings.epub_split = EpubSplit::Style("Chapter".into());
    assert!(one_file(&d, &EpubOptions { split: Some(EpubSplit::Off), ..Default::default() }), "the export's own setting wins");
    let mut z = zip::ZipArchive::new(std::io::Cursor::new(export_epub(&d, &EpubOptions::default()).unwrap())).unwrap();
    let mut read = |path: &str| {
        let mut s = String::new();
        z.by_name(path).unwrap().read_to_string(&mut s).unwrap();
        s
    };
    let (c1, c2, c3) = (read("OEBPS/content.xhtml"), read("OEBPS/content2.xhtml"), read("OEBPS/content3.xhtml"));
    assert!(c1.contains("Title page") && !c1.contains("One"), "{c1}");
    assert!(c2.contains("<section id=\"s1\">\n<h1 id=\"s1-p0\"") && c2.contains("First text") && !c2.contains("Two"), "{c2}");
    assert!(c3.contains("<section>\n<h1 id=\"s1-p2\""), "{c3}");
    for c in [&c1, &c2, &c3] {
        assert_eq!(c.matches("<section").count(), c.matches("</section>").count(), "{c}");
        assert!(!c.contains(CHAPTER_MARK));
    }
    let opf = read("OEBPS/content.opf");
    assert!(opf.contains("<itemref idref=\"content\"/>\n<itemref idref=\"content2\"/>\n<itemref idref=\"content3\"/>\n</spine>"), "{opf}");
    assert!(opf.contains("<item id=\"content3\" href=\"content3.xhtml\""), "{opf}");
    let nav = read("OEBPS/nav.xhtml");
    assert!(nav.contains("href=\"content2.xhtml#s1-p0\"") && nav.contains("href=\"content3.xhtml#s1-p2\""), "{nav}");
    assert!(z.by_name("OEBPS/content4.xhtml").is_err());
    // HTML stays one page, without the marks.
    let html = export_html(&d, &HtmlOptions::default());
    assert!(!html.contains(CHAPTER_MARK) && html.contains("Second text"));
}

/// Based on export tags, a style with Split Document on starts each chapter file and an h1
/// without it doesn't; navigation and endnote links point at the files their targets ended
/// up in.
#[test]
fn chapters_split_before_split_document_styles() {
    let mut d = Document::new(&NewDocument::default());
    d.settings.epub_split = EpubSplit::ExportTags;
    let lid = d.default_layer();
    for (name, tag, split) in [("Part", "p", true), ("Chapter", "h1", false)] {
        d.styles_mut().paragraph.push(designcraft_doc::ParagraphStyle {
            name: name.into(),
            based_on: None,
            next_style: None,
            para: Default::default(),
            chars: CharAttrs::default(),
            shortcut: String::new(),
        });
        d.styles_mut().export_tags.insert(format!("p:{name}"), designcraft_doc::ExportTag { tag: tag.into(), split, ..Default::default() });
    }
    d.add_text_frame(SpreadRef::Doc(0), Rect::new(36.0, 36.0, 300.0, 100.0), lid, "Title page", ParaFormat::default()).unwrap();
    let (_, sid) = d
        .add_text_frame(
            SpreadRef::Doc(0),
            Rect::new(36.0, 200.0, 300.0, 600.0),
            lid,
            "Part One\nChapter A\ntext a\nPart Two\nChapter B\ntext b",
            ParaFormat::default(),
        )
        .unwrap();
    let st = d.story_mut(sid).unwrap();
    for (i, style) in [(0, "Part"), (1, "Chapter"), (3, "Part"), (4, "Chapter")] {
        st.paras[i].style = style.into();
    }
    // After "text a".
    st.insert_endnote(25, "End note", ParaFormat::default());
    let mut z = zip::ZipArchive::new(std::io::Cursor::new(export_epub(&d, &EpubOptions::default()).unwrap())).unwrap();
    let mut read = |path: &str| {
        let mut s = String::new();
        z.by_name(path).unwrap().read_to_string(&mut s).unwrap();
        s
    };
    let (c1, c2, c3) = (read("OEBPS/content.xhtml"), read("OEBPS/content2.xhtml"), read("OEBPS/content3.xhtml"));
    assert!(c1.contains("Title page") && !c1.contains("Part One"), "{c1}");
    assert!(c2.contains("Part One") && c2.contains("<h1 id=\"s1-p1\"") && !c2.contains("Part Two"), "the h1 stays with its part: {c2}");
    assert!(c3.contains("Part Two") && c3.contains("<h1 id=\"s1-p4\"") && c3.contains("End note"), "{c3}");
    for c in [&c1, &c2, &c3] {
        assert_eq!(c.matches("<section").count(), c.matches("</section>").count(), "{c}");
    }
    let nav = read("OEBPS/nav.xhtml");
    assert!(nav.contains("href=\"content2.xhtml#s1-p1\"") && nav.contains("href=\"content3.xhtml#s1-p4\""), "{nav}");
    assert!(c2.contains("href=\"content3.xhtml#en-1\""), "{c2}");
    assert!(c3.contains("href=\"content2.xhtml#enref-1\""), "the endnote links back across files: {c3}");
    assert!(z.by_name("OEBPS/content4.xhtml").is_err());
}

/// Note links stay working across chapter files: endnotes end the book, in the last file.
#[test]
fn note_links_point_at_the_chapter_file_of_their_target() {
    let mut d = Document::new(&NewDocument::default());
    let lid = d.default_layer();
    d.styles_mut().paragraph.push(designcraft_doc::ParagraphStyle {
        name: "Chapter".into(),
        based_on: None,
        next_style: None,
        para: Default::default(),
        chars: CharAttrs::default(),
        shortcut: String::new(),
    });
    d.styles_mut()
        .export_tags
        .insert("p:Chapter".into(), designcraft_doc::ExportTag { tag: "h1".into(), class: String::new(), ..Default::default() });
    let (_, sid) = d
        .add_text_frame(SpreadRef::Doc(0), Rect::new(36.0, 36.0, 300.0, 400.0), lid, "One\nFirst text\nTwo\nSecond text", ParaFormat::default())
        .unwrap();
    let st = d.story_mut(sid).unwrap();
    st.paras[0].style = "Chapter".into();
    st.paras[2].style = "Chapter".into();
    // After "First": an endnote and a footnote.
    st.insert_endnote(9, "End note", ParaFormat::default());
    st.insert_note(9, "Foot note", ParaFormat::default());
    d.settings.epub_split = EpubSplit::Style("Chapter".into());
    let mut z = zip::ZipArchive::new(std::io::Cursor::new(export_epub(&d, &EpubOptions::default()).unwrap())).unwrap();
    let mut read = |path: &str| {
        let mut s = String::new();
        z.by_name(path).unwrap().read_to_string(&mut s).unwrap();
        s
    };
    let (first, last) = (read("OEBPS/content.xhtml"), read("OEBPS/content2.xhtml"));
    assert!(first.contains("id=\"enref-1\" href=\"content2.xhtml#en-1\""), "{first}");
    assert!(first.contains("href=\"#fn-1\"") && first.contains("href=\"#fnref-1\""), "a footnote stays in its chapter: {first}");
    assert!(last.contains("Second text") && last.contains("<a href=\"content.xhtml#enref-1\">"), "{last}");
}

/// A split before a list item or a preformatted line closes the open list or block first and
/// reopens it in the new file, so every file is balanced.
#[test]
fn chapter_files_split_inside_lists_and_pre_stay_balanced() {
    let mut d = Document::new(&NewDocument::default());
    d.settings.epub_split = EpubSplit::ExportTags;
    let lid = d.default_layer();
    for (name, tag) in [("Item", "li"), ("Code", "pre")] {
        d.styles_mut().paragraph.push(designcraft_doc::ParagraphStyle {
            name: name.into(),
            based_on: None,
            next_style: None,
            para: Default::default(),
            chars: CharAttrs::default(),
            shortcut: String::new(),
        });
        d.styles_mut().export_tags.insert(format!("p:{name}"), designcraft_doc::ExportTag { tag: tag.into(), split: true, ..Default::default() });
    }
    let (_, sid) = d
        .add_text_frame(
            SpreadRef::Doc(0),
            Rect::new(36.0, 36.0, 300.0, 300.0),
            lid,
            "Intro
one
two
x = 1
y = 2
End",
            ParaFormat::default(),
        )
        .unwrap();
    let st = d.story_mut(sid).unwrap();
    for (i, style) in [(1, "Item"), (2, "Item"), (3, "Code"), (4, "Code")] {
        st.paras[i].style = style.into();
    }
    let mut z = zip::ZipArchive::new(std::io::Cursor::new(export_epub(&d, &EpubOptions::default()).unwrap())).unwrap();
    let mut read = |path: &str| {
        let mut s = String::new();
        z.by_name(path).unwrap().read_to_string(&mut s).unwrap();
        s
    };
    let files: Vec<String> = ["content", "content2", "content3", "content4", "content5"].iter().map(|f| read(&format!("OEBPS/{f}.xhtml"))).collect();
    for (c, text) in files.iter().zip(["Intro", "one", "two", "x = 1", "y = 2"]) {
        assert!(c.contains(text), "{text}: {c}");
        for (open, close) in [("<ul", "</ul>"), ("<pre", "</pre>"), ("<section", "</section>")] {
            assert_eq!(c.matches(open).count(), c.matches(close).count(), "{open}: {c}");
        }
    }
    assert!(files[1].contains("<ul") && files[2].contains("<ul") && files[3].contains("<pre") && files[4].contains("<pre"));
    assert!(files[4].contains("End"));
    assert!(z.by_name("OEBPS/content6.xhtml").is_err());
}

#[test]
fn split_chapters_keeps_sections_well_formed() {
    let m = CHAPTER_MARK;
    assert_eq!(split_chapters("<section id=\"s0\">\n<p>a</p>\n</section>\n"), vec!["<section id=\"s0\">\n<p>a</p>\n</section>\n"]);
    assert_eq!(split_chapters(&format!("<section id=\"s0\">\n{m}<h1>A</h1>\n</section>\n")), vec!["<section id=\"s0\">\n<h1>A</h1>\n</section>\n"]);
    assert_eq!(
        split_chapters(&format!("<section id=\"s0\">\n<p>a</p>\n{m}<h1>B</h1>\n</section>\n")),
        vec!["<section id=\"s0\">\n<p>a</p>\n</section>\n", "<section>\n<h1>B</h1>\n</section>\n"]
    );
    assert_eq!(split_chapters(""), vec![String::new()]);
}

#[test]
fn styles_whose_names_slug_alike_keep_their_own_classes() {
    let mut d = Document::new(&NewDocument::default());
    let lid = d.default_layer();
    for (name, size) in [("見出し", 30.0), ("本文", 10.0), ("Body Text", 12.0), ("Body-Text", 20.0)] {
        d.styles_mut().paragraph.push(designcraft_doc::ParagraphStyle {
            name: name.into(),
            based_on: None,
            next_style: None,
            para: Default::default(),
            chars: CharAttrs { size: Some(size), ..Default::default() },
            shortcut: String::new(),
        });
    }
    for (k, name) in ["見出し", "本文", "Body Text", "Body-Text"].into_iter().enumerate() {
        let y = 36.0 + 100.0 * k as f64;
        let pf = ParaFormat { style: name.into(), ..Default::default() };
        d.add_text_frame(SpreadRef::Doc(0), Rect::new(36.0, y, 300.0, y + 50.0), lid, &format!("P{k}"), pf).unwrap();
    }
    let bytes = export_epub(&d, &EpubOptions::default()).unwrap();
    let mut z = zip::ZipArchive::new(std::io::Cursor::new(bytes)).unwrap();
    let mut html = String::new();
    z.by_name("OEBPS/content.xhtml").unwrap().read_to_string(&mut html).unwrap();
    let mut css = String::new();
    z.by_name("OEBPS/style.css").unwrap().read_to_string(&mut css).unwrap();
    let class_of = |text: &str| {
        let end = html.find(&format!("\">{text}</p>")).unwrap();
        let start = html[..end].rfind("class=\"").unwrap() + 7;
        html[start..end].to_string()
    };
    let classes: Vec<String> = (0..4).map(|k| class_of(&format!("P{k}"))).collect();
    for (k, c) in classes.iter().enumerate() {
        assert!(!classes[..k].contains(c), "class `{c}` shared: {classes:?}");
        assert_eq!(css.matches(&format!("p.{c} {{")).count(), 1, "one rule for `{c}`:\n{css}");
    }
}

#[test]
fn slugs() {
    assert_eq!(slug("[Basic Paragraph]"), "basic-paragraph");
    assert_eq!(slug("Body First"), "body-first");
    assert_eq!(slug("1 Head"), "s-1-head");
}

#[test]
fn html_is_one_self_contained_page() {
    let mut d = Document::new(&NewDocument::default());
    let lid = d.default_layer();
    d.add_text_frame(SpreadRef::Doc(0), Rect::new(36.0, 36.0, 300.0, 100.0), lid, "Hello <web>", ParaFormat::default()).unwrap();
    let html = export_html(&d, &HtmlOptions { title: Some("T".into()), language: None, ..Default::default() });
    assert!(html.starts_with("<!DOCTYPE html>") && html.contains("<title>T</title>") && html.contains("Hello &lt;web&gt;"));
    assert!(html.contains("p.basic-paragraph"), "styles inline");
    assert_eq!(base64(b"Man"), "TWFu");
    assert_eq!(base64(b"Ma"), "TWE=");
    assert_eq!(base64(b"M"), "TQ==");
}

/// #774: an image without alt text is decorative (`alt=""`) and reported, and package names
/// come from the original file names.
#[test]
fn images_get_readable_names_and_no_file_name_alt() {
    let mut d = Document::new(&NewDocument::default());
    let lid = d.default_layer();
    let place = |d: &mut Document, name: &str, mime: &str, alt: &str, y: f64| {
        let aid = AssetId(d.alloc());
        d.assets.insert(
            aid,
            Arc::new(Asset {
                page: 0,
                id: aid,
                name: name.into(),
                mime: mime.into(),
                link: None,
                data: Arc::new(vec![1]),
                pixels: Some((1, 1)),
                ..Default::default()
            }),
        );
        let mut item = Item::new(ItemId(d.alloc()), lid, Shape::Rectangle, shapes::rectangle(Rect::new(36.0, y, 72.0, y + 36.0)));
        item.alt_text = alt.into();
        item.content = Content::Graphic(Graphic {
            layer_options: None,
            asset: aid,
            size: (1.0, 1.0),
            xf: Affine::IDENTITY,
            auto_fit: Default::default(),
            fit_align: 4,
            crop: [0.0; 4],
            clip: None,
            wrap: Default::default(),
            fill: Default::default(),
        });
        d.insert_item(SpreadRef::Doc(0), item, None).unwrap();
        aid.0
    };
    let a = place(&mut d, "/tmp/Anim Frame (1).GIF", "image/gif", "", 36.0);
    let b = place(&mut d, "photo.jpg", "image/jpeg", "A heron", 120.0);
    let c = place(&mut d, "???.png", "image/png", "Mark", 200.0);
    assert_eq!(images_without_alt(&d), vec!["/tmp/Anim Frame (1).GIF".to_string()]);
    let mut z = zip::ZipArchive::new(std::io::Cursor::new(export_epub(&d, &EpubOptions::default()).unwrap())).unwrap();
    let mut x = String::new();
    z.by_name("OEBPS/content.xhtml").unwrap().read_to_string(&mut x).unwrap();
    assert!(x.contains(&format!("<img src=\"images/anim-frame-1-{a}.gif\" alt=\"\"/>")), "{x}");
    assert!(x.contains(&format!("<img src=\"images/photo-{b}.jpg\" alt=\"A heron\"/>")), "{x}");
    assert!(x.contains(&format!("<img src=\"images/{c}.png\" alt=\"Mark\"/>")), "{x}");
    assert!(z.by_name(&format!("OEBPS/images/anim-frame-1-{a}.gif")).is_ok());
}

/// #775: the generic fallback follows the font, and embedded fonts are packaged and declared.
#[test]
fn generic_fallbacks_and_embedded_fonts() {
    let none = BTreeMap::new();
    for (family, want) in [
        ("Source Serif 4", "serif"),
        ("Source Sans 3", "sans-serif"),
        ("Microsoft Sans Serif", "sans-serif"),
        ("JetBrains Mono", "monospace"),
        ("Courier New", "monospace"),
        ("Franklin Gothic", "sans-serif"),
        ("Garamond", "serif"),
    ] {
        assert_eq!(generic_family(family, &none), want, "{family}");
    }
    let known: BTreeMap<String, String> = [("Montserrat".to_string(), "sans-serif".to_string()), ("Odd".to_string(), "nonsense".to_string())].into();
    assert_eq!(generic_family("Montserrat", &known), "sans-serif");
    assert_eq!(generic_family("Odd", &known), "serif", "only CSS generic families are used");
    assert_eq!(css_weight_style("Semibold Italic"), (600, "italic"));
    assert_eq!(css_weight_style("Bold"), (700, "normal"));
    assert_eq!(css_weight_style("Extra Bold"), (800, "normal"));
    assert_eq!(css_weight_style("Regular"), (400, "normal"));
    assert_eq!(css_weight_style("Light"), (300, "normal"));
    assert_eq!(css_weight_style("Thin Oblique"), (100, "italic"));
    assert_eq!(css_weight_style("Black"), (900, "normal"));

    let mut d = Document::new(&NewDocument::default());
    let lid = d.default_layer();
    d.add_text_frame(SpreadRef::Doc(0), Rect::new(36.0, 36.0, 300.0, 100.0), lid, "Hello", ParaFormat::default()).unwrap();
    if let Some(basic) = d.styles_mut().paragraph.iter_mut().find(|s| s.name == story::BASIC_PARAGRAPH) {
        basic.chars.font_family = Some("Montserrat".into());
        basic.chars.font_style = Some("Bold Italic".into());
    }
    assert_eq!(stylesheet_fonts(&d), vec![("Montserrat".to_string(), "Bold Italic".to_string())]);
    let opts = EpubOptions {
        generic_families: known,
        fonts: vec![EmbeddedFont { family: "Montserrat".into(), style: "Bold Italic".into(), data: b"OTTO font".to_vec() }],
        ..Default::default()
    };
    let mut z = zip::ZipArchive::new(std::io::Cursor::new(export_epub(&d, &opts).unwrap())).unwrap();
    let mut read = |path: &str| {
        let mut s = String::new();
        z.by_name(path).unwrap().read_to_string(&mut s).unwrap();
        s
    };
    let css = read("OEBPS/style.css");
    assert!(
        css.starts_with(
            "@font-face { font-family: \"Montserrat\"; src: url(\"fonts/0-montserrat-bold-italic.otf\"); font-weight: 700; font-style: italic; }"
        ),
        "{css}"
    );
    assert!(css.contains("font-family: \"Montserrat\", sans-serif;"), "{css}");
    let opf = read("OEBPS/content.opf");
    assert!(opf.contains("<item id=\"font0\" href=\"fonts/0-montserrat-bold-italic.otf\" media-type=\"font/otf\"/>"), "{opf}");
    assert_eq!(read("OEBPS/fonts/0-montserrat-bold-italic.otf"), "OTTO font");
    // HTML takes the generic families too, and embeds nothing.
    let html = export_html(&d, &HtmlOptions { generic_families: opts.generic_families.clone(), ..Default::default() });
    assert!(html.contains("\"Montserrat\", sans-serif;") && !html.contains("@font-face"), "{html}");
}

#[test]
fn metadata_is_serialized_for_its_output_context() {
    let mut d = Document::new(&NewDocument::default());
    d.title = "Field Notes & <Edition>".into();
    let lid = d.default_layer();
    let (_, sid) = d.add_text_frame(SpreadRef::Doc(0), Rect::new(36.0, 36.0, 300.0, 100.0), lid, "A&B <C> \"D\"", ParaFormat::default()).unwrap();
    d.styles_mut().export_tags.insert(
        format!("p:{}", story::BASIC_PARAGRAPH),
        designcraft_doc::ExportTag { tag: "h2".into(), class: "Primary \"Copy\"".into(), ..Default::default() },
    );
    if let Some(basic) = d.styles_mut().paragraph.iter_mut().find(|s| s.name == story::BASIC_PARAGRAPH) {
        basic.chars.font_family = Some("Quoted \"Serif\" <Alt>".into());
    }

    let aid = AssetId(d.alloc());
    d.assets.insert(
        aid,
        Arc::new(Asset {
            id: aid,
            name: "Cover & Notes".into(),
            mime: "image/png; profile=screen".into(),
            data: Arc::new(vec![1]),
            pixels: Some((1, 1)),
            ..Default::default()
        }),
    );
    let mut item = Item::new(ItemId(d.alloc()), lid, Shape::Rectangle, shapes::rectangle(Rect::new(36.0, 120.0, 72.0, 156.0)));
    item.alt_text = "Cover \"A\" & B".into();
    item.content = Content::Graphic(Graphic {
        layer_options: None,
        asset: aid,
        size: (1.0, 1.0),
        xf: Affine::IDENTITY,
        clip: None,
        auto_fit: Default::default(),
        fit_align: 4,
        crop: [0.0; 4],
        wrap: Default::default(),
        fill: Default::default(),
    });
    d.insert_item(SpreadRef::Doc(0), item, None).unwrap();

    let story = story_html(&d, sid);
    assert!(story.contains("<h2 class=\"Primary &quot;Copy&quot;\">A&amp;B &lt;C&gt; &quot;D&quot;</h2>"));
    let html = export_html(&d, &HtmlOptions { title: None, language: Some("English (US)".into()), ..Default::default() });
    assert!(html.contains("<html lang=\"und\">"));
    assert!(html.contains("<title>Field Notes &amp; &lt;Edition&gt;</title>"));
    assert!(html.contains("alt=\"Cover &quot;A&quot; &amp; B\""));
    assert!(html.contains("src=\"data:image/png;base64,AQ==\""));
    assert!(!html.contains("image/png; profile=screen"));
    assert!(!stylesheet(&d).contains("<Alt>"));
    assert!(stylesheet(&d).contains("\\3c Alt\\3e "));

    let bytes = export_epub(
        &d,
        &EpubOptions {
            title: None,
            author: Some("A & B".into()),
            language: "English (US)".into(),
            identifier: Some("notes&edition".into()),
            cover: None,
            ..Default::default()
        },
    )
    .unwrap();
    let mut z = zip::ZipArchive::new(std::io::Cursor::new(bytes)).unwrap();
    let mut opf = String::new();
    z.by_name("OEBPS/content.opf").unwrap().read_to_string(&mut opf).unwrap();
    assert!(opf.contains("xml:lang=\"und\""));
    assert!(opf.contains("<dc:identifier id=\"bookid\">notes&amp;edition</dc:identifier>"));
    assert!(opf.contains("<dc:creator>A &amp; B</dc:creator>"));
    assert!(opf.contains("media-type=\"image/png\""));
}

/// File › File Info's Document Title and Author name the EPUB and the HTML page; the export
/// options' title and author replace them.
#[test]
fn file_info_titles_the_epub_and_the_html_page() {
    let mut d = Document::new(&NewDocument { title: "flyer-v3".into(), ..Default::default() });
    d.file_info.document_title = "Spring & Summer".into();
    d.file_info.author = "Ana Ruiz".into();
    let opf = |d: &Document, opts: &EpubOptions| {
        let mut z = zip::ZipArchive::new(std::io::Cursor::new(export_epub(d, opts).unwrap())).unwrap();
        let mut opf = String::new();
        z.by_name("OEBPS/content.opf").unwrap().read_to_string(&mut opf).unwrap();
        opf
    };
    let o = opf(&d, &EpubOptions::default());
    assert!(o.contains("<dc:title>Spring &amp; Summer</dc:title>") && o.contains("<dc:creator>Ana Ruiz</dc:creator>"), "{o}");
    let o = opf(&d, &EpubOptions { title: Some("Proof".into()), author: Some("Studio".into()), ..Default::default() });
    assert!(o.contains("<dc:title>Proof</dc:title>") && o.contains("<dc:creator>Studio</dc:creator>"), "{o}");
    let html = export_html(&d, &HtmlOptions::default());
    assert!(html.contains("<title>Spring &amp; Summer</title>"));
    d.file_info = Default::default();
    let o = opf(&d, &EpubOptions::default());
    assert!(o.contains("<dc:title>flyer-v3</dc:title>") && !o.contains("<dc:creator>"), "{o}");
}

/// "Body¹ text", a 2×2 table, then "Closing" with an endnote.
fn tables_and_notes() -> Document {
    let mut d = Document::new(&NewDocument::default());
    let lid = d.default_layer();
    let (_, sid) = d.add_text_frame(SpreadRef::Doc(0), Rect::new(36.0, 36.0, 300.0, 400.0), lid, "Body text", ParaFormat::default()).unwrap();
    let st = d.story_mut(sid).unwrap();
    st.insert_note(4, "Foot note text", ParaFormat::default());
    let mut t = Table::new(1, 2, 2, 0, 0, 200.0);
    for (r, c, s) in [(0, 0, "Cell A1"), (0, 1, "Cell B1"), (1, 0, "Cell A2"), (1, 1, "Cell B2")] {
        t.cell_mut(r, c).unwrap().text = Story::with_text(StoryId(0), s, ParaFormat::default());
    }
    let end = st.len();
    st.insert_table(end, t);
    let end = st.len();
    st.insert(end, "Closing");
    let end = st.len();
    st.insert_endnote(end, "End note text", ParaFormat::default());
    d
}

#[test]
fn tables_footnotes_and_endnotes_are_exported() {
    let d = tables_and_notes();
    let bytes = export_epub(&d, &EpubOptions::default()).unwrap();
    let mut z = zip::ZipArchive::new(std::io::Cursor::new(bytes)).unwrap();
    let mut html = String::new();
    z.by_name("OEBPS/content.xhtml").unwrap().read_to_string(&mut html).unwrap();
    let (t0, t1) = (html.find("<table>").unwrap(), html.find("</table>").unwrap());
    for cell in ["Cell A1", "Cell B1", "Cell A2", "Cell B2"] {
        let at = html.find(&format!("<td><p class=\"basic-paragraph\">{cell}</p>\n</td>")).unwrap_or_else(|| panic!("{cell} in a cell:\n{html}"));
        assert!(t0 < at && at < t1);
    }
    assert_eq!(html.matches("<tr>").count(), 2);

    let noteref = "Body<sup><a epub:type=\"noteref\" role=\"doc-noteref\" id=\"fnref-1\" href=\"#fn-1\">1</a></sup> text";
    assert!(html.contains(noteref), "footnote reference:\n{html}");
    let aside = html.find("<aside epub:type=\"footnote\" role=\"doc-footnote\" id=\"fn-1\">").unwrap();
    let aside_end = aside + html[aside..].find("</aside>").unwrap();
    assert!(html[aside..aside_end].contains("<a href=\"#fnref-1\">1</a> Foot note text"));
    assert!(aside < t0, "the footnote follows its paragraph");

    assert!(html.contains("Closing<sup><a epub:type=\"noteref\" role=\"doc-noteref\" id=\"enref-1\" href=\"#en-1\">1</a></sup>"));
    let section = html.find("<section epub:type=\"endnotes\" role=\"doc-endnotes\" id=\"endnotes\">").unwrap();
    assert!(section > html.find("Closing").unwrap());
    assert!(html[section..].contains("<li epub:type=\"endnote\" id=\"en-1\">"));
    assert!(html[section..].contains("<a href=\"#enref-1\">1</a> End note text"));
    let mut nav = String::new();
    z.by_name("OEBPS/nav.xhtml").unwrap().read_to_string(&mut nav).unwrap();
    assert!(nav.contains("href=\"content.xhtml#endnotes\""));

    let page = export_html(&d, &HtmlOptions::default());
    assert!(page.contains("<aside role=\"doc-footnote\" id=\"fn-1\">") && page.contains("Cell B2") && page.contains("End note text"));
    assert!(!page.contains("epub:type"), "plain HTML has no EPUB namespace");
}

/// A footnote or a table after a list paragraph or a preformatted line closes the list or
/// block first: `aside` and `table` can't sit inside `ul` or `pre`.
#[test]
fn notes_and_tables_close_an_open_list_or_pre_block() {
    for (tag, list, open, close) in [("li", Some(designcraft_doc::ListType::Bullets), "<ul>", "</ul>"), ("pre", None, "<pre", "</pre>")] {
        let mut d = tables_and_notes();
        let para = designcraft_doc::ParaAttrs { list_type: list, ..Default::default() };
        d.styles_mut().paragraph.push(designcraft_doc::ParagraphStyle {
            name: "Grouped".into(),
            based_on: None,
            next_style: None,
            para,
            chars: CharAttrs::default(),
            shortcut: String::new(),
        });
        d.styles_mut()
            .export_tags
            .insert("p:Grouped".into(), designcraft_doc::ExportTag { tag: tag.into(), class: String::new(), ..Default::default() });
        let sid = d.stories.keys().copied().find(|sid| d.story(*sid).is_some_and(|st| !st.notes.is_empty())).unwrap();
        for p in &mut d.story_mut(sid).unwrap().paras {
            p.style = "Grouped".into();
        }
        let html = story_html(&d, sid);
        let mut rest = html.as_str();
        while let Some(at) = rest.find(open) {
            let block = &rest[at..];
            let end = block.find(close).unwrap_or_else(|| panic!("{open} is closed:\n{html}"));
            assert!(!block[..end].contains("<aside") && !block[..end].contains("<table"), "{html}");
            rest = &block[end..];
        }
        assert!(html.contains("<aside") && html.contains("<table>"), "{html}");
    }
}

#[test]
fn table_rows_groups_and_merged_cells() {
    let mut d = Document::new(&NewDocument::default());
    let lid = d.default_layer();
    let (_, sid) = d.add_text_frame(SpreadRef::Doc(0), Rect::new(36.0, 36.0, 300.0, 400.0), lid, "", ParaFormat::default()).unwrap();
    // One header row, two body rows, one footer row; the header spans both columns and the
    // first body cell spans both body rows and runs into the footer.
    let mut t = Table::new(1, 2, 2, 1, 1, 200.0);
    for (r, c, s) in [(0, 0, "Head"), (1, 0, "Tall"), (1, 1, "B1"), (2, 1, "B2"), (3, 0, "F1"), (3, 1, "F2")] {
        t.cell_mut(r, c).unwrap().text = Story::with_text(StoryId(0), s, ParaFormat::default());
    }
    t.cell_mut(0, 0).unwrap().col_span = 2;
    t.cell_mut(1, 0).unwrap().row_span = 3;
    d.story_mut(sid).unwrap().insert_table(0, t);
    let html = story_html(&d, sid);
    let p = |s: &str| format!("<p class=\"basic-paragraph\">{s}</p>\n");
    let expected = format!(
        "<table>\n<thead>\n<tr><th colspan=\"2\">{}</th>\n</tr>\n</thead>\n<tbody>\n<tr><td rowspan=\"2\">{}</td>\n<td>{}</td>\n</tr>\n<tr><td>{}</td>\n</tr>\n</tbody>\n<tfoot>\n<tr><td>{}</td>\n<td>{}</td>\n</tr>\n</tfoot>\n</table>\n",
        p("Head"),
        p("Tall"),
        p("B1"),
        p("B2"),
        p("F1"),
        p("F2")
    );
    assert!(html.contains(&expected), "{html}");
}

/// Note links stay working across chapter files: a link whose target ended up in another
/// file (endnotes end the book) names that file; one in its own file stays as it is.
#[test]
fn note_links_name_the_chapter_file_of_their_target() {
    let files: BTreeMap<String, String> =
        [("enref-1", "content.xhtml"), ("fn-1", "content.xhtml"), ("fnref-1", "content.xhtml"), ("en-1", "content2.xhtml")]
            .into_iter()
            .map(|(id, f)| (id.to_string(), f.to_string()))
            .collect();
    let first = "<p>First<a id=\"enref-1\" href=\"#en-1\">1</a><a id=\"fnref-1\" href=\"#fn-1\">*</a></p><aside id=\"fn-1\"><a href=\"#fnref-1\">*</a></aside><a href=\"#gone\">x</a>";
    assert_eq!(
        link_across_chapters(first, "content.xhtml", &files),
        "<p>First<a id=\"enref-1\" href=\"content2.xhtml#en-1\">1</a><a id=\"fnref-1\" href=\"#fn-1\">*</a></p><aside id=\"fn-1\"><a href=\"#fnref-1\">*</a></aside><a href=\"#gone\">x</a>"
    );
    let last = "<li id=\"en-1\"><a href=\"#enref-1\">1</a> End note</li>";
    assert_eq!(link_across_chapters(last, "content2.xhtml", &files), "<li id=\"en-1\"><a href=\"content.xhtml#enref-1\">1</a> End note</li>");
}

#[test]
fn char_css_joins_underline_and_strikethrough_in_one_declaration() {
    let d = Document::new(&NewDocument::default());
    let css = |u: bool, s: bool| {
        let c = CharProps { underline: u, strikethrough: s, ..CharProps::default() };
        let mut out = String::new();
        char_css(&d, &c, 12.0, &BTreeMap::new(), &mut out);
        out
    };
    let both = css(true, true);
    assert_eq!(both.matches("text-decoration:").count(), 1, "{both}");
    assert!(both.contains("text-decoration: underline line-through;"), "{both}");
    assert!(css(true, false).contains("text-decoration: underline;"));
    assert!(css(false, true).contains("text-decoration: line-through;"));
    assert!(!css(false, false).contains("text-decoration"));
}

#[test]
fn character_styles_keep_decorations_caps_and_tracking() {
    let mut d = Document::new(&NewDocument::default());
    d.styles_mut().character.push(designcraft_doc::CharacterStyle {
        name: "Under".into(),
        based_on: None,
        chars: CharAttrs {
            underline: Some(true),
            strikethrough: Some(true),
            capitalization: Some(designcraft_doc::Capitalization::AllCaps),
            tracking: Some(100.0),
            ..Default::default()
        },
        shortcut: String::new(),
    });
    let css = css::stylesheet(&d);
    assert!(css.contains("text-decoration: underline line-through;"), "{css}");
    assert!(css.contains("text-transform: uppercase;"), "{css}");
    assert!(css.contains("letter-spacing: 0.100em;"), "{css}");
    let o = CharAttrs { strikethrough: Some(true), ..Default::default() };
    assert_eq!(css::override_css(&d, story::NO_CHAR_STYLE, &o), "text-decoration: line-through;");
}

fn char_style(name: &str, based_on: Option<&str>, chars: CharAttrs) -> designcraft_doc::CharacterStyle {
    designcraft_doc::CharacterStyle { name: name.into(), based_on: based_on.map(Into::into), chars, shortcut: String::new() }
}

#[test]
fn override_keeps_style_decorations_and_resets_tracking_and_caps() {
    use designcraft_doc::Capitalization;
    let mut d = Document::new(&NewDocument::default());
    let styles = d.styles_mut();
    styles.character.push(char_style(
        "Base",
        None,
        CharAttrs { underline: Some(true), tracking: Some(100.0), capitalization: Some(Capitalization::AllCaps), ..Default::default() },
    ));
    styles.character.push(char_style("Child", Some("Base"), CharAttrs::default()));
    let strike = CharAttrs { strikethrough: Some(true), ..Default::default() };
    for style in ["Base", "Child"] {
        assert_eq!(css::override_css(&d, style, &strike), "text-decoration: underline line-through;", "{style}");
    }
    let off = CharAttrs { underline: Some(false), ..Default::default() };
    assert_eq!(css::override_css(&d, "Child", &off), "text-decoration: none;");
    let reset = CharAttrs { tracking: Some(0.0), capitalization: Some(Capitalization::Normal), ..Default::default() };
    assert_eq!(css::override_css(&d, "Child", &reset), "letter-spacing: 0em; text-transform: none; font-variant: normal;");
    assert_eq!(css::override_css(&d, story::NO_CHAR_STYLE, &reset), "");
    let styles = d.styles_mut();
    styles.character.push(char_style("Small", None, CharAttrs { capitalization: Some(Capitalization::SmallCaps), ..Default::default() }));
    let small = CharAttrs { capitalization: Some(Capitalization::SmallCaps), ..Default::default() };
    let all = CharAttrs { capitalization: Some(Capitalization::AllCaps), ..Default::default() };
    assert_eq!(css::override_css(&d, "Base", &small), "text-transform: none; font-variant: normal; font-variant: small-caps;");
    assert_eq!(css::override_css(&d, "Small", &all), "text-transform: none; font-variant: normal; text-transform: uppercase;");
}
