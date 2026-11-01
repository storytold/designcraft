use serde_json::json;

use crate::Session;

#[test]
fn find_and_change_text_and_grep() {
    let mut s = Session::new();
    s.execute("file.new", &json!({})).unwrap();
    s.execute("frame.create", &json!({"rect": [36, 36, 300, 300], "content": "text", "text": "Call 555-1234 or 555-9876.\nColour and colour."}))
        .unwrap();
    let m = s.execute("find.find", &json!({"find": "colour"})).unwrap();
    assert_eq!(m.as_array().unwrap().len(), 2);
    let m = s.execute("find.find", &json!({"find": "colour", "caseSensitive": true})).unwrap();
    assert_eq!(m.as_array().unwrap().len(), 1);
    let r = s.execute("find.change", &json!({"find": r"(\d{3})-(\d{4})", "change": "($1) $2", "grep": true})).unwrap();
    assert_eq!(r["count"], 2);
    let r = s.execute("find.change", &json!({"find": "colour", "change": "color", "attrs": {"fontStyle": "Italic"}})).unwrap();
    assert_eq!(r["count"], 2);
    let d = s.execute("document.inspect", &json!({})).unwrap();
    let sid = d["stories"][0]["id"].as_u64().unwrap();
    let text = s.execute("story.get", &json!({"story": sid})).unwrap()["text"].as_str().unwrap().to_string();
    assert_eq!(text, "Call (555) 1234 or (555) 9876.\ncolor and color.");
    // Paragraph metacharacter in text mode.
    assert_eq!(s.execute("find.find", &json!({"find": ".^pc"})).unwrap().as_array().unwrap().len(), 1);
    s.execute("find.next", &json!({"find": "or"})).unwrap();
    assert!(s.doc().unwrap().selection.text.is_some());
}

#[test]
fn change_first_changes_one_match_in_the_whole_document() {
    for grep in [false, true] {
        let mut s = Session::new();
        s.execute("file.new", &json!({})).unwrap();
        for y in [36, 336] {
            s.execute("frame.create", &json!({"rect": [36, y, 300, y + 200], "content": "text", "text": "red and red"})).unwrap();
        }
        let r = s.execute("find.change", &json!({"find": "red", "change": "blue", "first": true, "grep": grep})).unwrap();
        assert_eq!(r["count"], 1);
        assert_eq!(s.execute("find.find", &json!({"find": "blue"})).unwrap().as_array().unwrap().len(), 1);
        assert_eq!(s.execute("find.find", &json!({"find": "red"})).unwrap().as_array().unwrap().len(), 3);
    }
}

fn scope_session(text: &str) -> (Session, designcraft_doc::StoryId) {
    let mut s = Session::new();
    s.execute("file.new", &json!({})).unwrap();
    let frame = s.execute("frame.create", &json!({"rect": [36, 36, 100, 100], "content": "text", "text": text})).unwrap();
    (s, designcraft_doc::StoryId(frame["story"].as_u64().unwrap()))
}

fn scope_select(s: &mut Session, sid: designcraft_doc::StoryId, a: usize, b: usize) {
    s.execute("text.select", &json!({"story": sid.0, "anchor": a, "focus": b})).unwrap();
}

fn scope_hit(s: &mut Session, p: &serde_json::Value, sid: designcraft_doc::StoryId, a: usize, b: usize) {
    assert_eq!(s.execute("find.next", p).unwrap(), json!({"story": sid.0, "start": a, "end": b}));
    let selected = s.doc().unwrap().selection.text.unwrap();
    assert_eq!((selected.story, selected.range(), selected.cell), (sid, a..b, None));
}

fn assert_document_change_scenarios() {
    for (texts, first, explicit, grep, expected, count) in [
        (vec!["red red", "red"], Some(true), false, false, vec!["blue red", "red"], 1),
        (vec!["keep", "red red", "red"], Some(true), false, false, vec!["keep", "blue red", "red"], 1),
        (vec!["keep", "other"], Some(true), false, false, vec!["keep", "other"], 0),
        (vec!["red red", "red"], Some(false), false, false, vec!["blue blue", "blue"], 3),
        (vec!["red red", "red"], None, false, false, vec!["blue blue", "blue"], 3),
        (vec!["red red", "red"], Some(true), true, false, vec!["blue red", "red"], 1),
        (vec!["red red", "red"], Some(false), true, false, vec!["blue blue", "red"], 2),
        (vec!["", "é🙂"], Some(true), false, true, vec!["X", "é🙂"], 1),
        (vec!["é", "🙂"], Some(false), false, true, vec!["XéX", "X🙂X"], 4),
    ] {
        let (mut s, sid) = scope_session(texts[0]);
        let mut sids = vec![sid];
        for text in texts.iter().skip(1) {
            let frame = s.execute("frame.create", &json!({"rect": [120, 36, 200, 100], "content": "text", "text": text})).unwrap();
            sids.push(designcraft_doc::StoryId(frame["story"].as_u64().unwrap()));
        }
        assert_eq!(s.doc().unwrap().doc.stories.keys().copied().collect::<Vec<_>>(), sids);
        // Document Change keeps canonical target order even with a later story current.
        scope_select(&mut s, *sids.last().unwrap(), 0, 0);
        let before = s.doc().unwrap().doc.clone();
        let mut p = json!({
            "find": if grep { "(?:)" } else { "red" },
            "change": if grep { "X" } else { "blue" },
            "scope": "document", "grep": grep
        });
        if let Some(first) = first {
            p["first"] = json!(first);
        }
        if explicit {
            p["story"] = json!(sid.0);
            p["scope"] = json!("selection");
        }
        if first == Some(true) && !explicit && !grep && texts.last() == Some(&"red") {
            let last = *sids.last().unwrap();
            scope_hit(&mut s, &p, last, 0, 3);
            let first_hit = sids[if texts.len() == 2 { 0 } else { 1 }];
            assert_eq!(
                s.execute("find.find", &p).unwrap(),
                json!([
                    {"story": first_hit.0, "start": 0, "end": 3, "text": "red"},
                    {"story": first_hit.0, "start": 4, "end": 7, "text": "red"},
                    {"story": last.0, "start": 0, "end": 3, "text": "red"}
                ]),
                "Count remains canonical after Next paints the later story"
            );
        }
        let selection = s.doc().unwrap().selection.clone();
        assert_eq!(s.execute("find.change", &p).unwrap(), json!({"count": count}));
        let st = s.doc().unwrap();
        assert_eq!(st.selection, selection);
        for (i, sid) in sids.iter().enumerate() {
            assert_eq!(st.doc.story(*sid).unwrap().text, expected[i]);
            if texts[i] == expected[i] {
                assert_eq!(st.doc.story(*sid), before.story(*sid), "untouched story {i} stays entirely unchanged");
            }
        }
        st.doc.check().unwrap();
    }
}

#[test]
fn find_scope_first_change_stops_after_first_matching_story_and_repairs_selection() {
    assert_document_change_scenarios();

    // Identical replacement is still one counted match, not a reason to visit another story.
    let (mut s, sid) = scope_session("red red");
    let other = s.execute("frame.create", &json!({"rect": [120, 36, 200, 100], "content": "text", "text": "red"})).unwrap();
    let other_sid = designcraft_doc::StoryId(other["story"].as_u64().unwrap());
    scope_select(&mut s, other_sid, 0, 3);
    let before = s.doc().unwrap().doc.clone();
    let selection = s.doc().unwrap().selection.clone();
    assert_eq!(s.execute("find.change", &json!({"find": "red", "change": "red", "scope": "document", "first": true})).unwrap(), json!({"count": 1}));
    assert_eq!(s.doc().unwrap().doc.story(sid).unwrap().text, "red red");
    assert_eq!(s.doc().unwrap().doc.story(other_sid), before.story(other_sid));
    assert_eq!(s.doc().unwrap().selection, selection);
    s.doc().unwrap().doc.check().unwrap();

    for reverse in [false, true] {
        let (mut s, sid) = scope_session("éX X");
        let other = s.execute("frame.create", &json!({"rect": [120, 36, 200, 100], "content": "text", "text": "X"})).unwrap();
        let other_sid = designcraft_doc::StoryId(other["story"].as_u64().unwrap());
        scope_select(&mut s, sid, if reverse { 3 } else { 2 }, if reverse { 2 } else { 3 });
        let before = s.doc().unwrap().doc.clone();
        let mut selection = s.doc().unwrap().selection.clone();
        let t = selection.text.as_mut().unwrap();
        (t.anchor, t.focus) = if reverse { (6, 2) } else { (2, 6) };
        let p = json!({"find": "X", "change": "🙂", "scope": "document", "first": true});
        assert_eq!(s.execute("find.find", &p).unwrap().as_array().unwrap().len(), 3);
        assert_eq!(s.execute("find.change", &p).unwrap(), json!({"count": 1}));
        let st = s.doc().unwrap();
        assert_eq!(st.doc.story(sid).unwrap().text, "é🙂 X");
        assert_eq!(st.doc.story(other_sid), before.story(other_sid));
        assert_eq!(st.selection, selection, "first-match exit must follow final UTF-8 repair");
        st.doc.check().unwrap();
        assert_eq!(
            s.execute("find.find", &json!({"find": "🙂", "scope": "selection"})).unwrap(),
            json!([{"story": sid.0, "start": 2, "end": 6, "text": "🙂"}])
        );
    }
}

#[test]
fn grep_in_the_documents_dialect() {
    let mut s = Session::new();
    s.execute("file.new", &json!({})).unwrap();
    s.execute("frame.create", &json!({"rect": [36, 36, 300, 300], "content": "text", "text": "Москва and Paris, 1\u{2013}2 days.\nok"})).unwrap();
    // `\u` uppercase, `\l` lowercase letter.
    let m = s.execute("find.find", &json!({"find": r"\u\l+", "grep": true, "caseSensitive": true})).unwrap();
    let words: Vec<&str> = m.as_array().unwrap().iter().map(|x| x["text"].as_str().unwrap()).collect();
    assert_eq!(words, ["Москва", "Paris"]);
    // `~=` en dash, `\h` space, `\r` paragraph end; the change uses groups, `~S` and `\r`.
    let r = s.execute("find.change", &json!({"find": r"(\d)~=(\d)\h(\l+)\.\r", "change": r"$1~=$2~S$3!\r", "grep": true})).unwrap();
    assert_eq!(r["count"], 1);
    let sid = s.execute("document.inspect", &json!({})).unwrap()["stories"][0]["id"].as_u64().unwrap();
    let text = s.execute("story.get", &json!({"story": sid})).unwrap()["text"].as_str().unwrap().to_string();
    assert!(text.starts_with("Москва and Paris, 1\u{2013}2\u{A0}days!\nok"), "{text:?}");
    // A pattern that doesn't compile is an error, not a silent no-op.
    assert!(s.execute("find.find", &json!({"find": "[[:nope:]]", "grep": true})).is_err());
    assert!(s.execute("find.change", &json!({"find": "(", "change": "", "grep": true})).is_err());
}

#[test]
fn find_scope_exact_baseline_count_next_change() {
    let mut s = Session::new();
    s.execute("file.new", &json!({})).unwrap();
    let frame = s
        .execute(
            "frame.create",
            &json!({
                "rect": [36, 36, 300, 200], "content": "text", "text": "red red"
            }),
        )
        .unwrap();
    let sid = frame["story"].as_u64().unwrap();
    s.execute("text.select", &json!({"story": sid, "anchor": 0, "focus": 3})).unwrap();
    assert_eq!(
        s.execute("find.find", &json!({"find": "red", "scope": "selection"})).unwrap(),
        json!([{"story": sid, "start": 0, "end": 3, "text": "red"}])
    );
    assert_eq!(s.execute("find.next", &json!({"find": "red", "scope": "selection"})).unwrap(), json!({"story": sid, "start": 0, "end": 3}));
    assert_eq!(s.execute("find.change", &json!({"find": "red", "change": "blue", "scope": "selection"})).unwrap(), json!({"count": 1}));
    assert_eq!(s.execute("story.get", &json!({"story": sid})).unwrap()["text"], "blue red");
}

#[test]
fn find_scope_change_growth_preserves_direction_and_full_scalar_hits() {
    for (text, find, change, changed, start, old_end, new_end) in [("a", "a", "é", "é", 0, 1, 2), ("éX", "X", "🙂", "é🙂", 2, 3, 6)] {
        for reverse in [false, true] {
            let (mut s, sid) = scope_session(text);
            // Real caret movement keeps the frame identity, unlike text.select.
            s.execute("text.move", &json!({"dir": "left", "extend": reverse})).unwrap();
            if !reverse {
                s.execute("text.move", &json!({"dir": "right", "extend": true})).unwrap();
            }
            let selected = s.doc().unwrap().selection.text.unwrap();
            assert_eq!((selected.anchor, selected.focus), if reverse { (old_end, start) } else { (start, old_end) });
            assert!(selected.frame.is_some());
            let mut expected = s.doc().unwrap().selection.clone();
            let t = expected.text.as_mut().unwrap();
            (t.anchor, t.focus) = if reverse { (new_end, start) } else { (start, new_end) };
            let p = json!({"find": find, "change": change, "scope": "selection"});
            assert_eq!(s.execute("find.find", &p).unwrap(), json!([{"story": sid.0, "start": start, "end": old_end, "text": find}]));
            assert_eq!(s.execute("find.change", &p).unwrap(), json!({"count": 1}));
            let st = s.doc().unwrap();
            assert_eq!(st.doc.story(sid).unwrap().text, changed);
            assert_eq!(st.selection, expected);
            let t = st.selection.text.unwrap();
            assert!(changed.is_char_boundary(t.anchor) && changed.is_char_boundary(t.focus));
            assert!(s.find_context.is_none());
            let p = json!({"find": change, "scope": "selection"});
            assert_eq!(s.execute("find.find", &p).unwrap(), json!([{"story": sid.0, "start": start, "end": new_end, "text": change}]));
            scope_hit(&mut s, &p, sid, start, new_end);
            assert_eq!(s.execute("find.next", &p).unwrap(), serde_json::Value::Null);
        }
    }
}

#[test]
fn find_scope_change_repairs_final_boundaries_without_remapping_valid_offsets() {
    for (text, (a, b), find, change, changed, (after_a, after_b), count) in [
        ("aX", (1, 1), "a", "é", "éX", (0, 0), 1),     // A caret stays collapsed and floors.
        ("aX", (2, 2), "a", "é", "éX", (2, 2), 1),     // A valid caret stays at its byte offset.
        ("abc", (1, 2), "abc", "🙂", "🙂", (0, 4), 1), // Both endpoints need repair.
        ("abc", (2, 1), "abc", "🙂", "🙂", (4, 0), 1),
        ("é🙂", (2, 6), "🙂", "X", "éX", (2, 3), 1), // Existing shortening behavior.
        ("é🙂", (6, 2), "🙂", "X", "éX", (3, 2), 1),
        ("red", (0, 3), "red", "blue", "blue", (0, 3), 1), // Do not extend a valid upper bound.
        ("aX", (1, 2), "a", "zz", "zzX", (1, 2), 1),       // Do not track the original X.
        ("aaX", (1, 2), "a", "é", "ééX", (0, 2), 2),       // Per-edit repair would wrongly yield 0..4.
        ("aaX", (2, 3), "a", "é", "ééX", (2, 4), 2),
        ("abc", (1, 3), "abc", "", "", (0, 0), 1),
        ("abc", (2, 3), "abc", "é", "é", (2, 2), 1), // EOF clamp may collapse a nonempty range.
    ] {
        let (mut s, sid) = scope_session(text);
        scope_select(&mut s, sid, a, b);
        let mut expected = s.doc().unwrap().selection.clone();
        let t = expected.text.as_mut().unwrap();
        (t.anchor, t.focus) = (after_a, after_b);
        assert_eq!(s.execute("find.change", &json!({"find": find, "change": change, "scope": "document"})).unwrap(), json!({"count": count}));
        let st = s.doc().unwrap();
        assert_eq!(st.doc.story(sid).unwrap().text, changed, "{text} {a}->{b}");
        assert_eq!(st.selection, expected, "{text} {a}->{b}");
        assert!(changed.is_char_boundary(after_a) && changed.is_char_boundary(after_b));
        if after_a == after_b {
            assert_eq!(
                s.execute("find.find", &json!({"find": "(?:)", "grep": true, "scope": "selection"})).unwrap(),
                json!([{"story": sid.0, "start": after_a, "end": after_a, "text": ""}])
            );
        }
    }
}

#[test]
fn find_scope_change_other_story_and_no_hits_preserve_selection() {
    for explicit in [false, true] {
        let (mut s, sid) = scope_session("éX");
        let frame = s.doc().unwrap().selection.text.unwrap().frame.unwrap();
        let other = s.execute("frame.create", &json!({"rect": [120, 36, 200, 100], "content": "text", "text": "a"})).unwrap();
        let other_sid = designcraft_doc::StoryId(other["story"].as_u64().unwrap());
        scope_select(&mut s, sid, 3, 2);
        let before = s.doc().unwrap().selection.clone();
        let mut p = json!({"find": "a", "change": "🙂", "scope": "document"});
        if explicit {
            p["story"] = json!(other_sid.0);
        }
        assert_eq!(s.execute("find.change", &p).unwrap(), json!({"count": 1}));
        assert_eq!(s.doc().unwrap().doc.story(sid).unwrap().text, "éX");
        assert_eq!(s.doc().unwrap().doc.story(other_sid).unwrap().text, "🙂");
        assert_eq!(s.doc().unwrap().selection, before);
        assert_eq!(s.execute("find.change", &p).unwrap(), json!({"count": 0}));
        assert_eq!(s.doc().unwrap().selection, before);
        assert_eq!(s.doc().unwrap().doc.story(sid).unwrap().text, "éX");
        assert_eq!(s.doc().unwrap().doc.story(other_sid).unwrap().text, "🙂");

        s.execute("selection.set", &json!({"ids": [frame.0]})).unwrap();
        let before = s.doc().unwrap().selection.clone();
        assert!(before.text.is_none());
        p["find"] = json!("🙂");
        p["change"] = json!("é");
        assert_eq!(s.execute("find.change", &p).unwrap(), json!({"count": 1}));
        assert_eq!(s.doc().unwrap().selection, before);
        assert_eq!(s.doc().unwrap().doc.story(sid).unwrap().text, "éX");
        assert_eq!(s.doc().unwrap().doc.story(other_sid).unwrap().text, "é");
    }
}

#[test]
fn find_scope_mixed_grep_matches_count_next_and_change() {
    for (text, pattern, expected, changed) in [
        ("a", "a*", vec![(0, 1)], "X"),
        ("aa", "a|(?:)", vec![(0, 1), (1, 2)], "XX"),
        ("ab", "a*", vec![(0, 1), (2, 2)], "XbX"),
        ("éa", "é|(?:)", vec![(0, 2), (3, 3)], "XaX"),
    ] {
        let (mut s, sid) = scope_session(text);
        scope_select(&mut s, sid, 0, text.len());
        let p = json!({"find": pattern, "grep": true, "scope": "selection", "change": "X"});
        let counted = s.execute("find.find", &p).unwrap();
        let ranges: Vec<_> =
            counted.as_array().unwrap().iter().map(|m| (m["start"].as_u64().unwrap() as usize, m["end"].as_u64().unwrap() as usize)).collect();
        assert_eq!(ranges, expected, "{pattern} on {text}");
        for (a, b) in &expected {
            scope_hit(&mut s, &p, sid, *a, *b);
        }
        for _ in 0..2 {
            assert_eq!(s.execute("find.next", &p).unwrap(), serde_json::Value::Null, "{pattern} on {text}");
        }
        assert_eq!(s.execute("find.find", &p).unwrap(), counted);
        assert_eq!(s.execute("find.change", &p).unwrap()["count"], expected.len());
        assert_eq!(s.doc().unwrap().doc.story(sid).unwrap().text, changed);
    }
}

#[test]
fn find_scope_capture_replacement_and_first_keep_original_domain() {
    let (mut s, sid) = scope_session("a1 a2 | a3");
    scope_select(&mut s, sid, 0, 5);
    let p = json!({"find": r"a(\d)", "grep": true, "scope": "selection", "change": "b$1", "first": true});
    scope_hit(&mut s, &p, sid, 0, 2);
    scope_hit(&mut s, &p, sid, 3, 5);
    assert_eq!(s.execute("find.change", &p).unwrap()["count"], 1);
    assert_eq!(s.doc().unwrap().doc.story(sid).unwrap().text, "b1 a2 | a3");
}

#[test]
fn find_scope_selection_retains_nonempty_hits_count_change_and_native_text() {
    for (a, b) in [(0, 7), (7, 0)] {
        let (mut s, sid) = scope_session("red red | red");
        scope_select(&mut s, sid, a, b);
        let p = json!({"find": "red", "change": "blue", "scope": "selection", "attrs": {"fontStyle": "Italic"}});
        let before = s.doc().unwrap().doc.clone();
        let selection = s.doc().unwrap().selection.clone();
        let revision = s.doc().unwrap().revision;
        let history = s.doc().unwrap().history.undo.len();
        let strong = std::sync::Arc::strong_count(&before);
        assert_eq!(s.execute("find.find", &p).unwrap().as_array().unwrap().len(), 2);
        assert_eq!(s.doc().unwrap().selection, selection);
        assert_eq!(s.doc().unwrap().revision, revision);
        assert_eq!(std::sync::Arc::strong_count(&before), strong, "search stores weak document identity");
        scope_hit(&mut s, &p, sid, 0, 3);
        scope_hit(&mut s, &p, sid, 4, 7);
        assert_eq!(s.execute("find.next", &p).unwrap(), serde_json::Value::Null);
        assert_eq!(s.execute("find.next", &p).unwrap(), serde_json::Value::Null);
        assert_eq!(s.execute("find.find", &p).unwrap().as_array().unwrap().len(), 2);
        assert!(std::sync::Arc::ptr_eq(&before, &s.doc().unwrap().doc));
        assert_eq!(s.doc().unwrap().history.undo.len(), history);
        assert_eq!(s.execute("find.change", &p).unwrap()["count"], 2);
        let changed = s.doc().unwrap().doc.story(sid).unwrap().clone();
        assert_eq!(changed.text, "blue blue | red");
        assert_eq!(changed.char_format_at(0).over.font_style.as_deref(), Some("Italic"));
        assert_eq!(s.doc().unwrap().history.undo.len(), history + 1);
        assert!(s.find_context.is_none());
        s.execute("edit.undo", &json!({})).unwrap();
        assert_eq!(s.doc().unwrap().doc.story(sid).unwrap().text, "red red | red");
        assert!(s.find_context.is_none());
        s.execute("edit.redo", &json!({})).unwrap();
        assert_eq!(s.doc().unwrap().doc.story(sid).unwrap(), &changed);
        let native = s.execute("file.serialize", &json!({})).unwrap();
        s.execute("file.openBytes", &json!({"name": "scope.designcraft", "base64": native["base64"]})).unwrap();
        assert_eq!(serde_json::to_value(s.doc().unwrap().doc.story(sid).unwrap()).unwrap(), serde_json::to_value(&changed).unwrap());
        assert!(s.find_context.is_none());
    }
}

#[test]
fn find_scope_matcher_restart_replacement_continuation_and_external_reselection() {
    let (mut s, sid) = scope_session("red blue red | blue red");
    scope_select(&mut s, sid, 0, 12);
    let mut p = json!({"find": "red", "scope": "selection", "change": "one"});
    scope_hit(&mut s, &p, sid, 0, 3);
    p["change"] = json!("two");
    p["attrs"] = json!({"fontStyle": "Italic"});
    scope_hit(&mut s, &p, sid, 9, 12);
    p["find"] = json!("blue");
    scope_hit(&mut s, &p, sid, 4, 8);
    p["find"] = json!("red");
    scope_hit(&mut s, &p, sid, 0, 3);
    p["caseSensitive"] = json!(true);
    scope_hit(&mut s, &p, sid, 0, 3);
    p["grep"] = json!(true);
    scope_hit(&mut s, &p, sid, 0, 3); // GREP alone changes matcher identity.
    p["find"] = json!("r.d");
    scope_hit(&mut s, &p, sid, 0, 3);
    p["wholeWord"] = json!(true);
    scope_hit(&mut s, &p, sid, 0, 3);
    let hit = s.doc().unwrap().selection.text.unwrap();
    scope_select(&mut s, sid, hit.anchor, hit.focus);
    assert_eq!(s.execute("find.find", &p).unwrap().as_array().unwrap().len(), 1, "same-range external reselection starts a new domain");
    scope_hit(&mut s, &p, sid, 0, 3);
    assert_eq!(s.execute("find.next", &p).unwrap(), serde_json::Value::Null);
    scope_select(&mut s, sid, 20, 23);
    p["find"] = json!("red");
    scope_hit(&mut s, &p, sid, 20, 23);
}

#[test]
fn find_scope_word_reselection_at_same_hit_recaptures_bounds() {
    let (mut s, sid) = scope_session("red red | red");
    scope_select(&mut s, sid, 0, 7);
    let p = json!({"find": "red", "change": "blue", "scope": "selection"});
    assert_eq!(s.execute("find.find", &p).unwrap().as_array().unwrap().len(), 2);
    scope_hit(&mut s, &p, sid, 0, 3);
    let selected = s.doc().unwrap().selection.clone();
    let frame = selected.text.unwrap().frame.unwrap();
    let point = {
        let st = s.doc().unwrap();
        let cs = s.cache.get(&st.doc, sid, None);
        let (fi, x, baseline, ascent, _) = designcraft_compose::caret(&cs, 1).unwrap();
        let inner = designcraft_geom::Point::new(x, baseline - ascent * 0.5);
        assert_eq!(cs.frames[fi].frame, frame);
        assert_eq!(designcraft_compose::hit(&cs, fi, inner), Some(1), "real point lies inside the current hit");
        let loc = st.doc.find(frame).unwrap();
        st.doc.parent_xf(&loc) * st.doc.text_xf(st.doc.item(frame).unwrap()) * inner
    };
    let before = s.doc().unwrap().doc.clone();
    let revision = s.doc().unwrap().revision;
    let history = s.doc().unwrap().history.undo.len();
    s.execute("text.selectWord", &json!({"frame": frame.0, "point": [point.x, point.y]})).unwrap();
    assert!(!s.text_drag, "explicit word reselection does not start a text drag");
    assert_eq!(s.doc().unwrap().selection, selected, "same full selection, including frame");
    assert!(s.doc().unwrap().revision > revision, "a new revision");
    assert!(std::sync::Arc::ptr_eq(&before, &s.doc().unwrap().doc));
    assert_eq!(s.doc().unwrap().history.undo.len(), history);
    assert!(s.find_context.is_none());
    assert_eq!(s.execute("find.find", &p).unwrap(), json!([{"story": sid.0, "start": 0, "end": 3, "text": "red"}]));
    assert_eq!(s.execute("find.change", &p).unwrap(), json!({"count": 1}));
    assert_eq!(s.doc().unwrap().doc.story(sid).unwrap().text, "blue red | red");
    assert_eq!(s.doc().unwrap().history.undo.len(), history + 1);
    s.execute("edit.undo", &json!({})).unwrap();
    assert_eq!(s.doc().unwrap().doc.story(sid).unwrap().text, "red red | red");
    s.execute("edit.redo", &json!({})).unwrap();
    assert_eq!(s.doc().unwrap().doc.story(sid).unwrap().text, "blue red | red");
}

#[test]
fn find_scope_story_explicit_story_document_order_and_scope_changes() {
    let (mut s, a) = scope_session("black red");
    let other = s.execute("frame.create", &json!({"rect": [150, 36, 100, 100], "content": "text", "text": "red"})).unwrap();
    let b = designcraft_doc::StoryId(other["story"].as_u64().unwrap());
    scope_select(&mut s, a, 0, 9);
    let story = json!({"find": "red", "scope": "story"});
    assert_eq!(s.execute("find.next", &story).unwrap(), serde_json::Value::Null, "Story does not enter B or wrap to its prefix");
    scope_select(&mut s, b, 0, 0);
    let explicit = json!({"find": "black", "story": a.0, "scope": "selection"});
    scope_hit(&mut s, &explicit, a, 0, 5);
    let selection = s.doc().unwrap().selection.clone();
    assert!(s.execute("find.next", &json!({"find": "red", "story": u64::MAX})).is_err());
    assert_eq!(s.doc().unwrap().selection, selection);
    scope_select(&mut s, a, 0, 9);
    scope_hit(&mut s, &json!({"find": "red"}), b, 0, 3);
    let counted = s.execute("find.find", &json!({"find": "red"})).unwrap();
    assert_eq!(counted[0]["story"], a.0, "Count retains canonical story order");
    assert_eq!(counted[1]["story"], b.0);
    scope_select(&mut s, a, 0, 9);
    scope_hit(&mut s, &json!({"find": "red", "scope": "selection"}), a, 6, 9);
    assert_eq!(s.execute("find.next", &story).unwrap(), serde_json::Value::Null, "scope change uses the current caret");
    s.execute("selection.set", &json!({"ids": []})).unwrap();
    for command in ["find.find", "find.next", "find.change"] {
        assert!(s.execute(command, &json!({"find": "red", "change": "blue", "scope": "selection"})).is_err());
    }
}

#[test]
fn find_scope_substring_context_utf8_zero_width_and_invalid_ranges() {
    let (mut s, sid) = scope_session("scat é🙂 red");
    scope_select(&mut s, sid, 1, 4);
    let p = json!({"find": "cat", "wholeWord": true, "scope": "selection"});
    scope_hit(&mut s, &p, sid, 1, 4);
    assert_eq!(s.execute("find.find", &p).unwrap().as_array().unwrap().len(), 1);
    scope_select(&mut s, sid, 1, 4);
    let p = json!({"find": "^c|^a", "grep": true, "scope": "selection"});
    scope_hit(&mut s, &p, sid, 1, 2);
    assert_eq!(s.execute("find.next", &p).unwrap(), serde_json::Value::Null, "cursor does not redefine ^");
    scope_select(&mut s, sid, 5, 11);
    let empty = json!({"find": "(?:)", "grep": true, "scope": "selection"});
    for pos in [5, 7, 11] {
        scope_hit(&mut s, &empty, sid, pos, pos);
    }
    for _ in 0..2 {
        assert_eq!(s.execute("find.next", &empty).unwrap(), serde_json::Value::Null);
    }
    assert_eq!(s.execute("find.find", &empty).unwrap().as_array().unwrap().len(), 3);
    scope_select(&mut s, sid, 7, 7);
    scope_hit(&mut s, &empty, sid, 7, 7);
    assert_eq!(s.execute("find.next", &empty).unwrap(), serde_json::Value::Null);
    scope_select(&mut s, sid, 12, 15);
    let nonempty = json!({"find": "red", "change": "blue", "scope": "selection"});
    scope_hit(&mut s, &nonempty, sid, 12, 15);
    assert_eq!(s.execute("find.change", &nonempty).unwrap()["count"], 1);
    assert_eq!(s.doc().unwrap().doc.story(sid).unwrap().text, "scat é🙂 blue");
    for range in [6..7, 0..usize::MAX] {
        let st = s.doc_mut().unwrap();
        st.selection =
            designcraft_doc::Selection::text(designcraft_doc::TextSel { story: sid, anchor: range.start, focus: range.end, frame: None, cell: None });
        st.bump_revision();
        for command in ["find.find", "find.next", "find.change"] {
            let before = s.doc().unwrap().doc.clone();
            let err = s.execute(command, &json!({"find": "x", "change": "y", "scope": "selection"})).unwrap_err();
            assert!(matches!(err, crate::EngineError::BadParams { .. }), "{err}");
            assert!(std::sync::Arc::ptr_eq(&before, &s.doc().unwrap().doc));
        }
    }
}

#[test]
fn find_scope_invalid_query_keeps_continuation_and_cross_boundary_nonmatches() {
    let (mut s, sid) = scope_session("red red | red");
    scope_select(&mut s, sid, 0, 7);
    let p = json!({"find": "red", "scope": "selection"});
    scope_hit(&mut s, &p, sid, 0, 3);
    let selected = s.doc().unwrap().selection.clone();
    assert!(s.execute("find.next", &json!({"find": "[", "grep": true, "scope": "selection"})).is_err());
    assert_eq!(s.doc().unwrap().selection, selected);
    scope_hit(&mut s, &p, sid, 4, 7);
    assert_eq!(s.execute("find.find", &json!({"find": "red |", "scope": "selection"})).unwrap(), json!([]));
}

#[test]
fn find_scope_document_switch_edit_and_reset() {
    let (mut s, sid) = scope_session("red red | red");
    scope_select(&mut s, sid, 0, 7);
    let p = json!({"find": "red", "scope": "selection"});
    scope_hit(&mut s, &p, sid, 0, 3);
    s.execute("file.new", &json!({})).unwrap();
    s.set_active(0);
    assert_eq!(s.execute("find.find", &p).unwrap().as_array().unwrap().len(), 1);
    scope_select(&mut s, sid, 0, 7);
    scope_hit(&mut s, &p, sid, 0, 3);
    s.execute("story.setText", &json!({"story": sid.0, "text": "red"})).unwrap();
    assert!(s.find_context.is_none());
    assert_eq!(s.execute("find.find", &p).unwrap().as_array().unwrap().len(), 1);
    s.reset_find_scope();
    assert!(s.find_context.is_none());
}

#[test]
fn find_scope_real_preview_restores_before_resolving_bounds() {
    let (mut s, sid) = scope_session("");
    s.execute(
        "data.source.select",
        &json!({
            "bytes": crate::cmd::base64_encode(b"x\nred red | red\n"), "name": "scope.csv"
        }),
    )
    .unwrap();
    s.execute("data.placeholder.add", &json!({"field": "x", "story": sid.0, "at": 0})).unwrap();
    let template = s.doc().unwrap().doc.clone();
    assert_eq!(template.story(sid).unwrap().text, "<<x>>");
    s.execute("data.preview", &json!({"record": 1})).unwrap();
    assert_eq!(s.doc().unwrap().doc.story(sid).unwrap().text, "red red | red");
    // Install a viewport selection in the real preview. text.select is journaled
    // and would itself restore the template before selecting.
    let st = s.doc_mut().unwrap();
    st.selection = designcraft_doc::Selection::text(designcraft_doc::TextSel { story: sid, anchor: 0, focus: 7, frame: None, cell: None });
    st.bump_revision();
    let p = json!({"find": "red", "change": "blue", "scope": "selection"});
    assert_eq!(s.execute("find.find", &p).unwrap().as_array().unwrap().len(), 2);
    let history = s.doc().unwrap().history.undo.len();
    let err = s.execute("find.change", &p).unwrap_err();
    assert!(matches!(err, crate::EngineError::BadParams { .. }), "{err}");
    assert!(std::sync::Arc::ptr_eq(&template, &s.doc().unwrap().doc));
    assert_eq!(s.doc().unwrap().doc.story(sid).unwrap().text, "<<x>>");
    assert_eq!(s.doc().unwrap().history.undo.len(), history);
    assert!(s.doc().unwrap().preview_stash.is_none());
    assert!(s.find_context.is_none());
    // Valid fresh bounds are allowed after restoration; no cached preview range
    // survives. A document query can see the preview, but Next sees the template.
    s.execute("data.preview", &json!({"record": 1})).unwrap();
    let document = json!({"find": "red"});
    assert_eq!(s.execute("find.find", &document).unwrap().as_array().unwrap().len(), 3);
    assert_eq!(s.execute("find.next", &document).unwrap(), serde_json::Value::Null);
    assert!(std::sync::Arc::ptr_eq(&template, &s.doc().unwrap().doc));
    assert_eq!(s.execute("find.find", &document).unwrap(), json!([]));
    assert_eq!(s.doc().unwrap().history.undo.len(), history);
}

#[test]
fn document_scope_searches_and_changes_table_cells() {
    let (mut s, sid) = scope_session("Ore 12.15 here");
    scope_select(&mut s, sid, 0, 0);
    let table = s.execute("table.insert", &json!({"rows": 1, "cols": 2})).unwrap()["table"].as_u64().unwrap();
    s.execute("table.setCell", &json!({"row": 0, "col": 1, "text": "Ore 12.15"})).unwrap();
    // The caret sits in the first cell, so Find Next goes to the second cell, then the story.
    let p = json!({"find": "Ore 12.15", "scope": "document"});
    let cell = designcraft_doc::CellAddr { table, row: 0, col: 1 };
    assert_eq!(s.execute("find.next", &p).unwrap(), json!({"story": sid.0, "start": 0, "end": 9, "cell": {"table": table, "row": 0, "col": 1}}));
    assert_eq!(s.doc().unwrap().selection.text.map(|t| (t.story, t.range(), t.cell)), Some((sid, 0..9, Some(cell))));
    let next = s.execute("find.next", &p).unwrap();
    assert!(next.get("start").is_some() && next.get("cell").is_none(), "{next}");
    let hits = s.execute("find.find", &p).unwrap();
    let hits = hits.as_array().unwrap();
    assert_eq!(hits.len(), 2);
    let in_cell: Vec<_> = hits.iter().filter(|h| h.get("cell").is_some()).collect();
    assert_eq!(in_cell.len(), 1);
    assert_eq!((in_cell[0]["cell"]["row"].as_u64(), in_cell[0]["cell"]["col"].as_u64()), (Some(0), Some(1)));
    let n = s.execute("find.change", &json!({"find": "Ore 12.15", "change": "Ore 13.00", "scope": "document"})).unwrap();
    assert_eq!(n["count"], 2);
    assert!(s.execute("find.find", &json!({"find": "12.15", "scope": "document"})).unwrap().as_array().unwrap().is_empty());
    assert_eq!(s.execute("find.find", &json!({"find": "13.00", "scope": "document"})).unwrap().as_array().unwrap().len(), 2);
}

/// Change (`found`) on text Find Next found in a table cell changes it there, leaves the
/// same text in the story alone, and Find Next goes on after it.
#[test]
fn change_changes_the_text_find_next_found_in_a_cell() {
    let (mut s, sid) = scope_session("red here");
    scope_select(&mut s, sid, 0, 0);
    let table = s.execute("table.insert", &json!({"rows": 1, "cols": 2})).unwrap()["table"].as_u64().unwrap();
    s.execute("table.setCell", &json!({"row": 0, "col": 1, "text": "red"})).unwrap();
    let p = json!({"find": "red", "change": "blue", "scope": "document"});
    let hit = s.execute("find.next", &p).unwrap();
    assert_eq!(hit["cell"], json!({"table": table, "row": 0, "col": 1}), "{hit}");
    let mut change = p.clone();
    change["found"] = json!(true);
    assert_eq!(s.execute("find.change", &change).unwrap()["count"], 1);
    let t = s.execute("table.get", &json!({"table": table})).unwrap();
    assert_eq!(t["cells"][1]["text"], "blue");
    assert!(s.doc().unwrap().doc.story(sid).unwrap().text.contains("red here"), "the story's red stays");
    let next = s.execute("find.next", &p).unwrap();
    assert!(next.get("cell").is_none() && next.get("start").is_some(), "on to the story: {next}");
}

fn child_scope_session(note: bool, child: &str) -> (Session, designcraft_doc::StoryId, designcraft_doc::TextSel) {
    let (mut s, sid) = scope_session("red");
    scope_select(&mut s, sid, 3, 3);
    if note {
        s.execute("footnote.insert", &json!({"text": child})).unwrap();
    } else {
        s.execute("table.insert", &json!({"rows": 1, "cols": 1})).unwrap();
        s.execute("text.insert", &json!({"text": child, "raw": true})).unwrap();
    }
    let selected = s.doc().unwrap().selection.text.unwrap();
    (s, sid, selected)
}

/// Child-story scopes are rejected by every find command, leaving the document untouched.
fn assert_child_scope_requests_rejected(s: &mut Session) {
    let before = s.doc().unwrap().doc.clone();
    let selection = s.doc().unwrap().selection.clone();
    let history = s.doc().unwrap().history.undo.len();
    let revision = s.doc().unwrap().revision;
    let journal = s.journal.clone();
    for scope in ["story", "selection"] {
        for command in ["find.find", "find.next", "find.change"] {
            let err = s.execute(command, &json!({"find": "red", "change": "blue", "scope": scope})).unwrap_err();
            assert!(matches!(err, crate::EngineError::BadParams { .. }), "{err}");
            assert!(err.to_string().contains("not supported"));
            assert!(std::sync::Arc::ptr_eq(&before, &s.doc().unwrap().doc));
            assert_eq!(s.doc().unwrap().selection, selection);
            assert_eq!(s.doc().unwrap().history.undo.len(), history);
            assert_eq!(s.doc().unwrap().revision, revision);
            assert_eq!(s.journal, journal);
        }
    }
}

#[test]
fn find_scope_children_error_atomically_and_parent_requests_stay_top_level() {
    for note in [false, true] {
        for child in ["red", "red x", "é red red red red red"] {
            let (mut s, sid, mut selected) = child_scope_session(note, child);
            selected.anchor = 0;
            selected.focus = child.len();
            s.doc_mut().unwrap().selection = designcraft_doc::Selection::text(selected);
            assert_child_scope_requests_rejected(&mut s);
            let selection = s.doc().unwrap().selection.clone();
            for explicit in [false, true] {
                let mut p = json!({"find": "red", "change": "blue"});
                if explicit {
                    p["story"] = json!(sid.0);
                    p["scope"] = json!("selection");
                }
                // Document scope also covers table cells (#1624); an explicit story and footnotes stay top-level.
                let in_cell = if explicit || note { 0 } else { child.matches("red").count() };
                let changed = if in_cell > 0 { child.replace("red", "blue") } else { child.to_string() };
                s.doc_mut().unwrap().selection = selection.clone();
                assert_eq!(s.execute("find.find", &p).unwrap().as_array().unwrap().len(), 1 + in_cell);
                scope_hit(&mut s, &p, sid, 0, 3);
                s.doc_mut().unwrap().selection = selection.clone();
                assert_eq!(s.execute("find.change", &p).unwrap()["count"], 1 + in_cell);
                assert_eq!(s.doc().unwrap().doc.text_story(sid, selected.cell).unwrap().text, changed);
                assert_eq!(s.doc().unwrap().selection, selection, "parent replacement does not clamp child offsets");
                p["find"] = json!("blue");
                p["change"] = json!("red");
                s.execute("find.change", &p).unwrap();
            }
        }
    }
    let (mut s, _, _) = child_scope_session(false, "red");
    s.execute("table.selectTable", &json!({})).unwrap();
    for scope in ["story", "selection"] {
        for command in ["find.find", "find.next", "find.change"] {
            assert!(s.execute(command, &json!({"find": "red", "change": "blue", "scope": scope})).unwrap_err().to_string().contains("not supported"));
        }
    }
}

#[test]
fn find_scope_threaded_overset_story_uses_logical_offsets() {
    let text = format!("{}red", "word ".repeat(100));
    let (mut s, sid) = scope_session(&text);
    let a = s.doc().unwrap().doc.story(sid).unwrap().frames[0];
    let next = s.execute("frame.create", &json!({"rect": [150, 36, 20, 1], "content": "text"})).unwrap();
    let b = designcraft_doc::ItemId(next["id"].as_u64().unwrap());
    s.edit(|d, _| Ok(d.thread(a, b)?)).unwrap();
    assert_eq!(s.doc().unwrap().doc.story(sid).unwrap().frames.len(), 2);
    scope_select(&mut s, sid, 0, 0);
    scope_hit(&mut s, &json!({"find": "red", "scope": "story"}), sid, 500, 503);
    assert_eq!(s.execute("find.next", &json!({"find": "red", "scope": "story"})).unwrap(), serde_json::Value::Null);
}

#[test]
fn find_scope_zero_width_story_explicit_and_document_endpoints_terminate() {
    for scope in ["story", "explicit", "document"] {
        let (mut s, sid) = scope_session("é🙂");
        scope_select(&mut s, sid, 0, 0);
        let mut p = json!({"find": "(?:)", "grep": true, "scope": scope});
        if scope == "explicit" {
            p["story"] = json!(sid.0);
        }
        for pos in [0, 2, 6] {
            scope_hit(&mut s, &p, sid, pos, pos);
        }
        assert_eq!(s.execute("find.next", &p).unwrap(), serde_json::Value::Null);
        assert_eq!(s.execute("find.next", &p).unwrap(), serde_json::Value::Null);
    }
}

#[cfg(not(target_arch = "wasm32"))]
#[test]
fn find_scope_revert_releases_context_with_same_document_uid() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("scope.designcraft");
    let (mut s, sid) = scope_session("red red | red");
    s.execute("file.save", &json!({"path": path.to_string_lossy()})).unwrap();
    scope_select(&mut s, sid, 0, 7);
    let p = json!({"find": "red", "scope": "selection"});
    scope_hit(&mut s, &p, sid, 0, 3);
    let uid = s.doc().unwrap().uid;
    s.execute("story.setText", &json!({"story": sid.0, "text": "red red"})).unwrap();
    scope_select(&mut s, sid, 0, 7);
    scope_hit(&mut s, &p, sid, 0, 3);
    s.execute("file.revert", &json!({})).unwrap();
    assert_eq!(s.doc().unwrap().uid, uid);
    assert!(s.find_context.is_none());
    assert_eq!(s.doc().unwrap().doc.story(sid).unwrap().text, "red red | red");
    assert!(s.execute("find.find", &p).is_err(), "revert's empty selection cannot inherit a previous range");
}

#[test]
fn change_format_styles_the_found_text_and_keeps_it() {
    // The course's chapter 22: every lowercase "eggs" gets the character style "eggs", with
    // Change to empty, so the words stay.
    let mut s = Session::new();
    s.execute("file.new", &json!({})).unwrap();
    let text = "Poached Eggs\nBreak eggs; cook until eggs are set.";
    let r = s.execute("frame.create", &json!({"rect": [36, 36, 300, 300], "content": "text", "text": text})).unwrap();
    let sid = designcraft_doc::StoryId(r["story"].as_u64().unwrap());
    s.execute("style.character.create", &json!({"name": "eggs", "chars": {"fontStyle": "Bold", "fill": "C=15 M=100 Y=100 K=0"}})).unwrap();
    s.execute("style.paragraph.create", &json!({"name": "Method"})).unwrap();
    let undo = s.doc().unwrap().history.undo.len();
    let r = s.execute("find.change", &json!({"find": "eggs", "change": "", "caseSensitive": true, "charStyle": "eggs"})).unwrap();
    assert_eq!(r["count"], 2);
    assert_eq!(s.doc().unwrap().history.undo.len(), undo + 1, "one undo step");
    let style_at = |s: &Session, at: usize| s.doc().unwrap().doc.story(sid).unwrap().format_after(at).style.clone();
    let story = s.doc().unwrap().doc.story(sid).unwrap().clone();
    assert_eq!(story.text, text, "the words stay");
    for at in [19, 36] {
        assert_eq!(&story.text[at..at + 4], "eggs");
        assert_eq!(style_at(&s, at), "eggs");
    }
    assert_eq!(style_at(&s, 8), designcraft_doc::NO_CHAR_STYLE, "case sensitive: not \"Eggs\"");
    assert_eq!(style_at(&s, 25), designcraft_doc::NO_CHAR_STYLE, "only the found text");
    // A paragraph style goes to the found text's paragraphs, with attributes over the text.
    s.execute("find.change", &json!({"find": "cook", "change": "", "paraStyle": "Method", "attrs": {"underline": true}})).unwrap();
    let story = s.doc().unwrap().doc.story(sid).unwrap().clone();
    assert_eq!((story.paras[0].style.as_str(), story.paras[1].style.as_str()), (designcraft_doc::BASIC_PARAGRAPH, "Method"));
    assert_eq!(story.format_after(25).over.underline, Some(true));
    assert_eq!(story.format_after(19).style, "eggs", "the other text keeps its character style");
    // Change to and a Change Format: the new text is formatted.
    s.execute("find.change", &json!({"find": "set", "change": "firm", "charStyle": "eggs"})).unwrap();
    assert_eq!(style_at(&s, s.doc().unwrap().doc.story(sid).unwrap().text.find("firm").unwrap()), "eggs");
    // No such style: refused, nothing changed.
    let before = s.doc().unwrap().doc.clone();
    assert!(s.execute("find.change", &json!({"find": "eggs", "change": "", "charStyle": "Nope"})).is_err());
    assert!(s.execute("find.change", &json!({"find": "eggs", "change": "", "paraStyle": "Nope"})).is_err());
    assert_eq!(s.doc().unwrap().doc, before);
    // A null attribute is a Change Format too: the found text stays and loses that override.
    let before = s.doc().unwrap().doc.story(sid).unwrap().clone();
    s.execute("find.change", &json!({"find": "cook", "change": "", "attrs": {"underline": null}})).unwrap();
    let story = s.doc().unwrap().doc.story(sid).unwrap().clone();
    assert_eq!(story.text, before.text, "the words stay");
    assert_eq!(story.format_after(25).over.underline, None);
    // With no Change Format, an empty Change to still deletes the found text.
    s.execute("find.change", &json!({"find": "Break ", "change": ""})).unwrap();
    assert!(!s.doc().unwrap().doc.story(sid).unwrap().text.contains("Break"));
}

#[test]
fn a_find_format_finds_only_text_with_it() {
    let mut s = Session::new();
    s.execute("file.new", &json!({})).unwrap();
    let r =
        s.execute("frame.create", &json!({"rect": [36, 36, 300, 300], "content": "text", "text": "Break eggs; cook eggs.\nEggs and eggs"})).unwrap();
    let sid = r["story"].as_u64().unwrap();
    s.execute("style.character.create", &json!({"name": "eggs", "chars": {"fontStyle": "Bold"}})).unwrap();
    s.execute("style.paragraph.create", &json!({"name": "Method"})).unwrap();
    for (a, b) in [(6, 10), (12, 16)] {
        s.execute("text.select", &json!({"story": sid, "anchor": a, "focus": b})).unwrap();
        s.execute("style.character.apply", &json!({"name": "eggs"})).unwrap();
    }
    s.execute("text.select", &json!({"story": sid, "anchor": 25, "focus": 25})).unwrap();
    s.execute("style.paragraph.apply", &json!({"name": "Method"})).unwrap();
    let found = |s: &mut Session, p: serde_json::Value| -> Vec<(u64, u64)> {
        let m = s.execute("find.find", &p).unwrap();
        m.as_array().unwrap().iter().map(|m| (m["start"].as_u64().unwrap(), m["end"].as_u64().unwrap())).collect()
    };
    // "eggs" where it has the character style: the first only.
    assert_eq!(found(&mut s, json!({"find": "eggs", "findFormat": {"charStyle": "eggs"}})), [(6, 10)]);
    // Nothing in Find what: each stretch of text in the style.
    assert_eq!(found(&mut s, json!({"find": "", "findFormat": {"charStyle": "eggs"}})), [(6, 10), (12, 16)]);
    // A paragraph style, and resolved attributes.
    assert_eq!(found(&mut s, json!({"find": "eggs", "findFormat": {"paraStyle": "Method"}})), [(23, 27), (32, 36)]);
    assert_eq!(found(&mut s, json!({"find": "", "findFormat": {"chars": {"fontStyle": "Bold"}}})), [(6, 10), (12, 16)]);
    assert_eq!(found(&mut s, json!({"find": "", "findFormat": {"chars": {"size": 12}}})), [(0, 22), (23, 36)], "each paragraph apart");
    // Find Next walks the stretches.
    s.execute("text.select", &json!({"story": sid, "anchor": 0, "focus": 0})).unwrap();
    let next = json!({"find": "", "findFormat": {"charStyle": "eggs"}});
    assert_eq!(s.execute("find.next", &next).unwrap()["start"], 6);
    assert_eq!(s.execute("find.next", &next).unwrap()["start"], 12);
    // Equivalent spellings the attributes accept find the same text: a snake_case key, and a
    // nested number written as an integer (14 pt leading on the second paragraph).
    s.execute("text.select", &json!({"story": sid, "anchor": 23, "focus": 36})).unwrap();
    s.execute("type.char", &json!({"leading": {"kind": "points", "value": 14.0}})).unwrap();
    for (a, b) in [
        (json!({"fontStyle": "Bold"}), json!({"font_style": "Bold"})),
        (json!({"leading": {"kind": "points", "value": 14.0}}), json!({"leading": {"kind": "points", "value": 14}})),
    ] {
        let (a, b) = (json!({"find": "", "findFormat": {"chars": a}}), json!({"find": "", "findFormat": {"chars": b}}));
        let all = found(&mut s, a.clone());
        assert!(!all.is_empty(), "{a}");
        assert_eq!(found(&mut s, b.clone()), all, "{b}");
        for p in [&a, &b] {
            s.execute("text.select", &json!({"story": sid, "anchor": 0, "focus": 0})).unwrap();
            assert_eq!(s.execute("find.next", p).unwrap()["start"].as_u64(), Some(all[0].0), "{p}");
        }
    }
    // A null attribute asks for nothing.
    assert_eq!(found(&mut s, json!({"find": "eggs", "findFormat": {"charStyle": "eggs", "chars": {"size": null}}})), [(6, 10)]);
    // With a Change Format: the text in the style goes back to [None], underlined.
    let r = s
        .execute(
            "find.change",
            &json!({"find": "", "change": "", "findFormat": {"charStyle": "eggs", "chars": {"font_style": "Bold"}}, "charStyle": "[None]", "attrs": {"underline": true}}),
        )
        .unwrap();
    assert_eq!(r["count"], 2, "a snake_case attribute changes the same text");
    assert!(found(&mut s, json!({"find": "", "findFormat": {"charStyle": "eggs"}})).is_empty());
    assert_eq!(found(&mut s, json!({"find": "", "findFormat": {"chars": {"underline": true}}})), [(6, 10), (12, 16)]);
    // Unknown styles and attributes are refused; an empty search needs a Find Format.
    assert!(s.execute("find.find", &json!({"find": "", "findFormat": {"charStyle": "Nope"}})).is_err());
    assert!(s.execute("find.next", &json!({"find": "eggs", "findFormat": {"paraStyle": "Nope"}})).is_err());
    assert!(s.execute("find.find", &json!({"find": "eggs", "findFormat": {"chars": {"bogus": 1}}})).is_err());
    // Malformed fields are refused, not searched without.
    for f in [json!({"chars": [1]}), json!({"chars": 12}), json!({"charStyle": 3}), json!({"paraStyle": ["Body"]})] {
        assert!(s.execute("find.find", &json!({"find": "eggs", "findFormat": f})).is_err(), "{f}");
    }
    assert!(s.execute("find.find", &json!({"find": ""})).is_err());
}

/// Change and Change/Find: only the text Find Next found changes; it stays selected and the
/// search goes on after it in its domain (here the original selection). A selection that
/// isn't a match changes nothing and makes no undo step.
#[test]
fn change_changes_the_found_text_and_the_search_goes_on() {
    let (mut s, sid) = scope_session("red red | red");
    scope_select(&mut s, sid, 0, 7);
    let p = json!({"find": "red", "change": "blue", "scope": "selection"});
    let mut change = p.clone();
    change["found"] = json!(true);
    scope_hit(&mut s, &p, sid, 0, 3);
    assert_eq!(s.execute("find.change", &change).unwrap()["count"], 1);
    let text = |s: &Session| s.doc().unwrap().doc.story(sid).unwrap().text.clone();
    assert_eq!(text(&s), "blue red | red");
    assert_eq!(s.doc().unwrap().selection.text.unwrap().range(), 0..4, "the changed text stays selected");
    scope_hit(&mut s, &p, sid, 5, 8);
    s.execute("find.change", &change).unwrap();
    assert_eq!(text(&s), "blue blue | red");
    assert_eq!(s.execute("find.next", &p).unwrap(), serde_json::Value::Null, "the last red is outside the selection searched");
    // Not a match: nothing changes, no undo step.
    scope_select(&mut s, sid, 0, 4);
    let history = s.doc().unwrap().history.undo.len();
    assert_eq!(s.execute("find.change", &json!({"find": "red", "change": "blue", "found": true})).unwrap()["count"], 0);
    assert_eq!(s.doc().unwrap().history.undo.len(), history);
    // A Change Format with nothing in Change to formats the found text only.
    scope_select(&mut s, sid, 0, 0);
    let fmt = json!({"find": "red", "change": "", "attrs": {"underline": true}});
    let hit = s.execute("find.next", &fmt).unwrap();
    assert_eq!(hit["start"], 12);
    let mut found = fmt.clone();
    found["found"] = json!(true);
    s.execute("find.change", &found).unwrap();
    assert_eq!(text(&s), "blue blue | red");
    assert_eq!(s.doc().unwrap().doc.story(sid).unwrap().format_after(12).over.underline, Some(true));
    assert_eq!(s.doc().unwrap().selection.text.unwrap().range(), 12..15);
}

/// Change accepts what Find Next selected even when Change All wouldn't match there: the
/// rest of a Find Format stretch from a caret inside it, and an overlapping match.
#[test]
fn change_takes_any_selection_find_next_made() {
    // A caret inside a bold stretch: Find Next with only a Find Format selects the rest of it.
    let (mut s, sid) = scope_session("plain bold text");
    s.execute("style.character.create", &json!({"name": "Strong", "chars": {"fontStyle": "Bold"}})).unwrap();
    scope_select(&mut s, sid, 6, 10);
    s.execute("style.character.apply", &json!({"name": "Strong"})).unwrap();
    scope_select(&mut s, sid, 8, 8);
    let p = json!({"find": "", "change": "", "findFormat": {"charStyle": "Strong"}, "attrs": {"underline": true}, "scope": "story"});
    let hit = s.execute("find.next", &p).unwrap();
    assert_eq!((hit["start"].as_u64(), hit["end"].as_u64()), (Some(8), Some(10)));
    let mut change = p.clone();
    change["found"] = json!(true);
    assert_eq!(s.execute("find.change", &change).unwrap()["count"], 1);
    let st = s.doc().unwrap().doc.story(sid).unwrap().clone();
    assert_eq!((st.format_after(7).over.underline, st.format_after(8).over.underline), (None, Some(true)), "only the selected rest");
    // "aa" in "aaa" from the second a: Find Next selects 1..3, which Change All never finds.
    for grep in [false, true] {
        let (mut s, sid) = scope_session("aaa");
        scope_select(&mut s, sid, 1, 1);
        let (find, change) = if grep { ("(a)a", "<$1>") } else { ("aa", "b") };
        let p = json!({"find": find, "change": change, "grep": grep, "scope": "story"});
        let hit = s.execute("find.next", &p).unwrap();
        assert_eq!((hit["start"].as_u64(), hit["end"].as_u64()), (Some(1), Some(3)));
        let mut found = p.clone();
        found["found"] = json!(true);
        assert_eq!(s.execute("find.change", &found).unwrap()["count"], 1, "grep: {grep}");
        assert_eq!(s.doc().unwrap().doc.story(sid).unwrap().text, if grep { "a<a>" } else { "ab" });
    }
}
