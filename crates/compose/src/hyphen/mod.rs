//! Hyphenation. English uses the bundled dictionary and patterns below. Other languages use the
//! Liang patterns compiled into `hypher`, one cargo feature per language: Spanish is Javier Bezos's
//! `hyph-es.tex` 5.0 from hyph-utf8, MIT (see NOTICE).
//!
//! English lookup order for each word:
//! 1. the **dictionary** ([`Dictionary::en_us`]) — the public-domain Moby Hyphenator II word list
//!    by Grady Ward (~160k words), stored compactly in `assets/hyphenation/en-us.dic`; simple
//!    inflections (`'s`, `-s`, `-es`, `-ed`, `-d`) inherit the breaks of their stem;
//! 2. **Liang patterns** ([`Patterns::en_us`]) for words not in the dictionary — our own pattern
//!    set, trained from the same list by [`patgen`] (`examples/hyphgen.rs` regenerates both files);
//! 3. a small syllable **heuristic** for words outside the pattern alphabet.
//!
//! Then the paragraph's limits ([`Limits`]) are applied.

pub mod dict;
pub mod patgen;
pub mod patterns;

use std::sync::OnceLock;

pub use dict::Dictionary;
pub use patterns::Patterns;

static EN_US_DIC: &[u8] = include_bytes!("../../../../assets/hyphenation/en-us.dic");
static EN_US_PAT: &str = include_str!("../../../../assets/hyphenation/en-us.pat");

impl Dictionary {
    /// The bundled US English dictionary (decoded on first use).
    pub fn en_us() -> &'static Dictionary {
        static D: OnceLock<Dictionary> = OnceLock::new();
        D.get_or_init(|| {
            if EN_US_DIC.is_empty() {
                return Dictionary::default();
            }
            Dictionary::from_bytes(EN_US_DIC).unwrap_or_else(|e| {
                log::error!("{e}");
                Dictionary::default()
            })
        })
    }
}

impl Patterns {
    /// The bundled US English patterns (parsed on first use).
    pub fn en_us() -> &'static Patterns {
        static P: OnceLock<Patterns> = OnceLock::new();
        P.get_or_init(|| Patterns::parse(EN_US_PAT))
    }
}

/// Hyphenation limits from the paragraph's Hyphenation settings.
#[derive(Clone, Copy, Debug)]
pub struct Limits {
    pub min_word: usize,
    pub after_first: usize,
    pub before_last: usize,
    pub capitalized: bool,
}

impl Default for Limits {
    fn default() -> Self {
        Limits { min_word: 5, after_first: 2, before_last: 2, capitalized: true }
    }
}

/// The hyphenator for a run of text.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Lang {
    English,
    Patterns(hypher::Lang),
}

impl Lang {
    /// The hyphenator for a language name or locale code (see [`designcraft_doc::language_tag`]).
    /// None when the language has no bundled hyphenation.
    pub fn for_language(language: &str) -> Option<Lang> {
        if language.starts_with("English") {
            return Some(Lang::English);
        }
        let tag = designcraft_doc::language_tag(language)?;
        let sub = designcraft_doc::language_subtag(tag);
        if sub == "en" {
            return Some(Lang::English);
        }
        let b = sub.as_bytes();
        if b.len() != 2 {
            return None;
        }
        hypher::Lang::from_iso([b[0], b[1]]).map(Lang::Patterns)
    }
}

/// Where a word's break points came from.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Source {
    Dictionary,
    Patterns,
    Heuristic,
    None,
}

fn is_apostrophe(c: char) -> bool {
    matches!(c, '\'' | '’' | 'ʼ')
}

fn is_word_char(c: char) -> bool {
    c.is_alphabetic() || is_apostrophe(c)
}

/// Unrestricted break points of one lowercase core (letters and `'` only, no leading/trailing `'`).
fn core_breaks(core: &[char], lang: Lang) -> (Vec<usize>, Source) {
    let n = core.len();
    if n < 2 {
        return (vec![], Source::None);
    }
    if let Lang::Patterns(l) = lang {
        if core.contains(&'\'') {
            return (vec![], Source::None);
        }
        let s: String = core.iter().collect();
        let mut out = Vec::new();
        let mut at = 0;
        let syl: Vec<&str> = hypher::hyphenate_bounded(&s, l, 1, 1).collect();
        for part in &syl[..syl.len().saturating_sub(1)] {
            at += part.chars().count();
            out.push(at);
        }
        return (out, Source::Patterns);
    }
    let dict = Dictionary::en_us();
    let s: String = core.iter().collect();
    let from_mask = |m: u64, len: usize| (1..len.min(64)).filter(|&i| m & (1 << i) != 0).collect::<Vec<_>>();
    if let Some(m) = dict.get(&s) {
        return (from_mask(m, n), Source::Dictionary);
    }
    // Inflections inherit the stem's breaks (the suffix adds no break of its own).
    for suf in ["'s", "s", "es", "ed", "d"] {
        if let Some(stem) = s.strip_suffix(suf)
            && stem.chars().count() >= 2
            && let Some(m) = dict.get(stem)
        {
            return (from_mask(m, stem.chars().count()), Source::Dictionary);
        }
    }
    let pats = Patterns::en_us();
    if !pats.is_empty()
        && let Some(p) = pats.points(core)
    {
        return (p, Source::Patterns);
    }
    (heuristic_points(core), Source::Heuristic)
}

/// Unrestricted break points of `word` (char indices; a hyphen goes before that char), handling
/// case, apostrophes (`don't`, `’s`), surrounding punctuation and compounds (each letter run is
/// hyphenated separately). No break is ever placed next to an apostrophe or non-letter.
pub fn word_breaks(word: &str) -> (Vec<usize>, Source) {
    word_breaks_in(word, Lang::English)
}

/// [`word_breaks`] for a language.
pub fn word_breaks_in(word: &str, lang: Lang) -> (Vec<usize>, Source) {
    let chars: Vec<char> = word.chars().collect();
    let mut out = Vec::new();
    let mut src = Source::None;
    let mut i = 0;
    while i < chars.len() {
        if !is_word_char(chars[i]) {
            i += 1;
            continue;
        }
        let mut j = i;
        while j < chars.len() && is_word_char(chars[j]) {
            j += 1;
        }
        let (mut a, mut b) = (i, j);
        while a < b && is_apostrophe(chars[a]) {
            a += 1;
        }
        while b > a && is_apostrophe(chars[b - 1]) {
            b -= 1;
        }
        let core: Vec<char> = chars[a..b].iter().map(|&c| if is_apostrophe(c) { '\'' } else { c.to_lowercase().next().unwrap_or(c) }).collect();
        let (pts, s) = core_breaks(&core, lang);
        if src == Source::None {
            src = s;
        }
        out.extend(pts.into_iter().filter(|&p| p > 0 && p < core.len() && core[p - 1] != '\'' && core[p] != '\'').map(|p| a + p));
        i = j;
    }
    (out, src)
}

/// Allowed hyphenation points in `word` as char indices (a hyphen goes before that char), with
/// the paragraph limits applied to each letter run.
pub fn hyphen_points(word: &str, lim: &Limits) -> Vec<usize> {
    hyphen_points_in(word, lim, Lang::English)
}

/// [`hyphen_points`] for a language.
pub fn hyphen_points_in(word: &str, lim: &Limits, lang: Lang) -> Vec<usize> {
    let chars: Vec<char> = word.chars().collect();
    let (pts, _) = word_breaks_in(word, lang);
    if pts.is_empty() {
        return pts;
    }
    let min_before = lim.after_first.max(1);
    let min_after = lim.before_last.max(1);
    // Run bounds of each point's letter run (trimmed of apostrophes).
    let run_of = |p: usize| {
        let mut a = p;
        while a > 0 && is_word_char(chars[a - 1]) {
            a -= 1;
        }
        let mut b = p;
        while b < chars.len() && is_word_char(chars[b]) {
            b += 1;
        }
        while a < b && is_apostrophe(chars[a]) {
            a += 1;
        }
        while b > a && is_apostrophe(chars[b - 1]) {
            b -= 1;
        }
        (a, b)
    };
    pts.into_iter()
        .filter(|&p| {
            let (a, b) = run_of(p);
            let run = &chars[a..b];
            let letters = run.iter().filter(|c| c.is_alphabetic()).count();
            if letters < lim.min_word.max(2) {
                return false;
            }
            if letters > 1 && run.iter().filter(|c| c.is_alphabetic()).all(|c| c.is_uppercase()) {
                return false;
            }
            if !lim.capitalized && run[0].is_uppercase() {
                return false;
            }
            let before = chars[a..p].iter().filter(|c| c.is_alphabetic()).count();
            let after = chars[p..b].iter().filter(|c| c.is_alphabetic()).count();
            before >= min_before && after >= min_after
        })
        .collect()
}

/// `word` with `-` at every hyphenation point (tests and diagnostics).
pub fn hyphenate_word(word: &str, lim: &Limits) -> String {
    let pts = hyphen_points(word, lim);
    let mut s = String::with_capacity(word.len() + pts.len());
    for (i, ch) in word.chars().enumerate() {
        if pts.contains(&i) {
            s.push('-');
        }
        s.push(ch);
    }
    s
}

// ---------- heuristic fallback (words outside the pattern alphabet) ----------

fn is_vowel(c: char) -> bool {
    matches!(c.to_ascii_lowercase(), 'a' | 'e' | 'i' | 'o' | 'u' | 'y') || (c.is_alphabetic() && !c.is_ascii())
}

fn onset_pair(a: char, b: char) -> bool {
    let (a, b) = (a.to_ascii_lowercase(), b.to_ascii_lowercase());
    matches!(
        (a, b),
        ('c' | 's' | 't' | 'p' | 'w' | 'g', 'h')
            | ('b' | 'c' | 'd' | 'f' | 'g' | 'k' | 'p' | 't', 'r')
            | ('b' | 'c' | 'f' | 'g' | 'k' | 'p' | 's', 'l')
            | ('q', 'u')
            | ('s', 't' | 'p' | 'c' | 'k')
    )
}

/// Syllable heuristic: V-CV ("ty-po"), VC-CV ("hap-pen"), keeping onset clusters ("gra-phy").
fn heuristic_points(c: &[char]) -> Vec<usize> {
    let n = c.len();
    if n < 4 || !c.iter().all(|c| c.is_alphabetic()) {
        return vec![];
    }
    let v: Vec<bool> = c.iter().map(|&x| is_vowel(x)).collect();
    let mut out = vec![];
    for i in 1..n {
        let ok = if v[i - 1] && !v[i] {
            (i + 1 < n && v[i + 1]) || (i + 2 < n && onset_pair(c[i], c[i + 1]) && v[i + 2])
        } else if !v[i - 1] && !v[i] {
            i >= 2 && v[i - 2] && i + 1 < n && v[i + 1] && !onset_pair(c[i - 1], c[i]) && c[i - 1] != c[i] || (c[i - 1] == c[i] && i >= 2 && v[i - 2])
        } else {
            false
        };
        // Both parts need a sounded vowel: a trailing silent `e`, `ed` or `es` doesn't count.
        let right: String = c[i..].iter().collect();
        let core = right.strip_suffix("ed").or_else(|| right.strip_suffix("es")).or_else(|| right.strip_suffix('e')).unwrap_or(&right);
        if ok && core.chars().any(is_vowel) && core.len() > 1 && v[..i].iter().any(|x| *x) {
            out.push(i);
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn h(w: &str) -> String {
        hyphenate_word(w, &Limits::default())
    }

    #[test]
    fn bundled_data_loads() {
        assert!(Dictionary::en_us().len() > 150_000, "{}", Dictionary::en_us().len());
        assert!(Patterns::en_us().len() > 1000, "{}", Patterns::en_us().len());
    }

    #[test]
    fn dictionary_words() {
        assert_eq!(h("happen"), "hap-pen");
        assert_eq!(h("typography"), "ty-pog-ra-phy");
        assert_eq!(h("composition"), "com-po-si-tion");
        assert_eq!(h("hyphenation"), "hy-phen-a-tion");
        assert_eq!(h("international"), "in-ter-na-tion-al");
        assert_eq!(h("cat"), "cat");
        assert!(!h("changed").contains('-'));
        assert_eq!(h("NASA"), "NASA");
        assert_eq!(word_breaks("publication").1, Source::Dictionary);
    }

    #[test]
    fn case_apostrophes_and_punctuation() {
        assert_eq!(h("Typography"), "Ty-pog-ra-phy");
        assert_eq!(h("TYPOGRAPHY"), "TYPOGRAPHY");
        assert_eq!(h("typography,"), "ty-pog-ra-phy,");
        assert_eq!(h("“typography”"), "“ty-pog-ra-phy”");
        assert_eq!(h("typography's"), "ty-pog-ra-phy's");
        assert_eq!(h("typography’s"), "ty-pog-ra-phy’s");
        assert_eq!(h("'typography'"), "'ty-pog-ra-phy'");
        // Compounds: each part separately, never next to the hyphen.
        assert_eq!(h("typography-composition"), "ty-pog-ra-phy-com-po-si-tion");
    }

    #[test]
    fn inflections_inherit_the_stem() {
        assert_eq!(h("ripples"), "rip-ples");
        assert_eq!(h("gradients"), "gra-di-ents");
        assert_eq!(h("restrained"), "re-strained");
    }

    #[test]
    fn patterns_cover_unknown_words() {
        // Not in the word list: the trained patterns decide.
        let (pts, src) = word_breaks("hyperlocalization");
        assert_eq!(src, Source::Patterns);
        assert!(pts.len() >= 4, "{}", h("hyperlocalization"));
        let (_, src) = word_breaks("zzyzx");
        assert_ne!(src, Source::Dictionary);
    }

    #[test]
    fn limits_are_respected() {
        let lim = Limits { min_word: 5, after_first: 3, before_last: 3, capitalized: false };
        for w in ["typography", "hyphenation", "justification", "Paragraph", "beautiful"] {
            for p in hyphen_points(w, &lim) {
                assert!(p >= 3 && p <= w.chars().count() - 3, "{w} {p}");
            }
        }
        assert!(hyphen_points("Paragraph", &lim).is_empty());
        let lim = Limits { min_word: 12, ..Limits::default() };
        assert!(hyphen_points("typography", &lim).is_empty());
    }

    #[test]
    fn spanish_uses_the_bezos_patterns() {
        let es = Lang::for_language("Spanish").unwrap();
        assert_eq!(es, Lang::for_language("es_ES").unwrap());
        let h = |w: &str| {
            let pts = hyphen_points_in(w, &Limits::default(), es);
            w.chars().enumerate().map(|(i, c)| if pts.contains(&i) { format!("-{c}") } else { c.to_string() }).collect::<String>()
        };
        assert_eq!(h("transición"), "tran-si-ción");
        assert_eq!(h("pingüino"), "pin-güino");
        assert_eq!(h("Construcción"), "Cons-truc-ción");
        assert_eq!(h("desesperación"), "de-ses-pe-ra-ción");
        assert_eq!(h("teatro"), "tea-tro", "a vowel run stays whole");
        assert_eq!(h("calle"), "ca-lle", "ll stays whole");
        assert!(Lang::for_language("French").is_none(), "only languages built in");
    }

    #[test]
    fn heuristic_fallback() {
        let c: Vec<char> = "happen".chars().collect();
        assert_eq!(heuristic_points(&c), vec![3]);
    }
}
