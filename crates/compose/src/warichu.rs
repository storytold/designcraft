//! Warichu (割り注): an inline note set in two or more smaller lines inside one parent em.
//!
//! Placement happens after the line is broken, in the composer's along/across space (`x` runs
//! along the line). The run gets shorter and the rest of the line closes up. Characters that did
//! not fit the line stay on the next line. Right-to-left and tate-chu-yoko runs are left alone.

use designcraft_doc::cjk::WarichuAlignment;

use crate::{PlacedGlyph, RunStyle};

pub(crate) fn place(styles: &[RunStyle], line: &mut [PlacedGlyph]) -> f64 {
    if !line.iter().any(|g| is_warichu(styles, g)) {
        return 0.0;
    }
    let mut delta = 0.0;
    let mut i = 0;
    while i < line.len() {
        if !is_warichu(styles, &line[i]) {
            i += 1;
            continue;
        }
        let Some(key) = key(styles, &line[i]) else {
            i += 1;
            continue;
        };
        let mut j = i + 1;
        while j < line.len() {
            let g = &line[j];
            if g.len == 0 {
                j += 1;
                continue;
            }
            if is_warichu(styles, g) && key_of(styles, g) == Some(key) {
                j += 1;
                continue;
            }
            break;
        }
        let members: Vec<usize> = (i..j).filter(|&k| is_warichu(styles, &line[k])).collect();
        if members.is_empty() || members.iter().any(|&k| line[k].rtl || line[k].tcy.is_some()) {
            i = j;
            continue;
        }
        delta += layout(styles, line, i, j, &members);
        i = j;
    }
    delta
}

fn style<'a>(styles: &'a [RunStyle], g: &PlacedGlyph) -> Option<&'a RunStyle> {
    styles.get(g.style as usize)
}

fn is_warichu(styles: &[RunStyle], g: &PlacedGlyph) -> bool {
    g.len > 0 && g.visible && style(styles, g).is_some_and(|s| s.warichu)
}

type Key = (u32, u64, u64, WarichuAlignment, u32, u32);

fn key(styles: &[RunStyle], g: &PlacedGlyph) -> Option<Key> {
    let s = style(styles, g)?;
    Some((
        s.warichu_lines,
        s.warichu_size.to_bits(),
        s.warichu_line_spacing.to_bits(),
        s.warichu_align,
        s.warichu_chars_before,
        s.warichu_chars_after,
    ))
}

fn key_of(styles: &[RunStyle], g: &PlacedGlyph) -> Option<Key> {
    is_warichu(styles, g).then(|| key(styles, g)).flatten()
}

/// Scale `members` and stack them in `lines`. Returns how far the rest of the line moves.
fn layout(styles: &[RunStyle], line: &mut [PlacedGlyph], start: usize, end: usize, members: &[usize]) -> f64 {
    let Some(st) = members.first().and_then(|&k| style(styles, &line[k])) else {
        return 0.0;
    };
    let lines = (st.warichu_lines as usize).clamp(2, 16);
    let min_line = (st.warichu_chars_before as usize).max(st.warichu_chars_after as usize).max(1);
    let max_lines = (members.len() / min_line).max(1);
    let n = lines.min(max_lines).min(members.len()).max(1);
    let scale = (st.warichu_size / 100.0).clamp(0.05, 4.0);
    let old_x0 = members.iter().map(|&k| line[k].x).fold(f64::INFINITY, f64::min);
    let old_right = members.iter().map(|&k| line[k].x + line[k].adv).fold(f64::NEG_INFINITY, f64::max);
    let old_span = old_right - old_x0;
    if !old_span.is_finite() || old_span <= 0.0 {
        return 0.0;
    }
    for &k in members {
        line[k].sx *= scale;
        line[k].sy *= scale;
        line[k].adv *= scale;
        line[k].y *= scale;
    }
    let mut counts = vec![members.len() / n; n];
    for c in counts.iter_mut().take(members.len() % n) {
        *c += 1;
    }
    let mut widths = Vec::with_capacity(n);
    let mut off = 0;
    for &c in &counts {
        let w: f64 = members[off..off + c].iter().map(|&k| line[k].adv).sum();
        widths.push(w);
        off += c;
    }
    let block = widths.iter().copied().fold(0.0_f64, f64::max);
    if !block.is_finite() {
        return 0.0;
    }
    let small = st.size * scale;
    let gap = if st.warichu_line_spacing > 0.0 { small + st.warichu_line_spacing } else { small };
    let mut off = 0;
    for (li, &c) in counts.iter().enumerate() {
        let w = widths[li];
        let (start_x, extra) = match st.warichu_align {
            WarichuAlignment::Left => (0.0, 0.0),
            WarichuAlignment::Right => ((block - w).max(0.0), 0.0),
            WarichuAlignment::Justify if c > 1 && block > w => (0.0, (block - w) / (c - 1) as f64),
            WarichuAlignment::Auto | WarichuAlignment::Center | WarichuAlignment::Justify => ((block - w) / 2.0, 0.0),
        };
        let y_off = (li as f64 - (n as f64 - 1.0) / 2.0) * gap;
        let mut x = old_x0 + start_x;
        for &k in &members[off..off + c] {
            line[k].x = x;
            line[k].y += y_off;
            x += line[k].adv + extra;
        }
        off += c;
    }
    let delta = block - old_span;
    if delta.abs() > 1e-9 {
        for g in &mut line[end..] {
            g.x += delta;
        }
        for k in start..end {
            if !members.contains(&k) {
                line[k].x += delta;
            }
        }
    }
    delta
}
