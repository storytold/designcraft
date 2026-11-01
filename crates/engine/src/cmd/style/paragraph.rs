use super::{CharAttrs, CharacterStyle, ParaAttrs, ParagraphStyle, Result, Session, Styles, Value, bad, json, ok, str_param};

#[cfg(test)]
mod redefine_tests {
    use super::*;

    fn sample() -> (Session, Value, Value) {
        let mut s = Session::new();
        s.execute("file.new", &json!({})).unwrap();
        let a = s.execute("frame.create", &json!({"rect":[36,36,300,200],"content":"text","text":"First header"})).unwrap();
        let b = s.execute("frame.create", &json!({"rect":[36,240,300,400],"content":"text","text":"Second header"})).unwrap();
        s.execute("style.paragraph.create", &json!({"name":"Header","chars":{"size":18,"fill":"[Paper]"}})).unwrap();
        for story in [&a["story"], &b["story"]] {
            s.execute("text.select", &json!({"story":story,"anchor":0,"focus":5})).unwrap();
            s.execute("style.paragraph.apply", &json!({"name":"Header"})).unwrap();
        }
        s.execute("text.select", &json!({"story":a["story"],"anchor":0,"focus":5})).unwrap();
        (s, a["story"].clone(), b["story"].clone())
    }

    #[test]
    fn redefine_paragraph_updates_other_stories_and_clears_the_source_overrides() {
        let (mut s, a, b) = sample();
        s.execute("style.character.create", &json!({"name":"Emphasis","chars":{"underline":true}})).unwrap();
        s.execute("style.character.apply", &json!({"name":"Emphasis"})).unwrap();
        s.execute("type.char", &json!({"size":20,"fill":"[Black]"})).unwrap();
        s.execute("type.para", &json!({"spaceAfter":9})).unwrap();
        let before = s.doc().unwrap().doc.clone();
        let undo = s.doc().unwrap().history.undo.len();
        s.execute("style.paragraph.redefine", &json!({})).unwrap();
        assert_eq!(s.doc().unwrap().history.undo.len(), undo + 1);
        assert!(s.doc().unwrap().doc.styles.para("Header").unwrap().chars.underline.is_none());
        for story in [&a, &b] {
            s.execute("text.select", &json!({"story":story,"anchor":0,"focus":5})).unwrap();
            let attrs = s.execute("type.selectionAttrs", &json!({})).unwrap();
            assert_eq!(attrs["chars"]["size"], 20.0);
            assert_eq!(attrs["chars"]["fill"], "[Black]");
            assert_eq!(attrs["para"]["spaceAfter"], 9.0);
            assert_eq!(attrs["chars"]["underline"], story == &a);
        }
        s.execute("style.paragraph.edit", &json!({"name":"Header","chars":{"size":22}})).unwrap();
        s.execute("text.select", &json!({"story":a,"anchor":0,"focus":5})).unwrap();
        assert_eq!(s.execute("type.selectionAttrs", &json!({})).unwrap()["chars"]["size"], 22.0);
        s.execute("edit.undo", &json!({})).unwrap();
        s.execute("edit.undo", &json!({})).unwrap();
        assert_eq!(s.doc().unwrap().doc, before);
    }

    #[test]
    fn redefine_character_does_not_capture_paragraph_font_size() {
        let (mut s, a, b) = sample();
        s.execute("style.character.create", &json!({"name":"Underline","chars":{"underline":true}})).unwrap();
        for story in [&a, &b] {
            s.execute("text.select", &json!({"story":story,"anchor":0,"focus":5})).unwrap();
            s.execute("style.character.apply", &json!({"name":"Underline"})).unwrap();
        }
        s.execute("type.char", &json!({"fill":"[Black]"})).unwrap();
        s.execute("style.character.redefine", &json!({"name":"Underline"})).unwrap();
        let style = s.doc().unwrap().doc.styles.char_style("Underline").unwrap();
        assert!(style.chars.size.is_none());
        assert_eq!(style.chars.underline, Some(true));
        for story in [&a, &b] {
            s.execute("text.select", &json!({"story":story,"anchor":0,"focus":5})).unwrap();
            assert_eq!(s.execute("type.selectionAttrs", &json!({})).unwrap()["chars"]["fill"], "[Black]");
        }
    }

    #[test]
    fn redefine_rejects_the_wrong_style_and_the_none_entry_atomically() {
        let (mut s, _, _) = sample();
        let before = s.doc().unwrap().doc.clone();
        let undo = s.doc().unwrap().history.undo.len();
        assert!(s.execute("style.paragraph.redefine", &json!({"name":"Other"})).is_err());
        assert!(s.execute("style.character.redefine", &json!({})).is_err());
        assert_eq!(s.doc().unwrap().doc, before);
        assert_eq!(s.doc().unwrap().history.undo.len(), undo);
    }

    #[test]
    fn redefine_character_at_a_caret_cleans_the_source_run_but_keeps_other_overrides() {
        let (mut s, story, _) = sample();
        s.execute("style.character.create", &json!({"name":"Emphasis","chars":{"underline":true}})).unwrap();
        for (start, end, size) in [(0, 5, 24), (6, 12, 32)] {
            s.execute("text.select", &json!({"story":story,"anchor":start,"focus":end})).unwrap();
            s.execute("style.character.apply", &json!({"name":"Emphasis"})).unwrap();
            s.execute("type.char", &json!({"size":size})).unwrap();
        }
        let before = s.doc().unwrap().doc.clone();
        s.execute("text.select", &json!({"story":story,"anchor":2,"focus":2})).unwrap();
        s.execute("style.character.redefine", &json!({"name":"Emphasis"})).unwrap();
        s.execute("style.character.edit", &json!({"name":"Emphasis","chars":{"size":30}})).unwrap();
        for (start, end, size) in [(0, 5, 30.0), (6, 12, 32.0)] {
            s.execute("text.select", &json!({"story":story,"anchor":start,"focus":end})).unwrap();
            assert_eq!(s.execute("type.selectionAttrs", &json!({})).unwrap()["chars"]["size"], size);
        }
        s.execute("edit.undo", &json!({})).unwrap();
        s.execute("edit.undo", &json!({})).unwrap();
        assert_eq!(s.doc().unwrap().doc, before);
    }
}

#[cfg(test)]
mod application_inheritance_tests {
    use super::*;

    fn setup(text: &str) -> (Session, Value) {
        let mut s = Session::new();
        s.execute("file.new", &json!({})).unwrap();
        let f = s.execute("frame.create", &json!({"rect": [36,36,300,300], "content": "text", "text": text})).unwrap();
        (s, f["story"].clone())
    }
    fn select(s: &mut Session, story: &Value, start: usize, end: usize) {
        s.execute("text.select", &json!({"story": story, "anchor": start, "focus": end})).unwrap();
    }
    fn attrs(s: &mut Session) -> Value {
        s.execute("type.selectionAttrs", &json!({})).unwrap()
    }

    #[test]
    fn paragraph_capture_source_follows_later_style_edits_and_undo() {
        let (mut s, story) = setup("First\nSecond");
        select(&mut s, &story, 0, 5);
        s.execute("type.char", &json!({"size": 18, "fill": "[Paper]"})).unwrap();
        s.execute("type.para", &json!({"spaceAfter": 6})).unwrap();
        s.execute("style.paragraph.create", &json!({"name": "Header", "fromSelection": true})).unwrap();
        let before = s.doc().unwrap().doc.clone();
        let undo = s.doc().unwrap().history.undo.len();
        s.execute("style.paragraph.apply", &json!({"name": "Header"})).unwrap();
        assert_eq!(s.doc().unwrap().history.undo.len(), undo + 1);
        s.execute("edit.undo", &json!({})).unwrap();
        assert_eq!(s.doc().unwrap().doc, before);
        s.execute("edit.redo", &json!({})).unwrap();
        select(&mut s, &story, 6, 6);
        s.execute("style.paragraph.apply", &json!({"name": "Header"})).unwrap();
        s.execute("style.paragraph.edit", &json!({"name": "Header", "chars": {"size": 20, "fill": "[Black]"}, "para": {"spaceAfter": 12}})).unwrap();
        for pos in [0, 6] {
            select(&mut s, &story, pos, pos + 1);
            let a = attrs(&mut s);
            assert_eq!(a["chars"]["size"], 20.0);
            assert_eq!(a["chars"]["fill"], "[Black]");
            assert_eq!(a["para"]["spaceAfter"], 12.0);
        }
    }

    #[test]
    fn applying_styles_keeps_different_overrides_and_named_character_styles() {
        let (mut s, story) = setup("First\nSecond");
        select(&mut s, &story, 0, 5);
        s.execute("type.char", &json!({"size": 14})).unwrap();
        s.execute("type.para", &json!({"spaceAfter": 9})).unwrap();
        s.execute("style.character.create", &json!({"name": "Emphasis", "chars": {"underline": true}})).unwrap();
        s.execute("style.character.apply", &json!({"name": "Emphasis"})).unwrap();
        s.execute("style.paragraph.create", &json!({"name": "Body", "chars": {"size": 10}, "para": {"spaceAfter": 6}})).unwrap();
        s.execute("style.paragraph.apply", &json!({"name": "Body"})).unwrap();
        let a = attrs(&mut s);
        assert_eq!(a["chars"]["size"], 14.0);
        assert_eq!(a["para"]["spaceAfter"], 9.0);
        assert_eq!(a["characterStyle"], "Emphasis");
        assert_eq!(a["chars"]["underline"], true);
    }

    #[test]
    fn character_capture_source_remains_sparse_and_follows_style_edits() {
        let (mut s, story) = setup("First\nSecond");
        select(&mut s, &story, 0, 5);
        s.execute("type.char", &json!({"underline": true})).unwrap();
        s.execute("style.character.create", &json!({"name": "Emphasis", "fromSelection": true})).unwrap();
        s.execute("style.character.apply", &json!({"name": "Emphasis"})).unwrap();
        s.execute("style.character.edit", &json!({"name": "Emphasis", "chars": {"underline": false}})).unwrap();
        assert_eq!(attrs(&mut s)["chars"]["underline"], false);
        assert!(s.doc().unwrap().doc.styles.char_style("Emphasis").unwrap().chars.size.is_none());
        s.execute("style.paragraph.edit", &json!({"name": designcraft_doc::BASIC_PARAGRAPH, "chars": {"size": 20}})).unwrap();
        assert_eq!(attrs(&mut s)["chars"]["size"], 20.0);
    }

    #[test]
    fn paragraph_selection_excludes_the_next_paragraph_at_its_start() {
        let (mut s, story) = setup("é\nNext");
        select(&mut s, &story, 0, 3);
        s.execute("style.paragraph.create", &json!({"name": "Header"})).unwrap();
        s.execute("style.paragraph.apply", &json!({"name": "Header", "clearOverrides": true})).unwrap();
        select(&mut s, &story, 3, 4);
        assert_eq!(attrs(&mut s)["paragraphStyle"], designcraft_doc::BASIC_PARAGRAPH);
        select(&mut s, &story, 0, 2);
        assert_eq!(attrs(&mut s)["paragraphStyle"], "Header");
    }

    #[test]
    fn clearing_a_partial_selection_preserves_character_overrides_outside_it() {
        let (mut s, story) = setup("First Second");
        select(&mut s, &story, 0, 12);
        s.execute("type.char", &json!({"size": 18})).unwrap();
        select(&mut s, &story, 0, 5);
        s.execute("style.paragraph.apply", &json!({"name": designcraft_doc::BASIC_PARAGRAPH, "clearOverrides": true})).unwrap();
        assert_ne!(attrs(&mut s)["chars"]["size"], 18.0);
        select(&mut s, &story, 6, 12);
        assert_eq!(attrs(&mut s)["chars"]["size"], 18.0);
    }
}

/// The `group` (`chars`, `para`) object of command `cmd`'s params as attributes; a bad attribute
/// is that command's parameter error, naming the group.
fn attrs<T: Default>(cmd: &str, group: &str, p: &Value, set: impl Fn(&mut T, &str, &Value) -> std::result::Result<(), String>) -> Result<T> {
    let mut a = T::default();
    match p.get(group) {
        None | Some(Value::Null) => {}
        Some(Value::Object(o)) => {
            for (k, v) in o {
                set(&mut a, k, v).map_err(|e| bad(cmd, format!("{group}: {e}")))?;
            }
        }
        Some(_) => return Err(bad(cmd, format!("{group} must be an object of attributes"))),
    }
    Ok(a)
}

pub(super) fn redefine(s: &mut Session, p: &Value, paragraph: bool) -> Result<Value> {
    let command = if paragraph { "style.paragraph.redefine" } else { "style.character.redefine" };
    let targets = crate::cmd::text::format_targets_pub(s);
    let target = targets.first().ok_or_else(|| bad(command, "select text using the style to redefine"))?;
    let state = s.doc()?;
    let story = state.doc.text_story(target.story, target.cell).ok_or(designcraft_doc::DocError::NoStory(target.story))?;
    let pf = story.paras.get(story.para_at(target.range.start)).ok_or_else(|| bad(command, "no paragraph at the selection"))?;
    let cf = crate::cmd::text::effective_char_format(s, target)?;
    let selected = if paragraph { &pf.style } else { &cf.style };
    let name = str_param(p, "name").unwrap_or(selected).to_string();
    if name != *selected || name == if paragraph { designcraft_doc::NO_PARA_STYLE } else { designcraft_doc::NO_CHAR_STYLE } {
        return Err(bad(command, "select text using the named style; the no-style entry cannot be redefined"));
    }
    let (base_para, base_chars) = state.doc.styles.resolve_para_style(&pf.style);
    let (resolved_para, paragraph_chars) = state.doc.styles.resolve_para(pf);
    let para_changes = base_para.delta(&resolved_para);
    let mut current = if paragraph {
        paragraph_chars.clone()
    } else {
        state.doc.styles.resolve_char(&paragraph_chars, &designcraft_doc::CharFormat { style: name.clone(), over: CharAttrs::default() })
    };
    let inherited = if paragraph { base_chars } else { current.clone() };
    // A named character style is an intentional layer, not a paragraph override to absorb.
    current.apply(&cf.over);
    let char_changes = inherited.delta(&current);
    let result = s.edit(|d, _| {
        if paragraph {
            let style = d.styles_mut().para_mut(&name).ok_or_else(|| bad(command, "paragraph style no longer exists"))?;
            style.para.merge(&para_changes);
            style.chars.merge(&char_changes);
        } else {
            let style =
                d.styles_mut().character.iter_mut().find(|st| st.name == name).ok_or_else(|| bad(command, "character style no longer exists"))?;
            style.chars.merge(&char_changes);
        }
        let styles = d.styles.clone();
        for target in &targets {
            let Some(story) = d.text_story_mut(target.story, target.cell) else { continue };
            // At a character-style caret, the sampled format belongs to the containing run
            // (the preceding run at a boundary). Normalize that source run too, otherwise its
            // old local values would mask the next edit of the redefined style.
            let mut selected_range = target.range.clone();
            if !paragraph && selected_range.is_empty() {
                let caret = selected_range.start;
                let mut start = 0usize;
                for run in &story.chars {
                    let end = start.saturating_add(run.len).min(story.len());
                    if caret < end || (caret == end && caret > start) {
                        selected_range = start..end;
                        break;
                    }
                    start = end;
                }
            }
            let end = if selected_range.is_empty() { selected_range.start } else { selected_range.end.saturating_sub(1) };
            let (first, last) = (story.para_at(selected_range.start), story.para_at(end));
            for (index, range) in story.para_ranges().into_iter().enumerate().take(last.saturating_add(1)).skip(first) {
                let Some(pf) = story.paras.get_mut(index) else { continue };
                if paragraph && pf.style != name {
                    continue;
                }
                if paragraph {
                    let (pp, cp) = styles.resolve_para(pf);
                    let (base_p, base_c) = styles.resolve_para_style(&name);
                    pf.para = base_p.delta(&pp);
                    pf.chars = base_c.delta(&cp);
                }
                let (_, base) = styles.resolve_para(pf);
                let end = range.end.saturating_add(1).min(story.len());
                let range = if paragraph { range.start..end } else { range.start.max(selected_range.start)..end.min(selected_range.end) };
                story.format_chars(range, |format| {
                    if !paragraph && format.style != name {
                        return;
                    }
                    let mut inherited = base.clone();
                    inherited.apply(&styles.char_style_attrs(&format.style));
                    let resolved = styles.resolve_char(&base, format);
                    format.over = inherited.delta(&resolved);
                });
            }
        }
        Ok(json!({"name":name}))
    });
    if result.is_ok() {
        s.typing_format = None;
    }
    result
}

/// Apply paragraph style `name` over `r` in one story; `clear` drops overrides instead of keeping
/// only those that differ from the style (`resolved` is the style's paragraph and character attributes).
fn apply_para_in_story(
    st: &mut designcraft_doc::Story,
    r: &std::ops::Range<usize>,
    name: &str,
    clear: bool,
    resolved: (&designcraft_doc::ParaProps, &designcraft_doc::CharProps),
    styles: &designcraft_doc::Styles,
) {
    let (style_para, style_chars) = resolved;
    // Text selections are half-open: a selection ending at the next paragraph's
    // start must not apply a style to that paragraph too.
    let end = if r.is_empty() { r.start } else { r.end.saturating_sub(1) };
    let (first, last) = (st.para_at(r.start), st.para_at(end));
    st.format_paras(r.start..end, |f| {
        f.style = name.to_string();
        if clear {
            f.para = ParaAttrs::default();
            f.chars = CharAttrs::default();
        } else {
            f.para.retain_differences(style_para);
            f.chars.retain_differences(style_chars);
        }
    });
    for (pi, range) in st.paras_touching(r.clone()).into_iter().filter(|(pi, _)| (first..=last).contains(pi)) {
        let Some(pf) = st.paras.get(pi) else { continue };
        let (_, base) = styles.resolve_para(pf);
        let end = range.end.saturating_add(1).min(st.len());
        let range = if clear && !r.is_empty() { range.start.max(r.start)..end.min(r.end) } else { range.start..end };
        st.format_chars(range, |f| {
            if clear {
                f.over = CharAttrs::default();
            } else {
                let mut inherited = base.clone();
                inherited.apply(&styles.char_style_attrs(&f.style));
                f.over.retain_differences(&inherited);
            }
        });
    }
}

/// Paragraph style `name` applied to the paragraphs of `r` in `st`: the paragraphs' and their
/// text's overrides that the style now gives go (all of them with `clear`).
pub(crate) fn apply_para_style(st: &mut designcraft_doc::Story, styles: &Styles, name: &str, r: &std::ops::Range<usize>, clear: bool) {
    let (style_para, style_chars) = styles.resolve_para_style(name);
    apply_para_in_story(st, r, name, clear, (&style_para, &style_chars), styles);
}

pub(super) fn apply_para(s: &mut Session, p: &Value) -> Result<Value> {
    let name = str_param(p, "name").ok_or_else(|| bad("style.paragraph.apply", "missing name"))?.to_string();
    let clear = p.get("clearOverrides").and_then(Value::as_bool).unwrap_or(false);
    if s.doc()?.doc.styles.para(&name).is_none() {
        return Err(bad("style.paragraph.apply", format!("no paragraph style `{name}`")));
    }
    let targets = crate::cmd::text::format_targets_pub(s);
    s.edit(|d, _| {
        let styles = d.styles.clone();
        let (style_para, style_chars) = styles.resolve_para_style(&name);
        for t in &targets {
            if let Some(st) = d.text_story_mut(t.story, t.cell) {
                apply_para_in_story(st, &t.range, &name, clear, (&style_para, &style_chars), &styles);
            }
        }
        ok()
    })
}

pub(super) fn apply_char(s: &mut Session, p: &Value) -> Result<Value> {
    let name = str_param(p, "name").ok_or_else(|| bad("style.character.apply", "missing name"))?.to_string();
    if s.doc()?.doc.styles.char_style(&name).is_none() {
        return Err(bad("style.character.apply", format!("no character style `{name}`")));
    }
    let targets = crate::cmd::text::format_targets_pub(s);
    s.edit(|d, _| {
        let styles = d.styles.clone();
        for t in &targets {
            if let Some(st) = d.text_story_mut(t.story, t.cell) {
                apply_char_style(st, &styles, &name, &t.range);
            }
        }
        ok()
    })
}

/// Character style `name` applied to `r` in `st`: the text's overrides that the style (over its
/// paragraph) now gives go.
pub(crate) fn apply_char_style(st: &mut designcraft_doc::Story, styles: &Styles, name: &str, r: &std::ops::Range<usize>) {
    for (pi, range) in st.paras_touching(r.clone()) {
        let range = range.start.max(r.start)..range.end.saturating_add(1).min(st.len()).min(r.end);
        if range.is_empty() {
            continue;
        }
        let Some(pf) = st.paras.get(pi) else { continue };
        let (_, mut inherited) = styles.resolve_para(pf);
        inherited.apply(&styles.char_style_attrs(name));
        st.format_chars(range, |f| {
            f.style = name.to_string();
            f.over.retain_differences(&inherited);
        });
    }
}

/// In a style, `startAt: null` is Continue from Previous Number: it is stored as an explicit
/// "continue" so it also overrides a Start At the style would inherit.
fn continue_numbering(p: Option<&Value>, para: &mut ParaAttrs) {
    let explicit =
        p.and_then(Value::as_object).is_some_and(|o| o.iter().any(|(k, v)| v.is_null() && k.replace('_', "").eq_ignore_ascii_case("startAt")));
    if explicit {
        para.start_at = Some(None);
    }
}

/// The style `p[key]` names: none when absent or null; anything but a name is refused.
fn style_name_param(cmd: &str, p: &Value, key: &str) -> Result<Option<String>> {
    match p.get(key) {
        None | Some(Value::Null) => Ok(None),
        Some(Value::String(n)) => Ok(Some(n.clone())),
        Some(_) => Err(bad(cmd, format!("`{key}` is a style name or null"))),
    }
}

pub(super) fn create_para(s: &mut Session, p: &Value) -> Result<Value> {
    let base = str_param(p, "name").unwrap_or("Paragraph Style 1").to_string();
    let based_on = style_name_param("style.paragraph.create", p, "basedOn")?;
    let next = style_name_param("style.paragraph.create", p, "nextStyle")?;
    let mut para: ParaAttrs =
        attrs("style.paragraph.create", "para", p, |a: &mut ParaAttrs, k, v| a.set_json_over(k, v, &designcraft_doc::ParaProps::default()))?;
    continue_numbering(p.get("para"), &mut para);
    explicit_default_kinsoku(&mut para, p.get("para"));
    check_grep_styles("style.paragraph.create", &para)?;
    let mut chars: CharAttrs = attrs("style.paragraph.create", "chars", p, |a: &mut CharAttrs, k, v| a.set_json(k, v))?;
    // Validate caller-supplied fills, not selection fills retained after a swatch is deleted.
    crate::cmd::text::validate_char_fill(&s.doc()?.doc, &chars, "style.paragraph.create")?;
    if p.get("fromSelection").and_then(Value::as_bool).unwrap_or(false)
        && let Ok(cur) = s.execute("type.selectionAttrs", &json!({}))
        && !cur.is_null()
    {
        if let Ok(pp) = serde_json::from_value::<designcraft_doc::ParaProps>(cur["para"].clone()) {
            let mut full = ParaProps_to_attrs(&pp);
            full.merge(&para);
            para = full;
        }
        if let Ok(cp) = serde_json::from_value::<designcraft_doc::CharProps>(cur["chars"].clone()) {
            let mut full = CharProps_to_attrs(&cp);
            full.merge(&chars);
            chars = full;
        }
    }
    s.edit(|d, _| {
        let name = Styles::unique_name(|n| d.styles.para(n).is_some(), &base);
        if let Some(b) = &based_on
            && d.styles.para(b).is_none()
        {
            return Err(bad("style.paragraph.create", format!("no style `{b}`")));
        }
        // Next Style: a style of the document, or this one.
        if let Some(n) = &next
            && *n != name
            && d.styles.para(n).is_none()
        {
            return Err(bad("style.paragraph.create", format!("no style `{n}`")));
        }
        d.styles_mut().paragraph.push(ParagraphStyle { name: name.clone(), based_on, next_style: next, para, chars, shortcut: String::new() });
        Ok(json!({"name": name}))
    })
}

/// `kinsoku: null` sets the composer's default kinsoku rules, also over a Based On style's set.
/// For the other attributes `null` leaves the style's value as it is.
fn explicit_default_kinsoku(para: &mut ParaAttrs, given: Option<&Value>) {
    if given.and_then(|o| o.get("kinsoku")).is_some_and(Value::is_null) {
        para.kinsoku = Some(None);
    }
}

/// The compile error of the first GREP style in `a` whose pattern can't compile.
pub(crate) fn grep_style_error(a: &ParaAttrs) -> Option<String> {
    a.grep_styles
        .iter()
        .flatten()
        .filter(|g| !g.pattern.is_empty())
        .find_map(|g| designcraft_compose::grep::Grep::new(&g.pattern, Default::default()).err().map(|e| format!("{e} (pattern {:?})", g.pattern)))
}

/// Rejects GREP styles a command would set with a pattern that can't compile.
pub(crate) fn check_grep_styles(cmd: &str, a: &ParaAttrs) -> Result<()> {
    grep_style_error(a).map_or(Ok(()), |e| Err(bad(cmd, e)))
}

/// One warning for each paragraph style or paragraph whose GREP style pattern can't compile.
/// Such a GREP style stays in the document and composition skips it.
pub(crate) fn grep_style_warnings(d: &designcraft_doc::Document) -> Vec<String> {
    let mut out: Vec<String> = d
        .styles
        .paragraph
        .iter()
        .filter_map(|st| grep_style_error(&st.para).map(|e| format!("paragraph style {:?}: GREP style not applied: {e}", st.name)))
        .collect();
    for st in d.stories.values() {
        for (i, pf) in st.paras.iter().enumerate() {
            if let Some(e) = grep_style_error(&pf.para) {
                out.push(format!("story {} paragraph {}: GREP style not applied: {e}", st.id.0, i + 1));
            }
        }
    }
    out
}

#[allow(non_snake_case)]
fn ParaProps_to_attrs(p: &designcraft_doc::ParaProps) -> ParaAttrs {
    designcraft_doc::ParaProps::common([p])
}

#[allow(non_snake_case)]
fn CharProps_to_attrs(p: &designcraft_doc::CharProps) -> CharAttrs {
    designcraft_doc::CharProps::common([p])
}

pub(super) fn edit_para(s: &mut Session, p: &Value) -> Result<Value> {
    let name = str_param(p, "name").ok_or_else(|| bad("style.paragraph.edit", "missing name"))?.to_string();
    let chars: CharAttrs = attrs("style.paragraph.edit", "chars", p, |a: &mut CharAttrs, k, v| a.set_json(k, v))?;
    crate::cmd::text::validate_char_fill(&s.doc()?.doc, &chars, "style.paragraph.edit")?;
    let rename = str_param(p, "rename").map(str::trim).filter(|n| *n != name).map(str::to_string);
    let based = p.get("basedOn").cloned();
    let next = p.get("nextStyle").cloned();
    let reset = p.get("resetToBase").and_then(Value::as_bool).unwrap_or(false);
    for (key, v) in [("basedOn", &based), ("nextStyle", &next)] {
        if v.as_ref().is_some_and(|v| !v.is_string() && !v.is_null()) {
            return Err(bad("style.paragraph.edit", format!("`{key}` must be a style name or null")));
        }
    }
    s.edit(|d, _| {
        if let Some(Value::String(b)) = &based {
            if d.styles.para(b).is_none() {
                return Err(bad("style.paragraph.edit", format!("no style `{b}`")));
            }
            if d.styles.para_based_on_cycles(&name, b) {
                return Err(bad("style.paragraph.edit", "based-on would create a cycle"));
            }
        }
        if let Some(Value::String(n)) = &next
            && d.styles.para(n).is_none()
        {
            return Err(bad("style.paragraph.edit", format!("no style `{n}`")));
        }
        // Reset To Base: what the style set of its own goes before the edits apply.
        if reset {
            let st = d.styles_mut().para_mut(&name).ok_or_else(|| bad("style.paragraph.edit", format!("no style `{name}`")))?;
            st.para = ParaAttrs::default();
            st.chars = CharAttrs::default();
        }
        // A new Based On first, so the fields below resolve over the new parent.
        if based.is_some() {
            let st = d.styles_mut().para_mut(&name).ok_or_else(|| bad("style.paragraph.edit", format!("no style `{name}`")))?;
            match &based {
                Some(Value::String(b)) => st.based_on = Some(b.clone()),
                Some(Value::Null) => st.based_on = None,
                _ => {}
            }
        }
        if let Some(n) = &rename {
            if n.is_empty() || n.starts_with('[') {
                return Err(bad("style.paragraph.edit", format!("can't rename to `{n}`")));
            }
            if d.styles.para(n).is_some() {
                return Err(bad("style.paragraph.edit", format!("a paragraph style named `{n}` already exists")));
            }
        }
        // A rule object changes only the rule fields it names, over the style's resolved rule.
        let (current, _) = d.styles.resolve_para_style(&name);
        let mut para: ParaAttrs = attrs("style.paragraph.edit", "para", p, |a: &mut ParaAttrs, k, v| a.set_json_over(k, v, &current))?;
        continue_numbering(p.get("para"), &mut para);
        explicit_default_kinsoku(&mut para, p.get("para"));
        check_grep_styles("style.paragraph.edit", &para)?;
        let st = d.styles_mut().para_mut(&name).ok_or_else(|| bad("style.paragraph.edit", format!("no style `{name}`")))?;
        st.para.merge(&para);
        st.chars.merge(&chars);
        match next {
            Some(Value::String(n)) => st.next_style = Some(n),
            Some(Value::Null) => st.next_style = None,
            _ => {}
        }
        if let Some(n) = rename.clone() {
            // Every use: other styles, stories, table cells, footnotes, object styles.
            rename_style(d, true, &name, &n);
        }
        ok()
    })
}

/// Rename a paragraph or character style everywhere it's used; its Export Tagging follows.
fn rename_style(d: &mut designcraft_doc::Document, para: bool, from: &str, to: &str) {
    let st = d.styles_mut();
    if para {
        for s in &mut st.paragraph {
            if s.name == from {
                s.name = to.to_string();
            }
        }
    } else {
        for s in &mut st.character {
            if s.name == from {
                s.name = to.to_string();
            }
        }
    }
    // An entry already under `to` belongs to no style (names are unique), so it never survives.
    st.export_tags.remove(&export_tag_key(para, to));
    if let Some(tag) = st.export_tags.remove(&export_tag_key(para, from)) {
        st.export_tags.insert(export_tag_key(para, to), tag);
    }
    redirect_style_refs(d, para, from, to);
}

/// The `Styles::export_tags` key of a paragraph (`para`) or character style.
pub(super) fn export_tag_key(para: bool, name: &str) -> String {
    format!("{}:{name}", if para { "p" } else { "c" })
}

/// Point every reference to style `from` at `to`, except the style's own registry entry (its
/// `name` field) — shared by renaming (which also updates the entry's name) and deletion (which
/// instead removes the entry, after its former uses point at the replacement).
fn redirect_style_refs(d: &mut designcraft_doc::Document, para: bool, from: &str, to: &str) {
    let rename = |r: &mut String| {
        if r == from {
            *r = to.to_string();
        }
    };
    let st = d.styles_mut();
    // A style is never based on itself: when `to` was based on `from`, it takes `from`'s parent.
    let parent_of = |based_on: Option<&String>| based_on.filter(|b| b.as_str() != to).cloned();
    if para {
        let from_parent = parent_of(st.paragraph.iter().find(|x| x.name == from).and_then(|x| x.based_on.as_ref()));
        for s in &mut st.paragraph {
            if s.based_on.as_deref() == Some(from) {
                s.based_on = if s.name == to { from_parent.clone() } else { Some(to.to_string()) };
            }
            if s.next_style.as_deref() == Some(from) {
                s.next_style = Some(to.to_string());
            }
        }
        let objects = st.object.iter_mut().map(|o| &mut o.paragraph_style);
        for r in objects.chain(st.cell.iter_mut().map(|c| &mut c.paragraph_style)) {
            if r.as_deref() == Some(from) {
                *r = Some(to.to_string());
            }
        }
        rename(&mut st.default_paragraph);
        let (fo, eo) = (&mut d.footnote_options, &mut d.endnote_options);
        [&mut fo.para_style, &mut eo.para_style, &mut eo.heading_style].into_iter().for_each(rename);
        d.toc.iter_mut().flat_map(|t| t.entries.iter_mut().map(|e| &mut e.style)).for_each(rename);
        for t in d.toc.iter_mut() {
            let formats = t.entries.iter_mut().map(|e| &mut e.entry_style).chain([&mut t.title_style]);
            for r in formats.filter(|r| r.as_deref() == Some(from)) {
                *r = Some(to.to_string());
            }
        }
        if let designcraft_doc::EpubSplit::Style(n) = &mut d.settings.epub_split {
            rename(n);
        }
    } else {
        let from_parent = parent_of(st.character.iter().find(|x| x.name == from).and_then(|x| x.based_on.as_ref()));
        for s in &mut st.character {
            if s.based_on.as_deref() == Some(from) {
                s.based_on = if s.name == to { from_parent.clone() } else { Some(to.to_string()) };
            }
        }
        for s in &mut st.paragraph {
            rename_char_refs(&mut s.para, from, to);
        }
        rename(&mut st.default_character);
        rename(&mut d.footnote_options.ref_char_style);
    }
    // The table of contents' character styles ([None] is stored as no style).
    if !para && let Some(toc) = d.toc.as_mut() {
        for e in &mut toc.entries {
            for n in [&mut e.page_number_style, &mut e.separator_style] {
                if n.as_deref() == Some(from) {
                    *n = Some(to.to_string()).filter(|t| t != designcraft_doc::NO_CHAR_STYLE);
                }
            }
        }
    }
    for sid in d.stories.keys().copied().collect::<Vec<_>>() {
        let Some(story) = d.story_mut(sid) else { continue };
        story.for_each_text_mut(&mut |st| {
            if para {
                for f in st.paras.iter_mut().filter(|f| f.style == from) {
                    f.style = to.to_string();
                }
            } else {
                for r in st.chars.iter_mut().filter(|r| r.format.style == from) {
                    r.format.style = to.to_string();
                }
                for f in &mut st.paras {
                    rename_char_refs(&mut f.para, from, to);
                }
            }
            st.rev = st.rev.wrapping_add(1);
        });
    }
}

/// Point the nested, nested line and GREP styles, and the drop cap style, of paragraph
/// attributes at a renamed (or deleted, pointed at its replacement) character style.
fn rename_char_refs(p: &mut ParaAttrs, from: &str, to: &str) {
    let names = p.nested_styles.iter_mut().flatten().map(|n| &mut n.style);
    let names = names.chain(p.nested_line_styles.iter_mut().flatten().map(|n| &mut n.style));
    let names = names.chain(p.drop_cap_style.iter_mut());
    for n in names.chain(p.grep_styles.iter_mut().flatten().map(|g| &mut g.style)) {
        if n == from {
            *n = to.to_string();
        }
    }
}

pub(super) fn group_styles(s: &mut Session, p: &Value) -> Result<Value> {
    let para = match str_param(p, "kind").unwrap_or("paragraph") {
        "paragraph" => true,
        "character" => false,
        k => return Err(bad("style.group", format!("unknown kind `{k}`"))),
    };
    let names: Vec<String> =
        p.get("names").and_then(Value::as_array).map(|a| a.iter().filter_map(|v| v.as_str().map(str::to_string)).collect()).unwrap_or_default();
    let group = str_param(p, "group").unwrap_or("").trim_matches('/').to_string();
    s.edit(|d, _| {
        let mut renamed = serde_json::Map::new();
        for n in &names {
            if n.starts_with('[') {
                return Err(bad("style.group", format!("`{n}` is built in")));
            }
            let exists = if para { d.styles.para(n).is_some() } else { d.styles.character.iter().any(|c| c.name == *n) };
            if !exists {
                return Err(bad("style.group", format!("no style `{n}`")));
            }
            let base = n.rsplit('/').next().unwrap_or(n);
            let mut to = if group.is_empty() { base.to_string() } else { format!("{group}/{base}") };
            let taken = |d: &designcraft_doc::Document, x: &str| {
                if para { d.styles.para(x).is_some() } else { d.styles.character.iter().any(|c| c.name == x) }
            };
            if to != *n {
                to = Styles::unique_name(|x| taken(d, x), &to);
                rename_style(d, para, n, &to);
                renamed.insert(n.clone(), json!(to));
            }
        }
        Ok(json!({"renamed": renamed}))
    })
}

pub(super) fn delete_para(s: &mut Session, p: &Value) -> Result<Value> {
    delete_style(s, p, true)
}

pub(super) fn delete_char(s: &mut Session, p: &Value) -> Result<Value> {
    delete_style(s, p, false)
}

/// Delete a paragraph (`para`) or character style; everything that used it uses `replaceWith`
/// (default [Basic Paragraph] or [None]) instead, in one undo step.
fn delete_style(s: &mut Session, p: &Value, para: bool) -> Result<Value> {
    let (id, default) =
        if para { ("style.paragraph.delete", designcraft_doc::BASIC_PARAGRAPH) } else { ("style.character.delete", designcraft_doc::NO_CHAR_STYLE) };
    let name = str_param(p, "name").filter(|n| !n.is_empty()).ok_or_else(|| bad(id, "missing name"))?.to_string();
    let repl = str_param(p, "replaceWith").unwrap_or(default).to_string();
    if name.starts_with('[') {
        return Err(bad(id, "built-in styles can't be deleted"));
    }
    if repl == name {
        return Err(bad(id, "`replaceWith` can't be the deleted style"));
    }
    s.edit(|d, _| {
        let st = &d.styles;
        let exists = |n: &str| if para { st.para(n).is_some() } else { n == designcraft_doc::NO_CHAR_STYLE || st.char_style(n).is_some() };
        if !exists(&name) {
            return Err(bad(id, format!("no style `{name}`")));
        }
        if !exists(&repl) {
            return Err(bad(id, format!("no style `{repl}` to replace it with")));
        }
        // The deleted style's children are based on the replacement, or on the deleted style's
        // own parent when the replacement descends from it (so no based-on chain loops).
        let (own_parent, descends) = if para {
            (st.para(&name).and_then(|x| x.based_on.clone()), st.para_based_on_cycles(&name, &repl))
        } else {
            (st.char_style(&name).and_then(|x| x.based_on.clone()), st.char_based_on_cycles(&name, &repl))
        };
        let parent = if descends {
            own_parent.filter(|b| *b != name)
        } else if repl == designcraft_doc::NO_CHAR_STYLE || repl == designcraft_doc::NO_PARA_STYLE {
            None
        } else {
            Some(repl.clone())
        };
        let st = d.styles_mut();
        let based = if para {
            st.paragraph.retain(|x| x.name != name);
            st.paragraph.iter_mut().map(|x| &mut x.based_on).collect::<Vec<_>>()
        } else {
            st.character.retain(|x| x.name != name);
            st.character.iter_mut().map(|x| &mut x.based_on).collect()
        };
        for b in based.into_iter().filter(|b| b.as_deref() == Some(name.as_str())) {
            *b = parent.clone();
        }
        // The replacement keeps its own Export Tagging.
        st.export_tags.remove(&export_tag_key(para, &name));
        // Every other use: styles, stories, table cells, nested and GREP styles, notes, defaults.
        redirect_style_refs(d, para, &name, &repl);
        ok()
    })
}

pub(super) fn create_char(s: &mut Session, p: &Value) -> Result<Value> {
    let base = str_param(p, "name").unwrap_or("Character Style 1").to_string();
    let mut chars: CharAttrs = attrs("style.character.create", "chars", p, |a: &mut CharAttrs, k, v| a.set_json(k, v))?;
    // Validate caller-supplied fills; captured formatting may retain a deleted swatch name.
    crate::cmd::text::validate_char_fill(&s.doc()?.doc, &chars, "style.character.create")?;
    if p.get("fromSelection").and_then(Value::as_bool).unwrap_or(false)
        && let Some(target) = crate::cmd::text::format_targets_pub(s).first()
    {
        let st = s.doc()?;
        // Match paragraph fromSelection's first-character/caret policy, but keep paragraph
        // attributes inherited instead of freezing the fully resolved character properties.
        let format = crate::cmd::text::effective_char_format(s, target)?;
        let mut captured = st.doc.styles.char_style_attrs(&format.style);
        captured.merge(&format.over);
        // Replay the explicit fields so null clears a captured value rather than looking absent.
        if let Some(overrides) = p.get("chars").and_then(Value::as_object) {
            for (key, value) in overrides {
                captured.set_json(key, value).map_err(|e| bad("style.character.create", format!("chars: {e}")))?;
            }
        }
        chars = captured;
    }
    // Based on [None] is based on nothing.
    let based_on = style_name_param("style.character.create", p, "basedOn")?.filter(|b| b != designcraft_doc::NO_CHAR_STYLE);
    s.edit(|d, _| {
        if let Some(b) = &based_on
            && d.styles.char_style(b).is_none()
        {
            return Err(bad("style.character.create", format!("no character style `{b}`")));
        }
        let name = Styles::unique_name(|n| d.styles.char_style(n).is_some(), &base);
        d.styles_mut().character.push(CharacterStyle { name: name.clone(), based_on, chars, shortcut: String::new() });
        Ok(json!({"name": name}))
    })
}

pub(super) fn edit_char(s: &mut Session, p: &Value) -> Result<Value> {
    let name = str_param(p, "name").ok_or_else(|| bad("style.character.edit", "missing name"))?.to_string();
    if name == designcraft_doc::NO_CHAR_STYLE {
        return Err(bad("style.character.edit", "[None] can't be edited"));
    }
    let chars: CharAttrs = attrs("style.character.edit", "chars", p, |a: &mut CharAttrs, k, v| a.set_json(k, v))?;
    crate::cmd::text::validate_char_fill(&s.doc()?.doc, &chars, "style.character.edit")?;
    // `null` unsets an attribute (the style no longer sets it).
    let unset: Vec<String> = p
        .get("chars")
        .and_then(Value::as_object)
        .map(|o| o.iter().filter(|(_, v)| v.is_null()).map(|(k, _)| k.clone()).collect())
        .unwrap_or_default();
    let rename = str_param(p, "rename").map(str::trim).filter(|n| *n != name).map(str::to_string);
    let based: Option<Option<String>> = match p.get("basedOn") {
        None => None,
        Some(Value::Null) => Some(None),
        Some(Value::String(b)) if b == designcraft_doc::NO_CHAR_STYLE => Some(None),
        Some(Value::String(b)) => Some(Some(b.clone())),
        Some(_) => return Err(bad("style.character.edit", "`basedOn` is a style name or null")),
    };
    s.edit(|d, _| {
        if let Some(Some(b)) = &based {
            if d.styles.char_style(b).is_none() {
                return Err(bad("style.character.edit", format!("no character style `{b}`")));
            }
            if d.styles.char_based_on_cycles(&name, b) {
                return Err(bad("style.character.edit", "based-on would create a cycle"));
            }
        }
        if let Some(n) = &rename {
            if n.is_empty() || n.starts_with('[') {
                return Err(bad("style.character.edit", format!("can't rename to `{n}`")));
            }
            if d.styles.char_style(n).is_some() {
                return Err(bad("style.character.edit", format!("a character style named `{n}` already exists")));
            }
        }
        let st = d.styles_mut().char_style_mut(&name).ok_or_else(|| bad("style.character.edit", format!("no character style `{name}`")))?;
        st.chars.merge(&chars);
        for k in &unset {
            st.chars.set_json(k, &Value::Null).map_err(|e| bad("style.character.edit", e))?;
        }
        if let Some(b) = based.clone() {
            st.based_on = b;
        }
        if let Some(n) = &rename {
            // Every use: other styles, stories, table cells, nested and GREP styles, footnotes.
            rename_style(d, false, &name, n);
        }
        ok()
    })
}

/// #777: the params descriptions name the attributes, and a wrong one is the command's error.
#[cfg(test)]
mod attribute_error_tests {
    use super::*;
    use crate::cmd::find_command;

    #[test]
    fn a_wrong_attribute_names_the_command_and_the_right_attribute() {
        let mut s = Session::new();
        s.execute("file.new", &json!({})).unwrap();
        let err = |s: &mut Session, id: &str, p: Value| s.execute(id, &p).unwrap_err().to_string();
        assert_eq!(
            err(&mut s, "style.paragraph.create", json!({"name": "H", "chars": {"font": "Source Sans 3"}})),
            "invalid parameters for `style.paragraph.create`: chars: unknown attribute `font`; did you mean `fontFamily`?"
        );
        assert_eq!(
            err(&mut s, "style.paragraph.create", json!({"name": "H", "para": {"spaceBefor": 6}})),
            "invalid parameters for `style.paragraph.create`: para: unknown attribute `spaceBefor`; did you mean `spaceBefore`?"
        );
        assert!(
            err(&mut s, "style.character.create", json!({"name": "C", "chars": {"font": "X"}}))
                .starts_with("invalid parameters for `style.character.create`: chars:")
        );
        assert!(err(&mut s, "style.paragraph.create", json!({"name": "H", "chars": "bold"})).contains("chars must be an object of attributes"));
        s.execute("style.paragraph.create", &json!({"name": "H", "chars": {"fontFamily": "Source Sans 3", "size": 14}})).unwrap();
        assert!(
            err(&mut s, "style.paragraph.edit", json!({"name": "H", "chars": {"font": "X"}}))
                .starts_with("invalid parameters for `style.paragraph.edit`: chars:")
        );
        s.execute("style.character.create", &json!({"name": "C", "chars": {"fontStyle": "Bold"}})).unwrap();
        assert!(
            err(&mut s, "style.character.edit", json!({"name": "C", "chars": {"font": "X"}}))
                .starts_with("invalid parameters for `style.character.edit`: chars:")
        );
        for id in ["style.paragraph.create", "style.paragraph.edit", "style.character.create", "style.character.edit"] {
            let doc = find_command(id).unwrap().params;
            assert!(doc.contains("as type.char names them: fontFamily"), "{id}: {doc}");
        }
        assert!(find_command("style.paragraph.create").unwrap().params.contains("as type.para names them: align"));
    }
}

#[cfg(test)]
mod export_tag_tests {
    use serde_json::json;

    use crate::Session;

    #[test]
    fn export_tagging_shapes_epub_markup() {
        let mut s = Session::new();
        s.execute("file.new", &json!({})).unwrap();
        s.execute("style.paragraph.create", &json!({"name": "Chapter Title", "chars": {"size": 24}})).unwrap();
        s.execute("style.character.create", &json!({"name": "Key Term", "chars": {"fontStyle": "Bold"}})).unwrap();
        s.execute("style.exportTag", &json!({"style": "Chapter Title", "tag": "h1", "class": "chapter"})).unwrap();
        s.execute("style.exportTag", &json!({"style": "Key Term", "character": true, "tag": "strong"})).unwrap();
        assert!(s.execute("style.exportTag", &json!({"style": "Nope"})).is_err());
        // Split Document is for paragraph styles; it alone keeps the tagging.
        assert!(s.execute("style.exportTag", &json!({"style": "Key Term", "character": true, "split": true})).is_err());
        s.execute("style.paragraph.create", &json!({"name": "Part"})).unwrap();
        assert_eq!(s.execute("style.exportTag", &json!({"style": "Part", "split": true})).unwrap()["split"], true);
        assert!(s.doc().unwrap().doc.styles.export_tag("Part", false).is_some_and(|t| t.split));
        s.execute("style.exportTag", &json!({"style": "Part", "split": false})).unwrap();
        assert!(s.doc().unwrap().doc.styles.export_tag("Part", false).is_none(), "an untouched tagging goes");
        let r = s.execute("frame.create", &json!({"rect": [72, 72, 400, 300], "content": "text", "text": "Origins\nA word"})).unwrap();
        let sid = r["story"].as_u64().unwrap();
        s.execute("text.select", &json!({"story": sid, "anchor": 0, "focus": 0})).unwrap();
        s.execute("style.paragraph.apply", &json!({"name": "Chapter Title"})).unwrap();
        s.execute("text.select", &json!({"story": sid, "anchor": 10, "focus": 14})).unwrap();
        s.execute("style.character.apply", &json!({"name": "Key Term"})).unwrap();
        let html = s.execute("file.exportHtml", &json!({})).unwrap()["text"].as_str().unwrap().to_string();
        assert!(html.contains("<h1 class=\"chapter\">Origins</h1>"), "{html}");
        assert!(html.contains("<strong class=\"key-term\">word</strong>"), "{html}");
        // Tagged PDF: a heading element and a paragraph element.
        let r = s.execute("file.exportPdf", &json!({"tagged": true, "compressImages": false})).unwrap();
        let pdf = crate::cmd::file::base64_decode(r["base64"].as_str().unwrap());
        let text = String::from_utf8_lossy(&pdf);
        assert!(text.contains("/S/H1") && text.contains("/S/P"), "structure elements");
        assert!(designcraft_render::pdf_page_count(&pdf) == Some(1));
    }

    #[test]
    fn export_tagging_follows_a_renamed_style_and_goes_with_a_deleted_one() {
        let mut s = Session::new();
        s.execute("file.new", &json!({})).unwrap();
        s.execute("style.paragraph.create", &json!({"name": "Head"})).unwrap();
        s.execute("style.paragraph.create", &json!({"name": "Body"})).unwrap();
        s.execute("style.character.create", &json!({"name": "Term"})).unwrap();
        s.execute("style.exportTag", &json!({"style": "Head", "tag": "h2", "class": "head"})).unwrap();
        s.execute("style.exportTag", &json!({"style": "Body", "tag": "p", "class": "body"})).unwrap();
        s.execute("style.exportTag", &json!({"style": "Term", "character": true, "tag": "em", "class": "term"})).unwrap();
        let tag = |s: &Session, name: &str, character: bool| {
            s.doc().unwrap().doc.styles.export_tag(name, character).map(|t| (t.tag.clone(), t.class.clone()))
        };
        let pair = |t: &str, c: &str| Some((t.to_string(), c.to_string()));
        // A rename onto another style's name is refused, so neither tagging is lost.
        assert!(s.execute("style.paragraph.edit", &json!({"name": "Head", "rename": "Body"})).is_err());
        assert_eq!((tag(&s, "Head", false), tag(&s, "Body", false)), (pair("h2", "head"), pair("p", "body")));

        s.execute("style.paragraph.edit", &json!({"name": "Head", "rename": "Title"})).unwrap();
        s.execute("style.character.edit", &json!({"name": "Term", "rename": "Word"})).unwrap();
        assert_eq!(tag(&s, "Title", false), pair("h2", "head"));
        assert_eq!(tag(&s, "Head", false), None);
        assert_eq!(tag(&s, "Word", true), pair("em", "term"));
        assert_eq!(tag(&s, "Term", true), None);
        // Moving a style into a group renames it to Group/Name.
        s.execute("style.group", &json!({"kind": "paragraph", "names": ["Title"], "group": "Heads"})).unwrap();
        assert_eq!(tag(&s, "Heads/Title", false), pair("h2", "head"));
        assert_eq!(tag(&s, "Title", false), None);

        for _ in 0..3 {
            s.execute("edit.undo", &json!({})).unwrap();
        }
        assert_eq!(tag(&s, "Head", false), pair("h2", "head"));
        assert_eq!(tag(&s, "Title", false), None);
        assert_eq!(tag(&s, "Term", true), pair("em", "term"));
        assert_eq!(tag(&s, "Word", true), None);

        // Deleting drops the deleted style's tagging; the replacement keeps its own.
        s.execute("style.paragraph.delete", &json!({"name": "Head", "replaceWith": "Body"})).unwrap();
        assert_eq!(tag(&s, "Head", false), None);
        assert_eq!(tag(&s, "Body", false), pair("p", "body"));
        s.execute("style.character.delete", &json!({"name": "Term"})).unwrap();
        assert_eq!(tag(&s, "Term", true), None);
        assert!(s.doc().unwrap().doc.styles.export_tags.keys().all(|k| k == "p:Body"));
    }

    #[test]
    fn a_rename_onto_a_taken_name_says_so_and_a_trimmed_rename_drops_a_stale_tag() {
        let mut s = Session::new();
        s.execute("file.new", &json!({})).unwrap();
        for (kind, a, b) in [("paragraph", "Head", "Body"), ("character", "Term", "Word")] {
            s.execute(&format!("style.{kind}.create"), &json!({"name": a})).unwrap();
            s.execute(&format!("style.{kind}.create"), &json!({"name": b})).unwrap();
            let e = s.execute(&format!("style.{kind}.edit"), &json!({"name": a, "rename": b})).unwrap_err().to_string();
            assert!(e.contains(&format!("a {kind} style named `{b}` already exists")), "{e}");
            let e = s.execute(&format!("style.{kind}.edit"), &json!({"name": a, "rename": "[None]"})).unwrap_err().to_string();
            assert!(e.contains("can't rename to `[None]`"), "{e}");
        }
        // An entry left under a name no style has doesn't attach to a style renamed onto it.
        s.edit(|d, _| {
            d.styles_mut()
                .export_tags
                .insert("p:Title".into(), designcraft_doc::ExportTag { tag: "h6".into(), class: "stale".into(), ..Default::default() });
            Ok(())
        })
        .unwrap();
        s.execute("style.paragraph.edit", &json!({"name": "Head", "rename": "  Title "})).unwrap();
        let st = &s.doc().unwrap().doc.styles;
        assert!(st.para("Title").is_some(), "the new name is trimmed");
        assert_eq!(st.export_tag("Title", false), None);
    }
}

#[cfg(test)]
mod cjk_tests {
    use super::*;
    #[test]
    fn cjk_composite_font_command_invalidates_layout_and_undo_restores_it() {
        let mut s = Session::new();
        s.execute("file.new", &json!({})).unwrap();
        let r = s.execute("frame.create", &json!({"rect": [0, 0, 300, 150], "content": "text", "text": "AB"})).unwrap();
        let sid = designcraft_doc::StoryId(r["story"].as_u64().unwrap());
        let definition = |scale| json!({"name": "Mixed", "entries": [{"family": "Source Serif 4", "relativeSize": scale}]});
        s.execute("style.compositeFont.set", &definition(1.0)).unwrap();
        s.execute("text.select", &json!({"story": sid.0, "anchor": 0, "focus": 2})).unwrap();
        s.execute("type.char", &json!({"fontFamily": "Mixed", "leadingAki": 0.25, "jidori": 4})).unwrap();
        let old = s.cache.get(&s.doc().unwrap().doc, sid, None);
        s.execute("style.compositeFont.set", &definition(2.0)).unwrap();
        let new = s.cache.get(&s.doc().unwrap().doc, sid, None);
        assert!(!std::sync::Arc::ptr_eq(&old, &new));
        assert!(new.frames[0].lines[0].end_x > old.frames[0].lines[0].end_x);
        s.execute("edit.undo", &json!({})).unwrap();
        let restored = s.cache.get(&s.doc().unwrap().doc, sid, None);
        assert_eq!(old.frames[0].lines[0].end_x, restored.frames[0].lines[0].end_x);
        assert!(s.execute("style.compositeFont.set", &definition(-1.0)).is_err());
    }
}

#[cfg(test)]
mod delete_tests {
    use designcraft_doc::{CellStyle, ObjectStyle, StoryId, Toc, TocEntry};
    use serde_json::json;

    use crate::Session;

    /// Is `name` still named anywhere in the document?
    fn named(s: &Session, name: &str) -> bool {
        serde_json::to_string(&*s.doc().unwrap().doc).unwrap().contains(&format!("\"{name}\""))
    }

    fn char_doc() -> (Session, u64) {
        let mut s = Session::new();
        s.execute("file.new", &json!({})).unwrap();
        s.execute("style.character.create", &json!({"name": "Base", "chars": {"fontStyle": "Bold"}})).unwrap();
        s.execute("style.character.create", &json!({"name": "Emphasis", "basedOn": "Base", "chars": {"tracking": 20}})).unwrap();
        s.execute("style.character.create", &json!({"name": "Child", "basedOn": "Emphasis"})).unwrap();
        s.execute("style.character.create", &json!({"name": "Strong", "chars": {"size": 14}})).unwrap();
        let refs = json!({
            "nestedStyles": [{"style": "Emphasis", "through": true, "count": 1, "until": {"kind": "words"}}],
            "grepStyles": [{"style": "Emphasis", "pattern": "\\d+"}],
            "nestedLineStyles": [{"style": "Emphasis", "lines": 1}],
            "dropCapLines": 2, "dropCapChars": 1, "dropCapStyle": "Emphasis"});
        s.execute("style.paragraph.create", &json!({"name": "Lead", "para": refs})).unwrap();
        s.execute("footnote.options", &json!({"refCharStyle": "Emphasis"})).unwrap();
        let sid = s.execute("frame.create", &json!({"rect": [72, 72, 300, 200], "content": "text", "text": "Hi there"})).unwrap()["story"]
            .as_u64()
            .unwrap();
        s.execute("text.select", &json!({"story": sid, "anchor": 0, "focus": 2})).unwrap();
        s.execute("style.character.apply", &json!({"name": "Emphasis"})).unwrap();
        // Local paragraph overrides name it too.
        s.execute("type.para", &json!({"attrs": refs})).unwrap();
        s.edit(|d, _| {
            d.styles_mut().default_character = "Emphasis".into();
            Ok(())
        })
        .unwrap();
        assert!(named(&s, "Emphasis"));
        (s, sid)
    }

    #[test]
    fn deleting_a_character_style_points_every_use_at_the_replacement() {
        let (mut s, sid) = char_doc();
        let undo = s.doc().unwrap().history.undo.len();
        s.execute("style.character.delete", &json!({"name": "Emphasis", "replaceWith": "Strong"})).unwrap();
        assert!(!named(&s, "Emphasis"), "no dangling reference");
        let d = s.doc().unwrap().doc.clone();
        assert_eq!(d.styles.char_style("Child").unwrap().based_on.as_deref(), Some("Strong"));
        assert_eq!(d.story(StoryId(sid)).unwrap().chars[0].format.style, "Strong");
        let lead = &d.styles.para("Lead").unwrap().para;
        assert_eq!(lead.nested_styles.as_ref().unwrap()[0].style, "Strong");
        assert_eq!(lead.grep_styles.as_ref().unwrap()[0].style, "Strong");
        assert_eq!(lead.nested_line_styles.as_ref().unwrap()[0].style, "Strong");
        assert_eq!(lead.drop_cap_style.as_deref(), Some("Strong"));
        assert_eq!(d.footnote_options.ref_char_style, "Strong");
        assert_eq!(d.styles.default_character, "Strong");
        // One undo step brings the style and every use back.
        assert_eq!(s.doc().unwrap().history.undo.len(), undo + 1);
        s.execute("edit.undo", &json!({})).unwrap();
        assert!(s.doc().unwrap().doc.styles.char_style("Emphasis").is_some());
        assert_eq!(s.doc().unwrap().doc.story(StoryId(sid)).unwrap().chars[0].format.style, "Emphasis");
    }

    #[test]
    fn deleting_a_character_style_for_none_unsets_based_on() {
        let (mut s, _) = char_doc();
        s.execute("style.character.delete", &json!({"name": "Emphasis"})).unwrap();
        assert!(!named(&s, "Emphasis"), "no dangling reference");
        let d = s.doc().unwrap().doc.clone();
        assert_eq!(d.styles.char_style("Child").unwrap().based_on, None);
        assert_eq!(d.footnote_options.ref_char_style, designcraft_doc::NO_CHAR_STYLE);
    }

    /// Replacing a style with its own descendant rebases the deleted style's children on its
    /// parent, so no based-on chain becomes a cycle.
    #[test]
    fn replacing_a_style_with_its_child_makes_no_cycle() {
        let (mut s, _) = char_doc();
        s.execute("style.character.create", &json!({"name": "Grandchild", "basedOn": "Child"})).unwrap();
        s.execute("style.character.delete", &json!({"name": "Emphasis", "replaceWith": "Grandchild"})).unwrap();
        let d = s.doc().unwrap().doc.clone();
        assert_eq!(d.styles.char_style("Child").unwrap().based_on.as_deref(), Some("Base"));
        assert_eq!(d.styles.char_style("Grandchild").unwrap().based_on.as_deref(), Some("Child"));
        assert!(!named(&s, "Emphasis"));
    }

    #[test]
    fn bad_style_deletes_are_errors_and_change_nothing() {
        let (mut s, _) = char_doc();
        s.execute("style.paragraph.create", &json!({"name": "Head"})).unwrap();
        let before = s.doc().unwrap().doc.clone();
        let undo = s.doc().unwrap().history.undo.len();
        for (id, p) in [
            ("style.character.delete", json!({})),
            ("style.character.delete", json!({"name": ""})),
            ("style.character.delete", json!({"name": "Nope"})),
            ("style.character.delete", json!({"name": "[None]"})),
            ("style.character.delete", json!({"name": "Emphasis", "replaceWith": "Nope"})),
            ("style.character.delete", json!({"name": "Emphasis", "replaceWith": "Emphasis"})),
            ("style.paragraph.delete", json!({})),
            ("style.paragraph.delete", json!({"name": ""})),
            ("style.paragraph.delete", json!({"name": "Nope"})),
            ("style.paragraph.delete", json!({"name": "[Basic Paragraph]"})),
            ("style.paragraph.delete", json!({"name": "Head", "replaceWith": "Nope"})),
            ("style.paragraph.delete", json!({"name": "Head", "replaceWith": "Head"})),
        ] {
            assert!(s.execute(id, &p).is_err(), "{id} {p}");
        }
        assert_eq!(*s.doc().unwrap().doc, *before);
        assert_eq!(s.doc().unwrap().history.undo.len(), undo);
    }

    /// Deleting a paragraph style also redirects TOC style references to its replacement.
    #[test]
    fn deleting_a_paragraph_style_points_every_use_at_the_replacement() {
        let mut s = Session::new();
        s.execute("file.newSample", &json!({})).unwrap();
        s.execute("style.paragraph.create", &json!({"name": "Sub", "basedOn": "Table Body", "nextStyle": "Table Body"})).unwrap();
        s.edit(|d, _| {
            let story = *d.stories.keys().next().ok_or(crate::EngineError::NoDocument)?;
            let st = d.styles_mut();
            st.default_paragraph = "Table Body".into();
            st.object.push(ObjectStyle { name: "Boxed".into(), paragraph_style: Some("Table Body".into()), ..Default::default() });
            st.cell.push(CellStyle { name: "Body Cell".into(), paragraph_style: Some("Table Body".into()), ..Default::default() });
            d.footnote_options.para_style = "Table Body".into();
            d.endnote_options.para_style = "Table Body".into();
            d.endnote_options.heading_style = "Table Body".into();
            d.settings.epub_split = designcraft_doc::EpubSplit::Style("Table Body".into());
            d.toc = Some(Toc {
                story,
                title: "Contents".into(),
                entries: vec![TocEntry { entry_style: Some("Table Body".into()), ..TocEntry::new("Table Body", 1) }],
                page_numbers: false,
                title_style: Some("Table Body".into()),
                create_bookmarks: false,
            });
            Ok(())
        })
        .unwrap();
        assert!(named(&s, "Table Body"));
        s.execute("style.paragraph.delete", &json!({"name": "Table Body", "replaceWith": "Body"})).unwrap();
        assert!(!named(&s, "Table Body"), "no dangling reference");
        let d = s.doc().unwrap().doc.clone();
        let sub = d.styles.para("Sub").unwrap();
        assert_eq!((sub.based_on.as_deref(), sub.next_style.as_deref()), (Some("Body"), Some("Body")));
        assert_eq!(d.styles.default_paragraph, "Body");
        assert_eq!(d.endnote_options.heading_style, "Body");
        assert_eq!(d.settings.epub_split, designcraft_doc::EpubSplit::Style("Body".into()), "EPUB Split Document follows the replacement");
        let after = s.execute("preflight.run", &json!({})).unwrap();
        assert!(!after.to_string().contains("Table Body"), "{after}");
    }
}

#[cfg(test)]
mod grep_style_tests {
    use serde_json::json;

    use crate::Session;

    #[test]
    fn grep_style_pattern_that_cannot_compile_is_rejected() {
        let mut s = Session::new();
        s.execute("file.new", &json!({})).unwrap();
        s.execute("style.character.create", &json!({"name": "Keep"})).unwrap();
        let bad = json!([{"style": "Keep", "pattern": "(\\d+"}]);
        let e = s.execute("style.paragraph.create", &json!({"name": "Broken", "para": {"grepStyles": bad}})).unwrap_err().to_string();
        assert!(e.contains("bad GREP") && e.contains("(\\\\d+"), "{e}");
        assert!(s.doc().unwrap().doc.styles.para("Broken").is_none());

        // The lookbehind dialect compiles.
        let good = json!([{"style": "Keep", "pattern": "(?<=\\s)[aiouwzAIOUWZ]\\s"}]);
        s.execute("style.paragraph.create", &json!({"name": "Polish", "para": {"grepStyles": good}})).unwrap();
        let e = s.execute("style.paragraph.edit", &json!({"name": "Polish", "para": {"grepStyles": bad, "spaceAfter": 6}})).unwrap_err().to_string();
        assert!(e.contains("bad GREP"), "{e}");
        let st = s.doc().unwrap().doc.styles.para("Polish").unwrap().clone();
        assert_eq!(st.para.grep_styles.unwrap()[0].pattern, "(?<=\\s)[aiouwzAIOUWZ]\\s");
        assert_eq!(st.para.space_after, None, "nothing changes");

        // A paragraph override too.
        let r = s.execute("frame.create", &json!({"rect": [72, 72, 300, 200], "content": "text", "text": "Ala i kot"})).unwrap();
        s.execute("text.select", &json!({"story": r["story"], "anchor": 0, "focus": 0})).unwrap();
        let e = s.execute("type.para", &json!({"grepStyles": bad})).unwrap_err().to_string();
        assert!(e.contains("bad GREP"), "{e}");
    }

    #[test]
    fn idml_import_keeps_a_broken_grep_style_and_warns() {
        let mut s = Session::new();
        s.execute("file.new", &json!({})).unwrap();
        s.execute("style.character.create", &json!({"name": "Keep"})).unwrap();
        s.execute("style.paragraph.create", &json!({"name": "Broken"})).unwrap();
        // Written past the command, as another application could.
        s.edit(|d, _| {
            if let Some(st) = d.styles_mut().para_mut("Broken") {
                st.para.grep_styles = Some(vec![designcraft_doc::GrepStyle { style: "Keep".into(), pattern: "[a-".into() }]);
            }
            Ok(())
        })
        .unwrap();
        let b64 = s.execute("file.exportIdml", &json!({})).unwrap()["base64"].as_str().unwrap().to_string();
        let r = s.execute("file.openIdml", &json!({"base64": b64})).unwrap();
        let warnings = r["warnings"].to_string();
        assert!(warnings.contains("Broken") && warnings.contains("[a-") && warnings.contains("bad GREP"), "{warnings}");
        let st = s.doc().unwrap().doc.styles.para("Broken").unwrap().clone();
        assert_eq!(st.para.grep_styles.unwrap()[0].pattern, "[a-", "the style opens with its pattern");
    }
}
