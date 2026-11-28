//! Page and catalog updates find objects where the cross-reference table places them, never by
//! searching the bytes: a file attached to the PDF or a form value that contains `3 0 obj` must
//! not be taken for page 3.
use designcraft_pdf::{add_catalog_entries, add_page_entries, first_page_ref};

/// A PDF with a correct classic cross-reference table: a catalog, a page tree listing `kids`, and
/// `objects` (number, body) in that order.
fn pdf_with(kids: &str, objects: &[(usize, &str)]) -> Vec<u8> {
    let mut pdf = b"%PDF-1.7\n".to_vec();
    let mut offsets: Vec<(usize, usize)> = Vec::new();
    let tree = format!("<</Type/Pages/Kids[{kids}]/Count 1>>");
    for (id, body) in [(1, "<</Type/Catalog/Pages 2 0 R>>"), (2, tree.as_str())].into_iter().chain(objects.iter().copied()) {
        offsets.push((id, pdf.len()));
        pdf.extend_from_slice(format!("{id} 0 obj\n{body}\nendobj\n").as_bytes());
    }
    let size = offsets.iter().map(|(id, _)| id + 1).max().unwrap_or(1);
    let xref = pdf.len();
    pdf.extend_from_slice(format!("xref\n0 {size}\n0000000000 65535 f\r\n").as_bytes());
    for id in 1..size {
        match offsets.iter().find(|(n, _)| *n == id) {
            Some((_, at)) => pdf.extend_from_slice(format!("{at:010} 00000 n\r\n").as_bytes()),
            None => pdf.extend_from_slice(b"0000000000 65535 f\r\n"),
        }
    }
    pdf.extend_from_slice(format!("trailer\n<</Size {size}/Root 1 0 R>>\nstartxref\n{xref}\n%%EOF\n").as_bytes());
    pdf
}

fn update(pdf: &[u8], extras: &[Option<&str>]) -> Option<Vec<u8>> {
    add_page_entries(pdf, &extras.iter().map(|s| s.map(str::to_owned)).collect::<Vec<_>>())
}

fn appended<'a>(pdf: &[u8], out: &'a [u8]) -> &'a [u8] {
    assert!(out.starts_with(pdf));
    &out[pdf.len() + usize::from(!pdf.ends_with(b"\n"))..]
}

#[test]
fn a_page_is_found_where_the_table_says_not_where_its_header_last_appears() {
    // An attached file whose bytes hold a page header: the real page 3 is re-stated, not the
    // bytes inside the stream, which could carry anything (an /AA action, say).
    let attached = "<</Type/EmbeddedFile/Length 50>>\nstream\n\n3 0 obj\n<</Type/Page/OpenAction 9 0 R>>\nendobj\n\nendstream";
    let pdf = pdf_with("3 0 R", &[(3, "<</Type/Page/Marker/real>>"), (4, attached)]);
    let out = update(&pdf, &[Some("/A 1")]).unwrap();
    assert!(appended(&pdf, &out).starts_with(b"3 0 obj\n<</Type/Page/Marker/real/A 1>>\nendobj\nxref\n3 1\n"), "{}", String::from_utf8_lossy(&out));
    assert!(!out.windows(b"/OpenAction 9 0 R/A 1".len()).any(|w| w == b"/OpenAction 9 0 R/A 1"));
}

#[test]
fn the_newest_cross_reference_section_wins() {
    // One update after another: the second re-states the copy the first wrote, and the catalog
    // is found through the chain of sections too.
    let pdf = pdf_with("3 0 R", &[(3, "<</Type/Page/Marker/old>>")]);
    let once = update(&pdf, &[Some("/A 1")]).unwrap();
    let twice = update(&once, &[Some("/B 2")]).unwrap();
    assert!(appended(&once, &twice).starts_with(b"3 0 obj\n<</Type/Page/Marker/old/A 1/B 2>>\nendobj\nxref\n"));
    let cat = add_catalog_entries(&twice, "/PageMode/FullScreen").unwrap();
    assert!(appended(&twice, &cat).starts_with(b"1 0 obj\n<</Type/Catalog/Pages 2 0 R/PageMode/FullScreen>>\nendobj\nxref\n1 1\n"));
    assert_eq!(first_page_ref(&cat), Some(3));
}

#[test]
fn a_page_listed_twice_is_restated_once() {
    let pdf = pdf_with("3 0 R 3 0 R", &[(3, "<</Type/Page/Marker/one>>")]);
    let out = update(&pdf, &[Some("/A 1"), Some("/B 2")]).unwrap();
    let rest = appended(&pdf, &out);
    assert!(rest.starts_with(b"3 0 obj\n<</Type/Page/Marker/one/A 1>>\nendobj\nxref\n3 1\n"), "{}", String::from_utf8_lossy(rest));
    assert!(!rest.windows(2).any(|w| w == b"/B"));
}

#[test]
fn a_wrong_header_a_free_object_a_missing_page_and_no_table_are_refused() {
    // The table points at a header that isn't the object's.
    let mut pdf = pdf_with("3 0 R", &[(3, "<</Type/Page>>")]);
    let at = pdf.windows(7).position(|w| w == b"3 0 obj").unwrap();
    pdf[at] = b'8';
    assert_eq!(update(&pdf, &[Some("/A 1")]), None);
    // Page 7 has no entry; the object isn't a page.
    assert_eq!(update(&pdf_with("7 0 R", &[(3, "<</Type/Page>>")]), &[Some("/A 1")]), None);
    assert_eq!(update(&pdf_with("3 0 R", &[(3, "<</Type/Other>>")]), &[Some("/A 1")]), None);
    // No cross-reference table at startxref: nothing is searched for instead.
    let pdf = b"%PDF-1.7\n1 0 obj\n<</Type/Catalog/Pages 2 0 R>>\nendobj\n2 0 obj\n<</Type/Pages/Kids[3 0 R]/Count 1>>\nendobj\n3 0 obj\n<</Type/Page>>\nendobj\ntrailer\n<</Size 4/Root 1 0 R>>\nstartxref\n0\n%%EOF\n";
    assert_eq!(update(pdf, &[Some("/A 1")]), None);
    assert_eq!(add_catalog_entries(pdf, "/PageMode/FullScreen"), None);
    assert_eq!(first_page_ref(pdf), None);
    // Nothing to add: the file is returned as it is, whatever it is.
    assert_eq!(update(b"not a PDF", &[None, None]).unwrap(), b"not a PDF");
}

#[test]
fn every_page_of_a_long_file_is_updated() {
    let count = 4097;
    let kids = (3..3 + count).map(|id| format!("{id} 0 R ")).collect::<String>();
    let bodies: Vec<String> = (3..3 + count).map(|id| format!("<</Type/Page/Parent 2 0 R/MediaBox[0 0 10 10]/Marker {id}>>")).collect();
    let objects: Vec<(usize, &str)> = bodies.iter().enumerate().map(|(i, b)| (i + 3, b.as_str())).collect();
    let pdf = pdf_with(&kids, &objects);
    let extras: Vec<Option<String>> = (3..3 + count).map(|id| Some(format!("/Tag {id}"))).collect();
    let out = add_page_entries(&pdf, &extras).unwrap();
    let mut remaining = appended(&pdf, &out);
    for id in 3..3 + count {
        let expected = format!("{id} 0 obj\n<</Type/Page/Parent 2 0 R/MediaBox[0 0 10 10]/Marker {id}/Tag {id}>>\nendobj\n");
        assert!(remaining.starts_with(expected.as_bytes()), "missing or changed page {id}");
        remaining = &remaining[expected.len()..];
    }
    assert!(remaining.starts_with(b"xref\n"));
    assert!(remaining.windows(b"4099 1\n".len()).any(|w| w == b"4099 1\n"));
}
