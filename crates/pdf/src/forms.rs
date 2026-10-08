//! Interactive PDF forms (Buttons and Forms: text fields, check boxes, combo and list boxes,
//! signature fields) and media (placed video and sound). krilla writes neither, so they're added
//! as an incremental update: widget and screen annotations on their pages, the page `/Annots`, the
//! catalog `/AcroForm`, and the media files embedded for rendition actions.

use std::fmt::Write as _;

use designcraft_doc::{Document, FieldKind, FormField, Item, MediaOptions, SpreadRef};
use designcraft_geom::Affine;

use crate::export::Sheet;
use crate::transitions::{int_after, object, rfind};

/// A field placed on an output page: (page index, PDF rectangle, field).
pub(crate) type Placed = (usize, [f64; 4], FormField);

/// A placed video or sound on an output page: (page index, PDF rectangle, file name, MIME type,
/// bytes, options).
pub(crate) type PlacedMedia = (usize, [f64; 4], String, String, std::sync::Arc<Vec<u8>>, MediaOptions);

/// Every shown item of the exported pages with its PDF rectangle (origin bottom left).
fn visit(doc: &Document, sheets: &[Sheet], mut f: impl FnMut(usize, [f64; 4], &Item)) {
    for (pi, sh) in sheets.iter().enumerate() {
        let Some(sp) = doc.spread(SpreadRef::Doc(sh.spread)) else { continue };
        let mut stack: Vec<(Affine, &std::sync::Arc<Item>)> = sp.items.iter().map(|it| (Affine::IDENTITY, it)).collect();
        while let Some((xf, it)) = stack.pop() {
            stack.extend(it.shown_children().map(|c| (xf * it.child_space(), c)));
            if it.hidden {
                continue;
            }
            let r = xf.transform_rect_bbox(it.bounds()).intersect(sh.bleed);
            if r.width() <= 0.0 || r.height() <= 0.0 {
                continue;
            }
            let h = sh.media.height();
            f(pi, [r.x0 - sh.media.x0, h - (r.y1 - sh.media.y0), r.x1 - sh.media.x0, h - (r.y0 - sh.media.y0)], it);
        }
    }
}

/// The form fields of every exported page.
pub(crate) fn collect(doc: &Document, sheets: &[Sheet]) -> Vec<Placed> {
    let mut out = Vec::new();
    visit(doc, sheets, |pi, r, it| {
        if let Some(f) = &it.form_field {
            out.push((pi, r, f.clone()));
        }
    });
    out
}

/// The video and sound of every exported page (those whose file is in the document).
pub(crate) fn collect_media(doc: &Document, sheets: &[Sheet]) -> Vec<PlacedMedia> {
    let mut out = Vec::new();
    visit(doc, sheets, |pi, r, it| {
        if let (Some(m), designcraft_doc::Content::Graphic(g)) = (&it.media, &it.content)
            && let Some(a) = doc.assets.get(&g.asset).filter(|a| !a.data.is_empty())
        {
            out.push((pi, r, a.name.clone(), a.mime.clone(), a.data.clone(), m.clone()));
        }
    });
    out
}

/// A PDF text string: literal for ASCII, UTF-16BE otherwise.
fn pdf_string(s: &str) -> String {
    if s.is_ascii() {
        let mut o = String::from("(");
        for c in s.chars() {
            if matches!(c, '(' | ')' | '\\') {
                o.push('\\');
            }
            o.push(c);
        }
        o.push(')');
        return o;
    }
    let mut o = String::from("<FEFF");
    for u in s.encode_utf16() {
        let _ = write!(o, "{u:04X}");
    }
    o.push('>');
    o
}

/// Add `fields` and `media` to `pdf` (page indices in output order). `None` when the file's
/// structure isn't the simple kind this understands.
pub(crate) fn add_fields(pdf: &[u8], fields: &[Placed], media: &[PlacedMedia]) -> Option<Vec<u8>> {
    if fields.is_empty() && media.is_empty() {
        return Some(pdf.to_vec());
    }
    let t_at = rfind(pdf, b"trailer")?;
    let trailer = String::from_utf8_lossy(&pdf[t_at..]).to_string();
    let size = int_after(&trailer, "/Size")?;
    let root = int_after(&trailer, "/Root")?;
    let prev = int_after(&trailer, "startxref")?;
    let info = int_after(&trailer, "/Info");
    let id = trailer.find("/ID").and_then(|i| trailer[i..].find(']').map(|j| trailer[i..i + j + 1].to_string()));
    let cat = String::from_utf8_lossy(&pdf[object(pdf, root)?]).to_string();
    let pages_id = int_after(&cat, "/Pages")?;
    let pages = String::from_utf8_lossy(&pdf[object(pdf, pages_id)?]).to_string();
    let kids_str = &pages[pages.find("/Kids")? + 5..];
    let kids_str = &kids_str[kids_str.find('[')? + 1..kids_str.find(']')?];
    let nums: Vec<usize> = kids_str.split_whitespace().filter_map(|t| t.parse().ok()).collect();
    let kids: Vec<usize> = nums.chunks(2).map(|c| c[0]).collect();

    let mut out = pdf.to_vec();
    if !out.ends_with(b"\n") {
        out.push(b'\n');
    }
    let mut entries: Vec<(usize, usize)> = Vec::new();
    let mut next = size;
    let mut put = |out: &mut Vec<u8>, id: usize, body: &[u8]| {
        entries.push((id, out.len()));
        out.extend_from_slice(format!("{id} 0 obj\n").as_bytes());
        out.extend_from_slice(body);
        out.extend_from_slice(b"\nendobj\n");
    };
    let mut per_page: Vec<Vec<usize>> = vec![Vec::new(); kids.len()];
    // Media: the file embedded, a rendition of it, and a screen annotation that plays it (on
    // click, and on page open when asked).
    for (page, r, name, mime, data, m) in media {
        let Some(&page_ref) = kids.get(*page) else { continue };
        let (file, spec, rendition, screen) = (next, next + 1, next + 2, next + 3);
        next += 4;
        let mut body = format!("<</Type/EmbeddedFile/Subtype/{}/Length {}>>\nstream\n", mime.replace('/', "#2F"), data.len()).into_bytes();
        body.extend_from_slice(data);
        body.extend_from_slice(b"\nendstream");
        put(&mut out, file, &body);
        put(&mut out, spec, format!("<</Type/Filespec/F{}/UF{}/EF<</F {file} 0 R>>>>", pdf_string(name), pdf_string(name)).as_bytes());
        // Play parameters: player controls, and a repeat count of 0 (forever) when looping.
        let params = format!("<</BE<</C {}/RC {}>>>>", m.controls, if m.looping { 0 } else { 1 });
        put(
            &mut out,
            rendition,
            format!(
                "<</Type/Rendition/S/MR/N{}/C<</Type/MediaClip/S/MCD/N{}/CT{}/D {spec} 0 R/P<</TF(TEMPACCESS)>>>>/P{params}>>",
                pdf_string(name),
                pdf_string(name),
                pdf_string(mime)
            )
            .as_bytes(),
        );
        let play = format!("<</S/Rendition/OP 0/AN {screen} 0 R/R {rendition} 0 R>>");
        let on_open = if m.play_on_page_load { format!("/AA<</PO{play}>>") } else { String::new() };
        put(
            &mut out,
            screen,
            format!(
                "<</Type/Annot/Subtype/Screen/Rect[{:.2} {:.2} {:.2} {:.2}]/P {page_ref} 0 R/F 4/T{}/A{play}{on_open}>>",
                r[0],
                r[1],
                r[2],
                r[3],
                pdf_string(name)
            )
            .as_bytes(),
        );
        per_page[*page].push(screen);
    }
    let font = next;
    if !fields.is_empty() {
        next += 1;
        put(&mut out, font, b"<</Type/Font/Subtype/Type1/BaseFont/Helvetica/Encoding/WinAnsiEncoding>>");
    }
    let mut all = Vec::new();
    for (page, r, f) in fields {
        let Some(&page_ref) = kids.get(*page) else { continue };
        let rect = format!("[{:.2} {:.2} {:.2} {:.2}]", r[0], r[1], r[2], r[3]);
        let (w, h) = (r[2] - r[0], r[3] - r[1]);
        let fs = if f.font_size > 0.0 { f.font_size } else { 0.0 };
        let common = format!(
            "/Type/Annot/Subtype/Widget/T{}/Rect{rect}/F 4/P {page_ref} 0 R/MK<</BC[0.45 0.45 0.45]/BG[1 1 1]>>/DA(/Helv {fs:.1} Tf 0 g)",
            pdf_string(&f.name)
        );
        let req = if f.required { 2 } else { 0 };
        let body = match f.kind {
            FieldKind::TextField => {
                let flags = req | if f.multiline { 4096 } else { 0 };
                format!("<<{common}/FT/Tx/Ff {flags}/V{}/DV{}>>", pdf_string(&f.value), pdf_string(&f.value))
            }
            FieldKind::CheckBox => {
                // Appearances: an empty box, and a box with a check mark.
                let border = format!("0.45 G 1 w 0.5 0.5 {:.2} {:.2} re S", (w - 1.0).max(0.0), (h - 1.0).max(0.0));
                let check = format!(
                    "{border} 0 g {:.2} w 1 J 1 j {:.2} {:.2} m {:.2} {:.2} l {:.2} {:.2} l S",
                    (w.min(h) * 0.12).max(0.8),
                    w * 0.22,
                    h * 0.52,
                    w * 0.42,
                    h * 0.28,
                    w * 0.78,
                    h * 0.76
                );
                let (on, off) = (next, next + 1);
                next += 2;
                for (oid, content) in [(on, check), (off, border)] {
                    put(
                        &mut out,
                        oid,
                        format!("<</Type/XObject/Subtype/Form/BBox[0 0 {w:.2} {h:.2}]/Length {}>>\nstream\n{content}\nendstream", content.len())
                            .as_bytes(),
                    );
                }
                let state = if f.value.is_empty() || f.value == "Off" { "Off" } else { "On" };
                format!("<<{common}/FT/Btn/Ff {req}/V/{state}/AS/{state}/AP<</N<</On {on} 0 R/Off {off} 0 R>>>>>>")
            }
            FieldKind::ComboBox | FieldKind::ListBox => {
                let flags = req | if f.kind == FieldKind::ComboBox { 131072 } else { 0 };
                let opts: Vec<String> = f.options.iter().map(|o| pdf_string(o)).collect();
                format!("<<{common}/FT/Ch/Ff {flags}/Opt[{}]/V{}>>", opts.join(" "), pdf_string(&f.value))
            }
            FieldKind::Signature => format!("<<{common}/FT/Sig>>"),
        };
        let fid = next;
        next += 1;
        put(&mut out, fid, body.as_bytes());
        per_page[*page].push(fid);
        all.push(fid);
    }
    // Pages list their widgets.
    for (k, ids) in per_page.iter().enumerate() {
        if ids.is_empty() {
            continue;
        }
        let refs: String = ids.iter().map(|i| format!(" {i} 0 R")).collect();
        let body = String::from_utf8_lossy(&pdf[object(pdf, kids[k])?]).to_string();
        let new = if let Some(a) = body.find("/Annots[") {
            let close = a + body[a..].find(']')?;
            format!("{}{refs}{}", &body[..close], &body[close..])
        } else if let Some(arr_id) = int_after(&body, "/Annots") {
            // An indirect array: re-state it with the widgets added.
            let arr = String::from_utf8_lossy(&pdf[object(pdf, arr_id)?]).to_string();
            let close = arr.rfind(']')?;
            put(&mut out, arr_id, format!("{}{refs}{}", arr[..close].trim(), &arr[close..]).trim().as_bytes());
            body.clone()
        } else {
            let close = body.rfind(">>")?;
            format!("{}/Annots[{}]{}", &body[..close], refs.trim(), &body[close..])
        };
        if new != body {
            put(&mut out, kids[k], new.trim().as_bytes());
        }
    }
    // The catalog's form.
    if !all.is_empty() {
        let refs: Vec<String> = all.iter().map(|i| format!("{i} 0 R")).collect();
        let close = cat.rfind(">>")?;
        let acro = format!("/AcroForm<</Fields[{}]/NeedAppearances true/DA(/Helv 0 Tf 0 g)/DR<</Font<</Helv {font} 0 R>>>>>>", refs.join(" "));
        put(&mut out, root, format!("{}{acro}{}", cat[..close].trim(), &cat[close..]).trim().as_bytes());
    }
    let xref_at = out.len();
    entries.sort();
    let mut x = String::from("xref\n");
    for (oid, off) in &entries {
        let _ = write!(x, "{oid} 1\n{off:010} 00000 n\r\n");
    }
    let info = info.map(|i| format!("/Info {i} 0 R")).unwrap_or_default();
    let id = id.unwrap_or_default();
    let _ = write!(x, "trailer\n<</Size {next}/Root {root} 0 R{info}{id}/Prev {prev}>>\nstartxref\n{xref_at}\n%%EOF\n");
    out.extend_from_slice(x.as_bytes());
    Some(out)
}
