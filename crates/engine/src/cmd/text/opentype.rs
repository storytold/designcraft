use super::format::{Target, format_chars, format_targets};
use super::story::selection_attrs;
#[cfg(test)]
use super::story::{SELECTED_TEXT_CHARS, selected_text_preview};
use super::{Result, Session, Value, bad, json, ok};

pub(super) fn open_type(s: &mut Session, p: &Value) -> Result<Value> {
    use designcraft_doc::otf;
    let cur = selection_attrs(s, &json!({}))?;
    let mut list: Vec<String> =
        cur["chars"]["otfFeatures"].as_array().map(|a| a.iter().filter_map(|v| v.as_str().map(str::to_string)).collect()).unwrap_or_default();
    if let Some(tag) = p.get("feature").and_then(Value::as_str) {
        if tag.is_empty() || tag.len() > 4 || !tag.chars().all(|c| c.is_ascii_alphanumeric()) {
            return Err(bad("type.openType", format!("bad feature tag `{tag}`")));
        }
        let on = p.get("on").and_then(Value::as_bool).unwrap_or(!otf::is_on(&list, tag));
        otf::set(&mut list, tag, on);
    }
    if let Some(f) = p.get("figures").and_then(Value::as_str)
        && !otf::set_figures(&mut list, f)
    {
        return Err(bad("type.openType", format!("unknown figure style `{f}`")));
    }
    if let Some(sets) = p.get("stylisticSets").and_then(Value::as_array) {
        let mask = sets.iter().filter_map(Value::as_u64).filter(|n| (1..=20).contains(n)).fold(0u32, |m, n| m | 1 << (n - 1));
        otf::set_stylistic_sets(&mut list, mask);
    }
    format_chars(s, &json!({"otfFeatures": list}))?;
    Ok(json!({"features": list, "figures": otf::figures(&list)}))
}

/// One stretch of the selection: its length in bytes, the font it is set in and its features.
struct FeatureSegment {
    len: usize,
    font: (String, String),
    applied: Vec<String>,
    ligatures: bool,
}

/// The OpenType features of the fonts in the selection, with how much of the selection each one is
/// applied to and how much of it the fonts offer. A caret counts as one byte. `state` is `none`,
/// `some` or `all`.
pub(super) fn open_type_features(s: &mut Session, _p: &Value) -> Result<Value> {
    let st = s.doc()?;
    let mut segments: Vec<FeatureSegment> = Vec::new();
    for Target { story: sid, cell, range: r } in format_targets(s) {
        let Some(story) = st.doc.text_story(sid, cell) else { continue };
        let covered: Vec<(usize, usize, designcraft_doc::story::CharFormat)> = if r.is_empty() {
            vec![(r.start, 1, story.format_after(r.start).clone())]
        } else {
            story
                .runs()
                .filter(|(run, _)| run.start < r.end && run.end > r.start)
                .map(|(run, f)| (run.start, run.end.min(r.end).saturating_sub(run.start.max(r.start)), f.clone()))
                .collect()
        };
        for (pos, len, cf) in covered {
            let Some(para) = story.paras.get(story.para_at(pos)) else { continue };
            let (_, base) = st.doc.styles.resolve_para(para);
            let cp = st.doc.styles.resolve_char(&base, &cf);
            segments.push(FeatureSegment { len, font: (cp.font_family, cp.font_style), applied: cp.otf_features, ligatures: cp.ligatures });
        }
    }
    if segments.is_empty() {
        let (_, base) = st.doc.styles.resolve_para_style(designcraft_doc::story::BASIC_PARAGRAPH);
        let cp = st.doc.styles.resolve_char(&base, &designcraft_doc::story::CharFormat::default());
        segments.push(FeatureSegment { len: 1, font: (cp.font_family, cp.font_style), applied: cp.otf_features, ligatures: cp.ligatures });
    }
    let db = designcraft_fonts::FontDb::global();
    let mut fonts: std::collections::BTreeMap<(String, String), designcraft_fonts::FontFeatures> = std::collections::BTreeMap::new();
    for seg in &segments {
        fonts.entry(seg.font.clone()).or_insert_with(|| designcraft_fonts::font_features(&db.face(&seg.font.0, &seg.font.1)));
    }
    let total: usize = segments.iter().map(|seg| seg.len).sum::<usize>().max(1);
    let state = |covered: usize| match covered {
        0 => "none",
        n if n >= total => "all",
        _ => "some",
    };
    // Bytes the feature is applied to, and bytes whose font offers it.
    let measure = |tag: &str| {
        let (mut applied, mut available) = (0, 0);
        for seg in &segments {
            // Standard ligatures are the `ligatures` attribute, which shaping reads, unless the
            // feature list turns `liga` off.
            let on = designcraft_doc::otf::is_on(&seg.applied, tag) && (tag != "liga" || seg.ligatures);
            if on {
                applied += seg.len;
            }
            if fonts.get(&seg.font).is_some_and(|f| f.tags.contains(tag)) {
                available += seg.len;
            }
        }
        (state(applied), state(available))
    };
    let tags: std::collections::BTreeSet<String> = fonts.values().flat_map(|f| f.tags.iter().cloned()).collect();
    let mut features = Vec::new();
    for tag in tags.iter().filter(|t| !(t.len() == 4 && t.starts_with("ss") && t[2..].parse::<u32>().is_ok())) {
        let (applied, available) = measure(tag);
        features.push(json!({"tag": tag, "applied": applied, "available": available}));
    }
    // A stylistic set's name is shared when every font that offers it gives it the same name.
    let mut sets = Vec::new();
    for set in 1..=20u32 {
        let tag = format!("ss{set:02}");
        if !tags.contains(&tag) {
            continue;
        }
        let names: std::collections::BTreeSet<&String> = fonts.values().filter_map(|f| f.stylistic_sets.get(&set)).collect();
        let name = if names.len() == 1 { names.first().map(ToString::to_string) } else { None };
        let (applied, available) = measure(&tag);
        sets.push(json!({"set": set, "tag": tag, "name": name, "applied": applied, "available": available}));
    }
    let used: Vec<Value> = fonts.keys().map(|(family, style)| json!({"family": family, "style": style})).collect();
    Ok(json!({"fonts": used, "features": features, "stylisticSets": sets}))
}

/// Turns OpenType features on or off across the selected text: `features` maps tags to whether they
/// are on. Each run keeps its other features, whichever font it is set in, and the change is one
/// undo step.
pub(super) fn set_features(s: &mut Session, p: &Value) -> Result<Value> {
    const ID: &str = "type.feature";
    let changes: Vec<(String, bool)> = p
        .get("features")
        .and_then(Value::as_object)
        .ok_or_else(|| bad(ID, "missing features: a map of tags to true or false"))?
        .iter()
        .map(|(tag, on)| Ok((tag.clone(), on.as_bool().ok_or_else(|| bad(ID, format!("`{tag}` needs true or false")))?)))
        .collect::<Result<_>>()?;
    let targets: Vec<Target> = format_targets(s).into_iter().filter(|t| !t.range.is_empty()).collect();
    if targets.is_empty() {
        return Err(bad(ID, "select text"));
    }
    let styles = s.doc()?.doc.styles.clone();
    s.edit(|d, _| {
        for t in &targets {
            let Some(st) = d.text_story_mut(t.story, t.cell) else { continue };
            let runs: Vec<(std::ops::Range<usize>, designcraft_doc::story::CharFormat)> =
                st.runs().filter(|(run, _)| run.start < t.range.end && run.end > t.range.start).map(|(run, f)| (run.clone(), f.clone())).collect();
            for (run, format) in runs {
                let Some(para) = st.paras.get(st.para_at(run.start)).cloned() else { continue };
                let (_, base) = styles.resolve_para(&para);
                let mut list = styles.resolve_char(&base, &format).otf_features;
                for (tag, on) in &changes {
                    designcraft_doc::otf::set(&mut list, tag, *on);
                }
                let range = run.start.max(t.range.start)..run.end.min(t.range.end);
                st.format_chars(range, |f| f.over.otf_features = Some(list.clone()));
            }
        }
        ok()
    })
}

#[cfg(test)]
mod open_type_tests {
    use serde_json::json;

    use crate::Session;

    #[test]
    fn text_selection_stays_on_utf8_boundaries() {
        let mut s = Session::new();
        s.execute("file.new", &json!({})).unwrap();
        let r = s.execute("frame.create", &json!({"rect": [0, 0, 200, 100], "content": "text", "text": "é漢🙂x"})).unwrap();
        let sid = r["story"].clone();
        s.execute("text.select", &json!({"story": sid, "anchor": 1, "focus": 4})).unwrap();
        let t = s.doc().unwrap().selection.text.unwrap();
        assert_eq!((t.anchor, t.focus), (0, 2));
        s.execute("text.select", &json!({"story": sid, "anchor": 4})).unwrap();
        s.execute("text.delete", &json!({"forward": true})).unwrap();
        assert_eq!(s.doc().unwrap().doc.story(t.story).unwrap().text, "é🙂x");
        s.execute("text.select", &json!({"story": sid, "anchor": 2})).unwrap();
        s.execute("story.setText", &json!({"story": sid, "text": "漢字"})).unwrap();
        let t = s.doc().unwrap().selection.text.unwrap();
        assert_eq!((t.anchor, t.focus), (0, 0));
        s.execute("text.move", &json!({"dir": "right"})).unwrap();
        assert_eq!(s.doc().unwrap().selection.text.unwrap().focus, 3);
    }

    #[test]
    fn replacing_a_range_stays_on_utf8_boundaries() {
        let mut s = Session::new();
        s.execute("file.new", &json!({})).unwrap();
        let r = s.execute("frame.create", &json!({"rect": [0, 0, 200, 100], "content": "text", "text": "é漢🙂x"})).unwrap();
        let sid = r["story"].clone();
        let story = designcraft_doc::StoryId(sid.as_u64().unwrap());
        // Offsets inside 漢 (2..5) and 🙂 (5..9), past the end, or reversed are refused and change nothing.
        for (start, end) in [(3, 6), (2, 4), (0, 99), (5, 2)] {
            let r = s.execute("story.replaceRange", &json!({"story": sid, "start": start, "end": end, "text": "字"}));
            assert!(r.is_err_and(|e| e.to_string().contains("10 bytes")), "{start}..{end}");
            assert_eq!(s.doc().unwrap().doc.story(story).unwrap().text, "é漢🙂x");
        }
        s.execute("story.replaceRange", &json!({"story": sid, "start": 2, "end": 5, "text": "字"})).unwrap();
        assert_eq!(s.doc().unwrap().doc.story(story).unwrap().text, "é字🙂x");
        // A caret before 🙂 (byte 5) would land inside it once é (2 bytes) is gone.
        s.execute("text.select", &json!({"story": sid, "anchor": 5})).unwrap();
        s.execute("story.replaceRange", &json!({"story": sid, "start": 0, "end": 2, "text": ""})).unwrap();
        let st = s.doc().unwrap();
        let t = st.selection.text.unwrap();
        let text = &st.doc.story(t.story).unwrap().text;
        assert_eq!(text, "字🙂x");
        assert!(text.is_char_boundary(t.anchor) && text.is_char_boundary(t.focus), "caret {t:?} in {text:?}");
        s.execute("text.move", &json!({"dir": "right"})).unwrap();
    }

    #[test]
    fn selected_text_preview_stops_at_its_character_budget() {
        let read = std::cell::Cell::new(0usize);
        let prefix = ['\t', '\u{2003}', designcraft_doc::OBJECT_MARK];
        let chars = prefix.into_iter().chain(std::iter::repeat_n('字', 100_000)).inspect(|_| read.set(read.get() + 1));
        let preview = super::selected_text_preview(chars);
        assert_eq!(preview, "字".repeat(super::SELECTED_TEXT_CHARS));
        assert_eq!(read.get(), prefix.len() + super::SELECTED_TEXT_CHARS, "the rest of a large selection is never copied or scanned");
        assert_eq!(super::selected_text_preview("A\nB".chars()), "A B");
    }

    #[test]
    fn selection_attrs_report_the_selected_text() {
        let mut s = Session::new();
        s.execute("file.new", &json!({})).unwrap();
        let r = s
            .execute(
                "frame.create",
                &json!({"rect": [72, 72, 300, 200], "content": "text", "text": "Spring issue
What's new in typography this season, and why it matters to every designer"}),
            )
            .unwrap();
        s.execute("text.select", &json!({"story": r["story"], "anchor": 0, "focus": 13})).unwrap();
        assert_eq!(s.execute("type.selectionAttrs", &json!({})).unwrap()["selectedText"], "Spring issue", "on one line, trimmed");
        s.execute("text.select", &json!({"story": r["story"], "anchor": 13, "focus": 200})).unwrap();
        let long = s.execute("type.selectionAttrs", &json!({})).unwrap()["selectedText"].as_str().unwrap().to_string();
        assert!(long.starts_with("What's new") && (55..=60).contains(&long.chars().count()), "the line break skipped, capped: {long}");
        s.execute("text.select", &json!({"story": r["story"], "anchor": 3, "focus": 3})).unwrap();
        assert_eq!(s.execute("type.selectionAttrs", &json!({})).unwrap()["selectedText"], "", "a caret selects nothing");
    }

    #[test]
    fn open_type_toggles_reach_the_text() {
        let mut s = Session::new();
        s.execute("file.new", &json!({})).unwrap();
        let r = s.execute("frame.create", &json!({"rect": [72, 72, 300, 200], "content": "text", "text": "Office 1/2 0"})).unwrap();
        s.execute("text.select", &json!({"story": r["story"], "anchor": 0, "focus": 12})).unwrap();
        let r = s.execute("type.openType", &json!({"feature": "dlig"})).unwrap();
        assert_eq!(r["features"], json!(["dlig"]));
        let r = s.execute("type.openType", &json!({"feature": "dlig"})).unwrap();
        assert_eq!(r["features"], json!([]), "toggles off");
        let r = s.execute("type.openType", &json!({"figures": "proportionalOldstyle", "stylisticSets": [1, 2]})).unwrap();
        assert_eq!(r["figures"], "proportionalOldstyle");
        let a = s.execute("type.selectionAttrs", &json!({})).unwrap();
        let f: Vec<&str> = a["chars"]["otfFeatures"].as_array().unwrap().iter().filter_map(|v| v.as_str()).collect();
        assert!(f.contains(&"onum") && f.contains(&"ss02"), "{f:?}");
        assert!(s.execute("type.openType", &json!({"feature": "bad tag!"})).is_err());
    }

    #[test]
    fn break_link_to_paragraph_and_character_styles() {
        let mut s = Session::new();
        s.execute("file.new", &json!({})).unwrap();
        s.execute("style.paragraph.create", &json!({"name": "Head", "para": {"spaceBefore": 6}, "chars": {"size": 24}})).unwrap();
        s.execute("style.character.create", &json!({"name": "Em", "chars": {"fontStyle": "Italic"}})).unwrap();
        let r = s.execute("frame.create", &json!({"rect": [72, 72, 400, 200], "content": "text", "text": "Title here"})).unwrap();
        let sid = r["story"].as_u64().unwrap();
        s.execute("text.select", &json!({"story": sid, "anchor": 0, "focus": 10})).unwrap();
        s.execute("style.paragraph.apply", &json!({"name": "Head"})).unwrap();
        s.execute("text.select", &json!({"story": sid, "anchor": 0, "focus": 5})).unwrap();
        s.execute("style.character.apply", &json!({"name": "Em"})).unwrap();
        let before = s.execute("type.selectionAttrs", &json!({})).unwrap();
        s.execute("style.breakLink", &json!({"kind": "character"})).unwrap();
        s.execute("text.select", &json!({"story": sid, "anchor": 0, "focus": 10})).unwrap();
        s.execute("style.breakLink", &json!({"kind": "paragraph"})).unwrap();
        s.execute("text.select", &json!({"story": sid, "anchor": 0, "focus": 5})).unwrap();
        let after = s.execute("type.selectionAttrs", &json!({})).unwrap();
        assert_eq!(after["paragraphStyle"], "[No Paragraph Style]");
        assert_eq!(after["characterStyle"], "[None]");
        assert_eq!(after["chars"], before["chars"], "same look");
        assert_eq!(after["para"], before["para"]);
    }

    /// Old-style figures shape to other glyphs than the default lining ones (Source Serif 4).
    #[test]
    fn oldstyle_figures_change_the_glyphs() {
        let mut s = Session::new();
        s.execute("file.new", &json!({})).unwrap();
        let r = s.execute("frame.create", &json!({"rect": [72, 72, 300, 200], "content": "text", "text": "2024"})).unwrap();
        let sid = designcraft_doc::StoryId(r["story"].as_u64().unwrap());
        let gids = |s: &Session| {
            let d = &s.doc().unwrap().doc;
            let cs = designcraft_compose::compose_story(d, sid, &Default::default());
            cs.frames[0].lines[0].glyphs.iter().map(|g| g.gid).collect::<Vec<_>>()
        };
        let lining = gids(&s);
        s.execute("text.select", &json!({"story": sid.0, "anchor": 0, "focus": 4})).unwrap();
        s.execute("type.openType", &json!({"figures": "proportionalOldstyle"})).unwrap();
        assert_ne!(gids(&s), lining);
    }

    /// A blank line (an empty paragraph) has no characters to select, but its hidden paragraph
    /// mark can still take a leading change right away: otherwise there'd be no way to adjust the
    /// line height of a blank line at all.
    #[test]
    fn leading_applies_to_an_empty_lines_hidden_paragraph_mark() {
        let mut s = Session::new();
        s.execute("file.new", &json!({})).unwrap();
        let text = "Para one\n\nPara two";
        let r = s.execute("frame.create", &json!({"rect": [72, 72, 300, 200], "content": "text", "text": text})).unwrap();
        let sid = designcraft_doc::StoryId(r["story"].as_u64().unwrap());
        // The blank line between the two paragraphs: a collapsed caret, since it has no text.
        let blank = text.find("\n\n").unwrap() + 1;
        s.execute("text.select", &json!({"story": sid.0, "anchor": blank})).unwrap();
        s.execute("type.char", &json!({"attrs": {"leading": {"kind": "points", "value": 40.0}}})).unwrap();
        let st = s.doc().unwrap().doc.story(sid).unwrap();
        // `char_format_at` resolves a boundary to the *preceding* run (new typing's format), so
        // check just past the mark's own one-byte run, which the mark alone occupies.
        assert_eq!(st.char_format_at(blank + 1).over.leading, Some(designcraft_doc::Leading::Points(40.0)));
        let d = s.doc().unwrap().doc.clone();
        let cs = designcraft_compose::compose_story(&d, sid, &Default::default());
        let blank_line = cs.frames[0].lines.iter().find(|l| l.range == (blank..blank)).unwrap();
        assert_eq!(blank_line.leading, 40.0, "the blank line's own height changed");
    }

    /// Formatting chosen on a blank line is also what gets typed there, not the format of the
    /// previous paragraph's mark that new text would otherwise take.
    #[test]
    fn text_typed_on_a_blank_line_takes_the_format_set_there() {
        let mut s = Session::new();
        s.execute("file.new", &json!({})).unwrap();
        let text = "Para one\n\nPara two";
        let r = s.execute("frame.create", &json!({"rect": [72, 72, 300, 200], "content": "text", "text": text})).unwrap();
        let sid = designcraft_doc::StoryId(r["story"].as_u64().unwrap());
        let blank = text.find("\n\n").unwrap() + 1;
        s.execute("text.select", &json!({"story": sid.0, "anchor": blank})).unwrap();
        s.execute("type.char", &json!({"attrs": {"size": 30}})).unwrap();
        s.execute("text.insert", &json!({"text": "Middle"})).unwrap();
        let st = s.doc().unwrap().doc.story(sid).unwrap();
        assert_eq!(st.text, "Para one\nMiddle\nPara two");
        for b in blank..blank + "Middle\n".len() {
            assert_eq!(st.format_after(b).over.size, Some(30.0), "byte {b}");
        }
        assert_eq!(st.format_after(blank - 1).over.size, None, "the first paragraph's mark keeps its size");
    }

    /// A blank line keeps its format for typing after the caret has left it: text typed on it
    /// takes its paragraph mark's format, with no pending typing format.
    #[test]
    fn a_blank_lines_mark_format_is_typed_after_the_caret_returns() {
        let mut s = Session::new();
        s.execute("file.new", &json!({})).unwrap();
        let text = "Para one\n\nPara two";
        let r = s.execute("frame.create", &json!({"rect": [72, 72, 300, 200], "content": "text", "text": text})).unwrap();
        let sid = designcraft_doc::StoryId(r["story"].as_u64().unwrap());
        let blank = text.find("\n\n").unwrap() + 1;
        s.execute("text.select", &json!({"story": sid.0, "anchor": blank})).unwrap();
        s.execute("type.char", &json!({"attrs": {"size": 30}})).unwrap();
        s.execute("text.select", &json!({"story": sid.0, "anchor": 2})).unwrap();
        s.execute("text.select", &json!({"story": sid.0, "anchor": blank})).unwrap();
        assert!(s.typing_format.is_none());
        s.execute("text.insert", &json!({"text": "Middle"})).unwrap();
        let st = s.doc().unwrap().doc.story(sid).unwrap();
        assert_eq!(st.text, "Para one\nMiddle\nPara two");
        assert_eq!(st.format_after(blank).over.size, Some(30.0));
        assert_eq!(st.format_after(blank - 1).over.size, None, "the first paragraph's mark keeps its size");
        assert_eq!(st.format_after(2).over.size, None);
    }
}

#[cfg(test)]
mod open_type_features_tests {
    use serde_json::{Value, json};

    use crate::Session;

    fn feature<'a>(v: &'a Value, tag: &str) -> &'a Value {
        v["features"].as_array().unwrap().iter().find(|f| f["tag"] == tag).unwrap_or(&Value::Null)
    }

    #[test]
    fn the_selected_fonts_features_are_listed_with_their_state() {
        let mut s = Session::new();
        s.execute("file.new", &json!({})).unwrap();
        let r = s.execute("frame.create", &json!({"rect": [72, 72, 400, 200], "content": "text", "text": "Hello fi"})).unwrap();
        s.execute("text.select", &json!({"story": r["story"], "anchor": 0, "focus": 8})).unwrap();
        let v = s.execute("type.openTypeFeatures", &json!({})).unwrap();
        assert_eq!(v["fonts"][0]["family"], "Source Serif 4", "{v}");
        assert_eq!(feature(&v, "liga")["applied"], "all", "liga is on by default: {v}");
        assert_eq!(feature(&v, "kern")["available"], "all", "{v}");
    }

    #[test]
    fn a_feature_on_part_of_the_selection_is_some_and_on_all_of_it_is_all() {
        let mut s = Session::new();
        s.execute("file.new", &json!({})).unwrap();
        let r = s.execute("frame.create", &json!({"rect": [72, 72, 400, 200], "content": "text", "text": "Hello world"})).unwrap();
        let story = r["story"].clone();
        s.execute("text.select", &json!({"story": story, "anchor": 0, "focus": 5})).unwrap();
        s.execute("type.feature", &json!({"features": {"lnum": true}})).unwrap();
        s.execute("text.select", &json!({"story": story, "anchor": 0, "focus": 11})).unwrap();
        let v = s.execute("type.openTypeFeatures", &json!({})).unwrap();
        assert_eq!(feature(&v, "lnum")["applied"], "some", "{v}");
        s.execute("text.select", &json!({"story": story, "anchor": 0, "focus": 5})).unwrap();
        let v = s.execute("type.openTypeFeatures", &json!({})).unwrap();
        assert_eq!(feature(&v, "lnum")["applied"], "all", "{v}");
        s.execute("type.feature", &json!({"features": {"lnum": false}})).unwrap();
        s.execute("text.select", &json!({"story": story, "anchor": 0, "focus": 11})).unwrap();
        let v = s.execute("type.openTypeFeatures", &json!({})).unwrap();
        assert_eq!(feature(&v, "lnum")["applied"], "none", "{v}");
    }

    #[test]
    fn standard_ligatures_follow_the_ligatures_attribute() {
        let mut s = Session::new();
        s.execute("file.new", &json!({})).unwrap();
        let r = s.execute("frame.create", &json!({"rect": [72, 72, 400, 200], "content": "text", "text": "office"})).unwrap();
        s.execute("text.select", &json!({"story": r["story"], "anchor": 0, "focus": 6})).unwrap();
        s.execute("type.char", &json!({"attrs": {"ligatures": false}})).unwrap();
        let v = s.execute("type.openTypeFeatures", &json!({})).unwrap();
        assert_eq!(feature(&v, "liga")["applied"], "none", "Character › Ligatures off: {v}");
        s.execute("type.char", &json!({"attrs": {"ligatures": true}})).unwrap();
        let v = s.execute("type.openTypeFeatures", &json!({})).unwrap();
        assert_eq!(feature(&v, "liga")["applied"], "all", "{v}");
        s.execute("type.feature", &json!({"features": {"liga": false}})).unwrap();
        let v = s.execute("type.openTypeFeatures", &json!({})).unwrap();
        assert_eq!(feature(&v, "liga")["applied"], "none", "-liga in the feature list: {v}");
    }
}
