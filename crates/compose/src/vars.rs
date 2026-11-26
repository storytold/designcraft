//! Text variable resolution at composition time, including running headers (which depend on
//! where paragraphs of a style land in the composed document).

use std::collections::{BTreeMap, HashMap};
use std::sync::Arc;

use designcraft_doc::vars::{Use, VarKind, var_index};
use designcraft_doc::{Document, ItemId, SpreadRef, Story, StoryId};

use crate::ComposedStory;

/// Does the story contain any text variable instance?
pub fn has_vars(st: &Story) -> bool {
    st.text.chars().any(|c| var_index(c).is_some())
}

/// Does the story contain a running-header or catchword instance that needs the composed document?
pub fn has_running(doc: &Document, st: &Story) -> bool {
    st.text
        .chars()
        .filter_map(var_index)
        .any(|i| matches!(doc.text_variables.get(i).map(|v| &v.kind), Some(VarKind::RunningHeader { .. } | VarKind::NextPageFirstWord { .. })))
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

/// Running headers per style and page, and catchwords per story and page.
#[derive(Debug, Default)]
pub struct RunningIndex {
    map: HashMap<(String, bool), BTreeMap<usize, (String, String)>>,
    catchwords: HashMap<(StoryId, usize), String>,
    default_catchwords: HashMap<usize, (usize, StoryId, String)>,
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
    let page = frame_page(doc, ft.frame)?;
    let b = doc.item(ft.frame)?.bounds();
    Some((page, ((b.y0 + baseline) * 10.0) as i64, ((b.x0 + x) * 10.0) as i64))
}

/// Absolute document page of a frame; parent-page frames have no document page.
fn frame_page(doc: &Document, frame_item: ItemId) -> Option<usize> {
    let loc = doc.find(frame_item)?;
    let SpreadRef::Doc(si) = loc.spread else { return None };
    let b = doc.item(frame_item)?.bounds();
    let pi = doc.spreads.get(si)?.page_at_x(b.center().x).unwrap_or(0);
    doc.first_page_of_spread(si).checked_add(pi)
}

/// The first visible word and its byte offset, preserving marks and internal Hebrew punctuation.
fn first_word(text: &str) -> Option<(usize, String)> {
    // ponytail: a word hyphenated across the page break shows its tail.
    let mut chars = text
        .char_indices()
        .filter(|(_, c)| !('\u{E000}'..='\u{E1FF}').contains(c) && *c != '\u{00AD}')
        .skip_while(|(_, c)| c.is_whitespace() || ['(', '[', '"', '\'', '“', '‘', '«'].contains(c))
        .peekable();
    let start = chars.peek()?.0;
    let word: String = chars.take_while(|(_, c)| !c.is_whitespace()).map(|(_, c)| c).collect();
    let word = word.trim_end_matches([',', '.', ';', ':', '!', '?', ')', ']', '"', '\'', '”', '’', '»', '׃', '׀']);
    (!word.is_empty()).then(|| (start, word.to_string()))
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
    /// Index running-header paragraphs / runs and catchwords. `get` composes a story (normally
    /// through the cache); stories containing either indexed variable kind are skipped to avoid cycles.
    pub fn build(doc: &Document, get: &dyn Fn(StoryId) -> Arc<ComposedStory>) -> RunningIndex {
        let wanted: Vec<(String, bool)> = doc
            .text_variables
            .iter()
            .filter_map(|v| match &v.kind {
                VarKind::RunningHeader { style, character, .. } => Some((style.clone(), *character)),
                _ => None,
            })
            .collect();
        let catchword_stories: Vec<_> = doc
            .text_variables
            .iter()
            .filter_map(|v| match v.kind {
                VarKind::NextPageFirstWord { story } => Some(story),
                _ => None,
            })
            .collect();
        let mut index = RunningIndex::default();
        // (place, story, byte) of each paragraph or run, with its text.
        let mut hits: HashMap<(String, bool), Vec<Hit>> = HashMap::new();
        if wanted.is_empty() && catchword_stories.is_empty() {
            return RunningIndex::default();
        }
        for (sid, st) in &doc.stories {
            if has_running(doc, st) {
                continue;
            }
            let para_hit = st.paras.iter().any(|f| wanted.iter().any(|(s, c)| !c && *s == f.style));
            let char_hit = wanted.iter().any(|(_, c)| *c) && st.runs().any(|(_, f)| wanted.iter().any(|(s, c)| *c && *s == f.style));
            let catchword_hit = catchword_stories.iter().any(|story| story.is_none_or(|id| id == *sid));
            if !para_hit && !char_hit && !catchword_hit {
                continue;
            }
            let cs = get(*sid);
            if catchword_hit {
                let mut pages: BTreeMap<usize, (usize, usize)> = BTreeMap::new();
                for ft in &cs.frames {
                    if let Some(page) = frame_page(doc, ft.frame) {
                        let shown = pages.entry(page).or_default();
                        shown.0 = shown.0.saturating_add(ft.range.len());
                        shown.1 = shown.1.max(ft.range.end);
                    }
                }
                for (page, (shown, end)) in pages {
                    let word = if end >= st.text.len() || cs.overset_at.is_some_and(|o| end >= o) {
                        String::new()
                    } else {
                        st.text
                            .get(end..)
                            .and_then(first_word)
                            .filter(|(offset, _)| {
                                let Some(start) = end.checked_add(*offset) else { return false };
                                cs.overset_at.is_none_or(|o| start < o)
                                    && cs.frames.iter().any(|ft| ft.range.contains(&start) && frame_page(doc, ft.frame).is_some_and(|p| p != page))
                            })
                            .map(|(_, word)| word)
                            .unwrap_or_default()
                    };
                    index.catchwords.insert((*sid, page), word.clone());
                    let default = index.default_catchwords.entry(page).or_insert((shown, *sid, word.clone()));
                    if shown > default.0 || (shown == default.0 && *sid < default.1) {
                        *default = (shown, *sid, word);
                    }
                }
            }
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
        index.map = map;
        index
    }

    /// The catchword for this story and page, or the dominant story when none is specified.
    pub fn catchword(&self, story: Option<StoryId>, page: usize) -> String {
        match story {
            Some(sid) => self.catchwords.get(&(sid, page)).cloned().unwrap_or_default(),
            None => self.default_catchwords.get(&page).map(|(_, _, word)| word.clone()).unwrap_or_default(),
        }
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
    values_shown(doc, page, false, running)
}

/// [`values`] as text shows them: on a parent page itself (`parent`, with no `page`), running
/// headers and catchwords show which variable sits there, `<Name>`. Elsewhere without a page,
/// running headers show their text before and after; catchwords show nothing.
pub fn values_shown(doc: &Document, page: Option<usize>, parent: bool, running: Option<&RunningIndex>) -> Vec<String> {
    doc.text_variables
        .iter()
        .enumerate()
        .map(|(i, v)| match &v.kind {
            VarKind::RunningHeader { .. } | VarKind::NextPageFirstWord { .. } if parent && page.is_none() => format!("<{}>", v.name),
            VarKind::RunningHeader { style, use_, character } => {
                let body = match (running, page) {
                    (Some(r), Some(p)) => r.value(style, *character, *use_, p),
                    _ => String::new(),
                };
                format!("{}{}{}", v.before, body, v.after)
            }
            VarKind::NextPageFirstWord { story } => {
                let word = match (running, page) {
                    (Some(r), Some(p)) => r.catchword(*story, p),
                    _ => String::new(),
                };
                if word.is_empty() { word } else { format!("{}{}{}", v.before, word, v.after) }
            }
            _ => doc.variable_value(i, page).unwrap_or_default(),
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::first_word;

    #[test]
    fn first_word_preserves_hebrew_marks_and_internal_punctuation() {
        for (text, word) in [
            ("בְּקְרוֹב׃ לְמַעַן", "בְּקְרוֹב"),
            ("בְּקְרוֹב, לְמַעַן", "בְּקְרוֹב"),
            ("כָּל־עַמְּךָ שָׁלוֹם", "כָּל־עַמְּךָ"),
            ("רש״י ה׳", "רש״י"),
            ("ה׳ אָמַר", "ה׳"),
            (" \t\u{E100}\u{E000}\u{AD}  בְּקְרוֹב", "בְּקְרוֹב"),
            ("«(בְּקְ\u{AD}רוֹב)»", "בְּקְרוֹב"),
            ("בְּקְרוֹב׀", "בְּקְרוֹב"),
            ("", ""),
        ] {
            assert_eq!(first_word(text).map(|(_, word)| word).unwrap_or_default(), word, "{text:?}");
        }
        let word = format!("{}בְּ", "א".repeat(63));
        assert_eq!(first_word(&format!("{word} next")), Some((0, word)));
        let prefix = " \t\u{E100}\u{AD}«[";
        assert_eq!(first_word(&format!("{prefix}בְּקְרוֹב,")), Some((prefix.len(), "בְּקְרוֹב".into())));
        assert_eq!(first_word(""), None);
        assert_eq!(first_word(" \u{E100}\u{AD}("), None);
    }
}
