//! Esperanto hyphenation by morpheme: a word is split into prefixes, roots, suffixes and its
//! grammatical ending, and breaks go only between morphemes (`tranĉ-ilo`, never `tran-ĉi-lo`;
//! `mal-san-ul-ejo`). The ending stays with the morpheme before it, and so does a linking vowel
//! inside a compound (`akvo-birdo`). A word that doesn't split into known pieces isn't broken.
//!
//! The roots (`assets/hyphenation/eo.roots`, one per line) come from English Wiktionary's
//! Esperanto entries, CC BY-SA 4.0, extracted by kaikki.org; `examples/eohyphgen.rs` regenerates
//! the list. The affixes are the official ones (Fundamento and Oficialaj Aldonoj).

use std::collections::HashSet;
use std::sync::OnceLock;

static EO_ROOTS: &str = include_str!("../../../../assets/hyphenation/eo.roots");

/// Prefixes: only at the start of a word or after another prefix.
pub const PREFIXES: &[&str] = &["bo", "dis", "ek", "eks", "ge", "mal", "re", "pra", "fi", "fuŝ", "mis"];

/// Suffixes, participle suffixes included: only after another morpheme.
pub const SUFFIXES: &[&str] = &[
    "aĉ", "ad", "aĵ", "an", "ar", "ebl", "ec", "eg", "ej", "em", "end", "er", "estr", "et", "id", "ig", "iĝ", "il", "in", "ind", "ing", "ism", "ist",
    "obl", "on", "op", "uj", "ul", "um", "ĉj", "nj", "ant", "int", "ont", "at", "it", "ot",
];

/// Longest word (in chars) segmented: the search is quadratic in the length.
const MAX_WORD: usize = 64;

/// Longest morpheme tried (in chars).
const MAX_PIECE: usize = 24;

/// The bundled roots (parsed on first use).
pub fn roots() -> &'static HashSet<String> {
    static R: OnceLock<HashSet<String>> = OnceLock::new();
    R.get_or_init(|| parse_roots(EO_ROOTS))
}

/// A root list: one root per line, `#` comments and blank lines skipped.
pub fn parse_roots(text: &str) -> HashSet<String> {
    text.lines().map(str::trim).filter(|l| !l.is_empty() && !l.starts_with('#')).map(str::to_string).collect()
}

/// Length in chars of the grammatical ending of a lowercase word (`-o`, `-a`, `-e`, `-i`, `-as`,
/// `-is`, `-os`, `-us`, `-u`, then `-j`, then `-n`), 0 if none; at least two chars stay before it.
pub fn ending_len(word: &[char]) -> usize {
    const ENDINGS: &[&str] = &["ojn", "ajn", "oj", "aj", "on", "an", "en", "as", "is", "os", "us", "o", "a", "e", "i", "u"];
    for e in ENDINGS {
        let len = e.chars().count();
        if word.len() >= len + 2 && word.iter().rev().take(len).rev().copied().eq(e.chars()) {
            return len;
        }
    }
    0
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Kind {
    Prefix,
    Root,
    Suffix,
}

/// What a morpheme costs: roots more than affixes, short roots more still (they match by chance).
fn cost(len: usize, kind: Kind) -> f32 {
    match kind {
        Kind::Root => {
            1.0 + match len {
                0 | 1 => 9.0,
                2 => 1.2,
                3 => 0.3,
                _ => 0.0,
            }
        }
        Kind::Prefix => 0.6,
        Kind::Suffix => 0.55,
    }
}

/// Extra cost of a linking vowel after a root inside a compound (`akv-o-birdo`).
const LINK: f32 = 0.35;

/// The cheapest split of a lowercase stem (a word without its ending) into morphemes, with its
/// cost: the char index where each morpheme after the first starts. None when no split covers the
/// stem with at least one root (affixes alone would split names and foreign words). `is_root` says
/// whether a piece is a root.
pub fn segment(stem: &[char], is_root: &dyn Fn(&str) -> bool) -> Option<(Vec<usize>, f32)> {
    let n = stem.len();
    if n == 0 || n > MAX_WORD {
        return None;
    }
    // best[i][r]: the cheapest split of stem[..i], with a root among its morphemes when r is 1,
    // without when 0. Splits with and without a root are kept apart so that a cheaper rootless one
    // never hides the split with a root.
    let mut best: Vec<[Option<Step>; 2]> = vec![[None; 2]; n + 1];
    if let Some(b) = best.get_mut(0) {
        b[0] = Some(Step { cost: 0.0, start: 0, kind: Kind::Prefix, had_root: false });
    }
    for i in 0..n {
        for had_root in [false, true] {
            let Some(Some(from)) = best.get(i).map(|b| b[usize::from(had_root)]) else { continue };
            let mut piece = String::new();
            for j in i + 1..=n.min(i + MAX_PIECE) {
                let Some(&ch) = stem.get(j - 1) else { break };
                piece.push(ch);
                let mut kinds = Vec::new();
                if is_root(&piece) {
                    kinds.push(Kind::Root);
                }
                if had_root && SUFFIXES.contains(&piece.as_str()) {
                    kinds.push(Kind::Suffix);
                }
                if (i == 0 || from.kind == Kind::Prefix) && PREFIXES.contains(&piece.as_str()) {
                    kinds.push(Kind::Prefix);
                }
                for kind in kinds {
                    let has_root = had_root || kind == Kind::Root;
                    let step = Step { cost: from.cost + cost(j - i, kind), start: i, kind, had_root };
                    relax(&mut best, j, has_root, step);
                    if kind == Kind::Root && j < n && matches!(stem.get(j), Some('o' | 'a' | 'e' | 'i')) {
                        relax(&mut best, j + 1, has_root, Step { cost: step.cost + LINK, ..step });
                    }
                }
            }
        }
    }
    let total = best.get(n)?[1]?.cost;
    let mut starts = Vec::new();
    let (mut at, mut has_root) = (n, true);
    while at > 0 {
        let step = best.get(at)?[usize::from(has_root)]?;
        if step.start > 0 {
            starts.push(step.start);
        }
        (at, has_root) = (step.start, step.had_root);
    }
    starts.reverse();
    Some((starts, total))
}

/// The last morpheme of a split: its cost so far, where it starts, its kind, and whether the
/// split before it already had a root.
#[derive(Clone, Copy)]
struct Step {
    cost: f32,
    start: usize,
    kind: Kind,
    had_root: bool,
}

fn relax(best: &mut [[Option<Step>; 2]], at: usize, has_root: bool, cand: Step) {
    if let Some(slot) = best.get_mut(at).map(|b| &mut b[usize::from(has_root)])
        && slot.is_none_or(|s| cand.cost < s.cost)
    {
        *slot = Some(cand);
    }
}

/// Break points (char indices) of a lowercase Esperanto word: between its morphemes, never
/// before the ending alone.
pub fn breaks(word: &[char]) -> Vec<usize> {
    breaks_with(word, &|p| roots().contains(p))
}

/// [`breaks`] with another root set.
pub fn breaks_with(word: &[char], is_root: &dyn Fn(&str) -> bool) -> Vec<usize> {
    if word.len() > MAX_WORD {
        return Vec::new();
    }
    let end = ending_len(word);
    let stem = word.get(..word.len() - end).unwrap_or(word);
    segment(stem, is_root).map(|(s, _)| s).unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn h(word: &str) -> String {
        let chars: Vec<char> = word.chars().collect();
        let pts = breaks(&chars);
        let mut s = String::new();
        for (i, c) in chars.iter().enumerate() {
            if pts.contains(&i) {
                s.push('-');
            }
            s.push(*c);
        }
        s
    }

    #[test]
    fn bundled_roots_load() {
        assert!(roots().len() > 5000, "{}", roots().len());
        assert!(roots().contains("tranĉ"));
    }

    #[test]
    fn words_break_between_morphemes_not_syllables() {
        assert_eq!(h("tranĉilo"), "tranĉ-ilo");
        assert_eq!(h("tranĉilojn"), "tranĉ-ilojn");
        assert_eq!(h("malsanulejo"), "mal-san-ul-ejo");
        assert_eq!(h("malegaleco"), "mal-egal-eco");
        assert_eq!(h("lernejo"), "lern-ejo");
        assert_eq!(h("vaporŝipo"), "vapor-ŝipo");
        assert_eq!(h("fervojo"), "fer-vojo");
    }

    #[test]
    fn a_linking_vowel_stays_with_its_root() {
        assert_eq!(h("akvobirdo"), "akvo-birdo");
    }

    #[test]
    fn the_ending_is_never_split_off() {
        let r = |p: &str| p == "parol";
        assert_eq!(breaks_with(&"parolas".chars().collect::<Vec<_>>(), &r), Vec::<usize>::new());
        assert_eq!(breaks_with(&"parolojn".chars().collect::<Vec<_>>(), &r), Vec::<usize>::new());
    }

    #[test]
    fn endings() {
        let e = |w: &str| ending_len(&w.chars().collect::<Vec<_>>());
        assert_eq!(e("domo"), 1);
        assert_eq!(e("domojn"), 3);
        assert_eq!(e("bela"), 1);
        assert_eq!(e("belajn"), 3);
        assert_eq!(e("parolas"), 2);
        assert_eq!(e("parolus"), 2);
        assert_eq!(e("hejmen"), 2);
        assert_eq!(e("ne"), 0);
        assert_eq!(e("kun"), 0);
        assert_eq!(e(""), 0);
    }

    #[test]
    fn unknown_words_stay_whole() {
        assert_eq!(h("xyzzyplugh"), "xyzzyplugh");
    }

    #[test]
    fn affixes_alone_are_not_a_word() {
        // `mal` + `eg` covers `maleg`, but neither is a root: no break.
        assert_eq!(h("malego"), "malego");
        let is_root = |p: &str| p == "san";
        assert!(breaks_with(&"malego".chars().collect::<Vec<_>>(), &is_root).is_empty());
        // With a root, the prefix and suffix around it still break.
        assert_eq!(breaks_with(&"malsanego".chars().collect::<Vec<_>>(), &is_root), vec![3, 6]);
    }

    #[test]
    fn hostile_words_are_bounded() {
        let long: Vec<char> = "malsanulejo".repeat(50).chars().collect();
        assert!(breaks(&long).is_empty());
        assert!(breaks(&[]).is_empty());
        assert!(breaks(&['o']).is_empty());
    }

    #[test]
    fn the_word_limit_counts_the_ending() {
        let is_root = |p: &str| p == "kat";
        let word = |n: usize| -> Vec<char> { format!("{}oj", "kat".repeat(n)).chars().collect() };
        // 62 chars: segmented.
        assert!(!breaks_with(&word(20), &is_root).is_empty());
        // 65 chars, whose 63-char stem tiles: still over the limit.
        assert!(breaks_with(&word(21), &is_root).is_empty());
    }
}
