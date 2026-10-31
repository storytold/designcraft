//! One render job: open a template, tag and fill it from XML, export it. Every file the engine
//! reads or writes during the job is confined to the job's own folder, the templates folder and
//! the asset folders, so an `href` in the XML can't reach other files on the server.

use std::collections::BTreeMap;
use std::path::{Component, Path, PathBuf};

use base64::Engine as _;
use designcraft_engine::Session;
use designcraft_engine::file_access::{self, AutomationRoots};
use serde::Deserialize;
use serde_json::{Value, json};

/// Where templates and server-side images live.
#[derive(Clone, Debug, Default)]
pub struct Folders {
    /// Templates named by `template` in a request; also readable by the job.
    pub templates: Option<PathBuf>,
    /// Further folders the job may read images from (absolute `href`s).
    pub assets: Vec<PathBuf>,
}

/// The body of `POST /render`.
#[derive(Debug, Default, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Request {
    /// A template in the templates folder, by its path relative to it.
    pub template: Option<String>,
    /// Or the template's bytes.
    pub template_base64: Option<String>,
    /// `designcraft` or `idml`; by default from `template`'s extension, else `designcraft`.
    pub template_type: Option<String>,
    /// The content: elements fill the frames with their tag (`href` elements place images).
    pub xml: Option<String>,
    /// Images the XML names by relative `href`: path → base64 bytes.
    #[serde(default)]
    pub images: BTreeMap<String, String>,
    /// Run `xml.autoTag` before the import.
    #[serde(default)]
    pub auto_tag: bool,
    /// `pdf` (default), `png`, `jpg`, `designcraft` or `idml`.
    pub format: Option<String>,
    /// The page for `png` and `jpg`, from 1 (default 1).
    pub page: Option<usize>,
    /// The scale for `png` and `jpg` (default 1: 72 ppi).
    pub scale: Option<f64>,
    /// `file.exportPdf` options (`preset`, `standard`, …).
    pub pdf: Option<Value>,
    /// Fail with 422 when an element found no frame or an image couldn't be placed.
    #[serde(default)]
    pub strict: bool,
}

/// A finished job.
#[derive(Debug)]
pub struct Output {
    pub bytes: Vec<u8>,
    pub content_type: &'static str,
    pub extension: &'static str,
    /// What `xml.autoTag` and `file.importXml` reported.
    pub report: Value,
}

#[derive(Debug)]
pub enum Error {
    /// The request is wrong (400).
    BadRequest(String),
    /// The template or the XML couldn't be used, or `strict` failed (422).
    Unprocessable { message: String, report: Value },
    /// The server failed (500).
    Internal(String),
}

impl Error {
    pub fn status(&self) -> u16 {
        match self {
            Error::BadRequest(_) => 400,
            Error::Unprocessable { .. } => 422,
            Error::Internal(_) => 500,
        }
    }

    pub fn body(&self) -> Value {
        match self {
            Error::BadRequest(m) | Error::Internal(m) => json!({"error": m}),
            Error::Unprocessable { message, report } => json!({"error": message, "report": report}),
        }
    }
}

/// A path relative to a folder that stays inside it: no root, drive, `..` or `.` parts.
pub fn relative(path: &str) -> Option<PathBuf> {
    let p = Path::new(path);
    let ok = !path.is_empty() && p.components().all(|c| matches!(c, Component::Normal(_)));
    ok.then(|| p.to_path_buf())
}

fn decode(what: &str, b64: &str) -> Result<Vec<u8>, Error> {
    base64::engine::general_purpose::STANDARD.decode(b64.trim()).map_err(|e| Error::BadRequest(format!("{what} isn't valid base64: {e}")))
}

/// What a format exports as: (extension, content type).
fn format_of(name: &str) -> Result<(&'static str, &'static str), Error> {
    Ok(match name.to_lowercase().as_str() {
        "pdf" => ("pdf", "application/pdf"),
        "png" => ("png", "image/png"),
        "jpg" | "jpeg" => ("jpg", "image/jpeg"),
        "designcraft" => ("designcraft", "application/vnd.designcraft+zip"),
        "idml" => ("idml", "application/vnd.adobe.indesign-idml-package"),
        other => return Err(Error::BadRequest(format!("unknown format `{other}` (pdf, png, jpg, designcraft, idml)"))),
    })
}

/// The template's path: in the job folder when uploaded, else in the templates folder.
fn template_path(req: &Request, folders: &Folders, job: &Path) -> Result<PathBuf, Error> {
    let kind = match (&req.template_type, &req.template) {
        (Some(t), _) => t.to_lowercase(),
        (None, Some(name)) if name.to_lowercase().ends_with(".idml") => "idml".into(),
        _ => "designcraft".into(),
    };
    if !matches!(kind.as_str(), "designcraft" | "idml") {
        return Err(Error::BadRequest(format!("unknown templateType `{kind}` (designcraft, idml)")));
    }
    match (&req.template, &req.template_base64) {
        (Some(_), Some(_)) => Err(Error::BadRequest("give `template` or `templateBase64`, not both".into())),
        (None, None) => Err(Error::BadRequest("`template` or `templateBase64` required".into())),
        (None, Some(b64)) => {
            let path = job.join(format!("template.{kind}"));
            std::fs::write(&path, decode("templateBase64", b64)?).map_err(|e| Error::Internal(e.to_string()))?;
            Ok(path)
        }
        (Some(name), None) => {
            let dir = folders.templates.as_ref().ok_or_else(|| Error::BadRequest("the server has no templates folder (--templates)".into()))?;
            let rel = relative(name).ok_or_else(|| Error::BadRequest(format!("`{name}` isn't a path inside the templates folder")))?;
            let path = dir.join(rel);
            if !path.is_file() {
                return Err(Error::BadRequest(format!("no template `{name}`")));
            }
            Ok(path)
        }
    }
}

/// Runs a job in a fresh folder that is removed afterwards.
pub fn render(req: &Request, folders: &Folders) -> Result<Output, Error> {
    let (extension, content_type) = format_of(req.format.as_deref().unwrap_or("pdf"))?;
    let job = tempfile::Builder::new().prefix("designcraft-job-").tempdir().map_err(|e| Error::Internal(e.to_string()))?;
    let dir = job.path().to_path_buf();
    for (name, b64) in &req.images {
        let rel = relative(name).ok_or_else(|| Error::BadRequest(format!("image name `{name}` must be a relative path without `..`")))?;
        let path = dir.join(rel);
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).map_err(|e| Error::Internal(e.to_string()))?;
        }
        std::fs::write(&path, decode(&format!("image `{name}`"), b64)?).map_err(|e| Error::Internal(e.to_string()))?;
    }
    let template = template_path(req, folders, &dir)?;
    let out = dir.join(format!("out.{extension}"));
    let mut read: Vec<&Path> = vec![&dir];
    read.extend(folders.templates.as_deref());
    read.extend(folders.assets.iter().map(PathBuf::as_path));
    let roots = AutomationRoots::new(&read, &[&dir]).map_err(Error::Internal)?;
    let report = file_access::confine(roots.as_ref(), || run(req, &template, &dir, &out, extension))?;
    let bytes = std::fs::read(&out).map_err(|e| Error::Internal(format!("the export wrote nothing: {e}")))?;
    Ok(Output { bytes, content_type, extension, report })
}

fn unprocessable(what: &str, e: impl std::fmt::Display, report: &Value) -> Error {
    Error::Unprocessable { message: format!("{what}: {e}"), report: report.clone() }
}

fn run(req: &Request, template: &Path, dir: &Path, out: &Path, extension: &str) -> Result<Value, Error> {
    let mut s = Session::new();
    let mut report = json!({});
    let open = if template.extension().is_some_and(|e| e.eq_ignore_ascii_case("idml")) { "file.openIdml" } else { "file.open" };
    s.execute(open, &json!({"path": template})).map_err(|e| unprocessable("can't open the template", e, &report))?;
    if req.auto_tag {
        report["autoTag"] = s.execute("xml.autoTag", &json!({})).map_err(|e| unprocessable("xml.autoTag", e, &report))?;
    }
    if let Some(xml) = &req.xml {
        let r = s.execute("file.importXml", &json!({"text": xml, "imageDir": dir})).map_err(|e| unprocessable("file.importXml", e, &report))?;
        report["import"] = r;
        let unmatched = report["import"]["unmatched"].as_u64().unwrap_or(0);
        let failed = report["import"]["failed"].as_array().map_or(0, Vec::len);
        if req.strict && (unmatched > 0 || failed > 0) {
            return Err(Error::Unprocessable {
                message: format!("strict: {unmatched} element(s) found no frame, {failed} image(s) couldn't be placed"),
                report,
            });
        }
    }
    match extension {
        "pdf" => {
            let mut p = req.pdf.clone().filter(Value::is_object).unwrap_or_else(|| json!({}));
            p["path"] = json!(out);
            let r = s.execute("file.exportPdf", &p).map_err(|e| unprocessable("file.exportPdf", e, &report))?;
            report["pdf"] = json!({"pages": r["pages"], "warnings": r["warnings"]});
        }
        "png" | "jpg" => {
            let page = req.page.unwrap_or(1);
            let index = page.checked_sub(1).ok_or_else(|| Error::BadRequest("`page` counts from 1".into()))?;
            let scale = req.scale.unwrap_or(1.0);
            if !(scale.is_finite() && scale > 0.0 && scale <= 16.0) {
                return Err(Error::BadRequest("`scale` must be greater than 0 and at most 16".into()));
            }
            let st = s.doc().map_err(|e| Error::Internal(e.to_string()))?;
            let mut r = designcraft_render::Renderer::new();
            let options = designcraft_render::RenderOptions { printing_only: true, ..Default::default() };
            let img = r.render_page(&st.doc, &s.cache, index, scale, true, &options).ok_or_else(|| Error::BadRequest(format!("no page {page}")))?;
            let bytes = if extension == "jpg" { img.to_jpeg(90) } else { img.to_png() };
            std::fs::write(out, bytes).map_err(|e| Error::Internal(e.to_string()))?;
        }
        "designcraft" => {
            s.execute("file.saveACopy", &json!({"path": out})).map_err(|e| unprocessable("file.saveACopy", e, &report))?;
        }
        _ => {
            s.execute("file.exportIdml", &json!({"path": out})).map_err(|e| unprocessable("file.exportIdml", e, &report))?;
        }
    }
    Ok(report)
}

/// The tags and tagged frames of a template in the templates folder.
pub fn structure(name: &str, folders: &Folders) -> Result<Value, Error> {
    let req = Request { template: Some(name.to_string()), ..Default::default() };
    let job = tempfile::Builder::new().prefix("designcraft-job-").tempdir().map_err(|e| Error::Internal(e.to_string()))?;
    let path = template_path(&req, folders, job.path())?;
    let read: Vec<&Path> = folders.templates.as_deref().into_iter().collect();
    let roots = AutomationRoots::new(&read, &[]).map_err(Error::Internal)?;
    file_access::confine(roots.as_ref(), || {
        let mut s = Session::new();
        let open = if path.extension().is_some_and(|e| e.eq_ignore_ascii_case("idml")) { "file.openIdml" } else { "file.open" };
        s.execute(open, &json!({"path": path})).map_err(|e| unprocessable("can't open the template", e, &json!({})))?;
        let structure = s.execute("xml.structure", &json!({})).map_err(|e| Error::Internal(e.to_string()))?;
        let tags = s.execute("xml.tags", &json!({})).map_err(|e| Error::Internal(e.to_string()))?;
        Ok(json!({"template": name, "root": structure["root"], "elements": structure["elements"], "tags": tags["tags"]}))
    })
}

/// Templates in the templates folder (`.designcraft` and `.idml`), as paths relative to it.
pub fn templates(folders: &Folders) -> Result<Vec<String>, Error> {
    let Some(dir) = &folders.templates else { return Ok(Vec::new()) };
    let mut out = Vec::new();
    let mut stack = vec![dir.clone()];
    while let Some(d) = stack.pop() {
        for entry in std::fs::read_dir(&d).map_err(|e| Error::Internal(format!("{}: {e}", d.display())))?.flatten() {
            let p = entry.path();
            if p.is_dir() {
                stack.push(p);
            } else if p.extension().is_some_and(|e| e.eq_ignore_ascii_case("designcraft") || e.eq_ignore_ascii_case("idml"))
                && let Ok(rel) = p.strip_prefix(dir)
            {
                out.push(rel.to_string_lossy().replace('\\', "/"));
            }
        }
    }
    out.sort();
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A template with a tagged headline, a tagged empty graphic frame and an untagged text frame.
    fn template(dir: &Path) -> PathBuf {
        let mut s = Session::new();
        s.execute("file.new", &json!({})).unwrap();
        let head = s.execute("frame.create", &json!({"rect": [72, 72, 400, 120], "content": "text", "text": "Title", "caret": false})).unwrap();
        let photo = s.execute("frame.create", &json!({"rect": [72, 200, 300, 400], "content": "graphic"})).unwrap();
        let body = s.execute("frame.create", &json!({"rect": [72, 450, 400, 600], "content": "text", "text": "Body", "caret": false})).unwrap();
        s.execute("xml.tag", &json!({"tag": "headline", "ids": [head["id"]]})).unwrap();
        s.execute("xml.tag", &json!({"tag": "photo", "ids": [photo["id"]]})).unwrap();
        s.execute("object.rename", &json!({"id": body["id"], "name": "body"})).unwrap();
        let path = dir.join("issue.designcraft");
        s.execute("file.saveAs", &json!({"path": path})).unwrap();
        path
    }

    fn png_b64() -> String {
        let png = designcraft_render::Rendered { width: 40, height: 20, pixels: vec![200u8; 40 * 20 * 4] }.to_png();
        base64::engine::general_purpose::STANDARD.encode(png)
    }

    fn folders() -> (tempfile::TempDir, Folders) {
        let dir = tempfile::tempdir().unwrap();
        template(dir.path());
        let f = Folders { templates: Some(dir.path().to_path_buf()), assets: vec![] };
        (dir, f)
    }

    #[test]
    fn renders_text_and_images_to_pdf_and_designcraft() {
        let (_dir, f) = folders();
        let req = Request {
            template: Some("issue.designcraft".into()),
            xml: Some("<Root><headline>Spring</headline><photo href=\"img/a.png\"/><body>Hello</body></Root>".into()),
            images: BTreeMap::from([("img/a.png".to_string(), png_b64())]),
            auto_tag: true,
            ..Default::default()
        };
        let out = render(&req, &f).unwrap();
        assert!(out.bytes.starts_with(b"%PDF"), "a PDF");
        assert_eq!(out.report["autoTag"]["tagged"][0]["tag"], "body", "{}", out.report);
        assert_eq!((out.report["import"]["placed"].as_u64(), out.report["import"]["unmatched"].as_u64()), (Some(3), Some(0)), "{}", out.report);

        // The saved result holds the new text and the image.
        let out = render(&Request { format: Some("designcraft".into()), ..req }, &f).unwrap();
        let job = tempfile::tempdir().unwrap();
        let saved = job.path().join("r.designcraft");
        std::fs::write(&saved, &out.bytes).unwrap();
        let mut s = Session::new();
        s.execute("file.open", &json!({"path": saved})).unwrap();
        let st = s.execute("xml.structure", &json!({})).unwrap();
        let text: Vec<&str> = st["elements"].as_array().unwrap().iter().filter_map(|e| e["text"].as_str()).collect();
        assert!(text.contains(&"Spring") && text.contains(&"Hello"), "{st}");
    }

    #[test]
    fn png_of_an_uploaded_template() {
        let dir = tempfile::tempdir().unwrap();
        let bytes = std::fs::read(template(dir.path())).unwrap();
        let req = Request {
            template_base64: Some(base64::engine::general_purpose::STANDARD.encode(bytes)),
            xml: Some("<Root><headline>Uploaded</headline></Root>".into()),
            format: Some("png".into()),
            ..Default::default()
        };
        let out = render(&req, &Folders::default()).unwrap();
        assert!(out.bytes.starts_with(b"\x89PNG") && out.content_type == "image/png");
        assert!(matches!(render(&Request { page: Some(9), ..req }, &Folders::default()), Err(Error::BadRequest(_))));
    }

    #[test]
    fn hrefs_outside_the_job_and_strict_mode() {
        let (_dir, f) = folders();
        let secret = tempfile::tempdir().unwrap();
        let png = base64::engine::general_purpose::STANDARD.decode(png_b64()).unwrap();
        std::fs::write(secret.path().join("s.png"), png).unwrap();
        let outside = format!("file:///{}", secret.path().join("s.png").to_string_lossy().replace('\\', "/"));
        let req = Request {
            template: Some("issue.designcraft".into()),
            xml: Some(format!("<Root><photo href=\"{outside}\"/><nothing>x</nothing></Root>")),
            ..Default::default()
        };
        // A file outside the roots isn't read; the rest still renders.
        let out = render(&req, &f).unwrap();
        let failed = &out.report["import"]["failed"][0];
        assert!(failed["error"].as_str().unwrap().contains("outside the read roots"), "{failed}");
        assert_eq!(out.report["import"]["unmatched"], 1);
        // Allowed as an asset folder, it is placed.
        let with_assets = Folders { assets: vec![secret.path().to_path_buf()], ..f.clone() };
        assert_eq!(render(&req, &with_assets).unwrap().report["import"]["failed"].as_array().unwrap().len(), 0);
        let strict = Request { strict: true, ..req };
        assert!(matches!(render(&strict, &f), Err(Error::Unprocessable { .. })));
    }

    #[test]
    fn bad_requests() {
        let (_dir, f) = folders();
        let r = |req: Request| render(&req, &f).map(|_| ()).unwrap_err().status();
        assert_eq!(r(Request::default()), 400, "no template");
        assert_eq!(r(Request { template: Some("../issue.designcraft".into()), ..Default::default() }), 400);
        assert_eq!(r(Request { template: Some("missing.designcraft".into()), ..Default::default() }), 400);
        assert_eq!(r(Request { template: Some("issue.designcraft".into()), format: Some("gif".into()), ..Default::default() }), 400);
        let bad_image =
            Request { template: Some("issue.designcraft".into()), images: BTreeMap::from([("../x.png".into(), png_b64())]), ..Default::default() };
        assert_eq!(r(bad_image), 400);
        assert_eq!(r(Request { template: Some("issue.designcraft".into()), xml: Some("<Root><a></Root>".into()), ..Default::default() }), 422);
    }

    #[test]
    fn lists_templates_and_their_structure() {
        let (_dir, f) = folders();
        assert_eq!(templates(&f).unwrap(), vec!["issue.designcraft".to_string()]);
        let st = structure("issue.designcraft", &f).unwrap();
        let tags: Vec<&str> = st["elements"].as_array().unwrap().iter().filter_map(|e| e["tag"].as_str()).collect();
        assert_eq!(tags, vec!["headline", "photo"]);
    }
}
