//! Edit → Spelling: check spelling against the bundled US English word list (the public-domain
//! Moby list used for hyphenation) plus the document's user dictionary; suggestions by edit distance.

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
        cmd!("spelling.change", "Change", [], None, "{story, start, end, to}", has_doc, |s, p| {
            let sid = StoryId(p.get("story").and_then(Value::as_u64).ok_or_else(|| bad("spelling.change", "missing story"))?);
            let (a, b) = (p.get("start").and_then(Value::as_u64).unwrap_or(0) as usize, p.get("end").and_then(Value::as_u64).unwrap_or(0) as usize);
            let to = str_param(p, "to").unwrap_or("").to_string();
            s.edit(|d, _| {
                let st = d.story_mut(sid).ok_or(designcraft_doc::DocError::NoStory(sid))?;
                st.replace(a.min(st.len())..b.min(st.len()), &to);
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

/// Misspelled words of a story: byte ranges (acronyms skipped; the user dictionary counts).
pub fn misspellings(doc: &designcraft_doc::Document, story: &designcraft_doc::Story, user: &[String]) -> Vec<std::ops::Range<usize>> {
    // Text in other languages isn't checked against the English dictionary.
    let foreign: Vec<std::ops::Range<usize>> = {
        story
            .runs()
            .filter(|(r, f)| {
                let pi = story.para_at(r.start.min(story.len()));
                let base = doc.styles.resolve_para(&story.paras[pi.min(story.paras.len().saturating_sub(1))]).1;
                !designcraft_compose::is_english(&doc.styles.resolve_char(&base, f).language)
            })
            .map(|(r, _)| r)
            .collect()
    };
    let dict = Dictionary::en_us();
    let text = &story.text;
    let bytes: Vec<(usize, char)> = text.char_indices().collect();
    let mut out = Vec::new();
    let mut i = 0;
    while i < bytes.len() {
        if !bytes[i].1.is_alphabetic() {
            i += 1;
            continue;
        }
        let start = bytes[i].0;
        let mut j = i;
        while j < bytes.len()
            && (bytes[j].1.is_alphabetic() || ((bytes[j].1 == '\'' || bytes[j].1 == '’') && j + 1 < bytes.len() && bytes[j + 1].1.is_alphabetic()))
        {
            j += 1;
        }
        let end = bytes.get(j).map(|b| b.0).unwrap_or(text.len());
        let word = &text[start..end];
        let acronym = word.chars().all(|c| c.is_uppercase()) && word.chars().count() <= 5;
        if !acronym && !foreign.iter().any(|r| r.contains(&start)) && !known(dict, user, &word.to_lowercase()) {
            out.push(start..end);
        }
        i = j.max(i + 1);
    }
    out
}

fn check(s: &mut Session, p: &Value) -> Result<Value> {
    let st = s.doc()?;
    let d = &st.doc;
    let dict = Dictionary::en_us();
    let want_sugg = p.get("suggestions").and_then(Value::as_bool).unwrap_or(true);
    let stories: Vec<StoryId> = match p.get("story").and_then(Value::as_u64) {
        Some(id) => vec![StoryId(id)],
        None => d.stories.keys().copied().collect(),
    };
    let mut out = Vec::new();
    for sid in stories {
        let Some(story) = d.story(sid) else { continue };
        for r in misspellings(d, story, &d.user_words) {
            let word = &story.text[r.clone()];
            let sugg = if want_sugg { suggest(dict, &word.to_lowercase()) } else { vec![] };
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
