//! File › Export Layouts and Versions: the document's alternate layouts, one file per layout or
//! all in one, in several versions at once (a press PDF with marks and bleed, a screen PDF, PNG or
//! JPEG images…), each version in a folder of its own, named from a template. A job can be saved
//! as a preset and run again.

use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use super::{CommandSpec, bad, cmd, has_doc, ok, str_param};
use crate::{EngineError, Result, Session};

const ID: &str = "file.exportBatch";
/// The most files one export writes.
const MAX_FILES: usize = 5_000;
/// The most versions in a job.
const MAX_VERSIONS: usize = 24;
/// The most pixels in one exported image (the limit EPUB page images have: an RGBA buffer of
/// about 320 MB before encoding).
const MAX_PIXELS: f64 = 8.0e7;
/// The most characters in a file or folder name.
const MAX_NAME: usize = 120;
/// The most saved export presets, and the longest preset name.
const MAX_PRESETS: usize = 100;
const MAX_PRESET_NAME: usize = 80;

/// What a version is exported as.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ExportFormat {
    #[default]
    Pdf,
    Png,
    Jpeg,
}

impl ExportFormat {
    pub fn ext(self) -> &'static str {
        match self {
            ExportFormat::Pdf => "pdf",
            ExportFormat::Png => "png",
            ExportFormat::Jpeg => "jpg",
        }
    }
}

/// Printer's marks of a PDF version.
#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct VersionMarks {
    pub crop: bool,
    pub bleed: bool,
    pub page_info: bool,
}

/// One output of a job: a format and its settings. Its name is its folder.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct ExportVersion {
    pub name: String,
    pub format: ExportFormat,
    /// PDF: the document's bleed.
    pub bleed: bool,
    /// PDF: printer's marks.
    pub marks: VersionMarks,
    /// PDF: none | x4 | a2b.
    pub standard: String,
    /// PDF: spreads rather than pages.
    pub spreads: bool,
    /// PDF: downsample and compress images.
    pub compress_images: bool,
    /// PNG / JPEG: pixels per inch.
    pub ppi: f64,
    /// JPEG: quality, 1–100.
    pub quality: u8,
}

impl Default for ExportVersion {
    fn default() -> Self {
        ExportVersion {
            name: "PDF".into(),
            format: ExportFormat::Pdf,
            bleed: false,
            marks: VersionMarks::default(),
            standard: "none".into(),
            spreads: false,
            compress_images: false,
            ppi: 150.0,
            quality: 85,
        }
    }
}

/// An export job (and what a preset keeps).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct ExportJob {
    /// The layouts to export, by name (empty: all of them).
    pub layouts: Vec<String>,
    /// PDF versions: all the chosen layouts in one file rather than a file per layout.
    pub combine: bool,
    /// File name template: {document}, {layout}, {version}, {date}.
    pub file_name: String,
    pub versions: Vec<ExportVersion>,
    /// Each version in a folder of its own (named after it).
    pub folders: bool,
}

impl Default for ExportJob {
    fn default() -> Self {
        ExportJob { layouts: Vec::new(), combine: false, file_name: "{document} - {layout}".into(), versions: vec![], folders: true }
    }
}

/// A saved export preset.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct ExportPreset {
    pub name: String,
    pub job: ExportJob,
}

/// The ready-made versions: a press PDF (crop and bleed marks, bleed), a PDF with bleed and no
/// marks, a screen PDF, and PNG and JPEG images.
pub fn version_templates() -> Vec<ExportVersion> {
    let pdf = ExportVersion::default();
    vec![
        ExportVersion {
            name: "PDF Print (marks + bleed)".into(),
            bleed: true,
            marks: VersionMarks { crop: true, bleed: true, page_info: true },
            ..pdf.clone()
        },
        ExportVersion { name: "PDF Bleed (no marks)".into(), bleed: true, ..pdf.clone() },
        ExportVersion { name: "PDF Screen".into(), compress_images: true, ..pdf.clone() },
        ExportVersion { name: "PNG".into(), format: ExportFormat::Png, ..pdf.clone() },
        ExportVersion { name: "JPEG".into(), format: ExportFormat::Jpeg, ..pdf },
    ]
}

/// The built-in presets, listed before the saved ones; they can't be replaced or deleted.
pub fn builtin_presets() -> Vec<ExportPreset> {
    let t = version_templates();
    let pick = |names: &[&str]| -> Vec<ExportVersion> { t.iter().filter(|v| names.contains(&v.name.as_str())).cloned().collect() };
    vec![
        ExportPreset {
            name: "Print + Screen".into(),
            job: ExportJob { versions: pick(&["PDF Print (marks + bleed)", "PDF Screen", "PNG"]), ..ExportJob::default() },
        },
        ExportPreset {
            name: "Press PDF per Layout".into(),
            job: ExportJob { versions: pick(&["PDF Print (marks + bleed)", "PDF Bleed (no marks)"]), ..ExportJob::default() },
        },
        ExportPreset { name: "Images per Layout".into(), job: ExportJob { versions: pick(&["PNG", "JPEG"]), ..ExportJob::default() } },
        ExportPreset {
            name: "All Layouts in One PDF".into(),
            job: ExportJob {
                combine: true,
                folders: false,
                file_name: "{document}".into(),
                versions: pick(&["PDF Bleed (no marks)"]),
                ..ExportJob::default()
            },
        },
    ]
}

pub fn specs() -> Vec<CommandSpec> {
    vec![
        cmd!(noundo "file.exportBatch", "Export Layouts and Versions…", ["File"], None,
            "{job?: {layouts?: [layout names] (default all), combine?: bool (PDF: all the layouts in one file), fileName?: \"{document} - {layout}\" (also {version}, {date}), versions: [{name, format: pdf|png|jpeg, bleed?, marks?: {crop?, bleed?, pageInfo?}, standard?: none|x4|a2b, spreads?, compressImages?, ppi?: 150, quality?: 85}], folders?: bool (default true: a folder per version)}, preset?: name (instead of or under `job`), dir?: folder to write into, dryRun?: bool (list the files only), date?: \"YYYY-MM-DD\" (for {date})} → {files: [{path, version, layout, pages, bytes, base64? (no dir)}], warnings}",
            has_doc, export_batch),
        cmd!(query "file.exportPresets", "Export Presets", [], None,
            "{} → {presets: [{name, job, builtIn}], versions: [the ready-made versions]}", super::always, |s, _| {
            let mut presets: Vec<Value> = builtin_presets().into_iter().map(|p| json!({"name": p.name, "job": p.job, "builtIn": true})).collect();
            presets.extend(s.prefs.export_presets.iter().map(|p| json!({"name": p.name, "job": p.job, "builtIn": false})));
            Ok(json!({"presets": presets, "versions": version_templates()}))
        }),
        cmd!(noundo "file.saveExportPreset", "Save Export Preset", [], None,
            "{name, job} — kept with the preferences; a saved preset of that name is replaced, built-in names are refused", super::always, save_preset),
        cmd!(noundo "file.deleteExportPreset", "Delete Export Preset", [], None, "{name} — a saved export preset", super::always, |s, p| {
            let name = str_param(p, "name").unwrap_or_default().trim().to_string();
            let before = s.prefs.export_presets.len();
            s.prefs.export_presets.retain(|x| !x.name.eq_ignore_ascii_case(&name));
            if s.prefs.export_presets.len() == before {
                return Err(bad("file.deleteExportPreset", format!("no saved export preset `{name}`")));
            }
            ok()
        }),
    ]
}

fn save_preset(s: &mut Session, p: &Value) -> Result<Value> {
    const CMD: &str = "file.saveExportPreset";
    let name = str_param(p, "name").map(str::trim).filter(|n| !n.is_empty()).ok_or_else(|| bad(CMD, "missing `name`"))?.to_string();
    if name.chars().count() > MAX_PRESET_NAME {
        return Err(bad(CMD, format!("a preset name has at most {MAX_PRESET_NAME} characters")));
    }
    if builtin_presets().iter().any(|b| b.name.eq_ignore_ascii_case(&name)) {
        return Err(bad(CMD, format!("`{name}` is a built-in preset")));
    }
    let job = job_param(CMD, p.get("job").ok_or_else(|| bad(CMD, "missing `job`"))?)?;
    let presets = &mut s.prefs.export_presets;
    presets.retain(|x| !x.name.eq_ignore_ascii_case(&name));
    if presets.len() >= MAX_PRESETS {
        return Err(bad(CMD, format!("at most {MAX_PRESETS} saved export presets")));
    }
    presets.push(ExportPreset { name: name.clone(), job });
    Ok(json!({"name": name}))
}

/// A job from JSON, checked: versions present and sane, numbers in range.
fn job_param(cmd: &str, v: &Value) -> Result<ExportJob> {
    let mut job: ExportJob = serde_json::from_value(v.clone()).map_err(|e| bad(cmd, format!("bad `job`: {e}")))?;
    if job.versions.is_empty() {
        return Err(bad(cmd, "a job needs at least one version"));
    }
    if job.versions.len() > MAX_VERSIONS {
        return Err(bad(cmd, format!("at most {MAX_VERSIONS} versions")));
    }
    if job.file_name.chars().count() > 400 {
        return Err(bad(cmd, "the file name template is too long"));
    }
    for v in &mut job.versions {
        if !v.ppi.is_finite() {
            return Err(bad(cmd, format!("version `{}`: `ppi` is a number", v.name)));
        }
        v.ppi = v.ppi.clamp(36.0, 1200.0);
        v.quality = v.quality.clamp(1, 100);
        if designcraft_pdf::Standard::parse(&v.standard).is_none() {
            return Err(bad(cmd, format!("version `{}`: unknown standard `{}` (none, x4, a2b)", v.name, v.standard)));
        }
    }
    Ok(job)
}

/// The pages (absolute, 0-based) of each chosen layout, by name; `names` empty: every layout.
pub(crate) fn layout_pages(cmd: &str, d: &designcraft_doc::Document, names: &[String]) -> Result<Vec<(String, Vec<usize>)>> {
    let layouts = d.layouts();
    if names.is_empty() {
        return Ok(layouts.into_iter().map(|l| (l.name, (l.start..l.end).collect())).collect());
    }
    for n in names {
        if !layouts.iter().any(|l| l.name == *n) {
            return Err(bad(cmd, format!("no layout `{n}`")));
        }
    }
    // In document order, whatever order they were named in.
    Ok(layouts.into_iter().filter(|l| names.contains(&l.name)).map(|l| (l.name, (l.start..l.end).collect())).collect())
}

/// `s` as a file or folder name: no path separators, reserved or control characters, no leading
/// dots, at most [`MAX_NAME`] characters; "Untitled" when nothing is left.
pub fn safe_name(s: &str) -> String {
    let cleaned: String = s.chars().map(|c| if c.is_control() || r#"/\:*?"<>|"#.contains(c) { ' ' } else { c }).collect();
    let words: Vec<&str> = cleaned.split_whitespace().collect();
    let joined = words.join(" ");
    let trimmed = joined.trim_start_matches('.').trim();
    let capped: String = trimmed.chars().take(MAX_NAME).collect();
    let capped = capped.trim_end_matches(['.', ' ']).to_string();
    if capped.is_empty() { "Untitled".into() } else { capped }
}

/// `name.ext`, or `name 2.ext`, `name 3.ext`… when `taken` has it (case-insensitively).
fn unique(taken: &mut Vec<String>, name: &str, ext: &str) -> String {
    let mut k = 1usize;
    loop {
        let candidate = if k == 1 { format!("{name}.{ext}") } else { format!("{name} {k}.{ext}") };
        if !taken.iter().any(|t| t.eq_ignore_ascii_case(&candidate)) {
            taken.push(candidate.clone());
            return candidate;
        }
        k = k.saturating_add(1);
    }
}

/// Today as YYYY-MM-DD (UTC), where there is a clock.
fn today() -> String {
    #[cfg(not(target_arch = "wasm32"))]
    {
        let secs = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_secs());
        let days = i64::try_from(secs / 86_400).unwrap_or(0);
        // Civil date from days since 1970-01-01 (Howard Hinnant's algorithm).
        let z = days + 719_468;
        let era = z.div_euclid(146_097);
        let doe = z.rem_euclid(146_097);
        let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
        let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
        let mp = (5 * doy + 2) / 153;
        let d = doy - (153 * mp + 2) / 5 + 1;
        let m = if mp < 10 { mp + 3 } else { mp - 9 };
        let y = yoe + era * 400 + i64::from(m <= 2);
        format!("{y:04}-{m:02}-{d:02}")
    }
    #[cfg(target_arch = "wasm32")]
    String::new()
}

/// One file a job writes.
struct Planned {
    /// Relative to the export folder: `version folder/name.ext`.
    path: String,
    version: usize,
    layout: String,
    /// Absolute, 0-based.
    pages: Vec<usize>,
}

/// The files `job` writes, in order: for each version, a PDF per layout (or one for all), or an
/// image per page.
fn plan(s: &Session, job: &ExportJob, date: &str) -> Result<Vec<Planned>> {
    let d = &s.doc()?.doc;
    let groups = layout_pages(ID, d, &job.layouts)?;
    if groups.iter().all(|(_, p)| p.is_empty()) {
        return Err(bad(ID, "the chosen layouts have no pages"));
    }
    let document = if d.title.trim().is_empty() { "Untitled" } else { d.title.as_str() };
    let mut out = Vec::new();
    let mut folders: Vec<String> = Vec::new();
    for (vi, v) in job.versions.iter().enumerate() {
        let folder = if job.folders {
            let f = safe_name(&v.name);
            // Two versions with one name share no folder.
            let f = unique(&mut folders, &f, "").trim_end_matches('.').to_string();
            format!("{f}/")
        } else {
            String::new()
        };
        let mut taken: Vec<String> = out.iter().filter_map(|p: &Planned| p.path.strip_prefix(folder.as_str()).map(str::to_string)).collect();
        let name = |layout: &str| {
            safe_name(
                &job.file_name.replace("{document}", document).replace("{layout}", layout).replace("{version}", &v.name).replace("{date}", date),
            )
        };
        match v.format {
            ExportFormat::Pdf if job.combine => {
                let pages: Vec<usize> = groups.iter().flat_map(|(_, p)| p.iter().copied()).collect();
                let file = unique(&mut taken, &name("All Layouts"), "pdf");
                out.push(Planned { path: format!("{folder}{file}"), version: vi, layout: "All Layouts".into(), pages });
            }
            ExportFormat::Pdf => {
                for (layout, pages) in groups.iter().filter(|(_, p)| !p.is_empty()) {
                    let file = unique(&mut taken, &name(layout), "pdf");
                    out.push(Planned { path: format!("{folder}{file}"), version: vi, layout: layout.clone(), pages: pages.clone() });
                }
            }
            ExportFormat::Png | ExportFormat::Jpeg => {
                for (layout, pages) in &groups {
                    let base = name(layout);
                    for &abs in pages {
                        let stem = safe_name(&format!("{base} - {}", s.page_label(abs)));
                        let file = unique(&mut taken, &stem, v.format.ext());
                        out.push(Planned { path: format!("{folder}{file}"), version: vi, layout: layout.clone(), pages: vec![abs] });
                    }
                }
            }
        }
        if out.len() > MAX_FILES {
            return Err(bad(ID, format!("the export would write more than {MAX_FILES} files")));
        }
    }
    Ok(out)
}

/// The `file.exportPdf` parameters of a PDF version for `pages` (absolute, 0-based).
fn pdf_params(v: &ExportVersion, pages: &[usize], title: &str) -> Value {
    json!({
        "pages": pages.iter().map(|p| p.saturating_add(1)).collect::<Vec<_>>(),
        "bleed": v.bleed,
        "marks": {"crop": v.marks.crop, "bleed": v.marks.bleed, "pageInfo": v.marks.page_info},
        "standard": v.standard,
        "spreads": v.spreads,
        "compressImages": v.compress_images,
        "tagged": true,
        "title": title,
    })
}

/// The bytes of one planned file.
fn render(s: &Session, v: &ExportVersion, f: &Planned) -> Result<(Vec<u8>, Vec<String>)> {
    let d = &s.doc()?.doc;
    match v.format {
        ExportFormat::Pdf => {
            let r = super::export::pdf_report(s, &pdf_params(v, &f.pages, &d.title))?;
            Ok((r.bytes, r.warnings))
        }
        ExportFormat::Png | ExportFormat::Jpeg => {
            let abs = f.pages.first().copied().ok_or_else(|| bad(ID, "no page"))?;
            let page = d.page(abs).ok_or_else(|| bad(ID, format!("no page {}", abs.saturating_add(1))))?;
            let scale = v.ppi / 72.0;
            if page.width * scale * page.height * scale > MAX_PIXELS {
                return Err(bad(ID, format!("page {} at {} ppi is too large for an image; lower the resolution", s.page_label(abs), v.ppi)));
            }
            let mut r = designcraft_render::Renderer::new();
            let opts = designcraft_render::RenderOptions { printing_only: true, rich_black: s.prefs.rich_black_output, ..Default::default() };
            let img = r.render_page(d, &s.cache, abs, scale, false, &opts).ok_or_else(|| bad(ID, "the page couldn't be rendered"))?;
            Ok((if v.format == ExportFormat::Png { img.to_png() } else { img.to_jpeg(v.quality) }, vec![]))
        }
    }
}

/// Write `bytes` at `rel` inside `dir`, making the version's folder.
#[cfg(not(target_arch = "wasm32"))]
fn write_into(dir: &str, rel: &str, bytes: &[u8]) -> Result<String> {
    let path = std::path::Path::new(dir).join(rel);
    if let Some(parent) = path.parent() {
        crate::io::create_dir_all(parent).map_err(EngineError::Other)?;
    }
    let s = path.to_string_lossy().into_owned();
    super::export::write_file(&s, bytes)?;
    Ok(s)
}
#[cfg(target_arch = "wasm32")]
fn write_into(_dir: &str, _rel: &str, _bytes: &[u8]) -> Result<String> {
    Err(EngineError::Other("no file system on the web; omit `dir` to get the files".into()))
}

fn export_batch(s: &mut Session, p: &Value) -> Result<Value> {
    let job = match (p.get("job"), str_param(p, "preset")) {
        (Some(j), _) => job_param(ID, j)?,
        (None, Some(name)) => {
            let preset = builtin_presets()
                .into_iter()
                .chain(s.prefs.export_presets.iter().cloned())
                .find(|x| x.name.eq_ignore_ascii_case(name.trim()))
                .ok_or_else(|| bad(ID, format!("no export preset `{name}`")))?;
            job_param(ID, &serde_json::to_value(&preset.job).map_err(|e| bad(ID, e.to_string()))?)?
        }
        (None, None) => return Err(bad(ID, "give a `job` or a `preset`")),
    };
    let date = match str_param(p, "date") {
        Some(d) => safe_name(d),
        None => today(),
    };
    let dir = str_param(p, "dir").map(str::to_string);
    let planned = plan(s, &job, &date)?;
    let dry = p.get("dryRun").and_then(Value::as_bool).unwrap_or(false);
    let mut files = Vec::with_capacity(planned.len());
    let mut warnings: Vec<String> = Vec::new();
    for f in &planned {
        let v = job.versions.get(f.version).ok_or_else(|| bad(ID, "no such version"))?;
        let mut entry = json!({
            "path": f.path,
            "version": v.name,
            "layout": f.layout,
            "pages": f.pages.iter().map(|p| p.saturating_add(1)).collect::<Vec<_>>(),
        });
        if !dry {
            let (bytes, w) = render(s, v, f)?;
            for w in w {
                let w = format!("{}: {w}", f.path);
                if !warnings.contains(&w) {
                    warnings.push(w);
                }
            }
            entry["bytes"] = json!(bytes.len());
            match &dir {
                Some(dir) => entry["path"] = json!(write_into(dir, &f.path, &bytes)?),
                None => entry["base64"] = json!(super::base64_encode(&bytes)),
            }
        }
        files.push(entry);
    }
    Ok(json!({"files": files, "warnings": warnings}))
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    fn two_layouts() -> Session {
        let mut s = Session::new();
        s.execute("file.new", &json!({"pages": 2, "facingPages": false, "width": 300, "height": 400, "bleed": 9, "title": "Mag"})).unwrap();
        s.execute("frame.create", &json!({"rect": [20, 20, 200, 200]})).unwrap();
        s.execute("layout.createAlternate", &json!({"name": "Tablet H", "width": 400, "height": 300, "fromName": "Print"})).unwrap();
        s
    }

    fn job(versions: &[&str]) -> Value {
        let t = version_templates();
        let v: Vec<_> = versions.iter().map(|n| t.iter().find(|x| x.name == *n).unwrap().clone()).collect();
        json!({"versions": v})
    }

    fn paths(r: &Value) -> Vec<String> {
        r["files"].as_array().unwrap().iter().map(|f| f["path"].as_str().unwrap().to_string()).collect()
    }

    #[test]
    fn a_file_per_layout_and_version_in_folders() {
        let mut s = two_layouts();
        let r = s.execute(ID, &json!({"job": job(&["PDF Print (marks + bleed)", "PNG"]), "dryRun": true})).unwrap();
        assert_eq!(
            paths(&r),
            [
                "PDF Print (marks + bleed)/Mag - Print.pdf",
                "PDF Print (marks + bleed)/Mag - Tablet H.pdf",
                "PNG/Mag - Print - 1.png",
                "PNG/Mag - Print - 2.png",
                "PNG/Mag - Tablet H - 1.png",
                "PNG/Mag - Tablet H - 2.png",
            ]
        );
        assert_eq!(r["files"][1]["pages"], json!([3, 4]));
        // One layout on its own, all layouts in one PDF, no folders, another name.
        let mut one = job(&["PDF Screen"]);
        one["layouts"] = json!(["Tablet H"]);
        assert_eq!(paths(&s.execute(ID, &json!({"job": one, "dryRun": true})).unwrap()), ["PDF Screen/Mag - Tablet H.pdf"]);
        let mut all = job(&["PDF Screen"]);
        all["combine"] = json!(true);
        all["folders"] = json!(false);
        all["fileName"] = json!("{document} {version} {date}");
        let r = s.execute(ID, &json!({"job": all, "dryRun": true, "date": "2026-10-20"})).unwrap();
        assert_eq!(paths(&r), ["Mag PDF Screen 2026-10-20.pdf"]);
        assert_eq!(r["files"][0]["pages"], json!([1, 2, 3, 4]));
    }

    #[test]
    fn the_export_renders_each_file() {
        let mut s = two_layouts();
        let r = s.execute(ID, &json!({"job": job(&["PDF Print (marks + bleed)", "PDF Bleed (no marks)", "JPEG"])})).unwrap();
        let files = r["files"].as_array().unwrap();
        assert_eq!(files.len(), 2 + 2 + 4);
        let bytes = |i: usize| super::super::file::base64_decode(files[i]["base64"].as_str().unwrap());
        for (i, f) in files.iter().enumerate().take(4) {
            assert!(bytes(i).starts_with(b"%PDF-"), "{}", f["path"]);
        }
        assert!(bytes(4).starts_with(&[0xFF, 0xD8]), "a JPEG");
        // Marks need room around the bleed: the marked PDF's page is larger.
        let size = |b: &[u8]| designcraft_render::pdf_page_size(b, 0).unwrap();
        let (marked, bled) = (size(&bytes(0)), size(&bytes(2)));
        assert!(marked.0 > bled.0 && marked.1 > bled.1, "{marked:?} {bled:?}");
        assert_eq!(designcraft_render::pdf_page_count(&bytes(1)), Some(2));
    }

    #[test]
    fn files_go_into_a_folder_per_version() {
        let dir = tempfile::tempdir().unwrap();
        let mut s = two_layouts();
        let r = s.execute(ID, &json!({"preset": "Print + Screen", "dir": dir.path().to_str().unwrap()})).unwrap();
        assert_eq!(r["files"].as_array().unwrap().len(), 2 + 2 + 4);
        for f in ["PDF Print (marks + bleed)/Mag - Print.pdf", "PDF Screen/Mag - Tablet H.pdf", "PNG/Mag - Tablet H - 2.png"] {
            assert!(dir.path().join(f).is_file(), "{f}");
        }
        let pdf = std::fs::read(dir.path().join("PDF Screen/Mag - Print.pdf")).unwrap();
        assert!(pdf.starts_with(b"%PDF-"));
    }

    #[test]
    fn names_are_made_safe_and_unique() {
        assert_eq!(safe_name("../a/b:c*?"), "a b c");
        assert_eq!(safe_name("  ...  "), "Untitled");
        assert_eq!(safe_name(&"x".repeat(500)).chars().count(), MAX_NAME);
        let mut s = two_layouts();
        let mut j = job(&["PNG", "PNG"]);
        j["fileName"] = json!("../{document}");
        let r = s.execute(ID, &json!({"job": j, "dryRun": true})).unwrap();
        let p = paths(&r);
        assert!(p.iter().all(|x| !x.contains("..")), "{p:?}");
        assert!(p.contains(&"PNG/Mag - 1.png".to_string()) && p.contains(&"PNG/Mag - 1 2.png".to_string()), "{p:?}");
        assert!(p.iter().any(|x| x.starts_with("PNG 2/")), "two versions, two folders: {p:?}");
    }

    #[test]
    fn bad_jobs_are_refused() {
        let mut s = two_layouts();
        for p in [
            json!({}),
            json!({"job": {"versions": []}}),
            json!({"job": {"versions": [{"format": "tiff"}]}}),
            json!({"job": {"versions": [{"standard": "x9"}]}}),
            json!({"job": {"versions": [{"format": "png"}], "layouts": ["Nope"]}}),
            json!({"job": "pdf"}),
            json!({"preset": "Nope"}),
        ] {
            assert!(s.execute(ID, &p).is_err(), "{p}");
        }
        // A huge resolution is capped, not a crash; past 80 million pixels an image is refused.
        let mut tall = Session::new();
        tall.execute("file.new", &json!({"pages": 1, "width": 3000, "height": 3000})).unwrap();
        assert!(tall.execute(ID, &json!({"job": {"versions": [{"format": "png", "ppi": 300}]}})).is_err(), "12500² pixels");
        let r = s.execute(ID, &json!({"job": {"versions": [{"format": "png", "ppi": 1e12}], "layouts": ["Print"]}, "dryRun": true})).unwrap();
        assert_eq!(r["files"].as_array().unwrap().len(), 2);
        let mut big = Session::new();
        big.execute("file.new", &json!({"pages": 1, "width": 10000, "height": 10000})).unwrap();
        assert!(big.execute(ID, &json!({"job": {"versions": [{"format": "png", "ppi": 1200}]}})).is_err(), "too many pixels");
    }

    #[test]
    fn presets_are_saved_listed_and_deleted() {
        let mut s = two_layouts();
        let builtin = s.execute("file.exportPresets", &json!({})).unwrap()["presets"].as_array().unwrap().len();
        s.execute("file.saveExportPreset", &json!({"name": "Client", "job": job(&["PNG"])})).unwrap();
        s.execute("file.saveExportPreset", &json!({"name": "client", "job": job(&["JPEG"])})).unwrap();
        let list = s.execute("file.exportPresets", &json!({})).unwrap();
        assert_eq!(list["presets"].as_array().unwrap().len(), builtin + 1, "same name replaces");
        let r = s.execute(ID, &json!({"preset": "CLIENT", "dryRun": true})).unwrap();
        assert!(paths(&r)[0].ends_with(".jpg"));
        assert!(s.execute("file.saveExportPreset", &json!({"name": "Print + Screen", "job": job(&["PNG"])})).is_err());
        assert!(s.execute("file.saveExportPreset", &json!({"name": "X", "job": {"versions": []}})).is_err());
        assert!(s.execute("file.deleteExportPreset", &json!({"name": "Print + Screen"})).is_err());
        s.execute("file.deleteExportPreset", &json!({"name": "Client"})).unwrap();
        assert!(s.execute(ID, &json!({"preset": "Client", "dryRun": true})).is_err());
    }
}
