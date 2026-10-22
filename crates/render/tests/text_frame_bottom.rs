//! Bottom-baseline fitting must preserve the visible descenders below the text frame.
use designcraft_compose::Cache;
use designcraft_doc::build::NewDocument;
use designcraft_doc::{Document, FirstBaseline, ParaFormat, SpreadRef};
use designcraft_geom::{Affine, Rect};
use designcraft_render::{Placed, RenderOptions, Rendered, Renderer};

fn text_image(height: f64) -> Rendered {
    let mut d = Document::new(&NewDocument { width: 160.0, height: 100.0, facing_pages: false, ..Default::default() });
    let lid = d.default_layer();
    let (fid, sid) = d.add_text_frame(SpreadRef::Doc(0), Rect::new(20.0, 20.0, 140.0, 20.0 + height), lid, "gyp map", ParaFormat::default()).unwrap();
    let tf = d.item_mut(fid).unwrap().text_frame_mut().unwrap();
    tf.options.first_baseline = FirstBaseline::Fixed;
    tf.options.first_baseline_min = 20.0;
    let len = d.story(sid).unwrap().len();
    d.story_mut(sid).unwrap().format_chars(0..len, |f| {
        f.over.font_family = Some("Source Serif 4".into());
        f.over.size = Some(20.0);
    });
    Renderer::new().render(
        &d,
        &Cache::new(),
        &[Placed { spread: SpreadRef::Doc(0), xf: Affine::IDENTITY }],
        160,
        100,
        Affine::IDENTITY,
        &RenderOptions { background: Some([255; 4]), ..Default::default() },
    )
}

#[test]
fn bottom_baseline_fit_does_not_clip_descenders() {
    let tight = text_image(20.0);
    let roomy = text_image(60.0);
    assert!(tight.to_straight() == roomy.to_straight(), "frame height must not change or clip the glyphs");
    assert!((41..49).any(|y| (20..120).any(|x| tight.pixel(x, y)[0] < 128)), "descenders below the baseline remain visible");
}
