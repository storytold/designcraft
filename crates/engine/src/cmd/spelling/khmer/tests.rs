//! Engine regressions. Linguistic fixtures for repetition, preferred forms and the
//! two reported paragraphs are adapted from craft-libs crates/proof/src/km.rs
//! (MIT, revision 6be86aa0f8f7183a01d238a225e77221ce9111ea).
use serde_json::json;

use crate::{
    Session,
    cmd::spelling::{misspellings, proof_issues},
};
use designcraft_doc::{Document, Story, StoryId};

fn available() -> bool {
    let available = craft_khmer::data_available();
    if option_env!("CRAFT_KHMER_REQUIRED") == Some("1") {
        assert!(available, "required Khmer data must activate engine regressions");
    }
    available
}

fn make_story(text: &str) -> Story {
    let mut story = Story::new(StoryId(1));
    story.insert(0, text);
    story
}

fn session(text: &str) -> (Session, StoryId) {
    let mut session = Session::new();
    session.execute("file.new", &json!({})).unwrap();
    let frame = session.execute("frame.create", &json!({"rect": [36, 36, 300, 200], "content": "text", "text": text})).unwrap();
    (session, StoryId(frame["story"].as_u64().unwrap()))
}

#[test]
fn availability_is_explicit_and_missing_data_does_not_create_findings() {
    let (mut session, _) = session("ឝឝឝឝ សំដី");
    let status = session.execute("spelling.status", &json!({})).unwrap();
    assert_eq!(status["khmerAvailable"], available());
    assert_eq!(status["englishAvailable"], !designcraft_compose::hyphen::Dictionary::en_us().is_empty());
    if !available() {
        assert_eq!(session.execute("spelling.check", &json!({})).unwrap(), json!([]));
        let doc = &session.doc().unwrap().doc;
        assert!(misspellings(doc, doc.stories.values().next().unwrap(), &[]).is_empty());
    }
}

#[test]
fn unspaced_typo_crosses_format_runs_and_retains_original_offsets() {
    if !available() {
        return;
    }
    let text = "Hello 🦀 ខ្មែរកសីករខ្មែរ";
    let mut story = make_story(text);
    let start = text.find("កសីករ").unwrap();
    // A style boundary inside a word must never become a segmentation boundary.
    story.format_chars(start + "ក".len()..start + "កសី".len(), |format| format.over.font_style = Some("Bold".into()));
    let doc = Document::new(&Default::default());
    let issues = proof_issues(&doc, &story, &[], true);
    assert_eq!(issues.len(), 1, "{issues:?}");
    assert_eq!(issues[0].range, start..start + "កសីករ".len());
    assert_eq!(issues[0].language, "km");
    assert_eq!(issues[0].kind, "spelling");
    assert_eq!(issues[0].suggestions.first().map(String::as_str), Some("កសិករ"));
    assert_eq!(misspellings(&doc, &story, &[]), vec![issues[0].range.clone()]);
    assert!(proof_issues(&doc, &story, &[], false).iter().all(|i| i.suggestions.is_empty()));
    if !designcraft_compose::hyphen::Dictionary::en_us().is_empty() {
        let mixed = make_story("typograpyខ្មែរemial");
        assert_eq!(
            proof_issues(&doc, &mixed, &[], false).iter().map(|i| mixed.text.get(i.range.clone()).unwrap()).collect::<Vec<_>>(),
            ["typograpy", "emial"]
        );
    }
}

#[test]
fn preferred_phrase_and_repeated_forms_use_shared_rules_without_duplicate_marks() {
    if !available() {
        return;
    }
    let doc = Document::new(&Default::default());
    for (text, variant, replacement) in [("Hello «ពាក្យសំដីៗ»។", "ពាក្យសំដី", "ពាក្យសម្ដី"), ("ខាងវិញ្ញាណ", "ខាងវិញ្ញាណ", "ខាងឯវិញ្ញាណ"), ("ព្រិលៗ", "ព្រិលៗ", "ព្រឹលៗ")]
    {
        let mut story = make_story(text);
        let at = text.find(variant).unwrap();
        story.format_chars(at..at + variant.chars().next().unwrap().len_utf8(), |format| format.over.font_style = Some("Bold".into()));
        let issues = proof_issues(&doc, &story, &[], true);
        assert_eq!(issues.len(), 1, "{text}: {issues:?}");
        let issue = &issues[0];
        assert_eq!(text.get(issue.range.clone()), Some(variant));
        assert_eq!(issue.kind, "grammar");
        assert_eq!(issue.suggestions, [replacement]);
        assert!(!issue.message.is_empty());
        assert!(proof_issues(&doc, &story, &[variant.to_owned()], false).is_empty());
    }
    assert!(proof_issues(&doc, &make_story("ព្រឹលៗ"), &[], true).is_empty());
}

#[test]
fn explicit_languages_apply_to_whole_corrections_and_each_paragraph_base() {
    if !available() {
        return;
    }
    let mut doc = Document::new(&Default::default());
    std::sync::Arc::make_mut(&mut doc.styles).character.push(designcraft_doc::styles::CharacterStyle {
        name: "Proof disabled".into(),
        based_on: None,
        chars: designcraft_doc::CharAttrs { language: Some("[No Language]".into()), ..Default::default() },
        shortcut: String::new(),
    });
    let mut disabled = make_story("សំដី");
    disabled.format_chars(0..disabled.len(), |format| format.style = "Proof disabled".into());
    assert!(proof_issues(&doc, &disabled, &[], false).is_empty());
    for language in ["Khmer", "km-KH", "$ID/Khmer", "Cambodian"] {
        let mut story = make_story("សំដី");
        story.format_chars(0..story.len(), |format| format.over.language = Some(language.into()));
        assert_eq!(proof_issues(&doc, &story, &[], false).len(), 1, "{language}");
    }
    for text in ["ខាងវិញ្ញាណ", "ឝឝឝឝ"] {
        for language in ["[No Language]", "French", "pl-PL", "Unknown"] {
            let mut story = make_story(text);
            let split = text.chars().next().unwrap().len_utf8();
            story.format_chars(split..story.len(), |format| format.over.language = Some(language.into()));
            assert!(proof_issues(&doc, &story, &[], true).is_empty(), "{language}: {text}");
        }
    }
    // One character run spans both paragraphs; its language comes from each base.
    let mut story = make_story("សំដី\nសំដី");
    assert_eq!(story.chars.len(), 1);
    story.paras[0].chars.language = Some("[No Language]".into());
    story.paras[1].chars.language = Some("km".into());
    let issues = proof_issues(&doc, &story, &[], false);
    assert_eq!(issues.len(), 1);
    assert_eq!(issues[0].range, "សំដី\n".len()..story.len());
}

#[test]
fn repetition_and_authored_joiners_do_not_create_false_spelling_marks() {
    if !available() {
        return;
    }
    let doc = Document::new(&Default::default());
    for text in ["គ្រប់ៗគ្នា", "ខុសៗគ្នា", "អ្វីៗ", "ផ្សេងៗ", "កូនៗ", "ថ្មីៗ", "ល្អៗ", "ស្អាតៗ", "រ៉ាមៗ", "ៗ", "«គ្រប់ៗ»។", "ខុសៗ\u{200b}គ្នា", "ខុសៗ\u{2060}គ្នា"]
    {
        let story = make_story(text);
        assert!(!proof_issues(&doc, &story, &[], true).iter().any(|i| i.kind == "spelling"), "{text}");
        assert_eq!(story.text, text);
    }
    for joiner in ['\u{200b}', '\u{2060}'] {
        let text = format!("ខ្មែរ{joiner}សំដីៗ");
        let issues = proof_issues(&doc, &make_story(&text), &[], true);
        assert_eq!(issues.len(), 1);
        assert_eq!(text.get(issues[0].range.clone()), Some("សំដី"));
        let mut corrected = text.clone();
        corrected.replace_range(issues[0].range.clone(), &issues[0].suggestions[0]);
        assert_eq!(corrected, format!("ខ្មែរ{joiner}សម្ដីៗ"));
    }
}

#[test]
fn reported_paragraphs_allow_legitimate_preferred_forms_but_not_repetition_false_positives() {
    if !available() {
        return;
    }
    let doc = Document::new(&Default::default());
    for text in [
        "ព្រះជាម្ចាស់ចាត់ចែងនិងរៀបចំឋានៈឲ្យយើងគ្រប់ៗគ្នាតាមដែលទ្រង់ឃើញថាល្អនៅក្នុងព្រះនេត្ររបស់ទ្រង់។",
        "ទ្រង់បានបង្កើតរបស់ខ្លះមកឲ្យអាចកម្រើកបាន និងខ្លះទៀតមិនអាចកម្រើកបានខ្លះស្អាត និងខ្លះទៀតមិនស្អាត ហើយរបស់ទាំងអស់នោះសុទ្ធតែមានគុណសម្បត្តិខុសៗគ្នា ប៉ុន្តែគ្មាននិចលភាព ឬក៏ស្មារតីសោះឡើយ។",
    ] {
        let mut story = make_story(text);
        // Repetition and its preceding stem can have different formatting.
        let sign = text.find('ៗ').unwrap();
        story.format_chars(sign..sign + 'ៗ'.len_utf8(), |format| format.over.font_style = Some("Bold".into()));
        for language in ["English: USA", "Khmer"] {
            story.paras[0].chars.language = Some(language.into());
            let issues = proof_issues(&doc, &story, &[], true);
            assert!(!issues.iter().any(|i| i.kind == "spelling"), "{language}: {issues:?}");
            let expected = craft_khmer::grammar_matches(text);
            assert_eq!(issues.iter().map(|i| (i.range.clone(), i.suggestions[0].clone())).collect::<Vec<_>>(), expected);
            assert!(issues.iter().all(|i| i.language == "km"));
            assert_eq!(story.text, text);
        }
    }
}

#[test]
fn native_check_change_dictionary_and_undo_preserve_repetition_and_ranges() {
    if !available() {
        return;
    }
    let text = "Hello «ខ្មែៗ»។";
    let (mut session, sid) = session(text);
    session
        .edit(|doc, _| {
            let story = doc.story_mut(sid).unwrap();
            let sign = text.find('ៗ').unwrap();
            story.format_chars(sign..sign + 'ៗ'.len_utf8(), |format| format.over.font_style = Some("Bold".into()));
            Ok(())
        })
        .unwrap();
    let issues = session.execute("spelling.check", &json!({"story": sid.0})).unwrap();
    assert_eq!(issues.as_array().unwrap().len(), 1, "{issues}");
    let issue = &issues[0];
    assert_eq!(issue["word"], "ខ្មែ");
    assert_eq!(issue["start"], "Hello «".len());
    assert!(issue["suggestions"].as_array().unwrap().iter().any(|s| s == "ខ្មែរ"));
    let request = json!({"story": sid.0, "start": issue["start"], "end": issue["end"]});
    let st = session.doc().unwrap();
    let snapshot = (st.doc.clone(), st.revision, st.selection.clone(), st.history.undo.len(), st.history.redo.len(), session.journal.len());
    let lazy = session.execute("spelling.suggest", &request).unwrap();
    assert_eq!(lazy, *issue);
    let st = session.doc().unwrap();
    assert!(std::sync::Arc::ptr_eq(&snapshot.0, &st.doc));
    assert_eq!(
        (st.revision, &st.selection, st.history.undo.len(), st.history.redo.len(), session.journal.len()),
        (snapshot.1, &snapshot.2, snapshot.3, snapshot.4, snapshot.5)
    );
    session.execute("spelling.change", &json!({"story": sid.0, "start": issue["start"], "end": issue["end"], "to": "ខ្មែរ"})).unwrap();
    assert_eq!(session.doc().unwrap().doc.story(sid).unwrap().text, "Hello «ខ្មែរៗ»។");
    assert_eq!(session.doc().unwrap().doc.story(sid).unwrap().char_format_of("Hello «ខ្មែរ".len()).over.font_style.as_deref(), Some("Bold"));
    assert_eq!(session.execute("spelling.check", &json!({})).unwrap(), json!([]));
    assert!(session.execute("spelling.suggest", &request).is_err(), "corrected issue is stale");
    session.execute("edit.undo", &json!({})).unwrap();
    assert_eq!(session.doc().unwrap().doc.story(sid).unwrap().text, text);
    assert_eq!(session.execute("spelling.check", &json!({"suggestions": false})).unwrap()[0]["suggestions"], json!([]));
    session.execute("spelling.addWord", &json!({"word": "ខ្មែ"})).unwrap();
    assert_eq!(session.execute("spelling.check", &json!({})).unwrap(), json!([]));
    assert!(session.execute("spelling.suggest", &request).is_err(), "user dictionary suppresses the old issue");
    session.execute("edit.undo", &json!({})).unwrap();
    let doc = &session.doc().unwrap().doc;
    assert_eq!(misspellings(doc, doc.story(sid).unwrap(), &doc.user_words), vec!["Hello «".len().."Hello «ខ្មែ".len()]);
    for (start, end) in [(8, 9), (12, 10), (0, usize::MAX)] {
        assert!(session.execute("spelling.change", &json!({"story": sid.0, "start": start, "end": end, "to": "x"})).is_err());
        assert!(session.execute("spelling.suggest", &json!({"story": sid.0, "start": start, "end": end})).is_err());
        assert_eq!(session.doc().unwrap().doc.story(sid).unwrap().text, text);
    }
    // One selected paragraph, one target suggestion; other paragraphs may contain many errors.
    let text = format!("{}\nពាក្យសំដីៗ\n{}", "ឝឝឝឝ ".repeat(32), "ឝឝឝឝ ".repeat(32));
    let (mut session, sid) = self::session(&text);
    let start = text.find("ពាក្យសំដី").unwrap();
    let request = json!({"story": sid.0, "start": start, "end": start + "ពាក្យសំដី".len()});
    let preferred = session.execute("spelling.suggest", &request).unwrap();
    assert_eq!(preferred["kind"], "grammar");
    assert_eq!(preferred["suggestions"], json!(["ពាក្យសម្ដី"]));
    assert_eq!(preferred["word"], "ពាក្យសំដី");
    assert!(session.execute("spelling.suggest", &json!({"story": sid.0, "start": start, "end": text.len()})).is_err());
    session
        .edit(|doc, _| {
            let story = doc.story_mut(sid).unwrap();
            story.format_chars(start..start + "ពាក្យសំដី".len(), |format| format.over.language = Some("[No Language]".into()));
            Ok(())
        })
        .unwrap();
    assert!(session.execute("spelling.suggest", &request).is_err(), "language changes invalidate the issue");
}

#[test]
fn has_khmer_agrees_with_proofing_at_the_script_boundaries() {
    use crate::cmd::spelling::has_khmer;
    assert!(has_khmer("abc ក") && has_khmer("\u{17ff}") && has_khmer("\u{19e0}") && has_khmer("\u{19ff}"));
    assert!(!has_khmer("") && !has_khmer("plain text") && !has_khmer("\u{177f}\u{1800}\u{19df}\u{1a00}\u{200b}"));
}

#[test]
fn the_issue_at_a_caret_comes_with_its_suggestions_only_inside_it() {
    let (mut s, sid) = session("A jumpd fox.\nPlain line.");
    let at = |s: &mut Session, at: usize| s.execute("spelling.at", &serde_json::json!({"story": sid.0, "at": at})).unwrap();
    for inside in [2, 4, 7] {
        let issue = at(&mut s, inside);
        assert_eq!(issue["word"], "jumpd", "offset {inside}");
        assert!(!issue["suggestions"].as_array().unwrap().is_empty());
    }
    assert!(at(&mut s, 0).is_null(), "a known word has no issue");
    assert!(at(&mut s, 15).is_null(), "nor does another paragraph");
    assert!(s.execute("spelling.at", &serde_json::json!({"story": sid.0, "at": 1000})).is_err());
}
