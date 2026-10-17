//! Network printing on `craft-print`'s registry: WS-Print (`wsd`) and IPP (`ipp`) printers the
//! person adds by address.
//!
//! * [`Printer`]: one printer the person chose, behind a private [`craft_print::Registry`] that holds
//!   one backend with only that printer added. [`Printer::info`] asks what it is ([`PrinterInfo`],
//!   no job sent), [`Printer::submit`] sends a document and returns when the printer has *accepted*
//!   it ([`Sent`]; it never waits for the paper), [`Printer::watch`] follows the accepted job.
//! * **Protocol by address.** `ipp://` (or a path that contains an `ipp` segment) is IPP; every other
//!   address is WS-Print ([`Protocol::of_address`]). Both backends go through the same `Registry`
//!   calls, so the rest of the app does not care which one it talks to.
//! * **No retry of an unknown outcome.** Nothing here sends a document twice. A failure whose
//!   [`PrintError::outcome_unknown`] is true may have created a job: the caller tells the person to
//!   check the printer, and (WS-Print) [`Printer::reconcile`] reads the printer's job list to find out.
//! * **Every job needs a [`UserConfirmed`]**, which the type of [`Printer::submit`] requires.
//! * **Formats.** A document in a format the printer lists is sent as it is; any other goes through
//!   the registered [`DocumentConverter`] ([`PdfToRaster`]: a print-ready PDF becomes PWG raster).

use std::sync::{Arc, Mutex, PoisonError, RwLock};

use craft_print::ipp::{self, IppBackend};
pub use craft_print::wsd;
use craft_print::wsd::error::Op;
use craft_print::wsd::{Client, PrinterUrl, Reconciled, WsdBackend};
pub use craft_print::{
    CancelToken, ColourMode, Conversion, ConversionPolicy, ConvertContext, Document, DocumentConverter, DocumentFormat, Duplex, ErrorKind, Job,
    JobId, JobOptions, JobSource, JobState, JobStatus, JobWatch, Orientation, OwnedDocument, Phase, PrintError, PrinterDetails, Progress, Quality,
    Registry, Resolution, Scaling, Submitted, UserConfirmed, WaitPolicy, WatchOutcome, WatchStep, pwg,
};
use craft_print::{SubmitRequest, namespaced_id};

mod pdf;
pub use pdf::PdfToRaster;

/// A transport shared with worker threads: the plain-HTTP one of craft-wsd, or an in-memory fake.
pub type Transport = Arc<dyn wsd::adapter::Transport + Send + Sync>;

/// Which protocol an address speaks.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Protocol {
    Wsd,
    Ipp,
}

impl Protocol {
    /// `ipp://...` and addresses whose path has an `ipp` segment (`http://192.168.1.5:631/ipp/print`)
    /// are IPP; anything else is WS-Print.
    pub fn of_address(address: &str) -> Self {
        let a = address.trim().to_ascii_lowercase();
        let rest = a.split_once("://").map_or(a.as_str(), |(_, rest)| rest);
        let path = rest.find('/').and_then(|i| rest.get(i..)).unwrap_or("");
        if a.starts_with("ipp://") || path.split('/').any(|s| s == "ipp") { Self::Ipp } else { Self::Wsd }
    }

    /// The registry name of the backend (the prefix of its ids).
    pub const fn backend(self) -> &'static str {
        match self {
            Self::Wsd => "wsd",
            Self::Ipp => "ipp",
        }
    }

    pub const fn label(self) -> &'static str {
        match self {
            Self::Wsd => "WS-Print",
            Self::Ipp => "IPP",
        }
    }
}

/// Check an address against the network policy of its protocol (numeric loopback, link-local or private
/// address; no host names, no TLS, no credentials) without opening anything.
pub fn validate_address(address: &str) -> Result<(), PrintError> {
    match Protocol::of_address(address) {
        Protocol::Wsd => PrinterUrl::parse(address).map(|_| ()).map_err(PrintError::from),
        Protocol::Ipp => ipp::PrinterUri::parse(address).map(|_| ()).map_err(PrintError::from),
    }
}

/// The converter every [`Printer`] opened afterwards registers on its registry.
static CONVERTER: RwLock<Option<Arc<dyn DocumentConverter>>> = RwLock::new(None);

/// Register the converter the app brings (replaces an earlier one). Done once, where network
/// printing is set up.
pub fn register_converter(converter: Arc<dyn DocumentConverter>) {
    *CONVERTER.write().unwrap_or_else(PoisonError::into_inner) = Some(converter);
}

/// Register [`PdfToRaster`] unless a converter is already registered. Safe to call from every
/// start-up path.
pub fn register_pdf_converter() {
    let mut slot = CONVERTER.write().unwrap_or_else(PoisonError::into_inner);
    if slot.is_none() {
        *slot = Some(Arc::new(PdfToRaster));
    }
}

fn registered_converter() -> Option<Arc<dyn DocumentConverter>> {
    CONVERTER.read().unwrap_or_else(PoisonError::into_inner).clone()
}

/// What a printer said about itself, whichever protocol it speaks. A list that is `None` was not
/// published (unknown); `Some(vec![])` was published empty (unsupported). WS-Print cannot say
/// "empty", so its lists are `None` or non-empty. Values are the tokens
/// `craft_ui::printing::network` uses (`OneSided`, `TwoSidedLongEdge`, `Portrait`, `Draft`; media
/// as PWG names).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct PrinterInfo {
    pub name: String,
    pub colour: Option<bool>,
    /// `Idle`, `Processing`, `Stopped` or a vendor value; empty when not reported.
    pub state: String,
    pub queued_jobs: Option<u32>,
    pub reasons: Vec<String>,
    /// The document formats as the printer spells them.
    pub formats: Vec<String>,
    pub sides: Option<Vec<String>>,
    pub orientations: Option<Vec<String>>,
    pub qualities: Option<Vec<String>>,
    pub media: Option<Vec<String>>,
    pub resolutions: Vec<(u32, u32)>,
    pub copies: Option<(u32, u32)>,
}

/// What a printer accepted: its job and what was sent.
#[derive(Clone, Debug)]
pub struct Sent {
    /// The registry's own record of the acceptance (message, warnings, what it can do afterwards).
    pub submitted: Submitted,
    /// The accepted job (`wsd:<printer url>#<number>` or `ipp:<printer uri>#<number>`).
    pub job: JobId,
    /// The format the document was sent in, as the printer spells it.
    pub format: String,
    /// What the registry converted, when the document was not in a format the printer lists.
    pub conversion: Option<Conversion>,
    pub bytes_sent: usize,
    pub printer_name: String,
    /// Settings that were dropped or changed because the printer does not list them.
    pub warnings: Vec<String>,
}

/// What a read of the printer left: the details the registry takes, the formats as the printer
/// spells them and its name.
#[derive(Clone)]
struct Known {
    details: PrinterDetails,
    listed: Vec<String>,
    name: Option<String>,
}

enum Wire {
    Wsd { client: Client<Transport>, backend: WsdBackend, url: PrinterUrl },
    Ipp { client: ipp::Client<Transport> },
}

/// One network printer: checked address, transport and the registry that follows its jobs.
pub struct Printer {
    protocol: Protocol,
    address: String,
    wire: Wire,
    registry: Registry,
    known: Mutex<Option<Known>>,
}

impl Printer {
    /// A printer at `address` (`http://` or `ipp://` and a numeric loopback, link-local or private
    /// IP address: craft-wsd's policy decides, and a refusal is [`ErrorKind::InvalidRequest`]).
    /// `transport` replaces the plain-HTTP one (tests, embedding). Opens no connection.
    pub fn open(address: &str, transport: Option<Transport>) -> Result<Self, PrintError> {
        let protocol = Protocol::of_address(address);
        let transport: Transport = transport.unwrap_or_else(wsd::adapter::default_transport);
        let registry = Registry::new();
        let (wire, id) = match protocol {
            Protocol::Wsd => {
                let backend = WsdBackend::new().with_transport(Arc::clone(&transport));
                let id = backend.add(address)?;
                let url = PrinterUrl::parse(&id).map_err(PrintError::from)?;
                registry.register(Box::new(backend.clone())).map_err(|e| PrintError::new(ErrorKind::Failed, e.to_string()))?;
                (Wire::Wsd { client: Client::new(transport, url.clone()), backend, url }, id)
            }
            Protocol::Ipp => {
                let backend = IppBackend::new().with_transport(Arc::clone(&transport));
                let id = backend.add(address)?;
                let uri = ipp::PrinterUri::parse(&id).map_err(PrintError::from)?;
                registry.register(Box::new(backend)).map_err(|e| PrintError::new(ErrorKind::Failed, e.to_string()))?;
                (Wire::Ipp { client: ipp::Client::new(transport, uri) }, id)
            }
        };
        if let Some(converter) = registered_converter() {
            registry.set_converter(converter);
        }
        Ok(Self { protocol, address: id, wire, registry, known: Mutex::new(None) })
    }

    pub fn protocol(&self) -> Protocol {
        self.protocol
    }

    /// The canonical address.
    pub fn address(&self) -> String {
        self.address.clone()
    }

    /// Use `converter` for this printer instead of the registered one (tests, embedding).
    pub fn set_converter(&self, converter: Arc<dyn DocumentConverter>) {
        self.registry.set_converter(converter);
    }

    /// The registry's choices around conversion (preferred target formats, the resolution to
    /// approach).
    pub fn set_conversion_policy(&self, policy: ConversionPolicy) {
        self.registry.set_conversion_policy(policy);
    }

    /// What the printer is and what it accepts. Sends no job. The answer is kept: [`Self::submit`]
    /// uses it instead of reading the printer again.
    pub fn info(&self) -> Result<PrinterInfo, PrintError> {
        let info = match &self.wire {
            Wire::Wsd { client, .. } => wsd_info(wsd::printer_info(client).map_err(|e| e.print_error(Op::Query))?),
            Wire::Ipp { client } => ipp_info(&client.get_printer_attributes(&[]).map_err(PrintError::from)?, &self.address),
        };
        self.remember(&info);
        Ok(info)
    }

    /// Hand over the answer of an earlier read of this printer, so the next [`Self::submit`] reads
    /// nothing before the registry decides on the format.
    pub fn remember(&self, info: &PrinterInfo) {
        *self.known.lock().unwrap_or_else(PoisonError::into_inner) = Some(known_from(&self.address, info));
    }

    fn remembered(&self) -> Option<Known> {
        self.known.lock().unwrap_or_else(PoisonError::into_inner).clone()
    }

    /// Send `document` and return when the printer has accepted it. `job` and `options` go to the
    /// registry as they are; `progress` gets the protocol phases; `cancel` is polled between phases
    /// and cancelling after the job was created cancels it on the printer. An error says whether
    /// sending again is safe ([`PrintError::retry_safety`]). Set [`JobOptions::expected_pages`].
    pub fn submit(
        &self,
        document: Document<'_>,
        job: Job,
        options: JobOptions,
        consent: UserConfirmed,
        cancel: &CancelToken,
        progress: &mut dyn FnMut(Progress),
    ) -> Result<Sent, PrintError> {
        if cancel.is_cancelled() {
            return Err(PrintError::new(ErrorKind::Cancelled, "the print job was cancelled before it started"));
        }
        let known = match self.remembered() {
            Some(known) => known,
            None => {
                self.info()?;
                self.remembered().ok_or_else(|| PrintError::new(ErrorKind::Failed, "the printer's details were not kept"))?
            }
        };
        let (format, len) = (document.format, document.bytes.len());
        let job = Job { printer: Some(namespaced_id(self.protocol.backend(), &self.address)), ..job };
        let request =
            SubmitRequest::new(document, job).with_options(options).with_consent(consent).with_cancel(cancel.clone()).with_progress(progress);
        let submitted = self.registry.submit_job_with_details(request, Some(known.details.clone()))?;
        let job_id =
            submitted.job.clone().ok_or_else(|| PrintError::new(ErrorKind::Failed, "the printer accepted the document but gave no job handle"))?;
        let conversion = submitted.conversion.clone();
        let sent_as = conversion.as_ref().map_or(format, |c| c.to);
        let spelled =
            known.listed.iter().find(|l| DocumentFormat::from_mime(l) == Some(sent_as)).cloned().unwrap_or_else(|| sent_as.mime().to_owned());
        Ok(Sent {
            job: job_id,
            format: spelled,
            bytes_sent: conversion.as_ref().map_or(len, |c| c.bytes),
            conversion,
            printer_name: known.name.unwrap_or_else(|| self.address.clone()),
            warnings: submitted.warnings.clone(),
            submitted,
        })
    }

    /// A follower for an accepted job. Tick it with [`Self::source`].
    pub fn watch(&self, sent: &Sent, policy: WaitPolicy) -> Result<JobWatch, PrintError> {
        JobWatch::for_submitted(&sent.submitted, policy).ok_or_else(|| PrintError::new(ErrorKind::Failed, "the accepted job cannot be followed"))
    }

    /// Where a [`JobWatch`] of this printer asks for status and sends its cancel.
    pub fn source(&self) -> &dyn JobSource {
        &self.registry
    }

    /// One read of a job's status.
    pub fn status(&self, job: &JobId) -> Result<JobStatus, PrintError> {
        self.registry.job_status(job)
    }

    /// Ask the printer to cancel a job ("too late" is success).
    pub fn cancel(&self, job: &JobId) -> Result<(), PrintError> {
        self.registry.cancel_job(job)
    }

    /// After an error with [`PrintError::outcome_unknown`]: look for a job titled `title` on the
    /// printer (reads only), before anything is sent again. WS-Print only.
    pub fn reconcile(&self, title: &str) -> Result<Reconciled, PrintError> {
        match &self.wire {
            Wire::Wsd { backend, url, .. } => backend.reconcile(&url.as_url(), title),
            Wire::Ipp { .. } => Err(PrintError::new(ErrorKind::Unsupported, "reconcile is not offered for IPP printers")),
        }
    }
}

fn list(v: Vec<String>) -> Option<Vec<String>> {
    if v.is_empty() { None } else { Some(v) }
}

fn wsd_info(elements: wsd::PrinterElements) -> PrinterInfo {
    let description = elements.description.unwrap_or_default();
    let status = elements.status.unwrap_or_default();
    let caps = elements.capabilities.unwrap_or_default();
    PrinterInfo {
        name: description.name,
        colour: description.color_supported,
        state: status.state,
        queued_jobs: status.queued_jobs,
        reasons: status.reasons,
        formats: caps.formats,
        sides: list(caps.sides),
        orientations: list(caps.orientations),
        qualities: list(caps.qualities),
        media: list(caps.media_sizes),
        resolutions: caps.resolutions,
        copies: caps.copies,
    }
}

/// IPP keywords and enums as the tokens of `craft_ui::printing::network`; a value with no token is dropped.
fn tokens<T>(values: &Option<Vec<T>>, token: impl Fn(&T) -> Option<&'static str>) -> Option<Vec<String>> {
    values.as_ref().map(|v| v.iter().filter_map(&token).map(str::to_owned).collect())
}

fn ipp_info(a: &ipp::PrinterAttrs, address: &str) -> PrinterInfo {
    let name =
        [a.name.as_deref(), a.info.as_deref(), a.make_and_model.as_deref()].into_iter().flatten().find(|n| !n.trim().is_empty()).unwrap_or(address);
    let state = match a.state {
        Some(3) => "Idle",
        Some(4) => "Processing",
        Some(5) => "Stopped",
        _ => "",
    };
    let sides = tokens(&a.sides, |s| match s.as_str() {
        "one-sided" => Some("OneSided"),
        "two-sided-long-edge" => Some("TwoSidedLongEdge"),
        "two-sided-short-edge" => Some("TwoSidedShortEdge"),
        _ => None,
    });
    let orientations = tokens(&a.orientations, |o| match o {
        3 => Some("Portrait"),
        4 => Some("Landscape"),
        5 => Some("ReverseLandscape"),
        6 => Some("ReversePortrait"),
        _ => None,
    });
    let qualities = tokens(&a.qualities, |q| match q {
        3 => Some("Draft"),
        4 => Some("Normal"),
        5 => Some("High"),
        _ => None,
    });
    let mono = |c: &String| matches!(c.as_str(), "monochrome" | "bi-level" | "auto-monochrome");
    let colour = a.color_supported.or_else(|| a.color_modes.as_ref().map(|m| m.iter().any(|c| !mono(c))));
    PrinterInfo {
        name: name.to_owned(),
        colour,
        state: state.to_owned(),
        queued_jobs: None,
        reasons: a.state_reasons.iter().filter(|r| r.as_str() != "none").cloned().collect(),
        formats: a.formats.clone(),
        sides,
        orientations,
        qualities,
        // `media-supported` also lists trays and types: only the self-describing size names are sizes.
        media: a.media.clone().map(|m| m.into_iter().filter(|n| n.contains('_') && n.contains('x')).collect()),
        resolutions: a.resolutions.clone().unwrap_or_default(),
        copies: a.copies.and_then(|(lo, hi)| Some((u32::try_from(lo).ok()?, u32::try_from(hi).ok()?))),
    }
}

/// The registry's view of a printer, from what it published: the formats craft-print has a name
/// for (in the printer's order), colour, paper, copies and the listed resolutions.
fn known_from(id: &str, info: &PrinterInfo) -> Known {
    let name = Some(info.name.clone()).filter(|n| !n.is_empty());
    let mut details =
        PrinterDetails { id: id.to_owned(), name: name.clone().unwrap_or_else(|| id.to_owned()), colour: info.colour, ..PrinterDetails::default() };
    let mut listed = Vec::new();
    for spelled in &info.formats {
        if let Some(format) = DocumentFormat::from_mime(spelled)
            && !details.formats.contains(&format)
        {
            details.formats.push(format);
            listed.push(spelled.clone());
        }
    }
    details.two_sided = info.sides.as_ref().map(|s| s.iter().any(|s| s.starts_with("TwoSided")));
    details.paper = info.media.clone().unwrap_or_default();
    details.max_copies = info.copies.map(|(_, max)| max);
    details.resolutions = info.resolutions.iter().map(|&(x, y)| Resolution::new(x, y)).collect();
    Known { details, listed, name }
}

/// A stable lowercase name for how following a job ended, for tool results and status lines.
pub fn outcome_name(outcome: &WatchOutcome) -> &'static str {
    match outcome {
        WatchOutcome::Terminal(status) => match status.state {
            // craft-print models "completed with warnings" as Completed plus warnings on the status.
            JobState::Completed if !status.warnings.is_empty() => "completed_with_warnings",
            JobState::Completed => "completed",
            JobState::CompletedWithErrors => "completed_with_errors",
            JobState::Cancelled => "canceled",
            JobState::Aborted => "aborted",
            _ => "unknown",
        },
        WatchOutcome::Forgotten { .. } => "forgotten",
        WatchOutcome::NoStatus => "accepted_unknown",
        WatchOutcome::TimedOut { .. } => "still_running",
        WatchOutcome::Failed(_) => "failed",
        _ => "unknown",
    }
}

/// What the person is told when a send failed and the printer may nevertheless have the document.
pub const CHECK_THE_PRINTER: &str = "the request may have reached the printer: check the printer before sending again";

#[cfg(test)]
mod tests;
