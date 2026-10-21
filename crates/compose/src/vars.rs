//! Text variable resolution at composition time, including running headers (which depend on
//! where paragraphs of a style land in the composed document).

use std::collections::{BTreeMap, HashMap};
use std::sync::Arc;

use designcraft_doc::vars::{Use, VarKind, var_index};
use designcraft_doc::{Document, SpreadRef, Story, StoryId};

use crate::ComposedStory;

/// Does the story contain any text variable instance?
pub fn has_vars(st: &Story) -> bool {
    st.text.chars().any(|c| var_index(c).is_some())
}

/// Does the story contain a running-header variable instance?
pub fn has_running(doc: &Document, st: &Story) -> bool {
    st.text.chars().filter_map(var_index).any(|i| matches!(doc.text_variables.get(i).map(|v| &v.kind), Some(VarKind::RunningHeader { .. })))
}

/// Absolute page showing story byte `pos` of a composed story.
pub fn page_of(doc: &Document, cs: &ComposedStory, pos: usize) -> Option<usize> {
    let (fi, ..) = crate::caret(cs, pos)?;
    // Overset text has no page.
    let ft = cs.frames.get(fi)?;
    if pos > ft.range.end && cs.overset_at.is_some_and(|o| pos >= o) {
        return None;
    }
    place(doc, cs, pos).map(|p| p.0)
}

/// Per (style, is character style): page → (first text, last text) on that page.
#[derive(Debug, Default)]
pub struct RunningIndex {
    map: HashMap<(String, bool), BTreeMap<usize, (String, String)>>,
}

fn clean(s: &str) -> String {
    let t: String =
        s.chars().filter(|c| !('\u{E000}'..='\u{E1FF}').contains(c)).map(|c| if c == '\t' || c == '\u{2028}' { ' ' } else { c }).collect();
    t.trim().to_string()
}

/// (absolute page, y, x) of story byte `pos` in a composed story.
pub(crate) fn place(doc: &Document, cs: &ComposedStory, pos: usize) -> Option<(usize, i64, i64)> {
    let (fi, x, baseline, ..) = crate::caret(cs, pos)?;
    let ft = cs.frames.get(fi)?;
    let loc = doc.find(ft.frame)?;
    let SpreadRef::Doc(si) = loc.spread else { return None };
    let b = doc.item(ft.frame)?.bounds();
    let pi = doc.spreads.get(si)?.page_at_x(b.center().x).unwrap_or(0);
    Some((doc.first_page_of_spread(si) + pi, ((b.y0 + baseline) * 10.0) as i64, ((b.x0 + x) * 10.0) as i64))
}

/// A paragraph or run a running header may show: where it starts ([`place`]), its story and byte
/// there, and its text.
struct Hit {
    at: (usize, i64, i64),
    story: StoryId,
    pos: usize,
    text: String,
}

/// (page, text) in the order running headers read a page: text order within a story (a story's
/// second column comes after its first, however high its entries sit), and stories by where their
/// first entry on the page is (top to bottom, then left to right).
fn in_page_order(hits: Vec<Hit>) -> Vec<(usize, String)> {
    // Each story on a page is placed by its first hit there in text order (not its highest hit,
    // which in a later column could put the whole story ahead of another).
    let mut first: HashMap<(usize, StoryId), (usize, (i64, i64))> = HashMap::new();
    for h in &hits {
        let (page, y, x) = h.at;
        first.entry((page, h.story)).and_modify(|f| *f = (*f).min((h.pos, (y, x)))).or_insert((h.pos, (y, x)));
    }
    let mut hits: Vec<_> =
        hits.into_iter().map(|h| (h.at.0, first.get(&(h.at.0, h.story)).map_or((h.at.1, h.at.2), |f| f.1), h.story, h.pos, h.text)).collect();
    hits.sort_by_key(|a| (a.0, a.1, a.2.0, a.3));
    hits.into_iter().map(|(page, _, _, _, text)| (page, text)).collect()
}

impl RunningIndex {
    /// Index the paragraphs / runs that running-header variables refer to. `get` composes a story
    /// (normally through the cache); stories that themselves contain running headers are skipped.
    pub fn build(doc: &Document, get: &dyn Fn(StoryId) -> Arc<ComposedStory>) -> RunningIndex {
        let wanted: Vec<(String, bool)> = doc
            .text_variables
            .iter()
            .filter_map(|v| match &v.kind {
                VarKind::RunningHeader { style, character, .. } => Some((style.clone(), *character)),
                _ => None,
            })
            .collect();
        // (place, story, byte) of each paragraph or run, with its text.
        let mut hits: HashMap<(String, bool), Vec<Hit>> = HashMap::new();
        if wanted.is_empty() {
            return RunningIndex::default();
        }
        for (sid, st) in &doc.stories {
            if has_running(doc, st) {
                continue;
            }
            let para_hit = st.paras.iter().any(|f| wanted.iter().any(|(s, c)| !c && *s == f.style));
            let char_hit = wanted.iter().any(|(_, c)| *c) && st.runs().any(|(_, f)| wanted.iter().any(|(s, c)| *c && *s == f.style));
            if !para_hit && !char_hit {
                continue;
            }
            let cs = get(*sid);
            if para_hit {
                for (pi, r) in st.para_ranges().into_iter().enumerate() {
                    let key = (st.paras[pi].style.clone(), false);
                    if !wanted.contains(&key) {
                        continue;
                    }
                    let text = clean(&st.text[r.clone()]);
                    if text.is_empty() {
                        continue;
                    }
                    if let Some(at) = place(doc, &cs, r.start) {
                        hits.entry(key).or_default().push(Hit { at, story: *sid, pos: r.start, text });
                    }
                }
            }
            if char_hit {
                for (r, f) in st.runs() {
                    let key = (f.style.clone(), true);
                    if !wanted.contains(&key) {
                        continue;
                    }
                    let text = clean(&st.text[r.clone()]);
                    if text.is_empty() {
                        continue;
                    }
                    if let Some(at) = place(doc, &cs, r.start) {
                        hits.entry(key).or_default().push(Hit { at, story: *sid, pos: r.start, text });
                    }
                }
            }
        }
        let mut map = HashMap::new();
        for (key, v) in hits {
            let mut pages: BTreeMap<usize, (String, String)> = BTreeMap::new();
            for (page, text) in in_page_order(v) {
                pages.entry(page).and_modify(|e| e.1 = text.clone()).or_insert((text.clone(), text));
            }
            map.insert(key, pages);
        }
        RunningIndex { map }
    }

    /// The running header for `page`: the first/last on the page, else the last before it.
    pub fn value(&self, style: &str, character: bool, use_: Use, page: usize) -> String {
        let Some(pages) = self.map.get(&(style.to_string(), character)) else { return String::new() };
        if let Some((first, last)) = pages.get(&page) {
            return if use_ == Use::FirstOnPage { first.clone() } else { last.clone() };
        }
        pages.range(..page).next_back().map(|(_, (_, last))| last.clone()).unwrap_or_default()
    }
}

/// Values of every variable on `page`.
pub fn values(doc: &Document, page: Option<usize>, running: Option<&RunningIndex>) -> Vec<String> {
    doc.text_variables
        .iter()
        .enumerate()
        .map(|(i, v)| match &v.kind {
            VarKind::RunningHeader { style, use_, character } => {
                let body = match (running, page) {
                    (Some(r), Some(p)) => r.value(style, *character, *use_, p),
                    _ => String::new(),
                };
                format!("{}{}{}", v.before, body, v.after)
            }
            _ => doc.variable_value(i, page).unwrap_or_default(),
        })
        .collect()
}
