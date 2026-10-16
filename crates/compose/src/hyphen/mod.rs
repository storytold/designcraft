//! Hyphenation. English uses the bundled dictionary and patterns below. Other languages use the
//! Liang patterns compiled into `hypher`, one cargo feature per language: Spanish is Javier Bezos's
//! `hyph-es.tex` 5.0 from hyph-utf8, MIT; Portuguese is `hyph-pt.tex` 1.4 from hyph-utf8 (Pedro J.
//! de Rezende, J. Joao Dias Almeida, Leonardo Araujo, Aline Benevides), BSD-3-Clause; Norwegian is
//! `hyph-no.tex` from hyph-utf8 (Rune Kleveland, Ole Michael Selberg), permissive (see NOTICE).
//!
//! Russian also uses the compiled data in hypher (feature "russian"). Lookup normalizes
//! Russian stress/decomposed letters only; break points retain original source indices.
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
use unicode_normalization::{UnicodeNormalization, char::is_combining_mark};
use unicode_segmentation::UnicodeSegmentation;

pub use dict::Dictionary;
pub use patterns::Patterns;

static EN_US_DIC: &[u8] = include_bytes!("../../../../assets/hyphenation/en-us.dic");
static EN_US_PAT: &str = include_str!("../../../../assets/hyphenation/en-us.pat");

/// Languages with bundled hyphenation data. An unsupported language has no automatic breaks.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Lang {
    English,
    Russian,
    Patterns(hypher::Lang),
}

impl Lang {
    /// Select data from a resolved BCP 47 language tag.
    pub fn from_tag(tag: &str) -> Option<Self> {
        match designcraft_doc::language_subtag(tag) {
            t if t.eq_ignore_ascii_case("en") => Some(Self::English),
            t if t.eq_ignore_ascii_case("ru") => Some(Self::Russian),
            sub => {
                let b = sub.as_bytes();
                if b.len() != 2 {
                    return None;
                }
                hypher::Lang::from_iso([b[0].to_ascii_lowercase(), b[1].to_ascii_lowercase()]).map(Self::Patterns)
            }
        }
    }

    /// Select data from a document language name or locale code (including IDML names).
    pub fn for_language(name: &str) -> Option<Self> {
        if name.starts_with("English") {
            return Some(Self::English);
        }
        designcraft_doc::language_tag(name).and_then(Self::from_tag)
    }
}

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

// Unsupported/oversized letter graphemes stay in the word and reject the whole run;
// treating them as punctuation would incorrectly hyphenate a Russian suffix in isolation.
const INVALID_RUSSIAN_GRAPHEME: char = '\u{fffd}';
const MAX_RUSSIAN_GRAPHEME_CHARS: usize = 32;

fn is_run_char(c: char, language: Lang) -> bool {
    is_word_char(c) || (language == Lang::Russian && c == INVALID_RUSSIAN_GRAPHEME)
}

/// One lookup character per source grapheme, with boundaries in ORIGINAL scalar indices.
/// The bounded per-grapheme NFC operation never normalizes a document or a glyph stream.
struct RussianWord {
    chars: Vec<char>,
    source_boundaries: Vec<usize>,
}

impl RussianWord {
    fn new(word: &str) -> Self {
        let mut chars = Vec::new();
        let mut source_boundaries = vec![0];
        let mut source_end = 0usize;
        for grapheme in word.graphemes(true) {
            let source_len = grapheme.chars().count();
            let Some(first) = grapheme.chars().next() else { continue };
            let c = if source_len == 1 && matches!(first, 'а'..='я' | 'А'..='Я' | 'ё' | 'Ё') {
                first
            } else if is_word_char(first) || is_combining_mark(first) {
                if source_len > MAX_RUSSIAN_GRAPHEME_CHARS {
                    INVALID_RUSSIAN_GRAPHEME
                } else {
                    // NFD also exposes canonically equivalent stress forms. Strip only acute
                    // and grave stress, then NFC composes е + diaeresis / и + breve. Keeping
                    // case lets paragraph capitalization limits operate on the lookup letters.
                    let mut normalized = grapheme.nfd().filter(|&c| !matches!(c, '\u{0300}' | '\u{0301}')).nfc();
                    match (normalized.next(), normalized.next()) {
                        (Some(c), None) if !is_combining_mark(c) => c,
                        _ => INVALID_RUSSIAN_GRAPHEME,
                    }
                }
            } else {
                first
            };
            chars.push(c);
            source_end = source_end.saturating_add(source_len);
            source_boundaries.push(source_end);
        }
        Self { chars, source_boundaries }
    }

    fn source_points(&self, points: Vec<usize>) -> Vec<usize> {
        points.into_iter().filter_map(|p| self.source_boundaries.get(p).copied()).collect()
    }
}

/// Unrestricted break points of one lowercase core (letters and `'` only, no leading/trailing `'`).
fn core_breaks(core: &[char], language: Lang) -> (Vec<usize>, Source) {
    let n = core.len();
    if n < 2 {
        return (vec![], Source::None);
    }
    let patterns = match language {
        Lang::English => None,
        Lang::Patterns(l) => Some(l),
        Lang::Russian => {
            // Reject unsupported or mixed graphemes as a whole word, not as a Russian suffix.
            if !core.iter().all(|&c| matches!(c, 'а'..='я' | 'ё')) {
                return (vec![], Source::None);
            }
            Some(hypher::Lang::Russian)
        }
    };
    if let Some(language) = patterns {
        if core.contains(&'\'') {
            return (vec![], Source::None);
        }
        let s: String = core.iter().collect();
        let syllables: Vec<&str> = hypher::hyphenate_bounded(&s, language, 1, 1).collect();
        let mut out = Vec::new();
        let mut at = 0;
        for part in &syllables[..syllables.len().saturating_sub(1)] {
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

/// Unrestricted break points using the selected language's data. See [`word_breaks`] for the
/// indexing and punctuation rules. Russian composes decomposed letters and ignores acute/grave
/// stress for lookup; returned points remain original source character indices at grapheme
/// boundaries. Russian never falls back to English or a syllable heuristic.
pub fn word_breaks_in(word: &str, language: Lang) -> (Vec<usize>, Source) {
    if language == Lang::Russian {
        let normalized = RussianWord::new(word);
        let (points, source) = word_breaks_from_chars(&normalized.chars, language);
        return (normalized.source_points(points), source);
    }
    word_breaks_from_chars(&word.chars().collect::<Vec<_>>(), language)
}

fn word_breaks_from_chars(chars: &[char], language: Lang) -> (Vec<usize>, Source) {
    let mut out = Vec::new();
    let mut src = Source::None;
    let mut i = 0;
    while i < chars.len() {
        if !is_run_char(chars[i], language) {
            i += 1;
            continue;
        }
        let mut j = i;
        while j < chars.len() && is_run_char(chars[j], language) {
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
        let (pts, s) = core_breaks(&core, language);
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

/// Allowed points from the selected language's data, with paragraph limits applied. Russian
/// additionally requires at least two letters on each side, as prescribed by its pattern set.
pub fn hyphen_points_in(word: &str, lim: &Limits, language: Lang) -> Vec<usize> {
    if language == Lang::Russian {
        let normalized = RussianWord::new(word);
        return normalized.source_points(limited_points(&normalized.chars, lim, language));
    }
    limited_points(&word.chars().collect::<Vec<_>>(), lim, language)
}

fn limited_points(chars: &[char], lim: &Limits, language: Lang) -> Vec<usize> {
    let (pts, _) = word_breaks_from_chars(chars, language);
    if pts.is_empty() {
        return pts;
    }
    let min_fragment = if language == Lang::Russian { 2 } else { 1 };
    let min_before = lim.after_first.max(min_fragment);
    let min_after = lim.before_last.max(min_fragment);
    // Run bounds of each point's letter run (trimmed of apostrophes).
    let run_of = |p: usize| {
        let mut a = p;
        while a > 0 && is_run_char(chars[a - 1], language) {
            a -= 1;
        }
        let mut b = p;
        while b < chars.len() && is_run_char(chars[b], language) {
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
            // Words in capitals (FERRUM COMMODI) hyphenate like capitalized words, as InDesign's: Hyphenate
            // Capitalized Words off keeps both, acronyms included, whole.
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
    hyphenate_word_in(word, lim, Lang::English)
}

/// Diagnostic form of [`hyphen_points_in`], inserting a hyphen at every allowed point.
pub fn hyphenate_word_in(word: &str, lim: &Limits, language: Lang) -> String {
    let pts = hyphen_points_in(word, lim, language);
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
        assert_eq!(h("TYPOGRAPHY"), "TY-POG-RA-PHY");
        assert_eq!(h("COMMODI"), "COM-MO-DI");
        let whole = Limits { capitalized: false, ..Limits::default() };
        assert_eq!(hyphenate_word("TYPOGRAPHY", &whole), "TYPOGRAPHY", "Hyphenate Capitalized Words off");
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
    fn portuguese_uses_the_hyph_pt_patterns() {
        let pt = Lang::for_language("Portuguese: Brazilian").unwrap();
        assert_eq!(pt, Lang::for_language("Portuguese").unwrap());
        assert_eq!(pt, Lang::for_language("pt_BR").unwrap());
        assert_ne!(pt, Lang::for_language("Spanish").unwrap());
        // hyph-pt's own minimum: three letters after the last break.
        let lim = Limits { before_last: 3, ..Limits::default() };
        let h = |w: &str| {
            let pts = hyphen_points_in(w, &lim, pt);
            w.chars().enumerate().map(|(i, c)| if pts.contains(&i) { format!("-{c}") } else { c.to_string() }).collect::<String>()
        };
        assert_eq!(h("matemática"), "ma-te-má-tica");
        assert_eq!(h("orientação"), "ori-en-ta-ção");
        assert_eq!(h("Universidade"), "Uni-ver-si-dade");
        assert_eq!(h("experiência"), "ex-pe-ri-ên-cia");
        assert_eq!(h("pós-graduação"), "pós-gra-du-a-ção", "each part of a compound");
    }

    #[test]
    fn norwegian_uses_the_hyph_no_patterns() {
        let nb = Lang::for_language("Norwegian: Bokmål").unwrap();
        assert_eq!(nb, Lang::for_language("Norwegian").unwrap());
        assert_eq!(nb, Lang::for_language("Norwegian: Nynorsk").unwrap());
        assert_eq!(nb, Lang::for_language("nb").unwrap());
        assert_eq!(nb, Lang::for_language("nn").unwrap());
        assert_eq!(nb, Lang::for_language("no").unwrap());
        assert_ne!(nb, Lang::for_language("Spanish").unwrap());
        let h = |w: &str| {
            let pts = hyphen_points_in(w, &Limits::default(), nb);
            w.chars().enumerate().map(|(i, c)| if pts.contains(&i) { format!("-{c}") } else { c.to_string() }).collect::<String>()
        };
        assert_eq!(h("kaller"), "kal-ler");
        assert_eq!(h("bilde"), "bil-de");
        assert_eq!(h("sjøfugl"), "sjø-fugl");
        assert_eq!(h("sirkulasjonen"), "sir-ku-la-sjo-nen");
        assert_eq!(h("beveger"), "be-ve-ger");
    }

    #[test]
    fn heuristic_fallback() {
        let c: Vec<char> = "happen".chars().collect();
        assert_eq!(heuristic_points(&c), vec![3]);
    }

    #[test]
    fn russian_uses_hypher_and_respects_limits() {
        for name in ["Russian", "$ID/Russian", "ru", "ru-RU", "ru_RU"] {
            assert_eq!(Lang::for_language(name), Some(Lang::Russian), "{name}");
        }
        let ru = Lang::Russian;
        assert_eq!(word_breaks_in("типография", ru).1, Source::Patterns);
        assert_eq!(hyphenate_word_in("типография", &Limits::default(), ru), "ти-по-гра-фия");
        let strict = Limits { after_first: 3, before_last: 3, ..Limits::default() };
        assert_eq!(hyphen_points_in("типография", &strict, ru), vec![4, 7]);
        assert!(hyphen_points_in("типография", &Limits { min_word: 11, ..strict }, ru).is_empty());
        assert!(hyphen_points_in("Типография", &Limits { capitalized: false, ..strict }, ru).is_empty());
        for text in ["typography", "типогpaфия", "т\u{0303}ипография"] {
            assert!(hyphen_points_in(text, &Limits::default(), ru).is_empty(), "{text}");
        }
        assert_eq!(Lang::for_language("pt_BR"), Some(Lang::Patterns(hypher::Lang::Portuguese)));
        assert_eq!(Lang::for_language("es_ES"), Some(Lang::Patterns(hypher::Lang::Spanish)));
        assert!(Lang::for_language("Ukrainian").is_none());
    }

    #[test]
    fn russian_lookup_preserves_decomposed_and_stressed_source_boundaries() {
        for (text, expected) in [("бере\u{0308}за", vec![2, 5]), ("маи\u{0306}ка", vec![4]), ("типо\u{0301}графия", vec![2, 5, 8])]
        {
            let points = hyphen_points_in(text, &Limits::default(), Lang::Russian);
            assert_eq!(points, expected, "{text}");
            for point in points {
                let byte = text.char_indices().nth(point).unwrap().0;
                assert!(text.grapheme_indices(true).any(|(i, _)| i == byte), "{text}: {point}");
            }
            assert_eq!(hyphenate_word_in(text, &Limits::default(), Lang::Russian).replace('-', ""), text);
        }
        let stressed = "ти\u{0301}по\u{0301}гра\u{0301}фи\u{0301}я";
        assert!(
            hyphen_points_in(stressed, &Limits { min_word: 11, ..Limits::default() }, Lang::Russian).is_empty(),
            "stress marks do not count as letters"
        );
        let oversized = format!("т{}ипография", "\u{0301}".repeat(MAX_RUSSIAN_GRAPHEME_CHARS));
        assert!(hyphen_points_in(&oversized, &Limits::default(), Lang::Russian).is_empty());
    }
}
