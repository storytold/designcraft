use super::*;

const SVG: &str = r##"<?xml version="1.0"?>
<svg xmlns="http://www.w3.org/2000/svg" width="200" height="100" viewBox="0 0 200 100">
  <rect x="0" y="0" width="100" height="100" fill="#ff0000"/>
  <text x="110" y="60" font-size="20">Hi</text>
</svg>"##;

/// Build an SVG with the requested number of nested `g` wrappers around a leaf rect.
fn nested_svg(groups: usize) -> String {
    format!(
        r#"<svg xmlns="http://www.w3.org/2000/svg" width="10" height="10">{}<rect width="10" height="10"/>{}</svg>"#,
        "<g opacity=\"0.99\">".repeat(groups),
        "</g>".repeat(groups)
    )
}

/// Fast-gzip helper for SVGZ tests.
fn gzip(bytes: &[u8]) -> Vec<u8> {
    use std::io::Write;

    let mut encoder = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::fast());
    encoder.write_all(bytes).unwrap();
    encoder.finish().unwrap()
}

/// A boundary-depth SVG parses into a tree; one level deeper and 2,000 levels are rejected by the tree, size and render APIs.
#[test]
fn svg_depth_is_checked_before_recursive_parsing() {
    // Count the SVG root and leaf too; the boundary document still draws its path.
    let accepted = nested_svg(MAX_SVG_DEPTH - 2);
    let tree = svg_tree(accepted.as_bytes()).unwrap();
    assert!(!tree.root().children().is_empty());
    drop(tree);
    for depth in [MAX_SVG_DEPTH - 1, 2000] {
        let rejected = nested_svg(depth);
        assert!(svg_tree(rejected.as_bytes()).is_none());
        assert!(natural_size(rejected.as_bytes()).is_none());
        assert!(render_svg(rejected.as_bytes(), 10).is_none());
    }
}

/// Markup-looking text in comments, CDATA and attributes does not count as XML depth.
#[test]
fn svg_depth_ignores_markup_in_comments_cdata_and_attributes() {
    let markup = "<g>".repeat(2000);
    let svg = format!(
        r#"<svg xmlns="http://www.w3.org/2000/svg" width="10" height="10"><!--{markup}--><desc><![CDATA[{markup}]]></desc><rect data-label="&lt;g>" width="10" height="10"/></svg>"#
    );
    assert!(svg_tree(svg.as_bytes()).is_some());
}

/// An external SVG doctype is accepted; an internal entity subset is rejected.
#[test]
fn svg_external_doctype_is_preserved_but_entity_markup_is_refused() {
    let external = format!(r#"<!DOCTYPE svg PUBLIC "-//W3C//DTD SVG 1.1//EN" "http://www.w3.org/Graphics/SVG/1.1/DTD/svg11.dtd">{}"#, nested_svg(2));
    assert!(svg_tree(external.as_bytes()).is_some());
    let internal = r#"<!DOCTYPE svg [<!ENTITY shape "<rect width='10' height='10'/>">]><svg xmlns="http://www.w3.org/2000/svg" width="10" height="10">&shape;</svg>"#;
    assert!(svg_tree(internal.as_bytes()).is_none());
}

/// The exact 16 MiB document and 100,000-event boundaries are accepted, each one-over case rejected.
#[test]
fn svg_byte_and_event_limits_are_enforced() {
    let mut bytes = nested_svg(0).into_bytes();
    bytes.resize(MAX_SVG_BYTES, b' ');
    assert!(svg_tree(&bytes).is_some());
    bytes.push(b' ');
    assert!(svg_tree(&bytes).is_none());

    let document = |comments| {
        format!(
            r#"<svg xmlns="http://www.w3.org/2000/svg" width="10" height="10">{}<rect width="10" height="10"/></svg>"#,
            "<!-- -->".repeat(comments)
        )
    };
    assert!(svg_tree(document(MAX_SVG_EVENTS - 4).as_bytes()).is_some());
    assert!(svg_tree(document(MAX_SVG_EVENTS - 3).as_bytes()).is_none());
}

/// SVGZ depth, decoded size and malformed gzip follow the plain-SVG limits.
#[test]
fn svgz_has_the_same_depth_and_decoded_size_limits() {
    assert!(svg_tree(&gzip(nested_svg(2).as_bytes())).is_some());
    assert!(svg_tree(&gzip(nested_svg(2000).as_bytes())).is_none());
    let mut bytes = nested_svg(0).into_bytes();
    bytes.resize(MAX_SVG_BYTES + 1, b' ');
    let compressed = gzip(&bytes);
    assert!(compressed.len() < MAX_SVG_BYTES);
    assert!(svg_tree(&compressed).is_none());
    assert!(svg_tree(b"\x1f\x8bnot gzip").is_none());
}

/// Deeply nested SVG data images are omitted without bypassing the depth preflight.
#[test]
fn nested_svg_data_images_cannot_bypass_the_depth_limit() {
    let nested = nested_svg(2000);
    let encoded = nested.bytes().map(|b| format!("%{b:02X}")).collect::<String>();
    for mime in ["image/svg+xml", "text/plain"] {
        let svg = format!(
            r#"<svg xmlns="http://www.w3.org/2000/svg" width="10" height="10"><image width="10" height="10" href="data:{mime},{encoded}"/></svg>"#
        );
        // As with other unreadable data images, the parent survives without the image.
        assert!(svg_tree(svg.as_bytes()).unwrap().root().children().is_empty());
    }
}

#[test]
fn svg_sniff_size_and_render() {
    let b = SVG.as_bytes();
    assert!(is_svg(b));
    assert_eq!(mime(b), "image/svg+xml");
    assert_eq!(natural_size(b), Some((150.0, 75.0)));
    let (px, w, h) = render_svg(b, 400).unwrap();
    assert_eq!((w, h), (400, 200));
    // Red on the left; the text drew some ink on the right.
    assert_eq!(&px[(100 * 400 + 100) * 4..][..4], &[255, 0, 0, 255]);
    assert!(px.chunks(4).skip(220).step_by(7).any(|p| p[3] > 128 && p[0] < 100), "text pixels");
    assert!(!is_svg(b"\x89PNG"));
}

#[test]
fn svg_images_come_only_from_data_urls() {
    let dir = std::env::temp_dir().join(format!("dc-svg-href-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let file = dir.join("red.png");
    image::RgbaImage::from_pixel(4, 4, image::Rgba([255, 0, 0, 255])).save(&file).unwrap();
    let blue = "data:image/svg+xml,%3Csvg xmlns='http://www.w3.org/2000/svg' width='10' height='10'%3E%3Crect width='10' height='10' fill='blue'/%3E%3C/svg%3E";
    let svg = format!(
        r#"<svg xmlns="http://www.w3.org/2000/svg" width="30" height="10">
<image x="0" y="0" width="10" height="10" href="{}"/>
<image x="10" y="0" width="10" height="10" href="{blue}"/>
<image x="20" y="0" width="10" height="10" href="/etc/hostname"/>
</svg>"#,
        file.display()
    );
    // The image nodes usvg kept: only the data URL's (PDF export embeds every one).
    fn images(g: &usvg::Group) -> usize {
        g.children()
            .iter()
            .map(|n| match n {
                usvg::Node::Group(g) => images(g),
                usvg::Node::Image(_) => 1,
                _ => 0,
            })
            .sum()
    }
    assert_eq!(images(svg_tree(svg.as_bytes()).unwrap().root()), 1, "files the SVG names are not read");
    let (px, w, _) = render_svg(svg.as_bytes(), 30).unwrap();
    let at = |x: usize, y: usize| px[(y * w as usize + x) * 4..][..4].to_vec();
    assert_eq!(at(15, 5), [0, 0, 255, 255], "a data URL is drawn");
    let _ = std::fs::remove_dir_all(&dir);
}

/// The family of the font that sets each text glyph of `svg`, and the glyph's id.
fn glyph_families(svg: &str) -> Vec<(String, u16)> {
    fn walk(g: &usvg::Group, tree: &usvg::Tree, out: &mut Vec<(String, u16)>) {
        for n in g.children() {
            match n {
                usvg::Node::Group(g) => walk(g, tree, out),
                usvg::Node::Text(t) => {
                    for g in t.layouted().iter().flat_map(|s| &s.positioned_glyphs) {
                        out.extend(tree.fontdb().face(g.font).map(|f| (f.families[0].0.clone(), g.id.0)));
                    }
                }
                _ => {}
            }
        }
    }
    let tree = svg_tree(svg.as_bytes()).unwrap();
    let mut out = Vec::new();
    walk(tree.root(), &tree, &mut out);
    out
}

#[test]
fn svg_text_uses_the_shared_fonts() {
    let font = designcraft_fonts::testing::font_with("DC Test SVG Hangul", &['한', '글']).unwrap();
    designcraft_fonts::FontDb::global().add_font(font);
    let text = |attrs: &str, body: &str| {
        format!(r#"<svg xmlns="http://www.w3.org/2000/svg" width="100" height="40"><text y="30" font-size="20"{attrs}>{body}</text></svg>"#)
    };
    let named = r#" font-family="DC Test SVG Hangul""#;
    // A family named in the SVG.
    let set = glyph_families(&text(named, "한글"));
    assert!(set.len() == 2 && set.iter().all(|(f, gid)| f == "DC Test SVG Hangul" && *gid != 0), "{set:?}");
    // Characters the bundled default lacks: a shared face that has them, not the default.
    let set = glyph_families(&text("", "한글"));
    assert!(set.len() == 2 && set.iter().all(|(f, gid)| f != "Source Sans 3" && *gid != 0), "{set:?}");
}

#[test]
fn svg_family_names_match_in_any_case() {
    let font = designcraft_fonts::testing::font_with("DC Test SVG Casing", &['A', 'b']).unwrap();
    designcraft_fonts::FontDb::global().add_font(font);
    let text = |family: &str, attrs: &str| {
        format!(
            r#"<svg xmlns="http://www.w3.org/2000/svg" width="100" height="40"><text y="30" font-size="20" font-family="{family}"{attrs}>Ab</text></svg>"#
        )
    };
    for family in ["DC Test SVG Casing", "dc test svg casing", "DC TEST svg Casing", "Nope, dc test SVG casing, serif"] {
        let set = glyph_families(&text(family, ""));
        assert!(set.len() == 2 && set.iter().all(|(f, gid)| f == "DC Test SVG Casing" && *gid != 0), "{family}: {set:?}");
    }
    // Weight and style still reach the query: there's only a regular face, so it sets the text.
    let set = glyph_families(&text("dc test svg casing", r#" font-weight="700" font-style="italic""#));
    assert!(set.len() == 2 && set.iter().all(|(f, _)| f == "DC Test SVG Casing"), "{set:?}");
    // A family nobody has still falls to the bundled serif.
    let set = glyph_families(&text("DC Test SVG Nobody", ""));
    assert!(set.len() == 2 && set.iter().all(|(f, _)| f == "Source Serif 4"), "{set:?}");
}

/// The PostScript name of the face that sets the text of `svg` (one face for all of it).
fn text_face(svg_attrs: &str) -> String {
    let svg = format!(r#"<svg xmlns="http://www.w3.org/2000/svg" width="100" height="40"><text y="30" font-size="20"{svg_attrs}>ab</text></svg>"#);
    let tree = svg_tree(svg.as_bytes()).unwrap();
    let mut faces = Vec::new();
    for n in tree.root().children() {
        if let usvg::Node::Text(t) = n {
            for g in t.layouted().iter().flat_map(|s| &s.positioned_glyphs) {
                faces.extend(tree.fontdb().face(g.font).map(|f| f.post_script_name.clone()));
            }
        }
    }
    faces.dedup();
    assert_eq!(faces.len(), 1, "{faces:?}");
    faces.remove(0)
}

#[test]
fn svg_text_finds_fonts_by_postscript_name() {
    // Bundled, as Illustrator's SVG export names it; any case.
    assert_eq!(text_face(r#" font-family="'SourceSerif4-Semibold'""#), "SourceSerif4-Semibold");
    assert_eq!(text_face(r#" font-family="sourceserif4-semibold""#), "SourceSerif4-Semibold");
    // A shared font (as an installed one would be).
    let font = designcraft_fonts::testing::font_with("DC Test SVG PostScript", &['a', 'b']).unwrap();
    designcraft_fonts::FontDb::global().add_font(font);
    assert_eq!(text_face(r#" font-family="DCTestSVGPostScript-Regular""#), "DCTestSVGPostScript-Regular");
    // The name picks that face: font-weight and font-style don't change it.
    assert_eq!(text_face(r#" font-family="SourceSerif4-Semibold" font-weight="bold" font-style="italic""#), "SourceSerif4-Semibold");
    // A family listed before it still comes first; one after it is a fallback.
    assert_eq!(text_face(r#" font-family="'Source Sans 3', SourceSerif4-Bold""#), "SourceSans3-Regular");
    assert_eq!(text_face(r#" font-family="NoSuch-Font, SourceSerif4-Bold, 'Source Sans 3'""#), "SourceSerif4-Bold");
    // Family names work as before.
    assert_eq!(text_face(r#" font-family="'Source Serif 4'" font-weight="bold""#), "SourceSerif4-Bold");
    // An unknown name falls back as before (the serif default).
    assert_eq!(text_face(r#" font-family="NoSuch-Font""#), text_face(r#" font-family="No Such Font""#));
    assert_eq!(text_face(r#" font-family="NoSuch-Font""#), "SourceSerif4-Regular");
}

/// A minimal Photoshop file: 2×1 RGB, no layers, raw merged image.
fn tiny_psd() -> Vec<u8> {
    let mut b = Vec::new();
    b.extend(b"8BPS");
    b.extend(1u16.to_be_bytes());
    b.extend([0u8; 6]);
    b.extend(3u16.to_be_bytes()); // channels
    b.extend(1u32.to_be_bytes()); // height
    b.extend(2u32.to_be_bytes()); // width
    b.extend(8u16.to_be_bytes()); // depth
    b.extend(3u16.to_be_bytes()); // RGB
    b.extend(0u32.to_be_bytes()); // colour mode data
    b.extend(0u32.to_be_bytes()); // image resources
    b.extend(0u32.to_be_bytes()); // layer and mask info
    b.extend(0u16.to_be_bytes()); // raw
    b.extend([255, 0, 0, 255, 0, 0]); // R, G, B planes: a red pixel, then a green one
    b
}

#[test]
fn psd_composite_decodes() {
    let b = tiny_psd();
    assert_eq!(mime(&b), "image/vnd.adobe.photoshop");
    assert_eq!(pixel_size(&b), Some((2, 1)));
    let img = decode_rgba(&b).unwrap();
    assert_eq!(img.get_pixel(0, 0).0, [255, 0, 0, 255]);
    assert_eq!(img.get_pixel(1, 0).0, [0, 255, 0, 255]);
}

fn tiff_of<C: tiff::encoder::colortype::ColorType>(w: u32, h: u32, px: &[C::Inner], alpha: bool) -> Vec<u8>
where
    [C::Inner]: tiff::encoder::TiffValue,
{
    let mut b = Vec::new();
    let mut enc = tiff::encoder::TiffEncoder::new(std::io::Cursor::new(&mut b)).unwrap();
    let mut img = enc.new_image::<C>(w, h).unwrap();
    if alpha {
        // CMYKA8 already has five samples: only the tag that says the fifth is alpha.
        img.encoder().write_tag(tiff::tags::Tag::ExtraSamples, &[2u16][..]).unwrap();
    }
    img.write_data(px).unwrap();
    b
}

/// A placed EPS is measured by its bounding box in points and drawn through its proxy; the
/// bytes stay the EPS.
#[test]
fn eps_draws_through_its_proxy_at_its_bounding_box_size() {
    let eps = b"%!PS-Adobe-3.1 EPSF-3.0\n%%BoundingBox: 0 0 4 2\n%%EndComments\n/rf { rectfill } bind def\n0 0 0 1 setcmykcolor\n0 0 2 2 rf\n%%EOF\n";
    assert_eq!(natural_size(eps), Some((4.0, 2.0)));
    let img = decode_rgba(eps).expect("drawn");
    assert_eq!(img.dimensions(), (32, 16));
    assert_eq!(img.get_pixel(1, 1).0, [0, 0, 0, 255], "the rectangle");
    assert_eq!(img.get_pixel(30, 1).0[3], 0, "outside it, transparent");
}

/// CMYK TIFFs read back as their ink values; RGB ones aren't CMYK.
#[test]
fn cmyk_tiff_keeps_inks() {
    use tiff::encoder::colortype::{CMYK8, CMYK16, CMYKA8, RGB8};
    // 2×1: 100% K, then C0 M100 Y100 K0.
    let px = [0, 0, 0, 255, 0, 255, 255, 0];
    let b = tiff_of::<CMYK8>(2, 1, &px, false);
    assert_eq!(mime(&b), "image/tiff");
    assert!(is_cmyk_tiff(&b));
    assert_eq!(decode_cmyk_tiff(&b), Some(CmykRaster { width: 2, height: 1, cmyk: px.to_vec(), alpha: None }));
    // The screen gets RGB through the working CMYK space.
    let raster = CmykRaster { width: 2, height: 1, cmyk: px.to_vec(), alpha: None };
    let want = colour::to_display_rgba(colour::Pixels::Cmyk(raster), None, &colour::DisplayColour::active()).unwrap();
    assert_eq!(decode_rgba(&b).unwrap(), want);

    let b = tiff_of::<CMYK16>(2, 1, &[0, 0, 0, 65_535, 0, 32_896, 65_535, 0], false);
    assert_eq!(decode_cmyk_tiff(&b).unwrap().cmyk, [0, 0, 0, 255, 0, 128, 255, 0]);

    let b = tiff_of::<CMYKA8>(2, 1, &[0, 0, 0, 255, 255, 0, 255, 255, 0, 0], true);
    let r = decode_cmyk_tiff(&b).unwrap();
    assert_eq!((r.cmyk, r.alpha), (px.to_vec(), Some(vec![255, 0])));

    let rgb = tiff_of::<RGB8>(1, 1, &[255, 0, 0], false);
    assert!(!is_cmyk_tiff(&rgb));
    assert_eq!(decode_cmyk_tiff(&rgb), None);
    assert!(!is_cmyk_tiff(b"II*\0"), "truncated");
    assert_eq!(decode_cmyk_tiff(&b[..b.len() / 2]), None, "truncated");
}

/// Indexed colour (PhotometricInterpretation 3), which the `tiff` encoder has no type for.
struct Palette<const BITS: u16>;
impl<const BITS: u16> tiff::encoder::colortype::ColorType for Palette<BITS> {
    type Inner = u8;
    const TIFF_VALUE: tiff::tags::PhotometricInterpretation = tiff::tags::PhotometricInterpretation::RGBPalette;
    const BITS_PER_SAMPLE: &[u16] = &[BITS];
    const SAMPLE_FORMAT: &[tiff::tags::SampleFormat] = &[tiff::tags::SampleFormat::Uint];
    fn horizontal_predict(row: &[u8], result: &mut Vec<u8>) {
        result.extend_from_slice(row);
    }
}

/// A palette TIFF as Photoshop and Pillow write it: 16-bit ColorMap entries (reds, then
/// greens, then blues), each 8-bit value times 256.
fn palette_tiff<const BITS: u16>(w: u32, h: u32, px: &[u8], colours: &[[u8; 3]], lzw: bool) -> Vec<u8> {
    let n = 1usize << BITS;
    let mut map = vec![0u16; 3 * n];
    for (i, c) in colours.iter().enumerate() {
        for (ch, &v) in c.iter().enumerate() {
            map[ch * n + i] = u16::from(v) << 8;
        }
    }
    let mut b = Vec::new();
    let mut enc = tiff::encoder::TiffEncoder::new(std::io::Cursor::new(&mut b)).unwrap();
    if lzw {
        enc = enc.with_compression(tiff::encoder::Compression::Lzw);
    }
    let mut img = enc.new_image::<Palette<BITS>>(w, h).unwrap();
    img.encoder().write_tag(tiff::tags::Tag::ColorMap, &map[..]).unwrap();
    img.write_data(px).unwrap();
    b
}

/// Indexed-colour TIFFs (an old GIF logo saved from Photoshop) are sized and drawn through
/// their palette, uncompressed or LZW, 8- or 4-bit.
#[test]
fn palette_tiff_decodes_through_its_colormap() {
    let colours = [[0, 128, 255], [255, 0, 0], [10, 20, 30]];
    for lzw in [false, true] {
        let b = palette_tiff::<8>(3, 1, &[2, 0, 1], &colours, lzw);
        assert_eq!(pixel_size(&b), Some((3, 1)), "lzw {lzw}");
        let img = decode_rgba(&b).expect("decodes");
        let px: Vec<_> = img.pixels().map(|p| p.0).collect();
        assert_eq!(px, [[10, 20, 30, 255], [0, 128, 255, 255], [255, 0, 0, 255]], "lzw {lzw}");
    }
    // Two indices per byte, high nibble first: 2, 1 | 0, 2 (the encoder counts samples, not
    // bytes, so the strip carries two spare bytes the decoder doesn't read).
    let b = palette_tiff::<4>(2, 2, &[0x21, 0x02, 0, 0], &colours, false);
    assert_eq!(pixel_size(&b), Some((2, 2)));
    let px: Vec<_> = decode_rgba(&b).expect("decodes").pixels().map(|p| p.0).collect();
    assert_eq!(px, [[10, 20, 30, 255], [255, 0, 0, 255], [0, 128, 255, 255], [10, 20, 30, 255]]);
}

#[test]
fn cmyk_tiff_with_alpha_reports_its_pixel_size() {
    use tiff::encoder::colortype::CMYKA8;
    let b = tiff_of::<CMYKA8>(2, 1, &[0, 0, 0, 255, 255, 0, 0, 0, 0, 0], true);
    assert_eq!(pixel_size(&b), Some((2, 1)));
}

#[test]
fn bmp_decodes() {
    let mut bmp = Vec::new();
    image::RgbaImage::from_pixel(3, 2, image::Rgba([0, 0, 255, 255])).write_to(&mut std::io::Cursor::new(&mut bmp), image::ImageFormat::Bmp).unwrap();
    assert_eq!(mime(&bmp), "image/bmp");
    assert_eq!(pixel_size(&bmp), Some((3, 2)));
    assert_eq!(decode_rgba(&bmp).unwrap().get_pixel(1, 1).0, [0, 0, 255, 255]);
}

/// A 1-bit gray PNG of `side` x `side` blank pixels, written row by row so the test stays small.
fn blank_png(side: u32) -> Vec<u8> {
    use std::io::Write;
    let chunk = |out: &mut Vec<u8>, kind: &[u8; 4], data: &[u8]| {
        out.extend((data.len() as u32).to_be_bytes());
        let mut crc = flate2::Crc::new();
        crc.update(kind);
        crc.update(data);
        out.extend(kind);
        out.extend(data);
        out.extend(crc.sum().to_be_bytes());
    };
    let row = vec![0u8; 1 + side.div_ceil(8) as usize];
    let mut z = flate2::write::ZlibEncoder::new(Vec::new(), flate2::Compression::fast());
    for _ in 0..side {
        z.write_all(&row).unwrap();
    }
    let mut ihdr = [side.to_be_bytes(), side.to_be_bytes()].concat();
    ihdr.extend([1, 0, 0, 0, 0]);
    let mut png = b"\x89PNG\r\n\x1a\n".to_vec();
    chunk(&mut png, b"IHDR", &ihdr);
    chunk(&mut png, b"IDAT", &z.finish().unwrap());
    chunk(&mut png, b"IEND", &[]);
    png
}

/// A small file that expands past 1 GiB of RGBA is refused from its header (#1586).
#[test]
fn rasters_past_the_rgba_budget_are_refused_before_decoding() {
    let png = blank_png(16_385);
    assert!(png.len() < 1 << 20, "{} bytes", png.len());
    assert_eq!(pixel_size(&png), Some((16_385, 16_385)));
    assert_eq!(natural_size(&png), Some((16_385.0, 16_385.0)));
    assert!(too_large_to_decode(&png));
    assert!(decode_rgba(&png).is_none());
    assert!(!too_large_to_decode(&blank_png(16_384)));
    let small = decode_rgba(&blank_png(9)).expect("decodes");
    assert_eq!(small.get_pixel(8, 8).0, [0, 0, 0, 255]);
}

/// A GIF whose logical screen is `side` x `side`, holding one black 1x1 frame.
fn gif_screen(side: u16) -> Vec<u8> {
    let mut gif = b"GIF89a".to_vec();
    gif.extend(side.to_le_bytes());
    gif.extend(side.to_le_bytes());
    // Two-colour global table: black, white.
    gif.extend([0x80, 0, 0, 0, 0, 0, 255, 255, 255]);
    // Frame at the origin; LZW minimum code size 2, codes clear, 0, end; trailer.
    gif.extend([0x2C, 0, 0, 0, 0, 1, 0, 1, 0, 0, 2, 2, 0x44, 0x01, 0, 0x3B]);
    gif
}

/// The header of an uncompressed 8-bit CMYK TIFF of `side` x `side` pixels, without the pixels.
fn cmyk_tiff_header(side: u32) -> Vec<u8> {
    let entries: [(u16, u16, u32, u32); 9] = [
        (256, 4, 1, side),            // width
        (257, 4, 1, side),            // height
        (258, 3, 4, 122),             // bits per sample, stored after the IFD
        (259, 3, 1, 1),               // no compression
        (262, 3, 1, 5),               // separated (CMYK)
        (273, 4, 1, 130),             // strip offset
        (277, 3, 1, 4),               // samples per pixel
        (278, 4, 1, side),            // rows per strip
        (279, 4, 1, side * side * 4), // strip byte count
    ];
    let mut t = b"II*\0".to_vec();
    t.extend(8u32.to_le_bytes());
    t.extend(9u16.to_le_bytes());
    for (tag, kind, count, value) in entries {
        t.extend(tag.to_le_bytes());
        t.extend(kind.to_le_bytes());
        t.extend(count.to_le_bytes());
        t.extend(value.to_le_bytes());
    }
    t.extend(0u32.to_le_bytes());
    assert_eq!(t.len(), 122);
    t.extend([8u16; 4].iter().flat_map(|b| b.to_le_bytes()));
    t
}

/// GIF tones and CMYK inks check the RGBA budget too (#1586).
#[test]
fn gif_tone_and_cmyk_inks_check_the_rgba_budget() {
    let gif = gif_screen(16_385);
    assert!(too_large_to_decode(&gif));
    assert_eq!(tone(&gif), None);
    assert_eq!(tone(&gif_screen(2)), Some(Tone::Bitmap));
    let tiff = cmyk_tiff_header(16_385);
    assert!(is_cmyk_tiff(&tiff));
    assert_eq!(pixel_size(&tiff), Some((16_385, 16_385)));
    assert!(too_large_to_decode(&tiff));
    assert_eq!(decode_cmyk_tiff(&tiff), None);
}
