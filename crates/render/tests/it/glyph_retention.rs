//! Rendering cache identity and ownership regressions without allocator/RSS assumptions.

use std::sync::Arc;

use designcraft_compose::Cache;
use designcraft_doc::{Document, ParaFormat, SpreadRef};
use designcraft_geom::Rect;
use designcraft_render::{RenderOptions, Renderer};

fn serial_renderer() -> Renderer {
    let mut renderer = Renderer::new();
    renderer.threads = 0;
    renderer
}

fn document() -> (Document, designcraft_doc::StoryId) {
    let mut doc = Document::new(&Default::default());
    let layer = doc.default_layer();
    let (_, story) = doc
        .add_text_frame(
            SpreadRef::Doc(0),
            Rect::new(36.0, 36.0, 500.0, 700.0),
            layer,
            &"A cached glyph path owns its outline. ".repeat(80),
            ParaFormat::default(),
        )
        .unwrap();
    (doc, story)
}

#[test]
fn glyph_paths_do_not_own_expired_composed_story_buffers() {
    let (doc, sid) = document();
    let cache = Cache::new();
    let mut renderer = serial_renderer();
    renderer.threads = 0;
    let layout = cache.get(&doc, sid, None);
    let lifetime = Arc::downgrade(&layout);
    let initial = renderer.render_page(&doc, &cache, 0, 1.0, false, &RenderOptions::default()).unwrap();
    let frames = renderer.cached_frames();
    assert!(frames > 0, "the regression requires actual cached glyph paths");
    drop(layout);
    assert!(lifetime.upgrade().is_some(), "the composition cache still owns this layout");
    cache.clear();
    assert!(lifetime.upgrade().is_none(), "glyph-path identity must not keep all story layout buffers alive");
    assert_eq!(renderer.cached_frames(), frames, "do not achieve release by evicting cached paths");
    let fresh = serial_renderer().render_page(&doc, &Cache::new(), 0, 1.0, false, &RenderOptions::default()).unwrap();
    let rebuilt = renderer.render_page(&doc, &cache, 0, 1.0, false, &RenderOptions::default()).unwrap();
    assert_eq!(rebuilt.pixels, initial.pixels);
    assert_eq!(rebuilt.pixels, fresh.pixels);
}

#[test]
fn repeated_cache_generations_preserve_pixels_and_do_not_pin_old_layouts() {
    let (doc, sid) = document();
    let cache = Cache::new();
    let mut renderer = serial_renderer();
    renderer.threads = 0;
    let reference = serial_renderer().render_page(&doc, &Cache::new(), 0, 1.0, false, &RenderOptions::default()).unwrap();
    for _ in 0..16 {
        let layout = cache.get(&doc, sid, None);
        let lifetime = Arc::downgrade(&layout);
        let cold = renderer.render_page(&doc, &cache, 0, 1.0, false, &RenderOptions::default()).unwrap();
        let frames = renderer.cached_frames();
        let warm = renderer.render_page(&doc, &cache, 0, 1.0, false, &RenderOptions::default()).unwrap();
        assert_eq!(cold.pixels, reference.pixels);
        assert_eq!(warm.pixels, reference.pixels);
        assert_eq!(renderer.cached_frames(), frames, "warm render must reuse existing glyph entries");
        drop(layout);
        cache.clear();
        assert!(lifetime.upgrade().is_none());
    }
}

#[test]
fn edit_style_decoration_and_snapshot_restoration_match_fresh_pixels() {
    let (original, sid) = document();
    let mut edited = original.clone();
    edited.story_mut(sid).unwrap().insert(0, "é changed text. ");
    assert!(edited.story(sid).unwrap().text.starts_with("é changed text. "));
    let mut styled = edited.clone();
    for style in &mut styled.styles_mut().paragraph {
        style.chars.font_family = Some("Source Sans 3".into());
        style.chars.underline = Some(true);
        style.chars.strikethrough = Some(true);
    }
    let cache = Cache::new();
    let mut renderer = serial_renderer();
    renderer.threads = 0;
    let mut states = Vec::new();
    for doc in [&original, &edited, &styled, &edited, &original, &edited, &styled] {
        doc.check().unwrap();
        let warm = renderer.render_page(doc, &cache, 0, 1.0, false, &RenderOptions::default()).unwrap();
        let fresh = serial_renderer().render_page(doc, &Cache::new(), 0, 1.0, false, &RenderOptions::default()).unwrap();
        assert_eq!(warm.pixels, fresh.pixels);
        states.push(warm.pixels);
    }
    assert_ne!(states[0], states[1], "text edit must affect the viewport");
    assert_ne!(states[1], states[2], "font and decorations must affect the viewport");
    assert_eq!(states[0], states[4], "original snapshot restored exactly");
    assert_eq!(states[1], states[3]);
    assert_eq!(states[1], states[5]);
    assert_eq!(states[2], states[6], "styled snapshot restored exactly");
}
