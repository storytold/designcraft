//! Combined Session-cache retirement and glyph-identity ownership regressions.

use std::sync::Arc;

use designcraft_compose::Cache;
use designcraft_doc::{Document, StoryId};
use designcraft_engine::Session;
use designcraft_render::Renderer;
use serde_json::json;

fn serial_renderer() -> Renderer {
    let mut renderer = Renderer::new();
    renderer.threads = 0;
    renderer
}

fn text_document(s: &mut Session, text: &str) -> StoryId {
    s.execute("file.new", &json!({})).unwrap();
    let value = s.execute("frame.create", &json!({"rect":[36,36,500,700],"content":"text","text":text})).unwrap();
    StoryId(value["story"].as_u64().unwrap())
}

fn checked_pixels(doc: &Document, cache: &Cache, renderer: &mut Renderer) -> Vec<u8> {
    let warm = renderer.render_page(doc, cache, 0, 1.0, false, &Default::default()).unwrap().pixels;
    let fresh = serial_renderer().render_page(doc, &Cache::new(), 0, 1.0, false, &Default::default()).unwrap().pixels;
    assert_eq!(warm, fresh);
    warm
}

#[test]
fn edited_history_closes_without_retaining_source_or_layout_but_keeps_glyph_paths() {
    let mut s = Session::new();
    let sid = text_document(&mut s, "Original visible text.");
    let mut renderer = serial_renderer();
    let before = s.doc().unwrap().doc.clone();
    let source_before = Arc::downgrade(&before.stories[&sid]);
    let layout_before = Arc::downgrade(&s.cache.get(&before, sid, None));
    let before_pixels = checked_pixels(&before, &s.cache, &mut renderer);
    s.execute("text.select", &json!({"story":sid.0,"anchor":0,"focus":0})).unwrap();
    s.execute("text.insert", &json!({"text":"é changed ","raw":true})).unwrap();
    let edited = s.doc().unwrap().doc.clone();
    assert_eq!(edited.story(sid).unwrap().text, "é changed Original visible text.");
    let source_edited = Arc::downgrade(&edited.stories[&sid]);
    let layout_edited = Arc::downgrade(&s.cache.get(&edited, sid, None));
    let edited_pixels = checked_pixels(&edited, &s.cache, &mut renderer);
    assert_ne!(before_pixels, edited_pixels);
    s.execute("edit.undo", &json!({})).unwrap();
    assert_eq!(s.doc().unwrap().doc, before);
    assert_eq!(checked_pixels(&s.doc().unwrap().doc, &s.cache, &mut renderer), before_pixels);
    let layout_undo = Arc::downgrade(&s.cache.get(&s.doc().unwrap().doc, sid, None));
    s.execute("edit.redo", &json!({})).unwrap();
    assert_eq!(s.doc().unwrap().doc, edited);
    assert_eq!(checked_pixels(&s.doc().unwrap().doc, &s.cache, &mut renderer), edited_pixels);
    let layout_redo = Arc::downgrade(&s.cache.get(&s.doc().unwrap().doc, sid, None));
    let cached_frames = renderer.cached_frames();
    assert!(cached_frames > 0);
    drop(before);
    drop(edited);
    s.execute("file.close", &json!({})).unwrap();
    assert!(s.documents().is_empty());
    assert!(source_before.upgrade().is_none());
    assert!(source_edited.upgrade().is_none());
    for layout in [layout_before, layout_edited, layout_undo, layout_redo] {
        assert!(layout.upgrade().is_none(), "neither Session nor glyph identity should own a closed layout");
    }
    assert_eq!(renderer.cached_frames(), cached_frames, "release must not evict the retained glyph paths");
}

#[test]
fn in_flight_old_document_remains_valid_while_reopened_story_uses_a_new_cache() {
    let mut s = Session::new();
    let sid = text_document(&mut s, "Old in-flight text.");
    let mut renderer = serial_renderer();
    let job_state = s.doc().unwrap().clone();
    let job_doc = job_state.doc.clone();
    let job_cache = s.cache.clone();
    let job_layout = job_cache.get(&job_doc, sid, None);
    let old_source = Arc::downgrade(&job_doc.stories[&sid]);
    let old_layout = Arc::downgrade(&job_layout);
    let old_cache = Arc::downgrade(&job_cache);
    let old_pixels = checked_pixels(&job_doc, &job_cache, &mut renderer);
    s.execute("file.close", &json!({})).unwrap();
    assert!(!Arc::ptr_eq(&s.cache, &job_cache));
    assert!(old_source.upgrade().is_some());
    assert!(old_layout.upgrade().is_some());

    let reopened_sid = text_document(&mut s, "New document, different visible text.");
    assert_eq!(reopened_sid, sid, "exercise the same per-document StoryId in a new cache generation");
    let new_source = Arc::downgrade(&s.doc().unwrap().doc.stories[&reopened_sid]);
    let new_layout = Arc::downgrade(&s.cache.get(&s.doc().unwrap().doc, reopened_sid, None));
    let new_pixels = checked_pixels(&s.doc().unwrap().doc, &s.cache, &mut renderer);
    assert_ne!(new_pixels, old_pixels);
    let cached_frames = renderer.cached_frames();
    assert_eq!(checked_pixels(&job_doc, &job_cache, &mut renderer), old_pixels);
    assert_eq!(renderer.cached_frames(), cached_frames, "old live snapshot still hits its own glyph entries");
    drop(job_state);
    drop(job_doc);
    drop(job_cache);
    assert!(old_cache.upgrade().is_none());
    assert!(old_source.upgrade().is_none());
    assert!(old_layout.upgrade().is_some(), "the explicit in-flight layout owner is still alive");
    drop(job_layout);
    assert!(old_layout.upgrade().is_none(), "the last real owner can release old layout buffers");
    assert_eq!(checked_pixels(&s.doc().unwrap().doc, &s.cache, &mut renderer), new_pixels);
    assert_eq!(renderer.cached_frames(), cached_frames);
    s.execute("file.close", &json!({})).unwrap();
    assert!(new_source.upgrade().is_none());
    assert!(new_layout.upgrade().is_none());
    assert_eq!(renderer.cached_frames(), cached_frames);
}

#[cfg(not(target_arch = "wasm32"))]
#[test]
fn scoped_font_jobs_and_owned_layouts_survive_close_and_same_story_reopen() {
    use designcraft_engine::FontScope;
    use designcraft_fonts::{FontDb, testing::font_with_glyph};

    const FAMILY: &str = "Memory Scoped Font";
    let root = std::env::temp_dir().join(format!("dc-memory-scoped-fonts-{}", std::process::id()));
    let old_folder = root.join("old");
    let new_folder = root.join("new");
    std::fs::create_dir_all(&old_folder).unwrap();
    std::fs::create_dir_all(&new_folder).unwrap();
    std::fs::write(old_folder.join("font.ttf"), font_with_glyph(FAMILY, &['x'], 'X').unwrap()).unwrap();
    std::fs::write(new_folder.join("font.ttf"), font_with_glyph(FAMILY, &['x'], 'O').unwrap()).unwrap();
    let db = FontDb::global();
    let old_fonts = db.load_document_fonts(&old_folder);
    let new_fonts = db.load_document_fonts(&new_folder);
    assert_eq!((old_fonts.faces, new_fonts.faces), (1, 1));
    assert_ne!(old_fonts.scope, new_fonts.scope);

    let mut s = Session::new();
    let sid = text_document(&mut s, "xxx");
    let state = s.doc_mut().unwrap();
    state.fonts = Some(Arc::new(FontScope(old_fonts.scope)));
    let doc = Arc::make_mut(&mut state.doc);
    doc.font_scope = old_fonts.scope;
    for style in &mut doc.styles_mut().paragraph {
        style.chars.font_family = Some(FAMILY.into());
    }
    // A job that can still compose must own DocState/FontScope, not only Arc<Document>.
    let job_state = s.doc().unwrap().clone();
    let old_doc = job_state.doc.clone();
    let old_source = Arc::downgrade(&old_doc.stories[&sid]);
    let job_cache = s.cache.clone();
    let job_layout = job_cache.get(&old_doc, sid, None);
    let layout_lifetime = Arc::downgrade(&job_layout);
    let mut renderer = serial_renderer();
    let old_pixels = checked_pixels(&old_doc, &job_cache, &mut renderer);
    s.execute("file.close", &json!({})).unwrap();
    assert!(db.scoped(old_fonts.scope).has_family(FAMILY));
    assert_eq!(checked_pixels(&job_state.doc, &job_cache, &mut renderer), old_pixels);

    let reopened_sid = text_document(&mut s, "xxx");
    assert_eq!(reopened_sid, sid);
    let state = s.doc_mut().unwrap();
    state.fonts = Some(Arc::new(FontScope(new_fonts.scope)));
    let doc = Arc::make_mut(&mut state.doc);
    doc.font_scope = new_fonts.scope;
    for style in &mut doc.styles_mut().paragraph {
        style.chars.font_family = Some(FAMILY.into());
    }
    let new_pixels = checked_pixels(&s.doc().unwrap().doc, &s.cache, &mut renderer);
    assert_ne!(old_pixels, new_pixels, "the new scope must use its own glyphs for the same family and StoryId");
    let cached_frames = renderer.cached_frames();
    drop(job_state);
    assert!(!db.scoped(old_fonts.scope).has_family(FAMILY), "Arc<Document> alone must not keep a font scope open");
    assert!(db.scoped(new_fonts.scope).has_family(FAMILY));
    assert!(Arc::ptr_eq(&job_layout, &job_cache.get(&old_doc, sid, None)));
    // Already composed layouts own their FontFaces, so even a cold glyph-path renderer works
    // after scope closure. Recomposition from a fresh Cache would require the old FontScope.
    let draw = |renderer: &mut Renderer| renderer.render_page(&old_doc, &job_cache, 0, 1.0, false, &Default::default()).unwrap().pixels;
    assert_eq!(draw(&mut renderer), old_pixels);
    assert_eq!(draw(&mut serial_renderer()), old_pixels);
    assert_eq!(renderer.cached_frames(), cached_frames);
    drop(old_doc);
    drop(job_cache);
    assert!(old_source.upgrade().is_none());
    assert!(layout_lifetime.upgrade().is_some());
    drop(job_layout);
    assert!(layout_lifetime.upgrade().is_none(), "cached glyph paths cannot own old layout buffers");
    assert_eq!(checked_pixels(&s.doc().unwrap().doc, &s.cache, &mut renderer), new_pixels);
    assert_eq!(renderer.cached_frames(), cached_frames);
    s.execute("file.close", &json!({})).unwrap();
    assert!(!db.scoped(new_fonts.scope).has_family(FAMILY));
    // FontDb's file/face caches have their own lifetime; this test claims Story/layout release.
    std::fs::remove_dir_all(root).unwrap();
}
