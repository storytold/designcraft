use super::caret::story_of;
use super::format::{Target, effective_char_format, format_targets};
use super::{ItemId, Result, Session, StoryId, Value, bad, bool_or, json};

/// The stories of the text selection, the selected cells and the selected text frames.
fn selected_stories(s: &Session) -> Result<Vec<StoryId>> {
    let st = s.doc()?;
    let mut stories: Vec<StoryId> = st.selection.text.map(|t| t.story).into_iter().chain(st.selection.cells.map(|c| c.story)).collect();
    stories.dedup();
    for id in &st.selection.items {
        if let Some(tf) = st.doc.item(*id).and_then(|i| i.text_frame())
            && !stories.contains(&tf.story)
        {
            stories.push(tf.story);
        }
    }
    Ok(stories)
}

/// Type ▸ Story: the story-wide options (Optical Margin Alignment and its size).
pub(super) fn story_options(s: &mut Session, p: &Value) -> Result<Value> {
    const CMD: &str = "story.options";
    let on = match p.get("opticalMargin") {
        None => None,
        Some(v) => Some(v.as_bool().ok_or_else(|| bad(CMD, "`opticalMargin` must be a bool"))?),
    };
    let size = match p.get("opticalMarginSize") {
        None => None,
        Some(v) => Some(
            v.as_f64()
                .and_then(designcraft_doc::OpticalMargin::clamp_size)
                .ok_or_else(|| bad(CMD, "`opticalMarginSize` must be a finite number of points"))?,
        ),
    };
    if on.is_none() && size.is_none() {
        return Err(bad(CMD, "nothing to set: give opticalMargin or opticalMarginSize"));
    }
    let explicit = |key| p.get(key).filter(|v| !v.is_null());
    let stories = if explicit("story").is_some() || explicit("frame").is_some() {
        // Explicit targets must not silently fall back to the current selection.
        let doc = &s.doc()?.doc;
        for key in ["story", "frame"] {
            if let Some(value) = explicit(key) {
                let id = value.as_u64().ok_or_else(|| bad(CMD, format!("`{key}` must be an unsigned integer")))?;
                let sid = if key == "story" {
                    StoryId(id)
                } else {
                    let item = doc.item(ItemId(id)).ok_or_else(|| bad(CMD, format!("no such frame {id}")))?;
                    item.text_frame().ok_or_else(|| bad(CMD, format!("frame {id} holds no text")))?.story
                };
                if doc.story(sid).is_none() {
                    return Err(bad(CMD, format!("no such story {}", sid.0)));
                }
            }
        }
        vec![story_of(s, p).ok_or_else(|| bad(CMD, "no such story"))?]
    } else {
        selected_stories(s)?
    };
    if stories.is_empty() {
        return Err(bad(CMD, "no story: select text or a text frame, or give story or frame"));
    }
    s.edit(|d, _| {
        for sid in &stories {
            let st = d.story_mut(*sid).ok_or(designcraft_doc::DocError::NoStory(*sid))?;
            let mut om = st.optical_margin;
            om.on = on.unwrap_or(om.on);
            om.size = size.unwrap_or(om.size);
            if om != st.optical_margin {
                st.optical_margin = om;
                st.rev = st.rev.wrapping_add(1);
            }
        }
        Ok(json!({"stories": stories.len()}))
    })
}

/// Type ▸ Story Direction.
pub(super) fn story_direction(s: &mut Session, p: &Value) -> Result<Value> {
    let vertical = p.get("vertical").and_then(Value::as_bool).ok_or_else(|| bad("type.storyDirection", "missing vertical"))?;
    let stories = selected_stories(s)?;
    s.edit(|d, _| {
        set_story_direction(d, &stories, vertical);
        Ok(json!({"stories": stories.len()}))
    })
}

/// Set the direction of `stories` (all their frames turn together).
pub(crate) fn set_story_direction(d: &mut designcraft_doc::Document, stories: &[StoryId], vertical: bool) {
    for sid in stories {
        if let Some(st) = d.story_mut(*sid)
            && st.vertical != vertical
        {
            st.vertical = vertical;
            st.rev = st.rev.wrapping_add(1);
        }
    }
}

/// How much of the selected text `type.selectionAttrs` reports.
pub(super) const SELECTED_TEXT_CHARS: usize = 60;

pub(super) fn selected_text_preview(chars: impl Iterator<Item = char>) -> String {
    chars
        .map(|c| if c.is_control() || c == designcraft_doc::OBJECT_MARK { ' ' } else { c })
        .skip_while(|c| c.is_whitespace())
        .take(SELECTED_TEXT_CHARS)
        .collect()
}

pub(super) fn selection_attrs(s: &mut Session, p: &Value) -> Result<Value> {
    let st = s.doc()?;
    let targets = format_targets(s);
    let Some(Target { story: sid, cell, range: r }) = targets.first().cloned() else { return Ok(Value::Null) };
    let story = st.doc.text_story(sid, cell).ok_or(designcraft_doc::DocError::NoStory(sid))?;
    let pi = story.para_at(r.start);
    let pf = &story.paras[pi];
    let (pp, base) = st.doc.styles.resolve_para(pf);
    let cf = effective_char_format(s, &Target { story: sid, cell, range: r.clone() })?;
    let cp = st.doc.styles.resolve_char(&base, &cf);
    let (style_para, style_chars) = st.doc.styles.resolve_para_style(&pf.style);
    let mut character_base = base.clone();
    let character_style = st.doc.styles.char_style_attrs(&cf.style);
    character_base.apply(&character_style);
    let char_delta = character_base.delta(&cp);
    let style_keys = character_style.keys();
    let character_style_overrides = char_delta.keys().iter().filter(|key| style_keys.contains(key)).count();
    // The text's own formatting over its paragraph, as Create New Style from Selection captures it.
    let mut own = character_style.clone();
    own.merge(&cf.over);
    // The selected text (its start, on one line), for previews such as the font menu's.
    let selected = selected_text_preview(story.text.get(r.start..r.end).unwrap_or_default().chars());
    let mut out = json!({"story": sid.0, "paragraphStyle": pf.style, "characterStyle": cf.style, "para": pp, "chars": cp, "ownChars": own,
        "paraOverrides": style_para.delta(&pp).count() + style_chars.delta(&base).count(),
        "charOverrides": char_delta.count(), "characterStyleOverrides": character_style_overrides,
        "selectedText": selected.trim()});
    let mut stories = selected_stories(s)?;
    if stories.is_empty() {
        stories.push(sid);
    }
    out["storyOptions"] = story_options_of(&st.doc, &stories);
    if bool_or(p, "mixed", false) {
        out["paraMixed"] = json!(mixed_para_attrs(&st.doc, &targets));
    }
    Ok(out)
}

/// The story options of `stories`, as story.options names them; a value that differs between
/// the stories is null.
fn story_options_of(doc: &designcraft_doc::Document, stories: &[StoryId]) -> Value {
    let mut values = stories.iter().filter_map(|sid| doc.story(*sid)).map(|st| st.optical_margin);
    let Some(first) = values.next() else { return json!({"opticalMargin": null, "opticalMarginSize": null}) };
    let (mut on, mut size) = (Some(first.on), Some(first.size));
    for om in values {
        on = on.filter(|v| *v == om.on);
        size = size.filter(|v| *v == om.size);
    }
    json!({"opticalMargin": on, "opticalMarginSize": size})
}

/// The paragraph attributes whose resolved values differ across the paragraphs of `targets`.
fn mixed_para_attrs(doc: &designcraft_doc::Document, targets: &[Target]) -> Vec<String> {
    let mut first: Option<serde_json::Map<String, Value>> = None;
    let mut mixed = std::collections::BTreeSet::new();
    for t in targets {
        let Some(story) = doc.text_story(t.story, t.cell) else { continue };
        let a = story.para_at(t.range.start);
        let b = story.para_at(t.range.end.max(t.range.start));
        for pf in story.paras.get(a..=b).unwrap_or(&[]) {
            let Ok(Value::Object(v)) = serde_json::to_value(doc.styles.resolve_para(pf).0) else { continue };
            match &first {
                None => first = Some(v),
                Some(f) => mixed.extend(v.into_iter().filter(|(k, x)| f.get(k) != Some(x)).map(|(k, _)| k)),
            }
        }
    }
    mixed.into_iter().collect()
}

#[cfg(test)]
mod story_options_tests {
    use super::*;

    #[test]
    fn malformed_explicit_story_targets_do_not_change_the_selection() {
        let mut s = Session::new();
        s.execute("file.new", &json!({})).unwrap();
        let r = s.execute("frame.create", &json!({"rect": [0, 0, 300, 200], "content": "text", "text": "Selected"})).unwrap();
        let sid = StoryId(r["story"].as_u64().unwrap());
        for target in ["story", "frame"] {
            for invalid in [json!("invalid"), json!(-1), json!(1.5), json!(true), json!({}), json!([]), json!(999_999)] {
                let mut params = json!({"opticalMargin": true});
                params[target] = invalid;
                assert!(s.execute("story.options", &params).is_err(), "{params}");
                assert!(!story(&s, sid).on, "{params} changed the selected story");
            }
        }
        // Selection fallback still works when neither explicit target is supplied.
        s.execute("story.options", &json!({"opticalMargin": true})).unwrap();
        assert!(story(&s, sid).on);
        s.execute("story.options", &json!({"frame": r["id"], "opticalMargin": false})).unwrap();
        assert!(!story(&s, sid).on);
    }

    #[test]
    fn story_options_treat_null_targets_as_absent_and_validate_both_explicit_ids() {
        let mut s = Session::new();
        s.execute("file.new", &json!({})).unwrap();
        let a = s.execute("frame.create", &json!({"rect": [0, 0, 300, 200], "content": "text", "text": "Explicit"})).unwrap();
        let b = s.execute("frame.create", &json!({"rect": [0, 300, 300, 500], "content": "text", "text": "Selected"})).unwrap();
        let aid = StoryId(a["story"].as_u64().unwrap());
        let bid = StoryId(b["story"].as_u64().unwrap());
        for targets in [json!({"story": null}), json!({"frame": null}), json!({"story": null, "frame": null})] {
            let mut p = targets;
            p["opticalMargin"] = json!(true);
            s.execute("story.options", &p).unwrap();
            assert!(story(&s, bid).on);
            assert!(!story(&s, aid).on);
            s.execute("edit.undo", &json!({})).unwrap();
            assert!(!story(&s, bid).on);
        }
        for targets in [json!({"story": null, "frame": a["id"]}), json!({"story": a["story"], "frame": null})] {
            let mut p = targets;
            p["opticalMargin"] = json!(true);
            s.execute("story.options", &p).unwrap();
            assert!(story(&s, aid).on);
            assert!(!story(&s, bid).on);
            s.execute("edit.undo", &json!({})).unwrap();
        }
        // Even a frame ignored by story precedence must be valid when explicitly supplied.
        let non_text = s.execute("frame.create", &json!({"rect": [400, 0, 500, 100]})).unwrap();
        for (targets, message) in [
            (json!({"frame": 999_999}), "no such frame 999999".to_string()),
            (json!({"frame": non_text["id"]}), format!("frame {} holds no text", non_text["id"])),
            (json!({"story": 999_999}), "no such story 999999".to_string()),
            (json!({"story": a["story"], "frame": 999_999}), "no such frame 999999".to_string()),
        ] {
            let mut p = targets;
            p["opticalMargin"] = json!(true);
            assert!(s.execute("story.options", &p).unwrap_err().to_string().contains(&message), "{p}: expected {message}");
            assert!(!story(&s, aid).on);
            assert!(!story(&s, bid).on);
        }
    }

    fn first_x(s: &Session, sid: StoryId) -> f64 {
        let cs = s.cache.get(&s.doc().unwrap().doc, sid, None);
        cs.frames[0].lines[0].glyphs[0].x
    }

    fn story(s: &Session, sid: StoryId) -> designcraft_doc::OpticalMargin {
        s.doc().unwrap().doc.story(sid).unwrap().optical_margin
    }

    #[test]
    fn story_options_set_optical_margin_with_one_undo_step_each() {
        let mut s = Session::new();
        s.execute("file.new", &json!({})).unwrap();
        let r = s.execute("frame.create", &json!({"rect": [0, 0, 300, 200], "content": "text", "text": "“Hung” words."})).unwrap();
        let sid = StoryId(r["story"].as_u64().unwrap());
        let flush = first_x(&s, sid);
        // The selected frame's story.
        s.execute("story.options", &json!({"opticalMargin": true})).unwrap();
        assert_eq!(story(&s, sid), designcraft_doc::OpticalMargin { on: true, size: 12.0 });
        let hung = first_x(&s, sid);
        assert!(hung < flush - 1.0, "{hung} {flush}");
        s.execute("story.options", &json!({"story": sid.0, "opticalMarginSize": 24})).unwrap();
        assert_eq!(story(&s, sid).size, 24.0);
        assert!(first_x(&s, sid) < hung - 1.0);
        s.execute("edit.undo", &json!({})).unwrap();
        assert_eq!(story(&s, sid), designcraft_doc::OpticalMargin { on: true, size: 12.0 });
        s.execute("edit.undo", &json!({})).unwrap();
        assert!(!story(&s, sid).on);
        assert_eq!(first_x(&s, sid), flush);
        // Hostile sizes.
        s.execute("story.options", &json!({"story": sid.0, "opticalMarginSize": 1e9})).unwrap();
        assert_eq!(story(&s, sid).size, designcraft_doc::OpticalMargin::MAX_SIZE);
        for bad in [json!({"opticalMarginSize": "big"}), json!({"opticalMargin": 1}), json!({}), json!({"story": 999_999, "opticalMargin": true})] {
            assert!(s.execute("story.options", &bad).is_err(), "{bad}");
        }
    }

    #[test]
    fn ignore_optical_margin_through_type_para_and_paragraph_styles() {
        let mut s = Session::new();
        s.execute("file.new", &json!({})).unwrap();
        let r = s.execute("frame.create", &json!({"rect": [0, 0, 300, 200], "content": "text", "text": "“Hung” words."})).unwrap();
        let sid = StoryId(r["story"].as_u64().unwrap());
        let flush = first_x(&s, sid);
        s.execute("story.options", &json!({"opticalMargin": true})).unwrap();
        assert!(first_x(&s, sid) < flush - 1.0);
        let ignored = |s: &Session| s.doc().unwrap().doc.story(sid).unwrap().paras[0].para.ignore_optical_margin;
        s.execute("type.para", &json!({"attrs": {"ignoreOpticalMargin": true}})).unwrap();
        assert_eq!(ignored(&s), Some(true));
        assert_eq!(first_x(&s, sid), flush);
        s.execute("edit.undo", &json!({})).unwrap();
        assert_eq!(ignored(&s), None);
        s.execute("style.paragraph.create", &json!({"name": "Flush", "para": {"ignoreOpticalMargin": true}})).unwrap();
        let resolved = |s: &Session| s.doc().unwrap().doc.styles.resolve_para_style("Flush").0.ignore_optical_margin;
        assert!(resolved(&s));
        s.execute("style.paragraph.edit", &json!({"name": "Flush", "para": {"ignoreOpticalMargin": false}})).unwrap();
        assert!(!resolved(&s));
        s.execute("edit.undo", &json!({})).unwrap();
        assert!(resolved(&s));
        s.execute("style.paragraph.apply", &json!({"name": "Flush"})).unwrap();
        assert_eq!(first_x(&s, sid), flush);
    }

    #[test]
    fn selection_attrs_report_the_story_options_with_differences_as_null() {
        let mut s = Session::new();
        s.execute("file.new", &json!({})).unwrap();
        let a = s.execute("frame.create", &json!({"rect": [0, 0, 300, 200], "content": "text", "text": "One"})).unwrap();
        let b = s.execute("frame.create", &json!({"rect": [0, 300, 300, 500], "content": "text", "text": "Two"})).unwrap();
        let options = |s: &mut Session| s.execute("type.selectionAttrs", &json!({})).unwrap()["storyOptions"].clone();
        assert_eq!(options(&mut s), json!({"opticalMargin": false, "opticalMarginSize": 12.0}));
        s.execute("story.options", &json!({"story": b["story"], "opticalMargin": true, "opticalMarginSize": 18})).unwrap();
        assert_eq!(options(&mut s), json!({"opticalMargin": true, "opticalMarginSize": 18.0}), "the selected frame's story");
        s.execute("selection.set", &json!({"ids": [a["id"], b["id"]]})).unwrap();
        assert_eq!(options(&mut s), json!({"opticalMargin": null, "opticalMarginSize": null}));
        s.execute("story.options", &json!({"opticalMarginSize": 18})).unwrap();
        assert_eq!(options(&mut s), json!({"opticalMargin": null, "opticalMarginSize": 18.0}));
    }
}

#[cfg(test)]
mod story_direction_tests {
    use designcraft_doc::{ItemId, StoryId};
    use serde_json::json;

    use crate::Session;

    fn frame(s: &mut Session, rect: [f64; 4], text: &str, vertical: bool) -> (ItemId, StoryId) {
        let r = s.execute("frame.create", &json!({"rect": rect, "content": "text", "text": text, "caret": false, "vertical": vertical})).unwrap();
        (ItemId(r["id"].as_u64().unwrap()), StoryId(r["story"].as_u64().unwrap()))
    }

    /// Which frames of the story are composed vertically, in thread order.
    fn composed(s: &Session, sid: StoryId) -> Vec<bool> {
        s.cache.get(&s.doc().unwrap().doc, sid, None).frames.iter().map(|f| f.vertical).collect()
    }

    #[test]
    fn story_direction_turns_every_frame_of_the_story() {
        let mut s = Session::new();
        s.execute("file.new", &json!({})).unwrap();
        let (a, sid) = frame(&mut s, [72.0, 72.0, 200.0, 400.0], "縦書きの文章です。", false);
        let (b, _) = frame(&mut s, [300.0, 72.0, 428.0, 400.0], "", false);
        s.edit(|d, _| Ok(d.thread(a, b)?)).unwrap();
        assert_eq!(composed(&s, sid), [false, false]);
        // From the second frame selected, through Type ▸ Story Direction.
        s.execute("selection.set", &json!({"ids": [b.0]})).unwrap();
        s.execute("type.storyDirection", &json!({"vertical": true})).unwrap();
        assert_eq!(composed(&s, sid), [true, true]);
        // From a caret in the text.
        s.execute("text.select", &json!({"story": sid.0, "anchor": 0, "focus": 0})).unwrap();
        s.execute("type.storyDirection", &json!({"vertical": false})).unwrap();
        assert_eq!(composed(&s, sid), [false, false]);
        // The frame option sets the frame's story.
        s.execute("object.textFrameOptions", &json!({"ids": [a.0], "vertical": true})).unwrap();
        assert_eq!(composed(&s, sid), [true, true]);
        // Undo puts it back.
        s.execute("edit.undo", &json!({})).unwrap();
        assert_eq!(composed(&s, sid), [false, false]);
    }

    #[test]
    fn a_frame_threaded_onto_a_vertical_story_turns_vertical() {
        let mut s = Session::new();
        s.execute("file.new", &json!({})).unwrap();
        let (a, sid) = frame(&mut s, [72.0, 72.0, 200.0, 400.0], "縦書きの文章です。", true);
        let (b, _) = frame(&mut s, [300.0, 72.0, 428.0, 400.0], "", false);
        let c = s.execute("frame.create", &json!({"rect": [450, 72, 550, 400]})).unwrap();
        let c = ItemId(c["id"].as_u64().unwrap());
        s.edit(|d, _| {
            d.thread(a, b)?;
            Ok(d.thread(b, c)?)
        })
        .unwrap();
        assert_eq!(composed(&s, sid), [true, true, true]);
    }
}

#[cfg(test)]
mod story_query_contract_tests {
    use serde_json::json;
    #[test]
    fn missing_story_errors_but_valid_empty_and_selected_stories_succeed() {
        let mut s = crate::Session::new();
        s.execute("file.new", &json!({})).unwrap();
        assert!(s.execute("story.get", &json!({"story":999})).unwrap_err().to_string().contains("no such story"));
        let f = s.execute("frame.create", &json!({"rect":[36,36,300,200],"content":"text"})).unwrap();
        assert_eq!(s.execute("story.get", &json!({"story":f["story"]})).unwrap()["text"], "");
        assert_eq!(s.execute("story.get", &json!({"frame":f["id"]})).unwrap()["text"], "");
        s.execute("text.select", &json!({"story":f["story"],"anchor":0})).unwrap();
        assert_eq!(s.execute("story.get", &json!({})).unwrap()["text"], "");
    }

    #[test]
    fn kenten_shatai_auto_tcy_and_grid_settings_take_checked_values() {
        use designcraft_doc::cjk::CharacterAlignment;
        use designcraft_doc::cjk_settings::{KentenKind, KentenPosition};
        let mut s = crate::Session::new();
        s.execute("file.new", &json!({})).unwrap();
        let r = s.execute("frame.create", &json!({"rect": [72, 72, 200, 400], "content": "text", "text": "縦書き12年", "vertical": true})).unwrap();
        let sid = designcraft_doc::StoryId(r["story"].as_u64().unwrap());
        s.execute("text.select", &json!({"story": sid.0, "anchor": 0, "focus": "縦書き".len()})).unwrap();
        let chars = |s: &crate::Session| s.doc().unwrap().doc.stories[&sid].char_format_at(1).over.clone();
        let para = |s: &crate::Session| s.doc().unwrap().doc.stories[&sid].paras[0].para.clone();
        // Any setting turns kenten on; values are clamped and text is capped.
        let long = "★".repeat(10_000);
        s.execute(
            "type.kenten",
            &json!({"kind": "custom", "character": long, "size": 1e300, "distance": -1e300, "xScale": 0, "fillTint": 250, "position": "belowLeft"}),
        )
        .unwrap();
        let a = chars(&s);
        assert_eq!((a.kenten, a.kenten_kind, a.kenten_position), (Some(true), Some(KentenKind::Custom), Some(KentenPosition::BelowLeft)));
        assert_eq!(a.kenten_character.as_ref().map(|c| c.chars().count()), Some(designcraft_doc::cjk_settings::KENTEN_MARK_MAX_CHARS));
        assert_eq!(
            (a.kenten_size, a.kenten_distance, a.kenten_x_scale, a.kenten_fill_tint),
            (Some(Some(1296.0)), Some(-1296.0), Some(0.01), Some(Some(1.0)))
        );
        // Bad values change nothing.
        for bad in [json!({"kind": "sparkle"}), json!({"size": "big"}), json!({"character": 5}), json!({"on": "yes"}), json!({"overprintFill": 1})] {
            assert!(s.execute("type.kenten", &bad).is_err(), "{bad}");
        }
        assert_eq!(chars(&s), a);
        s.execute("type.kenten", &json!({"on": false})).unwrap();
        assert_eq!((chars(&s).kenten, chars(&s).kenten_kind), (Some(false), Some(KentenKind::Custom)), "off keeps the settings");
        // Shatai.
        s.execute("type.shatai", &json!({"magnification": 1e9, "angle": -30, "adjustRotation": true})).unwrap();
        let a = chars(&s);
        assert_eq!((a.shatai_magnification, a.shatai_angle, a.shatai_adjust_rotation), (Some(90.0), Some(150.0), Some(true)));
        assert!(s.execute("type.shatai", &json!({})).is_err());
        assert!(s.execute("type.shatai", &json!({"angle": [45]})).is_err());
        // Paragraph settings.
        s.execute("type.autoTateChuYoko", &json!({"digits": 1e12, "includeRoman": true})).unwrap();
        assert_eq!((para(&s).auto_tcy, para(&s).auto_tcy_include_roman), (Some(designcraft_compose::shape::AUTO_TCY_MAX), Some(true)));
        s.execute("type.gridSettings", &json!({"alignment": "emCenter", "firstLineOnly": true, "gyoudori": 1e12, "paragraphGyoudori": true}))
            .unwrap();
        let p = para(&s);
        assert_eq!((p.grid_align, p.grid_reference), (Some(designcraft_doc::GridAlign::FirstLineOnly), Some(CharacterAlignment::EmCenter)));
        assert_eq!((p.grid_gyoudori, p.paragraph_gyoudori), (Some(100), Some(true)));
        assert!(s.execute("type.gridSettings", &json!({"alignment": "middle"})).is_err());
        s.execute("type.gridSettings", &json!({"alignment": "none"})).unwrap();
        assert_eq!(para(&s).grid_align, Some(designcraft_doc::GridAlign::None));
        s.execute("type.rotateRoman", &json!({})).unwrap();
        assert_eq!(para(&s).rotate_roman, Some(true));
        s.execute("type.rotateRoman", &json!({})).unwrap();
        assert_eq!(para(&s).rotate_roman, Some(false));
        s.execute("type.romanWordBreak", &json!({"on": true})).unwrap();
        assert_eq!(para(&s).roman_word_break, Some(true));
        // The story still composes with every setting at its limit.
        let cs = designcraft_compose::compose_story(&s.doc().unwrap().doc, sid, &Default::default());
        assert!(!cs.frames.is_empty());
    }
}
