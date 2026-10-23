//! Resumable spelling scan: neither a long story nor a punctuation-only run is
//! tokenized up front. The caller owns an immutable document snapshot.
use super::{Language, language, valid_word};
use designcraft_doc::{Document, Story};
use std::ops::Range;

#[derive(Default)]
pub struct SpellingCursor {
    byte: usize,
    run: usize,
    run_end: usize,
    para: usize,
    language: Option<Language>,
    token: Option<(Range<usize>, Language)>,
    connector: bool,
}

impl SpellingCursor {
    fn flush(&mut self, story: &Story, user: &[String], out: &mut Vec<Range<usize>>) {
        if let Some((range, lang)) = self.token.take()
            && let Some(word) = story.text.get(range.clone())
            && !valid_word(lang, word, user)
        {
            out.push(range);
        }
        self.connector = false;
    }

    /// Process at most `units` characters/run boundaries and `words` completed
    /// tokens. Returns true only at EOF. Reuse only with the same snapshot/story.
    /// A token spanning a budget boundary remains intact, including UTF-8 accents.
    pub fn advance(&mut self, doc: &Document, story: &Story, mut units: usize, mut words: usize, out: &mut Vec<Range<usize>>) -> bool {
        while units > 0 && words > 0 {
            units -= 1;
            if self.byte >= story.text.len() || (self.byte >= self.run_end && self.run >= story.chars.len()) {
                self.flush(story, &doc.user_words, out);
                return true;
            }
            // Visiting even an empty formatting run consumes budget.
            if self.byte >= self.run_end {
                if let Some(run) = story.chars.get(self.run) {
                    self.run_end = self.run_end.saturating_add(run.len);
                    self.run = self.run.saturating_add(1);
                    self.language = None;
                }
                continue;
            }
            let lang = *self.language.get_or_insert_with(|| {
                story.paras.get(self.para).zip(story.chars.get(self.run.saturating_sub(1))).map_or(Language::Unsupported, |(para, run)| {
                    let base = doc.styles.resolve_para(para).1;
                    language(&doc.styles.resolve_char(&base, &run.format).language)
                })
            });
            if self.token.as_ref().is_some_and(|(_, old)| *old != lang) {
                self.flush(story, &doc.user_words, out);
                words -= 1;
                continue;
            }
            let Some(c) = story.text.get(self.byte..).and_then(|s| s.chars().next()) else { return true };
            let end = self.byte.saturating_add(c.len_utf8());
            let hu = lang == Language::Hungarian;
            let letter = lang != Language::Unsupported && (c.is_alphabetic() || (hu && c.is_ascii_digit()));
            let accent = hu && ('\u{0300}'..='\u{036f}').contains(&c);
            let connector = c == '\'' || c == '’' || (hu && c == '-');
            if letter || (accent && self.token.is_some() && !self.connector) {
                if let Some((range, _)) = &mut self.token {
                    range.end = end;
                } else {
                    self.token = Some((self.byte..end, lang));
                }
                self.connector = false;
            } else if connector && self.token.is_some() && !self.connector {
                // Include a connector only after a following letter in the same language.
                self.connector = true;
            } else if self.token.is_some() {
                self.flush(story, &doc.user_words, out);
                words -= 1;
            }
            self.byte = end;
            if c == '\n' {
                self.para = self.para.saturating_add(1);
                self.language = None;
            }
        }
        false
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Session;
    use serde_json::json;

    #[test]
    fn incremental_scan_matches_full_scan_across_every_boundary() {
        let mut session = Session::new();
        session.execute("file.new", &json!({})).unwrap();
        let text = "hejesírás 2026-ban árvi\u{301}z qzxw can't dog’s -- 42-ok\nhello qzxw\nhejesírás";
        let r = session.execute("frame.create", &json!({"rect":[0,0,300,300],"content":"text","text":text})).unwrap();
        let sid = designcraft_doc::StoryId(r["story"].as_u64().unwrap());
        let mut doc = (*session.doc().unwrap().doc).clone();
        doc.user_words.push("qzxw".into());
        let story = doc.story_mut(sid).unwrap();
        story.format_chars(0..text.len(), |f| f.over.language = Some("Hungarian".into()));
        // Same-language formatting must not split a word or its combining accent.
        story.format_chars(2..5, |f| f.over.tracking = Some(10.0));
        let english = text.find("hello").unwrap();
        story.format_chars(english..english + 10, |f| f.over.language = Some("English".into()));
        let story = doc.story(sid).unwrap();
        let expected = super::super::misspellings(&doc, story, &doc.user_words);
        assert!(!expected.is_empty());
        for budget in [1, 2, 7, 256] {
            let mut cursor = SpellingCursor::default();
            let mut actual = Vec::new();
            let mut done = false;
            for _ in 0..text.len() * 3 {
                if cursor.advance(&doc, story, budget, 1, &mut actual) {
                    done = true;
                    break;
                }
            }
            assert!(done);
            assert_eq!(actual, expected, "budget={budget}");
        }
    }

    #[test]
    fn long_tokens_and_punctuation_yield_without_scanning_the_story() {
        for text in [".".repeat(20_000), "á".repeat(20_000)] {
            let mut session = Session::new();
            session.execute("file.new", &json!({})).unwrap();
            let doc = &session.doc().unwrap().doc;
            let story = Story::with_text(designcraft_doc::StoryId(1), &text, Default::default());
            let mut cursor = SpellingCursor::default();
            let mut out = Vec::new();
            assert!(!cursor.advance(doc, &story, 32, 2, &mut out));
            assert!(cursor.byte <= 64);
            assert!(out.is_empty());
        }
    }
}
