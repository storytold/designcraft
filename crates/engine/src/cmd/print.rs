//! File › Print: the pages go to a printer through the system print spooler (craft-print), as PDF.

use serde_json::{Value, json};

#[cfg(not(target_arch = "wasm32"))]
use super::str_param;
use super::{CommandSpec, bad, cmd, has_doc};
use crate::{Result, Session};

pub fn specs() -> Vec<CommandSpec> {
    vec![
        cmd!(query "file.printers", "Printers", [], None, "{} → {printers: [names], default}", super::always, |_, _| Ok(printers())),
        cmd!(
            noundo "file.print",
            "Print…",
            ["File"],
            Some("Cmd+P"),
            "{printer?, copies?: 1, pages?: \"1-3,5\", layouts?: [layout names] (their pages; instead of `pages`), separateJobs?: bool (a print job per layout, titled after it), dryRun?: bool (export and preview submission without printing), …the PDF settings of file.exportPdf (the PDF it writes is what prints)} → {printer, copies, pages, command, bytes, warnings, jobs}",
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

#[cfg(not(target_arch = "wasm32"))]
impl Session {
    /// Check the active document's print ranges, layouts and PDF option values without
    /// composing, exporting, reading files or contacting a printer. Desktop dialogs use this
    /// before queuing background work, so a correctable option error stays in the dialog.
    pub fn check_print_options(&self, p: &Value) -> Result<()> {
        print_jobs(&self.doc()?.doc, p).map(|_| ())
    }
}

/// Resolve the jobs and validate their option values using the same path for dialog admission
/// and actual printing. Preparing PDF bytes and asking the platform spooler happen later.
#[cfg(not(target_arch = "wasm32"))]
fn print_jobs(d: &designcraft_doc::Document, p: &Value) -> Result<Vec<(String, Option<Vec<usize>>)>> {
    super::export::check_pdf_options(p, d.page_count())?;
    // Layouts: their pages, in one job or a job each (`None`: the pages `p` names).
    match p.get("layouts") {
        None | Some(Value::Null) => Ok(vec![(d.title.clone(), None)]),
        Some(v) => {
            let names: Vec<String> = serde_json::from_value(v.clone()).map_err(|_| bad("file.print", "`layouts` is a list of layout names"))?;
            let groups = super::export_batch::layout_pages("file.print", d, &names)?;
            Ok(if p.get("separateJobs").and_then(Value::as_bool).unwrap_or(false) {
                groups.into_iter().map(|(name, pages)| (format!("{} - {name}", d.title), Some(pages))).collect()
            } else {
                vec![(d.title.clone(), Some(groups.into_iter().flat_map(|(_, p)| p).collect()))]
            })
        }
    }
}

/// Validate and prepare every PDF before submitting any job, preserving the all-or-nothing
/// preparation boundary. Dry runs build the same PDFs but never call the spooler callback.
#[cfg(not(target_arch = "wasm32"))]
fn print_with(
    s: &mut Session,
    p: &Value,
    mut submit: impl FnMut(&[u8], &craft_print::Job) -> std::result::Result<String, craft_print::Error>,
) -> Result<Value> {
    let d = &s.doc()?.doc;
    let jobs = print_jobs(d, p)?;
    let opts = super::export::options(p, d.page_count())?;
    let copies = p.get("copies").and_then(Value::as_u64).unwrap_or(1).clamp(1, 999) as u32;
    let dry = p.get("dryRun").and_then(Value::as_bool).unwrap_or(false);
    let mut out = Value::Null;
    let (mut total_pages, mut total_bytes) = (0usize, 0usize);
    let (mut titles, mut warnings) = (Vec::new(), Vec::<String>::new());
    // Every job's PDF first, so a failing one sends nothing.
    let mut ready = Vec::new();
    for (title, pages) in jobs {
        let job = craft_print::Job {
            printer: str_param(p, "printer").map(str::to_string),
            copies,
            scaling: craft_print::Scaling::ShrinkToFit,
            title: title.clone(),
            ..craft_print::Job::default()
        };
        // The PDF that Export PDF writes with these settings, on the job's pages.
        let mut params = p.clone();
        if let (Some(pages), Some(o)) = (&pages, params.as_object_mut()) {
            o.insert("pages".into(), json!(pages.iter().map(|p| p.saturating_add(1)).collect::<Vec<_>>()));
        }
        let pdf = super::export::pdf_export(s, &params)?;
        let command = craft_print::command_preview(&job).map_err(|e| bad("file.print", e.to_string()))?;
        total_pages += pages.as_ref().or(opts.pages.as_ref()).map_or(d.page_count(), Vec::len);
        total_bytes += pdf.bytes.len();
        for w in pdf.warnings {
            if !warnings.contains(&w) {
                warnings.push(w);
            }
        }
        out = json!({"printer": job.printer, "copies": copies, "command": command});
        titles.push(title);
        ready.push((pdf.bytes, job));
    }
    if !dry {
        for (pdf, job) in &ready {
            submit(pdf, job).map_err(|e| bad("file.print", e.to_string()))?;
        }
    }
    out["pages"] = json!(total_pages);
    out["bytes"] = json!(total_bytes);
    out["warnings"] = json!(warnings);
    out["jobs"] = json!(titles);
    Ok(out)
}

#[cfg(all(test, not(target_arch = "wasm32")))]
mod tests {
    use super::*;

    /// Dialog admission leaves the session untouched and returns the same option errors as
    /// submission, whose fake spooler must never be reached for invalid input.
    #[test]
    fn print_option_checks_share_the_submission_validation_without_changing_the_session() {
        let mut s = Session::new();
        s.execute("file.new", &json!({"pages":2})).unwrap();
        let doc = s.doc().unwrap().doc.clone();
        let cache = s.cache.clone();
        let journal = s.journal.len();
        s.check_print_options(&json!({"pages":"1", "layouts":[], "flatten":"high", "standard":"x4", "view":"fitPage", "created":0})).unwrap();
        assert!(std::sync::Arc::ptr_eq(&doc, &s.doc().unwrap().doc));
        assert!(std::sync::Arc::ptr_eq(&cache, &s.cache));
        assert_eq!(s.journal.len(), journal);
        for params in [
            json!({"pages":"9"}),
            json!({"layouts":["Missing layout"]}),
            json!({"layouts":"Layout 1"}),
            json!({"standard":"unknown"}),
            json!({"flatten":"unknown"}),
            json!({"created":"today"}),
            json!({"view":"unknown"}),
            json!({"pageLayout":"unknown"}),
        ] {
            let checked = s.check_print_options(&params).unwrap_err().to_string();
            let submitted = print_with(&mut s, &params, |_, _| panic!("invalid options must not submit a job")).unwrap_err().to_string();
            assert_eq!(checked, submitted, "{params}");
        }
    }

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
        let expected = super::super::export::pdf_export(&s, &params).unwrap().bytes;
        let r = print_with(&mut s, &params, |pdf, job| {
            // The production export options and full bytes reach the shared spooler boundary.
            assert_eq!(pdf, expected);
            assert!(pdf.starts_with(b"%PDF-"));
            assert_eq!(job.printer.as_deref(), Some("Office (east)"));
            assert_eq!(job.copies, 2);
            assert_eq!(job.duplex, None);
            assert!(!job.grayscale);
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

    #[test]
    fn print_layouts_together_or_as_a_job_each() {
        let mut s = Session::new();
        s.execute("file.new", &json!({"pages": 2, "facingPages": false, "title": "Mag"})).unwrap();
        s.execute("layout.createAlternate", &json!({"name": "Wide", "width": 900, "height": 600})).unwrap();
        let mut sent = Vec::new();
        let r = print_with(&mut s, &json!({"layouts": ["Wide"]}), |pdf, job| {
            sent.push((designcraft_render::pdf_page_count(pdf), job.title.clone()));
            Ok(String::new())
        })
        .unwrap();
        assert_eq!(r["pages"], 2);
        assert_eq!(sent, [(Some(2), "Mag".to_string())]);
        sent.clear();
        let r = print_with(&mut s, &json!({"layouts": [], "separateJobs": true}), |pdf, job| {
            sent.push((designcraft_render::pdf_page_count(pdf), job.title.clone()));
            Ok(String::new())
        })
        .unwrap();
        assert_eq!(r["jobs"], json!(["Mag - Layout 1", "Mag - Wide"]));
        assert_eq!(sent, [(Some(2), "Mag - Layout 1".to_string()), (Some(2), "Mag - Wide".to_string())]);
        assert!(print_with(&mut s, &json!({"layouts": ["Nope"]}), |_, _| panic!("unknown layouts must not spool")).is_err());
        assert!(print_with(&mut s, &json!({"layouts": "Wide"}), |_, _| panic!("bad layouts must not spool")).is_err());
    }
}
