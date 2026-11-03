fn page(width: f32, height: f32) -> Vec<u8> {
    let content = format!("1 0 0 rg 0 0 {width} {height} re f");
    format!(
        "%PDF-1.7\n1 0 obj << /Type /Catalog /Pages 2 0 R >> endobj\n2 0 obj << /Type /Pages /Kids [3 0 R] /Count 1 >> endobj\n3 0 obj << /Type /Page /Parent 2 0 R /MediaBox [0 0 {width} {height}] /Contents 4 0 R >> endobj\n4 0 obj << /Length {} >> stream\n{content}\nendstream endobj\ntrailer << /Root 1 0 R >>\n%%EOF",
        content.len()
    )
    .into_bytes()
}

#[test]
fn thin_pdf_page_has_visible_preview_and_export_pixels() {
    let bytes = page(1000.0, 0.2);
    let preview = designcraft_render::render_pdf_page(&bytes, 0, 700).unwrap();
    assert_eq!((preview.width(), preview.height()), (700, 1));
    assert!(preview.data().iter().any(|pixel| pixel.a > 0));
    let png = designcraft_render::pdf_page_png_checked(&bytes, 0, 700, &[], None).unwrap();
    let image = image::load_from_memory(&png).unwrap().to_rgba8();
    assert_eq!(image.dimensions(), (700, 1));
    assert!(image.pixels().any(|pixel| pixel[3] > 0));
}

#[test]
fn empty_media_box_preserves_the_parsers_default_page_repair() {
    let bytes = page(0.0, 0.0);
    let missing = String::from_utf8(bytes.clone()).unwrap().replace("/MediaBox [0 0 0 0]", "");
    let size = designcraft_render::pdf_page_size(&bytes, 0).unwrap();
    assert!(size.0 > 0.0 && size.1 > 0.0);
    assert_eq!(Some(size), designcraft_render::pdf_page_size(missing.as_bytes(), 0));
    let preview = designcraft_render::render_pdf_page(&bytes, 0, 700).unwrap();
    let png = designcraft_render::pdf_page_png_checked(&bytes, 0, 700, &[], None).unwrap();
    let image = image::load_from_memory(&png).unwrap();
    assert_eq!((image.width(), image.height()), (u32::from(preview.width()), u32::from(preview.height())));
}

#[test]
fn pdf_input_limit_is_actionable_before_placement() {
    let error = designcraft_render::check_pdf_input_size(craft_pdf::MAX_INPUT_BYTES + 1).unwrap_err();
    assert!(matches!(error, designcraft_render::PdfRasterError::Input(_)));
    assert!(error.to_string().contains("input limit"));
}

#[test]
fn pdf_boxes_match_scaled_and_rotated_page_coordinates() {
    for (rotation, size, trim, media) in [
        (0, (200.0, 120.0), (20.0, 40.0, 140.0, 60.0), (-20.0, -40.0, 400.0, 200.0)),
        (90, (120.0, 200.0), (20.0, 20.0, 60.0, 140.0), (-40.0, -20.0, 200.0, 400.0)),
        (180, (200.0, 120.0), (40.0, 20.0, 140.0, 60.0), (-180.0, -40.0, 400.0, 200.0)),
        (270, (120.0, 200.0), (40.0, 40.0, 60.0, 140.0), (-40.0, -180.0, 200.0, 400.0)),
    ] {
        let bytes = String::from_utf8(page(200.0, 100.0)).unwrap().replace(
            "/MediaBox [0 0 200 100]",
            &format!("/MediaBox [0 0 200 100] /CropBox [10 20 110 80] /TrimBox [20 30 90 60] /UserUnit 2 /Rotate {rotation}"),
        );
        let bytes = bytes.as_bytes();
        assert_eq!(designcraft_render::pdf_page_size(bytes, 0), Some(size), "rotation {rotation}");
        assert_eq!(designcraft_render::pdf_page_box(bytes, 0, "crop"), Some((0.0, 0.0, size.0, size.1)), "rotation {rotation}");
        assert_eq!(designcraft_render::pdf_page_box(bytes, 0, "trim"), Some(trim), "rotation {rotation}");
        assert_eq!(designcraft_render::pdf_page_box(bytes, 0, "media"), Some(media), "rotation {rotation}");
        assert_eq!(designcraft_render::pdf_page_box(bytes, 0, "art"), Some((0.0, 0.0, size.0, size.1)), "missing box, rotation {rotation}");
    }
}

/// Three pages of one file, each with its own boxes, rotation and drawing. The third's trim box
/// lies outside its media box: it has no usable one.
fn three_boxed_pages() -> Vec<u8> {
    let pages = [
        ("/MediaBox [0 0 200 100] /TrimBox [20 10 180 90]", "1 0 0 rg 50 20 60 40 re f"),
        ("/MediaBox [0 0 300 150] /CropBox [10 10 290 140] /BleedBox [5 5 295 145] /ArtBox [40 30 200 120] /Rotate 90", "0 1 0 rg 20 20 30 30 re f"),
        ("/MediaBox [0 0 100 100] /TrimBox [200 200 300 300]", "0 0 1 rg 10 10 10 10 re f"),
    ];
    let mut pdf =
        "%PDF-1.7\n1 0 obj << /Type /Catalog /Pages 2 0 R >> endobj\n2 0 obj << /Type /Pages /Kids [3 0 R 5 0 R 7 0 R] /Count 3 >> endobj\n"
            .to_string();
    for (i, (boxes, content)) in pages.iter().enumerate() {
        let (page, stream) = (3 + 2 * i, 4 + 2 * i);
        pdf += &format!(
            "{page} 0 obj << /Type /Page /Parent 2 0 R {boxes} /Contents {stream} 0 R >> endobj\n{stream} 0 obj << /Length {} >> stream\n{content}\nendstream endobj\n",
            content.len()
        );
    }
    pdf += "trailer << /Root 1 0 R >>\n%%EOF";
    pdf.into_bytes()
}

/// Measuring several pages of a file at once (File › Place of a page range) gives each page what
/// measuring it alone does, for every Crop to box; a page the file doesn't have is `None`.
#[test]
fn placed_pages_measure_as_each_page_alone_does() {
    use designcraft_doc::PdfCrop;
    let bytes = three_boxed_pages();
    let pages = [2, 0, 1, 7];
    for crop in PdfCrop::ALL {
        let placed = designcraft_render::pdf_placed_pages(&bytes, &pages, crop).unwrap();
        assert_eq!(placed.len(), pages.len(), "{crop:?}");
        for (&page, got) in pages.iter().zip(&placed) {
            assert_eq!(got.map(|g| (g.frac, g.box_size)), designcraft_render::pdf_crop_box(&bytes, page, crop), "{crop:?}, page {page}");
            if let Some(g) = got {
                assert_eq!(Some(g.size), designcraft_render::pdf_page_size(&bytes, page), "{crop:?}, page {page}");
            }
        }
        assert!(placed[3].is_none(), "{crop:?}: there is no page 8");
        assert!(placed[1].is_some() && placed[2].is_some(), "{crop:?}");
    }
    // The rotated page's art box, in the page as it displays.
    let art = designcraft_render::pdf_placed_pages(&bytes, &[1], PdfCrop::Art).unwrap()[0].unwrap();
    assert_eq!(art.size, (130.0, 280.0));
    assert!((art.box_size.0 - 90.0).abs() < 1e-3 && (art.box_size.1 - 160.0).abs() < 1e-3, "{art:?}");
    // No usable trim box on the third page; and a file that isn't a PDF can't be measured.
    assert!(designcraft_render::pdf_placed_pages(&bytes, &[2], PdfCrop::Trim).unwrap()[0].is_none());
    assert_eq!(designcraft_render::pdf_placed_pages(b"not a PDF", &[0], PdfCrop::Crop), None);
}
