//! Language-aware English and Hungarian spelling, shared by commands and dynamic underlines.

mod hungarian;
mod incremental;
pub use incremental::SpellingCursor;

use designcraft_compose::hyphen::Dictionary;
use designcraft_doc::StoryId;
use serde_json::{Value, json};

use super::{CommandSpec, bad, cmd, has_doc, str_param};
use crate::{Result, Session};

pub fn specs() -> Vec<CommandSpec> {
    vec![
        cmd!(query "spelling.check", "Check Spelling…", ["Edit", "Spelling"], Some("Cmd+I"),
            "{story?, suggestions?: true} → [{story, start, end, word, suggestions}]", has_doc, check),
        cmd!(
            "hyphenation.addException",
            "Add Hyphenation Exception",
            ["Edit", "Spelling", "User Dictionary"],
            None,
            "{word: \"ex~am~ple\" (breaks only at ~) or a word with no ~ (never hyphenated)}",
            has_doc,
            |s, p| {
                let w = str_param(p, "word")
                    .map(str::trim)
                    .filter(|w| !w.replace('~', "").is_empty())
                    .ok_or_else(|| bad("hyphenation.addException", "missing word"))?
                    .to_string();
                let plain = w.replace('~', "").to_lowercase();
                s.edit(|d, _| {
                    d.hyphenation_exceptions.retain(|e| e.replace('~', "").to_lowercase() != plain);
                    d.hyphenation_exceptions.push(w.clone());
                    Ok(Value::Null)
                })
            }
        ),
        cmd!("hyphenation.removeException", "Remove Hyphenation Exception", [], None, "{word} (with or without ~)", has_doc, |s, p| {
            let plain = str_param(p, "word").unwrap_or("").replace('~', "").to_lowercase();
            s.edit(|d, _| {
                d.hyphenation_exceptions.retain(|e| e.replace('~', "").to_lowercase() != plain);
                Ok(Value::Null)
            })
        }),
        cmd!(query "hyphenation.list", "Hyphenation Exceptions", [], None, "{} → [word]", has_doc, |s, _| Ok(serde_json::to_value(&s.doc()?.doc.hyphenation_exceptions).unwrap_or_default())),
        cmd!("spelling.setWords", "User Dictionary", [], None, "{words: [word]} — replace the document's user dictionary", has_doc, |s, p| {
            let mut words: Vec<String> = p
                .get("words")
                .and_then(Value::as_array)
                .ok_or_else(|| bad("spelling.setWords", "`words` required"))?
                .iter()
                .filter_map(Value::as_str)
                .map(|w| w.trim().to_lowercase())
                .filter(|w| !w.is_empty())
                .collect();
            words.sort();
            words.dedup();
            s.edit(|d, _| {
                d.user_words = words.clone();
                Ok(json!({"words": words.len()}))
            })
        }),
        cmd!(query "spelling.words", "User Dictionary Words", [], None, "{} → [word]", has_doc, |s, _| Ok(json!(s.doc()?.doc.user_words))),
        cmd!("spelling.addWord", "Add to Dictionary", ["Edit", "Spelling"], None, "{word}", has_doc, |s, p| {
            let w = str_param(p, "word").ok_or_else(|| bad("spelling.addWord", "missing word"))?.to_lowercase();
            s.edit(|d, _| {
                if !d.user_words.contains(&w) {
                    d.user_words.push(w.clone());
                }
                Ok(Value::Null)
            })
        }),
        cmd!("spelling.change", "Change", [], None, "{story, start, end, to, expected?}", has_doc, |s, p| {
            let sid = StoryId(p.get("story").and_then(Value::as_u64).ok_or_else(|| bad("spelling.change", "missing story"))?);
            let (a, b) = (p.get("start").and_then(Value::as_u64).unwrap_or(0) as usize, p.get("end").and_then(Value::as_u64).unwrap_or(0) as usize);
            let to = str_param(p, "to").unwrap_or("").to_string();
            s.edit(|d, _| {
                let st = d.story_mut(sid).ok_or(designcraft_doc::DocError::NoStory(sid))?;
                let current = st.text.get(a..b).ok_or_else(|| bad("spelling.change", "invalid text range"))?;
                if p.get("expected").and_then(Value::as_str).is_some_and(|w| w != current) {
                    return Err(bad("spelling.change", "text changed; check spelling again"));
                }
                st.replace(a..b, &to);
                Ok(Value::Null)
            })
        }),
    ]
}

/// Is `w` (lowercase) a known word, allowing common inflections of dictionary stems?
pub fn known(dict: &Dictionary, user: &[String], w: &str) -> bool {
    if w.len() <= 1 || dict.get(w).is_some() || user.iter().any(|u| u == w) {
        return true;
    }
    let w = w.trim_end_matches("'s").trim_end_matches("’s");
    if dict.get(w).is_some() {
        return true;
    }
    let stems: [(&str, &[&str]); 9] = [
        ("ies", &["y"]),
        ("es", &["", "e"]),
        ("s", &[""]),
        ("ed", &["", "e"]),
        ("ing", &["", "e"]),
        ("ly", &[""]),
        ("er", &["", "e"]),
        ("est", &["", "e"]),
        ("ness", &[""]),
    ];
    for (suf, adds) in stems {
        if let Some(base) = w.strip_suffix(suf) {
            for add in adds {
                let cand = format!("{base}{add}");
                if cand.len() > 1 && dict.get(&cand).is_some() {
                    return true;
                }
                // Doubled consonant: "stopped" → "stop".
                if let Some(c) = base.chars().last()
                    && base.len() > 2
                    && base.ends_with(&format!("{c}{c}"))
                    && dict.get(&base[..base.len() - c.len_utf8()]).is_some()
                {
                    return true;
                }
            }
        }
    }
    false
}

/// Up to 6 suggestions at edit distance 1, ranked: transpositions, replacements, deletions, insertions.
pub fn suggest(dict: &Dictionary, w: &str) -> Vec<String> {
    let chars: Vec<char> = w.chars().collect();
    let mut out: Vec<String> = Vec::new();
    let mut push = |c: Vec<char>| {
        let c: String = c.into_iter().collect();
        if c != w && !out.contains(&c) && dict.get(&c).is_some() {
            out.push(c);
        }
    };
    for i in 0..chars.len().saturating_sub(1) {
        let mut c = chars.clone();
        c.swap(i, i + 1);
        push(c);
    }
    for i in 0..chars.len() {
        for l in 'a'..='z' {
            let mut c = chars.clone();
            c[i] = l;
            push(c);
        }
    }
    for i in 0..chars.len() {
        let mut c = chars.clone();
        c.remove(i);
        push(c);
    }
    for i in 0..=chars.len() {
        for l in 'a'..='z' {
            let mut c = chars.clone();
            c.insert(i, l);
            push(c);
        }
    }
    out.truncate(6);
    out
}

/// Misspelled words of a story: byte ranges (acronyms skipped; the user dictionary counts).
pub fn misspellings(doc: &designcraft_doc::Document, story: &designcraft_doc::Story, user: &[String]) -> Vec<std::ops::Range<usize>> {
    misspellings_in_runs(story, user, &language_runs(doc, story))
}

fn misspellings_in_runs(story: &designcraft_doc::Story, user: &[String], runs: &[(std::ops::Range<usize>, Language)]) -> Vec<std::ops::Range<usize>> {
    let mut out = Vec::new();
    let mut checked = std::collections::HashMap::new();
    for (range, language) in runs {
        let language = *language;
        if language == Language::Unsupported {
            continue;
        }
        let Some(text) = story.text.get(range.clone()) else { continue };
        for local in tokens(text, language == Language::Hungarian) {
            let Some(word) = text.get(local.clone()) else { continue };
            let key = (language, word);
            let valid = *checked.entry(key).or_insert_with(|| valid_word(language, word, user));
            if !valid {
                out.push(range.start + local.start..range.start + local.end);
            }
        }
    }
    out
}

fn valid_word(language: Language, word: &str, user: &[String]) -> bool {
    match language {
        Language::English => {
            let acronym = word.chars().all(char::is_uppercase) && word.chars().count() <= 5;
            acronym || known(Dictionary::en_us(), user, &word.to_lowercase())
        }
        Language::Hungarian => user.iter().any(|u| u == &word.to_lowercase()) || hungarian::known(word),
        Language::Unsupported => true,
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
enum Language {
    English,
    Hungarian,
    Unsupported,
}

fn language(name: &str) -> Language {
    if designcraft_doc::language_tag(name).is_some_and(|t| designcraft_doc::language_subtag(t) == "hu") {
        Language::Hungarian
    } else if designcraft_compose::is_english(name) {
        Language::English
    } else {
        Language::Unsupported
    }
}

// Thread-local counters verify real command work, independent of parallel tests.
#[cfg(test)]
std::thread_local! {
    static LANGUAGE_RUN_CALLS: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
    static SUGGESTION_CALLS: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
}

/// Merge adjacent formatting runs with the same language, but never join across paragraphs.
fn language_runs(doc: &designcraft_doc::Document, story: &designcraft_doc::Story) -> Vec<(std::ops::Range<usize>, Language)> {
    #[cfg(test)]
    LANGUAGE_RUN_CALLS.with(|calls| calls.set(calls.get() + 1));
    let mut out: Vec<(std::ops::Range<usize>, Language)> = Vec::new();
    for (range, format) in story.runs() {
        let mut start = range.start;
        while start < range.end {
            let end = story.text.get(start..range.end).and_then(|t| t.find('\n')).map_or(range.end, |i| start + i + 1);
            let pi = story.para_at(start.min(story.len()));
            let Some(para) = story.paras.get(pi) else { break };
            let base = doc.styles.resolve_para(para).1;
            let lang = language(&doc.styles.resolve_char(&base, format).language);
            if let Some((last, previous)) = out.last_mut()
                && *previous == lang
                && last.end == start
                && !story.text.get(..start).is_some_and(|s| s.ends_with('\n'))
            {
                last.end = end;
            } else {
                out.push((start..end, lang));
            }
            start = end;
        }
    }
    out
}

/// Preserve Hungarian numeric suffixes, internal hyphens and combining accents.
fn tokens(text: &str, hu: bool) -> Vec<std::ops::Range<usize>> {
    let chars: Vec<_> = text.char_indices().collect();
    let letter = |c: char| c.is_alphabetic() || (hu && c.is_ascii_digit());
    let mut out = Vec::new();
    let mut i = 0;
    while let Some(&(start, c)) = chars.get(i) {
        if !letter(c) {
            i += 1;
            continue;
        }
        let mut j = i + 1;
        while let Some(&(_, c)) = chars.get(j) {
            let connector = c == '\'' || c == '’' || (hu && c == '-');
            let accent = hu && ('\u{0300}'..='\u{036f}').contains(&c);
            if !(letter(c) || accent || (connector && chars.get(j + 1).is_some_and(|(_, n)| letter(*n)))) {
                break;
            }
            j += 1;
        }
        out.push(start..chars.get(j).map_or(text.len(), |(b, _)| *b));
        i = j;
    }
    out
}

/// Suggestions for one checked range, respecting the text's language and capitalization.
pub fn suggestions(doc: &designcraft_doc::Document, story: &designcraft_doc::Story, range: std::ops::Range<usize>) -> Vec<String> {
    let Some(word) = story.text.get(range.clone()) else { return vec![] };
    let lang = language_runs(doc, story).into_iter().find(|(r, _)| r.start <= range.start && r.end >= range.end).map(|(_, l)| l);
    suggestions_in_language(lang.unwrap_or(Language::Unsupported), word)
}

fn suggestions_in_language(language: Language, word: &str) -> Vec<String> {
    #[cfg(test)]
    SUGGESTION_CALLS.with(|calls| calls.set(calls.get() + 1));
    match language {
        Language::Hungarian => hungarian::suggest(word),
        Language::English => suggest(Dictionary::en_us(), &word.to_lowercase()),
        Language::Unsupported => vec![],
    }
}

fn check(s: &mut Session, p: &Value) -> Result<Value> {
    let st = s.doc()?;
    let d = &st.doc;
    let want_sugg = p.get("suggestions").and_then(Value::as_bool).unwrap_or(true);
    let stories: Vec<StoryId> = match p.get("story").and_then(Value::as_u64) {
        Some(id) => vec![StoryId(id)],
        None => d.stories.keys().copied().collect(),
    };
    let mut out = Vec::new();
    // Borrow exact words from the immutable document; do not mix languages or case.
    // This cache is local to one command, including when several stories are checked.
    let mut suggestion_cache = std::collections::HashMap::new();
    for sid in stories {
        let Some(story) = d.story(sid) else { continue };
        let runs = language_runs(d, story);
        if runs.iter().any(|(_, language)| *language == Language::Hungarian) {
            hungarian::initialize().map_err(|e| bad("spelling.check", e))?;
        }
        for r in misspellings_in_runs(story, &d.user_words, &runs) {
            let Some(word) = story.text.get(r.clone()) else { continue };
            let sugg = if want_sugg {
                let run = runs.get(runs.partition_point(|(run, _)| run.end <= r.start));
                match run.filter(|(run, _)| run.start <= r.start && run.end >= r.end).map(|(_, lang)| *lang) {
                    Some(lang) if lang != Language::Unsupported => {
                        suggestion_cache.entry((lang, word)).or_insert_with(|| suggestions_in_language(lang, word)).clone()
                    }
                    _ => vec![],
                }
            } else {
                vec![]
            };
            out.push(json!({"story": sid.0, "start": r.start, "end": r.end, "word": word, "suggestions": sugg}));
        }
    }
    Ok(Value::Array(out))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn known_words_and_suggestions() {
        let d = Dictionary::en_us();
        if d.is_empty() {
            return; // data not bundled in this build
        }
        for w in ["typography", "pages", "composed", "arranging", "quickly", "layouts", "stopped"] {
            assert!(known(d, &[], w), "{w}");
        }
        assert!(!known(d, &[], "typograpy"));
        assert!(suggest(d, "typograpy").contains(&"typography".to_string()));
        assert!(known(d, &["designcraft".into()], "designcraft"));
    }

    #[test]
    fn user_dictionary_replaces_and_counts() {
        let mut s = Session::new();
        s.execute("file.new", &serde_json::json!({})).unwrap();
        s.execute("spelling.setWords", &serde_json::json!({"words": ["Zorbo", " zorbo", "", "Quux"]})).unwrap();
        assert_eq!(s.execute("spelling.words", &serde_json::json!({})).unwrap(), serde_json::json!(["quux", "zorbo"]));
        let mut story = designcraft_doc::Story::new(designcraft_doc::StoryId(1));
        story.insert(0, "zorbo qwzx");
        let bad = misspellings(&s.doc().unwrap().doc, &story, &s.doc().unwrap().doc.user_words);
        assert_eq!(bad, vec![6..10]);
    }

    #[test]
    fn check_finds_misspellings() {
        let mut s = Session::new();
        s.execute("file.new", &json!({})).unwrap();
        s.execute("frame.create", &json!({"rect": [36, 36, 300, 200], "content": "text", "text": "Thw quick brown fox jumpd over the NASA dog."}))
            .unwrap();
        let r = s.execute("spelling.check", &json!({})).unwrap();
        let words: Vec<&str> = r.as_array().unwrap().iter().map(|m| m["word"].as_str().unwrap()).collect();
        assert_eq!(words, vec!["Thw", "jumpd"]);
        assert!(r[0]["suggestions"].as_array().unwrap().iter().any(|v| v == "the"));
        s.execute("spelling.addWord", &json!({"word": "jumpd"})).unwrap();
        assert_eq!(s.execute("spelling.check", &json!({})).unwrap().as_array().unwrap().len(), 1);
    }
}

#[cfg(test)]
mod hyphenation_exception_tests {
    use serde_json::json;

    use crate::Session;

    fn first_line(s: &Session, sid: designcraft_doc::StoryId) -> String {
        let d = &s.doc().unwrap().doc;
        let cs = s.cache.get(d, sid, None);
        let l = &cs.frames[0].lines[0];
        let mut t = d.stories[&sid].text[l.range.clone()].to_string();
        if l.hyphenated {
            t.push('-');
        }
        t
    }

    #[test]
    fn user_exceptions_decide_where_words_break() {
        let mut s = Session::new();
        s.execute("file.new", &json!({})).unwrap();
        let r = s.execute("frame.create", &json!({"rect": [72, 72, 172, 400], "content": "text", "text": "a internationalization"})).unwrap();
        let sid = designcraft_doc::StoryId(r["story"].as_u64().unwrap());
        s.execute("text.select", &json!({"story": r["story"], "anchor": 0, "focus": 0})).unwrap();
        s.execute("type.para", &json!({"hyphenate": true, "hyphLastWord": true})).unwrap();
        s.execute("hyphenation.addException", &json!({"word": "in~ternationalization"})).unwrap();
        assert_eq!(first_line(&s, sid), "a in-", "the only break allowed");
        s.execute("hyphenation.addException", &json!({"word": "internationalization"})).unwrap();
        assert_eq!(s.execute("hyphenation.list", &json!({})).unwrap().as_array().unwrap().len(), 1, "replaces the earlier entry");
        assert!(!first_line(&s, sid).ends_with('-'), "never hyphenated");
    }
}

#[cfg(test)]
mod hungarian_integration_tests {
    use super::*;
    fn document(text: &str, lang: &str) -> (Session, u64) {
        let mut s = Session::new();
        s.execute("file.new", &json!({})).unwrap();
        let r = s.execute("frame.create", &json!({"rect": [36,36,500,700], "content":"text", "text":text})).unwrap();
        let sid = r["story"].as_u64().unwrap();
        s.execute("text.select", &json!({"story":sid,"anchor":0,"focus":text.len()})).unwrap();
        s.execute("type.char", &json!({"attrs":{"language":lang}})).unwrap();
        (s, sid)
    }
    fn words(s: &mut Session) -> Vec<String> {
        s.execute("spelling.check", &json!({"suggestions":false}))
            .unwrap()
            .as_array()
            .unwrap()
            .iter()
            .map(|v| v["word"].as_str().unwrap().to_string())
            .collect()
    }
    #[test]
    fn suggestions_are_cached_across_occurrences_and_stories() {
        let text = "hejesírás hejesírás";
        let (mut s, first) = document(text, "hu");
        let r = s.execute("frame.create", &json!({"rect":[36,36,500,700],"content":"text","text":text})).unwrap();
        let second = r["story"].as_u64().unwrap();
        s.execute("text.select", &json!({"story":second,"anchor":0,"focus":text.len()})).unwrap();
        s.execute("type.char", &json!({"attrs":{"language":"hu"}})).unwrap();
        let calls = SUGGESTION_CALLS.get();
        let runs = LANGUAGE_RUN_CALLS.get();
        let report = s.execute("spelling.check", &json!({})).unwrap();
        assert_eq!(SUGGESTION_CALLS.get() - calls, 1, "one lookup per distinct language/word in a command");
        assert_eq!(LANGUAGE_RUN_CALLS.get() - runs, 2, "resolve each story's languages once");
        let items = report.as_array().unwrap();
        assert_eq!(items.len(), 4);
        for (i, item) in items.iter().enumerate() {
            assert_eq!(item["story"], json!(if i < 2 { first } else { second }));
            assert_eq!(item["start"], json!((i % 2) * "hejesírás ".len()));
            assert_eq!(item["end"], json!((i % 2) * "hejesírás ".len() + "hejesírás".len()));
            assert_eq!(item["suggestions"], items[0]["suggestions"]);
            assert!(item["suggestions"].as_array().unwrap().contains(&json!("helyesírás")));
        }
        let calls = SUGGESTION_CALLS.get();
        let disabled = s.execute("spelling.check", &json!({"suggestions":false})).unwrap();
        assert_eq!(SUGGESTION_CALLS.get(), calls);
        assert_eq!(disabled.as_array().unwrap().len(), 4);
        assert!(disabled.as_array().unwrap().iter().all(|item| item["suggestions"] == json!([])));
        // Cache lifetime is one command, and selected-story scope still applies.
        s.execute("spelling.check", &json!({"story":first})).unwrap();
        assert_eq!(SUGGESTION_CALLS.get() - calls, 1);
    }

    #[test]
    fn suggestion_cache_preserves_language_case_and_unsupported_runs() {
        let text = "typograpy typograpy Hejesírás hejesírás qzxw";
        let (mut s, sid) = document(text, "hu");
        for (start, end, lang) in [(0, 9, "English: USA"), (text.find("qzxw").unwrap(), text.len(), "German")] {
            s.execute("text.select", &json!({"story":sid,"anchor":start,"focus":end})).unwrap();
            s.execute("type.char", &json!({"attrs":{"language":lang}})).unwrap();
        }
        let d = &s.doc().unwrap().doc;
        let story = d.story(StoryId(sid)).unwrap();
        let ranges = misspellings(d, story, &d.user_words);
        assert_eq!(ranges.len(), 4);
        let expected: Vec<_> = ranges.iter().map(|r| suggestions(d, story, r.clone())).collect();
        assert!(expected[0].contains(&"typography".to_string()));
        assert!(expected[2].contains(&"Helyesírás".to_string()));
        assert!(expected[3].contains(&"helyesírás".to_string()));
        let calls = SUGGESTION_CALLS.get();
        let runs = LANGUAGE_RUN_CALLS.get();
        let report = s.execute("spelling.check", &json!({})).unwrap();
        assert_eq!(SUGGESTION_CALLS.get() - calls, 4, "same spelling in different languages and cases must stay distinct");
        assert_eq!(LANGUAGE_RUN_CALLS.get() - runs, 1);
        let items = report.as_array().unwrap();
        assert_eq!(items.len(), ranges.len());
        for ((item, range), expected) in items.iter().zip(ranges).zip(expected) {
            assert_eq!(item["start"], json!(range.start));
            assert_eq!(item["end"], json!(range.end));
            assert_eq!(item["suggestions"], json!(expected));
        }
    }

    #[test]
    fn non_hungarian_checks_do_not_initialize_hungarian() {
        let mut empty = Session::new();
        empty.execute("file.new", &json!({})).unwrap();
        let before = hungarian::initialization_calls();
        assert_eq!(empty.execute("spelling.check", &json!({})).unwrap(), json!([]));
        assert_eq!(hungarian::initialization_calls(), before, "empty document");
        for lang in ["English: USA", "German"] {
            let (mut s, _) = document("typograpy", lang);
            for suggestions in [false, true] {
                let before = hungarian::initialization_calls();
                let result = s.execute("spelling.check", &json!({"suggestions": suggestions})).unwrap();
                assert_eq!(hungarian::initialization_calls(), before, "{lang}, suggestions={suggestions}");
                assert_eq!(result.as_array().unwrap().len(), usize::from(lang == "English: USA"));
            }
        }
    }

    #[test]
    fn hungarian_initialization_respects_story_scope() {
        let (mut s, hu) = document("hejesírás", "hu");
        let r = s.execute("frame.create", &json!({"rect": [36,36,500,700], "content":"text", "text":"typograpy"})).unwrap();
        let en = r["story"].as_u64().unwrap();
        s.execute("text.select", &json!({"story": en,"anchor":0,"focus":9})).unwrap();
        s.execute("type.char", &json!({"attrs":{"language":"English: USA"}})).unwrap();
        for sid in [en, u64::MAX] {
            let before = hungarian::initialization_calls();
            let result = s.execute("spelling.check", &json!({"story":sid})).unwrap();
            assert_eq!(hungarian::initialization_calls(), before, "unselected Hungarian story must not initialize");
            assert_eq!(result.as_array().unwrap().len(), usize::from(sid == en));
        }
        for params in [json!({"story":hu,"suggestions":false}), json!({"suggestions":false})] {
            let before = hungarian::initialization_calls();
            let result = s.execute("spelling.check", &params).unwrap();
            assert!(hungarian::initialization_calls() > before);
            assert!(result.as_array().unwrap().iter().any(|item| item["word"] == "hejesírás"));
        }
    }

    #[test]
    fn mixed_story_initializes_hungarian() {
        let (mut s, sid) = document("typograpy hejesírás", "English: USA");
        s.execute("text.select", &json!({"story":sid,"anchor":10,"focus":"typograpy hejesírás".len()})).unwrap();
        s.execute("type.char", &json!({"attrs":{"language":"hu"}})).unwrap();
        let before = hungarian::initialization_calls();
        assert_eq!(words(&mut s), ["typograpy", "hejesírás"]);
        assert!(hungarian::initialization_calls() > before);
    }

    #[test]
    fn hungarian_text_preserves_compounds_numbers_case_and_byte_ranges() {
        let text = "Árvíztűrő tükörfúrógép munkavállalóinkkal 2026-ban 2026-ben hejesírás.";
        for lang in ["Hungarian", "hu", "hu-HU", "hu_HU"] {
            let (mut s, _) = document(text, lang);
            assert_eq!(words(&mut s), ["2026-ben", "hejesírás"], "{lang}");
            let result = s.execute("spelling.check", &json!({})).unwrap();
            for item in result.as_array().unwrap() {
                let a = item["start"].as_u64().unwrap() as usize;
                let b = item["end"].as_u64().unwrap() as usize;
                assert_eq!(&text[a..b], item["word"].as_str().unwrap());
            }
            assert!(result[1]["suggestions"].as_array().unwrap().contains(&json!("helyesírás")));
        }
    }
    #[test]
    fn formatting_does_not_split_a_word_but_language_does() {
        let (mut s, sid) = document("helyesírás typograpy", "hu");
        s.execute("text.select", &json!({"story":sid,"anchor":0,"focus":3})).unwrap();
        s.execute("type.char", &json!({"attrs":{"size":18}})).unwrap();
        let start = "helyesírás ".len();
        s.execute("text.select", &json!({"story":sid,"anchor":start,"focus":start+9})).unwrap();
        s.execute("type.char", &json!({"attrs":{"language":"English: USA"}})).unwrap();
        assert_eq!(words(&mut s), ["typograpy"]);
    }
    #[test]
    fn user_words_changes_and_undo_work_in_hungarian() {
        let (mut s, sid) = document("hejesírás qzxw", "hu");
        s.execute("spelling.addWord", &json!({"word":"qzxw"})).unwrap();
        assert_eq!(words(&mut s), ["hejesírás"]);
        s.execute("spelling.change", &json!({"story":sid,"start":0,"end":"hejesírás".len(),"to":"helyesírás","expected":"hejesírás"})).unwrap();
        assert!(words(&mut s).is_empty());
        s.execute("edit.undo", &json!({})).unwrap();
        assert_eq!(words(&mut s), ["hejesírás"]);
        assert!(s.execute("spelling.change", &json!({"story":sid,"start":0,"end":2,"to":"x","expected":"wrong"})).is_err());
        let before = s.doc().unwrap().doc.story(StoryId(sid)).unwrap().text.clone();
        assert!(s.execute("spelling.change", &json!({"story":sid,"start":6,"end":7,"to":"x"})).is_err());
        assert_eq!(s.doc().unwrap().doc.story(StoryId(sid)).unwrap().text, before);
    }
    #[test]
    fn decomposed_accents_are_checked_without_changing_document_offsets() {
        let text = "tu\u{0308}ko\u{0308}rfu\u{0301}ro\u{0301}ge\u{0301}p hejesírás";
        let (mut s, _) = document(text, "hu");
        assert_eq!(words(&mut s), ["hejesírás"]);
        let result = s.execute("spelling.check", &json!({"suggestions":false})).unwrap();
        assert_eq!(result[0]["start"], json!(text.find("hejesírás").unwrap()));
    }
    #[test]
    fn unsupported_languages_are_not_checked_with_a_hungarian_dictionary() {
        let (mut s, _) = document("qzxw hejesírás", "German");
        assert!(words(&mut s).is_empty());
    }
}
