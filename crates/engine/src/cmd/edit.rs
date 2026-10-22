//! Edit menu: undo/redo, selection, clear, clipboard.

use std::sync::Arc;

use designcraft_doc::{ItemId, Selection, SpreadRef};
use serde_json::{Value, json};

use super::{CommandSpec, bool_or, cmd, has_doc, has_selection, ids_param, ok};
use crate::{HistoryEntry, Result, Session};

fn can_undo(s: &Session) -> std::result::Result<(), String> {
    s.active().filter(|d| !d.history.undo.is_empty()).map(|_| ()).ok_or_else(|| "nothing to undo".into())
}
fn can_redo(s: &Session) -> std::result::Result<(), String> {
    s.active().filter(|d| !d.history.redo.is_empty()).map(|_| ()).ok_or_else(|| "nothing to redo".into())
}
fn has_clip(s: &Session) -> std::result::Result<(), String> {
    has_doc(s)?;
    let text = s.text_clipboard.is_some() && s.active().is_some_and(|d| d.selection.text.is_some());
    if s.clipboard.is_some() || text { Ok(()) } else { Err("clipboard is empty".into()) }
}

fn has_selection_or_text(s: &Session) -> std::result::Result<(), String> {
    if s.active().and_then(|d| d.selection.text).is_some_and(|t| !t.is_caret()) {
        return Ok(());
    }
    has_selection(s)
}

/// Plain text of a story slice: markers and anchored objects dropped, footnote text omitted.
fn plain(st: &designcraft_doc::Story) -> String {
    st.text.chars().filter(|c| !('\u{E000}'..='\u{E1FF}').contains(c)).map(|c| if c == '\u{2028}' { '\n' } else { c }).collect()
}

/// Copy the selected text with its formatting. Returns its plain text (for the system clipboard).
fn copy_text(s: &mut Session) -> Result<Option<String>> {
    let st = s.doc()?;
    let Some(t) = st.selection.text.filter(|t| !t.is_caret()) else { return Ok(None) };
    let Some(story) = st.doc.text_story(t.story, t.cell) else { return Ok(None) };
    let slice = story.extract(t.range());
    let text = plain(&slice);
    s.text_clipboard = Some((Arc::new(slice), text.clone()));
    Ok(Some(text))
}

/// Paste text at the insertion point: the formatted copy when the system clipboard still holds
/// what was copied here (or `formatted` is forced), else plain text in the insertion format.
fn paste_text(s: &mut Session, text: Option<String>, formatted: bool) -> Result<Value> {
    let t = s.doc()?.selection.text.ok_or_else(|| super::bad("edit.paste", "no insertion point"))?;
    let clip = s.text_clipboard.clone();
    // Items copied last (no text copied since) paste into the text as anchored objects.
    if formatted
        && clip.is_none()
        && t.cell.is_none()
        && let Some(items) = s.clipboard.clone()
    {
        let list: Vec<designcraft_doc::Item> = items.spreads.first().map(|sp| sp.items.iter().map(|i| (**i).clone()).collect()).unwrap_or_default();
        if !list.is_empty() && list.iter().all(|i| !i.is_text_frame()) {
            return s.edit(|d, sel| {
                let st = d.story_mut(t.story).ok_or(designcraft_doc::DocError::NoStory(t.story))?;
                let r = t.range();
                st.delete(r.start.min(st.len())..r.end.min(st.len()));
                let end = super::anchored::anchor_items(d, t.story, r.start, list.clone(), &Default::default())?;
                sel.text = Some(designcraft_doc::TextSel { anchor: end, focus: end, ..t });
                Ok(json!({"pos": end, "anchored": list.len()}))
            });
        }
    }
    let use_clip = formatted && clip.as_ref().is_some_and(|(_, p)| text.as_ref().is_none_or(|x| x.replace("\r\n", "\n") == *p));
    // Footnotes, endnotes, editorial notes, cross-references and tables cannot be pasted into cells or footnotes.
    if use_clip && let Some((slice, _)) = clip {
        let slice = if t.cell.is_some() { strip_objects(&slice) } else { (*slice).clone() };
        return s.edit(|d, sel| {
            let st = d.text_story_mut(t.story, t.cell).ok_or(designcraft_doc::DocError::NoStory(t.story))?;
            let r = t.range();
            let r = r.start.min(st.len())..r.end.min(st.len());
            st.delete(r.clone());
            let end = st.insert_story(r.start, &slice);
            sel.text = Some(designcraft_doc::TextSel { anchor: end, focus: end, ..t });
            Ok(json!({"pos": end, "formatted": true}))
        });
    }
    let text = text.or_else(|| clip.map(|(_, p)| p)).ok_or_else(|| super::bad("edit.paste", "clipboard is empty"))?;
    let text = outside_text(&text)?;
    s.execute("text.insert", &json!({"text": text, "raw": true}))
}

/// Text from the system clipboard as story text: Unix line ends, and none of the private-use
/// characters (U+E000–U+E1FF) that stories use as markers for breaks, footnotes, tables and
/// variables, since a stray one would be read as a marker without its payload.
fn outside_text(text: &str) -> Result<String> {
    let text: String = text.replace("\r\n", "\n").replace('\r', "\n").chars().filter(|c| !('\u{E000}'..='\u{E1FF}').contains(c)).collect();
    if text.is_empty() { Err(super::bad("edit.paste", "the clipboard holds no text")) } else { Ok(text) }
}

/// Pasted text with no insertion point: a new text frame holding it, as wide as the first column
/// of the page's margin area and as tall as the text (at most the margin area's height), centred
/// on `at` (spread coordinates) or else on the margin area. The frame ends up selected. With
/// `slice` (the text copied here), the frame's story is that formatted copy.
fn paste_text_frame(s: &mut Session, p: &Value, text: &str, slice: Option<&designcraft_doc::Story>) -> Result<Value> {
    use designcraft_geom::{Point, Rect, shapes};
    const LIMIT: f64 = 1.0e5;
    let sr = super::spread_param(p, "edit.paste")?;
    let at = super::point_param(p, "at")
        .filter(|a| a.x.is_finite() && a.y.is_finite())
        .map(|a| Point::new(a.x.clamp(-LIMIT, LIMIT), a.y.clamp(-LIMIT, LIMIT)));
    let lid = s.doc()?.active_layer;
    let cache = s.cache.clone();
    let text = if slice.is_some() { String::new() } else { outside_text(text)? };
    s.edit(|d, sel| {
        let sp = d.spread(sr).ok_or_else(|| super::bad("edit.paste", "no such spread"))?;
        let page = at.and_then(|a| sp.page_at_x(a.x)).and_then(|i| sp.pages.get(i)).or_else(|| sp.pages.first());
        let page = page.ok_or_else(|| super::bad("edit.paste", "the spread has no pages"))?;
        let margins = page.margin_rect();
        let column = page.column_rects().first().copied().unwrap_or(margins);
        let w = column.width().max(12.0);
        let max_h = margins.height().max(12.0);
        let c = at.unwrap_or(Point::new(column.center().x, margins.center().y));
        let rect = |h: f64| Rect::new(c.x - w / 2.0, c.y - h / 2.0, c.x + w / 2.0, c.y + h / 2.0);
        let para = match slice.and_then(|sl| sl.paras.first()) {
            Some(pf) => designcraft_doc::ParaFormat { table: None, ..pf.clone() },
            None => designcraft_doc::ParaFormat { style: d.styles.default_paragraph.clone(), ..Default::default() },
        };
        let (id, sid) = d.add_text_frame(sr, rect(max_h), lid, &text, para)?;
        if let Some(sl) = slice {
            d.story_mut(sid).ok_or(designcraft_doc::DocError::NoStory(sid))?.insert_story(0, sl);
        }
        // Shrink to the composed text unless it already fills the frame.
        let composed = cache.get(d, sid, None);
        if !composed.is_overset()
            && let Some(ft) = composed.frame(id)
        {
            let inset = d.item(id).and_then(|i| i.text_frame()).map_or(0.0, |tf| tf.options.inset[0] + tf.options.inset[2]);
            let h = (ft.content_height + inset + 0.5).clamp(12.0, max_h);
            if h.is_finite()
                && h < max_h
                && let Some(it) = d.item_mut(id)
            {
                it.path = shapes::rectangle(rect(h));
                // Keep the full height if the last line no longer fits.
                if cache.get(d, sid, None).is_overset()
                    && let Some(it) = d.item_mut(id)
                {
                    it.path = shapes::rectangle(rect(max_h));
                }
            }
        }
        *sel = Selection::items(vec![id]);
        Ok(json!({"id": id.0, "story": sid.0}))
    })
}

fn strip_objects(st: &designcraft_doc::Story) -> designcraft_doc::Story {
    let mut out = st.clone();
    let marks = [
        designcraft_doc::FOOTNOTE_REF,
        designcraft_doc::ENDNOTE_REF,
        designcraft_doc::NOTE_MARK,
        designcraft_doc::XREF_MARK,
        designcraft_doc::TABLE_ANCHOR,
    ];
    while let Some(i) = out.text.find(marks) {
        let n = out.text[i..].chars().next().map_or(1, char::len_utf8);
        out.delete(i..i + n);
    }
    out
}

pub fn specs() -> Vec<CommandSpec> {
    vec![
        cmd!(noundo "edit.undo", "Undo", ["Edit"], Some("Cmd+Z"), "{}", can_undo, |s, _| {
            let st = s.doc_mut()?;
            let e = st.history.undo.pop().ok_or_else(|| super::bad("edit.undo", "nothing to undo"))?;
            st.history.redo.push(HistoryEntry { label: e.label.clone(), doc: st.doc.clone(), selection: st.selection.clone() });
            let mut doc = e.doc;
            super::rulers::keep_zero_point(&st.doc, &mut doc);
            st.doc = doc;
            st.selection = e.selection;
            st.bump_revision();
            // Pending typing overrides belong to the current edit state, not a restored snapshot.
            s.typing_format = None;
            Ok(json!({"undone": e.label}))
        }),
        cmd!(noundo "edit.redo", "Redo", ["Edit"], Some("Cmd+Shift+Z"), "{}", can_redo, |s, _| {
            let st = s.doc_mut()?;
            let e = st.history.redo.pop().ok_or_else(|| super::bad("edit.redo", "nothing to redo"))?;
            st.history.undo.push(HistoryEntry { label: e.label.clone(), doc: st.doc.clone(), selection: st.selection.clone() });
            let mut doc = e.doc;
            super::rulers::keep_zero_point(&st.doc, &mut doc);
            st.doc = doc;
            st.selection = e.selection;
            st.bump_revision();
            s.typing_format = None;
            Ok(json!({"redone": e.label}))
        }),
        cmd!(noundo "selection.set", "Select", [], None, "{ids: [id], add?: bool, content?: bool}", has_doc, |s, p| {
            let ids = ids_param(p, "ids").unwrap_or_default();
            let add = bool_or(p, "add", false);
            let content = bool_or(p, "content", false);
            let st = s.doc_mut()?;
            let ids: Vec<ItemId> = ids.into_iter().filter(|i| st.doc.item(*i).is_some()).collect();
            if add {
                for i in ids {
                    if !st.selection.items.contains(&i) {
                        st.selection.items.push(i);
                    }
                }
            } else {
                st.selection = Selection { items: ids, content, ..Default::default() };
            }
            st.selection.text = None;
            st.selection.cells = None;
            st.bump_revision();
            ok()
        }),
        cmd!(noundo "selection.toggle", "Toggle Selection", [], None, "{id}", has_doc, |s, p| {
            let id = super::id_param(p, "id").ok_or_else(|| super::bad("selection.toggle", "missing id"))?;
            let st = s.doc_mut()?;
            st.selection.text = None;
            st.selection.cells = None;
            if let Some(i) = st.selection.items.iter().position(|x| *x == id) {
                st.selection.items.remove(i);
            } else {
                st.selection.items.push(id);
            }
            st.bump_revision();
            ok()
        }),
        cmd!(noundo "edit.selectAll", "Select All", ["Edit"], Some("Cmd+A"), "{} — all items on the active spreads, or all text in the story", has_doc, |s, _| {
            let st = s.doc_mut()?;
            if let Some(t) = st.selection.text {
                let len = st.doc.text_story(t.story, t.cell).map(|x| x.len()).unwrap_or(0);
                st.selection.text = Some(designcraft_doc::TextSel { anchor: 0, focus: len, ..t });
            } else {
                let mut ids = Vec::new();
                let parents = st.editing_parents;
                let refs: Vec<SpreadRef> = st.doc.spread_refs().filter(|r| matches!(r, SpreadRef::Parent(_)) == parents).collect();
                for r in refs {
                    if let Some(sp) = st.doc.spread(r) {
                        for it in &sp.items {
                            let locked = it.locked || st.doc.layer(it.layer).is_some_and(|l| l.locked || !l.visible);
                            if !locked && !it.hidden {
                                ids.push(it.id);
                            }
                        }
                    }
                }
                st.selection = Selection::items(ids);
            }
            st.bump_revision();
            ok()
        }),
        cmd!(noundo "edit.deselectAll", "Deselect All", ["Edit"], Some("Cmd+Shift+A"), "{}", has_doc, |s, _| {
            let st = s.doc_mut()?;
            st.selection = Selection::default();
            st.bump_revision();
            ok()
        }),
        cmd!(
            "edit.clear",
            "Clear",
            ["Edit"],
            Some("Delete"),
            "{ids?} — delete selected items (or the selected text or ruler guides)",
            has_doc,
            |s, p| {
                if s.doc()?.selection.text.is_some() && p.get("ids").is_none() {
                    return super::text::delete_selection(s);
                }
                if p.get("ids").is_none() && s.doc()?.selection.items.is_empty() && !s.doc()?.selection.guides.is_empty() {
                    return s.execute("guide.delete", &json!({}));
                }
                let ids = super::targets(s, p)?;
                // With a placed graphic itself selected (Direct Selection), only the graphic goes: its
                // frame stays, empty and selected (as in InDesign). Other items are deleted.
                let content = s.doc()?.selection.content && p.get("ids").is_none();
                s.edit(|d, sel| {
                    let mut kept = Vec::new();
                    for id in &ids {
                        if let Some(it) = d.item_mut(*id).filter(|it| content && matches!(it.content, designcraft_doc::Content::Graphic(_))) {
                            it.content = designcraft_doc::Content::Unassigned;
                            kept.push(*id);
                        } else {
                            let _ = d.remove_item(*id);
                        }
                    }
                    *sel = Selection::items(kept);
                    Ok(json!({"deleted": ids.len()}))
                })
            }
        ),
        cmd!(noundo "edit.copy", "Copy", ["Edit"], Some("Cmd+C"), "{} → {text} when text is selected (with formatting, footnotes, markers)", has_selection_or_text, |s, _| {
            if let Some(text) = copy_text(s)? {
                return Ok(json!({"text": text}));
            }
            copy_items(s)?;
            s.text_clipboard = None;
            ok()
        }),
        cmd!("edit.cut", "Cut", ["Edit"], Some("Cmd+X"), "{}", has_selection_or_text, |s, p| {
            if let Some(text) = copy_text(s)? {
                super::text::delete_selection(s)?;
                return Ok(json!({"text": text}));
            }
            copy_items(s)?;
            s.execute("edit.clear", p)
        }),
        cmd!(
            "edit.pasteWithoutFormatting",
            "Paste without Formatting",
            ["Edit"],
            Some("Cmd+Shift+V"),
            "{text?} — plain text at the insertion point, in the format there",
            super::has_text,
            |s, p| paste_text(s, super::str_param(p, "text").map(str::to_string), false)
        ),
        cmd!(
            "edit.paste",
            "Paste",
            ["Edit"],
            Some("Cmd+V"),
            "{inPlace?: bool, spread?: index (0), text?: the system clipboard's text (pastes it unless it is what was copied here; with no insertion point it becomes a new text frame), at?: [x, y] (spread coords: the new text frame's centre)}",
            // The system clipboard may hold text from another app, so Paste is always available.
            has_doc,
            |s, p| {
                let text = super::str_param(p, "text").filter(|t| !t.is_empty());
                if s.doc()?.selection.text.is_some() {
                    return paste_text(s, text.map(str::to_string), true);
                }
                if let Some(text) = text {
                    // What our own text copy put on the clipboard keeps its formatting.
                    let slice = s.text_clipboard.clone().filter(|(_, plain)| text.replace("\r\n", "\n") == *plain).map(|(slice, _)| slice);
                    return paste_text_frame(s, p, text, slice.as_deref());
                }
                let Some(clip) = s.clipboard.clone() else {
                    return Err(super::bad("edit.paste", "clipboard is empty"));
                };
                let off = if bool_or(p, "inPlace", false) { 0.0 } else { 12.0 };
                let to = super::spread_param(p, "edit.paste")?;
                let from = s.clipboard_origin;
                let ids: Vec<ItemId> = clip.spreads.first().map(|sp| sp.items.iter().map(|i| i.id).collect()).unwrap_or_default();
                s.edit(|d, sel| {
                    // The same place on another spread is the same distance from its spine or left edge.
                    let dx = d.spread(to).map_or(0.0, |sp| paste_origin(sp) - from);
                    let new = super::object::duplicate_from(d, &clip, &ids, to, designcraft_geom::Vec2::new(dx + off, off))?;
                    *sel = Selection::items(new.clone());
                    Ok(json!({"ids": new.iter().map(|i| i.0).collect::<Vec<_>>()}))
                })
            }
        ),
        cmd!(
            "edit.pasteInto",
            "Paste Into",
            ["Edit"],
            Some("Cmd+Alt+V"),
            "{id?} — the copied objects become the content of the selected frame (or `id`), clipped by it",
            has_clip,
            paste_into
        ),
        cmd!("edit.pasteInPlace", "Paste in Place", ["Edit"], Some("Cmd+Alt+Shift+V"), "{spread?: index (0)}", has_clip, |s, p| s
            .execute("edit.paste", &json!({"inPlace": true, "spread": p.get("spread")}))),
        cmd!("edit.duplicate", "Duplicate", ["Edit"], Some("Cmd+Alt+Shift+D"), "{}", has_selection, |s, _| s
            .execute("transform.move", &json!({"dx": 12.0, "dy": 12.0, "copy": true}))),
        // Live previews (a panel slider being dragged): the change shows on the page, and only
        // `edit.commitPreview` makes it one undo step.
        cmd!(
            query "edit.preview",
            "Preview Change",
            [],
            None,
            "{label, command, params?} — run `command` on the document as it was before the preview began, replacing the last preview; the first call opens a preview named `label` (the undo step's name)",
            has_doc,
            preview
        ),
        cmd!(query "edit.commitPreview", "Commit Preview", [], None, "{} — the open preview becomes one undo step", has_doc, |s, _| {
            s.commit_interaction()?;
            ok()
        }),
        cmd!(query "edit.cancelPreview", "Cancel Preview", [], None, "{} — the document goes back to how it was before the preview", has_doc, |s, _| {
            s.cancel_interaction();
            ok()
        }),
    ]
}

/// Put the selected items on the clipboard, with the x Paste measures them from on their spread.
fn copy_items(s: &mut Session) -> Result<()> {
    let clip = clip_doc(s)?;
    let st = s.doc()?;
    let spread = st.selection.items.iter().find_map(|id| st.doc.find(*id)).and_then(|loc| st.doc.spread(loc.spread));
    s.clipboard_origin = spread.map_or(0.0, paste_origin);
    s.clipboard = Some(Arc::new(clip));
    Ok(())
}

/// The x a pasted object keeps its distance from: the spine, or the left edge of single-sided
/// pages, which have none.
fn paste_origin(sp: &designcraft_doc::Spread) -> f64 {
    if sp.pages.iter().all(|p| p.side == designcraft_doc::PageSide::Single) { sp.bounds().x0 } else { sp.spine_x() }
}

/// `edit.preview`: open the preview (unless one of the same name is open) and show `command` on it.
fn preview(s: &mut Session, p: &Value) -> Result<Value> {
    let label = super::str_param(p, "label").filter(|l| !l.is_empty()).ok_or_else(|| super::bad("edit.preview", "missing label"))?;
    let command = super::str_param(p, "command").ok_or_else(|| super::bad("edit.preview", "missing command"))?;
    // Only an edit can be previewed: a query, undo or another preview command would not be one undo step.
    if !super::find_command(command).is_some_and(|c| c.undoable) {
        return Err(super::bad("edit.preview", format!("{command} is not an edit that can be previewed")));
    }
    let params = p.get("params").cloned().unwrap_or_else(|| json!({}));
    let opened = match s.doc()?.interaction.as_ref().map(|i| i.label.as_str()) {
        None => {
            s.begin_interaction(label)?;
            true
        }
        Some(open) if open == label => false,
        // A tool's drag (or another preview) is in progress: leave it alone.
        Some(open) => return Err(super::bad("edit.preview", format!("{open} is in progress"))),
    };
    if let Err(e) = s.preview(command, params) {
        // A preview this call opened must not stay open: later edits would get no undo step.
        if opened {
            s.cancel_interaction();
        }
        return Err(e);
    }
    ok()
}

/// A document holding copies of the selected items (and their stories) on spread 0.
pub(crate) fn clip_doc(s: &Session) -> Result<designcraft_doc::Document> {
    let st = s.doc()?;
    let mut d = (*st.doc).clone();
    let keep: Vec<ItemId> = st.selection.items.clone();
    // Gather the selected items (in their spread coordinates, nested ones too) into spread 0.
    let mut items = Vec::new();
    for id in &keep {
        let (Some(it), Some(loc)) = (st.doc.item(*id), st.doc.find(*id)) else { continue };
        let parent = st.doc.parent_xf(&loc);
        items.push(Arc::new(match &it.content {
            designcraft_doc::Content::Graphic(g) if st.selection.content => graphic_alone(it, g, parent),
            _ => designcraft_doc::Item { xf: parent * it.xf, ..it.clone() },
        }));
    }
    for sp in d.spreads.iter_mut() {
        Arc::make_mut(sp).items.clear();
    }
    // Kept frames (including inside groups).
    let mut frames = std::collections::HashSet::new();
    for it in &items {
        it.walk(&mut |i| {
            if let designcraft_doc::Content::Text(_) = &i.content {
                frames.insert(i.id);
            }
        });
    }
    if let Some(sp) = d.spreads.first_mut() {
        Arc::make_mut(sp).items = items;
    }
    // Parent spreads aren't part of a clipping; stories keep only the copied frames.
    for p in d.parents.iter_mut() {
        Arc::make_mut(p).items.clear();
    }
    d.stories.retain(|_, st| st.frames.iter().any(|f| frames.contains(f)));
    for st in d.stories.values_mut() {
        Arc::make_mut(st).frames.retain(|f| frames.contains(f));
    }
    // Graphics on the items and in the kept stories (anchored objects, graphic cells, notes).
    let assets = d.used_assets();
    d.assets.retain(|k, _| assets.contains(k));
    d.hyperlinks.clear();
    d.bookmarks.clear();
    Ok(d)
}

/// A placed graphic on its own, as InDesign copies one selected with Direct Selection: a frame of
/// the whole graphic's bounds (uncropped), where the graphic sits on the spread (`parent` is the
/// frame's parent transform), without the frame's fill, stroke or effects.
fn graphic_alone(frame: &designcraft_doc::Item, g: &designcraft_doc::Graphic, parent: designcraft_geom::Affine) -> designcraft_doc::Item {
    let r = designcraft_geom::Rect::new(0.0, 0.0, g.size.0, g.size.1);
    let mut it = designcraft_doc::Item::new(frame.id, frame.layer, designcraft_doc::Shape::Rectangle, designcraft_geom::shapes::rectangle(r));
    it.xf = parent * frame.xf * g.xf;
    it.content = designcraft_doc::Content::Graphic(designcraft_doc::Graphic {
        xf: designcraft_geom::Affine::IDENTITY,
        auto_fit: designcraft_doc::Fitting::None,
        crop: [0.0; 4],
        ..g.clone()
    });
    it
}

#[cfg(test)]
mod text_clipboard_tests {
    use super::*;

    fn frame(s: &mut Session, text: &str) -> designcraft_doc::StoryId {
        let r = s.execute("frame.create", &json!({"rect": [72, 72, 400, 300], "content": "text", "text": text})).unwrap();
        s.doc().unwrap().doc.item(ItemId(r["id"].as_u64().unwrap())).unwrap().text_frame().unwrap().story
    }

    #[test]
    fn paste_text_with_no_insertion_point_makes_a_text_frame() {
        let mut s = Session::new();
        s.execute("file.new", &json!({})).unwrap();
        let items = |s: &Session| s.doc().unwrap().doc.spreads[0].items.len();
        // Text from another app, nothing copied here: a new, selected text frame.
        let r = s.execute("edit.paste", &json!({"text": "From elsewhere\r\nSecond line"})).unwrap();
        let id = ItemId(r["id"].as_u64().unwrap());
        let d = s.doc().unwrap();
        assert_eq!(d.selection.items, vec![id]);
        assert!(d.selection.text.is_none(), "selected as an object");
        let it = d.doc.item(id).unwrap();
        assert_eq!(d.doc.story(it.text_frame().unwrap().story).unwrap().text, "From elsewhere\nSecond line");
        let page = &d.doc.spreads[0].pages[0];
        let (b, m) = (it.bounds(), page.margin_rect());
        assert!((b.width() - page.column_rects()[0].width()).abs() < 1e-6, "{b:?}");
        assert!(b.height() < m.height() / 2.0, "fits two lines: {b:?}");
        assert!((b.center().y - m.center().y).abs() < 1e-6, "{b:?}");
        assert!(!s.cache.get(&d.doc, it.text_frame().unwrap().story, None).is_overset());
        s.execute("edit.undo", &json!({})).unwrap();
        assert_eq!(items(&s), 0, "one undo removes it");
        // Centred on `at`; hostile numbers are clamped.
        let r = s.execute("edit.paste", &json!({"text": "Here", "at": [200.0, 300.0]})).unwrap();
        let b = s.doc().unwrap().doc.item(ItemId(r["id"].as_u64().unwrap())).unwrap().bounds();
        assert!((b.center().x - 200.0).abs() < 1e-6 && (b.center().y - 300.0).abs() < 1e-6, "{b:?}");
        assert!(s.execute("edit.paste", &json!({"text": "Far", "at": [1e300, -1e300], "spread": 99})).is_err());
        s.execute("edit.paste", &json!({"text": "Far", "at": [1e300, -1e300]})).unwrap();
        // Copied objects paste when no text is given.
        s.execute("edit.deselectAll", &json!({})).unwrap();
        let rect = s.execute("frame.create", &json!({"rect": [72, 72, 200, 160]})).unwrap()["id"].clone();
        s.execute("selection.set", &json!({"ids": [rect]})).unwrap();
        s.execute("edit.copy", &json!({})).unwrap();
        let n = items(&s);
        let r = s.execute("edit.paste", &json!({})).unwrap();
        assert_eq!(r["ids"].as_array().unwrap().len(), 1);
        assert_eq!(items(&s), n + 1);
        assert!(s.doc().unwrap().doc.item(ItemId(r["ids"][0].as_u64().unwrap())).unwrap().text_frame().is_none());
        s.doc().unwrap().doc.check().unwrap();
    }

    #[test]
    fn outside_text_never_pastes_markers() {
        let mut s = Session::new();
        s.execute("file.new", &json!({})).unwrap();
        // Private-use characters from another app would read as footnote, table or variable markers.
        let dirty = "a\u{E00A}b\u{E009}c\u{E1FF}d\u{E200}";
        let r = s.execute("edit.paste", &json!({"text": dirty})).unwrap();
        let st = s.doc().unwrap().doc.item(ItemId(r["id"].as_u64().unwrap())).unwrap().text_frame().unwrap().story;
        assert_eq!(s.doc().unwrap().doc.story(st).unwrap().text, "abcd\u{E200}");
        s.doc().unwrap().doc.check().unwrap();
        let b = frame(&mut s, "Target");
        s.execute("text.select", &json!({"story": b.0, "anchor": 6, "focus": 6})).unwrap();
        s.execute("edit.paste", &json!({"text": dirty})).unwrap();
        s.execute("edit.pasteWithoutFormatting", &json!({"text": dirty})).unwrap();
        assert_eq!(s.doc().unwrap().doc.story(b).unwrap().text, "Targetabcd\u{E200}abcd\u{E200}");
        assert!(s.execute("edit.paste", &json!({"text": "\u{E00A}"})).is_err());
        s.doc().unwrap().doc.check().unwrap();
    }

    #[test]
    fn copied_text_pasted_with_no_insertion_point_keeps_its_formatting() {
        let mut s = Session::new();
        s.execute("file.new", &json!({})).unwrap();
        let a = frame(&mut s, "Bold words\nSecond");
        s.execute("text.select", &json!({"story": a.0, "anchor": 0, "focus": 4})).unwrap();
        s.execute("type.bold", &json!({})).unwrap();
        s.execute("text.select", &json!({"story": a.0, "anchor": 11, "focus": 17})).unwrap();
        s.execute("type.para", &json!({"attrs": {"align": "center"}})).unwrap();
        s.execute("text.select", &json!({"story": a.0, "anchor": 0, "focus": 17})).unwrap();
        let copied = s.execute("edit.copy", &json!({})).unwrap()["text"].as_str().unwrap().to_string();
        s.execute("edit.deselectAll", &json!({})).unwrap();
        let r = s.execute("edit.paste", &json!({"text": copied})).unwrap();
        let d = s.doc().unwrap();
        let st = d.doc.story(d.doc.item(ItemId(r["id"].as_u64().unwrap())).unwrap().text_frame().unwrap().story).unwrap();
        assert_eq!(st.text, "Bold words\nSecond");
        assert_eq!(st.char_format_at(2).over.font_style.as_deref(), Some("Bold"));
        assert_eq!(st.char_format_at(7).over.font_style, None);
        assert_eq!(st.paras[1], d.doc.story(a).unwrap().paras[1]);
        d.doc.check().unwrap();
    }

    #[test]
    fn formatted_copy_paste_and_plain_paste() {
        let mut s = Session::new();
        s.execute("file.new", &json!({})).unwrap();
        let a = frame(&mut s, "Bold words here.");
        let b = frame(&mut s, "Target: .");
        s.execute("text.select", &json!({"story": a.0, "anchor": 0, "focus": 4})).unwrap();
        s.execute("type.bold", &json!({})).unwrap();
        // A footnote inside the copied range travels with it.
        s.execute("text.select", &json!({"story": a.0, "anchor": 4, "focus": 4})).unwrap();
        s.execute("footnote.insert", &json!({"text": "n"})).unwrap();
        let fl = designcraft_doc::FOOTNOTE_REF.len_utf8();
        s.execute("text.select", &json!({"story": a.0, "anchor": 0, "focus": 10 + fl})).unwrap();
        let r = s.execute("edit.copy", &json!({})).unwrap();
        assert_eq!(r["text"], "Bold words");
        // Formatted paste when the system clipboard matches.
        s.execute("text.select", &json!({"story": b.0, "anchor": 8, "focus": 8})).unwrap();
        s.execute("edit.paste", &json!({"text": "Bold words"})).unwrap();
        let st = s.doc().unwrap().doc.story(b).unwrap().clone();
        assert_eq!(st.text, format!("Target: Bold{} words.", designcraft_doc::FOOTNOTE_REF));
        assert_eq!(st.char_format_at(9).over.font_style.as_deref(), Some("Bold"));
        assert_eq!(st.notes.len(), 1);
        assert_eq!(st.notes[0].text.text, "n");
        // Other text on the system clipboard pastes as plain text.
        s.execute("edit.paste", &json!({"text": "\"x\""})).unwrap();
        assert!(s.doc().unwrap().doc.story(b).unwrap().text.contains("\"x\""), "no smart quotes on paste");
        // Paste without formatting takes the insertion format.
        s.execute("text.select", &json!({"story": b.0, "anchor": 0, "focus": 0})).unwrap();
        s.execute("edit.pasteWithoutFormatting", &json!({})).unwrap();
        let st = s.doc().unwrap().doc.story(b).unwrap().clone();
        assert!(st.text.starts_with("Bold wordsTarget"), "{:?}", st.text);
        assert_eq!(st.char_format_at(1).over.font_style, None);
        // Cut removes and copies.
        s.execute("text.select", &json!({"story": b.0, "anchor": 0, "focus": 4})).unwrap();
        assert_eq!(s.execute("edit.cut", &json!({})).unwrap()["text"], "Bold");
        assert!(s.doc().unwrap().doc.story(b).unwrap().text.starts_with(" wordsTarget"));
        s.doc().unwrap().doc.check().unwrap();
    }

    fn note_clipboard_session() -> (Session, designcraft_doc::StoryId) {
        let mut s = Session::new();
        s.execute("file.new", &json!({})).unwrap();
        let source = frame(&mut s, "aéz");
        s.execute("text.select", &json!({"story": source.0, "anchor": 1, "focus": 1})).unwrap();
        s.execute("endnote.insert", &json!({"text": "Citation A"})).unwrap();
        s.execute("note.new", &json!({"text": "Memo A", "author": "Ed"})).unwrap();
        assert_eq!(s.doc().unwrap().doc.story(source).unwrap().text, "a\u{E00F}\u{E010}éz");
        s.execute("text.select", &json!({"story": source.0, "anchor": 0, "focus": 10})).unwrap();
        s.execute("type.char", &json!({"attrs": {"fontStyle": "Bold"}})).unwrap();
        assert_eq!(s.execute("edit.copy", &json!({})).unwrap(), json!({"text": "aéz"}));
        (s, source)
    }

    fn assert_child_note_paste(
        s: &mut Session,
        source: designcraft_doc::StoryId,
        parent: designcraft_doc::StoryId,
        cell: designcraft_doc::CellAddr,
        endpoints: (usize, usize),
    ) {
        let before = s.doc().unwrap().doc.clone();
        let selected = s.doc().unwrap().selection.text.unwrap();
        assert_eq!((selected.story, selected.cell, selected.anchor, selected.focus), (parent, Some(cell), endpoints.0, endpoints.1));
        assert_eq!(selected.frame, None);
        let before_child = before.text_story(parent, Some(cell)).unwrap();
        assert_eq!(before_child.text, "L----R");
        let clipboard = s.text_clipboard.clone();
        let history = s.execute("document.history", &json!({})).unwrap();
        assert_eq!(history["redo"], json!([]));

        let result = s.execute("edit.paste", &json!({})).unwrap();
        let after = s.doc().unwrap().doc.clone();
        let child = after.text_story(parent, Some(cell)).unwrap();
        assert_eq!(child.text, "LaézR", "root-only note markers must not enter child text");
        assert_eq!(child.len(), 6);
        assert_eq!(result, json!({"pos": 5, "formatted": true}));
        assert_eq!(s.doc().unwrap().selection, Selection::text(designcraft_doc::TextSel { anchor: 5, focus: 5, ..selected }));
        assert!(child.endnotes.is_empty() && child.editorial.is_empty());
        for pos in [1, 2, 4] {
            let (_, format) = child.runs().find(|(range, _)| range.contains(&pos)).unwrap();
            assert_eq!(format.over.font_style.as_deref(), Some("Bold"));
        }
        // Inspect actual run coverage: char_format_at at a run boundary means the preceding run.
        for pos in [0, 5] {
            let (_, old_format) = before_child.runs().find(|(range, _)| range.contains(&pos)).unwrap();
            let (_, new_format) = child.runs().find(|(range, _)| range.contains(&pos)).unwrap();
            assert_eq!(new_format, old_format);
        }
        assert_eq!(after.story(parent).unwrap().text, before.story(parent).unwrap().text);
        assert_eq!(after.story(parent).unwrap().chars, before.story(parent).unwrap().chars);
        assert_eq!(after.story(source), before.story(source));
        assert_eq!(s.text_clipboard, clipboard);

        let mut pasted_history = history.clone();
        pasted_history["undo"].as_array_mut().unwrap().push(json!("Paste"));
        assert_eq!(s.execute("document.history", &json!({})).unwrap(), pasted_history);
        assert_eq!(s.execute("edit.undo", &json!({})).unwrap(), json!({"undone": "Paste"}));
        assert_eq!(s.doc().unwrap().doc, before);
        let mut undone_history = history;
        undone_history["redo"] = json!(["Paste"]);
        assert_eq!(s.execute("document.history", &json!({})).unwrap(), undone_history);
        // This filter does not change which selection Undo restores.
        assert_eq!(s.execute("edit.redo", &json!({})).unwrap(), json!({"redone": "Paste"}));
        assert_eq!(s.doc().unwrap().doc, after);
        assert_eq!(s.execute("document.history", &json!({})).unwrap(), pasted_history);
    }

    #[test]
    fn formatted_child_paste_filters_endnotes_and_editorial_in_cells() {
        let (mut s, source) = note_clipboard_session();
        let parent = frame(&mut s, "PZ");
        s.execute("text.select", &json!({"story": parent.0, "anchor": 1, "focus": 1})).unwrap();
        let table_id = s.execute("table.insert", &json!({"rows": 1, "cols": 2})).unwrap()["table"].as_u64().unwrap();
        s.execute("text.insert", &json!({"text": "L----R"})).unwrap();
        s.execute("table.nextCell", &json!({})).unwrap();
        s.execute("text.insert", &json!({"text": "NEIGHBOR"})).unwrap();
        s.execute("table.prevCell", &json!({})).unwrap();
        s.execute("text.move", &json!({"dir": "storyStart"})).unwrap();
        s.execute("text.move", &json!({"dir": "right"})).unwrap();
        for _ in 0..4 {
            s.execute("text.move", &json!({"dir": "right", "extend": true})).unwrap();
        }
        let before_table = s.doc().unwrap().doc.story(parent).unwrap().tables[&table_id].clone();
        let cell = designcraft_doc::CellAddr { table: table_id, row: 0, col: 0 };
        assert_child_note_paste(&mut s, source, parent, cell, (1, 5));
        let table = &s.doc().unwrap().doc.story(parent).unwrap().tables[&table_id];
        assert_eq!(table.cell(0, 1).unwrap().text.text, "NEIGHBOR");
        let mut expected_table = (*before_table).clone();
        expected_table.cells[0].text = table.cells[0].text.clone();
        assert_eq!(table.as_ref(), &expected_table);

        // The same clipboard must still retain both markers when pasted into an ordinary root story.
        let root = frame(&mut s, "L----R");
        s.execute("text.select", &json!({"story": root.0, "anchor": 1, "focus": 5})).unwrap();
        assert_eq!(s.execute("edit.paste", &json!({})).unwrap(), json!({"pos": 11, "formatted": true}));
        assert_eq!(s.doc().unwrap().doc.story(root).unwrap().text, "La\u{E00F}\u{E010}ézR");
    }

    #[test]
    fn formatted_child_paste_filters_endnotes_and_editorial_in_footnotes() {
        let (mut s, source) = note_clipboard_session();
        let parent = frame(&mut s, "PZ");
        let neighbor = s.execute("footnote.insert", &json!({"text": "NEIGHBOR"})).unwrap()["id"].as_u64().unwrap();
        s.execute("text.select", &json!({"story": parent.0, "anchor": 1, "focus": 1})).unwrap();
        let target = s.execute("footnote.insert", &json!({"text": "L----R"})).unwrap()["id"].as_u64().unwrap();
        s.execute("text.move", &json!({"dir": "left"})).unwrap();
        for _ in 0..4 {
            s.execute("text.move", &json!({"dir": "left", "extend": true})).unwrap();
        }
        let before = s.doc().unwrap().doc.story(parent).unwrap().clone();
        assert_child_note_paste(&mut s, source, parent, designcraft_doc::CellAddr::footnote(target), (5, 1));
        let after = s.doc().unwrap().doc.story(parent).unwrap();
        assert_eq!(after.note(neighbor).unwrap().text.text, "NEIGHBOR");
        assert_eq!(after.note(neighbor), before.note(neighbor));
        assert_eq!(after.notes.iter().map(|note| note.id).collect::<Vec<_>>(), before.notes.iter().map(|note| note.id).collect::<Vec<_>>());
    }

    #[test]
    fn change_case_keeps_runs() {
        let mut s = Session::new();
        s.execute("file.new", &json!({})).unwrap();
        let a = frame(&mut s, "the quick brown fox. jumps over straße");
        s.execute("text.select", &json!({"story": a.0, "anchor": 4, "focus": 9})).unwrap();
        s.execute("type.italic", &json!({})).unwrap();
        let len = s.doc().unwrap().doc.story(a).unwrap().len();
        s.execute("text.select", &json!({"story": a.0, "anchor": 0, "focus": len})).unwrap();
        s.execute("type.changeCase", &json!({"case": "title"})).unwrap();
        assert_eq!(s.doc().unwrap().doc.story(a).unwrap().text, "The Quick Brown Fox. Jumps Over Straße");
        s.execute("type.changeCase", &json!({"case": "sentence"})).unwrap();
        assert_eq!(s.doc().unwrap().doc.story(a).unwrap().text, "The quick brown fox. Jumps over straße");
        s.execute("type.changeCase", &json!({"case": "upper"})).unwrap();
        let st = s.doc().unwrap().doc.story(a).unwrap().clone();
        assert_eq!(st.text, "THE QUICK BROWN FOX. JUMPS OVER STRASSE");
        assert_eq!(st.char_format_at(5).over.font_style.as_deref(), Some("Italic"));
        assert_eq!(s.doc().unwrap().selection.text.unwrap().range(), 0..st.len());
        st.check().unwrap();
    }

    #[test]
    fn change_case_leaves_georgian_without_initial_capitals() {
        let mut s = Session::new();
        s.execute("file.new", &json!({})).unwrap();
        let a = frame(&mut s, "ᲒᲐᲛᲐᲠᲯᲝᲑᲐ ᲛᲡᲝᲤᲚᲘᲝ. ᲐᲮᲐᲚᲘ word");
        let len = s.doc().unwrap().doc.story(a).unwrap().len();
        s.execute("text.select", &json!({"story": a.0, "anchor": 0, "focus": len})).unwrap();
        // Georgian has no title or sentence capitals: Mtavruli is only for all-caps text.
        s.execute("type.changeCase", &json!({"case": "title"})).unwrap();
        assert_eq!(s.doc().unwrap().doc.story(a).unwrap().text, "გამარჯობა მსოფლიო. ახალი Word");
        s.execute("type.changeCase", &json!({"case": "sentence"})).unwrap();
        assert_eq!(s.doc().unwrap().doc.story(a).unwrap().text, "გამარჯობა მსოფლიო. ახალი word");
        s.execute("type.changeCase", &json!({"case": "upper"})).unwrap();
        assert_eq!(s.doc().unwrap().doc.story(a).unwrap().text, "ᲒᲐᲛᲐᲠᲯᲝᲑᲐ ᲛᲡᲝᲤᲚᲘᲝ. ᲐᲮᲐᲚᲘ WORD");
    }
}

fn paste_into(s: &mut Session, p: &Value) -> Result<Value> {
    use super::object::RemoveKeep;
    let clip = s.clipboard.clone().ok_or_else(|| super::bad("edit.pasteInto", "the clipboard holds no objects"))?;
    let target = match super::id_param(p, "id") {
        Some(id) => id,
        None => {
            let st = s.doc()?;
            match st.selection.items.as_slice() {
                [one] => *one,
                _ => return Err(super::bad("edit.pasteInto", "select one frame to paste into")),
            }
        }
    };
    let ids: Vec<ItemId> = clip.spreads.first().map(|sp| sp.items.iter().map(|i| i.id).collect()).unwrap_or_default();
    if ids.is_empty() {
        return Err(super::bad("edit.pasteInto", "the clipboard holds no objects"));
    }
    s.edit(|d, sel| {
        let loc = d.find(target).ok_or(designcraft_doc::DocError::NoItem(target))?;
        let frame = d.item_at(&loc).ok_or(designcraft_doc::DocError::NoItem(target))?.clone();
        if frame.is_group() || frame.is_text_frame() {
            return Err(super::bad("edit.pasteInto", "paste into a graphic or empty frame (not a group or text frame)"));
        }
        let world = d.parent_xf(&loc) * frame.xf;
        // Copies on the frame's spread (stories and images come along), then nested in the frame.
        let new = super::object::duplicate_from(d, &clip, &ids, loc.spread, designcraft_geom::Vec2::ZERO)?;
        let mut kids = Vec::new();
        for id in &new {
            kids.push(d.remove_item_keep_story(*id)?);
        }
        // Centre the content on the frame when it doesn't overlap it (like InDesign).
        let fb = frame.bounds();
        let cb = kids.iter().map(|k| k.bounds()).reduce(|a, b| a.union(b)).unwrap_or(fb);
        let shift = if cb.intersect(fb).area() > 0.0 { designcraft_geom::Vec2::ZERO } else { fb.center().to_vec2() - cb.center().to_vec2() };
        let inv = world.inverse();
        let kids: Vec<Arc<designcraft_doc::Item>> = kids
            .into_iter()
            .map(|mut k| {
                k.xf = inv * designcraft_geom::Affine::translate(shift) * k.xf;
                Arc::new(k)
            })
            .collect();
        let it = d.item_mut(target).ok_or(designcraft_doc::DocError::NoItem(target))?;
        it.content = designcraft_doc::Content::Group { items: kids };
        *sel = Selection::items(vec![target]);
        Ok(json!({"id": target.0, "items": new.len()}))
    })
}

#[cfg(test)]
mod paste_into_tests {
    use super::*;

    #[test]
    fn paste_into_nests_and_clips() {
        let mut s = Session::new();
        s.execute("file.new", &json!({})).unwrap();
        let a = s.execute("frame.create", &json!({"rect": [100, 100, 300, 300]})).unwrap()["id"].as_u64().unwrap();
        s.execute("object.fill", &json!({"ids": [a], "swatch": "[Black]"})).unwrap();
        let frame = s.execute("frame.create", &json!({"rect": [150, 150, 250, 250]})).unwrap()["id"].as_u64().unwrap();
        s.execute("selection.set", &json!({"ids": [a]})).unwrap();
        s.execute("edit.cut", &json!({})).unwrap();
        s.execute("selection.set", &json!({"ids": [frame]})).unwrap();
        s.execute("edit.pasteInto", &json!({})).unwrap();
        let d = s.doc().unwrap().doc.clone();
        assert_eq!(d.spreads[0].items.len(), 1, "the black square is inside the frame now");
        let f = d.item(ItemId(frame)).unwrap();
        assert!(f.has_nested_items() && !f.is_group());
        assert_eq!(f.bounds(), designcraft_geom::Rect::new(150.0, 150.0, 250.0, 250.0), "bounds stay the frame's");
        // Rendered clipped: black inside the frame, nothing outside it.
        let cache = designcraft_compose::Cache::new();
        let mut r = designcraft_render::Renderer::new();
        let img = r.render_page(&d, &cache, 0, 1.0, false, &Default::default()).unwrap();
        assert!(img.pixel(200, 200)[0] < 90, "{:?}", img.pixel(200, 200));
        assert!(img.pixel(120, 120)[0] > 200, "{:?}", img.pixel(120, 120));
        s.doc().unwrap().doc.check().unwrap();
    }

    #[test]
    fn a_selected_graphic_copies_and_clears_on_its_own() {
        let dir = std::env::temp_dir().join(format!("dc-graphic-alone-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let png = dir.join("photo.png");
        std::fs::write(&png, designcraft_render::Rendered { width: 200, height: 100, pixels: vec![40u8; 200 * 100 * 4] }.to_png()).unwrap();
        let mut s = Session::new();
        s.execute("file.new", &json!({})).unwrap();
        // A square frame showing the middle of a wide photo (cropped left and right).
        let frame = s.execute("frame.create", &json!({"rect": [100, 100, 200, 200]})).unwrap()["id"].as_u64().unwrap();
        s.execute("file.place", &json!({"path": png.to_string_lossy(), "frame": frame})).unwrap();
        s.execute("object.fit", &json!({"mode": "fillProportionally", "ids": [frame]})).unwrap();
        s.execute("object.stroke", &json!({"ids": [frame], "swatch": "[Black]", "weight": 4})).unwrap();
        // Direct Selection on the photo, Copy: the whole photo, where it is, without the frame's stroke.
        s.execute("selection.set", &json!({"ids": [frame], "content": true})).unwrap();
        s.execute("edit.copy", &json!({})).unwrap();
        let clip = s.clipboard.clone().unwrap();
        let it = &clip.spreads[0].items[0];
        assert!(matches!(&it.content, designcraft_doc::Content::Graphic(g) if g.crop == [0.0; 4]));
        assert_eq!(it.bounds(), designcraft_geom::Rect::new(50.0, 100.0, 250.0, 200.0));
        assert!(it.stroke.weight == 0.0 || it.stroke.swatch == "[None]", "{:?}", it.stroke);
        // Paste Into a shape over it: the photo lands in place, inside the shape.
        let shape = s.execute("frame.create", &json!({"rect": [60, 90, 240, 210], "shape": "ellipse"})).unwrap()["id"].as_u64().unwrap();
        s.execute("selection.set", &json!({"ids": [shape]})).unwrap();
        s.execute("edit.pasteInto", &json!({})).unwrap();
        let d = s.doc().unwrap().doc.clone();
        let kid = d.item(ItemId(shape)).unwrap().children()[0].clone();
        let at = d.parent_xf(&d.find(kid.id).unwrap()) * kid.xf;
        assert!((at * designcraft_geom::Point::new(0.0, 0.0) - designcraft_geom::Point::new(50.0, 100.0)).hypot() < 1e-6);
        assert!((at * designcraft_geom::Point::new(200.0, 100.0) - designcraft_geom::Point::new(250.0, 200.0)).hypot() < 1e-6);
        // Delete with the photo selected: the frame stays, empty.
        s.execute("selection.set", &json!({"ids": [frame], "content": true})).unwrap();
        assert_eq!(s.execute("edit.clear", &json!({})).unwrap()["deleted"], 1);
        let d = s.doc().unwrap().doc.clone();
        assert!(matches!(d.item(ItemId(frame)).unwrap().content, designcraft_doc::Content::Unassigned));
        assert_eq!(s.doc().unwrap().selection, Selection::items(vec![ItemId(frame)]));
        d.check().unwrap();
        let _ = std::fs::remove_dir_all(&dir);
    }
}

#[cfg(test)]
mod paste_tests {
    use super::*;
    use designcraft_geom::Rect;

    fn pasted(s: &mut Session, id: &str, p: Value) -> (SpreadRef, Rect) {
        let new = ItemId(s.execute(id, &p).unwrap()["ids"][0].as_u64().unwrap());
        let d = &s.doc().unwrap().doc;
        (d.find(new).unwrap().spread, d.item(new).unwrap().bounds())
    }

    /// Paste and Paste in Place put the objects on the spread they are given (the first without one).
    #[test]
    fn paste_puts_the_objects_on_the_given_spread() {
        let mut s = Session::new();
        s.execute("file.new", &json!({"pages": 3, "facingPages": false})).unwrap();
        let a = s.execute("frame.create", &json!({"rect": [36, 36, 136, 136]})).unwrap()["id"].as_u64().unwrap();
        s.execute("selection.set", &json!({"ids": [a]})).unwrap();
        s.execute("edit.copy", &json!({})).unwrap();
        assert_eq!(pasted(&mut s, "edit.pasteInPlace", json!({"spread": 2})), (SpreadRef::Doc(2), Rect::new(36.0, 36.0, 136.0, 136.0)));
        assert_eq!(pasted(&mut s, "edit.paste", json!({"spread": 1})), (SpreadRef::Doc(1), Rect::new(48.0, 48.0, 148.0, 148.0)));
        assert_eq!(pasted(&mut s, "edit.pasteInPlace", json!({})).0, SpreadRef::Doc(0));
        // A spread the document doesn't have: an error, and nothing is pasted.
        let before = s.doc().unwrap().doc.clone();
        assert!(s.execute("edit.paste", &json!({"spread": 9})).is_err());
        assert_eq!(s.doc().unwrap().doc, before);
        before.check().unwrap();
    }

    /// With facing pages the first page sits alone, right of the spine. An object keeps its side
    /// of the spine when it is pasted in place on another spread.
    #[test]
    fn paste_in_place_keeps_the_side_of_the_spine() {
        let mut s = Session::new();
        s.execute("file.new", &json!({"pages": 4, "facingPages": true})).unwrap();
        let w = s.doc().unwrap().doc.settings.page_width;
        // On page 3, the right-hand page of the second spread.
        let a = s.execute("frame.create", &json!({"spread": 1, "rect": [w + 36.0, 36, w + 136.0, 136]})).unwrap()["id"].as_u64().unwrap();
        s.execute("selection.set", &json!({"ids": [a]})).unwrap();
        s.execute("edit.cut", &json!({})).unwrap();
        assert_eq!(pasted(&mut s, "edit.pasteInPlace", json!({"spread": 0})), (SpreadRef::Doc(0), Rect::new(36.0, 36.0, 136.0, 136.0)));
        // And back from page 1.
        s.execute("edit.cut", &json!({})).unwrap();
        assert_eq!(pasted(&mut s, "edit.pasteInPlace", json!({"spread": 1})), (SpreadRef::Doc(1), Rect::new(w + 36.0, 36.0, w + 136.0, 136.0)));
        // Page 4 sits alone left of the spine: the object stays right of it, off the page.
        assert_eq!(pasted(&mut s, "edit.pasteInPlace", json!({"spread": 2})), (SpreadRef::Doc(2), Rect::new(w + 36.0, 36.0, w + 136.0, 136.0)));
    }

    /// A single-sided page has no spine: an object keeps its distance from the page's left edge,
    /// whatever the page widths, and pastes as right-hand content into a facing-pages document.
    #[test]
    fn paste_in_place_from_a_single_sided_page_measures_from_its_left_edge() {
        let mut s = Session::new();
        s.execute("file.new", &json!({"pages": 2, "facingPages": false})).unwrap();
        s.execute("layout.pageSize", &json!({"pages": [2], "width": 400, "height": 792})).unwrap();
        let a = s.execute("frame.create", &json!({"spread": 1, "rect": [36, 36, 136, 136]})).unwrap()["id"].as_u64().unwrap();
        s.execute("selection.set", &json!({"ids": [a]})).unwrap();
        s.execute("edit.copy", &json!({})).unwrap();
        assert_eq!(pasted(&mut s, "edit.pasteInPlace", json!({"spread": 0})), (SpreadRef::Doc(0), Rect::new(36.0, 36.0, 136.0, 136.0)));
        s.execute("file.new", &json!({"pages": 3, "facingPages": true})).unwrap();
        let w = s.doc().unwrap().doc.settings.page_width;
        assert_eq!(pasted(&mut s, "edit.pasteInPlace", json!({})), (SpreadRef::Doc(0), Rect::new(36.0, 36.0, 136.0, 136.0)));
        assert_eq!(pasted(&mut s, "edit.pasteInPlace", json!({"spread": 1})), (SpreadRef::Doc(1), Rect::new(w + 36.0, 36.0, w + 136.0, 136.0)));
    }

    /// A copied text frame's clipping holds the files its anchored graphics show.
    #[test]
    fn copied_text_frames_keep_the_graphics_anchored_in_their_text() {
        let mut s = Session::new();
        s.execute("file.new", &json!({})).unwrap();
        let png = designcraft_render::Rendered { width: 4, height: 4, pixels: vec![200u8; 64] }.to_png();
        let placed = s.execute("file.place", &json!({"base64": super::super::file::base64_encode(&png), "name": "a.png", "x": 72, "y": 72})).unwrap();
        let (g, aid) = (ItemId(placed["id"].as_u64().unwrap()), designcraft_doc::AssetId(placed["asset"].as_u64().unwrap()));
        let f = ItemId(s.execute("frame.create", &json!({"rect": [36, 200, 300, 400], "content": "text"})).unwrap()["id"].as_u64().unwrap());
        s.edit(|d, _| {
            let it = d.item(g).cloned().ok_or(designcraft_doc::DocError::NoItem(g))?;
            let sid = d.item(f).and_then(|i| i.text_frame()).map(|t| t.story).ok_or(designcraft_doc::DocError::NoItem(f))?;
            super::super::anchored::anchor_items(d, sid, 0, vec![it], &Default::default())?;
            Ok(())
        })
        .unwrap();
        s.execute("selection.set", &json!({"ids": [f.0]})).unwrap();
        s.execute("edit.copy", &json!({})).unwrap();
        let clip = s.clipboard.clone().unwrap();
        assert_eq!(clip.assets.keys().copied().collect::<Vec<_>>(), [aid]);
    }

    /// A selection can name an object that is gone (here the copy that Undo removed). Copy
    /// measures from the spread of the first selected object that is still there.
    #[test]
    fn copy_measures_from_the_first_selected_object_that_exists() {
        let mut s = Session::new();
        s.execute("file.new", &json!({"pages": 3, "facingPages": true})).unwrap();
        let w = s.doc().unwrap().doc.settings.page_width;
        let a = s.execute("frame.create", &json!({"spread": 1, "rect": [w + 36.0, 36, w + 136.0, 136]})).unwrap()["id"].as_u64().unwrap();
        s.execute("selection.set", &json!({"ids": [a]})).unwrap();
        s.execute("edit.copy", &json!({})).unwrap();
        let gone = s.execute("edit.paste", &json!({"spread": 1})).unwrap()["ids"][0].as_u64().unwrap();
        s.execute("edit.undo", &json!({})).unwrap();
        assert_eq!(s.doc().unwrap().selection.items, [ItemId(a)], "Undo restores the selection from before the paste");
        s.doc_mut().unwrap().selection.items = vec![ItemId(gone), ItemId(a)];
        s.execute("edit.copy", &json!({})).unwrap();
        assert_eq!(pasted(&mut s, "edit.pasteInPlace", json!({"spread": 1})), (SpreadRef::Doc(1), Rect::new(w + 36.0, 36.0, w + 136.0, 136.0)));
    }
}

#[cfg(test)]
mod preview_tests {
    use super::*;

    fn text_session() -> Session {
        let mut s = Session::new();
        s.execute("file.new", &json!({})).unwrap();
        let r = s.execute("frame.create", &json!({"rect": [72, 72, 400, 300], "content": "text", "text": "Hello"})).unwrap();
        let sid = s.doc().unwrap().doc.item(ItemId(r["id"].as_u64().unwrap())).unwrap().text_frame().unwrap().story;
        s.execute("text.select", &json!({"story": sid.0, "anchor": 0, "focus": 5})).unwrap();
        s
    }

    fn size(s: &mut Session) -> f64 {
        s.execute("type.selectionAttrs", &json!({})).unwrap()["chars"]["size"].as_f64().unwrap()
    }

    fn preview_size(s: &mut Session, v: f64) {
        s.execute("edit.preview", &json!({"label": "Size", "command": "type.char", "params": {"attrs": {"size": v}}})).unwrap();
    }

    #[test]
    fn previews_show_on_the_page_and_commit_as_one_undo_step() {
        let mut s = text_session();
        let (before, undo) = (size(&mut s), s.doc().unwrap().history.undo.len());
        for v in [20.0, 30.0, 40.0] {
            preview_size(&mut s, v);
            assert_eq!(size(&mut s), v, "each preview replaces the last");
        }
        assert_eq!(s.doc().unwrap().history.undo.len(), undo, "no undo step while previewing");
        s.execute("edit.commitPreview", &json!({})).unwrap();
        assert_eq!(s.doc().unwrap().history.undo.len(), undo + 1);
        assert!(s.doc().unwrap().interaction.is_none());
        s.execute("edit.undo", &json!({})).unwrap();
        assert_eq!(size(&mut s), before);
    }

    /// A preview whose command fails closes again, so the next edit is its own undo step.
    #[test]
    fn a_failed_preview_leaves_no_preview_open() {
        let mut s = Session::new();
        s.execute("file.new", &json!({})).unwrap();
        let r = s.execute("edit.preview", &json!({"label": "Size", "command": "type.char", "params": {"attrs": {"size": 20}}}));
        assert!(r.is_err(), "nothing selected: type.char fails");
        assert!(s.doc().unwrap().interaction.is_none());
        let steps = s.doc().unwrap().history.undo.len();
        s.execute("frame.create", &json!({"rect": [72, 72, 144, 144]})).unwrap();
        assert_eq!(s.doc().unwrap().history.undo.len(), steps + 1);
    }

    #[test]
    fn a_cancelled_preview_leaves_the_document_as_it_was() {
        let mut s = text_session();
        let (before, undo) = (size(&mut s), s.doc().unwrap().history.undo.len());
        preview_size(&mut s, 50.0);
        s.execute("edit.cancelPreview", &json!({})).unwrap();
        assert_eq!(size(&mut s), before);
        assert_eq!(s.doc().unwrap().history.undo.len(), undo);
        // Committing with nothing open does nothing.
        s.execute("edit.commitPreview", &json!({})).unwrap();
        assert_eq!(s.doc().unwrap().history.undo.len(), undo);
    }

    #[test]
    fn only_edits_are_previewed_and_another_interaction_is_left_alone() {
        let mut s = text_session();
        for command in ["edit.preview", "edit.undo", "type.selectionAttrs", "no.such.command"] {
            assert!(s.execute("edit.preview", &json!({"label": "X", "command": command})).is_err(), "{command}");
        }
        assert!(s.execute("edit.preview", &json!({"command": "type.char"})).is_err(), "needs a label");
        s.begin_interaction("Move").unwrap();
        assert!(s.execute("edit.preview", &json!({"label": "Size", "command": "type.char", "params": {"attrs": {"size": 20}}})).is_err());
        assert_eq!(s.doc().unwrap().interaction.as_ref().unwrap().label, "Move");
    }
}
