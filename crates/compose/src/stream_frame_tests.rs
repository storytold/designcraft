use super::*;
use designcraft_doc::{FOOTNOTE_REF, NoteStream, SpreadRef, build::NewDocument};

fn commentary(pages: usize, count: usize, height: f64) -> (Document, StoryId, Vec<ItemId>) {
    let mut doc = Document::new(&NewDocument { pages, ..Default::default() });
    let layer = doc.default_layer();
    let main = (0..count).map(|i| format!("Passage {i}.\n")).collect::<String>();
    doc.note_series.push(designcraft_doc::NoteSeries {
        id: 1,
        name: "Commentary".into(),
        options: designcraft_doc::FootnoteOptions {
            placement: designcraft_doc::NotePlacement::Stream,
            ref_hidden: true,
            separator: " ".into(),
            ..Default::default()
        },
    });
    let mut host = None;
    let mut last = None;
    let mut streams = Vec::new();
    for page in 0..pages {
        let (si, pi) = doc.page_loc(page).unwrap();
        let x = doc.spreads[si].pages[pi].x;
        let (id, sid) = doc
            .add_text_frame(
                SpreadRef::Doc(si),
                Rect::new(x + 36.0, 36.0, x + 280.0, 336.0),
                layer,
                if page == 0 { &main } else { "" },
                Default::default(),
            )
            .unwrap();
        if let Some(previous) = last {
            doc.thread(previous, id).unwrap();
        }
        let sid = *host.get_or_insert(sid);
        last = Some(id);
        let (stream, _) =
            doc.add_text_frame(SpreadRef::Doc(si), Rect::new(x + 300.0, 36.0, x + 540.0, 36.0 + height), layer, "", Default::default()).unwrap();
        doc.item_mut(stream).unwrap().text_frame_mut().unwrap().options.note_stream = Some(NoteStream { story: sid, series: 1 });
        streams.push(stream);
    }
    let host = host.unwrap();
    let story = doc.story_mut(host).unwrap();
    for i in (0..count).rev() {
        let at = main.find(&format!("Passage {i}.")).unwrap() + format!("Passage {i}.").len();
        let id = story.insert_note(at, &format!("Comment {i}.\u{2028}Second line."), Default::default()).unwrap();
        story.note_mut(id).unwrap().series = 1;
    }
    (doc, host, streams)
}

fn check_starts(doc: &Document, host: StoryId, cs: &ComposedStory) {
    let body = frame_specs(doc, host);
    let routes = stream_frame_specs(doc, host);
    for note in &doc.story(host).unwrap().notes {
        let at = doc.story(host).unwrap().note_anchor(note.id).unwrap();
        let anchor_page = body.iter().find(|f| cs.frame(f.id).unwrap().lines.iter().any(|line| line.range.contains(&at))).unwrap().page;
        let first = cs.frames.iter().find(|f| f.notes.iter().any(|n| n.id == note.id && n.text.frames[0].range.start == 0)).unwrap();
        let note_page = routes.iter().find(|f| f.id == first.frame).unwrap().page;
        assert_eq!(anchor_page, note_page, "note {} starts on its anchor's page", note.id);
    }
}

#[test]
fn ten_comment_starts_follow_anchor_pages_and_reflow_after_an_edit() {
    let (mut doc, host, streams) = commentary(3, 10, 135.0);
    let cs = compose_story(&doc, host, &Default::default());
    assert!(!cs.is_overset(), "{:?}", cs.overset_at);
    check_starts(&doc, host, &cs);
    assert!(streams.iter().all(|id| !cs.frame(*id).unwrap().notes.is_empty()));
    let before = cs.frame(streams[0]).unwrap().notes.len();
    let first = doc.story(host).unwrap().notes[0].id;
    let note = doc.story_mut(host).unwrap().note_mut(first).unwrap();
    note.text.replace(0..note.text.len(), "Lengthened comment.\u{2028}Second line.\u{2028}Third line.\u{2028}Fourth line.");
    let after = compose_story(&doc, host, &Default::default());
    assert!(!after.is_overset());
    check_starts(&doc, host, &after);
    assert!(after.frame(streams[0]).unwrap().notes.len() < before);
    for frame in &after.frames {
        for glyph in frame
            .lines
            .iter()
            .flat_map(|l| &l.glyphs)
            .filter(|g| g.len > 0 && doc.story(host).unwrap().text.get(g.byte..).is_some_and(|text| text.starts_with(FOOTNOTE_REF)))
        {
            assert_eq!(glyph.adv, 0.0);
            assert!(!glyph.visible);
        }
    }
}

#[test]
fn continuation_precedes_new_comments_and_retains_source_ranges_and_keeps() {
    let (mut doc, host, streams) = commentary(3, 2, 95.0);
    let first = doc.story(host).unwrap().notes[0].id;
    let note = doc.story_mut(host).unwrap().note_mut(first).unwrap();
    let text = (0..12).map(|i| format!("Line {i}.")).collect::<Vec<_>>().join("\u{2028}");
    note.text.replace(0..note.text.len(), &text);
    note.text.paras[0].para.keep_lines_together = Some(true);
    note.text.paras[0].para.keep_first = Some(2);
    note.text.paras[0].para.keep_last = Some(2);
    let cs = compose_story(&doc, host, &Default::default());
    assert!(!cs.is_overset());
    check_starts(&doc, host, &cs);
    let parts: Vec<_> = streams.iter().flat_map(|id| &cs.frame(*id).unwrap().notes).filter(|n| n.id == first).collect();
    assert!(parts.len() >= 2);
    let mut end = 0;
    for part in parts {
        let frame = &part.text.frames[0];
        assert_eq!(frame.range.start, end);
        assert!(frame.lines.len() >= 2);
        end = frame.range.end;
    }
    assert_eq!(end, text.len());
    let second = cs.frame(streams[1]).unwrap();
    assert_eq!(second.notes[0].id, first);
    assert!(!cs.frame(streams[0]).unwrap().notes.iter().any(|n| n.id != first));
}

#[test]
fn route_continues_without_a_host_frame_and_overflow_does_not_set_main_overset() {
    let (mut doc, host, streams) = commentary(3, 1, 60.0);
    let frames = doc.story(host).unwrap().frames.clone();
    for frame in frames.into_iter().skip(1) {
        doc.remove_from_thread(frame).unwrap();
    }
    let first = doc.story(host).unwrap().notes[0].id;
    let note = doc.story_mut(host).unwrap().note_mut(first).unwrap();
    note.text.replace(0..note.text.len(), &"Long commentary line.\u{2028}".repeat(40));
    let cs = compose_story(&doc, host, &Default::default());
    assert_eq!(cs.overset_at, None);
    assert!(streams.iter().all(|id| !cs.frame(*id).unwrap().notes.is_empty()));
    assert!(cs.frame(*streams.last().unwrap()).unwrap().notes.last().unwrap().overflow);
    assert!(cs.warnings.iter().any(|w| w.contains("Stream overflow in series 1")));
}

#[test]
fn impossible_start_overflows_without_stalling_and_removing_routes_uses_column_foot() {
    let (mut doc, host, streams) = commentary(2, 1, 1.0);
    doc.note_series[0].options.allow_split = false;
    let cs = compose_story(&doc, host, &Default::default());
    assert!(!cs.is_overset());
    assert!(cs.frame(streams[0]).unwrap().notes[0].overflow);
    assert!(cs.warnings.iter().any(|w| w.contains("cannot start")));
    for id in streams {
        doc.item_mut(id).unwrap().text_frame_mut().unwrap().options.note_stream = None;
    }
    let cs = compose_story(&doc, host, &Default::default());
    assert_eq!(cs.frames.iter().flat_map(|f| &f.notes).count(), 1);
    assert!(cs.warnings.iter().any(|w| w.contains("use the column foot")));
    assert!(cs.frames[0].notes[0].rect.y0 > 250.0);
}

#[test]
fn a_full_continuation_allows_unanchored_main_lines_but_moves_the_next_anchor() {
    let (mut doc, host, streams) = commentary(3, 2, 95.0);
    let first = doc.story(host).unwrap().notes[0].id;
    let story = doc.story_mut(host).unwrap();
    let next_line = story.text.find('\n').unwrap() + 1;
    story.replace(next_line..next_line, "Unanchored one.\nUnanchored two.\nUnanchored three.\nUnanchored four.\n");
    let note = story.note_mut(first).unwrap();
    note.text.replace(0..note.text.len(), &(0..12).map(|i| format!("Line {i}.\u{2028}")).collect::<String>());
    for frame in doc.story(host).unwrap().frames.clone() {
        let item = doc.item_mut(frame).unwrap();
        let rect = item.inner_bounds();
        item.path = designcraft_geom::shapes::rectangle(Rect::new(rect.x0, rect.y0, rect.x1, rect.y0 + 50.0));
    }
    assert_eq!(frame_specs(&doc, host).iter().map(|f| f.page).collect::<Vec<_>>(), [Some(0), Some(1), Some(2)]);
    let cs = compose_story(&doc, host, &Default::default());
    assert!(!cs.is_overset());
    check_starts(&doc, host, &cs);
    let second_body = cs.frame(doc.story(host).unwrap().frames[1]).unwrap();
    assert!(!second_body.lines.is_empty());
    assert!(
        second_body.lines.iter().flat_map(|l| &l.glyphs).all(|g| !doc
            .story(host)
            .unwrap()
            .text
            .get(g.byte..)
            .is_some_and(|s| s.starts_with(FOOTNOTE_REF)))
    );
    assert_eq!(cs.frame(streams[1]).unwrap().notes[0].id, first);
}

#[test]
fn two_streams_fit_the_same_main_page_cut() {
    let (mut doc, host, streams) = commentary(3, 10, 70.0);
    let mut second = doc.note_series[0].clone();
    second.id = 2;
    second.name = "Outer commentary".into();
    doc.note_series.push(second);
    for (i, note) in doc.story_mut(host).unwrap().notes.iter_mut().enumerate() {
        if i % 2 == 1 {
            Arc::make_mut(note).series = 2;
        }
    }
    let mut outer = Vec::new();
    for first in streams {
        let loc = doc.find(first).unwrap();
        let bounds = doc.item(first).unwrap().bounds();
        let (id, _) = doc.add_text_frame(loc.spread, bounds, doc.default_layer(), "", Default::default()).unwrap();
        doc.item_mut(id).unwrap().text_frame_mut().unwrap().options.note_stream = Some(NoteStream { story: host, series: 2 });
        outer.push(id);
    }
    let cs = compose_story(&doc, host, &Default::default());
    assert!(!cs.is_overset());
    check_starts(&doc, host, &cs);
    assert!(outer.iter().all(|id| !cs.frame(*id).unwrap().notes.is_empty()));
}

#[test]
fn page_trials_do_not_consume_a_rejected_cut() {
    let (doc, host, _) = commentary(2, 4, 65.0);
    let story = doc.story(host).unwrap();
    let areas: Vec<_> = stream_frame_specs(&doc, host).into_iter().filter(|f| f.page == Some(0)).collect();
    let before = StreamState::default();
    let full = fit_stream_page(
        &doc,
        story,
        StreamPageInput { page: 0, main: 0..story.len(), areas: &areas, force_starts: None },
        &before,
        &Default::default(),
    );
    assert!(!full.starts_fit);
    let end = story.note_anchor(story.notes[1].id).unwrap();
    let short =
        fit_stream_page(&doc, story, StreamPageInput { page: 0, main: 0..end, areas: &areas, force_starts: None }, &before, &Default::default());
    assert!(short.starts_fit);
    assert_eq!(short.frames.iter().flat_map(|f| &f.notes).count(), 1);
    assert_eq!(before.pending_series().count(), 0);
}

#[test]
fn width_changes_on_the_stream_route_keep_every_source_byte_once() {
    let (mut doc, host, streams) = commentary(3, 1, 95.0);
    let id = doc.story(host).unwrap().notes[0].id;
    let note = doc.story_mut(host).unwrap().note_mut(id).unwrap();
    let text = "A comment changes width while keeping every source byte. ".repeat(8);
    note.text.replace(0..note.text.len(), &text);
    note.text.paras[0].para.keep_lines_together = Some(true);
    note.text.paras[0].para.keep_first = Some(2);
    note.text.paras[0].para.keep_last = Some(2);
    for (frame, width) in streams.iter().zip([240.0, 110.0, 300.0]) {
        let item = doc.item_mut(*frame).unwrap();
        let area = item.inner_bounds();
        item.path = designcraft_geom::shapes::rectangle(Rect::new(area.x0, area.y0, area.x0 + width, area.y1));
    }
    let cs = compose_story(&doc, host, &Default::default());
    let mut end = 0;
    let mut count = 0;
    for part in cs.frames.iter().flat_map(|f| &f.notes).filter(|n| n.id == id) {
        let frame = &part.text.frames[0];
        assert_eq!(frame.range.start, end);
        assert!(frame.lines.len() >= 2);
        end = frame.range.end;
        count += 1;
    }
    assert!(count > 1);
    assert_eq!(end, text.len());
}

#[test]
fn inherited_two_plus_two_keeps_refuse_a_one_line_start_in_remaining_space() {
    let (mut doc, host, streams) = commentary(2, 4, 110.0);
    let style = Arc::make_mut(&mut doc.styles).para_mut(designcraft_doc::story::BASIC_PARAGRAPH).unwrap();
    style.para.keep_lines_together = Some(true);
    style.para.keep_first = Some(2);
    style.para.keep_last = Some(2);
    for note in &mut doc.story_mut(host).unwrap().notes {
        let note = Arc::make_mut(note);
        note.text.replace(0..note.text.len(), "First line.\u{2028}Second line.\u{2028}Third line.");
    }
    let cs = compose_story(&doc, host, &Default::default());
    assert!(!cs.is_overset());
    check_starts(&doc, host, &cs);
    for note in cs.frames.iter().flat_map(|f| &f.notes) {
        assert!(note.text.frames[0].lines.len() >= 2, "a 2+2 comment must move rather than start with one line");
    }
    assert_eq!(cs.frame(streams[0]).unwrap().notes.len(), 2);
}

#[test]
fn adopted_stream_series_without_a_destination_route_warns_and_uses_column_foot() {
    let (source, host, _) = commentary(1, 1, 110.0);
    let mut dest = Document::new(&NewDocument::default());
    let (frame, sid) =
        dest.add_text_frame(SpreadRef::Doc(0), Rect::new(36.0, 36.0, 280.0, 336.0), dest.default_layer(), "", Default::default()).unwrap();
    let mut copy = source.story(host).unwrap().clone();
    dest.adopt_note_series(&mut copy, &source);
    copy.id = sid;
    copy.frames = vec![frame];
    dest.stories.insert(sid, Arc::new(copy));
    let cs = compose_story(&dest, sid, &Default::default());
    assert!(!cs.is_overset());
    assert_eq!(cs.frames[0].notes.len(), 1);
    assert!(cs.warnings.iter().any(|warning| warning.contains("no route")));
    dest.check().unwrap();
}

#[test]
fn undeclared_note_series_uses_the_series_zero_stream_route() {
    let (mut doc, host, streams) = commentary(1, 1, 110.0);
    doc.footnote_options = doc.note_series[0].options.clone();
    Arc::make_mut(&mut doc.story_mut(host).unwrap().notes[0]).series = 999;
    doc.item_mut(streams[0]).unwrap().text_frame_mut().unwrap().options.note_stream.as_mut().unwrap().series = 0;
    let cs = compose_story(&doc, host, &Default::default());
    assert!(!cs.is_overset());
    assert_eq!(cs.frame(streams[0]).unwrap().notes.len(), 1);
    assert_eq!(cs.frame(streams[0]).unwrap().notes[0].series, 0);
    assert!(cs.warnings.is_empty(), "{:?}", cs.warnings);
}

#[test]
fn stream_route_includes_pages_without_host_frames_and_invalidates_host_cache() {
    let mut doc = Document::new(&NewDocument { pages: 3, facing_pages: false, ..Default::default() });
    let layer = doc.default_layer();
    let (_, host) = doc.add_text_frame(SpreadRef::Doc(0), Rect::new(36.0, 36.0, 250.0, 400.0), layer, "Main text", Default::default()).unwrap();
    let mut route = Vec::new();
    // Create out of order: routing follows pages rather than item insertion order.
    for page in [2, 0, 1] {
        let (id, own) = doc.add_text_frame(SpreadRef::Doc(page), Rect::new(270.0, 36.0, 400.0, 400.0), layer, "", Default::default()).unwrap();
        doc.item_mut(id).unwrap().text_frame_mut().unwrap().options.note_stream = Some(NoteStream { story: host, series: 0 });
        assert!(frame_specs(&doc, own).is_empty(), "the frame never composes its own body story");
        route.push(id);
    }
    let frames = stream_frame_specs(&doc, host);
    assert_eq!(frames.iter().map(|f| f.page).collect::<Vec<_>>(), vec![Some(0), Some(1), Some(2)]);
    assert_eq!(frames.iter().map(|f| f.id).collect::<Vec<_>>(), vec![route[1], route[2], route[0]]);
    let cache = Cache::new();
    let before = cache.get(&doc, host, None);
    doc.item_mut(route[0]).unwrap().text_frame_mut().unwrap().options.inset[0] = 20.0;
    let after = cache.get(&doc, host, None);
    assert!(!Arc::ptr_eq(&before, &after), "stream changes on a page without host text must recompose the host");
    assert_eq!(stream_frame_specs(&doc, host)[2].area.y0, 56.0);
}

#[test]
fn a_hidden_normal_reference_after_body_text_keeps_its_stream_anchor() {
    let (mut doc, host, _) = commentary(1, 1, 100.0);
    doc.note_series[0].options.ref_position = designcraft_doc::Position::Normal;
    let cs = compose_story(&doc, host, &Default::default());
    assert!(!cs.is_overset());
    check_starts(&doc, host, &cs);
    assert_eq!(cs.frames.iter().map(|f| f.notes.len()).sum::<usize>(), 1);
}

#[test]
fn the_first_anchor_advances_to_a_page_with_a_route() {
    let (mut doc, host, streams) = commentary(2, 1, 100.0);
    doc.item_mut(streams[0]).unwrap().text_frame_mut().unwrap().options.note_stream = None;
    let cs = compose_story(&doc, host, &Default::default());
    assert!(!cs.is_overset());
    check_starts(&doc, host, &cs);
    assert_eq!(cs.frame(streams[1]).unwrap().notes.len(), 1);
    assert!(cs.frame(frame_specs(&doc, host)[0].id).unwrap().lines.is_empty());
}

#[test]
fn note_options_reflow_cached_cross_reference_pages() {
    let (mut doc, host, _) = commentary(3, 10, 135.0);
    let positions: Vec<_> = doc.story(host).unwrap().notes.iter().map(|n| doc.story(host).unwrap().note_anchor(n.id).unwrap()).collect();
    let anchors: Vec<_> = positions.into_iter().rev().map(|at| doc.story_mut(host).unwrap().insert_anchor(at, "Passage")).collect();
    let cache = Cache::default();
    let old = cache.xref_index(&doc);
    let old_pages: Vec<_> = anchors.iter().map(|id| old.page_index(*id)).collect();
    doc.note_series[0].options.space_before = 100.0;
    let fresh = Cache::default().xref_index(&doc);
    let expected: Vec<_> = anchors.iter().map(|id| fresh.page_index(*id)).collect();
    assert_ne!(old_pages, expected, "note options must actually change anchor pages");
    let updated = cache.xref_index(&doc);
    assert_eq!(anchors.iter().map(|id| updated.page_index(*id)).collect::<Vec<_>>(), expected);
}
