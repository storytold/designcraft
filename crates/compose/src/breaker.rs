//! Line breaking: the paragraph composer (Knuth–Plass total fit), the single-line composer
//! (greedy first fit) and balanced ragged lines. All work on a paragraph's glyphs and a per-line
//! measure function.
//!
//! Justification uses *prioritised* elastic material (plan: typography §5.2): word spaces first,
//! then letter spacing (every glyph gets elastic width from the paragraph's letter-spacing range),
//! then glyph scaling. The adjustment ratio charges letter and glyph adjustments extra
//! ([`TIER_COST`]), so the composer prefers word-space changes, then hyphenation, then letter
//! spacing, then glyph scaling.

use crate::shape::{Glyph, SOFT_HYPHEN};

/// One line chosen by a breaker: glyphs `start..end` are visible; `next` is where the next line starts.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Break {
    pub start: usize,
    pub end: usize,
    pub next: usize,
    /// Broken at a hyphenation point (a hyphen glyph must be added).
    pub hyphen: bool,
    /// Ended by a forced break (paragraph end, forced line break, column/frame/page break).
    pub forced: bool,
}

/// Spacing parameters (from the paragraph's Justification and Hyphenation settings), as fractions.
#[derive(Clone, Copy, Debug)]
pub struct Spacing {
    pub justify: bool,
    pub word_min: f64,
    pub word_desired: f64,
    pub word_max: f64,
    /// Letter spacing, as fractions of the space width added between glyphs.
    pub letter_min: f64,
    pub letter_desired: f64,
    pub letter_max: f64,
    /// Horizontal glyph scaling (1 = 100%).
    pub glyph_min: f64,
    pub glyph_desired: f64,
    pub glyph_max: f64,
    /// Penalty for a hyphenated break (0 = free, larger = fewer hyphens).
    pub hyphen_penalty: f64,
    /// Maximum consecutive hyphenated lines (0 = unlimited) — a hard constraint.
    pub hyphen_limit: u32,
    /// Ragged-right stretchability (points) for unjustified paragraphs.
    pub ragged_stretch: f64,
    /// Hyphenation zone (points, unjustified text only): a word is not hyphenated when breaking
    /// before it would leave less than this much space at the end of the line.
    pub hyph_zone: f64,
    /// Optical margin alignment: punctuation hangs outside the measure (counted as extra width).
    pub optical: bool,
}

impl Default for Spacing {
    fn default() -> Self {
        Spacing {
            justify: false,
            word_min: 0.8,
            word_desired: 1.0,
            word_max: 1.33,
            letter_min: 0.0,
            letter_desired: 0.0,
            letter_max: 0.0,
            glyph_min: 1.0,
            glyph_desired: 1.0,
            glyph_max: 1.0,
            hyphen_penalty: 275.0,
            hyphen_limit: 3,
            ragged_stretch: 24.0,
            hyph_zone: 0.0,
            optical: false,
        }
    }
}

impl Spacing {
    /// Per-tier (word, letter, glyph) stretch and shrink of glyph `g` as a box.
    fn box_elastic(&self, g: &Glyph) -> ([f64; 3], [f64; 3]) {
        if !self.justify || g.locked_advance {
            return ([0.0; 3], [0.0; 3]);
        }
        let natural = g.adv / self.glyph_desired.max(0.01);
        (
            [0.0, g.space * (self.letter_max - self.letter_desired).max(0.0), natural * (self.glyph_max - self.glyph_desired).max(0.0)],
            [0.0, g.space * (self.letter_desired - self.letter_min).max(0.0), natural * (self.glyph_desired - self.glyph_min).max(0.0)],
        )
    }
    /// Word-space stretch and shrink of space glyph `g` (only U+0020 is elastic).
    fn space_elastic(&self, g: &Glyph) -> (f64, f64) {
        if g.ch != ' ' && !(g.ch == '\u{3000}' && g.ideographic_space_elastic) {
            return (0.0, 0.0);
        }
        (g.space * (self.word_max - self.word_desired).max(0.0), g.space * (self.word_desired - self.word_min).max(0.0))
    }
}

/// Relative cost of using each tier of elastic material (word, letter, glyph).
pub const TIER_COST: [f64; 3] = [1.0, 0.75, 1.0];

/// Effective adjustment ratio for `d` points of stretch (`d > 0`) or shrink (`d < 0`), consuming
/// the tiers in priority order. Returns `(r, feasible)`; shrinking beyond all tiers is infeasible.
pub fn tiered_ratio(d: f64, y: [f64; 3], z: [f64; 3]) -> (f64, bool) {
    const EPS: f64 = 1e-9;
    if d.abs() < EPS {
        return (0.0, true);
    }
    let (mut rem, cap) = if d > 0.0 { (d, y) } else { (-d, z) };
    let mut r = 0.0;
    for t in 0..3 {
        if cap[t] > EPS && rem > EPS {
            let take = rem.min(cap[t]);
            r += TIER_COST[t] * take / cap[t];
            rem -= take;
        }
    }
    if d < 0.0 {
        return (-r, rem <= 1e-6);
    }
    if rem > 1e-6 {
        // Beyond every maximum: word spaces grow without bound (an H&J violation).
        let unit = if cap[0] > EPS { cap[0] } else { cap.iter().sum::<f64>() };
        r = if unit > EPS { r + rem / unit } else { INF };
    }
    (r, true)
}

#[derive(Clone, Copy, Debug, PartialEq)]
enum Kind {
    Box,
    Glue,
    Penalty,
}

#[derive(Clone, Copy, Debug)]
struct Item {
    kind: Kind,
    w: f64,
    st: [f64; 3],
    sh: [f64; 3],
    /// Penalty value (penalties only).
    p: f64,
    flagged: bool,
    /// An automatic/discretionary hyphenation point (subject to the hyphenation zone).
    auto: bool,
    /// Breaks: optical hang of the glyph ending the line. Boxes: hang of the glyph starting one.
    hang: f64,
    /// Auto hyphenation points: index of the break opportunity before the word (or `NONE`).
    zone_from: usize,
}

const NONE: usize = usize::MAX;

impl Item {
    fn boxed(w: f64, st: [f64; 3], sh: [f64; 3]) -> Item {
        Item { kind: Kind::Box, w, st, sh, p: 0.0, flagged: false, auto: false, hang: 0.0, zone_from: NONE }
    }
    fn glue(w: f64, st: f64, sh: f64) -> Item {
        Item { kind: Kind::Glue, w, st: [st, 0.0, 0.0], sh: [sh, 0.0, 0.0], p: 0.0, flagged: false, auto: false, hang: 0.0, zone_from: NONE }
    }
    fn penalty(w: f64, p: f64, flagged: bool) -> Item {
        Item { kind: Kind::Penalty, w, st: [0.0; 3], sh: [0.0; 3], p, flagged, auto: false, hang: 0.0, zone_from: NONE }
    }
}

const INF: f64 = 10000.0;

/// Does the glyph force a line end after it?
/// A line may break between `a` and `b` in CJK text (any break next to an ideograph, kana or
/// hangul), except where kinsoku forbids it: no line starts with closing punctuation, small
/// kana or the prolonged sound mark, and none ends with opening brackets.
pub fn cjk_break_between(a: char, b: char) -> bool {
    let cjk = |c: char| crate::upright_in_vertical(c) || matches!(c as u32, 0x3000..=0x303F);
    if !(cjk(a) || cjk(b)) {
        return false;
    }
    const NO_START: &str = "、。，．・：；？！ー）」』】〕〉》｝］〙〗ぁぃぅぇぉっゃゅょゎァィゥェォッャュョヮヵヶ々〻ゝゞヽヾ!),.:;?]}";
    const NO_END: &str = "（「『【〔〈《｛［〘〖([{";
    !NO_START.contains(b) && !NO_END.contains(a)
}

pub fn is_forced(c: char) -> bool {
    use designcraft_doc::story::*;
    matches!(c, FORCED_LINE_BREAK | COLUMN_BREAK | FRAME_BREAK | PAGE_BREAK)
}

/// Optical margin protrusion of `c` as fractions of its advance: (left edge, right edge).
pub fn hang(c: char) -> (f64, f64) {
    match c {
        '.' | ',' => (0.0, 1.0),
        '-' | '\u{2010}' | '\u{2011}' | '\u{AD}' => (0.75, 0.75),
        '\'' | '"' | '‘' | '’' | '“' | '”' | '‚' | '„' | '‹' | '›' | '«' | '»' => (1.0, 1.0),
        '\u{2013}' | '\u{2014}' => (0.5, 0.5),
        ':' | ';' => (0.0, 0.5),
        '!' | '?' => (0.0, 0.2),
        '*' => (0.3, 0.3),
        '(' | '[' | '{' => (0.1, 0.0),
        ')' | ']' | '}' => (0.0, 0.1),
        'A' | 'V' | 'W' | 'Y' | 'v' | 'w' | 'y' => (0.05, 0.05),
        'T' => (0.05, 0.05),
        'O' | 'C' | 'G' | 'Q' | 'o' | 'c' => (0.03, 0.03),
        _ => (0.0, 0.0),
    }
}

fn hang_left(g: &Glyph) -> f64 {
    hang(g.ch).0 * g.adv
}

fn right_hang(g: &Glyph, sp: &Spacing) -> f64 {
    g.cjk_hang.max(if sp.optical { hang_right(g) } else { 0.0 })
}

fn hang_right(g: &Glyph) -> f64 {
    hang(g.ch).1 * g.adv
}

/// Advance of the hyphen glyph that would follow `g` (cached per face).
fn hyphen_width(g: &Glyph, cache: &mut Vec<(u32, f64)>) -> f64 {
    let id = g.face.id();
    let units = match cache.iter().find(|c| c.0 == id) {
        Some(c) => c.1,
        None => {
            let gid = designcraft_fonts::first_glyph(&g.face, &['-', '\u{2010}']);
            let u = g.face.advance(gid);
            cache.push((id, u));
            u
        }
    };
    units * g.sx
}

/// Push a break inside a word. Ragged text gets the line-end stretch around it
/// (`penalty(∞) glue(0, s) penalty glue(0, −s)`), so hyphenated ragged lines are measured like
/// lines broken at spaces.
fn push_inword(it: &mut Vec<Item>, ig: &mut Vec<usize>, p: Item, g: usize, sp: &Spacing) {
    if sp.justify {
        it.push(p);
        ig.push(g);
        return;
    }
    it.push(Item::penalty(0.0, INF, false));
    it.push(Item::glue(0.0, sp.ragged_stretch, 0.0));
    it.push(p);
    it.push(Item::glue(0.0, -sp.ragged_stretch, 0.0));
    ig.extend([g; 4]);
}

/// Build Knuth items. `item_glyph[k]` = glyph index of item k (penalties: the glyph *before* which the break happens).
fn items(glyphs: &[Glyph], hyph_after: &[bool], sp: &Spacing) -> (Vec<Item>, Vec<usize>) {
    let mut it: Vec<Item> = Vec::with_capacity(glyphs.len() * 2 + 2);
    let mut ig = Vec::with_capacity(glyphs.len() * 2 + 2);
    let n = glyphs.len();
    let mut hy_cache = Vec::new();
    let mut last_space = NONE;
    for (i, g) in glyphs.iter().enumerate() {
        let prev_hang = if i > 0 { right_hang(&glyphs[i - 1], sp) } else { 0.0 };
        if is_forced(g.ch) {
            it.push(Item::glue(0.0, INF, 0.0));
            ig.push(i);
            let mut p = Item::penalty(0.0, -INF, false);
            p.hang = prev_hang;
            it.push(p);
            ig.push(i + 1);
            last_space = NONE;
            continue;
        }
        if g.is_space() && !g.no_break && g.break_after != Some(false) {
            let w = g.adv;
            if sp.justify {
                let (st, sh) = sp.space_elastic(g);
                let mut gl = Item::glue(w, st, sh);
                gl.hang = prev_hang;
                last_space = it.len();
                it.push(gl);
                ig.push(i);
            } else {
                // Ragged right (Knuth): glue(0, s, 0) penalty(0) glue(w, -s, 0).
                it.push(Item::glue(0.0, sp.ragged_stretch, 0.0));
                ig.push(i);
                let mut p = Item::penalty(0.0, 0.0, false);
                p.hang = prev_hang;
                last_space = it.len();
                it.push(p);
                ig.push(i);
                it.push(Item::glue(w, -sp.ragged_stretch, 0.0));
                ig.push(i);
            }
            continue;
        }
        if g.ch == SOFT_HYPHEN {
            let hw = glyphs[..i].iter().rev().find(|g| g.ch != SOFT_HYPHEN).map_or(g.size * 0.33, |p| hyphen_width(p, &mut hy_cache));
            let mut p = Item::penalty(hw, sp.hyphen_penalty, true);
            p.auto = true;
            p.zone_from = last_space;
            if sp.optical {
                p.hang = hw * hang('-').1;
            }
            push_inword(&mut it, &mut ig, p, i + 1, sp);
            continue;
        }
        let (st, sh) = sp.box_elastic(g);
        let mut bx = Item::boxed(g.adv, st, sh);
        if sp.optical {
            bx.hang = hang_left(g);
        }
        it.push(bx);
        ig.push(i);
        let next_is_space = glyphs.get(i + 1).is_none_or(|n| n.is_space());
        if i + 1 < n && !next_is_space && !g.no_break && g.break_after != Some(false) {
            if matches!(g.ch, '-' | '\u{2010}') {
                let mut p = Item::penalty(0.0, 50.0, true);
                p.hang = right_hang(g, sp);
                push_inword(&mut it, &mut ig, p, i + 1, sp);
            } else if matches!(g.ch, '\u{2013}' | '\u{2014}' | '/') || g.break_after.unwrap_or_else(|| cjk_break_between(g.ch, glyphs[i + 1].ch)) {
                let mut p = Item::penalty(0.0, 0.0, false);
                p.hang = right_hang(g, sp);
                push_inword(&mut it, &mut ig, p, i + 1, sp);
            } else if hyph_after[i] {
                let hw = hyphen_width(g, &mut hy_cache);
                let mut p = Item::penalty(hw, sp.hyphen_penalty, true);
                p.auto = true;
                p.zone_from = last_space;
                if sp.optical {
                    p.hang = hw * hang('-').1;
                }
                push_inword(&mut it, &mut ig, p, i + 1, sp);
            }
        }
    }
    // Paragraph end: fill glue + forced break.
    it.push(Item::glue(0.0, INF, 0.0));
    ig.push(n);
    let mut p = Item::penalty(0.0, -INF, false);
    p.hang = if n > 0 { right_hang(&glyphs[n - 1], sp) } else { 0.0 };
    it.push(p);
    ig.push(n);
    (it, ig)
}

#[derive(Clone, Copy, Debug)]
struct Node {
    pos: usize,
    line: usize,
    fitness: u8,
    tw: f64,
    ty: [f64; 3],
    tz: [f64; 3],
    demerits: f64,
    prev: Option<usize>,
    hyphens: u32,
    flagged: bool,
}

/// Knuth–Plass total-fit line breaking. `width(line)` gives each line's measure.
pub fn knuth_plass(glyphs: &[Glyph], hyph_after: &[bool], sp: &Spacing, width: &dyn Fn(usize) -> f64) -> Vec<Break> {
    if glyphs.is_empty() {
        return vec![Break { start: 0, end: 0, next: 0, hyphen: false, forced: true }];
    }
    let (items, ig) = items(glyphs, hyph_after, sp);
    for tolerance in [2.5, 12.0, f64::INFINITY] {
        if let Some(b) = kp_pass(&items, &ig, glyphs, sp, width, tolerance, tolerance.is_infinite()) {
            return b;
        }
    }
    greedy(glyphs, hyph_after, sp, width)
}

/// Upper bound on the hyphen-count states tracked per breakpoint.
const MAX_HYPHEN_STATES: usize = 8;

#[allow(clippy::too_many_arguments)]
fn kp_pass(
    items: &[Item],
    ig: &[usize],
    glyphs: &[Glyph],
    sp: &Spacing,
    width: &dyn Fn(usize) -> f64,
    tol: f64,
    emergency: bool,
) -> Option<Vec<Break>> {
    let m = items.len();
    // Prefix sums (before item i).
    let mut sw = vec![0.0; m + 1];
    let mut sy = vec![[0.0; 3]; m + 1];
    let mut sz = vec![[0.0; 3]; m + 1];
    for (i, it) in items.iter().enumerate() {
        let (w, y, z) = match it.kind {
            Kind::Box | Kind::Glue => (it.w, it.st, it.sh),
            Kind::Penalty => (0.0, [0.0; 3], [0.0; 3]),
        };
        sw[i + 1] = sw[i] + w;
        for t in 0..3 {
            sy[i + 1][t] = sy[i][t] + y[t];
            sz[i + 1][t] = sz[i][t] + z[t];
        }
    }
    let first_hang = items.iter().find(|i| i.kind == Kind::Box).map_or(0.0, |b| b.hang);
    let mut arena: Vec<Node> =
        vec![Node { pos: 0, line: 0, fitness: 1, tw: first_hang, ty: [0.0; 3], tz: [0.0; 3], demerits: 0.0, prev: None, hyphens: 0, flagged: false }];
    let limit = sp.hyphen_limit as usize;
    let states = if limit == 0 { 2 } else { (limit + 1).min(MAX_HYPHEN_STATES) };
    let mut active: Vec<usize> = vec![0];
    let mut best: Vec<Option<(f64, usize)>> = vec![None; 4 * states];
    let mut keep = Vec::new();
    for b in 0..m {
        let it = items[b];
        let (is_break, pw, pp, flagged) = match it.kind {
            Kind::Penalty if it.p < INF => (true, it.w, it.p, it.flagged),
            Kind::Glue if b > 0 && items[b - 1].kind == Kind::Box => (true, 0.0, 0.0, false),
            _ => (false, 0.0, 0.0, false),
        };
        if !is_break {
            continue;
        }
        let forced = pp <= -INF;
        best.iter_mut().for_each(|x| *x = None);
        keep.clear();
        let mut last_removed: Option<usize> = None;
        for &a in &active {
            let n = arena[a];
            let l = sw[b] - n.tw + pw - it.hang;
            let target = width(n.line).max(1.0);
            let mut y = [0.0; 3];
            let mut z = [0.0; 3];
            for t in 0..3 {
                y[t] = sy[b][t] - n.ty[t];
                z[t] = sz[b][t] - n.tz[t];
            }
            let (r, feasible) = tiered_ratio(target - l, y, z);
            if feasible && !forced {
                keep.push(a);
            } else {
                last_removed = Some(a);
            }
            if !feasible || (r > tol && !forced) {
                continue;
            }
            if flagged {
                // Hyphen limit: a hard constraint.
                if limit > 0 && n.hyphens as usize >= limit {
                    continue;
                }
                // Hyphenation zone (unjustified): break before the word instead when that leaves
                // less than the zone at the line end.
                if it.auto && !sp.justify && sp.hyph_zone > 0.0 && it.zone_from != NONE && it.zone_from > n.pos {
                    let before = sw[it.zone_from] - n.tw - items[it.zone_from].hang;
                    if target - before < sp.hyph_zone {
                        continue;
                    }
                }
            }
            let bad = 100.0 * r.abs().powi(3);
            let lp = 10.0 + bad;
            let mut d = if pp >= 0.0 {
                lp * lp + pp * pp
            } else if pp > -INF {
                lp * lp - pp * pp
            } else {
                lp * lp
            };
            if flagged && n.flagged {
                d += 3000.0;
            }
            let fit = if r < -0.5 {
                0
            } else if r <= 0.5 {
                1
            } else if r <= 1.0 {
                2
            } else {
                3
            };
            if (fit as i32 - n.fitness as i32).abs() > 1 {
                d += 3000.0;
            }
            let hs = if flagged { (n.hyphens as usize + 1).min(states - 1) } else { 0 };
            let total = n.demerits + d;
            let slot = &mut best[fit * states + hs];
            if slot.is_none_or(|(bd, _)| total < bd) {
                *slot = Some((total, a));
            }
        }
        // Emergency: nothing can reach this break and nothing remains active → accept an overfull line.
        if emergency
            && keep.is_empty()
            && best.iter().all(Option::is_none)
            && let Some(a) = last_removed
        {
            best[0] = Some((arena[a].demerits + 1e8, a));
        }
        std::mem::swap(&mut active, &mut keep);
        if best.iter().all(Option::is_none) {
            if active.is_empty() {
                return None;
            }
            continue;
        }
        // Totals after the break: skip glue and penalties up to the next box.
        let (mut tw, mut ty, mut tz) = (sw[b], sy[b], sz[b]);
        let mut i = b;
        while i < m {
            let x = items[i];
            match x.kind {
                Kind::Box => {
                    tw += x.hang;
                    break;
                }
                Kind::Glue => {
                    tw += x.w;
                    for t in 0..3 {
                        ty[t] += x.st[t];
                        tz[t] += x.sh[t];
                    }
                }
                Kind::Penalty if x.p <= -INF && i > b => break,
                Kind::Penalty => {}
            }
            i += 1;
        }
        // Knuth's pruning: a class is only worth a node if it is within the fitness demerit of the best.
        let min_d = best.iter().flatten().map(|x| x.0).fold(f64::INFINITY, f64::min);
        for (k, c) in best.iter().enumerate() {
            if let Some((d, a)) = *c
                && d <= min_d + 3000.0
            {
                let prev = arena[a];
                arena.push(Node {
                    pos: b,
                    line: prev.line + 1,
                    fitness: (k / states) as u8,
                    tw,
                    ty,
                    tz,
                    demerits: d,
                    prev: Some(a),
                    hyphens: if flagged { prev.hyphens + 1 } else { 0 },
                    flagged,
                });
                active.push(arena.len() - 1);
            }
        }
    }
    // The final forced break: best node at position m-1.
    let end = active.iter().copied().filter(|&a| arena[a].pos == m - 1).min_by(|&a, &b| arena[a].demerits.total_cmp(&arena[b].demerits))?;
    let mut chain = Vec::new();
    let mut cur = Some(end);
    while let Some(c) = cur {
        if arena[c].prev.is_some() {
            chain.push(arena[c].pos);
        }
        cur = arena[c].prev;
    }
    chain.reverse();
    Some(breaks_from_positions(items, ig, glyphs, &chain))
}

fn breaks_from_positions(items: &[Item], ig: &[usize], glyphs: &[Glyph], chain: &[usize]) -> Vec<Break> {
    let n = glyphs.len();
    let mut out = Vec::with_capacity(chain.len());
    let mut start = 0;
    for &b in chain {
        let it = items[b];
        let (end, hyphen, forced) = match it.kind {
            Kind::Penalty => {
                let e = ig[b].min(n);
                // A flagged penalty with width = an inserted hyphen (not an explicit '-').
                let explicit = e > 0 && matches!(glyphs[e - 1].ch, '-' | '\u{2010}');
                (e, it.flagged && it.w > 0.0 && !explicit || (it.flagged && e > 0 && glyphs[e - 1].ch == SOFT_HYPHEN), it.p <= -INF)
            }
            Kind::Glue | Kind::Box => (ig[b], false, false),
        };
        // Trim trailing spaces from the visible range; skip leading spaces of the next line.
        let mut vis_end = end;
        while vis_end > start && glyphs[vis_end - 1].is_space() {
            vis_end -= 1;
        }
        let mut next = end;
        while next < n && glyphs[next].is_space() {
            next += 1;
        }
        let forced_glyph = end > 0 && end <= n && is_forced(glyphs[end - 1].ch);
        out.push(Break { start, end: vis_end, next, hyphen, forced: forced || forced_glyph });
        start = next;
    }
    if out.is_empty() {
        out.push(Break { start: 0, end: n, next: n, hyphen: false, forced: true });
    }
    // Lines produced past the paragraph end (e.g. a trailing forced break) are dropped.
    let trailing_forced = glyphs.last().is_some_and(|g| is_forced(g.ch));
    out.retain(|b| b.start < n || (b.start == n && trailing_forced));
    out
}

/// Greedy first-fit breaking (single-line composer; also used for paragraphs with tabs).
pub fn greedy(glyphs: &[Glyph], hyph_after: &[bool], sp: &Spacing, width: &dyn Fn(usize) -> f64) -> Vec<Break> {
    let n = glyphs.len();
    let mut out = Vec::new();
    let mut start = 0;
    let mut line = 0;
    let mut hyphens = 0u32;
    let mut hy_cache = Vec::new();
    if n == 0 {
        return vec![Break { start: 0, end: 0, next: 0, hyphen: false, forced: true }];
    }
    let may_hyphenate = |hyphens: u32| sp.hyphen_limit == 0 || hyphens < sp.hyphen_limit;
    while start < n {
        let w = width(line).max(1.0);
        let mut x = if sp.optical { -hang_left(&glyphs[start]) } else { 0.0 };
        let mut shrink = 0.0;
        let mut last_ok: Option<(usize, bool)> = None; // (break position = end glyph, hyphen)
        let mut last_space: Option<(usize, f64)> = None; // (break position, line width there)
        let mut i = start;
        let mut brk: Option<(usize, bool, bool)> = None;
        while i < n {
            let g = &glyphs[i];
            if is_forced(g.ch) {
                brk = Some((i + 1, false, true));
                break;
            }
            if g.is_space() && !g.no_break && g.break_after != Some(false) {
                if sp.justify {
                    shrink += sp.space_elastic(g).1;
                }
                last_ok = Some((i, false));
                last_space = Some((i, x));
                x += g.adv;
                i += 1;
                continue;
            }
            let hang_r = right_hang(g, sp);
            if x + g.adv - hang_r > w + shrink
                && i > start
                && (last_ok.is_some() || glyphs.get(i - 1).is_none_or(|p| p.break_after != Some(false) && !p.no_break))
            {
                // Overflow: break at the last opportunity, else before this glyph.
                brk = Some(match last_ok {
                    // Hyphenation zone: break before the word if that leaves little space.
                    Some((_, true)) if !sp.justify && sp.hyph_zone > 0.0 && last_space.is_some_and(|(_, xs)| w - xs < sp.hyph_zone) => {
                        (last_space.map_or(i, |s| s.0), false, false)
                    }
                    Some((p, h)) => (p, h, false),
                    None => (i, false, false),
                });
                break;
            }
            x += g.adv;
            if sp.justify {
                shrink += sp.box_elastic(g).1.iter().sum::<f64>();
            }
            if i + 1 < n && !glyphs[i + 1].is_space() && !g.no_break && g.break_after != Some(false) {
                if matches!(g.ch, '-' | '\u{2010}' | '\u{2013}' | '\u{2014}' | '/')
                    || g.break_after.unwrap_or_else(|| cjk_break_between(g.ch, glyphs[i + 1].ch))
                {
                    last_ok = Some((i + 1, false));
                } else if hyph_after[i] && may_hyphenate(hyphens) {
                    let hy = hyphen_width(g, &mut hy_cache) * if sp.optical { 1.0 - hang('-').1 } else { 1.0 };
                    if x + hy <= w + shrink {
                        last_ok = Some((i + 1, true));
                    }
                }
            }
            i += 1;
        }
        let (end, hyphen, forced) = brk.unwrap_or((n, false, true));
        hyphens = if hyphen { hyphens + 1 } else { 0 };
        let mut vis_end = end;
        while vis_end > start && glyphs[vis_end - 1].is_space() {
            vis_end -= 1;
        }
        let mut next = end;
        while next < n && glyphs[next].is_space() {
            next += 1;
        }
        let next = next.max(start + 1).min(n.max(start + 1));
        out.push(Break { start, end: vis_end, next: next.min(n), hyphen, forced: forced || end == n });
        if next >= n {
            break;
        }
        start = next;
        line += 1;
    }
    // A forced break as the very last glyph leaves an empty final line (InDesign shows it).
    if let Some(last) = glyphs.last()
        && is_forced(last.ch)
    {
        out.push(Break { start: n, end: n, next: n, hyphen: false, forced: true });
    }
    out
}

/// Natural width of a broken line (with its inserted hyphen).
pub fn line_width(glyphs: &[Glyph], b: &Break) -> f64 {
    let w: f64 = glyphs[b.start..b.end.max(b.start)].iter().map(|g| g.adv).sum();
    w + if b.hyphen && b.end > 0 { glyphs[b.end - 1].size * 0.33 } else { 0.0 }
}

/// Balance Ragged Lines: keep the composer's line count but make the lines as even as possible,
/// by composing against the narrowest measure that still yields that many lines.
pub fn balanced(glyphs: &[Glyph], hyph_after: &[bool], sp: &Spacing, width: &dyn Fn(usize) -> f64) -> Vec<Break> {
    let base = knuth_plass(glyphs, hyph_after, sp, width);
    let n = base.len();
    if n < 2 || glyphs.len() > 3000 || base[..n - 1].iter().any(|b| b.forced) {
        return base;
    }
    let ok = |f: f64| -> Option<Vec<Break>> {
        let w = |j: usize| width(j) * f;
        let br = knuth_plass(glyphs, hyph_after, sp, &w);
        (br.len() == n && br.iter().enumerate().all(|(j, b)| line_width(glyphs, b) <= w(j) + 0.01)).then_some(br)
    };
    let (mut lo, mut hi) = (0.3, 1.0);
    let mut best = base;
    for _ in 0..14 {
        let mid = (lo + hi) / 2.0;
        match ok(mid) {
            Some(b) => {
                best = b;
                hi = mid;
            }
            None => lo = mid,
        }
    }
    best
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tiers_are_used_in_order() {
        // Word stretch alone.
        let (r, ok) = tiered_ratio(5.0, [10.0, 10.0, 10.0], [0.0; 3]);
        assert!(ok && (r - 0.5).abs() < 1e-9);
        // Word saturated, half the letter tier.
        let (r, _) = tiered_ratio(15.0, [10.0, 10.0, 10.0], [0.0; 3]);
        assert!((r - (1.0 + 0.5 * TIER_COST[1])).abs() < 1e-9);
        // Beyond every tier: unbounded word spacing.
        let (r, _) = tiered_ratio(40.0, [10.0, 10.0, 10.0], [0.0; 3]);
        assert!(r > 2.0);
        // Shrink beyond the tiers is infeasible.
        assert!(!tiered_ratio(-25.0, [0.0; 3], [10.0, 10.0, 0.0]).1);
        assert!(tiered_ratio(-15.0, [0.0; 3], [10.0, 10.0, 0.0]).1);
    }
}
