use serde_json::json;

use crate::Session;

fn session_with_pages(pages: u32) -> Session {
    let mut s = Session::new();
    s.execute("file.new", &json!({"pages": pages, "facingPages": false})).unwrap();
    s
}

/// Poll `file.export.progress` until the job is no longer busy, as the interface does each frame.
fn wait(s: &mut Session, job: u64) -> serde_json::Value {
    for _ in 0..2000 {
        let v = s.execute("file.export.progress", &json!({"job": job})).unwrap();
        if v["busy"] == false {
            return v;
        }
        std::thread::sleep(std::time::Duration::from_millis(5));
    }
    panic!("the background export did not finish");
}

#[cfg(not(target_arch = "wasm32"))]
#[test]
fn a_background_pdf_export_writes_the_file_and_reports_what_it_did() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("out.pdf").to_string_lossy().to_string();
    let mut s = session_with_pages(3);
    let queued = s.execute("file.exportPdf", &json!({"background": true, "path": path, "created": 0})).unwrap();
    let job = queued["job"].as_u64().expect("the queued job id");
    let done = wait(&mut s, job);
    assert_eq!(done["busy"], false, "{done}");
    assert_eq!(done["path"], path, "{done}");
    assert_eq!(done["pages"], 3, "{done}");
    assert!(done["bytes"].as_u64().unwrap_or(0) > 0, "{done}");
    let written = std::fs::read(&path).unwrap();
    assert!(written.starts_with(b"%PDF-"), "the job wrote a PDF");
    assert_eq!(written.len() as u64, done["bytes"].as_u64().unwrap(), "{done}");
}

#[test]
fn a_background_export_without_a_path_is_refused() {
    let mut s = session_with_pages(1);
    let e = s.execute("file.exportPdf", &json!({"background": true})).unwrap_err().to_string();
    assert!(e.contains("`background` needs a `path`"), "{e}");
}

#[cfg(not(target_arch = "wasm32"))]
#[test]
fn export_progress_reports_the_latest_job_and_refuses_an_unknown_one() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("out.pdf").to_string_lossy().to_string();
    let mut s = session_with_pages(1);
    let e = s.execute("file.export.progress", &json!({})).unwrap_err().to_string();
    assert!(e.contains("no background export"), "{e}");
    let queued = s.execute("file.exportPdf", &json!({"background": true, "path": path})).unwrap();
    let job = queued["job"].as_u64().unwrap();
    // Omitted `job` reports the latest one.
    let latest = s.execute("file.export.progress", &json!({})).unwrap();
    assert_eq!(latest["job"], job, "{latest}");
    let e = s.execute("file.export.progress", &json!({"job": job + 1000})).unwrap_err().to_string();
    assert!(e.contains("no such background export"), "{e}");
}

#[test]
fn a_pdf_export_reports_each_page_in_order() {
    let s = session_with_pages(3);
    let doc = s.doc().unwrap().doc.clone();
    let cache = s.cache.clone();
    let mut seen = Vec::new();
    crate::cmd::pdf_export_report_with_progress(&doc, &cache, &json!({}), &mut |p| seen.push(p), &|| false).unwrap();
    let pages: Vec<_> = seen.iter().filter(|p| p.phase == "pages").collect();
    assert!(!pages.is_empty(), "page progress was reported: {seen:?}");
    let last = pages.last().unwrap();
    assert_eq!(last.total, 3, "{seen:?}");
    assert_eq!(last.done, 3, "{seen:?}");
    let dones: Vec<usize> = pages.iter().map(|p| p.done).collect();
    let mut sorted = dones.clone();
    sorted.sort_unstable();
    assert_eq!(dones, sorted, "progress never goes back: {dones:?}");
}

#[cfg(target_arch = "wasm32")]
#[test]
fn a_background_export_on_the_web_finishes_before_it_returns() {
    // The web has no threads: the job runs inside the command and is already done. It has no file
    // system either, so the job ends with an error rather than a file; what matters here is that
    // it does not stay busy and that the module compiles without the native-only dependencies.
    let mut s = session_with_pages(1);
    let queued = s.execute("file.exportPdf", &json!({"background": true, "path": "out.pdf"})).unwrap();
    let job = queued["job"].as_u64().unwrap();
    let done = s.execute("file.export.progress", &json!({"job": job})).unwrap();
    assert_eq!(done["busy"], false, "{done}");
    assert!(done.get("error").is_some() || done.get("path").is_some(), "{done}");
}

/// A cancelled export stops between pages and leaves no file.
#[cfg(not(target_arch = "wasm32"))]
#[test]
fn a_cancelled_background_export_writes_no_file() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("out.pdf").to_string_lossy().to_string();
    // Enough pages that the worker is still writing when the cancel arrives.
    let mut s = session_with_pages(200);
    let queued = s.execute("file.exportPdf", &json!({"background": true, "path": path, "created": 0})).unwrap();
    let job = queued["job"].as_u64().unwrap();
    let asked = s.execute("file.export.cancel", &json!({"job": job})).unwrap();
    assert_eq!(asked["cancelRequested"], true, "{asked}");
    let done = wait(&mut s, job);
    assert_eq!(done["busy"], false, "{done}");
    assert_eq!(done["cancelled"], true, "{done}");
    assert!(!std::path::Path::new(&path).exists(), "a cancelled export wrote no file");
}

/// Cancelling a job that already finished changes nothing.
#[cfg(not(target_arch = "wasm32"))]
#[test]
fn cancelling_a_finished_export_is_a_no_op() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("out.pdf").to_string_lossy().to_string();
    let mut s = session_with_pages(1);
    let queued = s.execute("file.exportPdf", &json!({"background": true, "path": path})).unwrap();
    let job = queued["job"].as_u64().unwrap();
    let done = wait(&mut s, job);
    let again = s.execute("file.export.cancel", &json!({"job": job})).unwrap();
    assert_eq!(again["cancelRequested"], false, "{again}");
    assert_eq!(again["cancelled"], done["cancelled"], "{again}");
}

/// Two exports of one document run at once, are both listed, and both write their file.
#[cfg(not(target_arch = "wasm32"))]
#[test]
fn two_exports_of_one_document_both_write() {
    let dir = tempfile::tempdir().unwrap();
    let a = dir.path().join("a.pdf").to_string_lossy().to_string();
    let b = dir.path().join("b.pdf").to_string_lossy().to_string();
    let mut s = session_with_pages(4);
    let j1 = s.execute("file.exportPdf", &json!({"background": true, "path": a, "created": 0})).unwrap()["job"].as_u64().unwrap();
    let j2 = s.execute("file.exportPdf", &json!({"background": true, "path": b, "created": 0})).unwrap()["job"].as_u64().unwrap();
    let list = s.execute("file.export.jobs", &json!({})).unwrap();
    assert_eq!(list["jobs"].as_array().map(Vec::len), Some(2), "{list}");
    assert_eq!(wait(&mut s, j1)["path"], a, "the first job");
    assert_eq!(wait(&mut s, j2)["path"], b, "the second job");
    assert!(std::path::Path::new(&a).exists() && std::path::Path::new(&b).exists(), "both files");
}

/// A document being exported is frozen: mutating commands and the mutating `noundo` ones are
/// refused, another document stays editable, and the menus grey the mutating commands.
#[test]
fn a_document_being_exported_refuses_mutations() {
    let mut s = session_with_pages(2);
    let uid = s.doc().unwrap().uid;
    // A job that stays running for the whole test: the real worker is covered by the other tests.
    let shared = std::sync::Arc::new(super::Shared::new(uid));
    let _job = s.export_jobs.insert(shared);
    assert!(s.exporting_active(), "the active document is exporting");
    for (id, params) in [("frame.create", json!({"rect": [0, 0, 10, 10]})), ("edit.undo", json!({})), ("layout.parents.edit", json!({}))] {
        let e = s.execute(id, &params).unwrap_err().to_string();
        assert!(e.contains("the document is being exported"), "{id}: {e}");
        let info = crate::find_command(id).unwrap().info(&s);
        assert!(!info.enabled && info.disabled_reason.as_deref() == Some(crate::cmd::EXPORTING), "{id}: {info:?}");
    }
    // A non-mutating command still runs.
    s.execute("preflight.run", &json!({})).unwrap();
    // Another document is not frozen.
    s.execute("file.new", &json!({"pages": 1, "facingPages": false})).unwrap();
    assert!(!s.exporting_active(), "the new document is not exporting");
    s.execute("frame.create", &json!({"rect": [0, 0, 10, 10]})).unwrap();
    // Back on the exporting document, it is frozen again.
    s.execute("file.activate", &json!({"index": 0})).unwrap();
    assert!(s.exporting_active(), "the first document is still exporting");
    assert!(s.execute("frame.create", &json!({"rect": [0, 0, 10, 10]})).is_err());
}
/// A preview refused because the document is exporting leaves the preview on screen alone: the
/// guard runs before the interaction's snapshot is restored, so the page is not reset.
#[test]
fn a_preview_refused_during_an_export_keeps_the_one_on_screen() {
    let mut s = Session::new();
    s.execute("file.new", &json!({})).unwrap();
    let r = s.execute("frame.create", &json!({"rect": [72, 72, 400, 300], "content": "text", "text": "Hello"})).unwrap();
    let sid = s.doc().unwrap().doc.item(designcraft_doc::ItemId(r["id"].as_u64().unwrap())).unwrap().text_frame().unwrap().story;
    s.execute("text.select", &json!({"story": sid.0, "anchor": 0, "focus": 5})).unwrap();
    let size = |s: &mut Session| s.execute("type.selectionAttrs", &json!({})).unwrap()["chars"]["size"].as_f64().unwrap();
    s.begin_interaction("Size").unwrap();
    s.preview("type.char", json!({"attrs": {"size": 20.0}})).unwrap();
    assert_eq!(size(&mut s), 20.0);
    // The document starts exporting (a job that never finishes, so the test does not race it).
    let uid = s.doc().unwrap().uid;
    let _job = s.export_jobs.insert(std::sync::Arc::new(super::Shared::new(uid)));
    let e = s.preview("type.char", json!({"attrs": {"size": 40.0}})).unwrap_err().to_string();
    assert!(e.contains("the document is being exported"), "{e}");
    assert_eq!(size(&mut s), 20.0, "the shown preview was not reset by the refused one");
}

/// A background export is refused while a tool gesture is open: the document holds the gesture's
/// preview, which the user can still cancel, so it is not what the file should contain.
#[cfg(not(target_arch = "wasm32"))]
#[test]
fn a_background_export_is_refused_during_a_gesture() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("out.pdf").to_string_lossy().to_string();
    let mut s = session_with_pages(1);
    s.begin_interaction("Drag").unwrap();
    let e = s.execute("file.exportPdf", &json!({"background": true, "path": path})).unwrap_err().to_string();
    assert!(e.contains("finish the gesture before exporting"), "{e}");
    s.cancel_interaction();
    assert!(s.execute("file.exportPdf", &json!({"background": true, "path": path})).is_ok(), "the export runs once the gesture ends");
}

/// A finished result is kept past `MAX_JOBS` until a caller has read it, so a poll that arrives
/// after many queued exports still reports the file the job wrote. Only read results age out.
#[test]
fn a_finished_result_is_kept_until_a_caller_reads_it() {
    let mut jobs = super::Jobs::default();
    let first = std::sync::Arc::new(super::Shared::new(0));
    let id = jobs.insert(first.clone());
    first.finish(Ok(super::Outcome { path: "a.pdf".into(), bytes: 1, pages: 1, warnings: Vec::new(), cancelled: false }));
    for _ in 0..super::MAX_JOBS + 2 {
        jobs.insert(std::sync::Arc::new(super::Shared::new(0)));
    }
    assert!(jobs.get(id).is_some(), "an unread result survives newer jobs");
    let v = super::job_value(id, &first);
    assert_eq!(v["path"], "a.pdf", "{v}");
    for _ in 0..super::MAX_JOBS + 2 {
        jobs.insert(std::sync::Arc::new(super::Shared::new(0)));
    }
    assert!(jobs.get(id).is_none(), "a read result ages out of the history");
}

/// A cancel is refused once the job has begun writing its file, so a `cancelRequested: true`
/// reply always means the job writes nothing.
#[test]
fn a_cancel_is_refused_once_the_write_began() {
    let mut jobs = super::Jobs::default();
    let shared = std::sync::Arc::new(super::Shared::new(0));
    let id = jobs.insert(shared.clone());
    assert_eq!(jobs.cancel(id), Some(true), "a running job accepts a cancel");
    let other = std::sync::Arc::new(super::Shared::new(0));
    let id = jobs.insert(other.clone());
    other.start_writing();
    assert_eq!(jobs.cancel(id), Some(false), "the write has begun");
    assert!(!other.cancelled(), "no flag was set");
}

/// The lock covers a `noundo` command's document change too: placing a caret in an empty frame
/// converts it through `object.content`, which is refused while the document is exported.
#[test]
fn placing_a_caret_that_would_convert_a_frame_is_refused_during_an_export() {
    let mut s = Session::new();
    s.execute("file.new", &json!({})).unwrap();
    let frame = s.execute("frame.create", &json!({"rect": [72.0, 72.0, 300.0, 200.0]})).unwrap()["id"].as_u64().unwrap();
    s.execute("object.content", &json!({"ids": [frame], "type": "unassigned"})).unwrap();
    let uid = s.doc().unwrap().uid;
    let _job = s.export_jobs.insert(std::sync::Arc::new(super::Shared::new(uid)));
    let e = s.execute("text.placeCaret", &json!({"frame": frame, "point": [100.0, 100.0]})).unwrap_err().to_string();
    assert!(e.contains("the document is being exported"), "the conversion is refused: {e}");
}
