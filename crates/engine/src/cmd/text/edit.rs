use super::caret::{next_char, next_word, prev_char, prev_word, story_of};
use super::format::{apply_typing_format, format_targets, typing_format};
use super::{Result, Session, TextSel, Value, bad, bool_or, compose, floor_char_boundary, id_param, json, ok, str_param};
#[cfg(test)]
use designcraft_doc::StoryId;

// Each command edits a copy-on-write Story snapshot. Reserving its known final text growth
// avoids retaining geometric String spare capacity in every independent undo snapshot.
// General Story insertion keeps its existing amortized-growth policy for mutable builders.
fn reserve_command_text(st: &mut designcraft_doc::Story, additional: usize) -> Result<()> {
    st.text.try_reserve_exact(additional).map_err(|e| crate::EngineError::Other(format!("could not reserve text insertion: {e}")))
}

pub(super) fn insert(s: &mut Session, p: &Value) -> Result<Value> {
    let text = crate::cmd::text_param(p, "text");
    // Tab in a table cell moves to the next cell (Shift-Tab: `table.prevCell`). Footnote text is
    // addressed as a cell too: a Tab there is typed.
    if text == "\t" && s.doc()?.selection.text.is_some_and(|t| t.cell.is_some_and(|c| c.footnote_id().is_none())) {
        return crate::cmd::table::step_cell(s, true);
    }
    // Typing with one cell selected replaces its text, as InDesign.
    crate::cmd::table::one_selected_cell_to_text(s)?;
    let raw = p.get("raw").and_then(Value::as_bool).unwrap_or(false);
    let text = if s.prefs.typographers_quotes && !raw { smart_quotes(s, &text) } else { text };
    let tracking = s.doc()?.doc.settings.track_changes;
    let autocorrect = (s.prefs.autocorrect && !tracking && !raw).then(|| s.prefs.autocorrect_list.clone());
    let typing = typing_format(s).cloned();
    let result = s.edit(|d, sel| {
        let t = sel.text.ok_or_else(|| bad("text.insert", "no insertion point"))?;
        let st = d.text_story_mut(t.story, t.cell).ok_or(designcraft_doc::DocError::NoStory(t.story))?;
        let r = t.range();
        let mut r = r.start.min(st.len())..r.end.min(st.len());
        // Between a break character and its return is the end of that paragraph: typing there goes
        // to the start of the next one.
        if r.is_empty()
            && st.text.get(r.start..).is_some_and(|t| t.starts_with('\n'))
            && st.text.get(..r.start).and_then(|t| t.chars().next_back()).is_some_and(designcraft_doc::is_break_char)
        {
            r = r.start + 1..r.start + 1;
        }
        // Autocorrect: a word ended by a space or punctuation is looked up.
        if let Some(list) = &autocorrect
            && r.is_empty()
            && text.chars().count() == 1
            && text.chars().all(|c| c.is_whitespace() || c.is_ascii_punctuation())
        {
            let before = &st.text[..r.start];
            let ws = before.char_indices().rev().take_while(|(_, c)| c.is_alphabetic()).last().map(|(i, _)| i);
            if let Some(ws) = ws {
                let word = &before[ws..];
                if let Some((_, to)) = list.iter().find(|(from, _)| from.eq_ignore_ascii_case(word)) {
                    let to = if word.chars().next().is_some_and(char::is_uppercase) {
                        let mut c = to.chars();
                        c.next().map(|f| f.to_uppercase().chain(c).collect()).unwrap_or_default()
                    } else {
                        to.clone()
                    };
                    st.replace(ws..r.start, &to);
                    r = ws + to.len()..ws + to.len();
                }
            }
        }
        // A column, frame or page break ends its paragraph.
        let mut text = if text.contains(designcraft_doc::is_break_char) {
            designcraft_doc::end_paragraphs_at_breaks(&text, st.text.get(r.end..).and_then(|t| t.chars().next()))
        } else {
            text.clone()
        };
        // Typing next to a table anchor starts a paragraph of its own.
        let mut trail = 0;
        if t.cell.is_none() && !text.is_empty() && st.table_at(r.start).is_some() {
            let pi = st.para_at(r.start);
            let pr = st.para_ranges()[pi].clone();
            let anchor_at = pr.start + st.text[pr.clone()].find(designcraft_doc::TABLE_ANCHOR).unwrap_or(0);
            if r.start <= anchor_at {
                if !text.ends_with('\n') {
                    text.push('\n');
                    trail = 1;
                }
            } else if !text.starts_with('\n') {
                text.insert(0, '\n');
            }
        }
        if tracking && t.cell.is_none() {
            // Track Changes: the replaced text is marked deleted, the typing inserted after it.
            let at = crate::cmd::changes::mark_deleted(st, r.clone());
            reserve_command_text(st, text.len())?;
            let mut fmt = st.insert_format_at(at).clone();
            if let Some(f) = &typing {
                apply_typing_format(&mut fmt, f);
            }
            fmt.over.change = Some(designcraft_doc::ChangeMark::Inserted);
            st.insert_with(at, &text, fmt);
            let pos = at + text.len() - trail;
            sel.text = Some(TextSel { anchor: pos, focus: pos, ..t });
            return Ok(json!({"pos": pos}));
        }
        let a = floor_char_boundary(&st.text, r.start.min(st.len()));
        let b = floor_char_boundary(&st.text, r.end.min(st.len())).max(a);
        reserve_command_text(st, text.len().saturating_sub(b - a))?;
        st.replace(r.clone(), &text);
        if let Some(f) = &typing
            && !text.is_empty()
        {
            st.format_chars(r.start..r.start + text.len(), |fmt| apply_typing_format(fmt, f));
        }
        let pos = r.start + text.len() - trail;
        sel.text = Some(TextSel { anchor: pos, focus: pos, ..t });
        Ok(json!({"pos": pos}))
    });
    // A preview (IME marked text) leaves the typing format to the text typed in the end.
    if result.is_ok() && s.active().is_none_or(|d| d.interaction.is_none()) {
        s.typing_format = None;
    }
    result
}

/// The input method's delete-surrounding: remove `before`/`after` characters (or bytes) around
/// the selection, keeping the selection itself and its direction, then optionally type `commit`
/// over it. One undo step for the whole batch, through the normal command pipeline (preview
/// restoration, panic guard, undo, auto-size and the journal), unlike a private edit.
pub(super) fn delete_surrounding(s: &mut Session, p: &Value) -> Result<Value> {
    let count = |key: &str| -> Result<usize> {
        match p.get(key).and_then(Value::as_u64) {
            Some(v) => usize::try_from(v).map_err(|_| bad("text.deleteSurrounding", format!("{key} is out of range"))),
            None => Ok(0),
        }
    };
    let before = count("before")?;
    let after = count("after")?;
    let bytes = p.get("bytes").and_then(Value::as_bool).unwrap_or(false);
    let commit = designcraft_tools::ime_commit_text(p.get("commit").and_then(Value::as_str).unwrap_or(""));
    // What the request removes, from the committed text as it is now: the ranges exclude the
    // selection and any pre-edit (a pre-edit is only a preview, never in the story).
    let (before_range, after_range, tracking, length_before) = {
        let st = s.doc()?;
        let t = st.selection.text.ok_or_else(|| bad("text.deleteSurrounding", "no text insertion point"))?;
        let story = st.doc.text_story(t.story, t.cell).ok_or(designcraft_doc::DocError::NoStory(t.story))?;
        let (before_range, after_range) = crate::ime_delete_ranges(&story.text, t.anchor, t.focus, before, after, bytes)
            .ok_or_else(|| bad("text.deleteSurrounding", "the IME text selection is not on character boundaries"))?;
        (before_range, after_range, st.doc.settings.track_changes && t.cell.is_none(), story.text.len())
    };
    s.edit(|d, sel| {
        let t = sel.text.ok_or_else(|| bad("text.deleteSurrounding", "no text insertion point"))?;
        let story = d.text_story_mut(t.story, t.cell).ok_or(designcraft_doc::DocError::NoStory(t.story))?;
        if !after_range.is_empty() {
            if tracking {
                crate::cmd::changes::mark_deleted(story, after_range.clone());
            } else {
                story.delete(after_range.clone());
            }
        }
        let mut shift = 0usize;
        if !before_range.is_empty() {
            if tracking {
                // Tracked changes keep the text and mark it deleted: the selection does not move.
                let new_end = crate::cmd::changes::mark_deleted(story, before_range.clone());
                shift = before_range.end.saturating_sub(new_end);
            } else {
                shift = before_range.len();
                story.delete(before_range.clone());
            }
        }
        sel.text = Some(TextSel { anchor: t.anchor.saturating_sub(shift), focus: t.focus.saturating_sub(shift), ..t });
        Ok(())
    })?;
    if !commit.is_empty() {
        insert(s, &json!({"text": commit}))?;
    }
    let st = s.doc()?;
    let (anchor, focus) = st.selection.text.map_or((0, 0), |t| (t.anchor, t.focus));
    let length_after = st.selection.text.and_then(|t| st.doc.text_story(t.story, t.cell)).map_or(0, |story| story.text.len());
    Ok(json!({
        "deletedBefore": [before_range.start, before_range.end],
        "deletedAfter": [after_range.start, after_range.end],
        "lengthBefore": length_before,
        "lengthAfter": length_after,
        "anchor": anchor,
        "focus": focus,
    }))
}

fn smart_quotes(s: &Session, text: &str) -> String {
    if !text.contains(['"', '\'']) {
        return text.to_string();
    }
    let prev = s
        .active()
        .and_then(|st| {
            let t = st.selection.text?;
            let story = st.doc.text_story(t.story, t.cell)?;
            story.text[..t.range().start.min(story.len())].chars().last()
        })
        .unwrap_or(' ');
    // The language at the insertion point picks the quote marks.
    let language = s
        .active()
        .and_then(|st| {
            let t = st.selection.text?;
            let story = st.doc.text_story(t.story, t.cell)?;
            let pos = t.range().start.min(story.len());
            let pi = story.para_at(pos).min(story.paras.len().saturating_sub(1));
            let base = st.doc.styles.resolve_para(story.paras.get(pi)?).1;
            Some(st.doc.styles.resolve_char(&base, story.char_format_at(pos)).language)
        })
        .unwrap_or_default();
    let [dq_open, dq_close, sq_open, sq_close] = quote_marks(&language);
    let mut out = String::with_capacity(text.len());
    let mut last = prev;
    for c in text.chars() {
        let open = last.is_whitespace() || matches!(last, '(' | '[' | '{' | '\u{2014}' | '\u{2013}');
        let r = match c {
            '"' => {
                if open {
                    dq_open
                } else {
                    dq_close
                }
            }
            '\'' => {
                if open {
                    sq_open
                } else {
                    // An apostrophe inside a word stays an apostrophe.
                    if last.is_alphanumeric() { '\u{2019}' } else { sq_close }
                }
            }
            c => c,
        };
        out.push(r);
        last = c;
    }
    out
}

/// Typographer's quotes of a language (a name or locale code, see
/// [`designcraft_doc::language_tag`]): [double open, double close, single open, single close].
pub fn quote_marks(language: &str) -> [char; 4] {
    const ENGLISH: [char; 4] = ['\u{201C}', '\u{201D}', '\u{2018}', '\u{2019}'];
    let Some(tag) = designcraft_doc::language_tag(language) else { return ENGLISH };
    // Swiss German, French and Italian use guillemets.
    if tag.ends_with("-CH") {
        return ['\u{00AB}', '\u{00BB}', '\u{2039}', '\u{203A}'];
    }
    match tag {
        "zh-Hans" => return ENGLISH,
        "zh" | "zh-Hant" => return ['\u{300C}', '\u{300D}', '\u{300E}', '\u{300F}'],
        _ => {}
    }
    match designcraft_doc::language_subtag(tag) {
        "de" | "cs" | "sk" | "bg" | "lt" => ['\u{201E}', '\u{201C}', '\u{201A}', '\u{2018}'],
        "fr" | "ru" | "uk" | "no" | "nb" | "nn" | "el" => ['\u{00AB}', '\u{00BB}', '\u{2039}', '\u{203A}'],
        "es" | "it" | "pt" | "ca" => ['\u{00AB}', '\u{00BB}', '\u{201C}', '\u{201D}'],
        "nl" | "pl" | "ro" | "hu" | "hr" => ['\u{201E}', '\u{201D}', '\u{201A}', '\u{2019}'],
        "sv" | "fi" => ['\u{201D}', '\u{201D}', '\u{2019}', '\u{2019}'],
        "da" => ['\u{00BB}', '\u{00AB}', '\u{203A}', '\u{2039}'],
        "ja" => ['\u{300C}', '\u{300D}', '\u{300E}', '\u{300F}'],
        _ => ENGLISH,
    }
}

pub(crate) fn delete_selection(s: &mut Session) -> Result<Value> {
    let tracking = s.doc()?.doc.settings.track_changes;
    s.edit(|d, sel| {
        let t = sel.text.ok_or_else(|| bad("text.delete", "no text"))?;
        let r = t.range();
        if let Some(st) = d.text_story_mut(t.story, t.cell) {
            if tracking && t.cell.is_none() {
                crate::cmd::changes::mark_deleted(st, r.clone());
            } else {
                st.delete(r.clone());
            }
        }
        sel.text = Some(TextSel { anchor: r.start, focus: r.start, ..t });
        ok()
    })
}

pub(super) fn delete(s: &mut Session, p: &Value) -> Result<Value> {
    let forward = bool_or(p, "forward", false);
    let word = bool_or(p, "word", false);
    let tracking = s.doc()?.doc.settings.track_changes;
    s.edit(|d, sel| {
        let t = sel.text.ok_or_else(|| bad("text.delete", "no text"))?;
        let st = d.text_story_mut(t.story, t.cell).ok_or(designcraft_doc::DocError::NoStory(t.story))?;
        let r = t.range();
        let r = if !r.is_empty() {
            r
        } else if forward {
            let end = if word { next_word(&st.text, r.start) } else { next_char(&st.text, r.start) };
            r.start..end
        } else {
            let start = if word { prev_word(&st.text, r.start) } else { prev_char(&st.text, r.start) };
            start..r.start
        };
        let r = with_break_pair(&st.text, r);
        if tracking && t.cell.is_none() {
            // Marked, not removed: the caret steps over the deleted text.
            let at = crate::cmd::changes::mark_deleted(st, r.clone());
            let pos = if forward { at.max(r.end.min(st.len())) } else { r.start.min(at) };
            sel.text = Some(TextSel { anchor: pos, focus: pos, ..t });
            return ok();
        }
        st.delete(r.clone());
        sel.text = Some(TextSel { anchor: r.start, focus: r.start, ..t });
        ok()
    })
}

/// A break character and the return after it end one paragraph: deleting either deletes both.
fn with_break_pair(text: &str, r: std::ops::Range<usize>) -> std::ops::Range<usize> {
    let Some(del) = text.get(r.clone()) else { return r };
    let mut chars = del.chars();
    match (chars.next(), chars.next()) {
        (Some('\n'), None) => match text.get(..r.start).and_then(|t| t.chars().next_back()) {
            Some(b) if designcraft_doc::is_break_char(b) => r.start.saturating_sub(b.len_utf8())..r.end,
            _ => r,
        },
        (Some(b), None) if designcraft_doc::is_break_char(b) && text.get(r.end..).is_some_and(|t| t.starts_with('\n')) => r.start..r.end + 1,
        _ => r,
    }
}

/// Our own filler text (not Adobe's).
pub const PLACEHOLDER: &str = "Ovid ellum quisque arcet velut tempora sint, ne veriora pareant laudemque fieri. Lorem novum strata ponet amicos, \
et tamen uti ferrum caelo tempus mollit. Quisque sinat oppida retro, nec tenuere longas animi semper vias. \
Sic erat, ut nostris ceperunt pectora verbis, mitescunt flamma solidos campos venti. Moventur iam nubes, aequora pontus, \
dum sidera fulgent tacito per inane meatu. Arbore sub magna ludunt pueri, linquunt aurea litora fluctus. \
Iamque tibi rursus ponet nova carmina vates, et longas umbras quaerit sub tegmine fagi. Omnia mutantur, nihil interit.";

pub(super) fn fill_placeholder(s: &mut Session, p: &Value) -> Result<Value> {
    let st = s.doc()?;
    let frame = id_param(p, "frame").or_else(|| st.selection.items.first().copied()).or_else(|| st.selection.text.and_then(|t| t.frame));
    let sid = match frame.and_then(|f| st.doc.item(f)).and_then(|i| i.text_frame()) {
        Some(t) => t.story,
        None => {
            if let Some(f) = frame {
                s.execute("object.content", &json!({"ids": [f.0], "type": "text"}))?;
            }
            story_of(s, p).ok_or_else(|| bad("type.fillWithPlaceholder", "select a text frame"))?
        }
    };
    // Fill until overset with paragraphs of two to five sentences, as InDesign's placeholder text
    // comes (so a paragraph can be styled on its own).
    const SENTENCES_PER_PARAGRAPH: [usize; 7] = [2, 2, 3, 3, 5, 2, 4];
    let mut text = String::new();
    let sentences: Vec<&str> = PLACEHOLDER.split_inclusive(". ").map(str::trim_end).collect();
    let doc = s.doc()?.doc.clone();
    let mut d2 = (*doc).clone();
    let (mut paragraph, mut in_paragraph) = (0, 0);
    for i in 0..400 {
        text.push_str(sentences[i % sentences.len()]);
        in_paragraph += 1;
        if in_paragraph >= SENTENCES_PER_PARAGRAPH[paragraph % SENTENCES_PER_PARAGRAPH.len()] {
            text.push('\n');
            (paragraph, in_paragraph) = (paragraph + 1, 0);
        } else {
            text.push(' ');
        }
        if i % 3 == 2 {
            d2.set_story_text(sid, &text)?;
            let cs = compose::compose_story(&d2, sid, &compose::ComposeOptions::default());
            if cs.is_overset() {
                // Trim to the last sentence that fits.
                let cut = cs.overset_at.unwrap_or(text.len()).min(text.len());
                let keep = text.get(..cut).and_then(|t| t.rfind('.')).map(|k| k + 1).unwrap_or(cut);
                text.truncate(keep);
                break;
            }
        }
    }
    s.edit(|d, _| {
        d.set_story_text(sid, text.trim_end())?;
        ok()
    })
}

/// The case-changed text of `src`, char by char (a char may map to several, e.g. ß → SS).
pub(crate) fn case_map(src: &str, case: &str) -> Vec<String> {
    let mut out = Vec::with_capacity(src.len());
    let mut prev = ' ';
    let mut sentence_start = true;
    for c in src.chars() {
        let word_start = !(prev.is_alphanumeric() || matches!(prev, '\'' | '\u{2019}'));
        let up = |c: char| c.to_uppercase().collect::<String>();
        let low = |c: char| c.to_lowercase().collect::<String>();
        // Title and sentence case capitalize only scripts with initial capitals (not Georgian).
        let cap = |c: char| if designcraft_doc::capitalizes(c) { up(c) } else { low(c) };
        let m = match case {
            "upper" => up(c),
            "lower" => low(c),
            "title" => {
                if word_start {
                    cap(c)
                } else {
                    low(c)
                }
            }
            _ => {
                if sentence_start && c.is_alphanumeric() {
                    cap(c)
                } else {
                    low(c)
                }
            }
        };
        if c.is_alphanumeric() {
            sentence_start = false;
        } else if matches!(c, '.' | '!' | '?' | '\n' | '\u{2028}') {
            sentence_start = true;
        }
        out.push(m);
        prev = c;
    }
    out
}

pub(super) fn change_case(s: &mut Session, p: &Value) -> Result<Value> {
    let case = str_param(p, "case").unwrap_or("upper").to_string();
    if !matches!(case.as_str(), "upper" | "lower" | "title" | "sentence") {
        return Err(bad("type.changeCase", format!("unknown case `{case}`")));
    }
    let targets = format_targets(s);
    s.edit(|d, sel| {
        for t in &targets {
            let Some(st) = d.text_story_mut(t.story, t.cell) else { continue };
            let r = t.range.start.min(st.len())..t.range.end.min(st.len());
            let mapped = case_map(st.slice(r.clone()), &case);
            // Replace run by run (from the end) so every run keeps its formatting.
            let segs: Vec<std::ops::Range<usize>> =
                st.runs().map(|(rr, _)| rr.start.max(r.start)..rr.end.min(r.end)).filter(|x| x.start < x.end).collect();
            let starts: Vec<usize> = st.slice(r.clone()).char_indices().map(|(i, _)| r.start + i).collect();
            let mut delta: isize = 0;
            for seg in segs.iter().rev() {
                let old = st.slice(seg.clone()).to_string();
                let new: String = starts.iter().zip(&mapped).filter(|(b, _)| seg.contains(b)).map(|(_, m)| m.as_str()).collect();
                if new != old {
                    delta += new.len() as isize - old.len() as isize;
                    st.replace(seg.clone(), &new);
                }
            }
            if let Some(ts) = sel.text.as_mut().filter(|ts| ts.story == t.story && ts.cell == t.cell && !ts.is_caret()) {
                let end = (r.end as isize + delta).max(r.start as isize) as usize;
                if ts.anchor <= ts.focus {
                    ts.focus = end;
                } else {
                    ts.anchor = end;
                }
            }
        }
        Ok(Value::Null)
    })
}

#[cfg(test)]
mod autocorrect_tests {
    use serde_json::json;

    use crate::Session;

    #[test]
    fn autocorrect_fixes_the_word_before_a_space() {
        let mut s = Session::new();
        s.execute("file.new", &json!({})).unwrap();
        let r = s.execute("frame.create", &json!({"rect": [72, 72, 400, 200], "content": "text", "text": ""})).unwrap();
        let sid = designcraft_doc::StoryId(r["story"].as_u64().unwrap());
        s.execute("text.select", &json!({"story": sid.0, "anchor": 0, "focus": 0})).unwrap();
        let text = |s: &Session| s.doc().unwrap().doc.stories[&sid].text.clone();
        // Off by default.
        for c in ["t", "e", "h", " "] {
            s.execute("text.insert", &json!({"text": c})).unwrap();
        }
        assert_eq!(text(&s), "teh ");
        s.execute("prefs.set", &json!({"autocorrect": true})).unwrap();
        for c in ["T", "e", "h", " ", "c", "a", "t", "."] {
            s.execute("text.insert", &json!({"text": c})).unwrap();
        }
        assert_eq!(text(&s), "teh The cat.");
        // The caret ends after the inserted punctuation.
        assert_eq!(s.doc().unwrap().selection.text.unwrap().focus, "teh The cat.".len());
    }
}

#[cfg(test)]
mod break_tests {
    use serde_json::json;

    use crate::Session;

    /// Type › Insert Break Character: the break ends its paragraph, the caret starts the next one.
    #[test]
    fn inserted_break_ends_its_paragraph() {
        let mut s = Session::new();
        s.execute("file.new", &json!({})).unwrap();
        let r = s.execute("frame.create", &json!({"rect": [72, 72, 400, 200], "content": "text", "text": "OneTwo\nThree"})).unwrap();
        let sid = designcraft_doc::StoryId(r["story"].as_u64().unwrap());
        let story = |s: &Session| s.doc().unwrap().doc.stories[&sid].clone();
        s.execute("text.select", &json!({"story": sid.0, "anchor": 3, "focus": 3})).unwrap();
        s.execute("text.insert", &json!({"text": "\u{e011}", "raw": true})).unwrap();
        assert_eq!(story(&s).text, "One\u{e011}\nTwo\nThree");
        assert_eq!(s.doc().unwrap().selection.text.unwrap().focus, "One\u{e011}\n".len());
        // At the end of a paragraph the break takes the place of its return.
        let at = story(&s).text.find("\nThree").unwrap();
        s.execute("text.select", &json!({"story": sid.0, "anchor": at, "focus": at})).unwrap();
        s.execute("text.insert", &json!({"text": "\u{e002}", "raw": true})).unwrap();
        let st = story(&s);
        assert_eq!(st.text, "One\u{e011}\nTwo\u{e002}\nThree");
        assert_eq!(st.paras.len(), 3);
        st.check().unwrap();
        // Typing after the break goes to the next paragraph; deleting the break or its return
        // removes both.
        let at = "One\u{e011}".len();
        s.execute("text.select", &json!({"story": sid.0, "anchor": at, "focus": at})).unwrap();
        s.execute("text.insert", &json!({"text": "x"})).unwrap();
        assert_eq!(story(&s).text, "One\u{e011}\nxTwo\u{e002}\nThree");
        s.execute("text.select", &json!({"story": sid.0, "anchor": at + 1, "focus": at + 1})).unwrap();
        s.execute("text.delete", &json!({})).unwrap();
        assert_eq!(story(&s).text, "OnexTwo\u{e002}\nThree");
        s.execute("text.select", &json!({"story": sid.0, "anchor": "OnexTwo".len(), "focus": "OnexTwo".len()})).unwrap();
        s.execute("text.delete", &json!({"forward": true})).unwrap();
        assert_eq!(story(&s).text, "OnexTwoThree");
    }
}

#[cfg(test)]
mod language_tests {
    use serde_json::json;

    use crate::Session;

    #[test]
    fn language_sets_quotes_spelling_and_hyphenation() {
        let mut s = Session::new();
        s.execute("file.new", &json!({})).unwrap();
        let r = s.execute("frame.create", &json!({"rect": [72, 72, 400, 200], "content": "text", "text": ""})).unwrap();
        let sid = r["story"].as_u64().unwrap();
        s.execute("type.char", &json!({"attrs": {"language": "German: 2006 Reform"}})).unwrap();
        s.execute("text.insert", &json!({"text": "\"Wort\" ist's"})).unwrap();
        let text = s.doc().unwrap().doc.stories[&designcraft_doc::StoryId(sid)].text.clone();
        assert_eq!(text, "\u{201E}Wort\u{201C} ist\u{2019}s");
        // German words aren't English misspellings.
        let st = s.execute("spelling.check", &json!({"story": sid})).unwrap();
        assert!(st.as_array().unwrap().is_empty(), "{st}");
    }

    #[test]
    fn cjk_languages_have_their_quotes_and_no_english_rules() {
        use super::quote_marks;
        assert_eq!(quote_marks("Chinese: Simplified"), ['\u{201C}', '\u{201D}', '\u{2018}', '\u{2019}']);
        assert_eq!(quote_marks("Chinese: Traditional"), ['\u{300C}', '\u{300D}', '\u{300E}', '\u{300F}']);
        assert_eq!(quote_marks("Chinese"), ['\u{300C}', '\u{300D}', '\u{300E}', '\u{300F}']);
        assert_eq!(quote_marks("Japanese"), ['\u{300C}', '\u{300D}', '\u{300E}', '\u{300F}']);
        // Other spellings of the language names take the same quotes.
        assert_eq!(quote_marks("Simplified Chinese"), ['\u{201C}', '\u{201D}', '\u{2018}', '\u{2019}']);
        assert_eq!(quote_marks("zh_CN"), ['\u{201C}', '\u{201D}', '\u{2018}', '\u{2019}']);
        assert_eq!(quote_marks("Chinese (Traditional)"), ['\u{300C}', '\u{300D}', '\u{300E}', '\u{300F}']);
        assert_eq!(quote_marks("zh-TW"), ['\u{300C}', '\u{300D}', '\u{300E}', '\u{300F}']);
        assert_eq!(quote_marks("ja_JP"), ['\u{300C}', '\u{300D}', '\u{300E}', '\u{300F}']);
        for l in ["Japanese", "Korean", "Chinese", "Chinese: Simplified", "Chinese: Traditional"] {
            assert!(!designcraft_compose::is_english(l), "{l}: no English hyphenation or spelling");
        }
    }

    #[test]
    fn locale_coded_languages_take_their_quotes_and_rules() {
        use super::quote_marks;
        const GERMAN: [char; 4] = ['\u{201E}', '\u{201C}', '\u{201A}', '\u{2018}'];
        const DUTCH: [char; 4] = ['\u{201E}', '\u{201D}', '\u{201A}', '\u{2019}'];
        const GUILLEMETS: [char; 4] = ['\u{00AB}', '\u{00BB}', '\u{2039}', '\u{203A}'];
        const SPANISH: [char; 4] = ['\u{00AB}', '\u{00BB}', '\u{201C}', '\u{201D}'];
        const ENGLISH: [char; 4] = ['\u{201C}', '\u{201D}', '\u{2018}', '\u{2019}'];
        const CORNERS: [char; 4] = ['\u{300C}', '\u{300D}', '\u{300E}', '\u{300F}'];
        for (language, quotes, english) in [
            ("de_DE_2006", GERMAN, false),
            ("German: 2006 Reform", GERMAN, false),
            ("de_CH_2006", GUILLEMETS, false),
            ("German: Swiss 2006 Reform", GUILLEMETS, false),
            ("nl_NL_2005", DUTCH, false),
            ("Dutch: 2005 Reform", DUTCH, false),
            ("English: USA", ENGLISH, true),
            ("English: UK", ENGLISH, true),
            ("en_GB", ENGLISH, true),
            ("Spanish: Castilian", SPANISH, false),
            ("es_ES", SPANISH, false),
            ("French", GUILLEMETS, false),
            ("fr_FR", GUILLEMETS, false),
            ("nb_NO", GUILLEMETS, false),
            ("ja_JP", CORNERS, false),
            ("[No Language]", ENGLISH, false),
        ] {
            assert_eq!(quote_marks(language), quotes, "{language}");
            assert_eq!(designcraft_compose::is_english(language), english, "{language}");
        }
    }

    #[test]
    fn idml_language_names_keep_their_spelling_and_pick_the_language() {
        use std::io::Read;
        // A stand-in for Songti SC (first in the Simplified Chinese chain; the real one when installed).
        designcraft_fonts::FontDb::global().add_font(designcraft_fonts::testing::font_with("Songti SC", &['直']).unwrap());
        let mut s = Session::new();
        s.execute("file.new", &json!({})).unwrap();
        let r = s.execute("frame.create", &json!({"rect": [72, 72, 300, 200], "content": "text", "text": "直"})).unwrap();
        s.execute("text.select", &json!({"story": r["story"], "anchor": 0, "focus": 3})).unwrap();
        s.execute("type.char", &json!({"attrs": {"language": "Simplified Chinese"}})).unwrap();
        let idml = designcraft_idml::export_idml(&s.doc().unwrap().doc);
        let mut z = zip::ZipArchive::new(std::io::Cursor::new(idml.clone())).unwrap();
        let stories: String = (0..z.len())
            .map(|i| {
                let mut f = z.by_index(i).unwrap();
                let mut x = String::new();
                if f.name().starts_with("Stories/") {
                    f.read_to_string(&mut x).unwrap();
                }
                x
            })
            .collect();
        assert!(stories.contains(r#"AppliedLanguage="$ID/Simplified Chinese""#), "{stories}");
        // The name comes back as written and sets the text as Simplified Chinese.
        let mut back = designcraft_idml::import_idml(&idml).unwrap();
        back.settings.glyph_fallback = true;
        let st = back.stories.values().find(|st| st.text.contains('直')).unwrap();
        let (_, base) = back.styles.resolve_para(&st.paras[0]);
        assert_eq!(back.styles.resolve_char(&base, st.char_format_at(0)).language, "Simplified Chinese");
        let cs = designcraft_compose::compose_story(&back, st.id, &Default::default());
        let face = cs.frames[0].lines[0].glyphs.iter().find(|g| g.byte == 0).unwrap().face;
        assert_eq!(face.family, "Songti SC");
    }
}

#[cfg(test)]
mod arabic_tests {
    use super::*;
    #[test]
    fn arabic_story_direction_command_validates_and_supports_undo() {
        let mut s = Session::new();
        s.execute("file.new", &json!({})).unwrap();
        let r = s.execute("frame.create", &json!({"rect": [0, 0, 300, 200], "content": "text", "text": "abc"})).unwrap();
        let sid = StoryId(r["story"].as_u64().unwrap());
        let before = s.cache.get(&s.doc().unwrap().doc, sid, None);
        s.execute("story.setDirection", &json!({"story": sid.0, "direction": "rightToLeft"})).unwrap();
        assert_eq!(s.doc().unwrap().doc.story(sid).unwrap().direction, designcraft_doc::TextDirection::RightToLeft);
        let after = s.cache.get(&s.doc().unwrap().doc, sid, None);
        assert!(!std::sync::Arc::ptr_eq(&before, &after));
        assert!(s.execute("story.setDirection", &json!({"story": sid.0, "direction": "unknown"})).is_err());
        s.execute("edit.undo", &json!({})).unwrap();
        assert_eq!(s.doc().unwrap().doc.story(sid).unwrap().direction, designcraft_doc::TextDirection::LeftToRight);
    }
}

#[cfg(test)]
mod korean_breaks_tests {
    use serde_json::json;

    use crate::Session;

    #[test]
    fn korean_character_breaks_are_a_paragraph_attribute() {
        let mut s = Session::new();
        s.execute("file.new", &json!({})).unwrap();
        let r = s.execute("frame.create", &json!({"rect": [72, 72, 400, 200], "content": "text", "text": "한국어 문장"})).unwrap();
        let sid = designcraft_doc::StoryId(r["story"].as_u64().unwrap());
        s.execute("type.para", &json!({"attrs": {"koreanCharBreaks": true}})).unwrap();
        assert_eq!(s.doc().unwrap().doc.story(sid).unwrap().paras[0].para.korean_char_breaks, Some(true));
        s.execute("style.paragraph.create", &json!({"name": "Korean", "para": {"koreanCharBreaks": true}})).unwrap();
        assert_eq!(s.doc().unwrap().doc.styles.para("Korean").unwrap().para.korean_char_breaks, Some(true));
    }
}

#[cfg(test)]
mod reserve_tests {
    use super::*;

    #[test]
    fn failed_reservation_rolls_back_the_edit_even_after_local_text_was_removed() {
        let mut s = Session::new();
        s.execute("file.new", &json!({})).unwrap();
        let result = s.execute("frame.create", &json!({"rect":[36,36,300,300],"content":"text","text":"Keep this text."})).unwrap();
        let sid = StoryId(result["story"].as_u64().unwrap());
        s.execute("text.select", &json!({"story":sid.0,"anchor":1,"focus":4})).unwrap();
        let before = s.doc().unwrap().clone();
        assert!(before.selection.text.is_some_and(|t| !t.range().is_empty()));
        let journal = s.journal.clone();
        let error = s
            .edit(|d, sel| {
                let st = d.story_mut(sid).unwrap();
                st.format_chars(5..9, |f| f.over.change = Some(designcraft_doc::ChangeMark::Inserted));
                let at = crate::cmd::changes::mark_deleted(st, 0..9);
                assert_eq!(at, 5);
                assert_eq!(st.text, "Keep  text.");
                assert_eq!(st.format_after(0).over.change, Some(designcraft_doc::ChangeMark::Deleted));
                sel.text = None;
                reserve_command_text(st, usize::MAX)?;
                ok()
            })
            .unwrap_err();
        assert!(error.to_string().contains("could not reserve text insertion"));
        let after = s.doc().unwrap();
        assert!(std::sync::Arc::ptr_eq(&after.doc, &before.doc));
        assert_eq!(after.selection, before.selection);
        assert_eq!(after.revision, before.revision);
        assert_eq!(after.history.undo.len(), before.history.undo.len());
        assert_eq!(after.history.redo.len(), before.history.redo.len());
        assert_eq!(s.journal, journal);
    }
}

#[cfg(test)]
mod placeholder_tests {
    use serde_json::json;

    use crate::Session;

    /// Fill with Placeholder Text gives paragraphs of a few sentences, as InDesign's does (it was one
    /// paragraph, and the filler ran sentences together: "interit.Ovid").
    #[test]
    fn placeholder_text_comes_in_paragraphs() {
        let mut s = Session::new();
        s.execute("file.new", &json!({})).unwrap();
        let r = s.execute("frame.create", &json!({"rect": [36, 36, 336, 736], "content": "text", "text": ""})).unwrap();
        s.execute("type.fillWithPlaceholder", &json!({"frame": r["id"]})).unwrap();
        let st = s.execute("story.get", &json!({"story": r["story"]})).unwrap();
        let text = st["text"].as_str().unwrap();
        let paragraphs: Vec<&str> = text.split('\n').collect();
        assert!(paragraphs.len() >= 4, "{} paragraphs", paragraphs.len());
        for p in &paragraphs {
            assert!(p.ends_with('.') && !p.starts_with(' '), "a paragraph of whole sentences: {p:?}");
        }
        assert!(!text.contains(".O") && !text.contains(".L"), "sentences are spaced: {text}");
        assert!(st["overset"].is_null(), "nothing overset: {}", st["overset"]);
        assert!(text.len() > 1500, "the frame is filled: {}", text.len());
    }
}
