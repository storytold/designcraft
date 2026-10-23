//! Gradients on spot swatches shade in their inks: Separation for one spot, DeviceN for a spot
//! with process inks or a second spot.
use designcraft_color::{Color, ColorType, Gradient, GradientKind, GradientStop, Swatch, SwatchValue};
use designcraft_compose::Cache;
use designcraft_doc::{Document, Fill, Item, ItemId, Shape, SpreadRef, build::NewDocument};
use designcraft_geom::{Rect, shapes};
use designcraft_pdf::{PdfOptions, Standard, export_pdf_with_report};
use hayro_syntax::{
    Pdf,
    object::{Array, Dict, Name},
};

const CYAN: &str = "C=100 M=0 Y=0 K=0";

fn spot(d: &mut Document, name: &str, color: Color) {
    d.swatches.push(Swatch { value: SwatchValue::Color { color, color_type: ColorType::Spot }, ..Swatch::color(name, color) });
}

/// A document with spot inks Gold and Silver, a 40 % tint of Gold, and one box filled with a gradient on `stops`
/// (swatch name or plain color, location).
fn document(stops: &[(Result<&str, Color>, f32)], midpoint: f32) -> Document {
    document_with(stops, midpoint, false)
}

/// [`document`], the box's fill overprinting when `overprint`.
fn document_with(stops: &[(Result<&str, Color>, f32)], midpoint: f32, overprint: bool) -> Document {
    let mut d = Document::new(&NewDocument { width: 120.0, height: 120.0, facing_pages: false, ..Default::default() });
    spot(&mut d, "Gold", Color::cmyk(0.0, 0.3, 0.9, 0.1));
    spot(&mut d, "Silver", Color::cmyk(0.2, 0.15, 0.1, 0.3));
    d.swatches.push(Swatch { value: SwatchValue::Tint { base: "Gold".into(), tint: 0.4 }, ..Swatch::color("Gold 40%", Color::BLACK) });
    let stops = stops
        .iter()
        .map(|(s, offset)| match s {
            Ok(name) => GradientStop { swatch: Some(name.to_string()), midpoint, ..GradientStop::new(*offset, d.resolve_color(name, 1.0).unwrap()) },
            Err(c) => GradientStop { midpoint, ..GradientStop::new(*offset, *c) },
        })
        .collect();
    d.swatches.push(Swatch {
        value: SwatchValue::Gradient { gradient: Gradient { kind: GradientKind::Linear, stops } },
        ..Swatch::color("Fade", Color::BLACK)
    });
    let mut item = Item::new(ItemId(d.alloc()), d.default_layer(), Shape::Rectangle, shapes::rectangle(Rect::new(10.0, 10.0, 110.0, 110.0)));
    item.fill = Fill { overprint, ..Fill::swatch("Fade") };
    d.insert_item(SpreadRef::Doc(0), item, None).unwrap();
    d
}

fn endpoint(function: &Dict<'_>, first: bool) -> Vec<f32> {
    if let Some(functions) = function.get::<Array<'_>>(b"Functions") {
        let functions: Vec<_> = functions.iter::<Dict<'_>>().collect();
        return endpoint(if first { functions.first().unwrap() } else { functions.last().unwrap() }, first);
    }
    function.get::<Array<'_>>(if first { b"C0" } else { b"C1" }).unwrap().iter::<f32>().collect()
}

/// Each shading on the page: its color space family, colorant names, and the colors at its
/// two ends.
struct Shading {
    family: String,
    colorants: Vec<String>,
    start: Vec<f32>,
    end: Vec<f32>,
}

fn export(d: &Document, standard: Standard) -> (Vec<Shading>, Vec<String>) {
    let report = export_pdf_with_report(d, &Cache::new(), &PdfOptions { standard, compress: false, ..Default::default() }).unwrap();
    let pdf = Pdf::new(report.bytes).unwrap();
    let patterns = &pdf.pages()[0].resources().patterns;
    let mut out = Vec::new();
    for (name, _) in patterns.entries() {
        let pattern = patterns.get::<Dict<'_>>(name.as_ref()).unwrap();
        let shading = pattern.get::<Dict<'_>>(b"Shading").unwrap();
        let text = |n: Name<'_>| String::from_utf8_lossy(n.as_ref()).into_owned();
        let (family, colorants) = match shading.get::<Name<'_>>(b"ColorSpace") {
            Some(n) => (text(n), Vec::new()),
            None => {
                let space = shading.get::<Array<'_>>(b"ColorSpace").unwrap();
                let mut it = space.flex_iter();
                let family = text(it.next::<Name<'_>>().unwrap());
                let colorants = match family.as_str() {
                    "Separation" => vec![text(it.next::<Name<'_>>().unwrap())],
                    "DeviceN" => it.next::<Array<'_>>().unwrap().iter::<Name<'_>>().map(text).collect(),
                    _ => Vec::new(),
                };
                (family, colorants)
            }
        };
        let function = shading.get::<Dict<'_>>(b"Function").unwrap();
        out.push(Shading { family, colorants, start: endpoint(&function, true), end: endpoint(&function, false) });
    }
    assert!(!out.is_empty(), "the gradient is a shading");
    (out, report.warnings)
}

fn close(a: &[f32], b: &[f32]) -> bool {
    a.len() == b.len() && a.iter().zip(b).all(|(a, b)| (a - b).abs() < 0.01)
}

fn no_conversion(warnings: &[String]) {
    assert!(!warnings.iter().any(|w| w.contains("Fade")), "{warnings:?}");
}

#[test]
fn a_spot_to_paper_gradient_is_a_separation_shading() {
    for standard in [Standard::None, Standard::PdfX4] {
        let (shadings, warnings) = export(&document(&[(Ok("Gold"), 0.0), (Ok("[Paper]"), 1.0)], 0.5), standard);
        no_conversion(&warnings);
        let s = &shadings[0];
        assert_eq!((s.family.as_str(), s.colorants.as_slice()), ("Separation", &["Gold".to_string()][..]), "{standard:?}");
        assert!(close(&s.start, &[1.0]) && close(&s.end, &[0.0]), "{:?} → {:?}", s.start, s.end);
    }
    // A tint of the spot shades in the same Separation.
    let (shadings, _) = export(&document(&[(Ok("Gold"), 0.0), (Ok("Gold 40%"), 1.0)], 0.5), Standard::PdfX4);
    assert_eq!(shadings[0].family, "Separation");
    assert!(close(&shadings[0].end, &[0.4]), "{:?}", shadings[0].end);
    // Converted to process in the Ink Manager: CMYK.
    let mut d = document(&[(Ok("Gold"), 0.0), (Ok("[Paper]"), 1.0)], 0.5);
    d.inks.to_process.push("Gold".into());
    let (shadings, _) = export(&d, Standard::PdfX4);
    assert_eq!(shadings[0].family, "DeviceCMYK");
    assert!(close(&shadings[0].start, &[0.0, 0.3, 0.9, 0.1]), "{:?}", shadings[0].start);
}

#[test]
fn a_spot_to_cyan_gradient_is_a_device_n_shading() {
    let (shadings, warnings) = export(&document(&[(Ok("Gold"), 0.0), (Ok(CYAN), 1.0)], 0.5), Standard::PdfX4);
    no_conversion(&warnings);
    let s = &shadings[0];
    assert_eq!(s.family, "DeviceN");
    assert_eq!(s.colorants, ["Cyan", "Gold"]);
    assert!(close(&s.start, &[0.0, 1.0]) && close(&s.end, &[1.0, 0.0]), "{:?} → {:?}", s.start, s.end);
    // A midpoint stays in the inks: the colors at the ends are unchanged and in DeviceN.
    let (shadings, _) = export(&document(&[(Ok("Gold"), 0.0), (Ok(CYAN), 1.0)], 0.25), Standard::PdfX4);
    assert_eq!(shadings[0].family, "DeviceN");
    assert!(close(&shadings[0].start, &[0.0, 1.0]) && close(&shadings[0].end, &[1.0, 0.0]));
}

#[test]
fn a_two_spot_gradient_is_a_device_n_shading() {
    let (shadings, warnings) = export(&document(&[(Ok("Gold"), 0.0), (Ok("Silver"), 1.0)], 0.5), Standard::PdfX4);
    no_conversion(&warnings);
    let s = &shadings[0];
    assert_eq!(s.family, "DeviceN");
    assert_eq!(s.colorants, ["Gold", "Silver"]);
    assert!(close(&s.start, &[1.0, 0.0]) && close(&s.end, &[0.0, 1.0]), "{:?} → {:?}", s.start, s.end);
    // Stop opacity is a soft mask over the same inks.
    let mut d = document(&[(Ok("Gold"), 0.0), (Ok("Silver"), 1.0)], 0.5);
    if let Some(SwatchValue::Gradient { gradient }) = d.swatches.iter_mut().find(|w| w.name == "Fade").map(|w| &mut w.value) {
        gradient.stops[0].opacity = 0.4;
    }
    let (shadings, warnings) = export(&d, Standard::PdfX4);
    no_conversion(&warnings);
    assert!(shadings.iter().any(|s| s.family == "DeviceN" && s.colorants == ["Gold", "Silver"]));
}

#[test]
fn a_process_gradient_to_paper_prints_in_cmyk() {
    let (shadings, warnings) = export(&document(&[(Ok(CYAN), 0.0), (Ok("[Paper]"), 1.0)], 0.5), Standard::PdfX4);
    no_conversion(&warnings);
    assert_eq!(shadings[0].family, "DeviceCMYK");
    assert!(close(&shadings[0].start, &[1.0, 0.0, 0.0, 0.0]) && close(&shadings[0].end, &[0.0, 0.0, 0.0, 0.0]));
}

/// Nonzero overprint mode doesn't apply to shadings, so an overprinting process gradient names
/// only the inks it uses and leaves the other plates alone.
#[test]
fn an_overprinting_process_gradient_is_a_device_n_shading_of_its_inks() {
    for standard in [Standard::None, Standard::PdfX4] {
        let (shadings, warnings) = export(&document_with(&[(Ok(CYAN), 0.0), (Ok("[Paper]"), 1.0)], 0.5, true), standard);
        no_conversion(&warnings);
        let s = &shadings[0];
        assert_eq!((s.family.as_str(), s.colorants.as_slice()), ("DeviceN", &["Cyan".to_string()][..]), "{standard:?}");
        assert!(close(&s.start, &[1.0]) && close(&s.end, &[0.0]), "{:?} → {:?}", s.start, s.end);
    }
    // Plain-color stops too.
    let c_to_m = [(Err(Color::cmyk(1.0, 0.0, 0.0, 0.0)), 0.0), (Err(Color::cmyk(0.0, 1.0, 0.0, 0.0)), 1.0)];
    let (shadings, _) = export(&document_with(&c_to_m, 0.5, true), Standard::PdfX4);
    let s = &shadings[0];
    assert_eq!((s.family.as_str(), s.colorants.as_slice()), ("DeviceN", &["Cyan".to_string(), "Magenta".to_string()][..]));
    assert!(close(&s.start, &[1.0, 0.0]) && close(&s.end, &[0.0, 1.0]), "{:?} → {:?}", s.start, s.end);
    // Without overprint it stays in DeviceCMYK.
    let (shadings, _) = export(&document(&c_to_m, 0.5), Standard::PdfX4);
    assert_eq!(shadings[0].family, "DeviceCMYK");
}

/// PDF/A forbids DeviceN's PostScript tint transform: the gradient goes to process color, with
/// a warning, and the export still succeeds.
#[test]
fn pdf_a_writes_mixed_spot_gradients_in_process_color() {
    let (shadings, warnings) = export(&document(&[(Ok("Gold"), 0.0), (Ok(CYAN), 1.0)], 0.5), Standard::PdfA2b);
    assert!(warnings.iter().any(|w| w.contains("'Fade'") && w.contains("spot inks printed as process")), "{warnings:?}");
    assert!(shadings.iter().all(|s| s.family != "DeviceN"));
}

/// Stops with plain colors (every document saved before stops named swatches) export as they
/// did: the spot's values in DeviceCMYK.
#[test]
fn plain_color_stops_keep_their_device_space() {
    let gold = Color::cmyk(0.0, 0.3, 0.9, 0.1);
    let (shadings, warnings) = export(&document(&[(Err(gold), 0.0), (Err(Color::cmyk(1.0, 0.0, 0.0, 0.0)), 1.0)], 0.5), Standard::PdfX4);
    no_conversion(&warnings);
    assert_eq!(shadings[0].family, "DeviceCMYK");
    assert!(close(&shadings[0].start, &[0.0, 0.3, 0.9, 0.1]));
}
