use std::time::Duration;

use craft_print::wsd::testing::{Ending, FakeConfig, FakePrinter, MemTransport};

use super::*;

const ADDRESS: &str = "http://127.0.0.1:5357/printer";

fn tiny_pdf() -> Vec<u8> {
    b"%PDF-1.4\n1 0 obj<</Type/Catalog/Pages 2 0 R>>endobj\n2 0 obj<</Type/Pages/Kids[3 0 R]/Count 1/MediaBox[0 0 200 200]>>endobj\n3 0 obj<</Type/Page/Parent 2 0 R>>endobj\ntrailer<</Root 1 0 R/Size 4>>\n%%EOF\n".to_vec()
}

fn open(config: FakeConfig) -> (Printer, Arc<FakePrinter>) {
    let fake = FakePrinter::new(config);
    let transport: Transport = Arc::new(MemTransport(Arc::clone(&fake)));
    (Printer::open(ADDRESS, Some(transport)).expect("opens"), fake)
}

fn consent() -> UserConfirmed {
    UserConfirmed::attest(Some(true)).expect("attested")
}

#[test]
fn the_protocol_follows_the_address() {
    for (address, protocol) in [
        ("http://192.168.1.5:5357/service", Protocol::Wsd),
        ("192.168.1.5:5357/ABCD", Protocol::Wsd),
        ("ipp://192.168.1.5/ipp/print", Protocol::Ipp),
        ("IPP://192.168.1.5", Protocol::Ipp),
        ("http://192.168.1.5:631/ipp/print", Protocol::Ipp),
        ("http://192.168.1.5:631/printers/ipp-lookalike", Protocol::Wsd),
    ] {
        assert_eq!(Protocol::of_address(address), protocol, "{address}");
    }
}

#[test]
fn the_policy_refuses_what_it_should_before_anything_is_sent() {
    for bad in ["http://printer.local/x", "http://8.8.8.8/", "ipp://printer.local/ipp", "ipps://10.0.0.1/ipp/print", "ipp://8.8.8.8/ipp/print"] {
        let error = Printer::open(bad, None).err().expect(bad);
        assert!(matches!(error.kind(), ErrorKind::InvalidRequest | ErrorKind::Unsupported), "{bad}: {error:?}");
    }
    assert!(Printer::open("ipp://192.168.1.50/ipp/print", None).is_ok());
}

#[test]
fn info_reads_the_printer_and_submit_sends_the_pdf_as_it_is() {
    let (printer, fake) = open(FakeConfig::default());
    assert_eq!(printer.protocol(), Protocol::Wsd);
    let info = printer.info().expect("info");
    assert!(info.formats.iter().any(|f| f == "application/pdf"));
    assert_eq!(info.sides.as_deref(), Some(&["OneSided".to_owned(), "TwoSidedLongEdge".to_owned()][..]));
    assert_eq!(info.qualities, None, "a list the printer left out is unknown, not empty");
    assert_eq!(info.colour, Some(true));
    assert_eq!(fake.jobs(), 0, "asking sends no job");

    let pdf = tiny_pdf();
    let cancel = CancelToken::new();
    let options = JobOptions::new().with_expected_pages(1);
    let job = Job { title: "Layout".into(), copies: 2, ..Job::default() };
    let sent = printer.submit(Document::pdf(&pdf), job, options, consent(), &cancel, &mut |_| {}).expect("accepted");
    assert_eq!(sent.format, "application/pdf");
    assert!(sent.conversion.is_none());
    assert_eq!(sent.bytes_sent, pdf.len());
    assert_eq!(fake.jobs(), 1);
    let stored = fake.job(1).expect("stored");
    assert_eq!(stored.document.as_deref(), Some(pdf.as_slice()));
    assert_eq!(stored.name, "Layout");

    let mut watch = printer.watch(&sent, WaitPolicy::default().with_max_wait(Duration::from_secs(10))).expect("watch");
    let outcome = loop {
        match watch.tick(printer.source()) {
            WatchStep::Done(outcome) => break outcome,
            WatchStep::Pending { next_in } => std::thread::sleep(next_in.min(Duration::from_millis(20))),
            _ => {}
        }
    };
    assert_eq!(outcome_name(&outcome), "completed");
}

#[test]
fn a_printer_without_pdf_gets_pwg_raster_made_from_the_pdf() {
    register_pdf_converter();
    let (printer, fake) = open(FakeConfig { formats: vec!["image/pwg-raster"], resolutions: vec![(150, 150)], ..FakeConfig::default() });
    let pdf = tiny_pdf();
    let sent = printer.submit(Document::pdf(&pdf), Job::default(), JobOptions::new(), consent(), &CancelToken::new(), &mut |_| {}).expect("accepted");
    assert_eq!(sent.format, "image/pwg-raster");
    let conversion = sent.conversion.expect("converted");
    assert_eq!((conversion.from, conversion.to), (DocumentFormat::Pdf, DocumentFormat::PwgRaster));
    let stored = fake.job(1).expect("stored").document.expect("document");
    assert!(stored.starts_with(b"RaS2"), "a PWG raster stream");
}

#[test]
fn a_printer_that_lists_nothing_we_make_is_refused_before_a_job_exists() {
    let (printer, fake) = open(FakeConfig { formats: vec!["image/jpeg"], ..FakeConfig::default() });
    let error = printer
        .submit(Document::pdf(&tiny_pdf()), Job::default(), JobOptions::new(), consent(), &CancelToken::new(), &mut |_| {})
        .expect_err("refused");
    assert_eq!(error.kind(), ErrorKind::UnsupportedFormat);
    assert_eq!(fake.jobs(), 0);
}

#[test]
fn a_cancel_before_the_start_sends_nothing() {
    let (printer, fake) = open(FakeConfig::default());
    let cancel = CancelToken::new();
    cancel.cancel();
    let error =
        printer.submit(Document::pdf(&tiny_pdf()), Job::default(), JobOptions::new(), consent(), &cancel, &mut |_| {}).expect_err("cancelled");
    assert_eq!(error.kind(), ErrorKind::Cancelled);
    assert_eq!(fake.jobs(), 0);
}

#[test]
fn an_aborted_job_is_reported_as_aborted() {
    let (printer, _) = open(FakeConfig { ending: Ending::Aborts("PrinterStopped"), ..FakeConfig::default() });
    let sent =
        printer.submit(Document::pdf(&tiny_pdf()), Job::default(), JobOptions::new(), consent(), &CancelToken::new(), &mut |_| {}).expect("accepted");
    let mut watch = printer.watch(&sent, WaitPolicy::default().with_max_wait(Duration::from_secs(10))).expect("watch");
    let outcome = loop {
        match watch.tick(printer.source()) {
            WatchStep::Done(outcome) => break outcome,
            WatchStep::Pending { next_in } => std::thread::sleep(next_in.min(Duration::from_millis(20))),
            _ => {}
        }
    };
    assert_eq!(outcome_name(&outcome), "aborted");
}

#[test]
fn ipp_attributes_become_the_dialog_tokens() {
    let attrs = ipp::PrinterAttrs {
        name: Some("Office".into()),
        state: Some(3),
        formats: vec!["application/pdf".into(), "image/pwg-raster".into()],
        sides: Some(vec!["one-sided".into(), "two-sided-long-edge".into(), "flip".into()]),
        orientations: Some(vec![3, 4]),
        qualities: Some(vec![3, 4, 5]),
        color_modes: Some(vec!["monochrome".into(), "color".into()]),
        media: Some(vec!["iso_a4_210x297mm".into(), "na_letter_8.5x11in".into(), "tray-1".into(), "auto".into()]),
        copies: Some((1, 99)),
        resolutions: Some(vec![(600, 600)]),
        ..ipp::PrinterAttrs::default()
    };
    let info = ipp_info(&attrs, "ipp://10.0.0.2/ipp/print");
    assert_eq!(info.name, "Office");
    assert_eq!(info.state, "Idle");
    assert_eq!(info.sides, Some(vec!["OneSided".into(), "TwoSidedLongEdge".into()]));
    assert_eq!(info.orientations, Some(vec!["Portrait".into(), "Landscape".into()]));
    assert_eq!(info.qualities, Some(vec!["Draft".into(), "Normal".into(), "High".into()]));
    assert_eq!(info.media, Some(vec!["iso_a4_210x297mm".into(), "na_letter_8.5x11in".into()]));
    assert_eq!(info.colour, Some(true));
    assert_eq!(info.copies, Some((1, 99)));
    // Published as one-sided only: a two-sided request is dropped, not unknown.
    let simplex = ipp_info(&ipp::PrinterAttrs { sides: Some(vec!["one-sided".into()]), ..ipp::PrinterAttrs::default() }, "ipp://10.0.0.2/ipp");
    assert_eq!(simplex.sides, Some(vec!["OneSided".into()]));
    assert_eq!(simplex.name, "ipp://10.0.0.2/ipp");
    let known = known_from("ipp:x", &info);
    assert_eq!(known.details.two_sided, Some(true));
    assert_eq!(known.details.formats, [DocumentFormat::Pdf, DocumentFormat::PwgRaster]);
}
