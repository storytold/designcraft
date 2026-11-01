//! Hyphenation. English uses the bundled dictionary and patterns below. Other languages use the
//! Liang patterns compiled into `hypher`, one cargo feature per language: Spanish is Javier Bezos's
//! `hyph-es.tex` 5.0 from hyph-utf8, MIT; Portuguese is `hyph-pt.tex` 1.4 from hyph-utf8 (Pedro J.
//! de Rezende, J. Joao Dias Almeida, Leonardo Araujo, Aline Benevides), BSD-3-Clause; Norwegian is
//! `hyph-no.tex` from hyph-utf8 (Rune Kleveland, Ole Michael Selberg), permissive (see NOTICE);
//! Greek is `hyph-el-monoton.tex` 5.0 from hyph-utf8 (Dimitrios Filippou), LPPL or MIT, taken
//! under MIT. Those are the **monotonic** patterns, which is what `hypher` maps `el` to, so
//! polytonic text hyphenates by them as well. French is `hyph-fr.tex` 2.13 from hyph-utf8 (Daniel
//! Flipo, Bernard Gaulle, Arthur Reutenauer), MIT; Italian is `hyph-it.tex` 4.9 from hyph-utf8
//! (Claudio Beccari), LPPL or MIT, taken under MIT. A word with an elision (`l'imprimerie`)
//! hyphenates after its apostrophe.
//! German uses `hyph-de-1996.tex` (2024-02-28, Deutschsprachige Trennmustermannschaft, MIT):
//! despite the filename, these are the reformed 2006 spelling patterns. Only the two 2006 reform
//! names and German locale codes without a reform year or with 2006 use this set. Traditional,
//! 1901 and unknown German variants have no automatic hyphenation; no old-spelling set is bundled.
//! Serbian is `hyph-sh-cyrl.tex` 2.02 from hyph-utf8 (Dejan Muhamedagić), LPPL 1 or later; the
//! patterns are for Cyrillic, so Serbian words in Latin script are hyphenated through their
//! Cyrillic spelling ([`serbian_cyrillic`]), which also shows where `nj`, `lj` and `dž` are two
//! letters ([`designcraft_doc::serbian::two_letters`]); such a word takes this route even past an installed Latin
//! pattern set, whose patterns can't tell.
//!
//! Pattern files can also be **installed** at run time ([`install_patterns`]): a
//! LibreOffice/Hyphen `hyph_xx_XX.dic` for a language comes before the bundled patterns, which
//! still cover the words its alphabet doesn't (a Latin-script set and a Cyrillic one can be
//! installed for the same language).
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

use designcraft_doc::serbian::two_letters;
use std::sync::{OnceLock, PoisonError, RwLock};

pub use dict::Dictionary;
pub use patterns::Patterns;

static EN_US_DIC: &[u8] = include_bytes!("../../../assets/hyphenation/en-us.dic");
static EN_US_PAT: &str = include_str!("../../../assets/hyphenation/en-us.pat");

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
    /// Pattern files installed for a language ([`install_patterns`]), by index.
    Installed(u16),
}

impl Lang {
    /// The hyphenator for a language name or locale code (see [`designcraft_doc::language_tag`]).
    /// None when the language has neither installed nor bundled hyphenation.
    pub fn for_language(language: &str) -> Option<Lang> {
        if language.starts_with("English") {
            return Some(Lang::English);
        }
        let sub = pattern_language(language)?;
        // language_tag deliberately drops spelling variants for shaping and quotes. Hyphenation
        // must retain that distinction: the 2006 patterns are wrong for traditional ck/st breaks.
        if sub == "de" && !german_2006_language(language) {
            return None;
        }
        if sub == "en" {
            return Some(Lang::English);
        }
        let installed = INSTALLED.read().unwrap_or_else(PoisonError::into_inner);
        if let Some(i) = installed.iter().position(|l| l.sub == sub) {
            return u16::try_from(i).ok().map(Lang::Installed);
        }
        bundled(sub).map(Lang::Patterns)
    }
}

/// Accept explicit reform names and unqualified German locales (our modern default), but never
/// silently substitute reform patterns for another spelling variant imported from IDML.
fn german_2006_language(language: &str) -> bool {
    let language = language.trim();
    let normalized = language.strip_prefix("$ID/").unwrap_or(language).to_lowercase();
    let words: Vec<&str> = normalized.split(|c: char| !c.is_alphanumeric()).filter(|w| !w.is_empty()).collect();
    matches!(words.as_slice(), ["german", "2006", "reform"] | ["german", "swiss", "2006", "reform"] | ["de"] | ["de", _] | ["de", _, "2006"])
}

/// The language subtag patterns are kept under: `sh` (Serbo-Croatian, the code Hunspell's Serbian
/// Latin files use) is Serbian.
fn pattern_language(language: &str) -> Option<&'static str> {
    let sub = designcraft_doc::language_subtag(designcraft_doc::language_tag(language)?);
    Some(if sub == "sh" { "sr" } else { sub })
}

/// The patterns compiled into `hypher` for a language subtag.
fn bundled(sub: &str) -> Option<hypher::Lang> {
    match *sub.as_bytes() {
        [a, b] => hypher::Lang::from_iso([a, b]),
        _ => None,
    }
}

/// Pattern sets installed for one language.
struct Installed {
    sub: &'static str,
    sets: Vec<Patterns>,
}

static INSTALLED: RwLock<Vec<Installed>> = RwLock::new(Vec::new());

/// At most this many installed pattern files per language, and languages.
const MAX_SETS: usize = 8;
const MAX_LANGUAGES: usize = 256;

/// Install a LibreOffice/Hyphen pattern file (see [`Patterns::parse_hyphen_dic`]) for `language`,
/// a locale code such as `sr_RS` (from the file name `hyph_sr_RS.dic`) or a language name. Its
/// patterns then come first for text in that language. Install before composing: hyphenated
/// words are cached. Returns how many patterns were read and how many skipped. English is
/// refused: it always uses the bundled dictionary ([`Lang::for_language`]).
pub fn install_patterns(language: &str, file: impl AsRef<[u8]>) -> Result<(usize, usize), String> {
    let sub = pattern_language(language).ok_or_else(|| format!("{language}: not a language"))?;
    if sub == "en" {
        return Err(format!("{language}: English uses the bundled dictionary"));
    }
    let (pats, skipped) =
        Patterns::parse_hyphen_dic(file).ok_or_else(|| format!("{language}: not a pattern file in UTF-8, ISO8859-2 or ISO8859-5"))?;
    if pats.is_empty() {
        return Err(format!("{language}: no patterns"));
    }
    let n = pats.len();
    let mut installed = INSTALLED.write().unwrap_or_else(PoisonError::into_inner);
    let i = match installed.iter().position(|l| l.sub == sub) {
        Some(i) => i,
        None if installed.len() < MAX_LANGUAGES => {
            installed.push(Installed { sub, sets: Vec::new() });
            installed.len() - 1
        }
        None => return Err(format!("{language}: too many languages installed")),
    };
    let sets = &mut installed.get_mut(i).ok_or_else(|| format!("{language}: not installed"))?.sets;
    if sets.len() >= MAX_SETS {
        return Err(format!("{language}: too many pattern files"));
    }
    sets.push(pats);
    Ok((n, skipped))
}

/// Break points from the first pattern set installed under `i` whose alphabet covers `core`.
fn installed_points(i: u16, core: &[char]) -> Option<Vec<usize>> {
    let installed = INSTALLED.read().unwrap_or_else(PoisonError::into_inner);
    installed.get(usize::from(i))?.sets.iter().find_map(|p| p.points(core))
}

/// The bundled patterns behind installed language `i`.
fn installed_bundled(i: u16) -> Option<hypher::Lang> {
    let installed = INSTALLED.read().unwrap_or_else(PoisonError::into_inner);
    bundled(installed.get(usize::from(i))?.sub)
}

/// Compounds the patterns don't see, in Cyrillic: `|` is where the prefix ends. Pravopis srpskoga
/// jezika, §186е: a transparent compound breaks at the join.
const COMPOUNDS: &[&str] = &["над|индивид", "над|истор", "под|одбор", "под|одељ", "под|одјељ", "под|оцењ", "под|оцен", "пред|инфаркт"];

/// Prefixes that keep their join before a vowel.
const PREFIXES: &[&str] = &["без", "из", "над", "об", "обез", "од", "под", "пред", "раз"];

/// Is `lang` Serbian?
fn is_serbian(lang: Lang) -> bool {
    match lang {
        Lang::Patterns(l) => l == hypher::Lang::Serbian,
        Lang::Installed(i) => installed_bundled(i) == Some(hypher::Lang::Serbian),
        _ => false,
    }
}

/// `pts` of a lowercase Serbian `core`, with the join of a compound as §186е has it: `пред-услов`,
/// never `преду-слов`, though the patterns allow both. A vowel alone between two breaks after a
/// prefix belongs to the second word, so the break after it goes.
fn serbian_compound_breaks(core: &[char], pts: Vec<usize>) -> Vec<usize> {
    let latin = core.iter().any(|c| c.is_ascii_alphabetic() || "čćđšž".contains(*c));
    let (word, at) = if latin {
        match serbian_cyrillic(core) {
            Some(x) => x,
            None => return pts,
        }
    } else {
        (core.to_vec(), (0..core.len()).collect())
    };
    let mut marks: Vec<usize> = pts.iter().filter_map(|p| at.iter().position(|a| a == p)).collect();
    let starts = |s: &str, from: usize| s.chars().enumerate().all(|(i, c)| word.get(from + i) == Some(&c));
    for root in COMPOUNDS {
        let (prefix, rest) = root.split_once('|').unwrap_or((root, ""));
        let n = prefix.chars().count();
        if starts(prefix, 0) && starts(rest, n) {
            marks.retain(|&p| p > n);
            marks.push(n);
        }
    }
    for prefix in PREFIXES {
        let n = prefix.chars().count();
        if starts(prefix, 0) && word.get(n).is_some_and(|c| "аеиоу".contains(*c)) && marks.contains(&n) {
            marks.retain(|&p| p != n + 1);
        }
    }
    marks.sort_unstable();
    marks.dedup();
    marks.into_iter().filter_map(|p| at.get(p).copied()).collect()
}

/// A lowercase Serbian Latin word in Cyrillic, letter for letter (`lj`, `nj`, `dž` are one
/// letter each: љ, њ, џ, unless [`two_letters`] says otherwise), with the Latin index each
/// Cyrillic letter starts at. None when the word has a letter outside the Serbian Latin alphabet.
fn serbian_cyrillic(word: &[char]) -> Option<(Vec<char>, Vec<usize>)> {
    let pairs = two_letters(word);
    let mut out = Vec::with_capacity(word.len());
    let mut at = Vec::with_capacity(word.len());
    let mut i = 0;
    while let Some(&c) = word.get(i) {
        let next = word.get(i + 1).copied();
        let one_letter = !pairs.contains(&(i + 1));
        let (cyr, len) = match (c, next) {
            ('l', Some('j')) if one_letter => ('љ', 2),
            ('n', Some('j')) if one_letter => ('њ', 2),
            ('d', Some('ž')) if one_letter => ('џ', 2),
            _ => (
                match c {
                    'a' => 'а',
                    'b' => 'б',
                    'c' => 'ц',
                    'č' => 'ч',
                    'ć' => 'ћ',
                    'd' => 'д',
                    'đ' => 'ђ',
                    'e' => 'е',
                    'f' => 'ф',
                    'g' => 'г',
                    'h' => 'х',
                    'i' => 'и',
                    'j' => 'ј',
                    'k' => 'к',
                    'l' => 'л',
                    'm' => 'м',
                    'n' => 'н',
                    'o' => 'о',
                    'p' => 'п',
                    'r' => 'р',
                    's' => 'с',
                    'š' => 'ш',
                    't' => 'т',
                    'u' => 'у',
                    'v' => 'в',
                    'z' => 'з',
                    'ž' => 'ж',
                    _ => return None,
                },
                1,
            ),
        };
        out.push(cyr);
        at.push(i);
        i += len;
    }
    Some((out, at))
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

/// Break points of a Serbian Latin `core` through its Cyrillic spelling, in `lang`'s patterns.
fn serbian_latin_breaks(core: &[char], lang: Lang) -> (Vec<usize>, Source) {
    let Some((cyr, at)) = serbian_cyrillic(core) else { return (vec![], Source::None) };
    let (pts, src) = core_breaks(&cyr, lang);
    (pts.into_iter().filter_map(|p| at.get(p).copied()).collect(), src)
}

/// Unrestricted break points of one lowercase core (letters and `'` only, no leading/trailing `'`).
fn core_breaks(core: &[char], lang: Lang) -> (Vec<usize>, Source) {
    let n = core.len();
    if n < 2 {
        return (vec![], Source::None);
    }
    let lang = match lang {
        Lang::Installed(i) => {
            if installed_bundled(i) == Some(hypher::Lang::Serbian) && !two_letters(core).is_empty() {
                return serbian_latin_breaks(core, lang);
            }
            if let Some(p) = installed_points(i, core) {
                return (p, Source::Patterns);
            }
            match installed_bundled(i) {
                Some(l) => Lang::Patterns(l),
                None => return (vec![], Source::None),
            }
        }
        _ => lang,
    };
    if let Lang::Patterns(l) = lang {
        if let Some(ap) = core.iter().rposition(|&c| c == '\'') {
            // An elision (French "l'", Italian "dell'"): the word after the apostrophe hyphenates,
            // with no break up to it. Elsewhere (German "Rock'n'Roll") an apostrophe word gets none.
            if !matches!(l, hypher::Lang::French | hypher::Lang::Italian) {
                return (vec![], Source::None);
            }
            let (pts, src) = if ap + 1 < n { core_breaks(&core[ap + 1..], lang) } else { (vec![], Source::None) };
            return (pts.into_iter().map(|p| p + ap + 1).collect(), src);
        }
        // Serbian Latin through its Cyrillic spelling, the script of the bundled patterns.
        if l == hypher::Lang::Serbian && core.iter().any(|c| c.is_ascii_alphabetic() || "čćđšž".contains(*c)) {
            return serbian_latin_breaks(core, lang);
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
        let (mut pts, s) = core_breaks(&core, lang);
        if is_serbian(lang) {
            pts = serbian_compound_breaks(&core, pts);
        }
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
    let elides = matches!(lang, Lang::Patterns(hypher::Lang::French | hypher::Lang::Italian));
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
        // After an elision the word is the part after its last apostrophe: the limits count that.
        if elides && let Some(ap) = chars[a..b].iter().rposition(|&c| is_apostrophe(c)) {
            a += ap + 1;
        }
        (a, b)
    };
    pts.into_iter()
        .filter(|&p| {
            let (a, b) = run_of(p);
            let run = &chars[a..b];
            // Serbian `lj`, `nj` and `dž` are one letter each (Pravopis, §186г).
            let digraphs = is_serbian(lang).then(|| {
                let core: Vec<char> = run.iter().map(|c| c.to_lowercase().next().unwrap_or(*c)).collect();
                serbian_cyrillic(&core).map(|(_, at)| at)
            });
            let digraphs = digraphs.flatten();
            let letters = digraphs.as_ref().map_or_else(|| run.iter().filter(|c| c.is_alphabetic()).count(), Vec::len);
            if letters < lim.min_word.max(2) {
                return false;
            }
            // Words in capitals (FERRUM COMMODI) hyphenate like capitalized words, as InDesign's: Hyphenate
            // Capitalized Words off keeps both, acronyms included, whole.
            if !lim.capitalized && run[0].is_uppercase() {
                return false;
            }
            let before = digraphs
                .as_ref()
                .map_or_else(|| chars[a..p].iter().filter(|c| c.is_alphabetic()).count(), |at| at.iter().filter(|&&i| i < p - a).count());
            let after = letters - before;
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

    /// Only French and Italian elide at an apostrophe; other languages leave such words unbroken.
    #[test]
    fn apostrophe_words_hyphenate_only_in_french_and_italian() {
        let de = Lang::for_language("German: 2006 Reform").unwrap();
        assert!(hyphen_points_in("Rock'n'Roll", &Limits::default(), de).is_empty());
        let fr = Lang::for_language("French").unwrap();
        assert!(!hyphen_points_in("l'information", &Limits::default(), fr).is_empty());
    }

    #[test]
    fn german_2006_uses_reformed_patterns() {
        let de = Lang::for_language("German: 2006 Reform").unwrap();
        assert_eq!(de, Lang::Patterns(hypher::Lang::German));
        for name in [
            "$ID/German: 2006 Reform",
            " german: 2006 reform ",
            "de",
            "de-DE",
            "de_AT",
            "de_CH",
            "de_DE_2006",
            "German: Swiss 2006 Reform",
            "$ID/German: Swiss 2006 Reform",
            "de_CH_2006",
        ] {
            assert_eq!(Lang::for_language(name), Some(de), "{name}");
        }
        for (word, expected) in [
            ("Silbentrennung", "Sil-ben-tren-nung"),
            ("Zucker", "Zu-cker"),
            ("Bäcker", "Bä-cker"),
            ("Fenster", "Fens-ter"),
            ("Straße", "Stra-ße"),
            ("Fußgänger", "Fuß-gän-ger"),
            ("Donaudampfschifffahrt", "Do-nau-dampf-schiff-fahrt"),
            ("SILBENTRENNUNG", "SIL-BEN-TREN-NUNG"),
        ] {
            let pts = hyphen_points_in(word, &Limits::default(), de);
            let actual: String = word.chars().enumerate().map(|(i, c)| if pts.contains(&i) { format!("-{c}") } else { c.to_string() }).collect();
            assert_eq!(actual, expected, "{word}");
        }
    }

    #[test]
    fn non_2006_german_has_no_bundled_hyphenation() {
        for name in [
            "German",
            "German: Traditional",
            "$ID/German: Traditional",
            "German: Swiss",
            "German: 1901",
            "German: 1996 Reform",
            "German: Unknown",
            "German: Traditional 2006 Reform",
            "de_DE_1901",
            "de-CH-1901",
            "de_AT_1996",
            "de_DE_2099",
        ] {
            assert_eq!(designcraft_doc::language_subtag(designcraft_doc::language_tag(name).unwrap()), "de", "{name}");
            assert_eq!(Lang::for_language(name), None, "{name}: do not use 2006 ck/st breaks for unsupported spelling variants");
        }
    }

    #[test]
    fn german_2006_respects_paragraph_limits() {
        let de = Lang::for_language("de_DE_2006").unwrap();
        let lim = Limits { after_first: 4, before_last: 5, ..Limits::default() };
        assert_eq!(hyphen_points_in("Silbentrennung", &lim, de), vec![6]);
        let lim = Limits { min_word: 15, ..Limits::default() };
        assert!(hyphen_points_in("Silbentrennung", &lim, de).is_empty());
        let lim = Limits { capitalized: false, ..Limits::default() };
        assert!(hyphen_points_in("Silbentrennung", &lim, de).is_empty());
        assert!(!hyphen_points_in("silbentrennung", &lim, de).is_empty());
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
        assert!(Lang::for_language("Swedish").is_none(), "only languages built in");
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

    /// The paragraph limits count the letters of the word after an elision, not the elided prefix.
    #[test]
    fn elision_limits_apply_to_the_word_after_it() {
        let fr = Lang::for_language("French").unwrap();
        let lim = Limits { min_word: 5, after_first: 3, before_last: 2, capitalized: true };
        assert_eq!(hyphen_points_in("l'imprimerie", &lim, fr), vec![7, 9]);
        let lim = Limits { min_word: 11, ..Limits::default() };
        assert!(hyphen_points_in("l'imprimerie", &lim, fr).is_empty());
    }

    /// A word with an elision ("l'", "dell'") hyphenates as the word after it.
    fn marked(w: &str, l: Lang) -> String {
        let pts = hyphen_points_in(w, &Limits::default(), l);
        w.chars().enumerate().map(|(i, c)| if pts.contains(&i) { format!("-{c}") } else { c.to_string() }).collect()
    }

    #[test]
    fn french_uses_the_hyph_fr_patterns() {
        let fr = Lang::for_language("French").unwrap();
        for name in ["fr", "fr_FR", "French: Canadian", "French: Swiss"] {
            assert_eq!(Lang::for_language(name), Some(fr), "{name}");
        }
        assert_eq!(marked("hyphénation", fr), "hy-phé-na-tion");
        assert_eq!(marked("typographie", fr), "ty-po-gra-phie");
        assert_eq!(marked("Développement", fr), "Dé-ve-lop-pe-ment");
        assert_eq!(marked("anticonstitutionnellement", fr), "an-ti-cons-ti-tu-tion-nel-le-ment");
        assert_eq!(marked("paragraphe", fr), "pa-ra-graphe", "no break before a final mute syllable of two letters");
        assert_eq!(marked("l'imprimerie", fr), "l'im-pri-me-rie", "after an elision");
        assert_eq!(marked("l’imprimerie", fr), "l’im-pri-me-rie", "a typographer's apostrophe too");
    }

    #[test]
    fn italian_uses_the_hyph_it_patterns() {
        let it = Lang::for_language("Italian").unwrap();
        for name in ["it", "it_IT"] {
            assert_eq!(Lang::for_language(name), Some(it), "{name}");
        }
        assert_ne!(it, Lang::for_language("French").unwrap());
        assert_eq!(marked("sillabazione", it), "sil-la-ba-zio-ne");
        assert_eq!(marked("tipografia", it), "ti-po-gra-fia");
        assert_eq!(marked("Repubblica", it), "Re-pub-bli-ca");
        assert_eq!(marked("impaginazione", it), "im-pa-gi-na-zio-ne");
        assert_eq!(marked("dell'impaginazione", it), "dell'im-pa-gi-na-zio-ne", "after an elision");
    }

    #[test]
    fn serbian_uses_the_hyph_sh_patterns_in_both_scripts() {
        let sr = Lang::for_language("Serbian: Cyrillic").unwrap();
        for name in ["Serbian", "Serbian: Latin", "sr", "sr_RS", "sh_RS", "sr-Latn"] {
            assert_eq!(Lang::for_language(name), Some(sr), "{name}");
        }
        let h = |w: &str| {
            let pts = hyphen_points_in(w, &Limits::default(), sr);
            w.chars().enumerate().map(|(i, c)| if pts.contains(&i) { format!("-{c}") } else { c.to_string() }).collect::<String>()
        };
        assert_eq!(h("универзитет"), "уни-вер-зи-тет");
        assert_eq!(h("рачунар"), "ра-чу-нар");
        assert_eq!(h("Београду"), "Бео-гра-ду");
        // Latin hyphenates as its Cyrillic spelling, lj, nj and dž kept whole.
        assert_eq!(h("univerzitet"), "uni-ver-zi-tet");
        assert_eq!(h("računar"), "ra-ču-nar");
        assert_eq!(h("Beogradu"), "Beo-gra-du");
        assert_eq!(h("ljubavlju"), "lju-ba-vlju", "a consonant before a sonorant goes with it");
        assert_eq!(h("љубављу"), "љу-ба-вљу");
        assert_eq!(h("odgovornošću"), "od-go-vor-no-šću");
        // Both scripts break at the same letters.
        for (lat, cyr) in [("džungla", "џунгла"), ("njegovanje", "његовање"), ("đačkoj", "ђачкој"), ("Srbije", "Србије")]
        {
            let (l, c) = (h(lat), h(cyr));
            assert_eq!(
                serbian_cyrillic(&l.to_lowercase().replace('-', "").chars().collect::<Vec<_>>()).map(|x| x.0.iter().collect::<String>()),
                Some(cyr.to_lowercase())
            );
            assert_eq!(l.matches('-').count(), c.matches('-').count(), "{l} / {c}");
        }
        assert_eq!(h("xerox"), "xerox", "a letter outside the alphabet: no breaks");
    }

    /// `nj` and `dž` are two letters here: Latin breaks as Cyrillic does, between them.
    #[test]
    fn serbian_latin_breaks_a_pair_of_two_letters() {
        let lat = Lang::for_language("Serbian: Latin").unwrap();
        for (word, broken) in [
            ("nadživeti", "nad-ži-ve-ti"),
            ("injekcija", "in-jek-ci-ja"),
            ("konjunkcija", "kon-junk-ci-ja"),
            ("konjugacija", "kon-ju-ga-ci-ja"),
            ("vanjezički", "van-je-zič-ki"),
            ("panjugoslovenski", "pan-ju-go-slo-ven-ski"),
            ("podžanr", "pod-žanr"),
            ("Tanjug", "Tan-jug"),
            ("konjektura", "kon-jek-tu-ra"),
            ("injektor", "in-jek-tor"),
            ("odžaliti", "od-ža-li-ti"),
            ("podžupan", "pod-žu-pan"),
            ("nadživljavati", "nad-ži-vlja-va-ti"),
            ("predživot", "pred-ži-vot"),
            ("predžetveni", "pred-že-tve-ni"),
            ("podželudačni", "pod-že-lu-dač-ni"),
            ("podžariti", "pod-ža-ri-ti"),
            ("vanjugoslovenski", "van-ju-go-slo-ven-ski"),
            ("injunktiv", "in-junk-tiv"),
            // Inflected and with a prefix.
            ("konjunkcijama", "kon-junk-ci-ja-ma"),
            ("podžanrovi", "pod-žan-ro-vi"),
            ("mikroinjekcija", "mi-kro-in-jek-ci-ja"),
            ("Konjugacija", "Kon-ju-ga-ci-ja"),
            ("PODŽUPAN", "POD-ŽU-PAN"),
            // A real digraph later in the word stays one letter.
            ("nadživljen", "nad-ži-vljen"),
            ("odžaljen", "od-ža-ljen"),
        ] {
            assert_eq!(marked(word, lat), broken);
        }
    }

    /// Words that look like those roots, where the pair is one letter.
    #[test]
    fn serbian_latin_keeps_a_pair_of_one_letter() {
        let lat = Lang::for_language("Serbian: Latin").unwrap();
        for (word, cyrillic) in [
            ("kanjon", "кањон"),
            ("konj", "коњ"),
            ("konjušar", "коњушар"),
            ("tanjur", "тањур"),
            ("odžak", "оџак"),
            ("odžačar", "оџачар"),
            ("odžalan", "оџалан"),
            ("nadžak", "наџак"),
            ("budžet", "буџет"),
        ] {
            let w: Vec<char> = word.chars().collect();
            assert_eq!(serbian_cyrillic(&w).unwrap().0.iter().collect::<String>(), cyrillic);
        }
        assert_eq!(marked("kanjon", lat), "ka-njon");
        assert_eq!(marked("budžet", lat), "bu-džet");
        assert_eq!(marked("nadžak", lat), "na-džak");
        assert_eq!(marked("Njegovan", lat), "Nje-go-van");
        assert_eq!(marked("LJILJANA", lat), "LJI-LJA-NA");
        assert_eq!(marked("Džudista", lat), "Džu-di-sta");
    }

    /// A foreign name may look like a root's start, but no root reaches it.
    #[test]
    fn serbian_latin_leaves_names_alone() {
        let lat = Lang::for_language("Serbian: Latin").unwrap();
        let lim = Limits { min_word: 2, after_first: 1, before_last: 1, capitalized: true };
        let pts = hyphen_points_in("Podži", &lim, lat);
        assert!(!pts.contains(&3), "dž is one letter here: {pts:?}");
    }

    /// `lj`, `nj` and `dž` count as one letter against the limits, so both scripts agree.
    #[test]
    fn serbian_digraphs_count_as_one_letter() {
        let (lat, cyr) = (Lang::for_language("Serbian: Latin").unwrap(), Lang::for_language("Serbian: Cyrillic").unwrap());
        for (latin, cyrillic) in [("njive", "њиве"), ("ljudi", "људи"), ("konja", "коња"), ("Džaci", "Џаци")] {
            assert_eq!(marked(latin, lat), latin);
            assert_eq!(marked(cyrillic, cyr), cyrillic);
        }
        // Two letters still count as two.
        assert_eq!(marked("Njegovan", lat), "Nje-go-van");
        assert_eq!(marked("injekcija", lat), "in-jek-ci-ja");
    }

    /// Pravopis, §186е: a transparent compound breaks at the join, an opaque one by the rules.
    #[test]
    fn serbian_compounds_break_at_the_join() {
        let (lat, cyr) = (Lang::for_language("Serbian: Latin").unwrap(), Lang::for_language("Serbian: Cyrillic").unwrap());
        for (cyrillic, broken, latin) in [
            ("предуслов", "пред-услов", "pred-uslov"),
            ("предизборни", "пред-избор-ни", "pred-izbor-ni"),
            ("предуговор", "пред-уго-вор", "pred-ugo-vor"),
            ("подофицир", "под-офи-цир", "pod-ofi-cir"),
            ("подоцењивати", "под-оце-њи-ва-ти", "pod-oce-nji-va-ti"),
            ("пододбор", "под-од-бор", "pod-od-bor"),
            ("изиграти", "из-игра-ти", "iz-igra-ti"),
            ("разиграти", "раз-игра-ти", "raz-igra-ti"),
        ] {
            assert_eq!(marked(cyrillic, cyr), broken);
            assert_eq!(marked(&latin.replace('-', ""), lat), latin);
        }
        // `по-`, `пре-` and `на-` come before the д here, and these keep their breaks.
        for (word, broken) in [("разорити", "ра-зо-ри-ти"), ("предузети", "пре-ду-зе-ти"), ("подаци", "по-да-ци"), ("надовезати", "на-до-ве-за-ти")]
        {
            assert_eq!(marked(word, cyr), broken);
        }
    }

    /// The examples of Pravopis srpskoga jezika (Matica srpska, 2002), §186: the breaks it gives and
    /// those it forbids, in both scripts.
    #[test]
    fn serbian_follows_the_examples_of_the_pravopis() {
        const GOOD: &str = "ва-љати де-војка о-вако у-са-дити мо-лити са-мица брат-ство брат-ски град-ски голф-ски екав-ски бугар-ски \
                            умрт-вљен пољ-ски трам-вај мач-ји мај-ка тор-ба довољ-но пар-тија из-бацити из-градити из-ломити ис-тући \
                            од-водити под-вући пред-услов пред-школски рас-познати раз-љутити бе-стидан беза-злен уза-стопно раза-гнати \
                            о-тети о-творити ра-зорити у-зети мор-ски ин-стант рот-ква";
        const FORBIDDEN: &str = "исписа-о миса-о чу-о с-купити радо-ст ваљ-ати дев-ојка ов-ако ус-адити мол-ити сам-ица братс-тво бра-тство \
                                 братс-ки бра-тски градс-ки гра-дски голфс-ки го-лфски екавс-ки ека-вски бугарс-ки буга-рски умртвљ-ен \
                                 умр-твљен пољс-ки по-љски трамв-ај тра-мвај ма-чји ма-јка то-рба дово-љно па-ртија и-збацити и-зградити \
                                 и-стући о-дводити преду-слов";
        const WHOLE: &str = "пришт пласт склад склоп грозд власт чврст сласт страст";
        // One vowel may stay at the end of a line, one letter may not go to the next.
        let lim = Limits { min_word: 3, after_first: 1, before_last: 2, capitalized: true };
        let latin = |w: &str| -> String {
            let (from, to) = ("абвгдђежзијклмнопрстћуфхцчш", "abvgdđežzijklmnoprstćufhcčš");
            w.chars()
                .map(|c| match c {
                    'љ' => "lj".to_string(),
                    'њ' => "nj".to_string(),
                    'џ' => "dž".to_string(),
                    c => from.chars().position(|f| f == c).and_then(|i| to.chars().nth(i)).unwrap_or(c).to_string(),
                })
                .collect()
        };
        let check = |language: &str, spell: &dyn Fn(&str) -> String| {
            let lang = Lang::for_language(language).unwrap();
            let points = |w: &str| hyphen_points_in(&w.replace('-', ""), &lim, lang);
            let at = |w: &str| w.split('-').next().unwrap().chars().count();
            for w in GOOD.split_whitespace().map(spell) {
                for (i, _) in w.match_indices('-') {
                    assert!(points(&w).contains(&w[..i].chars().filter(|&c| c != '-').count()), "{w}");
                }
            }
            for w in FORBIDDEN.split_whitespace().map(spell) {
                assert!(!points(&w).contains(&at(&w)), "{w}");
            }
            for w in WHOLE.split_whitespace().map(spell) {
                assert!(points(&w).is_empty(), "{w}");
            }
        };
        check("Serbian: Cyrillic", &|w| w.to_string());
        check("Serbian: Latin", &latin);
        let lat = Lang::for_language("Serbian: Latin").unwrap();
        assert_eq!(marked("voljom", lat), "vo-ljom");
        assert_eq!(marked("hodžama", lat), "ho-dža-ma");
    }

    #[test]
    fn serbian_latin_spells_in_cyrillic() {
        let w: Vec<char> = "džunglanja".chars().collect();
        let (cyr, at) = serbian_cyrillic(&w).unwrap();
        assert_eq!(cyr.iter().collect::<String>(), "џунглања");
        assert_eq!(at, vec![0, 2, 3, 4, 5, 6, 7, 9]);
        assert!(serbian_cyrillic(&['w']).is_none());
    }

    #[test]
    fn installed_patterns_come_first() {
        // A language no other test uses: installing is process-wide.
        assert!(Lang::for_language("tlh").is_none());
        assert!(install_patterns("tlh", "KOI8-R\n1a\n").is_err());
        assert!(install_patterns("tlh", "UTF-8\nLEFTHYPHENMIN 2\n").is_err(), "no patterns");
        assert!(install_patterns("en_US", "UTF-8\n1ba\n").is_err(), "English keeps the bundled dictionary");
        assert_eq!(install_patterns("tlh", "UTF-8\nLEFTHYPHENMIN 2\n1ba\n1ga\n").unwrap(), (2, 0));
        assert_eq!(install_patterns("tlh", "UTF-8\n1бa\n").unwrap(), (1, 0));
        let tlh = Lang::for_language("tlh").unwrap();
        assert!(matches!(tlh, Lang::Installed(_)));
        assert_eq!(hyphen_points_in("bagaga", &Limits::default(), tlh), vec![2, 4]);
        // Neither set's alphabet covers the word and nothing is bundled.
        assert!(hyphen_points_in("qqqqqq", &Limits::default(), tlh).is_empty());
        for _ in 0..MAX_SETS {
            let _ = install_patterns("tlh", "UTF-8\n1ba\n");
        }
        assert!(install_patterns("tlh", "UTF-8\n1ba\n").is_err(), "pattern files per language are capped");
    }

    #[test]
    fn heuristic_fallback() {
        let c: Vec<char> = "happen".chars().collect();
        assert_eq!(heuristic_points(&c), vec![3]);
    }
}
