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

fn has_clip_or_text(s: &Session) -> std::result::Result<(), String> {
    if s.active().is_some_and(|d| d.selection.text.is_some()) {
        return Ok(());
    }
    has_clip(s)
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
    // Footnotes, cross-references and tables only paste into story text, not cells or footnotes.
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
    s.execute("text.insert", &json!({"text": text.replace("\r\n", "\n").replace('\r', "\n"), "raw": true}))
}

fn strip_objects(st: &designcraft_doc::Story) -> designcraft_doc::Story {
    let mut out = st.clone();
    let marks = [designcraft_doc::FOOTNOTE_REF, designcraft_doc::XREF_MARK, designcraft_doc::TABLE_ANCHOR];
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
            st.revision += 1;
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
            st.revision += 1;
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
            st.revision += 1;
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
            st.revision += 1;
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
            st.revision += 1;
            ok()
        }),
        cmd!(noundo "edit.deselectAll", "Deselect All", ["Edit"], Some("Cmd+Shift+A"), "{}", has_doc, |s, _| {
            let st = s.doc_mut()?;
            st.selection = Selection::default();
            st.revision += 1;
            ok()
        }),
        cmd!("edit.clear", "Clear", ["Edit"], Some("Delete"), "{ids?} — delete selected items (or the selected text)", has_doc, |s, p| {
            if s.doc()?.selection.text.is_some() && p.get("ids").is_none() {
                return super::text::delete_selection(s);
            }
            let ids = super::targets(s, p)?;
            s.edit(|d, sel| {
                for id in &ids {
                    let _ = d.remove_item(*id);
                }
                *sel = Selection::default();
                Ok(json!({"deleted": ids.len()}))
            })
        }),
        cmd!(noundo "edit.copy", "Copy", ["Edit"], Some("Cmd+C"), "{} → {text} when text is selected (with formatting, footnotes, markers)", has_selection_or_text, |s, _| {
            if let Some(text) = copy_text(s)? {
                return Ok(json!({"text": text}));
            }
            s.clipboard = Some(Arc::new(clip_doc(s)?));
            s.text_clipboard = None;
            ok()
        }),
        cmd!("edit.cut", "Cut", ["Edit"], Some("Cmd+X"), "{}", has_selection_or_text, |s, p| {
            if let Some(text) = copy_text(s)? {
                super::text::delete_selection(s)?;
                return Ok(json!({"text": text}));
            }
            s.clipboard = Some(Arc::new(clip_doc(s)?));
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
            "{inPlace?: bool, text?: the system clipboard's text (pastes it unless it is what was copied here)}",
            has_clip_or_text,
            |s, p| {
                if s.doc()?.selection.text.is_some() {
                    return paste_text(s, super::str_param(p, "text").map(str::to_string), true);
                }
                let Some(clip) = s.clipboard.clone() else {
                    return Err(super::bad("edit.paste", "clipboard is empty"));
                };
                let off = if bool_or(p, "inPlace", false) { 0.0 } else { 12.0 };
                let ids: Vec<ItemId> = clip.spreads.first().map(|sp| sp.items.iter().map(|i| i.id).collect()).unwrap_or_default();
                s.edit(|d, sel| {
                    let new = super::object::duplicate_from(d, &clip, &ids, SpreadRef::Doc(0), designcraft_geom::Vec2::new(off, off))?;
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
        cmd!("edit.pasteInPlace", "Paste in Place", ["Edit"], Some("Cmd+Alt+Shift+V"), "{}", has_clip, |s, _| s
            .execute("edit.paste", &json!({"inPlace": true}))),
        cmd!("edit.duplicate", "Duplicate", ["Edit"], Some("Cmd+Alt+Shift+D"), "{}", has_selection, |s, _| s
            .execute("transform.move", &json!({"dx": 12.0, "dy": 12.0, "copy": true}))),
    ]
}

/// A document holding copies of the selected items (and their stories) on spread 0.
pub(crate) fn clip_doc(s: &Session) -> Result<designcraft_doc::Document> {
    let st = s.doc()?;
    let mut d = (*st.doc).clone();
    let keep: Vec<ItemId> = st.selection.items.clone();
    // Gather the selected items (keep their spread coordinates) into spread 0.
    let mut items = Vec::new();
    for id in &keep {
        if let Some(it) = st.doc.item(*id) {
            items.push(Arc::new(it.clone()));
        }
    }
    for sp in d.spreads.iter_mut() {
        Arc::make_mut(sp).items.clear();
    }
    // Kept frames and graphics (including inside groups).
    let mut frames = std::collections::HashSet::new();
    let mut assets = std::collections::HashSet::new();
    for it in &items {
        it.walk(&mut |i| match &i.content {
            designcraft_doc::Content::Text(_) => {
                frames.insert(i.id);
            }
            designcraft_doc::Content::Graphic(g) => {
                assets.insert(g.asset);
            }
            _ => {}
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
    d.assets.retain(|k, _| assets.contains(k));
    d.hyperlinks.clear();
    d.bookmarks.clear();
    Ok(d)
}

#[cfg(test)]
mod text_clipboard_tests {
    use super::*;

    fn frame(s: &mut Session, text: &str) -> designcraft_doc::StoryId {
        let r = s.execute("frame.create", &json!({"rect": [72, 72, 400, 300], "content": "text", "text": text})).unwrap();
        s.doc().unwrap().doc.item(ItemId(r["id"].as_u64().unwrap())).unwrap().text_frame().unwrap().story
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
}
