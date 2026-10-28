//! File › Export PDF…: the Export PDF dialog. A preset and standard bar over a section list
//! (General, Compression, Marks and Bleeds, Output, Advanced, Summary), as InDesign arranges it.
//!
//! The dialog's fields are [`PdfExportSettings`]' fields by their camelCase names, plus the
//! per-job `pagesMode`, `pages` and `title` and the shown `section`. A preset (built-in or
//! saved) sets every setting at once; editing any setting turns the preset into "Custom".
//! [`export_params`] turns the settings into `file.exportPdf` parameters.

use designcraft_geom::{Unit, format_measure, units::parse_measure};
use serde_json::{Value, json};

use super::{Dialog, check, semibold, text_field, translated_combo};
use crate::state::{PdfExportPreset, PdfExportSettings};
use crate::{DesignApp, i18n};

/// The built-in presets, in display order. The stored value is the English name; the caption
/// is translated.
pub(crate) const BUILTIN_PRESETS: &[&str] = &["Desktop Printing", "Commercial Printing", "Screen and Email", "PDF/X-1a:2003", "PDF/X-4", "PDF/A-2b"];

/// What a preset is for (the General section's description). Our own words.
pub(crate) fn description(name: &str) -> &'static str {
    match name {
        "Desktop Printing" => "For office and home printers and for proofs: images at 300 ppi, colours as they were defined, no printer's marks.",
        "Commercial Printing" => {
            "For a print shop that takes plain PDF: images at 300 ppi in high quality, crop and bleed marks with the bleed, colours as defined."
        }
        "Screen and Email" => {
            "For reading on screen and sending by email: images reduced to 100 ppi in medium quality, colours converted to sRGB, transparency flattened at low resolution."
        }
        "PDF/X-1a:2003" => {
            "For printers that ask for PDF/X-1a: CMYK and spot colours only, no transparency (flattened at high resolution), the CMYK destination as the output intent."
        }
        "PDF/X-4" => {
            "For printers that ask for PDF/X-4: live transparency and layers kept, RGB converted to the CMYK destination, which is the output intent."
        }
        "PDF/A-2b" => "For archiving: a self-contained file whose appearance is preserved for long-term storage. Colours are RGB.",
        _ => "Settings saved as a preset, or edited from one.",
    }
}

/// A built-in preset's settings, or `None` for another name. Presets that leave the
/// Transparency Flattener alone keep the dialog's.
pub(crate) fn builtin(name: &str) -> Option<PdfExportSettings> {
    let base = PdfExportSettings { preset: name.to_string(), ..PdfExportSettings::default() };
    let print_marks = |s: PdfExportSettings| PdfExportSettings { marks_crop: true, marks_bleed: true, bleed: true, ..s };
    let quality = |s: PdfExportSettings, q: &str| PdfExportSettings { color_quality: q.into(), gray_quality: q.into(), ..s };
    Some(match name {
        "Desktop Printing" => base,
        "Commercial Printing" => print_marks(quality(base, "maximum")),
        "Screen and Email" => PdfExportSettings {
            color_ppi: "100".into(),
            color_above: "150".into(),
            gray_ppi: "100".into(),
            gray_above: "150".into(),
            mono_ppi: "300".into(),
            mono_above: "450".into(),
            color_compression: "jpeg".into(),
            gray_compression: "jpeg".into(),
            conversion: "destination".into(),
            destination: designcraft_color::cms::SRGB.into(),
            include_profile: true,
            bleed: false,
            flatten: "low".into(),
            ..quality(base, "medium")
        },
        "PDF/X-1a:2003" => PdfExportSettings {
            standard: "x1a".into(),
            conversion: "preserveNumbers".into(),
            include_profile: true,
            flatten: "high".into(),
            tagged: false,
            ..print_marks(quality(base, "maximum"))
        },
        "PDF/X-4" => PdfExportSettings {
            standard: "x4".into(),
            conversion: "preserveNumbers".into(),
            include_profile: true,
            tagged: false,
            ..print_marks(quality(base, "maximum"))
        },
        "PDF/A-2b" => PdfExportSettings { standard: "a2b".into(), color_compression: "zip".into(), gray_compression: "zip".into(), ..base },
        _ => return None,
    })
}

/// Whether a built-in preset sets the flattener (the others keep the dialog's).
fn sets_flatten(name: &str) -> bool {
    matches!(name, "Screen and Email" | "PDF/X-1a:2003")
}

/// The settings the dialog's fields describe (anything missing or mistyped is the default).
pub(crate) fn settings_from(d: &Dialog) -> PdfExportSettings {
    let defaults = serde_json::to_value(PdfExportSettings::default()).unwrap_or(Value::Null);
    let mut merged = serde_json::Map::new();
    if let Value::Object(def) = defaults {
        for (k, v) in def {
            // A value of the wrong type (a hand-edited ui.json) falls back to the default.
            let field = d.fields.get(&k).filter(|f| std::mem::discriminant(*f) == std::mem::discriminant(&v)).cloned();
            merged.insert(k, field.unwrap_or(v));
        }
    }
    serde_json::from_value(Value::Object(merged)).unwrap_or_default()
}

/// Put `s` into the dialog's fields (the per-job fields and the shown section stay).
fn set_fields(d: &mut Dialog, s: &PdfExportSettings) {
    if let Ok(Value::Object(m)) = serde_json::to_value(s) {
        for (k, v) in m {
            d.fields.insert(k, v);
        }
    }
}

/// Select preset `name`: a built-in or one of `saved`. "Custom" and unknown names change
/// nothing.
pub(crate) fn apply_preset(saved: &[PdfExportPreset], d: &mut Dialog, name: &str) {
    let Some(mut s) = preset_settings(saved, name) else { return };
    // Built-in presets that don't flatten keep the dialog's flattener.
    if builtin(name).is_some() && !sets_flatten(name) {
        s.flatten = d.s("flatten");
    }
    set_fields(d, &s);
}

/// Save the dialog's settings as preset `name` (replacing a saved one of that name). Built-in
/// names and "Custom" are refused.
pub(crate) fn save_preset(app: &mut DesignApp, d: &mut Dialog, name: &str) -> Result<(), String> {
    let name = name.trim();
    if name.is_empty() || name == "Custom" || BUILTIN_PRESETS.contains(&name) {
        return Err(i18n::tr(&app.ui.language, "Choose a name that isn't a built-in preset").to_string());
    }
    let settings = PdfExportSettings { preset: name.to_string(), ..settings_from(d) };
    let presets = &mut app.ui.pdf_export_presets;
    match presets.iter_mut().find(|p| p.name == name) {
        Some(p) => p.settings = settings,
        None => presets.push(PdfExportPreset { name: name.to_string(), settings }),
    }
    d.fields.insert("preset".into(), json!(name));
    Ok(())
}

/// The resolutions `file.exportPdf` accepts (its `ImageSampling::PPI_RANGE`).
const PPI_RANGE: std::ops::RangeInclusive<f64> = 9.0..=2400.0;

/// A resolution field: pixels per inch within the exporter's range.
fn ppi(label: &str, v: &str) -> Result<f64, String> {
    let n: f64 = v.trim().parse().map_err(|_| format!("{label}: `{v}` isn't a resolution"))?;
    if !PPI_RANGE.contains(&n) {
        return Err(format!("{label}: {n} ppi is outside {}–{} ppi", PPI_RANGE.start(), PPI_RANGE.end()));
    }
    Ok(n)
}

/// A measurement field in points; a bare number is in `unit`.
fn measure(label: &str, v: &str, unit: Unit) -> Result<f64, String> {
    let n = parse_measure(v, unit).map_err(|_| format!("{label}: `{v}` isn't a measurement"))?;
    if !n.is_finite() || !(0.0..=1000.0).contains(&n) {
        return Err(format!("{label}: {v} is outside 0–1000 pt"));
    }
    Ok(n)
}

/// The `file.exportPdf` parameters for `s` (without pages, title or path). Bleed values are in
/// `unit` when they have none of their own. An invalid number is an error naming the field.
pub(crate) fn export_params(s: &PdfExportSettings, unit: Unit) -> Result<Value, String> {
    let class = |prefix: &str, down: &str, target: &str, above: &str, compression: &str, quality: &str| -> Result<Value, String> {
        // Without downsampling the fields are disabled: a stale value in them doesn't block.
        let resample = down != "none";
        let target = if resample { ppi(&format!("{prefix} resolution"), target)? } else { 300.0 };
        let above = if resample { ppi(&format!("{prefix} threshold"), above)? } else { 450.0 };
        Ok(json!({"downsample": down, "ppi": target, "above": above.max(target), "compression": compression, "quality": quality}))
    };
    let images = json!({
        "color": class("Color images", &s.color_downsample, &s.color_ppi, &s.color_above, &s.color_compression, &s.color_quality)?,
        "gray": class("Grayscale images", &s.gray_downsample, &s.gray_ppi, &s.gray_above, &s.gray_compression, &s.gray_quality)?,
        "mono": class("Monochrome images", &s.mono_downsample, &s.mono_ppi, &s.mono_above, "zip", "maximum")?,
        "cropToFrames": s.crop_to_frames,
    });
    let pt = |label: &str, v: &str| measure(label, v, Unit::Points);
    let mut p = json!({
        "standard": s.standard,
        "spreads": s.spreads,
        "bleed": s.bleed,
        "slug": s.slug,
        "tagged": s.tagged,
        "media": s.media,
        "compressText": s.compress_text,
        "images": images,
        "marks": {
            "crop": s.marks_crop,
            "bleed": s.marks_bleed,
            "registration": s.marks_registration,
            "colorBars": s.marks_color_bars,
            "pageInfo": s.marks_page_info,
            "weight": pt("Mark weight", &s.marks_weight)?.max(0.05),
            "offset": pt("Mark offset", &s.marks_offset)?,
        },
        "output": {
            "conversion": s.conversion,
            "includeProfile": s.include_profile,
        },
    });
    if !s.destination.is_empty() {
        p["output"]["destination"] = json!(s.destination);
    }
    if !s.intent.is_empty() {
        p["output"]["intent"] = json!(s.intent);
    }
    if s.bleed && !s.use_doc_bleed {
        p["bleedValues"] = json!([
            measure("Bleed top", &s.bleed_top, unit)?,
            measure("Bleed bottom", &s.bleed_bottom, unit)?,
            measure("Bleed inside", &s.bleed_inside, unit)?,
            measure("Bleed outside", &s.bleed_outside, unit)?,
        ]);
    }
    if !s.flatten.is_empty() && s.flatten != "none" {
        p["flatten"] = json!(s.flatten);
    }
    if s.view != "default" && !s.view.is_empty() {
        p["view"] = json!(s.view);
    }
    if s.layout != "default" && !s.layout.is_empty() {
        p["pageLayout"] = json!(s.layout);
    }
    if s.full_screen {
        p["fullScreen"] = json!(true);
    }
    if s.bookmarks_panel {
        p["bookmarksPanel"] = json!(true);
    }
    Ok(p)
}

/// The settings preset `name` stands for: a built-in, or one of `saved`. `None` for an unknown
/// name.
pub(crate) fn preset_settings(saved: &[PdfExportPreset], name: &str) -> Option<PdfExportSettings> {
    builtin(name).or_else(|| saved.iter().find(|p| p.name == name).map(|p| PdfExportSettings { preset: name.to_string(), ..p.settings.clone() }))
}

/// The `file.exportPdf` parameters of preset `name` (built-in or saved), for other places that
/// make a PDF with a preset, such as a package's PDF (File › Package's PDF Preset, #1515).
/// `None` for an unknown name.
#[cfg_attr(not(test), allow(dead_code))]
pub(crate) fn preset_params(saved: &[PdfExportPreset], name: &str, unit: Unit) -> Option<Result<Value, String>> {
    preset_settings(saved, name).map(|s| export_params(&s, unit))
}

/// Problems with the settings worth knowing before exporting (the Summary's warnings).
pub(crate) fn warnings(s: &PdfExportSettings, cmyk_working: &str) -> Vec<&'static str> {
    let mut w = Vec::new();
    let profiles = designcraft_color::cms::profiles();
    let kind = |name: &str| profiles.iter().find(|p| p.name == name).map(|p| p.kind);
    let rgb_dest = !s.destination.is_empty() && kind(&s.destination) == Some(designcraft_color::cms::ProfileKind::Rgb);
    let pdfx = s.standard == "x4" || s.standard == "x1a";
    if pdfx && s.conversion != "none" && rgb_dest {
        w.push("PDF/X needs a CMYK destination: colours will not be converted to the RGB profile chosen.");
    }
    if s.standard == "a2b" && s.conversion != "none" && !rgb_dest {
        w.push("PDF/A output is RGB: conversion to a CMYK destination is left out.");
    }
    if s.standard == "x1a" && s.conversion == "none" {
        w.push("PDF/X-1a allows only CMYK and spot colours: RGB in the document will fail its check. Choose a Color Conversion.");
    }
    if pdfx && !s.bleed {
        w.push("Most printers need the bleed in a PDF/X file.");
    }
    if s.color_compression == "jpeg" && s.color_quality == "minimum" {
        w.push("Minimum JPEG quality shows visible artefacts in print.");
    }
    if s.conversion == "destination" && !s.destination.is_empty() && s.destination != cmyk_working && !rgb_dest {
        w.push("CMYK colours are re-separated for the destination; their numbers change. Preserve Numbers keeps them.");
    }
    w
}

/// The Summary: every setting in a line, then the warnings.
pub(crate) fn summary(lang: &str, s: &PdfExportSettings) -> Vec<String> {
    let tr = |t: &str| i18n::tr(lang, t).to_string();
    let onoff = |b: bool| tr(if b { "On" } else { "Off" });
    let down = |d: &str| {
        tr(match d {
            "average" => "Average Downsampling",
            "subsample" => "Subsampling",
            "bicubic" => "Bicubic Downsampling",
            _ => "Do Not Downsample",
        })
    };
    let compression = |c: &str| {
        tr(match c {
            "jpeg" => "JPEG",
            "zip" => "ZIP",
            _ => "Automatic (JPEG)",
        })
    };
    let quality = |q: &str| {
        tr(match q {
            "minimum" => "Minimum",
            "low" => "Low",
            "medium" => "Medium",
            "high" => "High",
            _ => "Maximum",
        })
    };
    let images = |d: &str, p: &str, a: &str, c: &str, q: &str| {
        let encoding = if c == "zip" { compression(c) } else { format!("{}, {}", compression(c), quality(q)) };
        if d == "none" { format!("{}, {encoding}", down(d)) } else { format!("{} {p} ppi (> {a} ppi), {encoding}", down(d)) }
    };
    let conversion = tr(match s.conversion.as_str() {
        "destination" => "Convert to Destination",
        "preserveNumbers" => "Convert to Destination (Preserve Numbers)",
        _ => "No Color Conversion",
    });
    let dest = if s.destination.is_empty() { tr("Working space") } else { s.destination.clone() };
    let mut marks: Vec<String> = Vec::new();
    for (on, name) in [
        (s.marks_crop, "Crop Marks"),
        (s.marks_bleed, "Bleed Marks"),
        (s.marks_registration, "Registration Marks"),
        (s.marks_color_bars, "Color Bars"),
        (s.marks_page_info, "Page Information"),
    ] {
        if on {
            marks.push(tr(name));
        }
    }
    let mut lines = vec![
        format!("{}: {}", tr("Preset"), tr(&s.preset)),
        format!("{}: {}", tr("Standard"), standard_label(&s.standard)),
        format!("{}: {}", tr("Export As"), tr(if s.spreads { "Spreads" } else { "Pages" })),
        format!("{}: {}", tr("Color Images"), images(&s.color_downsample, &s.color_ppi, &s.color_above, &s.color_compression, &s.color_quality)),
        format!("{}: {}", tr("Grayscale Images"), images(&s.gray_downsample, &s.gray_ppi, &s.gray_above, &s.gray_compression, &s.gray_quality)),
        format!("{}: {}", tr("Monochrome Images"), images(&s.mono_downsample, &s.mono_ppi, &s.mono_above, "zip", "maximum")),
        format!("{}: {}", tr("Crop Image Data to Frames"), onoff(s.crop_to_frames)),
        format!("{}: {}", tr("Compress Text and Line Art"), onoff(s.compress_text)),
        format!("{}: {}", tr("Marks"), if marks.is_empty() { tr("None") } else { marks.join(", ") }),
        format!(
            "{}: {}",
            tr("Bleed"),
            if !s.bleed {
                tr("None")
            } else if s.use_doc_bleed {
                tr("Document Bleed Settings")
            } else {
                format!("{} / {} / {} / {}", s.bleed_top, s.bleed_bottom, s.bleed_inside, s.bleed_outside)
            }
        ),
        format!("{}: {}", tr("Include Slug Area"), onoff(s.slug)),
        format!("{}: {}", tr("Color Conversion"), conversion),
    ];
    if s.conversion != "none" || s.include_profile {
        lines.push(format!("{}: {}", tr("Destination"), dest));
    }
    if s.conversion != "none" {
        let intent = tr(match s.intent.as_str() {
            "perceptual" => "Perceptual",
            "relative" => "Relative Colorimetric",
            "saturation" => "Saturation",
            "absolute" => "Absolute Colorimetric",
            _ => "Use Color Settings",
        });
        lines.push(format!("{}: {}", tr("Rendering Intent"), intent));
    }
    lines.push(format!(
        "{}: {}",
        tr("Profile Inclusion Policy"),
        tr(if s.include_profile { "Include Destination Profile" } else { "Don't Include Profiles" })
    ));
    lines.push(format!("{}: {}", tr("Create Tagged PDF"), onoff(s.tagged)));
    let flatten = tr(match s.flatten.as_str() {
        "high" => "High Resolution",
        "medium" => "Medium Resolution",
        "low" => "Low Resolution",
        _ => "None",
    });
    lines.push(format!("{}: {}", tr("Transparency Flattener"), flatten));
    lines
}

fn standard_label(s: &str) -> &'static str {
    match s {
        "x1a" => "PDF/X-1a:2003",
        "x4" => "PDF/X-4",
        "a2b" => "PDF/A-2b",
        _ => "None",
    }
}

/// The PDF version a standard writes (the Compatibility InDesign lets you pick follows from the
/// standard here).
fn compatibility(standard: &str) -> &'static str {
    match standard {
        "x1a" => "PDF 1.4",
        "x4" => "PDF 1.6",
        _ => "PDF 1.7",
    }
}

/// The dialog's preferred width (the host narrows it on a small screen).
pub(crate) const WIDTH: f32 = 780.0;

/// The section list's width.
const LIST_W: f32 = 150.0;

const SECTIONS: &[(&str, &str)] = &[
    ("general", "General"),
    ("compression", "Compression"),
    ("marks", "Marks and Bleeds"),
    ("output", "Output"),
    ("advanced", "Advanced"),
    ("summary", "Summary"),
];

const DOWNSAMPLE: &[(&str, &str)] =
    &[("none", "Do Not Downsample"), ("average", "Average Downsampling to"), ("subsample", "Subsampling to"), ("bicubic", "Bicubic Downsampling to")];

const COMPRESSION: &[(&str, &str)] = &[("auto", "Automatic (JPEG)"), ("jpeg", "JPEG"), ("zip", "ZIP")];

const QUALITY: &[(&str, &str)] = &[("minimum", "Minimum"), ("low", "Low"), ("medium", "Medium"), ("high", "High"), ("maximum", "Maximum")];

/// Seed the dialog the first time it draws: the section, the page range mode, and the bleed
/// fields from the document while they follow it.
fn seed(app: &DesignApp, d: &mut Dialog) {
    if d.fields.contains_key("section") {
        return;
    }
    d.fields.insert("section".into(), json!("general"));
    let pages = d.s("pages");
    let mode = if pages.trim().is_empty() || pages == "all" { "all" } else { "range" };
    d.fields.insert("pagesMode".into(), json!(mode));
    if let Some(st) = app.session.active() {
        d.unit = st.doc.settings.horizontal_units;
        if d.b("useDocBleed") {
            let b = st.doc.settings.bleed;
            for (k, v) in ["bleedTop", "bleedBottom", "bleedInside", "bleedOutside"].iter().zip(b) {
                d.fields.insert((*k).into(), json!(format_measure(v, d.unit)));
            }
        }
    }
}

/// A heading inside a section.
fn heading(lang: &str, ui: &mut egui::Ui, t: &str) {
    ui.add_space(4.0);
    crate::rtl::label(ui, egui::RichText::new(i18n::tr(lang, t)).font(semibold(12.0)));
}

/// A framed group, as the dialog's boxes.
fn group(ui: &mut egui::Ui, add: impl FnOnce(&mut egui::Ui)) {
    egui::Frame::group(ui.style()).inner_margin(egui::Margin::same(8)).show(ui, |ui| {
        ui.set_width(ui.available_width());
        add(ui);
    });
}

/// The whole dialog body.
pub(crate) fn body(app: &mut DesignApp, ui: &mut egui::Ui, d: &mut Dialog) {
    seed(app, d);
    let lang = app.ui.language.clone();
    let tr = |s: &str| i18n::tr(&lang, s).to_string();
    // Why the last OK didn't export (a field that doesn't parse, a refused option, a failed
    // write): the dialog stays open with the settings as they were.
    let status = d.s("status");
    if !status.is_empty() {
        let warn = ui.visuals().warn_fg_color;
        crate::rtl::label(ui, egui::RichText::new(format!("⚠ {status}")).color(warn));
        ui.add_space(4.0);
    }
    let before = settings_from(d);
    let mut preset_clicked = false;
    // The section's width: what the list and its separator leave.
    let content_w = (ui.available_width() - LIST_W - 24.0).max(240.0);

    // Preset, standard and the PDF version they write.
    egui::Grid::new("pdf_top").num_columns(4).spacing([8.0, 6.0]).show(ui, |ui| {
        crate::rtl::label(ui, tr("Preset:"));
        let cur = d.s("preset");
        let saved: Vec<String> = app.ui.pdf_export_presets.iter().map(|p| p.name.clone()).collect();
        let shown = if cur == "Custom" { tr("Custom") } else { format!("[{}]", tr(&cur)) };
        craft_ui::ComboBox::from_id_salt("pdf_preset").selected_text(crate::rtl::widget(ui, shown)).width(280.0).show_ui(ui, |ui| {
            for p in BUILTIN_PRESETS.iter().map(|p| p.to_string()).chain(saved.iter().cloned()) {
                if ui.selectable_label(cur == p, crate::rtl::widget(ui, format!("[{}]", tr(&p)))).clicked() {
                    d.fields.insert("preset".into(), json!(p));
                    preset_clicked = true;
                }
            }
        });
        ui.end_row();
        crate::rtl::label(ui, tr("Standard:"));
        translated_combo(&lang, ui, d, "standard", &[("none", "None"), ("x1a", "PDF/X-1a:2003"), ("x4", "PDF/X-4"), ("a2b", "PDF/A-2b")]);
        crate::rtl::label(ui, tr("Compatibility:"));
        crate::rtl::label(ui, egui::RichText::new(compatibility(&d.s("standard"))).color(crate::theme::Tokens::get(ui.ctx()).text_dim));
        ui.end_row();
    });
    if preset_clicked {
        let p = d.s("preset");
        apply_preset(&app.ui.pdf_export_presets, d, &p);
    }
    ui.add_space(6.0);

    ui.horizontal_top(|ui| {
        ui.set_min_height(470.0);
        ui.set_max_height(470.0);
        ui.vertical(|ui| {
            ui.set_width(LIST_W);
            for (id, label) in SECTIONS {
                if ui.selectable_label(d.s("section") == *id, crate::rtl::widget(ui, tr(label))).clicked() {
                    d.fields.insert("section".into(), json!(id));
                }
            }
        });
        ui.separator();
        egui::ScrollArea::vertical().id_salt("pdf_section").max_width(content_w).auto_shrink([false, false]).show(ui, |ui| {
            ui.set_width(content_w);
            ui.vertical(|ui| match d.s("section").as_str() {
                "compression" => compression(&lang, ui, d),
                "marks" => marks(app, &lang, ui, d),
                "output" => output(app, &lang, ui, d),
                "advanced" => advanced(&lang, ui, d),
                "summary" => summary_section(app, &lang, ui, d),
                _ => general(&lang, ui, d),
            });
        });
    });

    // Any setting edited by hand: the settings are no longer that preset.
    if !preset_clicked {
        let after = PdfExportSettings { preset: String::new(), ..settings_from(d) };
        let before = PdfExportSettings { preset: String::new(), ..before };
        if after != before {
            d.fields.insert("preset".into(), json!("Custom"));
        }
    }
}

fn general(lang: &str, ui: &mut egui::Ui, d: &mut Dialog) {
    let tr = |s: &str| i18n::tr(lang, s).to_string();
    heading(lang, ui, "General");
    ui.horizontal_top(|ui| {
        crate::rtl::label(ui, tr("Description:"));
        let mut text = tr(description(&d.s("preset")));
        let w = ui.available_width();
        ui.add_enabled(false, egui::TextEdit::multiline(&mut text).desired_rows(3).desired_width(w));
    });
    ui.add_space(4.0);
    ui.columns(2, |cols| {
        let ui = &mut cols[0];
        group(ui, |ui| {
            crate::rtl::label(ui, egui::RichText::new(tr("Pages")).font(semibold(12.0)));
            let mode = d.s("pagesMode");
            ui.horizontal(|ui| {
                if ui.radio(mode != "range", crate::rtl::widget(ui, tr("All"))).clicked() {
                    d.fields.insert("pagesMode".into(), json!("all"));
                }
                if ui.radio(mode == "range", crate::rtl::widget(ui, tr("Range:"))).clicked() {
                    d.fields.insert("pagesMode".into(), json!("range"));
                }
                let before = d.s("pages");
                text_field(ui, d, "pages", 90.0);
                if d.s("pages") != before {
                    d.fields.insert("pagesMode".into(), json!("range"));
                }
            });
            ui.horizontal(|ui| {
                crate::rtl::label(ui, tr("Export As"));
                let spreads = d.b("spreads");
                if ui.radio(!spreads, crate::rtl::widget(ui, tr("Pages"))).clicked() {
                    d.fields.insert("spreads".into(), json!(false));
                }
                if ui.radio(spreads, crate::rtl::widget(ui, tr("Spreads"))).clicked() {
                    d.fields.insert("spreads".into(), json!(true));
                }
            });
        });
        let ui = &mut cols[1];
        group(ui, |ui| {
            crate::rtl::label(ui, egui::RichText::new(tr("Viewing")).font(semibold(12.0)));
            egui::Grid::new("pdf_view").num_columns(2).spacing([8.0, 4.0]).show(ui, |ui| {
                crate::rtl::label(ui, tr("View:"));
                translated_combo(
                    lang,
                    ui,
                    d,
                    "view",
                    &[("default", "Default"), ("fitPage", "Fit Page"), ("fitWidth", "Fit Width"), ("actual", "Actual Size")],
                );
                ui.end_row();
                crate::rtl::label(ui, tr("Layout:"));
                translated_combo(
                    lang,
                    ui,
                    d,
                    "layout",
                    &[
                        ("default", "Default"),
                        ("single", "Single Page"),
                        ("continuous", "Single Page Continuous"),
                        ("twoUp", "Two-Up (Facing)"),
                        ("twoUpCover", "Two-Up (Cover Page)"),
                        ("twoUpContinuous", "Two-Up Continuous (Cover Page)"),
                    ],
                );
                ui.end_row();
            });
            check(ui, d, "fullScreen", &tr("Open in Full Screen Mode"));
        });
    });
    ui.add_space(4.0);
    group(ui, |ui| {
        crate::rtl::label(ui, egui::RichText::new(tr("Options")).font(semibold(12.0)));
        ui.columns(2, |cols| {
            check(&mut cols[0], d, "tagged", &tr("Create Tagged PDF"));
            check(&mut cols[1], d, "bookmarksPanel", &tr("Show Bookmarks Panel"));
            check(&mut cols[0], d, "media", &tr("Include Video and Sound"));
        });
    });
}

/// A text field that can be disabled, `w` wide.
fn field(ui: &mut egui::Ui, d: &mut Dialog, key: &str, w: f32, enabled: bool) {
    let mut v = d.s(key);
    if ui.add_enabled(enabled, egui::TextEdit::singleline(&mut v).desired_width(w)).changed() {
        d.fields.insert(key.into(), Value::String(v));
    }
}

/// A row with its caption in a column `w` wide.
fn row(ui: &mut egui::Ui, w: f32, caption: &str, add: impl FnOnce(&mut egui::Ui)) {
    ui.horizontal(|ui| {
        ui.allocate_ui_with_layout(egui::vec2(w, ui.spacing().interact_size.y), egui::Layout::left_to_right(egui::Align::Center), |ui| {
            ui.set_min_width(w);
            ui.add(egui::Label::new(crate::rtl::widget(ui, caption)).truncate());
        });
        add(ui);
    });
}

/// One image class's group in Compression.
fn image_class(lang: &str, ui: &mut egui::Ui, d: &mut Dialog, title: &str, prefix: &str, mono: bool) {
    let tr = |s: &str| i18n::tr(lang, s).to_string();
    let key = |k: &str| format!("{prefix}{k}");
    const CAPTION: f32 = 210.0;
    group(ui, |ui| {
        crate::rtl::label(ui, egui::RichText::new(tr(title)).font(semibold(12.0)));
        let resample = d.s(&key("Downsample")) != "none";
        ui.horizontal(|ui| {
            ui.allocate_ui_with_layout(egui::vec2(CAPTION, ui.spacing().interact_size.y), egui::Layout::left_to_right(egui::Align::Center), |ui| {
                translated_combo(lang, ui, d, &key("Downsample"), DOWNSAMPLE);
            });
            field(ui, d, &key("Ppi"), 60.0, resample);
            crate::rtl::label(ui, tr("pixels per inch"));
        });
        row(ui, CAPTION, &tr("for images above:"), |ui| {
            field(ui, d, &key("Above"), 60.0, resample);
            crate::rtl::label(ui, tr("pixels per inch"));
        });
        row(ui, CAPTION, &tr("Compression:"), |ui| {
            if mono {
                ui.add_enabled(false, egui::Label::new(tr("ZIP")));
            } else {
                translated_combo(lang, ui, d, &key("Compression"), COMPRESSION);
            }
        });
        if !mono {
            let lossy = d.s(&key("Compression")) != "zip";
            row(ui, CAPTION, &tr("Image Quality:"), |ui| {
                ui.add_enabled_ui(lossy, |ui| translated_combo(lang, ui, d, &key("Quality"), QUALITY));
            });
        }
    });
    ui.add_space(4.0);
}

fn compression(lang: &str, ui: &mut egui::Ui, d: &mut Dialog) {
    let tr = |s: &str| i18n::tr(lang, s).to_string();
    heading(lang, ui, "Compression");
    image_class(lang, ui, d, "Color Images", "color", false);
    image_class(lang, ui, d, "Grayscale Images", "gray", false);
    image_class(lang, ui, d, "Monochrome Images", "mono", true);
    ui.columns(2, |cols| {
        check(&mut cols[0], d, "compressText", &tr("Compress Text and Line Art"));
        check(&mut cols[1], d, "cropToFrames", &tr("Crop Image Data to Frames"));
    });
    crate::rtl::label(
        ui,
        egui::RichText::new(tr("Resolution is measured at the size each image is placed. Black-and-white images stay lossless."))
            .small()
            .color(crate::theme::Tokens::get(ui.ctx()).text_dim),
    );
}

const MARK_KEYS: [(&str, &str); 5] = [
    ("marksCrop", "Crop Marks"),
    ("marksBleed", "Bleed Marks"),
    ("marksRegistration", "Registration Marks"),
    ("marksColorBars", "Color Bars"),
    ("marksPageInfo", "Page Information"),
];

fn marks(app: &DesignApp, lang: &str, ui: &mut egui::Ui, d: &mut Dialog) {
    let tr = |s: &str| i18n::tr(lang, s).to_string();
    heading(lang, ui, "Marks and Bleeds");
    group(ui, |ui| {
        crate::rtl::label(ui, egui::RichText::new(tr("Marks")).font(semibold(12.0)));
        ui.columns(2, |cols| {
            let ui = &mut cols[0];
            // All Printer's Marks: on when every mark is, mixed when some are.
            let on = MARK_KEYS.iter().filter(|(k, _)| d.b(k)).count();
            let mut all = on == MARK_KEYS.len();
            let resp = ui
                .add(egui::Checkbox::new(&mut all, crate::rtl::widget(ui, tr("All Printer's Marks"))).indeterminate(on > 0 && on < MARK_KEYS.len()));
            if resp.changed() {
                for (k, _) in MARK_KEYS {
                    d.fields.insert(k.into(), json!(all));
                }
            }
            ui.indent("pdf_marks", |ui| {
                for (k, label) in MARK_KEYS {
                    check(ui, d, k, &tr(label));
                }
            });
            let ui = &mut cols[1];
            egui::Grid::new("pdf_marks_size").num_columns(2).spacing([8.0, 4.0]).show(ui, |ui| {
                crate::rtl::label(ui, tr("Weight:"));
                let cur = d.s("marksWeight");
                craft_ui::ComboBox::from_id_salt("pdf_mark_weight").selected_text(format!("{cur} pt")).width(90.0).show_ui(ui, |ui| {
                    for w in ["0.125", "0.25", "0.5"] {
                        if ui.selectable_label(cur == w, format!("{w} pt")).clicked() {
                            d.fields.insert("marksWeight".into(), json!(w));
                        }
                    }
                });
                ui.end_row();
                crate::rtl::label(ui, tr("Offset:"));
                ui.horizontal(|ui| {
                    text_field(ui, d, "marksOffset", 70.0);
                    // A bare number is in points; "2 mm" works too.
                    crate::rtl::label(ui, "pt");
                });
                ui.end_row();
            });
        });
    });
    ui.add_space(4.0);
    group(ui, |ui| {
        crate::rtl::label(ui, egui::RichText::new(tr("Bleed and Slug")).font(semibold(12.0)));
        check(ui, d, "bleed", &tr("Include Bleed"));
        ui.add_enabled_ui(d.b("bleed"), |ui| {
            let was = d.b("useDocBleed");
            check(ui, d, "useDocBleed", &tr("Use Document Bleed Settings"));
            // Back to the document's: show its values again.
            if d.b("useDocBleed")
                && !was
                && let Some(st) = app.session.active()
            {
                for (k, v) in ["bleedTop", "bleedBottom", "bleedInside", "bleedOutside"].iter().zip(st.doc.settings.bleed) {
                    d.fields.insert((*k).into(), json!(format_measure(v, d.unit)));
                }
            }
            let custom = !d.b("useDocBleed");
            for [(a, ka), (b, kb)] in [[("Top:", "bleedTop"), ("Inside:", "bleedInside")], [("Bottom:", "bleedBottom"), ("Outside:", "bleedOutside")]]
            {
                ui.horizontal(|ui| {
                    row(ui, 70.0, &tr(a), |ui| field(ui, d, ka, 80.0, custom));
                    ui.add_space(16.0);
                    row(ui, 70.0, &tr(b), |ui| field(ui, d, kb, 80.0, custom));
                });
            }
        });
        check(ui, d, "slug", &tr("Include Slug Area"));
    });
}

/// Output's hover help: what the control under the pointer does (our own words).
fn output_help(key: &str) -> &'static str {
    match key {
        "conversion" => {
            "No Color Conversion writes colours as they were defined. Convert to Destination converts every colour to the destination profile. Preserve Numbers converts only colours of the other model (RGB for a CMYK destination) and keeps the numbers of the rest."
        }
        "destination" => "The profile of the press or device the PDF is made for. Converted colours and the output intent use it.",
        "include" => {
            "Include Destination Profile writes the destination as the PDF's output intent, so a RIP or viewer knows the printing condition. PDF/X always includes it."
        }
        "inks" => "The Ink Manager sets which spot inks print as process colours and which are aliased to another ink.",
        "intent" => {
            "How colours outside the destination's gamut are brought in. Perceptual keeps the relations between colours, Relative Colorimetric keeps in-gamut colours exact, Saturation keeps colours vivid, Absolute Colorimetric also simulates the paper white."
        }
        "load" => {
            "Load a CMYK ICC profile (.icc or .icm) from disk, such as an uncoated or newspaper press condition, and use it as the destination."
        }
        "pdfx" => "The printing condition a PDF/X file names: the destination profile and its registered characterisation.",
        _ => "Position the pointer over a heading to view a description.",
    }
}

fn output(app: &mut DesignApp, lang: &str, ui: &mut egui::Ui, d: &mut Dialog) {
    let tr = |s: &str| i18n::tr(lang, s).to_string();
    let mut hovered = String::new();
    let hover = |r: &egui::Response, key: &str, hovered: &mut String| {
        if r.hovered() {
            *hovered = key.to_string();
        }
    };
    heading(lang, ui, "Output");
    let profiles = designcraft_color::cms::profiles();
    let pdfx = matches!(d.s("standard").as_str(), "x4" | "x1a");
    group(ui, |ui| {
        crate::rtl::label(ui, egui::RichText::new(tr("Color")).font(semibold(12.0)));
        egui::Grid::new("pdf_output").num_columns(2).spacing([8.0, 6.0]).show(ui, |ui| {
            let r = crate::rtl::label(ui, tr("Color Conversion:"));
            hover(&r, "conversion", &mut hovered);
            translated_combo(
                lang,
                ui,
                d,
                "conversion",
                &[
                    ("none", "No Color Conversion"),
                    ("destination", "Convert to Destination"),
                    ("preserveNumbers", "Convert to Destination (Preserve Numbers)"),
                ],
            );
            ui.end_row();
            let r = crate::rtl::label(ui, tr("Destination:"));
            hover(&r, "destination", &mut hovered);
            let cur = d.s("destination");
            let needs_dest = d.s("conversion") != "none" || d.b("includeProfile") || pdfx;
            ui.add_enabled_ui(needs_dest, |ui| {
                let shown = if cur.is_empty() { tr("Working CMYK space") } else { cur.clone() };
                craft_ui::ComboBox::from_id_salt("pdf_destination").selected_text(crate::rtl::isolate(&shown)).width(300.0).show_ui(ui, |ui| {
                    if ui.selectable_label(cur.is_empty(), tr("Working CMYK space")).clicked() {
                        d.fields.insert("destination".into(), json!(""));
                    }
                    for (kind, title) in [(designcraft_color::cms::ProfileKind::Cmyk, "CMYK"), (designcraft_color::cms::ProfileKind::Rgb, "RGB")] {
                        // PDF/X takes only a CMYK destination.
                        if pdfx && kind != designcraft_color::cms::ProfileKind::Cmyk {
                            continue;
                        }
                        ui.separator();
                        crate::rtl::label(ui, egui::RichText::new(title).small());
                        // An RGB destination is sRGB: vector colours can't carry another RGB profile.
                        for p in profiles.iter().filter(|p| {
                            p.kind == kind && (kind != designcraft_color::cms::ProfileKind::Rgb || p.name == designcraft_color::cms::SRGB)
                        }) {
                            if ui.selectable_label(cur == p.name, crate::rtl::isolate(&p.name)).clicked() {
                                d.fields.insert("destination".into(), json!(p.name));
                            }
                        }
                    }
                });
            });
            ui.end_row();
            let r = crate::rtl::label(ui, tr("Rendering Intent:"));
            hover(&r, "intent", &mut hovered);
            ui.add_enabled_ui(d.s("conversion") != "none", |ui| {
                translated_combo(
                    lang,
                    ui,
                    d,
                    "intent",
                    &[
                        ("", "Use Color Settings"),
                        ("perceptual", "Perceptual"),
                        ("relative", "Relative Colorimetric"),
                        ("saturation", "Saturation"),
                        ("absolute", "Absolute Colorimetric"),
                    ],
                );
            });
            ui.end_row();
            let r = crate::rtl::label(ui, tr("Profile Inclusion Policy:"));
            hover(&r, "include", &mut hovered);
            ui.add_enabled_ui(!pdfx, |ui| {
                let inc = d.b("includeProfile") || pdfx;
                let label = tr(if inc { "Include Destination Profile" } else { "Don't Include Profiles" });
                craft_ui::ComboBox::from_id_salt("pdf_include").selected_text(label).width(300.0).show_ui(ui, |ui| {
                    if ui.selectable_label(!inc, tr("Don't Include Profiles")).clicked() {
                        d.fields.insert("includeProfile".into(), json!(false));
                    }
                    if ui.selectable_label(inc, tr("Include Destination Profile")).clicked() {
                        d.fields.insert("includeProfile".into(), json!(true));
                    }
                });
            });
            ui.end_row();
        });
        ui.horizontal(|ui| {
            // A press profile that isn't built in (an uncoated or newspaper condition, say).
            if app.services.pick_open.is_some() {
                let r = ui.button(crate::rtl::widget(ui, tr("Load Profile…")));
                hover(&r, "load", &mut hovered);
                if r.clicked() {
                    load_profile(app, d);
                }
            }
            let r = crate::menus::menu_button(ui, crate::rtl::widget(ui, tr("Ink Manager…")), |ui| ink_manager(app, lang, ui));
            hover(&r.response, "inks", &mut hovered);
        });
    });
    ui.add_space(4.0);
    group(ui, |ui| {
        let r = crate::rtl::label(ui, egui::RichText::new("PDF/X").font(semibold(12.0)));
        hover(&r, "pdfx", &mut hovered);
        let working = designcraft_color::cms::active_settings().cmyk;
        let dest = d.s("destination");
        let dest = if dest.is_empty() { working } else { dest };
        let (name, condition, id, registry) = if pdfx {
            let registered = dest == designcraft_color::cms::COATED_FOGRA39;
            (
                dest.clone(),
                dest.clone(),
                if registered { "FOGRA39".to_string() } else { "Custom".to_string() },
                if registered { "http://www.color.org".to_string() } else { String::new() },
            )
        } else {
            ("N/A".to_string(), "N/A".to_string(), "N/A".to_string(), "N/A".to_string())
        };
        egui::Grid::new("pdf_pdfx").num_columns(2).spacing([8.0, 4.0]).show(ui, |ui| {
            for (label, value) in [
                ("Output Intent Profile Name:", name),
                ("Output Condition Name:", condition),
                ("Output Condition Identifier:", id),
                ("Registry Name:", registry),
            ] {
                ui.add_enabled_ui(pdfx, |ui| crate::rtl::label(ui, tr(label)));
                let mut v = value;
                ui.add_enabled(false, egui::TextEdit::singleline(&mut v).desired_width(300.0));
                ui.end_row();
            }
        });
    });
    ui.add_space(4.0);
    group(ui, |ui| {
        crate::rtl::label(ui, egui::RichText::new(tr("Description")).font(semibold(12.0)));
        crate::rtl::label(ui, egui::RichText::new(tr(output_help(&hovered))).color(crate::theme::Tokens::get(ui.ctx()).text_dim));
    });
}

/// Load Profile…: an ICC profile from disk becomes the destination, and is loaded again at the
/// next start, as Color Settings' Load Profile does.
pub(crate) fn load_profile(app: &mut DesignApp, d: &mut Dialog) {
    let Some(path) = app.services.pick_open.as_mut().and_then(|f| f("icc")) else { return };
    match app.session.execute("color.loadProfile", &json!({"path": path})) {
        Ok(info) => {
            app.ui.color_profiles.retain(|p| p != &path);
            app.ui.color_profiles.push(path);
            match (info["name"].as_str(), info["kind"].as_str()) {
                (Some(name), Some("cmyk")) => {
                    d.fields.insert("destination".into(), json!(name));
                }
                // Loaded for Color Settings and proofing, but not a PDF destination.
                (Some(name), _) => {
                    let msg = i18n::tr(&app.ui.language, "{name} is loaded; the PDF destination is a CMYK profile or sRGB").replace("{name}", name);
                    d.fields.insert("status".into(), json!(msg));
                }
                _ => {}
            }
        }
        Err(e) => app.status_error("Export PDF", e),
    }
}

/// The Ink Manager's spot inks, as in the Swatches panel menu.
fn ink_manager(app: &mut DesignApp, lang: &str, ui: &mut egui::Ui) {
    let tr = |s: &str| i18n::tr(lang, s).to_string();
    let Ok(l) = app.session.execute("ink.list", &json!({})) else {
        crate::rtl::label(ui, tr("No spot inks"));
        return;
    };
    let mut all = l["allToProcess"].as_bool().unwrap_or(false);
    if ui.checkbox(&mut all, crate::rtl::widget(ui, tr("All Spots to Process"))).changed() {
        let _ = app.run("ink.options", json!({"allToProcess": all}));
    }
    ui.separator();
    let inks = l["inks"].as_array().cloned().unwrap_or_default();
    if inks.is_empty() {
        crate::rtl::label(ui, tr("No spot inks"));
    }
    for ink in inks {
        let name = ink["name"].as_str().unwrap_or("").to_string();
        let mut process = ink["process"].as_bool().unwrap_or(false);
        ui.add_enabled_ui(!all, |ui| {
            if ui.checkbox(&mut process, format!("{name} → {}", tr("process"))).changed() {
                let _ = app.run("ink.options", json!({"ink": name, "toProcess": process}));
            }
        });
    }
}

fn advanced(lang: &str, ui: &mut egui::Ui, d: &mut Dialog) {
    let tr = |s: &str| i18n::tr(lang, s).to_string();
    heading(lang, ui, "Advanced");
    group(ui, |ui| {
        crate::rtl::label(ui, egui::RichText::new(tr("Transparency Flattener")).font(semibold(12.0)));
        ui.horizontal(|ui| {
            crate::rtl::label(ui, tr("Preset:"));
            translated_combo(
                lang,
                ui,
                d,
                "flatten",
                // "none" is a choice; "" (never chosen) follows Edit › Transparency Flattener Presets.
                &[("none", "None"), ("high", "High Resolution"), ("medium", "Medium Resolution"), ("low", "Low Resolution")],
            );
        });
        crate::rtl::label(
            ui,
            egui::RichText::new(tr("Spreads with transparency are flattened at the preset's resolution. PDF/X-1a always flattens."))
                .small()
                .color(crate::theme::Tokens::get(ui.ctx()).text_dim),
        );
    });
    ui.add_space(4.0);
    group(ui, |ui| {
        crate::rtl::label(ui, egui::RichText::new(tr("Document")).font(semibold(12.0)));
        ui.horizontal(|ui| {
            crate::rtl::label(ui, tr("Title:"));
            let mut t = d.s("title");
            let r = ui.add(egui::TextEdit::singleline(&mut t).hint_text(tr("From File Info")).desired_width(300.0));
            if r.changed() {
                d.fields.insert("title".into(), json!(t));
            }
        });
    });
}

fn summary_section(app: &DesignApp, lang: &str, ui: &mut egui::Ui, d: &mut Dialog) {
    let tr = |s: &str| i18n::tr(lang, s).to_string();
    heading(lang, ui, "Summary");
    let s = settings_from(d);
    group(ui, |ui| {
        for line in summary(lang, &s) {
            crate::rtl::label(ui, line);
        }
    });
    let unit = app.session.active().map_or(Unit::Points, |st| st.doc.settings.horizontal_units);
    let mut problems: Vec<String> = warnings(&s, &designcraft_color::cms::active_settings().cmyk).iter().map(|w| tr(w)).collect();
    if let Err(e) = export_params(&s, unit) {
        problems.insert(0, e);
    }
    ui.add_space(6.0);
    crate::rtl::label(ui, egui::RichText::new(tr("Warnings")).font(semibold(12.0)));
    if problems.is_empty() {
        crate::rtl::label(ui, egui::RichText::new(tr("None")).color(crate::theme::Tokens::get(ui.ctx()).text_dim));
    }
    for p in problems {
        let warn = ui.visuals().warn_fg_color;
        crate::rtl::label(ui, egui::RichText::new(format!("⚠ {p}")).color(warn));
    }
}

/// The footer's Save Preset… (a name in a popup; the dialog ignores Enter while it is open).
pub(crate) fn footer(app: &mut DesignApp, ui: &mut egui::Ui, d: &mut Dialog) {
    let lang = app.ui.language.clone();
    let tr = |s: &str| i18n::tr(&lang, s).to_string();
    let save = ui.button(crate::rtl::widget(ui, tr("Save Preset…")));
    egui::Popup::menu(&save).close_behavior(egui::PopupCloseBehavior::CloseOnClickOutside).show(|ui| {
        ui.set_min_width(240.0);
        crate::rtl::label(ui, tr("Save Preset As:"));
        let mut name = d.s("_presetName");
        let field = ui.add(egui::TextEdit::singleline(&mut name).hint_text(tr("My Print Settings")).desired_width(220.0));
        if save.clicked() {
            field.request_focus();
        }
        if field.changed() {
            d.fields.insert("_presetName".into(), json!(name));
        }
        let enter = field.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));
        if ui.button(crate::rtl::widget(ui, tr("Save Preset"))).clicked() || enter {
            match save_preset(app, d, &name) {
                Ok(()) => ui.close(),
                Err(e) => app.status(e),
            }
        }
    });
    // A saved preset can be deleted while it is selected.
    let cur = d.s("preset");
    if app.ui.pdf_export_presets.iter().any(|p| p.name == cur) && ui.button(crate::rtl::widget(ui, tr("Delete Preset"))).clicked() {
        app.ui.pdf_export_presets.retain(|p| p.name != cur);
        d.fields.insert("preset".into(), json!("Custom"));
    }
}

/// OK: remember the settings, then pick where to save and export.
pub(crate) fn confirm(app: &mut DesignApp, d: &Dialog) -> Result<Value, String> {
    let settings = settings_from(d);
    let unit = app.session.active().map_or(Unit::Points, |st| st.doc.settings.horizontal_units);
    let mut params = export_params(&settings, unit)?;
    app.ui.pdf_export = settings;
    let pages = d.s("pages");
    if d.s("pagesMode") == "range" && !pages.trim().is_empty() && pages != "all" {
        params["pages"] = json!(pages);
    } else if d.s("pagesMode").is_empty() && !pages.trim().is_empty() && pages != "all" {
        // A dialog that never drew (scripts, tests): a given range applies.
        params["pages"] = json!(pages);
    }
    let title = d.s("title");
    // Without a title the document's name names the file, and the PDF's title comes from
    // File Info.
    let doc_name = app.session.active().map(|st| st.doc.title.clone()).unwrap_or_default();
    if !title.is_empty() {
        params["title"] = json!(title);
    }
    let stem = if title.is_empty() { doc_name } else { title };
    let name = if stem.is_empty() { "Export.pdf".to_string() } else { format!("{stem}.pdf") };
    match app.pick_save(&name) {
        Some(path) => {
            params["path"] = json!(path);
            crate::menus::export_pdf(app, &params)
        }
        // The user cancelled the save dialog: export nothing.
        None => Ok(Value::Null),
    }
}
