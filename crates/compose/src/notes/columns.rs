//! One plan is used by body reservation and final note placement.
use crate::ComposedStory;
use std::{ops::Range, sync::Arc};

pub(super) struct Fragment {
    pub note: usize,
    pub column: usize,
    pub lines: Range<usize>,
    pub y: f64,
    pub top: f64,
    pub bottom: f64,
}

pub(super) fn balance(texts: &[Arc<ComposedStory>], count: usize, space: f64) -> (f64, Vec<Fragment>) {
    let lines: Vec<_> = texts.iter().map(|t| t.frames.first().map(|f| f.lines.as_slice()).unwrap_or_default()).collect();
    let run = |height: f64| {
        let mut out: Vec<Fragment> = Vec::new();
        let (mut col, mut bottom, mut last) = (0, 0.0_f64, None::<f64>);
        for (note, ls) in lines.iter().enumerate() {
            let mut start = 0;
            while let Some(first) = ls.get(start) {
                let first_offset = if start == 0 { first.baseline } else { first.leading };
                let y = last.map_or(first_offset - first.baseline, |b| b + space + first.leading - first.baseline);
                let mut end = start;
                for (i, line) in ls.iter().enumerate().skip(start) {
                    if y + line.baseline + line.descent > height + 1e-7 {
                        break;
                    }
                    let n = i + 1;
                    // Long notes leave at least two lines on both sides of every break.
                    if n == ls.len() || ls.len() < 4 || (n - start >= 2 && ls.len() - n >= 2) {
                        end = n;
                    }
                }
                if end == start {
                    col += 1;
                    if col >= count {
                        return None;
                    }
                    last = None;
                    continue;
                }
                let z = ls.get(end - 1)?;
                let fragment_bottom = ls.get(start..end)?.iter().map(|line| y + line.baseline + line.descent).fold(0.0_f64, f64::max);
                bottom = bottom.max(fragment_bottom);
                out.push(Fragment { note, column: col, lines: start..end, y, top: y + first.baseline - first.ascent, bottom: fragment_bottom });
                last = Some(y + z.baseline);
                start = end;
                if start < ls.len() {
                    col += 1;
                    if col >= count {
                        return None;
                    }
                    last = None;
                }
            }
        }
        Some((bottom, out))
    };
    // A full single-column stack is a feasible upper bound, including inter-note leading.
    let (mut high, mut previous) = (0.0_f64, None::<f64>);
    for ls in &lines {
        if let (Some(a), Some(z)) = (ls.first(), ls.last()) {
            let y = previous.map_or(0.0, |b| b + space + a.leading - a.baseline);
            for line in *ls {
                high = high.max(y + line.baseline + line.descent);
            }
            previous = Some(y + z.baseline);
        }
    }
    high = high.max(1.0);
    let mut low = 0.0;
    // A fixed iteration bound avoids input-dependent searches and gives sub-point precision.
    for _ in 0..48 {
        let mid = (low + high) * 0.5;
        if run(mid).is_some() {
            high = mid;
        } else {
            low = mid;
        }
    }
    run(high).unwrap_or_else(|| {
        // Invalid imported metrics must not silently discard a note's text.
        let mut bottom = 0.0;
        let mut out = Vec::new();
        for (note, ls) in lines.iter().enumerate() {
            if let (Some(a), Some(z)) = (ls.first(), ls.last()) {
                let y = bottom;
                bottom = y + z.baseline + z.descent;
                out.push(Fragment { note, column: 0, lines: 0..ls.len(), y, top: y + a.baseline - a.ascent, bottom });
                bottom += space;
            }
        }
        (bottom, out)
    })
}

/// Keep original line coordinates and byte offsets; placement translates the fragment.
pub(super) fn fragment(text: &Arc<ComposedStory>, range: Range<usize>) -> Arc<ComposedStory> {
    let mut cs = (**text).clone();
    if let Some(ft) = cs.frames.first_mut() {
        let first = ft.lines.get(range.start);
        let last = ft.lines.get(range.end.saturating_sub(1));
        if let (Some(a), Some(z)) = (first, last) {
            let y0 = a.baseline - a.ascent;
            let y1 = ft.lines.get(range.clone()).unwrap_or_default().iter().map(|l| l.baseline + l.descent).fold(z.baseline + z.descent, f64::max);
            ft.range = a.range.start..z.range.end;
            ft.decos.retain_mut(|d| {
                d.rect.y0 = d.rect.y0.max(y0);
                d.rect.y1 = d.rect.y1.min(y1);
                d.rect.y1 > d.rect.y0
            });
            ft.objects.retain(|o| range.contains(&o.line));
            for o in &mut ft.objects {
                o.line -= range.start;
            }
            ft.tables.retain(|t| range.contains(&t.line));
            for t in &mut ft.tables {
                t.line -= range.start;
            }
        }
        ft.lines = ft.lines.get(range).unwrap_or_default().to_vec();
    }
    Arc::new(cs)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn footnote_upper_bound_includes_earlier_line_descent() {
        let mut d = designcraft_doc::Document::new(&designcraft_doc::build::NewDocument::default());
        let (_, sid) = d
            .add_text_frame(
                designcraft_doc::SpreadRef::Doc(0),
                designcraft_geom::Rect::new(0.0, 0.0, 200.0, 500.0),
                d.default_layer(),
                "First.\nSecond.",
                Default::default(),
            )
            .unwrap();
        let base = crate::compose_story(&d, sid, &Default::default());
        let texts: Vec<_> = [40.0, 25.0]
            .into_iter()
            .map(|descent| {
                let mut t = base.clone();
                let lines = &mut t.frames[0].lines;
                assert_eq!(lines.len(), 2);
                for (i, line) in lines.iter_mut().enumerate() {
                    line.baseline = 10.0 + 10.0 * i as f64;
                    line.leading = 10.0;
                    line.descent = if i == 0 { descent } else { 2.0 };
                }
                Arc::new(t)
            })
            .collect();
        let (height, fragments) = balance(&texts, 2, 0.0);
        assert!((height - 50.0).abs() < 0.01);
        assert_eq!(fragments.iter().map(|f| f.column).collect::<Vec<_>>(), [0, 1]);
    }
}
