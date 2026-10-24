//! Edit → Find/Change: plain text and GREP (regular expressions) with InDesign's metacharacters,
//! optional formatting in the change, scoped to the document, a story or the selection.

use std::ops::Range;
use std::sync::{Arc, Weak};

use designcraft_compose::grep::{Grep, GrepOptions, translate_replacement};
use designcraft_doc::story::floor_char_boundary;
use designcraft_doc::{CellAddr, Document, Selection, StoryId, TextSel};
use serde_json::{Value, json};

use super::{CommandSpec, bad, bool_or, cmd, has_doc, str_param};
use crate::{DocState, Result, Session};

pub fn specs() -> Vec<CommandSpec> {
    vec![
        cmd!(noundo "find.objects", "Find Object", [], None,
        "{fill?, stroke?: swatch, strokeWeight?, opacity? (0–1), kind?: text|graphic|shape|line|group, layer?, label?, select?: bool (default true)} → {ids} — Find/Change › Object, through the document",
        has_doc, |s, p| {
            s.reset_find_scope();
            let ids = find_objects(&s.doc()?.doc, p);
            if p.get("select").and_then(Value::as_bool).unwrap_or(true) && !ids.is_empty() {
                let st = s.doc_mut()?;
                st.selection = designcraft_doc::Selection::items(ids.clone());
                st.bump_revision();
            }
            Ok(json!({"ids": ids.iter().map(|i| i.0).collect::<Vec<_>>()}))
        }),
        cmd!(
            "find.changeObjects",
            "Change All (Object)",
            [],
            None,
            "{…the criteria of find.objects, change: {fill?, stroke?, strokeWeight?, opacity?}} → {changed}",
            has_doc,
            |s, p| {
                s.reset_find_scope();
                let ids = find_objects(&s.doc()?.doc, p);
                let ch = p.get("change").cloned().unwrap_or(Value::Null);
                if !ch.is_object() {
                    return Err(bad("find.changeObjects", "`change` required"));
                }
                s.edit(|d, _| {
                    for id in &ids {
                        let Some(it) = d.item_mut(*id) else { continue };
                        if let Some(f) = ch.get("fill").and_then(Value::as_str) {
                            it.fill = designcraft_doc::Fill::swatch(f);
                        }
                        if let Some(c) = ch.get("stroke").and_then(Value::as_str) {
                            it.stroke.swatch = c.to_string();
                        }
                        if let Some(w) = ch.get("strokeWeight").and_then(Value::as_f64) {
                            it.stroke.weight = w.max(0.0);
                        }
                        if let Some(o) = ch.get("opacity").and_then(Value::as_f64) {
                            it.opacity = o.clamp(0.0, 1.0) as f32;
                        }
                    }
                    Ok(json!({"changed": ids.len()}))
                })
            }
        ),
        cmd!(query "find.find", "Find", ["Edit", "Find/Change"], None,
            "{find, grep?: bool, caseSensitive?: bool, wholeWord?: bool, scope?: document|story|selection, story?, change?|first?|attrs?: ignored (find.change takes them; one request serves all three Find/Change commands)} → matches [{story, start, end, text, cell?: {table, row, col}}] (document scope also searches table cells; start/end then index the cell's text)",
            has_doc, find),
        cmd!(
            "find.change",
            "Change All",
            ["Edit", "Find/Change"],
            None,
            "{find, change, grep?, caseSensitive?, wholeWord?, scope?, story?, first?: bool (change only the first match), attrs?: {character attributes applied to changed text}} → {count}",
            has_doc,
            change
        ),
        cmd!(noundo "find.next", "Find Next", ["Edit"], Some("Cmd+Alt+F"), "{find, grep?, caseSensitive?, wholeWord?, scope?: document|story|selection, story?, change?|first?|attrs?: ignored (find.change takes them; one request serves all three Find/Change commands)} — Selection starts at its original beginning; later calls advance within the scope", has_doc, find_next),
    ]
}

/// Translate text-search metacharacters (`^p`, `^t`…) to our story encoding.
fn metachars(s: &str) -> String {
    let pairs: &[(&str, &str)] = &[
        ("^p", "\n"),
        ("^t", "\t"),
        ("^n", "\u{2028}"),
        ("^#", "\u{E000}"),
        ("^_", "\u{2014}"),
        ("^=", "\u{2013}"),
        ("^-", "\u{AD}"),
        ("^s", "\u{A0}"),
        ("^^", "^"),
    ];
    let mut out = s.to_string();
    for (a, b) in pairs {
        out = out.replace(a, b);
    }
    out
}

/// The search pattern: GREP (in the documents' dialect, see `designcraft_compose::grep`) or
/// literal text with metacharacters.
fn build(p: &Value) -> Result<Grep> {
    let f = str_param(p, "find").ok_or_else(|| bad("find", "missing `find`"))?;
    if f.is_empty() {
        return Err(bad("find", "empty search"));
    }
    let opts = GrepOptions { case_insensitive: !bool_or(p, "caseSensitive", false), multi_line: true, whole_word: bool_or(p, "wholeWord", false) };
    let re = if bool_or(p, "grep", false) { Grep::new(f, opts) } else { Grep::literal(&metachars(f), opts) };
    re.map_err(|e| bad("find", e))
}

#[derive(Clone, Debug, PartialEq, Eq)]
enum Domain {
    Document,
    Story,
    Selection,
    Explicit(StoryId),
}

impl Domain {
    fn from_params(p: &Value) -> Self {
        if let Some(id) = p.get("story").and_then(Value::as_u64) {
            return Self::Explicit(StoryId(id));
        }
        match str_param(p, "scope").unwrap_or("document") {
            "story" => Self::Story,
            "selection" => Self::Selection,
            _ => Self::Document,
        }
    }
}

/// Whether two requests use the same search domain, including explicit-story precedence.
pub fn same_find_domain(a: &Value, b: &Value) -> bool {
    Domain::from_params(a) == Domain::from_params(b)
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct Matcher {
    find: String,
    grep: bool,
    case_sensitive: bool,
    whole_word: bool,
}

impl Matcher {
    fn from_params(p: &Value) -> Self {
        Self {
            find: str_param(p, "find").unwrap_or("").to_string(),
            grep: bool_or(p, "grep", false),
            case_sensitive: bool_or(p, "caseSensitive", false),
            whole_word: bool_or(p, "wholeWord", false),
        }
    }
}

/// A searched text: a story, or one table cell of it, and the byte range inside.
type Target = (StoryId, Option<CellAddr>, Range<usize>);

/// A search owns its original domain, not the selection used to paint the latest hit.
/// Weak identity avoids retaining an old document merely for this transient cache.
#[derive(Clone)]
pub(crate) struct FindContext {
    document: Weak<Document>,
    uid: u64,
    revision: u64,
    selection: Selection,
    domain: Domain,
    matcher: Matcher,
    targets: Vec<Target>,
    order: Vec<usize>,
    target: usize,
    offset: usize,
    last_match_end: Option<usize>,
}

impl FindContext {
    pub(crate) fn is_current(&self, st: &DocState) -> bool {
        self.uid == st.uid && Weak::ptr_eq(&self.document, &Arc::downgrade(&st.doc)) && self.revision == st.revision && self.selection == st.selection
    }

    fn acknowledge(&mut self, st: &DocState) {
        self.revision = st.revision;
        self.selection = st.selection.clone();
    }

    fn restart(&mut self, st: &DocState) -> Result<()> {
        self.target = 0;
        self.last_match_end = None;
        self.order = (0..self.targets.len()).collect();
        let caret = st.selection.text.filter(|_| st.selection.cells.is_none());
        // A caret in a searched cell starts there; one in a footnote starts at its story.
        let start = st.selection.text.and_then(|t| {
            let at = |want: Option<CellAddr>| self.targets.iter().position(|(sid, cell, _)| *sid == t.story && *cell == want);
            at(t.cell).or_else(|| at(None))
        });
        if let Some(i) = start.filter(|_| self.domain == Domain::Document) {
            self.order.rotate_left(i);
        }
        let first = self.order.first().and_then(|i| self.targets.get(*i));
        self.offset = match first {
            Some((sid, cell, r)) if self.domain != Domain::Selection => {
                caret.filter(|t| t.story == *sid && t.cell == *cell).map_or(r.start, |t| t.range().end.min(r.end))
            }
            Some((_, _, r)) => r.start,
            None => 0,
        };
        if let Some((sid, cell, r)) = first {
            let text = target_text(&st.doc, *sid, *cell, r)?;
            if text.get(self.offset.saturating_sub(r.start)..).is_none() {
                return Err(bad("find", "invalid UTF-8 search cursor"));
            }
        }
        Ok(())
    }

    fn advance_target(&mut self) {
        self.target = self.target.saturating_add(1);
        self.last_match_end = None;
        self.offset = self.order.get(self.target).and_then(|i| self.targets.get(*i)).map_or(0, |(_, _, r)| r.start);
    }

    fn advance_empty(&mut self, text: &str, relative_end: usize, end: usize) {
        // Move one Unicode scalar, not one byte; an endpoint is returned at most once.
        if let Some(ch) = text.get(relative_end..).and_then(|rest| rest.chars().next()) {
            self.offset = end + ch.len_utf8();
        } else {
            self.advance_target();
        }
    }
}

fn target_text<'a>(d: &'a Document, sid: StoryId, cell: Option<CellAddr>, r: &Range<usize>) -> Result<&'a str> {
    d.text_story(sid, cell)
        .ok_or_else(|| bad("find", "no such story"))?
        .text
        .get(r.clone())
        .ok_or_else(|| bad("find", "invalid selection bounds or UTF-8 boundary"))
}

fn scope(s: &Session, p: &Value) -> Result<FindContext> {
    let st = s.doc()?;
    let domain = Domain::from_params(p);
    let matcher = Matcher::from_params(p);
    if let Some(previous) = s.find_context.as_ref().filter(|c| c.is_current(st) && c.domain == domain) {
        let mut context = previous.clone();
        if context.matcher != matcher {
            context.matcher = matcher;
            context.restart(st)?;
        }
        return Ok(context);
    }
    let d = &st.doc;
    let targets = match domain {
        Domain::Explicit(sid) => {
            let len = d.story(sid).ok_or_else(|| bad("find", "no such story"))?.len();
            vec![(sid, None, 0..len)]
        }
        Domain::Story | Domain::Selection => {
            if st.selection.cells.is_some() || st.selection.text.is_some_and(|t| t.cell.is_some()) {
                return Err(bad("find", "Story/Selection search in table cells or notes is not supported"));
            }
            let t = st.selection.text.ok_or_else(|| bad("find", "no text selected"))?;
            let story = d.story(t.story).ok_or_else(|| bad("find", "no such story"))?;
            let r = if domain == Domain::Selection { t.range() } else { 0..story.len() };
            target_text(d, t.story, None, &r)?;
            vec![(t.story, None, r)]
        }
        // Table cells hold their own text outside `stories`; search them right after the story that anchors the table.
        Domain::Document => d
            .stories
            .values()
            .flat_map(|x| {
                let cells = x.tables.iter().flat_map(move |(id, t)| {
                    let cols = t.ncols().max(1);
                    t.cells.iter().enumerate().map(move |(i, c)| (x.id, Some(CellAddr { table: *id, row: i / cols, col: i % cols }), 0..c.text.len()))
                });
                std::iter::once((x.id, None, 0..x.len())).chain(cells)
            })
            .collect(),
    };
    let mut context = FindContext {
        document: Arc::downgrade(d),
        uid: st.uid,
        revision: st.revision,
        selection: st.selection.clone(),
        domain,
        matcher,
        targets,
        order: Vec::new(),
        target: 0,
        offset: 0,
        last_match_end: None,
    };
    context.restart(st)?;
    Ok(context)
}

/// Most matches `find.find` returns.
const MAX_MATCHES: usize = 10_000;

fn find(s: &mut Session, p: &Value) -> Result<Value> {
    let re = build(p)?;
    let context = scope(s, p)?;
    let st = s.doc()?;
    let mut out = Vec::new();
    for (sid, cell, r) in &context.targets {
        let text = target_text(&st.doc, *sid, *cell, r)?;
        for m in re.find_all(text, MAX_MATCHES.saturating_sub(out.len())).map_err(|e| bad("find", e))? {
            let mut hit = json!({"story": sid.0, "start": r.start + m.start, "end": r.start + m.end, "text": text.get(m)});
            if let Some(c) = cell {
                hit["cell"] = json!({"table": c.table, "row": c.row, "col": c.col});
            }
            out.push(hit);
        }
    }
    s.find_context = Some(context);
    Ok(Value::Array(out))
}

/// The change string: an expansion template (`$1`…) for GREP, else literal text.
fn replacement(p: &Value) -> String {
    let c = str_param(p, "change").unwrap_or("");
    if bool_or(p, "grep", false) { translate_replacement(c) } else { metachars(c) }
}

/// Repair byte validity of a text selection once against the final story, without remapping valid offsets.
fn repair_selection(t: &mut designcraft_doc::TextSel, st: &designcraft_doc::Story) {
    let range = t.range();
    let start = floor_char_boundary(&st.text, range.start);
    let mut end = range.end.min(st.len());
    if t.is_caret() {
        end = start;
    } else if !st.text.is_char_boundary(end) {
        let floor = floor_char_boundary(&st.text, end);
        end = st.text.get(floor..).and_then(|rest| rest.chars().next()).map_or(floor, |ch| floor + ch.len_utf8());
    }
    (t.anchor, t.focus) = if t.anchor <= t.focus { (start, end) } else { (end, start) };
}

/// The character overrides a Change applies to every replacement, from its `attrs` object.
fn char_overrides(attrs: Option<&Value>) -> Result<designcraft_doc::CharAttrs> {
    let mut over = designcraft_doc::CharAttrs::default();
    if let Some(o) = attrs.and_then(Value::as_object) {
        for (k, v) in o {
            over.set_json(k, v).map_err(|e| bad("find.change", e))?;
        }
    }
    Ok(over)
}

fn change(s: &mut Session, p: &Value) -> Result<Value> {
    let re = build(p)?;
    let grep = bool_or(p, "grep", false);
    let rep = replacement(p);
    let first_only = bool_or(p, "first", false);
    let attrs = p.get("attrs").cloned();
    let over = char_overrides(attrs.as_ref())?;
    super::text::validate_char_fill(&s.doc()?.doc, &over, "find.change")?;
    let targets = scope(s, p)?.targets;
    s.edit(|d, sel| {
        let mut count = 0usize;
        for (sid, cell, r) in targets {
            let Some(story) = d.text_story(sid, cell) else { continue };
            // Collect matches first (positions refer to the original text), apply back to front.
            let text = story.text.get(r.clone()).ok_or_else(|| bad("find", "invalid selection bounds or UTF-8 boundary"))?;
            let edits: Vec<(std::ops::Range<usize>, String)> = if grep {
                re.replacements(text, &rep, first_only)
            } else {
                re.find_all(text, if first_only { 1 } else { usize::MAX }).map(|ms| ms.into_iter().map(|m| (m, rep.clone())).collect())
            }
            .map_err(|e| bad("find.change", e))?
            .into_iter()
            .map(|(m, out)| (r.start + m.start..r.start + m.end, out))
            .collect();
            let Some(st) = d.text_story_mut(sid, cell) else { continue };
            for (range, new) in edits.iter().rev() {
                st.replace(range.clone(), new);
                if !over.is_empty() && !new.is_empty() {
                    st.format_chars(range.start..range.start + new.len(), |f| f.over.merge(&over));
                }
                count += 1;
            }
            if let Some(t) = sel.text.as_mut().filter(|t| t.story == sid && t.cell == cell) {
                repair_selection(t, st);
            }
            // `first` is the first match of the whole search, not of each story.
            if first_only && count > 0 {
                break;
            }
        }
        Ok(json!({"count": count}))
    })
}

fn find_next(s: &mut Session, p: &Value) -> Result<Value> {
    let re = build(p)?;
    let mut context = scope(s, p)?;
    let st = s.doc()?;
    let mut hit = None;
    while let Some((sid, cell, r)) = context.order.get(context.target).and_then(|i| context.targets.get(*i)).cloned() {
        let text = target_text(&st.doc, sid, cell, &r)?;
        let from = context.offset.checked_sub(r.start).ok_or_else(|| bad("find", "invalid search cursor"))?;
        if text.get(from..).is_none() {
            return Err(bad("find", "invalid UTF-8 search cursor"));
        }
        if let Some(m) = re.find_at(text, from).map_err(|e| bad("find.next", e))? {
            let (a, b) = (r.start + m.start, r.start + m.end);
            // Match find_iter/captures_iter: suppress an empty match adjacent to the
            // previous match's end, but keep adjacent nonempty matches.
            if m.is_empty() && context.last_match_end == Some(b) {
                context.advance_empty(text, m.end, b);
                continue;
            }
            hit = Some((sid, cell, a, b));
            context.last_match_end = Some(b);
            if m.is_empty() {
                context.advance_empty(text, m.end, b);
            } else {
                context.offset = b;
            }
            break;
        }
        context.advance_target();
    }
    let Some((sid, cell, a, b)) = hit else {
        s.find_context = Some(context);
        return Ok(Value::Null);
    };
    let frame = st.doc.story(sid).and_then(|x| x.frames.first().copied());
    let cell_json = cell.map(|c| json!({"table": c.table, "row": c.row, "col": c.col}));
    let stm = s.doc_mut()?;
    stm.selection = Selection::text(TextSel { story: sid, anchor: a, focus: b, frame, cell });
    stm.bump_revision();
    context.acknowledge(stm);
    s.find_context = Some(context);
    let mut out = json!({"story": sid.0, "start": a, "end": b});
    if let Some(c) = cell_json {
        out["cell"] = c;
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use crate::Session;

    #[test]
    fn find_and_change_text_and_grep() {
        let mut s = Session::new();
        s.execute("file.new", &json!({})).unwrap();
        s.execute("frame.create", &json!({"rect": [36, 36, 300, 300], "content": "text", "text": "Call 555-1234 or 555-9876.\nColour and colour."}))
            .unwrap();
        let m = s.execute("find.find", &json!({"find": "colour"})).unwrap();
        assert_eq!(m.as_array().unwrap().len(), 2);
        let m = s.execute("find.find", &json!({"find": "colour", "caseSensitive": true})).unwrap();
        assert_eq!(m.as_array().unwrap().len(), 1);
        let r = s.execute("find.change", &json!({"find": r"(\d{3})-(\d{4})", "change": "($1) $2", "grep": true})).unwrap();
        assert_eq!(r["count"], 2);
        let r = s.execute("find.change", &json!({"find": "colour", "change": "color", "attrs": {"fontStyle": "Italic"}})).unwrap();
        assert_eq!(r["count"], 2);
        let d = s.execute("document.inspect", &json!({})).unwrap();
        let sid = d["stories"][0]["id"].as_u64().unwrap();
        let text = s.execute("story.get", &json!({"story": sid})).unwrap()["text"].as_str().unwrap().to_string();
        assert_eq!(text, "Call (555) 1234 or (555) 9876.\ncolor and color.");
        // Paragraph metacharacter in text mode.
        assert_eq!(s.execute("find.find", &json!({"find": ".^pc"})).unwrap().as_array().unwrap().len(), 1);
        s.execute("find.next", &json!({"find": "or"})).unwrap();
        assert!(s.doc().unwrap().selection.text.is_some());
    }

    #[test]
    fn change_first_changes_one_match_in_the_whole_document() {
        for grep in [false, true] {
            let mut s = Session::new();
            s.execute("file.new", &json!({})).unwrap();
            for y in [36, 336] {
                s.execute("frame.create", &json!({"rect": [36, y, 300, y + 200], "content": "text", "text": "red and red"})).unwrap();
            }
            let r = s.execute("find.change", &json!({"find": "red", "change": "blue", "first": true, "grep": grep})).unwrap();
            assert_eq!(r["count"], 1);
            assert_eq!(s.execute("find.find", &json!({"find": "blue"})).unwrap().as_array().unwrap().len(), 1);
            assert_eq!(s.execute("find.find", &json!({"find": "red"})).unwrap().as_array().unwrap().len(), 3);
        }
    }

    fn scope_session(text: &str) -> (Session, designcraft_doc::StoryId) {
        let mut s = Session::new();
        s.execute("file.new", &json!({})).unwrap();
        let frame = s.execute("frame.create", &json!({"rect": [36, 36, 100, 100], "content": "text", "text": text})).unwrap();
        (s, designcraft_doc::StoryId(frame["story"].as_u64().unwrap()))
    }

    fn scope_select(s: &mut Session, sid: designcraft_doc::StoryId, a: usize, b: usize) {
        s.execute("text.select", &json!({"story": sid.0, "anchor": a, "focus": b})).unwrap();
    }

    fn scope_hit(s: &mut Session, p: &serde_json::Value, sid: designcraft_doc::StoryId, a: usize, b: usize) {
        assert_eq!(s.execute("find.next", p).unwrap(), json!({"story": sid.0, "start": a, "end": b}));
        let selected = s.doc().unwrap().selection.text.unwrap();
        assert_eq!((selected.story, selected.range(), selected.cell), (sid, a..b, None));
    }

    #[test]
    fn find_scope_first_change_stops_after_first_matching_story_and_repairs_selection() {
        for (texts, first, explicit, grep, expected, count) in [
            (vec!["red red", "red"], Some(true), false, false, vec!["blue red", "red"], 1),
            (vec!["keep", "red red", "red"], Some(true), false, false, vec!["keep", "blue red", "red"], 1),
            (vec!["keep", "other"], Some(true), false, false, vec!["keep", "other"], 0),
            (vec!["red red", "red"], Some(false), false, false, vec!["blue blue", "blue"], 3),
            (vec!["red red", "red"], None, false, false, vec!["blue blue", "blue"], 3),
            (vec!["red red", "red"], Some(true), true, false, vec!["blue red", "red"], 1),
            (vec!["red red", "red"], Some(false), true, false, vec!["blue blue", "red"], 2),
            (vec!["", "é🙂"], Some(true), false, true, vec!["X", "é🙂"], 1),
            (vec!["é", "🙂"], Some(false), false, true, vec!["XéX", "X🙂X"], 4),
        ] {
            let (mut s, sid) = scope_session(texts[0]);
            let mut sids = vec![sid];
            for text in texts.iter().skip(1) {
                let frame = s.execute("frame.create", &json!({"rect": [120, 36, 200, 100], "content": "text", "text": text})).unwrap();
                sids.push(designcraft_doc::StoryId(frame["story"].as_u64().unwrap()));
            }
            assert_eq!(s.doc().unwrap().doc.stories.keys().copied().collect::<Vec<_>>(), sids);
            // Document Change keeps canonical target order even with a later story current.
            scope_select(&mut s, *sids.last().unwrap(), 0, 0);
            let before = s.doc().unwrap().doc.clone();
            let mut p = json!({
                "find": if grep { "(?:)" } else { "red" },
                "change": if grep { "X" } else { "blue" },
                "scope": "document", "grep": grep
            });
            if let Some(first) = first {
                p["first"] = json!(first);
            }
            if explicit {
                p["story"] = json!(sid.0);
                p["scope"] = json!("selection");
            }
            if first == Some(true) && !explicit && !grep && texts.last() == Some(&"red") {
                let last = *sids.last().unwrap();
                scope_hit(&mut s, &p, last, 0, 3);
                let first_hit = sids[if texts.len() == 2 { 0 } else { 1 }];
                assert_eq!(
                    s.execute("find.find", &p).unwrap(),
                    json!([
                        {"story": first_hit.0, "start": 0, "end": 3, "text": "red"},
                        {"story": first_hit.0, "start": 4, "end": 7, "text": "red"},
                        {"story": last.0, "start": 0, "end": 3, "text": "red"}
                    ]),
                    "Count remains canonical after Next paints the later story"
                );
            }
            let selection = s.doc().unwrap().selection.clone();
            assert_eq!(s.execute("find.change", &p).unwrap(), json!({"count": count}));
            let st = s.doc().unwrap();
            assert_eq!(st.selection, selection);
            for (i, sid) in sids.iter().enumerate() {
                assert_eq!(st.doc.story(*sid).unwrap().text, expected[i]);
                if texts[i] == expected[i] {
                    assert_eq!(st.doc.story(*sid), before.story(*sid), "untouched story {i} stays entirely unchanged");
                }
            }
            st.doc.check().unwrap();
        }

        // Identical replacement is still one counted match, not a reason to visit another story.
        let (mut s, sid) = scope_session("red red");
        let other = s.execute("frame.create", &json!({"rect": [120, 36, 200, 100], "content": "text", "text": "red"})).unwrap();
        let other_sid = designcraft_doc::StoryId(other["story"].as_u64().unwrap());
        scope_select(&mut s, other_sid, 0, 3);
        let before = s.doc().unwrap().doc.clone();
        let selection = s.doc().unwrap().selection.clone();
        assert_eq!(
            s.execute("find.change", &json!({"find": "red", "change": "red", "scope": "document", "first": true})).unwrap(),
            json!({"count": 1})
        );
        assert_eq!(s.doc().unwrap().doc.story(sid).unwrap().text, "red red");
        assert_eq!(s.doc().unwrap().doc.story(other_sid), before.story(other_sid));
        assert_eq!(s.doc().unwrap().selection, selection);
        s.doc().unwrap().doc.check().unwrap();

        for reverse in [false, true] {
            let (mut s, sid) = scope_session("éX X");
            let other = s.execute("frame.create", &json!({"rect": [120, 36, 200, 100], "content": "text", "text": "X"})).unwrap();
            let other_sid = designcraft_doc::StoryId(other["story"].as_u64().unwrap());
            scope_select(&mut s, sid, if reverse { 3 } else { 2 }, if reverse { 2 } else { 3 });
            let before = s.doc().unwrap().doc.clone();
            let mut selection = s.doc().unwrap().selection.clone();
            let t = selection.text.as_mut().unwrap();
            (t.anchor, t.focus) = if reverse { (6, 2) } else { (2, 6) };
            let p = json!({"find": "X", "change": "🙂", "scope": "document", "first": true});
            assert_eq!(s.execute("find.find", &p).unwrap().as_array().unwrap().len(), 3);
            assert_eq!(s.execute("find.change", &p).unwrap(), json!({"count": 1}));
            let st = s.doc().unwrap();
            assert_eq!(st.doc.story(sid).unwrap().text, "é🙂 X");
            assert_eq!(st.doc.story(other_sid), before.story(other_sid));
            assert_eq!(st.selection, selection, "first-match exit must follow final UTF-8 repair");
            st.doc.check().unwrap();
            assert_eq!(
                s.execute("find.find", &json!({"find": "🙂", "scope": "selection"})).unwrap(),
                json!([{"story": sid.0, "start": 2, "end": 6, "text": "🙂"}])
            );
        }
    }

    #[test]
    fn grep_in_the_documents_dialect() {
        let mut s = Session::new();
        s.execute("file.new", &json!({})).unwrap();
        s.execute("frame.create", &json!({"rect": [36, 36, 300, 300], "content": "text", "text": "Москва and Paris, 1\u{2013}2 days.\nok"})).unwrap();
        // `\u` uppercase, `\l` lowercase letter.
        let m = s.execute("find.find", &json!({"find": r"\u\l+", "grep": true, "caseSensitive": true})).unwrap();
        let words: Vec<&str> = m.as_array().unwrap().iter().map(|x| x["text"].as_str().unwrap()).collect();
        assert_eq!(words, ["Москва", "Paris"]);
        // `~=` en dash, `\h` space, `\r` paragraph end; the change uses groups, `~S` and `\r`.
        let r = s.execute("find.change", &json!({"find": r"(\d)~=(\d)\h(\l+)\.\r", "change": r"$1~=$2~S$3!\r", "grep": true})).unwrap();
        assert_eq!(r["count"], 1);
        let sid = s.execute("document.inspect", &json!({})).unwrap()["stories"][0]["id"].as_u64().unwrap();
        let text = s.execute("story.get", &json!({"story": sid})).unwrap()["text"].as_str().unwrap().to_string();
        assert!(text.starts_with("Москва and Paris, 1\u{2013}2\u{A0}days!\nok"), "{text:?}");
        // A pattern that doesn't compile is an error, not a silent no-op.
        assert!(s.execute("find.find", &json!({"find": "[[:nope:]]", "grep": true})).is_err());
        assert!(s.execute("find.change", &json!({"find": "(", "change": "", "grep": true})).is_err());
    }

    #[test]
    fn find_scope_exact_baseline_count_next_change() {
        let mut s = Session::new();
        s.execute("file.new", &json!({})).unwrap();
        let frame = s
            .execute(
                "frame.create",
                &json!({
                    "rect": [36, 36, 300, 200], "content": "text", "text": "red red"
                }),
            )
            .unwrap();
        let sid = frame["story"].as_u64().unwrap();
        s.execute("text.select", &json!({"story": sid, "anchor": 0, "focus": 3})).unwrap();
        assert_eq!(
            s.execute("find.find", &json!({"find": "red", "scope": "selection"})).unwrap(),
            json!([{"story": sid, "start": 0, "end": 3, "text": "red"}])
        );
        assert_eq!(s.execute("find.next", &json!({"find": "red", "scope": "selection"})).unwrap(), json!({"story": sid, "start": 0, "end": 3}));
        assert_eq!(s.execute("find.change", &json!({"find": "red", "change": "blue", "scope": "selection"})).unwrap(), json!({"count": 1}));
        assert_eq!(s.execute("story.get", &json!({"story": sid})).unwrap()["text"], "blue red");
    }

    #[test]
    fn find_scope_change_growth_preserves_direction_and_full_scalar_hits() {
        for (text, find, change, changed, start, old_end, new_end) in [("a", "a", "é", "é", 0, 1, 2), ("éX", "X", "🙂", "é🙂", 2, 3, 6)] {
            for reverse in [false, true] {
                let (mut s, sid) = scope_session(text);
                // Real caret movement keeps the frame identity, unlike text.select.
                s.execute("text.move", &json!({"dir": "left", "extend": reverse})).unwrap();
                if !reverse {
                    s.execute("text.move", &json!({"dir": "right", "extend": true})).unwrap();
                }
                let selected = s.doc().unwrap().selection.text.unwrap();
                assert_eq!((selected.anchor, selected.focus), if reverse { (old_end, start) } else { (start, old_end) });
                assert!(selected.frame.is_some());
                let mut expected = s.doc().unwrap().selection.clone();
                let t = expected.text.as_mut().unwrap();
                (t.anchor, t.focus) = if reverse { (new_end, start) } else { (start, new_end) };
                let p = json!({"find": find, "change": change, "scope": "selection"});
                assert_eq!(s.execute("find.find", &p).unwrap(), json!([{"story": sid.0, "start": start, "end": old_end, "text": find}]));
                assert_eq!(s.execute("find.change", &p).unwrap(), json!({"count": 1}));
                let st = s.doc().unwrap();
                assert_eq!(st.doc.story(sid).unwrap().text, changed);
                assert_eq!(st.selection, expected);
                let t = st.selection.text.unwrap();
                assert!(changed.is_char_boundary(t.anchor) && changed.is_char_boundary(t.focus));
                assert!(s.find_context.is_none());
                let p = json!({"find": change, "scope": "selection"});
                assert_eq!(s.execute("find.find", &p).unwrap(), json!([{"story": sid.0, "start": start, "end": new_end, "text": change}]));
                scope_hit(&mut s, &p, sid, start, new_end);
                assert_eq!(s.execute("find.next", &p).unwrap(), serde_json::Value::Null);
            }
        }
    }

    #[test]
    fn find_scope_change_repairs_final_boundaries_without_remapping_valid_offsets() {
        for (text, (a, b), find, change, changed, (after_a, after_b), count) in [
            ("aX", (1, 1), "a", "é", "éX", (0, 0), 1),     // A caret stays collapsed and floors.
            ("aX", (2, 2), "a", "é", "éX", (2, 2), 1),     // A valid caret stays at its byte offset.
            ("abc", (1, 2), "abc", "🙂", "🙂", (0, 4), 1), // Both endpoints need repair.
            ("abc", (2, 1), "abc", "🙂", "🙂", (4, 0), 1),
            ("é🙂", (2, 6), "🙂", "X", "éX", (2, 3), 1), // Existing shortening behavior.
            ("é🙂", (6, 2), "🙂", "X", "éX", (3, 2), 1),
            ("red", (0, 3), "red", "blue", "blue", (0, 3), 1), // Do not extend a valid upper bound.
            ("aX", (1, 2), "a", "zz", "zzX", (1, 2), 1),       // Do not track the original X.
            ("aaX", (1, 2), "a", "é", "ééX", (0, 2), 2),       // Per-edit repair would wrongly yield 0..4.
            ("aaX", (2, 3), "a", "é", "ééX", (2, 4), 2),
            ("abc", (1, 3), "abc", "", "", (0, 0), 1),
            ("abc", (2, 3), "abc", "é", "é", (2, 2), 1), // EOF clamp may collapse a nonempty range.
        ] {
            let (mut s, sid) = scope_session(text);
            scope_select(&mut s, sid, a, b);
            let mut expected = s.doc().unwrap().selection.clone();
            let t = expected.text.as_mut().unwrap();
            (t.anchor, t.focus) = (after_a, after_b);
            assert_eq!(s.execute("find.change", &json!({"find": find, "change": change, "scope": "document"})).unwrap(), json!({"count": count}));
            let st = s.doc().unwrap();
            assert_eq!(st.doc.story(sid).unwrap().text, changed, "{text} {a}->{b}");
            assert_eq!(st.selection, expected, "{text} {a}->{b}");
            assert!(changed.is_char_boundary(after_a) && changed.is_char_boundary(after_b));
            if after_a == after_b {
                assert_eq!(
                    s.execute("find.find", &json!({"find": "(?:)", "grep": true, "scope": "selection"})).unwrap(),
                    json!([{"story": sid.0, "start": after_a, "end": after_a, "text": ""}])
                );
            }
        }
    }

    #[test]
    fn find_scope_change_other_story_and_no_hits_preserve_selection() {
        for explicit in [false, true] {
            let (mut s, sid) = scope_session("éX");
            let frame = s.doc().unwrap().selection.text.unwrap().frame.unwrap();
            let other = s.execute("frame.create", &json!({"rect": [120, 36, 200, 100], "content": "text", "text": "a"})).unwrap();
            let other_sid = designcraft_doc::StoryId(other["story"].as_u64().unwrap());
            scope_select(&mut s, sid, 3, 2);
            let before = s.doc().unwrap().selection.clone();
            let mut p = json!({"find": "a", "change": "🙂", "scope": "document"});
            if explicit {
                p["story"] = json!(other_sid.0);
            }
            assert_eq!(s.execute("find.change", &p).unwrap(), json!({"count": 1}));
            assert_eq!(s.doc().unwrap().doc.story(sid).unwrap().text, "éX");
            assert_eq!(s.doc().unwrap().doc.story(other_sid).unwrap().text, "🙂");
            assert_eq!(s.doc().unwrap().selection, before);
            assert_eq!(s.execute("find.change", &p).unwrap(), json!({"count": 0}));
            assert_eq!(s.doc().unwrap().selection, before);
            assert_eq!(s.doc().unwrap().doc.story(sid).unwrap().text, "éX");
            assert_eq!(s.doc().unwrap().doc.story(other_sid).unwrap().text, "🙂");

            s.execute("selection.set", &json!({"ids": [frame.0]})).unwrap();
            let before = s.doc().unwrap().selection.clone();
            assert!(before.text.is_none());
            p["find"] = json!("🙂");
            p["change"] = json!("é");
            assert_eq!(s.execute("find.change", &p).unwrap(), json!({"count": 1}));
            assert_eq!(s.doc().unwrap().selection, before);
            assert_eq!(s.doc().unwrap().doc.story(sid).unwrap().text, "éX");
            assert_eq!(s.doc().unwrap().doc.story(other_sid).unwrap().text, "é");
        }
    }

    #[test]
    fn find_scope_mixed_grep_matches_count_next_and_change() {
        for (text, pattern, expected, changed) in [
            ("a", "a*", vec![(0, 1)], "X"),
            ("aa", "a|(?:)", vec![(0, 1), (1, 2)], "XX"),
            ("ab", "a*", vec![(0, 1), (2, 2)], "XbX"),
            ("éa", "é|(?:)", vec![(0, 2), (3, 3)], "XaX"),
        ] {
            let (mut s, sid) = scope_session(text);
            scope_select(&mut s, sid, 0, text.len());
            let p = json!({"find": pattern, "grep": true, "scope": "selection", "change": "X"});
            let counted = s.execute("find.find", &p).unwrap();
            let ranges: Vec<_> =
                counted.as_array().unwrap().iter().map(|m| (m["start"].as_u64().unwrap() as usize, m["end"].as_u64().unwrap() as usize)).collect();
            assert_eq!(ranges, expected, "{pattern} on {text}");
            for (a, b) in &expected {
                scope_hit(&mut s, &p, sid, *a, *b);
            }
            for _ in 0..2 {
                assert_eq!(s.execute("find.next", &p).unwrap(), serde_json::Value::Null, "{pattern} on {text}");
            }
            assert_eq!(s.execute("find.find", &p).unwrap(), counted);
            assert_eq!(s.execute("find.change", &p).unwrap()["count"], expected.len());
            assert_eq!(s.doc().unwrap().doc.story(sid).unwrap().text, changed);
        }
    }

    #[test]
    fn find_scope_capture_replacement_and_first_keep_original_domain() {
        let (mut s, sid) = scope_session("a1 a2 | a3");
        scope_select(&mut s, sid, 0, 5);
        let p = json!({"find": r"a(\d)", "grep": true, "scope": "selection", "change": "b$1", "first": true});
        scope_hit(&mut s, &p, sid, 0, 2);
        scope_hit(&mut s, &p, sid, 3, 5);
        assert_eq!(s.execute("find.change", &p).unwrap()["count"], 1);
        assert_eq!(s.doc().unwrap().doc.story(sid).unwrap().text, "b1 a2 | a3");
    }

    #[test]
    fn find_scope_selection_retains_nonempty_hits_count_change_and_native_text() {
        for (a, b) in [(0, 7), (7, 0)] {
            let (mut s, sid) = scope_session("red red | red");
            scope_select(&mut s, sid, a, b);
            let p = json!({"find": "red", "change": "blue", "scope": "selection", "attrs": {"fontStyle": "Italic"}});
            let before = s.doc().unwrap().doc.clone();
            let selection = s.doc().unwrap().selection.clone();
            let revision = s.doc().unwrap().revision;
            let history = s.doc().unwrap().history.undo.len();
            let strong = std::sync::Arc::strong_count(&before);
            assert_eq!(s.execute("find.find", &p).unwrap().as_array().unwrap().len(), 2);
            assert_eq!(s.doc().unwrap().selection, selection);
            assert_eq!(s.doc().unwrap().revision, revision);
            assert_eq!(std::sync::Arc::strong_count(&before), strong, "search stores weak document identity");
            scope_hit(&mut s, &p, sid, 0, 3);
            scope_hit(&mut s, &p, sid, 4, 7);
            assert_eq!(s.execute("find.next", &p).unwrap(), serde_json::Value::Null);
            assert_eq!(s.execute("find.next", &p).unwrap(), serde_json::Value::Null);
            assert_eq!(s.execute("find.find", &p).unwrap().as_array().unwrap().len(), 2);
            assert!(std::sync::Arc::ptr_eq(&before, &s.doc().unwrap().doc));
            assert_eq!(s.doc().unwrap().history.undo.len(), history);
            assert_eq!(s.execute("find.change", &p).unwrap()["count"], 2);
            let changed = s.doc().unwrap().doc.story(sid).unwrap().clone();
            assert_eq!(changed.text, "blue blue | red");
            assert_eq!(changed.char_format_at(0).over.font_style.as_deref(), Some("Italic"));
            assert_eq!(s.doc().unwrap().history.undo.len(), history + 1);
            assert!(s.find_context.is_none());
            s.execute("edit.undo", &json!({})).unwrap();
            assert_eq!(s.doc().unwrap().doc.story(sid).unwrap().text, "red red | red");
            assert!(s.find_context.is_none());
            s.execute("edit.redo", &json!({})).unwrap();
            assert_eq!(s.doc().unwrap().doc.story(sid).unwrap(), &changed);
            let native = s.execute("file.serialize", &json!({})).unwrap();
            s.execute("file.openBytes", &json!({"name": "scope.designcraft", "base64": native["base64"]})).unwrap();
            assert_eq!(serde_json::to_value(s.doc().unwrap().doc.story(sid).unwrap()).unwrap(), serde_json::to_value(&changed).unwrap());
            assert!(s.find_context.is_none());
        }
    }

    #[test]
    fn find_scope_matcher_restart_replacement_continuation_and_external_reselection() {
        let (mut s, sid) = scope_session("red blue red | blue red");
        scope_select(&mut s, sid, 0, 12);
        let mut p = json!({"find": "red", "scope": "selection", "change": "one"});
        scope_hit(&mut s, &p, sid, 0, 3);
        p["change"] = json!("two");
        p["attrs"] = json!({"fontStyle": "Italic"});
        scope_hit(&mut s, &p, sid, 9, 12);
        p["find"] = json!("blue");
        scope_hit(&mut s, &p, sid, 4, 8);
        p["find"] = json!("red");
        scope_hit(&mut s, &p, sid, 0, 3);
        p["caseSensitive"] = json!(true);
        scope_hit(&mut s, &p, sid, 0, 3);
        p["grep"] = json!(true);
        scope_hit(&mut s, &p, sid, 0, 3); // GREP alone changes matcher identity.
        p["find"] = json!("r.d");
        scope_hit(&mut s, &p, sid, 0, 3);
        p["wholeWord"] = json!(true);
        scope_hit(&mut s, &p, sid, 0, 3);
        let hit = s.doc().unwrap().selection.text.unwrap();
        scope_select(&mut s, sid, hit.anchor, hit.focus);
        assert_eq!(s.execute("find.find", &p).unwrap().as_array().unwrap().len(), 1, "same-range external reselection starts a new domain");
        scope_hit(&mut s, &p, sid, 0, 3);
        assert_eq!(s.execute("find.next", &p).unwrap(), serde_json::Value::Null);
        scope_select(&mut s, sid, 20, 23);
        p["find"] = json!("red");
        scope_hit(&mut s, &p, sid, 20, 23);
    }

    #[test]
    fn find_scope_word_reselection_at_same_hit_recaptures_bounds() {
        let (mut s, sid) = scope_session("red red | red");
        scope_select(&mut s, sid, 0, 7);
        let p = json!({"find": "red", "change": "blue", "scope": "selection"});
        assert_eq!(s.execute("find.find", &p).unwrap().as_array().unwrap().len(), 2);
        scope_hit(&mut s, &p, sid, 0, 3);
        let selected = s.doc().unwrap().selection.clone();
        let frame = selected.text.unwrap().frame.unwrap();
        let point = {
            let st = s.doc().unwrap();
            let cs = s.cache.get(&st.doc, sid, None);
            let (fi, x, baseline, ascent, _) = designcraft_compose::caret(&cs, 1).unwrap();
            let inner = designcraft_geom::Point::new(x, baseline - ascent * 0.5);
            assert_eq!(cs.frames[fi].frame, frame);
            assert_eq!(designcraft_compose::hit(&cs, fi, inner), Some(1), "real point lies inside the current hit");
            let loc = st.doc.find(frame).unwrap();
            st.doc.parent_xf(&loc) * st.doc.text_xf(st.doc.item(frame).unwrap()) * inner
        };
        let before = s.doc().unwrap().doc.clone();
        let revision = s.doc().unwrap().revision;
        let history = s.doc().unwrap().history.undo.len();
        s.execute("text.selectWord", &json!({"frame": frame.0, "point": [point.x, point.y]})).unwrap();
        assert!(!s.text_drag, "explicit word reselection does not start a text drag");
        assert_eq!(s.doc().unwrap().selection, selected, "same full selection, including frame");
        assert!(s.doc().unwrap().revision > revision, "a new revision");
        assert!(std::sync::Arc::ptr_eq(&before, &s.doc().unwrap().doc));
        assert_eq!(s.doc().unwrap().history.undo.len(), history);
        assert!(s.find_context.is_none());
        assert_eq!(s.execute("find.find", &p).unwrap(), json!([{"story": sid.0, "start": 0, "end": 3, "text": "red"}]));
        assert_eq!(s.execute("find.change", &p).unwrap(), json!({"count": 1}));
        assert_eq!(s.doc().unwrap().doc.story(sid).unwrap().text, "blue red | red");
        assert_eq!(s.doc().unwrap().history.undo.len(), history + 1);
        s.execute("edit.undo", &json!({})).unwrap();
        assert_eq!(s.doc().unwrap().doc.story(sid).unwrap().text, "red red | red");
        s.execute("edit.redo", &json!({})).unwrap();
        assert_eq!(s.doc().unwrap().doc.story(sid).unwrap().text, "blue red | red");
    }

    #[test]
    fn find_scope_story_explicit_story_document_order_and_scope_changes() {
        let (mut s, a) = scope_session("black red");
        let other = s.execute("frame.create", &json!({"rect": [150, 36, 100, 100], "content": "text", "text": "red"})).unwrap();
        let b = designcraft_doc::StoryId(other["story"].as_u64().unwrap());
        scope_select(&mut s, a, 0, 9);
        let story = json!({"find": "red", "scope": "story"});
        assert_eq!(s.execute("find.next", &story).unwrap(), serde_json::Value::Null, "Story does not enter B or wrap to its prefix");
        scope_select(&mut s, b, 0, 0);
        let explicit = json!({"find": "black", "story": a.0, "scope": "selection"});
        scope_hit(&mut s, &explicit, a, 0, 5);
        let selection = s.doc().unwrap().selection.clone();
        assert!(s.execute("find.next", &json!({"find": "red", "story": u64::MAX})).is_err());
        assert_eq!(s.doc().unwrap().selection, selection);
        scope_select(&mut s, a, 0, 9);
        scope_hit(&mut s, &json!({"find": "red"}), b, 0, 3);
        let counted = s.execute("find.find", &json!({"find": "red"})).unwrap();
        assert_eq!(counted[0]["story"], a.0, "Count retains canonical story order");
        assert_eq!(counted[1]["story"], b.0);
        scope_select(&mut s, a, 0, 9);
        scope_hit(&mut s, &json!({"find": "red", "scope": "selection"}), a, 6, 9);
        assert_eq!(s.execute("find.next", &story).unwrap(), serde_json::Value::Null, "scope change uses the current caret");
        s.execute("selection.set", &json!({"ids": []})).unwrap();
        for command in ["find.find", "find.next", "find.change"] {
            assert!(s.execute(command, &json!({"find": "red", "change": "blue", "scope": "selection"})).is_err());
        }
    }

    #[test]
    fn find_scope_substring_context_utf8_zero_width_and_invalid_ranges() {
        let (mut s, sid) = scope_session("scat é🙂 red");
        scope_select(&mut s, sid, 1, 4);
        let p = json!({"find": "cat", "wholeWord": true, "scope": "selection"});
        scope_hit(&mut s, &p, sid, 1, 4);
        assert_eq!(s.execute("find.find", &p).unwrap().as_array().unwrap().len(), 1);
        scope_select(&mut s, sid, 1, 4);
        let p = json!({"find": "^c|^a", "grep": true, "scope": "selection"});
        scope_hit(&mut s, &p, sid, 1, 2);
        assert_eq!(s.execute("find.next", &p).unwrap(), serde_json::Value::Null, "cursor does not redefine ^");
        scope_select(&mut s, sid, 5, 11);
        let empty = json!({"find": "(?:)", "grep": true, "scope": "selection"});
        for pos in [5, 7, 11] {
            scope_hit(&mut s, &empty, sid, pos, pos);
        }
        for _ in 0..2 {
            assert_eq!(s.execute("find.next", &empty).unwrap(), serde_json::Value::Null);
        }
        assert_eq!(s.execute("find.find", &empty).unwrap().as_array().unwrap().len(), 3);
        scope_select(&mut s, sid, 7, 7);
        scope_hit(&mut s, &empty, sid, 7, 7);
        assert_eq!(s.execute("find.next", &empty).unwrap(), serde_json::Value::Null);
        scope_select(&mut s, sid, 12, 15);
        let nonempty = json!({"find": "red", "change": "blue", "scope": "selection"});
        scope_hit(&mut s, &nonempty, sid, 12, 15);
        assert_eq!(s.execute("find.change", &nonempty).unwrap()["count"], 1);
        assert_eq!(s.doc().unwrap().doc.story(sid).unwrap().text, "scat é🙂 blue");
        for range in [6..7, 0..usize::MAX] {
            let st = s.doc_mut().unwrap();
            st.selection = designcraft_doc::Selection::text(designcraft_doc::TextSel {
                story: sid,
                anchor: range.start,
                focus: range.end,
                frame: None,
                cell: None,
            });
            st.bump_revision();
            for command in ["find.find", "find.next", "find.change"] {
                let before = s.doc().unwrap().doc.clone();
                let err = s.execute(command, &json!({"find": "x", "change": "y", "scope": "selection"})).unwrap_err();
                assert!(matches!(err, crate::EngineError::BadParams { .. }), "{err}");
                assert!(std::sync::Arc::ptr_eq(&before, &s.doc().unwrap().doc));
            }
        }
    }

    #[test]
    fn find_scope_invalid_query_keeps_continuation_and_cross_boundary_nonmatches() {
        let (mut s, sid) = scope_session("red red | red");
        scope_select(&mut s, sid, 0, 7);
        let p = json!({"find": "red", "scope": "selection"});
        scope_hit(&mut s, &p, sid, 0, 3);
        let selected = s.doc().unwrap().selection.clone();
        assert!(s.execute("find.next", &json!({"find": "[", "grep": true, "scope": "selection"})).is_err());
        assert_eq!(s.doc().unwrap().selection, selected);
        scope_hit(&mut s, &p, sid, 4, 7);
        assert_eq!(s.execute("find.find", &json!({"find": "red |", "scope": "selection"})).unwrap(), json!([]));
    }

    #[test]
    fn find_scope_document_switch_edit_and_reset() {
        let (mut s, sid) = scope_session("red red | red");
        scope_select(&mut s, sid, 0, 7);
        let p = json!({"find": "red", "scope": "selection"});
        scope_hit(&mut s, &p, sid, 0, 3);
        s.execute("file.new", &json!({})).unwrap();
        s.set_active(0);
        assert_eq!(s.execute("find.find", &p).unwrap().as_array().unwrap().len(), 1);
        scope_select(&mut s, sid, 0, 7);
        scope_hit(&mut s, &p, sid, 0, 3);
        s.execute("story.setText", &json!({"story": sid.0, "text": "red"})).unwrap();
        assert!(s.find_context.is_none());
        assert_eq!(s.execute("find.find", &p).unwrap().as_array().unwrap().len(), 1);
        s.reset_find_scope();
        assert!(s.find_context.is_none());
    }

    #[test]
    fn find_scope_real_preview_restores_before_resolving_bounds() {
        let (mut s, sid) = scope_session("");
        s.execute(
            "data.source.select",
            &json!({
                "bytes": crate::cmd::base64_encode(b"x\nred red | red\n"), "name": "scope.csv"
            }),
        )
        .unwrap();
        s.execute("data.placeholder.add", &json!({"field": "x", "story": sid.0, "at": 0})).unwrap();
        let template = s.doc().unwrap().doc.clone();
        assert_eq!(template.story(sid).unwrap().text, "<<x>>");
        s.execute("data.preview", &json!({"record": 1})).unwrap();
        assert_eq!(s.doc().unwrap().doc.story(sid).unwrap().text, "red red | red");
        // Install a viewport selection in the real preview. text.select is journaled
        // and would itself restore the template before selecting.
        let st = s.doc_mut().unwrap();
        st.selection = designcraft_doc::Selection::text(designcraft_doc::TextSel { story: sid, anchor: 0, focus: 7, frame: None, cell: None });
        st.bump_revision();
        let p = json!({"find": "red", "change": "blue", "scope": "selection"});
        assert_eq!(s.execute("find.find", &p).unwrap().as_array().unwrap().len(), 2);
        let history = s.doc().unwrap().history.undo.len();
        let err = s.execute("find.change", &p).unwrap_err();
        assert!(matches!(err, crate::EngineError::BadParams { .. }), "{err}");
        assert!(std::sync::Arc::ptr_eq(&template, &s.doc().unwrap().doc));
        assert_eq!(s.doc().unwrap().doc.story(sid).unwrap().text, "<<x>>");
        assert_eq!(s.doc().unwrap().history.undo.len(), history);
        assert!(s.doc().unwrap().preview_stash.is_none());
        assert!(s.find_context.is_none());
        // Valid fresh bounds are allowed after restoration; no cached preview range
        // survives. A document query can see the preview, but Next sees the template.
        s.execute("data.preview", &json!({"record": 1})).unwrap();
        let document = json!({"find": "red"});
        assert_eq!(s.execute("find.find", &document).unwrap().as_array().unwrap().len(), 3);
        assert_eq!(s.execute("find.next", &document).unwrap(), serde_json::Value::Null);
        assert!(std::sync::Arc::ptr_eq(&template, &s.doc().unwrap().doc));
        assert_eq!(s.execute("find.find", &document).unwrap(), json!([]));
        assert_eq!(s.doc().unwrap().history.undo.len(), history);
    }

    #[test]
    fn document_scope_searches_and_changes_table_cells() {
        let (mut s, sid) = scope_session("Ore 12.15 here");
        scope_select(&mut s, sid, 0, 0);
        let table = s.execute("table.insert", &json!({"rows": 1, "cols": 2})).unwrap()["table"].as_u64().unwrap();
        s.execute("table.setCell", &json!({"row": 0, "col": 1, "text": "Ore 12.15"})).unwrap();
        // The caret sits in the first cell, so Find Next goes to the second cell, then the story.
        let p = json!({"find": "Ore 12.15", "scope": "document"});
        let cell = designcraft_doc::CellAddr { table, row: 0, col: 1 };
        assert_eq!(s.execute("find.next", &p).unwrap(), json!({"story": sid.0, "start": 0, "end": 9, "cell": {"table": table, "row": 0, "col": 1}}));
        assert_eq!(s.doc().unwrap().selection.text.map(|t| (t.story, t.range(), t.cell)), Some((sid, 0..9, Some(cell))));
        let next = s.execute("find.next", &p).unwrap();
        assert!(next.get("start").is_some() && next.get("cell").is_none(), "{next}");
        let hits = s.execute("find.find", &p).unwrap();
        let hits = hits.as_array().unwrap();
        assert_eq!(hits.len(), 2);
        let in_cell: Vec<_> = hits.iter().filter(|h| h.get("cell").is_some()).collect();
        assert_eq!(in_cell.len(), 1);
        assert_eq!((in_cell[0]["cell"]["row"].as_u64(), in_cell[0]["cell"]["col"].as_u64()), (Some(0), Some(1)));
        let n = s.execute("find.change", &json!({"find": "Ore 12.15", "change": "Ore 13.00", "scope": "document"})).unwrap();
        assert_eq!(n["count"], 2);
        assert!(s.execute("find.find", &json!({"find": "12.15", "scope": "document"})).unwrap().as_array().unwrap().is_empty());
        assert_eq!(s.execute("find.find", &json!({"find": "13.00", "scope": "document"})).unwrap().as_array().unwrap().len(), 2);
    }

    fn child_scope_session(note: bool, child: &str) -> (Session, designcraft_doc::StoryId, designcraft_doc::TextSel) {
        let (mut s, sid) = scope_session("red");
        scope_select(&mut s, sid, 3, 3);
        if note {
            s.execute("footnote.insert", &json!({"text": child})).unwrap();
        } else {
            s.execute("table.insert", &json!({"rows": 1, "cols": 1})).unwrap();
            s.execute("text.insert", &json!({"text": child, "raw": true})).unwrap();
        }
        let selected = s.doc().unwrap().selection.text.unwrap();
        (s, sid, selected)
    }

    #[test]
    fn find_scope_children_error_atomically_and_parent_requests_stay_top_level() {
        for note in [false, true] {
            for child in ["red", "red x", "é red red red red red"] {
                let (mut s, sid, mut selected) = child_scope_session(note, child);
                selected.anchor = 0;
                selected.focus = child.len();
                s.doc_mut().unwrap().selection = designcraft_doc::Selection::text(selected);
                let before = s.doc().unwrap().doc.clone();
                let selection = s.doc().unwrap().selection.clone();
                let history = s.doc().unwrap().history.undo.len();
                let revision = s.doc().unwrap().revision;
                let journal = s.journal.clone();
                for scope in ["story", "selection"] {
                    for command in ["find.find", "find.next", "find.change"] {
                        let err = s.execute(command, &json!({"find": "red", "change": "blue", "scope": scope})).unwrap_err();
                        assert!(matches!(err, crate::EngineError::BadParams { .. }), "{err}");
                        assert!(err.to_string().contains("not supported"));
                        assert!(std::sync::Arc::ptr_eq(&before, &s.doc().unwrap().doc));
                        assert_eq!(s.doc().unwrap().selection, selection);
                        assert_eq!(s.doc().unwrap().history.undo.len(), history);
                        assert_eq!(s.doc().unwrap().revision, revision);
                        assert_eq!(s.journal, journal);
                    }
                }
                for explicit in [false, true] {
                    let mut p = json!({"find": "red", "change": "blue"});
                    if explicit {
                        p["story"] = json!(sid.0);
                        p["scope"] = json!("selection");
                    }
                    // Document scope also covers table cells (#1624); an explicit story and footnotes stay top-level.
                    let in_cell = if explicit || note { 0 } else { child.matches("red").count() };
                    let changed = if in_cell > 0 { child.replace("red", "blue") } else { child.to_string() };
                    s.doc_mut().unwrap().selection = selection.clone();
                    assert_eq!(s.execute("find.find", &p).unwrap().as_array().unwrap().len(), 1 + in_cell);
                    scope_hit(&mut s, &p, sid, 0, 3);
                    s.doc_mut().unwrap().selection = selection.clone();
                    assert_eq!(s.execute("find.change", &p).unwrap()["count"], 1 + in_cell);
                    assert_eq!(s.doc().unwrap().doc.text_story(sid, selected.cell).unwrap().text, changed);
                    assert_eq!(s.doc().unwrap().selection, selection, "parent replacement does not clamp child offsets");
                    p["find"] = json!("blue");
                    p["change"] = json!("red");
                    s.execute("find.change", &p).unwrap();
                }
            }
        }
        let (mut s, _, _) = child_scope_session(false, "red");
        s.execute("table.selectTable", &json!({})).unwrap();
        for scope in ["story", "selection"] {
            for command in ["find.find", "find.next", "find.change"] {
                assert!(
                    s.execute(command, &json!({"find": "red", "change": "blue", "scope": scope})).unwrap_err().to_string().contains("not supported")
                );
            }
        }
    }

    #[test]
    fn find_scope_threaded_overset_story_uses_logical_offsets() {
        let text = format!("{}red", "word ".repeat(100));
        let (mut s, sid) = scope_session(&text);
        let a = s.doc().unwrap().doc.story(sid).unwrap().frames[0];
        let next = s.execute("frame.create", &json!({"rect": [150, 36, 20, 1], "content": "text"})).unwrap();
        let b = designcraft_doc::ItemId(next["id"].as_u64().unwrap());
        s.edit(|d, _| Ok(d.thread(a, b)?)).unwrap();
        assert_eq!(s.doc().unwrap().doc.story(sid).unwrap().frames.len(), 2);
        scope_select(&mut s, sid, 0, 0);
        scope_hit(&mut s, &json!({"find": "red", "scope": "story"}), sid, 500, 503);
        assert_eq!(s.execute("find.next", &json!({"find": "red", "scope": "story"})).unwrap(), serde_json::Value::Null);
    }

    #[test]
    fn find_scope_zero_width_story_explicit_and_document_endpoints_terminate() {
        for scope in ["story", "explicit", "document"] {
            let (mut s, sid) = scope_session("é🙂");
            scope_select(&mut s, sid, 0, 0);
            let mut p = json!({"find": "(?:)", "grep": true, "scope": scope});
            if scope == "explicit" {
                p["story"] = json!(sid.0);
            }
            for pos in [0, 2, 6] {
                scope_hit(&mut s, &p, sid, pos, pos);
            }
            assert_eq!(s.execute("find.next", &p).unwrap(), serde_json::Value::Null);
            assert_eq!(s.execute("find.next", &p).unwrap(), serde_json::Value::Null);
        }
    }

    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    fn find_scope_revert_releases_context_with_same_document_uid() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("scope.designcraft");
        let (mut s, sid) = scope_session("red red | red");
        s.execute("file.save", &json!({"path": path.to_string_lossy()})).unwrap();
        scope_select(&mut s, sid, 0, 7);
        let p = json!({"find": "red", "scope": "selection"});
        scope_hit(&mut s, &p, sid, 0, 3);
        let uid = s.doc().unwrap().uid;
        s.execute("story.setText", &json!({"story": sid.0, "text": "red red"})).unwrap();
        scope_select(&mut s, sid, 0, 7);
        scope_hit(&mut s, &p, sid, 0, 3);
        s.execute("file.revert", &json!({})).unwrap();
        assert_eq!(s.doc().unwrap().uid, uid);
        assert!(s.find_context.is_none());
        assert_eq!(s.doc().unwrap().doc.story(sid).unwrap().text, "red red | red");
        assert!(s.execute("find.find", &p).is_err(), "revert's empty selection cannot inherit a previous range");
    }
}

/// Objects matching Find/Change › Object criteria, in document order (nested ones included).
fn find_objects(d: &designcraft_doc::Document, p: &Value) -> Vec<designcraft_doc::ItemId> {
    use designcraft_doc::{Content, Shape};
    let fill = str_param(p, "fill");
    let stroke = str_param(p, "stroke");
    let weight = p.get("strokeWeight").and_then(Value::as_f64);
    let opacity = p.get("opacity").and_then(Value::as_f64);
    let kind = str_param(p, "kind");
    let layer = str_param(p, "layer").and_then(|n| d.layers.iter().find(|l| l.name == n).map(|l| l.id));
    if str_param(p, "layer").is_some() && layer.is_none() {
        return vec![];
    }
    let label = str_param(p, "label");
    let mut out = Vec::new();
    for sp in d.spreads.iter().chain(d.parents.iter()) {
        for top in &sp.items {
            top.walk(&mut |it| {
                let k = match (&it.content, it.shape) {
                    (_, Shape::Group) => "group",
                    (Content::Text(_), _) => "text",
                    (Content::Graphic(_), _) => "graphic",
                    (_, Shape::GraphicLine) => "line",
                    _ => "shape",
                };
                let ok = fill.is_none_or(|f| it.fill.swatch == f)
                    && stroke.is_none_or(|c| it.stroke.swatch == c && it.stroke.weight > 0.0)
                    && weight.is_none_or(|w| (it.stroke.weight - w).abs() < 1e-6)
                    && opacity.is_none_or(|o| (it.opacity as f64 - o).abs() < 1e-3)
                    && kind.is_none_or(|kk| kk == k)
                    && layer.is_none_or(|l| it.layer == l)
                    && label.is_none_or(|lb| it.label == lb);
                if ok {
                    out.push(it.id);
                }
            });
        }
    }
    out
}

#[cfg(test)]
mod object_find_tests {
    use serde_json::json;

    use crate::Session;

    #[test]
    fn find_and_change_objects_by_attributes() {
        let mut s = Session::new();
        s.execute("file.new", &json!({})).unwrap();
        let a = s.execute("frame.create", &json!({"rect": [10, 10, 50, 50], "content": "unassigned"})).unwrap()["id"].clone();
        let _b = s.execute("frame.create", &json!({"rect": [60, 10, 90, 50], "content": "unassigned"})).unwrap()["id"].clone();
        s.execute("object.fill", &json!({"swatch": "[Black]", "ids": [a]})).unwrap();
        s.execute("frame.create", &json!({"rect": [10, 60, 90, 90], "content": "text", "text": "t"})).unwrap();
        let r = s.execute("find.objects", &json!({"fill": "[Black]"})).unwrap();
        assert_eq!(r["ids"], json!([a]));
        assert_eq!(s.doc().unwrap().selection.items.len(), 1, "found objects are selected");
        assert_eq!(s.execute("find.objects", &json!({"kind": "text", "select": false})).unwrap()["ids"].as_array().unwrap().len(), 1);
        let c = s.execute("find.changeObjects", &json!({"kind": "shape", "change": {"opacity": 0.5}})).unwrap();
        assert_eq!(c["changed"], 2);
        assert_eq!(s.execute("find.objects", &json!({"opacity": 0.5, "select": false})).unwrap()["ids"].as_array().unwrap().len(), 2);
    }
}
