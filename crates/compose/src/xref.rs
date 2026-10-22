//! Cross-reference resolution: the page each text anchor lands on (from composition) and the
//! generated text of each cross-reference.

use std::collections::HashMap;
use std::sync::Arc;

use designcraft_doc::{Document, Story, StoryId, XREF_MARK, xref};

use crate::ComposedStory;

/// Page name of every text anchor in the document.
#[derive(Debug, Default)]
pub struct XrefIndex {
    pages: HashMap<u64, (usize, String)>,
}

pub fn has_xrefs(st: &Story) -> bool {
    !st.xrefs.is_empty()
}

impl XrefIndex {
    /// `get` composes a story (normally through the cache). Stories that hold cross-references or
    /// running headers are composed without resolving them (their anchors' pages hardly move):
    /// through the cache they would build this index again, directly or by way of the running-header index.
    pub fn build(doc: &Document, get: &dyn Fn(StoryId) -> Arc<ComposedStory>) -> XrefIndex {
        let mut pages = HashMap::new();
        for (sid, st) in &doc.stories {
            if st.anchors.is_empty() {
                continue;
            }
            let indexed = has_xrefs(st) || crate::vars::has_running(doc, st);
            let cs = if indexed { Arc::new(crate::compose_story(doc, *sid, &Default::default())) } else { get(*sid) };
            for a in &st.anchors {
                let Some(pos) = st.anchor_pos(a.id) else { continue };
                if let Some((page, ..)) = crate::vars::place(doc, &cs, pos) {
                    pages.insert(a.id, (page, doc.page_name(page)));
                }
            }
        }
        XrefIndex { pages }
    }

    /// Page name of an anchor.
    pub fn page(&self, anchor: u64) -> Option<&str> {
        self.pages.get(&anchor).map(|p| p.1.as_str())
    }

    /// Absolute page index of an anchor.
    pub fn page_index(&self, anchor: u64) -> Option<usize> {
        self.pages.get(&anchor).map(|p| p.0)
    }
}

/// The text a cross-reference shows ("??" when its destination is gone, like an unresolved
/// reference in typesetting).
pub fn xref_text(doc: &Document, x: &xref::CrossRef, index: Option<&XrefIndex>) -> String {
    let Some(mut v) = doc.xref_values(x.target) else { return "??".into() };
    v.page = index.and_then(|i| i.page(x.target)).unwrap_or("?").to_string();
    let def = doc.xref_format(&x.format).map_or("<fullPara />", |f| f.definition.as_str());
    xref::expand(def, &v)
}

/// Generated text of the cross-references in `range` of `story`, by byte of their mark.
pub fn texts_in(doc: &Document, story: &Story, range: std::ops::Range<usize>, index: Option<&XrefIndex>) -> HashMap<usize, String> {
    let mut out = HashMap::new();
    if story.xrefs.is_empty() {
        return out;
    }
    let before = story.text[..range.start].matches(XREF_MARK).count();
    for (k, (i, _)) in story.text[range.clone()].match_indices(XREF_MARK).enumerate() {
        if let Some(x) = story.xrefs.get(before + k) {
            out.insert(range.start + i, xref_text(doc, x, index));
        }
    }
    out
}
