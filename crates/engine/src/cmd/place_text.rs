//! File › Place of text files (plain text, Word, RTF): into the text insertion point, the
//! selected frame, or a new frame on the page; with autoflow, pages and threaded frames are added
//! until the whole story fits (InDesign's Shift-click with a loaded text cursor).

use designcraft_doc::{
    CharacterStyle, Content, Document, ParaFormat, ParagraphStyle, SpreadRef, Story, StoryId, TextDirection, TextFileLink, TextSel,
};
use designcraft_geom::Rect;
use serde_json::{Value, json};

use super::bad;
use crate::{Result, Session};

/// Add the imported styles the document doesn't have yet (existing styles win, as in InDesign's
/// default style-conflict option).
/// How imported styles that share a name with the document's are handled (Word Import Options).
#[derive(Clone, Copy, PartialEq, Eq)]
enum Conflict {
    /// Use the document's definition.
    UseExisting,
    /// The imported definition replaces the document's.
    Redefine,
    /// Import under a new name (`Name_wrd_1`).
    AutoRename,
}

impl Conflict {
    fn parse(c: &str) -> Result<Self> {
        match c {
            "useExisting" | "" => Ok(Conflict::UseExisting),
            "redefine" => Ok(Conflict::Redefine),
            "autoRename" => Ok(Conflict::AutoRename),
            c => Err(bad("file.place", format!("unknown styleConflicts `{c}` (useExisting, redefine, autoRename)"))),
        }
    }

    fn name(self) -> &'static str {
        match self {
            Conflict::UseExisting => "useExisting",
            Conflict::Redefine => "redefine",
            Conflict::AutoRename => "autoRename",
        }
    }
}

/// Map imported style names (`styleMap`) and resolve name clashes in the import before it lands.
/// Returns `map` with the names Auto Rename chose.
fn map_styles(
    d: &Document,
    imp: &mut designcraft_textimport::Imported,
    map: &std::collections::HashMap<String, String>,
    conflict: Conflict,
) -> std::collections::HashMap<String, String> {
    let mut rename: std::collections::HashMap<String, String> = map.clone();
    if conflict == Conflict::AutoRename {
        let st = &d.styles;
        for s in imp.para_styles.iter().filter(|s| !map.contains_key(&s.name) && st.para(&s.name).is_some()) {
            if let Some(n) = (1..=st.paragraph.len() + 1).map(|i| format!("{}_wrd_{i}", s.name)).find(|n| st.para(n).is_none()) {
                rename.insert(s.name.clone(), n);
            }
        }
        for s in imp.char_styles.iter().filter(|s| !map.contains_key(&s.name) && st.char_style(&s.name).is_some()) {
            if let Some(n) = (1..=st.character.len() + 1).map(|i| format!("{}_wrd_{i}", s.name)).find(|n| st.char_style(n).is_none()) {
                rename.insert(s.name.clone(), n);
            }
        }
    }
    if rename.is_empty() {
        return rename;
    }
    // Mapped styles aren't imported; renamed ones come in under their new name.
    imp.para_styles.retain(|s| !map.contains_key(&s.name));
    imp.char_styles.retain(|s| !map.contains_key(&s.name));
    for s in imp.para_styles.iter_mut().chain(imp.char_styles.iter_mut()) {
        if let Some(n) = rename.get(&s.name) {
            s.name = n.clone();
        }
        if let Some(b) = s.based_on.as_ref().and_then(|b| rename.get(b)) {
            s.based_on = Some(b.clone());
        }
    }
    let n = imp.story.len();
    imp.story.format_paras(0..n, |p| {
        if let Some(m) = rename.get(&p.style) {
            p.style = m.clone();
        }
    });
    imp.story.format_chars(0..n, |f| {
        if let Some(m) = rename.get(&f.style) {
            f.style = m.clone();
        }
    });
    rename
}

fn add_styles(d: &mut Document, imp: &designcraft_textimport::Imported, conflict: Conflict) {
    let st = d.styles_mut();
    if conflict == Conflict::Redefine {
        for s in &imp.para_styles {
            if let Some(ps) = st.paragraph.iter_mut().find(|p| p.name == s.name) {
                (ps.para, ps.chars) = (s.para.clone(), s.chars.clone());
            }
        }
        for s in &imp.char_styles {
            if let Some(cs) = st.character.iter_mut().find(|c| c.name == s.name) {
                cs.chars = s.chars.clone();
            }
        }
    }
    for s in &imp.para_styles {
        if st.para(&s.name).is_none() {
            st.paragraph.push(ParagraphStyle {
                name: s.name.clone(),
                based_on: s.based_on.clone().or_else(|| Some(designcraft_doc::story::BASIC_PARAGRAPH.into())),
                next_style: None,
                para: s.para.clone(),
                chars: s.chars.clone(),
                shortcut: String::new(),
            });
        }
    }
    for s in &imp.char_styles {
        if !st.character.iter().any(|c| c.name == s.name) {
            st.character.push(CharacterStyle { name: s.name.clone(), based_on: s.based_on.clone(), chars: s.chars.clone(), shortcut: String::new() });
        }
    }
}

/// "Remove styles and formatting": every paragraph [Basic Paragraph], no local formatting.
fn strip(story: &mut Story) {
    let n = story.len();
    story.format_paras(0..n, |p| *p = ParaFormat { table: p.table, ..Default::default() });
    story.format_chars(0..n, |f| *f = Default::default());
}

/// The story's whole text replaced by the import's (its frames stay).
fn fill(story: &mut Story, from: &Story) {
    let n = story.len();
    story.delete(0..n);
    story.insert_story(0, from);
    if let (Some(first), Some(p0)) = (from.paras.first(), story.paras.first_mut()) {
        *p0 = ParaFormat { table: p0.table, ..first.clone() };
    }
}

/// The link a story placed from `path` keeps (Create Links When Placing Text Files).
/// `before`: the file's size and time before `bytes` were read (its time is kept only when the
/// size matches what was read and it can be trusted, see [`super::links::settled`]).
fn text_link(
    path: &str,
    bytes: &[u8],
    before: Option<designcraft_doc::Fingerprint>,
    map: std::collections::HashMap<String, String>,
    conflict: Conflict,
    remove: bool,
    story: &Story,
) -> TextFileLink {
    TextFileLink {
        path: path.to_string(),
        size: bytes.len() as u64,
        hash: super::links::fnv_hex(bytes),
        mtime: before.filter(|f| f.size == bytes.len() as u64 && super::links::settled(f)).map_or(0, |f| f.mtime),
        style_map: map.into_iter().collect(),
        style_conflicts: conflict.name().into(),
        remove_styles: remove,
        story_hash: super::links::story_hash(story),
    }
}

/// Update Link of a linked text file: `bytes` (read from `path`) imported again into the story
/// with the import options it was placed with, keeping its frames. Returns the import's warnings.
pub(crate) fn reimport(
    d: &mut Document,
    sid: StoryId,
    path: &str,
    bytes: &[u8],
    before: Option<designcraft_doc::Fingerprint>,
) -> Result<Vec<String>> {
    let link =
        d.story(sid).and_then(|s| s.file_link.clone()).ok_or_else(|| bad("links.update", format!("story {} isn't linked to a file", sid.0)))?;
    let name = std::path::Path::new(path).file_name().map_or_else(|| path.to_string(), |n| n.to_string_lossy().to_string());
    let mut imp = designcraft_textimport::import(&name, bytes).map_err(|e| bad("links.update", format!("{name}: {e}")))?;
    if link.remove_styles {
        strip(&mut imp.story);
        imp.para_styles.clear();
        imp.char_styles.clear();
    }
    let conflict = Conflict::parse(&link.style_conflicts)?;
    // A mapping to a style the document no longer has is dropped: that style is imported again.
    let map: std::collections::HashMap<String, String> =
        link.style_map.into_iter().filter(|(_, to)| d.styles.para(to).is_some() || d.styles.char_style(to).is_some()).collect();
    let map = map_styles(d, &mut imp, &map, conflict);
    add_styles(d, &imp, conflict);
    let story = d.story_mut(sid).ok_or(designcraft_doc::DocError::NoStory(sid))?;
    fill(story, &imp.story);
    story.file_link = Some(text_link(path, bytes, before, map, conflict, link.remove_styles, story));
    Ok(imp.warnings)
}

/// Is the story overset in its frames, and how much text do its full frames hold (bytes, frames)?
/// A frame a column, frame or page break ends early is left out: it says nothing about how much
/// a frame holds.
fn fit(d: &Document, sid: StoryId) -> (Option<usize>, usize, usize) {
    let cs = designcraft_compose::compose_story(d, sid, &Default::default());
    let text = d.story(sid).map_or("", |s| s.text.as_str());
    let (mut held, mut full) = (0, 0);
    for f in &cs.frames {
        let Some(shown) = text.get(f.range.clone()) else { continue };
        if !shown.is_empty() && !shown.chars().any(designcraft_doc::is_break_char) {
            held += shown.len();
            full += 1;
        }
    }
    (cs.overset_at, held, full)
}

/// The frames autoflow sets on a page: one per column inside the margins, in the order a story
/// of direction `dir` runs through them. A vertical story gets one frame on the live area.
fn flow_rects(d: &Document, dir: TextDirection, vertical: bool, page: usize) -> Vec<Rect> {
    let Some(p) = d.page(page) else { return Vec::new() };
    if vertical {
        return vec![p.margin_rect()];
    }
    let mut cols = p.column_rects();
    if dir == TextDirection::RightToLeft {
        cols.reverse();
    }
    cols
}

fn same_rect(a: Rect, b: Rect) -> bool {
    [a.x0 - b.x0, a.y0 - b.y0, a.x1 - b.x1, a.y1 - b.y1].iter().all(|v| v.abs() < 0.01)
}

/// Thread new frames until the story fits: first into the columns after the last frame's column
/// on its page (when it sits in one), then one frame per column on each page that follows it,
/// whatever else the page holds, and on each page it adds at the end of the document, as InDesign
/// does. With `primary` (Smart Text Reflow of the primary text frame), pages are added right after
/// the last frame's page, each with one frame on the live area with the page's columns instead.
/// Empty frames it made at the end of the thread are removed again; a page it added that holds
/// only such a frame goes with it. Returns pages added.
/// A story on a parent spread doesn't autoflow: its frames thread only on that parent. Errors
/// name `cmd`, the command that autoflows.
pub(crate) fn autoflow(d: &mut Document, sid: StoryId, max_pages: usize, cmd: &str, primary: bool) -> Result<usize> {
    let last = d.story(sid).and_then(|s| s.frames.last().copied()).ok_or_else(|| bad(cmd, "the story has no frame"))?;
    if !matches!(d.find(last).map(|l| l.spread), Some(SpreadRef::Doc(_))) {
        return Err(bad(cmd, "autoflow works only from a frame on a document page"));
    }
    let (dir, story_vertical) = d.story(sid).map(|s| (s.direction, s.vertical)).ok_or_else(|| bad(cmd, "no such story"))?;
    // One frame per page: on the live area, its columns those of the page.
    let vertical = story_vertical || primary;
    let mut added = 0;
    let mut made = Vec::new();
    let mut new_pages = Vec::new();
    let mut first = true;
    // The next page of the document autoflow may set frames on; it only moves forward.
    let mut following = 0;
    loop {
        let (overset, held, full) = fit(d, sid);
        let Some(at) = overset else { break };
        let len = d.story(sid).map_or(0, |s| s.len());
        // Frames still needed at the rate the full ones are filled (at least one); with no full
        // frame to go by yet, one more.
        let want = held.checked_div(full).map_or(1, |per_frame| len.saturating_sub(at).div_ceil(per_frame.max(1))).max(1);
        let last = *d.story(sid).and_then(|s| s.frames.last()).ok_or_else(|| bad(cmd, "the story has no frame"))?;
        let page = d.page_of_item(last).unwrap_or(d.page_count().saturating_sub(1));
        let layer = d.default_layer();
        let mut prev = last;
        if std::mem::take(&mut first) {
            let rects = flow_rects(d, dir, vertical, page);
            let next = d.item(last).and_then(|it| rects.iter().position(|r| same_rect(*r, it.bounds()))).map_or(rects.len(), |k| k + 1);
            if let (Some(rest), Some((si, _))) = (rects.get(next..).filter(|r| !r.is_empty()), d.page_loc(page)) {
                // Whole pages: an estimate that stops inside one would leave its later columns out
                // of the thread for good. The cleanup below takes the frames the text doesn't reach.
                for r in rest {
                    prev = add_flow_frame(d, si, *r, layer, prev)?;
                    made.push(prev);
                }
                continue;
            }
        }
        following = following.max(page + 1);
        if !primary && following < d.page_count() {
            let mut left = want;
            while left > 0 && following < d.page_count() {
                let Some((si, _)) = d.page_loc(following) else { break };
                for r in flow_rects(d, dir, vertical, following) {
                    prev = add_flow_frame(d, si, r, layer, prev)?;
                    made.push(prev);
                    left = left.saturating_sub(1);
                }
                following += 1;
            }
            continue;
        }
        if added >= max_pages {
            return Err(bad(cmd, format!("autoflow stopped after {max_pages} pages")));
        }
        // Past the following pages, `page` is the document's last: autoflow adds pages at its end.
        let per_page = if vertical { 1 } else { d.page(page).map_or(1, |p| p.column_rects().len().max(1)) };
        let pages = want.div_ceil(per_page).clamp(1, max_pages - added);
        let parent = d.page_loc(page).and_then(|(si, pi)| d.spreads[si].pages[pi].parent);
        new_pages.extend(d.insert_pages(Some(page), pages, parent)?);
        for pg in page + 1..=page + pages {
            let (si, pi) = d.page_loc(pg).ok_or_else(|| bad(cmd, "page insert failed"))?;
            let columns = d.spreads[si].pages[pi].columns.clone();
            for r in flow_rects(d, dir, vertical, pg) {
                prev = add_flow_frame(d, si, r, layer, prev)?;
                made.push(prev);
                if primary && let Some(tf) = d.item_mut(prev).and_then(|it| it.text_frame_mut()) {
                    tf.options.columns = columns.count.max(1);
                    tf.options.gutter = columns.gutter;
                }
            }
        }
        added += pages;
    }
    let cs = designcraft_compose::compose_story(d, sid, &Default::default());
    while let Some(&f) = made.last() {
        let frames = d.story(sid).map_or(&[][..], |s| s.frames.as_slice());
        let [.., before, tail] = frames else { break };
        if *tail != f || cs.frame(f).is_some_and(|ft| !ft.range.is_empty()) {
            break;
        }
        let page = d.page_of_item(f);
        if d.page_of_item(*before) != page {
            // The first frame on its page: the estimate asked for more pages than the text needs.
            // Drop the page too when autoflow added it and the frame is all it holds; a page the
            // document had keeps whatever else is on it.
            match page.filter(|pg| d.page(*pg).is_some_and(|p| new_pages.contains(&p.id))) {
                Some(pg) => {
                    let alone = d.spreads.iter().flat_map(|sp| sp.items.iter()).all(|it| it.id == f || d.page_of_item(it.id) != Some(pg));
                    if !alone {
                        break;
                    }
                    d.remove_item(f)?;
                    d.delete_pages(&[pg])?;
                    added = added.saturating_sub(1);
                }
                None => {
                    d.remove_item(f)?;
                }
            }
        } else {
            d.remove_item(f)?;
        }
        made.pop();
    }
    Ok(added)
}

fn add_flow_frame(
    d: &mut Document,
    si: usize,
    r: Rect,
    layer: designcraft_doc::LayerId,
    prev: designcraft_doc::ItemId,
) -> Result<designcraft_doc::ItemId> {
    let (fid, _) = d.add_text_frame(SpreadRef::Doc(si), r, layer, "", ParaFormat::default())?;
    d.thread(prev, fid)?;
    Ok(fid)
}

pub(super) fn place_text(
    s: &mut Session,
    p: &Value,
    name: &str,
    bytes: &[u8],
    path: Option<String>,
    before: Option<designcraft_doc::Fingerprint>,
) -> Result<Value> {
    let mut imp = designcraft_textimport::import(name, bytes).map_err(|e| bad("file.place", e.to_string()))?;
    let remove = p.get("removeStyles").and_then(Value::as_bool).unwrap_or(false);
    if remove {
        strip(&mut imp.story);
        imp.para_styles.clear();
        imp.char_styles.clear();
    }
    let conflict = Conflict::parse(p.get("styleConflicts").and_then(Value::as_str).unwrap_or("useExisting"))?;
    let map: std::collections::HashMap<String, String> = p
        .get("styleMap")
        .and_then(Value::as_object)
        .map(|o| o.iter().filter_map(|(k, v)| Some((k.clone(), v.as_str()?.to_string()))).collect())
        .unwrap_or_default();
    let map = map_styles(&s.doc()?.doc, &mut imp, &map, conflict);
    // Create Links When Placing Text Files (only a file on disk can be linked).
    let link = path.filter(|_| p.get("link").and_then(Value::as_bool).unwrap_or(s.prefs.link_text_files));
    // A file too large for the Links panel to check isn't linked.
    let (link, too_large) = match link {
        Some(_) if !super::links::checkable_size(bytes.len()) => (None, true),
        l => (l, false),
    };
    let autoflow_on = p.get("autoflow").and_then(Value::as_bool).unwrap_or(false);
    let st = s.doc()?;
    let caret = st.selection.text.filter(|t| t.cell.is_none());
    let frame = super::id_param(p, "frame").or_else(|| {
        st.selection.items.iter().copied().find(|i| st.doc.item(*i).is_some_and(|it| matches!(it.content, Content::Unassigned | Content::Text(_))))
    });
    let page = p.get("page").and_then(Value::as_u64).map(|v| (v as usize).saturating_sub(1));
    let rect = super::rect_param(p, "rect");
    let mut warnings = imp.warnings.clone();
    if too_large {
        warnings.push("not linked: the file is too large to keep linked".into());
    }
    s.edit(|d, sel| {
        add_styles(d, &imp, conflict);
        let sid = if let Some(t) = caret {
            // Into the text at the insertion point (replacing the selection).
            let story = d.story_mut(t.story).ok_or(designcraft_doc::DocError::NoStory(t.story))?;
            let r = t.range();
            story.delete(r.clone());
            // An insertion point in an empty paragraph takes the first imported paragraph's style
            // (insert_story would keep the empty paragraph's own, losing the styleMap result).
            let p0 = story.para_at(r.start);
            let blank = story.para_ranges().get(p0).is_some_and(|x| x.is_empty());
            let end = story.insert_story(r.start, &imp.story);
            if blank && let (Some(first), Some(p)) = (imp.story.paras.first(), story.paras.get_mut(p0)) {
                *p = ParaFormat { table: p.table, ..first.clone() };
            }
            sel.text = Some(TextSel { anchor: end, focus: end, ..t });
            if link.is_some() {
                warnings.push("not linked: a linked text file fills a whole story, not an insertion point".into());
            }
            t.story
        } else {
            let fid = match frame {
                Some(f) => f,
                None => {
                    let pg = page.unwrap_or(0).min(d.page_count().saturating_sub(1));
                    let (si, pi) = d.page_loc(pg).ok_or_else(|| bad("file.place", "no such page"))?;
                    // An autoflowed story starts in the first column; a placed one on the live area.
                    let first_column = autoflow_on.then(|| flow_rects(d, d.new_story_direction(), false, pg).first().copied()).flatten();
                    let r: Rect = rect.or(first_column).unwrap_or_else(|| d.spreads[si].pages[pi].margin_rect());
                    let layer = d.default_layer();
                    d.add_text_frame(SpreadRef::Doc(si), r, layer, "", ParaFormat::default())?.0
                }
            };
            // Empty frames become text frames.
            let sid = match d.item(fid).map(|i| i.content.clone()) {
                Some(Content::Text(t)) => t.story,
                Some(_) => {
                    let sid = StoryId(d.alloc());
                    let mut ns = Story::new(sid);
                    ns.direction = d.new_story_direction();
                    ns.frames = vec![fid];
                    d.stories.insert(sid, std::sync::Arc::new(ns));
                    if let Some(it) = d.item_mut(fid) {
                        it.content = Content::Text(designcraft_doc::TextFrame { story: sid, options: Default::default() });
                    }
                    sid
                }
                None => return Err(bad("file.place", "no such frame")),
            };
            let story = d.story_mut(sid).ok_or(designcraft_doc::DocError::NoStory(sid))?;
            fill(story, &imp.story);
            story.file_link = link.as_deref().map(|path| text_link(path, bytes, before, map.clone(), conflict, remove, story));
            *sel = designcraft_doc::Selection::items(vec![fid]);
            sid
        };
        let pages = if autoflow_on { autoflow(d, sid, 2000, "file.place", false)? } else { 0 };
        let overset = fit(d, sid).0.is_some();
        Ok(json!({"story": sid.0, "pagesAdded": pages, "overset": overset, "warnings": warnings}))
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn place_text_file_with_autoflow() {
        let dir = std::env::temp_dir().join(format!("dc-place-text-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("long.txt");
        let para = "The quick brown fox jumps over the lazy dog, again and again, to fill the page. ".repeat(12);
        std::fs::write(&path, vec![para.as_str(); 60].join("\n")).unwrap();
        let mut s = Session::new();
        s.execute("file.new", &json!({"pages": 1})).unwrap();
        let r = s.execute("file.place", &json!({"path": path.to_string_lossy(), "autoflow": true})).unwrap();
        assert!(r["pagesAdded"].as_u64().unwrap() >= 2, "{r}");
        assert_eq!(r["overset"], false);
        let d = &s.doc().unwrap().doc;
        let sid = StoryId(r["story"].as_u64().unwrap());
        assert_eq!(d.story(sid).unwrap().frames.len(), d.page_count());
        assert_eq!(d.story(sid).unwrap().paras.len(), 60);
        // Into the insertion point of another frame.
        let fr = s.execute("frame.create", &json!({"rect": [72, 72, 300, 200], "content": "text", "text": "Start  end"})).unwrap();
        let sid2 = fr["story"].as_u64().unwrap();
        s.execute("text.select", &json!({"story": sid2, "anchor": 6, "focus": 6})).unwrap();
        let small = dir.join("small.rtf");
        std::fs::write(&small, br"{\rtf1 {\b middle}}").unwrap();
        s.execute("file.place", &json!({"path": small.to_string_lossy()})).unwrap();
        let st = s.doc().unwrap().doc.story(StoryId(sid2)).unwrap().clone();
        assert_eq!(st.text, "Start middle end");
        assert_eq!(st.format_after(6).over.font_style.as_deref(), Some("Bold"));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn autoflow_sets_one_frame_per_column() {
        // Regression (#625): autoflow set one frame across the margins, over the gutter.
        let dir = std::env::temp_dir().join(format!("dc-place-columns-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("long.txt");
        let para = "Lorem ipsum dolor sit amet, consectetur adipiscing elit. ".repeat(12);
        std::fs::write(&path, vec![para.as_str(); 40].join("\n")).unwrap();
        for binding in ["leftToRight", "rightToLeft"] {
            let mut s = Session::new();
            s.execute("file.new", &json!({"preset": "Letter", "pages": 1, "columns": 2, "gutter": 18, "margins": 54})).unwrap();
            s.execute("layout.documentSetup", &json!({"binding": binding})).unwrap();
            let r = s.execute("file.place", &json!({"path": path.to_string_lossy(), "page": 1, "autoflow": true})).unwrap();
            assert_eq!(r["overset"], false, "{r}");
            let d = &s.doc().unwrap().doc;
            let frames = &d.story(StoryId(r["story"].as_u64().unwrap())).unwrap().frames;
            assert!(d.page_count() >= 3, "{binding}: {r}");
            // Two frames on every page but maybe the last, in column order, each on its column.
            assert!(frames.len() > 2 * (d.page_count() - 1), "{binding}: {} frames", frames.len());
            for (i, f) in frames.iter().enumerate() {
                let page = d.page_of_item(*f).unwrap();
                assert_eq!(page, i / 2, "{binding}: frame {i}");
                let mut cols = d.page(page).unwrap().column_rects();
                if binding == "rightToLeft" {
                    cols.reverse();
                }
                assert_eq!(d.item(*f).unwrap().bounds(), cols[i % 2], "{binding}: frame {i}");
            }
        }
        // One column: one frame on the live area per page, as before.
        let mut s = Session::new();
        s.execute("file.new", &json!({"preset": "Letter", "pages": 1, "margins": 54})).unwrap();
        let r = s.execute("file.place", &json!({"path": path.to_string_lossy(), "page": 1, "autoflow": true})).unwrap();
        let d = &s.doc().unwrap().doc;
        let frames = &d.story(StoryId(r["story"].as_u64().unwrap())).unwrap().frames;
        assert_eq!(frames.len(), d.page_count());
        for f in frames {
            assert_eq!(d.item(*f).unwrap().bounds(), d.page(d.page_of_item(*f).unwrap()).unwrap().margin_rect());
        }
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// Autoflow follows dragged column guides: one frame per dragged column, on the first page and
    /// on every page it adds.
    #[test]
    fn autoflow_sets_frames_on_dragged_columns() {
        let dir = std::env::temp_dir().join(format!("dc-place-dragged-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("long.txt");
        std::fs::write(&path, vec!["Lorem ipsum dolor sit amet, consectetur adipiscing elit. ".repeat(12); 40].join("\n")).unwrap();
        let mut s = Session::new();
        s.execute("file.new", &json!({"preset": "Letter", "pages": 1, "columns": 2, "gutter": 18, "margins": 54})).unwrap();
        // The margin box is 504 pt wide; the gutter is dragged left. There is no command for
        // dragging a column guide yet, so set what an import would leave, on the parent.
        let dragged = designcraft_doc::Columns { count: 2, gutter: 18.0, positions: Some(vec![0.0, 150.0, 170.0, 504.0]) };
        s.edit(|d, _| {
            for pg in &mut std::sync::Arc::make_mut(&mut d.parents[0]).pages {
                pg.columns = dragged.clone();
            }
            d.sync_parent_margins();
            Ok(())
        })
        .unwrap();
        let r = s.execute("file.place", &json!({"path": path.to_string_lossy(), "page": 1, "autoflow": true})).unwrap();
        assert_eq!(r["overset"], false, "{r}");
        let d = &s.doc().unwrap().doc;
        assert!(d.page_count() >= 3, "{r}");
        let frames = &d.story(StoryId(r["story"].as_u64().unwrap())).unwrap().frames;
        assert!(frames.len() > 2 * (d.page_count() - 1), "{} frames", frames.len());
        for (i, f) in frames.iter().enumerate() {
            let page = d.page_of_item(*f).unwrap();
            assert_eq!(page, i / 2, "frame {i}");
            let m = d.page(page).unwrap().margin_rect();
            let (x0, x1) = [(0.0, 150.0), (170.0, 504.0)][i % 2];
            assert_eq!(d.item(*f).unwrap().bounds(), Rect::new(m.x0 + x0, m.y0, m.x0 + x1, m.y1), "frame {i}");
        }
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn autoflow_keeps_the_frame_the_user_drew() {
        let dir = std::env::temp_dir().join(format!("dc-place-drawn-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("long.txt");
        std::fs::write(&path, vec!["Lorem ipsum dolor sit amet, consectetur adipiscing elit. ".repeat(12); 30].join("\n")).unwrap();
        let mut s = Session::new();
        s.execute("file.new", &json!({"preset": "Letter", "pages": 1, "columns": 2, "gutter": 18, "margins": 54})).unwrap();
        let r = s.execute("file.place", &json!({"path": path.to_string_lossy(), "rect": [100, 100, 400, 300], "autoflow": true})).unwrap();
        assert_eq!(r["overset"], false, "{r}");
        let d = &s.doc().unwrap().doc;
        let frames = &d.story(StoryId(r["story"].as_u64().unwrap())).unwrap().frames;
        assert_eq!(d.item(frames[0]).unwrap().bounds(), Rect::new(100.0, 100.0, 400.0, 300.0));
        let cols = d.page(1).unwrap().column_rects();
        assert_eq!(d.page_of_item(frames[1]), Some(1));
        assert_eq!(d.item(frames[1]).unwrap().bounds(), cols[0]);
        assert_eq!(d.item(frames[2]).unwrap().bounds(), cols[1]);
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// A facing-pages document with two columns, its inside margin wider than the outside one, so
    /// left and right pages have their columns in different places. A graphic frame sits on the
    /// page `marker` (0-based). Returns the session and the marker's id.
    fn facing_document(pages: usize, marker: usize) -> (Session, u64) {
        let mut s = Session::new();
        let margins = json!({"top": 54, "bottom": 54, "inside": 72, "outside": 36});
        s.execute("file.new", &json!({"preset": "Letter", "pages": pages, "facingPages": true, "columns": 2, "gutter": 18, "margins": margins}))
            .unwrap();
        let d = &s.doc().unwrap().doc;
        let (si, _) = d.page_loc(marker).unwrap();
        let m = d.page(marker).unwrap().margin_rect();
        let rect = [m.x0 + 20.0, m.y0 + 20.0, m.x0 + 120.0, m.y0 + 120.0];
        let id = s.execute("frame.create", &json!({"rect": rect, "spread": si, "content": "unassigned"})).unwrap()["id"].as_u64().unwrap();
        // Selected, an empty frame would take the placed text.
        s.execute("edit.deselectAll", &json!({})).unwrap();
        (s, id)
    }

    /// Every frame of the story on the column of its page, the story running through the pages in
    /// order, two frames a page.
    fn assert_frames_in_columns(d: &Document, frames: &[designcraft_doc::ItemId]) {
        for (i, f) in frames.iter().enumerate() {
            let page = d.page_of_item(*f).unwrap();
            assert_eq!(page, i / 2, "frame {i}");
            assert_eq!(d.item(*f).unwrap().bounds(), d.page(page).unwrap().column_rects()[i % 2], "frame {i}");
        }
    }

    #[test]
    fn autoflow_fills_the_pages_that_follow_before_adding_any() {
        // Gap 26: autoflow inserted pages after the first frame's page, in front of the empty
        // pages the document already had. InDesign threads frames on those first.
        let dir = std::env::temp_dir().join(format!("dc-place-following-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("long.txt");
        std::fs::write(&path, vec!["Lorem ipsum dolor sit amet, consectetur adipiscing elit. ".repeat(12); 24].join("\n")).unwrap();
        let (mut s, marker) = facing_document(6, 2);
        let ids: Vec<_> = (0..6).map(|i| s.doc().unwrap().doc.page(i).unwrap().id).collect();
        let r = s.execute("file.place", &json!({"path": path.to_string_lossy(), "page": 1, "autoflow": true})).unwrap();
        assert_eq!(r["overset"], false, "{r}");
        assert_eq!(r["pagesAdded"], 0, "{r}");
        let d = &s.doc().unwrap().doc;
        assert_eq!((0..d.page_count()).map(|i| d.page(i).unwrap().id).collect::<Vec<_>>(), ids);
        let sid = StoryId(r["story"].as_u64().unwrap());
        let frames = d.story(sid).unwrap().frames.clone();
        // Past page 3, which holds the marker: frames go on a page whatever else it holds.
        assert!(d.page_of_item(*frames.last().unwrap()).unwrap() > 2, "{} frames", frames.len());
        assert_frames_in_columns(d, &frames);
        // The text ends before the document does: no empty frame after it.
        let cs = designcraft_compose::compose_story(d, sid, &Default::default());
        assert!(frames.iter().all(|f| cs.frame(*f).is_some_and(|ft| !ft.range.is_empty())));
        assert!(d.page_of_item(*frames.last().unwrap()).unwrap() < 5);
        assert_eq!(d.page_of_item(designcraft_doc::ItemId(marker)), Some(2));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn autoflow_adds_pages_at_the_end_of_the_document() {
        let dir = std::env::temp_dir().join(format!("dc-place-end-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("long.txt");
        std::fs::write(&path, vec!["Lorem ipsum dolor sit amet, consectetur adipiscing elit. ".repeat(12); 60].join("\n")).unwrap();
        let (mut s, marker) = facing_document(3, 2);
        let ids: Vec<_> = (0..3).map(|i| s.doc().unwrap().doc.page(i).unwrap().id).collect();
        let r = s.execute("file.place", &json!({"path": path.to_string_lossy(), "page": 1, "autoflow": true})).unwrap();
        assert_eq!(r["overset"], false, "{r}");
        let d = &s.doc().unwrap().doc;
        assert!(d.page_count() > 4, "{r}");
        assert_eq!(r["pagesAdded"].as_u64(), Some(d.page_count() as u64 - 3), "{r}");
        // The document's own pages come first, the added ones after the last of them.
        assert_eq!((0..3).map(|i| d.page(i).unwrap().id).collect::<Vec<_>>(), ids);
        assert_eq!(d.page_of_item(designcraft_doc::ItemId(marker)), Some(2));
        let frames = d.story(StoryId(r["story"].as_u64().unwrap())).unwrap().frames.clone();
        assert_frames_in_columns(d, &frames);
        assert_eq!(d.page_of_item(*frames.last().unwrap()), Some(d.page_count() - 1));
        // One undo step takes the frames, the pages and the story away.
        s.execute("edit.undo", &json!({})).unwrap();
        let d = &s.doc().unwrap().doc;
        assert_eq!((0..d.page_count()).map(|i| d.page(i).unwrap().id).collect::<Vec<_>>(), ids);
        assert!(d.story(StoryId(r["story"].as_u64().unwrap())).is_none());
        assert_eq!(d.spreads.iter().map(|sp| sp.items.len()).sum::<usize>(), 1);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn autoflow_fills_every_column_of_a_following_page() {
        // A first frame across the whole live area holds twice what a column does, so autoflow
        // asks for half the frames it needs. Asked page by page, it left the later columns of a
        // page out of the thread and went on to the next page.
        let dir = std::env::temp_dir().join(format!("dc-place-under-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("long.txt");
        std::fs::write(&path, vec!["Lorem ipsum dolor sit amet, consectetur adipiscing elit. ".repeat(12); 30].join("\n")).unwrap();
        let mut s = Session::new();
        s.execute("file.new", &json!({"preset": "Letter", "pages": 6, "facingPages": false, "columns": 2, "gutter": 18, "margins": 54})).unwrap();
        let m = s.doc().unwrap().doc.page(0).unwrap().margin_rect();
        let r = s.execute("file.place", &json!({"path": path.to_string_lossy(), "rect": [m.x0, m.y0, m.x1, m.y1], "autoflow": true})).unwrap();
        assert_eq!(r["overset"], false, "{r}");
        assert_eq!(r["pagesAdded"], 0, "{r}");
        let d = &s.doc().unwrap().doc;
        let frames = &d.story(StoryId(r["story"].as_u64().unwrap())).unwrap().frames;
        assert_eq!(d.item(frames[0]).unwrap().bounds(), m);
        assert!(d.page_of_item(*frames.last().unwrap()).unwrap() >= 3, "{} frames", frames.len());
        for (k, f) in frames[1..].iter().enumerate() {
            let page = d.page_of_item(*f).unwrap();
            assert_eq!(page, 1 + k / 2, "frame {}", k + 1);
            assert_eq!(d.item(*f).unwrap().bounds(), d.page(page).unwrap().column_rects()[k % 2], "frame {}", k + 1);
        }
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn autoflow_leaves_no_empty_frame_on_the_pages_that_follow() {
        // A small first frame makes autoflow ask for more frames than the columns need; the ones
        // it set on the following pages past the end of the text go again, the pages stay.
        let dir = std::env::temp_dir().join(format!("dc-place-small-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("long.txt");
        std::fs::write(&path, vec!["Lorem ipsum dolor sit amet, consectetur adipiscing elit. ".repeat(12); 12].join("\n")).unwrap();
        let mut s = Session::new();
        s.execute("file.new", &json!({"preset": "Letter", "pages": 8, "facingPages": false, "margins": 54})).unwrap();
        let r = s.execute("file.place", &json!({"path": path.to_string_lossy(), "rect": [100, 100, 200, 130], "autoflow": true})).unwrap();
        assert_eq!(r["overset"], false, "{r}");
        assert_eq!(r["pagesAdded"], 0, "{r}");
        let d = &s.doc().unwrap().doc;
        assert_eq!(d.page_count(), 8);
        let sid = StoryId(r["story"].as_u64().unwrap());
        let frames = &d.story(sid).unwrap().frames;
        assert!(frames.len() >= 2, "{r}");
        let cs = designcraft_compose::compose_story(d, sid, &Default::default());
        assert!(frames.iter().all(|f| cs.frame(*f).is_some_and(|ft| !ft.range.is_empty())), "{} frames", frames.len());
        assert_eq!(d.spreads.iter().map(|sp| sp.items.len()).sum::<usize>(), frames.len());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn autoflow_adds_only_the_pages_breaks_ask_for() {
        // Regression (#262): a frame a break ends early made autoflow take it for a frame that
        // holds a few characters, and add a page per few characters left (33 for this text).
        let dir = std::env::temp_dir().join(format!("dc-place-breaks-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        for (name, rtf, pages) in [
            ("lead.rtf", r"{\rtf1 \page\par A Short Heading\par First short paragraph.\par Second short paragraph.\par}", 2),
            ("each.rtf", r"{\rtf1 One\par\page\par Two\par\page\par Three\par\page\par Four\par}", 4),
        ] {
            let path = dir.join(name);
            std::fs::write(&path, rtf).unwrap();
            let mut s = Session::new();
            s.execute("file.new", &json!({"pages": 1})).unwrap();
            let r = s.execute("file.place", &json!({"path": path.to_string_lossy(), "autoflow": true})).unwrap();
            assert_eq!(r["overset"], false, "{name}: {r}");
            let d = &s.doc().unwrap().doc;
            assert_eq!(d.page_count(), pages, "{name}: {r}");
            assert_eq!(d.story(StoryId(r["story"].as_u64().unwrap())).unwrap().frames.len(), pages, "{name}");
        }
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn autoflow_adds_the_pages_odd_and_even_page_breaks_ask_for() {
        // From page 1, an odd page break needs pages 2 and 3 (2 stays empty), an even one page 2.
        // A frame such a break ends early holds few characters; taken for a full frame, it made
        // autoflow add a page per few characters left.
        use designcraft_doc::{EVEN_PAGE_BREAK, ODD_PAGE_BREAK, build::NewDocument};
        for (brk, added) in [(ODD_PAGE_BREAK, 2), (EVEN_PAGE_BREAK, 1)] {
            let mut d = Document::new(&NewDocument { pages: 1, facing_pages: false, primary_text_frame: true, ..Default::default() });
            let sid = d.settings.primary_story.unwrap();
            d.story_mut(sid).unwrap().insert(
                0,
                &format!(
                    "a{brk}
{}",
                    "word ".repeat(200)
                ),
            );
            assert_eq!(autoflow(&mut d, sid, 50, "file.place", false).unwrap(), added, "{brk:?}");
            assert_eq!(d.page_count(), added + 1, "{brk:?}");
            assert_eq!(designcraft_compose::compose_story(&d, sid, &Default::default()).overset_at, None, "{brk:?}");
        }
    }
}

#[cfg(test)]
mod place_pdf_tests {
    use serde_json::json;

    use crate::Session;

    #[test]
    fn place_pdf_renders_and_exports_as_vector() {
        let dir = std::env::temp_dir().join(format!("dc-place-pdf-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let src = dir.join("ad.pdf");
        // A PDF made by our own exporter: a black square on a page.
        let mut s = Session::new();
        s.execute("file.new", &json!({"width": 200, "height": 100})).unwrap();
        let id = s.execute("frame.create", &json!({"rect": [0, 0, 100, 100]})).unwrap()["id"].as_u64().unwrap();
        s.execute("object.fill", &json!({"ids": [id], "swatch": "[Black]"})).unwrap();
        s.execute("file.exportPdf", &json!({"path": src.to_string_lossy()})).unwrap();
        // Place it in a new document.
        s.execute("file.new", &json!({})).unwrap();
        let r = s.execute("file.place", &json!({"path": src.to_string_lossy(), "x": 100, "y": 100})).unwrap();
        let d = s.doc().unwrap().doc.clone();
        let it = d.spreads[0].items.last().unwrap();
        let b = it.bounds();
        assert!((b.width() - 200.0).abs() < 1.0 && (b.height() - 100.0).abs() < 1.0, "sized by the PDF page: {b:?} {r}");
        let asset = d.assets.values().next().unwrap();
        assert_eq!(asset.mime, "application/pdf");
        // On screen: the square is black, the rest of the placed page white.
        let mut rr = designcraft_render::Renderer::new();
        let img = rr.render_page(&d, &s.cache, 0, 1.0, false, &Default::default()).unwrap();
        assert!(img.pixel(150, 150)[0] < 90, "{:?}", img.pixel(150, 150));
        assert!(img.pixel(250, 150)[0] > 200, "{:?}", img.pixel(250, 150));
        // Exported: still a valid PDF, larger than a page without the placed PDF would be.
        let out = dir.join("out.pdf");
        s.execute("file.exportPdf", &json!({"path": out.to_string_lossy()})).unwrap();
        let bytes = std::fs::read(&out).unwrap();
        let pdf = hayro_syntax::Pdf::new(std::sync::Arc::new(bytes.clone())).expect("valid pdf");
        assert_eq!(pdf.pages().len(), 1);
        let text = String::from_utf8_lossy(&bytes);
        assert!(text.contains("/Subtype /Form") || text.contains("/Subtype/Form"), "embedded as a form XObject");
        assert!(!text.contains("/Subtype /Image") && !text.contains("/Subtype/Image"), "not rasterized");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn place_svg_on_screen_and_as_vectors_in_pdf() {
        let dir = std::env::temp_dir().join(format!("dc-place-svg-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let src = dir.join("logo.svg");
        std::fs::write(
            &src,
            r##"<svg xmlns="http://www.w3.org/2000/svg" width="160" height="80"><rect width="80" height="80" fill="#0000ff"/></svg>"##,
        )
        .unwrap();
        let mut s = Session::new();
        s.execute("file.new", &json!({})).unwrap();
        s.execute("file.place", &json!({"path": src.to_string_lossy(), "x": 100, "y": 100})).unwrap();
        let d = s.doc().unwrap().doc.clone();
        let b = d.spreads[0].items.last().unwrap().bounds();
        assert!((b.width() - 120.0).abs() < 1.0 && (b.height() - 60.0).abs() < 1.0, "160×80 px = 120×60 pt: {b:?}");
        assert_eq!(d.assets.values().next().unwrap().mime, "image/svg+xml");
        let mut rr = designcraft_render::Renderer::new();
        let img = rr.render_page(&d, &s.cache, 0, 1.0, false, &Default::default()).unwrap();
        let p = img.pixel(130, 130);
        assert!(p[2] > 200 && p[0] < 60, "blue square on screen: {p:?}");
        let out = dir.join("out.pdf");
        s.execute("file.exportPdf", &json!({"path": out.to_string_lossy()})).unwrap();
        let bytes = std::fs::read(&out).unwrap();
        hayro_syntax::Pdf::new(std::sync::Arc::new(bytes.clone())).expect("valid pdf");
        let text = String::from_utf8_lossy(&bytes);
        assert!(!text.contains("/Subtype /Image") && !text.contains("/Subtype/Image"), "not rasterized");
        let _ = std::fs::remove_dir_all(&dir);
    }
}

#[cfg(test)]
mod reflow_tests {
    use serde_json::json;

    use crate::Session;

    #[test]
    fn smart_text_reflow_adds_and_removes_pages() {
        let mut s = Session::new();
        s.execute("file.new", &json!({"pages": 1, "primaryTextFrame": true})).unwrap();
        let sid = s.doc().unwrap().doc.settings.primary_story.expect("primary story").0;
        assert_eq!(s.doc().unwrap().doc.story(designcraft_doc::StoryId(sid)).unwrap().frames.len(), 1);
        s.execute("text.select", &json!({"story": sid, "anchor": 0, "focus": 0})).unwrap();
        let long = "Words keep coming and the page fills up quickly with them. ".repeat(250);
        s.execute("text.insert", &json!({"text": long})).unwrap();
        let pages = s.doc().unwrap().doc.page_count();
        assert!(pages >= 3, "pages added: {pages}");
        let d = s.doc().unwrap().doc.clone();
        assert_eq!(d.story(designcraft_doc::StoryId(sid)).unwrap().frames.len(), pages);
        // One undo takes the typing and its pages back.
        s.execute("edit.undo", &json!({})).unwrap();
        assert_eq!(s.doc().unwrap().doc.page_count(), 1);
        s.execute("edit.redo", &json!({})).unwrap();
        // Delete the text: the empty pages at the end go.
        let n = s.doc().unwrap().doc.story(designcraft_doc::StoryId(sid)).unwrap().len();
        s.execute("text.select", &json!({"story": sid, "anchor": 0, "focus": n})).unwrap();
        s.execute("text.delete", &json!({})).unwrap();
        assert_eq!(s.doc().unwrap().doc.page_count(), 1);
        s.doc().unwrap().doc.check().unwrap();
        // Off: no pages added.
        s.execute("prefs.set", &json!({"smartTextReflow": false})).unwrap();
        s.execute("text.insert", &json!({"text": long})).unwrap();
        assert_eq!(s.doc().unwrap().doc.page_count(), 1);
    }

    #[test]
    fn smart_text_reflow_adds_primary_frames_with_the_page_columns() {
        let mut s = Session::new();
        s.execute("file.new", &json!({"pages": 1, "columns": 2, "gutter": 18, "primaryTextFrame": true})).unwrap();
        let sid = s.doc().unwrap().doc.settings.primary_story.expect("primary story");
        s.execute("text.select", &json!({"story": sid.0, "anchor": 0, "focus": 0})).unwrap();
        s.execute("text.insert", &json!({"text": "Words keep coming and the page fills up quickly with them. ".repeat(250)})).unwrap();
        let d = s.doc().unwrap().doc.clone();
        let frames = &d.story(sid).unwrap().frames;
        assert!(d.page_count() >= 2);
        assert_eq!(frames.len(), d.page_count());
        for f in frames {
            let it = d.item(*f).unwrap();
            assert_eq!(it.bounds(), d.page(d.page_of_item(*f).unwrap()).unwrap().margin_rect());
            let tf = it.text_frame().unwrap();
            assert_eq!((tf.options.columns, tf.options.gutter), (2, 18.0));
        }
        let n = d.story(sid).unwrap().len();
        s.execute("text.select", &json!({"story": sid.0, "anchor": 0, "focus": n})).unwrap();
        s.execute("text.delete", &json!({})).unwrap();
        assert_eq!(s.doc().unwrap().doc.page_count(), 1);
    }

    /// Placing text into the primary frame adds only the pages the text needs, also when a wrap
    /// leaves page 1 little room or the text ends at a page boundary (#1165).
    #[test]
    fn smart_text_reflow_place_adds_no_empty_pages() {
        let dir = std::env::temp_dir().join(format!("dc-reflow-place-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        for (n, wrap, want) in [(280, true, 5), (250, false, 3)] {
            let mut s = Session::new();
            s.execute("file.new", &json!({"preset": "Letter", "pages": 1, "primaryTextFrame": true})).unwrap();
            if wrap {
                let id = s.execute("frame.create", &json!({"rect": [36, 36, 576, 600]})).unwrap()["id"].as_u64().unwrap();
                s.execute("object.textWrap", &json!({"ids": [id], "mode": "boundingBox"})).unwrap();
            }
            let sid = s.doc().unwrap().doc.settings.primary_story.expect("primary story");
            s.execute("text.select", &json!({"story": sid.0, "anchor": 0, "focus": 0})).unwrap();
            let path = dir.join(format!("words-{n}.txt"));
            std::fs::write(&path, "Words keep coming and the page fills up quickly with them. ".repeat(n)).unwrap();
            s.execute("file.place", &json!({"path": path.to_string_lossy()})).unwrap();
            let d = s.doc().unwrap().doc.clone();
            assert_eq!(d.page_count(), want, "{n} lines");
            let frames = &d.story(sid).unwrap().frames;
            assert_eq!(frames.len(), d.page_count(), "{n} lines");
            let cs = designcraft_compose::compose_story(&d, sid, &Default::default());
            assert!(cs.overset_at.is_none(), "{n} lines");
            for f in frames {
                assert!(cs.frame(*f).is_some_and(|ft| !ft.range.is_empty()), "{n} lines: empty frame {f:?}");
            }
            d.check().unwrap();
        }
        let _ = std::fs::remove_dir_all(&dir);
    }
}

#[cfg(test)]
mod pdf_page_tests {
    use serde_json::json;

    use crate::Session;

    #[test]
    fn place_a_chosen_pdf_page() {
        let dir = std::env::temp_dir().join(format!("dc-pdf-page-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let src = dir.join("two.pdf");
        // Page 1 white, page 2 a different size with a black square.
        let mut s = Session::new();
        s.execute("file.new", &json!({"pages": 2, "facingPages": false, "width": 200, "height": 100})).unwrap();
        s.execute("layout.pageSize", &json!({"pages": [2], "width": 300, "height": 150})).unwrap();
        let id = s.execute("frame.create", &json!({"spread": 1, "rect": [0, 0, 300, 150]})).unwrap()["id"].as_u64().unwrap();
        s.execute("object.fill", &json!({"ids": [id], "swatch": "[Black]"})).unwrap();
        s.execute("file.exportPdf", &json!({"path": src.to_string_lossy()})).unwrap();
        s.execute("file.new", &json!({})).unwrap();
        assert!(s.execute("file.place", &json!({"path": src.to_string_lossy(), "pdfPage": 3})).is_err());
        s.execute("file.place", &json!({"path": src.to_string_lossy(), "pdfPage": 2, "x": 100, "y": 100})).unwrap();
        let d = s.doc().unwrap().doc.clone();
        let it = d.spreads[0].items.last().unwrap();
        let b = it.bounds();
        assert!((b.width() - 300.0).abs() < 1.0 && (b.height() - 150.0).abs() < 1.0, "page 2's size: {b:?}");
        assert_eq!(d.assets.values().next().unwrap().page, 1);
        let mut rr = designcraft_render::Renderer::new();
        rr.threads = 0;
        let img = rr.render_page(&d, &s.cache, 0, 1.0, false, &Default::default()).unwrap();
        assert!(img.pixel(250, 170)[0] < 60, "page 2 (black) shows: {:?}", img.pixel(250, 170));
        let pdf = designcraft_pdf::export_pdf(&d, &s.cache, &Default::default()).unwrap();
        assert!(String::from_utf8_lossy(&pdf).contains("/Subtype /Form") || String::from_utf8_lossy(&pdf).contains("/Subtype/Form"));
        let _ = std::fs::remove_dir_all(&dir);
    }

    fn word_file() -> Vec<u8> {
        use std::io::Write;
        const W: &str = r#"xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main""#;
        let styles = format!(
            r#"<w:styles {W}><w:style w:type="paragraph" w:styleId="Normal"><w:name w:val="Normal"/></w:style>
            <w:style w:type="paragraph" w:styleId="Title"><w:name w:val="Title"/><w:rPr><w:sz w:val="56"/></w:rPr></w:style>
            <w:style w:type="paragraph" w:styleId="Body"><w:name w:val="Body"/><w:rPr><w:sz w:val="20"/></w:rPr></w:style></w:styles>"#
        );
        let doc = format!(
            r#"<w:document {W}><w:body><w:p><w:pPr><w:pStyle w:val="Title"/></w:pPr><w:r><w:t>Head</w:t></w:r></w:p>
            <w:p><w:pPr><w:pStyle w:val="Body"/></w:pPr><w:r><w:t>Text</w:t></w:r></w:p></w:body></w:document>"#
        );
        let mut w = zip::ZipWriter::new(std::io::Cursor::new(Vec::new()));
        let o = zip::write::SimpleFileOptions::default();
        for (n, c) in [("word/document.xml", doc), ("word/styles.xml", styles)] {
            w.start_file(n, o).unwrap();
            w.write_all(c.as_bytes()).unwrap();
        }
        w.finish().unwrap().into_inner()
    }

    #[test]
    fn word_import_options_map_and_rename_styles() {
        let b64 = crate::cmd::base64_encode(&word_file());
        let mut s = Session::new();
        s.execute("file.new", &json!({})).unwrap();
        s.execute("style.paragraph.create", &json!({"name": "Body", "chars": {"size": 9}})).unwrap();
        s.execute("style.paragraph.create", &json!({"name": "Headline", "chars": {"size": 30}})).unwrap();
        let info = s.execute("place.styles", &json!({"base64": b64, "name": "a.docx"})).unwrap();
        assert_eq!(info["conflicts"], json!(["Body"]));
        // Title → the document's Headline; Body clashes → imported as Body_wrd_1.
        let r = s
            .execute("file.place", &json!({"base64": b64, "name": "a.docx", "styleMap": {"Title": "Headline"}, "styleConflicts": "autoRename"}))
            .unwrap();
        let d = &s.doc().unwrap().doc;
        let st = d.story(designcraft_doc::StoryId(r["story"].as_u64().unwrap())).unwrap();
        assert_eq!(st.paras.iter().map(|p| p.style.as_str()).collect::<Vec<_>>(), ["Headline", "Body_wrd_1"]);
        assert!(d.styles.para("Title").is_none(), "mapped styles aren't imported");
        assert_eq!(d.styles.para("Body").unwrap().chars.size, Some(9.0), "the document's Body is untouched");
        // Redefine: the imported definition replaces the document's.
        s.execute("file.place", &json!({"base64": b64, "name": "a.docx", "styleConflicts": "redefine", "rect": [72, 400, 300, 500]})).unwrap();
        assert_eq!(s.doc().unwrap().doc.styles.para("Body").unwrap().chars.size, Some(10.0));
    }

    #[test]
    fn word_first_paragraph_into_empty_caret_keeps_mapped_style() {
        // Regression (#1208): placing into a new text frame (which leaves its caret selected) left the first paragraph unstyled.
        let b64 = crate::cmd::base64_encode(&word_file());
        let mut s = Session::new();
        s.execute("file.new", &json!({})).unwrap();
        s.execute("style.paragraph.create", &json!({"name": "Headline"})).unwrap();
        let fr = s.execute("frame.create", &json!({"rect": [72, 72, 300, 200], "content": "text"})).unwrap();
        let sid = fr["story"].as_u64().unwrap();
        s.execute("file.place", &json!({"base64": b64, "name": "a.docx", "frame": fr["id"], "styleMap": {"Title": "Headline"}})).unwrap();
        let st = s.doc().unwrap().doc.story(designcraft_doc::StoryId(sid)).unwrap().clone();
        assert_eq!(st.paras.iter().map(|p| p.style.as_str()).collect::<Vec<_>>(), ["Headline", "Body"]);
    }

    #[test]
    fn word_normal_comes_in_as_its_own_style() {
        // Regression (#722): Normal's formatting (over Word's document defaults) was dropped.
        use std::io::Write;
        const W: &str = r#"xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main""#;
        let styles = format!(
            r#"<w:styles {W}><w:docDefaults><w:rPrDefault><w:rPr><w:sz w:val="22"/></w:rPr></w:rPrDefault></w:docDefaults>
            <w:style w:type="paragraph" w:styleId="Normal"><w:name w:val="Normal"/><w:rPr><w:rFonts w:ascii="Georgia"/><w:sz w:val="26"/></w:rPr></w:style></w:styles>"#
        );
        let doc = format!(r#"<w:document {W}><w:body><w:p><w:r><w:t>Plain</w:t></w:r></w:p></w:body></w:document>"#);
        let mut w = zip::ZipWriter::new(std::io::Cursor::new(Vec::new()));
        for (n, c) in [("word/document.xml", doc), ("word/styles.xml", styles)] {
            w.start_file(n, zip::write::SimpleFileOptions::default()).unwrap();
            w.write_all(c.as_bytes()).unwrap();
        }
        let b64 = crate::cmd::base64_encode(&w.finish().unwrap().into_inner());
        let basic = designcraft_doc::story::BASIC_PARAGRAPH;
        let placed = |s: &Session, story: &serde_json::Value| {
            let d = &s.doc().unwrap().doc;
            let st = d.story(designcraft_doc::StoryId(story["story"].as_u64().unwrap())).unwrap();
            let (_, para_chars) = d.styles.resolve_para(&st.paras[0]);
            let c = d.styles.resolve_char(&para_chars, st.format_after(0));
            (st.paras[0].style.clone(), c.size, c.font_family)
        };
        let georgia = |style: &str| (style.to_string(), 13.0, "Georgia".to_string());
        // Default options: the document gains Normal and the text looks as it did in Word.
        let mut s = Session::new();
        s.execute("file.new", &json!({})).unwrap();
        let info = s.execute("place.styles", &json!({"base64": b64, "name": "a.docx"})).unwrap();
        assert_eq!(info["paragraph"], json!(["Normal"]));
        assert_eq!(info["conflicts"], json!([]));
        let r = s.execute("file.place", &json!({"base64": b64, "name": "a.docx"})).unwrap();
        assert_eq!(placed(&s, &r), georgia("Normal"));
        assert!(s.doc().unwrap().doc.styles.para(basic).unwrap().chars.is_empty(), "[Basic Paragraph] untouched");
        // Normal mapped to [Basic Paragraph]: no new style, and the document's definition is used
        // (mapped styles aren't imported, whatever the conflict option).
        s.execute("file.new", &json!({})).unwrap();
        let r =
            s.execute("file.place", &json!({"base64": b64, "name": "a.docx", "styleMap": {"Normal": basic}, "styleConflicts": "redefine"})).unwrap();
        assert!(s.doc().unwrap().doc.styles.para("Normal").is_none());
        assert!(s.doc().unwrap().doc.styles.para(basic).unwrap().chars.is_empty());
        let (style, size, _) = placed(&s, &r);
        assert_eq!((style.as_str(), size), (basic, 12.0));
        // A document with its own Normal: the usual conflict rules.
        s.execute("style.paragraph.create", &json!({"name": "Normal", "chars": {"size": 9}})).unwrap();
        let r = s.execute("file.place", &json!({"base64": b64, "name": "a.docx", "rect": [72, 400, 300, 500]})).unwrap();
        assert_eq!(placed(&s, &r).1, 9.0, "use existing");
        let r =
            s.execute("file.place", &json!({"base64": b64, "name": "a.docx", "styleConflicts": "autoRename", "rect": [72, 520, 300, 600]})).unwrap();
        assert_eq!(placed(&s, &r), georgia("Normal_wrd_1"));
    }
}
