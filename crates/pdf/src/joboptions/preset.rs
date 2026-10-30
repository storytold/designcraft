//! A parsed `.joboptions` as the Export PDF fields DesignCraft can set.
//!
//! Every field is an `Option`: `None` means the file says nothing, so the dialog keeps what the
//! user set. What DesignCraft cannot do goes in `not_applied`, in plain words, so nothing is
//! dropped in silence.

use serde::{Deserialize, Serialize};

use super::JobOptions;

/// An Adobe PDF preset, imported. Saved in `ui.json` as `pdfPresets`.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct ImportedPreset {
    pub name: String,
    /// `none`, `x1a` or `x4`.
    pub standard: Option<String>,
    pub compress_images: Option<bool>,
    pub bleed: Option<bool>,
    pub marks_crop: Option<bool>,
    pub marks_bleed: Option<bool>,
    pub marks_page_info: Option<bool>,
    /// Points, as the dialog shows them.
    pub marks_weight: Option<String>,
    pub marks_offset: Option<String>,
    pub tagged: Option<bool>,
    /// `""` (no flattening) or `high`.
    pub flatten: Option<String>,
    pub not_applied: Vec<String>,
}

/// The largest mark weight or offset that is read: a real file has a few points.
const MAX_MARK_POINTS: f64 = 1000.0;

/// A number as the dialog's text fields show it: `6`, `0.25`.
fn points(n: f64) -> String {
    format!("{n}")
}

/// Map `jo` to Export PDF fields. `name` is what the Preset menu shows.
pub fn to_preset(name: &str, jo: &JobOptions) -> ImportedPreset {
    let mut p = ImportedPreset { name: name.to_string(), ..ImportedPreset::default() };
    let mut note = |text: String| p.not_applied.push(text);
    // The standard.
    let compliance = jo.names("CheckCompliance");
    let x1a = jo.flag("PDFX1aCheck") == Some(true) || compliance.iter().any(|n| n.starts_with("PDFX1a"));
    let x3 = jo.flag("PDFX3Check") == Some(true) || compliance.iter().any(|n| n.starts_with("PDFX3"));
    let x4 = compliance.iter().any(|n| n.starts_with("PDFX4"));
    let pdf_a = compliance.iter().find(|n| n.starts_with("PDFA"));
    let stated = jo.flag("PDFX1aCheck").is_some() || jo.flag("PDFX3Check").is_some() || !compliance.is_empty();
    let standard = if x1a {
        Some("x1a")
    } else if x3 {
        note("PDF/X-3 was asked for. DesignCraft has no PDF/X-3, so it uses PDF/X-4.".into());
        Some("x4")
    } else if x4 {
        Some("x4")
    } else if let Some(a) = pdf_a {
        if !a.starts_with("PDFA2") {
            let asked = if a.starts_with("PDFA1") { "PDF/A-1b" } else { "PDF/A" };
            note(format!("{asked} was asked for. DesignCraft writes PDF/A-2b."));
        }
        Some("a2b")
    } else if stated {
        Some("none")
    } else {
        None
    };
    // The rest of the mapping, then the report of what cannot be done.
    if jo.flag("AddRegMarks") == Some(true) {
        note("Registration marks are not supported.".into());
    }
    if jo.flag("AddColorBars") == Some(true) {
        note("Colour bars are not supported.".into());
    }
    if let Some(v) = jo.number("CompatibilityLevel").filter(|v| (*v - 1.7).abs() > f64::EPSILON) {
        note(format!("PDF version {v} was asked for. DesignCraft writes its own version."));
    }
    if jo.flag("DownsampleColorImages") == Some(true) {
        note(match jo.number("ColorImageResolution") {
            Some(ppi) => format!("Images are not downsampled to {ppi} ppi. They keep their own resolution."),
            None => "Images are not downsampled. They keep their own resolution.".into(),
        });
    }
    if let Some(n) = jo.name("ColorConversionStrategy").filter(|n| *n != "LeaveColorUnchanged") {
        note(format!("Colour conversion ({n}) is not supported."));
    }
    if let Some(n) = jo.name("DefaultRenderingIntent").filter(|n| *n != "Default") {
        note(format!("Rendering intent {n} is not supported."));
    }
    if matches!(standard, Some("x1a" | "x4"))
        && let Some(profile) = jo.text("PDFXOutputIntentProfile").map(str::trim).filter(|p| !p.is_empty() && !p.eq_ignore_ascii_case("none"))
    {
        note(format!("Output intent profile \"{profile}\" is not chosen by the preset. DesignCraft uses its own."));
    }
    if jo.flag("SubsetFonts") == Some(false) {
        note("Full font embedding is not supported. Fonts are subset.".into());
    }
    if jo.flag("Optimize") == Some(true) {
        note("Fast web view is not supported.".into());
    }

    // A mark's weight or offset must be a sane number of points; anything else is left to the dialog.
    let mut mark = |key: &str, what: &str| {
        let value = jo.number(key)?;
        if value.is_finite() && (0.0..=MAX_MARK_POINTS).contains(&value) {
            return Some(points(value));
        }
        note(format!("Mark {what} {value} is not a sensible number of points, so it is not used."));
        None
    };
    let (weight, offset) = (mark("MarksWeight", "weight"), mark("MarksOffset", "offset"));

    p.standard = standard.map(str::to_string);
    p.compress_images = if jo.flag("EncodeColorImages") == Some(false) {
        Some(false)
    } else if jo.flag("AutoFilterColorImages") == Some(true) {
        Some(true)
    } else {
        jo.name("ColorImageFilter").map(|f| f == "DCTEncode")
    };
    p.bleed = jo.flag("UseDocumentBleed");
    p.marks_crop = jo.flag("AddCropMarks");
    p.marks_bleed = jo.flag("AddBleedMarks");
    p.marks_page_info = jo.flag("AddPageInfo");
    p.marks_weight = weight;
    p.marks_offset = offset;
    p.tagged = jo.flag("GenerateStructure");
    // InDesign flattens transparency only for PDF 1.3 and older. `/AllowTransparency` is a Distiller
    // setting that Adobe's own presets set to false at every version, so it is not read.
    p.flatten = jo.number("CompatibilityLevel").map(|v| if v < 1.4 { "high" } else { "" }.to_string());
    p
}
