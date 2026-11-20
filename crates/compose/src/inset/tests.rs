use super::*;

fn config(main: usize) -> Config {
    Config { bounds: Rect::new(0.0, 0.0, 600.0, 800.0), gutter: 12.0, main, mirror: false, position: None }
}

fn area(cfg: Config, areas: &[f64]) -> Layout<f64, std::ops::Range<f64>> {
    let streams = areas.iter().map(|&a| Stream { cursor: a, ratio: 1.0, finished: a == 0.0 }).collect();
    layout(cfg, streams, |i, &remaining, rect| {
        let used = remaining.min(rect.width() * rect.height());
        Measurement {
            cursor: remaining - used,
            height: used / rect.width(),
            finished: remaining - used < EPS,
            placement: (areas[i] - remaining)..(areas[i] - remaining + used),
        }
    })
    .unwrap()
}

fn rects<C, P>(out: &Layout<C, P>, stream: usize) -> Vec<Rect> {
    out.parts.iter().filter(|p| p.stream == stream).map(|p| p.rect).collect()
}

fn near(actual: &[Rect], expected: &[Rect]) {
    assert_eq!(actual.len(), expected.len());
    for (a, e) in actual.iter().zip(expected) {
        for (a, e) in [a.x0, a.y0, a.x1, a.y1].into_iter().zip([e.x0, e.y0, e.x1, e.y1]) {
            assert!((a - e).abs() < EPS, "{actual:?} != {expected:?}");
        }
    }
}

fn continuous(out: &Layout<f64, std::ops::Range<f64>>) {
    for i in 0..out.streams.len() {
        let mut end = 0.0;
        for p in out.parts.iter().filter(|p| p.stream == i) {
            assert!((p.placement.start - end).abs() < EPS);
            end = p.placement.end;
        }
    }
}

#[test]
fn both_commentaries_short_example_g_and_mirror() {
    let cfg = config(1);
    let areas = [38400.0, 185400.0, 86400.0];
    let out = area(cfg, &areas);
    near(&rects(&out, 0), &[Rect::new(0.0, 0.0, 192.0, 200.0)]);
    near(&rects(&out, 1), &[Rect::new(204.0, 0.0, 396.0, 200.0), Rect::new(0.0, 200.0, 396.0, 450.0), Rect::new(0.0, 450.0, 600.0, 530.0)]);
    near(&rects(&out, 2), &[Rect::new(408.0, 0.0, 600.0, 200.0), Rect::new(408.0, 200.0, 600.0, 450.0)]);
    let mirrored = area(Config { mirror: true, ..cfg }, &areas);
    for (a, m) in out.parts.iter().zip(&mirrored.parts) {
        near(&[m.rect], &[Rect::new(600.0 - a.rect.x1, a.rect.y0, 600.0 - a.rect.x0, a.rect.y1)]);
        assert_eq!(a.placement, m.placement);
    }
    assert!(out.streams.iter().all(|s| s.finished));
    continuous(&out);
}

#[test]
fn main_shortest_sides_share_hole_then_last_stream_full_width() {
    let out = area(config(1), &[67800.0, 38400.0, 127800.0]);
    near(&rects(&out, 0), &[Rect::new(0.0, 0.0, 192.0, 200.0), Rect::new(0.0, 200.0, 294.0, 300.0)]);
    near(&rects(&out, 2), &[Rect::new(408.0, 0.0, 600.0, 200.0), Rect::new(306.0, 200.0, 600.0, 300.0), Rect::new(0.0, 300.0, 600.0, 400.0)]);
    continuous(&out);
}

#[test]
fn main_ends_before_surviving_right_commentary() {
    let out = area(config(1), &[38400.0, 78000.0, 117600.0]);
    near(&rects(&out, 1), &[Rect::new(204.0, 0.0, 396.0, 200.0), Rect::new(0.0, 200.0, 396.0, 300.0)]);
    near(&rects(&out, 2), &[Rect::new(408.0, 0.0, 600.0, 200.0), Rect::new(408.0, 200.0, 600.0, 300.0), Rect::new(0.0, 300.0, 600.0, 400.0)]);
}

#[test]
fn two_streams_fill_exactly() {
    // The issue's rectangles total 369840, not its stated 374640 units of main area.
    let out = area(config(1), &[105840.0, 369840.0]);
    near(&rects(&out, 0), &[Rect::new(0.0, 0.0, 294.0, 360.0)]);
    near(&rects(&out, 1), &[Rect::new(306.0, 0.0, 600.0, 360.0), Rect::new(0.0, 360.0, 600.0, 800.0)]);
    assert!(out.streams.iter().all(|s| s.finished));
    let overset = area(config(1), &[105840.0, 374640.0]);
    near(&rects(&overset, 1), &rects(&out, 1));
    assert!(!overset.streams[1].finished);
    assert!((overset.streams[1].cursor - 4800.0).abs() < EPS);
}

#[test]
fn empty_stream_has_no_gutter() {
    let out = area(config(1), &[88200.0, 328200.0, 0.0]);
    near(&rects(&out, 1), &[Rect::new(306.0, 0.0, 600.0, 300.0), Rect::new(0.0, 300.0, 600.0, 700.0)]);
    assert!(rects(&out, 2).is_empty());
}

#[test]
fn simultaneous_ends_leave_together() {
    let out = area(config(1), &[38400.0, 158400.0, 38400.0]);
    near(&rects(&out, 1), &[Rect::new(204.0, 0.0, 396.0, 200.0), Rect::new(0.0, 200.0, 600.0, 400.0)]);
}

#[test]
fn four_streams_preserve_live_outside_columns() {
    let out = area(config(1), &[150000.0, 158760.0, 25380.0, 50760.0]);
    near(&rects(&out, 1), &[Rect::new(153.0, 0.0, 294.0, 180.0), Rect::new(153.0, 180.0, 447.0, 360.0), Rect::new(153.0, 360.0, 600.0, 540.0)]);
    let left = rects(&out, 0);
    assert!(left.iter().take(3).all(|r| r.x0 == 0.0 && r.x1 == 141.0));
    assert_eq!(left.last().unwrap().x1, 600.0);
    continuous(&out);
}

#[test]
fn positioned_start_is_a_geometry_event() {
    let cfg = Config { position: Some(MainPosition { top: 120.0, left: 210.0, right: 162.0 }), ..config(1) };
    let out = area(cfg, &[200000.0, 22800.0, 200000.0]);
    near(&rects(&out, 1), &[Rect::new(210.0, 120.0, 438.0, 220.0)]);
    let l = rects(&out, 0);
    near(&l[..2], &[Rect::new(0.0, 0.0, 294.0, 120.0), Rect::new(0.0, 120.0, 198.0, 220.0)]);
    let r = rects(&out, 2);
    near(&r[..2], &[Rect::new(306.0, 0.0, 600.0, 120.0), Rect::new(450.0, 120.0, 600.0, 220.0)]);
    continuous(&out);
}

#[test]
fn clipped_whole_lines_keep_cursor_and_baseline() {
    #[derive(Clone)]
    struct Cursor {
        next: usize,
        baseline: f64,
    }
    let streams = [5, 12, 20].into_iter().map(|_| Stream { cursor: Cursor { next: 0, baseline: 8.0 }, ratio: 1.0, finished: false }).collect();
    let out = layout(config(1), streams, |i, c, r| {
        let counts = [5, 12, 20];
        let lead = [11.0, 15.0, 13.0][i];
        let mut cursor = c.clone();
        let mut lines = Vec::new();
        while cursor.next < counts[i] && cursor.baseline + 2.0 <= r.y1 + EPS {
            lines.push((cursor.next, cursor.baseline));
            cursor.next += 1;
            cursor.baseline += lead;
        }
        let height = lines.last().map_or(0.0, |(_, b)| b + 2.0 - r.y0);
        let finished = cursor.next == counts[i];
        Measurement { cursor, height, finished, placement: lines }
    })
    .unwrap();
    for i in 0..3 {
        let lines: Vec<_> = out.parts.iter().filter(|p| p.stream == i).flat_map(|p| &p.placement).copied().collect();
        for (k, &(source, baseline)) in lines.iter().enumerate() {
            assert_eq!(source, k);
            assert_eq!(baseline, 8.0 + k as f64 * [11.0, 15.0, 13.0][i]);
        }
        assert_eq!(lines.len(), [5, 12, 20][i]);
    }
}

#[test]
fn invalid_inputs_and_measurements_return_errors() {
    let stream = || vec![Stream { cursor: (), ratio: 1.0, finished: false }];
    let measure = |_, _: &(), _: Rect| Measurement { cursor: (), height: f64::NAN, finished: false, placement: () };
    assert_eq!(layout(config(0), stream(), measure).unwrap_err(), Error::InvalidMeasurement);
    assert_eq!(layout(Config { gutter: f64::NAN, ..config(0) }, stream(), measure).unwrap_err(), Error::InvalidGeometry);
    assert_eq!(layout(config(1), stream(), measure).unwrap_err(), Error::InvalidGeometry);
}

#[test]
fn every_acceptance_geometry_mirrors_without_changing_source() {
    let cases: &[(usize, &[f64])] = &[
        (1, &[67800.0, 38400.0, 127800.0]),
        (1, &[38400.0, 78000.0, 117600.0]),
        (1, &[105840.0, 374640.0]),
        (1, &[88200.0, 328200.0, 0.0]),
        (1, &[38400.0, 158400.0, 38400.0]),
        (1, &[150000.0, 158760.0, 25380.0, 50760.0]),
    ];
    for &(main, areas) in cases {
        let out = area(config(main), areas);
        let mirror = area(Config { mirror: true, ..config(main) }, areas);
        assert_eq!(out.parts.len(), mirror.parts.len());
        for (a, b) in out.parts.iter().zip(&mirror.parts) {
            assert_eq!(a.stream, b.stream);
            assert_eq!(a.placement, b.placement);
            near(&[b.rect], &[Rect::new(600.0 - a.rect.x1, a.rect.y0, 600.0 - a.rect.x0, a.rect.y1)]);
        }
        continuous(&mirror);
    }
}
