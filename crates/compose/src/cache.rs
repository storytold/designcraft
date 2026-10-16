//! Composition cache shared by the engine, the renderer and exporters.
//!
//! An entry is valid while the inputs it was composed from are the *same allocations*: the story
//! `Arc` (and its revision), the styles `Arc`, the frame items' `Arc`s and the `Arc`s of every
//! wrapping item on the frames' spreads. Entries hold clones of those `Arc`s, so the pointers can't
//! be reused by other allocations while the entry lives.

use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use designcraft_doc::{Document, Item, Story, StoryId, Styles, WrapMode};

use crate::vars::RunningIndex;
use crate::{ComposeOptions, ComposedStory};

/// A story, composed for one absolute page (parent-page items) or for its own frames (`None`).
type Key = (StoryId, Option<usize>);

struct Entry {
    sig: Vec<usize>,
    _keep: (Arc<Story>, Arc<Styles>, Vec<Arc<Item>>),
    out: Arc<ComposedStory>,
    stamp: u64,
}

#[derive(Default)]
pub struct Cache {
    map: Mutex<(HashMap<Key, Entry>, u64)>,
    /// Running-header index with the document signature it was built for.
    running: Mutex<Option<(u64, Arc<RunningIndex>)>>,
    /// Cross-reference anchor pages with the document signature they were built for.
    xrefs: Mutex<Option<(u64, Arc<crate::xref::XrefIndex>)>>,
    /// The composition each story (page-independent key) had before its latest recomposition,
    /// with the story revision it was composed from (repaint only the frames an edit changed).
    previous: Mutex<HashMap<StoryId, (Vec<usize>, Arc<ComposedStory>)>>,
}

/// Identity of everything a running header can depend on (stories, spreads, styles, variables).
fn doc_signature(doc: &Document) -> u64 {
    use std::hash::{Hash, Hasher};
    let mut h = std::collections::hash_map::DefaultHasher::new();
    for (id, st) in &doc.stories {
        (id.0, Arc::as_ptr(st) as usize, st.rev).hash(&mut h);
    }
    for sp in &doc.spreads {
        (Arc::as_ptr(sp) as usize).hash(&mut h);
    }
    (Arc::as_ptr(&doc.styles) as usize).hash(&mut h);
    format!("{:?}", doc.text_variables).hash(&mut h);
    sections_hash(doc).hash(&mut h);
    h.finish()
}

/// Everything page names and section markers are made from.
fn sections_hash(doc: &Document) -> u64 {
    use std::hash::{Hash, Hasher};
    let mut h = std::collections::hash_map::DefaultHasher::new();
    doc.sections.len().hash(&mut h);
    for s in &doc.sections {
        (s.start, s.start_number, s.style as u8, &s.prefix, &s.marker, s.include_prefix).hash(&mut h);
    }
    h.finish()
}

impl Cache {
    pub fn new() -> Self {
        Self::default()
    }

    /// The composed story, from cache or freshly composed. `page` is the absolute page a
    /// parent-page item is shown on (its page number markers and variables are that page's), or
    /// `None` for a story's own frames. Pages are identified by index, not name: names repeat when
    /// sections restart their numbering.
    pub fn get(&self, doc: &Document, sid: StoryId, page: Option<usize>) -> Arc<ComposedStory> {
        let Some(story) = doc.stories.get(&sid) else { return Arc::new(ComposedStory { story: sid, ..Default::default() }) };
        let (mut sig, keep_items) = signature(doc, story);
        let key = (sid, page);
        // The key holds the page's index, which a renumbering doesn't change: its name (section
        // start number, style, prefix) is part of the signature.
        let page_name = page.map(|p| doc.page_name(p));
        let section_marker = page.map(|p| crate::section_marker(doc, p));
        if let Some(name) = &page_name {
            use std::hash::{Hash, Hasher};
            let mut h = std::collections::hash_map::DefaultHasher::new();
            (name, &section_marker).hash(&mut h);
            sig.push(h.finish() as usize);
        }
        if story.text.contains(designcraft_doc::story::SECTION_MARKER) {
            // Frames on document pages show their section's marker text.
            use std::hash::{Hash, Hasher};
            let mut h = std::collections::hash_map::DefaultHasher::new();
            doc.sections.iter().for_each(|s| (s.start, &s.marker).hash(&mut h));
            sig.push(h.finish() as usize);
        }
        let with_vars = crate::vars::has_vars(story);
        let running = with_vars && crate::vars::has_running(doc, story);
        if with_vars {
            // Variables depend on the page count, sections and definitions (and running headers on
            // the whole document).
            use std::hash::{Hash, Hasher};
            let mut h = std::collections::hash_map::DefaultHasher::new();
            format!("{:?}", doc.text_variables).hash(&mut h);
            sig.extend([doc.page_count(), h.finish() as usize]);
            if running {
                sig.push(doc_signature(doc) as usize);
            }
        }
        let with_xrefs = crate::xref::has_xrefs(story);
        if with_xrefs {
            // Cross-references show other stories' text and pages.
            use std::hash::{Hash, Hasher};
            let mut h = std::collections::hash_map::DefaultHasher::new();
            format!("{:?}", doc.xref_formats).hash(&mut h);
            sig.extend([doc_signature(doc) as usize, h.finish() as usize]);
        }
        if !story.notes.is_empty() {
            // Footnote numbers continue from earlier stories; layout follows the options.
            use std::hash::{Hash, Hasher};
            let mut h = std::collections::hash_map::DefaultHasher::new();
            format!("{:?}", doc.footnote_options).hash(&mut h);
            sig.extend([doc.footnote_start(sid) as usize, h.finish() as usize]);
        }
        // Endnote numbers continue from stories on earlier pages.
        if !story.endnotes.is_empty() {
            sig.push(doc.endnote_start(sid) as usize);
        }
        // User hyphenation exceptions.
        if !doc.hyphenation_exceptions.is_empty() {
            use std::hash::{Hash, Hasher};
            let mut h = std::collections::hash_map::DefaultHasher::new();
            doc.hyphenation_exceptions.hash(&mut h);
            sig.push(h.finish() as usize);
        }
        // Hidden conditions take their text out of the layout.
        for (i, c) in doc.conditions.iter().enumerate() {
            if !c.visible {
                sig.push(usize::MAX - i);
            }
        }
        // Advanced Type sizes super/subscripts.
        let a = doc.settings.advanced_type;
        sig.extend([a.superscript_size, a.superscript_position, a.subscript_size, a.subscript_position].map(|v| v.to_bits() as usize));
        // Missing glyphs: fallback fonts or the font's box.
        sig.push(usize::from(doc.settings.glyph_fallback));
        // Named lists continue from earlier stories.
        for l in &doc.settings.lists {
            sig.push(doc.list_start(sid, &l.name) as usize);
        }
        {
            let mut g = self.map.lock().unwrap_or_else(|e| e.into_inner());
            g.1 += 1;
            let stamp = g.1;
            if let Some(e) = g.0.get_mut(&key)
                && e.sig == sig
            {
                e.stamp = stamp;
                return e.out.clone();
            }
        }
        // Parent-page items are composed per page: markers show its name, variables need its index.
        let page = page.filter(|_| with_vars);
        let running = running.then(|| self.running_index(doc));
        let out = Arc::new(crate::compose_story(
            doc,
            sid,
            &ComposeOptions { page_name, section_marker, page, running, label: None, xrefs: with_xrefs.then(|| self.xref_index(doc)) },
        ));
        let mut g = self.map.lock().unwrap_or_else(|e| e.into_inner());
        let stamp = g.1;
        if g.0.len() > 4096 {
            g.0.retain(|_, e| stamp - e.stamp < 64);
        }
        let replaced = g.0.insert(key, Entry { sig, _keep: (story.clone(), doc.styles.clone(), keep_items), out: out.clone(), stamp });
        drop(g);
        if key.1.is_none()
            && let Some(old) = replaced
        {
            self.previous.lock().unwrap_or_else(|e| e.into_inner()).insert(sid, (old.sig, old.out));
        }
        out
    }

    /// The cached composition of `sid` made from exactly this version of `doc` (the current
    /// entry or the one it replaced), without composing anything.
    pub fn composed_for(&self, doc: &Document, sid: StoryId) -> Option<Arc<ComposedStory>> {
        let story = doc.stories.get(&sid)?;
        let (sig, _) = signature(doc, story);
        // Only the geometry/story part of the signature (variables etc. add to it).
        let matches = |s: &[usize]| s.len() >= sig.len() && s[..sig.len()] == sig[..];
        {
            let g = self.map.lock().unwrap_or_else(|e| e.into_inner());
            if let Some(e) = g.0.get(&(sid, None))
                && matches(&e.sig)
            {
                return Some(e.out.clone());
            }
        }
        let g = self.previous.lock().unwrap_or_else(|e| e.into_inner());
        g.get(&sid).filter(|(s, _)| matches(s)).map(|(_, c)| c.clone())
    }

    /// Frames whose composed content differs between two compositions of a story.
    pub fn changed_frames(a: &ComposedStory, b: &ComposedStory) -> Vec<designcraft_doc::ItemId> {
        let same_line = |x: &crate::Line, y: &crate::Line| {
            x.range == y.range
                && (x.baseline - y.baseline).abs() < 1e-9
                && (x.x0 - y.x0).abs() < 1e-9
                && (x.end_x - y.end_x).abs() < 1e-9
                && x.glyphs.len() == y.glyphs.len()
                && x.glyphs
                    .iter()
                    .zip(&y.glyphs)
                    .all(|(g, h)| g.gid == h.gid && g.style == h.style && (g.x - h.x).abs() < 1e-9 && (g.y - h.y).abs() < 1e-9)
        };
        let mut out = Vec::new();
        for fb in &b.frames {
            let same = a.frame(fb.frame).is_some_and(|fa| {
                fa.lines.len() == fb.lines.len()
                    && fa.lines.iter().zip(&fb.lines).all(|(x, y)| same_line(x, y))
                    && fa.decos == fb.decos
                    && fa.tables.is_empty()
                    && fb.tables.is_empty()
                    && fa.notes.is_empty()
                    && fb.notes.is_empty()
                    && fa.objects.is_empty()
                    && fb.objects.is_empty()
            }) && a.styles == b.styles;
            if !same {
                out.push(fb.frame);
            }
        }
        for fa in &a.frames {
            if b.frame(fa.frame).is_none() {
                out.push(fa.frame);
            }
        }
        out
    }

    pub fn clear(&self) {
        self.map.lock().unwrap_or_else(|e| e.into_inner()).0.clear();
        *self.running.lock().unwrap_or_else(|e| e.into_inner()) = None;
        *self.xrefs.lock().unwrap_or_else(|e| e.into_inner()) = None;
    }

    /// Anchor pages for cross-references (rebuilt when the document changes).
    pub fn xref_index(&self, doc: &Document) -> Arc<crate::xref::XrefIndex> {
        let sig = doc_signature(doc);
        if let Some((s, r)) = &*self.xrefs.lock().unwrap_or_else(|e| e.into_inner())
            && *s == sig
        {
            return r.clone();
        }
        let r = Arc::new(crate::xref::XrefIndex::build(doc, &|sid| self.get(doc, sid, None)));
        *self.xrefs.lock().unwrap_or_else(|e| e.into_inner()) = Some((sig, r.clone()));
        r
    }

    /// The running-header index for `doc` (rebuilt when the document changes).
    pub fn running_index(&self, doc: &Document) -> Arc<RunningIndex> {
        let sig = doc_signature(doc);
        if let Some((s, r)) = &*self.running.lock().unwrap_or_else(|e| e.into_inner())
            && *s == sig
        {
            return r.clone();
        }
        let r = Arc::new(RunningIndex::build(doc, &|sid| self.get(doc, sid, None)));
        *self.running.lock().unwrap_or_else(|e| e.into_inner()) = Some((sig, r.clone()));
        r
    }
}

fn signature(doc: &Document, story: &Arc<Story>) -> (Vec<usize>, Vec<Arc<Item>>) {
    // The document's font scope: another document's fonts of the same name set it differently.
    // Sections name the pages under the frames (page-number and section markers).
    let g = &doc.settings.baseline_grid;
    let mut sig = vec![
        Arc::as_ptr(story) as usize,
        story.rev as usize,
        Arc::as_ptr(&doc.styles) as usize,
        sections_hash(doc) as usize,
        doc.font_scope as usize,
        g.start.to_bits() as usize,
        g.increment.to_bits() as usize,
    ];
    let mut keep = Vec::new();
    let mut spreads = Vec::new();
    for f in &story.frames {
        let Some(loc) = doc.find(*f) else {
            sig.push(0);
            continue;
        };
        if let Some(sp) = doc.spread(loc.spread) {
            let top = &sp.items[loc.top()];
            sig.push(Arc::as_ptr(top) as usize);
            keep.push(top.clone());
            // Page position matters for page-number markers.
            sig.push(loc.top());
            if !spreads.contains(&loc.spread) {
                spreads.push(loc.spread);
                for it in &sp.items {
                    if it.wrap.mode != WrapMode::None {
                        sig.push(Arc::as_ptr(it) as usize);
                        // Wrap objects on hidden layers don't wrap text.
                        sig.push(usize::from(doc.layer(it.layer).is_some_and(|l| l.visible)));
                        keep.push(it.clone());
                    }
                }
                sig.push(usize::MAX);
            }
        }
    }
    (sig, keep)
}
