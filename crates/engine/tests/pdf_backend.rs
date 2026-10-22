//! Synthetic placement/export regressions for PDF backend updates.

use serde_json::json;

use designcraft_engine::Session;
use designcraft_engine::cmd::{base64_decode, base64_encode};

fn pdf(objects: &[String]) -> Vec<u8> {
    let mut bytes = b"%PDF-1.7\n".to_vec();
    let mut offsets = vec![0];
    for (i, object) in objects.iter().enumerate() {
        offsets.push(bytes.len());
        bytes.extend_from_slice(format!("{} 0 obj\n{object}\nendobj\n", i + 1).as_bytes());
    }
    let xref = bytes.len();
    bytes.extend_from_slice(format!("xref\n0 {}\n0000000000 65535 f\r\n", offsets.len()).as_bytes());
    for offset in offsets.iter().skip(1) {
        bytes.extend_from_slice(format!("{offset:010} 00000 n\r\n").as_bytes());
    }
    bytes.extend_from_slice(format!("trailer\n<< /Size {} /Root 1 0 R >>\nstartxref\n{xref}\n%%EOF\n", offsets.len()).as_bytes());
    bytes
}

fn layered_pdf(second_name: &str) -> Vec<u8> {
    layered_pdf_with_box(second_name, "")
}

fn layered_pdf_with_box(second_name: &str, page_box: &str) -> Vec<u8> {
    let content = "/OC /a BDC 0 0 1 rg 10 10 35 80 re f EMC /OC /b BDC 1 0 0 rg 55 10 35 80 re f EMC";
    pdf(&[
        "<< /Type /Catalog /Pages 2 0 R /OCProperties << /OCGs [5 0 R 6 0 R] /D << /Order [5 0 R 6 0 R] >> >> >>".into(),
        "<< /Type /Pages /Kids [3 0 R] /Count 1 >>".into(),
        format!(
            "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 100 100] {page_box} /Resources << /Properties << /a 5 0 R /b 6 0 R >> >> /Contents 4 0 R >>"
        ),
        format!("<< /Length {} >>\nstream\n{content}\nendstream", content.len()),
        "<< /Type /OCG /Name (Blue) >>".into(),
        format!("<< /Type /OCG /Name ({second_name}) >>"),
    ])
}

fn rendered(bytes: &[u8]) -> designcraft_render::Rendered {
    let pm = designcraft_render::render_pdf_page(bytes, 0, 100).expect("render synthetic PDF");
    designcraft_render::Rendered {
        width: pm.width().into(),
        height: pm.height().into(),
        pixels: pm.data().iter().flat_map(|p| [p.r, p.g, p.b, p.a]).collect(),
    }
}

// Optional local evidence output; no binary fixtures are checked in.
fn evidence(name: &str, bytes: &[u8]) {
    if let Some(dir) = std::env::var_os("CRAFT_PDF_EVIDENCE_DIR") {
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(std::path::Path::new(&dir).join(name), bytes).unwrap();
    }
}

#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
#[cfg_attr(not(target_arch = "wasm32"), test)]
fn placed_pdf_preserves_transparent_background_and_layer_choices() {
    let bytes = layered_pdf("Red");
    let img = rendered(&bytes);
    assert_eq!(img.pixel(2, 2), [0, 0, 0, 0]);
    assert_eq!(img.pixel(25, 50), [0, 0, 255, 255]);
    assert_eq!(img.pixel(75, 50), [255, 0, 0, 255]);
    let hidden = designcraft_render::pdf_hide_layers(&bytes, &["Red".into()]).unwrap();
    let img = rendered(&hidden);
    assert_eq!(img.pixel(75, 50), [0, 0, 0, 0]);
    assert_eq!(img.pixel(25, 50), [0, 0, 255, 255]);
    evidence("layers-hidden.png", &img.to_png());
    evidence("layers-source.pdf", &bytes);
}

#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
#[cfg_attr(not(target_arch = "wasm32"), test)]
fn duplicate_layer_names_keep_name_based_hiding() {
    let bytes = layered_pdf("Blue");
    assert_eq!(designcraft_render::pdf_layers(&bytes).len(), 2);
    let hidden = designcraft_render::pdf_hide_layers(&bytes, &["Blue".into()]).unwrap();
    let img = rendered(&hidden);
    assert_eq!(img.pixel(25, 50), [0, 0, 0, 0]);
    assert_eq!(img.pixel(75, 50), [0, 0, 0, 0]);
    // A second render of the original must not inherit the hidden configuration.
    assert_eq!(rendered(&bytes).pixel(25, 50), [0, 0, 255, 255]);
}

#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
#[cfg_attr(not(target_arch = "wasm32"), test)]
fn crop_box_and_rotation_keep_placed_dimensions() {
    let bytes = pdf(&[
        "<< /Type /Catalog /Pages 2 0 R >>".into(),
        "<< /Type /Pages /Kids [3 0 R] /Count 1 >>".into(),
        "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 120 100] /CropBox [10 20 110 80] /Rotate 90 >>".into(),
    ]);
    assert_eq!(designcraft_render::pdf_page_count(&bytes), Some(1));
    assert_eq!(designcraft_render::pdf_page_size(&bytes, 0), Some((60.0, 100.0)));
    let img = rendered(&bytes);
    assert_eq!((img.width, img.height), (60, 100));
    assert_eq!(img.pixel(20, 20), [0, 0, 0, 0]);
}

fn exported(hidden: bool) -> Vec<u8> {
    let mut session = Session::new();
    session.execute("file.new", &json!({"width": 100, "height": 100, "pages": 1})).unwrap();
    session.execute("edit.transparencyBlendSpace", &json!({"space": "rgb"})).unwrap();
    session.execute("frame.create", &json!({"rect": [0, 0, 100, 100]})).unwrap();
    session.execute("object.fill", &json!({"swatch": "[Black]"})).unwrap();
    let placed = session
        .execute(
            "file.place",
            &json!({
                "base64": base64_encode(&layered_pdf("Red")), "name": "layers.pdf", "x": 0, "y": 0, "width": 100
            }),
        )
        .unwrap();
    if hidden {
        session.execute("object.layerOptions", &json!({"ids": [placed["id"]], "hidden": ["Red"]})).unwrap();
    }
    let report = session.execute("file.exportPdf", &json!({"compress": false})).unwrap();
    let bytes = base64_decode(report["base64"].as_str().unwrap());
    if hidden {
        assert!(report["warnings"].to_string().contains("hidden layers"));
    }
    bytes
}

#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
#[cfg_attr(not(target_arch = "wasm32"), test)]
fn hidden_layer_export_composites_over_underlying_artwork() {
    let bytes = exported(true);
    let img = rendered(&bytes);
    let right = img.pixel(75, 50);
    assert!(right[..3].iter().all(|c| *c < 45), "hidden area must show the black backdrop, not opaque white: {right:?}");
    let left = img.pixel(25, 50);
    assert!(left[2] > 180 && left[0] < 80, "visible blue layer: {left:?}");
    evidence("hidden-export.pdf", &bytes);
    evidence("hidden-export.png", &img.to_png());
}

#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
#[cfg_attr(not(target_arch = "wasm32"), test)]
fn ordinary_placed_pages_still_embed_as_vectors() {
    use hayro_syntax::object::Name;
    let bytes = exported(false);
    let parsed = hayro_syntax::Pdf::new(bytes.clone()).unwrap();
    let subtypes: Vec<_> = parsed
        .objects()
        .into_iter()
        .filter_map(|o| o.into_stream())
        .filter_map(|s| s.dict().get::<Name>("Subtype").map(|n| n.as_str().to_owned()))
        .collect();
    assert!(subtypes.iter().any(|s| s == "Form"), "placed PDF form: {subtypes:?}");
    assert!(!subtypes.iter().any(|s| s == "Image"), "no rasterization: {subtypes:?}");
    let img = rendered(&bytes);
    let right = img.pixel(75, 50);
    assert!(right[0] > 180 && right[1] < 80, "visible red vector layer: {right:?}");
    evidence("vector-export.pdf", &bytes);
    evidence("vector-export.png", &img.to_png());
}

#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
#[cfg_attr(not(target_arch = "wasm32"), test)]
fn oversized_image_does_not_silently_disappear_from_layered_export() {
    // Just above the backend's 2^28-pixel cap. The stream is one byte; no large
    // fixture or large image allocation is needed to exercise the refusal.
    let content = "/OC /a BDC 0 0 1 rg 10 10 35 80 re f EMC q 100 0 0 100 0 0 cm /Im Do Q";
    let bytes = pdf(&[
        "<< /Type /Catalog /Pages 2 0 R /OCProperties << /OCGs [6 0 R] /D << /Order [6 0 R] >> >> >>".into(),
        "<< /Type /Pages /Kids [3 0 R] /Count 1 >>".into(),
        "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 100 100] /Resources << /Properties << /a 6 0 R >> /XObject << /Im 5 0 R >> >> /Contents 4 0 R >>".into(),
        format!("<< /Length {} >>\nstream\n{content}\nendstream", content.len()),
        "<< /Type /XObject /Subtype /Image /Width 32769 /Height 8192 /ColorSpace /DeviceGray /BitsPerComponent 8 /Length 1 >>\nstream\nX\nendstream".into(),
        "<< /Type /OCG /Name (Blue) >>".into(),
    ]);
    let mut session = Session::new();
    session.execute("file.new", &json!({"width": 100, "height": 100, "pages": 1})).unwrap();
    let placed = session
        .execute(
            "file.place",
            &json!({
                "base64": base64_encode(&bytes), "name": "oversized-scan.pdf", "x": 0, "y": 0, "width": 100
            }),
        )
        .unwrap();
    session.execute("object.layerOptions", &json!({"ids": [placed["id"]], "hidden": ["Blue"]})).unwrap();
    let before = session.execute("file.serialize", &json!({})).unwrap();
    let result = session.execute("file.exportPdf", &json!({}));
    assert!(result.is_err(), "export must report the image omitted by the backend's resource limit");
    assert_eq!(session.execute("file.serialize", &json!({})).unwrap(), before, "a failed export must leave the document unchanged");
    let error = result.unwrap_err().to_string();
    assert!(error.contains("oversized-scan.pdf") && error.contains("image"), "{error}");
}

#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
#[cfg_attr(not(target_arch = "wasm32"), test)]
fn failed_layer_rewrite_aborts_export_instead_of_revealing_hidden_content() {
    // A stored layer choice can outlive relinking to a PDF with no layers.
    let bytes = pdf(&[
        "<< /Type /Catalog /Pages 2 0 R >>".into(),
        "<< /Type /Pages /Kids [3 0 R] /Count 1 >>".into(),
        "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 100 100] >>".into(),
    ]);
    assert!(designcraft_render::render_pdf_page(&bytes, 0, 100).is_some());
    assert!(designcraft_render::pdf_hide_layers(&bytes, &["Red".into()]).is_none());
    let mut session = Session::new();
    session.execute("file.new", &json!({"width": 100, "height": 100, "pages": 1})).unwrap();
    let placed =
        session.execute("file.place", &json!({"base64": base64_encode(&bytes), "name": "missing-layers.pdf", "x": 0, "y": 0, "width": 100})).unwrap();
    session.execute("object.layerOptions", &json!({"ids": [placed["id"]], "hidden": ["Red"]})).unwrap();
    let before = session.execute("file.serialize", &json!({})).unwrap();
    let result = session.execute("file.exportPdf", &json!({}));
    assert!(result.is_err(), "failed layer hiding must not export the original visible layers");
    let error = result.unwrap_err().to_string();
    assert!(error.contains("missing-layers.pdf") && error.contains("layer"), "{error}");
    assert_eq!(session.execute("file.serialize", &json!({})).unwrap(), before);
}

#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
#[cfg_attr(not(target_arch = "wasm32"), test)]
fn placed_standard_font_text_renders_ink_on_a_transparent_background() {
    let content = "BT /F1 18 Tf 10 40 Td (PDF text) Tj ET";
    let bytes = pdf(&[
        "<< /Type /Catalog /Pages 2 0 R >>".into(),
        "<< /Type /Pages /Kids [3 0 R] /Count 1 >>".into(),
        "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 100 100] /Resources << /Font << /F1 5 0 R >> >> /Contents 4 0 R >>".into(),
        format!("<< /Length {} >>\nstream\n{content}\nendstream", content.len()),
        "<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica >>".into(),
    ]);
    let img = rendered(&bytes);
    let ink = (40..65).flat_map(|y| (5..95).map(move |x| (x, y))).filter(|(x, y)| img.pixel(*x, *y)[3] > 100).count();
    assert!(ink > 100, "standard-font glyphs must paint visible ink: {ink}");
    assert_eq!(img.pixel(2, 2), [0, 0, 0, 0]);
    assert_eq!(img.pixel(50, 80), [0, 0, 0, 0]);
    assert!(designcraft_render::pdf_page_png_checked(&bytes, 0, 100, &[], None).is_ok());
    evidence("standard-font-text.png", &img.to_png());
}

#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
#[cfg_attr(not(target_arch = "wasm32"), test)]
fn unavailable_placed_page_reports_the_page_and_recovery_action() {
    let error = designcraft_render::pdf_page_png_checked(&layered_pdf("Red"), 2, 100, &["Red".into()], None).unwrap_err();
    assert!(matches!(error, designcraft_render::PdfRasterError::MissingPage { page: 3 }));
    assert!(error.to_string().contains("place the replacement PDF again"));
}

#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
#[cfg_attr(not(target_arch = "wasm32"), test)]
fn hidden_layers_work_with_an_xref_stream_and_compressed_catalog() {
    // Reuse the tiny synthetic objects, but put the catalog in an object stream
    // and index everything with a binary cross-reference stream.
    let classic = String::from_utf8(layered_pdf("Red")).unwrap();
    let objects: Vec<&str> = (1..=6).map(|i| classic.split_once(&format!("{i} 0 obj\n")).unwrap().1.split_once("\nendobj").unwrap().0).collect();
    let mut bytes = b"%PDF-1.7\n".to_vec();
    let mut offsets = [0u32; 9];
    for (i, object) in objects.iter().enumerate().skip(1) {
        offsets[i + 1] = u32::try_from(bytes.len()).unwrap();
        bytes.extend_from_slice(format!("{} 0 obj\n{object}\nendobj\n", i + 1).as_bytes());
    }
    offsets[7] = u32::try_from(bytes.len()).unwrap();
    let catalog = format!("1 0 {}", objects[0]);
    bytes.extend_from_slice(
        format!("7 0 obj\n<< /Type /ObjStm /N 1 /First 4 /Length {} >>\nstream\n{catalog}\nendstream\nendobj\n", catalog.len()).as_bytes(),
    );
    offsets[8] = u32::try_from(bytes.len()).unwrap();
    let mut entries = Vec::new();
    for (i, offset) in offsets.iter().enumerate() {
        let (kind, field, generation): (u8, u32, u16) = match i {
            0 => (0, 0, 65535),
            1 => (2, 7, 0),
            _ => (1, *offset, 0),
        };
        entries.push(kind);
        entries.extend_from_slice(&field.to_be_bytes());
        entries.extend_from_slice(&generation.to_be_bytes());
    }
    bytes.extend_from_slice(format!("8 0 obj\n<< /Type /XRef /Size 9 /Root 1 0 R /W [1 4 2] /Length {} >>\nstream\n", entries.len()).as_bytes());
    bytes.extend_from_slice(&entries);
    bytes.extend_from_slice(format!("\nendstream\nendobj\nstartxref\n{}\n%%EOF\n", offsets[8]).as_bytes());
    assert_eq!(rendered(&bytes).pixel(75, 50), [255, 0, 0, 255]);
    let hidden = designcraft_render::pdf_hide_layers(&bytes, &["Red".into()]).unwrap();
    let img = rendered(&hidden);
    assert_eq!(img.pixel(75, 50), [0, 0, 0, 0]);
    assert_eq!(img.pixel(25, 50), [0, 0, 255, 255]);
    assert!(designcraft_render::pdf_page_png_checked(&bytes, 0, 100, &["Red".into()], None).is_ok());
}

#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
#[cfg_attr(not(target_arch = "wasm32"), test)]
fn trailing_whitespace_does_not_prevent_layer_hiding() {
    let mut bytes = layered_pdf("Red");
    bytes.extend_from_slice(&[b' '; 96]);
    let hidden = designcraft_render::pdf_hide_layers(&bytes, &["Red".into()]).expect("trailing whitespace is accepted by the layer rewriter");
    let img = rendered(&hidden);
    assert_eq!(img.pixel(75, 50), [0, 0, 0, 0]);
    assert_eq!(img.pixel(25, 50), [0, 0, 255, 255]);
    assert!(designcraft_render::pdf_page_png_checked(&bytes, 0, 100, &["Red".into()], None).is_ok());
}

#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
#[cfg_attr(not(target_arch = "wasm32"), test)]
fn failed_content_decode_aborts_layered_export_without_changing_the_document() {
    for filter in ["FlateDecode", "DCTDecode"] {
        let bytes = pdf(&[
            "<< /Type /Catalog /Pages 2 0 R /OCProperties << /OCGs [5 0 R] /D << /Order [5 0 R] >> >> >>".into(),
            "<< /Type /Pages /Kids [3 0 R] /Count 1 >>".into(),
            "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 100 100] /Resources << /Properties << /a 5 0 R >> >> /Contents 4 0 R >>".into(),
            format!("<< /Filter /{filter} /Length 1 >>\nstream\nx\nendstream"),
            "<< /Type /OCG /Name (Blue) >>".into(),
        ]);
        let mut session = Session::new();
        session.execute("file.new", &json!({"width": 100, "height": 100, "pages": 1})).unwrap();
        let placed = session
            .execute("file.place", &json!({"base64": base64_encode(&bytes), "name": "broken-content.pdf", "x": 0, "y": 0, "width": 100}))
            .unwrap();
        session.execute("object.layerOptions", &json!({"ids": [placed["id"]], "hidden": ["Blue"]})).unwrap();
        let before = session.execute("file.serialize", &json!({})).unwrap();
        let error = session.execute("file.exportPdf", &json!({})).unwrap_err().to_string();
        assert!(error.contains("broken-content.pdf") && error.contains("content"), "{filter}: {error}");
        assert_eq!(session.execute("file.serialize", &json!({})).unwrap(), before, "{filter}");
    }
}

#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
#[cfg_attr(not(target_arch = "wasm32"), test)]
fn hidden_layer_export_keeps_the_placed_trim_box() {
    let bytes = layered_pdf_with_box("Red", "/TrimBox [0 0 50 100]");
    let mut session = Session::new();
    session.execute("file.new", &json!({"width": 50, "height": 100, "pages": 1})).unwrap();
    session.execute("edit.transparencyBlendSpace", &json!({"space": "rgb"})).unwrap();
    let placed = session
        .execute(
            "file.place",
            &json!({"base64": base64_encode(&bytes), "name": "trimmed-layers.pdf", "pdfCrop": "trim", "x": 0, "y": 0, "width": 100}),
        )
        .unwrap();
    session.execute("object.layerOptions", &json!({"ids": [placed["id"]], "hidden": ["Red"]})).unwrap();
    let before = session.execute("file.serialize", &json!({})).unwrap();
    let report = session.execute("file.exportPdf", &json!({"compress": false})).unwrap();
    let exported = base64_decode(report["base64"].as_str().unwrap());
    let img = rendered(&exported);
    assert_eq!((img.width, img.height), (50, 100));
    // Omitting the shown box squeezes the whole page into the trimmed graphic,
    // moving the blue rectangle's right edge from x=45 to x=22.5.
    let blue = img.pixel(35, 50);
    assert!(blue[2] > 180 && blue[0] < 80, "the trim box keeps the visible layer at its placed coordinates: {blue:?}");
    assert_eq!(session.execute("file.serialize", &json!({})).unwrap(), before);
    evidence("trimmed-layer-export.pdf", &exported);
}
