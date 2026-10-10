//! Synthetic regressions for gradients that previously aborted inside the PDF writer.
use designcraft_color::{Color, Gradient, GradientKind, GradientStop, Swatch, SwatchValue};
use designcraft_compose::Cache;
use designcraft_doc::{Document, Fill, Item, ItemId, Shape, SpreadRef, build::NewDocument};
use designcraft_geom::{Rect, shapes};
use designcraft_pdf::{PdfOptions, Standard, export_pdf_with_report};
use hayro_syntax::{
    Pdf,
    object::{Array, Dict, Name},
};

fn document(colors: &[Color], kind: GradientKind, midpoint: f32, opacity: f32) -> Document {
    let mut doc = Document::new(&NewDocument { width: 120.0, height: 120.0, facing_pages: false, ..Default::default() });
    let stops = colors
        .iter()
        .enumerate()
        .map(|(i, &color)| GradientStop { offset: i as f32 / (colors.len() - 1) as f32, color, opacity, midpoint })
        .collect();
    doc.swatches.push(Swatch {
        name: "Mixed inks".into(),
        value: SwatchValue::Gradient { gradient: Gradient { kind, stops } },
        locked: false,
        named: true,
        hidden: false,
    });
    // Two uses of the same swatch must not duplicate the warning.
    for x in [10.0, 65.0] {
        let mut item = Item::new(ItemId(doc.alloc()), doc.default_layer(), Shape::Rectangle, shapes::rectangle(Rect::new(x, 10.0, x + 40.0, 100.0)));
        item.fill = Fill::swatch("Mixed inks");
        doc.insert_item(SpreadRef::Doc(0), item, None).unwrap();
    }
    doc
}

fn conversion_warnings(warnings: &[String]) -> usize {
    warnings.iter().filter(|w| w.contains("Mixed inks") && w.contains("converted to RGB")).count()
}

/// A subprocess is essential: the original assertion panicked again in Surface::drop and aborted.
#[test]
fn mixed_gradient_export_survives_in_subprocess() {
    const CHILD: &str = "DESIGNCRAFT_TEST_MIXED_GRADIENT_CHILD";
    if std::env::var_os(CHILD).is_none() {
        let out = std::process::Command::new(std::env::current_exe().unwrap())
            .args(["--exact", "mixed_gradient_export_survives_in_subprocess", "--nocapture"])
            .env(CHILD, "1")
            .output()
            .unwrap();
        assert!(out.status.success(), "gradient export child failed: {}\n{}", out.status, String::from_utf8_lossy(&out.stderr));
        assert!(String::from_utf8_lossy(&out.stderr).contains("gradient matrix completed"), "child must execute the regression matrix");
        return;
    }
    let rgb = Color::rgb(1.0, 0.0, 0.0);
    let cmyk = Color::cmyk(1.0, 0.0, 0.0, 0.0);
    let gray = Color::gray(0.4);
    for colors in [vec![cmyk, rgb], vec![rgb, cmyk], vec![rgb, gray], vec![gray, cmyk], vec![gray, rgb, cmyk]] {
        for kind in [GradientKind::Linear, GradientKind::Radial] {
            for standard in [Standard::None, Standard::PdfX4, Standard::PdfA2b] {
                let d = document(&colors, kind, 0.5, 0.7);
                let report = export_pdf_with_report(&d, &Cache::new(), &PdfOptions { standard, ..Default::default() }).unwrap();
                assert_eq!(Pdf::new(report.bytes.clone()).unwrap().pages().len(), 1);
                let archival_rgb = standard == Standard::PdfA2b && !colors.iter().any(|c| matches!(c, Color::Gray { .. }));
                assert_eq!(conversion_warnings(&report.warnings), usize::from(!archival_rgb), "{standard:?}: {:?}", report.warnings);
                if standard == Standard::PdfX4 {
                    assert!(
                        report.warnings.iter().any(|w| w.starts_with("PDF/X-4:") && w.contains("RGB")),
                        "RGB conformance warning must survive: {:?}",
                        report.warnings
                    );
                }
                if standard == Standard::PdfA2b {
                    assert!(
                        report.warnings.iter().any(|w| w.contains("PDF/A") && w.contains("RGB")),
                        "archival policy warning must survive: {:?}",
                        report.warnings
                    );
                }
            }
        }
    }
    // Expansion inserts an RGB midpoint even when the authoring stops share CMYK or Gray.
    for colors in [[cmyk, Color::cmyk(0.0, 1.0, 0.0, 0.0)], [gray, Color::gray(0.8)]] {
        for kind in [GradientKind::Linear, GradientKind::Radial] {
            for standard in [Standard::None, Standard::PdfX4, Standard::PdfA2b] {
                let report =
                    export_pdf_with_report(&document(&colors, kind, 0.25, 1.0), &Cache::new(), &PdfOptions { standard, ..Default::default() })
                        .unwrap();
                let archival_cmyk = standard == Standard::PdfA2b && matches!(colors[0], Color::Cmyk { .. });
                assert_eq!(conversion_warnings(&report.warnings), usize::from(!archival_cmyk), "{standard:?}: {:?}", report.warnings);
                assert_eq!(Pdf::new(report.bytes).unwrap().pages().len(), 1);
            }
        }
    }
    eprintln!("gradient matrix completed");
}

fn endpoint(function: &Dict<'_>, first: bool) -> Vec<f32> {
    if let Some(functions) = function.get::<Array<'_>>(b"Functions") {
        let functions: Vec<_> = functions.iter::<Dict<'_>>().collect();
        return endpoint(if first { functions.first().unwrap() } else { functions.last().unwrap() }, first);
    }
    function.get::<Array<'_>>(if first { b"C0" } else { b"C1" }).unwrap().iter::<f32>().collect()
}

#[test]
fn homogeneous_gradients_keep_device_spaces_and_components() {
    for (colors, space, start, end) in [
        ([Color::rgb(1.0, 0.0, 0.0), Color::rgb(0.0, 0.0, 1.0)], "DeviceRGB", vec![1.0, 0.0, 0.0], vec![0.0, 0.0, 1.0]),
        ([Color::cmyk(1.0, 0.0, 0.0, 0.0), Color::cmyk(0.0, 1.0, 0.0, 0.0)], "DeviceCMYK", vec![1.0, 0.0, 0.0, 0.0], vec![0.0, 1.0, 0.0, 0.0]),
        ([Color::gray(0.2), Color::gray(0.8)], "DeviceGray", vec![0.8], vec![0.2]),
    ] {
        for standard in [Standard::None, Standard::PdfX4] {
            let report = export_pdf_with_report(
                &document(&colors, GradientKind::Linear, 0.5, 1.0),
                &Cache::new(),
                &PdfOptions { standard, ..Default::default() },
            )
            .unwrap();
            assert_eq!(conversion_warnings(&report.warnings), 0);
            let pdf = Pdf::new(report.bytes).unwrap();
            let patterns = &pdf.pages()[0].resources().patterns;
            let mut count = 0;
            for (name, _) in patterns.entries() {
                let pattern = patterns.get::<Dict<'_>>(name.as_ref()).unwrap();
                let shading = pattern.get::<Dict<'_>>(b"Shading").unwrap();
                assert_eq!(shading.get::<Name<'_>>(b"ColorSpace").unwrap().as_ref(), space.as_bytes());
                let function = shading.get::<Dict<'_>>(b"Function").unwrap();
                for (actual, expected) in [(endpoint(&function, true), &start), (endpoint(&function, false), &end)] {
                    assert_eq!(actual.len(), expected.len());
                    assert!(actual.iter().zip(expected).all(|(a, b)| (a - b).abs() < 0.005), "{space}: {actual:?} vs {expected:?}");
                }
                count += 1;
            }
            assert!(count > 0, "inspect actual shading resources, not unrelated blend spaces");
        }
    }
}

#[test]
fn mixed_gradient_booklet_reports_conversion() {
    let d = document(&[Color::cmyk(1.0, 0.0, 0.0, 0.0), Color::rgb(1.0, 0.0, 0.0)], GradientKind::Linear, 0.5, 1.0);
    let report = designcraft_pdf::export_booklet(&d, &Cache::new(), &Default::default()).unwrap();
    assert_eq!(conversion_warnings(&report.warnings), 1);
    assert!(!Pdf::new(report.bytes).unwrap().pages().is_empty());
}
