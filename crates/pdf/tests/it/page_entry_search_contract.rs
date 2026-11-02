//! Independent literal-search contract tests. These pass on the old implementation too:
//! this optimization must preserve the current deliberately narrow PDF handling.
use designcraft_pdf::add_page_entries;

fn fixture(kids: &str, objects: &[u8]) -> Vec<u8> {
    let mut pdf =
        format!("%PDF-1.7\n1 0 obj\n<</Type/Catalog/Pages 2 0 R>>\nendobj\n2 0 obj\n<</Type/Pages/Kids[{kids}]/Count 2>>\nendobj\n").into_bytes();
    pdf.extend_from_slice(objects);
    pdf.extend_from_slice(b"\ntrailer\n<</Size 10/Root 1 0 R>>\nstartxref\n0\n%%EOF\n");
    pdf
}
fn old_page() -> &'static [u8] {
    b"3 0 obj\n<</Type/Page/Marker/old>>\nendobj\n"
}
fn update(pdf: &[u8], extras: &[Option<&str>]) -> Option<Vec<u8>> {
    add_page_entries(pdf, &extras.iter().map(|s| s.map(str::to_owned)).collect::<Vec<_>>())
}
fn appended<'a>(pdf: &[u8], out: &'a [u8]) -> &'a [u8] {
    assert!(out.starts_with(pdf));
    &out[pdf.len() + usize::from(!pdf.ends_with(b"\n"))..]
}

#[test]
fn newest_copy_is_selected_and_duplicates_keep_request_order() {
    let mut objects = old_page().to_vec();
    objects.extend_from_slice(b"3 0 obj\n<</Type/Page/Marker/new>>\nendobj\n");
    let pdf = fixture("3 0 R 3 0 R", &objects);
    let out = update(&pdf, &[Some("/A 1"), Some("/B 2")]).unwrap();
    assert!(
        appended(&pdf, &out).starts_with(b"3 0 obj\n<</Type/Page/Marker/new/A 1>>\nendobj\n3 0 obj\n<</Type/Page/Marker/new/B 2>>\nendobj\nxref\n")
    );
}

#[test]
fn embedded_header_and_object_suffix_are_literal_matches() {
    let mut objects = old_page().to_vec();
    objects.extend_from_slice(b"8 0 obj\nstream\n3 0 object<</Type/Page/Marker/embedded>>endobj\nendstream\nendobj\n");
    let pdf = fixture("3 0 R", &objects);
    let out = update(&pdf, &[Some("/A 1")]).unwrap();
    assert!(appended(&pdf, &out).starts_with(b"3 0 object<</Type/Page/Marker/embedded/A 1>>endobj\nxref\n"));
}

#[test]
fn latest_unterminated_match_does_not_fall_back() {
    let mut pdf = fixture("3 0 R", old_page());
    pdf.extend_from_slice(b"3 0 obj<</Type/Page/Marker/unterminated>>");
    assert_eq!(update(&pdf, &[Some("/A 1")]), None);
}

#[test]
fn latest_nonpage_and_missing_requested_page_are_rejected() {
    let mut objects = old_page().to_vec();
    objects.extend_from_slice(b"3 0 obj<</Type/Other>>endobj\n");
    assert_eq!(update(&fixture("3 0 R", &objects), &[Some("/A 1")]), None);
    assert_eq!(update(&fixture("7 0 R", old_page()), &[Some("/A 1")]), None);
}

#[test]
fn only_canonical_decimal_and_exact_generation_spacing_match() {
    for header in ["03 0 obj", "+3 0 obj", "3 1 obj", "3  0 obj", "3\t0 obj", "3 0\tobj", "184467440737095516160 0 obj"] {
        let mut objects = old_page().to_vec();
        objects.extend_from_slice(format!("{header}<</Type/Page/Marker/wrong>>endobj\n").as_bytes());
        let pdf = fixture("3 0 R", &objects);
        let out = update(&pdf, &[Some("/A 1")]).unwrap();
        assert!(appended(&pdf, &out).starts_with(b"3 0 obj\n<</Type/Page/Marker/old/A 1>>\nendobj\nxref\n"), "{header}");
    }
}

#[test]
fn crlf_matches_but_bare_cr_and_byte_zero_do_not() {
    let mut objects = old_page().to_vec();
    objects.extend_from_slice(b"\r\n3 0 obj<</Type/Page/Marker/crlf>>endobj\r3 0 obj<</Type/Page/Marker/barecr>>endobj\n");
    let pdf = fixture("3 0 R", &objects);
    let out = update(&pdf, &[Some("/A 1")]).unwrap();
    assert!(appended(&pdf, &out).starts_with(b"3 0 obj<</Type/Page/Marker/crlf/A 1>>endobj\nxref\n"));
    let mut pdf = old_page().to_vec();
    pdf.extend_from_slice(&fixture("3 0 R", b""));
    assert_eq!(update(&pdf, &[Some("/A 1")]), None);
}

#[test]
fn first_subsequent_endobj_and_lossy_body_are_preserved() {
    let pdf = fixture("3 0 R", b"3 0 obj<</Type/Page/Marker/\xff>>endobj IGNORE endobj\n");
    let out = update(&pdf, &[Some("/A 1")]).unwrap();
    assert!(appended(&pdf, &out).starts_with("3 0 obj<</Type/Page/Marker/\u{fffd}/A 1>>endobj\nxref\n".as_bytes()));
}

#[test]
fn absent_extras_do_not_require_objects_and_out_of_range_requests_are_ignored() {
    let malformed = b"not a PDF";
    assert_eq!(update(malformed, &[]).unwrap(), malformed);
    assert_eq!(update(malformed, &[None, None]).unwrap(), malformed);
    let pdf = fixture("7 0 R 3 0 R", old_page());
    let out = update(&pdf, &[None, Some("/A 1"), Some("/Ignored 1")]).unwrap();
    assert!(appended(&pdf, &out).starts_with(b"3 0 obj\n<</Type/Page/Marker/old/A 1>>\nendobj\nxref\n"));
    assert!(!out.windows(b"/Ignored".len()).any(|w| w == b"/Ignored"));
}

#[test]
fn zero_and_maximum_usize_ids_are_supported_without_index_sizing_by_id() {
    for id in [0, usize::MAX] {
        let pdf = fixture(&format!("{id} 0 R"), format!("{id} 0 obj<</Type/Page/Marker/extreme>>endobj\n").as_bytes());
        let out = update(&pdf, &[Some("/A 1")]).unwrap();
        assert!(appended(&pdf, &out).starts_with(format!("{id} 0 obj<</Type/Page/Marker/extreme/A 1>>endobj\nxref\n").as_bytes()));
    }
}

/// A complete, blank PDF with correct classic xref offsets and 4,097 distinct pages.
/// This deliberately exceeds the optional index's documented 4,096-entry budget.
pub(crate) fn over_index_budget_fixture() -> (Vec<u8>, Vec<Option<String>>) {
    let count = 4097;
    let size = count + 3;
    let kids = (3..size).map(|id| format!("{id} 0 R ")).collect::<String>();
    let mut pdf = b"%PDF-1.7\n".to_vec();
    let mut offsets = vec![0usize];
    for (id, body) in [(1, "<</Type/Catalog/Pages 2 0 R>>".to_owned()), (2, format!("<</Type/Pages/Kids[{kids}]/Count {count}>>"))] {
        offsets.push(pdf.len());
        pdf.extend_from_slice(format!("{id} 0 obj\n{body}\nendobj\n").as_bytes());
    }
    for id in 3..size {
        offsets.push(pdf.len());
        pdf.extend_from_slice(format!("{id} 0 obj\n<</Type/Page/Parent 2 0 R/MediaBox[0 0 10 10]/Marker {id}>>\nendobj\n").as_bytes());
    }
    let xref = pdf.len();
    pdf.extend_from_slice(format!("xref\n0 {size}\n0000000000 65535 f\r\n").as_bytes());
    for offset in offsets.iter().skip(1) {
        pdf.extend_from_slice(format!("{offset:010} 00000 n\r\n").as_bytes());
    }
    pdf.extend_from_slice(format!("trailer\n<</Size {size}/Root 1 0 R>>\nstartxref\n{xref}\n%%EOF\n").as_bytes());
    let extras = (3..size).map(|id| Some(format!("/Tag {id}"))).collect();
    (pdf, extras)
}

#[test]
fn index_budget_fallback_preserves_every_requested_page_update() {
    let (pdf, extras) = over_index_budget_fixture();
    assert_eq!(extras.len(), 4097);
    let out = add_page_entries(&pdf, &extras).unwrap();
    let mut remaining = appended(&pdf, &out);
    for id in 3..4100 {
        let expected = format!("{id} 0 obj\n<</Type/Page/Parent 2 0 R/MediaBox[0 0 10 10]/Marker {id}/Tag {id}>>\nendobj\n");
        assert!(remaining.starts_with(expected.as_bytes()), "missing or changed page {id}");
        remaining = &remaining[expected.len()..];
    }
    assert!(remaining.starts_with(b"xref\n"));
    assert!(remaining.windows(b"4099 1\n".len()).any(|w| w == b"4099 1\n"));
}
