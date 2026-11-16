//! Composition cache shared by the engine, the renderer and exporters.
//!
//! An entry is valid while the inputs it was composed from are the *same allocations*: the story
//! `Arc` (and its revision), the styles `Arc`, the frame items' `Arc`s and the `Arc`s of every
//! wrapping item on the frames' spreads, and the same for every story whose anchored objects wrap
//! its text ([`crate::wrap::neighbours`]). An anchored frame's entry also records where its host
//! story's composition places it. Entries hold clones of those `Arc`s, so the pointers
//! can't be reused by other allocations while the entry lives. The document's Alpha Channel
//! outlines count by serial number, which no other outline shares.
//!
//! When only the story itself differs from an entry's (an edit of its text or formatting), the
//! new composition starts from the entry's and composes the paragraphs the edit can change
//! ([`crate::reuse`]).

use std::collections::{HashMap, HashSet};
use std::sync::{Arc, Mutex, PoisonError};

use designcraft_doc::{Document, Item, Story, StoryId, Styles};

use crate::reuse::{Reuse, Trace};
use crate::vars::RunningIndex;
use crate::{ComposeOptions, ComposedStory};

/// Where a story's frames are shown, which decides what its page-number markers and variables
/// read.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum ShownOn {
    /// In its own frames.
    #[default]
    Own,
    /// As a parent item on this absolute document page.
    Page(usize),
    /// As a base parent's item on this parent spread (index into `parents`): markers show that
    /// parent's prefix.
    Parent(usize),
}

impl From<Option<usize>> for ShownOn {
    fn from(page: Option<usize>) -> Self {
        page.map_or(ShownOn::Own, ShownOn::Page)
    }
}

/// A story, composed for where it is shown, with other stories' anchored objects wrapping it
/// (`true`) or not.
type Key = (StoryId, ShownOn, bool);

/// The hosts and wrap neighbours already resolved while answering one [`Cache::get_on`], by
/// story, whether other stories wrap it, and depth. A story reached from many others (frames of
/// many stories anchored in it, at several levels) is resolved once instead of once per path to
/// it.
type Resolved = HashMap<(StoryId, bool, usize), Arc<ComposedStory>>;

struct Entry {
    sig: Vec<usize>,
    _keep: (Arc<Story>, Arc<Styles>, Vec<Arc<Item>>, Vec<Arc<Story>>),
    out: Arc<ComposedStory>,
    stamp: u64,
    /// For composing the story again after an edit ([`crate::reuse`]).
    trace: Option<Arc<Trace>>,
}

/// The story an entry was composed from, its composition and that composition's trace.
type Before = (Arc<Story>, Arc<ComposedStory>, Arc<Trace>);

/// How many leading values of a [`signature`] identify the story's own allocation and revision;
/// the rest is everything else the composition was made from.
const STORY_SIG: usize = 2;

impl Entry {
    /// What composing for `sig` can take from this entry: set when only the story itself differs
    /// (an edit of its text or formatting) and the entry's composition left a trace.
    fn before(&self, sig: &[usize]) -> Option<Before> {
        let trace = self.trace.as_ref()?;
        (self.sig.len() == sig.len() && self.sig.get(STORY_SIG..) == sig.get(STORY_SIG..))
            .then(|| (self._keep.0.clone(), self.out.clone(), trace.clone()))
    }
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
    /// A cache this one reads without writing to: a composition or index found there for the same
    /// inputs is used, and anything composed is kept here only.
    base: Option<Arc<Cache>>,
}

/// A signature's hash, standing in for it in another story's signature.
fn hash_of(sig: &[usize]) -> usize {
    use std::hash::{Hash, Hasher};
    let mut h = std::collections::hash_map::DefaultHasher::new();
    sig.hash(&mut h);
    h.finish() as usize
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
    // The compositions the indexes are built from also depend on these (see `signature`).
    let g = &doc.settings.baseline_grid;
    (doc.font_scope, g.relative_to == designcraft_doc::GridRelative::TopMargin, g.start.to_bits(), g.increment.to_bits()).hash(&mut h);
    outlines_hash(doc).hash(&mut h);
    h.finish()
}

/// The Alpha Channel outlines text wraps around: a relink traces a new one while the items and
/// stories stay the same.
fn outlines_hash(doc: &Document) -> u64 {
    use std::hash::{Hash, Hasher};
    let mut h = std::collections::hash_map::DefaultHasher::new();
    for o in &doc.alpha_outlines {
        o.serial.hash(&mut h);
    }
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

    /// An empty cache that also uses what `base` holds for the same inputs, and never changes
    /// `base`. A reader of an older state of the document composes that state here instead of
    /// replacing the compositions of the current state in `base`.
    pub fn over(base: Arc<Cache>) -> Self {
        Cache { base: Some(base), ..Self::default() }
    }

    /// The cache given to [`Cache::over`].
    pub fn base(&self) -> Option<&Arc<Cache>> {
        self.base.as_ref()
    }

    /// The composed story, from cache or freshly composed. `page` is the absolute page a
    /// parent-page item is shown on (its page number markers and variables are that page's), or
    /// `None` for a story's own frames. Pages are identified by index, not name: names repeat when
    /// sections restart their numbering.
    pub fn get(&self, doc: &Document, sid: StoryId, page: Option<usize>) -> Arc<ComposedStory> {
        self.get_on(doc, sid, page.into())
    }

    /// [`Cache::get`] for any [`ShownOn`].
    pub fn get_on(&self, doc: &Document, sid: StoryId, on: ShownOn) -> Arc<ComposedStory> {
        self.get_with(doc, sid, on, true, 0, &mut Resolved::new())
    }

    /// [`Cache::get_with`] for a host or a wrap neighbour (shown in its own frames), once per
    /// traversal.
    fn resolve(&self, doc: &Document, sid: StoryId, cross: bool, depth: usize, resolved: &mut Resolved) -> Arc<ComposedStory> {
        if let Some(c) = resolved.get(&(sid, cross, depth)) {
            return c.clone();
        }
        let c = self.get_with(doc, sid, ShownOn::Own, cross, depth, resolved);
        resolved.insert((sid, cross, depth), c.clone());
        c
    }

    /// Where the host story places anchored text frame `fid`, one of story `sid`'s frames; `None` for
    /// a frame on a spread.
    pub fn placement(&self, doc: &Document, sid: StoryId, fid: designcraft_doc::ItemId) -> Option<crate::Placement> {
        let mut resolved = Resolved::new();
        let placed = crate::placed::placements(doc, sid, &mut |host| self.resolve(doc, host, true, 1, &mut resolved));
        placed.into_iter().find_map(|(id, p)| (id == fid).then_some(p))
    }

    /// [`Cache::get_on`], wrapped by other stories' anchored objects only when `cross` is set, for
    /// a story `depth` hosts down from the story asked for.
    fn get_with(&self, doc: &Document, sid: StoryId, on: ShownOn, cross: bool, depth: usize, resolved: &mut Resolved) -> Arc<ComposedStory> {
        let Some(story) = doc.stories.get(&sid) else { return Arc::new(ComposedStory { story: sid, ..Default::default() }) };
        let (mut sig, mut keep_items) = signature(doc, story);
        let neighbours = if cross { crate::wrap::neighbours(doc, sid) } else { Vec::new() };
        let mut keep_stories = Vec::new();
        // A neighbour's signature goes in as its hash, and its items only once each: stories on
        // one spread share its wrapping items, so copying every neighbour's whole signature and
        // items made each entry as large as all of theirs together.
        let mut kept: HashSet<*const Item> = keep_items.iter().map(Arc::as_ptr).collect();
        for n in &neighbours {
            if let Some(ns) = doc.stories.get(n) {
                let (s, k) = signature(doc, ns);
                sig.push(usize::MAX - 1);
                sig.push(hash_of(&s));
                keep_items.extend(k.into_iter().filter(|it| kept.insert(Arc::as_ptr(it))));
                keep_stories.push(ns.clone());
            }
        }
        // Anchored frames compose where their host story places them, so the host is composed
        // first and the placements are part of the signature.
        let placed = if depth < crate::placed::MAX_DEPTH {
            crate::placed::placements(doc, sid, &mut |host| self.resolve(doc, host, true, depth + 1, resolved))
        } else {
            Vec::new()
        };
        for (id, p) in &placed {
            use std::hash::{Hash, Hasher};
            let mut h = std::collections::hash_map::DefaultHasher::new();
            (id, p.spread, p.xf.as_coeffs().map(f64::to_bits)).hash(&mut h);
            // The placed spread's pages give the frame its page, side and page rect.
            for pg in doc.spread(p.spread).map_or(&[][..], |sp| &sp.pages[..]) {
                let m = &pg.margins;
                (pg.side as u8, [pg.x, pg.width, pg.height, m.top, m.bottom, m.inside, m.outside].map(f64::to_bits)).hash(&mut h);
            }
            sig.push(h.finish() as usize);
        }
        let key = (sid, on, !neighbours.is_empty());
        // The key holds the page's index, which a renumbering doesn't change: its name (section
        // start number, style, prefix) is part of the signature.
        let (page_name, page) = match on {
            ShownOn::Own => (None, None),
            ShownOn::Page(p) => (Some(doc.page_name(p)), Some(p)),
            ShownOn::Parent(pi) => (doc.parents.get(pi).and_then(|sp| sp.parent.as_ref()).map(|i| i.prefix.clone()), None),
        };
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
        sig.extend([a.superscript_size, a.superscript_position, a.subscript_size, a.subscript_position].into_iter().flat_map(bits));
        // Missing glyphs: fallback fonts or the font's box.
        sig.push(usize::from(doc.settings.glyph_fallback));
        // Named lists continue from earlier stories.
        for l in &doc.settings.lists {
            let mut h = std::collections::hash_map::DefaultHasher::new();
            std::hash::Hash::hash(&doc.list_start(sid, &l.name), &mut h);
            sig.push(std::hash::Hasher::finish(&h) as usize);
        }
        let before = {
            let mut g = self.map.lock().unwrap_or_else(PoisonError::into_inner);
            g.1 += 1;
            let stamp = g.1;
            match g.0.get_mut(&key) {
                Some(e) if e.sig == sig => {
                    e.stamp = stamp;
                    return e.out.clone();
                }
                Some(e) => e.before(&sig),
                None => None,
            }
        };
        if let Some(base) = &self.base {
            let g = base.map.lock().unwrap_or_else(PoisonError::into_inner);
            if let Some(e) = g.0.get(&key)
                && e.sig == sig
            {
                return e.out.clone();
            }
        }
        // Parent-page items are composed per page: markers show its name, variables need its index.
        let page = page.filter(|_| with_vars);
        let running = running.then(|| self.running_index(doc));
        let others: Vec<Arc<ComposedStory>> = neighbours.iter().map(|n| self.neighbour(doc, *n, depth, resolved)).collect();
        let (out, trace) = crate::wrap::compose_among_traced(
            doc,
            sid,
            &ComposeOptions { page_name, section_marker, page, running, label: None, xrefs: with_xrefs.then(|| self.xref_index(doc)), placed },
            &others.iter().map(|o| &**o).collect::<Vec<_>>(),
            before.as_ref().map(|(story, composed, trace)| Reuse { story, composed, trace }),
        );
        let (out, trace) = (Arc::new(out), trace.map(Arc::new));
        let mut g = self.map.lock().unwrap_or_else(PoisonError::into_inner);
        let stamp = g.1;
        if g.0.len() > 4096 {
            g.0.retain(|_, e| stamp - e.stamp < 64);
        }
        let replaced =
            g.0.insert(key, Entry { sig, _keep: (story.clone(), doc.styles.clone(), keep_items, keep_stories), out: out.clone(), stamp, trace });
        drop(g);
        // Only the composition that is shown (not one made for another story's wrap).
        if key.1 == ShownOn::Own
            && cross
            && let Some(old) = replaced
        {
            self.previous.lock().unwrap_or_else(PoisonError::into_inner).insert(sid, (old.sig, old.out));
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
            let g = self.map.lock().unwrap_or_else(PoisonError::into_inner);
            for cross in [true, false] {
                if let Some(e) = g.0.get(&(sid, ShownOn::Own, cross))
                    && matches(&e.sig)
                {
                    return Some(e.out.clone());
                }
            }
        }
        let g = self.previous.lock().unwrap_or_else(PoisonError::into_inner);
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

    /// Story `sid` composed for the wrap of its anchored objects in other stories: without other
    /// stories' anchored objects (no cycles), and outside the cache when it needs the document-wide
    /// indexes (they compose every story).
    fn neighbour(&self, doc: &Document, sid: StoryId, depth: usize, resolved: &mut Resolved) -> Arc<ComposedStory> {
        let indexed =
            doc.stories.get(&sid).is_some_and(|st| crate::xref::has_xrefs(st) || (crate::vars::has_vars(st) && crate::vars::has_running(doc, st)));
        if indexed {
            Arc::new(crate::wrap::compose_among(doc, sid, &ComposeOptions::default(), &[]))
        } else {
            self.resolve(doc, sid, false, depth, resolved)
        }
    }

    pub fn clear(&self) {
        self.map.lock().unwrap_or_else(PoisonError::into_inner).0.clear();
        *self.running.lock().unwrap_or_else(PoisonError::into_inner) = None;
        *self.xrefs.lock().unwrap_or_else(PoisonError::into_inner) = None;
    }

    /// Anchor pages for cross-references (rebuilt when the document changes).
    pub fn xref_index(&self, doc: &Document) -> Arc<crate::xref::XrefIndex> {
        let sig = doc_signature(doc);
        if let Some((s, r)) = &*self.xrefs.lock().unwrap_or_else(PoisonError::into_inner)
            && *s == sig
        {
            return r.clone();
        }
        if let Some(base) = &self.base
            && let Some((s, r)) = &*base.xrefs.lock().unwrap_or_else(PoisonError::into_inner)
            && *s == sig
        {
            return r.clone();
        }
        let r = Arc::new(crate::xref::XrefIndex::build(doc, &|sid| self.get(doc, sid, None)));
        *self.xrefs.lock().unwrap_or_else(PoisonError::into_inner) = Some((sig, r.clone()));
        r
    }

    /// The running-header index for `doc` (rebuilt when the document changes).
    pub fn running_index(&self, doc: &Document) -> Arc<RunningIndex> {
        let sig = doc_signature(doc);
        if let Some((s, r)) = &*self.running.lock().unwrap_or_else(PoisonError::into_inner)
            && *s == sig
        {
            return r.clone();
        }
        if let Some(base) = &self.base
            && let Some((s, r)) = &*base.running.lock().unwrap_or_else(PoisonError::into_inner)
            && *s == sig
        {
            return r.clone();
        }
        let r = Arc::new(RunningIndex::build(doc, &|sid| self.get(doc, sid, None)));
        *self.running.lock().unwrap_or_else(PoisonError::into_inner) = Some((sig, r.clone()));
        r
    }
}

/// Both halves of `v`'s bits: a 32-bit `usize` (wasm) would drop the high half, where whole
/// numbers keep their differences.
fn bits(v: f64) -> [usize; 2] {
    let b = v.to_bits();
    [(b & 0xffff_ffff) as usize, (b >> 32) as usize]
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
        usize::from(g.relative_to == designcraft_doc::GridRelative::TopMargin),
        outlines_hash(doc) as usize,
    ];
    sig.extend([g.start, g.increment].into_iter().flat_map(bits));
    let mut keep = Vec::new();
    let mut spreads = Vec::new();
    for f in &story.frames {
        let Some(loc) = doc.find(*f) else {
            // Embedded frames live in another story (possibly in a note/cell),
            // so changing their geometry does not change this story's pointer.
            use std::hash::{Hash, Hasher};
            let mut hash = std::collections::hash_map::DefaultHasher::new();
            if let Some(item) = doc.anchored_item(*f) {
                format!("{item:?}").hash(&mut hash);
            }
            sig.push(hash.finish() as usize);
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
                // The page under a frame (found by x) gives its grid origin, page rect and side.
                for p in &sp.pages {
                    let m = &p.margins;
                    sig.extend([p.x, p.width, p.height, m.top, m.bottom, m.inside, m.outside].into_iter().flat_map(bits));
                    sig.push(p.side as usize);
                }
                for it in &sp.items {
                    let mut wraps = false;
                    it.walk(&mut |i| wraps |= i.has_wrap());
                    if wraps {
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
