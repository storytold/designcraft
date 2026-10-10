//! Exercise retained layouts through the same commands as the editor. Fresh composition and
//! a new renderer/cache are independent oracles for history states beyond the two cache slots.

use std::sync::Arc;

use designcraft_compose::{Cache, ComposeOptions, ComposedStory, compose_story};
use designcraft_doc::{Document, StoryId};
use designcraft_render::{RenderOptions, Rendered, Renderer};
use serde_json::{Value, json};

use crate::Session;

fn session_with_text(text: &str) -> (Session, StoryId) {
    let mut s = Session::new();
    s.execute("file.new", &json!({"pages": 1, "width": 288, "height": 216, "facingPages": false, "margins": 18})).unwrap();
    let frame = s.execute("frame.create", &json!({"rect": [18, 18, 270, 198], "content": "text", "text": text})).unwrap();
    (s, StoryId(frame["story"].as_u64().unwrap()))
}

fn select(s: &mut Session, sid: StoryId, all: bool) {
    let end = s.doc().unwrap().doc.story(sid).unwrap().text.len();
    s.execute("text.select", &json!({"story": sid.0, "anchor": if all { 0 } else { end }, "focus": end})).unwrap();
}

fn layout(s: &Session, sid: StoryId) -> Arc<ComposedStory> {
    s.cache.get(&s.doc().unwrap().doc, sid, None)
}

fn assert_fresh_layout(doc: &Document, sid: StoryId, got: &ComposedStory, state: &str) {
    let fresh = compose_story(doc, sid, &ComposeOptions::default());
    // Debug includes every layout field: ranges, overflow, geometry, run styles, decorations,
    // columns and vertical flags. Face Debug names alone do not establish font identity.
    assert_eq!(format!("{got:?}"), format!("{fresh:?}"), "layout at {state}");
    let faces = |cs: &ComposedStory| cs.frames.iter().flat_map(|f| &f.lines).flat_map(|l| &l.glyphs).map(|g| g.face.id()).collect::<Vec<_>>();
    assert_eq!(faces(got), faces(&fresh), "glyph face identities at {state}");
}

fn renderer() -> Renderer {
    let mut r = Renderer::new();
    r.threads = 0;
    r
}

fn assert_fresh_state(s: &Session, sid: StoryId, render: &mut Renderer, state: &str) -> Rendered {
    let doc = &s.doc().unwrap().doc;
    doc.check().unwrap();
    assert_fresh_layout(doc, sid, &layout(s, sid), state);
    let got = render.render_page(doc, &s.cache, 0, 1.0, false, &RenderOptions::default()).unwrap();
    let fresh = renderer().render_page(doc, &Cache::new(), 0, 1.0, false, &RenderOptions::default()).unwrap();
    assert_eq!((got.width, got.height), (fresh.width, fresh.height), "raster size at {state}");
    // Same process, fonts, backend, scale and options: no cross-platform golden or tolerance.
    assert!(got.pixels == fresh.pixels, "cached pixels differ from fresh pixels at {state}");
    got
}

fn assert_command_reuses_previous(command: &str, params: &Value, select_all: bool) {
    let db = designcraft_fonts::FontDb::global();
    // Other engine tests publish fonts in the global database. Retry only a genuinely changed
    // epoch, and fail if no stable attempt can establish the identity contract.
    for _ in 0..32 {
        let (mut s, sid) = session_with_text(&"Ordinary body text with ligatures, café and reflow. ".repeat(6));
        select(&mut s, sid, select_all);
        let epoch = db.composition_epoch().unwrap();
        let before_doc = s.doc().unwrap().doc.clone();
        let before = layout(&s, sid);
        s.execute(command, params).unwrap();
        let after_doc = s.doc().unwrap().doc.clone();
        let after = layout(&s, sid);
        assert!(!Arc::ptr_eq(&before, &after), "{command} must change the composition");
        assert_ne!(format!("{before:?}"), format!("{after:?}"), "{command} must change actual layout metadata");
        assert_fresh_layout(&after_doc, sid, &after, command);
        let mut round_trips = Vec::new();
        for _ in 0..3 {
            s.execute("edit.undo", &json!({})).unwrap();
            assert!(Arc::ptr_eq(&s.doc().unwrap().doc, &before_doc));
            let undone = layout(&s, sid);
            assert_fresh_layout(&before_doc, sid, &undone, "immediate undo");
            s.execute("edit.redo", &json!({})).unwrap();
            assert!(Arc::ptr_eq(&s.doc().unwrap().doc, &after_doc));
            let redone = layout(&s, sid);
            assert_fresh_layout(&after_doc, sid, &redone, "immediate redo");
            round_trips.push((undone, redone));
        }
        if db.composition_epoch() != Some(epoch) {
            continue;
        }
        for (undone, redone) in round_trips {
            assert!(Arc::ptr_eq(&before, &undone), "{command}: undo must promote the retained allocation");
            assert!(Arc::ptr_eq(&after, &redone), "{command}: redo must promote the retained allocation");
        }
        return;
    }
    panic!("{command}: no stable font epoch in 32 complete undo/redo attempts");
}

#[test]
fn insert_command_undo_redo_reuses_previous_layout() {
    assert_command_reuses_previous("text.insert", &json!({"text": " Extra é text."}), false);
}

#[test]
fn font_command_undo_redo_reuses_previous_layout() {
    assert_command_reuses_previous("type.char", &json!({"attrs": {"fontFamily": "Source Sans 3", "underline": true}}), true);
}

#[test]
fn deep_command_history_and_branch_match_fresh_layout_and_pixels() {
    let (mut s, sid) = session_with_text("History starts with café, office and several short lines.\n");
    let mut render = renderer();
    let mut docs = vec![s.doc().unwrap().doc.clone()];
    let mut images = vec![assert_fresh_state(&s, sid, &mut render, "initial")];
    for step in 0..18 {
        match step % 3 {
            0 => {
                select(&mut s, sid, false);
                s.execute("text.insert", &json!({"text": format!("Edit {step}: naïve words fill another line.\n")})).unwrap();
            }
            1 => {
                select(&mut s, sid, true);
                let family = if step % 2 == 0 { "Source Serif 4" } else { "Source Sans 3" };
                s.execute("type.char", &json!({"attrs": {"fontFamily": family, "size": 11 + step, "underline": step % 2 == 0}})).unwrap();
            }
            _ => {
                select(&mut s, sid, true);
                s.execute("type.para", &json!({"attrs": {"align": if step % 2 == 0 { "center" } else { "right" }}})).unwrap();
            }
        }
        docs.push(s.doc().unwrap().doc.clone());
        images.push(assert_fresh_state(&s, sid, &mut render, &format!("forward {step}")));
    }
    assert!(layout(&s, sid).is_overset(), "the history must exercise overflow metadata too");
    assert!(images[0].pixels != images.last().unwrap().pixels, "the command sequence must visibly change the page");
    for index in (0..18).rev() {
        s.execute("edit.undo", &json!({})).unwrap();
        assert!(Arc::ptr_eq(&s.doc().unwrap().doc, &docs[index]), "undo restores document {index}");
        let got = assert_fresh_state(&s, sid, &mut render, &format!("deep undo {index}"));
        assert!(got.pixels == images[index].pixels, "undo restores original pixels for state {index}");
    }
    for index in 1..=18 {
        s.execute("edit.redo", &json!({})).unwrap();
        assert!(Arc::ptr_eq(&s.doc().unwrap().doc, &docs[index]), "redo restores document {index}");
        let got = assert_fresh_state(&s, sid, &mut render, &format!("deep redo {index}"));
        assert!(got.pixels == images[index].pixels, "redo restores original pixels for state {index}");
    }
    for index in (11..18).rev() {
        s.execute("edit.undo", &json!({})).unwrap();
        assert!(Arc::ptr_eq(&s.doc().unwrap().doc, &docs[index]));
        assert_fresh_state(&s, sid, &mut render, &format!("undo before branch {index}"));
    }
    let branch_base = s.doc().unwrap().doc.clone();
    s.execute("text.select", &json!({"story": sid.0, "anchor": 0, "focus": 0})).unwrap();
    s.execute("text.insert", &json!({"text": "A new branch.\n"})).unwrap();
    assert!(s.doc().unwrap().history.redo.is_empty(), "editing after undo discards the old redo branch");
    assert!(s.execute("edit.redo", &json!({})).is_err(), "the abandoned branch cannot be redone");
    let branch_doc = s.doc().unwrap().doc.clone();
    let branch = assert_fresh_state(&s, sid, &mut render, "new branch");
    assert!(branch.pixels != images[11].pixels, "the new branch must visibly differ from its base");
    s.execute("edit.undo", &json!({})).unwrap();
    assert!(Arc::ptr_eq(&s.doc().unwrap().doc, &branch_base));
    let undone = assert_fresh_state(&s, sid, &mut render, "branch undo");
    assert!(undone.pixels == images[11].pixels);
    s.execute("edit.redo", &json!({})).unwrap();
    assert!(Arc::ptr_eq(&s.doc().unwrap().doc, &branch_doc));
    let redone = assert_fresh_state(&s, sid, &mut render, "branch redo");
    assert!(redone.pixels == branch.pixels);
}

#[test]
fn glyph_fallback_history_matches_fresh_layout_and_distinct_pixels() {
    const FAMILY: &str = "DC Previous Layout Limited Primary";
    designcraft_fonts::FontDb::global().add_font(designcraft_fonts::testing::font_with(FAMILY, &['a']).unwrap());
    let (mut s, sid) = session_with_text("azaz");
    select(&mut s, sid, true);
    s.execute("type.char", &json!({"attrs": {"fontFamily": FAMILY, "size": 28}})).unwrap();
    let mut render = renderer();
    let off = layout(&s, sid);
    assert_eq!(off.frames[0].lines[0].glyphs.iter().find(|g| g.byte == 1).unwrap().gid, 0);
    let off_image = assert_fresh_state(&s, sid, &mut render, "fallback disabled");
    select(&mut s, sid, false);
    s.execute("text.insert", &json!({"text": "a"})).unwrap();
    assert_fresh_state(&s, sid, &mut render, "fallback disabled edit");
    s.execute("edit.undo", &json!({})).unwrap();
    // Do not look up the restored story yet: the next request sees its old layout in previous.
    s.execute("document.preferences", &json!({"glyphFallback": true})).unwrap();
    let on = layout(&s, sid);
    assert!(!Arc::ptr_eq(&off, &on), "changing glyph fallback must veto the old previous entry");
    let z = on.frames[0].lines[0].glyphs.iter().find(|g| g.byte == 1).unwrap();
    assert_ne!(z.gid, 0, "the bundled fallback covers z without system fonts");
    assert_ne!(z.face.family, FAMILY);
    let on_image = assert_fresh_state(&s, sid, &mut render, "fallback enabled");
    assert!(off_image.pixels != on_image.pixels, "fallback must be a visible control");
    for _ in 0..3 {
        s.execute("edit.undo", &json!({})).unwrap();
        let got = assert_fresh_state(&s, sid, &mut render, "fallback undo");
        assert!(got.pixels == off_image.pixels);
        s.execute("edit.redo", &json!({})).unwrap();
        let got = assert_fresh_state(&s, sid, &mut render, "fallback redo");
        assert!(got.pixels == on_image.pixels);
    }
}

#[test]
fn vertical_command_history_matches_fresh_layout_and_pixels() {
    use designcraft_fonts::testing::{font_mapping, with_vmtx};

    const FAMILY: &str = "DC Previous Layout Vertical";
    let font = font_mapping(FAMILY, &[('日', 'X'), ('本', 'O'), ('A', 'A'), ('b', 'b'), ('1', '1'), ('2', '2')]).unwrap();
    designcraft_fonts::FontDb::global().add_font(with_vmtx(&font, (1000, 120), &[]).unwrap());
    let (mut s, sid) = session_with_text("日本Ab12日本Ab12");
    select(&mut s, sid, true);
    s.execute("type.char", &json!({"attrs": {"fontFamily": FAMILY, "size": 24}})).unwrap();
    let mut render = renderer();
    let horizontal = assert_fresh_state(&s, sid, &mut render, "horizontal control");
    s.execute("type.storyDirection", &json!({"vertical": true})).unwrap();
    let vertical = layout(&s, sid);
    assert!(vertical.frames.iter().all(|f| f.vertical));
    assert!(vertical.frames[0].lines[0].glyphs.iter().any(|g| g.upright));
    let vertical_image = assert_fresh_state(&s, sid, &mut render, "vertical control");
    assert!(horizontal.pixels != vertical_image.pixels);
    select(&mut s, sid, false);
    s.execute("text.insert", &json!({"text": "日本"})).unwrap();
    let edited = assert_fresh_state(&s, sid, &mut render, "vertical insertion");
    assert!(edited.pixels != vertical_image.pixels);
    for _ in 0..3 {
        s.execute("edit.undo", &json!({})).unwrap();
        let got = assert_fresh_state(&s, sid, &mut render, "vertical insertion undo");
        assert!(got.pixels == vertical_image.pixels);
        s.execute("edit.redo", &json!({})).unwrap();
        let got = assert_fresh_state(&s, sid, &mut render, "vertical insertion redo");
        assert!(got.pixels == edited.pixels);
    }
}
