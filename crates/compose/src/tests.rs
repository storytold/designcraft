use designcraft_doc::build::NewDocument;
use designcraft_doc::{Align, Document, ParaAttrs, ParaFormat, SpreadRef};
use designcraft_geom::Rect;

use super::*;

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

#[test]
fn hostile_wrap_geometry_terminates() {
    let (d, sid) = wrap_doc(ParaAttrs { grid_align: Some(GridAlign::AllLines), ..Default::default() }, &[]);
    let mut specs = frame_specs(&d, sid);
    let ex = |x0: f64, y0: f64, x1: f64, y1: f64, mode| Exclusion { rect: Rect::new(x0, y0, x1, y1), mode };
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
    for c in [designcraft_doc::Composer::Paragraph, designcraft_doc::Composer::SingleLine] {
        let (free, zoned, huge) = (count(0.0, c), count(36.0, c), count(1000.0, c));
        // A huge zone leaves only words that start a line (no space to break at) hyphenable.
        assert!(free > 0 && zoned <= free && huge * 4 <= free, "{c:?}: {free} {zoned} {huge}");
    }
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

/// Story of `n` one-line filler paragraphs followed by `extra` paragraphs, in a 2-column frame.
fn keep_doc(fillers: usize, extra: &[&str], height: f64) -> (Document, StoryId, Vec<usize>) {
    let mut parts: Vec<String> = (0..fillers).map(|k| format!("Filler {k}")).collect();
    parts.extend(extra.iter().map(|s| s.to_string()));
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
                Some(designcraft_doc::PathType { start: 0.0, flip: false, align: designcraft_doc::PathAlign::Baseline });
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

/// OS/2 field offsets: sTypoAscender, sxHeight, sCapHeight.
const TYPO_ASCENDER: usize = 68;
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
    // Vertical scale scales it like the ascent.
    let (mut d, sid, fid) = doc_with("Hxg", Rect::new(36.0, 36.0, 300.0, 300.0), ParaAttrs::default());
    d.story_mut(sid).unwrap().format_chars(0..3, |f| {
        f.over.font_family = Some("TypoAscent 07".into());
        f.over.size = Some(20.0);
        f.over.v_scale = Some(1.5);
    });
    d.item_mut(fid).unwrap().text_frame_mut().unwrap().options.inset = [4.0, 0.0, 0.0, 0.0];
    let l = &compose_story(&d, sid, &ComposeOptions::default()).frames[0].lines[0];
    assert!(close(l.baseline, top + 21.0), "{}", l.baseline);
    // The line box keeps the font's ascent.
    assert!(close(l.ascent, 30.0), "{}", l.ascent);
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
    // No OS/2 table, zero / negative and absurd values: the top of the H (0.656 em) and 0.5 of the
    // (1 em) ascent.
    let zero: &[(usize, i16)] = &[(CAP_HEIGHT, 0), (X_HEIGHT, -40)];
    let big: &[(usize, i16)] = &[(CAP_HEIGHT, i16::MAX), (X_HEIGHT, i16::MAX)];
    for (family, fields) in [("CapXHeightNoO", None), ("CapXHeightNeg", Some(zero)), ("CapXHeightBig", Some(big))] {
        db.add_font(test_font(family, fields));
        let face = db.face(family, "Regular");
        assert_eq!(face.family, family);
        assert_eq!((face.cap_height, face.x_height), (656.0, face.ascent * 0.5), "{family}");
        let cap = first_baseline_in(family, |o| o.first_baseline = FirstBaseline::CapHeight);
        let x = first_baseline_in(family, |o| o.first_baseline = FirstBaseline::XHeight);
        assert!((cap - (40.0 + 13.12)).abs() < 0.01 && (x - (40.0 + 10.0)).abs() < 0.01, "{family}: {cap} {x}");
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
fn drop_cap_sits_on_its_line_as_set() {
    // Lines on a 15 pt baseline grid (not the 12 pt leading): the drop cap follows line 3.
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

#[test]
fn cjk_center_leading_measures_em_centers_across_different_sizes() {
    let (mut d, sid, _) = doc_with("A\nB", Rect::new(0.0, 0.0, 300.0, 200.0), ParaAttrs::default());
    d.story_mut(sid).unwrap().format_chars(0..3, |f| {
        f.over.leading = Some(designcraft_doc::Leading::Points(30.0));
        f.over.leading_model = Some(designcraft_doc::cjk::LeadingModel::Center);
    });
    d.story_mut(sid).unwrap().format_chars(0..1, |f| f.over.size = Some(24.0));
    let cs = compose_story(&d, sid, &ComposeOptions::default());
    let lines = all_lines(&cs);
    assert_eq!(lines.len(), 2);
    let center = |l: &Line| {
        let g = &l.glyphs[0];
        let (a, b) = g.face.vertical_metrics();
        l.baseline + (b - a) / (2.0 * (a + b)) * g.face.units_per_em() * g.sy
    };
    assert!((center(lines[1]) - center(lines[0]) - 30.0).abs() < 1e-6);
    assert!((lines[1].baseline - lines[0].baseline - 30.0).abs() > 0.1);
}

#[test]
fn cjk_em_center_alignment_moves_small_characters() {
    let (mut d, sid, _) = doc_with("AB", Rect::new(0.0, 0.0, 300.0, 100.0), ParaAttrs::default());
    d.story_mut(sid).unwrap().format_chars(0..1, |f| f.over.size = Some(24.0));
    d.story_mut(sid).unwrap().format_chars(1..2, |f| f.over.character_alignment = Some(designcraft_doc::cjk::CharacterAlignment::EmCenter));
    let cs = compose_story(&d, sid, &ComposeOptions::default());
    let line = &cs.frames[0].lines[0];
    let center = |g: &PlacedGlyph| {
        let (a, b) = g.face.vertical_metrics();
        g.y + (b - a) / (2.0 * (a + b)) * g.face.units_per_em() * g.sy
    };
    assert!((center(&line.glyphs[0]) - center(&line.glyphs[1])).abs() < 1e-6);
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
        scopes.push((designcraft_fonts::FontDb::global().load_document_fonts(&folder).scope, glyph));
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
    let scope = FontDb::global().load_document_fonts(&folder).scope;
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
