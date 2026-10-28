//! File › Print: the pages go to a printer through the system print spooler (craft-print), as PDF.

use serde_json::{Value, json};

#[cfg(not(target_arch = "wasm32"))]
use super::str_param;
use super::{CommandSpec, bad, cmd, has_doc};
#[cfg(not(target_arch = "wasm32"))]
use crate::EngineError;
use crate::{Result, Session};

pub fn specs() -> Vec<CommandSpec> {
    vec![
        cmd!(query "file.printers", "Printers", [], None, "{} → {printers: [names], default}", super::always, |_, _| Ok(printers())),
        cmd!(
            noundo "file.print",
            "Print…",
            ["File"],
            Some("Cmd+P"),
            "{printer?, copies?: 1, pages?: \"1-3,5\", spreads?, marks?, bleed?, grayscale?: bool (send gray pages: every page rendered to Rec. 709 gray, and the driver told), dryRun?: bool (export and preview submission without printing), …the PDF settings of file.exportPdf (the PDF it writes is what prints)} → {printer, copies, pages, command, bytes, warnings}",
            has_doc,
            print
        ),
    ]
}

fn printers() -> Value {
    let printers = craft_print::printers();
    let default = printers.iter().find(|p| p.default).map(|p| p.name.clone());
    let names: Vec<_> = printers.into_iter().map(|p| p.name).collect();
    json!({"printers": names, "default": default})
}

fn print(s: &mut Session, p: &Value) -> Result<Value> {
    #[cfg(target_arch = "wasm32")]
    {
        super::export::options(p, s.doc()?.doc.page_count())?;
        Err(bad("file.print", "printing isn't available on the web: export a PDF and print it"))
    }
    #[cfg(not(target_arch = "wasm32"))]
    print_with(s, p, craft_print::submit)
}

/// The print-ready PDF of the pages a print would send (`pages`, `spreads`, `marks`, `bleed` as in
/// `file.print`), for a destination the host sends itself: a network printer's worker runs off the
/// session's thread, so it takes the bytes rather than the document.
#[cfg(not(target_arch = "wasm32"))]
#[derive(Clone, Debug)]
pub struct PrintDocument {
    pub pdf: Vec<u8>,
    /// How many sheets the PDF has (a spread is one).
    pub pages: usize,
    /// The document's title, for the job's name.
    pub title: String,
}

/// Export the active document as a print-ready PDF (see [`PrintDocument`]). Sends nothing.
#[cfg(not(target_arch = "wasm32"))]
pub fn print_document(s: &mut Session, p: &Value) -> Result<PrintDocument> {
    print_snapshot(s, p)?.render()
}

/// What a print needs of the session, taken cheaply: the document (shared, not copied), the render
/// cache (shared, internally locked) and the print settings. [`PrintSnapshot::render`] does the heavy
/// work (PDF export, grayscale rendering) on any thread, so a host can keep it off the UI thread.
#[cfg(not(target_arch = "wasm32"))]
#[derive(Clone)]
pub struct PrintSnapshot {
    doc: std::sync::Arc<designcraft_doc::Document>,
    cache: std::sync::Arc<designcraft_compose::Cache>,
    params: Value,
}

/// Snapshot the active document for [`PrintSnapshot::render`] (`p` as for [`print_document`]).
#[cfg(not(target_arch = "wasm32"))]
pub fn print_snapshot(s: &Session, p: &Value) -> Result<PrintSnapshot> {
    Ok(PrintSnapshot { doc: std::sync::Arc::clone(&s.doc()?.doc), cache: std::sync::Arc::clone(&s.cache), params: p.clone() })
}

#[cfg(not(target_arch = "wasm32"))]
impl PrintSnapshot {
    /// The print-ready PDF of the snapshot, exactly as [`print_document`] makes it.
    pub fn render(&self) -> Result<PrintDocument> {
        let (d, p) = (&*self.doc, &self.params);
        let mut pdf = super::export::pdf_export_of(d, &self.cache, p, true)?.bytes;
        if p.get("grayscale").and_then(Value::as_bool).unwrap_or(false) {
            pdf = grayscale_pdf(&pdf, Some(&d.title))?;
        }
        // The PDF's sheets: printed as spreads, two document pages share one.
        let pages = designcraft_render::pdf_page_count(&pdf).unwrap_or_else(|| p_pages(p, d.page_count()));
        Ok(PrintDocument { pdf, pages, title: d.title.clone() })
    }
}

/// The PDF that File › Export PDF writes for `p`, as a print-only export (no annotations: links,
/// buttons, form fields and media don't print, and the Windows spooler path refuses to fit a page
/// that carries any).
#[cfg(not(target_arch = "wasm32"))]
fn print_pdf(s: &Session, p: &Value) -> Result<designcraft_pdf::ExportReport> {
    super::export::pdf_export_for(s, p, true)
}

/// How many pages `p`'s page selection names (all of the document's `page_count` without one).
#[cfg(not(target_arch = "wasm32"))]
fn p_pages(p: &Value, page_count: usize) -> usize {
    super::export::options(p, page_count).ok().and_then(|o| o.pages.map(|v| v.len())).unwrap_or(page_count)
}

/// Resolution the gray pages are composed at.
#[cfg(not(target_arch = "wasm32"))]
const GRAY_DPI: f64 = 300.0;

/// The print-ready PDF with every page converted to Rec. 709 gray (rendered at 300 dpi over white
/// and stored as one gray picture per page), so a "Grayscale" print is gray whatever the driver does.
#[cfg(not(target_arch = "wasm32"))]
pub fn grayscale_pdf(pdf: &[u8], title: Option<&str>) -> Result<Vec<u8>> {
    let count =
        designcraft_render::pdf_page_count(pdf).filter(|n| *n > 0).ok_or_else(|| EngineError::Other("no pages to convert to grayscale".into()))?;
    // Pages are rendered as `gray_pdf` pulls them, so only one raster is alive at a time (a Letter
    // page at 300 dpi is ~8.4 MB). A page that fails is kept here, so its own message is the error.
    let mut failure = None;
    let pages = (0..count).map(|i| {
        gray_page(pdf, i).map_err(|e| {
            failure = Some(e);
            designcraft_pdf::PdfError::Write(String::new())
        })
    });
    designcraft_pdf::gray_pdf(pages, title).map_err(|e| failure.take().unwrap_or_else(|| EngineError::Other(e.to_string())))
}

/// Page `i` of `pdf` rendered over white and reduced to Rec. 709 gray.
#[cfg(not(target_arch = "wasm32"))]
fn gray_page(pdf: &[u8], i: usize) -> Result<designcraft_pdf::GrayPage> {
    let (w, h) = designcraft_render::pdf_page_size(pdf, i).ok_or_else(|| EngineError::Other(format!("page {} could not be read", i + 1)))?;
    let side = (w.max(h) * GRAY_DPI / 72.0).round().clamp(1.0, 8000.0) as u32;
    let pm = designcraft_render::render_pdf_page(pdf, i, side).ok_or_else(|| EngineError::Other(format!("page {} could not be rendered", i + 1)))?;
    let pixels: Vec<u8> = pm
        .data()
        .iter()
        .map(|p| {
            // Premultiplied RGBA over white.
            let over = |c: u8| (u16::from(c) + (255 - u16::from(p.a))).min(255) as u8;
            designcraft_pdf::rec709_luma(over(p.r), over(p.g), over(p.b))
        })
        .collect();
    Ok(designcraft_pdf::GrayPage { points: (w as f32, h as f32), size: (u32::from(pm.width()), u32::from(pm.height())), pixels })
}

#[cfg(not(target_arch = "wasm32"))]
fn print_with(
    s: &mut Session,
    p: &Value,
    submit: impl FnOnce(&[u8], &craft_print::Job) -> std::result::Result<String, craft_print::Error>,
) -> Result<Value> {
    let d = &s.doc()?.doc;
    let opts = super::export::options(p, d.page_count())?;
    let grayscale = p.get("grayscale").and_then(Value::as_bool).unwrap_or(false);
    let copies = p.get("copies").and_then(Value::as_u64).unwrap_or(1).clamp(1, 999) as u32;
    let job = craft_print::Job {
        printer: str_param(p, "printer").map(str::to_string),
        copies,
        // The queue's own default paper, the page shrunk to fit it: a document's own page size
        // (a slide, an artboard) is not a paper the queue has to list. No media is ever demanded.
        scaling: craft_print::Scaling::ShrinkToFit,
        grayscale,
        title: d.title.clone(),
        ..craft_print::Job::default()
    };
    let dry = p.get("dryRun").and_then(Value::as_bool).unwrap_or(false);
    // The PDF that Export PDF writes with these settings, minus the interactive layer.
    let mut report = print_pdf(s, p)?;
    if grayscale && !dry {
        report.bytes = grayscale_pdf(&report.bytes, Some(&d.title))?;
    }
    let (pdf, warnings) = (report.bytes, report.warnings);
    let command = craft_print::command_preview(&job).map_err(|e| bad("file.print", e.to_string()))?;
    let pages = opts.pages.as_ref().map_or(d.page_count(), |v| v.len());
    let out = json!({"printer": job.printer, "copies": copies, "pages": pages, "command": command, "bytes": pdf.len(), "warnings": warnings});
    if !dry {
        submit(&pdf, &job).map_err(|e| bad("file.print", e.to_string()))?;
    }
    Ok(out)
}

#[cfg(all(test, not(target_arch = "wasm32")))]
mod tests {
    use super::*;

    #[test]
    fn print_builds_the_spool_command() {
        let mut s = Session::new();
        s.execute("file.new", &json!({"pages": 4})).unwrap();
        let r = s.execute("file.print", &json!({"printer": "Office", "copies": 2, "pages": "2-3", "dryRun": true})).unwrap();
        assert_eq!(r["pages"], 2);
        let command: Vec<&str> = r["command"].as_array().unwrap().iter().filter_map(Value::as_str).collect();
        if craft_print::backend() == Some(craft_print::Backend::Cups) {
            assert_eq!(&command[..5], ["lp", "-d", "Office", "-n", "2"]);
            assert!(command.contains(&"print-scaling=auto-fit"));
            assert!(!command.iter().any(|a| a.starts_with("sides=")));
        } else {
            assert_eq!(
                command,
                [
                    "xps-print",
                    "printer=Office",
                    "copies=2",
                    "collate=true",
                    "duplex=(queue default)",
                    "scaling=shrink-to-fit",
                    "color=(driver default)"
                ]
            );
        }
        assert!(r["bytes"].as_u64().unwrap() > 500);
        assert!(s.execute("file.print", &json!({"pages": "9", "dryRun": true})).is_err());
        assert!(s.execute("file.printers", &json!({})).unwrap()["printers"].is_array());
    }

    #[test]
    fn print_submits_the_selected_export_and_reports_refusal() {
        let mut s = Session::new();
        s.execute("file.new", &json!({"pages": 4})).unwrap();
        // A fixed creation date: both exports would otherwise take the clock's second.
        let params = json!({"printer": "Office (east)", "copies": 2, "pages": "2-3", "marks": true, "bleed": true, "created": 1_700_000_000});
        let expected = super::super::export::pdf_export_for(&s, &params, true).unwrap().bytes;
        let r = print_with(&mut s, &params, |pdf, job| {
            // The production export options and full bytes reach the shared spooler boundary.
            assert_eq!(pdf, expected);
            assert!(pdf.starts_with(b"%PDF-"));
            assert_eq!(job.printer.as_deref(), Some("Office (east)"));
            assert_eq!(job.copies, 2);
            assert_eq!(job.duplex, None);
            assert_eq!(job.scaling, craft_print::Scaling::ShrinkToFit);
            assert!(!job.title.is_empty());
            Ok("request id is Office-1".into())
        })
        .unwrap();
        assert_eq!(r["pages"], 2);
        let refusal = print_with(&mut s, &params, |_, _| Err(craft_print::Error::Spool("printer offline".into()))).unwrap_err();
        assert!(refusal.to_string().contains("printer offline"));
        print_with(&mut s, &json!({"dryRun": true}), |_, _| panic!("dry runs must not spool")).unwrap();
        assert!(print_with(&mut s, &json!({"pages": "9"}), |_, _| panic!("invalid ranges must not spool")).is_err());
        assert!(print_with(&mut s, &json!({"created": "today"}), |_, _| panic!("an invalid date must not spool")).is_err());
    }

    /// C: a slide-sized document (16:9, 13.33 x 7.50 in) is printed on the queue's own default paper,
    /// the page shrunk to fit; no paper is demanded of the queue.
    #[test]
    fn a_custom_page_size_uses_the_default_paper_and_shrinks_to_fit() {
        let mut s = Session::new();
        s.execute("file.new", &json!({"width": 960.0, "height": 540.0, "pages": 1})).unwrap();
        let mut seen = None;
        print_with(&mut s, &json!({"printer": "Office"}), |_, job| {
            seen = Some(job.clone());
            Ok("ok".into())
        })
        .unwrap();
        let job = seen.unwrap();
        assert_eq!(job.scaling, craft_print::Scaling::ShrinkToFit);
        assert!(!job.grayscale);
        let r = s.execute("file.print", &json!({"dryRun": true})).unwrap();
        let command = format!("{:?}", r["command"]);
        assert!(!command.contains("media"), "no paper size is asked for: {command}");
        assert!(!command.contains("13.33"), "{command}");
    }

    /// A document with a text hyperlink, a URL button and a form field.
    #[allow(clippy::unwrap_used)]
    fn interactive_session() -> Session {
        let mut s = Session::new();
        s.execute("file.new", &json!({"pages": 2})).unwrap();
        let r = s.execute("frame.create", &json!({"rect": [72, 72, 400, 300], "content": "text", "text": "Visit us online"})).unwrap();
        s.execute("text.select", &json!({"story": r["story"], "anchor": 6, "focus": 8})).unwrap();
        s.execute("hyperlink.create", &json!({"url": "https://example.com"})).unwrap();
        let b = s.execute("frame.create", &json!({"rect": [72, 320, 200, 380]})).unwrap()["id"].clone();
        s.execute("selection.set", &json!({"ids": [b]})).unwrap();
        s.execute("button.set", &json!({"action": "url", "url": "https://example.com/buy"})).unwrap();
        let f = s.execute("frame.create", &json!({"rect": [72, 400, 300, 430]})).unwrap()["id"].clone();
        s.execute("selection.set", &json!({"ids": [f]})).unwrap();
        s.execute("form.set", &json!({"kind": "textField", "name": "who"})).unwrap();
        s
    }

    /// The craft-print Windows path refuses a page that carries annotations (shrink-to-fit cannot
    /// re-frame them; the print-visibility check wants flag 4), so what a print sends has none.
    #[test]
    fn a_document_with_links_buttons_and_fields_prints_with_no_annotations() {
        let mut s = interactive_session();
        let d = &s.doc().unwrap().doc;
        // The normal export keeps the interactive layer (this is what the print used to send).
        let kept = designcraft_pdf::export_pdf(d, &s.cache, &designcraft_pdf::PdfOptions::default()).unwrap();
        let kept = String::from_utf8_lossy(&kept).into_owned();
        assert!(kept.contains("/Annots") && kept.contains("/URI"), "export keeps links");
        assert!(kept.contains("/Widget"), "export keeps form fields");
        let mut sent = None;
        let r = print_with(&mut s, &json!({"printer": "Office"}), |pdf, job| {
            assert_eq!(job.scaling, craft_print::Scaling::ShrinkToFit);
            sent = Some(pdf.to_vec());
            Ok("ok".into())
        })
        .unwrap();
        assert_eq!(r["pages"], 2);
        let sent = sent.expect("the job was sent");
        let text = String::from_utf8_lossy(&sent);
        for bad in ["/Annots", "/Annot", "/URI", "/Link", "/Widget", "/AcroForm", "/Screen"] {
            assert!(!text.contains(bad), "the printed PDF has {bad}");
        }
        // The page art is still there.
        assert_eq!(designcraft_render::pdf_page_count(&sent), Some(2));
        let again = print_document(&mut s, &json!({"pages": "1"})).unwrap();
        assert!(!String::from_utf8_lossy(&again.pdf).contains("/Annots"));
    }

    /// B: a grayscale print sends a PDF whose pixels are all gray (Rec. 709) and tells the driver.
    #[test]
    fn grayscale_prints_gray_pages_and_sets_the_driver_hint() {
        let mut s = Session::new();
        s.execute("file.new", &json!({"pages": 1})).unwrap();
        let id = s.execute("frame.create", &json!({"rect": [50, 50, 350, 350]})).unwrap()["id"].clone();
        s.execute("selection.set", &json!({"ids": [id]})).unwrap();
        s.execute("object.color", &json!({"color": "#ff0000"})).unwrap();
        let coloured = print_document(&mut s, &json!({})).unwrap();
        let gray = print_document(&mut s, &json!({"grayscale": true})).unwrap();
        assert_eq!(gray.pages, coloured.pages);
        let pc = designcraft_render::render_pdf_page(&coloured.pdf, 0, 400).unwrap();
        assert!(pc.data().iter().any(|p| p.r != p.g || p.g != p.b), "the colour print has colour");
        let pm = designcraft_render::render_pdf_page(&gray.pdf, 0, 400).expect("gray page renders");
        assert!(pm.data().iter().all(|p| p.r == p.g && p.g == p.b), "every pixel is gray");
        assert_eq!(designcraft_render::pdf_page_size(&gray.pdf, 0), designcraft_render::pdf_page_size(&coloured.pdf, 0));
        let mut hint = false;
        print_with(&mut s, &json!({"grayscale": true}), |pdf, job| {
            hint = job.grayscale;
            assert!(pdf.starts_with(b"%PDF-"));
            Ok("ok".into())
        })
        .unwrap();
        assert!(hint);
    }

    /// A placed PDF whose Bounding Box crop is still unmeasured prints the same through the network
    /// path (`print_document`) as through the system queue (`print_with`): both measure the box.
    #[test]
    fn a_pending_content_box_is_measured_for_both_print_paths() {
        let mut s = Session::new();
        s.execute("file.new", &json!({})).unwrap();
        s.execute("frame.create", &json!({"rect": [100, 200, 300, 260], "content": "unassigned"})).unwrap();
        s.execute("object.fill", &json!({"swatch": "[Black]"})).unwrap();
        let b64 = s.execute("file.exportPdf", &json!({})).unwrap()["base64"].as_str().unwrap().to_string();
        let mut t = Session::new();
        t.execute("file.new", &json!({})).unwrap();
        t.execute("file.place", &json!({"base64": b64, "name": "a.pdf", "pdfCrop": "contentVisible", "width": 200, "x": 20, "y": 30})).unwrap();
        for a in std::sync::Arc::make_mut(&mut t.doc_mut().unwrap().doc).assets.values_mut() {
            std::sync::Arc::make_mut(a).pdf_box = None;
        }
        assert!(t.doc().unwrap().doc.assets.values().any(|a| a.content_box_pending()), "the box is pending");
        let mut sent = None;
        print_with(&mut t, &json!({}), |pdf, _| {
            sent = Some(pdf.to_vec());
            Ok("ok".into())
        })
        .unwrap();
        let network = print_document(&mut t, &json!({})).unwrap();
        assert_eq!(network.pdf, sent.expect("the job was sent"));
    }

    /// Printed as spreads, a facing-pages document has fewer sheets than pages: the count a network
    /// job expects is the PDF's sheets, not the document's pages.
    #[test]
    fn a_spreads_print_counts_its_sheets() {
        let mut s = Session::new();
        s.execute("file.new", &json!({"pages": 4, "facingPages": true})).unwrap();
        let doc = print_document(&mut s, &json!({"spreads": true})).unwrap();
        let sheets = designcraft_render::pdf_page_count(&doc.pdf).unwrap();
        assert!(sheets < 4, "spreads share sheets: {sheets}");
        assert_eq!(doc.pages, sheets);
        assert_eq!(print_document(&mut s, &json!({"pages": "2-3"})).unwrap().pages, 2);
    }

    #[test]
    fn rec709_weights() {
        assert_eq!(designcraft_pdf::rec709_luma(255, 255, 255), 255);
        assert_eq!(designcraft_pdf::rec709_luma(0, 0, 0), 0);
        assert_eq!(designcraft_pdf::rec709_luma(255, 0, 0), 54);
        assert_eq!(designcraft_pdf::rec709_luma(0, 255, 0), 182);
        assert_eq!(designcraft_pdf::rec709_luma(0, 0, 255), 18);
    }

    /// File › Print spools the PDF that File › Export PDF writes with the same settings: soft
    /// effects as images, CMYK print color, the same warnings.
    #[test]
    fn print_spools_what_export_pdf_writes() {
        let mut s = Session::new();
        s.execute("file.new", &json!({})).unwrap();
        let id = s.execute("frame.create", &json!({"rect": [72, 72, 200, 200]})).unwrap()["id"].clone();
        s.execute("object.fill", &json!({"ids": [id], "swatch": "C=100 M=0 Y=0 K=0"})).unwrap();
        s.execute("object.dropShadow", &json!({"ids": [id], "on": true, "distance": 8, "size": 6})).unwrap();
        let params = json!({"standard": "x4"});
        // The creation date is in whole seconds: an attempt can straddle a second boundary.
        let mut matched = false;
        for _ in 0..3 {
            let exported = s.execute("file.exportPdf", &params).unwrap();
            let mut printed = Vec::new();
            let r = print_with(&mut s, &params, |pdf, _| {
                printed = pdf.to_vec();
                Ok("request id is Office-1".into())
            })
            .unwrap();
            assert_eq!(r["warnings"], exported["warnings"]);
            assert!(r["warnings"].as_array().unwrap().iter().any(|w| w.as_str().unwrap().contains("soft effects")), "{r}");
            if printed == super::super::file::base64_decode(exported["base64"].as_str().unwrap()) {
                matched = true;
                break;
            }
        }
        assert!(matched, "File › Print spools Export PDF's bytes");
    }
}
