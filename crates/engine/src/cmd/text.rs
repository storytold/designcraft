//! Text editing (Type tool) and character/paragraph formatting.

use designcraft_compose as compose;
use designcraft_doc::story::floor_char_boundary;
use designcraft_doc::{CellAddr, CharAttrs, Content, ItemId, ParaAttrs, Selection, StoryId, TextSel};
use designcraft_geom::Point;
use serde_json::{Value, json};

use super::{CommandSpec, bad, bool_or, cmd, has_doc, has_text, has_text_or_frames, id_param, ok, point_param, str_param};
use crate::{Result, Session};

pub fn specs() -> Vec<CommandSpec> {
    vec![
        cmd!(noundo "text.placeCaret", "Place Caret", [], None, "{frame, point: [x,y] (spread coords)} — converts empty frames to text frames", has_doc, |s, p| place(s, p, false)),
        cmd!(
            "text.release",
            "Release",
            [],
            None,
            "{frame, point, moved: bool, copy?: bool} — ends a press in text: a drag that started in the selected text moves (copy: duplicates) it to the point; a click places the caret",
            has_doc,
            release
        ),
        cmd!(noundo "text.extendTo", "Extend Text Selection", [], None, "{frame, point}", has_doc, |s, p| place(s, p, true)),
        cmd!(noundo "text.selectWord", "Select Word", [], None, "{frame, point}", has_doc, |s, p| {
            place(s, p, false)?;
            let st = s.doc_mut()?;
            let Some(t) = st.selection.text else { return ok() };
            let text = &st.doc.text_story(t.story, t.cell).map(|x| x.text.clone()).unwrap_or_default();
            let (a, b) = word_bounds(text, t.focus);
            st.selection.text = Some(TextSel { anchor: a, focus: b, ..t });
            ok()
        }),
        cmd!(noundo "text.select", "Select Text", [], None, "{story, anchor, focus} (UTF-8 byte offsets into the story text, as find.find reports them: á or — counts 2 or 3)", has_doc, |s, p| {
            let sid = StoryId(p.get("story").and_then(Value::as_u64).unwrap_or(0));
            let st = s.doc_mut()?;
            let text = &st.doc.story(sid).ok_or_else(|| bad("text.select", "no such story"))?.text;
            let a = floor_char_boundary(text, p.get("anchor").and_then(Value::as_u64).unwrap_or(0) as usize);
            let f = floor_char_boundary(text, p.get("focus").and_then(Value::as_u64).map(|v| v as usize).unwrap_or(a));
            st.selection = Selection::text(TextSel { story: sid, anchor: a, focus: f, frame: None, cell: None });
            st.revision += 1;
            ok()
        }),
        cmd!("text.insert", "Type", [], None, "{text, raw?: bool (no typographer's quotes)} — replaces the selected text", has_text, insert),
        cmd!("text.delete", "Delete Text", [], None, "{forward?: bool, word?: bool}", has_text, delete),
        cmd!(noundo "text.move", "Move Caret", [], None, "{dir: left|right|up|down|lineStart|lineEnd|storyStart|storyEnd, extend?, word?}", has_text, move_caret),
        cmd!(noundo "text.exitToFrame", "Select Frame", [], None, "{}", has_doc, |s, _| {
            let st = s.doc_mut()?;
            if let Some(t) = st.selection.text.take() {
                let f = t.frame.or_else(|| st.doc.story(t.story).and_then(|x| x.frames.first().copied()));
                st.selection.items = f.into_iter().collect();
            }
            st.selection.cells = None;
            st.revision += 1;
            ok()
        }),
        cmd!(
            "story.setDirection",
            "Story Column Direction",
            ["Type", "Story Column Direction"],
            None,
            "{story?, direction: leftToRight|rightToLeft} — column progression, independent of paragraph direction",
            has_doc,
            |s, p| {
                let sid = story_of(s, p).ok_or_else(|| bad("story.setDirection", "no story"))?;
                let direction: designcraft_doc::TextDirection =
                    serde_json::from_value(p.get("direction").cloned().ok_or_else(|| bad("story.setDirection", "missing direction"))?)
                        .map_err(|e| bad("story.setDirection", e.to_string()))?;
                s.edit(|d, _| {
                    let st = d.story_mut(sid).ok_or(designcraft_doc::DocError::NoStory(sid))?;
                    st.direction = direction;
                    st.rev = st.rev.wrapping_add(1);
                    ok()
                })
            }
        ),
        cmd!("story.setText", "Set Story Text", [], None, "{story, text} — replace a story's whole text", has_doc, |s, p| {
            let sid = StoryId(p.get("story").and_then(Value::as_u64).ok_or_else(|| bad("story.setText", "missing story"))?);
            let text = str_param(p, "text").unwrap_or("").to_string();
            s.edit(|d, sel| {
                d.set_story_text(sid, &text)?;
                if let Some(t) = sel.text.as_mut().filter(|t| t.story == sid) {
                    t.anchor = floor_char_boundary(&text, t.anchor);
                    t.focus = floor_char_boundary(&text, t.focus);
                }
                ok()
            })
        }),
        cmd!(
            "story.replaceRange",
            "Edit Story",
            [],
            None,
            "{story, start, end, text} — replace a byte range keeping surrounding formatting",
            has_doc,
            |s, p| {
                let sid = StoryId(p.get("story").and_then(Value::as_u64).ok_or_else(|| bad("story.replaceRange", "missing story"))?);
                let a = p.get("start").and_then(Value::as_u64).unwrap_or(0) as usize;
                let b = p.get("end").and_then(Value::as_u64).map(|v| v as usize).unwrap_or(a);
                let text = str_param(p, "text").unwrap_or("").to_string();
                s.edit(|d, sel| {
                    let st = d.story_mut(sid).ok_or(designcraft_doc::DocError::NoStory(sid))?;
                    let (a, b) = (a.min(st.len()), b.min(st.len()).max(a.min(st.len())));
                    st.replace(a..b, &text);
                    let len = st.len();
                    if let Some(t) = sel.text.as_mut().filter(|t| t.story == sid) {
                        t.anchor = floor_char_boundary(&st.text, t.anchor);
                        t.focus = floor_char_boundary(&st.text, t.focus);
                    }
                    Ok(json!({"length": len}))
                })
            }
        ),
        cmd!(query "story.get", "Get Story", [], None, "{story? | frame?} → text, frames, paragraphs, overset", has_doc, |s, p| {
            let st = s.doc()?;
            let sid = story_of(s, p).ok_or_else(|| bad("story.get", "no story"))?;
            let cs = s.cache.get(&st.doc, sid, None);
            let mut v = st.doc.story_summary(sid).unwrap_or_default();
            v["overset"] = json!(cs.overset_at);
            v["lines"] = json!(cs.line_count());
            Ok(v)
        }),
        cmd!("type.fillWithPlaceholder", "Fill with Placeholder Text", ["Type"], None, "{frame?}", has_text_or_frames, fill_placeholder),
        cmd!(
            "type.char",
            "Character Formatting",
            [],
            None,
            "{attrs: {fontFamily?, fontStyle?, size?, leading?: {kind:auto}|{kind:points,value}, tracking?, kerning?, hScale?, vScale?, baselineShift?, fill?, capitalization?, underline?, …}}",
            has_text_or_frames,
            |s, p| format_chars(s, p.get("attrs").unwrap_or(p))
        ),
        cmd!(
            "type.para",
            "Paragraph Formatting",
            [],
            None,
            "{attrs: {align?, leftIndent?, firstLineIndent?, spaceBefore?, spaceAfter?, dropCapLines?, hyphenate?, composer?, tabs?, …}}",
            has_text_or_frames,
            |s, p| format_paras(s, p.get("attrs").unwrap_or(p))
        ),
        cmd!("type.align", "Align", [], None, "{align: left|center|right|leftJustified|…}", has_text_or_frames, |s, p| {
            format_paras(s, &json!({"align": p.get("align").cloned().unwrap_or(json!("left"))}))
        }),
        cmd!("type.alignLeft", "Align Left", [], Some("Cmd+Shift+L"), "{}", has_text_or_frames, |s, _| format_paras(s, &json!({"align": "left"}))),
        cmd!("type.alignCenter", "Align Center", [], Some("Cmd+Shift+C"), "{}", has_text_or_frames, |s, _| format_paras(
            s,
            &json!({"align": "center"})
        )),
        cmd!("type.alignRight", "Align Right", [], Some("Cmd+Shift+R"), "{}", has_text_or_frames, |s, _| format_paras(s, &json!({"align": "right"}))),
        cmd!("type.justify", "Justify Left", [], Some("Cmd+Shift+J"), "{}", has_text_or_frames, |s, _| format_paras(
            s,
            &json!({"align": "leftJustified"})
        )),
        cmd!("type.bold", "Bold", [], Some("Cmd+Shift+B"), "{}", has_text_or_frames, |s, _| format_chars(s, &json!({"fontStyle": "Bold"}))),
        cmd!("type.italic", "Italic", [], Some("Cmd+Shift+I"), "{}", has_text_or_frames, |s, _| format_chars(s, &json!({"fontStyle": "Italic"}))),
        cmd!("type.underline", "Underline", [], Some("Cmd+Shift+U"), "{}", has_text_or_frames, |s, _| format_chars(s, &json!({"underline": true}))),
        cmd!("type.allCaps", "All Caps", [], Some("Cmd+Shift+K"), "{}", has_text_or_frames, |s, _| format_chars(
            s,
            &json!({"capitalization": "allCaps"})
        )),
        cmd!(
            "type.tateChuYoko",
            "Tate-Chu-Yoko",
            [],
            None,
            "{on?: bool} — set the selected text horizontally within one em of a vertical line (toggles by default)",
            has_text_or_frames,
            |s, p| {
                let on = match p.get("on").and_then(Value::as_bool) {
                    Some(v) => v,
                    None => !format_targets(s).first().is_some_and(|t| {
                        s.doc().ok().and_then(|d| d.doc.text_story(t.story, t.cell)).is_some_and(|st| {
                            st.char_format_at(if t.range.is_empty() { t.range.start } else { t.range.start + 1 }).over.tate_chu_yoko == Some(true)
                        })
                    }),
                };
                format_chars(s, &json!({"tate_chu_yoko": on}))
            }
        ),
        cmd!(
            "type.ruby",
            "Ruby",
            [],
            None,
            "{text} — the reading set over the selected text (one group); empty removes it",
            has_text_or_frames,
            |s, p| { format_chars(s, &json!({"ruby": str_param(p, "text").unwrap_or("")})) }
        ),
        cmd!(
            "type.kenten",
            "Kenten",
            [],
            None,
            "{on?: bool} — emphasis dots over the selected characters (toggles by default)",
            has_text_or_frames,
            |s, p| {
                let on = match p.get("on").and_then(Value::as_bool) {
                    Some(v) => v,
                    None => !format_targets(s).first().is_some_and(|t| {
                        s.doc().ok().and_then(|d| d.doc.text_story(t.story, t.cell)).is_some_and(|st| {
                            st.char_format_at(if t.range.is_empty() { t.range.start } else { t.range.start + 1 }).over.kenten == Some(true)
                        })
                    }),
                };
                format_chars(s, &json!({"kenten": on}))
            }
        ),
        cmd!("type.sizeUp", "Increase Point Size", [], Some("Cmd+Shift+."), "{}", has_text_or_frames, |s, _| step_size(s, 2.0)),
        cmd!("type.sizeDown", "Decrease Point Size", [], Some("Cmd+Shift+,"), "{}", has_text_or_frames, |s, _| step_size(s, -2.0)),
        cmd!(
            "type.changeCase",
            "Change Case",
            ["Type"],
            None,
            "{case: upper|lower|title|sentence} — the selected text, or the stories of selected frames (formatting kept)",
            has_text_or_frames,
            change_case
        ),
        cmd!(
            "type.openType",
            "OpenType",
            [],
            None,
            "{feature?: dlig|frac|ordn|swsh|titl|calt|zero|ssNN|any tag, on?: bool (default: toggle), figures?: tabularLining|proportionalOldstyle|proportionalLining|tabularOldstyle|default, stylisticSets?: [1–20]} → {features, figures}",
            has_text_or_frames,
            open_type
        ),
        cmd!(
            "style.breakLink",
            "Break Link to Style",
            [],
            None,
            "{kind: paragraph|character} — the selected text keeps its look as local formatting, with [No Paragraph Style] / [None]",
            has_text_or_frames,
            break_link
        ),
        cmd!(query "type.selectionAttrs", "Selection Attributes", [], None, "{} → resolved character/paragraph attributes at the text selection", has_doc, selection_attrs),
    ]
}

pub(crate) fn story_of(s: &Session, p: &Value) -> Option<StoryId> {
    let st = s.active()?;
    if let Some(v) = p.get("story").and_then(Value::as_u64) {
        return Some(StoryId(v));
    }
    let frame = id_param(p, "frame").or_else(|| st.selection.items.first().copied());
    if let Some(f) = frame {
        return st.doc.item(f).and_then(|i| i.text_frame()).map(|t| t.story);
    }
    st.selection.text.map(|t| t.story)
}

/// Story byte at spread point `pt` in text frame `frame` (in a table cell's story when the point is in a cell).
fn hit_byte(s: &Session, frame: ItemId, pt: Point) -> Option<(StoryId, usize, Option<CellAddr>)> {
    let st = s.active()?;
    let loc = st.doc.find(frame)?;
    let it = st.doc.item_at(&loc)?;
    let sid = it.text_frame()?.story;
    let xf = st.doc.parent_xf(&loc) * it.text_xf();
    let inner = xf.inverse() * pt;
    let cs = s.cache.get(&st.doc, sid, None);
    let fi = cs.frames.iter().position(|f| f.frame == frame)?;
    if let Some((table, row, col, b)) = compose::hit_cell(&cs, fi, inner) {
        return Some((sid, b, Some(CellAddr { table, row, col })));
    }
    if let Some((id, b)) = compose::hit_note(&cs, fi, inner) {
        return Some((sid, b, Some(CellAddr::footnote(id))));
    }
    let b = compose::hit(&cs, fi, inner).unwrap_or(0);
    Some((sid, b, None))
}

fn place(s: &mut Session, p: &Value, extend: bool) -> Result<Value> {
    let frame = id_param(p, "frame").ok_or_else(|| bad("text.placeCaret", "missing frame"))?;
    let pt = point_param(p, "point").unwrap_or(Point::ZERO);
    // Empty frames become text frames.
    let needs_convert = s.doc()?.doc.item(frame).is_some_and(|i| matches!(i.content, Content::Unassigned));
    if needs_convert {
        s.execute("object.content", &json!({"ids": [frame.0], "type": "text"}))?;
    }
    let (sid, b, cell) = hit_byte(s, frame, pt).ok_or_else(|| bad("text.placeCaret", "not a text frame"))?;
    // Drag and drop: a press inside the selected text keeps it (to be dragged).
    if !extend {
        s.text_drag = s.doc()?.selection.text.is_some_and(|t| t.story == sid && t.cell == cell && !t.range().is_empty() && t.range().contains(&b));
        if s.text_drag {
            return Ok(json!({"story": sid.0, "dragging": true}));
        }
    } else if s.text_drag {
        return Ok(json!({"story": sid.0, "dragging": true}));
    }
    let st = s.doc_mut()?;
    // Dragging from one cell into another selects cells.
    if extend
        && let (Some(t), Some(c)) = (st.selection.text, cell)
        && let Some(a) = t.cell
        && t.story == sid
        && a.table == c.table
        && (a.row, a.col) != (c.row, c.col)
    {
        let range = designcraft_doc::CellRange::new(a.row, a.col, c.row, c.col);
        st.selection.cells = Some(designcraft_doc::TableSel { story: sid, table: c.table, range });
        st.revision += 1;
        return Ok(json!({"story": sid.0, "cells": range}));
    }
    let t = match (extend, st.selection.text) {
        (true, Some(t)) if t.story == sid && t.cell == cell => TextSel { focus: b, frame: Some(frame), ..t },
        _ => TextSel { story: sid, anchor: b, focus: b, frame: Some(frame), cell },
    };
    st.selection = Selection::text(t);
    st.revision += 1;
    Ok(json!({"story": sid.0, "pos": b, "cell": cell}))
}

fn release(s: &mut Session, p: &Value) -> Result<Value> {
    if !std::mem::take(&mut s.text_drag) {
        return ok();
    }
    let frame = id_param(p, "frame").ok_or_else(|| bad("text.release", "missing frame"))?;
    let pt = point_param(p, "point").unwrap_or(Point::ZERO);
    if !p.get("moved").and_then(Value::as_bool).unwrap_or(false) {
        // A click in the selection: the caret goes there.
        s.doc_mut()?.selection.text = None;
        return place(s, &json!({"frame": frame.0, "point": [pt.x, pt.y]}), false);
    }
    let copy = p.get("copy").and_then(Value::as_bool).unwrap_or(false);
    let Some(t) = s.doc()?.selection.text else { return ok() };
    let Some((sid, b, cell)) = hit_byte(s, frame, pt) else { return ok() };
    let r = t.range();
    if sid != t.story || cell != t.cell || (r.contains(&b) && !copy) {
        return ok();
    }
    s.edit(|d, sel| {
        let st = d.text_story_mut(sid, cell).ok_or(designcraft_doc::DocError::NoStory(sid))?;
        let piece = st.extract(r.clone());
        let mut at = b;
        if !copy {
            st.delete(r.clone());
            if at > r.start {
                at -= r.len().min(at - r.start);
            }
        }
        let end = st.insert_story(at, &piece);
        sel.text = Some(TextSel { story: sid, anchor: at, focus: end, frame: Some(frame), cell });
        Ok(json!({"moved": piece.text.len(), "at": at}))
    })
}

fn insert(s: &mut Session, p: &Value) -> Result<Value> {
    let text = super::text_param(p, "text");
    // Tab in a table cell moves to the next cell (Shift-Tab: `table.prevCell`).
    if text == "\t" && s.doc()?.selection.text.is_some_and(|t| t.cell.is_some()) {
        return super::table::step_cell(s, true);
    }
    let raw = p.get("raw").and_then(Value::as_bool).unwrap_or(false);
    let text = if s.prefs.typographers_quotes && !raw { smart_quotes(s, &text) } else { text };
    let tracking = s.doc()?.doc.settings.track_changes;
    let autocorrect = (s.prefs.autocorrect && !tracking && !raw).then(|| s.prefs.autocorrect_list.clone());
    s.edit(|d, sel| {
        let t = sel.text.ok_or_else(|| bad("text.insert", "no insertion point"))?;
        let st = d.text_story_mut(t.story, t.cell).ok_or(designcraft_doc::DocError::NoStory(t.story))?;
        let r = t.range();
        let mut r = r.start.min(st.len())..r.end.min(st.len());
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
        // Typing next to a table anchor starts a paragraph of its own.
        let mut text = text.clone();
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
            let at = super::changes::mark_deleted(st, r.clone());
            let mut fmt = st.char_format_at(at).clone();
            fmt.over.change = Some(designcraft_doc::ChangeMark::Inserted);
            st.insert_with(at, &text, fmt);
            let pos = at + text.len() - trail;
            sel.text = Some(TextSel { anchor: pos, focus: pos, ..t });
            return Ok(json!({"pos": pos}));
        }
        st.replace(r.clone(), &text);
        let pos = r.start + text.len() - trail;
        sel.text = Some(TextSel { anchor: pos, focus: pos, ..t });
        Ok(json!({"pos": pos}))
    })
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

/// Typographer's quotes of a language: [double open, double close, single open, single close].
pub fn quote_marks(language: &str) -> [char; 4] {
    let l = language.to_ascii_lowercase();
    if l.starts_with("german: swiss") || l.contains("swiss") {
        ['\u{00AB}', '\u{00BB}', '\u{2039}', '\u{203A}']
    } else if l.starts_with("german")
        || l.starts_with("czech")
        || l.starts_with("slovak")
        || l.starts_with("bulgarian")
        || l.starts_with("lithuanian")
    {
        ['\u{201E}', '\u{201C}', '\u{201A}', '\u{2018}']
    } else if l.starts_with("french")
        || l.starts_with("russian")
        || l.starts_with("ukrainian")
        || l.starts_with("norwegian")
        || l.starts_with("greek")
    {
        ['\u{00AB}', '\u{00BB}', '\u{2039}', '\u{203A}']
    } else if l.starts_with("spanish") || l.starts_with("italian") || l.starts_with("portuguese") || l.starts_with("catalan") {
        ['\u{00AB}', '\u{00BB}', '\u{201C}', '\u{201D}']
    } else if l.starts_with("dutch")
        || l.starts_with("polish")
        || l.starts_with("romanian")
        || l.starts_with("hungarian")
        || l.starts_with("croatian")
    {
        ['\u{201E}', '\u{201D}', '\u{201A}', '\u{2019}']
    } else if l.starts_with("swedish") || l.starts_with("finnish") {
        ['\u{201D}', '\u{201D}', '\u{2019}', '\u{2019}']
    } else if l.starts_with("japanese") || l.starts_with("chinese") {
        ['\u{300C}', '\u{300D}', '\u{300E}', '\u{300F}']
    } else if l.starts_with("danish") {
        ['\u{00BB}', '\u{00AB}', '\u{203A}', '\u{2039}']
    } else {
        ['\u{201C}', '\u{201D}', '\u{2018}', '\u{2019}']
    }
}

pub(crate) fn delete_selection(s: &mut Session) -> Result<Value> {
    let tracking = s.doc()?.doc.settings.track_changes;
    s.edit(|d, sel| {
        let t = sel.text.ok_or_else(|| bad("text.delete", "no text"))?;
        let r = t.range();
        if let Some(st) = d.text_story_mut(t.story, t.cell) {
            if tracking && t.cell.is_none() {
                super::changes::mark_deleted(st, r.clone());
            } else {
                st.delete(r.clone());
            }
        }
        sel.text = Some(TextSel { anchor: r.start, focus: r.start, ..t });
        ok()
    })
}

fn delete(s: &mut Session, p: &Value) -> Result<Value> {
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
        if tracking && t.cell.is_none() {
            // Marked, not removed: the caret steps over the deleted text.
            let at = super::changes::mark_deleted(st, r.clone());
            let pos = if forward { at.max(r.end.min(st.len())) } else { r.start.min(at) };
            sel.text = Some(TextSel { anchor: pos, focus: pos, ..t });
            return ok();
        }
        st.delete(r.clone());
        sel.text = Some(TextSel { anchor: r.start, focus: r.start, ..t });
        ok()
    })
}

pub fn next_char(s: &str, i: usize) -> usize {
    s[i.min(s.len())..].chars().next().map(|c| i + c.len_utf8()).unwrap_or(s.len())
}
pub fn prev_char(s: &str, i: usize) -> usize {
    s[..i.min(s.len())].chars().next_back().map(|c| i - c.len_utf8()).unwrap_or(0)
}
fn is_word(c: char) -> bool {
    c.is_alphanumeric() || c == '\'' || c == '’'
}
pub fn next_word(s: &str, i: usize) -> usize {
    let mut j = i;
    let mut seen = false;
    for (k, c) in s[i..].char_indices() {
        if is_word(c) {
            seen = true;
        } else if seen {
            return i + k;
        }
        j = i + k + c.len_utf8();
    }
    j
}
pub fn prev_word(s: &str, i: usize) -> usize {
    let mut seen = false;
    for (k, c) in s[..i].char_indices().rev() {
        if is_word(c) {
            seen = true;
        } else if seen {
            return k + c.len_utf8();
        }
    }
    0
}
fn word_bounds(s: &str, i: usize) -> (usize, usize) {
    let a = s[..i.min(s.len())].char_indices().rev().take_while(|(_, c)| is_word(*c)).last().map(|(k, _)| k).unwrap_or(i);
    let b = s[i.min(s.len())..].char_indices().find(|(_, c)| !is_word(*c)).map(|(k, _)| i + k).unwrap_or(s.len());
    (a, b)
}

fn move_caret(s: &mut Session, p: &Value) -> Result<Value> {
    let dir = str_param(p, "dir").unwrap_or("right").to_string();
    let extend = bool_or(p, "extend", false);
    let word = bool_or(p, "word", false);
    let st = s.doc()?;
    let t = st.selection.text.ok_or_else(|| bad("text.move", "no caret"))?;
    let story = st.doc.text_story(t.story, t.cell).ok_or(designcraft_doc::DocError::NoStory(t.story))?;
    let text = story.text.clone();
    let cs = s.cache.get(&st.doc, t.story, None);
    // In a cell, lines come from the cell's own composition.
    let cs = match t.cell {
        Some(c) if c.footnote_id().is_some() => compose::find_note(&cs, c.row as u64).map(|(_, n)| n.text.clone()).unwrap_or(cs),
        Some(c) => compose::find_cell(&cs, c.table, c.row, c.col).map(|(_, _, pc)| pc.text.clone()).unwrap_or(cs),
        None => cs,
    };
    let pos = t.focus.min(text.len());
    let collapse_to = |left: bool| if left { t.range().start } else { t.range().end };
    let new = match dir.as_str() {
        "left" if !extend && !t.is_caret() => collapse_to(true),
        "right" if !extend && !t.is_caret() => collapse_to(false),
        "left" | "right" => {
            // Lines with right-to-left text move as drawn; at a line's edge, or by words, a
            // right-to-left line's "left" is forward in the text.
            let left = dir == "left";
            let visual = if word { None } else { compose::visual_step(&cs, pos, left) };
            match visual {
                Some(p) => p,
                None => {
                    let forward = left == compose::line_rtl(&cs, pos);
                    match (forward, word) {
                        (true, true) => next_word(&text, pos),
                        (true, false) => next_char(&text, pos),
                        (false, true) => prev_word(&text, pos),
                        (false, false) => prev_char(&text, pos),
                    }
                }
            }
        }
        "up" | "down" => vertical(&cs, pos, dir == "up").unwrap_or(pos),
        "lineStart" => line_of(&cs, pos).map(|l| l.0).unwrap_or(0),
        "lineEnd" => line_of(&cs, pos).map(|l| l.1).unwrap_or(text.len()),
        "storyStart" => 0,
        "storyEnd" => text.len(),
        other => return Err(bad("text.move", format!("unknown dir `{other}`"))),
    };
    let stm = s.doc_mut()?;
    stm.selection.text = Some(if extend { TextSel { focus: new, ..t } } else { TextSel { anchor: new, focus: new, ..t } });
    stm.revision += 1;
    Ok(json!({"pos": new}))
}

fn line_of(cs: &compose::ComposedStory, pos: usize) -> Option<(usize, usize)> {
    cs.frames.iter().flat_map(|f| f.lines.iter()).find(|l| pos >= l.range.start && pos <= l.range.end).map(|l| (l.range.start, l.range.end))
}

fn vertical(cs: &compose::ComposedStory, pos: usize, up: bool) -> Option<usize> {
    let (fi, x, baseline, _, _) = compose::caret(cs, pos)?;
    let lines: Vec<(usize, &compose::Line)> = cs.frames.iter().enumerate().flat_map(|(i, f)| f.lines.iter().map(move |l| (i, l))).collect();
    let cur = lines.iter().position(|(i, l)| *i == fi && (l.baseline - baseline).abs() < 0.01 && pos >= l.range.start && pos <= l.range.end)?;
    let target = if up { cur.checked_sub(1)? } else { cur + 1 };
    let (tf, tl) = lines.get(target)?;
    compose::hit(cs, *tf, Point::new(x, tl.baseline - 1.0))
}

/// Where a formatting command applies: a story (or a table cell's story) and a byte range.
#[derive(Clone, Debug)]
pub(crate) struct Target {
    pub story: StoryId,
    pub cell: Option<CellAddr>,
    pub range: std::ops::Range<usize>,
}

/// Ranges and stories a formatting command applies to: selected table cells (whole cells), the
/// text selection, or whole stories of the selected text frames.
pub(crate) fn format_targets(s: &Session) -> Vec<Target> {
    let Some(st) = s.active() else { return vec![] };
    if let Some(ts) = st.selection.cells
        && let Some(t) = st.doc.story(ts.story).and_then(|x| x.tables.get(&ts.table))
    {
        let mut v = Vec::new();
        let owners = t.owners();
        for r in ts.range.r0..=ts.range.r1.min(t.nrows().saturating_sub(1)) {
            for c in ts.range.c0..=ts.range.c1.min(t.ncols().saturating_sub(1)) {
                if owners[r * t.ncols() + c] == (r, c) {
                    let len = t.cell(r, c).map_or(0, |x| x.text.len());
                    v.push(Target { story: ts.story, cell: Some(CellAddr { table: ts.table, row: r, col: c }), range: 0..len });
                }
            }
        }
        return v;
    }
    if let Some(t) = st.selection.text {
        return vec![Target { story: t.story, cell: t.cell, range: t.range() }];
    }
    let mut v: Vec<Target> = Vec::new();
    for id in &st.selection.items {
        if let Some(tf) = st.doc.item(*id).and_then(|i| i.text_frame()) {
            let len = st.doc.story(tf.story).map(|x| x.len()).unwrap_or(0);
            if !v.iter().any(|t| t.story == tf.story) {
                v.push(Target { story: tf.story, cell: None, range: 0..len });
            }
        }
    }
    v
}

pub(crate) fn format_chars(s: &mut Session, attrs: &Value) -> Result<Value> {
    let mut a = CharAttrs::default();
    if let Some(o) = attrs.as_object() {
        for (k, v) in o {
            a.set_json(k, v).map_err(|e| bad("type.char", e))?;
        }
    }
    let cleared: Vec<String> = attrs.as_object().map(|o| o.iter().filter(|(_, v)| v.is_null()).map(|(k, _)| k.clone()).collect()).unwrap_or_default();
    let targets = format_targets(s);
    s.edit(|d, _| {
        for t in &targets {
            let r = &t.range;
            let Some(st) = d.text_story_mut(t.story, t.cell) else { continue };
            if r.is_empty() {
                // Caret: change the typing format (the empty run at the caret).
                let pos = r.start;
                if st.is_empty() {
                    st.chars[0].format.over.merge(&a);
                    st.rev += 1;
                } else {
                    let _ = pos;
                }
                continue;
            }
            st.format_chars(r.clone(), |f| {
                f.over.merge(&a);
                for k in &cleared {
                    let _ = f.over.set_json(k, &Value::Null);
                }
            });
        }
        ok()
    })
}

pub(crate) fn format_paras(s: &mut Session, attrs: &Value) -> Result<Value> {
    let mut a = ParaAttrs::default();
    if let Some(o) = attrs.as_object() {
        for (k, v) in o {
            a.set_json(k, v).map_err(|e| bad("type.para", e))?;
        }
    }
    // `null` removes the override (back to the style's value).
    let cleared: Vec<String> = attrs.as_object().map(|o| o.iter().filter(|(_, v)| v.is_null()).map(|(k, _)| k.clone()).collect()).unwrap_or_default();
    let targets = format_targets(s);
    s.edit(|d, _| {
        for t in &targets {
            if let Some(st) = d.text_story_mut(t.story, t.cell) {
                st.format_paras(t.range.clone(), |p| {
                    p.para.merge(&a);
                    for k in &cleared {
                        let _ = p.para.set_json(k, &Value::Null);
                    }
                });
            }
        }
        ok()
    })
}

fn step_size(s: &mut Session, delta: f64) -> Result<Value> {
    let cur = selection_attrs(s, &json!({}))?;
    let size = cur["chars"]["size"].as_f64().unwrap_or(12.0);
    format_chars(s, &json!({"size": (size + delta).clamp(0.1, 1296.0)}))
}

fn break_link(s: &mut Session, p: &Value) -> Result<Value> {
    use designcraft_doc::{CharFormat, CharProps, ParaProps};
    let para = match p.get("kind").and_then(Value::as_str).unwrap_or("paragraph") {
        "paragraph" => true,
        "character" => false,
        k => return Err(bad("style.breakLink", format!("unknown kind `{k}`"))),
    };
    let targets = format_targets(s);
    s.edit(|d, _| {
        let styles = d.styles.clone();
        for t in &targets {
            let Some(st) = d.text_story_mut(t.story, t.cell) else { continue };
            let r = t.range.clone();
            if para {
                st.format_paras(r, |pf| {
                    let (pp, pc) = styles.resolve_para(pf);
                    pf.style = designcraft_doc::styles::NO_PARA_STYLE.into();
                    pf.para = ParaProps::default().delta(&pp);
                    pf.chars = CharProps::default().delta(&pc);
                });
            } else {
                let ranges = st.para_ranges();
                for (pi, pr) in ranges.into_iter().enumerate() {
                    let (a, b) = (pr.start.max(r.start), pr.end.min(r.end));
                    if a >= b {
                        continue;
                    }
                    let (_, pc) = styles.resolve_para(&st.paras[pi]);
                    st.format_chars(a..b, |f| {
                        let cp = styles.resolve_char(&pc, f);
                        *f = CharFormat { style: designcraft_doc::story::NO_CHAR_STYLE.into(), over: pc.delta(&cp) };
                    });
                }
            }
        }
        ok()
    })
}

fn open_type(s: &mut Session, p: &Value) -> Result<Value> {
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

fn selection_attrs(s: &mut Session, _p: &Value) -> Result<Value> {
    let st = s.doc()?;
    let targets = format_targets(s);
    let Some(Target { story: sid, cell, range: r }) = targets.first().cloned() else { return Ok(Value::Null) };
    let story = st.doc.text_story(sid, cell).ok_or(designcraft_doc::DocError::NoStory(sid))?;
    let pi = story.para_at(r.start);
    let pf = &story.paras[pi];
    let (pp, base) = st.doc.styles.resolve_para(pf);
    let cf = if r.is_empty() { story.char_format_at(r.start) } else { story.format_after(r.start) };
    let cp = st.doc.styles.resolve_char(&base, cf);
    Ok(json!({"story": sid.0, "paragraphStyle": pf.style, "characterStyle": cf.style, "para": pp, "chars": cp,
        "paraOverrides": pf.para.count(), "charOverrides": cf.over.count()}))
}

/// Our own filler text (not Adobe's).
pub const PLACEHOLDER: &str = "Ovid ellum quisque arcet velut tempora sint, ne veriora pareant laudemque fieri. Lorem novum strata ponet amicos, \
et tamen uti ferrum caelo tempus mollit. Quisque sinat oppida retro, nec tenuere longas animi semper vias. \
Sic erat, ut nostris ceperunt pectora verbis, mitescunt flamma solidos campos venti. Moventur iam nubes, aequora pontus, \
dum sidera fulgent tacito per inane meatu. Arbore sub magna ludunt pueri, linquunt aurea litora fluctus. \
Iamque tibi rursus ponet nova carmina vates, et longas umbras quaerit sub tegmine fagi. Omnia mutantur, nihil interit.";

fn fill_placeholder(s: &mut Session, p: &Value) -> Result<Value> {
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
    // Fill until overset: repeat the filler sentences.
    let mut text = String::new();
    let sentences: Vec<&str> = PLACEHOLDER.split_inclusive(". ").collect();
    let doc = s.doc()?.doc.clone();
    let mut d2 = (*doc).clone();
    for i in 0..400 {
        text.push_str(sentences[i % sentences.len()]);
        if i % 3 == 2 {
            d2.set_story_text(sid, &text)?;
            let cs = compose::compose_story(&d2, sid, &compose::ComposeOptions::default());
            if cs.is_overset() {
                // Trim to the last sentence that fits.
                let cut = cs.overset_at.unwrap_or(text.len());
                let keep = text[..cut.min(text.len())].rfind(". ").map(|k| k + 1).unwrap_or(cut);
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

pub(crate) fn format_targets_pub(s: &Session) -> Vec<Target> {
    format_targets(s)
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
        let m = match case {
            "upper" => up(c),
            "lower" => low(c),
            "title" => {
                if word_start {
                    up(c)
                } else {
                    low(c)
                }
            }
            _ => {
                if sentence_start && c.is_alphanumeric() {
                    up(c)
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

fn change_case(s: &mut Session, p: &Value) -> Result<Value> {
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
        // Offsets inside 漢 (2..5) and 🙂 (5..9) snap back to the character starts: 漢 is replaced.
        s.execute("story.replaceRange", &json!({"story": sid, "start": 3, "end": 6, "text": "字"})).unwrap();
        let story = designcraft_doc::StoryId(sid.as_u64().unwrap());
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
}

#[cfg(test)]
mod nested_style_tests {
    use serde_json::json;

    use crate::Session;

    #[test]
    fn nested_and_grep_styles_colour_the_text() {
        let mut s = Session::new();
        s.execute("file.new", &json!({})).unwrap();
        s.execute("style.character.create", &json!({"name": "Lead", "chars": {"fill": "C=100 M=0 Y=0 K=0"}})).unwrap();
        s.execute("style.character.create", &json!({"name": "Num", "chars": {"fill": "C=0 M=100 Y=0 K=0"}})).unwrap();
        let r = s.execute("frame.create", &json!({"rect": [72, 72, 400, 200], "content": "text", "text": "Opening words then 2026 here"})).unwrap();
        let sid = r["story"].as_u64().unwrap();
        s.execute("text.select", &json!({"story": sid, "anchor": 0, "focus": 3})).unwrap();
        s.execute(
            "type.para",
            &json!({"attrs": {
                "nestedStyles": [{"style": "Lead", "through": true, "count": 2, "until": {"kind": "words"}}],
                "grepStyles": [{"style": "Num", "pattern": "\\d+"}]
            }}),
        )
        .unwrap();
        let d = s.doc().unwrap().doc.clone();
        let cs = designcraft_compose::compose_story(&d, designcraft_doc::StoryId(sid), &Default::default());
        let text = &d.story(designcraft_doc::StoryId(sid)).unwrap().text;
        let fill_at = |byte: usize| {
            let g = cs.frames[0].lines.iter().flat_map(|l| &l.glyphs).find(|g| g.byte == byte).unwrap();
            cs.styles[g.style as usize].fill.clone()
        };
        assert_eq!(fill_at(0), "C=100 M=0 Y=0 K=0", "nested: first two words");
        assert_eq!(fill_at(text.find("words").unwrap()), "C=100 M=0 Y=0 K=0");
        assert_eq!(fill_at(text.find("then").unwrap()), "[Black]");
        assert_eq!(fill_at(text.find("2026").unwrap()), "C=0 M=100 Y=0 K=0", "GREP: digits");
    }
}

#[cfg(test)]
mod drag_text_tests {
    use serde_json::json;

    use crate::Session;

    #[test]
    fn drag_selected_text_to_move_or_copy_it() {
        let mut s = Session::new();
        s.execute("file.new", &json!({})).unwrap();
        let r = s.execute("frame.create", &json!({"rect": [72, 72, 500, 200], "content": "text", "text": "Hello brave world"})).unwrap();
        let (fid, sid) = (r["id"].as_u64().unwrap(), r["story"].as_u64().unwrap());
        // Spread point of the glyph at byte `b`.
        let at = |s: &Session, b: usize| {
            let d = &s.doc().unwrap().doc;
            let cs = designcraft_compose::compose_story(d, designcraft_doc::StoryId(sid), &Default::default());
            let l = &cs.frames[0].lines[0];
            let g = l.glyphs.iter().find(|g| g.byte == b).unwrap();
            json!([g.x + g.adv * 0.25, l.baseline - 3.0])
        };
        let end = |s: &Session| {
            let d = &s.doc().unwrap().doc;
            let cs = designcraft_compose::compose_story(d, designcraft_doc::StoryId(sid), &Default::default());
            let l = &cs.frames[0].lines[0];
            json!([l.end_x + 2.0, l.baseline - 3.0])
        };
        s.execute("text.select", &json!({"story": sid, "anchor": 6, "focus": 12})).unwrap();
        let r = s.execute("text.placeCaret", &json!({"frame": fid, "point": at(&s, 8)})).unwrap();
        assert_eq!(r["dragging"], true);
        s.execute("text.extendTo", &json!({"frame": fid, "point": end(&s)})).unwrap();
        s.execute("text.release", &json!({"frame": fid, "point": end(&s), "moved": true})).unwrap();
        let text = |s: &Session| s.doc().unwrap().doc.story(designcraft_doc::StoryId(sid)).unwrap().text.clone();
        assert_eq!(text(&s), "Hello worldbrave ");
        // Alt-drag copies (the moved text is still selected).
        s.execute("text.placeCaret", &json!({"frame": fid, "point": at(&s, 12)})).unwrap();
        s.execute("text.release", &json!({"frame": fid, "point": at(&s, 0), "moved": true, "copy": true})).unwrap();
        assert_eq!(text(&s), "brave Hello worldbrave ");
        // A click inside the selection just places the caret.
        s.execute("text.placeCaret", &json!({"frame": fid, "point": at(&s, 2)})).unwrap();
        s.execute("text.release", &json!({"frame": fid, "point": at(&s, 2), "moved": false})).unwrap();
        let t = s.doc().unwrap().selection.text.unwrap();
        assert_eq!(t.anchor, t.focus);
    }
}

#[cfg(test)]
mod nested_line_style_tests {
    use serde_json::json;

    use crate::Session;

    #[test]
    fn first_line_takes_the_nested_line_style() {
        let mut s = Session::new();
        s.execute("file.new", &json!({})).unwrap();
        s.execute("style.character.create", &json!({"name": "Lead", "chars": {"size": 18}})).unwrap();
        let text = "The opening line of this paragraph reads larger, and the lines after it return to the body size again.";
        let r = s.execute("frame.create", &json!({"rect": [72, 72, 300, 400], "content": "text", "text": text})).unwrap();
        s.execute("text.select", &json!({"story": r["story"], "anchor": 0, "focus": 0})).unwrap();
        s.execute("type.para", &json!({"nestedLineStyles": [{"style": "Lead", "lines": 1}]})).unwrap();
        let sid = designcraft_doc::StoryId(r["story"].as_u64().unwrap());
        let cs = s.cache.get(&s.doc().unwrap().doc, sid, None);
        let lines = &cs.frames[0].lines;
        assert!(lines.len() >= 3);
        let size_of = |l: usize| cs.styles[lines[l].glyphs.iter().find(|g| g.visible).unwrap().style as usize].size;
        assert_eq!(size_of(0), 18.0, "the first line");
        assert!(
            lines[0].glyphs.iter().filter(|g| g.visible && g.adv > 0.0).all(|g| cs.styles[g.style as usize].size == 18.0),
            "the whole first line"
        );
        assert_eq!(size_of(1), 12.0, "the next lines");
        let back = designcraft_idml::import_idml(&designcraft_idml::export_idml(&s.doc().unwrap().doc)).unwrap();
        let st = back.stories.values().find(|st| st.text.starts_with("The opening")).unwrap();
        let nl = st.paras[0].para.nested_line_styles.clone().unwrap_or_default();
        assert_eq!((nl.len(), nl.first().map(|n| n.lines)), (1, Some(1)), "IDML AllNestedLineStyles");
        assert_eq!(nl[0].style, "Lead");
    }
}

#[cfg(test)]
mod border_tests {
    use serde_json::json;

    use crate::Session;

    #[test]
    fn paragraph_border_and_shading_follow_each_column() {
        let mut s = Session::new();
        s.execute("file.new", &json!({})).unwrap();
        let text = "word ".repeat(80);
        let r = s.execute("frame.create", &json!({"rect": [72, 72, 400, 160], "content": "text", "text": text, "caret": false})).unwrap();
        s.execute("object.textFrameOptions", &json!({"ids": [r["id"]], "columns": 2})).unwrap();
        s.execute("text.select", &json!({"story": r["story"], "anchor": 0, "focus": 0})).unwrap();
        s.execute("type.para", &json!({"borderOn": true, "borderWeights": [2, 1, 2, 1], "borderOffsets": [4, 4, 4, 4], "borderColor": "[Black]", "shadingOn": true, "shadingOffsets": [4, 4, 4, 4]})).unwrap();
        let sid = designcraft_doc::StoryId(r["story"].as_u64().unwrap());
        let cs = s.cache.get(&s.doc().unwrap().doc, sid, None);
        let decos = &cs.frames[0].decos;
        let shading: Vec<_> = decos.iter().filter(|d| d.tint < 0.5).collect();
        assert_eq!(shading.len(), 2, "one shaded box per column");
        let edges = decos.len() - shading.len();
        assert_eq!(edges, 6, "top on the first part, bottom on the last, sides on both");
        let top = decos.iter().filter(|d| d.tint >= 0.5).map(|d| d.rect).fold(f64::MAX, |a, r| a.min(r.y0));
        let first_line = &cs.frames[0].lines[0];
        assert!((top - (first_line.baseline - first_line.ascent - 4.0 - 2.0)).abs() < 1e-6, "offset 4 + weight 2 above the text");
        let back = designcraft_idml::import_idml(&designcraft_idml::export_idml(&s.doc().unwrap().doc)).unwrap();
        let p = &back.stories.values().find(|st| st.text.starts_with("word")).unwrap().paras[0].para;
        assert_eq!(
            (p.border_on, p.border_weights, p.shading_offsets),
            (Some(true), Some([2.0, 1.0, 2.0, 1.0]), Some([4.0; 4])),
            "IDML paragraph border"
        );
    }
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
}

#[cfg(test)]
mod tcy_tests {
    use serde_json::json;

    use crate::Session;

    #[test]
    fn tate_chu_yoko_toggles_on_the_selection() {
        let mut s = Session::new();
        s.execute("file.new", &json!({})).unwrap();
        let r = s.execute("frame.create", &json!({"rect": [72, 72, 200, 400], "content": "text", "text": "令和12年", "vertical": true})).unwrap();
        let sid = r["story"].as_u64().unwrap();
        let at = "令和".len();
        s.execute("text.select", &json!({"story": sid, "anchor": at, "focus": at + 2})).unwrap();
        let get = |s: &Session| s.doc().unwrap().doc.stories[&designcraft_doc::StoryId(sid)].char_format_at(at + 1).over.tate_chu_yoko;
        s.execute("type.tateChuYoko", &json!({})).unwrap();
        assert_eq!(get(&s), Some(true));
        s.execute("type.tateChuYoko", &json!({})).unwrap();
        assert_eq!(get(&s), Some(false));
        s.execute("type.tateChuYoko", &json!({"on": true})).unwrap();
        assert_eq!(get(&s), Some(true));
    }

    #[test]
    fn ruby_and_kenten_on_the_selection() {
        let mut s = Session::new();
        s.execute("file.new", &json!({})).unwrap();
        let r = s.execute("frame.create", &json!({"rect": [72, 72, 400, 200], "content": "text", "text": "漢字です"})).unwrap();
        let sid = r["story"].as_u64().unwrap();
        s.execute("text.select", &json!({"story": sid, "anchor": 0, "focus": "漢字".len()})).unwrap();
        s.execute("type.ruby", &json!({"text": "かんじ"})).unwrap();
        s.execute("type.kenten", &json!({})).unwrap();
        let f = |s: &Session| s.doc().unwrap().doc.stories[&designcraft_doc::StoryId(sid)].char_format_at(1).over.clone();
        assert_eq!(f(&s).ruby.as_deref(), Some("かんじ"));
        assert_eq!(f(&s).kenten, Some(true));
        s.execute("type.kenten", &json!({})).unwrap();
        s.execute("type.ruby", &json!({"text": ""})).unwrap();
        assert_eq!(f(&s).kenten, Some(false));
        assert_eq!(f(&s).ruby.as_deref(), Some(""));
    }
}

#[cfg(test)]
mod bidi_caret_tests {
    use serde_json::json;

    use crate::Session;

    #[test]
    fn arrow_keys_in_a_right_to_left_paragraph() {
        let mut s = Session::new();
        s.execute("file.new", &json!({})).unwrap();
        let r = s.execute("frame.create", &json!({"rect": [72, 72, 400, 200], "content": "text", "text": "سلام عليكم"})).unwrap();
        let sid = r["story"].as_u64().unwrap();
        s.execute("type.para", &json!({"attrs": {"direction": "rightToLeft"}})).unwrap();
        s.execute("text.select", &json!({"story": sid, "anchor": 0, "focus": 0})).unwrap();
        // Left moves forward through right-to-left text, Right back.
        let p = s.execute("text.move", &json!({"dir": "left"})).unwrap()["pos"].as_u64().unwrap();
        assert_eq!(p, 'س'.len_utf8() as u64);
        let p = s.execute("text.move", &json!({"dir": "right"})).unwrap()["pos"].as_u64().unwrap();
        assert_eq!(p, 0);
        // By words too.
        let p = s.execute("text.move", &json!({"dir": "left", "word": true})).unwrap()["pos"].as_u64().unwrap();
        assert!(p >= "سلام".len() as u64, "{p}");
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
