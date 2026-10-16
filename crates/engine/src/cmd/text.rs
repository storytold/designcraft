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
        cmd!(noundo "text.selectWord", "Select Word", [], None, "{frame, point} — the word (or run of spaces) under the point", has_doc, |s, p| {
            select_at(s, p, "word")
        }),
        cmd!(
            noundo "text.selectAt",
            "Select Text Unit",
            [],
            None,
            "{frame, point, unit: word|line|paragraph|story} — what a double, triple, quadruple or quintuple click selects: the word (or run of spaces) under the point, its line, its paragraph with the paragraph return, or the whole story",
            has_doc,
            |s, p| select_at(s, p, str_param(p, "unit").unwrap_or("word"))
        ),
        cmd!(noundo "text.selectSentence", "Select Sentence", [], None, "{frame, point} — the sentence under the point", has_doc, |s, p| {
            select_around(s, p, "text.selectSentence", sentence_bounds)
        }),
        cmd!(noundo "text.selectParagraph", "Select Paragraph", [], None, "{frame, point} — the paragraph under the point, without its paragraph return", has_doc, |s, p| {
            select_around(s, p, "text.selectParagraph", para_bounds)
        }),
        cmd!(noundo "text.selectStory", "Select Story", [], None, "{frame, point} — all the text of the story under the point, threaded frames included", has_doc, |s, p| {
            select_around(s, p, "text.selectStory", |t, _| (0, t.len()))
        }),
        cmd!(noundo "text.select", "Select Text", [], None, "{story, anchor, focus} (UTF-8 byte offsets into the story text, as find.find reports them: á or — counts 2 or 3)", has_doc, |s, p| {
            let sid = StoryId(p.get("story").and_then(Value::as_u64).unwrap_or(0));
            let st = s.doc_mut()?;
            let text = &st.doc.story(sid).ok_or_else(|| bad("text.select", "no such story"))?.text;
            let a = floor_char_boundary(text, p.get("anchor").and_then(Value::as_u64).unwrap_or(0) as usize);
            let f = floor_char_boundary(text, p.get("focus").and_then(Value::as_u64).map(|v| v as usize).unwrap_or(a));
            st.selection = Selection::text(TextSel { story: sid, anchor: a, focus: f, frame: None, cell: None });
            st.revision += 1;
            s.typing_format = None;
            ok()
        }),
        cmd!("text.insert", "Type", [], None, "{text, raw?: bool (no typographer's quotes)} — replaces the selected text", has_text, insert),
        // Neither journalled nor undoable: what is composed is a preview until it's typed.
        CommandSpec {
            id: "text.compose",
            label: "Compose Text",
            menu: &[],
            shortcut: None,
            params: "{text} — input-method text being composed (a dead key's accent), shown at the caret in the text's format until text.insert types the result; \"\" drops it",
            enabled: has_doc,
            run: compose_text,
            journal: false,
            undoable: false,
        },
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
            let text = super::text_param(p, "text");
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
                let text = super::text_param(p, "text");
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
        cmd!(query "story.get", "Get Story", [], None, "{story? | frame?} → text, frames, paragraphs, vertical, overset", has_doc, |s, p| {
            let st = s.doc()?;
            let sid = story_of(s, p).ok_or_else(|| bad("story.get", "no story"))?;
            let mut v = st.doc.story_summary(sid).ok_or(designcraft_doc::DocError::NoStory(sid))?;
            let cs = s.cache.get(&st.doc, sid, None);
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
            "{attrs: {align?, leftIndent?, firstLineIndent?, spaceBefore?, spaceAfter?, dropCapLines?, dropCapChars?, dropCapStyle?, dropCapAlignLeft?, dropCapScaleDescenders?, hyphenate?: bool (default true), composer?, tabs?, ruleAbove?/ruleBelow?: {on?, weight?, color?, tint?, columnWidth?, offset?, leftIndent?, rightIndent?} (only the named rule fields change), …}}",
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
        cmd!(
            "type.bold",
            "Bold",
            [],
            Some("Cmd+Shift+B"),
            "{on?: bool} — the family's bold style, keeping italic (style linking); toggles when `on` is left out",
            has_text_or_frames,
            |s, p| link_style(s, p, true)
        ),
        cmd!(
            "type.italic",
            "Italic",
            [],
            Some("Cmd+Shift+I"),
            "{on?: bool} — the family's italic style, keeping the weight (style linking); toggles when `on` is left out",
            has_text_or_frames,
            |s, p| link_style(s, p, false)
        ),
        cmd!(
            "type.swapFillStroke",
            "Swap Text Fill and Stroke",
            [],
            None,
            "{} — swap each selected character's resolved fill/stroke and tints; at a caret, swap the next typing's colors",
            has_text_or_frames,
            swap_fill_stroke
        ),
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
        cmd!(
            "type.warichu",
            "Warichu",
            [],
            None,
            "{on?: bool, lines?: int, size?: number, lineSpacing?: number, align?: auto|left|center|right|justify, charsBeforeBreak?: int, charsAfterBreak?: int} — set the selection as an inline note in smaller lines inside one line height (toggles by default)",
            has_text_or_frames,
            warichu_cmd
        ),
        cmd!(
            "type.storyDirection",
            "Story Direction",
            [],
            None,
            "{vertical: bool} — the stories of the text selection or the selected frames: vertical lines run top to bottom and follow each other right to left, in every frame of the thread",
            has_text_or_frames,
            story_direction
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
        cmd!(query "type.selectionAttrs", "Selection Attributes", [], None, "{} → resolved character/paragraph attributes at the text selection, and its start as `selectedText` (one line, up to 60 characters)", has_doc, selection_attrs),
        cmd!(query "type.openTypeFeatures", "OpenType Features", [], None, "{} → the OpenType features and stylistic sets the selected text's fonts offer, each with its applied and available state (none, some or all of the selection)", has_doc, open_type_features),
        cmd!(
            "type.feature",
            "Set OpenType Features",
            [],
            None,
            "{features: {tag: true|false, …}} — turns OpenType features on or off across the selected text, keeping each run's other features",
            has_doc,
            set_features
        ),
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
    hit_byte_with(s, frame, pt, compose::hit)
}

/// [`hit_byte`] with the hit test: [`compose::hit`] (caret position) or [`compose::hit_char`]
/// (the character under the point).
fn hit_byte_with(
    s: &Session,
    frame: ItemId,
    pt: Point,
    hit: fn(&compose::ComposedStory, usize, Point) -> Option<usize>,
) -> Option<(StoryId, usize, Option<CellAddr>)> {
    let st = s.active()?;
    let loc = st.doc.find(frame)?;
    let it = st.doc.item_at(&loc)?;
    let sid = it.text_frame()?.story;
    let xf = st.doc.parent_xf(&loc) * st.doc.text_xf(it);
    let inner = xf.inverse() * pt;
    let cs = s.cache.get(&st.doc, sid, None);
    let fi = cs.frames.iter().position(|f| f.frame == frame)?;
    // Map path-text hits along the curve before choosing the character unit.
    let inner = cs.frames.get(fi).and_then(|ft| compose::PathTextMap::new(it, ft)).map_or(inner, |m| m.to_line(inner));
    if let Some((table, row, col, b)) = compose::hit_cell_with(&cs, fi, inner, hit) {
        return Some((sid, b, Some(CellAddr { table, row, col })));
    }
    if let Some((id, b)) = compose::hit_note_with(&cs, fi, inner, hit) {
        return Some((sid, b, Some(CellAddr::footnote(id))));
    }
    let b = hit(&cs, fi, inner).unwrap_or(0);
    Some((sid, b, None))
}

/// Select the word, line, paragraph or story at `point` in `frame` (multiple clicks).
fn select_at(s: &mut Session, p: &Value, unit: &str) -> Result<Value> {
    const ID: &str = "text.selectAt";
    let frame = id_param(p, "frame").ok_or_else(|| bad(ID, "missing frame"))?;
    let pt = point_param(p, "point").ok_or_else(|| bad(ID, "missing point"))?;
    let (sid, b, cell) = hit_byte_with(s, frame, pt, compose::hit_char).ok_or_else(|| bad(ID, "not a text frame"))?;
    let st = s.doc()?;
    let story = st.doc.text_story(sid, cell).ok_or(designcraft_doc::DocError::NoStory(sid))?;
    let text = story.text.as_str();
    let b = floor_char_boundary(text, b);
    let (a, e) = match unit {
        "word" => word_or_spaces_at(text, b),
        "line" => {
            let cs = s.cache.get(&st.doc, sid, None);
            line_at(&text_composition(cs, cell), b).unwrap_or((b, b))
        }
        // The paragraph with its return (the last paragraph has none).
        "paragraph" => story.para_ranges().get(story.para_at(b)).map_or((b, b), |r| (r.start, (r.end + 1).min(text.len()))),
        "story" => (0, text.len()),
        other => return Err(bad(ID, format!("unknown unit `{other}` (word, line, paragraph or story)"))),
    };
    s.text_drag = false;
    s.typing_format = None;
    let st = s.doc_mut()?;
    st.selection = Selection::text(TextSel { story: sid, anchor: a, focus: e, frame: Some(frame), cell });
    // Explicit reselection invalidates the Find scope even when the range is unchanged.
    st.revision = st.revision.wrapping_add(1);
    Ok(json!({"story": sid.0, "anchor": a, "focus": e}))
}

/// The word at byte `i`; on spaces the run of spaces; on anything else that one character
/// (grapheme cluster). At the end of a paragraph, what ends it. Grapheme clusters are never split:
/// combining marks, viramas, variation selectors and emoji ZWJ sequences stay with their base.
fn word_or_spaces_at(s: &str, i: usize) -> (usize, usize) {
    use unicode_segmentation::UnicodeSegmentation;
    let i = floor_char_boundary(s, i.min(s.len()));
    // Clusters never span a paragraph return, so the paragraph is enough.
    let ps = s[..i].rfind('\n').map_or(0, |k| k + 1);
    let pe = s[i..].find('\n').map_or(s.len(), |k| i + k);
    let g: Vec<(usize, &str)> = s[ps..pe].grapheme_indices(true).collect();
    let Some(k) = g.iter().rposition(|(o, _)| ps + o <= i) else { return (i, i) };
    let kind = |c: &str| match c.chars().next() {
        Some(c) if is_word(c) => 1,
        Some(c) if c.is_whitespace() => 2,
        _ => 0,
    };
    let span = |a: usize, e: usize| g.get(a).zip(g.get(e)).map_or((i, i), |((oa, _), (oe, ge))| (ps + oa, ps + oe + ge.len()));
    let kd = g.get(k).map_or(0, |(_, c)| kind(c));
    if kd == 0 {
        return span(k, k);
    }
    let mut a = k;
    while a > 0 && g.get(a - 1).is_some_and(|(_, c)| kind(c) == kd) {
        a -= 1;
    }
    let mut e = k;
    while g.get(e + 1).is_some_and(|(_, c)| kind(c) == kd) {
        e += 1;
    }
    span(a, e)
}

/// The composition whose lines hold a story's text: the story's own, a table cell's or a footnote's.
fn text_composition(cs: std::sync::Arc<compose::ComposedStory>, cell: Option<CellAddr>) -> std::sync::Arc<compose::ComposedStory> {
    match cell {
        Some(c) if c.footnote_id().is_some() => compose::find_note(&cs, c.row as u64).map(|(_, n)| n.text.clone()).unwrap_or(cs),
        Some(c) => compose::find_cell(&cs, c.table, c.row, c.col).map(|(_, _, pc)| pc.text.clone()).unwrap_or(cs),
        None => cs,
    }
}

/// The composed line holding the character at byte `pos`.
fn line_at(cs: &compose::ComposedStory, pos: usize) -> Option<(usize, usize)> {
    let lines = || cs.frames.iter().flat_map(|f| f.lines.iter());
    lines().find(|l| l.range.contains(&pos)).map(|l| (l.range.start, l.range.end)).or_else(|| line_of(cs, pos))
}

fn place(s: &mut Session, p: &Value, extend: bool) -> Result<Value> {
    let frame = id_param(p, "frame").ok_or_else(|| bad("text.placeCaret", "missing frame"))?;
    let pt = point_param(p, "point").unwrap_or(Point::ZERO);
    // Empty frames become text frames; a locked one stays as it is.
    let st = s.doc()?;
    let locked = st.doc.item(frame).is_some_and(|i| i.locked || st.doc.layer(i.layer).is_some_and(|l| l.locked));
    let needs_convert = st.doc.item(frame).is_some_and(|i| matches!(i.content, Content::Unassigned));
    if needs_convert && locked {
        return Err(bad("text.placeCaret", "the frame is locked"));
    }
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
    s.typing_format = None;
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

/// `text.compose`: show input-method text at the caret, typed in the format the next
/// character would get, without making it part of the story. Replaces what was composed before.
fn compose_text(s: &mut Session, p: &Value) -> Result<Value> {
    let text = super::text_param(p, "text");
    s.end_composition();
    if text.is_empty() {
        return ok();
    }
    has_text(s).map_err(|e| bad("text.compose", &e))?;
    // The preview types the text; the format chosen at the caret is still for the real typing.
    let typing = s.typing_format.clone();
    s.begin_interaction("Type")?;
    let r = s.preview("text.insert", json!({"text": text, "raw": true}));
    s.typing_format = typing;
    if let Err(e) = r {
        s.cancel_interaction();
        return Err(e);
    }
    let uid = s.doc()?.uid;
    s.composition = Some((uid, text.len()));
    let pos = s.doc()?.selection.text.map_or(0, |t| t.focus);
    Ok(json!({"range": [pos.saturating_sub(text.len()), pos]}))
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
            let at = super::changes::mark_deleted(st, r.clone());
            let mut fmt = st.char_format_at(at).clone();
            if let Some(f) = &typing {
                apply_typing_format(&mut fmt, f);
            }
            fmt.over.change = Some(designcraft_doc::ChangeMark::Inserted);
            st.insert_with(at, &text, fmt);
            let pos = at + text.len() - trail;
            sel.text = Some(TextSel { anchor: pos, focus: pos, ..t });
            return Ok(json!({"pos": pos}));
        }
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
    if result.is_ok() {
        s.typing_format = None;
    }
    result
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
        let r = with_break_pair(&st.text, r);
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
/// Selects `bounds(text, byte)` around the byte under `point` in `frame`. Unlike a caret
/// placement, a press inside the current selection is no start of a text drag here: it is the
/// third or later click of a row, which lands in the word or sentence just selected.
fn select_around(s: &mut Session, p: &Value, id: &str, bounds: fn(&str, usize) -> (usize, usize)) -> Result<Value> {
    let frame = id_param(p, "frame").ok_or_else(|| bad(id, "missing frame"))?;
    let pt = point_param(p, "point").unwrap_or(Point::ZERO);
    let (sid, b, cell) = hit_byte(s, frame, pt).ok_or_else(|| bad(id, "not a text frame"))?;
    let st = s.doc_mut()?;
    let text = st.doc.text_story(sid, cell).map(|x| x.text.clone()).unwrap_or_default();
    let (anchor, focus) = bounds(&text, floor_char_boundary(&text, b));
    st.selection = Selection::text(TextSel { story: sid, anchor, focus, frame: Some(frame), cell });
    st.revision += 1;
    Ok(json!({"story": sid.0, "anchor": anchor, "focus": focus}))
}

/// The paragraph around byte `i`, without its `\n`.
fn para_bounds(s: &str, i: usize) -> (usize, usize) {
    let i = i.min(s.len());
    let a = s.get(..i).and_then(|t| t.rfind('\n')).map_or(0, |k| k + 1);
    let b = s.get(i..).and_then(|t| t.find('\n')).map_or(s.len(), |k| i + k);
    (a, b)
}

/// The sentence around byte `i`, within its paragraph: it ends after `.`, `!`, `?` or `…` (and
/// any closing quotes or brackets) followed by a space or the paragraph end, or after a CJK full
/// stop, and takes the spaces after it, as deleting a sentence should leave none behind.
fn sentence_bounds(s: &str, i: usize) -> (usize, usize) {
    let (pa, pb) = para_bounds(s, i);
    let para = s.get(pa..pb).unwrap_or("");
    let mut start = 0;
    let mut it = para.char_indices().peekable();
    while let Some((_, c)) = it.next() {
        let cjk = matches!(c, '。' | '！' | '？');
        if !cjk && !matches!(c, '.' | '!' | '?' | '…') {
            continue;
        }
        while it.peek().is_some_and(|&(_, c)| {
            matches!(c, '.' | '!' | '?' | '…' | '。' | '！' | '？' | '"' | '\'' | '”' | '’' | '»' | '›' | ')' | ']' | '」' | '』' | '）')
        }) {
            it.next();
        }
        if !cjk && it.peek().is_some_and(|&(_, c)| !c.is_whitespace()) {
            continue;
        }
        while it.peek().is_some_and(|&(_, c)| c.is_whitespace()) {
            it.next();
        }
        let end = it.peek().map_or(para.len(), |&(k, _)| k);
        if pa + end > i || end == para.len() {
            return (pa + start, pa + end);
        }
        start = end;
    }
    (pa + start, pb)
}

/// Type ▸ Story Direction.
fn story_direction(s: &mut Session, p: &Value) -> Result<Value> {
    let vertical = p.get("vertical").and_then(Value::as_bool).ok_or_else(|| bad("type.storyDirection", "missing vertical"))?;
    let st = s.doc()?;
    let mut stories: Vec<StoryId> = st.selection.text.map(|t| t.story).into_iter().chain(st.selection.cells.map(|c| c.story)).collect();
    for id in &st.selection.items {
        if let Some(tf) = st.doc.item(*id).and_then(|i| i.text_frame())
            && !stories.contains(&tf.story)
        {
            stories.push(tf.story);
        }
    }
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
            st.rev += 1;
        }
    }
}

fn move_caret(s: &mut Session, p: &Value) -> Result<Value> {
    let dir = str_param(p, "dir").unwrap_or("right").to_string();
    let extend = bool_or(p, "extend", false);
    let word = bool_or(p, "word", false);
    let st = s.doc()?;
    let t = st.selection.text.ok_or_else(|| bad("text.move", "no caret"))?;
    let story = st.doc.text_story(t.story, t.cell).ok_or(designcraft_doc::DocError::NoStory(t.story))?;
    let text = story.text.clone();
    // In a cell, lines come from the cell's own composition.
    let cs = text_composition(s.cache.get(&st.doc, t.story, None), t.cell);
    let pos = t.focus.min(text.len());
    // Vertical lines run top to bottom and follow each other right to left: Down and Up move along
    // the line (Right and Left of horizontal text), Left and Right to the next and previous line.
    let vertical_text = compose::caret(&cs, pos).and_then(|(fi, ..)| cs.frames.get(fi)).is_some_and(|f| f.vertical);
    let dir = match (vertical_text, dir.as_str()) {
        (true, "down") => "right",
        (true, "up") => "left",
        (true, "left") => "down",
        (true, "right") => "up",
        (_, d) => d,
    };
    let collapse_to = |left: bool| if left { t.range().start } else { t.range().end };
    let new = match dir {
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
    s.typing_format = None;
    let stm = s.doc_mut()?;
    stm.selection.text = Some(if extend { TextSel { focus: new, ..t } } else { TextSel { anchor: new, focus: new, ..t } });
    stm.revision += 1;
    Ok(json!({"pos": new}))
}

fn line_of(cs: &compose::ComposedStory, pos: usize) -> Option<(usize, usize)> {
    cs.frames.iter().flat_map(|f| f.lines.iter()).find(|l| pos >= l.range.start && pos <= l.range.end).map(|l| (l.range.start, l.range.end))
}

fn vertical(cs: &compose::ComposedStory, pos: usize, up: bool) -> Option<usize> {
    let (fi, x, caret_y, _, _) = compose::caret(cs, pos)?;
    let lines: Vec<(usize, &compose::Line)> = cs.frames.iter().enumerate().flat_map(|(i, f)| f.lines.iter().map(move |l| (i, l))).collect();
    // Match the line by the bytes it owns. A warichu caret sits on a small-line baseline, so it
    // does not equal the parent line's baseline.
    let cur = lines.iter().position(|(i, l)| *i == fi && pos >= l.range.start && pos <= l.range.end && (pos < l.range.end || l.last_in_para))?;
    let (_, l) = lines.get(cur)?;
    if let Some(y) = compose::adjacent_row(l, x, caret_y, up)
        && let Some(p) = compose::hit(cs, fi, Point::new(x, y))
        && p != pos
    {
        return Some(p);
    }
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
        for (r, c) in t.selected_owners(ts.range) {
            let len = t.cell(r, c).map_or(0, |x| x.text.len());
            v.push(Target { story: ts.story, cell: Some(CellAddr { table: ts.table, row: r, col: c }), range: 0..len });
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

/// Character formatting chosen at a bare caret in text: applied to what is typed there next.
#[derive(Clone, Debug)]
pub(crate) struct TypingFormat {
    doc: u64,
    at: TextSel,
    attrs: CharAttrs,
    /// Overrides removed (`null` in `type.char`).
    cleared: Vec<String>,
}

/// The typing format for the active document's caret, if it was chosen at this caret.
fn typing_format(s: &Session) -> Option<&TypingFormat> {
    let st = s.active()?;
    let t = st.selection.text?;
    s.typing_format.as_ref().filter(|f| f.doc == st.uid && f.at.story == t.story && f.at.cell == t.cell && f.at.range() == t.range())
}

fn warichu_cmd(s: &mut Session, p: &Value) -> Result<Value> {
    let has_opts = ["lines", "size", "lineSpacing", "align", "charsBeforeBreak", "charsAfterBreak"].iter().any(|k| p.get(k).is_some());
    let on = match p.get("on").and_then(Value::as_bool) {
        Some(v) => v,
        None if has_opts => true,
        None => !format_targets(s).first().is_some_and(|t| {
            s.doc()
                .ok()
                .and_then(|d| d.doc.text_story(t.story, t.cell))
                .is_some_and(|st| st.char_format_at(if t.range.is_empty() { t.range.start } else { t.range.start + 1 }).over.warichu == Some(true))
        }),
    };
    let mut body = serde_json::Map::new();
    body.insert("warichu".into(), json!(on));
    if on {
        if let Some(v) = p.get("lines").and_then(Value::as_u64) {
            body.insert("warichuLines".into(), json!(v.clamp(2, 16)));
        }
        if let Some(v) = p.get("size").and_then(Value::as_f64).filter(|v| v.is_finite()) {
            body.insert("warichuSize".into(), json!(v.clamp(1.0, 1000.0)));
        }
        if let Some(v) = p.get("lineSpacing").and_then(Value::as_f64).filter(|v| v.is_finite()) {
            body.insert("warichuLineSpacing".into(), json!(v.clamp(-10_000.0, 10_000.0)));
        }
        if let Some(v) = p.get("align").and_then(Value::as_str) {
            body.insert("warichuAlignment".into(), json!(v));
        }
        if let Some(v) = p.get("charsBeforeBreak").and_then(Value::as_u64) {
            body.insert("warichuCharsBeforeBreak".into(), json!(v.min(100)));
        }
        if let Some(v) = p.get("charsAfterBreak").and_then(Value::as_u64) {
            body.insert("warichuCharsAfterBreak".into(), json!(v.min(100)));
        }
    }
    format_chars(s, &Value::Object(body))
}

pub(crate) fn validate_char_fill(document: &designcraft_doc::Document, attrs: &CharAttrs, command_id: &str) -> Result<()> {
    if let Some(fill) = &attrs.fill
        && document.swatch(fill).is_none()
    {
        return Err(bad(
            command_id,
            format!("unknown character fill `{fill}`; create a swatch with `swatch.create` and use its returned name as `fill`"),
        ));
    }
    Ok(())
}

/// Byte `pos` is the hidden paragraph mark of an *empty* paragraph: the only character on that
/// line, and so the one a line-affecting attribute like leading can land on right away instead of
/// waiting for the next typed character (otherwise an empty line could never get its own leading).
fn empty_para_mark(story: &designcraft_doc::Story, pos: usize) -> bool {
    let ranges = story.para_ranges();
    ranges.get(story.para_at(pos)).is_some_and(|r| r.start == pos && r.end == pos) && story.text.as_bytes().get(pos) == Some(&b'\n')
}

pub(crate) fn format_chars(s: &mut Session, attrs: &Value) -> Result<Value> {
    let mut a = CharAttrs::default();
    if let Some(o) = attrs.as_object() {
        for (k, v) in o {
            a.set_json(k, v).map_err(|e| bad("type.char", e))?;
        }
    }
    validate_char_fill(&s.doc()?.doc, &a, "type.char")?;
    let cleared: Vec<String> = attrs.as_object().map(|o| o.iter().filter(|(_, v)| v.is_null()).map(|(k, _)| k.clone()).collect()).unwrap_or_default();
    let targets = format_targets(s);
    // A caret in text: the format is for the next typing — unless it sits on an empty paragraph's
    // hidden mark, which takes the format immediately (there is nothing else on that line to type
    // into first).
    if let [t] = targets.as_slice()
        && t.range.is_empty()
        && let Some(at) = s.doc()?.selection.text
        && s.doc()?.doc.text_story(t.story, t.cell).is_some_and(|st| !st.is_empty() && !empty_para_mark(st, t.range.start))
    {
        let doc = s.doc()?.uid;
        let mut f = typing_format(s).cloned().unwrap_or(TypingFormat { doc, at, attrs: CharAttrs::default(), cleared: vec![] });
        f.attrs.merge(&a);
        f.cleared.retain(|k| attrs.get(k).is_none_or(|v| v.is_null()) && !cleared.contains(k));
        f.cleared.extend(cleared);
        s.typing_format = Some(f);
        return ok();
    }
    s.edit(|d, _| {
        apply_char_attrs(d, &targets, &a, &cleared);
        ok()
    })
}

/// Character attributes `a` (and the overrides `cleared` removed) over each target's text.
pub(crate) fn apply_char_attrs(d: &mut designcraft_doc::Document, targets: &[Target], a: &CharAttrs, cleared: &[String]) {
    for t in targets {
        let r = &t.range;
        let Some(st) = d.text_story_mut(t.story, t.cell) else { continue };
        if r.is_empty() {
            if st.is_empty() {
                // An empty story: its one (empty) run is the format typing takes.
                if let Some(run) = st.chars.first_mut() {
                    run.format.over.merge(a);
                    st.rev += 1;
                }
            } else if empty_para_mark(st, r.start) {
                // An empty paragraph elsewhere in the story: its hidden paragraph mark is its only
                // character, and the one that carries line-affecting attributes like leading.
                st.format_chars(r.start..r.start + 1, |f| {
                    f.over.merge(a);
                    for k in cleared {
                        let _ = f.over.set_json(k, &Value::Null);
                    }
                });
            }
            continue;
        }
        st.format_chars(r.clone(), |f| {
            f.over.merge(a);
            for k in cleared {
                let _ = f.over.set_json(k, &Value::Null);
            }
        });
    }
}

pub(crate) fn format_paras(s: &mut Session, attrs: &Value) -> Result<Value> {
    let empty = serde_json::Map::new();
    let obj = attrs.as_object().unwrap_or(&empty);
    // A rule object changes only the rule fields it names, so each paragraph gets its own attrs
    // over its resolved rule; this pass validates them.
    let mut a = ParaAttrs::default();
    for (k, v) in obj {
        a.set_json_over(k, v, &designcraft_doc::ParaProps::default()).map_err(|e| bad("type.para", e))?;
    }
    // `null` removes the override (back to the style's value).
    let cleared: Vec<String> = obj.iter().filter(|(_, v)| v.is_null()).map(|(k, _)| k.clone()).collect();
    let targets = format_targets(s);
    s.edit(|d, _| {
        let styles = d.styles.clone();
        for t in &targets {
            if let Some(st) = d.text_story_mut(t.story, t.cell) {
                st.format_paras(t.range.clone(), |p| {
                    let (current, _) = styles.resolve_para(p);
                    let mut a = a.clone();
                    for (k, v) in obj {
                        let _ = a.set_json_over(k, v, &current);
                    }
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

/// How much of the selected text `type.selectionAttrs` reports.
const SELECTED_TEXT_CHARS: usize = 60;

fn selected_text_preview(chars: impl Iterator<Item = char>) -> String {
    chars
        .map(|c| if c.is_control() || c == designcraft_doc::OBJECT_MARK { ' ' } else { c })
        .skip_while(|c| c.is_whitespace())
        .take(SELECTED_TEXT_CHARS)
        .collect()
}

fn selection_attrs(s: &mut Session, _p: &Value) -> Result<Value> {
    let st = s.doc()?;
    let targets = format_targets(s);
    let Some(Target { story: sid, cell, range: r }) = targets.first().cloned() else { return Ok(Value::Null) };
    let story = st.doc.text_story(sid, cell).ok_or(designcraft_doc::DocError::NoStory(sid))?;
    let pi = story.para_at(r.start);
    let pf = &story.paras[pi];
    let (pp, base) = st.doc.styles.resolve_para(pf);
    let cf = effective_char_format(s, &Target { story: sid, cell, range: r.clone() })?;
    let cp = st.doc.styles.resolve_char(&base, &cf);
    let selected = selected_text_preview(story.text.get(r.start..r.end).unwrap_or_default().chars());
    Ok(json!({"story": sid.0, "paragraphStyle": pf.style, "characterStyle": cf.style, "para": pp, "chars": cp,
        "paraOverrides": pf.para.count(), "charOverrides": cf.over.count(), "selectedText": selected.trim()}))
}

/// The first target character, with any pending caret overrides. Keep character-style
/// inheritance separate from paragraph defaults for style creation from the selection.
pub(crate) fn effective_char_format(s: &Session, target: &Target) -> Result<designcraft_doc::CharFormat> {
    let story = s.doc()?.doc.text_story(target.story, target.cell).ok_or(designcraft_doc::DocError::NoStory(target.story))?;
    let mut format = if target.range.is_empty() { story.char_format_at(target.range.start) } else { story.format_after(target.range.start) }.clone();
    if let Some(pending) = typing_format(s)
        && target.story == pending.at.story
        && target.cell == pending.at.cell
        && target.range == pending.at.range()
    {
        apply_typing_format(&mut format, pending);
    }
    Ok(format)
}

/// A swatch promoted to a named color keeps any not-yet-inserted text pointing at it.
pub(crate) fn rename_typing_swatch(s: &mut Session, from: &str, to: &str) {
    let Some(uid) = s.active().map(|st| st.uid) else { return };
    if let Some(pending) = &mut s.typing_format
        && pending.doc == uid
    {
        for name in [&mut pending.attrs.fill, &mut pending.attrs.stroke, &mut pending.attrs.underline_color, &mut pending.attrs.strikethrough_color]
            .into_iter()
            .flatten()
        {
            if name == from {
                *name = to.to_string();
            }
        }
    }
}

fn swapped_colors(cp: &designcraft_doc::CharProps) -> CharAttrs {
    CharAttrs {
        fill: Some(cp.stroke.clone()),
        fill_tint: Some(cp.stroke_tint),
        stroke: Some(cp.fill.clone()),
        stroke_tint: Some(cp.fill_tint),
        ..Default::default()
    }
}

/// Resolve each paragraph/run before editing: a mixed selection must not take the first run's colors.
fn swap_fill_stroke(s: &mut Session, _p: &Value) -> Result<Value> {
    const ID: &str = "type.swapFillStroke";
    let targets = format_targets(s);
    if let [target] = targets.as_slice()
        && target.range.is_empty()
        && s.doc()?.selection.text.is_some()
    {
        let st = s.doc()?;
        let story = st.doc.text_story(target.story, target.cell).ok_or(designcraft_doc::DocError::NoStory(target.story))?;
        let pf = story.paras.get(story.para_at(target.range.start)).ok_or_else(|| bad(ID, "missing paragraph"))?;
        let (_, base) = st.doc.styles.resolve_para(pf);
        let cp = st.doc.styles.resolve_char(&base, &effective_char_format(s, target)?);
        return format_chars(s, &json!({"fill": cp.stroke, "fillTint": cp.stroke_tint, "stroke": cp.fill, "strokeTint": cp.fill_tint}));
    }
    let mut edits = Vec::new();
    let d = &s.doc()?.doc;
    for target in targets {
        let story = d.text_story(target.story, target.cell).ok_or(designcraft_doc::DocError::NoStory(target.story))?;
        if story.is_empty() {
            let pf = story.paras.first().ok_or_else(|| bad(ID, "missing paragraph"))?;
            let (_, base) = d.styles.resolve_para(pf);
            let attrs = swapped_colors(&d.styles.resolve_char(&base, story.char_format_at(0)));
            validate_char_fill(d, &attrs, ID)?;
            edits.push((target, attrs));
            continue;
        }
        let mut runs = story.runs().peekable();
        for (pi, para) in story.para_ranges().into_iter().enumerate() {
            let start = para.start.max(target.range.start);
            let end = para.end.saturating_add(1).min(story.len()).min(target.range.end);
            if start >= end {
                continue;
            }
            let pf = story.paras.get(pi).ok_or_else(|| bad(ID, "missing paragraph"))?;
            let (_, base) = d.styles.resolve_para(pf);
            while let Some((range, format)) = runs.peek() {
                if range.end <= start {
                    runs.next();
                    continue;
                }
                if range.start >= end {
                    break;
                }
                let attrs = swapped_colors(&d.styles.resolve_char(&base, format));
                validate_char_fill(d, &attrs, ID)?;
                edits.push((Target { range: start.max(range.start)..end.min(range.end), ..target.clone() }, attrs));
                if range.end > end {
                    break;
                }
                runs.next();
            }
        }
    }
    s.edit(|d, _| {
        for (target, attrs) in &edits {
            apply_char_attrs(d, std::slice::from_ref(target), attrs, &[]);
        }
        ok()
    })
}

fn apply_typing_format(cf: &mut designcraft_doc::CharFormat, f: &TypingFormat) {
    cf.over.merge(&f.attrs);
    for k in &f.cleared {
        let _ = cf.over.set_json(k, &Value::Null);
    }
}

/// Bold (`bold`) or italic from the font family (style linking), per run of the selection: on or
/// off as asked, else the opposite of the first selected character.
fn link_style(s: &mut Session, p: &Value, bold: bool) -> Result<Value> {
    let id = if bold { "type.bold" } else { "type.italic" };
    let cur = selection_attrs(s, &json!({}))?;
    let family = cur["chars"]["fontFamily"].as_str().unwrap_or_default().to_string();
    let style = cur["chars"]["fontStyle"].as_str().unwrap_or_default().to_string();
    // The document's own fonts count too.
    let db = designcraft_fonts::FontDb::global().scoped(s.doc()?.doc.font_scope);
    let (weight, italic) = db.traits_of(&family, &style);
    let on = p.get("on").and_then(Value::as_bool).unwrap_or(if bold { weight < 650.0 } else { !italic });
    let (want_bold, want_italic) = if bold { (Some(on), None) } else { (None, Some(on)) };
    let targets = format_targets(s);
    // A caret: the linked style of the format there, for the next typing.
    if let [t] = targets.as_slice()
        && t.range.is_empty()
    {
        let linked = db.linked_style(&family, &style, want_bold, want_italic).ok_or_else(|| bad(id, format!("{family} has no style for that")))?;
        format_chars(s, &json!({"fontStyle": linked}))?;
        return Ok(json!({"on": on, "fontStyle": linked}));
    }
    let mut changed = 0;
    let r = s.edit(|d, _| {
        let styles = d.styles.clone();
        for t in &targets {
            let Some(st) = d.text_story_mut(t.story, t.cell) else { continue };
            for (pi, pr) in st.para_ranges().into_iter().enumerate() {
                let (a, b) = (pr.start.max(t.range.start), pr.end.min(t.range.end));
                if a >= b {
                    continue;
                }
                let Some(pf) = st.paras.get(pi) else { continue };
                let (_, pc) = styles.resolve_para(pf);
                st.format_chars(a..b, |f| {
                    let cp = styles.resolve_char(&pc, f);
                    if let Some(linked) = db.linked_style(&cp.font_family, &cp.font_style, want_bold, want_italic)
                        && linked != cp.font_style
                    {
                        f.over.font_style = Some(linked);
                        changed += 1;
                    }
                });
            }
        }
        // No undo step for nothing.
        if changed == 0 {
            return Err(bad(id, "nothing to change"));
        }
        ok()
    });
    r?;
    Ok(json!({"on": on, "changed": changed}))
}

/// One stretch of the selection: its length in bytes, the font it is set in and its features.
struct FeatureSegment {
    len: usize,
    font: (String, String),
    applied: Vec<String>,
}

/// The OpenType features of the fonts in the selection, with how much of the selection each one is
/// applied to and how much of it the fonts offer. A caret counts as one byte. `state` is `none`,
/// `some` or `all`.
fn open_type_features(s: &mut Session, _p: &Value) -> Result<Value> {
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
            segments.push(FeatureSegment { len, font: (cp.font_family, cp.font_style), applied: cp.otf_features });
        }
    }
    if segments.is_empty() {
        let (_, base) = st.doc.styles.resolve_para_style(designcraft_doc::story::BASIC_PARAGRAPH);
        let cp = st.doc.styles.resolve_char(&base, &designcraft_doc::story::CharFormat::default());
        segments.push(FeatureSegment { len: 1, font: (cp.font_family, cp.font_style), applied: cp.otf_features });
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
            if designcraft_doc::otf::is_on(&seg.applied, tag) {
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
        let name = if names.len() == 1 { names.first().map(|n| n.to_string()) } else { None };
        let (applied, available) = measure(&tag);
        sets.push(json!({"set": set, "tag": tag, "name": name, "applied": applied, "available": available}));
    }
    let used: Vec<Value> = fonts.keys().map(|(family, style)| json!({"family": family, "style": style})).collect();
    Ok(json!({"fonts": used, "features": features, "stylisticSets": sets}))
}

/// Turns OpenType features on or off across the selected text: `features` maps tags to whether they
/// are on. Each run keeps its other features, whichever font it is set in, and the change is one
/// undo step.
fn set_features(s: &mut Session, p: &Value) -> Result<Value> {
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
        // No pending typing format was left behind: it applied immediately.
        assert!(s.typing_format.is_none());
        let d = s.doc().unwrap().doc.clone();
        let cs = designcraft_compose::compose_story(&d, sid, &Default::default());
        let blank_line = cs.frames[0].lines.iter().find(|l| l.range == (blank..blank)).unwrap();
        assert_eq!(blank_line.leading, 40.0, "the blank line's own height changed");
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
mod multi_click_tests {
    use serde_json::{Value, json};

    use crate::Session;

    #[test]
    fn moving_the_caret_drops_the_pending_typing_format() {
        let mut s = Session::new();
        s.execute("file.new", &json!({})).unwrap();
        let r = s.execute("frame.create", &json!({"rect": [72, 72, 500, 200], "content": "text", "text": "Hello world"})).unwrap();
        let (fid, sid) = (r["id"].as_u64().unwrap(), r["story"].as_u64().unwrap());
        let size_at = |s: &Session, b: usize| s.doc().unwrap().doc.story(designcraft_doc::StoryId(sid)).unwrap().char_format_at(b).over.size;
        let pending = |s: &mut Session| {
            s.execute("text.select", &json!({"story": sid, "anchor": 5})).unwrap();
            s.execute("type.char", &json!({"attrs": {"size": 30}})).unwrap();
        };
        // Away and back by arrow keys, by a click, by selecting and by double-clicking: typing at the
        // same caret is in the text's own format.
        let moves: Vec<Box<dyn Fn(&mut Session)>> = vec![
            Box::new(|s| {
                s.execute("text.move", &json!({"dir": "right"})).unwrap();
                s.execute("text.move", &json!({"dir": "left"})).unwrap();
            }),
            Box::new(|s| {
                s.execute("text.placeCaret", &json!({"frame": fid, "point": [80, 80]})).unwrap();
                s.execute("text.select", &json!({"story": sid, "anchor": 5})).unwrap();
            }),
            Box::new(|s| {
                s.execute("text.selectAt", &json!({"frame": fid, "point": [80, 80], "unit": "story"})).unwrap();
                s.execute("text.select", &json!({"story": sid, "anchor": 5})).unwrap();
            }),
        ];
        for m in &moves {
            pending(&mut s);
            m(&mut s);
            s.execute("text.insert", &json!({"text": "x"})).unwrap();
            assert_eq!(size_at(&s, 6), None);
            s.execute("edit.undo", &json!({})).unwrap();
        }
        // Without a move it applies.
        pending(&mut s);
        s.execute("text.insert", &json!({"text": "x"})).unwrap();
        assert_eq!(size_at(&s, 6), Some(30.0));
    }

    #[test]
    fn word_selection_keeps_grapheme_clusters_whole() {
        use super::word_or_spaces_at as word;
        // Decomposed "café": the combining acute belongs to the word, wherever the click lands.
        let s = "un cafe\u{301} noir";
        let cafe = (3, 3 + "cafe\u{301}".len());
        assert_eq!(word(s, 4), cafe);
        assert_eq!(word(s, 7), cafe, "on the combining mark");
        // Devanagari with a virama (U+094D) and a dependent vowel sign.
        let s = "\u{928}\u{92E}\u{938}\u{94D}\u{924}\u{947} x";
        let w = "\u{928}\u{92E}\u{938}\u{94D}\u{924}\u{947}".len();
        assert_eq!(word(s, 0), (0, w));
        assert_eq!(word(s, 9), (0, w), "on the virama");
        // An emoji ZWJ sequence and a variation selector stay one character.
        let s = "a \u{1F469}\u{200D}\u{1F4BB} b \u{263A}\u{FE0F}";
        let e = (2, 2 + "\u{1F469}\u{200D}\u{1F4BB}".len());
        assert_eq!(word(s, 2), e);
        assert_eq!(word(s, 6), e, "on the joiner");
        assert_eq!(word(s, s.len() - 1), (s.len() - 6, s.len()), "at the end: the last character whole");
        // Empty paragraphs select nothing.
        assert_eq!(word("a\n\nb", 2), (2, 2));
    }

    #[test]
    fn clicks_select_words_spaces_lines_paragraphs_and_the_story() {
        let mut s = Session::new();
        s.execute("file.new", &json!({})).unwrap();
        let text = "Hello  brave world.\nSecond paragraph here.";
        let r = s.execute("frame.create", &json!({"rect": [72, 72, 500, 200], "content": "text", "text": text})).unwrap();
        let (fid, sid) = (r["id"].as_u64().unwrap(), r["story"].as_u64().unwrap());
        // Spread point over the right half of the glyph at byte `b`.
        let at = |s: &Session, b: usize| {
            let d = &s.doc().unwrap().doc;
            let cs = designcraft_compose::compose_story(d, designcraft_doc::StoryId(sid), &Default::default());
            let (l, g) = cs.frames[0].lines.iter().find_map(|l| l.glyphs.iter().find(|g| g.byte == b && g.len > 0).map(|g| (l, g))).unwrap();
            json!([g.x + g.adv * 0.75, l.baseline - 3.0])
        };
        let mut select = |unit: &str, b: usize| {
            let p = at(&s, b);
            let r: Value = s.execute("text.selectAt", &json!({"frame": fid, "point": p, "unit": unit})).unwrap();
            (r["anchor"].as_u64().unwrap() as usize, r["focus"].as_u64().unwrap() as usize)
        };
        assert_eq!(select("word", 1), (0, 5), "inside a word");
        assert_eq!(select("word", 4), (0, 5), "its last letter");
        assert_eq!(select("word", 5), (5, 7), "the run of spaces after it");
        assert_eq!(select("word", 18), (18, 19), "punctuation alone");
        assert_eq!(select("line", 9), (0, 19));
        assert_eq!(select("paragraph", 9), (0, 20), "with its return");
        assert_eq!(select("paragraph", 22), (20, text.len()));
        assert_eq!(select("story", 9), (0, text.len()));
        // A second click inside the selected word selects again rather than starting a text drag.
        s.execute("text.select", &json!({"story": sid, "anchor": 7, "focus": 12})).unwrap();
        let p = at(&s, 9);
        s.execute("text.selectAt", &json!({"frame": fid, "point": p, "unit": "line"})).unwrap();
        let t = s.doc().unwrap().selection.text.unwrap();
        assert_eq!((t.anchor, t.focus), (0, 19));
        s.execute("text.release", &json!({"frame": fid, "point": p, "moved": false})).unwrap();
        let t = s.doc().unwrap().selection.text.unwrap();
        assert_eq!((t.anchor, t.focus), (0, 19), "releasing keeps it");
        // The older word command agrees.
        let p = at(&s, 8);
        s.execute("text.selectWord", &json!({"frame": fid, "point": p})).unwrap();
        let t = s.doc().unwrap().selection.text.unwrap();
        assert_eq!((t.anchor, t.focus), (7, 12));
        assert!(s.execute("text.selectAt", &json!({"frame": fid, "point": p, "unit": "sentence"})).is_err());
        assert!(s.execute("text.selectAt", &json!({"frame": fid, "unit": "word"})).is_err(), "no point");
    }
}

#[cfg(test)]
mod style_link_tests {
    use serde_json::json;

    use crate::Session;

    #[test]
    fn bold_and_italic_follow_the_family_and_toggle() {
        let mut s = Session::new();
        s.execute("file.new", &json!({})).unwrap();
        let r = s.execute("frame.create", &json!({"rect": [72, 72, 500, 200], "content": "text", "text": "plain words here"})).unwrap();
        let sid = r["story"].as_u64().unwrap();
        let style_at = |s: &mut Session, b: usize| {
            s.execute("text.select", &json!({"story": sid, "anchor": b, "focus": b + 1})).unwrap();
            s.execute("type.selectionAttrs", &json!({})).unwrap()["chars"]["fontStyle"].as_str().unwrap().to_string()
        };
        s.execute("text.select", &json!({"story": sid, "anchor": 0, "focus": 5})).unwrap();
        s.execute("type.italic", &json!({})).unwrap();
        assert_eq!(style_at(&mut s, 0), "Italic");
        // Bold over italic and plain text: each run takes its own linked style.
        s.execute("text.select", &json!({"story": sid, "anchor": 0, "focus": 11})).unwrap();
        s.execute("type.bold", &json!({})).unwrap();
        assert_eq!(style_at(&mut s, 0), "Bold Italic");
        assert_eq!(style_at(&mut s, 7), "Bold");
        // Again: off, back to each run's regular weight.
        s.execute("text.select", &json!({"story": sid, "anchor": 0, "focus": 11})).unwrap();
        s.execute("type.bold", &json!({})).unwrap();
        assert_eq!(style_at(&mut s, 0), "Italic");
        assert_eq!(style_at(&mut s, 7), "Regular");
        // Nothing to change: an error and no undo step.
        s.execute("text.select", &json!({"story": sid, "anchor": 6, "focus": 11})).unwrap();
        let steps = s.doc().unwrap().history.undo.len();
        assert!(s.execute("type.bold", &json!({"on": false})).is_err());
        assert_eq!(s.doc().unwrap().history.undo.len(), steps);
        // At a caret the style is for what is typed next.
        let end = "plain words here".len();
        s.execute("text.select", &json!({"story": sid, "anchor": end, "focus": end})).unwrap();
        s.execute("type.bold", &json!({})).unwrap();
        assert_eq!(s.execute("type.selectionAttrs", &json!({})).unwrap()["chars"]["fontStyle"], "Bold");
        s.execute("text.insert", &json!({"text": " now"})).unwrap();
        assert_eq!(style_at(&mut s, end + 1), "Bold");
        assert_eq!(style_at(&mut s, end - 1), "Regular", "text before the caret is unchanged");
        // Moving the caret forgets it.
        s.execute("text.select", &json!({"story": sid, "anchor": 2, "focus": 2})).unwrap();
        s.execute("type.char", &json!({"attrs": {"size": 30}})).unwrap();
        s.execute("text.select", &json!({"story": sid, "anchor": 3, "focus": 3})).unwrap();
        s.execute("text.insert", &json!({"text": "x"})).unwrap();
        s.execute("text.select", &json!({"story": sid, "anchor": 3, "focus": 4})).unwrap();
        assert_eq!(s.execute("type.selectionAttrs", &json!({})).unwrap()["chars"]["size"], 12.0);
    }
}

#[cfg(test)]
mod text_color_tests {
    use serde_json::json;

    use crate::Session;

    #[test]
    fn unnamed_colours_go_to_text_when_asked() {
        let mut s = Session::new();
        s.execute("file.new", &json!({})).unwrap();
        let r = s.execute("frame.create", &json!({"rect": [72, 72, 300, 200], "content": "text", "text": "colour me", "caret": false})).unwrap();
        let (fid, sid) = (r["id"].as_u64().unwrap(), r["story"].as_u64().unwrap());
        s.execute("selection.set", &json!({"ids": [fid]})).unwrap();
        let fill_of_frame = |s: &Session| s.doc().unwrap().doc.item(designcraft_doc::ItemId(fid)).unwrap().fill.swatch.clone();
        let before = fill_of_frame(&s);
        let r = s.execute("object.color", &json!({"color": "#ff0000", "text": true})).unwrap();
        let red = r["swatch"].as_str().unwrap().to_string();
        assert_eq!(fill_of_frame(&s), before, "the frame keeps its fill");
        let story = s.doc().unwrap().doc.story(designcraft_doc::StoryId(sid)).unwrap().clone();
        assert_eq!(story.char_format_at(3).over.fill.as_deref(), Some(red.as_str()));
        // A text selection takes it without asking.
        s.execute("text.select", &json!({"story": sid, "anchor": 0, "focus": 3})).unwrap();
        let r = s.execute("object.color", &json!({"color": "#00ff00", "target": "stroke"})).unwrap();
        let story = s.doc().unwrap().doc.story(designcraft_doc::StoryId(sid)).unwrap().clone();
        assert_eq!(story.format_after(0).over.stroke.as_deref(), r["swatch"].as_str());
        assert_eq!(s.doc().unwrap().history.undo.len(), 3, "the frame, then one undo step per colour");
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
mod rule_tests {
    use serde_json::json;

    use crate::Session;

    #[test]
    fn a_partial_rule_edit_keeps_the_other_fields() {
        let mut s = Session::new();
        s.execute("file.new", &json!({})).unwrap();
        let rule = json!({"on": true, "weight": 3.0, "color": "[Black]", "tint": 0.5, "columnWidth": false, "offset": 4.0, "leftIndent": 6.0, "rightIndent": 2.0});
        s.execute("style.paragraph.create", &json!({"name": "Ruled", "para": {"ruleBelow": rule}})).unwrap();
        let r = s.execute("frame.create", &json!({"rect": [72, 72, 300, 400], "content": "text", "text": "One\nTwo"})).unwrap();
        s.execute("text.select", &json!({"story": r["story"], "anchor": 0, "focus": 0})).unwrap();
        s.execute("style.paragraph.apply", &json!({"name": "Ruled"})).unwrap();
        // Local: only the colour changes; the rest is inherited from the style.
        s.execute("type.para", &json!({"attrs": {"ruleBelow": {"color": "[Registration]"}}})).unwrap();
        let attrs = s.execute("type.selectionAttrs", &json!({})).unwrap();
        let mut want = rule.clone();
        want["color"] = json!("[Registration]");
        assert_eq!(attrs["para"]["ruleBelow"], want);
        assert_eq!(attrs["para"]["ruleAbove"]["on"], false, "the other rule is untouched");
        // Style: only the weight changes.
        s.execute("style.paragraph.edit", &json!({"name": "Ruled", "para": {"ruleBelow": {"weight": 1.5}}})).unwrap();
        let st = s.doc().unwrap().doc.styles.para("Ruled").cloned().unwrap();
        let below = serde_json::to_value(st.para.rule_below.unwrap()).unwrap();
        let mut want = rule.clone();
        want["weight"] = json!(1.5);
        assert_eq!(below, want);
        assert!(s.execute("type.para", &json!({"attrs": {"ruleBelow": {"thickness": 2}}})).is_err(), "unknown rule field");
    }

    #[test]
    fn rule_above_width_follows_column_or_text() {
        let mut s = Session::new();
        s.execute("file.new", &json!({})).unwrap();
        let r = s.execute("frame.create", &json!({"rect": [72, 72, 400, 400], "content": "text", "text": "Short"})).unwrap();
        s.execute("text.select", &json!({"story": r["story"], "anchor": 0, "focus": 0})).unwrap();
        let sid = designcraft_doc::StoryId(r["story"].as_u64().unwrap());
        let mut widths = vec![];
        for column in [true, false] {
            s.execute("type.para", &json!({"ruleAbove": {"on": true, "columnWidth": column}})).unwrap();
            let cs = s.cache.get(&s.doc().unwrap().doc, sid, None);
            widths.push(cs.frames[0].decos[0].rect.width());
        }
        assert!((widths[0] - 328.0).abs() < 1e-6, "column: {widths:?}");
        assert!(widths[1] < 100.0, "text: {widths:?}");
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

    #[test]
    fn warichu_toggles_on_the_selection_and_undoes() {
        let mut s = Session::new();
        s.execute("file.new", &json!({})).unwrap();
        let r = s.execute("frame.create", &json!({"rect": [72, 72, 400, 200], "content": "text", "text": "注釈です"})).unwrap();
        let sid = r["story"].as_u64().unwrap();
        let n = "注釈".len();
        s.execute("text.select", &json!({"story": sid, "anchor": 0, "focus": n})).unwrap();
        s.execute("type.warichu", &json!({"lines": 2, "size": 40, "align": "left", "lineSpacing": 1, "charsBeforeBreak": 2, "charsAfterBreak": 2}))
            .unwrap();
        let f = |s: &Session| s.doc().unwrap().doc.stories[&designcraft_doc::StoryId(sid)].char_format_at(1).over.clone();
        let a = f(&s);
        assert_eq!(a.warichu, Some(true));
        assert_eq!(a.warichu_lines, Some(2));
        assert_eq!(a.warichu_size, Some(40.0));
        assert_eq!(a.warichu_line_spacing, Some(1.0));
        assert_eq!(a.warichu_alignment, Some(designcraft_doc::cjk::WarichuAlignment::Left));
        assert_eq!(a.warichu_chars_before_break, Some(2));
        assert_eq!(a.warichu_chars_after_break, Some(2));
        // The characters after the selection are not part of the note.
        assert_eq!(s.doc().unwrap().doc.stories[&designcraft_doc::StoryId(sid)].char_format_at(n + 1).over.warichu, None);
        s.execute("edit.undo", &json!({})).unwrap();
        assert_eq!(f(&s).warichu, None);
        s.execute("edit.redo", &json!({})).unwrap();
        assert_eq!(f(&s).warichu, Some(true));
        s.execute("type.warichu", &json!({})).unwrap();
        assert_eq!(f(&s).warichu, Some(false));
    }

    #[test]
    fn warichu_up_and_down_move_between_the_rows() {
        let mut s = Session::new();
        s.execute("file.new", &json!({})).unwrap();
        let r = s.execute("frame.create", &json!({"rect": [72, 72, 500, 200], "content": "text", "text": "ABCDEFGHZ"})).unwrap();
        let sid = r["story"].as_u64().unwrap();
        s.execute("text.select", &json!({"story": sid, "anchor": 0, "focus": 8})).unwrap();
        s.execute("type.warichu", &json!({"lines": 2, "align": "left"})).unwrap();
        s.execute("text.select", &json!({"story": sid, "anchor": 0, "focus": 0})).unwrap();
        let down = s.execute("text.move", &json!({"dir": "down"})).unwrap()["pos"].as_u64().unwrap() as usize;
        assert!((4..8).contains(&down), "down enters the second row: {down}");
        let up = s.execute("text.move", &json!({"dir": "up"})).unwrap()["pos"].as_u64().unwrap() as usize;
        assert!(up < 4, "up returns to the first row: {up}");
        s.execute("text.select", &json!({"story": sid, "anchor": 8, "focus": 8})).unwrap();
        let stayed = s.execute("text.move", &json!({"dir": "down"})).unwrap()["pos"].as_u64().unwrap() as usize;
        assert_eq!(stayed, 8, "down from the character after the note does not fall back into a row");
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
mod vertical_caret_tests {
    use designcraft_doc::StoryId;
    use serde_json::json;

    use crate::Session;

    /// A story set in three or more lines; returns its id.
    fn story(s: &mut Session, vertical: bool) -> StoryId {
        s.execute("file.new", &json!({})).unwrap();
        let text = "一二三四五六七八九十".repeat(8);
        let rect = if vertical { [72, 72, 200, 400] } else { [72, 72, 400, 200] };
        let r = s.execute("frame.create", &json!({"rect": rect, "content": "text", "text": text, "vertical": vertical})).unwrap();
        StoryId(r["story"].as_u64().unwrap())
    }

    fn mv(s: &mut Session, dir: &str, extend: bool, word: bool) -> usize {
        s.execute("text.move", &json!({"dir": dir, "extend": extend, "word": word})).unwrap()["pos"].as_u64().unwrap() as usize
    }

    /// (line index, position along the line) of the caret at `pos`.
    fn place(s: &Session, sid: StoryId, pos: usize) -> (usize, f64) {
        let cs = s.cache.get(&s.doc().unwrap().doc, sid, None);
        let (_, x, baseline, _, _) = crate::compose::caret(&cs, pos).unwrap();
        let line = cs.frames[0].lines.iter().position(|l| (l.baseline - baseline).abs() < 0.01).unwrap();
        (line, x)
    }

    #[test]
    fn arrow_keys_follow_vertical_lines() {
        let mut s = Session::new();
        let sid = story(&mut s, true);
        assert!(s.cache.get(&s.doc().unwrap().doc, sid, None).frames[0].lines.len() >= 3);
        let c = '一'.len_utf8();
        s.execute("text.select", &json!({"story": sid.0, "anchor": 3 * c, "focus": 3 * c})).unwrap();
        // Down and Up move along the line, a character at a time.
        assert_eq!(mv(&mut s, "down", false, false), 4 * c);
        assert_eq!(mv(&mut s, "up", false, false), 3 * c);
        // Left moves to the next line (lines follow each other right to left), keeping the
        // position along the line; Right comes back.
        let (line, x) = place(&s, sid, 3 * c);
        let next = mv(&mut s, "left", false, false);
        let (nline, nx) = place(&s, sid, next);
        assert_eq!(nline, line + 1);
        assert!((nx - x).abs() < 0.5, "{x} {nx}");
        assert_eq!(mv(&mut s, "right", false, false), 3 * c);
        // Shift extends.
        assert_eq!(mv(&mut s, "down", true, false), 4 * c);
        let t = s.doc().unwrap().selection.text.unwrap();
        assert_eq!((t.anchor, t.focus), (3 * c, 4 * c));
        let p = mv(&mut s, "left", true, false);
        let t = s.doc().unwrap().selection.text.unwrap();
        assert_eq!((t.anchor, t.focus), (3 * c, p));
        assert_eq!(place(&s, sid, p).0, line + 1);
        // With a selection, Up collapses it to its start.
        assert_eq!(mv(&mut s, "up", false, false), 3 * c);
        // By words: Alt+Down is the vertical Alt+Right.
        let text = s.doc().unwrap().doc.story(sid).unwrap().text.clone();
        assert_eq!(mv(&mut s, "down", false, true), super::next_word(&text, 3 * c));
    }

    #[test]
    fn arrow_keys_in_horizontal_lines() {
        let mut s = Session::new();
        let sid = story(&mut s, false);
        let c = '一'.len_utf8();
        s.execute("text.select", &json!({"story": sid.0, "anchor": 3 * c, "focus": 3 * c})).unwrap();
        assert_eq!(mv(&mut s, "right", false, false), 4 * c);
        assert_eq!(mv(&mut s, "left", false, false), 3 * c);
        let (line, x) = place(&s, sid, 3 * c);
        let next = mv(&mut s, "down", false, false);
        let (nline, nx) = place(&s, sid, next);
        assert_eq!(nline, line + 1);
        assert!((nx - x).abs() < 0.5, "{x} {nx}");
        assert_eq!(mv(&mut s, "up", false, false), 3 * c);
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
}

#[cfg(test)]
mod sentence_selection_tests {
    use super::{para_bounds, sentence_bounds};

    fn sentence(s: &str, at: &str) -> String {
        let i = s.find(at).unwrap();
        let (a, b) = sentence_bounds(s, i);
        s[a..b].to_string()
    }

    #[test]
    fn sentences_end_at_a_stop_before_a_space_or_the_paragraph_end() {
        let s = "It costs 3.5 euros. \u{201C}Really?\u{201D} (Twice!) Yes\u{2026} fine.\nNext one. Last";
        assert_eq!(sentence(s, "costs"), "It costs 3.5 euros. ");
        assert_eq!(sentence(s, "Really"), "\u{201C}Really?\u{201D} ");
        assert_eq!(sentence(s, "Twice"), "(Twice!) ");
        assert_eq!(sentence(s, "Yes"), "Yes\u{2026} ");
        assert_eq!(sentence(s, "fine"), "fine.");
        assert_eq!(sentence(s, "one"), "Next one. ");
        assert_eq!(sentence(s, "Last"), "Last");
        // The caret at the very end of a paragraph is in its last sentence.
        let end = s.find('\n').unwrap();
        assert_eq!(sentence_bounds(s, end), (s.find("fine").unwrap(), end));
    }

    #[test]
    fn cjk_full_stops_end_sentences_without_a_space() {
        let s = "これは文です。次の文！最後";
        assert_eq!(sentence(s, "次"), "次の文！");
        assert_eq!(sentence(s, "最後"), "最後");
    }

    #[test]
    fn paragraphs_stop_at_returns() {
        let s = "One.\nTwo two.\n";
        assert_eq!(para_bounds(s, 6), (5, 13));
        assert_eq!(para_bounds(s, 0), (0, 4));
        assert_eq!(para_bounds(s, s.len()), (s.len(), s.len()));
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
}

#[cfg(test)]
mod gradient_text_tests {
    use serde_json::json;

    /// Mean red and blue of the ink (pixels whose green is low: red → blue has none) in columns
    /// `x0..x1`.
    fn ink(img: &designcraft_render::Rendered, x0: u32, x1: u32) -> (f64, f64) {
        let (mut r, mut b, mut n) = (0.0, 0.0, 0.0);
        for x in x0..x1 {
            for y in 0..img.height {
                let p = img.pixel(x, y);
                if p[1] < 100 {
                    (r, b, n) = (r + p[0] as f64, b + p[2] as f64, n + 1.0);
                }
            }
        }
        assert!(n > 50.0, "no ink in {x0}..{x1}");
        (r / n, b / n)
    }

    /// Red ink at the frame's left, blue at its right.
    fn check(img: &designcraft_render::Rendered, what: &str) {
        let (lr, lb) = ink(img, 0, 60);
        let (rr, rb) = ink(img, 200, 300);
        assert!(lr > lb + 80.0, "{what}: the left ink is red ({lr:.0}, {lb:.0})");
        assert!(rb > rr + 80.0, "{what}: the right ink is blue ({rr:.0}, {rb:.0})");
    }

    #[test]
    fn a_gradient_on_text_spans_its_frame_on_screen_and_in_pdf() {
        let mut s = crate::Session::new();
        s.execute("file.new", &json!({"width": 300, "height": 120, "facingPages": false, "margins": 0})).unwrap();
        // A gradient swatch (an off-page object's unnamed one), red → blue.
        let shape = s.execute("frame.create", &json!({"rect": [-60, 0, -50, 10], "content": "unassigned"})).unwrap()["id"].clone();
        s.execute("object.gradient", &json!({"ids": [shape], "stops": [{"location": 0, "color": "#ff0000"}, {"location": 100, "color": "#0000ff"}]}))
            .unwrap();
        let id = designcraft_doc::ItemId(shape.as_u64().unwrap());
        let swatch = s.doc().unwrap().doc.item(id).unwrap().fill.swatch.clone();
        let r = s.execute("frame.create", &json!({"rect": [0, 0, 300, 120], "content": "text", "text": "IIIIIIIIIIIIIIIIIIIIIIIIIIIIII"})).unwrap();
        s.execute("text.select", &json!({"story": r["story"], "anchor": 0, "focus": 30})).unwrap();
        s.execute("type.char", &json!({"size": 60, "tracking": 0, "fill": swatch})).unwrap();
        let d = s.doc().unwrap().doc.clone();
        let mut rr = designcraft_render::Renderer::new();
        check(&rr.render_page(&d, &s.cache, 0, 1.0, false, &Default::default()).unwrap(), "screen");
        // The PDF, placed back and drawn: the same gradient.
        let pdf = designcraft_pdf::export_pdf(&d, &s.cache, &Default::default()).unwrap();
        let mut p = crate::Session::new();
        p.execute("file.new", &json!({"width": 300, "height": 120, "facingPages": false})).unwrap();
        p.execute("file.place", &json!({"base64": super::super::file::base64_encode(&pdf), "name": "text.pdf", "x": 0, "y": 0, "width": 300}))
            .unwrap();
        let d = p.doc().unwrap().doc.clone();
        check(&rr.render_page(&d, &p.cache, 0, 1.0, false, &Default::default()).unwrap(), "pdf");
    }

    /// The columns the red → blue ink spans (black table edges left out).
    fn ink_span(img: &designcraft_render::Rendered) -> (u32, u32) {
        let cols: Vec<u32> = (0..img.width)
            .filter(|&x| {
                (0..img.height).any(|y| {
                    let p = img.pixel(x, y);
                    p[1] < 100 && p[0].max(p[2]) > 100
                })
            })
            .collect();
        (*cols.first().unwrap(), *cols.last().unwrap())
    }

    /// Mean red and blue of the red → blue ink in columns `x0..x1`.
    fn coloured_ink(img: &designcraft_render::Rendered, x0: u32, x1: u32) -> (f64, f64) {
        let (mut r, mut b, mut n) = (0.0, 0.0, 0.0);
        for x in x0..x1 {
            for y in 0..img.height {
                let p = img.pixel(x, y);
                if p[1] < 100 && p[0].max(p[2]) > 100 {
                    (r, b, n) = (r + p[0] as f64, b + p[2] as f64, n + 1.0);
                }
            }
        }
        assert!(n > 20.0, "no ink in {x0}..{x1}");
        (r / n, b / n)
    }

    #[test]
    fn a_gradient_on_table_cell_text_spans_the_cells_text_in_pdf_as_on_screen() {
        let mut s = crate::Session::new();
        s.execute("file.new", &json!({"width": 300, "height": 120, "facingPages": false, "margins": 0})).unwrap();
        // A gradient swatch (an off-page object's unnamed one), red → blue.
        let shape = s.execute("frame.create", &json!({"rect": [-60, 0, -50, 10], "content": "unassigned"})).unwrap()["id"].clone();
        s.execute("object.gradient", &json!({"ids": [shape], "stops": [{"location": 0, "color": "#ff0000"}, {"location": 100, "color": "#0000ff"}]}))
            .unwrap();
        let g = s.doc().unwrap().doc.item(designcraft_doc::ItemId(shape.as_u64().unwrap())).unwrap().fill.swatch.clone();
        let r = s.execute("frame.create", &json!({"rect": [0, 0, 300, 120], "content": "text", "text": ""})).unwrap();
        s.execute("text.select", &json!({"story": r["story"], "anchor": 0, "focus": 0})).unwrap();
        s.execute("table.insert", &json!({"rows": 1, "cols": 3})).unwrap();
        // A third of the frame's width: the cell's text, red → blue.
        let mut d: designcraft_doc::Document = (*s.doc().unwrap().doc).clone();
        let sid = d.stories.values().find(|st| !st.tables.is_empty()).unwrap().id;
        let t = std::sync::Arc::make_mut(d.story_mut(sid).unwrap().tables.values_mut().next().unwrap());
        let text = &mut t.cell_mut(0, 0).unwrap().text;
        text.insert(0, "HHHH");
        text.format_chars(0..4, |f| {
            f.over.fill = Some(g.clone());
            f.over.size = Some(24.0);
        });
        let mut rr = designcraft_render::Renderer::new();
        let cache = designcraft_compose::Cache::new();
        let screen = rr.render_page(&d, &cache, 0, 1.0, false, &Default::default()).unwrap();
        let pdf = designcraft_pdf::export_pdf(&d, &cache, &Default::default()).unwrap();
        let mut p = crate::Session::new();
        p.execute("file.new", &json!({"width": 300, "height": 120, "facingPages": false})).unwrap();
        p.execute("file.place", &json!({"base64": super::super::file::base64_encode(&pdf), "name": "cell.pdf", "x": 0, "y": 0, "width": 300}))
            .unwrap();
        let placed = rr.render_page(&p.doc().unwrap().doc.clone(), &p.cache, 0, 1.0, false, &Default::default()).unwrap();
        let (x0, x1) = ink_span(&screen);
        assert!(x1 < 120, "the text stays in the first column: {x0}..{x1}");
        let q = (x1 - x0) / 4;
        for (what, img) in [("screen", &screen), ("pdf", &placed)] {
            let (lr, lb) = coloured_ink(img, x0, x0 + q);
            let (rr, rb) = coloured_ink(img, x1 + 1 - q, x1 + 1);
            assert!(lr > lb + 50.0, "{what}: the left ink is red ({lr:.0}, {lb:.0})");
            assert!(rb > rr + 50.0, "{what}: the right ink is blue ({rr:.0}, {rb:.0})");
        }
    }
}

#[cfg(test)]
mod compose_tests {
    use serde_json::json;

    use crate::Session;

    /// A story "Ta" with the caret after "T" and 30 pt chosen for the next typing.
    fn session() -> (Session, u64) {
        let mut s = Session::new();
        s.execute("file.new", &json!({})).unwrap();
        let r = s.execute("frame.create", &json!({"rect": [72, 72, 500, 200], "content": "text", "text": "Ta"})).unwrap();
        let sid = r["story"].as_u64().unwrap();
        s.set_tool("type");
        s.execute("text.select", &json!({"story": sid, "anchor": 1})).unwrap();
        s.execute("type.char", &json!({"attrs": {"size": 30}})).unwrap();
        (s, sid)
    }

    fn text(s: &Session, sid: u64) -> String {
        s.doc().unwrap().doc.story(designcraft_doc::StoryId(sid)).unwrap().text.clone()
    }

    /// ⌥E shows the accent in the story, in the format it will be typed in, then I types "í":
    /// one undo step, and the accent was never part of the document.
    #[test]
    fn composed_text_shows_in_the_typing_format_until_it_is_typed() {
        let (mut s, sid) = session();
        let r = s.execute("text.compose", &json!({"text": "´"})).unwrap();
        assert_eq!(r["range"], json!([1, 3]));
        assert_eq!(text(&s, sid), "T´a");
        let size_at = |s: &Session, b| s.doc().unwrap().doc.story(designcraft_doc::StoryId(sid)).unwrap().char_format_at(b).over.size;
        assert_eq!(size_at(&s, 2), Some(30.0), "the accent is shown in the format chosen at the caret");
        assert_eq!(s.composition().map(|c| c.2), Some(1..3));
        s.execute("text.compose", &json!({"text": "¨"})).unwrap();
        assert_eq!(text(&s, sid), "T¨a", "a new composition replaces the last one");
        s.execute("text.compose", &json!({"text": ""})).unwrap();
        assert_eq!(text(&s, sid), "Ta");
        assert!(s.composition().is_none());
        s.execute("text.insert", &json!({"text": "í"})).unwrap();
        assert_eq!(text(&s, sid), "Tía");
        assert_eq!(size_at(&s, 2), Some(30.0), "the typing format survived the composition");
        s.execute("edit.undo", &json!({})).unwrap();
        assert_eq!(text(&s, sid), "Ta");
    }

    /// Anything else that edits, a tool change or a drag drops the uncommitted accent.
    #[test]
    fn other_edits_drop_the_composed_text() {
        for act in 0..3 {
            let (mut s, sid) = session();
            s.execute("text.compose", &json!({"text": "´"})).unwrap();
            match act {
                0 => {
                    s.execute("text.move", &json!({"dir": "right"})).unwrap();
                }
                1 => s.set_tool("selection"),
                _ => s.begin_interaction("Move").unwrap(),
            }
            assert_eq!(text(&s, sid), "Ta", "case {act}");
            assert!(s.composition().is_none());
        }
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
