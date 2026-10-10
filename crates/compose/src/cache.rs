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
use crate::{ComposeOptions, ComposedStory, FrameSpec};

type Key = (StoryId, Option<String>);

struct Entry {
    sig: Vec<usize>,
    _keep: (Arc<Story>, Arc<Styles>, Vec<Arc<Item>>),
    out: Arc<ComposedStory>,
    stamp: u64,
    reuse: Option<ReuseWitness>,
}

/// Exact supplemental inputs that the existing pointer signature does not cover. Frame
/// preparation resolves page geometry, layer visibility and baseline grids without shaping.
#[derive(PartialEq)]
struct ReuseWitness {
    frames: Vec<FrameSpec>,
    font_epoch: u64,
    /// Keep all bits on wasm32 too; the existing signature narrows them to usize.
    advanced_type: [u64; 4],
}

impl ReuseWitness {
    fn matches(&self, frames: &[FrameSpec], font_epoch: u64, advanced_type: [u64; 4]) -> bool {
        self.font_epoch == font_epoch && self.advanced_type == advanced_type && self.frames == frames
    }

    fn after_composition(frames: Vec<FrameSpec>, before: Option<u64>, after: Option<u64>, advanced_type: [u64; 4]) -> Option<Self> {
        if before != after {
            return None;
        }
        before.map(|font_epoch| Self { frames, font_epoch, advanced_type })
    }
}

const MAX_REUSE_CONTEXT_BYTES: usize = 128 * 1024;

fn promotion_epoch_is_current(captured: u64, live: Option<u64>) -> bool {
    live == Some(captured)
}

fn reusable_story(doc: &Document, story: &Story, page_name: Option<&str>, with_vars: bool) -> bool {
    page_name.is_none()
        && !with_vars
        && story.xrefs.is_empty()
        && story.tables.is_empty()
        && story.notes.is_empty()
        && story.endnotes.is_empty()
        && story.objects.is_empty()
        && doc.conditions.is_empty()
        // These original signatures are hashes narrowed to usize; no new resurrection may
        // depend on their collision resistance. Broader contexts need a separate contract.
        && doc.hyphenation_exceptions.is_empty()
        && doc.settings.lists.is_empty()
        // Main's signature retains top-level items only. Avoid relying on ancestor identities
        // or parent/receiving-page semantics for this bounded body-story optimization.
        && story.frames.iter().all(|id| doc.find(*id).is_some_and(|loc| matches!(loc.spread, designcraft_doc::SpreadRef::Doc(_)) && loc.path.len() == 1))
}

fn bounded_body_context(frames: &[FrameSpec], capacity: usize) -> bool {
    if frames.is_empty() || frames.iter().any(|f| f.page.is_none() || !finite_frame(f)) {
        return false;
    }
    let mut bytes = std::mem::size_of::<ReuseWitness>().saturating_add(capacity.saturating_mul(std::mem::size_of::<FrameSpec>()));
    for f in frames {
        bytes = bytes
            .saturating_add(f.exclusions.capacity().saturating_mul(std::mem::size_of::<crate::Exclusion>()))
            .saturating_add(f.page_name.as_ref().map_or(0, String::capacity))
            .saturating_add(f.opts.column_rule_color.capacity());
        if bytes > MAX_REUSE_CONTEXT_BYTES {
            return false;
        }
    }
    bytes <= MAX_REUSE_CONTEXT_BYTES
}

fn finite_frame(frame: &FrameSpec) -> bool {
    let FrameSpec { id: _, area, opts, vertical: _, exclusions, page_name: _, page: _, grid, left_page: _, page_rect } = frame;
    let designcraft_doc::TextFrameOptions {
        columns: _,
        columns_kind: _,
        gutter,
        column_width,
        balance_columns: _,
        inset,
        vertical_justification: _,
        vj_paragraph_spacing_limit,
        first_baseline: _,
        first_baseline_min,
        ignore_wrap: _,
        auto_size: _,
        auto_size_ref: _,
        column_rule: _,
        column_rule_weight,
        column_rule_color: _,
        baseline_grid,
        path,
    } = opts;
    let finite_pair = |pair: &(f64, f64)| pair.0.is_finite() && pair.1.is_finite();
    area.is_finite()
        && [gutter, column_width, vj_paragraph_spacing_limit, first_baseline_min, column_rule_weight].iter().all(|v| v.is_finite())
        && inset.iter().all(|v| v.is_finite())
        && baseline_grid.as_ref().is_none_or(finite_pair)
        && path.as_ref().is_none_or(|designcraft_doc::PathType { start, flip: _, align: _ }| start.is_finite())
        && exclusions.iter().all(|e| e.rect.is_finite())
        && grid.as_ref().is_none_or(finite_pair)
        && page_rect.as_ref().is_none_or(|(page, margin)| page.is_finite() && margin.is_finite())
}

/// Test-only scheduling points in the actual cache lookup, including the final commit check.
#[cfg(test)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum LookupStage {
    CapturedEpoch,
    BeforePromotion,
    AfterComposition,
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
    previous: Mutex<HashMap<StoryId, Entry>>,
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
    doc.sections.len().hash(&mut h);
    h.finish()
}

impl Cache {
    pub fn new() -> Self {
        Self::default()
    }

    /// The composed story, from cache or freshly composed.
    pub fn get(&self, doc: &Document, sid: StoryId, page_name: Option<&str>) -> Arc<ComposedStory> {
        self.get_with_db(
            doc,
            sid,
            page_name,
            designcraft_fonts::FontDb::global(),
            #[cfg(test)]
            &mut |_| {},
        )
    }

    fn get_with_db(
        &self,
        doc: &Document,
        sid: StoryId,
        page_name: Option<&str>,
        db: &designcraft_fonts::FontDb,
        #[cfg(test)] hook: &mut dyn FnMut(LookupStage),
    ) -> Arc<ComposedStory> {
        let Some(story) = doc.stories.get(&sid) else { return Arc::new(ComposedStory { story: sid, ..Default::default() }) };
        let (mut sig, keep_items) = signature(doc, story);
        let key = (sid, page_name.map(str::to_string));
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
        // Current hits retain their existing invalidation contract. Only promotion of the
        // displaced entry needs this new, exact witness and stable font publication epoch.
        let font_epoch = db.composition_epoch();
        #[cfg(test)]
        hook(LookupStage::CapturedEpoch);
        let frames = reusable_story(doc, story, page_name, with_vars).then(|| crate::frame_specs(doc, sid));
        let type_values = [a.superscript_size, a.superscript_position, a.subscript_size, a.subscript_position];
        let advanced_type = type_values.map(f64::to_bits);
        let reusable = type_values.iter().all(|v| v.is_finite()) && frames.as_ref().is_some_and(|f| bounded_body_context(f, f.capacity()));
        if reusable && let (Some(epoch), Some(frames)) = (font_epoch, frames.as_ref()) {
            // Consistent order for promotion, insertion and clear; never compose under locks.
            let mut g = self.map.lock().unwrap_or_else(|e| e.into_inner());
            let stamp = g.1;
            let mut previous = self.previous.lock().unwrap_or_else(|e| e.into_inner());
            let matches =
                previous.get(&sid).is_some_and(|old| old.sig == sig && old.reuse.as_ref().is_some_and(|w| w.matches(frames, epoch, advanced_type)));
            #[cfg(test)]
            hook(LookupStage::BeforePromotion);
            // Publication completed during preparation/comparison must veto the swap.
            if matches
                && promotion_epoch_is_current(epoch, db.composition_epoch())
                && let Some(mut old) = previous.remove(&sid)
            {
                old.stamp = stamp;
                let out = old.out.clone();
                if let Some(displaced) = g.0.insert(key.clone(), old) {
                    previous.insert(sid, displaced);
                }
                return out;
            }
        }
        // Parent-page items are composed per page; variables need that page's index.
        let page = match page_name {
            Some(n) if with_vars => (0..doc.page_count()).find(|&i| doc.page_name(i) == n),
            _ => None,
        };
        let running = running.then(|| self.running_index(doc));
        let opts =
            ComposeOptions { page_name: page_name.map(str::to_string), page, running, label: None, xrefs: with_xrefs.then(|| self.xref_index(doc)) };
        // Reuse already prepared specs on a fresh eligible miss, too.
        let out = Arc::new(match &frames {
            Some(frames) => crate::compose_with_db(doc, story, frames, &opts, db),
            None => crate::compose_with_db(doc, story, &crate::frame_specs(doc, sid), &opts, db),
        });
        #[cfg(test)]
        hook(LookupStage::AfterComposition);
        let reuse = if reusable {
            frames.and_then(|frames| ReuseWitness::after_composition(frames, font_epoch, db.composition_epoch(), advanced_type))
        } else {
            None
        };
        let mut g = self.map.lock().unwrap_or_else(|e| e.into_inner());
        let stamp = g.1;
        if g.0.len() > 4096 {
            g.0.retain(|_, e| stamp - e.stamp < 64);
        }
        let replaced = g.0.insert(key.clone(), Entry { sig, _keep: (story.clone(), doc.styles.clone(), keep_items), out: out.clone(), stamp, reuse });
        let retired = if key.1.is_none() {
            replaced.and_then(|old| self.previous.lock().unwrap_or_else(|e| e.into_inner()).insert(sid, old))
        } else {
            replaced
        };
        drop(g);
        drop(retired);
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
        g.get(&sid).filter(|e| matches(&e.sig)).map(|e| e.out.clone())
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
        let retired = {
            let mut map = self.map.lock().unwrap_or_else(|e| e.into_inner());
            let mut previous = self.previous.lock().unwrap_or_else(|e| e.into_inner());
            (std::mem::take(&mut map.0), std::mem::take(&mut *previous))
        };
        drop(retired);
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
    let mut sig =
        vec![Arc::as_ptr(story) as usize, story.rev as usize, Arc::as_ptr(&doc.styles) as usize, doc.sections.len(), doc.font_scope as usize];
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
                        keep.push(it.clone());
                    }
                }
                sig.push(usize::MAX);
            }
        }
    }
    (sig, keep)
}

#[cfg(test)]
#[path = "cache_reuse_tests.rs"]
mod reuse_tests;
