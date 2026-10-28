use super::*;
use craft_ui::capability::Capability;
use designcraft_print::network::wsd::testing::{Ending, FakeConfig as Config, FakePrinter, MemTransport};
use net::{JobView, Query};

const ADDRESS: &str = "127.0.0.1:5357/printer";

fn app(config: Config) -> (DesignApp, Arc<FakePrinter>) {
    let printer = FakePrinter::new(config);
    let mut app = DesignApp::new(designcraft_engine::Session::new(), crate::Services::default());
    app.run("file.new", json!({"pages": 3})).unwrap();
    app.set_net_transport(Arc::new(MemTransport(printer.clone())));
    app.net_print.state.draft = net::Draft::for_address(ADDRESS);
    (app, printer)
}

fn wait(app: &mut DesignApp, mut done: impl FnMut(&DesignApp) -> bool) {
    for _ in 0..400 {
        app.poll_network();
        if done(app) {
            return;
        }
        std::thread::sleep(Duration::from_millis(25));
    }
    panic!("timed out waiting");
}

/// Press Print and wait for the worker to hand over the document, so the job has started.
fn send(app: &mut DesignApp, params: Value) {
    app.network_send(&params).unwrap();
    wait(app, |a| a.net_print.preparing.is_none());
}

fn draft(app: &DesignApp) -> &net::Draft {
    &app.net_print.state.draft
}

/// Ask the printer, as the Query button does, and wait for the answer.
fn query(app: &mut DesignApp) {
    let address_ok = validate(&app.net_print.state.draft.normalized_url()).is_ok();
    let worker = query_printer(app.net_print.transport(), None, Arc::clone(&app.net_print.queried));
    assert_eq!(app.net_print.state.start_query(&worker, address_ok), address_ok);
    wait(app, |a| !draft(a).querying());
}

#[test]
fn the_policy_refuses_what_it_should_before_anything_is_sent() {
    for bad in ["printer.local", "http://8.8.8.8/", "https://10.0.0.1/", "http://0.0.0.0/", "ipp://printer.local/ipp", "ipps://10.0.0.1/ipp/print"] {
        assert_eq!(validate(&net::normalize_url(bad)).map_err(|f| f.kind), Err(FailureKind::Policy), "{bad}");
    }
    assert!(validate(&net::normalize_url("192.168.1.50:5357/service")).is_ok());
    assert!(validate("http://[::1]:5357/x").is_ok());
    assert!(validate("ipp://192.168.1.50/ipp/print").is_ok());
}

#[test]
fn errors_map_onto_the_failures_the_dialog_words() {
    for (kind, want) in [
        (ErrorKind::InvalidRequest, FailureKind::Invalid),
        (ErrorKind::UnsupportedFormat, FailureKind::NoCommonFormat),
        (ErrorKind::Unavailable, FailureKind::Transport),
        (ErrorKind::Timeout, FailureKind::Transport),
        (ErrorKind::Failed, FailureKind::Protocol),
        (ErrorKind::Rejected, FailureKind::Fault),
        (ErrorKind::Cancelled, FailureKind::Stopped),
    ] {
        assert_eq!(failure(&PrintError::new(kind, "x"), false).kind, want, "{kind:?}");
    }
    assert_eq!(failure(&PrintError::new(ErrorKind::InvalidRequest, "x"), true).kind, FailureKind::Policy);
    assert_eq!(failure(&PrintError::new(ErrorKind::Failed, "x").with_phase(Phase::Prepare), false).kind, FailureKind::Encode);
}

#[test]
fn what_the_printer_leaves_out_is_unknown_and_what_it_lists_is_offered() {
    let info = PrinterInfo {
        colour: Some(false),
        sides: Some(vec!["OneSided".into()]),
        qualities: None,
        orientations: Some(Vec::new()),
        copies: Some((1, 5)),
        ..PrinterInfo::default()
    };
    let caps = caps_of(&info);
    assert!(matches!(caps.qualities, Capability::Unknown), "left out: unknown, enabled with a note");
    assert!(matches!(caps.orientations, Capability::Unsupported), "published empty: unsupported");
    assert!(matches!(caps.sides, Capability::Supported(_)));
    assert!(matches!(caps.color_modes, Capability::Supported(_)));
    assert!(matches!(caps.copies, Capability::Supported(_)));
    assert!(matches!(caps.resolutions, Capability::Unknown));
}

#[test]
fn sheet_sizes_name_the_paper_the_printers_list() {
    assert_eq!(media_for_size(595.28, 841.89), Some("iso_a4_210x297mm"));
    assert_eq!(media_for_size(842.0, 595.0), Some("iso_a4_210x297mm"), "landscape is the same paper");
    assert_eq!(media_for_size(612.0, 792.0), Some("na_letter_8.5x11in"));
    assert_eq!(media_for_size(700.0, 900.0), None, "a sheet with marks or bleed has no standard name");
}

#[test]
fn a_media_is_named_only_when_every_sheet_is_one_known_size() {
    const LETTER: (f64, f64) = (612.0, 792.0);
    const A4: (f64, f64) = (595.28, 841.89);
    assert_eq!(uniform_media(&[LETTER, LETTER, (792.0, 612.0)]), Some("na_letter_8.5x11in"), "landscape letter is letter");
    assert_eq!(uniform_media(&[A4]), Some("iso_a4_210x297mm"));
    assert_eq!(uniform_media(&[LETTER, A4]), None);
    assert_eq!(uniform_media(&[A4, LETTER, LETTER]), None);
    assert_eq!(uniform_media(&[LETTER, (1224.0, 792.0)]), None, "a spread is not the page's paper");
    assert_eq!(uniform_media(&[]), None);
}

/// The paper a job names comes from every sheet, not from the first one alone.
#[test]
fn a_mixed_size_document_names_no_paper_and_a_uniform_one_does() {
    let ticket = |app: &mut DesignApp, printer: &FakePrinter, n: u32| {
        app.net_print.state.draft.confirmed = true;
        send(app, json!({}));
        wait(app, |a| matches!(draft(a).job, JobView::Finished(_)));
        printer.job(n).unwrap().ticket
    };
    let (mut app, printer) = app(Config::default());
    app.run("file.new", json!({"pages": 2, "preset": "A4"})).unwrap();
    query(&mut app);
    assert!(ticket(&mut app, &printer, 1).contains("iso_a4_210x297mm"), "every sheet is A4");
    app.run("layout.pageSize", json!({"pages": [2], "preset": "Letter"})).unwrap();
    let mixed = ticket(&mut app, &printer, 2);
    assert!(!mixed.contains("iso_a4_210x297mm") && !mixed.contains("na_letter"), "A4 and Letter name no paper: {mixed}");
}

/// A spreads print expects as many sheets as the PDF has, not the document's page count.
#[test]
fn a_spreads_job_expects_the_sheets_it_sends() {
    let (mut app, printer) = app(Config::default());
    app.run("file.new", json!({"pages": 4, "facingPages": true})).unwrap();
    query(&mut app);
    app.net_print.state.draft.confirmed = true;
    send(&mut app, json!({"spreads": true}));
    let expected = draft(&app).expected_pages;
    wait(&mut app, |a| matches!(draft(a).job, JobView::Finished(_)));
    let sheets = designcraft_render::pdf_page_count(&printer.job(1).unwrap().document.unwrap()).unwrap();
    assert!(sheets < 4);
    assert_eq!(expected, u32::try_from(sheets).ok());
}

/// A job that ends while another dialog is open (Export PDF here) leaves that dialog alone and says
/// how it ended on the status line.
#[test]
fn a_job_that_ends_under_another_dialog_does_not_replace_it() {
    let (mut app, _) = app(Config::default());
    query(&mut app);
    app.net_print.state.draft.confirmed = true;
    send(&mut app, json!({}));
    app.ui.dialog = Some(crate::dialogs::Dialog::new("pdfExport", json!({"preset": "Custom"})));
    wait(&mut app, |a| matches!(draft(a).job, JobView::Finished(_)));
    let dialog = app.ui.dialog.as_ref().expect("a dialog is open");
    assert_eq!((dialog.id.as_str(), dialog.s("preset").as_str()), ("pdfExport", "Custom"), "the dialog is the person's, untouched");
    let JobView::Finished(report) = &draft(&app).job else { panic!("finished") };
    assert!(app.ui.status.contains(&app.finished_text(&labels("en"), report)), "the status line tells it: {}", app.ui.status);
}

/// An address changed while its query runs: the old answer ends the query, so the Query button and
/// printing are not blocked until the dialog is reopened.
#[test]
fn changing_the_address_mid_query_does_not_leave_it_running() {
    let (mut app, _) = app(Config::default());
    let worker = query_printer(app.net_print.transport(), None, Arc::clone(&app.net_print.queried));
    assert!(app.net_print.state.start_query(&worker, true));
    app.net_print.use_address("127.0.0.1:5358/other");
    wait(&mut app, |a| !draft(a).querying());
    assert!(draft(&app).printer().is_none(), "nothing was learned for the address now typed");
}

/// A worker thread that cannot be started is reported, not a panic.
#[test]
fn a_worker_that_cannot_start_is_a_failure() {
    let impossible = std::thread::Builder::new().stack_size(1 << 50);
    let failure = spawn_worker(impossible, || {}).unwrap_err();
    assert_eq!(failure.kind, FailureKind::Other);
    assert!(spawn_worker(std::thread::Builder::new(), || {}).is_ok());
}

/// The words the app adds around the network job follow the interface language.
#[test]
fn the_apps_own_print_words_are_translated() {
    let (mut app, _) = app(Config::default());
    app.ui.language = "de".into();
    query(&mut app);
    app.net_print.state.draft.confirmed = true;
    send(&mut app, json!({}));
    app.ui.dialog = None;
    wait(&mut app, |a| matches!(draft(a).job, JobView::Finished(_)));
    let alert = app.ui.dialog.as_ref().expect("the alert");
    assert_eq!(alert.s("title"), "Drucken");
    for english in ["Nothing to print", "the print job did not start", "the printer"] {
        assert_ne!(crate::i18n::tr("de", english), english, "{english}");
    }
}

#[test]
fn settings_become_job_options() {
    let request = Request {
        copies: Some(3),
        sides: Some(net::Sides::LongEdge),
        orientation: Some(net::Orientation::Landscape),
        quality: Some(net::Quality::High),
        color: Some(net::ColorMode::Monochrome),
        resolution: Some(net::Resolution::new(600, 600)),
        media_size: Some("iso_a4_210x297mm".into()),
        expected_pages: Some(4),
    };
    let (job, options) = options(&request, "Magazine\n");
    assert_eq!((job.copies, job.duplex, job.scaling), (3, Some(Duplex::LongEdge), Scaling::ShrinkToFit));
    assert_eq!(job.title, "Magazine");
    assert_eq!(options.media.as_deref(), Some("iso_a4_210x297mm"));
    assert_eq!(options.orientation, Some(Orientation::Landscape));
    assert_eq!(options.quality, Some(Quality::High));
    assert_eq!(options.colour, ColourMode::Mono);
    assert_eq!(options.expected_pages, Some(4));
    let (job, _) = super::options(&Request::default(), "");
    assert_eq!((job.copies, job.duplex), (1, None), "no setting means the printer's own");
}

#[test]
fn a_print_needs_the_ticked_confirmation_and_sends_nothing_without_it() {
    let (mut app, printer) = app(Config::default());
    assert_eq!(app.network_send(&json!({})).unwrap_err(), labels("").query_first, "query first");
    query(&mut app);
    assert!(matches!(draft(&app).query, Query::Done(_)));
    assert_eq!(app.network_send(&json!({})).unwrap_err(), labels("").confirm);
    assert_eq!(printer.jobs(), 0);
}

#[test]
fn a_confirmed_print_sends_the_exported_pages_follows_the_job_and_names_the_printer() {
    let (mut app, printer) = app(Config::default());
    query(&mut app);
    let name = draft(&app).printer().map(|p| p.name.clone()).unwrap();
    app.net_print.state.draft.confirmed = true;
    app.net_print.state.draft.copies = 2;
    send(&mut app, json!({"pages": "2-3"}));
    assert!(!draft(&app).confirmed, "the confirmation is spent by the job");
    assert!(draft(&app).running());
    let mut lines = Vec::new();
    wait(&mut app, |a| {
        lines.push(a.ui.status.clone());
        matches!(draft(a).job, JobView::Finished(_))
    });
    let JobView::Finished(report) = &draft(&app).job else { panic!("finished") };
    assert_eq!(report.outcome, Outcome::Completed);
    assert!(lines.iter().any(|l| l.contains(&name)), "a status line names the printer {name:?}: {lines:?}");
    assert_eq!(printer.jobs(), 1);
    let pdf = printer.job(1).unwrap().document.unwrap();
    assert!(pdf.starts_with(b"%PDF-"));
    assert_eq!(designcraft_render::pdf_page_count(&pdf), Some(2), "the chosen range, not the whole document");
}

#[test]
fn a_printer_that_lists_only_raster_gets_the_pages_rendered() {
    let (mut app, printer) = app(Config { formats: vec!["image/pwg-raster"], resolutions: vec![(100, 100)], ..Config::default() });
    query(&mut app);
    app.net_print.state.draft.confirmed = true;
    send(&mut app, json!({"pages": "1"}));
    wait(&mut app, |a| matches!(draft(a).job, JobView::Finished(_)));
    let doc = printer.job(1).unwrap().document.unwrap();
    assert!(doc.starts_with(b"RaS2"), "PWG raster");
}

#[test]
fn an_aborted_job_is_reported_and_with_the_dialog_gone_the_person_is_alerted() {
    let (mut app, _) = app(Config { ending: Ending::Aborts("PrinterStopped"), ..Config::default() });
    query(&mut app);
    app.net_print.state.draft.confirmed = true;
    send(&mut app, json!({}));
    app.ui.dialog = None;
    wait(&mut app, |a| matches!(draft(a).job, JobView::Finished(_)));
    let JobView::Finished(report) = &draft(&app).job else { panic!("finished") };
    assert_eq!(report.outcome, Outcome::Aborted);
    assert_eq!(app.ui.dialog.as_ref().map(|d| d.id.as_str()), Some("alert"));
}

#[test]
fn the_colour_choice_says_grayscale_and_a_grayscale_job_sends_gray_pages() {
    let l = labels("en");
    assert_eq!(l.color_monochrome, "Grayscale");
    assert_eq!(l.unsupported_color, "This printer only prints in grayscale.");
    assert_eq!(labels("de").color_monochrome, "Graustufen");
    let (mut app, printer) = app(Config::default());
    let id = app.run("frame.create", json!({"rect": [50, 50, 350, 350]})).unwrap()["id"].clone();
    app.run("selection.set", json!({"ids": [id]})).unwrap();
    app.run("object.color", json!({"color": "#ff0000"})).unwrap();
    query(&mut app);
    app.net_print.state.draft.confirmed = true;
    app.net_print.state.draft.color = Some(net::ColorMode::Monochrome);
    send(&mut app, json!({"pages": "1"}));
    wait(&mut app, |a| matches!(draft(a).job, JobView::Finished(_)));
    let pdf = printer.job(1).unwrap().document.unwrap();
    let colour = designcraft_engine::print_document(&mut app.session, &json!({"pages": "1"})).unwrap().pdf;
    let pc = designcraft_render::render_pdf_page(&colour, 0, 300).unwrap();
    assert!(pc.data().iter().any(|p| p.r != p.g || p.g != p.b), "the document has colour");
    let pm = designcraft_render::render_pdf_page(&pdf, 0, 300).unwrap();
    assert!(pm.data().iter().all(|p| p.r == p.g && p.g == p.b), "the sent pages are gray");
}

#[test]
fn queried_printers_are_remembered_by_their_reported_name_for_the_session() {
    let (mut app, _) = app(Config::default());
    assert!(app.net_print.known().is_empty());
    query(&mut app);
    app.net_print.remember_queried();
    app.net_print.remember_queried();
    let name = draft(&app).printer().map(|p| p.name.clone()).unwrap();
    assert_eq!(app.net_print.known(), [(name, ADDRESS.to_owned())]);
    app.net_print.use_address("127.0.0.1:5358/other");
    assert!(draft(&app).printer().is_none(), "another address is another printer");
}

/// The dialog stays open while a network job runs: the refusal of an earlier OK (the confirmation
/// not yet ticked) must be gone once the job has started, not shown beside it in the error colour.
#[test]
fn a_started_job_clears_the_earlier_refusal_from_the_open_print_dialog() {
    let (mut app, printer) = app(Config::default());
    query(&mut app);
    let mut dialog = crate::dialogs::Dialog::new("print", json!({}));
    dialog.fields.insert("destination".into(), json!("network"));
    dialog.fields.insert("range".into(), json!("all"));
    app.ui.dialog = Some(dialog);
    assert_eq!(crate::dialogs::confirm(&mut app).unwrap_err(), labels("").confirm);
    let shown = app.ui.dialog.as_ref().unwrap();
    assert_eq!(shown.fields.get("status").and_then(Value::as_str), Some(labels("").confirm), "the refusal is in the dialog");
    app.net_print.state.draft.confirmed = true;
    crate::dialogs::confirm(&mut app).unwrap();
    let shown = app.ui.dialog.as_ref().expect("the dialog stays open while the job runs");
    assert!(!shown.fields.contains_key("status"), "no stale error beside a running job");
    wait(&mut app, |a| matches!(draft(a).job, JobView::Finished(_)));
    assert_eq!(printer.jobs(), 1);
}

/// The UI thread only takes a snapshot: the document is made on a worker, the job starts when the
/// finished document is collected, and it names the sheets and paper of that document.
#[test]
fn print_preparation_runs_off_the_ui_thread_and_the_job_starts_when_it_is_ready() {
    let (mut app, printer) = app(Config::default());
    app.run("file.new", json!({"pages": 3, "preset": "A4"})).unwrap();
    query(&mut app);
    app.net_print.state.draft.confirmed = true;
    let (release, gate) = mpsc::channel::<()>();
    let rendering = Arc::new(std::sync::atomic::AtomicBool::new(false));
    let seen = Arc::clone(&rendering);
    app.network_send_with(&json!({}), move |snapshot| {
        seen.store(true, std::sync::atomic::Ordering::SeqCst);
        gate.recv().unwrap();
        snapshot.render().map_err(|e| e.to_string())
    })
    .unwrap();
    // The call has returned while the render is still held back.
    assert!(app.net_print.preparing.is_some());
    assert!(!draft(&app).running() && draft(&app).expected_pages.is_none(), "no job yet");
    assert_eq!(app.network_block(), Some(net::Block::Busy), "a second Print waits for the first");
    assert!(app.network_send(&json!({})).is_err());
    for _ in 0..5 {
        app.poll_network();
        std::thread::sleep(Duration::from_millis(10));
    }
    assert!(app.net_print.preparing.is_some() && printer.jobs() == 0, "nothing is sent while the document is made");
    release.send(()).unwrap();
    wait(&mut app, |a| a.net_print.preparing.is_none());
    assert!(rendering.load(std::sync::atomic::Ordering::SeqCst));
    assert!(draft(&app).running(), "the collected document started the job");
    assert!(!draft(&app).confirmed, "the confirmation is spent when the job starts");
    assert_eq!(draft(&app).expected_pages, Some(3));
    wait(&mut app, |a| matches!(draft(a).job, JobView::Finished(_)));
    assert!(printer.job(1).unwrap().ticket.contains("iso_a4_210x297mm"));
}

/// A document that cannot be made says why on the status line and starts nothing.
#[test]
fn a_print_that_cannot_be_prepared_is_told_and_sends_nothing() {
    let (mut app, printer) = app(Config::default());
    query(&mut app);
    app.net_print.state.draft.confirmed = true;
    app.network_send_with(&json!({}), |_| Err("no pages".into())).unwrap();
    wait(&mut app, |a| a.net_print.preparing.is_none());
    assert_eq!(app.ui.status, "no pages");
    assert!(!draft(&app).running());
    assert!(draft(&app).confirmed, "the confirmation was not spent");
    assert_eq!(printer.jobs(), 0);
}
