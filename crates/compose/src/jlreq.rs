//! Japanese composition (W3C *Requirements for Japanese Text Layout*, JLREQ, and JIS X 4051):
//! character classes, mojikumi (spacing between classes), line-edge treatment, justification
//! material and frame-grid cells.
//!
//! Shaping keeps the glyphs' own advances. [`apply_mojikumi`] then turns a full-width punctuation
//! mark into its half-width ink plus *aki* (space) on the side its blank was on, and gives each
//! pair of neighbours the aki its classes call for (JLREQ §3.1, Appendix A/B):
//!
//! - an opening bracket has half an em before it, a closing bracket, comma or full stop half an em
//!   after it, a middle dot a quarter em either side;
//! - consecutive punctuation collapses (`」「` is half an em apart, `。」` and `「『` touch);
//! - Japanese next to Western letters or numerals gets a quarter em (*wa-ō kan*, §3.2.4).
//!
//! The aki is part of the glyph's advance (`aki_before` shifts the ink, `aki_after` follows it) and
//! records how much of it stays at a line start or end ([`Aki::edge`]): the line breakers drop the
//! rest when a line breaks there, which is how a closing bracket ends a line half-width and an
//! opening bracket starts one flush (*tentsuki*, §3.1.5). Aki may shrink and stretch within its
//! range; justification uses it first, then spaces between Japanese characters (`jl_expand`),
//! then glyph scaling: the priorities of JLREQ §3.8.
//!
//! The built-in mojikumi sets are our own, built from JLREQ: no vendor tables are reproduced.

use designcraft_doc::FrameGrid;

use crate::shape::Glyph;

/// JLREQ character classes (Appendix A), the ones composition distinguishes.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Class {
    /// cl-01 opening brackets.
    Open,
    /// cl-02 closing brackets.
    Close,
    /// cl-03 hyphens.
    Hyphen,
    /// cl-04 dividing punctuation (？！).
    Dividing,
    /// cl-05 middle dots (・：；).
    Middle,
    /// cl-06 full stops (。．).
    FullStop,
    /// cl-07 commas (、，).
    Comma,
    /// cl-08 inseparable characters (—…‥).
    Inseparable,
    /// cl-09 iteration marks.
    Iteration,
    /// cl-10 prolonged sound mark.
    Prolonged,
    /// cl-11 small kana.
    SmallKana,
    /// cl-12 prefixed abbreviations (￥＄).
    Prefix,
    /// cl-13 postfixed abbreviations (％℃).
    Postfix,
    /// cl-14 ideographic space.
    IdeographicSpace,
    /// cl-15 hiragana.
    Hiragana,
    /// cl-16 katakana.
    Katakana,
    /// cl-17/18 mathematical symbols and operators.
    Math,
    /// cl-19 ideographic characters (kanji and other full-width characters).
    Ideographic,
    /// cl-24 digits (Western numerals).
    Digit,
    /// cl-26 Western word space.
    Space,
    /// cl-27 Western characters.
    Western,
}

/// The JLREQ class of `c` (Appendix A; characters it doesn't list follow their width).
pub fn class_of(c: char) -> Class {
    match c {
        '‘' | '“' | '（' | '〔' | '［' | '｛' | '〈' | '《' | '「' | '『' | '【' | '⦅' | '〘' | '〖' | '«' | '〝' | '｟' | '｢' => {
            Class::Open
        }
        '’' | '”' | '）' | '〕' | '］' | '｝' | '〉' | '》' | '」' | '』' | '】' | '⦆' | '〙' | '〗' | '»' | '〟' | '｠' | '｣' => {
            Class::Close
        }
        '‐' | '〜' | '゠' | '–' | '～' => Class::Hyphen,
        '？' | '！' | '‼' | '⁇' | '⁈' | '⁉' => Class::Dividing,
        '・' | '：' | '；' | '･' => Class::Middle,
        '。' | '．' | '｡' => Class::FullStop,
        '、' | '，' | '､' => Class::Comma,
        '—' | '―' | '…' | '‥' | '〳' | '〴' | '〵' => Class::Inseparable,
        'ヽ' | 'ヾ' | 'ゝ' | 'ゞ' | '々' | '〻' => Class::Iteration,
        'ー' => Class::Prolonged,
        'ぁ'
        | 'ぃ'
        | 'ぅ'
        | 'ぇ'
        | 'ぉ'
        | 'っ'
        | 'ゃ'
        | 'ゅ'
        | 'ょ'
        | 'ゎ'
        | 'ゕ'
        | 'ゖ'
        | 'ァ'
        | 'ィ'
        | 'ゥ'
        | 'ェ'
        | 'ォ'
        | 'ッ'
        | 'ャ'
        | 'ュ'
        | 'ョ'
        | 'ヮ'
        | 'ヵ'
        | 'ヶ'
        | '\u{31F0}'..='\u{31FF}' => Class::SmallKana,
        '￥' | '＄' | '￡' | '＃' | '€' | '№' => Class::Prefix,
        '°' | '′' | '″' | '℃' | '￠' | '％' | '‰' | '㏋' | 'ℓ' | '\u{3300}'..='\u{3357}' | '\u{3371}'..='\u{33DF}' => Class::Postfix,
        '\u{3000}' => Class::IdeographicSpace,
        '\u{3041}'..='\u{309F}' => Class::Hiragana,
        '\u{30A0}'..='\u{30FF}' => Class::Katakana,
        '＝' | '≠' | '＜' | '＞' | '≦' | '≧' | '⊂' | '⊃' | '∪' | '∩' | '∈' | '∋' | '＋' | '－' | '±' | '×' | '÷' => {
            Class::Math
        }
        '0'..='9' => Class::Digit,
        ' ' | '\u{2002}'..='\u{200A}' => Class::Space,
        c if crate::upright_in_vertical(c) => Class::Ideographic,
        _ => Class::Western,
    }
}

impl Class {
    /// Kanji and kana (and the marks that behave like them): the characters between which
    /// justification may add space and next to which Western text gets *wa-ō* aki.
    pub fn is_japanese_letter(self) -> bool {
        matches!(self, Class::Hiragana | Class::Katakana | Class::Ideographic | Class::Iteration | Class::Prolonged | Class::SmallKana)
    }
    fn is_closing(self) -> bool {
        matches!(self, Class::Close | Class::FullStop | Class::Comma)
    }
    /// The order its blank shrinks in to take a line in (see `Aki::rank`).
    fn shrink_rank(self) -> u8 {
        if self == Class::Middle { 1 } else { 2 }
    }
    /// The blank a full-width glyph of this class has (before, after), in ems.
    fn blanks(self) -> (f64, f64) {
        match self {
            Class::Open => (0.5, 0.0),
            Class::Close | Class::FullStop | Class::Comma => (0.0, 0.5),
            Class::Middle => (0.25, 0.25),
            _ => (0.0, 0.0),
        }
    }
}

/// Space between glyphs from the mojikumi set, in points: the amount used, how much of it may
/// stretch and shrink, and how much stays when the glyph starts or ends a line.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Aki {
    pub w: f64,
    pub stretch: f64,
    pub shrink: f64,
    pub edge: f64,
    /// When the shrink is used to take a line in (JLREQ §3.8.3): after the word spaces, the
    /// middle dots' quarter ems (1), then the half ems of brackets and commas (2), then wa-ō
    /// aki (3).
    pub rank: u8,
}

impl Aki {
    /// The part a line edge drops.
    pub fn drop(&self) -> f64 {
        (self.w - self.edge).max(0.0)
    }
    fn add(&mut self, w: f64, stretch: f64, shrink: f64, edge: f64) {
        self.add_ranked(w, stretch, shrink, edge, 0);
    }
    fn add_ranked(&mut self, w: f64, stretch: f64, shrink: f64, edge: f64, rank: u8) {
        self.rank = self.rank.max(rank);
        self.w += w;
        self.stretch += stretch;
        self.shrink += shrink;
        self.edge += edge;
    }
}

/// How punctuation is set in a line.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Punctuation {
    /// Half-width everywhere (the blank only comes back to justify a line).
    Half,
    /// Full width in the line (consecutive marks collapse), half-width at the line end.
    FullHalfAtEnd,
    /// Full width everywhere, the line end included.
    Full,
}

/// A mojikumi (character spacing) set.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct MojikumiSet {
    /// Id stored in a paragraph's `mojikumi`.
    pub id: &'static str,
    pub label: &'static str,
    pub punctuation: Punctuation,
    /// An opening bracket starting a line (not a paragraph) keeps its half em before it.
    pub keep_open_at_line_start: bool,
    /// Aki between Japanese and Western letters or numerals, in ems (0 = none).
    pub wa_ou: f64,
    /// Consecutive punctuation collapses (JLREQ §3.1.4); off, every mark keeps its own blank.
    pub collapse: bool,
}

/// The built-in mojikumi sets (an empty `mojikumi` sets text solid, without aki).
pub const MOJIKUMI_SETS: &[MojikumiSet] = &[
    MojikumiSet {
        id: "lineEndHalf",
        label: "Full-Width Punctuation, Half at Line End",
        punctuation: Punctuation::FullHalfAtEnd,
        keep_open_at_line_start: false,
        wa_ou: 0.25,
        collapse: true,
    },
    MojikumiSet {
        id: "halfWidth",
        label: "Half-Width Punctuation",
        punctuation: Punctuation::Half,
        keep_open_at_line_start: false,
        wa_ou: 0.25,
        collapse: true,
    },
    MojikumiSet {
        id: "fullWidth",
        label: "Full-Width Punctuation",
        punctuation: Punctuation::Full,
        keep_open_at_line_start: true,
        wa_ou: 0.25,
        collapse: true,
    },
];

/// Text set solid (no mojikumi set) in a justified paragraph: punctuation keeps its full width
/// and every mark its own blank, so nothing moves while lines fit; the blanks may compress so a
/// line one character over takes it in (push-in, JLREQ §3.8.3) instead of pushing it out.
pub const SOLID: MojikumiSet =
    MojikumiSet { id: "", label: "None (Solid)", punctuation: Punctuation::Full, keep_open_at_line_start: true, wa_ou: 0.0, collapse: false };

/// The set with id `id` (also IDML-style names of the same sets), or None (set solid).
pub fn mojikumi_set(id: &str) -> Option<&'static MojikumiSet> {
    let id = id.trim_start_matches("MojikumiTable/").trim_start_matches("$ID/");
    MOJIKUMI_SETS.iter().find(|s| s.id == id)
}

/// The em of `g` along the line, in points.
fn em_of(g: &Glyph) -> f64 {
    let unit = if g.upright { g.sy } else { g.sx };
    let em = (g.face.units_per_em() * unit).abs();
    if em.is_finite() && em > 0.0 { em } else { g.size.abs().max(0.01) }
}

/// Glyphs mojikumi leaves alone: controls and markers, generated glyphs, tate-chu-yoko groups and
/// fixed-width (jidori) groups.
fn skipped(g: &Glyph) -> bool {
    g.adv <= 0.0 || g.len == 0 || g.ch.is_control() || g.tcy.is_some() || g.locked_advance || crate::breaker::is_forced(g.ch)
}

#[derive(Clone, Copy)]
struct Unit {
    class: Class,
    em: f64,
    /// Full-width punctuation, trimmed to its ink: its blanks (before, after) in points.
    blanks: (f64, f64),
}

/// Apply mojikumi set `set` to a paragraph's glyphs (after shaping and kinsoku, before breaking).
pub fn apply_mojikumi(glyphs: &mut [Glyph], set: &MojikumiSet) {
    let units: Vec<Option<Unit>> = glyphs
        .iter_mut()
        .map(|g| {
            if skipped(g) {
                return None;
            }
            let class = class_of(g.ch);
            let em = em_of(g);
            let (b, a) = class.blanks();
            // The glyph's own advance (a frame grid's character aki is already aki after it).
            let own = g.adv - g.aki_before.w - g.aki_after.w;
            let full = (b + a) > 0.0 && (own - em).abs() <= 0.15 * em;
            if !full {
                return Some(Unit { class, em, blanks: (0.0, 0.0) });
            }
            // Trim the blanks: the ink stays where it was drawn, the advance shrinks to it.
            let (b, a) = (b * em, a * em);
            g.adv -= b + a;
            g.dx -= b;
            Some(Unit { class, em, blanks: (b, a) })
        })
        .collect();
    let first = units.iter().position(Option::is_some);
    // The set's aki, added to what the glyphs have (frame-grid character aki) at the end.
    let mut before = vec![Aki::default(); glyphs.len()];
    let mut after = vec![Aki::default(); glyphs.len()];
    for i in 0..glyphs.len() {
        let Some(u) = units[i] else { continue };
        // Paragraph start: like a line start, an opening bracket sits at the edge (below the
        // first-line indent: JIS X 4051's method, JLREQ §3.1.5 ①) unless the set keeps its blank.
        if Some(i) == first && u.blanks.0 > 0.0 && (set.keep_open_at_line_start || u.class != Class::Open) {
            let b = u.blanks.0;
            before[i].add(b, 0.0, 0.0, b);
        }
        let next = units.get(i + 1).copied().flatten();
        match next {
            Some(v) if !matches!(u.class, Class::Space | Class::IdeographicSpace) && !matches!(v.class, Class::Space | Class::IdeographicSpace) => {
                pair(&mut before, &mut after, i, u, v, set);
            }
            _ => {
                // Before a space, a break or the paragraph end: the mark's own blank as aki after
                // it, subject to the line-end rule.
                if u.blanks.1 > 0.0 {
                    let a = u.blanks.1;
                    let (w, stretch, shrink, edge) = closing_aki(set, u.class, a);
                    after[i].add_ranked(w, stretch, shrink, edge, u.class.shrink_rank());
                }
            }
        }
    }
    for ((g, b), a) in glyphs.iter_mut().zip(before).zip(after) {
        g.adv += b.w + a.w;
        g.dx += b.w;
        g.aki_before.add_ranked(b.w, b.stretch, b.shrink, b.edge, b.rank);
        g.aki_after.add_ranked(a.w, a.stretch, a.shrink, a.edge, a.rank);
    }
}

/// Nominal range of the space justification may add between Japanese characters, in ems. Small,
/// so that compressing punctuation (push-in) costs the composer less than spreading a line out
/// (push-out) whenever push-in can make the line fit: the order of JLREQ §3.8. Lines that must
/// spread still do, past the range.
const EXPANSION: f64 = 1.0 / 32.0;

/// Mark where justification may add space between Japanese characters (JLREQ §3.8.2 c): between
/// any two Japanese characters (letters or punctuation) where a line may break. The last resort:
/// the range is small (an eighth of an em) so that the composer prefers lines that don't need it;
/// a line that does gets it anyway, beyond the range if it must. Needed with or without a mojikumi
/// set: text set solid otherwise has nothing to justify with, every inexact line is equally bad,
/// and the composer may strand a line of two characters.
pub fn mark_expansion(glyphs: &mut [Glyph]) {
    let japanese = |g: &Glyph| !skipped(g) && !matches!(class_of(g.ch), Class::Western | Class::Digit | Class::Space | Class::IdeographicSpace);
    for i in 0..glyphs.len().saturating_sub(1) {
        let (a, b) = (&glyphs[i], &glyphs[i + 1]);
        if !japanese(a) || !japanese(b) || (class_of(a.ch) == Class::Inseparable && class_of(b.ch) == Class::Inseparable) {
            continue;
        }
        let allowed = a.break_after.unwrap_or_else(|| crate::breaker::cjk_break_between(a.ch, b.ch));
        if allowed && !a.no_break {
            glyphs[i].jl_expand = EXPANSION * em_of(a);
        }
    }
}

/// Aki after a closing mark (closing bracket, comma, full stop or middle dot) of blank `a`:
/// (used, stretch, shrink, kept at a line end).
fn closing_aki(set: &MojikumiSet, class: Class, a: f64) -> (f64, f64, f64, f64) {
    // A full stop's space doesn't shrink inside a line (JLREQ §3.8.3).
    let shrinkable = if class == Class::FullStop { 0.0 } else { a };
    match set.punctuation {
        Punctuation::Half => (0.0, a, 0.0, 0.0),
        Punctuation::FullHalfAtEnd => (a, 0.0, shrinkable, 0.0),
        Punctuation::Full => (a, 0.0, shrinkable, a),
    }
}

/// Aki between neighbours `i` (unit `u`) and `i + 1` (unit `v`).
fn pair(before: &mut [Aki], after: &mut [Aki], i: usize, u: Unit, v: Unit, set: &MojikumiSet) {
    let (a, b) = (u.blanks.1, v.blanks.0);
    if !set.collapse {
        // Each blank stays where it is, compressible (a full stop's isn't, JLREQ §3.8.3).
        if b > 0.0
            && let Some(x) = before.get_mut(i + 1)
        {
            x.add_ranked(b, 0.0, b, b, v.class.shrink_rank());
        }
        if a > 0.0
            && let Some(x) = after.get_mut(i)
        {
            let (w, stretch, shrink, edge) = closing_aki(set, u.class, a);
            x.add_ranked(w, stretch, shrink, edge, u.class.shrink_rank());
        }
        return;
    }
    // Consecutive punctuation: brackets of one kind and closing marks touch; otherwise the
    // larger blank stays (JLREQ §3.1.4).
    let punct = if (a == 0.0 && b == 0.0) || (u.class == Class::Open && v.class == Class::Open) || (u.class.is_closing() && v.class.is_closing()) {
        0.0
    } else {
        a.max(b)
    };
    if punct > 0.0 {
        if b >= a {
            // The opening bracket's space: dropped where it starts a line (tentsuki) unless the set
            // keeps it.
            // After a full stop it is the sentence break: not used to take a line in (§3.8.3).
            let shrinkable = if u.class == Class::FullStop { 0.0 } else { punct };
            let (w, stretch, shrink) = match set.punctuation {
                Punctuation::Half => (0.0, punct, 0.0),
                _ => (punct, 0.0, shrinkable),
            };
            let edge = if set.keep_open_at_line_start { w } else { 0.0 };
            if let Some(b) = before.get_mut(i + 1) {
                b.add_ranked(w, stretch, shrink, edge, v.class.shrink_rank());
            }
        } else {
            let (w, stretch, shrink, edge) = closing_aki(set, u.class, punct);
            if let Some(a) = after.get_mut(i) {
                a.add_ranked(w, stretch, shrink, edge, u.class.shrink_rank());
            }
        }
        return;
    }
    // Wa-ō aki between Japanese and Western letters or numerals (JLREQ §3.2.4): a quarter em,
    // shrinking to an eighth and stretching to a half. None at a line edge.
    let western = |c: Class| matches!(c, Class::Western | Class::Digit);
    let japanese = |c: Class| c.is_japanese_letter() || matches!(c, Class::Prefix | Class::Postfix);
    if set.wa_ou > 0.0 && ((japanese(u.class) && western(v.class)) || (western(u.class) && japanese(v.class))) {
        let em = if japanese(u.class) { u.em } else { v.em };
        let w = set.wa_ou * em;
        if let Some(a) = after.get_mut(i) {
            a.add_ranked(w, w, w / 2.0, 0.0, 3);
        }
    }
}

/// Characters a frame grid puts one to a cell: Japanese and other full-width characters.
fn on_grid(g: &Glyph) -> bool {
    !skipped(g) && !matches!(class_of(g.ch), Class::Western | Class::Digit | Class::Space)
}

/// Fit a paragraph's glyphs to frame grid `grid`: full-width characters of the grid's size take
/// exactly one cell (the ink centred in it), and every grid character is followed by the grid's
/// character aki (fixed aki after it, kept at a line end).
pub fn snap_to_grid(glyphs: &mut [Glyph], grid: &FrameGrid, vertical: bool) {
    let (along, _) = grid.cell(vertical);
    for g in glyphs.iter_mut().filter(|g| on_grid(g)) {
        let em = em_of(g);
        let same_size = (g.size - grid.size).abs() < 0.01;
        if same_size && g.adv >= 0.4 * em && g.adv <= 1.25 * em {
            g.dx += (along - g.adv) / 2.0;
            g.adv = along;
        }
        let aki = grid.char_aki.max(-g.adv);
        g.adv += aki;
        g.aki_after.add(aki, 0.0, 0.0, aki);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn classes_follow_jlreq_appendix_a() {
        assert_eq!(class_of('「'), Class::Open);
        assert_eq!(class_of('」'), Class::Close);
        assert_eq!(class_of('、'), Class::Comma);
        assert_eq!(class_of('。'), Class::FullStop);
        assert_eq!(class_of('・'), Class::Middle);
        assert_eq!(class_of('ー'), Class::Prolonged);
        assert_eq!(class_of('ッ'), Class::SmallKana);
        assert_eq!(class_of('々'), Class::Iteration);
        assert_eq!(class_of('漢'), Class::Ideographic);
        assert_eq!(class_of('か'), Class::Hiragana);
        assert_eq!(class_of('カ'), Class::Katakana);
        assert_eq!(class_of('A'), Class::Western);
        assert_eq!(class_of('7'), Class::Digit);
        assert_eq!(class_of('…'), Class::Inseparable);
        assert_eq!(class_of('\u{3000}'), Class::IdeographicSpace);
    }

    #[test]
    fn sets_resolve_by_id() {
        assert!(mojikumi_set("").is_none());
        assert!(mojikumi_set("Nothing").is_none());
        assert_eq!(mojikumi_set("lineEndHalf").map(|s| s.punctuation), Some(Punctuation::FullHalfAtEnd));
        assert_eq!(mojikumi_set("MojikumiTable/halfWidth").map(|s| s.punctuation), Some(Punctuation::Half));
        assert!(mojikumi_set("unknown vendor table").is_none());
    }
}
