//! Khmer lexical boundaries from the shared Aksara-derived core. The composer owns shaped
//! cluster safety and justification. The attached-glyph protection adapts ideas from
//! @visarutforthaipbs's DesignCraft PR #516:
//! https://github.com/storytold/designcraft/pull/516. Khmer lexical decisions use craft-khmer,
//! as in WordCraft, rather than the ICU4X segmenters proposed in that contribution.
//!
//! A boundary only permits a line break: it has no width, so justification never opens a gap
//! there. As is standard in page layout, a line fills through its real word spaces and the
//! paragraph's glyph scaling; a line without a real space keeps its natural width until the author adds one.
use std::{
    collections::HashMap,
    ops::Range,
    sync::{Arc, Mutex, OnceLock},
};

use crate::shape::Glyph;
use unicode_segmentation::UnicodeSegmentation;

pub(crate) fn character(c: char) -> bool {
    matches!(c, '\u{1780}'..='\u{17ff}' | '\u{19e0}'..='\u{19ff}')
}
fn mark(c: char) -> bool {
    matches!(c, '\u{17b4}'..='\u{17d3}' | '\u{17dd}' | '\u{200c}' | '\u{200d}')
}
fn closing(c: char) -> bool {
    matches!(c, 'ៗ' | '។' | '៕' | '៖' | '៘' | '៚' | ',' | '.' | ':' | ';' | '!' | '?' | ')' | ']' | '}' | '»' | '”')
}

#[derive(Default)]
struct Cache {
    values: HashMap<Box<str>, Arc<[usize]>>,
    bytes: usize,
}
// A bounded cache avoids recomputing unchanged paragraphs during whole-story recomposition.
// Scoring happens outside the lock; oversized inputs are computed but never retained. Recomposing
// walks paragraphs in order, so a book past this limit would clear each entry before its next use.
fn boundaries(text: &str) -> Arc<[usize]> {
    const MAX_BYTES: usize = 64 << 20;
    static CACHE: OnceLock<Mutex<Cache>> = OnceLock::new();
    let cache = CACHE.get_or_init(Default::default);
    if let Some(found) = cache.lock().unwrap_or_else(std::sync::PoisonError::into_inner).values.get(text) {
        return found.clone();
    }
    let breaks: Arc<[usize]> = if craft_khmer::data_available() {
        craft_khmer::line_breaks(text).into()
    } else {
        text.char_indices().filter_map(|(i, c)| (c == '\u{200b}').then_some(i + c.len_utf8())).collect()
    };
    let size = text.len().saturating_add(std::mem::size_of_val(breaks.as_ref()));
    if size <= MAX_BYTES {
        let mut cache = cache.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
        if cache.bytes.saturating_add(size) > MAX_BYTES {
            cache.values.clear();
            cache.bytes = 0;
        }
        if !cache.values.contains_key(text) {
            cache.values.insert(text.into(), breaks.clone());
            cache.bytes += size;
        }
    }
    breaks
}

/// Tokens separated by whitespace that hold a letter or digit. Detached marks, repetition signs
/// and punctuation are not words. Unicode counts Khmer vowel signs as alphabetic.
fn spaced_words(text: &str) -> usize {
    let word_char = |c: char| if character(c) { ('\u{1780}'..='\u{17b3}').contains(&c) || c.is_numeric() } else { c.is_alphanumeric() };
    text.split_whitespace().filter(|token| token.chars().any(word_char)).count()
}

fn segmented_words(text: &str) -> usize {
    let mut words = 0usize;
    let mut end = 0;
    for range in craft_khmer::word_ranges(text) {
        words = words.saturating_add(text.get(end..range.start).map_or(0, spaced_words));
        let letter = text.get(range.clone()).is_some_and(|word| word.chars().any(|c| ('\u{1780}'..='\u{17b3}').contains(&c)));
        words = words.saturating_add(usize::from(letter));
        end = range.end;
    }
    words.saturating_add(text.get(end..).map_or(0, spaced_words))
}

/// Words in one paragraph holding Khmer, by the same rules as WordCraft: word spans from the shared
/// model, whitespace tokens for other text. Counts recur on every frame of the Story Editor, so
/// paragraphs are cached by their text; each gets its own refinement budget. A count walks them
/// in order, so the bound sits far above real books: past it, entries clear before their next use.
fn paragraph_words(text: &str) -> usize {
    const MAX_TEXT: usize = 64 << 20;
    const MAX_PARAGRAPH: usize = 64 << 10;
    #[derive(Default)]
    struct Counts {
        words: HashMap<Box<str>, usize>,
        bytes: usize,
    }
    static COUNTS: OnceLock<Mutex<Counts>> = OnceLock::new();
    let counts = COUNTS.get_or_init(Default::default);
    if let Some(words) = counts.lock().unwrap_or_else(std::sync::PoisonError::into_inner).words.get(text).copied() {
        return words;
    }
    let words = segmented_words(text);
    if text.len() <= MAX_PARAGRAPH {
        let mut counts = counts.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
        if counts.bytes.saturating_add(text.len()) > MAX_TEXT {
            counts.words.clear();
            counts.bytes = 0;
        }
        if !counts.words.contains_key(text) {
            counts.words.insert(text.into(), words);
            counts.bytes = counts.bytes.saturating_add(text.len());
        }
    }
    words
}

/// Words in `text` for document statistics: Khmer counts by its word boundaries, not by the real
/// spaces that justification uses; other text counts by whitespace. Without the data, authored
/// zero-width spaces still separate Khmer words.
pub fn word_count(text: &str) -> usize {
    if !text.chars().any(character) {
        return text.split_whitespace().count();
    }
    if !craft_khmer::data_available() {
        return text.split('\u{200b}').map(spaced_words).fold(0, usize::saturating_add);
    }
    text.split(['\n', '\r'])
        .map(|paragraph| if paragraph.chars().any(character) { paragraph_words(paragraph) } else { spaced_words(paragraph) })
        .fold(0, usize::saturating_add)
}

/// Letter spacing never reaches Khmer letters or the glyphs of a shaped cluster, including across
/// an authored control between another script and Khmer. Other text is not affected.
pub(crate) fn letter_gap(line: &[Glyph], i: usize) -> bool {
    let khmer_at = |j: usize| line.get(j).is_some_and(|g| character(g.ch));
    let (Some(_), Some(next)) = (line.get(i), line.get(i + 1)) else { return false };
    !khmer_at(i) && !khmer_at(i + 1) && !(matches!(next.ch, '\u{200b}' | '\u{2060}') && khmer_at(i + 2))
}
/// How the breaker rates slack on a justified Khmer line: lines without a real space stay short,
/// so a short line costs in proportion to its text rather than being impossible to rate. This is
/// never applied to a glyph's width.
pub(crate) fn slack_unit(g: &Glyph) -> f64 {
    if character(g.ch) { g.adv * 0.05 } else { 0.0 }
}

/// Model and authored opportunities must also be source-grapheme and shaped-cluster edges.
/// Hard exclusions remain separate from dictionary decisions, allowing cluster-safe emergency
/// wrapping of overlong unknown words without overriding WJ, punctuation or No Break.
pub(crate) fn apply(glyphs: &mut [Glyph], text: &str, range: Range<usize>) {
    let Some(span) = text.get(range.clone()) else { return };
    if !span.chars().any(character) {
        return;
    }
    let breaks = boundaries(span);
    let clusters: Vec<_> = span.grapheme_indices(true).map(|(i, _)| i).collect();
    for i in 0..glyphs.len().saturating_sub(1) {
        let (left, right) = glyphs.split_at_mut(i + 1);
        let (Some(g), Some(next)) = (left.last_mut(), right.first()) else { continue };
        // An authored control has no width, so letter spacing has nothing to work with.
        if matches!(g.ch, '\u{200b}' | '\u{2060}') {
            g.space = 0.0;
        }
        if next.byte < range.start || next.byte >= range.end || g.list_label || next.list_label {
            continue;
        }
        if next.byte == g.byte {
            if character(g.ch) || character(next.ch) {
                g.break_after = Some(false);
            }
            continue;
        }
        let at = next.byte - range.start;
        let before = span.get(..at).and_then(|s| s.chars().next_back());
        let after = span.get(at..).and_then(|s| s.chars().next());
        let (Some(before), Some(after)) = (before, after) else { continue };
        let khmer_edge =
            character(before) || character(after) || matches!(before, '\u{200b}' | '\u{2060}') || matches!(after, '\u{200b}' | '\u{2060}');
        if !khmer_edge {
            continue;
        }
        let forbidden = clusters.binary_search(&at).is_err()
            || matches!(before, '\u{17d2}' | '័')
            || before == '\u{2060}'
            || after == '\u{2060}'
            || after == '\u{200b}'
            || mark(after)
            || closing(after)
            || matches!(before, '(' | '[' | '{' | '«' | '“');
        if forbidden {
            g.break_after = Some(false);
        } else if g.break_after != Some(false) && !g.no_break && !next.no_break && !g.locked_advance && breaks.binary_search(&at).is_ok() {
            g.break_after = Some(true);
        }
    }
}
