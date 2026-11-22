//! Language-aware English and Hungarian spelling, shared by commands and dynamic underlines.

mod hungarian;
mod incremental;
pub use incremental::SpellingCursor;

use designcraft_compose::hyphen::Dictionary;
use designcraft_doc::StoryId;
use serde_json::{Value, json};
use unicode_normalization::UnicodeNormalization;

use super::{CommandSpec, bad, cmd, has_doc, str_param};
use crate::{Result, Session};

/// Original licence/author notices of embedded and installed Language packs.
pub fn language_data_licences() -> Vec<(String, String)> {
    craft_proof::packs::licence_texts()
}

/// Register spelling commands and their accepted parameters.
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
                let r = {
                    let st = d.story_mut(sid).ok_or(designcraft_doc::DocError::NoStory(sid))?;
                    let current = st.text.get(a..b).ok_or_else(|| bad("spelling.change", "invalid text range"))?;
                    if p.get("expected").and_then(Value::as_str).is_some_and(|w| w != current) {
                        return Err(bad("spelling.change", "text changed; check spelling again"));
                    }
                    let r = a..b;
                    st.replace(r.clone(), &to);
                    r
                };
                d.adjust_text_ranges(sid, r, to.len());
                Ok(Value::Null)
            })
        }),
    ]
}

/// Common words the hyphenation word list (from the early 1990s) lacks: everyday short words
/// such as "we", "us" and "if", titles, and words that came with computers and the web. Sorted,
/// lowercase, base forms (the inflections below reach "emails", "downloaded", "bloggers").
const COMMON: &[&str] = &[
    "ah",
    "bitmap",
    "blog",
    "bluetooth",
    "chatbot",
    "checkbox",
    "cybersecurity",
    "database",
    "dataset",
    "desktop",
    "download",
    "dr",
    "dropdown",
    "ebook",
    "ecommerce",
    "eh",
    "email",
    "emoji",
    "firewall",
    "gigabyte",
    "hashtag",
    "hm",
    "hmm",
    "homepage",
    "hyperlink",
    "if",
    "inbox",
    "internet",
    "jr",
    "kilobyte",
    "laptop",
    "livestream",
    "login",
    "logout",
    "malware",
    "megabyte",
    "metadata",
    "mr",
    "offline",
    "ok",
    "online",
    "ow",
    "pm",
    "podcast",
    "popup",
    "printout",
    "screenshot",
    "selfie",
    "smartphone",
    "smartwatch",
    "spreadsheet",
    "sr",
    "startup",
    "terabyte",
    "touchscreen",
    "um",
    "upload",
    "us",
    "username",
    "voicemail",
    "vs",
    "we",
    "webinar",
    "webmaster",
    "webpage",
    "website",
    "wifi",
    "workflow",
    "yay",
];

/// Longest word [`suggest`] makes candidates for.
pub const MAX_SUGGEST_CHARS: usize = 40;

/// Is `w` (lowercase) a known word, allowing common inflections of dictionary stems?
pub fn known(dict: &Dictionary, user: &[String], w: &str) -> bool {
    // A typographer's apostrophe (don’t, we’re) is the word list's straight one.
    let w = &w.replace('’', "'");
    let has = |w: &str| dict.get(w).is_some() || COMMON.binary_search(&w).is_ok();
    if w.len() <= 1 || has(w) || user.iter().any(|u| u.replace('’', "'") == *w) {
        return true;
    }
    let w = w.trim_end_matches("'s");
    if has(w) {
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
                if cand.len() > 1 && has(&cand) {
                    return true;
                }
                // Doubled consonant: "stopped" → "stop".
                if let Some(c) = base.chars().last()
                    && base.len() > 2
                    && base.ends_with(&format!("{c}{c}"))
                    && has(&base[..base.len() - c.len_utf8()])
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
    // Each candidate is a copy of the word: 53 per position, quadratic in its length. No word of
    // a dictionary is this long, so no edit of such a "word" is one either. Checked before collecting.
    if w.chars().take(MAX_SUGGEST_CHARS + 1).count() > MAX_SUGGEST_CHARS {
        return Vec::new();
    }
    let chars: Vec<char> = w.chars().collect();
    let mut out: Vec<String> = Vec::new();
    let mut push = |c: Vec<char>| {
        let c: String = c.into_iter().collect();
        if c != w && !out.contains(&c) && (dict.get(&c).is_some() || COMMON.binary_search(&c.as_str()).is_ok()) {
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

/// Ensure that the dictionaries required by this story are available. English
/// retains its existing checker; Hungarian data is loaded only for Hungarian runs.
pub fn ensure_story_dictionary(doc: &designcraft_doc::Document, story: &designcraft_doc::Story) -> std::result::Result<(), &'static str> {
    ensure_run_dictionaries(&language_runs(doc, story))
}

/// Load Hungarian data only when at least one checked run requires it.
fn ensure_run_dictionaries(runs: &[(std::ops::Range<usize>, Language)]) -> std::result::Result<(), &'static str> {
    if runs.iter().any(|(_, language)| *language == Language::Hungarian) {
        hungarian::initialize()?;
    }
    Ok(())
}

/// Misspelled words of a story: byte ranges (acronyms skipped; the user dictionary counts).
pub fn misspellings(doc: &designcraft_doc::Document, story: &designcraft_doc::Story, user: &[String]) -> Vec<std::ops::Range<usize>> {
    misspellings_in_runs(story, user, &language_runs(doc, story))
}

/// Collect source byte ranges, reusing verdicts only within the same language and spelling.
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

/// Check a token with its language checker and the document user dictionary.
fn valid_word(language: Language, word: &str, user: &[String]) -> bool {
    match language {
        Language::English => {
            let acronym = word.chars().all(char::is_uppercase) && word.chars().count() <= 5;
            acronym || known(Dictionary::en_us(), user, &word.to_lowercase())
        }
        Language::Hungarian => {
            if user.is_empty() {
                return hungarian::known(word);
            }
            if hungarian::known(word) {
                return true;
            }
            let canonical = |s: &str| s.nfc().collect::<String>().to_lowercase();
            let key = canonical(word);
            user.iter().any(|u| canonical(u) == key)
        }
        Language::Unsupported => true,
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
enum Language {
    English,
    Hungarian,
    Unsupported,
}

/// Resolve a document language name or tag to a supported spelling checker.
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

/// Bound the English edit-candidate generator before allocating lowercase text or edits.
const MAX_ENGLISH_SUGGESTION_BYTES: usize = 256;

/// Generate language-specific suggestions without sharing results across languages.
fn suggestions_in_language(language: Language, word: &str) -> Vec<String> {
    if language == Language::English && word.len() > MAX_ENGLISH_SUGGESTION_BYTES {
        return vec![];
    }
    #[cfg(test)]
    SUGGESTION_CALLS.with(|calls| calls.set(calls.get() + 1));
    match language {
        Language::Hungarian => hungarian::suggest(word),
        Language::English => suggest(Dictionary::en_us(), &word.to_lowercase()),
        Language::Unsupported => vec![],
    }
}

/// Check the requested stories, optionally caching suggestions for repeated tokens.
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
        ensure_run_dictionaries(&runs).map_err(|e| bad("spelling.check", e))?;
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
        assert!(suggest(d, "emial").contains(&"email".to_string()), "words in COMMON are suggested too");
        assert!(known(d, &["designcraft".into()], "designcraft"));
    }

    /// A "word" longer than any in the dictionary gets no candidates: there would be more than
    /// fifty copies of it per character.
    #[test]
    fn very_long_words_get_no_suggestions() {
        // The dictionary holds the longest allowed word, so deleting one letter of the 41-letter
        // input would be a hit if the length cap were not applied.
        let d = Dictionary::from_entries(vec![("a".repeat(MAX_SUGGEST_CHARS), 0)]);
        assert_eq!(suggest(&d, &"a".repeat(MAX_SUGGEST_CHARS)), Vec::<String>::new());
        assert_eq!(suggest(&d, &"a".repeat(MAX_SUGGEST_CHARS - 1)), vec!["a".repeat(MAX_SUGGEST_CHARS)]);
        assert!(suggest(&d, &"a".repeat(MAX_SUGGEST_CHARS + 1)).is_empty());
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

    /// Everyday words the hyphenation list lacks aren't misspelled ("we", "us", "if", "email"…),
    /// with their inflections, and neither are contractions typed with a typographer's
    /// apostrophe, which Use Typographer's Quotes turns ' into.
    #[test]
    fn common_words_and_curly_apostrophes_are_known() {
        let d = Dictionary::en_us();
        if d.is_empty() {
            return; // data not bundled in this build
        }
        assert!(COMMON.is_sorted(), "COMMON is binary-searched");
        for w in [
            "we",
            "us",
            "if",
            "ok",
            "mr",
            "email",
            "emails",
            "emailed",
            "online",
            "website",
            "websites",
            "downloaded",
            "database",
            "databases",
            "blogger",
            "don’t",
            "can’t",
            "i’m",
            "we’re",
            "they’ve",
            "it’s",
            "don't",
        ] {
            assert!(known(d, &[], w), "{w}");
        }
        for w in ["dont", "wee’re", "emial"] {
            assert!(!known(d, &[], w), "{w}");
        }
        // A user word added with a typographer's apostrophe matches either form.
        assert!(known(d, &["zorbo’s".into()], "zorbo's") && known(d, &["zorbo's".into()], "zorbo’s"));
        let mut s = Session::new();
        s.execute("file.new", &json!({})).unwrap();
        s.execute("frame.create", &json!({"rect": [36, 36, 300, 200], "content": "text", "text": "We don’t email us if we’re online."})).unwrap();
        assert_eq!(s.execute("spelling.check", &json!({})).unwrap(), json!([]));
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
    /// Require explicit selection of the verified Hungarian pack for these opt-in integration tests.
    fn require_hu_pack() {
        assert!(
            option_env!("CRAFT_PROOF_EMBED_PACKS").is_some_and(|locales| locales.split(',').any(|locale| locale.trim() == "hu")),
            "select the verified hu Language pack before explicitly running this test"
        );
    }

    /// Build a small mixed-language document for spelling regressions.
    fn document(text: &str, lang: &str) -> (Session, u64) {
        let mut s = Session::new();
        s.execute("file.new", &json!({})).unwrap();
        let r = s.execute("frame.create", &json!({"rect": [36,36,500,700], "content":"text", "text":text})).unwrap();
        let sid = r["story"].as_u64().unwrap();
        s.execute("text.select", &json!({"story":sid,"anchor":0,"focus":text.len()})).unwrap();
        s.execute("type.char", &json!({"attrs":{"language":lang}})).unwrap();
        (s, sid)
    }
    /// Extract reported spelling words for readable assertions.
    fn words(s: &mut Session) -> Vec<String> {
        s.execute("spelling.check", &json!({"suggestions":false}))
            .unwrap()
            .as_array()
            .unwrap()
            .iter()
            .map(|v| v["word"].as_str().unwrap().to_string())
            .collect()
    }
    /// Oversized UTF-8 and ASCII tokens never reach the English edit generator.
    #[test]
    fn english_suggestions_bound_bytes_before_generating_candidates() {
        let calls = SUGGESTION_CALLS.get();
        for word in ["x".repeat(MAX_ENGLISH_SUGGESTION_BYTES + 1), "ő".repeat(MAX_ENGLISH_SUGGESTION_BYTES / 2 + 1)] {
            assert!(suggestions_in_language(Language::English, &word).is_empty());
            assert_eq!(SUGGESTION_CALLS.get(), calls);
        }
        let boundary = "x".repeat(MAX_ENGLISH_SUGGESTION_BYTES);
        suggestions_in_language(Language::English, &boundary);
        assert_eq!(SUGGESTION_CALLS.get(), calls + 1, "the byte limit is inclusive");
        assert!(suggestions_in_language(Language::English, "typograpy").contains(&"typography".to_string()));
        assert!(suggestions_in_language(Language::Unsupported, "typograpy").is_empty());
    }

    /// A replacement keeps live text links aligned and rejects stale or invalid source ranges.
    #[test]
    fn spelling_change_adjusts_text_links_and_validates_source() {
        use designcraft_doc::{Hyperlink, HyperlinkDest, HyperlinkSource};
        let (mut s, sid) = document("typo linked", "English: USA");
        std::sync::Arc::make_mut(&mut s.doc_mut().unwrap().doc).hyperlinks.push(Hyperlink {
            id: 1,
            name: "Link".into(),
            source: HyperlinkSource::Text { story: StoryId(sid), start: 5, end: 11 },
            dest: HyperlinkDest::Url("https://example.com".into()),
        });
        for (start, end, expected) in [(0, 4, "stale"), (0, 100, "typo"), (4, 0, "typo")] {
            assert!(s.execute("spelling.change", &json!({"story":sid,"start":start,"end":end,"to":"typography","expected":expected})).is_err());
            assert_eq!(s.doc().unwrap().doc.stories[&StoryId(sid)].text, "typo linked");
        }
        s.execute("spelling.change", &json!({"story":sid,"start":0,"end":4,"to":"typography","expected":"typo"})).unwrap();
        assert_eq!(s.doc().unwrap().doc.hyperlinks[0].source, HyperlinkSource::Text { story: StoryId(sid), start: 11, end: 17 });
        assert_eq!(s.doc().unwrap().doc.stories[&StoryId(sid)].text, "typography linked");
        s.execute("edit.undo", &json!({})).unwrap();
        assert_eq!(s.doc().unwrap().doc.hyperlinks[0].source, HyperlinkSource::Text { story: StoryId(sid), start: 5, end: 11 });
    }

    /// Regression: suggestions are cached across occurrences and stories.
    #[test]
    #[ignore = "requires the verified hu Language pack; see docs/hungarian-language-support.md"]
    fn suggestions_are_cached_across_occurrences_and_stories() {
        require_hu_pack();
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

    /// Regression: suggestion cache preserves language case and unsupported runs.
    #[test]
    #[ignore = "requires the verified hu Language pack; see docs/hungarian-language-support.md"]
    fn suggestion_cache_preserves_language_case_and_unsupported_runs() {
        require_hu_pack();
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

    /// Regression: non hungarian checks do not initialize hungarian.
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

    /// Regression: hungarian initialization respects story scope.
    #[test]
    #[ignore = "requires the verified hu Language pack; see docs/hungarian-language-support.md"]
    fn hungarian_initialization_respects_story_scope() {
        require_hu_pack();
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

    /// Regression: mixed story initializes hungarian.
    #[test]
    #[ignore = "requires the verified hu Language pack; see docs/hungarian-language-support.md"]
    fn mixed_story_initializes_hungarian() {
        require_hu_pack();
        let (mut s, sid) = document("typograpy hejesírás", "English: USA");
        s.execute("text.select", &json!({"story":sid,"anchor":10,"focus":"typograpy hejesírás".len()})).unwrap();
        s.execute("type.char", &json!({"attrs":{"language":"hu"}})).unwrap();
        let before = hungarian::initialization_calls();
        assert_eq!(words(&mut s), ["typograpy", "hejesírás"]);
        assert!(hungarian::initialization_calls() > before);
    }

    /// Regression: hungarian text preserves compounds numbers case and byte ranges.
    #[test]
    #[ignore = "requires the verified hu Language pack; see docs/hungarian-language-support.md"]
    fn hungarian_text_preserves_compounds_numbers_case_and_byte_ranges() {
        require_hu_pack();
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
    /// Regression: formatting does not split a word but language does.
    #[test]
    #[ignore = "requires the verified hu Language pack; see docs/hungarian-language-support.md"]
    fn formatting_does_not_split_a_word_but_language_does() {
        require_hu_pack();
        let (mut s, sid) = document("helyesírás typograpy", "hu");
        s.execute("text.select", &json!({"story":sid,"anchor":0,"focus":3})).unwrap();
        s.execute("type.char", &json!({"attrs":{"size":18}})).unwrap();
        let start = "helyesírás ".len();
        s.execute("text.select", &json!({"story":sid,"anchor":start,"focus":start+9})).unwrap();
        s.execute("type.char", &json!({"attrs":{"language":"English: USA"}})).unwrap();
        assert_eq!(words(&mut s), ["typograpy"]);
    }
    /// Regression: user words changes and undo work in hungarian.
    #[test]
    #[ignore = "requires the verified hu Language pack; see docs/hungarian-language-support.md"]
    fn user_words_changes_and_undo_work_in_hungarian() {
        require_hu_pack();
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
    /// Regression: decomposed accents are checked without changing document offsets.
    #[test]
    #[ignore = "requires the verified hu Language pack; see docs/hungarian-language-support.md"]
    fn decomposed_accents_are_checked_without_changing_document_offsets() {
        require_hu_pack();
        let text = "tu\u{0308}ko\u{0308}rfu\u{0301}ro\u{0301}ge\u{0301}p hejesírás";
        let (mut s, _) = document(text, "hu");
        assert_eq!(words(&mut s), ["hejesírás"]);
        let result = s.execute("spelling.check", &json!({"suggestions":false})).unwrap();
        assert_eq!(result[0]["start"], json!(text.find("hejesírás").unwrap()));
    }
    /// Regression: unsupported languages are not checked with a hungarian dictionary.
    #[test]
    fn unsupported_languages_are_not_checked_with_a_hungarian_dictionary() {
        let (mut s, _) = document("qzxw hejesírás", "German");
        assert!(words(&mut s).is_empty());
    }

    /// Regression: hungarian user words match both unicode forms and legacy case.
    #[test]
    #[ignore = "requires the verified hu Language pack; see docs/hungarian-language-support.md"]
    fn hungarian_user_words_match_both_unicode_forms_and_legacy_case() {
        require_hu_pack();
        let composed = "qzxőr";
        let decomposed = "qzxo\u{030b}r";
        for text in [composed, decomposed] {
            for user in [composed.to_string(), decomposed.to_string(), composed.to_uppercase(), decomposed.to_uppercase()] {
                assert!(valid_word(Language::Hungarian, text, std::slice::from_ref(&user)), "{text:?}: {user:?}");
                let (mut s, sid) = document(text, "hu");
                s.execute("spelling.setWords", &json!({"words":[user]})).unwrap();
                assert!(words(&mut s).is_empty());
                assert_eq!(s.doc().unwrap().doc.story(StoryId(sid)).unwrap().text, text);
            }
        }
    }
}
