//! Engine adapter for craft-khmer. Repetition and honorific handling adapted from
//! craft-libs crates/proof/src/km.rs (MIT), revision 2322f68f2e24bebb6e89e8333de3bbb7d02d1704.
//! The shared library owns segmentation, dictionaries and preferred-form rules.

use std::{
    collections::HashMap,
    sync::{Mutex, OnceLock, PoisonError},
};

use super::ProofIssue;

pub(super) use designcraft_compose::is_khmer as character;

fn user_word(user: &[String], word: &str) -> bool {
    user.iter().any(|entry| entry == word)
}

/// Aksara skips up to three whitespace-separated proper-name parts after a title.
fn follows_title(text: &str, start: usize) -> bool {
    let Some(mut before) = text.get(..start) else { return false };
    for _ in 0..3 {
        let trimmed = before.trim_end_matches(|c: char| c.is_whitespace() || c == '\u{200b}');
        if trimmed.len() == before.len() {
            return false;
        }
        let at = trimmed.char_indices().rev().find(|(_, c)| c.is_whitespace() || *c == '\u{200b}').map_or(0, |(at, c)| at + c.len_utf8());
        let Some(word) = trimmed.get(at..) else { return false };
        if craft_khmer::is_title(word) {
            return true;
        }
        if !word.chars().all(character) {
            return false;
        }
        before = trimmed.get(..at).unwrap_or("");
    }
    false
}

/// Tokens separated by whitespace that hold a letter or digit. Detached marks, repetition signs
/// and punctuation are not words.
fn spaced_words(text: &str) -> usize {
    text.split_whitespace().filter(|token| token.chars().any(|c| c.is_alphanumeric() && c != 'ៗ')).count()
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
/// paragraphs are cached by their text, bounded; each gets its own refinement budget.
fn paragraph_words(text: &str) -> usize {
    const MAX_TEXT: usize = 8 << 20;
    const MAX_PARAGRAPH: usize = 64 << 10;
    #[derive(Default)]
    struct Counts {
        words: HashMap<Box<str>, usize>,
        bytes: usize,
    }
    static COUNTS: OnceLock<Mutex<Counts>> = OnceLock::new();
    let counts = COUNTS.get_or_init(Default::default);
    if let Some(words) = counts.lock().unwrap_or_else(PoisonError::into_inner).words.get(text).copied() {
        return words;
    }
    let words = segmented_words(text);
    if text.len() <= MAX_PARAGRAPH {
        let mut counts = counts.lock().unwrap_or_else(PoisonError::into_inner);
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

/// Words in text holding Khmer. Without the data, authored zero-width spaces still separate words.
pub(super) fn words(text: &str) -> usize {
    if !craft_khmer::data_available() {
        return text.split('\u{200b}').map(spaced_words).fold(0, usize::saturating_add);
    }
    text.split(['\n', '\r'])
        .map(|paragraph| if paragraph.chars().any(character) { paragraph_words(paragraph) } else { spaced_words(paragraph) })
        .fold(0, usize::saturating_add)
}

pub(super) fn issues(text: &str, user: &[String], suggestions: bool) -> Vec<ProofIssue> {
    // is_correct() deliberately fails open without data. Do not treat that as a
    // successful check, or run rules against a partially decoded language pack.
    if !craft_khmer::data_available() {
        return Vec::new();
    }
    let mut out: Vec<_> = craft_khmer::grammar_matches(text)
        .into_iter()
        .filter(|(range, _)| text.get(range.clone()).is_some_and(|word| !user_word(user, word)))
        .map(|(range, standard)| ProofIssue {
            range,
            kind: "grammar",
            message: "អក្ខរាវិរុទ្ធស្តង់ដារ",
            language: "km",
            suggestions: if suggestions { vec![standard.clone()] } else { vec![] },
            preferred: Some(standard),
        })
        .collect();
    let preferred_count = out.len();
    for range in craft_khmer::word_ranges(text) {
        let Some(word) = text.get(range.clone()) else { continue };
        let stem = word.trim_end_matches('ៗ');
        // Core phrase findings are ordered and non-overlapping. Do not rescan
        // every earlier spelling error for each word in a long paragraph.
        let preferred = out.get(..preferred_count).is_some_and(|rules| {
            let at = rules.partition_point(|issue| issue.range.end <= range.start);
            rules.get(at).is_some_and(|issue| issue.range.start < range.end)
        });
        // Exact repeated expressions can be dictionary entries in their own right.
        // Otherwise check the stem, preserving LEK TOO and its formatting on edits.
        if !word.chars().any(character)
            || stem.is_empty()
            || user_word(user, word)
            || user_word(user, stem)
            || craft_khmer::is_correct(word)
            || (stem.len() != word.len() && craft_khmer::is_correct(stem))
            || follows_title(text, range.start)
            || preferred
        {
            continue;
        }
        out.push(ProofIssue {
            range: range.start..range.start + stem.len(),
            kind: "spelling",
            message: "អាចមានកំហុសអក្ខរាវិរុទ្ធ",
            language: "km",
            suggestions: if suggestions { craft_khmer::suggest(stem, 6) } else { vec![] },
            preferred: None,
        });
    }
    out
}

#[cfg(test)]
mod tests;
