//! Frame grids: grid-aligned paragraphs set their lines on the grid's rows, as InDesign does
//! (measured with 12 pt cells and a 6 pt line aki: rows 18 pt apart, 12 pt high).

use designcraft_doc::FrameGrid;
use designcraft_doc::cjk::CharacterAlignment as R;

use super::*;

const EPS: f64 = 1e-6;

/// Composed lines of `text` in a frame grid story of 12 pt cells and 6 pt line aki, after `edit`;
/// with the frame's rows.
fn grid_lines(text: &str, para: ParaAttrs, edit: impl FnOnce(&mut Document, StoryId)) -> (Vec<Line>, FrameRows) {
    grid_lines_from(0.0, text, para, edit)
}

/// [`grid_lines`] with the first baseline `first` below the frame top (a fixed offset, so the
/// test font's ascent doesn't move it).
fn grid_lines_from(first: f64, text: &str, para: ParaAttrs, edit: impl FnOnce(&mut Document, StoryId)) -> (Vec<Line>, FrameRows) {
    grid_lines_with(designcraft_doc::FirstBaseline::Fixed, first, text, para, edit)
}

/// [`grid_lines`] with the frame's first baseline offset `fb` and its minimum `min`.
fn grid_lines_with(
    fb: designcraft_doc::FirstBaseline,
    min: f64,
    text: &str,
    para: ParaAttrs,
    edit: impl FnOnce(&mut Document, StoryId),
) -> (Vec<Line>, FrameRows) {
    let (mut d, sid, fid) = doc_with(text, Rect::new(0.0, 0.0, 300.0, 400.0), para);
    d.story_mut(sid).unwrap().frame_grid = Some(FrameGrid { size: 12.0, line_aki: 6.0, ..Default::default() });
    if let Some(tf) = d.item_mut(fid).and_then(|i| i.text_frame_mut()) {
        tf.options.first_baseline = fb;
        tf.options.first_baseline_min = min;
    }
    edit(&mut d, sid);
    let rows = frame_specs(&d, sid)[0].frame_grid.expect("a frame grid frame has rows");
    let cs = compose_story(&d, sid, &ComposeOptions::default());
    (all_lines(&cs).into_iter().cloned().collect(), rows)
}

fn aligned(r: R) -> ParaAttrs {
    ParaAttrs { grid_align: Some(GridAlign::AllLines), grid_reference: Some(r), ..Default::default() }
}

/// A line's em box (top, bottom), y down, as the composer measures it for grid alignment: its
/// largest glyph's em, split by the face's ascent and descent.
fn em_box(l: &Line) -> (f64, f64) {
    let g = l.glyphs.iter().max_by(|a, b| a.sy.abs().total_cmp(&b.sy.abs())).unwrap();
    let (a, b) = g.face.vertical_metrics();
    let em = g.face.units_per_em() * g.sy.abs();
    let base = l.baseline + g.y;
    (base - em * a / (a + b), base + em * b / (a + b))
}

fn em_centre_of(l: &Line) -> f64 {
    let (t, b) = em_box(l);
    (t + b) / 2.0
}

fn close(a: f64, b: f64) -> bool {
    (a - b).abs() < EPS
}

#[test]
fn a_paragraphs_grid_alignment_puts_its_lines_on_the_rows() {
    for (r, at) in [(R::EmTop, 0.0), (R::EmCenter, 6.0), (R::EmBottom, 12.0)] {
        let (lines, rows) = grid_lines("AB\nCD\nEF", aligned(r), |_, _| {});
        assert_eq!(lines.len(), 3);
        let y: Vec<f64> = lines
            .iter()
            .map(|l| {
                let (t, b) = em_box(l);
                match r {
                    R::EmTop => t,
                    R::EmCenter => (t + b) / 2.0,
                    _ => b,
                }
            })
            .collect();
        // Each line on a row, one row after the other: the rows are farther apart than the leading.
        for (k, w) in y.windows(2).enumerate() {
            assert!(close(w[1] - w[0], rows.pitch), "{r:?} line {k}: {y:?}");
        }
        assert!(close(y[0], rows.top + at), "{r:?} on the first row's {at}: {y:?}");
    }
}

#[test]
fn the_first_line_is_on_the_first_row_with_an_ascent_first_baseline() {
    // InDesign: Ascent, Em Box Height and a 0 fixed offset all put a 12 pt line on the first row,
    // even in a face whose ascender reaches above its em box; Leading puts it on the second.
    for (fb, row) in [
        (designcraft_doc::FirstBaseline::Ascent, 0.0),
        (designcraft_doc::FirstBaseline::EmboxHeight, 0.0),
        (designcraft_doc::FirstBaseline::Leading, 1.0),
    ] {
        let (lines, rows) = grid_lines_with(fb, 0.0, "漢字", aligned(R::EmCenter), |_, _| {});
        let want = rows.top + row * rows.pitch + rows.cell / 2.0;
        assert!(close(em_centre_of(&lines[0]), want), "{fb:?}: {} not {want}", em_centre_of(&lines[0]));
    }
}

/// Lines of a `size` pt heading and a 12 pt body aligned by `r`, the heading's em box top on the
/// frame top before the grid moves it (InDesign's Em Box Height first baseline in a frame grid).
fn heading(size: f64, r: R) -> (Vec<Line>, FrameRows) {
    let big = move |d: &mut Document, sid: StoryId| d.story_mut(sid).unwrap().format_chars(0..4, |f| f.over.size = Some(size));
    let (probe, _) = grid_lines_from(0.0, "Head\nBody", ParaAttrs::default(), big);
    let above = probe[0].baseline - em_box(&probe[0]).0;
    grid_lines_from(above, "Head\nBody", aligned(r), big)
}

#[test]
fn a_heading_larger_than_a_cell_takes_the_first_rows_its_em_box_needs() {
    // 20 pt in 12 pt cells: rows 0–1 (0..30), centred at 15, though its natural em centre (10) is
    // below the first row's (6); the body goes on row 2 (centre 42).
    let (lines, rows) = heading(20.0, R::EmCenter);
    let (head, body) = (em_centre_of(&lines[0]), em_centre_of(&lines[1]));
    assert!(close(head, rows.top + 15.0), "{head}");
    assert!(close(body, rows.top + 2.0 * rows.pitch + 6.0), "{body}");
    // 40 pt: three rows (0..48), centred at 24.
    let (lines, rows) = heading(40.0, R::EmCenter);
    assert!(close(em_centre_of(&lines[0]), rows.top + 24.0), "{}", em_centre_of(&lines[0]));
    assert!(close(em_centre_of(&lines[1]), rows.top + 3.0 * rows.pitch + 6.0));
    // Em box bottom: a 30 pt heading's bottom on the second row's bottom (30), where it already is.
    let (lines, rows) = heading(30.0, R::EmBottom);
    assert!(close(em_box(&lines[0]).1, rows.top + rows.pitch + rows.cell), "{:?}", em_box(&lines[0]));
}

#[test]
fn icf_alignment_puts_the_em_box_edge_on_the_cells_icf_whatever_the_size() {
    // InDesign: 6 to 30 pt text with ICF Top all start the same way into a 12 pt cell.
    let top = |size: f64| {
        let (lines, rows) = heading(size, R::IcfTop);
        em_box(&lines[0]).0 - rows.top
    };
    let (small, large) = (top(8.0), top(20.0));
    assert!(close(small, large), "{small} {large}");
    assert!(small > 0.0 && small < 0.5 * 12.0, "inside the cell: {small}");
    // ICF Bottom: the em box bottom as far above the cell's bottom as ICF Top is below its top.
    let (lines, rows) = heading(8.0, R::IcfBottom);
    let below = rows.top + rows.cell - em_box(&lines[0]).1;
    assert!(close(below, small), "{below} {small}");
}

#[test]
fn roman_baseline_alignment_puts_the_baseline_on_the_last_rows_baseline() {
    // A 20 pt line takes rows 0–1; its baseline goes where a cell-sized em box on row 1 has its own.
    let (lines, rows) = heading(20.0, R::Baseline);
    let (top, bottom) = em_box(&lines[0]);
    let roman = (lines[0].baseline - top) / (bottom - top);
    assert!(close(lines[0].baseline, rows.top + rows.pitch + roman * rows.cell), "{} {roman}", lines[0].baseline);
}

#[test]
fn gyoudori_given_is_used_as_given() {
    // Three rows for a 12 pt line: centred on the middle row; one row for a 20 pt line, even though
    // its em box overflows it.
    let (lines, rows) = grid_lines("Head\nBody", aligned(R::EmCenter), |d, sid| {
        d.story_mut(sid).unwrap().format_paras(0..4, |p| p.para.grid_gyoudori = Some(3));
    });
    assert!(close(em_centre_of(&lines[0]), rows.top + rows.pitch + 6.0));
    assert!(close(em_centre_of(&lines[1]), rows.top + 3.0 * rows.pitch + 6.0));
    let (lines, rows) = grid_lines("Head\nBody", aligned(R::EmCenter), |d, sid| {
        let st = d.story_mut(sid).unwrap();
        st.format_chars(0..4, |f| f.over.size = Some(20.0));
        st.format_paras(0..4, |p| p.para.grid_gyoudori = Some(1));
    });
    assert!(close(em_centre_of(&lines[0]), rows.top + 6.0));
    assert!(close(em_centre_of(&lines[1]), rows.top + rows.pitch + 6.0));
}

#[test]
fn a_paragraph_aligned_higher_in_its_row_takes_the_next_row() {
    // Em Box Bottom, then Em Box Top (and the reverse, and Roman Baseline after Em Box Top): the
    // second paragraph goes on the row after the first, not one row further.
    for (a, b) in [(R::EmBottom, R::EmTop), (R::EmTop, R::EmBottom), (R::EmTop, R::Baseline), (R::Baseline, R::EmTop)] {
        let (lines, rows) = grid_lines("AB\nCD", aligned(a), |d, sid| {
            d.story_mut(sid).unwrap().format_paras(3..5, |p| p.para.grid_reference = Some(b));
        });
        let tops: Vec<f64> = lines.iter().map(|l| ((em_box(l).0 - rows.top) / rows.pitch).round()).collect();
        assert_eq!(tops, [0.0, 1.0], "{a:?} then {b:?}");
    }
}

#[test]
fn an_empty_paragraph_takes_the_rows_its_marks_em_box_needs() {
    // InDesign: an empty paragraph whose mark is 36 pt takes three 12 pt rows (18 pt apart), so
    // the next line starts below them.
    let first_row_after = |mark: f64| {
        let (lines, rows) = grid_lines("AB\n\nCD", aligned(R::EmCenter), |d, sid| {
            // Leading below a cell, so only the em box can make the mark take more rows.
            d.story_mut(sid).unwrap().format_chars(3..4, |f| {
                f.over.size = Some(mark);
                f.over.leading = Some(designcraft_doc::Leading::Points(6.0));
            });
        });
        let last = lines.last().unwrap();
        ((em_box(last).0 - rows.top) / rows.pitch).round()
    };
    assert_eq!(first_row_after(12.0), 2.0, "a 12 pt mark takes one row");
    assert!(first_row_after(36.0) >= 4.0, "a 36 pt mark takes rows 1 to 3: {}", first_row_after(36.0));
}

#[test]
fn an_empty_paragraphs_mark_box_is_scaled_vertically() {
    // A 12 pt mark at 300% vertical scale has a 36 pt em box, as a glyph of that style has, so it
    // takes the rows a 36 pt mark takes.
    let (lines, rows) = grid_lines("AB\n\nCD", aligned(R::EmCenter), |d, sid| {
        d.story_mut(sid).unwrap().format_chars(3..4, |f| {
            f.over.v_scale = Some(3.0);
            f.over.leading = Some(designcraft_doc::Leading::Points(6.0));
        });
    });
    let after = ((em_box(lines.last().unwrap()).0 - rows.top) / rows.pitch).round();
    assert!(after >= 4.0, "a 36 pt high mark takes rows 1 to 3: {after}");
}

#[test]
fn vertical_justification_moves_frame_grid_lines_by_whole_rows() {
    // InDesign: Bottom puts the last line on the last row and Center moves the lines by whole rows.
    for vj in [designcraft_doc::VerticalJustification::Bottom, designcraft_doc::VerticalJustification::Center] {
        let (lines, rows) = grid_lines("AB\nCD\nEF", aligned(R::EmCenter), |d, sid| {
            let fid = d.story(sid).unwrap().frames[0];
            d.item_mut(fid).unwrap().text_frame_mut().unwrap().options.vertical_justification = vj;
        });
        for l in &lines {
            let at = (em_box(l).0 - rows.top) / rows.pitch;
            assert!((at - at.round()).abs() < 1e-6, "{vj:?}: a line between rows ({at})");
        }
        let last = (em_box(lines.last().unwrap()).0 - rows.top) / rows.pitch;
        assert!(last.round() > 2.0, "{vj:?}: the lines moved ({last})");
        if vj == designcraft_doc::VerticalJustification::Bottom {
            let next = rows.top + (last.round() + 1.0) * rows.pitch + rows.cell;
            assert!(next > 400.0 + 1e-6, "{vj:?}: a row is left below the last line ({last})");
        }
    }
}

#[test]
fn vertical_justification_uses_the_frame_grid_over_a_baseline_grid() {
    // A caller may give a frame both grids. Lines are placed on the frame grid's rows, so
    // justification moves them by its rows, not by the baseline grid's 10 pt steps.
    let (mut d, sid, fid) = doc_with("AB\nCD\nEF", Rect::new(0.0, 0.0, 300.0, 400.0), aligned(R::EmCenter));
    d.story_mut(sid).unwrap().frame_grid = Some(FrameGrid { size: 12.0, line_aki: 6.0, ..Default::default() });
    if let Some(tf) = d.item_mut(fid).and_then(|i| i.text_frame_mut()) {
        tf.options.first_baseline = designcraft_doc::FirstBaseline::Fixed;
        tf.options.vertical_justification = designcraft_doc::VerticalJustification::Bottom;
    }
    let mut specs = frame_specs(&d, sid);
    let rows = specs[0].frame_grid.expect("a frame grid frame has rows");
    specs[0].grid = Some((rows.top, 10.0));
    let cs = compose(&d, d.story(sid).unwrap(), &specs, &ComposeOptions::default());
    let lines: Vec<&Line> = all_lines(&cs);
    for l in &lines {
        let at = (em_box(l).0 - rows.top) / rows.pitch;
        assert!((at - at.round()).abs() < 1e-6, "a line between rows ({at})");
    }
    let last = (em_box(lines.last().unwrap()).0 - rows.top) / rows.pitch;
    assert!(rows.top + (last.round() + 1.0) * rows.pitch + rows.cell > 400.0 + 1e-6, "a row is left below the last line ({last})");
}

/// The row the first of `lines` one-line paragraphs sits on, in a frame grid of 12 pt cells 18 pt
/// apart whose rows run `extent` pt, justified `vj`.
fn justified_first_row(vertical: bool, extent: f64, lines: usize, vj: designcraft_doc::VerticalJustification) -> i64 {
    let text = vec!["AB"; lines].join("\n");
    let rect = if vertical { Rect::new(0.0, 0.0, extent, 400.0) } else { Rect::new(0.0, 0.0, 400.0, extent) };
    let (mut d, sid, fid) = doc_with(&text, rect, aligned(R::EmCenter));
    let st = d.story_mut(sid).unwrap();
    st.frame_grid = Some(FrameGrid { size: 12.0, line_aki: 6.0, ..Default::default() });
    st.vertical = vertical;
    let tf = d.item_mut(fid).unwrap().text_frame_mut().unwrap();
    tf.options.first_baseline = designcraft_doc::FirstBaseline::Fixed;
    tf.options.vertical_justification = vj;
    let rows = frame_specs(&d, sid)[0].frame_grid.expect("a frame grid frame has rows");
    let cs = compose_story(&d, sid, &ComposeOptions::default());
    let at = (em_box(all_lines(&cs)[0]).0 - rows.top) / rows.pitch;
    assert!((at - at.round()).abs() < 1e-6, "a line between rows ({at})");
    at.round() as i64
}

/// Frame extent, lines, and the first line's row with Center and with Bottom, as measured in
/// InDesign 20.4.1: 300 pt holds 17 rows and 282 pt holds 16.
const JUSTIFIED_ROWS: [(f64, usize, i64, i64); 4] = [(300.0, 2, 7, 15), (300.0, 3, 7, 14), (282.0, 2, 7, 14), (282.0, 3, 6, 13)];

#[test]
fn an_empty_paragraph_sits_where_a_character_typed_in_it_does() {
    // The rows' roman baseline and ICF come from the face: an empty paragraph takes its mark's
    // face, so typing a character there doesn't move the line.
    for r in [R::Baseline, R::IcfTop, R::IcfBottom, R::EmCenter] {
        let second = |text: &str| grid_lines(text, aligned(r), |_, _| {}).0[1].baseline;
        assert!(close(second("AB\n\nCD"), second("AB\nX\nCD")), "{r:?}: empty {} typed {}", second("AB\n\nCD"), second("AB\nX\nCD"));
    }
}

#[test]
fn justify_keeps_the_first_and_last_lines_on_rows_and_spreads_the_rest() {
    // Measured in InDesign 20.4.1, the same in both directions: in 16 rows, 3 lines go on row 0,
    // halfway between rows 7 and 8, and on row 15; 4 lines in 5 rows go 24 pt apart, off the
    // 18 pt rows.
    let tops = |extent: f64, lines: usize| {
        let text = vec!["AB"; lines].join("\n");
        let (mut d, sid, fid) = doc_with(&text, Rect::new(0.0, 0.0, 400.0, extent), aligned(R::EmCenter));
        d.story_mut(sid).unwrap().frame_grid = Some(FrameGrid { size: 12.0, line_aki: 6.0, ..Default::default() });
        let tf = d.item_mut(fid).unwrap().text_frame_mut().unwrap();
        tf.options.first_baseline = designcraft_doc::FirstBaseline::Fixed;
        tf.options.vertical_justification = designcraft_doc::VerticalJustification::Justify;
        let cs = compose_story(&d, sid, &ComposeOptions::default());
        all_lines(&cs).iter().map(|l| em_box(l).0).collect::<Vec<f64>>()
    };
    for (extent, lines, expected) in [(282.0, 3, vec![0.0, 135.0, 270.0]), (84.0, 4, vec![0.0, 24.0, 48.0, 72.0]), (300.0, 2, vec![0.0, 288.0])] {
        let got = tops(extent, lines);
        assert!(got.len() == expected.len() && got.iter().zip(&expected).all(|(a, b)| close(*a, *b)), "{extent} pt, {lines} lines: {got:?}");
    }
}

#[test]
fn centre_leaves_the_odd_row_after_the_lines() {
    use designcraft_doc::VerticalJustification::{Bottom, Center};
    for (extent, lines, centre, bottom) in JUSTIFIED_ROWS {
        assert_eq!(justified_first_row(false, extent, lines, Center), centre, "Center, {extent} pt, {lines} lines");
        assert_eq!(justified_first_row(false, extent, lines, Bottom), bottom, "Bottom, {extent} pt, {lines} lines");
    }
}

#[test]
fn space_before_moves_a_line_to_the_next_row_it_reaches() {
    // Space before 20 takes the next line past row 1 to row 2; 5 keeps it on row 1.
    for (space, row) in [(5.0, 1.0), (20.0, 2.0)] {
        let (lines, rows) = grid_lines("AB\nCD", aligned(R::EmTop), |d, sid| {
            let st = d.story_mut(sid).unwrap();
            // Leading as solid as the cell, as Apply Grid Format leaves it (100 % auto leading).
            st.format_chars(0..5, |f| f.over.leading = Some(designcraft_doc::Leading::Points(12.0)));
            st.format_paras(3..5, |p| p.para.space_before = Some(space));
        });
        assert!(close(em_box(&lines[1]).0, rows.top + row * rows.pitch), "{space}: {:?}", em_box(&lines[1]));
    }
}

#[test]
fn paragraphs_not_aligned_to_the_grid_follow_their_leading() {
    let (lines, rows) = grid_lines("AB\nCD", ParaAttrs::default(), |_, _| {});
    let step = lines[1].baseline - lines[0].baseline;
    assert!(!close(step, rows.pitch), "no snapping: {step}");
}

#[test]
fn a_frame_grid_replaces_the_baseline_grid() {
    let (lines, rows) = grid_lines("AB\nCD", aligned(R::Baseline), |d, sid| {
        d.settings.baseline_grid.start = 3.0;
        d.settings.baseline_grid.increment = 10.0;
        let fid = d.story(sid).unwrap().frames[0];
        if let Some(tf) = d.item_mut(fid).and_then(|i| i.text_frame_mut()) {
            tf.options.baseline_grid = Some(designcraft_doc::FrameBaselineGrid::new(0.0, 10.0));
        }
    });
    // On the rows (18 apart), not on the 10 pt baseline grids.
    assert!(close(lines[1].baseline - lines[0].baseline, rows.pitch), "{} {}", lines[0].baseline, lines[1].baseline);
}

#[test]
fn hostile_grids_compose_without_panicking() {
    for g in [
        FrameGrid { size: f64::NAN, line_aki: f64::INFINITY, ..Default::default() },
        FrameGrid { size: 0.0, line_aki: -1e9, v_scale: 0.0, ..Default::default() },
        FrameGrid { size: 1e12, h_scale: f64::NEG_INFINITY, ..Default::default() },
    ] {
        let (mut d, sid, _) = doc_with("AB\nCD", Rect::new(0.0, 0.0, 300.0, 400.0), aligned(R::EmCenter));
        d.story_mut(sid).unwrap().frame_grid = Some(g);
        let rows = frame_specs(&d, sid)[0].frame_grid.unwrap();
        assert!(rows.pitch > 0.0 && rows.cell > 0.0, "{rows:?}");
        let cs = compose_story(&d, sid, &ComposeOptions::default());
        assert!(all_lines(&cs).iter().all(|l| l.baseline.is_finite()));
    }
}
