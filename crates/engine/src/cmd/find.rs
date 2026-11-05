//! Edit → Find/Change: plain text and GREP (regular expressions) with InDesign's metacharacters,
//! a Find Format and a Change Format, scoped to the document, a story or the selection.

use std::ops::Range;
use std::sync::{Arc, Weak};

use designcraft_compose::grep::{Grep, GrepOptions, translate_replacement};
use designcraft_doc::story::floor_char_boundary;
use designcraft_doc::{CellAddr, Document, Selection, Story, StoryId, Styles, TextSel};
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
            "{find, grep?: bool, caseSensitive?: bool, wholeWord?: bool, scope?: document|story|selection, story?, findFormat?: {charStyle?, paraStyle?, chars?: resolved character attributes} (Find Format: only text with it all through; with an empty find, each stretch of text with it), change?|first?|found?|attrs?|charStyle?|paraStyle?: ignored (find.change takes them; one request serves all three Find/Change commands)} → matches [{story, start, end, text, cell?: {table, row, col}}] (document scope also searches table cells; start/end then index the cell's text)",
            has_doc, find),
        cmd!(
            "find.change",
            "Change All",
            ["Edit", "Find/Change"],
            None,
            "{find, change, grep?, caseSensitive?, wholeWord?, scope?, story?, findFormat?: {charStyle?, paraStyle?, chars?: resolved character attributes} (Find Format: only text with it all through; with an empty find, each stretch of text with it), first?: bool (change only the first match), found?: bool (Change: only the text Find Next found, while it is selected and still a match; it stays selected, and Find Next goes on after it in the same domain), attrs?: {character attributes applied to changed text}, charStyle?: a character style applied to it, paraStyle?: a paragraph style applied to its paragraphs} → {count} — attrs, charStyle and paraStyle are the Change Format: with one and an empty change, the found text stays and is only formatted; with no find, change is literal text",
            has_doc,
            change
        ),
        cmd!(noundo "find.next", "Find Next", ["Edit"], Some("Cmd+Alt+F"), "{find, grep?, caseSensitive?, wholeWord?, scope?: document|story|selection, story?, findFormat?: {charStyle?, paraStyle?, chars?: resolved character attributes} (Find Format: only text with it all through; with an empty find, each stretch of text with it), change?|first?|found?|attrs?|charStyle?|paraStyle?: ignored (find.change takes them; one request serves all three Find/Change commands)} — Selection starts at its original beginning; later calls advance within the scope", has_doc, find_next),
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

/// Find/Change's Find Format: the text a search finds must have it all through.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
struct FindFormat {
    char_style: Option<String>,
    para_style: Option<String>,
    /// Resolved character attributes, as `type.selectionAttrs` reports them.
    chars: serde_json::Map<String, Value>,
}

impl FindFormat {
    fn from_params(p: &Value) -> Result<Option<Self>> {
        let Some(f) = p.get("findFormat").filter(|f| !f.is_null()) else { return Ok(None) };
        let o = f.as_object().ok_or_else(|| bad("find", "`findFormat` is an object"))?;
        // A style is named; a null asks for none. Any other value is refused rather than
        // searched without.
        let name = |k: &str| match o.get(k) {
            None | Some(Value::Null) => Ok(None),
            Some(Value::String(n)) => Ok(Some(n.clone())),
            Some(v) => Err(bad("find", format!("findFormat: `{k}` is a style name, not {v}"))),
        };
        let chars = match o.get("chars") {
            None | Some(Value::Null) => None,
            Some(Value::Object(m)) => Some(m),
            Some(v) => return Err(bad("find", format!("findFormat: `chars` is an object of character attributes, not {v}"))),
        };
        // Only attributes text has, kept as the document writes them: `font_style` and
        // `fontStyle`, or 12 and 12.0, then find the same text. A null asks for nothing.
        let mut known = designcraft_doc::CharAttrs::default();
        for (k, v) in chars.into_iter().flatten() {
            known.set_json(k, v).map_err(|e| bad("find", format!("findFormat: {e}")))?;
        }
        let chars = match serde_json::to_value(&known) {
            Ok(Value::Object(m)) => m,
            _ => serde_json::Map::new(),
        };
        let f = Self { char_style: name("charStyle")?, para_style: name("paraStyle")?, chars };
        Ok((f != Self::default()).then_some(f))
    }

    /// Styles the document doesn't have are refused, as `find.change` refuses them.
    fn check(&self, styles: &Styles) -> Result<()> {
        if let Some(n) = self.char_style.as_ref().filter(|n| styles.char_style(n).is_none()) {
            return Err(bad("find", format!("no character style `{n}`")));
        }
        if let Some(n) = self.para_style.as_ref().filter(|n| styles.para(n).is_none()) {
            return Err(bad("find", format!("no paragraph style `{n}`")));
        }
        Ok(())
    }

    /// The text in `r` of `story` that has the format, relative to `r.start`: the longest
    /// stretches, each paragraph's apart (a paragraph's `\n` isn't in one).
    fn spans(&self, styles: &Styles, story: &Story, r: &Range<usize>) -> Vec<Range<usize>> {
        let mut out: Vec<Range<usize>> = Vec::new();
        // Paragraphs and runs both go in text order: the runs before a paragraph are passed once.
        let runs: Vec<_> = story.runs().collect();
        let mut first = 0;
        for (pi, pr) in story.para_ranges().into_iter().enumerate() {
            let (a, b) = (pr.start.max(r.start), pr.end.min(r.end));
            let Some(pf) = story.paras.get(pi) else { continue };
            if a >= b || self.para_style.as_ref().is_some_and(|n| *n != pf.style) {
                continue;
            }
            while runs.get(first).is_some_and(|(run, _)| run.end <= a) {
                first += 1;
            }
            let base = (!self.chars.is_empty()).then(|| styles.resolve_para(pf).1);
            for (run, cf) in runs[first..].iter().take_while(|(run, _)| run.start < b) {
                let (x, y) = (run.start.max(a), run.end.min(b));
                if x >= y || self.char_style.as_ref().is_some_and(|n| *n != cf.style) {
                    continue;
                }
                if let Some(base) = &base {
                    let cp = serde_json::to_value(styles.resolve_char(base, cf)).unwrap_or_default();
                    if !self.chars.iter().all(|(k, v)| cp.get(k).is_some_and(|c| same_value(c, v))) {
                        continue;
                    }
                }
                match out.last_mut() {
                    Some(last) if last.end == x - r.start => last.end = y - r.start,
                    _ => out.push(x - r.start..y - r.start),
                }
            }
        }
        out
    }
}

/// Attribute values alike: numbers to within rounding (inside objects and lists too), the rest
/// exactly.
fn same_value(a: &Value, b: &Value) -> bool {
    match (a, b) {
        (Value::Number(_), Value::Number(_)) => match (a.as_f64(), b.as_f64()) {
            (Some(x), Some(y)) => (x - y).abs() < 1e-6,
            _ => a == b,
        },
        (Value::Array(x), Value::Array(y)) => x.len() == y.len() && x.iter().zip(y).all(|(a, b)| same_value(a, b)),
        (Value::Object(x), Value::Object(y)) => x.len() == y.len() && x.iter().all(|(k, v)| y.get(k).is_some_and(|w| same_value(v, w))),
        _ => a == b,
    }
}

/// Whether match `m` lies in one of `spans`.
fn within(spans: &[Range<usize>], m: &Range<usize>) -> bool {
    spans.iter().any(|s| s.start <= m.start && m.end <= s.end)
}

/// What a search finds: the pattern's matches (with a Find Format, those whose text has it all
/// through), or, with no pattern, the stretches of text with the Find Format.
struct Search {
    re: Option<Grep>,
    format: Option<FindFormat>,
}

/// The search of a Find/Change request: an empty `find` needs a Find Format.
fn search(p: &Value) -> Result<Search> {
    let format = FindFormat::from_params(p)?;
    if format.is_some() && str_param(p, "find") == Some("") {
        return Ok(Search { re: None, format });
    }
    Ok(Search { re: Some(build(p)?), format })
}

impl Search {
    fn check(&self, styles: &Styles) -> Result<()> {
        self.format.as_ref().map_or(Ok(()), |f| f.check(styles))
    }

    /// Where the Find Format holds in target `r` of story `sid` (of its table cell `cell`, if any;
    /// `None`: no Find Format).
    fn spans(&self, d: &Document, sid: StoryId, cell: Option<CellAddr>, r: &Range<usize>) -> Option<Vec<Range<usize>>> {
        let f = self.format.as_ref()?;
        Some(d.text_story(sid, cell).map(|story| f.spans(&d.styles, story, r)).unwrap_or_default())
    }

    /// Up to `limit` matches in `text`, target `r` of story `sid` (offsets in `text`).
    fn find_all(&self, d: &Document, sid: StoryId, cell: Option<CellAddr>, r: &Range<usize>, text: &str, limit: usize) -> Result<Vec<Range<usize>>> {
        let spans = self.spans(d, sid, cell, r);
        let Some(re) = &self.re else { return Ok(spans.unwrap_or_default().into_iter().take(limit).collect()) };
        let Some(spans) = spans else { return re.find_all(text, limit).map_err(|e| bad("find", e)) };
        Ok(re.find_all(text, usize::MAX).map_err(|e| bad("find", e))?.into_iter().filter(|m| within(&spans, m)).take(limit).collect())
    }

    /// The first match in `text` (target `r` of story `sid`) at or after byte `from`.
    fn find_at(&self, d: &Document, sid: StoryId, cell: Option<CellAddr>, r: &Range<usize>, text: &str, from: usize) -> Result<Option<Range<usize>>> {
        let spans = self.spans(d, sid, cell, r);
        let Some(re) = &self.re else {
            // From inside a stretch, the rest of it.
            return Ok(spans.unwrap_or_default().into_iter().find(|s| s.end > from).map(|s| s.start.max(from)..s.end));
        };
        let mut at = from;
        loop {
            let Some(m) = re.find_at(text, at).map_err(|e| bad("find.next", e))? else { return Ok(None) };
            if spans.as_ref().is_none_or(|s| within(s, &m)) {
                return Ok(Some(m));
            }
            // On past it: its end, or a character on from an empty match.
            at = if m.is_empty() { text.get(m.end..).and_then(|t| t.chars().next()).map_or(text.len() + 1, |c| m.end + c.len_utf8()) } else { m.end };
            if at > text.len() {
                return Ok(None);
            }
        }
    }

    /// The edit Change makes when the selection `range` (offsets in `text`, target `r` of story
    /// `sid`) is what Find Next found there, so the two commands agree on what a match is. With
    /// no pattern: a selection inside one stretch with the Find Format (Find Next selects the rest
    /// of a stretch from inside it). With a pattern: the match it makes from the selection's start
    /// must be the selection, overlapping matches Find Next can select included.
    #[allow(clippy::too_many_arguments)]
    fn found_edit(
        &self,
        d: &Document,
        sid: StoryId,
        cell: Option<CellAddr>,
        r: &Range<usize>,
        text: &str,
        range: &Range<usize>,
        rep: &str,
        grep: bool,
    ) -> Result<Option<(Range<usize>, String)>> {
        let spans = self.spans(d, sid, cell, r);
        let Some(re) = &self.re else {
            let inside = !range.is_empty() && spans.unwrap_or_default().iter().any(|s| s.start <= range.start && range.end <= s.end);
            return Ok(inside.then(|| (range.clone(), rep.to_string())));
        };
        let found =
            if grep { re.replacement_at(text, range.start, rep) } else { re.find_at(text, range.start).map(|m| m.map(|m| (m, rep.to_string()))) };
        let found = found.map_err(|e| bad("find.change", e))?;
        Ok(found.filter(|(m, _)| m == range && spans.as_ref().is_none_or(|s| within(s, m))))
    }

    /// The edits Change All makes in `text` (target `r` of story `sid`): `rep` expanded for each
    /// GREP match, else `rep` itself.
    #[allow(clippy::too_many_arguments)]
    fn replacements(
        &self,
        d: &Document,
        sid: StoryId,
        cell: Option<CellAddr>,
        r: &Range<usize>,
        text: &str,
        rep: &str,
        grep: bool,
        first_only: bool,
    ) -> Result<Vec<(Range<usize>, String)>> {
        let limit = if first_only { 1 } else { usize::MAX };
        match (&self.re, self.spans(d, sid, cell, r)) {
            (Some(re), None) if grep => re.replacements(text, rep, first_only).map_err(|e| bad("find.change", e)),
            (Some(re), Some(spans)) if grep => Ok(re
                .replacements(text, rep, false)
                .map_err(|e| bad("find.change", e))?
                .into_iter()
                .filter(|(m, _)| within(&spans, m))
                .take(limit)
                .collect()),
            _ => Ok(self.find_all(d, sid, cell, r, text, limit)?.into_iter().map(|m| (m, rep.to_string())).collect()),
        }
    }
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
    format: Option<FindFormat>,
}

impl Matcher {
    fn from_params(p: &Value) -> Self {
        Self {
            find: str_param(p, "find").unwrap_or("").to_string(),
            grep: bool_or(p, "grep", false),
            case_sensitive: bool_or(p, "caseSensitive", false),
            whole_word: bool_or(p, "wholeWord", false),
            format: FindFormat::from_params(p).ok().flatten(),
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

    /// After Change put `new_len` bytes in place of `old` in story `sid`: the domain keeps the
    /// changed text and the search goes on after it.
    fn after_change(&mut self, st: &DocState, sid: StoryId, cell: Option<CellAddr>, old: &Range<usize>, new_len: usize) {
        let moved = |x: usize| if x >= old.end { x - old.len() + new_len } else { x };
        for (tid, tcell, r) in &mut self.targets {
            if *tid == sid && *tcell == cell {
                *r = moved(r.start)..moved(r.end);
            }
        }
        self.offset = old.start + new_len;
        self.last_match_end = Some(self.offset);
        let at = self.offset;
        if let Some(i) = self
            .order
            .iter()
            .position(|i| self.targets.get(*i).is_some_and(|(tid, tcell, r)| *tid == sid && *tcell == cell && r.start <= at && at <= r.end))
        {
            self.target = i;
        }
        self.document = Arc::downgrade(&st.doc);
        self.acknowledge(st);
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
    let re = search(p)?;
    re.check(&s.doc()?.doc.styles)?;
    let context = scope(s, p)?;
    let st = s.doc()?;
    let mut out = Vec::new();
    for (sid, cell, r) in &context.targets {
        let text = target_text(&st.doc, *sid, *cell, r)?;
        for m in re.find_all(&st.doc, *sid, *cell, r, text, MAX_MATCHES.saturating_sub(out.len()))? {
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
    let re = search(p)?;
    re.check(&s.doc()?.doc.styles)?;
    // With no pattern (a Find Format alone), the change is literal text.
    let grep = bool_or(p, "grep", false) && re.re.is_some();
    let rep = if re.re.is_some() { replacement(p) } else { metachars(str_param(p, "change").unwrap_or("")) };
    let first_only = bool_or(p, "first", false);
    let attrs = p.get("attrs").cloned();
    let over = char_overrides(attrs.as_ref())?;
    // A null attribute clears that override on the found text.
    let cleared: Vec<String> = attrs
        .as_ref()
        .and_then(Value::as_object)
        .map(|o| o.iter().filter(|(_, v)| v.is_null()).map(|(k, _)| k.clone()).collect())
        .unwrap_or_default();
    super::text::validate_char_fill(&s.doc()?.doc, &over, "find.change")?;
    let char_style = str_param(p, "charStyle").map(str::to_string);
    let para_style = str_param(p, "paraStyle").map(str::to_string);
    let styles = &s.doc()?.doc.styles;
    if let Some(n) = char_style.as_ref().filter(|n| styles.char_style(n).is_none()) {
        return Err(bad("find.change", format!("no character style `{n}`")));
    }
    if let Some(n) = para_style.as_ref().filter(|n| styles.para(n).is_none()) {
        return Err(bad("find.change", format!("no paragraph style `{n}`")));
    }
    // InDesign: with a Change Format and nothing in Change to, the found text is only formatted.
    let format_only =
        str_param(p, "change").unwrap_or("").is_empty() && (char_style.is_some() || para_style.is_some() || !over.is_empty() || !cleared.is_empty());
    let context = scope(s, p)?;
    let targets = context.targets.clone();
    // Change: only the text Find Next found, the selection, while it is still a match (as Find
    // Next defines one), with its replacement.
    let (found, found_out) = if bool_or(p, "found", false) {
        let st = s.doc()?;
        let selected = st.selection.text.filter(|t| st.selection.cells.is_none() && !t.is_caret()).map(|t| (t.story, t.cell, t.range()));
        let hit = selected.and_then(|(sid, cell, range)| {
            targets.iter().find_map(|(tid, tcell, r)| {
                if *tid != sid || *tcell != cell || range.start < r.start || r.end < range.end {
                    return None;
                }
                let text = target_text(&st.doc, *tid, cell, r).ok()?;
                let local = range.start - r.start..range.end - r.start;
                let (m, out) = re.found_edit(&st.doc, *tid, cell, r, text, &local, &rep, grep).ok()??;
                Some(((sid, cell, r.start + m.start..r.start + m.end), out))
            })
        });
        match hit {
            Some((hit, out)) => (Some(hit), Some(out)),
            None => return Ok(json!({"count": 0})),
        }
    } else {
        (None, None)
    };
    let (count, changed) = s.edit(|d, sel| {
        let styles = d.styles.clone();
        let mut count = 0usize;
        let mut changed = None;
        for (sid, cell, r) in targets {
            if found.as_ref().is_some_and(|(at, in_cell, _)| *at != sid || *in_cell != cell) {
                continue;
            }
            let Some(story) = d.text_story(sid, cell) else { continue };
            // Collect matches first (positions refer to the original text), apply back to front.
            let text = story.text.get(r.clone()).ok_or_else(|| bad("find", "invalid selection bounds or UTF-8 boundary"))?;
            let edits: Vec<(std::ops::Range<usize>, String)> = match (&found, &found_out) {
                // Change: the found text alone, in the target it lies in.
                (Some((_, _, range)), Some(out)) => {
                    if r.start <= range.start && range.end <= r.end {
                        vec![(range.clone(), out.clone())]
                    } else {
                        Vec::new()
                    }
                }
                _ => re
                    .replacements(d, sid, cell, &r, text, &rep, grep, first_only)?
                    .into_iter()
                    .map(|(m, out)| (r.start + m.start..r.start + m.end, out))
                    .collect(),
            };
            let Some(st) = d.text_story_mut(sid, cell) else { continue };
            for (range, new) in edits.iter().rev() {
                let changed = if format_only {
                    range.clone()
                } else {
                    st.replace(range.clone(), new);
                    range.start..range.start + new.len()
                };
                // A paragraph style goes to the match's paragraphs, an empty match's (`^`) too;
                // character formatting needs characters.
                if let Some(n) = &para_style {
                    super::style::apply_para_style(st, &styles, n, &changed, false);
                }
                if !changed.is_empty() {
                    if let Some(n) = &char_style {
                        super::style::apply_char_style(st, &styles, n, &changed);
                    }
                    if !over.is_empty() || !cleared.is_empty() {
                        st.format_chars(changed, |f| {
                            f.over.merge(&over);
                            for k in &cleared {
                                let _ = f.over.set_json(k, &Value::Null);
                            }
                        });
                    }
                }
                count += 1;
            }
            if let Some(t) = sel.text.as_mut().filter(|t| t.story == sid && t.cell == cell) {
                repair_selection(t, st);
            }
            // Change leaves the changed text selected.
            if let Some((_, _, range)) = &found
                && let Some((_, new)) = edits.first()
                && let Some(t) = sel.text.as_mut()
            {
                let len = if format_only { range.len() } else { new.len() };
                (t.anchor, t.focus) = (range.start, range.start + len);
                changed = Some(len);
            }
            // `first` is the first match of the whole search, not of each story.
            if first_only && count > 0 {
                break;
            }
        }
        Ok((count, changed))
    })?;
    // Change: Find Next goes on after the changed text, in the search's domain.
    if let (Some((sid, cell, old)), Some(len)) = (&found, changed) {
        let mut context = context;
        context.after_change(s.doc()?, *sid, *cell, old, len);
        s.find_context = Some(context);
    }
    Ok(json!({"count": count}))
}

fn find_next(s: &mut Session, p: &Value) -> Result<Value> {
    let re = search(p)?;
    re.check(&s.doc()?.doc.styles)?;
    let mut context = scope(s, p)?;
    let st = s.doc()?;
    let mut hit = None;
    while let Some((sid, cell, r)) = context.order.get(context.target).and_then(|i| context.targets.get(*i)).cloned() {
        let text = target_text(&st.doc, sid, cell, &r)?;
        let from = context.offset.checked_sub(r.start).ok_or_else(|| bad("find", "invalid search cursor"))?;
        if text.get(from..).is_none() {
            return Err(bad("find", "invalid UTF-8 search cursor"));
        }
        if let Some(m) = re.find_at(&st.doc, sid, cell, &r, text, from)? {
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
mod tests;

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

    /// A GREP match that is empty (`^`, the start of each paragraph) gets the paragraph style:
    /// only character formatting needs characters.
    #[test]
    fn change_format_styles_paragraphs_of_empty_matches() {
        let mut s = Session::new();
        s.execute("file.new", &json!({})).unwrap();
        let r = s
            .execute(
                "frame.create",
                &json!({"rect": [36, 36, 300, 300], "content": "text", "text": "One
Two
Three"}),
            )
            .unwrap();
        let sid = designcraft_doc::StoryId(r["story"].as_u64().unwrap());
        s.execute("style.paragraph.create", &json!({"name": "Method"})).unwrap();
        let r = s.execute("find.change", &json!({"find": "^", "change": "", "grep": true, "paraStyle": "Method"})).unwrap();
        assert_eq!(r["count"], 3);
        let story = s.doc().unwrap().doc.story(sid).unwrap().clone();
        assert!(story.paras.iter().all(|p| p.style == "Method"), "{:?}", story.paras.iter().map(|p| &p.style).collect::<Vec<_>>());
        assert_eq!(
            story.text,
            "One
Two
Three"
        );
    }

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
