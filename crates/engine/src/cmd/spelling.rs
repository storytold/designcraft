//! Edit → Spelling, over the shared proofing engine (`craft-proof`): each language's spelling and
//! Khmer preferred forms.
//! All findings address the original story bytes, including across formatting runs.

mod khmer;

use std::ops::Range;

use craft_proof::{IssueKind, ProofLang};
use designcraft_compose::hyphen::Dictionary;
use designcraft_doc::StoryId;
use serde_json::{Value, json};

use super::{CommandSpec, bad, cmd, has_doc, str_param};
use crate::{Result, Session};

pub fn specs() -> Vec<CommandSpec> {
    vec![
        cmd!(query "spelling.check", "Check Spelling…", ["Edit", "Spelling"], Some("Cmd+I"),
            "{story?, suggestions?: true} → [{story, start, end, word, suggestions, kind, message, language}]", has_doc, check),
        cmd!(query "spelling.suggest", "Spelling Suggestions", [], None,
            "{story, start, end} → {story, start, end, word, suggestions, kind, message, language}; revalidates one current issue in its paragraph", has_doc, suggest_issue),
        cmd!(query "spelling.at", "Spelling Issue at Caret", [], None,
            "{story, at} → the issue around byte offset `at`, with its suggestions, or null", has_doc, issue_at),
        cmd!(query "spelling.status", "Spelling Dictionaries", [], None,
            "{} → {englishAvailable, khmerAvailable}; unavailable text is unchecked, not correct", has_doc,
            |_, _| Ok(json!({"englishAvailable": craft_proof::has_dictionary(Some("en-US")), "khmerAvailable": craft_khmer::data_available()}))),
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
            let offset = |key| {
                p.get(key)
                    .and_then(Value::as_u64)
                    .and_then(|n| usize::try_from(n).ok())
                    .ok_or_else(|| bad("spelling.change", "start and end must be byte offsets"))
            };
            let (a, b) = (offset("start")?, offset("end")?);
            let to = str_param(p, "to").unwrap_or("").to_string();
            s.edit(|d, _| {
                let st = d.story_mut(sid).ok_or(designcraft_doc::DocError::NoStory(sid))?;
                if a > b || st.text.get(a..b).is_none() {
                    return Err(bad("spelling.change", "range must address original UTF-8 character boundaries"));
                }
                st.replace(a..b, &to);
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
    // Bound candidate construction for hostile long tokens in imported books.
    if w.chars().take(65).count() > 64 {
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

/// Whether Khmer proofing has its data. Without it, Khmer text is unchecked, not correct.
pub fn khmer_available() -> bool {
    craft_khmer::data_available()
}

/// Whether `text` holds Khmer, by the same rule proofing uses.
pub fn has_khmer(text: &str) -> bool {
    text.chars().any(khmer::character)
}

/// A spelling or preferred-form finding; `range` always addresses original story UTF-8 bytes.
#[derive(Clone, Debug)]
pub struct ProofIssue {
    pub range: Range<usize>,
    /// `spelling`, or `grammar` for the shared Khmer preferred spelling/phrase rules.
    pub kind: &'static str,
    pub message: String,
    /// The language's tag (`en`, `km`, …): suggestions come from its dictionary.
    pub language: String,
    lang: ProofLang,
    pub suggestions: Vec<String>,
    /// Retain the cheap rule replacement even when candidate generation is disabled.
    preferred: Option<String>,
}

/// The language proofing checks a run in. A language it doesn't know, or No Language, is
/// [`ProofLang::Other`]: no marks.
fn proof_language(language: &str) -> ProofLang {
    let name = language.trim().trim_start_matches("$ID/");
    match designcraft_doc::language_tag(name) {
        Some(tag) => ProofLang::from_tag(Some(tag)),
        // No tag means English to craft-proof; for a name it means [No Language] or an unknown one.
        None if designcraft_compose::is_english(name) => ProofLang::En,
        None => ProofLang::Other,
    }
}

/// Intersect character runs with paragraphs: one character run can span paragraphs
/// with different base languages. Formatting boundaries do not become word boundaries.
fn language_spans(
    doc: &designcraft_doc::Document,
    story: &designcraft_doc::Story,
    paragraphs: impl IntoIterator<Item = (usize, Range<usize>)>,
) -> Vec<(Range<usize>, ProofLang)> {
    let mut runs = story.runs().peekable();
    let mut spans = Vec::new();
    for (pi, para) in paragraphs {
        let Some(format) = story.paras.get(pi) else { continue };
        let base = doc.styles.resolve_para(format).1;
        while runs.peek().is_some_and(|(r, _)| r.end <= para.start) {
            runs.next();
        }
        while let Some((run, format)) = runs.peek() {
            if run.start >= para.end {
                break;
            }
            let range = run.start.max(para.start)..run.end.min(para.end);
            if range.start < range.end {
                spans.push((range, proof_language(&doc.styles.resolve_char(&base, format).language)));
            }
            if run.end > para.end {
                break;
            }
            runs.next();
        }
    }
    spans
}

/// A paragraph's issues from the shared proofing, at story offsets: every language's spelling,
/// and of the grammar rules only Khmer's preferred forms, which are about spelling too. The
/// document's user dictionary counts. Candidates are left for [`suggest_issue`], except a
/// preferred form's, which needs no search.
fn paragraph_issues(
    story: &designcraft_doc::Story,
    user: &[String],
    suggestions: bool,
    paragraph: Range<usize>,
    spans: &[(Range<usize>, ProofLang)],
) -> Vec<ProofIssue> {
    let Some(text) = story.text.get(paragraph.clone()) else { return Vec::new() };
    let local: Vec<_> = spans
        .iter()
        .filter(|(r, _)| r.start < paragraph.end && r.end > paragraph.start)
        .map(|(r, l)| (r.start.max(paragraph.start) - paragraph.start..r.end.min(paragraph.end) - paragraph.start, *l))
        .collect();
    let known = |word: &str| user.iter().any(|w| w == word || w.eq_ignore_ascii_case(word));
    craft_proof::check_text(text, &local)
        .into_iter()
        .filter(|issue| issue.kind == IssueKind::Spelling || issue.lang == ProofLang::Km)
        .filter(|issue| text.get(issue.start..issue.end).is_some_and(|word| !known(word) && !known(word.trim_end_matches('ៗ'))))
        .map(|issue| {
            let preferred = (issue.kind == IssueKind::Grammar).then(|| issue.suggestions.first().cloned()).flatten();
            let word = text.get(issue.start..issue.end).unwrap_or_default();
            let mut found = ProofIssue {
                range: paragraph.start + issue.start..paragraph.start + issue.end,
                kind: if issue.kind == IssueKind::Spelling { "spelling" } else { "grammar" },
                message: issue.message,
                language: issue.lang.tag().into_owned(),
                lang: issue.lang,
                suggestions: Vec::new(),
                preferred,
            };
            if suggestions {
                found.suggestions = suggestions_for(&mut found, word);
            }
            found
        })
        .collect()
}

/// Proof logical paragraphs, then map unchanged byte offsets to the story. Pass false
/// for dynamic underlines or book-wide refreshes to avoid edit-distance work.
pub fn proof_issues(doc: &designcraft_doc::Document, story: &designcraft_doc::Story, user: &[String], suggestions: bool) -> Vec<ProofIssue> {
    let paragraphs = story.para_ranges();
    let spans = language_spans(doc, story, paragraphs.iter().cloned().enumerate());
    let mut out: Vec<_> = paragraphs.into_iter().flat_map(|paragraph| paragraph_issues(story, user, suggestions, paragraph, &spans)).collect();
    out.sort_by_key(|issue| (issue.range.start, issue.range.end));
    out
}

/// Dynamic proofing ranges, including Khmer preferred forms; the user dictionary counts.
/// An empty list is not a correctness claim: consult `spelling.status` for data availability.
pub fn misspellings(doc: &designcraft_doc::Document, story: &designcraft_doc::Story, user: &[String]) -> Vec<Range<usize>> {
    proof_issues(doc, story, user, false).into_iter().map(|issue| issue.range).collect()
}

fn issue_value(sid: StoryId, text: &str, issue: ProofIssue) -> Option<Value> {
    let word = text.get(issue.range.clone())?;
    Some(json!({
        "story": sid.0, "start": issue.range.start, "end": issue.range.end,
        "word": word, "suggestions": issue.suggestions, "kind": issue.kind,
        "message": issue.message, "language": issue.language,
    }))
}

fn suggest_issue(s: &mut Session, p: &Value) -> Result<Value> {
    let command = "spelling.suggest";
    let sid = StoryId(p.get("story").and_then(Value::as_u64).ok_or_else(|| bad(command, "missing story"))?);
    let offset = |key| {
        p.get(key).and_then(Value::as_u64).and_then(|n| usize::try_from(n).ok()).ok_or_else(|| bad(command, "start and end must be byte offsets"))
    };
    let range = offset("start")?..offset("end")?;
    let doc = &s.doc()?.doc;
    let story = doc.story(sid).ok_or(designcraft_doc::DocError::NoStory(sid))?;
    if range.start >= range.end || story.text.get(range.clone()).is_none() {
        return Err(bad(command, "range must address a nonempty current issue on UTF-8 character boundaries"));
    }
    // Search outward only as far as this paragraph; do not segment the book or
    // generate any other word's suggestions during selection/refresh.
    let before = story.text.get(..range.start).ok_or_else(|| bad(command, "invalid start"))?;
    let tail = story.text.get(range.start..).ok_or_else(|| bad(command, "invalid start"))?;
    let paragraph = before.rfind('\n').map_or(0, |at| at + 1)..tail.find('\n').map_or(story.len(), |at| range.start + at);
    if range.end > paragraph.end {
        return Err(bad(command, "range crosses a paragraph boundary"));
    }
    let pi = story.para_at(paragraph.start);
    let spans = language_spans(doc, story, std::iter::once((pi, paragraph.clone())));
    let mut issue = paragraph_issues(story, &doc.user_words, false, paragraph, &spans)
        .into_iter()
        .find(|issue| issue.range == range)
        .ok_or_else(|| bad(command, "range is no longer a current spelling or preferred-form issue"))?;
    let word = story.text.get(range).ok_or_else(|| bad(command, "invalid issue range"))?;
    issue.suggestions = suggestions_for(&mut issue, word);
    issue_value(sid, &story.text, issue).ok_or_else(|| bad(command, "invalid issue range"))
}

fn issue_at(s: &mut Session, p: &Value) -> Result<Value> {
    let command = "spelling.at";
    let sid = StoryId(p.get("story").and_then(Value::as_u64).ok_or_else(|| bad(command, "missing story"))?);
    let at = p.get("at").and_then(Value::as_u64).and_then(|n| usize::try_from(n).ok()).ok_or_else(|| bad(command, "at must be a byte offset"))?;
    let doc = &s.doc()?.doc;
    let story = doc.story(sid).ok_or(designcraft_doc::DocError::NoStory(sid))?;
    let (Some(before), Some(tail)) = (story.text.get(..at), story.text.get(at..)) else {
        return Err(bad(command, "at must be on a UTF-8 character boundary inside the story"));
    };
    let paragraph = before.rfind('\n').map_or(0, |i| i + 1)..tail.find('\n').map_or(story.len(), |i| at + i);
    let spans = language_spans(doc, story, std::iter::once((story.para_at(paragraph.start), paragraph.clone())));
    let Some(mut issue) =
        paragraph_issues(story, &doc.user_words, false, paragraph, &spans).into_iter().find(|i| i.range.contains(&at) || i.range.end == at)
    else {
        return Ok(Value::Null);
    };
    let word = story.text.get(issue.range.clone()).ok_or_else(|| bad(command, "invalid issue range"))?;
    issue.suggestions = suggestions_for(&mut issue, word);
    Ok(issue_value(sid, &story.text, issue).unwrap_or(Value::Null))
}

/// The candidates for one issue: its preferred form, else edit-distance suggestions.
fn suggestions_for(issue: &mut ProofIssue, word: &str) -> Vec<String> {
    match issue.preferred.take() {
        Some(preferred) => vec![preferred],
        None => craft_proof::suggest_in(word, 6, issue.lang),
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
    for sid in stories {
        let Some(story) = d.story(sid) else { continue };
        out.extend(proof_issues(d, story, &d.user_words, want_sugg).into_iter().filter_map(|issue| issue_value(sid, &story.text, issue)));
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
        assert!(r[0]["suggestions"].as_array().unwrap().iter().any(|v| v == "The"), "in the word's own capitalization");
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
