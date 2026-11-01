//! Type → Text Variables: define, list, delete and insert variables.

use designcraft_doc::vars::{TextVariable, VarKind, var_char};
use serde_json::{Value, json};

use super::{CommandSpec, bad, cmd, has_doc, str_param};
use crate::{Result, Session};

pub fn specs() -> Vec<CommandSpec> {
    vec![
        cmd!(query "variables.list", "Text Variables", [], None, "{} → [{index, name, type, …, value (page 1)}]", has_doc, |s, _| {
            let d = &s.doc()?.doc;
            Ok(Value::Array(
                d.text_variables
                    .iter()
                    .enumerate()
                    .map(|(i, v)| {
                        let mut o = serde_json::to_value(v).unwrap_or_default();
                        o["index"] = json!(i);
                        o["value"] = json!(d.variable_value(i, Some(0)));
                        o
                    })
                    .collect(),
            ))
        }),
        cmd!(
            "variables.define",
            "Define Text Variable…",
            ["Type", "Text Variables"],
            None,
            "{name, type: custom|lastPageNumber|chapterNumber|fileName|creationDate|modificationDate|outputDate|runningHeader, text?, format?, style?, use?: firstOnPage|lastOnPage, character?, section?, extension?, before?, after?} — creates or replaces by name",
            has_doc,
            define
        ),
        cmd!(
            "variables.delete",
            "Delete Text Variable",
            [],
            None,
            "{name} — instances become plain text of their current value",
            has_doc,
            |s, p| {
                let name = str_param(p, "name").ok_or_else(|| bad("variables.delete", "missing name"))?.to_string();
                s.edit(|d, _| {
                    let i = d.variable(&name).ok_or_else(|| bad("variables.delete", format!("no variable `{name}`")))?;
                    // Replace instances with their value (first page context), then shift higher indices down.
                    let value = d.variable_value(i, Some(0)).unwrap_or_default();
                    let n = d.text_variables.len();
                    let ids: Vec<_> = d.stories.keys().copied().collect();
                    for sid in ids {
                        if !d.story(sid).is_some_and(|st| needs_unvar(st, i, 0)) {
                            continue;
                        }
                        let st = d.story_mut(sid).ok_or(designcraft_doc::DocError::NoStory(sid))?;
                        unvar(st, i, n, &value, 0);
                    }
                    d.text_variables.remove(i);
                    Ok(Value::Null)
                })
            }
        ),
        cmd!(
            "variables.insert",
            "Insert Variable",
            ["Type", "Text Variables"],
            None,
            "{name} — at the text insertion point",
            super::has_text,
            |s, p| {
                let name = str_param(p, "name").ok_or_else(|| bad("variables.insert", "missing name"))?;
                let i = s.doc()?.doc.variable(name).ok_or_else(|| bad("variables.insert", format!("no variable `{name}`")))?;
                let c = var_char(i).ok_or_else(|| bad("variables.insert", "too many variables"))?;
                s.execute("text.insert", &json!({"text": c.to_string()}))
            }
        ),
        cmd!(
            "variables.convertToText",
            "Convert Variable to Text",
            ["Type", "Text Variables"],
            None,
            "{} — the variable instances in the selected text become plain text of the value each shows on its page; the variables stay defined",
            has_selected_instance,
            convert_selection
        ),
        cmd!(
            "variables.convertAllToText",
            "Convert to Text",
            [],
            None,
            "{name} — every instance of the variable in the document becomes plain text of the value it shows on its page (Text Variables › Convert to Text); the variable stays defined",
            has_doc,
            convert_all
        ),
    ]
}

/// The selected text holds a variable instance (InDesign: select the variable, then Convert
/// Variable to Text). The endnote story is rebuilt from the stories' endnotes, so an edit there
/// would not last.
fn has_selected_instance(s: &Session) -> std::result::Result<(), String> {
    has_doc(s)?;
    let st = s.active().ok_or("no document")?;
    let t = st.selection.text.ok_or("no text selected")?;
    let held = st.doc.endnote_story != Some(t.story)
        && st
            .doc
            .text_story(t.story, t.cell)
            .and_then(|x| x.text.get(t.range()))
            .is_some_and(|x| x.chars().any(|c| designcraft_doc::vars::var_index(c).is_some()));
    if held { Ok(()) } else { Err("no text variable selected".into()) }
}

/// The page story byte `pos` is shown on, which a variable's value depends on: overset text takes
/// the page of the story's last frame; parent pages and the pasteboard have none.
fn shown_page(d: &designcraft_doc::Document, cs: &designcraft_compose::ComposedStory, pos: usize) -> Option<usize> {
    designcraft_compose::vars::page_of(d, cs, pos).or_else(|| cs.frames.last().and_then(|f| d.page_of_item(f.frame)))
}

/// Values of every variable on the pages asked for, as composition shows them (running headers
/// included), computed once per page.
struct PageValues<'a> {
    doc: &'a designcraft_doc::Document,
    running: std::sync::Arc<designcraft_compose::vars::RunningIndex>,
    pages: std::collections::HashMap<Option<usize>, Vec<String>>,
}

impl PageValues<'_> {
    fn get(&mut self, i: usize, page: Option<usize>) -> String {
        let (doc, running) = (self.doc, &self.running);
        let v = self.pages.entry(page).or_insert_with(|| designcraft_compose::vars::values(doc, page, Some(running)));
        v.get(i).cloned().unwrap_or_default()
    }
}

/// Byte offsets of the variable instances in `range` of `text` (of variable `which`, or any),
/// with their variable index.
fn instances(text: &str, range: std::ops::Range<usize>, which: Option<usize>) -> Vec<(usize, usize)> {
    let Some(t) = text.get(range.clone()) else { return Vec::new() };
    t.char_indices()
        .filter_map(|(k, c)| designcraft_doc::vars::var_index(c).filter(|i| which.is_none_or(|w| w == *i)).map(|i| (range.start + k, i)))
        .collect()
}

/// Replaces the instances at `at` (byte, variable) in `st` with `value(byte)`, last first so the
/// earlier offsets hold. Returns the change in length before each replaced byte, in text order.
fn replace_instances(st: &mut designcraft_doc::Story, at: &[(usize, usize)], value: &dyn Fn(usize) -> String) -> Vec<(usize, isize)> {
    let mut grown = Vec::new();
    for &(pos, i) in at.iter().rev() {
        let len = var_char(i).map_or(0, char::len_utf8);
        let v = value(pos);
        st.replace(pos..pos + len, &v);
        grown.push((pos, v.len() as isize - len as isize));
    }
    grown.reverse();
    grown
}

/// Moves byte `b` of a story by the growth of the instances replaced before it.
fn shifted(b: usize, grown: &[(usize, isize)]) -> usize {
    let d: isize = grown.iter().filter(|(pos, _)| *pos < b).map(|(_, g)| g).sum();
    b.saturating_add_signed(d)
}

/// Replaces every instance of variable `i` in `st`, its table cells and its notes with `value`.
/// Returns the growth of `st`'s own text, as [`replace_instances`].
fn replace_nested(st: &mut designcraft_doc::Story, i: usize, value: &str, depth: usize) -> Vec<(usize, isize)> {
    if depth > MAX_DEPTH {
        return Vec::new();
    }
    let at = instances(&st.text, 0..st.text.len(), Some(i));
    let grown = replace_instances(st, &at, &|_| value.to_string());
    let mut nested = false;
    for t in st.tables.values_mut() {
        if t.cells.iter().any(|c| holds(&c.text, i, depth + 1)) {
            for c in &mut std::sync::Arc::make_mut(t).cells {
                replace_nested(&mut c.text, i, value, depth + 1);
            }
            nested = true;
        }
    }
    for note in st.notes.iter_mut().chain(st.endnotes.iter_mut()) {
        if holds(&note.text, i, depth + 1) {
            replace_nested(&mut std::sync::Arc::make_mut(note).text, i, value, depth + 1);
            nested = true;
        }
    }
    if nested {
        st.rev = st.rev.wrapping_add(1);
    }
    grown
}

/// Whether `st` or text nested in it holds an instance of variable `i`.
fn holds(st: &designcraft_doc::Story, i: usize, depth: usize) -> bool {
    depth <= MAX_DEPTH
        && (st.text.chars().any(|c| designcraft_doc::vars::var_index(c) == Some(i))
            || st.tables.values().any(|t| t.cells.iter().any(|c| holds(&c.text, i, depth + 1)))
            || st.notes.iter().chain(st.endnotes.iter()).any(|n| holds(&n.text, i, depth + 1)))
}

/// Byte offset in `st` of the anchor of table `id`.
fn table_anchor(st: &designcraft_doc::Story, id: u64) -> Option<usize> {
    let (pi, _) = st.paras.iter().enumerate().find(|(_, p)| p.table == Some(id))?;
    let r = st.para_ranges().into_iter().nth(pi)?;
    Some(r.start + st.text.get(r)?.find(designcraft_doc::story::TABLE_ANCHOR)?)
}

fn convert_selection(s: &mut Session, _: &Value) -> Result<Value> {
    const ID: &str = "variables.convertToText";
    let t = s.doc()?.selection.text.ok_or_else(|| bad(ID, "no text selected"))?;
    let range = t.range();
    let (at, values) = {
        let d = &s.doc()?.doc;
        let st = d.story(t.story).ok_or(designcraft_doc::DocError::NoStory(t.story))?;
        let text = d.text_story(t.story, t.cell).ok_or(designcraft_doc::DocError::NoStory(t.story))?;
        // Text in a cell or a footnote is on the page of its table's or note's anchor.
        let anchor = match t.cell {
            None => None,
            Some(c) if c.table == designcraft_doc::FOOTNOTE_TABLE => st.note_anchor(c.row as u64),
            Some(c) => table_anchor(st, c.table),
        };
        let cs = s.cache.get(d, t.story, None);
        let mut pv = PageValues { doc: d, running: s.cache.running_index(d), pages: Default::default() };
        let at = instances(&text.text, range.clone(), None);
        let values: std::collections::HashMap<usize, String> =
            at.iter().map(|&(pos, i)| (pos, pv.get(i, shown_page(d, &cs, anchor.unwrap_or(pos))))).collect();
        (at, values)
    };
    if at.is_empty() {
        return Err(bad(ID, "no text variable selected"));
    }
    s.edit(|d, sel| {
        let st = d.text_story_mut(t.story, t.cell).ok_or(designcraft_doc::DocError::NoStory(t.story))?;
        let grown = replace_instances(st, &at, &|pos| values.get(&pos).cloned().unwrap_or_default());
        // The converted text stays selected.
        sel.text = Some(designcraft_doc::TextSel { anchor: range.start, focus: shifted(range.end, &grown), ..t });
        Ok(json!({"converted": at.len()}))
    })
}

/// What converting one story's instances of a variable writes: the value for each instance in
/// its own text, and for the text of each table (by id), footnote and endnote, the value on the
/// page of its anchor.
struct StoryPlan {
    sid: designcraft_doc::StoryId,
    own: Vec<(usize, String)>,
    tables: Vec<(u64, String)>,
    notes: Vec<String>,
    endnotes: Vec<String>,
}

fn convert_all(s: &mut Session, p: &Value) -> Result<Value> {
    const ID: &str = "variables.convertAllToText";
    let name = str_param(p, "name").ok_or_else(|| bad(ID, "missing name"))?;
    let (i, plans) = {
        let d = &s.doc()?.doc;
        let i = d.variable(name).ok_or_else(|| bad(ID, format!("no variable `{name}`")))?;
        let mut pv = PageValues { doc: d, running: s.cache.running_index(d), pages: Default::default() };
        let mut plans = Vec::new();
        for (sid, st) in &d.stories {
            if Some(*sid) == d.endnote_story || !holds(st, i, 0) {
                continue;
            }
            let cs = s.cache.get(d, *sid, None);
            let mut value = |pos: usize| pv.get(i, shown_page(d, &cs, pos));
            let own = instances(&st.text, 0..st.text.len(), Some(i)).into_iter().map(|(pos, _)| (pos, value(pos))).collect();
            let tables = st.tables.keys().map(|id| (*id, table_anchor(st, *id).map(&mut value).unwrap_or_default())).collect();
            let notes = st.text.match_indices(designcraft_doc::FOOTNOTE_REF).map(|(k, _)| value(k)).collect();
            let endnotes = st.text.match_indices(designcraft_doc::ENDNOTE_REF).map(|(k, _)| value(k)).collect();
            plans.push(StoryPlan { sid: *sid, own, tables, notes, endnotes });
        }
        (i, plans)
    };
    s.edit(|d, sel| {
        for plan in &plans {
            convert_plan(d, sel, i, plan)?;
        }
        Ok(Value::Null)
    })
}

/// Converts one story's instances of variable `i` as planned, keeping the selection in place.
fn convert_plan(d: &mut designcraft_doc::Document, sel: &mut designcraft_doc::Selection, i: usize, plan: &StoryPlan) -> Result<()> {
    // A selection in a table cell or a footnote keeps its place in that text: it is
    // converted first, so the loop below finds nothing left there.
    if let Some(t) = sel.text.as_mut().filter(|t| t.story == plan.sid)
        && let Some(c) = t.cell
    {
        let value = match c.footnote_id() {
            Some(id) => d.story(plan.sid).and_then(|st| st.notes.iter().position(|n| n.id == id)).and_then(|k| plan.notes.get(k)),
            None => plan.tables.iter().find(|(id, _)| *id == c.table).map(|(_, v)| v),
        };
        if let Some(v) = value
            && let Some(text) = d.text_story_mut(plan.sid, Some(c))
        {
            let grown = replace_nested(text, i, v, 1);
            t.anchor = shifted(t.anchor, &grown);
            t.focus = shifted(t.focus, &grown);
        }
    }
    let st = d.story_mut(plan.sid).ok_or(designcraft_doc::DocError::NoStory(plan.sid))?;
    let at: Vec<(usize, usize)> = plan.own.iter().map(|(pos, _)| (*pos, i)).collect();
    let grown = replace_instances(st, &at, &|pos| plan.own.iter().find(|(k, _)| *k == pos).map(|(_, v)| v.clone()).unwrap_or_default());
    if replace_in_tables_and_notes(st, i, plan) {
        st.rev = st.rev.wrapping_add(1);
    }
    // A selection in the story keeps its place in the text.
    if let Some(t) = sel.text.as_mut().filter(|t| t.story == plan.sid && t.cell.is_none()) {
        t.anchor = shifted(t.anchor, &grown);
        t.focus = shifted(t.focus, &grown);
    }
    Ok(())
}

/// Replaces variable `i` in the tables and foot/endnotes of `st` that hold it. Returns whether any did.
fn replace_in_tables_and_notes(st: &mut designcraft_doc::Story, i: usize, plan: &StoryPlan) -> bool {
    let mut nested = false;
    for (id, v) in &plan.tables {
        if let Some(t) = st.tables.get_mut(id)
            && t.cells.iter().any(|c| holds(&c.text, i, 1))
        {
            for c in &mut std::sync::Arc::make_mut(t).cells {
                replace_nested(&mut c.text, i, v, 1);
            }
            nested = true;
        }
    }
    for (note, v) in st.notes.iter_mut().zip(&plan.notes).chain(st.endnotes.iter_mut().zip(&plan.endnotes)) {
        if holds(&note.text, i, 1) {
            replace_nested(&mut std::sync::Arc::make_mut(note).text, i, v, 1);
            nested = true;
        }
    }
    nested
}

/// Nesting limit for tables in cells and notes (hostile documents).
const MAX_DEPTH: usize = 32;

/// Whether `st` or a table cell or foot/endnote nested in it holds an instance of variable `i`
/// or a later variable.
fn needs_unvar(st: &designcraft_doc::Story, i: usize, depth: usize) -> bool {
    if depth > MAX_DEPTH {
        return false;
    }
    st.text.chars().any(|c| designcraft_doc::vars::var_index(c).is_some_and(|k| k >= i))
        || st.tables.values().any(|t| t.cells.iter().any(|c| needs_unvar(&c.text, i, depth + 1)))
        || st.notes.iter().chain(st.endnotes.iter()).any(|n| needs_unvar(&n.text, i, depth + 1))
}

/// Replaces variable `i`'s instances in `st` with `value` and moves the instances of variables
/// `i + 1..n` down by one, including table cells and foot/endnotes nested in the story. Only
/// touches (and unshares) the parts that hold such instances; a story whose nested text changed
/// gets a new revision.
fn unvar(st: &mut designcraft_doc::Story, i: usize, n: usize, value: &str, depth: usize) {
    if depth > MAX_DEPTH {
        return;
    }
    if st.text.chars().any(|c| designcraft_doc::vars::var_index(c).is_some_and(|k| k >= i)) {
        let target = var_char(i).unwrap_or(' ').to_string();
        while let Some(pos) = st.text.find(&target) {
            st.replace(pos..pos + target.len(), value);
        }
        for k in i + 1..n {
            let (from, to) = (var_char(k).unwrap_or(' ').to_string(), var_char(k - 1).unwrap_or(' ').to_string());
            while let Some(pos) = st.text.find(&from) {
                st.replace(pos..pos + from.len(), &to);
            }
        }
    }
    let mut nested = false;
    for t in st.tables.values_mut() {
        if !t.cells.iter().any(|c| needs_unvar(&c.text, i, depth + 1)) {
            continue;
        }
        for c in &mut std::sync::Arc::make_mut(t).cells {
            if needs_unvar(&c.text, i, depth + 1) {
                unvar(&mut c.text, i, n, value, depth + 1);
            }
        }
        nested = true;
    }
    for note in st.notes.iter_mut().chain(st.endnotes.iter_mut()) {
        if needs_unvar(&note.text, i, depth + 1) {
            unvar(&mut std::sync::Arc::make_mut(note).text, i, n, value, depth + 1);
            nested = true;
        }
    }
    if nested {
        st.rev = st.rev.wrapping_add(1);
    }
}

fn define(s: &mut Session, p: &Value) -> Result<Value> {
    let name = str_param(p, "name").ok_or_else(|| bad("variables.define", "missing name"))?.to_string();
    let fmt = || str_param(p, "format").unwrap_or("MM/dd/yy").to_string();
    let kind = match str_param(p, "type").unwrap_or("custom") {
        "custom" => VarKind::Custom { text: str_param(p, "text").unwrap_or("").into() },
        "lastPageNumber" => VarKind::LastPageNumber { section: p.get("section").and_then(Value::as_bool).unwrap_or(false) },
        "chapterNumber" => VarKind::ChapterNumber,
        "fileName" => VarKind::FileName { extension: p.get("extension").and_then(Value::as_bool).unwrap_or(false) },
        "creationDate" => VarKind::CreationDate { format: fmt() },
        "modificationDate" => VarKind::ModificationDate { format: fmt() },
        "outputDate" => VarKind::OutputDate { format: fmt() },
        "runningHeader" => VarKind::RunningHeader {
            style: str_param(p, "style").ok_or_else(|| bad("variables.define", "runningHeader needs `style`"))?.into(),
            use_: if str_param(p, "use") == Some("lastOnPage") { designcraft_doc::vars::Use::LastOnPage } else { Default::default() },
            character: p.get("character").and_then(Value::as_bool).unwrap_or(false),
        },
        t => return Err(bad("variables.define", format!("unknown type `{t}`"))),
    };
    let mut v = TextVariable::new(&name, kind);
    v.before = str_param(p, "before").unwrap_or("").into();
    v.after = str_param(p, "after").unwrap_or("").into();
    s.edit(|d, _| {
        let i = match d.variable(&name) {
            Some(i) => {
                d.text_variables[i] = v.clone();
                i
            }
            None => {
                if d.text_variables.len() >= designcraft_doc::vars::VAR_MAX {
                    return Err(bad("variables.define", "too many variables"));
                }
                d.text_variables.push(v.clone());
                d.text_variables.len() - 1
            }
        };
        Ok(json!({"index": i}))
    })
}

#[cfg(test)]
mod tests;
