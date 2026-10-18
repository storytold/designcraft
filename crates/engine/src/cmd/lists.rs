//! Type › Bulleted & Numbered Lists › Define Lists: named lists whose numbers can continue from
//! story to story. Paragraphs join one with `listName` (and restart with `startAt`). Convert
//! Bullets and Numbering to Text types the labels in.

use designcraft_doc::{
    ChangeMark, CharFormat, Document, ListType, NO_CHAR_STYLE, NumberedList, ParaAttrs, SAME_TAB_POSITION, Story, TabAlign, TabStop,
};
use serde_json::{Value, json};

use super::text::{Target, format_targets, reserve_command_text};
use super::{CommandSpec, bad, cmd, has_doc, has_text_or_frames, str_param};
use crate::{Result, Session};

pub fn specs() -> Vec<CommandSpec> {
    vec![
        cmd!(
            "list.define",
            "Define Lists…",
            ["Type", "Bulleted & Numbered Lists"],
            None,
            "{name, continueAcrossStories?: true, rename?} — paragraphs join it with type.para {listType: numbers, listName}",
            has_doc,
            |s, p| {
                let name = str_param(p, "name").ok_or_else(|| bad("list.define", "missing name"))?.to_string();
                let cont = p.get("continueAcrossStories").and_then(Value::as_bool);
                let rename = str_param(p, "rename").map(str::to_string);
                s.edit(|d, _| {
                    let lists = &mut d.settings.lists;
                    match lists.iter_mut().find(|l| l.name == name) {
                        Some(l) => {
                            if let Some(c) = cont {
                                l.continue_across_stories = c;
                            }
                            if let Some(r) = &rename {
                                l.name = r.clone();
                            }
                        }
                        None => {
                            lists.push(NumberedList { name: rename.clone().unwrap_or(name.clone()), continue_across_stories: cont.unwrap_or(true) })
                        }
                    }
                    Ok(json!({"lists": d.settings.lists}))
                })
            }
        ),
        cmd!("list.delete", "Delete List", [], None, "{name}", has_doc, |s, p| {
            let name = str_param(p, "name").ok_or_else(|| bad("list.delete", "missing name"))?.to_string();
            s.edit(|d, _| {
                d.settings.lists.retain(|l| l.name != name);
                Ok(json!({"lists": d.settings.lists}))
            })
        }),
        cmd!(
            "list.convertToText",
            "Convert Bullets and Numbering to Text",
            ["Type", "Bulleted & Numbered Lists"],
            None,
            "{} — the selected paragraphs' bullets and numbers become text, and the paragraphs leave the list",
            has_text_or_frames,
            |s, _| convert_to_text(s)
        ),
    ]
}

/// Per target, the labels to type in (each one's paragraph start, text and format), and the
/// paragraphs whose Tab Position becomes a tab stop (index and their tabs with it).
type Labels = Vec<(Target, Vec<(usize, String, CharFormat)>, Vec<(usize, Vec<TabStop>)>)>;

fn convert_to_text(s: &mut Session) -> Result<Value> {
    let targets = format_targets(s);
    let d = &s.doc()?.doc;
    let tracking = d.settings.track_changes;
    let mut todo: Labels = Vec::new();
    for t in targets {
        let Some(st) = d.text_story(t.story, t.cell) else { continue };
        // The labels composition draws: the numbering is never counted here.
        let all = designcraft_compose::list_labels(d, st);
        let ranges = st.para_ranges();
        let (a, b) = (st.para_at(t.range.start), st.para_at(t.range.end.max(t.range.start)));
        let mut labels = Vec::new();
        let mut tabs = Vec::new();
        for pi in a..=b {
            let (Some(Some(label)), Some(r), Some(p)) = (all.get(pi), ranges.get(pi), st.paras.get(pi)) else { continue };
            let mut fmt = label_format(d, st, p, r.clone());
            // Track Changes marks typing outside table cells only, as text.insert does.
            if tracking && t.cell.is_none() {
                fmt.over.change = Some(ChangeMark::Inserted);
            }
            labels.push((r.start, label.clone(), fmt));
            if let Some(t) = list_tab_stops(d, p) {
                tabs.push((pi, t));
            }
        }
        todo.push((t, labels, tabs));
    }
    let converted: usize = todo.iter().map(|(_, l, _)| l.len()).sum();
    if converted == 0 {
        return Ok(json!({"converted": 0}));
    }
    s.edit(|d, sel| {
        for (t, labels, tabs) in &todo {
            let st = d.text_story_mut(t.story, t.cell).ok_or(designcraft_doc::DocError::NoStory(t.story))?;
            st.format_paras(t.range.clone(), |p| p.para.merge(&ParaAttrs { list_type: Some(ListType::None), ..Default::default() }));
            for (pi, stops) in tabs {
                if let Some(p) = st.paras.get_mut(*pi) {
                    p.para.tabs = Some(stops.clone());
                }
            }
            reserve_command_text(st, labels.iter().map(|(_, l, _)| l.len()).sum())?;
            // Last first, so the earlier paragraphs' starts still hold.
            for (at, label, fmt) in labels.iter().rev() {
                st.insert_with(*at, label, fmt.clone());
            }
            if let Some(ts) = sel.text.as_mut().filter(|ts| ts.story == t.story && ts.cell == t.cell) {
                // A caret at a paragraph start stays before the label, so the selection takes it in.
                let shift = |pos: usize| pos + labels.iter().filter(|(at, _, _)| *at < pos).map(|(_, l, _)| l.len()).sum::<usize>();
                (ts.anchor, ts.focus) = (shift(ts.anchor), shift(ts.focus));
            }
        }
        Ok(json!({"converted": converted}))
    })
}

/// A list paragraph's tab stops with its Tab Position as a left stop, when composition adds that
/// stop for the label only: the typed-in label's tab then still reaches it. The IDML export
/// writes Tab Position as the same stop. A stop at the Tab Position or just before it (closer
/// than [`SAME_TAB_POSITION`], where the list would merge them) is the one composition uses, so
/// it stays and nothing is added.
fn list_tab_stops(d: &Document, p: &designcraft_doc::ParaFormat) -> Option<Vec<TabStop>> {
    let (pp, _) = d.styles.resolve_para(p);
    let t = pp.list_tab.filter(|t| t.is_finite() && *t >= 0.0 && !pp.tabs.iter().any(|s| s.position <= *t && *t - s.position < SAME_TAB_POSITION))?;
    let mut tabs = pp.tabs;
    tabs.push(TabStop { position: t, align: TabAlign::Left, leader: String::new(), align_on: String::new() });
    Some(TabStop::sanitized_list(tabs))
}

/// The format of a paragraph's label as composition draws it: the paragraph style's characters
/// under the bullet's or number's character style, in the bullet font, and in the Digits of the
/// paragraph's first run.
fn label_format(d: &Document, st: &Story, p: &designcraft_doc::ParaFormat, r: std::ops::Range<usize>) -> CharFormat {
    let (pp, base) = d.styles.resolve_para(p);
    let bullets = pp.list_type == ListType::Bullets;
    let style = if bullets { &pp.bullet_char_style } else { &pp.number_char_style };
    let mut fmt = CharFormat { style: if style.is_empty() { NO_CHAR_STYLE.into() } else { style.clone() }, ..Default::default() };
    if bullets && !pp.bullet_font.is_empty() {
        fmt.over.font_family = Some(pp.bullet_font.clone());
        if !pp.bullet_font_style.is_empty() {
            fmt.over.font_style = Some(pp.bullet_font_style.clone());
        }
    }
    if d.styles.resolve_char(&base, &fmt).digits == base.digits
        && let Some((_, run)) = st.runs().find(|(rr, _)| rr.end > r.start && rr.start < r.end)
    {
        let digits = d.styles.resolve_char(&base, run).digits;
        if digits != base.digits {
            fmt.over.digits = Some(digits);
        }
    }
    fmt
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use crate::Session;

    /// The digit the story's first line starts with (its list number).
    fn digit(g: &designcraft_compose::PlacedGlyph) -> char {
        let face = g.face.get();
        ('0'..='9').find(|c| face.glyph_for(*c) == g.gid).unwrap_or('?')
    }

    fn first_char(s: &Session, sid: u64) -> char {
        let d = &s.doc().unwrap().doc;
        let cs = designcraft_compose::compose_story(d, designcraft_doc::StoryId(sid), &Default::default());
        digit(&cs.frames[0].lines[0].glyphs[0])
    }

    #[test]
    fn named_lists_continue_across_stories() {
        let mut s = Session::new();
        s.execute("file.new", &json!({"pages": 2, "facingPages": false})).unwrap();
        s.execute("list.define", &json!({"name": "Steps"})).unwrap();
        let mut stories = vec![];
        for (spread, text) in [(0, "Mix\nStir"), (1, "Bake\nServe")] {
            let r = s.execute("frame.create", &json!({"spread": spread, "rect": [72, 72, 400, 300], "content": "text", "text": text})).unwrap();
            let sid = r["story"].as_u64().unwrap();
            s.execute("text.select", &json!({"story": sid, "anchor": 0, "focus": text.len()})).unwrap();
            s.execute("type.para", &json!({"attrs": {"listType": "numbers", "listName": "Steps"}})).unwrap();
            stories.push(sid);
        }
        assert_eq!(first_char(&s, stories[0]), '1');
        assert_eq!(first_char(&s, stories[1]), '3', "carries on from the first story");
        // Not continuing: each story starts at 1.
        s.execute("list.define", &json!({"name": "Steps", "continueAcrossStories": false})).unwrap();
        assert_eq!(first_char(&s, stories[1]), '1');
        s.execute("list.define", &json!({"name": "Steps", "continueAcrossStories": true})).unwrap();
        // The session's cache follows earlier stories (no stale numbers).
        s.execute("text.select", &json!({"story": stories[0], "anchor": 0, "focus": 0})).unwrap();
        s.execute("text.insert", &json!({"text": "Prep\n"})).unwrap();
        let d = s.doc().unwrap().doc.clone();
        let cs = s.cache.get(&d, designcraft_doc::StoryId(stories[1]), None);
        assert_eq!(digit(&cs.frames[0].lines[0].glyphs[0]), '4');
        // Start At restarts.
        s.execute("text.select", &json!({"story": stories[1], "anchor": 0, "focus": 0})).unwrap();
        s.execute("type.para", &json!({"attrs": {"startAt": 7}})).unwrap();
        assert_eq!(first_char(&s, stories[1]), '7');
        // null removes the override again.
        s.execute("type.para", &json!({"attrs": {"startAt": null}})).unwrap();
        assert_eq!(first_char(&s, stories[1]), '4');
    }

    /// A text frame holding `text`, its paragraphs all set with `attrs`.
    fn list_story(s: &mut Session, text: &str, attrs: serde_json::Value) -> u64 {
        let r = s.execute("frame.create", &json!({"rect": [72, 72, 400, 400], "content": "text", "text": text})).unwrap();
        let sid = r["story"].as_u64().unwrap();
        select(s, sid, 0, text.len());
        s.execute("type.para", &json!({ "attrs": attrs })).unwrap();
        sid
    }

    fn select(s: &mut Session, sid: u64, anchor: usize, focus: usize) {
        s.execute("text.select", &json!({"story": sid, "anchor": anchor, "focus": focus})).unwrap();
    }

    fn story(s: &Session, sid: u64) -> designcraft_doc::Story {
        s.doc().unwrap().doc.story(designcraft_doc::StoryId(sid)).unwrap().clone()
    }

    fn convert(s: &mut Session) -> serde_json::Value {
        s.execute("list.convertToText", &json!({})).unwrap()
    }

    #[test]
    fn convert_to_text_types_the_numbers_composition_draws() {
        let mut s = Session::new();
        s.execute("file.new", &json!({"pages": 2, "facingPages": false})).unwrap();
        // A named list carries on from the story before: the second story starts at 3.
        s.execute("list.define", &json!({"name": "Steps"})).unwrap();
        let steps = json!({"listType": "numbers", "listName": "Steps"});
        list_story(&mut s, "Mix\nStir", steps.clone());
        let r = s.execute("frame.create", &json!({"spread": 1, "rect": [72, 72, 400, 300], "content": "text", "text": "Bake\nServe"})).unwrap();
        let second = r["story"].as_u64().unwrap();
        select(&mut s, second, 0, 10);
        s.execute("type.para", &json!({ "attrs": steps })).unwrap();
        select(&mut s, second, 0, 10);
        assert_eq!(convert(&mut s)["converted"], 2);
        assert_eq!(story(&s, second).text, "3.\tBake\n4.\tServe");

        // Start At on the third item: 5, and 6 after it.
        let sid = list_story(&mut s, "A\nB\nC\nD", json!({"listType": "numbers"}));
        select(&mut s, sid, 4, 4);
        s.execute("type.para", &json!({"attrs": {"startAt": 5}})).unwrap();
        select(&mut s, sid, 4, 7);
        convert(&mut s);
        assert_eq!(story(&s, sid).text, "A\nB\n5.\tC\n6.\tD");
        let d = s.doc().unwrap().doc.clone();
        let labels = designcraft_compose::list_labels(&d, d.story(designcraft_doc::StoryId(sid)).unwrap());
        assert_eq!(labels, [Some("1.\t".to_string()), Some("2.\t".to_string()), None, None]);

        // Levels: `^1` is the level above's current number.
        let sid = list_story(&mut s, "A\nB\nC", json!({"listType": "numbers"}));
        select(&mut s, sid, 2, 5);
        s.execute("type.para", &json!({"attrs": {"listLevel": 2, "numberExpression": "^1.^#.^t"}})).unwrap();
        select(&mut s, sid, 5, 5);
        convert(&mut s);
        assert_eq!(story(&s, sid).text, "A\nB\n1.2.\tC");

        // A number's expression around it.
        let sid = list_story(&mut s, "A\nB\nC", json!({"listType": "numbers", "numberExpression": "Tabel ^#^t"}));
        select(&mut s, sid, 5, 5);
        convert(&mut s);
        assert_eq!(story(&s, sid).text, "A\nB\nTabel 3\tC");
    }

    #[test]
    fn a_converted_bullet_keeps_its_font_and_character_style() {
        let mut s = Session::new();
        s.execute("file.new", &json!({})).unwrap();
        s.execute("style.character.create", &json!({"name": "Bullet", "chars": {"size": 20}})).unwrap();
        let attrs = json!({"listType": "bullets", "bulletFont": "Source Sans 3", "bulletFontStyle": "Bold", "bulletCharStyle": "Bullet"});
        let sid = list_story(&mut s, "Item", attrs);
        s.execute("type.char", &json!({"size": 9})).unwrap();
        select(&mut s, sid, 0, 0);
        convert(&mut s);
        let st = story(&s, sid);
        assert_eq!(st.text, "\u{2022}\tItem");
        let label = st.char_format_of(0);
        assert_eq!(label.style, "Bullet");
        assert_eq!(label.over.font_family.as_deref(), Some("Source Sans 3"));
        assert_eq!(label.over.font_style.as_deref(), Some("Bold"));
        assert_eq!(label.over.size, None, "the label is drawn without the text's own overrides");
        assert_eq!(st.char_format_of(3), label, "the tab after the bullet is part of it");
        let text = st.char_format_of(4);
        assert_eq!((text.style.as_str(), text.over.size), (designcraft_doc::NO_CHAR_STYLE, Some(9.0)));
    }

    #[test]
    fn convert_to_text_overrides_a_list_style_on_the_selected_paragraphs_only() {
        let mut s = Session::new();
        s.execute("file.new", &json!({})).unwrap();
        s.execute("style.paragraph.create", &json!({"name": "Steps", "para": {"listType": "numbers"}})).unwrap();
        let r = s.execute("frame.create", &json!({"rect": [72, 72, 400, 400], "content": "text", "text": "A\nB\nC"})).unwrap();
        let sid = r["story"].as_u64().unwrap();
        select(&mut s, sid, 0, 5);
        s.execute("style.paragraph.apply", &json!({"name": "Steps"})).unwrap();
        select(&mut s, sid, 2, 2);
        convert(&mut s);
        let st = story(&s, sid);
        assert_eq!(st.text, "A\n2.\tB\nC");
        let types: Vec<_> = st.paras.iter().map(|p| p.para.list_type).collect();
        assert_eq!(types, [None, Some(designcraft_doc::ListType::None), None]);
        assert_eq!(serde_json::to_value(&st.paras[1].para).unwrap()["listType"], "none");
        let d = s.doc().unwrap().doc.clone();
        let labels = designcraft_compose::list_labels(&d, &st);
        assert!(labels[0].is_some() && labels[1].is_none() && labels[2].is_some(), "{labels:?}");
        // IDML keeps the paragraph out of the list.
        let back = designcraft_idml::import_idml(&designcraft_idml::export_idml(&d)).unwrap();
        let st = back.stories.values().find(|st| st.text.starts_with("A\n2.")).unwrap();
        let types: Vec<_> = st.paras.iter().map(|p| back.styles.resolve_para(p).0.list_type).collect();
        assert_eq!(types, [designcraft_doc::ListType::Numbers, designcraft_doc::ListType::None, designcraft_doc::ListType::Numbers]);
    }

    #[test]
    fn convert_to_text_is_one_undo_step_and_tracked() {
        let mut s = Session::new();
        s.execute("file.new", &json!({})).unwrap();
        let sid = list_story(&mut s, "A\nB", json!({"listType": "numbers"}));
        let before = s.doc().unwrap().doc.clone();
        let undo = s.doc().unwrap().history.undo.len();
        convert(&mut s);
        assert_eq!(s.doc().unwrap().history.undo.len(), undo + 1);
        s.execute("edit.undo", &json!({})).unwrap();
        assert_eq!(s.doc().unwrap().doc, before);
        // Nothing to convert: no change, no undo step.
        let plain = s.execute("frame.create", &json!({"rect": [72, 420, 400, 500], "content": "text", "text": "Plain"})).unwrap();
        select(&mut s, plain["story"].as_u64().unwrap(), 0, 5);
        let undo = s.doc().unwrap().history.undo.len();
        assert_eq!(convert(&mut s)["converted"], 0);
        assert_eq!(s.doc().unwrap().history.undo.len(), undo);
        // Track Changes: the labels are inserted text.
        s.execute("changes.track", &json!({"on": true})).unwrap();
        select(&mut s, sid, 0, 3);
        convert(&mut s);
        let st = story(&s, sid);
        assert_eq!(st.text, "1.\tA\n2.\tB");
        for at in [0, 5] {
            assert_eq!(st.char_format_of(at).over.change, Some(designcraft_doc::ChangeMark::Inserted), "label at {at}");
        }
        assert_eq!(st.char_format_of(3).over.change, None);
    }

    /// A stop just before the Tab Position is the one composition uses: conversion keeps it rather
    /// than replacing it with a left stop at the Tab Position.
    #[test]
    fn convert_to_text_keeps_a_stop_just_before_the_tab_position() {
        let mut s = Session::new();
        s.execute("file.new", &json!({})).unwrap();
        let sid = list_story(&mut s, "Item", json!({"listType": "bullets", "listTab": 50.0}));
        s.execute("type.tabs.add", &json!({"position": 49.995, "align": "right"})).unwrap();
        select(&mut s, sid, 0, 0);
        convert(&mut s);
        let tabs = story(&s, sid).paras[0].para.tabs.clone().unwrap_or_default();
        assert_eq!(tabs.len(), 1, "{tabs:?}");
        assert_eq!((tabs[0].position, tabs[0].align), (49.995, designcraft_doc::TabAlign::Right), "{tabs:?}");
    }

    /// The label's and the text's x from the column's left: a Tab Position keeps placing the text
    /// once the label is typed in.
    #[test]
    fn convert_to_text_keeps_the_text_at_the_tab_position() {
        let xs = |s: &Session, sid: u64| {
            let d = &s.doc().unwrap().doc;
            let cs = designcraft_compose::compose_story(d, designcraft_doc::StoryId(sid), &Default::default());
            let col = cs.frames[0].columns[0];
            let l = &cs.frames[0].lines[0];
            let body_at = d.story(designcraft_doc::StoryId(sid)).unwrap().text.find("Item").unwrap();
            let label = l.glyphs.iter().find(|g| g.len == 0 || g.byte < body_at).map(|g| g.x - col.x0).unwrap();
            let body = l.glyphs.iter().find(|g| g.len > 0 && g.byte == body_at).map(|g| g.x - col.x0).unwrap();
            (label, body)
        };
        let cases = [
            ("bullets, Tab Position 50", json!({"listType": "bullets", "listTab": 50.0})),
            ("numbers, Tab Position 50", json!({"listType": "numbers", "listTab": 50.0})),
            (
                "numbers, indents 36/-18, Tab Position 30",
                json!({"listType": "numbers", "leftIndent": 36.0, "firstLineIndent": -18.0, "listTab": 30.0}),
            ),
            ("numbers, indents 36/-18", json!({"listType": "numbers", "leftIndent": 36.0, "firstLineIndent": -18.0})),
        ];
        for (name, attrs) in cases {
            let mut s = Session::new();
            s.execute("file.new", &json!({})).unwrap();
            let sid = list_story(&mut s, "Item", attrs);
            let (label, body) = xs(&s, sid);
            select(&mut s, sid, 0, 0);
            convert(&mut s);
            let (label_after, body_after) = xs(&s, sid);
            assert!((label - label_after).abs() < 0.01, "{name}: label {label} -> {label_after}");
            assert!((body - body_after).abs() < 0.01, "{name}: text {body} -> {body_after}");
        }
    }
}
