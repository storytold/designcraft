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
mod tests {
    use super::*;

    /// A parent-page page-number marker follows Numbering & Section Options: changing a section's
    /// start number renames its pages without changing their index or the number of sections, and
    /// the marker composed for a page shows the new name.
    #[test]
    fn parent_page_number_follows_section_renumbering() {
        let mut s = Session::new();
        s.execute("file.new", &json!({"pages": 3, "facingPages": false})).unwrap();
        let marker = designcraft_doc::story::PAGE_NUMBER.to_string();
        let r = s
            .execute("frame.create", &json!({"spread": {"kind": "parent", "index": 0}, "rect": [72, 30, 500, 60], "content": "text", "text": marker}))
            .unwrap();
        let sid = s.doc().unwrap().doc.item(designcraft_doc::ItemId(r["id"].as_u64().unwrap())).unwrap().text_frame().unwrap().story;
        assert_eq!(s.doc().unwrap().doc.story(sid).unwrap().text, marker);
        let shown = |s: &Session, page: usize| -> usize {
            let d = s.doc().unwrap().doc.clone();
            let cs = s.cache.get(&d, sid, Some(page));
            cs.frames.iter().flat_map(|f| f.lines.iter()).map(|l| l.glyphs.iter().filter(|g| g.visible && g.adv > 0.0).count()).sum()
        };
        assert_eq!(shown(&s, 2), 1, "page 3");
        s.execute("layout.section", &json!({"page": 1, "startNumber": 100})).unwrap();
        assert_eq!(s.doc().unwrap().doc.page_name(2), "102");
        assert_eq!(shown(&s, 2), 3, "page 102 after renumbering");
    }

    /// A section marker character shows the marker text of the section of the page it is composed
    /// for: on each document page a parent-page frame is shown on, and in a frame on a document
    /// page. A section without marker text shows nothing; the parent page itself shows "Section".
    #[test]
    fn section_marker_shows_the_pages_section_marker() {
        let mut s = Session::new();
        s.execute("file.new", &json!({"pages": 4, "facingPages": false})).unwrap();
        s.execute("layout.section", &json!({"page": 3, "startNumber": 1, "marker": "Intro"})).unwrap();
        let marker = designcraft_doc::story::SECTION_MARKER.to_string();
        let r = s
            .execute("frame.create", &json!({"spread": {"kind": "parent", "index": 0}, "rect": [72, 30, 500, 60], "content": "text", "text": marker}))
            .unwrap();
        let parent = s.doc().unwrap().doc.item(designcraft_doc::ItemId(r["id"].as_u64().unwrap())).unwrap().text_frame().unwrap().story;
        let sp = s.doc().unwrap().doc.page_loc(3).unwrap().0;
        let r = s.execute("frame.create", &json!({"spread": sp, "rect": [72, 200, 400, 260], "content": "text", "text": marker})).unwrap();
        let own = s.doc().unwrap().doc.item(designcraft_doc::ItemId(r["id"].as_u64().unwrap())).unwrap().text_frame().unwrap().story;
        let shown = |s: &Session, sid, page: Option<usize>| -> usize {
            let d = s.doc().unwrap().doc.clone();
            let cs = s.cache.get(&d, sid, page);
            cs.frames.iter().flat_map(|f| f.lines.iter()).map(|l| l.glyphs.iter().filter(|g| g.visible && g.adv > 0.0).count()).sum()
        };
        assert_eq!(shown(&s, parent, Some(0)), 0, "the first section has no marker");
        assert_eq!(shown(&s, parent, Some(2)), "Intro".len());
        assert_eq!(shown(&s, parent, Some(3)), "Intro".len());
        assert_eq!(shown(&s, own, None), "Intro".len(), "a frame on a document page");
        assert_eq!(shown(&s, parent, None), "Section".len(), "the parent page itself");
        // Editing the marker recomposes.
        s.execute("layout.section", &json!({"page": 3, "marker": "Appendix"})).unwrap();
        assert_eq!(shown(&s, parent, Some(2)), "Appendix".len());
        assert_eq!(shown(&s, own, None), "Appendix".len());
    }

    /// Two sections that both number from 1 repeat page names ("1, 2, 1, 2"). A parent-page running
    /// header is composed for the page it is shown on, not the first page with that name.
    #[test]
    fn running_header_on_parent_page_with_repeated_page_numbers() {
        let mut s = Session::new();
        s.execute("file.new", &json!({"pages": 4, "facingPages": false})).unwrap();
        s.execute("layout.section", &json!({"page": 3, "startNumber": 1})).unwrap();
        let names: Vec<String> = (0..4).map(|i| s.doc().unwrap().doc.page_name(i)).collect();
        assert_eq!(names, ["1", "2", "1", "2"]);
        s.execute("style.paragraph.create", &json!({"name": "Heading"})).ok();
        // The only heading is on the third page (the second section's page 1).
        let sp = s.doc().unwrap().doc.page_loc(2).unwrap().0;
        let r = s.execute("frame.create", &json!({"spread": sp, "rect": [72, 200, 400, 260], "content": "text", "text": "Epsilon"})).unwrap();
        let sid = s.doc().unwrap().doc.item(designcraft_doc::ItemId(r["id"].as_u64().unwrap())).unwrap().text_frame().unwrap().story;
        s.execute("text.select", &json!({"story": sid.0, "anchor": 0, "focus": 1})).unwrap();
        s.execute("style.paragraph.apply", &json!({"name": "Heading"})).unwrap();
        s.execute("variables.define", &json!({"name": "Chapter Head", "type": "runningHeader", "style": "Heading"})).unwrap();
        let r = s
            .execute("frame.create", &json!({"spread": {"kind": "parent", "index": 0}, "rect": [72, 30, 500, 60], "content": "text", "text": ""}))
            .unwrap();
        let sid = s.doc().unwrap().doc.item(designcraft_doc::ItemId(r["id"].as_u64().unwrap())).unwrap().text_frame().unwrap().story;
        s.execute("text.select", &json!({"story": sid.0, "anchor": 0, "focus": 0})).unwrap();
        s.execute("variables.insert", &json!({"name": "Chapter Head"})).unwrap();
        let d = s.doc().unwrap().doc.clone();
        let shown = |page: usize| -> usize {
            let cs = s.cache.get(&d, sid, Some(page));
            cs.frames.iter().flat_map(|f| f.lines.iter()).map(|l| l.glyphs.iter().filter(|g| g.visible && g.adv > 0.0).count()).sum()
        };
        assert_eq!(shown(0), 0, "no heading on or before the first page");
        assert_eq!(shown(2), "Epsilon".len(), "the third page (named 1) shows its own heading");
        assert_eq!(shown(3), "Epsilon".len(), "the fourth page (named 2) carries it");
    }

    /// Three pages with "Alpha" in the Heading style on page 1 and "Epsilon" on page 3, a
    /// `Chapter Head` running header on Heading, and a text frame on pages 1 and 3 that each hold
    /// "<head>|<head>" (two instances). Returns the two frames' stories.
    fn running_heads() -> (Session, designcraft_doc::StoryId, designcraft_doc::StoryId) {
        let mut s = Session::new();
        s.execute("file.new", &json!({"pages": 3, "facingPages": false})).unwrap();
        s.execute("style.paragraph.create", &json!({"name": "Heading"})).ok();
        let frame = |s: &mut Session, page: usize, y: f64, text: &str| {
            let sp = s.doc().unwrap().doc.page_loc(page).unwrap().0;
            let r = s.execute("frame.create", &json!({"spread": sp, "rect": [72, y, 400, y + 60.0], "content": "text", "text": text})).unwrap();
            s.doc().unwrap().doc.item(designcraft_doc::ItemId(r["id"].as_u64().unwrap())).unwrap().text_frame().unwrap().story
        };
        for (page, text) in [(0, "Alpha"), (2, "Epsilon")] {
            let sid = frame(&mut s, page, 200.0, text);
            s.execute("text.select", &json!({"story": sid.0, "anchor": 0, "focus": 1})).unwrap();
            s.execute("style.paragraph.apply", &json!({"name": "Heading"})).unwrap();
        }
        s.execute("variables.define", &json!({"name": "Chapter Head", "type": "runningHeader", "style": "Heading"})).unwrap();
        let c = var_char(s.doc().unwrap().doc.variable("Chapter Head").unwrap()).unwrap();
        let both = format!("{c}|{c}");
        let (p1, p3) = (frame(&mut s, 0, 400.0, &both), frame(&mut s, 2, 400.0, &both));
        (s, p1, p3)
    }

    /// Convert Variable to Text turns the selected instance into the value it shows on its own
    /// page (Epsilon on page 3, not page 1's Alpha); the other instance and the variable stay.
    #[test]
    fn convert_variable_to_text_uses_the_instances_page() {
        let (mut s, p1, p3) = running_heads();
        let c = var_char(s.doc().unwrap().doc.variable("Chapter Head").unwrap()).unwrap();
        let second = c.len_utf8() + 1;
        s.execute("text.select", &json!({"story": p3.0, "anchor": second, "focus": second + c.len_utf8()})).unwrap();
        s.execute("variables.convertToText", &json!({})).unwrap();
        let d = &s.doc().unwrap().doc;
        assert_eq!(d.story(p3).unwrap().text, format!("{c}|Epsilon"), "only the selected instance");
        assert_eq!(d.story(p1).unwrap().text, format!("{c}|{c}"));
        assert!(d.variable("Chapter Head").is_some(), "the variable stays defined");
        let t = s.doc().unwrap().selection.text.unwrap();
        assert_eq!((t.anchor, t.focus), (second, second + "Epsilon".len()), "the converted text stays selected");
        s.execute("edit.undo", &json!({})).unwrap();
        assert_eq!(s.doc().unwrap().doc.story(p3).unwrap().text, format!("{c}|{c}"), "one undo step");
    }

    /// The menu item is enabled only for a selection that holds an instance: not a caret, not
    /// text without one.
    #[test]
    fn convert_variable_to_text_needs_a_selected_instance() {
        let (mut s, p1, _) = running_heads();
        let c = var_char(s.doc().unwrap().doc.variable("Chapter Head").unwrap()).unwrap();
        let bar = c.len_utf8();
        let enabled = |s: &Session| (crate::find_command("variables.convertToText").unwrap().enabled)(s).is_ok();
        for (anchor, focus) in [(0, 0), (bar, bar + 1)] {
            s.execute("text.select", &json!({"story": p1.0, "anchor": anchor, "focus": focus})).unwrap();
            assert!(!enabled(&s), "{anchor}..{focus}");
        }
        s.execute("text.select", &json!({"story": p1.0, "anchor": 0, "focus": bar})).unwrap();
        assert!(enabled(&s));
    }

    /// A story threaded from a frame on page 1, which holds only "Lead", to a frame on page 3,
    /// where the caret is left at the end of "Body". A cell's or a note's own byte 0 read in this
    /// story is on page 1, its anchor on page 3.
    fn threaded(s: &mut Session) -> designcraft_doc::StoryId {
        let sp1 = s.doc().unwrap().doc.page_loc(0).unwrap().0;
        let sp3 = s.doc().unwrap().doc.page_loc(2).unwrap().0;
        let r = s.execute("frame.create", &json!({"spread": sp1, "rect": [72, 500, 400, 520], "content": "text", "text": "Lead\nBody"})).unwrap();
        let from = r["id"].as_u64().unwrap();
        s.execute("text.thread", &json!({"from": from, "rect": [72, 500, 400, 700], "spread": sp3})).unwrap();
        let d = s.doc().unwrap().doc.clone();
        let sid = d.item(designcraft_doc::ItemId(from)).unwrap().text_frame().unwrap().story;
        let cs = s.cache.get(&d, sid, None);
        let page = |pos| designcraft_compose::vars::page_of(&d, &cs, pos);
        assert_eq!((page(0), page("Lead\nB".len())), (Some(0), Some(2)), "Lead on page 1, Body on page 3");
        s.execute("text.select", &json!({"story": sid.0, "anchor": "Lead\nBody".len(), "focus": "Lead\nBody".len()})).unwrap();
        sid
    }

    /// An instance selected in a table cell becomes the value on the page of the table's anchor.
    #[test]
    fn convert_variable_to_text_in_a_table_cell() {
        let (mut s, ..) = running_heads();
        let c = var_char(s.doc().unwrap().doc.variable("Chapter Head").unwrap()).unwrap();
        let sid = threaded(&mut s);
        s.execute("table.insert", &json!({"rows": 1, "cols": 1})).unwrap();
        s.execute("table.setCell", &json!({"row": 0, "col": 0, "text": c.to_string()})).unwrap();
        let table = *s.doc().unwrap().doc.story(sid).unwrap().tables.keys().next().unwrap();
        let cell = designcraft_doc::CellAddr { table, row: 0, col: 0 };
        s.doc_mut().unwrap().selection =
            designcraft_doc::Selection::text(designcraft_doc::TextSel { story: sid, anchor: 0, focus: c.len_utf8(), frame: None, cell: Some(cell) });
        s.execute("variables.convertToText", &json!({})).unwrap();
        let d = &s.doc().unwrap().doc;
        assert_eq!(d.text_story(sid, Some(cell)).unwrap().text, "Epsilon");
    }

    /// An instance selected in a footnote becomes the value on the page of the note's reference.
    #[test]
    fn convert_variable_to_text_in_a_footnote() {
        let (mut s, ..) = running_heads();
        let c = var_char(s.doc().unwrap().doc.variable("Chapter Head").unwrap()).unwrap();
        let sid = threaded(&mut s);
        s.execute("footnote.insert", &json!({"text": c.to_string()})).unwrap();
        let id = s.doc().unwrap().doc.story(sid).unwrap().notes[0].id;
        let note = designcraft_doc::CellAddr::footnote(id);
        let at = s.doc().unwrap().doc.text_story(sid, Some(note)).unwrap().text.find(c).unwrap();
        s.doc_mut().unwrap().selection = designcraft_doc::Selection::text(designcraft_doc::TextSel {
            story: sid,
            anchor: at,
            focus: at + c.len_utf8(),
            frame: None,
            cell: Some(note),
        });
        s.execute("variables.convertToText", &json!({})).unwrap();
        let d = &s.doc().unwrap().doc;
        let text = &d.text_story(sid, Some(note)).unwrap().text;
        assert!(!text.contains(c) && text.ends_with("Epsilon"), "{text:?}");
    }

    /// Text Variables › Convert to Text turns every instance of the variable into the value on
    /// its page, in table cells, footnotes and endnotes on the page of their anchor, and keeps the
    /// variable.
    #[test]
    fn convert_all_instances_to_text() {
        let (mut s, p1, p3) = running_heads();
        let c = var_char(s.doc().unwrap().doc.variable("Chapter Head").unwrap()).unwrap();
        let sid = threaded(&mut s);
        let at_end = |s: &mut Session| {
            let n = s.doc().unwrap().doc.story(sid).unwrap().len();
            s.execute("text.select", &json!({"story": sid.0, "anchor": n, "focus": n})).unwrap();
        };
        s.execute("footnote.insert", &json!({"text": c.to_string()})).unwrap();
        at_end(&mut s);
        s.execute("endnote.insert", &json!({"text": c.to_string()})).unwrap();
        at_end(&mut s);
        s.execute("table.insert", &json!({"rows": 1, "cols": 1})).unwrap();
        s.execute("table.setCell", &json!({"row": 0, "col": 0, "text": c.to_string()})).unwrap();
        s.execute("variables.convertAllToText", &json!({"name": "Chapter Head"})).unwrap();
        let d = &s.doc().unwrap().doc;
        assert_eq!(d.story(p1).unwrap().text, "Alpha|Alpha");
        assert_eq!(d.story(p3).unwrap().text, "Epsilon|Epsilon");
        let st = d.story(sid).unwrap();
        assert_eq!(st.tables.values().next().unwrap().cells[0].text.text, "Epsilon", "table cell");
        assert!(st.notes[0].text.text.ends_with("Epsilon"), "footnote: {:?}", st.notes[0].text.text);
        assert!(st.endnotes[0].text.text.ends_with("Epsilon"), "endnote: {:?}", st.endnotes[0].text.text);
        assert!(d.variable("Chapter Head").is_some(), "the variable stays defined");
    }

    /// A caret in a table cell or a footnote after an instance stays after the value Convert to
    /// Text writes there, so typing lands after it.
    #[test]
    fn convert_all_keeps_the_caret_in_a_cell_or_a_footnote() {
        for footnote in [false, true] {
            let (mut s, ..) = running_heads();
            let c = var_char(s.doc().unwrap().doc.variable("Chapter Head").unwrap()).unwrap();
            let sid = threaded(&mut s);
            let text = format!("{c}|");
            let at = if footnote {
                s.execute("footnote.insert", &json!({"text": text})).unwrap();
                designcraft_doc::CellAddr::footnote(s.doc().unwrap().doc.story(sid).unwrap().notes[0].id)
            } else {
                s.execute("table.insert", &json!({"rows": 1, "cols": 1})).unwrap();
                s.execute("table.setCell", &json!({"row": 0, "col": 0, "text": text})).unwrap();
                let table = *s.doc().unwrap().doc.story(sid).unwrap().tables.keys().next().unwrap();
                designcraft_doc::CellAddr { table, row: 0, col: 0 }
            };
            let n = s.doc().unwrap().doc.text_story(sid, Some(at)).unwrap().text.len();
            s.doc_mut().unwrap().selection =
                designcraft_doc::Selection::text(designcraft_doc::TextSel { story: sid, anchor: n, focus: n, frame: None, cell: Some(at) });
            s.execute("variables.convertAllToText", &json!({"name": "Chapter Head"})).unwrap();
            s.execute("text.insert", &json!({"text": "x"})).unwrap();
            let text = &s.doc().unwrap().doc.text_story(sid, Some(at)).unwrap().text;
            assert!(text.ends_with("Epsilon|x"), "footnote {footnote}: {text:?}");
        }
    }

    /// Deleting a variable flattens its instances in table cells too and moves the instances of
    /// later variables down, so cells keep showing the same text.
    #[test]
    fn delete_variable_reaches_table_cells() {
        let mut s = Session::new();
        s.execute("file.new", &json!({})).unwrap();
        s.execute("variables.define", &json!({"name": "A", "type": "custom", "text": "Alpha"})).unwrap();
        let b = s.execute("variables.define", &json!({"name": "B", "type": "custom", "text": "Beta"})).unwrap()["index"].as_u64().unwrap() as usize;
        let a = s.doc().unwrap().doc.variable("A").unwrap();
        let r = s.execute("frame.create", &json!({"rect": [72, 72, 400, 300], "content": "text", "text": ""})).unwrap();
        let sid = s.doc().unwrap().doc.item(designcraft_doc::ItemId(r["id"].as_u64().unwrap())).unwrap().text_frame().unwrap().story;
        s.execute("text.select", &json!({"story": sid.0, "anchor": 0, "focus": 0})).unwrap();
        s.execute("table.insert", &json!({"rows": 1, "cols": 2})).unwrap();
        let text = format!("{}|{}", var_char(a).unwrap(), var_char(b).unwrap());
        s.execute("table.setCell", &json!({"row": 0, "col": 0, "text": text})).unwrap();
        s.execute("variables.delete", &json!({"name": "A"})).unwrap();
        let d = &s.doc().unwrap().doc;
        let t = d.story(sid).unwrap().tables.values().next().unwrap();
        let nb = d.variable("B").unwrap();
        assert_eq!(t.cells[0].text.text, format!("Alpha|{}", var_char(nb).unwrap()));
    }

    /// Deleting a variable flattens its instances in footnotes and endnotes too and moves the
    /// instances of later variables down.
    #[test]
    fn delete_variable_reaches_notes() {
        let mut s = Session::new();
        s.execute("file.new", &json!({})).unwrap();
        s.execute("variables.define", &json!({"name": "A", "type": "custom", "text": "Alpha"})).unwrap();
        let b = s.execute("variables.define", &json!({"name": "B", "type": "custom", "text": "Beta"})).unwrap()["index"].as_u64().unwrap() as usize;
        let a = s.doc().unwrap().doc.variable("A").unwrap();
        let r = s.execute("frame.create", &json!({"rect": [72, 72, 400, 300], "content": "text", "text": "x"})).unwrap();
        let sid = s.doc().unwrap().doc.item(designcraft_doc::ItemId(r["id"].as_u64().unwrap())).unwrap().text_frame().unwrap().story;
        let text = format!("{}|{}", var_char(a).unwrap(), var_char(b).unwrap());
        s.execute("text.select", &json!({"story": sid.0, "anchor": 1, "focus": 1})).unwrap();
        s.execute("footnote.insert", &json!({"text": text})).unwrap();
        s.execute("text.select", &json!({"story": sid.0, "anchor": 0, "focus": 0})).unwrap();
        s.execute("endnote.insert", &json!({"text": text})).unwrap();
        s.execute("variables.delete", &json!({"name": "A"})).unwrap();
        let d = &s.doc().unwrap().doc;
        let st = d.story(sid).unwrap();
        let want = format!("Alpha|{}", var_char(d.variable("B").unwrap()).unwrap());
        assert_eq!(st.notes[0].text.text, want);
        assert_eq!(st.endnotes[0].text.text, want);
    }

    #[test]
    fn running_header_on_parent_page_follows_headings() {
        let mut s = Session::new();
        s.execute("file.new", &json!({"pages": 3, "facingPages": false})).unwrap();
        s.execute("style.paragraph.create", &json!({"name": "Heading"})).ok();
        // Headings on pages 1 and 3 (page 2 carries page 1's).
        for (page, text) in [(0, "Alpha"), (2, "Epsilon")] {
            let sp = s.doc().unwrap().doc.page_loc(page).unwrap().0;
            let r = s.execute("frame.create", &json!({"spread": sp, "rect": [72, 200, 400, 260], "content": "text", "text": text})).unwrap();
            let sid = s.doc().unwrap().doc.item(designcraft_doc::ItemId(r["id"].as_u64().unwrap())).unwrap().text_frame().unwrap().story;
            s.execute("text.select", &json!({"story": sid.0, "anchor": 0, "focus": 1})).unwrap();
            s.execute("style.paragraph.apply", &json!({"name": "Heading"})).unwrap();
        }
        s.execute("variables.define", &json!({"name": "Chapter Head", "type": "runningHeader", "style": "Heading"})).unwrap();
        s.execute("variables.define", &json!({"name": "Total", "type": "custom", "text": "of "})).unwrap();
        // A header frame on the parent with "<running header> · <page> / <last page>".
        let r = s
            .execute("frame.create", &json!({"spread": {"kind": "parent", "index": 0}, "rect": [72, 30, 500, 60], "content": "text", "text": ""}))
            .unwrap();
        let fid = designcraft_doc::ItemId(r["id"].as_u64().unwrap());
        let sid = s.doc().unwrap().doc.item(fid).unwrap().text_frame().unwrap().story;
        s.execute("text.select", &json!({"story": sid.0, "anchor": 0, "focus": 0})).unwrap();
        s.execute("variables.insert", &json!({"name": "Chapter Head"})).unwrap();
        s.execute("text.insert", &json!({"text": " · "})).unwrap();
        s.execute("variables.insert", &json!({"name": "Last Page Number"})).unwrap();
        let d = s.doc().unwrap().doc.clone();
        assert!(matches!(d.find(fid).unwrap().spread, designcraft_doc::SpreadRef::Parent(0)));
        // Visible glyphs of the header composed for each page: "Alpha · 3" vs "Epsilon · 3".
        let shown = |page: usize| -> usize {
            let cs = s.cache.get(&d, sid, Some(page));
            cs.frames.iter().flat_map(|f| f.lines.iter()).map(|l| l.glyphs.iter().filter(|g| g.visible && g.adv > 0.0).count()).sum()
        };
        let (p1, p2, p3) = (shown(0), shown(1), shown(2));
        assert_eq!(p1, p2, "page 2 carries page 1's header");
        assert_eq!(p3, p1 + 2, "Epsilon is two letters longer than Alpha");
        let idx = s.cache.running_index(&d);
        assert_eq!(idx.value("Heading", false, Default::default(), 0), "Alpha");
        assert_eq!(idx.value("Heading", false, Default::default(), 1), "Alpha");
        assert_eq!(idx.value("Heading", false, Default::default(), 2), "Epsilon");
        let list = s.execute("variables.list", &json!({})).unwrap();
        assert!(list.as_array().unwrap().iter().any(|v| v["name"] == "Last Page Number" && v["value"] == "3"));
        // Deleting a variable flattens its instances to text.
        s.execute("variables.delete", &json!({"name": "Last Page Number"})).unwrap();
        assert!(s.doc().unwrap().doc.story(sid).unwrap().text.ends_with(" · 3"));
    }
}
