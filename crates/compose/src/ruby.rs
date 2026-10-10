//! Ruby: small glyphs set over laid-out text (to its right in vertical frames, where the frame's
//! turn carries "above" there), or under it (to its left). Kenten are in [`crate::kenten`].
//!
//! Ruby follows JIS X 4051 / JLReq §3.3. Before line breaking, [`reserve`] splits per-character
//! ruby into one unit per parent character, keeps group ruby on one line, and spaces out parent
//! characters whose ruby is longer than they are by what overhang onto the neighbours can't take.
//! After line breaking, [`annotate`] sets the ruby glyphs with the same arithmetic on the placed
//! line. Ruby doesn't change leading.

use designcraft_doc::cjk_settings::AdornmentOverprint;
use designcraft_doc::ruby::{MAX_RUBY_CHARS, RubyAlignment, RubyOverhang, RubyParentSpacing, RubyPosition, RubySpec, RubyType};
use designcraft_fonts::{FaceRef, ScopedFonts};

use crate::shape::Glyph;
use crate::{PlacedGlyph, RunStyle, upright_in_vertical};

/// `text` cut to the longest reading laid out, at a character boundary.
pub(crate) fn capped(text: &str) -> &str {
    match text.char_indices().nth(MAX_RUBY_CHARS) {
        Some((i, _)) => text.get(..i).unwrap_or(text),
        None => text,
    }
}

// ---------- ruby text ----------

/// A shaped ruby glyph, `x` from the ruby's start.
#[derive(Clone, Debug)]
struct RubyGlyph {
    gid: u32,
    ch: char,
    x: f64,
    y: f64,
    adv: f64,
    sx: f64,
    sy: f64,
    tcy: Option<[f64; 3]>,
    /// Part of the tate-chu-yoko group the previous glyph started (spread as one character).
    joined: bool,
}

/// A run's ruby, shaped in its font at its size.
struct Shaped {
    face: FaceRef,
    glyphs: Vec<RubyGlyph>,
    width: f64,
    size: f64,
    /// Width of one ruby character (the unit of overhang and of 1-aki alignment).
    em: f64,
}

/// The ruby's face: its own font, else the parent glyph's. With `fallback`, a face that lacks a
/// character of `text` gives way to a fallback font for it.
fn ruby_face(db: &ScopedFonts<'_>, parent: FaceRef, spec: &RubySpec, text: &str, fallback: bool) -> FaceRef {
    let face = if spec.font_family.is_empty() {
        parent
    } else {
        let style = if spec.font_style.is_empty() { "Regular" } else { spec.font_style.as_str() };
        FaceRef::of(&db.face(&spec.font_family, style))
    };
    if fallback
        && let Some(c) = text.chars().find(|c| !c.is_whitespace() && !face.covers(*c))
        && let Some(f) = db.fallback_for(c, face.id(), None)
    {
        return FaceRef::of(&f);
    }
    face
}

fn tcy_char(c: char, roman: bool) -> bool {
    c.is_ascii_digit() || (roman && c.is_ascii_alphabetic())
}

/// Shape `text` as the ruby of parent text `parent_size` points big.
fn shape_ruby(db: &ScopedFonts<'_>, parent: FaceRef, text: &str, spec: &RubySpec, parent_size: f64, fallback: bool, vertical: bool) -> Shaped {
    let size = spec.size_for(parent_size);
    let face = ruby_face(db, parent, spec, text, fallback);
    let upem = face.units_per_em();
    let k = if upem > 0.0 { size / upem } else { 0.0 };
    let (sx, sy) = (k * spec.x_scale, k * spec.y_scale);
    let features: Vec<designcraft_fonts::Feature> =
        if spec.open_type_pro { designcraft_fonts::feature("ruby").into_iter().collect() } else { Vec::new() };
    let mut glyphs: Vec<RubyGlyph> = designcraft_fonts::shape(&face, text, &features, |c| c)
        .into_iter()
        .map(|sg| RubyGlyph {
            gid: sg.gid,
            ch: text.get(sg.cluster..).and_then(|s| s.chars().next()).unwrap_or(' '),
            x: sg.x_offset as f64 * sx,
            y: -(sg.y_offset as f64) * sy,
            adv: sg.x_advance as f64 * sx,
            sx,
            sy,
            tcy: None,
            joined: false,
        })
        .collect();
    // Tate-chu-yoko: short runs of digits (and Latin letters) take one ruby em along the line.
    if vertical && spec.auto_tcy_digits > 0 {
        let roman = spec.auto_tcy_include_roman;
        let mut i = 0;
        while i < glyphs.len() {
            let j = i + glyphs.get(i..).unwrap_or_default().iter().take_while(|g| tcy_char(g.ch, roman)).count();
            if j == i {
                i += 1;
                continue;
            }
            if u32::try_from(j - i).is_ok_and(|n| n <= spec.auto_tcy_digits)
                && let Some(run) = glyphs.get_mut(i..j)
            {
                let total: f64 = run.iter().map(|g| g.adv).sum();
                let f = if spec.auto_tcy_auto_scale && total > size && total > 0.0 { size / total } else { 1.0 };
                let mut across = -total * f / 2.0;
                for (n, g) in run.iter_mut().enumerate() {
                    g.tcy = Some([size / 2.0, across + g.x * f, size]);
                    across += g.adv * f;
                    g.sx *= f;
                    g.x = 0.0;
                    g.adv = if n == 0 { size } else { 0.0 };
                    g.joined = n > 0;
                }
            }
            i = j;
        }
    }
    // Pen positions: a joined glyph sits at its group's start.
    let mut pen = 0.0;
    let mut start = 0.0;
    for g in &mut glyphs {
        if !g.joined {
            start = pen;
        }
        g.x += start;
        pen += g.adv;
    }
    Shaped { face, glyphs, width: pen, size, em: size * spec.x_scale }
}

// ---------- overhang ----------

/// What is next to a ruby's parent text along the line.
#[derive(Clone, Copy, Debug)]
enum Side {
    /// The line's (or paragraph's) start or end.
    Edge,
    /// A character with no ruby.
    Char { ch: char, adv: f64 },
    /// A character with ruby: `free` is the room its ruby leaves on this side (only a
    /// neighbouring per-character ruby unit leaves any, as in jukugo ruby).
    Ruby { free: f64 },
}

fn is_kana(c: char) -> bool {
    matches!(c, '\u{3041}'..='\u{309F}' | '\u{30A0}'..='\u{30FA}' | '\u{30FC}'..='\u{30FF}' | '\u{31F0}'..='\u{31FF}' | '\u{FF66}'..='\u{FF9F}')
}

/// Opening brackets (JLReq cl-01): their first half is blank.
fn is_opening(c: char) -> bool {
    matches!(
        c,
        '\u{2018}'
            | '\u{201C}'
            | '\u{3008}'
            | '\u{300A}'
            | '\u{300C}'
            | '\u{300E}'
            | '\u{3010}'
            | '\u{3014}'
            | '\u{3016}'
            | '\u{3018}'
            | '\u{301D}'
            | '\u{FF08}'
            | '\u{FF3B}'
            | '\u{FF5B}'
            | '\u{FF5F}'
    )
}

/// Closing brackets, commas and full stops (JLReq cl-02, cl-06, cl-07): their second half is
/// blank.
fn is_closing(c: char) -> bool {
    matches!(
        c,
        '\u{2019}'
            | '\u{201D}'
            | '\u{3001}'
            | '\u{3002}'
            | '\u{3009}'
            | '\u{300B}'
            | '\u{300D}'
            | '\u{300F}'
            | '\u{3011}'
            | '\u{3015}'
            | '\u{3017}'
            | '\u{3019}'
            | '\u{301F}'
            | '\u{FF09}'
            | '\u{FF0C}'
            | '\u{FF0E}'
            | '\u{FF3D}'
            | '\u{FF5D}'
            | '\u{FF60}'
    )
}

/// Middle dots, colons and semicolons (JLReq cl-05): a quarter blank on each side.
fn is_middle_dot(c: char) -> bool {
    matches!(c, '\u{30FB}' | '\u{FF1A}' | '\u{FF1B}')
}

/// How far a ruby may extend over `side` (before the parent when `before`), in points. Ruby
/// goes over kana and the blank part of punctuation, never over kanji or Latin text.
fn allowance(spec: &RubySpec, ruby_em: f64, parent_em: f64, side: Side, before: bool) -> f64 {
    let amount = match spec.overhang {
        RubyOverhang::None => 0.0,
        RubyOverhang::OneRuby => ruby_em,
        RubyOverhang::HalfRuby => ruby_em / 2.0,
        RubyOverhang::OneChar => parent_em,
        RubyOverhang::HalfChar => parent_em / 2.0,
        RubyOverhang::NoLimit => f64::INFINITY,
    };
    let cap = match side {
        // Auto-align sets a longer ruby flush with the line's edge.
        Side::Edge => {
            if spec.auto_align {
                0.0
            } else {
                f64::INFINITY
            }
        }
        Side::Ruby { free } => free,
        Side::Char { ch, adv } => {
            if is_kana(ch) {
                adv
            } else if (before && is_opening(ch)) || (!before && is_closing(ch)) {
                adv / 2.0
            } else if is_middle_dot(ch) {
                adv / 4.0
            } else {
                0.0
            }
        }
    };
    amount.min(cap).max(0.0)
}

/// The overhang allowances the alignment can use: flush-start ruby only extends after its
/// parent, flush-end ruby only before it.
fn usable(align: RubyAlignment, before: f64, after: f64) -> (f64, f64) {
    match align {
        RubyAlignment::Left => (0.0, after),
        RubyAlignment::Right => (before, 0.0),
        _ => (before, after),
    }
}

/// The factor a longer ruby is narrowed by to fit its parent and overhang (1.0 = not at all).
fn auto_scale(spec: &RubySpec, ruby: f64, parent: f64, room: f64) -> f64 {
    if !spec.auto_scaling || ruby <= 0.0 || ruby <= parent + room {
        return 1.0;
    }
    ((parent + room) / ruby).max(spec.scaling_min).min(1.0)
}

/// How far before its parent's start a ruby `excess` longer than the parent starts, given the
/// overhang allowed `before` and `after`: centred where the neighbours allow it, else pushed
/// towards the side that takes more.
fn lead_overhang(align: RubyAlignment, excess: f64, before: f64, after: f64) -> f64 {
    match align {
        RubyAlignment::Left => 0.0,
        RubyAlignment::Right => excess,
        _ => {
            let lo = (excess - after).max(0.0);
            let hi = before.min(excess);
            if lo <= hi { (excess / 2.0).max(lo).min(hi) } else { excess / 2.0 }
        }
    }
}

/// Offsets from the parent's start of the ruby's characters (`advs`, one per slot) over a parent
/// `width` wide that is at least as wide as they are.
fn spread(align: RubyAlignment, width: f64, advs: &[f64], ruby_em: f64) -> Vec<f64> {
    let n = advs.len();
    if n == 0 {
        return Vec::new();
    }
    let w: f64 = advs.iter().sum();
    let gap = (width - w).max(0.0);
    let nf = n as f64;
    let jis = (gap / (2.0 * nf), gap / nf);
    let (lead, inner) = match align {
        RubyAlignment::Left => (0.0, 0.0),
        RubyAlignment::Center => (gap / 2.0, 0.0),
        RubyAlignment::Right => (gap, 0.0),
        RubyAlignment::FullJustify if n > 1 => (0.0, gap / (nf - 1.0)),
        RubyAlignment::FullJustify => (gap / 2.0, 0.0),
        RubyAlignment::Jis => jis,
        RubyAlignment::EqualAki => (gap / (nf + 1.0), gap / (nf + 1.0)),
        RubyAlignment::OneAki if n == 1 => (gap / 2.0, 0.0),
        RubyAlignment::OneAki if gap >= ruby_em => (ruby_em / 2.0, (gap - ruby_em) / (nf - 1.0)),
        RubyAlignment::OneAki => jis,
    };
    let mut out = Vec::with_capacity(n);
    let mut x = lead;
    for a in advs {
        out.push(x);
        x += a + inner;
    }
    out
}

/// The space to add around and between `n` parent characters to make them `need` wider: (before
/// the first, between each two, after the last).
fn parent_spacing(mode: RubyParentSpacing, n: usize, need: f64) -> Option<(f64, f64, f64)> {
    if n == 0 || !need.is_finite() || need <= 1e-9 {
        return None;
    }
    let nf = n as f64;
    Some(match mode {
        RubyParentSpacing::NoAdjustment => return None,
        RubyParentSpacing::BothSides => (need / 2.0, 0.0, need / 2.0),
        RubyParentSpacing::Aki121 => (need / (2.0 * nf), need / nf, need / (2.0 * nf)),
        RubyParentSpacing::EqualAki => (need / (nf + 1.0), need / (nf + 1.0), need / (nf + 1.0)),
        RubyParentSpacing::FullJustify if n > 1 => (0.0, need / (nf - 1.0), 0.0),
        RubyParentSpacing::FullJustify => (need / 2.0, 0.0, need / 2.0),
    })
}

fn intern(styles: &mut Vec<RunStyle>, rs: RunStyle) -> u32 {
    if let Some(i) = styles.iter().rposition(|s| *s == rs) {
        return u32::try_from(i).unwrap_or(u32::MAX);
    }
    styles.push(rs);
    u32::try_from(styles.len().saturating_sub(1)).unwrap_or(u32::MAX)
}

/// What groups glyphs under one ruby: its reading and, for per-character ruby, its unit.
fn ruby_key(st: &RunStyle) -> Option<(&str, Option<u32>)> {
    st.ruby.as_deref().map(|t| (t, st.ruby_unit))
}

/// The room a per-character ruby unit's reading leaves on each side of its parent `adv` wide.
fn unit_free(db: &ScopedFonts<'_>, face: FaceRef, adv: f64, st: &RunStyle, mono: bool, fallback: bool, vertical: bool) -> f64 {
    match (mono, st.ruby_unit, st.ruby.as_deref(), st.ruby_spec.as_ref()) {
        (true, Some(_), Some(t), Some(sp)) => {
            let w = shape_ruby(db, face, t, sp, st.size, fallback || st.missing_font, vertical).width;
            ((adv - w) / 2.0).max(0.0)
        }
        _ => 0.0,
    }
}

// ---------- before line breaking ----------

/// Prepare a shaped paragraph's ruby: one unit per parent character for per-character ruby,
/// no breaks inside group ruby, and parent spacing where a longer ruby needs it. `fallback`: the
/// document draws missing glyphs from fallback fonts.
pub(crate) fn reserve(db: &ScopedFonts<'_>, styles: &mut Vec<RunStyle>, glyphs: &mut [Glyph], fallback: bool, vertical: bool) {
    if !glyphs.iter().any(|g| styles.get(g.style as usize).is_some_and(|s| s.ruby.is_some())) {
        return;
    }
    let in_group = |styles: &[RunStyle], g: &Glyph, key: (&str, Option<u32>)| {
        g.len == 0 || styles.get(g.style as usize).and_then(ruby_key).is_some_and(|k| k == key)
    };
    // Per-character ruby: each parent character gets its own reading.
    let mut i = 0;
    while i < glyphs.len() {
        let Some(st) = glyphs.get(i).and_then(|g| styles.get(g.style as usize)) else {
            i += 1;
            continue;
        };
        let Some(text) = st.ruby.clone() else {
            i += 1;
            continue;
        };
        let mono = st.ruby_unit.is_none() && st.ruby_spec.as_ref().is_some_and(|s| s.kind == RubyType::PerCharacter);
        let mut j = i + 1;
        while glyphs.get(j).is_some_and(|g| in_group(styles.as_slice(), g, (text.as_str(), None))) {
            j += 1;
        }
        if mono {
            let parents: Vec<usize> = (i..j).filter(|&k| glyphs.get(k).is_some_and(|g| g.len > 0)).collect();
            if let Some(readings) = designcraft_doc::ruby::mono_readings(&text, parents.len()) {
                for (u, (&k, reading)) in parents.iter().zip(readings).enumerate() {
                    let end = parents.get(u + 1).copied().unwrap_or(j);
                    let Some(mut rs) = glyphs.get(k).and_then(|g| styles.get(g.style as usize)).cloned() else { continue };
                    rs.ruby = (!reading.is_empty()).then(|| reading.to_string());
                    rs.ruby_unit = Some(u32::try_from(u).unwrap_or(u32::MAX));
                    let idx = intern(styles, rs);
                    for g in glyphs.get_mut(k..end).unwrap_or_default() {
                        g.style = idx;
                    }
                }
            }
        }
        i = j;
    }
    // Parent spacing and breaks, group by group.
    let mut i = 0;
    while i < glyphs.len() {
        let Some(st) = glyphs.get(i).and_then(|g| styles.get(g.style as usize)) else {
            i += 1;
            continue;
        };
        let (Some((text, unit)), Some(spec)) = (ruby_key(st).map(|(t, u)| (t.to_string(), u)), st.ruby_spec.clone()) else {
            i += 1;
            continue;
        };
        let missing = st.missing_font;
        let mut j = i + 1;
        while glyphs.get(j).is_some_and(|g| in_group(styles.as_slice(), g, (text.as_str(), unit))) {
            j += 1;
        }
        let parents: Vec<usize> = (i..j).filter(|&k| glyphs.get(k).is_some_and(|g| g.len > 0)).collect();
        let (Some(&first), Some(&last)) = (parents.first(), parents.last()) else {
            i = j;
            continue;
        };
        let Some((face, parent_size)) = glyphs.get(first).map(|g| (g.face, g.size)) else {
            i = j;
            continue;
        };
        let shaped = shape_ruby(db, face, &text, &spec, parent_size, fallback || missing, vertical);
        let width: f64 = glyphs.get(i..j).unwrap_or_default().iter().map(|g| g.adv).sum();
        let mono = unit.is_some();
        let side = |k: Option<usize>| -> Side {
            let Some(g) = k.and_then(|k| glyphs.get(k)) else { return Side::Edge };
            if g.ch.is_control() || g.ch == crate::shape::HIDDEN {
                return Side::Edge;
            }
            match styles.get(g.style as usize) {
                Some(s) if s.ruby.is_some() => Side::Ruby { free: unit_free(db, g.face, g.adv, s, mono, fallback, vertical) },
                _ => Side::Char { ch: g.ch, adv: g.adv },
            }
        };
        let before = allowance(&spec, shaped.em, parent_size, side(i.checked_sub(1)), true);
        let after = allowance(&spec, shaped.em, parent_size, side(Some(j)), false);
        let (before, after) = usable(spec.alignment, before, after);
        let w = shaped.width * auto_scale(&spec, shaped.width, width, before + after);
        if let Some((lead, inner, trail)) = parent_spacing(spec.parent_spacing, parents.len(), w - width - before - after) {
            if let Some(g) = glyphs.get_mut(first) {
                g.dx += lead;
                g.adv += lead;
            }
            for pair in parents.windows(2) {
                if let Some(g) = pair.get(1).and_then(|n| n.checked_sub(1)).and_then(|k| glyphs.get_mut(k)) {
                    g.adv += inner;
                }
            }
            if let Some(g) = j.checked_sub(1).and_then(|k| glyphs.get_mut(k)) {
                g.adv += trail;
            }
        }
        // The overhang this relies on needs the neighbour on the same line: at a line edge the
        // edge takes none (auto-align) or only the overhang amount, so the group stays with each
        // neighbour its ruby overhangs and the spacing reserved here is what `annotate` places.
        let spaced: f64 = glyphs.get(i..j).unwrap_or_default().iter().map(|g| g.adv).sum();
        let excess = w - spaced;
        if excess > 1e-9 {
            let lead = lead_overhang(spec.alignment, excess, before, after);
            if lead > 1e-9
                && let Some(g) = i.checked_sub(1).and_then(|k| glyphs.get_mut(k))
            {
                g.break_after = Some(false);
            }
            if excess - lead > 1e-9
                && let Some(g) = j.checked_sub(1).and_then(|k| glyphs.get_mut(k))
            {
                g.break_after = Some(false);
            }
        }
        // Group ruby keeps its parent on one line (JLReq 3.3).
        if !mono {
            for g in glyphs.get_mut(first..last).unwrap_or_default() {
                g.break_after = Some(false);
            }
        }
        i = j;
    }
}

// ---------- after line breaking ----------

/// The run style ruby glyphs are drawn in: the parent's, with the ruby's colours and none of the
/// parent's decorations.
fn ruby_style(styles: &mut Vec<RunStyle>, base: &RunStyle, spec: &RubySpec, size: f64, missing: bool) -> u32 {
    let mut rs = base.clone();
    if spec.fill.is_empty() {
        if let Some(t) = spec.fill_tint {
            rs.fill_tint = t;
        }
    } else {
        rs.fill.clone_from(&spec.fill);
        rs.fill_tint = spec.fill_tint.unwrap_or(1.0);
    }
    if spec.stroke.is_empty() {
        if let Some(t) = spec.stroke_tint {
            rs.stroke_tint = t;
        }
    } else {
        rs.stroke.clone_from(&spec.stroke);
        rs.stroke_tint = spec.stroke_tint.unwrap_or(1.0);
    }
    if let Some(w) = spec.stroke_weight {
        rs.stroke_weight = w;
    }
    // Text itself never overprints, so `Auto` (follow the text) is off.
    rs.overprint_fill = spec.overprint_fill == AdornmentOverprint::On;
    rs.overprint_stroke = spec.overprint_stroke == AdornmentOverprint::On;
    rs.size = size;
    rs.missing_font = missing;
    rs.underline = false;
    rs.strikethrough = false;
    rs.custom_tracking = false;
    rs.condition = None;
    rs.xml_tag = None;
    rs.ruby = None;
    rs.ruby_spec = None;
    rs.ruby_unit = None;
    rs.kenten_mark = None;
    rs.warichu = false;
    intern(styles, rs)
}

/// How far the ruby must move away from its parent (down for ruby below, up for ruby above: the
/// result is signed along y) so that it clears kenten set on the same side of the parent
/// characters. Kenten sit next to the parent (JIS X 4051, JLReq: emphasis dots go between the
/// base characters and ruby on the same side); the ruby's em box then starts where the marks' em
/// box ends. `y` is the ruby's baseline without kenten and `tall` its em height.
fn kenten_clearance(styles: &[RunStyle], base: &[&PlacedGlyph], shaped: &Shaped, tall: f64, position: RubyPosition, y: f64) -> f64 {
    let below = position == RubyPosition::BelowLeft;
    let (ruby_top, ruby_bottom) = shaped.face.em_box();
    let k = tall / shaped.face.units_per_em().max(1.0);
    let overlap = base
        .iter()
        .filter_map(|b| {
            let m = styles.get(b.style as usize)?.kenten_mark.as_ref().filter(|m| m.below == below)?;
            let (top, bottom) = b.face.em_box();
            let h = m.size * m.y_scale;
            Some(if below {
                (b.y - bottom * b.sy + m.distance + h) - (y - ruby_top * k)
            } else {
                (y - ruby_bottom * k) - (b.y - top * b.sy - m.distance - h)
            })
        })
        .fold(0.0, f64::max);
    if below { overlap } else { -overlap }
}

/// Add the ruby of a laid-out line's glyphs. `text` is the story text the glyphs'
/// bytes index; `glyph_fallback`: the document draws missing glyphs from fallback fonts (a
/// missing font's substitute always does); `vertical`: the frame is vertical.
pub(crate) fn annotate(
    db: &ScopedFonts<'_>,
    styles: &mut Vec<RunStyle>,
    text: &str,
    line: &mut Vec<PlacedGlyph>,
    glyph_fallback: bool,
    vertical: bool,
) {
    if !line.iter().any(|g| styles.get(g.style as usize).is_some_and(|s| s.ruby.is_some())) {
        return;
    }
    let mut extra = Vec::new();
    let mut i = 0;
    while i < line.len() {
        let Some(g) = line.get(i) else { break };
        let Some(st) = styles.get(g.style as usize).filter(|_| g.len > 0 && g.visible).cloned() else {
            i += 1;
            continue;
        };
        let fallback = glyph_fallback || st.missing_font;
        let (Some(reading), Some(spec)) = (st.ruby.clone(), st.ruby_spec.clone()) else {
            i += 1;
            continue;
        };
        // The group: following glyphs with the same ruby (and unit).
        let key = (reading.as_str(), st.ruby_unit);
        let mut j = i + 1;
        while line.get(j).is_some_and(|b| b.len == 0 || styles.get(b.style as usize).and_then(ruby_key).is_some_and(|k| k == key)) {
            j += 1;
        }
        let group = line.get(i..j).unwrap_or_default();
        let base: Vec<&PlacedGlyph> = group.iter().filter(|b| b.len > 0).collect();
        let Some(&first) = base.first() else {
            i = j;
            continue;
        };
        // The parent's extent from its pen positions (parent spacing lies inside it).
        let x0 = base.iter().map(|b| b.x - b.dx).fold(f64::MAX, f64::min);
        let x1 = base.iter().map(|b| b.x - b.dx + b.adv).fold(f64::MIN, f64::max);
        let width = (x1 - x0).max(0.0);
        let shaped = shape_ruby(db, first.face, &reading, &spec, st.size, fallback, vertical);
        let mono = st.ruby_unit.is_some();
        let side = |b: Option<&PlacedGlyph>| -> Side {
            let Some(b) = b.filter(|b| b.visible) else { return Side::Edge };
            match styles.get(b.style as usize) {
                Some(s) if s.ruby.is_some() => Side::Ruby { free: unit_free(db, b.face, b.adv, s, mono, glyph_fallback, vertical) },
                _ => Side::Char { ch: text.get(b.byte..).and_then(|s| s.chars().next()).unwrap_or(' '), adv: b.adv },
            }
        };
        let prev = line.get(..i).unwrap_or_default().iter().rev().find(|b| b.len > 0);
        let next = line.get(j..).unwrap_or_default().iter().find(|b| b.len > 0);
        let before = allowance(&spec, shaped.em, st.size, side(prev), true);
        let after = allowance(&spec, shaped.em, st.size, side(next), false);
        let (before, after) = usable(spec.alignment, before, after);
        let scale = auto_scale(&spec, shaped.width, width, before + after);
        let w = shaped.width * scale;
        // Each glyph's offset from the parent's start.
        let offsets: Vec<f64> = if w <= width + 1e-9 {
            let mut slots: Vec<f64> = Vec::new();
            for r in &shaped.glyphs {
                match slots.last_mut() {
                    Some(a) if r.joined => *a += r.adv,
                    _ => slots.push(r.adv),
                }
            }
            let at = spread(spec.alignment, width, &slots, shaped.em);
            let (mut slot, mut slot_x) = (0usize, 0.0);
            let mut out = Vec::with_capacity(shaped.glyphs.len());
            for (n, r) in shaped.glyphs.iter().enumerate() {
                if !r.joined {
                    if n > 0 {
                        slot += 1;
                    }
                    slot_x = r.x;
                }
                out.push(at.get(slot).copied().unwrap_or(0.0) + (r.x - slot_x));
            }
            out
        } else {
            let lead = lead_overhang(spec.alignment, w - width, before, after);
            shaped.glyphs.iter().map(|r| r.x * scale - lead).collect()
        };
        let tall = shaped.size * spec.y_scale;
        let y = match spec.position {
            // Its em box sits just clear of the parent's.
            RubyPosition::AboveRight => -(st.size * 0.88 + tall * 0.2) - spec.y_offset,
            RubyPosition::BelowLeft => st.size * 0.12 + tall * 0.96 + spec.y_offset,
        };
        let y = y + kenten_clearance(styles, &base, &shaped, tall, spec.position, y);
        let missing = if spec.font_family.is_empty() { st.missing_font } else { !db.has_family(&spec.font_family) };
        let style = ruby_style(styles, &st, &spec, shaped.size, missing);
        for (r, off) in shaped.glyphs.iter().zip(offsets) {
            extra.push(PlacedGlyph {
                face: shaped.face,
                gid: r.gid,
                x: x0 + off + spec.x_offset,
                y: y + r.y,
                adv: r.adv * scale,
                sx: r.sx * scale,
                sy: r.sy,
                style,
                byte: first.byte,
                len: 0,
                visible: true,
                upright: r.tcy.is_none() && upright_in_vertical(r.ch),
                tcy: r.tcy,
                rtl: false,
                dx: 0.0,
            });
        }
        i = j;
    }
    line.extend(extra);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn shorter_ruby_spreads_by_its_alignment() {
        // A 24 pt parent and three 6 pt ruby characters: 6 pt to spare.
        assert_eq!(spread(RubyAlignment::Jis, 24.0, &[6.0, 6.0, 6.0], 6.0), [1.0, 9.0, 17.0]);
        assert_eq!(spread(RubyAlignment::EqualAki, 24.0, &[6.0, 6.0, 6.0], 6.0), [1.5, 9.0, 16.5]);
        assert_eq!(spread(RubyAlignment::OneAki, 24.0, &[6.0, 6.0, 6.0], 6.0), [3.0, 9.0, 15.0]);
        assert_eq!(spread(RubyAlignment::FullJustify, 24.0, &[6.0, 6.0, 6.0], 6.0), [0.0, 9.0, 18.0]);
        assert_eq!(spread(RubyAlignment::Right, 24.0, &[6.0, 6.0, 6.0], 6.0), [6.0, 12.0, 18.0]);
    }

    #[test]
    fn longer_ruby_leans_to_the_side_it_may_overhang() {
        // 6 pt too long; only the following kana takes overhang.
        assert_eq!(lead_overhang(RubyAlignment::Jis, 6.0, 0.0, 6.0), 0.0);
        assert_eq!(lead_overhang(RubyAlignment::Jis, 6.0, 6.0, 6.0), 3.0);
        assert_eq!(lead_overhang(RubyAlignment::Jis, 6.0, 6.0, 0.0), 6.0);
        // Neither side takes enough: centred.
        assert_eq!(lead_overhang(RubyAlignment::Jis, 6.0, 1.0, 1.0), 3.0);
    }

    #[test]
    fn overhang_goes_over_kana_and_the_blank_half_of_punctuation_only() {
        let spec = RubySpec::of(&designcraft_doc::CharProps::default());
        let kana = Side::Char { ch: 'で', adv: 12.0 };
        let kanji = Side::Char { ch: '字', adv: 12.0 };
        let stop = Side::Char { ch: '。', adv: 12.0 };
        assert_eq!(allowance(&spec, 6.0, 12.0, kana, false), 6.0);
        assert_eq!(allowance(&spec, 6.0, 12.0, kanji, false), 0.0);
        assert_eq!(allowance(&spec, 6.0, 12.0, stop, false), 6.0);
        assert_eq!(allowance(&spec, 6.0, 12.0, stop, true), 0.0);
        assert_eq!(allowance(&spec, 6.0, 12.0, Side::Edge, true), 0.0);
    }

    #[test]
    fn parent_spacing_splits_the_extra_room() {
        assert_eq!(parent_spacing(RubyParentSpacing::Aki121, 2, 8.0), Some((2.0, 4.0, 2.0)));
        assert_eq!(parent_spacing(RubyParentSpacing::FullJustify, 1, 8.0), Some((4.0, 0.0, 4.0)));
        assert_eq!(parent_spacing(RubyParentSpacing::NoAdjustment, 2, 8.0), None);
        assert_eq!(parent_spacing(RubyParentSpacing::Aki121, 2, f64::NAN), None);
    }

    #[test]
    fn capped_cuts_long_readings_at_a_char_boundary() {
        let long = "あ".repeat(MAX_RUBY_CHARS + 10);
        assert_eq!(capped(&long).chars().count(), MAX_RUBY_CHARS);
    }
}
