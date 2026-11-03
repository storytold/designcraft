//! Engine adapter for craft-khmer. Repetition and honorific handling adapted from
//! craft-libs crates/proof/src/km.rs (MIT), revision 2322f68f2e24bebb6e89e8333de3bbb7d02d1704.
//! The shared library owns segmentation, dictionaries and preferred-form rules.

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
