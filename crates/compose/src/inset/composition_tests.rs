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
            main_start_baseline: None,
            main_left_inset: None,
            main_right_inset: None,
            catchwords: false.into(),
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

fn routed_document(height: f64) -> (Document, StoryId, Vec<ItemId>) {
    let (mut doc, sid) = document(1, "Main passage.", "Inset comment.", "");
    let host = doc.story(sid).unwrap().frames[0];
    doc.item_mut(host).unwrap().text_frame_mut().unwrap().options.inset_columns.as_mut().unwrap().streams.pop();
    let mut routes = Vec::new();
    for rect in [Rect::new(640.0, 36.0, 670.0, 36.0 + height), Rect::new(36.0, 600.0, 636.0, 600.0 + height)] {
        let (id, _) = doc.add_text_frame(SpreadRef::Doc(0), rect, doc.default_layer(), "", Default::default()).unwrap();
        doc.item_mut(id).unwrap().text_frame_mut().unwrap().options.note_stream =
            Some(designcraft_doc::NoteStream { story: sid, series: 2, follow_inset: None });
        routes.push(id);
    }
    let story = doc.story_mut(sid).unwrap();
    for text in ["Third route note.", "Second route note.", "First route note with several lines of text."] {
        let id = story.insert_note(0, text, Default::default()).unwrap();
        story.note_mut(id).unwrap().series = 2;
    }
    (doc, sid, routes)
}

#[test]
fn catchwords_align_to_each_stream_paragraph_line_end() {
    for rtl in [false, true] {
        let (mut doc, sid) = document(3, &"Main word.\u{2028}".repeat(80), &"INNER body.\u{2028}".repeat(90), "");
        enable_catchwords(&mut doc, sid);
        let direction = if rtl { designcraft_doc::TextDirection::RightToLeft } else { designcraft_doc::TextDirection::LeftToRight };
        let story = doc.story_mut(sid).unwrap();
        story.format_paras(0..story.len(), |p| p.para.direction = Some(direction));
        for note in &mut story.notes {
            let note = std::sync::Arc::make_mut(note);
            note.text.format_paras(0..note.text.len(), |p| p.para.direction = Some(direction));
        }
        let cs = compose_story(&doc, sid, &Default::default());
        assert!(!cs.frames[0].catchwords.is_empty());
        for c in cs.frames.iter().flat_map(|f| &f.catchwords) {
            let width = c.text.frames.iter().flat_map(|f| &f.lines).flat_map(|l| &l.glyphs).map(|g| g.x + g.adv).fold(0.0_f64, f64::max);
            let edge = if rtl { c.origin.x } else { c.origin.x + width * c.scale };
            assert!((edge - if rtl { c.rect.x0 } else { c.rect.x1 }).abs() < 0.01);
        }
    }
}

#[test]
fn margin_note_below_main_does_not_lower_catchword() {
    let (mut doc, sid) = document(2, "Short main.\nNext page.", "", "");
    let story = doc.story_mut(sid).unwrap();
    let at = story.text.find("Next").unwrap();
    story.format_paras(at..story.len(), |p| p.para.start_paragraph = Some(designcraft_doc::StartParagraph::NextPage));
    enable_catchwords(&mut doc, sid);
    doc.note_series.push(NoteSeries {
        id: 3,
        name: "Margin".into(),
        options: designcraft_doc::FootnoteOptions {
            placement: NotePlacement::Margin { side: designcraft_doc::MarginSide::Right, gap: 10.0, width: 80.0 },
            ref_hidden: true,
            ..Default::default()
        },
    });
    let id = doc.story_mut(sid).unwrap().insert_note(5, "Small", Default::default()).unwrap();
    doc.story_mut(sid).unwrap().note_mut(id).unwrap().series = 3;
    let before = compose_story(&doc, sid, &Default::default());
    let row = |cs: &ComposedStory| cs.frames[0].catchwords.iter().find(|c| c.series.is_none()).map(|c| c.rect);
    let note = doc.story_mut(sid).unwrap().note_mut(id).unwrap();
    note.text.insert(note.text.len(), &"\nMargin line".repeat(8));
    let after = compose_story(&doc, sid, &Default::default());
    let side = after.frames[0].notes.iter().find(|n| n.series == 3).unwrap();
    assert!(side.rect.y1 > row(&before).unwrap().y1);
    assert!(row(&before).is_some(), "fixture must reserve a main catchword");
    assert_eq!(row(&after), row(&before));
}

#[test]
fn follower_clears_lowest_commentary_catchword() {
    let (mut doc, sid) = document(2, "Short main.\nNext page.", &"Commentary line.\u{2028}".repeat(100), "");
    let story = doc.story_mut(sid).unwrap();
    let at = story.text.find("Next").unwrap();
    story.format_paras(at..story.len(), |p| p.para.start_paragraph = Some(designcraft_doc::StartParagraph::NextPage));
    enable_catchwords(&mut doc, sid);
    let host = doc.story(sid).unwrap().frames[0];
    let (route, _) = doc.add_text_frame(SpreadRef::Doc(0), Rect::new(36.0, 36.0, 636.0, 850.0), doc.default_layer(), "", Default::default()).unwrap();
    doc.item_mut(route).unwrap().text_frame_mut().unwrap().options.note_stream = Some(designcraft_doc::NoteStream {
        story: sid,
        series: 2,
        follow_inset: Some(designcraft_doc::InsetFollow { edge: designcraft_doc::InsetEdge::Start, gap: 0.0, after: None }),
    });
    // Series 2 is routed rather than inset.
    for frame in doc.story(sid).unwrap().frames.clone() {
        doc.item_mut(frame).unwrap().text_frame_mut().unwrap().options.inset_columns.as_mut().unwrap().streams.pop();
    }
    let cs = compose_story(&doc, sid, &Default::default());
    let main = cs.frame(host).unwrap().catchwords.iter().find(|c| c.series.is_none()).unwrap();
    let commentary = cs.frame(host).unwrap().catchwords.iter().find(|c| c.series == Some(1)).unwrap();
    assert!(commentary.rect.y1 > main.rect.y1);
    assert!(cs.frame(route).unwrap().columns[0].y0 >= commentary.rect.y1 - 0.01);
}

#[test]
fn no_reserved_catchword_regions_skip_lookup() {
    let (doc, sid) = document(1, "Main text.", "", "");
    super::catchwords::LOOKUPS.with(|n| n.set(0));
    compose_story(&doc, sid, &Default::default());
    assert_eq!(super::catchwords::LOOKUPS.with(|n| n.get()), 0);
}

#[test]
fn routed_series_catchword_appears_only_in_last_route_on_continuing_page() {
    let (mut doc, sid) = document(2, &"Main.\u{2028}".repeat(65), "", &"Route word.\u{2028}".repeat(30));
    enable_catchwords(&mut doc, sid);
    for id in doc.story(sid).unwrap().frames.clone() {
        doc.item_mut(id).unwrap().text_frame_mut().unwrap().options.inset_columns.as_mut().unwrap().streams.retain(|s| s.series != Some(2));
    }
    let mut routes = Vec::new();
    for (page, top, height) in [(0, 36.0, 100.0), (0, 150.0, 100.0), (1, 36.0, 700.0)] {
        let (si, pi) = doc.page_loc(page).unwrap();
        let x = doc.spreads[si].pages[pi].x;
        let (id, _) = doc
            .add_text_frame(SpreadRef::Doc(si), Rect::new(x + 440.0, top, x + 636.0, top + height), doc.default_layer(), "", Default::default())
            .unwrap();
        doc.item_mut(id).unwrap().text_frame_mut().unwrap().options.note_stream =
            Some(designcraft_doc::NoteStream { story: sid, series: 2, follow_inset: None });
        routes.push(id);
    }
    let cs = compose_story(&doc, sid, &Default::default());
    assert!(!cs.is_overset(), "{:?}", cs.warnings);
    assert!(cs.frame(routes[0]).unwrap().catchwords.is_empty());
    let word = &cs.frame(routes[1]).unwrap().catchwords[0];
    assert_eq!(word.series, Some(2));
    assert_eq!(word.source.as_ref(), "Route");
    assert!(cs.frame(routes[2]).unwrap().catchwords.is_empty());
}

#[test]
fn follow_inset_reuses_candidate_main_layout() {
    let (mut doc, sid, routes) = routed_document(180.0);
    super::fitting::MAIN_LAYOUTS.with(|n| n.set(0));
    compose_story(&doc, sid, &Default::default());
    let ordinary = super::fitting::MAIN_LAYOUTS.with(|n| n.get());
    let follow = designcraft_doc::InsetFollow { edge: designcraft_doc::InsetEdge::Start, gap: 0.0, after: None };
    doc.item_mut(routes[0]).unwrap().text_frame_mut().unwrap().options.note_stream.as_mut().unwrap().follow_inset = Some(follow);
    super::fitting::MAIN_LAYOUTS.with(|n| n.set(0));
    compose_story(&doc, sid, &Default::default());
    assert_eq!(super::fitting::MAIN_LAYOUTS.with(|n| n.get()), ordinary);
}

#[test]
fn routed_catchword_hugs_last_line_and_its_follower() {
    let (mut doc, sid) = document(2, "Short main.\nNext page.", "", "");
    let story = doc.story_mut(sid).unwrap();
    let at = story.text.find("Next").unwrap();
    story.format_paras(at..story.len(), |p| p.para.start_paragraph = Some(designcraft_doc::StartParagraph::NextPage));
    enable_catchwords(&mut doc, sid);
    for id in doc.story(sid).unwrap().frames.clone() {
        doc.item_mut(id).unwrap().text_frame_mut().unwrap().options.inset_columns.as_mut().unwrap().streams.retain(|s| s.series.is_none());
    }
    let mut follower_series = doc.note_series[1].clone();
    follower_series.id = 3;
    doc.note_series.push(follower_series);
    for (at, series, text) in [(at, 2, "Following page."), (0, 2, "Route line.\nRoute line.\nRoute line."), (0, 3, "Follower.")] {
        let id = doc.story_mut(sid).unwrap().insert_note(at, text, Default::default()).unwrap();
        doc.story_mut(sid).unwrap().note_mut(id).unwrap().series = series;
    }
    let mut routes = Vec::new();
    for (page, series) in [(0, 2), (0, 3), (1, 2)] {
        let (si, pi) = doc.page_loc(page).unwrap();
        let x = doc.spreads[si].pages[pi].x;
        let (id, _) = doc
            .add_text_frame(SpreadRef::Doc(si), Rect::new(x + 440.0, 36.0, x + 636.0, 800.0), doc.default_layer(), "", Default::default())
            .unwrap();
        let follow_inset =
            (series == 3).then(|| designcraft_doc::InsetFollow { edge: designcraft_doc::InsetEdge::Start, gap: 0.0, after: routes.first().copied() });
        doc.item_mut(id).unwrap().text_frame_mut().unwrap().options.note_stream =
            Some(designcraft_doc::NoteStream { story: sid, series, follow_inset });
        routes.push(id);
    }
    let cs = compose_story(&doc, sid, &Default::default());
    assert!(!cs.is_overset(), "{:?}", cs.warnings);
    let route = cs.frame(routes[0]).unwrap();
    let last = route
        .notes
        .iter()
        .flat_map(|n| n.text.frames.iter().flat_map(move |f| f.lines.iter().map(move |l| (n.origin.y + l.baseline, l.descent))))
        .last()
        .unwrap();
    let word = &route.catchwords[0];
    assert!((word.rect.y0 - last.0 - last.1).abs() < 0.01, "row {:?}, last line {last:?}", word.rect);
    let baseline = word.origin.y + word.text.frames[0].lines[0].baseline * word.scale;
    assert!(baseline > last.0 && baseline - last.0 <= word.rect.height());
    assert!((cs.frame(routes[1]).unwrap().columns[0].y0 - word.rect.y1).abs() < 0.01);
}

#[test]
fn small_catchword_leading_does_not_scale_glyph_size() {
    let (mut doc, sid) = document(2, &"Main line.\u{2028}".repeat(80), "", "");
    std::sync::Arc::make_mut(&mut doc.styles).character.push(designcraft_doc::styles::CharacterStyle {
        name: "Small leading".into(),
        based_on: None,
        shortcut: String::new(),
        chars: designcraft_doc::CharAttrs { size: Some(18.0), leading: Some(designcraft_doc::Leading::Points(2.0)), ..Default::default() },
    });
    for id in doc.story(sid).unwrap().frames.clone() {
        doc.item_mut(id).unwrap().text_frame_mut().unwrap().options.inset_columns.as_mut().unwrap().catchwords =
            designcraft_doc::InsetCatchwords::Styled { style: "Small leading".into() };
    }
    let cs = compose_story(&doc, sid, &Default::default());
    let word = &cs.frames[0].catchwords[0];
    assert!(word.text.styles.iter().any(|s| s.size == 18.0));
    assert_eq!(word.scale, 1.0);
}

#[test]
fn chained_followers_fit_each_route_area_once_per_candidate() {
    let (mut doc, sid, routes) = routed_document(180.0);
    super::fitting::ROUTE_AREAS.with(|n| n.set(0));
    compose_story(&doc, sid, &Default::default());
    let ordinary = super::fitting::ROUTE_AREAS.with(|n| n.get());
    for (i, id) in routes.iter().enumerate() {
        doc.item_mut(*id).unwrap().text_frame_mut().unwrap().options.note_stream.as_mut().unwrap().follow_inset =
            Some(designcraft_doc::InsetFollow { edge: designcraft_doc::InsetEdge::Start, gap: 0.0, after: i.checked_sub(1).map(|p| routes[p]) });
    }
    super::fitting::ROUTE_AREAS.with(|n| n.set(0));
    compose_story(&doc, sid, &Default::default());
    assert_eq!(super::fitting::ROUTE_AREAS.with(|n| n.get()), ordinary);
}

#[test]
fn two_same_page_hosts_share_one_route_fit_including_second_host_anchor() {
    let (mut doc, sid, routes) = routed_document(180.0);
    let host = doc.story(sid).unwrap().frames[0];
    doc.story_mut(sid).unwrap().notes.clear();
    let text = "Main line.\n".repeat(20);
    let len = doc.story(sid).unwrap().len();
    doc.story_mut(sid).unwrap().replace(0..len, &text);
    doc.item_mut(host).unwrap().path = designcraft_geom::shapes::rectangle(Rect::new(36.0, 36.0, 330.0, 120.0));
    let (second, _) =
        doc.add_text_frame(SpreadRef::Doc(0), Rect::new(340.0, 36.0, 636.0, 500.0), doc.default_layer(), "", Default::default()).unwrap();
    doc.thread(host, second).unwrap();
    let config = doc.item(host).unwrap().text_frame().unwrap().options.inset_columns.clone();
    doc.item_mut(second).unwrap().text_frame_mut().unwrap().options.inset_columns = config;
    let at = text.rfind("Main line.").unwrap();
    let note = doc.story_mut(sid).unwrap().insert_note(at, "Second host route note.", Default::default()).unwrap();
    doc.story_mut(sid).unwrap().note_mut(note).unwrap().series = 2;
    let cs = compose_story(&doc, sid, &Default::default());
    assert!(cs.frame(second).unwrap().range.contains(&at));
    assert_eq!(cs.frames.iter().filter(|f| f.frame == routes[0]).count(), 1);
    assert!(routes.iter().filter_map(|id| cs.frame(*id)).flat_map(|f| &f.notes).any(|n| n.id == note));
}

#[test]
fn revisited_page_routes_include_return_visit_anchor_once() {
    let text = "Main line.\n".repeat(30);
    let (mut doc, sid) = document(2, &text, "", "");
    let hosts = doc.story(sid).unwrap().frames.clone();
    for id in &hosts {
        let item = doc.item_mut(*id).unwrap();
        let bounds = item.bounds();
        item.path = designcraft_geom::shapes::rectangle(Rect::new(bounds.x0, 36.0, bounds.x1, 120.0));
        item.text_frame_mut().unwrap().options.inset_columns.as_mut().unwrap().streams.retain(|s| s.series.is_none());
    }
    let (last, _) = doc.add_text_frame(SpreadRef::Doc(0), Rect::new(36.0, 140.0, 636.0, 830.0), doc.default_layer(), "", Default::default()).unwrap();
    doc.thread(hosts[1], last).unwrap();
    doc.item_mut(last).unwrap().text_frame_mut().unwrap().options.inset_columns =
        doc.item(hosts[0]).unwrap().text_frame().unwrap().options.inset_columns.clone();
    let (route, _) =
        doc.add_text_frame(SpreadRef::Doc(0), Rect::new(640.0, 36.0, 671.0, 830.0), doc.default_layer(), "", Default::default()).unwrap();
    doc.item_mut(route).unwrap().text_frame_mut().unwrap().options.note_stream =
        Some(designcraft_doc::NoteStream { story: sid, series: 2, follow_inset: None });
    let at = text.rfind("Main line.").unwrap();
    let note = doc.story_mut(sid).unwrap().insert_note(at, "Return visit note.", Default::default()).unwrap();
    doc.story_mut(sid).unwrap().note_mut(note).unwrap().series = 2;
    let (si, pi) = doc.page_loc(1).unwrap();
    let x = doc.spreads[si].pages[pi].x;
    let (other_route, _) =
        doc.add_text_frame(SpreadRef::Doc(si), Rect::new(x + 640.0, 36.0, x + 671.0, 830.0), doc.default_layer(), "", Default::default()).unwrap();
    doc.item_mut(other_route).unwrap().text_frame_mut().unwrap().options.note_stream =
        Some(designcraft_doc::NoteStream { story: sid, series: 2, follow_inset: None });
    let first = doc.story_mut(sid).unwrap().insert_note(0, "First visit note.", Default::default()).unwrap();
    doc.story_mut(sid).unwrap().note_mut(first).unwrap().series = 2;
    let cs = compose_story(&doc, sid, &Default::default());
    assert!(cs.frame(last).unwrap().range.contains(&doc.story(sid).unwrap().note_anchor(note).unwrap()));
    assert_eq!(cs.frames.iter().filter(|f| f.frame == route).count(), 1);
    assert_eq!(cs.frames.iter().filter(|f| f.frame == other_route).count(), 1);
    assert!(cs.frame(route).unwrap().notes.iter().any(|n| n.id == first));
    assert!(cs.frame(route).unwrap().notes.iter().any(|n| n.id == note && n.source.as_ref() == "Return visit note."));
}

#[test]
fn inset_preserves_unplaced_column_footnote_overset() {
    let (mut doc, sid) = document(1, "Opening.\nReference.", "Short commentary.", "");
    let text = "Unplaced column footnote line.\n".repeat(150);
    doc.story_mut(sid).unwrap().insert_note(17, &text, Default::default()).unwrap();
    let cs = compose_story(&doc, sid, &Default::default());
    let story = doc.story(sid).unwrap();
    assert_eq!(cs.frames.iter().map(|f| f.range.end).max(), Some(story.len()));
    let parts: Vec<_> = cs.frames.iter().flat_map(|f| &f.notes).filter(|n| n.series == 0).collect();
    assert!(!parts.is_empty(), "the column-foot note must start");
    assert!(parts.iter().flat_map(|n| &n.text.frames).map(|f| f.range.end).max().unwrap() < text.len());
    assert!(cs.is_overset(), "unplaced column-foot continuation must report overset");
}

#[test]
fn standalone_route_beside_inset_preserves_note_order_and_all_source_text() {
    let (doc, sid, routes) = routed_document(90.0);
    let cs = compose_story(&doc, sid, &Default::default());
    assert!(!cs.is_overset(), "{:?}", cs.warnings);
    let parts: Vec<_> = routes.iter().flat_map(|id| &cs.frame(*id).expect("route frame must be composed").notes).collect();
    assert!(routes.iter().all(|id| !cs.frame(*id).unwrap().notes.is_empty()));
    assert!(parts.windows(2).all(|pair| pair[0].index <= pair[1].index));
    for note in doc.story(sid).unwrap().notes.iter().filter(|n| n.series == 2) {
        let mut end = 0;
        for part in parts.iter().filter(|p| p.id == note.id) {
            for frame in &part.text.frames {
                assert_eq!(frame.range.start, end);
                end = frame.range.end;
            }
        }
        assert_eq!(end, note.text.len());
    }
}

#[test]
fn standalone_route_beside_inset_recomposes_after_note_edit() {
    let (mut doc, sid, routes) = routed_document(90.0);
    let cache = Cache::new();
    let before = cache.get(&doc, sid, None);
    let id = before.frame(routes[0]).unwrap().notes[0].id;
    doc.story_mut(sid).unwrap().note_mut(id).unwrap().text.replace(0..0, "Additional route words. ");
    let warm = cache.get(&doc, sid, None);
    let fresh = compose_story(&doc, sid, &Default::default());
    assert_eq!(warm.overset_at, fresh.overset_at);
    for route in routes {
        let signature = |cs: &ComposedStory| {
            cs.frame(route)
                .unwrap()
                .notes
                .iter()
                .flat_map(|n| n.text.frames.iter().map(move |f| (n.id, f.range.clone(), n.origin)))
                .collect::<Vec<_>>()
        };
        assert_eq!(signature(&warm), signature(&fresh));
    }
}

#[test]
fn standalone_route_beside_inset_reports_unplaced_tail_as_overset() {
    let (mut doc, sid, routes) = routed_document(35.0);
    let note = doc.story_mut(sid).unwrap().notes.iter_mut().find(|n| n.series == 2).unwrap();
    let note = Arc::make_mut(note);
    note.text.replace(0..note.text.len(), &"Long route commentary. ".repeat(500));
    let cs = compose_story(&doc, sid, &Default::default());
    assert!(cs.is_overset());
    assert!(cs.warnings.iter().any(|w| w.contains("commentary remains")));
    assert!(cs.frame(*routes.last().unwrap()).unwrap().notes.last().unwrap().overflow);
}

#[test]
fn main_start_matches_sixth_commentary_baseline_with_different_leading() {
    let (doc, sid) = document(1, &"Main words. ".repeat(30), &"Commentary words. ".repeat(90), &"Outside words. ".repeat(90));
    let mut frames = frame_specs(&doc, sid);
    let options = frames[0].opts.inset_columns.as_mut().unwrap();
    options.main_start_lines = 20;
    options.main_start_baseline = Some(6);
    let cs = compose(&doc, doc.story(sid).unwrap(), &frames, &Default::default());
    let main = cs.frames[0].lines[0].baseline;
    let (note, line) = cs.frames[0]
        .notes
        .iter()
        .filter(|n| n.series == 1)
        .flat_map(|n| n.text.frames.iter().flat_map(move |f| f.lines.iter().map(move |l| (n, l))))
        .nth(5)
        .unwrap();
    assert!((main - note.origin.y - line.baseline).abs() < 0.01, "main {main}, commentary {}", note.origin.y + line.baseline);
}

#[test]
fn commentary_continues_to_main_axis_without_losing_text() {
    let (doc, sid) = document(1, "Short main text.", &"Inner commentary words. ".repeat(160), &"Outer commentary words. ".repeat(120));
    let mut frames = frame_specs(&doc, sid);
    let options = frames[0].opts.inset_columns.as_mut().unwrap();
    options.main_left_inset = Some(210.0);
    options.main_right_inset = Some(162.0);
    options.streams[0].ratio = 2.0;
    let cs = compose(&doc, doc.story(sid).unwrap(), &frames, &Default::default());
    assert!(!cs.is_overset(), "{:?}", cs.warnings);
    for note in &doc.story(sid).unwrap().notes {
        let parts: Vec<_> = cs.frames.iter().flat_map(|f| &f.notes).filter(|n| n.id == note.id).collect();
        let mut end = 0;
        for part in &parts {
            for frame in &part.text.frames {
                assert_eq!(frame.range.start, end);
                for line in &frame.lines {
                    assert_eq!(line.range.start, end);
                    end = line.range.end;
                }
            }
        }
        assert_eq!(end, note.text.len());
        let widened = parts.iter().find(|n| if note.series == 1 { n.rect.x1 > 246.0 } else { n.rect.x0 < 474.0 }).unwrap();
        let axis = 36.0 + (210.0 + 600.0 - 162.0) / 2.0;
        assert!((if note.series == 1 { widened.rect.x1 - (axis - 6.0) } else { widened.rect.x0 - (axis + 6.0) }).abs() < 0.01, "{:?}", widened.rect);
    }
}

#[test]
fn unplaced_commentary_reports_story_overset() {
    let (mut doc, sid) = document(1, "Short main text.", "Commentary.", "Outside.");
    let frames = frame_specs(&doc, sid);
    let note = doc.story_mut(sid).unwrap().notes[0].id;
    doc.story_mut(sid).unwrap().note_mut(note).unwrap().text.insert(0, &"Unplaced words. ".repeat(5000));
    let cs = compose(&doc, doc.story(sid).unwrap(), &frames, &Default::default());
    assert!(cs.is_overset());
}

#[test]
fn commentary_starting_mid_band_keeps_its_tail_below_main() {
    let (mut doc, sid) = document(1, &"Main words. ".repeat(50), &"Inner words. ".repeat(300), &"First note. ".repeat(30));
    let story = doc.story_mut(sid).unwrap();
    let id = story.insert_note(6, &"Final commentary words. ".repeat(90), Default::default()).unwrap();
    let note = story.note_mut(id).unwrap();
    note.series = 2;
    note.text.format_chars(0..note.text.len(), |f| {
        f.over.size = Some(9.0);
        f.over.leading = Some(designcraft_doc::Leading::Points(17.0));
    });
    let cs = compose(&doc, doc.story(sid).unwrap(), &frame_specs(&doc, sid), &Default::default());
    let note = doc.story(sid).unwrap().note(id).unwrap();
    let mut end = 0;
    for part in cs.frames.iter().flat_map(|f| &f.notes).filter(|n| n.id == id) {
        for frame in &part.text.frames {
            for line in &frame.lines {
                assert!(line.baseline >= 0.0);
            }
            assert_eq!(frame.range.start, end);
            end = frame.lines.last().unwrap().range.end;
        }
    }
    assert_eq!(end, note.text.len());
    assert!(!cs.is_overset(), "{:?}", cs.warnings);
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

fn enable_catchwords(doc: &mut Document, sid: StoryId) {
    for id in doc.story(sid).unwrap().frames.clone() {
        doc.item_mut(id).unwrap().text_frame_mut().unwrap().options.inset_columns.as_mut().unwrap().catchwords = true.into();
    }
}

#[test]
fn catchwords_follow_each_stream_and_its_line_end() {
    let (mut doc, sid) = document(3, &"Main word.\u{2028}".repeat(80), &"INNER body.\u{2028}".repeat(90), &"OUTER body.\u{2028}".repeat(80));
    enable_catchwords(&mut doc, sid);
    let cs = compose_story(&doc, sid, &Default::default());
    assert!(!cs.is_overset(), "{:?}", cs.warnings);
    assert_eq!(cs.frames[0].catchwords.len(), 3);
    assert!(cs.frames.last().unwrap().catchwords.is_empty());
    doc.text_variables.push(designcraft_doc::vars::TextVariable::new(
        "Inner catchword",
        designcraft_doc::vars::VarKind::NextPageFirstWord { story: Some(sid), series: Some(1) },
    ));
    let index = crate::vars::RunningIndex::build(&doc, &|id| {
        assert_eq!(id, sid);
        std::sync::Arc::new(cs.clone())
    });
    let values = crate::vars::values(&doc, Some(0), Some(&index));
    assert_eq!(values[doc.variable("Inner catchword").unwrap()], "INNER");
    for frame in &cs.frames {
        for c in &frame.catchwords {
            let expected = match c.series {
                None => "Main",
                Some(1) => "INNER",
                Some(2) => "OUTER",
                _ => "",
            };
            assert_eq!(&*c.source, expected);
            let width = c.text.frames.iter().flat_map(|f| &f.lines).flat_map(|l| &l.glyphs).map(|g| g.x + g.adv).fold(0.0_f64, f64::max);
            let edge = c.origin.x + width * c.scale;
            assert!((edge - c.rect.x1).abs() < 0.01);
            for line in &frame.lines {
                assert!(
                    line.x1 <= c.rect.x0 + 0.01
                        || c.rect.x1 <= line.x0 + 0.01
                        || line.baseline + line.descent <= c.rect.y0 + 0.01
                        || line.baseline - line.ascent >= c.rect.y1 - 0.01
                );
            }
            for note in &frame.notes {
                for line in note.text.frames.iter().flat_map(|f| &f.lines) {
                    let (x0, x1) = (note.origin.x + line.x0, note.origin.x + line.x1);
                    let (y0, y1) = (note.origin.y + line.baseline - line.ascent, note.origin.y + line.baseline + line.descent);
                    assert!(x1 <= c.rect.x0 + 0.01 || c.rect.x1 <= x0 + 0.01 || y1 <= c.rect.y0 + 0.01 || y0 >= c.rect.y1 - 0.01);
                }
            }
        }
    }
    continuity(&cs);
}

#[test]
fn changed_next_page_word_updates_previous_page_catchword() {
    let (mut doc, sid) = document(3, &"Main word.\u{2028}".repeat(80), &"INNER body.\u{2028}".repeat(90), "");
    enable_catchwords(&mut doc, sid);
    let cache = Cache::new();
    let before = cache.get(&doc, sid, None);
    let note = before.frames[1].notes.iter().find(|n| n.series == 1).unwrap();
    let start = note.text.frames.first().unwrap().range.start;
    let at = start + note.source.get(start..).unwrap().find("INNER").unwrap();
    doc.story_mut(sid).unwrap().note_mut(note.id).unwrap().text.replace(at..at + 5, "ALTER");
    let after = cache.get(&doc, sid, None);
    let fresh = compose_story(&doc, sid, &Default::default());
    assert_eq!(before.frames[0].catchwords.iter().find(|c| c.series == Some(1)).unwrap().source.as_ref(), "INNER");
    assert_eq!(after.frames[0].catchwords.iter().find(|c| c.series == Some(1)).unwrap().source.as_ref(), "ALTER");
    for (a, b) in after.frames.iter().zip(&fresh.frames) {
        assert_eq!(
            a.catchwords.iter().map(|c| (&c.source, c.rect)).collect::<Vec<_>>(),
            b.catchwords.iter().map(|c| (&c.source, c.rect)).collect::<Vec<_>>()
        );
    }
}

#[test]
fn fixed_width_host_columns_do_not_partition_inset_bands() {
    let (mut doc, sid) = document(1, &"Main source words. ".repeat(40), &"Inner source. ".repeat(30), &"Outer source. ".repeat(40));
    let expected = compose_story(&doc, sid, &Default::default());
    for id in doc.story(sid).unwrap().frames.clone() {
        let options = &mut doc.item_mut(id).unwrap().text_frame_mut().unwrap().options;
        options.columns_kind = designcraft_doc::ColumnsKind::FixedWidth;
        options.column_width = 90.0;
    }
    let actual = compose_story(&doc, sid, &Default::default());
    assert_eq!(actual.frames[0].columns, expected.frames[0].columns);
    assert_eq!(actual.frames[0].range, expected.frames[0].range);
    no_overlap(&actual);
}

#[test]
fn independently_changed_future_notes_are_not_reused() {
    let main: String = (0..150).map(|i| format!("Main line {i:03}.\u{2028}")).collect();
    let (mut doc, sid) = document(4, &main, "First commentary.", "");
    let at = doc.story(sid).unwrap().text.find("Main line 110").unwrap();
    let late = doc.story_mut(sid).unwrap().insert_note(at, "Later commentary.", Default::default()).unwrap();
    doc.story_mut(sid).unwrap().note_mut(late).unwrap().series = 1;
    let first = doc.story(sid).unwrap().notes[0].id;
    let cache = Cache::new();
    let before = cache.get(&doc, sid, None);
    assert!(before.frames[2].notes.iter().any(|n| n.id == late));
    for id in [first, late] {
        let note = doc.story_mut(sid).unwrap().note_mut(id).unwrap();
        let len = note.text.len();
        note.text.format_chars(0..len, |f| f.over.fill = Some("[Registration]".into()));
    }
    let warm = cache.get(&doc, sid, None);
    let fresh = compose_story(&doc, sid, &Default::default());
    let a = warm.frames.iter().flat_map(|f| &f.notes).find(|n| n.id == late).unwrap();
    let b = fresh.frames.iter().flat_map(|f| &f.notes).find(|n| n.id == late).unwrap();
    assert_eq!(a.text.styles, b.text.styles);
    assert!(a.text.styles.iter().any(|s| s.fill == "[Registration]"));
}

#[test]
fn explicit_page_break_skips_band_baselines() {
    let (mut doc, sid) = document(2, &format!("{}\nAfter the page break.", "Before.\u{2028}".repeat(7)), "", "");
    let at = doc.story(sid).unwrap().text.find("After").unwrap();
    let len = doc.story(sid).unwrap().len();
    doc.story_mut(sid).unwrap().format_paras(at..len, |p| p.para.start_paragraph = Some(designcraft_doc::StartParagraph::NextPage));
    let original = frame_specs(&doc, sid);
    let frames = vec![original[0].clone(), original[0].clone(), original[1].clone()];
    let ordinary = compose(&doc, doc.story(sid).unwrap(), &frames, &ComposeOptions { inset_measure: true, ..Default::default() });
    let bands =
        compose(&doc, doc.story(sid).unwrap(), &frames, &ComposeOptions { inset_measure: true, continuations: vec![(1, 0.0)], ..Default::default() });
    assert_eq!(bands.frames[2].lines[0].baseline, ordinary.frames[2].lines[0].baseline);
    assert_eq!(bands.frames[2].range, ordinary.frames[2].range);
}

#[test]
fn band_note_cache_respects_changed_exclusions() {
    let (doc, sid) = document(1, "Body", &"Cache input words. ".repeat(20), "");
    let story = doc.story(sid).unwrap();
    let mut notes = crate::notes::SeriesNotes::new(&doc, story, 1, false);
    let spec = frame_specs(&doc, sid).remove(0);
    let opts = ComposeOptions { note_bands: true, ..Default::default() };
    let plain = notes.compose_parts(&doc, 0, spec.area.width(), &spec, &opts, &[], &[]);
    let mut blocked = spec.clone();
    blocked.exclusions.push(Exclusion {
        rect: Rect::new(spec.area.x0, spec.area.y0, spec.area.x1 - 40.0, spec.area.y0 + 100.0),
        mode: designcraft_doc::WrapMode::BoundingBox,
        side: designcraft_doc::WrapSide::BothSides,
        contour: None,
        after: None,
        anchor_baseline: None,
    });
    let wrapped = notes.compose_parts(&doc, 0, spec.area.width(), &blocked, &opts, &[], &[]);
    let plain = plain[0].as_ref().unwrap().0.frames[0].lines[0].range.end;
    let wrapped = wrapped[0].as_ref().unwrap().0.frames[0].lines[0].range.end;
    assert!(wrapped < plain, "{wrapped} not before {plain}");
}

#[test]
fn terminal_forced_break_does_not_duplicate_the_paragraph_source_range() {
    let (doc, sid) = document(1, "Main\u{2028}", "", "");
    let cs = compose_story(&doc, sid, &Default::default());
    let at = doc.story(sid).unwrap().len();
    assert_eq!(cs.frames[0].lines.last().unwrap().range, at..at);
    continuity(&cs);
}

#[test]
fn series_catchword_omits_deleted_source_inside_a_word() {
    let (mut doc, sid) = document(3, &"Main word.\u{2028}".repeat(80), &"INNER body.\u{2028}".repeat(90), "");
    enable_catchwords(&mut doc, sid);
    let before = compose_story(&doc, sid, &Default::default());
    let note = before.frames[1].notes.iter().find(|n| n.series == 1).unwrap();
    let start = note.text.frames[0].range.start;
    let at = start + note.source.get(start..).unwrap().find("INNER").unwrap();
    let source = &mut doc.story_mut(sid).unwrap().note_mut(note.id).unwrap().text;
    source.replace(at..at + 5, "PuHIDEblic");
    source.format_chars(at + 2..at + 6, |f| f.over.change = Some(designcraft_doc::ChangeMark::Deleted));
    let after = compose_story(&doc, sid, &Default::default());
    assert_eq!(after.frames[0].catchwords.iter().find(|c| c.series == Some(1)).unwrap().source.as_ref(), "Public");
}

#[test]
fn series_catchword_distinguishes_hidden_controls_from_active_delimiters() {
    for control in ['\t', designcraft_doc::story::FORCED_LINE_BREAK] {
        for state in ["active", "hidden", "deleted"] {
            let (mut doc, sid) = document(3, &"Main word.\u{2028}".repeat(80), &"INNER body.\u{2028}".repeat(90), "");
            enable_catchwords(&mut doc, sid);
            let before = compose_story(&doc, sid, &Default::default());
            let note = before.frames[1].notes.iter().find(|n| n.series == 1).unwrap();
            let start = note.text.frames[0].range.start;
            let at = start + note.source.get(start..).unwrap().find("INNER").unwrap();
            doc.conditions.push(designcraft_doc::Condition { name: "Hidden".into(), visible: false, color: [0, 0, 0] });
            let source = &mut doc.story_mut(sid).unwrap().note_mut(note.id).unwrap().text;
            source.replace(at..at + 5, &format!("Pu{control}blic"));
            source.format_chars(at + 2..at + 2 + control.len_utf8(), |f| match state {
                "hidden" => f.over.conditions = Some(vec!["Hidden".into()]),
                "deleted" => f.over.change = Some(designcraft_doc::ChangeMark::Deleted),
                _ => {}
            });
            let after = compose_story(&doc, sid, &Default::default());
            assert_eq!(
                after.frames[0].catchwords.iter().find(|c| c.series == Some(1)).unwrap().source.as_ref(),
                if state == "active" { "Pu" } else { "Public" },
                "{state} {control:?}"
            );
        }
    }
}
