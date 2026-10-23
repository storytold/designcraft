//! Hungarian Huhyphn patterns. Only selected breaks add display letters.
use super::{Limits, Opportunity};
use hyphenation::{Hyphenator, Language, Load, extended::Extended};
use std::sync::OnceLock;
use unicode_normalization::UnicodeNormalization;
use unicode_segmentation::UnicodeSegmentation;
fn dictionary() -> Option<&'static Extended> {
    static D: OnceLock<Option<Extended>> = OnceLock::new();
    D.get_or_init(|| {
        let data: &[u8] = include_bytes!("../../../../assets/hyphenation/hu/hu.extended.bincode");
        match Extended::from_reader(Language::Hungarian, &mut std::io::Cursor::new(data)) {
            Ok(mut d) => {
                d.minima = (1, 1);
                Some(d)
            }
            Err(e) => {
                log::error!("Hungarian hyphenation dictionary: {e}");
                None
            }
        }
    })
    .as_ref()
}
/// Original character offsets, with an insertion before the hyphen if needed.
/// Unsupported compatibility-ligature replacements are conservatively left unbroken.
pub(super) fn opportunities(word: &str, lim: &Limits) -> Vec<Opportunity> {
    let mut out = Vec::new();
    let mut start = None;
    // Keep combining marks and soft hyphens inside their word so normalization
    // and explicit-break precedence remain unchanged. The sentinel flushes the last run.
    for (at, (byte, c)) in word.char_indices().chain(std::iter::once((word.len(), ' '))).enumerate() {
        if super::is_word_char(c) || unicode_normalization::char::is_combining_mark(c) || c == '\u{ad}' {
            start.get_or_insert((byte, at));
        } else if let Some((byte_start, char_start)) = start.take() {
            let Some(run) = word.get(byte_start..byte) else { continue };
            let leading = run.chars().take_while(|&c| super::is_apostrophe(c)).count();
            let core = run.trim_matches(super::is_apostrophe);
            out.extend(word_opportunities(core, lim).into_iter().map(|mut point| {
                point.at += char_start + leading;
                point
            }));
        }
    }
    out
}

fn word_opportunities(word: &str, lim: &Limits) -> Vec<Opportunity> {
    if word.len() > 1024
        || word.chars().filter(|c| c.is_alphabetic()).count() < lim.min_word.max(2)
        || (!lim.capitalized && word.chars().next().is_some_and(char::is_uppercase))
        || word.contains('\u{ad}')
    {
        return Vec::new();
    }
    let Some(dict) = dictionary() else { return Vec::new() };
    let mut normal = String::new();
    let mut boundaries = vec![(0, 0)];
    for (byte, grapheme) in word.grapheme_indices(true) {
        normal.extend(grapheme.nfc().flat_map(char::to_lowercase));
        boundaries.push((normal.len(), byte + grapheme.len()));
    }
    let mut out = Vec::new();
    for (at, sub) in dict.hyphenate(&normal).breaks {
        let Some((_, byte)) = boundaries.iter().find(|(n, _)| *n == at) else { continue };
        let (Some(left), Some(right)) = (word.get(..*byte), word.get(*byte..)) else { continue };
        if left.chars().filter(|c| c.is_alphabetic()).count() < lim.after_first.max(1)
            || right.chars().filter(|c| c.is_alphabetic()).count() < lim.before_last.max(1)
        {
            continue;
        }
        let mut suffix = String::new();
        if let Some(sub) = sub {
            let Some(a) = at.checked_sub(sub.left) else { continue };
            let Some(b) = at.checked_add(sub.right) else { continue };
            let (Some(old_left), Some(old_right), Some(new_left), Some(new_right)) =
                (normal.get(a..at), normal.get(at..b), sub.substitution.get(..sub.breakpoint), sub.substitution.get(sub.breakpoint..))
            else {
                continue;
            };
            let Some(extra) = new_left.strip_prefix(old_left).filter(|_| old_right == new_right) else { continue };
            if extra.len() > 4 || !extra.chars().all(|c| matches!(c, 's' | 'z' | 'y')) {
                continue;
            }
            let upper = left.chars().rev().find(|c| c.is_alphabetic()).is_some_and(char::is_uppercase);
            suffix = if upper { extra.to_uppercase() } else { extra.to_owned() };
        }
        out.push(Opportunity { at: left.chars().count(), suffix: suffix.into_boxed_str() });
    }
    out
}
#[cfg(test)]
mod tests {
    use super::*;
    fn marked(w: &str) -> String {
        let ops = opportunities(w, &Limits { min_word: 2, ..Limits::default() });
        let mut out = String::new();
        for (i, c) in w.chars().enumerate() {
            if let Some(op) = ops.iter().find(|o| o.at == i) {
                out.push_str(&op.suffix);
                out.push('-');
            }
            out.push(c);
        }
        out
    }
    #[test]
    fn hungarian_extended_cases_and_case_preservation() {
        for (w, want) in [
            ("asszony", "asz-szony"),
            ("könnyen", "köny-nyen"),
            ("hosszú", "hosz-szú"),
            ("meggyel", "megy-gyel"),
            ("asszonnyal", "asz-szony-nyal"),
            ("ASSZONY", "ASZ-SZONY"),
            ("KÖNNYEN", "KÖNY-NYEN"),
            ("Asszony", "Asz-szony"),
            ("meggyőző", "meg-győ-ző"),
            ("jegygyűrű", "jegy-gyű-rű"),
            ("masszázszokni", "masz-százs-zok-ni"),
            ("giccsetek", "gics-cse-tek"),
        ] {
            assert_eq!(marked(w), want, "{w}");
        }
    }
    #[test]
    fn hungarian_limits_combining_marks_and_bounded_input() {
        assert_eq!(marked("ko\u{308}nnyen"), "ko\u{308}ny-nyen");
        let lim = Limits { capitalized: false, ..Limits::default() };
        assert!(opportunities("ASSZONY", &lim).is_empty());
        assert!(opportunities("asszony", &Limits { after_first: 3, ..lim }).is_empty());
        assert!(opportunities("asszony", &Limits { min_word: 8, ..lim }).is_empty());
        assert!(opportunities("as\u{ad}szony", &lim).is_empty());
        assert!(opportunities(&"á".repeat(600), &lim).is_empty());
        for w in ["", "a", "ﬂlyé", "İİİİ", "ááááá", "a\u{308}\u{301}sszony"] {
            for p in opportunities(w, &Limits::default()) {
                assert!(p.at > 0 && p.at < w.chars().count());
            }
        }
    }

    #[test]
    fn hungarian_matches_native_hyphen_corpus() {
        let input = include_str!("../../../../tools/hu-language-tests/fixtures/hyphen-native.tsv");
        let mut count = 0;
        for line in input.lines() {
            let (word, expected) = line.split_once('\t').unwrap();
            assert_eq!(marked(word), expected.replace('=', "-"), "{word}");
            count += 1;
        }
        assert_eq!(count, 857);
        eprintln!("{count} words match native Hyphen with the corrected dictionary");
    }
}
