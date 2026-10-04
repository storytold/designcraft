//! PDF/X-4: the output intent (our Generic CMYK profile), the PDF/X version in the document
//! information and XMP, written as an incremental update of krilla's PDF 1.6 output; and a
//! check of the requirements this exporter is responsible for.

use crate::transitions::{find, int_after, object, rfind};

/// Escape a PDF literal string.
fn lit(s: &str) -> String {
    let mut o = String::from("(");
    for c in s.chars() {
        if matches!(c, '(' | ')' | '\\') {
            o.push('\\');
        }
        o.push(c);
    }
    o.push(')');
    o
}

/// Turn krilla's output into PDF/X-4. `None` when the file isn't the simple kind this
/// understands (a classic cross-reference table and plain dictionaries).
pub fn make_pdfx4(pdf: &[u8], title: &str) -> Option<Vec<u8>> {
    let t_at = rfind(pdf, b"trailer")?;
    let trailer = String::from_utf8_lossy(&pdf[t_at..]).to_string();
    let size = int_after(&trailer, "/Size")?;
    let root = int_after(&trailer, "/Root")?;
    let prev = int_after(&trailer, "startxref")?;
    let info = int_after(&trailer, "/Info");
    let id = trailer.find("/ID").and_then(|i| trailer[i..].find(']').map(|j| trailer[i..i + j + 1].to_string()));
    let cat = String::from_utf8_lossy(&pdf[object(pdf, root)?]).to_string();
    let mut out = pdf.to_vec();
    if !out.ends_with(b"\n") {
        out.push(b'\n');
    }
    let mut entries: Vec<(usize, usize)> = Vec::new();
    let mut put = |out: &mut Vec<u8>, id: usize, body: &[u8]| {
        entries.push((id, out.len()));
        out.extend_from_slice(format!("{id} 0 obj\n").as_bytes());
        out.extend_from_slice(body);
        out.extend_from_slice(b"\nendobj\n");
    };
    // The output profile and the output intent.
    let icc = designcraft_color::cms::icc_out::generic_cmyk_icc();
    let (icc_id, oi_id) = (size, size + 1);
    let mut s = format!("<</N 4/Length {}>>\nstream\n", icc.len()).into_bytes();
    s.extend_from_slice(icc);
    s.extend_from_slice(b"\nendstream");
    put(&mut out, icc_id, &s);
    let cond = designcraft_color::cms::GENERIC_CMYK;
    put(
        &mut out,
        oi_id,
        format!(
            "<</Type/OutputIntent/S/GTS_PDFX/OutputConditionIdentifier{}/OutputCondition{}/Info{}/RegistryName(http://www.color.org)/DestOutputProfile {icc_id} 0 R>>",
            lit("Custom"),
            lit(cond),
            lit(cond)
        )
        .as_bytes(),
    );
    // The catalog with the intent.
    let close = cat.rfind(">>")?;
    let cat_new = format!("{}/OutputIntents[{oi_id} 0 R]{}", cat[..close].trim(), &cat[close..]);
    put(&mut out, root, cat_new.trim().as_bytes());
    // Document information: the PDF/X version and trapping state.
    let mut next = size + 2;
    let info_id = match info {
        Some(i) => {
            let body = String::from_utf8_lossy(&pdf[object(pdf, i)?]).to_string();
            let close = body.rfind(">>")?;
            put(&mut out, i, format!("{}/GTS_PDFXVersion(PDF/X-4)/Trapped/False{}", body[..close].trim(), &body[close..]).trim().as_bytes());
            i
        }
        None => {
            let i = next;
            next += 1;
            put(&mut out, i, format!("<</Title{}/GTS_PDFXVersion(PDF/X-4)/Trapped/False>>", lit(title)).as_bytes());
            i
        }
    };
    // XMP: the PDF/X identification schema.
    if let Some(meta) = int_after(&cat, "/Metadata") {
        let r = object(pdf, meta)?;
        let body = &pdf[r];
        let s0 = find(body, b"stream", 0)? + b"stream".len();
        let s0 = s0 + body[s0..].iter().take_while(|c| **c == b'\r' || **c == b'\n').count();
        let s1 = rfind(body, b"endstream")?;
        let xml = String::from_utf8_lossy(&body[s0..s1]).trim_end().to_string();
        let dict = String::from_utf8_lossy(&body[..find(body, b"stream", 0)?]).to_string();
        if dict.contains("/Filter") {
            return None;
        }
        let add = "<rdf:Description rdf:about=\"\" xmlns:pdfxid=\"http://www.npes.org/pdfx/ns/id/\"><pdfxid:GTS_PDFXVersion>PDF/X-4</pdfxid:GTS_PDFXVersion></rdf:Description>";
        let xml = xml.replacen("</rdf:RDF>", &format!("{add}</rdf:RDF>"), 1);
        let dict = {
            let i = dict.find("/Length")?;
            let rest = &dict[i + 7..];
            let rest = rest.trim_start();
            let end = rest.find(|c: char| !c.is_ascii_digit()).unwrap_or(rest.len());
            format!("{}/Length {}{}", &dict[..i], xml.len(), &rest[end..])
        };
        let mut b = dict.trim().as_bytes().to_vec();
        b.extend_from_slice(b"\nstream\n");
        b.extend_from_slice(xml.as_bytes());
        b.extend_from_slice(b"\nendstream");
        put(&mut out, meta, &b);
    }
    // Cross-reference section and trailer.
    let xref_at = out.len();
    entries.sort();
    let mut x = String::from("xref\n");
    for (oid, off) in &entries {
        x.push_str(&format!("{oid} 1\n{off:010} 00000 n\r\n"));
    }
    let id = id.unwrap_or_default();
    x.push_str(&format!("trailer\n<</Size {next}/Root {root} 0 R/Info {info_id} 0 R{id}/Prev {prev}>>\nstartxref\n{xref_at}\n%%EOF\n"));
    out.extend_from_slice(x.as_bytes());
    Some(out)
}

/// PDF/X-4 checks on an exported file: what's missing (empty = passes these checks).
pub fn check_pdfx4(pdf: &[u8]) -> Vec<String> {
    let mut issues = Vec::new();
    let text = String::from_utf8_lossy(pdf);
    if !pdf.starts_with(b"%PDF-1.") || pdf.get(7).is_none_or(|v| *v > b'6') {
        issues.push("the PDF version must be 1.6 or lower".into());
    }
    if !text.contains("/S/GTS_PDFX") {
        issues.push("no PDF/X output intent".into());
    }
    if !text.contains("/DestOutputProfile") {
        issues.push("the output intent has no destination profile".into());
    }
    if !text.contains("/GTS_PDFXVersion(PDF/X-4)") {
        issues.push("the document information doesn't name PDF/X-4".into());
    }
    if !text.contains("pdfxid:GTS_PDFXVersion>PDF/X-4<") {
        issues.push("the XMP metadata doesn't name PDF/X-4".into());
    }
    if text.contains("/Encrypt") {
        issues.push("the file is encrypted".into());
    }
    let pages = text.matches("/Type/Page/").count() + text.matches("/Type/Page>").count();
    let trims = text.matches("/TrimBox").count();
    if pages > 0 && trims < pages {
        issues.push("every page needs a trim box".into());
    }
    if text.contains("/Subtype/Type3") {
        issues.push("Type 3 fonts aren't allowed".into());
    }
    issues
}
