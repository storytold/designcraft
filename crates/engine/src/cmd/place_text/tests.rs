use super::*;

#[test]
fn place_text_file_with_autoflow() {
    let dir = std::env::temp_dir().join(format!("dc-place-text-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("long.txt");
    let para = "The quick brown fox jumps over the lazy dog, again and again, to fill the page. ".repeat(12);
    std::fs::write(&path, vec![para.as_str(); 60].join("\n")).unwrap();
    let mut s = Session::new();
    s.execute("file.new", &json!({"pages": 1})).unwrap();
    let r = s.execute("file.place", &json!({"path": path.to_string_lossy(), "autoflow": true})).unwrap();
    assert!(r["pagesAdded"].as_u64().unwrap() >= 2, "{r}");
    assert_eq!(r["overset"], false);
    let d = &s.doc().unwrap().doc;
    let sid = StoryId(r["story"].as_u64().unwrap());
    assert_eq!(d.story(sid).unwrap().frames.len(), d.page_count());
    assert_eq!(d.story(sid).unwrap().paras.len(), 60);
    // Into the insertion point of another frame.
    let fr = s.execute("frame.create", &json!({"rect": [72, 72, 300, 200], "content": "text", "text": "Start  end"})).unwrap();
    let sid2 = fr["story"].as_u64().unwrap();
    s.execute("text.select", &json!({"story": sid2, "anchor": 6, "focus": 6})).unwrap();
    let small = dir.join("small.rtf");
    std::fs::write(&small, br"{\rtf1 {\b middle}}").unwrap();
    s.execute("file.place", &json!({"path": small.to_string_lossy()})).unwrap();
    let st = s.doc().unwrap().doc.story(StoryId(sid2)).unwrap().clone();
    assert_eq!(st.text, "Start middle end");
    assert_eq!(st.format_after(6).over.font_style.as_deref(), Some("Bold"));
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn autoflow_sets_one_frame_per_column() {
    // Regression (#625): autoflow set one frame across the margins, over the gutter.
    let dir = std::env::temp_dir().join(format!("dc-place-columns-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("long.txt");
    let para = "Lorem ipsum dolor sit amet, consectetur adipiscing elit. ".repeat(12);
    std::fs::write(&path, vec![para.as_str(); 40].join("\n")).unwrap();
    for binding in ["leftToRight", "rightToLeft"] {
        let mut s = Session::new();
        s.execute("file.new", &json!({"preset": "Letter", "pages": 1, "columns": 2, "gutter": 18, "margins": 54})).unwrap();
        s.execute("layout.documentSetup", &json!({"binding": binding})).unwrap();
        let r = s.execute("file.place", &json!({"path": path.to_string_lossy(), "page": 1, "autoflow": true})).unwrap();
        assert_eq!(r["overset"], false, "{r}");
        let d = &s.doc().unwrap().doc;
        let frames = &d.story(StoryId(r["story"].as_u64().unwrap())).unwrap().frames;
        assert!(d.page_count() >= 3, "{binding}: {r}");
        // Two frames on every page but maybe the last, in column order, each on its column.
        assert!(frames.len() > 2 * (d.page_count() - 1), "{binding}: {} frames", frames.len());
        for (i, f) in frames.iter().enumerate() {
            let page = d.page_of_item(*f).unwrap();
            assert_eq!(page, i / 2, "{binding}: frame {i}");
            let mut cols = d.page(page).unwrap().column_rects();
            if binding == "rightToLeft" {
                cols.reverse();
            }
            assert_eq!(d.item(*f).unwrap().bounds(), cols[i % 2], "{binding}: frame {i}");
        }
    }
    // One column: one frame on the live area per page, as before.
    let mut s = Session::new();
    s.execute("file.new", &json!({"preset": "Letter", "pages": 1, "margins": 54})).unwrap();
    let r = s.execute("file.place", &json!({"path": path.to_string_lossy(), "page": 1, "autoflow": true})).unwrap();
    let d = &s.doc().unwrap().doc;
    let frames = &d.story(StoryId(r["story"].as_u64().unwrap())).unwrap().frames;
    assert_eq!(frames.len(), d.page_count());
    for f in frames {
        assert_eq!(d.item(*f).unwrap().bounds(), d.page(d.page_of_item(*f).unwrap()).unwrap().margin_rect());
    }
    let _ = std::fs::remove_dir_all(&dir);
}

/// Autoflow follows dragged column guides: one frame per dragged column, on the first page and
/// on every page it adds.
#[test]
fn autoflow_sets_frames_on_dragged_columns() {
    let dir = std::env::temp_dir().join(format!("dc-place-dragged-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("long.txt");
    std::fs::write(&path, vec!["Lorem ipsum dolor sit amet, consectetur adipiscing elit. ".repeat(12); 40].join("\n")).unwrap();
    let mut s = Session::new();
    s.execute("file.new", &json!({"preset": "Letter", "pages": 1, "columns": 2, "gutter": 18, "margins": 54})).unwrap();
    // The margin box is 504 pt wide; the gutter is dragged left. There is no command for
    // dragging a column guide yet, so set what an import would leave, on the parent.
    let dragged = designcraft_doc::Columns { count: 2, gutter: 18.0, positions: Some(vec![0.0, 150.0, 170.0, 504.0]) };
    s.edit(|d, _| {
        for pg in &mut std::sync::Arc::make_mut(&mut d.parents[0]).pages {
            pg.columns = dragged.clone();
        }
        d.sync_parent_margins();
        Ok(())
    })
    .unwrap();
    let r = s.execute("file.place", &json!({"path": path.to_string_lossy(), "page": 1, "autoflow": true})).unwrap();
    assert_eq!(r["overset"], false, "{r}");
    let d = &s.doc().unwrap().doc;
    assert!(d.page_count() >= 3, "{r}");
    let frames = &d.story(StoryId(r["story"].as_u64().unwrap())).unwrap().frames;
    assert!(frames.len() > 2 * (d.page_count() - 1), "{} frames", frames.len());
    for (i, f) in frames.iter().enumerate() {
        let page = d.page_of_item(*f).unwrap();
        assert_eq!(page, i / 2, "frame {i}");
        let m = d.page(page).unwrap().margin_rect();
        let (x0, x1) = [(0.0, 150.0), (170.0, 504.0)][i % 2];
        assert_eq!(d.item(*f).unwrap().bounds(), Rect::new(m.x0 + x0, m.y0, m.x0 + x1, m.y1), "frame {i}");
    }
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn autoflow_keeps_the_frame_the_user_drew() {
    let dir = std::env::temp_dir().join(format!("dc-place-drawn-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("long.txt");
    std::fs::write(&path, vec!["Lorem ipsum dolor sit amet, consectetur adipiscing elit. ".repeat(12); 30].join("\n")).unwrap();
    let mut s = Session::new();
    s.execute("file.new", &json!({"preset": "Letter", "pages": 1, "columns": 2, "gutter": 18, "margins": 54})).unwrap();
    let r = s.execute("file.place", &json!({"path": path.to_string_lossy(), "rect": [100, 100, 400, 300], "autoflow": true})).unwrap();
    assert_eq!(r["overset"], false, "{r}");
    let d = &s.doc().unwrap().doc;
    let frames = &d.story(StoryId(r["story"].as_u64().unwrap())).unwrap().frames;
    assert_eq!(d.item(frames[0]).unwrap().bounds(), Rect::new(100.0, 100.0, 400.0, 300.0));
    let cols = d.page(1).unwrap().column_rects();
    assert_eq!(d.page_of_item(frames[1]), Some(1));
    assert_eq!(d.item(frames[1]).unwrap().bounds(), cols[0]);
    assert_eq!(d.item(frames[2]).unwrap().bounds(), cols[1]);
    let _ = std::fs::remove_dir_all(&dir);
}

/// A facing-pages document with two columns, its inside margin wider than the outside one, so
/// left and right pages have their columns in different places. A graphic frame sits on the
/// page `marker` (0-based). Returns the session and the marker's id.
fn facing_document(pages: usize, marker: usize) -> (Session, u64) {
    let mut s = Session::new();
    let margins = json!({"top": 54, "bottom": 54, "inside": 72, "outside": 36});
    s.execute("file.new", &json!({"preset": "Letter", "pages": pages, "facingPages": true, "columns": 2, "gutter": 18, "margins": margins})).unwrap();
    let d = &s.doc().unwrap().doc;
    let (si, _) = d.page_loc(marker).unwrap();
    let m = d.page(marker).unwrap().margin_rect();
    let rect = [m.x0 + 20.0, m.y0 + 20.0, m.x0 + 120.0, m.y0 + 120.0];
    let id = s.execute("frame.create", &json!({"rect": rect, "spread": si, "content": "unassigned"})).unwrap()["id"].as_u64().unwrap();
    // Selected, an empty frame would take the placed text.
    s.execute("edit.deselectAll", &json!({})).unwrap();
    (s, id)
}

/// Every frame of the story on the column of its page, the story running through the pages in
/// order, two frames a page.
fn assert_frames_in_columns(d: &Document, frames: &[designcraft_doc::ItemId]) {
    for (i, f) in frames.iter().enumerate() {
        let page = d.page_of_item(*f).unwrap();
        assert_eq!(page, i / 2, "frame {i}");
        assert_eq!(d.item(*f).unwrap().bounds(), d.page(page).unwrap().column_rects()[i % 2], "frame {i}");
    }
}

#[test]
fn autoflow_fills_the_pages_that_follow_before_adding_any() {
    // Gap 26: autoflow inserted pages after the first frame's page, in front of the empty
    // pages the document already had. InDesign threads frames on those first.
    let dir = std::env::temp_dir().join(format!("dc-place-following-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("long.txt");
    std::fs::write(&path, vec!["Lorem ipsum dolor sit amet, consectetur adipiscing elit. ".repeat(12); 24].join("\n")).unwrap();
    let (mut s, marker) = facing_document(6, 2);
    let ids: Vec<_> = (0..6).map(|i| s.doc().unwrap().doc.page(i).unwrap().id).collect();
    let r = s.execute("file.place", &json!({"path": path.to_string_lossy(), "page": 1, "autoflow": true})).unwrap();
    assert_eq!(r["overset"], false, "{r}");
    assert_eq!(r["pagesAdded"], 0, "{r}");
    let d = &s.doc().unwrap().doc;
    assert_eq!((0..d.page_count()).map(|i| d.page(i).unwrap().id).collect::<Vec<_>>(), ids);
    let sid = StoryId(r["story"].as_u64().unwrap());
    let frames = d.story(sid).unwrap().frames.clone();
    // Past page 3, which holds the marker: frames go on a page whatever else it holds.
    assert!(d.page_of_item(*frames.last().unwrap()).unwrap() > 2, "{} frames", frames.len());
    assert_frames_in_columns(d, &frames);
    // The text ends before the document does: no empty frame after it.
    let cs = designcraft_compose::compose_story(d, sid, &Default::default());
    assert!(frames.iter().all(|f| cs.frame(*f).is_some_and(|ft| !ft.range.is_empty())));
    assert!(d.page_of_item(*frames.last().unwrap()).unwrap() < 5);
    assert_eq!(d.page_of_item(designcraft_doc::ItemId(marker)), Some(2));
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn autoflow_adds_pages_at_the_end_of_the_document() {
    let dir = std::env::temp_dir().join(format!("dc-place-end-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("long.txt");
    std::fs::write(&path, vec!["Lorem ipsum dolor sit amet, consectetur adipiscing elit. ".repeat(12); 60].join("\n")).unwrap();
    let (mut s, marker) = facing_document(3, 2);
    let ids: Vec<_> = (0..3).map(|i| s.doc().unwrap().doc.page(i).unwrap().id).collect();
    let r = s.execute("file.place", &json!({"path": path.to_string_lossy(), "page": 1, "autoflow": true})).unwrap();
    assert_eq!(r["overset"], false, "{r}");
    let d = &s.doc().unwrap().doc;
    assert!(d.page_count() > 4, "{r}");
    assert_eq!(r["pagesAdded"].as_u64(), Some(d.page_count() as u64 - 3), "{r}");
    // The document's own pages come first, the added ones after the last of them.
    assert_eq!((0..3).map(|i| d.page(i).unwrap().id).collect::<Vec<_>>(), ids);
    assert_eq!(d.page_of_item(designcraft_doc::ItemId(marker)), Some(2));
    let frames = d.story(StoryId(r["story"].as_u64().unwrap())).unwrap().frames.clone();
    assert_frames_in_columns(d, &frames);
    assert_eq!(d.page_of_item(*frames.last().unwrap()), Some(d.page_count() - 1));
    // One undo step takes the frames, the pages and the story away.
    s.execute("edit.undo", &json!({})).unwrap();
    let d = &s.doc().unwrap().doc;
    assert_eq!((0..d.page_count()).map(|i| d.page(i).unwrap().id).collect::<Vec<_>>(), ids);
    assert!(d.story(StoryId(r["story"].as_u64().unwrap())).is_none());
    assert_eq!(d.spreads.iter().map(|sp| sp.items.len()).sum::<usize>(), 1);
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn autoflow_fills_every_column_of_a_following_page() {
    // A first frame across the whole live area holds twice what a column does, so autoflow
    // asks for half the frames it needs. Asked page by page, it left the later columns of a
    // page out of the thread and went on to the next page.
    let dir = std::env::temp_dir().join(format!("dc-place-under-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("long.txt");
    std::fs::write(&path, vec!["Lorem ipsum dolor sit amet, consectetur adipiscing elit. ".repeat(12); 30].join("\n")).unwrap();
    let mut s = Session::new();
    s.execute("file.new", &json!({"preset": "Letter", "pages": 6, "facingPages": false, "columns": 2, "gutter": 18, "margins": 54})).unwrap();
    let m = s.doc().unwrap().doc.page(0).unwrap().margin_rect();
    let r = s.execute("file.place", &json!({"path": path.to_string_lossy(), "rect": [m.x0, m.y0, m.x1, m.y1], "autoflow": true})).unwrap();
    assert_eq!(r["overset"], false, "{r}");
    assert_eq!(r["pagesAdded"], 0, "{r}");
    let d = &s.doc().unwrap().doc;
    let frames = &d.story(StoryId(r["story"].as_u64().unwrap())).unwrap().frames;
    assert_eq!(d.item(frames[0]).unwrap().bounds(), m);
    assert!(d.page_of_item(*frames.last().unwrap()).unwrap() >= 3, "{} frames", frames.len());
    for (k, f) in frames[1..].iter().enumerate() {
        let page = d.page_of_item(*f).unwrap();
        assert_eq!(page, 1 + k / 2, "frame {}", k + 1);
        assert_eq!(d.item(*f).unwrap().bounds(), d.page(page).unwrap().column_rects()[k % 2], "frame {}", k + 1);
    }
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn autoflow_leaves_no_empty_frame_on_the_pages_that_follow() {
    // A small first frame makes autoflow ask for more frames than the columns need; the ones
    // it set on the following pages past the end of the text go again, the pages stay.
    let dir = std::env::temp_dir().join(format!("dc-place-small-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("long.txt");
    std::fs::write(&path, vec!["Lorem ipsum dolor sit amet, consectetur adipiscing elit. ".repeat(12); 12].join("\n")).unwrap();
    let mut s = Session::new();
    s.execute("file.new", &json!({"preset": "Letter", "pages": 8, "facingPages": false, "margins": 54})).unwrap();
    let r = s.execute("file.place", &json!({"path": path.to_string_lossy(), "rect": [100, 100, 200, 130], "autoflow": true})).unwrap();
    assert_eq!(r["overset"], false, "{r}");
    assert_eq!(r["pagesAdded"], 0, "{r}");
    let d = &s.doc().unwrap().doc;
    assert_eq!(d.page_count(), 8);
    let sid = StoryId(r["story"].as_u64().unwrap());
    let frames = &d.story(sid).unwrap().frames;
    assert!(frames.len() >= 2, "{r}");
    let cs = designcraft_compose::compose_story(d, sid, &Default::default());
    assert!(frames.iter().all(|f| cs.frame(*f).is_some_and(|ft| !ft.range.is_empty())), "{} frames", frames.len());
    assert_eq!(d.spreads.iter().map(|sp| sp.items.len()).sum::<usize>(), frames.len());
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn autoflow_adds_only_the_pages_breaks_ask_for() {
    // Regression (#262): a frame a break ends early made autoflow take it for a frame that
    // holds a few characters, and add a page per few characters left (33 for this text).
    let dir = std::env::temp_dir().join(format!("dc-place-breaks-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    for (name, rtf, pages) in [
        ("lead.rtf", r"{\rtf1 \page\par A Short Heading\par First short paragraph.\par Second short paragraph.\par}", 2),
        ("each.rtf", r"{\rtf1 One\par\page\par Two\par\page\par Three\par\page\par Four\par}", 4),
    ] {
        let path = dir.join(name);
        std::fs::write(&path, rtf).unwrap();
        let mut s = Session::new();
        s.execute("file.new", &json!({"pages": 1})).unwrap();
        let r = s.execute("file.place", &json!({"path": path.to_string_lossy(), "autoflow": true})).unwrap();
        assert_eq!(r["overset"], false, "{name}: {r}");
        let d = &s.doc().unwrap().doc;
        assert_eq!(d.page_count(), pages, "{name}: {r}");
        assert_eq!(d.story(StoryId(r["story"].as_u64().unwrap())).unwrap().frames.len(), pages, "{name}");
    }
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn autoflow_adds_the_pages_odd_and_even_page_breaks_ask_for() {
    // From page 1, an odd page break needs pages 2 and 3 (2 stays empty), an even one page 2.
    // A frame such a break ends early holds few characters; taken for a full frame, it made
    // autoflow add a page per few characters left.
    use designcraft_doc::{EVEN_PAGE_BREAK, ODD_PAGE_BREAK, build::NewDocument};
    for (brk, added) in [(ODD_PAGE_BREAK, 2), (EVEN_PAGE_BREAK, 1)] {
        let mut d = Document::new(&NewDocument { pages: 1, facing_pages: false, primary_text_frame: true, ..Default::default() });
        let sid = d.settings.primary_story.unwrap();
        d.story_mut(sid).unwrap().insert(
            0,
            &format!(
                "a{brk}
{}",
                "word ".repeat(200)
            ),
        );
        assert_eq!(autoflow(&mut d, sid, 50, "file.place", false).unwrap(), added, "{brk:?}");
        assert_eq!(d.page_count(), added + 1, "{brk:?}");
        assert_eq!(designcraft_compose::compose_story(&d, sid, &Default::default()).overset_at, None, "{brk:?}");
    }
}

#[test]
fn autoflow_sizes_no_frame_by_one_a_start_option_ends_early() {
    // Regression (#1209): the title frame a start option ends early isn't a frame's worth of text.
    use std::io::Write;
    const W: &str = r#"xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main""#;
    let para = |style: &str, text: &str| format!(r#"<w:p><w:pPr><w:pStyle w:val="{style}"/></w:pPr><w:r><w:t>{text}</w:t></w:r></w:p>"#);
    let body = [para("Title", "Front"), para("Chapter", "Chapter one"), para("Body", "lorem ipsum dolor sit amet ".repeat(60).trim())].concat();
    let styles: String = ["Title", "Chapter", "Body"]
        .iter()
        .map(|s| format!(r#"<w:style w:type="paragraph" w:styleId="{s}"><w:name w:val="{s}"/></w:style>"#))
        .collect();
    let mut w = zip::ZipWriter::new(std::io::Cursor::new(Vec::new()));
    let o = zip::write::SimpleFileOptions::default();
    for (n, c) in [
        ("word/document.xml", format!(r#"<w:document {W}><w:body>{body}</w:body></w:document>"#)),
        ("word/styles.xml", format!(r#"<w:styles {W}>{styles}</w:styles>"#)),
    ] {
        w.start_file(n, o).unwrap();
        w.write_all(c.as_bytes()).unwrap();
    }
    let docx = w.finish().unwrap().into_inner();
    for (start, pages) in [("nextOddPage", 5), ("nextEvenPage", 4)] {
        let mut s = Session::new();
        s.execute("file.new", &json!({"pages": 3, "facingPages": true})).unwrap();
        s.execute("style.paragraph.create", &json!({"name": "Chapter", "para": {"startParagraph": start}})).unwrap();
        let ids = s.doc().unwrap().doc.next_id;
        let r = s
            .execute("file.place", &json!({"base64": super::super::file::base64_encode(&docx), "name": "front.docx", "page": 3, "autoflow": true}))
            .unwrap();
        assert_eq!(r["overset"], false, "{start}: {r}");
        let d = &s.doc().unwrap().doc;
        assert_eq!(d.page_count(), pages, "{start}: {r}");
        // A few ids per page it keeps, not hundreds for pages it builds and removes.
        assert!(d.next_id - ids < 100, "{start}: {} ids", d.next_id - ids);
    }
}

#[test]
fn autoflow_looks_past_a_deleted_paragraph_for_the_start_option() {
    // The title frame ends before a tracked deletion; the chapter after it starts on an odd page.
    use designcraft_doc::{ChangeMark, build::NewDocument};
    let mut d = Document::new(&NewDocument { pages: 1, facing_pages: false, primary_text_frame: true, ..Default::default() });
    let sid = d.settings.primary_story.unwrap();
    let st = d.story_mut(sid).unwrap();
    st.insert(0, &format!("Front\nGone\nChapter one\n{}", "lorem ipsum dolor sit amet ".repeat(400).trim()));
    st.paras[2].para.start_paragraph = Some(StartParagraph::NextOddPage);
    st.format_chars(6..11, |f| f.over.change = Some(ChangeMark::Deleted));
    let ids = d.next_id;
    assert_eq!(autoflow(&mut d, sid, 2000, "file.place", false).unwrap(), 4);
    assert_eq!(designcraft_compose::compose_story(&d, sid, &Default::default()).overset_at, None);
    assert!(d.next_id - ids < 100, "{} ids", d.next_id - ids);
}

#[test]
fn autoflow_asks_for_every_frame_the_start_options_ask_for_at_once() {
    // A paragraph per page: no frame sets a fill rate, yet one round shouldn't add one frame.
    use designcraft_doc::build::NewDocument;
    let mut d = Document::new(&NewDocument { pages: 1, facing_pages: false, primary_text_frame: true, ..Default::default() });
    let sid = d.settings.primary_story.unwrap();
    let st = d.story_mut(sid).unwrap();
    st.insert(0, (0..200).map(|i| format!("Chapter {i}\n")).collect::<String>().trim_end());
    for p in &mut st.paras {
        p.para.start_paragraph = Some(StartParagraph::NextPage);
    }
    let ids = d.next_id;
    assert_eq!(autoflow(&mut d, sid, 2000, "file.place", false).unwrap(), 199);
    assert_eq!(designcraft_compose::compose_story(&d, sid, &Default::default()).overset_at, None);
    assert!(d.next_id - ids < 2000, "{} ids", d.next_id - ids);
}

#[test]
fn autoflow_asks_no_frame_for_a_deleted_or_hidden_break() {
    // Composing skips breaks in the overset text that tracked changes delete or a condition hides.
    use designcraft_doc::{ChangeMark, Condition, PAGE_BREAK, build::NewDocument};
    for hide in [false, true] {
        let mut d = Document::new(&NewDocument { pages: 1, facing_pages: false, primary_text_frame: true, ..Default::default() });
        d.conditions.push(Condition { name: "Hidden".into(), color: [0, 0, 0], visible: false });
        let sid = d.settings.primary_story.unwrap();
        let st = d.story_mut(sid).unwrap();
        let body = "lorem ipsum dolor sit amet ".repeat(300);
        let gone = format!("x{PAGE_BREAK}\n").repeat(200);
        st.insert(0, &format!("{}\n{gone}{}", body.trim(), body.trim()));
        let at = body.trim().len() + 1;
        if hide {
            for (i, _) in gone.match_indices(PAGE_BREAK) {
                st.format_chars(at + i..at + i + PAGE_BREAK.len_utf8(), |f| f.over.conditions = Some(vec!["Hidden".into()]));
            }
        } else {
            st.format_chars(at..at + gone.len(), |f| f.over.change = Some(ChangeMark::Deleted));
        }
        let ids = d.next_id;
        autoflow(&mut d, sid, 2000, "file.place", false).unwrap();
        assert_eq!(designcraft_compose::compose_story(&d, sid, &Default::default()).overset_at, None, "hide {hide}");
        assert!(d.next_id - ids < 100, "hide {hide}: {} pages, {} ids", d.page_count(), d.next_id - ids);
    }
}

#[test]
fn autoflow_counts_a_break_and_the_start_option_it_meets_once() {
    // Each chapter ends in a page break and starts on the next page: one new page each, not two.
    use designcraft_doc::{PAGE_BREAK, build::NewDocument};
    let mut d = Document::new(&NewDocument { pages: 1, facing_pages: false, primary_text_frame: true, ..Default::default() });
    let sid = d.settings.primary_story.unwrap();
    let st = d.story_mut(sid).unwrap();
    st.insert(0, (0..200).map(|i| format!("Chapter {i}{PAGE_BREAK}\n")).collect::<String>().trim_end());
    for p in &mut st.paras {
        p.para.start_paragraph = Some(StartParagraph::NextPage);
    }
    let ids = d.next_id;
    assert_eq!(autoflow(&mut d, sid, 2000, "file.place", false).unwrap(), 199);
    assert_eq!(designcraft_compose::compose_story(&d, sid, &Default::default()).overset_at, None);
    assert!(d.next_id - ids < 2000, "{} ids", d.next_id - ids);
}
