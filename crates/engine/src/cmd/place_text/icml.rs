//! File › Place of an InCopy story (ICML). The IDML importer reads the file
//! ([`designcraft_idml::import_icml`]); its paragraph and character styles go through the same
//! style options as a Word file's (`styleMap`, `styleConflicts`, `removeStyles`), and the rest of
//! what the story uses comes into the document here: swatches, numbering lists, conditions,
//! stroke styles, object, table and cell styles, composite fonts and mojikumi tables (the
//! document's own definitions win, as for styles by default), placed graphics, and the stories of
//! text frames anchored in it, each with ids of its own.

use std::borrow::Cow;
use std::collections::{BTreeSet, HashMap};
use std::sync::Arc;

use designcraft_doc::{AssetId, Content, Document, Item, ItemId, NO_PARA_STYLE, Story, StoryId, story as st};
use designcraft_textimport::{Imported, ImportedStyle};

/// How deep anchored text frames may nest, as `Document::check` allows.
const MAX_DEPTH: usize = 64;

/// A text file read for File › Place: its text and styles, and for an InCopy story the document
/// holding what else the story uses.
pub(crate) struct TextImport {
    pub(crate) imp: Imported,
    pub(crate) icml: Option<Document>,
}

/// Is `name` an InCopy story file?
pub(crate) fn is_icml_name(name: &str) -> bool {
    name.to_ascii_lowercase().ends_with(".icml")
}

/// Read a text file named `name`. `file` is where it was read from: graphics an InCopy story
/// links to are looked for beside it.
pub(crate) fn import(name: &str, bytes: &[u8], file: Option<&std::path::Path>) -> std::result::Result<TextImport, String> {
    if !is_icml_name(name) {
        return designcraft_textimport::import(name, bytes).map(|imp| TextImport { imp, icml: None }).map_err(|e| e.to_string());
    }
    let r = crate::cmd::interchange::import_icml_confined(bytes, file).map_err(|e| e.to_string())?;
    let src = r.document;
    let story = src.stories.get(&r.story).map(|s| (**s).clone()).ok_or("the InCopy story is missing")?;
    let mut para_styles: Vec<ImportedStyle> = r
        .paragraph_styles
        .iter()
        .filter_map(|n| src.styles.para(n))
        // A style the file bases on nothing is based on [No Paragraph Style] here too: placing
        // it must not make it inherit the document's [Basic Paragraph].
        .map(|p| ImportedStyle {
            name: p.name.clone(),
            based_on: p.based_on.clone().or_else(|| Some(NO_PARA_STYLE.into())),
            para: p.para.clone(),
            chars: p.chars.clone(),
        })
        .collect();
    let mut char_styles: Vec<ImportedStyle> = r
        .character_styles
        .iter()
        .filter_map(|n| src.styles.char_style(n))
        .map(|c| ImportedStyle { name: c.name.clone(), based_on: c.based_on.clone(), chars: c.chars.clone(), ..Default::default() })
        .collect();
    // Styles the text applies that the file doesn't define come in empty, by name.
    let (paras, chars) = applied_styles(&src);
    let missing = |names: BTreeSet<String>, have: &[ImportedStyle], builtins: &[&str]| -> Vec<ImportedStyle> {
        names
            .into_iter()
            .filter(|n| !n.is_empty() && !builtins.contains(&n.as_str()) && !have.iter().any(|s| &s.name == n))
            .map(|name| ImportedStyle { name, ..Default::default() })
            .collect()
    };
    para_styles.extend(missing(paras, &para_styles, &[st::BASIC_PARAGRAPH, NO_PARA_STYLE]));
    char_styles.extend(missing(chars, &char_styles, &[st::NO_CHAR_STYLE]));
    let imp = Imported { story, para_styles, char_styles, warnings: r.warnings };
    Ok(TextImport { imp, icml: Some(src) })
}

/// The paragraph and character styles applied in the stories of `d`, their notes and table cells.
fn applied_styles(d: &Document) -> (BTreeSet<String>, BTreeSet<String>) {
    let (mut paras, mut chars) = (BTreeSet::new(), BTreeSet::new());
    let mut stack: Vec<(&Story, usize)> = d.stories.values().map(|s| (s.as_ref(), 0)).collect();
    while let Some((s, depth)) = stack.pop() {
        paras.extend(s.paras.iter().map(|p| p.style.clone()));
        chars.extend(s.chars.iter().map(|r| r.format.style.clone()));
        if depth < MAX_DEPTH {
            stack.extend(s.notes.iter().chain(&s.endnotes).map(|n| (&n.text, depth + 1)));
            stack.extend(s.tables.values().flat_map(|t| t.cells.iter().map(move |c| (&c.text, depth + 1))));
        }
    }
    (paras, chars)
}

/// What the story placed from an InCopy file uses, brought into `d` (see the module comment),
/// and the story to insert: its graphics and anchored objects given ids of `d`. A text file of
/// another kind brings nothing more: its story is inserted as it is.
pub(crate) fn adopt<'a>(d: &mut Document, imp: &'a Imported, src: Option<&Document>, rename: &HashMap<String, String>) -> Cow<'a, Story> {
    let Some(src) = src else { return Cow::Borrowed(&imp.story) };
    adopt_resources(d, src);
    link_next_styles(d, src, imp, rename);
    let mut story = imp.story.clone();
    let mut copies = Copies::default();
    adopt_story(d, src, &mut story, 0, &mut copies);
    copies.thread(d, src);
    Cow::Owned(story)
}

/// The stories of the file copied into the document so far: source id → the copy's id and the
/// anchored frames showing it (source id, new id). A story several anchored frames share is
/// copied once, so the copies can't outnumber the file's stories however the frames nest.
#[derive(Default)]
struct Copies {
    stories: HashMap<StoryId, (StoryId, Vec<(ItemId, ItemId)>)>,
}

impl Copies {
    /// Each copied story is threaded through the copies of its frames, in the file's thread
    /// order; frames the file's story doesn't list (or a story the file lacks) follow in the
    /// order they were met in, so every frame still shows its story.
    fn thread(self, d: &mut Document, src: &Document) {
        for (from, (to, mut frames)) in self.stories {
            let order: HashMap<ItemId, usize> =
                src.story(from).map(|s| s.frames.iter().enumerate().map(|(k, f)| (*f, k)).collect()).unwrap_or_default();
            frames.sort_by_key(|(old, _)| order.get(old).copied().unwrap_or(usize::MAX));
            if let Some(st) = d.story_mut(to) {
                st.frames = frames.into_iter().map(|(_, new)| new).collect();
            }
        }
    }
}

/// Does `want` hold an entry whose name `have` lacks?
fn lacks<T>(have: &[T], want: &[T], name: impl Fn(&T) -> &str) -> bool {
    want.iter().any(|w| !have.iter().any(|h| name(h) == name(w)))
}

/// Push each entry of `from` whose name `to` lacks onto `to`: the document's definitions win.
fn add_missing<T: Clone>(to: &mut Vec<T>, from: &[T], name: impl Fn(&T) -> &str) {
    for x in from {
        if !to.iter().any(|y| name(y) == name(x)) {
            to.push(x.clone());
        }
    }
}

/// The swatches, numbering lists, conditions, stroke styles, object, table and cell styles,
/// composite fonts and mojikumi tables of `src` that `d` lacks, by name.
fn adopt_resources(d: &mut Document, src: &Document) {
    for sw in &src.swatches {
        if d.swatch(&sw.name).is_none() {
            d.swatches.push(sw.clone());
        }
    }
    add_missing(&mut d.settings.lists, &src.settings.lists, |l| l.name.as_str());
    add_missing(&mut d.conditions, &src.conditions, |c| c.name.as_str());
    add_missing(&mut d.stroke_styles, &src.stroke_styles, |s| s.name.as_str());
    let (have, want) = (&d.styles, &src.styles);
    // The styles are shared (copy on write): copied only when something is missing.
    if !(lacks(&have.object, &want.object, |s| s.name.as_str())
        || lacks(&have.table, &want.table, |s| s.name.as_str())
        || lacks(&have.cell, &want.cell, |s| s.name.as_str())
        || lacks(&have.composite_fonts, &want.composite_fonts, |s| s.name.as_str())
        || lacks(&have.mojikumi_tables, &want.mojikumi_tables, |s| s.name.as_str()))
    {
        return;
    }
    let st = d.styles_mut();
    add_missing(&mut st.object, &want.object, |s| s.name.as_str());
    add_missing(&mut st.table, &want.table, |s| s.name.as_str());
    add_missing(&mut st.cell, &want.cell, |s| s.name.as_str());
    add_missing(&mut st.composite_fonts, &want.composite_fonts, |s| s.name.as_str());
    add_missing(&mut st.mojikumi_tables, &want.mojikumi_tables, |s| s.name.as_str());
}

/// Next Style: a paragraph style the placed file brought in (under the name `rename` gave it)
/// follows the file's next style, when the document has that style. A style the document kept
/// its own definition of keeps its own next style.
fn link_next_styles(d: &mut Document, src: &Document, imp: &Imported, rename: &HashMap<String, String>) {
    let named = |n: &str| rename.get(n).cloned().unwrap_or_else(|| n.to_string());
    let links: Vec<(String, String)> = src
        .styles
        .paragraph
        .iter()
        .filter_map(|p| Some((named(&p.name), named(p.next_style.as_deref()?))))
        .filter(|(name, next)| {
            let brought = imp.para_styles.iter().find(|s| &s.name == name);
            let ours = d.styles.para(name);
            matches!((brought, ours), (Some(b), Some(o)) if o.para == b.para && o.chars == b.chars && o.next_style.is_none())
                && d.styles.para(next).is_some()
        })
        .collect();
    if links.is_empty() {
        return;
    }
    let st = d.styles_mut();
    for (name, next) in links {
        if let Some(p) = st.paragraph.iter_mut().find(|p| p.name == name) {
            p.next_style = Some(next);
        }
    }
}

/// Give `story`'s graphics and anchored objects (in its notes and table cells too) ids of `d`,
/// bringing the graphics' files and the anchored text frames' stories from `src`.
fn adopt_story(d: &mut Document, src: &Document, story: &mut Story, depth: usize, copies: &mut Copies) {
    let mut assets: HashMap<AssetId, AssetId> = HashMap::new();
    for a in designcraft_doc::story_asset_refs(story) {
        if assets.contains_key(&a) {
            continue;
        }
        // A graphic whose file the import didn't have stays missing under a fresh id.
        let n = d.adopt_asset(src, a).unwrap_or_else(|| AssetId(d.alloc()));
        assets.insert(a, n);
    }
    designcraft_doc::remap_story_assets(story, &assets);
    renumber_objects(d, src, story, depth, copies);
}

/// Fresh ids for the anchored objects of `story`, its notes and its table cells. `depth` counts
/// the anchored text frames `story` is in; notes and cells nest as deep as the file's XML, which
/// the parser bounds.
fn renumber_objects(d: &mut Document, src: &Document, story: &mut Story, depth: usize, copies: &mut Copies) {
    for o in &mut story.objects {
        renumber_item(d, src, &mut Arc::make_mut(o).item, depth, copies);
    }
    for n in story.notes.iter_mut().chain(story.endnotes.iter_mut()) {
        if has_objects(&n.text) {
            renumber_objects(d, src, &mut Arc::make_mut(n).text, depth, copies);
        }
    }
    for t in story.tables.values_mut() {
        if t.cells.iter().any(|c| has_objects(&c.text)) {
            for c in &mut Arc::make_mut(t).cells {
                renumber_objects(d, src, &mut c.text, depth, copies);
            }
        }
    }
}

/// Does `story` hold an anchored object, in its notes and table cells too?
fn has_objects(story: &Story) -> bool {
    !story.objects.is_empty()
        || story.notes.iter().chain(&story.endnotes).any(|n| has_objects(&n.text))
        || story.tables.values().any(|t| t.cells.iter().any(|c| has_objects(&c.text)))
}

/// A fresh id for `it` and what it holds; an anchored text frame gets the copy of its story
/// (made on the first frame that needs it, threaded by [`Copies::thread`]). One nested deeper than
/// `Document::check` allows (it refused such a file already) is left empty.
fn renumber_item(d: &mut Document, src: &Document, it: &mut Item, depth: usize, copies: &mut Copies) {
    let old = it.id;
    it.id = ItemId(d.alloc());
    if d.layer(it.layer).is_none() {
        it.layer = d.default_layer();
    }
    if depth >= MAX_DEPTH && it.text_frame().is_some() {
        it.content = Content::Unassigned;
    }
    match &mut it.content {
        Content::Text(tf) => {
            if let Some((copy, frames)) = copies.stories.get_mut(&tf.story) {
                frames.push((old, it.id));
                tf.story = *copy;
                return;
            }
            let new = StoryId(d.alloc());
            copies.stories.insert(tf.story, (new, vec![(old, it.id)]));
            let mut inner = src.story(tf.story).cloned().unwrap_or_else(|| Story::new(new));
            inner.id = new;
            inner.frames.clear();
            inner.file_link = None;
            inner.link = None;
            adopt_story(d, src, &mut inner, depth + 1, copies);
            d.stories.insert(new, Arc::new(inner));
            tf.story = new;
        }
        Content::Group { items } => {
            for c in items.iter_mut() {
                renumber_item(d, src, Arc::make_mut(c), depth, copies);
            }
        }
        Content::Graphic(_) | Content::Unassigned => {}
    }
}

/// The stories of the text frames anchored in story `sid` (in its notes and table cells too), and
/// in theirs, at any depth.
pub(crate) fn anchored_stories(d: &Document, sid: StoryId) -> Vec<StoryId> {
    let owned: Vec<(StoryId, StoryId)> = d.anchored_items().into_iter().filter_map(|(owner, it)| Some((owner, it.text_frame()?.story))).collect();
    let mut out = vec![sid];
    let mut i = 0;
    while let Some(&s) = out.get(i) {
        i += 1;
        for (_, inner) in owned.iter().filter(|(owner, _)| *owner == s) {
            if !out.contains(inner) {
                out.push(*inner);
            }
        }
    }
    out.remove(0);
    out
}

/// Remove those of `stories` none of whose frames is left: their anchored text frames went with
/// the text that was replaced. Hyperlinks and placeholders in their text go with them. One that
/// keeps a frame elsewhere (threaded to a frame on a page) stays, without the frames that went.
/// Removing a story takes the frames anchored in it too, so this repeats until nothing more goes:
/// at most once per level of nesting, which `Document::check` bounds.
pub(crate) fn drop_detached(d: &mut Document, stories: &[StoryId]) {
    while !stories.is_empty() {
        let alive = live_items(d);
        let mut dropped = false;
        for sid in stories {
            let Some(st) = d.story(*sid) else { continue };
            if !st.frames.iter().any(|f| alive.contains(f)) {
                d.stories.remove(sid);
                d.prune_story_text_ranges(*sid);
                dropped = true;
            } else if !st.frames.iter().all(|f| alive.contains(f))
                && let Some(st) = d.story_mut(*sid)
            {
                st.frames.retain(|f| alive.contains(f));
            }
        }
        if !dropped {
            break;
        }
    }
}

/// The ids of every item on the spreads and parents, and anchored in a story of `d`.
fn live_items(d: &Document) -> BTreeSet<ItemId> {
    let mut alive: BTreeSet<ItemId> = d.anchored_items().into_iter().map(|(_, it)| it.id).collect();
    for sp in d.spreads.iter().chain(&d.parents) {
        for it in &sp.items {
            it.walk(&mut |i| {
                alive.insert(i.id);
            });
        }
    }
    alive
}

/// After `src` (an InCopy story) went into `story` at `pos`: its cross-references point at its
/// own text anchors again (inserting gave the anchors new ids), and when it filled the whole
/// story (`whole`), the story takes its direction and optical margin settings.
pub(crate) fn inserted(story: &mut Story, pos: usize, src: &Story, whole: bool) {
    let before = story.text.get(..pos).unwrap_or_default();
    let (na, nx) = (before.matches(designcraft_doc::ANCHOR_MARK).count(), before.matches(designcraft_doc::XREF_MARK).count());
    let ids: HashMap<u64, u64> = src.anchors.iter().enumerate().filter_map(|(k, a)| Some((a.id, story.anchors.get(na + k)?.id))).collect();
    // `Story::xrefs` and `Story::anchors` hold one entry per mark in text order (`check_marks`
    // holds the counts to that), and `Story::insert_story` wrote `src`'s entries to the slots
    // after the `nx` / `na` marks before `pos`: the same slots read here.
    let count = src.xrefs.len();
    for x in story.xrefs.iter_mut().skip(nx).take(count) {
        if let Some(&to) = ids.get(&x.target)
            && to != x.target
        {
            Arc::make_mut(x).target = to;
        }
    }
    if whole {
        story.direction = src.direction;
        story.vertical = src.vertical;
        story.optical_margin = src.optical_margin;
    }
}

#[cfg(test)]
mod tests;
