use crate::*;
use designcraft_doc::{InsetColumns, InsetStream, NotePlacement, NoteSeries, SpreadRef, build::NewDocument};

fn document(pages: usize, main: &str, left: &str, right: &str) -> (Document, StoryId) {
    let mut doc = Document::new(&NewDocument { pages, width: 672.0, height: 872.0, facing_pages: true, ..Default::default() });
    for id in [1, 2] {
        doc.note_series.push(NoteSeries {
            id,
            name: format!("Commentary {id}"),
            options: designcraft_doc::FootnoteOptions {
                placement: NotePlacement::Stream,
                ref_hidden: true,
                first_baseline: designcraft_doc::FirstBaseline::Fixed,
                first_baseline_min: 10.0,
                space_before: 0.0,
                space_between: 0.0,
                separator: " ".into(),
                ..Default::default()
            },
        });
    }
    let mut host = None;
    let mut previous = None;
    for page in 0..pages {
        let (si, pi) = doc.page_loc(page).unwrap();
        let x = doc.spreads[si].pages[pi].x;
        let layer = doc.default_layer();
        let (id, sid) = doc
            .add_text_frame(
                SpreadRef::Doc(si),
                Rect::new(x + 36.0, 36.0, x + 636.0, 836.0),
                layer,
                if page == 0 { main } else { "" },
                Default::default(),
            )
            .unwrap();
        if let Some(p) = previous {
            doc.thread(p, id).unwrap();
        }
        let _ = host.get_or_insert(sid);
        previous = Some(id);
        doc.item_mut(id).unwrap().text_frame_mut().unwrap().options.inset_columns = Some(InsetColumns {
            streams: vec![
                InsetStream { series: Some(1), ratio: 1.0 },
                InsetStream { series: None, ratio: 1.0 },
                InsetStream { series: Some(2), ratio: 1.0 },
            ],
            gutter: 12.0,
            main_start_lines: 0,
            main_left_inset: None,
            main_right_inset: None,
        });
    }
    let sid = host.unwrap();
    for (series, text, leading) in [(2, right, 17.0), (1, left, 11.0)] {
        if text.is_empty() {
            continue;
        }
        let story = doc.story_mut(sid).unwrap();
        let id = story.insert_note(0, text, Default::default()).unwrap();
        let note = story.note_mut(id).unwrap();
        note.series = series;
        note.text.format_chars(0..text.len(), |f| {
            f.over.size = Some(9.0);
            f.over.leading = Some(designcraft_doc::Leading::Points(leading));
        });
    }
    (doc, sid)
}

fn no_overlap(cs: &ComposedStory) {
    for frame in &cs.frames {
        let mut lines: Vec<(u64, Rect)> =
            frame.lines.iter().map(|l| (0, Rect::new(l.x0, l.baseline - l.ascent, l.x1, l.baseline + l.descent))).collect();
        for note in &frame.notes {
            lines.extend(note.text.frames.iter().flat_map(|f| f.lines.iter()).map(|l| {
                (
                    note.series,
                    Rect::new(
                        note.origin.x + l.x0,
                        note.origin.y + l.baseline - l.ascent,
                        note.origin.x + l.x1,
                        note.origin.y + l.baseline + l.descent,
                    ),
                )
            }));
        }
        for (i, (stream, a)) in lines.iter().enumerate() {
            for (other, b) in lines.iter().skip(i + 1) {
                if stream == other {
                    continue;
                }
                assert!(
                    a.x1 <= b.x0 + 0.01 || b.x1 <= a.x0 + 0.01 || a.y1 <= b.y0 + 0.01 || b.y1 <= a.y0 + 0.01,
                    "streams {stream}/{other} overlap {a:?} {b:?}"
                );
            }
        }
    }
}

fn continuity(cs: &ComposedStory) {
    no_overlap(cs);
    for frame in &cs.frames {
        for pair in frame.lines.windows(2) {
            assert!(
                (pair[1].baseline - pair[0].baseline - pair[1].leading).abs() < 0.01,
                "main baseline gap {:?}",
                (pair[0].baseline, pair[1].baseline)
            );
            assert_eq!(pair[0].range.end, pair[1].range.start);
        }
        for series in [1, 2] {
            let parts: Vec<_> = frame.notes.iter().filter(|n| n.series == series).collect();
            let lines: Vec<_> = parts
                .iter()
                .flat_map(|p| p.text.frames.iter().flat_map(move |f| f.lines.iter().map(move |l| (p.origin.y + l.baseline, l.leading))))
                .collect();
            for pair in lines.windows(2) {
                assert!((pair[1].0 - pair[0].0 - pair[1].1).abs() < 0.01, "series {series} gap {pair:?}");
            }
            for pair in parts.windows(2) {
                assert_eq!(pair[0].text.frames[0].range.end, pair[1].text.frames[0].range.start);
            }
        }
    }
}

#[test]
fn real_text_expands_main_on_both_sides_without_restarting_baselines() {
    let main = "Main text has a continuous sequence of words across changing column widths. ".repeat(35);
    let (doc, sid) = document(2, &main, &"Left line.\u{2028}".repeat(7), &"Right line.\u{2028}".repeat(16));
    let cs = compose_story(&doc, sid, &Default::default());
    assert!(!cs.is_overset(), "{:?}", cs.warnings);
    assert!(!cs.frames[0].notes.is_empty(), "{:?}", cs.warnings);
    assert!(cs.frames[0].lines.iter().any(|l| l.x0 == 240.0));
    assert!(cs.frames[0].lines.iter().any(|l| l.x0 == 36.0 && l.x1 == 432.0));
    assert!(cs.frames[0].lines.iter().any(|l| l.x0 == 36.0 && l.x1 == 636.0));
    continuity(&cs);
}

#[test]
fn positioned_main_and_mirrored_regions_keep_continuous_text() {
    let main = "Positioned main text keeps its own leading while commentary changes width. ".repeat(14);
    let (mut doc, sid) = document(1, &main, &"Left commentary sentence. ".repeat(60), &"Right commentary sentence. ".repeat(60));
    let id = doc.story(sid).unwrap().frames[0];
    let options = doc.item_mut(id).unwrap().text_frame_mut().unwrap().options.inset_columns.as_mut().unwrap();
    options.main_start_lines = 10;
    options.main_left_inset = Some(210.0);
    options.main_right_inset = Some(162.0);
    let frames = frame_specs(&doc, sid);
    let normal = compose(&doc, doc.story(sid).unwrap(), &frames, &Default::default());
    assert!(!normal.frames[0].lines.is_empty(), "{:?}", normal.warnings);
    assert_eq!(normal.frames[0].lines[0].x0, 246.0);
    assert_eq!(normal.frames[0].lines[0].x1, 474.0);
    let mut mirrored = frames.clone();
    mirrored[0].left_page = true;
    let mirror = compose(&doc, doc.story(sid).unwrap(), &mirrored, &Default::default());
    assert_eq!(normal.frames[0].lines.len(), mirror.frames[0].lines.len());
    for (a, b) in normal.frames[0].lines.iter().zip(&mirror.frames[0].lines) {
        assert_eq!(a.range, b.range);
        assert!((a.x0 - (672.0 - b.x1)).abs() < 0.01);
    }
    continuity(&normal);
    continuity(&mirror);
}

#[test]
fn main_and_comment_sources_partition_across_three_pages() {
    let (doc, sid) = document(
        3,
        &"Main passage with several words and stable source offsets. ".repeat(150),
        &"Inner commentary words. ".repeat(90),
        &"Outer commentary words. ".repeat(110),
    );
    let cs = compose_story(&doc, sid, &Default::default());
    assert!(!cs.is_overset(), "{:?}", cs.warnings);
    let mut end = 0;
    for frame in &cs.frames {
        for line in &frame.lines {
            assert_eq!(line.range.start, end);
            end = line.range.end;
        }
    }
    assert_eq!(end, doc.story(sid).unwrap().text.len());
    for note in &doc.story(sid).unwrap().notes {
        let mut end = 0;
        for part in cs.frames.iter().flat_map(|f| &f.notes).filter(|n| n.id == note.id) {
            assert_eq!(part.text.frames[0].range.start, end);
            end = part.text.frames[0].range.end;
        }
        assert_eq!(end, note.text.len());
    }
    continuity(&cs);
}

#[test]
fn empty_series_has_no_column_and_direction_does_not_choose_regions() {
    let main = "Main text occupies the available region regardless of paragraph direction. ".repeat(20);
    let (mut doc, sid) = document(1, &main, "", &"Outer line.\u{2028}".repeat(6));
    let ltr = compose_story(&doc, sid, &Default::default());
    assert_eq!(ltr.frames[0].lines[0].x0, 36.0);
    assert_eq!(ltr.frames[0].lines[0].x1, 330.0);
    doc.story_mut(sid).unwrap().direction = designcraft_doc::TextDirection::RightToLeft;
    let rtl = compose_story(&doc, sid, &Default::default());
    assert_eq!(ltr.frames[0].columns, rtl.frames[0].columns);
}

#[test]
fn editing_commentary_on_page_three_keeps_the_first_two_pages() {
    let (mut doc, sid) = document(
        4,
        &"Main text continues in its column. ".repeat(60),
        &"Inner commentary line.\u{2028}".repeat(260),
        &"Outer commentary line.\u{2028}".repeat(260),
    );
    let cache = Cache::new();
    let before = cache.get(&doc, sid, None);
    assert!(!before.frames[2].notes.is_empty(), "{:?}", before.warnings);
    let part = before.frames[2].notes.iter().find(|n| n.series == 1).unwrap();
    let at = part.text.frames[0].range.start + 5;
    doc.story_mut(sid).unwrap().note_mut(part.id).unwrap().text.replace(at..at, "Added paragraph.\n");
    super::fitting::REFITTED.with(|v| v.borrow_mut().clear());
    let after = cache.get(&doc, sid, None);
    let refitted = super::fitting::REFITTED.with(|v| v.borrow().clone());
    assert!(!refitted.contains(&before.frames[0].frame), "{refitted:?}");
    assert!(!refitted.contains(&before.frames[1].frame), "{refitted:?}");
    assert!(refitted.contains(&before.frames[2].frame), "{refitted:?}");
    for index in 0..2 {
        assert_eq!(before.frames[index].columns, after.frames[index].columns);
        assert_eq!(
            before.frames[index].lines.iter().map(|l| (&l.range, l.baseline)).collect::<Vec<_>>(),
            after.frames[index].lines.iter().map(|l| (&l.range, l.baseline)).collect::<Vec<_>>()
        );
    }
}

#[test]
fn impossible_start_only_overflows_comments_on_the_actual_first_main_line() {
    let main = "Main text proceeds through a sequence of words while its comments need room. ".repeat(20);
    let (mut doc, sid) = document(2, &main, &"Long inner line.\u{2028}".repeat(200), "");
    for series in &mut doc.note_series {
        series.options.allow_split = false;
    }
    let id = doc.story_mut(sid).unwrap().insert_note(80, &"Long outer line.\u{2028}".repeat(200), Default::default()).unwrap();
    doc.story_mut(sid).unwrap().note_mut(id).unwrap().series = 2;
    let cs = compose_story(&doc, sid, &Default::default());
    for frame in &cs.frames {
        if let Some(first) = frame.lines.first() {
            for note in frame.notes.iter().filter(|n| n.overflow) {
                let at = doc.story(sid).unwrap().note_anchor(note.id).unwrap();
                assert!(first.range.contains(&at), "overflow anchor {at} outside first main line {:?}", first.range);
            }
        }
    }
    assert!(!cs.frames[0].lines.is_empty(), "{:?}", cs.warnings);
    assert!(cs.frames[0].notes.iter().any(|n| n.overflow));
    assert!(!cs.frames[0].notes.iter().any(|n| n.id == id));
}

#[test]
fn one_full_width_key_can_finish_several_narrow_main_lines() {
    let (doc, sid) = document(
        1,
        "A short main paragraph can wrap into several narrow lines beside its commentary.",
        &"Inner line.\u{2028}".repeat(20),
        &"Outer line.\u{2028}".repeat(25),
    );
    let cs = compose_story(&doc, sid, &Default::default());
    assert!(!cs.is_overset(), "{:?}", cs.warnings);
    assert!(cs.frames[0].lines.len() > 1);
    assert_eq!(cs.frames[0].lines.last().unwrap().range.end, doc.story(sid).unwrap().text.len());
    continuity(&cs);
}

#[test]
fn separated_main_format_edits_match_fresh_page_fitting() {
    let main: String = (0..75).map(|i| format!("Main line {i:03} words.\u{2028}")).collect();
    let (mut doc, sid) = document(3, &main, "", "");
    for id in doc.story(sid).unwrap().frames.clone() {
        let item = doc.item_mut(id).unwrap();
        let b = item.bounds();
        item.path = designcraft_geom::shapes::rectangle(Rect::new(b.x0, b.y0, b.x1, b.y0 + 400.0));
    }
    let cache = Cache::new();
    let before = cache.get(&doc, sid, None);
    assert!(!before.is_overset());
    let late = main.find("Main line 065").unwrap();
    let story = doc.story_mut(sid).unwrap();
    story.format_chars(0..4, |f| f.over.fill = Some("Paper".into()));
    story.format_chars(late..main.len(), |f| f.over.leading = Some(designcraft_doc::Leading::Points(40.0)));
    let warm = cache.get(&doc, sid, None);
    let fresh = compose_story(&doc, sid, &Default::default());
    assert!(fresh.is_overset());
    assert_eq!(warm.overset_at, fresh.overset_at);
    for (a, b) in warm.frames.iter().zip(&fresh.frames) {
        assert_eq!(a.range, b.range);
        assert_eq!(a.columns, b.columns);
        assert_eq!(
            a.lines.iter().map(|l| (&l.range, l.baseline)).collect::<Vec<_>>(),
            b.lines.iter().map(|l| (&l.range, l.baseline)).collect::<Vec<_>>()
        );
    }
}

#[test]
fn commentary_keep_all_crosses_bands_on_one_physical_page() {
    let (mut doc, sid) = document(1, "Main one.\u{2028}Main two.\u{2028}Main three.", &"Commentary line.\u{2028}".repeat(20), "");
    let ordinary = compose_story(&doc, sid, &Default::default());
    let note = doc.story_mut(sid).unwrap().notes.first_mut().unwrap();
    let note = std::sync::Arc::make_mut(note);
    let len = note.text.len();
    note.text.format_paras(0..len, |p| {
        p.para.keep_lines_together = Some(true);
        p.para.keep_all_lines = Some(true);
    });
    let kept = compose_story(&doc, sid, &Default::default());
    let lines = |cs: &ComposedStory| {
        cs.frames
            .iter()
            .flat_map(|f| &f.notes)
            .flat_map(|n| n.text.frames.iter().flat_map(move |f| f.lines.iter().map(move |l| (l.range.clone(), n.origin.y + l.baseline))))
            .collect::<Vec<_>>()
    };
    assert!(ordinary.frames[0].notes.len() > 1, "fixture must cross a width event");
    assert_eq!(lines(&ordinary), lines(&kept));
    assert!(kept.frames.iter().flat_map(|f| &f.notes).flat_map(|n| &n.text.frames).flat_map(|f| &f.lines).all(|l| !l.keep_violation));
    no_overlap(&kept);
}
