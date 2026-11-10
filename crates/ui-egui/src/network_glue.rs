//! Print ▸ Network printer (WS-Print and IPP): the app's part of `craft_ui::printing::network`. The
//! model, its rules and the widgets are craft-ui's; the printing is craft-print's registry through
//! `designcraft_print::network`. This file supplies what a host supplies: the address check, the
//! worker that asks a printer what it lists, the worker that sends a job and follows it, the
//! mapping of craft-print's answers onto craft-ui's light types, and the wording.
//!
//! Not built for wasm32 (there is no socket there). All network work runs on its own threads; the UI
//! thread only starts it and drains receivers. Making the print-ready PDF (export, grayscale
//! rendering) is work too: the UI thread only takes a snapshot of the document and settings
//! (`designcraft_engine::print_snapshot`), a worker renders it, and the job starts when the UI
//! thread collects the finished document (`poll_network`).
//!
//! The document is the print-ready PDF of the pages the dialog chose (`designcraft_engine::print_document`).
//! A printer that lists PDF gets it as it is; one that lists only PWG raster gets the pages rendered by
//! `PdfToRaster`, the converter registered once when the destination is created (the registry converts,
//! at the resolution the printer lists nearest 300 dpi). The printer the person queried is remembered,
//! so the job does not read it again before the registry decides.
//!
//! A job has two phases and so does its worker: it sends the document and reports **accepted** as soon
//! as the printer has it (`JobUpdate::Accepted`), then follows the job with craft-print's `JobWatch`,
//! reporting each new status (`JobUpdate::Status`) until it ends. A document is sent once: a send whose
//! outcome is unknown is shown with the request to check the printer before sending again
//! (`Failure::outcome_unknown`), and nothing retries it. The status line names the printer.

use std::sync::{Arc, Mutex, PoisonError, mpsc};
use std::time::Duration;

use craft_ui::capability::Capability;
use craft_ui::printing::network::{
    self as net, Caps, Failure, FailureKind, JobHandle, JobUpdate, Labels, Outcome, PrinterSummary, Progress, QueryPrinter, Report, Request, SendJob,
    Stage,
};
use designcraft_engine::{PrintDocument, PrintSnapshot};
use designcraft_print::network::{
    self as wire, CancelToken, ColourMode, Document, Duplex, ErrorKind, Job, JobOptions, JobState, JobStatus, Orientation, Phase, PrintError,
    Printer, PrinterInfo, Quality, Resolution, Scaling, UserConfirmed, WaitPolicy, WatchOutcome, WatchStep,
};
use serde_json::{Value, json};

use crate::DesignApp;

/// Any transport that can be shared with the worker threads (tests give an in-memory printer).
pub type Transport = designcraft_print::network::Transport;

/// The media types the dialog can produce: the print-ready PDF, or its sheets as PWG raster (the
/// registry converts through `PdfToRaster` when the printer lists no PDF).
pub(crate) const FORMATS: &[&str] = &["application/pdf", "image/pwg-raster"];

/// What the last query read, so a job to that printer does not read it again.
type Queried = Arc<Mutex<Option<(String, PrinterInfo)>>>;

fn lock<T>(m: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    m.lock().unwrap_or_else(PoisonError::into_inner)
}
const MAX_JOB_NAME: usize = 100;
/// How long a job is followed after the printer accepted it; it carries on at the printer after that.
const FOLLOW_FOR: Duration = Duration::from_secs(300);

/// The destination's state and the closures its workers are made from.
pub(crate) struct NetTasks {
    transport: Option<Transport>,
    pub(crate) state: net::State,
    query: Option<QueryPrinter>,
    queried: Queried,
    /// The printer of the job in flight (its name as the printer reported it, else the address).
    printer: String,
    /// The last stage line shown for the job, to put on the status bar only when it changes.
    shown: Option<String>,
    /// Network printers queried in this session (reported name, address as typed), so the printer
    /// list offers them again without retyping. Memory only: nothing is written to disk.
    known: Vec<(String, String)>,
    /// The PDF of a print being made on a worker; the job starts when it arrives.
    preparing: Option<Preparing>,
}

/// A print whose document is being made: where it will arrive, and the person's confirmation taken
/// when Print was pressed.
struct Preparing {
    document: mpsc::Receiver<Result<PrintDocument, String>>,
    confirmed: UserConfirmed,
}

impl Default for NetTasks {
    /// Creating the destination is the start-up of network printing: the app's `DocumentConverter`
    /// (PDF to PWG raster, with this build's renderer) is registered once, here.
    fn default() -> Self {
        wire::register_pdf_converter();
        Self {
            transport: None,
            state: net::State::default(),
            query: None,
            queried: Queried::default(),
            printer: String::new(),
            shown: None,
            known: Vec::new(),
            preparing: None,
        }
    }
}

impl NetTasks {
    pub(crate) fn set_transport(&mut self, transport: Transport) {
        self.transport = Some(transport);
        self.query = None;
    }

    /// Remember the printer the last query reached (by the name it reported), for this session.
    pub(crate) fn remember_queried(&mut self) {
        let draft = &self.state.draft;
        if let Some(p) = draft.printer() {
            let (name, url) = (p.name.trim().to_owned(), draft.url.trim().to_owned());
            if !name.is_empty() && !url.is_empty() && !self.known.iter().any(|(_, u)| *u == url) {
                self.known.push((name, url));
            }
        }
    }

    /// The network printers queried so far: (name, address).
    pub(crate) fn known(&self) -> &[(String, String)] {
        &self.known
    }

    /// Make the network printer at `url` the destination's address (the query is then to be run).
    pub(crate) fn use_address(&mut self, url: &str) {
        if self.state.draft.url != url {
            self.state.draft.url = url.to_owned();
            self.state.draft.address_edited();
        }
    }

    pub(crate) fn transport(&self) -> Option<Transport> {
        self.transport.clone()
    }

    /// Draw the section; `sheets_ok`: the dialog's settings give something to print.
    pub(crate) fn show(&mut self, ui: &mut egui::Ui, lang: &str, sheets_ok: bool) -> net::Response {
        let (transport, ctx, queried) = (self.transport.clone(), ui.ctx().clone(), Arc::clone(&self.queried));
        let query = self.query.get_or_insert_with(|| query_printer(transport, Some(ctx), queried));
        let host = net::Host { validate: &validate, query, sheets_ok, formats: FORMATS };
        self.state.show(ui, egui::Id::unique("network-printer"), &host, &labels(lang))
    }
}

/// The address checked against the network policy of its protocol (a numeric loopback, link-local
/// or private address). The error is what the dialog shows under the field.
pub(crate) fn validate(text: &str) -> Result<(), Failure> {
    wire::validate_address(text).map_err(|e| Failure::new(FailureKind::Policy, e.message()))
}

/// What went wrong, as craft-ui words it. `open`: the failure is the printer's address being refused.
fn failure(e: &PrintError, open: bool) -> Failure {
    let kind = if e.native().is_some_and(|n| n.domain == "wsd-fault") {
        FailureKind::Fault
    } else if e.phase() == Some(Phase::Prepare) && e.kind() == ErrorKind::Failed {
        // The document could not be made into something the printer takes (the converter failed).
        FailureKind::Encode
    } else {
        match e.kind() {
            ErrorKind::InvalidRequest if open => FailureKind::Policy,
            ErrorKind::InvalidRequest | ErrorKind::ConsentRequired => FailureKind::Invalid,
            ErrorKind::UnsupportedFormat => FailureKind::NoCommonFormat,
            ErrorKind::Unsupported => FailureKind::Unsupported,
            ErrorKind::Unavailable | ErrorKind::Timeout | ErrorKind::Busy | ErrorKind::Io => FailureKind::Transport,
            ErrorKind::Failed => FailureKind::Protocol,
            ErrorKind::Rejected | ErrorKind::NotFound => FailureKind::Fault,
            ErrorKind::Cancelled => FailureKind::Stopped,
            _ => FailureKind::Other,
        }
    };
    Failure::new(kind, e.message()).with_outcome_unknown(e.outcome_unknown())
}

fn summary(info: PrinterInfo) -> PrinterSummary {
    let caps = caps_of(&info);
    PrinterSummary { name: info.name, color: info.colour, state: info.state, queued_jobs: info.queued_jobs, reasons: info.reasons, caps }
}

/// What the printer published, as craft-ui's three-state `Caps`: a list it left out is unknown (the
/// control stays enabled with a note and the value is sent as a hint), a list it published empty is
/// unsupported (disabled with a reason), a list with values offers exactly those. WS-Print cannot say
/// "an empty list" and publishes no colour list, only whether the printer prints in colour.
fn caps_of(info: &PrinterInfo) -> Caps {
    let tokens = |list: &Option<Vec<String>>| Capability::from_option_list(list.as_deref());
    let resolutions: Vec<net::Resolution> = info.resolutions.iter().map(|&(x, y)| net::Resolution::new(x, y)).collect();
    Caps {
        formats: info.formats.clone(),
        sides: tokens(&info.sides),
        orientations: tokens(&info.orientations),
        qualities: tokens(&info.qualities),
        color_modes: match info.colour {
            None => Capability::Unknown,
            Some(true) => {
                Capability::from_option_list(Some(&[net::ColorMode::Color.token().to_owned(), net::ColorMode::Monochrome.token().to_owned()]))
            }
            Some(false) => Capability::from_option_list(Some(&[net::ColorMode::Monochrome.token().to_owned()])),
        },
        media_sizes: tokens(&info.media),
        resolutions: if resolutions.is_empty() { Capability::Unknown } else { Capability::from_option_list(Some(&resolutions)) },
        copies: Capability::from_option_range(info.copies),
    }
}

fn repaint(ctx: &Option<egui::Context>) {
    if let Some(ctx) = ctx {
        ctx.request_repaint();
    }
}

/// The worker that asks the printer at the (normalised) address what it is and what it lists. It
/// sends no job.
pub(crate) fn query_printer(transport: Option<Transport>, ctx: Option<egui::Context>, queried: Queried) -> QueryPrinter {
    Box::new(move |address: &str| {
        let (tx, rx) = mpsc::channel();
        let (address, transport, ctx, queried) = (address.to_owned(), transport.clone(), ctx.clone(), Arc::clone(&queried));
        let failed = tx.clone();
        let spawned = spawn_worker(std::thread::Builder::new().name("network-print-query".into()), move || {
            let result = match Printer::open(&address, transport) {
                Ok(printer) => printer
                    .info()
                    .map(|info| {
                        *lock(&queried) = Some((printer.address(), info.clone()));
                        summary(info)
                    })
                    .map_err(|e| failure(&e, false)),
                Err(e) => Err(failure(&e, true)),
            };
            let _ = tx.send(result);
            repaint(&ctx);
        });
        if let Err(f) = spawned {
            let _ = failed.send(Err(f));
        }
        rx
    })
}

/// The `MediaSizeName` for a sheet of `width` x `height` points (either way up), for the sizes
/// printers list; `None` for anything else (a print with marks or bleed has a sheet of its own size,
/// and the printer's default paper is used).
pub(crate) fn media_for_size(width: f64, height: f64) -> Option<&'static str> {
    const SIZES: &[(f64, f64, &str)] = &[
        (612.0, 792.0, "na_letter_8.5x11in"),
        (612.0, 1008.0, "na_legal_8.5x14in"),
        (792.0, 1224.0, "na_ledger_11x17in"),
        (841.89, 1190.55, "iso_a3_297x420mm"),
        (595.28, 841.89, "iso_a4_210x297mm"),
        (419.53, 595.28, "iso_a5_148x210mm"),
    ];
    let (short, long) = if width <= height { (width, height) } else { (height, width) };
    SIZES.iter().find(|(w, h, _)| (short - w).abs() <= 3.0 && (long - h).abs() <= 3.0).map(|(_, _, name)| *name)
}

/// The `MediaSizeName` for a job whose sheets are `sizes`: named only when every sheet is the same
/// known size (either way up), so spreads or mixed page sizes are not all fitted to the first one's paper.
pub(crate) fn uniform_media(sizes: &[(f64, f64)]) -> Option<&'static str> {
    let first = sizes.first().and_then(|&(w, h)| media_for_size(w, h))?;
    sizes.iter().all(|&(w, h)| media_for_size(w, h) == Some(first)).then_some(first)
}

/// Run `work` on a thread made by `builder`; a thread that cannot be made is a failure, not a panic.
fn spawn_worker(builder: std::thread::Builder, work: impl FnOnce() + Send + 'static) -> Result<(), Failure> {
    builder.spawn(work).map(drop).map_err(|e| Failure::new(FailureKind::Other, e.to_string()))
}

/// The job and its per-job options for the registry. `Photo` is craft-print's `Quality::Photo`: the
/// backend maps it to the printer's own quality when the printer lists one and says so when it does not.
/// `Scaling::ShrinkToFit` is what the system spooler path also asks for: a printer that lists
/// print-scaling (IPP) fits the page to its paper; WS-Print has no scaling element.
fn options(request: &Request, job_name: &str) -> (Job, JobOptions) {
    let name: String = job_name.chars().filter(|c| !c.is_control()).take(MAX_JOB_NAME).collect();
    let mut job = Job::default();
    if !name.trim().is_empty() {
        job.title = name;
    }
    job.copies = request.copies.unwrap_or(1);
    job.scaling = Scaling::ShrinkToFit;
    job.duplex = request.sides.map(|s| match s {
        net::Sides::OneSided => Duplex::Off,
        net::Sides::LongEdge => Duplex::LongEdge,
        net::Sides::ShortEdge => Duplex::ShortEdge,
    });
    let mut options = JobOptions::new();
    if let Some(o) = request.orientation {
        options = options.with_orientation(match o {
            net::Orientation::Portrait => Orientation::Portrait,
            net::Orientation::Landscape => Orientation::Landscape,
            net::Orientation::ReversePortrait => Orientation::ReversePortrait,
            net::Orientation::ReverseLandscape => Orientation::ReverseLandscape,
        });
    }
    match request.quality {
        Some(net::Quality::Draft) => options = options.with_quality(Quality::Draft),
        Some(net::Quality::Normal) => options = options.with_quality(Quality::Normal),
        Some(net::Quality::High) => options = options.with_quality(Quality::High),
        Some(net::Quality::Photo) => options = options.with_quality(Quality::Photo),
        None => {}
    }
    if let Some(color) = request.color {
        options = options.with_colour(match color {
            net::ColorMode::Color => ColourMode::Colour,
            net::ColorMode::Monochrome => ColourMode::Mono,
        });
    }
    if let Some(r) = request.resolution {
        options = options.with_resolution(Resolution::new(r.x_dpi, r.y_dpi));
    }
    if let Some(media) = &request.media_size {
        options = options.with_media(media.as_str());
    }
    if let Some(pages) = request.expected_pages {
        options = options.with_expected_pages(pages);
    }
    (job, options)
}

/// What the printer says about an accepted job, as craft-ui shows it.
fn progress(status: &JobStatus) -> Progress {
    Progress {
        stage: Stage::from_name(status.state.name()),
        sheets_done: status.sheets_done,
        sheets_total: status.sheets_total,
        notes: status.reasons.clone(),
    }
}

/// How following a job ended, as craft-ui's report. The document was accepted in every case, so a
/// watch that could not finish is "sent, outcome unknown", never a failure.
fn report(outcome: &WatchOutcome, printer: &str, warnings: &[String]) -> Report {
    let (kind, status) = match outcome {
        WatchOutcome::Terminal(s) => (
            match s.state {
                // craft-print models "completed with warnings" as Completed plus warnings on the status.
                JobState::Completed if !s.warnings.is_empty() => Outcome::CompletedWithWarnings,
                JobState::Completed => Outcome::Completed,
                JobState::CompletedWithErrors => Outcome::CompletedWithErrors,
                JobState::Cancelled => Outcome::Canceled,
                JobState::Aborted => Outcome::Aborted,
                _ => Outcome::AcceptedUnknown,
            },
            Some(s),
        ),
        WatchOutcome::Forgotten { last } => (Outcome::Forgotten, last.as_ref()),
        WatchOutcome::TimedOut { last } => (Outcome::StillRunning, last.as_ref()),
        _ => (Outcome::AcceptedUnknown, None),
    };
    let mut notes: Vec<String> = status.map(|s| s.reasons.clone()).unwrap_or_default();
    notes.extend(warnings.iter().cloned());
    if let WatchOutcome::Failed(e) = outcome {
        notes.push(e.message().to_owned());
    }
    Report { outcome: kind, printer: printer.to_owned(), sheets: status.and_then(|s| s.sheets_done).map_or(0, u64::from), notes }
}

/// The worker that sends the print-ready `pdf` and follows the job. `queried` is what the last query
/// read of the printer, used when it is the one addressed. Each call makes a new job, with the
/// person's `confirmed` (never `interactive()`: it is the attestation of the ticked box, taken when
/// Print was pressed).
pub(crate) fn send_job(
    transport: Option<Transport>,
    ctx: Option<egui::Context>,
    name: String,
    pdf: Arc<Vec<u8>>,
    queried: Queried,
    confirmed: UserConfirmed,
) -> SendJob {
    Box::new(move |address: &str, request: Request| {
        let (result_tx, result_rx) = mpsc::channel();
        let (update_tx, update_rx) = mpsc::channel();
        let cancel = CancelToken::new();
        let (address, transport, ctx) = (address.to_owned(), transport.clone(), ctx.clone());
        let (name, pdf, queried, worker_cancel) = (name.clone(), Arc::clone(&pdf), Arc::clone(&queried), cancel.clone());
        let failed = result_tx.clone();
        let spawned = spawn_worker(std::thread::Builder::new().name("network-print-job".into()), move || {
            let (job, options) = options(&request, &name);
            let result = run_job(&address, transport, (job, options), &pdf, &queried, confirmed, &worker_cancel, &update_tx, &ctx);
            let _ = result_tx.send(result);
            repaint(&ctx);
        });
        if let Err(f) = spawned {
            let _ = failed.send(Err(f));
        }
        JobHandle::new(result_rx, Box::new(move || cancel.cancel())).with_updates(update_rx)
    })
}

#[allow(clippy::too_many_arguments)]
fn run_job(
    address: &str,
    transport: Option<Transport>,
    (job, options): (Job, JobOptions),
    pdf: &Arc<Vec<u8>>,
    queried: &Queried,
    confirmed: UserConfirmed,
    cancel: &CancelToken,
    updates: &mpsc::Sender<JobUpdate>,
    ctx: &Option<egui::Context>,
) -> Result<Report, Failure> {
    let printer = Printer::open(address, transport).map_err(|e| failure(&e, true))?;
    // What the person was shown is what the registry decides on: no second read of the printer.
    if let Some((_, info)) = lock(queried).as_ref().filter(|(a, _)| *a == printer.address()) {
        printer.remember(info);
    }
    // Phase one: the printer accepts the document. A cancel before that sends nothing, or cancels the job just made.
    let sent = match printer.submit(Document::pdf(pdf.as_slice()), job, options, confirmed, cancel, &mut |_| {}) {
        Ok(sent) => sent,
        Err(e) if e.kind() == ErrorKind::Cancelled => {
            return Ok(Report { outcome: Outcome::Canceled, printer: String::new(), sheets: 0, notes: Vec::new() });
        }
        Err(e) => return Err(failure(&e, false)),
    };
    let notes = sent.warnings.clone();
    let _ = updates.send(JobUpdate::Accepted(Progress { notes: notes.clone(), ..Progress::default() }));
    repaint(ctx);
    // Phase two: follow it. One cancel is sent if asked; the watch then carries on to the end state.
    let policy = WaitPolicy { interval: Some(Duration::from_millis(500)), ..WaitPolicy::default() }.with_max_wait(FOLLOW_FOR);
    let outcome = match printer.watch(&sent, policy) {
        Ok(mut watch) => {
            watch = watch.with_cancel(cancel.clone());
            loop {
                match watch.tick(printer.source()) {
                    WatchStep::Pending { next_in } => std::thread::sleep(next_in.min(Duration::from_millis(100))),
                    WatchStep::Update(status) => {
                        let _ = updates.send(JobUpdate::Status(progress(&status)));
                        repaint(ctx);
                    }
                    WatchStep::Done(outcome) => break outcome,
                    _ => std::thread::sleep(Duration::from_millis(100)),
                }
            }
        }
        Err(e) => WatchOutcome::Failed(e),
    };
    Ok(report(&outcome, &sent.printer_name, &notes))
}

/// The wording of the destination, in the interface language. Templates keep their `{...}` holes.
pub(crate) fn labels(lang: &str) -> Labels<'static> {
    macro_rules! tl {
        ($s:literal) => {
            crate::i18n::tr(lang, $s)
        };
    }
    Labels {
        address: tl!("Printer address:"),
        address_hint: "http://192.168.1.50:5357/service  or  ipp://192.168.1.50/ipp/print",
        address_note: tl!("Only IP addresses on this network are accepted (private, link-local or loopback), not host names."),
        query_printer: tl!("Query printer"),
        querying: tl!("Asking the printer…"),
        query_first: tl!("Query the printer first to see what it supports."),
        printer_status: tl!("Printer status: {state}"),
        reasons: tl!("Reasons: {reasons}"),
        no_common_format: tl!("The printer accepts none of the formats this document can be sent in"),
        copies: tl!("Copies:"),
        sides: tl!("Two-sided:"),
        sides_off: tl!("Off"),
        sides_long_edge: tl!("Flip on long edge"),
        sides_short_edge: tl!("Flip on short edge"),
        orientation: tl!("Printer orientation:"),
        portrait: tl!("Portrait"),
        landscape: tl!("Landscape"),
        reverse_portrait: tl!("Reverse portrait"),
        reverse_landscape: tl!("Reverse landscape"),
        quality: tl!("Quality:"),
        draft: tl!("Draft"),
        normal: tl!("Normal"),
        high: tl!("High"),
        photo: tl!("Photo"),
        color: tl!("Color:"),
        color_color: tl!("Colour"),
        color_monochrome: tl!("Grayscale"),
        resolution: tl!("Resolution:"),
        resolution_dpi: "{dpi} dpi",
        resolution_dpi_xy: "{x} x {y} dpi",
        auto: tl!("Auto"),
        unsupported_copies: tl!("This printer does not take a copies setting."),
        unsupported_sides: tl!("This printer only prints one-sided."),
        unsupported_orientation: tl!("This printer does not take an orientation setting."),
        unsupported_quality: tl!("This printer does not take a quality setting."),
        unsupported_color: tl!("This printer only prints in grayscale."),
        unsupported_resolution: tl!("This printer does not take a resolution setting."),
        unconfirmed_note: tl!("Not confirmed by the printer"),
        confirm: tl!("I confirm sending this document to this printer"),
        sending: tl!("Sending to {printer}…"),
        canceling: tl!("Canceling…"),
        cancel: tl!("Cancel"),
        sent: tl!("Sent to {printer}"),
        accepted: tl!("Accepted by {printer}"),
        stage_waiting: tl!("Waiting in the printer's queue"),
        stage_held: tl!("Held by the printer"),
        stage_printing: tl!("The printer is printing it"),
        stage_stopped: tl!("The printer paused the job; it needs attention"),
        sheets: tl!("Sheets printed: {n}"),
        sheets_progress: tl!("Sheets printed: {done} of {total}"),
        outcome_warnings: tl!("Printed with warnings"),
        outcome_errors: tl!("Printed with errors"),
        outcome_canceled: tl!("The job was canceled"),
        outcome_aborted: tl!("The printer stopped the job"),
        outcome_forgotten: tl!("The printer no longer lists the job; it most likely finished"),
        outcome_still_running: tl!("Still printing; the job continues on the printer"),
        outcome_accepted_unknown: tl!("Sent. The printer can't say whether it printed."),
        outcome_unknown_warning: tl!("The printer may have received the document. Check the printer before sending again."),
        failure_policy: tl!("The printer address was refused"),
        failure_transport: tl!("Couldn't reach the printer"),
        failure_protocol: tl!("The printer's reply couldn't be understood"),
        failure_fault: tl!("The printer refused the request"),
        failure_no_common_format: tl!("The printer accepts none of the formats this document can be sent in"),
        failure_unsupported: tl!("The printer doesn't support this setting"),
        failure_encode: tl!("The document couldn't be prepared"),
        failure_invalid: tl!("A setting isn't valid"),
        failure_stopped: tl!("The print job stopped unexpectedly"),
        failure_other: tl!("The print job stopped unexpectedly"),
    }
}

fn fill(template: &str, printer: &str) -> String {
    template.replace("{printer}", printer)
}

impl DesignApp {
    /// Use `transport` for network printing instead of the built-in HTTP one (tests; embedding).
    pub fn set_net_transport(&mut self, transport: Transport) {
        self.net_print.set_transport(transport);
    }

    /// Why the network job cannot start now, when it cannot (the dialog's rules, craft-ui's).
    pub(crate) fn network_block(&self) -> Option<net::Block> {
        if self.net_print.preparing.is_some() {
            return Some(net::Block::Busy);
        }
        let draft = &self.net_print.state.draft;
        draft.block(validate(&draft.normalized_url()).is_ok(), self.session.active().is_some(), FORMATS)
    }

    /// Export the pages `params` select (`pages`, `spreads`, `marks`, `bleed`) as the print-ready PDF and
    /// send it to the queried printer, off the UI thread. It needs the person's confirmation (the ticked
    /// box), which it spends when the job starts. `Ok(())` when the print was taken up: the document is
    /// made on a worker, and `poll_network` starts the job (or tells why it could not) when it is ready.
    pub(crate) fn network_send(&mut self, params: &Value) -> Result<(), String> {
        self.network_send_with(params, |snapshot| snapshot.render().map_err(|e| e.to_string()))
    }

    /// [`Self::network_send`] with the worker's rendering step given (tests hold it back).
    fn network_send_with(
        &mut self,
        params: &Value,
        render: impl FnOnce(PrintSnapshot) -> Result<PrintDocument, String> + Send + 'static,
    ) -> Result<(), String> {
        if let Some(block) = self.network_block() {
            let l = labels(&self.ui.language);
            let printer = self.net_print.state.draft.printer().map_or_else(String::new, |p| p.name.clone());
            return Err(match block {
                net::Block::Address => l.failure_policy.to_owned(),
                net::Block::NotQueried => l.query_first.to_owned(),
                net::Block::Format => l.no_common_format.to_owned(),
                net::Block::Unconfirmed => l.confirm.to_owned(),
                net::Block::Busy => fill(l.sending, &printer),
                _ => crate::i18n::tr(&self.ui.language, "Nothing to print").to_owned(),
            });
        }
        // A grayscale choice is honoured in the document itself, not only as the printer's colour hint.
        let mut params = params.clone();
        if self.net_print.state.draft.color == Some(net::ColorMode::Monochrome) {
            params["grayscale"] = json!(true);
        }
        let snapshot = designcraft_engine::print_snapshot(&self.session, &params).map_err(|e| e.to_string())?;
        // The attestation comes from the ticked box and nothing else.
        let net = &mut self.net_print;
        let confirmed = UserConfirmed::attest(Some(net.state.draft.confirmed)).map_err(|e| e.to_string())?;
        let (tx, document) = mpsc::channel();
        let ctx = self.egui_ctx.clone();
        spawn_worker(std::thread::Builder::new().name("network-print-prepare".into()), move || {
            let _ = tx.send(render(snapshot));
            repaint(&ctx);
        })
        .map_err(|f| f.detail)?;
        net.preparing = Some(Preparing { document, confirmed });
        net.printer = net.state.draft.printer().map(|p| p.name.clone()).unwrap_or_default();
        net.shown = None;
        let line = fill(labels(&self.ui.language).sending, &net.printer);
        self.status(line);
        Ok(())
    }

    /// Collect the document of a print being made and start its job (each frame). A document that
    /// could not be made, or a job that cannot start now, is told on the status line and in the Print
    /// dialog, as a refusal at the button is.
    fn poll_prepared(&mut self) {
        let Some(preparing) = &self.net_print.preparing else { return };
        let made = match preparing.document.try_recv() {
            Ok(made) => made,
            Err(mpsc::TryRecvError::Empty) => return,
            Err(mpsc::TryRecvError::Disconnected) => Err(crate::i18n::tr(&self.ui.language, "the print job did not start").to_owned()),
        };
        let confirmed = preparing.confirmed;
        self.net_print.preparing = None;
        if let Err(e) = made.and_then(|doc| self.start_prepared(doc, confirmed)) {
            if let Some(dialog) = self.ui.dialog.as_mut().filter(|d| d.id == "print") {
                dialog.fields.insert("status".into(), json!(e));
            }
            self.status(e);
        }
    }

    /// Start the job for the finished `doc`: the sheets it has and the paper they are decide what the
    /// job expects and names.
    fn start_prepared(&mut self, doc: PrintDocument, confirmed: UserConfirmed) -> Result<(), String> {
        let net = &mut self.net_print;
        let sizes = designcraft_render::pdf_page_sizes(&doc.pdf).unwrap_or_default();
        let media = uniform_media(&sizes);
        let send = send_job(net.transport(), self.egui_ctx.clone(), doc.title, Arc::new(doc.pdf), Arc::clone(&net.queried), confirmed);
        // The sheets sent, not the document's pages: a spread is one sheet.
        net.state.draft.expected_pages = u32::try_from(sizes.len()).ok().filter(|&n| n > 0);
        let address_ok = validate(&net.state.draft.normalized_url()).is_ok();
        if net.state.start_job(&send, address_ok, true, FORMATS, media) {
            Ok(())
        } else {
            Err(crate::i18n::tr(&self.ui.language, "the print job did not start").to_owned())
        }
    }

    /// Collect finished work (each frame), also while the dialog is closed: the status bar names the
    /// printer at each stage, and a job that ends (or fails) with the dialog gone is told in an alert.
    pub(crate) fn poll_network(&mut self) {
        self.poll_prepared();
        let was_running = self.net_print.state.draft.running();
        let ctx = self.egui_ctx.clone().unwrap_or_default();
        self.net_print.state.poll(&ctx);
        let labels = labels(&self.ui.language);
        let printer = if self.net_print.printer.is_empty() {
            crate::i18n::tr(&self.ui.language, "the printer").to_owned()
        } else {
            self.net_print.printer.clone()
        };
        let line = match &self.net_print.state.draft.job {
            net::JobView::Running { .. } => Some(fill(labels.sending, &printer)),
            net::JobView::Accepted { progress, .. } => {
                Some(format!("{}: {}", fill(labels.accepted, &printer), labels.stage_text(progress.stage, &printer)))
            }
            net::JobView::Finished(r) => Some(self.finished_text(&labels, r)),
            net::JobView::Failed(f) => Some(failed_text(&labels, f)),
            _ => None,
        };
        // Any open dialog (the Print dialog, or one the person opened since) is left alone: the
        // status line tells how the job ended.
        let dialog_open = self.ui.dialog.is_some();
        if !was_running && !self.net_print.state.draft.running() {
            return;
        }
        if let Some(line) = line {
            if self.net_print.shown.as_deref() != Some(line.as_str()) {
                self.net_print.shown = Some(line.clone());
                self.status(line.clone());
            }
            if was_running && !self.net_print.state.draft.running() && !dialog_open {
                let alert = crate::dialogs::Dialog::new("alert", json!({"title": crate::i18n::tr(&self.ui.language, "Print"), "message": line}));
                self.ui.dialog = Some(alert);
            }
        }
    }

    fn finished_text(&self, labels: &Labels<'_>, r: &Report) -> String {
        labels.outcome_text(r.outcome).map_or_else(|| fill(labels.sent, &r.printer), str::to_owned).replace("{printer}", &r.printer)
    }
}

fn failed_text(labels: &Labels<'_>, f: &Failure) -> String {
    let mut text = format!("{}: {}", labels.failure_heading(f.kind), f.detail);
    if f.outcome_unknown {
        text = format!("{text} {}", labels.outcome_unknown_warning);
    }
    text
}

#[cfg(test)]
mod tests;
