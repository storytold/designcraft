use super::asian::bool_param;
use super::caret::hit_byte_with;
use super::story::selection_attrs;
use super::{
    CellAddr, CharAttrs, ParaAttrs, Result, Session, StoryId, TextSel, Value, bad, compose, floor_char_boundary, id_param, json, ok, point_param,
};

/// The Eyedropper with text selected: the formatting of the text under `point` in `frame` goes to
/// the selection, as InDesign's does (character style and overrides on the selected characters,
/// paragraph style and overrides on their paragraphs).
pub(super) fn pick_up_format(s: &mut Session, p: &Value) -> Result<Value> {
    const ID: &str = "type.pickUpFormat";
    let frame = id_param(p, "frame").ok_or_else(|| bad(ID, "missing frame"))?;
    let pt = point_param(p, "point").ok_or_else(|| bad(ID, "missing point"))?;
    let (sid, b, cell) = hit_byte_with(s, frame, pt, compose::hit_char).ok_or_else(|| bad(ID, "not a text frame"))?;
    let st = s.doc()?;
    let sel = st.selection.text.ok_or_else(|| bad(ID, "select the text to format"))?;
    let src = st.doc.text_story(sid, cell).ok_or(designcraft_doc::DocError::NoStory(sid))?;
    let b = floor_char_boundary(&src.text, b);
    // The character under the pointer (at the end of a line, the one before it).
    let at = if b >= src.text.len() || src.text[b..].starts_with('\n') { b.saturating_sub(1) } else { b };
    let at = floor_char_boundary(&src.text, at);
    let chars = src.char_format_at(at).clone();
    let para = src.paras.get(src.para_at(at)).cloned().ok_or(designcraft_doc::DocError::NoStory(sid))?;
    let (a, e) = (sel.anchor.min(sel.focus), sel.anchor.max(sel.focus));
    s.typing_format = None;
    s.edit(|d, _| {
        let story = d.text_story_mut(sel.story, sel.cell).ok_or(designcraft_doc::DocError::NoStory(sel.story))?;
        if e > a {
            story.format_chars(a..e, |f| *f = chars.clone());
        }
        story.format_paras(a..e, |f| {
            f.style = para.style.clone();
            f.para = para.para.clone();
        });
        Ok(json!({"style": chars.style, "paragraphStyle": para.style}))
    })
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
pub(super) fn typing_format(s: &Session) -> Option<&TypingFormat> {
    let st = s.active()?;
    let t = st.selection.text?;
    s.typing_format.as_ref().filter(|f| f.doc == st.uid && f.at.story == t.story && f.at.cell == t.cell && f.at.range() == t.range())
}

/// Is `flag` on in the character format at the start of the first formatting target?
pub(super) fn char_flag_on(s: &Session, flag: impl Fn(&CharAttrs) -> Option<bool>) -> bool {
    format_targets(s).first().is_some_and(|t| {
        s.doc()
            .ok()
            .and_then(|d| d.doc.text_story(t.story, t.cell))
            .is_some_and(|st| flag(&st.char_format_at(if t.range.is_empty() { t.range.start } else { t.range.start + 1 }).over) == Some(true))
    })
}

/// Switch a paragraph attribute of the selected paragraphs: `on`, else the opposite of the
/// first one's.
pub(super) fn para_toggle(s: &mut Session, p: &Value, cmd: &str, attr: &str, get: impl Fn(&designcraft_doc::ParaProps) -> bool) -> Result<Value> {
    let on = match bool_param(p, "on", cmd)? {
        Some(v) => v,
        None => !format_targets(s).first().is_some_and(|t| {
            s.doc().ok().is_some_and(|d| {
                d.doc
                    .text_story(t.story, t.cell)
                    .and_then(|st| st.paras.get(st.para_at(t.range.start)))
                    .is_some_and(|pf| get(&d.doc.styles.resolve_para(pf).0))
            })
        }),
    };
    let mut body = serde_json::Map::new();
    body.insert(attr.to_string(), json!(on));
    format_paras(s, &Value::Object(body))
}

/// A character fill must name a swatch of the document.
pub(crate) fn validate_char_fill(document: &designcraft_doc::Document, attrs: &CharAttrs, command_id: &str) -> Result<()> {
    validate_char_size(attrs, command_id)?;
    match &attrs.fill {
        Some(fill) => crate::cmd::object::known_swatch(document, fill, command_id, "fill"),
        None => Ok(()),
    }
}

/// Type size is 0.1 to 1296 pt, the range this engine already applies to a ruby annotation's
/// size. Nothing checked it for ordinary text, so `{"size": 0}` set type with no size, a negative
/// size was accepted as well, and 1e9 pt asked the composer for a line a million times the page.
pub(crate) fn validate_char_size(attrs: &CharAttrs, command_id: &str) -> Result<()> {
    match attrs.size {
        Some(size) if !(MIN_TYPE_SIZE..=MAX_TYPE_SIZE).contains(&size) => {
            Err(bad(command_id, format!("size {size}: {MIN_TYPE_SIZE} to {MAX_TYPE_SIZE} pt")))
        }
        _ => Ok(()),
    }
}

/// Type size limits (InDesign's, and the ones `type.ruby` already uses).
pub(crate) const MIN_TYPE_SIZE: f64 = 0.1;

pub(crate) const MAX_TYPE_SIZE: f64 = 1296.0;

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
    // A caret in text: the format is for the next typing. On an empty paragraph's hidden mark it
    // also formats the mark right away, since the mark alone sets that line's height; typing there
    // still needs the typing format, as new text takes the format of the run before the caret
    // (the previous paragraph's mark).
    if let [t] = targets.as_slice()
        && t.range.is_empty()
        && let Some(at) = s.doc()?.selection.text
        && let Some(on_mark) = s.doc()?.doc.text_story(t.story, t.cell).filter(|st| !st.is_empty()).map(|st| empty_para_mark(st, t.range.start))
    {
        let doc = s.doc()?.uid;
        let mut f = typing_format(s).cloned().unwrap_or(TypingFormat { doc, at, attrs: CharAttrs::default(), cleared: vec![] });
        f.attrs.merge(&a);
        f.cleared.retain(|k| attrs.get(k).is_none_or(|v| v.is_null()) && !cleared.contains(k));
        f.cleared.extend(cleared.iter().cloned());
        if on_mark {
            s.edit(|d, _| {
                apply_char_attrs(d, &targets, &a, &cleared);
                ok()
            })?;
        }
        s.typing_format = Some(f);
        return ok();
    }
    s.edit(|d, _| {
        apply_char_attrs(d, &targets, &a, &cleared);
        ok()
    })
}

/// Kerning spaces a character from the next one, so the pair at a caret is kerned through the
/// character before it. With no such pair (story or paragraph start, a selection) this is
/// `type.char`.
pub(super) fn kern_pair(s: &mut Session, p: &Value) -> Result<Value> {
    const ID: &str = "type.kernPair";
    let kerning = p.get("kerning").ok_or_else(|| bad(ID, "missing `kerning`"))?;
    let targets = format_targets(s);
    if let [t] = targets.as_slice()
        && t.range.is_empty()
        && s.doc()?.selection.text.is_some()
        && let Some(start) = s.doc()?.doc.text_story(t.story, t.cell).and_then(|st| pair_start(&st.text, t.range.start))
    {
        let mut a = CharAttrs::default();
        a.set_json("kerning", kerning).map_err(|e| bad(ID, e))?;
        let cleared = if kerning.is_null() { vec!["kerning".to_string()] } else { vec![] };
        let pair = Target { story: t.story, cell: t.cell, range: start..t.range.start };
        // Kerning chosen earlier for typing at this caret would hide the pair's value.
        if typing_format(s).is_some()
            && let Some(f) = &mut s.typing_format
        {
            f.attrs.kerning = None;
            f.cleared.retain(|k| k != "kerning");
        }
        return s.edit(|d, _| {
            apply_char_attrs(d, &[pair], &a, &cleared);
            ok()
        });
    }
    format_chars(s, &json!({ "kerning": kerning }))
}

/// Byte offset of the character before `pos` when it starts a kerning pair with the character at
/// `pos`: both exist and neither is a paragraph or line break.
fn pair_start(text: &str, pos: usize) -> Option<usize> {
    let is_text = |c: &char| !matches!(*c, '\n' | designcraft_doc::story::FORCED_LINE_BREAK);
    text.get(pos..)?.chars().next().filter(is_text)?;
    let c = text.get(..pos)?.chars().next_back().filter(is_text)?;
    pos.checked_sub(c.len_utf8())
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
                    st.rev = st.rev.wrapping_add(1);
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
    crate::cmd::style::check_grep_styles("type.para", &a)?;
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

/// Alt+Left/Right in text: kerning at a caret, tracking of a selection, by the Kerning/Tracking
/// increment.
pub(super) fn kern_step(s: &mut Session, p: &Value) -> Result<Value> {
    use designcraft_doc::Kerning;
    let inc = s.doc()?.doc.settings.kerning_increment;
    let inc = if inc.is_finite() { inc.clamp(1.0, 1000.0) } else { 20.0 };
    let amount = match p.get("amount").and_then(Value::as_f64) {
        Some(a) => a,
        None => p.get("steps").and_then(Value::as_f64).unwrap_or(1.0) * inc,
    };
    if !amount.is_finite() {
        return Err(bad("type.kernStep", "`amount` must be a number"));
    }
    let amount = amount.clamp(-11000.0, 11000.0);
    // Kerning and tracking range, in 1/1000 em.
    let clamp = |v: f64| v.clamp(-1000.0, 10000.0);
    let targets = format_targets(s);
    s.edit(|d, _| {
        let styles = d.styles.clone();
        let mut out = json!({});
        for t in &targets {
            let Some(st) = d.text_story_mut(t.story, t.cell) else { continue };
            if t.range.is_empty() {
                // Manual kerning sits on the character before the caret (it applies after it).
                let pos = t.range.start;
                let Some(c) = st.text.get(..pos).and_then(|before| before.chars().next_back()) else { continue };
                let at = pos - c.len_utf8();
                let para = st.paras.get(st.para_at(at)).cloned().unwrap_or_default();
                let current = styles.resolve_char(&styles.resolve_para(&para).1, st.format_after(at)).kerning;
                // The font's metrics or optical value isn't known here: those start from 0.
                let v = clamp(if let Kerning::Manual(v) = current { v } else { 0.0 } + amount);
                st.format_chars(at..pos, |f| f.over.kerning = Some(Kerning::Manual(v)));
                out = json!({"kerning": v});
            } else {
                for (i, pr) in st.para_ranges().into_iter().enumerate() {
                    let r = pr.start.max(t.range.start)..pr.end.min(t.range.end);
                    let Some(para) = st.paras.get(i).cloned() else { break };
                    if r.is_empty() {
                        continue;
                    }
                    let pc = styles.resolve_para(&para).1;
                    st.format_chars(r, |f| {
                        let v = clamp(styles.resolve_char(&pc, f).tracking + amount);
                        f.over.tracking = Some(v);
                        out = json!({"tracking": v});
                    });
                }
            }
        }
        Ok(out)
    })
}

pub(super) fn step_size(s: &mut Session, delta: f64) -> Result<Value> {
    let cur = selection_attrs(s, &json!({}))?;
    let size = cur["chars"]["size"].as_f64().unwrap_or(12.0);
    format_chars(s, &json!({"size": (size + delta).clamp(0.1, 1296.0)}))
}

pub(super) fn break_link(s: &mut Session, p: &Value) -> Result<Value> {
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

/// The first target character, with any pending caret overrides. Keep character-style
/// inheritance separate from paragraph defaults for style creation from the selection.
pub(crate) fn effective_char_format(s: &Session, target: &Target) -> Result<designcraft_doc::CharFormat> {
    let story = s.doc()?.doc.text_story(target.story, target.cell).ok_or(designcraft_doc::DocError::NoStory(target.story))?;
    let mut format =
        if target.range.is_empty() { story.insert_format_at(target.range.start) } else { story.format_after(target.range.start) }.clone();
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
pub(super) fn swap_fill_stroke(s: &mut Session, _p: &Value) -> Result<Value> {
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

pub(super) fn apply_typing_format(cf: &mut designcraft_doc::CharFormat, f: &TypingFormat) {
    cf.over.merge(&f.attrs);
    for k in &f.cleared {
        let _ = cf.over.set_json(k, &Value::Null);
    }
}

/// Bold (`bold`) or italic from the font family (style linking), per run of the selection: on or
/// off as asked, else the opposite of the first selected character.
pub(super) fn link_style(s: &mut Session, p: &Value, bold: bool) -> Result<Value> {
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

pub(crate) fn format_targets_pub(s: &Session) -> Vec<Target> {
    format_targets(s)
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
        p.execute("file.place", &json!({"base64": crate::cmd::file::base64_encode(&pdf), "name": "text.pdf", "x": 0, "y": 0, "width": 300})).unwrap();
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
        let g = s
            .execute("swatch.create", &json!({"gradient": {"stops": [{"location": 0, "color": "#ff0000"}, {"location": 100, "color": "#0000ff"}]}}))
            .unwrap()["name"]
            .as_str()
            .unwrap()
            .to_string();
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
        p.execute("file.place", &json!({"base64": crate::cmd::file::base64_encode(&pdf), "name": "cell.pdf", "x": 0, "y": 0, "width": 300})).unwrap();
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
mod kern_pair_tests {
    use designcraft_doc::{Kerning, StoryId};
    use serde_json::json;

    use crate::Session;

    fn avatar() -> (Session, StoryId) {
        let mut s = Session::new();
        s.execute("file.new", &json!({})).unwrap();
        let r = s.execute("frame.create", &json!({"rect": [72, 72, 300, 200], "content": "text", "text": "AVATAR"})).unwrap();
        (s, StoryId(r["story"].as_u64().unwrap()))
    }

    fn kern_at(s: &Session, story: StoryId, pos: usize) -> Option<Kerning> {
        s.doc().unwrap().doc.story(story).unwrap().format_after(pos).over.kerning
    }

    #[test]
    fn kerning_at_a_caret_kerns_the_pair_in_one_undo_step() {
        let (mut s, sid) = avatar();
        s.execute("text.select", &json!({"story": sid.0, "anchor": 1, "focus": 1})).unwrap();
        let steps = s.doc().unwrap().history.undo.len();
        s.execute("type.kernPair", &json!({"kerning": {"kind": "manual", "value": -80}})).unwrap();
        assert_eq!(kern_at(&s, sid, 0), Some(Kerning::Manual(-80.0)), "A, before the caret, spaces the A|V pair");
        assert_eq!(kern_at(&s, sid, 1), None);
        assert_eq!(s.doc().unwrap().history.undo.len(), steps + 1);
        let shown = s.execute("type.selectionAttrs", &json!({})).unwrap()["chars"]["kerning"].clone();
        assert_eq!(shown, json!({"kind": "manual", "value": -80.0}), "the caret shows the pair's kerning");
        s.execute("edit.undo", &json!({})).unwrap();
        assert_eq!(kern_at(&s, sid, 0), None);
    }

    #[test]
    fn kerning_a_selection_or_at_the_story_start_works_like_type_char() {
        let (mut s, sid) = avatar();
        s.execute("text.select", &json!({"story": sid.0, "anchor": 2, "focus": 4})).unwrap();
        s.execute("type.kernPair", &json!({"kerning": {"kind": "manual", "value": 20}})).unwrap();
        let kerned: Vec<_> = (0..6).map(|i| kern_at(&s, sid, i)).collect();
        let m = Some(Kerning::Manual(20.0));
        assert_eq!(kerned, [None, None, m, m, None, None]);
        // No character before the caret: the value is for the next typing.
        s.execute("text.select", &json!({"story": sid.0, "anchor": 0, "focus": 0})).unwrap();
        let steps = s.doc().unwrap().history.undo.len();
        s.execute("type.kernPair", &json!({"kerning": {"kind": "none"}})).unwrap();
        assert_eq!(s.doc().unwrap().history.undo.len(), steps);
        assert_eq!(kern_at(&s, sid, 0), None);
        s.execute("text.insert", &json!({"text": "X"})).unwrap();
        assert_eq!(kern_at(&s, sid, 0), Some(Kerning::None));
    }

    #[test]
    fn kerning_at_the_end_of_a_paragraph_or_story_leaves_the_text_alone() {
        let mut s = Session::new();
        s.execute("file.new", &json!({})).unwrap();
        let r = s.execute("frame.create", &json!({"rect": [72, 72, 300, 200], "content": "text", "text": "AV\nTA"})).unwrap();
        let sid = StoryId(r["story"].as_u64().unwrap());
        for caret in [2, 5] {
            s.execute("text.select", &json!({"story": sid.0, "anchor": caret, "focus": caret})).unwrap();
            let steps = s.doc().unwrap().history.undo.len();
            s.execute("type.kernPair", &json!({"kerning": {"kind": "manual", "value": -80}})).unwrap();
            assert_eq!(s.doc().unwrap().history.undo.len(), steps, "no pair at {caret}: nothing is edited");
            assert_eq!((kern_at(&s, sid, 1), kern_at(&s, sid, 4)), (None, None), "caret {caret}");
        }
    }
}

#[cfg(test)]
mod pick_up_format_tests {
    use designcraft_doc::ItemId;
    use serde_json::json;

    use crate::Session;

    /// Typefool's video (12:04): select body text, Eyedropper, click the third header: the body
    /// text takes the header's font, size and paragraph settings. DesignCraft copied the frame's
    /// fill and stroke instead.
    #[test]
    fn the_eyedropper_copies_text_formatting_to_selected_text() {
        let mut s = Session::new();
        s.execute("file.new", &json!({})).unwrap();
        let head = s.execute("frame.create", &json!({"rect": [36, 36, 400, 100], "content": "text", "text": "Innovative Design"})).unwrap();
        s.execute("text.select", &json!({"story": head["story"], "anchor": 0, "focus": 17})).unwrap();
        s.execute("type.char", &json!({"attrs": {"fontStyle": "Bold", "size": 18, "tracking": 20}})).unwrap();
        s.execute("type.para", &json!({"attrs": {"align": "center", "spaceAfter": 6}})).unwrap();
        let body = s.execute("frame.create", &json!({"rect": [36, 200, 400, 300], "content": "text", "text": "Body copy here.\nSecond."})).unwrap();
        s.execute("text.select", &json!({"story": body["story"], "anchor": 0, "focus": 4})).unwrap();
        // Click the header's middle (the frame's text starts at its top left).
        let r = s.execute("type.pickUpFormat", &json!({"frame": head["id"], "point": [100, 50]})).unwrap();
        assert!(r.is_object(), "{r}");
        s.execute("text.select", &json!({"story": body["story"], "anchor": 0, "focus": 4})).unwrap();
        let a = s.execute("type.selectionAttrs", &json!({})).unwrap();
        assert_eq!((&a["chars"]["fontStyle"], &a["chars"]["size"], &a["chars"]["tracking"]), (&json!("Bold"), &json!(18.0), &json!(20.0)), "{a}");
        assert_eq!((&a["para"]["align"], &a["para"]["spaceAfter"]), (&json!("center"), &json!(6.0)));
        // Only the selected characters change; the second paragraph keeps its own.
        s.execute("text.select", &json!({"story": body["story"], "anchor": 6, "focus": 10})).unwrap();
        assert_eq!(s.execute("type.selectionAttrs", &json!({})).unwrap()["chars"]["size"], json!(12.0));
        s.execute("text.select", &json!({"story": body["story"], "anchor": 17, "focus": 20})).unwrap();
        assert_eq!(s.execute("type.selectionAttrs", &json!({})).unwrap()["para"]["align"], json!("left"));
        // Not a text frame, or nothing selected: refused.
        let rect = s.execute("frame.create", &json!({"rect": [500, 500, 520, 520]})).unwrap()["id"].as_u64().unwrap();
        assert!(s.execute("type.pickUpFormat", &json!({"frame": rect, "point": [510, 510]})).is_err());
        let _ = ItemId(rect);
    }
}

#[cfg(test)]
mod size_bounds_tests {
    use serde_json::json;

    use crate::Session;

    /// Type size was not checked anywhere: `{"size": 0}` set type with no size at all, a negative
    /// size was accepted the same way, and 1e9 pt asked the composer for a line a million times
    /// the height of the page. The range is the one `type.ruby` already applies, 0.1 to 1296 pt.
    #[test]
    fn a_type_size_outside_the_range_is_refused() {
        let mut s = Session::new();
        s.execute("file.new", &json!({"pages": 1})).unwrap();
        let f = s.execute("frame.create", &json!({"rect": [20, 20, 400, 300], "content": "text", "text": "Hello world"})).unwrap();
        let select = |s: &mut Session| {
            s.execute("text.select", &json!({"story": f["story"].clone(), "anchor": 0, "focus": 5})).unwrap();
        };
        for size in [0.0, -12.0, 0.05, 1297.0, 1e9] {
            select(&mut s);
            let e = s.execute("type.char", &json!({"size": size})).expect_err("refused").to_string();
            assert!(e.contains("0.1 to 1296 pt"), "{size}: {e}");
        }
        // The sizes in range still apply, including the ends.
        for size in [0.1, 12.0, 1296.0] {
            select(&mut s);
            s.execute("type.char", &json!({"size": size})).unwrap_or_else(|e| panic!("{size}: {e}"));
            assert_eq!(s.execute("type.selectionAttrs", &json!({})).unwrap()["chars"]["size"].as_f64(), Some(size));
        }
        // And the styles that carry character attributes are checked the same way.
        for (command, params) in [
            ("style.character.create", json!({"name": "Tiny", "chars": {"size": 0}})),
            ("style.paragraph.create", json!({"name": "Huge", "chars": {"size": 2000}})),
        ] {
            let e = s.execute(command, &params).expect_err("refused").to_string();
            assert!(e.contains("0.1 to 1296 pt"), "{command}: {e}");
        }
    }
}
