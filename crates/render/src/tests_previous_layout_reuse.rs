//! A retained layout reuses glyph paths while paints still come from the current document.

use std::sync::Arc;

use designcraft_compose::{Cache, ComposeOptions, ComposedStory, compose_story};
use designcraft_doc::{Document, ParaFormat, SpreadRef, StoryId, build::NewDocument};
use designcraft_geom::Rect;

use crate::{RenderOptions, Rendered, Renderer};

fn renderer() -> Renderer {
    let mut r = Renderer::new();
    r.threads = 0;
    r
}

fn assert_fresh(doc: &Document, sid: StoryId, cache: &Cache, render: &mut Renderer) -> Rendered {
    let got = cache.get(doc, sid, None);
    let fresh = compose_story(doc, sid, &ComposeOptions::default());
    assert_eq!(format!("{got:?}"), format!("{fresh:?}"));
    let faces = |cs: &ComposedStory| cs.frames.iter().flat_map(|f| &f.lines).flat_map(|l| &l.glyphs).map(|g| g.face.id()).collect::<Vec<_>>();
    assert_eq!(faces(&got), faces(&fresh));
    let got = render.render_page(doc, cache, 0, 1.0, false, &RenderOptions::default()).unwrap();
    let fresh = renderer().render_page(doc, &Cache::new(), 0, 1.0, false, &RenderOptions::default()).unwrap();
    assert_eq!((got.width, got.height), (fresh.width, fresh.height));
    assert!(got.pixels == fresh.pixels, "retained layout must render exactly like fresh composition and fresh glyph paths");
    got
}

#[test]
fn previous_layout_reuses_glyph_paths_with_fresh_paints_and_pixels() {
    let mut a = Document::new(&NewDocument { width: 288.0, height: 216.0, facing_pages: false, ..Default::default() });
    let (_, sid) = a
        .add_text_frame(
            SpreadRef::Doc(0),
            Rect::new(18.0, 18.0, 270.0, 198.0),
            a.default_layer(),
            &"Typography, decorations and reflow. ".repeat(8),
            ParaFormat::default(),
        )
        .unwrap();
    let text_len = a.story(sid).unwrap().text.len();
    a.story_mut(sid).unwrap().format_chars(0..text_len, |f| {
        f.over.underline = Some(true);
        f.over.strikethrough = Some(true);
    });
    for font_edit in [false, true] {
        let mut b = a.clone();
        if font_edit {
            b.story_mut(sid).unwrap().format_chars(0..text_len, |f| f.over.font_family = Some("Source Sans 3".into()));
        } else {
            b.story_mut(sid).unwrap().insert(0, "é inserted. ");
        }
        let mut painted = a.clone();
        let black = painted.swatches.iter_mut().find(|s| s.name == "[Black]").unwrap();
        *black = designcraft_color::swatch::Swatch::color("[Black]", designcraft_color::Color::rgb8(170, 10, 90));
        let db = designcraft_fonts::FontDb::global();
        let mut proved_reuse = false;
        // Unrelated renderer tests can publish a font or retry a missing primary face. An
        // unstable epoch permits fresh work; require a complete stable attempt for identity.
        for _ in 0..32 {
            let epoch = db.composition_epoch().unwrap();
            let cache = Cache::new();
            let mut render = renderer();
            let first = cache.get(&a, sid, None);
            let initial = assert_fresh(&a, sid, &cache, &mut render);
            let second = cache.get(&b, sid, None);
            let changed = assert_fresh(&b, sid, &cache, &mut render);
            assert!(initial.pixels != changed.pixels, "the edit must change actual output");
            let cached_frames = render.cached_frames();
            let mut identities = Vec::new();
            let mut frame_counts = Vec::new();
            for (doc, expected, recolored) in [(&a, &first, false), (&b, &second, false), (&painted, &first, true)] {
                identities.push(Arc::ptr_eq(expected, &cache.get(doc, sid, None)));
                let got = assert_fresh(doc, sid, &cache, &mut render);
                frame_counts.push(render.cached_frames());
                if recolored {
                    assert!(got.pixels != initial.pixels, "changed swatches must repaint glyphs, underlines and strikethroughs");
                }
            }
            if db.composition_epoch() != Some(epoch) {
                continue;
            }
            assert!(identities.into_iter().all(|same| same), "both old allocations must be promoted");
            assert!(frame_counts.into_iter().all(|n| n == cached_frames), "promotion must reuse the same glyph-path entries");
            assert_eq!(cached_frames, 2, "one frame's paths for each of the two layouts");
            proved_reuse = true;
            break;
        }
        assert!(proved_reuse, "no stable font epoch in 32 complete rendering attempts");
    }
}
