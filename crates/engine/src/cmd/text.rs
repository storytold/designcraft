//! Text editing (Type tool) and character/paragraph formatting.

use designcraft_compose as compose;
use designcraft_doc::story::floor_char_boundary;
use designcraft_doc::{CellAddr, CharAttrs, Content, ItemId, ParaAttrs, Selection, StoryId, TextSel};
use designcraft_geom::Point;
use serde_json::{Value, json};

use super::{CommandSpec, bad, bool_or, cmd, has_doc, has_text, has_text_or_frames, id_param, ok, point_param, str_param};
use crate::{Result, Session};

mod asian;
mod caret;
mod edit;
mod format;
mod opentype;
mod story;

use asian::{auto_tcy_cmd, grid_settings_cmd, kenten_cmd, ruby_cmd, shatai_cmd, warichu_cmd};
pub(crate) use caret::story_of;
use caret::{move_caret, para_bounds, place, release, select_around, select_at, sentence_bounds};
pub use caret::{next_char, next_word, prev_char, prev_word};
pub(crate) use edit::delete_selection;
pub use edit::{PLACEHOLDER, quote_marks};
use edit::{change_case, delete, delete_surrounding, fill_placeholder, insert};
pub(crate) use format::{
    Target, TypingFormat, apply_char_attrs, effective_char_format, format_chars, format_paras, format_targets, format_targets_pub,
    rename_typing_swatch, validate_char_fill,
};
use format::{break_link, kern_pair, kern_step, link_style, para_toggle, pick_up_format, step_size, swap_fill_stroke};
use opentype::{open_type, open_type_features, set_features};
pub(crate) use story::set_story_direction;
use story::{selection_attrs, story_direction, story_options};

/// Build the text command registry: caret/selection movement, insertion/deletion, release, text and paragraph formatting, story queries, ruby and OpenType commands, with their parameters and enabled predicates.
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
        cmd!(
            noundo "text.extendTo",
            "Extend Text Selection",
            [],
            None,
            "{frame, point, unit?: word|line|paragraph|story} — moves the selection's focus to the point; with `unit`, after a text.selectAt, the selection grows by whole units from the unit first selected (a drag after a double, triple or quadruple click)",
            has_doc,
            |s, p| place(s, p, true)
        ),
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
        cmd!(
            "type.pickUpFormat",
            "Apply Text Attributes (Eyedropper)",
            [],
            None,
            "{frame, point} — the Eyedropper with text selected: the character and paragraph formatting (styles and overrides) of the text under the point go to the selected text",
            has_text,
            pick_up_format
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
            st.bump_revision();
            s.typing_format = None;
            ok()
        }),
        cmd!("text.insert", "Type", [], None, "{text, raw?: bool (no typographer's quotes)} — replaces the selected text", has_text, insert),
        cmd!(
            "text.deleteSurrounding",
            "Delete Surrounding Text",
            [],
            None,
            "{before?: int, after?: int, bytes?: bool, commit?: string} — removes the text around the selection (an input method's delete-surrounding), keeps the selection and its direction, and with `commit` types that text over it: one undo step",
            has_text,
            delete_surrounding
        ),
        cmd!("text.delete", "Delete Text", [], None, "{forward?: bool, word?: bool}", has_text, delete),
        cmd!(noundo "text.move", "Move Caret", [], None, "{dir: left|right|up|down|lineStart|lineEnd|storyStart|storyEnd, extend?, word?}", has_text, move_caret),
        cmd!(noundo "text.exitToFrame", "Select Frame", [], None, "{}", has_doc, |s, _| {
            let st = s.doc_mut()?;
            if let Some(t) = st.selection.text.take() {
                let f = t.frame.or_else(|| st.doc.story(t.story).and_then(|x| x.frames.first().copied()));
                st.selection.items = f.into_iter().collect();
            }
            st.selection.cells = None;
            st.bump_revision();
            ok()
        }),
        cmd!(
            "story.setDirection",
            "Story Column Direction",
            ["Type", "Story Column Direction"],
            None,
            "{story?, frame?: a text frame of the story, direction: leftToRight|rightToLeft} — column progression, independent of paragraph direction",
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
        cmd!(
            "story.options",
            "Story Options",
            [],
            None,
            "{story?, frame?: a text frame of the story (default: the stories of the text selection or the selected frames), opticalMargin?: bool — Optical Margin Alignment: punctuation and letter edges hang outside the frame in every paragraph of the story except those with ignoreOpticalMargin, opticalMarginSize?: pt (0.1–1296, default 12) — the type size the hang amounts are set for} → {stories}",
            has_doc,
            story_options
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
                let offset = |v: u64| usize::try_from(v).map_err(|_| bad("story.replaceRange", format!("offset {v} is out of range")));
                let a = offset(p.get("start").and_then(Value::as_u64).unwrap_or(0))?;
                let b = match p.get("end").and_then(Value::as_u64) {
                    Some(v) => offset(v)?,
                    None => a,
                };
                let text = super::text_param(p, "text");
                s.edit(|d, sel| {
                    let st = d.story_mut(sid).ok_or(designcraft_doc::DocError::NoStory(sid))?;
                    let n = st.len();
                    if a > b || b > n || !st.text.is_char_boundary(a) || !st.text.is_char_boundary(b) {
                        let msg = format!("range {a}..{b} must be UTF-8 byte offsets on character boundaries, start <= end; the story is {n} bytes");
                        return Err(bad("story.replaceRange", msg));
                    }
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
        cmd!("type.fillWithPlaceholder", "Fill with Placeholder Text", ["Type"], None, "{frame?, story?}", has_text_or_frames, fill_placeholder),
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
            "type.kernPair",
            "Kerning",
            [],
            None,
            "{kerning: {kind: metrics|optical|none}|{kind: manual, value: 1/1000 em}|null} — at a caret in text, the pair at the caret (the character before the caret takes it); at the start of a story or paragraph, the next typing; a selection or frames take it like `type.char`",
            has_text_or_frames,
            kern_pair
        ),
        cmd!(
            "type.para",
            "Paragraph Formatting",
            [],
            None,
            "{attrs: {align?, leftIndent?, firstLineIndent?, spaceBefore?, spaceAfter?, dropCapLines?, dropCapChars?, dropCapStyle?, dropCapAlignLeft?, dropCapScaleDescenders?, hyphenate?: bool (default true), composer?, opticalMargin?: bool (hang at each character's own size; a story with story.options opticalMargin overrides it), ignoreOpticalMargin?: bool (no hang in the paragraph), tabs?, ruleAbove?/ruleBelow?: {on?, weight?, color?, tint?, columnWidth?, offset?, leftIndent?, rightIndent?} (only the named rule fields change), …}}",
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
            "{text?, type?: group|perCharacter, alignment?: left|center|right|fullJustify|jis|equalAki|oneAki, position?: aboveRight|belowLeft, xOffset?, yOffset? (pt), font?, fontStyle? (\"\" = the text's), size? (pt; null = half the text size), xScale?, yScale? (%), openTypePro?: bool, autoTcyDigits?: 0-9, autoTcyIncludeRoman?: bool, autoTcyAutoScale?: bool, overhang?: none|oneRuby|halfRuby|oneChar|halfChar|noLimit, parentSpacing?: noAdjustment|bothSides|aki121|equalAki|fullJustify, autoAlign?: bool, autoScaling?: bool, scalingPercent? (%), fill?, stroke? (swatch; \"\" = the text's), fillTint?, strokeTint? (%; null = the text's), strokeWeight? (pt; null = the text's), overprintFill?, overprintStroke?: auto|on|off} — the reading set over the selected text and how it is placed; empty text removes it. Per-character ruby separates the readings with U+3000",
            has_text_or_frames,
            ruby_cmd
        ),
        cmd!(
            "type.kenten",
            "Kenten",
            [],
            None,
            "{on?: bool, kind?: sesameDot|whiteSesameDot|fisheye|blackCircle|smallBlackCircle|bullseye|blackTriangle|whiteTriangle|whiteCircle|smallWhiteCircle|custom, character?: text (custom kind), font?: family (custom kind), fontStyle?, size?: pt (null: half the text size), distance?: pt, position?: aboveRight|belowLeft, alignment?: center|start, xScale?: %, yScale?: %, fill?: swatch (\"\": the text's), fillTint?: %, stroke?: swatch, strokeTint?: %, strokeWeight?: pt, overprintFill?: auto|on|off, overprintStroke?: auto|on|off} — emphasis marks beside the selected characters (toggles by default; any setting turns them on)",
            has_text_or_frames,
            kenten_cmd
        ),
        cmd!(
            "type.shatai",
            "Shatai",
            [],
            None,
            "{magnification?: % (0 = off), angle?: degrees, adjustRotation?: bool, adjustTsume?: bool} — slant the selected characters by compressing them across a direction",
            has_text_or_frames,
            shatai_cmd
        ),
        cmd!(
            "type.autoTateChuYoko",
            "Auto Tate-Chu-Yoko",
            [],
            None,
            "{digits?: 0..10 (0 = off), includeRoman?: bool} — in vertical text, set runs of up to `digits` half-width digits (and roman letters) across one em, for the selected paragraphs",
            has_text_or_frames,
            auto_tcy_cmd
        ),
        cmd!(
            "type.gridSettings",
            "Grid Settings",
            [],
            None,
            "{alignment?: none|romanBaseline|emTop|emCenter|emBottom|icfTop|icfBottom, firstLineOnly?: bool, gyoudori?: 0..100 (0 = auto), paragraphGyoudori?: bool} — how the selected paragraphs sit on the baseline grid",
            has_text_or_frames,
            grid_settings_cmd
        ),
        cmd!(
            "type.rotateRoman",
            "Rotate Roman Characters in Vertical Text",
            [],
            None,
            "{on?: bool} — half-width characters stand upright in vertical text, for the selected paragraphs (toggles by default)",
            has_text_or_frames,
            |s, p| para_toggle(s, p, "type.rotateRoman", "rotateRoman", |pp| pp.rotate_roman)
        ),
        cmd!(
            "type.romanWordBreak",
            "Roman Word Break",
            [],
            None,
            "{on?: bool} — roman words in the selected paragraphs may break between any two letters, without a hyphen (toggles by default)",
            has_text_or_frames,
            |s, p| para_toggle(s, p, "type.romanWordBreak", "romanWordBreak", |pp| pp.roman_word_break)
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
        cmd!(
            "type.kernStep",
            "Kerning/Tracking Step",
            [],
            None,
            "{steps?: n (default 1; negative tightens), amount?: 1/1000 em (in place of steps × the document's kerningIncrement)} — with a caret, the manual kerning between the characters at the caret (from the pair's manual value, else 0); with a selection, the tracking of the selected text",
            has_text_or_frames,
            kern_step
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
        cmd!(
            query "type.selectionAttrs",
            "Selection Attributes",
            [],
            None,
            "{mixed?: bool} → resolved character/paragraph attributes at the text selection; `ownChars`: what the text has of its own over its paragraph (its character style's attributes and its overrides, what a character style made from the selection sets); with mixed, paraMixed lists the paragraph attributes that differ across the selected paragraphs; the selection's start as `selectedText` (one line, up to 60 characters); `storyOptions` {opticalMargin, opticalMarginSize} of the stories story.options would change (null where they differ)",
            has_doc,
            selection_attrs
        ),
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
