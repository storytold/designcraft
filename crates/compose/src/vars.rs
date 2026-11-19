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
    series_catchwords: HashMap<(StoryId, u64, usize), String>,
    default_catchwords: HashMap<usize, (usize, StoryId, String)>,
}

/// One index per composition, keyed by stream then page. Main bytes never include routed notes.
pub(crate) struct CatchwordIndex {
    pub main: BTreeMap<usize, (usize, String)>,
    pub series: HashMap<u64, BTreeMap<usize, String>>,
}

impl CatchwordIndex {
    pub fn build(doc: &Document, cs: &ComposedStory, story: &Story) -> Self {
        let ids: std::collections::BTreeSet<_> = cs.frames.iter().flat_map(|f| &f.notes).map(|n| n.series).collect();
        Self { main: story_catchwords(doc, cs, story), series: ids.into_iter().map(|id| (id, series_catchwords(doc, cs, id))).collect() }
    }
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
pub(crate) fn frame_page(doc: &Document, frame_item: ItemId) -> Option<usize> {
    let loc = doc.find(frame_item)?;
    let SpreadRef::Doc(si) = loc.spread else { return None };
    let b = doc.parent_xf(&loc).transform_rect_bbox(doc.item(frame_item)?.bounds());
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

/// Reconstruct source text represented by visible glyphs, in source order. Hidden placeholders
/// keep byte ranges for editing but must not supply catchwords. Undrawn trimmed spaces and
/// actual line-break controls still delimit words; generated labels have no source length.
fn visible_source<'a>(source: &str, glyphs: impl Iterator<Item = &'a crate::PlacedGlyph>) -> String {
    let mut spans = BTreeMap::new();
    for g in glyphs.filter(|g| g.len > 0) {
        #[cfg(test)]
        VISIBLE_GLYPHS.with(|n| n.set(n.get() + 1));
        let shown = g.visible || g.adv > 0.0 || crate::breaker::is_forced(g.rendered_char) || g.rendered_char == '\t';
        spans
            .entry(g.byte)
            .and_modify(|v: &mut (usize, bool)| {
                v.0 = v.0.max(g.len);
                v.1 |= shown;
            })
            .or_insert((g.len, shown));
    }
    let mut text = String::new();
    let mut end = None;
    for (start, (len, shown)) in spans {
        if let Some(before) = end {
            if start < before {
                continue;
            }
            if source.get(before..start).is_some_and(|s| s.contains('\n')) {
                text.push('\n');
            }
        }
        let next = start.saturating_add(len);
        if shown && let Some(part) = source.get(start..next) {
            text.push_str(part);
        }
        end = Some(next);
    }
    text
}

#[cfg(test)]
thread_local! {
    static VISIBLE_BUILDS: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
    static VISIBLE_GLYPHS: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
}

/// Index a stream's visible words once per page, retaining suffixes of words split across pages.
fn page_words(pages: &BTreeMap<usize, String>, continuation: Option<&str>) -> BTreeMap<usize, String> {
    let mut visible = String::new();
    let mut offsets = BTreeMap::new();
    for (page, text) in pages {
        #[cfg(test)]
        VISIBLE_BUILDS.with(|n| n.set(n.get() + 1));
        offsets.insert(*page, visible.len());
        visible.extend(text.chars().filter(|c| !('\u{E000}'..='\u{E1FF}').contains(c) && *c != '\u{00AD}'));
    }
    let mut words = BTreeMap::new();
    let mut word_end = 0;
    let mut word_start = 0;
    let mut full_word = String::new();
    let mut candidates = visible.char_indices().filter(|(_, c)| !c.is_whitespace() && !['(', '[', '"', '\'', '“', '‘', '«'].contains(c)).peekable();
    for (page, offset) in offsets {
        while candidates.peek().is_some_and(|(at, _)| *at < offset) {
            candidates.next();
        }
        let word = if let Some(&(at, _)) = candidates.peek() {
            if at >= word_end {
                word_end = visible.get(at..).and_then(|s| s.find(char::is_whitespace)).map_or(visible.len(), |end| at + end);
                word_start = at;
                full_word = visible.get(at..word_end).and_then(first_word).map(|(_, w)| w).unwrap_or_default();
            }
            full_word.get(at.saturating_sub(word_start)..).unwrap_or_default().to_string()
        } else {
            String::new()
        };
        words.insert(page, word);
    }
    pages
        .keys()
        .map(|page| {
            let word =
                words.range(page.saturating_add(1)..).next().map(|(_, w)| w.clone()).unwrap_or_else(|| continuation.unwrap_or_default().to_string());
            (*page, word)
        })
        .collect()
}

/// First visible word on the next page showing this series. Source ranges, rather than
/// generated labels, distinguish a continued note from the next note in the same stream.
pub(crate) fn series_catchwords(doc: &Document, cs: &ComposedStory, series: u64) -> BTreeMap<usize, String> {
    let mut pages: BTreeMap<usize, Vec<_>> = BTreeMap::new();
    for frame in &cs.frames {
        let Some(page) = frame_page(doc, frame.frame) else { continue };
        for note in frame.notes.iter().filter(|n| n.series == series) {
            for part in &note.text.frames {
                let glyphs = || part.lines.iter().flat_map(|l| &l.glyphs);
                let Some(start) = glyphs().filter(|g| g.visible && g.len > 0).map(|g| g.byte).min() else { continue };
                pages.entry(page).or_default().push((note.index, start, visible_source(&note.source, glyphs())));
            }
        }
    }
    for parts in pages.values_mut() {
        parts.sort_by_key(|(index, start, _)| (*index, *start));
    }
    let text = pages.into_iter().map(|(p, parts)| (p, parts.into_iter().map(|(_, _, t)| t).collect::<Vec<_>>().join("\n") + "\n")).collect();
    page_words(&text, None)
}

/// Catchwords for the main source, with its shown-byte count for default-source selection.
pub(crate) fn story_catchwords(doc: &Document, cs: &ComposedStory, st: &Story) -> BTreeMap<usize, (usize, String)> {
    // Routed note frames are appended to the composition but never carry main-story bytes.
    let main =
        |ft: &&crate::FrameText| doc.item(ft.frame).and_then(|i| i.text_frame()).is_some_and(|f| f.story == st.id && f.options.note_stream.is_none());
    let mut pages: BTreeMap<usize, (usize, usize)> = BTreeMap::new();
    for ft in cs.frames.iter().filter(main) {
        if let Some(page) = frame_page(doc, ft.frame) {
            let shown = pages.entry(page).or_default();
            shown.0 = shown.0.saturating_add(ft.range.len());
            shown.1 = shown.1.max(ft.range.end);
        }
    }
    let mut glyph_pages: BTreeMap<usize, Vec<_>> = BTreeMap::new();
    for ft in cs.frames.iter().filter(main) {
        if let Some(page) = frame_page(doc, ft.frame) {
            glyph_pages.entry(page).or_default().extend(ft.lines.iter().flat_map(|l| &l.glyphs).filter(|g| cs.overset_at.is_none_or(|o| g.byte < o)));
        }
    }
    let mut text = BTreeMap::new();
    let mut previous_end = None;
    for (page, glyphs) in glyph_pages {
        let start = glyphs.iter().filter(|g| g.len > 0).map(|g| g.byte).min();
        let mut value = String::new();
        if previous_end.zip(start).is_some_and(|(a, b)| st.text.get(a..b).is_some_and(|s| s.contains('\n'))) {
            value.push('\n');
        }
        value.push_str(&visible_source(&st.text, glyphs.iter().copied()));
        previous_end = glyphs.iter().filter(|g| g.len > 0).map(|g| g.byte.saturating_add(g.len)).max().or(previous_end);
        text.insert(page, value);
    }
    page_words(&text, st.catchword_continuation.as_deref()).into_iter().map(|(p, w)| (p, (pages.get(&p).map_or(0, |s| s.0), w))).collect()
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
    /// Index running-header paragraphs / runs and catchwords from body layouts without
    /// resolving document-wide indexes, including indirect margin dependencies. Stories
    /// containing either indexed variable kind are skipped to avoid cycles.
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
                VarKind::NextPageFirstWord { story, series } => Some((story, series)),
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
            let catchword_hit = catchword_stories.iter().any(|(story, _)| story.is_none_or(|id| id == *sid));
            if !para_hit && !char_hit && !catchword_hit {
                continue;
            }
            let cs = get(*sid);
            let catchwords = catchword_hit.then(|| CatchwordIndex::build(doc, &cs, st));
            if let Some(words) = &catchwords {
                for (series, pages) in &words.series {
                    index.series_catchwords.extend(pages.iter().map(|(page, word)| ((*sid, *series, *page), word.clone())));
                }
            }
            if catchword_stories.iter().any(|(story, series)| series.is_none() && story.is_none_or(|id| id == *sid)) {
                for (page, (shown, word)) in catchwords.into_iter().flat_map(|w| w.main) {
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
            VarKind::NextPageFirstWord { story, series } => {
                let word = match (running, page) {
                    (Some(r), Some(p)) if series.is_some() => {
                        story.zip(*series).and_then(|(sid, series)| r.series_catchwords.get(&(sid, series, p))).cloned().unwrap_or_default()
                    }
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
    fn catchword_visible_text_is_reconstructed_once_per_page() {
        use super::*;
        use designcraft_doc::{
            build::NewDocument,
            vars::{TextVariable, VarKind},
        };
        let mut doc = Document::new(&NewDocument { pages: 24, ..Default::default() });
        let text = (0..24)
            .map(|p| format!("{} tail{}\n", if p == 0 { ".....".into() } else { format!("Word{p}") }, designcraft_doc::story::FRAME_BREAK))
            .collect::<String>();
        let mut previous = None;
        let mut story = None;
        for page in 0..24 {
            let (si, pi) = doc.page_loc(page).unwrap();
            let x = doc.spreads[si].pages[pi].x;
            let (id, sid) = doc
                .add_text_frame(
                    SpreadRef::Doc(si),
                    designcraft_geom::Rect::new(x + 72.0, 72.0, x + 500.0, 172.0),
                    doc.default_layer(),
                    if page == 0 { &text } else { "" },
                    Default::default(),
                )
                .unwrap();
            if let Some(from) = previous {
                doc.thread(from, id).unwrap();
            }
            previous = Some(id);
            story.get_or_insert(sid);
        }
        let sid = story.unwrap();
        doc.text_variables.push(TextVariable::new("Catchword", VarKind::NextPageFirstWord { story: Some(sid), series: None }));
        let cs = Arc::new(crate::compose_story(&doc, sid, &Default::default()));
        VISIBLE_BUILDS.with(|n| n.set(0));
        VISIBLE_GLYPHS.with(|n| n.set(0));
        let index = RunningIndex::build(&doc, &|_| cs.clone());
        let glyphs = cs.frames.iter().flat_map(|f| &f.lines).flat_map(|l| &l.glyphs).filter(|g| g.len > 0).count();
        assert_eq!(VISIBLE_GLYPHS.with(|n| n.get()), glyphs, "each source glyph is reconstructed only once");
        assert_eq!(VISIBLE_BUILDS.with(|n| n.get()), 24);
        for page in 0..23 {
            assert_eq!(index.catchword(Some(sid), page), format!("Word{}", page + 1));
        }
        assert_eq!(index.catchword(Some(sid), 23), "");
        doc.story_mut(sid).unwrap().catchword_continuation = Some("Next excerpt".into());
        let index = RunningIndex::build(&doc, &|_| cs.clone());
        assert_eq!(index.catchword(Some(sid), 0), "Word1");
        assert_eq!(index.catchword(Some(sid), 23), "Next excerpt");
    }

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
