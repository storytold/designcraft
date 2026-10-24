use designcraft_doc::build::NewDocument;
use designcraft_doc::{Align, Composer, Document, ParaAttrs, ParaFormat, SpreadRef};
use designcraft_geom::Rect;

use super::*;

#[test]
fn long_wrapped_paragraph_rechecks_width_after_estimated_slots_end() {
    let text="Distinctive shapes and warm colours set the rhythm of the page. The story flows around the illustration while the frame keeps its original dimensions. ".repeat(14);
    let (mut d, sid, _) = doc_with(&text, Rect::new(36.0, 36.0, 576.0, 756.0), ParaAttrs::default());
    d.story_mut(sid).unwrap().format_chars(0..text.len(), |f| {
        f.over.font_family = Some("Source Serif 4".into());
        f.over.size = Some(13.0);
        f.over.leading = Some(designcraft_doc::Leading::Points(17.0));
    });
    add_wrap_objects(&mut d, &[(Rect::new(180.0, 135.0, 440.0, 568.3333333333334), [13.5; 4], WrapMode::Contour)]);
    let composed = compose_story(&d, sid, &ComposeOptions::default());
    let lines = &composed.frames[0].lines;
    assert!(lines.iter().any(|l| l.baseline > 450.0 && l.baseline < 560.0));
    for line in lines {
        for glyph in line.glyphs.iter().filter(|g| g.visible && text.as_bytes().get(g.byte).is_some_and(|b| !b.is_ascii_whitespace())) {
            assert!(
                glyph.x + glyph.adv <= line.x1 + 0.5,
                "glyph {} ends at {} outside the segment {}..{} on baseline {}",
                glyph.byte,
                glyph.x + glyph.adv,
                line.x0,
                line.x1,
                line.baseline
            );
        }
    }
}

const LOREM: &str = "Typography is the art and technique of arranging type to make written language legible, readable and appealing when displayed. \
The arrangement of type involves selecting typefaces, point sizes, line lengths, line spacing, and letter spacing, and adjusting the space between pairs of letters.";

fn doc_with(text: &str, rect: Rect, para: ParaAttrs) -> (Document, StoryId, ItemId) {
    let mut d = Document::new(&NewDocument::default());
    let lid = d.default_layer();
    let (fid, sid) = d.add_text_frame(SpreadRef::Doc(0), rect, lid, text, ParaFormat { para, ..Default::default() }).unwrap();
    (d, sid, fid)
}

fn all_lines(cs: &ComposedStory) -> Vec<&Line> {
    cs.frames.iter().flat_map(|f| f.lines.iter()).collect()
}

#[test]
fn composes_simple_paragraph() {
    let (d, sid, _) = doc_with(LOREM, Rect::new(36.0, 36.0, 300.0, 700.0), ParaAttrs::default());
    let cs = compose_story(&d, sid, &ComposeOptions::default());
    assert!(!cs.is_overset());
    let lines = all_lines(&cs);
    assert!(lines.len() >= 5, "{}", lines.len());
    // Baselines increase by the auto leading (12 × 120% = 14.4).
    for w in lines.windows(2) {
        assert!((w[1].baseline - w[0].baseline - 14.4).abs() < 1e-6);
    }
    // Lines fit their measure.
    for l in &lines {
        assert!(l.end_x <= l.x1 + 0.5, "line overflows: {} > {}", l.end_x, l.x1);
    }
}

#[test]
fn line_ranges_partition_the_story() {
    let text = format!("{LOREM}\n\nSecond paragraph here.\n{LOREM}");
    let (d, sid, _) = doc_with(&text, Rect::new(0.0, 0.0, 200.0, 2000.0), ParaAttrs::default());
    let cs = compose_story(&d, sid, &ComposeOptions::default());
    let lines = all_lines(&cs);
    let mut pos = 0;
    for l in &lines {
        // Each line starts where the previous ended (or after a paragraph separator).
        assert!(l.range.start == pos || l.range.start == pos + 1, "gap at {pos}: {:?}", l.range);
        pos = l.range.end;
    }
    assert_eq!(pos, text.len());
    assert_eq!(lines.iter().filter(|l| l.first_in_para).count(), 4);
}

#[test]
fn justified_lines_fill_the_measure() {
    let (d, sid, _) = doc_with(LOREM, Rect::new(0.0, 0.0, 220.0, 2000.0), ParaAttrs { align: Some(Align::LeftJustified), ..Default::default() });
    let cs = compose_story(&d, sid, &ComposeOptions::default());
    let lines = all_lines(&cs);
    for l in &lines[..lines.len() - 1] {
        assert!((l.end_x - l.x1).abs() < 0.6, "justified line ends at {} not {}", l.end_x, l.x1);
    }
    let last = lines.last().unwrap();
    assert!(last.end_x < last.x1 - 1.0, "last line is ragged");
}

#[test]
fn small_frame_oversets() {
    let (d, sid, _) = doc_with(LOREM, Rect::new(0.0, 0.0, 200.0, 40.0), ParaAttrs::default());
    let cs = compose_story(&d, sid, &ComposeOptions::default());
    assert!(cs.is_overset());
    let shown = cs.frames[0].range.clone();
    assert_eq!(cs.overset_at.unwrap(), shown.end.max(cs.overset_at.unwrap()).min(cs.overset_at.unwrap()));
    assert!(cs.frames[0].lines.len() <= 3);
}

#[test]
fn text_flows_through_threaded_frames() {
    let mut d = Document::new(&NewDocument::default());
    let lid = d.default_layer();
    let long = [LOREM; 4].join("\n");
    let (a, sid) = d.add_text_frame(SpreadRef::Doc(0), Rect::new(0.0, 0.0, 200.0, 120.0), lid, &long, ParaFormat::default()).unwrap();
    let (b, _) = d.add_text_frame(SpreadRef::Doc(0), Rect::new(250.0, 0.0, 450.0, 2000.0), lid, "", ParaFormat::default()).unwrap();
    d.thread(a, b).unwrap();
    let cs = compose_story(&d, sid, &ComposeOptions::default());
    assert!(!cs.is_overset());
    assert!(!cs.frames[0].lines.is_empty() && !cs.frames[1].lines.is_empty());
    assert_eq!(cs.frames[0].range.end, cs.frames[1].range.start.min(cs.frames[0].range.end).max(cs.frames[0].range.end));
    // The second frame's lines start at its top.
    assert!(cs.frames[1].lines[0].baseline < 20.0);
}

#[test]
fn columns_fill_left_to_right() {
    let (mut d, sid, fid) = doc_with(&[LOREM; 3].join(" "), Rect::new(0.0, 0.0, 400.0, 150.0), ParaAttrs::default());
    d.item_mut(fid).unwrap().text_frame_mut().unwrap().options.columns = 2;
    let cs = compose_story(&d, sid, &ComposeOptions::default());
    let lines = all_lines(&cs);
    assert!(lines.iter().any(|l| l.column == 1));
    let c1 = lines.iter().find(|l| l.column == 1).unwrap();
    assert!(c1.x0 > 200.0);
}

#[test]
fn centered_text_is_centered() {
    let (d, sid, _) = doc_with("Hello", Rect::new(0.0, 0.0, 300.0, 100.0), ParaAttrs { align: Some(Align::Center), ..Default::default() });
    let cs = compose_story(&d, sid, &ComposeOptions::default());
    let l = &cs.frames[0].lines[0];
    let left = l.glyphs[0].x - l.x0;
    let right = l.x1 - l.end_x;
    assert!((left - right).abs() < 0.5, "{left} vs {right}");
}

#[test]
fn caret_and_hit_roundtrip() {
    let (d, sid, _) = doc_with(LOREM, Rect::new(0.0, 0.0, 200.0, 1000.0), ParaAttrs::default());
    let cs = compose_story(&d, sid, &ComposeOptions::default());
    for pos in [0, 5, 40, 100, LOREM.len()] {
        let (fi, x, b, _, _) = caret(&cs, pos).unwrap();
        let back = hit(&cs, fi, Point::new(x + 0.1, b - 2.0)).unwrap();
        assert!((back as i64 - pos as i64).abs() <= 1, "{pos} -> {back}");
    }
}

#[test]
fn empty_story_has_one_line() {
    let (d, sid, _) = doc_with("", Rect::new(0.0, 0.0, 200.0, 100.0), ParaAttrs::default());
    let cs = compose_story(&d, sid, &ComposeOptions::default());
    assert_eq!(cs.line_count(), 1);
    assert!(caret(&cs, 0).is_some());
}

#[test]
fn wrap_pushes_text_aside() {
    let (mut d, sid, _) = doc_with(&[LOREM; 2].join(" "), Rect::new(0.0, 0.0, 300.0, 800.0), ParaAttrs::default());
    let lid = d.default_layer();
    let id = ItemId(d.alloc());
    let mut it =
        designcraft_doc::Item::new(id, lid, designcraft_doc::Shape::Rectangle, designcraft_geom::shapes::rectangle(Rect::new(0.0, 0.0, 120.0, 60.0)));
    it.wrap.mode = WrapMode::BoundingBox;
    it.wrap.offsets = [0.0, 0.0, 6.0, 6.0];
    d.insert_item(SpreadRef::Doc(0), it, None).unwrap();
    let cs = compose_story(&d, sid, &ComposeOptions::default());
    let first = &cs.frames[0].lines[0];
    assert!(first.x0 >= 126.0 - 1e-6, "{}", first.x0);
    let below = cs.frames[0].lines.iter().find(|l| l.baseline - l.ascent > 66.0).unwrap();
    assert!(below.x0 < 1.0);
}

/// A small frame holding one character (a tombstone at the end of an article: a 7.6 pt symbol in
/// a 10.5 pt box), in a paragraph whose style is 14 pt, with a text-wrap object on the spread at
/// `wrap`.
fn tombstone(frame: Rect, wrap: Rect) -> (Document, StoryId) {
    let (mut d, sid, _) = doc_with("X", frame, ParaAttrs::default());
    let st = d.story_mut(sid).unwrap();
    st.paras[0].chars = designcraft_doc::CharAttrs { size: Some(14.0), ..Default::default() };
    st.format_chars(0..1, |f| f.over.size = Some(7.6));
    let lid = d.default_layer();
    let mut it = designcraft_doc::Item::new(ItemId(d.alloc()), lid, designcraft_doc::Shape::Rectangle, designcraft_geom::shapes::rectangle(wrap));
    it.wrap.mode = WrapMode::BoundingBox;
    d.insert_item(SpreadRef::Doc(0), it, None).unwrap();
    (d, sid)
}

/// A text-wrap object elsewhere on the spread doesn't affect a frame it doesn't overlap: the small
/// frame's one character is set (it was overset: every frame took every wrap object on the
/// spread, and then no gap narrower than 1.5 em of the paragraph's size was used).
#[test]
fn small_single_character_frame_sets_beside_unrelated_text_wrap() {
    let (d, sid) = tombstone(Rect::new(300.0, 300.0, 310.5, 310.5), Rect::new(0.0, 0.0, 120.0, 60.0));
    let cs = compose_story(&d, sid, &ComposeOptions::default());
    assert!(!cs.is_overset(), "the tombstone is set");
    assert_eq!(cs.line_count(), 1);
}

/// A narrow frame that a text-wrap object overlaps further down sets the lines the object doesn't
/// reach: the minimum gap width only applies to gaps a wrap object cuts.
#[test]
fn narrow_frame_sets_lines_no_wrap_object_reaches() {
    let (d, sid) = tombstone(Rect::new(300.0, 300.0, 310.5, 340.0), Rect::new(290.0, 330.0, 330.0, 360.0));
    let cs = compose_story(&d, sid, &ComposeOptions::default());
    assert!(!cs.is_overset(), "the first line is above the wrap object");
    assert_eq!(cs.line_count(), 1);
    let (top, baseline) = (cs.frames[0].columns[0].y0, cs.frames[0].lines[0].baseline);
    assert!(baseline - top < 20.0, "on the first line: {baseline} (text area from {top})");
}

/// A wrap object that cuts a narrow frame leaves the sliver beside it (under 1.5 em) empty: no
/// line is set beside the object, and the text continues below it.
#[test]
fn a_wrap_object_cutting_a_narrow_frame_leaves_its_sliver_empty() {
    let text = ["of"; 400].join(" ");
    let (mut d, sid, _) = doc_with(&text, Rect::new(0.0, 0.0, 60.0, 400.0), ParaAttrs::default());
    d.story_mut(sid).unwrap().format_chars(0..text.len(), |f| f.over.size = Some(6.0));
    // An 8 pt sliver at 6 pt: narrower than 1.5 em (9 pt), wide enough for a word.
    add_wrap_objects(&mut d, &[(Rect::new(0.0, 50.0, 52.0, 120.0), [0.0; 4], WrapMode::BoundingBox)]);
    let cs = compose_story(&d, sid, &ComposeOptions::default());
    let lines = &cs.frames[0].lines;
    assert!(lines.iter().all(|l| l.baseline <= 50.0 + 1e-6 || l.baseline - l.ascent >= 120.0 - 1e-6), "a line beside the object");
    assert!(lines.iter().any(|l| l.baseline - l.ascent >= 120.0 - 1e-6), "no text below the object");
}

/// A wrap object beside the second column of a two-column frame leaves the first column, narrower
/// than 1.5 em, set in full.
#[test]
fn a_wrap_object_beside_column_two_leaves_a_narrow_column_one_set() {
    let (mut d, sid, fid) = doc_with(&["X"; 12].join(" "), Rect::new(0.0, 0.0, 46.0, 100.0), ParaAttrs::default());
    let o = &mut d.item_mut(fid).unwrap().text_frame_mut().unwrap().options;
    o.columns = 2;
    o.gutter = 12.0;
    // 17 pt columns at 14 pt: narrower than 1.5 em (21 pt).
    d.story_mut(sid).unwrap().format_chars(0..23, |f| f.over.size = Some(14.0));
    add_wrap_objects(&mut d, &[(Rect::new(35.0, 0.0, 60.0, 100.0), [0.0; 4], WrapMode::BoundingBox)]);
    let cs = compose_story(&d, sid, &ComposeOptions::default());
    let first = all_lines(&cs).iter().filter(|l| l.column == 0).count();
    assert!(first >= 4, "{first} lines in column 1");
}

/// In a frame of narrow columns, a wrap object beside column 2 leaves column 1 alone: only a gap
/// the object cuts in its own column is held to the minimum width (it was dropped, as the object
/// overlapped the line's band, and the text went past column 1).
#[test]
fn wrap_object_beside_one_narrow_column_leaves_the_other_alone() {
    // Two 18 pt columns (12 pt gutter): narrower than 1.5 em of the 14 pt paragraph.
    let (mut d, sid) = tombstone(Rect::new(300.0, 300.0, 348.0, 340.0), Rect::new(336.0, 300.0, 380.0, 340.0));
    let fid = d.spreads[0].items.iter().find(|i| i.text_frame().is_some_and(|t| t.story == sid)).unwrap().id;
    d.item_mut(fid).unwrap().text_frame_mut().unwrap().options.columns = 2;
    let cs = compose_story(&d, sid, &ComposeOptions::default());
    let f = &cs.frames[0];
    assert_eq!(f.columns.len(), 2);
    assert!(f.columns[0].width() < 21.0, "column 1 is narrower than 1.5 em: {}", f.columns[0].width());
    assert!(!cs.is_overset());
    let l = &f.lines[0];
    assert_eq!(l.column, 0, "set in column 1");
    assert!(l.baseline - f.columns[0].y0 < 20.0, "on its first line: {}", l.baseline);
}

/// Insert rectangles with wrap, given as (bounds, [top, left, bottom, right] offsets, mode).
fn add_wrap_objects(d: &mut Document, objects: &[(Rect, [f64; 4], WrapMode)]) {
    let lid = d.default_layer();
    for &(r, offsets, mode) in objects {
        let id = ItemId(d.alloc());
        let mut it = designcraft_doc::Item::new(id, lid, designcraft_doc::Shape::Rectangle, designcraft_geom::shapes::rectangle(r));
        it.wrap.mode = mode;
        it.wrap.offsets = offsets;
        d.insert_item(SpreadRef::Doc(0), it, None).unwrap();
    }
}

/// 10 pt on 12 pt text in a 288 × 528 frame at (36, 36), with wrap objects.
fn wrap_doc(para: ParaAttrs, objects: &[(Rect, [f64; 4], WrapMode)]) -> (Document, StoryId) {
    let text = [LOREM; 6].join(" ");
    let (mut d, sid, _) = doc_with(&text, Rect::new(36.0, 36.0, 324.0, 564.0), para);
    d.story_mut(sid).unwrap().format_chars(0..text.len(), |f| {
        f.over.size = Some(10.0);
        f.over.leading = Some(designcraft_doc::Leading::Points(12.0));
    });
    add_wrap_objects(&mut d, objects);
    (d, sid)
}

/// The first line whose baseline is below the top of `ex`.
fn first_line_below<'a>(cs: &'a ComposedStory, ex: &Exclusion) -> &'a Line {
    cs.frames[0].lines.iter().find(|l| l.baseline > ex.rect.y0).unwrap()
}

/// 14 pt on 18 pt `text` in a frame from y 62.36 to 532.91 with first baseline offset Leading, so lines
/// sit at 80.36 + n × 18 (the setup measured in InDesign), with Jump Object wraps given as
/// (bounds, [top, left, bottom, right] offsets).
fn jump_doc(text: &str, objects: &[(Rect, [f64; 4])]) -> (Document, StoryId, ItemId) {
    let (mut d, sid, fid) = doc_with(text, Rect::new(36.0, 62.36, 324.0, 532.91), ParaAttrs::default());
    d.story_mut(sid).unwrap().format_chars(0..text.len(), |f| {
        f.over.size = Some(14.0);
        f.over.leading = Some(designcraft_doc::Leading::Points(18.0));
    });
    d.item_mut(fid).unwrap().text_frame_mut().unwrap().options.first_baseline = FirstBaseline::Leading;
    let objects: Vec<_> = objects.iter().map(|&(r, o)| (r, o, WrapMode::JumpObject)).collect();
    add_wrap_objects(&mut d, &objects);
    (d, sid, fid)
}

/// Baselines of the last line above the top of the first wrap and of the first line below it.
fn around_wrap(d: &Document, sid: StoryId) -> (Option<f64>, f64) {
    let specs = frame_specs(d, sid);
    let ex = &specs[0].exclusions[0];
    let cs = compose_story(d, sid, &ComposeOptions::default());
    let lines = &cs.frames[0].lines;
    let above = lines.iter().rev().find(|l| l.baseline <= ex.rect.y0 + 1e-6).map(|l| l.baseline);
    (above, first_line_below(&cs, ex).baseline)
}

fn near(a: f64, b: f64) -> bool {
    (a - b).abs() < 0.01
}

#[test]
fn jump_object_moves_the_next_line_down_by_whole_leadings() {
    // As measured: last baseline above 386.41, wrap bottom 472.77 → 494.40 (6 leadings). Here the last
    // line above is at 386.36 and the wrap ends 86.36 below it.
    let text = [LOREM; 8].join(" ");
    let (d, sid, _) = jump_doc(&text, &[(Rect::new(36.0, 395.0, 200.0, 472.72), [0.0; 4])]);
    let (above, below) = around_wrap(&d, sid);
    assert!(near(above.unwrap(), 386.36), "{above:?}");
    assert!(near(below, 386.36 + 6.0 * 18.0), "{below}");
    // The lines after it keep the leading.
    let cs = compose_story(&d, sid, &ComposeOptions::default());
    let next = cs.frames[0].lines.iter().find(|l| l.baseline > below + 1.0).unwrap();
    assert!(near(next.baseline, below + 18.0));
}

#[test]
fn jump_object_clears_the_wrap_with_the_leading_not_the_ascent() {
    // As measured: last 152.38, wrap bottom 246.90 → 278.38. Clearing the wrap with the line's ascent
    // instead of its leading would put the line one leading higher (here the wrap ends a little higher,
    // as the test font's ascent is larger).
    let text = [LOREM; 8].join(" ");
    let (d, sid, _) = jump_doc(&text, &[(Rect::new(36.0, 160.0, 324.0, 244.0), [0.0; 4])]);
    let (above, below) = around_wrap(&d, sid);
    assert!(near(above.unwrap(), 152.36), "{above:?}");
    assert!(near(below, 152.36 + 7.0 * 18.0), "{below}");
    let cs = compose_story(&d, sid, &ComposeOptions::default());
    let asc = cs.frames[0].lines[0].ascent;
    let by_ascent = (1..).map(|k| 152.36 + k as f64 * 18.0).find(|b| b - asc >= 244.0).unwrap();
    assert!(!near(by_ascent, below), "the case must tell the two rules apart ({asc})");
}

#[test]
fn jump_object_adds_space_before_once() {
    // As measured: a heading at 78.36, then a paragraph with 60 pt space before, wrap bottom 439.71 →
    // 462.36 = 78.36 + 60 + 18 × 18.
    let text = format!("Heading\n{}", [LOREM; 6].join(" "));
    let (mut d, sid, _) = jump_doc(&text, &[(Rect::new(36.0, 90.0, 324.0, 439.71), [0.0; 4])]);
    d.story_mut(sid).unwrap().format_paras(10..10, |p| p.para.space_before = Some(60.0));
    let (above, below) = around_wrap(&d, sid);
    assert!(near(above.unwrap(), 80.36), "{above:?}");
    assert!(near(below, 80.36 + 60.0 + 18.0 * 18.0), "{below}");
}

#[test]
fn jump_object_at_the_frame_top_starts_text_like_a_frame_top() {
    // As measured: wrap bottom 266.83 → first baseline 284.835 (Leading offset: one leading below).
    let text = [LOREM; 8].join(" ");
    let (mut d, sid, fid) = jump_doc(&text, &[(Rect::new(36.0, 50.0, 324.0, 266.83), [0.0; 4])]);
    let cs = compose_story(&d, sid, &ComposeOptions::default());
    let lines = &cs.frames[0].lines;
    assert!(near(lines[0].baseline, 266.83 + 18.0), "{}", lines[0].baseline);
    assert!(near(lines[1].baseline, 266.83 + 36.0), "{}", lines[1].baseline);
    // Ascent offset: the line's ascent below the wrap.
    d.item_mut(fid).unwrap().text_frame_mut().unwrap().options.first_baseline = FirstBaseline::Ascent;
    let cs = compose_story(&d, sid, &ComposeOptions::default());
    let l = &cs.frames[0].lines[0];
    assert!(near(l.baseline, 266.83 + l.ascent), "{} vs {}", l.baseline, l.ascent);
}

#[test]
fn line_above_jump_object_stays_while_its_baseline_is_above_the_wrap() {
    let text = [LOREM; 8].join(" ");
    // The line at 386.36 hangs its descenders into the object and stays.
    let (d, sid, _) = jump_doc(&text, &[(Rect::new(36.0, 386.36, 324.0, 420.0), [0.0; 4])]);
    let (above, below) = around_wrap(&d, sid);
    assert!(near(above.unwrap(), 386.36), "{above:?}");
    assert!(near(below, 440.36), "{below}");
    // A wrap starting just above its baseline makes it jump.
    let (d, sid, _) = jump_doc(&text, &[(Rect::new(36.0, 386.3, 324.0, 420.0), [0.0; 4])]);
    let (above, below) = around_wrap(&d, sid);
    assert!(near(above.unwrap(), 368.36), "{above:?}");
    assert!(near(below, 440.36), "{below}");
}

#[test]
fn negative_jump_offsets_shrink_the_wrap() {
    let text = [LOREM; 8].join(" ");
    let (d, sid, _) = jump_doc(&text, &[(Rect::new(36.0, 300.0, 324.0, 500.0), [-20.0, 0.0, -30.0, 0.0])]);
    let specs = frame_specs(&d, sid);
    let r = specs[0].exclusions[0].rect;
    assert!(near(r.y0, 320.0) && near(r.y1, 470.0), "{r:?}");
    let (above, below) = around_wrap(&d, sid);
    assert!(near(above.unwrap(), 314.36), "{above:?}");
    assert!(near(below, 494.36), "{below}");
}

#[test]
fn jump_object_down_to_the_frame_bottom_sends_text_to_the_next_frame() {
    let text = [LOREM; 8].join(" ");
    let (mut d, sid, a) = jump_doc(&text, &[(Rect::new(36.0, 300.0, 324.0, 540.0), [0.0; 4])]);
    let lid = d.default_layer();
    let (b, _) = d.add_text_frame(SpreadRef::Doc(0), Rect::new(350.0, 62.36, 550.0, 532.91), lid, "", ParaFormat::default()).unwrap();
    d.item_mut(b).unwrap().text_frame_mut().unwrap().options.first_baseline = FirstBaseline::Leading;
    d.thread(a, b).unwrap();
    let cs = compose_story(&d, sid, &ComposeOptions::default());
    let last = cs.frames[0].lines.last().unwrap();
    assert!(near(last.baseline, 296.36), "{}", last.baseline);
    assert!(near(cs.frames[1].lines[0].baseline, 62.36 + 18.0), "{}", cs.frames[1].lines[0].baseline);
    assert_eq!(cs.frames[0].range.end, cs.frames[1].range.start);
}

#[test]
fn full_width_bounding_box_wrap_resumes_exactly_below() {
    // A bounding-box wrap across the whole column leaves no slot: the line jumps it like Jump Object.
    let (d, sid) = wrap_doc(ParaAttrs::default(), &[(Rect::new(20.0, 100.0, 340.0, 150.25), [0.0; 4], WrapMode::BoundingBox)]);
    let specs = frame_specs(&d, sid);
    let cs = compose_story(&d, sid, &ComposeOptions::default());
    let ex = &specs[0].exclusions[0];
    let l = first_line_below(&cs, ex);
    assert!((l.baseline - l.ascent - ex.rect.y1).abs() < 0.01, "{} vs {}", l.baseline - l.ascent, ex.rect.y1);
    // A slot narrower than 1.5 × the size is no slot either.
    let (d, sid) = wrap_doc(ParaAttrs::default(), &[(Rect::new(50.0, 100.0, 340.0, 150.25), [0.0; 4], WrapMode::BoundingBox)]);
    let specs = frame_specs(&d, sid);
    let cs = compose_story(&d, sid, &ComposeOptions::default());
    let ex = &specs[0].exclusions[0];
    let l = first_line_below(&cs, ex);
    assert!((l.baseline - l.ascent - ex.rect.y1).abs() < 0.01, "{} vs {}", l.baseline - l.ascent, ex.rect.y1);
}

#[test]
fn jump_object_keeps_grid_aligned_lines_on_the_grid() {
    // The wrap ends at 166.5, off the 12 pt grid; a second object blocks the grid line the line jumps
    // to, so the line moves on to the grid line after that object.
    let objects = [
        (Rect::new(36.0, 100.0, 200.0, 160.5), [0.0, 0.0, 6.0, 0.0], WrapMode::JumpObject),
        (Rect::new(250.0, 178.0, 324.0, 190.0), [0.0; 4], WrapMode::JumpObject),
    ];
    let (d, sid) = wrap_doc(ParaAttrs { grid_align: Some(GridAlign::AllLines), ..Default::default() }, &objects);
    let specs = frame_specs(&d, sid);
    let (g0, inc) = specs[0].grid.unwrap();
    let cs = compose_story(&d, sid, &ComposeOptions::default());
    let on_grid = |b: f64| (((b - g0) / inc).round() * inc + g0 - b).abs() < 1e-6;
    let lines = &cs.frames[0].lines;
    assert!(lines.iter().all(|l| on_grid(l.baseline)), "{:?}", lines.iter().map(|l| l.baseline).collect::<Vec<_>>());
    let exs = &specs[0].exclusions;
    let l = first_line_below(&cs, &exs[0]);
    // Naive oracle: the first grid line whose leading band clears the objects.
    let clear = |b: f64, exs: &[Exclusion]| exs.iter().all(|e| b <= e.rect.y0 || b - l.leading >= e.rect.y1);
    let want = |exs: &[Exclusion]| (0..).map(|n| g0 + n as f64 * inc).find(|&b| b > exs[0].rect.y0 && clear(b, exs)).unwrap();
    assert!((l.baseline - want(exs)).abs() < 1e-6, "baseline {} should be on grid line {}", l.baseline, want(exs));
    assert!(want(&exs[..1]) < want(exs), "the second object pushed the line again");

    // First line only: a later line of the paragraph jumps by whole leadings from the line above.
    let (d, sid) = wrap_doc(ParaAttrs { grid_align: Some(GridAlign::FirstLineOnly), ..Default::default() }, &objects[..1]);
    let specs = frame_specs(&d, sid);
    let cs = compose_story(&d, sid, &ComposeOptions::default());
    let ex = &specs[0].exclusions[0];
    let l = first_line_below(&cs, ex);
    assert!(!l.first_in_para);
    let above = cs.frames[0].lines.iter().rev().find(|a| a.baseline <= ex.rect.y0).unwrap().baseline;
    let want = (1..).map(|k| above + k as f64 * 12.0).find(|b| b - 12.0 >= ex.rect.y1).unwrap();
    assert!((l.baseline - want).abs() < 1e-6, "{} vs {want}", l.baseline);
}

/// An automatic page number wider than its frame stays on one line and hangs out, as in InDesign
/// ("136" in a frame made for two digits); breaking it put "13" over "4".
#[test]
fn page_number_wider_than_its_frame_stays_whole() {
    let text = designcraft_doc::story::PAGE_NUMBER.to_string();
    let (d, sid, _) = doc_with(&text, Rect::new(0.0, 0.0, 10.0, 40.0), ParaAttrs::default());
    let mut specs = frame_specs(&d, sid);
    specs[0].page_name = Some("136".into());
    let cs = compose(&d, d.story(sid).unwrap(), &specs, &ComposeOptions::default());
    let lines = all_lines(&cs);
    assert_eq!(lines.len(), 1, "{lines:?}");
    assert!(!cs.is_overset());
    // A word of ordinary letters that doesn't fit still breaks where it must.
    let (d, sid, _) = doc_with("Wordwiderthanframe", Rect::new(0.0, 0.0, 30.0, 200.0), ParaAttrs::default());
    assert!(all_lines(&compose_story(&d, sid, &ComposeOptions::default())).len() > 1);
}

#[test]
fn hostile_wrap_geometry_terminates() {
    let (d, sid) = wrap_doc(ParaAttrs { grid_align: Some(GridAlign::AllLines), ..Default::default() }, &[]);
    let mut specs = frame_specs(&d, sid);
    let ex = |x0: f64, y0: f64, x1: f64, y1: f64, mode| Exclusion {
        rect: Rect::new(x0, y0, x1, y1),
        mode,
        side: WrapSide::BothSides,
        contour: None,
        after: None,
    };
    let f = &mut specs[0];
    f.exclusions.push(ex(f64::NAN, f64::NAN, f64::NAN, f64::NAN, WrapMode::JumpObject));
    f.exclusions.push(ex(-1e308, 300.0, 1e308, f64::INFINITY, WrapMode::JumpObject));
    f.exclusions.push(ex(0.0, f64::NEG_INFINITY, 400.0, 40.0, WrapMode::BoundingBox));
    // A wall of overlapping strips, each ending 0.25 pt below the previous.
    for i in 0..3000 {
        let y = 60.0 + i as f64 * 0.25;
        f.exclusions.push(ex(0.0, y, 400.0, y + 1.0, if i % 2 == 0 { WrapMode::JumpObject } else { WrapMode::BoundingBox }));
    }
    f.grid = Some((0.0, 1e-300));
    let story = d.story(sid).unwrap();
    let cs = compose(&d, story, &specs, &ComposeOptions::default());
    assert!(cs.is_overset());
    for l in &cs.frames[0].lines {
        assert!(l.baseline.is_finite() && l.baseline - l.ascent >= 40.0 && l.baseline + l.descent <= 300.0, "{}", l.baseline);
    }
}

/// A text frame (0,0)-(300,800) with an ellipse (110,100)-(190,200) wrapped as `mode`/`side`.
fn doc_with_ellipse(mode: WrapMode, side: WrapSide) -> (Document, StoryId) {
    let (mut d, sid, _) = doc_with(&[LOREM; 4].join(" "), Rect::new(0.0, 0.0, 300.0, 800.0), ParaAttrs::default());
    let lid = d.default_layer();
    let id = ItemId(d.alloc());
    let mut it =
        designcraft_doc::Item::new(id, lid, designcraft_doc::Shape::Oval, designcraft_geom::shapes::ellipse(Rect::new(110.0, 100.0, 190.0, 200.0)));
    it.wrap.mode = mode;
    it.wrap.side = side;
    it.wrap.offsets = [4.0; 4];
    d.insert_item(SpreadRef::Doc(0), it, None).unwrap();
    (d, sid)
}

#[test]
fn wrap_on_both_sides_sets_text_left_then_right_of_the_object() {
    let (d, sid) = doc_with_ellipse(WrapMode::Contour, WrapSide::BothSides);
    let cs = compose_story(&d, sid, &ComposeOptions::default());
    let lines = &cs.frames[0].lines;
    let pairs: Vec<(&Line, &Line)> = lines.windows(2).filter(|w| w[0].baseline == w[1].baseline).map(|w| (&w[0], &w[1])).collect();
    assert!(pairs.len() >= 4, "{} lines share a baseline", pairs.len());
    let ex = &frame_specs(&d, sid)[0].exclusions[0];
    for (l, r) in &pairs {
        // Left segment, then the right one; the text runs on from one to the other.
        assert!(l.x1 < 150.0 && r.x0 > 150.0, "{} {}", l.x1, r.x0);
        assert_eq!(l.range.end, r.range.start);
        // No glyph inside the wrapped outline at its line.
        let (a, b) = ex.contour.as_ref().unwrap().band_extent(l.baseline - l.ascent, l.baseline).unwrap();
        let n = l.glyphs.len().saturating_sub(1);
        // The left segment's last glyph may be the space the line breaks at.
        for g in l.glyphs.iter().take(n).chain(&r.glyphs).filter(|g| g.visible) {
            assert!(g.x + g.adv <= a + 0.5 || g.x >= b - 0.5, "glyph at {}..{} in {a}..{b}", g.x, g.x + g.adv);
        }
    }
    // Lines beside the narrow top of the ellipse reach closer to its centre than those at its middle.
    let mid = pairs.iter().map(|(l, _)| l.x1).fold(f64::INFINITY, f64::min);
    let top = pairs.first().unwrap().0.x1;
    assert!(top > mid + 5.0, "top {top}, middle {mid}");
    assert!(!cs.is_overset());
}

/// Same As Clipping follows a placed graphic's clipping path: beside a triangle that widens
/// downwards, text reaches further in at the narrow top than at the wide bottom.
#[test]
fn same_as_clipping_wrap_follows_the_clipping_path() {
    use designcraft_geom::{PathData, Point, SubPath};
    let (mut d, sid, _) = doc_with(&[LOREM; 4].join(" "), Rect::new(0.0, 0.0, 300.0, 800.0), ParaAttrs::default());
    let lid = d.default_layer();
    let id = ItemId(d.alloc());
    let mut it = designcraft_doc::Item::new(
        id,
        lid,
        designcraft_doc::Shape::Rectangle,
        designcraft_geom::shapes::rectangle(Rect::new(150.0, 100.0, 300.0, 300.0)),
    );
    let triangle = PathData::single(SubPath::polyline(&[Point::new(75.0, 0.0), Point::new(150.0, 200.0), Point::new(0.0, 200.0)], true));
    it.content = designcraft_doc::Content::Graphic(designcraft_doc::Graphic {
        asset: designcraft_doc::AssetId(1),
        size: (150.0, 200.0),
        xf: designcraft_geom::Affine::translate((150.0, 100.0)),
        clip: Some(std::sync::Arc::new(designcraft_doc::ClippingPath::new(designcraft_doc::ClippingKind::DetectEdges, triangle))),
        auto_fit: Default::default(),
        fit_align: 0,
        crop: [0.0; 4],
        wrap: Default::default(),
        fill: Default::default(),
    });
    it.wrap.mode = WrapMode::Contour;
    it.wrap.side = WrapSide::LeftSide;
    it.wrap.contour = designcraft_doc::ContourType::SameAsClipping;
    d.insert_item(SpreadRef::Doc(0), it, None).unwrap();
    let cs = compose_story(&d, sid, &ComposeOptions::default());
    let beside: Vec<&Line> = cs.frames[0].lines.iter().filter(|l| l.baseline - l.ascent > 100.0 && l.baseline + l.descent < 300.0).collect();
    let (top, bottom) = (beside.first().unwrap().x1, beside.last().unwrap().x1);
    assert!(top > bottom + 40.0, "top {top}, bottom {bottom}");
    assert!(bottom < 160.0, "the bottom lines stop near the triangle's base: {bottom}");
}

/// LOREM beside an ellipse wrapped on both sides along its contour that leaves a sliver (about
/// 20 pt at its middle) between it and the frame's right edge.
fn sliver_wrap(para: ParaAttrs) -> (Document, StoryId, String) {
    let text = [LOREM; 4].join(" ");
    let (mut d, sid, _) = doc_with(&text, Rect::new(0.0, 0.0, 300.0, 800.0), para);
    let lid = d.default_layer();
    let id = ItemId(d.alloc());
    let mut it =
        designcraft_doc::Item::new(id, lid, designcraft_doc::Shape::Oval, designcraft_geom::shapes::ellipse(Rect::new(150.0, 100.0, 276.0, 220.0)));
    it.wrap.mode = WrapMode::Contour;
    it.wrap.side = WrapSide::BothSides;
    it.wrap.offsets = [4.0; 4];
    d.insert_item(SpreadRef::Doc(0), it, None).unwrap();
    (d, sid, text)
}

#[test]
fn words_stay_whole_beside_a_wrap_sliver() {
    for align in [Align::Left, Align::LeftJustified] {
        for hyphenate in [true, false] {
            let (d, sid, text) = sliver_wrap(ParaAttrs { align: Some(align), hyphenate: Some(hyphenate), ..Default::default() });
            let ex = &frame_specs(&d, sid)[0].exclusions[0];
            let cs = compose_story(&d, sid, &ComposeOptions::default());
            let lines = all_lines(&cs);
            let word = |i: usize| text.as_bytes().get(i).is_some_and(u8::is_ascii_alphanumeric);
            let mut empty_slivers = 0;
            for (i, l) in lines.iter().enumerate() {
                let line = &text[l.range.clone()];
                // A line ends inside a word only at a hyphenation point.
                assert!(l.hyphenated || !(word(l.range.end - 1) && word(l.range.end)), "{align:?} {hyphenate}: split word in {line:?}");
                // Every word fits its segment whole.
                for g in l.glyphs.iter().filter(|g| g.visible) {
                    assert!(g.x + g.adv <= l.x1 + 0.5, "{align:?} {hyphenate}: {line:?} overflows its {}..{} segment", l.x0, l.x1);
                }
                // A left segment beside the object whose sliver stays empty: the next word doesn't fit it.
                let Some((_, b)) = ex.contour.as_ref().unwrap().band_extent(l.baseline - l.ascent, l.baseline) else { continue };
                let next = lines.get(i + 1);
                if l.x1 > b || 300.0 - b < 18.0 || next.is_some_and(|n| n.baseline == l.baseline) {
                    continue;
                }
                let Some(next) = next else { continue };
                let first = text[next.range.clone()].split(' ').next().unwrap_or_default();
                let w: f64 = next.glyphs.iter().filter(|g| g.byte < next.range.start + first.len()).map(|g| g.adv).sum();
                assert!(w > 300.0 - b, "{align:?} {hyphenate}: {first:?} ({w}) fits the {} pt sliver beside {line:?}", 300.0 - b);
                empty_slivers += 1;
            }
            if !hyphenate {
                assert!(empty_slivers > 0, "{align:?}: no sliver stays empty");
            }
            assert!(!cs.is_overset());
        }
    }
}

#[test]
fn one_sided_wrap_leaves_the_other_side_empty() {
    for (side, right) in [(WrapSide::RightSide, true), (WrapSide::LeftSide, false), (WrapSide::LargestArea, false)] {
        let (d, sid) = doc_with_ellipse(WrapMode::BoundingBox, side);
        let cs = compose_story(&d, sid, &ComposeOptions::default());
        let beside: Vec<&Line> = cs.frames[0].lines.iter().filter(|l| l.baseline - l.ascent < 200.0 && l.baseline + l.descent > 100.0).collect();
        assert!(!beside.is_empty());
        let mut rows: Vec<f64> = beside.iter().map(|l| l.baseline).collect();
        rows.dedup();
        assert_eq!(rows.len(), beside.len(), "{side:?}: one segment per line");
        for l in beside {
            assert!(if right { l.x0 >= 194.0 - 1e-6 } else { l.x1 <= 106.0 + 1e-6 }, "{side:?}: {}..{}", l.x0, l.x1);
        }
    }
}

#[test]
fn contour_wrap_follows_the_outline_band_by_band() {
    let (d, sid) = doc_with_ellipse(WrapMode::Contour, WrapSide::BothSides);
    let ex = &frame_specs(&d, sid)[0].exclusions[0];
    let c = ex.contour.as_ref().unwrap();
    let width = |y0: f64, y1: f64| c.band_extent(y0, y1).map_or(0.0, |(a, b)| b - a);
    let (top, mid, bottom) = (width(100.0, 104.0), width(146.0, 154.0), width(196.0, 200.0));
    // 80 wide plus the 4 pt offset each side at the middle; much narrower at the ends.
    assert!((mid - 88.0).abs() < 0.5, "{mid}");
    assert!(top < mid - 20.0 && bottom < mid - 20.0, "{top} {mid} {bottom}");
    // The band is the line's height, not its baseline: a band reaching the middle is full width.
    assert!((width(120.0, 150.0) - 88.0).abs() < 0.5);
    // Outside the outline and its offset: no block.
    assert!(c.band_extent(205.0, 210.0).is_none());
    // A bounding-box wrap blocks the rectangle at every band.
    let (d, sid) = doc_with_ellipse(WrapMode::BoundingBox, WrapSide::BothSides);
    let ex = &frame_specs(&d, sid)[0].exclusions[0];
    assert!(ex.contour.is_none());
    assert_eq!(ex.rect, Rect::new(106.0, 96.0, 194.0, 204.0));
}

/// A contour wrap with a slanted right edge, x = 100 + 2(y − 50) for y in 50..100, text to its right.
fn doc_with_slant(contour: designcraft_geom::path::PathData) -> (Document, StoryId) {
    let (mut d, sid, _) = doc_with(&[LOREM; 2].join(" "), Rect::new(0.0, 0.0, 300.0, 800.0), ParaAttrs::default());
    let lid = d.default_layer();
    let mut it = designcraft_doc::Item::new(ItemId(d.alloc()), lid, designcraft_doc::Shape::Polygon, contour);
    it.wrap.mode = WrapMode::Contour;
    it.wrap.side = WrapSide::RightSide;
    d.insert_item(SpreadRef::Doc(0), it, None).unwrap();
    (d, sid)
}

#[test]
fn a_line_meets_the_wrap_from_its_top_to_its_baseline() {
    let pts = [Point::new(0.0, 50.0), Point::new(100.0, 50.0), Point::new(200.0, 100.0), Point::new(0.0, 100.0)];
    let path = designcraft_geom::path::PathData::single(designcraft_geom::path::SubPath::polyline(&pts, true));
    let (d, sid) = doc_with_slant(path);
    let cs = compose_story(&d, sid, &ComposeOptions::default());
    let beside: Vec<&Line> = cs.frames[0].lines.iter().filter(|l| l.baseline - l.ascent > 50.0 && l.baseline < 100.0).collect();
    assert!(beside.len() >= 2, "{} lines beside the slant", beside.len());
    for l in beside {
        // The edge at the baseline, not at the bottom of the descenders (about 6 pt further out).
        let edge = 100.0 + 2.0 * (l.baseline - 50.0);
        assert!((l.x0 - edge).abs() < 0.5, "line at {} starts at {}, edge {edge}", l.baseline, l.x0);
    }
}

#[test]
fn an_image_contour_stops_at_its_frame() {
    let (mut d, sid, _) = doc_with(&[LOREM; 2].join(" "), Rect::new(0.0, 0.0, 300.0, 800.0), ParaAttrs::default());
    let lid = d.default_layer();
    // An L-shaped frame: its right edge is at x = 150 down to y = 150, then at 250 below.
    let pts = [(0.0, 50.0), (150.0, 50.0), (150.0, 150.0), (250.0, 150.0), (250.0, 200.0), (0.0, 200.0)].map(|(x, y)| Point::new(x, y));
    let frame = designcraft_geom::path::PathData::single(designcraft_geom::path::SubPath::polyline(&pts, true));
    let mut it = designcraft_doc::Item::new(ItemId(d.alloc()), lid, designcraft_doc::Shape::Polygon, frame);
    // The image overhangs the frame on every side.
    it.content = designcraft_doc::Content::Graphic(designcraft_doc::Graphic {
        asset: designcraft_doc::AssetId(d.alloc()),
        size: (300.0, 250.0),
        xf: designcraft_geom::Affine::IDENTITY,
        clip: None,
        auto_fit: designcraft_doc::Fitting::None,
        fit_align: 4,
        crop: [0.0; 4],
        wrap: Default::default(),
        fill: Default::default(),
    });
    it.wrap.mode = WrapMode::Contour;
    it.wrap.side = WrapSide::RightSide;
    it.wrap.contour = designcraft_doc::ContourType::BoundingBox;
    it.wrap.offsets = [4.0; 4];
    d.insert_item(SpreadRef::Doc(0), it, None).unwrap();
    let cs = compose_story(&d, sid, &ComposeOptions::default());
    let beside: Vec<&Line> = cs.frames[0].lines.iter().filter(|l| l.baseline - l.ascent > 54.0 && l.baseline < 146.0).collect();
    assert!(beside.len() >= 3, "{} lines beside the frame", beside.len());
    for l in beside {
        assert!((l.x0 - 154.0).abs() < 0.5, "line at {} starts at {}", l.baseline, l.x0);
    }
    // Below the step the frame is wider.
    let low = cs.frames[0].lines.iter().find(|l| l.baseline - l.ascent > 154.0 && l.baseline < 196.0).unwrap();
    assert!((low.x0 - 254.0).abs() < 0.5, "{}", low.x0);
}

/// A clipping path (Detect Edges with a large inset) that overhangs its frame: a Same As
/// Clipping wrap follows it only within the frame.
#[test]
fn a_clipping_path_contour_stops_at_its_frame() {
    let (mut d, sid, _) = doc_with(&[LOREM; 4].join(" "), Rect::new(0.0, 0.0, 300.0, 800.0), ParaAttrs::default());
    let lid = d.default_layer();
    let mut it = designcraft_doc::Item::new(
        ItemId(d.alloc()),
        lid,
        designcraft_doc::Shape::Rectangle,
        designcraft_geom::shapes::rectangle(Rect::new(150.0, 100.0, 300.0, 300.0)),
    );
    // The clip path reaches 100 pt left of the frame.
    let wide = designcraft_geom::path::PathData::single(designcraft_geom::path::SubPath::polyline(
        &[Point::new(-100.0, 0.0), Point::new(150.0, 0.0), Point::new(150.0, 200.0), Point::new(-100.0, 200.0)],
        true,
    ));
    it.content = designcraft_doc::Content::Graphic(designcraft_doc::Graphic {
        asset: designcraft_doc::AssetId(1),
        size: (150.0, 200.0),
        xf: designcraft_geom::Affine::translate((150.0, 100.0)),
        clip: Some(std::sync::Arc::new(designcraft_doc::ClippingPath::new(designcraft_doc::ClippingKind::DetectEdges, wide))),
        auto_fit: Default::default(),
        fit_align: 0,
        crop: [0.0; 4],
        wrap: Default::default(),
        fill: Default::default(),
    });
    it.wrap.mode = WrapMode::Contour;
    it.wrap.side = WrapSide::LeftSide;
    it.wrap.contour = designcraft_doc::ContourType::SameAsClipping;
    it.wrap.offsets = [4.0; 4];
    d.insert_item(SpreadRef::Doc(0), it, None).unwrap();
    let cs = compose_story(&d, sid, &ComposeOptions::default());
    let beside: Vec<&Line> = cs.frames[0].lines.iter().filter(|l| l.baseline - l.ascent > 104.0 && l.baseline < 296.0).collect();
    assert!(beside.len() >= 5, "{} lines beside the frame", beside.len());
    for l in beside {
        assert!(l.x1 > 140.0 && l.x1 <= 146.0 + 1e-6, "line at {} ends at {}", l.baseline, l.x1);
    }
}

#[test]
fn paragraph_composer_is_no_worse_than_greedy() {
    // Sum of squared slack over lines (excluding last) should not exceed the greedy result.
    let measure = 180.0;
    let mk = |c: designcraft_doc::Composer| {
        let (d, sid, _) = doc_with(
            &[LOREM; 2].join(" "),
            Rect::new(0.0, 0.0, measure, 4000.0),
            ParaAttrs { composer: Some(c), hyphenate: Some(false), ..Default::default() },
        );
        let cs = compose_story(&d, sid, &ComposeOptions::default());
        let lines = all_lines(&cs);
        let n = lines.len();
        lines[..n - 1].iter().map(|l| (l.x1 - l.end_x).powi(2)).sum::<f64>()
    };
    let kp = mk(designcraft_doc::Composer::Paragraph);
    let gr = mk(designcraft_doc::Composer::SingleLine);
    assert!(kp <= gr * 1.05 + 1.0, "kp {kp} greedy {gr}");
}

#[test]
fn tabs_align_to_stops() {
    let pa = ParaAttrs {
        tabs: Some(vec![designcraft_doc::TabStop { position: 100.0, align: TabAlign::Right, leader: String::new(), align_on: String::new() }]),
        ..Default::default()
    };
    let (d, sid, _) = doc_with("Name\t42", Rect::new(0.0, 0.0, 300.0, 100.0), pa);
    let cs = compose_story(&d, sid, &ComposeOptions::default());
    let l = &cs.frames[0].lines[0];
    assert!((l.end_x - 100.0).abs() < 0.5, "{}", l.end_x);
}

const SWATCH_TEXT: &str = "Color holds it all together. A restrained palette of two or three swatches, applied consistently to headlines, rules and backgrounds, gives a publication its voice. Spot inks, tints and gradients are all just named swatches, so a single change ripples through every page.";

/// Worst word-space ratio (excluding last lines) of `text` in a narrow justified column.
fn narrow_worst(para: ParaAttrs) -> f64 {
    let para = ParaAttrs { align: Some(Align::LeftJustified), first_line_indent: Some(12.0), hyph_min_word: Some(6), ..para };
    let mut d = Document::new(&NewDocument::default());
    let lid = d.default_layer();
    let (_, sid) = d
        .add_text_frame(SpreadRef::Doc(0), Rect::new(0.0, 0.0, 170.67, 2000.0), lid, SWATCH_TEXT, ParaFormat { para, ..Default::default() })
        .unwrap();
    d.story_mut(sid).unwrap().format_chars(0..SWATCH_TEXT.len(), |f| f.over.size = Some(9.75));
    let cs = compose_story(&d, sid, &ComposeOptions::default());
    let mut worst = 0.0f64;
    for l in all_lines(&cs) {
        eprintln!("{:5.2} {}", l.spacing, &SWATCH_TEXT[l.range.clone()]);
        assert!(l.end_x <= l.x1 + 0.5, "overfull line");
        if !l.last_in_para {
            assert!((l.end_x - l.x1).abs() < 0.6, "justified line ends at {} not {}", l.end_x, l.x1);
            worst = worst.max(l.spacing);
        }
    }
    worst
}

#[test]
fn justified_narrow_column_has_no_extreme_lines() {
    // Word spacing only (InDesign's defaults: letter spacing 0%, glyph scaling 100%). The first
    // line can't do better than ~2.7: "…together. A re-" is the longest first line that fits.
    let words_only = narrow_worst(ParaAttrs::default());
    assert!(words_only < 4.5, "loose line ratio {words_only}");
    // A typical magazine body setup: letter spacing −5…10% and glyph scaling 97…103% absorb the rest.
    let full = narrow_worst(ParaAttrs {
        letter_space_min: Some(-0.05),
        letter_space_max: Some(0.10),
        glyph_scale_min: Some(0.97),
        glyph_scale_max: Some(1.03),
        ..Default::default()
    });
    assert!(full < 2.0, "loose line ratio {full}");
}

#[test]
fn glyph_scaling_and_letter_spacing_are_applied() {
    let para = ParaAttrs {
        align: Some(Align::LeftJustified),
        hyphenate: Some(false),
        letter_space_min: Some(-0.05),
        letter_space_max: Some(0.2),
        glyph_scale_min: Some(0.95),
        glyph_scale_max: Some(1.05),
        ..Default::default()
    };
    let text = CORPUS.join(" ");
    let (d, sid, _) = doc_with(&text, Rect::new(0.0, 0.0, 150.0, 20000.0), para.clone());
    let cs = compose_story(&d, sid, &ComposeOptions::default());
    let lines = all_lines(&cs);
    let mut scaled = false;
    let mut sum = 0.0;
    for l in &lines[..lines.len() - 1] {
        assert!((l.end_x - l.x1).abs() < 0.6, "justified line ends at {} not {}", l.end_x, l.x1);
        sum += l.spacing;
        let k = 12.0 / l.glyphs[0].face.upem;
        scaled |= l.glyphs.iter().any(|g| g.visible && (g.sx / k - 1.0).abs() > 1e-3);
    }
    assert!(scaled, "some line uses glyph scaling");
    // Word spaces stay closer to their desired width than with word spacing alone.
    let plain = ParaAttrs { align: Some(Align::LeftJustified), hyphenate: Some(false), ..Default::default() };
    let (d, sid, _) = doc_with(&text, Rect::new(0.0, 0.0, 150.0, 20000.0), plain);
    let cs = compose_story(&d, sid, &ComposeOptions::default());
    let pl = all_lines(&cs);
    let plain_avg = pl[..pl.len() - 1].iter().map(|l| l.spacing).sum::<f64>() / (pl.len() - 1) as f64;
    let avg = sum / (lines.len() - 1) as f64;
    assert!(avg < plain_avg - 0.1, "with tiers {avg:.3}, words only {plain_avg:.3}");
    // Desired values apply to every line, including ragged text.
    let wide = |ls: f64| {
        let (d, sid, _) =
            doc_with("Hello world", Rect::new(0.0, 0.0, 300.0, 100.0), ParaAttrs { letter_space_desired: Some(ls), ..Default::default() });
        compose_story(&d, sid, &ComposeOptions::default()).frames[0].lines[0].end_x
    };
    assert!(wide(0.5) > wide(0.0) + 10.0 * 0.5 * 2.0);
}

/// Width of `text` set at 48 pt with `kerning`, and the advance of each visible glyph.
fn set_with_kerning(text: &str, kerning: designcraft_doc::Kerning) -> (f64, Vec<(char, f64)>) {
    let (mut d, sid, _) = doc_with(text, Rect::new(0.0, 0.0, 2000.0, 200.0), ParaAttrs::default());
    d.story_mut(sid).unwrap().format_chars(0..text.len(), |f| {
        f.over.size = Some(48.0);
        f.over.kerning = Some(kerning);
    });
    let cs = compose_story(&d, sid, &ComposeOptions::default());
    let l = &cs.frames[0].lines[0];
    let advs = l.glyphs.iter().filter(|g| g.visible).map(|g| (text[g.byte..].chars().next().unwrap_or(' '), g.adv)).collect();
    (l.end_x - l.x0, advs)
}

#[test]
fn optical_kerning_closes_open_pairs_and_leaves_stems() {
    use designcraft_doc::Kerning;
    // `AV` and `To` set tighter than unkerned; `nn`, the reference pair, stays as the font set it.
    for pair in ["AV", "To", "r."] {
        let (none, _) = set_with_kerning(pair, Kerning::None);
        let (optical, _) = set_with_kerning(pair, Kerning::Optical);
        assert!(optical < none - 0.5, "{pair}: optical {optical:.2} vs none {none:.2}");
    }
    let (none, _) = set_with_kerning("nn", Kerning::None);
    let (optical, _) = set_with_kerning("nn", Kerning::Optical);
    assert!((none - optical).abs() < 48.0 * 0.02, "nn: optical {optical:.2} vs none {none:.2}");
    // Optical is measured, not the font's pairs: `HH` has no kern pair but its gap is wider
    // than `nn`'s, so it closes up under Optical and not under Metrics.
    let (metrics, _) = set_with_kerning("HH", Kerning::Metrics);
    let (optical, _) = set_with_kerning("HH", Kerning::Optical);
    assert!(optical < metrics - 0.5, "HH: optical {optical:.2} vs metrics {metrics:.2}");
}

#[test]
fn optical_kerning_is_bounded_and_skips_spaces() {
    use designcraft_doc::Kerning;
    let text = "Sphinx of black quartz, judge my vow. 1997 “Park” AWAY";
    let (_, none) = set_with_kerning(text, Kerning::None);
    let (_, optical) = set_with_kerning(text, Kerning::Optical);
    assert_eq!(none.len(), optical.len());
    for ((c, a), (_, b)) in none.iter().zip(&optical) {
        let em = (b - a) / 48.0;
        assert!((-0.12 - 1e-6..=0.06 + 1e-6).contains(&em), "{c:?}: {em:.3} em");
        if c.is_whitespace() {
            assert!((a - b).abs() < 1e-9, "space before a word keeps its width");
        }
    }
    // The glyph before a space is not kerned against it either.
    let (_, none) = set_with_kerning("r ", Kerning::None);
    let (_, optical) = set_with_kerning("r ", Kerning::Optical);
    assert!((none[0].1 - optical[0].1).abs() < 1e-9);
}

#[test]
fn optical_kerning_leaves_dash_pairs_alone() {
    use designcraft_doc::Kerning;
    // Two bars end to end have a near-zero ink gap; pulling them towards the stem gap would
    // break `--` and the double em dash `——` that Chinese sets as one continuous rule.
    for text in ["--", "\u{2014}\u{2014}", "a--b", "\u{2013}\u{2013}"] {
        let (none_w, none) = set_with_kerning(text, Kerning::None);
        let (optical_w, optical) = set_with_kerning(text, Kerning::Optical);
        assert!((none_w - optical_w).abs() < 1e-9, "{text:?}: optical {optical_w:.3} vs none {none_w:.3}");
        for ((c, a), (_, b)) in none.iter().zip(&optical) {
            assert!((a - b).abs() < 1e-9, "{text:?} {c:?}: {a:.3} vs {b:.3}");
        }
    }
}

#[test]
fn hyphen_limit_is_a_hard_constraint() {
    let text = CORPUS.join(" ");
    for limit in [1u32, 2] {
        let para = ParaAttrs { align: Some(Align::LeftJustified), hyph_limit: Some(limit), hyph_weight: Some(0.0), ..Default::default() };
        let (d, sid, _) = doc_with(&text, Rect::new(0.0, 0.0, 130.0, 100_000.0), para);
        let cs = compose_story(&d, sid, &ComposeOptions::default());
        let mut run = 0;
        let mut any = false;
        for l in all_lines(&cs) {
            run = if l.hyphenated { run + 1 } else { 0 };
            any |= l.hyphenated;
            assert!(run <= limit, "{run} consecutive hyphens with limit {limit}");
        }
        assert!(any);
    }
}

#[test]
fn hyphenation_zone_limits_ragged_hyphens() {
    let text = CORPUS.join(" ");
    let count = |zone: f64, composer: designcraft_doc::Composer| {
        let para =
            ParaAttrs { align: Some(Align::Left), hyph_zone: Some(zone), composer: Some(composer), hyph_weight: Some(0.0), ..Default::default() };
        let (d, sid, _) = doc_with(&text, Rect::new(0.0, 0.0, 110.0, 100_000.0), para);
        let cs = compose_story(&d, sid, &ComposeOptions::default());
        all_lines(&cs).iter().filter(|l| l.hyphenated).count()
    };
    // Single-line Composer: a huge zone leaves only words that start a line (no space to break
    // at) hyphenable.
    let c = designcraft_doc::Composer::SingleLine;
    let (free, zoned, huge) = (count(0.0, c), count(36.0, c), count(1000.0, c));
    assert!(free > 0 && zoned <= free && huge * 4 <= free, "{c:?}: {free} {zoned} {huge}");
    // The zone is the Single-line Composer's only (InDesign): the Paragraph Composer hyphenates
    // the same whatever it is.
    let c = designcraft_doc::Composer::Paragraph;
    let (free, zoned, huge) = (count(0.0, c), count(36.0, c), count(1000.0, c));
    assert!(free > 0 && zoned == free && huge == free, "{c:?}: {free} {zoned} {huge}");
}

#[test]
fn discretionary_hyphen_replaces_automatic_points() {
    let text = "aaaa bbbb cccc extraordi\u{AD}narily dddd eeee";
    let para = ParaAttrs { align: Some(Align::Left), ..Default::default() };
    let (d, sid, _) = doc_with(text, Rect::new(0.0, 0.0, 120.0, 1000.0), para);
    let cs = compose_story(&d, sid, &ComposeOptions::default());
    let soft = text.find('\u{AD}').unwrap();
    for l in all_lines(&cs) {
        if l.hyphenated {
            let last = l.glyphs.iter().rfind(|g| g.len > 0).unwrap();
            assert!(last.byte + last.len >= soft && last.byte <= soft + 2, "{:?}", &text[l.range.clone()]);
        }
    }
    // Hyphenation points of the word itself: none besides the discretionary one.
    let lines = all_lines(&cs);
    assert!(lines.iter().filter(|l| l.hyphenated).count() <= 1);
}

#[test]
fn hyphenation_skips_a_no_break_word_at_the_start() {
    // Used to index before the first glyph and panic.
    let text = "Extraordinarily long words wrap in a narrow column";
    let (mut d, sid, _) = doc_with(text, Rect::new(0.0, 0.0, 60.0, 1000.0), ParaAttrs::default());
    d.story_mut(sid).unwrap().format_chars(0..15, |f| f.over.no_break = Some(true));
    let cs = compose_story(&d, sid, &ComposeOptions::default());
    // The no-break word stays whole on the first line.
    let first = all_lines(&cs)[0];
    assert!(!first.hyphenated && first.range.end >= 15, "{:?}", &text[first.range.clone()]);
}

/// Natural advance of each glyph of `text` by byte, and a frame width that puts the first three
/// words on a justified line with one word space of extra to share.
fn three_word_measure(text: &str) -> (std::collections::HashMap<usize, f64>, f64) {
    let (d, sid, _) = doc_with(text, Rect::new(0.0, 0.0, 1000.0, 100.0), ParaAttrs::default());
    let cs = compose_story(&d, sid, &ComposeOptions::default());
    let line = all_lines(&cs)[0];
    let adv = line.glyphs.iter().map(|g| (g.byte, g.adv)).collect::<std::collections::HashMap<_, _>>();
    let third_end = text.find(" dddd").unwrap();
    let gap = line.glyphs.iter().find(|g| g.byte == third_end).unwrap();
    (adv, gap.x - line.x0 + gap.adv + 2.0 * line.x0)
}

fn justified_first_line(text: &str, width: f64, no_break: Option<std::ops::Range<usize>>) -> Line {
    let para = ParaAttrs { align: Some(Align::LeftJustified), word_space_max: Some(4.0), ..Default::default() };
    let (mut d, sid, _) = doc_with(text, Rect::new(0.0, 0.0, width, 1000.0), para);
    if let Some(run) = no_break {
        d.story_mut(sid).unwrap().format_chars(run, |f| f.over.no_break = Some(true));
    }
    let cs = compose_story(&d, sid, &ComposeOptions::default());
    let line = all_lines(&cs)[0].clone();
    assert!((line.end_x - line.x1).abs() < 0.1, "line ends at {} not {}", line.end_x, line.x1);
    line
}

#[test]
fn no_break_spaces_stretch_on_justified_lines() {
    let text = "aaaa bbbb cccc dddd eeee ffff gggg hhhh";
    let (natural, width) = three_word_measure(text);
    // The run holds one of the line's two spaces, then both.
    for run in [0..9, 0..14] {
        let line = justified_first_line(text, width, Some(run.clone()));
        assert_eq!(text[line.range.clone()].trim_end(), "aaaa bbbb cccc");
        let words = line.glyphs.iter().filter(|g| g.byte < 14);
        let (spaces, letters): (Vec<_>, Vec<_>) = words.partition(|g| text.as_bytes()[g.byte] == b' ');
        assert_eq!(spaces.len(), 2);
        for s in &spaces {
            assert!(s.adv > natural[&s.byte] + 1.0 && (s.adv - spaces[0].adv).abs() < 1e-6, "run {run:?}: space at {} is {}", s.byte, s.adv);
        }
        for g in letters {
            assert!((g.adv - natural[&g.byte]).abs() < 1e-6, "run {run:?}: letter at {} is {}", g.byte, g.adv);
        }
    }
}

#[test]
fn nonbreaking_space_stretches_and_fixed_width_one_does_not() {
    for (nb, stretches) in [('\u{A0}', true), ('\u{202F}', false)] {
        let text = format!("aaaa{nb}bbbb cccc dddd eeee ffff gggg hhhh");
        let (natural, width) = three_word_measure(&text);
        let line = justified_first_line(&text, width, None);
        let nb_glyph = line.glyphs.iter().find(|g| g.byte == 4).unwrap();
        let space = line.glyphs.iter().find(|g| text.as_bytes()[g.byte] == b' ').unwrap();
        assert!(space.adv > natural[&space.byte] + 1.0);
        if stretches {
            assert!((nb_glyph.adv - space.adv).abs() < 1e-6, "{nb:?}: {} vs {}", nb_glyph.adv, space.adv);
        } else {
            assert!((nb_glyph.adv - natural[&4]).abs() < 1e-6, "{nb:?}: {} vs {}", nb_glyph.adv, natural[&4]);
        }
    }
}

#[test]
fn jidori_run_keeps_its_width_on_a_justified_line() {
    fn first_line(text: &str, width: f64, para: ParaAttrs, run: std::ops::Range<usize>) -> Line {
        let (mut d, sid, _) = doc_with(text, Rect::new(0.0, 0.0, width, 1000.0), para);
        d.story_mut(sid).unwrap().format_chars(run, |f| f.over.jidori = Some(6));
        let cs = compose_story(&d, sid, &ComposeOptions::default());
        all_lines(&cs)[0].clone()
    }
    for sp in ['\u{A0}', ' '] {
        let text = format!("aa{sp}aa bbbb cccc dddd eeee ffff gggg hhhh");
        let run = 0..4 + sp.len_utf8();
        let run_width = |l: &Line| l.glyphs.iter().filter(|g| run.contains(&g.byte)).map(|g| g.adv).sum::<f64>();
        let ragged = first_line(&text, 1000.0, ParaAttrs::default(), run.clone());
        let gap = ragged.glyphs.iter().find(|g| g.byte == text.find(" dddd").unwrap()).unwrap();
        let width = gap.x - ragged.x0 + gap.adv + 2.0 * ragged.x0;
        let para = ParaAttrs { align: Some(Align::LeftJustified), word_space_max: Some(4.0), ..Default::default() };
        let line = first_line(&text, width, para, run.clone());
        assert_eq!(text[line.range.clone()].trim_end(), format!("aa{sp}aa bbbb cccc"));
        assert!((line.end_x - line.x1).abs() < 0.1, "{sp:?}: line ends at {} not {}", line.end_x, line.x1);
        assert!((run_width(&line) - run_width(&ragged)).abs() < 1e-6, "{sp:?}: run is {} not {}", run_width(&line), run_width(&ragged));
    }
}

#[test]
fn nonbreaking_space_alone_fills_a_justified_line() {
    let text = "10\u{A0}km 1234";
    let (d, sid, _) = doc_with(text, Rect::new(0.0, 0.0, 1000.0, 100.0), ParaAttrs::default());
    let cs = compose_story(&d, sid, &ComposeOptions::default());
    let ragged = all_lines(&cs)[0];
    let natural = ragged.glyphs.iter().map(|g| (g.byte, g.adv)).collect::<std::collections::HashMap<_, _>>();
    let m = ragged.glyphs.iter().find(|g| g.byte == 5).unwrap();
    let width = m.x - ragged.x0 + m.adv + 15.0 + 2.0 * ragged.x0;
    let line = justified_first_line(text, width, None);
    assert_eq!(text[line.range.clone()].trim_end(), "10\u{A0}km");
    for g in line.glyphs.iter().filter(|g| g.byte < 6) {
        if g.byte == 2 {
            assert!((g.adv - natural[&2] - 15.0).abs() < 0.1, "U+00A0 is {}", g.adv);
        } else {
            assert!((g.adv - natural[&g.byte]).abs() < 1e-6, "glyph at {} is {}", g.byte, g.adv);
        }
    }
}

/// Story of `n` one-line filler paragraphs followed by `extra` paragraphs, in a 2-column frame.
fn keep_doc(fillers: usize, extra: &[&str], height: f64) -> (Document, StoryId, Vec<usize>) {
    let mut parts: Vec<String> = (0..fillers).map(|k| format!("Filler {k}")).collect();
    parts.extend(extra.iter().map(ToString::to_string));
    let text = parts.join("\n");
    let (mut d, sid, fid) = doc_with(&text, Rect::new(0.0, 0.0, 400.0, height), ParaAttrs::default());
    d.item_mut(fid).unwrap().text_frame_mut().unwrap().options.columns = 2;
    (d, sid, (0..parts.len()).collect())
}

fn column_of_para(cs: &ComposedStory, pi: usize) -> Vec<u32> {
    all_lines(cs).iter().filter(|l| l.para == pi).map(|l| l.column).collect()
}

/// Lines that fit in one column of the keep test frame.
fn lines_per_column(height: f64) -> usize {
    let (d, sid, _) = keep_doc(60, &[], height);
    let cs = compose_story(&d, sid, &ComposeOptions::default());
    all_lines(&cs).iter().filter(|l| l.column == 0).count()
}

#[test]
fn keep_with_next_moves_a_heading() {
    let h = 200.0;
    let n = lines_per_column(h);
    let body = "Body text that follows the heading.";
    let (mut d, sid, _) = keep_doc(n - 1, &["Heading", body], h);
    let cs = compose_story(&d, sid, &ComposeOptions::default());
    assert_eq!(column_of_para(&cs, n - 1), vec![0], "without keeps the heading ends column 0");
    d.story_mut(sid).unwrap().paras[n - 1].para.keep_with_next = Some(1);
    let cs = compose_story(&d, sid, &ComposeOptions::default());
    assert_eq!(column_of_para(&cs, n - 1), vec![1]);
    assert_eq!(column_of_para(&cs, n), vec![1]);
    assert_eq!(column_of_para(&cs, n - 2), vec![0]);
}

#[test]
fn keep_lines_together_moves_the_paragraph() {
    let h = 200.0;
    let n = lines_per_column(h);
    let long = [LOREM, LOREM].join(" ");
    let (mut d, sid, _) = keep_doc(n - 3, &[&long], h);
    let cs = compose_story(&d, sid, &ComposeOptions::default());
    assert!(column_of_para(&cs, n - 3).contains(&0));
    {
        let p = &mut d.story_mut(sid).unwrap().paras[n - 3].para;
        p.keep_lines_together = Some(true);
        p.keep_all_lines = Some(true);
    }
    let cs = compose_story(&d, sid, &ComposeOptions::default());
    assert!(column_of_para(&cs, n - 3).iter().all(|&c| c == 1));
}

#[test]
fn keep_first_and_last_lines() {
    let h = 200.0;
    let n = lines_per_column(h);
    // Orphan: one line at the bottom of column 0 → the paragraph moves.
    let (mut d, sid, _) = keep_doc(n - 1, &[LOREM], h);
    {
        let p = &mut d.story_mut(sid).unwrap().paras[n - 1].para;
        p.keep_lines_together = Some(true);
        p.keep_all_lines = Some(false);
    }
    let cs = compose_story(&d, sid, &ComposeOptions::default());
    let cols = column_of_para(&cs, n - 1);
    assert!(cols.iter().all(|&c| c == 1), "{cols:?}");
    // Widow: a paragraph whose last line would sit alone in column 1 gives it a companion.
    let para_lines = {
        let (d, sid, _) = keep_doc(0, &[LOREM], 2000.0);
        compose_story(&d, sid, &ComposeOptions::default()).line_count()
    };
    assert!(para_lines >= 4);
    let start = n - (para_lines - 1);
    let (mut d, sid, _) = keep_doc(start, &[LOREM], h);
    let cs = compose_story(&d, sid, &ComposeOptions::default());
    assert_eq!(column_of_para(&cs, start).iter().filter(|&&c| c == 1).count(), 1, "one widowed line without keeps");
    {
        let p = &mut d.story_mut(sid).unwrap().paras[start].para;
        p.keep_lines_together = Some(true);
        p.keep_all_lines = Some(false);
    }
    let cs = compose_story(&d, sid, &ComposeOptions::default());
    let cols = column_of_para(&cs, start);
    assert_eq!(cols.iter().filter(|&&c| c == 1).count(), 2, "{cols:?}");
    assert!(cols.iter().filter(|&&c| c == 0).count() >= 2);
}

#[test]
fn balance_ragged_lines_evens_a_headline() {
    let text = "Balanced headlines read better than a stub";
    let widths = |balance: bool| {
        let para = ParaAttrs { balance_ragged: Some(balance), hyphenate: Some(false), ..Default::default() };
        let (mut d, sid, _) = doc_with(text, Rect::new(0.0, 0.0, 300.0, 500.0), para);
        d.story_mut(sid).unwrap().format_chars(0..text.len(), |f| f.over.size = Some(18.0));
        let cs = compose_story(&d, sid, &ComposeOptions::default());
        all_lines(&cs).iter().map(|l| l.end_x - l.x0).collect::<Vec<_>>()
    };
    let plain = widths(false);
    let bal = widths(true);
    assert_eq!(plain.len(), bal.len());
    assert!(plain.len() >= 2);
    let spread = |w: &[f64]| w.iter().cloned().fold(f64::MIN, f64::max) - w.iter().cloned().fold(f64::MAX, f64::min);
    assert!(spread(&bal) < spread(&plain), "{bal:?} vs {plain:?}");
}

#[test]
fn optical_margin_hangs_punctuation() {
    let text = "Hanging punctuation, quietly. “Quotes” sit outside the measure, as do commas, periods and hyphens.";
    let run = |optical: bool| {
        let para = ParaAttrs { align: Some(Align::LeftJustified), optical_margin: Some(optical), ..Default::default() };
        let (d, sid, _) = doc_with(text, Rect::new(0.0, 0.0, 120.0, 1000.0), para);
        compose_story(&d, sid, &ComposeOptions::default())
    };
    let plain = run(false);
    let hung = run(true);
    for l in all_lines(&plain) {
        assert!(l.end_x <= l.x1 + 0.5);
    }
    // Some justified line ends with punctuation that now protrudes past the right edge.
    let lines = all_lines(&hung);
    assert!(lines[..lines.len() - 1].iter().any(|l| l.end_x > l.x1 + 0.5), "no line hangs");
    assert!(lines.iter().all(|l| l.end_x <= l.x1 + 12.0));
}

// ---------- golden paragraph corpus ----------

/// Original prose (written for this corpus) with a realistic mix of short and long words.
const CORPUS: &[&str] = &[
    "Every printed page is a negotiation between the designer and the reader. The designer proposes an order: a column of type, a picture that interrupts it, a caption that explains the picture. The reader accepts that order only when nothing on the page calls attention to itself for the wrong reasons.",
    "Justified text is especially unforgiving. When the measure is narrow, the composer must choose between loose lines with rivers of white running down the column, tight lines in which words collide, and hyphenated lines that interrupt the rhythm of reading. Good composition balances all three considerations across the whole paragraph instead of one line at a time.",
    "Hyphenation dictionaries record where conventional syllable divisions fall in thousands of ordinary words: international, responsibility, characteristically, misunderstanding, photographer, extraordinarily, administration, comprehensive, establishment, unquestionably and their many relatives.",
    "Magazines often set body copy in two or three columns on a page, with generous gutters and a baseline grid that keeps neighbouring lines aligned. Headlines span the columns, pull quotes break into them, and captions sit beside photographs in a smaller, contrasting typeface.",
    "The typesetter of the nineteenth century worked with metal sorts, composing sticks and wedges of spacing material. Many of the conventions we take for granted today, such as the preference for consistent word spacing over consistent line endings, were established by those craftsmen long before computers automated their decisions.",
    "Readers rarely notice good typography, but they immediately feel the discomfort of bad typography: uneven spacing, awkward breaks, widowed lines stranded at the top of a column, and orphaned headings left at the bottom of a page without the paragraph that follows them.",
];

#[derive(Debug, Default)]
struct CorpusStats {
    lines: usize,
    overfull: usize,
    hyphens: usize,
    sum_spacing: f64,
    worst: f64,
    loose: usize,
}

fn corpus_stats(align: Align, measure: f64, size: f64) -> CorpusStats {
    let text = CORPUS.join("\n");
    let para = ParaAttrs { align: Some(align), ..Default::default() };
    let mut d = Document::new(&NewDocument::default());
    let lid = d.default_layer();
    let (_, sid) =
        d.add_text_frame(SpreadRef::Doc(0), Rect::new(0.0, 0.0, measure, 100_000.0), lid, &text, ParaFormat { para, ..Default::default() }).unwrap();
    d.story_mut(sid).unwrap().format_chars(0..text.len(), |f| f.over.size = Some(size));
    let cs = compose_story(&d, sid, &ComposeOptions::default());
    assert!(!cs.is_overset());
    let mut st = CorpusStats::default();
    for l in all_lines(&cs) {
        if l.end_x > l.x1 + 0.5 {
            st.overfull += 1;
        }
        if l.hyphenated {
            st.hyphens += 1;
        }
        if !l.last_in_para {
            st.lines += 1;
            st.sum_spacing += l.spacing;
            st.worst = st.worst.max(l.spacing);
            if l.spacing > 1.33 + 1e-6 {
                st.loose += 1;
            }
        }
    }
    st
}

#[test]
fn golden_corpus_spacing() {
    let mut total = CorpusStats::default();
    for &(measure, size) in &[(120.0, 9.0), (170.67, 9.75), (240.0, 10.0), (340.0, 11.0)] {
        for align in [Align::LeftJustified, Align::Left] {
            let st = corpus_stats(align, measure, size);
            let avg = st.sum_spacing / st.lines.max(1) as f64;
            eprintln!(
                "{align:?} measure {measure:6.2} size {size:5.2}: lines {:3} hyphens {:2} avg {avg:.3} worst {:.3} loose {:2} overfull {}",
                st.lines, st.hyphens, st.worst, st.loose, st.overfull
            );
            assert_eq!(st.overfull, 0, "overfull lines at {measure}");
            if align == Align::LeftJustified && measure >= 170.0 {
                assert!(st.worst < 3.5, "worst justified line {} at {measure}", st.worst);
                assert!(avg < 1.65, "average justified word-space ratio {avg} at {measure}");
            }
            if align == Align::LeftJustified {
                total.lines += st.lines;
                total.sum_spacing += st.sum_spacing;
                total.worst = total.worst.max(st.worst);
                total.loose += st.loose;
            }
        }
    }
    let avg = total.sum_spacing / total.lines.max(1) as f64;
    eprintln!("justified total: lines {} avg {avg:.3} worst {:.3} loose {}", total.lines, total.worst, total.loose);
    assert!(avg < 1.9, "average justified word-space ratio {avg}");
}

#[test]
#[ignore = "perf: run with --release -- --ignored --nocapture"]
fn perf_compose_300k_story() {
    let mut text = String::new();
    let mut k = 0;
    while text.len() < 300_000 {
        text.push_str(CORPUS[k % CORPUS.len()]);
        text.push('\n');
        k += 1;
    }
    let para = ParaAttrs { align: Some(Align::LeftJustified), ..Default::default() };
    let mut d = Document::new(&NewDocument::default());
    let lid = d.default_layer();
    let (_, sid) =
        d.add_text_frame(SpreadRef::Doc(0), Rect::new(0.0, 0.0, 240.0, 3_000_000.0), lid, &text, ParaFormat { para, ..Default::default() }).unwrap();
    // Cold run (loads hyphenation data, fills the word caches), then the best of a few warm runs.
    let t = std::time::Instant::now();
    let _ = compose_story(&d, sid, &ComposeOptions::default());
    let cold = t.elapsed().as_secs_f64() * 1000.0;
    let mut ms = f64::INFINITY;
    let mut cs = ComposedStory::default();
    for _ in 0..5 {
        let t = std::time::Instant::now();
        cs = compose_story(&d, sid, &ComposeOptions::default());
        ms = ms.min(t.elapsed().as_secs_f64() * 1000.0);
    }
    eprintln!("composed {} chars, {} lines: cold {cold:.1} ms, warm {ms:.1} ms", text.len(), cs.line_count());
    assert!(!cs.is_overset());
    if !cfg!(debug_assertions) {
        assert!(ms < 300.0, "composition took {ms:.1} ms");
    }
}

#[test]
fn column_break_moves_following_text() {
    let text = format!("First column text.{}Second column text.", designcraft_doc::story::COLUMN_BREAK);
    let (mut d, sid, fid) = doc_with(&text, Rect::new(0.0, 0.0, 400.0, 300.0), ParaAttrs::default());
    d.item_mut(fid).unwrap().text_frame_mut().unwrap().options.columns = 2;
    let cs = compose_story(&d, sid, &ComposeOptions::default());
    let lines = all_lines(&cs);
    assert_eq!(lines.len(), 2, "{}", lines.len());
    assert_eq!(lines[0].column, 0);
    assert_eq!(lines[1].column, 1);
    assert!(lines[1].baseline < 20.0, "second column starts at the top");
}

/// InDesign: a break character ends its paragraph; the next paragraph starts on the next page (odd
/// or even for those breaks), with no empty line where the break was.
#[test]
fn break_characters_end_their_paragraph_and_keep_page_parity() {
    use designcraft_doc::story::{EVEN_PAGE_BREAK, ODD_PAGE_BREAK, PAGE_BREAK};
    let mut d = Document::new(&NewDocument { pages: 4, facing_pages: false, primary_text_frame: true, ..Default::default() });
    let sid = d.settings.primary_story.unwrap();
    let text = format!("one{ODD_PAGE_BREAK}\ntwo{EVEN_PAGE_BREAK}\nthree{PAGE_BREAK}\nfour");
    d.story_mut(sid).unwrap().insert(0, &text);
    let cs = compose_story(&d, sid, &ComposeOptions::default());
    // Frame i is on page i + 1: "one" on 1, "two" on the next odd page (3), "three" on the next
    // even page (4), and "four" has no page left.
    let lines: Vec<(usize, usize)> = cs.frames.iter().enumerate().flat_map(|(fi, f)| f.lines.iter().map(move |l| (fi, l.para))).collect();
    assert_eq!(lines, vec![(0, 0), (2, 1), (3, 2)]);
    assert_eq!(cs.overset_at, Some(text.find("four").unwrap()));
}

/// A paragraph that starts at the top of a frame isn't pushed on by Next Page, but Next Odd Page
/// (or Next Even Page) still moves it when that frame is on a page of the other parity.
#[test]
fn odd_and_even_page_starts_keep_parity_at_the_top_of_a_frame() {
    use designcraft_doc::StartParagraph;
    use designcraft_doc::story::PAGE_BREAK;
    let pages_of = |start: StartParagraph| {
        let mut d = Document::new(&NewDocument { pages: 4, facing_pages: false, primary_text_frame: true, ..Default::default() });
        let sid = d.settings.primary_story.unwrap();
        // "two" begins at the top of page 2, after the page break that ends "one".
        d.story_mut(sid).unwrap().insert(0, &format!("one{PAGE_BREAK}\ntwo\nthree"));
        d.story_mut(sid).unwrap().paras[1].para.start_paragraph = Some(start);
        let cs = compose_story(&d, sid, &ComposeOptions::default());
        // Frame i is on page i + 1.
        cs.frames.iter().enumerate().flat_map(|(fi, f)| f.lines.iter().map(move |l| (l.para, fi + 1))).collect::<Vec<_>>()
    };
    // Already at the top of an even page: Next Page and Next Even Page leave "two" where it is.
    assert_eq!(pages_of(StartParagraph::NextPage), vec![(0, 1), (1, 2), (2, 2)]);
    assert_eq!(pages_of(StartParagraph::NextEvenPage), vec![(0, 1), (1, 2), (2, 2)]);
    // Next Odd Page takes it to page 3, leaving page 2 empty.
    assert_eq!(pages_of(StartParagraph::NextOddPage), vec![(0, 1), (1, 3), (2, 3)]);
}

#[test]
fn line_before_a_break_character_is_a_last_line() {
    // As in InDesign: column, frame and page breaks end the paragraph's last line (set with the
    // last-line alignment), while a forced line break inside a justified paragraph is justified.
    use designcraft_doc::story::{COLUMN_BREAK, FORCED_LINE_BREAK, FRAME_BREAK, PAGE_BREAK};
    for (brk, align, justified) in [
        (COLUMN_BREAK, Align::LeftJustified, false),
        (FRAME_BREAK, Align::LeftJustified, false),
        (PAGE_BREAK, Align::LeftJustified, false),
        (PAGE_BREAK, Align::FullyJustified, true),
        (FORCED_LINE_BREAK, Align::LeftJustified, true),
    ] {
        // A space after the break is skipped and does not keep the break from moving the text.
        let text = format!("one two three{brk} four five six");
        let (mut d, sid, fid) = doc_with(&text, Rect::new(0.0, 0.0, 400.0, 300.0), ParaAttrs { align: Some(align), ..Default::default() });
        d.item_mut(fid).unwrap().text_frame_mut().unwrap().options.columns = 2;
        let cs = compose_story(&d, sid, &ComposeOptions::default());
        let l = &cs.frames[0].lines[0];
        let full = (l.end_x - l.x1).abs() < 0.6;
        assert_eq!(full, justified, "{brk:?} {align:?}: line ends at {} of {}", l.end_x, l.x1);
        if brk == COLUMN_BREAK {
            assert_eq!(all_lines(&cs)[1].column, 1, "the text after a column break starts the next column");
        }
    }
}

#[test]
fn tabs_without_stops_use_default_half_inch() {
    let (d, sid, _) = doc_with("A\tB", Rect::new(0.0, 0.0, 300.0, 100.0), ParaAttrs::default());
    let cs = compose_story(&d, sid, &ComposeOptions::default());
    let l = &cs.frames[0].lines[0];
    let b = l.glyphs.iter().find(|g| g.byte == 2).expect("B");
    assert!((b.x - 36.0).abs() < 0.5, "B at {}", b.x);
}

// ---------- tables ----------

fn table_doc(frame: Rect, t: designcraft_doc::Table) -> (Document, StoryId, ItemId) {
    let (mut d, sid, fid) = doc_with("Intro", frame, ParaAttrs::default());
    let st = d.story_mut(sid).unwrap();
    let end = st.len();
    st.insert_table(end, t);
    let end = st.len();
    st.insert(end, "After");
    d.check().unwrap();
    (d, sid, fid)
}

fn filled(rows: usize, cols: usize, header: usize, width: f64) -> designcraft_doc::Table {
    let mut t = designcraft_doc::Table::new(77, rows, cols, header, 0, width);
    for r in 0..t.nrows() {
        for c in 0..t.ncols() {
            t.cell_mut(r, c).unwrap().text.insert(0, &format!("R{r}C{c}"));
        }
    }
    t
}

#[test]
fn table_rows_fit_their_content() {
    let mut t = filled(3, 3, 1, 300.0);
    t.cell_mut(2, 1).unwrap().text.insert(0, &format!("{LOREM} "));
    t.rows[3].height = 50.0;
    t.rows[3].mode = designcraft_doc::RowHeightMode::Exactly;
    let (d, sid, _) = table_doc(Rect::new(0.0, 0.0, 400.0, 1000.0), t);
    let cs = compose_story(&d, sid, &ComposeOptions::default());
    assert!(!cs.is_overset());
    let ft = &cs.frames[0];
    assert_eq!(ft.tables.len(), 1);
    let tf = &ft.tables[0];
    assert_eq!(tf.cells.len(), 12);
    let h = |r: usize| tf.cell(r, 0).unwrap().rect.height();
    // One line of 12 pt text + 4 + 4 insets.
    assert!(h(0) > 12.0 && h(0) < 25.0, "{}", h(0));
    // The long cell makes its row taller; all cells in a row share the height.
    assert!(h(2) > 5.0 * h(0), "{} vs {}", h(2), h(0));
    assert!((tf.cell(2, 2).unwrap().rect.height() - h(2)).abs() < 1e-9);
    assert!((h(3) - 50.0).abs() < 1e-9);
    // Cell text sits inside the cell insets.
    let c = tf.cell(1, 1).unwrap();
    let l = &c.text.frames[0].lines[0];
    assert!(c.origin.x + l.x0 >= c.rect.x0 + 3.99);
    assert!(c.origin.y + l.baseline < c.rect.y1);
    // Text after the table is below it.
    let len = d.story(sid).unwrap().len();
    let after = ft.lines.iter().find(|l| l.range.start == len - 5).unwrap();
    assert!(after.baseline > tf.rect.y1);
    // Strokes: outer border + inner edges.
    assert!(tf.strokes.len() >= 4 * 3 + 4);
}

#[test]
fn table_columns_scale_to_the_text_column() {
    let (d, sid, _) = table_doc(Rect::new(0.0, 0.0, 200.0, 1000.0), filled(2, 4, 0, 600.0));
    let cs = compose_story(&d, sid, &ComposeOptions::default());
    let tf = &cs.frames[0].tables[0];
    assert!((tf.rect.width() - 200.0).abs() < 1e-6, "{}", tf.rect.width());
    assert!((tf.cell(0, 3).unwrap().rect.x1 - 200.0).abs() < 1e-6);
    // A narrow table keeps its widths.
    let (d, sid, _) = table_doc(Rect::new(0.0, 0.0, 400.0, 1000.0), filled(2, 2, 0, 100.0));
    let cs = compose_story(&d, sid, &ComposeOptions::default());
    assert!((cs.frames[0].tables[0].rect.width() - 100.0).abs() < 1e-6);
}

#[test]
fn table_rows_break_across_frames_with_header_repeat() {
    let mut d = Document::new(&NewDocument::default());
    let lid = d.default_layer();
    let (f1, sid) = d.add_text_frame(SpreadRef::Doc(0), Rect::new(0.0, 0.0, 300.0, 150.0), lid, "", ParaFormat::default()).unwrap();
    let (f2, _) = d.add_text_frame(SpreadRef::Doc(0), Rect::new(0.0, 200.0, 300.0, 350.0), lid, "", ParaFormat::default()).unwrap();
    d.thread(f1, f2).unwrap();
    d.story_mut(sid).unwrap().insert_table(0, filled(9, 2, 1, 300.0));
    d.check().unwrap();
    let cs = compose_story(&d, sid, &ComposeOptions::default());
    assert!(!cs.is_overset(), "overset at {:?}", cs.overset_at);
    let (a, b) = (&cs.frames[0].tables, &cs.frames[1].tables);
    assert_eq!((a.len(), b.len()), (1, 1));
    // Every body row appears exactly once; the header is repeated in the second frame.
    let body: Vec<usize> = a[0].cells.iter().chain(b[0].cells.iter()).filter(|c| c.row > 0 && c.col == 0).map(|c| c.row).collect();
    assert_eq!(body, (1..10).collect::<Vec<_>>());
    assert!(!a[0].cell(0, 0).unwrap().repeated);
    assert!(b[0].cell(0, 0).unwrap().repeated);
    assert!((b[0].rect.y0 - 200.0).abs() < 1e-6, "{}", b[0].rect.y0);
    assert!(a[0].rect.y1 <= 150.0 + 1e-6);
    assert!(a[0].first && !a[0].last && b[0].last);
    // Without header repeat the second fragment starts with a body row.
    let st = d.story_mut(sid).unwrap();
    st.table_mut(77).unwrap().options.repeat_header = false;
    let cs = compose_story(&d, sid, &ComposeOptions::default());
    assert!(cs.frames[1].tables[0].cell(0, 0).is_none());
    // Too little room: overset.
    let st = d.story_mut(sid).unwrap();
    let t = st.table_mut(77).unwrap();
    t.insert_rows(5, 30);
    let cs = compose_story(&d, sid, &ComposeOptions::default());
    assert!(cs.is_overset());
}

#[test]
fn table_cells_hit_test_and_place_carets() {
    let (d, sid, _) = table_doc(Rect::new(0.0, 0.0, 300.0, 1000.0), filled(2, 3, 0, 300.0));
    let cs = compose_story(&d, sid, &ComposeOptions::default());
    let c = cs.frames[0].tables[0].cell(1, 2).unwrap().clone();
    let p = Point::new(c.rect.x1 - 2.0, c.rect.center().y);
    let (t, r, col, b) = hit_cell(&cs, 0, p).unwrap();
    assert_eq!((t, r, col), (77, 1, 2));
    assert_eq!(b, "R1C2".len());
    let (fi, x, bl, _, _) = cell_caret(&cs, 77, 1, 2, 0).unwrap();
    assert_eq!(fi, 0);
    assert!(c.rect.contains(Point::new(x + 0.1, bl - 1.0)));
    // Outside every cell: no cell.
    assert!(hit_cell(&cs, 0, Point::new(1.0, 1.0)).is_none());
    // The anchor has a caret position (after the table).
    let a = d.story(sid).unwrap().table_anchor(77).unwrap();
    assert!(caret(&cs, a).is_some());
}

#[test]
fn table_fills_and_merged_cells() {
    let mut t = filled(4, 3, 1, 300.0);
    t.options.alt_rows = Some(designcraft_doc::AltFills { first: 1, next: 1, ..Default::default() });
    t.cell_mut(0, 0).unwrap().fill = "[Black]".into();
    t.merge(designcraft_doc::CellRange::new(1, 1, 2, 2)).unwrap();
    let (d, sid, _) = table_doc(Rect::new(0.0, 0.0, 300.0, 1000.0), t);
    let cs = compose_story(&d, sid, &ComposeOptions::default());
    let tf = &cs.frames[0].tables[0];
    assert!(tf.cell(0, 0).unwrap().fill.is_some());
    assert!(tf.cell(0, 1).unwrap().fill.is_none(), "header rows don't alternate");
    assert!(tf.cell(1, 0).unwrap().fill.is_some());
    assert!(tf.cell(2, 0).unwrap().fill.is_none());
    let m = tf.cell(1, 1).unwrap();
    let both = tf.cell(1, 0).unwrap().rect.height() + tf.cell(2, 0).unwrap().rect.height();
    assert!((m.rect.height() - both).abs() < 1e-6);
    assert!(tf.cell(2, 2).is_none());
    assert_eq!(tf.cells.len(), 15 - 3);
}

#[test]
fn tab_leaders_fill_the_gap() {
    let mut doc = designcraft_doc::Document::new(&designcraft_doc::build::NewDocument::default());
    let lid = doc.default_layer();
    let pf = designcraft_doc::ParaFormat {
        para: designcraft_doc::ParaAttrs {
            tabs: Some(vec![designcraft_doc::TabStop { position: 200.0, align: TabAlign::Right, leader: ".".into(), align_on: String::new() }]),
            ..Default::default()
        },
        ..Default::default()
    };
    let (_, sid) =
        doc.add_text_frame(designcraft_doc::SpreadRef::Doc(0), designcraft_geom::Rect::new(0.0, 0.0, 300.0, 100.0), lid, "Intro\t12", pf).unwrap();
    let cs = crate::compose_story(&doc, sid, &Default::default());
    let line = &cs.frames[0].lines[0];
    let dots: Vec<&crate::PlacedGlyph> = line.glyphs.iter().filter(|g| g.len == 0 && g.visible).collect();
    assert!(dots.len() > 20, "{} leader glyphs", dots.len());
    let last_dot = dots.iter().map(|g| g.x).fold(0.0, f64::max);
    let num_x = line.glyphs.iter().find(|g| g.byte == 6).unwrap().x;
    assert!(last_dot < num_x, "leaders stop before the page number");
}

#[test]
fn right_indent_tab_leaders_follow_stop_precedence_without_moving_text() {
    let text = "Intro\u{e006}12";
    let cases = [
        ("last interior", 0.0, vec![(200.0, "."), (80.0, "-")], Some('.')),
        ("first exterior", 0.0, vec![(380.0, "+"), (200.0, "-"), (320.0, ".")], Some('.')),
        ("at the margin", 0.0, vec![(300.0, "."), (80.0, "-")], Some('.')),
        ("no stops", 0.0, vec![], None),
        ("empty exterior", 0.0, vec![(200.0, "."), (320.0, ""), (350.0, "+")], None),
        ("empty interior", 0.0, vec![(80.0, "."), (200.0, "")], None),
        ("indented margin", 36.0, vec![(260.0, "-"), (280.0, "."), (340.0, "+")], Some('.')),
    ];
    for (name, right_indent, stops, expected) in cases {
        let tabs = stops
            .into_iter()
            .map(|(position, leader)| designcraft_doc::TabStop { position, align: TabAlign::Right, leader: leader.into(), align_on: String::new() })
            .collect();
        let para = ParaAttrs { tabs: Some(tabs), left_indent: Some(12.0), right_indent: Some(right_indent), ..Default::default() };
        // Translation and left/right indents distinguish the tab origin from line/absolute x.
        let (mut d, sid, _) = doc_with(text, Rect::new(72.0, 36.0, 372.0, 136.0), para);
        let cs = compose_story(&d, sid, &Default::default());
        assert!(!cs.is_overset(), "{name}");
        assert_eq!(cs.frames[0].lines.len(), 1, "{name}");
        let line = &cs.frames[0].lines[0];
        let tab = line.glyphs.iter().find(|g| g.byte == 5 && g.len > 0).unwrap();
        let leaders: Vec<_> = line.glyphs.iter().filter(|g| g.visible && g.len == 0).collect();
        assert_eq!(!leaders.is_empty(), expected.is_some(), "{name}");
        for g in leaders {
            assert_eq!(g.gid, g.face.get().glyph_for(expected.unwrap()), "{name}");
            let width = g.face.get().advance(g.gid) * g.sx;
            assert!(g.x >= tab.x - 0.01 && g.x + width <= tab.x + tab.adv + 0.01, "{name}: leader outside tab gap");
            assert_eq!(g.byte, tab.byte, "{name}");
        }
        assert!((line.end_x - (372.0 - right_indent)).abs() < 0.01, "{name}: number stays at the right margin");
        // Removing only the leaders must leave every source glyph and the line itself unchanged.
        for t in d.story_mut(sid).unwrap().paras[0].para.tabs.as_mut().unwrap() {
            t.leader.clear();
        }
        let plain = compose_story(&d, sid, &Default::default());
        assert_eq!(plain.frames[0].lines.len(), 1, "{name}");
        let plain_line = &plain.frames[0].lines[0];
        assert_eq!((&line.range, line.baseline, line.end_x), (&plain_line.range, plain_line.baseline, plain_line.end_x), "{name}");
        let source_positions = |l: &Line| {
            l.glyphs.iter().filter(|g| g.len > 0).map(|g| (g.byte, g.len, g.gid, g.x, g.y, g.adv, g.sx, g.sy, g.style)).collect::<Vec<_>>()
        };
        assert_eq!(source_positions(line), source_positions(plain_line), "{name}");
        assert_eq!(d.stories[&sid].text, text, "{name}: source text is not expanded");
    }
}

#[test]
fn right_indent_tab_leaders_use_the_tab_format_and_regular_tab_drawing() {
    let mut rendered = Vec::new();
    for ch in [story::RIGHT_INDENT_TAB, '\t'] {
        let text = format!("Intro{ch}12");
        let para = ParaAttrs {
            tabs: Some(vec![designcraft_doc::TabStop { position: 300.0, align: TabAlign::Right, leader: ".-".into(), align_on: String::new() }]),
            ..Default::default()
        };
        let (mut d, sid, _) = doc_with(&text, Rect::new(72.0, 36.0, 372.0, 136.0), para);
        d.story_mut(sid).unwrap().format_chars(5..5 + ch.len_utf8(), |f| {
            f.over.font_family = Some("Inter".into());
            f.over.size = Some(18.0);
            f.over.h_scale = Some(1.3);
            f.over.v_scale = Some(0.8);
            f.over.baseline_shift = Some(4.0);
            f.over.fill_tint = Some(0.4);
        });
        let cs = compose_story(&d, sid, &Default::default());
        assert!(!cs.is_overset());
        assert_eq!(cs.frames[0].lines.len(), 1);
        let line = &cs.frames[0].lines[0];
        let tab = line.glyphs.iter().find(|g| g.byte == 5 && g.len > 0).unwrap();
        let prefix = &line.glyphs[0];
        assert_ne!(tab.face.id(), prefix.face.id(), "tab uses its own font");
        assert_ne!(tab.style, prefix.style, "tab uses its own character formatting");
        assert_eq!(cs.styles[tab.style as usize].size, 18.0);
        assert_eq!(cs.styles[tab.style as usize].fill_tint, 0.4);
        let leaders: Vec<_> = line.glyphs.iter().filter(|g| g.visible && g.len == 0).collect();
        assert!(leaders.len() > 10);
        assert_eq!(leaders.len() % 2, 0, "only complete patterns are drawn");
        for (i, g) in leaders.iter().enumerate() {
            assert_eq!(g.gid, tab.face.get().glyph_for(if i % 2 == 0 { '.' } else { '-' }));
            assert_eq!(g.face.id(), tab.face.id());
            assert_eq!((g.sx, g.sy, g.y, g.style, g.byte), (tab.sx, tab.sy, -4.0, tab.style, tab.byte));
            let width = g.face.get().advance(g.gid) * g.sx;
            assert!(g.x >= tab.x - 0.01 && g.x + width <= tab.x + tab.adv + 0.01);
        }
        rendered.push(leaders.iter().map(|g| (g.gid, g.x, g.y, g.sx, g.sy, g.style)).collect::<Vec<_>>());
    }
    assert_eq!(rendered[0], rendered[1], "right-indent and equivalent regular right tabs use the same leader drawing");
}

#[test]
fn right_indent_tab_leaders_do_not_draw_when_the_gap_cannot_fit_a_pattern() {
    let text = "A\u{e006}B";
    let (d, sid, _) = doc_with(text, Rect::new(0.0, 0.0, 300.0, 100.0), ParaAttrs::default());
    let cs = compose_story(&d, sid, &Default::default());
    let line = &cs.frames[0].lines[0];
    let natural: f64 = line.glyphs.iter().filter(|g| g.visible).map(|g| g.adv).sum();
    let first = &line.glyphs[0];
    let pattern_width = first.face.get().advance(first.face.get().glyph_for('-')) * first.sx;
    assert!(pattern_width > 0.01);
    for gap in [0.0, pattern_width / 2.0] {
        let para = ParaAttrs {
            tabs: Some(vec![designcraft_doc::TabStop { position: 1.0, align: TabAlign::Left, leader: "-".into(), align_on: String::new() }]),
            ..Default::default()
        };
        let (d, sid, _) = doc_with(text, Rect::new(0.0, 0.0, natural + gap, 100.0), para);
        let cs = compose_story(&d, sid, &Default::default());
        assert!(!cs.is_overset());
        assert_eq!(cs.frames[0].lines.len(), 1);
        let line = &cs.frames[0].lines[0];
        let tab = line.glyphs.iter().find(|g| g.byte == 1 && g.len > 0).unwrap();
        assert!((tab.adv - gap).abs() < 1e-6);
        assert!(line.glyphs.iter().all(|g| g.len > 0 || !g.visible), "a partial leader pattern must not overlap adjacent text");
    }
}

#[test]
fn footnotes_sit_at_the_column_bottom_and_push_text() {
    // A short frame: without footnotes the text fills it; with two footnotes the body text
    // makes room and the footnotes stack against the bottom with a rule above.
    let text = format!("{LOREM} {LOREM} {LOREM}");
    let (mut d, sid, fid) = doc_with(&text, Rect::new(36.0, 36.0, 300.0, 236.0), ParaAttrs::default());
    let plain = compose_story(&d, sid, &ComposeOptions::default());
    let plain_lines = plain.frames[0].lines.len();
    {
        let st = d.story_mut(sid).unwrap();
        st.insert_note(10, "First note, long enough to take two lines in this narrow frame for sure.", ParaFormat::default());
        st.insert_note(40, "Second note.", ParaFormat::default());
        st.check().unwrap();
    }
    let cs = compose_story(&d, sid, &ComposeOptions::default());
    let ft = cs.frame(fid).unwrap();
    assert_eq!(ft.notes.len(), 2);
    assert!(ft.lines.len() < plain_lines, "body text makes room: {} vs {plain_lines}", ft.lines.len());
    let (a, b) = (&ft.notes[0], &ft.notes[1]);
    assert_eq!(a.label, "1");
    assert_eq!(b.label, "2");
    assert!(a.rect.y1 <= b.rect.y0 + 1e-6, "stacked in order");
    let area = ft.columns[0];
    assert!((b.rect.y1 - area.y1).abs() < 1e-6, "last footnote ends at the column bottom");
    let last = ft.lines.last().unwrap();
    assert!(last.baseline + last.descent <= a.rect.y0 - d.footnote_options.space_before + 0.01, "text clears the footnotes");
    assert!(a.text.frames[0].lines.len() >= 2);
    // The rule sits above the first footnote.
    assert!(ft.decos.iter().any(|dc| (dc.rect.width() - 72.0).abs() < 1e-6 && dc.rect.y1 <= a.rect.y0 + 1e-6));
    // The reference shows its number as superscript glyphs in the text.
    let refs: Vec<&PlacedGlyph> = ft.lines.iter().flat_map(|l| l.glyphs.iter()).filter(|g| g.byte == 10 && g.len > 0).collect();
    assert_eq!(refs.len(), 1);
    // Numbering continues from options and restarts per page when asked.
    d.footnote_options.start_at = 5;
    let cs = compose_story(&d, sid, &ComposeOptions::default());
    assert_eq!(cs.frames[0].notes[1].label, "6");
    // Hit testing and carets reach footnote text.
    let n = &cs.frames[0].notes[1];
    let (id, _) = hit_note(&cs, 0, Point::new(n.rect.x0 + 40.0, n.rect.y0 + 5.0)).unwrap();
    assert_eq!(id, n.id);
    assert!(note_caret(&cs, n.id, 0).is_some());
}

#[test]
fn footnotes_stack_baseline_to_baseline() {
    // Single-line footnotes read as one text: consecutive baselines sit one leading (plus
    // Space Between Footnotes) apart, and only the last footnote's descent adds to the area.
    let text = format!("{LOREM} {LOREM} {LOREM} {LOREM}");
    for space_between in [0.0, 3.0] {
        let (mut d, sid, fid) = doc_with(&text, Rect::new(36.0, 36.0, 300.0, 236.0), ParaAttrs::default());
        d.footnote_options.space_between = space_between;
        d.footnote_options.space_before = 4.0;
        {
            let st = d.story_mut(sid).unwrap();
            for at in [60, 40, 20, 10] {
                st.insert_note(at, "Note.", ParaFormat::default());
            }
            st.check().unwrap();
        }
        let cs = compose_story(&d, sid, &ComposeOptions::default());
        let ft = cs.frame(fid).unwrap();
        assert_eq!(ft.notes.len(), 4);
        let first_line = |n: &PlacedNote| {
            let lines = &n.text.frames[0].lines;
            assert_eq!(lines.len(), 1);
            lines[0].clone()
        };
        let baselines: Vec<f64> = ft.notes.iter().map(|n| n.origin.y + first_line(n).baseline).collect();
        let step = first_line(&ft.notes[1]).leading + space_between;
        for w in baselines.windows(2) {
            assert!((w[1] - w[0] - step).abs() < 1e-6, "baselines {baselines:?}, step {step}");
        }
        for w in ft.notes.windows(2) {
            assert!(w[0].rect.y1 <= w[1].rect.y0 + 1e-6, "rects stack in order");
        }
        // Area: first-baseline offset + (n - 1) steps + the last descent, ending at the column bottom.
        let col = ft.columns[0];
        let top = ft.notes[0].rect.y0;
        let (l0, l3) = (first_line(&ft.notes[0]), first_line(&ft.notes[3]));
        assert!((col.y1 - top - (l0.baseline + 3.0 * step + l3.descent)).abs() < 1e-6);
        assert!((ft.notes[3].rect.y1 - col.y1).abs() < 1e-6);
        // The body text fills the room left above the footnotes and their space before.
        let room = top - d.footnote_options.space_before;
        let last = ft.lines.last().unwrap();
        assert!(last.baseline + last.descent <= room + 0.01, "text clears the footnotes");
        assert!(last.baseline + last.leading + last.descent > room, "one more line wouldn't fit");
    }
}

#[test]
fn footnote_first_line_wraps_at_the_column_edge_after_its_number() {
    // InDesign sets the footnote number and separator as part of the first line: that line
    // wraps at the same right edge as the others, whatever the separator. A tab separator
    // reaches the footnote style's tab stop.
    let tab_at = |position: f64| {
        let stop = designcraft_doc::TabStop { position, align: TabAlign::Left, leader: String::new(), align_on: String::new() };
        ParaAttrs { tabs: Some(vec![stop]), ..Default::default() }
    };
    let right_edge = |l: &Line, source: &str| {
        let space = |g: &&PlacedGlyph| g.len > 0 && source.get(g.byte..).and_then(|s| s.chars().next()).is_some_and(char::is_whitespace);
        l.glyphs.iter().filter(|g| g.visible && !space(g)).map(|g| g.x + g.adv).fold(0.0, f64::max)
    };
    let cases =
        [("\t", tab_at(100.0)), ("\t", ParaAttrs::default()), (" ", ParaAttrs::default()), (".\u{2003}\u{2003}\u{2003}", ParaAttrs::default())];
    for (sep, para) in cases {
        let (mut d, sid, _) = doc_with("Short text.", Rect::new(36.0, 36.0, 300.0, 400.0), ParaAttrs::default());
        d.footnote_options.separator = sep.into();
        d.footnote_options.start_at = 1234;
        d.story_mut(sid).unwrap().insert_note(5, &LOREM.repeat(2), ParaFormat { para, ..Default::default() });
        let cs = compose_story(&d, sid, &ComposeOptions::default());
        let n = &cs.frames[0].notes[0];
        let width = n.rect.width();
        let lines = &n.text.frames[0].lines;
        assert!(lines.len() >= 3, "{sep:?}: {} lines", lines.len());
        for (i, l) in lines.iter().enumerate() {
            let right = right_edge(l, &n.source);
            assert!(right <= width + 0.01, "{sep:?}: line {i} ends at {right}, past the column's {width}");
        }
        assert!(lines[0].glyphs.first().is_some_and(|g| g.len == 0 && g.x < 1.0), "{sep:?}: the number leads line 0");
    }
    // The same holds for a tab in body text.
    let text = format!("Term\t{LOREM}");
    let (d, sid, _) = doc_with(&text, Rect::new(36.0, 36.0, 300.0, 400.0), tab_at(100.0));
    let cs = compose_story(&d, sid, &ComposeOptions::default());
    let l = &cs.frames[0].lines[0];
    let edge = cs.frames[0].columns[0].x1;
    assert!(right_edge(l, &text) <= edge + 0.01, "body line 0 ends at {}, past {edge}", right_edge(l, &text));
}

#[test]
fn footnote_line_at_column_top_still_sets() {
    // A note taller than the frame can't push its reference line forever.
    let (mut d, sid, _) = doc_with("Short text.", Rect::new(0.0, 0.0, 200.0, 40.0), ParaAttrs::default());
    let long = LOREM.repeat(3);
    d.story_mut(sid).unwrap().insert_note(5, &long, ParaFormat::default());
    let cs = compose_story(&d, sid, &ComposeOptions::default());
    assert!(!cs.frames[0].lines.is_empty());
    assert_eq!(cs.frames[0].notes.len(), 1);
}

#[test]
fn footnotes_span_columns_across_the_frame_bottom() {
    // Span Footnotes Across Columns: the note area runs under every column of the frame and each
    // column's text ends above it, also the columns before the one holding the reference.
    let text = [LOREM; 8].join(" ");
    let note = "A long footnote, long enough to wrap if it is set one column wide rather than across the frame.";
    let compose = |at: usize, span: bool| {
        let (mut d, sid, fid) = doc_with(&text, Rect::new(36.0, 36.0, 436.0, 236.0), ParaAttrs::default());
        d.item_mut(fid).unwrap().text_frame_mut().unwrap().options.columns = 2;
        d.footnote_options.span_columns = span;
        d.story_mut(sid).unwrap().insert_note(at, note, ParaFormat::default());
        let space_before = d.footnote_options.space_before;
        (compose_story(&d, sid, &ComposeOptions::default()), space_before)
    };
    let plain = compose(10, false).0;
    // A reference halfway down the second column.
    let in_col1 = plain.frames[0].lines.iter().filter(|l| l.column == 1).nth(5).unwrap().range.start + 5;
    for (at, ref_col) in [(10, 0), (in_col1, 1)] {
        let (cs, space_before) = compose(at, true);
        let ft = &cs.frames[0];
        let (c0, c1) = (ft.columns[0], ft.columns[1]);
        assert_eq!(ft.notes.len(), 1);
        let n = &ft.notes[0];
        assert_eq!(n.column, ref_col, "ref at {at}");
        assert!((n.rect.x0 - c0.x0).abs() < 1e-6 && (n.rect.x1 - c1.x1).abs() < 1e-6, "ref at {at}: note {:?} spans the frame", n.rect);
        assert!((n.rect.y1 - c0.y1).abs() < 1e-6, "ref at {at}: note ends at the frame bottom");
        for col in 0..2 {
            let last = ft.lines.iter().rfind(|l| l.column == col).unwrap();
            assert!(last.baseline + last.descent <= n.rect.y0 - space_before + 0.01, "ref at {at}: column {col} text clears the note");
        }
        assert!(n.text.frames[0].lines.iter().all(|l| l.x1 - l.x0 > c0.width() + 1.0), "ref at {at}: note set at the frame width");
    }
    // Off: the note stays in the column holding its reference.
    let n = &compose(in_col1, false).0.frames[0].notes[0];
    assert_eq!(n.column, 1);
    assert!((n.rect.x0 - plain.frames[0].columns[1].x0).abs() < 1e-6 && (n.rect.x1 - plain.frames[0].columns[1].x1).abs() < 1e-6);
}

#[test]
fn hj_severity_shades() {
    use crate::hj_severity;
    assert_eq!(hj_severity(1.0, 0.8, 1.33), 0);
    assert_eq!(hj_severity(1.4, 0.8, 1.33), 1);
    assert_eq!(hj_severity(1.6, 0.8, 1.33), 2);
    assert_eq!(hj_severity(2.5, 0.8, 1.33), 3);
    assert_eq!(hj_severity(0.6, 0.8, 1.33), 3);
}

#[test]
fn right_to_left_runs_are_ordered_visually() {
    // Hebrew between Latin words: the Hebrew letters read right to left.
    let text = "abc \u{5D0}\u{5D1}\u{5D2} def";
    let (d, sid, _) = doc_with(text, Rect::new(36.0, 36.0, 500.0, 200.0), ParaAttrs::default());
    let cs = compose_story(&d, sid, &ComposeOptions::default());
    let l = all_lines(&cs)[0];
    let x_of = |byte: usize| l.glyphs.iter().find(|g| g.byte == byte).map(|g| g.x).unwrap();
    let (alef, bet, gimel) = (4, 6, 8);
    assert!(x_of(alef) > x_of(bet) && x_of(bet) > x_of(gimel), "Hebrew reversed");
    assert!(x_of(0) < x_of(gimel) && x_of(alef) < x_of(11), "Latin around it stays in place");
    // A right-to-left paragraph puts its first word on the right.
    let rtl = ParaAttrs { direction: Some(designcraft_doc::TextDirection::RightToLeft), ..Default::default() };
    let (d, sid, _) = doc_with("\u{5D0}\u{5D1} abc", Rect::new(36.0, 36.0, 500.0, 200.0), rtl);
    let cs = compose_story(&d, sid, &ComposeOptions::default());
    let l = all_lines(&cs)[0];
    let x_of = |byte: usize| l.glyphs.iter().find(|g| g.byte == byte).map(|g| g.x).unwrap();
    assert!(x_of(0) > x_of(5), "the Hebrew word (first in the text) is right of the Latin one");
    // Shaping keeps clusters in text order.
    let face = designcraft_fonts::FontDb::global().face(designcraft_fonts::DEFAULT_FAMILY, "Regular");
    let g = designcraft_fonts::shape(&face, "ab \u{5D0}\u{5D1}", &[], |c| c);
    assert!(g.windows(2).all(|w| w[0].cluster <= w[1].cluster), "{:?}", g.iter().map(|g| g.cluster).collect::<Vec<_>>());
}

#[test]
fn bidi_matches_the_reference_order() {
    use unicode_bidi::{BidiInfo, Level};
    let text = "English then \u{645}\u{631}\u{62D}\u{628}\u{627} \u{628}\u{627}\u{644}\u{639}\u{627}\u{644}\u{645} 123 and \u{5E9}\u{5DC}\u{5D5}\u{5DD} \u{5E2}\u{5D5}\u{5DC}\u{5DD} end.";
    for rtl in [false, true] {
        let dir = if rtl { designcraft_doc::TextDirection::RightToLeft } else { designcraft_doc::TextDirection::LeftToRight };
        let (d, sid, _) = doc_with(text, Rect::new(0.0, 0.0, 2000.0, 200.0), ParaAttrs { direction: Some(dir), ..Default::default() });
        let cs = compose_story(&d, sid, &ComposeOptions::default());
        let l = all_lines(&cs)[0];
        // Characters by glyph x (one glyph per character in this text).
        let mut gs: Vec<_> = l.glyphs.iter().filter(|g| g.len > 0).collect();
        gs.sort_by(|a, b| a.x.total_cmp(&b.x));
        let ours: String = gs.iter().map(|g| text[g.byte..].chars().next().unwrap()).collect();
        let info = BidiInfo::new(text, Some(if rtl { Level::rtl() } else { Level::ltr() }));
        let reference = info.reorder_line(&info.paragraphs[0], 0..text.len()).to_string();
        assert_eq!(ours, reference, "rtl={rtl}");
    }
}

#[test]
fn carets_in_right_to_left_text_follow_the_drawing() {
    let mut d = Document::new(&designcraft_doc::build::NewDocument::default());
    let lid = d.default_layer();
    // "abc" then an Arabic word, in a left-to-right paragraph.
    let text = "abc سلام";
    let (_, sid) = d.add_text_frame(SpreadRef::Doc(0), Rect::new(100.0, 100.0, 400.0, 200.0), lid, text, ParaFormat::default()).unwrap();
    let cs = compose_story(&d, sid, &ComposeOptions::default());
    let l = &cs.frames[0].lines[0];
    let ar = text.find('س').unwrap();
    assert!(l.glyphs.iter().filter(|g| g.len > 0 && g.byte >= ar).all(|g| g.rtl));
    assert!(l.glyphs.iter().filter(|g| g.byte < 3).all(|g| !g.rtl));
    let x = |p: usize| caret(&cs, p).unwrap().1;
    // The Arabic word starts at its right edge and runs left.
    let after = ar + 'س'.len_utf8();
    assert!(x(after) < x(ar), "{} {}", x(after), x(ar));
    assert!(x(text.len()) < x(ar));
    // Arrow keys move as drawn: the word's start is at its right edge, so Left steps into it.
    assert_eq!(visual_step(&cs, ar, true), Some(after));
    assert_eq!(visual_step(&cs, after, false), Some(ar));
    // Latin text has no right-to-left glyphs.
    let (_, s2) = d.add_text_frame(SpreadRef::Doc(0), Rect::new(100.0, 300.0, 400.0, 400.0), lid, "abc", ParaFormat::default()).unwrap();
    assert_eq!(visual_step(&compose_story(&d, s2, &ComposeOptions::default()), 1, true), None);
    // A click at the word's right edge puts the caret at its start.
    let hx = x(ar) - 0.1;
    assert_eq!(hit(&cs, 0, Point::new(hx, l.baseline)), Some(ar));
}

#[test]
fn kashidas_stretch_justified_arabic_before_spaces() {
    check_kashida_justification(FontDb::global(), false);
}

#[test]
fn kashidas_without_tatweel_fall_back_to_spaces() {
    // A private database excludes host fonts without mutating the global database
    // used by parallel tests. Bundled Latin/Japanese fonts have no tatweel.
    let db = FontDb::with_font_dirs(Vec::new());
    db.set_system_fallback(false);
    check_kashida_justification(&db, true);
}

fn check_kashida_justification(db: &FontDb, require_fallback: bool) {
    let mut d = Document::new(&designcraft_doc::build::NewDocument::default());
    let lid = d.default_layer();
    let text = "بسم الله الرحمن الرحيم الحمد لله رب العالمين الرحمن الرحيم مالك يوم الدين اياك نعبد واياك نستعين";
    let pf = ParaFormat {
        para: ParaAttrs { align: Some(Align::RightJustified), direction: Some(designcraft_doc::TextDirection::RightToLeft), ..Default::default() },
        ..Default::default()
    };
    let (_, sid) = d.add_text_frame(SpreadRef::Doc(0), Rect::new(100.0, 100.0, 260.0, 400.0), lid, text, pf).unwrap();
    // The Arabic is drawn from a fallback font when the database has one.
    d.settings.glyph_fallback = true;
    let composed = |d: &Document| compose_with_db(d, d.story(sid).unwrap(), &frame_specs(d, sid), &ComposeOptions::default(), db);
    let first = |d: &Document| composed(d).frames[0].lines[0].clone();
    let with = first(&d);
    assert!(composed(&d).frames[0].lines.len() > 1);
    let lacks_tatweel = with.glyphs.iter().all(|g| g.face.glyph_for('\u{0640}') == 0);
    if require_fallback {
        assert!(lacks_tatweel);
    }
    // Spaces keep (about) their natural width; the joins took the extra length.
    let space_w = |l: &Line| l.glyphs.iter().filter(|g| g.len > 0 && text[g.byte..].starts_with(' ')).map(|g| g.adv).fold(0.0, f64::max);
    d.story_mut(sid).unwrap().paras[0].para.kashidas = Some(false);
    let without = first(&d);
    if lacks_tatweel {
        // No legal elongation can be drawn: enabling kashidas must preserve
        // the same space justification as disabling them.
        assert_eq!(with.glyphs.len(), without.glyphs.len());
        for (a, b) in with.glyphs.iter().zip(&without.glyphs) {
            assert_eq!((a.byte, a.gid, a.x, a.adv), (b.byte, b.gid, b.x, b.adv));
        }
    } else {
        assert!(space_w(&with) < space_w(&without) - 0.5, "{} {}", space_w(&with), space_w(&without));
    }
    // Both fill the measure: the line's (visually last) glyph reaches the left edge either way.
    let left = |l: &Line| l.glyphs.iter().filter(|g| g.len > 0).map(|g| g.x).fold(f64::MAX, f64::min);
    assert!((left(&with) - left(&without)).abs() < 1.0, "{} {}", left(&with), left(&without));
    // Tatweels are drawn in the gaps when the font has one.
    if with.glyphs.iter().any(|g| g.len == 0 && g.gid != 0) {
        assert!(with.glyphs.len() > without.glyphs.len());
    }
}

#[test]
fn digits_option_draws_figures_in_another_script() {
    let mut d = Document::new(&designcraft_doc::build::NewDocument::default());
    let lid = d.default_layer();
    let (_, sid) = d.add_text_frame(SpreadRef::Doc(0), Rect::new(100.0, 100.0, 400.0, 200.0), lid, "No. 2024", ParaFormat::default()).unwrap();
    let plain = compose_story(&d, sid, &ComposeOptions::default()).frames[0].lines[0].glyphs.clone();
    d.story_mut(sid).unwrap().format_chars(0..8, |f| f.over.digits = Some(designcraft_doc::Digits::Hindi));
    let hindi = compose_story(&d, sid, &ComposeOptions::default()).frames[0].lines[0].glyphs.clone();
    // One glyph per digit, each still mapped to its own source character.
    let digits: Vec<_> = hindi.iter().filter(|g| g.byte >= 4 && g.len > 0).collect();
    assert_eq!(digits.iter().map(|g| (g.byte, g.len)).collect::<Vec<_>>(), [(4, 1), (5, 1), (6, 1), (7, 1)]);
    // The letters are untouched; the digits are other glyphs.
    assert_eq!(plain[0].gid, hindi[0].gid);
    let gid = |gs: &[PlacedGlyph], b: usize| gs.iter().find(|g| g.byte == b).map(|g| g.gid);
    assert_ne!(gid(&plain, 4), gid(&hindi, 4));
}

#[test]
fn page_numbers_take_the_digits_option() {
    use designcraft_fonts::testing::font_mapping;
    // Draws ١ as X, every other character as itself.
    let glyphs = [('N', 'N'), ('o', 'o'), ('.', '.'), (' ', ' '), ('1', '1'), ('\u{661}', 'X')];
    designcraft_fonts::FontDb::global().add_font(font_mapping("DC Test Page Digits", &glyphs).unwrap());
    // The glyph drawn at byte 4: the page number, or a typed Arabic-Indic one.
    let fourth = |text: &str| {
        let (mut d, sid, _) = doc_with(text, Rect::new(100.0, 100.0, 400.0, 200.0), ParaAttrs::default());
        d.story_mut(sid).unwrap().format_chars(0..text.len(), |f| {
            f.over.font_family = Some("DC Test Page Digits".into());
            f.over.digits = Some(designcraft_doc::Digits::Hindi);
        });
        let cs = compose_story(&d, sid, &ComposeOptions::default());
        cs.frames[0].lines[0].glyphs.iter().find(|g| g.byte == 4).map(|g| g.gid).unwrap()
    };
    assert_eq!(fourth(&format!("No. {}", story::PAGE_NUMBER)), fourth("No. \u{661}"));
}

#[test]
fn digits_option_draws_page_numbers_and_list_numbers_in_that_script() {
    use designcraft_doc::{Digits, ListType};
    let gids = |text: &str, list: Option<ListType>, digits: Option<Digits>| {
        let (mut d, sid, _) = doc_with(text, Rect::new(0.0, 0.0, 300.0, 100.0), ParaAttrs { list_type: list, ..Default::default() });
        if let Some(dg) = digits {
            let n = text.len();
            d.story_mut(sid).unwrap().format_chars(0..n, |f| f.over.digits = Some(dg));
        }
        let cs = compose_story(&d, sid, &ComposeOptions::default());
        all_lines(&cs)[0].glyphs.iter().map(|g| g.gid).collect::<Vec<_>>()
    };
    let pn = story::PAGE_NUMBER.to_string();
    assert_ne!(gids(&pn, None, None), gids(&pn, None, Some(Digits::Farsi)), "page number marker");
    // Digits set on the text (not on the paragraph style) reach the list number too.
    assert_ne!(gids("Item", Some(ListType::Numbers), None), gids("Item", Some(ListType::Numbers), Some(Digits::Farsi)), "list number");
}

#[test]
fn ruby_and_kenten_sit_over_their_text() {
    let mut d = Document::new(&designcraft_doc::build::NewDocument::default());
    let lid = d.default_layer();
    let (_, sid) = d.add_text_frame(SpreadRef::Doc(0), Rect::new(100.0, 100.0, 400.0, 200.0), lid, "漢字です", ParaFormat::default()).unwrap();
    let base_n = compose_story(&d, sid, &ComposeOptions::default()).frames[0].lines[0].glyphs.len();
    let kanji = 0.."漢字".len();
    d.story_mut(sid).unwrap().format_chars(kanji.clone(), |f| f.over.ruby = Some("かんじ".into()));
    let cs = compose_story(&d, sid, &ComposeOptions::default());
    let l = &cs.frames[0].lines[0];
    let ruby: Vec<_> = l.glyphs[base_n..].iter().collect();
    assert_eq!(ruby.len(), 3, "one glyph per kana");
    let size = cs.styles[l.glyphs[0].style as usize].size;
    let (x0, x1) = (l.glyphs[0].x, l.glyphs[1].x + l.glyphs[1].adv);
    // Above the base, half size, within the base's width (shorter: spread 1-2-1).
    assert!(ruby.iter().all(|g| g.y < -size * 0.8 && g.len == 0 && (g.sx - l.glyphs[0].sx * 0.5).abs() < 1e-9));
    assert!(ruby[0].x > x0 && ruby[2].x + ruby[2].adv < x1 + 1e-6, "{x0} {x1} {:?}", ruby.iter().map(|g| g.x).collect::<Vec<_>>());
    // Caret positions ignore them.
    assert_eq!(caret(&cs, "漢字".len()).unwrap().1, l.glyphs[2].x);
    // Kenten: a dot over each character.
    d.story_mut(sid).unwrap().format_chars(kanji.end..kanji.end + "で".len(), |f| f.over.kenten = Some(true));
    let l = compose_story(&d, sid, &ComposeOptions::default()).frames[0].lines[0].clone();
    assert_eq!(l.glyphs.len(), base_n + 4);
    let dot = l.glyphs.last().unwrap();
    let de = &l.glyphs[2];
    assert!((dot.x + dot.adv / 2.0 - (de.x + de.adv / 2.0)).abs() < 1.0 && dot.y < 0.0);
}

/// `text` on one line, composed plain and then with the ruby attributes `set` on `range`: the
/// plain glyphs, and the ruby line (its ruby glyphs follow the text's) with its story.
fn ruby_compose(text: &str, range: std::ops::Range<usize>, set: impl Fn(&mut designcraft_doc::CharAttrs)) -> (Vec<PlacedGlyph>, Line, ComposedStory) {
    let mut d = Document::new(&designcraft_doc::build::NewDocument::default());
    let lid = d.default_layer();
    let (_, sid) = d.add_text_frame(SpreadRef::Doc(0), Rect::new(100.0, 100.0, 400.0, 200.0), lid, text, ParaFormat::default()).unwrap();
    let plain = compose_story(&d, sid, &ComposeOptions::default()).frames[0].lines[0].glyphs.clone();
    d.story_mut(sid).unwrap().format_chars(range, |f| set(&mut f.over));
    let cs = compose_story(&d, sid, &ComposeOptions::default());
    let line = cs.frames[0].lines[0].clone();
    (plain, line, cs)
}

#[test]
fn group_ruby_spreads_1_2_1_over_the_word_and_mono_ruby_sits_over_each_character() {
    let kanji = 0.."漢字".len();
    // Group: three kana over two kanji, half a gap at each end (JIS 1-2-1).
    let (plain, l, _) = ruby_compose("漢字です", kanji.clone(), |a| a.ruby = Some("かんじ".into()));
    let ruby = &l.glyphs[plain.len()..];
    assert_eq!(ruby.len(), 3);
    let (x0, x1) = (l.glyphs[0].x, l.glyphs[1].x + l.glyphs[1].adv);
    let w: f64 = ruby.iter().map(|g| g.adv).sum();
    assert!(w < x1 - x0);
    let gap = (x1 - x0 - w) / 3.0;
    let mut x = x0 + gap / 2.0;
    for g in ruby {
        assert!((g.x - x).abs() < 1e-6, "{} != {x}", g.x);
        x += g.adv + gap;
    }
    // Mono: かん over 漢 and じ centred on 字; the U+3000 separator isn't drawn.
    let (plain, l, _) = ruby_compose("漢字です", kanji, |a| {
        a.ruby = Some("かん\u{3000}じ".into());
        a.ruby_type = Some(designcraft_doc::ruby::RubyType::PerCharacter);
    });
    let ruby = &l.glyphs[plain.len()..];
    assert_eq!(ruby.len(), 3);
    let (kan, ji) = (&l.glyphs[0], &l.glyphs[1]);
    assert!((ruby[0].x - kan.x).abs() < 1e-6 && ruby[1].x + ruby[1].adv <= ji.x + 1e-6);
    assert!((ruby[2].x + ruby[2].adv / 2.0 - (ji.x + ji.adv / 2.0)).abs() < 1e-6);
}

#[test]
fn longer_ruby_overhangs_kana_and_spaces_out_kanji() {
    let ji = "漢".len().."漢字".len();
    // Over 字 before a kana: the ruby starts flush with 字 (never over the kanji before it) and
    // hangs over で by one ruby character, so nothing moves.
    let (plain, l, _) = ruby_compose("漢字で", ji.clone(), |a| a.ruby = Some("かんじ".into()));
    let ruby = &l.glyphs[plain.len()..];
    let w: f64 = ruby.iter().map(|g| g.adv).sum();
    let parent = &l.glyphs[1];
    assert!(w > parent.adv);
    assert!((ruby[0].x - parent.x).abs() < 1e-6, "{} != {}", ruby[0].x, parent.x);
    assert!((l.glyphs[2].x - plain[2].x).abs() < 1e-6, "で stays");
    assert!(ruby[2].x + ruby[2].adv > l.glyphs[2].x, "over で");
    // Between kanji there is nothing to overhang: 字 gets the difference as space on both sides
    // and the ruby spans it.
    let (plain, l, _) = ruby_compose("漢字漢", ji, |a| a.ruby = Some("かんじ".into()));
    let ruby = &l.glyphs[plain.len()..];
    let w: f64 = ruby.iter().map(|g| g.adv).sum();
    let grow = l.glyphs[2].x - plain[2].x;
    assert!((grow - (w - plain[1].adv)).abs() < 1e-6, "{grow} vs {w}");
    assert!((l.glyphs[1].dx - grow / 2.0).abs() < 1e-6);
    assert!((ruby[0].x - (l.glyphs[1].x - l.glyphs[1].dx)).abs() < 1e-6);
}

#[test]
fn ruby_is_half_size_by_default_and_can_sit_below_in_its_own_colour() {
    let kanji = 0.."漢字".len();
    let (plain, l, cs) = ruby_compose("漢字です", kanji.clone(), |a| {
        a.ruby = Some("かんじ".into());
        a.ruby_position = Some(designcraft_doc::ruby::RubyPosition::BelowLeft);
    });
    let ruby = &l.glyphs[plain.len()..];
    let size = cs.styles[l.glyphs[0].style as usize].size;
    assert!(ruby.iter().all(|g| g.y > size * 0.12), "below the parent's em box");
    assert!(ruby.iter().all(|g| (g.sy - l.glyphs[0].sy * 0.5).abs() < 1e-9), "half size");
    assert!((cs.styles[ruby[0].style as usize].size - size * 0.5).abs() < 1e-9);
    // Its own size and fill; the parent keeps its colour.
    let (plain, l, cs) = ruby_compose("漢字です", kanji, |a| {
        a.ruby = Some("かんじ".into());
        a.ruby_font_size = Some(Some(4.0));
        a.ruby_fill = Some("[Paper]".into());
    });
    let r = &l.glyphs[plain.len()];
    assert!(r.y < -size * 0.8);
    assert!((r.sy - l.glyphs[0].sy * 4.0 / size).abs() < 1e-9);
    assert_eq!(cs.styles[r.style as usize].fill, "[Paper]");
    assert_eq!(cs.styles[l.glyphs[0].style as usize].fill, "[Black]");
}

#[test]
fn ruby_moves_out_past_kenten_on_the_same_side() {
    use designcraft_doc::cjk_settings::KentenPosition;
    use designcraft_doc::ruby::RubyPosition;
    let kanji = 0.."漢字".len();
    // The ruby glyphs' baseline, and the outer edge of the kenten's em box (y down).
    let compose = |ruby: RubyPosition, kenten: Option<KentenPosition>| {
        let (plain, l, cs) = ruby_compose("漢字です", kanji.clone(), |a| {
            a.ruby = Some("かんじ".into());
            a.ruby_position = Some(ruby);
            if let Some(k) = kenten {
                a.kenten = Some(true);
                a.kenten_size = Some(Some(6.0));
                a.kenten_distance = Some(1.0);
                a.kenten_position = Some(k);
            }
        });
        let parent = &l.glyphs[0];
        let marks = cs.styles[parent.style as usize].kenten_mark.as_ref().map(|m| m.style);
        let ruby: Vec<PlacedGlyph> = l.glyphs[plain.len()..].iter().filter(|g| Some(g.style) != marks).cloned().collect();
        assert_eq!(ruby.len(), 3);
        let (top, bottom) = parent.face.em_box();
        let edge = match kenten {
            Some(KentenPosition::BelowLeft) => parent.y - bottom * parent.sy + 1.0 + 6.0,
            _ => parent.y - top * parent.sy - 1.0 - 6.0,
        };
        (ruby, edge)
    };
    for (position, kenten) in [(RubyPosition::AboveRight, KentenPosition::AboveRight), (RubyPosition::BelowLeft, KentenPosition::BelowLeft)] {
        let (alone, _) = compose(position, None);
        let (ruby, edge) = compose(position, Some(kenten));
        for (r, a) in ruby.iter().zip(&alone) {
            let (top, bottom) = r.face.em_box();
            if position == RubyPosition::AboveRight {
                assert!(r.y - bottom * r.sy <= edge + 1e-6, "the ruby's em box ends where the marks' starts");
                assert!(r.y < a.y, "moved up");
            } else {
                assert!(r.y - top * r.sy >= edge - 1e-6, "the ruby's em box starts where the marks' ends");
                assert!(r.y > a.y, "moved down");
            }
        }
    }
    // Kenten on the other side leave the ruby where it was.
    let (alone, _) = compose(RubyPosition::AboveRight, None);
    let (ruby, _) = compose(RubyPosition::AboveRight, Some(KentenPosition::BelowLeft));
    assert!(ruby.iter().zip(&alone).all(|(r, a)| (r.y - a.y).abs() < 1e-9));
}

#[test]
fn ruby_and_kenten_overprint_their_own_run_styles() {
    use designcraft_doc::cjk_settings::AdornmentOverprint as O;
    let kanji = 0.."漢字".len();
    let (plain, l, cs) = ruby_compose("漢字です", kanji, |a| {
        a.ruby = Some("かんじ".into());
        a.ruby_overprint_stroke = Some(O::On);
        a.kenten = Some(true);
        a.kenten_overprint_fill = Some(O::On);
    });
    let parent = &cs.styles[l.glyphs[0].style as usize];
    assert!(!parent.overprint_fill && !parent.overprint_stroke, "the text itself doesn't overprint");
    let marks = parent.kenten_mark.as_ref().unwrap().style;
    let mark = &cs.styles[marks as usize];
    assert!(mark.overprint_fill && !mark.overprint_stroke);
    let ruby = l.glyphs[plain.len()..].iter().find(|g| g.style != marks).unwrap();
    let ruby = &cs.styles[ruby.style as usize];
    assert!(!ruby.overprint_fill && ruby.overprint_stroke);
}

#[test]
fn longer_ruby_never_overhangs_a_line_edge_or_more_than_its_neighbours_take() {
    // Four 6 pt ruby characters over one 12 pt kanji: 12 pt longer, which the kana on each side
    // take (one ruby character, 6 pt, each). A line edge takes none, so the group stays on the
    // line of each kana it overhangs.
    let text = "かな字".repeat(12);
    let mut d = Document::new(&designcraft_doc::build::NewDocument::default());
    let lid = d.default_layer();
    let (_, sid) = d.add_text_frame(SpreadRef::Doc(0), Rect::new(100.0, 100.0, 190.0, 500.0), lid, &text, ParaFormat::default()).unwrap();
    for (k, _) in text.match_indices('字') {
        d.story_mut(sid).unwrap().format_chars(k..k + "字".len(), |f| f.over.ruby = Some("じじじじ".into()));
    }
    let cs = compose_story(&d, sid, &ComposeOptions::default());
    let lines = &cs.frames[0].lines;
    assert!(lines.len() > 2, "the text wraps");
    for l in lines {
        let parents: Vec<&PlacedGlyph> = l.glyphs.iter().filter(|g| g.len > 0 && g.visible).collect();
        let x0 = parents.iter().map(|g| g.x - g.dx).fold(f64::MAX, f64::min);
        let x1 = parents.iter().map(|g| g.x - g.dx + g.adv).fold(f64::MIN, f64::max);
        for p in parents.iter().filter(|g| text.get(g.byte..).is_some_and(|t| t.starts_with('字'))) {
            let ruby: Vec<&PlacedGlyph> = l.glyphs.iter().filter(|g| g.len == 0 && g.byte == p.byte).collect();
            assert_eq!(ruby.len(), 4);
            let r0 = ruby.iter().map(|g| g.x).fold(f64::MAX, f64::min);
            let r1 = ruby.iter().map(|g| g.x + g.adv).fold(f64::MIN, f64::max);
            assert!(r0 >= x0 - 1e-6 && r1 <= x1 + 1e-6, "inside the line: {r0}..{r1} in {x0}..{x1}");
            let (p0, p1) = (p.x - p.dx, p.x - p.dx + p.adv);
            assert!(p0 - r0 <= 6.0 + 1e-6 && r1 - p1 <= 6.0 + 1e-6, "at most one ruby character over each kana: {r0}..{r1} over {p0}..{p1}");
        }
    }
}

#[test]
fn warichu_stacks_the_run_inside_the_line_and_closes_up() {
    let text = "ABCDEFGHZ";
    let mut d = Document::new(&designcraft_doc::build::NewDocument::default());
    let lid = d.default_layer();
    let (_, sid) = d.add_text_frame(SpreadRef::Doc(0), Rect::new(36.0, 36.0, 500.0, 200.0), lid, text, ParaFormat::default()).unwrap();
    let plain = compose_story(&d, sid, &ComposeOptions::default()).frames[0].lines[0].glyphs.clone();
    let z0 = plain.iter().find(|g| g.byte == 8 && g.len > 0).unwrap().x;
    let sx0 = plain.iter().find(|g| g.byte == 0 && g.len > 0).unwrap().sx;
    d.story_mut(sid).unwrap().format_chars(0..8, |f| {
        f.over.warichu = Some(true);
        f.over.warichu_alignment = Some(designcraft_doc::cjk::WarichuAlignment::Left);
    });
    let cs = compose_story(&d, sid, &ComposeOptions::default());
    let l = &cs.frames[0].lines[0];
    let run: Vec<_> = l.glyphs.iter().filter(|g| g.byte < 8 && g.len > 0).collect();
    assert_eq!(run.len(), 8, "the note keeps one glyph per letter");
    let size = cs.styles[run[0].style as usize].size;
    assert!((run[0].sx - sx0 * 0.5).abs() < 1e-6, "half the parent size");
    let y_of = |slice: &[&PlacedGlyph]| slice.iter().map(|g| g.y).sum::<f64>() / slice.len() as f64;
    let (y_top, y_bot) = (y_of(&run[..4]), y_of(&run[4..]));
    assert!((y_bot - y_top - size * 0.5).abs() < 0.05 * size, "two lines one small em apart: {y_top} {y_bot} {size}");
    assert!((run[0].x - run[4].x).abs() < 1e-6, "left alignment shares the start");
    let z = l.glyphs.iter().find(|g| g.byte == 8 && g.len > 0).unwrap();
    let right = run.iter().map(|g| g.x + g.adv).fold(f64::MIN, f64::max);
    assert!((z.x - right).abs() < 1e-3, "the next character follows the note: {} {right}", z.x);
    assert!(z.x < z0 - 1.0, "the note is narrower than the full-size run: {} {z0}", z.x);
    assert!((l.end_x - (z.x + z.adv)).abs() < 1e-3, "the line-end caret follows the close-up");
}

#[test]
fn warichu_line_spacing_from_a_document_stays_finite() {
    for spacing in [1e308, -1e308, f64::INFINITY, f64::NAN] {
        let mut d = Document::new(&designcraft_doc::build::NewDocument::default());
        let lid = d.default_layer();
        let (_, sid) = d.add_text_frame(SpreadRef::Doc(0), Rect::new(36.0, 36.0, 500.0, 200.0), lid, "ABCDEFGHZ", ParaFormat::default()).unwrap();
        d.story_mut(sid).unwrap().format_chars(0..8, |f| {
            f.over.warichu = Some(true);
            f.over.warichu_lines = Some(16);
            f.over.warichu_line_spacing = Some(spacing);
        });
        let cs = compose_story(&d, sid, &ComposeOptions::default());
        let l = &cs.frames[0].lines[0];
        assert!(
            l.glyphs.iter().all(|g| g.x.is_finite() && g.y.is_finite() && g.y.abs() < 1e4),
            "{spacing}: {:?}",
            l.glyphs.iter().map(|g| g.y).collect::<Vec<_>>()
        );
    }
}

#[test]
fn warichu_break_minimum_keeps_a_short_run_on_one_line() {
    let mut d = Document::new(&designcraft_doc::build::NewDocument::default());
    let lid = d.default_layer();
    let (_, sid) = d.add_text_frame(SpreadRef::Doc(0), Rect::new(36.0, 36.0, 500.0, 200.0), lid, "ABCD", ParaFormat::default()).unwrap();
    d.story_mut(sid).unwrap().format_chars(0..4, |f| {
        f.over.warichu = Some(true);
        f.over.warichu_lines = Some(2);
        f.over.warichu_chars_before_break = Some(3);
        f.over.warichu_chars_after_break = Some(3);
    });
    let l = &compose_story(&d, sid, &ComposeOptions::default()).frames[0].lines[0];
    let run: Vec<_> = l.glyphs.iter().filter(|g| g.len > 0 && g.byte < 4).collect();
    assert_eq!(run.len(), 4);
    let y0 = run[0].y;
    assert!(run.iter().all(|g| (g.y - y0).abs() < 1e-6), "not enough characters to break");
}

#[test]
fn warichu_break_minimums_apply_to_the_first_and_last_line() {
    let mut d = Document::new(&designcraft_doc::build::NewDocument::default());
    let lid = d.default_layer();
    let (_, sid) = d.add_text_frame(SpreadRef::Doc(0), Rect::new(36.0, 36.0, 500.0, 200.0), lid, "ABCDEFGH", ParaFormat::default()).unwrap();
    d.story_mut(sid).unwrap().format_chars(0..8, |f| {
        f.over.warichu = Some(true);
        f.over.warichu_lines = Some(2);
        f.over.warichu_alignment = Some(designcraft_doc::cjk::WarichuAlignment::Left);
        f.over.warichu_chars_before_break = Some(1);
        f.over.warichu_chars_after_break = Some(5);
    });
    let l = &compose_story(&d, sid, &ComposeOptions::default()).frames[0].lines[0];
    let run: Vec<_> = l.glyphs.iter().filter(|g| g.len > 0 && g.byte < 8).collect();
    let y0 = run[0].y;
    let first = run.iter().filter(|g| (g.y - y0).abs() < 1e-6).count();
    let second = run.len() - first;
    assert_eq!(first + second, run.len());
    assert!(first >= 1 && second >= 5, "before 1 and after 5 on eight letters: {first} {second}");
}

#[test]
fn warichu_rows_balance_by_width() {
    let mut d = Document::new(&designcraft_doc::build::NewDocument::default());
    let lid = d.default_layer();
    let (_, sid) = d.add_text_frame(SpreadRef::Doc(0), Rect::new(36.0, 36.0, 500.0, 200.0), lid, "IIIIWWWW", ParaFormat::default()).unwrap();
    d.story_mut(sid).unwrap().format_chars(0..8, |f| {
        f.over.warichu = Some(true);
        f.over.warichu_lines = Some(2);
        f.over.warichu_alignment = Some(designcraft_doc::cjk::WarichuAlignment::Left);
    });
    let l = &compose_story(&d, sid, &ComposeOptions::default()).frames[0].lines[0];
    let run: Vec<_> = l.glyphs.iter().filter(|g| g.len > 0 && g.byte < 8).collect();
    let y0 = run[0].y;
    let width = |pred: bool| run.iter().filter(|g| ((g.y - y0).abs() < 1e-6) == pred).map(|g| g.adv).sum::<f64>();
    let (top, bot) = (width(true), width(false));
    let widest = run.iter().map(|g| g.adv).fold(0.0_f64, f64::max);
    assert!((top - bot).abs() <= widest + 0.05, "rows differ by at most one glyph: {top} {bot}");
}

#[test]
fn warichu_negative_spacing_tightens_the_rows() {
    let mut d = Document::new(&designcraft_doc::build::NewDocument::default());
    let lid = d.default_layer();
    let (_, sid) = d.add_text_frame(SpreadRef::Doc(0), Rect::new(36.0, 36.0, 500.0, 200.0), lid, "ABCDEFGH", ParaFormat::default()).unwrap();
    d.story_mut(sid).unwrap().format_chars(0..8, |f| {
        f.over.warichu = Some(true);
        f.over.warichu_alignment = Some(designcraft_doc::cjk::WarichuAlignment::Left);
        f.over.warichu_line_spacing = Some(-1.0);
    });
    let cs = compose_story(&d, sid, &ComposeOptions::default());
    let l = &cs.frames[0].lines[0];
    let run: Vec<_> = l.glyphs.iter().filter(|g| g.len > 0 && g.byte < 8).collect();
    let size = cs.styles[run[0].style as usize].size;
    let gap = run[4].y - run[0].y;
    assert!((gap - (size * 0.5 - 1.0)).abs() < 0.05, "one point tighter than a small em: {gap} {size}");
}

#[test]
fn warichu_tab_leaders_stay_in_the_gap() {
    let frame = |warichu: bool| {
        let mut doc = Document::new(&designcraft_doc::build::NewDocument::default());
        let lid = doc.default_layer();
        let pf = ParaFormat {
            para: ParaAttrs {
                tabs: Some(vec![designcraft_doc::TabStop {
                    position: 200.0,
                    align: designcraft_doc::TabAlign::Right,
                    leader: ".".into(),
                    align_on: String::new(),
                }]),
                ..Default::default()
            },
            ..Default::default()
        };
        let (_, sid) = doc.add_text_frame(SpreadRef::Doc(0), Rect::new(0.0, 0.0, 300.0, 100.0), lid, "Intro\tABCDEFGH", pf).unwrap();
        if warichu {
            let at = "Intro\t".len();
            doc.story_mut(sid).unwrap().format_chars(at..at + 8, |f| {
                f.over.warichu = Some(true);
                f.over.warichu_alignment = Some(designcraft_doc::cjk::WarichuAlignment::Left);
            });
        }
        let cs = compose_story(&doc, sid, &ComposeOptions::default());
        let line = cs.frames[0].lines[0].clone();
        let dots: Vec<f64> = line.glyphs.iter().filter(|g| g.len == 0 && g.visible).map(|g| g.x).collect();
        (dots, line)
    };
    let (plain, _) = frame(false);
    let (noted, line) = frame(true);
    assert!(plain.len() > 5 && plain.len() == noted.len(), "the leader is not rebuilt or dropped: {} {}", plain.len(), noted.len());
    for (a, b) in plain.iter().zip(&noted) {
        assert!((a - b).abs() < 1e-6, "a leader before the note stays put: {a} {b}");
    }
    let note_x = line.glyphs.iter().filter(|g| g.len > 0 && g.byte >= "Intro\t".len()).map(|g| g.x).fold(f64::INFINITY, f64::min);
    assert!(noted.iter().all(|x| *x < note_x), "leaders stay ahead of the note");
}

#[test]
fn warichu_hit_caret_and_selection_follow_each_row() {
    let text = "ABCDEFGHZ";
    let mut d = Document::new(&designcraft_doc::build::NewDocument::default());
    let lid = d.default_layer();
    let (_, sid) = d.add_text_frame(SpreadRef::Doc(0), Rect::new(36.0, 36.0, 500.0, 200.0), lid, text, ParaFormat::default()).unwrap();
    d.story_mut(sid).unwrap().format_chars(0..8, |f| {
        f.over.warichu = Some(true);
        f.over.warichu_alignment = Some(designcraft_doc::cjk::WarichuAlignment::Left);
    });
    let cs = compose_story(&d, sid, &ComposeOptions::default());
    let l = &cs.frames[0].lines[0];
    let at = |byte: usize| l.glyphs.iter().find(|g| g.byte == byte && g.len > 0).unwrap();
    let top = at(0);
    let bot = at(4);
    let z = at(8);
    let (_, _, y0, asc, _) = caret(&cs, 0).unwrap();
    let (_, _, y4, _, _) = caret(&cs, 4).unwrap();
    let (_, x_end, y_end, _, _) = caret(&cs, text.len()).unwrap();
    assert!((y0 - (l.baseline + top.y)).abs() < 1e-6 && (y4 - (l.baseline + bot.y)).abs() < 1e-6, "caret sits on the row");
    assert!(asc < l.ascent * 0.8, "the row caret is shorter than the parent line");
    assert!((y_end - l.baseline).abs() < 1e-6 && (x_end - l.end_x).abs() < 1e-3, "the line end stays on the parent baseline");
    let click = |g: &PlacedGlyph| hit(&cs, 0, designcraft_geom::Point::new(g.x + g.adv * 0.25, l.baseline + g.y)).unwrap();
    assert_eq!(click(top), 0);
    assert_eq!(click(bot), 4);
    assert_eq!(hit(&cs, 0, designcraft_geom::Point::new(z.x + z.adv * 0.25, l.baseline)).unwrap(), 8);
    let up = adjacent_row(l, bot.x + bot.adv * 0.25, l.baseline + bot.y, true).unwrap();
    assert!(hit(&cs, 0, designcraft_geom::Point::new(bot.x + bot.adv * 0.25, up)).unwrap() < 4);
    assert!(adjacent_row(l, top.x + 0.1, l.baseline + top.y, true).is_none(), "the top row does not move up inside the note");
    assert!(adjacent_row(l, z.x + 0.1, l.baseline, false).is_none(), "the character after the note is not inside a row");
    let quads = highlight_quads(l, 0, 8, false);
    assert_eq!(quads.len(), 2, "one band per row");
    let mid_y = |q: &[designcraft_geom::Point; 4]| (q[0].y + q[2].y) / 2.0;
    assert!(mid_y(&quads[0]) < l.baseline && mid_y(&quads[1]) > l.baseline, "the bands sit on either side of the parent baseline");
    let style = &cs.styles[top.style as usize];
    assert!((rule_baseline(style, l, top) - (l.baseline + top.y)).abs() < 1e-9);
    let z_style = &cs.styles[z.style as usize];
    assert!(!z_style.warichu && (rule_baseline(z_style, l, z) - l.baseline).abs() < 1e-9);
}

#[test]
fn tate_chu_yoko_sets_digits_across_one_em() {
    let mut d = Document::new(&designcraft_doc::build::NewDocument::default());
    let lid = d.default_layer();
    let (_, sid) = d.add_text_frame(SpreadRef::Doc(0), Rect::new(100.0, 100.0, 200.0, 400.0), lid, "令和12年", ParaFormat::default()).unwrap();
    let at = "令和".len();
    d.story_mut(sid).unwrap().format_chars(at..at + 2, |f| f.over.tate_chu_yoko = Some(true));
    let glyphs = |d: &Document| compose_story(d, sid, &ComposeOptions::default()).frames[0].lines[0].glyphs.clone();
    // Horizontal text ignores it.
    assert!(glyphs(&d).iter().all(|g| g.tcy.is_none()));
    d.story_mut(sid).unwrap().vertical = true;
    let gs = glyphs(&d);
    let digits: Vec<_> = gs.iter().filter(|g| g.tcy.is_some()).collect();
    assert_eq!(digits.len(), 2);
    let em = digits[0].tcy.unwrap()[2];
    // The pair takes one em along the line, so the next ideograph follows an em after it.
    assert!((digits.iter().map(|g| g.adv).sum::<f64>() - em).abs() < 1e-6);
    let nen = gs.iter().find(|g| g.byte == at + 2).unwrap();
    assert!((nen.x - (digits[0].x + em)).abs() < 1e-6, "{} {}", nen.x, digits[0].x);
    // Both turn about the same centre and sit side by side across it.
    let [a0, x0, _] = digits[0].tcy.unwrap();
    let [a1, x1, _] = digits[1].tcy.unwrap();
    assert!(((digits[0].x + a0) - (digits[1].x + a1)).abs() < 1e-6);
    assert!(x0 < 0.0 && x1 > x0 && x1 < em, "{x0} {x1}");
    // Offsets move the turned group: the Y offset along the line, the X offset across it.
    let origin = |d: &Document| {
        let cs = compose_story(d, sid, &ComposeOptions::default());
        let line = &cs.frames[0].lines[0];
        let g = line.glyphs.iter().find(|g| g.tcy.is_some()).unwrap();
        g.vertical_xf(line.baseline).unwrap() * Point::new(g.x, line.baseline + g.y)
    };
    let plain = origin(&d);
    d.story_mut(sid).unwrap().format_chars(at..at + 2, |f| {
        f.over.tate_chu_yoko_x_offset = Some(2.0);
        f.over.tate_chu_yoko_y_offset = Some(3.0);
    });
    let moved = origin(&d);
    assert!(((moved.x - plain.x) - 3.0).abs() < 1e-6 && ((moved.y - plain.y).abs() - 2.0).abs() < 1e-6, "{plain:?} {moved:?}");
}

#[test]
fn vertical_frames_compose_in_the_turned_box() {
    let mut d = Document::new(&designcraft_doc::build::NewDocument::default());
    let lid = d.default_layer();
    let (fid, sid) =
        d.add_text_frame(SpreadRef::Doc(0), Rect::new(100.0, 100.0, 200.0, 400.0), lid, "縦書きの文章です。Latin", ParaFormat::default()).unwrap();
    if let Some(tf) = d.item_mut(fid).and_then(|i| i.text_frame_mut()) {
        tf.options.inset = [0.0; 4];
    }
    d.story_mut(sid).unwrap().vertical = true;
    let cs = compose_story(&d, sid, &ComposeOptions::default());
    let ft = &cs.frames[0];
    assert!(ft.vertical);
    // The line runs along the frame's height (300 pt), so it all fits on one line.
    assert_eq!(ft.lines.len(), 1);
    let l = &ft.lines[0];
    assert!(l.x1 - l.x0 > 290.0, "{} {}", l.x0, l.x1);
    assert!(l.glyphs.iter().filter(|g| g.len > 0).take(5).all(|g| g.upright));
    assert!(!l.glyphs.iter().rev().find(|g| g.len > 0).unwrap().upright, "Latin turns");
    // Text space → frame: the first line sits at the right edge and runs down.
    let xf = d.text_xf(d.item(fid).unwrap());
    let p0 = xf * designcraft_geom::Point::new(l.glyphs[0].x, l.baseline);
    let p1 = xf * designcraft_geom::Point::new(l.glyphs[3].x, l.baseline);
    assert!(p0.x > 150.0 && p1.y > p0.y && (p1.x - p0.x).abs() < 1e-6, "{p0:?} {p1:?}");
}

#[test]
fn cjk_text_breaks_between_characters_with_kinsoku() {
    use crate::breaker::cjk_break_between;
    assert!(cjk_break_between('日', '本'));
    assert!(!cjk_break_between('す', '。'), "no line starts with a full stop");
    assert!(!cjk_break_between('「', '日'), "no line ends with an opening bracket");
    assert!(!cjk_break_between('a', 'b'));
    let text = "日本語の文章は単語の間に空白を入れずに書くので、文字と文字の間で改行します。";
    let (d, sid, _) = doc_with(text, Rect::new(36.0, 36.0, 156.0, 400.0), ParaAttrs::default());
    let cs = compose_story(&d, sid, &ComposeOptions::default());
    let lines = all_lines(&cs);
    assert!(lines.len() > 1, "wraps");
    for l in &lines {
        assert!(l.end_x <= l.x1 + 0.5, "line fits: {} > {}", l.end_x, l.x1);
        let first = text[l.range.clone()].chars().next().unwrap_or(' ');
        assert!(!"、。".contains(first), "kinsoku: {:?}", &text[l.range.clone()]);
    }
}

#[test]
fn only_english_text_gets_english_hyphenation() {
    let text = "internationalization internationalization internationalization internationalization";
    let narrow = Rect::new(36.0, 36.0, 120.0, 700.0);
    let hyphenated = |d: &Document, sid| all_lines(&compose_story(d, sid, &ComposeOptions::default())).iter().filter(|l| l.hyphenated).count();
    let (d, sid, _) = doc_with(text, narrow, ParaAttrs::default());
    assert!(hyphenated(&d, sid) > 0, "English hyphenates");
    let mut d2 = d.clone();
    let st = d2.story_mut(sid).unwrap();
    let n = st.len();
    st.format_chars(0..n, |f| f.over.language = Some("French".into()));
    assert_eq!(hyphenated(&d2, sid), 0, "French isn't hyphenated with English rules");
}

// A text frame's bottom inset constrains its last baseline. The full font descent
// still belongs to the line metrics, caret geometry and measured content height.
fn baseline_fit_doc(text: &str, height: f64) -> (Document, StoryId, ItemId) {
    let (mut d, sid, fid) = doc_with(text, Rect::new(0.0, 0.0, 200.0, height), ParaAttrs::default());
    let tf = d.item_mut(fid).unwrap().text_frame_mut().unwrap();
    tf.options.first_baseline = FirstBaseline::Fixed;
    tf.options.first_baseline_min = 10.0;
    d.story_mut(sid).unwrap().format_chars(0..text.len(), |f| {
        f.over.font_family = Some("Source Serif 4".into());
        f.over.size = Some(8.0);
        f.over.leading = Some(designcraft_doc::Leading::Points(11.0));
    });
    (d, sid, fid)
}

#[test]
fn horizontal_text_fits_its_baseline_at_the_bottom_inset() {
    // Include spaces, a nonprinting anchor and real descenders, rather than only capitals.
    let text = format!("{}Map gaps ", designcraft_doc::ANCHOR_MARK);
    let (d, sid, _) = baseline_fit_doc(&text, 10.0);
    let cs = compose_story(&d, sid, &ComposeOptions::default());
    assert!(!cs.is_overset());
    let line = &cs.frames[0].lines[0];
    assert_eq!(line.baseline, 10.0);
    assert!(line.descent > 0.0);
    assert!(cs.frames[0].content_height > 10.0, "measured content must retain descenders");
    assert!(line.glyphs.iter().any(|g| g.visible && text[g.byte..].starts_with('g')));
}

#[test]
fn bottom_baseline_fit_preserves_leading_and_paragraph_spacing() {
    let text = "Heading\nFirst line\u{2028}Second line\u{2028}Last line.";
    let (mut d, sid, fid) = baseline_fit_doc(text, 65.5);
    d.item_mut(fid).unwrap().text_frame_mut().unwrap().options.inset = [9.0; 4];
    d.story_mut(sid).unwrap().paras[0].para.space_after = Some(4.5);
    let cs = compose_story(&d, sid, &ComposeOptions::default());
    assert!(!cs.is_overset());
    assert_eq!(cs.frames[0].lines.iter().map(|l| l.baseline).collect::<Vec<_>>(), [19.0, 34.5, 45.5, 56.5]);
    assert_eq!(cs.frames[0].columns[0].y1, 56.5);
    assert_eq!(cs.frames[0].range.end, text.len());
}

#[test]
fn baseline_beyond_bottom_still_oversets_and_threads() {
    let text = "First\u{2028}Second";
    let (mut d, sid, f1) = baseline_fit_doc(text, 20.75);
    let before = compose_story(&d, sid, &ComposeOptions::default());
    assert_eq!(before.line_count(), 1);
    assert_eq!(before.overset_at, Some("First\u{2028}".len()));
    let lid = d.default_layer();
    let (f2, _) = d.add_text_frame(SpreadRef::Doc(0), Rect::new(0.0, 50.0, 200.0, 60.0), lid, "", ParaFormat::default()).unwrap();
    let options = d.item(f1).unwrap().text_frame().unwrap().options.clone();
    d.item_mut(f2).unwrap().text_frame_mut().unwrap().options = options;
    d.thread(f1, f2).unwrap();
    let after = compose_story(&d, sid, &ComposeOptions::default());
    assert!(!after.is_overset());
    assert_eq!(after.frames.iter().map(|f| f.lines.len()).collect::<Vec<_>>(), [1, 1]);
    assert_eq!(after.frames[0].lines[0].baseline, before.frames[0].lines[0].baseline);
}

#[test]
fn vertical_text_keeps_its_full_cross_line_extent() {
    let (mut d, sid, fid) = baseline_fit_doc("Map", 200.0);
    let it = d.item_mut(fid).unwrap();
    it.path = designcraft_geom::shapes::rectangle(Rect::new(0.0, 0.0, 10.0, 200.0));
    d.story_mut(sid).unwrap().vertical = true;
    let cs = compose_story(&d, sid, &ComposeOptions::default());
    assert!(cs.is_overset());
    assert_eq!(cs.line_count(), 0);
}

#[test]
fn inline_objects_still_clear_the_bottom_edge() {
    let (mut d, sid, _) = baseline_fit_doc("Text", 10.0);
    let it = designcraft_doc::Item::new(
        ItemId(d.alloc()),
        d.default_layer(),
        designcraft_doc::Shape::Rectangle,
        designcraft_geom::shapes::rectangle(Rect::new(0.0, 0.0, 6.0, 6.0)),
    );
    d.story_mut(sid).unwrap().insert_object(0, designcraft_doc::AnchoredObject::new(it, designcraft_doc::AnchorPosition::Inline { y_offset: -3.0 }));
    let cs = compose_story(&d, sid, &ComposeOptions::default());
    assert!(cs.is_overset());
    assert_eq!(cs.line_count(), 0);
}

#[test]
fn custom_anchors_do_not_change_bottom_baseline_fitting() {
    let (mut d, sid, _) = baseline_fit_doc("Text", 10.0);
    let it = designcraft_doc::Item::new(
        ItemId(d.alloc()),
        d.default_layer(),
        designcraft_doc::Shape::Rectangle,
        designcraft_geom::shapes::rectangle(Rect::new(0.0, 0.0, 6.0, 6.0)),
    );
    let pos = designcraft_doc::AnchorPosition::Custom {
        x_relative: Default::default(),
        y_relative: Default::default(),
        x_offset: 0.0,
        y_offset: 0.0,
        object_point: 0,
        ref_point: 0,
        keep_within_column: false,
        spine_relative: false,
    };
    d.story_mut(sid).unwrap().insert_object(0, designcraft_doc::AnchoredObject::new(it, pos));
    let cs = compose_story(&d, sid, &ComposeOptions::default());
    assert!(!cs.is_overset());
    assert_eq!(cs.line_count(), 1);
    assert_eq!(cs.frames[0].objects.len(), 1);
}

#[test]
fn baseline_shift_keeps_its_metrics_and_glyph_offset_at_bottom() {
    let (mut d, sid, _) = baseline_fit_doc("Map", 10.0);
    let plain = compose_story(&d, sid, &ComposeOptions::default());
    d.story_mut(sid).unwrap().format_chars(0..3, |f| f.over.baseline_shift = Some(-3.0));
    let shifted = compose_story(&d, sid, &ComposeOptions::default());
    assert!(!plain.is_overset() && !shifted.is_overset());
    let (p, s) = (&plain.frames[0].lines[0], &shifted.frames[0].lines[0]);
    assert_eq!(p.baseline, s.baseline);
    assert!((s.descent - p.descent - 3.0).abs() < 1e-6);
    assert!((s.glyphs[0].y - p.glyphs[0].y - 3.0).abs() < 1e-6);
}

#[test]
fn path_text_keeps_the_original_vertical_fit_limit() {
    // A path's synthetic layout area is not an ordinary text frame: admitting a
    // second line would paint both lines on the same path, regardless of leading.
    for separator in ['\n', designcraft_doc::story::FORCED_LINE_BREAK] {
        for leading in [8.0, 10.0] {
            let text = format!("HELLO{separator}WORLD");
            let (mut d, sid, fid) = doc_with(&text, Rect::new(0.0, 0.0, 200.0, 100.0), ParaAttrs::default());
            let it = d.item_mut(fid).unwrap();
            it.shape = designcraft_doc::Shape::Path;
            it.path = designcraft_geom::shapes::line(Point::ZERO, Point::new(200.0, 0.0));
            it.text_frame_mut().unwrap().options.path =
                Some(designcraft_doc::PathType { start: 0.0, end: None, flip: false, align: designcraft_doc::PathAlign::Baseline });
            d.story_mut(sid).unwrap().format_chars(0..text.len(), |f| {
                f.over.font_family = Some("Source Serif 4".into());
                f.over.size = Some(20.0);
                f.over.leading = Some(designcraft_doc::Leading::Points(leading));
            });
            let cs = compose_story(&d, sid, &ComposeOptions::default());
            assert_eq!(cs.line_count(), 1, "{separator:?}, leading {leading}");
            assert_eq!(cs.overset_at, Some("HELLO".len() + separator.len_utf8()));
        }
    }
}

/// A path whose length is NaN (a NaN point from a damaged file) composes without panicking on the
/// end bracket's clamp.
#[test]
fn path_text_with_a_nan_length_composes() {
    let (mut d, sid, fid) = doc_with("HELLO", Rect::new(0.0, 0.0, 200.0, 100.0), ParaAttrs::default());
    let it = d.item_mut(fid).unwrap();
    it.shape = designcraft_doc::Shape::Path;
    it.path = designcraft_geom::shapes::line(Point::ZERO, Point::new(f64::NAN, 0.0));
    it.text_frame_mut().unwrap().options.path =
        Some(designcraft_doc::PathType { start: 0.0, end: Some(50.0), flip: false, align: designcraft_doc::PathAlign::Baseline });
    let _ = compose_story(&d, sid, &ComposeOptions::default());
}

fn two_keep_frames(text: &str) -> (Document, StoryId, ItemId, ItemId) {
    let (mut d, sid, first) = baseline_fit_doc(text, 48.0);
    d.item_mut(first).unwrap().path = designcraft_geom::shapes::rectangle(Rect::new(0.0, 0.0, 75.0, 48.0));
    let lid = d.default_layer();
    let (second, _) = d.add_text_frame(SpreadRef::Doc(0), Rect::new(100.0, 0.0, 210.0, 69.0), lid, "", ParaFormat::default()).unwrap();
    let options = d.item(first).unwrap().text_frame().unwrap().options.clone();
    d.item_mut(second).unwrap().text_frame_mut().unwrap().options = options;
    d.thread(first, second).unwrap();
    (d, sid, first, second)
}

#[test]
fn keep_chain_spanning_frames_does_not_empty_its_top_column() {
    let a = "One\u{2028}Two\u{2028}Three\u{2028}Four\u{2028}Five\u{2028}Six";
    let b = "Seven\u{2028}Eight\u{2028}Nine\u{2028}Ten";
    let text = format!("{a}\n{b}\nEnd");
    let (mut d, sid, first, second) = two_keep_frames(&text);
    let paras = &mut d.story_mut(sid).unwrap().paras;
    paras[0].para.keep_with_next = Some(2);
    paras[0].para.space_after = Some(2.0);
    paras[1].para.keep_with_next = Some(2);
    let cs = compose_story(&d, sid, &ComposeOptions::default());
    // The keep chain cannot fit in either column. Fall back to flowing it, rather
    // than moving a paragraph already at a column top and throwing that column away.
    assert_eq!(cs.frame(first).unwrap().lines.len(), 4);
    assert_eq!(cs.frame(second).unwrap().lines.len(), 6);
    assert_eq!(cs.overset_at, Some(a.len() + b.len() + 2));
}

#[test]
fn satisfiable_keep_chain_still_moves_together_to_the_next_frame() {
    let text = "One\u{2028}Two\nHeading\nSubheading\nBody";
    let (mut d, sid, first, second) = two_keep_frames(text);
    let paras = &mut d.story_mut(sid).unwrap().paras;
    paras[1].para.keep_with_next = Some(1);
    paras[2].para.keep_with_next = Some(1);
    let cs = compose_story(&d, sid, &ComposeOptions::default());
    assert!(!cs.is_overset());
    assert_eq!(cs.frame(first).unwrap().lines.len(), 2);
    assert_eq!(cs.frame(second).unwrap().lines.iter().map(|l| l.para).collect::<Vec<_>>(), [1, 2, 3]);
}

/// OS/2 field offsets: sTypoAscender, sTypoDescender, the high half of ulCodePageRange1, sxHeight,
/// sCapHeight.
const TYPO_ASCENDER: usize = 68;
const TYPO_DESCENDER: usize = 70;
const CODE_PAGES_HIGH: usize = 78;
const X_HEIGHT: usize = 86;
const CAP_HEIGHT: usize = 88;

/// Source Sans 3 Regular (1000 units per em; OS/2 typo ascender = hhea ascender = 1000, cap height
/// 660, x height 486) renamed to `family` (13 characters, the length of "Source Sans 3"), with the
/// OS/2 fields at the given offsets set, or without an OS/2 table for `None`.
fn test_font(family: &str, os2_fields: Option<&[(usize, i16)]>) -> Vec<u8> {
    let mut b = designcraft_fonts::bundled()[0].to_vec();
    assert_eq!(family.len(), "Source Sans 3".len());
    let utf16 = |s: &str| s.encode_utf16().flat_map(u16::to_be_bytes).collect::<Vec<u8>>();
    for (from, to) in [(utf16("Source Sans 3"), utf16(family)), (b"Source Sans 3".to_vec(), family.as_bytes().to_vec())] {
        let mut i = 0;
        while let Some(p) = b[i..].windows(from.len()).position(|w| w == from.as_slice()) {
            b[i + p..i + p + to.len()].copy_from_slice(&to);
            i += p + to.len();
        }
    }
    let tables = u16::from_be_bytes([b[4], b[5]]) as usize;
    let rec = (0..tables).map(|t| 12 + 16 * t).find(|&r| &b[r..r + 4] == b"OS/2").unwrap();
    match os2_fields {
        Some(fields) => {
            let os2 = u32::from_be_bytes(b[rec + 8..rec + 12].try_into().unwrap()) as usize;
            for &(at, v) in fields {
                b[os2 + at..os2 + at + 2].copy_from_slice(&v.to_be_bytes());
            }
        }
        // Still sorted between its neighbours, so the table directory stays valid.
        None => b[rec..rec + 4].copy_from_slice(b"OS/1"),
    }
    b
}

/// [`test_font`] with OS/2 `sTypoAscender` set to `typo_ascender`, or without OS/2 for `None`.
fn typo_test_font(family: &str, typo_ascender: Option<i16>) -> Vec<u8> {
    test_font(family, typo_ascender.map(|v| [(TYPO_ASCENDER, v)]).as_ref().map(|f| f.as_slice()))
}

/// First baseline (frame space) of a 20 pt line in `family` in a frame at y = 36 with a 4 pt top
/// inset, set up by `opts`.
fn first_baseline_in(family: &str, opts: impl Fn(&mut designcraft_doc::TextFrameOptions)) -> f64 {
    first_baseline_in_dir(family, false, opts)
}

/// [`first_baseline_in`] in a vertical story when `vertical`.
fn first_baseline_in_dir(family: &str, vertical: bool, opts: impl Fn(&mut designcraft_doc::TextFrameOptions)) -> f64 {
    let (mut d, sid, fid) = doc_with("Hxg Hxg", Rect::new(36.0, 36.0, 300.0, 300.0), ParaAttrs::default());
    d.story_mut(sid).unwrap().format_chars(0..7, |f| {
        f.over.font_family = Some(family.into());
        f.over.font_style = Some("Regular".into());
        f.over.size = Some(20.0);
    });
    d.story_mut(sid).unwrap().vertical = vertical;
    let tf = d.item_mut(fid).unwrap().text_frame_mut().unwrap();
    tf.options.inset = [4.0, 0.0, 0.0, 0.0];
    opts(&mut tf.options);
    let cs = compose_story(&d, sid, &ComposeOptions::default());
    cs.frames[0].lines[0].baseline
}

#[test]
fn ascent_first_baseline_uses_the_typographic_ascender() {
    use designcraft_doc::FirstBaseline;
    let db = designcraft_fonts::FontDb::global();
    db.add_font(typo_test_font("TypoAscent 07", Some(700)));
    let face = db.face("TypoAscent 07", "Regular");
    assert_eq!(face.family, "TypoAscent 07");
    assert_eq!((face.ascent, face.typo_ascent), (1000.0, 700.0), "hhea 1 em, typo 0.7 em");
    let top = 36.0 + 4.0;
    let at = |kind: FirstBaseline, min: f64| {
        first_baseline_in("TypoAscent 07", |o| {
            o.first_baseline = kind;
            o.first_baseline_min = min;
        })
    };
    let close = |a: f64, b: f64| (a - b).abs() < 0.01;
    // Ascent: top + typo ascender (0.7 em of 20 pt), not the hhea ascender (1 em).
    assert!(close(at(FirstBaseline::Ascent, 0.0), top + 14.0), "{}", at(FirstBaseline::Ascent, 0.0));
    // The minimum offset still wins when it is larger.
    assert!(close(at(FirstBaseline::Ascent, 18.0), top + 18.0));
    assert!(close(at(FirstBaseline::Ascent, 10.0), top + 14.0));
    // The other kinds don't use it.
    assert!(close(at(FirstBaseline::CapHeight, 0.0), top + 0.660 * 20.0));
    assert!(close(at(FirstBaseline::XHeight, 0.0), top + 0.486 * 20.0));
    assert!(close(at(FirstBaseline::Leading, 0.0), top + 24.0));
    assert!(close(at(FirstBaseline::Fixed, 0.0), top));
    // A mixed first line takes its tallest typographic ascender: 0.7 × 20 pt beats 1 em × 10 pt …
    let (mut d, sid, fid) = doc_with("Hxg Hxg", Rect::new(36.0, 36.0, 300.0, 300.0), ParaAttrs::default());
    let st = d.story_mut(sid).unwrap();
    st.format_chars(0..3, |f| {
        f.over.font_family = Some("TypoAscent 07".into());
        f.over.size = Some(20.0);
    });
    st.format_chars(3..7, |f| {
        f.over.font_family = Some("Source Sans 3".into());
        f.over.size = Some(10.0);
    });
    d.item_mut(fid).unwrap().text_frame_mut().unwrap().options.inset = [4.0, 0.0, 0.0, 0.0];
    let first = |d: &Document| compose_story(d, sid, &ComposeOptions::default()).frames[0].lines[0].baseline;
    assert!(close(first(&d), top + 14.0), "{}", first(&d));
    // … and 1 em × 16 pt beats it.
    d.story_mut(sid).unwrap().format_chars(3..7, |f| f.over.size = Some(16.0));
    assert!(close(first(&d), top + 16.0), "{}", first(&d));
}

#[test]
fn em_box_first_baseline_follows_base_then_the_typo_box_or_the_cap_height() {
    use designcraft_doc::FirstBaseline;
    let db = designcraft_fonts::FontDb::global();
    // [`test_font`] (typo descender -326, cap height 660), with or without its `BASE` table, whose
    // em box bottom (`ideo`) is at -170 units.
    let add = |family: &str, base: bool, fields: &[(usize, i16)]| {
        let mut b = test_font(family, Some(fields));
        if !base {
            let tables = u16::from_be_bytes([b[4], b[5]]) as usize;
            let rec = (0..tables).map(|t| 12 + 16 * t).find(|&r| &b[r..r + 4] == b"BASE").unwrap();
            // Still sorted between its neighbours, so the table directory stays valid.
            b[rec..rec + 4].copy_from_slice(b"BASX");
        }
        db.add_font(b);
        db.face(family, "Regular").declared_em_box()
    };
    // Typo ascender − descender = 1 em in a CJK font (JIS code page, bit 17): the typo ascender is
    // the top.
    let typo = [(TYPO_ASCENDER, 850), (TYPO_DESCENDER, -150)];
    assert_eq!(add("EmBoxTypo 850", false, &[typo[0], typo[1], (CODE_PAGES_HIGH, 1 << 1)]), Some((850.0, -150.0)));
    // In a Latin font the cap height decides: 0.5 em + 660 / 2.
    assert_eq!(add("EmBoxLatn 850", false, &typo), Some((830.0, -170.0)));
    // Otherwise the em box is centred on half the cap height: 0.5 em + 600 / 2.
    assert_eq!(add("EmBoxCapH 600", false, &[(TYPO_ASCENDER, 700), (CAP_HEIGHT, 600)]), Some((800.0, -200.0)));
    // No OS/2 cap height: the H glyph's top, 656.
    assert_eq!(add("EmBoxGlyphH 0", false, &[(TYPO_ASCENDER, 700), (CAP_HEIGHT, 0)]), Some((828.0, -172.0)));
    // A `BASE` em box comes first.
    assert_eq!(add("EmBoxBase 850", true, &[(TYPO_ASCENDER, 850), (TYPO_DESCENDER, -150)]), Some((830.0, -170.0)));
    let (mut d, sid, fid) = doc_with("Hxg Hxg", Rect::new(36.0, 36.0, 300.0, 300.0), ParaAttrs::default());
    let mut first = |family: &str, sizes: [f64; 2], kind: FirstBaseline| {
        let st = d.story_mut(sid).unwrap();
        for (range, size) in [(0..4, sizes[0]), (4..7, sizes[1])] {
            st.format_chars(range, |f| {
                f.over.font_family = Some(family.into());
                f.over.font_style = Some("Regular".into());
                f.over.size = Some(size);
            });
        }
        d.item_mut(fid).unwrap().text_frame_mut().unwrap().options.first_baseline = kind;
        compose_story(&d, sid, &ComposeOptions::default()).frames[0].lines[0].baseline - 36.0
    };
    let close = |a: f64, b: f64| (a - b).abs() < 0.01;
    let asc = first("EmBoxCapH 600", [10.0; 2], FirstBaseline::Ascent);
    assert!(close(asc, 7.0), "{asc}");
    for (family, top) in [("EmBoxTypo 850", 8.5), ("EmBoxLatn 850", 8.3), ("EmBoxCapH 600", 8.0), ("EmBoxGlyphH 0", 8.28), ("EmBoxBase 850", 8.3)] {
        let emb = first(family, [10.0; 2], FirstBaseline::EmboxHeight);
        assert!(close(emb, top), "{family}: {emb}");
    }
    // A mixed first line takes its largest size.
    let emb = first("EmBoxCapH 600", [10.0, 20.0], FirstBaseline::EmboxHeight);
    assert!(close(emb, 16.0), "{emb}");
}

#[test]
fn em_box_first_baseline_in_a_latin_font_with_a_one_em_typo_box_follows_the_cap_height() {
    use designcraft_doc::FirstBaseline;
    // DejaVu Serif: 2048 units per em, typo ascender 1556 and descender -492 (one em), cap height
    // 1493. Read from the system fonts; the test skips where it isn't installed.
    let Some(bytes) = designcraft_fonts::system_font_dirs().iter().find_map(|dir| {
        ["truetype/dejavu/DejaVuSerif.ttf", "dejavu/DejaVuSerif.ttf", "DejaVuSerif.ttf"].iter().find_map(|p| std::fs::read(dir.join(p)).ok())
    }) else {
        eprintln!("skipped: DejaVu Serif isn't installed");
        return;
    };
    let db = designcraft_fonts::FontDb::global();
    db.add_font(bytes);
    let face = db.face("DejaVu Serif", "Book");
    assert_eq!(face.family, "DejaVu Serif");
    let (mut d, sid, fid) = doc_with("Hxg Hxg", Rect::new(36.0, 36.0, 300.0, 300.0), ParaAttrs::default());
    let st = d.story_mut(sid).unwrap();
    let n = st.len();
    st.format_chars(0..n, |f| {
        f.over.font_family = Some("DejaVu Serif".into());
        f.over.font_style = Some("Book".into());
        f.over.size = Some(10.0);
    });
    d.item_mut(fid).unwrap().text_frame_mut().unwrap().options.first_baseline = FirstBaseline::EmboxHeight;
    let first = compose_story(&d, sid, &ComposeOptions::default()).frames[0].lines[0].baseline - 36.0;
    // 0.5 em + cap height / 2, not the 1556-unit typo ascender (7.60 pt).
    let expected = 5.0 + 1493.0 / 2048.0 * 5.0;
    assert!((first - expected).abs() < 0.01, "{first}, expected {expected}");
}

#[test]
fn em_box_first_baseline_ignores_baseline_shift() {
    use designcraft_doc::FirstBaseline;
    let db = designcraft_fonts::FontDb::global();
    // Source Sans 3's `BASE` em box tops 0.83 em above the baseline.
    db.add_font(test_font("EmBoxShft 830", Some(&[])));
    let (mut d, sid, fid) = doc_with("Hxg Hxg", Rect::new(36.0, 36.0, 300.0, 300.0), ParaAttrs::default());
    let st = d.story_mut(sid).unwrap();
    st.format_chars(0..7, |f| {
        f.over.font_family = Some("EmBoxShft 830".into());
        f.over.font_style = Some("Regular".into());
        f.over.size = Some(10.0);
    });
    st.format_chars(4..7, |f| f.over.baseline_shift = Some(1.0));
    d.item_mut(fid).unwrap().text_frame_mut().unwrap().options.first_baseline = FirstBaseline::EmboxHeight;
    let frames = compose_story(&d, sid, &ComposeOptions::default()).frames;
    let line = &frames[0].lines[0];
    // The unshifted run's baseline stays 8.3 pt below the frame top; the shifted run is drawn 1 pt
    // higher.
    assert!((line.baseline - 36.0 - 8.3).abs() < 0.01, "{}", line.baseline);
    let ys: Vec<f64> = line.glyphs.iter().map(|g| g.y).collect();
    assert_eq!(ys.len(), 7, "{ys:?}");
    assert!(ys[..3].iter().all(|y| y.abs() < 0.01), "{ys:?}");
    assert!(ys[4..].iter().all(|y| (y + 1.0).abs() < 0.01), "{ys:?}");
}

#[test]
fn ascent_first_baseline_falls_back_to_the_ascent_without_a_usable_typo_ascender() {
    let db = designcraft_fonts::FontDb::global();
    // No OS/2 table, a negative and an absurd typo ascender: the (hhea) ascent, 1 em.
    for (family, typo) in [("TypoAscentNoO", None), ("TypoAscentNeg", Some(-300)), ("TypoAscentBig", Some(i16::MAX))] {
        db.add_font(typo_test_font(family, typo));
        let face = db.face(family, "Regular");
        assert_eq!(face.family, family);
        assert_eq!(face.typo_ascent, face.ascent, "{family}");
        let b = first_baseline_in(family, |_| {});
        assert!((b - (36.0 + 4.0 + 20.0)).abs() < 0.01, "{family}: {b}");
    }
}

#[test]
fn vertical_frames_keep_the_ascent_first_baseline() {
    let db = designcraft_fonts::FontDb::global();
    db.add_font(typo_test_font("TypoAscentVrt", Some(700)));
    let b = first_baseline_in_dir("TypoAscentVrt", true, |o| {
        o.inset = [0.0; 4];
    });
    // Composed in the turned box (no inset): the hhea ascent, 1 em of 20 pt.
    assert!((b - 20.0).abs() < 0.01, "{b}");
}

#[test]
fn a_visible_frame_stroke_insets_the_text() {
    use designcraft_doc::{Stroke, StrokeAlign};
    let line = |stroke: Option<(StrokeAlign, &str)>| {
        let (mut d, sid, fid) = doc_with("Hxg", Rect::new(36.0, 36.0, 300.0, 300.0), ParaAttrs::default());
        let it = d.item_mut(fid).unwrap();
        if let Some((align, swatch)) = stroke {
            it.stroke = Stroke { weight: 4.0, align, swatch: swatch.into(), ..Stroke::default() };
        }
        it.text_frame_mut().unwrap().options.inset = [4.0; 4];
        let l = &compose_story(&d, sid, &ComposeOptions::default()).frames[0].lines[0];
        (l.baseline, l.x0, l.x1)
    };
    let black = designcraft_color::swatch::BLACK;
    let (b, x0, x1) = line(None);
    // The part of the 4 pt stroke inside the frame: half of it centred, all of it inside, none
    // outside or without a colour; on the left and right as on the top.
    for (stroke, d) in [
        ((StrokeAlign::Center, black), 2.0),
        ((StrokeAlign::Inside, black), 4.0),
        ((StrokeAlign::Outside, black), 0.0),
        ((StrokeAlign::Inside, designcraft_color::swatch::NONE), 0.0),
    ] {
        let (sb, sx0, sx1) = line(Some(stroke));
        assert!((sb - (b + d)).abs() < 1e-6, "{stroke:?}: baseline {sb} vs {b}");
        assert!((sx0 - (x0 + d)).abs() < 1e-6 && (sx1 - (x1 - d)).abs() < 1e-6, "{stroke:?}: {sx0}..{sx1} vs {x0}..{x1}");
    }
}

#[test]
fn list_labels_do_not_raise_the_ascent_first_baseline() {
    use designcraft_doc::{CharAttrs, ListType};
    // The paragraph's own character size (the label's) is 40 pt; its text is 10 pt (Source Sans 3:
    // typo ascender 1 em).
    let line = |text: &str, list: ListType| {
        let (mut d, sid, _) = doc_with(text, Rect::new(36.0, 36.0, 300.0, 300.0), ParaAttrs { list_type: Some(list), ..Default::default() });
        let st = d.story_mut(sid).unwrap();
        for p in &mut st.paras {
            p.chars = CharAttrs { font_family: Some("Source Sans 3".into()), size: Some(40.0), ..Default::default() };
        }
        st.format_chars(0..text.len(), |f| f.over.size = Some(10.0));
        let l = compose_story(&d, sid, &ComposeOptions::default()).frames[0].lines[0].clone();
        (l.baseline, l.ascent, l.leading)
    };
    let (b, asc, lead) = line("Hxg Hxg", ListType::None);
    // 1 em of the 10 pt text below the frame top.
    assert!((b - 46.0).abs() < 0.01, "{b}");
    for list in [ListType::Bullets, ListType::Numbers] {
        let (lb, lasc, llead) = line("Hxg Hxg", list);
        assert!((lb - b).abs() < 1e-6, "{list:?}: {lb} vs {b}");
        // The 40 pt label still sets the line's ascent and leading.
        assert!(lasc > asc + 20.0 && (llead - 48.0).abs() < 1e-6 && (lead - 12.0).abs() < 1e-6, "{list:?}: {lasc} {asc} {llead} {lead}");
    }
    // An empty bulleted paragraph sits like an empty paragraph.
    let (eb, _, _) = line("", ListType::None);
    let (ebl, _, _) = line("", ListType::Bullets);
    assert!((ebl - eb).abs() < 1e-6, "{ebl} vs {eb}");
}

#[test]
fn spaces_count_toward_the_ascent_first_baseline() {
    // 10 pt text with a 40 pt space between its words (Source Sans 3: typo ascender 1 em).
    let (mut d, sid, _) = doc_with("Hxg Hxg", Rect::new(36.0, 36.0, 300.0, 300.0), ParaAttrs::default());
    let st = d.story_mut(sid).unwrap();
    st.format_chars(0..7, |f| {
        f.over.font_family = Some("Source Sans 3".into());
        f.over.size = Some(10.0);
    });
    st.format_chars(3..4, |f| f.over.size = Some(40.0));
    let l = &compose_story(&d, sid, &ComposeOptions::default()).frames[0].lines[0];
    assert!((l.baseline - (36.0 + 40.0)).abs() < 0.01, "{}", l.baseline);
}

#[test]
fn vertical_scale_does_not_move_the_ascent_first_baseline() {
    let db = designcraft_fonts::FontDb::global();
    db.add_font(typo_test_font("TypoAscentVSc", Some(700)));
    let line = |v_scale: f64| {
        let (mut d, sid, fid) = doc_with("Hxg", Rect::new(36.0, 36.0, 300.0, 300.0), ParaAttrs::default());
        d.story_mut(sid).unwrap().format_chars(0..3, |f| {
            f.over.font_family = Some("TypoAscentVSc".into());
            f.over.size = Some(20.0);
            f.over.v_scale = Some(v_scale);
        });
        d.item_mut(fid).unwrap().text_frame_mut().unwrap().options.inset = [4.0, 0.0, 0.0, 0.0];
        let l = &compose_story(&d, sid, &ComposeOptions::default()).frames[0].lines[0];
        (l.baseline, l.ascent)
    };
    let ((b100, a100), (b200, a200)) = (line(1.0), line(2.0));
    // The typo ascender of the nominal size (0.7 × 20 pt) at 100 % and at 200 %.
    assert!((b100 - (36.0 + 4.0 + 14.0)).abs() < 0.01, "{b100}");
    assert!((b200 - b100).abs() < 0.01, "{b200} vs {b100}");
    // The line box keeps the scaled ascent.
    assert!((a100 - 20.0).abs() < 0.01 && (a200 - 40.0).abs() < 0.01, "{a100} {a200}");
}

#[test]
fn cap_height_and_x_height_first_baselines_use_the_fonts_cap_and_x_heights() {
    use designcraft_doc::FirstBaseline;
    let db = designcraft_fonts::FontDb::global();
    db.add_font(test_font("CapXHeight 01", Some(&[(CAP_HEIGHT, 500), (X_HEIGHT, 300)])));
    let face = db.face("CapXHeight 01", "Regular");
    assert_eq!(face.family, "CapXHeight 01");
    assert_eq!((face.ascent, face.cap_height, face.x_height), (1000.0, 500.0, 300.0));
    let top = 36.0 + 4.0;
    let at = |kind: FirstBaseline, min: f64| {
        first_baseline_in("CapXHeight 01", |o| {
            o.first_baseline = kind;
            o.first_baseline_min = min;
        })
    };
    let close = |a: f64, b: f64| (a - b).abs() < 0.01;
    // Cap height 0.5 em and x height 0.3 em of 20 pt, not 0.72 / 0.5 of the 1 em ascent.
    assert!(close(at(FirstBaseline::CapHeight, 0.0), top + 10.0), "{}", at(FirstBaseline::CapHeight, 0.0));
    assert!(close(at(FirstBaseline::XHeight, 0.0), top + 6.0), "{}", at(FirstBaseline::XHeight, 0.0));
    // The minimum offset still wins when it is larger.
    assert!(close(at(FirstBaseline::CapHeight, 12.0), top + 12.0));
    assert!(close(at(FirstBaseline::XHeight, 5.0), top + 6.0));
    // Vertical scale and a raised baseline count, as for the ascent.
    let (mut d, sid, fid) = doc_with("Hxg Hxg", Rect::new(36.0, 36.0, 300.0, 300.0), ParaAttrs::default());
    let st = d.story_mut(sid).unwrap();
    st.format_chars(0..7, |f| {
        f.over.font_family = Some("CapXHeight 01".into());
        f.over.size = Some(20.0);
        f.over.v_scale = Some(1.5);
    });
    st.format_chars(4..5, |f| f.over.baseline_shift = Some(1.0));
    let tf = d.item_mut(fid).unwrap().text_frame_mut().unwrap();
    tf.options.inset = [4.0, 0.0, 0.0, 0.0];
    tf.options.first_baseline = FirstBaseline::CapHeight;
    let first = |d: &Document| compose_story(d, sid, &ComposeOptions::default()).frames[0].lines[0].baseline;
    assert!(close(first(&d), top + 15.0 + 1.0), "{}", first(&d));
    d.item_mut(fid).unwrap().text_frame_mut().unwrap().options.first_baseline = FirstBaseline::XHeight;
    assert!(close(first(&d), top + 9.0 + 1.0), "{}", first(&d));
}

#[test]
fn cap_height_and_x_height_fall_back_without_usable_os2_values() {
    use designcraft_doc::FirstBaseline;
    let db = designcraft_fonts::FontDb::global();
    // No OS/2 table, zero / negative and absurd values: the top of the H (0.656 em) and of the x
    // (0.486 em).
    let zero: &[(usize, i16)] = &[(CAP_HEIGHT, 0), (X_HEIGHT, -40)];
    let big: &[(usize, i16)] = &[(CAP_HEIGHT, i16::MAX), (X_HEIGHT, i16::MAX)];
    for (family, fields) in [("CapXHeightNoO", None), ("CapXHeightNeg", Some(zero)), ("CapXHeightBig", Some(big))] {
        db.add_font(test_font(family, fields));
        let face = db.face(family, "Regular");
        assert_eq!(face.family, family);
        assert_eq!((face.cap_height, face.x_height), (656.0, 486.0), "{family}");
        let cap = first_baseline_in(family, |o| o.first_baseline = FirstBaseline::CapHeight);
        let x = first_baseline_in(family, |o| o.first_baseline = FirstBaseline::XHeight);
        assert!((cap - (40.0 + 13.12)).abs() < 0.01 && (x - (40.0 + 9.72)).abs() < 0.01, "{family}: {cap} {x}");
    }
}

#[test]
fn vertical_frames_keep_the_cap_height_and_x_height_first_baselines() {
    use designcraft_doc::FirstBaseline;
    let db = designcraft_fonts::FontDb::global();
    db.add_font(test_font("CapXHeightVrt", Some(&[(CAP_HEIGHT, 500), (X_HEIGHT, 300)])));
    let at = |kind: FirstBaseline| {
        first_baseline_in_dir("CapXHeightVrt", true, |o| {
            o.first_baseline = kind;
            o.inset = [0.0; 4];
        })
    };
    // 0.72 and 0.5 of the 1 em ascent of 20 pt.
    assert!((at(FirstBaseline::CapHeight) - 14.4).abs() < 0.01, "{}", at(FirstBaseline::CapHeight));
    assert!((at(FirstBaseline::XHeight) - 10.0).abs() < 0.01, "{}", at(FirstBaseline::XHeight));
}

// ---------- drop caps ----------

/// `text` set 10 pt on 12 pt leading in a 300 pt measure, with `para` on top.
fn drop_doc(text: &str, para: ParaAttrs) -> (Document, StoryId, ItemId) {
    drop_doc_in(text, Rect::new(0.0, 0.0, 300.0, 1000.0), para)
}

fn drop_doc_in(text: &str, rect: Rect, para: ParaAttrs) -> (Document, StoryId, ItemId) {
    let (mut d, sid, fid) = doc_with(text, rect, para);
    for p in &mut d.story_mut(sid).unwrap().paras {
        p.chars = designcraft_doc::CharAttrs { size: Some(10.0), leading: Some(designcraft_doc::Leading::Points(12.0)), ..Default::default() };
    }
    (d, sid, fid)
}

fn drop_cap(lines: u32, chars: u32) -> ParaAttrs {
    ParaAttrs { drop_cap_lines: Some(lines), drop_cap_chars: Some(chars), ..Default::default() }
}

/// Cap height of a placed glyph, in points.
fn cap_of(g: &PlacedGlyph) -> f64 {
    g.face.cap_height * g.sy
}

/// x of the first glyph of `l` set from story byte `from` on.
fn text_x(l: &Line, from: usize) -> f64 {
    l.glyphs.iter().find(|g| g.len > 0 && g.byte >= from).map(|g| g.x).unwrap()
}

#[test]
fn drop_cap_spans_lines_and_indents_them() {
    let text = [LOREM; 2].join(" ");
    let (plain, psid, _) = drop_doc(&text, ParaAttrs::default());
    let plain = compose_story(&plain, psid, &ComposeOptions::default());
    let (d, sid, _) = drop_doc(&text, drop_cap(3, 1));
    let cs = compose_story(&d, sid, &ComposeOptions::default());
    let lines = all_lines(&cs);
    let pl = all_lines(&plain);
    assert!(lines.len() > 4);
    // Baselines don't move.
    for (k, l) in lines.iter().enumerate() {
        assert!((l.baseline - (pl[0].baseline + 12.0 * k as f64)).abs() < 1e-6, "line {k} at {}", l.baseline);
    }
    // The drop cap: the first glyph, on line 1, its baseline on line 3's and its cap top on line 1's.
    let dc = &lines[0].glyphs[0];
    assert_eq!((dc.byte, dc.len), (0, 1));
    assert!((lines[0].baseline + dc.y - lines[2].baseline).abs() < 1e-6, "drop cap baseline {}", lines[0].baseline + dc.y);
    let body = lines[0].glyphs.iter().find(|g| g.byte == 1).unwrap();
    let top = lines[2].baseline - cap_of(dc);
    assert!((top - (lines[0].baseline - cap_of(body))).abs() < 0.01, "cap top {top}");
    assert!(dc.sy > body.sy * 3.0);
    // Lines 1–3 start after it, line 4 at the frame edge.
    let right = dc.x + dc.adv;
    assert!(right > 20.0);
    for l in &lines[..3] {
        assert!(text_x(l, 1) >= right - 1e-6, "{} < {right}", text_x(l, 1));
        assert!(l.end_x <= l.x1 + 0.5);
    }
    assert!(text_x(lines[3], 1) < 0.5);
    // The text stays in order: the drop cap starts line 1 (exports and caret read it there).
    assert_eq!(lines[0].range.start, 0);
}

#[test]
fn tab_beside_a_drop_cap_is_measured_from_the_drop_cap() {
    // The line breaker measures a tab from where the line starts, which beside a drop cap is
    // after the drop cap: measured from the frame edge, the tab looks wider and the line breaks
    // a word early.
    let stop = designcraft_doc::TabStop { position: 100.0, align: TabAlign::Left, leader: String::new(), align_on: String::new() };
    let para = ParaAttrs { tabs: Some(vec![stop]), hyphenate: Some(false), ..drop_cap(3, 1) };
    let text = format!("Wort\t{LOREM}");
    let (d, sid, _) = drop_doc(&text, para);
    let cs = compose_story(&d, sid, &ComposeOptions::default());
    let lines = all_lines(&cs);
    let space = |g: &PlacedGlyph| g.len > 0 && text.get(g.byte..).and_then(|s| s.chars().next()).is_some_and(char::is_whitespace);
    let right = lines[0].glyphs.iter().filter(|g| g.visible && !space(g)).map(|g| g.x + g.adv).fold(0.0, f64::max);
    assert!(right <= lines[0].x1 + 0.01, "line 1 ends at {right}, past {}", lines[0].x1);
    let word: f64 = lines[1].glyphs.iter().filter(|g| g.len > 0).take_while(|g| !space(g)).map(|g| g.adv).sum();
    assert!(right + word + 4.0 > lines[0].x1, "line 2's first word ({word} pt) fits after line 1's end at {right} (edge {})", lines[0].x1);
}

#[test]
fn warichu_on_a_drop_cap_line_leaves_the_drop_cap_alone() {
    let text = format!("WABCDEFGH {LOREM}");
    let (mut d, sid, _) = drop_doc(&text, drop_cap(3, 1));
    let plain = compose_story(&d, sid, &ComposeOptions::default());
    let plain_dc = plain.frames[0].lines[0].glyphs[0].clone();
    // A plain drop cap line has no stacked rows: one selection band, caret on the line.
    let pl = &plain.frames[0].lines[0];
    assert_eq!(highlight_quads(pl, 0, 12, false).len(), 1);
    assert_eq!(caret(&plain, 1).map(|c| (c.2, c.3)), Some((pl.baseline, pl.ascent)));
    d.story_mut(sid).unwrap().format_chars(1..9, |f| {
        f.over.warichu = Some(true);
        f.over.warichu_alignment = Some(designcraft_doc::cjk::WarichuAlignment::Left);
    });
    let cs = compose_story(&d, sid, &ComposeOptions::default());
    let l = &cs.frames[0].lines[0];
    let dc = &l.glyphs[0];
    assert_eq!((dc.byte, dc.x, dc.y, dc.sy), (0, plain_dc.x, plain_dc.y, plain_dc.sy), "the drop cap is not part of the note");
    let at = |byte: usize| l.glyphs.iter().find(|g| g.byte == byte && g.len > 0).unwrap();
    let (top, bot, after) = (at(1), at(5), at(10));
    assert!(bot.y - top.y > 1.0, "the note is stacked in two rows");
    assert!(top.x >= dc.x + dc.adv - 1e-6, "the note starts after the drop cap");
    assert!((after.x - (at(8).x + at(8).adv + at(9).adv)).abs() < 1e-3, "the line closes up after the note");
    // The caret: in the drop cap as tall as it, on a note row that row's baseline, after the
    // note the parent line's.
    let dc_box = l.drop_cap.unwrap();
    assert_eq!(caret(&cs, 0).map(|c| c.2), Some(dc_box.baseline));
    assert!((caret(&cs, 1).unwrap().2 - (l.baseline + top.y)).abs() < 1e-6);
    assert_eq!(caret(&cs, 10).map(|c| (c.2, c.3)), Some((l.baseline, l.ascent)));
    let click = |g: &PlacedGlyph| hit(&cs, 0, designcraft_geom::Point::new(g.x + g.adv * 0.25, l.baseline + g.y)).unwrap();
    assert_eq!((click(top), click(bot), click(after)), (1, 5, 10));
    // Selecting the drop cap and the note: the drop cap's band and one per row.
    assert_eq!(highlight_quads(l, 0, 9, false).len(), 3);
}

#[test]
fn drop_cap_of_two_characters_as_a_local_override() {
    let (d, sid, _) = drop_doc(LOREM, drop_cap(2, 2));
    let cs = compose_story(&d, sid, &ComposeOptions::default());
    let lines = all_lines(&cs);
    let (a, b) = (&lines[0].glyphs[0], &lines[0].glyphs[1]);
    assert_eq!((a.byte, b.byte), (0, 1));
    let body = lines[0].glyphs.iter().find(|g| g.byte == 2).unwrap();
    for g in [a, b] {
        assert!((lines[0].baseline + g.y - lines[1].baseline).abs() < 1e-6);
        assert!(g.sy > body.sy * 1.5);
    }
    assert!((b.x - (a.x + a.adv)).abs() < 1e-6, "the drop cap characters sit side by side");
    let right = b.x + b.adv;
    assert!(text_x(lines[0], 2) >= right - 1e-6 && text_x(lines[1], 2) >= right - 1e-6);
    assert!(text_x(lines[2], 2) < 0.5);
}

#[test]
fn short_paragraph_keeps_its_drop_cap_and_the_next_paragraph_is_not_indented() {
    let (mut d, sid, _) = drop_doc(&format!("Short.\n{LOREM}"), drop_cap(3, 1));
    d.story_mut(sid).unwrap().paras[1].para = ParaAttrs::default();
    let cs = compose_story(&d, sid, &ComposeOptions::default());
    let lines = all_lines(&cs);
    assert_eq!(lines.iter().filter(|l| l.para == 0).count(), 1);
    let dc = &lines[0].glyphs[0];
    // Still three lines tall (it may overlap the next paragraph, which isn't indented).
    assert!((dc.y - 24.0).abs() < 1e-6, "{}", dc.y);
    assert!(text_x(lines[1], 7) < 0.5);
}

#[test]
fn drop_cap_counts_code_points_without_splitting_clusters_and_clamps_to_the_paragraph() {
    // Four characters: a letter, its combining mark, another letter and a no-break space.
    let (d, sid, _) = drop_doc("A\u{301}B\u{a0}rest of the words", drop_cap(2, 4));
    let cs = compose_story(&d, sid, &ComposeOptions::default());
    let l = &cs.frames[0].lines[0];
    let big: Vec<usize> = l.glyphs.iter().filter(|g| g.len > 0 && g.y > 1.0).map(|g| g.byte).collect();
    assert!(big.contains(&0) && big.contains(&3), "{big:?}");
    assert!(big.iter().all(|b| *b < 6), "the r isn't part of it: {big:?}");
    // One character that has a combining mark after it takes the mark too.
    let (d, sid, _) = drop_doc("e\u{301}tude and more words", drop_cap(2, 1));
    let cs = compose_story(&d, sid, &ComposeOptions::default());
    let l = &cs.frames[0].lines[0];
    let big: Vec<usize> = l.glyphs.iter().filter(|g| g.y > 1.0).map(|g| g.byte).collect();
    assert!(!big.is_empty() && big.iter().all(|b| *b < 3), "{big:?}");
    assert!(l.glyphs.iter().any(|g| g.byte == 0 && g.y > 1.0));
    // More characters than the paragraph has: all of them, and nothing of the next paragraph.
    let (mut d, sid, _) = drop_doc("Hi\nNext", drop_cap(2, 50));
    d.story_mut(sid).unwrap().paras[1].para = ParaAttrs::default();
    let cs = compose_story(&d, sid, &ComposeOptions::default());
    let lines = all_lines(&cs);
    assert!(lines[0].glyphs.iter().filter(|g| g.len > 0).all(|g| g.y > 1.0));
    assert!(lines[1].glyphs.iter().all(|g| g.y.abs() < 1e-6));
}

#[test]
fn hostile_drop_cap_values_do_not_panic() {
    for (lines, chars) in [(u32::MAX, u32::MAX), (1_000_000, 1), (2, 0), (0, 3)] {
        let (d, sid, _) = drop_doc(LOREM, drop_cap(lines, chars));
        let cs = compose_story(&d, sid, &ComposeOptions::default());
        let g = &cs.frames[0].lines[0].glyphs[0];
        assert!(g.y.is_finite() && g.sy.is_finite() && g.y <= 12.0 * 25.0, "{lines}/{chars}: {}", g.y);
    }
    // No lines or no characters: no drop cap.
    let (d, sid, _) = drop_doc(LOREM, drop_cap(0, 3));
    let cs = compose_story(&d, sid, &ComposeOptions::default());
    assert!(cs.frames[0].lines[0].glyphs[0].y.abs() < 1e-6);
}

#[test]
fn one_line_drop_cap_is_not_enlarged() {
    let (d, sid, _) = drop_doc(LOREM, drop_cap(1, 1));
    let cs = compose_story(&d, sid, &ComposeOptions::default());
    let l = &cs.frames[0].lines[0];
    let (dc, body) = (&l.glyphs[0], l.glyphs.iter().find(|g| g.byte == 1).unwrap());
    assert!((cap_of(dc) - cap_of(body)).abs() < 0.01 && dc.y.abs() < 1e-6);
}

#[test]
fn drop_cap_stays_with_the_first_line_across_a_column_break() {
    // Two lines fit a column: line 3 starts the second column, at its edge.
    let (mut d, sid, fid) = drop_doc_in(&[LOREM; 2].join(" "), Rect::new(0.0, 0.0, 400.0, 30.0), drop_cap(3, 1));
    d.item_mut(fid).unwrap().text_frame_mut().unwrap().options.columns = 2;
    let cs = compose_story(&d, sid, &ComposeOptions::default());
    let ft = &cs.frames[0];
    let col1: Vec<&Line> = ft.lines.iter().filter(|l| l.column == 1).collect();
    assert_eq!(ft.lines.iter().filter(|l| l.column == 0).count(), 2, "{:?}", ft.lines.iter().map(|l| l.column).collect::<Vec<_>>());
    let dc = &ft.lines[0].glyphs[0];
    assert!((dc.y - 24.0).abs() < 1e-6, "{}", dc.y);
    assert!(text_x(&ft.lines[1], 1) >= dc.x + dc.adv - 1e-6);
    assert!((text_x(col1[0], 1) - ft.columns[1].x0).abs() < 0.5);
}

#[test]
fn drop_cap_in_right_to_left_paragraphs_is_on_the_right() {
    let para = ParaAttrs { direction: Some(designcraft_doc::TextDirection::RightToLeft), align: Some(Align::Right), ..drop_cap(2, 1) };
    let (d, sid, _) = drop_doc(LOREM, para);
    let cs = compose_story(&d, sid, &ComposeOptions::default());
    let lines = all_lines(&cs);
    let dc = lines[0].glyphs.iter().find(|g| g.byte == 0).unwrap();
    assert!((dc.x + dc.adv - 300.0).abs() < 0.5, "{}", dc.x + dc.adv);
    for l in &lines[..2] {
        assert!(l.glyphs.iter().filter(|g| g.byte > 0 && g.visible).all(|g| g.x + g.adv <= dc.x + 0.5));
    }
    assert!(lines[2].end_x > 299.0 || lines[2].glyphs.iter().any(|g| g.x + g.adv > 299.0));
}

#[test]
fn vertical_frames_set_no_drop_cap() {
    let (mut d, sid, _) = drop_doc(LOREM, drop_cap(3, 1));
    d.story_mut(sid).unwrap().vertical = true;
    let cs = compose_story(&d, sid, &ComposeOptions::default());
    assert!(cs.frames[0].lines[0].glyphs[0].y.abs() < 1e-6);
}

#[test]
fn drop_cap_takes_its_character_style_and_nested_styles_count_it() {
    let (mut d, sid, _) = drop_doc(LOREM, ParaAttrs { drop_cap_style: Some("Initial".into()), ..drop_cap(2, 3) });
    for (name, fill) in [("Initial", "Initial Red"), ("Lead", "Lead Blue")] {
        std::sync::Arc::make_mut(&mut d.styles).character.push(designcraft_doc::CharacterStyle {
            name: name.into(),
            based_on: None,
            chars: designcraft_doc::CharAttrs { fill: Some(fill.into()), ..Default::default() },
            shortcut: String::new(),
        });
    }
    let fill = |cs: &ComposedStory, g: &PlacedGlyph| cs.styles[g.style as usize].fill.clone();
    let cs = compose_story(&d, sid, &ComposeOptions::default());
    let l = &cs.frames[0].lines[0];
    let at = |b: usize| l.glyphs.iter().find(|g| g.byte == b).unwrap();
    assert_eq!(fill(&cs, at(0)), "Initial Red");
    assert_eq!(fill(&cs, at(2)), "Initial Red");
    assert_eq!(fill(&cs, at(3)), "[Black]");
    // The drop cap's style is a leading "through 1 drop cap" nested style: the nested styles start
    // after the drop cap.
    let one = designcraft_doc::NestedStyle { style: "Lead".into(), through: true, count: 1, until: designcraft_doc::NestedUntil::Characters };
    d.story_mut(sid).unwrap().paras[0].para.nested_styles = Some(vec![one]);
    let cs = compose_story(&d, sid, &ComposeOptions::default());
    let l = &cs.frames[0].lines[0];
    let at = |b: usize| l.glyphs.iter().find(|g| g.byte == b).unwrap();
    assert_eq!(fill(&cs, at(0)), "Initial Red");
    assert_eq!((fill(&cs, at(3)), fill(&cs, at(4))), ("Lead Blue".to_string(), "[Black]".to_string()));
    // A nested style "through 1 drop cap" covers the drop cap; the next one starts after it.
    let ns = |style: &str, until: designcraft_doc::NestedUntil| designcraft_doc::NestedStyle { style: style.into(), through: true, count: 1, until };
    d.story_mut(sid).unwrap().paras[0].para = ParaAttrs {
        nested_styles: Some(vec![ns("Lead", designcraft_doc::NestedUntil::Dropcap), ns("Initial", designcraft_doc::NestedUntil::Words)]),
        ..drop_cap(2, 3)
    };
    let cs = compose_story(&d, sid, &ComposeOptions::default());
    let l = &cs.frames[0].lines[0];
    let at = |b: usize| l.glyphs.iter().find(|g| g.byte == b).unwrap();
    assert_eq!(fill(&cs, at(0)), "Lead Blue");
    assert_eq!(fill(&cs, at(2)), "Lead Blue");
    assert_eq!(fill(&cs, at(3)), "Initial Red", "the second nested style starts after the drop cap");
    let first_space = LOREM.find(' ').unwrap();
    assert_eq!(fill(&cs, at(first_space + 1)), "[Black]");
}

#[test]
fn drop_cap_caret_and_hit_testing() {
    let (d, sid, _) = drop_doc(LOREM, drop_cap(3, 1));
    let cs = compose_story(&d, sid, &ComposeOptions::default());
    let lines = all_lines(&cs);
    let dc = lines[0].glyphs[0].clone();
    let drop_box = lines[0].drop_cap.unwrap().rect;
    assert!(drop_box.y1 > lines[2].baseline - 0.01 && drop_box.y0 < lines[0].baseline - 5.0, "{drop_box:?}");
    // A click on the lower half of the big letter (beside line 3) is at the drop cap.
    let y = lines[2].baseline - 2.0;
    assert_eq!(hit(&cs, 0, Point::new(dc.x + dc.adv * 0.25, y)), Some(0));
    assert_eq!(hit(&cs, 0, Point::new(dc.x + dc.adv * 0.75, y)), Some(1));
    // A double click there is on the drop cap's character, not on line 3's text.
    assert_eq!(hit_char(&cs, 0, Point::new(dc.x + dc.adv * 0.5, y)), Some(0));
    // The caret before the drop cap is as tall as it; after it, beside line 1's text.
    let (_, x, bl, asc, _) = caret(&cs, 0).unwrap();
    assert!((x - dc.x).abs() < 1e-6 && (bl - lines[2].baseline).abs() < 1e-6 && asc > 24.0, "{x} {bl} {asc}");
    let (_, x, bl, _, _) = caret(&cs, 1).unwrap();
    assert!(x >= dc.x + dc.adv - 1e-6 && (bl - lines[0].baseline).abs() < 1e-6);
}

#[test]
fn drop_cap_on_a_baseline_grid_reaches_down_by_grid_steps() {
    // Lines on a 15 pt baseline grid (not the 12 pt leading): the drop cap reaches down two grid
    // steps, to line 3.
    let (mut d, sid, _) = drop_doc(LOREM, ParaAttrs { grid_align: Some(designcraft_doc::GridAlign::AllLines), ..drop_cap(3, 1) });
    d.settings.baseline_grid.increment = 15.0;
    d.settings.baseline_grid.start = 0.0;
    let cs = compose_story(&d, sid, &ComposeOptions::default());
    let lines = all_lines(&cs);
    assert!((lines[1].baseline - lines[0].baseline - 15.0).abs() < 1e-6);
    let dc = &lines[0].glyphs[0];
    assert!((lines[0].baseline + dc.y - lines[2].baseline).abs() < 1e-6);
    let b = lines[0].drop_cap.unwrap();
    assert!((b.baseline - lines[2].baseline).abs() < 1e-6 && b.rect.y1 > lines[2].baseline);
    // Scaled to the grid's line pitch: its cap top is still line 1's.
    let body = lines[0].glyphs.iter().find(|g| g.byte == 1).unwrap();
    assert!((lines[2].baseline - cap_of(dc) - (lines[0].baseline - cap_of(body))).abs() < 0.01);
}

/// A line with a larger auto leading doesn't move the drop cap: it reaches down `lines − 1`
/// leadings of the text after it, not to where line N is set.
#[test]
fn drop_cap_reaches_down_by_the_text_leading_past_a_line_with_larger_leading() {
    use designcraft_doc::{CharAttrs, Composer, Leading};
    let text = [LOREM; 2].join(" ");
    let para = ParaAttrs { composer: Some(Composer::SingleLine), ..drop_cap(3, 1) };
    let (mut d, sid, _) = doc_with(&text, Rect::new(0.0, 0.0, 300.0, 1000.0), para);
    d.story_mut(sid).unwrap().paras[0].chars = CharAttrs { size: Some(12.0), leading: Some(Leading::Auto), ..Default::default() };
    // A larger word starts line 2, so lines 2 and 3 are set further down than 2 × 14.4 pt.
    let start2 = all_lines(&compose_story(&d, sid, &ComposeOptions::default()))[1].range.start;
    d.story_mut(sid).unwrap().format_chars(start2..start2 + 3, |f| f.over.size = Some(20.0));
    let cs = compose_story(&d, sid, &ComposeOptions::default());
    let lines = all_lines(&cs);
    assert_eq!(lines[1].range.start, start2);
    assert!(lines[2].baseline - lines[0].baseline > 2.0 * 14.4 + 5.0, "{} {}", lines[0].baseline, lines[2].baseline);
    let dc = &lines[0].glyphs[0];
    assert!((dc.y - 2.0 * 14.4).abs() < 1e-6, "drop cap baseline {} below line 1", dc.y);
    let b = lines[0].drop_cap.unwrap();
    assert!((b.baseline - (lines[0].baseline + 2.0 * 14.4)).abs() < 1e-6, "{}", b.baseline);
}

/// A space after the drop cap is set at its size on its baseline and doesn't widen the indent: the
/// lines beside it start where its characters end.
#[test]
fn space_after_a_drop_cap_does_not_widen_the_indent() {
    let (d, sid, _) = drop_doc(&format!("In 2003 {LOREM}"), drop_cap(2, 2));
    let cs = compose_story(&d, sid, &ComposeOptions::default());
    let lines = all_lines(&cs);
    let (i, n) = (&lines[0].glyphs[0], &lines[0].glyphs[1]);
    assert_eq!((i.byte, n.byte), (0, 1));
    let end = n.x + n.adv;
    for l in &lines[..2] {
        assert!((text_x(l, 3) - end).abs() < 1e-6, "{} vs {end}", text_x(l, 3));
    }
    let space = lines[0].glyphs.iter().find(|g| g.byte == 2).unwrap();
    assert!((space.sy - n.sy).abs() < 1e-9 && (space.y - n.y).abs() < 1e-9, "the space is set like the drop cap");
    assert_eq!(lines[0].drop_cap.unwrap().end, 3);
}

/// A one-line drop cap keeps its characters' own size (a smaller verse number stays smaller), and
/// the tabs in it advance to the paragraph's tab stops.
#[test]
fn one_line_drop_cap_keeps_its_size_and_sets_its_tabs() {
    let stop = |position, align| designcraft_doc::TabStop { position, align, leader: String::new(), align_on: String::new() };
    let para = ParaAttrs { tabs: Some(vec![stop(30.0, TabAlign::Right), stop(40.0, TabAlign::Left)]), ..drop_cap(1, 3) };
    let (mut d, sid, _) = drop_doc(&format!("\t7\t{LOREM}"), para);
    d.story_mut(sid).unwrap().format_chars(0..3, |f| f.over.size = Some(8.0));
    let cs = compose_story(&d, sid, &ComposeOptions::default());
    let l = &cs.frames[0].lines[0];
    let at = |b: usize| l.glyphs.iter().find(|g| g.byte == b).unwrap();
    let (seven, body) = (at(1), at(3));
    assert!((seven.sy / body.sy - 0.8).abs() < 1e-9, "{} vs {}", seven.sy, body.sy);
    assert!(seven.y.abs() < 1e-9);
    assert!((seven.x + seven.adv - 30.0).abs() < 1e-6, "the number ends at the right tab: {}", seven.x + seven.adv);
    assert!((text_x(l, 3) - 40.0).abs() < 1e-6, "the text starts at the left tab: {}", text_x(l, 3));
}

/// Right to left, the tabs in a drop cap advance to tab stops measured from the column's right
/// edge.
#[test]
fn right_to_left_drop_cap_sets_its_tabs_from_the_right_edge() {
    let stop = |position, align| designcraft_doc::TabStop { position, align, leader: String::new(), align_on: String::new() };
    let para = ParaAttrs {
        direction: Some(designcraft_doc::TextDirection::RightToLeft),
        align: Some(Align::Right),
        tabs: Some(vec![stop(30.0, TabAlign::Right), stop(40.0, TabAlign::Left)]),
        ..drop_cap(1, 3)
    };
    let (d, sid, _) = drop_doc(&format!("\t7\t{LOREM}"), para);
    let cs = compose_story(&d, sid, &ComposeOptions::default());
    let l = &cs.frames[0].lines[0];
    let seven = l.glyphs.iter().find(|g| g.byte == 1).unwrap();
    // The 300 pt column's right edge is at 300: the number's far (left) end is 30 pt from it.
    assert!((seven.x - 270.0).abs() < 1e-6, "the number ends at the right tab: {}", seven.x);
    // The text beside it starts at the left tab, 40 pt from the edge.
    let right = l.glyphs.iter().filter(|g| g.byte >= 3 && g.visible).map(|g| g.x + g.adv).fold(f64::NEG_INFINITY, f64::max);
    assert!(right <= 260.0 + 1e-6 && right > 255.0, "text ends at {right}");
}

/// The cap's point size scales it, as in InDesign: at the body's size (10 pt) it fills its lines
/// exactly; set larger or smaller it is scaled by its size over the body's, keeping its baseline,
/// so it rises above the first line (or falls short of it). The body's lines don't move, and the
/// cap's box (highlight, caret) reaches the top of a cap that rises above its line.
#[test]
fn drop_cap_scales_with_its_point_size() {
    let set = |size: f64| {
        let (mut d, sid, _) = drop_doc(&[LOREM; 2].join(" "), drop_cap(3, 1));
        d.story_mut(sid).unwrap().format_chars(0..1, |f| f.over.size = Some(size));
        let cs = compose_story(&d, sid, &ComposeOptions::default());
        all_lines(&cs).into_iter().cloned().collect::<Vec<Line>>()
    };
    let base = set(10.0);
    let (b0, b2) = (base[0].baseline, base[2].baseline);
    let body = base[0].glyphs.iter().find(|g| g.byte == 1).unwrap();
    let fitted = cap_of(&base[0].glyphs[0]);
    assert!((b2 - fitted - (b0 - cap_of(body))).abs() < 0.01, "at the body's size the cap fills its lines");
    for (size, scale) in [(15.0, 1.5), (7.5, 0.75)] {
        let l = set(size);
        let dc = &l[0].glyphs[0];
        for (a, b) in l.iter().zip(&base) {
            assert!((a.baseline - b.baseline).abs() < 1e-6, "{size} pt: the lines stay where they were");
        }
        assert!((l[0].baseline + dc.y - b2).abs() < 1e-6, "{size} pt: the cap sits on line 3's baseline");
        assert!((cap_of(dc) - fitted * scale).abs() < 0.01, "{size} pt: cap height {} vs {}", cap_of(dc), fitted * scale);
        let top = (l[0].baseline - l[0].ascent).min(b2 - cap_of(dc));
        assert!((l[0].drop_cap.unwrap().rect.y0 - top).abs() < 1e-6, "{size} pt: the box starts at the line's or the cap's top");
    }
    assert!(b2 - fitted * 1.5 < b0 - base[0].ascent, "a 15 pt cap rises above the first line");
}

/// A rule in Text Color takes the colour of the paragraph's text: the first character's for the
/// rule above, the last character's for the rule below. A swatch colour is used as is.
#[test]
fn text_color_rules_follow_the_text() {
    use designcraft_doc::{CharAttrs, CharFormat, Rule, TEXT_COLOR};
    let rule = Rule { on: true, weight: 2.0, ..Default::default() };
    let para = ParaAttrs { rule_above: Some(rule.clone()), rule_below: Some(rule.clone()), ..Default::default() };
    let (mut d, sid, fid) = doc_with("Red then blue", Rect::new(0.0, 0.0, 300.0, 200.0), para);
    {
        let s = d.story_mut(sid).unwrap();
        s.format_chars(0..3, |f: &mut CharFormat| f.over.merge(&CharAttrs { fill: Some("Red".into()), fill_tint: Some(0.5), ..Default::default() }));
        s.format_chars(9..13, |f: &mut CharFormat| f.over.fill = Some("Blue".into()));
    }
    let decos = |d: &Document| {
        let cs = compose_story(d, sid, &ComposeOptions::default());
        let mut v: Vec<(f64, String, f32)> = cs.frame(fid).unwrap().decos.iter().map(|dc| (dc.rect.y0, dc.color.clone(), dc.tint)).collect();
        v.sort_by(|a, b| a.0.total_cmp(&b.0));
        v.into_iter().map(|(_, c, t)| (c, t)).collect::<Vec<_>>()
    };
    assert_eq!(rule.color, TEXT_COLOR);
    assert_eq!(decos(&d), vec![("Red".to_string(), 0.5), ("Blue".to_string(), 1.0)]);
    let swatch = Rule { color: "[Black]".into(), tint: 0.3, ..rule };
    d.story_mut(sid).unwrap().paras[0].para.rule_above = Some(swatch);
    assert_eq!(decos(&d)[0], ("[Black]".to_string(), 0.3));
}

/// Align Left Edge (InDesign's default): the drop cap's origin moves left by its first letter's
/// left side bearing so its ink starts at the indent, and the lines beside it start where its
/// advance ends. Measured case: 14/18 text, two lines, one character.
#[test]
fn drop_cap_aligns_its_left_edge() {
    let text = format!("Once {LOREM}");
    let compose = |align: Option<bool>| {
        let para = ParaAttrs { left_indent: Some(10.0), drop_cap_align_left: align, ..drop_cap(2, 1) };
        let (mut d, sid, _) = doc_with(&text, Rect::new(0.0, 0.0, 300.0, 1000.0), para);
        d.story_mut(sid).unwrap().paras[0].chars =
            designcraft_doc::CharAttrs { size: Some(14.0), leading: Some(designcraft_doc::Leading::Points(18.0)), ..Default::default() };
        compose_story(&d, sid, &ComposeOptions::default())
    };
    use designcraft_geom::Shape as _;
    let ink_left = |g: &PlacedGlyph| g.x + designcraft_fonts::FontDb::global().outline(g.face.get(), g.gid).bounding_box().x0 * g.sx;
    for align in [None, Some(true), Some(false)] {
        let cs = compose(align);
        let lines = all_lines(&cs);
        let dc = &lines[0].glyphs[0];
        assert_eq!(dc.byte, 0);
        let lsb = ink_left(dc) - dc.x;
        assert!(lsb > 0.5, "the O has a visible side bearing at this size: {lsb}");
        if align != Some(false) {
            assert!((ink_left(dc) - 10.0).abs() < 1e-6, "{align:?}: ink at {}", ink_left(dc));
        } else {
            assert!((dc.x - 10.0).abs() < 1e-6, "origin at {}", dc.x);
        }
        for l in &lines[..2] {
            assert!((text_x(l, 1) - (dc.x + dc.adv)).abs() < 1e-6, "{align:?}: {} vs {}", text_x(l, 1), dc.x + dc.adv);
        }
        assert!((text_x(lines[2], 1) - 10.0).abs() < 1e-6);
    }
}

#[test]
fn caret_follows_spaces_typed_at_the_end() {
    let x_at_end = |text: &str| {
        let (d, sid, _) = doc_with(text, Rect::new(0.0, 0.0, 300.0, 100.0), ParaAttrs::default());
        let cs = compose_story(&d, sid, &ComposeOptions::default());
        let end_x = cs.frames[0].lines[0].end_x;
        (caret(&cs, text.len()).unwrap().1, end_x)
    };
    let (word, word_end) = x_at_end("Word");
    let (one, one_end) = x_at_end("Word ");
    let (two, _) = x_at_end("Word  ");
    assert!(one > word + 1.0 && two > one + 1.0, "{word} {one} {two}");
    // The spaces don't count for the line's own length (alignment, justification).
    assert!((one_end - word_end).abs() < 1e-9);
    // Wrapped lines keep their spaces too: a caret between two spaces at a line end sits after
    // the first one.
    let text = "Lorem ipsum dolor sit amet  consectetur";
    let (d, sid, _) = doc_with(text, Rect::new(0.0, 0.0, 140.0, 100.0), ParaAttrs::default());
    let cs = compose_story(&d, sid, &ComposeOptions::default());
    let l0 = &cs.frames[0].lines[0];
    if l0.range.end == text.find("consectetur").unwrap() {
        let mid = text.find("  ").unwrap() + 1;
        assert!(caret(&cs, mid).unwrap().1 > l0.end_x, "between the trailing spaces");
    }
}

#[test]
fn colour_change_keeps_kerning_and_ligatures_keep_colour() {
    let mut d = Document::new(&NewDocument::default());
    let lid = d.default_layer();
    let (_, sid) = d.add_text_frame(SpreadRef::Doc(0), Rect::new(100.0, 100.0, 400.0, 200.0), lid, "ayay", ParaFormat::default()).unwrap();
    d.story_mut(sid).unwrap().format_chars(0..4, |f| f.over.font_style = Some("Bold".into()));
    let line = |d: &Document| compose_story(d, sid, &ComposeOptions::default()).frames[0].lines[0].clone();
    let xs = |l: &Line| l.glyphs.iter().map(|g| (g.x, g.adv)).collect::<Vec<_>>();
    let kerned = line(&d);
    // The font kerns "ay" (otherwise this test proves nothing).
    d.story_mut(sid).unwrap().format_chars(0..4, |f| f.over.kerning = Some(designcraft_doc::Kerning::None));
    assert_ne!(xs(&line(&d)), xs(&kerned), "Source Serif 4 Bold kerns a–y");
    d.story_mut(sid).unwrap().format_chars(0..4, |f| f.over.kerning = Some(designcraft_doc::Kerning::Metrics));
    // Colour the first "a": positions are unchanged, only its style differs.
    d.story_mut(sid).unwrap().format_chars(0..1, |f| f.over.fill = Some("Cyan".into()));
    let coloured = line(&d);
    assert_eq!(xs(&coloured), xs(&kerned), "a colour change must not change spacing");
    let cs = compose_story(&d, sid, &ComposeOptions::default());
    let fill = |l: &Line, i: usize| cs.styles[l.glyphs[i].style as usize].fill.clone();
    assert_eq!(fill(&coloured, 0), "Cyan");
    assert_ne!(fill(&coloured, 1), "Cyan");

    // A ligature never swallows a differently coloured letter: the "i" of "fi" keeps its colour.
    let (_, sid) = d.add_text_frame(SpreadRef::Doc(0), Rect::new(100.0, 300.0, 400.0, 400.0), lid, "fifi", ParaFormat::default()).unwrap();
    d.story_mut(sid).unwrap().format_chars(1..2, |f| f.over.fill = Some("Cyan".into()));
    let cs = compose_story(&d, sid, &ComposeOptions::default());
    let l = &cs.frames[0].lines[0];
    let at = |b: usize| l.glyphs.iter().find(|g| g.len > 0 && g.byte <= b && b < g.byte + g.len).map(|g| cs.styles[g.style as usize].fill.clone());
    assert_eq!(at(1).as_deref(), Some("Cyan"));
    assert_ne!(at(2).as_deref(), Some("Cyan"));
}

/// The emphasis mark drawn over the text only paints: a change of kenten character keeps the
/// kerning across it.
#[test]
fn kenten_character_change_keeps_kerning() {
    let mut d = Document::new(&NewDocument::default());
    let lid = d.default_layer();
    let (_, sid) = d.add_text_frame(SpreadRef::Doc(0), Rect::new(100.0, 100.0, 400.0, 200.0), lid, "ayay", ParaFormat::default()).unwrap();
    d.story_mut(sid).unwrap().format_chars(0..4, |f| {
        f.over.font_style = Some("Bold".into());
        f.over.kenten = Some(true);
    });
    let xs = |d: &Document| {
        let cs = compose_story(d, sid, &ComposeOptions::default());
        cs.frames[0].lines[0].glyphs.iter().filter(|g| g.len > 0).map(|g| (g.byte, g.x, g.adv)).collect::<Vec<_>>()
    };
    let kerned = xs(&d);
    d.story_mut(sid).unwrap().format_chars(0..4, |f| f.over.kerning = Some(designcraft_doc::Kerning::None));
    assert_ne!(xs(&d), kerned, "Source Serif 4 Bold kerns a–y");
    d.story_mut(sid).unwrap().format_chars(0..4, |f| f.over.kerning = Some(designcraft_doc::Kerning::Metrics));
    d.story_mut(sid).unwrap().format_chars(0..1, |f| f.over.kenten_character = Some("●".into()));
    assert_eq!(xs(&d), kerned, "a kenten character change must not change spacing");
    let cs = compose_story(&d, sid, &ComposeOptions::default());
    let mark = |b: usize| {
        cs.frames[0].lines[0]
            .glyphs
            .iter()
            .find(|g| g.len > 0 && g.byte == b)
            .and_then(|g| cs.styles[g.style as usize].kenten_mark.as_ref().map(|m| m.text.clone()))
    };
    // The default kind (sesame dot) draws a stored character, else its own mark.
    assert_eq!((mark(0).as_deref(), mark(1).as_deref()), (Some("●"), Some("\u{FE45}")));
}

#[test]
fn empty_paragraph_takes_its_marks_format() {
    // Paragraph style text is 12 pt (auto leading 14.4); every character, including the empty
    // paragraph's mark, is 6 pt with 8 pt leading: the empty line must advance 8, not 14.4.
    let text = "One\n\nTwo";
    let (mut d, sid, _) = doc_with(text, Rect::new(0.0, 0.0, 200.0, 200.0), ParaAttrs::default());
    d.story_mut(sid).unwrap().format_chars(0..text.len(), |f| {
        f.over.size = Some(6.0);
        f.over.leading = Some(designcraft_doc::Leading::Points(8.0));
    });
    let cs = compose_story(&d, sid, &ComposeOptions::default());
    let lines = all_lines(&cs);
    assert_eq!(lines.len(), 3);
    assert!((lines[1].baseline - lines[0].baseline - 8.0).abs() < 1e-6, "{} {}", lines[0].baseline, lines[1].baseline);
    assert!((lines[2].baseline - lines[1].baseline - 8.0).abs() < 1e-6, "{} {}", lines[1].baseline, lines[2].baseline);
}

#[test]
fn empty_paragraph_uses_its_own_mark_not_the_previous_one() {
    // "One\n" is 20 pt; the empty paragraph's mark (byte 4) and "Two" are 6 pt with 8 pt leading.
    // The format run boundary sits exactly at the empty paragraph's start: its line must take the
    // 8 pt leading of its own mark, not the previous paragraph's 20 pt (auto 24) mark.
    let text = "One\n\nTwo";
    let (mut d, sid, _) = doc_with(text, Rect::new(0.0, 0.0, 200.0, 200.0), ParaAttrs::default());
    let st = d.story_mut(sid).unwrap();
    st.format_chars(0..4, |f| f.over.size = Some(20.0));
    st.format_chars(4..text.len(), |f| {
        f.over.size = Some(6.0);
        f.over.leading = Some(designcraft_doc::Leading::Points(8.0));
    });
    let cs = compose_story(&d, sid, &ComposeOptions::default());
    let lines = all_lines(&cs);
    assert_eq!(lines.len(), 3);
    assert!((lines[1].baseline - lines[0].baseline - 8.0).abs() < 1e-6, "{} {}", lines[0].baseline, lines[1].baseline);
}

#[test]
fn spanish_text_gets_spanish_hyphenation() {
    let text = "La transición democrática fue una construcción colectiva extraordinariamente compleja y desesperadamente necesaria.";
    let narrow = Rect::new(36.0, 36.0, 120.0, 700.0);
    let (mut d, sid, _) = doc_with(text, narrow, ParaAttrs::default());
    let st = d.story_mut(sid).unwrap();
    let n = st.len();
    st.format_chars(0..n, |f| f.over.language = Some("Spanish".into()));
    let cs = compose_story(&d, sid, &ComposeOptions::default());
    let lines = all_lines(&cs);
    let hy: Vec<&str> = lines.iter().filter(|l| l.hyphenated).map(|l| text[l.range.clone()].trim_end()).collect();
    assert!(!hy.is_empty(), "Spanish hyphenates");
    for l in &hy {
        let last: String = l.chars().rev().take_while(|c| c.is_alphabetic()).collect::<Vec<_>>().into_iter().rev().collect();
        let whole = text[text.find(l).unwrap() + l.len() - last.len()..].split(|c: char| !c.is_alphabetic()).next().unwrap();
        let es = hyphen::Lang::for_language("Spanish").unwrap();
        let pts = hyphen::hyphen_points_in(whole, &hyphen::Limits::default(), es);
        assert!(pts.contains(&last.chars().count()), "break {last}- in {whole} is a Bezos point");
    }
}

#[test]
fn portuguese_text_gets_portuguese_hyphenation() {
    let text = "matemática matemática matemática matemática matemática matemática";
    let (mut d, sid, _) = doc_with(text, Rect::new(36.0, 36.0, 76.0, 700.0), ParaAttrs::default());
    let st = d.story_mut(sid).unwrap();
    let n = st.len();
    st.format_chars(0..n, |f| f.over.language = Some("Portuguese: Brazilian".into()));
    let cs = compose_story(&d, sid, &ComposeOptions::default());
    let breaks: Vec<&str> = all_lines(&cs).into_iter().filter(|l| l.hyphenated).map(|l| text[l.range.clone()].trim_end()).collect();
    assert!(!breaks.is_empty(), "Portuguese text is hyphenated");
    // Portuguese syllables: ma-te-má-tica.
    for b in breaks {
        let tail = b.rsplit(' ').next().unwrap_or(b);
        assert!(["ma", "mate", "matemá"].contains(&tail), "{b:?}");
    }
}

#[test]
fn norwegian_text_gets_norwegian_hyphenation() {
    let text = "sirkulasjonen beveger nedsynkningen kaller overflaten bilde";
    let (mut d, sid, _) = doc_with(text, Rect::new(36.0, 36.0, 76.0, 700.0), ParaAttrs::default());
    let st = d.story_mut(sid).unwrap();
    let n = st.len();
    st.format_chars(0..n, |f| f.over.language = Some("Norwegian: Bokmål".into()));
    let cs = compose_story(&d, sid, &ComposeOptions::default());
    let breaks: Vec<&str> = all_lines(&cs).into_iter().filter(|l| l.hyphenated).map(|l| text[l.range.clone()].trim_end()).collect();
    assert!(!breaks.is_empty(), "Norwegian text is hyphenated");
    for b in breaks {
        let tail = b.rsplit(' ').next().unwrap_or(b);
        let whole = text[text.find(b).unwrap() + b.len() - tail.len()..].split(|c: char| !c.is_alphabetic()).next().unwrap_or("");
        let nb = hyphen::Lang::for_language("Norwegian: Bokmål").unwrap();
        let pts = hyphen::hyphen_points_in(whole, &hyphen::Limits::default(), nb);
        assert!(pts.contains(&tail.chars().count()), "break {tail}- in {whole} is a hyph-no point");
    }
}

#[test]
fn cjk_aki_adds_space_without_scaling_outlines() {
    let (mut d, sid, _) = doc_with("AB", Rect::new(0.0, 0.0, 300.0, 100.0), ParaAttrs::default());
    let old = compose_story(&d, sid, &ComposeOptions::default());
    d.story_mut(sid).unwrap().format_chars(0..2, |f| {
        f.over.leading_aki = Some(Some(0.25));
        f.over.trailing_aki = Some(Some(0.5));
    });
    let new = compose_story(&d, sid, &ComposeOptions::default());
    let a = &old.frames[0].lines[0];
    let b = &new.frames[0].lines[0];
    assert!((b.end_x - a.end_x - 18.0).abs() < 1e-6);
    assert_eq!(a.glyphs[0].sx, b.glyphs[0].sx);
    assert!((b.glyphs[0].x - a.glyphs[0].x - 3.0).abs() < 1e-6);
}

#[test]
fn cjk_jidori_sets_group_width_and_survives_narrow_frames() {
    for composer in [designcraft_doc::Composer::SingleLine, designcraft_doc::Composer::Paragraph] {
        let (mut d, sid, _) = doc_with(
            "AB",
            Rect::new(0.0, 0.0, 30.0, 150.0),
            ParaAttrs { composer: Some(composer), glyph_scale_desired: Some(1.2), letter_space_desired: Some(0.2), ..Default::default() },
        );
        d.story_mut(sid).unwrap().format_chars(0..2, |f| f.over.jidori = Some(4));
        let cs = compose_story(&d, sid, &ComposeOptions::default());
        let lines = all_lines(&cs);
        assert_eq!(lines.len(), 1, "an indivisible jidori group must not be emergency-split");
        assert!((lines[0].end_x - lines[0].x0 - 48.0).abs() < 1e-6);
        assert_eq!(lines[0].range, 0..2);
    }
}

#[test]
fn cjk_tsume_removes_sidebearings_without_collapsing_ink() {
    let (mut d, sid, _) = doc_with("HH", Rect::new(0.0, 0.0, 300.0, 100.0), ParaAttrs::default());
    let old = compose_story(&d, sid, &ComposeOptions::default());
    d.story_mut(sid).unwrap().format_chars(0..2, |f| f.over.tsume = Some(1.0));
    let new = compose_story(&d, sid, &ComposeOptions::default());
    let a = &old.frames[0].lines[0];
    let b = &new.frames[0].lines[0];
    let g = &b.glyphs[0];
    let outline = designcraft_fonts::FontDb::global().outline(&g.face, g.gid);
    let ink_width = designcraft_geom::Shape::bounding_box(&*outline).width() * g.sx;
    assert!(ink_width > 0.0);
    assert!((b.end_x - b.x0 - 2.0 * ink_width).abs() < 1e-6);
    assert!(b.end_x < a.end_x);
    assert_eq!(a.glyphs[0].sx, g.sx);
}

#[test]
fn cjk_composite_font_selects_entries_before_shaping() {
    use designcraft_doc::cjk::{CompositeFont, CompositeFontEntry};
    let (mut d, sid, _) = doc_with("A1B", Rect::new(0.0, 0.0, 300.0, 100.0), ParaAttrs::default());
    std::sync::Arc::make_mut(&mut d.styles).composite_fonts.push(CompositeFont {
        name: "Mixed".into(),
        entries: vec![
            CompositeFontEntry { family: "Source Serif 4".into(), ..Default::default() },
            CompositeFontEntry { characters: "0123456789".into(), family: "Source Sans 3".into(), relative_size: 0.5, ..Default::default() },
        ],
    });
    d.story_mut(sid).unwrap().format_chars(0..3, |f| f.over.font_family = Some("CompositeFont/Mixed".into()));
    let cs = compose_story(&d, sid, &ComposeOptions::default());
    let line = &cs.frames[0].lines[0];
    let a = line.glyphs.iter().find(|g| g.byte == 0).unwrap();
    let n = line.glyphs.iter().find(|g| g.byte == 1).unwrap();
    assert_ne!(a.face.id(), n.face.id());
    assert_eq!(cs.styles[a.style as usize].size, 12.0);
    assert_eq!(cs.styles[n.style as usize].size, 6.0);
    assert!(!cs.styles.iter().any(|s| s.missing_font));
}

#[test]
fn cjk_bunri_does_not_split_double_dash() {
    for composer in [designcraft_doc::Composer::SingleLine, designcraft_doc::Composer::Paragraph] {
        let (d, sid, _) =
            doc_with("--", Rect::new(0.0, 0.0, 5.0, 100.0), ParaAttrs { composer: Some(composer), bunri_kinshi: Some(true), ..Default::default() });
        let cs = compose_story(&d, sid, &ComposeOptions::default());
        assert_eq!(all_lines(&cs).len(), 1);
    }
}

#[test]
fn cjk_custom_kinsoku_disables_selected_breaks() {
    let (mut d, sid, _) = doc_with(
        "甲乙丙丁",
        Rect::new(0.0, 0.0, 25.0, 200.0),
        ParaAttrs {
            composer: Some(designcraft_doc::Composer::SingleLine),
            kinsoku: Some(Some(designcraft_doc::cjk::Kinsoku { name: "Test".into(), no_start: "乙丙丁".into(), ..Default::default() })),
            ..Default::default()
        },
    );
    d.story_mut(sid).unwrap().format_chars(0.."甲乙丙丁".len(), |f| f.over.size = Some(12.0));
    let cs = compose_story(&d, sid, &ComposeOptions::default());
    assert_eq!(all_lines(&cs).len(), 1);
}

fn japanese() -> ParaAttrs {
    ParaAttrs { composer: Some(Composer::Japanese), ..Default::default() }
}

#[test]
fn cjk_center_leading_measures_em_centers_across_different_sizes() {
    let lines_with = |para: ParaAttrs| {
        let (mut d, sid, _) = doc_with("A\nB", Rect::new(0.0, 0.0, 300.0, 200.0), para);
        d.story_mut(sid).unwrap().format_chars(0..3, |f| {
            f.over.leading = Some(designcraft_doc::Leading::Points(30.0));
            f.over.leading_model = Some(designcraft_doc::cjk::LeadingModel::Center);
        });
        d.story_mut(sid).unwrap().format_chars(0..1, |f| f.over.size = Some(24.0));
        let cs = compose_story(&d, sid, &ComposeOptions::default());
        all_lines(&cs).into_iter().cloned().collect::<Vec<_>>()
    };
    let lines = lines_with(japanese());
    assert_eq!(lines.len(), 2);
    let center = |l: &Line| {
        let g = &l.glyphs[0];
        let (a, b) = g.face.vertical_metrics();
        l.baseline + (b - a) / (2.0 * (a + b)) * g.face.units_per_em() * g.sy
    };
    assert!((center(&lines[1]) - center(&lines[0]) - 30.0).abs() < 1e-6);
    assert!((lines[1].baseline - lines[0].baseline - 30.0).abs() > 0.1);
    // The other composers set lines baseline to baseline.
    let lines = lines_with(ParaAttrs::default());
    assert!((lines[1].baseline - lines[0].baseline - 30.0).abs() < 1e-6);
}

#[test]
fn cjk_aki_below_sets_the_upper_lines_leading_below_it() {
    let lines_with = |para: ParaAttrs| {
        let (mut d, sid, _) = doc_with("A\nB", Rect::new(0.0, 0.0, 300.0, 200.0), para);
        let st = d.story_mut(sid).unwrap();
        st.format_chars(0..3, |f| f.over.leading_model = Some(designcraft_doc::cjk::LeadingModel::AkiBelow));
        st.format_chars(0..2, |f| {
            f.over.size = Some(24.0);
            f.over.leading = Some(designcraft_doc::Leading::Points(40.0));
        });
        st.format_chars(2..3, |f| {
            f.over.size = Some(12.0);
            f.over.leading = Some(designcraft_doc::Leading::Points(20.0));
        });
        let cs = compose_story(&d, sid, &ComposeOptions::default());
        all_lines(&cs).into_iter().cloned().collect::<Vec<_>>()
    };
    let lines = lines_with(japanese());
    assert_eq!(lines.len(), 2);
    // Aki Below measures from the em box tops: the lower line's sits the upper line's leading
    // below the upper line's.
    let em_top = |l: &Line| {
        let g = &l.glyphs[0];
        let (a, b) = g.face.vertical_metrics();
        l.baseline - a / (a + b) * g.face.units_per_em() * g.sy
    };
    let gap = em_top(&lines[1]) - em_top(&lines[0]);
    assert!((gap - 40.0).abs() < 1e-6, "{gap}");
    // The other composers advance by the lower line's leading, baseline to baseline.
    let lines = lines_with(ParaAttrs::default());
    assert!((lines[1].baseline - lines[0].baseline - 20.0).abs() < 1e-6);
}

#[test]
fn cjk_aki_below_measures_em_tops_and_aki_above_em_bottoms() {
    use designcraft_doc::cjk::LeadingModel;
    for (model, top) in [(LeadingModel::AkiBelow, true), (LeadingModel::AkiAbove, false)] {
        let (mut d, sid, _) = doc_with("A\nB", Rect::new(0.0, 0.0, 300.0, 200.0), japanese());
        d.story_mut(sid).unwrap().format_chars(0..3, |f| {
            f.over.leading = Some(designcraft_doc::Leading::Points(30.0));
            f.over.leading_model = Some(model);
        });
        d.story_mut(sid).unwrap().format_chars(0..1, |f| f.over.size = Some(24.0));
        let cs = compose_story(&d, sid, &ComposeOptions::default());
        let lines = all_lines(&cs);
        assert_eq!(lines.len(), 2);
        // The em box edge below the baseline (y down): its top, or its bottom one em lower.
        let edge = |l: &Line| {
            let g = &l.glyphs[0];
            let (a, b) = g.face.vertical_metrics();
            let em = g.face.units_per_em() * g.sy;
            let em_top = l.baseline - em * a / (a + b);
            if top { em_top } else { em_top + em }
        };
        assert!((edge(lines[1]) - edge(lines[0]) - 30.0).abs() < 1e-6, "{model:?}");
    }
}

#[test]
fn cjk_em_center_alignment_moves_small_characters() {
    let line_with = |para: ParaAttrs| {
        let (mut d, sid, _) = doc_with("AB", Rect::new(0.0, 0.0, 300.0, 100.0), para);
        d.story_mut(sid).unwrap().format_chars(0..1, |f| f.over.size = Some(24.0));
        d.story_mut(sid).unwrap().format_chars(0..2, |f| f.over.character_alignment = Some(designcraft_doc::cjk::CharacterAlignment::EmCenter));
        compose_story(&d, sid, &ComposeOptions::default()).frames[0].lines[0].clone()
    };
    let line = line_with(japanese());
    // Em box centre (y down): the font's BASE em box, else 0.88/-0.12 em.
    let center = |g: &PlacedGlyph| {
        let upem = g.face.units_per_em();
        let (top, bottom) = g.face.declared_em_box().unwrap_or((upem * 0.88, upem * -0.12));
        g.y - (top + bottom) / 2.0 * g.sy
    };
    assert!((center(&line.glyphs[0]) - center(&line.glyphs[1])).abs() < 1e-6);
    // The other composers keep every character on the baseline.
    let line = line_with(ParaAttrs::default());
    assert!(line.glyphs.iter().all(|g| g.y.abs() < 1e-9), "{:?}", line.glyphs.iter().map(|g| g.y).collect::<Vec<_>>());
}

#[test]
fn smaller_cjk_run_aligns_to_the_line_em_box_centre() {
    use designcraft_doc::cjk::CharacterAlignment;
    let text = "Ab 甲乙 cd";
    let run = "Ab ".len().."Ab 甲乙".len();
    // Em box centre (y down) of a placed glyph: its font's BASE em box, else 0.88/-0.12 em.
    let centre = |l: &Line, g: &PlacedGlyph| {
        let upem = g.face.units_per_em();
        let (top, bottom) = g.face.declared_em_box().unwrap_or((upem * 0.88, upem * -0.12));
        l.baseline + g.y - (top + bottom) / 2.0 * g.sy
    };
    for alignment in [CharacterAlignment::EmCenter, CharacterAlignment::Baseline] {
        let (mut d, sid, _) = doc_with(text, Rect::new(0.0, 0.0, 300.0, 100.0), japanese());
        d.story_mut(sid).unwrap().format_chars(0..text.len(), |f| {
            f.over.size = Some(10.0);
            f.over.character_alignment = Some(alignment);
        });
        d.story_mut(sid).unwrap().format_chars(run.clone(), |f| f.over.size = Some(6.0));
        let cs = compose_story(&d, sid, &ComposeOptions::default());
        let line = &cs.frames[0].lines[0];
        let latin = &line.glyphs[0];
        let cjk: Vec<&PlacedGlyph> = line.glyphs.iter().filter(|g| run.contains(&g.byte)).collect();
        assert_eq!(cjk.len(), 2);
        assert!(latin.y.abs() < 1e-9);
        for g in cjk {
            match alignment {
                CharacterAlignment::EmCenter => {
                    assert!((centre(line, g) - centre(line, latin)).abs() < 0.01, "{} vs {}", centre(line, g), centre(line, latin));
                    assert!(g.y < -1.0, "a 6 pt run rises in a 10 pt line: {}", g.y);
                }
                _ => assert!(g.y.abs() < 1e-9, "Roman baseline keeps the run on the baseline: {}", g.y),
            }
        }
    }
}

#[test]
fn cjk_hanging_punctuation_uses_frame_edge_without_changing_glyph_size() {
    let k = designcraft_doc::cjk::Kinsoku { name: "Test".into(), no_start: ",".into(), hanging: ",".into(), ..Default::default() };
    let (mut d, sid, fid) = doc_with(
        "AB,",
        Rect::new(0.0, 0.0, 300.0, 100.0),
        ParaAttrs {
            composer: Some(designcraft_doc::Composer::SingleLine),
            kinsoku: Some(Some(k)),
            kinsoku_hang: Some(designcraft_doc::cjk::KinsokuHang::Regular),
            ..Default::default()
        },
    );
    let initial = compose_story(&d, sid, &ComposeOptions::default());
    let line = &initial.frames[0].lines[0];
    let text_w = line.glyphs.iter().take(2).map(|g| g.adv).sum::<f64>();
    // A fresh frame whose measure fits the letters, but not the comma.
    let frame = d.item_mut(fid).unwrap();
    frame.path = designcraft_geom::shapes::rectangle(Rect::new(0.0, 0.0, text_w + 1.0, 100.0));
    let cs = compose_story(&d, sid, &ComposeOptions::default());
    assert_eq!(all_lines(&cs).len(), 1);
    assert_eq!(cs.frames[0].lines[0].glyphs.last().unwrap().sx, line.glyphs.last().unwrap().sx);
}

#[test]
fn cjk_fullwidth_space_obeys_the_paragraph_spacing_switch() {
    let text = "A　B";
    let (mut d, sid, _) = doc_with(
        text,
        Rect::new(0.0, 0.0, 300.0, 100.0),
        ParaAttrs { word_space_desired: Some(2.0), treat_ideographic_space_as_space: Some(false), ..Default::default() },
    );
    let fixed = compose_story(&d, sid, &ComposeOptions::default());
    d.story_mut(sid).unwrap().paras[0].para.treat_ideographic_space_as_space = Some(true);
    let elastic = compose_story(&d, sid, &ComposeOptions::default());
    let space = fixed.frames[0].lines[0].glyphs.iter().find(|g| g.byte == 1).unwrap().adv;
    assert!((elastic.frames[0].lines[0].end_x - fixed.frames[0].lines[0].end_x - space).abs() < 1e-6);
}

#[test]
fn arabic_bidi_keeps_paragraph_context_across_forced_lines() {
    let text = "مرحبا\u{2028}123 - 456";
    let (d, sid, _) = doc_with(text, Rect::new(0.0, 0.0, 600.0, 200.0), ParaAttrs::default());
    let cs = compose_story(&d, sid, &ComposeOptions::default());
    let lines = all_lines(&cs);
    assert_eq!(lines.len(), 2);
    let mut gs: Vec<_> = lines[1].glyphs.iter().filter(|g| g.len > 0).collect();
    gs.sort_by(|a, b| a.x.total_cmp(&b.x));
    let actual: String = gs.iter().map(|g| text[g.byte..].chars().next().unwrap()).collect();
    let info = unicode_bidi::BidiInfo::new(text, Some(unicode_bidi::Level::ltr()));
    let start = text.find('1').unwrap();
    let expected = info.reorder_line(&info.paragraphs[0], start..text.len()).to_string();
    assert_eq!(actual, expected);
    assert_ne!(actual, "123 - 456", "the previous Arabic strong character affects number ordering");
}

#[test]
fn arabic_character_direction_override_survives_line_layout() {
    let text = "abc 123 def";
    let (mut d, sid, _) = doc_with(text, Rect::new(0.0, 0.0, 600.0, 200.0), ParaAttrs::default());
    d.story_mut(sid).unwrap().format_chars(4..7, |f| f.over.character_direction = Some(designcraft_doc::arabic::CharacterDirection::RightToLeft));
    let cs = compose_story(&d, sid, &ComposeOptions::default());
    let line = all_lines(&cs)[0];
    let x = |byte| line.glyphs.iter().find(|g| g.byte == byte).unwrap().x;
    assert!(x(4) > x(5) && x(5) > x(6));
    assert!(x(0) < x(6) && x(4) < x(8));
}

#[test]
fn arabic_fallback_marks_stay_with_bases_and_custom_offsets_move_only_marks() {
    let db = designcraft_fonts::FontDb::global();
    let Some(face) = db.fallback_for('ب', db.face(designcraft_fonts::DEFAULT_FAMILY, "Regular").id(), None) else { return };
    if !face.covers('ُ') {
        return;
    }
    let text = "بُبَ";
    let (mut d, sid, _) = doc_with(text, Rect::new(0.0, 0.0, 600.0, 200.0), ParaAttrs::default());
    d.settings.glyph_fallback = true;
    let plain = compose_story(&d, sid, &ComposeOptions::default()).frames[0].lines[0].glyphs.clone();
    assert!(plain.iter().all(|g| g.face.id() == face.id()), "fallback must not split Arabic marks into the Latin face");
    assert!(plain.iter().all(|g| g.gid != 0));
    d.story_mut(sid).unwrap().format_chars(0..text.len(), |f| {
        f.over.diacritic_x_offset = Some(200.0);
        f.over.diacritic_y_offset = Some(-100.0);
    });
    let shifted = compose_story(&d, sid, &ComposeOptions::default()).frames[0].lines[0].glyphs.clone();
    let mut marks = 0;
    for (a, b) in plain.iter().zip(&shifted) {
        assert_eq!(a.gid, b.gid);
        assert_eq!(a.adv, b.adv);
        if face.glyph_is_mark(a.gid) {
            marks += 1;
            assert!((b.x - a.x - 2.4).abs() < 1e-6);
            assert!((b.y - a.y - 1.2).abs() < 1e-6);
        } else {
            assert!((a.x - b.x).abs() < 1e-6 && (a.y - b.y).abs() < 1e-6);
        }
    }
    assert!(marks > 0);
}

#[test]
fn arabic_joining_context_crosses_character_style_boundaries() {
    let text = "ببب";
    let (mut d, sid, _) = doc_with(text, Rect::new(0.0, 0.0, 600.0, 200.0), ParaAttrs::default());
    d.settings.glyph_fallback = true;
    let plain = compose_story(&d, sid, &ComposeOptions::default()).frames[0].lines[0].glyphs.clone();
    if plain.iter().any(|g| g.gid == 0) {
        return;
    }
    d.story_mut(sid).unwrap().format_chars(2..4, |f| f.over.fill = Some("Paper".into()));
    let split = compose_story(&d, sid, &ComposeOptions::default()).frames[0].lines[0].glyphs.clone();
    assert_eq!(plain.iter().map(|g| (g.byte, g.gid)).collect::<Vec<_>>(), split.iter().map(|g| (g.byte, g.gid)).collect::<Vec<_>>());
}

#[test]
fn arabic_character_kashida_switch_prevents_insertion() {
    let text = "بسم الله الرحمن الرحيم الحمد لله رب العالمين";
    let (mut d, sid, _) = doc_with(
        text,
        Rect::new(0.0, 0.0, 160.0, 300.0),
        ParaAttrs { direction: Some(designcraft_doc::TextDirection::RightToLeft), align: Some(Align::RightJustified), ..Default::default() },
    );
    d.settings.glyph_fallback = true;
    d.story_mut(sid).unwrap().format_chars(0..text.len(), |f| f.over.allow_kashidas = Some(false));
    let off = compose_story(&d, sid, &ComposeOptions::default());
    assert!(all_lines(&off).iter().all(|l| l.glyphs.iter().all(|g| g.len > 0 || g.face.glyph_for('\u{0640}') != g.gid)));
}

#[test]
fn arabic_indic_digit_conversion_retains_original_utf8_ranges() {
    let text = "١٢٣";
    let (mut d, sid, _) = doc_with(text, Rect::new(0.0, 0.0, 200.0, 100.0), ParaAttrs::default());
    d.story_mut(sid).unwrap().format_chars(0..text.len(), |f| f.over.digits = Some(designcraft_doc::Digits::Arabic));
    let cs = compose_story(&d, sid, &ComposeOptions::default());
    let gs = &cs.frames[0].lines[0].glyphs;
    assert_eq!(gs.iter().map(|g| (g.byte, g.len)).collect::<Vec<_>>(), [(0, 2), (2, 2), (4, 2)]);
    for (g, c) in gs.iter().zip("123".chars()) {
        assert_eq!(g.gid, g.face.glyph_for(c));
    }
}

#[test]
fn arabic_contextual_digits_follow_strong_text_across_style_changes() {
    let text = "ب 12 A 34";
    let (mut d, sid, _) = doc_with(text, Rect::new(0.0, 0.0, 300.0, 100.0), ParaAttrs::default());
    d.settings.glyph_fallback = true;
    let arabic_digits = text.find('1').unwrap();
    d.story_mut(sid).unwrap().format_chars(arabic_digits..arabic_digits + 2, |f| f.over.fill = Some("Paper".into()));
    let cs = compose_story(&d, sid, &ComposeOptions::default());
    let gs = &cs.frames[0].lines[0].glyphs;
    let g = gs.iter().find(|g| g.byte == arabic_digits).unwrap();
    assert_eq!(g.gid, g.face.glyph_for('١'));
    let g = gs.iter().find(|g| g.byte == text.find('3').unwrap()).unwrap();
    assert_eq!(g.gid, g.face.glyph_for('3'));
}

#[test]
fn arabic_story_direction_flows_right_column_first() {
    let text = format!("first{}second", designcraft_doc::story::COLUMN_BREAK);
    let (mut d, sid, fid) = doc_with(&text, Rect::new(0.0, 0.0, 300.0, 200.0), ParaAttrs::default());
    d.story_mut(sid).unwrap().direction = designcraft_doc::TextDirection::RightToLeft;
    d.item_mut(fid).unwrap().text_frame_mut().unwrap().options.columns = 2;
    let cs = compose_story(&d, sid, &ComposeOptions::default());
    let lines = all_lines(&cs);
    assert_eq!(lines.len(), 2);
    assert!(lines[0].x0 > lines[1].x0);
    assert!(cs.frames[0].columns[0].x0 > cs.frames[0].columns[1].x0);
}

#[test]
fn arabic_rtl_table_keeps_logical_indices_and_merged_cell_hit_testing() {
    let mut t = designcraft_doc::Table::new(987, 2, 3, 0, 0, 240.0);
    t.options.direction = designcraft_doc::TextDirection::RightToLeft;
    t.cell_mut(0, 0).unwrap().text = designcraft_doc::Story::with_text(StoryId(0), "Right", ParaFormat::default());
    t.cell_mut(0, 2).unwrap().text = designcraft_doc::Story::with_text(StoryId(0), "Left", ParaFormat::default());
    t.merge(designcraft_doc::CellRange::new(1, 0, 1, 1)).unwrap();
    let (d, sid, _) = table_doc(Rect::new(0.0, 0.0, 400.0, 400.0), t);
    let cs = compose_story(&d, sid, &ComposeOptions::default());
    let (_, _, right) = table::find_cell(&cs, 987, 0, 0).unwrap();
    let (_, _, left) = table::find_cell(&cs, 987, 0, 2).unwrap();
    assert!(right.rect.x0 > left.rect.x0);
    let (_, _, merged) = table::find_cell(&cs, 987, 1, 0).unwrap();
    assert!((merged.rect.width() - 160.0).abs() < 1e-6);
    assert_eq!(table::hit_cell(&cs, 0, right.rect.center()).map(|(_, r, c, _)| (r, c)), Some((0, 0)));
}

#[test]
fn arabic_kashida_moves_marks_with_their_cluster_and_respects_nonjoiners() {
    let text = "بُسْمِ بُسْمِ";
    let (mut d, sid, _) = doc_with(
        text,
        Rect::new(0.0, 0.0, 180.0, 100.0),
        ParaAttrs { direction: Some(designcraft_doc::TextDirection::RightToLeft), align: Some(Align::FullyJustified), ..Default::default() },
    );
    d.settings.glyph_fallback = true;
    let with = compose_story(&d, sid, &ComposeOptions::default()).frames[0].lines[0].glyphs.clone();
    if with.iter().any(|g| g.gid == 0) {
        return;
    }
    assert!(with.iter().any(|g| g.len == 0 && g.gid == g.face.glyph_for('\u{0640}')));
    d.story_mut(sid).unwrap().paras[0].para.kashidas = Some(false);
    let without = compose_story(&d, sid, &ComposeOptions::default()).frames[0].lines[0].glyphs.clone();
    for (a, b) in with.iter().zip(&without) {
        assert_eq!((a.gid, a.byte), (b.gid, b.byte));
        let first_a = with.iter().find(|g| g.byte == a.byte).unwrap();
        let first_b = without.iter().find(|g| g.byte == b.byte).unwrap();
        assert!(((a.x - first_a.x) - (b.x - first_b.x)).abs() < 1e-6, "marks detached from their cluster");
    }
    let text = "ب\u{200C}ب ب\u{200C}ب";
    let (mut d, sid, _) = doc_with(
        text,
        Rect::new(0.0, 0.0, 180.0, 100.0),
        ParaAttrs { direction: Some(designcraft_doc::TextDirection::RightToLeft), align: Some(Align::FullyJustified), ..Default::default() },
    );
    d.settings.glyph_fallback = true;
    let cs = compose_story(&d, sid, &ComposeOptions::default());
    assert!(cs.frames[0].lines[0].glyphs.iter().all(|g| g.gid != g.face.glyph_for('\u{0640}') || g.len > 0));
}

#[test]
fn arabic_explicit_isolated_forms_disable_contextual_joining() {
    let text = "بب";
    let (mut d, sid, _) = doc_with(text, Rect::new(0.0, 0.0, 180.0, 100.0), ParaAttrs::default());
    d.settings.glyph_fallback = true;
    d.story_mut(sid).unwrap().format_chars(0..text.len(), |f| f.over.positional_form = Some("Isolated".into()));
    let cs = compose_story(&d, sid, &ComposeOptions::default());
    let gs = &cs.frames[0].lines[0].glyphs;
    for g in gs {
        assert_eq!(g.gid, g.face.glyph_for('ب'));
    }
}

#[test]
fn arabic_character_overrides_mirror_brackets_using_final_levels() {
    use designcraft_doc::arabic::CharacterDirection as D;
    for (text, direction, mirrored) in [("(123)", D::RightToLeft, true), ("(ب)", D::LeftToRight, false)] {
        let (mut d, sid, _) = doc_with(text, Rect::new(0.0, 0.0, 300.0, 100.0), ParaAttrs::default());
        d.story_mut(sid).unwrap().format_chars(0..text.len(), |f| f.over.character_direction = Some(direction));
        let cs = compose_story(&d, sid, &ComposeOptions::default());
        let gs = &cs.frames[0].lines[0].glyphs;
        for (byte, ch) in [(0, '('), (text.len() - 1, ')')] {
            let g = gs.iter().find(|g| g.byte == byte).unwrap();
            let expected = if mirrored { unicode_bidi_mirroring::get_mirrored(ch).unwrap() } else { ch };
            assert_eq!(g.gid, g.face.glyph_for(expected), "{text:?} {direction:?} byte {byte}");
        }
    }
}

#[test]
fn language_picks_localized_forms_through_the_word_cache() {
    // Source Serif 4 has a Turkish `locl` form of `i`; the same words in English and Turkish
    // (both shaped word by word through the cache) keep their own forms.
    let text = "in in in in";
    let (mut d, sid, _) = doc_with(text, Rect::new(36.0, 36.0, 500.0, 200.0), ParaAttrs::default());
    d.story_mut(sid).unwrap().format_chars(6..text.len(), |f| f.over.language = Some("Turkish".into()));
    let cs = compose_story(&d, sid, &ComposeOptions::default());
    let gid_at = |byte: usize| all_lines(&cs).iter().flat_map(|l| l.glyphs.iter()).find(|g| g.byte == byte).map(|g| g.gid).unwrap();
    assert_eq!(gid_at(0), gid_at(3), "English i");
    assert_eq!(gid_at(6), gid_at(9), "Turkish i");
    assert_ne!(gid_at(0), gid_at(6), "Turkish i takes its localized form");
    // A single word is shaped directly, in its language too.
    let (mut d, sid, _) = doc_with("i", Rect::new(36.0, 36.0, 500.0, 200.0), ParaAttrs::default());
    d.story_mut(sid).unwrap().format_chars(0..1, |f| f.over.language = Some("Turkish".into()));
    let cs = compose_story(&d, sid, &ComposeOptions::default());
    assert_eq!(all_lines(&cs)[0].glyphs[0].gid, gid_at(6), "Turkish i, shaped on its own");
}

#[test]
fn chinese_and_japanese_take_their_localized_forms() {
    // Noto Sans CJK SC (craft-fonts) draws 直 differently for Japanese (`locl`).
    let Some(noto) = designcraft_fonts::CRAFT_FONTS.iter().find(|f| f.family == "Noto Sans CJK SC") else { return };
    FontDb::global().add_font(noto.bytes.to_vec());
    let gid = |language: &str| {
        let (mut d, sid, _) = doc_with("直", Rect::new(36.0, 36.0, 300.0, 100.0), ParaAttrs::default());
        d.story_mut(sid).unwrap().format_chars(0.."直".len(), |f| {
            f.over.font_family = Some(noto.family.into());
            f.over.language = Some(language.into());
        });
        let cs = compose_story(&d, sid, &ComposeOptions::default());
        let g = &all_lines(&cs)[0].glyphs[0];
        assert_eq!(g.face.family, noto.family);
        g.gid
    };
    let chinese = gid("Chinese: Simplified");
    assert_eq!(chinese, FontDb::global().face(noto.family, noto.style).glyph_for('直'));
    assert_ne!(gid("Japanese"), chinese, "Japanese 直");
}

#[test]
fn cjk_fallback_follows_the_language() {
    use designcraft_fonts::testing::font_with;
    // Stand-ins for Hiragino Mincho ProN (the Japanese chain's first system font) and Songti SC
    // (first in the Simplified Chinese chain); the real ones when installed.
    let db = designcraft_fonts::FontDb::global();
    db.add_font(font_with("Hiragino Mincho ProN", &['直']).unwrap());
    db.add_font(font_with("Songti SC", &['直']).unwrap());
    // (face of the Latin letter, face of the ideograph)
    let faces_of = |language: &str| {
        let (mut d, sid, _) = doc_with("a直", Rect::new(36.0, 36.0, 300.0, 100.0), ParaAttrs::default());
        d.settings.glyph_fallback = true;
        d.story_mut(sid).unwrap().format_chars(0..4, |f| f.over.language = Some(language.into()));
        let cs = compose_story(&d, sid, &ComposeOptions::default());
        let face_at = |byte: usize| all_lines(&cs).iter().flat_map(|l| l.glyphs.iter()).find(|g| g.byte == byte).map(|g| g.face).unwrap();
        (face_at(0), face_at(1))
    };
    assert_eq!(faces_of("Chinese: Simplified").1.family, "Songti SC");
    // Japanese: the craft-fonts faces lead the chain (Mincho first), then the system fonts.
    let japanese = designcraft_fonts::japanese_document_fonts().first().map_or("Hiragino Mincho ProN", |f| f.family);
    assert_eq!(faces_of("Japanese").1.family, japanese);
    // Without a CJK language, the language-blind search.
    let (primary, han) = faces_of("English: USA");
    assert_eq!(Some(han.family.clone()), db.fallback_for('直', primary.id(), None).map(|f| f.family.clone()));
}

#[test]
fn malayalam_falls_back_to_a_font_that_forms_its_conjuncts() {
    // Run against the installed fonts: on macOS the first file mapping Malayalam is Arial
    // Unicode MS, which leaves ക്ഷ as three glyphs.
    let db = designcraft_fonts::FontDb::global();
    if !["Malayalam Sangam MN", "Malayalam MN", "Nirmala UI", "Noto Sans Malayalam"].iter().any(|f| db.has_family(f)) {
        return;
    }
    let text = "ക്ഷ";
    let (mut d, sid, _) = doc_with(text, Rect::new(0.0, 0.0, 300.0, 100.0), ParaAttrs::default());
    d.settings.glyph_fallback = true;
    let cs = compose_story(&d, sid, &ComposeOptions::default());
    let glyphs: Vec<_> = all_lines(&cs).iter().flat_map(|l| l.glyphs.iter()).map(|g| (g.face.family.clone(), g.gid)).collect();
    assert_eq!(glyphs.len(), 1, "one conjunct: {glyphs:?}");
    assert_ne!(glyphs[0].1, 0);
}

#[test]
fn missing_glyphs_are_the_fonts_box_unless_fallback_is_on() {
    use designcraft_fonts::testing::font_with;
    // Some font draws 語 when fallback fonts are allowed (a system CJK font, or this stand-in).
    designcraft_fonts::FontDb::global().add_font(font_with("DC Test Missing Glyph Helper", &['語']).unwrap());
    let text = "a語";
    let (mut d, sid, _) = doc_with(text, Rect::new(36.0, 36.0, 300.0, 100.0), ParaAttrs::default());
    d.story_mut(sid).unwrap().format_chars(0..text.len(), |f| f.over.language = Some("Japanese".into()));
    let glyph_at = |d: &Document, byte: usize| {
        let cs = compose_story(d, sid, &ComposeOptions::default());
        all_lines(&cs).iter().flat_map(|l| l.glyphs.iter()).find(|g| g.byte == byte).map(|g| (g.face, g.gid, g.adv)).unwrap()
    };
    assert!(!d.settings.glyph_fallback, "new documents draw missing glyphs as InDesign does");
    let (face, gid, adv) = glyph_at(&d, 1);
    assert_eq!((face.family.as_str(), gid), (designcraft_fonts::DEFAULT_FAMILY, 0), "Source Serif 4's .notdef");
    assert!((adv - face.advance(0) * 12.0 / face.upem).abs() < 1e-6, "its advance: {adv}");
    d.settings.glyph_fallback = true;
    let (face, gid, _) = glyph_at(&d, 1);
    assert_ne!(face.family, designcraft_fonts::DEFAULT_FAMILY);
    assert_ne!(gid, 0, "a fallback font draws it");
}

/// A missing font's substitute draws what it lacks from fallback fonts whatever the setting, also
/// in text shaped as one piece across a colour change.
#[test]
fn a_missing_fonts_substitute_takes_fallback_fonts() {
    use designcraft_fonts::testing::font_with;
    designcraft_fonts::FontDb::global().add_font(font_with("DC Test Missing Glyph Helper", &['語']).unwrap());
    let text = "a語b語";
    let (mut d, sid, _) = doc_with(text, Rect::new(36.0, 36.0, 300.0, 100.0), ParaAttrs::default());
    {
        let s = d.story_mut(sid).unwrap();
        s.format_chars(0..text.len(), |f| {
            f.over.language = Some("Japanese".into());
            f.over.font_family = Some("DC Test No Such Family".into());
        });
        s.format_chars("a語".len()..text.len(), |f| f.over.fill = Some("Red".into()));
    }
    assert!(!d.settings.glyph_fallback);
    let cs = compose_story(&d, sid, &ComposeOptions::default());
    let glyphs: Vec<&PlacedGlyph> = all_lines(&cs).into_iter().flat_map(|l| l.glyphs.iter()).filter(|g| text[g.byte..].starts_with('語')).collect();
    assert_eq!(glyphs.len(), 2);
    for g in glyphs {
        assert_ne!(g.face.family, designcraft_fonts::DEFAULT_FAMILY, "byte {}", g.byte);
        assert_ne!(g.gid, 0, "byte {}: a fallback font draws it", g.byte);
    }
}

#[test]
fn a_mark_starting_a_run_is_shaped_with_its_base() {
    use designcraft_fonts::testing::font_with;
    const HEBREW: &str = "DC Test Hebrew Points";
    designcraft_fonts::FontDb::global().add_font(font_with(HEBREW, &['ש', '\u{5C2}', 'ל']).unwrap());
    // Shin, then sin dot and lamed in a run that only turns ligatures off.
    let text = "ש\u{5C2}ל";
    let mark = "ש".len();
    let glyphs = |family: &str, mark_family: Option<&str>| {
        let (mut d, sid, _) = doc_with(text, Rect::new(36.0, 36.0, 300.0, 100.0), ParaAttrs::default());
        let story = d.story_mut(sid).unwrap();
        story.format_chars(0..text.len(), |f| f.over.font_family = Some(family.into()));
        story.format_chars(mark..text.len(), |f| {
            f.over.ligatures = Some(false);
            if let Some(m) = mark_family {
                f.over.font_family = Some(m.into());
            }
        });
        let cs = compose_story(&d, sid, &ComposeOptions::default());
        all_lines(&cs).iter().flat_map(|l| l.glyphs.iter()).map(|g| (g.face, g.gid, g.byte)).collect::<Vec<_>>()
    };
    // A missing font: its substitute lacks both, so the letter comes from a fallback font, and the
    // mark from the same font, in its letter's cluster.
    let g = glyphs("DC Test No Such Family", None);
    let (base, dot) = (g[0], g[1]);
    assert_ne!(base.1, 0, "{g:?}");
    assert_eq!((dot.0.family.as_str(), dot.2), (base.0.family.as_str(), 0), "the sin dot goes with its shin: {g:?}");
    assert_ne!(dot.1, 0, "{g:?}");
    // The run's own font lacks the mark: its box, whatever font the letter before it has.
    let g = glyphs(HEBREW, Some(designcraft_fonts::DEFAULT_FAMILY));
    let dot = g.iter().find(|x| x.2 == mark).unwrap();
    assert_eq!((dot.0.family.as_str(), dot.1), (designcraft_fonts::DEFAULT_FAMILY, 0), "{g:?}");
}

#[test]
fn kenten_missing_from_the_font_are_its_box_unless_fallback_is_on() {
    use designcraft_fonts::testing::font_with;
    // Some font draws the sesame dot when fallback fonts are allowed (a system font, or this one).
    designcraft_fonts::FontDb::global().add_font(font_with("DC Test Kenten Helper", &['\u{FE45}']).unwrap());
    let (mut d, sid, _) = doc_with("ab", Rect::new(36.0, 36.0, 300.0, 100.0), ParaAttrs::default());
    let plain = compose_story(&d, sid, &ComposeOptions::default()).frames[0].lines[0].glyphs.len();
    d.story_mut(sid).unwrap().format_chars(0..2, |f| f.over.kenten = Some(true));
    let marks = |d: &Document| {
        let cs = compose_story(d, sid, &ComposeOptions::default());
        cs.frames[0].lines[0].glyphs[plain..].iter().map(|g| (g.face.family.clone(), g.gid)).collect::<Vec<_>>()
    };
    let missing = marks(&d);
    assert_eq!(missing.len(), 2);
    assert!(missing.iter().all(|(family, gid)| family == designcraft_fonts::DEFAULT_FAMILY && *gid == 0), "{missing:?}");
    d.settings.glyph_fallback = true;
    let drawn = marks(&d);
    assert_eq!(drawn.len(), 2);
    assert!(drawn.iter().all(|(family, gid)| family != designcraft_fonts::DEFAULT_FAMILY && *gid != 0), "{drawn:?}");
}

#[test]
fn each_document_shapes_words_in_its_own_fonts() {
    use designcraft_fonts::testing::font_with_glyph;
    const FAMILY: &str = "DocFont Word Cache";
    // Two documents' fonts of one name: the same words, drawn as `X` in one and `O` in the other.
    let root = std::env::temp_dir().join(format!("dc-compose-docfonts-{}", std::process::id()));
    let mut scopes = Vec::new();
    for (name, glyph) in [("x", 'X'), ("o", 'O')] {
        let folder = root.join(name);
        std::fs::create_dir_all(&folder).unwrap();
        std::fs::write(folder.join("word.ttf"), font_with_glyph(FAMILY, &['a', 'b', ' '], glyph).unwrap()).unwrap();
        scopes.push((designcraft_fonts::FontDb::global().load_document_fonts(&folder, &|_| Ok(())).scope, glyph));
    }
    let text = "ab ab ab ab";
    let mut faces = Vec::new();
    for (scope, glyph) in scopes {
        let (mut d, sid, _) = doc_with(text, Rect::new(36.0, 36.0, 300.0, 100.0), ParaAttrs::default());
        d.font_scope = scope;
        d.story_mut(sid).unwrap().format_chars(0..text.len(), |f| f.over.font_family = Some(FAMILY.into()));
        let cs = compose_story(&d, sid, &ComposeOptions::default());
        let glyphs: Vec<&PlacedGlyph> =
            all_lines(&cs).into_iter().flat_map(|l| l.glyphs.iter()).filter(|g| text[g.byte..].starts_with('a')).collect();
        assert_eq!(glyphs.len(), 4);
        let face = glyphs[0].face;
        let drawn = designcraft_fonts::FontDb::global().face(designcraft_fonts::FALLBACK_FAMILY, "Regular").glyph_for(glyph);
        assert!(glyphs.iter().all(|g| g.face == face && g.gid == drawn), "scope {scope}: {glyph}");
        faces.push(face);
    }
    assert!(faces[0] != faces[1] && faces.iter().all(|f| f.family == FAMILY));
    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn composite_fonts_draw_from_the_documents_fonts() {
    use designcraft_doc::cjk::{CompositeFont, CompositeFontEntry};
    use designcraft_fonts::testing::font_with_glyph;
    const FAMILY: &str = "DocFont Composite Member";
    // A composite font whose digits come from a font only the document has.
    let folder = std::env::temp_dir().join(format!("dc-compose-composite-docfont-{}", std::process::id()));
    std::fs::create_dir_all(&folder).unwrap();
    std::fs::write(folder.join("digits.ttf"), font_with_glyph(FAMILY, &['1'], 'X').unwrap()).unwrap();
    let scope = FontDb::global().load_document_fonts(&folder, &|_| Ok(())).scope;
    let (mut d, sid, _) = doc_with("A1B", Rect::new(0.0, 0.0, 300.0, 100.0), ParaAttrs::default());
    d.font_scope = scope;
    std::sync::Arc::make_mut(&mut d.styles).composite_fonts.push(CompositeFont {
        name: "Mixed".into(),
        entries: vec![
            CompositeFontEntry { family: designcraft_fonts::DEFAULT_FAMILY.into(), ..Default::default() },
            CompositeFontEntry { characters: "0123456789".into(), family: FAMILY.into(), ..Default::default() },
        ],
    });
    d.story_mut(sid).unwrap().format_chars(0..3, |f| f.over.font_family = Some("CompositeFont/Mixed".into()));
    let cs = compose_story(&d, sid, &ComposeOptions::default());
    let digit = cs.frames[0].lines[0].glyphs.iter().find(|g| g.byte == 1).unwrap();
    assert_eq!(digit.face.family, FAMILY);
    assert_ne!(digit.gid, 0);
    assert!(!cs.styles.iter().any(|s| s.missing_font), "the document's font isn't missing");
    let _ = std::fs::remove_dir_all(&folder);
}

const KO_WORDS: &str = "한국어 문장은 띄어쓰기 단위로 줄을 바꿉니다 한국어 문장은 띄어쓰기 단위로 줄을 바꿉니다";
const KO_HANJA: &str = "大韓民國 憲法은 國民의 權利를 保障한다 大韓民國 憲法은 國民의 權利를 保障한다";
const KO_PUNCT: &str = "가나다라마。바사아자차」카타파하가、나다라마바。사아자차카」타파하가나、다라마바사。아자차카타」파하가나다、";
/// Syllables for words wider than the frame.
const KO_LONG: &str = "가나다라마바사아자차카타파하」";

/// A Korean paragraph in a narrow frame (70 pt), set in a stand-in font covering the samples.
fn korean_doc(text: &str, language: &str, para: ParaAttrs) -> (Document, StoryId) {
    let chars: Vec<char> = [KO_WORDS, KO_HANJA, KO_PUNCT, KO_LONG].concat().chars().collect();
    let font = designcraft_fonts::testing::font_with("DC Test Korean", &chars).unwrap();
    designcraft_fonts::FontDb::global().add_font(font);
    let (mut d, sid, _) = doc_with(text, Rect::new(36.0, 36.0, 106.0, 400.0), para);
    d.story_mut(sid).unwrap().format_chars(0..text.len(), |f| {
        f.over.font_family = Some("DC Test Korean".into());
        f.over.language = Some(language.into());
    });
    (d, sid)
}

/// The lines (after the first) that start inside a space-separated word.
fn lines_starting_mid_word(d: &Document, sid: StoryId, text: &str) -> Vec<String> {
    let cs = compose_story(d, sid, &ComposeOptions::default());
    let lines = all_lines(&cs);
    assert!(lines.len() > 2, "wraps: {} lines", lines.len());
    lines.iter().skip(1).filter(|l| !text[..l.range.start].ends_with(' ')).map(|l| text[l.range.clone()].to_string()).collect()
}

#[test]
fn korean_breaks_at_spaces() {
    for composer in [designcraft_doc::Composer::Paragraph, designcraft_doc::Composer::SingleLine] {
        for align in [Align::Left, Align::LeftJustified] {
            let para = ParaAttrs { composer: Some(composer), align: Some(align), ..Default::default() };
            // By script: Hangul is set word by word whatever the language.
            for language in ["Korean", "English: USA"] {
                let (d, sid) = korean_doc(KO_WORDS, language, para.clone());
                let mid = lines_starting_mid_word(&d, sid, KO_WORDS);
                assert!(mid.is_empty(), "{composer:?} {align:?} {language}: {mid:?}");
            }
        }
    }
}

#[test]
fn korean_breaks_at_spaces_under_a_kinsoku_set() {
    let set = designcraft_doc::cjk::Kinsoku::named("KoreanKinsoku");
    assert!(set.is_some());
    for composer in [designcraft_doc::Composer::Paragraph, designcraft_doc::Composer::SingleLine] {
        let para = ParaAttrs { composer: Some(composer), kinsoku: Some(set.clone()), ..Default::default() };
        let (d, sid) = korean_doc(KO_WORDS, "Korean", para.clone());
        let mid = lines_starting_mid_word(&d, sid, KO_WORDS);
        assert!(mid.is_empty(), "{composer:?}: {mid:?}");
        let (d, sid) = korean_doc(KO_WORDS, "Korean", ParaAttrs { korean_char_breaks: Some(true), ..para });
        assert!(!lines_starting_mid_word(&d, sid, KO_WORDS).is_empty(), "{composer:?}: breaks between syllables on request");
    }
}

#[test]
fn korean_character_breaks_on_request() {
    let para = ParaAttrs { korean_char_breaks: Some(true), composer: Some(designcraft_doc::Composer::SingleLine), ..Default::default() };
    let (d, sid) = korean_doc(KO_WORDS, "Korean", para);
    assert!(!lines_starting_mid_word(&d, sid, KO_WORDS).is_empty(), "breaks between syllables");
}

#[test]
fn korean_by_its_locale_code_breaks_at_spaces() {
    let para = ParaAttrs { composer: Some(designcraft_doc::Composer::SingleLine), ..Default::default() };
    let (d, sid) = korean_doc(KO_HANJA, "ko_KR", para);
    let mid = lines_starting_mid_word(&d, sid, KO_HANJA);
    assert!(mid.is_empty(), "{mid:?}");
}

#[test]
fn hanja_in_korean_text_breaks_at_spaces() {
    let para = ParaAttrs { composer: Some(designcraft_doc::Composer::SingleLine), ..Default::default() };
    let (d, sid) = korean_doc(KO_HANJA, "Korean", para.clone());
    let mid = lines_starting_mid_word(&d, sid, KO_HANJA);
    assert!(mid.is_empty(), "{mid:?}");
    // Han in Japanese text breaks between characters.
    let (d, sid) = korean_doc(KO_HANJA, "Japanese", para);
    assert!(!lines_starting_mid_word(&d, sid, KO_HANJA).is_empty());
}

#[test]
fn korean_lines_keep_kinsoku() {
    // Words of 9 to 14 syllables and a closing bracket: in one of them the bracket is what
    // overflows the frame.
    let syllables: Vec<char> = KO_LONG.chars().collect();
    let long = (9..=14).map(|n| format!("{}」 가나", syllables[..n].iter().collect::<String>()));
    for text in std::iter::once(KO_PUNCT.to_string()).chain(long) {
        let text = text.as_str();
        for char_breaks in [false, true] {
            for composer in [designcraft_doc::Composer::Paragraph, designcraft_doc::Composer::SingleLine] {
                let para = ParaAttrs { korean_char_breaks: Some(char_breaks), composer: Some(composer), ..Default::default() };
                let (d, sid) = korean_doc(text, "Korean", para);
                let cs = compose_story(&d, sid, &ComposeOptions::default());
                let lines = all_lines(&cs);
                assert!(lines.len() > 1, "wraps");
                let texts: Vec<&str> = lines.iter().map(|l| &text[l.range.clone()]).collect();
                assert!(texts.iter().all(|t| !t.starts_with(['、', '。', '」'])), "kinsoku ({char_breaks}, {composer:?}): {texts:?}");
            }
        }
    }
}

/// A synthetic font for vertical text: Source Sans 3 glyphs (CJK characters drawn as Latin ones)
/// with vertical metrics, every glyph one em down the line and 120 units below its vertical
/// origin, except ヸ (drawn as O): 1.024 em. No vertical forms. Added to the global database once.
fn vertical_test_font() -> &'static str {
    use designcraft_fonts::testing::{font_mapping, with_vmtx};
    const FAMILY: &str = "DC Test Vertical";
    static ADDED: std::sync::Once = std::sync::Once::new();
    ADDED.call_once(|| {
        let glyphs = [('一', 'X'), ('二', 'X'), ('日', 'X'), ('本', 'X'), ('ま', 'o'), ('ヸ', 'O'), ('、', ','), ('A', 'A'), ('b', 'b')];
        let base = font_mapping(FAMILY, &glyphs).unwrap();
        let scratch = designcraft_fonts::FontDb::with_font_dirs(vec![]);
        scratch.add_font(base.clone());
        let wi = u16::try_from(scratch.face(FAMILY, "Regular").glyph_for('ヸ')).unwrap();
        designcraft_fonts::FontDb::global().add_font(with_vmtx(&base, (1000, 120), &[(wi, 1024, 100)]).unwrap());
    });
    FAMILY
}

/// Shippori Mincho (vertical forms, `vpal`) when built with craft-fonts; else `None`, with a note.
fn shippori() -> Option<&'static str> {
    if !designcraft_fonts::japanese_fonts().any(|f| f.family == "Shippori Mincho") {
        eprintln!("skipped Shippori Mincho: built without craft-fonts (set CRAFT_FONTS_DIR to a checkout)");
        return None;
    }
    Some("Shippori Mincho")
}

/// The fonts vertical layout is checked in: the synthetic one, and Shippori Mincho with craft-fonts.
fn vertical_families() -> Vec<&'static str> {
    std::iter::once(vertical_test_font()).chain(shippori()).collect()
}

/// A vertical frame (no inset) with `text` in `family` at 20 pt (and `over`), and its first line
/// with the frame's first column.
fn vertical_line(family: &str, text: &str, over: impl Fn(&mut designcraft_doc::CharFormat)) -> (Line, Rect) {
    let mut d = Document::new(&NewDocument::default());
    let lid = d.default_layer();
    let (fid, sid) = d.add_text_frame(SpreadRef::Doc(0), Rect::new(100.0, 100.0, 200.0, 500.0), lid, text, ParaFormat::default()).unwrap();
    if let Some(tf) = d.item_mut(fid).and_then(|i| i.text_frame_mut()) {
        tf.options.inset = [0.0; 4];
    }
    d.story_mut(sid).unwrap().vertical = true;
    d.story_mut(sid).unwrap().format_chars(0..text.len(), |f| {
        f.over.font_family = Some(family.into());
        f.over.size = Some(20.0);
        over(f);
    });
    let cs = compose_story(&d, sid, &ComposeOptions::default());
    let ft = &cs.frames[0];
    assert!(ft.vertical);
    let line = ft.lines[0].clone();
    assert!(line.glyphs.iter().all(|g| g.face.family == family), "{family}: {:?}", line.glyphs.iter().map(|g| &g.face.family).collect::<Vec<_>>());
    (line, ft.columns[0])
}

/// Text-space bounds of a glyph's ink as drawn in a vertical frame.
fn vertical_ink(g: &PlacedGlyph, baseline: f64) -> Rect {
    use designcraft_geom::{Affine, Shape};
    let ink = designcraft_fonts::FontDb::global().outline(&g.face, g.gid).bounding_box();
    let mut a = Affine::translate((g.x, baseline + g.y)) * Affine::scale_non_uniform(g.sx, g.sy);
    if let Some(turn) = g.vertical_xf(baseline) {
        a = turn * a;
    }
    a.transform_rect_bbox(ink)
}

/// The em box in text space: along the line from x (page down), across from its top (text -y,
/// page right) to its bottom → (right, left, centre).
fn em_across(l: &Line, g: &PlacedGlyph) -> (f64, f64, f64) {
    let (top, bottom) = g.face.em_box();
    let right = l.baseline + g.y - top * g.sy;
    let left = l.baseline + g.y - bottom * g.sy;
    (right, left, (right + left) / 2.0)
}

#[test]
fn upright_glyphs_hang_on_the_line_centre() {
    for family in vertical_families() {
        let (l, _) = vertical_line(family, "一", |_| {});
        let ichi = &l.glyphs[0];
        assert!(ichi.upright, "{family}");
        // An ideograph is centred across the line and inside its em along it.
        let (_, _, centre) = em_across(&l, ichi);
        let ink = vertical_ink(ichi, l.baseline);
        assert!(((ink.y0 + ink.y1) / 2.0 - centre).abs() < 0.05 * 20.0, "{family}: {ink:?} {centre}");
        assert!(ink.x0 > ichi.x && ink.x1 < ichi.x + ichi.adv, "{family}: {ink:?}");
    }
    // The synthetic glyph's top is its top side bearing (120 units) below its vertical origin.
    let (l, _) = vertical_line(vertical_test_font(), "一", |_| {});
    let ink = vertical_ink(&l.glyphs[0], l.baseline);
    assert!((ink.x0 - l.glyphs[0].x - 2.4).abs() < 1e-6, "{ink:?} {}", l.glyphs[0].x);
}

#[test]
fn upright_punctuation_takes_its_vertical_form_and_place() {
    // A font with vertical forms: Shippori Mincho.
    let Some(family) = shippori() else { return };
    let (l, _) = vertical_line(family, "一、", |_| {});
    let comma = &l.glyphs[1];
    assert!(comma.upright);
    assert_ne!(comma.gid, comma.face.glyph_for('、'), "vertical form (vert)");
    let (right, _, centre) = em_across(&l, comma);
    // The comma's ink sits in the upper right quarter of its em box on the page.
    let ink = vertical_ink(comma, l.baseline);
    assert!(ink.x0 >= comma.x - 1e-6 && ink.x1 < comma.x + comma.adv / 2.0, "upper half: {ink:?} {} {}", comma.x, comma.adv);
    assert!(ink.y0 >= right - 1e-6 && ink.y1 < centre, "right half: {ink:?} {right} {centre}");
}

#[test]
fn latin_in_vertical_text_turns_as_a_run() {
    // Shippori Mincho's `vrt2` has turned Latin forms; a turned run must not use them.
    for family in vertical_families() {
        let (l, _) = vertical_line(family, "日本Ab", |_| {});
        let latin: Vec<_> = l.glyphs.iter().filter(|g| g.byte >= "日本".len()).collect();
        let face = latin[0].face;
        assert_eq!(latin.iter().map(|g| g.gid).collect::<Vec<_>>(), [face.glyph_for('A'), face.glyph_for('b')], "{family}");
        assert!(latin.iter().all(|g| !g.upright && g.vertical_xf(l.baseline).is_none()), "{family}");
        assert!((latin[0].adv - face.advance(latin[0].gid) * latin[0].sx).abs() < 1e-9, "{family}: horizontal advance");
    }
}

#[test]
fn upright_glyphs_advance_by_their_vertical_metrics() {
    // ヸ is 1.024 em tall in both fonts.
    for family in vertical_families() {
        let (l, _) = vertical_line(family, "一ヸ一", |_| {});
        let g = &l.glyphs;
        assert_eq!(g[1].face.v_advance(g[1].gid), 1024.0, "{family}");
        assert!((g[1].x - g[0].x - 20.0).abs() < 1e-9, "{family}");
        assert!((g[2].x - g[1].x - 20.48).abs() < 1e-9, "{family}: {} {}", g[1].x, g[2].x);
    }
    let Some(family) = shippori() else { return };
    // Shippori's ヸ is an em wide.
    let (l, _) = vertical_line(family, "ヸ", |_| {});
    assert_eq!(l.glyphs[0].face.advance(l.glyphs[0].gid), 1000.0);
    // Proportional vertical metrics (`vpal`): ま takes 0.974 em.
    let (l, _) = vertical_line(family, "まま", |f| f.over.otf_features = Some(vec!["vpal".into()]));
    assert!((l.glyphs[1].x - l.glyphs[0].x - 19.48).abs() < 1e-9, "{:?}", l.glyphs.iter().map(|g| g.x).collect::<Vec<_>>());
    let (l, _) = vertical_line(family, "まま", |_| {});
    assert!((l.glyphs[1].x - l.glyphs[0].x - 20.0).abs() < 1e-9);
}

#[test]
fn vertical_lines_fit_the_em_box() {
    // First baseline at the ascent: an upright line's ascent is its em box top, so the em box
    // touches the frame's right edge. The synthetic font's em box is Source Sans 3's BASE one
    // (0.83 em, -0.17 em); Shippori Mincho's its OS/2 typo metrics (0.88 em, -0.12 em).
    let expected = [(vertical_test_font(), 16.6, 3.4)].into_iter().chain(shippori().map(|f| (f, 17.6, 2.4)));
    for (family, ascent, descent) in expected {
        let (l, col) = vertical_line(family, "一二", |_| {});
        assert!((l.ascent - ascent).abs() < 1e-9 && (l.descent - descent).abs() < 1e-9, "{family}: {} {}", l.ascent, l.descent);
        assert!((l.baseline - col.y0 - ascent).abs() < 1e-9, "{family}: {} {}", l.baseline, col.y0);
    }
}

/// A drop cap set in one of the document's own fonts measures its side bearing in that font.
#[test]
fn drop_cap_in_a_document_font_aligns_its_left_edge() {
    use designcraft_geom::Shape as _;
    const FAMILY: &str = "DocFont Drop Cap";
    let text = format!("Once {LOREM}");
    let mut chars: Vec<char> = text.chars().collect();
    chars.sort_unstable();
    chars.dedup();
    let folder = std::env::temp_dir().join(format!("dc-compose-dropcap-docfont-{}", std::process::id()));
    std::fs::create_dir_all(&folder).unwrap();
    std::fs::write(folder.join("dropcap.ttf"), designcraft_fonts::testing::font_with_glyph(FAMILY, &chars, 'O').unwrap()).unwrap();
    let scope = designcraft_fonts::FontDb::global().load_document_fonts(&folder, &|_| Ok(())).scope;
    let para = ParaAttrs { left_indent: Some(10.0), ..drop_cap(2, 1) };
    let (mut d, sid, _) = doc_with(&text, Rect::new(0.0, 0.0, 300.0, 1000.0), para);
    d.font_scope = scope;
    d.story_mut(sid).unwrap().format_chars(0..text.len(), |f| f.over.font_family = Some(FAMILY.into()));
    let cs = compose_story(&d, sid, &ComposeOptions::default());
    let dc = &all_lines(&cs)[0].glyphs[0];
    assert_eq!(dc.face.family, FAMILY);
    let ink_left = dc.x + designcraft_fonts::FontDb::global().outline(dc.face.get(), dc.gid).bounding_box().x0 * dc.sx;
    assert!(ink_left - dc.x > 0.5, "the O has a visible side bearing");
    assert!((ink_left - 10.0).abs() < 1e-6, "ink at {ink_left}");
    let _ = std::fs::remove_dir_all(&folder);
}

#[test]
fn embedded_text_frames_compose_and_footnotes_retain_object_sources() {
    use designcraft_doc::{AnchorPosition, AnchoredObject};
    let (mut d, body, _) = doc_with("Body", Rect::new(20.0, 20.0, 400.0, 600.0), ParaAttrs::default());
    let (fid, child) =
        d.add_text_frame(SpreadRef::Doc(0), Rect::new(0.0, 0.0, 80.0, 40.0), d.default_layer(), "Embedded", ParaFormat::default()).unwrap();
    let item = d.item(fid).unwrap().clone();
    d.spread_mut(SpreadRef::Doc(0)).unwrap().items.retain(|i| i.id != fid);
    let story = d.story_mut(body).unwrap();
    story.insert_note(4, "Note", ParaFormat::default());
    Arc::make_mut(&mut story.notes[0]).text.insert_object(0, AnchoredObject::new(item, AnchorPosition::Inline { y_offset: 0.0 }));
    d.check().unwrap();
    let child_cs = compose_story(&d, child, &ComposeOptions::default());
    assert!(!child_cs.is_overset());
    assert!(!child_cs.frame(fid).unwrap().lines.is_empty());
    let cs = compose_story(&d, body, &ComposeOptions::default());
    assert!(!cs.is_overset());
    let note = &cs.frames[0].notes[0];
    assert_eq!(note.text.story, StoryId(0));
    assert_eq!(note.text.frames[0].objects[0].object.item.id, fid);

    let cache = Cache::new();
    assert!(!cache.get(&d, child, None).is_overset());
    let note = Arc::make_mut(&mut d.story_mut(body).unwrap().notes[0]);
    let anchor = Arc::make_mut(&mut note.text.objects[0]);
    let original_path = anchor.item.path.clone();
    anchor.item.path = designcraft_geom::shapes::rectangle(Rect::new(0.0, 0.0, 80.0, 1.0));
    assert!(cache.get(&d, child, None).is_overset(), "embedded frame geometry must invalidate cached composition");
    Arc::make_mut(&mut Arc::make_mut(&mut d.story_mut(body).unwrap().notes[0]).text.objects[0]).item.path = original_path;

    // A reference back into the owning story must be rejected before rendering.
    let mut cycle = d.anchored_item(fid).unwrap().clone();
    cycle.id = ItemId(d.alloc());
    cycle.text_frame_mut().unwrap().story = body;
    d.story_mut(body).unwrap().frames.push(cycle.id);
    d.story_mut(child).unwrap().insert_object(0, AnchoredObject::new(cycle, AnchorPosition::Inline { y_offset: 0.0 }));
    assert!(d.check().unwrap_err().to_string().contains("cyclic"));
}

#[test]
fn inline_object_only_line_does_not_reserve_phantom_font_descent() {
    use designcraft_doc::{AnchorPosition, AnchoredObject, Item, Shape};
    let (mut d, sid, _) = doc_with("", Rect::new(0.0, 0.0, 10.0, 15.0), ParaAttrs::default());
    let item = Item::new(ItemId(d.alloc()), d.default_layer(), Shape::Rectangle, designcraft_geom::shapes::rectangle(Rect::new(0.0, 0.0, 7.0, 9.0)));
    d.story_mut(sid).unwrap().insert_object(0, AnchoredObject::new(item, AnchorPosition::Inline { y_offset: 5.0 }));
    let cs = compose_story(&d, sid, &ComposeOptions::default());
    assert!(!cs.is_overset());
    assert_eq!(cs.frames[0].objects.len(), 1);
    assert_eq!(cs.frames[0].lines[0].descent, 0.0);
    // Genuine geometric overflow must still be reported.
    d.story_mut(sid).unwrap().objects[0] =
        Arc::new(AnchoredObject::new(cs.frames[0].objects[0].object.item.clone(), AnchorPosition::Inline { y_offset: 7.0 }));
    assert!(compose_story(&d, sid, &ComposeOptions::default()).is_overset());
}

#[test]
fn composite_metrics_and_character_mappings_apply_inside_footnotes() {
    use designcraft_doc::cjk::{CompositeFont, CompositeFontEntry};
    let (mut d, sid, _) = doc_with("Body", Rect::new(0.0, 0.0, 300.0, 300.0), ParaAttrs::default());
    d.styles_mut().composite_fonts.push(CompositeFont {
        name: "Mixed".into(),
        entries: vec![
            CompositeFontEntry { family: "Source Serif 4".into(), ..Default::default() },
            CompositeFontEntry {
                characters: "12".into(),
                family: "Source Sans 3".into(),
                relative_size: 0.8,
                horizontal_scale: 0.9,
                vertical_scale: 1.1,
                baseline_shift: 0.1,
                scale_option: false,
                ..Default::default()
            },
            CompositeFontEntry { characters: "2".into(), family: "Inter".into(), ..Default::default() },
        ],
    });
    let story = d.story_mut(sid).unwrap();
    story.insert_note(4, "A12", ParaFormat::default());
    let note = Arc::make_mut(&mut story.notes[0]);
    note.text.format_chars(0..3, |f| f.over.font_family = Some("CompositeFont/Mixed".into()));
    let cs = compose_story(&d, sid, &ComposeOptions::default());
    let note = &cs.frames[0].notes[0];
    let glyphs = &note.text.frames[0].lines[0].glyphs;
    let a = glyphs.iter().find(|g| g.byte == 0 && g.len > 0).unwrap();
    let one = glyphs.iter().find(|g| g.byte == 1 && g.len > 0).unwrap();
    let two = glyphs.iter().find(|g| g.byte == 2 && g.len > 0).unwrap();
    assert_eq!(a.face.family, "Source Serif 4");
    assert_eq!(one.face.family, "Source Sans 3");
    assert_eq!(two.face.family, "Inter");
    let style = &note.text.styles[one.style as usize];
    assert!((style.size - 9.6).abs() < 1e-8);
    assert!(!note.text.styles.iter().any(|s| s.missing_font));
}

#[test]
fn generated_note_labels_resolve_composite_components_and_keep_one_anchor() {
    use designcraft_doc::cjk::{CompositeFont, CompositeFontEntry};
    let (mut d, sid, _) = doc_with("Body", Rect::new(0.0, 0.0, 300.0, 300.0), ParaAttrs::default());
    d.styles_mut().composite_fonts.push(CompositeFont {
        name: "Generated Mixed".into(),
        entries: vec![
            CompositeFontEntry { family: "Source Serif 4".into(), ..Default::default() },
            CompositeFontEntry { characters: "[]".into(), family: "Inter".into(), relative_size: 0.8, ..Default::default() },
        ],
    });
    d.footnote_options.affix_in = designcraft_doc::notes::AffixIn::Both;
    d.footnote_options.prefix = "[".into();
    d.footnote_options.suffix = "]".into();
    let story = d.story_mut(sid).unwrap();
    story.insert_note(4, "Note", ParaFormat::default());
    story.format_chars(4..7, |f| f.over.font_family = Some("Generated Mixed".into()));
    let cs = compose_story(&d, sid, &ComposeOptions::default());
    let glyphs: Vec<_> = cs.frames[0].lines.iter().flat_map(|l| &l.glyphs).filter(|g| g.byte == 4).collect();
    assert_eq!(glyphs.iter().filter(|g| g.len > 0).count(), 1);
    assert_eq!(glyphs.iter().map(|g| g.rendered_char).collect::<String>(), "[1]");
    assert!(glyphs.iter().all(|g| g.gid != 0));
    assert!(glyphs.iter().filter(|g| matches!(g.rendered_char, '[' | ']')).all(|g| g.face.family == "Inter"));
    assert_eq!(glyphs.iter().find(|g| g.rendered_char == '1').unwrap().face.family, "Source Serif 4");
    assert_eq!(cs.frames[0].notes.len(), 1);
}

#[test]
fn generated_note_labels_use_coverage_fallback() {
    let font = designcraft_fonts::testing::font_with("Generated Label Coverage", &['\u{E888}']).unwrap();
    designcraft_fonts::FontDb::global().add_font(font);
    let (mut d, sid, _) = doc_with("Body", Rect::new(0.0, 0.0, 300.0, 300.0), ParaAttrs::default());
    d.footnote_options.affix_in = designcraft_doc::notes::AffixIn::Both;
    d.footnote_options.prefix = "\u{E888}".into();
    d.footnote_options.suffix = "".into();
    d.settings.glyph_fallback = true;
    d.story_mut(sid).unwrap().insert_note(4, "Note", ParaFormat::default());
    let cs = compose_story(&d, sid, &ComposeOptions::default());
    let glyph = cs.frames[0].lines.iter().flat_map(|l| &l.glyphs).find(|g| g.rendered_char == '\u{E888}').unwrap();
    assert_ne!(glyph.gid, 0);
    assert_eq!(glyph.face.family, "Generated Label Coverage");
}

#[test]
fn mojikumi_custom_pairs_change_positions_and_breaks_without_font_scaling() {
    use designcraft_doc::cjk::{MojikumiAki, MojikumiTable};
    let (mut d, sid, _) = doc_with(
        "A1A1A1",
        Rect::new(0.0, 0.0, 55.0, 300.0),
        ParaAttrs { hyphenate: Some(false), composer: Some(Composer::SingleLine), ..Default::default() },
    );
    let before = compose_story(&d, sid, &ComposeOptions::default());
    Arc::make_mut(&mut d.styles).mojikumi_tables.push(MojikumiTable {
        name: "Synthetic".into(),
        based_on: "SimpChineseDefault".into(),
        overrides: vec![
            MojikumiAki { target_class: 18, side_class: 25, after: true, minimum: 1.0, desired: 1.0, maximum: 1.0, ..Default::default() },
            MojikumiAki { target_class: 25, side_class: 18, after: true, minimum: 1.0, desired: 1.0, maximum: 1.0, ..Default::default() },
        ],
    });
    d.story_mut(sid).unwrap().paras[0].para.mojikumi = Some("MojikumiTable/Synthetic".into());
    let after = compose_story(&d, sid, &ComposeOptions::default());
    assert!(all_lines(&after).len() > all_lines(&before).len());
    let a = &all_lines(&after)[0].glyphs;
    let b = &all_lines(&before)[0].glyphs;
    assert!((a[1].x - b[1].x - 12.0).abs() < 0.001);
    assert_eq!(a[0].sx, b[0].sx);
    assert!(all_lines(&after).iter().all(|l| l.end_x <= l.x1 + 0.01));
}

#[test]
fn mojikumi_paragraph_indent_adds_to_paragraph_settings_and_rounds_no_glyphs() {
    use designcraft_doc::cjk::{MojikumiAki, MojikumiTable};
    let (mut d, sid, _) = doc_with(
        "ABC",
        Rect::new(0.0, 0.0, 200.0, 100.0),
        ParaAttrs { first_line_indent: Some(7.0), mojikumi: Some("MojikumiTable/Indent".into()), ..Default::default() },
    );
    Arc::make_mut(&mut d.styles).mojikumi_tables.push(MojikumiTable {
        name: "Indent".into(),
        based_on: "SimpChineseDefault".into(),
        overrides: vec![MojikumiAki {
            target_class: 18,
            side_class: 23,
            after: false,
            minimum: 2.0,
            desired: 2.0,
            maximum: 2.0,
            ..Default::default()
        }],
    });
    let cs = compose_story(&d, sid, &ComposeOptions::default());
    let line = all_lines(&cs)[0];
    assert!((line.glyphs[0].x - 31.0).abs() < 0.001, "{}", line.glyphs[0].x);
    assert_eq!(line.glyphs.len(), 3);
}

#[test]
fn mojikumi_justifies_cjk_without_spaces_and_respects_fixed_spacing() {
    use designcraft_doc::cjk::{MojikumiAki, MojikumiTable};
    let (mut d, sid, _) = doc_with(
        "A1A1",
        Rect::new(0.0, 0.0, 100.0, 100.0),
        ParaAttrs { align: Some(Align::FullyJustified), mojikumi: Some("MojikumiTable/Elastic".into()), ..Default::default() },
    );
    Arc::make_mut(&mut d.styles).mojikumi_tables.push(MojikumiTable {
        name: "Elastic".into(),
        based_on: "SimpChineseDefault".into(),
        overrides: vec![MojikumiAki {
            target_class: 18,
            side_class: 25,
            after: true,
            minimum: 0.0,
            desired: 0.0,
            maximum: 5.0,
            priority: 1,
            ..Default::default()
        }],
    });
    let cs = compose_story(&d, sid, &ComposeOptions::default());
    assert!((all_lines(&cs)[0].end_x - 100.0).abs() < 0.001, "{}", all_lines(&cs)[0].end_x);
}

#[test]
fn mojikumi_fixed_pairs_are_not_stretched_by_single_word_justification() {
    use designcraft_doc::cjk::{MojikumiAki, MojikumiTable};
    let (mut d, sid, _) = doc_with(
        "A1A1",
        Rect::new(0.0, 0.0, 150.0, 100.0),
        ParaAttrs { align: Some(Align::FullyJustified), mojikumi: Some("MojikumiTable/Fixed".into()), ..Default::default() },
    );
    Arc::make_mut(&mut d.styles).mojikumi_tables.push(MojikumiTable {
        name: "Fixed".into(),
        based_on: "SimpChineseDefault".into(),
        overrides: [(18, 25), (25, 18)]
            .into_iter()
            .map(|(target_class, side_class)| MojikumiAki {
                target_class,
                side_class,
                after: true,
                minimum: 0.25,
                desired: 0.25,
                maximum: 0.25,
                ..Default::default()
            })
            .collect(),
    });
    let cs = compose_story(&d, sid, &ComposeOptions::default());
    let line = all_lines(&cs)[0];
    assert!(line.end_x < 100.0, "Fixed spacing was stretched: {}", line.end_x);
    d.story_mut(sid).unwrap().paras[0].para.align = Some(Align::Left);
    let ragged = compose_story(&d, sid, &ComposeOptions::default());
    assert_eq!(line.end_x, all_lines(&ragged)[0].end_x);
}

#[test]
fn mojikumi_kinsoku_push_in_and_push_out_select_different_legal_breaks() {
    use designcraft_doc::cjk::{MojikumiAki, MojikumiTable};
    let text = "甲乙丙丁戊己庚辛";
    let (mut d, sid, _) = doc_with(
        text,
        Rect::new(0.0, 0.0, 45.0, 300.0),
        ParaAttrs {
            composer: Some(Composer::SingleLine),
            mojikumi: Some("MojikumiTable/Compress".into()),
            kinsoku_type: Some("KinsokuPushInFirst".into()),
            ..Default::default()
        },
    );
    Arc::make_mut(&mut d.styles).mojikumi_tables.push(MojikumiTable {
        name: "Compress".into(),
        based_on: "SimpChineseDefault".into(),
        overrides: vec![MojikumiAki {
            target_class: 12,
            side_class: 12,
            after: true,
            minimum: 0.0,
            desired: 1.0,
            maximum: 1.0,
            priority: 1,
            ..Default::default()
        }],
    });
    let inside = compose_story(&d, sid, &ComposeOptions::default());
    d.story_mut(sid).unwrap().paras[0].para.kinsoku_type = Some("KinsokuPushOutFirst".into());
    let outside = compose_story(&d, sid, &ComposeOptions::default());
    assert!(all_lines(&inside)[0].range.end > all_lines(&outside)[0].range.end);
    for cs in [&inside, &outside] {
        assert!(all_lines(cs).iter().all(|l| l.end_x <= l.x1 + 0.01));
    }
}

#[test]
fn mojikumi_priority_and_elastic_line_start_are_used_in_placement() {
    use designcraft_doc::cjk::{MojikumiAki, MojikumiTable};
    let (mut d, sid, _) = doc_with("A1B2", Rect::new(0.0, 0.0, 45.0, 100.0), ParaAttrs::default());
    let natural = compose_story(&d, sid, &ComposeOptions::default());
    let middle_gap = all_lines(&natural)[0].glyphs[2].x - all_lines(&natural)[0].glyphs[1].x;
    Arc::make_mut(&mut d.styles).mojikumi_tables.push(MojikumiTable {
        name: "Priority".into(),
        based_on: "SimpChineseDefault".into(),
        overrides: vec![
            MojikumiAki {
                target_class: 18,
                side_class: 25,
                after: true,
                minimum: 0.0,
                desired: 0.0,
                maximum: 10.0,
                priority: 1,
                ..Default::default()
            },
            MojikumiAki {
                target_class: 25,
                side_class: 18,
                after: true,
                minimum: 0.0,
                desired: 0.0,
                maximum: 10.0,
                priority: 2,
                ..Default::default()
            },
        ],
    });
    let para = &mut d.story_mut(sid).unwrap().paras[0].para;
    para.mojikumi = Some("MojikumiTable/Priority".into());
    para.align = Some(Align::FullyJustified);
    let result = compose_story(&d, sid, &ComposeOptions::default());
    let line = all_lines(&result)[0];
    assert!((line.end_x - 45.0).abs() < 0.001);
    assert!((line.glyphs[2].x - line.glyphs[1].x - middle_gap).abs() < 0.001, "Priority 2 was used before priority 1 was exhausted");

    let (mut d, sid, _) = doc_with(
        "A",
        Rect::new(0.0, 0.0, 30.0, 100.0),
        ParaAttrs { mojikumi: Some("MojikumiTable/Start".into()), align: Some(Align::FullyJustified), ..Default::default() },
    );
    Arc::make_mut(&mut d.styles).mojikumi_tables.push(MojikumiTable {
        name: "Start".into(),
        based_on: "SimpChineseDefault".into(),
        overrides: vec![MojikumiAki {
            target_class: 18,
            side_class: 23,
            after: false,
            minimum: 0.0,
            desired: 0.0,
            maximum: 5.0,
            priority: 1,
            ..Default::default()
        }],
    });
    let result = compose_story(&d, sid, &ComposeOptions::default());
    let line = all_lines(&result)[0];
    assert!((line.end_x - 30.0).abs() < 0.001);
    assert!(line.glyphs[0].x > 10.0, "Leading gap must move the first outline too");
}

#[test]
fn mojikumi_equal_priority_shares_leading_and_internal_space() {
    use designcraft_doc::cjk::{MojikumiAki, MojikumiTable};
    let (mut d, sid, _) = doc_with("A1", Rect::new(0.0, 0.0, 40.0, 100.0), ParaAttrs::default());
    let natural = compose_story(&d, sid, &ComposeOptions::default());
    let old = all_lines(&natural)[0];
    let extra = 40.0 - old.end_x;
    let old_second = old.glyphs[1].x;
    Arc::make_mut(&mut d.styles).mojikumi_tables.push(MojikumiTable {
        name: "Shared".into(),
        based_on: "SimpChineseDefault".into(),
        overrides: [(18, 23, false), (18, 25, true)]
            .into_iter()
            .map(|(target_class, side_class, after)| MojikumiAki {
                target_class,
                side_class,
                after,
                maximum: 10.0,
                priority: 1,
                ..Default::default()
            })
            .collect(),
    });
    let para = &mut d.story_mut(sid).unwrap().paras[0].para;
    para.mojikumi = Some("MojikumiTable/Shared".into());
    para.align = Some(Align::FullyJustified);
    let result = compose_story(&d, sid, &ComposeOptions::default());
    let line = all_lines(&result)[0];
    assert!((line.glyphs[0].x - extra / 2.0).abs() < 0.001);
    assert!((line.glyphs[1].x - old_second - extra).abs() < 0.001);
    assert!((line.end_x - 40.0).abs() < 0.001);
}

#[test]
fn mojikumi_nonfloating_and_leading_only_compression_fit_ragged_lines() {
    use designcraft_doc::cjk::{MojikumiAki, MojikumiTable};
    for composer in [Composer::SingleLine, Composer::Paragraph] {
        for leading in [true, false] {
            let (mut d, sid, _) = doc_with(
                "A",
                Rect::new(0.0, 0.0, 15.0, 100.0),
                ParaAttrs { composer: Some(composer), mojikumi: Some("MojikumiTable/Endpoint".into()), ..Default::default() },
            );
            Arc::make_mut(&mut d.styles).mojikumi_tables.push(MojikumiTable {
                name: "Endpoint".into(),
                based_on: "SimpChineseDefault".into(),
                overrides: vec![MojikumiAki {
                    target_class: 18,
                    side_class: if leading { 23 } else { 22 },
                    after: !leading,
                    minimum: 0.0,
                    desired: 1.0,
                    maximum: 1.0,
                    priority: 1,
                    does_not_float: true,
                }],
            });
            let result = compose_story(&d, sid, &ComposeOptions::default());
            let line = all_lines(&result)[0];
            assert!(line.end_x <= 15.0, "{composer:?}, leading={leading}: {}", line.end_x);
            assert!(line.glyphs[0].x.abs() < 0.001, "Discrete leading gap must reach its endpoint");
        }
    }
}

// ---------- span columns ----------

/// A story of `before` body paragraphs, a heading spanning all columns, and `after` body
/// paragraphs, in a three-column frame. Returns the heading's paragraph index.
fn span_doc(before: usize, after: usize, rect: Rect) -> (Document, StoryId, ItemId, usize) {
    let mut paras: Vec<&str> = vec![LOREM; before];
    paras.push("A heading across the columns");
    paras.extend(vec![LOREM; after]);
    let (mut d, sid, fid) = doc_with(&paras.join("\n"), rect, ParaAttrs::default());
    d.item_mut(fid).unwrap().text_frame_mut().unwrap().options.columns = 3;
    d.story_mut(sid).unwrap().paras[before].para.span_columns = Some(SpanColumns::Span(0));
    (d, sid, fid, before)
}

/// The line's leading slot (glyph ascent and descent may reach into the neighbouring lines).
fn line_box(l: &Line) -> (f64, f64) {
    (l.baseline - 0.75 * l.leading, l.baseline + 0.25 * l.leading)
}

/// No two lines of a frame share any area.
fn assert_no_overlap(ft: &FrameText) {
    for (i, a) in ft.lines.iter().enumerate() {
        for b in &ft.lines[i + 1..] {
            let ((at, ab), (bt, bb)) = (line_box(a), line_box(b));
            let x_overlap = a.x0 < b.x1 - 0.5 && b.x0 < a.x1 - 0.5;
            let y_overlap = at < bb - 0.5 && bt < ab - 0.5;
            assert!(
                !(x_overlap && y_overlap),
                "lines overlap: para {} col {} y {:.1}..{:.1} and para {} col {} y {:.1}..{:.1}",
                a.para,
                a.column,
                at,
                ab,
                b.para,
                b.column,
                bt,
                bb
            );
        }
    }
}

#[test]
fn mojikumi_places_custom_gap_after_combining_marks() {
    use designcraft_doc::cjk::{MojikumiAki, MojikumiTable};
    let (mut d, sid, _) = doc_with("A\u{0323}\u{0301}1", Rect::new(0.0, 0.0, 200.0, 100.0), ParaAttrs::default());
    let natural = compose_story(&d, sid, &ComposeOptions::default());
    let old = &all_lines(&natural)[0].glyphs;
    assert!(old.len() >= 3, "Exercise a cluster with a separate combining glyph");
    Arc::make_mut(&mut d.styles).mojikumi_tables.push(MojikumiTable {
        name: "Clusters".into(),
        based_on: "SimpChineseDefault".into(),
        overrides: vec![MojikumiAki {
            target_class: 18,
            side_class: 25,
            after: true,
            minimum: 1.0,
            desired: 1.0,
            maximum: 1.0,
            ..Default::default()
        }],
    });
    d.story_mut(sid).unwrap().paras[0].para.mojikumi = Some("MojikumiTable/Clusters".into());
    let result = compose_story(&d, sid, &ComposeOptions::default());
    let new = &all_lines(&result)[0].glyphs;
    assert_eq!(new.len(), old.len());
    for (before, after) in old.iter().zip(new) {
        let expected = if before.rendered_char == '1' { 12.0 } else { 0.0 };
        assert!((after.x - before.x - expected).abs() < 0.001, "Glyph {:?} moved incorrectly", before.rendered_char);
    }
}

#[test]
fn mojikumi_does_not_restore_fullwidth_blanks_to_narrow_tracked_punctuation() {
    let family = "Mojikumi Narrow Body";
    let font = designcraft_fonts::testing::font_mapping(family, &[('（', 'i'), ('A', 'A')]).unwrap();
    designcraft_fonts::FontDb::global().add_font(font);
    let (mut d, sid, _) = doc_with("（A", Rect::new(0.0, 0.0, 200.0, 100.0), ParaAttrs::default());
    d.story_mut(sid).unwrap().format_chars(0.."（A".len(), |f| {
        f.over.font_family = Some(family.into());
        f.over.tracking = Some(1000.0);
    });
    let natural = compose_story(&d, sid, &ComposeOptions::default());
    d.story_mut(sid).unwrap().paras[0].para.mojikumi = Some("SimpChineseDefault".into());
    let result = compose_story(&d, sid, &ComposeOptions::default());
    for (a, b) in all_lines(&natural)[0].glyphs.iter().zip(&all_lines(&result)[0].glyphs) {
        assert!((a.x - b.x).abs() < 0.001);
        assert!((a.adv - b.adv).abs() < 0.001);
    }
}

#[test]
fn mojikumi_one_em_line_end_set_sets_closing_punctuation_full_width_at_line_end() {
    let family = "Mojikumi Full Stop";
    let font = designcraft_fonts::testing::font_mapping(family, &[('（', '‰'), ('一', '‰'), ('。', '‰')]).unwrap();
    designcraft_fonts::FontDb::global().add_font(font);
    let text = "（一。";
    let (mut d, sid, _) = doc_with(text, Rect::new(0.0, 0.0, 200.0, 100.0), ParaAttrs::default());
    d.story_mut(sid).unwrap().format_chars(0..text.len(), |f| f.over.font_family = Some(family.into()));
    // Advances of the opening bracket at line start and the full stop at line end.
    let mut edges = |set: &str| {
        d.story_mut(sid).unwrap().paras[0].para.mojikumi = Some(set.into());
        let cs = compose_story(&d, sid, &ComposeOptions::default());
        let line = all_lines(&cs)[0];
        let adv = |c| line.glyphs.iter().find(|g| g.rendered_char == c).map(|g| (g.adv, g.face.units_per_em() * g.sx)).unwrap();
        (adv('（').0, adv('。').0, adv('。').1)
    };
    let (open, stop, em) = edges("Nothing");
    let half = edges("LineEndAllOneHalfEmEnum");
    let one = edges("LineEndAllOneEmEnum");
    assert!((open - half.0 - em * 0.5).abs() < 0.01 && (stop - half.1 - em * 0.5).abs() < 0.01, "half-em set: {half:?} of {open}/{stop}");
    assert!((open - one.0 - em * 0.5).abs() < 0.01 && (stop - one.1).abs() < 0.01, "one-em set: {one:?} of {open}/{stop}");
}

fn ruled(text: &str, rect: Rect, columns: u32, edit: impl FnOnce(&mut Document, StoryId)) -> (Document, ItemId, Vec<Rect>) {
    let (mut d, sid, fid) = doc_with(text, rect, ParaAttrs::default());
    {
        let o = &mut d.item_mut(fid).unwrap().text_frame_mut().unwrap().options;
        o.columns = columns;
        o.gutter = 12.0;
        o.inset = [10.0, 6.0, 20.0, 6.0];
        o.column_rule = true;
        o.column_rule_weight = 2.0;
        o.column_rule_color = "[Black]".into();
    }
    edit(&mut d, sid);
    let cs = compose_story(&d, sid, &ComposeOptions::default());
    let rules = cs.frames[0].decos.iter().filter(|dc| dc.color == "[Black]").map(|dc| dc.rect).collect();
    (d, fid, rules)
}

#[test]
fn column_rules_sit_in_the_gutters_inside_the_insets() {
    // Two columns in 0..300 with 6 pt side insets: the gutter is centred on 150.
    let (_, _, rules) = ruled("Text", Rect::new(0.0, 0.0, 300.0, 200.0), 2, |_, _| {});
    assert_eq!(rules.len(), 1, "{rules:?}");
    let r = rules[0];
    assert!((r.center().x - 150.0).abs() < 1e-9 && (r.width() - 2.0).abs() < 1e-9, "{r:?}");
    assert!((r.y0 - 10.0).abs() < 1e-9 && (r.y1 - 180.0).abs() < 1e-9, "{r:?}");
    // Three columns: one rule per gutter, each centred between its two columns.
    let (_, _, rules) = ruled("Text", Rect::new(0.0, 0.0, 300.0, 200.0), 3, |_, _| {});
    assert_eq!(rules.len(), 2, "{rules:?}");
    let w = (288.0 - 24.0) / 3.0;
    for (i, r) in rules.iter().enumerate() {
        let edge = 6.0 + w * (i + 1) as f64 + 12.0 * i as f64;
        assert!((r.center().x - (edge + 6.0)).abs() < 1e-9, "{i}: {r:?}");
    }
    // Off, or a single column: no rule.
    let (_, _, rules) = ruled("Text", Rect::new(0.0, 0.0, 300.0, 200.0), 1, |_, _| {});
    assert!(rules.is_empty());
    let (_, _, rules) = ruled("Text", Rect::new(0.0, 0.0, 300.0, 200.0), 2, |d, sid| {
        let fid = d.story(sid).unwrap().frames[0];
        d.item_mut(fid).unwrap().text_frame_mut().unwrap().options.column_rule = false;
    });
    assert!(rules.is_empty());
}

#[test]
fn column_rules_centre_on_unequal_gutters_and_ignore_bad_weights() {
    let cols = [Rect::new(200.0, 0.0, 300.0, 50.0), Rect::new(0.0, 0.0, 100.0, 50.0), Rect::new(110.0, 0.0, 180.0, 50.0)];
    let weight = |w: f64| TextFrameOptions { column_rule_weight: w, ..Default::default() };
    let rules = column_rule_rects(&cols, &[], &weight(1.0));
    let centres: Vec<f64> = rules.iter().map(|r| r.center().x).collect();
    assert_eq!(centres, [105.0, 190.0]);
    assert!(column_rule_rects(&cols, &[], &weight(f64::NAN)).is_empty());
    assert!(column_rule_rects(&cols, &[], &weight(-1.0)).is_empty());
    assert!((column_rule_rects(&cols, &[], &weight(1e12))[0].width() - 1000.0).abs() < 1e-9);
}

#[test]
fn column_rules_in_right_to_left_and_vertical_frames() {
    let (_, _, rules) = ruled("نص", Rect::new(0.0, 0.0, 300.0, 200.0), 2, |d, sid| {
        d.story_mut(sid).unwrap().direction = designcraft_doc::TextDirection::RightToLeft;
    });
    assert_eq!(rules.len(), 1);
    assert!((rules[0].center().x - 150.0).abs() < 1e-9, "{rules:?}");
    // Vertical type: the columns stack top to bottom, so the rule runs across the frame.
    let (d, fid, rules) = ruled("縦書き", Rect::new(100.0, 100.0, 400.0, 300.0), 2, |d, sid| d.story_mut(sid).unwrap().vertical = true);
    assert_eq!(rules.len(), 1);
    let r = d.text_xf(d.item(fid).unwrap()).transform_rect_bbox(rules[0]);
    // Text area: x 106..394, y 110..280; the gutter is centred on y 195.
    assert!((r.x0 - 106.0).abs() < 1e-6 && (r.x1 - 394.0).abs() < 1e-6, "{r:?}");
    assert!((r.center().y - 195.0).abs() < 1e-6 && (r.height() - 2.0).abs() < 1e-6, "{r:?}");
}

#[test]
fn column_rules_break_around_paragraphs_that_span_columns() {
    let text = format!("A heading that spans both columns\n{LOREM} {LOREM}");
    let (d, fid, rules) = ruled(&text, Rect::new(0.0, 0.0, 300.0, 400.0), 2, |d, sid| {
        d.story_mut(sid).unwrap().paras[0].para.span_columns = Some(designcraft_doc::SpanColumns::Span(0));
    });
    let sid = d.item(fid).unwrap().text_frame().unwrap().story;
    let cs = compose_story(&d, sid, &ComposeOptions::default());
    let span: Vec<&Line> = cs.frames[0].lines.iter().filter(|l| l.para == 0).collect();
    assert!(!span.is_empty() && span.iter().all(|l| l.x0 < 150.0 && l.x1 > 150.0), "the heading spans");
    let (top, bottom) = (span[0].baseline - span[0].ascent, span[span.len() - 1].baseline + span[span.len() - 1].descent);
    assert!(!rules.is_empty());
    for r in &rules {
        assert!(r.y1 <= top + 1e-9 || r.y0 >= bottom - 1e-9, "rule {r:?} cuts the span {top}..{bottom}");
    }
    // The rule resumes below the span and runs to the bottom of the columns.
    assert!(rules.iter().any(|r| (r.y0 - bottom).abs() < 1e-9 && (r.y1 - 380.0).abs() < 1e-9), "{rules:?}");
}

#[test]
fn column_rules_take_their_offset_insets_and_tint() {
    let (d, fid, rules) = ruled("Text", Rect::new(0.0, 0.0, 300.0, 200.0), 2, |d, sid| {
        let fid = d.story(sid).unwrap().frames[0];
        let o = &mut d.item_mut(fid).unwrap().text_frame_mut().unwrap().options;
        o.column_rule_offset = 3.0;
        o.column_rule_top_inset = 15.0;
        o.column_rule_bottom_inset = 25.0;
        o.column_rule_tint = 0.4;
    });
    // Gutter centre 150 moved 3 pt; columns 10..180 shortened to 25..155.
    assert_eq!(rules.len(), 1, "{rules:?}");
    let r = rules[0];
    assert!((r.center().x - 153.0).abs() < 1e-9 && (r.y0 - 25.0).abs() < 1e-9 && (r.y1 - 155.0).abs() < 1e-9, "{r:?}");
    let sid = d.item(fid).unwrap().text_frame().unwrap().story;
    let cs = compose_story(&d, sid, &ComposeOptions::default());
    assert!(cs.frames[0].decos.iter().any(|dc| dc.rect == r && (dc.tint - 0.4).abs() < 1e-6));
    // Insets that meet leave no rule.
    let (_, _, rules) = ruled("Text", Rect::new(0.0, 0.0, 300.0, 200.0), 2, |d, sid| {
        let fid = d.story(sid).unwrap().frames[0];
        let o = &mut d.item_mut(fid).unwrap().text_frame_mut().unwrap().options;
        o.column_rule_top_inset = 100.0;
        o.column_rule_bottom_inset = 100.0;
    });
    assert!(rules.is_empty(), "{rules:?}");
}

/// The heading spans the frame; text before it sits above it and text after it below it.
fn assert_span_layout(ft: &FrameText, h: usize) {
    let heading: Vec<&Line> = ft.lines.iter().filter(|l| l.para == h).collect();
    assert!(!heading.is_empty());
    let (c0, cn) = (ft.columns[0], ft.columns[ft.columns.len() - 1]);
    for l in &heading {
        assert!((l.x0 - c0.x0.min(cn.x0)).abs() < 1e-6 && (l.x1 - c0.x1.max(cn.x1)).abs() < 1e-6, "heading spans {}..{}", l.x0, l.x1);
    }
    let top = heading.iter().map(|l| line_box(l).0).fold(f64::INFINITY, f64::min);
    let bottom = heading.iter().map(|l| line_box(l).1).fold(f64::NEG_INFINITY, f64::max);
    for l in ft.lines.iter().filter(|l| l.para < h) {
        assert!(line_box(l).1 <= top + 0.5, "para {} col {} ends at {} below the heading top {top}", l.para, l.column, line_box(l).1);
    }
    for l in ft.lines.iter().filter(|l| l.para > h) {
        assert!(line_box(l).0 >= bottom - 0.5, "para {} col {} starts at {} above the heading bottom {bottom}", l.para, l.column, line_box(l).0);
    }
    assert_no_overlap(ft);
}

#[test]
fn text_after_a_spanning_paragraph_flows_below_it() {
    let (d, sid, _, h) = span_doc(2, 3, Rect::new(0.0, 0.0, 540.0, 300.0));
    let cs = compose_story(&d, sid, &ComposeOptions::default());
    assert!(!cs.is_overset());
    let ft = &cs.frames[0];
    assert_span_layout(ft, h);
    // The text before is balanced across the three columns above the heading.
    let before: Vec<&Line> = ft.lines.iter().filter(|l| l.para < h).collect();
    let per_col = |c: u32| before.iter().filter(|l| l.column == c).count();
    let counts = [per_col(0), per_col(1), per_col(2)];
    assert!(counts.iter().all(|&n| n > 0), "{counts:?}");
    assert!(counts.iter().max().unwrap() - counts.iter().min().unwrap() <= 1, "{counts:?}");
    // The text after starts right below the heading in every column, on the same baseline.
    let bottom = ft.lines.iter().filter(|l| l.para == h).map(|l| l.baseline).fold(f64::NEG_INFINITY, f64::max);
    let firsts: Vec<f64> = (0..3).map(|c| ft.lines.iter().find(|l| l.para > h && l.column == c).map(|l| l.baseline).unwrap()).collect();
    for b in &firsts {
        assert!(*b > bottom && *b < bottom + 40.0, "{firsts:?} after {bottom}");
    }
    assert!((firsts[1] - firsts[2]).abs() < 1e-6, "{firsts:?}");
}

#[test]
fn spanning_heading_kept_with_next_and_spanning_two_of_three_columns() {
    let (mut d, sid, _, h) = span_doc(2, 3, Rect::new(0.0, 0.0, 540.0, 300.0));
    d.story_mut(sid).unwrap().paras[h].para.keep_with_next = Some(2);
    let cs = compose_story(&d, sid, &ComposeOptions::default());
    assert!(!cs.is_overset());
    assert_span_layout(&cs.frames[0], h);
    // Spanning two of three columns: the third column beside the heading stays empty.
    d.story_mut(sid).unwrap().paras[h].para.span_columns = Some(SpanColumns::Span(2));
    let cs = compose_story(&d, sid, &ComposeOptions::default());
    let ft = &cs.frames[0];
    let heading = ft.lines.iter().find(|l| l.para == h).unwrap();
    assert!((heading.x0 - ft.columns[0].x0).abs() < 1e-6 && (heading.x1 - ft.columns[1].x1).abs() < 1e-6);
    assert_no_overlap(ft);
    let bottom = heading.baseline + 0.25 * heading.leading;
    assert!(ft.lines.iter().filter(|l| l.para > h).all(|l| line_box(l).0 >= bottom - 0.5));
    assert!((0..3).all(|c| ft.lines.iter().any(|l| l.para > h && l.column == c)));
}

#[test]
fn spanning_paragraph_at_the_top_and_end_of_a_frame() {
    let (d, sid, _, h) = span_doc(0, 4, Rect::new(0.0, 0.0, 540.0, 300.0));
    let cs = compose_story(&d, sid, &ComposeOptions::default());
    assert!(!cs.is_overset());
    let ft = &cs.frames[0];
    assert_span_layout(ft, h);
    assert!(ft.lines[0].para == h && ft.lines[0].baseline < 20.0);
    assert!((1..3).all(|c| ft.lines.iter().any(|l| l.column == c)));

    let (d, sid, _, h) = span_doc(3, 0, Rect::new(0.0, 0.0, 540.0, 300.0));
    let cs = compose_story(&d, sid, &ComposeOptions::default());
    assert!(!cs.is_overset());
    assert_span_layout(&cs.frames[0], h);
}

#[test]
fn text_after_a_spanning_paragraph_threads_into_the_next_frame() {
    let (mut d, sid, f1, h) = span_doc(2, 8, Rect::new(0.0, 0.0, 540.0, 200.0));
    let lid = d.default_layer();
    let (f2, _) = d.add_text_frame(SpreadRef::Doc(0), Rect::new(0.0, 300.0, 540.0, 700.0), lid, "", ParaFormat::default()).unwrap();
    d.item_mut(f2).unwrap().text_frame_mut().unwrap().options.columns = 3;
    d.thread(f1, f2).unwrap();
    let cs = compose_story(&d, sid, &ComposeOptions::default());
    assert!(!cs.is_overset(), "overset at {:?}", cs.overset_at);
    assert_span_layout(&cs.frames[0], h);
    let (a, b) = (&cs.frames[0], &cs.frames[1]);
    assert!((0..3).all(|c| a.lines.iter().any(|l| l.para > h && l.column == c)));
    assert!(!b.lines.is_empty() && b.lines[0].para > h && b.lines[0].baseline < 320.0);
    assert_no_overlap(b);
    // Every line of the story is set once, in order.
    let lines = all_lines(&cs);
    for w in lines.windows(2) {
        assert!(w[1].range.start >= w[0].range.end, "{:?} then {:?}", w[0].range, w[1].range);
    }
    assert_eq!(lines.last().unwrap().range.end, d.story(sid).unwrap().text.len());
}

#[test]
fn spanning_paragraph_in_rtl_and_vertical_frames() {
    let (mut d, sid, _, h) = span_doc(2, 4, Rect::new(0.0, 0.0, 540.0, 500.0));
    d.story_mut(sid).unwrap().direction = designcraft_doc::TextDirection::RightToLeft;
    let cs = compose_story(&d, sid, &ComposeOptions::default());
    assert!(!cs.is_overset());
    assert_span_layout(&cs.frames[0], h);

    let (mut d, sid, _, h) = span_doc(2, 4, Rect::new(0.0, 0.0, 540.0, 540.0));
    d.story_mut(sid).unwrap().vertical = true;
    let cs = compose_story(&d, sid, &ComposeOptions::default());
    assert!(!cs.frames[0].lines.is_empty());
    assert_no_overlap(&cs.frames[0]);
    assert!(cs.frames[0].lines.iter().any(|l| l.para == h));
}

/// A heading spanning the columns mid-frame: text above it is balanced over the columns, text
/// after it flows below it, and each gutter's rule stops above the heading and resumes below it.
#[test]
fn column_rules_break_around_a_spanning_paragraph_between_bands() {
    let (mut d, sid, fid, h) = span_doc(2, 4, Rect::new(0.0, 0.0, 450.0, 700.0));
    {
        let o = &mut d.item_mut(fid).unwrap().text_frame_mut().unwrap().options;
        o.column_rule = true;
        o.column_rule_weight = 1.0;
        o.column_rule_color = "[Black]".into();
    }
    let cs = compose_story(&d, sid, &ComposeOptions::default());
    let ft = &cs.frames[0];
    assert_no_overlap(ft);
    let span: Vec<&Line> = ft.lines.iter().filter(|l| l.para == h).collect();
    assert!(!span.is_empty());
    let top = span.iter().map(|l| l.baseline - l.ascent).fold(f64::INFINITY, f64::min);
    let bottom = span.iter().map(|l| l.baseline + l.descent).fold(f64::NEG_INFINITY, f64::max);
    assert!(ft.lines.iter().any(|l| l.para < h), "text above the heading");
    assert!(ft.lines.iter().any(|l| l.para > h && l.column > 0 && l.baseline > bottom), "text after the heading flows below it");
    let rules: Vec<Rect> = ft.decos.iter().filter(|dc| dc.color == "[Black]").map(|dc| dc.rect).collect();
    let mut gutters: Vec<f64> = rules.iter().map(|r| r.center().x).collect();
    gutters.sort_by(f64::total_cmp);
    gutters.dedup_by(|a, b| (*a - *b).abs() < 1e-6);
    assert_eq!(gutters.len(), 2, "{rules:?}");
    for x in gutters {
        let here: Vec<&Rect> = rules.iter().filter(|r| (r.center().x - x).abs() < 1e-6).collect();
        assert!(here.iter().all(|r| r.y1 <= top + 1e-9 || r.y0 >= bottom - 1e-9), "a rule cuts the heading {top}..{bottom}: {here:?}");
        assert!(here.iter().any(|r| r.y1 <= top + 1e-9), "rule above the heading at {x}: {here:?}");
        assert!(here.iter().any(|r| (r.y0 - bottom).abs() < 1e-9), "rule resumes below the heading at {x}: {here:?}");
    }
}

#[test]
fn justified_line_with_a_tab_justifies_the_text_after_its_last_tab() {
    // InDesign justifies only the text after a line's last (left) tab; tab stops stay aligned.
    let stop = |align| designcraft_doc::TabStop { position: 100.0, align, leader: String::new(), align_on: String::new() };
    let text = "Name\t42 and some words\u{2028}more";
    let x_of = |l: &Line, byte: usize| l.glyphs.iter().find(|g| g.byte == byte && g.len > 0).map(|g| g.x).unwrap();
    let left = ParaAttrs { align: Some(Align::LeftJustified), tabs: Some(vec![stop(TabAlign::Left)]), ..Default::default() };
    let (d, sid, _) = doc_with(text, Rect::new(0.0, 0.0, 300.0, 100.0), left.clone());
    let cs = compose_story(&d, sid, &ComposeOptions::default());
    let l = all_lines(&cs)[0];
    assert!((l.end_x - l.x1).abs() < 0.6, "line with a left tab ends at {} not {}", l.end_x, l.x1);
    assert!((x_of(l, 5) - 100.0).abs() < 0.5, "text after the tab starts at the stop: {}", x_of(l, 5));
    let (d, sid, _) = doc_with(text, Rect::new(0.0, 0.0, 300.0, 100.0), ParaAttrs { align: Some(Align::Left), ..left });
    let ragged = compose_story(&d, sid, &ComposeOptions::default());
    let r = all_lines(&ragged)[0];
    for byte in 0..4 {
        assert!((x_of(l, byte) - x_of(r, byte)).abs() < 1e-6, "text before the tab keeps its natural position");
    }

    // After a right tab the line stays as set.
    let right = ParaAttrs { align: Some(Align::LeftJustified), tabs: Some(vec![stop(TabAlign::Right)]), ..Default::default() };
    let (d, sid, _) = doc_with("Name\t42\u{2028}more", Rect::new(0.0, 0.0, 300.0, 100.0), right);
    let cs = compose_story(&d, sid, &ComposeOptions::default());
    let l = all_lines(&cs)[0];
    assert!((l.end_x - 100.0).abs() < 0.5, "right-tab line ends at its stop: {}", l.end_x);

    // A footnote's number and tab separator: its first line is justified like the others.
    let (mut d, sid, fid) = doc_with("Body text.", Rect::new(0.0, 0.0, 200.0, 400.0), ParaAttrs::default());
    let note = ParaFormat { para: ParaAttrs { align: Some(Align::LeftJustified), ..Default::default() }, ..Default::default() };
    d.story_mut(sid).unwrap().insert_note(4, LOREM, note);
    let cs = compose_story(&d, sid, &ComposeOptions::default());
    let n = &cs.frame(fid).unwrap().notes[0];
    let lines = all_lines(&n.text);
    assert!(lines.len() > 2);
    for l in &lines[..lines.len() - 1] {
        assert!((l.end_x - l.x1).abs() < 0.6, "footnote line ends at {} not {}", l.end_x, l.x1);
    }
    assert!((x_of(lines[0], 0) - 36.0).abs() < 0.5, "note text starts at the default tab stop: {}", x_of(lines[0], 0));
}

#[test]
fn mojikumi_justification_after_a_tab_keeps_the_text_before_it() {
    // Justification (mojikumi included) adjusts only the text after a line's last tab.
    use designcraft_doc::cjk::{MojikumiAki, MojikumiTable};
    let stop = designcraft_doc::TabStop { position: 100.0, align: TabAlign::Left, leader: String::new(), align_on: String::new() };
    let compose = |align| {
        let para =
            ParaAttrs { align: Some(align), mojikumi: Some("MojikumiTable/Elastic".into()), tabs: Some(vec![stop.clone()]), ..Default::default() };
        let (mut d, sid, _) = doc_with("A1A1\tA1A1", Rect::new(0.0, 0.0, 300.0, 100.0), para);
        Arc::make_mut(&mut d.styles).mojikumi_tables.push(MojikumiTable {
            name: "Elastic".into(),
            based_on: "SimpChineseDefault".into(),
            overrides: vec![MojikumiAki {
                target_class: 18,
                side_class: 25,
                after: true,
                minimum: 0.0,
                desired: 0.0,
                maximum: 5.0,
                priority: 1,
                ..Default::default()
            }],
        });
        compose_story(&d, sid, &ComposeOptions::default())
    };
    let justified = compose(Align::FullyJustified);
    let ragged = compose(Align::Left);
    let (l, r) = (all_lines(&justified)[0], all_lines(&ragged)[0]);
    assert!(l.end_x > r.end_x + 1.0, "the text after the tab stretches: {} vs {}", l.end_x, r.end_x);
    let x_of = |l: &Line, byte: usize| l.glyphs.iter().find(|g| g.byte == byte && g.len > 0).map(|g| g.x).unwrap();
    for byte in 0..5 {
        assert!((x_of(l, byte) - x_of(r, byte)).abs() < 1e-6, "byte {byte} before the tab moved: {} vs {}", x_of(l, byte), x_of(r, byte));
    }
}

#[test]
fn list_first_line_wraps_at_the_column_edge_after_its_label() {
    use designcraft_doc::{ListType, TabStop};
    // A hanging indent (left 18, first line −18): the label sits at the column start, a tab
    // after it reaches the left indent (an implicit stop when no explicit stop comes first), and
    // the first line wraps at the same right edge as the others.
    let right_edge = |l: &Line, source: &str| {
        let space = |g: &&PlacedGlyph| g.len > 0 && source.get(g.byte..).and_then(|s| s.chars().next()).is_some_and(char::is_whitespace);
        l.glyphs.iter().filter(|g| g.visible && !space(g)).map(|g| g.x + g.adv).fold(0.0, f64::max)
    };
    let text_start = |l: &Line| l.glyphs.iter().find(|g| g.len > 0 && g.visible).map_or(f64::NAN, |g| g.x);
    let stop = |position: f64| TabStop { position, align: TabAlign::Left, leader: String::new(), align_on: String::new() };
    let text = format!("{LOREM}\n{LOREM}");
    // (list, separator, explicit tab stops, where line 0's text starts past the column start:
    // None = right after the label, wherever that is).
    let cases = [
        (ListType::Bullets, "\t", None, Some(18.0)),
        (ListType::Numbers, "\t", None, Some(18.0)),
        (ListType::Numbers, "\t", Some(vec![stop(12.0)]), Some(12.0)),
        (ListType::Numbers, "\t", Some(vec![stop(30.0)]), Some(18.0)),
        (ListType::Bullets, " ", None, None),
        (ListType::Numbers, "\u{2003}", None, None),
    ];
    // Justified too: only the text after the tab stretches (#284).
    for (align, (list, sep, tabs, first_at)) in [Align::Left, Align::LeftJustified].into_iter().flat_map(|a| cases.clone().map(|c| (a, c))) {
        let para = ParaAttrs {
            align: Some(align),
            list_type: Some(list),
            list_separator: Some(sep.into()),
            left_indent: Some(18.0),
            first_line_indent: Some(-18.0),
            tabs: tabs.clone(),
            ..Default::default()
        };
        let (d, sid, _) = doc_with(&text, Rect::new(36.0, 36.0, 300.0, 400.0), para);
        let cs = compose_story(&d, sid, &ComposeOptions::default());
        let col = cs.frames[0].columns[0];
        let lines = &cs.frames[0].lines;
        let what = format!("{align:?} {list:?} {sep:?} {tabs:?}");
        assert!(lines.len() >= 6, "{what}: {} lines", lines.len());
        for (i, l) in lines.iter().enumerate() {
            let right = right_edge(l, &text);
            assert!(right <= col.x1 + 0.01, "{what}: line {i} ends at {right}, past the column's {}", col.x1);
            let start = text_start(l) - col.x0;
            if !l.first_in_para {
                assert!((start - 18.0).abs() < 0.01, "{what}: line {i} text starts at {start}, not the left indent");
            } else if let Some(at) = first_at {
                assert!((start - at).abs() < 0.01, "{what}: line {i} text starts at {start}, not {at}");
            } else {
                assert!(start > 0.0, "{what}: line {i} text starts at {start}");
            }
            if l.first_in_para {
                assert!(l.glyphs.first().is_some_and(|g| g.len == 0 && (g.x - col.x0).abs() < 0.01), "{what}: the label leads line {i}");
            }
        }
    }
}

/// Lines of `text` set in a frame `width` wide, with `tracking` on every character.
fn tracked_lines(text: &str, width: f64, para: ParaAttrs, tracking: f64) -> Vec<Line> {
    let (mut d, sid, _) = doc_with(text, Rect::new(0.0, 0.0, width, 4000.0), para);
    d.story_mut(sid).unwrap().format_chars(0..text.len(), |f| f.over.tracking = Some(tracking));
    let cs = compose_story(&d, sid, &ComposeOptions::default());
    assert!(!cs.is_overset(), "overset at width {width}");
    all_lines(&cs).into_iter().cloned().collect()
}

#[test]
fn a_word_longer_than_the_line_breaks_at_the_column_edge() {
    use designcraft_doc::Composer;
    let word = "n".repeat(60);
    let long = format!("Then {word} and on");
    for tracking in [0.0, 740.0] {
        // Natural advances, from one unbroken line.
        let natural: HashMap<usize, f64> =
            tracked_lines(&long, 1e5, ParaAttrs::default(), tracking)[0].glyphs.iter().map(|g| (g.byte, g.adv)).collect();
        for composer in [Composer::Paragraph, Composer::SingleLine] {
            for align in [Align::Left, Align::LeftJustified] {
                let para = ParaAttrs { composer: Some(composer), align: Some(align), hyphenate: Some(false), ..Default::default() };
                let lines = tracked_lines(&long, 72.0, para, tracking);
                let what = format!("{composer:?} {align:?} tracking {tracking}");
                assert!(lines.len() >= 4, "{what}: {} lines", lines.len());
                for l in &lines {
                    let ink: Vec<_> = l.glyphs.iter().filter(|g| g.visible && g.adv > 0.0).collect();
                    let right = ink.iter().map(|g| g.x + g.adv.min(natural[&g.byte])).fold(l.x0, f64::max);
                    assert!(l.end_x <= l.x1 + 0.02 && right <= l.x1 + 0.02, "{what}: line {:?} ends at {} past {}", l.range, l.end_x, l.x1);
                    for w in ink.windows(2) {
                        assert!(w[1].x >= w[0].x + natural[&w[0].byte] - 1e-6, "{what}: glyphs overlap at {}", w[1].byte);
                    }
                }
            }
        }
    }
}

#[test]
fn a_line_that_exactly_fills_the_measure_fits() {
    use designcraft_doc::Composer;
    let text = "#knowyourplastic";
    let one = &tracked_lines(text, 1e5, ParaAttrs::default(), 0.0)[0];
    // The measure a rounding error short of the natural width.
    let width = one.end_x - one.x0 - 0.005;
    for composer in [Composer::Paragraph, Composer::SingleLine] {
        for align in [Align::Left, Align::LeftJustified] {
            let para = ParaAttrs { composer: Some(composer), align: Some(align), ..Default::default() };
            let lines = tracked_lines(text, width, para, 0.0);
            assert_eq!(lines.len(), 1, "{composer:?} {align:?}: {:?}", lines.iter().map(|l| l.range.clone()).collect::<Vec<_>>());
        }
    }
}

// ---------- split columns ----------

/// A story of a body paragraph, `split` paragraphs in split columns and `after` body paragraphs.
/// Returns the index of the first split paragraph.
fn split_doc(split: usize, after: usize, rect: Rect, cfg: (u32, f64, f64)) -> (Document, StoryId, ItemId, usize) {
    let mut paras: Vec<&str> = vec![LOREM; 1 + split + after];
    paras[0] = "An introduction set at the full measure of the column, before the split block.";
    let (mut d, sid, fid) = doc_with(&paras.join("\n"), rect, ParaAttrs::default());
    let st = d.story_mut(sid).unwrap();
    for p in &mut st.paras[1..=split] {
        p.para.span_columns = Some(SpanColumns::Split(cfg.0));
        p.para.split_inside_gutter = Some(cfg.1);
        p.para.split_outside_gutter = Some(cfg.2);
    }
    (d, sid, fid, 1)
}

/// The sub-columns of `col`: (x0, x1) of each.
fn sub_columns(col: Rect, (n, inside, outside): (u32, f64, f64)) -> Vec<(f64, f64)> {
    let w = (col.width() - 2.0 * outside - inside * (n - 1) as f64) / n as f64;
    (0..n).map(|k| col.x0 + outside + k as f64 * (w + inside)).map(|x| (x, x + w)).collect()
}

/// The split paragraphs' lines, by sub-column.
fn split_lines<'a>(ft: &'a FrameText, paras: std::ops::Range<usize>, subs: &[(f64, f64)]) -> Vec<Vec<&'a Line>> {
    let mut by_sub = vec![Vec::new(); subs.len()];
    for l in ft.lines.iter().filter(|l| paras.contains(&l.para)) {
        let k = subs.iter().position(|&(x0, x1)| (l.x0 - x0).abs() < 1e-6 && (l.x1 - x1).abs() < 1e-6);
        let k = k.unwrap_or_else(|| panic!("line of para {} at {}..{} is in no sub-column of {subs:?}", l.para, l.x0, l.x1));
        by_sub[k].push(l);
    }
    by_sub
}

#[test]
fn split_paragraphs_are_set_in_balanced_sub_columns() {
    for cfg in [(2, 12.0, 10.0), (3, 6.0, 0.0)] {
        let (d, sid, _, s) = split_doc(2, 1, Rect::new(0.0, 0.0, 400.0, 700.0), cfg);
        let cs = compose_story(&d, sid, &ComposeOptions::default());
        assert!(!cs.is_overset());
        let ft = &cs.frames[0];
        assert_no_overlap(ft);
        let by_sub = split_lines(ft, s..s + 2, &sub_columns(ft.columns[0], cfg));
        let counts: Vec<usize> = by_sub.iter().map(Vec::len).collect();
        assert!(counts.iter().all(|&n| n > 0), "{cfg:?}: {counts:?}");
        assert!(counts.iter().max().unwrap() - counts.iter().min().unwrap() <= 1, "{cfg:?}: balanced {counts:?}");
        // Every sub-column starts on the same baseline, below the paragraph before the block.
        let firsts: Vec<f64> = by_sub.iter().map(|v| v[0].baseline).collect();
        assert!(firsts.iter().all(|b| (b - firsts[0]).abs() < 1e-6), "{cfg:?}: {firsts:?}");
        let intro = ft.lines.iter().filter(|l| l.para == 0).map(|l| line_box(l).1).fold(f64::NEG_INFINITY, f64::max);
        assert!(firsts[0] > intro && firsts[0] < intro + 20.0, "{cfg:?}: block at {} after {intro}", firsts[0]);
        // The text after the block continues at the full measure below its deepest sub-column.
        let bottom = by_sub.iter().flatten().map(|l| line_box(l).1).fold(f64::NEG_INFINITY, f64::max);
        let next = ft.lines.iter().find(|l| l.para == s + 2).unwrap();
        assert!(line_box(next).0 >= bottom - 0.5 && line_box(next).0 < bottom + 20.0, "{cfg:?}: {} after {bottom}", line_box(next).0);
        assert!((next.x0 - ft.columns[0].x0).abs() < 1e-6 && (next.x1 - ft.columns[0].x1).abs() < 1e-6);
    }
}

#[test]
fn split_block_in_the_second_column_and_at_the_story_end() {
    let cfg = (2, 12.0, 0.0);
    let (mut d, sid, fid, s) = split_doc(3, 0, Rect::new(0.0, 0.0, 540.0, 400.0), cfg);
    d.item_mut(fid).unwrap().text_frame_mut().unwrap().options.columns = 2;
    d.story_mut(sid).unwrap().paras[s].para.start_paragraph = Some(StartParagraph::NextColumn);
    let cs = compose_story(&d, sid, &ComposeOptions::default());
    assert!(!cs.is_overset());
    let ft = &cs.frames[0];
    assert_no_overlap(ft);
    assert!(ft.lines.iter().filter(|l| l.para >= s).all(|l| l.column == 1));
    let by_sub = split_lines(ft, s..s + 3, &sub_columns(ft.columns[1], cfg));
    // Nothing follows the block: it fills its first sub-column to the bottom of the frame, then
    // the next.
    let (a, b) = (&by_sub[0], &by_sub[1]);
    assert!(!a.is_empty() && !b.is_empty() && a.len() > b.len() + 1, "{} and {} lines", a.len(), b.len());
    let low = a.last().unwrap();
    assert!(low.baseline + low.descent > 400.0 - low.leading, "the first sub-column ends at {}", low.baseline);
    assert!((a[0].baseline - b[0].baseline).abs() < 1e-6);
}

#[test]
fn split_block_flows_into_the_next_column() {
    let cfg = (2, 12.0, 0.0);
    let (mut d, sid, fid, s) = split_doc(6, 1, Rect::new(0.0, 0.0, 540.0, 200.0), cfg);
    d.item_mut(fid).unwrap().text_frame_mut().unwrap().options.columns = 2;
    let cs = compose_story(&d, sid, &ComposeOptions::default());
    let ft = &cs.frames[0];
    assert_no_overlap(ft);
    for c in 0..2 {
        let in_col: Vec<&Line> = ft.lines.iter().filter(|l| l.column == c as u32).collect();
        let lines: Vec<&Line> = in_col.iter().copied().filter(|l| (s..s + 6).contains(&l.para)).collect();
        let subs = sub_columns(ft.columns[c], cfg);
        assert!(subs.iter().all(|&(x0, x1)| lines.iter().any(|l| (l.x0 - x0).abs() < 1e-6 && (l.x1 - x1).abs() < 1e-6)), "column {c}");
    }
    // The block continues at the top of the second column.
    let top = ft.lines.iter().find(|l| l.column == 1).unwrap();
    assert!((s..s + 6).contains(&top.para) && top.baseline < 20.0);
}

/// Each paragraph's list label as text: its label glyphs (laid before the paragraph's own text)
/// matched back to characters of the label font; tabs and other invisible glyphs are skipped.
fn list_labels(cs: &ComposedStory, alphabet: &str) -> Vec<String> {
    all_lines(cs)
        .iter()
        .filter(|l| l.first_in_para)
        .map(|l| {
            l.glyphs
                .iter()
                .take_while(|g| g.len == 0)
                .filter(|g| g.visible)
                .map(|g| alphabet.chars().find(|c| g.face.glyph_for(*c) == g.gid).unwrap_or('?'))
                .collect()
        })
        .collect()
}

const LABEL_CHARS: &str = "0123456789ABCDIVXabcdivx.\u{2022} Tabel";

fn numbered(style: designcraft_doc::NumberStyle, expression: &str) -> ParaAttrs {
    ParaAttrs {
        list_type: Some(designcraft_doc::ListType::Numbers),
        number_style: Some(style),
        number_expression: Some(expression.into()),
        ..Default::default()
    }
}

#[test]
fn numbered_lists_follow_their_format_and_expression() {
    use designcraft_doc::NumberStyle as N;
    let cases: [(N, &str, [&str; 3]); 4] = [
        (N::UpperLetters, "^#.^t", ["A.", "B.", "C."]),
        (N::LowerRoman, "^#.^t", ["i.", "ii.", "iii."]),
        (N::ArabicThreeDigits, "^#.^t", ["001.", "002.", "003."]),
        (N::Arabic, "Tabel ^#^t", ["Tabel 1", "Tabel 2", "Tabel 3"]),
    ];
    for (style, expression, want) in cases {
        let (d, sid, _) = doc_with("One\nTwo\nThree", Rect::new(0.0, 0.0, 300.0, 300.0), numbered(style, expression));
        let cs = compose_story(&d, sid, &ComposeOptions::default());
        assert_eq!(list_labels(&cs, LABEL_CHARS), want, "{style:?} {expression}");
    }
}

#[test]
fn numbering_from_the_largest_start_number_does_not_overflow() {
    let (mut d, sid, _) = doc_with("One\nTwo", Rect::new(0.0, 0.0, 300.0, 300.0), numbered(designcraft_doc::NumberStyle::Arabic, "^#."));
    d.story_mut(sid).unwrap().paras[0].para.start_at = Some(Some(u32::MAX));
    let cs = compose_story(&d, sid, &ComposeOptions::default());
    let max = format!("{}.", u32::MAX);
    assert_eq!(list_labels(&cs, LABEL_CHARS), [max.clone(), max]);
}

#[test]
fn empty_paragraphs_get_no_bullet_or_number() {
    let bullets = ParaAttrs { list_type: Some(designcraft_doc::ListType::Bullets), ..Default::default() };
    let (d, sid, _) = doc_with("One\n\nTwo\n", Rect::new(0.0, 0.0, 300.0, 300.0), bullets);
    let cs = compose_story(&d, sid, &ComposeOptions::default());
    assert_eq!(list_labels(&cs, LABEL_CHARS), ["\u{2022}", "", "\u{2022}", ""]);
    // An empty paragraph doesn't use up a number, in a story's own list or a named one.
    for name in ["", "Steps"] {
        let attrs = ParaAttrs { list_name: Some(name.into()), ..numbered(designcraft_doc::NumberStyle::Arabic, "^#.^t") };
        let (mut d, sid, _) = doc_with("One\n\nTwo\n", Rect::new(0.0, 0.0, 300.0, 300.0), attrs);
        if !name.is_empty() {
            d.settings.lists.push(designcraft_doc::NumberedList { name: name.into(), continue_across_stories: false });
        }
        let cs = compose_story(&d, sid, &ComposeOptions::default());
        assert_eq!(list_labels(&cs, LABEL_CHARS), ["1.", "", "2.", ""], "list `{name}`");
    }
}

#[test]
fn tight_word_spacing_does_not_isolate_a_word() {
    // A narrow word-spacing range (100–105%) with some letter spacing and glyph scaling, no
    // hyphenation: lines that need more stretch must open their word spaces, not letter-space a
    // single word across the measure.
    let para = ParaAttrs {
        align: Some(Align::LeftJustified),
        hyphenate: Some(false),
        first_line_indent: Some(9.0),
        word_space_min: Some(1.0),
        word_space_desired: Some(1.03),
        word_space_max: Some(1.05),
        letter_space_min: Some(-0.05),
        letter_space_max: Some(0.05),
        glyph_scale_min: Some(0.99),
        glyph_scale_max: Some(1.01),
        ..Default::default()
    };
    // Short words: any two fit on a line together.
    let text = CORPUS[..2].join(" ");
    for w in (200..=320).step_by(5) {
        let (d, sid, _) = doc_with(&text, Rect::new(0.0, 0.0, w as f64, 20000.0), para.clone());
        let cs = compose_story(&d, sid, &ComposeOptions::default());
        let lines = all_lines(&cs);
        for l in &lines[..lines.len() - 1] {
            let words = text[l.range.clone()].split_whitespace().count();
            assert!(words > 1, "width {w}: a single-word line {:?}", &text[l.range.clone()]);
        }
    }
}

#[test]
fn fixed_width_spaces_do_not_stretch_on_justified_lines() {
    // Long words in a narrow column: lines stretch past the maximum word spacing. Only the word
    // spaces take that stretch; em, en and thin spaces keep their width.
    let words = ["characteristically", "misunderstanding", "unquestionably", "responsibility", "administration", "photographer"];
    let seps = [" ", "\u{2003}", " ", "\u{2002}", " ", "\u{2009}"];
    let text: String = (0..60).map(|i| format!("{}{}", words[i % words.len()], seps[i % seps.len()])).collect();
    let text = text.trim_end().to_string();
    let fixed = |c: char| matches!(c, '\u{2003}' | '\u{2002}' | '\u{2009}');
    // (character, advance) of each fixed space, per line, and whether the line ends the paragraph.
    let spaces = |align: Align, w: f64| -> Vec<(Vec<(char, f64)>, bool)> {
        let para = ParaAttrs { align: Some(align), hyphenate: Some(false), ..Default::default() };
        let (d, sid, _) = doc_with(&text, Rect::new(0.0, 0.0, w, 20000.0), para);
        let cs = compose_story(&d, sid, &ComposeOptions::default());
        all_lines(&cs)
            .iter()
            .map(|l| {
                let found = l.glyphs.iter().filter_map(|g| text[g.byte..].chars().next().filter(|&c| fixed(c)).map(|c| (c, g.adv))).collect();
                (found, l.last_in_para)
            })
            .collect()
    };
    let mut checked = 0;
    for w in [190.0, 230.0, 270.0] {
        let natural: Vec<(char, f64)> = spaces(Align::Left, w).into_iter().flat_map(|l| l.0).collect();
        for (found, last) in spaces(Align::LeftJustified, w) {
            if last {
                continue;
            }
            for (c, a) in found {
                let Some(&(_, n)) = natural.iter().find(|x| x.0 == c) else { continue };
                assert!((a - n).abs() < 1e-6, "width {w}: U+{:04X} is {a:.2} wide, not {n:.2}", c as u32);
                checked += 1;
            }
        }
    }
    assert!(checked > 5, "{checked} fixed spaces on justified lines");
}

#[test]
fn grep_style_in_the_documents_dialect_keeps_short_words_with_the_next() {
    // `\l` (lowercase letter) and `\h` (horizontal space) aren't Rust regex syntax; the style used
    // to be skipped silently.
    let text = "Мы и он в доме и в саду, а она у окна и с книгой в руках у стола.";
    let para = ParaAttrs {
        align: Some(Align::Left),
        grep_styles: Some(vec![designcraft_doc::GrepStyle { style: "nobreak".into(), pattern: r"\b\l{1,2}\h".into() }]),
        ..Default::default()
    };
    let (mut d, sid, _) = doc_with(text, Rect::new(0.0, 0.0, 70.0, 1000.0), para);
    let chars = designcraft_doc::CharAttrs { no_break: Some(true), ..Default::default() };
    std::sync::Arc::make_mut(&mut d.styles).character.push(designcraft_doc::CharacterStyle {
        name: "nobreak".into(),
        based_on: None,
        chars,
        shortcut: String::new(),
    });
    let cs = compose_story(&d, sid, &ComposeOptions::default());
    let lines = all_lines(&cs);
    assert!(lines.len() > 3);
    let short = |w: &str| w.chars().count() <= 2 && w.chars().all(char::is_lowercase);
    for l in &lines[..lines.len() - 1] {
        let last = text[l.range.clone()].split_whitespace().last().unwrap_or("");
        assert!(!short(last), "line ends with a short word: {:?}", &text[l.range.clone()]);
    }
}

// ---------- paragraph border and shading ----------

fn bordered(text: &str, rect: Rect, para: ParaAttrs) -> ComposedStory {
    let para = ParaAttrs { border_on: Some(true), border_weights: Some([1.0; 4]), ..para };
    let (d, sid, _) = doc_with(text, rect, para);
    compose_story(&d, sid, &ComposeOptions::default())
}

fn has_curves(bp: &designcraft_geom::BezPath) -> bool {
    bp.elements().iter().any(|e| matches!(e, designcraft_geom::PathEl::CurveTo(..) | designcraft_geom::PathEl::QuadTo(..)))
}

#[test]
fn rounded_border_corners_are_curves_and_dotted_borders_paint_their_gap_first() {
    use designcraft_geom::corners::{Corner, CornerShape};
    let rounded = [Corner { shape: CornerShape::Rounded, size: 6.0 }; 4];
    let cs = bordered("Boxed text", Rect::new(0.0, 0.0, 300.0, 200.0), ParaAttrs { border_corners: Some(rounded), ..Default::default() });
    let paths: Vec<_> = cs.frames[0].decos.iter().filter_map(|dc| dc.path.as_ref()).collect();
    assert_eq!(paths.len(), 1, "one stroked outline");
    assert!(has_curves(paths[0]));
    // Square corners: plain edges, no curves.
    let cs = bordered("Boxed text", Rect::new(0.0, 0.0, 300.0, 200.0), ParaAttrs::default());
    assert!(cs.frames[0].decos.iter().all(|dc| dc.path.is_none()));
    // A dotted border with a gap colour: the gap under the dots.
    let dotted = ParaAttrs { border_type: Some(designcraft_doc::StrokeType::Dotted), border_gap_color: Some("[Paper]".into()), ..Default::default() };
    let cs = bordered("Boxed text", Rect::new(0.0, 0.0, 300.0, 200.0), dotted);
    let colors: Vec<&str> = cs.frames[0].decos.iter().filter(|dc| dc.path.is_some()).map(|dc| dc.color.as_str()).collect();
    assert_eq!(colors, ["[Paper]", "[Black]"]);
}

/// The width of the widest border edge (the top and bottom edges span the box).
fn border_width(cs: &ComposedStory) -> f64 {
    cs.frames[0].decos.iter().filter(|dc| dc.color == "[Black]").map(|dc| dc.rect.width()).fold(0.0, f64::max)
}

#[test]
fn a_text_width_border_is_narrower_than_a_column_width_one() {
    let rect = Rect::new(0.0, 0.0, 300.0, 200.0);
    let column = border_width(&bordered("Short", rect, ParaAttrs::default()));
    let text = border_width(&bordered("Short", rect, ParaAttrs { border_width: Some(designcraft_doc::BoxWidth::Text), ..Default::default() }));
    assert!((column - 302.0).abs() < 1e-6, "column and both side strokes: {column}");
    assert!(text > 2.0 && text < 60.0, "text: {text}");
}

/// Edges of a plain border by frame column: (column, count).
fn edges_per_column(cs: &ComposedStory) -> Vec<usize> {
    let cols = &cs.frames[0].columns;
    cols.iter()
        .map(|c| {
            cs.frames[0].decos.iter().filter(|dc| dc.color == "[Black]" && dc.rect.center().x > c.x0 - 2.0 && dc.rect.center().x < c.x1 + 2.0).count()
        })
        .collect()
}

#[test]
fn a_split_paragraph_closes_its_border_in_each_column_only_when_set_to() {
    let text = format!("{LOREM} {LOREM}");
    let compose = |display: bool| {
        let para = ParaAttrs { border_on: Some(true), border_weights: Some([1.0; 4]), border_display_if_splits: Some(display), ..Default::default() };
        let (mut d, sid, fid) = doc_with(&text, Rect::new(0.0, 0.0, 400.0, 90.0), para);
        d.item_mut(fid).unwrap().text_frame_mut().unwrap().options.columns = 2;
        compose_story(&d, sid, &ComposeOptions::default())
    };
    let open = compose(false);
    assert!(open.frames[0].lines.iter().any(|l| l.column == 1), "the paragraph reaches the second column");
    // Open at the split: top and sides in the first column, sides and bottom in the second.
    assert_eq!(edges_per_column(&open), [3, 3]);
    assert_eq!(edges_per_column(&compose(true)), [4, 4]);
}

#[test]
fn consecutive_paragraphs_with_the_same_border_share_one_box() {
    let compose = |merge: bool| {
        let para = ParaAttrs { space_after: Some(10.0), border_merge: Some(merge), ..Default::default() };
        bordered("One\nTwo\nThree", Rect::new(0.0, 0.0, 300.0, 300.0), para)
    };
    let merged = compose(true);
    let edges: Vec<Rect> = merged.frames[0].decos.iter().filter(|dc| dc.color == "[Black]").map(|dc| dc.rect).collect();
    assert_eq!(edges.len(), 4, "one box: {edges:?}");
    let lines = &merged.frames[0].lines;
    let (first, last) = (&lines[0], &lines[lines.len() - 1]);
    let left = edges.iter().find(|r| r.height() > r.width()).unwrap();
    assert!((left.y0 - (first.baseline - first.ascent)).abs() < 1e-6 && (left.y1 - (last.baseline + last.descent)).abs() < 1e-6, "{left:?}");
    assert_eq!(compose(false).frames[0].decos.iter().filter(|dc| dc.color == "[Black]").count(), 12, "a box each");
}

#[test]
fn shading_clips_to_the_frame_and_can_stay_off_print() {
    let para = ParaAttrs {
        shading_on: Some(true),
        shading_offsets: Some([50.0; 4]),
        shading_clip: Some(true),
        shading_nonprinting: Some(true),
        ..Default::default()
    };
    let (d, sid, _) = doc_with("Shaded", Rect::new(0.0, 0.0, 300.0, 200.0), para);
    let cs = compose_story(&d, sid, &ComposeOptions::default());
    let area = cs.frames[0].columns[0];
    let shade = &cs.frames[0].decos[0];
    assert!(shade.nonprinting);
    assert!(shade.rect.x0 >= area.x0 - 1e-6 && shade.rect.y0 >= area.y0 - 1e-6 && shade.rect.x1 <= area.x1 + 1e-6, "{:?} in {area:?}", shade.rect);
}

// ---------- span space, bullets and numbering ----------

#[test]
fn space_before_and_after_a_span_moves_the_span_and_the_text_below() {
    let at = |before: f64, after: f64| {
        let (mut d, sid, _, h) = span_doc(1, 1, Rect::new(0.0, 0.0, 600.0, 800.0));
        let p = &mut d.story_mut(sid).unwrap().paras[h].para;
        p.span_space_before = Some(before);
        p.span_space_after = Some(after);
        let cs = compose_story(&d, sid, &ComposeOptions::default());
        let first = |pi: usize| cs.frames[0].lines.iter().find(|l| l.para == pi).map(|l| l.baseline).unwrap();
        (first(h), first(h + 1))
    };
    let (span0, below0) = at(0.0, 0.0);
    let (span1, below1) = at(24.0, 0.0);
    assert!((span1 - span0 - 24.0).abs() < 0.01, "{span0} → {span1}");
    assert!((below1 - below0 - 24.0).abs() < 0.01, "the text below moves with it");
    let (span2, below2) = at(0.0, 18.0);
    assert!((span2 - span0).abs() < 0.01 && (below2 - below0 - 18.0).abs() < 0.01, "{below0} → {below2}");
}

#[test]
fn span_space_applies_only_where_the_paragraph_spans_columns() {
    // Every paragraph spans with 12 pt span space after; the frame has `columns` columns.
    let tops = |columns: u32, span: bool| {
        let (mut d, sid, fid) = doc_with("One\nTwo\nThree", Rect::new(0.0, 0.0, 400.0, 400.0), ParaAttrs::default());
        d.item_mut(fid).unwrap().text_frame_mut().unwrap().options.columns = columns;
        for p in &mut d.story_mut(sid).unwrap().paras {
            if span {
                p.para.span_columns = Some(SpanColumns::Span(0));
            }
            p.para.span_space_after = Some(12.0);
        }
        let cs = compose_story(&d, sid, &ComposeOptions::default());
        first_lines(&cs).iter().map(|l| l.baseline).collect::<Vec<_>>()
    };
    let plain = tops(1, false);
    assert_eq!(plain.len(), 3);
    assert_eq!(tops(1, true), plain, "a span in a one-column frame adds no space");
    let spread = tops(3, true);
    let gap = |v: &[f64], i: usize| v[i + 1] - v[i];
    assert!((gap(&spread, 0) - gap(&plain, 0) - 12.0).abs() < 0.01, "{spread:?} vs {plain:?}");
}

/// The first line of each paragraph.
fn first_lines(cs: &ComposedStory) -> Vec<&Line> {
    all_lines(cs).into_iter().filter(|l| l.first_in_para).collect()
}

#[test]
fn a_right_aligned_number_ends_at_the_line_start() {
    use designcraft_doc::{ListAlign, ListType};
    let compose = |align: ListAlign| {
        let para = ParaAttrs {
            list_type: Some(ListType::Numbers),
            number_align: Some(align),
            left_indent: Some(36.0),
            first_line_indent: Some(-18.0),
            ..Default::default()
        };
        let (d, sid, _) = doc_with("Item", Rect::new(0.0, 0.0, 300.0, 200.0), para);
        compose_story(&d, sid, &ComposeOptions::default())
    };
    // The label "1.\t": its first two glyphs are the number and the full stop.
    let cs = compose(ListAlign::Left);
    let col = cs.frames[0].columns[0];
    let l = first_lines(&cs)[0];
    assert!((l.glyphs[0].x - (col.x0 + 18.0)).abs() < 0.01, "left: starts at the line start");
    let text_x = l.glyphs.iter().find(|g| g.len > 0).unwrap().x;
    let cs = compose(ListAlign::Right);
    let l = first_lines(&cs)[0];
    let end = l.glyphs[1].x + l.glyphs[1].adv;
    assert!((end - (col.x0 + 18.0)).abs() < 0.01, "right: ends at the line start ({end})");
    assert!((l.glyphs.iter().find(|g| g.len > 0).unwrap().x - text_x).abs() < 0.01, "the text stays at the tab stop");
}

#[test]
fn level_two_numbering_restarts_after_level_one() {
    let para = numbered(designcraft_doc::NumberStyle::Arabic, "^1.^#");
    let (mut d, sid, _) = doc_with("A\nB\nC\nD\nE", Rect::new(0.0, 0.0, 300.0, 300.0), para);
    for (p, level) in d.story_mut(sid).unwrap().paras.iter_mut().zip([1, 2, 2, 1, 2]) {
        p.para.list_level = Some(level);
    }
    let cs = compose_story(&d, sid, &ComposeOptions::default());
    // `^1` is level 1's number, `^#` the paragraph's own level.
    assert_eq!(list_labels(&cs, LABEL_CHARS), ["1.1", "1.1", "1.2", "2.2", "2.1"]);
    // Without the restart, level 2 carries on.
    for p in &mut d.story_mut(sid).unwrap().paras {
        p.para.restart_numbers = Some(false);
    }
    let cs = compose_story(&d, sid, &ComposeOptions::default());
    assert_eq!(list_labels(&cs, LABEL_CHARS), ["1.1", "1.1", "1.2", "2.2", "2.3"]);
}

#[test]
fn the_tab_position_places_the_text_after_the_label() {
    use designcraft_doc::ListType;
    let compose = |tab: Option<f64>| {
        let para = ParaAttrs { list_type: Some(ListType::Bullets), list_tab: Some(tab), ..Default::default() };
        let (d, sid, _) = doc_with("Item", Rect::new(0.0, 0.0, 300.0, 200.0), para);
        let cs = compose_story(&d, sid, &ComposeOptions::default());
        let col = cs.frames[0].columns[0];
        first_lines(&cs)[0].glyphs.iter().find(|g| g.len > 0).unwrap().x - col.x0
    };
    assert!((compose(Some(50.0)) - 50.0).abs() < 0.01);
    assert!((compose(None) - 36.0).abs() < 0.01, "the default tab stop");
}

#[test]
fn paragraph_mark_size_counts_in_its_line_auto_leading() {
    let gaps = |mark_size: f64| {
        let (mut d, sid, _) = doc_with("One\nTwo\nThree", Rect::new(36.0, 36.0, 300.0, 300.0), ParaAttrs::default());
        d.story_mut(sid).unwrap().format_chars(7..8, |f| f.over.size = Some(mark_size));
        let cs = compose_story(&d, sid, &ComposeOptions::default());
        let b: Vec<f64> = all_lines(&cs).iter().map(|l| l.baseline).collect();
        assert_eq!(b.len(), 3);
        (b[1] - b[0], b[2] - b[1])
    };
    // A 12 pt line ending in a 14 pt return: 14 × 120% above it, the next line unchanged.
    let (above, below) = gaps(14.0);
    assert!((above - 16.8).abs() < 1e-6, "{above}");
    assert!((below - 14.4).abs() < 1e-6, "{below}");
    let (above, below) = gaps(12.0);
    assert!((above - 14.4).abs() < 1e-6, "{above}");
    assert!((below - 14.4).abs() < 1e-6, "{below}");
}

// Kenten settings, shatai, auto tate-chu-yoko, rotate roman, roman word break and grid settings.

/// The centre of a placed glyph's ink, relative to its line's baseline.
fn ink_centre(g: &PlacedGlyph) -> Point {
    let r = designcraft_geom::Shape::bounding_box(&*FontDb::global().outline(&g.face, g.gid));
    Point::new(g.x + (r.x0 + r.x1) / 2.0 * g.sx, g.y + (r.y0 + r.y1) / 2.0 * g.sy)
}

#[test]
fn kenten_sit_above_or_below_at_their_size_and_distance() {
    use designcraft_doc::cjk_settings::{KentenKind, KentenPosition};
    let (mut d, sid, _) = doc_with("AB", Rect::new(36.0, 36.0, 300.0, 100.0), ParaAttrs::default());
    let plain = compose_story(&d, sid, &ComposeOptions::default()).frames[0].lines[0].glyphs.len();
    let set = |d: &mut Document, position: KentenPosition| {
        d.story_mut(sid).unwrap().format_chars(0..2, |f| {
            f.over.kenten = Some(true);
            f.over.kenten_kind = Some(KentenKind::BlackCircle);
            f.over.kenten_size = Some(Some(8.0));
            f.over.kenten_x_scale = Some(1.5);
            f.over.kenten_distance = Some(2.0);
            f.over.kenten_position = Some(position);
        });
    };
    for position in [KentenPosition::AboveRight, KentenPosition::BelowLeft] {
        set(&mut d, position);
        let cs = compose_story(&d, sid, &ComposeOptions::default());
        let l = &cs.frames[0].lines[0];
        let marks = &l.glyphs[plain..];
        assert_eq!(marks.len(), 2, "one mark per character");
        for (b, m) in l.glyphs[..plain].iter().zip(marks) {
            // 8 pt, 150% wide, in a run style of its own.
            assert!((m.sx - 8.0 / m.face.units_per_em() * 1.5).abs() < 1e-9 && (m.sy - 8.0 / m.face.units_per_em()).abs() < 1e-9);
            assert_ne!(m.style, b.style);
            assert_eq!(cs.styles[m.style as usize].size, 8.0);
            let (top, bottom) = b.face.em_box();
            let want_y = match position {
                KentenPosition::AboveRight => b.y - top * b.sy - 2.0 - 4.0,
                KentenPosition::BelowLeft => b.y - bottom * b.sy + 2.0 + 4.0,
            };
            let c = ink_centre(m);
            assert!((c.y - want_y).abs() < 1e-6, "{position:?}: {} vs {want_y}", c.y);
            assert!((c.x - (b.x + b.adv / 2.0)).abs() < 1e-6, "centred on its character");
        }
    }
}

#[test]
fn kenten_colour_is_its_own_run_style() {
    let (mut d, sid, _) = doc_with("AB", Rect::new(36.0, 36.0, 300.0, 100.0), ParaAttrs::default());
    d.story_mut(sid).unwrap().format_chars(0..2, |f| {
        f.over.kenten = Some(true);
        f.over.kenten_fill = Some("[Registration]".into());
        f.over.kenten_fill_tint = Some(Some(0.5));
    });
    let cs = compose_story(&d, sid, &ComposeOptions::default());
    let l = &cs.frames[0].lines[0];
    let mark = l.glyphs.last().unwrap();
    let st = &cs.styles[mark.style as usize];
    assert_eq!((st.fill.as_str(), st.fill_tint), ("[Registration]", 0.5));
    assert!(st.kenten_mark.is_none() && !st.underline);
    assert_eq!(cs.styles[l.glyphs[0].style as usize].fill, "[Black]", "the text keeps its colour");
}

#[test]
fn shatai_matches_the_measured_matrices() {
    use crate::shatai::Shatai;
    let close = |a: [f64; 4], b: [f64; 4]| a.iter().zip(b).all(|(x, y)| (x - y).abs() < 1e-3);
    let plain = Shatai { magnification: 0.2, angle: 60.0, adjust_rotation: false, adjust_tsume: false };
    assert!(close(plain.linear(true), [0.850, 0.0866, 0.0866, 0.950]), "{:?}", plain.linear(true));
    let turned = Shatai { magnification: 0.1, angle: 60.0, adjust_rotation: true, adjust_tsume: false };
    assert!(close(turned.linear(true), [0.9222, 0.0843, 0.0, 0.9759]), "{:?}", turned.linear(true));
    // In horizontal text the turn keeps the glyph's horizontal axis horizontal.
    assert!(turned.linear(false)[1].abs() < 1e-12);
}

#[test]
fn shatai_slants_glyphs_about_their_em_centre_and_fits_the_advance() {
    let (mut d, sid, _) = doc_with("AB", Rect::new(36.0, 36.0, 300.0, 100.0), ParaAttrs::default());
    let glyph = |d: &Document| {
        let cs = compose_story(d, sid, &ComposeOptions::default());
        let l = cs.frames[0].lines[0].clone();
        (cs.styles[l.glyphs[0].style as usize].clone(), l.glyphs[0].clone(), l.baseline)
    };
    let (_, before, _) = glyph(&d);
    d.story_mut(sid).unwrap().format_chars(0..2, |f| {
        f.over.shatai_magnification = Some(20.0);
        f.over.shatai_angle = Some(60.0);
        f.over.shatai_adjust_tsume = Some(false);
    });
    let (style, g, baseline) = glyph(&d);
    assert!((g.adv - before.adv).abs() < 1e-9, "without Adjust Tsume the advance stays");
    let xf = style.shatai_xf(&g, baseline).unwrap();
    // y down: the off-diagonal terms change sign.
    let [a, b, c, dd, _, _] = xf.as_coeffs();
    assert!((a - 0.85).abs() < 1e-3 && (b + 0.0866).abs() < 1e-3 && (c + 0.0866).abs() < 1e-3 && (dd - 0.95).abs() < 1e-3);
    let (top, bottom) = g.face.em_box();
    let centre = Point::new(g.x + g.face.advance(g.gid) * g.sx / 2.0, baseline + g.y - (top + bottom) / 2.0 * g.sy);
    assert!((xf * centre - centre).hypot() < 1e-9, "the em box centre stays put");
    // Adjust Tsume: the advance becomes the compressed em box's width.
    d.story_mut(sid).unwrap().format_chars(0..2, |f| f.over.shatai_adjust_tsume = Some(true));
    let (style, fitted, _) = glyph(&d);
    let sh = style.shatai.unwrap();
    let w = g.face.advance(g.gid) * g.sx;
    let want = before.adv + w * (sh.extent(false, w, (top - bottom) * g.sy) - 1.0);
    assert!((fitted.adv - want).abs() < 1e-9, "{} vs {want}", fitted.adv);
    // No shatai without magnification.
    d.story_mut(sid).unwrap().format_chars(0..2, |f| f.over.shatai_magnification = Some(0.0));
    assert!(glyph(&d).0.shatai.is_none());
}

#[test]
fn auto_tate_chu_yoko_takes_short_digit_runs_in_vertical_text() {
    let text = "令和12年と1234年と123";
    let (mut d, sid, _) = doc_with(text, Rect::new(100.0, 100.0, 200.0, 400.0), ParaAttrs { auto_tcy: Some(3), ..Default::default() });
    let glyphs = |d: &Document| compose_story(d, sid, &ComposeOptions::default()).frames[0].lines[0].glyphs.clone();
    assert!(glyphs(&d).iter().all(|g| g.tcy.is_none()), "horizontal text has none");
    d.story_mut(sid).unwrap().vertical = true;
    let gs = glyphs(&d);
    let at = |s: &str| text.find(s).unwrap();
    let tcy: Vec<&PlacedGlyph> = gs.iter().filter(|g| g.tcy.is_some()).collect();
    let two = at("12年");
    let three = text.rfind("123").unwrap();
    assert_eq!(tcy.iter().map(|g| g.byte).collect::<Vec<_>>(), [two, two + 1, three, three + 1, three + 2], "2 and 3 digits, not 4");
    // Each group takes one em along the line; three digits are narrowed to fit across it.
    let em = tcy[0].tcy.unwrap()[2];
    assert!((tcy[..2].iter().map(|g| g.adv).sum::<f64>() - em).abs() < 1e-6);
    let plain = gs.iter().find(|g| g.byte == at("1234")).unwrap();
    assert!(tcy[2].sx < plain.sx, "{} {}", tcy[2].sx, plain.sx);
    let [_, left, _] = tcy[2].tcy.unwrap();
    assert!(left >= -em / 2.0 - 1e-6, "the narrowed group stays within the em");
    // Roman letters too, with Include Roman.
    let text2 = "縦AB横";
    let (mut d, sid, _) = doc_with(text2, Rect::new(100.0, 100.0, 200.0, 400.0), ParaAttrs { auto_tcy: Some(2), ..Default::default() });
    d.story_mut(sid).unwrap().vertical = true;
    let count = |d: &Document| compose_story(d, sid, &ComposeOptions::default()).frames[0].lines[0].glyphs.iter().filter(|g| g.tcy.is_some()).count();
    assert_eq!(count(&d), 0);
    d.story_mut(sid).unwrap().paras[0].para.auto_tcy_include_roman = Some(true);
    assert_eq!(count(&d), 2);
}

#[test]
fn rotate_roman_stands_half_width_characters_upright() {
    let (mut d, sid, _) = doc_with("縦書きLatin", Rect::new(100.0, 100.0, 200.0, 400.0), ParaAttrs::default());
    d.story_mut(sid).unwrap().vertical = true;
    let latin_upright = |d: &Document| {
        let cs = compose_story(d, sid, &ComposeOptions::default());
        cs.frames[0].lines[0].glyphs.iter().filter(|g| g.len > 0 && g.byte >= "縦書き".len()).all(|g| g.upright)
    };
    assert!(!latin_upright(&d));
    d.story_mut(sid).unwrap().paras[0].para.rotate_roman = Some(true);
    assert!(latin_upright(&d));
}

#[test]
fn roman_word_break_breaks_inside_words_without_hyphens() {
    let para = ParaAttrs { composer: Some(designcraft_doc::Composer::SingleLine), hyphenate: Some(false), ..Default::default() };
    let first_line = |rwb: bool| {
        let (d, sid, _) = doc_with("ab cdefghijklm", Rect::new(0.0, 0.0, 40.0, 300.0), ParaAttrs { roman_word_break: Some(rwb), ..para.clone() });
        let cs = compose_story(&d, sid, &ComposeOptions::default());
        let lines = all_lines(&cs);
        assert!(lines.iter().all(|l| !l.hyphenated && l.end_x <= l.x1 + 0.5));
        lines[0].range.clone()
    };
    assert_eq!(first_line(false), 0..3, "the word moves down whole");
    assert!(first_line(true).end > 3, "the line fills with part of the word");
}

/// Composed lines of a story in a frame with a 12 pt baseline grid; returns them and the grid.
fn grid_doc(text: &str, para: ParaAttrs, edit: impl FnOnce(&mut Document, StoryId)) -> (Vec<Line>, (f64, f64)) {
    let (mut d, sid, fid) = doc_with(text, Rect::new(0.0, 0.0, 300.0, 400.0), para);
    if let Some(tf) = d.item_mut(fid).and_then(|i| i.text_frame_mut()) {
        tf.options.baseline_grid = Some((12.0, 0.0));
    }
    edit(&mut d, sid);
    // Vertical frames have no baseline grid.
    let grid = frame_specs(&d, sid)[0].grid.unwrap_or((0.0, 12.0));
    let cs = compose_story(&d, sid, &ComposeOptions::default());
    (all_lines(&cs).into_iter().cloned().collect(), grid)
}

/// The em box centre of a line's first glyph (as the composer measures it), y down.
fn em_centre(l: &Line) -> f64 {
    let g = &l.glyphs[0];
    let (a, b) = g.face.vertical_metrics();
    l.baseline + g.y + (b - a) / (2.0 * (a + b)) * g.face.units_per_em() * g.sy
}

fn on_grid(y: f64, (start, inc): (f64, f64)) -> bool {
    let n = (y - start) / inc;
    (n - n.round()).abs() < 1e-6
}

#[test]
fn grid_alignment_puts_the_chosen_point_on_the_grid() {
    use designcraft_doc::cjk::CharacterAlignment as R;
    let para = |r: R| ParaAttrs { grid_align: Some(designcraft_doc::GridAlign::AllLines), grid_reference: Some(r), ..Default::default() };
    let (roman, grid) = grid_doc("AB\nCD", para(R::Baseline), |_, _| {});
    assert!(roman.iter().all(|l| on_grid(l.baseline, grid)));
    let (centred, grid) = grid_doc("AB\nCD", para(R::EmCenter), |_, _| {});
    assert!(centred.iter().all(|l| on_grid(em_centre(l), grid)));
    assert!(!on_grid(centred[0].baseline, grid), "the baseline moves off the grid by the em centre's offset");
    let offset = em_centre(&centred[0]) - centred[0].baseline;
    let shift = centred[0].baseline - roman[0].baseline;
    assert!(on_grid(shift + offset, (0.0, grid.1)), "{shift} {offset}");
}

#[test]
fn gyoudori_two_lines_centres_a_heading_in_two_grid_lines() {
    use designcraft_doc::cjk::CharacterAlignment as R;
    let para = ParaAttrs { grid_align: Some(designcraft_doc::GridAlign::AllLines), grid_reference: Some(R::EmCenter), ..Default::default() };
    let (lines, grid) = grid_doc("Head\nBody text", para, |d, sid| {
        let st = d.story_mut(sid).unwrap();
        st.format_chars(0..4, |f| f.over.size = Some(18.0));
        st.format_paras(0..4, |p| p.para.grid_gyoudori = Some(2));
    });
    let (head, body) = (em_centre(&lines[0]), em_centre(&lines[1]));
    assert!(on_grid(head - grid.1 / 2.0, grid), "midway between two grid lines: {head}");
    assert!(on_grid(body, grid));
    assert!((body - head - 1.5 * grid.1).abs() < 1e-6, "the body takes the grid line after the heading's two: {head} {body}");
}

#[test]
fn paragraph_gyoudori_centres_the_whole_paragraph() {
    use designcraft_doc::cjk::CharacterAlignment as R;
    let para = ParaAttrs {
        grid_align: Some(designcraft_doc::GridAlign::AllLines),
        grid_reference: Some(R::EmCenter),
        grid_gyoudori: Some(4),
        paragraph_gyoudori: Some(true),
        ..Default::default()
    };
    let (lines, grid) = grid_doc("One\u{2028}Two\nNext", para, |d, sid| {
        d.story_mut(sid).unwrap().format_paras(10..14, |p| {
            p.para.grid_gyoudori = Some(0);
            p.para.paragraph_gyoudori = Some(false);
        });
    });
    let firsts: Vec<f64> = lines.iter().map(em_centre).collect();
    assert_eq!(lines.len(), 3, "{firsts:?}");
    // Two lines a leading apart, centred as a block in four grid lines; the next paragraph on the fifth.
    let middle = (firsts[0] + firsts[1]) / 2.0;
    assert!(on_grid(middle - 1.5 * grid.1, grid), "{firsts:?}");
    assert!((firsts[2] - (middle - 1.5 * grid.1) - 4.0 * grid.1).abs() < 1e-6, "{firsts:?}");
}

#[test]
fn hostile_cjk_settings_compose_without_panicking() {
    let para = ParaAttrs {
        auto_tcy: Some(u32::MAX),
        auto_tcy_include_roman: Some(true),
        rotate_roman: Some(true),
        roman_word_break: Some(true),
        grid_align: Some(designcraft_doc::GridAlign::AllLines),
        grid_reference: Some(designcraft_doc::cjk::CharacterAlignment::IcfBottom),
        grid_gyoudori: Some(u32::MAX),
        paragraph_gyoudori: Some(true),
        ..Default::default()
    };
    for vertical in [false, true] {
        let (_, _) = grid_doc("縦12AB書き\nmore text here", para.clone(), |d, sid| {
            let st = d.story_mut(sid).unwrap();
            st.vertical = vertical;
            let len = st.len();
            st.format_chars(0..len, |f| {
                f.over.kenten = Some(true);
                f.over.kenten_kind = Some(designcraft_doc::cjk_settings::KentenKind::Custom);
                f.over.kenten_character = Some("★".repeat(100_000));
                f.over.kenten_font = Some("No Such Font".into());
                f.over.kenten_size = Some(Some(1e308));
                f.over.kenten_x_scale = Some(-1e308);
                f.over.kenten_y_scale = Some(f64::NAN);
                f.over.kenten_distance = Some(f64::INFINITY);
                f.over.kenten_fill_tint = Some(Some(f32::NAN));
                f.over.kenten_stroke_weight = Some(Some(-1e308));
                f.over.shatai_magnification = Some(1e308);
                f.over.shatai_angle = Some(f64::NEG_INFINITY);
            });
        });
    }
}

/// A 100×60 rectangle anchored at `at` in `sid`, custom-positioned with its top-left at
/// (`x`, `y`) from the top-left of the anchor's frame (`y_rel`: from the anchor's baseline).
fn anchor_box(d: &mut Document, sid: StoryId, at: usize, x: f64, y: f64, y_rel: bool, wrap: WrapMode) {
    use designcraft_doc::anchored::AnchorRelative as R;
    let lid = d.default_layer();
    let mut it = designcraft_doc::Item::new(
        ItemId(d.alloc()),
        lid,
        designcraft_doc::Shape::Rectangle,
        designcraft_geom::shapes::rectangle(Rect::new(0.0, 0.0, 100.0, 60.0)),
    );
    it.wrap.mode = wrap;
    let position = designcraft_doc::AnchorPosition::Custom {
        x_relative: R::TextFrame,
        y_relative: if y_rel { R::Anchor } else { R::TextFrame },
        x_offset: x,
        y_offset: y,
        object_point: 0,
        ref_point: 0,
        keep_within_column: false,
        spine_relative: false,
    };
    d.story_mut(sid).unwrap().insert_object(at, designcraft_doc::AnchoredObject::new(it, position));
}

/// Lines whose band (top to baseline) meets `y0..y1`.
fn lines_in_band(cs: &ComposedStory, y0: f64, y1: f64) -> Vec<&Line> {
    all_lines(cs).into_iter().filter(|l| l.baseline - l.ascent < y1 && l.baseline > y0).collect()
}

fn line_layout(cs: &ComposedStory) -> Vec<(Range<usize>, u64, u64, u64)> {
    all_lines(cs).iter().map(|l| (l.range.clone(), l.baseline.to_bits(), l.x0.to_bits(), l.x1.to_bits())).collect()
}

#[test]
fn anchored_object_wraps_another_storys_text() {
    let long = [LOREM; 3].join(" ");
    let (mut d, a, fa) = doc_with("Anchor here.", Rect::new(0.0, 0.0, 300.0, 400.0), ParaAttrs::default());
    let lid = d.default_layer();
    let (_, b) = d.add_text_frame(SpreadRef::Doc(0), Rect::new(320.0, 0.0, 620.0, 400.0), lid, &long, ParaFormat::default()).unwrap();
    let plain = compose_story(&d, b, &ComposeOptions::default());
    let left = all_lines(&plain)[0].x0;
    // Over the left part of B's frame, 100–160 pt down.
    let area = d.item(fa).unwrap().text_area();
    anchor_box(&mut d, a, 0, left - area.x0, 100.0 - area.y0, false, WrapMode::BoundingBox);
    let cs = compose_story(&d, b, &ComposeOptions::default());
    let band = lines_in_band(&cs, 100.0, 160.0);
    assert!(!band.is_empty());
    assert!(band.iter().all(|l| l.x0 >= left + 100.0 - 1e-6), "{:?}", band.iter().map(|l| l.x0).collect::<Vec<_>>());
    assert!(lines_in_band(&cs, 0.0, 80.0).iter().all(|l| (l.x0 - left).abs() < 1e-6));
    // The cache composes the same, and follows the object when it moves.
    let cache = Cache::new();
    assert_eq!(line_layout(&cache.get(&d, b, None)), line_layout(&cs));
    anchor_box(&mut d, a, 0, left - area.x0, 200.0 - area.y0, false, WrapMode::BoundingBox);
    let st = d.story_mut(a).unwrap();
    st.delete(OBJECT_LEN..2 * OBJECT_LEN);
    let moved = cache.get(&d, b, None);
    assert_eq!(line_layout(&moved), line_layout(&compose_story(&d, b, &ComposeOptions::default())));
    assert!(lines_in_band(&moved, 100.0, 160.0).iter().any(|l| (l.x0 - left).abs() < 1e-6));
    assert!(lines_in_band(&moved, 200.0, 260.0).iter().all(|l| l.x0 >= left + 100.0 - 1e-6));
}

/// An anchored object's wrap is built as a page item's: a contour on both sides of an oval sets
/// text left and right of it, closer in where the oval narrows.
#[test]
fn anchored_object_wraps_with_its_contour_and_side() {
    use designcraft_doc::anchored::AnchorRelative as R;
    let long = [LOREM; 4].join(" ");
    let (mut d, a, fa) = doc_with("Anchor here.", Rect::new(0.0, 0.0, 300.0, 400.0), ParaAttrs::default());
    let lid = d.default_layer();
    let (_, b) = d.add_text_frame(SpreadRef::Doc(0), Rect::new(320.0, 0.0, 620.0, 400.0), lid, &long, ParaFormat::default()).unwrap();
    let left = all_lines(&compose_story(&d, b, &ComposeOptions::default()))[0].x0;
    let area = d.item(fa).unwrap().text_area();
    let mut it = designcraft_doc::Item::new(
        ItemId(d.alloc()),
        lid,
        designcraft_doc::Shape::Oval,
        designcraft_geom::shapes::ellipse(Rect::new(0.0, 0.0, 100.0, 100.0)),
    );
    it.wrap.mode = WrapMode::Contour;
    it.wrap.side = WrapSide::BothSides;
    let position = designcraft_doc::AnchorPosition::Custom {
        x_relative: R::TextFrame,
        y_relative: R::TextFrame,
        x_offset: left + 100.0 - area.x0,
        y_offset: 100.0 - area.y0,
        object_point: 0,
        ref_point: 0,
        keep_within_column: false,
        spine_relative: false,
    };
    d.story_mut(a).unwrap().insert_object(0, designcraft_doc::AnchoredObject::new(it, position));
    let cs = compose_story(&d, b, &ComposeOptions::default());
    let lines = all_lines(&cs);
    let pairs: Vec<(&Line, &Line)> = lines.windows(2).filter(|w| w[0].baseline == w[1].baseline).map(|w| (w[0], w[1])).collect();
    assert!(pairs.len() >= 3, "{} lines share a baseline", pairs.len());
    let centre = left + 150.0;
    assert!(pairs.iter().all(|(l, r)| l.x1 < centre && r.x0 > centre), "text on both sides");
    let top = pairs.first().unwrap().0.x1;
    let mid = pairs.iter().map(|(l, _)| l.x1).fold(f64::INFINITY, f64::min);
    assert!(top > mid + 5.0, "top {top}, middle {mid}");
}

/// Relative to Spine: a marginal note set up for a left-hand page (its right edge 12 pt left of
/// the column) mirrors on the right-hand page (its left edge 12 pt right of the column), so the
/// notes of a story that runs across a spread all sit in the outside margin. Placed as on a
/// left-hand page, the right page's note sat in the inside margin and reached across the spine.
#[test]
fn spine_relative_anchored_notes_stay_in_the_outside_margin_on_both_pages() {
    use designcraft_doc::anchored::AnchorRelative as R;
    let compose_spread = |spine_relative: bool| {
        let mut d = Document::new(&NewDocument { pages: 3, ..Default::default() });
        let lid = d.default_layer();
        let sp = &d.spreads[1];
        let (l, r) = (sp.pages[0].clone(), sp.pages[1].clone());
        assert_eq!((l.side, r.side), (designcraft_doc::PageSide::Left, designcraft_doc::PageSide::Right));
        // Body frames leave a 120 pt outside margin for the notes.
        let left_rect = Rect::new(l.x + 120.0, 36.0, l.x + l.width - 36.0, 120.0);
        let right_rect = Rect::new(r.x + 36.0, 36.0, r.x + r.width - 120.0, 600.0);
        let text = [LOREM; 3].join(" ");
        let (lf, sid) = d.add_text_frame(SpreadRef::Doc(1), left_rect, lid, &text, ParaFormat::default()).unwrap();
        let (rf, _) = d.add_text_frame(SpreadRef::Doc(1), right_rect, lid, "", ParaFormat::default()).unwrap();
        d.thread(lf, rf).unwrap();
        let plain = compose_story(&d, sid, &ComposeOptions::default());
        let second_line = |fid| plain.frame(fid).unwrap().lines[1].range.start;
        let (at_left, at_right) = (second_line(lf), second_line(rf));
        // Top-right corner of the note 12 pt left of the column's top-left, on the anchor's baseline.
        let position = designcraft_doc::AnchorPosition::Custom {
            x_relative: R::ColumnEdge,
            y_relative: R::Anchor,
            x_offset: -12.0,
            y_offset: 0.0,
            object_point: 2,
            ref_point: 0,
            keep_within_column: false,
            spine_relative,
        };
        for at in [at_right, at_left] {
            let note = designcraft_doc::Item::new(
                ItemId(d.alloc()),
                lid,
                designcraft_doc::Shape::Rectangle,
                designcraft_geom::shapes::rectangle(Rect::new(0.0, 0.0, 72.0, 40.0)),
            );
            d.story_mut(sid).unwrap().insert_object(at, designcraft_doc::AnchoredObject::new(note, position.clone()));
        }
        let cs = compose_story(&d, sid, &ComposeOptions::default());
        // Per page: the note's left and right edges, the column, and the page's left and right edges.
        let placed = |fid, page: &designcraft_doc::Page| {
            let ft = cs.frame(fid).unwrap();
            assert_eq!(ft.objects.len(), 1);
            let o = &ft.objects[0];
            ((o.origin.x, o.origin.x + o.size.0), ft.columns[0], (page.x, page.x + page.width))
        };
        (placed(lf, &l), placed(rf, &r))
    };
    let near = |a: f64, b: f64| (a - b).abs() < 1e-6;

    let (((lx0, lx1), lcol, (lp0, _)), ((rx0, rx1), rcol, (_, rp1))) = compose_spread(true);
    assert!(near(lx1, lcol.x0 - 12.0) && lx0 >= lp0, "left page: note {lx0}..{lx1}, column {lcol:?}");
    assert!(near(rx0, rcol.x1 + 12.0) && rx1 <= rp1, "right page: note {rx0}..{rx1}, column {rcol:?}");

    // Without Relative to Spine both pages take the settings as written.
    let (_, ((rx0, rx1), rcol, _)) = compose_spread(false);
    assert!(near(rx1, rcol.x0 - 12.0), "right page as written: note {rx0}..{rx1}, column {rcol:?}");
}

const OBJECT_LEN: usize = designcraft_doc::OBJECT_MARK.len_utf8();

#[test]
fn anchored_object_wraps_its_own_storys_later_lines() {
    let text = [LOREM; 4].join(" ");
    let (mut d, sid, _) = doc_with(&text, Rect::new(0.0, 0.0, 300.0, 600.0), ParaAttrs::default());
    let plain = compose_story(&d, sid, &ComposeOptions::default());
    let left = all_lines(&plain)[0].x0;
    // Anchored in the third line, hanging from its baseline over the lines below.
    let at = all_lines(&plain)[2].range.start + 3;
    anchor_box(&mut d, sid, at, 0.0, 0.0, true, WrapMode::BoundingBox);
    let cs = compose_story(&d, sid, &ComposeOptions::default());
    let lines = all_lines(&cs);
    let (li, anchor_line) = lines.iter().enumerate().find(|(_, l)| l.range.contains(&at)).unwrap();
    let top = anchor_line.baseline;
    // The anchor line and those above it keep the full measure; the lines under the object move right.
    assert!(lines[..=li].iter().all(|l| (l.x0 - left).abs() < 1e-6));
    let under: Vec<_> = lines[li + 1..].iter().filter(|l| l.baseline - l.ascent < top + 60.0).collect();
    assert!(under.len() >= 2);
    assert!(under.iter().all(|l| l.x0 >= left + 100.0 - 1e-6), "{:?}", under.iter().map(|l| l.x0).collect::<Vec<_>>());
    assert!(lines.iter().filter(|l| l.baseline - l.ascent > top + 61.0).all(|l| (l.x0 - left).abs() < 1e-6));
    // Settled: the object is where the final pass wrapped the text around.
    let o = &cs.frames[0].objects[0];
    assert!((o.origin.y - top).abs() < 1e-6);
}

#[test]
fn anchored_object_without_wrap_leaves_layout_unchanged() {
    let long = [LOREM; 3].join(" ");
    let (mut d, a, _) = doc_with(&long, Rect::new(0.0, 0.0, 300.0, 400.0), ParaAttrs::default());
    let lid = d.default_layer();
    let (_, b) = d.add_text_frame(SpreadRef::Doc(0), Rect::new(320.0, 0.0, 620.0, 400.0), lid, &long, ParaFormat::default()).unwrap();
    anchor_box(&mut d, a, 40, 330.0, 0.0, true, WrapMode::None);
    for sid in [a, b] {
        let direct = compose(&d, d.story(sid).unwrap(), &frame_specs(&d, sid), &ComposeOptions::default());
        assert_eq!(line_layout(&compose_story(&d, sid, &ComposeOptions::default())), line_layout(&direct));
    }
}

#[test]
fn anchored_wrap_that_moves_its_own_anchor_terminates() {
    // Jump Object pushes the lines after the anchor below the object; vertical centring then moves
    // every line, the anchor's too, and the object with it.
    let text = [LOREM; 2].join(" ");
    let (mut d, sid, fid) = doc_with(&text, Rect::new(0.0, 0.0, 300.0, 500.0), ParaAttrs::default());
    d.item_mut(fid).unwrap().text_frame_mut().unwrap().options.vertical_justification = designcraft_doc::VerticalJustification::Center;
    let plain = compose_story(&d, sid, &ComposeOptions::default());
    let at = all_lines(&plain)[1].range.start + 2;
    anchor_box(&mut d, sid, at, 0.0, -20.0, true, WrapMode::JumpObject);
    let cs = compose_story(&d, sid, &ComposeOptions::default());
    assert!(cs.line_count() > 0);
    assert_eq!(cs.frames[0].objects.len(), 1);
}

#[test]
fn same_para_style_spacing_replaces_space_between_same_style_paragraphs() {
    let rect = Rect::new(0.0, 0.0, 300.0, 700.0);
    let base = ParaAttrs { space_before: Some(5.0), space_after: Some(10.0), ..Default::default() };
    // `other_last`: the last paragraph gets a different paragraph style.
    let baselines = |para: ParaAttrs, other_last: bool| {
        let (mut d, sid, _) = doc_with("A\n\nB", rect, para);
        if other_last && let Some(st) = d.stories.get_mut(&sid) {
            std::sync::Arc::make_mut(st).paras[2].style = "Other".into();
        }
        let cs = compose_story(&d, sid, &ComposeOptions::default());
        all_lines(&cs).iter().map(|l| l.baseline).collect::<Vec<_>>()
    };
    let tight_attrs = ParaAttrs { same_para_style_spacing: Some(Some(0.0)), ..base.clone() };
    let normal = baselines(base.clone(), false);
    let tight = baselines(tight_attrs.clone(), false);
    let wide = baselines(ParaAttrs { same_para_style_spacing: Some(Some(20.0)), ..base }, false);
    let mixed = baselines(tight_attrs, true);
    assert_eq!(normal.len(), 3);
    // Ordinary spacing: space after (10) + space before (5) between each pair, empty paragraph included.
    assert!((normal[1] - normal[0] - 14.4 - 15.0).abs() < 1e-6, "{normal:?}");
    assert!((tight[1] - tight[0] - 14.4).abs() < 1e-6 && (tight[2] - tight[1] - 14.4).abs() < 1e-6, "{tight:?}");
    assert!((wide[2] - wide[1] - 14.4 - 20.0).abs() < 1e-6, "{wide:?}");
    // A different style keeps the ordinary spacing at its boundary.
    assert!((mixed[1] - mixed[0] - 14.4).abs() < 1e-6 && (mixed[2] - mixed[1] - 14.4 - 15.0).abs() < 1e-6, "{mixed:?}");
}

// ---------- balance columns ----------

/// A story of `paras` body paragraphs in one frame of `columns` columns, Balance Columns `on`.
fn balance_doc(paras: usize, columns: u32, rect: Rect, on: bool) -> (Document, StoryId, ItemId) {
    let (mut d, sid, fid) = doc_with(&vec![LOREM; paras].join("\n"), rect, ParaAttrs::default());
    let o = &mut d.item_mut(fid).unwrap().text_frame_mut().unwrap().options;
    o.columns = columns;
    o.balance_columns = on;
    (d, sid, fid)
}

fn column_line_counts(ft: &FrameText) -> Vec<usize> {
    (0..ft.columns.len() as u32).map(|c| ft.lines.iter().filter(|l| l.column == c).count()).collect()
}

/// The baseline of each column's last line.
fn column_ends(ft: &FrameText) -> Vec<f64> {
    (0..ft.columns.len() as u32).map(|c| ft.lines.iter().filter(|l| l.column == c).map(|l| l.baseline).fold(f64::NEG_INFINITY, f64::max)).collect()
}

#[test]
fn balance_columns_evens_out_the_columns_the_story_ends_in() {
    let rect = Rect::new(0.0, 0.0, 400.0, 300.0);
    let (d, sid, _) = balance_doc(3, 2, rect, false);
    let off = compose_story(&d, sid, &ComposeOptions::default());
    let counts = column_line_counts(&off.frames[0]);
    assert!(counts[1] > 0 && counts[0] > counts[1] + 2, "fills the first column: {counts:?}");

    let (d, sid, _) = balance_doc(3, 2, rect, true);
    let cs = compose_story(&d, sid, &ComposeOptions::default());
    assert!(!cs.is_overset());
    let ft = &cs.frames[0];
    assert_no_overlap(ft);
    let on = column_line_counts(ft);
    assert_eq!(on.iter().sum::<usize>(), counts.iter().sum::<usize>(), "the same lines");
    assert!(on[0] >= on[1] && on[0] - on[1] <= 1, "{on:?}");
    let ends = column_ends(ft);
    assert!((ends[0] - ends[1]).abs() <= 14.4 + 1e-6, "{ends:?}");
    assert_eq!(all_lines(&cs).last().unwrap().range.end, d.story(sid).unwrap().text.len());

    // A paragraph kept together stays in one column while the columns still balance.
    let (mut d, sid, _) = balance_doc(3, 2, rect, true);
    let p = &mut d.story_mut(sid).unwrap().paras[1].para;
    p.keep_lines_together = Some(true);
    p.keep_all_lines = Some(true);
    let cs = compose_story(&d, sid, &ComposeOptions::default());
    assert!(!cs.is_overset());
    let kept: Vec<u32> = cs.frames[0].lines.iter().filter(|l| l.para == 1).map(|l| l.column).collect();
    assert!(kept.iter().all(|&c| c == kept[0]), "{kept:?}");
    let c = column_line_counts(&cs.frames[0]);
    assert!(c[1] > 0 && c[0].max(c[1]) < counts[0], "{c:?} vs {counts:?}");

    // Three columns: the first two are equal, the last at most two lines shorter.
    let (d, sid, _) = balance_doc(3, 3, Rect::new(0.0, 0.0, 540.0, 500.0), true);
    let cs = compose_story(&d, sid, &ComposeOptions::default());
    assert!(!cs.is_overset());
    let c = column_line_counts(&cs.frames[0]);
    let total: usize = c.iter().sum();
    assert_eq!(c[0], total.div_ceil(3), "{c:?}");
    assert!(c[1] <= c[0] && c[2] <= c[1] && c[0] - c[2] <= 2 && c[2] > 0, "{c:?}");
    assert_no_overlap(&cs.frames[0]);

    // Vertical stories balance their columns too (bands across the turned frame).
    let (mut d, sid, _) = balance_doc(2, 2, Rect::new(0.0, 0.0, 400.0, 400.0), true);
    d.story_mut(sid).unwrap().vertical = true;
    let cs = compose_story(&d, sid, &ComposeOptions::default());
    assert!(!cs.is_overset());
    let c = column_line_counts(&cs.frames[0]);
    assert!(c[1] > 0 && c[0].abs_diff(c[1]) <= 1, "{c:?}");
}

#[test]
fn balance_columns_leaves_a_frame_the_text_runs_on_from() {
    let rect = Rect::new(0.0, 0.0, 400.0, 150.0);
    let layout = |on: bool| {
        let (mut d, sid, f1) = balance_doc(4, 2, rect, on);
        let lid = d.default_layer();
        let (f2, _) = d.add_text_frame(SpreadRef::Doc(0), Rect::new(0.0, 300.0, 400.0, 700.0), lid, "", ParaFormat::default()).unwrap();
        let o = &mut d.item_mut(f2).unwrap().text_frame_mut().unwrap().options;
        o.columns = 2;
        o.balance_columns = on;
        d.thread(f1, f2).unwrap();
        compose_story(&d, sid, &ComposeOptions::default())
    };
    let (off, on) = (layout(false), layout(true));
    assert!(!on.is_overset());
    let first = |cs: &ComposedStory| cs.frames[0].lines.iter().map(|l| (l.column, l.baseline, l.range.clone())).collect::<Vec<_>>();
    assert_eq!(first(&on), first(&off), "the first frame fills normally");
    let c = column_line_counts(&on.frames[1]);
    assert!(c[1] > 0 && c[0] - c[1] <= 1, "the frame the story ends in is balanced: {c:?}");
}

#[test]
fn balance_columns_starts_after_a_column_break() {
    // The first paragraph ends with a column break: the first column holds it alone and the
    // other two share the rest.
    let text = format!("{LOREM}{}\n{}", designcraft_doc::story::COLUMN_BREAK, [LOREM; 3].join("\n"));
    let (mut d, sid, fid) = doc_with(&text, Rect::new(0.0, 0.0, 540.0, 500.0), ParaAttrs::default());
    let o = &mut d.item_mut(fid).unwrap().text_frame_mut().unwrap().options;
    o.columns = 3;
    o.balance_columns = true;
    let cs = compose_story(&d, sid, &ComposeOptions::default());
    assert!(!cs.is_overset());
    let ft = &cs.frames[0];
    assert_no_overlap(ft);
    assert!(ft.lines.iter().filter(|l| l.column == 0).all(|l| l.para == 0));
    assert!(ft.lines.iter().filter(|l| l.para == 0).all(|l| l.column == 0));
    let c = column_line_counts(ft);
    assert!(c[2] > 0 && c[1] >= c[2] && c[1] - c[2] <= 1, "{c:?}");
}

fn shaped_doc(path: designcraft_geom::PathData, rect: Rect, inset: f64) -> (Document, StoryId, ItemId) {
    let (mut d, sid, fid) = doc_with(&[LOREM; 4].join(" "), rect, ParaAttrs::default());
    let it = d.item_mut(fid).unwrap();
    it.path = path;
    it.text_frame_mut().unwrap().options.inset = [inset; 4];
    (d, sid, fid)
}

/// The corners of a line's band (slot ends from its top to its bottom).
fn line_corners(l: &Line) -> [(f64, f64); 4] {
    let (t, b) = (l.baseline - l.ascent, l.baseline + l.descent);
    [(l.x0, t), (l.x1, t), (l.x0, b), (l.x1, b)]
}

/// Is `p` inside the ellipse in `r` (allowing for the Bézier bulge)?
fn in_ellipse(r: Rect, p: (f64, f64)) -> bool {
    let (c, a, b) = (r.center(), r.width() / 2.0, r.height() / 2.0);
    ((p.0 - c.x) / a).powi(2) + ((p.1 - c.y) / b).powi(2) <= 1.001
}

/// Distance from `p` inside the ellipse in `r` to its outline (sampled).
fn ellipse_clearance(r: Rect, p: (f64, f64)) -> f64 {
    let (c, a, b) = (r.center(), r.width() / 2.0, r.height() / 2.0);
    let inside = ((p.0 - c.x) / a).powi(2) + ((p.1 - c.y) / b).powi(2) <= 1.0 + 1e-9;
    let d = (0..7200)
        .map(|k| {
            let t = k as f64 / 7200.0 * std::f64::consts::TAU;
            (c.x + a * t.cos() - p.0).hypot(c.y + b * t.sin() - p.1)
        })
        .fold(f64::INFINITY, f64::min);
    if inside { d } else { -d }
}

#[test]
fn oval_frame_sets_lines_inside_the_ellipse() {
    let r = Rect::new(0.0, 0.0, 300.0, 200.0);
    let (d, sid, _) = shaped_doc(designcraft_geom::shapes::ellipse(r), r, 0.0);
    let cs = compose_story(&d, sid, &ComposeOptions::default());
    let lines = all_lines(&cs);
    assert!(lines.len() >= 8, "{}", lines.len());
    for l in &lines {
        for p in line_corners(l) {
            assert!(in_ellipse(r, p), "line corner {p:?} outside the oval");
        }
        assert!(l.end_x <= l.x1 + 0.5, "line overflows its slot: {} > {}", l.end_x, l.x1);
    }
    let w = |l: &Line| l.x1 - l.x0;
    let widest = lines.iter().map(|l| w(l)).fold(0.0, f64::max);
    assert!(w(lines[0]) < widest - 50.0, "first {} widest {widest}", w(lines[0]));
    // Bands where the oval is narrower than 1.5 em hold no line.
    assert!(lines.iter().all(|l| w(l) >= 18.0 - 1e-6));
}

#[test]
fn oval_frame_keeps_lines_an_inset_inside_the_outline() {
    let r = Rect::new(0.0, 0.0, 300.0, 240.0);
    let (d, sid, _) = shaped_doc(designcraft_geom::shapes::ellipse(r), r, 10.0);
    let cs = compose_story(&d, sid, &ComposeOptions::default());
    let lines = all_lines(&cs);
    assert!(lines.len() >= 6, "{}", lines.len());
    for l in &lines {
        for p in line_corners(l) {
            assert!(ellipse_clearance(r, p) >= 10.0 - 0.1, "line corner {p:?} closer than the inset: {}", ellipse_clearance(r, p));
        }
    }
}

#[test]
fn triangle_frame_lines_widen_downward() {
    use designcraft_geom::{Anchor, PathData, Point, SubPath};
    let r = Rect::new(0.0, 0.0, 300.0, 300.0);
    let tri = [Point::new(150.0, 0.0), Point::new(300.0, 300.0), Point::new(0.0, 300.0)];
    let path = PathData::single(SubPath { anchors: tri.iter().map(|&p| Anchor::corner(p)).collect(), closed: true });
    let (d, sid, _) = shaped_doc(path, r, 0.0);
    let cs = compose_story(&d, sid, &ComposeOptions::default());
    let lines = all_lines(&cs);
    assert!(lines.len() >= 8, "{}", lines.len());
    for w in lines.windows(2) {
        assert!(w[1].x1 - w[1].x0 > w[0].x1 - w[0].x0, "{} then {}", w[0].x1 - w[0].x0, w[1].x1 - w[1].x0);
    }
    // Every slot end lies inside the triangle: |x - 150| ≤ y / 2 over the whole band.
    for l in &lines {
        for (x, y) in line_corners(l) {
            assert!((x - 150.0).abs() <= y / 2.0 + 1e-6, "({x}, {y}) outside the triangle");
        }
    }
}

#[test]
fn rectangular_frame_paths_keep_the_rectangular_layout() {
    use designcraft_geom::{Anchor, PathData, Point, SubPath};
    let r = Rect::new(20.0, 30.0, 260.0, 400.0);
    let (d, sid, _) = doc_with(&[LOREM; 4].join(" "), r, ParaAttrs::default());
    let plain = compose_story(&d, sid, &ComposeOptions::default());
    // The same rectangle drawn with an extra anchor on its top edge.
    let pts = [(20.0, 30.0), (100.0, 30.0), (260.0, 30.0), (260.0, 400.0), (20.0, 400.0)];
    let path = PathData::single(SubPath { anchors: pts.iter().map(|&(x, y)| Anchor::corner(Point::new(x, y))).collect(), closed: true });
    let (d2, sid2, _) = shaped_doc(path, r, 0.0);
    assert!(frame_specs(&d2, sid2)[0].outline.is_none());
    assert!(frame_specs(&d, sid)[0].outline.is_none());
    let redrawn = compose_story(&d2, sid2, &ComposeOptions::default());
    let key = |cs: &ComposedStory| all_lines(cs).iter().map(|l| (l.range.clone(), l.baseline, l.x0, l.x1)).collect::<Vec<_>>();
    assert_eq!(key(&plain), key(&redrawn));
}

#[test]
fn resizing_an_oval_frame_reflows_its_lines() {
    let r = Rect::new(0.0, 0.0, 240.0, 200.0);
    let (mut d, sid, fid) = shaped_doc(designcraft_geom::shapes::ellipse(r), r, 0.0);
    let cache = Cache::new();
    let small = cache.get(&d, sid, None);
    let big_r = Rect::new(0.0, 0.0, 400.0, 300.0);
    d.item_mut(fid).unwrap().path = designcraft_geom::shapes::ellipse(big_r);
    let big = cache.get(&d, sid, None);
    let (ls, lb) = (all_lines(&small), all_lines(&big));
    assert!(lb.len() > ls.len(), "{} vs {}", lb.len(), ls.len());
    for l in &lb {
        for p in line_corners(l) {
            assert!(in_ellipse(big_r, p), "line corner {p:?} outside the resized oval");
        }
    }
    assert!(lb.iter().map(|l| l.x1 - l.x0).fold(0.0, f64::max) > 300.0);
}

#[test]
fn vertical_oval_frame_sets_columns_inside_the_turned_ellipse() {
    let r = Rect::new(0.0, 0.0, 300.0, 200.0);
    let (mut d, sid, _) = shaped_doc(designcraft_geom::shapes::ellipse(r), r, 0.0);
    d.story_mut(sid).unwrap().vertical = true;
    let cs = compose_story(&d, sid, &ComposeOptions::default());
    let lines = all_lines(&cs);
    assert!(!lines.is_empty());
    // The turned box is 200 wide and 300 tall; the ellipse turns with it.
    let turned = Rect::new(0.0, 0.0, 200.0, 300.0);
    for l in &lines {
        for p in line_corners(l) {
            assert!(in_ellipse(turned, p), "line corner {p:?} outside the turned oval");
        }
    }
}

#[test]
fn oval_frame_columns_each_follow_the_outline() {
    let r = Rect::new(0.0, 0.0, 400.0, 300.0);
    let (mut d, sid, fid) = shaped_doc(designcraft_geom::shapes::ellipse(r), r, 0.0);
    d.item_mut(fid).unwrap().text_frame_mut().unwrap().options.columns = 2;
    let cs = compose_story(&d, sid, &ComposeOptions::default());
    let lines = all_lines(&cs);
    assert!(lines.iter().any(|l| l.column == 1));
    for l in &lines {
        for p in line_corners(l) {
            assert!(in_ellipse(r, p), "line corner {p:?} outside the oval");
        }
    }
}

#[test]
fn degenerate_frame_outlines_compose_without_panicking() {
    use designcraft_geom::{Anchor, PathData, Point, SubPath};
    let tri = |pts: [(f64, f64); 3]| {
        PathData::single(SubPath { anchors: pts.iter().map(|&(x, y)| Anchor::corner(Point::new(x, y))).collect(), closed: true })
    };
    let r = Rect::new(0.0, 0.0, 200.0, 200.0);
    for path in [
        tri([(0.0, 0.0), (100.0, 100.0), (200.0, 200.0)]),
        tri([(0.0, 0.0), (f64::NAN, 100.0), (200.0, 200.0)]),
        designcraft_geom::shapes::ellipse(Rect::new(-1e15, -1e15, 1e15, 1e15)),
    ] {
        let (d, sid, _) = shaped_doc(path, r, 1e300);
        let _ = compose_story(&d, sid, &ComposeOptions::default());
    }
}

#[test]
fn cache_recomposes_when_the_baseline_grid_changes() {
    let para = ParaAttrs { grid_align: Some(GridAlign::AllLines), ..Default::default() };
    let (mut d, sid, _) = doc_with(LOREM, Rect::new(0.0, 0.0, 200.0, 600.0), para);
    let cache = Cache::new();
    let a = cache.get(&d, sid, None);
    d.settings.baseline_grid.increment = 30.0;
    let b = cache.get(&d, sid, None);
    assert!(!Arc::ptr_eq(&a, &b));
    let (la, lb) = (&a.frames[0].lines[1], &b.frames[0].lines[1]);
    assert!((la.baseline - lb.baseline).abs() > 1.0, "{} {}", la.baseline, lb.baseline);
}

#[test]
fn cache_recomposes_when_a_wrap_layer_is_hidden() {
    let (mut d, sid, _) = doc_with(&[LOREM; 2].join(" "), Rect::new(0.0, 0.0, 300.0, 800.0), ParaAttrs::default());
    let lid = d.default_layer();
    let id = ItemId(d.alloc());
    let mut it =
        designcraft_doc::Item::new(id, lid, designcraft_doc::Shape::Rectangle, designcraft_geom::shapes::rectangle(Rect::new(0.0, 0.0, 120.0, 60.0)));
    it.wrap.mode = WrapMode::BoundingBox;
    d.insert_item(SpreadRef::Doc(0), it, None).unwrap();
    let cache = Cache::new();
    let a = cache.get(&d, sid, None);
    assert!(a.frames[0].lines[0].x0 >= 120.0 - 1e-6);
    d.layer_mut(lid).unwrap().visible = false;
    let b = cache.get(&d, sid, None);
    assert!(!Arc::ptr_eq(&a, &b));
    assert!(b.frames[0].lines[0].x0 < 1.0, "{}", b.frames[0].lines[0].x0);
}

#[test]
fn cache_recomposes_page_numbers_when_a_section_is_renumbered() {
    let text = story::PAGE_NUMBER.to_string();
    let (mut d, sid, _) = doc_with(&text, Rect::new(0.0, 0.0, 300.0, 100.0), ParaAttrs::default());
    let cache = Cache::new();
    let a = cache.get(&d, sid, None);
    assert_eq!(a.frames[0].lines[0].glyphs.len(), 1);
    d.sections[0].start_number = Some(1000);
    let b = cache.get(&d, sid, None);
    assert!(!Arc::ptr_eq(&a, &b));
    assert_eq!(b.frames[0].lines[0].glyphs.len(), 4);
}

#[test]
fn spaces_at_the_end_of_a_paragraph_take_no_room() {
    // A frame as wide as "2 VOOR" also holds "2 VOOR " on one line: the trailing space hangs.
    let (d, sid, _) = doc_with("2 VOOR", Rect::new(0.0, 0.0, 500.0, 100.0), ParaAttrs::default());
    let cs = compose_story(&d, sid, &ComposeOptions::default());
    let l = all_lines(&cs)[0];
    let width = l.end_x - l.x0;
    for text in ["2 VOOR ", "2 VOOR  ", "2 VOOR \u{2028}more"] {
        let (d, sid, _) = doc_with(text, Rect::new(0.0, 0.0, width + 0.02, 100.0), ParaAttrs::default());
        let cs = compose_story(&d, sid, &ComposeOptions::default());
        let first = all_lines(&cs)[0];
        assert_eq!(&text[first.range.clone()].trim_end(), &"2 VOOR", "{text:?}");
    }
}

/// A story that ends with a return isn't overset when only its empty last paragraph doesn't fit,
/// as in InDesign (Montilla Design's file: a caption fitted to its three lines, "No errors"). Text
/// that doesn't fit still is.
#[test]
fn an_empty_last_paragraph_isnt_overset() {
    // 12 pt, 14.4 leading: two lines fit in 35 pt, a third (baseline ~40) doesn't.
    let (d, sid, _) = doc_with("One\nTwo\n", Rect::new(0.0, 0.0, 200.0, 35.0), ParaAttrs::default());
    let cs = compose_story(&d, sid, &ComposeOptions::default());
    assert_eq!(all_lines(&cs).len(), 2);
    assert!(!cs.is_overset(), "only the empty paragraph after the last return is left");
    let (d, sid, _) = doc_with("One\nTwo\nThree", Rect::new(0.0, 0.0, 200.0, 35.0), ParaAttrs::default());
    assert!(compose_story(&d, sid, &ComposeOptions::default()).is_overset());
    let (d, sid, _) = doc_with("One\nTwo\n\n", Rect::new(0.0, 0.0, 200.0, 35.0), ParaAttrs::default());
    assert!(compose_story(&d, sid, &ComposeOptions::default()).is_overset(), "an empty paragraph before the last one is text");
}

#[test]
fn greek_text_gets_greek_hyphenation() {
    // Greek had no hyphenation at all: `hypher`'s Greek patterns were not among the features
    // the crate asks for, so `Lang::for_language` found nothing and Greek text set unhyphenated
    // however narrow the column (#1008).
    let text = "δημοκρατια ανθρωπος πολιτισμος δημοκρατια ανθρωπος πολιτισμος";
    let (mut d, sid, _) = doc_with(text, Rect::new(36.0, 36.0, 76.0, 700.0), ParaAttrs::default());
    let st = d.story_mut(sid).unwrap();
    let n = st.len();
    st.format_chars(0..n, |f| f.over.language = Some("Greek".into()));
    let cs = compose_story(&d, sid, &ComposeOptions::default());
    let breaks: Vec<&str> = all_lines(&cs).into_iter().filter(|l| l.hyphenated).map(|l| text[l.range.clone()].trim_end()).collect();
    assert!(!breaks.is_empty(), "Greek text is hyphenated");
    // Every break the composer took is a break the patterns give for that word.
    for b in breaks {
        let tail = b.rsplit(' ').next().unwrap_or(b);
        let whole = text[text.find(b).unwrap() + b.len() - tail.len()..].split(|c: char| !c.is_alphabetic()).next().unwrap_or("");
        let el = hyphen::Lang::for_language("Greek").unwrap();
        let pts = hyphen::hyphen_points_in(whole, &hyphen::Limits::default(), el);
        assert!(pts.contains(&tail.chars().count()), "break {tail}- in {whole} is a hyph-el point");
    }
    // The language is reached by name and by tag, as the other languages are.
    for name in ["Greek", "Greek: Greece", "el", "el-GR"] {
        assert!(hyphen::Lang::for_language(name).is_some(), "{name} has hyphenation");
    }
    // αν-θρω-πος, the syllables hyph-el gives.
    let el = hyphen::Lang::for_language("Greek").unwrap();
    assert_eq!(hyphen::hyphen_points_in("ανθρωπος", &hyphen::Limits::default(), el), vec![2, 5]);
}

/// Rule Above's offset runs from the first line's baseline up to the rule's bottom edge, as in
/// InDesign: a rule set clear of the cap height sits between the rows, not across the row above
/// (an INDD project list whose rules crossed the previous paragraph's text).
#[test]
fn rule_above_offset_is_measured_from_the_baseline() {
    use designcraft_doc::Rule;
    let rule = Rule { on: true, weight: 1.5, offset: 10.0, ..Default::default() };
    let para = ParaAttrs { rule_above: Some(rule), ..Default::default() };
    let (d, sid, fid) = doc_with("Row one\nRow two", Rect::new(0.0, 0.0, 300.0, 200.0), para);
    let cs = compose_story(&d, sid, &ComposeOptions::default());
    let lines = all_lines(&cs);
    let (first, second) = (lines[0], lines[1]);
    let mut decos: Vec<Rect> = cs.frame(fid).unwrap().decos.iter().map(|dc| dc.rect).collect();
    decos.sort_by(|a, b| a.y0.total_cmp(&b.y0));
    let rule = decos[1];
    assert!((rule.y1 - (second.baseline - 10.0)).abs() < 1e-6, "{rule:?}, baseline {}", second.baseline);
    assert!((rule.height() - 1.5).abs() < 1e-6);
    assert!(rule.y0 > first.baseline, "the rule stays below the row above: {rule:?}, its baseline {}", first.baseline);
}

#[test]
fn parent_page_frames_know_which_side_of_the_spine_they_are_on() {
    // Folios on a facing-pages parent: towards/away from spine must differ between its left and right page.
    let mut d = Document::new(&NewDocument::default());
    let lid = d.default_layer();
    let w = d.parents[0].pages[0].width;
    let para = ParaFormat { para: ParaAttrs { align: Some(Align::TowardsSpine), ..Default::default() }, ..Default::default() };
    let (_, left_story) = d.add_text_frame(SpreadRef::Parent(0), Rect::new(36.0, 700.0, w - 36.0, 730.0), lid, "7", para.clone()).unwrap();
    let (_, right_story) = d.add_text_frame(SpreadRef::Parent(0), Rect::new(w + 36.0, 700.0, 2.0 * w - 36.0, 730.0), lid, "8", para).unwrap();
    assert!(frame_specs(&d, left_story)[0].left_page);
    assert!(!frame_specs(&d, right_story)[0].left_page);
}

#[test]
fn a_grouped_frame_takes_the_side_of_the_page_its_group_moves_it_to() {
    let mut d = Document::new(&NewDocument { pages: 3, ..Default::default() });
    let lid = d.default_layer();
    let (l, r) = (d.spreads[1].pages[0].clone(), d.spreads[1].pages[1].clone());
    let rect = Rect::new(l.x + 36.0, 36.0, l.x + l.width - 36.0, 300.0);
    let (fid, sid) = d.add_text_frame(SpreadRef::Doc(1), rect, lid, "Text", ParaFormat::default()).unwrap();
    // The frame's own coordinates are on the left page; its group's transform puts it on the right one.
    let mut g = designcraft_doc::Item::new(ItemId(d.alloc()), lid, designcraft_doc::Shape::Group, designcraft_geom::PathData::default());
    g.xf = designcraft_geom::Affine::translate((r.x - l.x, 0.0));
    let sp = Arc::make_mut(&mut d.spreads[1]);
    let at = sp.items.iter().position(|it| it.id == fid).unwrap();
    g.content = designcraft_doc::Content::Group { items: vec![sp.items.remove(at)] };
    sp.items.push(Arc::new(g));
    let spec = &frame_specs(&d, sid)[0];
    assert!(spec.right_page && !spec.left_page);
    assert_eq!(spec.page, Some(2));
}

/// A right-to-left paragraph's first-line indent belongs at its start side, the right. Applied on
/// the left it moved the first line the wrong way — out of the frame for the hanging indent a
/// numbered list uses (right indent 10 mm, first line −10 mm), while the wrapped lines sat where
/// the first line should have been (#1448).
#[test]
fn an_rtl_first_line_indent_is_at_the_right_edge() {
    const MM: f64 = 72.0 / 25.4;
    // Latin text in a right-to-left paragraph: the direction is the paragraph's, and it is the
    // placement that is being measured, so no Arabic font has to be present.
    let text = LOREM;
    let rect = Rect::new(0.0, 0.0, 200.0, 300.0);
    let para = |dir, align, first, left, right| ParaAttrs {
        direction: Some(dir),
        align: Some(align),
        first_line_indent: Some(first),
        left_indent: Some(left),
        right_indent: Some(right),
        ..Default::default()
    };
    // The hanging indent of a numbered list, right to left.
    let (d, sid, _) = doc_with(text, rect, para(designcraft_doc::TextDirection::RightToLeft, Align::Right, -10.0 * MM, 0.0, 10.0 * MM));
    let cs = compose_story(&d, sid, &ComposeOptions::default());
    let lines = all_lines(&cs);
    assert!(lines.len() >= 2, "the text wraps: {}", lines.len());
    let right_edge = |l: &Line| l.glyphs.iter().filter(|g| g.visible).map(|g| g.x + g.adv).fold(f64::MIN, f64::max);
    let left_edge = |l: &Line| l.glyphs.iter().filter(|g| g.visible).map(|g| g.x).fold(f64::MAX, f64::min);
    // The first line reaches the frame's right edge; the wrapped lines stop 10 mm short of it.
    assert!((right_edge(lines[0]) - rect.x1).abs() < 1.0, "first line at the edge: {}", right_edge(lines[0]));
    assert!((right_edge(lines[1]) - (rect.x1 - 10.0 * MM)).abs() < 1.0, "wrapped line indented: {}", right_edge(lines[1]));
    // And nothing leaves the frame on the left, which is where the first line used to go.
    for (i, l) in lines.iter().enumerate() {
        assert!(left_edge(l) >= rect.x0 - 0.01, "line {i} starts inside the frame: {}", left_edge(l));
    }

    // The same paragraph left to right is unchanged: the first line hangs on the left.
    let (d, sid, _) = doc_with(text, rect, para(designcraft_doc::TextDirection::LeftToRight, Align::Left, -10.0 * MM, 10.0 * MM, 0.0));
    let cs = compose_story(&d, sid, &ComposeOptions::default());
    let lines = all_lines(&cs);
    assert!((left_edge(lines[0]) - rect.x0).abs() < 1.0, "left to right, first line at the edge: {}", left_edge(lines[0]));
    assert!((left_edge(lines[1]) - (rect.x0 + 10.0 * MM)).abs() < 1.0, "left to right, wrapped line indented: {}", left_edge(lines[1]));
}
