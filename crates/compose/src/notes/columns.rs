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
                bottom = bottom.max(y + z.baseline + z.descent);
                out.push(Fragment {
                    note,
                    column: col,
                    lines: start..end,
                    y,
                    top: y + first.baseline - first.ascent,
                    bottom: y + z.baseline + z.descent,
                });
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
            high = high.max(y + z.baseline + z.descent);
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
            let y1 = z.baseline + z.descent;
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
