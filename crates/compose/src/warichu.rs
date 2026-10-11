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
        delta += layout(styles, line, &members);
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

/// How many small lines `len` glyphs can fill. The first line keeps at least `before` glyphs and
/// the last at least `after`; every other line keeps one. Fewer lines than `want` when the run is
/// too short for that break.
fn line_count(len: usize, want: usize, before: usize, after: usize) -> usize {
    if len == 0 {
        return 1;
    }
    let mut n = want.clamp(1, 16).min(len);
    let before = before.max(1);
    let after = after.max(1);
    while n >= 2 {
        // The minimums come from documents and commands: on 32-bit targets their sum can overflow.
        if before.saturating_add(after).saturating_add(n - 2) <= len {
            return n;
        }
        n -= 1;
    }
    1
}

/// Glyph counts for `n` lines. Minimises the widest line, and the first and last lines keep their
/// break minimums. One line when no split satisfies them.
fn split_even(advs: &[f64], n: usize, before: usize, after: usize) -> Vec<usize> {
    let m = advs.len();
    if n <= 1 || m == 0 {
        return vec![m];
    }
    let before = before.clamp(1, m);
    let after = after.clamp(1, m);
    let mut pref = vec![0.0_f64; m + 1];
    for (i, a) in advs.iter().enumerate() {
        pref[i + 1] = pref[i] + *a;
    }
    let mut best = vec![vec![f64::INFINITY; m + 1]; n + 1];
    let mut take = vec![vec![0_usize; m + 1]; n + 1];
    for s in 0..m {
        if m - s >= after {
            best[1][s] = pref[m] - pref[s];
            take[1][s] = m - s;
        }
    }
    for lines in 2..=n {
        for s in 0..m {
            let min_t = if lines == n { before } else { 1 };
            let min_leave = after + lines - 2;
            if s + min_t + min_leave > m {
                continue;
            }
            let max_t = m - s - min_leave;
            let mut local = f64::INFINITY;
            let mut best_t = min_t;
            for t in min_t..=max_t {
                let rest = best[lines - 1][s + t];
                if !rest.is_finite() {
                    continue;
                }
                let mx = (pref[s + t] - pref[s]).max(rest);
                if mx < local {
                    local = mx;
                    best_t = t;
                }
            }
            best[lines][s] = local;
            take[lines][s] = best_t;
        }
    }
    if !best[n][0].is_finite() {
        return vec![m];
    }
    let mut counts = Vec::with_capacity(n);
    let mut s = 0_usize;
    for lines in (1..=n).rev() {
        let t = take[lines][s];
        if t == 0 || s + t > m {
            return vec![m];
        }
        counts.push(t);
        s += t;
    }
    if s != m { vec![m] } else { counts }
}

fn restore(line: &mut [PlacedGlyph], members: &[usize], saved: &[(f64, f64, f64, f64)]) {
    for (i, &k) in members.iter().enumerate() {
        let Some((y, adv, sx, sy)) = saved.get(i).copied() else { continue };
        let Some(g) = line.get_mut(k) else { continue };
        g.y = y;
        g.adv = adv;
        g.sx = sx;
        g.sy = sy;
    }
}

/// Scale `members` and stack them in `lines`. Returns how far the rest of the line moves.
fn layout(styles: &[RunStyle], line: &mut [PlacedGlyph], members: &[usize]) -> f64 {
    let Some(st) = members.first().and_then(|&k| style(styles, &line[k])) else {
        return 0.0;
    };
    let before = st.warichu_chars_before as usize;
    let after = st.warichu_chars_after as usize;
    let n = line_count(members.len(), st.warichu_lines as usize, before, after);
    let scale = (st.warichu_size / 100.0).clamp(0.05, 4.0);
    let old_x0 = members.iter().map(|&k| line[k].x).fold(f64::INFINITY, f64::min);
    let old_right = members.iter().map(|&k| line[k].x + line[k].adv).fold(f64::NEG_INFINITY, f64::max);
    let old_span = old_right - old_x0;
    if !old_span.is_finite() || old_span <= 0.0 {
        return 0.0;
    }
    let saved: Vec<(f64, f64, f64, f64)> = members
        .iter()
        .map(|&k| {
            let g = &line[k];
            (g.y, g.adv, g.sx, g.sy)
        })
        .collect();
    let old_xs: Vec<f64> = members.iter().map(|&k| line[k].x).collect();
    for &k in members {
        line[k].sx *= scale;
        line[k].sy *= scale;
        line[k].adv *= scale;
        line[k].y *= scale;
    }
    let scaled: Vec<f64> = members.iter().map(|&k| line[k].adv).collect();
    if scaled.iter().any(|a| !a.is_finite()) {
        restore(line, members, &saved);
        return 0.0;
    }
    let counts = if n <= 1 { vec![members.len()] } else { split_even(&scaled, n, before.max(1), after.max(1)) };
    let mut widths = Vec::with_capacity(counts.len());
    let mut off = 0_usize;
    for &c in &counts {
        if off + c > members.len() {
            restore(line, members, &saved);
            return 0.0;
        }
        let w: f64 = members[off..off + c].iter().map(|&k| line[k].adv).sum();
        widths.push(w);
        off += c;
    }
    let block = widths.iter().copied().fold(0.0_f64, f64::max);
    if !block.is_finite() || off != members.len() {
        restore(line, members, &saved);
        return 0.0;
    }
    let rows = counts.len().max(1);
    let small = st.size * scale;
    // 0 is the default (one small em). Any other value, including a negative one, is added to
    // that em and then clamped so the rows cannot swap order.
    // Bounded, so a hostile value can't move a row to an infinite or huge coordinate.
    let spacing = if st.warichu_line_spacing.is_finite() { st.warichu_line_spacing.clamp(-16.0 * st.size, 16.0 * st.size) } else { 0.0 };
    let gap = if spacing == 0.0 { small } else { (small + spacing).max(0.0) };
    let mut off = 0_usize;
    for (li, &c) in counts.iter().enumerate() {
        let w = widths[li];
        let (start_x, extra) = match st.warichu_align {
            WarichuAlignment::Left => (0.0, 0.0),
            WarichuAlignment::Right => ((block - w).max(0.0), 0.0),
            WarichuAlignment::Justify if c > 1 && block > w => (0.0, (block - w) / (c - 1) as f64),
            WarichuAlignment::Auto | WarichuAlignment::Center | WarichuAlignment::Justify => ((block - w) / 2.0, 0.0),
        };
        let y_off = (li as f64 - (rows as f64 - 1.0) / 2.0) * gap;
        let mut x = old_x0 + start_x;
        for &k in &members[off..off + c] {
            line[k].x = x;
            line[k].y += y_off;
            x += line[k].adv + extra;
        }
        off += c;
    }
    let delta = block - old_span;
    let mut placed: Vec<(f64, f64)> = old_xs.iter().enumerate().map(|(i, ox)| (*ox, line[members[i]].x)).collect();
    placed.sort_by(|a, b| a.0.total_cmp(&b.0));
    let mut member_at = vec![false; line.len()];
    for &k in members {
        if let Some(slot) = member_at.get_mut(k) {
            *slot = true;
        }
    }
    for (k, g) in line.iter_mut().enumerate() {
        if member_at.get(k).copied().unwrap_or(false) {
            continue;
        }
        if g.x >= old_right - 1e-6 {
            g.x += delta;
        } else if g.x >= old_x0 - 1e-6
            && let Some(anchor) = placed.iter().rev().find(|p| p.0 <= g.x + 1e-6)
        {
            // A mark drawn with the run (a tab leader is not: it sits before old_x0) keeps its
            // offset from the glyph it was attached to, in the scaled line.
            g.x = anchor.1 + (g.x - anchor.0) * scale;
        }
    }
    delta
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn huge_break_minimums_keep_one_line() {
        assert_eq!(line_count(10, 2, usize::MAX, 1), 1);
        assert_eq!(line_count(10, 3, 1, usize::MAX), 1);
        assert_eq!(line_count(10, 2, 3, 3), 2);
    }
}
