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
/// stem. `is_root` says whether a piece is a root.
pub fn segment(stem: &[char], is_root: &dyn Fn(&str) -> bool) -> Option<(Vec<usize>, f32)> {
    let n = stem.len();
    if n == 0 || n > MAX_WORD {
        return None;
    }
    // best[i]: (cost, start of the last morpheme, kind of the last morpheme) of the cheapest split
    // of stem[..i].
    let mut best: Vec<Option<(f32, usize, Kind)>> = vec![None; n + 1];
    if let Some(b) = best.get_mut(0) {
        *b = Some((0.0, 0, Kind::Prefix));
    }
    for i in 0..n {
        let Some(Some((c0, _, last))) = best.get(i).copied() else { continue };
        let mut piece = String::new();
        for j in i + 1..=n.min(i + MAX_PIECE) {
            let Some(&ch) = stem.get(j - 1) else { break };
            piece.push(ch);
            let mut kinds = Vec::new();
            if is_root(&piece) {
                kinds.push(Kind::Root);
            }
            if i > 0 && SUFFIXES.contains(&piece.as_str()) {
                kinds.push(Kind::Suffix);
            }
            if (i == 0 || last == Kind::Prefix) && PREFIXES.contains(&piece.as_str()) {
                kinds.push(Kind::Prefix);
            }
            for kind in kinds {
                let c = c0 + cost(j - i, kind);
                relax(&mut best, j, (c, i, kind));
                if kind == Kind::Root && j < n && matches!(stem.get(j), Some('o' | 'a' | 'e' | 'i')) {
                    relax(&mut best, j + 1, (c + LINK, i, kind));
                }
            }
        }
    }
    let (total, _, _) = best.get(n).copied().flatten()?;
    let mut starts = Vec::new();
    let mut at = n;
    while at > 0 {
        let (_, start, _) = best.get(at).copied().flatten()?;
        if start > 0 {
            starts.push(start);
        }
        at = start;
    }
    starts.reverse();
    Some((starts, total))
}

fn relax(best: &mut [Option<(f32, usize, Kind)>], at: usize, cand: (f32, usize, Kind)) {
    if let Some(slot) = best.get_mut(at)
        && slot.is_none_or(|(c, _, _)| cand.0 < c)
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
    fn hostile_words_are_bounded() {
        let long: Vec<char> = "malsanulejo".repeat(50).chars().collect();
        assert!(breaks(&long).is_empty());
        assert!(breaks(&[]).is_empty());
        assert!(breaks(&['o']).is_empty());
    }
}
