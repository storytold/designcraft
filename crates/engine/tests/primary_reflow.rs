use std::sync::Arc;

use designcraft_doc::{Document, ItemId, StoryId};
use designcraft_engine::{Session, compose};
use serde_json::json;

#[derive(Clone, Copy, Debug)]
enum Removal {
    Frame,
    Page,
    Cut,
}

fn primary_chain() -> (Session, StoryId, ItemId) {
    let mut s = Session::new();
    s.execute("file.new", &json!({"pages": 1, "primaryTextFrame": true})).unwrap();
    let sid = s.doc().unwrap().doc.settings.primary_story.unwrap();
    s.execute("text.select", &json!({"story": sid.0, "anchor": 0, "focus": 0})).unwrap();
    s.execute("text.insert", &json!({"text": "Words keep coming and the page fills up quickly with them. ".repeat(250)})).unwrap();
    // Autoflow estimates a batch of pages; it need not fill the last one. Remove only proven
    // empty tail pages before choosing the occupied frame whose removal must leave overset text.
    let initial = s.doc().unwrap().doc.clone();
    let initial_layout = compose::compose_story(&initial, sid, &Default::default());
    eprintln!("primary fixture initial ranges: {:?}", initial_layout.frames.iter().map(|f| (f.frame, f.range.clone())).collect::<Vec<_>>());
    for _ in 0..initial.story(sid).unwrap().frames.len() {
        let d = &s.doc().unwrap().doc;
        let last = *d.story(sid).unwrap().frames.last().unwrap();
        let cs = compose::compose_story(d, sid, &Default::default());
        assert!(!cs.is_overset());
        let frame = cs.frame(last).unwrap();
        if !frame.range.is_empty() {
            break;
        }
        assert!(frame.lines.iter().all(|line| line.range.is_empty()), "do not trim a frame with text");
        let page = d.page_of_item(last).unwrap();
        assert_eq!(page + 1, d.page_count());
        assert!(page > 0);
        let (si, _) = d.page_loc(page).unwrap();
        assert!(d.spreads[si].items.iter().filter(|it| d.page_of_item(it.id) == Some(page)).all(|it| it.id == last));
        s.execute("layout.pages.delete", &json!({"pages": [page]})).unwrap();
        assert_eq!(s.doc().unwrap().doc.story(sid).unwrap().text, initial.story(sid).unwrap().text);
    }
    let d = &s.doc().unwrap().doc;
    let last = *d.story(sid).unwrap().frames.last().unwrap();
    assert!(d.page_count() >= 3);
    let cs = compose::compose_story(d, sid, &Default::default());
    eprintln!("primary fixture final ranges: {:?}", cs.frames.iter().map(|f| (f.frame, f.range.clone())).collect::<Vec<_>>());
    assert!(!cs.is_overset());
    assert!(!cs.frame(last).unwrap().range.is_empty(), "the removal witness contains text");
    let mut probe = (**d).clone();
    probe.remove_item(last).unwrap();
    assert!(compose::compose_story(&probe, sid, &Default::default()).is_overset(), "the removed frame holds text");
    (s, sid, last)
}

fn remove(s: &mut Session, frame: ItemId, kind: Removal) {
    match kind {
        Removal::Frame => s.execute("edit.clear", &json!({"ids": [frame.0]})).unwrap(),
        Removal::Page => {
            let page = s.doc().unwrap().doc.page_of_item(frame).unwrap();
            s.execute("layout.pages.delete", &json!({"pages": [page]})).unwrap()
        }
        Removal::Cut => {
            s.execute("selection.set", &json!({"ids": [frame.0]})).unwrap();
            s.execute("edit.cut", &json!({})).unwrap()
        }
    };
}

fn assert_removed(s: &Session, before: &Document, sid: StoryId, frame: ItemId, pages: usize) {
    let d = &s.doc().unwrap().doc;
    assert!(d.item(frame).is_none());
    assert_eq!(d.page_count(), pages);
    assert_eq!(d.settings.primary_story, Some(sid));
    let story = d.story(sid).unwrap();
    let old = before.story(sid).unwrap();
    assert_eq!(story.text, old.text);
    assert_eq!(story.frames, old.frames.iter().copied().filter(|f| *f != frame).collect::<Vec<_>>());
    assert!(compose::compose_story(d, sid, &Default::default()).is_overset());
    d.check().unwrap();
}

#[test]
fn explicit_primary_removals_survive_unrelated_edits_with_a_cold_cache() {
    for kind in [Removal::Frame, Removal::Page, Removal::Cut] {
        let (mut s, sid, last) = primary_chain();
        let before = s.doc().unwrap().doc.clone();
        let pages = before.page_count() - usize::from(matches!(kind, Removal::Page));
        remove(&mut s, last, kind);
        assert_removed(&s, &before, sid, last, pages);
        let removed = s.doc().unwrap().doc.clone();
        s.execute("edit.undo", &json!({})).unwrap();
        assert!(Arc::ptr_eq(&s.doc().unwrap().doc, &before));
        s.execute("edit.redo", &json!({})).unwrap();
        assert!(Arc::ptr_eq(&s.doc().unwrap().doc, &removed));

        s.cache.clear();
        assert!(s.cache.composed_for(&removed, sid).is_none());
        s.execute("frame.create", &json!({"rect": [10, 10, 20, 20], "content": "unassigned"})).unwrap();
        assert_removed(&s, &before, sid, last, pages);
        s.execute("frame.create", &json!({"rect": [10, 30, 100, 60], "content": "text", "text": "Other story", "caret": false})).unwrap();
        assert_removed(&s, &before, sid, last, pages);
        assert!(s.cache.composed_for(&s.doc().unwrap().doc, sid).is_none(), "unrelated commands do not compose the primary story");
        assert!(s.prefs.smart_text_reflow);
    }
}

#[test]
fn primary_content_and_frame_changes_resume_reflow_in_one_undo_step() {
    for frame_change in [false, true] {
        let (mut s, sid, last) = primary_chain();
        remove(&mut s, last, Removal::Page);
        let removed = s.doc().unwrap().doc.clone();
        let undo = s.doc().unwrap().history.undo.len();
        if frame_change {
            let first = removed.story(sid).unwrap().frames[0];
            let inset = removed.item(first).unwrap().text_frame().unwrap().options.inset.map(|v| v + 1.0);
            s.execute("object.textFrameOptions", &json!({"ids": [first.0], "inset": inset})).unwrap();
        } else {
            let end = removed.story(sid).unwrap().len();
            s.execute("text.select", &json!({"story": sid.0, "anchor": end, "focus": end})).unwrap();
            s.execute("text.insert", &json!({"text": " More text."})).unwrap();
        }
        let flowed = s.doc().unwrap().doc.clone();
        assert!(flowed.page_count() > removed.page_count());
        assert!(!compose::compose_story(&flowed, sid, &Default::default()).is_overset());
        assert!(flowed.item(last).is_none());
        assert_eq!(flowed.settings.primary_story, Some(sid));
        let expected = removed.story(sid).unwrap().text.clone() + if frame_change { "" } else { " More text." };
        assert_eq!(flowed.story(sid).unwrap().text, expected);
        assert_eq!(s.doc().unwrap().history.undo.len(), undo + 1);
        flowed.check().unwrap();
        s.execute("edit.undo", &json!({})).unwrap();
        assert!(Arc::ptr_eq(&s.doc().unwrap().doc, &removed));
        s.execute("edit.redo", &json!({})).unwrap();
        assert!(Arc::ptr_eq(&s.doc().unwrap().doc, &flowed));
    }
}

#[test]
fn deleting_primary_text_still_prunes_empty_pages() {
    for command in ["text.delete", "edit.clear", "edit.cut"] {
        let (mut s, sid, _) = primary_chain();
        let before = s.doc().unwrap().doc.clone();
        let end = before.story(sid).unwrap().len();
        s.execute("text.select", &json!({"story": sid.0, "anchor": 0, "focus": end})).unwrap();
        s.execute(command, &json!({})).unwrap();
        let empty = s.doc().unwrap().doc.clone();
        assert_eq!(empty.page_count(), 1, "{command}");
        assert_eq!(empty.settings.primary_story, Some(sid));
        assert_eq!(empty.story(sid).unwrap().frames.len(), 1);
        assert!(empty.story(sid).unwrap().text.is_empty());
        empty.check().unwrap();
        s.execute("edit.undo", &json!({})).unwrap();
        assert!(Arc::ptr_eq(&s.doc().unwrap().doc, &before));
        s.execute("edit.redo", &json!({})).unwrap();
        assert!(Arc::ptr_eq(&s.doc().unwrap().doc, &empty));
    }
}

#[test]
fn unchanged_wrapping_flex_does_not_resume_deliberately_removed_primary_pages() {
    use designcraft_doc::{Content, WrapMode, flex::FlexSize};

    let (mut s, sid, last) = primary_chain();
    let id = s.execute("frame.create", &json!({"spread": 0, "rect": [10, 10, 20, 20], "content": "unassigned"})).unwrap()["id"].as_u64().unwrap();
    s.execute("flex.convert", &json!({"ids": [id]})).unwrap();
    s.execute("object.textWrap", &json!({"ids": [id], "mode": "boundingBox", "offset": 0})).unwrap();
    let before = s.doc().unwrap().doc.clone();
    let it = before.item(ItemId(id)).unwrap();
    assert!(matches!(&it.content, Content::Group { items } if items.is_empty()));
    assert_eq!(it.flex.as_ref().unwrap().width, FlexSize::Fixed);
    assert_eq!(it.flex.as_ref().unwrap().height, FlexSize::Fixed);
    assert_eq!(it.wrap.mode, WrapMode::BoundingBox);
    assert_eq!(it.bounds(), designcraft_geom::Rect::new(10.0, 10.0, 20.0, 20.0));
    assert!(!compose::compose_story(&before, sid, &Default::default()).is_overset());
    let pages = before.page_count() - 1;
    remove(&mut s, last, Removal::Page);
    assert_removed(&s, &before, sid, last, pages);
    let removed = s.doc().unwrap().doc.clone();
    let wrapping = removed.spreads[0].items.iter().find(|it| it.id == ItemId(id)).unwrap().clone();
    s.execute("edit.undo", &json!({})).unwrap();
    assert!(Arc::ptr_eq(&s.doc().unwrap().doc, &before));
    s.execute("edit.redo", &json!({})).unwrap();
    assert!(Arc::ptr_eq(&s.doc().unwrap().doc, &removed));

    s.cache.clear();
    assert!(s.cache.composed_for(&removed, sid).is_none());
    let undo = s.doc().unwrap().history.undo.len();
    s.execute("frame.create", &json!({"spread": 0, "rect": [10, 30, 20, 40], "content": "unassigned"})).unwrap();
    assert_removed(&s, &before, sid, last, pages);
    let edited = s.doc().unwrap().doc.clone();
    let current = edited.spreads[0].items.iter().find(|it| it.id == ItemId(id)).unwrap();
    assert_eq!(serde_json::to_value(&*wrapping).unwrap(), serde_json::to_value(&**current).unwrap());
    assert!(Arc::ptr_eq(&wrapping, current), "settled wrapping Flex retains its composition identity");
    assert!(s.cache.composed_for(&edited, sid).is_none(), "the unrelated edit must not compose the primary story");
    assert_eq!(s.doc().unwrap().history.undo.len(), undo + 1);
    s.execute("edit.undo", &json!({})).unwrap();
    assert!(Arc::ptr_eq(&s.doc().unwrap().doc, &removed));
    s.execute("edit.redo", &json!({})).unwrap();
    assert!(Arc::ptr_eq(&s.doc().unwrap().doc, &edited));

    // A genuine primary geometry change still invalidates composition and resumes reflow.
    s.cache.clear();
    let first = edited.story(sid).unwrap().frames[0];
    let inset = edited.item(first).unwrap().text_frame().unwrap().options.inset.map(|v| v + 1.0);
    let undo = s.doc().unwrap().history.undo.len();
    s.execute("object.textFrameOptions", &json!({"ids": [first.0], "inset": inset})).unwrap();
    let flowed = s.doc().unwrap().doc.clone();
    assert!(flowed.page_count() > edited.page_count());
    assert!(!compose::compose_story(&flowed, sid, &Default::default()).is_overset());
    assert!(flowed.item(last).is_none());
    assert_eq!(flowed.settings.primary_story, Some(sid));
    assert_eq!(flowed.story(sid).unwrap().text, before.story(sid).unwrap().text);
    assert_eq!(s.doc().unwrap().history.undo.len(), undo + 1);
    assert!(s.prefs.smart_text_reflow);
    flowed.check().unwrap();
    s.execute("edit.undo", &json!({})).unwrap();
    assert!(Arc::ptr_eq(&s.doc().unwrap().doc, &edited));
    s.execute("edit.redo", &json!({})).unwrap();
    assert!(Arc::ptr_eq(&s.doc().unwrap().doc, &flowed));
}
