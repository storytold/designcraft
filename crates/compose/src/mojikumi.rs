//! Mojikumi boundary widths. The same values feed line selection and placement.
use crate::shape::Glyph;
use designcraft_doc::{
    Styles,
    mojikumi::{self as rules, Aki, Rules},
};

#[derive(Clone, Copy, Debug, Default)]
pub struct Metrics {
    pub active: bool,
    /// Shaped body width before tracking and manual character spacing.
    pub body: f64,
    pub blank_before: f64,
    pub blank_after: f64,
    /// Desired internal gap attached to this glyph's advance.
    pub gap: f64,
    pub stretch: f64,
    pub shrink: f64,
    pub priority: u8,
    pub discrete: bool,
    /// Width changes when this glyph starts / ends a line.
    pub start: f64,
    pub end: f64,
    pub end_aki: Aki,
    pub start_aki: Aki,
    pub at_start: bool,
    pub explicit_before: bool,
    pub explicit_after: bool,
}

fn eligible(g: &Glyph) -> bool {
    g.adv > 0.0
        && !g.locked_advance
        && !g.ch.is_control()
        && (!g.ch.is_whitespace() || g.ch == '\u{3000}')
        && !matches!(g.ch, '\u{e000}'..='\u{e1ff}')
}
fn em(g: &Glyph) -> f64 {
    // Actual scaled em, including vertical and composite-font scaling.
    g.face.units_per_em() * if g.upright { g.sy } else { g.sx }
}

pub fn apply(glyphs: &mut [Glyph], styles: &Styles, name: &str) {
    let Ok(Some(table)) = Rules::resolve(styles, name) else { return };
    // Boundaries belong to shaped clusters, not individual glyphs. In particular
    // the advance carrying a following gap must come after all attached marks.
    let mut clusters = Vec::new();
    let mut start = 0;
    while start < glyphs.len() {
        let byte = glyphs[start].byte;
        let mut end = start + 1;
        while end < glyphs.len() && glyphs[end].byte == byte {
            end += 1;
        }
        clusters.push((start, end));
        start = end;
    }
    for &(start, end) in &clusters {
        let cluster = &mut glyphs[start..end];
        if !cluster.iter().any(eligible) {
            continue;
        }
        let class = rules::class(cluster[0].rendered_char);
        let unit = em(&cluster[0]);
        let body: f64 = cluster.iter().map(|g| g.moji.body).sum();
        for g in cluster.iter_mut() {
            g.moji.active = true;
        }
        // Tracking must not make a proportional or half-width glyph appear to
        // contain a removable half-em body. Shaping features already affect body.
        if body >= unit * 0.8 {
            let before = if cluster[0].moji.explicit_before { 0.0 } else { rules::leading(class) * unit };
            let after = if cluster[0].moji.explicit_after { 0.0 } else { rules::trailing(class) * unit };
            let trim = before + after;
            if let Some(owner) = cluster.iter().position(|g| g.adv >= trim) {
                cluster[0].moji.blank_before = before / unit.max(1e-9);
                cluster[end - start - 1].moji.blank_after = after / unit.max(1e-9);
                cluster[owner].adv -= trim;
                for (i, g) in cluster.iter_mut().enumerate() {
                    g.dx += if i > owner { trim - before } else { -before };
                }
            }
        }
        let head = &mut cluster[0];
        head.moji.start_aki = table.pair_with_bodies(if start == 0 { 23 } else { 22 }, class, 0.0, head.moji.blank_before);
        head.moji.start = if head.moji.explicit_before { 0.0 } else { head.moji.start_aki.desired * unit };
    }
    for (index, &(start, end)) in clusters.iter().enumerate() {
        if !glyphs[start].moji.active {
            continue;
        }
        let class = rules::class(glyphs[start].rendered_char);
        let unit = em(&glyphs[start]);
        let mut gap = Aki::default();
        if let Some(&(next, _)) = clusters.get(index + 1)
            && glyphs[next].moji.active
            && !glyphs[end - 1].moji.explicit_after
            && !glyphs[next].moji.explicit_before
        {
            gap = table.pair_with_bodies(
                class,
                rules::class(glyphs[next].rendered_char),
                glyphs[end - 1].moji.blank_after,
                glyphs[next].moji.blank_before,
            );
        }
        let width: f64 = glyphs[start..end].iter().map(|g| g.adv).sum();
        let g = &mut glyphs[end - 1];
        g.moji.end_aki = table.pair_with_bodies(class, 22, g.moji.blank_after, 0.0);
        let desired = (gap.desired * unit).max(-width);
        g.moji.gap = desired;
        g.moji.stretch = (gap.max * unit - desired).max(0.0);
        g.moji.shrink = (desired - gap.min * unit).max(0.0).min(width + desired);
        g.moji.priority = gap.priority;
        g.moji.discrete = gap.discrete;
        g.moji.end = if g.moji.explicit_after { 0.0 } else { g.moji.end_aki.desired * unit } - desired;
        g.adv += desired;
    }
}

pub fn edges(line: &mut [Glyph]) {
    if let Some(g) = line.first_mut() {
        g.moji.at_start = true;
        g.adv += g.moji.start;
        g.dx += g.moji.start;
    }
    if let Some(g) = line.iter_mut().rev().find(|g| !crate::breaker::is_forced(g.ch)) {
        g.adv += g.moji.end;
        // A line-end gap is not the internal gap that happened to follow it.
        let unit = em(g);
        let aki = g.moji.end_aki;
        g.moji.stretch = if g.moji.explicit_after { 0.0 } else { (aki.max - aki.desired).max(0.0) * unit };
        g.moji.shrink = if g.moji.explicit_after { 0.0 } else { (aki.desired - aki.min).max(0.0) * unit };
        g.moji.priority = aki.priority;
        g.moji.discrete = aki.discrete;
    }
}

/// Consume spacing in priority order; discrete rules use an entire endpoint.
/// Returns the residual for ordinary word/letter/glyph justification.
pub fn distribute(line: &mut [Glyph], extra: f64, add: &mut [f64]) -> f64 {
    let mut rem = extra.abs();
    let sign = extra.signum();
    let properties = |g: &Glyph, before: bool| {
        if before {
            let a = g.moji.start_aki;
            let cap = if !g.moji.at_start || g.moji.explicit_before {
                0.0
            } else if sign < 0.0 {
                (a.desired - a.min).max(0.0) * em(g)
            } else {
                (a.max - a.desired).max(0.0) * em(g)
            };
            (a.priority, a.discrete, cap)
        } else {
            (g.moji.priority, g.moji.discrete, if sign < 0.0 { g.moji.shrink } else { g.moji.stretch })
        }
    };
    for priority in 0..=10 {
        if rem <= 1e-9 {
            break;
        }
        // Leading and following continuous gaps participate in one pool.
        // Equal priorities must not depend on whether a gap precedes a glyph.
        let total: f64 = line
            .iter()
            .flat_map(|g| [true, false].map(|before| properties(g, before)))
            .filter(|&(p, discrete, _)| p == priority && !discrete)
            .map(|(_, _, cap)| cap)
            .sum();
        if total > 0.0 {
            let take = rem.min(total);
            for (g, a) in line.iter_mut().zip(add.iter_mut()) {
                for before in [true, false] {
                    let (p, discrete, cap) = properties(g, before);
                    if p == priority && !discrete {
                        let amount = sign * take * cap / total;
                        *a += amount;
                        if before {
                            g.dx += amount;
                        }
                    }
                }
            }
            rem -= take;
        }
        // Never interpolate a non-floating rule. Compression may have to use
        // its entire endpoint even when that leaves a little unused line width;
        // refusing it would contradict the capacity accepted by the breaker.
        for (g, a) in line.iter_mut().zip(add.iter_mut()) {
            for before in [true, false] {
                let (p, discrete, cap) = properties(g, before);
                if rem > 1e-9 && p == priority && discrete && cap > 0.0 && (sign < 0.0 || cap <= rem + 1e-9) {
                    *a += sign * cap;
                    if before {
                        g.dx += sign * cap;
                    }
                    rem -= cap;
                }
            }
        }
    }
    sign * rem
}

/// Replace the final internal gap's elasticity by its line-end elasticity.
pub fn end_elastic(g: &Glyph, justify: bool, shrink: bool) -> [f64; 2] {
    let unit = em(g);
    let a = g.moji.end_aki;
    [
        if justify { (if g.moji.explicit_after { 0.0 } else { (a.max - a.desired).max(0.0) * unit }) - g.moji.stretch } else { 0.0 },
        if shrink { (if g.moji.explicit_after { 0.0 } else { (a.desired - a.min).max(0.0) * unit }) - g.moji.shrink } else { 0.0 },
    ]
}

pub fn start_elastic(g: &Glyph, justify: bool, shrink: bool) -> [f64; 2] {
    let a = g.moji.start_aki;
    if g.moji.explicit_before {
        return [0.0; 2];
    }
    [if justify { (a.max - a.desired).max(0.0) * em(g) } else { 0.0 }, if shrink { (a.desired - a.min).max(0.0) * em(g) } else { 0.0 }]
}
