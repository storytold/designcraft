use super::*;
use designcraft_doc::build::NewDocument;
use designcraft_doc::{Fill, ParaFormat};

#[test]
fn eps_source_decodes_like_its_preview() {
    let source = b"%!PS-Adobe-3.0 EPSF-3.0\n%%BoundingBox: 0 0 20 10\n%%EOF\n";
    let mut tiff = Vec::new();
    image::DynamicImage::ImageRgba8(image::RgbaImage::from_pixel(3, 2, image::Rgba([255, 0, 0, 128])))
        .write_to(&mut std::io::Cursor::new(&mut tiff), image::ImageFormat::Tiff)
        .unwrap();
    let mut dos = vec![0xC5, 0xD0, 0xD3, 0xC6];
    for value in [30usize, source.len(), 0, 0, 30 + source.len(), tiff.len()] {
        dos.extend((value as u32).to_le_bytes());
    }
    dos.extend([0xFF, 0xFF]);
    dos.extend_from_slice(source);
    dos.extend_from_slice(&tiff);
    for source in [source.as_slice(), dos.as_slice()] {
        let proxy = designcraft_images::eps_proxy(source).unwrap().0;
        let actual = decode_pixmap(source).unwrap();
        let expected = decode_pixmap(&proxy).unwrap();
        assert_eq!((actual.width(), actual.height()), (expected.width(), expected.height()));
        assert_eq!(actual.data(), expected.data());
        assert_eq!(image_size(source), image_size(&proxy));
        assert_eq!(image_mime(source), "application/postscript");
    }
    let corrupt = b"%!PS-Adobe-3.0 EPSF-3.0\nno bounding box\n";
    assert!(decode_pixmap(corrupt).is_none());
    assert!(image_size(corrupt).is_none());
}

#[test]
fn footnote_vector_objects_are_painted() {
    use designcraft_doc::{AnchorPosition, AnchoredObject, ItemId, Shape, SpreadRef};
    let mut d = Document::new(&NewDocument::default());
    let (_, sid) =
        d.add_text_frame(SpreadRef::Doc(0), Rect::new(20.0, 20.0, 300.0, 300.0), d.default_layer(), "Body", ParaFormat::default()).unwrap();
    let mut item =
        Item::new(ItemId(d.alloc()), d.default_layer(), Shape::Rectangle, designcraft_geom::shapes::rectangle(Rect::new(0.0, 0.0, 12.0, 12.0)));
    item.fill = Fill::swatch(designcraft_color::swatch::BLACK);
    // Open contours with a fill are implicitly closed for painting. A single
    // open contour must not hide the whole compound glyph.
    item.path.subpaths[0].closed = false;
    let story = d.story_mut(sid).unwrap();
    story.insert_note(4, "", ParaFormat::default());
    Arc::make_mut(&mut story.notes[0]).text.insert_object(0, AnchoredObject::new(item, AnchorPosition::Inline { y_offset: 0.0 }));
    let cache = Cache::new();
    let cs = cache.get(&d, sid, None);
    let note = &cs.frames[0].notes[0];
    let object = &note.text.frames[0].objects[0];
    let centre = note.origin + object.origin.to_vec2() + designcraft_geom::Vec2::new(6.0, 6.0);
    let image = Renderer::new().render_page(&d, &cache, 0, 1.0, false, &RenderOptions::default()).unwrap();
    let pixel = image.pixel(centre.x as u32, centre.y as u32);
    assert!(pixel[0] < 80 && pixel[1] < 80 && pixel[2] < 80, "{pixel:?}");
}

/// `Rendered` has public fields: a buffer that doesn't match the stated size used to panic
/// in `to_png`.
/// A canvas 65535 pixels wide (any render wider than that was clamped to it) panicked inside
/// vello_cpu when it snapped the width up to its tiles; a huge page export now declines.
#[test]
fn very_wide_renders_do_not_panic() {
    let doc = Document::new(&NewDocument::default());
    let cache = Cache::new();
    let mut r = Renderer::new();
    let img = r.render(&doc, &cache, &[], (70_000, 4), Affine::IDENTITY, &RenderOptions::default());
    assert_eq!(img.width, MAX_SIDE);
    let page = [Placed { spread: SpreadRef::Doc(0), xf: Affine::IDENTITY }];
    let opts = RenderOptions { background: Some([255, 255, 255, 255]), ..Default::default() };
    let img = r.render(&doc, &cache, &page, (70_000, 64), Affine::scale(120.0), &opts);
    assert_eq!(img.width, MAX_SIDE);
    assert!(r.render_page(&doc, &cache, 0, 500.0, true, &RenderOptions::default()).is_none());
}

#[test]
fn to_png_survives_a_mismatched_buffer() {
    let short = Rendered { width: 4, height: 4, pixels: vec![255; 10] };
    let png = short.to_png();
    assert_eq!(image::load_from_memory(&png).map(|i| (i.width(), i.height())).ok(), Some((4, 4)));
    assert!(Rendered { width: 0, height: 0, pixels: vec![] }.to_png().len() < 1024);
}

#[test]
fn proof_view_simulates_the_target() {
    use designcraft_color::cms::{ProofSetup, ProofTarget};
    let red = |a: u8| Rendered { width: 1, height: 1, pixels: vec![a, 0, 0, a] };
    let mut img = red(255);
    proof_view(&mut img, &ProofSetup { target: ProofTarget::Protanopia, ..Default::default() });
    assert!(img.pixels[1] > 40, "protanopia turns pure red olive: {:?}", img.pixels);
    let mut img = red(255);
    proof_view(&mut img, &ProofSetup { target: ProofTarget::MonitorRgb, ..Default::default() });
    assert!(img.pixels[0] > 245 && img.pixels[1] < 10, "monitor RGB is no simulation: {:?}", img.pixels);
    // Transparent pixels stay transparent; premultiplication is kept.
    let mut img = Rendered { width: 2, height: 1, pixels: vec![0, 0, 0, 0, 100, 0, 0, 128] };
    proof_view(&mut img, &ProofSetup { target: ProofTarget::WorkingCmyk, ..Default::default() });
    assert_eq!(&img.pixels[..4], &[0, 0, 0, 0]);
    assert!(img.pixels[4] <= 128 && img.pixels[7] == 128);
}

#[test]
fn renders_page_with_text_and_fill() {
    let mut d = Document::new(&NewDocument::default());
    let lid = d.default_layer();
    let (fid, _) = d.add_text_frame(SpreadRef::Doc(0), Rect::new(36.0, 36.0, 576.0, 300.0), lid, "Hello DesignCraft", ParaFormat::default()).unwrap();
    let id = designcraft_doc::ItemId(d.alloc());
    let mut box_ = Item::new(id, lid, designcraft_doc::Shape::Rectangle, designcraft_geom::shapes::rectangle(Rect::new(36.0, 400.0, 136.0, 500.0)));
    box_.fill = Fill::swatch("C=100 M=0 Y=0 K=0");
    d.insert_item(SpreadRef::Doc(0), box_, None).unwrap();
    let cache = Cache::new();
    let mut r = Renderer::new();
    r.threads = 0;
    let img = r.render_page(&d, &cache, 0, 1.0, false, &RenderOptions::default()).unwrap();
    assert_eq!((img.width, img.height), (612, 792));
    // Paper is white, the box is cyan-ish, text drew some dark pixels.
    assert_eq!(img.pixel(300, 700), [255, 255, 255, 255]);
    let c = img.pixel(80, 450);
    assert!(c[0] < 80 && c[2] > 150, "{c:?}");
    let dark = (36..300).flat_map(|x| (36..60).map(move |y| (x, y))).filter(|&(x, y)| img.pixel(x, y)[0] < 100).count();
    assert!(dark > 50, "text pixels: {dark}");
    let _ = fid;
    assert!(r.stats.glyphs >= 15);
}

/// An inside-aligned stroke is the stroke of the path inset by half its weight: a band from the
/// edge inwards, its rounded corners concentric with the frame's.
#[test]
fn inside_strokes_are_inset_by_half_the_weight() {
    let mut d = Document::new(&NewDocument::default());
    let lid = d.default_layer();
    let id = designcraft_doc::ItemId(d.alloc());
    let mut it = Item::new(id, lid, designcraft_doc::Shape::Rectangle, designcraft_geom::shapes::rectangle(Rect::new(100.0, 100.0, 300.0, 300.0)));
    it.stroke = designcraft_doc::Stroke {
        swatch: designcraft_color::swatch::BLACK.into(),
        weight: 10.0,
        align: designcraft_doc::StrokeAlign::Inside,
        ..Default::default()
    };
    it.corners = designcraft_geom::corners::CornerOptions::uniform(designcraft_geom::corners::CornerShape::Rounded, 20.0);
    d.insert_item(SpreadRef::Doc(0), it, None).unwrap();
    let cache = Cache::new();
    let mut r = Renderer::new();
    r.threads = 0;
    let img = r.render_page(&d, &cache, 0, 1.0, false, &RenderOptions::default()).unwrap();
    let ink = |x: u32, y: u32| img.pixel(x, y)[0] < 64;
    let paper = |x: u32, y: u32| img.pixel(x, y)[0] > 192;
    // Straight edges: ink from the edge to 10 pt inside, nothing outside.
    assert!(paper(97, 200) && ink(101, 200) && ink(108, 200) && paper(112, 200));
    assert!(ink(298, 200) && paper(302, 200) && paper(288, 200));
    // Rounded corner centred at (120, 120): ink between radii 10 and 20, paper outside the arc
    // and inside the band.
    assert!(paper(103, 103) && ink(108, 108) && paper(114, 114));
}

#[test]
fn arrowheads_draw_at_line_ends() {
    let mut d = Document::new(&NewDocument::default());
    let lid = d.default_layer();
    let id = designcraft_doc::ItemId(d.alloc());
    let path = designcraft_geom::shapes::line(designcraft_geom::Point::new(100.0, 400.0), designcraft_geom::Point::new(400.0, 400.0));
    let mut line = Item::new(id, lid, designcraft_doc::Shape::GraphicLine, path);
    line.stroke = designcraft_doc::Stroke {
        swatch: designcraft_color::swatch::BLACK.into(),
        weight: 2.0,
        end: designcraft_doc::Arrowhead::TriangleWide,
        ..Default::default()
    };
    d.insert_item(SpreadRef::Doc(0), line, None).unwrap();
    // A dashed line with a gap colour: the gaps are painted.
    let id = designcraft_doc::ItemId(d.alloc());
    let path = designcraft_geom::shapes::line(designcraft_geom::Point::new(100.0, 500.0), designcraft_geom::Point::new(400.0, 500.0));
    let mut dashed = Item::new(id, lid, designcraft_doc::Shape::GraphicLine, path);
    dashed.stroke = designcraft_doc::Stroke {
        swatch: designcraft_color::swatch::BLACK.into(),
        weight: 4.0,
        kind: designcraft_doc::StrokeType::Dashed { pattern: vec![12.0, 4.0] },
        gap_swatch: "C=100 M=0 Y=0 K=0".into(),
        ..Default::default()
    };
    d.insert_item(SpreadRef::Doc(0), dashed, None).unwrap();
    let cache = Cache::new();
    let mut r = Renderer::new();
    r.threads = 0;
    let img = r.render_page(&d, &cache, 0, 1.0, false, &RenderOptions::default()).unwrap();
    // The head is 6.4 × weight wide: ink 4 pt above the line near the tip, none at the start.
    assert!(img.pixel(393, 396)[0] < 128, "{:?}", img.pixel(393, 396));
    assert_eq!(img.pixel(105, 396), [255, 255, 255, 255]);
    // Nothing past the tip.
    assert_eq!(img.pixel(403, 400), [255, 255, 255, 255]);
    let (dash, gap) = (img.pixel(106, 500), img.pixel(114, 500));
    assert!(dash[0] < 80 && dash[2] < 80, "dash {dash:?}");
    assert!(gap[0] < 80 && gap[2] > 150, "gap {gap:?}");
}

#[test]
fn overprint_preview_mixes_inks() {
    let mut d = Document::new(&NewDocument::default());
    let lid = d.default_layer();
    for (sw, op) in [("C=0 M=0 Y=100 K=0", false), ("C=0 M=100 Y=0 K=0", true)] {
        let id = designcraft_doc::ItemId(d.alloc());
        let mut it =
            Item::new(id, lid, designcraft_doc::Shape::Rectangle, designcraft_geom::shapes::rectangle(Rect::new(100.0, 100.0, 200.0, 200.0)));
        it.fill = Fill { overprint: op, ..Fill::swatch(sw) };
        d.insert_item(SpreadRef::Doc(0), it, None).unwrap();
    }
    let cache = Cache::new();
    let mut r = Renderer::new();
    r.threads = 0;
    let px = |on: bool, r: &mut Renderer| {
        r.render_page(&d, &cache, 0, 1.0, false, &RenderOptions { overprint_preview: on, ..Default::default() }).unwrap().pixel(150, 150)
    };
    let off = px(false, &mut r);
    let on = px(true, &mut r);
    assert!(off[2] > 100, "magenta knocks out: {off:?}");
    assert!(on[2] < 60 && on[0] > 180, "magenta over yellow → red: {on:?}");
}

#[test]
fn cmyk_blend_space_shows_transparent_spreads_in_cmyk() {
    let mut d = Document::new(&NewDocument::default());
    d.swatches.push(designcraft_color::swatch::Swatch::color("Green", designcraft_color::Color::rgb8(0, 255, 0)));
    let lid = d.default_layer();
    let id = designcraft_doc::ItemId(d.alloc());
    let mut it = Item::new(id, lid, designcraft_doc::Shape::Rectangle, designcraft_geom::shapes::rectangle(Rect::new(100.0, 100.0, 200.0, 200.0)));
    it.fill = Fill::swatch("Green");
    d.insert_item(SpreadRef::Doc(0), it, None).unwrap();
    let cache = Cache::new();
    let mut r = Renderer::new();
    r.threads = 0;
    let px = |d: &Document, r: &mut Renderer| {
        r.render_page(d, &cache, 0, 1.0, false, &RenderOptions { blend_space_view: true, ..Default::default() }).unwrap().pixel(150, 150)
    };
    // No transparency on the spread: shown as is.
    assert_eq!(px(&d, &mut r)[1], 255);
    // With transparency in a CMYK blend space the bright green leaves the RGB gamut's edge.
    d.item_mut(id).unwrap().opacity = 0.99;
    let cmyk = px(&d, &mut r);
    assert!(cmyk[0] > 20 || cmyk[1] < 240, "{cmyk:?}");
    d.settings.blend_space = designcraft_doc::BlendSpace::Rgb;
    let rgb = px(&d, &mut r);
    assert!(rgb[1] > 245 && rgb[0] < 10, "{rgb:?}");
}

#[test]
fn blend_modes_blend_in_cmyk_on_screen_in_a_cmyk_blend_space() {
    // A lilac (RGB) rectangle, and a 100% K one over its lower half and the paper below it,
    // in Color Dodge: in CMYK the black clears the lilac's inks (white), over the paper it
    // shows as it is; in RGB, dodging with black changes nothing.
    let mut d = Document::new(&NewDocument::default());
    d.swatches.push(designcraft_color::swatch::Swatch::color("Lilac", designcraft_color::Color::rgb8(171, 154, 227)));
    let lid = d.default_layer();
    let rect = |d: &mut Document, r: Rect, fill: &str| {
        let id = designcraft_doc::ItemId(d.alloc());
        let mut it = Item::new(id, lid, designcraft_doc::Shape::Rectangle, designcraft_geom::shapes::rectangle(r));
        it.fill = Fill::swatch(fill);
        d.insert_item(SpreadRef::Doc(0), it, None).unwrap();
        id
    };
    rect(&mut d, Rect::new(100.0, 100.0, 300.0, 200.0), "Lilac");
    let black = rect(&mut d, Rect::new(150.0, 150.0, 250.0, 250.0), designcraft_color::swatch::BLACK);
    d.item_mut(black).unwrap().blend = DcBlend::ColorDodge;
    let cache = Cache::new();
    let mut r = Renderer::new();
    r.threads = 0;
    let px = |d: &Document, r: &mut Renderer, x: u32, y: u32| {
        r.render_page(d, &cache, 0, 1.0, false, &RenderOptions { blend_space_view: true, ..Default::default() }).unwrap().pixel(x, y)
    };
    let over_lilac = px(&d, &mut r, 200, 175);
    assert!(over_lilac[..3].iter().all(|v| *v > 235), "dodged to white: {over_lilac:?}");
    let over_paper = px(&d, &mut r, 200, 225);
    assert!(over_paper[..3].iter().all(|v| *v < 90), "black as it is: {over_paper:?}");
    let lilac = px(&d, &mut r, 120, 150);
    assert!(lilac[2] > lilac[1] + 25, "the lilac beside it: {lilac:?}");
    // Inside a group, the group's blend mode.
    let g = designcraft_doc::ItemId(d.alloc());
    let it = d.remove_item(black).unwrap();
    let mut grp = Item::new(g, lid, designcraft_doc::Shape::Group, Default::default());
    let mut inner = (*it).clone();
    inner.blend = DcBlend::Normal;
    grp.content = Content::Group { items: vec![Arc::new(inner)] };
    grp.blend = DcBlend::ColorDodge;
    d.insert_item(SpreadRef::Doc(0), grp, None).unwrap();
    let grouped = px(&d, &mut r, 200, 175);
    assert!(grouped[..3].iter().all(|v| *v > 235), "{grouped:?}");
    // An RGB blend space: still lilac under the black (lightened a little: [Black] shows as a
    // dark grey in RGB).
    d.settings.blend_space = designcraft_doc::BlendSpace::Rgb;
    let rgb = px(&d, &mut r, 200, 175);
    assert!(rgb[2] > rgb[1] + 25 && rgb[1] < 220, "{rgb:?}");
}

#[test]
fn knockout_group_hides_its_own_objects() {
    let mut d = Document::new(&NewDocument::default());
    let lid = d.default_layer();
    let mut kids = Vec::new();
    for (sw, x) in [("C=0 M=0 Y=100 K=0", 100.0), ("C=0 M=100 Y=0 K=0", 150.0)] {
        let id = designcraft_doc::ItemId(d.alloc());
        let mut it =
            Item::new(id, lid, designcraft_doc::Shape::Rectangle, designcraft_geom::shapes::rectangle(Rect::new(x, 100.0, x + 100.0, 200.0)));
        it.fill = Fill::swatch(sw);
        it.opacity = 0.5;
        kids.push(std::sync::Arc::new(it));
    }
    let gid = designcraft_doc::ItemId(d.alloc());
    let mut g = Item::new(gid, lid, designcraft_doc::Shape::Group, designcraft_geom::shapes::rectangle(Rect::new(100.0, 100.0, 250.0, 200.0)));
    g.content = designcraft_doc::Content::Group { items: kids };
    d.insert_item(SpreadRef::Doc(0), g, None).unwrap();
    let cache = Cache::new();
    let mut r = Renderer::new();
    r.threads = 0;
    let overlap = |d: &Document, r: &mut Renderer| r.render_page(d, &cache, 0, 1.0, false, &RenderOptions::default()).unwrap().pixel(175, 150);
    let plain = overlap(&d, &mut r);
    d.item_mut(gid).unwrap().knockout = true;
    let ko = overlap(&d, &mut r);
    let magenta_only = r.render_page(&d, &cache, 0, 1.0, false, &RenderOptions::default()).unwrap().pixel(225, 150);
    assert_ne!(plain, ko, "{plain:?} {ko:?}");
    for c in 0..3 {
        assert!((ko[c] as i32 - magenta_only[c] as i32).abs() <= 3, "the overlap shows magenta alone: {ko:?} vs {magenta_only:?}");
    }
}

#[test]
fn black_overprints_only_with_the_preference() {
    let mut d = Document::new(&NewDocument::default());
    let lid = d.default_layer();
    for (sw, r) in
        [("C=100 M=0 Y=0 K=0", Rect::new(100.0, 100.0, 200.0, 200.0)), (designcraft_color::swatch::BLACK, Rect::new(150.0, 100.0, 250.0, 200.0))]
    {
        let id = designcraft_doc::ItemId(d.alloc());
        let mut it = Item::new(id, lid, designcraft_doc::Shape::Rectangle, designcraft_geom::shapes::rectangle(r));
        it.fill = Fill::swatch(sw);
        d.insert_item(SpreadRef::Doc(0), it, None).unwrap();
    }
    let cache = Cache::new();
    let mut r = Renderer::new();
    r.threads = 0;
    let o = RenderOptions { overprint_preview: true, ..Default::default() };
    let on = r.render_page(&d, &cache, 0, 1.0, false, &o).unwrap().pixel(175, 150);
    let alone = r.render_page(&d, &cache, 0, 1.0, false, &o).unwrap().pixel(225, 150);
    d.settings.overprint_black = false;
    let off = r.render_page(&d, &cache, 0, 1.0, false, &o).unwrap().pixel(175, 150);
    assert_eq!(off, alone, "knocks out: black alone");
    assert_ne!(on, alone, "overprints: the cyan shows through");
}

#[test]
fn appearance_of_black_on_screen() {
    let mut d = Document::new(&NewDocument::default());
    let lid = d.default_layer();
    let id = designcraft_doc::ItemId(d.alloc());
    let mut it = Item::new(id, lid, designcraft_doc::Shape::Rectangle, designcraft_geom::shapes::rectangle(Rect::new(100.0, 100.0, 200.0, 200.0)));
    it.fill = Fill::swatch(designcraft_color::swatch::BLACK);
    d.insert_item(SpreadRef::Doc(0), it, None).unwrap();
    let cache = Cache::new();
    let mut r = Renderer::new();
    r.threads = 0;
    let px = |rich: bool, r: &mut Renderer| {
        r.render_page(&d, &cache, 0, 1.0, false, &RenderOptions { rich_black: rich, ..Default::default() }).unwrap().pixel(150, 150)
    };
    assert!(px(false, &mut r)[0] > 20, "accurate: a dark grey");
    assert_eq!(px(true, &mut r)[..3], [0, 0, 0]);
}

#[test]
fn display_performance_fast_draws_grey_boxes() {
    let mut d = Document::new(&NewDocument::default());
    let lid = d.default_layer();
    // A 4×4 red PNG placed at 100,100 (100×100 pt).
    let mut png = Vec::new();
    image::RgbaImage::from_pixel(4, 4, image::Rgba([255, 0, 0, 255])).write_to(&mut std::io::Cursor::new(&mut png), image::ImageFormat::Png).unwrap();
    let aid = designcraft_doc::AssetId(d.alloc());
    d.assets.insert(
        aid,
        Arc::new(designcraft_doc::Asset {
            id: aid,
            name: "red.png".into(),
            mime: "image/png".into(),
            data: Arc::new(png),
            pixels: Some((4, 4)),
            ..Default::default()
        }),
    );
    let id = designcraft_doc::ItemId(d.alloc());
    let mut it = Item::new(id, lid, designcraft_doc::Shape::Rectangle, designcraft_geom::shapes::rectangle(Rect::new(100.0, 100.0, 200.0, 200.0)));
    it.content = designcraft_doc::Content::Graphic(designcraft_doc::Graphic {
        layer_options: None,
        asset: aid,
        size: (100.0, 100.0),
        xf: Affine::translate((100.0, 100.0)),
        clip: None,
        auto_fit: Default::default(),
        fit_align: 4,
        crop: [0.0; 4],
        wrap: Default::default(),
        fill: Default::default(),
    });
    d.insert_item(SpreadRef::Doc(0), it, None).unwrap();
    let cache = Cache::new();
    let mut r = Renderer::new();
    r.threads = 0;
    let px = |q: DisplayQuality, r: &mut Renderer| {
        r.render_page(&d, &cache, 0, 1.0, false, &RenderOptions { quality: q, ..Default::default() }).unwrap().pixel(150, 150)
    };
    assert_eq!(px(DisplayQuality::High, &mut r)[..3], [255, 0, 0]);
    assert_eq!(px(DisplayQuality::Typical, &mut r)[..3], [255, 0, 0]);
    assert_eq!(px(DisplayQuality::Fast, &mut r)[..3], [178, 178, 178]);
}

#[test]
fn clipping_path_hides_the_image_outside_it() {
    let mut d = Document::new(&NewDocument::default());
    let lid = d.default_layer();
    let mut png = Vec::new();
    image::RgbaImage::from_pixel(4, 4, image::Rgba([255, 0, 0, 255])).write_to(&mut std::io::Cursor::new(&mut png), image::ImageFormat::Png).unwrap();
    let aid = designcraft_doc::AssetId(d.alloc());
    d.assets.insert(
        aid,
        Arc::new(designcraft_doc::Asset {
            page: 0,
            id: aid,
            name: "red.png".into(),
            mime: "image/png".into(),
            link: None,
            data: Arc::new(png),
            pixels: Some((4, 4)),
            ..Default::default()
        }),
    );
    // A 100 pt image at 100,100 clipped by a triangle with its apex at the top middle.
    let triangle = designcraft_geom::PathData::single(designcraft_geom::path::SubPath::polyline(
        &[designcraft_geom::Point::new(0.0, 100.0), designcraft_geom::Point::new(50.0, 0.0), designcraft_geom::Point::new(100.0, 100.0)],
        true,
    ));
    let mut clip = designcraft_doc::ClippingPath::new(designcraft_doc::ClippingKind::PhotoshopPath, triangle);
    let id = designcraft_doc::ItemId(d.alloc());
    let mut it = Item::new(id, lid, designcraft_doc::Shape::Rectangle, designcraft_geom::shapes::rectangle(Rect::new(100.0, 100.0, 200.0, 200.0)));
    it.content = designcraft_doc::Content::Graphic(designcraft_doc::Graphic {
        layer_options: None,
        asset: aid,
        size: (100.0, 100.0),
        xf: Affine::translate((100.0, 100.0)),
        auto_fit: Default::default(),
        fit_align: 4,
        crop: [0.0; 4],
        wrap: Default::default(),
        clip: Some(Arc::new(clip.clone())),
        fill: Default::default(),
    });
    d.insert_item(SpreadRef::Doc(0), it, None).unwrap();
    let cache = Cache::new();
    let mut r = Renderer::new();
    r.threads = 0;
    let page = r.render_page(&d, &cache, 0, 1.0, false, &RenderOptions::default()).unwrap();
    assert_eq!(page.pixel(150, 170)[..3], [255, 0, 0], "inside the triangle");
    assert_eq!(page.pixel(110, 110)[..3], [255, 255, 255], "inside the frame, outside the triangle: paper");
    // Inverted: the corners show and the triangle is paper.
    clip.invert = true;
    if let Some(designcraft_doc::Content::Graphic(g)) = d.item_mut(id).map(|it| &mut it.content) {
        g.clip = Some(Arc::new(clip));
    }
    let cache = Cache::new();
    let page = r.render_page(&d, &cache, 0, 1.0, false, &RenderOptions::default()).unwrap();
    assert_eq!(page.pixel(150, 170)[..3], [255, 255, 255]);
    assert_eq!(page.pixel(110, 110)[..3], [255, 0, 0]);
}

#[test]
fn missing_fonts_are_highlighted_on_screen_only() {
    let mut d = Document::new(&NewDocument::default());
    let lid = d.default_layer();
    let (_, sid) = d.add_text_frame(SpreadRef::Doc(0), Rect::new(36.0, 36.0, 336.0, 100.0), lid, "Missing font", ParaFormat::default()).unwrap();
    d.story_mut(sid).unwrap().format_chars(0..12, |f| f.over.font_family = Some("No Such Family".into()));
    let cache = Cache::new();
    let cs = cache.get(&d, sid, None);
    assert!(cs.styles.iter().any(|s| s.missing_font));
    let l = &cs.frames[0].lines[0];
    // A point inside the line box but between glyph strokes: the space after "Missing".
    let g = l.glyphs.iter().find(|g| g.byte == 7).unwrap();
    let (x, y) = ((g.x + g.adv / 2.0) as u32, (l.baseline - l.ascent * 0.5) as u32);
    let mut r = Renderer::new();
    r.threads = 0;
    let plain = r.render_page(&d, &cache, 0, 1.0, false, &RenderOptions::default()).unwrap();
    let shown = r.render_page(&d, &cache, 0, 1.0, false, &RenderOptions { highlight_missing_fonts: true, ..Default::default() }).unwrap();
    assert_eq!(plain.pixel(x, y)[1], 255, "no highlight in output");
    let p = shown.pixel(x, y);
    assert!(p[0] > 240 && p[1] < 200, "pink on screen: {p:?}");
}

#[test]
fn table_edges_paint_gap_colour_between_dashes_and_stripes() {
    let mut d = Document::new(&NewDocument::default());
    let lid = d.default_layer();
    let (_, sid) = d.add_text_frame(SpreadRef::Doc(0), Rect::new(36.0, 36.0, 336.0, 400.0), lid, "", ParaFormat::default()).unwrap();
    let mut t = designcraft_doc::Table::new(1, 1, 1, 0, 0, 200.0);
    t.rows[0].height = 60.0;
    t.rows[0].mode = designcraft_doc::RowHeightMode::Exactly;
    let gap = |kind| designcraft_doc::CellStroke { weight: 8.0, kind, gap_color: "C=100 M=0 Y=0 K=0".into(), ..Default::default() };
    let cell = t.cell_mut(0, 0).unwrap();
    cell.strokes[0] = gap(designcraft_doc::StrokeType::Dashed { pattern: vec![12.0, 6.0] });
    cell.strokes[2] = gap(designcraft_doc::StrokeType::ThickThin);
    cell.border_overrides = [true; 4];
    cell.stroke_defined = [true; 4];
    d.story_mut(sid).unwrap().insert_table(0, t);
    let cache = Cache::new();
    let rect = cache.get(&d, sid, None).frames[0].tables[0].cell(0, 0).unwrap().rect;
    let mut r = Renderer::new();
    r.threads = 0;
    let img = r.render_page(&d, &cache, 0, 1.0, false, &RenderOptions::default()).unwrap();
    let ink = |p: [u8; 4]| p[0] < 80 && p[2] < 80;
    let cyan = |p: [u8; 4]| p[0] < 80 && p[2] > 150;
    let y = rect.y0.round() as u32;
    let (dash, space) = (img.pixel((rect.x0 + 6.0) as u32, y), img.pixel((rect.x0 + 15.0) as u32, y));
    assert!(ink(dash), "dash {dash:?}");
    assert!(cyan(space), "gap {space:?}");
    let column: Vec<_> = (rect.y1 as u32 - 5..=rect.y1 as u32 + 5).map(|y| img.pixel(rect.center().x as u32, y)).collect();
    assert!(column.iter().any(|p| ink(*p)) && column.iter().any(|p| cyan(*p)), "stripes and gap: {column:?}");
}

#[test]
fn degenerate_dash_patterns_draw_solid_and_fill_types_close_table_corners() {
    let fine = designcraft_doc::StrokeType::Dashed { pattern: vec![1e-6, 1e-6] };
    // Expanding these would take ~10^8 dashes per edge.
    assert!(text::cell_stroke(&fine, 4.0, 200.0).dash_pattern.is_empty());
    assert!(text::cell_stroke(&designcraft_doc::StrokeType::Dotted, 1e-6, 200.0).dash_pattern.is_empty());
    assert!(!text::cell_stroke(&designcraft_doc::StrokeType::Dashed { pattern: vec![3.0, 2.0] }, 1.0, 200.0).dash_pattern.is_empty());
    let mut d = Document::new(&NewDocument::default());
    let lid = d.default_layer();
    let (fid, sid) = d.add_text_frame(SpreadRef::Doc(0), Rect::new(36.0, 36.0, 336.0, 400.0), lid, "", ParaFormat::default()).unwrap();
    d.item_mut(fid).unwrap().stroke = designcraft_doc::Stroke { weight: 2.0, kind: fine.clone(), ..Default::default() };
    let mut t = designcraft_doc::Table::new(1, 2, 2, 0, 0, 200.0);
    for r in 0..2 {
        t.rows[r].height = 60.0;
        t.rows[r].mode = designcraft_doc::RowHeightMode::Exactly;
    }
    let stroke = |kind| designcraft_doc::CellStroke { weight: 8.0, kind, gap_color: "C=100 M=0 Y=0 K=0".into(), ..Default::default() };
    for cell in &mut t.cells {
        cell.strokes = std::array::from_fn(|_| stroke(designcraft_doc::StrokeType::ThickThin));
        cell.border_overrides = [true; 4];
        cell.stroke_defined = [true; 4];
    }
    if let Some(cell) = t.cell_mut(1, 0) {
        cell.strokes[2] = stroke(fine);
    }
    d.story_mut(sid).unwrap().insert_table(0, t);
    let cache = Cache::new();
    let frag = cache.get(&d, sid, None).frames[0].tables[0].clone();
    let (top, bottom) = (frag.cell(0, 0).unwrap().rect, frag.cell(1, 0).unwrap().rect);
    let mut r = Renderer::new();
    r.threads = 0;
    let img = r.render_page(&d, &cache, 0, 1.0, false, &RenderOptions::default()).unwrap();
    let ink = |p: [u8; 4]| p[0] < 80;
    let solid: Vec<_> = (2..10).map(|i| img.pixel((bottom.x0 + 20.0 + i as f64 * 0.5) as u32, bottom.y1.round() as u32)).collect();
    assert!(solid.iter().all(|p| ink(*p)), "the degenerate dashed edge draws solid: {solid:?}");
    // Outside the table's top-left corner, inside both edges' reach.
    let corner = img.pixel((top.x0 - 3.0) as u32, (top.y0 - 3.0) as u32);
    assert!(ink(corner), "corner closed: {corner:?}");
    let edge = frag.strokes.iter().find(|s| s.a == designcraft_geom::Point::new(top.x0, top.y0) && s.a.y == s.b.y).unwrap();
    let (a, b) = frag.corner_capped(edge);
    assert_eq!((a, b), (designcraft_geom::Point::new(top.x0 - 4.0, top.y0), edge.b), "only table corners extend");
}

#[test]
fn table_gradient_fill_is_continuous_across_cells() {
    let mut d = Document::new(&NewDocument::default());
    let mut gradient = designcraft_color::Gradient::default();
    gradient.stops[0].color = designcraft_color::Color::rgb(1.0, 0.0, 0.0);
    gradient.stops[1].color = designcraft_color::Color::rgb(0.0, 0.0, 1.0);
    d.swatches.push(designcraft_color::Swatch {
        name: "Table gradient".into(),
        value: designcraft_color::SwatchValue::Gradient { gradient },
        locked: false,
        named: true,
        hidden: false,
    });
    let (_, sid) = d.add_text_frame(SpreadRef::Doc(0), Rect::new(36.0, 36.0, 336.0, 300.0), d.default_layer(), "", ParaFormat::default()).unwrap();
    let mut t = designcraft_doc::Table::new(1, 1, 2, 0, 0, 200.0);
    t.rows[0].height = 40.0;
    t.rows[0].mode = designcraft_doc::RowHeightMode::Exactly;
    for cell in &mut t.cells {
        cell.fill = "Table gradient".into();
    }
    d.story_mut(sid).unwrap().insert_table(0, t);
    let cache = Cache::new();
    let rect = cache.get(&d, sid, None).frames[0].tables[0].rect;
    let mut renderer = Renderer::new();
    renderer.threads = 0;
    let img = renderer.render_page(&d, &cache, 0, 1.0, false, &RenderOptions::default()).unwrap();
    let y = rect.center().y as u32;
    let a = img.pixel((rect.x0 + 20.0) as u32, y);
    let b = img.pixel((rect.x1 - 20.0) as u32, y);
    assert!(i16::from(a[0]) - i16::from(a[2]) > 80, "red end: {a:?}");
    assert!(i16::from(b[2]) - i16::from(b[0]) > 80, "blue end: {b:?}");
    let left = img.pixel((rect.center().x - 2.0) as u32, y);
    let right = img.pixel((rect.center().x + 2.0) as u32, y);
    assert!((i16::from(left[0]) - i16::from(right[0])).abs() < 20, "no gradient restart between cells");
}

#[test]
fn renders_tables() {
    let mut d = Document::new(&NewDocument::default());
    let lid = d.default_layer();
    let (_, sid) = d.add_text_frame(SpreadRef::Doc(0), Rect::new(36.0, 36.0, 336.0, 400.0), lid, "", ParaFormat::default()).unwrap();
    let mut t = designcraft_doc::Table::new(5, 3, 3, 1, 0, 300.0);
    for c in 0..3 {
        let cell = t.cell_mut(0, c).unwrap();
        cell.fill = "C=100 M=0 Y=0 K=0".into();
        cell.text.insert(0, "Head");
        t.cell_mut(2, c).unwrap().text.insert(0, "WWWWWWWW");
    }
    d.story_mut(sid).unwrap().insert_table(0, t);
    let cache = Cache::new();
    let cs = cache.get(&d, sid, None);
    let tf = &cs.frames[0].tables[0];
    let head = tf.cell(0, 1).unwrap().rect;
    let body = tf.cell(2, 0).unwrap().rect;
    let mut r = Renderer::new();
    r.threads = 0;
    let img = r.render_page(&d, &cache, 0, 1.0, false, &RenderOptions::default()).unwrap();
    // Header fill (cyan) near the right edge of the middle header cell.
    let c = img.pixel((head.x1 - 3.0) as u32, (head.y0 + 3.0) as u32);
    assert!(c[0] < 80 && c[2] > 150, "{c:?}");
    // The table border (black) on the left edge.
    let e = img.pixel(body.x0.round() as u32, body.center().y as u32);
    assert!(e[0] < 200, "{e:?}");
    // Cell text drew dark pixels inside the body cell.
    let dark = ((body.x0 + 5.0) as u32..(body.x1 - 2.0) as u32)
        .flat_map(|x| ((body.y0 + 2.0) as u32..(body.y1 - 2.0) as u32).map(move |y| (x, y)))
        .filter(|&(x, y)| img.pixel(x, y)[0] < 100)
        .count();
    assert!(dark > 30, "cell text pixels: {dark}");
}

/// A one-page PDF (612 x 792 pt) that draws `content`.
fn one_page_pdf(content: &str) -> Vec<u8> {
    let objects = [
        "<< /Type /Catalog /Pages 2 0 R >>".to_string(),
        "<< /Type /Pages /Kids [3 0 R] /Count 1 >>".to_string(),
        "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 612 792] /Contents 4 0 R >>".to_string(),
        format!("<< /Length {} >>\nstream\n{content}\nendstream", content.len()),
    ];
    let mut out = b"%PDF-1.4\n".to_vec();
    let mut offsets = Vec::new();
    for (i, o) in objects.iter().enumerate() {
        offsets.push(out.len());
        out.extend(format!("{} 0 obj\n{o}\nendobj\n", i + 1).bytes());
    }
    let xref = out.len();
    out.extend(format!("xref\n0 {}\n0000000000 65535 f \n", objects.len() + 1).bytes());
    for o in offsets {
        out.extend(format!("{o:010} 00000 n \n").bytes());
    }
    out.extend(format!("trailer\n<< /Size {} /Root 1 0 R >>\nstartxref\n{xref}\n%%EOF\n", objects.len() + 1).bytes());
    out
}

/// A Bounding Box crop is the exact extent of what the page draws, strokes included, found
/// without rasterizing the page: PDF export of a document that places a complex illustration
/// cropped that way spent seconds rendering it just to measure it.
#[test]
fn content_crop_box_is_measured_without_rendering_the_page() {
    // A filled rectangle 100..300 x 532..592 (PDF y up) and a 10 pt horizontal line at
    // y 100 from x 400 to 500, butt capped. Clipped away: a rectangle off to the left.
    let pdf = one_page_pdf("0 0 0 rg 100 532 200 60 re f 10 w 400 100 m 500 100 l S q 0 0 50 50 re W n 100 0 10 10 re f Q");
    let before = PDF_RENDERS.with(std::cell::Cell::get);
    let (frac, size) = pdf_crop_box(&pdf, 0, designcraft_doc::PdfCrop::ContentVisible).unwrap();
    assert_eq!(PDF_RENDERS.with(std::cell::Cell::get), before, "the page was rasterized");
    let want = [100.0 / 612.0, 200.0 / 792.0, 500.0 / 612.0, 697.0 / 792.0];
    assert!(frac.iter().zip(want).all(|(a, b)| (a - b).abs() < 1e-6), "{frac:?} vs {want:?}");
    assert!((size.0 - 400.0).abs() < 1e-3 && (size.1 - 497.0).abs() < 1e-3, "{size:?}");
}

/// An inline object is painted under its frame's text, as InDesign draws it: a picture
/// whose frame reaches down into the next line (a negative Y offset) doesn't hide that line.
#[test]
fn inline_objects_are_painted_under_the_text() {
    use designcraft_doc::{AnchorPosition, AnchoredObject, ItemId, Shape, SpreadRef};
    let mut d = Document::new(&NewDocument::default());
    let (fid, sid) =
        d.add_text_frame(SpreadRef::Doc(0), Rect::new(20.0, 20.0, 300.0, 300.0), d.default_layer(), "A\nWWWW", ParaFormat::default()).unwrap();
    let mut item =
        Item::new(ItemId(d.alloc()), d.default_layer(), Shape::Rectangle, designcraft_geom::shapes::rectangle(Rect::new(0.0, 0.0, 250.0, 60.0)));
    item.fill = Fill::swatch(designcraft_color::swatch::PAPER);
    d.story_mut(sid).unwrap().insert_object(1, AnchoredObject::new(item, AnchorPosition::Inline { y_offset: -40.0 }));
    let cache = Cache::new();
    let cs = cache.get(&d, sid, None);
    let (ft, obj) = (&cs.frames[0], &cs.frames[0].objects[0]);
    let l = ft.lines.iter().find(|l| l.range.start > 1).unwrap();
    // The second line lies inside the white box.
    assert!(obj.origin.y + obj.size.1 > l.baseline && obj.origin.y < l.baseline - l.ascent, "{:?} {}", obj.origin, l.baseline);
    let m = d.text_xf(d.item(fid).unwrap());
    let image = Renderer::new().render_page(&d, &cache, 0, 1.0, false, &RenderOptions::default()).unwrap();
    let (a, b) = (m * designcraft_geom::Point::new(l.x0, l.baseline - l.ascent), m * designcraft_geom::Point::new(l.end_x, l.baseline));
    let dark =
        (a.y as u32..b.y as u32).flat_map(|y| (a.x as u32..b.x as u32).map(move |x| (x, y))).filter(|&(x, y)| image.pixel(x, y)[0] < 100).count();
    assert!(dark > 20, "the line's glyphs show over the box: {dark} dark pixels");
}
